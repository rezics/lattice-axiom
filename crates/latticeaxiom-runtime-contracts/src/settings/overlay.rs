//! Fixed-precedence overlays, provenance, and deterministic diffs.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{RuntimeApplyImpact, SettingAuthority, SettingScope};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageName, StableId, canonical_json_bytes,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::catalog::{ValidatedSettingsCatalog, validate_setting_value};

/// Monotonic revision of one setting store.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct StoreRevision(u64);

impl StoreRevision {
    /// Creates a store revision.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric revision.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next revision, saturating at `u64::MAX`.
    #[must_use]
    pub const fn saturating_next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Monotonic revision of an atomic setting transaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SettingTransactionRevision(u64);

impl SettingTransactionRevision {
    /// Creates a transaction revision.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric revision.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next revision, saturating at `u64::MAX`.
    #[must_use]
    pub const fn saturating_next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Authenticated writer that produced a scope overlay.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingWriter {
    /// Local-user action or local settings migration.
    LocalUser,
    /// Authoritative world-owner action.
    WorldOwner,
    /// Authoritative server action.
    Server,
    /// Verified administrator command.
    VerifiedAdmin,
}

/// Values and provenance read atomically from one runtime scope.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeOverlay {
    scope: SettingScope,
    store_revision: StoreRevision,
    transaction_revision: SettingTransactionRevision,
    writer: SettingWriter,
    context_hash: Option<CanonicalHash>,
    values: BTreeMap<StableId, Value>,
}

impl ScopeOverlay {
    /// Creates one scope overlay from an atomic store snapshot.
    #[must_use]
    pub fn new(
        scope: SettingScope,
        store_revision: StoreRevision,
        transaction_revision: SettingTransactionRevision,
        writer: SettingWriter,
        context_hash: Option<CanonicalHash>,
        values: BTreeMap<StableId, Value>,
    ) -> Self {
        Self {
            scope,
            store_revision,
            transaction_revision,
            writer,
            context_hash,
            values,
        }
    }

    /// Returns the persistence scope.
    #[must_use]
    pub const fn scope(&self) -> SettingScope {
        self.scope
    }

    /// Returns the store revision recorded by the adapter.
    #[must_use]
    pub const fn store_revision(&self) -> StoreRevision {
        self.store_revision
    }

    /// Returns the atomic transaction revision recorded by the adapter.
    #[must_use]
    pub const fn transaction_revision(&self) -> SettingTransactionRevision {
        self.transaction_revision
    }

    /// Returns the authenticated writer recorded by the adapter.
    #[must_use]
    pub const fn writer(&self) -> SettingWriter {
        self.writer
    }

    /// Returns the values keyed by stable setting ID.
    #[must_use]
    pub const fn values(&self) -> &BTreeMap<StableId, Value> {
        &self.values
    }
}

/// One legal lower-precedence store hidden by the effective winner.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowedScopeRevision {
    /// Hidden scope.
    pub scope: SettingScope,
    /// Revision of that store snapshot.
    pub store_revision: StoreRevision,
    /// Atomic transaction revision for that snapshot.
    pub transaction_revision: SettingTransactionRevision,
}

/// Source of an effective runtime value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "source")]
pub enum EffectiveSettingSource {
    /// The declaration default won because no legal overlay supplied a value.
    Default,
    /// One runtime store supplied the effective value.
    Scope {
        /// Winning persistence scope.
        scope: SettingScope,
        /// Revision of the winning store.
        store_revision: StoreRevision,
        /// Revision of the winning atomic transaction.
        transaction_revision: SettingTransactionRevision,
        /// Authenticated writer recorded by the adapter.
        writer: SettingWriter,
        /// Privacy-preserving world/player/store context fingerprint.
        context_hash: Option<CanonicalHash>,
    },
}

/// Deterministic explanation attached to an effective setting value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveSettingProvenance {
    /// Stable setting ID.
    pub setting: StableId,
    /// Owner-controlled value schema version.
    pub schema_version: u32,
    /// Winning source.
    pub winner: EffectiveSettingSource,
    /// Lock governing the active shell or world.
    pub active_lock: CanonicalHash,
    /// Legal lower-precedence revisions hidden by the winner.
    pub shadowed: Vec<ShadowedScopeRevision>,
}

/// One effective setting and the declaration data needed for planning.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveSetting {
    /// Stable setting ID.
    pub id: StableId,
    /// Package owning callbacks and migration.
    pub declared_by: PackageName,
    /// Effective typed JSON value.
    pub value: Value,
    /// Runtime impact of a committed change.
    pub apply_impact: RuntimeApplyImpact,
    /// Effective-value explanation.
    pub provenance: EffectiveSettingProvenance,
}

/// Immutable effective settings resolved from validated scope overlays.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveSettingsSnapshot {
    active_lock: CanonicalHash,
    values: BTreeMap<StableId, EffectiveSetting>,
}

impl EffectiveSettingsSnapshot {
    /// Returns the active lock fingerprint used for provenance.
    #[must_use]
    pub const fn active_lock(&self) -> CanonicalHash {
        self.active_lock
    }

    /// Returns effective values in stable setting-ID order.
    #[must_use]
    pub const fn values(&self) -> &BTreeMap<StableId, EffectiveSetting> {
        &self.values
    }

    /// Produces a stable diff against `next`.
    #[must_use]
    pub fn diff(&self, next: &Self) -> SettingsDiff {
        let ids = self
            .values
            .keys()
            .chain(next.values.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let entries = ids
            .into_iter()
            .filter_map(|id| {
                let before = self.values.get(&id).cloned();
                let after = next.values.get(&id).cloned();
                (before != after).then_some(SettingDiffEntry { id, before, after })
            })
            .collect();
        SettingsDiff { entries }
    }

    /// Encodes the snapshot in deterministic compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }
}

/// Stored setting retained after its owning declaration leaves the active catalog.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrphanedStoredSetting {
    /// Stable setting ID retained verbatim.
    pub id: StableId,
    /// Store scope retaining the value.
    pub scope: SettingScope,
    /// Store revision containing the orphan.
    pub store_revision: StoreRevision,
    /// Transaction revision containing the orphan.
    pub transaction_revision: SettingTransactionRevision,
    /// Last authenticated writer known to the store adapter.
    pub writer: SettingWriter,
    /// Privacy-preserving store context fingerprint.
    pub context_hash: Option<CanonicalHash>,
    /// Opaque value retained for a future compatible owner migration.
    pub value: Value,
}

/// Effective active values plus inactive orphan preservation evidence.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveSettingsResolution {
    snapshot: EffectiveSettingsSnapshot,
    orphans: Vec<OrphanedStoredSetting>,
}

impl EffectiveSettingsResolution {
    /// Returns active effective values; orphan bytes are deliberately excluded.
    #[must_use]
    pub const fn snapshot(&self) -> &EffectiveSettingsSnapshot {
        &self.snapshot
    }

    /// Returns inactive orphan values in scope then stable-ID order.
    #[must_use]
    pub fn orphans(&self) -> &[OrphanedStoredSetting] {
        &self.orphans
    }

    /// Consumes the resolution into active and orphan parts.
    #[must_use]
    pub fn into_parts(self) -> (EffectiveSettingsSnapshot, Vec<OrphanedStoredSetting>) {
        (self.snapshot, self.orphans)
    }
}

/// Resolves declaration defaults and zero-or-one overlay per runtime scope.
///
/// Scope precedence is fixed by authority; input ordering and timestamps have
/// no effect. The function validates every stored value, including a value
/// later shadowed by a higher-precedence scope.
/// Unknown stored IDs are retained as inactive orphans and never enter the
/// effective snapshot or its canonical hash.
///
/// # Errors
///
/// Returns [`SettingsOverlayError`] for duplicate scopes, illegal scopes/writers,
/// or invalid typed stored values.
#[allow(
    clippy::too_many_lines,
    reason = "overlay validation and fixed precedence stay linear for policy auditability"
)]
pub fn resolve_effective_settings(
    catalog: &ValidatedSettingsCatalog,
    overlays: impl IntoIterator<Item = ScopeOverlay>,
    active_lock: CanonicalHash,
) -> Result<EffectiveSettingsResolution, SettingsOverlayError> {
    let mut by_scope = BTreeMap::new();
    for overlay in overlays {
        let scope = overlay.scope;
        if by_scope.insert(scope, overlay).is_some() {
            return Err(SettingsOverlayError::DuplicateScope { scope });
        }
    }

    let mut orphans = Vec::new();
    for overlay in by_scope.values() {
        for (setting_id, value) in &overlay.values {
            let Some(setting) = catalog.as_catalog().runtime.get(setting_id) else {
                orphans.push(OrphanedStoredSetting {
                    id: setting_id.clone(),
                    scope: overlay.scope,
                    store_revision: overlay.store_revision,
                    transaction_revision: overlay.transaction_revision,
                    writer: overlay.writer,
                    context_hash: overlay.context_hash,
                    value: value.clone(),
                });
                continue;
            };
            if !setting.allowed_scopes.contains(&overlay.scope) {
                return Err(SettingsOverlayError::ScopeNotAllowed {
                    setting: setting_id.clone(),
                    scope: overlay.scope,
                });
            }
            if !writer_matches(setting.authority, overlay.writer) {
                return Err(SettingsOverlayError::WriterNotAuthorized {
                    setting: setting_id.clone(),
                    authority: setting.authority,
                    writer: overlay.writer,
                });
            }
            validate_setting_value(setting, value).map_err(|source| {
                SettingsOverlayError::InvalidStoredValue {
                    setting: setting_id.clone(),
                    scope: overlay.scope,
                    reason: source.to_string(),
                }
            })?;
        }
    }

    let mut values = BTreeMap::new();
    for (setting_id, setting) in &catalog.as_catalog().runtime {
        let mut effective = setting.default.clone();
        let mut winner = EffectiveSettingSource::Default;
        let mut shadowed = Vec::new();
        for scope in precedence(setting.authority) {
            if let Some(overlay) = by_scope.get(scope)
                && let Some(value) = overlay.values.get(setting_id)
            {
                if !matches!(winner, EffectiveSettingSource::Default)
                    && let EffectiveSettingSource::Scope {
                        scope,
                        store_revision,
                        transaction_revision,
                        ..
                    } = winner
                {
                    shadowed.push(ShadowedScopeRevision {
                        scope,
                        store_revision,
                        transaction_revision,
                    });
                }
                effective = value.clone();
                winner = EffectiveSettingSource::Scope {
                    scope: *scope,
                    store_revision: overlay.store_revision,
                    transaction_revision: overlay.transaction_revision,
                    writer: overlay.writer,
                    context_hash: overlay.context_hash,
                };
            }
        }
        values.insert(
            setting_id.clone(),
            EffectiveSetting {
                id: setting_id.clone(),
                declared_by: setting.declared_by.clone(),
                value: effective,
                apply_impact: setting.apply_impact,
                provenance: EffectiveSettingProvenance {
                    setting: setting_id.clone(),
                    schema_version: setting.schema_version,
                    winner,
                    active_lock,
                    shadowed,
                },
            },
        );
    }
    Ok(EffectiveSettingsResolution {
        snapshot: EffectiveSettingsSnapshot {
            active_lock,
            values,
        },
        orphans,
    })
}

fn writer_matches(authority: SettingAuthority, writer: SettingWriter) -> bool {
    matches!(
        (authority, writer),
        (SettingAuthority::LocalUser, SettingWriter::LocalUser)
            | (SettingAuthority::WorldOwner, SettingWriter::WorldOwner)
            | (SettingAuthority::Server, SettingWriter::Server)
            | (SettingAuthority::AdminOnly, SettingWriter::VerifiedAdmin)
    )
}

fn precedence(authority: SettingAuthority) -> &'static [SettingScope] {
    const LOCAL: &[SettingScope] = &[
        SettingScope::Device,
        SettingScope::User,
        SettingScope::PlayerWorld,
        SettingScope::Session,
    ];
    const WORLD: &[SettingScope] = &[SettingScope::World, SettingScope::PlayerWorld];
    match authority {
        SettingAuthority::LocalUser => LOCAL,
        SettingAuthority::WorldOwner | SettingAuthority::Server | SettingAuthority::AdminOnly => {
            WORLD
        }
        SettingAuthority::FixedByProfile => &[],
    }
}

/// A stored scope overlay was invalid for the compiled catalog.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SettingsOverlayError {
    /// More than one snapshot was supplied for a scope.
    #[error("more than one overlay was supplied for scope `{scope:?}`")]
    DuplicateScope {
        /// Duplicated scope.
        scope: SettingScope,
    },

    /// A store used a scope outside the declaration.
    #[error("setting `{setting}` does not allow scope `{scope:?}`")]
    ScopeNotAllowed {
        /// Stable setting ID.
        setting: StableId,
        /// Rejected scope.
        scope: SettingScope,
    },
    /// The authenticated writer did not match the authority lane.
    #[error(
        "writer `{writer:?}` is not authorized for setting `{setting}` authority `{authority:?}`"
    )]
    WriterNotAuthorized {
        /// Stable setting ID.
        setting: StableId,
        /// Required authority.
        authority: SettingAuthority,
        /// Rejected writer.
        writer: SettingWriter,
    },
    /// A stored value did not satisfy the active declaration.
    #[error("setting `{setting}` in scope `{scope:?}` is invalid: {reason}")]
    InvalidStoredValue {
        /// Stable setting ID.
        setting: StableId,
        /// Store containing it.
        scope: SettingScope,
        /// Stable validation explanation.
        reason: String,
    },
}

/// One deterministic effective-value or provenance change.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingDiffEntry {
    /// Stable setting ID.
    pub id: StableId,
    /// Previous effective row, if present.
    pub before: Option<EffectiveSetting>,
    /// Proposed effective row, if present.
    pub after: Option<EffectiveSetting>,
}

/// Stable-ID-ordered effective settings diff.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsDiff {
    entries: Vec<SettingDiffEntry>,
}

impl SettingsDiff {
    /// Returns changes in stable setting-ID order.
    #[must_use]
    pub fn entries(&self) -> &[SettingDiffEntry] {
        &self.entries
    }

    /// Returns whether the diff has no value or provenance changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the strongest runtime impact in this diff.
    #[must_use]
    pub fn required_impact(&self) -> Option<RuntimeApplyImpact> {
        self.entries
            .iter()
            .filter_map(|entry| {
                entry
                    .after
                    .as_ref()
                    .or(entry.before.as_ref())
                    .map(|setting| setting.apply_impact)
            })
            .max_by_key(|impact| impact_rank(*impact))
    }

    /// Reports restart and reactivation impact without applying values.
    #[must_use]
    pub fn restart_impact(&self) -> RestartImpactMetadata {
        let strongest = self.required_impact();
        RestartImpactMetadata {
            strongest_impact: strongest,
            process_restart: matches!(strongest, Some(RuntimeApplyImpact::ProcessRestart)),
            world_reactivate: matches!(
                strongest,
                Some(RuntimeApplyImpact::WorldReactivate | RuntimeApplyImpact::ProcessRestart)
            ),
            live_immediate: matches!(strongest, Some(RuntimeApplyImpact::Immediate)),
        }
    }

    /// Builds reverse stable owner/setting order for undoing prepared values.
    ///
    /// This plan neither grants preview support nor proves reversibility; that
    /// requires an independent accepted preview policy. It only orders undo for
    /// values the host actually prepared, and must never retract a durable commit.
    #[must_use]
    pub fn pre_persist_rollback_plan(&self) -> SettingsRollbackPlan {
        let mut steps = self
            .entries
            .iter()
            .filter_map(|entry| {
                let representative = entry.after.as_ref().or(entry.before.as_ref())?;
                Some(SettingsRollbackStep {
                    id: entry.id.clone(),
                    owner: representative.declared_by.clone(),
                    expected_applied: entry.after.as_ref().map(|row| row.value.clone()),
                    restore: entry.before.as_ref().map(|row| row.value.clone()),
                })
            })
            .collect::<Vec<_>>();
        steps.sort_by(|left, right| {
            left.owner
                .cmp(&right.owner)
                .then_with(|| left.id.cmp(&right.id))
        });
        steps.reverse();
        SettingsRollbackPlan { steps }
    }

    /// Encodes the diff in deterministic compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }
}

const fn impact_rank(impact: RuntimeApplyImpact) -> u8 {
    match impact {
        RuntimeApplyImpact::Preview => 0,
        RuntimeApplyImpact::Immediate => 1,
        RuntimeApplyImpact::WorldReactivate => 2,
        RuntimeApplyImpact::ProcessRestart => 3,
    }
}

/// Restart and world-reactivation metadata derived from one change set.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RestartImpactMetadata {
    /// Strongest runtime impact present in the change set.
    pub strongest_impact: Option<RuntimeApplyImpact>,
    /// The change set requires a process restart after persistence.
    pub process_restart: bool,
    /// The change set requires world reactivation or a stricter restart.
    pub world_reactivate: bool,
    /// The change set can commit inside the live UI or fixed-tick barrier.
    pub live_immediate: bool,
}

/// One pre-persist prepared-value restoration operation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsRollbackStep {
    /// Stable setting ID.
    pub id: StableId,
    /// Package participant owner.
    pub owner: PackageName,
    /// Value expected after preparation, or absence for a newly prepared row.
    pub expected_applied: Option<Value>,
    /// Value to restore, or absence when the previous catalog had no row.
    pub restore: Option<Value>,
}

/// Reverse stable-order plan for cancelling prepared setting values.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsRollbackPlan {
    steps: Vec<SettingsRollbackStep>,
}

impl SettingsRollbackPlan {
    /// Returns rollback steps in execution order.
    #[must_use]
    pub fn steps(&self) -> &[SettingsRollbackStep] {
        &self.steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_compose::{SettingAuthority, SettingSensitivity, SettingSpec, ValueType};
    use serde_json::json;

    use crate::settings::catalog::{SettingsCatalogFragment, SettingsCatalogPolicy};

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

    fn setting(owner: &str, path: &str, value_type: ValueType, default: Value) -> SettingSpec {
        SettingSpec {
            id: id(&format!("example:setting/{path}")),
            declared_by: package(owner),
            schema_version: 1,
            value_type,
            default,
            allowed_scopes: BTreeSet::from([SettingScope::User, SettingScope::Session]),
            default_scope: SettingScope::User,
            authority: SettingAuthority::LocalUser,
            apply_impact: RuntimeApplyImpact::Immediate,
            category: id("example:setting-category/general"),
            order: 0,
            label_key: "setting.label".to_owned(),
            description_key: "setting.description".to_owned(),
            visibility: None,
            enabled_when: None,
            sensitivity: SettingSensitivity::Ordinary,
            replacement: None,
        }
    }

    fn compile(settings: Vec<SettingSpec>) -> ValidatedSettingsCatalog {
        match ValidatedSettingsCatalog::compile(
            [SettingsCatalogFragment::new(
                package("@example/runtime"),
                settings,
            )],
            SettingsCatalogPolicy::default(),
        ) {
            Ok(value) => value,
            Err(error) => panic!("valid catalog fixture failed: {error}"),
        }
    }

    #[test]
    fn overlays_use_fixed_precedence_and_reject_wrong_writer() {
        let spec = setting(
            "@example/runtime",
            "scale",
            ValueType::Integer {
                min: Some(1),
                max: Some(4),
                step: Some(1),
            },
            json!(1),
        );
        let catalog = compile(vec![spec.clone()]);
        let user = ScopeOverlay::new(
            SettingScope::User,
            StoreRevision::new(3),
            SettingTransactionRevision::new(4),
            SettingWriter::LocalUser,
            None,
            BTreeMap::from([(spec.id.clone(), json!(2))]),
        );
        let session = ScopeOverlay::new(
            SettingScope::Session,
            StoreRevision::new(8),
            SettingTransactionRevision::new(9),
            SettingWriter::LocalUser,
            None,
            BTreeMap::from([(spec.id.clone(), json!(3))]),
        );
        let snapshot = resolve_effective_settings(
            &catalog,
            [session.clone(), user],
            CanonicalHash::digest(b"lock"),
        );
        let row = snapshot
            .ok()
            .and_then(|resolution| resolution.snapshot().values().get(&spec.id).cloned());
        assert_eq!(row.as_ref().map(|row| &row.value), Some(&json!(3)));
        assert_eq!(row.map(|row| row.provenance.shadowed.len()), Some(1));

        let wrong_writer = ScopeOverlay::new(
            SettingScope::Session,
            StoreRevision::new(8),
            SettingTransactionRevision::new(9),
            SettingWriter::Server,
            None,
            BTreeMap::from([(spec.id, json!(3))]),
        );
        assert!(matches!(
            resolve_effective_settings(&catalog, [wrong_writer], CanonicalHash::digest(b"lock")),
            Err(SettingsOverlayError::WriterNotAuthorized { .. })
        ));
    }

    #[test]
    fn diff_and_pre_persist_rollback_have_stable_reverse_owner_order() {
        let alpha = setting("@example/runtime", "alpha", ValueType::Bool, json!(false));
        let beta = setting("@example/runtime", "beta", ValueType::Bool, json!(false));
        let catalog = compile(vec![beta.clone(), alpha.clone()]);
        let lock = CanonicalHash::digest(b"lock");
        let before = resolve_effective_settings(&catalog, [], lock);
        let after = resolve_effective_settings(
            &catalog,
            [ScopeOverlay::new(
                SettingScope::Session,
                StoreRevision::new(1),
                SettingTransactionRevision::new(1),
                SettingWriter::LocalUser,
                None,
                BTreeMap::from([
                    (alpha.id.clone(), json!(true)),
                    (beta.id.clone(), json!(true)),
                ]),
            )],
            lock,
        );
        let diff = match (before, after) {
            (Ok(before), Ok(after)) => before.snapshot().diff(after.snapshot()),
            (Err(error), _) | (_, Err(error)) => panic!("overlay fixture failed: {error}"),
        };
        assert_eq!(
            diff.entries().iter().map(|row| &row.id).collect::<Vec<_>>(),
            vec![&alpha.id, &beta.id]
        );
        assert_eq!(
            diff.pre_persist_rollback_plan()
                .steps()
                .iter()
                .map(|row| &row.id)
                .collect::<Vec<_>>(),
            vec![&beta.id, &alpha.id]
        );
        assert_eq!(
            diff.restart_impact(),
            RestartImpactMetadata {
                strongest_impact: Some(RuntimeApplyImpact::Immediate),
                process_restart: false,
                world_reactivate: false,
                live_immediate: true,
            }
        );
    }

    #[test]
    fn unknown_stored_values_are_preserved_but_excluded_from_effective_bytes() {
        let spec = setting("@example/runtime", "known", ValueType::Bool, json!(false));
        let catalog = compile(vec![spec]);
        let lock = CanonicalHash::digest(b"lock");
        let baseline = match resolve_effective_settings(&catalog, [], lock) {
            Ok(value) => value,
            Err(error) => panic!("baseline resolution failed: {error}"),
        };
        let orphan_id = id("example:setting/removed-owner");
        let with_orphan = match resolve_effective_settings(
            &catalog,
            [ScopeOverlay::new(
                SettingScope::User,
                StoreRevision::new(7),
                SettingTransactionRevision::new(9),
                SettingWriter::LocalUser,
                None,
                BTreeMap::from([(orphan_id.clone(), json!({"retained": true}))]),
            )],
            lock,
        ) {
            Ok(value) => value,
            Err(error) => panic!("orphan resolution failed: {error}"),
        };
        assert_eq!(with_orphan.orphans().len(), 1);
        assert_eq!(with_orphan.orphans()[0].id, orphan_id);
        assert_eq!(
            baseline.snapshot().canonical_bytes().ok(),
            with_orphan.snapshot().canonical_bytes().ok()
        );
    }

    #[test]
    fn scope_overlay_denies_unknown_fields() {
        let json = r#"{
            "scope":"user",
            "store_revision":1,
            "transaction_revision":1,
            "writer":"local-user",
            "context_hash":null,
            "values":{},
            "surprise":true
        }"#;
        assert!(serde_json::from_str::<ScopeOverlay>(json).is_err());
    }
}
