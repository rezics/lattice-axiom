//! Native pause, input ownership and runtime settings for the Web client.

use super::{ProductionSessionPause, ProductionSpine, hud::ProductionHudSurfaces};
use crate::{
    cursor_capture::{
        ConfirmedPrimaryWindowFocus, CursorCaptureGesture, CursorCaptureState,
        apply_cursor_capture, screenshot_release_requested,
    },
    settings::{HostSettingsCatalog, HostSettingsError, HostUserSettings},
    video::{FrameRateLimit, VideoRuntimeSettings},
};
use avian3d::prelude::LinearVelocity;
use bevy::{
    ecs::system::NonSendMarker,
    input::{ButtonInput, keyboard::KeyCode, mouse::MouseButton},
    prelude::{Entity, Query, Res, ResMut, Resource, With},
    window::{CursorOptions, PrimaryWindow, Window},
};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_input::ClientSurfaceActionV1;
use latticeaxiom_player::{
    ActionFrameInbox, ActionState, ClientInputOwnership, D2Player, LeafwingPlayerAction,
    LocalPlayerInput, SimulationTickRate, SimulationTickRateRequest, SurfaceActionFrame,
};
use latticeaxiom_runtime_contracts::{
    EffectiveSettingsSnapshot, FarTerrainQualityV1, RequestedFullDetailDistanceChunksV1,
    RequestedRenderDistanceChunksV1, RequestedSimulationDistanceChunksV1,
    TerrainDistanceRequestsV1, distant_terrain_quality_setting_id, full_detail_distance_setting_id,
    render_distance_setting_id, simulation_distance_setting_id, simulation_tick_rate_setting_id,
    video_background_frame_rate_limit_setting_id, video_frame_rate_limit_setting_id,
    video_vsync_setting_id,
};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug)]
struct AppliedRuntimeSettings {
    terrain_distances: TerrainDistanceRequestsV1,
    far_terrain_quality: FarTerrainQualityV1,
    video: VideoRuntimeSettings,
    tick_rate: SimulationTickRate,
}

/// Runtime initialization snapshot; Web settings own drafts and publication.
#[derive(Debug, Resource)]
pub(super) struct ProductionSettingsState {
    runtime: AppliedRuntimeSettings,
}

impl ProductionSettingsState {
    pub(super) fn new(
        _root: PathBuf,
        user: HostUserSettings,
        catalog: HostSettingsCatalog,
        active_lock: CanonicalHash,
    ) -> Result<Self, HostSettingsError> {
        Ok(Self {
            runtime: runtime_settings_from_snapshot(
                &user.effective_snapshot(&catalog, active_lock)?,
            )?,
        })
    }
    pub(super) const fn applied_terrain_distances(&self) -> TerrainDistanceRequestsV1 {
        self.runtime.terrain_distances
    }
    pub(super) const fn applied_far_terrain_quality(&self) -> FarTerrainQualityV1 {
        self.runtime.far_terrain_quality
    }
    pub(super) const fn applied_video(&self) -> VideoRuntimeSettings {
        self.runtime.video
    }
    pub(super) const fn applied_tick_rate(&self) -> SimulationTickRate {
        self.runtime.tick_rate
    }
}

/// Validates a complete Web settings draft against the native runtime domains.
pub(super) fn validate_web_runtime_values(
    values: &BTreeMap<StableId, serde_json::Value>,
) -> Result<(), HostSettingsError> {
    web_runtime_settings(values).map(|_| ())
}

/// Applies a previously validated settings snapshot at the host update boundary.
pub(super) fn apply_web_runtime_values(
    world: &mut bevy::prelude::World,
    values: &BTreeMap<StableId, serde_json::Value>,
    request_id: u64,
) -> Result<(), HostSettingsError> {
    let requested = web_runtime_settings(values)?;
    if let Some(spine) = world.get_resource::<ProductionSpine>() {
        spine
            .set_terrain_presentation(requested.terrain_distances, requested.far_terrain_quality)
            .map_err(|error| HostSettingsError::Runtime {
                reason: error.to_string(),
            })?;
    }
    if let Some(mut video) = world.get_resource_mut::<VideoRuntimeSettings>() {
        video.replace(
            requested.video.vsync(),
            requested.video.foreground_limit(),
            requested.video.background_limit(),
        );
    }
    if let Some(mut requests) =
        world.get_resource_mut::<bevy::ecs::message::Messages<SimulationTickRateRequest>>()
    {
        requests.write(SimulationTickRateRequest {
            request_id,
            rate: requested.tick_rate,
        });
    }
    Ok(())
}

fn web_runtime_settings(
    values: &BTreeMap<StableId, serde_json::Value>,
) -> Result<AppliedRuntimeSettings, HostSettingsError> {
    runtime_settings_from_proposed(
        AppliedRuntimeSettings {
            terrain_distances: TerrainDistanceRequestsV1::default(),
            far_terrain_quality: FarTerrainQualityV1::Balanced,
            video: VideoRuntimeSettings::default(),
            tick_rate: SimulationTickRate::default(),
        },
        values,
    )
}

fn runtime_settings_from_snapshot(
    snapshot: &EffectiveSettingsSnapshot,
) -> Result<AppliedRuntimeSettings, HostSettingsError> {
    let values = snapshot
        .values()
        .iter()
        .map(|(id, effective)| (id.clone(), effective.value.clone()))
        .collect();
    runtime_settings_from_proposed(
        AppliedRuntimeSettings {
            terrain_distances: TerrainDistanceRequestsV1::default(),
            far_terrain_quality: FarTerrainQualityV1::Balanced,
            video: VideoRuntimeSettings::default(),
            tick_rate: SimulationTickRate::default(),
        },
        &values,
    )
}

fn runtime_settings_from_proposed(
    prior: AppliedRuntimeSettings,
    proposed: &BTreeMap<StableId, serde_json::Value>,
) -> Result<AppliedRuntimeSettings, HostSettingsError> {
    let terrain_distances =
        terrain_distance_requests_from_proposed(prior.terrain_distances, proposed)?;
    let far_terrain_quality = proposed.get(&distant_terrain_quality_setting_id()).map_or(
        Ok(prior.far_terrain_quality),
        |value| {
            value
                .as_str()
                .and_then(FarTerrainQualityV1::parse)
                .ok_or_else(|| HostSettingsError::Runtime {
                    reason: "distant terrain quality is not a supported enum value".to_owned(),
                })
        },
    )?;
    let vsync = proposed
        .get(&video_vsync_setting_id())
        .map_or(Some(prior.video.vsync()), serde_json::Value::as_bool)
        .ok_or_else(|| HostSettingsError::Runtime {
            reason: "vertical synchronization setting is not a boolean".to_owned(),
        })?;
    let foreground_limit = proposed.get(&video_frame_rate_limit_setting_id()).map_or(
        Ok(prior.video.foreground_limit()),
        |value| {
            value
                .as_str()
                .ok_or_else(|| HostSettingsError::Runtime {
                    reason: "foreground frame limit is not a string".to_owned(),
                })
                .and_then(|value| {
                    FrameRateLimit::parse(value).map_err(|error| HostSettingsError::Runtime {
                        reason: error.to_string(),
                    })
                })
        },
    )?;
    let background_limit = proposed
        .get(&video_background_frame_rate_limit_setting_id())
        .map_or(Ok(prior.video.background_limit()), |value| {
            value
                .as_u64()
                .and_then(|value| u16::try_from(value).ok())
                .ok_or_else(|| HostSettingsError::Runtime {
                    reason: "background frame limit is outside the unsigned 16-bit domain"
                        .to_owned(),
                })
                .and_then(|value| {
                    FrameRateLimit::capped(value).map_err(|error| HostSettingsError::Runtime {
                        reason: error.to_string(),
                    })
                })
        })?;
    let tick_rate =
        proposed
            .get(&simulation_tick_rate_setting_id())
            .map_or(Ok(prior.tick_rate), |value| {
                value
                    .as_u64()
                    .and_then(|value| u16::try_from(value).ok())
                    .ok_or_else(|| HostSettingsError::Runtime {
                        reason: "simulation tick rate is outside the unsigned 16-bit domain"
                            .to_owned(),
                    })
                    .and_then(|value| {
                        SimulationTickRate::new(value).map_err(|error| HostSettingsError::Runtime {
                            reason: error.to_string(),
                        })
                    })
            })?;
    Ok(AppliedRuntimeSettings {
        terrain_distances,
        far_terrain_quality,
        video: VideoRuntimeSettings::new(vsync, foreground_limit, background_limit),
        tick_rate,
    })
}

fn terrain_distance_requests_from_proposed(
    prior: TerrainDistanceRequestsV1,
    proposed: &BTreeMap<StableId, serde_json::Value>,
) -> Result<TerrainDistanceRequestsV1, HostSettingsError> {
    let render = requested_chunk_distance(
        proposed,
        &render_distance_setting_id(),
        prior.render(),
        "render distance",
        RequestedRenderDistanceChunksV1::new,
    )?;
    let simulation = requested_chunk_distance(
        proposed,
        &simulation_distance_setting_id(),
        prior.simulation(),
        "simulation distance",
        RequestedSimulationDistanceChunksV1::new,
    )?;
    let full_detail = requested_chunk_distance(
        proposed,
        &full_detail_distance_setting_id(),
        prior.full_detail(),
        "full-detail distance",
        RequestedFullDetailDistanceChunksV1::new,
    )?;
    Ok(TerrainDistanceRequestsV1::new(
        render,
        simulation,
        full_detail,
    ))
}

fn requested_chunk_distance<T: Copy>(
    proposed: &BTreeMap<StableId, serde_json::Value>,
    setting: &StableId,
    prior: T,
    label: &'static str,
    constructor: fn(u32) -> Option<T>,
) -> Result<T, HostSettingsError> {
    proposed.get(setting).map_or(Ok(prior), |value| {
        value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .and_then(constructor)
            .ok_or_else(|| HostSettingsError::Runtime {
                reason: format!("{label} is outside the non-zero unsigned 32-bit domain"),
            })
    })
}

/// Toggles the in-session pause overlay from the Pause action or Escape.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn toggle_pause(
    action_states: Query<'_, '_, &ActionState<LeafwingPlayerAction>, With<LocalPlayerInput>>,
    surface: Option<Res<'_, SurfaceActionFrame>>,
    ownership: Res<'_, ClientInputOwnership>,
    mut pause: ResMut<'_, ProductionSessionPause>,
    mut router: Option<ResMut<'_, super::ProductionSurfaceRouter>>,
    persistent: Option<Res<'_, super::persistent::PersistentGameSession>>,
) {
    if persistent
        .as_ref()
        .is_some_and(|session| session.shutdown_requested())
    {
        return;
    }
    if *ownership == ClientInputOwnership::Released {
        return;
    }
    if surface.is_some_and(|frame| frame.just_started(ClientSurfaceActionV1::Pause)) {
        return;
    }
    let from_action = action_states
        .iter()
        .any(|state| state.just_pressed(&LeafwingPlayerAction::Pause));
    if !from_action {
        return;
    }
    if let Some(router) = router.as_mut() {
        let command = if pause.is_paused() {
            latticeaxiom_client_ui::SurfaceCommandV1::Back
        } else {
            latticeaxiom_client_ui::SurfaceCommandV1::Pause
        };
        if let Ok(receipt) = router.apply(&command) {
            pause.set(matches!(
                receipt.route.modal(),
                latticeaxiom_client_ui::GameModalV1::Pause
                    | latticeaxiom_client_ui::GameModalV1::Settings
                    | latticeaxiom_client_ui::GameModalV1::ConfirmSaveQuit
            ));
        }
    }
}

/// Advances gesture-owned cursor capture before Leafwing samples local input.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn update_cursor_capture(
    mut capture: ResMut<'_, CursorCaptureState>,
    confirmed_focus: Res<'_, ConfirmedPrimaryWindowFocus>,
    router: Option<Res<'_, super::ProductionSurfaceRouter>>,
    spine: Option<Res<'_, ProductionSpine>>,
    windows: Query<'_, '_, Entity, With<PrimaryWindow>>,
    mut mouse: ResMut<'_, ButtonInput<MouseButton>>,
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    mut ownership: ResMut<'_, ClientInputOwnership>,
) {
    let Ok(window) = windows.single() else {
        *ownership = ClientInputOwnership::Released;
        return;
    };
    let focused = confirmed_focus.is_focused(window);
    let route_allows_capture =
        spine.is_some() && router.as_deref().is_some_and(super::surface::cursor_locked);
    let release_requested =
        keyboard.just_pressed(KeyCode::Escape) || screenshot_release_requested(&keyboard);
    capture.reconcile(
        focused && route_allows_capture,
        CursorCaptureGesture::from_primary_button(&mouse),
        release_requested,
    );
    if capture.release_pending() {
        mouse.reset_all();
    }
    *ownership = input_ownership(focused, route_allows_capture, *capture);
}

/// Centers and locks the cursor only while the gesture-owned state permits it.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_cursor_capture(
    mut capture: ResMut<'_, CursorCaptureState>,
    confirmed_focus: Res<'_, ConfirmedPrimaryWindowFocus>,
    router: Option<Res<'_, super::ProductionSurfaceRouter>>,
    spine: Option<Res<'_, ProductionSpine>>,
    mut windows: Query<'_, '_, (Entity, &mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    main_thread: NonSendMarker,
) {
    let Ok((window_entity, mut window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let capture_allowed = spine.is_some()
        && confirmed_focus.is_focused(window_entity)
        && router.as_deref().is_some_and(super::surface::cursor_locked);
    if let Err(error) = apply_cursor_capture(
        &mut capture,
        capture_allowed,
        window_entity,
        &mut window,
        &mut cursor,
        &main_thread,
    ) {
        bevy::log::warn!(error = %error, "cursor capture transition failed");
    }
}

const fn input_ownership(
    focused: bool,
    route_allows_capture: bool,
    capture: CursorCaptureState,
) -> ClientInputOwnership {
    if !focused {
        ClientInputOwnership::Released
    } else if route_allows_capture && capture.owns_gameplay_input() {
        ClientInputOwnership::Gameplay
    } else {
        ClientInputOwnership::Surface
    }
}

/// Drops live walk, look, and edit input while the overlay is open.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn suppress_gameplay_while_paused(
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Option<Res<'_, ProductionHudSurfaces>>,
    mut inbox: ResMut<'_, ActionFrameInbox>,
) {
    if pause.is_paused() || surfaces.is_some_and(|surfaces| surfaces.inventory_open()) {
        inbox.suppress_live_gameplay();
    }
}

/// Holds the kinematic capsule still while paused so gravity does not continue.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn freeze_player_while_paused(
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Option<Res<'_, ProductionHudSurfaces>>,
    mut players: Query<'_, '_, &mut LinearVelocity, With<D2Player>>,
) {
    if !pause.is_paused() && !surfaces.is_some_and(|surfaces| surfaces.inventory_open()) {
        return;
    }
    for mut velocity in &mut players {
        *velocity = LinearVelocity::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_cursor_sync_is_forced_onto_the_winit_main_thread() {
        use bevy::ecs::{
            system::{IntoSystem, System},
            world::World,
        };

        let mut world = World::new();
        let mut system = IntoSystem::into_system(sync_cursor_capture);
        system.initialize(&mut world);
        assert!(
            !system.is_send(),
            "winit's thread-local window table is empty on Bevy worker threads"
        );
    }

    #[test]
    fn cursor_requires_a_focused_viewport_gesture_and_never_auto_recaptures() {
        let mut router = super::super::ProductionSurfaceRouter::playing()
            .expect("route vocabulary is supported");
        let mut capture = CursorCaptureState::default();
        assert_eq!(
            input_ownership(true, true, capture),
            ClientInputOwnership::Surface
        );
        capture.reconcile(true, CursorCaptureGesture::JustPressed, false);
        assert!(capture.capture_pending());
        assert_eq!(
            input_ownership(true, true, capture),
            ClientInputOwnership::Surface
        );
        capture.reconcile(false, CursorCaptureGesture::Released, false);
        assert!(capture.release_pending());
        capture.reconcile(true, CursorCaptureGesture::Released, false);
        assert!(capture.release_pending());
        capture = CursorCaptureState::ReleasedAwaitingGesture;
        capture.reconcile(true, CursorCaptureGesture::Released, false);
        assert!(!capture.capture_pending());
        assert_eq!(
            input_ownership(false, true, capture),
            ClientInputOwnership::Released
        );

        router
            .apply(&latticeaxiom_client_ui::SurfaceCommandV1::ToggleInventory)
            .expect("inventory opens from gameplay");
        assert!(!super::super::surface::cursor_locked(&router));
        router
            .apply(&latticeaxiom_client_ui::SurfaceCommandV1::Back)
            .expect("inventory closes back to gameplay");
        assert!(super::super::surface::cursor_locked(&router));

        router
            .apply(&latticeaxiom_client_ui::SurfaceCommandV1::Pause)
            .expect("pause opens from gameplay");
        assert!(!super::super::surface::cursor_locked(&router));
    }

    #[test]
    fn runtime_projection_keeps_all_terrain_controls_distinct() {
        let prior = AppliedRuntimeSettings {
            terrain_distances: TerrainDistanceRequestsV1::default(),
            far_terrain_quality: FarTerrainQualityV1::Balanced,
            video: VideoRuntimeSettings::default(),
            tick_rate: SimulationTickRate::default(),
        };
        let proposed = BTreeMap::from([
            (render_distance_setting_id(), serde_json::json!(21)),
            (simulation_distance_setting_id(), serde_json::json!(3)),
            (full_detail_distance_setting_id(), serde_json::json!(5)),
            (
                distant_terrain_quality_setting_id(),
                serde_json::json!("quality"),
            ),
        ]);
        let projected = runtime_settings_from_proposed(prior, &proposed)
            .expect("catalog-valid terrain controls project independently");
        assert_eq!(projected.terrain_distances.render().chunks(), 21);
        assert_eq!(projected.terrain_distances.simulation().chunks(), 3);
        assert_eq!(projected.terrain_distances.full_detail().chunks(), 5);
        assert_eq!(projected.far_terrain_quality, FarTerrainQualityV1::Quality);
    }
}
