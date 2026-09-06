//! Bounded, always-available native frame monitor and opt-in capture evidence.

use std::{collections::VecDeque, time::Instant};

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
};
use serde::Serialize;

const HISTORY: usize = 240;
const CAPTURE_LIMIT: usize = 16_384;

#[derive(Component, Debug)]
struct FrameReadout;

#[derive(Resource, Debug)]
pub(crate) struct FrameMonitor {
    started: Instant,
    main_ms: f64,
    frames: VecDeque<f64>,
    capture: Option<Vec<f64>>,
    capture_main: Vec<f64>,
    capture_truncated: bool,
    next_refresh: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct FrameSummary {
    samples: usize,
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    fps: f64,
}

fn summarize(samples: impl Iterator<Item = f64>) -> Option<FrameSummary> {
    let mut values: Vec<_> = samples.filter(|v| v.is_finite() && *v > 0.0).collect();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f64::total_cmp);
    let count = values.len();
    let mean = values.iter().sum::<f64>() / f64::from(u32::try_from(count).unwrap_or(u32::MAX));
    let percentile = |p: usize| values[(count * p).div_ceil(100).saturating_sub(1)];
    Some(FrameSummary {
        samples: count,
        mean_ms: mean,
        p50_ms: percentile(50),
        p95_ms: percentile(95),
        p99_ms: percentile(99),
        max_ms: values[count - 1],
        fps: 1000.0 / mean,
    })
}

impl FrameMonitor {
    pub(crate) fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "frame": self.capture.as_ref().and_then(|v| summarize(v.iter().copied())),
            "main_schedule": summarize(self.capture_main.iter().copied()),
            "warmup_seconds": 10,
            "truncated": self.capture_truncated,
            "notes": "Real window frame intervals after warmup, including pacing. Main schedule excludes render-thread and GPU time."
        })
    }
}

pub(crate) fn install(app: &mut App) {
    app.insert_resource(FrameMonitor {
        started: Instant::now(),
        main_ms: 0.0,
        frames: VecDeque::with_capacity(HISTORY),
        capture: std::env::var_os("LATTICEAXIOM_CAPTURE_PATH").map(|_| Vec::new()),
        capture_main: Vec::new(),
        capture_truncated: false,
        next_refresh: 0.0,
    })
    .add_systems(Startup, spawn)
    .add_systems(First, begin_frame)
    .add_systems(Update, update)
    .add_systems(Last, end_frame);
}

fn spawn(mut commands: Commands<'_, '_>) {
    commands
        .spawn((
            Name::new("Frame monitor"),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                top: Val::Px(64.0),
                padding: UiRect::all(Val::Px(10.0)),
                max_width: Val::Percent(48.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.04, 0.055, 0.92)),
            GlobalZIndex(100),
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            parent.spawn((
                FrameReadout,
                Text::new("FPS —   Frame — ms"),
                crate::ui_font::ui_text_font(14.0),
                TextColor(Color::srgb(0.92, 0.95, 0.98)),
                Pickable::IGNORE,
            ));
        });
}

fn begin_frame(mut monitor: ResMut<'_, FrameMonitor>) {
    monitor.started = Instant::now();
}
fn end_frame(mut monitor: ResMut<'_, FrameMonitor>) {
    monitor.main_ms = monitor.started.elapsed().as_secs_f64() * 1000.0;
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn update(
    time: Res<'_, Time<Real>>,
    diagnostics: Res<'_, DiagnosticsStore>,
    mut monitor: ResMut<'_, FrameMonitor>,
    mut readout: Query<'_, '_, &mut Text, With<FrameReadout>>,
    windows: Query<'_, '_, &Window, With<bevy::window::PrimaryWindow>>,
) {
    let ms = time.delta_secs_f64() * 1000.0;
    if ms > 0.0 {
        if monitor.frames.len() == HISTORY {
            monitor.frames.pop_front();
        }
        monitor.frames.push_back(ms);
        if time.elapsed_secs_f64() >= 10.0 {
            let main_ms = monitor.main_ms;
            if let Some(capture) = monitor.capture.as_mut() {
                if capture.len() < CAPTURE_LIMIT {
                    capture.push(ms);
                    monitor.capture_main.push(main_ms);
                } else {
                    monitor.capture_truncated = true;
                }
            }
        }
    }
    if time.elapsed_secs_f64() < monitor.next_refresh {
        return;
    }
    monitor.next_refresh = time.elapsed_secs_f64() + 0.25;
    let Some(summary) = summarize(monitor.frames.iter().copied()) else {
        return;
    };
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(bevy::diagnostic::Diagnostic::smoothed)
        .unwrap_or(summary.mean_ms);
    let state = if windows.iter().next().is_some_and(|w| !w.focused) {
        " · Background"
    } else {
        ""
    };
    for mut text in &mut readout {
        text.0 = format!(
            "{:.0} FPS   {:.1} ms{state}\nP95 {:.1} ms   CPU {:.1} ms",
            1000.0 / frame_ms,
            frame_ms,
            summary.p95_ms,
            monitor.main_ms
        );
    }
}

#[cfg(test)]
mod tests {
    use super::summarize;
    #[test]
    fn quantiles_keep_hitches_and_fps_uses_elapsed_time() {
        let mut values = vec![10.0; 99];
        values.push(100.0);
        let result = summarize(values.into_iter()).expect("nonempty samples");
        assert_eq!(result.p95_ms.to_bits(), 10.0_f64.to_bits());
        assert_eq!(result.max_ms.to_bits(), 100.0_f64.to_bits());
        assert!((result.fps - 1000.0 / 10.9).abs() < 0.001);
        assert!(summarize([0.0, f64::NAN, f64::INFINITY].into_iter()).is_none());
    }
}
