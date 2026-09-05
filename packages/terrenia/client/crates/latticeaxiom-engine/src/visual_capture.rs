//! Opt-in native GPU capture for visual acceptance. Output is never source content.

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
    text: Option<String>,
    typed: bool,
    action: Option<String>,
    acted: bool,
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
    app.insert_resource(CaptureRequest {
        path,
        requested: false,
        text: std::env::var("LATTICEAXIOM_CAPTURE_TEXT").ok(),
        typed: false,
        action: std::env::var("LATTICEAXIOM_CAPTURE_ACTION").ok(),
        acted: false,
    })
    .add_systems(
        Update,
        (
            inject_capture_text,
            inject_capture_action,
            capture_once,
            capture_scene_probe,
        ),
    );
}

#[allow(clippy::needless_pass_by_value)]
fn capture_scene_probe(
    frame: Res<'_, FrameCount>,
    request: Res<'_, CaptureRequest>,
    mut written: bevy::prelude::Local<'_, bool>,
    spine: Option<Res<'_, crate::ProductionSpine>>,
    cameras: bevy::prelude::Query<
        '_,
        '_,
        (&bevy::prelude::Transform, &bevy::camera::Projection),
        bevy::prelude::With<bevy::prelude::Camera3d>,
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
    let report = serde_json::json!({"frame": frame.0, "world": spine.world_id(), "chunk_edge": spine.chunk_edge(), "player": pose.translation.to_array(),
        "resident": format!("{:?}", spine.resident_chunks()), "cameras": camera_rows, "mesh_count": meshes.iter().count(), "meshes": mesh_rows});
    if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
        let _ = std::fs::write(request.path.with_extension("json"), bytes);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn inject_capture_action(
    mut commands: Commands<'_, '_>,
    frame: Res<'_, FrameCount>,
    mut request: ResMut<'_, CaptureRequest>,
    controls: bevy::prelude::Query<
        '_,
        '_,
        (bevy::prelude::Entity, &bevy::prelude::Name),
        bevy::prelude::With<bevy::ui_widgets::Button>,
    >,
) {
    if frame.0 < 65 || request.acted {
        return;
    }
    let Some(action) = &request.action else {
        return;
    };
    if let Some((entity, _)) = controls.iter().find(|(_, name)| name.as_str() == action) {
        commands.trigger(bevy::ui_widgets::Activate { entity });
        request.acted = true;
    }
}

#[allow(clippy::needless_pass_by_value)]
fn inject_capture_text(
    frame: Res<'_, FrameCount>,
    mut request: ResMut<'_, CaptureRequest>,
    windows: bevy::prelude::Query<
        '_,
        '_,
        bevy::prelude::Entity,
        bevy::prelude::With<bevy::window::PrimaryWindow>,
    >,
    mut input: MessageWriter<'_, bevy::input::keyboard::KeyboardInput>,
) {
    if frame.0 < 45 || request.typed {
        return;
    }
    let Some(text) = request.text.clone() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    input.write(bevy::input::keyboard::KeyboardInput {
        key_code: bevy::input::keyboard::KeyCode::KeyA,
        logical_key: bevy::input::keyboard::Key::Character(text.clone().into()),
        state: bevy::input::ButtonState::Pressed,
        text: Some(text.into()),
        repeat: false,
        window,
    });
    request.typed = true;
}

#[allow(clippy::needless_pass_by_value)]
fn capture_once(
    mut commands: Commands<'_, '_>,
    frame: Res<'_, FrameCount>,
    time: Res<'_, bevy::prelude::Time<bevy::time::Real>>,
    mut request: ResMut<'_, CaptureRequest>,
) {
    if frame.0 < 90 || time.elapsed_secs() < 3.0 || request.requested {
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
