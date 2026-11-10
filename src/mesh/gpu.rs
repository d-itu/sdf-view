use std::borrow::Cow;

use futures::{channel::oneshot, executor::block_on};
use marching_cubes::tables::TRI_TABLE;
use wgpu::util::DeviceExt;

use super::{CORNERS, EDGES, MeshOptions};
use crate::{Error, MeshError};

const BRICK: u32 = 32;
// Bound scratch memory while amortizing synchronous count readback.
const BATCH: usize = 8;
const SAMPLE_BYTES: u64 = 35 * 35 * 35 * 4;
const CELL_BYTES: u64 = 32 * 32 * 32 * 16;
const EDGE_BYTES: u64 = 3 * 33 * 33 * 33 * 4;
const STATUS_BYTES: u64 = 12;

pub(super) struct Chunk {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub count: u32,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

struct Slot {
    params: wgpu::Buffer,
    samples: wgpu::Buffer,
    cells: wgpu::Buffer,
    groups: wgpu::Buffer,
    status: wgpu::Buffer,
    edges: wgpu::Buffer,
    sampling: wgpu::BindGroup,
    compute: wgpu::BindGroup,
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffers: &[&wgpu::Buffer],
) -> wgpu::BindGroup {
    let entries: Vec<_> = buffers
        .iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: buffer.as_entire_binding(),
        })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh compute bindings"),
        layout,
        entries: &entries,
    })
}

fn dispatch(
    pass: &mut wgpu::ComputePass<'_>,
    pipeline: &wgpu::ComputePipeline,
    bind: &wgpu::BindGroup,
    groups: [u32; 3],
) {
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
}

pub(super) fn readback(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Result<Vec<u32>, Error> {
    let (sender, receiver) = oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    block_on(receiver).expect("mapping callback must complete after polling")?;
    let mapped = buffer.slice(..).get_mapped_range()?;
    let words = mapped
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| u32::from_ne_bytes(*v))
        .collect();
    drop(mapped);
    buffer.unmap();
    Ok(words)
}

pub(super) fn extract(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sdf: &str,
    options: MeshOptions,
) -> Result<Vec<Chunk>, Error> {
    options.validate()?;
    let side = u64::from(options.resolution.div_ceil(BRICK));
    let bricks = side.checked_pow(3).ok_or(MeshError::SamplingBufferLimit)?;
    let batch = bricks.min(BATCH as u64) as usize;
    let limit = device
        .limits()
        .max_buffer_size
        .min(device.limits().max_storage_buffer_binding_size);
    if limit < CELL_BYTES.max(SAMPLE_BYTES).max(EDGE_BYTES) {
        return Err(MeshError::SamplingBufferLimit.into());
    }
    let source = format!(
        "#version 450\n\
         layout(local_size_x=4,local_size_y=4,local_size_z=4) in;\n\
         layout(set=0,binding=0,std140) uniform Params {{ vec4 lo; vec4 hi; uvec4 origin; }} sdf_view_params;\n\
         layout(set=0,binding=1,std430) buffer Samples {{ float values[]; }} sdf_view_samples;\n\
         #line 1 1\n{sdf}\n#line 1 0\n\
         void main() {{\n\
           uvec3 q=gl_GlobalInvocationID; if(any(greaterThanEqual(q,uvec3(35u)))) return;\n\
           uvec3 prev=min(sdf_view_params.origin.xyz,uvec3(1u));\n\
           uvec3 base=sdf_view_params.origin.xyz-prev;\n\
           uvec3 offset=q-min(q,uvec3(1u)-prev);\n\
           uvec3 g=base+min(offset,uvec3(sdf_view_params.origin.w)-base);\n\
           vec3 p=sdf_view_params.lo.xyz+vec3(g)/float(sdf_view_params.origin.w)*(sdf_view_params.hi.xyz-sdf_view_params.lo.xyz);\n\
           sdf_view_samples.values[q.x+35u*(q.y+35u*q.z)]=sdf(p);\n\
         }}"
    );
    let module = wgpu::naga::front::glsl::Frontend::default()
        .parse(
            &wgpu::naga::front::glsl::Options::from(wgpu::naga::ShaderStage::Compute),
            &source,
        )
        .map_err(|error| {
            Error::Gpu(wgpu::Error::Validation {
                description: error.emit_to_string(&source),
                source: Box::new(error),
            })
        })?;
    if module
        .global_variables
        .iter()
        .filter(|(_, v)| v.binding.is_some())
        .count()
        != 2
    {
        return Err(MeshError::ResourceBindings.into());
    }
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let sampling_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("brick SDF sampling"),
        source: wgpu::ShaderSource::Naga(Cow::Owned(module)),
    });
    let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("GPU marching cubes"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/marching_cubes.wgsl").into()),
    });
    if let Some(error) = block_on(scope.pop()) {
        return Err(error.into());
    }
    let entries: Vec<_> = (0..9)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 0 {
                    wgpu::BufferBindingType::Uniform
                } else {
                    wgpu::BufferBindingType::Storage {
                        read_only: binding == 2,
                    }
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("marching cubes layout"),
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("marching cubes layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let make = |module: &wgpu::ShaderModule, entry: &str, layout| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout,
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let sampling = make(&sampling_shader, "main", None);
    let classify = make(&compute_shader, "classify", Some(&pipeline_layout));
    let scan = make(&compute_shader, "scan_groups", Some(&pipeline_layout));
    let generate = make(&compute_shader, "generate", Some(&pipeline_layout));
    let generate_indices = make(&compute_shader, "generate_indices", Some(&pipeline_layout));
    if let Some(error) = block_on(scope.pop()) {
        return Err(error.into());
    }
    // Append packed case metadata and the eight brick-boundary ownership masks.
    // This avoids scanning triangle lists or recomputing edge ownership per cell.
    let metadata = TRI_TABLE.iter().map(|row| {
        let mut count = 0u32;
        let mut edges = 0u32;
        for &edge in row.iter().take_while(|&&edge| edge >= 0) {
            count += 1;
            edges |= 1 << edge;
        }
        count | (edges << 4)
    });
    let ownership = (0..8).map(|boundary| {
        EDGES
            .iter()
            .enumerate()
            .fold(0u32, |mask, (edge, &[a, b])| {
                let owns = (0..3).all(|axis| {
                    CORNERS[a][axis] != CORNERS[b][axis]
                        || CORNERS[a][axis] == 1
                        || boundary & (1 << axis) != 0
                });
                mask | (u32::from(owns) << edge)
            })
    });
    let table_bytes: Vec<_> = TRI_TABLE
        .iter()
        .flatten()
        .flat_map(|&edge| (edge as i32).to_ne_bytes())
        .chain(metadata.chain(ownership).flat_map(u32::to_ne_bytes))
        .collect();
    let table = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("marching cubes table"),
        contents: &table_bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let dummy = buffer(
        device,
        "unused vertex output",
        24,
        wgpu::BufferUsages::STORAGE,
    );
    let dummy_indices = buffer(
        device,
        "unused index output",
        4,
        wgpu::BufferUsages::STORAGE,
    );
    let slots: Vec<_> = (0..batch)
        .map(|_| {
            let params = buffer(
                device,
                "brick parameters",
                48,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            let samples = buffer(
                device,
                "brick samples",
                SAMPLE_BYTES,
                wgpu::BufferUsages::STORAGE,
            );
            let cells = buffer(
                device,
                "brick cell offsets",
                CELL_BYTES,
                wgpu::BufferUsages::STORAGE,
            );
            let groups = buffer(
                device,
                "brick group offsets",
                128 * 8,
                wgpu::BufferUsages::STORAGE,
            );
            let status = buffer(
                device,
                "brick status",
                STATUS_BYTES,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            );
            let edges = buffer(
                device,
                "brick edge vertices",
                EDGE_BYTES,
                wgpu::BufferUsages::STORAGE,
            );
            let sampling = bind(
                device,
                &sampling.get_bind_group_layout(0),
                &[&params, &samples],
            );
            let compute = bind(
                device,
                &layout,
                &[
                    &params,
                    &samples,
                    &table,
                    &cells,
                    &groups,
                    &status,
                    &dummy,
                    &edges,
                    &dummy_indices,
                ],
            );
            Slot {
                params,
                samples,
                cells,
                groups,
                status,
                edges,
                sampling,
                compute,
            }
        })
        .collect();
    let counts = buffer(
        device,
        "batch counts readback",
        batch as u64 * STATUS_BYTES,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    );
    let mut chunks = Vec::new();
    for first in (0..bricks).step_by(batch) {
        let active = (bricks - first).min(batch as u64) as usize;
        let mut encoder = device.create_command_encoder(&Default::default());
        for (i, slot) in slots[..active].iter().enumerate() {
            let index = first + i as u64;
            let origin = [
                (index % side) as u32 * BRICK,
                ((index / side) % side) as u32 * BRICK,
                (index / (side * side)) as u32 * BRICK,
                options.resolution,
            ];
            let bytes: Vec<_> = options
                .min
                .into_iter()
                .chain([0.0])
                .chain(options.max)
                .chain([0.0])
                .flat_map(f32::to_ne_bytes)
                .chain(origin.into_iter().flat_map(u32::to_ne_bytes))
                .collect();
            queue.write_buffer(&slot.params, 0, &bytes);
            encoder.clear_buffer(&slot.status, 0, None);
            {
                // Dispatch boundaries retain storage dependencies without ending the pass.
                let mut pass = encoder.begin_compute_pass(&Default::default());
                dispatch(&mut pass, &sampling, &slot.sampling, [9, 9, 9]);
                dispatch(&mut pass, &classify, &slot.compute, [128, 1, 1]);
                dispatch(&mut pass, &scan, &slot.compute, [1, 1, 1]);
            }
            encoder.copy_buffer_to_buffer(
                &slot.status,
                0,
                &counts,
                i as u64 * STATUS_BYTES,
                STATUS_BYTES,
            );
        }
        queue.submit([encoder.finish()]);
        let results = readback(device, &counts)?;
        let mut encoder = device.create_command_encoder(&Default::default());
        for (i, slot) in slots[..active].iter().enumerate() {
            if results[i * 3 + 1] != 0 {
                return Err(MeshError::NonFiniteSamples.into());
            }
            let count = results[i * 3];
            if count == 0 {
                continue;
            }
            let size = u64::from(results[i * 3 + 2]) * 24;
            let index_size = u64::from(count) * 4;
            if size > limit || index_size > limit {
                return Err(MeshError::VertexBufferLimit.into());
            }
            let vertices = buffer(
                device,
                "GPU mesh vertices",
                size,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::VERTEX
                    | wgpu::BufferUsages::COPY_SRC,
            );
            let indices = buffer(
                device,
                "GPU mesh indices",
                index_size,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::INDEX
                    | wgpu::BufferUsages::COPY_SRC,
            );
            let output = bind(
                device,
                &layout,
                &[
                    &slot.params,
                    &slot.samples,
                    &table,
                    &slot.cells,
                    &slot.groups,
                    &slot.status,
                    &vertices,
                    &slot.edges,
                    &indices,
                ],
            );
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                dispatch(&mut pass, &generate, &output, [128, 1, 1]);
                dispatch(&mut pass, &generate_indices, &output, [128, 1, 1]);
            }
            let index = first + i as u64;
            let origin = [index % side, (index / side) % side, index / (side * side)]
                .map(|v| v * u64::from(BRICK));
            let point = |end: bool| {
                [0, 1, 2].map(|axis| {
                    let grid = (origin[axis] + if end { u64::from(BRICK) } else { 0 })
                        .min(u64::from(options.resolution));
                    options.min[axis]
                        + grid as f32 / options.resolution as f32
                            * (options.max[axis] - options.min[axis])
                })
            };
            chunks.push(Chunk {
                vertices,
                indices,
                count,
                min: point(false),
                max: point(true),
            });
        }
        queue.submit([encoder.finish()]);
    }
    Ok(chunks)
}

#[cfg(all(test, feature = "gpu-test"))]
mod tests {
    use std::{collections::BTreeMap, time::Instant};

    use super::*;
    use crate::Renderer;

    fn vertices(renderer: &Renderer, chunks: &[Chunk]) -> Vec<[f32; 6]> {
        let size = chunks
            .iter()
            .map(|c| c.vertices.size() + c.indices.size())
            .sum();
        if size == 0 {
            return Vec::new();
        }
        let output = buffer(
            renderer.device(),
            "test mesh readback",
            size,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        let mut encoder = renderer
            .device()
            .create_command_encoder(&Default::default());
        let mut offset = 0;
        for chunk in chunks {
            for source in [&chunk.vertices, &chunk.indices] {
                encoder.copy_buffer_to_buffer(source, 0, &output, offset, source.size());
                offset += source.size();
            }
        }
        renderer.queue().submit([encoder.finish()]);
        let words = readback(renderer.device(), &output).unwrap();
        let mut offset = 0;
        let mut expanded = Vec::new();
        for chunk in chunks {
            let vertex_words = chunk.vertices.size() as usize / 4;
            let vertices = &words[offset..offset + vertex_words];
            offset += vertex_words;
            let mut used = vec![false; vertex_words / 6];
            for &index in &words[offset..offset + chunk.count as usize] {
                assert!((index as usize) < used.len(), "index outside vertex buffer");
                used[index as usize] = true;
                expanded.push(
                    [0, 1, 2, 3, 4, 5]
                        .map(|axis| f32::from_bits(vertices[index as usize * 6 + axis])),
                );
            }
            assert!(used.iter().all(|&used| used), "unreferenced shared vertex");
            offset += chunk.count as usize;
        }
        expanded
    }

    fn compare(
        renderer: &Renderer,
        sdf: &str,
        options: MeshOptions,
        field: impl Fn([f32; 3]) -> f32,
    ) {
        let n = options.resolution + 1;
        let mut samples = Vec::new();
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    samples.push(field([x, y, z].map(|q| q as f32)));
                }
            }
        }
        let bytes = super::super::polygonize(&samples, options);
        let cpu: Vec<[f32; 6]> = bytes
            .as_chunks::<24>()
            .0
            .iter()
            .map(|v| {
                let words = v.as_chunks::<4>().0;
                [0, 1, 2, 3, 4, 5].map(|i| f32::from_ne_bytes(words[i]))
            })
            .collect();
        let chunks = extract(renderer.device(), renderer.queue(), sdf, options).unwrap();
        let gpu = vertices(renderer, &chunks);
        assert_eq!(gpu.len(), cpu.len());
        // Preserve triangle connectivity and winding, not just the vertex multiset.
        let triangles = |vertices: &[[f32; 6]]| {
            let mut result = BTreeMap::new();
            for triangle in vertices.as_chunks::<3>().0 {
                let points: [[i32; 3]; 3] = [0, 1, 2]
                    .map(|v| [0, 1, 2].map(|axis| (triangle[v][axis] * 1000.0).round() as i32));
                let rotations = [
                    points,
                    [points[1], points[2], points[0]],
                    [points[2], points[0], points[1]],
                ];
                *result
                    .entry(*rotations.iter().min().unwrap())
                    .or_insert(0u32) += 1;
            }
            result
        };
        assert_eq!(triangles(&cpu), triangles(&gpu));
        let group = |vertices: Vec<[f32; 6]>| {
            let mut map: BTreeMap<[i32; 3], Vec<[f32; 6]>> = BTreeMap::new();
            for v in vertices {
                assert!(v.iter().all(|f| f.is_finite()));
                let key = [0, 1, 2].map(|i| (v[i] * 1000.0).round() as i32);
                map.entry(key).or_default().push(v);
            }
            map
        };
        let cpu = group(cpu);
        let gpu = group(gpu);
        assert_eq!(cpu.len(), gpu.len());
        for (key, expected) in cpu {
            let actual = &gpu[&key];
            assert_eq!(expected.len(), actual.len());
            for vertex in actual {
                for axis in 0..6 {
                    assert!(
                        (vertex[axis] - expected[0][axis]).abs() < 0.0002,
                        "vertex mismatch: {vertex:?} versus {:?}",
                        expected[0]
                    );
                }
            }
        }
    }

    #[test]
    fn indexed_planes_share_edges_across_partial_bricks() {
        let renderer = Renderer::new().unwrap();
        let options = MeshOptions {
            min: [0.0; 3],
            max: [33.0; 3],
            resolution: 33,
        };
        for (axis, component) in ["x", "y", "z"].into_iter().enumerate() {
            // Exact zeros exercise endpoints; the final row/column is a partial brick.
            let sdf = format!("float sdf(vec3 p) {{ return round(p.{component}) - 16.0; }}");
            let chunks = extract(renderer.device(), renderer.queue(), &sdf, options).unwrap();
            assert_eq!(chunks.len(), 4);
            assert_eq!(chunks.iter().map(|c| c.count).sum::<u32>(), 6 * 33 * 33);
            // Each brick shares all of its grid edges. Only brick seams are duplicated.
            assert_eq!(
                chunks.iter().map(|c| c.vertices.size() / 24).sum::<u64>(),
                35 * 35
            );
            for vertex in vertices(&renderer, &chunks) {
                assert!((vertex[axis] - 16.0).abs() < 0.00001);
                for direction in 0..3 {
                    assert_eq!(
                        vertex[3 + direction],
                        if direction == axis { 1.0 } else { 0.0 }
                    );
                }
            }
            compare(&renderer, &sdf, options, |p| p[axis] - 16.0);
        }
    }

    #[test]
    fn gpu_matches_cpu_cases_and_brick_boundaries() {
        let renderer = Renderer::new().unwrap();
        // Each 2x2x2 sample tile encodes one case; all 256 cases occur.
        let options = MeshOptions {
            min: [0.0; 3],
            max: [16.0; 3],
            resolution: 16,
        };
        compare(
            &renderer,
            "float sdf(vec3 p) {
            uvec3 q=uvec3(round(p)); uint mask=(q.x/2u+8u*(q.y/2u)+64u*(q.z/2u))&255u;
            uint x=q.x%2u; uint y=q.y%2u;
            uint corner=(y==0u ? x : 3u-x)+4u*(q.z%2u);
            return (mask & (1u<<corner))!=0u ? -1.0 : 1.0;
        }",
            options,
            |p| {
                let [x, y, z] = p.map(|v| v as u32);
                let mask = (x / 2 + 8 * (y / 2) + 64 * (z / 2)) & 255;
                let corner = if y % 2 == 0 { x % 2 } else { 3 - x % 2 } + 4 * (z % 2);
                if mask & (1 << corner) != 0 { -1.0 } else { 1.0 }
            },
        );
        // Twenty-seven bricks cross batch boundaries and include partial extents.
        let options = MeshOptions {
            min: [0.0; 3],
            max: [67.0; 3],
            resolution: 67,
        };
        compare(
            &renderer,
            "float sdf(vec3 p) { return p.x+2.0*p.y+3.0*p.z-191.5; }",
            options,
            |[x, y, z]| x + 2.0 * y + 3.0 * z - 191.5,
        );
    }

    #[test]
    fn empty_groups_preserve_validation_across_batches() {
        let renderer = Renderer::new().unwrap();
        let options = MeshOptions {
            min: [0.0; 3],
            max: [65.0; 3],
            resolution: 65,
        };
        for value in ["-1.0", "0.0", "1.0"] {
            assert!(
                extract(
                    renderer.device(),
                    renderer.queue(),
                    &format!("float sdf(vec3 p) {{ return {value}; }}"),
                    options
                )
                .unwrap()
                .is_empty()
            );
        }
        // The last partial brick in a later batch has no surface but is invalid.
        assert!(matches!(
            extract(
                renderer.device(),
                renderer.queue(),
                "float sdf(vec3 p) {
                if (all(greaterThan(p, vec3(64.5)))) return uintBitsToFloat(0x7fc00000u);
                return 1.0;
            }",
                options
            ),
            Err(Error::Mesh(MeshError::NonFiniteSamples))
        ));
    }

    #[test]
    fn extreme_samples() {
        let renderer = Renderer::new().unwrap();
        let options = MeshOptions {
            resolution: 3,
            ..Default::default()
        };
        let chunks = extract(
            renderer.device(),
            renderer.queue(),
            "float sdf(vec3 p) { return p.x < 0.0 ? -3.0e38 : 3.0e38; }",
            options,
        )
        .unwrap();
        let output = vertices(&renderer, &chunks);
        assert!(!output.is_empty());
        for v in output {
            assert!(v.iter().all(|f| f.is_finite()));
            assert!(v[0].abs() < 0.00001);
        }
        for bits in ["0x7fc00000u", "0xff800000u"] {
            assert!(matches!(
                extract(
                    renderer.device(),
                    renderer.queue(),
                    &format!("float sdf(vec3 p) {{ return uintBitsToFloat({bits}); }}"),
                    options
                ),
                Err(Error::Mesh(MeshError::NonFiniteSamples))
            ));
        }
        assert!(
            extract(
                renderer.device(),
                renderer.queue(),
                "float sdf(vec3 p) { return 0.0; }",
                options
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    #[ignore = "large GPU workload; run explicitly in release mode"]
    fn large_mesh_sampling() {
        let renderer = Renderer::new().unwrap();
        eprintln!("Adapter: {:?}", renderer.adapter_info());
        for resolution in [512, 1024, 2048] {
            let start = Instant::now();
            let options = MeshOptions {
                resolution,
                ..Default::default()
            };
            let chunks = extract(
                renderer.device(),
                renderer.queue(),
                "float sdf(vec3 p) { return length(p)-1.0; }",
                options,
            )
            .unwrap();
            renderer
                .device()
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            let count: u64 = chunks.iter().map(|c| u64::from(c.count)).sum();
            let vertex_count: u64 = chunks.iter().map(|c| c.vertices.size() / 24).sum();
            let bytes: u64 = chunks
                .iter()
                .map(|c| c.vertices.size() + c.indices.size())
                .sum();
            assert!(count > 0);
            eprintln!(
                "{resolution}^3: {:?}, {} chunks, {vertex_count} vertices, {count} indices, {} MiB",
                start.elapsed(),
                chunks.len(),
                bytes / 1024 / 1024
            );
        }
    }
}
