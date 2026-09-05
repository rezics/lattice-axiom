//! Chunk-record key golden, range, and rejection conformance tests.

use std::{cmp::Ordering, error::Error, str::FromStr};

use latticeaxiom_core::{StableId, WorldId};
use latticeaxiom_storage::{ChunkCoordinate, ChunkKey, DimensionId};
use latticeaxiom_world_wire::{
    ChunkRecordKey, RecordKind, WireSegment, WorldWireError, WorldWireLimits, chunk_key_prefix,
    decode_chunk_record_key, dimension_key_prefix, encode_chunk_record_key, prefix_range,
    record_kind_key_prefix, world_key_prefix,
};

fn sample_world() -> Result<WorldId, Box<dyn Error>> {
    Ok(WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")?)
}

fn sample_dimension() -> Result<DimensionId, Box<dyn Error>> {
    Ok(DimensionId::from_str("terrenia:dimension/terrenia")?)
}

fn sample_owner() -> Result<StableId, Box<dyn Error>> {
    Ok(StableId::from_str("terrenia:schema/chunk-snapshot@1")?)
}

fn sample_key(coordinate: ChunkCoordinate) -> Result<ChunkRecordKey, Box<dyn Error>> {
    Ok(ChunkRecordKey::new(
        ChunkKey::new(sample_world()?, sample_dimension()?, coordinate),
        RecordKind::CHUNK_SNAPSHOT,
        sample_owner()?,
    ))
}

#[test]
fn chunk_record_key_has_exact_v1_golden_bytes() -> Result<(), Box<dyn Error>> {
    let encoded = encode_chunk_record_key(
        &sample_key(ChunkCoordinate::new(-2, 0, 3))?,
        WorldWireLimits::default(),
    )?;
    assert_eq!(
        hex::encode(encoded),
        concat!(
            "0001",
            "018f1e2d3c4b4a598c6d7e8f9012abcd",
            "001b74657272656e69613a64696d656e73696f6e2f74657272656e6961",
            "7ffffffe8000000080000003",
            "0001",
            "002074657272656e69613a736368656d612f6368756e6b2d736e617073686f744031"
        )
    );
    Ok(())
}

#[test]
fn key_round_trip_preserves_positive_negative_and_opaque_tags() -> Result<(), Box<dyn Error>> {
    let key = ChunkRecordKey::new(
        ChunkKey::new(
            sample_world()?,
            sample_dimension()?,
            ChunkCoordinate::new(i32::MIN, -1, i32::MAX),
        ),
        RecordKind::from_raw(u16::MAX),
        sample_owner()?,
    );
    assert_eq!(RecordKind::from_raw(1), RecordKind::CHUNK_SNAPSHOT);
    assert!(RecordKind::from_raw(1).is_known_v1());
    assert!(RecordKind::from_raw(1).is_writable_v1());
    assert!(matches!(
        encode_chunk_record_key(&key, WorldWireLimits::default()),
        Err(WorldWireError::ReadOnlyRecordKind { found: u16::MAX })
    ));
    let writable = ChunkRecordKey::new(
        key.chunk().clone(),
        RecordKind::CHUNK_SNAPSHOT,
        key.owner().clone(),
    );
    let mut encoded = encode_chunk_record_key(&writable, WorldWireLimits::default())?;
    let kind_offset = chunk_key_prefix(key.chunk(), WorldWireLimits::default())?.len();
    encoded[kind_offset..kind_offset + 2].copy_from_slice(&u16::MAX.to_be_bytes());
    let decoded = decode_chunk_record_key(&encoded, WorldWireLimits::default())?;
    assert_eq!(decoded, key);
    assert!(!decoded.record_kind().is_writable_v1());
    assert_eq!(decoded.record_kind().raw(), u16::MAX);
    Ok(())
}

#[test]
fn ordered_coordinates_match_numeric_tuple_order() -> Result<(), Box<dyn Error>> {
    let coordinates = [
        ChunkCoordinate::new(i32::MIN, 0, 0),
        ChunkCoordinate::new(-2, 100, -100),
        ChunkCoordinate::new(-1, i32::MAX, i32::MAX),
        ChunkCoordinate::new(0, i32::MIN, 0),
        ChunkCoordinate::new(0, -1, i32::MAX),
        ChunkCoordinate::new(0, 0, -1),
        ChunkCoordinate::new(0, 0, 0),
        ChunkCoordinate::new(0, 0, 1),
        ChunkCoordinate::new(1, i32::MIN, i32::MIN),
        ChunkCoordinate::new(i32::MAX, 0, 0),
    ];
    let mut encoded = coordinates
        .iter()
        .map(|coordinate| {
            encode_chunk_record_key(&sample_key(*coordinate)?, WorldWireLimits::default())
                .map(|bytes| (*coordinate, bytes))
                .map_err(Into::into)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    encoded.sort_by(|left, right| left.1.cmp(&right.1));
    let decoded_order = encoded
        .into_iter()
        .map(|(coordinate, _)| coordinate)
        .collect::<Vec<_>>();
    assert_eq!(decoded_order, coordinates);
    assert_eq!(
        ChunkCoordinate::new(-1, i32::MAX, 0).cmp(&ChunkCoordinate::new(0, i32::MIN, 0)),
        Ordering::Less
    );
    Ok(())
}

#[test]
fn prefixes_form_exact_half_open_ranges() -> Result<(), Box<dyn Error>> {
    let chunk = ChunkKey::new(
        sample_world()?,
        sample_dimension()?,
        ChunkCoordinate::new(-4, 7, 12),
    );
    let key = ChunkRecordKey::new(chunk.clone(), RecordKind::CHUNK_SNAPSHOT, sample_owner()?);
    let encoded = encode_chunk_record_key(&key, WorldWireLimits::default())?;

    for prefix in [
        world_key_prefix(chunk.world),
        dimension_key_prefix(chunk.world, &chunk.dimension, WorldWireLimits::default())?,
        chunk_key_prefix(&chunk, WorldWireLimits::default())?,
        record_kind_key_prefix(
            &chunk,
            RecordKind::CHUNK_SNAPSHOT,
            WorldWireLimits::default(),
        )?,
    ] {
        let range = prefix_range(prefix.clone());
        assert_eq!(range.start(), prefix);
        assert!(range.contains(&encoded));
        assert!(range.exclusive_end().is_some());
    }

    let next_chunk = sample_key(ChunkCoordinate::new(-4, 7, 13))?;
    let next_encoded = encode_chunk_record_key(&next_chunk, WorldWireLimits::default())?;
    let chunk_range = prefix_range(chunk_key_prefix(&chunk, WorldWireLimits::default())?);
    assert!(!chunk_range.contains(&next_encoded));

    let other_world = WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abce")?;
    let other_world_key = ChunkRecordKey::new(
        ChunkKey::new(other_world, chunk.dimension.clone(), chunk.coordinate),
        RecordKind::CHUNK_SNAPSHOT,
        sample_owner()?,
    );
    let other_world_encoded =
        encode_chunk_record_key(&other_world_key, WorldWireLimits::default())?;
    assert!(!prefix_range(world_key_prefix(chunk.world)).contains(&other_world_encoded));

    let other_dimension = DimensionId::from_str("terrenia:dimension/underworld")?;
    let other_dimension_key = ChunkRecordKey::new(
        ChunkKey::new(chunk.world, other_dimension, chunk.coordinate),
        RecordKind::CHUNK_SNAPSHOT,
        sample_owner()?,
    );
    let other_dimension_encoded =
        encode_chunk_record_key(&other_dimension_key, WorldWireLimits::default())?;
    let dimension_range = prefix_range(dimension_key_prefix(
        chunk.world,
        &chunk.dimension,
        WorldWireLimits::default(),
    )?);
    assert!(!dimension_range.contains(&other_dimension_encoded));

    let kind_offset = chunk_key_prefix(&chunk, WorldWireLimits::default())?.len();
    let mut unknown_kind_encoded = encoded.clone();
    unknown_kind_encoded[kind_offset..kind_offset + 2].copy_from_slice(&2_u16.to_be_bytes());
    let kind_range = prefix_range(record_kind_key_prefix(
        &chunk,
        RecordKind::CHUNK_SNAPSHOT,
        WorldWireLimits::default(),
    )?);
    assert!(!kind_range.contains(&unknown_kind_encoded));
    Ok(())
}

#[test]
fn prefix_successor_handles_carry_and_unbounded_prefixes() {
    let carry = prefix_range(vec![0x12, u8::MAX]);
    assert_eq!(carry.exclusive_end(), Some([0x13].as_slice()));
    assert!(carry.contains(&[0x12, u8::MAX]));
    assert!(carry.contains(&[0x12, u8::MAX, 0]));
    assert!(!carry.contains(&[0x13]));

    let unbounded = prefix_range(vec![u8::MAX, u8::MAX]);
    assert_eq!(unbounded.exclusive_end(), None);
    assert!(unbounded.contains(&[u8::MAX, u8::MAX]));
    assert!(unbounded.contains(&[u8::MAX, u8::MAX, 0]));
    assert!(!unbounded.contains(&[u8::MAX]));
}

#[test]
fn key_decoder_rejects_major_truncation_malformed_and_trailing_bytes() -> Result<(), Box<dyn Error>>
{
    let encoded = encode_chunk_record_key(
        &sample_key(ChunkCoordinate::default())?,
        WorldWireLimits::default(),
    )?;

    let mut unknown_major = encoded.clone();
    unknown_major[1] = 2;
    assert!(matches!(
        decode_chunk_record_key(&unknown_major, WorldWireLimits::default()),
        Err(WorldWireError::UnsupportedKeyMajor { found: 2 })
    ));

    for length in 0..encoded.len() {
        let result = decode_chunk_record_key(&encoded[..length], WorldWireLimits::default());
        assert!(result.is_err(), "truncated length {length} was accepted");
    }

    let mut invalid_utf8 = encoded.clone();
    let dimension_start = 2 + 16 + 2;
    invalid_utf8[dimension_start] = u8::MAX;
    assert!(matches!(
        decode_chunk_record_key(&invalid_utf8, WorldWireLimits::default()),
        Err(WorldWireError::InvalidUtf8 {
            segment: WireSegment::DimensionId
        })
    ));

    let mut trailing = encoded;
    trailing.push(0);
    assert!(matches!(
        decode_chunk_record_key(&trailing, WorldWireLimits::default()),
        Err(WorldWireError::TrailingKeyBytes { remaining: 1 })
    ));
    Ok(())
}

#[test]
fn malformed_world_dimension_and_owner_identities_are_typed() -> Result<(), Box<dyn Error>> {
    let encoded = encode_chunk_record_key(
        &sample_key(ChunkCoordinate::default())?,
        WorldWireLimits::default(),
    )?;

    let mut bad_world = encoded.clone();
    bad_world[2 + 6] = 0;
    assert!(matches!(
        decode_chunk_record_key(&bad_world, WorldWireLimits::default()),
        Err(WorldWireError::InvalidWorldId { .. })
    ));

    let dimension_start = 2 + 16 + 2;
    let mut bad_dimension = encoded.clone();
    bad_dimension[dimension_start] = b'T';
    assert!(matches!(
        decode_chunk_record_key(&bad_dimension, WorldWireLimits::default()),
        Err(WorldWireError::InvalidDimensionId { .. })
    ));

    let owner_start = dimension_start
        + sample_dimension()?.as_str().len()
        + (3 * size_of::<u32>())
        + size_of::<u16>()
        + size_of::<u16>();
    let mut bad_owner = encoded;
    bad_owner[owner_start] = b'T';
    assert!(matches!(
        decode_chunk_record_key(&bad_owner, WorldWireLimits::default()),
        Err(WorldWireError::InvalidRecordOwnerId { .. })
    ));
    Ok(())
}

#[test]
fn key_segments_enforce_exact_boundary_and_boundary_plus_one() -> Result<(), Box<dyn Error>> {
    let key = sample_key(ChunkCoordinate::default())?;
    let dimension_length = u16::try_from(key.chunk().dimension.as_str().len())?;
    let owner_length = u16::try_from(key.owner().as_str().len())?;
    let exact = WorldWireLimits::new(dimension_length, owner_length, 1, 1, 1)?;
    assert!(encode_chunk_record_key(&key, exact).is_ok());

    let too_short = WorldWireLimits::new(dimension_length - 1, owner_length, 1, 1, 1)?;
    assert!(matches!(
        encode_chunk_record_key(&key, too_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::DimensionId,
            ..
        })
    ));

    let encoded = encode_chunk_record_key(&key, exact)?;
    assert!(matches!(
        decode_chunk_record_key(&encoded, too_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::DimensionId,
            ..
        })
    ));

    let owner_short = WorldWireLimits::new(dimension_length, owner_length - 1, 1, 1, 1)?;
    assert!(matches!(
        encode_chunk_record_key(&key, owner_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::RecordOwnerId,
            ..
        })
    ));
    assert!(matches!(
        decode_chunk_record_key(&encoded, owner_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::RecordOwnerId,
            ..
        })
    ));
    Ok(())
}
