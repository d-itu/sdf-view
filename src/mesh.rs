use std::cell::RefCell;

use futures::executor::block_on;
#[cfg(all(test, feature = "gpu-test"))]
use marching_cubes::tables::TRI_TABLE;

mod gpu;

use crate::{Antialiasing, Error, MeshError, RenderOptions, SettingsError};

/// World-space sampling bounds and number of marching-cubes cells per axis.
#[derive(Clone, Copy, Debug)]
pub struct MeshOptions {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub resolution: u32,
}

impl Default for MeshOptions {
    fn default() -> Self {
        Self {
            min: [-2.0; 3],
            max: [2.0; 3],
            resolution: 64,
        }
    }
}

impl MeshOptions {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.resolution < 2
            || !(0..3).all(|i| {
                self.min[i].is_finite()
                    && self.max[i].is_finite()
                    && self.min[i] < self.max[i]
                    && (self.max[i] - self.min[i]).is_finite()
                    && (self.max[i] - self.min[i]) / self.resolution as f32 > 0.0
            })
        {
            return Err(SettingsError::InvalidMesh);
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "gpu-test"))]
const CORNERS: [[usize; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];
#[cfg(all(test, feature = "gpu-test"))]
const EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

#[cfg(all(test, feature = "gpu-test"))]
fn polygonize(values: &[f32], options: MeshOptions) -> Vec<u8> {
    let r = options.resolution as usize;
    let n = r + 1;
    let index = |p: [usize; 3]| p[0] + n * (p[1] + n * p[2]);
    let step: [f32; 3] =
        [0, 1, 2].map(|i| (options.max[i] - options.min[i]) / options.resolution as f32);
    let mut vertices = Vec::new();
    for z in 0..r {
        for y in 0..r {
            for x in 0..r {
                let points = CORNERS.map(|c| [x + c[0], y + c[1], z + c[2]]);
                let distances = points.map(|p| values[index(p)]);
                let case = distances
                    .iter()
                    .enumerate()
                    .fold(0, |mask, (i, d)| mask | (usize::from(*d < 0.0) << i));
                if case == 0 || case == 255 {
                    continue;
                }
                let normals = points.map(|p| {
                    let mut normal = [0.0; 3];
                    for axis in 0..3 {
                        let mut lo = p;
                        let mut hi = p;
                        lo[axis] = p[axis].saturating_sub(1);
                        hi[axis] = (p[axis] + 1).min(r);
                        normal[axis] = (values[index(hi)] - values[index(lo)])
                            / ((hi[axis] - lo[axis]) as f32 * step[axis]);
                    }
                    normal
                });
                for &edge in TRI_TABLE[case].iter().take_while(|&&e| e >= 0) {
                    let [a, b] = EDGES[edge as usize];
                    let t = (f64::from(distances[a])
                        / (f64::from(distances[a]) - f64::from(distances[b])))
                        as f32;
                    let position: [f32; 3] = [0, 1, 2].map(|i| {
                        options.min[i]
                            + (points[a][i] as f32
                                + t * (points[b][i] as f32 - points[a][i] as f32))
                                * step[i]
                    });
                    let mut normal =
                        [0, 1, 2].map(|i| normals[a][i] * (1.0 - t) + normals[b][i] * t);
                    let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
                    normal = if length > 0.0 && length.is_finite() {
                        normal.map(|v| v / length)
                    } else {
                        [0.0, 0.0, 1.0]
                    };
                    for value in position.into_iter().chain(normal) {
                        vertices.extend_from_slice(&value.to_ne_bytes());
                    }
                }
            }
        }
    }
    vertices
}

struct Targets {
    dimensions: [u32; 2],
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

pub(crate) struct Mesh {
    device: wgpu::Device,
    uniform: wgpu::Buffer,
    chunks: Vec<gpu::Chunk>,
    raster: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    targets: RefCell<Option<Targets>>,
}

impl Mesh {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sdf: &str,
        format: wgpu::TextureFormat,
        options: MeshOptions,
        uniform: &wgpu::Buffer,
    ) -> Result<Self, Error> {
        let chunks = gpu::extract(device, queue, sdf, options)?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh rasterization"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mesh.wgsl").into()),
        });
        if let Some(error) = block_on(scope.pop()) {
            return Err(error.into());
        }
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let make_pipeline = |mesh: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if mesh {
                    "mesh rasterization"
                } else {
                    "mesh composite"
                }),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if mesh { "mesh_vertex" } else { "fullscreen" }),
                    compilation_options: Default::default(),
                    buffers: if mesh { &buffers } else { &[] },
                },
                primitive: Default::default(),
                depth_stencil: mesh.then_some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if mesh { "mesh_fragment" } else { "composite" }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: if mesh {
                            wgpu::TextureFormat::Rgba16Float
                        } else {
                            format
                        },
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let raster = make_pipeline(true);
        let composite = make_pipeline(false);
        if let Some(error) = block_on(scope.pop()) {
            return Err(error.into());
        }
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &raster.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Ok(Self {
            device: device.clone(),
            uniform: uniform.clone(),
            chunks,
            raster,
            composite,
            bind,
            targets: RefCell::new(None),
        })
    }

    pub(crate) fn update(&self, options: RenderOptions) -> Result<(), Error> {
        let scale = if options.antialiasing == Antialiasing::X4 {
            2
        } else {
            1
        };
        let width = options
            .width
            .checked_mul(scale)
            .ok_or(MeshError::ImageDimensionsOverflow)?;
        let height = options
            .height
            .checked_mul(scale)
            .ok_or(MeshError::ImageDimensionsOverflow)?;
        if width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
        {
            return Err(MeshError::TextureDimensionLimit.into());
        }
        let mut targets = self.targets.borrow_mut();
        if targets
            .as_ref()
            .is_some_and(|t| t.dimensions == [width, height])
        {
            return Ok(());
        }
        let texture = |format, usage| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("mesh render target"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color = texture(
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.composite.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&color),
                },
            ],
        });
        *targets = Some(Targets {
            dimensions: [width, height],
            color,
            depth,
            bind,
        });
        Ok(())
    }

    pub(crate) fn draw(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let targets = self.targets.borrow();
        let targets = targets
            .as_ref()
            .expect("update scene parameters before drawing a mesh");
        for mesh in [true, false] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mesh render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: if mesh { &targets.color } else { view },
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: mesh.then_some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(if mesh { &self.raster } else { &self.composite });
            pass.set_bind_group(0, if mesh { &self.bind } else { &targets.bind }, &[]);
            if mesh {
                for chunk in &self.chunks {
                    pass.set_vertex_buffer(0, chunk.vertices.slice(..));
                    pass.draw(0..chunk.count, 0..1);
                }
            } else {
                pass.draw(0..3, 0..1);
            }
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn validates_large_grids_without_a_device() {
        for resolution in [2, 257, 512, 1024, 1_048_577, u32::MAX] {
            assert!(
                MeshOptions {
                    resolution,
                    ..Default::default()
                }
                .validate()
                .is_ok()
            );
        }
        for resolution in [0, 1] {
            assert!(
                MeshOptions {
                    resolution,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            MeshOptions {
                min: [0.0; 3],
                max: [f32::from_bits(1); 3],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
