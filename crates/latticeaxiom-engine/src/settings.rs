//! Canonical user-settings persistence across replacement-process hops.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use latticeaxiom_core::{CanonicalHash, CanonicalLogicalPath, CapabilityId, PackageName};
use latticeaxiom_input::{BindingProfileV1 as InputBindingProfile, InputBindingV1, KeyModifierV1};
use latticeaxiom_runtime_contracts::{
    BindingProfileV1 as StoredBindingProfile, EffectiveSettingsSnapshot,
    FilesystemLocalSettingsStore, InputBindingV1 as StoredBinding, KeyboardModifiersV1,
    KnownInputBindingV1, LatticeLocalSettingsV1, LocalSettingsOrigin, LocalSettingsStore,
    MouseButtonV1 as StoredMouseButton, PreviewPolicyV1, SettingChangedBatchV1,
    SettingsApplyTransaction, SettingsCatalogFragment, SettingsCatalogPolicy,
    ValidatedSettingsCatalog, assert_foundation_catalog, resolve_effective_settings,
    view_distance_setting_id,
};
use latticeaxiom_settings_ui::SettingsSurfaceError;
use serde_json::Value;
use thiserror::Error;

use crate::LockVerifiedComposeImages;

const SETTINGS_REGISTRY_CAPABILITY: &str = "latticeaxiom:capability/settings-registry@1";
const SETTINGS_CATALOG_PACKAGE_PATH: &str = "data/user-catalog-v1.json";

/// Failure to load or persist local user settings.
#[derive(Debug, Error)]
pub enum HostSettingsError {
    /// The local-settings store rejected a read or write.
    #[error(transparent)]
    Persist(#[from] latticeaxiom_runtime_contracts::LocalSettingsPersistError),
    /// A stored binding could not be converted into the input catalog profile.
    #[error("stored binding profile is not representable: {reason}")]
    Binding {
        /// Diagnostic.
        reason: String,
    },
    /// A lock-selected settings catalog failed validation.
    #[error(transparent)]
    Catalog(#[from] latticeaxiom_runtime_contracts::SettingsCatalogError),
    /// A settings apply transaction failed.
    #[error(transparent)]
    Transaction(#[from] latticeaxiom_runtime_contracts::SettingsTransactionError),
    /// The package-owned settings surface rejected a catalog or draft.
    #[error(transparent)]
    Surface(#[from] SettingsSurfaceError),
    /// Effective settings overlays could not be resolved.
    #[error("effective settings resolution failed: {reason}")]
    Overlay {
        /// Validation diagnostic.
        reason: String,
    },
    /// The live production spine rejected initial or applied settings.
    #[error("runtime settings apply failed: {reason}")]
    Runtime {
        /// Diagnostic.
        reason: String,
    },
    /// A selected settings package artifact or required row was unavailable.
    #[error("settings registry catalog is unavailable: {reason}")]
    CatalogUnavailable {
        /// Diagnostic.
        reason: String,
    },
}

impl HostSettingsError {
    /// Returns whether the visible settings replacement requires a safe restart.
    #[cfg(any(feature = "client", test))]
    #[must_use]
    pub(crate) const fn requires_safe_process_restart(&self) -> bool {
        match self {
            Self::Transaction(error) => error.requires_safe_process_restart(),
            Self::Persist(error) => error.publication_state_uncertain(),
            _ => false,
        }
    }

    /// Returns whether the proposed value is known to be visible in storage.
    ///
    /// This is intentionally narrower than [`Self::requires_safe_process_restart`]:
    /// failed visible-state recovery requires a restart without proving whether
    /// the old or proposed value owns the visible name.
    #[cfg(any(feature = "client", test))]
    #[must_use]
    pub(crate) fn proposed_value_is_visible_but_durability_uncertain(&self) -> bool {
        match self {
            Self::Transaction(error) => error.visible_proposed_envelope().is_some(),
            Self::Persist(error) => error.proposed_value_is_visible_but_durability_uncertain(),
            _ => false,
        }
    }
}

/// Lock-selected, validated settings catalog.
#[derive(Clone, Debug)]
pub struct HostSettingsCatalog {
    catalog: ValidatedSettingsCatalog,
}

impl HostSettingsCatalog {
    fn new(catalog: ValidatedSettingsCatalog) -> Result<Self, HostSettingsError> {
        assert_foundation_catalog(&catalog)?;
        Ok(Self { catalog })
    }

    /// Returns the validated runtime catalog.
    #[must_use]
    pub const fn as_validated(&self) -> &ValidatedSettingsCatalog {
        &self.catalog
    }
}

/// Loads and compiles the exactly-one settings provider selected by a reopened lock.
///
/// Returns `Ok(None)` only when the graph does not declare the settings-registry
/// capability.
///
/// # Errors
///
/// Returns [`HostSettingsError`] when provider cardinality, verified package
/// data, or the typed catalog contract is invalid.
pub fn compile_lock_selected_settings(
    images: &LockVerifiedComposeImages,
) -> Result<Option<HostSettingsCatalog>, HostSettingsError> {
    let graph = images.images().graph();
    let Some(package) = settings_provider(&graph.capability_providers)? else {
        return Ok(None);
    };
    let data_root = images
        .locked_artifacts()
        .data_root(&package)
        .map_err(|error| HostSettingsError::CatalogUnavailable {
            reason: error.to_string(),
        })?;
    let catalog_path =
        CanonicalLogicalPath::new(SETTINGS_CATALOG_PACKAGE_PATH).map_err(|error| {
            HostSettingsError::CatalogUnavailable {
                reason: error.to_string(),
            }
        })?;
    let file = data_root.require_file(&catalog_path).map_err(|error| {
        HostSettingsError::CatalogUnavailable {
            reason: error.to_string(),
        }
    })?;
    let fragment = SettingsCatalogFragment::from_canonical_json(file)?;
    let catalog = ValidatedSettingsCatalog::compile([fragment], SettingsCatalogPolicy::default())?;
    Ok(Some(HostSettingsCatalog::new(catalog)?))
}

fn settings_provider(
    providers: &BTreeMap<CapabilityId, Vec<PackageName>>,
) -> Result<Option<PackageName>, HostSettingsError> {
    let capability = SETTINGS_REGISTRY_CAPABILITY
        .parse::<CapabilityId>()
        .map_err(|error| HostSettingsError::CatalogUnavailable {
            reason: error.to_string(),
        })?;
    let Some(packages) = providers.get(&capability) else {
        return Ok(None);
    };
    if packages.len() != 1 {
        return Err(HostSettingsError::CatalogUnavailable {
            reason: format!(
                "settings registry requires exactly one provider; found {}",
                packages.len()
            ),
        });
    }
    Ok(packages.first().cloned())
}

/// Loaded user settings that survive shell↔world process replacement.
#[derive(Clone, Debug, PartialEq)]
pub struct HostUserSettings {
    envelope: LatticeLocalSettingsV1,
    profile: InputBindingProfile,
}

impl HostUserSettings {
    /// Loads or creates the canonical local-settings envelope under `root`.
    ///
    /// # Errors
    ///
    /// Returns [`HostSettingsError`] when the store cannot be opened or decoded.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, HostSettingsError> {
        let store = FilesystemLocalSettingsStore::open(root)?;
        let loaded = store.load()?;
        let envelope = match loaded.origin() {
            LocalSettingsOrigin::Complete => loaded.envelope().clone(),
            LocalSettingsOrigin::Missing | LocalSettingsOrigin::Isolated { .. } => {
                LatticeLocalSettingsV1::empty()
            }
        };
        let profile = stored_profile_to_input(envelope.binding_profile())?;
        Ok(Self { envelope, profile })
    }

    /// Returns the compiled-input binding profile.
    #[must_use]
    pub const fn binding_profile(&self) -> &InputBindingProfile {
        &self.profile
    }

    /// Returns the last confirmed settings transaction revision.
    #[must_use]
    pub fn transaction_revision(&self) -> u64 {
        self.envelope.transaction_revision().get()
    }

    /// Resolves the effective local settings snapshot under the active lock.
    ///
    /// # Errors
    ///
    /// Returns an error when a stored overlay contradicts the lock-selected
    /// catalog.
    pub fn effective_snapshot(
        &self,
        catalog: &HostSettingsCatalog,
        active_lock: CanonicalHash,
    ) -> Result<EffectiveSettingsSnapshot, HostSettingsError> {
        resolve_effective_settings(
            catalog.as_validated(),
            [self.envelope.device_overlay(), self.envelope.user_overlay()],
            active_lock,
        )
        .map(latticeaxiom_runtime_contracts::EffectiveSettingsResolution::into_parts)
        .map(|(snapshot, _)| snapshot)
        .map_err(|source| HostSettingsError::Overlay {
            reason: source.to_string(),
        })
    }

    /// Resolves the stored raw view-distance request from the active catalog.
    ///
    /// # Errors
    ///
    /// Returns [`HostSettingsError`] when stored overlays or the required row
    /// violate the active lock-selected catalog.
    pub fn requested_view_distance(
        &self,
        catalog: &HostSettingsCatalog,
        active_lock: CanonicalHash,
    ) -> Result<u32, HostSettingsError> {
        let snapshot = self.effective_snapshot(catalog, active_lock)?;
        let id = view_distance_setting_id();
        let value = snapshot
            .values()
            .get(&id)
            .ok_or_else(|| HostSettingsError::CatalogUnavailable {
                reason: format!("effective snapshot is missing `{id}`"),
            })?
            .value
            .as_i64()
            .ok_or_else(|| HostSettingsError::CatalogUnavailable {
                reason: format!("effective `{id}` is not an integer"),
            })?;
        u32::try_from(value).map_err(|_| HostSettingsError::CatalogUnavailable {
            reason: format!("effective `{id}` is outside the chunk-distance domain"),
        })
    }

    /// Persists one catalog-validated view-distance draft atomically.
    ///
    /// The raw request is retained even when the active host can admit a lower
    /// distance. Runtime clamping is a separate host decision.
    ///
    /// A post-replace durability error adopts the proved-visible proposed
    /// envelope in memory and requires a safe process restart. A failed
    /// visible-state recovery does not adopt either value because storage did
    /// not prove which envelope owns the visible name.
    ///
    /// # Errors
    ///
    /// Returns [`HostSettingsError`] when validation, preparation, or atomic
    /// publication fails.
    pub fn persist_view_distance(
        &mut self,
        root: impl AsRef<Path>,
        catalog: &HostSettingsCatalog,
        active_lock: CanonicalHash,
        requested_chunks: u32,
    ) -> Result<SettingChangedBatchV1, HostSettingsError> {
        let store = FilesystemLocalSettingsStore::open(root)?;
        self.persist_view_distance_to_store(&store, catalog, active_lock, requested_chunks)
    }

    /// Persists one non-empty, catalog-validated user-domain draft atomically.
    ///
    /// # Errors
    ///
    /// Returns [`HostSettingsError`] when validation, preparation, or atomic
    /// publication fails.
    pub fn persist_user_values(
        &mut self,
        root: impl AsRef<Path>,
        catalog: &HostSettingsCatalog,
        active_lock: CanonicalHash,
        proposed: &BTreeMap<latticeaxiom_core::StableId, Value>,
    ) -> Result<SettingChangedBatchV1, HostSettingsError> {
        let store = FilesystemLocalSettingsStore::open(root)?;
        self.persist_user_values_to_store(&store, catalog, active_lock, proposed)
    }

    fn persist_view_distance_to_store(
        &mut self,
        store: &impl LocalSettingsStore,
        catalog: &HostSettingsCatalog,
        active_lock: CanonicalHash,
        requested_chunks: u32,
    ) -> Result<SettingChangedBatchV1, HostSettingsError> {
        let proposed =
            BTreeMap::from([(view_distance_setting_id(), Value::from(requested_chunks))]);
        self.persist_user_values_to_store(store, catalog, active_lock, &proposed)
    }

    fn persist_user_values_to_store(
        &mut self,
        store: &impl LocalSettingsStore,
        catalog: &HostSettingsCatalog,
        active_lock: CanonicalHash,
        proposed: &BTreeMap<latticeaxiom_core::StableId, Value>,
    ) -> Result<SettingChangedBatchV1, HostSettingsError> {
        let before = resolve_effective_settings(
            catalog.as_validated(),
            [self.envelope.device_overlay(), self.envelope.user_overlay()],
            active_lock,
        )
        .map_err(|source| HostSettingsError::Overlay {
            reason: source.to_string(),
        })?;
        let mut transaction = SettingsApplyTransaction::user_draft(
            catalog.as_validated(),
            before.snapshot(),
            &self.envelope,
            proposed,
            self.envelope.binding_profile().clone(),
            PreviewPolicyV1::None,
            active_lock,
        )?;
        transaction.prepare()?;
        match transaction.persist(store, &self.envelope, active_lock) {
            Ok((next, batch)) => {
                self.envelope = next;
                Ok(batch)
            }
            Err(error) => {
                if let Some(proposed) = error.visible_proposed_envelope() {
                    self.envelope = proposed.clone();
                }
                Err(error.into())
            }
        }
    }

    /// Replaces the binding profile and persists it atomically.
    ///
    /// A post-replace durability error adopts the proved-visible profile in
    /// memory. Pre-replace failures and failed visible-state recovery preserve
    /// the last confirmed in-memory profile.
    ///
    /// # Errors
    ///
    /// Returns [`HostSettingsError`] when conversion or publication fails.
    pub fn persist_binding_profile(
        &mut self,
        root: impl AsRef<Path>,
        profile: InputBindingProfile,
    ) -> Result<(), HostSettingsError> {
        let store = FilesystemLocalSettingsStore::open(root)?;
        self.persist_binding_profile_to_store(&store, profile)
    }

    fn persist_binding_profile_to_store(
        &mut self,
        store: &impl LocalSettingsStore,
        profile: InputBindingProfile,
    ) -> Result<(), HostSettingsError> {
        let stored = input_profile_to_stored(&profile)?;
        let next = self
            .envelope
            .with_user_commit(self.envelope.user().clone(), stored);
        match store.persist(&next) {
            Ok(_) => {
                self.envelope = next;
                self.profile = profile;
                Ok(())
            }
            Err(error) => {
                if error.proposed_value_is_visible_but_durability_uncertain() {
                    self.envelope = next;
                    self.profile = profile;
                }
                Err(error.into())
            }
        }
    }
}

fn stored_profile_to_input(
    stored: &StoredBindingProfile,
) -> Result<InputBindingProfile, HostSettingsError> {
    let mut profile = InputBindingProfile::empty();
    for (action, bindings) in stored.overrides() {
        let converted = bindings
            .iter()
            .map(stored_binding_to_input)
            .collect::<Result<Vec<_>, _>>()?;
        profile.set_override(action.clone(), converted);
    }
    Ok(profile)
}

fn input_profile_to_stored(
    profile: &InputBindingProfile,
) -> Result<StoredBindingProfile, HostSettingsError> {
    let mut overrides = std::collections::BTreeMap::new();
    for (action, bindings) in &profile.overrides {
        let converted = bindings
            .iter()
            .map(input_binding_to_stored)
            .collect::<Result<Vec<_>, _>>()?;
        overrides.insert(action.clone(), converted);
    }
    StoredBindingProfile::new(1, overrides).map_err(|error| HostSettingsError::Binding {
        reason: error.to_string(),
    })
}

fn stored_binding_to_input(binding: &StoredBinding) -> Result<InputBindingV1, HostSettingsError> {
    match binding {
        StoredBinding::Known(KnownInputBindingV1::Keyboard { usage, modifiers }) => {
            Ok(InputBindingV1::Keyboard {
                usage: usage.clone(),
                modifiers: stored_modifiers(*modifiers),
            })
        }
        StoredBinding::Known(KnownInputBindingV1::MouseButton { button }) => {
            Ok(InputBindingV1::MouseButton {
                button: match button {
                    StoredMouseButton::Left => latticeaxiom_input::MouseButtonV1::Left,
                    StoredMouseButton::Right => latticeaxiom_input::MouseButtonV1::Right,
                    StoredMouseButton::Middle => latticeaxiom_input::MouseButtonV1::Middle,
                    StoredMouseButton::Extra(_) => {
                        return Err(HostSettingsError::Binding {
                            reason: "extra mouse buttons are not in the V1 input catalog"
                                .to_owned(),
                        });
                    }
                },
            })
        }
        StoredBinding::Known(KnownInputBindingV1::MouseMotion) => {
            Ok(InputBindingV1::MouseMotion { sensitivity: 0.002 })
        }
        StoredBinding::Known(_) | StoredBinding::Unknown(_) => Err(HostSettingsError::Binding {
            reason: "stored binding kind cannot be compiled into the V1 catalog".to_owned(),
        }),
    }
}

fn input_binding_to_stored(binding: &InputBindingV1) -> Result<StoredBinding, HostSettingsError> {
    match binding {
        InputBindingV1::Keyboard { usage, modifiers } => {
            Ok(StoredBinding::Known(KnownInputBindingV1::Keyboard {
                usage: usage.clone(),
                modifiers: input_modifiers(modifiers),
            }))
        }
        InputBindingV1::MouseButton { button } => {
            Ok(StoredBinding::Known(KnownInputBindingV1::MouseButton {
                button: match button {
                    latticeaxiom_input::MouseButtonV1::Left => StoredMouseButton::Left,
                    latticeaxiom_input::MouseButtonV1::Right => StoredMouseButton::Right,
                    latticeaxiom_input::MouseButtonV1::Middle => StoredMouseButton::Middle,
                    latticeaxiom_input::MouseButtonV1::Back
                    | latticeaxiom_input::MouseButtonV1::Forward => {
                        return Err(HostSettingsError::Binding {
                            reason: "forward/back mouse buttons are not stored in V1 settings"
                                .to_owned(),
                        });
                    }
                },
            }))
        }
        _ => Err(HostSettingsError::Binding {
            reason: "gamepad and axis recipes stay package-fixed in V1".to_owned(),
        }),
    }
}

fn stored_modifiers(modifiers: KeyboardModifiersV1) -> BTreeSet<KeyModifierV1> {
    let mut set = BTreeSet::new();
    if modifiers.shift {
        set.insert(KeyModifierV1::Shift);
    }
    if modifiers.control {
        set.insert(KeyModifierV1::Control);
    }
    if modifiers.alt {
        set.insert(KeyModifierV1::Alt);
    }
    if modifiers.super_key {
        set.insert(KeyModifierV1::Super);
    }
    set
}

fn input_modifiers(modifiers: &BTreeSet<KeyModifierV1>) -> KeyboardModifiersV1 {
    KeyboardModifiersV1 {
        shift: modifiers.contains(&KeyModifierV1::Shift),
        control: modifiers.contains(&KeyModifierV1::Control),
        alt: modifiers.contains(&KeyModifierV1::Alt),
        super_key: modifiers.contains(&KeyModifierV1::Super),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailedVisibleRecoveryStore;

    impl LocalSettingsStore for FailedVisibleRecoveryStore {
        fn load(
            &self,
        ) -> Result<
            latticeaxiom_runtime_contracts::LocalSettingsLoad,
            latticeaxiom_runtime_contracts::LocalSettingsPersistError,
        > {
            Err(latticeaxiom_runtime_contracts::LocalSettingsPersistError::StatePoisoned)
        }

        fn persist(
            &self,
            _envelope: &LatticeLocalSettingsV1,
        ) -> Result<
            latticeaxiom_runtime_contracts::LocalSettingsPublishReceipt,
            latticeaxiom_runtime_contracts::LocalSettingsPersistError,
        > {
            Err(
                latticeaxiom_runtime_contracts::LocalSettingsPersistError::VisibleRecoveryFailed {
                    reason: "injected failed backup restoration".to_owned(),
                },
            )
        }
    }

    struct ProductionDurabilityUncertainStore;

    impl LocalSettingsStore for ProductionDurabilityUncertainStore {
        fn load(
            &self,
        ) -> Result<
            latticeaxiom_runtime_contracts::LocalSettingsLoad,
            latticeaxiom_runtime_contracts::LocalSettingsPersistError,
        > {
            Err(latticeaxiom_runtime_contracts::LocalSettingsPersistError::StatePoisoned)
        }

        fn persist(
            &self,
            _envelope: &LatticeLocalSettingsV1,
        ) -> Result<
            latticeaxiom_runtime_contracts::LocalSettingsPublishReceipt,
            latticeaxiom_runtime_contracts::LocalSettingsPersistError,
        > {
            Err(
                latticeaxiom_runtime_contracts::LocalSettingsPersistError::PublicationDurabilityUncertain {
                    reason: "injected production directory-sync failure".to_owned(),
                },
            )
        }
    }

    fn capability() -> CapabilityId {
        SETTINGS_REGISTRY_CAPABILITY
            .parse()
            .expect("settings-registry capability is canonical")
    }

    fn package(name: &str) -> PackageName {
        name.parse().expect("test package name is canonical")
    }

    fn foundation_catalog() -> HostSettingsCatalog {
        let catalog = ValidatedSettingsCatalog::compile(
            [SettingsCatalogFragment::new(
                latticeaxiom_runtime_contracts::settings_package_name(),
                latticeaxiom_runtime_contracts::foundation_setting_specs(),
            )],
            SettingsCatalogPolicy::default(),
        )
        .expect("foundation settings catalog compiles");
        HostSettingsCatalog::new(catalog).expect("foundation catalog projects a slider")
    }

    fn non_empty_binding_profile() -> InputBindingProfile {
        let action = "example:action/explicitly-unbound"
            .parse()
            .expect("test action ID is canonical");
        let mut profile = InputBindingProfile::empty();
        profile.set_override(action, Vec::new());
        profile
    }

    #[test]
    fn directory_sync_uncertainty_adopts_the_visible_proposed_envelope() {
        let catalog = foundation_catalog();
        let active_lock = CanonicalHash::digest(b"settings-lock");
        let mut user = HostUserSettings {
            envelope: LatticeLocalSettingsV1::empty(),
            profile: InputBindingProfile::empty(),
        };
        let store = latticeaxiom_runtime_contracts::DeterministicLocalSettingsStore::new();
        store
            .inject_fault(latticeaxiom_runtime_contracts::LocalSettingsFaultPoint::DirectorySync)
            .expect("directory-sync fault is armed");

        let error = user
            .persist_view_distance_to_store(&store, &catalog, active_lock, 24)
            .expect_err("directory-sync uncertainty must not claim a durable save");
        assert!(error.requires_safe_process_restart());
        assert!(error.proposed_value_is_visible_but_durability_uncertain());
        assert_eq!(
            user.requested_view_distance(&catalog, active_lock)
                .expect("adopted envelope resolves"),
            24
        );

        let visible = store
            .load()
            .expect("the deterministic post-replace value remains visible");
        assert_eq!(visible.envelope(), &user.envelope);
        assert_eq!(
            visible
                .envelope()
                .user()
                .get(&view_distance_setting_id())
                .map(latticeaxiom_runtime_contracts::StoredSettingEntryV1::value),
            Some(&Value::from(24))
        );
    }

    #[test]
    fn production_durability_uncertainty_propagates_and_adopts_visible_state() {
        let catalog = foundation_catalog();
        let active_lock = CanonicalHash::digest(b"settings-lock");
        let mut user = HostUserSettings {
            envelope: LatticeLocalSettingsV1::empty(),
            profile: InputBindingProfile::empty(),
        };

        let error = user
            .persist_view_distance_to_store(
                &ProductionDurabilityUncertainStore,
                &catalog,
                active_lock,
                24,
            )
            .expect_err("production durability uncertainty must not claim a durable save");

        assert!(matches!(
            &error,
            HostSettingsError::Transaction(
                latticeaxiom_runtime_contracts::SettingsTransactionError::PublicationStateUncertain {
                    source:
                        latticeaxiom_runtime_contracts::LocalSettingsPersistError::PublicationDurabilityUncertain {
                            reason
                        },
                    ..
                }
            ) if reason.contains("production directory-sync failure")
        ));
        assert!(error.requires_safe_process_restart());
        assert!(error.proposed_value_is_visible_but_durability_uncertain());
        assert_eq!(
            user.requested_view_distance(&catalog, active_lock)
                .expect("the proposed visible envelope was adopted"),
            24
        );
    }

    #[test]
    fn failed_visible_recovery_does_not_adopt_an_unproved_envelope() {
        let catalog = foundation_catalog();
        let active_lock = CanonicalHash::digest(b"settings-lock");
        let mut user = HostUserSettings {
            envelope: LatticeLocalSettingsV1::empty(),
            profile: InputBindingProfile::empty(),
        };
        let confirmed = user.clone();

        let error = user
            .persist_view_distance_to_store(&FailedVisibleRecoveryStore, &catalog, active_lock, 24)
            .expect_err("failed visible recovery must not claim a durable save");

        assert!(error.requires_safe_process_restart());
        assert!(!error.proposed_value_is_visible_but_durability_uncertain());
        assert_eq!(user, confirmed);
    }

    #[test]
    fn binding_profile_pre_replace_failure_preserves_confirmed_memory() {
        let mut user = HostUserSettings {
            envelope: LatticeLocalSettingsV1::empty(),
            profile: InputBindingProfile::empty(),
        };
        let confirmed = user.clone();
        let store = latticeaxiom_runtime_contracts::DeterministicLocalSettingsStore::new();
        store
            .inject_fault(latticeaxiom_runtime_contracts::LocalSettingsFaultPoint::TempWrite)
            .expect("temporary-write fault is armed");

        let error = user
            .persist_binding_profile_to_store(&store, non_empty_binding_profile())
            .expect_err("pre-replace failure must not claim a durable save");
        assert!(!error.requires_safe_process_restart());
        assert_eq!(user, confirmed);
    }

    #[test]
    fn binding_profile_failed_visible_recovery_preserves_confirmed_memory() {
        let mut user = HostUserSettings {
            envelope: LatticeLocalSettingsV1::empty(),
            profile: InputBindingProfile::empty(),
        };
        let confirmed = user.clone();

        let error = user
            .persist_binding_profile_to_store(
                &FailedVisibleRecoveryStore,
                non_empty_binding_profile(),
            )
            .expect_err("failed visible recovery must not claim a durable save");

        assert!(error.requires_safe_process_restart());
        assert!(!error.proposed_value_is_visible_but_durability_uncertain());
        assert_eq!(user, confirmed);
    }

    #[test]
    fn binding_profile_directory_sync_uncertainty_adopts_visible_state() {
        let mut user = HostUserSettings {
            envelope: LatticeLocalSettingsV1::empty(),
            profile: InputBindingProfile::empty(),
        };
        let profile = non_empty_binding_profile();
        let store = latticeaxiom_runtime_contracts::DeterministicLocalSettingsStore::new();
        store
            .inject_fault(latticeaxiom_runtime_contracts::LocalSettingsFaultPoint::DirectorySync)
            .expect("directory-sync fault is armed");

        let error = user
            .persist_binding_profile_to_store(&store, profile.clone())
            .expect_err("directory-sync uncertainty must not claim a durable save");
        assert!(error.requires_safe_process_restart());
        assert!(error.proposed_value_is_visible_but_durability_uncertain());
        assert_eq!(user.binding_profile(), &profile);
        assert_eq!(user.transaction_revision(), 1);

        let visible = store
            .load()
            .expect("the deterministic post-replace profile remains visible");
        assert_eq!(visible.envelope(), &user.envelope);
    }

    #[test]
    fn settings_provider_is_absent_without_capability_evidence() {
        let providers = BTreeMap::new();
        assert!(
            settings_provider(&providers)
                .expect("an absent capability is valid")
                .is_none()
        );
    }

    #[test]
    fn settings_provider_requires_exactly_one_capability_provider() {
        let selected = package("@example/settings");
        let providers = BTreeMap::from([(capability(), vec![selected.clone()])]);
        assert_eq!(
            settings_provider(&providers).expect("one provider is valid"),
            Some(selected)
        );

        for invalid in [
            Vec::new(),
            vec![package("@example/a"), package("@example/b")],
        ] {
            let providers = BTreeMap::from([(capability(), invalid)]);
            assert!(settings_provider(&providers).is_err());
        }
    }
}
