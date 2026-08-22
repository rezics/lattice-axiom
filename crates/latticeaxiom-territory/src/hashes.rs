//! Typed hashes and deterministic hash domains.

use std::{fmt, str::FromStr};

use latticeaxiom_core::{CanonicalHash, StableId};
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};

use crate::{TerritoryError, TerritoryResult};

macro_rules! typed_hash {
    ($name:ident, $docs:literal) => {
        #[doc = $docs]
        #[repr(transparent)]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(CanonicalHash);

        impl $name {
            /// Wraps validated SHA-256 bytes.
            #[must_use]
            pub const fn from_hash(hash: CanonicalHash) -> Self {
                Self(hash)
            }

            /// Returns the canonical hash.
            #[must_use]
            pub const fn as_hash(&self) -> &CanonicalHash {
                &self.0
            }

            /// Returns the exact digest bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; CanonicalHash::BYTE_LENGTH] {
                self.0.as_bytes()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

typed_hash!(
    AtlasPlanHashV1,
    "Canonical identity of a validated D7 territory plan."
);
typed_hash!(
    TerritoryPlanReceiptHashV1,
    "Canonical identity of a finite compiled territory-plan receipt."
);
typed_hash!(
    TerritoryConflictDiagnosticHashV1,
    "Canonical identity of a territory ownership-conflict diagnostic."
);
typed_hash!(
    CavePortalIdV1,
    "Direction-independent identity of a validated cave portal."
);
typed_hash!(
    CaveEntranceIdV1,
    "Direction-independent identity of a planned cave surface entrance."
);
typed_hash!(
    CaveTopologyPlanHashV1,
    "Canonical identity of a compiled V6 cave topology graph and passability plan."
);
typed_hash!(
    CavePassabilityReceiptHashV1,
    "Canonical identity of one entrance-to-destination passability receipt."
);
typed_hash!(
    CaveTopologyNodeIdV1,
    "Direction-independent identity of one cave topology graph node."
);
typed_hash!(
    HydrologyPlanHashV1,
    "Canonical identity of an abstract bounded hydrology plan."
);
typed_hash!(
    TransitionReceiptHashV1,
    "Canonical identity of planning-cell epoch transition evidence."
);

/// Stable terrain ownership domain identity.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TerritoryDomainIdV1(StableId);

impl TerritoryDomainIdV1 {
    /// Validates a stable ID as a terrain ownership domain.
    ///
    /// # Errors
    ///
    /// Returns [`TerritoryError::InvalidStableKind`] unless the registration
    /// kind is `territory-domain`.
    pub fn new(value: StableId) -> TerritoryResult<Self> {
        if value.kind() != "territory-domain" {
            return Err(TerritoryError::InvalidStableKind {
                value: value.to_string(),
                expected: "territory-domain",
            });
        }
        Ok(Self(value))
    }

    /// Returns the underlying stable ID.
    #[must_use]
    pub const fn as_stable_id(&self) -> &StableId {
        &self.0
    }

    /// Returns canonical identity text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for TerritoryDomainIdV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for TerritoryDomainIdV1 {
    type Err = TerritoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let stable_id = value.parse::<StableId>()?;
        Self::new(stable_id)
    }
}

impl<'de> Deserialize<'de> for TerritoryDomainIdV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let stable_id = StableId::deserialize(deserializer)?;
        Self::new(stable_id).map_err(de::Error::custom)
    }
}

pub(crate) fn domain_hash(domain: &[u8], parts: &[&[u8]]) -> CanonicalHash {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        let length = u64::try_from(part.len()).unwrap_or(u64::MAX);
        hasher.update(length.to_be_bytes());
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; CanonicalHash::BYTE_LENGTH];
    bytes.copy_from_slice(&digest);
    CanonicalHash::from_bytes(bytes)
}

pub(crate) fn hash_u64(domain: &[u8], parts: &[&[u8]]) -> u64 {
    let digest = domain_hash(domain, parts);
    let mut bytes = [0_u8; size_of::<u64>()];
    bytes.copy_from_slice(&digest.as_bytes()[..size_of::<u64>()]);
    u64::from_be_bytes(bytes)
}
