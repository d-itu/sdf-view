use std::borrow::Cow;

use futures::executor::block_on;

use crate::{Antialiasing, Error, RenderOptions};

/// Compiled SDF pipeline for drawing directly to a GPU texture.
/// Create and use it with the same device and an sRGB render target.
/// Update parameters before submitting a draw; do not update between unsubmitted draws.
pub struct ScenePipeline {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl ScenePipeline {
    /// Compile a snippet for the target format. Failed compilation leaves existing pipelines intact.
    pub fn new(
        device: &wgpu::Device,
        sdf: &str,
        format: wgpu::TextureFormat,
    ) -> Result<Self, Error> {
        let source = format!(
            "#version 450\n{}\n#line 1 1\n{}\n#line 1 0\n{}",
            include_str!("shaders/scene.glsl"),
            sdf,
            include_str!("shaders/fragment.glsl")
        );
        let module = wgpu::naga::front::glsl::Frontend::default()
            .parse(
                &wgpu::naga::front::glsl::Options::from(wgpu::naga::ShaderStage::Fragment),
                &source,
            )
            .map_err(|error| {
                Error::Gpu(wgpu::Error::Validation {
                    description: error.emit_to_string(&source),
                    source: Box::new(error),
                })
            })?;
        // Reject user resource declarations, even when unused or sharing our binding.
        let bindings = module
            .global_variables
            .iter()
            .filter(|(_, global)| global.binding.is_some())
            .count();
        if bindings != 1 {
            let message = "user snippets must not declare resource bindings";
            return Err(Error::Gpu(wgpu::Error::Validation {
                description: message.into(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    message,
                )),
            }));
        }
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("SDF scene layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(144),
                },
                count: None,
            }],
        });
        // Resolve shader errors before attempting to build or submit a pipeline.
        let shader_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SDF fragment shader"),
            source: wgpu::ShaderSource::Naga(Cow::Owned(module)),
        });
        if let Some(error) = block_on(shader_scope.pop()) {
            return Err(Error::Gpu(error));
        }
        let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fullscreen triangle"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/vertex.wgsl").into()),
        });
        let pipeline_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        // Only the renderer-owned scene uniform is available to shaders.
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SDF pipeline layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("SDF pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        if let Some(error) = block_on(pipeline_scope.pop()) {
            return Err(Error::Gpu(error));
        }

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SDF scene parameters"),
            size: 144,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SDF scene"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Ok(Self {
            pipeline,
            uniform,
            bind_group,
        })
    }

    /// Upload scene parameters, including the background, using straight alpha.
    pub fn update(&self, queue: &wgpu::Queue, options: RenderOptions) -> Result<(), Error> {
        self.update_surface(queue, options, false)
    }

    /// Upload scene parameters for a window surface.
    /// Set `premultiplied` only when the surface uses premultiplied alpha.
    pub fn update_surface(
        &self,
        queue: &wgpu::Queue,
        options: RenderOptions,
        premultiplied: bool,
    ) -> Result<(), Error> {
        let (background, color) = match options.background {
            crate::Background::Transparent => (0.0, [0.0; 3]),
            crate::Background::Checkerboard => (1.0, [0.0; 3]),
            crate::Background::Rgb(color) => (
                2.0,
                color.map(|value| {
                    let srgb = f32::from(value) / 255.0;
                    if srgb <= 0.04045 {
                        srgb / 12.92
                    } else {
                        ((srgb + 0.055) / 1.055).powf(2.4)
                    }
                }),
            ),
        };
        options.validate()?;
        let basis = options.camera.basis()?;
        let light = options.light.normalized_direction()?;
        let rows = [
            [
                options.width as f32,
                options.height as f32,
                match options.antialiasing {
                    Antialiasing::X1 => 1.0,
                    Antialiasing::X4 => 2.0,
                },
                background,
            ],
            [
                options.camera.position[0],
                options.camera.position[1],
                options.camera.position[2],
                (options.camera.vertical_fov_degrees.to_radians() * 0.5).tan(),
            ],
            [basis.forward[0], basis.forward[1], basis.forward[2], 0.0],
            [basis.right[0], basis.right[1], basis.right[2], 0.0],
            [basis.up[0], basis.up[1], basis.up[2], 0.0],
            [light[0], light[1], light[2], options.light.intensity],
            [
                options.light.color[0],
                options.light.color[1],
                options.light.color[2],
                options.light.ambient,
            ],
            [
                color[0],
                color[1],
                color[2],
                if premultiplied { 1.0 } else { 0.0 },
            ],
            [
                options.object_color[0],
                options.object_color[1],
                options.object_color[2],
                0.0,
            ],
        ];
        let mut bytes = [0u8; 144];
        for (dst, value) in bytes
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rows.into_iter().flatten())
        {
            dst.copy_from_slice(&value.to_ne_bytes());
        }
        queue.write_buffer(&self.uniform, 0, &bytes);
        Ok(())
    }

    /// Record a fullscreen draw into a target matching the pipeline format.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("SDF render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
