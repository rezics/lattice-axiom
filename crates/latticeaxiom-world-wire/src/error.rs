use latticeaxiom_core::{PackageName, SchemaId};
use thiserror::Error;

use crate::limits::{SnapshotCodecResource, WireSegment};

/// Result of a world-wire key or snapshot operation.
pub type WireResult<T> = Result<T, WorldWireError>;

/// Typed rejection from the explicit world-wire boundary.
#[derive(Debug, Error)]
pub enum WorldWireError {
    /// A configured hard ceiling was zero.
    #[error("invalid zero world-wire limit `{name}` ({value})")]
    InvalidLimit {
        /// Stable policy field name.
        name: &'static str,
        /// Rejected value.
        value: u64,
    },
    /// A caller attempted to relax a version-one absolute hard ceiling.
    #[error("world-wire limit `{name}` is {value}, exceeding hard maximum {maximum}")]
    LimitExceedsHardMaximum {
        /// Stable policy field name.
        name: &'static str,
        /// Rejected configured value.
        value: u64,
        /// Absolute maximum for this wire major.
        maximum: u64,
    },
    /// A host length could not be represented on the wire.
    #[error("{segment} length cannot be represented by world-wire")]
    LengthOverflow {
        /// Segment whose length overflowed.
        segment: WireSegment,
    },
    /// A declared or actual segment exceeded its preflight ceiling.
    #[error("{segment} has {actual} bytes, exceeding the hard ceiling {maximum}")]
    SegmentTooLong {
        /// Rejected segment.
        segment: WireSegment,
        /// Declared or actual bytes.
        actual: u64,
        /// Active ceiling.
        maximum: u64,
    },
    /// A slice ended inside a required field.
    #[error("truncated {section}: needed {needed} bytes, only {remaining} remain")]
    Truncated {
        /// Stable field or section name.
        section: &'static str,
        /// Bytes required at this position.
        needed: usize,
        /// Bytes available at this position.
        remaining: usize,
    },
    /// A stream ended inside a required field.
    #[error("stream ended while reading {section}")]
    TruncatedStream {
        /// Stable field or section name.
        section: &'static str,
    },
    /// A stream operation failed for a reason other than end-of-input.
    #[error("I/O failed while reading or writing {section}: {source}")]
    Io {
        /// Stable field or section name.
        section: &'static str,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The key major is not writable or decodable here.
    #[error("unsupported chunk-record key major {found}")]
    UnsupportedKeyMajor {
        /// Encountered major.
        found: u16,
    },
    /// A future record kind was passed to the writable encoder.
    #[error("record kind {found} is opaque and read-only under key major 1")]
    ReadOnlyRecordKind {
        /// Opaque `u16` tag.
        found: u16,
    },
    /// The snapshot marker was not `LAXWSNP\0`.
    #[error("invalid snapshot magic {found:02x?}")]
    InvalidSnapshotMagic {
        /// Encountered bytes.
        found: [u8; 8],
    },
    /// The envelope major is not writable or decodable here.
    #[error("unsupported snapshot envelope major {found}")]
    UnsupportedEnvelopeMajor {
        /// Encountered major.
        found: u16,
    },
    /// A textual segment was not UTF-8.
    #[error("{segment} is not canonical UTF-8")]
    InvalidUtf8 {
        /// Rejected segment.
        segment: WireSegment,
    },
    /// Raw world bytes were not a valid canonical v4 world identity.
    #[error("invalid world ID bytes: {reason}")]
    InvalidWorldId {
        /// Identity validator diagnostic.
        reason: String,
    },
    /// Dimension text was not a canonical dimension stable ID.
    #[error("invalid dimension ID: {reason}")]
    InvalidDimensionId {
        /// Identity validator diagnostic.
        reason: String,
    },
    /// Key owner text was not a canonical stable ID.
    #[error("invalid record owner ID: {reason}")]
    InvalidRecordOwnerId {
        /// Identity validator diagnostic.
        reason: String,
    },
    /// Envelope schema text was not a canonical schema ID.
    #[error("invalid snapshot schema ID: {reason}")]
    InvalidSchemaId {
        /// Identity validator diagnostic.
        reason: String,
    },
    /// Envelope owner text was not a canonical package name.
    #[error("invalid snapshot owner: {reason}")]
    InvalidSnapshotOwner {
        /// Identity validator diagnostic.
        reason: String,
    },
    /// The schema is structurally valid but not the decoder's schema.
    #[error("unknown snapshot schema `{found}`")]
    UnknownSchema {
        /// Unsupported schema identity.
        found: SchemaId,
    },
    /// The package is structurally valid but not the decoder's owner.
    #[error("unknown snapshot owner `{found}`")]
    UnknownOwner {
        /// Unsupported package owner.
        found: PackageName,
    },
    /// The schema version is not the version owned by this decoder.
    #[error("unsupported schema version {found}; decoder accepts exactly {expected}")]
    UnsupportedSchemaVersion {
        /// Encountered positive schema version.
        found: u32,
        /// Exact version handled by this decoder.
        expected: u32,
    },
    /// Version zero is reserved and cannot identify a schema payload.
    #[error("snapshot schema version zero is reserved")]
    InvalidSchemaVersion,
    /// Declared payload bytes do not match the complete slice.
    #[error("snapshot has {actual} payload bytes, but declares {declared}")]
    PayloadLengthMismatch {
        /// Length declared in the envelope.
        declared: u64,
        /// Bytes present after the envelope header.
        actual: u64,
    },
    /// Bytes remain after a complete chunk-record key tuple.
    #[error("chunk-record key contains {remaining} trailing bytes")]
    TrailingKeyBytes {
        /// Bytes after the owner segment.
        remaining: usize,
    },
    /// Snapshot bytes remain after the declared payload.
    #[error("snapshot stream contains trailing data")]
    TrailingEnvelopeData,
    /// The payload hash did not match the envelope digest.
    #[error("snapshot payload SHA-256 mismatch")]
    DigestMismatch {
        /// Digest carried by the envelope.
        expected: [u8; 32],
        /// Digest recomputed over the uncompressed postcard payload.
        actual: [u8; 32],
    },
    /// A schema-owned pre-decode or pre-allocation budget was exceeded.
    #[error("{resource} is {actual}, exceeding the schema codec ceiling {maximum}")]
    CodecBudgetExceeded {
        /// Resource whose declared or observed use was rejected.
        resource: SnapshotCodecResource,
        /// Declared or observed resource use.
        actual: u64,
        /// Fixed ceiling owned by the sealed schema codec.
        maximum: u64,
    },
    /// Postcard could not encode a DTO into the bounded caller buffer.
    #[error("postcard payload encoding failed: {reason}")]
    PostcardEncode {
        /// Stable serializer diagnostic.
        reason: String,
    },
    /// Payload bytes decode but are not the codec's unique canonical encoding.
    #[error("non-canonical postcard payload: {reason}")]
    NonCanonicalPayload {
        /// Stable codec diagnostic.
        reason: String,
    },
    /// Postcard could not decode the exact schema payload bytes.
    #[error("malformed postcard payload: {reason}")]
    MalformedPayload {
        /// Stable decoder diagnostic.
        reason: String,
    },
    /// A decoded DTO did not consume all payload bytes.
    #[error("schema DTO left {remaining} trailing payload bytes")]
    TrailingPayloadBytes {
        /// Unconsumed payload bytes.
        remaining: usize,
    },

    /// A versioned DTO contained an unrecognized fixed numeric tag.
    #[error("unknown numeric tag {found} in `{field}`")]
    UnknownNumericTag {
        /// Stable schema field name.
        field: &'static str,
        /// Unsupported tag.
        found: u16,
    },
}
