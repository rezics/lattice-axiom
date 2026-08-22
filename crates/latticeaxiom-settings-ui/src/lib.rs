//! Typed settings catalog projection and transaction UI contracts.
//!
//! This crate owns the client/tool settings surface: frozen categories, control
//! vocabulary, draft/preview/apply/rollback *requests*, and AccessKit
//! projection through `latticeaxiom-client-ui`. Apply, persistence, and
//! rollback execution stay in `@latticeaxiom/settings` via
//! [`SettingsApplyTransaction`]. Headless profiles may omit this crate without
//! changing authoritative registration.

mod category;
mod control;
mod surface;
mod transaction;

pub use category::{
    SETTINGS_CATEGORY_VOCABULARY_MAJOR, SettingsCategoryDocument, SettingsCategoryError,
    SettingsCategoryRecord, SettingsCategoryV1,
};
pub use control::{
    SETTINGS_CONTROL_VOCABULARY_MAJOR, SettingsControlDocument, SettingsControlError,
    SettingsControlKind, SettingsControlRecord,
};
pub use latticeaxiom_runtime_contracts::{
    PreviewPolicyV1, SettingsApplyTransaction, SettingsDurabilityDomain, SettingsTransactionError,
    SettingsTransactionPhase,
};
pub use surface::{
    SettingsReadOnlyReason, SettingsSurfaceAuthority, SettingsSurfaceError, SettingsSurfaceModel,
    SettingsSurfaceRow,
};
pub use transaction::SettingsSurfaceTransactionRequest;
