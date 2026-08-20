#![allow(
    clippy::expect_used,
    reason = "property fixtures use literals whose validity is a test invariant"
)]

use std::{cmp::Ordering, str::FromStr};

use latticeaxiom_core::WorldId;

use proptest::prelude::*;

use crate::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCoordinate, ChunkMutation,
    ChunkRevisionExpectation, MemoryTransactionKernel, PersistentEntityId, StorageError,
    TransactionId, TransactionKernelLimits, WorldRevision, WorldTransaction,
    conformance::{sample_data, sample_key, sample_world},
};

proptest! {
    #[test]
    fn coordinate_order_is_canonical_xyz(
        left in (any::<i32>(), any::<i32>(), any::<i32>()),
        right in (any::<i32>(), any::<i32>(), any::<i32>()),
    ) {
        let left_coordinate = ChunkCoordinate::new(left.0, left.1, left.2);
        let right_coordinate = ChunkCoordinate::new(right.0, right.1, right.2);
        prop_assert_eq!(left_coordinate.cmp(&right_coordinate), left.cmp(&right));
    }

    #[test]
    fn transaction_permutation_preserves_materialized_chunk_state_hash(
        coordinates in prop::collection::btree_set(
            (any::<i16>(), any::<i16>(), any::<i16>()),
            1..16,
        ),
    ) {
        let world = sample_world();
        let mutations = coordinates
            .iter()
            .map(|(x, y, z)| {
                let seed = x.to_le_bytes()[0] ^ y.to_le_bytes()[0] ^ z.to_le_bytes()[0];
                let entity_id = (u128::from(u16::from_be_bytes(x.to_be_bytes())) << 32)
                    | (u128::from(u16::from_be_bytes(y.to_be_bytes())) << 16)
                    | u128::from(u16::from_be_bytes(z.to_be_bytes()));
                let mut data = sample_data(seed);
                let (_, payload) = data
                    .persistent_entities
                    .pop_first()
                    .expect("sample property data contains one entity");
                data.persistent_entities
                    .insert(PersistentEntityId::from_u128(entity_id), payload);
                ChunkMutation::new(
                    sample_key(
                        world,
                        ChunkCoordinate::new(i32::from(*x), i32::from(*y), i32::from(*z)),
                    ),
                    ChunkRevisionExpectation::Absent,
                    ChangedDomains::ALL,
                    data,
                )            })
            .collect::<Vec<_>>();
        let mut reversed = mutations.clone();
        reversed.reverse();
        let forward = WorldTransaction::new(
            TransactionId::from_u128(100),
            world,
            WorldRevision::ZERO,
            mutations,
        );
        let reverse = WorldTransaction::new(
            TransactionId::from_u128(101),
            world,
            WorldRevision::ZERO,
            reversed,
        );
        let first = MemoryTransactionKernel::new()
            .commit(forward)
            .expect("a unique bounded property transaction must commit");
        let second = MemoryTransactionKernel::new()
            .commit(reverse)
            .expect("the reversed unique bounded transaction must commit");
        prop_assert_eq!(first.materialized_chunk_state_hash(), second.materialized_chunk_state_hash());
        prop_assert_eq!(first.chunks(), second.chunks());
    }

    #[test]
    fn every_valid_changed_domain_mask_round_trips(bits in 0_u8..=ChangedDomains::ALL.bits()) {
        let mask = ChangedDomains::from_bits(bits)
            .expect("the generated bit range contains only defined domain bits");
        prop_assert_eq!(mask.bits(), bits);
        let encoded = serde_json::to_string(&mask)
            .expect("a valid domain mask must serialize");
        let decoded = serde_json::from_str::<ChangedDomains>(&encoded)
            .expect("serialized valid domain mask must deserialize");
        prop_assert_eq!(decoded, mask);
    }
}

#[test]
fn payload_limit_accepts_boundary_and_rejects_boundary_plus_one() {
    let world = sample_world();
    let data = sample_data(55);
    let payload_bytes = data
        .encoded_len()
        .expect("the small fixture payload length must fit u64");
    let mutation = ChunkMutation::new(
        sample_key(world, ChunkCoordinate::new(0, 0, 0)),
        ChunkRevisionExpectation::Absent,
        ChangedDomains::ALL,
        data,
    );
    let transaction = WorldTransaction::new(
        TransactionId::from_u128(200),
        world,
        WorldRevision::ZERO,
        vec![mutation],
    );
    let at_boundary = MemoryTransactionKernel::with_limits(
        TransactionKernelLimits::new(1, payload_bytes, 4)
            .expect("a positive exact payload boundary is a valid limit"),
    );
    assert!(at_boundary.commit(transaction.clone()).is_ok());

    let below_boundary = MemoryTransactionKernel::with_limits(
        TransactionKernelLimits::new(1, payload_bytes - 1, 4)
            .expect("the fixture is larger than one byte, so this limit remains positive"),
    );
    assert!(matches!(
        below_boundary.commit(transaction),
        Err(StorageError::TransactionPayloadLimitExceeded { .. })
    ));
}

#[test]
fn payload_limit_precedes_chunk_world_validation() {
    let world = sample_world();
    let other_world = WorldId::from_str("028f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
        .expect("the alternate world UUID literal is canonical");
    let data = sample_data(56);
    let payload_bytes = data
        .encoded_len()
        .expect("the small fixture payload length must fit u64");
    let storage = MemoryTransactionKernel::with_limits(
        TransactionKernelLimits::new(1, payload_bytes - 1, 4)
            .expect("the fixture payload is larger than one byte"),
    );
    let transaction = WorldTransaction::new(
        TransactionId::from_u128(201),
        world,
        WorldRevision::ZERO,
        vec![ChunkMutation::new(
            sample_key(other_world, ChunkCoordinate::new(0, 0, 0)),
            ChunkRevisionExpectation::Absent,
            ChangedDomains::ALL,
            data,
        )],
    );
    assert!(matches!(
        storage.commit(transaction),
        Err(StorageError::TransactionPayloadLimitExceeded { .. })
    ));
}
#[test]
fn coordinate_order_places_y_between_x_and_z() {
    let coordinates = [
        ChunkCoordinate::new(0, 1, -10),
        ChunkCoordinate::new(0, 0, 10),
        ChunkCoordinate::new(-1, 100, 100),
    ];
    let mut sorted = coordinates;
    sorted.sort();
    assert_eq!(
        sorted,
        [
            ChunkCoordinate::new(-1, 100, 100),
            ChunkCoordinate::new(0, 0, 10),
            ChunkCoordinate::new(0, 1, -10),
        ]
    );
    assert_eq!(
        ChunkCoordinate::new(0, 0, 1).cmp(&ChunkCoordinate::new(0, 1, 0)),
        Ordering::Less
    );
}
