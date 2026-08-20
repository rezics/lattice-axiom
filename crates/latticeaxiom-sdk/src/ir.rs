//! Sealed registration IR produced by explicit macro expansion.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{
    CanonicalHash, PackageName, PackageVersion, SchemaId, SourceProvenance, StableId,
    canonical_json_hash,
};
use serde::Serialize;

use crate::{
    ComponentMode, RegistrationIrError,
    component::ComponentSeed,
    system::{
        ComponentAccess, ComponentAccessKind, NativeStaticOnlyReason, ParameterSeed, QueryFilter,
        SystemParameter, SystemPolicySeed, SystemPortability, SystemSeed, SystemSignature,
    },
};

const STAGES_V1: [&str; 10] = [
    "latticeaxiom:system-stage/input/sample@1",
    "latticeaxiom:system-stage/gameplay/fixed-pre@1",
    "latticeaxiom:system-stage/gameplay/fixed@1",
    "latticeaxiom:system-stage/physics/integrate@1",
    "latticeaxiom:system-stage/world/commands-apply@1",
    "latticeaxiom:system-stage/world/revision-commit@1",
    "latticeaxiom:system-stage/derived-work/queue@1",
    "latticeaxiom:system-stage/persistence/capture@1",
    "latticeaxiom:system-stage/async-results/observe@1",
    "latticeaxiom:system-stage/presentation/update@1",
];

/// One validated component row in the sealed registration IR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentIr {
    pub(crate) id: StableId,
    pub(crate) schema: Option<SchemaId>,
    pub(crate) mode: ComponentMode,
    pub(crate) rust_api_fingerprint: CanonicalHash,
}

impl ComponentIr {
    /// Returns the stable component registration ID.
    #[must_use]
    pub const fn id(&self) -> &StableId {
        &self.id
    }

    /// Returns the optional public versioned schema.
    #[must_use]
    pub const fn schema(&self) -> Option<&SchemaId> {
        self.schema.as_ref()
    }

    /// Returns the selected schema realization mode.
    #[must_use]
    pub const fn mode(&self) -> ComponentMode {
        self.mode
    }

    /// Returns the generated Rust API fingerprint used by build planning.
    #[must_use]
    pub const fn rust_api_fingerprint(&self) -> CanonicalHash {
        self.rust_api_fingerprint
    }
}

/// One validated system row in the sealed registration IR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemIr {
    pub(crate) signature: SystemSignature,
    pub(crate) row_kernel_symbol: String,
    pub(crate) rust_signature_fingerprint: CanonicalHash,
}

impl SystemIr {
    /// Returns the canonical system signature.
    #[must_use]
    pub const fn signature(&self) -> &SystemSignature {
        &self.signature
    }

    /// Returns the source-level row-kernel symbol for static glue generation.
    #[must_use]
    pub fn row_kernel_symbol(&self) -> &str {
        &self.row_kernel_symbol
    }

    /// Returns the source Rust signature fingerprint, excluded from semantics.
    #[must_use]
    pub const fn rust_signature_fingerprint(&self) -> CanonicalHash {
        self.rust_signature_fingerprint
    }
}

/// Validated, order-independent code registration IR for one package.
///
/// Fields are private and no public builder exists. The explicit
/// [`crate::registration_ir`] macro is the only supported authoring entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationIr {
    pub(crate) package: PackageName,
    pub(crate) version: PackageVersion,
    pub(crate) components: BTreeMap<StableId, ComponentIr>,
    pub(crate) systems: BTreeMap<StableId, SystemIr>,
}

impl RegistrationIr {
    /// Returns the logical owner package.
    #[must_use]
    pub const fn package(&self) -> &PackageName {
        &self.package
    }

    /// Returns the exact package version.
    #[must_use]
    pub const fn version(&self) -> &PackageVersion {
        &self.version
    }

    /// Returns components in canonical `StableId` byte order.
    #[must_use]
    pub const fn components(&self) -> &BTreeMap<StableId, ComponentIr> {
        &self.components
    }

    /// Returns systems in canonical `StableId` byte order.
    #[must_use]
    pub const fn systems(&self) -> &BTreeMap<StableId, SystemIr> {
        &self.systems
    }
}

/// Complete source provenance keyed by generated row or callback ID.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProvenanceCatalog {
    rows: BTreeMap<StableId, SourceProvenance>,
}

impl ProvenanceCatalog {
    /// Creates an explicitly keyed provenance catalog.
    #[must_use]
    pub const fn new(rows: BTreeMap<StableId, SourceProvenance>) -> Self {
        Self { rows }
    }

    /// Returns the canonical provenance map.
    #[must_use]
    pub const fn rows(&self) -> &BTreeMap<StableId, SourceProvenance> {
        &self.rows
    }

    pub(crate) fn get(&self, id: &StableId) -> Option<&SourceProvenance> {
        self.rows.get(id)
    }
}

/// Builds the sealed IR from macro-generated seeds.
///
/// # Errors
///
/// Returns [`RegistrationIrError`] for malformed identifiers, duplicate rows,
/// unsupported stages, inconsistent schemas, or invalid system access.
#[doc(hidden)]
pub fn build_registration_ir(
    package: &str,
    version: &str,
    component_seeds: Vec<ComponentSeed>,
    system_seeds: Vec<SystemSeed>,
) -> Result<RegistrationIr, RegistrationIrError> {
    let package = parse_identifier("package", package)?;
    let version = parse_identifier("package-version", version)?;
    let components = compile_components(component_seeds)?;
    let systems = compile_systems(system_seeds, &components)?;
    validate_global_ids(&components, &systems)?;
    validate_local_ordering(&systems)?;
    Ok(RegistrationIr {
        package,
        version,
        components,
        systems,
    })
}

fn compile_components(
    seeds: Vec<ComponentSeed>,
) -> Result<BTreeMap<StableId, ComponentIr>, RegistrationIrError> {
    let mut components = BTreeMap::new();
    let mut schema_fingerprints = BTreeMap::<SchemaId, CanonicalHash>::new();
    for seed in seeds {
        let id = parse_identifier("component-id", seed.id)?;
        let schema: Option<SchemaId> = seed
            .schema
            .map(|value| parse_identifier("component-schema", value))
            .transpose()?;
        if !matches!(seed.mode, ComponentMode::HostTyped) && schema.is_none() {
            return Err(RegistrationIrError::MissingPublicSchema {
                component: id,
                mode: component_mode_name(seed.mode),
            });
        }
        let rust_api_fingerprint = CanonicalHash::digest(seed.rust_api_descriptor.as_bytes());
        if let Some(schema) = &schema
            && let Some(previous) = schema_fingerprints.insert(schema.clone(), rust_api_fingerprint)
            && previous != rust_api_fingerprint
        {
            return Err(RegistrationIrError::ConflictingSchemaFingerprint {
                schema: schema.clone(),
            });
        }
        let component = ComponentIr {
            id: id.clone(),
            schema,
            mode: seed.mode,
            rust_api_fingerprint,
        };
        if components.insert(id.clone(), component).is_some() {
            return Err(RegistrationIrError::DuplicateRegistration { id });
        }
    }
    Ok(components)
}

fn compile_systems(
    seeds: Vec<SystemSeed>,
    components: &BTreeMap<StableId, ComponentIr>,
) -> Result<BTreeMap<StableId, SystemIr>, RegistrationIrError> {
    let mut systems = BTreeMap::new();
    for seed in seeds {
        let system = compile_system(seed, components)?;
        let id = system.signature.system.clone();
        if systems.insert(id.clone(), system).is_some() {
            return Err(RegistrationIrError::DuplicateRegistration { id });
        }
    }
    Ok(systems)
}

fn compile_system(
    seed: SystemSeed,
    components: &BTreeMap<StableId, ComponentIr>,
) -> Result<SystemIr, RegistrationIrError> {
    let system = parse_identifier("system-id", seed.id)?;
    let callback = parse_identifier("callback-key", seed.callback)?;
    require_contract_major("callback-key", &callback)?;
    let stage = parse_identifier("system-stage", seed.stage)?;
    if !STAGES_V1.contains(&seed.stage) {
        return Err(RegistrationIrError::InvalidIdentifier {
            field: "system-stage",
            value: seed.stage.to_owned(),
            reason: "not in the system-stage catalog v1".to_owned(),
        });
    }
    let set = seed
        .set
        .map(|value| parse_identifier("system-set", value))
        .transpose()?;
    let after = parse_id_set("system-after", seed.after)?;
    let before = parse_id_set("system-before", seed.before)?;
    if after.contains(&system) || before.contains(&system) {
        return Err(RegistrationIrError::SelfOrderingEdge { system });
    }

    let (parameters, mut portability_reasons) =
        compile_parameters(seed.parameters, &system, components)?;

    if let Some(component) = parameters.iter().find_map(|parameter| match parameter {
        SystemParameter::Component { value, .. } if value.schema.is_none() => {
            Some(&value.component)
        }
        _ => None,
    }) {
        match seed.policy {
            SystemPolicySeed::Dual => {
                return Err(RegistrationIrError::DualComponentWithoutSchema {
                    system: system.clone(),
                    component: Box::new(component.clone()),
                });
            }
            SystemPolicySeed::Auto | SystemPolicySeed::NativeStaticOnly => {
                portability_reasons.insert(NativeStaticOnlyReason::UnsupportedSystemParameter);
            }
        }
    }

    let portability = match seed.policy {
        SystemPolicySeed::Dual | SystemPolicySeed::Auto if portability_reasons.is_empty() => {
            SystemPortability::Dual
        }
        SystemPolicySeed::Dual => {
            return Err(RegistrationIrError::InvalidIdentifier {
                field: "system-parameter",
                value: system.to_string(),
                reason: "explicit dual policy contains a static-only parameter".to_owned(),
            });
        }
        SystemPolicySeed::Auto => SystemPortability::NativeStaticOnly {
            reasons: portability_reasons,
        },
        SystemPolicySeed::NativeStaticOnly => {
            portability_reasons.insert(NativeStaticOnlyReason::ExplicitPolicy);
            SystemPortability::NativeStaticOnly {
                reasons: portability_reasons,
            }
        }
    };

    let signature_hash = canonical_json_hash(&SignatureIdentity {
        system: &system,
        callback: &callback,
        stage: &stage,
        set: set.as_ref(),
        after: &after,
        before: &before,
        parameters: &parameters,
        portability: &portability,
    })?;
    Ok(SystemIr {
        signature: SystemSignature {
            system,
            callback,
            stage,
            set,
            after,
            before,
            parameters,
            portability,
            signature_hash,
        },
        row_kernel_symbol: seed.row_kernel_symbol.to_owned(),
        rust_signature_fingerprint: CanonicalHash::digest(
            seed.rust_signature_descriptor.as_bytes(),
        ),
    })
}

fn compile_parameters(
    seeds: Vec<ParameterSeed>,
    system: &StableId,
    components: &BTreeMap<StableId, ComponentIr>,
) -> Result<(Vec<SystemParameter>, BTreeSet<NativeStaticOnlyReason>), RegistrationIrError> {
    let mut parameters = Vec::with_capacity(seeds.len());
    let mut portability_reasons = BTreeSet::new();
    let mut component_access = BTreeMap::<StableId, (ComponentAccessKind, bool)>::new();
    let mut filters = BTreeMap::<StableId, bool>::new();
    for (index, parameter) in seeds.into_iter().enumerate() {
        let ordinal = u32::try_from(index).map_err(|_| RegistrationIrError::TooManyParameters {
            system: system.clone(),
        })?;
        parameters.push(compile_parameter(
            parameter,
            ordinal,
            system,
            components,
            &mut component_access,
            &mut filters,
            &mut portability_reasons,
        )?);
    }
    Ok((parameters, portability_reasons))
}

#[allow(
    clippy::too_many_arguments,
    reason = "parameter validation updates the three canonical conflict indexes atomically"
)]
fn compile_parameter(
    seed: ParameterSeed,
    ordinal: u32,
    system: &StableId,
    components: &BTreeMap<StableId, ComponentIr>,
    component_access: &mut BTreeMap<StableId, (ComponentAccessKind, bool)>,
    filters: &mut BTreeMap<StableId, bool>,
    portability_reasons: &mut BTreeSet<NativeStaticOnlyReason>,
) -> Result<SystemParameter, RegistrationIrError> {
    match seed {
        ParameterSeed::Component {
            component,
            requirement,
            access,
        } => {
            let id = parse_identifier("component-id", component.id)?;
            let Some(declaration) = components.get(&id) else {
                return Err(RegistrationIrError::UndeclaredComponent {
                    system: system.clone(),
                    component: Box::new(id),
                });
            };
            let optional = matches!(requirement, crate::system::ComponentRequirement::Optional);
            if component_access
                .insert(id.clone(), (access, optional))
                .is_some()
            {
                return Err(RegistrationIrError::ConflictingComponentAccess {
                    system: system.clone(),
                    component: Box::new(id),
                });
            }
            Ok(SystemParameter::Component {
                ordinal,
                value: ComponentAccess {
                    component: id,
                    schema: declaration.schema.clone(),
                    requirement,
                    access,
                },
            })
        }
        ParameterSeed::Filter { component, with } => {
            let id = parse_identifier("component-id", component.id)?;
            if !components.contains_key(&id) {
                return Err(RegistrationIrError::UndeclaredComponent {
                    system: system.clone(),
                    component: Box::new(id),
                });
            }
            if let Some(previous) = filters.insert(id.clone(), with) {
                if previous != with {
                    return Err(RegistrationIrError::ConflictingQueryFilter {
                        system: system.clone(),
                        component: Box::new(id),
                    });
                }
                return Err(RegistrationIrError::ConflictingComponentAccess {
                    system: system.clone(),
                    component: Box::new(id),
                });
            }
            Ok(SystemParameter::Filter {
                ordinal,
                value: if with {
                    QueryFilter::With { component: id }
                } else {
                    QueryFilter::Without { component: id }
                },
            })
        }
        ParameterSeed::RowEntity => Ok(SystemParameter::RowEntity { ordinal }),
        ParameterSeed::FixedTick => Ok(SystemParameter::FixedTick { ordinal }),
        ParameterSeed::CommandSink => Ok(SystemParameter::CommandSink { ordinal }),
        ParameterSeed::StaticOnly { reason } => {
            portability_reasons.insert(reason);
            Ok(SystemParameter::StaticOnly { ordinal, reason })
        }
    }
}

fn validate_global_ids(
    components: &BTreeMap<StableId, ComponentIr>,
    systems: &BTreeMap<StableId, SystemIr>,
) -> Result<(), RegistrationIrError> {
    let mut ids = BTreeSet::new();
    for id in components.keys() {
        ids.insert(id.clone());
    }
    let mut callbacks = BTreeSet::new();
    for (id, system) in systems {
        if !ids.insert(id.clone()) {
            return Err(RegistrationIrError::DuplicateRegistration { id: id.clone() });
        }
        if !callbacks.insert(system.signature.callback.clone()) {
            return Err(RegistrationIrError::DuplicateCallback {
                callback: system.signature.callback.clone(),
            });
        }
        if !ids.insert(system.signature.callback.clone()) {
            return Err(RegistrationIrError::DuplicateRegistration {
                id: system.signature.callback.clone(),
            });
        }
    }
    Ok(())
}

fn validate_local_ordering(
    systems: &BTreeMap<StableId, SystemIr>,
) -> Result<(), RegistrationIrError> {
    for system in systems.values() {
        for target in system
            .signature
            .after
            .iter()
            .chain(system.signature.before.iter())
        {
            if let Some(target) = systems.get(target)
                && target.signature.stage != system.signature.stage
            {
                return Err(RegistrationIrError::InvalidIdentifier {
                    field: "system-ordering",
                    value: target.signature.system.to_string(),
                    reason: "local ordering edges must remain within one semantic stage".to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn parse_id_set(
    field: &'static str,
    values: Vec<&'static str>,
) -> Result<BTreeSet<StableId>, RegistrationIrError> {
    values
        .into_iter()
        .map(|value| parse_identifier(field, value))
        .collect()
}

fn parse_identifier<T>(field: &'static str, value: &str) -> Result<T, RegistrationIrError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|error: T::Err| RegistrationIrError::InvalidIdentifier {
            field,
            value: value.to_owned(),
            reason: error.to_string(),
        })
}

fn require_contract_major(field: &'static str, id: &StableId) -> Result<(), RegistrationIrError> {
    let value = id.to_string();
    let has_positive_major = value
        .rsplit_once('@')
        .and_then(|(_, major)| major.parse::<u64>().ok())
        .is_some_and(|major| major > 0);
    if has_positive_major {
        Ok(())
    } else {
        Err(RegistrationIrError::MissingContractMajor {
            field,
            id: id.clone(),
        })
    }
}

const fn component_mode_name(mode: ComponentMode) -> &'static str {
    match mode {
        ComponentMode::HostTyped => "host-typed",
        ComponentMode::GeneratedSharedSchema => "generated-shared-schema",
        ComponentMode::RuntimeDynamic => "runtime-dynamic",
    }
}

#[derive(Serialize)]
struct SignatureIdentity<'a> {
    system: &'a StableId,
    callback: &'a StableId,
    stage: &'a StableId,
    set: Option<&'a StableId>,
    after: &'a BTreeSet<StableId>,
    before: &'a BTreeSet<StableId>,
    parameters: &'a [SystemParameter],
    portability: &'a SystemPortability,
}
