//! Deterministic bounded package-graph resolver.

use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    CapabilityCardinality, CapabilityProvision, CapabilityRequirement, CompositionSpec,
    PackageDomain, PackageSpec, RealizationPreference, RealizationSpec,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalLogicalPath, CapabilityId, NamespaceGrant, NamespaceGrantPattern,
    NamespaceGrantor, PackageName, PackageVersionReq, RegistrationNamespace, SourceId,
    canonical_json_hash, provenance_hash,
};

use crate::error::{ResolutionError, ResolutionFailureContext};
use crate::host::HostCompatibilityV1;
use crate::model::{
    BUILD_INTENT_SCHEMA_VERSION, BacktrackingFailureV1, BuildIntentV1, CandidateIdentityV1,
    CapabilityDemandReceiptV1, CapabilityProviderReceiptV1, CapabilityResolutionReceiptV1,
    PackageBuildIntentV1, PackageCandidate, PackageResolutionV1, PreBuildSurface,
    RESOLUTION_RECEIPT_SCHEMA_VERSION, ResolutionBudget, ResolutionDecisionV1,
    ResolutionExplanationV1, ResolutionFailureCodeV1, ResolutionLimits, ResolutionOutcomeV1,
    ResolutionReasonV1, ResolutionReceiptV1, ResolutionSubjectV1, ResolvedDependencyV1,
    ResolvedPackageV1, ResolvedRealizationV1,
};

/// An immutable, in-memory catalog and deterministic local package resolver.
///
/// Acquisition and Nickel evaluation happen before construction. The catalog
/// may represent workspace paths, local directories, local prebuilts, or
/// in-memory fixtures without putting filesystem I/O in the solver.
#[derive(Clone, Debug)]
pub struct PackageResolver {
    candidates: BTreeMap<SourceId, PackageCandidate>,
    by_package: BTreeMap<PackageName, Vec<SourceId>>,
    host_compatibility: Option<HostCompatibilityV1>,
}

impl PackageResolver {
    /// Validates and indexes evaluated candidates.
    ///
    /// # Errors
    ///
    /// Returns `ResolutionError` when a package is invalid, source and package
    /// identities disagree, or a source ID is duplicated.
    pub fn new(
        candidates: impl IntoIterator<Item = PackageCandidate>,
    ) -> Result<Self, ResolutionError> {
        Self::index(candidates, None)
    }

    /// Validates and indexes candidates against trusted host evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when host evidence or a package/source binding is
    /// invalid.
    pub fn new_with_host_compatibility(
        candidates: impl IntoIterator<Item = PackageCandidate>,
        host_compatibility: HostCompatibilityV1,
    ) -> Result<Self, ResolutionError> {
        host_compatibility
            .validate()
            .map_err(|source| ResolutionError::InvalidHostCompatibility { source })?;
        Self::index(candidates, Some(host_compatibility))
    }

    fn index(
        candidates: impl IntoIterator<Item = PackageCandidate>,
        host_compatibility: Option<HostCompatibilityV1>,
    ) -> Result<Self, ResolutionError> {
        let mut by_source = BTreeMap::new();
        let mut by_package = BTreeMap::<PackageName, Vec<SourceId>>::new();

        for candidate in candidates {
            candidate
                .package
                .validate()
                .map_err(|source| ResolutionError::InvalidPackage {
                    package: candidate.package.name.clone(),
                    source: Box::new(source),
                })?;
            validate_candidate_binding(&candidate)?;
            let source_id = candidate.source.source_id.clone();
            if by_source.contains_key(&source_id) {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("duplicate evaluated source ID {source_id}"),
                    context: Box::new(ResolutionFailureContext {
                        package_chain: vec![candidate.package.name.clone()],
                        requested: vec![source_id.to_string()],
                        available: vec![CandidateIdentityV1::from(&candidate).to_string()],
                    }),
                });
            }
            by_package
                .entry(candidate.package.name.clone())
                .or_default()
                .push(source_id.clone());
            by_source.insert(source_id, candidate);
        }

        for source_ids in by_package.values_mut() {
            source_ids.sort_by(
                |left, right| match (by_source.get(left), by_source.get(right)) {
                    (Some(left), Some(right)) => compare_package_candidates(left, right),
                    _ => left.cmp(right),
                },
            );
        }

        Ok(Self {
            candidates: by_source,
            by_package,
            host_compatibility,
        })
    }

    /// Resolves a composition using the controlled source universe.
    ///
    /// Finite backtracking permits a lower candidate when a higher candidate's
    /// transitive dependency or capability closure is unsatisfiable.
    ///
    /// # Errors
    ///
    /// Returns `ResolutionError` for invalid inputs, an unsatisfied graph, a
    /// cycle, unavailable realization, or canonical encoding failure.
    pub fn resolve(
        &self,
        composition: &CompositionSpec,
    ) -> Result<PackageResolutionV1, ResolutionError> {
        self.resolve_with_limits(composition, ResolutionLimits::default())
    }

    /// Resolves a composition with explicit deterministic logical-work limits.
    ///
    /// # Errors
    ///
    /// Returns `ResolutionError` for invalid inputs, an unsatisfied graph, an
    /// unavailable realization, or an exceeded logical-work budget.
    pub fn resolve_with_limits(
        &self,
        composition: &CompositionSpec,
        limits: ResolutionLimits,
    ) -> Result<PackageResolutionV1, ResolutionError> {
        self.resolve_with_pins(composition, &BTreeMap::new(), limits)
    }

    /// Revalidates and exactly reuses a frozen pre-build resolution receipt.
    ///
    /// No compatible fallback is attempted. Every package is pinned to its
    /// exact source ID, version, content hash, and selected realization.
    ///
    /// # Errors
    ///
    /// Returns `ResolutionError` when the receipt is invalid, an exact source
    /// is unavailable, or reconstruction differs from its selected closure.
    pub fn resolve_frozen(
        &self,
        composition: &CompositionSpec,
        frozen: &ResolutionReceiptV1,
    ) -> Result<PackageResolutionV1, ResolutionError> {
        self.resolve_frozen_with_limits(composition, frozen, ResolutionLimits::default())
    }

    /// Replays an exact resolver-stage receipt with explicit safety limits.
    ///
    /// Unselected additions to the current source universe are ignored. This
    /// verifies selection inputs and source receipts only; built artifacts are
    /// outside this pre-build API.
    ///
    /// # Errors
    ///
    /// Returns an error when the receipt or current intent is invalid, a
    /// selected source receipt changed, replay selects a different closure, or
    /// a deterministic logical-work budget is exceeded.
    pub fn resolve_frozen_with_limits(
        &self,
        composition: &CompositionSpec,
        frozen: &ResolutionReceiptV1,
        limits: ResolutionLimits,
    ) -> Result<PackageResolutionV1, ResolutionError> {
        frozen
            .validate()
            .map_err(|source| ResolutionError::InvalidResolutionReceipt { source })?;
        limits
            .validate()
            .map_err(|source| ResolutionError::InvalidLimits { source })?;
        composition
            .validate()
            .map_err(|source| ResolutionError::InvalidComposition {
                source: Box::new(source),
            })?;
        Self::validate_prebuild_surfaces(composition)?;
        self.validate_host_target(composition)?;

        let intent_hash = resolution_intent_hash(composition)?;
        Self::validate_frozen_intent_headers(composition, frozen, intent_hash)?;

        let allowed = self.validate_frozen_source_closure(composition, frozen)?;
        let root_context = ResolutionFailureContext {
            package_chain: frozen.roots.iter().cloned().collect(),
            requested: Vec::new(),
            available: Vec::new(),
        };
        check_count_limit(
            ResolutionBudget::SourceCandidates,
            limits.max_source_candidates,
            allowed.len(),
            &root_context,
        )?;
        let pins = frozen
            .packages
            .iter()
            .map(|(name, package)| (name.clone(), package.source_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut session = SearchSession::new(limits);
        let analysis = self.search(
            composition,
            &allowed,
            &pins,
            &SearchState::default(),
            &mut session,
            0,
        )?;
        let replayed =
            self.build_resolution(composition, &allowed, &analysis, &session.trace, limits)?;
        if replayed.receipt.resolution_intent_hash != frozen.resolution_intent_hash
            || replayed.receipt.profile != frozen.profile
            || replayed.receipt.profile_kind != frozen.profile_kind
            || replayed.receipt.target != frozen.target
            || replayed.receipt.host_compatibility_hash != frozen.host_compatibility_hash
            || replayed.receipt.evaluation_policy != frozen.evaluation_policy
            || replayed.receipt.evaluation_limits != frozen.evaluation_limits
            || replayed.receipt.roots != frozen.roots
            || replayed.receipt.packages != frozen.packages
            || replayed.receipt.capabilities != frozen.capabilities
            || replayed.receipt.namespace_grants != frozen.namespace_grants
            || selected_source_explanation(&replayed.receipt) != selected_source_explanation(frozen)
        {
            return Err(ResolutionError::ResolutionReceiptMismatch {
                reason: "exact selected closure differs from the receipt".to_owned(),
                context: Box::new(ResolutionFailureContext {
                    package_chain: frozen.roots.iter().cloned().collect(),
                    requested: vec![frozen.receipt_hash.to_string()],
                    available: vec![replayed.receipt.receipt_hash.to_string()],
                }),
            });
        }
        Ok(PackageResolutionV1 {
            receipt: frozen.clone(),
            build_intent: Self::build_intent_from_receipt(frozen)?,
        })
    }

    fn resolve_with_pins(
        &self,
        composition: &CompositionSpec,
        pins: &BTreeMap<PackageName, SourceId>,
        limits: ResolutionLimits,
    ) -> Result<PackageResolutionV1, ResolutionError> {
        limits
            .validate()
            .map_err(|source| ResolutionError::InvalidLimits { source })?;
        composition
            .validate()
            .map_err(|source| ResolutionError::InvalidComposition {
                source: Box::new(source),
            })?;
        Self::validate_prebuild_surfaces(composition)?;
        self.validate_host_target(composition)?;
        let allowed = self.validate_source_universe(composition)?;
        let context = ResolutionFailureContext {
            package_chain: composition.roots.keys().cloned().collect(),
            requested: Vec::new(),
            available: Vec::new(),
        };
        check_count_limit(
            ResolutionBudget::SourceCandidates,
            limits.max_source_candidates,
            allowed.len(),
            &context,
        )?;
        let mut session = SearchSession::new(limits);
        let analysis = self.search(
            composition,
            &allowed,
            pins,
            &SearchState::default(),
            &mut session,
            0,
        )?;
        self.build_resolution(composition, &allowed, &analysis, &session.trace, limits)
    }

    fn validate_frozen_intent_headers(
        composition: &CompositionSpec,
        frozen: &ResolutionReceiptV1,
        current_intent_hash: CanonicalHash,
    ) -> Result<(), ResolutionError> {
        let current_roots = composition.roots.keys().cloned().collect::<BTreeSet<_>>();
        let mismatch = if frozen.resolution_intent_hash != current_intent_hash {
            Some((
                "current resolution intent differs from the receipt",
                frozen.resolution_intent_hash.to_string(),
                current_intent_hash.to_string(),
            ))
        } else if frozen.profile != composition.profile {
            Some((
                "profile identity differs from the receipt",
                frozen.profile.to_string(),
                composition.profile.to_string(),
            ))
        } else if frozen.profile_kind != composition.profile_kind {
            Some((
                "profile projection differs from the receipt",
                format!("{:?}", frozen.profile_kind),
                format!("{:?}", composition.profile_kind),
            ))
        } else if frozen.target != composition.policy.target {
            Some((
                "target differs from the receipt",
                frozen.target.to_string(),
                composition.policy.target.to_string(),
            ))
        } else if frozen.evaluation_policy != composition.policy.evaluation_policy {
            Some((
                "evaluation policy differs from the receipt",
                frozen.evaluation_policy.to_string(),
                composition.policy.evaluation_policy.to_string(),
            ))
        } else if frozen.evaluation_limits != composition.policy.evaluation_limits {
            Some((
                "evaluation limits differ from the receipt",
                format!("{:?}", frozen.evaluation_limits),
                format!("{:?}", composition.policy.evaluation_limits),
            ))
        } else if frozen.roots != current_roots {
            Some((
                "root package set differs from the receipt",
                frozen
                    .roots
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
                current_roots
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ))
        } else {
            None
        };

        if let Some((reason, requested, available)) = mismatch {
            return Err(ResolutionError::ResolutionReceiptMismatch {
                reason: reason.to_owned(),
                context: Box::new(ResolutionFailureContext {
                    package_chain: current_roots.into_iter().collect(),
                    requested: vec![requested],
                    available: vec![available],
                }),
            });
        }
        Ok(())
    }

    fn validate_host_target(&self, composition: &CompositionSpec) -> Result<(), ResolutionError> {
        if let Some(host) = &self.host_compatibility
            && host.target != composition.policy.target
        {
            return Err(ResolutionError::HostTargetMismatch {
                selected: composition.policy.target.clone(),
                available: host.target.clone(),
            });
        }
        Ok(())
    }

    fn host_compatibility_hash(&self) -> Result<Option<CanonicalHash>, ResolutionError> {
        Ok(self
            .host_compatibility
            .as_ref()
            .map(HostCompatibilityV1::compatibility_hash)
            .transpose()?)
    }

    fn validate_prebuild_surfaces(composition: &CompositionSpec) -> Result<(), ResolutionError> {
        if !composition.parameters.is_empty() {
            return Err(ResolutionError::UnsupportedPreBuildSurface {
                surface: PreBuildSurface::CompositionParameters,
                context: Box::new(ResolutionFailureContext {
                    package_chain: composition.roots.keys().cloned().collect(),
                    requested: composition
                        .parameters
                        .keys()
                        .map(ToString::to_string)
                        .collect(),
                    available: Vec::new(),
                }),
            });
        }
        Ok(())
    }

    fn validate_selected_candidate_surfaces(
        candidate: &PackageCandidate,
        node: &Node,
    ) -> Result<(), ResolutionError> {
        if !candidate.package.parameters.is_empty() {
            return Err(ResolutionError::UnsupportedPreBuildSurface {
                surface: PreBuildSurface::PackageParameters,
                context: Box::new(
                    node.context(
                        candidate
                            .package
                            .parameters
                            .keys()
                            .map(ToString::to_string)
                            .collect(),
                    ),
                ),
            });
        }
        Ok(())
    }

    fn validate_frozen_source_closure(
        &self,
        composition: &CompositionSpec,
        receipt: &ResolutionReceiptV1,
    ) -> Result<BTreeSet<SourceId>, ResolutionError> {
        let mut allowed = BTreeSet::new();
        for (name, locked) in &receipt.packages {
            let context = ResolutionFailureContext {
                package_chain: vec![name.clone()],
                requested: vec![format!(
                    "{} {} {}",
                    locked.version, locked.source_id, locked.source_hash
                )],
                available: self.available_summaries(name),
            };
            let Some(source) = composition
                .sources
                .iter()
                .find(|source| source.source_id == locked.source_id)
            else {
                return Err(ResolutionError::ResolutionReceiptMismatch {
                    reason: format!("selected source {} is absent", locked.source_id),
                    context: Box::new(context),
                });
            };
            let Some(candidate) = self.candidates.get(&locked.source_id) else {
                return Err(ResolutionError::ResolutionReceiptMismatch {
                    reason: format!("selected source {} was not evaluated", locked.source_id),
                    context: Box::new(context),
                });
            };
            if &candidate.source != source {
                return Err(ResolutionError::ResolutionReceiptMismatch {
                    reason: format!(
                        "selected source {} differs from the current source receipt",
                        locked.source_id
                    ),
                    context: Box::new(context),
                });
            }
            let source_path =
                CanonicalLogicalPath::new(candidate.source.path.clone()).map_err(|error| {
                    ResolutionError::InvalidCandidate {
                        candidate: Box::new(CandidateIdentityV1::from(candidate)),
                        reason: format!("source path is not canonical: {error}"),
                        context: Box::new(context.clone()),
                    }
                })?;
            let source_provenance_hash = provenance_hash(&candidate.source.provenance)?;
            let package_provenance_hash = provenance_hash(&candidate.package.provenance)?;
            if candidate.package.name != *name
                || candidate.package.version != locked.version
                || candidate.source_kind != locked.source_kind
                || candidate.source.priority != locked.source_priority
                || candidate.source.content_hash != locked.source_hash
                || source_provenance_hash != locked.source_provenance_hash
                || package_provenance_hash != locked.package_provenance_hash
                || source_path != locked.source_path
            {
                return Err(ResolutionError::ResolutionReceiptMismatch {
                    reason: format!(
                        "selected source {} no longer matches its exact pre-build receipt",
                        locked.source_id
                    ),
                    context: Box::new(context),
                });
            }
            allowed.insert(locked.source_id.clone());
        }
        Ok(allowed)
    }

    fn build_intent_from_receipt(
        receipt: &ResolutionReceiptV1,
    ) -> Result<BuildIntentV1, ResolutionError> {
        let packages = receipt
            .packages
            .iter()
            .map(|(name, package)| {
                (
                    name.clone(),
                    PackageBuildIntentV1 {
                        version: package.version.clone(),
                        source_id: package.source_id.clone(),
                        source_hash: package.source_hash,
                        features: package.features.clone(),
                        realization: package.realization.clone(),
                    },
                )
            })
            .collect();
        let mut intent = BuildIntentV1 {
            schema_version: BUILD_INTENT_SCHEMA_VERSION,
            resolution_receipt_hash: receipt.receipt_hash,
            target: receipt.target.clone(),
            packages,
            namespace_grants: receipt.namespace_grants.clone(),
            build_intent_hash: CanonicalHash::digest(b"pending-build-intent"),
        };
        intent.build_intent_hash = intent.recompute_hash()?;
        Ok(intent)
    }

    fn validate_source_universe(
        &self,
        composition: &CompositionSpec,
    ) -> Result<BTreeSet<SourceId>, ResolutionError> {
        let mut allowed = BTreeSet::new();
        for source in &composition.sources {
            let Some(candidate) = self.candidates.get(&source.source_id) else {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("source {} has no evaluated package", source.source_id),
                    context: Box::new(ResolutionFailureContext {
                        package_chain: vec![source.package.clone()],
                        requested: vec![format!(
                            "{} {} {}",
                            source.version, source.source_id, source.content_hash
                        )],
                        available: self.available_summaries(&source.package),
                    }),
                });
            };
            if &candidate.source != source {
                return Err(ResolutionError::InvalidCandidate {
                    candidate: Box::new(CandidateIdentityV1::from(candidate)),
                    reason: "candidate source differs from CompositionSpec".to_owned(),
                    context: Box::new(ResolutionFailureContext {
                        package_chain: vec![source.package.clone()],
                        requested: vec![format!(
                            "{} {} {}",
                            source.version, source.source_id, source.content_hash
                        )],
                        available: vec![candidate_summary(candidate)],
                    }),
                });
            }
            allowed.insert(source.source_id.clone());
        }
        Ok(allowed)
    }

    fn search(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        pins: &BTreeMap<PackageName, SourceId>,
        state: &SearchState,
        session: &mut SearchSession,
        depth: u64,
    ) -> Result<CompleteAnalysis, ResolutionError> {
        let progress = self.analyze(composition, allowed, pins, state, session)?;
        let (live_state, context) = match &progress {
            AnalysisProgress::NeedPackage { node, state, .. } => (state, node.context(Vec::new())),
            AnalysisProgress::NeedProvider { context, state, .. } => (state, context.clone()),
            AnalysisProgress::Complete(analysis) => (
                &analysis.state,
                ResolutionFailureContext {
                    package_chain: composition.roots.keys().cloned().collect(),
                    requested: Vec::new(),
                    available: analysis
                        .state
                        .packages
                        .values()
                        .map(ToString::to_string)
                        .collect(),
                },
            ),
        };
        let key = SearchKey::from(live_state);
        session.enter_state(&key, depth, &context)?;
        let result = self.search_progress(composition, allowed, pins, progress, session, depth);
        session.active_states.remove(&key);
        if let Err(error) = &result
            && let Some(failure) = backtracking_failure(error)
        {
            session.failed_states.insert(key, failure);
        }
        result
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn search_progress(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        pins: &BTreeMap<PackageName, SourceId>,
        progress: AnalysisProgress,
        session: &mut SearchSession,
        depth: u64,
    ) -> Result<CompleteAnalysis, ResolutionError> {
        match progress {
            AnalysisProgress::Complete(analysis) => Ok(analysis),
            AnalysisProgress::NeedPackage {
                package,
                node,
                state,
            } => {
                let candidates =
                    self.compatible_package_candidates(composition, allowed, pins, &package, &node);
                let context = node.context(
                    candidates
                        .iter()
                        .filter_map(|source| self.candidates.get(source))
                        .map(candidate_summary)
                        .collect(),
                );
                check_count_limit(
                    ResolutionBudget::CandidatesPerDecision,
                    session.limits.max_candidates_per_decision,
                    candidates.len(),
                    &context,
                )?;
                if candidates.is_empty() {
                    return Err(self.no_package_candidate_error(
                        composition,
                        allowed,
                        &package,
                        &node,
                    ));
                }
                let mut failures = Vec::new();
                for source_id in candidates {
                    session.observe_transition(&context)?;
                    let mut next = state.clone();
                    next.packages.insert(package.clone(), source_id.clone());
                    match self.search(
                        composition,
                        allowed,
                        pins,
                        &next,
                        session,
                        depth.saturating_add(1),
                    ) {
                        Ok(solution) => return Ok(solution),
                        Err(error) if is_global_fatal(&error) => return Err(error),
                        Err(error) => {
                            if let Some(failure) = backtracking_failure(&error) {
                                session.trace.record_package(source_id, failure.clone());
                                failures.push((failure, error));
                            } else {
                                return Err(error);
                            }
                        }
                    }
                }
                Err(primary_branch_error(failures).unwrap_or_else(|| {
                    self.no_package_candidate_error(composition, allowed, &package, &node)
                }))
            }
            AnalysisProgress::NeedProvider {
                capability,
                candidates,
                context,
                state,
            } => {
                check_count_limit(
                    ResolutionBudget::CandidatesPerDecision,
                    session.limits.max_candidates_per_decision,
                    candidates.len(),
                    &context,
                )?;
                if candidates.is_empty() {
                    return Err(ResolutionError::MissingCapability {
                        capability,
                        context: Box::new(context),
                    });
                }
                let mut failures = Vec::new();
                for source_id in candidates {
                    session.observe_transition(&context)?;
                    let Some(candidate) = self.candidates.get(&source_id) else {
                        continue;
                    };
                    if state
                        .packages
                        .get(&candidate.package.name)
                        .is_some_and(|selected| selected != &source_id)
                    {
                        let error = ResolutionError::PinnedSourceUnavailable {
                            package: candidate.package.name.clone(),
                            required: BTreeSet::from([source_id.clone()]),
                            context: Box::new(context.clone()),
                        };
                        let failure = backtracking_failure(&error).ok_or_else(|| {
                            ResolutionError::MissingCapability {
                                capability: capability.clone(),
                                context: Box::new(context.clone()),
                            }
                        })?;
                        session.trace.record_provider(
                            capability.clone(),
                            source_id,
                            failure.clone(),
                        );
                        failures.push((failure, error));
                        continue;
                    }
                    let mut next = state.clone();
                    next.providers.insert(capability.clone(), source_id.clone());
                    next.packages
                        .insert(candidate.package.name.clone(), source_id.clone());
                    match self.search(
                        composition,
                        allowed,
                        pins,
                        &next,
                        session,
                        depth.saturating_add(1),
                    ) {
                        Ok(solution) => return Ok(solution),
                        Err(error) if is_global_fatal(&error) => return Err(error),
                        Err(error) => {
                            if let Some(failure) = backtracking_failure(&error) {
                                session.trace.record_provider(
                                    capability.clone(),
                                    source_id,
                                    failure.clone(),
                                );
                                failures.push((failure, error));
                            } else {
                                return Err(error);
                            }
                        }
                    }
                }
                Err(
                    primary_branch_error(failures).unwrap_or(ResolutionError::MissingCapability {
                        capability,
                        context: Box::new(context),
                    }),
                )
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn analyze(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        pins: &BTreeMap<PackageName, SourceId>,
        state: &SearchState,
        session: &mut SearchSession,
    ) -> Result<AnalysisProgress, ResolutionError> {
        let mut nodes = BTreeMap::<PackageName, Node>::new();

        for (name, request) in &composition.roots {
            let node = nodes.entry(name.clone()).or_default();
            node.requested_features
                .extend(request.features.iter().cloned());
            node.add_requirement(Requirement {
                range: Some(request.version.clone()),
                origin: RequirementOrigin::Root,
                chain: vec![name.clone()],
            });
        }
        let analysis_context = ResolutionFailureContext {
            package_chain: composition.roots.keys().cloned().collect(),
            requested: Vec::new(),
            available: Vec::new(),
        };
        loop {
            session.observe_analysis_step(&analysis_context)?;
            let live_capabilities = self.collect_capabilities(composition, &nodes, state)?;
            check_graph_limits(session, &nodes, &live_capabilities, &analysis_context)?;
            let provider_nodes_added =
                self.add_live_provider_nodes(composition, &mut nodes, state)?;
            if let Some((package, node)) = nodes
                .iter()
                .find(|(package, _)| !state.packages.contains_key(*package))
            {
                return Ok(AnalysisProgress::NeedPackage {
                    package: package.clone(),
                    node: node.clone(),
                    state: Self::normalized_search_state(&nodes, &live_capabilities, state),
                });
            }

            for (package, node) in &nodes {
                let Some(source_id) = state.packages.get(package) else {
                    continue;
                };
                let Some(candidate) = self.candidates.get(source_id) else {
                    return Err(ResolutionError::InvalidSourceUniverse {
                        reason: format!("selected source {source_id} left the catalog"),
                        context: Box::new(node.context(self.available_summaries(package))),
                    });
                };
                let reasons = self.package_candidate_reasons(composition, candidate, node);
                if !reasons.is_empty() {
                    return Err(
                        self.error_for_candidate_reasons(package, node, candidate, &reasons)
                    );
                }
                Self::validate_selected_candidate_surfaces(candidate, node)?;
            }

            let mut changed = provider_nodes_added;
            let selected = nodes
                .keys()
                .filter_map(|name| {
                    state
                        .packages
                        .get(name)
                        .and_then(|source_id| self.candidates.get(source_id))
                        .map(|candidate| (name.clone(), candidate.clone()))
                })
                .collect::<Vec<_>>();

            for (name, candidate) in selected {
                let requested = nodes
                    .get(&name)
                    .map(|node| node.requested_features.clone())
                    .unwrap_or_default();
                let active_features = active_features(composition, &candidate.package, &requested);
                let active_dependencies = candidate
                    .package
                    .dependencies
                    .iter()
                    .filter(|(_, dependency)| {
                        domains_active(&dependency.domains, composition)
                            && (!dependency.optional
                                || dependency
                                    .when_features
                                    .iter()
                                    .any(|feature| active_features.contains(feature)))
                    })
                    .map(|(name, dependency)| (name.clone(), dependency.clone()))
                    .collect::<BTreeMap<_, _>>();

                if let Some(node) = nodes.get_mut(&name) {
                    if node.active_features != active_features {
                        node.active_features = active_features;
                        changed = true;
                    }
                    if node.dependencies != active_dependencies {
                        node.dependencies = active_dependencies.clone();
                        changed = true;
                    }
                }

                let parent_chain = nodes
                    .get(&name)
                    .map_or_else(|| vec![name.clone()], Node::primary_chain);
                for (dependency_name, dependency) in active_dependencies {
                    let mut chain = parent_chain.clone();
                    chain.push(dependency_name.clone());
                    let dependency_node = nodes.entry(dependency_name.clone()).or_default();
                    if dependency_node.add_requirement(Requirement {
                        range: Some(dependency.version.clone()),
                        origin: RequirementOrigin::Dependency(name.clone()),
                        chain,
                    }) {
                        changed = true;
                    }
                    let old_len = dependency_node.requested_features.len();
                    dependency_node
                        .requested_features
                        .extend(dependency.features.iter().cloned());
                    if old_len != dependency_node.requested_features.len() {
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }

        let capabilities = self.collect_capabilities(composition, &nodes, state)?;
        for (capability, group) in &capabilities {
            check_graph_limits(session, &nodes, &capabilities, &analysis_context)?;
            let providers =
                self.compatible_selected_providers(composition, capability, group, &nodes, state)?;
            let needs_provider = !group.demands.is_empty()
                && matches!(
                    group.cardinality,
                    CapabilityCardinality::ExactlyOne | CapabilityCardinality::OneOrMore
                );

            if group.cardinality == CapabilityCardinality::ExactlyOne && providers.len() > 1 {
                return Err(ResolutionError::ConflictingCapabilityProviders {
                    capability: capability.clone(),
                    context: Box::new(group.context(self.provider_summaries(capability, allowed))),
                });
            }
            if needs_provider && providers.is_empty() {
                let candidates = self.compatible_provider_candidates(
                    composition,
                    allowed,
                    pins,
                    capability,
                    group,
                    state,
                );
                if candidates.is_empty()
                    && self.has_provider_cardinality_mismatch(
                        composition,
                        allowed,
                        pins,
                        capability,
                        group,
                        state,
                    )
                {
                    return Err(ResolutionError::CapabilityCardinalityConflict {
                        capability: capability.clone(),
                        required: group.cardinality,
                        actual: 0,
                        context: Box::new(
                            group.context(self.provider_summaries(capability, allowed)),
                        ),
                    });
                }
                return Ok(AnalysisProgress::NeedProvider {
                    capability: capability.clone(),
                    candidates,
                    context: group.context(self.provider_summaries(capability, allowed)),
                    state: Self::normalized_search_state(&nodes, &capabilities, state),
                });
            }
        }

        let capability_providers = capabilities
            .iter()
            .map(|(capability, group)| {
                let providers = self
                    .compatible_selected_providers(composition, capability, group, &nodes, state)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|source_id| {
                        self.candidates
                            .get(&source_id)
                            .map(|candidate| candidate.package.name.clone())
                    })
                    .collect::<Vec<_>>();
                (capability.clone(), providers)
            })
            .filter(|(_, providers)| !providers.is_empty())
            .collect::<BTreeMap<_, _>>();

        if let Some(cycle) = dependency_cycle(&nodes, &capabilities, &capability_providers) {
            return Err(ResolutionError::DependencyCycle {
                context: Box::new(ResolutionFailureContext {
                    package_chain: cycle,
                    requested: vec!["acyclic active package graph".to_owned()],
                    available: state.packages.keys().map(ToString::to_string).collect(),
                }),
            });
        }

        let live_state = Self::normalized_search_state(&nodes, &capabilities, state);
        let selected_sources = live_state.packages.clone();
        let analysis = CompleteAnalysis {
            state: live_state,
            nodes,
            capabilities,
            capability_providers,
            selected_sources,
        };
        self.namespace_grants(composition, &analysis)?;
        Ok(AnalysisProgress::Complete(analysis))
    }

    fn add_live_provider_nodes(
        &self,
        composition: &CompositionSpec,
        nodes: &mut BTreeMap<PackageName, Node>,
        state: &SearchState,
    ) -> Result<bool, ResolutionError> {
        let capabilities = self.collect_capabilities(composition, nodes, state)?;
        let mut changed = false;
        for (capability, group) in capabilities {
            let requires_provider = !group.demands.is_empty()
                && matches!(
                    group.cardinality,
                    CapabilityCardinality::ExactlyOne | CapabilityCardinality::OneOrMore
                );
            if !requires_provider {
                continue;
            }
            let Some(source_id) = state.providers.get(&capability) else {
                continue;
            };
            let Some(candidate) = self.candidates.get(source_id) else {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("selected provider source {source_id} left the catalog"),
                    context: Box::new(group.context(Vec::new())),
                });
            };
            let mut chain = group.context(Vec::new()).package_chain;
            if chain.last() != Some(&candidate.package.name) {
                chain.push(candidate.package.name.clone());
            }
            let node = nodes.entry(candidate.package.name.clone()).or_default();
            changed |= node.pinned_sources.insert(source_id.clone());
            changed |= node.add_requirement(Requirement {
                range: None,
                origin: RequirementOrigin::Capability(capability),
                chain,
            });
        }
        Ok(changed)
    }

    fn normalized_search_state(
        nodes: &BTreeMap<PackageName, Node>,
        capabilities: &BTreeMap<CapabilityId, CapabilityGroup>,
        state: &SearchState,
    ) -> SearchState {
        let packages = state
            .packages
            .iter()
            .filter(|(package, _)| nodes.contains_key(*package))
            .map(|(package, source)| (package.clone(), source.clone()))
            .collect();
        let providers = state
            .providers
            .iter()
            .filter(|(capability, _)| {
                capabilities.get(*capability).is_some_and(|group| {
                    !group.demands.is_empty()
                        && matches!(
                            group.cardinality,
                            CapabilityCardinality::ExactlyOne | CapabilityCardinality::OneOrMore
                        )
                })
            })
            .map(|(capability, source)| (capability.clone(), source.clone()))
            .collect();
        SearchState {
            packages,
            providers,
        }
    }

    fn collect_capabilities(
        &self,
        composition: &CompositionSpec,
        nodes: &BTreeMap<PackageName, Node>,
        state: &SearchState,
    ) -> Result<BTreeMap<CapabilityId, CapabilityGroup>, ResolutionError> {
        let mut groups = BTreeMap::<CapabilityId, CapabilityGroup>::new();
        for (capability, requirement) in &composition.capabilities {
            if domains_active(&requirement.domains, composition) {
                groups
                    .entry(capability.clone())
                    .or_insert_with(|| CapabilityGroup::new(requirement.cardinality))
                    .add_demand(CapabilityDemand {
                        requirement: requirement.clone(),
                        required_by: None,
                        chain: Vec::new(),
                    })?;
            }
        }

        for (name, node) in nodes {
            let Some(candidate) = state
                .packages
                .get(name)
                .and_then(|source_id| self.candidates.get(source_id))
            else {
                continue;
            };
            for (capability, requirement) in &candidate.package.requires {
                if domains_active(&requirement.domains, composition) {
                    groups
                        .entry(capability.clone())
                        .or_insert_with(|| CapabilityGroup::new(requirement.cardinality))
                        .add_demand(CapabilityDemand {
                            requirement: requirement.clone(),
                            required_by: Some(name.clone()),
                            chain: node.primary_chain(),
                        })?;
                }
            }
        }
        Ok(groups)
    }

    fn compatible_selected_providers(
        &self,
        composition: &CompositionSpec,
        capability: &CapabilityId,
        group: &CapabilityGroup,
        nodes: &BTreeMap<PackageName, Node>,
        state: &SearchState,
    ) -> Result<Vec<SourceId>, ResolutionError> {
        let mut providers = Vec::new();
        for name in nodes.keys() {
            let Some(candidate) = state
                .packages
                .get(name)
                .and_then(|source_id| self.candidates.get(source_id))
            else {
                continue;
            };
            let Some(provision) = candidate.package.provides.get(capability) else {
                continue;
            };
            if !domains_active(&provision.domains, composition) {
                continue;
            }
            if group.cardinality == CapabilityCardinality::ExactlyOne
                && state
                    .providers
                    .get(capability)
                    .is_some_and(|selected| selected != &candidate.source.source_id)
            {
                continue;
            }
            if provision.cardinality == group.cardinality
                && provider_satisfies_demands(candidate, provision, group)
            {
                providers.push(candidate.source.source_id.clone());
            }
        }

        providers.sort_by(|left, right| self.compare_provider_ids(left, right));
        if group.cardinality == CapabilityCardinality::ExactlyOne && providers.len() > 1 {
            return Err(ResolutionError::ConflictingCapabilityProviders {
                capability: capability.clone(),
                context: Box::new(
                    group.context(
                        providers
                            .iter()
                            .filter_map(|source_id| self.candidates.get(source_id))
                            .map(candidate_summary)
                            .collect(),
                    ),
                ),
            });
        }
        Ok(providers)
    }

    fn compatible_provider_candidates(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        pins: &BTreeMap<PackageName, SourceId>,
        capability: &CapabilityId,
        group: &CapabilityGroup,
        state: &SearchState,
    ) -> Vec<SourceId> {
        let mut candidates = allowed
            .iter()
            .filter_map(|source_id| {
                let candidate = self.candidates.get(source_id)?;
                let provision = candidate.package.provides.get(capability)?;
                if state
                    .packages
                    .get(&candidate.package.name)
                    .is_some_and(|selected| selected != source_id)
                {
                    return None;
                }

                let mut node = Node::default();
                if let Some(pin) = pins.get(&candidate.package.name) {
                    node.pinned_sources.insert(pin.clone());
                }
                let eligible = self
                    .package_candidate_reasons(composition, candidate, &node)
                    .is_empty()
                    && provision.cardinality == group.cardinality
                    && domains_active(&provision.domains, composition)
                    && provider_satisfies_demands(candidate, provision, group);
                eligible.then(|| source_id.clone())
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| self.compare_provider_ids(left, right));
        candidates
    }

    fn has_provider_cardinality_mismatch(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        pins: &BTreeMap<PackageName, SourceId>,
        capability: &CapabilityId,
        group: &CapabilityGroup,
        state: &SearchState,
    ) -> bool {
        allowed.iter().any(|source_id| {
            let Some(candidate) = self.candidates.get(source_id) else {
                return false;
            };
            let Some(provision) = candidate.package.provides.get(capability) else {
                return false;
            };
            if state
                .packages
                .get(&candidate.package.name)
                .is_some_and(|selected| selected != source_id)
                || pins
                    .get(&candidate.package.name)
                    .is_some_and(|pinned| pinned != source_id)
            {
                return false;
            }
            let node = Node::default();
            self.package_candidate_reasons(composition, candidate, &node)
                .is_empty()
                && domains_active(&provision.domains, composition)
                && provider_satisfies_demands(candidate, provision, group)
                && provision.cardinality != group.cardinality
        })
    }

    fn compatible_package_candidates(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        pins: &BTreeMap<PackageName, SourceId>,
        package: &PackageName,
        node: &Node,
    ) -> Vec<SourceId> {
        self.by_package
            .get(package)
            .into_iter()
            .flatten()
            .filter(|source_id| allowed.contains(*source_id))
            .filter(|source_id| {
                pins.get(package).is_none_or(|pin| pin == *source_id)
                    && (node.pinned_sources.is_empty() || node.pinned_sources.contains(*source_id))
            })
            .filter_map(|source_id| {
                self.candidates.get(source_id).and_then(|candidate| {
                    self.package_candidate_reasons(composition, candidate, node)
                        .is_empty()
                        .then(|| source_id.clone())
                })
            })
            .collect()
    }

    fn package_candidate_reasons(
        &self,
        composition: &CompositionSpec,
        candidate: &PackageCandidate,
        node: &Node,
    ) -> Vec<ResolutionReasonV1> {
        let mut reasons = Vec::new();
        if !node.pinned_sources.is_empty()
            && !node.pinned_sources.contains(&candidate.source.source_id)
        {
            reasons.push(ResolutionReasonV1::PinnedSourceMismatch {
                required: node.pinned_sources.clone(),
            });
        }

        let mismatched = node
            .requirements
            .iter()
            .filter_map(|requirement| requirement.range.as_ref())
            .filter(|range| !range.matches(&candidate.package.version))
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if !mismatched.is_empty() {
            reasons.push(ResolutionReasonV1::VersionMismatch {
                requirements: mismatched,
            });
        }

        let active_domains = candidate
            .package
            .domains
            .intersection(&composition.projection_domains)
            .copied()
            .collect::<BTreeSet<_>>();
        if active_domains.is_empty() {
            reasons.push(ResolutionReasonV1::DomainMismatch);
        }

        let requested = requested_features(composition, &candidate.package.name, node);
        let unknown = requested
            .iter()
            .filter(|feature| !candidate.package.features.contains_key(*feature))
            .cloned()
            .collect::<BTreeSet<_>>();
        if !unknown.is_empty() {
            reasons.push(ResolutionReasonV1::UnknownFeatures { features: unknown });
        }
        if candidate.package.trust > composition.policy.maximum_trust {
            reasons.push(ResolutionReasonV1::TrustExceeded {
                required: candidate.package.trust,
                allowed: composition.policy.maximum_trust,
            });
        }

        let features = active_features(composition, &candidate.package, &node.requested_features);
        if self
            .select_realization(composition, candidate, &active_domains, &features)
            .is_none()
        {
            reasons.push(ResolutionReasonV1::NoEligibleRealization);
        }
        reasons
    }

    fn select_realization<'a>(
        &self,
        composition: &CompositionSpec,
        candidate: &'a PackageCandidate,
        active_domains: &BTreeSet<PackageDomain>,
        features: &BTreeSet<String>,
    ) -> Option<&'a RealizationSpec> {
        let preference = composition
            .roots
            .get(&candidate.package.name)
            .map_or(RealizationPreference::Auto, |request| request.realization);
        let mut eligible = candidate
            .package
            .realizations
            .values()
            .filter(|realization| {
                self.realization_reasons(
                    composition,
                    realization,
                    preference,
                    active_domains,
                    features,
                )
                .is_empty()
            })
            .collect::<Vec<_>>();
        eligible.sort_by(|left, right| {
            realization_policy_rank(composition, left, preference)
                .cmp(&realization_policy_rank(composition, right, preference))
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left.registration_fragment.cmp(&right.registration_fragment))
        });
        eligible.into_iter().next()
    }

    fn realization_reasons(
        &self,
        composition: &CompositionSpec,
        realization: &RealizationSpec,
        preference: RealizationPreference,
        active_domains: &BTreeSet<PackageDomain>,
        features: &BTreeSet<String>,
    ) -> Vec<ResolutionReasonV1> {
        let mut reasons = Vec::new();
        match preference {
            RealizationPreference::Exact(required) if realization.kind != required => {
                reasons.push(ResolutionReasonV1::ExplicitKindMismatch { required });
            }
            RealizationPreference::Auto
                if !composition
                    .policy
                    .realization_order
                    .contains(&realization.kind) =>
            {
                reasons.push(ResolutionReasonV1::AutomaticKindDisabled);
            }
            _ => {}
        }
        if !active_domains.is_subset(&realization.domains) {
            reasons.push(ResolutionReasonV1::DomainMismatch);
        }
        let target_matches = match realization.kind {
            latticeaxiom_compose::RealizationKind::Data => {
                realization.targets.is_empty()
                    || realization.targets.contains(&composition.policy.target)
            }
            _ => realization.targets.contains(&composition.policy.target),
        };
        if !target_matches {
            reasons.push(ResolutionReasonV1::TargetMismatch {
                target: composition.policy.target.clone(),
            });
        }
        let missing = realization
            .required_features
            .difference(features)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !missing.is_empty() {
            reasons.push(ResolutionReasonV1::UnknownFeatures { features: missing });
        }
        if realization.trust > composition.policy.maximum_trust {
            reasons.push(ResolutionReasonV1::TrustExceeded {
                required: realization.trust,
                allowed: composition.policy.maximum_trust,
            });
        }
        if let Some((interface, requirement)) =
            realization
                .interfaces
                .iter()
                .find(|(interface, requirement)| {
                    !requirement.optional
                        && !self
                            .host_compatibility
                            .as_ref()
                            .is_some_and(|host| host.supports(interface, requirement))
                })
        {
            reasons.push(ResolutionReasonV1::InterfaceRequirementUnavailable {
                interface: interface.clone(),
                requirements: vec![requirement.version.to_string()],
            });
        }
        let available_engine_build = self
            .host_compatibility
            .as_ref()
            .and_then(|host| host.engine_build_id);
        if realization.kind == latticeaxiom_compose::RealizationKind::EngineCoupledNative
            && let Some(required) = realization.engine_build
            && Some(required) != available_engine_build
        {
            reasons.push(ResolutionReasonV1::EngineBuildMismatch {
                required,
                available: available_engine_build,
            });
        }
        reasons
    }

    fn no_package_candidate_error(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        package: &PackageName,
        node: &Node,
    ) -> ResolutionError {
        let candidates = self
            .by_package
            .get(package)
            .into_iter()
            .flatten()
            .filter(|source_id| allowed.contains(*source_id))
            .filter_map(|source_id| self.candidates.get(source_id))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return ResolutionError::MissingPackage {
                package: package.clone(),
                context: Box::new(node.context(Vec::new())),
            };
        }

        if candidates.iter().all(|candidate| {
            node.requirements
                .iter()
                .filter_map(|requirement| requirement.range.as_ref())
                .any(|range| !range.matches(&candidate.package.version))
        }) {
            return ResolutionError::VersionConflict {
                package: package.clone(),
                context: Box::new(
                    node.context(
                        candidates
                            .iter()
                            .map(|item| candidate_summary(item))
                            .collect(),
                    ),
                ),
            };
        }

        let unknown = candidates
            .iter()
            .flat_map(|candidate| {
                requested_features(composition, package, node)
                    .into_iter()
                    .filter(|feature| !candidate.package.features.contains_key(feature))
            })
            .next();
        if let Some(feature) = unknown {
            return ResolutionError::UnknownFeature {
                package: package.clone(),
                feature,
                context: Box::new(
                    node.context(
                        candidates
                            .iter()
                            .map(|item| candidate_summary(item))
                            .collect(),
                    ),
                ),
            };
        }

        ResolutionError::RealizationUnavailable {
            package: package.clone(),
            context: Box::new(
                node.context(
                    candidates
                        .iter()
                        .map(|item| candidate_summary(item))
                        .collect(),
                ),
            ),
        }
    }

    fn error_for_candidate_reasons(
        &self,
        package: &PackageName,
        node: &Node,
        candidate: &PackageCandidate,
        reasons: &[ResolutionReasonV1],
    ) -> ResolutionError {
        if reasons
            .iter()
            .any(|reason| matches!(reason, ResolutionReasonV1::VersionMismatch { .. }))
        {
            return ResolutionError::VersionConflict {
                package: package.clone(),
                context: Box::new(node.context(self.available_summaries(package))),
            };
        }
        if let Some(feature) = reasons.iter().find_map(|reason| match reason {
            ResolutionReasonV1::UnknownFeatures { features } => features.iter().next().cloned(),
            _ => None,
        }) {
            return ResolutionError::UnknownFeature {
                package: package.clone(),
                feature,
                context: Box::new(node.context(vec![candidate_summary(candidate)])),
            };
        }
        if let Some(interface) = reasons.iter().find_map(|reason| match reason {
            ResolutionReasonV1::InterfaceRequirementUnavailable { interface, .. } => {
                Some(interface.clone())
            }
            _ => None,
        }) {
            return ResolutionError::InterfaceRequirementUnavailable {
                package: package.clone(),
                interface,
                context: Box::new(node.context(vec![candidate_summary(candidate)])),
            };
        }
        if let Some((required, available)) = reasons.iter().find_map(|reason| match reason {
            ResolutionReasonV1::EngineBuildMismatch {
                required,
                available,
            } => Some((*required, *available)),
            _ => None,
        }) {
            return ResolutionError::EngineBuildMismatch {
                package: package.clone(),
                required,
                available,
                context: Box::new(node.context(vec![candidate_summary(candidate)])),
            };
        }
        ResolutionError::RealizationUnavailable {
            package: package.clone(),
            context: Box::new(node.context(vec![candidate_summary(candidate)])),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn build_resolution(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        analysis: &CompleteAnalysis,
        trace: &SearchTrace,
        limits: ResolutionLimits,
    ) -> Result<PackageResolutionV1, ResolutionError> {
        let mut packages = BTreeMap::new();
        for (name, node) in &analysis.nodes {
            let Some(source_id) = analysis.selected_sources.get(name) else {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("complete graph has no source for {name}"),
                    context: Box::new(node.context(self.available_summaries(name))),
                });
            };
            let Some(candidate) = self.candidates.get(source_id) else {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("selected source {source_id} left the catalog"),
                    context: Box::new(node.context(self.available_summaries(name))),
                });
            };
            let active_domains = candidate
                .package
                .domains
                .intersection(&composition.projection_domains)
                .copied()
                .collect::<BTreeSet<_>>();
            let Some(realization) = self.select_realization(
                composition,
                candidate,
                &active_domains,
                &node.active_features,
            ) else {
                return Err(ResolutionError::RealizationUnavailable {
                    package: name.clone(),
                    context: Box::new(node.context(vec![candidate_summary(candidate)])),
                });
            };
            let dependencies = node
                .dependencies
                .iter()
                .filter_map(|(dependency_name, dependency)| {
                    analysis
                        .selected_sources
                        .get(dependency_name)
                        .and_then(|source| self.candidates.get(source))
                        .map(|selected| {
                            (
                                dependency_name.clone(),
                                ResolvedDependencyV1 {
                                    version: selected.package.version.clone(),
                                    features: dependency.features.clone(),
                                },
                            )
                        })
                })
                .collect();
            let schemas = candidate
                .package
                .registration
                .registrations
                .iter()
                .filter_map(|registration| registration.schema.clone())
                .collect();
            let source_path =
                CanonicalLogicalPath::new(candidate.source.path.clone()).map_err(|error| {
                    ResolutionError::InvalidCandidate {
                        candidate: Box::new(CandidateIdentityV1::from(candidate)),
                        reason: format!("source path is not canonical: {error}"),
                        context: Box::new(node.context(vec![candidate_summary(candidate)])),
                    }
                })?;
            packages.insert(
                name.clone(),
                ResolvedPackageV1 {
                    version: candidate.package.version.clone(),
                    source_kind: candidate.source_kind,
                    source_priority: candidate.source.priority,
                    source_id: candidate.source.source_id.clone(),
                    source_hash: candidate.source.content_hash,
                    source_provenance_hash: provenance_hash(&candidate.source.provenance)?,
                    package_provenance_hash: provenance_hash(&candidate.package.provenance)?,
                    source_path,
                    features: node.active_features.clone(),
                    domains: active_domains,
                    dependencies,
                    namespace_requests: candidate.package.namespace_requests.clone(),
                    schemas,
                    realization: ResolvedRealizationV1 {
                        id: realization.id.clone(),
                        kind: realization.kind,
                        target: composition.policy.target.clone(),
                        artifact: realization.artifact.clone(),
                        interfaces: realization.interfaces.clone(),
                        trust: realization.trust,
                        engine_build_id: realization.engine_build,
                        registration_fragment: realization.registration_fragment,
                    },
                },
            );
        }

        let namespace_grants = self.namespace_grants(composition, analysis)?;
        let capabilities = self.capability_receipts(analysis)?;
        let explanation =
            self.explanation(composition, allowed, analysis, &packages, trace, limits)?;
        let mut receipt = ResolutionReceiptV1 {
            schema_version: RESOLUTION_RECEIPT_SCHEMA_VERSION,
            composition_hash: composition.semantic_hash()?,
            composition_provenance_hash: composition.provenance_hash()?,
            resolution_intent_hash: resolution_intent_hash(composition)?,
            profile: composition.profile.clone(),
            profile_kind: composition.profile_kind,
            target: composition.policy.target.clone(),
            host_compatibility_hash: self.host_compatibility_hash()?,
            evaluation_policy: composition.policy.evaluation_policy.clone(),
            evaluation_limits: composition.policy.evaluation_limits,
            roots: composition.roots.keys().cloned().collect(),
            packages,
            capabilities,
            namespace_grants,
            explanation,
            resolution_hash: CanonicalHash::digest(b"pending-resolution"),
            receipt_hash: CanonicalHash::digest(b"pending-receipt"),
        };
        receipt.resolution_hash = receipt.recompute_resolution_hash()?;
        receipt.receipt_hash = receipt.recompute_receipt_hash()?;
        receipt
            .validate()
            .map_err(|source| ResolutionError::InvalidResolutionReceipt { source })?;
        let build_intent = Self::build_intent_from_receipt(&receipt)?;
        Ok(PackageResolutionV1 {
            receipt,
            build_intent,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn namespace_grants(
        &self,
        composition: &CompositionSpec,
        analysis: &CompleteAnalysis,
    ) -> Result<BTreeSet<NamespaceGrant>, ResolutionError> {
        let root_context = ResolutionFailureContext {
            package_chain: composition.roots.keys().cloned().collect(),
            requested: composition
                .policy
                .namespace_grants
                .values()
                .flatten()
                .map(ToString::to_string)
                .collect(),
            available: Vec::new(),
        };
        let profile_grantor =
            NamespaceGrantor::profile(composition.profile.clone()).map_err(|error| {
                namespace_authorization_error(
                    format!("profile namespace grantor is invalid: {error}"),
                    root_context.clone(),
                )
            })?;
        let mut grants = BTreeSet::new();
        let mut effective = analysis
            .nodes
            .keys()
            .cloned()
            .map(|package| (package, BTreeSet::<NamespaceGrantPattern>::new()))
            .collect::<BTreeMap<_, _>>();

        for (grantee, patterns) in &composition.policy.namespace_grants {
            if !composition.roots.contains_key(grantee) || !analysis.nodes.contains_key(grantee) {
                return Err(namespace_authorization_error(
                    format!("profile grant selects non-root or absent package {grantee}"),
                    ResolutionFailureContext {
                        package_chain: vec![grantee.clone()],
                        requested: patterns.iter().map(ToString::to_string).collect(),
                        available: composition.roots.keys().map(ToString::to_string).collect(),
                    },
                ));
            }
            let rows = namespace_grant_rows(&profile_grantor, grantee, patterns)
                .map_err(|reason| namespace_authorization_error(reason, root_context.clone()))?;
            grants.extend(rows);
            let Some(incoming) = effective.get_mut(grantee) else {
                return Err(namespace_authorization_error(
                    format!("profile namespace grant selects absent package {grantee}"),
                    root_context.clone(),
                ));
            };
            incoming.extend(patterns.iter().cloned());
        }

        let mut pending = Vec::new();
        for (grantor, node) in &analysis.nodes {
            let Some(source_id) = analysis.selected_sources.get(grantor) else {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("selected namespace grantor {grantor} has no source"),
                    context: Box::new(node.context(Vec::new())),
                });
            };
            let Some(candidate) = self.candidates.get(source_id) else {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("selected source {source_id} left the catalog"),
                    context: Box::new(node.context(Vec::new())),
                });
            };
            for (grantee, patterns) in &candidate.package.namespace_delegations {
                if node.dependencies.contains_key(grantee) {
                    pending.push((grantor.clone(), grantee.clone(), patterns.clone()));
                }
            }
        }

        while !pending.is_empty() {
            let mut next = Vec::new();
            let mut progress = false;
            for (grantor, grantee, patterns) in pending {
                let authorized = effective.get(&grantor).is_some_and(|incoming| {
                    patterns
                        .iter()
                        .all(|child| incoming.iter().any(|parent| parent.covers(child)))
                });
                if authorized {
                    let mut chain = analysis
                        .nodes
                        .get(&grantor)
                        .map_or_else(|| vec![grantor.clone()], Node::primary_chain);
                    chain.push(grantee.clone());
                    let context = ResolutionFailureContext {
                        package_chain: chain,
                        requested: patterns.iter().map(ToString::to_string).collect(),
                        available: effective
                            .get(&grantor)
                            .into_iter()
                            .flatten()
                            .map(ToString::to_string)
                            .collect(),
                    };
                    let rows = namespace_grant_rows(
                        &NamespaceGrantor::package(grantor.clone()),
                        &grantee,
                        &patterns,
                    )
                    .map_err(|reason| namespace_authorization_error(reason, context.clone()))?;
                    grants.extend(rows);
                    let Some(incoming) = effective.get_mut(&grantee) else {
                        return Err(namespace_authorization_error(
                            format!("namespace delegation selects absent package {grantee}"),
                            context,
                        ));
                    };
                    incoming.extend(patterns);
                    progress = true;
                } else {
                    next.push((grantor, grantee, patterns));
                }
            }
            if !progress {
                let Some((grantor, grantee, patterns)) = next.first() else {
                    return Err(namespace_authorization_error(
                        "namespace delegation fixed point made no progress".to_owned(),
                        root_context.clone(),
                    ));
                };
                let mut chain = analysis
                    .nodes
                    .get(grantor)
                    .map_or_else(|| vec![grantor.clone()], Node::primary_chain);
                chain.push(grantee.clone());
                return Err(namespace_authorization_error(
                    format!(
                        "namespace delegation from {grantor} to {grantee} widens effective authority"
                    ),
                    ResolutionFailureContext {
                        package_chain: chain,
                        requested: patterns.iter().map(ToString::to_string).collect(),
                        available: effective
                            .get(grantor)
                            .into_iter()
                            .flatten()
                            .map(ToString::to_string)
                            .collect(),
                    },
                ));
            }
            pending = next;
        }

        for (package, node) in &analysis.nodes {
            let Some(source_id) = analysis.selected_sources.get(package) else {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("selected package {package} has no source"),
                    context: Box::new(node.context(Vec::new())),
                });
            };
            let Some(candidate) = self.candidates.get(source_id) else {
                return Err(ResolutionError::InvalidSourceUniverse {
                    reason: format!("selected source {source_id} left the catalog"),
                    context: Box::new(node.context(Vec::new())),
                });
            };
            let incoming = effective.get(package).into_iter().flatten();
            let incoming = incoming.cloned().collect::<BTreeSet<_>>();
            if let Some(request) = candidate
                .package
                .namespace_requests
                .iter()
                .find(|request| !incoming.iter().any(|grant| grant.covers(request)))
            {
                return Err(namespace_authorization_error(
                    format!("package {package} namespace request {request} is not covered"),
                    ResolutionFailureContext {
                        package_chain: node.primary_chain(),
                        requested: candidate
                            .package
                            .namespace_requests
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                        available: incoming.iter().map(ToString::to_string).collect(),
                    },
                ));
            }
        }
        Ok(grants)
    }

    #[allow(clippy::too_many_lines)]
    fn capability_receipts(
        &self,
        analysis: &CompleteAnalysis,
    ) -> Result<BTreeMap<CapabilityId, CapabilityResolutionReceiptV1>, ResolutionError> {
        analysis
            .capabilities
            .iter()
            .map(|(capability, group)| {
                let demands = group
                    .demands
                    .iter()
                    .map(|demand| CapabilityDemandReceiptV1 {
                        required_by: demand.required_by.clone(),
                        range: demand.requirement.version.clone(),
                        explicit_provider: demand.requirement.provider.clone(),
                        cardinality: demand.requirement.cardinality,
                        domains: demand.requirement.domains.clone(),
                    })
                    .collect();
                let providers = analysis
                    .capability_providers
                    .get(capability)
                    .into_iter()
                    .flatten()
                    .map(|name| {
                        let source_id = analysis.selected_sources.get(name).ok_or_else(|| {
                            ResolutionError::InvalidSourceUniverse {
                                reason: format!(
                                    "capability {capability} selects absent provider {name}"
                                ),
                                context: Box::new(group.context(Vec::new())),
                            }
                        })?;
                        let candidate = self.candidates.get(source_id).ok_or_else(|| {
                            ResolutionError::InvalidSourceUniverse {
                                reason: format!("provider source {source_id} left the catalog"),
                                context: Box::new(group.context(Vec::new())),
                            }
                        })?;
                        let provision =
                            candidate.package.provides.get(capability).ok_or_else(|| {
                                ResolutionError::InvalidSourceUniverse {
                                    reason: format!(
                                        "selected package {name} no longer provides {capability}"
                                    ),
                                    context: Box::new(group.context(Vec::new())),
                                }
                            })?;
                        Ok(CapabilityProviderReceiptV1 {
                            package: name.clone(),
                            provided_version: provision.version.clone(),
                            cardinality: provision.cardinality,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolutionError>>()?;
                Ok((
                    capability.clone(),
                    CapabilityResolutionReceiptV1 { demands, providers },
                ))
            })
            .collect()
    }

    #[allow(clippy::too_many_lines)]
    fn explanation(
        &self,
        composition: &CompositionSpec,
        allowed: &BTreeSet<SourceId>,
        analysis: &CompleteAnalysis,
        packages: &BTreeMap<PackageName, ResolvedPackageV1>,
        trace: &SearchTrace,
        limits: ResolutionLimits,
    ) -> Result<ResolutionExplanationV1, ResolutionError> {
        let mut decisions = Vec::new();
        for (name, node) in &analysis.nodes {
            let selected_source = analysis.selected_sources.get(name);
            for source_id in self
                .by_package
                .get(name)
                .into_iter()
                .flatten()
                .filter(|source_id| allowed.contains(*source_id))
            {
                let Some(candidate) = self.candidates.get(source_id) else {
                    continue;
                };
                let selected = selected_source == Some(source_id);
                let mut reasons = self.package_candidate_reasons(composition, candidate, node);
                if selected {
                    reasons = node.selection_reasons();
                } else {
                    if let Some(failures) = trace.package_rejections.get(source_id) {
                        reasons.extend(
                            failures
                                .iter()
                                .cloned()
                                .map(|failure| ResolutionReasonV1::Backtracked { failure }),
                        );
                    }
                    if reasons.is_empty() {
                        reasons.push(ResolutionReasonV1::LowerPreference);
                    }
                }
                decisions.push(ResolutionDecisionV1 {
                    subject: ResolutionSubjectV1::Package {
                        candidate: CandidateIdentityV1::from(candidate),
                    },
                    outcome: if selected {
                        ResolutionOutcomeV1::Selected
                    } else {
                        ResolutionOutcomeV1::Discarded
                    },
                    reasons,
                });

                let active_domains = candidate
                    .package
                    .domains
                    .intersection(&composition.projection_domains)
                    .copied()
                    .collect::<BTreeSet<_>>();
                let features =
                    active_features(composition, &candidate.package, &node.requested_features);
                let selected_realization = packages
                    .get(name)
                    .map(|package| (&package.realization.id, &package.source_id));
                let preference = composition
                    .roots
                    .get(name)
                    .map_or(RealizationPreference::Auto, |request| request.realization);
                for realization in candidate.package.realizations.values() {
                    let is_selected = selected_realization.is_some_and(|(id, selected_id)| {
                        id == &realization.id && selected_id == source_id
                    });
                    let mut reasons = self.realization_reasons(
                        composition,
                        realization,
                        preference,
                        &active_domains,
                        &features,
                    );
                    if !is_selected && reasons.is_empty() {
                        reasons.push(ResolutionReasonV1::LowerPreference);
                    }
                    decisions.push(ResolutionDecisionV1 {
                        subject: ResolutionSubjectV1::Realization {
                            package: name.clone(),
                            candidate: CandidateIdentityV1::from(candidate),
                            realization: realization.id.clone(),
                            kind: realization.kind,
                        },
                        outcome: if is_selected {
                            ResolutionOutcomeV1::Selected
                        } else {
                            ResolutionOutcomeV1::Discarded
                        },
                        reasons,
                    });
                }
            }
        }

        for (capability, group) in &analysis.capabilities {
            let selected_packages = analysis
                .capability_providers
                .get(capability)
                .cloned()
                .unwrap_or_default();
            let mut candidates = allowed
                .iter()
                .filter_map(|source_id| {
                    let candidate = self.candidates.get(source_id)?;
                    let provision = candidate.package.provides.get(capability)?;
                    Some((source_id.clone(), candidate, provision))
                })
                .collect::<Vec<_>>();
            candidates
                .sort_by(|(left, _, _), (right, _, _)| self.compare_provider_ids(left, right));
            for (source_id, candidate, provision) in candidates {
                let package_selected = analysis.selected_sources.get(&candidate.package.name);
                let selected = package_selected == Some(&source_id)
                    && selected_packages.contains(&candidate.package.name);
                let mut reasons = Vec::new();
                if provision.cardinality != group.cardinality {
                    reasons.push(ResolutionReasonV1::CardinalityMismatch {
                        required: group.cardinality,
                        provided: provision.cardinality,
                    });
                }
                if !domains_active(&provision.domains, composition) {
                    reasons.push(ResolutionReasonV1::DomainMismatch);
                }
                let mismatches = group
                    .demands
                    .iter()
                    .filter(|demand| {
                        !demand.requirement.version.matches(&provision.version)
                            || demand
                                .requirement
                                .provider
                                .as_ref()
                                .is_some_and(|provider| provider != &candidate.package.name)
                    })
                    .map(|demand| demand.requirement.version.to_string())
                    .collect::<Vec<_>>();
                if !mismatches.is_empty() {
                    reasons.push(ResolutionReasonV1::InterfaceVersionMismatch {
                        requirements: mismatches,
                    });
                }
                if package_selected != Some(&source_id) {
                    reasons.push(ResolutionReasonV1::PackageNotSelected);
                }
                if !selected {
                    if let Some(failures) = trace
                        .provider_rejections
                        .get(&(capability.clone(), source_id.clone()))
                    {
                        reasons.extend(
                            failures
                                .iter()
                                .cloned()
                                .map(|failure| ResolutionReasonV1::Backtracked { failure }),
                        );
                    }
                    if reasons.is_empty() {
                        reasons.push(ResolutionReasonV1::LowerPreference);
                    }
                }
                decisions.push(ResolutionDecisionV1 {
                    subject: ResolutionSubjectV1::CapabilityProvider {
                        capability: capability.clone(),
                        candidate: CandidateIdentityV1::from(candidate),
                        provided_version: provision.version.clone(),
                    },
                    outcome: if selected {
                        ResolutionOutcomeV1::Selected
                    } else {
                        ResolutionOutcomeV1::Discarded
                    },
                    reasons,
                });
            }
        }
        let context = ResolutionFailureContext {
            package_chain: composition.roots.keys().cloned().collect(),
            requested: Vec::new(),
            available: Vec::new(),
        };
        check_count_limit(
            ResolutionBudget::ExplanationDecisions,
            limits.max_explanation_decisions,
            decisions.len(),
            &context,
        )?;
        Ok(ResolutionExplanationV1 { decisions })
    }

    fn compare_provider_ids(&self, left: &SourceId, right: &SourceId) -> Ordering {
        match (self.candidates.get(left), self.candidates.get(right)) {
            (Some(left), Some(right)) => right
                .package
                .version
                .precedence_cmp(&left.package.version)
                .then_with(|| compare_source_tie_break(left, right))
                .then_with(|| left.package.name.cmp(&right.package.name)),
            _ => left.cmp(right),
        }
    }

    fn available_summaries(&self, package: &PackageName) -> Vec<String> {
        self.by_package
            .get(package)
            .into_iter()
            .flatten()
            .filter_map(|source_id| self.candidates.get(source_id))
            .map(candidate_summary)
            .collect()
    }

    fn provider_summaries(
        &self,
        capability: &CapabilityId,
        allowed: &BTreeSet<SourceId>,
    ) -> Vec<String> {
        let mut providers = allowed
            .iter()
            .filter_map(|source_id| {
                let candidate = self.candidates.get(source_id)?;
                candidate.package.provides.get(capability).map(|provision| {
                    (
                        source_id.clone(),
                        format!(
                            "{} provides {} from {}",
                            candidate_summary(candidate),
                            provision.version,
                            capability
                        ),
                    )
                })
            })
            .collect::<Vec<_>>();
        providers.sort_by(|(left, _), (right, _)| self.compare_provider_ids(left, right));
        providers.into_iter().map(|(_, summary)| summary).collect()
    }
}

fn namespace_grant_rows(
    grantor: &NamespaceGrantor,
    grantee: &PackageName,
    patterns: &BTreeSet<NamespaceGrantPattern>,
) -> Result<BTreeSet<NamespaceGrant>, String> {
    let mut grouped = BTreeMap::<RegistrationNamespace, BTreeSet<NamespaceGrantPattern>>::new();
    for pattern in patterns {
        let namespace = RegistrationNamespace::new(pattern.namespace().to_owned())
            .map_err(|error| format!("invalid namespace grant pattern {pattern}: {error}"))?;
        grouped
            .entry(namespace)
            .or_default()
            .insert(pattern.clone());
    }
    grouped
        .into_iter()
        .map(|(namespace, grouped_patterns)| {
            NamespaceGrant::new(
                namespace,
                grantor.clone(),
                grantee.clone(),
                grouped_patterns,
            )
            .map_err(|error| error.to_string())
        })
        .collect()
}

fn namespace_authorization_error(
    reason: String,
    context: ResolutionFailureContext,
) -> ResolutionError {
    ResolutionError::NamespaceAuthorization {
        reason,
        context: Box::new(context),
    }
}

impl std::fmt::Display for CandidateIdentityV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} {} {} {}",
            self.package, self.version, self.source_id, self.source_hash
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SearchState {
    packages: BTreeMap<PackageName, SourceId>,
    providers: BTreeMap<CapabilityId, SourceId>,
}
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct SearchKey {
    packages: BTreeMap<PackageName, SourceId>,
    providers: BTreeMap<CapabilityId, SourceId>,
}

impl From<&SearchState> for SearchKey {
    fn from(state: &SearchState) -> Self {
        Self {
            packages: state.packages.clone(),
            providers: state.providers.clone(),
        }
    }
}

#[derive(Debug, Default)]
struct SearchTrace {
    package_rejections: BTreeMap<SourceId, BTreeSet<BacktrackingFailureV1>>,
    provider_rejections: BTreeMap<(CapabilityId, SourceId), BTreeSet<BacktrackingFailureV1>>,
}

impl SearchTrace {
    fn record_package(&mut self, source: SourceId, failure: BacktrackingFailureV1) {
        self.package_rejections
            .entry(source)
            .or_default()
            .insert(failure);
    }

    fn record_provider(
        &mut self,
        capability: CapabilityId,
        source: SourceId,
        failure: BacktrackingFailureV1,
    ) {
        self.provider_rejections
            .entry((capability, source))
            .or_default()
            .insert(failure);
    }
}

#[derive(Debug)]
struct SearchSession {
    limits: ResolutionLimits,
    active_states: BTreeSet<SearchKey>,
    failed_states: BTreeMap<SearchKey, BacktrackingFailureV1>,
    unique_states: BTreeSet<SearchKey>,
    state_transitions: u64,
    analysis_steps: u64,
    trace: SearchTrace,
}

impl SearchSession {
    fn new(limits: ResolutionLimits) -> Self {
        Self {
            limits,
            active_states: BTreeSet::new(),
            failed_states: BTreeMap::new(),
            unique_states: BTreeSet::new(),
            state_transitions: 0,
            analysis_steps: 0,
            trace: SearchTrace::default(),
        }
    }

    fn check_limit(
        budget: ResolutionBudget,
        limit: u64,
        observed: u64,
        context: &ResolutionFailureContext,
    ) -> Result<(), ResolutionError> {
        if observed > limit {
            return Err(ResolutionError::BudgetExceeded {
                budget,
                limit,
                observed,
                context: Box::new(context.clone()),
            });
        }
        Ok(())
    }

    fn observe_transition(
        &mut self,
        context: &ResolutionFailureContext,
    ) -> Result<(), ResolutionError> {
        let observed = self.state_transitions.saturating_add(1);
        Self::check_limit(
            ResolutionBudget::StateTransitions,
            self.limits.max_state_transitions,
            observed,
            context,
        )?;
        self.state_transitions = observed;
        Ok(())
    }

    fn observe_analysis_step(
        &mut self,
        context: &ResolutionFailureContext,
    ) -> Result<(), ResolutionError> {
        let observed = self.analysis_steps.saturating_add(1);
        Self::check_limit(
            ResolutionBudget::AnalysisSteps,
            self.limits.max_analysis_steps,
            observed,
            context,
        )?;
        self.analysis_steps = observed;
        Ok(())
    }

    fn enter_state(
        &mut self,
        key: &SearchKey,
        depth: u64,
        context: &ResolutionFailureContext,
    ) -> Result<(), ResolutionError> {
        Self::check_limit(
            ResolutionBudget::DecisionDepth,
            self.limits.max_decision_depth,
            depth,
            context,
        )?;
        if self.active_states.contains(key) {
            return Err(ResolutionError::StateCycle {
                state_hash: canonical_json_hash(key)?,
                context: Box::new(context.clone()),
            });
        }
        if let Some(failure) = self.failed_states.get(key) {
            return Err(ResolutionError::MemoizedStateFailure {
                failure: Box::new(failure.clone()),
                context: Box::new(context.clone()),
            });
        }
        if !self.unique_states.contains(key) {
            let observed = u64::try_from(self.unique_states.len())
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            Self::check_limit(
                ResolutionBudget::UniqueStates,
                self.limits.max_unique_states,
                observed,
                context,
            )?;
            self.unique_states.insert(key.clone());
        }
        self.active_states.insert(key.clone());
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RequirementOrigin {
    Root,
    Dependency(PackageName),
    Capability(CapabilityId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Requirement {
    range: Option<PackageVersionReq>,
    origin: RequirementOrigin,
    chain: Vec<PackageName>,
}

#[derive(Clone, Debug, Default)]
struct Node {
    requirements: Vec<Requirement>,
    requested_features: BTreeSet<String>,
    active_features: BTreeSet<String>,
    dependencies: BTreeMap<PackageName, latticeaxiom_compose::PackageDependency>,
    pinned_sources: BTreeSet<SourceId>,
}

impl Node {
    fn add_requirement(&mut self, requirement: Requirement) -> bool {
        if self.requirements.contains(&requirement) {
            false
        } else {
            self.requirements.push(requirement);
            self.requirements.sort_by(|left, right| {
                left.chain
                    .cmp(&right.chain)
                    .then_with(|| {
                        left.range
                            .as_ref()
                            .map(ToString::to_string)
                            .cmp(&right.range.as_ref().map(ToString::to_string))
                    })
                    .then_with(|| {
                        requirement_origin_key(&left.origin)
                            .cmp(&requirement_origin_key(&right.origin))
                    })
            });
            true
        }
    }

    fn primary_chain(&self) -> Vec<PackageName> {
        self.requirements
            .iter()
            .map(|requirement| requirement.chain.clone())
            .min()
            .unwrap_or_default()
    }

    fn context(&self, available: Vec<String>) -> ResolutionFailureContext {
        ResolutionFailureContext {
            package_chain: self.primary_chain(),
            requested: self
                .requirements
                .iter()
                .filter_map(|requirement| requirement.range.as_ref())
                .map(ToString::to_string)
                .collect(),
            available,
        }
    }

    fn selection_reasons(&self) -> Vec<ResolutionReasonV1> {
        let mut reasons = Vec::new();
        for requirement in &self.requirements {
            let reason = match &requirement.origin {
                RequirementOrigin::Root => ResolutionReasonV1::Root,
                RequirementOrigin::Dependency(required_by) => ResolutionReasonV1::Dependency {
                    required_by: required_by.clone(),
                },
                RequirementOrigin::Capability(capability) => ResolutionReasonV1::Capability {
                    capability: capability.clone(),
                },
            };
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
        reasons
    }
}

#[derive(Clone, Debug)]
struct CapabilityDemand {
    requirement: CapabilityRequirement,
    required_by: Option<PackageName>,
    chain: Vec<PackageName>,
}

#[derive(Clone, Debug)]
struct CapabilityGroup {
    cardinality: CapabilityCardinality,
    demands: Vec<CapabilityDemand>,
}

impl CapabilityGroup {
    const fn new(cardinality: CapabilityCardinality) -> Self {
        Self {
            cardinality,
            demands: Vec::new(),
        }
    }

    fn add_demand(&mut self, demand: CapabilityDemand) -> Result<(), ResolutionError> {
        if self.cardinality != demand.requirement.cardinality {
            return Err(ResolutionError::CapabilityCardinalityConflict {
                capability: demand.requirement.capability.clone(),
                required: self.cardinality,
                actual: 0,
                context: Box::new(self.context(Vec::new())),
            });
        }
        if !self.demands.iter().any(|existing| {
            existing.required_by == demand.required_by
                && existing.chain == demand.chain
                && existing.requirement == demand.requirement
        }) {
            self.demands.push(demand);
            self.demands.sort_by(|left, right| {
                left.chain
                    .cmp(&right.chain)
                    .then_with(|| left.required_by.cmp(&right.required_by))
                    .then_with(|| {
                        left.requirement
                            .version
                            .to_string()
                            .cmp(&right.requirement.version.to_string())
                    })
            });
        }
        Ok(())
    }

    fn context(&self, available: Vec<String>) -> ResolutionFailureContext {
        ResolutionFailureContext {
            package_chain: self
                .demands
                .iter()
                .map(|demand| demand.chain.clone())
                .min()
                .unwrap_or_default(),
            requested: self
                .demands
                .iter()
                .map(|demand| demand.requirement.version.to_string())
                .collect(),
            available,
        }
    }
}

#[derive(Debug)]
enum AnalysisProgress {
    NeedPackage {
        package: PackageName,
        node: Node,
        /// Canonical currently reachable package and provider choices.
        state: SearchState,
    },
    NeedProvider {
        capability: CapabilityId,
        candidates: Vec<SourceId>,
        context: ResolutionFailureContext,
        /// Canonical currently reachable package and provider choices.
        state: SearchState,
    },
    Complete(CompleteAnalysis),
}

#[derive(Debug)]
struct CompleteAnalysis {
    nodes: BTreeMap<PackageName, Node>,
    /// Canonical currently reachable package and provider choices.
    state: SearchState,
    capabilities: BTreeMap<CapabilityId, CapabilityGroup>,
    capability_providers: BTreeMap<CapabilityId, Vec<PackageName>>,
    selected_sources: BTreeMap<PackageName, SourceId>,
}

fn check_count_limit(
    budget: ResolutionBudget,
    limit: u64,
    observed: usize,
    context: &ResolutionFailureContext,
) -> Result<(), ResolutionError> {
    let observed = u64::try_from(observed).unwrap_or(u64::MAX);
    if observed > limit {
        return Err(ResolutionError::BudgetExceeded {
            budget,
            limit,
            observed,
            context: Box::new(context.clone()),
        });
    }
    Ok(())
}

fn check_graph_limits(
    session: &SearchSession,
    nodes: &BTreeMap<PackageName, Node>,
    capabilities: &BTreeMap<CapabilityId, CapabilityGroup>,
    context: &ResolutionFailureContext,
) -> Result<(), ResolutionError> {
    check_count_limit(
        ResolutionBudget::LivePackages,
        session.limits.max_live_packages,
        nodes.len(),
        context,
    )?;
    check_count_limit(
        ResolutionBudget::ActiveCapabilities,
        session.limits.max_active_capabilities,
        capabilities.len(),
        context,
    )?;
    let dependency_edges = nodes
        .values()
        .map(|node| node.dependencies.len())
        .fold(0usize, usize::saturating_add);
    let capability_edges = capabilities
        .values()
        .map(|group| group.demands.len())
        .fold(0usize, usize::saturating_add);
    check_count_limit(
        ResolutionBudget::ActiveEdges,
        session.limits.max_active_edges,
        dependency_edges.saturating_add(capability_edges),
        context,
    )
}

#[allow(clippy::too_many_lines)]
fn backtracking_failure(error: &ResolutionError) -> Option<BacktrackingFailureV1> {
    let (code, context) = match error {
        ResolutionError::StateCycle { context, .. } => {
            (ResolutionFailureCodeV1::SearchStateCycle, context.as_ref())
        }
        ResolutionError::MemoizedStateFailure { failure, .. } => {
            return Some(failure.as_ref().clone());
        }
        ResolutionError::MissingPackage { package, context } => (
            ResolutionFailureCodeV1::MissingPackage {
                package: package.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::VersionConflict { package, context } => (
            ResolutionFailureCodeV1::VersionConflict {
                package: package.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::UnknownFeature {
            package,
            feature,
            context,
        } => (
            ResolutionFailureCodeV1::UnknownFeature {
                package: package.clone(),
                feature: feature.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::DependencyCycle { context } => {
            (ResolutionFailureCodeV1::DependencyCycle, context.as_ref())
        }
        ResolutionError::MissingCapability {
            capability,
            context,
        } => (
            ResolutionFailureCodeV1::MissingCapability {
                capability: capability.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::CapabilityCardinalityConflict {
            capability,
            context,
            ..
        } => (
            ResolutionFailureCodeV1::CapabilityCardinalityConflict {
                capability: capability.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::ConflictingCapabilityProviders {
            capability,
            context,
        } => (
            ResolutionFailureCodeV1::ConflictingCapabilityProviders {
                capability: capability.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::RealizationUnavailable { package, context } => (
            ResolutionFailureCodeV1::RealizationUnavailable {
                package: package.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::PinnedSourceUnavailable {
            package, context, ..
        } => (
            ResolutionFailureCodeV1::PinnedSourceUnavailable {
                package: package.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::InterfaceRequirementUnavailable {
            interface, context, ..
        } => (
            ResolutionFailureCodeV1::InterfaceRequirementUnavailable {
                interface: interface.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::EngineBuildMismatch {
            package, context, ..
        } => (
            ResolutionFailureCodeV1::EngineBuildMismatch {
                package: package.clone(),
            },
            context.as_ref(),
        ),
        ResolutionError::NamespaceAuthorization { context, .. } => (
            ResolutionFailureCodeV1::NamespaceAuthorization,
            context.as_ref(),
        ),
        ResolutionError::InvalidComposition { .. }
        | ResolutionError::InvalidHostCompatibility { .. }
        | ResolutionError::HostTargetMismatch { .. }
        | ResolutionError::InvalidLimits { .. }
        | ResolutionError::InvalidPackage { .. }
        | ResolutionError::InvalidCandidate { .. }
        | ResolutionError::InvalidSourceUniverse { .. }
        | ResolutionError::BudgetExceeded { .. }
        | ResolutionError::UnsupportedPreBuildSurface { .. }
        | ResolutionError::Canonical(_)
        | ResolutionError::InvalidResolutionReceipt { .. }
        | ResolutionError::ResolutionReceiptMismatch { .. } => return None,
    };
    Some(BacktrackingFailureV1 {
        code,
        package_chain: context.package_chain.clone(),
        requested: context.requested.clone(),
    })
}

fn is_global_fatal(error: &ResolutionError) -> bool {
    matches!(
        error,
        ResolutionError::InvalidComposition { .. }
            | ResolutionError::InvalidHostCompatibility { .. }
            | ResolutionError::HostTargetMismatch { .. }
            | ResolutionError::InvalidLimits { .. }
            | ResolutionError::InvalidPackage { .. }
            | ResolutionError::InvalidCandidate { .. }
            | ResolutionError::InvalidSourceUniverse { .. }
            | ResolutionError::BudgetExceeded { .. }
            | ResolutionError::UnsupportedPreBuildSurface { .. }
            | ResolutionError::Canonical(_)
            | ResolutionError::InvalidResolutionReceipt { .. }
            | ResolutionError::ResolutionReceiptMismatch { .. }
    )
}

fn primary_branch_error(
    mut failures: Vec<(BacktrackingFailureV1, ResolutionError)>,
) -> Option<ResolutionError> {
    failures.sort_by(|left, right| left.0.cmp(&right.0));
    failures.into_iter().next().map(|(_, error)| error)
}

fn selected_source_explanation(receipt: &ResolutionReceiptV1) -> Vec<&ResolutionDecisionV1> {
    let selected_sources = receipt
        .packages
        .values()
        .map(|package| package.source_id.clone())
        .collect::<BTreeSet<_>>();
    receipt
        .explanation
        .decisions
        .iter()
        .filter(|decision| {
            let candidate = match &decision.subject {
                ResolutionSubjectV1::Package { candidate }
                | ResolutionSubjectV1::Realization { candidate, .. }
                | ResolutionSubjectV1::CapabilityProvider { candidate, .. } => candidate,
            };
            selected_sources.contains(&candidate.source_id)
        })
        .collect()
}

fn resolution_intent_hash(composition: &CompositionSpec) -> Result<CanonicalHash, ResolutionError> {
    let mut intent = composition.clone();
    intent.sources.clear();
    Ok(intent.semantic_hash()?)
}

fn validate_candidate_binding(candidate: &PackageCandidate) -> Result<(), ResolutionError> {
    let identity = CandidateIdentityV1::from(candidate);
    let context = ResolutionFailureContext {
        package_chain: vec![candidate.package.name.clone()],
        requested: vec![format!(
            "{} {}",
            candidate.source.package, candidate.source.version
        )],
        available: vec![candidate_summary(candidate)],
    };
    if candidate.source.package != candidate.package.name {
        return Err(ResolutionError::InvalidCandidate {
            candidate: Box::new(identity),
            reason: "source package differs from PackageSpec".to_owned(),
            context: Box::new(context),
        });
    }
    if candidate.source.version != candidate.package.version {
        return Err(ResolutionError::InvalidCandidate {
            candidate: Box::new(identity),
            reason: "source version differs from PackageSpec".to_owned(),
            context: Box::new(context),
        });
    }
    if candidate.package.provenance.source_id() != &candidate.source.source_id {
        return Err(ResolutionError::InvalidCandidate {
            candidate: Box::new(identity),
            reason: "PackageSpec provenance belongs to another source".to_owned(),
            context: Box::new(context),
        });
    }
    Ok(())
}

fn compare_package_candidates(left: &PackageCandidate, right: &PackageCandidate) -> Ordering {
    right
        .package
        .version
        .precedence_cmp(&left.package.version)
        .then_with(|| compare_source_tie_break(left, right))
}

fn compare_source_tie_break(left: &PackageCandidate, right: &PackageCandidate) -> Ordering {
    right
        .source
        .priority
        .cmp(&left.source.priority)
        .then_with(|| left.source_kind.cmp(&right.source_kind))
        .then_with(|| left.source.source_id.cmp(&right.source.source_id))
        .then_with(|| left.source.content_hash.cmp(&right.source.content_hash))
}

fn candidate_summary(candidate: &PackageCandidate) -> String {
    format!(
        "{} {} from {} ({:?}, priority {}, {})",
        candidate.package.name,
        candidate.package.version,
        candidate.source.source_id,
        candidate.source_kind,
        candidate.source.priority,
        candidate.source.content_hash
    )
}

fn requested_features(
    composition: &CompositionSpec,
    package: &PackageName,
    node: &Node,
) -> BTreeSet<String> {
    let mut requested = node.requested_features.clone();
    if let Some(profile_features) = composition.features.get(package) {
        requested.extend(profile_features.iter().cloned());
    }
    requested
}

fn active_features(
    composition: &CompositionSpec,
    package: &PackageSpec,
    dependency_features: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut active: BTreeSet<String> = if let Some(overrides) =
        composition.features.get(&package.name)
    {
        overrides
            .iter()
            .filter(|feature| {
                package
                    .features
                    .get(*feature)
                    .is_some_and(|spec| domains_active(&spec.domains, composition))
            })
            .cloned()
            .collect()
    } else {
        package
            .features
            .iter()
            .filter(|(_, feature)| feature.default && domains_active(&feature.domains, composition))
            .map(|(name, _)| name.clone())
            .collect()
    };
    active.extend(
        dependency_features
            .iter()
            .filter(|feature| {
                package
                    .features
                    .get(*feature)
                    .is_some_and(|spec| domains_active(&spec.domains, composition))
            })
            .cloned(),
    );
    if let Some(root) = composition.roots.get(&package.name) {
        active.extend(
            root.features
                .iter()
                .filter(|feature| {
                    package
                        .features
                        .get(*feature)
                        .is_some_and(|spec| domains_active(&spec.domains, composition))
                })
                .cloned(),
        );
    }
    active
}

fn domains_active(domains: &BTreeSet<PackageDomain>, composition: &CompositionSpec) -> bool {
    !domains.is_disjoint(&composition.projection_domains)
}

fn realization_policy_rank(
    composition: &CompositionSpec,
    realization: &RealizationSpec,
    preference: RealizationPreference,
) -> usize {
    match preference {
        RealizationPreference::Exact(_) => 0,
        RealizationPreference::Auto => composition
            .policy
            .realization_order
            .iter()
            .position(|kind| *kind == realization.kind)
            .unwrap_or(usize::MAX),
    }
}

fn provider_satisfies_demands(
    candidate: &PackageCandidate,
    provision: &CapabilityProvision,
    group: &CapabilityGroup,
) -> bool {
    group.demands.iter().all(|demand| {
        demand.requirement.version.matches(&provision.version)
            && demand
                .requirement
                .provider
                .as_ref()
                .is_none_or(|provider| provider == &candidate.package.name)
    })
}

fn requirement_origin_key(origin: &RequirementOrigin) -> String {
    match origin {
        RequirementOrigin::Root => "0-root".to_owned(),
        RequirementOrigin::Dependency(package) => format!("1-dependency:{package}"),
        RequirementOrigin::Capability(capability) => format!("2-capability:{capability}"),
    }
}

fn dependency_cycle(
    nodes: &BTreeMap<PackageName, Node>,
    capabilities: &BTreeMap<CapabilityId, CapabilityGroup>,
    providers: &BTreeMap<CapabilityId, Vec<PackageName>>,
) -> Option<Vec<PackageName>> {
    let mut adjacency = nodes
        .iter()
        .map(|(name, node)| {
            (
                name.clone(),
                node.dependencies.keys().cloned().collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    for (capability, group) in capabilities {
        let selected = providers.get(capability).cloned().unwrap_or_default();
        for demand in &group.demands {
            let Some(required_by) = &demand.required_by else {
                continue;
            };
            for provider in &selected {
                if provider != required_by {
                    adjacency
                        .entry(required_by.clone())
                        .or_default()
                        .insert(provider.clone());
                }
            }
        }
    }

    let mut visited = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut stack = Vec::new();
    for node in adjacency.keys() {
        if let Some(cycle) = visit_cycle(node, &adjacency, &mut visited, &mut active, &mut stack) {
            return Some(cycle);
        }
    }
    None
}

fn visit_cycle(
    node: &PackageName,
    adjacency: &BTreeMap<PackageName, BTreeSet<PackageName>>,
    visited: &mut BTreeSet<PackageName>,
    active: &mut BTreeSet<PackageName>,
    stack: &mut Vec<PackageName>,
) -> Option<Vec<PackageName>> {
    if active.contains(node) {
        let start = stack.iter().position(|item| item == node).unwrap_or(0);
        let mut cycle = stack[start..].to_vec();
        cycle.push(node.clone());
        return Some(cycle);
    }
    if !visited.insert(node.clone()) {
        return None;
    }
    active.insert(node.clone());
    stack.push(node.clone());
    for dependency in adjacency.get(node).into_iter().flatten() {
        if let Some(cycle) = visit_cycle(dependency, adjacency, visited, active, stack) {
            return Some(cycle);
        }
    }
    stack.pop();
    active.remove(node);
    None
}
