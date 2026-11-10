use std::time::{Duration, Instant};

use super::*;
use crate::Renderer;

// Expand entirely on the GPU, outside the measured interval, to reproduce the
// former non-indexed draw with identical triangle order, positions and normals.
fn expanded_vertices(mesh: &Mesh, queue: &wgpu::Queue) -> Vec<wgpu::Buffer> {
    let shader = mesh
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("benchmark vertex expansion"),
            source: wgpu::ShaderSource::Wgsl(
                " 
            @group(0) @binding(0) var<storage, read> vertices: array<f32>;
            @group(0) @binding(1) var<storage, read> indices: array<u32>;
            @group(0) @binding(2) var<storage, read_write> expanded: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) id: vec3u) {
                if id.x >= arrayLength(&indices) { return; }
                for (var i = 0u; i < 6u; i++) {
                    expanded[id.x * 6u + i] = vertices[indices[id.x] * 6u + i];
                }
            }"
                .into(),
            ),
        });
    let pipeline = mesh
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let mut encoder = mesh.device.create_command_encoder(&Default::default());
    let buffers = mesh
        .chunks
        .iter()
        .map(|chunk| {
            let output = mesh.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("benchmark expanded vertices"),
                size: u64::from(chunk.count) * 24,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            });
            let bindings = [&chunk.vertices, &chunk.indices, &output];
            let entries: Vec<_> = bindings
                .iter()
                .enumerate()
                .map(|(i, buffer)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            let bind = mesh.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(chunk.count.div_ceil(64), 1, 1);
            output
        })
        .collect();
    queue.submit([encoder.finish()]);
    mesh.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    buffers
}

fn frame(mesh: &Mesh, queue: &wgpu::Queue, target: &wgpu::TextureView) {
    let mut encoder = mesh.device.create_command_encoder(&Default::default());
    mesh.draw(&mut encoder, target);
    queue.submit([encoder.finish()]);
}

fn image(mesh: &Mesh, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    frame(mesh, queue, &texture.create_view(&Default::default()));
    let buffer = mesh.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(texture.width()) * u64::from(texture.height()) * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = mesh.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texture.width() * 4),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    gpu::readback(&mesh.device, &buffer)
        .unwrap()
        .into_iter()
        .flat_map(u32::to_ne_bytes)
        .collect()
}

fn measure(mesh: &Mesh, queue: &wgpu::Queue, view: &wgpu::TextureView) -> Duration {
    // Warm up, then report the median of five batches. This measures CPU command
    // recording/submission plus GPU completion, not isolated GPU timestamp time.
    for _ in 0..5 {
        frame(mesh, queue, view);
    }
    mesh.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let mut timings = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..20 {
            frame(mesh, queue, view);
        }
        mesh.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        timings.push(start.elapsed() / 20);
    }
    timings.sort();
    timings[2]
}

#[test]
#[ignore = "rendering benchmark; run explicitly in release mode"]
fn mesh_rendering() {
    let renderer = Renderer::new().unwrap();
    let device = renderer.device();
    let queue = renderer.queue();
    eprintln!("Adapter: {:?}", renderer.adapter_info());
    for resolution in [128, 512, 1024] {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 144,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut mesh = Mesh::new(
            device,
            queue,
            "float sdf(vec3 p) { return length(p)-1.0; }",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            MeshOptions {
                resolution,
                ..Default::default()
            },
            &uniform,
        )
        .unwrap();
        let shared: Vec<_> = mesh.chunks.iter().map(|c| c.vertices.clone()).collect();
        let expanded = expanded_vertices(&mesh, queue);
        for width in [512, 1024] {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width,
                    height: width,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            for (name, distance) in [("full", 3.0), ("close", 1.2)] {
                let options = RenderOptions {
                    width,
                    height: width,
                    camera: crate::Camera {
                        position: [0.0, 0.0, distance],
                        ..Default::default()
                    },
                    ..Default::default()
                };
                // Fixed axial camera and simple lighting match the Scene uniform layout.
                let rows: [[f32; 4]; 9] = [
                    [width as f32, width as f32, 1.0, 0.0],
                    [0.0, 0.0, distance, (45.0f32.to_radians() * 0.5).tan()],
                    [0.0, 0.0, -1.0, 0.0],
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.85],
                    [1.0, 1.0, 1.0, 0.15],
                    [0.0; 4],
                    [0.3, 0.5, 0.8, 0.0],
                ];
                let bytes: Vec<_> = rows
                    .into_iter()
                    .flatten()
                    .flat_map(f32::to_ne_bytes)
                    .collect();
                queue.write_buffer(&uniform, 0, &bytes);
                mesh.update(options).unwrap();
                let visible = mesh.visible.borrow().len();
                let reference = image(&mesh, queue, &texture);
                assert!(reference.as_chunks::<4>().0.iter().any(|p| p[3] != 0));
                let indexed = measure(&mesh, queue, &view);
                for (chunk, buffer) in mesh.chunks.iter_mut().zip(&expanded) {
                    chunk.vertices = buffer.clone();
                }
                mesh.unindexed = true;
                assert_eq!(image(&mesh, queue, &texture), reference);
                let unindexed = measure(&mesh, queue, &view);
                for (chunk, buffer) in mesh.chunks.iter_mut().zip(&shared) {
                    chunk.vertices = buffer.clone();
                }
                mesh.unindexed = false;
                *mesh.visible.borrow_mut() = (0..mesh.chunks.len()).collect();
                assert_eq!(image(&mesh, queue, &texture), reference);
                let unculled = measure(&mesh, queue, &view);
                eprintln!(
                    "grid={resolution} image={width} {name}: {visible}/{} chunks; indexed={indexed:?}, expanded={unindexed:?}, indexed without culling={unculled:?}",
                    mesh.chunks.len()
                );
            }
        }
    }
}
