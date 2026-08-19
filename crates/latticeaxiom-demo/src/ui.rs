//! egui diagnostics and control overlay for the sandbox host.

use egui::{Align2, Color32, CornerRadius, Frame, Id, Stroke, ViewportId};
use latticeaxiom_render_wgpu::UiFrame;
use winit::event::WindowEvent;
use winit::window::Window;

/// Snapshot of simulation metrics displayed by the overlay.
#[derive(Clone, Copy, Debug, Default)]
pub struct UiMetrics {
    /// Smoothed rendered frames per second.
    pub frames_per_second: f32,
    /// Player eye position in canonical `(x, y, z)` world coordinates.
    pub player_position: [f32; 3],
    /// Number of chunks resident in memory.
    pub loaded_chunks: usize,
    /// Number of requested chunks waiting for storage or generation.
    pub pending_loads: usize,
    /// Number of chunk meshes waiting to be rebuilt.
    pub pending_meshes: usize,
    /// Total durable block edits during this run.
    pub edits: u64,
    /// Raw ID of the block selected for placement.
    pub selected_block: u32,
    /// Whether gameplay currently owns relative mouse input.
    pub mouse_captured: bool,
}

/// Owns egui's platform input state while rendering remains in the wgpu
/// backend crate.
pub struct UiLayer {
    context: egui::Context,
    state: egui_winit::State,
    viewport_initialized: bool,
}

impl std::fmt::Debug for UiLayer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UiLayer")
            .field("viewport_initialized", &self.viewport_initialized)
            .finish_non_exhaustive()
    }
}

impl UiLayer {
    /// Creates UI state for the root window viewport.
    #[must_use]
    pub fn new(window: &Window) -> Self {
        let context = egui::Context::default();
        #[allow(
            clippy::cast_possible_truncation,
            reason = "egui and winit define pixels-per-point as f32 and f64 respectively"
        )]
        let pixels_per_point = window.scale_factor() as f32;
        let state = egui_winit::State::new(
            context.clone(),
            ViewportId::ROOT,
            window,
            Some(pixels_per_point),
            window.theme(),
            None,
        );
        Self {
            context,
            state,
            viewport_initialized: false,
        }
    }

    /// Feeds a window event to egui and reports whether UI consumed it.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.state.on_window_event(window, event).consumed
    }

    /// Feeds raw mouse motion to egui when gameplay has not captured it.
    pub fn on_mouse_motion(&mut self, delta: (f64, f64)) -> bool {
        self.state.on_mouse_motion(delta)
    }

    /// Builds the next overlay frame for the render backend.
    #[must_use]
    pub fn frame(&mut self, window: &Window, metrics: UiMetrics) -> UiFrame {
        let viewport = self
            .state
            .egui_input_mut()
            .viewports
            .entry(ViewportId::ROOT)
            .or_default();
        egui_winit::update_viewport_info(
            viewport,
            &self.context,
            window,
            !self.viewport_initialized,
        );
        self.viewport_initialized = true;

        let raw_input = self.state.take_egui_input(window);
        let output = self.context.run_ui(raw_input, |root| {
            egui::Area::new(Id::new("sandbox metrics"))
                .anchor(Align2::LEFT_TOP, [12.0, 12.0])
                .interactable(false)
                .show(root.ctx(), |ui| {
                    Frame::new()
                        .fill(Color32::from_black_alpha(190))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.heading("Lattice Axiom · sandbox");
                            ui.monospace(format!(
                                "{:.0} fps  pos {:.1} {:.1} {:.1}",
                                metrics.frames_per_second,
                                metrics.player_position[0],
                                metrics.player_position[1],
                                metrics.player_position[2]
                            ));
                            ui.monospace(format!(
                                "chunks {} (+{})  remesh {}  edits {}  block #{}",
                                metrics.loaded_chunks,
                                metrics.pending_loads,
                                metrics.pending_meshes,
                                metrics.edits,
                                metrics.selected_block
                            ));
                            ui.label(if metrics.mouse_captured {
                                "WASD move · Space jump · LMB break · RMB place · Esc release"
                            } else {
                                "Click the world to capture the mouse"
                            });
                        });
                });

            let center = root.max_rect().center();
            let painter = root.painter();
            let stroke = Stroke::new(2.0, Color32::WHITE);
            painter.line_segment(
                [
                    center + egui::vec2(-7.0, 0.0),
                    center + egui::vec2(7.0, 0.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(0.0, -7.0),
                    center + egui::vec2(0.0, 7.0),
                ],
                stroke,
            );
        });

        let pixels_per_point = output.pixels_per_point;
        let primitives = self.context.tessellate(output.shapes, pixels_per_point);
        self.state
            .handle_platform_output(window, output.platform_output);

        UiFrame {
            primitives,
            textures_delta: output.textures_delta,
            pixels_per_point,
        }
    }
}
