//! Non-executable composition bootstrap and package source manifests.
//!
//! Restricted TOML is the human-authored representation of
//! [`CompositionBootstrapV1`] (`latticeaxiom.toml`) and
//! [`PackageSourceManifestV1`] (`latticeaxiom-package.toml`). Semantic identity
//! is the validated serde DTO hashed with project canonical JSON. Raw TOML
//! layout is not identity.
//!
//! These manifests authorize root requirements, local source providers, and
//! graph-affecting package metadata before Nickel composition. They reject
//! scripts, functions, import, environment expansion, conditional I/O, network
//! locators, and ambient source discovery. They do not acquire CAS objects,
//! persist a lock, or launch a runtime.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CanonicalLogicalPath, CapabilityId, PackageName,
    PackageVersion, PackageVersionReq, SourceId, StableId, TargetTriple, canonical_json_hash,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    ArtifactIntent, AuthorizedRoot, CapabilityProvision, CapabilityRequirement,
    CompositionParameterSpec, FeatureSpec, InterfaceRequirement, NickelEvaluationLimits,
    PackageAlias, PackageDomain, PackageRequest, ProfileKind, R0_LIBRARY_PACKAGE_ALIAS,
    R0_NICKEL_EVALUATION_POLICY, RealizationId, RealizationKind, SourceScanError, SourceScanLimits,
    SourceSnapshot, SourceSnapshotError, TrustClass,
};

/// Schema version for [`CompositionBootstrapV1`].
pub const COMPOSITION_BOOTSTRAP_SCHEMA_VERSION: u32 = 1;

/// Schema version for [`PackageSourceManifestV1`].
pub const PACKAGE_SOURCE_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Default human-authored file name for [`CompositionBootstrapV1`].
pub const COMPOSITION_BOOTSTRAP_FILE_NAME: &str = "latticeaxiom.toml";

/// Default human-authored file name for [`PackageSourceManifestV1`].
pub const PACKAGE_SOURCE_MANIFEST_FILE_NAME: &str = "latticeaxiom-package.toml";

const DEFAULT_NICKEL_ENTRYPOINT: &str = "default";

/// Versioned static bootstrap input that precedes full Nickel composition.
///
/// Source providers are an explicit closed set. The kernel must not scan the
/// ambient workspace, home directory, or network for undeclared packages.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionBootstrapV1 {
    /// Bootstrap contract schema version.
    pub schema_version: u32,
    /// Domain projection selected for this composition.
    pub projection: ProfileKind,
    /// Exact domains selected by the projection.
    pub projection_domains: BTreeSet<PackageDomain>,
    /// Root package requirements keyed by logical package name.
    pub roots: BTreeMap<PackageName, PackageRequest>,
    /// Explicit local source providers. Discovery order is not identity.
    pub sources: Vec<BootstrapSourceProviderV1>,
    /// Package-qualified feature selections.
    #[serde(default)]
    pub features: BTreeMap<PackageName, BTreeSet<String>>,
    /// Graph-affecting finite parameter values.
    #[serde(default)]
    pub parameters: BTreeMap<StableId, Value>,
    /// Ordered automatic realization preference.
    pub realization_policy: Vec<RealizationKind>,
    /// Versioned Nickel evaluation policy.
    pub evaluation_policy: StableId,
    /// Effective evaluator limits authorized by this bootstrap.
    #[serde(default)]
    pub evaluation_limits: NickelEvaluationLimits,
    /// Root-relative Nickel profile entry evaluated after resolution.
    pub nickel_profile_entry: CanonicalLogicalPath,
}

/// One bounded local source provider declared by a bootstrap manifest.
///
/// Path values are acquisition locators, not runnable identity. Remote,
/// registry, git, and URL providers are not part of this schema.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum BootstrapSourceProviderV1 {
    /// A workspace package named by this bootstrap.
    Workspace {
        /// Logical package provided by the workspace.
        package: PackageName,
    },
    /// A package located at a root-manifest-relative path.
    Path {
        /// Logical package contained at `path`.
        package: PackageName,
        /// Canonical root-relative acquisition path.
        path: CanonicalLogicalPath,
    },
    /// A package selected from a local catalog by exact version.
    LocalCatalog {
        /// Logical package stored in the catalog.
        package: PackageName,
        /// Exact cataloged version.
        version: PackageVersion,
        /// Optional root-relative catalog locator.
        #[serde(default)]
        catalog: Option<CanonicalLogicalPath>,
    },
    /// An explicit in-memory or filesystem fixture.
    Fixture {
        /// Logical package provided by the fixture.
        package: PackageName,
        /// Exact fixture version.
        version: PackageVersion,
        /// Stable source-universe identity for the fixture.
        source_id: SourceId,
        /// Optional root-relative filesystem fixture path.
        #[serde(default)]
        path: Option<CanonicalLogicalPath>,
    },
}

impl CompositionBootstrapV1 {
    /// Parses restricted TOML into a validated bootstrap DTO.
    ///
    /// # Errors
    ///
    /// Returns [`BootstrapManifestError`] when the document is not TOML,
    /// contains unknown fields, uses a forbidden construct, or violates schema
    /// invariants.
    pub fn from_toml_str(text: &str) -> Result<Self, BootstrapManifestError> {
        let mut bootstrap = parse_restricted_toml::<Self>(text)?;
        bootstrap.sources.sort_by(compare_source_providers);
        bootstrap.validate()?;
        Ok(bootstrap)
    }

    /// Validates schema, sources, roots, projection, and evaluation policy.
    ///
    /// # Errors
    ///
    /// Returns a deterministic [`BootstrapManifestError`] for the first invalid
    /// row in stable map order.
    pub fn validate(&self) -> Result<(), BootstrapManifestError> {
        if self.schema_version != COMPOSITION_BOOTSTRAP_SCHEMA_VERSION {
            return Err(BootstrapManifestError::UnsupportedBootstrapSchema {
                found: self.schema_version,
                supported: COMPOSITION_BOOTSTRAP_SCHEMA_VERSION,
            });
        }
        if self.roots.is_empty() {
            return Err(BootstrapManifestError::EmptyRoots);
        }
        if self.sources.is_empty() {
            return Err(BootstrapManifestError::EmptySources);
        }
        if self.realization_policy.is_empty() {
            return Err(BootstrapManifestError::EmptyRealizationOrder);
        }
        validate_projection_domains(self.projection, &self.projection_domains)?;
        validate_evaluation_policy(
            self.projection,
            &self.evaluation_policy,
            &self.evaluation_limits,
        )?;

        let mut realization_kinds = BTreeSet::new();
        for kind in &self.realization_policy {
            if !realization_kinds.insert(*kind) {
                return Err(BootstrapManifestError::DuplicateRealizationPreference { kind: *kind });
            }
        }

        let mut source_packages = BTreeSet::new();
        for source in &self.sources {
            if let Some(path) = source.ambient_locator() {
                return Err(BootstrapManifestError::AmbientSourcePath {
                    path: path.to_string(),
                });
            }
            if !source_packages.insert(source.package().clone()) {
                return Err(BootstrapManifestError::DuplicateSourcePackage {
                    package: source.package().clone(),
                });
            }
        }
        if let Some(package) = self
            .roots
            .keys()
            .find(|package| !source_packages.contains(*package))
        {
            return Err(BootstrapManifestError::RootWithoutSource {
                package: package.clone(),
            });
        }
        Ok(())
    }

    /// Hashes the validated DTO with project canonical JSON.
    ///
    /// Source-provider order is sorted before hashing so TOML array layout is
    /// not identity. Realization-policy order is semantically meaningful and is
    /// preserved.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when the DTO cannot be encoded.
    pub fn canonical_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        let mut sources = self.sources.clone();
        sources.sort_by(compare_source_providers);
        canonical_json_hash(&BootstrapIdentity {
            schema_version: self.schema_version,
            projection: self.projection,
            projection_domains: &self.projection_domains,
            roots: &self.roots,
            sources,
            features: &self.features,
            parameters: &self.parameters,
            realization_policy: &self.realization_policy,
            evaluation_policy: &self.evaluation_policy,
            evaluation_limits: self.evaluation_limits,
            nickel_profile_entry: &self.nickel_profile_entry,
        })
    }
}

impl BootstrapSourceProviderV1 {
    /// Returns the logical package located by this provider.
    #[must_use]
    pub const fn package(&self) -> &PackageName {
        match self {
            Self::Workspace { package }
            | Self::Path { package, .. }
            | Self::LocalCatalog { package, .. }
            | Self::Fixture { package, .. } => package,
        }
    }

    const fn kind_rank(&self) -> u8 {
        match self {
            Self::Workspace { .. } => 0,
            Self::Path { .. } => 1,
            Self::LocalCatalog { .. } => 2,
            Self::Fixture { .. } => 3,
        }
    }

    fn ambient_locator(&self) -> Option<&CanonicalLogicalPath> {
        let path = match self {
            Self::Path { path, .. } => Some(path),
            Self::LocalCatalog { catalog, .. } => catalog.as_ref(),
            Self::Fixture { path, .. } => path.as_ref(),
            Self::Workspace { .. } => None,
        }?;
        path_is_ambient(path).then_some(path)
    }
}

#[derive(Serialize)]
struct BootstrapIdentity<'a> {
    schema_version: u32,
    projection: ProfileKind,
    projection_domains: &'a BTreeSet<PackageDomain>,
    roots: &'a BTreeMap<PackageName, PackageRequest>,
    sources: Vec<BootstrapSourceProviderV1>,
    features: &'a BTreeMap<PackageName, BTreeSet<String>>,
    parameters: &'a BTreeMap<StableId, Value>,
    realization_policy: &'a [RealizationKind],
    evaluation_policy: &'a StableId,
    evaluation_limits: NickelEvaluationLimits,
    nickel_profile_entry: &'a CanonicalLogicalPath,
}

/// Graph-affecting static projection of one package, authored before Nickel.
///
/// Direct dependency aliases are the only package names visible to
/// package-aware Nickel import. Overlapping fields must later equal the
/// evaluated [`crate::PackageSpec`] after typed normalization.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSourceManifestV1 {
    /// Package source-manifest schema version.
    pub schema_version: u32,
    /// Logical package identity.
    pub name: PackageName,
    /// Exact package version.
    pub version: PackageVersion,
    /// Direct dependency aliases used by package-aware Nickel import.
    #[serde(default)]
    pub dependencies: BTreeMap<PackageAlias, ManifestDependencyAliasV1>,
    /// Feature declarations keyed by package-local stable names.
    #[serde(default)]
    pub features: BTreeMap<String, FeatureSpec>,
    /// Package domains used to project client, server, and authoritative closures.
    pub domains: BTreeSet<PackageDomain>,
    /// Versioned capabilities required by the package.
    #[serde(default)]
    pub requires: BTreeMap<CapabilityId, CapabilityRequirement>,
    /// Versioned capabilities provided by the package.
    #[serde(default)]
    pub provides: BTreeMap<CapabilityId, CapabilityProvision>,
    /// Graph-affecting realization candidates keyed by a stable local label.
    pub realizations: BTreeMap<RealizationId, ManifestRealizationV1>,
    /// Graph-affecting parameter schema as finite normalized JSON data.
    #[serde(default)]
    pub parameters: BTreeMap<StableId, CompositionParameterSpec>,
    /// Highest trust class required by any package operation.
    pub trust: TrustClass,
    /// Public Nickel files keyed by entry name. Alias import uses `default`.
    pub nickel_public_entrypoints: BTreeMap<String, CanonicalLogicalPath>,
    /// Explicit source files and directories included in a later snapshot.
    pub source_inclusion: SourceInclusionPolicyV1,
}

/// One direct dependency declared under a package-local import alias.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestDependencyAliasV1 {
    /// Logical dependency package identity.
    pub package: PackageName,
    /// Compatible versions.
    pub version: PackageVersionReq,
    /// Whether the dependency is activated only by a feature.
    pub optional: bool,
    /// Package-local features, any one of which activates this optional edge.
    #[serde(default)]
    pub when_features: BTreeSet<String>,
    /// Feature names forwarded to the dependency.
    #[serde(default)]
    pub features: BTreeSet<String>,
    /// Domains in which this edge is active.
    pub domains: BTreeSet<PackageDomain>,
}

/// Graph-affecting realization candidate published by a package source manifest.
///
/// Registration-fragment receipts are produced later and are not authored here.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRealizationV1 {
    /// Package-local stable realization ID.
    pub id: RealizationId,
    /// Realization family.
    pub kind: RealizationKind,
    /// Package domains implemented by this realization.
    pub domains: BTreeSet<PackageDomain>,
    /// Target triples supported by the realization, empty for target-neutral data.
    #[serde(default)]
    pub targets: BTreeSet<TargetTriple>,
    /// Required and optional versioned dynamic interfaces.
    #[serde(default)]
    pub interfaces: BTreeMap<StableId, InterfaceRequirement>,
    /// Features that must be active before this candidate is eligible.
    #[serde(default)]
    pub required_features: BTreeSet<String>,
    /// Source build, local prebuilt, or data-root intent.
    pub artifact: ArtifactIntent,
    /// Trust required to prepare and activate the realization.
    pub trust: TrustClass,
    /// Exact engine build required only by engine-coupled realizations.
    #[serde(default)]
    pub engine_build: Option<CanonicalHash>,
}

/// Explicit source-tree inclusion used instead of ambient directory discovery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInclusionPolicyV1 {
    /// Root-relative files and directories included in the package snapshot.
    pub include: BTreeSet<CanonicalLogicalPath>,
    /// Root-relative exclusions subtracted from included directories.
    #[serde(default)]
    pub exclude: BTreeSet<CanonicalLogicalPath>,
}

/// Applies an explicit package source-inclusion policy to a verified snapshot.
///
/// Inclusion and exclusion entries cover either one exact logical path or all
/// descendants below that path. Exclusion wins. The returned snapshot carries
/// a newly computed source identity over only the retained file receipts.
///
/// # Errors
///
/// Returns [`SourceSnapshotError`] if the input snapshot is invalid or the
/// selected canonical file table cannot be represented on this host.
pub fn apply_source_inclusion(
    snapshot: &SourceSnapshot,
    inclusion: &SourceInclusionPolicyV1,
) -> Result<SourceSnapshot, SourceSnapshotError> {
    snapshot.select_files(|logical_path| source_inclusion_selects(inclusion, logical_path))
}

/// Acquires only files reachable through one package source-inclusion policy.
///
/// Directories that cannot contain an included file, including explicitly
/// excluded subtrees, are pruned before metadata inspection and before source
/// file/byte budgets are charged. Retained paths use the same canonical hash,
/// link rejection, collision checks, and limits as a full source scan.
///
/// # Errors
///
/// Returns [`SourceScanError`] for filesystem failures, invalid retained
/// paths, links or reparse points on retained paths, collisions, or retained
/// source-budget violations.
pub fn scan_included_source_snapshot(
    root: &AuthorizedRoot,
    limits: SourceScanLimits,
    inclusion: &SourceInclusionPolicyV1,
) -> Result<SourceSnapshot, SourceScanError> {
    crate::imports::scan_source_snapshot_selected(
        root,
        limits,
        |logical_path| source_inclusion_selects(inclusion, logical_path),
        |logical_path| source_inclusion_may_descend(inclusion, logical_path),
    )
}

fn source_inclusion_selects(inclusion: &SourceInclusionPolicyV1, logical_path: &str) -> bool {
    logical_path_covered(&inclusion.include, logical_path)
        && !logical_path_covered(&inclusion.exclude, logical_path)
}

fn source_inclusion_may_descend(
    inclusion: &SourceInclusionPolicyV1,
    logical_directory: &str,
) -> bool {
    !logical_path_covered(&inclusion.exclude, logical_directory)
        && inclusion.include.iter().any(|item| {
            logical_prefix_covers(item.as_str(), logical_directory)
                || logical_prefix_covers(logical_directory, item.as_str())
        })
}

fn logical_path_covered(prefixes: &BTreeSet<CanonicalLogicalPath>, logical_path: &str) -> bool {
    prefixes
        .iter()
        .any(|item| logical_prefix_covers(item.as_str(), logical_path))
}

fn logical_prefix_covers(prefix: &str, logical_path: &str) -> bool {
    logical_path == prefix
        || logical_path.starts_with(prefix)
            && logical_path
                .as_bytes()
                .get(prefix.len())
                .is_some_and(|byte| *byte == b'/')
}

impl PackageSourceManifestV1 {
    /// Parses restricted TOML into a validated package source manifest.
    ///
    /// # Errors
    ///
    /// Returns [`BootstrapManifestError`] when the document is not TOML,
    /// contains unknown fields, uses a forbidden construct, or violates schema
    /// invariants.
    pub fn from_toml_str(text: &str) -> Result<Self, BootstrapManifestError> {
        let manifest = parse_restricted_toml::<Self>(text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates identity, aliases, graph-affecting rows, and source inclusion.
    ///
    /// # Errors
    ///
    /// Returns a deterministic [`BootstrapManifestError`] for the first invalid
    /// row in stable map order.
    pub fn validate(&self) -> Result<(), BootstrapManifestError> {
        if self.schema_version != PACKAGE_SOURCE_MANIFEST_SCHEMA_VERSION {
            return Err(BootstrapManifestError::UnsupportedPackageManifestSchema {
                found: self.schema_version,
                supported: PACKAGE_SOURCE_MANIFEST_SCHEMA_VERSION,
            });
        }
        if self.domains.is_empty() {
            return Err(BootstrapManifestError::EmptyPackageDomains {
                package: self.name.clone(),
            });
        }
        if self.realizations.is_empty() {
            return Err(BootstrapManifestError::EmptyRealizations {
                package: self.name.clone(),
            });
        }
        if self.source_inclusion.include.is_empty() {
            return Err(BootstrapManifestError::EmptySourceInclusion {
                package: self.name.clone(),
            });
        }
        self.validate_dependencies_and_features()?;
        self.validate_capabilities_and_parameters()?;
        self.validate_realizations()?;
        self.validate_entrypoints()
    }

    /// Hashes the validated DTO with project canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when the DTO cannot be encoded.
    pub fn canonical_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(self)
    }

    fn validate_dependencies_and_features(&self) -> Result<(), BootstrapManifestError> {
        let mut dependency_packages = BTreeSet::new();
        for (alias, dependency) in &self.dependencies {
            if alias.as_str() == R0_LIBRARY_PACKAGE_ALIAS {
                return Err(BootstrapManifestError::ReservedLibraryAlias {
                    alias: alias.to_string(),
                });
            }
            if !dependency_packages.insert(&dependency.package) {
                return Err(BootstrapManifestError::DuplicateDependencyPackage {
                    package: dependency.package.clone(),
                });
            }
            ensure_domain_subset(
                &self.name,
                alias.as_str(),
                &dependency.domains,
                &self.domains,
            )?;
            if dependency.optional == dependency.when_features.is_empty() {
                return Err(BootstrapManifestError::InvalidDependencyActivation {
                    package: self.name.clone(),
                    dependency: dependency.package.clone(),
                });
            }
            for feature in &dependency.when_features {
                if !self.features.contains_key(feature) {
                    return Err(BootstrapManifestError::UnknownDependencyFeature {
                        package: self.name.clone(),
                        dependency: dependency.package.clone(),
                        feature: feature.clone(),
                    });
                }
            }
        }
        for (name, feature) in &self.features {
            ensure_domain_subset(
                &self.name,
                &format!("feature:{name}"),
                &feature.domains,
                &self.domains,
            )?;
        }
        Ok(())
    }

    fn validate_capabilities_and_parameters(&self) -> Result<(), BootstrapManifestError> {
        for (key, requirement) in &self.requires {
            if key != &requirement.capability {
                return Err(BootstrapManifestError::CapabilityKeyMismatch {
                    key: key.to_string(),
                    value: requirement.capability.to_string(),
                });
            }
            ensure_domain_subset(
                &self.name,
                key.as_str(),
                &requirement.domains,
                &self.domains,
            )?;
        }
        for (key, provision) in &self.provides {
            if key != &provision.capability {
                return Err(BootstrapManifestError::CapabilityKeyMismatch {
                    key: key.to_string(),
                    value: provision.capability.to_string(),
                });
            }
            ensure_domain_subset(&self.name, key.as_str(), &provision.domains, &self.domains)?;
        }
        for (key, parameter) in &self.parameters {
            if key != &parameter.id || parameter.declared_by != self.name {
                return Err(BootstrapManifestError::ParameterDeclarationMismatch {
                    package: self.name.clone(),
                    key: key.to_string(),
                    value: parameter.id.to_string(),
                });
            }
        }
        Ok(())
    }

    fn validate_realizations(&self) -> Result<(), BootstrapManifestError> {
        for (key, realization) in &self.realizations {
            if key != &realization.id {
                return Err(BootstrapManifestError::RealizationKeyMismatch {
                    key: key.clone(),
                    value: realization.id.clone(),
                });
            }
            if realization.domains.is_empty() || !realization.domains.is_subset(&self.domains) {
                return Err(BootstrapManifestError::InvalidRealizationDomains {
                    package: self.name.clone(),
                    realization: key.clone(),
                });
            }
            if realization.trust > self.trust {
                return Err(BootstrapManifestError::RealizationTrustExceedsPackage {
                    package: self.name.clone(),
                    realization: key.clone(),
                });
            }
            for feature in &realization.required_features {
                if !self.features.contains_key(feature) {
                    return Err(BootstrapManifestError::UnknownRealizationFeature {
                        package: self.name.clone(),
                        realization: key.clone(),
                        feature: feature.clone(),
                    });
                }
            }
            let valid_boundary = match realization.kind {
                RealizationKind::Data | RealizationKind::NativeStatic => {
                    realization.interfaces.is_empty() && realization.engine_build.is_none()
                }
                RealizationKind::PortableNative => {
                    !realization.interfaces.is_empty() && realization.engine_build.is_none()
                }
                RealizationKind::EngineCoupledNative => realization.engine_build.is_some(),
            };
            if !valid_boundary {
                return Err(BootstrapManifestError::InvalidRealizationBoundary {
                    package: self.name.clone(),
                    realization: key.clone(),
                    kind: realization.kind,
                });
            }
        }
        Ok(())
    }

    fn validate_entrypoints(&self) -> Result<(), BootstrapManifestError> {
        if !self
            .nickel_public_entrypoints
            .contains_key(DEFAULT_NICKEL_ENTRYPOINT)
        {
            return Err(BootstrapManifestError::MissingDefaultNickelEntrypoint {
                package: self.name.clone(),
            });
        }
        let mut seen_paths = BTreeSet::new();
        for (name, path) in &self.nickel_public_entrypoints {
            if name != DEFAULT_NICKEL_ENTRYPOINT {
                PackageAlias::new(name.clone()).map_err(|error| {
                    BootstrapManifestError::InvalidNickelEntrypointName {
                        package: self.name.clone(),
                        name: name.clone(),
                        reason: error.to_string(),
                    }
                })?;
            }
            if path_is_ambient(path) {
                return Err(BootstrapManifestError::AmbientSourcePath {
                    path: path.to_string(),
                });
            }
            if !seen_paths.insert(path) {
                return Err(BootstrapManifestError::DuplicateNickelEntrypointPath {
                    path: path.clone(),
                });
            }
            if !inclusion_covers(&self.source_inclusion.include, path) {
                return Err(BootstrapManifestError::EntrypointOutsideInclusion {
                    package: self.name.clone(),
                    path: path.clone(),
                });
            }
        }
        for path in self
            .source_inclusion
            .include
            .iter()
            .chain(self.source_inclusion.exclude.iter())
        {
            if path_is_ambient(path) {
                return Err(BootstrapManifestError::AmbientSourcePath {
                    path: path.to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Failures while parsing or validating a static bootstrap or package manifest.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum BootstrapManifestError {
    /// The document could not be parsed as TOML.
    #[error("manifest is not valid TOML: {reason}")]
    InvalidToml {
        /// Parser diagnostic.
        reason: String,
    },
    /// A field is outside the static bootstrap contract.
    #[error("unknown field `{field}` is not part of the static manifest contract")]
    UnknownField {
        /// Rejected field name.
        field: String,
    },
    /// The document used a construct forbidden in non-executable manifests.
    #[error("bootstrap and package manifests forbid {construct}")]
    ForbiddenConstruct {
        /// Rejected construct class.
        construct: ForbiddenManifestConstruct,
    },
    /// The bootstrap schema version is unsupported.
    #[error("unsupported bootstrap schema {found}; supported schema is {supported}")]
    UnsupportedBootstrapSchema {
        /// Version found in the document.
        found: u32,
        /// Version implemented by this crate.
        supported: u32,
    },
    /// The package source-manifest schema version is unsupported.
    #[error("unsupported package source-manifest schema {found}; supported schema is {supported}")]
    UnsupportedPackageManifestSchema {
        /// Version found in the document.
        found: u32,
        /// Version implemented by this crate.
        supported: u32,
    },
    /// The bootstrap declared no root packages.
    #[error("composition bootstrap must declare at least one root package")]
    EmptyRoots,
    /// The bootstrap declared no source providers.
    #[error("composition bootstrap must declare source providers; ambient discovery is forbidden")]
    EmptySources,
    /// Automatic realization selection has no candidates.
    #[error("composition bootstrap must contain at least one realization preference")]
    EmptyRealizationOrder,
    /// Automatic realization policy repeats one kind.
    #[error("duplicate realization preference {kind:?}")]
    DuplicateRealizationPreference {
        /// Repeated realization kind.
        kind: RealizationKind,
    },
    /// Two source providers locate the same logical package.
    #[error("duplicate source provider for package {package}")]
    DuplicateSourcePackage {
        /// Duplicated package.
        package: PackageName,
    },
    /// A root package has no declared source provider.
    #[error("root package {package} has no declared source provider")]
    RootWithoutSource {
        /// Root missing a source.
        package: PackageName,
    },
    /// A locator used home, absolute, or other ambient path syntax.
    #[error("source path `{path}` is ambient and is not a canonical root-relative locator")]
    AmbientSourcePath {
        /// Rejected path text.
        path: String,
    },
    /// A fixed profile projection carried the wrong domains or a headless one was empty.
    #[error("profile projection {projection:?} has invalid explicit domains")]
    InvalidProjectionDomains {
        /// Projection whose domains were invalid.
        projection: ProfileKind,
    },
    /// The fixed R0 policy was paired with non-default effective limits.
    #[error("the R0 Nickel policy requires its exact frozen effective limits")]
    R0EvaluationLimitsMismatch,
    /// The selected Nickel policy is not implemented by this schema version.
    #[error("unsupported Nickel evaluation policy {policy}")]
    UnsupportedEvaluationPolicy {
        /// Rejected policy ID.
        policy: String,
    },
    /// One effective evaluator limit was zero.
    #[error("Nickel evaluation limit `{field}` must be nonzero")]
    ZeroEvaluationLimit {
        /// Invalid limit field.
        field: &'static str,
    },
    /// A package declares no execution domains.
    #[error("package {package} must declare at least one domain")]
    EmptyPackageDomains {
        /// Invalid package.
        package: PackageName,
    },
    /// A package declares no realization candidates.
    #[error("package {package} must declare at least one realization")]
    EmptyRealizations {
        /// Invalid package.
        package: PackageName,
    },
    /// Source inclusion listed no files or directories.
    #[error("package {package} must declare an explicit source inclusion set")]
    EmptySourceInclusion {
        /// Invalid package.
        package: PackageName,
    },
    /// Alias import requires the `default` public Nickel entry.
    #[error("package {package} must declare a `default` Nickel public entrypoint")]
    MissingDefaultNickelEntrypoint {
        /// Invalid package.
        package: PackageName,
    },
    /// A named public Nickel entry is not a package-local alias.
    #[error("package {package} has invalid Nickel entrypoint name `{name}`: {reason}")]
    InvalidNickelEntrypointName {
        /// Invalid package.
        package: PackageName,
        /// Rejected entry name.
        name: String,
        /// Alias grammar diagnostic.
        reason: String,
    },
    /// Two public entries point at the same logical file.
    #[error("duplicate Nickel public entrypoint path `{path}`")]
    DuplicateNickelEntrypointPath {
        /// Duplicated logical path.
        path: CanonicalLogicalPath,
    },
    /// A public Nickel entry is outside the declared source inclusion set.
    #[error("package {package} entrypoint `{path}` is not covered by source inclusion")]
    EntrypointOutsideInclusion {
        /// Invalid package.
        package: PackageName,
        /// Uncovered entry path.
        path: CanonicalLogicalPath,
    },
    /// Two aliases refer to the same logical dependency package.
    #[error("duplicate dependency package {package}")]
    DuplicateDependencyPackage {
        /// Duplicated dependency.
        package: PackageName,
    },
    /// A dependency alias collides with the versioned library alias.
    #[error("dependency alias `{alias}` is reserved for latticeaxiom.lib")]
    ReservedLibraryAlias {
        /// Rejected alias.
        alias: String,
    },
    /// An optional dependency has no activation feature or a required edge has one.
    #[error("dependency {dependency} in package {package} has inconsistent optional activation")]
    InvalidDependencyActivation {
        /// Declaring package.
        package: PackageName,
        /// Dependency package.
        dependency: PackageName,
    },
    /// An optional dependency activation references an undeclared feature.
    #[error("dependency {dependency} in package {package} references unknown feature `{feature}`")]
    UnknownDependencyFeature {
        /// Declaring package.
        package: PackageName,
        /// Dependency package.
        dependency: PackageName,
        /// Unknown package-local feature.
        feature: String,
    },
    /// A realization eligibility condition references an undeclared feature.
    #[error(
        "realization {realization} in package {package} references unknown feature `{feature}`"
    )]
    UnknownRealizationFeature {
        /// Declaring package.
        package: PackageName,
        /// Realization with the invalid condition.
        realization: RealizationId,
        /// Unknown package-local feature.
        feature: String,
    },
    /// A conditional row references a domain not declared by its package.
    #[error("row `{row}` uses a domain outside package {package}")]
    DomainOutsidePackage {
        /// Invalid package.
        package: PackageName,
        /// Stable row key.
        row: String,
    },
    /// Realization map key and row identity disagree.
    #[error("realization map key {key} does not match row value {value}")]
    RealizationKeyMismatch {
        /// Map key.
        key: RealizationId,
        /// ID stored in the row.
        value: RealizationId,
    },
    /// Realization domains are empty or escape package domains.
    #[error("realization {realization} has invalid domains for package {package}")]
    InvalidRealizationDomains {
        /// Invalid package.
        package: PackageName,
        /// Invalid realization.
        realization: RealizationId,
    },
    /// Realization trust exceeds the package trust declaration.
    #[error("realization {realization} exceeds the trust declared by package {package}")]
    RealizationTrustExceedsPackage {
        /// Invalid package.
        package: PackageName,
        /// Invalid realization.
        realization: RealizationId,
    },
    /// Realization interfaces or engine-build receipt conflict with its kind.
    #[error("realization {realization} in package {package} has an invalid {kind:?} boundary")]
    InvalidRealizationBoundary {
        /// Invalid package.
        package: PackageName,
        /// Invalid realization.
        realization: RealizationId,
        /// Declared realization kind.
        kind: RealizationKind,
    },
    /// A capability map key disagrees with the row value.
    #[error("capability map key {key} does not match row value {value}")]
    CapabilityKeyMismatch {
        /// Map key.
        key: String,
        /// Capability stored in the row.
        value: String,
    },
    /// A parameter map key, row ID, or declaring package disagrees.
    #[error("parameter `{key}` in package {package} disagrees with row `{value}` or owner")]
    ParameterDeclarationMismatch {
        /// Declaring package.
        package: PackageName,
        /// Parameter map key.
        key: String,
        /// Parameter row ID.
        value: String,
    },
}

/// Construct classes forbidden in non-executable bootstrap and package manifests.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ForbiddenManifestConstruct {
    /// Build or lifecycle scripts.
    Script,
    /// Functions or executable expressions.
    Function,
    /// Import of another document or package.
    Import,
    /// Environment expansion.
    EnvExpansion,
    /// Conditional I/O or cfg-style branching.
    ConditionalIo,
    /// Network, git, registry, or URL locators.
    Network,
}

impl ForbiddenManifestConstruct {
    /// Returns the stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Script => "scripts",
            Self::Function => "functions",
            Self::Import => "import",
            Self::EnvExpansion => "environment expansion",
            Self::ConditionalIo => "conditional I/O",
            Self::Network => "network",
        }
    }
}

impl fmt::Display for ForbiddenManifestConstruct {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn compare_source_providers(
    left: &BootstrapSourceProviderV1,
    right: &BootstrapSourceProviderV1,
) -> Ordering {
    left.kind_rank()
        .cmp(&right.kind_rank())
        .then_with(|| left.package().cmp(right.package()))
        .then_with(|| match (left, right) {
            (
                BootstrapSourceProviderV1::Path {
                    path: left_path, ..
                },
                BootstrapSourceProviderV1::Path {
                    path: right_path, ..
                },
            ) => left_path.cmp(right_path),
            (
                BootstrapSourceProviderV1::LocalCatalog {
                    version: left_version,
                    catalog: left_catalog,
                    ..
                },
                BootstrapSourceProviderV1::LocalCatalog {
                    version: right_version,
                    catalog: right_catalog,
                    ..
                },
            ) => left_version.exact_cmp(right_version).then_with(|| {
                left_catalog
                    .as_ref()
                    .map(CanonicalLogicalPath::as_str)
                    .cmp(&right_catalog.as_ref().map(CanonicalLogicalPath::as_str))
            }),
            (
                BootstrapSourceProviderV1::Fixture {
                    version: left_version,
                    source_id: left_source,
                    path: left_path,
                    ..
                },
                BootstrapSourceProviderV1::Fixture {
                    version: right_version,
                    source_id: right_source,
                    path: right_path,
                    ..
                },
            ) => left_version
                .exact_cmp(right_version)
                .then_with(|| left_source.cmp(right_source))
                .then_with(|| {
                    left_path
                        .as_ref()
                        .map(CanonicalLogicalPath::as_str)
                        .cmp(&right_path.as_ref().map(CanonicalLogicalPath::as_str))
                }),
            _ => Ordering::Equal,
        })
}

fn parse_restricted_toml<T>(text: &str) -> Result<T, BootstrapManifestError>
where
    T: DeserializeOwned,
{
    let value: toml::Value =
        toml::from_str(text).map_err(|error| BootstrapManifestError::InvalidToml {
            reason: error.to_string(),
        })?;
    reject_forbidden_toml(&value)?;
    T::deserialize(value).map_err(|error| map_deserialize_error(&error))
}

fn map_deserialize_error(error: &toml::de::Error) -> BootstrapManifestError {
    let reason = error.to_string();
    if let Some(field) = unknown_field_name(&reason) {
        BootstrapManifestError::UnknownField { field }
    } else {
        BootstrapManifestError::InvalidToml { reason }
    }
}

fn unknown_field_name(reason: &str) -> Option<String> {
    let marker = "unknown field `";
    let start = reason.find(marker)? + marker.len();
    let field = reason[start..].split('`').next()?.to_owned();
    (!field.is_empty()).then_some(field)
}

fn reject_forbidden_toml(value: &toml::Value) -> Result<(), BootstrapManifestError> {
    match value {
        toml::Value::String(text) => reject_forbidden_string(text),
        toml::Value::Array(items) => items.iter().try_for_each(reject_forbidden_toml),
        toml::Value::Table(table) => {
            for (key, nested) in table {
                if let Some(construct) = forbidden_key(key) {
                    return Err(BootstrapManifestError::ForbiddenConstruct { construct });
                }
                if normalize_key(key) == "kind"
                    && let toml::Value::String(kind) = nested
                    && let Some(construct) = forbidden_source_kind(kind)
                {
                    return Err(BootstrapManifestError::ForbiddenConstruct { construct });
                }
                reject_forbidden_toml(nested)?;
            }
            Ok(())
        }
        toml::Value::Boolean(_)
        | toml::Value::Integer(_)
        | toml::Value::Float(_)
        | toml::Value::Datetime(_) => Ok(()),
    }
}

fn reject_forbidden_string(text: &str) -> Result<(), BootstrapManifestError> {
    if looks_like_env_expansion(text) {
        return Err(BootstrapManifestError::ForbiddenConstruct {
            construct: ForbiddenManifestConstruct::EnvExpansion,
        });
    }
    if looks_like_network(text) {
        return Err(BootstrapManifestError::ForbiddenConstruct {
            construct: ForbiddenManifestConstruct::Network,
        });
    }
    Ok(())
}

fn forbidden_key(key: &str) -> Option<ForbiddenManifestConstruct> {
    match normalize_key(key).as_str() {
        "script" | "scripts" | "buildscript" | "exec" | "execute" | "command" | "shell" => {
            Some(ForbiddenManifestConstruct::Script)
        }
        "function" | "functions" | "fn" | "fun" | "lambda" => {
            Some(ForbiddenManifestConstruct::Function)
        }
        "import" | "imports" => Some(ForbiddenManifestConstruct::Import),
        "env" | "environment" | "envvar" | "envvars" => {
            Some(ForbiddenManifestConstruct::EnvExpansion)
        }
        "if" | "then" | "else" | "cfg" | "match" | "cond" | "conditional" => {
            Some(ForbiddenManifestConstruct::ConditionalIo)
        }
        "url" | "urls" | "uri" | "git" | "http" | "https" | "registry" | "network" | "download"
        | "fetch" | "remote" => Some(ForbiddenManifestConstruct::Network),
        _ => None,
    }
}

fn forbidden_source_kind(kind: &str) -> Option<ForbiddenManifestConstruct> {
    match normalize_key(kind).as_str() {
        "git" | "http" | "https" | "registry" | "url" | "network" | "remote" | "download" => {
            Some(ForbiddenManifestConstruct::Network)
        }
        "script" | "command" | "shell" => Some(ForbiddenManifestConstruct::Script),
        _ => None,
    }
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|character| *character != '-' && *character != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn looks_like_env_expansion(text: &str) -> bool {
    text.contains('$') || windows_env_pattern(text)
}

fn windows_env_pattern(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while let Some(relative) = bytes[index..].iter().position(|byte| *byte == b'%') {
        let start = index + relative;
        let Some(end_relative) = bytes[start + 1..].iter().position(|byte| *byte == b'%') else {
            return false;
        };
        let end = start + 1 + end_relative;
        let name = &bytes[start + 1..end];
        if !name.is_empty()
            && (name[0].is_ascii_alphabetic() || name[0] == b'_')
            && name
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            return true;
        }
        index = start + 1;
    }
    false
}

fn looks_like_network(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("://")
        || lower.starts_with("git@")
        || lower.starts_with("\\\\")
        || lower.contains("\\\\")
}

fn path_is_ambient(path: &CanonicalLogicalPath) -> bool {
    path.as_str().starts_with('~')
}

fn inclusion_covers(include: &BTreeSet<CanonicalLogicalPath>, path: &CanonicalLogicalPath) -> bool {
    include.iter().any(|item| {
        item == path
            || path
                .as_str()
                .as_bytes()
                .get(item.as_str().len())
                .is_some_and(|byte| *byte == b'/')
                && path.as_str().starts_with(item.as_str())
    })
}

fn ensure_domain_subset(
    package: &PackageName,
    row: &str,
    row_domains: &BTreeSet<PackageDomain>,
    package_domains: &BTreeSet<PackageDomain>,
) -> Result<(), BootstrapManifestError> {
    if row_domains.is_subset(package_domains) {
        Ok(())
    } else {
        Err(BootstrapManifestError::DomainOutsidePackage {
            package: package.clone(),
            row: row.to_owned(),
        })
    }
}

fn validate_projection_domains(
    projection: ProfileKind,
    domains: &BTreeSet<PackageDomain>,
) -> Result<(), BootstrapManifestError> {
    let expected = match projection {
        ProfileKind::ClientShell => BTreeSet::from([PackageDomain::Client]),
        ProfileKind::ClientWorld => {
            BTreeSet::from([PackageDomain::Authoritative, PackageDomain::Client])
        }
        ProfileKind::DedicatedServer => {
            BTreeSet::from([PackageDomain::Authoritative, PackageDomain::Server])
        }
        ProfileKind::Tool => BTreeSet::from([PackageDomain::Tool]),
        ProfileKind::HeadlessTest if !domains.is_empty() => return Ok(()),
        ProfileKind::HeadlessTest => BTreeSet::new(),
    };
    if domains == &expected && !domains.is_empty() {
        Ok(())
    } else {
        Err(BootstrapManifestError::InvalidProjectionDomains { projection })
    }
}

fn validate_evaluation_policy(
    projection: ProfileKind,
    policy: &StableId,
    limits: &NickelEvaluationLimits,
) -> Result<(), BootstrapManifestError> {
    if policy.namespace() != "latticeaxiom" {
        return Err(BootstrapManifestError::UnsupportedEvaluationPolicy {
            policy: policy.to_string(),
        });
    }
    if policy.as_str() == R0_NICKEL_EVALUATION_POLICY {
        if limits == &NickelEvaluationLimits::default() {
            Ok(())
        } else {
            Err(BootstrapManifestError::R0EvaluationLimitsMismatch)
        }
    } else if projection == ProfileKind::Tool
        && policy.kind() == "nickel-evaluation-policy"
        && policy.major().is_some()
    {
        validate_nonzero_limits(limits)
    } else {
        Err(BootstrapManifestError::UnsupportedEvaluationPolicy {
            policy: policy.to_string(),
        })
    }
}

fn validate_nonzero_limits(limits: &NickelEvaluationLimits) -> Result<(), BootstrapManifestError> {
    let fields = [
        ("wall_clock_ms", limits.wall_clock_ms),
        ("memory_bytes", limits.memory_bytes),
        ("recursion_depth", u64::from(limits.recursion_depth)),
        ("import_depth", u64::from(limits.import_depth)),
        ("imported_files", u64::from(limits.imported_files)),
        ("source_bytes", limits.source_bytes),
        ("output_bytes", limits.output_bytes),
        (
            "retained_diagnostics",
            u64::from(limits.retained_diagnostics),
        ),
    ];
    if let Some((field, _)) = fields.into_iter().find(|(_, value)| *value == 0) {
        Err(BootstrapManifestError::ZeroEvaluationLimit { field })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAPPY_BOOTSTRAP: &str = r#"
schema_version = 1
projection = "headless-test"
projection_domains = ["authoritative"]
evaluation_policy = "latticeaxiom:nickel-evaluation-policy/r0@1"
realization_policy = ["data"]
nickel_profile_entry = "profiles/test.ncl"

[roots.terrenia]
version = "=0.1.0"
realization = { mode = "auto" }

[roots."@latticeaxiom/settings"]
version = "=0.1.0"
realization = { mode = "auto" }

[[sources]]
kind = "workspace"
package = "terrenia"

[[sources]]
kind = "path"
package = "@terrenia/blocks"
path = "packages/terrenia/blocks"

[[sources]]
kind = "local-catalog"
package = "@latticeaxiom/settings"
version = "0.1.0"

[[sources]]
kind = "fixture"
package = "@latticeaxiom/observability"
version = "0.1.0"
source_id = "latticeaxiom:source/observability"
path = "packages/latticeaxiom/observability"
"#;

    const HAPPY_PACKAGE: &str = r#"
schema_version = 1
name = "terrenia"
version = "0.1.0"
domains = ["authoritative", "client"]
trust = "data-only"

[dependencies.blocks]
package = "@terrenia/blocks"
version = "~0.1.0"
optional = false
domains = ["authoritative"]

[features.worldgen]
default = true
domains = ["authoritative"]

[requires."latticeaxiom:capability/content-blocks@1"]
capability = "latticeaxiom:capability/content-blocks@1"
version = "^1.0.0"
provider = "@terrenia/blocks"
cardinality = "exactly-one"
domains = ["authoritative"]

[realizations.data]
id = "data"
kind = "data"
domains = ["authoritative", "client"]
artifact = { kind = "data-root", path = "data" }
trust = "data-only"

[nickel_public_entrypoints]
default = "package.ncl"

[source_inclusion]
include = ["package.ncl", "data"]
"#;

    #[test]
    fn parses_valid_bootstrap_and_package_manifests() {
        let bootstrap = CompositionBootstrapV1::from_toml_str(HAPPY_BOOTSTRAP)
            .unwrap_or_else(|error| panic!("valid bootstrap must parse: {error}"));
        assert_eq!(
            bootstrap.schema_version,
            COMPOSITION_BOOTSTRAP_SCHEMA_VERSION
        );
        assert_eq!(bootstrap.projection, ProfileKind::HeadlessTest);
        assert_eq!(bootstrap.roots.len(), 2);
        assert_eq!(bootstrap.sources.len(), 4);
        assert!(
            bootstrap
                .sources
                .iter()
                .any(|source| matches!(source, BootstrapSourceProviderV1::Workspace { .. }))
        );
        assert!(
            bootstrap
                .sources
                .iter()
                .any(|source| matches!(source, BootstrapSourceProviderV1::Path { .. }))
        );
        assert!(
            bootstrap
                .sources
                .iter()
                .any(|source| matches!(source, BootstrapSourceProviderV1::LocalCatalog { .. }))
        );
        assert!(
            bootstrap
                .sources
                .iter()
                .any(|source| matches!(source, BootstrapSourceProviderV1::Fixture { .. }))
        );

        let manifest = PackageSourceManifestV1::from_toml_str(HAPPY_PACKAGE)
            .unwrap_or_else(|error| panic!("valid package manifest must parse: {error}"));
        assert_eq!(
            manifest.schema_version,
            PACKAGE_SOURCE_MANIFEST_SCHEMA_VERSION
        );
        assert_eq!(manifest.name.as_str(), "terrenia");
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(
            manifest.nickel_public_entrypoints[DEFAULT_NICKEL_ENTRYPOINT].as_str(),
            "package.ncl"
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        let bootstrap = format!("{HAPPY_BOOTSTRAP}\nunknown_field = true\n");
        let error = CompositionBootstrapV1::from_toml_str(&bootstrap)
            .expect_err("unknown bootstrap field must be rejected");
        assert!(
            matches!(error, BootstrapManifestError::UnknownField { ref field } if field == "unknown_field"),
            "unexpected bootstrap error: {error}"
        );

        let manifest = format!("{HAPPY_PACKAGE}\nmetadata = {{ display_name = \"x\" }}\n");
        let error = PackageSourceManifestV1::from_toml_str(&manifest)
            .expect_err("unknown package field must be rejected");
        assert!(
            matches!(error, BootstrapManifestError::UnknownField { ref field } if field == "metadata"),
            "unexpected package error: {error}"
        );
    }

    #[test]
    fn rejects_forbidden_script_env_and_network() {
        let with_scripts = format!("{HAPPY_BOOTSTRAP}\n[scripts]\nbuild = \"echo hi\"\n");
        assert!(
            matches!(
                CompositionBootstrapV1::from_toml_str(&with_scripts),
                Err(BootstrapManifestError::ForbiddenConstruct {
                    construct: ForbiddenManifestConstruct::Script,
                })
            ),
            "scripts must be rejected"
        );

        let with_import = format!("{HAPPY_PACKAGE}\nimport = \"package.ncl\"\n");
        assert!(
            matches!(
                PackageSourceManifestV1::from_toml_str(&with_import),
                Err(BootstrapManifestError::ForbiddenConstruct {
                    construct: ForbiddenManifestConstruct::Import,
                })
            ),
            "import must be rejected"
        );

        let with_function = format!("{HAPPY_BOOTSTRAP}\nfun = \"ignored\"\n");
        assert!(
            matches!(
                CompositionBootstrapV1::from_toml_str(&with_function),
                Err(BootstrapManifestError::ForbiddenConstruct {
                    construct: ForbiddenManifestConstruct::Function,
                })
            ),
            "functions must be rejected"
        );

        let with_env =
            HAPPY_BOOTSTRAP.replace("packages/terrenia/blocks", "$HOME/packages/terrenia/blocks");
        assert!(
            matches!(
                CompositionBootstrapV1::from_toml_str(&with_env),
                Err(BootstrapManifestError::ForbiddenConstruct {
                    construct: ForbiddenManifestConstruct::EnvExpansion,
                })
            ),
            "environment expansion must be rejected"
        );

        let with_network = HAPPY_BOOTSTRAP.replace(
            "packages/terrenia/blocks",
            "https://example.invalid/terrenia.tgz",
        );
        assert!(
            matches!(
                CompositionBootstrapV1::from_toml_str(&with_network),
                Err(BootstrapManifestError::ForbiddenConstruct {
                    construct: ForbiddenManifestConstruct::Network,
                })
            ),
            "network locators must be rejected"
        );

        let with_git = HAPPY_BOOTSTRAP.replace("kind = \"path\"", "kind = \"git\"");
        assert!(
            matches!(
                CompositionBootstrapV1::from_toml_str(&with_git),
                Err(BootstrapManifestError::ForbiddenConstruct {
                    construct: ForbiddenManifestConstruct::Network,
                })
            ),
            "git source kinds must be rejected"
        );
    }

    #[test]
    fn hash_is_independent_of_toml_layout() {
        let compact_bootstrap = CompositionBootstrapV1::from_toml_str(HAPPY_BOOTSTRAP)
            .unwrap_or_else(|error| panic!("compact bootstrap must parse: {error}"));
        let laid_out_bootstrap = CompositionBootstrapV1::from_toml_str(
            r#"
# Comments and reordered keys are not identity.
nickel_profile_entry = "profiles/test.ncl"
realization_policy = ["data"]
evaluation_policy = "latticeaxiom:nickel-evaluation-policy/r0@1"
projection_domains = ["authoritative"]
projection = "headless-test"
schema_version = 1

[roots."@latticeaxiom/settings"]
realization = { mode = "auto" }
version = "=0.1.0"

[roots.terrenia]
realization = { mode = "auto" }
version = "=0.1.0"

[[sources]]
package = "@latticeaxiom/observability"
path = "packages/latticeaxiom/observability"
source_id = "latticeaxiom:source/observability"
version = "0.1.0"
kind = "fixture"

[[sources]]
package = "terrenia"
kind = "workspace"

[[sources]]
path = "packages/terrenia/blocks"
package = "@terrenia/blocks"
kind = "path"

[[sources]]
version = "0.1.0"
package = "@latticeaxiom/settings"
kind = "local-catalog"
"#,
        )
        .unwrap_or_else(|error| panic!("reordered bootstrap must parse: {error}"));
        assert_eq!(
            compact_bootstrap
                .canonical_hash()
                .unwrap_or_else(|error| panic!("compact bootstrap hash failed: {error}")),
            laid_out_bootstrap
                .canonical_hash()
                .unwrap_or_else(|error| panic!("reordered bootstrap hash failed: {error}"))
        );

        let compact_package = PackageSourceManifestV1::from_toml_str(HAPPY_PACKAGE)
            .unwrap_or_else(|error| panic!("compact package must parse: {error}"));
        let laid_out_package = PackageSourceManifestV1::from_toml_str(
            r#"
trust = "data-only"
domains = [ "client", "authoritative" ]
version = "0.1.0"
name = "terrenia"
schema_version = 1

[source_inclusion]
include = [ "data", "package.ncl" ]

[nickel_public_entrypoints]
default = "package.ncl"

[realizations.data]
trust = "data-only"
artifact = { path = "data", kind = "data-root" }
domains = ["client", "authoritative"]
kind = "data"
id = "data"

[requires."latticeaxiom:capability/content-blocks@1"]
domains = ["authoritative"]
cardinality = "exactly-one"
provider = "@terrenia/blocks"
version = "^1.0.0"
capability = "latticeaxiom:capability/content-blocks@1"

[features.worldgen]
domains = ["authoritative"]
default = true

[dependencies.blocks]
domains = ["authoritative"]
optional = false
version = "~0.1.0"
package = "@terrenia/blocks"
"#,
        )
        .unwrap_or_else(|error| panic!("reordered package must parse: {error}"));
        assert_eq!(
            compact_package
                .canonical_hash()
                .unwrap_or_else(|error| panic!("compact package hash failed: {error}")),
            laid_out_package
                .canonical_hash()
                .unwrap_or_else(|error| panic!("reordered package hash failed: {error}"))
        );
    }
}
