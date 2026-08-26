//! D4 world-generation conformance, determinism, and bounded-failure corpus.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test fixtures fail immediately when authored IDs or invariants are invalid"
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU16, NonZeroU32, NonZeroU64},
};

use latticeaxiom_core::{CanonicalHash, StableId, WorldId};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChunkKey, CommitReceipt, MemoryTransactionKernel,
    TransactionId, WorldRevision,
};
use latticeaxiom_worldgen::{
    AdjacentCellEpochV1, AdjacentEpochSnapshotV1, BoundaryAdapterDeclarationV1,
    BoundedGeneratedRegionV1, CellEpochStateV1, ChunkCoordinate, ChunkFaceV1,
    ChunkGenerationOutcomeV1, ChunkGenerationRequestV1, ChunkRevision, ChunkRevisionExpectation,
    D4BlockCatalogClosureV1, D4MaterialRoleV1, D4RoleVocabularyV1, DimensionId,
    ExistingSnapshotEvidenceV1, FrozenRoleBindingsV1, GenerationEpochIdV1, GenerationPlanInputV1,
    GenerationPlanV1, MAX_BOUNDED_REGION_CHUNKS, ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1,
    PlacementPredicateKindV1, PlanActivationIdV1, PlanningCellCoordinateV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, TerrainStyleV1, WorldSeedV1,
    WorldgenConfigV1, WorldgenError, WorldgenLimitsV1,
};
use proptest::prelude::*;

type GeneratedEvidence = (Vec<u8>, Vec<u8>, Vec<u8>);
const CATALOG_PATHS: [&str; 18] = [
    "air",
    "grass",
    "dirt",
    "stone",
    "obsidian",
    "copper-block",
    "clay",
    "sand",
    "red-sand",
    "gravel",
    "limestone",
    "basalt",
    "sandstone",
    "red-sandstone",
    "copper-ore",
    "oak-log",
    "oak-leaves",
    "tall-grass",
];

const ROLE_TARGETS: [(D4MaterialRoleV1, &str); 16] = [
    (D4MaterialRoleV1::Empty, "air"),
    (D4MaterialRoleV1::TemperateSurface, "grass"),
    (D4MaterialRoleV1::TemperateSubsurface, "dirt"),
    (D4MaterialRoleV1::TemperateBaseRock, "stone"),
    (D4MaterialRoleV1::TemperateSecondaryRock, "limestone"),
    (D4MaterialRoleV1::TemperateClay, "clay"),
    (D4MaterialRoleV1::TemperateGravel, "gravel"),
    (D4MaterialRoleV1::WoodlandLog, "oak-log"),
    (D4MaterialRoleV1::WoodlandLeaves, "oak-leaves"),
    (D4MaterialRoleV1::WoodlandGroundCover, "tall-grass"),
    (D4MaterialRoleV1::AridSand, "sand"),
    (D4MaterialRoleV1::AridRedSand, "red-sand"),
    (D4MaterialRoleV1::AridSandstone, "sandstone"),
    (D4MaterialRoleV1::AridRedSandstone, "red-sandstone"),
    (D4MaterialRoleV1::AridBaseRock, "basalt"),
    (D4MaterialRoleV1::CopperResource, "copper-ore"),
];

#[test]
fn exact_18_block_external_catalog_closes_all_material_roles() {
    let catalog = block_catalog();
    assert_eq!(catalog.blocks().len(), 18);
    let plan = fixture_plan(false, &[b"lock-a"]);
    let concrete = plan
        .generate(vacant_request(ChunkCoordinate::new(0, 0, 0)))
        .expect("valid fixture generation should succeed");
    let prepared = prepared(&concrete);
    assert_eq!(prepared.receipt().role_bindings().len(), 16);
    for receipt in prepared.receipt().role_bindings() {
        assert!(catalog.contains(receipt.block_id()));
        assert_eq!(receipt.role_id().kind(), "block-role");
        assert_eq!(receipt.block_id().kind(), "block");
    }
    let predicates = prepared.receipt().placement_predicates();
    assert_eq!(predicates.len(), 4);
    assert_eq!(
        predicates
            .iter()
            .map(latticeaxiom_worldgen::PlacementPredicateReceiptV1::predicate)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            PlacementPredicateKindV1::CaveOccupancy,
            PlacementPredicateKindV1::TreeAnchor,
            PlacementPredicateKindV1::GroundCover,
            PlacementPredicateKindV1::CopperResource,
        ])
    );
    for predicate in predicates {
        assert!(predicate.accepted() <= predicate.evaluations());
        assert!(!predicate.placement_roles().is_empty());
    }
}

#[test]
fn catalog_boundary_and_missing_role_fail_closed() {
    let only_seventeen: Vec<_> = CATALOG_PATHS[..17]
        .iter()
        .map(|path| block_id(path))
        .collect();
    assert!(matches!(
        D4BlockCatalogClosureV1::new(only_seventeen),
        Err(WorldgenError::IncompleteCatalogClosure {
            minimum: 18,
            actual: 17
        })
    ));

    let mut entries = role_entries();
    let missing_role = entries.pop().expect("fixture has roles").0;
    let input = fixture_input(
        default_config(),
        provider_offers(false),
        D4RoleVocabularyV1::new(role_vocabulary_entries()).expect("valid role vocabulary"),
        FrozenRoleBindingsV1::new(entries).expect("remaining bindings are valid"),
        block_catalog(),
        vec![CanonicalHash::digest(b"lock-a")],
        WorldgenLimitsV1::default(),
    );
    assert!(matches!(
        GenerationPlanV1::compile(input),
        Err(WorldgenError::MissingRoleBinding { role }) if role == missing_role
    ));
}

#[test]
fn provider_restart_and_chunk_permutations_preserve_exact_bytes() {
    let forward = fixture_plan(false, &[b"lock-a"]);
    let reverse = fixture_plan(true, &[b"lock-a"]);
    assert_eq!(
        forward.generation_input_hash(),
        reverse.generation_input_hash()
    );
    assert_eq!(
        forward.generator_fingerprint(),
        reverse.generator_fingerprint()
    );

    let chunks = [
        ChunkCoordinate::new(-2, 0, 3),
        ChunkCoordinate::new(0, 2, 0),
        ChunkCoordinate::new(5, -1, -4),
    ];
    let first = generated_by_coordinate(&forward, chunks);
    let second = generated_by_coordinate(&reverse, chunks.into_iter().rev());
    assert_eq!(first, second);
}

#[test]
fn exclusive_provider_missing_and_conflict_fail_before_generation() {
    let mut missing = provider_offers(false);
    missing.retain(|offer| offer.slot() != ProviderSlotV1::CaveTopology);
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input_with_defaults(missing)),
        Err(WorldgenError::MissingProvider {
            slot: ProviderSlotV1::CaveTopology
        })
    ));

    let mut conflict = provider_offers(false);
    let duplicate = conflict
        .iter()
        .find(|offer| offer.slot() == ProviderSlotV1::TemperateTerrain)
        .expect("fixture contains temperate provider")
        .clone();
    conflict.push(duplicate);
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input_with_defaults(conflict)),
        Err(WorldgenError::ConflictingProviders {
            slot: ProviderSlotV1::TemperateTerrain,
            ..
        })
    ));
}

#[test]
fn same_provider_revision_cannot_claim_two_fingerprints() {
    let mut offers = provider_offers(false);
    let shared_id = stable_id("fixture:worldgen-provider/shared@1");
    offers[0] = ProviderOfferV1::new(
        ProviderSlotV1::GenerationCoordinator,
        ProviderGenerationIdentityV1::new(
            shared_id.clone(),
            NonZeroU32::MIN,
            7,
            CanonicalHash::digest(b"implementation-a"),
        ),
    );
    offers[1] = ProviderOfferV1::new(
        ProviderSlotV1::StyleSelector,
        ProviderGenerationIdentityV1::new(
            shared_id.clone(),
            NonZeroU32::MIN,
            7,
            CanonicalHash::digest(b"implementation-b"),
        ),
    );
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input_with_defaults(offers)),
        Err(WorldgenError::ProviderFingerprintConflict { provider, .. }) if provider == shared_id
    ));
}

#[test]
fn lock_provenance_does_not_become_prng_salt() {
    let first = fixture_plan(false, &[b"artifact-a"]);
    let second = fixture_plan(false, &[b"artifact-b"]);
    assert_eq!(
        first.generation_input_hash(),
        second.generation_input_hash()
    );
    assert_ne!(
        first.generation_provenance_hash(),
        second.generation_provenance_hash()
    );
    let coordinate = ChunkCoordinate::new(0, 1, 0);
    let first_outcome = first
        .generate(vacant_request(coordinate))
        .expect("first generation succeeds");
    let second_outcome = second
        .generate(vacant_request(coordinate))
        .expect("second generation succeeds");
    let first_candidate = prepared(&first_outcome);
    let second_candidate = prepared(&second_outcome);
    assert_eq!(first_candidate.draft(), second_candidate.draft());
    assert_eq!(
        first_candidate.snapshot_bytes(),
        second_candidate.snapshot_bytes()
    );
    assert_eq!(first_candidate.checksum(), second_candidate.checksum());
    assert_ne!(
        first_candidate.receipt().canonical_bytes().ok(),
        second_candidate.receipt().canonical_bytes().ok()
    );
}

#[test]
fn old_materialized_epoch_is_returned_without_recomputation() {
    let old_plan = fixture_plan(false, &[b"lock-a"]);
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let old_outcome = old_plan
        .generate(vacant_request(coordinate))
        .expect("old plan generates fixture");
    let old_candidate = prepared(&old_outcome);
    let existing = ExistingSnapshotEvidenceV1::new(
        old_plan.dimension().clone(),
        coordinate,
        PlanningCellCoordinateV1::new(0, 0),
        old_plan.generation_epoch(),
        ChunkRevision::new(41),
        3,
        old_candidate.snapshot_bytes().to_vec(),
        old_candidate.checksum(),
    )
    .expect("candidate bytes match checksum");

    let mut offers = provider_offers(false);
    let changed = ProviderGenerationIdentityV1::new(
        stable_id("fixture:worldgen-provider/temperate@1"),
        NonZeroU32::MIN,
        8,
        CanonicalHash::digest(b"temperate-implementation-v8"),
    );
    let position = offers
        .iter()
        .position(|offer| offer.slot() == ProviderSlotV1::TemperateTerrain)
        .expect("fixture contains temperate provider");
    offers[position] = ProviderOfferV1::new(ProviderSlotV1::TemperateTerrain, changed);
    let new_plan = GenerationPlanV1::compile(fixture_input_with_defaults(offers))
        .expect("updated provider plan compiles");
    assert_ne!(old_plan.generation_epoch(), new_plan.generation_epoch());

    assert!(matches!(
        new_plan.generate(ChunkGenerationRequestV1::new(
            coordinate,
            Some(existing.clone()),
            CellEpochStateV1::Unassigned,
            unassigned_adjacency(coordinate),
            Vec::new(),
        )),
        Err(WorldgenError::SnapshotEpochStateMismatch { .. })
    ));

    let outcome = new_plan
        .generate(ChunkGenerationRequestV1::new(
            coordinate,
            Some(existing.clone()),
            CellEpochStateV1::Frozen(old_plan.generation_epoch()),
            unassigned_adjacency(coordinate),
            Vec::new(),
        ))
        .expect("existing durable snapshot wins before epoch generation");
    assert_eq!(
        outcome,
        ChunkGenerationOutcomeV1::Existing(existing.clone())
    );
    let ChunkGenerationOutcomeV1::Existing(returned) = outcome else {
        panic!("expected existing snapshot")
    };
    assert_eq!(returned.bytes(), existing.bytes());
    assert_eq!(returned.revision(), ChunkRevision::new(41));
    assert_eq!(returned.writer_count(), 3);
    assert_eq!(returned.checksum(), existing.checksum());
}

#[test]
fn snapshot_candidate_carries_activation_and_storage_cas_without_changing_bytes() {
    let first = fixture_plan_with_activation(b"activation-a");
    let second = fixture_plan_with_activation(b"activation-b");
    assert_eq!(
        first.generation_input_hash(),
        second.generation_input_hash()
    );
    let coordinate = ChunkCoordinate::new(1, -1, 2);
    let expected_revision = ChunkRevisionExpectation::Exact(ChunkRevision::new(9));
    let first_outcome = first
        .generate(vacant_request(coordinate).with_expected_chunk_revision(expected_revision))
        .expect("revision-aware candidate generates");
    let second_outcome = second
        .generate(vacant_request(coordinate))
        .expect("new activation candidate generates");
    let first_candidate = prepared(&first_outcome);
    let second_candidate = prepared(&second_outcome);
    assert_eq!(first_candidate.expected_chunk_revision(), expected_revision);
    assert_eq!(
        first_candidate.plan_activation_id(),
        first.plan_activation_id()
    );
    assert_ne!(
        first_candidate.plan_activation_id(),
        second_candidate.plan_activation_id()
    );
    assert_eq!(
        first_candidate.snapshot_bytes(),
        second_candidate.snapshot_bytes()
    );
    assert_eq!(first_candidate.checksum(), second_candidate.checksum());
    assert_eq!(
        first_candidate
            .receipt()
            .canonical_bytes()
            .expect("first receipt is canonical"),
        second_candidate
            .receipt()
            .canonical_bytes()
            .expect("second receipt is canonical")
    );
}
#[test]
fn epoch_boundary_requires_complete_state_and_verified_application() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let old_epoch = GenerationEpochIdV1::from_hash(CanonicalHash::digest(b"old-epoch"));
    let cell = PlanningCellCoordinateV1::new(0, 0);
    let adjacent = AdjacentEpochSnapshotV1::new(
        cell,
        [
            AdjacentCellEpochV1::frozen(PlanningCellCoordinateV1::new(1, 0), old_epoch),
            AdjacentCellEpochV1::unassigned(PlanningCellCoordinateV1::new(-1, 0)),
            AdjacentCellEpochV1::unassigned(PlanningCellCoordinateV1::new(0, 1)),
            AdjacentCellEpochV1::outside_dimension(PlanningCellCoordinateV1::new(0, -1)),
        ],
    )
    .expect("all cardinal states are explicit");
    let request = ChunkGenerationRequestV1::new(
        ChunkCoordinate::new(0, 0, 0),
        None,
        CellEpochStateV1::Unassigned,
        adjacent.clone(),
        Vec::new(),
    );
    assert!(matches!(
        plan.generate(request),
        Err(WorldgenError::MissingBoundaryAdapter { .. })
    ));

    let declaration = BoundaryAdapterDeclarationV1::new(
        stable_id("fixture:worldgen-boundary-adapter/old-new@1"),
        NonZeroU32::MIN,
        CanonicalHash::digest(b"adapter-artifact"),
        old_epoch,
        plan.generation_epoch(),
        NonZeroU32::new(8).unwrap_or(NonZeroU32::MIN),
        CanonicalHash::digest(b"terrain-signature"),
        vec![CanonicalHash::digest(b"portal")],
    )
    .expect("bounded declaration is valid");
    assert!(matches!(
        plan.generate(ChunkGenerationRequestV1::new(
            ChunkCoordinate::new(0, 0, 0),
            None,
            CellEpochStateV1::Unassigned,
            adjacent,
            vec![declaration],
        )),
        Err(WorldgenError::BoundaryAdapterNotVerified { .. })
    ));

    let duplicate = AdjacentEpochSnapshotV1::new(
        cell,
        [
            AdjacentCellEpochV1::unassigned(PlanningCellCoordinateV1::new(1, 0)),
            AdjacentCellEpochV1::unassigned(PlanningCellCoordinateV1::new(1, 0)),
            AdjacentCellEpochV1::unassigned(PlanningCellCoordinateV1::new(0, 1)),
            AdjacentCellEpochV1::unassigned(PlanningCellCoordinateV1::new(0, -1)),
        ],
    );
    assert!(matches!(
        duplicate,
        Err(WorldgenError::DuplicateAdjacentEpochCell { .. })
    ));
}

#[test]
fn frozen_unmaterialized_cell_requires_archived_plan() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let old_epoch = GenerationEpochIdV1::from_hash(CanonicalHash::digest(b"old-epoch"));
    let result = plan.generate(ChunkGenerationRequestV1::new(
        ChunkCoordinate::new(0, 0, 0),
        None,
        CellEpochStateV1::Frozen(old_epoch),
        unassigned_adjacency(ChunkCoordinate::new(0, 0, 0)),
        Vec::new(),
    ));
    assert!(matches!(
        result,
        Err(WorldgenError::FrozenEpochUnavailable { frozen, .. }) if frozen == old_epoch
    ));
}

#[test]
fn shared_cave_portal_key_contract_and_field_match_from_both_sides() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let left_key = plan
        .shared_face_key(ChunkCoordinate::new(3, -2, 4), ChunkFaceV1::PositiveX)
        .expect("adjacent coordinate is representable");
    let right_key = plan
        .shared_face_key(ChunkCoordinate::new(4, -2, 4), ChunkFaceV1::NegativeX)
        .expect("adjacent coordinate is representable");
    assert_eq!(left_key, right_key);
    assert!(matches!(
        plan.shared_face_key(ChunkCoordinate::new(i32::MAX, 0, 0), ChunkFaceV1::PositiveX),
        Err(WorldgenError::ArithmeticOverflow { .. })
    ));

    let (chunk, left_receipt) = (-16..=16)
        .flat_map(|z| (-16..=16).map(move |x| ChunkCoordinate::new(x, -2, z)))
        .find_map(|chunk| {
            plan.cave_face_field_requests(chunk)
                .ok()?
                .into_iter()
                .find(|receipt| {
                    receipt.face() == ChunkFaceV1::PositiveX && receipt.portal_requested()
                })
                .map(|receipt| (chunk, receipt))
        })
        .expect("fixed corpus contains a requested positive-X portal");
    let neighbor = ChunkCoordinate::new(chunk.x + 1, chunk.y, chunk.z);
    let right_receipt = plan
        .cave_face_field_requests(neighbor)
        .expect("neighbor receipts are representable")
        .into_iter()
        .find(|receipt| receipt.face() == ChunkFaceV1::NegativeX)
        .expect("all six face receipts exist");
    assert_eq!(left_receipt.key(), right_receipt.key());
    assert_eq!(
        left_receipt.portal_u_voxel(),
        right_receipt.portal_u_voxel()
    );
    assert_eq!(
        left_receipt.portal_v_voxel(),
        right_receipt.portal_v_voxel()
    );
    assert_eq!(
        left_receipt.clearance_radius_voxels(),
        right_receipt.clearance_radius_voxels()
    );
    assert!(right_receipt.portal_requested());

    let edge = i64::from(default_config().chunk_edge_voxels);
    let y = i64::from(chunk.y)
        .saturating_mul(edge)
        .saturating_add(i64::from(left_receipt.portal_u_voxel()));
    let z = i64::from(chunk.z)
        .saturating_mul(edge)
        .saturating_add(i64::from(left_receipt.portal_v_voxel()));
    let left_x = i64::from(chunk.x)
        .saturating_mul(edge)
        .saturating_add(edge - 1);
    let right_x = i64::from(neighbor.x).saturating_mul(edge);
    assert!(plan.cave_signed_distance_fixed(left_x, y, z) <= 0);
    assert!(plan.cave_signed_distance_fixed(right_x, y, z) <= 0);
    assert_eq!(plan.terrain_density(left_x, y, z), -1);
    assert_eq!(plan.terrain_density(right_x, y, z), -1);

    let left_outcome = plan
        .generate(vacant_request(chunk))
        .expect("deep left portal chunk generates");
    let left_validation = prepared(&left_outcome)
        .receipt()
        .cave_occupancy_validations()
        .iter()
        .copied()
        .find(|validation| validation.request().face() == ChunkFaceV1::PositiveX)
        .expect("all six final occupancy validations exist");
    assert_eq!(left_validation.request(), left_receipt);
    assert_eq!(
        left_validation.field_void_samples(),
        left_validation.aperture_samples()
    );
    assert!(left_validation.is_finally_open());
}

#[test]
fn cave_field_portal_plan_is_identical_under_shuffled_chunk_order() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let mut chunks = Vec::new();
    for z in -4..=4 {
        for y in -3..=-1 {
            for x in -4..=4 {
                chunks.push(ChunkCoordinate::new(x, y, z));
            }
        }
    }
    let forward = plan
        .cave_field_portal_plan(chunks.clone())
        .expect("forward field portal plan is representable");
    let reversed = plan
        .cave_field_portal_plan(chunks.iter().copied().rev())
        .expect("reversed field portal plan is representable");
    let mut rotated = chunks;
    let rotate_by = rotated.len() / 3;
    rotated.rotate_left(rotate_by);
    let rotated = plan
        .cave_field_portal_plan(rotated)
        .expect("rotated field portal plan is representable");
    assert_eq!(forward, reversed);
    assert_eq!(forward, rotated);
    let assertions = forward.assertions();
    assert!(!assertions.is_empty());
    for assertion in assertions {
        assert!(assertion.clearance_radius_voxels() > 0);
    }
}

#[test]
fn all_six_face_field_requests_validate_both_sides_against_final_occupancy() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    for face in ChunkFaceV1::ALL {
        let (chunk, request) = (-16..=16)
            .flat_map(|z| (-16..=16).map(move |x| ChunkCoordinate::new(x, -4, z)))
            .find_map(|chunk| {
                plan.cave_face_field_requests(chunk)
                    .ok()?
                    .into_iter()
                    .find(|request| request.face() == face && request.portal_requested())
                    .map(|request| (chunk, request))
            })
            .unwrap_or_else(|| panic!("fixed corpus lacks a requested {face:?} field portal"));
        let neighbor = match face {
            ChunkFaceV1::NegativeX => ChunkCoordinate::new(chunk.x - 1, chunk.y, chunk.z),
            ChunkFaceV1::PositiveX => ChunkCoordinate::new(chunk.x + 1, chunk.y, chunk.z),
            ChunkFaceV1::NegativeY => ChunkCoordinate::new(chunk.x, chunk.y - 1, chunk.z),
            ChunkFaceV1::PositiveY => ChunkCoordinate::new(chunk.x, chunk.y + 1, chunk.z),
            ChunkFaceV1::NegativeZ => ChunkCoordinate::new(chunk.x, chunk.y, chunk.z - 1),
            ChunkFaceV1::PositiveZ => ChunkCoordinate::new(chunk.x, chunk.y, chunk.z + 1),
        };
        let opposite_request = plan
            .cave_face_field_requests(neighbor)
            .expect("neighbor field requests are representable")
            .into_iter()
            .find(|candidate| candidate.face() == face.opposite())
            .expect("neighbor has all six field requests");
        assert_eq!(request.key(), opposite_request.key());
        assert_eq!(request.portal_u_voxel(), opposite_request.portal_u_voxel());
        assert_eq!(request.portal_v_voxel(), opposite_request.portal_v_voxel());
        assert_eq!(
            request.clearance_radius_voxels(),
            opposite_request.clearance_radius_voxels()
        );

        for (side, local_face) in [(chunk, face), (neighbor, face.opposite())] {
            let outcome = plan
                .generate(vacant_request(side))
                .expect("deep shared-face chunk generates");
            let validation = prepared(&outcome)
                .receipt()
                .cave_occupancy_validations()
                .iter()
                .copied()
                .find(|candidate| candidate.request().face() == local_face)
                .expect("all six final occupancy validations exist");
            let diameter = u32::from(validation.request().clearance_radius_voxels())
                .saturating_mul(2)
                .saturating_add(1);
            assert_eq!(
                validation.aperture_samples(),
                diameter.saturating_mul(diameter)
            );
            assert_eq!(
                validation.field_void_samples(),
                validation.aperture_samples()
            );
            assert_eq!(
                validation.final_empty_samples(),
                validation.aperture_samples()
            );
            assert!(validation.portal_clearance_intact());
            assert!(validation.is_finally_open());
        }
    }
}

#[test]
fn material_ore_and_fluid_placement_use_final_cave_occupancy() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let empty = plan.role_target(D4MaterialRoleV1::Empty);
    let copper = plan.role_target(D4MaterialRoleV1::CopperResource);
    let mut inspected_voids = 0_u32;
    let mut inspected_solids = 0_u32;
    let (portal_chunk, _) = (-16..=16)
        .flat_map(|z| (-16..=16).map(move |x| ChunkCoordinate::new(x, -4, z)))
        .find_map(|chunk| {
            plan.cave_face_field_requests(chunk)
                .ok()?
                .into_iter()
                .find(|request| request.portal_requested())
                .map(|request| (chunk, request))
        })
        .expect("deep corpus contains a requested portal");
    let chunks = [
        portal_chunk,
        ChunkCoordinate::new(portal_chunk.x + 1, portal_chunk.y, portal_chunk.z),
        ChunkCoordinate::new(portal_chunk.x, portal_chunk.y - 1, portal_chunk.z),
    ];
    for chunk in chunks {
        let outcome = plan
            .generate(vacant_request(chunk))
            .expect("deep occupancy chunk generates");
        let candidate = prepared(&outcome);
        let edge = i64::from(candidate.draft().edge_voxels());
        let origin_x = i64::from(chunk.x).saturating_mul(edge);
        let origin_y = i64::from(chunk.y).saturating_mul(edge);
        let origin_z = i64::from(chunk.z).saturating_mul(edge);
        for local_y in 0..edge {
            for local_z in 0..edge {
                for local_x in 0..edge {
                    let world_x = origin_x.saturating_add(local_x);
                    let world_y = origin_y.saturating_add(local_y);
                    let world_z = origin_z.saturating_add(local_z);
                    let occupancy = plan.cave_occupancy_arbitration(world_x, world_y, world_z);
                    let local_x = u16::try_from(local_x).expect("local X fits the draft");
                    let local_y = u16::try_from(local_y).expect("local Y fits the draft");
                    let local_z = u16::try_from(local_z).expect("local Z fits the draft");
                    let block = candidate
                        .draft()
                        .block_at(local_x, local_y, local_z)
                        .expect("draft contains the occupancy sample");
                    assert_eq!(
                        occupancy.allows_fluid_occupancy(),
                        occupancy.is_finally_void()
                    );
                    if occupancy.is_finally_void() {
                        inspected_voids = inspected_voids.saturating_add(1);
                        assert_eq!(block, empty);
                        assert!(!occupancy.allows_solid_placement());
                        assert_ne!(block, copper);
                    } else {
                        inspected_solids = inspected_solids.saturating_add(1);
                        assert!(occupancy.allows_solid_placement());
                        assert!(!occupancy.allows_fluid_occupancy());
                    }
                }
            }
        }
        for validation in candidate.receipt().cave_occupancy_validations() {
            if validation.request().portal_requested() {
                assert!(validation.portal_clearance_intact());
            }
        }
    }
    assert!(inspected_voids > 0);
    assert!(inspected_solids > 0);
}

#[test]
fn both_fixture_styles_and_named_transition_are_queryable() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let edge = 64_i64;
    let mut found = BTreeSet::new();
    let mut transition = None;
    for cell_z in -16_i64..=16 {
        for cell_x in -16_i64..=16 {
            let center_x = cell_x.saturating_mul(edge).saturating_add(edge / 2);
            let center_z = cell_z.saturating_mul(edge).saturating_add(edge / 2);
            found.insert(plan.territory_query(center_x, center_z).winner());
            let east_edge = cell_x
                .saturating_mul(edge)
                .saturating_add(edge.saturating_sub(1));
            let query = plan.territory_query(east_edge, center_z);
            if query.transition().is_active() {
                transition = Some(query);
            }
        }
    }
    assert_eq!(
        found,
        BTreeSet::from([
            TerrainStyleV1::TemperateWoodland,
            TerrainStyleV1::AridBadlands
        ])
    );
    let transition = transition.expect("fixed seed corpus crosses a named transition");
    assert_ne!(
        transition.winner(),
        transition.transition().adjacent_style()
    );
    assert_eq!(transition.boundary_distance_voxels(), 0);
    assert_eq!(transition.transition().width_voxels(), 8);
}

#[test]
fn high_relief_height_blends_continuously_through_planning_cell_corners() {
    let mut config = default_config();
    config.transition_width_voxels = 16;
    config.height_noise_scale_voxels = 32;
    config.world_ceiling_y = 319;
    config.temperate_base_height = 80;
    config.temperate_relief = 112;
    config.arid_base_height = 88;
    config.arid_relief = 128;
    let transition_width = i64::from(config.transition_width_voxels);
    let cell_edge = i64::from(config.chunk_edge_voxels)
        .saturating_mul(i64::from(config.planning_cell_edge_chunks));
    let plan = GenerationPlanV1::compile(fixture_input(
        config,
        provider_offers(false),
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        vec![CanonicalHash::digest(b"lock-a")],
        WorldgenLimitsV1::default(),
    ))
    .expect("high-relief transition plan compiles");

    let mut maximum_step = 0_u32;
    for cell_z in -4_i64..=4 {
        for cell_x in -4_i64..=4 {
            let corner_x = cell_x.saturating_mul(cell_edge);
            let corner_z = cell_z.saturating_mul(cell_edge);
            for z in corner_z.saturating_sub(transition_width)
                ..=corner_z.saturating_add(transition_width)
            {
                for x in corner_x.saturating_sub(transition_width)
                    ..=corner_x.saturating_add(transition_width)
                {
                    let height = plan.terrain_height(x, z);
                    maximum_step = maximum_step
                        .max(height.abs_diff(plan.terrain_height(x.saturating_add(1), z)))
                        .max(height.abs_diff(plan.terrain_height(x, z.saturating_add(1))));
                }
            }
        }
    }
    assert!(
        maximum_step <= 12,
        "high-relief transition introduced a {maximum_step}-voxel adjacent cliff"
    );
}

#[test]
fn deterministic_height_density_and_coarse_void_use_y_as_height() {
    let mut config = default_config();
    config.cave_threshold_per_1024 = 1_024;
    let plan = GenerationPlanV1::compile(fixture_input(
        config,
        provider_offers(false),
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        vec![CanonicalHash::digest(b"lock-a")],
        WorldgenLimitsV1::default(),
    ))
    .expect("all-cave test plan compiles");
    let height = plan.terrain_height(13, -9);
    assert_eq!(plan.terrain_height(13, -9), height);
    assert!(plan.terrain_density(13, i64::from(height) + 1, -9) < 0);
    assert_eq!(plan.terrain_density(13, i64::from(height) - 8, -9), -1);
}

#[test]
fn style_materialization_uses_role_resolved_concrete_blocks() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    for (style, required) in [
        (TerrainStyleV1::TemperateWoodland, &["grass", "dirt"][..]),
        (
            TerrainStyleV1::AridBadlands,
            &["sand", "red-sand", "sandstone", "red-sandstone"][..],
        ),
    ] {
        let coordinate = find_surface_chunk(&plan, style);
        let mut used = BTreeSet::new();
        for sample in [
            coordinate,
            ChunkCoordinate::new(coordinate.x, coordinate.y.saturating_sub(1), coordinate.z),
        ] {
            let outcome = plan
                .generate(vacant_request(sample))
                .expect("style surface and subsurface chunks generate");
            let candidate = prepared(&outcome);
            used.extend(
                candidate
                    .draft()
                    .voxel_palette_indices()
                    .iter()
                    .map(|index| {
                        candidate.draft().palette()[usize::from(*index)]
                            .as_str()
                            .to_owned()
                    }),
            );
        }
        if style == TerrainStyleV1::TemperateWoodland {
            assert!(required.iter().all(|path| {
                let id = format!("terrenia:block/{path}");
                used.contains(id.as_str())
            }));
        } else {
            assert!(required.iter().any(|path| {
                let id = format!("terrenia:block/{path}");
                used.contains(id.as_str())
            }));
            assert!(
                used.contains("terrenia:block/basalt")
                    || used.contains("terrenia:block/copper-ore")
            );
        }
    }
}

#[test]
fn deterministic_two_style_corpus_materializes_every_required_role() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let mut used = BTreeSet::<StableId>::new();
    'search: for chunk_z in -12..=12 {
        for chunk_x in -12..=12 {
            let world_x = i64::from(chunk_x).saturating_mul(8).saturating_add(4);
            let world_z = i64::from(chunk_z).saturating_mul(8).saturating_add(4);
            let surface_chunk_y = plan.terrain_height(world_x, world_z).div_euclid(8);
            for offset_y in -3..=1 {
                let coordinate = ChunkCoordinate::new(
                    chunk_x,
                    surface_chunk_y.saturating_add(offset_y),
                    chunk_z,
                );
                let outcome = plan
                    .generate(vacant_request(coordinate))
                    .expect("bounded role corpus chunk generates");
                let draft = prepared(&outcome).draft();
                for index in draft.voxel_palette_indices() {
                    used.insert(draft.palette()[usize::from(*index)].clone());
                }
                if ROLE_TARGETS
                    .iter()
                    .all(|(_, path)| used.contains(&block_id(path)))
                {
                    break 'search;
                }
            }
        }
    }
    let missing = ROLE_TARGETS
        .iter()
        .filter_map(|(role, path)| (!used.contains(&block_id(path))).then_some(*role))
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "role corpus missed {missing:?}");
}
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one table-like test keeps every independent hard-limit boundary visible"
)]
fn every_hard_limit_rejects_boundary_plus_one_before_unbounded_work() {
    let provider_limits = WorldgenLimitsV1 {
        max_provider_offers: NonZeroU16::new(6).unwrap_or(NonZeroU16::MIN),
        max_plan_input_bytes: NonZeroU64::MIN,
        ..WorldgenLimitsV1::default()
    };
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input(
            default_config(),
            provider_offers(false),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            vec![],
            provider_limits
        )),
        Err(WorldgenError::CollectionLimitExceeded {
            kind: "provider offers",
            actual: 7,
            limit: 6
        })
    ));

    let role_limits = WorldgenLimitsV1 {
        max_role_bindings: NonZeroU16::new(15).unwrap_or(NonZeroU16::MIN),
        max_plan_input_bytes: NonZeroU64::MIN,
        ..WorldgenLimitsV1::default()
    };
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input(
            default_config(),
            provider_offers(false),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            vec![],
            role_limits,
        )),
        Err(WorldgenError::CollectionLimitExceeded {
            kind: "frozen role bindings",
            actual: 16,
            limit: 15,
        })
    ));

    let catalog_limits = WorldgenLimitsV1 {
        max_catalog_blocks: NonZeroU16::new(17).unwrap_or(NonZeroU16::MIN),
        max_plan_input_bytes: NonZeroU64::MIN,
        ..WorldgenLimitsV1::default()
    };
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input(
            default_config(),
            provider_offers(false),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            vec![],
            catalog_limits,
        )),
        Err(WorldgenError::CollectionLimitExceeded {
            kind: "D4 catalog blocks",
            actual: 18,
            limit: 17,
        })
    ));

    let voxel_limits = WorldgenLimitsV1 {
        max_voxels_per_chunk: NonZeroU32::new(511).unwrap_or(NonZeroU32::MIN),
        ..WorldgenLimitsV1::default()
    };
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input(
            default_config(),
            provider_offers(false),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            vec![],
            voxel_limits
        )),
        Err(WorldgenError::BudgetExceeded {
            budget: "voxels per chunk",
            required: 512,
            limit: 511
        })
    ));

    let identity_limits = WorldgenLimitsV1 {
        max_identity_bytes: NonZeroU32::new(8).unwrap_or(NonZeroU32::MIN),
        ..WorldgenLimitsV1::default()
    };
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input(
            default_config(),
            provider_offers(false),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            vec![],
            identity_limits,
        )),
        Err(WorldgenError::BudgetExceeded {
            budget: "stable identity bytes",
            limit: 8,
            ..
        })
    ));

    let input_limits = WorldgenLimitsV1 {
        max_plan_input_bytes: NonZeroU64::new(4_096).unwrap_or(NonZeroU64::MIN),
        ..WorldgenLimitsV1::default()
    };
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input(
            default_config(),
            provider_offers(false),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            vec![],
            input_limits,
        )),
        Err(WorldgenError::BudgetExceeded {
            budget: "plan input bytes",
            limit: 4_096,
            ..
        })
    ));

    let snapshot_limits = WorldgenLimitsV1 {
        max_snapshot_bytes: NonZeroU64::MIN,
        ..WorldgenLimitsV1::default()
    };
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input(
            default_config(),
            provider_offers(false),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            vec![],
            snapshot_limits,
        )),
        Err(WorldgenError::BudgetExceeded {
            budget: "snapshot bytes",
            limit: 1,
            ..
        })
    ));

    let live_limits = WorldgenLimitsV1 {
        max_live_generation_bytes: NonZeroU64::MIN,
        ..WorldgenLimitsV1::default()
    };
    assert!(matches!(
        GenerationPlanV1::compile(fixture_input(
            default_config(),
            provider_offers(false),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            vec![],
            live_limits,
        )),
        Err(WorldgenError::BudgetExceeded {
            budget: "live generation bytes",
            limit: 1,
            ..
        })
    ));
    let adjacency_limits = WorldgenLimitsV1 {
        max_adjacent_epochs: NonZeroU16::new(3).unwrap_or(NonZeroU16::MIN),
        ..WorldgenLimitsV1::default()
    };
    let limited_plan = GenerationPlanV1::compile(fixture_input(
        default_config(),
        provider_offers(false),
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        vec![],
        adjacency_limits,
    ))
    .expect("the request-level adjacency limit compiles");
    assert!(matches!(
        limited_plan.generate(vacant_request(ChunkCoordinate::new(0, 0, 0))),
        Err(WorldgenError::CollectionLimitExceeded {
            kind: "complete adjacent epoch states",
            actual: 4,
            limit: 3
        })
    ));
}

#[test]
fn fixed_algorithm_hashes_match_known_answer_vectors() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    assert_eq!(
        [
            plan.config_hash().to_string(),
            plan.generator_fingerprint().to_string(),
            plan.locked_closure_fingerprint().to_string(),
            plan.generation_input_hash().to_string(),
            plan.generation_provenance_hash().to_string(),
            plan.generation_epoch().to_string(),
        ],
        [
            "d50126fa9254fe1f5426e2f31d93ec43340d81506f6444e59662ce55198a9c9d".to_owned(),
            "27cc4f80bd6fe8f530ed76d60cd9e4cda9dbe1b062787bfa122085ae83fb57c4".to_owned(),
            "52898402e6d702372b222dba71d84b0b1bc4111800d40c74ad3ef3808401112c".to_owned(),
            "1a6ab5de5bdcacb671498b32b46085b01e25de2ff2e31fb9b5f5fced97c40e09".to_owned(),
            "b9d34472374d7b3a1558cca99e820e2243f531aed9044d2c04cf3243bbea820b".to_owned(),
            "82dd0cd7c0ee48cfb094fce6c42a31350500f0193a7cfb64242735587314cce2".to_owned(),
        ]
    );
}

#[test]
fn fixed_d4_snapshot_has_stable_golden_checksum() {
    let plan = fixture_plan(false, &[b"lock-a", b"lock-b"]);
    let outcome = plan
        .generate(vacant_request(ChunkCoordinate::new(-3, 2, 5)))
        .expect("golden chunk generates");
    assert_eq!(
        prepared(&outcome).checksum().to_string(),
        "4b00586c0dcbae28e703c322ef4684a80d06a30e8dfd84c5599cae1b57dcb441"
    );
}

#[test]
fn both_style_surface_snapshots_have_independent_goldens() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let actual = [
        TerrainStyleV1::TemperateWoodland,
        TerrainStyleV1::AridBadlands,
    ]
    .into_iter()
    .map(|style| {
        let coordinate = find_surface_chunk(&plan, style);
        let outcome = plan
            .generate(vacant_request(coordinate))
            .expect("style golden chunk generates");
        (style, coordinate, prepared(&outcome).checksum().to_string())
    })
    .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (
                TerrainStyleV1::TemperateWoodland,
                ChunkCoordinate::new(-198, 2, -255),
                "1c0c6faaa6c8cf44ca3213942f11570ae049c70a337105555f04c31f6563b472".to_owned(),
            ),
            (
                TerrainStyleV1::AridBadlands,
                ChunkCoordinate::new(-254, 2, -255),
                "b54fa884662871d5b46f2e6a9efbb5df0270b8b5945072ef82acf2d1e906ebf0".to_owned(),
            ),
        ]
    );
}

#[test]
fn origin_neighborhood_includes_negative_xz_and_commits_snapshot_candidates() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    let region = BoundedGeneratedRegionV1::materialize(&plan)
        .expect("origin neighborhood generates through D4");
    assert_eq!(region.len(), ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1.len());
    assert!(
        ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1
            .iter()
            .any(|coordinate| coordinate.x < 0 && coordinate.z < 0)
    );
    for coordinate in ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1 {
        assert!(region.candidate(coordinate).is_some());
    }

    let world = fixture_world_id();
    let transaction = region
        .to_storage_transaction(world, TransactionId::from_u128(1), WorldRevision::ZERO)
        .expect("region converts to kernel mutations without opening a writer");
    assert_eq!(
        transaction.mutations().len(),
        ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1.len()
    );

    let storage = MemoryTransactionKernel::new();
    let receipt = storage
        .commit(transaction)
        .expect("in-memory kernel accepts D4 snapshot candidates");
    assert_eq!(
        receipt.chunks().len(),
        ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1.len()
    );

    let snapshot = storage
        .reference_snapshot(world)
        .expect("committed region remains readable");
    for coordinate in ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1 {
        let candidate = region
            .candidate(coordinate)
            .expect("generated neighborhood contains the coordinate");
        let stored = snapshot
            .chunk(&ChunkKey::new(world, plan.dimension().clone(), coordinate))
            .expect("kernel snapshot contains the generated chunk");
        assert_eq!(stored.data().voxels().bytes(), candidate.snapshot_bytes());
        assert_eq!(
            stored.data().provenance().len(),
            2,
            "receipt and checksum sidecars are persisted with the candidate"
        );
    }
}

#[test]
fn same_seed_and_config_match_across_independent_region_runs() {
    let first_plan = fixture_plan_with_seed(42);
    let second_plan = fixture_plan_with_seed(42);
    let divergent_plan = fixture_plan_with_seed(43);
    assert_eq!(
        first_plan.generation_input_hash(),
        second_plan.generation_input_hash()
    );
    assert_ne!(
        first_plan.generation_input_hash(),
        divergent_plan.generation_input_hash()
    );

    let first = BoundedGeneratedRegionV1::materialize(&first_plan)
        .expect("first independent run materializes");
    let second = BoundedGeneratedRegionV1::materialize_coordinates(
        &second_plan,
        ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1.into_iter().rev(),
    )
    .expect("second independent run materializes in reverse request order");
    let divergent = BoundedGeneratedRegionV1::materialize(&divergent_plan)
        .expect("divergent seed still materializes");
    assert_eq!(first, second);
    assert_ne!(first, divergent);

    let world = fixture_world_id();
    let first_hash = commit_region(&first, world, 11).materialized_chunk_state_hash();
    let second_hash = commit_region(&second, world, 11).materialized_chunk_state_hash();
    let divergent_hash = commit_region(&divergent, world, 11).materialized_chunk_state_hash();
    assert_eq!(first_hash, second_hash);
    assert_ne!(first_hash, divergent_hash);
}

#[test]
fn bounded_region_requests_fail_closed() {
    let plan = fixture_plan(false, &[b"lock-a"]);
    assert!(matches!(
        BoundedGeneratedRegionV1::materialize_coordinates(&plan, []),
        Err(WorldgenError::EmptyGeneratedRegion)
    ));
    assert!(matches!(
        BoundedGeneratedRegionV1::materialize_coordinates(
            &plan,
            [
                ChunkCoordinate::new(0, 0, 0),
                ChunkCoordinate::new(0, 0, 0),
            ]
        ),
        Err(WorldgenError::DuplicateGeneratedRegionChunk {
            coordinate
        }) if coordinate == ChunkCoordinate::new(0, 0, 0)
    ));

    let too_many = (0..=i32::try_from(MAX_BOUNDED_REGION_CHUNKS).expect("limit fits i32"))
        .map(|x| ChunkCoordinate::new(x, 0, 0));
    assert!(matches!(
        BoundedGeneratedRegionV1::materialize_coordinates(&plan, too_many),
        Err(WorldgenError::GeneratedRegionLimitExceeded {
            actual,
            limit: MAX_BOUNDED_REGION_CHUNKS
        }) if actual == MAX_BOUNDED_REGION_CHUNKS + 1
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn coordinate_queries_are_repeatable_across_plan_permutations(
        x in any::<i32>(),
        y in any::<i16>(),
        z in any::<i32>(),
    ) {
        let forward = fixture_plan(false, &[b"lock-a"]);
        let reverse = fixture_plan(true, &[b"lock-a"]);
        let x = i64::from(x);
        let y = i64::from(y);
        let z = i64::from(z);
        prop_assert_eq!(forward.territory_query(x, z), reverse.territory_query(x, z));
        prop_assert_eq!(forward.terrain_height(x, z), reverse.terrain_height(x, z));
        prop_assert_eq!(forward.terrain_density(x, y, z), reverse.terrain_density(x, y, z));
    }

    #[test]
    fn minimum_cover_boundary_is_inclusive_for_an_all_void_raw_field(
        x in -4_096_i32..=4_096_i32,
        z in -4_096_i32..=4_096_i32,
    ) {
        let plan = all_cave_plan();
        let x = i64::from(x);
        let z = i64::from(z);
        let height = i64::from(plan.terrain_height(x, z));
        let cover = i64::from(default_config().cave_minimum_cover);
        let protected_y = height.saturating_sub(cover).saturating_add(1);
        let first_carvable_y = height.saturating_sub(cover);
        prop_assert!(plan.cave_signed_distance_fixed(x, protected_y, z) <= 0);
        prop_assert!(plan.cave_signed_distance_fixed(x, first_carvable_y, z) <= 0);
        prop_assert_eq!(
            plan.terrain_density(x, protected_y, z),
            cover.saturating_sub(1)
        );
        prop_assert_eq!(plan.terrain_density(x, first_carvable_y, z), -1);
    }
}

fn fixture_plan(reverse_providers: bool, locks: &[&[u8]]) -> GenerationPlanV1 {
    let receipts = locks.iter().map(CanonicalHash::digest).collect::<Vec<_>>();
    GenerationPlanV1::compile(fixture_input(
        default_config(),
        provider_offers(reverse_providers),
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        receipts,
        WorldgenLimitsV1::default(),
    ))
    .expect("valid fixture plan compiles")
}

fn all_cave_plan() -> GenerationPlanV1 {
    let mut config = default_config();
    config.cave_threshold_per_1024 = 1_024;
    GenerationPlanV1::compile(fixture_input(
        config,
        provider_offers(false),
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        vec![CanonicalHash::digest(b"lock-a")],
        WorldgenLimitsV1::default(),
    ))
    .expect("all-cave fixture plan compiles")
}

fn fixture_plan_with_seed(seed: i64) -> GenerationPlanV1 {
    GenerationPlanV1::compile(GenerationPlanInputV1::new(
        dimension_id(),
        WorldSeedV1::from_integer(seed),
        default_config(),
        7,
        PlanActivationIdV1::from_hash(CanonicalHash::digest(b"fixture-activation")),
        provider_offers(false),
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        CanonicalHash::digest(b"authoritative-semantic-image"),
        vec![CanonicalHash::digest(b"lock-a")],
        WorldgenLimitsV1::default(),
    ))
    .expect("seeded fixture plan compiles")
}

fn fixture_world_id() -> WorldId {
    "018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd"
        .parse()
        .expect("fixture world UUID is a canonical version-4 identifier")
}

fn commit_region(
    region: &BoundedGeneratedRegionV1,
    world: WorldId,
    transaction: u128,
) -> CommitReceipt {
    let storage = MemoryTransactionKernel::new();
    storage
        .commit(
            region
                .to_storage_transaction(
                    world,
                    TransactionId::from_u128(transaction),
                    WorldRevision::ZERO,
                )
                .expect("region converts to an in-memory transaction"),
        )
        .expect("in-memory kernel commits generated snapshot candidates")
}

fn fixture_plan_with_activation(activation: &[u8]) -> GenerationPlanV1 {
    GenerationPlanV1::compile(GenerationPlanInputV1::new(
        dimension_id(),
        WorldSeedV1::from_integer(42),
        default_config(),
        7,
        PlanActivationIdV1::from_hash(CanonicalHash::digest(activation)),
        provider_offers(false),
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        CanonicalHash::digest(b"authoritative-semantic-image"),
        vec![CanonicalHash::digest(b"lock-a")],
        WorldgenLimitsV1::default(),
    ))
    .expect("activation fixture plan compiles")
}
fn fixture_input_with_defaults(offers: Vec<ProviderOfferV1>) -> GenerationPlanInputV1 {
    fixture_input(
        default_config(),
        offers,
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        vec![CanonicalHash::digest(b"lock-a")],
        WorldgenLimitsV1::default(),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "test helper mirrors the public hash boundary"
)]
fn fixture_input(
    config: WorldgenConfigV1,
    offers: Vec<ProviderOfferV1>,
    vocabulary: D4RoleVocabularyV1,
    bindings: FrozenRoleBindingsV1,
    catalog: D4BlockCatalogClosureV1,
    locks: Vec<CanonicalHash>,
    limits: WorldgenLimitsV1,
) -> GenerationPlanInputV1 {
    GenerationPlanInputV1::new(
        dimension_id(),
        WorldSeedV1::from_integer(42),
        config,
        7,
        PlanActivationIdV1::from_hash(CanonicalHash::digest(b"fixture-activation")),
        offers,
        vocabulary,
        bindings,
        catalog,
        CanonicalHash::digest(b"authoritative-semantic-image"),
        locks,
        limits,
    )
}

fn default_config() -> WorldgenConfigV1 {
    WorldgenConfigV1 {
        chunk_edge_voxels: 8,
        planning_cell_edge_chunks: 8,
        transition_width_voxels: 8,
        ..WorldgenConfigV1::default()
    }
}

fn provider_offers(reverse: bool) -> Vec<ProviderOfferV1> {
    let paths = [
        (ProviderSlotV1::GenerationCoordinator, "coordinator"),
        (ProviderSlotV1::StyleSelector, "selector"),
        (ProviderSlotV1::TemperateTerrain, "temperate"),
        (ProviderSlotV1::AridTerrain, "arid"),
        (ProviderSlotV1::TerrainTransition, "transition"),
        (ProviderSlotV1::CaveTopology, "cave"),
        (ProviderSlotV1::Materializer, "materializer"),
    ];
    let mut offers = paths
        .into_iter()
        .map(|(slot, path)| {
            let revision = if slot == ProviderSlotV1::CaveTopology {
                8
            } else {
                7
            };
            ProviderOfferV1::new(
                slot,
                ProviderGenerationIdentityV1::new(
                    stable_id(&format!("fixture:worldgen-provider/{path}@1")),
                    NonZeroU32::MIN,
                    revision,
                    CanonicalHash::digest(format!("{path}-implementation-v{revision}")),
                ),
            )
        })
        .collect::<Vec<_>>();
    if reverse {
        offers.reverse();
    }
    offers
}

fn role_vocabulary_entries() -> Vec<(D4MaterialRoleV1, StableId)> {
    ROLE_TARGETS
        .iter()
        .map(|(purpose, _)| {
            (
                *purpose,
                stable_id(&format!("terrenia:block-role/d4/{}@1", purpose.as_str())),
            )
        })
        .collect()
}

fn role_entries() -> Vec<(StableId, StableId)> {
    ROLE_TARGETS
        .iter()
        .map(|(purpose, target)| {
            (
                stable_id(&format!("terrenia:block-role/d4/{}@1", purpose.as_str())),
                block_id(target),
            )
        })
        .collect()
}

fn role_vocabulary() -> D4RoleVocabularyV1 {
    D4RoleVocabularyV1::new(role_vocabulary_entries()).expect("fixture vocabulary is complete")
}

fn role_bindings() -> FrozenRoleBindingsV1 {
    FrozenRoleBindingsV1::new(role_entries()).expect("fixture role bindings are valid")
}

fn block_catalog() -> D4BlockCatalogClosureV1 {
    D4BlockCatalogClosureV1::new(CATALOG_PATHS.iter().map(|path| block_id(path)))
        .expect("18-block fixture catalog is valid")
}

fn generated_by_coordinate(
    plan: &GenerationPlanV1,
    coordinates: impl IntoIterator<Item = ChunkCoordinate>,
) -> BTreeMap<ChunkCoordinate, GeneratedEvidence> {
    coordinates
        .into_iter()
        .map(|coordinate| {
            let outcome = plan
                .generate(vacant_request(coordinate))
                .expect("fixture chunk generates");
            let candidate = prepared(&outcome);
            (
                coordinate,
                (
                    candidate.draft().canonical_bytes().unwrap_or_default(),
                    candidate.receipt().canonical_bytes().unwrap_or_default(),
                    candidate.snapshot_bytes().to_vec(),
                ),
            )
        })
        .collect()
}

fn find_surface_chunk(plan: &GenerationPlanV1, style: TerrainStyleV1) -> ChunkCoordinate {
    let edge = 8_i64;
    let cell_edge = 64_i64;
    for cell_z in -32_i64..=32 {
        for cell_x in -32_i64..=32 {
            for local in 8_i64..56 {
                let world_x = cell_x.saturating_mul(cell_edge).saturating_add(local);
                let world_z = cell_z
                    .saturating_mul(cell_edge)
                    .saturating_add(local.saturating_mul(3).rem_euclid(48).saturating_add(8));
                let query = plan.territory_query(world_x, world_z);
                let height = plan.terrain_height(world_x, world_z);
                if query.winner() == style && height.rem_euclid(8) >= 4 {
                    return ChunkCoordinate::new(
                        i32::try_from(world_x.div_euclid(edge)).unwrap_or_default(),
                        height.div_euclid(i32::try_from(edge).unwrap_or(8)),
                        i32::try_from(world_z.div_euclid(edge)).unwrap_or_default(),
                    );
                }
            }
        }
    }
    panic!("fixed corpus did not find style {style:?}")
}

fn vacant_request(coordinate: ChunkCoordinate) -> ChunkGenerationRequestV1 {
    ChunkGenerationRequestV1::new(
        coordinate,
        None,
        CellEpochStateV1::Unassigned,
        unassigned_adjacency(coordinate),
        Vec::new(),
    )
}

fn unassigned_adjacency(coordinate: ChunkCoordinate) -> AdjacentEpochSnapshotV1 {
    let edge = i64::from(default_config().planning_cell_edge_chunks);
    let cell = PlanningCellCoordinateV1::new(
        i64::from(coordinate.x).div_euclid(edge),
        i64::from(coordinate.z).div_euclid(edge),
    );
    AdjacentEpochSnapshotV1::all_unassigned(cell)
        .expect("fixture planning-cell neighbors are representable")
}

fn prepared(outcome: &ChunkGenerationOutcomeV1) -> &latticeaxiom_worldgen::D4SnapshotCandidateV1 {
    let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
        panic!("fixture unexpectedly reused an existing snapshot")
    };
    candidate
}

fn dimension_id() -> DimensionId {
    "terrenia:dimension/terrenia"
        .parse()
        .unwrap_or_else(|error| panic!("invalid fixture dimension: {error}"))
}

fn stable_id(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid fixture stable ID `{value}`: {error}"))
}

fn block_id(path: &str) -> StableId {
    stable_id(&format!("terrenia:block/{path}"))
}
