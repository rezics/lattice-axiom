//! Windowed first-person sandbox host.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context as _, Result};
use glam::Vec3;
use latticeaxiom_core::{BlockPos, break_block, place_block, raycast_voxels};
use latticeaxiom_render::{RenderWorld, Renderer as _};
use latticeaxiom_render_wgpu::WgpuRenderer;
use latticeaxiom_storage::WorldStorage;
use latticeaxiom_storage_rocksdb::RocksDbWorldStorage;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::chunk_render::{ChunkMeshes, rebuild_pending};
use crate::player_controller::PlayerController;
use crate::runtime_config::{RuntimeConfig, project_root};
use crate::sandbox_world::SandboxWorld;
use crate::ui::{UiLayer, UiMetrics};

const INITIAL_MESH_BUDGET: usize = usize::MAX;
const STREAM_BUDGET_PER_FRAME: usize = 4;
const MESH_BUDGET_PER_FRAME: usize = 6;
const REACH_METERS: f32 = 6.0;

/// Runs the windowed demo until the window closes.
///
/// # Errors
///
/// Fails when composition, persistent world startup, the event loop, window,
/// or renderer cannot be created.
pub fn run() -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    event_loop.run_app(&mut app)?;

    match app.terminal_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Top-level winit application; live state is created from the resumed event.
#[derive(Default)]
struct App {
    active: Option<ActiveApp>,
    terminal_error: Option<anyhow::Error>,
}

/// Window, platform input, simulation, persistence, and renderer state.
struct ActiveApp {
    window: Arc<Window>,
    renderer: WgpuRenderer,
    ui: UiLayer,
    world: SandboxWorld,
    chunks: ChunkMeshes,
    player: PlayerController,
    last_frame: Instant,
    smoothed_fps: f32,
    mouse_captured: bool,
}

impl ActiveApp {
    fn create(event_loop: &ActiveEventLoop) -> Result<Self> {
        let attributes = Window::default_attributes()
            .with_title("Lattice Axiom — M3 sandbox")
            .with_inner_size(LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attributes)?);

        let root = project_root();
        let runtime = RuntimeConfig::load(&root)?;
        tracing::info!(graph_sha256 = %runtime.graph_sha256, "resolved game composition");

        let runtime_directory = root.join("run");
        std::fs::create_dir_all(&runtime_directory)
            .context("failed to create the local runtime directory")?;
        let storage: Arc<dyn WorldStorage> = Arc::new(
            RocksDbWorldStorage::open(runtime_directory.join("world"))
                .context("failed to open the authoritative RocksDB world")?,
        );
        let mut world = SandboxWorld::new(
            storage,
            runtime.catalog,
            runtime.seed,
            runtime.producer_hash,
        )?;
        let surface_height = world.heightmap().height_at(0, 0);
        let preferred_feet_z = surface_height
            .checked_add(1)
            .context("the fixed sandbox spawn height overflowed")?;
        let _ = world.center_on(BlockPos::new(0, 0, preferred_feet_z));
        world
            .stream_all()
            .context("failed to materialize the initial chunk residency window")?;
        let spawn_feet = world
            .find_standing_space(0, 0, preferred_feet_z)
            .context("no safe standing space exists near the sandbox spawn column")?;
        #[allow(
            clippy::cast_precision_loss,
            reason = "the bounded spawn search stays inside exact f32 integer precision"
        )]
        let spawn = Vec3::new(
            spawn_feet.x as f32 + 0.5,
            spawn_feet.y as f32 + 0.5,
            spawn_feet.z as f32 + 0.9,
        );
        let player = PlayerController::spawn(spawn, std::f32::consts::FRAC_PI_2, -0.25)
            .context("resolved sandbox spawn must form a valid player body")?;
        let _ = world.center_on(player.center_block());
        world
            .stream_all()
            .context("failed to center chunk residency around the resolved spawn")?;

        let size = window.inner_size();
        let mut renderer = WgpuRenderer::new(Arc::clone(&window), size.width, size.height)?;
        let mut chunks = ChunkMeshes::new(&mut renderer)?;
        rebuild_pending(&mut renderer, &mut chunks, &mut world, INITIAL_MESH_BUDGET)?;
        let ui = UiLayer::new(&window);

        tracing::info!(
            resident_chunks = world.loaded_chunks(),
            visible_chunk_meshes = chunks.active_len(),
            "sandbox world ready"
        );
        Ok(Self {
            window,
            renderer,
            ui,
            world,
            chunks,
            player,
            last_frame: Instant::now(),
            smoothed_fps: 60.0,
            mouse_captured: false,
        })
    }

    fn render_frame(&mut self) -> Result<()> {
        let now = Instant::now();
        let delta_seconds = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        self.player
            .advance(delta_seconds, |block| {
                self.world.is_collision_solid_at(block)
            })
            .context("player physics step failed")?;
        let residency = self.world.center_on(self.player.center_block());
        for position in residency.unloaded {
            self.chunks.unload(position);
        }
        self.world
            .stream(STREAM_BUDGET_PER_FRAME)
            .context("chunk streaming failed")?;
        rebuild_pending(
            &mut self.renderer,
            &mut self.chunks,
            &mut self.world,
            MESH_BUDGET_PER_FRAME,
        )?;

        if delta_seconds > f32::EPSILON {
            let instantaneous = delta_seconds.recip().min(1_000.0);
            self.smoothed_fps = self.smoothed_fps.mul_add(0.92, instantaneous * 0.08);
        }
        let eye = self.player.eye_position();
        let metrics = UiMetrics {
            frames_per_second: self.smoothed_fps,
            player_position: eye.to_array(),
            loaded_chunks: self.world.loaded_chunks(),
            pending_loads: self.world.pending_loads(),
            pending_meshes: self.world.pending_meshes(),
            edits: self.world.edit_count(),
            selected_block: self.world.catalog().selected().to_raw(),
            mouse_captured: self.mouse_captured,
        };
        self.renderer.queue_ui(self.ui.frame(&self.window, metrics));
        let render_world = RenderWorld {
            camera: self.player.camera(),
            instances: self.chunks.instances()?,
        };
        self.renderer
            .submit(&render_world)
            .context("frame submission failed")?;
        Ok(())
    }

    fn interact(&mut self, button: MouseButton) -> Result<()> {
        let hit = raycast_voxels(
            self.player.eye_position(),
            self.player.direction(),
            REACH_METERS,
            |block| self.world.is_solid_at(block),
        );
        let Some(hit) = hit else {
            return Ok(());
        };

        let edit = match button {
            MouseButton::Left => break_block(hit.block, self.world.block_at(hit.block)),
            MouseButton::Right => place_block(
                hit.placement,
                self.world.block_at(hit.placement),
                self.world.catalog().selected(),
                self.player.body().bounds(),
            ),
            _ => return Ok(()),
        };
        match edit {
            Ok(edit) => {
                self.world
                    .set_block_durable(edit.position, edit.replacement)
                    .context("failed to durably commit the block edit")?;
            }
            Err(error) => tracing::debug!(%error, "block interaction rejected"),
        }
        Ok(())
    }

    fn set_mouse_captured(&mut self, captured: bool) {
        if captured == self.mouse_captured {
            return;
        }
        if captured {
            let grabbed = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
            if let Err(error) = grabbed {
                tracing::warn!(%error, "mouse capture is unavailable on this window system");
                return;
            }
            self.window.set_cursor_visible(false);
        } else {
            if let Err(error) = self.window.set_cursor_grab(CursorGrabMode::None) {
                tracing::warn!(%error, "failed to release mouse capture");
            }
            self.window.set_cursor_visible(true);
            self.player.clear_inputs();
        }
        self.mouse_captured = captured;
    }

    fn controller_key(&mut self, code: KeyCode, pressed: bool) {
        if self.mouse_captured {
            self.player.set_key(code, pressed);
        }
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
                self.terminal_error = Some(error);
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
        let ui_consumed = active.ui.on_window_event(&active.window, &event);

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => active.renderer.resize(size.width, size.height),
            WindowEvent::Focused(false) => active.set_mouse_captured(false),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::Escape && event.state.is_pressed() {
                        if active.mouse_captured {
                            active.set_mouse_captured(false);
                        } else {
                            event_loop.exit();
                        }
                        return;
                    }
                    if !ui_consumed {
                        active.controller_key(code, event.state.is_pressed());
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. }
                if state == ElementState::Pressed && !ui_consumed =>
            {
                if active.mouse_captured {
                    if let Err(error) = active.interact(button) {
                        tracing::warn!(%error, "block interaction failed");
                    }
                } else if button == MouseButton::Left {
                    active.set_mouse_captured(true);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = active.render_frame() {
                    tracing::error!(error = ?error, "sandbox frame failed; exiting event loop");
                    self.terminal_error = Some(error);
                    event_loop.exit();
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
        if let DeviceEvent::MouseMotion { delta } = event {
            if active.mouse_captured {
                active.player.apply_look(delta.0, delta.1);
            } else {
                active.ui.on_mouse_motion(delta);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(active) = &self.active {
            active.window.request_redraw();
        }
    }
}
