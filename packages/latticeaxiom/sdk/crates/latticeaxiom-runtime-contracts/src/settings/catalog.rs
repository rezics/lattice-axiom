//! Validated runtime-setting catalogs compiled from package fragments.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    RuntimeApplyImpact, SettingAuthority, SettingPredicate, SettingScope, SettingSpec,
    SettingsCatalog, ValueType, ValueValidationError,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageName, StableId, canonical_json_bytes,
    canonical_json_hash,
};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use unicode_normalization::is_nfc;

use super::binding::InputBindingV1;

const SETTINGS_PACKAGE_NAME: &str = "@latticeaxiom/settings";

fn is_reserved_foundation_setting(id: &StableId) -> bool {
    matches!(
        id.as_str(),
        "latticeaxiom:setting/ui-scale"
            | "latticeaxiom:setting/view-distance"
            | "latticeaxiom:setting/gameplay/simulation-distance"
            | "latticeaxiom:setting/video/full-detail-distance"
            | "latticeaxiom:setting/video/distant-terrain-quality"
    )
}

/// Default maximum number of runtime settings accepted by one catalog compile.
///
/// This is a defensive implementation limit, not a persistence or ABI constant.
pub const DEFAULT_MAX_RUNTIME_SETTINGS: usize = 4_096;

/// Accepted-ADR limit for one declarative setting predicate.
pub const MAX_SETTING_PREDICATE_DEPTH: usize = 8;

/// Accepted-ADR limit for nodes in one declarative setting predicate.
pub const MAX_SETTING_PREDICATE_NODES: usize = 32;

/// Canonical schema identity for a shipped settings-catalog fragment document.
pub const SETTINGS_CATALOG_FRAGMENT_SCHEMA: &str =
    "latticeaxiom:schema/settings-catalog-fragment@1";

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsCatalogFragmentDocument {
    schema: String,
    owner: PackageName,
    settings: Vec<SettingSpec>,
}

impl SettingsCatalogFragment {
    /// Creates a package-owned runtime-setting fragment.
    #[must_use]
    pub fn new(owner: PackageName, settings: Vec<SettingSpec>) -> Self {
        Self { owner, settings }
    }

    /// Decodes a canonical fragment document authored by one package.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsCatalogError`] when the document schema, owner, or
    /// typed rows cannot be decoded.
    pub fn from_canonical_json(bytes: &[u8]) -> Result<Self, SettingsCatalogError> {
        let document =
            serde_json::from_slice::<SettingsCatalogFragmentDocument>(bytes).map_err(|source| {
                SettingsCatalogError::InvalidCatalogDocument {
                    reason: source.to_string(),
                }
            })?;
        if document.schema != SETTINGS_CATALOG_FRAGMENT_SCHEMA {
            return Err(SettingsCatalogError::InvalidCatalogDocument {
                reason: format!("unsupported fragment schema `{}`", document.schema),
            });
        }
        Ok(Self::new(document.owner, document.settings))
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
/// therefore rejects legacy `Preview` and `FixedByProfile` declarations, and
/// rejects string `KeyBinding` values instead of silently inventing
/// [`super::InputBindingV1`] fields.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(transparent)]
pub struct ValidatedSettingsCatalog {
    catalog: SettingsCatalog,
}

impl ValidatedSettingsCatalog {
    /// Compiles package fragments into stable-ID order and validates them.
    ///
    /// Discovery order cannot affect successful bytes or collision ownership.
    /// Composition parameters are deliberately outside this runtime compiler.
    /// Graph-selected foundation IDs remain owned by `@latticeaxiom/settings`.
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

pub(super) fn validate_setting_value(
    setting: &SettingSpec,
    value: &Value,
) -> Result<(), SettingsCatalogError> {
    if matches!(setting.value_type, ValueType::KeyBinding) {
        if value.as_str().is_some() {
            return Err(SettingsCatalogError::UnrepresentableKeyBinding {
                setting: setting.id.clone(),
            });
        }
        return InputBindingV1::from_json(value)
            .map(|_| ())
            .map_err(|source| SettingsCatalogError::InvalidKeyBinding {
                setting: setting.id.clone(),
                reason: source.to_string(),
            });
    }
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

fn validate_setting(setting: &SettingSpec) -> Result<(), SettingsCatalogError> {
    if setting.id.kind() != "setting" {
        return Err(SettingsCatalogError::WrongSettingKind {
            setting: setting.id.clone(),
            actual: setting.id.kind().to_owned(),
        });
    }
    if is_reserved_foundation_setting(&setting.id)
        && setting.declared_by.as_str() != SETTINGS_PACKAGE_NAME
    {
        return Err(SettingsCatalogError::FoundationOwnerMismatch {
            setting: setting.id.clone(),
            declared_by: setting.declared_by.clone(),
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
        ValueType::Bool
        | ValueType::Integer { .. }
        | ValueType::Number { .. }
        | ValueType::Color
        | ValueType::KeyBinding => {}
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
    /// A reserved first-consumer ID was not owned by the settings package.
    #[error(
        "foundation setting `{setting}` must be declared by `@latticeaxiom/settings`, not `{declared_by}`"
    )]
    FoundationOwnerMismatch {
        /// Reserved setting ID.
        setting: StableId,
        /// Rejected owner.
        declared_by: PackageName,
    },
    /// A catalog fragment document could not be decoded.
    #[error("settings catalog fragment is invalid: {reason}")]
    InvalidCatalogDocument {
        /// Decoder diagnostic.
        reason: String,
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
    /// The current DTO represented key bindings as untyped strings.
    #[error(
        "setting `{setting}` uses a string KeyBinding; accepted ADR 0025 requires InputBindingV1"
    )]
    UnrepresentableKeyBinding {
        /// Invalid declaration ID.
        setting: StableId,
    },
    /// A typed `InputBindingV1` object failed validation.
    #[error("setting `{setting}` has an invalid input binding: {reason}")]
    InvalidKeyBinding {
        /// Invalid declaration ID.
        setting: StableId,
        /// Binding diagnostic.
        reason: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_compose::SettingSensitivity;
    use serde_json::json;

    use crate::settings::binding::KnownInputBindingV1;

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
    fn typed_input_binding_defaults_are_accepted() {
        let default =
            match serde_json::to_value(InputBindingV1::Known(KnownInputBindingV1::Keyboard {
                usage: "KeyW".to_owned(),
                modifiers: crate::settings::binding::KeyboardModifiersV1::default(),
            })) {
                Ok(value) => value,
                Err(error) => panic!("binding default encode failed: {error}"),
            };
        let spec = setting(
            "@example/runtime",
            "binding",
            ValueType::KeyBinding,
            default,
        );
        assert!(compile(vec![spec]).is_ok());
    }

    #[test]
    fn reserved_foundation_ids_cannot_change_owner() {
        let mut stolen = setting(
            "@example/runtime",
            "ui-scale",
            ValueType::Number {
                min: None,
                max: None,
                step: None,
            },
            json!(1.0),
        );
        stolen.id = id("latticeaxiom:setting/ui-scale");
        assert!(matches!(
            compile(vec![stolen]),
            Err(SettingsCatalogError::FoundationOwnerMismatch { .. })
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
}
