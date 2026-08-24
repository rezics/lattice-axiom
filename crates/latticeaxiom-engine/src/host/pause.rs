//! In-session pause overlay and cursor capture for the interactive client.

use std::path::PathBuf;

use accesskit::{Node as AccessKitNode, Role as AccessKitRole};
use avian3d::prelude::LinearVelocity;
use bevy::{
    a11y::AccessibilityNode,
    app::AppExit,
    ecs::change_detection::{DetectChanges, Ref},
    ecs::observer::On,
    ecs::query::{Has, Or},
    input::{ButtonInput, keyboard::KeyCode, mouse::MouseButton},
    input_focus::tab_navigation::{NavAction, TabGroup, TabIndex, TabNavigation},
    input_focus::{FocusCause, InputFocus, InputFocusVisible},
    prelude::{
        AlignItems, BackgroundColor, BorderColor, BorderRadius, Button, Changed, Color, Commands,
        Component, Display, Entity, FlexDirection, GlobalZIndex, Interaction, JustifyContent,
        MessageWriter, Name, Node, Pickable, PositionType, Query, Res, ResMut, Resource, Text,
        TextColor, UiRect, Val, With, Without,
    },
    ui::FocusPolicy,
    ui_widgets::{
        SetSliderValue, Slider, SliderPrecision, SliderRange, SliderStep, SliderThumb, SliderValue,
        SliderValueChange, TrackClick, ValueChange,
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_input::ClientSurfaceActionV1;
use latticeaxiom_player::{
    ActionFrameInbox, ActionState, D2Player, LeafwingPlayerAction, LocalPlayerInput,
    SurfaceActionFrame,
};
use latticeaxiom_runtime_contracts::view_distance_setting_id;
use latticeaxiom_settings_ui::{
    SettingsDurabilityDomain, SettingsIntegerSliderState, SettingsSurfaceApplyResolution,
    SettingsSurfaceAuthority, SettingsSurfaceCommand, SettingsSurfaceModel, SettingsSurfaceOutcome,
    SettingsSurfaceScope,
};

use super::{
    ProductionSessionPause, ProductionSpine, ViewDistanceClampReasonV1, ViewDistanceStatusV1,
    hud::ProductionHudSurfaces,
};
use crate::{
    EngineProfile,
    settings::{HostSettingsCatalog, HostSettingsError, HostUserSettings},
    ui_font::ui_text_font,
};

/// Marker on the pause hint / settings readout.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct PauseSettingsHint;

/// Honest raw request plus the host's typed admitted/effective status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ViewDistanceStatus {
    requested: u32,
    host: ViewDistanceStatusV1,
}

const SAFE_PROCESS_RESTART_REQUIRED: &str =
    "Settings state requires a safe process restart; editing is locked";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsPublicationState {
    Confirmed,
    SafeProcessRestartRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsApplyFailureDisposition {
    RollbackRuntime,
    KeepRuntimeWithVisiblePublicationAndRestart,
    KeepRuntimeWithUnknownPublicationAndRestart,
}

fn settings_apply_failure_disposition(
    error: &HostSettingsError,
) -> SettingsApplyFailureDisposition {
    if error.proposed_value_is_visible_but_durability_uncertain() {
        SettingsApplyFailureDisposition::KeepRuntimeWithVisiblePublicationAndRestart
    } else if error.requires_safe_process_restart() {
        SettingsApplyFailureDisposition::KeepRuntimeWithUnknownPublicationAndRestart
    } else {
        SettingsApplyFailureDisposition::RollbackRuntime
    }
}

/// Live user settings draft owned by the game process.
#[derive(Resource)]
pub(super) struct ProductionSettingsState {
    root: PathBuf,
    user: HostUserSettings,
    catalog: HostSettingsCatalog,
    active_lock: CanonicalHash,
    surface: SettingsSurfaceModel,
    view_distance: StableId,
    runtime_request: u32,
    publication: SettingsPublicationState,
    diagnostic: Option<String>,
}

impl ProductionSettingsState {
    pub(super) fn new(
        root: PathBuf,
        user: HostUserSettings,
        catalog: HostSettingsCatalog,
        active_lock: CanonicalHash,
    ) -> Result<Self, HostSettingsError> {
        let snapshot = user.effective_snapshot(&catalog, active_lock)?;
        let mut surface = SettingsSurfaceModel::from_snapshot(
            &catalog.as_validated().as_catalog().runtime,
            &snapshot,
            SettingsSurfaceAuthority::shell(),
        )?;
        surface.apply_scope_filter(SettingsSurfaceScope::InGame);
        let view_distance = view_distance_setting_id();
        let slider = surface.integer_slider(&view_distance)?;
        validate_view_distance_slider(slider)?;
        let runtime_request =
            u32::try_from(slider.applied).map_err(|_| HostSettingsError::CatalogUnavailable {
                reason: "applied view distance is outside the chunk-distance domain".to_owned(),
            })?;
        Ok(Self {
            root,
            user,
            catalog,
            active_lock,
            surface,
            view_distance,
            runtime_request,
            publication: SettingsPublicationState::Confirmed,
            diagnostic: None,
        })
    }

    fn begin_edit(&mut self) {
        if self.publication == SettingsPublicationState::SafeProcessRestartRequired {
            self.diagnostic = Some(SAFE_PROCESS_RESTART_REQUIRED.to_owned());
            return;
        }
        match self.surface.handle(SettingsSurfaceCommand::BeginEdit) {
            Ok(SettingsSurfaceOutcome::EditingBegan { .. }) => self.diagnostic = None,
            Ok(_) => {
                self.diagnostic = Some("Settings surface returned an invalid begin outcome".into());
            }
            Err(error) => self.diagnostic = Some(error.to_string()),
        }
    }

    fn rollback_draft(&mut self) {
        if self.surface.apply_pending() {
            self.diagnostic = Some("Settings apply is still pending".to_owned());
            return;
        }
        self.diagnostic = match self.surface.handle(SettingsSurfaceCommand::Cancel) {
            Ok(SettingsSurfaceOutcome::Cancelled { .. }) => (self.publication
                == SettingsPublicationState::SafeProcessRestartRequired)
                .then(|| SAFE_PROCESS_RESTART_REQUIRED.to_owned()),
            Ok(_) => Some("Settings surface returned an invalid cancel outcome".to_owned()),
            Err(error) => Some(error.to_string()),
        };
    }

    fn reset_draft(&mut self) {
        if self.publication == SettingsPublicationState::SafeProcessRestartRequired {
            self.diagnostic = Some(SAFE_PROCESS_RESTART_REQUIRED.to_owned());
            return;
        }
        self.diagnostic = match self.surface.handle(SettingsSurfaceCommand::Undo) {
            Ok(SettingsSurfaceOutcome::DraftRestored { .. }) => None,
            Ok(_) => Some("Settings surface returned an invalid undo outcome".to_owned()),
            Err(error) => Some(error.to_string()),
        };
    }

    pub(super) const fn applied_request(&self) -> u32 {
        self.runtime_request
    }

    fn dirty(&self) -> bool {
        self.surface.is_dirty()
    }

    fn slider(&self) -> Result<SettingsIntegerSliderState, HostSettingsError> {
        self.surface
            .integer_slider(&self.view_distance)
            .map_err(HostSettingsError::from)
    }

    fn draft_chunks(&self) -> Result<u32, HostSettingsError> {
        u32::try_from(self.slider()?.draft).map_err(|_| HostSettingsError::CatalogUnavailable {
            reason: "draft view distance is outside the chunk-distance domain".to_owned(),
        })
    }

    fn set_draft_from_slider(&mut self, value: f32) -> bool {
        if self.publication == SettingsPublicationState::SafeProcessRestartRequired {
            self.diagnostic = Some(SAFE_PROCESS_RESTART_REQUIRED.to_owned());
            return false;
        }
        match self
            .surface
            .handle(SettingsSurfaceCommand::SetIntegerSliderValue {
                setting: self.view_distance.clone(),
                value,
            }) {
            Ok(SettingsSurfaceOutcome::DraftChanged { .. }) => {
                self.diagnostic = None;
                true
            }
            Ok(SettingsSurfaceOutcome::Unchanged) => false,
            Ok(_) => {
                self.diagnostic =
                    Some("Settings surface returned an invalid slider outcome".to_owned());
                false
            }
            Err(error) => {
                self.diagnostic = Some(error.to_string());
                false
            }
        }
    }

    fn lock_for_safe_restart(&mut self, runtime_request: u32, diagnostic: String) {
        self.runtime_request = runtime_request;
        self.publication = SettingsPublicationState::SafeProcessRestartRequired;
        self.surface.lock_for_safe_restart();
        self.diagnostic = Some(diagnostic);
    }

    fn finish_surface_apply(
        &mut self,
        resolution: SettingsSurfaceApplyResolution,
    ) -> Result<(), String> {
        match self.surface.finish_apply(resolution) {
            Ok(SettingsSurfaceOutcome::ApplyCompleted {
                resolution: observed,
            }) if observed == resolution => Ok(()),
            Ok(_) => Err("settings surface returned an invalid completion outcome".to_owned()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn finish_persisted_apply(&mut self, requested: u32) {
        if let Err(error) = self.finish_surface_apply(SettingsSurfaceApplyResolution::Committed) {
            self.lock_for_safe_restart(
                requested,
                format!(
                    "Settings were saved, but the presentation state could not confirm the commit: {error}"
                ),
            );
            return;
        }
        self.runtime_request = requested;
        self.publication = SettingsPublicationState::Confirmed;
        self.diagnostic = Some("Settings applied and saved".to_owned());
    }

    fn handle_persist_failure(
        &mut self,
        spine: &ProductionSpine,
        requested: u32,
        prior_runtime_request: u32,
        error: &HostSettingsError,
    ) {
        match settings_apply_failure_disposition(error) {
            SettingsApplyFailureDisposition::KeepRuntimeWithVisiblePublicationAndRestart => {
                let completion = self.finish_surface_apply(
                    SettingsSurfaceApplyResolution::PublicationVisibleRestartRequired,
                );
                self.lock_for_safe_restart(
                    requested,
                    completion.map_or_else(
                        |completion| {
                            format!(
                                "The new file is visible, durability is uncertain, and surface recovery failed: {error}; {completion}"
                            )
                        },
                        |()| {
                            format!(
                                "Runtime kept the requested value; the new file is visible but its durability is uncertain: {error}"
                            )
                        },
                    ),
                );
            }
            SettingsApplyFailureDisposition::KeepRuntimeWithUnknownPublicationAndRestart => {
                let completion = self.finish_surface_apply(
                    SettingsSurfaceApplyResolution::PublicationUnknownRestartRequired,
                );
                self.lock_for_safe_restart(
                    requested,
                    completion.map_or_else(
                        |completion| {
                            format!(
                                "Storage visibility and surface recovery are uncertain: {error}; {completion}"
                            )
                        },
                        |()| {
                            format!(
                                "Runtime kept the requested value, but storage visibility could not be proven: {error}"
                            )
                        },
                    ),
                );
            }
            SettingsApplyFailureDisposition::RollbackRuntime => {
                match spine.set_requested_view_distance(prior_runtime_request) {
                    Ok(_) => {
                        self.runtime_request = prior_runtime_request;
                        self.diagnostic = match self
                            .finish_surface_apply(SettingsSurfaceApplyResolution::Rejected)
                        {
                            Ok(()) => Some(format!(
                                "Settings were not saved; runtime restored: {error}"
                            )),
                            Err(completion) => {
                                self.lock_for_safe_restart(
                                    prior_runtime_request,
                                    format!(
                                        "Runtime was restored, but presentation recovery failed: {error}; {completion}"
                                    ),
                                );
                                return;
                            }
                        };
                    }
                    Err(rollback) => {
                        let completion = self.finish_surface_apply(
                            SettingsSurfaceApplyResolution::PublicationUnknownRestartRequired,
                        );
                        self.lock_for_safe_restart(
                            requested,
                            completion.map_or_else(
                                |completion| {
                                    format!(
                                        "Persistence, runtime rollback, and surface recovery failed; safe restart required: {error}; {rollback}; {completion}"
                                    )
                                },
                                |()| {
                                    format!(
                                        "Settings were not saved and runtime rollback failed; editing is locked until a safe process restart: {error}; {rollback}"
                                    )
                                },
                            ),
                        );
                    }
                }
            }
        }
    }

    fn apply(&mut self, spine: &ProductionSpine) {
        if self.publication == SettingsPublicationState::SafeProcessRestartRequired {
            self.diagnostic = Some(SAFE_PROCESS_RESTART_REQUIRED.to_owned());
            return;
        }
        let request = match self.surface.handle(SettingsSurfaceCommand::Apply) {
            Ok(SettingsSurfaceOutcome::ApplyRequested(request)) => request,
            Ok(_) => {
                self.diagnostic =
                    Some("Settings surface returned an invalid apply outcome".to_owned());
                return;
            }
            Err(error) => {
                self.diagnostic = Some(error.to_string());
                return;
            }
        };
        let prior_runtime_request = self.runtime_request;
        let requested = request
            .proposed
            .get(&self.view_distance)
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok());
        let Some(requested) = requested.filter(|_| {
            request.domain == SettingsDurabilityDomain::User && request.proposed.len() == 1
        }) else {
            match self.finish_surface_apply(SettingsSurfaceApplyResolution::Rejected) {
                Ok(()) => {
                    self.diagnostic =
                        Some("Host cannot execute the package settings request atomically".into());
                }
                Err(completion) => self.lock_for_safe_restart(
                    prior_runtime_request,
                    format!(
                        "Host rejected the package request and surface recovery failed: {completion}"
                    ),
                ),
            }
            return;
        };
        if let Err(error) = spine.set_requested_view_distance(requested) {
            let completion = self.finish_surface_apply(SettingsSurfaceApplyResolution::Rejected);
            self.diagnostic = Some(format!("Runtime rejected the draft: {error}"));
            if let Err(completion) = completion {
                self.lock_for_safe_restart(
                    prior_runtime_request,
                    format!("Runtime rejected the draft and surface recovery failed: {completion}"),
                );
            }
            return;
        }

        match self.user.persist_view_distance(
            &self.root,
            &self.catalog,
            self.active_lock,
            requested,
        ) {
            Ok(_) => self.finish_persisted_apply(requested),
            Err(error) => {
                self.handle_persist_failure(spine, requested, prior_runtime_request, &error);
            }
        }
    }

    fn status(&self, spine: &ProductionSpine) -> Option<ViewDistanceStatus> {
        spine.view_distance_status().map(|host| ViewDistanceStatus {
            requested: self.runtime_request,
            host,
        })
    }

    fn status_text(&self, spine: &ProductionSpine) -> String {
        let mut text = if let Some(status) = self.status(spine) {
            let mut text = format!(
                "Requested {} · admitted {} · effective {} chunks",
                status.requested,
                status.host.admitted(),
                status.host.effective()
            );
            if status.requested > status.host.admitted() {
                text.push_str(" · host request limit ");
                text.push_str(&status.host.requested_cap().to_string());
            }
            match status.host.clamp_reason() {
                Some(ViewDistanceClampReasonV1::GenerationRadius) => {
                    text.push_str(" · generation-radius limited");
                }
                Some(ViewDistanceClampReasonV1::ResidentBudget) => {
                    text.push_str(" · resident-budget limited");
                }
                Some(ViewDistanceClampReasonV1::GenerationRadiusAndResidentBudget) => {
                    text.push_str(" · generation-radius and resident-budget limited");
                }
                None => {}
            }
            text
        } else {
            format!(
                "Requested {} · runtime admission/effective status unavailable",
                self.runtime_request
            )
        };
        if self.publication == SettingsPublicationState::SafeProcessRestartRequired {
            text.push_str(" · ");
            text.push_str(SAFE_PROCESS_RESTART_REQUIRED);
        }
        if self.dirty() {
            match self.draft_chunks() {
                Ok(draft) => {
                    text.push_str(if self.publication == SettingsPublicationState::Confirmed {
                        " · draft "
                    } else {
                        " · retained unconfirmed draft "
                    });
                    text.push_str(&draft.to_string());
                    if self.publication == SettingsPublicationState::Confirmed {
                        text.push_str(" (Apply to save)");
                    }
                }
                Err(error) => {
                    text.push_str(" · invalid package draft: ");
                    text.push_str(&error.to_string());
                }
            }
        }
        if let Some(diagnostic) = &self.diagnostic {
            text.push_str(" · ");
            text.push_str(diagnostic);
        }
        text
    }
}

fn validate_view_distance_slider(
    state: SettingsIntegerSliderState,
) -> Result<(), HostSettingsError> {
    if slider_widget_values(state).is_none() {
        return Err(HostSettingsError::CatalogUnavailable {
            reason: "view-distance slider exceeds the exact Bevy f32 integer domain".to_owned(),
        });
    }
    for value in [state.applied, state.draft, state.min, state.max] {
        u32::try_from(value).map_err(|_| HostSettingsError::CatalogUnavailable {
            reason: "view-distance slider is outside the chunk-distance domain".to_owned(),
        })?;
    }
    Ok(())
}

fn slider_widget_values(state: SettingsIntegerSliderState) -> Option<(f32, f32, f32, f32)> {
    let value = f32::from(i16::try_from(state.draft).ok()?);
    let min = f32::from(i16::try_from(state.min).ok()?);
    let max = f32::from(i16::try_from(state.max).ok()?);
    let step = f32::from(u16::try_from(state.step).ok()?);
    (step > 0.0 && min <= max).then_some((value, min, max, step))
}

/// Explicit viewport-click latch for relative-mouse capture.
///
/// A focused window is not enough to capture the OS cursor: the player must
/// click the viewport first, and focus loss or an overlay always releases it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub(super) struct CursorCaptureState {
    captured: bool,
}

impl CursorCaptureState {
    pub(super) const fn captured(self) -> bool {
        self.captured
    }

    fn set(&mut self, captured: bool) {
        self.captured = captured;
    }
}

/// Marker on the full-screen pause overlay.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct PauseOverlay;

/// Pause-menu button action.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) enum PauseMenuAction {
    Resume,
    Settings,
    Apply,
    Undo,
    Back,
    Quit,
}

/// Marker on the native Bevy view-distance slider.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ViewDistanceSlider;

/// Marker on the view-distance slider thumb.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ViewDistanceSliderThumb;

/// One-frame latch that releases an Interaction pulse after button handling.
#[derive(Debug, Default, Resource)]
pub(super) struct TypedSurfacePress(Option<Entity>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SurfaceSliderInput {
    Absent,
    Decrement,
    Increment,
    Cancelled,
}

const fn surface_slider_input(left: bool, right: bool) -> SurfaceSliderInput {
    match (left, right) {
        (false, false) => SurfaceSliderInput::Absent,
        (true, false) => SurfaceSliderInput::Decrement,
        (false, true) => SurfaceSliderInput::Increment,
        (true, true) => SurfaceSliderInput::Cancelled,
    }
}

fn native_slider_keyboard_already_adjusted(
    input: SurfaceSliderInput,
    keyboard: Option<&ButtonInput<KeyCode>>,
    slider_value_changed: bool,
) -> bool {
    if !slider_value_changed {
        return false;
    }
    keyboard.is_some_and(|keyboard| match input {
        SurfaceSliderInput::Decrement => keyboard.just_pressed(KeyCode::ArrowLeft),
        SurfaceSliderInput::Increment => keyboard.just_pressed(KeyCode::ArrowRight),
        SurfaceSliderInput::Absent | SurfaceSliderInput::Cancelled => false,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SurfaceFocusInput {
    Absent,
    Previous,
    Next,
    Cancelled,
}

const fn surface_focus_input(previous: bool, next: bool) -> SurfaceFocusInput {
    match (previous, next) {
        (false, false) => SurfaceFocusInput::Absent,
        (true, false) => SurfaceFocusInput::Previous,
        (false, true) => SurfaceFocusInput::Next,
        (true, true) => SurfaceFocusInput::Cancelled,
    }
}

const VIEW_DISTANCE_TAB_INDEX: i32 = 0;

const fn settings_tab_index(action: PauseMenuAction) -> Option<i32> {
    match action {
        PauseMenuAction::Apply => Some(1),
        PauseMenuAction::Undo => Some(2),
        PauseMenuAction::Back => Some(3),
        PauseMenuAction::Resume | PauseMenuAction::Settings | PauseMenuAction::Quit => None,
    }
}

/// Spawns the pause overlay when this Bevy app is the interactive client.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn spawn_pause_overlay_if_client(
    mut commands: Commands<'_, '_>,
    profile: Res<'_, EngineProfile>,
    settings: Option<Res<'_, ProductionSettingsState>>,
) {
    if *profile != EngineProfile::Client {
        return;
    }
    spawn_pause_overlay(&mut commands, settings.as_deref());
}

fn spawn_pause_overlay(
    commands: &mut Commands<'_, '_>,
    settings: Option<&ProductionSettingsState>,
) {
    commands
        .spawn((
            PauseOverlay,
            Name::new("Pause overlay"),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                ..Node::default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.03, 0.62)),
            GlobalZIndex(100),
            FocusPolicy::Block,
            Pickable::IGNORE,
            TabGroup::modal(),
        ))
        .with_children(|overlay| {
            overlay.spawn((
                Name::new("Paused title"),
                Text::new("Paused"),
                ui_text_font(36.0),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..Node::default()
                },
            ));
            spawn_pause_button(overlay, PauseMenuAction::Resume, "Resume");
            spawn_pause_button(overlay, PauseMenuAction::Settings, "Settings");
            if let Some(settings) = settings {
                spawn_view_distance_slider(overlay, settings);
            }
            spawn_pause_button(overlay, PauseMenuAction::Apply, "Apply");
            spawn_pause_button(overlay, PauseMenuAction::Undo, "Undo changes");
            spawn_pause_button(overlay, PauseMenuAction::Back, "Back");
            spawn_pause_button(overlay, PauseMenuAction::Quit, "Quit Game");
            overlay.spawn((
                PauseSettingsHint,
                Name::new("Pause hint"),
                Text::new("Esc resumes · Settings: render distance"),
                ui_text_font(16.0),
                TextColor(Color::srgb(0.72, 0.74, 0.68)),
                Node {
                    margin: UiRect::top(Val::Px(8.0)),
                    ..Node::default()
                },
            ));
        });
}

fn spawn_view_distance_slider(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    settings: &ProductionSettingsState,
) {
    let Ok(state) = settings.slider() else {
        return;
    };
    let Some((value, min, max, step)) = slider_widget_values(state) else {
        return;
    };
    parent
        .spawn((
            ViewDistanceSlider,
            Name::new("View distance"),
            Node {
                position_type: PositionType::Relative,
                width: Val::Px(360.0),
                height: Val::Px(24.0),
                border: UiRect::all(Val::Px(2.0)),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            Slider {
                track_click: TrackClick::Snap,
                ..Slider::default()
            },
            SliderValue(value),
            SliderRange::new(min, max),
            SliderStep(step),
            SliderPrecision(0),
            accessibility_node(AccessKitRole::Slider, "View distance"),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|slider| {
            slider.spawn((
                Name::new("View distance rail"),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(8.0),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..Node::default()
                },
                BackgroundColor(Color::srgb(0.14, 0.18, 0.17)),
            ));
            slider
                .spawn((
                    Name::new("View distance thumb track"),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(16.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        ..Node::default()
                    },
                ))
                .with_children(|track| {
                    track.spawn((
                        ViewDistanceSliderThumb,
                        SliderThumb,
                        Name::new("View distance thumb"),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(0.0),
                            width: Val::Px(16.0),
                            height: Val::Px(16.0),
                            border_radius: BorderRadius::MAX,
                            ..Node::default()
                        },
                        BackgroundColor(Color::srgb(0.32, 0.78, 0.63)),
                    ));
                });
        });
}
fn spawn_pause_button(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    action: PauseMenuAction,
    label: &'static str,
) {
    let hidden = matches!(
        action,
        PauseMenuAction::Apply | PauseMenuAction::Undo | PauseMenuAction::Back
    );
    parent
        .spawn((
            Button,
            action,
            Name::new(label),
            accessibility_node(AccessKitRole::Button, label),
            Node {
                width: Val::Px(240.0),
                height: Val::Px(44.0),
                display: if hidden { Display::None } else { Display::Flex },
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            BackgroundColor(button_color(Interaction::None, false)),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                ui_text_font(20.0),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
            ));
        });
}

fn accessibility_node(role: AccessKitRole, label: &'static str) -> AccessibilityNode {
    let mut node = AccessKitNode::new(role);
    node.set_label(label);
    node.into()
}

/// Toggles the in-session pause overlay from the Pause action or Escape.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn toggle_pause(
    action_states: Query<'_, '_, &ActionState<LeafwingPlayerAction>, With<LocalPlayerInput>>,
    surface: Option<Res<'_, SurfaceActionFrame>>,
    mut pause: ResMut<'_, ProductionSessionPause>,
    mut router: Option<ResMut<'_, super::ProductionSurfaceRouter>>,
) {
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

/// Copies native slider changes into the settings draft without publishing them.
#[allow(clippy::needless_pass_by_value)] // Bevy observers receive SystemParams by value.
pub(super) fn view_distance_slider_changed(
    change: On<'_, '_, ValueChange<f32>>,
    sliders: Query<'_, '_, (), With<ViewDistanceSlider>>,
    mut settings: Option<ResMut<'_, ProductionSettingsState>>,
    mut commands: Commands<'_, '_>,
) {
    if sliders.get(change.source).is_err() {
        return;
    }
    let Some(settings) = settings.as_mut() else {
        return;
    };
    settings.set_draft_from_slider(change.value);
    if let Ok(state) = settings.slider()
        && let Some((value, _, _, _)) = slider_widget_values(state)
    {
        commands.entity(change.source).insert(SliderValue(value));
    }
}

/// Bridges graph-compiled surface navigation into native Bevy focus and widgets.
///
/// The surface router runs first so Back or Pause owns a simultaneous modal
/// transition. Native Tab and focused-slider keyboard handling run during
/// `PreUpdate`; change detection proves when this bridge must consume the
/// corresponding compiled action without applying it a second time.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::too_many_arguments)] // One exclusive settings input-context bridge.
#[allow(clippy::too_many_lines)] // Keep the exclusive input decision in one ordered system.
#[allow(clippy::type_complexity)] // The control query is the typed settings focus ring.
pub(super) fn apply_settings_surface_actions(
    pause: Res<'_, ProductionSessionPause>,
    router: Option<Res<'_, super::ProductionSurfaceRouter>>,
    mut frame: ResMut<'_, SurfaceActionFrame>,
    keyboard: Option<Res<'_, ButtonInput<KeyCode>>>,
    mut focus: Option<ResMut<'_, InputFocus>>,
    mut focus_visible: Option<ResMut<'_, InputFocusVisible>>,
    tab_navigation: TabNavigation<'_, '_>,
    overlays: Query<'_, '_, Entity, With<PauseOverlay>>,
    controls: Query<
        '_,
        '_,
        (
            Entity,
            Has<ViewDistanceSlider>,
            Option<&PauseMenuAction>,
            &Node,
        ),
        Or<(With<ViewDistanceSlider>, With<PauseMenuAction>)>,
    >,
    sliders: Query<'_, '_, (Entity, Ref<'_, SliderValue>, &Node), With<ViewDistanceSlider>>,
    mut buttons: Query<'_, '_, (&PauseMenuAction, &Node, &mut Interaction), With<Button>>,
    mut typed_press: ResMut<'_, TypedSurfacePress>,
    mut commands: Commands<'_, '_>,
) {
    let showing_settings = pause.is_paused()
        && router.as_ref().is_some_and(|router| {
            router.inner().route().modal() == latticeaxiom_client_ui::GameModalV1::Settings
        });
    let focused_before = focus.as_ref().and_then(|focus| focus.get());
    let focused_settings_control = focused_before.is_some_and(|focused| {
        controls.get(focused).is_ok_and(|(_, slider, action, _)| {
            slider || action.is_some_and(|action| settings_tab_index(*action).is_some())
        })
    });
    if !showing_settings {
        if focused_settings_control {
            if let Some(focus) = focus.as_mut() {
                focus.clear();
            }
            if let Some(visible) = focus_visible.as_mut() {
                visible.0 = false;
            }
        }
        return;
    }

    let focus_changed_before_bridge = focus.as_ref().is_some_and(DetectChanges::is_changed);
    let focused_control_is_visible = focused_before.is_some_and(|focused| {
        controls
            .get(focused)
            .is_ok_and(|(_, slider, action, node)| {
                node.display != Display::None
                    && (slider
                        || action.is_some_and(|action| settings_tab_index(*action).is_some()))
            })
    });
    if !focused_control_is_visible
        && let (Some(focus), Ok(overlay)) = (focus.as_mut(), overlays.single())
        && let Ok(first) = tab_navigation.initialize(overlay, NavAction::First)
    {
        focus.set(first, FocusCause::Navigated);
        if let Some(visible) = focus_visible.as_mut() {
            visible.0 = true;
        }
    }

    let slider_input = surface_slider_input(
        frame.just_started(ClientSurfaceActionV1::NavLeft),
        frame.just_started(ClientSurfaceActionV1::NavRight),
    );
    let focus_input = surface_focus_input(
        frame.just_started(ClientSurfaceActionV1::NavPrevious)
            || frame.just_started(ClientSurfaceActionV1::NavUp),
        frame.just_started(ClientSurfaceActionV1::NavNext)
            || frame.just_started(ClientSurfaceActionV1::NavDown),
    );
    let activate = frame.just_started(ClientSurfaceActionV1::Activate);
    let consumed = slider_input != SurfaceSliderInput::Absent
        || focus_input != SurfaceFocusInput::Absent
        || activate;
    if !consumed {
        return;
    }

    let focus_action = match focus_input {
        SurfaceFocusInput::Previous => Some(NavAction::Previous),
        SurfaceFocusInput::Next => Some(NavAction::Next),
        SurfaceFocusInput::Absent | SurfaceFocusInput::Cancelled => None,
    };
    if let Some(action) = focus_action
        && !focus_changed_before_bridge
        && let Some(focus) = focus.as_mut()
        && let Ok(next) = tab_navigation.navigate(focus, action)
    {
        focus.set(next, FocusCause::Navigated);
        if let Some(visible) = focus_visible.as_mut() {
            visible.0 = true;
        }
    }

    let slider_delta = match slider_input {
        SurfaceSliderInput::Decrement => Some(-1.0),
        SurfaceSliderInput::Increment => Some(1.0),
        SurfaceSliderInput::Absent | SurfaceSliderInput::Cancelled => None,
    };
    if focus_input == SurfaceFocusInput::Absent
        && let Some(delta) = slider_delta
        && let Ok((slider_entity, slider_value, slider_node)) = sliders.single()
        && slider_node.display != Display::None
        && focus
            .as_ref()
            .and_then(|focus| focus.get())
            .is_some_and(|focused| focused == slider_entity)
    {
        let native_keyboard_already_adjusted = native_slider_keyboard_already_adjusted(
            slider_input,
            keyboard.as_deref(),
            slider_value.is_changed(),
        );
        if !native_keyboard_already_adjusted {
            commands.trigger(SetSliderValue {
                entity: slider_entity,
                change: SliderValueChange::RelativeStep(delta),
            });
        }
        if let Some(visible) = focus_visible.as_mut() {
            visible.0 = true;
        }
    }

    // Navigation and activation in one generation never click the newly moved
    // focus target. The next generation must explicitly confirm it.
    if activate
        && slider_input == SurfaceSliderInput::Absent
        && focus_input == SurfaceFocusInput::Absent
        && let Some(focused) = focus.as_ref().and_then(|focus| focus.get())
        && let Ok((action, node, mut interaction)) = buttons.get_mut(focused)
        && node.display != Display::None
        && settings_tab_index(*action).is_some()
        && *interaction != Interaction::Pressed
    {
        *interaction = Interaction::Pressed;
        typed_press.0 = Some(focused);
    }

    // Settings is an exclusive input context. Once a recognized edge enters a
    // widget or focus path, no later surface consumer may act on that frame.
    frame.clear();
}

/// Releases the one-frame Interaction pulse after the existing button system.
///
/// This keeps pointer and typed activation on the same action path without
/// leaving a controller-activated button stuck in `Pressed`.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn release_typed_surface_press(
    mut typed_press: ResMut<'_, TypedSurfacePress>,
    mut interactions: Query<'_, '_, &mut Interaction, With<Button>>,
) {
    let Some(entity) = typed_press.0.take() else {
        return;
    };
    if let Ok(mut interaction) = interactions.get_mut(entity)
        && *interaction == Interaction::Pressed
    {
        *interaction = Interaction::None;
    }
}
/// Synchronizes a visible focus indicator for pointer, keyboard, and gamepad.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_settings_control_focus_visuals(
    focus: Option<Res<'_, InputFocus>>,
    mut buttons: Query<'_, '_, (Entity, &Interaction, &mut BackgroundColor), With<PauseMenuAction>>,
    mut sliders: Query<'_, '_, (Entity, &mut BorderColor), With<ViewDistanceSlider>>,
) {
    let focused = focus.as_ref().and_then(|focus| focus.get());
    for (entity, interaction, mut background) in &mut buttons {
        let desired = button_color(*interaction, focused == Some(entity));
        if background.0 != desired {
            background.0 = desired;
        }
    }
    for (entity, mut border) in &mut sliders {
        let desired = BorderColor::all(if focused == Some(entity) {
            Color::srgb(0.32, 0.78, 0.63)
        } else {
            Color::NONE
        });
        if *border != desired {
            *border = desired;
        }
    }
}

/// Shows or hides the overlay to match the pause latch.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_pause_overlay(
    pause: Res<'_, ProductionSessionPause>,
    mut overlay: Query<'_, '_, (&mut Node, &mut Pickable), With<PauseOverlay>>,
) {
    let Ok((mut node, mut pickable)) = overlay.single_mut() else {
        return;
    };
    if pause.is_paused() {
        node.display = Display::Flex;
        *pickable = Pickable::default();
    } else {
        node.display = Display::None;
        *pickable = Pickable::IGNORE;
    }
}

/// Latches capture only after a click in the viewport and releases it on any
/// focus or modal transition.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn update_cursor_capture(
    mut capture: ResMut<'_, CursorCaptureState>,
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Option<Res<'_, ProductionHudSurfaces>>,
    router: Option<Res<'_, super::ProductionSurfaceRouter>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    mouse: Res<'_, ButtonInput<MouseButton>>,
    keyboard: Res<'_, ButtonInput<KeyCode>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let blocked = pause.is_paused()
        || surfaces.is_some_and(|surfaces| surfaces.inventory_open())
        || router
            .as_ref()
            .is_some_and(|router| !super::surface::cursor_locked(router));
    if !window.focused || blocked || keyboard.just_pressed(KeyCode::Escape) {
        capture.set(false);
        return;
    }
    if mouse.just_pressed(MouseButton::Left) && window.cursor_position().is_some() {
        capture.set(true);
    }
}

/// Locks the cursor only while the window is focused and the session is live.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_cursor_capture(
    capture: Res<'_, CursorCaptureState>,
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Option<Res<'_, ProductionHudSurfaces>>,
    router: Option<Res<'_, super::ProductionSurfaceRouter>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    mut cursors: Query<'_, '_, &mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    let blocked = pause.is_paused()
        || surfaces.is_some_and(|surfaces| surfaces.inventory_open())
        || router
            .as_ref()
            .is_some_and(|router| !super::surface::cursor_locked(router));
    let should_capture = capture.captured() && window.focused && !blocked;
    cursor.grab_mode = if should_capture {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    cursor.visible = !should_capture;
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

/// Applies Resume and Quit from the pause overlay buttons.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Button interaction query is one pause-menu mapping.
pub(super) fn pause_menu_buttons(
    mut interactions: Query<
        '_,
        '_,
        (&Interaction, &PauseMenuAction),
        (Changed<Interaction>, With<Button>),
    >,
    mut pause: ResMut<'_, ProductionSessionPause>,
    mut settings: Option<ResMut<'_, ProductionSettingsState>>,
    spine: Option<Res<'_, ProductionSpine>>,
    mut router: Option<ResMut<'_, super::ProductionSurfaceRouter>>,
    mut exits: MessageWriter<'_, AppExit>,
) {
    for (interaction, action) in &mut interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            PauseMenuAction::Resume => {
                if let Some(router) = router.as_mut()
                    && router
                        .apply(&latticeaxiom_client_ui::SurfaceCommandV1::Back)
                        .is_ok()
                {
                    pause.set(false);
                }
            }
            PauseMenuAction::Settings => {
                if let Some(router) = router.as_mut()
                    && router
                        .apply(&latticeaxiom_client_ui::SurfaceCommandV1::OpenSettings)
                        .is_ok()
                    && let Some(settings) = settings.as_mut()
                {
                    settings.begin_edit();
                }
            }
            PauseMenuAction::Back => {
                if let Some(router) = router.as_mut()
                    && router
                        .apply(&latticeaxiom_client_ui::SurfaceCommandV1::Back)
                        .is_ok()
                    && let Some(settings) = settings.as_mut()
                {
                    settings.rollback_draft();
                }
            }
            PauseMenuAction::Apply => {
                if let (Some(settings), Some(spine)) = (settings.as_mut(), spine.as_ref()) {
                    settings.apply(spine);
                }
            }
            PauseMenuAction::Undo => {
                if let Some(settings) = settings.as_mut() {
                    settings.reset_draft();
                }
            }
            PauseMenuAction::Quit => {
                if let Some(router) = router.as_mut() {
                    let _ =
                        router.apply(&latticeaxiom_client_ui::SurfaceCommandV1::RequestSaveQuit);
                    let _ = router.apply(&latticeaxiom_client_ui::SurfaceCommandV1::Confirm);
                }
                exits.write(AppExit::Success);
            }
        }
    }
}

/// Shows home or settings controls on the pause overlay.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::too_many_arguments)] // One pass keeps page visibility and focusability atomic.
#[allow(clippy::type_complexity)] // Page visibility and focusability share exact UI queries.
pub(super) fn sync_pause_menu_page(
    pause: Res<'_, ProductionSessionPause>,
    router: Option<Res<'_, super::ProductionSurfaceRouter>>,
    spine: Option<Res<'_, ProductionSpine>>,
    mut settings_state: Option<ResMut<'_, ProductionSettingsState>>,
    mut commands: Commands<'_, '_>,
    mut buttons: Query<
        '_,
        '_,
        (Entity, &PauseMenuAction, &mut Node, Has<TabIndex>),
        (
            Without<ViewDistanceSlider>,
            Without<ViewDistanceSliderThumb>,
        ),
    >,
    mut sliders: Query<
        '_,
        '_,
        (Entity, &SliderValue, &mut Node, Has<TabIndex>),
        (
            With<ViewDistanceSlider>,
            Without<PauseMenuAction>,
            Without<ViewDistanceSliderThumb>,
        ),
    >,
    mut thumbs: Query<
        '_,
        '_,
        &mut Node,
        (
            With<ViewDistanceSliderThumb>,
            Without<PauseMenuAction>,
            Without<ViewDistanceSlider>,
        ),
    >,
    mut hint: Query<'_, '_, &mut Text, With<PauseSettingsHint>>,
) {
    let showing_settings = pause.is_paused()
        && router.as_ref().is_some_and(|router| {
            router.inner().route().modal() == latticeaxiom_client_ui::GameModalV1::Settings
        });
    if let Some(settings) = settings_state.as_mut() {
        if showing_settings && !settings.surface.is_editing() {
            settings.begin_edit();
        } else if !showing_settings && settings.surface.is_editing() {
            settings.rollback_draft();
        }
    }

    for (entity, action, mut node, has_tab_index) in &mut buttons {
        let page_visible = match action {
            PauseMenuAction::Resume | PauseMenuAction::Settings | PauseMenuAction::Quit => {
                !showing_settings
            }
            PauseMenuAction::Apply | PauseMenuAction::Undo | PauseMenuAction::Back => {
                showing_settings
            }
        };
        node.display = if page_visible {
            Display::Flex
        } else {
            Display::None
        };

        let desired_tab_index = pause
            .is_paused()
            .then(|| settings_tab_index(*action))
            .flatten()
            .filter(|_| page_visible);
        match (desired_tab_index, has_tab_index) {
            (Some(index), false) => {
                commands.entity(entity).insert(TabIndex(index));
            }
            (None, true) => {
                commands.entity(entity).remove::<TabIndex>();
            }
            (Some(_), true) | (None, false) => {}
        }
    }

    if let Ok((slider_entity, slider_value, mut slider_node, has_tab_index)) = sliders.single_mut()
    {
        slider_node.display = if showing_settings {
            Display::Flex
        } else {
            Display::None
        };
        match (showing_settings, has_tab_index) {
            (true, false) => {
                commands
                    .entity(slider_entity)
                    .insert(TabIndex(VIEW_DISTANCE_TAB_INDEX));
            }
            (false, true) => {
                commands.entity(slider_entity).remove::<TabIndex>();
            }
            (true, true) | (false, false) => {}
        }

        if let Some(settings) = settings_state.as_ref()
            && let Ok(state) = settings.slider()
            && let Some((desired, min, max, _)) = slider_widget_values(state)
        {
            if slider_value.0.to_bits() != desired.to_bits() {
                commands.entity(slider_entity).insert(SliderValue(desired));
            }
            let span = (max - min).max(1.0);
            let thumb_left = Val::Percent(((desired - min) / span).clamp(0.0, 1.0) * 100.0);
            if let Ok(mut thumb) = thumbs.single_mut()
                && thumb.left != thumb_left
            {
                thumb.left = thumb_left;
            }
        }
    }

    if let Ok(mut text) = hint.single_mut() {
        let label = if showing_settings {
            match (settings_state.as_ref(), spine.as_ref()) {
                (Some(settings), Some(spine)) => settings.status_text(spine),
                _ => "Settings registry or runtime limits unavailable".to_owned(),
            }
        } else {
            "Esc resumes · Settings: render distance".to_owned()
        };
        if text.0 != label {
            *text = Text::new(label);
        }
    }
}

const fn button_color(interaction: Interaction, focused: bool) -> Color {
    match interaction {
        Interaction::Pressed => Color::srgb(0.18, 0.42, 0.36),
        Interaction::Hovered => Color::srgb(0.16, 0.22, 0.20),
        Interaction::None if focused => Color::srgb(0.13, 0.28, 0.24),
        Interaction::None => Color::srgb(0.08, 0.11, 0.10),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_distance_slider_projects_only_exact_widget_integers() {
        let state = SettingsIntegerSliderState {
            min: 2,
            max: 32,
            step: 3,
            applied: 8,
            draft: 11,
        };
        assert_eq!(slider_widget_values(state), Some((11.0, 2.0, 32.0, 3.0)));
        let oversized = SettingsIntegerSliderState {
            min: 2,
            max: i64::MAX,
            step: 1,
            applied: 8,
            draft: 8,
        };
        assert_eq!(slider_widget_values(oversized), None);
    }

    #[test]
    fn engine_source_has_no_competing_view_distance_model_or_range() {
        let source = include_str!("pause.rs");
        for duplicate in [
            concat!("struct ViewDistance", "Range"),
            concat!("struct RequestedView", "Distance"),
            concat!("fn snap_view_distance_", "slider"),
            concat!("range: ViewDistance", "Range"),
            concat!("draft: RequestedView", "Distance"),
        ] {
            assert!(
                !source.contains(duplicate),
                "engine duplicate settings authority returned: {duplicate}"
            );
        }
        assert!(source.contains("surface: SettingsSurfaceModel"));
        assert!(source.contains("SettingsSurfaceCommand::Apply"));
        assert!(source.contains("SettingsSurfaceCommand::Undo"));
        assert!(source.contains("SettingsSurfaceCommand::Cancel"));

        let invalid_step = SettingsIntegerSliderState {
            min: 2,
            max: 32,
            step: 0,
            applied: 8,
            draft: 8,
        };
        assert_eq!(slider_widget_values(invalid_step), None);
    }

    #[test]
    fn opposite_surface_slider_edges_cancel_deterministically() {
        assert_eq!(
            surface_slider_input(false, false),
            SurfaceSliderInput::Absent
        );
        assert_eq!(
            surface_slider_input(true, false),
            SurfaceSliderInput::Decrement
        );
        assert_eq!(
            surface_slider_input(false, true),
            SurfaceSliderInput::Increment
        );
        assert_eq!(
            surface_slider_input(true, true),
            SurfaceSliderInput::Cancelled
        );
    }

    #[test]
    fn native_slider_duplicate_suppression_requires_matching_arrow_and_change() {
        let mut keyboard = ButtonInput::<KeyCode>::default();
        keyboard.press(KeyCode::ArrowLeft);

        assert!(native_slider_keyboard_already_adjusted(
            SurfaceSliderInput::Decrement,
            Some(&keyboard),
            true,
        ));
        assert!(!native_slider_keyboard_already_adjusted(
            SurfaceSliderInput::Increment,
            Some(&keyboard),
            true,
        ));
        assert!(!native_slider_keyboard_already_adjusted(
            SurfaceSliderInput::Decrement,
            Some(&keyboard),
            false,
        ));
        assert!(!native_slider_keyboard_already_adjusted(
            SurfaceSliderInput::Decrement,
            None,
            true,
        ));
    }

    #[test]
    fn settings_surface_focus_contract_is_deterministic() {
        assert_eq!(surface_focus_input(false, false), SurfaceFocusInput::Absent);
        assert_eq!(
            surface_focus_input(true, false),
            SurfaceFocusInput::Previous
        );
        assert_eq!(surface_focus_input(false, true), SurfaceFocusInput::Next);
        assert_eq!(
            surface_focus_input(true, true),
            SurfaceFocusInput::Cancelled
        );

        assert_eq!(VIEW_DISTANCE_TAB_INDEX, 0);
        assert_eq!(settings_tab_index(PauseMenuAction::Apply), Some(1));
        assert_eq!(settings_tab_index(PauseMenuAction::Undo), Some(2));
        assert_eq!(settings_tab_index(PauseMenuAction::Back), Some(3));
        assert_eq!(settings_tab_index(PauseMenuAction::Resume), None);
        assert_eq!(settings_tab_index(PauseMenuAction::Settings), None);
        assert_eq!(settings_tab_index(PauseMenuAction::Quit), None);

        assert_ne!(
            button_color(Interaction::None, true),
            button_color(Interaction::None, false)
        );

        let button = accessibility_node(AccessKitRole::Button, "Apply");
        assert_eq!(button.role(), AccessKitRole::Button);
        assert_eq!(button.label(), Some("Apply"));
        let slider = accessibility_node(AccessKitRole::Slider, "View distance");
        assert_eq!(slider.role(), AccessKitRole::Slider);
        assert_eq!(slider.label(), Some("View distance"));
    }

    #[test]
    fn publication_uncertainty_keeps_runtime_while_pre_replace_failure_rolls_back() {
        let uncertain = HostSettingsError::Transaction(
            latticeaxiom_runtime_contracts::SettingsTransactionError::PublicationStateUncertain {
                source: latticeaxiom_runtime_contracts::LocalSettingsPersistError::Injected(
                    latticeaxiom_runtime_contracts::LocalSettingsFaultPoint::DirectorySync,
                ),
                proposed: Box::new(latticeaxiom_runtime_contracts::LatticeLocalSettingsV1::empty()),
            },
        );
        assert_eq!(
            settings_apply_failure_disposition(&uncertain),
            SettingsApplyFailureDisposition::KeepRuntimeWithVisiblePublicationAndRestart
        );

        let failed_recovery = HostSettingsError::Transaction(
            latticeaxiom_runtime_contracts::SettingsTransactionError::PublicationStateUncertain {
                source:
                    latticeaxiom_runtime_contracts::LocalSettingsPersistError::VisibleRecoveryFailed {
                        reason: "injected failed backup restore".to_owned(),
                    },
                proposed: Box::new(latticeaxiom_runtime_contracts::LatticeLocalSettingsV1::empty()),
            },
        );
        assert_eq!(
            settings_apply_failure_disposition(&failed_recovery),
            SettingsApplyFailureDisposition::KeepRuntimeWithUnknownPublicationAndRestart
        );

        let pre_replace = HostSettingsError::Transaction(
            latticeaxiom_runtime_contracts::SettingsTransactionError::Persist(
                latticeaxiom_runtime_contracts::LocalSettingsPersistError::Injected(
                    latticeaxiom_runtime_contracts::LocalSettingsFaultPoint::TempWrite,
                ),
            ),
        );
        assert_eq!(
            settings_apply_failure_disposition(&pre_replace),
            SettingsApplyFailureDisposition::RollbackRuntime
        );
    }
}
