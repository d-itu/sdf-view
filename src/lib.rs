//! Headless rendering of GLSL signed distance functions with wgpu.
//!
//! Camera and directional lighting are configurable through [`RenderOptions`].
//! By default the camera is at `(0, 0, 3)`, looks toward the origin with Y up,
//! and has a 45-degree vertical field of view;
//! missed rays produce transparent black pixels. This blocking API targets
//! native applications and requires a working wgpu graphics adapter.

use futures::{channel::oneshot, executor::block_on};

mod pipeline;
pub use pipeline::ScenePipeline;

mod scene;
pub use scene::{Camera, DirectionalLight};

mod error;
pub use error::{Error, SettingsError};

/// Number of subpixel rays traced per output pixel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Antialiasing {
    /// One ray through the pixel center (no antialiasing).
    #[default]
    X1,
    /// Four rays on a uniform 2-by-2 subpixel grid.
    X4,
}

/// Background shared by headless rendering and interactive previews.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    /// Preserve transparent pixels and antialiased coverage.
    #[default]
    Transparent,
    /// Composite onto a 16-pixel gray checkerboard.
    Checkerboard,
    /// Composite onto an opaque sRGB color with 8-bit components.
    Rgb([u8; 3]),
}

/// Image dimensions, camera, lighting, antialiasing, and background for a render.
#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    pub camera: Camera,
    pub light: DirectionalLight,
    pub antialiasing: Antialiasing,
    pub background: Background,
    /// Linear RGB surface albedo; finite components are clamped to [0, 1].
    pub object_color: [f32; 3],
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 512,
            height: 512,
            camera: Camera::default(),
            light: DirectionalLight::default(),
            antialiasing: Antialiasing::default(),
            background: Background::default(),
            object_color: [137u8, 196, 237]
                .map(|value| ((f32::from(value) / 255.0 + 0.055) / 1.055).powf(2.4)),
        }
    }
}

impl RenderOptions {
    /// Validate dimensions and scene settings without initializing a GPU.
    /// Device-specific size limits are checked separately by `Renderer::render`.
    pub fn validate(&self) -> Result<(), SettingsError> {
        ReadbackLayout::new([self.width, self.height])?;
        if !self.object_color.iter().all(|v| v.is_finite()) {
            return Err(SettingsError::NonFiniteFloat);
        }
        self.camera.basis()?;
        self.light.normalized_direction()?;
        Ok(())
    }
}

/// A reusable, headless graphics device for SDF rendering.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: wgpu::AdapterInfo,
}

impl Renderer {
    /// Select a native graphics adapter and create a device without a window.
    /// Software adapters are supported when provided by the graphics driver.
    pub fn new() -> Result<Self, Error> {
        block_on(async {
            let instance = wgpu::Instance::new(
                wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
            );
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                    ..Default::default()
                })
                .await?;
            Ok(adapter)
        })
        .and_then(|adapter| Self::from_adapter(&adapter))
    }

    /// Create a renderer from an adapter selected for a window surface.
    pub fn from_adapter(adapter: &wgpu::Adapter) -> Result<Self, Error> {
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("sdf-view"),
            ..Default::default()
        }))?;
        Ok(Self {
            device,
            queue,
            adapter_info: adapter.get_info(),
        })
    }

    /// Device used by this renderer, for compatible GPU pipelines and surfaces.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Queue used by this renderer.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Render a GLSL snippet defining `float sdf(vec3 p)`.
    ///
    /// Helper functions are allowed. Omit `#version`, `main`, and resource
    /// bindings: the library supplies a GLSL 450 fragment shader around the
    /// snippet. Naga's GLSL frontend determines the supported GLSL subset.
    ///
    /// Sphere tracing uses 256 steps, a 100-unit distance limit, and a 0.001-unit
    /// hit tolerance. The function must return a signed distance (or a
    /// conservative distance estimate) for reliable results.
    /// Compilation and validation errors are returned instead of panicking.
    /// The returned view owns top-to-bottom sRGB RGBA8 readback memory, even
    /// after the renderer is dropped. Each row contains `options.width * 4`
    /// pixel bytes followed by padding to a multiple of 256 bytes
    /// ([`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`]). The view length is the padded
    /// row stride times `options.height`. No CPU pixel copy is performed.
    pub fn render(&self, sdf: &str, options: RenderOptions) -> Result<wgpu::BufferView, Error> {
        options.validate()?;
        self.with_error_scope(|| {
            let pipeline =
                ScenePipeline::new(&self.device, sdf, wgpu::TextureFormat::Rgba8UnormSrgb)?;
            self.render_inner(&pipeline, options)
        })
    }

    /// Render with an already compiled pipeline.
    ///
    /// The pipeline must have been created with this renderer's device and target
    /// `wgpu::TextureFormat::Rgba8UnormSrgb`. This is useful when several images
    /// use the same SDF with different scene settings.
    pub fn render_with_pipeline(
        &self,
        pipeline: &ScenePipeline,
        options: RenderOptions,
    ) -> Result<wgpu::BufferView, Error> {
        options.validate()?;
        self.with_error_scope(|| self.render_inner(pipeline, options))
    }

    fn with_error_scope<T>(&self, render: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = self.device.push_error_scope(wgpu::ErrorFilter::Internal);
        let result = render();
        let internal_error = block_on(internal.pop());
        let memory_error = block_on(memory.pop());
        let validation_error = block_on(validation.pop());
        if let Some(error) = internal_error.or(memory_error).or(validation_error) {
            return Err(Error::Gpu(error));
        }
        result
    }

    fn render_inner(
        &self,
        pipeline: &ScenePipeline,
        options: RenderOptions,
    ) -> Result<wgpu::BufferView, Error> {
        let layout = ReadbackLayout::new([options.width, options.height])?;
        pipeline.update(&self.queue, options)?;
        let size = wgpu::Extent3d {
            width: options.width,
            height: options.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("SDF image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SDF readback"),
            size: layout.buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        pipeline.draw(&mut encoder, &view);
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.padded_row_bytes),
                    rows_per_image: Some(options.height),
                },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = oneshot::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        // Drive GPU completion and mapping callbacks before waiting on the future.
        // The futures executor does not poll the wgpu device itself.
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        block_on(receiver).unwrap()?;
        let mapped = slice.get_mapped_range()?;
        Ok(mapped)
    }
}

#[derive(Clone, Copy, Debug)]
struct ReadbackLayout {
    padded_row_bytes: u32,
    buffer_size: u64,
}

impl ReadbackLayout {
    fn new([width, height]: [u32; 2]) -> Result<Self, SettingsError> {
        if width == 0 || height == 0 {
            return Err(SettingsError::ZeroDimensions);
        }
        let row_bytes = width.checked_mul(4).ok_or(SettingsError::RowSizeOverflow)?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row_bytes = row_bytes
            .checked_add(alignment - 1)
            .ok_or(SettingsError::PaddedRowSizeOverflow)?
            / alignment
            * alignment;
        let buffer_size = u64::from(padded_row_bytes) * u64::from(height);
        if usize::try_from(buffer_size).is_err() {
            return Err(SettingsError::ReadbackSizeOverflow);
        }
        Ok(Self {
            padded_row_bytes,
            buffer_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_colors() {
        for component in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for options in [
                RenderOptions {
                    object_color: [component; 3],
                    ..Default::default()
                },
                RenderOptions {
                    light: DirectionalLight {
                        color: [component; 3],
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ] {
                assert_eq!(options.validate(), Err(SettingsError::NonFiniteFloat));
            }
        }
        RenderOptions {
            object_color: [-1.0, 0.5, 2.0],
            light: DirectionalLight {
                color: [2.0, -1.0, 0.5],
                ..Default::default()
            },
            ..Default::default()
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn readback_padding() {
        for (width, padded) in [(1, 256), (64, 256), (65, 512), (129, 768)] {
            let layout = ReadbackLayout::new([width, 3]).unwrap();
            assert_eq!(layout.padded_row_bytes, padded);
            assert_eq!(layout.buffer_size, u64::from(padded) * 3);
        }
    }

    #[test]
    fn invalid_dimensions() {
        for (width, height, error) in [
            (0, 1, SettingsError::ZeroDimensions),
            (1, 0, SettingsError::ZeroDimensions),
            (u32::MAX, 1, SettingsError::RowSizeOverflow),
            (u32::MAX / 4, 1, SettingsError::PaddedRowSizeOverflow),
        ] {
            assert_eq!(ReadbackLayout::new([width, height]).err(), Some(error));
            assert_eq!(
                RenderOptions {
                    width,
                    height,
                    ..Default::default()
                }
                .validate(),
                Err(error)
            );
        }
    }
}
