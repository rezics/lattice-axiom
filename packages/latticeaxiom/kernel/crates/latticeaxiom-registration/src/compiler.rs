//! Registration compiler orchestration and closure-wide validation.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    InspectFragmentProviderSpec, LOCK_SCHEMA_VERSION, NumericRegistrationId, ObservabilityCatalog,
    PackageDomain, REGISTRATION_MANIFEST_SCHEMA_VERSION, RegistrationFragment, RegistrationImage,
    RegistrationKind, SettingPredicate, SettingSpec, SettingsCatalog,
};
use latticeaxiom_core::{
    CanonicalHash, NamespaceGrantPattern, PackageName, RegistrationNamespace, SchemaId, StableId,
    canonical_json_hash,
};
use serde::Serialize;

use crate::{
    CallbackBinding, CallbackMapReceipt, CompiledRegistration, NamespaceGrant, NamespaceGrantorRef,
    PackageProvenanceReceipt, PackageRegistrationInput,
    REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION, RegistrationCompileError,
    RegistrationCompileInput, RegistrationImageReceipt, RegistrationProvenanceReceipt,
    SchemaDeclaration, SemanticResolutionReceipt, SystemCallbackProjection, SystemDeclaration,
    schedule::compile_schedule,
    semantics::{SemanticCompileContext, compile_semantics, finalize_numeric_semantics},
};

/// Bounded policy applied before package code activation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistrationCompilerLimits {
    /// Maximum active exact registrations in each registry kind.
    pub registrations_per_kind: usize,
    /// Maximum callback declarations in one closure.
    pub callbacks: usize,
    /// Maximum scheduled system declarations in one closure.
    pub systems: usize,
    /// Maximum Tag definitions.
    pub tag_definitions: usize,
    /// Maximum source nodes in one Role predicate.
    pub predicate_nodes: usize,
    /// Maximum nesting depth in one Role predicate.
    pub predicate_nesting: usize,
}

impl Default for RegistrationCompilerLimits {
    fn default() -> Self {
        Self {
            registrations_per_kind: 65_536,
            callbacks: 65_536,
            systems: 65_536,
            tag_definitions: 4_096,
            predicate_nodes: 256,
            predicate_nesting: 32,
        }
    }
}

/// Stateless deterministic registration compiler.
#[derive(Clone, Copy, Debug, Default)]
pub struct RegistrationCompiler {
    limits: RegistrationCompilerLimits,
}

impl RegistrationCompiler {
    /// Creates a compiler using explicit hard limits.
    #[must_use]
    pub const fn new(limits: RegistrationCompilerLimits) -> Self {
        Self { limits }
    }

    /// Returns the effective compiler limits.
    #[must_use]
    pub const fn limits(self) -> RegistrationCompilerLimits {
        self.limits
    }

    /// Compiles one exact locked closure into independently hashed outputs.
    ///
    /// No package callback or Bevy API is invoked by this operation.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationCompileError`] before producing any partial image
    /// when composition, lock, ownership, schema, callback, schedule, semantic,
    /// or hard-limit validation fails.
    // Linear activation pipeline mirrors the accepted gate order.
    #[allow(clippy::too_many_lines)]
    pub fn compile(
        self,
        input: RegistrationCompileInput<'_>,
    ) -> Result<CompiledRegistration, RegistrationCompileError> {
        validate_composition_and_graph(&input)?;
        validate_package_inputs(&input)?;
        let effective_grants = validate_namespace_grants(&input)?;

        let catalogs = collect_catalogs(&input, &effective_grants, self.limits)?;
        validate_semantic_namespace_authority(input.packages, &effective_grants)?;
        validate_capability_providers(&input, &catalogs.registrations)?;

        let semantic = compile_semantics(&SemanticCompileContext {
            registrations: &catalogs.registrations,
            schemas: &catalogs.schemas,
            packages: input.packages,
            profile_bindings: &input.composition.semantic_bindings,
            max_tag_definitions: self.limits.tag_definitions,
            max_predicate_nodes: self.limits.predicate_nodes,
            max_predicate_nesting: self.limits.predicate_nesting,
        })?;

        let (numeric_by_kind, flattened_numeric, counts) = assign_numeric_ids(
            &catalogs.registrations,
            &semantic.active_registrations,
            self.limits.registrations_per_kind,
        )?;
        let active_systems = catalogs
            .systems
            .iter()
            .filter(|(id, _)| semantic.active_registrations.contains(*id))
            .map(|(id, system)| (id.clone(), *system))
            .collect();
        let schedule = compile_schedule(&active_systems)?;
        let semantic_image = finalize_numeric_semantics(&semantic, &flattened_numeric, &counts)?;

        let owners = semantic
            .active_registrations
            .iter()
            .filter_map(|id| {
                catalogs
                    .registrations
                    .get(id)
                    .map(|(_, owner)| (id.clone(), owner.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let schema_owners = catalogs
            .schemas
            .iter()
            .filter(|(schema, _)| {
                semantic
                    .active_registrations
                    .contains(schema.as_stable_id())
            })
            .map(|(schema, declaration)| (schema.clone(), declaration.declared_by.clone()))
            .collect();
        let authoritative = semantic
            .active_registrations
            .iter()
            .filter(|id| {
                catalogs
                    .registrations
                    .get(*id)
                    .and_then(|(_, owner)| input.graph.packages.get(owner))
                    .is_some_and(|package| package.domains.contains(&PackageDomain::Authoritative))
            })
            .cloned()
            .collect();

        let mut image = RegistrationImage {
            graph_hash: input.graph.graph_hash,
            numeric_ids: flattened_numeric,
            owners,
            schema_owners,
            semantics: semantic.catalog.clone(),
            settings: catalogs.settings.clone(),
            observability: catalogs.observability.clone(),
            schedule: schedule.clone(),
            authoritative,
            image_hash: CanonicalHash::digest(b"unsealed-registration-image"),
        };
        image.image_hash = image.recompute_image_hash()?;

        let registration_semantic_hash = registration_semantic_hash(&input, &effective_grants)?;
        let mut image_receipt = RegistrationImageReceipt {
            schema_version: REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION,
            graph_hash: input.graph.graph_hash,
            registration_semantic_hash,
            image_hash: image.image_hash,
            numeric_ids: numeric_by_kind,
            schedule,
            receipt_hash: CanonicalHash::digest(b"unsealed-image-receipt"),
        };
        image_receipt.receipt_hash = image_receipt.recompute_hash()?;

        let callback_receipt = build_callback_receipt(
            input.graph.graph_hash,
            registration_semantic_hash,
            &catalogs,
            &semantic.active_registrations,
        )?;
        let provenance_receipt = build_provenance_receipt(&input, registration_semantic_hash)?;
        let mut semantic_receipt = SemanticResolutionReceipt {
            schema_version: REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION,
            graph_hash: input.graph.graph_hash,
            registration_semantic_hash,
            semantic_image_hash: semantic_image.semantic_hash,
            active_bundles: semantic.active_bundles,
            role_bindings: semantic.role_bindings,
            explanation: semantic.explanation,
            receipt_hash: CanonicalHash::digest(b"unsealed-semantic-receipt"),
        };
        semantic_receipt.receipt_hash = semantic_receipt.recompute_hash()?;

        let compiled = CompiledRegistration {
            image,
            semantic_image,
            semantic_receipt,
            image_receipt,
            callback_receipt,
            provenance_receipt,
        };
        compiled
            .verify()
            .map_err(|error| RegistrationCompileError::CompiledReceipt(error.to_string()))?;
        Ok(compiled)
    }
}

struct Catalogs<'a> {
    registrations: BTreeMap<StableId, (RegistrationKind, PackageName)>,
    schemas: BTreeMap<SchemaId, SchemaDeclaration>,
    systems: BTreeMap<StableId, &'a SystemDeclaration>,
    callbacks: BTreeMap<StableId, (&'a crate::CallbackDeclaration, BTreeSet<StableId>)>,
    settings: SettingsCatalog,
    observability: ObservabilityCatalog,
}

// One pass keeps closure diagnostics in stable validation order.
#[allow(clippy::too_many_lines)]
fn validate_composition_and_graph(
    input: &RegistrationCompileInput<'_>,
) -> Result<(), RegistrationCompileError> {
    input.composition.validate()?;
    if input.graph.schema_version != LOCK_SCHEMA_VERSION {
        return Err(RegistrationCompileError::UnsupportedLockSchema {
            found: input.graph.schema_version,
            supported: LOCK_SCHEMA_VERSION,
        });
    }
    input.graph.verify_hashes()?;

    let composition_hash = input.composition.semantic_hash()?;
    if composition_hash != input.graph.composition_hash {
        return Err(RegistrationCompileError::CompositionHashMismatch {
            locked: input.graph.composition_hash,
            actual: composition_hash,
        });
    }
    let provenance_hash = input.composition.provenance_hash()?;
    if provenance_hash != input.graph.composition_provenance_hash {
        return Err(RegistrationCompileError::CompositionProvenanceMismatch {
            locked: input.graph.composition_provenance_hash,
            actual: provenance_hash,
        });
    }
    let composition_roots = input
        .composition
        .roots
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if composition_roots != input.graph.roots {
        return Err(RegistrationCompileError::RootSetMismatch);
    }
    if input.composition.policy.evaluation_policy != input.graph.evaluation_policy
        || input.composition.policy.evaluation_limits != input.graph.evaluation_limits
    {
        return Err(RegistrationCompileError::RootSetMismatch);
    }

    for root in &input.graph.roots {
        if !input.graph.packages.contains_key(root) {
            return Err(RegistrationCompileError::MissingRoot {
                package: root.clone(),
            });
        }
    }
    for (key, package) in &input.graph.packages {
        if key != &package.name {
            return Err(RegistrationCompileError::PackageKeyMismatch {
                key: key.clone(),
                embedded: package.name.clone(),
            });
        }
        for (dependency, edge) in &package.dependencies {
            let Some(node) = input.graph.packages.get(dependency) else {
                return Err(RegistrationCompileError::MissingDependency {
                    package: key.clone(),
                    dependency: dependency.clone(),
                });
            };
            if edge.version != node.version {
                return Err(RegistrationCompileError::DependencyVersionMismatch {
                    package: key.clone(),
                    dependency: dependency.clone(),
                    edge_version: edge.version.clone(),
                    node_version: node.version.clone(),
                });
            }
        }
    }
    for (capability, providers) in &input.graph.capability_providers {
        let mut seen = BTreeSet::new();
        if providers.is_empty() || providers.iter().any(|provider| !seen.insert(provider)) {
            return Err(RegistrationCompileError::NonCanonicalCapabilityProviders {
                capability: capability.clone(),
            });
        }
        for provider in providers {
            if !input.graph.packages.contains_key(provider) {
                return Err(RegistrationCompileError::MissingCapabilityProvider {
                    capability: capability.clone(),
                    provider: provider.clone(),
                });
            }
        }
    }

    let mut reachable = input.graph.roots.clone();
    reachable.extend(input.graph.capability_providers.values().flatten().cloned());
    loop {
        let before = reachable.len();
        let dependencies = reachable
            .iter()
            .filter_map(|package| input.graph.packages.get(package))
            .flat_map(|package| package.dependencies.keys().cloned())
            .collect::<Vec<_>>();
        reachable.extend(dependencies);
        if reachable.len() == before {
            break;
        }
    }
    if let Some(package) = input
        .graph
        .packages
        .keys()
        .find(|package| !reachable.contains(*package))
    {
        return Err(RegistrationCompileError::UnreachablePackage {
            package: package.clone(),
        });
    }
    Ok(())
}

fn validate_package_inputs(
    input: &RegistrationCompileInput<'_>,
) -> Result<(), RegistrationCompileError> {
    for package in input.graph.packages.keys() {
        if !input.packages.contains_key(package) {
            return Err(RegistrationCompileError::MissingPackageInput {
                package: package.clone(),
            });
        }
    }
    for package in input.packages.keys() {
        if !input.graph.packages.contains_key(package) {
            return Err(RegistrationCompileError::UnexpectedPackageInput {
                package: package.clone(),
            });
        }
    }
    for (key, package_input) in input.packages {
        let manifest = &package_input.manifest;
        if &manifest.package != key {
            return Err(RegistrationCompileError::ManifestPackageMismatch {
                key: key.clone(),
                manifest: manifest.package.clone(),
            });
        }
        let Some(locked) = input.graph.packages.get(key) else {
            return Err(RegistrationCompileError::UnexpectedPackageInput {
                package: key.clone(),
            });
        };
        if manifest.version != locked.version {
            return Err(RegistrationCompileError::ManifestVersionMismatch {
                package: key.clone(),
                manifest: manifest.version.clone(),
                locked: locked.version.clone(),
            });
        }
        if manifest.schema_version != REGISTRATION_MANIFEST_SCHEMA_VERSION {
            return Err(RegistrationCompileError::UnsupportedManifestSchema {
                package: key.clone(),
                found: manifest.schema_version,
            });
        }
        manifest.verify_semantic_hash()?;
        if manifest.semantic_hash != locked.manifest_hash {
            return Err(RegistrationCompileError::LockedManifestMismatch {
                package: key.clone(),
            });
        }
    }
    Ok(())
}

fn expected_profile_namespace_grants(
    input: &RegistrationCompileInput<'_>,
) -> Result<
    BTreeMap<(PackageName, RegistrationNamespace), BTreeSet<NamespaceGrantPattern>>,
    RegistrationCompileError,
> {
    let mut expected = BTreeMap::new();
    for (grantee, patterns) in &input.composition.policy.namespace_grants {
        for pattern in patterns {
            let namespace = RegistrationNamespace::new(pattern.namespace())
                .map_err(|_| RegistrationCompileError::ProfileNamespaceGrantMismatch)?;
            expected
                .entry((grantee.clone(), namespace))
                .or_insert_with(BTreeSet::new)
                .insert(pattern.clone());
        }
    }
    Ok(expected)
}
// Grant-chain validation is a single bounded fixed-point pass.
#[allow(clippy::too_many_lines)]
fn validate_namespace_grants(
    input: &RegistrationCompileInput<'_>,
) -> Result<BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>, RegistrationCompileError> {
    let mut effective = input
        .graph
        .packages
        .keys()
        .cloned()
        .map(|package| (package, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let expected_profile = expected_profile_namespace_grants(input)?;
    let mut actual_profile = BTreeMap::new();
    let mut delegations = Vec::new();

    for grant in &input.graph.namespace_grants {
        if grant.patterns().is_empty() {
            return Err(RegistrationCompileError::EmptyNamespaceGrant {
                grantee: grant.grantee().clone(),
            });
        }
        if !input.graph.packages.contains_key(grant.grantee()) {
            return Err(RegistrationCompileError::UnexpectedPackageInput {
                package: grant.grantee().clone(),
            });
        }
        if let Some(pattern) = grant
            .patterns()
            .iter()
            .find(|pattern| pattern.namespace() != grant.namespace().as_str())
        {
            return Err(RegistrationCompileError::NamespacePatternMismatch {
                namespace: grant.namespace().clone(),
                pattern: pattern.clone(),
            });
        }
        match grant.grantor().as_ref() {
            NamespaceGrantorRef::Profile { profile } => {
                if profile != &input.composition.profile
                    || !input.graph.roots.contains(grant.grantee())
                {
                    return Err(RegistrationCompileError::UnauthorizedNamespaceGrant {
                        grantee: grant.grantee().clone(),
                    });
                }
                let key = (grant.grantee().clone(), grant.namespace().clone());
                if actual_profile
                    .insert(key, grant.patterns().clone())
                    .is_some()
                {
                    return Err(RegistrationCompileError::ProfileNamespaceGrantMismatch);
                }
                if let Some(patterns) = effective.get_mut(grant.grantee()) {
                    patterns.extend(grant.patterns().iter().cloned());
                }
            }
            NamespaceGrantorRef::Package { package } => {
                let Some(grantor) = input.graph.packages.get(package) else {
                    return Err(RegistrationCompileError::UnauthorizedNamespaceGrant {
                        grantee: grant.grantee().clone(),
                    });
                };
                if !grantor.dependencies.contains_key(grant.grantee()) {
                    return Err(
                        RegistrationCompileError::NamespaceDelegationOutsideDependency {
                            grantor: package.clone(),
                            grantee: grant.grantee().clone(),
                        },
                    );
                }
                delegations.push(grant);
            }
        }
    }

    if actual_profile != expected_profile {
        return Err(RegistrationCompileError::ProfileNamespaceGrantMismatch);
    }
    let mut pending = delegations;
    while !pending.is_empty() {
        let mut next = Vec::new();
        let mut progress = false;
        for grant in pending {
            let NamespaceGrantorRef::Package { package } = grant.grantor().as_ref() else {
                continue;
            };
            let Some(parent_patterns) = effective.get(package) else {
                return Err(RegistrationCompileError::UnauthorizedNamespaceGrant {
                    grantee: grant.grantee().clone(),
                });
            };
            if grant
                .patterns()
                .iter()
                .all(|child| parent_patterns.iter().any(|parent| parent.covers(child)))
            {
                if let Some(patterns) = effective.get_mut(grant.grantee()) {
                    patterns.extend(grant.patterns().iter().cloned());
                }
                progress = true;
            } else {
                next.push(grant);
            }
        }
        if !progress {
            let grant = next
                .first()
                .ok_or(RegistrationCompileError::LimitExceeded {
                    limit: "namespace-delegation-chain",
                    observed: 0,
                    maximum: input.graph.namespace_grants.len(),
                })?;
            let grantor = match grant.grantor().as_ref() {
                NamespaceGrantorRef::Package { package } => package.clone(),
                NamespaceGrantorRef::Profile { .. } => grant.grantee().clone(),
            };
            return Err(RegistrationCompileError::NamespaceDelegationExpansion {
                grantor,
                grantee: grant.grantee().clone(),
            });
        }
        pending = next;
    }
    Ok(effective)
}

fn ensure_namespace_authority(
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    package: &PackageName,
    id: &StableId,
) -> Result<(), RegistrationCompileError> {
    if grants
        .get(package)
        .is_some_and(|patterns| patterns.iter().any(|pattern| pattern.matches(id)))
    {
        Ok(())
    } else {
        Err(RegistrationCompileError::MissingNamespaceGrant {
            package: package.clone(),
            id: id.clone(),
        })
    }
}

fn canonical_rows<T: Serialize>(rows: &[T]) -> Result<Vec<&T>, RegistrationCompileError> {
    let mut keyed = rows
        .iter()
        .map(|row| latticeaxiom_core::canonical_json_bytes(row).map(|bytes| (bytes, row)))
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    Ok(keyed.into_iter().map(|(_, row)| row).collect())
}

fn validate_semantic_namespace_authority(
    packages: &BTreeMap<PackageName, PackageRegistrationInput>,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
) -> Result<(), RegistrationCompileError> {
    for (package, input) in packages {
        for fragment in canonical_rows(&input.manifest.fragment.semantics)? {
            validate_semantic_fragment_namespace_authority(package, fragment, grants)?;
        }
    }
    Ok(())
}

fn validate_semantic_fragment_namespace_authority(
    package: &PackageName,
    fragment: &latticeaxiom_compose::SemanticFragment,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
) -> Result<(), RegistrationCompileError> {
    use latticeaxiom_compose::SemanticFragment;

    let id = match fragment {
        SemanticFragment::TagDefinition(definition) => Some(&definition.tag.id),
        SemanticFragment::MapDefinition(definition) => Some(&definition.map.id),
        SemanticFragment::StateProperty(definition) => Some(&definition.id),
        SemanticFragment::Affordance(definition) => Some(&definition.id),
        SemanticFragment::Role(definition) => Some(&definition.id),
        SemanticFragment::Bundle(bundle) => Some(&bundle.id),
        SemanticFragment::TagContribution(_)
        | SemanticFragment::MapContribution(_)
        | SemanticFragment::RoleOffer(_) => None,
    };
    if let Some(id) = id {
        ensure_namespace_authority(grants, package, id)?;
    }
    if let SemanticFragment::Bundle(bundle) = fragment {
        for nested in canonical_rows(&bundle.semantics)? {
            validate_semantic_fragment_namespace_authority(package, nested, grants)?;
        }
    }
    Ok(())
}

fn collect_catalogs<'a>(
    input: &'a RegistrationCompileInput<'_>,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    limits: RegistrationCompilerLimits,
) -> Result<Catalogs<'a>, RegistrationCompileError> {
    let mut registrations = BTreeMap::new();
    let mut schemas = BTreeMap::new();
    let mut systems = BTreeMap::new();
    let mut callback_declarations = BTreeMap::new();
    let mut callback_consumers = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    let mut settings = SettingsCatalog::default();
    let mut observability = ObservabilityCatalog::default();
    let mut catalog_ids = BTreeMap::<StableId, &'static str>::new();

    for (package, package_input) in input.packages {
        collect_exact_registrations(
            package,
            &package_input.manifest.fragment,
            grants,
            &mut registrations,
            &mut catalog_ids,
        )?;
        collect_schemas(package, package_input, grants, &mut schemas)?;
        collect_systems_and_callbacks(
            package,
            package_input,
            grants,
            &mut systems,
            &mut callback_declarations,
            &mut callback_consumers,
        )?;
        collect_settings(
            package,
            &package_input.manifest.fragment,
            grants,
            &mut settings,
            &mut catalog_ids,
        )?;
        collect_observability(
            package,
            &package_input.manifest.fragment,
            grants,
            &mut observability,
            &mut callback_consumers,
            &mut catalog_ids,
        )?;
    }

    validate_schema_links(input, &registrations, &schemas)?;
    validate_system_links(&registrations, &systems)?;
    compile_schedule(&systems)?;
    validate_setting_links(
        input,
        &settings,
        limits.predicate_nodes,
        limits.predicate_nesting,
    )?;
    validate_observability_links(&observability, &schemas)?;
    validate_callback_links(
        &systems,
        &callback_declarations,
        &callback_consumers,
        &observability,
    )?;
    if systems.len() > limits.systems {
        return Err(RegistrationCompileError::LimitExceeded {
            limit: "systems",
            observed: systems.len(),
            maximum: limits.systems,
        });
    }
    if callback_declarations.len() > limits.callbacks {
        return Err(RegistrationCompileError::LimitExceeded {
            limit: "callbacks",
            observed: callback_declarations.len(),
            maximum: limits.callbacks,
        });
    }
    let callbacks = callback_declarations
        .into_iter()
        .map(|(id, declaration)| {
            let consumers = callback_consumers.remove(&id).unwrap_or_default();
            (id, (declaration, consumers))
        })
        .collect();
    Ok(Catalogs {
        registrations,
        schemas,
        systems,
        callbacks,
        settings,
        observability,
    })
}
fn collect_exact_registrations(
    package: &PackageName,
    fragment: &RegistrationFragment,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    registrations: &mut BTreeMap<StableId, (RegistrationKind, PackageName)>,
    catalog_ids: &mut BTreeMap<StableId, &'static str>,
) -> Result<(), RegistrationCompileError> {
    let mut registrations_by_id = fragment.registrations.iter().collect::<Vec<_>>();
    registrations_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for registration in registrations_by_id {
        verify_owner(
            "exact-registration",
            &registration.id,
            &registration.declared_by,
            package,
        )?;
        let expected = registration_kind_name(registration.kind);
        if registration.id.kind() != expected {
            return Err(RegistrationCompileError::RegistrationKindMismatch {
                id: registration.id.clone(),
                registration_kind: registration.kind,
                expected,
                actual: registration.id.kind().to_owned(),
            });
        }
        ensure_namespace_authority(grants, package, &registration.id)?;
        register_catalog_id(catalog_ids, &registration.id, "exact-registration")?;
        if registrations
            .insert(
                registration.id.clone(),
                (registration.kind, package.clone()),
            )
            .is_some()
        {
            return Err(RegistrationCompileError::DuplicateCatalogId {
                id: registration.id.clone(),
                first: "exact-registration",
                second: "exact-registration",
            });
        }
    }
    Ok(())
}

fn collect_schemas(
    package: &PackageName,
    input: &PackageRegistrationInput,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    schemas: &mut BTreeMap<SchemaId, SchemaDeclaration>,
) -> Result<(), RegistrationCompileError> {
    for (key, declaration) in &input.schemas {
        if key != &declaration.id {
            return Err(RegistrationCompileError::CatalogKeyMismatch {
                catalog: "schemas",
                key: key.to_string(),
                row: declaration.id.to_string(),
            });
        }
        if &declaration.declared_by != package {
            return Err(RegistrationCompileError::ForeignOwner {
                catalog: "schemas",
                id: declaration.id.as_stable_id().clone(),
                declared_by: declaration.declared_by.clone(),
                package: package.clone(),
            });
        }
        if declaration.id.as_stable_id().major().is_none() {
            return Err(RegistrationCompileError::UnversionedSchema {
                schema: declaration.id.clone(),
            });
        }
        ensure_namespace_authority(grants, package, declaration.id.as_stable_id())?;
        if schemas.insert(key.clone(), declaration.clone()).is_some() {
            return Err(RegistrationCompileError::DuplicateCatalogId {
                id: declaration.id.as_stable_id().clone(),
                first: "schema",
                second: "schema",
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_systems_and_callbacks<'a>(
    package: &PackageName,
    input: &'a PackageRegistrationInput,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    systems: &mut BTreeMap<StableId, &'a SystemDeclaration>,
    callbacks: &mut BTreeMap<StableId, &'a crate::CallbackDeclaration>,
    consumers: &mut BTreeMap<StableId, BTreeSet<StableId>>,
) -> Result<(), RegistrationCompileError> {
    for (key, system) in &input.systems {
        if key != &system.id {
            return Err(RegistrationCompileError::CatalogKeyMismatch {
                catalog: "systems",
                key: key.to_string(),
                row: system.id.to_string(),
            });
        }
        verify_owner("systems", &system.id, &system.declared_by, package)?;
        ensure_namespace_authority(grants, package, &system.id)?;
        if systems.insert(key.clone(), system).is_some() {
            return Err(RegistrationCompileError::DuplicateCatalogId {
                id: key.clone(),
                first: "system",
                second: "system",
            });
        }
        consumers
            .entry(system.callback.clone())
            .or_default()
            .insert(system.id.clone());
    }
    for (key, callback) in &input.callbacks {
        if key != &callback.id {
            return Err(RegistrationCompileError::CatalogKeyMismatch {
                catalog: "callbacks",
                key: key.to_string(),
                row: callback.id.to_string(),
            });
        }
        verify_owner("callbacks", &callback.id, &callback.declared_by, package)?;
        if callback.id.kind() != "callback" || callback.id.major().is_none() {
            return Err(RegistrationCompileError::InvalidCallbackId {
                callback: callback.id.clone(),
            });
        }
        ensure_namespace_authority(grants, package, &callback.id)?;
        if callbacks.insert(key.clone(), callback).is_some() {
            return Err(RegistrationCompileError::DuplicateCatalogId {
                id: key.clone(),
                first: "callback",
                second: "callback",
            });
        }
    }
    Ok(())
}

fn collect_settings(
    package: &PackageName,
    fragment: &RegistrationFragment,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    settings: &mut SettingsCatalog,
    catalog_ids: &mut BTreeMap<StableId, &'static str>,
) -> Result<(), RegistrationCompileError> {
    let mut settings_by_id = fragment.settings.iter().collect::<Vec<_>>();
    settings_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for setting in settings_by_id {
        verify_owner("settings", &setting.id, &setting.declared_by, package)?;
        ensure_namespace_authority(grants, package, &setting.id)?;
        register_catalog_id(catalog_ids, &setting.id, "setting")?;
        setting.value_type.validate_value(&setting.default)?;
        if setting.schema_version == 0
            || setting.allowed_scopes.is_empty()
            || !setting.allowed_scopes.contains(&setting.default_scope)
        {
            return Err(RegistrationCompileError::InvalidCatalogRow {
                catalog: "settings",
                id: setting.id.clone(),
                reason: "schema version and persistence scopes must be non-empty and consistent",
            });
        }
        settings.runtime.insert(setting.id.clone(), setting.clone());
    }
    let mut parameters_by_id = fragment.composition_parameters.iter().collect::<Vec<_>>();
    parameters_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for parameter in parameters_by_id {
        verify_owner(
            "composition-parameters",
            &parameter.id,
            &parameter.declared_by,
            package,
        )?;
        ensure_namespace_authority(grants, package, &parameter.id)?;
        register_catalog_id(catalog_ids, &parameter.id, "composition-parameter")?;
        parameter.validate()?;
        settings
            .composition
            .insert(parameter.id.clone(), parameter.clone());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_observability(
    package: &PackageName,
    fragment: &RegistrationFragment,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    observability: &mut ObservabilityCatalog,
    callback_consumers: &mut BTreeMap<StableId, BTreeSet<StableId>>,
    catalog_ids: &mut BTreeMap<StableId, &'static str>,
) -> Result<(), RegistrationCompileError> {
    let mut info_items_by_id = fragment.info_items.iter().collect::<Vec<_>>();
    info_items_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for item in info_items_by_id {
        verify_owner("info-items", &item.id, &item.declared_by, package)?;
        ensure_namespace_authority(grants, package, &item.id)?;
        register_catalog_id(catalog_ids, &item.id, "info-item")?;
        callback_consumers
            .entry(item.callback.clone())
            .or_default()
            .insert(item.id.clone());
        observability
            .info_items
            .insert(item.id.clone(), item.clone());
    }
    let mut metrics_by_id = fragment.metrics.iter().collect::<Vec<_>>();
    metrics_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for metric in metrics_by_id {
        verify_owner("metrics", &metric.id, &metric.declared_by, package)?;
        ensure_namespace_authority(grants, package, &metric.id)?;
        register_catalog_id(catalog_ids, &metric.id, "metric")?;
        if metric.sampling_interval_ms == 0 || metric.history_limit == 0 {
            return Err(RegistrationCompileError::InvalidCatalogRow {
                catalog: "metrics",
                id: metric.id.clone(),
                reason: "sampling interval and history limit must be positive",
            });
        }
        observability
            .metrics
            .insert(metric.id.clone(), metric.clone());
    }
    let mut inspect_by_id = fragment.inspect.iter().collect::<Vec<_>>();
    inspect_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for provider in inspect_by_id {
        verify_owner("inspect", &provider.id, &provider.declared_by, package)?;
        ensure_namespace_authority(grants, package, &provider.id)?;
        register_catalog_id(catalog_ids, &provider.id, "inspect-provider")?;
        if provider.target_kinds.is_empty()
            || !provider.primary_keys.is_disjoint(&provider.extension_slots)
        {
            return Err(RegistrationCompileError::InvalidCatalogRow {
                catalog: "inspect",
                id: provider.id.clone(),
                reason: "target kinds must be non-empty and owned keys must not overlap extension slots",
            });
        }
        callback_consumers
            .entry(provider.callback.clone())
            .or_default()
            .insert(provider.id.clone());
        observability
            .inspect
            .insert(provider.id.clone(), provider.clone());
    }
    let mut visualizers_by_id = fragment.visualizers.iter().collect::<Vec<_>>();
    visualizers_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for visualizer in visualizers_by_id {
        verify_owner(
            "visualizers",
            &visualizer.id,
            &visualizer.declared_by,
            package,
        )?;
        ensure_namespace_authority(grants, package, &visualizer.id)?;
        register_catalog_id(catalog_ids, &visualizer.id, "debug-visualizer")?;
        if visualizer.legend.is_empty()
            || visualizer.budget.radius == 0
            || visualizer.budget.primitives == 0
            || visualizer.budget.upload_bytes == 0
            || visualizer.budget.history == 0
        {
            return Err(RegistrationCompileError::InvalidCatalogRow {
                catalog: "visualizers",
                id: visualizer.id.clone(),
                reason: "legend and all visualizer hard budgets must be non-empty",
            });
        }
        observability
            .visualizers
            .insert(visualizer.id.clone(), visualizer.clone());
    }
    Ok(())
}

fn register_catalog_id(
    ids: &mut BTreeMap<StableId, &'static str>,
    id: &StableId,
    catalog: &'static str,
) -> Result<(), RegistrationCompileError> {
    if let Some(first) = ids.insert(id.clone(), catalog) {
        Err(RegistrationCompileError::DuplicateCatalogId {
            id: id.clone(),
            first,
            second: catalog,
        })
    } else {
        Ok(())
    }
}

fn verify_owner(
    catalog: &'static str,
    id: &StableId,
    declared_by: &PackageName,
    package: &PackageName,
) -> Result<(), RegistrationCompileError> {
    if declared_by == package {
        Ok(())
    } else {
        Err(RegistrationCompileError::ForeignOwner {
            catalog,
            id: id.clone(),
            declared_by: declared_by.clone(),
            package: package.clone(),
        })
    }
}

const fn registration_kind_name(kind: RegistrationKind) -> &'static str {
    match kind {
        RegistrationKind::Dimension => "dimension",
        RegistrationKind::Block => "block",
        RegistrationKind::Item => "item",
        RegistrationKind::Fluid => "fluid",
        RegistrationKind::Biome => "biome",
        RegistrationKind::Component => "component",
        RegistrationKind::Message => "message",
        RegistrationKind::System => "system",
        RegistrationKind::Asset => "asset",
        RegistrationKind::Capability => "capability",
        RegistrationKind::Schema => "schema",
        RegistrationKind::Render => "render",
    }
}
fn validate_schema_links(
    input: &RegistrationCompileInput<'_>,
    registrations: &BTreeMap<StableId, (RegistrationKind, PackageName)>,
    schemas: &BTreeMap<SchemaId, SchemaDeclaration>,
) -> Result<(), RegistrationCompileError> {
    for (schema, declaration) in schemas {
        if !registrations
            .get(schema.as_stable_id())
            .is_some_and(|(kind, owner)| {
                *kind == RegistrationKind::Schema && owner == &declaration.declared_by
            })
        {
            return Err(RegistrationCompileError::MissingSchemaRegistration {
                schema: schema.clone(),
            });
        }
    }
    for (id, (kind, _)) in registrations {
        if *kind == RegistrationKind::Schema {
            let schema = id.as_str().parse::<SchemaId>().map_err(|_| {
                RegistrationCompileError::RegistrationKindMismatch {
                    id: id.clone(),
                    registration_kind: *kind,
                    expected: "schema",
                    actual: id.kind().to_owned(),
                }
            })?;
            if !schemas.contains_key(&schema) {
                return Err(RegistrationCompileError::MissingSchemaRegistration { schema });
            }
        }
    }
    for (package, package_input) in input.packages {
        let Some(locked) = input.graph.packages.get(package) else {
            return Err(RegistrationCompileError::UnexpectedPackageInput {
                package: package.clone(),
            });
        };
        for schema in &locked.schemas {
            if !package_input.schemas.contains_key(schema) {
                return Err(RegistrationCompileError::LockedSchemaUndeclared {
                    package: package.clone(),
                    schema: schema.clone(),
                });
            }
        }
        if let Some(schema) = package_input
            .schemas
            .keys()
            .find(|schema| !locked.schemas.contains(*schema))
        {
            return Err(RegistrationCompileError::UnlockedSchemaDeclaration {
                package: package.clone(),
                schema: schema.clone(),
            });
        }
        let mut registrations_by_id = package_input
            .manifest
            .fragment
            .registrations
            .iter()
            .collect::<Vec<_>>();
        registrations_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        for registration in registrations_by_id {
            if let Some(schema) = &registration.schema {
                if schema.as_stable_id().major().is_none() {
                    return Err(RegistrationCompileError::UnversionedSchema {
                        schema: schema.clone(),
                    });
                }
                if !schemas.contains_key(schema) {
                    return Err(RegistrationCompileError::UndeclaredSchema {
                        consumer: registration.id.clone(),
                        schema: schema.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn validate_system_links(
    registrations: &BTreeMap<StableId, (RegistrationKind, PackageName)>,
    systems: &BTreeMap<StableId, &SystemDeclaration>,
) -> Result<(), RegistrationCompileError> {
    for (id, (kind, _)) in registrations {
        if *kind == RegistrationKind::System && !systems.contains_key(id) {
            return Err(RegistrationCompileError::MissingSystemDeclaration { system: id.clone() });
        }
    }
    for system in systems.keys() {
        if registrations
            .get(system)
            .is_none_or(|(kind, _)| *kind != RegistrationKind::System)
        {
            return Err(RegistrationCompileError::UnexpectedSystemDeclaration {
                system: system.clone(),
            });
        }
    }
    Ok(())
}

fn validate_setting_links(
    input: &RegistrationCompileInput<'_>,
    settings: &SettingsCatalog,
    max_nodes: usize,
    max_nesting: usize,
) -> Result<(), RegistrationCompileError> {
    for (id, setting) in &settings.runtime {
        for predicate in [&setting.visibility, &setting.enabled_when]
            .into_iter()
            .flatten()
        {
            let mut nodes = 0_usize;
            validate_setting_predicate(
                id,
                predicate,
                &settings.runtime,
                1,
                &mut nodes,
                max_nodes,
                max_nesting,
            )?;
        }
        if let Some(replacement) = &setting.replacement
            && !settings.runtime.contains_key(replacement)
        {
            return Err(RegistrationCompileError::InvalidCatalogRow {
                catalog: "settings",
                id: id.clone(),
                reason: "replacement setting is not registered",
            });
        }
    }
    for (id, value) in &input.composition.parameters {
        let Some(parameter) = settings.composition.get(id) else {
            return Err(RegistrationCompileError::InvalidCatalogRow {
                catalog: "composition-parameters",
                id: id.clone(),
                reason: "effective composition value has no registered schema",
            });
        };
        parameter.value_type.validate_value(value)?;
    }
    Ok(())
}

fn validate_setting_predicate(
    consumer: &StableId,
    predicate: &SettingPredicate,
    settings: &BTreeMap<StableId, SettingSpec>,
    depth: usize,
    nodes: &mut usize,
    max_nodes: usize,
    max_nesting: usize,
) -> Result<(), RegistrationCompileError> {
    if depth > max_nesting {
        return Err(RegistrationCompileError::LimitExceeded {
            limit: "setting-predicate-nesting",
            observed: depth,
            maximum: max_nesting,
        });
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or(RegistrationCompileError::LimitExceeded {
            limit: "setting-predicate-nodes",
            observed: usize::MAX,
            maximum: max_nodes,
        })?;
    if *nodes > max_nodes {
        return Err(RegistrationCompileError::LimitExceeded {
            limit: "setting-predicate-nodes",
            observed: *nodes,
            maximum: max_nodes,
        });
    }
    match predicate {
        SettingPredicate::Equals { setting, value } => {
            let Some(referenced) = settings.get(setting) else {
                return Err(RegistrationCompileError::InvalidCatalogRow {
                    catalog: "settings",
                    id: consumer.clone(),
                    reason: "predicate references an unknown setting",
                });
            };
            referenced.value_type.validate_value(value)?;
        }
        SettingPredicate::All { predicates } | SettingPredicate::Any { predicates } => {
            for child in predicates {
                validate_setting_predicate(
                    consumer,
                    child,
                    settings,
                    depth.saturating_add(1),
                    nodes,
                    max_nodes,
                    max_nesting,
                )?;
            }
        }
        SettingPredicate::Not { predicate } => validate_setting_predicate(
            consumer,
            predicate,
            settings,
            depth.saturating_add(1),
            nodes,
            max_nodes,
            max_nesting,
        )?,
    }
    Ok(())
}

fn validate_observability_links(
    observability: &ObservabilityCatalog,
    schemas: &BTreeMap<SchemaId, SchemaDeclaration>,
) -> Result<(), RegistrationCompileError> {
    for item in observability.info_items.values() {
        if !schemas.contains_key(&item.value_schema) {
            return Err(RegistrationCompileError::UndeclaredSchema {
                consumer: item.id.clone(),
                schema: item.value_schema.clone(),
            });
        }
    }
    for provider in observability.inspect.values() {
        for dependency in &provider.after {
            if !observability.inspect.contains_key(dependency) {
                return Err(RegistrationCompileError::InvalidCatalogRow {
                    catalog: "inspect",
                    id: provider.id.clone(),
                    reason: "ordering edge references an unknown inspect provider",
                });
            }
        }
    }
    validate_inspect_acyclic(&observability.inspect)?;
    for visualizer in observability.visualizers.values() {
        if !observability
            .info_items
            .contains_key(&visualizer.data_source)
            && !observability.metrics.contains_key(&visualizer.data_source)
        {
            return Err(RegistrationCompileError::InvalidCatalogRow {
                catalog: "visualizers",
                id: visualizer.id.clone(),
                reason: "data source must be a registered info item or metric",
            });
        }
    }
    Ok(())
}

fn validate_inspect_acyclic(
    providers: &BTreeMap<StableId, InspectFragmentProviderSpec>,
) -> Result<(), RegistrationCompileError> {
    let mut incoming = providers
        .keys()
        .cloned()
        .map(|id| (id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = providers
        .keys()
        .cloned()
        .map(|id| (id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for provider in providers.values() {
        for dependency in &provider.after {
            if outgoing
                .get_mut(dependency)
                .is_some_and(|targets| targets.insert(provider.id.clone()))
            {
                let Some(count) = incoming.get_mut(&provider.id) else {
                    return Err(RegistrationCompileError::InvalidCatalogRow {
                        catalog: "inspect",
                        id: provider.id.clone(),
                        reason: "provider is absent from its own catalog",
                    });
                };
                *count = count
                    .checked_add(1)
                    .ok_or(RegistrationCompileError::LimitExceeded {
                        limit: "inspect-order-edges",
                        observed: usize::MAX,
                        maximum: usize::MAX - 1,
                    })?;
            }
        }
    }
    let mut ready = incoming
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut visited = 0_usize;
    while let Some(next) = ready.pop_first() {
        visited = visited.saturating_add(1);
        if let Some(targets) = outgoing.get(&next) {
            for target in targets {
                if let Some(count) = incoming.get_mut(target) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.insert(target.clone());
                    }
                }
            }
        }
    }
    if visited == providers.len() {
        Ok(())
    } else {
        let id = incoming
            .into_iter()
            .find(|(_, count)| *count > 0)
            .map(|(id, _)| id)
            .ok_or(RegistrationCompileError::LimitExceeded {
                limit: "inspect-order-cycle-diagnostic",
                observed: visited,
                maximum: providers.len(),
            })?;
        Err(RegistrationCompileError::InvalidCatalogRow {
            catalog: "inspect",
            id,
            reason: "provider ordering contains a cycle",
        })
    }
}

fn validate_callback_links(
    systems: &BTreeMap<StableId, &SystemDeclaration>,
    callbacks: &BTreeMap<StableId, &crate::CallbackDeclaration>,
    consumers: &BTreeMap<StableId, BTreeSet<StableId>>,
    observability: &ObservabilityCatalog,
) -> Result<(), RegistrationCompileError> {
    for (callback, callback_consumers) in consumers {
        let Some(declaration) = callbacks.get(callback) else {
            let consumer = callback_consumers
                .first()
                .cloned()
                .unwrap_or_else(|| callback.clone());
            return Err(RegistrationCompileError::UndeclaredCallback {
                consumer,
                callback: callback.clone(),
            });
        };
        for consumer in callback_consumers {
            if let Some(system) = systems.get(consumer) {
                if system.declared_by != declaration.declared_by {
                    return Err(RegistrationCompileError::ForeignOwner {
                        catalog: "callbacks",
                        id: callback.clone(),
                        declared_by: declaration.declared_by.clone(),
                        package: system.declared_by.clone(),
                    });
                }
                if system.signature_hash != declaration.signature_hash {
                    return Err(RegistrationCompileError::CallbackSignatureMismatch {
                        system: system.id.clone(),
                        callback: callback.clone(),
                    });
                }
            } else {
                let owner = observability
                    .info_items
                    .get(consumer)
                    .map(|row| &row.declared_by)
                    .or_else(|| {
                        observability
                            .inspect
                            .get(consumer)
                            .map(|row| &row.declared_by)
                    });
                if owner != Some(&declaration.declared_by) {
                    return Err(RegistrationCompileError::ForeignOwner {
                        catalog: "callbacks",
                        id: callback.clone(),
                        declared_by: declaration.declared_by.clone(),
                        package: owner
                            .cloned()
                            .unwrap_or_else(|| declaration.declared_by.clone()),
                    });
                }
            }
        }
    }
    for callback in callbacks.keys() {
        if !consumers.contains_key(callback) {
            return Err(RegistrationCompileError::UnusedCallback {
                callback: callback.clone(),
            });
        }
    }
    Ok(())
}
fn validate_capability_providers(
    input: &RegistrationCompileInput<'_>,
    _registrations: &BTreeMap<StableId, (RegistrationKind, PackageName)>,
) -> Result<(), RegistrationCompileError> {
    for (capability, providers) in &input.graph.capability_providers {
        for provider in providers {
            let Some(package) = input.packages.get(provider) else {
                return Err(RegistrationCompileError::MissingCapabilityProvider {
                    capability: capability.clone(),
                    provider: provider.clone(),
                });
            };
            if !package.provided_capabilities.contains(capability) {
                return Err(RegistrationCompileError::UndeclaredCapabilityProvider {
                    capability: capability.clone(),
                    provider: provider.clone(),
                });
            }
        }
    }
    Ok(())
}

type NumericTables = (
    BTreeMap<RegistrationKind, BTreeMap<StableId, NumericRegistrationId>>,
    BTreeMap<StableId, NumericRegistrationId>,
    BTreeMap<RegistrationKind, usize>,
);

fn assign_numeric_ids(
    registrations: &BTreeMap<StableId, (RegistrationKind, PackageName)>,
    active: &BTreeSet<StableId>,
    maximum: usize,
) -> Result<NumericTables, RegistrationCompileError> {
    let mut grouped = BTreeMap::<RegistrationKind, Vec<StableId>>::new();
    for id in active {
        let Some((kind, _)) = registrations.get(id) else {
            return Err(RegistrationCompileError::UnknownSemanticContract {
                contract: id.clone(),
            });
        };
        grouped.entry(*kind).or_default().push(id.clone());
    }
    let mut tables = BTreeMap::new();
    let mut flattened = BTreeMap::new();
    let mut counts = BTreeMap::new();
    for (kind, ids) in grouped {
        if ids.len() > maximum {
            return Err(RegistrationCompileError::LimitExceeded {
                limit: "registrations-per-kind",
                observed: ids.len(),
                maximum,
            });
        }
        let mut table = BTreeMap::new();
        for (index, id) in ids.into_iter().enumerate() {
            let numeric = u32::try_from(index)
                .map_err(|_| RegistrationCompileError::NumericIdOverflow { kind })?;
            let numeric = NumericRegistrationId(numeric);
            table.insert(id.clone(), numeric);
            flattened.insert(id, numeric);
        }
        counts.insert(kind, table.len());
        tables.insert(kind, table);
    }
    Ok((tables, flattened, counts))
}

fn build_callback_receipt(
    graph_hash: CanonicalHash,
    registration_semantic_hash: CanonicalHash,
    catalogs: &Catalogs<'_>,
    active_registrations: &BTreeSet<StableId>,
) -> Result<CallbackMapReceipt, RegistrationCompileError> {
    let mut callbacks = BTreeMap::new();
    for (id, (declaration, consumers)) in &catalogs.callbacks {
        let active_consumers = consumers
            .iter()
            .filter(|consumer| {
                !catalogs.systems.contains_key(*consumer)
                    || active_registrations.contains(*consumer)
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if active_consumers.is_empty() {
            continue;
        }
        callbacks.insert(
            id.clone(),
            CallbackBinding {
                owner: declaration.declared_by.clone(),
                signature_hash: declaration.signature_hash,
                consumers: active_consumers,
            },
        );
    }
    let systems = catalogs
        .systems
        .iter()
        .filter(|(system, _)| active_registrations.contains(*system))
        .map(|(id, system)| {
            (
                id.clone(),
                SystemCallbackProjection {
                    callback: system.callback.clone(),
                    signature_hash: system.signature_hash,
                },
            )
        })
        .collect();
    let mut receipt = CallbackMapReceipt {
        schema_version: REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION,
        graph_hash,
        registration_semantic_hash,
        callbacks,
        systems,
        callback_map_hash: CanonicalHash::digest(b"unsealed-callback-receipt"),
    };
    receipt.callback_map_hash = receipt.recompute_hash()?;
    Ok(receipt)
}

#[derive(Serialize)]
struct RegistrationSemanticIdentity<'a> {
    graph_hash: CanonicalHash,
    composition_hash: CanonicalHash,
    namespace_grants: &'a BTreeSet<NamespaceGrant>,
    packages: BTreeMap<&'a PackageName, PackageSemanticIdentity<'a>>,
}

#[derive(Serialize)]
struct PackageSemanticIdentity<'a> {
    manifest_semantic_hash: CanonicalHash,
    schemas: &'a BTreeMap<SchemaId, SchemaDeclaration>,
    systems: &'a BTreeMap<StableId, SystemDeclaration>,
    callbacks: &'a BTreeMap<StableId, crate::CallbackDeclaration>,
    provided_capabilities: &'a BTreeSet<latticeaxiom_core::CapabilityId>,
    semantic_grants: &'a BTreeSet<crate::SemanticContributionGrant>,
}

fn registration_semantic_hash(
    input: &RegistrationCompileInput<'_>,
    _effective_grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
) -> Result<CanonicalHash, RegistrationCompileError> {
    let packages = input
        .packages
        .iter()
        .map(|(name, package)| {
            (
                name,
                PackageSemanticIdentity {
                    manifest_semantic_hash: package.manifest.semantic_hash,
                    schemas: &package.schemas,
                    systems: &package.systems,
                    callbacks: &package.callbacks,
                    provided_capabilities: &package.provided_capabilities,
                    semantic_grants: &package.semantic_grants,
                },
            )
        })
        .collect();
    Ok(canonical_json_hash(&RegistrationSemanticIdentity {
        graph_hash: input.graph.graph_hash,
        composition_hash: input.graph.composition_hash,
        namespace_grants: &input.graph.namespace_grants,
        packages,
    })?)
}

#[derive(Serialize)]
struct ManifestProvenanceIdentity<'a> {
    semantic_hash: CanonicalHash,
    producer: &'a latticeaxiom_compose::ManifestProducer,
    source_receipts: Vec<CanonicalHash>,
}

fn build_provenance_receipt(
    input: &RegistrationCompileInput<'_>,
    registration_semantic_hash: CanonicalHash,
) -> Result<RegistrationProvenanceReceipt, RegistrationCompileError> {
    let mut packages = BTreeMap::new();
    for (package, registration) in input.packages {
        let Some(locked) = input.graph.packages.get(package) else {
            return Err(RegistrationCompileError::UnexpectedPackageInput {
                package: package.clone(),
            });
        };
        let mut source_receipts = Vec::new();
        collect_fragment_provenance(&registration.manifest.fragment, &mut source_receipts)?;
        source_receipts.sort_unstable();
        let manifest_provenance_hash = canonical_json_hash(&ManifestProvenanceIdentity {
            semantic_hash: registration.manifest.semantic_hash,
            producer: &registration.manifest.producer,
            source_receipts,
        })?;
        packages.insert(
            package.clone(),
            PackageProvenanceReceipt {
                manifest_semantic_hash: locked.manifest_hash,
                manifest_provenance_hash,
                producer: registration.manifest.producer.clone(),
            },
        );
    }
    let mut receipt = RegistrationProvenanceReceipt {
        schema_version: REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION,
        graph_lock_hash: input.graph.lock_hash,
        composition_provenance_hash: input.graph.composition_provenance_hash,
        registration_semantic_hash,
        packages,
        provenance_hash: CanonicalHash::digest(b"unsealed-provenance-receipt"),
    };
    receipt.provenance_hash = receipt.recompute_hash()?;
    Ok(receipt)
}

fn collect_fragment_provenance(
    fragment: &RegistrationFragment,
    receipts: &mut Vec<CanonicalHash>,
) -> Result<(), RegistrationCompileError> {
    let mut registrations_by_id = fragment.registrations.iter().collect::<Vec<_>>();
    registrations_by_id.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    for registration in registrations_by_id {
        receipts.push(canonical_json_hash(&registration.provenance)?);
    }
    for semantic in &fragment.semantics {
        collect_semantic_provenance(semantic, receipts)?;
    }
    Ok(())
}

fn collect_semantic_provenance(
    semantic: &latticeaxiom_compose::SemanticFragment,
    receipts: &mut Vec<CanonicalHash>,
) -> Result<(), RegistrationCompileError> {
    match semantic {
        latticeaxiom_compose::SemanticFragment::TagContribution(contribution) => {
            receipts.push(canonical_json_hash(&contribution.provenance)?);
        }
        latticeaxiom_compose::SemanticFragment::MapContribution(contribution) => {
            receipts.push(canonical_json_hash(&contribution.provenance)?);
        }
        latticeaxiom_compose::SemanticFragment::Bundle(bundle) => {
            for nested in canonical_rows(&bundle.semantics)? {
                collect_semantic_provenance(nested, receipts)?;
            }
        }
        latticeaxiom_compose::SemanticFragment::TagDefinition(_)
        | latticeaxiom_compose::SemanticFragment::MapDefinition(_)
        | latticeaxiom_compose::SemanticFragment::StateProperty(_)
        | latticeaxiom_compose::SemanticFragment::Affordance(_)
        | latticeaxiom_compose::SemanticFragment::Role(_)
        | latticeaxiom_compose::SemanticFragment::RoleOffer(_) => {}
    }
    Ok(())
}
