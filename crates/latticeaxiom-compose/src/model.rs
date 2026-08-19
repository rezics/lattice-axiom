use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current schema version of [`CompositionSpec`].
pub const COMPOSITION_SCHEMA_VERSION: u32 = 1;

/// Current schema version of [`PackageManifest`].
pub const PACKAGE_SCHEMA_VERSION: u32 = 1;

/// Stable logical package identifier such as `latticeaxiom.official`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PackageId(String);

impl PackageId {
    /// Creates an identifier, validating its stable key syntax.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationError::InvalidPackageId`] when `value` is not a
    /// lowercase dot-separated identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_dotted_identifier(&value)
            .then_some(Self(value.clone()))
            .ok_or(ValidationError::InvalidPackageId { value })
    }

    /// Returns the identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Exact package version selected by the v1 package kernel.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PackageVersion(String);

impl PackageVersion {
    /// Creates an exact version without invoking a general version solver.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationError::InvalidExactVersion`] for an empty version or
    /// one containing a range operator.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_exact_version(&value)
            .then_some(Self(value.clone()))
            .ok_or(ValidationError::InvalidExactVersion { value })
    }

    /// Returns the exact version text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Versioned capability key such as `content.blocks@1`.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CapabilityId(String);

impl CapabilityId {
    /// Creates and validates a capability identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationError::InvalidCapabilityId`] unless the key has a
    /// lowercase dotted name and positive integer contract version.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_capability(&value)
            .then_some(Self(value.clone()))
            .ok_or(ValidationError::InvalidCapabilityId { value })
    }

    /// Returns the complete name and contract version.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A root profile's publishable identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDeclaration {
    /// Stable root profile identifier.
    pub id: PackageId,
    /// Exact release label for this root profile.
    pub version: PackageVersion,
}

/// An individual package's publishable identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageDeclaration {
    /// Stable package identifier.
    pub id: PackageId,
    /// Exact package version.
    pub version: PackageVersion,
}

/// Supported exact source request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRequest {
    /// A path relative to the project root.
    Path {
        /// Slash-normalized project-relative package directory.
        path: String,
    },
}

/// Realization selected for a logical package.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Realization {
    /// Trusted native code linked into the static game closure.
    #[serde(rename = "native_static", alias = "NativeStatic")]
    NativeStatic,
    /// Trusted native code loaded from a precompiled dynamic library.
    #[serde(rename = "native_dynamic", alias = "NativeDynamic")]
    NativeDynamic,
    /// Pure declarative data compiled into runtime tables.
    #[serde(rename = "data", alias = "Data")]
    Data,
}

impl fmt::Display for Realization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NativeStatic => "native_static",
            Self::NativeDynamic => "native_dynamic",
            Self::Data => "data",
        })
    }
}

/// One exact package request from a root game profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRequest {
    /// Requested package identity.
    pub id: PackageId,
    /// Requested exact version.
    pub version: PackageVersion,
    /// Exact local source for milestone 2.
    pub source: SourceRequest,
    /// Requested realization.
    pub realization: Realization,
}

/// Requirement for one named and versioned capability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    /// Required capability contract.
    pub capability: CapabilityId,
    /// Explicit provider selection, or `None` when exactly one must exist.
    pub provider: Option<PackageId>,
}

/// Policy for trusted native code in a composed game.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum NativeCodePolicy {
    /// Reject every native realization.
    #[serde(rename = "deny", alias = "Deny")]
    Deny,
    /// Permit native realizations from the explicitly composed local closure.
    #[serde(rename = "trusted_only", alias = "TrustedOnly")]
    TrustedOnly,
}

/// Root policy values that affect package realization and lock identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionPolicy {
    /// Native code trust policy.
    pub native_code: NativeCodePolicy,
    /// Exact target triple recorded in the lock.
    pub target: String,
    /// Toolchain identity recorded in the lock.
    pub toolchain: String,
    /// Capability prefixes whose semantics are authoritative.
    pub authoritative: Vec<String>,
}

/// Fully evaluated result of a Nickel root game profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionSpec {
    /// Semantic schema version.
    pub schema_version: u32,
    /// Root profile identity.
    pub root_profile: ProfileDeclaration,
    /// Exact package requests. Input ordering carries no semantics.
    pub package_requests: Vec<PackageRequest>,
    /// Capability requirements resolved by the Rust package kernel.
    pub capability_requirements: Vec<CapabilityRequirement>,
    /// Ordered fallback preferences when a request omits a realization.
    pub realization_preferences: Vec<Realization>,
    /// Fully evaluated finite composition parameters.
    pub parameters: BTreeMap<String, String>,
    /// Trust and target policy.
    pub policy: CompositionPolicy,
}

impl CompositionSpec {
    /// Validates schema, identifiers, exact versions, and uniqueness.
    ///
    /// # Errors
    ///
    /// Returns a precise [`ValidationError`] for the first deterministic
    /// violation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != COMPOSITION_SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedCompositionSchema {
                found: self.schema_version,
                supported: COMPOSITION_SCHEMA_VERSION,
            });
        }
        validate_package_id(&self.root_profile.id)?;
        validate_package_version(&self.root_profile.version)?;
        let mut packages = self.package_requests.iter().collect::<Vec<_>>();
        packages.sort_by(|left, right| left.id.cmp(&right.id));
        for pair in packages.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(ValidationError::DuplicatePackageRequest {
                    id: pair[0].id.clone(),
                });
            }
        }
        for request in packages {
            validate_package_id(&request.id)?;
            validate_package_version(&request.version)?;
            match &request.source {
                SourceRequest::Path { path } => validate_source_path(path)?,
            }
            if matches!(self.policy.native_code, NativeCodePolicy::Deny)
                && matches!(
                    request.realization,
                    Realization::NativeStatic | Realization::NativeDynamic
                )
            {
                return Err(ValidationError::NativeRealizationDenied {
                    id: request.id.clone(),
                    realization: request.realization,
                });
            }
        }
        for requirement in &self.capability_requirements {
            validate_capability_id(&requirement.capability)?;
            if let Some(provider) = &requirement.provider {
                validate_package_id(provider)?;
            }
        }
        let mut requirements = self.capability_requirements.iter().collect::<Vec<_>>();
        requirements.sort_by(|left, right| left.capability.cmp(&right.capability));
        for pair in requirements.windows(2) {
            if pair[0].capability == pair[1].capability {
                return Err(ValidationError::DuplicateCapabilityRequirement {
                    capability: pair[0].capability.clone(),
                });
            }
        }
        if self.policy.target.trim().is_empty() {
            return Err(ValidationError::EmptyPolicyValue { field: "target" });
        }
        if self.policy.toolchain.trim().is_empty() {
            return Err(ValidationError::EmptyPolicyValue { field: "toolchain" });
        }
        Ok(())
    }
}

/// Exact dependency on another package already requested by the root profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyRequest {
    /// Depended-on package.
    pub id: PackageId,
    /// Required exact version.
    pub version: PackageVersion,
}

/// A data-defined block registration from a package manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDeclaration {
    /// Package-local stable block key.
    pub id: String,
    /// User-facing fallback name.
    pub display_name: String,
    /// Whether the block participates in collision and opaque meshing.
    pub solid: bool,
    /// Whether this registration is the unique empty-space block mapped to ID zero.
    pub is_air: bool,
    /// Linear RGBA color used by the first demo.
    pub color: [u8; 4],
}

/// Content and capabilities exported by one package.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProvidedContent {
    /// Capability contracts implemented by the package.
    pub capabilities: Vec<CapabilityId>,
    /// Data block declarations registered by the package.
    pub blocks: Vec<BlockDeclaration>,
}

/// Fully evaluated result of a package's `package.ncl`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    /// Semantic schema version.
    pub schema_version: u32,
    /// Package identity.
    pub package: PackageDeclaration,
    /// Exact package dependencies.
    pub dependencies: Vec<DependencyRequest>,
    /// Public capabilities and data registrations.
    pub provides: ProvidedContent,
    /// Realizations published for this logical package.
    pub realizations: Vec<Realization>,
}

impl PackageManifest {
    /// Validates schema, stable keys, exact dependencies, and uniqueness.
    ///
    /// # Errors
    ///
    /// Returns a precise [`ValidationError`] for the first deterministic
    /// violation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != PACKAGE_SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedPackageSchema {
                found: self.schema_version,
                supported: PACKAGE_SCHEMA_VERSION,
            });
        }
        validate_package_id(&self.package.id)?;
        validate_package_version(&self.package.version)?;
        if self.realizations.is_empty() {
            return Err(ValidationError::NoRealizations {
                id: self.package.id.clone(),
            });
        }
        let mut dependencies = self.dependencies.iter().collect::<Vec<_>>();
        dependencies.sort_by(|left, right| left.id.cmp(&right.id));
        for pair in dependencies.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(ValidationError::DuplicateDependency {
                    package: self.package.id.clone(),
                    dependency: pair[0].id.clone(),
                });
            }
        }
        for dependency in dependencies {
            validate_package_id(&dependency.id)?;
            validate_package_version(&dependency.version)?;
            if dependency.id == self.package.id {
                return Err(ValidationError::SelfDependency {
                    id: self.package.id.clone(),
                });
            }
        }
        let mut capabilities = self.provides.capabilities.iter().collect::<Vec<_>>();
        capabilities.sort();
        for pair in capabilities.windows(2) {
            if pair[0] == pair[1] {
                return Err(ValidationError::DuplicateCapability {
                    capability: (*pair[0]).clone(),
                });
            }
        }
        for capability in capabilities {
            validate_capability_id(capability)?;
        }
        let mut blocks = self.provides.blocks.iter().collect::<Vec<_>>();
        blocks.sort_by(|left, right| left.id.cmp(&right.id));
        for pair in blocks.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(ValidationError::DuplicateBlock {
                    package: self.package.id.clone(),
                    block: pair[0].id.clone(),
                });
            }
        }
        for block in blocks {
            if !validate_local_identifier(&block.id) {
                return Err(ValidationError::InvalidBlockId {
                    package: self.package.id.clone(),
                    block: block.id.clone(),
                });
            }
            if block.display_name.trim().is_empty() {
                return Err(ValidationError::EmptyBlockDisplayName {
                    package: self.package.id.clone(),
                    block: block.id.clone(),
                });
            }
            if block.is_air && block.solid {
                return Err(ValidationError::AirBlockIsSolid {
                    package: self.package.id.clone(),
                    block: block.id.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Semantic validation failures shared by game and package manifests.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ValidationError {
    /// The profile uses an unsupported semantic schema.
    #[error("composition schema {found} is unsupported; this binary supports {supported}")]
    UnsupportedCompositionSchema {
        /// Version found in the input.
        found: u32,
        /// Version supported by this crate.
        supported: u32,
    },
    /// The package uses an unsupported semantic schema.
    #[error("package schema {found} is unsupported; this binary supports {supported}")]
    UnsupportedPackageSchema {
        /// Version found in the input.
        found: u32,
        /// Version supported by this crate.
        supported: u32,
    },
    /// A package identifier is not a lowercase dotted stable key.
    #[error("invalid package id `{value}`; use lowercase dot-separated segments")]
    InvalidPackageId {
        /// Invalid identifier.
        value: String,
    },
    /// A version is not exact.
    #[error(
        "invalid exact version `{value}`; ranges and comparison operators are not supported in v1"
    )]
    InvalidExactVersion {
        /// Invalid version.
        value: String,
    },
    /// A capability identifier lacks a valid contract version.
    #[error(
        "invalid capability `{value}`; expected a dotted name followed by `@<positive integer>`"
    )]
    InvalidCapabilityId {
        /// Invalid capability identifier.
        value: String,
    },
    /// The root requests a package more than once.
    #[error("package `{id}` is requested more than once")]
    DuplicatePackageRequest {
        /// Duplicate package identifier.
        id: PackageId,
    },
    /// The root requires one capability more than once.
    #[error("capability `{capability}` is required more than once; keep one provider selection")]
    DuplicateCapabilityRequirement {
        /// Duplicate capability requirement.
        capability: CapabilityId,
    },
    /// A source path escapes or does not use portable relative syntax.
    #[error(
        "invalid local source path `{path}`; use a slash-separated project-relative path without `.` or `..`"
    )]
    InvalidSourcePath {
        /// Invalid source path.
        path: String,
    },
    /// Root policy denied a requested native realization.
    #[error("package `{id}` requests `{realization}`, but policy denies native code")]
    NativeRealizationDenied {
        /// Denied package.
        id: PackageId,
        /// Denied realization.
        realization: Realization,
    },
    /// A required policy value was empty.
    #[error("policy field `{field}` must not be empty")]
    EmptyPolicyValue {
        /// Policy field name.
        field: &'static str,
    },
    /// A package declared no realization.
    #[error("package `{id}` declares no realizations")]
    NoRealizations {
        /// Package identifier.
        id: PackageId,
    },
    /// A package directly depends on itself.
    #[error("package `{id}` directly depends on itself")]
    SelfDependency {
        /// Package identifier.
        id: PackageId,
    },
    /// A package lists one exact dependency more than once.
    #[error("package `{package}` depends on `{dependency}` more than once")]
    DuplicateDependency {
        /// Package with the duplicate edge.
        package: PackageId,
        /// Repeated dependency.
        dependency: PackageId,
    },
    /// A package lists the same capability more than once.
    #[error("capability `{capability}` is provided more than once")]
    DuplicateCapability {
        /// Duplicate capability.
        capability: CapabilityId,
    },
    /// A package lists the same block more than once.
    #[error("package `{package}` declares block `{block}` more than once")]
    DuplicateBlock {
        /// Owning package.
        package: PackageId,
        /// Duplicate local block key.
        block: String,
    },
    /// A block uses an invalid local key.
    #[error(
        "package `{package}` has invalid block id `{block}`; use lowercase letters, digits, `_`, or `-`"
    )]
    InvalidBlockId {
        /// Owning package.
        package: PackageId,
        /// Invalid local block key.
        block: String,
    },
    /// A block has no user-facing fallback name.
    #[error("package `{package}` block `{block}` has an empty display name")]
    EmptyBlockDisplayName {
        /// Owning package.
        package: PackageId,
        /// Block local key.
        block: String,
    },
    /// The special air block was incorrectly declared solid.
    #[error("package `{package}` block `{block}` is marked as air and must not be solid")]
    AirBlockIsSolid {
        /// Owning package.
        package: PackageId,
        /// Air block local key.
        block: String,
    },
}

fn validate_package_id(id: &PackageId) -> Result<(), ValidationError> {
    if validate_dotted_identifier(id.as_str()) {
        Ok(())
    } else {
        Err(ValidationError::InvalidPackageId {
            value: id.as_str().to_owned(),
        })
    }
}

fn validate_package_version(version: &PackageVersion) -> Result<(), ValidationError> {
    if validate_exact_version(version.as_str()) {
        Ok(())
    } else {
        Err(ValidationError::InvalidExactVersion {
            value: version.as_str().to_owned(),
        })
    }
}

fn validate_capability_id(capability: &CapabilityId) -> Result<(), ValidationError> {
    if validate_capability(capability.as_str()) {
        Ok(())
    } else {
        Err(ValidationError::InvalidCapabilityId {
            value: capability.as_str().to_owned(),
        })
    }
}

fn validate_dotted_identifier(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(validate_local_identifier)
}

fn validate_local_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn validate_exact_version(value: &str) -> bool {
    !value.trim().is_empty()
        && value == value.trim()
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'=' | b'<' | b'>' | b'^' | b'~' | b'*' | b',' | b' '))
}

fn validate_capability(value: &str) -> bool {
    value.rsplit_once('@').is_some_and(|(name, version)| {
        validate_dotted_identifier(name) && version.parse::<u32>().is_ok_and(|parsed| parsed > 0)
    })
}

fn validate_source_path(path: &str) -> Result<(), ValidationError> {
    let valid = !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if valid {
        Ok(())
    } else {
        Err(ValidationError::InvalidSourcePath {
            path: path.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{CapabilityId, PackageId, PackageVersion};

    #[test]
    fn stable_keys_accept_only_canonical_forms() {
        assert!(PackageId::new("latticeaxiom.official").is_ok());
        assert!(PackageId::new("LatticeAxiom.official").is_err());
        assert!(PackageId::new("latticeaxiom..official").is_err());
        assert!(CapabilityId::new("content.blocks@1").is_ok());
        assert!(CapabilityId::new("content.blocks@0").is_err());
        assert!(PackageVersion::new("0.1.0").is_ok());
        assert!(PackageVersion::new("^0.1").is_err());
    }
}
