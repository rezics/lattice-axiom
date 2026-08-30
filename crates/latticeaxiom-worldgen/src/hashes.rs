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
    TerrainConfigHashV2,
    "Canonical hash of a fully resolved `TerrainConfigV2`."
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
typed_hash!(
    NaturalLayerHashV1,
    "Canonical hash of the optional V5 natural-layer config, roles, and providers."
);
typed_hash!(
    RiverBasinIdV1,
    "Stable identity of one locally queryable surface river basin."
);
typed_hash!(
    HydrologyOccupancyHashV1,
    "Canonical hash of the optional V6 hydrology occupancy config and frozen fluids."
);
typed_hash!(
    AquiferBasinIdV1,
    "Stable identity of one locally queryable underground aquifer basin."
);
typed_hash!(
    DrainageLinkIdV1,
    "Stable identity of one vertical surface-to-underground drainage column."
);
typed_hash!(
    CaveTopologyLayerHashV1,
    "Canonical hash of the optional V6 cave-topology realization layer."
);
typed_hash!(
    HydrologicDomainIdV1,
    "Stable identity of one finite hydrologic planning domain."
);
typed_hash!(
    HydrologicPortIdV1,
    "Direction-independent identity of one hydrologic domain boundary port."
);
typed_hash!(
    HydrologicDomainInputHashV1,
    "Canonical hash of every output-affecting hydrologic domain input."
);
typed_hash!(
    HydrologicDomainConfigHashV1,
    "Canonical hash of a bounded hydrologic domain configuration."
);
typed_hash!(
    HydrologicDomainPlanHashV1,
    "Canonical hash of one complete hydrologic domain plan."
);
typed_hash!(
    HydrologicBoundarySignatureV1,
    "Direction-independent signature of one hydrologic boundary result."
);
typed_hash!(
    HydrologicBasinIdV1,
    "Stable identity of one basin in a finite hydrologic-domain topology."
);
typed_hash!(
    HydrologicOutletIdV1,
    "Stable identity of a retained, ocean, or cross-domain hydrologic outlet."
);
typed_hash!(
    RiverSegmentIdV1,
    "Stable identity of one canonical single-receiver river segment."
);
typed_hash!(
    WaterBodyIdV1,
    "Stable identity of one ocean, lake, river, or wetland water body."
);
typed_hash!(
    HydrologicTopologyHashV1,
    "Canonical hash of a topology-first river and water-body plan."
);
typed_hash!(
    StaticReservoirHashV1,
    "Canonical hash of one sparse static-reservoir chunk candidate."
);
typed_hash!(
    SemanticTerrainPolicyHashV1,
    "Canonical hash of one closed semantic-terrain field and spline policy."
);
typed_hash!(
    SemanticTerrainPlanHashV1,
    "Canonical hash of one hydrology-constrained semantic terrain plan."
);
typed_hash!(
    TerrainBoundaryAdapterHashV1,
    "Direction-independent evidence hash for an old/new terrain boundary adapter."
);
typed_hash!(
    LandscapeEvolutionConfigHashV1,
    "Canonical hash of one bounded landscape-evolution configuration."
);
typed_hash!(
    LandscapeEvolutionPlanHashV1,
    "Canonical hash of one evolved hydrologic-domain and topology artifact."
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

/// Samples a fast deterministic two-dimensional field from a pre-derived seed.
pub(crate) fn sample_hash_2d(seed: u64, x: i64, z: i64) -> u64 {
    let x = u64::from_be_bytes(x.to_be_bytes());
    let z = u64::from_be_bytes(z.to_be_bytes());
    avalanche(
        seed ^ x.wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ z.wrapping_mul(0xc2b2_ae3d_27d4_eb4f).rotate_left(32),
    )
}

/// Samples a fast deterministic three-dimensional field from a pre-derived seed.
pub(crate) fn sample_hash_3d(seed: u64, x: i64, y: i64, z: i64) -> u64 {
    let x = u64::from_be_bytes(x.to_be_bytes());
    let y = u64::from_be_bytes(y.to_be_bytes());
    let z = u64::from_be_bytes(z.to_be_bytes());
    avalanche(
        seed ^ x.wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ y.wrapping_mul(0xbf58_476d_1ce4_e5b9).rotate_left(21)
            ^ z.wrapping_mul(0x94d0_49bb_1331_11eb).rotate_left(42),
    )
}

fn avalanche(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
