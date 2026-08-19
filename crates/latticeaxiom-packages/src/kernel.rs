use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use latticeaxiom_compose::{
    CONTRACT_SCHEMA_VERSION, CapabilityId, CompositionSpec, Evaluator, NICKEL_EVALUATOR_ID,
    PackageId, PackageManifest, PackageRequest, SourceRequest,
};

use crate::canonical::{graph_hash, hash_contract_library, hash_directory};
use crate::model::{
    CapabilityBinding, LOCK_SCHEMA_VERSION, LockedGameGraph, LockedPackage, LockedSource,
    PUBLISHED_DESCRIPTOR_SCHEMA_VERSION, PackageError, PublishedPackageDescriptor, locked_block,
};

/// Exact-local-source package resolver for milestone 2.
#[derive(Clone, Debug)]
pub struct PackageKernel {
    project_root: PathBuf,
    evaluator: Evaluator,
}

impl PackageKernel {
    /// Creates a package kernel rooted at one controlled project directory.
    #[must_use]
    pub fn new(project_root: impl Into<PathBuf>, evaluator: Evaluator) -> Self {
        Self {
            project_root: project_root.into(),
            evaluator,
        }
    }

    /// Resolves a typed composition into one exact, deterministic game graph.
    ///
    /// # Errors
    ///
    /// Returns [`PackageError`] if a source escapes the project root, source
    /// evaluation fails, an exact identity or version disagrees, dependencies
    /// do not close, a cycle exists, or a capability cannot be selected.
    pub fn resolve(&self, spec: &CompositionSpec) -> Result<LockedGameGraph, PackageError> {
        let canonical_root =
            fs::canonicalize(&self.project_root).map_err(|source| PackageError::Io {
                path: self.project_root.clone(),
                source,
            })?;
        let mut candidates = Vec::with_capacity(spec.package_requests.len());
        for request in &spec.package_requests {
            let (source, directory) = Self::resolve_source(&canonical_root, &request.source)?;
            let manifest = self
                .evaluator
                .evaluate_package(directory.join("package.ncl"))?;
            let content_sha256 = hash_directory(&directory)?;
            candidates.push(Candidate {
                request: request.clone(),
                manifest,
                source,
                content_sha256,
            });
        }
        let contract_sha256 = hash_contract_library(self.evaluator.library_root())?;
        assemble_graph(spec, candidates, contract_sha256)
    }

    /// Produces the typed descriptor used by the future folder loader.
    ///
    /// # Errors
    ///
    /// Returns [`PackageError`] if `package_dir` escapes the project root,
    /// cannot be evaluated, or cannot be hashed deterministically.
    pub fn describe_package(
        &self,
        package_dir: impl AsRef<Path>,
    ) -> Result<PublishedPackageDescriptor, PackageError> {
        let canonical_root =
            fs::canonicalize(&self.project_root).map_err(|source| PackageError::Io {
                path: self.project_root.clone(),
                source,
            })?;
        let requested = package_dir.as_ref();
        let joined = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            canonical_root.join(requested)
        };
        let directory = fs::canonicalize(&joined).map_err(|source| PackageError::Io {
            path: joined,
            source,
        })?;
        ensure_within_root(&canonical_root, &directory)?;
        if !directory.is_dir() {
            return Err(PackageError::SourceNotDirectory { path: directory });
        }
        let manifest = self
            .evaluator
            .evaluate_package(directory.join("package.ncl"))?;
        let content_sha256 = hash_directory(&directory)?;
        Ok(PublishedPackageDescriptor {
            schema_version: PUBLISHED_DESCRIPTOR_SCHEMA_VERSION,
            manifest,
            content_sha256,
            target: None,
            toolchain: None,
        })
    }

    fn resolve_source(
        canonical_root: &Path,
        request: &SourceRequest,
    ) -> Result<(LockedSource, PathBuf), PackageError> {
        match request {
            SourceRequest::Path { path } => {
                let joined = canonical_root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
                let directory = fs::canonicalize(&joined).map_err(|source| PackageError::Io {
                    path: joined,
                    source,
                })?;
                ensure_within_root(canonical_root, &directory)?;
                if !directory.is_dir() {
                    return Err(PackageError::SourceNotDirectory { path: directory });
                }
                Ok((LockedSource::Path { path: path.clone() }, directory))
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    request: PackageRequest,
    manifest: PackageManifest,
    source: LockedSource,
    content_sha256: String,
}

fn assemble_graph(
    spec: &CompositionSpec,
    candidates: Vec<Candidate>,
    contract_sha256: String,
) -> Result<LockedGameGraph, PackageError> {
    let mut by_id = BTreeMap::new();
    for candidate in candidates {
        if candidate.request.id != candidate.manifest.package.id {
            return Err(PackageError::PackageIdentityMismatch {
                requested: candidate.request.id,
                declared: candidate.manifest.package.id,
            });
        }
        if candidate.request.version != candidate.manifest.package.version {
            return Err(PackageError::PackageVersionMismatch {
                id: candidate.request.id,
                requested: candidate.request.version,
                declared: candidate.manifest.package.version,
            });
        }
        if !candidate
            .manifest
            .realizations
            .contains(&candidate.request.realization)
        {
            return Err(PackageError::UnsupportedRealization {
                id: candidate.request.id,
                realization: candidate.request.realization,
                available: candidate.manifest.realizations,
            });
        }
        by_id.insert(candidate.request.id.clone(), candidate);
    }

    validate_dependencies(&by_id)?;
    reject_cycles(&by_id)?;
    let capability_bindings = resolve_capabilities(spec, &by_id)?;
    let packages = by_id
        .into_values()
        .map(|candidate| {
            let mut dependencies = candidate
                .manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.id.clone())
                .collect::<Vec<_>>();
            dependencies.sort();
            let mut capabilities = candidate.manifest.provides.capabilities;
            capabilities.sort();
            let mut blocks = candidate
                .manifest
                .provides
                .blocks
                .iter()
                .map(|block| locked_block(&candidate.request.id, block))
                .collect::<Vec<_>>();
            blocks.sort_by(|left, right| left.key.cmp(&right.key));
            LockedPackage {
                id: candidate.request.id,
                version: candidate.request.version,
                source: candidate.source,
                content_sha256: candidate.content_sha256,
                dependencies,
                capabilities,
                blocks,
                realization: candidate.request.realization,
            }
        })
        .collect::<Vec<_>>();

    let mut graph = LockedGameGraph {
        schema_version: LOCK_SCHEMA_VERSION,
        composition_schema_version: spec.schema_version,
        contract_schema_version: CONTRACT_SCHEMA_VERSION,
        nickel_evaluator: NICKEL_EVALUATOR_ID.to_owned(),
        contract_sha256,
        graph_sha256: String::new(),
        root_profile: spec.root_profile.clone(),
        target: spec.policy.target.clone(),
        toolchain: spec.policy.toolchain.clone(),
        parameters: spec.parameters.clone(),
        packages,
        capability_bindings,
    };
    graph.graph_sha256 = graph_hash(&graph)?;
    Ok(graph)
}

fn validate_dependencies(candidates: &BTreeMap<PackageId, Candidate>) -> Result<(), PackageError> {
    for candidate in candidates.values() {
        let mut dependencies = candidate.manifest.dependencies.iter().collect::<Vec<_>>();
        dependencies.sort_by(|left, right| left.id.cmp(&right.id));
        for dependency in dependencies {
            let Some(selected) = candidates.get(&dependency.id) else {
                return Err(PackageError::MissingDependency {
                    package: candidate.request.id.clone(),
                    dependency: dependency.id.clone(),
                });
            };
            if dependency.version != selected.manifest.package.version {
                return Err(PackageError::DependencyVersionMismatch {
                    package: candidate.request.id.clone(),
                    dependency: dependency.id.clone(),
                    required: dependency.version.clone(),
                    selected: selected.manifest.package.version.clone(),
                });
            }
        }
    }
    Ok(())
}

fn reject_cycles(candidates: &BTreeMap<PackageId, Candidate>) -> Result<(), PackageError> {
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    for id in candidates.keys() {
        visit(id, candidates, &mut visiting, &mut visited, &mut stack)?;
    }
    Ok(())
}

fn visit(
    id: &PackageId,
    candidates: &BTreeMap<PackageId, Candidate>,
    visiting: &mut BTreeSet<PackageId>,
    visited: &mut BTreeSet<PackageId>,
    stack: &mut Vec<PackageId>,
) -> Result<(), PackageError> {
    if visited.contains(id) {
        return Ok(());
    }
    if visiting.contains(id) {
        let start = stack.iter().position(|entry| entry == id).unwrap_or(0);
        let mut cycle = stack[start..].to_vec();
        cycle.push(id.clone());
        return Err(PackageError::DependencyCycle { cycle });
    }
    visiting.insert(id.clone());
    stack.push(id.clone());
    if let Some(candidate) = candidates.get(id) {
        let mut dependencies = candidate
            .manifest
            .dependencies
            .iter()
            .map(|dependency| dependency.id.clone())
            .collect::<Vec<_>>();
        dependencies.sort();
        for dependency in dependencies {
            visit(&dependency, candidates, visiting, visited, stack)?;
        }
    }
    stack.pop();
    visiting.remove(id);
    visited.insert(id.clone());
    Ok(())
}

fn resolve_capabilities(
    spec: &CompositionSpec,
    candidates: &BTreeMap<PackageId, Candidate>,
) -> Result<Vec<CapabilityBinding>, PackageError> {
    let mut providers: BTreeMap<CapabilityId, Vec<PackageId>> = BTreeMap::new();
    for candidate in candidates.values() {
        for capability in &candidate.manifest.provides.capabilities {
            providers
                .entry(capability.clone())
                .or_default()
                .push(candidate.request.id.clone());
        }
    }
    for candidates in providers.values_mut() {
        candidates.sort();
        candidates.dedup();
    }

    let mut requirements = spec.capability_requirements.iter().collect::<Vec<_>>();
    requirements.sort_by(|left, right| {
        left.capability
            .cmp(&right.capability)
            .then_with(|| left.provider.cmp(&right.provider))
    });
    let mut bindings = Vec::with_capacity(requirements.len());
    for requirement in requirements {
        let available = providers
            .get(&requirement.capability)
            .cloned()
            .unwrap_or_default();
        let provider = if let Some(explicit) = &requirement.provider {
            if !available.contains(explicit) {
                return Err(PackageError::InvalidCapabilityProvider {
                    capability: requirement.capability.clone(),
                    provider: explicit.clone(),
                });
            }
            explicit.clone()
        } else {
            match available.as_slice() {
                [] => {
                    return Err(PackageError::MissingCapability {
                        capability: requirement.capability.clone(),
                    });
                }
                [only] => only.clone(),
                _ => {
                    return Err(PackageError::AmbiguousCapability {
                        capability: requirement.capability.clone(),
                        candidates: available,
                    });
                }
            }
        };
        bindings.push(CapabilityBinding {
            capability: requirement.capability.clone(),
            provider,
        });
    }
    Ok(bindings)
}

fn ensure_within_root(root: &Path, path: &Path) -> Result<(), PackageError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(PackageError::SourceOutsideRoot {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use latticeaxiom_compose::{
        BlockDeclaration, CapabilityId, CapabilityRequirement, CompositionPolicy, CompositionSpec,
        NativeCodePolicy, PackageDeclaration, PackageId, PackageManifest, PackageRequest,
        PackageVersion, ProfileDeclaration, ProvidedContent, Realization, SourceRequest,
    };
    use proptest::prelude::*;

    use super::{Candidate, assemble_graph};
    use crate::{LockedSource, PackageError, canonical_lock_bytes};

    fn package_id(value: &str) -> PackageId {
        PackageId::new(value).expect("test package ids are canonical")
    }

    fn version() -> PackageVersion {
        PackageVersion::new("1.0.0").expect("test version is exact")
    }

    fn candidate(id: &str, dependency: Option<&str>) -> Candidate {
        let id = package_id(id);
        let dependencies = dependency
            .map(|dependency| {
                vec![latticeaxiom_compose::DependencyRequest {
                    id: package_id(dependency),
                    version: version(),
                }]
            })
            .unwrap_or_default();
        Candidate {
            request: PackageRequest {
                id: id.clone(),
                version: version(),
                source: SourceRequest::Path {
                    path: format!("packages/{id}"),
                },
                realization: Realization::Data,
            },
            manifest: PackageManifest {
                schema_version: 1,
                package: PackageDeclaration {
                    id: id.clone(),
                    version: version(),
                },
                dependencies,
                provides: ProvidedContent {
                    capabilities: Vec::new(),
                    blocks: vec![BlockDeclaration {
                        id: "stone".to_owned(),
                        display_name: "Stone".to_owned(),
                        solid: true,
                        is_air: false,
                        color: [1, 2, 3, 255],
                    }],
                },
                realizations: vec![Realization::Data],
            },
            source: LockedSource::Path {
                path: format!("packages/{id}"),
            },
            content_sha256: format!("hash-{id}"),
        }
    }

    fn spec(candidates: &[Candidate]) -> CompositionSpec {
        CompositionSpec {
            schema_version: 1,
            root_profile: ProfileDeclaration {
                id: package_id("example.game"),
                version: version(),
            },
            package_requests: candidates
                .iter()
                .map(|candidate| candidate.request.clone())
                .collect(),
            capability_requirements: Vec::<CapabilityRequirement>::new(),
            realization_preferences: vec![Realization::Data],
            parameters: BTreeMap::new(),
            policy: CompositionPolicy {
                native_code: NativeCodePolicy::Deny,
                target: "test-target".to_owned(),
                toolchain: "test-toolchain".to_owned(),
                authoritative: Vec::new(),
            },
        }
    }

    proptest! {
        #[test]
        fn discovery_order_does_not_change_lock_bytes(order in prop::array::uniform3(any::<u8>())) {
            let all = [
                candidate("example.alpha", None),
                candidate("example.beta", Some("example.alpha")),
                candidate("example.gamma", None),
            ];
            let baseline_spec = spec(&all);
            let baseline = assemble_graph(&baseline_spec, all.to_vec(), "contract-hash".to_owned())
                .expect("baseline graph must resolve");
            let mut indices = [0_usize, 1, 2];
            indices.sort_by_key(|index| (order[*index], *index));
            let shuffled = indices.map(|index| all[index].clone()).to_vec();
            let graph = assemble_graph(&baseline_spec, shuffled, "contract-hash".to_owned())
                .expect("shuffled graph must resolve");
            prop_assert_eq!(
                canonical_lock_bytes(&baseline).expect("baseline serializes"),
                canonical_lock_bytes(&graph).expect("shuffled graph serializes")
            );
        }
    }

    #[test]
    fn repository_graph_is_resolvable() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let evaluator = latticeaxiom_compose::Evaluator::new(root.join("nickel"));
        let spec = evaluator
            .evaluate_game(root.join("profiles/dev.ncl"))
            .expect("checked-in profile evaluates");
        let graph = super::PackageKernel::new(&root, evaluator)
            .resolve(&spec)
            .expect("checked-in exact package graph resolves");
        assert_eq!(graph.packages.len(), 1);
        assert_eq!(graph.capability_bindings.len(), 1);
        assert_eq!(
            canonical_lock_bytes(&graph).expect("resolved graph serializes canonically"),
            include_bytes!("../../../latticeaxiom.lock"),
            "checked-in lock is the golden canonical encoding"
        );
    }

    #[test]
    fn missing_and_ambiguous_capabilities_have_repair_advice() {
        let capability =
            CapabilityId::new("content.blocks@1").expect("test capability id is canonical");
        let mut candidates = vec![
            candidate("example.alpha", None),
            candidate("example.beta", None),
        ];
        let mut composition = spec(&candidates);
        composition.capability_requirements = vec![CapabilityRequirement {
            capability: capability.clone(),
            provider: None,
        }];
        let missing = assemble_graph(&composition, candidates.clone(), "contract-hash".to_owned())
            .expect_err("a missing capability must fail before gameplay");
        assert!(matches!(missing, PackageError::MissingCapability { .. }));
        assert!(missing.to_string().contains("add a package"));

        for candidate in &mut candidates {
            candidate
                .manifest
                .provides
                .capabilities
                .push(capability.clone());
        }
        let ambiguous = assemble_graph(&composition, candidates, "contract-hash".to_owned())
            .expect_err("multiple implicit providers must require an explicit selection");
        assert!(matches!(
            ambiguous,
            PackageError::AmbiguousCapability { .. }
        ));
        assert!(ambiguous.to_string().contains("set `provider`"));
    }
}
