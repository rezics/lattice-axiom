//! Property tests for durable reopen, receipt binding, and chunk-read absence.

#![allow(
    clippy::expect_used,
    reason = "property fixtures use literals whose validity is a test invariant"
)]

use std::{collections::BTreeMap, str::FromStr, sync::Arc};

use latticeaxiom_core::{CanonicalHash, SchemaId, StableId, WorldId};
use latticeaxiom_storage::{
    ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey, ChunkMutation, ChunkRevision,
    ChunkRevisionExpectation, ContinuationId, DimensionId, PayloadSchemaVersion,
    PersistentEntityId, TransactionId, VersionedPayload, WorldRevision, WorldTransaction,
};
use latticeaxiom_world_catalog::SealedActivationBindingV1;
use latticeaxiom_world_wire::WorldWireLimits;
use proptest::prelude::*;

use crate::{
    ActivationPermitV1, AuthoritativeMetadataInputV1, CommitDurabilityV1,
    DeterministicHeaderPublisher, DeterministicWorldStorage, DigestV1, DisplayName,
    FrozenLockReceiptV1, HeaderPublisher, StoreId, WorldCommitRequestV1, WorldCreateRequestV1,
    WorldRequirementClosureV1, WorldStorage, WorldStorageLimitsV1, WriterActivationV1,
};
use latticeaxiom_world_catalog::{
    ReconciliationState, WorldOpenAction, WorldOpenPlan, WorldOpenRisk, WorldOpenStatus,
};

fn fixture_digest(label: &[u8]) -> DigestV1 {
    DigestV1::hash(b"latticeaxiom/world-db-property/v1", label)
}

fn fixture_metadata() -> AuthoritativeMetadataInputV1 {
    let lock = FrozenLockReceiptV1::new(
        br#"{"version":1,"packages":[]}"#.to_vec(),
        BTreeMap::new(),
        fixture_digest(b"registration"),
        fixture_digest(b"semantic"),
        fixture_digest(b"bundles"),
        fixture_digest(b"roles"),
        fixture_digest(b"settings"),
    )
    .expect("property frozen-lock metadata is valid");
    let closure = WorldRequirementClosureV1::new(
        BTreeMap::new(),
        BTreeMap::new(),
        std::collections::BTreeSet::new(),
        BTreeMap::new(),
        fixture_digest(b"bundle-receipts"),
        fixture_digest(b"role-bindings"),
        BTreeMap::new(),
    )
    .expect("property requirement closure is valid");
    AuthoritativeMetadataInputV1::new(lock, closure)
}

fn fixture_data(seed: u8) -> ChunkData {
    let schema = SchemaId::from_str("latticeaxiom:schema/chunk-voxels@1")
        .expect("property payload schema is canonical");
    let version =
        PayloadSchemaVersion::new(1).expect("property payload schema version is positive");
    let voxels = VersionedPayload::new(schema.clone(), version, vec![seed, seed ^ 0x5a]);
    let entities = BTreeMap::from([(
        PersistentEntityId::from_u128(u128::from(seed)),
        VersionedPayload::new(schema.clone(), version, vec![seed.wrapping_add(1)]),
    )]);
    let continuations = BTreeMap::from([(
        ContinuationId::from_u128(u128::from(seed)),
        VersionedPayload::new(schema, version, vec![seed.wrapping_add(2)]),
    )]);
    let provenance = BTreeMap::from([(
        StableId::from_str("latticeaxiom:provenance/worldgen@1")
            .expect("property provenance is canonical"),
        CanonicalHash::digest([seed]),
    )]);
    ChunkData::new(voxels, entities, continuations, provenance)
}

fn fixture_key(world: WorldId, coordinate: ChunkCoordinate) -> ChunkKey {
    ChunkKey::new(
        world,
        DimensionId::from_str("terrenia:dimension/terrenia")
            .expect("property dimension is canonical"),
        coordinate,
    )
}

fn sealed_activation(world: WorldId, permit: ActivationPermitV1) -> WriterActivationV1 {
    let binding = SealedActivationBindingV1 {
        store_id: permit.store_id().clone(),
        metadata_epoch: permit.metadata_epoch().get(),
        metadata_hash: CanonicalHash::from_bytes(*permit.metadata_hash().as_bytes()),
        projection_hash: CanonicalHash::from_bytes(*permit.projection_hash().as_bytes()),
        plan_generation: permit.metadata_epoch().get(),
    };
    let action = WorldOpenAction::UseFrozenLock;
    let plan = WorldOpenPlan {
        world_id: world,
        status: WorldOpenStatus::ReadyExact,
        risk: WorldOpenRisk::None,
        reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
        next_safe_step: Some(action.clone()),
        actions: vec![action.clone()],
        diagnostics: Vec::new(),
        activation_binding: Some(binding),
    };
    let accepted = plan
        .accept(action)
        .expect("property plan offers writable activation");
    WriterActivationV1::new(accepted, permit).expect("property accepted plan is writable")
}

fn durable_store() -> (
    DeterministicWorldStorage,
    WorldId,
    AuthoritativeMetadataInputV1,
) {
    let world = WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
        .expect("property world UUID is canonical");
    let publisher: Arc<dyn HeaderPublisher> = Arc::new(DeterministicHeaderPublisher::new());
    let storage = DeterministicWorldStorage::durable(
        StableId::from_str("latticeaxiom:schema/world-db-chunk@1")
            .expect("property record owner is canonical"),
        WorldWireLimits::default(),
        WorldStorageLimitsV1::D3_BOOTSTRAP,
        publisher,
    );
    (storage, world, fixture_metadata())
}

fn provision_ready(
    storage: &DeterministicWorldStorage,
    world: WorldId,
    metadata: &AuthoritativeMetadataInputV1,
) -> ActivationPermitV1 {
    storage
        .provision_world(WorldCreateRequestV1::new(
            world,
            DisplayName::new("Property World").expect("property display name is valid"),
            StoreId::new("store-generation-1").expect("property store ID is valid"),
            metadata.clone(),
        ))
        .expect("property store provisions");
    storage
        .preflight(world)
        .expect("property preflight succeeds")
        .activation_permit()
        .expect("ready property store carries a permit")
        .clone()
}

proptest! {
    #[test]
    fn durable_reopen_preserves_materialized_chunk_and_leaves_absent_unmaterialized(
        x in -8_i32..8,
        y in -4_i32..4,
        z in -8_i32..8,
        seed in 1_u8..200,
    ) {
        let (storage, world, metadata) = durable_store();
        let permit = provision_ready(&storage, world, &metadata);
        let present = fixture_key(world, ChunkCoordinate::new(x, y, z));
        let absent = fixture_key(world, ChunkCoordinate::new(x.saturating_add(1), y, z));
        let mut writer = storage
            .activate_writer(sealed_activation(world, permit))
            .expect("property durable writer activates");
        let request = WorldCommitRequestV1::new(
            WorldTransaction::new(
                TransactionId::from_u128(7),
                world,
                WorldRevision::ZERO,
                vec![ChunkMutation::new(
                    present.clone(),
                    ChunkRevisionExpectation::Absent,
                    ChangedDomains::ALL,
                    fixture_data(seed),
                )],
            ),
            metadata,
            CommitDurabilityV1::Durable,
        );
        writer
            .commit(request)
            .expect("property durable commit publishes");
        writer.close().expect("property durable writer closes");

        let publisher: Arc<dyn HeaderPublisher> = Arc::new(DeterministicHeaderPublisher::new());
        let recovered = storage
            .canonical_reopen(publisher)
            .expect("property canonical reopen succeeds");
        recovered
            .verify_crash_recovery(world)
            .expect("property crash recovery verifies");
        let view = recovered
            .begin_read(world)
            .expect("property recovered world is readable");
        let loaded = view
            .load_chunk(&present)
            .expect("property materialized chunk decodes")
            .expect("durable chunk must be loaded before generation");
        prop_assert_eq!(loaded.chunk_revision(), ChunkRevision::new(1));
        prop_assert_eq!(loaded.data(), &fixture_data(seed));
        prop_assert!(
            view.load_chunk(&absent)
                .expect("property absent chunk remains decodable")
                .is_none()
        );
    }
}
