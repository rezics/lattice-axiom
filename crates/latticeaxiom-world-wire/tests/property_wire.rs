//! Property tests for chunk-record key ordering and total decoding.

use std::str::FromStr;

use latticeaxiom_core::{StableId, WorldId};
use latticeaxiom_storage::{ChunkCoordinate, ChunkKey, DimensionId};
use latticeaxiom_world_wire::{
    ChunkRecordKey, RecordKind, WorldWireLimits, chunk_key_prefix, decode_chunk_record_key,
    encode_chunk_record_key,
};
use proptest::prelude::*;

fn fixture_key(
    coordinate: ChunkCoordinate,
    raw_kind: u16,
) -> Result<ChunkRecordKey, TestCaseError> {
    let world = WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
        .map_err(|error| TestCaseError::fail(error.to_string()))?;
    let dimension = DimensionId::from_str("terrenia:dimension/terrenia")
        .map_err(|error| TestCaseError::fail(error.to_string()))?;
    let owner = StableId::from_str("terrenia:schema/chunk-snapshot@1")
        .map_err(|error| TestCaseError::fail(error.to_string()))?;
    Ok(ChunkRecordKey::new(
        ChunkKey::new(world, dimension, coordinate),
        RecordKind::from_raw(raw_kind),
        owner,
    ))
}

proptest! {
    #[test]
    fn arbitrary_coordinates_and_record_tags_round_trip(
        x in any::<i32>(),
        y in any::<i32>(),
        z in any::<i32>(),
        raw_kind in any::<u16>(),
    ) {
        let key = fixture_key(ChunkCoordinate::new(x, y, z), raw_kind)?;
        let writable = fixture_key(
            ChunkCoordinate::new(x, y, z),
            RecordKind::CHUNK_SNAPSHOT.raw(),
        )?;
        let mut encoded = encode_chunk_record_key(&writable, WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let kind_offset = chunk_key_prefix(key.chunk(), WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?
            .len();
        encoded[kind_offset..kind_offset + 2].copy_from_slice(&raw_kind.to_be_bytes());
        let decoded = decode_chunk_record_key(&encoded, WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(decoded, key);
    }

    #[test]
    fn lexicographic_bytes_equal_signed_xyz_tuple_order(
        left in (any::<i32>(), any::<i32>(), any::<i32>()),
        right in (any::<i32>(), any::<i32>(), any::<i32>()),
    ) {
        let left_coordinate = ChunkCoordinate::new(left.0, left.1, left.2);
        let right_coordinate = ChunkCoordinate::new(right.0, right.1, right.2);
        let left_key = fixture_key(left_coordinate, 1)?;
        let right_key = fixture_key(right_coordinate, 1)?;
        let left_bytes = encode_chunk_record_key(&left_key, WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let right_bytes = encode_chunk_record_key(&right_key, WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let byte_order = left_bytes.cmp(&right_bytes);
        let tuple_order = left_coordinate.cmp(&right_coordinate);
        prop_assert_eq!(byte_order, tuple_order);

    }

    #[test]
    fn arbitrary_key_bytes_never_panic(candidate in proptest::collection::vec(any::<u8>(), 0..512)) {
        let outcome = std::panic::catch_unwind(|| {
            let _ = decode_chunk_record_key(&candidate, WorldWireLimits::default());
        });
        prop_assert!(outcome.is_ok());
    }

    #[test]
    fn signed_y_up_keys_keep_recovery_prefix_order(
        y in any::<i32>(),
        left_x in any::<i32>(),
        right_x in any::<i32>(),
    ) {
        let left = fixture_key(ChunkCoordinate::new(left_x, y, 0), 1)?;
        let right = fixture_key(ChunkCoordinate::new(right_x, y, 0), 1)?;
        let left_bytes = encode_chunk_record_key(&left, WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let right_bytes = encode_chunk_record_key(&right, WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let left_prefix = chunk_key_prefix(left.chunk(), WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let right_prefix = chunk_key_prefix(right.chunk(), WorldWireLimits::default())
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert!(left_bytes.starts_with(&left_prefix));
        prop_assert!(right_bytes.starts_with(&right_prefix));
        prop_assert_eq!(
            left_bytes.cmp(&right_bytes),
            left.chunk().coordinate.cmp(&right.chunk().coordinate)
        );
    }
}
