//! Development-only 3D presentation for the local playable slice.

use std::collections::BTreeMap;

use avian3d::prelude::{Collider, RigidBody};
use bevy::prelude::*;
use latticeaxiom_gameplay::{BlockId, BlockPosition, PlayerId};
use latticeaxiom_player::{
    BlockEditReceiptV1, D2PlayerBundle, LocalPlayerInput, PlayerMovementProfileV1, PlayerViewV1,
};

use super::authority::PlayableSeedCells;

const GRASS_BLOCK_ID: &str = "terrenia:block/grass";
const DIRT_BLOCK_ID: &str = "terrenia:block/dirt";
const SAND_BLOCK_ID: &str = "terrenia:block/sand";

/// Marks the camera driven by the local playable-slice player.
#[derive(Component, Debug, Default)]
pub(super) struct PlayableCamera;

/// Shared presentation assets and the live voxel-to-entity projection.
#[derive(Debug, Resource)]
pub(super) struct PlayableSceneState {
    blocks: BTreeMap<BlockPosition, Entity>,
    block_mesh: Handle<Mesh>,
    grass_material: Handle<StandardMaterial>,
    dirt_material: Handle<StandardMaterial>,
    stone_material: Handle<StandardMaterial>,
    sand_material: Handle<StandardMaterial>,
}

impl PlayableSceneState {
    fn material_for(&self, block: &BlockId) -> Handle<StandardMaterial> {
        match block.as_str() {
            GRASS_BLOCK_ID => self.grass_material.clone(),
            DIRT_BLOCK_ID => self.dirt_material.clone(),
            SAND_BLOCK_ID => self.sand_material.clone(),
            // Stone is also the deliberate placeholder for any fixture block
            // that does not yet have a presentation-specific material.
            _ => self.stone_material.clone(),
        }
    }
}

/// Spawns the development voxel scene, local player, camera, and lighting.
///
/// This system projects only the trusted in-memory playable fixture. It is not
/// a package renderer or a durable-world activation boundary.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn setup_playable_scene(
    mut commands: Commands<'_, '_>,
    seed_cells: Res<'_, PlayableSeedCells>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
) {
    let mut scene = PlayableSceneState {
        blocks: BTreeMap::new(),
        block_mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        grass_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.24, 0.56, 0.18),
            perceptual_roughness: 1.0,
            ..default()
        }),
        dirt_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.39, 0.24, 0.12),
            perceptual_roughness: 1.0,
            ..default()
        }),
        stone_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.38, 0.41, 0.43),
            perceptual_roughness: 1.0,
            ..default()
        }),
        sand_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.76, 0.67, 0.42),
            perceptual_roughness: 1.0,
            ..default()
        }),
    };

    for (position, block) in seed_cells.iter() {
        let entity = spawn_block(&mut commands, &scene, position, block);
        scene.blocks.insert(position, entity);
    }

    let player_center = Vec3::new(0.5, 3.0, 6.0);
    commands.spawn((
        Name::new("Playable Local Player"),
        D2PlayerBundle::new(PlayerId::new(1), Transform::from_translation(player_center)),
    ));

    let profile = PlayerMovementProfileV1::default();
    let eye_offset = profile.eye_height_m() - profile.capsule_total_height_m() * 0.5;
    commands.spawn((
        Name::new("Playable Camera"),
        PlayableCamera,
        Camera3d::default(),
        AmbientLight {
            color: Color::srgb(0.72, 0.78, 0.88),
            brightness: 90.0,
            ..default()
        },
        Transform::from_translation(player_center + Vec3::Y * eye_offset),
    ));

    commands.spawn((
        Name::new("Playable Sun"),
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.7, 0.0)),
    ));

    commands.insert_resource(scene);
}

/// Applies successful authoritative edit receipts to visible blocks and their
/// static colliders.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn apply_playable_block_receipts(
    mut commands: Commands<'_, '_>,
    mut receipts: MessageReader<'_, '_, BlockEditReceiptV1>,
    mut scene: ResMut<'_, PlayableSceneState>,
) {
    for receipt in receipts.read() {
        let Ok(success) = &receipt.result else {
            continue;
        };

        if let Some(entity) = scene.blocks.remove(&success.position) {
            commands.entity(entity).despawn();
        }

        if let Some(block) = &success.new_content {
            let entity = spawn_block(&mut commands, &scene, success.position, block);
            scene.blocks.insert(success.position, entity);
        }
    }
}

/// Synchronizes the playable camera with the local player's authoritative eye
/// position and Y-up yaw/pitch view.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_playable_camera(
    players: Query<
        '_,
        '_,
        (&Transform, &PlayerMovementProfileV1, &PlayerViewV1),
        With<LocalPlayerInput>,
    >,
    mut cameras: Query<'_, '_, &mut Transform, (With<PlayableCamera>, Without<LocalPlayerInput>)>,
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

#[allow(clippy::cast_precision_loss)] // The development fixture bounds coordinates near zero.
fn spawn_block(
    commands: &mut Commands<'_, '_>,
    scene: &PlayableSceneState,
    position: BlockPosition,
    block: &BlockId,
) -> Entity {
    commands
        .spawn((
            Name::new("Playable Block"),
            Mesh3d(scene.block_mesh.clone()),
            MeshMaterial3d(scene.material_for(block)),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
            Transform::from_xyz(
                position.x as f32 + 0.5,
                position.y as f32 + 0.5,
                position.z as f32 + 0.5,
            ),
        ))
        .id()
}
