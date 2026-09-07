//! Opt-in Bevy scene capture. Embedded `WebView` content is not captured.
//!
//! These images cannot verify Web UI appearance, DOM interaction, or window
//! composition. Output is never source content.

use std::path::PathBuf;

use bevy::{
    diagnostic::FrameCount,
    prelude::{App, AppExit, Commands, MessageWriter, On, Res, ResMut, Resource, Update},
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

#[derive(Debug, Resource)]
struct CaptureRequest {
    path: PathBuf,
    requested: bool,
}

pub(crate) fn install(app: &mut App) {
    let Some(path) = std::env::var_os("LATTICEAXIOM_CAPTURE_PATH").map(PathBuf::from) else {
        return;
    };
    if let Ok(size) = std::env::var("LATTICEAXIOM_CAPTURE_SIZE")
        && let Some((width, height)) = size.split_once('x')
        && let (Ok(width), Ok(height)) = (width.parse::<u32>(), height.parse::<u32>())
    {
        let world = app.world_mut();
        let mut windows = world.query::<&mut bevy::window::Window>();
        for mut window in windows.iter_mut(world) {
            window.resolution = bevy::window::WindowResolution::new(
                width.clamp(640, 3840),
                height.clamp(480, 2160),
            );
        }
    }
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        bevy::log::error!(%error, "capture output directory could not be created");
        return;
    }
    if std::env::var_os("LATTICEAXIOM_CAPTURE_TEXT").is_some()
        || std::env::var_os("LATTICEAXIOM_CAPTURE_ACTION").is_some()
    {
        bevy::log::warn!(
            "CAPTURE_TEXT and CAPTURE_ACTION no longer inject removed native widgets; use the gated lifecycle domain exercise or test the Web UI directly"
        );
    }
    bevy::log::info!("Capture records the Bevy scene only; embedded WebView content is excluded");
    app.insert_resource(CaptureRequest {
        path,
        requested: false,
    })
    .add_systems(Update, (capture_once, capture_scene_probe));
}

/// A presentation-only camera override applied before medium/fog selection.
pub(crate) fn overview_camera_transform(
    center: bevy::prelude::Vec3,
) -> Option<bevy::prelude::Transform> {
    if std::env::var_os("LATTICEAXIOM_CAPTURE_PATH").is_none()
        || std::env::var_os("LATTICEAXIOM_CAPTURE_OVERVIEW").is_none()
    {
        return None;
    }
    Some(
        bevy::prelude::Transform::from_translation(
            center + bevy::prelude::Vec3::new(0.0, 48.0, 32.0),
        )
        .looking_at(
            center + bevy::prelude::Vec3::new(0.0, 0.0, -48.0),
            bevy::prelude::Vec3::Y,
        ),
    )
}

#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::too_many_arguments)] // Snapshot independent render and timing evidence once.
fn capture_scene_probe(
    frame: Res<'_, FrameCount>,
    request: Res<'_, CaptureRequest>,
    mut written: bevy::prelude::Local<'_, bool>,
    spine: Option<Res<'_, crate::ProductionSpine>>,
    monitor: Res<'_, crate::frame_monitor::FrameMonitor>,
    diagnostics: Res<'_, bevy::diagnostic::DiagnosticsStore>,
    cameras: bevy::prelude::Query<
        '_,
        '_,
        (&bevy::prelude::Transform, &bevy::camera::Projection),
        bevy::prelude::With<crate::host::ProductionCamera>,
    >,
    meshes: bevy::prelude::Query<
        '_,
        '_,
        (
            &bevy::prelude::GlobalTransform,
            Option<&bevy::prelude::ViewVisibility>,
        ),
        bevy::prelude::With<bevy::prelude::Mesh3d>,
    >,
) {
    if !request.requested || *written {
        return;
    }
    let Some(spine) = spine else {
        return;
    };
    *written = true;
    let pose = spine.player_pose();
    let camera_rows = cameras.iter().map(|(transform, projection)| serde_json::json!({
        "translation": transform.translation.to_array(), "rotation": transform.rotation.to_array(), "projection": format!("{projection:?}")
    })).collect::<Vec<_>>();
    let mesh_rows = meshes.iter().take(24).map(|(transform, visible)| serde_json::json!({
        "translation": transform.translation().to_array(), "visible": visible.map(|value| value.get())
    })).collect::<Vec<_>>();
    let timings = diagnostics.iter().map(|d| serde_json::json!({"path": d.path().as_str(), "mean": d.average(), "smoothed": d.smoothed()})).collect::<Vec<_>>();
    let report = serde_json::json!({"scope":"bevy-scene-only","webview_captured":false,"frame": frame.0, "performance": monitor.report(), "diagnostics": timings, "world": spine.world_id(), "chunk_edge": spine.chunk_edge(), "player": pose.translation.to_array(),
        "resident": format!("{:?}", spine.resident_chunks()), "cameras": camera_rows, "mesh_count": meshes.iter().count(), "meshes": mesh_rows});
    if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
        let _ = std::fs::write(request.path.with_extension("json"), bytes);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn capture_once(
    mut commands: Commands<'_, '_>,
    frame: Res<'_, FrameCount>,
    time: Res<'_, bevy::prelude::Time<bevy::time::Real>>,
    mut request: ResMut<'_, CaptureRequest>,
) {
    let delay = std::env::var("LATTICEAXIOM_CAPTURE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(3.0)
        .clamp(3.0, 600.0);
    if frame.0 < 90 || time.elapsed_secs() < delay || request.requested {
        return;
    }
    request.requested = true;
    let mut save = save_to_disk(request.path.clone());
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<'_, '_, ScreenshotCaptured>,
              mut exits: MessageWriter<'_, AppExit>,
              session: Option<Res<'_, crate::host::persistent::PersistentGameSession>>| {
            save(captured);
            if std::env::var_os("LATTICEAXIOM_CAPTURE_SAVE").is_some()
                && let Some(session) = session
            {
                session.request_save();
            }
            exits.write(AppExit::Success);
        },
    );
}
