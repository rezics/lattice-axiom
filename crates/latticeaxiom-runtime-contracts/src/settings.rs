//! Validated runtime-setting catalogs, scope overlays, and deterministic plans.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    RuntimeApplyImpact, SettingAuthority, SettingPredicate, SettingScope, SettingSpec,
    SettingsCatalog, ValueType, ValueValidationError,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageName, StableId, canonical_json_bytes,
    canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use unicode_normalization::is_nfc;

/// Default maximum number of runtime settings accepted by one catalog compile.
///
/// This is a defensive implementation limit, not a persistence or ABI constant.
pub const DEFAULT_MAX_RUNTIME_SETTINGS: usize = 4_096;

/// Accepted-ADR limit for one declarative setting predicate.
pub const MAX_SETTING_PREDICATE_DEPTH: usize = 8;

/// Accepted-ADR limit for nodes in one declarative setting predicate.
pub const MAX_SETTING_PREDICATE_NODES: usize = 32;

/// Bounded host policy for compiling runtime-setting declarations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsCatalogPolicy {
    max_settings: usize,
}

impl SettingsCatalogPolicy {
    /// Creates a catalog policy with an explicit setting-count limit.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsCatalogPolicyError`] when `max_settings` is zero.
    pub const fn new(max_settings: usize) -> Result<Self, SettingsCatalogPolicyError> {
        if max_settings == 0 {
            Err(SettingsCatalogPolicyError::ZeroSettingLimit)
        } else {
            Ok(Self { max_settings })
        }
    }

    /// Returns the maximum declarations accepted by one compile.
    #[must_use]
    pub const fn max_settings(self) -> usize {
        self.max_settings
    }
}

impl Default for SettingsCatalogPolicy {
    fn default() -> Self {
        Self {
            max_settings: DEFAULT_MAX_RUNTIME_SETTINGS,
        }
    }
}

/// An invalid runtime-setting catalog policy.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SettingsCatalogPolicyError {
    /// A zero limit could never accept a useful catalog.
    #[error("the runtime-setting limit must be positive")]
    ZeroSettingLimit,
}

/// Settings declared by one package before closure-wide compilation.
#[derive(Clone, Debug)]
pub struct SettingsCatalogFragment {
    owner: PackageName,
    settings: Vec<SettingSpec>,
}

impl SettingsCatalogFragment {
    /// Creates a package-owned runtime-setting fragment.
    #[must_use]
    pub fn new(owner: PackageName, settings: Vec<SettingSpec>) -> Self {
        Self { owner, settings }
    }

    /// Returns the package expected to own every declaration.
    #[must_use]
    pub const fn owner(&self) -> &PackageName {
        &self.owner
    }

    /// Returns this fragment's declarations.
    #[must_use]
    pub fn settings(&self) -> &[SettingSpec] {
        &self.settings
    }
}

/// A runtime-setting catalog whose expressible V1 invariants were checked.
///
/// The accepted ADR is stricter than the current compose DTO. Compilation
/// therefore rejects legacy `Preview`, `FixedByProfile`, and `KeyBinding`
/// declarations instead of silently inventing semantics for missing
/// `PreviewPolicyV1`, fixed lock values, or `InputBindingV1` fields.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct ValidatedSettingsCatalog {
    catalog: SettingsCatalog,
}

impl ValidatedSettingsCatalog {
    /// Compiles package fragments into stable-ID order and validates them.
    ///
    /// Discovery order cannot affect successful bytes or collision ownership.
    /// Composition parameters are deliberately outside this runtime compiler.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsCatalogError`] for an exceeded limit, ownership or ID
    /// collision, unsupported legacy representation, invalid schema/default,
    /// invalid scope lane, invalid predicate, or predicate cycle.
    pub fn compile<I>(
        fragments: I,
        policy: SettingsCatalogPolicy,
    ) -> Result<Self, SettingsCatalogError>
    where
        I: IntoIterator<Item = SettingsCatalogFragment>,
    {
        let mut rows = Vec::new();
        for fragment in fragments {
            for setting in fragment.settings {
                rows.push((fragment.owner.clone(), setting));
                if rows.len() > policy.max_settings {
                    return Err(SettingsCatalogError::CatalogLimitExceeded {
                        observed: rows.len(),
                        maximum: policy.max_settings,
                    });
                }
            }
        }
        rows.sort_by(|(left_owner, left), (right_owner, right)| {
            left.id
                .cmp(&right.id)
                .then_with(|| left_owner.cmp(right_owner))
                .then_with(|| left.declared_by.cmp(&right.declared_by))
        });

        let mut runtime = BTreeMap::new();
        for (fragment_owner, setting) in rows {
            if fragment_owner != setting.declared_by {
                return Err(SettingsCatalogError::FragmentOwnerMismatch {
                    setting: setting.id,
                    fragment_owner,
                    declared_by: setting.declared_by,
                });
            }
            if let Some(first) = runtime.insert(setting.id.clone(), setting.clone()) {
                return Err(SettingsCatalogError::SettingCollision {
                    setting: setting.id,
                    first_owner: first.declared_by,
                    second_owner: setting.declared_by,
                });
            }
        }

        for setting in runtime.values() {
            validate_setting(setting)?;
        }
        validate_predicates(&runtime)?;

        Ok(Self {
            catalog: SettingsCatalog {
                runtime,
                composition: BTreeMap::new(),
            },
        })
    }

    /// Returns the compose-owned canonical declaration DTO.
    #[must_use]
    pub const fn as_catalog(&self) -> &SettingsCatalog {
        &self.catalog
    }

    /// Consumes the validated wrapper and returns the compose-owned DTO.
    #[must_use]
    pub fn into_catalog(self) -> SettingsCatalog {
        self.catalog
    }

    /// Encodes the catalog as recursively key-sorted compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(&self.catalog)
    }

    /// Hashes the canonical catalog bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(&self.catalog)
    }
}

fn validate_setting(setting: &SettingSpec) -> Result<(), SettingsCatalogError> {
    if setting.id.kind() != "setting" {
        return Err(SettingsCatalogError::WrongSettingKind {
            setting: setting.id.clone(),
            actual: setting.id.kind().to_owned(),
        });
    }
    if setting.schema_version == 0 {
        return Err(SettingsCatalogError::ZeroSchemaVersion {
            setting: setting.id.clone(),
        });
    }
    if setting.label_key.is_empty() || setting.description_key.is_empty() {
        return Err(SettingsCatalogError::EmptyLocalizationKey {
            setting: setting.id.clone(),
        });
    }
    if !is_nfc(&setting.label_key) || !is_nfc(&setting.description_key) {
        return Err(SettingsCatalogError::NonCanonicalUnicode {
            setting: setting.id.clone(),
        });
    }
    if setting.replacement.as_ref() == Some(&setting.id) {
        return Err(SettingsCatalogError::SelfReplacement {
            setting: setting.id.clone(),
        });
    }
    if let Some(replacement) = &setting.replacement
        && replacement.kind() != "setting"
    {
        return Err(SettingsCatalogError::WrongReplacementKind {
            setting: setting.id.clone(),
            replacement: Box::new(replacement.clone()),
        });
    }

    match setting.apply_impact {
        RuntimeApplyImpact::Preview => {
            return Err(SettingsCatalogError::LegacyPreviewImpact {
                setting: setting.id.clone(),
            });
        }
        RuntimeApplyImpact::Immediate
        | RuntimeApplyImpact::WorldReactivate
        | RuntimeApplyImpact::ProcessRestart => {}
    }
    if setting.authority == SettingAuthority::FixedByProfile {
        return Err(SettingsCatalogError::UnrepresentableFixedByProfile {
            setting: setting.id.clone(),
        });
    }
    validate_scope_lane(setting)?;
    validate_value_schema(setting)?;
    validate_setting_value(setting, &setting.default)
}

fn validate_scope_lane(setting: &SettingSpec) -> Result<(), SettingsCatalogError> {
    if setting.allowed_scopes.is_empty() {
        return Err(SettingsCatalogError::EmptyAllowedScopes {
            setting: setting.id.clone(),
        });
    }
    if !setting.allowed_scopes.contains(&setting.default_scope) {
        return Err(SettingsCatalogError::DefaultScopeNotAllowed {
            setting: setting.id.clone(),
            scope: setting.default_scope,
        });
    }
    for scope in &setting.allowed_scopes {
        let allowed = match setting.authority {
            SettingAuthority::LocalUser => matches!(
                scope,
                SettingScope::Device
                    | SettingScope::User
                    | SettingScope::PlayerWorld
                    | SettingScope::Session
            ),
            SettingAuthority::WorldOwner
            | SettingAuthority::Server
            | SettingAuthority::AdminOnly => {
                matches!(scope, SettingScope::World | SettingScope::PlayerWorld)
            }
            SettingAuthority::FixedByProfile => false,
        };
        if !allowed {
            return Err(SettingsCatalogError::ScopeAuthorityMismatch {
                setting: setting.id.clone(),
                authority: setting.authority,
                scope: *scope,
            });
        }
    }
    Ok(())
}

fn validate_value_schema(setting: &SettingSpec) -> Result<(), SettingsCatalogError> {
    match &setting.value_type {
        ValueType::String { max_length, .. } => {
            if max_length.is_none() {
                return Err(SettingsCatalogError::UnboundedString {
                    setting: setting.id.clone(),
                });
            }
        }
        ValueType::Enum { values } => {
            if values
                .iter()
                .any(|value| value.is_empty() || !is_nfc(value))
            {
                return Err(SettingsCatalogError::InvalidEnumVariant {
                    setting: setting.id.clone(),
                });
            }
        }
        ValueType::KeyBinding => {
            return Err(SettingsCatalogError::UnrepresentableKeyBinding {
                setting: setting.id.clone(),
            });
        }
        ValueType::Bool
        | ValueType::Integer { .. }
        | ValueType::Number { .. }
        | ValueType::Color => {}
    }
    Ok(())
}

fn validate_setting_value(
    setting: &SettingSpec,
    value: &Value,
) -> Result<(), SettingsCatalogError> {
    setting.value_type.validate_value(value).map_err(|source| {
        SettingsCatalogError::InvalidTypedValue {
            setting: setting.id.clone(),
            source,
        }
    })?;
    if matches!(setting.value_type, ValueType::String { .. })
        && value.as_str().is_some_and(|text| !is_nfc(text))
    {
        return Err(SettingsCatalogError::NonCanonicalUnicode {
            setting: setting.id.clone(),
        });
    }
    Ok(())
}

fn validate_predicates(
    catalog: &BTreeMap<StableId, SettingSpec>,
) -> Result<(), SettingsCatalogError> {
    let mut dependencies = BTreeMap::new();
    for (setting_id, setting) in catalog {
        let mut references = BTreeSet::new();
        if let Some(predicate) = &setting.visibility {
            validate_predicate(setting_id, predicate, catalog, &mut references)?;
        }
        if let Some(predicate) = &setting.enabled_when {
            validate_predicate(setting_id, predicate, catalog, &mut references)?;
        }
        dependencies.insert(setting_id.clone(), references);
    }
    reject_predicate_cycles(&dependencies)
}

fn validate_predicate(
    owner: &StableId,
    predicate: &SettingPredicate,
    catalog: &BTreeMap<StableId, SettingSpec>,
    references: &mut BTreeSet<StableId>,
) -> Result<(), SettingsCatalogError> {
    let mut nodes = 0_usize;
    validate_predicate_node(owner, predicate, catalog, references, 1, &mut nodes)
}

fn validate_predicate_node(
    owner: &StableId,
    predicate: &SettingPredicate,
    catalog: &BTreeMap<StableId, SettingSpec>,
    references: &mut BTreeSet<StableId>,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), SettingsCatalogError> {
    if depth > MAX_SETTING_PREDICATE_DEPTH {
        return Err(SettingsCatalogError::PredicateDepthExceeded {
            setting: owner.clone(),
            observed: depth,
            maximum: MAX_SETTING_PREDICATE_DEPTH,
        });
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_SETTING_PREDICATE_NODES {
        return Err(SettingsCatalogError::PredicateNodeLimitExceeded {
            setting: owner.clone(),
            observed: *nodes,
            maximum: MAX_SETTING_PREDICATE_NODES,
        });
    }
    match predicate {
        SettingPredicate::Equals { setting, value } => {
            let target_spec = catalog.get(setting).ok_or_else(|| {
                SettingsCatalogError::UnknownPredicateReference {
                    setting: owner.clone(),
                    referenced: Box::new(setting.clone()),
                }
            })?;
            validate_setting_value(target_spec, value).map_err(|_| {
                SettingsCatalogError::PredicateTypeMismatch {
                    setting: owner.clone(),
                    referenced: Box::new(setting.clone()),
                }
            })?;
            references.insert(setting.clone());
        }
        SettingPredicate::All { predicates } | SettingPredicate::Any { predicates } => {
            if predicates.is_empty() {
                return Err(SettingsCatalogError::EmptyPredicateGroup {
                    setting: owner.clone(),
                });
            }
            for child in predicates {
                validate_predicate_node(
                    owner,
                    child,
                    catalog,
                    references,
                    depth.saturating_add(1),
                    nodes,
                )?;
            }
        }
        SettingPredicate::Not { predicate } => validate_predicate_node(
            owner,
            predicate,
            catalog,
            references,
            depth.saturating_add(1),
            nodes,
        )?,
    }
    Ok(())
}

fn reject_predicate_cycles(
    dependencies: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Result<(), SettingsCatalogError> {
    let mut remaining = dependencies
        .iter()
        .map(|(setting, references)| (setting.clone(), references.len()))
        .collect::<BTreeMap<_, _>>();
    let mut dependants = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for (setting, references) in dependencies {
        for referenced in references {
            dependants
                .entry(referenced.clone())
                .or_default()
                .insert(setting.clone());
        }
    }
    let mut ready = remaining
        .iter()
        .filter_map(|(setting, count)| (*count == 0).then_some(setting.clone()))
        .collect::<BTreeSet<_>>();
    let mut processed = 0_usize;
    while let Some(setting) = ready.pop_first() {
        processed = processed.saturating_add(1);
        if let Some(rows) = dependants.get(&setting) {
            for dependant in rows {
                if let Some(count) = remaining.get_mut(dependant) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.insert(dependant.clone());
                    }
                }
            }
        }
    }
    if processed != remaining.len() {
        let setting = remaining
            .into_iter()
            .find_map(|(setting, count)| (count != 0).then_some(setting))
            .ok_or(SettingsCatalogError::PredicateGraphAccounting)?;
        return Err(SettingsCatalogError::PredicateCycle { setting });
    }
    Ok(())
}

/// A runtime-setting catalog failed closed validation.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SettingsCatalogError {
    /// More settings were supplied than the host policy permits.
    #[error("runtime-setting catalog has {observed} declarations; maximum is {maximum}")]
    CatalogLimitExceeded {
        /// Supplied declaration count.
        observed: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// A fragment claimed a setting whose declaration names another owner.
    #[error(
        "setting `{setting}` fragment owner `{fragment_owner}` does not match declared owner `{declared_by}`"
    )]
    FragmentOwnerMismatch {
        /// Conflicting setting ID.
        setting: StableId,
        /// Owner of the containing fragment.
        fragment_owner: PackageName,
        /// Owner embedded in the declaration.
        declared_by: PackageName,
    },
    /// Two declarations used the same exact setting ID.
    #[error("setting `{setting}` is declared by both `{first_owner}` and `{second_owner}`")]
    SettingCollision {
        /// Colliding stable ID.
        setting: StableId,
        /// Stable-first owner.
        first_owner: PackageName,
        /// Stable-second owner.
        second_owner: PackageName,
    },
    /// A catalog row did not use the `setting` stable-ID kind.
    #[error("runtime setting `{setting}` uses kind `{actual}` instead of `setting`")]
    WrongSettingKind {
        /// Invalid declaration ID.
        setting: StableId,
        /// Observed kind.
        actual: String,
    },
    /// Schema version zero is not a versioned contract.
    #[error("setting `{setting}` has schema version zero")]
    ZeroSchemaVersion {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// A localization key was absent.
    #[error("setting `{setting}` has an empty localization key")]
    EmptyLocalizationKey {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// A string affecting deterministic bytes was not NFC.
    #[error("setting `{setting}` contains text that is not Unicode NFC")]
    NonCanonicalUnicode {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// A deprecated declaration pointed to itself.
    #[error("setting `{setting}` cannot replace itself")]
    SelfReplacement {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// A replacement did not identify another setting.
    #[error("setting `{setting}` replacement `{replacement}` is not a setting ID")]
    WrongReplacementKind {
        /// Invalid declaration ID.
        setting: StableId,
        /// Invalid replacement ID.
        replacement: Box<StableId>,
    },
    /// The legacy DTO still encoded preview as an apply impact.
    #[error(
        "setting `{setting}` uses legacy preview impact; accepted ADR 0025 requires PreviewPolicyV1"
    )]
    LegacyPreviewImpact {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// The current DTO cannot identify a fixed lock value without a fake scope.
    #[error(
        "setting `{setting}` uses FixedByProfile, which the current mandatory scope fields cannot represent faithfully"
    )]
    UnrepresentableFixedByProfile {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// The current DTO represents key bindings as untyped strings.
    #[error(
        "setting `{setting}` uses KeyBinding before a versioned InputBindingV1 DTO is available"
    )]
    UnrepresentableKeyBinding {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// A mutable setting had no persistence scope.
    #[error("setting `{setting}` has no allowed runtime scopes")]
    EmptyAllowedScopes {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// The default scope was not one of the allowed scopes.
    #[error("setting `{setting}` default scope `{scope:?}` is not allowed")]
    DefaultScopeNotAllowed {
        /// Invalid declaration ID.
        setting: StableId,
        /// Rejected default scope.
        scope: SettingScope,
    },
    /// A scope was incompatible with its authority lane.
    #[error("setting `{setting}` authority `{authority:?}` cannot use scope `{scope:?}`")]
    ScopeAuthorityMismatch {
        /// Invalid declaration ID.
        setting: StableId,
        /// Declared authority.
        authority: SettingAuthority,
        /// Rejected scope.
        scope: SettingScope,
    },
    /// V1 strings require an explicit scalar-count maximum.
    #[error("setting `{setting}` string schema has no maximum scalar count")]
    UnboundedString {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// An enum variant was empty or non-NFC.
    #[error("setting `{setting}` contains an invalid stable enum variant")]
    InvalidEnumVariant {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// A schema or value failed the compose-owned finite-type validator.
    #[error("setting `{setting}` has an invalid schema or typed value: {source}")]
    InvalidTypedValue {
        /// Invalid declaration ID.
        setting: StableId,
        /// Canonical finite-value error.
        #[source]
        source: ValueValidationError,
    },
    /// A condition referenced a setting absent from the compiled catalog.
    #[error("setting `{setting}` predicate references unknown setting `{referenced}`")]
    UnknownPredicateReference {
        /// Setting containing the predicate.
        setting: StableId,
        /// Missing setting ID.
        referenced: Box<StableId>,
    },
    /// A condition literal did not match the referenced setting type.
    #[error("setting `{setting}` predicate literal has the wrong type for `{referenced}`")]
    PredicateTypeMismatch {
        /// Setting containing the predicate.
        setting: StableId,
        /// Referenced setting ID.
        referenced: Box<StableId>,
    },
    /// A variadic boolean condition contained no children.
    #[error("setting `{setting}` contains an empty predicate group")]
    EmptyPredicateGroup {
        /// Setting containing the predicate.
        setting: StableId,
    },
    /// A condition exceeded the accepted depth limit.
    #[error("setting `{setting}` predicate depth {observed} exceeds {maximum}")]
    PredicateDepthExceeded {
        /// Setting containing the predicate.
        setting: StableId,
        /// Observed depth.
        observed: usize,
        /// Accepted maximum.
        maximum: usize,
    },
    /// A condition exceeded the accepted node limit.
    #[error("setting `{setting}` predicate nodes {observed} exceeds {maximum}")]
    PredicateNodeLimitExceeded {
        /// Setting containing the predicate.
        setting: StableId,
        /// Observed nodes.
        observed: usize,
        /// Accepted maximum.
        maximum: usize,
    },
    /// Presentation predicates formed a dependency cycle.
    #[error("setting predicate dependency cycle includes `{setting}`")]
    PredicateCycle {
        /// Stable-first member known to remain in the cycle graph.
        setting: StableId,
    },
    /// Internal topological accounting found no member for a non-empty cycle.
    #[error("predicate graph accounting invariant failed")]
    PredicateGraphAccounting,
}

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
            let Some(setting) = catalog.catalog.runtime.get(setting_id) else {
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
    for (setting_id, setting) in &catalog.catalog.runtime {
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
    use latticeaxiom_compose::SettingSensitivity;
    use serde_json::json;

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

    fn compile(
        settings: Vec<SettingSpec>,
    ) -> Result<ValidatedSettingsCatalog, SettingsCatalogError> {
        ValidatedSettingsCatalog::compile(
            [SettingsCatalogFragment::new(
                package("@example/runtime"),
                settings,
            )],
            SettingsCatalogPolicy::default(),
        )
    }

    #[test]
    fn catalog_order_is_permutation_invariant() {
        let alpha = setting("@example/runtime", "alpha", ValueType::Bool, json!(true));
        let beta = setting(
            "@example/runtime",
            "beta",
            ValueType::Integer {
                min: Some(0),
                max: Some(10),
                step: Some(2),
            },
            json!(4),
        );
        let forward = compile(vec![alpha.clone(), beta.clone()]);
        let reverse = compile(vec![beta, alpha]);
        assert_eq!(
            forward.ok().and_then(|value| value.canonical_bytes().ok()),
            reverse.ok().and_then(|value| value.canonical_bytes().ok())
        );
    }

    #[test]
    fn collision_diagnostic_is_stable_across_fragment_order() {
        let first = setting("@example/a", "shared", ValueType::Bool, json!(true));
        let second = setting("@example/z", "shared", ValueType::Bool, json!(false));
        let a = SettingsCatalogFragment::new(package("@example/a"), vec![first.clone()]);
        let z = SettingsCatalogFragment::new(package("@example/z"), vec![second.clone()]);
        let forward = ValidatedSettingsCatalog::compile(
            [z.clone(), a.clone()],
            SettingsCatalogPolicy::default(),
        );
        let reverse = ValidatedSettingsCatalog::compile([a, z], SettingsCatalogPolicy::default());
        assert_eq!(forward.err(), reverse.err());
    }

    #[test]
    fn type_range_step_enum_and_nfc_are_validated() {
        let mut integer = setting(
            "@example/runtime",
            "integer",
            ValueType::Integer {
                min: Some(2),
                max: Some(8),
                step: Some(2),
            },
            json!(5),
        );
        assert!(matches!(
            compile(vec![integer.clone()]),
            Err(SettingsCatalogError::InvalidTypedValue { .. })
        ));
        integer.default = json!(6);
        assert!(compile(vec![integer]).is_ok());

        let invalid_enum = setting(
            "@example/runtime",
            "enum",
            ValueType::Enum {
                values: BTreeSet::from([String::new(), "safe".to_owned()]),
            },
            json!("safe"),
        );
        assert!(matches!(
            compile(vec![invalid_enum]),
            Err(SettingsCatalogError::InvalidEnumVariant { .. })
        ));

        let non_nfc = setting(
            "@example/runtime",
            "text",
            ValueType::String {
                min_length: None,
                max_length: Some(16),
            },
            json!("e\u{301}"),
        );
        assert!(matches!(
            compile(vec![non_nfc]),
            Err(SettingsCatalogError::NonCanonicalUnicode { .. })
        ));
    }

    #[test]
    fn obsolete_or_unrepresentable_setting_forms_fail_closed() {
        let mut spec = setting(
            "@example/runtime",
            "legacy-preview",
            ValueType::Bool,
            json!(true),
        );
        spec.apply_impact = RuntimeApplyImpact::Preview;
        assert!(matches!(
            compile(vec![spec]),
            Err(SettingsCatalogError::LegacyPreviewImpact { .. })
        ));

        let mut fixed = setting("@example/runtime", "fixed", ValueType::Bool, json!(true));
        fixed.authority = SettingAuthority::FixedByProfile;
        assert!(matches!(
            compile(vec![fixed]),
            Err(SettingsCatalogError::UnrepresentableFixedByProfile { .. })
        ));

        let binding = setting(
            "@example/runtime",
            "binding",
            ValueType::KeyBinding,
            json!("W"),
        );
        assert!(matches!(
            compile(vec![binding]),
            Err(SettingsCatalogError::UnrepresentableKeyBinding { .. })
        ));
    }

    #[test]
    fn predicates_enforce_depth_unknown_reference_and_cycles() {
        let mut alpha = setting("@example/runtime", "alpha", ValueType::Bool, json!(true));
        alpha.visibility = Some(SettingPredicate::Equals {
            setting: id("example:setting/missing"),
            value: json!(true),
        });
        assert!(matches!(
            compile(vec![alpha.clone()]),
            Err(SettingsCatalogError::UnknownPredicateReference { .. })
        ));

        let mut beta = setting("@example/runtime", "beta", ValueType::Bool, json!(false));
        alpha.visibility = Some(SettingPredicate::Equals {
            setting: beta.id.clone(),
            value: json!(false),
        });
        beta.visibility = Some(SettingPredicate::Equals {
            setting: alpha.id.clone(),
            value: json!(true),
        });
        assert!(matches!(
            compile(vec![alpha, beta]),
            Err(SettingsCatalogError::PredicateCycle { .. })
        ));
    }

    #[test]
    fn predicate_depth_and_node_caps_accept_boundary_and_reject_plus_one() {
        fn nested_not(mut predicate: SettingPredicate, depth: usize) -> SettingPredicate {
            for _ in 1..depth {
                predicate = SettingPredicate::Not {
                    predicate: Box::new(predicate),
                };
            }
            predicate
        }

        let target = setting("@example/runtime", "target", ValueType::Bool, json!(true));
        let leaf = SettingPredicate::Equals {
            setting: target.id.clone(),
            value: json!(true),
        };
        let mut boundary = setting("@example/runtime", "boundary", ValueType::Bool, json!(true));
        boundary.visibility = Some(nested_not(leaf.clone(), MAX_SETTING_PREDICATE_DEPTH));
        assert!(compile(vec![target.clone(), boundary]).is_ok());

        let mut too_deep = setting("@example/runtime", "too-deep", ValueType::Bool, json!(true));
        too_deep.visibility = Some(nested_not(leaf.clone(), MAX_SETTING_PREDICATE_DEPTH + 1));
        assert!(matches!(
            compile(vec![target.clone(), too_deep]),
            Err(SettingsCatalogError::PredicateDepthExceeded { .. })
        ));

        let mut node_boundary = setting(
            "@example/runtime",
            "node-boundary",
            ValueType::Bool,
            json!(true),
        );
        node_boundary.visibility = Some(SettingPredicate::All {
            predicates: vec![leaf.clone(); MAX_SETTING_PREDICATE_NODES - 1],
        });
        assert!(compile(vec![target.clone(), node_boundary]).is_ok());

        let mut too_many_nodes = setting(
            "@example/runtime",
            "too-many-nodes",
            ValueType::Bool,
            json!(true),
        );
        too_many_nodes.visibility = Some(SettingPredicate::All {
            predicates: vec![leaf; MAX_SETTING_PREDICATE_NODES],
        });
        assert!(matches!(
            compile(vec![target, too_many_nodes]),
            Err(SettingsCatalogError::PredicateNodeLimitExceeded { .. })
        ));
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
        let catalog = match compile(vec![spec.clone()]) {
            Ok(value) => value,
            Err(error) => panic!("valid catalog fixture failed: {error}"),
        };
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
        let catalog = match compile(vec![beta.clone(), alpha.clone()]) {
            Ok(value) => value,
            Err(error) => panic!("valid catalog fixture failed: {error}"),
        };
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
    }

    #[test]
    fn unknown_stored_values_are_preserved_but_excluded_from_effective_bytes() {
        let spec = setting("@example/runtime", "known", ValueType::Bool, json!(false));
        let catalog = match compile(vec![spec]) {
            Ok(value) => value,
            Err(error) => panic!("valid catalog fixture failed: {error}"),
        };
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
