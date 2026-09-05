//! Host-level sealed writer activation: receipts fail closed; reopen restores.
#![allow(clippy::expect_used)]

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use latticeaxiom_core::{CanonicalHash, SchemaId, StableId, WorldId};
use latticeaxiom_engine::{
    SealedWorldWriterHost, SealedWriterHostError, sealed_activation_binding,
};
use latticeaxiom_storage::{
    ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey, ChunkMutation, ChunkRevision,
    ChunkRevisionExpectation, ContinuationId, DimensionId, PayloadSchemaVersion,
    PersistentEntityId, TransactionId, VersionedPayload, WorldRevision, WorldTransaction,
};
use latticeaxiom_world_catalog::{
    ReconciliationState, SealedActivationBindingV1, WorldOpenAction, WorldOpenPlan, WorldOpenRisk,
    WorldOpenStatus,
};
use latticeaxiom_world_db::{
    ActivationPermitV1, AuthoritativeMetadataInputV1, CheckpointId, CheckpointKindV1,
    CheckpointRequestV1, CommitDurabilityV1, DisplayName, FrozenLockReceiptV1,
    StorageDurabilityCapabilityV1, StoreId, WorldCommitRequestV1, WorldCreateRequestV1,
    WorldDbError, WorldRequirementClosureV1, WriterActivationV1,
};

fn fixture_host() -> (SealedWorldWriterHost, WorldId, AuthoritativeMetadataInputV1) {
    let world = WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
        .expect("fixture world UUID is canonical");
    let record_owner = StableId::from_str("latticeaxiom:schema/world-db-chunk@1")
        .expect("fixture record owner is canonical");
    (
        SealedWorldWriterHost::volatile_reference_with_default_publisher(record_owner),
        world,
        fixture_metadata(),
    )
}

fn fixture_durable_host() -> (SealedWorldWriterHost, WorldId, AuthoritativeMetadataInputV1) {
    let world = WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
        .expect("fixture world UUID is canonical");
    let record_owner = StableId::from_str("latticeaxiom:schema/world-db-chunk@1")
        .expect("fixture record owner is canonical");
    (
        SealedWorldWriterHost::durable_reference_with_default_publisher(record_owner),
        world,
        fixture_metadata(),
    )
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
    .expect("fixture frozen-lock metadata is valid");
    let closure = WorldRequirementClosureV1::new(
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeSet::new(),
        BTreeMap::new(),
        fixture_digest(b"bundle-receipts"),
        fixture_digest(b"role-bindings"),
        BTreeMap::new(),
    )
    .expect("fixture requirement closure is valid");
    AuthoritativeMetadataInputV1::new(lock, closure)
}

fn fixture_digest(label: &[u8]) -> latticeaxiom_world_db::DigestV1 {
    latticeaxiom_world_db::DigestV1::hash(b"latticeaxiom/world-db-test/v1", label)
}

fn fixture_key(world: WorldId) -> ChunkKey {
    ChunkKey::new(
        world,
        DimensionId::from_str("terrenia:dimension/terrenia")
            .expect("fixture dimension is canonical"),
        ChunkCoordinate::new(1, -2, 3),
    )
}

fn fixture_data(seed: u8) -> ChunkData {
    let schema = SchemaId::from_str("latticeaxiom:schema/chunk-voxels@1")
        .expect("fixture payload schema is canonical");
    let version = PayloadSchemaVersion::new(1).expect("fixture payload schema version is positive");
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
            .expect("fixture provenance is canonical"),
        CanonicalHash::digest([seed]),
    )]);
    ChunkData::new(voxels, entities, continuations, provenance)
}

fn fixture_written_commit(
    world: WorldId,
    metadata: &AuthoritativeMetadataInputV1,
) -> WorldCommitRequestV1 {
    WorldCommitRequestV1::new(
        WorldTransaction::new(
            TransactionId::from_u128(7),
            world,
            WorldRevision::ZERO,
            vec![ChunkMutation::new(
                fixture_key(world),
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                fixture_data(11),
            )],
        ),
        metadata.clone(),
        CommitDurabilityV1::Written,
    )
}

fn ready_exact_plan(
    world: WorldId,
    activation_binding: Option<SealedActivationBindingV1>,
) -> WorldOpenPlan {
    let action = WorldOpenAction::UseFrozenLock;
    WorldOpenPlan {
        world_id: world,
        status: WorldOpenStatus::ReadyExact,
        risk: WorldOpenRisk::None,
        reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
        next_safe_step: Some(action.clone()),
        actions: vec![action],
        diagnostics: Vec::new(),
        activation_binding,
    }
}

fn provision_ready(
    host: &SealedWorldWriterHost,
    world: WorldId,
    metadata: &AuthoritativeMetadataInputV1,
) -> ActivationPermitV1 {
    host.provision_world(WorldCreateRequestV1::new(
        world,
        DisplayName::new("Deterministic World").expect("fixture display name is valid"),
        StoreId::new("store-generation-1").expect("fixture store ID is valid"),
        metadata.clone(),
    ))
    .expect("fresh host store provisions");
    host.preflight(world)
        .expect("published header cross-checks authoritative metadata")
        .activation_permit()
        .expect("ready preflight carries one activation permit")
        .clone()
}

#[test]
fn missing_receipt_fails_closed_with_activation_evidence_unavailable() {
    let (mut host, world, metadata) = fixture_host();
    let permit = provision_ready(&host, world, &metadata);
    let plan = ready_exact_plan(world, None);

    assert!(matches!(
        host.accept(&plan, WorldOpenAction::UseFrozenLock),
        Err(SealedWriterHostError::WorldDb(
            WorldDbError::ActivationEvidenceUnavailable { world: found }
        )) if found == world
    ));

    let accepted = plan
        .accept(WorldOpenAction::UseFrozenLock)
        .expect("catalog accept still yields a writable plan without a receipt");
    assert!(accepted.activation_receipt().is_none());
    let activation = WriterActivationV1::new(accepted, permit)
        .expect("writable accept binds to a storage permit");
    assert!(matches!(
        host.activate_writer(activation),
        Err(SealedWriterHostError::WorldDb(
            WorldDbError::ActivationEvidenceUnavailable { world: found }
        )) if found == world
    ));
}

#[test]
fn stale_and_mismatched_receipts_fail_closed() {
    let (mut host, world, metadata) = fixture_host();
    let permit = provision_ready(&host, world, &metadata);
    let plan = ready_exact_plan(world, Some(sealed_activation_binding(&permit)));
    let accepted = host
        .accept(&plan, WorldOpenAction::UseFrozenLock)
        .expect("frozen-lock accept requires bound catalog evidence");
    let activation =
        WriterActivationV1::new(accepted, permit.clone()).expect("sealed accept is writable");
    host.activate_writer(activation.clone())
        .expect("sealed writer activation succeeds");
    host.close().expect("sealed writer closes");

    assert!(matches!(
        host.activate_writer(activation),
        Err(SealedWriterHostError::WorldDb(
            WorldDbError::ActivationPermitStale { world: found, .. }
        )) if found == world
    ));

    let reopened = host
        .preflight(world)
        .expect("closing a writer leaves preflight evidence")
        .activation_permit()
        .expect("ready store retains activation evidence")
        .clone();
    let stale_plan = ready_exact_plan(world, Some(sealed_activation_binding(&permit)));
    let stale_accepted = stale_plan
        .accept(WorldOpenAction::UseFrozenLock)
        .expect("catalog still seals a receipt from stale binding");
    let stale_activation = WriterActivationV1::new(stale_accepted, reopened.clone())
        .expect("writable accept binds to a fresh permit");
    assert!(
        matches!(
            host.activate_writer(stale_activation),
            Err(SealedWriterHostError::WorldDb(
                WorldDbError::ActivationEvidenceUnavailable { world: found }
                    | WorldDbError::ActivationPermitInvalid { world: found, .. }
                    | WorldDbError::ActivationPermitHashMismatch { world: found, .. }
                    | WorldDbError::ActivationPermitStale { world: found, .. }
            )) if found == world
        ),
        "stale sealed receipts must fail closed against a later permit"
    );

    let mut mismatched = sealed_activation_binding(&reopened);
    mismatched.metadata_hash = CanonicalHash::digest(b"forged-metadata");
    let mismatched_plan = ready_exact_plan(world, Some(mismatched));
    let mismatched_accepted = host
        .accept(&mismatched_plan, WorldOpenAction::UseFrozenLock)
        .expect("host accept seals whatever binding the plan carries");
    let mismatched_activation = WriterActivationV1::new(mismatched_accepted, reopened)
        .expect("writable accept binds to a storage permit");
    assert!(matches!(
        host.activate_writer(mismatched_activation),
        Err(SealedWriterHostError::WorldDb(
            WorldDbError::ActivationPermitHashMismatch { world: found, .. }
        )) if found == world
    ));
}

#[test]
fn sealed_commit_then_close_is_visible_to_begin_read_and_can_reactivate() {
    let (mut host, world, metadata) = fixture_host();
    let permit = provision_ready(&host, world, &metadata);
    let plan = ready_exact_plan(world, Some(sealed_activation_binding(&permit)));
    let accepted = host
        .accept(&plan, WorldOpenAction::UseFrozenLock)
        .expect("frozen-lock accept requires bound catalog evidence");
    host.activate_writer(
        WriterActivationV1::new(accepted, permit).expect("sealed accept is writable"),
    )
    .expect("sealed writer activation succeeds");
    host.commit(fixture_written_commit(world, &metadata))
        .expect("sealed writer commits a Written chunk mutation");
    assert!(matches!(
        host.flush_durable(),
        Err(SealedWriterHostError::WorldDb(
            WorldDbError::PhysicalDurabilityUnsupported { .. }
        ))
    ));
    host.close().expect("sealed writer closes");

    let loaded = host
        .begin_read(world)
        .expect("closed world remains readable")
        .load_chunk(&fixture_key(world))
        .expect("portable record decodes")
        .expect("committed chunk exists");
    assert_eq!(loaded.chunk_revision(), ChunkRevision::new(1));

    let reopened = host
        .preflight(world)
        .expect("closing a writer leaves preflight evidence")
        .activation_permit()
        .expect("ready store retains activation evidence")
        .clone();
    let reopen_plan = ready_exact_plan(world, Some(sealed_activation_binding(&reopened)));
    host.reactivate(&reopen_plan, reopened)
        .expect("sealed reactivation succeeds after close");
    host.close().expect("reactivated writer closes");
}

fn fixture_durable_commit(
    world: WorldId,
    metadata: &AuthoritativeMetadataInputV1,
) -> WorldCommitRequestV1 {
    WorldCommitRequestV1::new(
        WorldTransaction::new(
            TransactionId::from_u128(9),
            world,
            WorldRevision::ZERO,
            vec![ChunkMutation::new(
                fixture_key(world),
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                fixture_data(11),
            )],
        ),
        metadata.clone(),
        CommitDurabilityV1::Durable,
    )
}

#[test]
fn durable_sealed_commit_checkpoint_and_crash_reopen_restores_materialized_chunk() {
    let (mut host, world, metadata) = fixture_durable_host();
    assert_eq!(
        host.durability_capability(),
        StorageDurabilityCapabilityV1::WalSyncCheckpoint
    );
    let permit = provision_ready(&host, world, &metadata);
    let stale = permit.clone();
    let plan = ready_exact_plan(world, Some(sealed_activation_binding(&permit)));
    let accepted = host
        .accept(&plan, WorldOpenAction::UseFrozenLock)
        .expect("frozen-lock accept requires bound catalog evidence");
    host.activate_writer(
        WriterActivationV1::new(accepted, permit).expect("sealed accept is writable"),
    )
    .expect("sealed durable writer activation succeeds");
    let conflict_plan = ready_exact_plan(world, Some(sealed_activation_binding(&stale)));
    let conflict_accepted = conflict_plan
        .accept(WorldOpenAction::UseFrozenLock)
        .expect("catalog accept is still offered");
    let conflict = WriterActivationV1::new(conflict_accepted, stale)
        .expect("writable accept binds to a storage permit");
    assert!(
        matches!(
            host.activate_writer(conflict),
            Err(SealedWriterHostError::WorldDb(
                WorldDbError::WriterAlreadyActive { world: found }
            )) if found == world
        ),
        "lease conflict must reject a second writer"
    );
    host.commit(fixture_durable_commit(world, &metadata))
        .expect("sealed durable writer commits a Durable chunk mutation");
    let checkpoint = host
        .create_checkpoint(CheckpointRequestV1::new(
            CheckpointId::from_u128(1),
            CheckpointKindV1::Protected,
            "save-and-quit",
        ))
        .expect("checkpoint at the durable frontier is retained");
    assert!(checkpoint.receipt().restore_verified());

    host.canonical_reopen()
        .expect("canonical reopen drops the lease and restores the durable image");
    assert!(!host.is_writer_active());
    let preflight = host
        .preflight(world)
        .expect("reopened durable metadata remains readable");
    if let Some(repair) = preflight.header_repair_permit().cloned() {
        host.repair_header(repair)
            .expect("sidecar repair is a read-only recovery action");
    }
    host.verify_crash_recovery(world)
        .expect("unclean durable frontier verifies without a writer");
    let restored = host
        .begin_read(world)
        .expect("reopened durable world remains readable")
        .load_chunk(&fixture_key(world))
        .expect("portable record decodes")
        .expect("durable materialized chunk survived reopen");
    assert_eq!(restored.chunk_revision(), ChunkRevision::new(1));
    assert_eq!(restored.data(), &fixture_data(11));
    assert!(
        host.preflight(world)
            .expect("post-recovery preflight remains readable")
            .activation_permit()
            .is_some(),
        "verified recovery may later open a sealed writer"
    );
}
