//! Typed settings catalog projection and presentation state contracts.
//!
//! This crate owns the client/tool settings surface: frozen categories, control
//! vocabulary, the official settings page, the presentation draft lifecycle,
//! immutable apply requests, and AccessKit projection through
//! `latticeaxiom-client-ui`. The host maps an emitted request into the registry
//! transaction and reports only a typed completion resolution. Runtime apply,
//! persistence, and rollback execution are deliberately not adapted here; a
//! memory host is supplied for page construction when the durable path is not
//! yet wired.

mod baseline;
mod category;
mod control;
mod error;
mod host;
mod layout;
mod page;
mod surface;

pub use baseline::{SettingsPageCatalogKind, compile_baseline_page_catalog, section_for_setting};
pub use category::{
    SETTINGS_CATEGORY_VOCABULARY_MAJOR, SettingsCategoryDocument, SettingsCategoryError,
    SettingsCategoryRecord, SettingsCategoryV1,
};
pub use control::{
    SETTINGS_CONTROL_VOCABULARY_MAJOR, SettingsControlDocument, SettingsControlError,
    SettingsControlKind, SettingsControlRecord, SettingsSliderConstraint,
};
pub use error::SettingsPageError;
pub use host::{MemorySettingsHost, SettingsPageHost, SettingsValueAdmission};
pub use latticeaxiom_runtime_contracts::{RestartImpactMetadata, SettingsDurabilityDomain};
pub use layout::SettingsSectionV1;
pub use page::{
    SettingsBindingRow, SettingsPageCommand, SettingsPageOpen, SettingsPageOperation,
    SettingsPageOutcome, SettingsPageSession, SettingsRowDetail, category_display_name,
    section_display_name, setting_display_name,
};
pub use surface::{
    SettingsIntegerSliderState, SettingsReadOnlyReason, SettingsSliderDirection,
    SettingsSurfaceApplyRequest, SettingsSurfaceApplyResolution, SettingsSurfaceAuthority,
    SettingsSurfaceCommand, SettingsSurfaceError, SettingsSurfaceModel, SettingsSurfaceOutcome,
    SettingsSurfaceRow, SettingsSurfaceScope, SettingsSurfaceState,
};
