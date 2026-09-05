//! Stable semantic validation, one-wave fallback, and numeric table compilation.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    AffordanceDefinition, AffordancePolicy, BundleActivation, ContentBundle, ContentPredicate,
    ContentRoleDefinition, PredicateExpression, RegistrationKind, RoleAuthority, RoleCardinality,
    RoleOffer, RoleOfferClass, SemanticCatalog, SemanticFragment, SemanticMapDefinition,
    SemanticMapMerger, SemanticTagDefinition, StatePropertyDefinition, TargetKind, TypedCondition,
    TypedSemanticId,
};
use latticeaxiom_core::{PackageName, SchemaId, StableId};
use serde::Serialize;
use serde_json::Value;

use crate::{
    CompiledSemanticImage, NumericMapTable, NumericTagTable, PackageRegistrationInput,
    RegistrationCompileError, RoleBindingReceipt, RoleSelectionRule, SchemaDeclaration,
    SemanticContributionGrant, SemanticGrantKind, SemanticResolutionStep,
};
use latticeaxiom_compose::NumericRegistrationId;

#[derive(Clone, Debug)]
struct OwnedFragment {
    owner: PackageName,
    fragment: SemanticFragment,
}

#[derive(Clone, Debug)]
struct OwnedOffer {
    owner: PackageName,
    offer: RoleOffer,
}

#[derive(Clone, Debug)]
struct OwnedBundle {
    owner: PackageName,
    bundle: ContentBundle,
}

#[derive(Clone, Debug)]
struct TagContract {
    owner: PackageName,
    definition: SemanticTagDefinition,
}

#[derive(Clone, Debug)]
struct MapContract {
    owner: PackageName,
    definition: SemanticMapDefinition,
}

#[derive(Clone, Debug)]
struct RoleContract {
    definition: ContentRoleDefinition,
}

#[derive(Clone, Copy, Debug, Default)]
struct OfferClasses {
    normal: bool,
    fallback: bool,
}

#[derive(Clone, Debug)]
struct SemanticState {
    catalog: SemanticCatalog,
    map_contracts: BTreeMap<TypedSemanticId, MapContract>,
    state_properties: BTreeMap<StableId, StatePropertyDefinition>,
    affordances: BTreeMap<StableId, AffordanceDefinition>,
    roles: BTreeMap<StableId, RoleContract>,
    offers: BTreeMap<StableId, BTreeMap<StableId, OfferClasses>>,
}

pub(crate) struct SemanticDraft {
    pub(crate) active_registrations: BTreeSet<StableId>,
    pub(crate) catalog: SemanticCatalog,
    pub(crate) map_contracts: BTreeMap<TypedSemanticId, SemanticMapDefinition>,
    pub(crate) state_properties: BTreeMap<StableId, StatePropertyDefinition>,
    pub(crate) affordances: BTreeMap<StableId, AffordanceDefinition>,
    pub(crate) roles: BTreeMap<StableId, ContentRoleDefinition>,
    pub(crate) active_bundles: BTreeSet<StableId>,
    pub(crate) role_bindings: BTreeMap<StableId, RoleBindingReceipt>,
    pub(crate) explanation: Vec<SemanticResolutionStep>,
}

pub(crate) struct SemanticCompileContext<'a> {
    pub(crate) registrations: &'a BTreeMap<StableId, (RegistrationKind, PackageName)>,
    pub(crate) schemas: &'a BTreeMap<SchemaId, SchemaDeclaration>,
    pub(crate) packages: &'a BTreeMap<PackageName, PackageRegistrationInput>,
    pub(crate) profile_bindings: &'a BTreeMap<StableId, StableId>,
    pub(crate) max_tag_definitions: usize,
    pub(crate) max_predicate_nodes: usize,
    pub(crate) max_predicate_nesting: usize,
}

// The one-wave transition remains visible as one ordered transaction.
#[allow(clippy::too_many_lines)]
pub(crate) fn compile_semantics(
    context: &SemanticCompileContext<'_>,
) -> Result<SemanticDraft, RegistrationCompileError> {
    let (base_fragments, base_offers, bundles) = collect_semantic_rows(context.packages)?;
    validate_semantic_grant_catalog(context, &base_fragments, &bundles)?;
    if bundles.len() > context.max_tag_definitions.saturating_mul(4) {
        return Err(RegistrationCompileError::LimitExceeded {
            limit: "content-bundles",
            observed: bundles.len(),
            maximum: context.max_tag_definitions.saturating_mul(4),
        });
    }

    let mut conditional = BTreeMap::<StableId, StableId>::new();
    let mut bundle_membership = BTreeMap::<StableId, StableId>::new();
    let mut active_bundles = BTreeSet::new();
    let mut active_registrations = context
        .registrations
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut unconditional_fragments = base_fragments;
    let mut unconditional_offers = base_offers;
    let mut fallback_bundles = BTreeMap::new();

    for (bundle_id, owned) in bundles {
        validate_bundle_registrations(context.registrations, &owned)?;
        for registration in &owned.bundle.registrations {
            if bundle_membership
                .insert(registration.clone(), bundle_id.clone())
                .is_some()
            {
                return Err(RegistrationCompileError::DuplicateFallbackRegistration {
                    registration: registration.clone(),
                });
            }
        }
        match &owned.bundle.activation {
            BundleActivation::Always => {
                active_bundles.insert(bundle_id);
                append_bundle_rows(
                    &owned,
                    &mut unconditional_fragments,
                    &mut unconditional_offers,
                    RoleOfferClass::Normal,
                )?;
            }
            BundleActivation::FallbackForMissingRoles(_) => {
                for registration in &owned.bundle.registrations {
                    if conditional
                        .insert(registration.clone(), bundle_id.clone())
                        .is_some()
                    {
                        return Err(RegistrationCompileError::DuplicateFallbackRegistration {
                            registration: registration.clone(),
                        });
                    }
                    active_registrations.remove(registration);
                }
                fallback_bundles.insert(bundle_id, owned);
            }
        }
    }

    let base = compile_rows(
        context,
        &unconditional_fragments,
        &unconditional_offers,
        &active_registrations,
    )?;
    validate_fallback_guards(&base, &fallback_bundles)?;
    for bundle in fallback_bundles.values() {
        let mut candidate_registrations = active_registrations.clone();
        candidate_registrations.extend(bundle.bundle.registrations.iter().cloned());
        let mut candidate_fragments = unconditional_fragments.clone();
        let mut candidate_offers = unconditional_offers.clone();
        append_bundle_rows(
            bundle,
            &mut candidate_fragments,
            &mut candidate_offers,
            RoleOfferClass::Fallback,
        )?;
        compile_rows(
            context,
            &candidate_fragments,
            &candidate_offers,
            &candidate_registrations,
        )?;
    }
    let (normal_bindings, missing, mut explanation) =
        resolve_roles(context, &base, RolePhase::Normal)?;

    let mut selected_fallbacks = BTreeSet::new();
    for role in &missing {
        let mut candidates = Vec::new();
        for (bundle_id, bundle) in &fallback_bundles {
            let BundleActivation::FallbackForMissingRoles(guard_roles) = &bundle.bundle.activation
            else {
                continue;
            };
            if !guard_roles.contains(role) {
                continue;
            }
            if let Some(invalid_guard) = guard_roles.iter().find(|guard| !missing.contains(*guard))
            {
                return Err(RegistrationCompileError::FallbackCycle {
                    bundle: bundle_id.clone(),
                    role: invalid_guard.clone(),
                });
            }
            let mut candidate_registrations = active_registrations.clone();
            candidate_registrations.extend(bundle.bundle.registrations.iter().cloned());
            let mut candidate_fragments = unconditional_fragments.clone();
            let mut candidate_offers = unconditional_offers.clone();
            append_bundle_rows(
                bundle,
                &mut candidate_fragments,
                &mut candidate_offers,
                RoleOfferClass::Fallback,
            )?;
            let candidate_state = compile_rows(
                context,
                &candidate_fragments,
                &candidate_offers,
                &candidate_registrations,
            )?;
            let mut satisfies_guards = true;
            for guard in guard_roles {
                if !bundle_satisfies_role(context, &candidate_state, bundle, guard)? {
                    satisfies_guards = false;
                    break;
                }
            }
            if satisfies_guards {
                candidates.push(bundle_id.clone());
            }
        }
        if candidates.is_empty() {
            if let Some(target) = context.profile_bindings.get(role) {
                return Err(RegistrationCompileError::BindingRejected {
                    role: role.clone(),
                    target: target.clone(),
                });
            }
            return Err(RegistrationCompileError::RoleUnsatisfied { role: role.clone() });
        }
        if candidates.len() > 1 {
            return Err(RegistrationCompileError::FallbackAmbiguous {
                role: role.clone(),
                bundles: candidates,
            });
        }
        selected_fallbacks.insert(candidates.remove(0));
    }

    let mut final_fragments = unconditional_fragments;
    let mut final_offers = unconditional_offers;
    for bundle_id in &selected_fallbacks {
        let Some(bundle) = fallback_bundles.get(bundle_id) else {
            return Err(RegistrationCompileError::FallbackCycle {
                bundle: bundle_id.clone(),
                role: bundle_id.clone(),
            });
        };
        active_registrations.extend(bundle.bundle.registrations.iter().cloned());
        append_bundle_rows(
            bundle,
            &mut final_fragments,
            &mut final_offers,
            RoleOfferClass::Fallback,
        )?;
        let roles = match &bundle.bundle.activation {
            BundleActivation::FallbackForMissingRoles(roles) => roles.iter().cloned().collect(),
            BundleActivation::Always => Vec::new(),
        };
        explanation.push(SemanticResolutionStep::FallbackActivated {
            bundle: bundle_id.clone(),
            roles,
        });
        active_bundles.insert(bundle_id.clone());
    }

    let final_state = compile_rows(
        context,
        &final_fragments,
        &final_offers,
        &active_registrations,
    )?;
    let (mut role_bindings, final_missing, final_explanation) =
        resolve_roles(context, &final_state, RolePhase::Final)?;
    if let Some(role) = final_missing.into_iter().next() {
        return Err(RegistrationCompileError::RoleUnsatisfied { role });
    }
    for (role, binding) in normal_bindings {
        role_bindings.entry(role).or_insert(binding);
    }
    explanation.extend(final_explanation);

    let mut catalog = final_state.catalog;
    catalog.role_bindings = role_bindings
        .iter()
        .map(|(role, binding)| (role.clone(), binding.targets.clone()))
        .collect();
    catalog.active_bundles.clone_from(&active_bundles);

    Ok(SemanticDraft {
        active_registrations,
        catalog,

        map_contracts: final_state
            .map_contracts
            .into_iter()
            .map(|(id, contract)| (id, contract.definition))
            .collect(),
        state_properties: final_state.state_properties,
        affordances: final_state.affordances,
        roles: final_state
            .roles
            .into_iter()
            .map(|(id, contract)| (id, contract.definition))
            .collect(),
        active_bundles,
        role_bindings,
        explanation,
    })
}

pub(crate) fn finalize_numeric_semantics(
    draft: &SemanticDraft,
    numeric_ids: &BTreeMap<StableId, NumericRegistrationId>,
    counts: &BTreeMap<RegistrationKind, usize>,
) -> Result<CompiledSemanticImage, RegistrationCompileError> {
    let mut tags = BTreeMap::new();
    for (tag, members) in &draft.catalog.tags {
        let Some(kind) = registration_kind_for_target(tag.target_kind) else {
            return Err(
                RegistrationCompileError::UnsupportedSemanticDefinitionTarget {
                    contract: tag.id.clone(),
                    target_kind: tag.target_kind,
                },
            );
        };
        let count = counts.get(&kind).copied().unwrap_or(0);
        let words = count.div_ceil(64);
        let mut bitset = vec![0_u64; words];
        for member in members {
            let Some(numeric) = numeric_ids.get(member) else {
                return Err(RegistrationCompileError::UnknownSemanticContract {
                    contract: member.clone(),
                });
            };
            let index = usize::try_from(numeric.0)
                .map_err(|_| RegistrationCompileError::NumericIdOverflow { kind })?;
            let word = index / 64;
            let bit = index % 64;
            let Some(slot) = bitset.get_mut(word) else {
                return Err(RegistrationCompileError::NumericIdOverflow { kind });
            };
            *slot |= 1_u64 << bit;
        }
        tags.insert(
            tag.clone(),
            NumericTagTable {
                target_kind: tag.target_kind,
                words: bitset,
            },
        );
    }

    let mut maps = BTreeMap::new();
    for (map, values) in &draft.catalog.maps {
        let Some(contract) = draft.map_contracts.get(map) else {
            return Err(RegistrationCompileError::UnknownSemanticContract {
                contract: map.id.clone(),
            });
        };
        let mut numeric_values = BTreeMap::new();
        for (target, value) in values {
            let Some(numeric) = numeric_ids.get(target) else {
                return Err(RegistrationCompileError::UnknownSemanticContract {
                    contract: target.clone(),
                });
            };
            numeric_values.insert(*numeric, value.clone());
        }
        maps.insert(
            map.clone(),
            NumericMapTable {
                target_kind: map.target_kind,
                value_schema: contract.value_schema.clone(),
                values: numeric_values,
            },
        );
    }

    let mut numeric_role_bindings = BTreeMap::new();
    for (role, binding) in &draft.role_bindings {
        let mut numeric = Vec::with_capacity(binding.targets.len());
        for target in &binding.targets {
            let Some(id) = numeric_ids.get(target) else {
                return Err(RegistrationCompileError::UnknownSemanticContract {
                    contract: target.clone(),
                });
            };
            numeric.push(*id);
        }
        numeric_role_bindings.insert(role.clone(), numeric);
    }

    let mut image = CompiledSemanticImage {
        tags,
        maps,
        state_properties: draft.state_properties.clone(),
        affordances: draft.affordances.clone(),
        roles: draft.roles.clone(),
        role_bindings: numeric_role_bindings,
        semantic_hash: latticeaxiom_core::CanonicalHash::digest(b"unsealed-semantic-image"),
    };
    image.semantic_hash = image.recompute_hash()?;
    Ok(image)
}

type CollectedSemanticRows = (
    Vec<OwnedFragment>,
    Vec<OwnedOffer>,
    BTreeMap<StableId, OwnedBundle>,
);

fn canonical_rows<T: Serialize>(rows: &[T]) -> Result<Vec<&T>, RegistrationCompileError> {
    let mut keyed = rows
        .iter()
        .map(|row| latticeaxiom_core::canonical_json_bytes(row).map(|bytes| (bytes, row)))
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    Ok(keyed.into_iter().map(|(_, row)| row).collect())
}

fn collect_semantic_rows(
    packages: &BTreeMap<PackageName, PackageRegistrationInput>,
) -> Result<CollectedSemanticRows, RegistrationCompileError> {
    let mut fragments = Vec::new();
    let mut offers = Vec::new();
    let mut bundles = BTreeMap::new();
    for (package, input) in packages {
        for fragment in canonical_rows(&input.manifest.fragment.semantics)? {
            match fragment {
                SemanticFragment::Bundle(bundle) => {
                    validate_semantic_id(&bundle.id, "content-bundle")?;
                    let owned = OwnedBundle {
                        owner: package.clone(),
                        bundle: bundle.clone(),
                    };
                    if bundles.insert(bundle.id.clone(), owned).is_some() {
                        return Err(RegistrationCompileError::DuplicateSemanticDefinition {
                            id: bundle.id.clone(),
                        });
                    }
                }
                SemanticFragment::RoleOffer(offer) => {
                    if offer.class != RoleOfferClass::Normal {
                        return Err(RegistrationCompileError::InvalidTopLevelRoleOffer {
                            role: offer.role.clone(),
                            target: offer.target.clone(),
                        });
                    }
                    offers.push(OwnedOffer {
                        owner: package.clone(),
                        offer: offer.clone(),
                    });
                }
                _ => fragments.push(OwnedFragment {
                    owner: package.clone(),
                    fragment: fragment.clone(),
                }),
            }
        }
    }
    Ok((fragments, offers, bundles))
}

fn validate_bundle_registrations(
    registrations: &BTreeMap<StableId, (RegistrationKind, PackageName)>,
    bundle: &OwnedBundle,
) -> Result<(), RegistrationCompileError> {
    for registration in &bundle.bundle.registrations {
        if matches!(
            &bundle.bundle.activation,
            BundleActivation::FallbackForMissingRoles(_)
        ) && registrations
            .get(registration)
            .is_some_and(|(kind, _)| *kind == RegistrationKind::Schema)
        {
            return Err(RegistrationCompileError::ConditionalSchemaRegistration {
                bundle: bundle.bundle.id.clone(),
                schema: registration.clone(),
            });
        }
        if registrations
            .get(registration)
            .is_none_or(|(_, owner)| owner != &bundle.owner)
        {
            return Err(RegistrationCompileError::InvalidBundleRegistration {
                bundle: bundle.bundle.id.clone(),
                package: bundle.owner.clone(),
                registration: registration.clone(),
            });
        }
    }
    for semantic in canonical_rows(&bundle.bundle.semantics)? {
        if let SemanticFragment::Bundle(nested) = semantic {
            return Err(RegistrationCompileError::NestedBundle {
                bundle: bundle.bundle.id.clone(),
                nested: nested.id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_fallback_guards(
    base: &SemanticState,
    bundles: &BTreeMap<StableId, OwnedBundle>,
) -> Result<(), RegistrationCompileError> {
    for (bundle_id, bundle) in bundles {
        let BundleActivation::FallbackForMissingRoles(guards) = &bundle.bundle.activation else {
            continue;
        };
        if guards.is_empty() {
            return Err(RegistrationCompileError::InvalidFallbackGuard {
                bundle: bundle_id.clone(),
                role: None,
            });
        }
        for guard in guards {
            let Some(role) = base.roles.get(guard) else {
                return Err(RegistrationCompileError::InvalidFallbackGuard {
                    bundle: bundle_id.clone(),
                    role: Some(guard.clone()),
                });
            };
            if role.definition.cardinality == RoleCardinality::ZeroOrOne {
                return Err(RegistrationCompileError::InvalidFallbackGuard {
                    bundle: bundle_id.clone(),
                    role: Some(guard.clone()),
                });
            }
        }
    }
    Ok(())
}

fn append_bundle_rows(
    bundle: &OwnedBundle,
    fragments: &mut Vec<OwnedFragment>,
    offers: &mut Vec<OwnedOffer>,
    expected_class: RoleOfferClass,
) -> Result<(), RegistrationCompileError> {
    for semantic in canonical_rows(&bundle.bundle.semantics)? {
        match semantic {
            SemanticFragment::RoleOffer(offer) => {
                validate_offer_class(&bundle.bundle.id, offer.class, expected_class)?;
                offers.push(OwnedOffer {
                    owner: bundle.owner.clone(),
                    offer: offer.clone(),
                });
            }
            SemanticFragment::Bundle(nested) => {
                return Err(RegistrationCompileError::NestedBundle {
                    bundle: bundle.bundle.id.clone(),
                    nested: nested.id.clone(),
                });
            }
            _ => fragments.push(OwnedFragment {
                owner: bundle.owner.clone(),
                fragment: semantic.clone(),
            }),
        }
    }
    for offer in canonical_rows(&bundle.bundle.role_offers)? {
        validate_offer_class(&bundle.bundle.id, offer.class, expected_class)?;
        offers.push(OwnedOffer {
            owner: bundle.owner.clone(),
            offer: offer.clone(),
        });
    }
    Ok(())
}

fn validate_offer_class(
    bundle: &StableId,
    actual: RoleOfferClass,
    expected: RoleOfferClass,
) -> Result<(), RegistrationCompileError> {
    if actual == expected {
        Ok(())
    } else {
        Err(RegistrationCompileError::InvalidRoleOfferClass {
            bundle: bundle.clone(),
            expected,
            actual,
        })
    }
}

// Exhaustive DTO variants are validated in two explicit passes.
#[allow(clippy::too_many_lines)]
fn compile_rows(
    context: &SemanticCompileContext<'_>,
    fragments: &[OwnedFragment],
    offers: &[OwnedOffer],
    active_registrations: &BTreeSet<StableId>,
) -> Result<SemanticState, RegistrationCompileError> {
    let mut semantic_ids = BTreeMap::<StableId, &'static str>::new();
    let mut tag_contracts = BTreeMap::new();
    let mut map_contracts = BTreeMap::new();
    let mut state_properties = BTreeMap::new();
    let mut affordances = BTreeMap::new();
    let mut roles = BTreeMap::new();

    for owned in fragments {
        match &owned.fragment {
            SemanticFragment::TagDefinition(definition) => {
                register_target_semantic_id(
                    &mut semantic_ids,
                    &definition.tag.id,
                    "tag",
                    definition.tag.target_kind,
                    "tag",
                )?;
                if tag_contracts
                    .insert(
                        definition.tag.clone(),
                        TagContract {
                            owner: owned.owner.clone(),
                            definition: definition.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(RegistrationCompileError::DuplicateSemanticDefinition {
                        id: definition.tag.id.clone(),
                    });
                }
            }
            SemanticFragment::MapDefinition(definition) => {
                register_target_semantic_id(
                    &mut semantic_ids,
                    &definition.map.id,
                    "map",
                    definition.map.target_kind,
                    "map",
                )?;
                require_value_schema(context, &definition.map.id, &definition.value_schema)?;
                if definition.merger != SemanticMapMerger::Conflict {
                    return Err(RegistrationCompileError::MapConflict {
                        map: definition.map.id.clone(),
                        target: definition.map.id.clone(),
                    });
                }
                if map_contracts
                    .insert(
                        definition.map.clone(),
                        MapContract {
                            owner: owned.owner.clone(),
                            definition: definition.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(RegistrationCompileError::DuplicateSemanticDefinition {
                        id: definition.map.id.clone(),
                    });
                }
            }
            SemanticFragment::StateProperty(definition) => {
                if definition.target_kind != TargetKind::Block {
                    return Err(
                        RegistrationCompileError::UnsupportedSemanticDefinitionTarget {
                            contract: definition.id.clone(),
                            target_kind: definition.target_kind,
                        },
                    );
                }
                register_semantic_id(
                    &mut semantic_ids,
                    &definition.id,
                    "state-property",
                    "block-state",
                )?;
                let schema =
                    require_value_schema(context, &definition.id, &definition.value_schema)?;
                if definition.allowed_values.is_empty() {
                    return Err(RegistrationCompileError::LimitExceeded {
                        limit: "state-property-allowed-values",
                        observed: 0,
                        maximum: 1,
                    });
                }
                let mut values = BTreeSet::new();
                for value in &definition.allowed_values {
                    schema.validate_value(value)?;
                    let bytes = latticeaxiom_core::canonical_json_bytes(value)?;
                    if !values.insert(bytes) {
                        return Err(RegistrationCompileError::DuplicateSemanticDefinition {
                            id: definition.id.clone(),
                        });
                    }
                }
                state_properties.insert(definition.id.clone(), definition.clone());
            }
            SemanticFragment::Affordance(definition) => {
                register_target_semantic_id(
                    &mut semantic_ids,
                    &definition.id,
                    "affordance",
                    definition.target_kind,
                    "affordance",
                )?;
                require_schema(context, &definition.id, &definition.request_schema)?;
                require_schema(context, &definition.id, &definition.response_schema)?;
                if definition.policy != AffordancePolicy::StaticData
                    || definition.callback.is_some()
                {
                    return Err(RegistrationCompileError::AffordanceCallbackDeferred {
                        affordance: definition.id.clone(),
                    });
                }
                affordances.insert(definition.id.clone(), definition.clone());
            }
            SemanticFragment::Role(definition) => {
                register_target_semantic_id(
                    &mut semantic_ids,
                    &definition.id,
                    "role",
                    definition.accepts.target_kind,
                    "role",
                )?;
                roles.insert(
                    definition.id.clone(),
                    RoleContract {
                        definition: definition.clone(),
                    },
                );
            }
            SemanticFragment::TagContribution(_)
            | SemanticFragment::MapContribution(_)
            | SemanticFragment::RoleOffer(_) => {}
            SemanticFragment::Bundle(bundle) => {
                return Err(RegistrationCompileError::NestedBundle {
                    bundle: bundle.id.clone(),
                    nested: bundle.id.clone(),
                });
            }
        }
    }

    if tag_contracts.len() > context.max_tag_definitions {
        return Err(RegistrationCompileError::LimitExceeded {
            limit: "tag-definitions",
            observed: tag_contracts.len(),
            maximum: context.max_tag_definitions,
        });
    }
    let mut catalog = SemanticCatalog::default();
    for tag in tag_contracts.keys() {
        catalog.tags.insert(tag.clone(), BTreeSet::new());
    }
    for map in map_contracts.keys() {
        catalog.maps.insert(map.clone(), BTreeMap::new());
    }

    for owned in fragments {
        match &owned.fragment {
            SemanticFragment::TagContribution(contribution) => {
                verify_fragment_owner(
                    "tag-contribution",
                    &contribution.tag.id,
                    &contribution.declared_by,
                    &owned.owner,
                )?;
                let Some(contract) = tag_contracts.get(&contribution.tag) else {
                    return Err(RegistrationCompileError::UnknownSemanticContract {
                        contract: contribution.tag.id.clone(),
                    });
                };
                validate_contribution_authority(
                    context,
                    SemanticGrantKind::Tag,
                    &contribution.tag,
                    &contract.owner,
                    contract.definition.extensible,
                    &owned.owner,
                    &contribution.target,
                    active_registrations,
                )?;
                validate_target_kind(
                    context,
                    &contribution.tag.id,
                    &contribution.target,
                    contribution.tag.target_kind,
                    active_registrations,
                )?;
                if let Some(members) = catalog.tags.get_mut(&contribution.tag) {
                    members.insert(contribution.target.clone());
                }
            }
            SemanticFragment::MapContribution(contribution) => {
                verify_fragment_owner(
                    "map-contribution",
                    &contribution.map.id,
                    &contribution.declared_by,
                    &owned.owner,
                )?;
                let Some(contract) = map_contracts.get(&contribution.map) else {
                    return Err(RegistrationCompileError::UnknownSemanticContract {
                        contract: contribution.map.id.clone(),
                    });
                };
                validate_contribution_authority(
                    context,
                    SemanticGrantKind::Map,
                    &contribution.map,
                    &contract.owner,
                    contract.definition.extensible,
                    &owned.owner,
                    &contribution.target,
                    active_registrations,
                )?;
                validate_target_kind(
                    context,
                    &contribution.map.id,
                    &contribution.target,
                    contribution.map.target_kind,
                    active_registrations,
                )?;
                let schema = require_value_schema(
                    context,
                    &contribution.map.id,
                    &contract.definition.value_schema,
                )?;
                schema.validate_value(&contribution.value)?;
                if let Some(values) = catalog.maps.get_mut(&contribution.map) {
                    if let Some(existing) = values.get(&contribution.target)
                        && existing != &contribution.value
                    {
                        return Err(RegistrationCompileError::MapConflict {
                            map: contribution.map.id.clone(),
                            target: contribution.target.clone(),
                        });
                    }
                    values.insert(contribution.target.clone(), contribution.value.clone());
                }
            }
            _ => {}
        }
    }

    let mut compiled_offers = BTreeMap::<StableId, BTreeMap<StableId, OfferClasses>>::new();
    for owned in offers {
        let Some(role) = roles.get(&owned.offer.role) else {
            return Err(RegistrationCompileError::UnknownSemanticContract {
                contract: owned.offer.role.clone(),
            });
        };
        validate_target_kind(
            context,
            &owned.offer.role,
            &owned.offer.target,
            role.definition.accepts.target_kind,
            active_registrations,
        )?;
        let Some((_, target_owner)) = context.registrations.get(&owned.offer.target) else {
            return Err(RegistrationCompileError::UnknownSemanticContract {
                contract: owned.offer.target.clone(),
            });
        };
        if target_owner != &owned.owner {
            return Err(RegistrationCompileError::ForeignRoleOffer {
                role: owned.offer.role.clone(),
                target: owned.offer.target.clone(),
                package: owned.owner.clone(),
            });
        }
        let matches = predicate_matches(
            context,
            &catalog,
            &map_contracts,
            &state_properties,
            &affordances,
            &owned.offer.role,
            &role.definition.accepts,
            &owned.offer.target,
            active_registrations,
        )?;
        if !matches {
            return Err(RegistrationCompileError::BindingRejected {
                role: owned.offer.role.clone(),
                target: owned.offer.target.clone(),
            });
        }
        let classes = compiled_offers
            .entry(owned.offer.role.clone())
            .or_default()
            .entry(owned.offer.target.clone())
            .or_default();
        match owned.offer.class {
            RoleOfferClass::Normal => classes.normal = true,
            RoleOfferClass::Fallback => classes.fallback = true,
        }
    }

    Ok(SemanticState {
        catalog,
        map_contracts,
        state_properties,
        affordances,
        roles,
        offers: compiled_offers,
    })
}

fn register_target_semantic_id(
    ids: &mut BTreeMap<StableId, &'static str>,
    id: &StableId,
    catalog: &'static str,
    target_kind: TargetKind,
    family: &'static str,
) -> Result<(), RegistrationCompileError> {
    let Some(target) = semantic_target_name(target_kind) else {
        return Err(
            RegistrationCompileError::UnsupportedSemanticDefinitionTarget {
                contract: id.clone(),
                target_kind,
            },
        );
    };
    let expected_kind = format!("{target}-{family}");
    register_semantic_id(ids, id, catalog, &expected_kind)
}

fn register_semantic_id(
    ids: &mut BTreeMap<StableId, &'static str>,
    id: &StableId,
    catalog: &'static str,
    expected_kind: &str,
) -> Result<(), RegistrationCompileError> {
    validate_semantic_id(id, expected_kind)?;
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

fn validate_semantic_id(
    id: &StableId,
    expected_kind: &str,
) -> Result<(), RegistrationCompileError> {
    if id.kind() == expected_kind && id.major().is_some() {
        Ok(())
    } else {
        Err(RegistrationCompileError::InvalidSemanticId {
            id: id.clone(),
            expected_kind: expected_kind.to_owned(),
        })
    }
}

const fn semantic_target_name(target: TargetKind) -> Option<&'static str> {
    match target {
        TargetKind::Block => Some("block"),
        TargetKind::Item => Some("item"),
        TargetKind::Fluid => Some("fluid"),
        TargetKind::Biome => Some("biome"),
        TargetKind::Dimension => Some("dimension"),
        TargetKind::Entity => None,
    }
}

fn verify_fragment_owner(
    catalog: &'static str,
    id: &StableId,
    declared_by: &PackageName,
    owner: &PackageName,
) -> Result<(), RegistrationCompileError> {
    if declared_by == owner {
        Ok(())
    } else {
        Err(RegistrationCompileError::ForeignOwner {
            catalog,
            id: id.clone(),
            declared_by: declared_by.clone(),
            package: owner.clone(),
        })
    }
}

fn require_schema<'a>(
    context: &'a SemanticCompileContext<'_>,
    consumer: &StableId,
    schema: &SchemaId,
) -> Result<&'a SchemaDeclaration, RegistrationCompileError> {
    context
        .schemas
        .get(schema)
        .ok_or_else(|| RegistrationCompileError::UndeclaredSchema {
            consumer: consumer.clone(),
            schema: schema.clone(),
        })
}

fn require_value_schema<'a>(
    context: &'a SemanticCompileContext<'_>,
    consumer: &StableId,
    schema: &SchemaId,
) -> Result<&'a latticeaxiom_compose::ValueType, RegistrationCompileError> {
    let declaration = require_schema(context, consumer, schema)?;
    declaration
        .value_type
        .as_ref()
        .ok_or_else(|| RegistrationCompileError::UndeclaredSchema {
            consumer: consumer.clone(),
            schema: schema.clone(),
        })
}

fn validate_semantic_grant_catalog(
    context: &SemanticCompileContext<'_>,
    base_fragments: &[OwnedFragment],
    bundles: &BTreeMap<StableId, OwnedBundle>,
) -> Result<(), RegistrationCompileError> {
    let mut contracts = BTreeMap::<TypedSemanticId, (SemanticGrantKind, PackageName)>::new();
    for owned in base_fragments {
        register_semantic_grant_contract(&mut contracts, &owned.owner, &owned.fragment)?;
    }
    for bundle in bundles.values() {
        for fragment in canonical_rows(&bundle.bundle.semantics)? {
            register_semantic_grant_contract(&mut contracts, &bundle.owner, fragment)?;
        }
    }

    for (package, input) in context.packages {
        for grant in &input.semantic_grants {
            let Some((kind, owner)) = contracts.get(&grant.contract) else {
                return Err(RegistrationCompileError::UnknownSemanticContract {
                    contract: grant.contract.id.clone(),
                });
            };
            if *kind != grant.kind || owner != package {
                return Err(RegistrationCompileError::UnauthorizedSemanticContribution {
                    contract: grant.contract.id.clone(),
                    contributor: package.clone(),
                    target: grant.contract.id.clone(),
                });
            }
            if !context.packages.contains_key(&grant.grantee) {
                return Err(RegistrationCompileError::UnexpectedPackageInput {
                    package: grant.grantee.clone(),
                });
            }
        }
    }
    Ok(())
}

fn register_semantic_grant_contract(
    contracts: &mut BTreeMap<TypedSemanticId, (SemanticGrantKind, PackageName)>,
    owner: &PackageName,
    fragment: &SemanticFragment,
) -> Result<(), RegistrationCompileError> {
    let contract = match fragment {
        SemanticFragment::TagDefinition(definition) => {
            Some((definition.tag.clone(), SemanticGrantKind::Tag))
        }
        SemanticFragment::MapDefinition(definition) => {
            Some((definition.map.clone(), SemanticGrantKind::Map))
        }
        SemanticFragment::TagContribution(_)
        | SemanticFragment::MapContribution(_)
        | SemanticFragment::StateProperty(_)
        | SemanticFragment::Affordance(_)
        | SemanticFragment::Role(_)
        | SemanticFragment::RoleOffer(_)
        | SemanticFragment::Bundle(_) => None,
    };
    let Some((contract, kind)) = contract else {
        return Ok(());
    };
    if contracts
        .insert(contract.clone(), (kind, owner.clone()))
        .is_some()
    {
        return Err(RegistrationCompileError::DuplicateSemanticDefinition { id: contract.id });
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn validate_contribution_authority(
    context: &SemanticCompileContext<'_>,
    kind: SemanticGrantKind,
    contract: &TypedSemanticId,
    contract_owner: &PackageName,
    extensible: bool,
    contributor: &PackageName,
    target: &StableId,
    active_registrations: &BTreeSet<StableId>,
) -> Result<(), RegistrationCompileError> {
    let Some((_, target_owner)) = context.registrations.get(target) else {
        return Err(RegistrationCompileError::UnknownSemanticContract {
            contract: target.clone(),
        });
    };
    if !active_registrations.contains(target) || target_owner != contributor {
        return Err(RegistrationCompileError::UnauthorizedSemanticContribution {
            contract: contract.id.clone(),
            contributor: contributor.clone(),
            target: target.clone(),
        });
    }
    if contributor == contract_owner {
        return Ok(());
    }
    let authorized = extensible
        && context.packages.get(contract_owner).is_some_and(|input| {
            input.semantic_grants.contains(&SemanticContributionGrant {
                kind,
                contract: contract.clone(),
                grantee: contributor.clone(),
            })
        });
    if authorized {
        Ok(())
    } else {
        Err(RegistrationCompileError::UnauthorizedSemanticContribution {
            contract: contract.id.clone(),
            contributor: contributor.clone(),
            target: target.clone(),
        })
    }
}

fn validate_target_kind(
    context: &SemanticCompileContext<'_>,
    contract: &StableId,
    target: &StableId,
    expected: TargetKind,
    active_registrations: &BTreeSet<StableId>,
) -> Result<(), RegistrationCompileError> {
    if !active_registrations.contains(target) {
        return Err(RegistrationCompileError::UnknownSemanticContract {
            contract: target.clone(),
        });
    }
    let Some((kind, _)) = context.registrations.get(target) else {
        return Err(RegistrationCompileError::UnknownSemanticContract {
            contract: target.clone(),
        });
    };
    let actual = target_kind_for_registration(*kind).ok_or_else(|| {
        RegistrationCompileError::UnsupportedSemanticTarget {
            target: target.clone(),
            kind: *kind,
        }
    })?;
    if actual == expected {
        Ok(())
    } else {
        Err(RegistrationCompileError::SemanticTargetKindMismatch {
            contract: contract.clone(),
            target: target.clone(),
            expected,
            actual,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RolePhase {
    Normal,
    Final,
}

type RoleResolution = (
    BTreeMap<StableId, RoleBindingReceipt>,
    BTreeSet<StableId>,
    Vec<SemanticResolutionStep>,
);
// The cardinality and authority matrix is kept together for auditability.
#[allow(clippy::too_many_lines)]
fn resolve_roles(
    context: &SemanticCompileContext<'_>,
    state: &SemanticState,
    phase: RolePhase,
) -> Result<RoleResolution, RegistrationCompileError> {
    for (role, target) in context.profile_bindings {
        if !state.roles.contains_key(role) {
            return Err(RegistrationCompileError::BindingRejected {
                role: role.clone(),
                target: target.clone(),
            });
        }
    }

    let mut bindings = BTreeMap::new();
    let mut missing = BTreeSet::new();
    let mut explanation = Vec::new();
    for (role_id, role) in &state.roles {
        let candidates = role_candidates(state, role_id, phase);
        explanation.push(SemanticResolutionStep::Candidates {
            role: role_id.clone(),
            targets: candidates.clone(),
        });

        let selection = if let Some(target) = context.profile_bindings.get(role_id) {
            if role.definition.authority != RoleAuthority::Profile {
                return Err(RegistrationCompileError::BindingRejected {
                    role: role_id.clone(),
                    target: target.clone(),
                });
            }
            if candidates.contains(target) {
                Some((vec![target.clone()], RoleSelectionRule::Profile))
            } else if phase == RolePhase::Normal {
                None
            } else {
                return Err(RegistrationCompileError::BindingRejected {
                    role: role_id.clone(),
                    target: target.clone(),
                });
            }
        } else {
            match role.definition.cardinality {
                RoleCardinality::ExactlyOne => match candidates.as_slice() {
                    [] => None,
                    [target] => Some((
                        vec![target.clone()],
                        candidate_rule(state, role_id, target, phase),
                    )),
                    _ => {
                        return Err(RegistrationCompileError::RoleAmbiguous {
                            role: role_id.clone(),
                            candidates,
                        });
                    }
                },
                RoleCardinality::ZeroOrOne => match candidates.as_slice() {
                    [] => Some((Vec::new(), RoleSelectionRule::Unique)),
                    [target] => Some((
                        vec![target.clone()],
                        candidate_rule(state, role_id, target, phase),
                    )),
                    _ => {
                        return Err(RegistrationCompileError::RoleAmbiguous {
                            role: role_id.clone(),
                            candidates,
                        });
                    }
                },
                RoleCardinality::OneOrMore => {
                    if candidates.is_empty() {
                        None
                    } else {
                        let rule = if phase == RolePhase::Final
                            && candidates.iter().any(|target| {
                                state
                                    .offers
                                    .get(role_id)
                                    .and_then(|offers| offers.get(target))
                                    .is_some_and(|classes| classes.fallback && !classes.normal)
                            }) {
                            RoleSelectionRule::Fallback
                        } else {
                            RoleSelectionRule::Unique
                        };
                        Some((candidates, rule))
                    }
                }
            }
        };

        if let Some((targets, selected_by)) = selection {
            explanation.push(SemanticResolutionStep::Bound {
                role: role_id.clone(),
                targets: targets.clone(),
                selected_by,
            });
            bindings.insert(
                role_id.clone(),
                RoleBindingReceipt {
                    targets,
                    selected_by,
                },
            );
        } else {
            explanation.push(SemanticResolutionStep::Missing {
                role: role_id.clone(),
            });
            missing.insert(role_id.clone());
        }
    }
    Ok((bindings, missing, explanation))
}

fn role_candidates(state: &SemanticState, role: &StableId, phase: RolePhase) -> Vec<StableId> {
    state
        .offers
        .get(role)
        .into_iter()
        .flat_map(BTreeMap::iter)
        .filter(|(_, classes)| match phase {
            RolePhase::Normal => classes.normal,
            RolePhase::Final => classes.normal || classes.fallback,
        })
        .map(|(target, _)| target.clone())
        .collect()
}

fn candidate_rule(
    state: &SemanticState,
    role: &StableId,
    target: &StableId,
    phase: RolePhase,
) -> RoleSelectionRule {
    if phase == RolePhase::Final
        && state
            .offers
            .get(role)
            .and_then(|offers| offers.get(target))
            .is_some_and(|classes| classes.fallback && !classes.normal)
    {
        RoleSelectionRule::Fallback
    } else {
        RoleSelectionRule::Unique
    }
}

fn bundle_satisfies_role(
    context: &SemanticCompileContext<'_>,
    state: &SemanticState,
    bundle: &OwnedBundle,
    role: &StableId,
) -> Result<bool, RegistrationCompileError> {
    let Some(role_contract) = state.roles.get(role) else {
        return Err(RegistrationCompileError::UnknownSemanticContract {
            contract: role.clone(),
        });
    };
    let mut targets = bundle
        .bundle
        .role_offers
        .iter()
        .chain(bundle.bundle.semantics.iter().filter_map(|fragment| {
            if let SemanticFragment::RoleOffer(offer) = fragment {
                Some(offer)
            } else {
                None
            }
        }))
        .filter(|offer| offer.role == *role && offer.class == RoleOfferClass::Fallback)
        .map(|offer| offer.target.clone())
        .collect::<BTreeSet<_>>();
    if let Some(profile_target) = context.profile_bindings.get(role) {
        targets.retain(|target| target == profile_target);
    }
    for target in targets {
        if predicate_matches(
            context,
            &state.catalog,
            &state.map_contracts,
            &state.state_properties,
            &state.affordances,
            role,
            &role_contract.definition.accepts,
            &target,
            &bundle.bundle.registrations,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn predicate_matches(
    context: &SemanticCompileContext<'_>,
    catalog: &SemanticCatalog,
    maps: &BTreeMap<TypedSemanticId, MapContract>,
    state_properties: &BTreeMap<StableId, StatePropertyDefinition>,
    affordances: &BTreeMap<StableId, AffordanceDefinition>,
    role: &StableId,
    predicate: &ContentPredicate,
    target: &StableId,
    active_registrations: &BTreeSet<StableId>,
) -> Result<bool, RegistrationCompileError> {
    validate_target_kind(
        context,
        role,
        target,
        predicate.target_kind,
        active_registrations,
    )?;
    let mut nodes = 0_usize;
    validate_predicate(
        context,
        catalog,
        maps,
        state_properties,
        affordances,
        role,
        predicate.target_kind,
        &predicate.expression,
        1,
        &mut nodes,
    )?;
    Ok(evaluate_predicate(catalog, &predicate.expression, target))
}

#[allow(clippy::too_many_arguments)]
// Exhaustive predicate typing stays colocated with its variants.
#[allow(clippy::too_many_lines)]
fn validate_predicate(
    context: &SemanticCompileContext<'_>,
    catalog: &SemanticCatalog,
    maps: &BTreeMap<TypedSemanticId, MapContract>,
    state_properties: &BTreeMap<StableId, StatePropertyDefinition>,
    affordances: &BTreeMap<StableId, AffordanceDefinition>,
    role: &StableId,
    target_kind: TargetKind,
    expression: &PredicateExpression,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), RegistrationCompileError> {
    if depth > context.max_predicate_nesting {
        return Err(RegistrationCompileError::LimitExceeded {
            limit: "predicate-nesting",
            observed: depth,
            maximum: context.max_predicate_nesting,
        });
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or(RegistrationCompileError::LimitExceeded {
            limit: "predicate-nodes",
            observed: usize::MAX,
            maximum: context.max_predicate_nodes,
        })?;
    if *nodes > context.max_predicate_nodes {
        return Err(RegistrationCompileError::LimitExceeded {
            limit: "predicate-nodes",
            observed: *nodes,
            maximum: context.max_predicate_nodes,
        });
    }

    match expression {
        PredicateExpression::Exact { target } => {
            let Some((kind, _)) = context.registrations.get(target) else {
                return Err(RegistrationCompileError::UnknownSemanticContract {
                    contract: target.clone(),
                });
            };
            let actual = target_kind_for_registration(*kind).ok_or_else(|| {
                RegistrationCompileError::UnsupportedSemanticTarget {
                    target: target.clone(),
                    kind: *kind,
                }
            })?;
            if actual != target_kind {
                return Err(RegistrationCompileError::SemanticTargetKindMismatch {
                    contract: role.clone(),
                    target: target.clone(),
                    expected: target_kind,
                    actual,
                });
            }
        }
        PredicateExpression::InTag { tag } => {
            if tag.target_kind != target_kind {
                return Err(RegistrationCompileError::SemanticTargetKindMismatch {
                    contract: role.clone(),
                    target: tag.id.clone(),
                    expected: target_kind,
                    actual: tag.target_kind,
                });
            }
            if !catalog.tags.contains_key(tag) {
                return Err(RegistrationCompileError::UnknownSemanticContract {
                    contract: tag.id.clone(),
                });
            }
        }
        PredicateExpression::MapPresent { map } => {
            validate_map_predicate(role, target_kind, map, maps)?;
        }
        PredicateExpression::MapMatches { map, condition } => {
            let contract = validate_map_predicate(role, target_kind, map, maps)?;
            let schema = require_value_schema(context, &map.id, &contract.definition.value_schema)?;
            validate_condition(schema, condition)?;
        }
        PredicateExpression::StateMatches {
            property,
            condition,
        } => {
            let Some(definition) = state_properties.get(property) else {
                return Err(RegistrationCompileError::UnknownSemanticContract {
                    contract: property.clone(),
                });
            };
            if definition.target_kind != target_kind {
                return Err(RegistrationCompileError::SemanticTargetKindMismatch {
                    contract: role.clone(),
                    target: property.clone(),
                    expected: target_kind,
                    actual: definition.target_kind,
                });
            }
            let schema = require_value_schema(context, property, &definition.value_schema)?;
            validate_condition(schema, condition)?;
            return Err(RegistrationCompileError::ContextualRolePredicate {
                role: role.clone(),
                operation: "state-matches",
            });
        }
        PredicateExpression::Supports { affordance, .. } => {
            let Some(definition) = affordances.get(affordance) else {
                return Err(RegistrationCompileError::UnknownSemanticContract {
                    contract: affordance.clone(),
                });
            };
            if definition.target_kind != target_kind {
                return Err(RegistrationCompileError::SemanticTargetKindMismatch {
                    contract: role.clone(),
                    target: affordance.clone(),
                    expected: target_kind,
                    actual: definition.target_kind,
                });
            }
            return Err(RegistrationCompileError::ContextualRolePredicate {
                role: role.clone(),
                operation: "supports",
            });
        }
        PredicateExpression::All { predicates } | PredicateExpression::Any { predicates } => {
            for predicate in predicates {
                validate_predicate(
                    context,
                    catalog,
                    maps,
                    state_properties,
                    affordances,
                    role,
                    target_kind,
                    predicate,
                    depth.saturating_add(1),
                    nodes,
                )?;
            }
        }
        PredicateExpression::Not { predicate } => {
            validate_predicate(
                context,
                catalog,
                maps,
                state_properties,
                affordances,
                role,
                target_kind,
                predicate,
                depth.saturating_add(1),
                nodes,
            )?;
        }
    }
    Ok(())
}

fn validate_map_predicate<'a>(
    role: &StableId,
    target_kind: TargetKind,
    map: &TypedSemanticId,
    maps: &'a BTreeMap<TypedSemanticId, MapContract>,
) -> Result<&'a MapContract, RegistrationCompileError> {
    if map.target_kind != target_kind {
        return Err(RegistrationCompileError::SemanticTargetKindMismatch {
            contract: role.clone(),
            target: map.id.clone(),
            expected: target_kind,
            actual: map.target_kind,
        });
    }
    maps.get(map)
        .ok_or_else(|| RegistrationCompileError::UnknownSemanticContract {
            contract: map.id.clone(),
        })
}

fn validate_condition(
    schema: &latticeaxiom_compose::ValueType,
    condition: &TypedCondition,
) -> Result<(), RegistrationCompileError> {
    match condition {
        TypedCondition::Equal(value) => schema.validate_value(value)?,
        TypedCondition::In(values) => {
            if values.is_empty() {
                return Err(RegistrationCompileError::LimitExceeded {
                    limit: "predicate-condition-values",
                    observed: 0,
                    maximum: 1,
                });
            }
            for value in values {
                schema.validate_value(value)?;
            }
        }
    }
    Ok(())
}

fn evaluate_predicate(
    catalog: &SemanticCatalog,
    expression: &PredicateExpression,
    target: &StableId,
) -> bool {
    match expression {
        PredicateExpression::Exact { target: exact } => exact == target,
        PredicateExpression::InTag { tag } => catalog
            .tags
            .get(tag)
            .is_some_and(|members| members.contains(target)),
        PredicateExpression::MapPresent { map } => catalog
            .maps
            .get(map)
            .is_some_and(|values| values.contains_key(target)),
        PredicateExpression::MapMatches { map, condition } => catalog
            .maps
            .get(map)
            .and_then(|values| values.get(target))
            .is_some_and(|value| condition_matches(condition, value)),
        PredicateExpression::StateMatches { .. } | PredicateExpression::Supports { .. } => false,
        PredicateExpression::All { predicates } => predicates
            .iter()
            .all(|predicate| evaluate_predicate(catalog, predicate, target)),
        PredicateExpression::Any { predicates } => predicates
            .iter()
            .any(|predicate| evaluate_predicate(catalog, predicate, target)),
        PredicateExpression::Not { predicate } => !evaluate_predicate(catalog, predicate, target),
    }
}

fn condition_matches(condition: &TypedCondition, value: &Value) -> bool {
    match condition {
        TypedCondition::Equal(expected) => expected == value,
        TypedCondition::In(expected) => expected.contains(value),
    }
}

fn target_kind_for_registration(kind: RegistrationKind) -> Option<TargetKind> {
    match kind {
        RegistrationKind::Dimension => Some(TargetKind::Dimension),
        RegistrationKind::Block => Some(TargetKind::Block),
        RegistrationKind::Item => Some(TargetKind::Item),
        RegistrationKind::Fluid => Some(TargetKind::Fluid),
        RegistrationKind::Biome => Some(TargetKind::Biome),
        RegistrationKind::Component
        | RegistrationKind::Message
        | RegistrationKind::System
        | RegistrationKind::Asset
        | RegistrationKind::Capability
        | RegistrationKind::Schema
        | RegistrationKind::Render => None,
    }
}

const fn registration_kind_for_target(kind: TargetKind) -> Option<RegistrationKind> {
    match kind {
        TargetKind::Dimension => Some(RegistrationKind::Dimension),
        TargetKind::Block => Some(RegistrationKind::Block),
        TargetKind::Item => Some(RegistrationKind::Item),
        TargetKind::Fluid => Some(RegistrationKind::Fluid),
        TargetKind::Biome => Some(RegistrationKind::Biome),
        TargetKind::Entity => None,
    }
}
