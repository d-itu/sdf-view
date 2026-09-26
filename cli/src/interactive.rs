use std::{fs, result, sync::Arc};

use futures::executor::block_on;
use glam::{Quat, Vec3};
use sdf_view::{Antialiasing, Camera, RenderOptions, Renderer, ScenePipeline};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{CursorGrabMode, Window, WindowId},
};

use crate::{Args, Error};

type Result<T> = result::Result<T, Error>;

struct Preview {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    pipeline: ScenePipeline,
}

impl Preview {
    fn new(events: &ActiveEventLoop, args: &Args, source: &str) -> Result<Self> {
        let window = Arc::new(
            events.create_window(
                Window::default_attributes()
                    .with_transparent(matches!(args.background, sdf_view::Background::Transparent))
                    .with_title("sdf-view")
                    .with_inner_size(PhysicalSize::new(args.width, args.height)),
            )?,
        );
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
        );
        let surface = instance.create_surface(window.clone())?;
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(sdf_view::Error::Adapter)?;
        let renderer = Renderer::from_adapter(&adapter)?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or(Error::NoSupportedConfiguration)?;
        config.format = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(|format| format.is_srgb())
            .ok_or(Error::NoSupportedFormat)?;
        if let sdf_view::Background::Transparent = args.background {
            let modes = surface.get_capabilities(&adapter).alpha_modes;
            if let Some(mode) = [
                wgpu::CompositeAlphaMode::PostMultiplied,
                wgpu::CompositeAlphaMode::PreMultiplied,
            ]
            .into_iter()
            .find(|mode| modes.contains(mode))
            {
                config.alpha_mode = mode;
            } else {
                tracing::warn!(
                    "Window surface does not expose alpha compositing; transparent pixels may appear opaque. PNG output retains transparency."
                );
            }
        }
        let pipeline = ScenePipeline::new(renderer.device(), source, config.format)?;
        let mut preview = Self {
            window,
            instance,
            surface,
            config,
            renderer,
            pipeline,
        };
        preview.resize(size)?;
        Ok(preview)
    }

    fn resize(&mut self, size: PhysicalSize<u32>) -> Result<()> {
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(self.renderer.device(), &self.config);
        self.window.request_redraw();
        Ok(())
    }

    fn draw(&mut self, mut options: sdf_view::RenderOptions) -> Result<()> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        options.width = self.config.width;
        options.height = self.config.height;
        let mut reconfigure = false;
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                reconfigure = true;
                frame
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self.instance.create_surface(self.window.clone())?;
                self.resize(size)?;
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(size)?;
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(Error::SurfaceValidation);
            }
        };
        self.pipeline.update_surface(
            self.renderer.queue(),
            options,
            self.config.alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied,
        )?;
        let mut encoder = self
            .renderer
            .device()
            .create_command_encoder(&Default::default());
        self.pipeline.draw(
            &mut encoder,
            &frame.texture.create_view(&Default::default()),
        );
        self.renderer.queue().submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.renderer.queue().present(frame);
        if reconfigure {
            self.resize(size)?;
        }
        Ok(())
    }
}

struct App {
    args: Args,
    source: String,
    orbit: Orbit,
    preview: Option<Preview>,
    drag: Drag,
    confined: bool,
    error: Option<Error>,
}

impl App {
    fn redraw(&self) {
        if let Some(preview) = &self.preview {
            preview.window.request_redraw();
        }
    }

    fn release_drag(&mut self) {
        self.drag.cancel();
        if self.confined {
            if let Some(preview) = &self.preview {
                let _ = preview.window.set_cursor_grab(CursorGrabMode::None);
            }
            self.confined = false;
        }
    }

    fn title(&self) {
        if let Some(preview) = &self.preview {
            let samples = match self.args.antialiasing {
                Antialiasing::X1 => 1,
                Antialiasing::X4 => 4,
            };
            preview.window.set_title(&format!(
                "sdf-view — {} — {samples} sample(s)",
                self.args.input.display()
            ));
            preview.window.request_redraw();
        }
    }

    fn reload(&mut self) -> Result<()> {
        let source = fs::read_to_string(&self.args.input)?;
        if let Some(preview) = &mut self.preview {
            let pipeline =
                ScenePipeline::new(preview.renderer.device(), &source, preview.config.format)?;
            preview.pipeline = pipeline;
            self.source = source;
            preview.window.request_redraw();
        }
        Ok(())
    }

    fn screenshot(&mut self) -> Result<()> {
        let output = if let Some(output) = self.args.output.as_ref() {
            output
        } else {
            tracing::warn!("specify -o <PNG> to enable screenshots");
            return Ok(());
        };
        if let Some(preview) = &mut self.preview {
            let mut options = self.args.render_options();
            options.camera = self.orbit.camera;
            options.width = preview.config.width;
            options.height = preview.config.height;
            let pixels = preview
                .renderer
                .render_with_pipeline(&preview.pipeline, options)?;
            crate::save_png(options.width, options.height, &pixels, output)?;
            tracing::info!(path = %output.display(), width = options.width, height = options.height, "Saved PNG");
        }
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if self.preview.is_some() {
            return;
        }
        match Preview::new(events, &self.args, &self.source) {
            Ok(preview) => {
                self.preview = Some(preview);
                self.title();
            }
            Err(error) => {
                self.error = Some(error);
                events.exit();
            }
        }
    }

    fn window_event(&mut self, events: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.release_drag();
                events.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(preview) = &mut self.preview
                    && let Err(error) = preview.resize(size)
                {
                    self.error = Some(error);
                    events.exit();
                }
            }
            WindowEvent::RedrawRequested => {
                let mut options = self.args.render_options();
                options.camera = self.orbit.camera;
                if let Some(preview) = &mut self.preview
                    && let Err(error) = preview.draw(options)
                {
                    self.error = Some(error);
                    events.exit();
                }
            }
            WindowEvent::Occluded(false) => self.redraw(),
            WindowEvent::Focused(false) => self.release_drag(),
            WindowEvent::CursorLeft { .. } => {
                if !self.confined {
                    self.release_drag();
                }
            }
            WindowEvent::CursorEntered { .. } | WindowEvent::ScaleFactorChanged { .. } => {
                self.drag.reset_position();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.drag.button(button, state);
                if self.drag.mode.is_none() {
                    self.release_drag();
                } else if !self.confined
                    && let Some(preview) = &self.preview
                {
                    self.confined = preview
                        .window
                        .set_cursor_grab(CursorGrabMode::Confined)
                        .is_ok();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(preview) = &self.preview {
                    let scale = preview.window.scale_factor();
                    if let Some((mode, dx, dy)) = self.drag.moved(position, scale) {
                        match mode {
                            Mode::Orbit => self.orbit.rotate(dx, dy),
                            Mode::Pan => {
                                self.orbit
                                    .pan(dx, dy, preview.config.height as f32 / scale as f32)
                            }
                        }
                        self.redraw();
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.orbit.zoom(steps);
                self.redraw();
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                let result = match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) => {
                        self.release_drag();
                        events.exit();
                        Ok(())
                    }
                    Key::Named(NamedKey::Home) => {
                        self.release_drag();
                        self.orbit.reset();
                        Ok(())
                    }
                    Key::Character("a" | "A") => {
                        self.args.antialiasing = match self.args.antialiasing {
                            Antialiasing::X1 => Antialiasing::X4,
                            Antialiasing::X4 => Antialiasing::X1,
                        };
                        self.title();
                        Ok(())
                    }
                    Key::Character("r" | "R") => self.reload(),
                    Key::Character("s" | "S") => self.screenshot(),
                    _ => Ok(()),
                };
                if let Err(error) = result {
                    tracing::error!("{error}");
                }
                self.redraw();
            }
            _ => {}
        }
    }
}

pub fn run(args: Args, source: String) -> result::Result<(), Error> {
    let events = EventLoop::new()?;
    events.set_control_flow(ControlFlow::Wait);
    let orbit = Orbit::new(args.render_options().camera);
    let mut app = App {
        args,
        source,
        orbit,
        preview: None,
        drag: Drag::default(),
        confined: false,
        error: None,
    };
    tracing::info!(
        "Left drag: orbit; right drag: pan; wheel: zoom; Home: reset; A: samples; R: reload; S: save; Esc: exit"
    );
    events.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error)?,
        None => Ok(()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Orbit,
    Pan,
}

#[derive(Default)]
struct Drag {
    mode: Option<Mode>,
    cursor: Option<PhysicalPosition<f64>>,
}

impl Drag {
    fn cancel(&mut self) {
        self.mode = None;
        self.cursor = None;
    }

    fn reset_position(&mut self) {
        self.cursor = None;
    }

    fn button(&mut self, button: MouseButton, state: ElementState) {
        let mode = match button {
            MouseButton::Left => Mode::Orbit,
            MouseButton::Right => Mode::Pan,
            _ => return,
        };
        if state == ElementState::Pressed {
            // The latest pressed button owns the drag; never apply both operations.
            self.mode = Some(mode);
        } else if self.mode == Some(mode) {
            self.cancel();
        }
    }

    fn moved(&mut self, position: PhysicalPosition<f64>, scale: f64) -> Option<(Mode, f32, f32)> {
        let previous = self.cursor.replace(position)?;
        let mode = self.mode?;
        if !scale.is_finite() || scale <= 0.0 {
            self.reset_position();
            return None;
        }
        let dx = ((position.x - previous.x) / scale) as f32;
        let dy = ((position.y - previous.y) / scale) as f32;
        (dx.is_finite() && dy.is_finite()).then_some((mode, dx, dy))
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;
    use ElementState::{Pressed, Released};
    use MouseButton::{Left, Right};

    #[test]
    fn drag_boundaries_and_button_switches() {
        let mut drag = Drag::default();
        let p = PhysicalPosition::new;
        drag.cancel();
        drag.button(Left, Pressed);
        assert_eq!(drag.moved(p(10.0, 10.0), 1.0), None);
        assert_eq!(
            drag.moved(p(20.0, 15.0), 1.0),
            Some((Mode::Orbit, 10.0, 5.0))
        );
        drag.button(Right, Pressed);
        drag.button(Left, Released);
        assert_eq!(drag.moved(p(30.0, 20.0), 1.0), Some((Mode::Pan, 10.0, 5.0)));
        assert_eq!(drag.moved(p(40.0, 25.0), 1.0), Some((Mode::Pan, 10.0, 5.0)));
        drag.button(Right, Released);
        assert_eq!(drag.moved(p(50.0, 30.0), 1.0), None);
        drag.button(Left, Pressed);
        drag.moved(p(50.0, 30.0), 1.0);
        drag.cancel();
        assert_eq!(drag.moved(p(1000.0, 1000.0), 1.0), None);
        assert_eq!(drag.mode, None);
    }

    #[test]
    fn logical_motion_matches_across_display_scales() {
        for scale in [1.0, 1.5, 2.0] {
            let mut drag = Drag::default();
            drag.button(Left, Pressed);
            drag.moved(PhysicalPosition::new(10.0 * scale, 20.0 * scale), scale);
            assert_eq!(
                drag.moved(PhysicalPosition::new(30.0 * scale, 25.0 * scale), scale),
                Some((Mode::Orbit, 20.0, 5.0))
            );
            drag.reset_position();
            assert_eq!(drag.moved(PhysicalPosition::new(500.0, 500.0), scale), None);
        }
    }
}

struct Orbit {
    initial: Camera,
    camera: Camera,
}

impl Orbit {
    fn new(camera: Camera) -> Self {
        Self {
            initial: camera,
            camera,
        }
    }

    fn reset(&mut self) {
        self.camera = self.initial;
    }

    fn accept(&mut self, camera: Camera) {
        if (RenderOptions {
            camera,
            ..Default::default()
        })
        .validate()
        .is_ok()
        {
            self.camera = camera;
        }
    }

    fn rotate(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let target = Vec3::from(self.camera.target);
        let up = Vec3::from(self.camera.up).normalize();
        let offset = Vec3::from(self.camera.position) - target;
        let distance = offset.length();
        let vertical = offset.dot(up);
        let horizontal = offset - vertical * up;
        let elevation = vertical.atan2(horizontal.length());
        // Clamp the angle itself so a large input cannot cross either pole.
        let limit = 0.995_f32.asin();
        let elevation = (elevation + dy * 0.005).clamp(-limit, limit);
        let yaw = Quat::from_axis_angle(up, (-dx * 0.005).rem_euclid(std::f32::consts::TAU));
        let horizontal = yaw * horizontal.normalize();
        let offset = distance * (horizontal * elevation.cos() + up * elevation.sin());
        self.accept(Camera {
            position: (target + offset).to_array(),
            ..self.camera
        });
    }

    fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        let position = Vec3::from(self.camera.position);
        let target = Vec3::from(self.camera.target);
        let forward = (target - position).normalize();
        let right = forward.cross(Vec3::from(self.camera.up)).normalize();
        let up = right.cross(forward);
        let scale = 2.0
            * position.distance(target)
            * (self.camera.vertical_fov_degrees.to_radians() * 0.5).tan()
            / height.max(1.0);
        let movement = (-dx * right + dy * up) * scale;
        self.accept(Camera {
            position: (position + movement).to_array(),
            target: (target + movement).to_array(),
            ..self.camera
        });
    }

    fn zoom(&mut self, steps: f32) {
        let target = Vec3::from(self.camera.target);
        let offset = Vec3::from(self.camera.position) - target;
        let distance =
            (offset.length() * (-steps.clamp(-20.0, 20.0) * 0.12).exp()).clamp(0.01, 10000.0);
        self.accept(Camera {
            position: (target + offset.normalize() * distance).to_array(),
            ..self.camera
        });
    }
}

#[cfg(test)]
mod orbit_tests {
    use super::*;

    #[test]
    fn pole_clamping_is_continuous_and_reversible() {
        for up in [Vec3::Y, Vec3::Z] {
            let camera = Camera {
                position: Vec3::X.to_array(),
                up: up.to_array(),
                ..Default::default()
            };
            for sign in [-1.0, 1.0] {
                let mut orbit = Orbit::new(camera);
                orbit.rotate(0.0, sign * 100_000.0);
                let position = Vec3::from(orbit.camera.position);
                assert!((position.length() - 1.0).abs() < 1e-5);
                assert!((position.dot(up) - sign * 0.995).abs() < 1e-5);
                assert!(position.dot(Vec3::X) > 0.0);
                orbit.rotate(0.0, sign * 20.0);
                assert!(Vec3::from(orbit.camera.position).distance(position) < 1e-5);
                orbit.rotate(0.0, -sign);
                assert!(Vec3::from(orbit.camera.position).dot(up).abs() < 0.995);
                orbit.rotate(50.0, 0.0);
                (RenderOptions {
                    camera: orbit.camera,
                    ..Default::default()
                })
                .validate()
                .unwrap();
            }
        }
    }

    #[test]
    fn rotation_is_consistent_across_event_sizes() {
        let camera = Camera {
            position: [3.0, 2.0, 4.0],
            target: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            ..Default::default()
        };
        let mut single = Orbit::new(camera);
        let mut many = Orbit::new(camera);
        single.rotate(120.0, 60.0);
        for _ in 0..120 {
            many.rotate(1.0, 0.5);
        }
        assert!(Vec3::from(single.camera.position).distance(many.camera.position.into()) < 1e-4);
        single.rotate(-120.0, -60.0);
        assert!(Vec3::from(single.camera.position).distance(camera.position.into()) < 1e-5);
        let before = single.camera.position;
        single.rotate(f32::NAN, 0.0);
        single.rotate(0.0, f32::INFINITY);
        assert_eq!(single.camera.position, before);
    }

    #[test]
    fn orbit_preserves_valid_custom_camera_and_resets() {
        let camera = Camera {
            position: [3.0, 4.0, 2.0],
            target: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            ..Default::default()
        };
        let mut orbit = Orbit::new(camera);
        let distance = Vec3::from(camera.position).distance(camera.target.into());
        orbit.rotate(20.0, 30.0);
        assert!(
            (Vec3::from(orbit.camera.position).distance(orbit.camera.target.into()) - distance)
                .abs()
                < 1e-5
        );
        assert_ne!(orbit.camera.position, camera.position);
        orbit.pan(10.0, 20.0, 512.0);
        assert_ne!(orbit.camera.target, camera.target);
        orbit.zoom(2.0);
        assert!(Vec3::from(orbit.camera.position).distance(orbit.camera.target.into()) < distance);
        for _ in 0..1000 {
            orbit.rotate(10.0, 10.0);
            orbit.zoom(20.0);
        }
        (RenderOptions {
            camera: orbit.camera,
            ..Default::default()
        })
        .validate()
        .unwrap();
        orbit.reset();
        assert_eq!(orbit.camera.position, camera.position);
        assert_eq!(orbit.camera.target, camera.target);
        assert_eq!(orbit.camera.up, camera.up);
    }
}
