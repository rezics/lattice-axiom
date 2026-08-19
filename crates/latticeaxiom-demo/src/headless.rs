//! Headless mode: the vertical slice without a window or GPU.
//!
//! Runs the same scene as the windowed host through the headless renderer.
//! CI uses this as the milestone 1 smoke check that simulation, extraction,
//! and the render contract hold without creating any GPU device.

use glam::Vec3;
use latticeaxiom_render::{Camera, Renderer as _};
use latticeaxiom_render_headless::HeadlessRenderer;

use crate::scene::Scene;

/// Simulates and submits `frames` frames, then reports the totals.
///
/// # Errors
///
/// Fails when resource upload or any frame submission is rejected — which
/// would mean the demo violates its own render contract.
pub fn run(frames: u32) -> anyhow::Result<()> {
    let mut renderer = HeadlessRenderer::new(1280, 720);
    let scene = Scene::upload(&mut renderer)?;
    let camera = Camera::look_at(Vec3::new(6.0, -8.0, 4.5), Vec3::new(0.0, 0.0, 0.5));

    let mut instances_drawn = 0;
    for frame in 0..frames {
        // Fixed timestep instead of wall-clock time so runs are reproducible.
        #[expect(
            clippy::cast_precision_loss,
            reason = "frame counts stay far below f32 integer precision limits"
        )]
        let seconds = frame as f32 / 60.0;
        let world = scene.render_world(camera, seconds);
        let report = renderer.submit(&world)?;
        instances_drawn = report.instances_drawn;
    }

    tracing::info!(
        frames,
        instances_per_frame = instances_drawn,
        "headless run completed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_run_completes() {
        run(3).unwrap();
    }
}
