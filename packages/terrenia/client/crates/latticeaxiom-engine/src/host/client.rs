//! Client-only camera and lighting for the production host.

use bevy::{
    asset::Assets,
    core_pipeline::prepass::DepthPrepass,
    prelude::{
        AmbientLight, Camera, Camera3d, Changed, ClearColorConfig, Color, Commands, Component,
        DirectionalLight, DistanceFog, EulerRot, FogFalloff, Image, Name, Projection, Quat, Query,
        Res, ResMut, StandardMaterial, Transform, Vec3, With, Without,
    },
    render::view::ColorGrading,
};
use latticeaxiom_content::FluidStateV1;
use latticeaxiom_gameplay::BlockPosition;
use latticeaxiom_player::{LocalPlayerInput, PlayerMovementProfileV1, PlayerViewV1};

use super::chunk_mesh::{
    ProductionTerrainMaterials, ProductionTerrainPalette, nearest_clamp_sampler,
};
use super::far_mesh::{FarTerrainPresentationStatusV1, FarTerrainRolePalette};
use super::water_material::{WaterMaterial, water_normal_image};
use super::{CellOccupancyV1, ProductionSpine, TerrainDistanceStatusV1};

use crate::EngineProfile;

/// Marks the camera driven by the local production-host player.
#[derive(Component, Debug, Default)]
pub(super) struct ProductionCamera;

/// Authoritative fluid medium occupied by the production camera eye.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) enum CameraMediumV1 {
    /// The eye is not submerged in a recognized fluid.
    #[default]
    Air,
    /// The eye is submerged in water.
    Water,
    /// The eye is submerged in lava.
    Lava,
}

const WATERLINE_HYSTERESIS_M: f32 = 0.04;
const DEFAULT_AIR_FOG_VISIBILITY_M: f32 = 512.0;
const DEFAULT_CAMERA_FAR_M: f32 = 1_000.0;

/// Presentation range derived from the effective streamed world radius.
#[derive(Clone, Copy, Component, Debug, PartialEq)]
pub(super) struct ProductionCameraViewRangeV1 {
    air_fog_visibility_m: f32,
    far_plane_m: f32,
}

impl Default for ProductionCameraViewRangeV1 {
    fn default() -> Self {
        Self {
            air_fog_visibility_m: DEFAULT_AIR_FOG_VISIBILITY_M,
            far_plane_m: DEFAULT_CAMERA_FAR_M,
        }
    }
}

type ProductionCameraQuery<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static mut Transform,
        &'static mut Camera,
        &'static mut CameraMediumV1,
        &'static mut DistanceFog,
        &'static mut ColorGrading,
        &'static mut Projection,
        &'static mut ProductionCameraViewRangeV1,
    ),
    (With<ProductionCamera>, Without<LocalPlayerInput>),
>;

/// Spawns the first-person camera and lighting used by the interactive client.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn spawn_production_client_view(
    mut commands: Commands<'_, '_>,
    profile: Res<'_, EngineProfile>,
    spine: Res<'_, ProductionSpine>,
    mut standard_materials: ResMut<'_, Assets<StandardMaterial>>,
    mut water_materials: ResMut<'_, Assets<WaterMaterial>>,
    mut images: ResMut<'_, Assets<Image>>,
    presentation: (
        ResMut<'_, ProductionTerrainPalette>,
        Option<Res<'_, crate::resource_packs::ClientResourcePacks>>,
    ),
) {
    if *profile != EngineProfile::Client {
        return;
    }

    let (mut palette, resources) = presentation;
    let empty = latticeaxiom_render_contracts::ResolvedResourcePacks::default();
    let resources = resources.as_ref().map_or(&empty, |value| &value.0);
    palette.apply_resource_packs(resources);
    let mut atlas_image = palette.atlas_image();
    atlas_image.sampler = nearest_clamp_sampler();
    let atlas = images.add(atlas_image);
    let water_normal_map = images.add(water_normal_image());
    commands.insert_resource(ProductionTerrainMaterials::from_atlas(
        &mut standard_materials,
        &mut water_materials,
        &atlas,
        &water_normal_map,
    ));
    commands.insert_resource(FarTerrainRolePalette::from_blocks(
        &spine.far_terrain_role_blocks(),
        resources,
    ));
    commands.spawn((
        Name::new("Production Camera"),
        ProductionCamera,
        CameraMediumV1::Air,
        ProductionCameraViewRangeV1::default(),
        Camera3d::default(),
        DepthPrepass,
        air_fog(DEFAULT_AIR_FOG_VISIBILITY_M),
        ColorGrading::default(),
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

/// Mirrors the authoritative camera medium into the shared water material.
///
/// This selects the below-surface absorption and alpha parameters without
/// inferring submersion from visibility, face winding, or screen position.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_water_material_medium(
    cameras: Query<
        '_,
        '_,
        &'static CameraMediumV1,
        (With<ProductionCamera>, Changed<CameraMediumV1>),
    >,
    terrain_materials: Res<'_, ProductionTerrainMaterials>,
    mut water_materials: ResMut<'_, Assets<WaterMaterial>>,
) {
    let Some(medium) = cameras.iter().next() else {
        return;
    };
    let Some(mut material) = water_materials.get_mut(terrain_materials.water_handle()) else {
        return;
    };
    material
        .extension
        .set_camera_underwater(matches!(medium, CameraMediumV1::Water));
}

/// Synchronizes the production camera with the local player's eye pose.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_camera(
    spine: Res<'_, ProductionSpine>,
    far_presentation: Option<Res<'_, FarTerrainPresentationStatusV1>>,
    players: Query<
        '_,
        '_,
        (&Transform, &PlayerMovementProfileV1, &PlayerViewV1),
        With<LocalPlayerInput>,
    >,
    mut cameras: ProductionCameraQuery<'_, '_>,
) {
    let Some((player_transform, profile, view)) = players.iter().next() else {
        return;
    };
    let Some((
        mut camera_transform,
        mut camera,
        mut medium,
        mut fog,
        mut grading,
        mut projection,
        mut view_range,
    )) = cameras.iter_mut().next()
    else {
        return;
    };

    let eye_offset = profile.eye_height_m() - profile.capsule_total_height_m() * 0.5;
    camera_transform.translation = player_transform.translation + Vec3::Y * eye_offset;
    camera_transform.rotation =
        Quat::from_rotation_y(view.yaw_radians()) * Quat::from_rotation_x(view.pitch_radians());

    let next_view_range = spine.terrain_distance_status().map_or_else(
        ProductionCameraViewRangeV1::default,
        |status| {
            camera_view_range(
                status,
                far_presentation
                    .as_ref()
                    .and_then(|presentation| presentation.contiguous_meters()),
            )
        },
    );
    if *view_range != next_view_range {
        *view_range = next_view_range;
        apply_camera_view_range(*view_range, *medium, &mut projection, &mut fog);
    }

    let next_medium = authoritative_camera_medium(&spine, *medium, camera_transform.translation);
    if *medium != next_medium {
        *medium = next_medium;
        apply_medium_presentation(
            next_medium,
            *view_range,
            &mut camera,
            &mut fog,
            &mut grading,
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FluidSurfaceSample {
    medium: CameraMediumV1,
    surface_y_m: f32,
}

fn authoritative_camera_medium(
    spine: &ProductionSpine,
    previous: CameraMediumV1,
    eye: Vec3,
) -> CameraMediumV1 {
    let Some(position) = eye_block_position(eye) else {
        return previous;
    };
    let current = spine.inspect_occupancy(position);
    let below = position.y.checked_sub(1).map(|y| {
        spine.inspect_occupancy(BlockPosition {
            x: position.x,
            y,
            z: position.z,
        })
    });
    if current.is_err() && below.as_ref().is_none_or(Result::is_err) {
        return previous;
    }

    let surface = current
        .ok()
        .as_ref()
        .and_then(fluid_surface_sample)
        .into_iter()
        .chain(
            below
                .and_then(Result::ok)
                .as_ref()
                .and_then(fluid_surface_sample),
        )
        .max_by(|left, right| left.surface_y_m.total_cmp(&right.surface_y_m));
    resolve_camera_medium(previous, eye.y, surface)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "finite f32 world coordinates use Rust's saturating float-to-integer conversion"
)]
fn eye_block_position(eye: Vec3) -> Option<BlockPosition> {
    if !eye.is_finite() {
        return None;
    }
    Some(BlockPosition {
        x: eye.x.floor() as i32,
        y: eye.y.floor() as i32,
        z: eye.z.floor() as i32,
    })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "camera and presentation coordinates already use f32 world space"
)]
fn fluid_surface_sample(occupancy: &CellOccupancyV1) -> Option<FluidSurfaceSample> {
    let fluid = occupancy.fluid.as_ref()?;
    if fluid.kind() != "fluid" {
        return None;
    }
    let medium = match fluid.path() {
        "water" => CameraMediumV1::Water,
        "lava" => CameraMediumV1::Lava,
        _ => return None,
    };
    let state = occupancy.fluid_state?;
    Some(FluidSurfaceSample {
        medium,
        surface_y_m: occupancy.position.y as f32 + fluid_fill_height(state),
    })
}

fn fluid_fill_height(state: FluidStateV1) -> f32 {
    1.0 - f32::from(state.level.get()) / 8.0
}

fn resolve_camera_medium(
    previous: CameraMediumV1,
    eye_y_m: f32,
    surface: Option<FluidSurfaceSample>,
) -> CameraMediumV1 {
    let Some(surface) = surface else {
        return CameraMediumV1::Air;
    };
    let threshold = if previous == surface.medium {
        surface.surface_y_m + WATERLINE_HYSTERESIS_M
    } else {
        surface.surface_y_m - WATERLINE_HYSTERESIS_M
    };
    if eye_y_m <= threshold {
        surface.medium
    } else {
        CameraMediumV1::Air
    }
}

fn camera_view_range(
    status: TerrainDistanceStatusV1,
    gpu_presented_meters: Option<u32>,
) -> ProductionCameraViewRangeV1 {
    let cpu_presented_chunks = status.presented_render_distance().chunks();
    let cpu_presented_meters = status.presented_render_distance_meters().meters();
    let chunk_edge_meters = cpu_presented_meters
        .checked_div(cpu_presented_chunks)
        .unwrap_or(1)
        .max(1);
    let fail_closed_meters = status
        .full_detail_distance()
        .chunks()
        .saturating_mul(chunk_edge_meters);
    camera_view_range_from_values(
        gpu_presented_meters
            .unwrap_or(fail_closed_meters)
            .clamp(fail_closed_meters, cpu_presented_meters),
        status.target_render_distance().chunks(),
        chunk_edge_meters,
    )
}

#[expect(
    clippy::cast_precision_loss,
    reason = "bounded view distances are exactly representable in f32"
)]
fn camera_view_range_from_values(
    presented_meters: u32,
    target_chunks: u32,
    chunk_edge_meters: u32,
) -> ProductionCameraViewRangeV1 {
    let fog_visibility_m = presented_meters.saturating_add(chunk_edge_meters / 2);
    let far_plane_m = target_chunks
        .saturating_mul(chunk_edge_meters)
        .saturating_add(chunk_edge_meters)
        .saturating_mul(2);
    ProductionCameraViewRangeV1 {
        air_fog_visibility_m: fog_visibility_m as f32,
        far_plane_m: far_plane_m as f32,
    }
}

fn apply_camera_view_range(
    range: ProductionCameraViewRangeV1,
    medium: CameraMediumV1,
    projection: &mut Projection,
    fog: &mut DistanceFog,
) {
    if let Projection::Perspective(perspective) = projection {
        perspective.far = range.far_plane_m;
    }
    if medium == CameraMediumV1::Air {
        *fog = air_fog(range.air_fog_visibility_m);
    }
}

fn air_fog(visibility_m: f32) -> DistanceFog {
    DistanceFog {
        color: Color::NONE,
        falloff: FogFalloff::from_visibility(visibility_m),
        ..DistanceFog::default()
    }
}

fn apply_medium_presentation(
    medium: CameraMediumV1,
    view_range: ProductionCameraViewRangeV1,
    camera: &mut Camera,
    fog: &mut DistanceFog,
    grading: &mut ColorGrading,
) {
    *grading = ColorGrading::default();
    match medium {
        CameraMediumV1::Air => {
            camera.clear_color = ClearColorConfig::Default;
            *fog = air_fog(view_range.air_fog_visibility_m);
        }
        CameraMediumV1::Water => {
            let attenuation = Color::srgb(0.015, 0.12, 0.19);
            camera.clear_color = ClearColorConfig::Custom(attenuation);
            *fog = DistanceFog {
                color: attenuation,
                directional_light_color: Color::srgb(0.08, 0.32, 0.36),
                directional_light_exponent: 12.0,
                falloff: FogFalloff::from_visibility(22.0),
            };
            grading.global.exposure = -0.35;
            grading.global.temperature = -0.12;
            grading.global.tint = -0.06;
            grading.global.post_saturation = 0.82;
        }
        CameraMediumV1::Lava => {
            let attenuation = Color::srgb(0.24, 0.025, 0.006);
            camera.clear_color = ClearColorConfig::Custom(attenuation);
            *fog = DistanceFog {
                color: attenuation,
                directional_light_color: Color::srgb(1.0, 0.16, 0.015),
                directional_light_exponent: 6.0,
                falloff: FogFalloff::from_visibility(5.0),
            };
            grading.global.exposure = -0.2;
            grading.global.temperature = 0.18;
            grading.global.tint = 0.04;
            grading.global.post_saturation = 0.9;
        }
    }
}

#[cfg(test)]
mod tests {
    use latticeaxiom_content::{FluidFlowV1, FluidLevelV1, FluidStateV1};

    use super::{
        CameraMediumV1, FluidSurfaceSample, camera_view_range_from_values, fluid_fill_height,
        resolve_camera_medium,
    };

    fn surface(medium: CameraMediumV1, surface_y_m: f32) -> FluidSurfaceSample {
        FluidSurfaceSample {
            medium,
            surface_y_m,
        }
    }

    #[test]
    fn camera_medium_is_correct_above_at_and_below_the_waterline() {
        let waterline = 12.0;
        assert_eq!(
            resolve_camera_medium(
                CameraMediumV1::Air,
                waterline + 0.05,
                Some(surface(CameraMediumV1::Water, waterline)),
            ),
            CameraMediumV1::Air
        );
        assert_eq!(
            resolve_camera_medium(
                CameraMediumV1::Air,
                waterline,
                Some(surface(CameraMediumV1::Water, waterline)),
            ),
            CameraMediumV1::Air
        );
        assert_eq!(
            resolve_camera_medium(
                CameraMediumV1::Water,
                waterline,
                Some(surface(CameraMediumV1::Water, waterline)),
            ),
            CameraMediumV1::Water
        );
        assert_eq!(
            resolve_camera_medium(
                CameraMediumV1::Air,
                waterline - 0.05,
                Some(surface(CameraMediumV1::Water, waterline)),
            ),
            CameraMediumV1::Water
        );
    }

    #[test]
    fn waterline_hysteresis_prevents_medium_flicker() {
        let waterline = 4.0;
        assert_eq!(
            resolve_camera_medium(
                CameraMediumV1::Water,
                waterline + 0.03,
                Some(surface(CameraMediumV1::Water, waterline)),
            ),
            CameraMediumV1::Water
        );
        assert_eq!(
            resolve_camera_medium(
                CameraMediumV1::Water,
                waterline + 0.05,
                Some(surface(CameraMediumV1::Water, waterline)),
            ),
            CameraMediumV1::Air
        );
        assert_eq!(
            resolve_camera_medium(
                CameraMediumV1::Air,
                waterline - 0.03,
                Some(surface(CameraMediumV1::Water, waterline)),
            ),
            CameraMediumV1::Air
        );
    }

    #[test]
    fn medium_classification_needs_no_surface_visibility_input() {
        assert_eq!(
            resolve_camera_medium(
                CameraMediumV1::Air,
                2.9,
                Some(surface(CameraMediumV1::Lava, 3.0)),
            ),
            CameraMediumV1::Lava
        );
        assert_eq!(
            resolve_camera_medium(CameraMediumV1::Water, 2.9, None),
            CameraMediumV1::Air
        );
    }

    #[test]
    fn accepted_fluid_levels_have_monotone_presentation_heights() {
        let height = |level| {
            fluid_fill_height(FluidStateV1 {
                level: FluidLevelV1::new(level).expect("fixture level is in range"),
                flow: FluidFlowV1::Still,
            })
        };
        assert!((height(0) - 1.0).abs() < f32::EPSILON);
        assert!((height(7) - 0.125).abs() < f32::EPSILON);
        for level in 0..FluidLevelV1::MAX {
            assert!(height(level) > height(level + 1));
        }
    }

    #[test]
    fn camera_range_tracks_presented_chunks_in_world_meters() {
        let six_of_twenty_one = camera_view_range_from_values(192, 21, 32);
        let four_chunks = camera_view_range_from_values(128, 4, 32);
        assert!((six_of_twenty_one.air_fog_visibility_m - 208.0).abs() < f32::EPSILON);
        assert!((six_of_twenty_one.far_plane_m - 1_408.0).abs() < f32::EPSILON);
        assert!((four_chunks.air_fog_visibility_m - 144.0).abs() < f32::EPSILON);
        assert!((four_chunks.far_plane_m - 320.0).abs() < f32::EPSILON);
        assert!(four_chunks.air_fog_visibility_m < six_of_twenty_one.air_fog_visibility_m);
        assert!(four_chunks.far_plane_m < six_of_twenty_one.far_plane_m);
    }
}
