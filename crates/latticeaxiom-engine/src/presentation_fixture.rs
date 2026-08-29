//! Temporary client-only presentation fixture used by the thin binary.

use bevy::prelude::*;

use crate::video::VideoRuntimePlugin;
use crate::{EngineInstanceError, instance::reserve_client_event_loop};

/// Runs the temporary D0 client presentation fixture.
///
/// This scene is deliberately separate from [`crate::EngineInstance`]. It is not
/// authoritative package content, is never installed in headless apps, and
/// will be replaced once the client binary receives prepared profile images.
///
/// # Errors
///
/// Returns [`EngineInstanceError::ClientInstanceAlreadyExists`] after the
/// process-global client event-loop slot has been reserved.
pub fn run_temporary_client_presentation_fixture() -> Result<(), EngineInstanceError> {
    reserve_client_event_loop()?;
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.025, 0.035, 0.055)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Lattice Axiom - temporary D0 presentation fixture".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(VideoRuntimePlugin)
        .add_systems(Startup, setup_temporary_scene)
        .run();
    Ok(())
}

fn setup_temporary_scene(
    mut commands: Commands<'_, '_>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
) {
    commands.spawn((
        Name::new("Temporary Fixture Ground"),
        Mesh3d(meshes.add(Cuboid::new(12.0, 0.2, 12.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.16, 0.19, 0.23))),
        Transform::from_xyz(0.0, -0.1, 0.0),
    ));

    commands.spawn((
        Name::new("Temporary Fixture Cube"),
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.28, 0.58, 0.95))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));

    commands.spawn((
        Name::new("Temporary Fixture Light"),
        PointLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    commands.spawn((
        Name::new("Temporary Fixture Camera"),
        Camera3d::default(),
        Transform::from_xyz(5.0, 4.0, 8.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
    ));
}
