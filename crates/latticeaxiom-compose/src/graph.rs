//! Resolved package graph and executable build-plan DTOs.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CapabilityId, PackageName, PackageVersion,
    PackageVersionReq, SchemaId, SourceId, StableId, TargetTriple, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{NickelEvaluationLimits, PackageDomain, RealizationId, RealizationKind};

/// Current lock-file schema version.
pub const LOCK_SCHEMA_VERSION: u32 = 1;

/// One exact dependency edge in a frozen graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedDependency {
    /// Exact selected version.
    pub version: PackageVersion,
    /// Activated features.
    pub features: BTreeSet<String>,
}

/// One exact package node in a frozen graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedPackage {
    /// Logical package name.
    pub name: PackageName,
    /// Exact selected version.
    pub version: PackageVersion,
    /// Stable source-universe entry selected by the resolver.
    pub source_id: SourceId,
    /// Content-addressed source identity.
    pub source_hash: CanonicalHash,
    /// Audit receipt covering paths, origins, and producer provenance.
    pub provenance_hash: CanonicalHash,
    /// Exact selected realization.
    pub realization: RealizationKind,
    /// Package-local exact realization identity.
    pub realization_id: RealizationId,
    /// Verified registration manifest identity.
    pub manifest_hash: CanonicalHash,
    /// Verified executable or data artifact identity.
    pub artifact_hash: CanonicalHash,
    /// Required dynamic interfaces frozen for this realization.
    pub interfaces: BTreeMap<StableId, PackageVersionReq>,
    /// Exact host build required by an engine-coupled realization.
    pub engine_build_id: Option<CanonicalHash>,
    /// Activated package domains.
    pub domains: BTreeSet<PackageDomain>,
    /// Exact dependency edges.
    pub dependencies: BTreeMap<PackageName, LockedDependency>,
    /// Persistent schemas owned by this package.
    pub schemas: BTreeSet<SchemaId>,
    /// Human-readable source locator excluded from semantic graph hashing.
    pub source_path: String,
}

/// One deterministic resolver explanation event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "reason")]
pub enum ResolutionStep {
    /// A root request selected a package.
    Root {
        /// Selected package.
        package: PackageName,
    },
    /// A dependency selected a package.
    Dependency {
        /// Requiring package.
        required_by: PackageName,
        /// Selected dependency.
        package: PackageName,
    },
    /// A capability requirement selected a provider.
    Capability {
        /// Required capability.
        capability: CapabilityId,
        /// Selected provider.
        provider: PackageName,
    },
    /// Frozen mode retained an exact package.
    Frozen {
        /// Retained package.
        package: PackageName,
    },
}

/// Exact deterministic package closure persisted by the package kernel.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedGameGraph {
    /// Lock schema version.
    pub schema_version: u32,
    /// Semantic hash of the evaluated composition input.
    pub composition_hash: CanonicalHash,
    /// Provenance receipt of the evaluated composition and source universe.
    pub composition_provenance_hash: CanonicalHash,
    /// Exact Nickel evaluation policy used to produce the composition.
    pub evaluation_policy: StableId,
    /// Exact effective evaluator limits used to produce the composition.
    pub evaluation_limits: NickelEvaluationLimits,
    /// Root package names.
    pub roots: BTreeSet<PackageName>,
    /// Exact package nodes keyed by logical package name.
    pub packages: BTreeMap<PackageName, LockedPackage>,
    /// Capability providers in deterministic execution order.
    pub capability_providers: BTreeMap<CapabilityId, Vec<PackageName>>,
    /// Resolution explanation events.
    pub explanation: Vec<ResolutionStep>,
    /// Canonical semantic graph hash, excluding local source paths.
    pub graph_hash: CanonicalHash,
    /// Canonical lock receipt including exact artifacts and provenance hashes.
    pub lock_hash: CanonicalHash,
}

impl LockedGameGraph {
    /// Recomputes the semantic graph hash without acquisition provenance.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if the graph cannot be encoded.
    pub fn recompute_graph_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(&GraphSemanticIdentity::from(self))
    }

    /// Recomputes the exact frozen-lock hash without machine-local source paths.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if the lock cannot be encoded.
    pub fn recompute_lock_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(&LockIdentity::from(self))
    }

    /// Verifies both claimed graph and exact lock hashes.
    ///
    /// # Errors
    ///
    /// Returns [`GraphHashError`] if encoding fails or either claim differs
    /// from its normalized payload.
    pub fn verify_hashes(&self) -> Result<(), GraphHashError> {
        let graph = self.recompute_graph_hash()?;
        if graph != self.graph_hash {
            return Err(GraphHashError::GraphMismatch {
                expected: self.graph_hash,
                actual: graph,
            });
        }
        let lock = self.recompute_lock_hash()?;
        if lock != self.lock_hash {
            return Err(GraphHashError::LockMismatch {
                expected: self.lock_hash,
                actual: lock,
            });
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct GraphSemanticIdentity<'a> {
    schema_version: u32,
    composition_hash: CanonicalHash,
    evaluation_policy: &'a StableId,
    evaluation_limits: NickelEvaluationLimits,
    roots: &'a BTreeSet<PackageName>,
    packages: BTreeMap<&'a PackageName, GraphPackageIdentity<'a>>,
    capability_providers: &'a BTreeMap<CapabilityId, Vec<PackageName>>,
}

impl<'a> From<&'a LockedGameGraph> for GraphSemanticIdentity<'a> {
    fn from(graph: &'a LockedGameGraph) -> Self {
        Self {
            schema_version: graph.schema_version,
            composition_hash: graph.composition_hash,
            evaluation_policy: &graph.evaluation_policy,
            evaluation_limits: graph.evaluation_limits,
            roots: &graph.roots,
            packages: graph
                .packages
                .iter()
                .map(|(name, package)| (name, GraphPackageIdentity::from(package)))
                .collect(),
            capability_providers: &graph.capability_providers,
        }
    }
}

#[derive(Serialize)]
struct GraphPackageIdentity<'a> {
    name: &'a PackageName,
    version: &'a PackageVersion,
    realization: RealizationKind,
    realization_id: &'a RealizationId,
    interfaces: &'a BTreeMap<StableId, PackageVersionReq>,
    domains: &'a BTreeSet<PackageDomain>,
    dependencies: &'a BTreeMap<PackageName, LockedDependency>,
    schemas: &'a BTreeSet<SchemaId>,
}

impl<'a> From<&'a LockedPackage> for GraphPackageIdentity<'a> {
    fn from(package: &'a LockedPackage) -> Self {
        Self {
            name: &package.name,
            version: &package.version,
            realization: package.realization,
            realization_id: &package.realization_id,
            interfaces: &package.interfaces,
            domains: &package.domains,
            dependencies: &package.dependencies,
            schemas: &package.schemas,
        }
    }
}

#[derive(Serialize)]
struct LockIdentity<'a> {
    schema_version: u32,
    composition_hash: CanonicalHash,
    composition_provenance_hash: CanonicalHash,
    evaluation_policy: &'a StableId,
    evaluation_limits: NickelEvaluationLimits,
    roots: &'a BTreeSet<PackageName>,
    packages: BTreeMap<&'a PackageName, LockedPackageIdentity<'a>>,
    capability_providers: &'a BTreeMap<CapabilityId, Vec<PackageName>>,
    explanation: &'a [ResolutionStep],
    graph_hash: CanonicalHash,
}

impl<'a> From<&'a LockedGameGraph> for LockIdentity<'a> {
    fn from(graph: &'a LockedGameGraph) -> Self {
        Self {
            schema_version: graph.schema_version,
            composition_hash: graph.composition_hash,
            composition_provenance_hash: graph.composition_provenance_hash,
            evaluation_policy: &graph.evaluation_policy,
            evaluation_limits: graph.evaluation_limits,
            roots: &graph.roots,
            packages: graph
                .packages
                .iter()
                .map(|(name, package)| (name, LockedPackageIdentity::from(package)))
                .collect(),
            capability_providers: &graph.capability_providers,
            explanation: &graph.explanation,
            graph_hash: graph.graph_hash,
        }
    }
}

#[derive(Serialize)]
struct LockedPackageIdentity<'a> {
    name: &'a PackageName,
    version: &'a PackageVersion,
    source_id: &'a SourceId,
    source_hash: CanonicalHash,
    provenance_hash: CanonicalHash,
    realization: RealizationKind,
    realization_id: &'a RealizationId,
    manifest_hash: CanonicalHash,
    artifact_hash: CanonicalHash,
    interfaces: &'a BTreeMap<StableId, PackageVersionReq>,
    engine_build_id: Option<CanonicalHash>,
    domains: &'a BTreeSet<PackageDomain>,
    dependencies: &'a BTreeMap<PackageName, LockedDependency>,
    schemas: &'a BTreeSet<SchemaId>,
}

impl<'a> From<&'a LockedPackage> for LockedPackageIdentity<'a> {
    fn from(package: &'a LockedPackage) -> Self {
        Self {
            name: &package.name,
            version: &package.version,
            source_id: &package.source_id,
            source_hash: package.source_hash,
            provenance_hash: package.provenance_hash,
            realization: package.realization,
            realization_id: &package.realization_id,
            manifest_hash: package.manifest_hash,
            artifact_hash: package.artifact_hash,
            interfaces: &package.interfaces,
            engine_build_id: package.engine_build_id,
            domains: &package.domains,
            dependencies: &package.dependencies,
            schemas: &package.schemas,
        }
    }
}

/// Frozen graph or lock hash verification failure.
#[derive(Debug, Error)]
pub enum GraphHashError {
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// The semantic graph hash differs from its payload.
    #[error("graph hash mismatch: expected {expected}, recomputed {actual}")]
    GraphMismatch {
        /// Claimed graph hash.
        expected: CanonicalHash,
        /// Recomputed graph hash.
        actual: CanonicalHash,
    },
    /// The exact lock hash differs from its payload.
    #[error("lock hash mismatch: expected {expected}, recomputed {actual}")]
    LockMismatch {
        /// Claimed lock hash.
        expected: CanonicalHash,
        /// Recomputed lock hash.
        actual: CanonicalHash,
    },
}

/// Build target for one exact package node.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum BuildUnit {
    /// Static Rust package compiled into the host.
    NativeStatic {
        /// Generated static glue unit ID.
        glue: StableId,
    },
    /// Portable native artifact using the stable ABI.
    PortableNative {
        /// Required versioned ABI interfaces.
        interfaces: BTreeMap<StableId, PackageVersionReq>,
        /// Expected artifact hash.
        artifact_hash: CanonicalHash,
    },
    /// Engine-coupled artifact for one exact host build.
    EngineCoupledNative {
        /// Exact engine build identity.
        engine_build_id: CanonicalHash,
        /// Expected artifact hash.
        artifact_hash: CanonicalHash,
    },
    /// Pure data and asset processing unit.
    Data {
        /// Content-addressed artifact hash.
        artifact_hash: CanonicalHash,
    },
}

/// Exact, reproducible plan for turning a lock into artifacts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPlan {
    /// Lock graph hash used as the generated-artifact directory key.
    pub graph_hash: CanonicalHash,
    /// Target triple.
    pub target: TargetTriple,
    /// Toolchain identity.
    pub toolchain: String,
    /// Build units keyed by package name.
    pub units: BTreeMap<PackageName, BuildUnit>,
    /// Generated shared-schema crates keyed by schema ID.
    pub shared_schemas: BTreeMap<SchemaId, CanonicalHash>,
}

/// Stable callback binding for one runtime package instance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeBinding {
    /// Selected realization.
    pub realization: RealizationKind,
    /// Verified artifact identity.
    pub artifact_hash: CanonicalHash,
    /// Callback keys present in the realization.
    pub callbacks: BTreeSet<StableId>,
}

/// Activation-ready runtime image without process-local pointers or handles.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeImage {
    /// Matching registration image hash.
    pub registration_hash: CanonicalHash,
    /// Runtime bindings keyed by package name.
    pub packages: BTreeMap<PackageName, RuntimeBinding>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_hash_is_independent_of_dependency_insertion_order() {
        let first = graph_with_dependencies(["@terrenia/a", "@terrenia/b"]);
        let second = graph_with_dependencies(["@terrenia/b", "@terrenia/a"]);

        assert_eq!(
            first.recompute_graph_hash().ok(),
            second.recompute_graph_hash().ok()
        );
        assert_eq!(
            first.recompute_lock_hash().ok(),
            second.recompute_lock_hash().ok()
        );
    }

    #[test]
    fn graph_and_lock_hashes_cover_different_receipts() {
        let mut graph = graph_with_dependencies(["@terrenia/a", "@terrenia/b"]);
        graph.graph_hash = graph
            .recompute_graph_hash()
            .unwrap_or_else(|error| panic!("fixture graph hash failed: {error}"));
        graph.lock_hash = graph
            .recompute_lock_hash()
            .unwrap_or_else(|error| panic!("fixture lock hash failed: {error}"));
        assert!(graph.verify_hashes().is_ok());

        graph
            .packages
            .values_mut()
            .for_each(|package| package.artifact_hash = CanonicalHash::digest(b"changed"));
        assert!(matches!(
            graph.verify_hashes(),
            Err(GraphHashError::LockMismatch { .. })
        ));
    }

    fn graph_with_dependencies<const N: usize>(dependencies: [&str; N]) -> LockedGameGraph {
        let root = package_name("terrenia");
        let dependencies = dependencies
            .into_iter()
            .map(|name| {
                (
                    package_name(name),
                    LockedDependency {
                        version: version("0.1.0"),
                        features: BTreeSet::new(),
                    },
                )
            })
            .collect();
        let package = LockedPackage {
            name: root.clone(),
            version: version("0.1.0"),
            source_id: "latticeaxiom:source/terrenia"
                .parse()
                .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
            source_hash: CanonicalHash::digest(b"source"),
            provenance_hash: CanonicalHash::digest(b"provenance"),
            realization: RealizationKind::Data,
            realization_id: RealizationId::new("data")
                .unwrap_or_else(|error| panic!("fixture realization is invalid: {error}")),
            manifest_hash: CanonicalHash::digest(b"manifest"),
            artifact_hash: CanonicalHash::digest(b"artifact"),
            interfaces: BTreeMap::new(),
            engine_build_id: None,
            domains: BTreeSet::from([PackageDomain::Authoritative]),
            dependencies,
            schemas: BTreeSet::new(),
            source_path: "packages/terrenia".to_owned(),
        };
        LockedGameGraph {
            schema_version: LOCK_SCHEMA_VERSION,
            composition_hash: CanonicalHash::digest(b"composition"),
            composition_provenance_hash: CanonicalHash::digest(b"composition-provenance"),
            evaluation_policy: "latticeaxiom:nickel-evaluation-policy/r0@1"
                .parse()
                .unwrap_or_else(|error| panic!("fixture policy ID is invalid: {error}")),
            evaluation_limits: NickelEvaluationLimits::default(),
            roots: BTreeSet::from([root.clone()]),
            packages: BTreeMap::from([(root, package)]),
            capability_providers: BTreeMap::new(),
            explanation: Vec::new(),
            graph_hash: CanonicalHash::digest(b"unverified-graph"),
            lock_hash: CanonicalHash::digest(b"unverified-lock"),
        }
    }

    fn package_name(value: &str) -> PackageName {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture package `{value}` is invalid: {error}"))
    }

    fn version(value: &str) -> PackageVersion {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture version `{value}` is invalid: {error}"))
    }
}
