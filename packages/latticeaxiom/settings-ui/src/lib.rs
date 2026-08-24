//! Typed settings catalog projection and presentation state contracts.
//!
//! This crate owns the client/tool settings surface: frozen categories, control
//! vocabulary, the presentation draft lifecycle, immutable apply requests, and
//! AccessKit projection through `latticeaxiom-client-ui`. The host maps an
//! emitted request into the registry transaction and reports only a typed
//! completion resolution. Runtime apply, persistence, and rollback execution
//! are deliberately not adapted or re-exported here.

mod category;
mod control;
mod surface;

pub use category::{
    SETTINGS_CATEGORY_VOCABULARY_MAJOR, SettingsCategoryDocument, SettingsCategoryError,
    SettingsCategoryRecord, SettingsCategoryV1,
};
pub use control::{
    SETTINGS_CONTROL_VOCABULARY_MAJOR, SettingsControlDocument, SettingsControlError,
    SettingsControlKind, SettingsControlRecord, SettingsSliderConstraint,
};
pub use latticeaxiom_runtime_contracts::{RestartImpactMetadata, SettingsDurabilityDomain};
pub use surface::{
    SettingsIntegerSliderState, SettingsReadOnlyReason, SettingsSliderDirection,
    SettingsSurfaceApplyRequest, SettingsSurfaceApplyResolution, SettingsSurfaceAuthority,
    SettingsSurfaceCommand, SettingsSurfaceError, SettingsSurfaceModel, SettingsSurfaceOutcome,
    SettingsSurfaceRow, SettingsSurfaceScope, SettingsSurfaceState,
};
