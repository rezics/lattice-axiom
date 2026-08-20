use latticeaxiom_core::CanonicalHash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
            /// Wraps an already validated canonical SHA-256 hash.
            #[must_use]
            pub const fn from_hash(hash: CanonicalHash) -> Self {
                Self(hash)
            }

            /// Returns the underlying canonical SHA-256 hash.
            #[must_use]
            pub const fn as_hash(&self) -> &CanonicalHash {
                &self.0
            }

            /// Returns the exact 32 digest bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; CanonicalHash::BYTE_LENGTH] {
                self.0.as_bytes()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

typed_hash!(
    WorldgenConfigHashV1,
    "Canonical hash of a fully materialized `WorldgenConfigV1`."
);
typed_hash!(
    GeneratorFingerprintV1,
    "Aggregate fingerprint of the resolved output-affecting providers."
);
typed_hash!(
    LockedClosureFingerprintV1,
    "Fingerprint of the sorted locked package, source, artifact, and realization receipts."
);
typed_hash!(
    GenerationInputHashV1,
    "Hash of output-affecting generation inputs, excluding package coordinates and realization."
);
typed_hash!(
    GenerationProvenanceHashV1,
    "Hash that adds locked package and artifact provenance to a generation input hash."
);
typed_hash!(
    GenerationEpochIdV1,
    "Identity frozen for a planning cell before its first durable materialization."
);
typed_hash!(
    PlanActivationIdV1,
    "Runtime activation token used to reject stale asynchronous generation publication."
);
typed_hash!(
    PlanningCellIdV1,
    "Stable identity of a coarse two-dimensional planning cell."
);
typed_hash!(
    BoundaryIdV1,
    "Direction-independent identity of a boundary between adjacent planning cells."
);
typed_hash!(
    SharedFaceHashV1,
    "Direction-independent identity of one shared chunk face used by the coarse cave plan."
);
typed_hash!(
    SnapshotChecksumV1,
    "Checksum of the exact provisional snapshot bytes handed to storage."
);

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

pub(crate) fn concatenated_hash(domain: &[u8], parts: &[&[u8]]) -> CanonicalHash {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
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
