//! Trusted host compatibility evidence consumed by package resolution.

use std::collections::BTreeMap;

use latticeaxiom_compose::InterfaceRequirement;
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageVersion, StableId, TargetTriple, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current schema version for trusted host compatibility evidence.
pub const HOST_COMPATIBILITY_SCHEMA_VERSION: u32 = 1;

/// One exact interface implementation exposed by the selected host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostInterfaceV1 {
    /// Exact semantic interface version implemented by the host.
    pub version: PackageVersion,
    /// Hash of the generated canonical interface table descriptor.
    pub descriptor_hash: CanonicalHash,
}

/// Trusted, target-specific host compatibility evidence.
///
/// This value is supplied by host build tooling, never by a package manifest.
/// Its canonical hash is sealed into resolver receipts so frozen replay cannot
/// silently move to a host with different ABI tables or build identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostCompatibilityV1 {
    /// Evidence schema version.
    pub schema_version: u32,
    /// Exact target on which the host process will execute.
    pub target: TargetTriple,
    /// Exact host build identity used by engine-coupled realizations.
    pub engine_build_id: Option<CanonicalHash>,
    /// Implemented interfaces keyed by canonical unversioned interface ID.
    pub interfaces: BTreeMap<StableId, HostInterfaceV1>,
}

impl HostCompatibilityV1 {
    /// Validates schema and canonical interface identities.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported schema, a non-interface key, a
    /// versioned key, or an all-zero descriptor hash.
    pub fn validate(&self) -> Result<(), HostCompatibilityError> {
        if self.schema_version != HOST_COMPATIBILITY_SCHEMA_VERSION {
            return Err(HostCompatibilityError::UnsupportedSchema {
                found: self.schema_version,
                supported: HOST_COMPATIBILITY_SCHEMA_VERSION,
            });
        }
        if self
            .engine_build_id
            .is_some_and(|hash| hash.as_bytes() == &[0; CanonicalHash::BYTE_LENGTH])
        {
            return Err(HostCompatibilityError::ZeroEngineBuildId);
        }
        for (interface, implementation) in &self.interfaces {
            if interface.kind() != "interface" {
                return Err(HostCompatibilityError::WrongInterfaceKind {
                    interface: interface.clone(),
                });
            }
            if interface.major().is_some() {
                return Err(HostCompatibilityError::VersionedInterfaceIdentity {
                    interface: interface.clone(),
                });
            }
            if implementation.descriptor_hash.as_bytes() == &[0; CanonicalHash::BYTE_LENGTH] {
                return Err(HostCompatibilityError::ZeroDescriptorHash {
                    interface: interface.clone(),
                });
            }
        }
        Ok(())
    }

    /// Computes the exact canonical hash sealed into a resolver receipt.
    ///
    /// # Errors
    ///
    /// Returns an error only if canonical JSON encoding fails.
    pub fn compatibility_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(self)
    }

    /// Returns whether the host implements a compatible version of an
    /// interface requirement.
    #[must_use]
    pub fn supports(&self, interface: &StableId, requirement: &InterfaceRequirement) -> bool {
        self.interfaces
            .get(interface)
            .is_some_and(|implementation| requirement.version.matches(&implementation.version))
    }
}

/// Invalid trusted host compatibility evidence.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HostCompatibilityError {
    /// The evidence schema is unsupported.
    #[error("unsupported host compatibility schema {found}; supported schema is {supported}")]
    UnsupportedSchema {
        /// Schema supplied by the host.
        found: u32,
        /// Schema supported by this resolver.
        supported: u32,
    },
    /// A map key is not an interface stable ID.
    #[error("host interface key {interface} does not use kind interface")]
    WrongInterfaceKind {
        /// Invalid interface identity.
        interface: StableId,
    },
    /// Interface versions belong in the implementation row, not the key.
    #[error("host interface key {interface} must be unversioned")]
    VersionedInterfaceIdentity {
        /// Invalid versioned interface identity.
        interface: StableId,
    },
    /// A canonical descriptor cannot use the all-zero sentinel hash.
    #[error("host interface {interface} has an all-zero descriptor hash")]
    ZeroDescriptorHash {
        /// Interface with an invalid descriptor claim.
        interface: StableId,
    },
    /// A present engine build identity cannot use the all-zero sentinel.
    #[error("host engine build identity has an all-zero hash")]
    ZeroEngineBuildId,
}
