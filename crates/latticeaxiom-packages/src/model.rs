use std::collections::BTreeMap;
use std::path::PathBuf;

use latticeaxiom_compose::{
    BlockDeclaration, CapabilityId, PackageDeclaration, PackageId, PackageManifest, PackageVersion,
    ProfileDeclaration, Realization,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current schema version of [`LockedGameGraph`].
pub const LOCK_SCHEMA_VERSION: u32 = 1;

/// Current schema version of [`BuildPlan`].
pub const BUILD_PLAN_SCHEMA_VERSION: u32 = 1;

/// Current schema version of [`PublishedPackageDescriptor`].
pub const PUBLISHED_DESCRIPTOR_SCHEMA_VERSION: u32 = 1;

/// A source whose identity is exact and whose content has already been hashed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LockedSource {
    /// A package directory relative to the project root.
    Path {
        /// Portable slash-normalized path.
        path: String,
    },
}

/// A resolved block registration carried into runtime image construction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedBlock {
    /// Globally stable registration key, `<package>:<local-id>`.
    pub key: String,
    /// User-facing fallback name.
    pub display_name: String,
    /// Whether the block is solid for collision and meshing.
    pub solid: bool,
    /// Whether this is the unique empty-space block mapped to ID zero.
    pub is_air: bool,
    /// Linear RGBA color used by the first demo.
    pub color: [u8; 4],
}

/// One exact package node in a locked game graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedPackage {
    /// Stable package identity.
    pub id: PackageId,
    /// Exact package version.
    pub version: PackageVersion,
    /// Exact source identity.
    pub source: LockedSource,
    /// SHA-256 of the canonical source-directory byte stream.
    pub content_sha256: String,
    /// Direct exact dependencies, sorted by package identity.
    pub dependencies: Vec<PackageId>,
    /// Capabilities exported by this package, sorted by stable key.
    pub capabilities: Vec<CapabilityId>,
    /// Data block registrations, sorted by global stable key.
    pub blocks: Vec<LockedBlock>,
    /// Selected realization.
    pub realization: Realization,
}

/// Resolution of one required capability to one exact provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityBinding {
    /// Required capability contract.
    pub capability: CapabilityId,
    /// Selected provider package.
    pub provider: PackageId,
}

/// Exact package, capability, target, and parameter closure used by all builds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedGameGraph {
    /// Persistent lock schema version.
    pub schema_version: u32,
    /// Semantic schema version of the source [`latticeaxiom_compose::CompositionSpec`].
    pub composition_schema_version: u32,
    /// Semantic version of the `latticeaxiom.lib` contract family.
    pub contract_schema_version: u32,
    /// Exact embedded Nickel evaluator identity.
    pub nickel_evaluator: String,
    /// SHA-256 identity of every file in the shared contract library.
    pub contract_sha256: String,
    /// SHA-256 identity of every other semantic field in this graph.
    pub graph_sha256: String,
    /// Root profile identity and version.
    pub root_profile: ProfileDeclaration,
    /// Exact build target.
    pub target: String,
    /// Exact toolchain identity.
    pub toolchain: String,
    /// Fully evaluated finite parameters, ordered by key.
    pub parameters: BTreeMap<String, String>,
    /// Exact package nodes, sorted by package identity.
    pub packages: Vec<LockedPackage>,
    /// Capability selections, sorted by capability then provider.
    pub capability_bindings: Vec<CapabilityBinding>,
}

impl LockedGameGraph {
    /// Finds an exact package by its stable identifier.
    #[must_use]
    pub fn package(&self, id: &PackageId) -> Option<&LockedPackage> {
        self.packages
            .binary_search_by(|package| package.id.cmp(id))
            .ok()
            .and_then(|index| self.packages.get(index))
    }
}

/// Kind of deterministic work represented by one build-plan node.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildJobKind {
    /// Verify that an exact source is available and matches its hash.
    PrepareSource,
    /// Compile pure package data into runtime registrations.
    RealizeData,
    /// Compile a trusted native-static package.
    CompileNativeStatic,
    /// Validate a precompiled native-dynamic descriptor.
    ValidateNativeDynamic,
    /// Generate the deterministic static registration glue crate.
    GenerateStaticGlue,
    /// Assemble the final game closure and its metadata.
    PackageClosure,
}

/// One node in the deterministic milestone 2 build DAG.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildJob {
    /// Stable job key.
    pub id: String,
    /// Work category.
    pub kind: BuildJobKind,
    /// Logical package when this is a per-package job.
    pub package: Option<PackageId>,
    /// Stable job keys that must finish first, sorted lexicographically.
    pub dependencies: Vec<String>,
    /// Content or graph hash defining the job input.
    pub input_sha256: String,
}

/// Versioned deterministic work DAG derived from one locked graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPlan {
    /// Build-plan schema version.
    pub schema_version: u32,
    /// Input graph identity.
    pub graph_sha256: String,
    /// SHA-256 identity of every other semantic field in this plan.
    pub plan_sha256: String,
    /// Build jobs sorted by stable job key.
    pub jobs: Vec<BuildJob>,
}

/// Versioned descriptor produced by `pack` from a typed package manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedPackageDescriptor {
    /// Descriptor schema version.
    pub schema_version: u32,
    /// Exact package source manifest.
    pub manifest: PackageManifest,
    /// SHA-256 of the package source directory.
    pub content_sha256: String,
    /// Optional target for precompiled artifacts; absent for pure data.
    pub target: Option<String>,
    /// Optional toolchain identity for precompiled artifacts.
    pub toolchain: Option<String>,
}

impl PublishedPackageDescriptor {
    /// Returns the descriptor's package declaration.
    #[must_use]
    pub fn package(&self) -> &PackageDeclaration {
        &self.manifest.package
    }
}

/// Package-kernel, lock, hashing, or build-plan failure.
#[derive(Debug, Error)]
pub enum PackageError {
    /// An exact local source path could not be canonicalized or read.
    #[error("failed to access `{path}`: {source}")]
    Io {
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying file-system error.
        source: std::io::Error,
    },
    /// Nickel evaluation or direct typed conversion failed.
    #[error(transparent)]
    Compose(#[from] latticeaxiom_compose::ComposeError),
    /// A requested source escaped the controlled project root.
    #[error("local source `{path}` resolves outside project root `{root}`")]
    SourceOutsideRoot {
        /// Rejected source path.
        path: PathBuf,
        /// Controlled project root.
        root: PathBuf,
    },
    /// A requested source is not a directory.
    #[error("local source `{path}` is not a package directory")]
    SourceNotDirectory {
        /// Rejected source path.
        path: PathBuf,
    },
    /// A symlink was found inside a hashed package source.
    #[error(
        "package source contains symlink `{path}`; v1 source hashes require regular files and directories"
    )]
    SourceSymlink {
        /// Rejected symlink path.
        path: PathBuf,
    },
    /// A manifest identity did not match the root's exact request.
    #[error(
        "source for `{requested}` declares package `{declared}`; fix the profile path or package id"
    )]
    PackageIdentityMismatch {
        /// Requested package id.
        requested: PackageId,
        /// Declared package id.
        declared: PackageId,
    },
    /// A manifest version did not match the root's exact request.
    #[error(
        "package `{id}` requested exact version `{requested}`, but source declares `{declared}`"
    )]
    PackageVersionMismatch {
        /// Package identifier.
        id: PackageId,
        /// Exact requested version.
        requested: PackageVersion,
        /// Exact declared version.
        declared: PackageVersion,
    },
    /// The requested realization is not published by the package.
    #[error("package `{id}` does not publish `{realization}`; choose one of: {available:?}")]
    UnsupportedRealization {
        /// Package identifier.
        id: PackageId,
        /// Requested realization.
        realization: Realization,
        /// Published realizations.
        available: Vec<Realization>,
    },
    /// A package dependency was not listed with an exact source by the root.
    #[error(
        "package `{package}` depends on missing `{dependency}`; add an exact package request and local source to the root profile"
    )]
    MissingDependency {
        /// Package with the dependency.
        package: PackageId,
        /// Missing dependency.
        dependency: PackageId,
    },
    /// An exact dependency version disagreed with the selected node.
    #[error(
        "package `{package}` requires `{dependency}` version `{required}`, but the root selects `{selected}`"
    )]
    DependencyVersionMismatch {
        /// Package with the dependency.
        package: PackageId,
        /// Dependency package.
        dependency: PackageId,
        /// Required exact version.
        required: PackageVersion,
        /// Selected exact version.
        selected: PackageVersion,
    },
    /// The package dependency graph contains a cycle.
    #[error("package dependency cycle: {cycle:?}; remove one dependency edge")]
    DependencyCycle {
        /// Stable package path closing the cycle.
        cycle: Vec<PackageId>,
    },
    /// No package provides a required capability.
    #[error("required capability `{capability}` has no provider; add a package that provides it")]
    MissingCapability {
        /// Missing capability.
        capability: CapabilityId,
    },
    /// More than one package provides a requirement with no explicit choice.
    #[error(
        "capability `{capability}` has multiple providers {candidates:?}; set `provider` in the root profile"
    )]
    AmbiguousCapability {
        /// Ambiguous capability.
        capability: CapabilityId,
        /// Candidate providers in stable order.
        candidates: Vec<PackageId>,
    },
    /// An explicitly chosen provider is absent or does not provide the capability.
    #[error(
        "package `{provider}` does not provide required capability `{capability}`; choose a listed provider"
    )]
    InvalidCapabilityProvider {
        /// Capability requirement.
        capability: CapabilityId,
        /// Invalid explicit provider.
        provider: PackageId,
    },
    /// JSON persistence failed.
    #[error("failed to encode or decode a versioned package model: {0}")]
    Json(#[from] serde_json::Error),
    /// A lock schema is newer or otherwise unsupported.
    #[error("lock schema {found} is unsupported; this binary supports {supported}")]
    UnsupportedLockSchema {
        /// Version found in the lock.
        found: u32,
        /// Version supported by this binary.
        supported: u32,
    },
    /// A lock graph hash does not match its semantic payload.
    #[error(
        "lock graph hash mismatch: recorded `{recorded}`, calculated `{calculated}`; regenerate latticeaxiom.lock"
    )]
    LockHashMismatch {
        /// Hash stored in the lock.
        recorded: String,
        /// Hash calculated from the payload.
        calculated: String,
    },
}

pub(crate) fn locked_block(package: &PackageId, block: &BlockDeclaration) -> LockedBlock {
    LockedBlock {
        key: format!("{}:{}", package.as_str(), block.id),
        display_name: block.display_name.clone(),
        solid: block.solid,
        is_air: block.is_air,
        color: block.color,
    }
}
