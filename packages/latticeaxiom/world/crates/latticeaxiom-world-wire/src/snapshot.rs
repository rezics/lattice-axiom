use std::io::{ErrorKind, Read, Write};

use latticeaxiom_core::{PackageName, SchemaId};
use latticeaxiom_storage::{PayloadSchemaVersion, VersionedPayload};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    SNAPSHOT_ENVELOPE_MAJOR, SNAPSHOT_MAGIC, SnapshotCodecLimits, WireResult, WireSegment,
    WorldWireError, WorldWireLimits,
};

const MAGIC_BYTES: usize = 8;
const MAJOR_BYTES: usize = 2;
const SEGMENT_LENGTH_BYTES: usize = 2;
const SCHEMA_VERSION_BYTES: usize = 4;
const PAYLOAD_LENGTH_BYTES: usize = 8;
const DIGEST_BYTES: usize = 32;

/// Exact schema-owner-version contract for one postcard decoder.
///
/// Every schema version receives an independent decoder and golden corpus; this
/// type deliberately cannot claim that one Rust DTO decodes a version range.
#[derive(Clone, Copy, Debug)]
pub struct SnapshotContract<'a> {
    schema: &'a SchemaId,
    owner: &'a PackageName,
    version: PayloadSchemaVersion,
}

impl<'a> SnapshotContract<'a> {
    /// Declares the only schema, owner, and version this decoder accepts.
    #[must_use]
    pub const fn new(
        schema: &'a SchemaId,
        owner: &'a PackageName,
        version: PayloadSchemaVersion,
    ) -> Self {
        Self {
            schema,
            owner,
            version,
        }
    }

    /// Returns the expected schema identity.
    #[must_use]
    pub const fn schema(self) -> &'a SchemaId {
        self.schema
    }

    /// Returns the expected package owner.
    #[must_use]
    pub const fn owner(self) -> &'a PackageName {
        self.owner
    }

    /// Returns the exact schema version owned by this decoder.
    #[must_use]
    pub const fn version(self) -> PayloadSchemaVersion {
        self.version
    }
}

pub(crate) mod codec_seal {
    pub trait Sealed {}
}

/// Receipt proving envelope integrity, exact contract, and payload-byte budget.
///
/// Only the world-wire typed entry points can construct this receipt. A sealed
/// codec therefore cannot be invoked on opaque or mismatched envelope bytes.
#[derive(Clone, Copy, Debug)]
pub struct ValidatedSnapshotPayload<'a> {
    bytes: &'a [u8],
    limits: SnapshotCodecLimits,
}

impl<'a> ValidatedSnapshotPayload<'a> {
    const fn new(bytes: &'a [u8], limits: SnapshotCodecLimits) -> Self {
        Self { bytes, limits }
    }

    /// Returns the digest-verified postcard bytes for the exact codec contract.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// Returns the fixed schema-owned budgets attached before decoding.
    #[must_use]
    pub const fn limits(self) -> SnapshotCodecLimits {
        self.limits
    }
}

/// Audited postcard codec for exactly one snapshot schema-owner-version.
///
/// This trait is sealed: arbitrary serde types cannot claim to be authoritative
/// wire DTOs. Only implementations registered in this crate can exist; each
/// must use fixed-width/domain numeric types, stable collection order, explicit
/// numeric tags, normalized finite floats when applicable, and schema-specific
/// decode preflights that enforce [`SnapshotCodecLimits`] before allocation or
/// descent.
pub trait SnapshotSchemaCodec: codec_seal::Sealed {
    /// The one audited DTO owned by this exact codec contract.
    type Value: Serialize;

    /// Returns the exact schema, owner, and version accepted by this codec.
    fn contract(&self) -> SnapshotContract<'_>;

    /// Returns fixed payload, collection, and nesting ceilings for this codec.
    fn limits(&self) -> SnapshotCodecLimits;

    /// Validates canonical semantic restrictions before postcard serialization.
    ///
    /// # Errors
    ///
    /// Returns a typed tag, canonicality, or schema-budget error before bytes
    /// are produced.
    fn validate_for_encode(
        &self,
        value: &Self::Value,
        limits: SnapshotCodecLimits,
    ) -> WireResult<()>;

    /// Decodes one exact postcard payload with schema-owned preflight budgets.
    ///
    /// Implementations must reject unknown tags and malformed or non-canonical
    /// encodings, consume every payload byte, and check collection counts and
    /// nesting depth before allocating elements or descending recursively.
    ///
    /// # Errors
    ///
    /// Returns a typed malformed, trailing, tag, canonicality, or budget error.
    fn decode_postcard_bounded(
        &self,
        payload: ValidatedSnapshotPayload<'_>,
    ) -> WireResult<Self::Value>;
}

/// Structurally validated envelope borrowing its exact opaque payload bytes.
///
/// This type deliberately has no serde implementation: the outer envelope has
/// a hand-written byte contract. Use [`decode_typed_snapshot`] so DTO decoding
/// cannot bypass exact schema-owner-version validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BorrowedSnapshotEnvelope<'a> {
    owner: PackageName,
    schema: SchemaId,
    schema_version: PayloadSchemaVersion,
    payload_bytes: &'a [u8],
    payload_digest: [u8; 32],
}

impl<'a> BorrowedSnapshotEnvelope<'a> {
    /// Returns the canonical package owner.
    #[must_use]
    pub const fn owner(&self) -> &PackageName {
        &self.owner
    }

    /// Returns the canonical schema identity.
    #[must_use]
    pub const fn schema(&self) -> &SchemaId {
        &self.schema
    }

    /// Returns the positive owner-controlled schema version.
    #[must_use]
    pub const fn schema_version(&self) -> PayloadSchemaVersion {
        self.schema_version
    }

    /// Returns the exact uncompressed opaque payload bytes.
    #[must_use]
    pub const fn payload_bytes(&self) -> &'a [u8] {
        self.payload_bytes
    }

    /// Returns the verified SHA-256 digest carried by the envelope.
    #[must_use]
    pub const fn payload_digest(&self) -> [u8; 32] {
        self.payload_digest
    }

    /// Validates this structural envelope against one exact decoder contract.
    ///
    /// # Errors
    ///
    /// Returns a typed unknown-schema, unknown-owner, or unsupported-version
    /// error without altering the opaque payload bytes.
    pub fn validate_contract(&self, contract: SnapshotContract<'_>) -> WireResult<()> {
        validate_contract(&self.schema, &self.owner, self.schema_version, contract)
    }

    /// Copies this already bounded envelope into the shared opaque payload type.
    #[must_use]
    pub fn to_owned(&self) -> OwnedSnapshotEnvelope {
        OwnedSnapshotEnvelope {
            owner: self.owner.clone(),
            payload: VersionedPayload::new(
                self.schema.clone(),
                self.schema_version,
                self.payload_bytes.to_vec(),
            ),
            payload_digest: self.payload_digest,
        }
    }
}

/// Structurally validated envelope owning its exact opaque payload bytes.
///
/// This type deliberately reuses [`VersionedPayload`] rather than creating a
/// second schema/version/byte representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedSnapshotEnvelope {
    owner: PackageName,
    payload: VersionedPayload,
    payload_digest: [u8; 32],
}

impl OwnedSnapshotEnvelope {
    /// Returns the canonical package owner.
    #[must_use]
    pub const fn owner(&self) -> &PackageName {
        &self.owner
    }

    /// Returns the shared versioned payload.
    #[must_use]
    pub const fn payload(&self) -> &VersionedPayload {
        &self.payload
    }

    /// Returns the verified SHA-256 digest carried by the envelope.
    #[must_use]
    pub const fn payload_digest(&self) -> [u8; 32] {
        self.payload_digest
    }

    /// Validates this structural envelope against one exact decoder contract.
    ///
    /// # Errors
    ///
    /// Returns a typed unknown-schema, unknown-owner, or unsupported-version
    /// error without altering the opaque payload bytes.
    pub fn validate_contract(&self, contract: SnapshotContract<'_>) -> WireResult<()> {
        validate_contract(
            self.payload.schema(),
            &self.owner,
            self.payload.schema_version(),
            contract,
        )
    }
}

/// Encodes caller-asserted schema-owned payload bytes into the exact envelope.
///
/// This structural encoder does not interpret or validate schema canonicality.
///
/// # Errors
///
/// Returns a typed preflight error before allocating when a metadata segment or
/// payload exceeds its hard ceiling, or an I/O error if the in-memory writer
/// unexpectedly fails.
pub fn encode_snapshot(
    owner: &PackageName,
    payload: &VersionedPayload,
    limits: WorldWireLimits,
) -> WireResult<Vec<u8>> {
    preflight_encode(owner, payload, limits)?;
    let capacity = encoded_length(owner, payload)?;
    let mut output = Vec::with_capacity(capacity);
    write_snapshot(&mut output, owner, payload, limits)?;
    Ok(output)
}

/// Writes caller-asserted schema-owned payload bytes using the exact envelope.
///
/// This structural writer does not interpret or validate schema canonicality.
///
/// Callers are responsible for staging output before publication; a generic
/// writer cannot promise atomic external visibility.
///
/// # Errors
///
/// Returns a typed preflight error before the first write when a metadata
/// segment or payload exceeds its hard ceiling, or a typed I/O error.
pub fn write_snapshot<W: Write>(
    writer: &mut W,
    owner: &PackageName,
    payload: &VersionedPayload,
    limits: WorldWireLimits,
) -> WireResult<()> {
    preflight_encode(owner, payload, limits)?;
    let schema = payload.schema().as_str().as_bytes();
    let owner = owner.as_str().as_bytes();
    let schema_length = wire_u16_length(WireSegment::SchemaId, schema.len())?;
    let owner_length = wire_u16_length(WireSegment::SnapshotOwner, owner.len())?;
    let payload_length =
        u64::try_from(payload.bytes().len()).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    let digest = digest(payload.bytes());

    write_all(writer, &SNAPSHOT_MAGIC, "snapshot magic")?;
    write_all(
        writer,
        &SNAPSHOT_ENVELOPE_MAJOR.to_be_bytes(),
        "snapshot envelope major",
    )?;
    write_all(writer, &schema_length.to_be_bytes(), "schema ID length")?;
    write_all(writer, schema, "schema ID")?;
    write_all(writer, &owner_length.to_be_bytes(), "snapshot owner length")?;
    write_all(writer, owner, "snapshot owner")?;
    write_all(
        writer,
        &payload.schema_version().get().to_be_bytes(),
        "schema version",
    )?;
    write_all(
        writer,
        &payload_length.to_be_bytes(),
        "uncompressed payload length",
    )?;
    write_all(writer, &digest, "payload digest")?;
    write_all(writer, payload.bytes(), "snapshot payload")
}

/// Structurally validates a complete envelope slice before any postcard decode.
///
/// Metadata lengths and the declared payload length are checked against hard
/// ceilings before parsing identities, hashing payload bytes, or constructing
/// a DTO.
///
/// # Errors
///
/// Returns a typed error for bad magic, an unsupported major, malformed
/// schema/owner/version fields, oversize segments, length mismatch, or digest
/// mismatch.
pub fn preflight_snapshot(
    encoded: &[u8],
    limits: WorldWireLimits,
) -> WireResult<BorrowedSnapshotEnvelope<'_>> {
    let mut cursor = SliceCursor::new(encoded);
    let magic = cursor.read_array::<8>("snapshot magic")?;
    if magic != SNAPSHOT_MAGIC {
        return Err(WorldWireError::InvalidSnapshotMagic { found: magic });
    }
    let major = cursor.read_u16("snapshot envelope major")?;
    if major != SNAPSHOT_ENVELOPE_MAJOR {
        return Err(WorldWireError::UnsupportedEnvelopeMajor { found: major });
    }
    let schema_text = cursor.read_text_segment(
        limits,
        WireSegment::SchemaId,
        "schema ID length",
        "schema ID",
    )?;
    let schema = parse_schema(schema_text)?;
    let owner_text = cursor.read_text_segment(
        limits,
        WireSegment::SnapshotOwner,
        "snapshot owner length",
        "snapshot owner",
    )?;
    let owner = parse_owner(owner_text)?;
    let schema_version = parse_schema_version(cursor.read_u32("schema version")?)?;
    let payload_length = cursor.read_u64("uncompressed payload length")?;
    limits.check_u64(WireSegment::SnapshotPayload, payload_length)?;
    let expected_digest = cursor.read_array::<32>("payload digest")?;
    let actual_remaining =
        u64::try_from(cursor.remaining()).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    if actual_remaining != payload_length {
        return Err(WorldWireError::PayloadLengthMismatch {
            declared: payload_length,
            actual: actual_remaining,
        });
    }
    let payload_bytes = cursor.read(cursor.remaining(), "snapshot payload")?;
    let actual_digest = digest(payload_bytes);
    if actual_digest != expected_digest {
        return Err(WorldWireError::DigestMismatch {
            expected: expected_digest,
            actual: actual_digest,
        });
    }
    Ok(BorrowedSnapshotEnvelope {
        owner,
        schema,
        schema_version,
        payload_bytes,
        payload_digest: expected_digest,
    })
}

/// Structurally validates a slice and then checks one exact decoder contract.
///
/// Structural validation, including the payload digest, completes even when the
/// contract is unknown so callers can safely retain opaque read-only bytes.
///
/// # Errors
///
/// Returns any structural error or a typed contract mismatch.
pub fn preflight_snapshot_for_contract<'a>(
    encoded: &'a [u8],
    contract: SnapshotContract<'_>,
    limits: WorldWireLimits,
) -> WireResult<BorrowedSnapshotEnvelope<'a>> {
    let envelope = preflight_snapshot(encoded, limits)?;
    envelope.validate_contract(contract)?;
    Ok(envelope)
}

/// Reads and structurally validates one bounded envelope from a stream.
///
/// Variable-length metadata is allocated only after its `u16` length passes
/// policy. The payload is allocated only after its `u64` length passes policy
/// and converts to `usize`. No postcard DTO is decoded by this function.
///
/// `reader` must be finite and contain exactly one envelope. This function reads
/// once beyond the declared payload to require EOF and reject trailing data; a
/// multiplexed or long-lived source must first be bounded to one complete frame.
///
/// # Errors
///
/// Returns a typed preflight, integrity, trailing-data, or I/O error.
pub fn read_snapshot<R: Read>(
    reader: &mut R,
    limits: WorldWireLimits,
) -> WireResult<OwnedSnapshotEnvelope> {
    read_snapshot_inner(reader, None, None, limits)
}

/// Reads one bounded envelope and rejects an unknown decoder contract before
/// allocating the payload.
///
/// This fast path consumes the header before returning a contract error and
/// cannot rewind a non-seek reader. Unknown or opaque input must therefore use
/// [`read_snapshot`] so its verified bytes can be retained read-only. As with
/// [`read_snapshot`], the reader must be a finite, exactly one-envelope frame.
///
/// # Errors
///
/// Returns a typed structural, contract, integrity, trailing-data, or I/O error.
pub fn read_snapshot_for_contract<R: Read>(
    reader: &mut R,
    contract: SnapshotContract<'_>,
    limits: WorldWireLimits,
) -> WireResult<OwnedSnapshotEnvelope> {
    read_snapshot_inner(reader, Some(contract), None, limits)
}

fn read_snapshot_inner<R: Read>(
    reader: &mut R,
    contract: Option<SnapshotContract<'_>>,
    codec_limits: Option<SnapshotCodecLimits>,
    limits: WorldWireLimits,
) -> WireResult<OwnedSnapshotEnvelope> {
    let magic = read_array::<8, _>(reader, "snapshot magic")?;
    if magic != SNAPSHOT_MAGIC {
        return Err(WorldWireError::InvalidSnapshotMagic { found: magic });
    }
    let major = read_u16(reader, "snapshot envelope major")?;
    if major != SNAPSHOT_ENVELOPE_MAJOR {
        return Err(WorldWireError::UnsupportedEnvelopeMajor { found: major });
    }
    let schema_text = read_text_segment(
        reader,
        limits,
        WireSegment::SchemaId,
        "schema ID length",
        "schema ID",
    )?;
    let schema = parse_schema(&schema_text)?;
    let owner_text = read_text_segment(
        reader,
        limits,
        WireSegment::SnapshotOwner,
        "snapshot owner length",
        "snapshot owner",
    )?;
    let owner = parse_owner(&owner_text)?;
    let schema_version = parse_schema_version(read_u32(reader, "schema version")?)?;
    if let Some(contract) = contract {
        validate_contract(&schema, &owner, schema_version, contract)?;
    }
    let payload_length = read_u64(reader, "uncompressed payload length")?;
    limits.check_u64(WireSegment::SnapshotPayload, payload_length)?;
    let payload_length =
        usize::try_from(payload_length).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    if let Some(codec_limits) = codec_limits {
        codec_limits.check_payload_bytes(payload_length)?;
    }
    let expected_digest = read_array::<32, _>(reader, "payload digest")?;
    let mut payload_bytes = vec![0_u8; payload_length];
    read_exact(reader, &mut payload_bytes, "snapshot payload")?;
    reject_stream_trailing_data(reader)?;
    let actual_digest = digest(&payload_bytes);
    if actual_digest != expected_digest {
        return Err(WorldWireError::DigestMismatch {
            expected: expected_digest,
            actual: actual_digest,
        });
    }
    Ok(OwnedSnapshotEnvelope {
        owner,
        payload: VersionedPayload::new(schema, schema_version, payload_bytes),
        payload_digest: expected_digest,
    })
}

/// Decodes a complete slice with one sealed schema codec after preflight.
///
/// Envelope integrity and the exact schema-owner-version contract are validated
/// before the codec can allocate or recurse. The codec's payload ceiling is
/// checked before decoding; its sealed implementation owns element/depth
/// preflights and canonical DTO validation.
///
/// # Errors
///
/// Returns any envelope, contract, schema-budget, malformed, trailing, tag, or
/// canonicality error.
pub fn decode_typed_snapshot<C: SnapshotSchemaCodec>(
    encoded: &[u8],
    codec: &C,
    limits: WorldWireLimits,
) -> WireResult<C::Value> {
    let contract = codec.contract();
    let envelope = preflight_snapshot_for_contract(encoded, contract, limits)?;
    let codec_limits = codec.limits();
    codec_limits.check_payload_bytes(envelope.payload_bytes().len())?;
    codec.decode_postcard_bounded(ValidatedSnapshotPayload::new(
        envelope.payload_bytes(),
        codec_limits,
    ))
}

/// Reads a bounded envelope and decodes it with one sealed schema codec.
///
/// Contract and schema-specific payload ceilings are checked before streaming
/// payload allocation. Unknown or opaque inputs must instead use
/// [`read_snapshot`], because this fast path consumes the header before returning
/// a contract error on a non-seek reader. The reader must be a finite, exactly
/// one-envelope frame. The sealed codec owns element/depth preflights.
///
/// # Errors
///
/// Returns any stream, contract, integrity, schema-budget, malformed, trailing,
/// tag, or canonicality error.
pub fn read_typed_snapshot<C: SnapshotSchemaCodec, R: Read>(
    reader: &mut R,
    codec: &C,
    limits: WorldWireLimits,
) -> WireResult<C::Value> {
    let contract = codec.contract();
    let codec_limits = codec.limits();
    let envelope = read_snapshot_inner(reader, Some(contract), Some(codec_limits), limits)?;
    codec.decode_postcard_bounded(ValidatedSnapshotPayload::new(
        envelope.payload().bytes(),
        codec_limits,
    ))
}

/// Encodes one value with its sealed schema-owned postcard codec and envelope.
///
/// Metadata and schema-specific resource rules are checked before postcard
/// serialization. The caller-owned scratch slice is tightened by both the outer
/// wire ceiling and the codec's fixed payload ceiling before serialization.
///
/// # Errors
///
/// Returns a typed metadata, schema-budget, canonicality, tag, postcard, or
/// envelope error.
pub fn encode_typed_snapshot<C: SnapshotSchemaCodec>(
    codec: &C,
    value: &C::Value,
    scratch: &mut [u8],
    limits: WorldWireLimits,
) -> WireResult<Vec<u8>> {
    let contract = codec.contract();
    limits.check(WireSegment::SchemaId, contract.schema().as_str().len())?;
    limits.check(WireSegment::SnapshotOwner, contract.owner().as_str().len())?;
    wire_u16_length(WireSegment::SchemaId, contract.schema().as_str().len())?;
    wire_u16_length(WireSegment::SnapshotOwner, contract.owner().as_str().len())?;

    let codec_limits = codec.limits();
    codec.validate_for_encode(value, codec_limits)?;
    let maximum = limits
        .max_snapshot_payload_bytes()
        .min(codec_limits.max_payload_bytes());
    let maximum = usize::try_from(maximum).unwrap_or(usize::MAX);
    let bounded_length = scratch.len().min(maximum);
    let encoded =
        postcard::to_slice(value, &mut scratch[..bounded_length]).map_err(|postcard_error| {
            WorldWireError::PostcardEncode {
                reason: postcard_error.to_string(),
            }
        })?;
    codec_limits.check_payload_bytes(encoded.len())?;
    let payload = VersionedPayload::new(
        contract.schema().clone(),
        contract.version(),
        encoded.to_vec(),
    );
    encode_snapshot(contract.owner(), &payload, limits)
}
pub(crate) fn encode_typed_snapshot_exact<C: SnapshotSchemaCodec>(
    codec: &C,
    value: &C::Value,
    payload_length: usize,
    limits: WorldWireLimits,
) -> WireResult<Vec<u8>> {
    let contract = codec.contract();
    let schema = contract.schema().as_str().as_bytes();
    let owner = contract.owner().as_str().as_bytes();
    limits.check(WireSegment::SchemaId, schema.len())?;
    limits.check(WireSegment::SnapshotOwner, owner.len())?;
    limits.check(WireSegment::SnapshotPayload, payload_length)?;
    let schema_length = wire_u16_length(WireSegment::SchemaId, schema.len())?;
    let owner_length = wire_u16_length(WireSegment::SnapshotOwner, owner.len())?;
    codec.validate_for_encode(value, codec.limits())?;
    codec.limits().check_payload_bytes(payload_length)?;
    let payload_length_u64 =
        u64::try_from(payload_length).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    let capacity = encoded_length_from_parts(schema.len(), owner.len(), payload_length)?;
    let mut output = Vec::with_capacity(capacity);
    output.extend_from_slice(&SNAPSHOT_MAGIC);
    output.extend_from_slice(&SNAPSHOT_ENVELOPE_MAJOR.to_be_bytes());
    output.extend_from_slice(&schema_length.to_be_bytes());
    output.extend_from_slice(schema);
    output.extend_from_slice(&owner_length.to_be_bytes());
    output.extend_from_slice(owner);
    output.extend_from_slice(&contract.version().get().to_be_bytes());
    output.extend_from_slice(&payload_length_u64.to_be_bytes());
    let digest_start = output.len();
    output.resize(digest_start + DIGEST_BYTES, 0);
    let payload_start = output.len();
    output.resize(capacity, 0);

    let encoded = postcard::to_slice(value, &mut output[payload_start..]).map_err(|error| {
        WorldWireError::PostcardEncode {
            reason: error.to_string(),
        }
    })?;
    if encoded.len() != payload_length {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "schema-owned length meter disagrees with the pinned postcard serializer"
                .to_owned(),
        });
    }
    let payload_digest = digest(encoded);
    output[digest_start..payload_start].copy_from_slice(&payload_digest);
    Ok(output)
}

/// Rejects a numeric DTO tag unless it appears in an explicit allowed set.
///
/// This helper keeps tag meaning independent from Rust enum declaration order.
///
/// # Errors
///
/// Returns [`WorldWireError::UnknownNumericTag`] for an unrecognized tag.
pub fn require_numeric_tag(field: &'static str, found: u16, allowed: &[u16]) -> WireResult<()> {
    if allowed.contains(&found) {
        Ok(())
    } else {
        Err(WorldWireError::UnknownNumericTag { field, found })
    }
}

fn preflight_encode(
    owner: &PackageName,
    payload: &VersionedPayload,
    limits: WorldWireLimits,
) -> WireResult<()> {
    limits.check(WireSegment::SchemaId, payload.schema().as_str().len())?;
    limits.check(WireSegment::SnapshotOwner, owner.as_str().len())?;
    limits.check(WireSegment::SnapshotPayload, payload.bytes().len())?;
    wire_u16_length(WireSegment::SchemaId, payload.schema().as_str().len())?;
    wire_u16_length(WireSegment::SnapshotOwner, owner.as_str().len())?;
    Ok(())
}

fn encoded_length(owner: &PackageName, payload: &VersionedPayload) -> WireResult<usize> {
    encoded_length_from_parts(
        payload.schema().as_str().len(),
        owner.as_str().len(),
        payload.bytes().len(),
    )
}

fn encoded_length_from_parts(
    schema_length: usize,
    owner_length: usize,
    payload_length: usize,
) -> WireResult<usize> {
    MAGIC_BYTES
        .checked_add(MAJOR_BYTES)
        .and_then(|value| value.checked_add(SEGMENT_LENGTH_BYTES))
        .and_then(|value| value.checked_add(schema_length))
        .and_then(|value| value.checked_add(SEGMENT_LENGTH_BYTES))
        .and_then(|value| value.checked_add(owner_length))
        .and_then(|value| value.checked_add(SCHEMA_VERSION_BYTES))
        .and_then(|value| value.checked_add(PAYLOAD_LENGTH_BYTES))
        .and_then(|value| value.checked_add(DIGEST_BYTES))
        .and_then(|value| value.checked_add(payload_length))
        .ok_or(WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })
}

fn validate_contract(
    schema: &SchemaId,
    owner: &PackageName,
    version: PayloadSchemaVersion,
    contract: SnapshotContract<'_>,
) -> WireResult<()> {
    if schema != contract.schema {
        return Err(WorldWireError::UnknownSchema {
            found: schema.clone(),
        });
    }
    if owner != contract.owner {
        return Err(WorldWireError::UnknownOwner {
            found: owner.clone(),
        });
    }
    if version != contract.version {
        return Err(WorldWireError::UnsupportedSchemaVersion {
            found: version.get(),
            expected: contract.version.get(),
        });
    }
    Ok(())
}

fn parse_schema(value: &str) -> WireResult<SchemaId> {
    value
        .parse()
        .map_err(
            |error: latticeaxiom_core::IdentifierError| WorldWireError::InvalidSchemaId {
                reason: error.to_string(),
            },
        )
}

fn parse_owner(value: &str) -> WireResult<PackageName> {
    value
        .parse()
        .map_err(
            |error: latticeaxiom_core::IdentifierError| WorldWireError::InvalidSnapshotOwner {
                reason: error.to_string(),
            },
        )
}

fn parse_schema_version(value: u32) -> WireResult<PayloadSchemaVersion> {
    PayloadSchemaVersion::new(value).map_err(|_| WorldWireError::InvalidSchemaVersion)
}

fn digest(payload: &[u8]) -> [u8; 32] {
    Sha256::digest(payload).into()
}

fn wire_u16_length(segment: WireSegment, length: usize) -> WireResult<u16> {
    u16::try_from(length).map_err(|_| WorldWireError::LengthOverflow { segment })
}

fn write_all<W: Write>(writer: &mut W, bytes: &[u8], section: &'static str) -> WireResult<()> {
    writer
        .write_all(bytes)
        .map_err(|source| WorldWireError::Io { section, source })
}

fn read_exact<R: Read>(reader: &mut R, bytes: &mut [u8], section: &'static str) -> WireResult<()> {
    reader.read_exact(bytes).map_err(|source| {
        if source.kind() == ErrorKind::UnexpectedEof {
            WorldWireError::TruncatedStream { section }
        } else {
            WorldWireError::Io { section, source }
        }
    })
}

fn read_array<const N: usize, R: Read>(
    reader: &mut R,
    section: &'static str,
) -> WireResult<[u8; N]> {
    let mut bytes = [0_u8; N];
    read_exact(reader, &mut bytes, section)?;
    Ok(bytes)
}

fn read_u16<R: Read>(reader: &mut R, section: &'static str) -> WireResult<u16> {
    Ok(u16::from_be_bytes(read_array(reader, section)?))
}

fn read_u32<R: Read>(reader: &mut R, section: &'static str) -> WireResult<u32> {
    Ok(u32::from_be_bytes(read_array(reader, section)?))
}

fn read_u64<R: Read>(reader: &mut R, section: &'static str) -> WireResult<u64> {
    Ok(u64::from_be_bytes(read_array(reader, section)?))
}

fn read_text_segment<R: Read>(
    reader: &mut R,
    limits: WorldWireLimits,
    segment: WireSegment,
    length_section: &'static str,
    value_section: &'static str,
) -> WireResult<String> {
    let length = usize::from(read_u16(reader, length_section)?);
    limits.check(segment, length)?;
    let mut bytes = vec![0_u8; length];
    read_exact(reader, &mut bytes, value_section)?;
    String::from_utf8(bytes).map_err(|_| WorldWireError::InvalidUtf8 { segment })
}

fn reject_stream_trailing_data<R: Read>(reader: &mut R) -> WireResult<()> {
    let mut byte = [0_u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err(WorldWireError::TrailingEnvelopeData),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(source) => {
                return Err(WorldWireError::Io {
                    section: "snapshot trailing-data check",
                    source,
                });
            }
        }
    }
}

struct SliceCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> SliceCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn read(&mut self, length: usize, section: &'static str) -> WireResult<&'a [u8]> {
        let remaining = self.remaining();
        if length > remaining {
            return Err(WorldWireError::Truncated {
                section,
                needed: length,
                remaining,
            });
        }
        let end = self
            .position
            .checked_add(length)
            .ok_or(WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?;
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn read_array<const N: usize>(&mut self, section: &'static str) -> WireResult<[u8; N]> {
        let mut output = [0_u8; N];
        output.copy_from_slice(self.read(N, section)?);
        Ok(output)
    }

    fn read_u16(&mut self, section: &'static str) -> WireResult<u16> {
        Ok(u16::from_be_bytes(self.read_array(section)?))
    }

    fn read_u32(&mut self, section: &'static str) -> WireResult<u32> {
        Ok(u32::from_be_bytes(self.read_array(section)?))
    }

    fn read_u64(&mut self, section: &'static str) -> WireResult<u64> {
        Ok(u64::from_be_bytes(self.read_array(section)?))
    }

    fn read_text_segment(
        &mut self,
        limits: WorldWireLimits,
        segment: WireSegment,
        length_section: &'static str,
        value_section: &'static str,
    ) -> WireResult<&'a str> {
        let length = usize::from(self.read_u16(length_section)?);
        limits.check(segment, length)?;
        let bytes = self.read(length, value_section)?;
        std::str::from_utf8(bytes).map_err(|_| WorldWireError::InvalidUtf8 { segment })
    }

    const fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}
#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
