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
    })
    .add_systems(Update, (inject_capture_text, capture_once));
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
    mut request: ResMut<'_, CaptureRequest>,
) {
    if frame.0 < 90 || request.requested {
        return;
    }
    request.requested = true;
    let mut save = save_to_disk(request.path.clone());
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<'_, '_, ScreenshotCaptured>, mut exits: MessageWriter<'_, AppExit>| {
            save(captured);
            exits.write(AppExit::Success);
        },
    );
}
