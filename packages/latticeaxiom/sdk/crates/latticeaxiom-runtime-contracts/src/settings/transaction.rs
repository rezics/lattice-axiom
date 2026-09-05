//! One-domain prepare, preview, apply, and rollback transactions.

use std::collections::BTreeMap;

use latticeaxiom_compose::{RuntimeApplyImpact, SettingScope};
use latticeaxiom_core::{CanonicalHash, PackageName, StableId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::binding::BindingProfileV1;
use super::catalog::{ValidatedSettingsCatalog, validate_setting_value};
use super::overlay::{
    EffectiveSettingsSnapshot, RestartImpactMetadata, ScopeOverlay, SettingTransactionRevision,
    SettingWriter, SettingsDiff, SettingsRollbackPlan, resolve_effective_settings,
};
use super::persist::{
    LatticeLocalSettingsV1, LocalSettingsPersistError, LocalSettingsStore, PendingRestartJournalV1,
    StoredSettingEntryV1,
};

/// Durability boundary for one honest settings transaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsDurabilityDomain {
    /// Device-local application settings.
    Device,
    /// User-local application settings.
    User,
    /// One authoritative world metadata transaction.
    World,
    /// One player's world-owned transaction.
    PlayerWorld,
    /// Memory-only session state.
    Session,
}

impl From<SettingScope> for SettingsDurabilityDomain {
    fn from(value: SettingScope) -> Self {
        match value {
            SettingScope::Device => Self::Device,
            SettingScope::User => Self::User,
            SettingScope::World => Self::World,
            SettingScope::PlayerWorld => Self::PlayerWorld,
            SettingScope::Session => Self::Session,
        }
    }
}

/// Reversible presentation-preview policy declared outside apply impact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "policy")]
pub enum PreviewPolicyV1 {
    /// No preview callback is used.
    None,
    /// Preview must roll back after a non-zero timeout unless confirmed.
    Reversible {
        /// Timeout in milliseconds.
        timeout_ms: u32,
        /// Generated participant owners in stable order.
        participants: Vec<PackageName>,
    },
}

/// Current apply transaction phase.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsTransactionPhase {
    /// Typed draft exists but no participant has prepared it.
    Draft,
    /// Participants prepared in stable owner/key order.
    Prepared,
    /// Reversible preview is active and timed.
    Previewing,
    /// Atomic store persistence has been requested.
    Persisting,
    /// Durable value is published; rollback must not retract it.
    Committed,
    /// Pre-persist rollback is being executed.
    RollingBack,
    /// Draft/preview was rolled back completely.
    RolledBack,
    /// Rollback could not be proven; safe process restart is required.
    SafeProcessRestartRequired,
}

/// Stable change batch published after a successful persist.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingChangedBatchV1 {
    transaction_revision: SettingTransactionRevision,
    active_lock: CanonicalHash,
    domain: SettingsDurabilityDomain,
    impact: Option<RuntimeApplyImpact>,
    restart_impact: RestartImpactMetadata,
    entries: Vec<super::overlay::SettingDiffEntry>,
}

impl SettingChangedBatchV1 {
    /// Returns the committed transaction revision.
    #[must_use]
    pub const fn transaction_revision(&self) -> SettingTransactionRevision {
        self.transaction_revision
    }

    /// Returns restart-impact metadata disclosed before persistence.
    #[must_use]
    pub const fn restart_impact(&self) -> RestartImpactMetadata {
        self.restart_impact
    }

    /// Returns changed rows in stable setting-ID order.
    #[must_use]
    pub fn entries(&self) -> &[super::overlay::SettingDiffEntry] {
        &self.entries
    }
}

/// One-domain settings transaction with apply/preview/rollback semantics.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsApplyTransaction {
    domain: SettingsDurabilityDomain,
    diff: SettingsDiff,
    rollback: SettingsRollbackPlan,
    preview: PreviewPolicyV1,
    phase: SettingsTransactionPhase,
    committed_revision: Option<SettingTransactionRevision>,
    proposed_user: BTreeMap<StableId, StoredSettingEntryV1>,
    proposed_binding_profile: BindingProfileV1,
}

impl SettingsApplyTransaction {
    /// Creates a user-domain draft from confirmed and proposed envelopes.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsTransactionError`] when the draft is empty, spans more
    /// than the user domain, or the proposed snapshot fails catalog validation.
    pub fn user_draft(
        catalog: &ValidatedSettingsCatalog,
        before: &EffectiveSettingsSnapshot,
        current: &LatticeLocalSettingsV1,
        proposed_user: &BTreeMap<StableId, Value>,
        proposed_binding_profile: BindingProfileV1,
        preview: PreviewPolicyV1,
        active_lock: CanonicalHash,
    ) -> Result<Self, SettingsTransactionError> {
        if let PreviewPolicyV1::Reversible { timeout_ms, .. } = preview
            && timeout_ms == 0
        {
            return Err(SettingsTransactionError::ZeroPreviewTimeout);
        }
        let mut stored = current.user().clone();
        for (id, value) in proposed_user {
            let Some(spec) = catalog.as_catalog().runtime.get(id) else {
                return Err(SettingsTransactionError::UnknownSetting {
                    setting: id.clone(),
                });
            };
            if !spec.allowed_scopes.contains(&SettingScope::User) {
                return Err(SettingsTransactionError::ScopeNotAllowed {
                    setting: id.clone(),
                });
            }
            validate_setting_value(spec, value).map_err(|source| {
                SettingsTransactionError::InvalidValue {
                    setting: id.clone(),
                    reason: source.to_string(),
                }
            })?;
            stored.insert(
                id.clone(),
                StoredSettingEntryV1::new(spec.schema_version, value.clone()),
            );
        }
        let proposed_user_overlay = ScopeOverlay::new(
            SettingScope::User,
            current.store_revision(),
            current.transaction_revision(),
            SettingWriter::LocalUser,
            None,
            stored
                .iter()
                .map(|(id, entry)| (id.clone(), entry.value().clone()))
                .collect(),
        );
        let proposed = resolve_effective_settings(
            catalog,
            [current.device_overlay(), proposed_user_overlay],
            active_lock,
        )
        .map_err(|source| SettingsTransactionError::Overlay(source.to_string()))?;
        let diff = before.diff(proposed.snapshot());
        if diff.is_empty() && proposed_binding_profile == *current.binding_profile() {
            return Err(SettingsTransactionError::EmptyDraft);
        }
        let rollback = diff.pre_persist_rollback_plan();
        Ok(Self {
            domain: SettingsDurabilityDomain::User,
            diff,
            rollback,
            preview,
            phase: SettingsTransactionPhase::Draft,
            committed_revision: None,
            proposed_user: stored,
            proposed_binding_profile,
        })
    }

    /// Returns the single durability domain.
    #[must_use]
    pub const fn domain(&self) -> SettingsDurabilityDomain {
        self.domain
    }

    /// Returns the stable complete diff shown before apply.
    #[must_use]
    pub const fn diff(&self) -> &SettingsDiff {
        &self.diff
    }

    /// Returns restart-impact metadata for this draft.
    #[must_use]
    pub fn restart_impact(&self) -> RestartImpactMetadata {
        self.diff.restart_impact()
    }

    /// Returns the current phase.
    #[must_use]
    pub const fn phase(&self) -> SettingsTransactionPhase {
        self.phase
    }

    /// Returns the committed revision, if any.
    #[must_use]
    pub const fn committed_revision(&self) -> Option<SettingTransactionRevision> {
        self.committed_revision
    }

    /// Starts participant preparation in stable owner/setting order.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsTransactionError`] if the transaction is not in Draft.
    pub fn prepare(&mut self) -> Result<(), SettingsTransactionError> {
        self.require_phase(SettingsTransactionPhase::Draft)?;
        self.phase = SettingsTransactionPhase::Prepared;
        Ok(())
    }

    /// Begins a declared reversible preview after preparation.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsTransactionError`] if no valid reversible policy exists
    /// or the transaction is not prepared.
    pub fn begin_preview(&mut self) -> Result<u32, SettingsTransactionError> {
        self.require_phase(SettingsTransactionPhase::Prepared)?;
        let PreviewPolicyV1::Reversible { timeout_ms, .. } = self.preview else {
            return Err(SettingsTransactionError::PreviewNotDeclared);
        };
        self.phase = SettingsTransactionPhase::Previewing;
        Ok(timeout_ms)
    }

    /// Cancels preparation or handles preview timeout before persistence.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsTransactionError`] after persistence or in another phase.
    pub fn rollback_before_persist(
        &mut self,
    ) -> Result<SettingsRollbackPlan, SettingsTransactionError> {
        if !matches!(
            self.phase,
            SettingsTransactionPhase::Prepared | SettingsTransactionPhase::Previewing
        ) {
            return Err(SettingsTransactionError::InvalidPhase);
        }
        self.phase = SettingsTransactionPhase::RollingBack;
        Ok(self.rollback.clone())
    }

    /// Records whether every reverse rollback step completed.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsTransactionError`] outside `RollingBack`.
    pub fn finish_rollback(&mut self, complete: bool) -> Result<(), SettingsTransactionError> {
        self.require_phase(SettingsTransactionPhase::RollingBack)?;
        self.phase = if complete {
            SettingsTransactionPhase::RolledBack
        } else {
            SettingsTransactionPhase::SafeProcessRestartRequired
        };
        Ok(())
    }

    /// Atomically persists the user-domain draft.
    ///
    /// Immediate and `WorldReactivate` commits replace the confirmed snapshot.
    /// `ProcessRestart` stages a pending journal and keeps the old confirmed
    /// values until [`LatticeLocalSettingsV1::confirm_pending_restart`].
    ///
    /// # Errors
    ///
    /// Returns [`SettingsTransactionError`] outside a pre-persist prepared
    /// state, for a non-user domain, or when the store rejects the write.
    pub fn persist<S: LocalSettingsStore>(
        &mut self,
        store: &S,
        current: &LatticeLocalSettingsV1,
        active_lock: CanonicalHash,
    ) -> Result<(LatticeLocalSettingsV1, SettingChangedBatchV1), SettingsTransactionError> {
        if !matches!(
            self.phase,
            SettingsTransactionPhase::Prepared | SettingsTransactionPhase::Previewing
        ) {
            return Err(SettingsTransactionError::InvalidPhase);
        }
        if self.domain != SettingsDurabilityDomain::User {
            return Err(SettingsTransactionError::UnsupportedDomain {
                domain: self.domain,
            });
        }
        let prepared_phase = self.phase;
        self.phase = SettingsTransactionPhase::Persisting;
        let impact = self.diff.required_impact();
        let next = if matches!(impact, Some(RuntimeApplyImpact::ProcessRestart)) {
            let journal = PendingRestartJournalV1::new(
                current.store_revision().get().saturating_add(1),
                current.transaction_revision().saturating_next(),
                self.proposed_user.clone(),
                current.device().clone(),
                self.proposed_binding_profile.clone(),
            );
            current.with_pending_restart(journal)
        } else {
            current.with_user_commit(
                merge_user_values(current, &self.proposed_user),
                self.proposed_binding_profile.clone(),
            )
        };
        if let Err(error) = store.persist(&next) {
            if error.publication_state_uncertain() {
                self.phase = SettingsTransactionPhase::SafeProcessRestartRequired;
                return Err(SettingsTransactionError::PublicationStateUncertain {
                    source: error,
                    proposed: Box::new(next),
                });
            }
            self.phase = prepared_phase;
            return Err(SettingsTransactionError::Persist(error));
        }
        let transaction_revision = next.transaction_revision();
        self.committed_revision = Some(transaction_revision);
        self.phase = SettingsTransactionPhase::Committed;
        Ok((
            next,
            SettingChangedBatchV1 {
                transaction_revision,
                active_lock,
                domain: self.domain,
                impact,
                restart_impact: self.diff.restart_impact(),
                entries: self.diff.entries().to_vec(),
            },
        ))
    }
}

impl SettingsApplyTransaction {
    fn require_phase(
        &self,
        expected: SettingsTransactionPhase,
    ) -> Result<(), SettingsTransactionError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(SettingsTransactionError::InvalidPhase)
        }
    }
}

fn merge_user_values(
    current: &LatticeLocalSettingsV1,
    proposed: &BTreeMap<StableId, StoredSettingEntryV1>,
) -> BTreeMap<StableId, StoredSettingEntryV1> {
    let mut merged = current.user().clone();
    for (id, entry) in proposed {
        merged.insert(id.clone(), entry.clone());
    }
    merged
}

/// Settings transaction failure.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum SettingsTransactionError {
    /// Operation does not apply to the current phase.
    #[error("settings transaction operation is invalid in the current phase")]
    InvalidPhase,
    /// No changes exist to prepare.
    #[error("settings draft is empty")]
    EmptyDraft,
    /// Preview was requested without a reversible policy.
    #[error("settings preview was not declared reversible")]
    PreviewNotDeclared,
    /// A reversible preview requires a non-zero timeout.
    #[error("settings preview timeout must be non-zero")]
    ZeroPreviewTimeout,
    /// A draft attempted to author an ID absent from the active catalog.
    #[error("setting `{setting}` is not declared by the active catalog")]
    UnknownSetting {
        /// Rejected setting ID.
        setting: StableId,
    },
    /// The setting is not writable in the user durability domain.
    #[error("setting `{setting}` is not writable in the user durability domain")]
    ScopeNotAllowed {
        /// Rejected setting ID.
        setting: StableId,
    },
    /// A proposed value failed catalog validation.
    #[error("setting `{setting}` is invalid: {reason}")]
    InvalidValue {
        /// Rejected setting ID.
        setting: StableId,
        /// Validation diagnostic.
        reason: String,
    },
    /// Overlay resolution failed for the proposed snapshot.
    #[error("settings overlay failed: {0}")]
    Overlay(String),
    /// Persistence is implemented only for the user durability domain here.
    #[error("settings domain `{domain:?}` cannot persist through the local user store")]
    UnsupportedDomain {
        /// Rejected domain.
        domain: SettingsDurabilityDomain,
    },
    /// The local-settings store rejected the atomic write.
    #[error(transparent)]
    Persist(LocalSettingsPersistError),
    /// The store reached a state where ordinary rollback cannot prove visibility.
    #[error("settings publication state is uncertain; safe process restart required: {source}")]
    PublicationStateUncertain {
        /// Stage-specific storage failure.
        #[source]
        source: LocalSettingsPersistError,
        /// Proposed envelope needed to reconcile a visible post-replace value.
        proposed: Box<LatticeLocalSettingsV1>,
    },
}

impl SettingsTransactionError {
    /// Returns whether ordinary pre-persist rollback is forbidden.
    #[must_use]
    pub const fn requires_safe_process_restart(&self) -> bool {
        matches!(self, Self::PublicationStateUncertain { .. })
    }

    /// Returns the proposed envelope when the new visible value is proven present.
    #[must_use]
    pub fn visible_proposed_envelope(&self) -> Option<&LatticeLocalSettingsV1> {
        match self {
            Self::PublicationStateUncertain { source, proposed }
                if source.proposed_value_is_visible_but_durability_uncertain() =>
            {
                Some(proposed)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_compose::{SettingAuthority, SettingSensitivity, SettingSpec, ValueType};
    use serde_json::json;
    use std::collections::BTreeSet;

    use crate::settings::catalog::{
        SettingsCatalogFragment, SettingsCatalogPolicy, ValidatedSettingsCatalog,
    };
    use crate::settings::overlay::resolve_effective_settings;
    use crate::settings::persist::{
        DeterministicLocalSettingsStore, LocalSettingsFaultPoint, LocalSettingsPersistError,
    };

    fn package(value: &str) -> PackageName {
        match value.parse() {
            Ok(value) => value,
            Err(error) => panic!("invalid package fixture `{value}`: {error}"),
        }
    }

    fn id(value: &str) -> StableId {
        match value.parse() {
            Ok(value) => value,
            Err(error) => panic!("invalid stable-ID fixture `{value}`: {error}"),
        }
    }

    fn bool_setting(path: &str) -> SettingSpec {
        SettingSpec {
            id: id(&format!("example:setting/{path}")),
            declared_by: package("@example/runtime"),
            schema_version: 1,
            value_type: ValueType::Bool,
            default: json!(false),
            allowed_scopes: BTreeSet::from([SettingScope::User, SettingScope::Session]),
            default_scope: SettingScope::User,
            authority: SettingAuthority::LocalUser,
            apply_impact: RuntimeApplyImpact::Immediate,
            category: id("example:setting-category/general"),
            order: 0,
            label_key: "label".to_owned(),
            description_key: "description".to_owned(),
            visibility: None,
            enabled_when: None,
            sensitivity: SettingSensitivity::Ordinary,
            replacement: None,
        }
    }

    fn catalog(settings: Vec<SettingSpec>) -> ValidatedSettingsCatalog {
        match ValidatedSettingsCatalog::compile(
            [SettingsCatalogFragment::new(
                package("@example/runtime"),
                settings,
            )],
            SettingsCatalogPolicy::default(),
        ) {
            Ok(value) => value,
            Err(error) => panic!("catalog fixture: {error}"),
        }
    }

    #[test]
    fn preview_cancel_rolls_back_in_reverse_stable_order() {
        let alpha = bool_setting("alpha");
        let beta = bool_setting("beta");
        let catalog = catalog(vec![alpha.clone(), beta.clone()]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([
            (alpha.id.clone(), json!(true)),
            (beta.id.clone(), json!(true)),
        ]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::Reversible {
                timeout_ms: 10_000,
                participants: vec![package("@example/runtime")],
            },
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }
        assert_eq!(transaction.begin_preview(), Ok(10_000));
        let rollback = match transaction.rollback_before_persist() {
            Ok(value) => value,
            Err(error) => panic!("rollback: {error}"),
        };
        assert_eq!(
            rollback
                .steps()
                .iter()
                .map(|step| step.id.to_string())
                .collect::<Vec<_>>(),
            vec![
                "example:setting/beta".to_owned(),
                "example:setting/alpha".to_owned()
            ]
        );
        match transaction.finish_rollback(true) {
            Ok(()) => {}
            Err(error) => panic!("finish rollback: {error}"),
        }
        assert_eq!(transaction.phase(), SettingsTransactionPhase::RolledBack);
    }

    #[test]
    fn persist_after_preview_cannot_be_rolled_back() {
        let spec = bool_setting("alpha");
        let catalog = catalog(vec![spec.clone()]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([(spec.id, json!(true))]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::None,
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }
        let store = DeterministicLocalSettingsStore::new();
        let (next, batch) = match transaction.persist(&store, &current, lock) {
            Ok(value) => value,
            Err(error) => panic!("persist: {error}"),
        };
        assert_eq!(transaction.phase(), SettingsTransactionPhase::Committed);
        assert!(transaction.rollback_before_persist().is_err());
        assert!(batch.restart_impact().live_immediate);
        assert!(next.pending_restart().is_none());
        assert_eq!(next.user().len(), 1);
    }

    #[test]
    fn partial_user_draft_preserves_unrelated_confirmed_values() {
        let alpha = bool_setting("alpha");
        let beta = bool_setting("beta");
        let catalog = catalog(vec![alpha.clone(), beta.clone()]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty().with_user_commit(
            BTreeMap::from([
                (
                    alpha.id.clone(),
                    StoredSettingEntryV1::new(alpha.schema_version, json!(true)),
                ),
                (
                    beta.id.clone(),
                    StoredSettingEntryV1::new(beta.schema_version, json!(true)),
                ),
            ]),
            BindingProfileV1::empty(),
        );
        let before = match resolve_effective_settings(&catalog, [current.user_overlay()], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([(alpha.id.clone(), json!(false))]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::None,
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        assert_eq!(
            transaction
                .diff()
                .entries()
                .iter()
                .map(|entry| &entry.id)
                .collect::<Vec<_>>(),
            vec![&alpha.id]
        );
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }
        let store = DeterministicLocalSettingsStore::new();
        let (next, _) = match transaction.persist(&store, &current, lock) {
            Ok(value) => value,
            Err(error) => panic!("persist: {error}"),
        };
        assert_eq!(
            next.user().get(&alpha.id).map(StoredSettingEntryV1::value),
            Some(&json!(false))
        );
        assert_eq!(
            next.user().get(&beta.id).map(StoredSettingEntryV1::value),
            Some(&json!(true))
        );
    }

    #[test]
    fn user_draft_rejects_ids_absent_from_active_catalog() {
        let spec = bool_setting("alpha");
        let catalog = catalog(vec![spec]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let unknown = id("example:setting/not-declared");
        let proposed = BTreeMap::from([(unknown.clone(), json!(true))]);
        assert!(matches!(
            SettingsApplyTransaction::user_draft(
                &catalog,
                before.snapshot(),
                &current,
                &proposed,
                BindingProfileV1::empty(),
                PreviewPolicyV1::None,
                lock,
            ),
            Err(SettingsTransactionError::UnknownSetting { setting }) if setting == unknown
        ));
    }

    #[test]
    fn persist_fault_restores_the_previewing_phase_for_rollback() {
        let spec = bool_setting("alpha");
        let catalog = catalog(vec![spec.clone()]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([(spec.id, json!(true))]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::Reversible {
                timeout_ms: 10_000,
                participants: vec![package("@example/runtime")],
            },
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }
        assert_eq!(transaction.begin_preview(), Ok(10_000));
        let store = DeterministicLocalSettingsStore::new();
        match store.inject_fault(LocalSettingsFaultPoint::TempWrite) {
            Ok(()) => {}
            Err(error) => panic!("inject fault: {error}"),
        }
        assert!(matches!(
            transaction.persist(&store, &current, lock),
            Err(SettingsTransactionError::Persist(
                LocalSettingsPersistError::Injected(LocalSettingsFaultPoint::TempWrite)
            ))
        ));
        assert_eq!(transaction.phase(), SettingsTransactionPhase::Previewing);
        assert!(transaction.rollback_before_persist().is_ok());
        assert_eq!(transaction.phase(), SettingsTransactionPhase::RollingBack);
    }

    #[test]
    fn directory_sync_fault_requires_safe_restart_and_forbids_rollback() {
        let spec = bool_setting("post-replace");
        let setting_id = spec.id.clone();
        let catalog = catalog(vec![spec]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([(setting_id.clone(), json!(true))]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::None,
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }
        let store = DeterministicLocalSettingsStore::new();
        match store.inject_fault(LocalSettingsFaultPoint::DirectorySync) {
            Ok(()) => {}
            Err(error) => panic!("inject fault: {error}"),
        }
        let Err(error) = transaction.persist(&store, &current, lock) else {
            panic!("directory-sync fault must not claim a durable commit");
        };
        assert!(matches!(
            &error,
            SettingsTransactionError::PublicationStateUncertain {
                source: LocalSettingsPersistError::Injected(LocalSettingsFaultPoint::DirectorySync),
                ..
            }
        ));
        assert!(error.visible_proposed_envelope().is_some());
        assert!(error.requires_safe_process_restart());
        assert_eq!(
            transaction.phase(),
            SettingsTransactionPhase::SafeProcessRestartRequired
        );
        assert!(transaction.rollback_before_persist().is_err());

        let loaded = match store.load() {
            Ok(value) => value,
            Err(error) => panic!("load visible replacement: {error}"),
        };
        assert_eq!(
            loaded
                .envelope()
                .user()
                .get(&setting_id)
                .map(StoredSettingEntryV1::value),
            Some(&json!(true))
        );
    }

    #[test]
    fn production_durability_uncertainty_exposes_proposed_and_forbids_rollback() {
        struct ProductionDurabilityUncertainStore;

        impl LocalSettingsStore for ProductionDurabilityUncertainStore {
            fn load(
                &self,
            ) -> Result<crate::settings::persist::LocalSettingsLoad, LocalSettingsPersistError>
            {
                Err(LocalSettingsPersistError::StatePoisoned)
            }

            fn persist(
                &self,
                _envelope: &LatticeLocalSettingsV1,
            ) -> Result<
                crate::settings::persist::LocalSettingsPublishReceipt,
                LocalSettingsPersistError,
            > {
                Err(LocalSettingsPersistError::PublicationDurabilityUncertain {
                    reason: "injected production directory-sync failure".to_owned(),
                })
            }
        }

        let spec = bool_setting("production-post-replace");
        let setting_id = spec.id.clone();
        let catalog = catalog(vec![spec]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([(setting_id.clone(), json!(true))]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::None,
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }

        let Err(error) = transaction.persist(&ProductionDurabilityUncertainStore, &current, lock)
        else {
            panic!("production durability uncertainty must not claim a durable commit");
        };
        assert!(matches!(
            &error,
            SettingsTransactionError::PublicationStateUncertain {
                source: LocalSettingsPersistError::PublicationDurabilityUncertain { reason },
                ..
            } if reason.contains("production directory-sync failure")
        ));
        assert!(error.requires_safe_process_restart());
        let proposed = error
            .visible_proposed_envelope()
            .expect("production post-replace uncertainty proves the proposed envelope is visible");
        assert_eq!(
            proposed
                .user()
                .get(&setting_id)
                .map(StoredSettingEntryV1::value),
            Some(&json!(true))
        );
        assert_eq!(
            transaction.phase(),
            SettingsTransactionPhase::SafeProcessRestartRequired
        );
        assert!(transaction.rollback_before_persist().is_err());
    }

    #[test]
    fn failed_visible_recovery_requires_restart_without_assuming_proposed_visibility() {
        struct FailedRecoveryStore;

        impl LocalSettingsStore for FailedRecoveryStore {
            fn load(
                &self,
            ) -> Result<crate::settings::persist::LocalSettingsLoad, LocalSettingsPersistError>
            {
                Err(LocalSettingsPersistError::StatePoisoned)
            }

            fn persist(
                &self,
                _envelope: &LatticeLocalSettingsV1,
            ) -> Result<
                crate::settings::persist::LocalSettingsPublishReceipt,
                LocalSettingsPersistError,
            > {
                Err(LocalSettingsPersistError::VisibleRecoveryFailed {
                    reason: "injected failed backup restore".to_owned(),
                })
            }
        }

        let spec = bool_setting("failed-recovery");
        let catalog = catalog(vec![spec.clone()]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([(spec.id, json!(true))]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::None,
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }

        let Err(error) = transaction.persist(&FailedRecoveryStore, &current, lock) else {
            panic!("failed visible-state recovery must not claim a durable commit");
        };
        assert!(matches!(
            &error,
            SettingsTransactionError::PublicationStateUncertain {
                source: LocalSettingsPersistError::VisibleRecoveryFailed { .. },
                ..
            }
        ));
        assert!(error.requires_safe_process_restart());
        assert!(error.visible_proposed_envelope().is_none());
        assert_eq!(
            transaction.phase(),
            SettingsTransactionPhase::SafeProcessRestartRequired
        );
        assert!(transaction.rollback_before_persist().is_err());
    }

    #[test]
    fn persisted_entry_uses_the_active_setting_schema_version() {
        let mut spec = bool_setting("versioned");
        spec.schema_version = 7;
        let catalog = catalog(vec![spec.clone()]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([(spec.id.clone(), json!(true))]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::None,
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }
        let store = DeterministicLocalSettingsStore::new();
        let (next, _) = match transaction.persist(&store, &current, lock) {
            Ok(value) => value,
            Err(error) => panic!("persist: {error}"),
        };
        assert_eq!(
            next.user()
                .get(&spec.id)
                .map(StoredSettingEntryV1::schema_version),
            Some(7)
        );
    }

    #[test]
    fn process_restart_impact_stages_a_journal() {
        let mut spec = bool_setting("restart");
        spec.apply_impact = RuntimeApplyImpact::ProcessRestart;
        let catalog = catalog(vec![spec.clone()]);
        let lock = CanonicalHash::digest(b"lock");
        let current = LatticeLocalSettingsV1::empty();
        let before = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("before: {error}"),
        };
        let proposed = BTreeMap::from([(spec.id, json!(true))]);
        let mut transaction = match SettingsApplyTransaction::user_draft(
            &catalog,
            before.snapshot(),
            &current,
            &proposed,
            BindingProfileV1::empty(),
            PreviewPolicyV1::None,
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("draft: {error}"),
        };
        match transaction.prepare() {
            Ok(()) => {}
            Err(error) => panic!("prepare: {error}"),
        }
        let store = DeterministicLocalSettingsStore::new();
        let (next, batch) = match transaction.persist(&store, &current, lock) {
            Ok(value) => value,
            Err(error) => panic!("persist: {error}"),
        };
        assert!(batch.restart_impact().process_restart);
        assert!(next.pending_restart().is_some());
        assert!(next.user().is_empty());
        let confirmed = next.confirm_pending_restart();
        assert!(confirmed.pending_restart().is_none());
        assert_eq!(confirmed.user().len(), 1);
        let rejected = next.reject_pending_restart();
        assert!(rejected.user().is_empty());
    }
}
