//! Client-only camera and lighting for the production host.

use bevy::{
    asset::Assets,
    prelude::{
        AmbientLight, Camera, Camera3d, ClearColorConfig, Color, Commands, Component,
        DirectionalLight, DistanceFog, EulerRot, FogFalloff, Image, Name, Quat, Query, Res, ResMut,
        StandardMaterial, Transform, Vec3, With, Without,
    },
    render::view::ColorGrading,
};
use latticeaxiom_content::FluidStateV1;
use latticeaxiom_gameplay::BlockPosition;
use latticeaxiom_player::{LocalPlayerInput, PlayerMovementProfileV1, PlayerViewV1};

use super::chunk_mesh::{
    ProductionTerrainMaterials, ProductionTerrainPalette, nearest_clamp_sampler,
};
use super::{CellOccupancyV1, ProductionSpine};

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

type ProductionCameraQuery<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static mut Transform,
        &'static mut Camera,
        &'static mut CameraMediumV1,
        &'static mut DistanceFog,
        &'static mut ColorGrading,
    ),
    (With<ProductionCamera>, Without<LocalPlayerInput>),
>;

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
    commands.insert_resource(ProductionTerrainMaterials::from_atlas(
        &mut materials,
        &atlas,
    ));
    commands.spawn((
        Name::new("Production Camera"),
        ProductionCamera,
        CameraMediumV1::Air,
        Camera3d::default(),
        air_fog(),
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

/// Synchronizes the production camera with the local player's eye pose.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_camera(
    spine: Res<'_, ProductionSpine>,
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
    let Some((mut camera_transform, mut camera, mut medium, mut fog, mut grading)) =
        cameras.iter_mut().next()
    else {
        return;
    };

    let eye_offset = profile.eye_height_m() - profile.capsule_total_height_m() * 0.5;
    camera_transform.translation = player_transform.translation + Vec3::Y * eye_offset;
    camera_transform.rotation =
        Quat::from_rotation_y(view.yaw_radians()) * Quat::from_rotation_x(view.pitch_radians());

    let next_medium = authoritative_camera_medium(&spine, *medium, camera_transform.translation);
    if *medium != next_medium {
        *medium = next_medium;
        apply_medium_presentation(next_medium, &mut camera, &mut fog, &mut grading);
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

fn air_fog() -> DistanceFog {
    DistanceFog {
        color: Color::NONE,
        falloff: FogFalloff::from_visibility(512.0),
        ..DistanceFog::default()
    }
}

fn apply_medium_presentation(
    medium: CameraMediumV1,
    camera: &mut Camera,
    fog: &mut DistanceFog,
    grading: &mut ColorGrading,
) {
    *grading = ColorGrading::default();
    match medium {
        CameraMediumV1::Air => {
            camera.clear_color = ClearColorConfig::Default;
            *fog = air_fog();
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

    use super::{CameraMediumV1, FluidSurfaceSample, fluid_fill_height, resolve_camera_medium};

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
}
