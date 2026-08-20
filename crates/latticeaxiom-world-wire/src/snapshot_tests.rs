use std::{
    cell::Cell, error::Error, io::Cursor, num::NonZeroU16, num::NonZeroU32, num::NonZeroU64,
    str::FromStr,
};

use latticeaxiom_core::{PackageName, SchemaId};
use latticeaxiom_storage::{PayloadSchemaVersion, VersionedPayload};
use serde::{Deserialize, Serialize};

use super::{
    SnapshotContract, SnapshotSchemaCodec, ValidatedSnapshotPayload, codec_seal,
    decode_typed_snapshot, encode_snapshot, encode_typed_snapshot, read_typed_snapshot,
    require_numeric_tag,
};
use crate::{
    SnapshotCodecLimits, SnapshotCodecResource, WireResult, WorldWireError, WorldWireLimits,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RegisteredSnapshotV1 {
    kind_tag: u16,
    values: Vec<u32>,
}

struct RegisteredSnapshotCodecV1 {
    schema: SchemaId,
    owner: PackageName,
    version: PayloadSchemaVersion,
    limits: SnapshotCodecLimits,
    postcard_decode_calls: Cell<u32>,
}

impl RegisteredSnapshotCodecV1 {
    fn new(max_payload_bytes: u64) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            schema: SchemaId::from_str("latticeaxiom:schema/registered-snapshot@1")?,
            owner: PackageName::from_str("terrenia")?,
            version: PayloadSchemaVersion::new(1)?,
            limits: SnapshotCodecLimits::new(
                NonZeroU64::new(max_payload_bytes)
                    .ok_or("test codec payload limit must be non-zero")?,
                NonZeroU32::new(4).expect("test codec collection limit is non-zero"),
                NonZeroU16::new(2).expect("test codec nesting limit is non-zero"),
            ),
            postcard_decode_calls: Cell::new(0),
        })
    }

    fn decode_calls(&self) -> u32 {
        self.postcard_decode_calls.get()
    }

    fn preflight_payload(payload: &[u8], limits: SnapshotCodecLimits) -> WireResult<()> {
        let (&tag, remainder) =
            payload
                .split_first()
                .ok_or_else(|| WorldWireError::MalformedPayload {
                    reason: "missing fixed snapshot kind tag".to_owned(),
                })?;
        require_numeric_tag("snapshot.kind", u16::from(tag), &[1])?;
        let count = preflight_varint(remainder)?;
        limits.check_collection_elements(count)?;
        limits.check_nesting_depth(1)
    }
}

impl codec_seal::Sealed for RegisteredSnapshotCodecV1 {}

impl SnapshotSchemaCodec for RegisteredSnapshotCodecV1 {
    type Value = RegisteredSnapshotV1;

    fn contract(&self) -> SnapshotContract<'_> {
        SnapshotContract::new(&self.schema, &self.owner, self.version)
    }

    fn limits(&self) -> SnapshotCodecLimits {
        self.limits
    }

    fn validate_for_encode(
        &self,
        value: &Self::Value,
        limits: SnapshotCodecLimits,
    ) -> WireResult<()> {
        require_numeric_tag("snapshot.kind", value.kind_tag, &[1])?;
        limits.check_collection_elements(u64::try_from(value.values.len()).unwrap_or(u64::MAX))?;
        limits.check_nesting_depth(1)?;
        if value.values.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "values must be sorted by their stable numeric key".to_owned(),
            });
        }
        Ok(())
    }

    fn decode_postcard_bounded(
        &self,
        payload: ValidatedSnapshotPayload<'_>,
    ) -> WireResult<Self::Value> {
        let limits = payload.limits();
        let payload = payload.bytes();
        Self::preflight_payload(payload, limits)?;
        self.postcard_decode_calls
            .set(self.postcard_decode_calls.get() + 1);
        let (decoded, remaining) = postcard::take_from_bytes(payload).map_err(|error| {
            WorldWireError::MalformedPayload {
                reason: error.to_string(),
            }
        })?;
        if !remaining.is_empty() {
            return Err(WorldWireError::TrailingPayloadBytes {
                remaining: remaining.len(),
            });
        }
        self.validate_for_encode(&decoded, limits)?;
        let mut canonical_scratch = [0_u8; 32];
        let canonical = postcard::to_slice(&decoded, &mut canonical_scratch).map_err(|error| {
            WorldWireError::PostcardEncode {
                reason: error.to_string(),
            }
        })?;
        if canonical != payload {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "payload differs from the unique postcard re-encoding".to_owned(),
            });
        }
        Ok(decoded)
    }
}

fn preflight_varint(payload: &[u8]) -> WireResult<u64> {
    let mut value = 0_u64;
    for (index, byte) in payload.iter().copied().enumerate().take(10) {
        let low = u64::from(byte & 0x7f);
        if index == 9 && low > 1 {
            return Err(WorldWireError::MalformedPayload {
                reason: "collection length varint overflows u64".to_owned(),
            });
        }
        let shift = u32::try_from(index * 7).unwrap_or(u32::MAX);
        value |= low
            .checked_shl(shift)
            .ok_or_else(|| WorldWireError::MalformedPayload {
                reason: "collection length varint shift overflow".to_owned(),
            })?;
        if byte & 0x80 == 0 {
            if index > 0 && low == 0 {
                return Err(WorldWireError::NonCanonicalPayload {
                    reason: "collection length uses a redundant varint byte".to_owned(),
                });
            }
            return Ok(value);
        }
    }
    Err(WorldWireError::MalformedPayload {
        reason: "unterminated collection length varint".to_owned(),
    })
}

fn envelope(codec: &RegisteredSnapshotCodecV1, payload: Vec<u8>) -> WireResult<Vec<u8>> {
    let contract = codec.contract();
    encode_snapshot(
        contract.owner(),
        &VersionedPayload::new(contract.schema().clone(), contract.version(), payload),
        WorldWireLimits::default(),
    )
}

#[test]
fn sealed_codec_round_trips_and_rejects_noncanonical_values() -> Result<(), Box<dyn Error>> {
    let codec = RegisteredSnapshotCodecV1::new(32)?;
    let expected = RegisteredSnapshotV1 {
        kind_tag: 1,
        values: vec![1, 2, 9],
    };
    let mut scratch = [0_u8; 32];
    let encoded =
        encode_typed_snapshot(&codec, &expected, &mut scratch, WorldWireLimits::default())?;
    assert_eq!(
        decode_typed_snapshot(&encoded, &codec, WorldWireLimits::default())?,
        expected
    );
    assert_eq!(
        read_typed_snapshot(
            &mut Cursor::new(encoded),
            &codec,
            WorldWireLimits::default(),
        )?,
        expected
    );

    let before = [0xa5_u8; 32];
    let mut untouched = before;
    assert!(matches!(
        encode_typed_snapshot(
            &codec,
            &RegisteredSnapshotV1 {
                kind_tag: 99,
                values: Vec::new(),
            },
            &mut untouched,
            WorldWireLimits::default(),
        ),
        Err(WorldWireError::UnknownNumericTag {
            field: "snapshot.kind",
            found: 99
        })
    ));
    assert_eq!(untouched, before);

    let unsorted = RegisteredSnapshotV1 {
        kind_tag: 1,
        values: vec![2, 1],
    };
    assert!(matches!(
        encode_typed_snapshot(&codec, &unsorted, &mut scratch, WorldWireLimits::default(),),
        Err(WorldWireError::NonCanonicalPayload { .. })
    ));
    Ok(())
}

#[test]
fn contract_and_budgets_reject_before_postcard_decode() -> Result<(), Box<dyn Error>> {
    let codec = RegisteredSnapshotCodecV1::new(4)?;
    let limits = WorldWireLimits::default();

    let oversized = envelope(&codec, vec![1, 1, 0, 0, 0])?;
    assert!(matches!(
        decode_typed_snapshot(&oversized, &codec, limits),
        Err(WorldWireError::CodecBudgetExceeded {
            resource: SnapshotCodecResource::PayloadBytes,
            actual: 5,
            maximum: 4
        })
    ));
    assert_eq!(codec.decode_calls(), 0);
    let mut oversized_stream = Cursor::new(oversized);
    assert!(matches!(
        read_typed_snapshot(&mut oversized_stream, &codec, limits),
        Err(WorldWireError::CodecBudgetExceeded {
            resource: SnapshotCodecResource::PayloadBytes,
            ..
        })
    ));
    let contract = codec.contract();
    let before_digest = 26_u64
        + u64::try_from(contract.schema().as_str().len())?
        + u64::try_from(contract.owner().as_str().len())?;
    assert_eq!(oversized_stream.position(), before_digest);
    assert_eq!(codec.decode_calls(), 0);

    let alternate_schema = SchemaId::from_str("other:schema/registered-snapshot@1")?;
    let wrong_contract = encode_snapshot(
        contract.owner(),
        &VersionedPayload::new(alternate_schema, contract.version(), vec![1, 0]),
        limits,
    )?;
    assert!(matches!(
        decode_typed_snapshot(&wrong_contract, &codec, limits),
        Err(WorldWireError::UnknownSchema { .. })
    ));
    assert_eq!(codec.decode_calls(), 0);

    let unbounded_collection = envelope(&codec, vec![1, 0x80, 0x01])?;
    assert!(matches!(
        decode_typed_snapshot(&unbounded_collection, &codec, limits),
        Err(WorldWireError::CodecBudgetExceeded {
            resource: SnapshotCodecResource::CollectionElements,
            actual: 128,
            maximum: 4
        })
    ));
    assert_eq!(codec.decode_calls(), 0);

    let malformed_collection = envelope(&codec, vec![1, 0x80])?;
    assert!(matches!(
        decode_typed_snapshot(&malformed_collection, &codec, limits),
        Err(WorldWireError::MalformedPayload { .. })
    ));
    assert_eq!(codec.decode_calls(), 0);

    let noncanonical_collection = envelope(&codec, vec![1, 0x80, 0])?;
    assert!(matches!(
        decode_typed_snapshot(&noncanonical_collection, &codec, limits),
        Err(WorldWireError::NonCanonicalPayload { .. })
    ));
    assert_eq!(codec.decode_calls(), 0);
    Ok(())
}
