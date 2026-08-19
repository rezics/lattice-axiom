//! Windowed host: winit event loop driving the wgpu renderer.

use std::sync::Arc;
use std::time::Instant;

use glam::Vec3;
use latticeaxiom_render::{RenderError, Renderer as _};
use latticeaxiom_render_wgpu::WgpuRenderer;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::camera_controller::CameraController;
use crate::scene::Scene;

/// Runs the windowed demo until the window closes.
///
/// # Errors
///
/// Fails when the event loop, window, or renderer cannot be created.
pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    event_loop.run_app(&mut app)?;

    match app.startup_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Top-level winit application; state is created lazily in [`Self::resumed`]
/// as required by the winit 0.30 lifecycle.
#[derive(Default)]
struct App {
    active: Option<ActiveApp>,
    startup_error: Option<anyhow::Error>,
}

/// Live state once the window and renderer exist.
struct ActiveApp {
    window: Arc<Window>,
    renderer: WgpuRenderer,
    scene: Scene,
    controller: CameraController,
    started: Instant,
    last_frame: Instant,
    look_active: bool,
}

impl ActiveApp {
    fn create(event_loop: &ActiveEventLoop) -> anyhow::Result<Self> {
        let attributes = Window::default_attributes()
            .with_title("Lattice Axiom demo — milestone 1")
            .with_inner_size(LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attributes)?);

        let size = window.inner_size();
        let mut renderer = WgpuRenderer::new(Arc::clone(&window), size.width, size.height)?;
        let scene = Scene::upload(&mut renderer)?;
        let controller =
            CameraController::looking_at(Vec3::new(6.0, -8.0, 4.5), Vec3::new(0.0, 0.0, 0.5));

        let now = Instant::now();
        Ok(Self {
            window,
            renderer,
            scene,
            controller,
            started: now,
            last_frame: now,
            look_active: false,
        })
    }

    fn render_frame(&mut self) -> Result<(), RenderError> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        self.controller.advance(dt);
        let world = self.scene.render_world(
            self.controller.camera(),
            self.started.elapsed().as_secs_f32(),
        );
        self.renderer.submit(&world).map(drop)
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.active.is_some() {
            return;
        }
        match ActiveApp::create(event_loop) {
            Ok(active) => self.active = Some(active),
            Err(error) => {
                self.startup_error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(active) = &mut self.active else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => active.renderer.resize(size.width, size.height),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::Escape {
                        event_loop.exit();
                        return;
                    }
                    active.controller.set_key(code, event.state.is_pressed());
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state,
                ..
            } => active.look_active = state == ElementState::Pressed,
            WindowEvent::RedrawRequested => {
                if let Err(error) = active.render_frame() {
                    // Transient surface loss is retried inside the renderer;
                    // anything surfacing here is worth seeing but not fatal.
                    tracing::warn!(%error, "frame submission failed");
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        let Some(active) = &mut self.active else {
            return;
        };
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && active.look_active
        {
            active.controller.apply_look(dx, dy);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(active) = &self.active {
            active.window.request_redraw();
        }
    }
}
