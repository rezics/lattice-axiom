//! Snapshot envelope golden, mutation, and streaming conformance tests.

use std::{
    error::Error,
    io::{self, Cursor, Read, Write},
    str::FromStr,
};

use latticeaxiom_core::{PackageName, SchemaId};
use latticeaxiom_storage::{PayloadSchemaVersion, VersionedPayload};
use latticeaxiom_world_wire::{
    SnapshotContract, WireResult, WireSegment, WorldWireError, WorldWireLimits, encode_snapshot,
    preflight_snapshot, preflight_snapshot_for_contract, read_snapshot, read_snapshot_for_contract,
    require_numeric_tag, write_snapshot,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct TaggedPayloadV1 {
    kind_tag: u16,
    value: u32,
}

struct FaultAfter {
    inner: Cursor<Vec<u8>>,
    fail_at: u64,
}

impl FaultAfter {
    fn new(bytes: Vec<u8>, fail_at: u64) -> Self {
        Self {
            inner: Cursor::new(bytes),
            fail_at,
        }
    }

    fn position(&self) -> u64 {
        self.inner.position()
    }
}

impl Read for FaultAfter {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let position = self.inner.position();
        if position >= self.fail_at {
            return Err(io::Error::other("injected reader fault"));
        }
        let permitted = usize::try_from(self.fail_at - position).unwrap_or(usize::MAX);
        let length = buffer.len().min(permitted);
        self.inner.read(&mut buffer[..length])
    }
}

struct FaultAfterWriter {
    bytes: Vec<u8>,
    fail_at: usize,
}

impl FaultAfterWriter {
    const fn new(fail_at: usize) -> Self {
        Self {
            bytes: Vec::new(),
            fail_at,
        }
    }
}

impl Write for FaultAfterWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.bytes.len() >= self.fail_at {
            return Err(io::Error::other("injected writer fault"));
        }
        let permitted = self.fail_at - self.bytes.len();
        let length = buffer.len().min(permitted);
        self.bytes.extend_from_slice(&buffer[..length]);
        Ok(length)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn schema() -> Result<SchemaId, Box<dyn Error>> {
    Ok(SchemaId::from_str("latticeaxiom:schema/chunk-snapshot@1")?)
}

fn owner() -> Result<PackageName, Box<dyn Error>> {
    Ok(PackageName::from_str("terrenia")?)
}

fn version() -> Result<PayloadSchemaVersion, Box<dyn Error>> {
    Ok(PayloadSchemaVersion::new(1)?)
}

fn contract<'a>(
    schema: &'a SchemaId,
    owner: &'a PackageName,
) -> Result<SnapshotContract<'a>, Box<dyn Error>> {
    Ok(SnapshotContract::new(schema, owner, version()?))
}

fn raw_payload(bytes: Vec<u8>) -> Result<VersionedPayload, Box<dyn Error>> {
    Ok(VersionedPayload::new(schema()?, version()?, bytes))
}

fn sample_envelope() -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(encode_snapshot(
        &owner()?,
        &raw_payload(vec![1, 42])?,
        WorldWireLimits::default(),
    )?)
}

fn serialize_tagged_fixture<'a>(
    value: &TaggedPayloadV1,
    scratch: &'a mut [u8],
) -> WireResult<&'a [u8]> {
    postcard::to_slice(value, scratch)
        .map(|encoded| &*encoded)
        .map_err(|postcard_error| WorldWireError::PostcardEncode {
            reason: postcard_error.to_string(),
        })
}

fn decode_tagged_schema(payload: &[u8]) -> WireResult<TaggedPayloadV1> {
    let (decoded, remaining): (TaggedPayloadV1, &[u8]) = postcard::take_from_bytes(payload)
        .map_err(|postcard_error| WorldWireError::MalformedPayload {
            reason: postcard_error.to_string(),
        })?;
    if !remaining.is_empty() {
        return Err(WorldWireError::TrailingPayloadBytes {
            remaining: remaining.len(),
        });
    }
    require_numeric_tag("chunk.layer_kind", decoded.kind_tag, &[1])?;
    let mut canonical = [0_u8; 6];
    let canonical = serialize_tagged_fixture(&decoded, &mut canonical)?;
    if canonical != payload {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "payload differs from the unique postcard re-encoding".to_owned(),
        });
    }
    Ok(decoded)
}

#[test]
fn envelope_and_postcard_have_exact_v1_golden_bytes() -> Result<(), Box<dyn Error>> {
    let expected_dto = TaggedPayloadV1 {
        kind_tag: 1,
        value: 42,
    };
    let mut scratch = [0_u8; 16];
    let postcard = serialize_tagged_fixture(&expected_dto, &mut scratch)?;
    assert_eq!(postcard, [1, 42]);
    let mut exact_scratch = [0_u8; 2];
    assert_eq!(
        serialize_tagged_fixture(&expected_dto, &mut exact_scratch)?,
        [1, 42]
    );
    let mut short_scratch = [0_u8; 1];
    assert!(matches!(
        serialize_tagged_fixture(&expected_dto, &mut short_scratch),
        Err(WorldWireError::PostcardEncode { .. })
    ));

    let encoded = sample_envelope()?;
    assert_eq!(
        hex::encode(&encoded),
        concat!(
            "4c415857534e5000",
            "0001",
            "00246c6174746963656178696f6d3a736368656d612f6368756e6b2d736e617073686f744031",
            "000874657272656e6961",
            "00000001",
            "0000000000000002",
            "12a0f65cb25738c3251f2ddfab7129fb80de0f7f05e3e105ccac2f2b71076e9d",
            "012a"
        )
    );

    let schema = schema()?;
    let owner = owner()?;
    let envelope = preflight_snapshot_for_contract(
        &encoded,
        contract(&schema, &owner)?,
        WorldWireLimits::default(),
    )?;
    assert_eq!(
        decode_tagged_schema(envelope.payload_bytes())?,
        expected_dto
    );
    Ok(())
}
#[test]
fn writer_and_stream_reader_are_byte_equivalent() -> Result<(), Box<dyn Error>> {
    let schema = schema()?;
    let owner = owner()?;
    let payload = raw_payload(vec![1, 42])?;
    let expected = encode_snapshot(&owner, &payload, WorldWireLimits::default())?;
    let mut written = Vec::new();
    write_snapshot(&mut written, &owner, &payload, WorldWireLimits::default())?;
    assert_eq!(written, expected);

    let mut reader = Cursor::new(expected.clone());
    let decoded = read_snapshot(&mut reader, WorldWireLimits::default())?;
    decoded.validate_contract(contract(&schema, &owner)?)?;
    assert_eq!(decoded.owner(), &owner);
    assert_eq!(decoded.payload(), &payload);
    assert_eq!(
        decode_tagged_schema(decoded.payload().bytes())?,
        TaggedPayloadV1 {
            kind_tag: 1,
            value: 42
        }
    );
    Ok(())
}
#[test]
fn schema_postcard_and_stream_envelope_round_trip() -> Result<(), Box<dyn Error>> {
    let schema = schema()?;
    let owner = owner()?;
    let expected = TaggedPayloadV1 {
        kind_tag: 1,
        value: u32::MAX,
    };
    let mut scratch = [0_u8; 32];
    let payload = serialize_tagged_fixture(&expected, &mut scratch)?.to_vec();
    let encoded = encode_snapshot(
        &owner,
        &VersionedPayload::new(schema.clone(), version()?, payload),
        WorldWireLimits::default(),
    )?;
    let decoded = read_snapshot_for_contract(
        &mut Cursor::new(encoded),
        contract(&schema, &owner)?,
        WorldWireLimits::default(),
    )?;
    assert_eq!(decode_tagged_schema(decoded.payload().bytes())?, expected);
    Ok(())
}
#[test]
fn unknown_contracts_remain_structurally_valid_and_are_typed() -> Result<(), Box<dyn Error>> {
    let expected_schema = schema()?;
    let expected_owner = owner()?;
    let alternate_schema = SchemaId::from_str("other:schema/chunk-snapshot@1")?;
    let alternate_owner = PackageName::from_str("other")?;
    let bytes = vec![1, 42];
    let limits = WorldWireLimits::default();

    let wrong_schema = encode_snapshot(
        &expected_owner,
        &VersionedPayload::new(alternate_schema.clone(), version()?, bytes.clone()),
        limits,
    )?;
    let structural_schema = preflight_snapshot(&wrong_schema, limits)?;
    assert_eq!(structural_schema.schema(), &alternate_schema);
    assert_eq!(structural_schema.payload_bytes(), bytes);
    let opaque_schema = structural_schema.to_owned();
    assert_eq!(
        encode_snapshot(opaque_schema.owner(), opaque_schema.payload(), limits)?,
        wrong_schema
    );
    assert!(matches!(
        structural_schema.validate_contract(contract(&expected_schema, &expected_owner)?),
        Err(WorldWireError::UnknownSchema { found }) if found == alternate_schema
    ));
    let mut wrong_schema_stream = Cursor::new(wrong_schema);
    assert!(matches!(
        read_snapshot_for_contract(
            &mut wrong_schema_stream,
            contract(&expected_schema, &expected_owner)?,
            limits
        ),
        Err(WorldWireError::UnknownSchema { .. })
    ));
    assert_eq!(
        wrong_schema_stream.position(),
        18 + u64::try_from(alternate_schema.as_str().len())?
            + u64::try_from(expected_owner.as_str().len())?
    );

    let wrong_owner = encode_snapshot(&alternate_owner, &raw_payload(bytes.clone())?, limits)?;
    let structural_owner = preflight_snapshot(&wrong_owner, limits)?;
    assert_eq!(structural_owner.owner(), &alternate_owner);
    assert_eq!(structural_owner.payload_bytes(), bytes);
    assert!(matches!(
        structural_owner.validate_contract(contract(&expected_schema, &expected_owner)?),
        Err(WorldWireError::UnknownOwner { found }) if found == alternate_owner
    ));
    let mut wrong_owner_stream = Cursor::new(wrong_owner);
    assert!(matches!(
        read_snapshot_for_contract(
            &mut wrong_owner_stream,
            contract(&expected_schema, &expected_owner)?,
            limits
        ),
        Err(WorldWireError::UnknownOwner { .. })
    ));
    assert_eq!(
        wrong_owner_stream.position(),
        18 + u64::try_from(expected_schema.as_str().len())?
            + u64::try_from(alternate_owner.as_str().len())?
    );

    let version_two = PayloadSchemaVersion::new(2)?;
    let wrong_version = encode_snapshot(
        &expected_owner,
        &VersionedPayload::new(expected_schema.clone(), version_two, bytes.clone()),
        limits,
    )?;
    let structural_version = preflight_snapshot(&wrong_version, limits)?;
    assert_eq!(structural_version.schema_version(), version_two);
    assert_eq!(structural_version.payload_bytes(), bytes);
    assert!(matches!(
        structural_version.validate_contract(contract(&expected_schema, &expected_owner)?),
        Err(WorldWireError::UnsupportedSchemaVersion {
            found: 2,
            expected: 1
        })
    ));
    let mut wrong_version_stream = Cursor::new(wrong_version);
    assert!(matches!(
        read_snapshot_for_contract(
            &mut wrong_version_stream,
            contract(&expected_schema, &expected_owner)?,
            limits
        ),
        Err(WorldWireError::UnsupportedSchemaVersion { .. })
    ));
    assert_eq!(
        wrong_version_stream.position(),
        18 + u64::try_from(expected_schema.as_str().len())?
            + u64::try_from(expected_owner.as_str().len())?
    );
    Ok(())
}
#[test]
fn envelope_mutations_fail_closed() -> Result<(), Box<dyn Error>> {
    let schema = schema()?;
    let owner = owner()?;
    let contract = contract(&schema, &owner)?;
    let encoded = sample_envelope()?;

    let mut bad_magic = encoded.clone();
    bad_magic[0] ^= 1;
    assert!(matches!(
        preflight_snapshot_for_contract(&bad_magic, contract, WorldWireLimits::default()),
        Err(WorldWireError::InvalidSnapshotMagic { .. })
    ));

    let mut unknown_major = encoded.clone();
    unknown_major[9] = 2;
    assert!(matches!(
        preflight_snapshot_for_contract(&unknown_major, contract, WorldWireLimits::default()),
        Err(WorldWireError::UnsupportedEnvelopeMajor { found: 2 })
    ));

    let mut bad_digest = encoded.clone();
    let last_index = bad_digest.len() - 1;
    bad_digest[last_index] ^= 1;
    assert!(matches!(
        preflight_snapshot_for_contract(&bad_digest, contract, WorldWireLimits::default()),
        Err(WorldWireError::DigestMismatch { .. })
    ));

    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(matches!(
        preflight_snapshot_for_contract(&trailing, contract, WorldWireLimits::default()),
        Err(WorldWireError::PayloadLengthMismatch { .. })
    ));
    assert!(matches!(
        read_snapshot_for_contract(
            &mut Cursor::new(trailing),
            contract,
            WorldWireLimits::default()
        ),
        Err(WorldWireError::TrailingEnvelopeData)
    ));

    for length in 0..encoded.len() {
        assert!(
            preflight_snapshot_for_contract(
                &encoded[..length],
                contract,
                WorldWireLimits::default()
            )
            .is_err(),
            "truncated length {length} was accepted"
        );
    }
    Ok(())
}

#[test]
fn configured_limits_can_only_tighten_absolute_hard_ceilings() {
    assert!(matches!(
        WorldWireLimits::new(1_025, 1, 1, 1, 1),
        Err(WorldWireError::LimitExceedsHardMaximum {
            name: "max_dimension_id_bytes",
            value: 1_025,
            maximum: 1_024
        })
    ));
    assert!(matches!(
        WorldWireLimits::new(1, 1, 1, 1, (64 * 1024 * 1024) + 1),
        Err(WorldWireError::LimitExceedsHardMaximum {
            name: "max_snapshot_payload_bytes",
            value: 67_108_865,
            maximum: 67_108_864
        })
    ));
}

#[test]
fn malformed_schema_owner_and_zero_version_are_typed() -> Result<(), Box<dyn Error>> {
    let schema = schema()?;
    let owner = owner()?;
    let contract = contract(&schema, &owner)?;
    let encoded = sample_envelope()?;
    let schema_start = 8 + 2 + 2;
    let owner_length_offset = schema_start + schema.as_str().len();
    let owner_start = owner_length_offset + 2;
    let version_start = owner_start + owner.as_str().len();

    let mut invalid_schema_utf8 = encoded.clone();
    invalid_schema_utf8[schema_start] = u8::MAX;
    assert!(matches!(
        preflight_snapshot_for_contract(&invalid_schema_utf8, contract, WorldWireLimits::default()),
        Err(WorldWireError::InvalidUtf8 {
            segment: WireSegment::SchemaId
        })
    ));

    let mut invalid_schema = encoded.clone();
    invalid_schema[schema_start] = b'L';
    assert!(matches!(
        preflight_snapshot_for_contract(&invalid_schema, contract, WorldWireLimits::default()),
        Err(WorldWireError::InvalidSchemaId { .. })
    ));

    let mut invalid_owner = encoded.clone();
    invalid_owner[owner_start] = b'T';
    assert!(matches!(
        preflight_snapshot_for_contract(&invalid_owner, contract, WorldWireLimits::default()),
        Err(WorldWireError::InvalidSnapshotOwner { .. })
    ));

    let mut zero_version = encoded;
    zero_version[version_start..version_start + 4].fill(0);
    assert!(matches!(
        preflight_snapshot_for_contract(&zero_version, contract, WorldWireLimits::default()),
        Err(WorldWireError::InvalidSchemaVersion)
    ));
    Ok(())
}

#[test]
fn payload_and_metadata_ceilings_reject_boundary_plus_one_before_body_io()
-> Result<(), Box<dyn Error>> {
    let schema = schema()?;
    let owner = owner()?;
    let schema_length = u16::try_from(schema.as_str().len())?;
    let owner_length = u16::try_from(owner.as_str().len())?;
    let exact = WorldWireLimits::new(1, 1, schema_length, owner_length, 2)?;
    let payload = raw_payload(vec![1, 42])?;
    let encoded = encode_snapshot(&owner, &payload, exact)?;
    assert!(preflight_snapshot_for_contract(&encoded, contract(&schema, &owner)?, exact).is_ok());
    assert!(read_snapshot(&mut Cursor::new(encoded.clone()), exact).is_ok());

    let payload_short = WorldWireLimits::new(1, 1, schema_length, owner_length, 1)?;
    assert!(matches!(
        encode_snapshot(&owner, &payload, payload_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::SnapshotPayload,
            actual: 2,
            maximum: 1
        })
    ));
    let mut payload_writer = Vec::new();
    assert!(write_snapshot(&mut payload_writer, &owner, &payload, payload_short).is_err());
    assert!(payload_writer.is_empty());
    assert!(matches!(
        preflight_snapshot(&encoded, payload_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::SnapshotPayload,
            actual: 2,
            maximum: 1
        })
    ));
    let mut payload_stream = Cursor::new(encoded.clone());
    assert!(matches!(
        read_snapshot(&mut payload_stream, payload_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::SnapshotPayload,
            actual: 2,
            maximum: 1
        })
    ));
    let before_digest = 26_u64 + u64::from(schema_length) + u64::from(owner_length);
    assert_eq!(payload_stream.position(), before_digest);

    let schema_short = WorldWireLimits::new(1, 1, schema_length - 1, owner_length, 2)?;
    let mut schema_writer = Vec::new();
    assert!(write_snapshot(&mut schema_writer, &owner, &payload, schema_short).is_err());
    assert!(schema_writer.is_empty());
    assert!(matches!(
        preflight_snapshot(&encoded, schema_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::SchemaId,
            ..
        })
    ));
    let mut schema_stream = Cursor::new(encoded.clone());
    assert!(matches!(
        read_snapshot(&mut schema_stream, schema_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::SchemaId,
            ..
        })
    ));
    assert_eq!(schema_stream.position(), 12);

    let owner_short = WorldWireLimits::new(1, 1, schema_length, owner_length - 1, 2)?;
    let mut owner_writer = Vec::new();
    assert!(write_snapshot(&mut owner_writer, &owner, &payload, owner_short).is_err());
    assert!(owner_writer.is_empty());
    assert!(matches!(
        preflight_snapshot(&encoded, owner_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::SnapshotOwner,
            ..
        })
    ));
    let mut owner_stream = Cursor::new(encoded);
    assert!(matches!(
        read_snapshot(&mut owner_stream, owner_short),
        Err(WorldWireError::SegmentTooLong {
            segment: WireSegment::SnapshotOwner,
            ..
        })
    ));
    assert_eq!(owner_stream.position(), 14 + u64::from(schema_length));
    Ok(())
}

#[test]
fn stream_truncation_and_fault_injection_are_typed() -> Result<(), Box<dyn Error>> {
    let encoded = sample_envelope()?;
    for length in 0..encoded.len() {
        let mut reader = Cursor::new(&encoded[..length]);
        assert!(
            matches!(
                read_snapshot(&mut reader, WorldWireLimits::default()),
                Err(WorldWireError::TruncatedStream { .. })
            ),
            "stream truncation at length {length} was not typed"
        );
    }

    let schema_length = u64::try_from(schema()?.as_str().len())?;
    let owner_length = u64::try_from(owner()?.as_str().len())?;
    let digest_start = 26 + schema_length + owner_length;
    let mut faulty = FaultAfter::new(encoded, digest_start + 5);
    assert!(matches!(
        read_snapshot(&mut faulty, WorldWireLimits::default()),
        Err(WorldWireError::Io {
            section: "payload digest",
            ..
        })
    ));
    assert_eq!(faulty.position(), digest_start + 5);

    let payload = raw_payload(vec![1, 42])?;
    let mut faulty_writer = FaultAfterWriter::new(usize::try_from(digest_start)? + 5);
    assert!(matches!(
        write_snapshot(
            &mut faulty_writer,
            &owner()?,
            &payload,
            WorldWireLimits::default()
        ),
        Err(WorldWireError::Io {
            section: "payload digest",
            ..
        })
    ));
    assert_eq!(
        faulty_writer.bytes.len(),
        usize::try_from(digest_start)? + 5
    );
    Ok(())
}

#[test]
fn schema_decoder_rejects_unknown_tags_trailing_and_malformed_payloads()
-> Result<(), Box<dyn Error>> {
    let schema = schema()?;
    let owner = owner()?;
    let limits = WorldWireLimits::default();
    let mut scratch = [0_u8; 16];
    let unknown_payload = serialize_tagged_fixture(
        &TaggedPayloadV1 {
            kind_tag: 99,
            value: 7,
        },
        &mut scratch,
    )?;
    let encoded = encode_snapshot(
        &owner,
        &VersionedPayload::new(schema.clone(), version()?, unknown_payload.to_vec()),
        limits,
    )?;
    let envelope = preflight_snapshot_for_contract(&encoded, contract(&schema, &owner)?, limits)?;
    assert!(matches!(
        decode_tagged_schema(envelope.payload_bytes()),
        Err(WorldWireError::UnknownNumericTag {
            field: "chunk.layer_kind",
            found: 99
        })
    ));

    let extra_field = encode_snapshot(&owner, &raw_payload(vec![1, 42, 0])?, limits)?;
    let envelope =
        preflight_snapshot_for_contract(&extra_field, contract(&schema, &owner)?, limits)?;
    assert!(matches!(
        decode_tagged_schema(envelope.payload_bytes()),
        Err(WorldWireError::TrailingPayloadBytes { remaining: 1 })
    ));

    let malformed = encode_snapshot(&owner, &raw_payload(vec![0x80])?, limits)?;
    let envelope = preflight_snapshot_for_contract(&malformed, contract(&schema, &owner)?, limits)?;
    assert!(matches!(
        decode_tagged_schema(envelope.payload_bytes()),
        Err(WorldWireError::MalformedPayload { .. })
    ));
    Ok(())
}
#[test]
fn fuzz_like_slice_stream_and_postcard_corpora_never_panic() -> Result<(), Box<dyn Error>> {
    let encoded = sample_envelope()?;

    let mut corpus = Vec::new();
    corpus.push(Vec::new());
    for length in 0..encoded.len() {
        corpus.push(encoded[..length].to_vec());
    }
    for index in 0..encoded.len() {
        let mut mutation = encoded.clone();
        mutation[index] ^= 0x5a;
        corpus.push(mutation);
    }
    corpus.push(vec![u8::MAX; 512]);

    for candidate in corpus {
        let outcome = std::panic::catch_unwind(|| {
            let _ = preflight_snapshot(&candidate, WorldWireLimits::default());
            let _ = read_snapshot(&mut Cursor::new(candidate), WorldWireLimits::default());
        });
        assert!(outcome.is_ok());
    }

    let schema = schema()?;
    let owner = owner()?;
    let contract = contract(&schema, &owner)?;
    for length in 0_u8..=127 {
        let payload = (0..length)
            .map(|index| index.wrapping_mul(73).wrapping_add(length))
            .collect::<Vec<_>>();
        let encoded = encode_snapshot(
            &owner,
            &VersionedPayload::new(schema.clone(), version()?, payload),
            WorldWireLimits::default(),
        )?;
        let outcome = std::panic::catch_unwind(|| {
            if let Ok(envelope) =
                preflight_snapshot_for_contract(&encoded, contract, WorldWireLimits::default())
            {
                let _ = decode_tagged_schema(envelope.payload_bytes());
            }
        });
        assert!(outcome.is_ok());
    }
    Ok(())
}
