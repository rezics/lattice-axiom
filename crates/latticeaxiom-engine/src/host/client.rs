//! Client-only camera and lighting for the production host.

use bevy::{
    asset::Assets,
    prelude::{
        AmbientLight, Camera3d, Color, Commands, Component, DirectionalLight, EulerRot, Image,
        Name, Quat, Query, Res, ResMut, StandardMaterial, Transform, Vec3, With, Without,
    },
};
use latticeaxiom_player::{LocalPlayerInput, PlayerMovementProfileV1, PlayerViewV1};

use super::chunk_mesh::{
    ProductionTerrainMaterial, ProductionTerrainPalette, nearest_clamp_sampler,
};

use crate::EngineProfile;

/// Marks the camera driven by the local production-host player.
#[derive(Component, Debug, Default)]
pub(super) struct ProductionCamera;

/// Spawns the first-person camera and lighting used by the interactive client.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn spawn_production_client_view(
    mut commands: Commands<'_, '_>,
    profile: Res<'_, EngineProfile>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
    mut images: ResMut<'_, Assets<Image>>,
    palette: Res<'_, ProductionTerrainPalette>,
) {
    if *profile != EngineProfile::Client {
        return;
    }

    let mut atlas_image = palette.atlas_image();
    atlas_image.sampler = nearest_clamp_sampler();
    let atlas = images.add(atlas_image);
    commands.insert_resource(ProductionTerrainMaterial(materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(atlas),
        perceptual_roughness: 0.95,
        reflectance: 0.06,
        ..StandardMaterial::default()
    })));
    commands.spawn((
        Name::new("Production Camera"),
        ProductionCamera,
        Camera3d::default(),
        AmbientLight {
            color: Color::srgb(0.72, 0.78, 0.88),
            brightness: 90.0,
            ..AmbientLight::default()
        },
        Transform::from_xyz(0.0, 2.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Name::new("Production Sun"),
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..DirectionalLight::default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.7, 0.0)),
    ));
}

/// Synchronizes the production camera with the local player's eye pose.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_camera(
    players: Query<
        '_,
        '_,
        (&Transform, &PlayerMovementProfileV1, &PlayerViewV1),
        With<LocalPlayerInput>,
    >,
    mut cameras: Query<'_, '_, &mut Transform, (With<ProductionCamera>, Without<LocalPlayerInput>)>,
) {
    let Some((player_transform, profile, view)) = players.iter().next() else {
        return;
    };
    let Some(mut camera_transform) = cameras.iter_mut().next() else {
        return;
    };

    let eye_offset = profile.eye_height_m() - profile.capsule_total_height_m() * 0.5;
    camera_transform.translation = player_transform.translation + Vec3::Y * eye_offset;
    camera_transform.rotation =
        Quat::from_rotation_y(view.yaw_radians()) * Quat::from_rotation_x(view.pitch_radians());
}
