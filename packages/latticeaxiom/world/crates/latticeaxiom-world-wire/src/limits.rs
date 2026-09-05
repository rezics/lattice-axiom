use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

use crate::{WireResult, WorldWireError};

/// A bounded variable-length segment in a world-wire key or envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireSegment {
    /// Canonical dimension stable ID in a chunk-record key.
    DimensionId,
    /// Canonical owner stable ID in a chunk-record key.
    RecordOwnerId,
    /// Canonical schema ID in a snapshot envelope.
    SchemaId,
    /// Canonical package owner in a snapshot envelope.
    SnapshotOwner,
    /// Uncompressed postcard bytes in a snapshot envelope.
    SnapshotPayload,
}

impl std::fmt::Display for WireSegment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::DimensionId => "dimension ID",
            Self::RecordOwnerId => "record owner ID",
            Self::SchemaId => "schema ID",
            Self::SnapshotOwner => "snapshot owner",
            Self::SnapshotPayload => "snapshot payload",
        })
    }
}

/// A schema-owned resource measured before postcard allocation or recursion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotCodecResource {
    /// Uncompressed outer postcard payload bytes.
    PayloadBytes,
    /// Bytes in one nested schema-owned opaque payload.
    NestedPayloadBytes,
    /// Bytes across every nested schema-owned opaque payload.
    NestedPayloadTotalBytes,
    /// Bytes in one canonical identifier carried inside the DTO.
    IdentifierBytes,
    /// Elements across bounded DTO collections.
    CollectionElements,
    /// Nested DTO/container depth.
    NestingDepth,
}

impl std::fmt::Display for SnapshotCodecResource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::PayloadBytes => "snapshot payload bytes",
            Self::NestedPayloadBytes => "nested snapshot payload bytes",
            Self::NestedPayloadTotalBytes => "total nested snapshot payload bytes",
            Self::IdentifierBytes => "snapshot identifier bytes",
            Self::CollectionElements => "snapshot collection elements",
            Self::NestingDepth => "snapshot nesting depth",
        })
    }
}

/// Non-zero resource ceilings owned by one exact snapshot schema decoder.
///
/// The sealed codec fixes these values for its schema-owner-version contract;
/// callers cannot supply looser limits to typed encode or decode operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    clippy::struct_field_names,
    reason = "the max prefix makes each independently enforced schema budget explicit"
)]
pub struct SnapshotCodecLimits {
    max_payload_bytes: NonZeroU64,
    max_collection_elements: NonZeroU32,
    max_nesting_depth: NonZeroU16,
}

impl SnapshotCodecLimits {
    /// Creates the fixed resource limits for one sealed schema codec.
    #[must_use]
    pub const fn new(
        max_payload_bytes: NonZeroU64,
        max_collection_elements: NonZeroU32,
        max_nesting_depth: NonZeroU16,
    ) -> Self {
        Self {
            max_payload_bytes,
            max_collection_elements,
            max_nesting_depth,
        }
    }

    /// Returns the schema-specific uncompressed payload ceiling.
    #[must_use]
    pub const fn max_payload_bytes(self) -> u64 {
        self.max_payload_bytes.get()
    }

    /// Returns the schema-specific collection-element ceiling.
    #[must_use]
    pub const fn max_collection_elements(self) -> u32 {
        self.max_collection_elements.get()
    }

    /// Returns the schema-specific nesting-depth ceiling.
    #[must_use]
    pub const fn max_nesting_depth(self) -> u16 {
        self.max_nesting_depth.get()
    }

    /// Checks payload bytes before postcard decode or allocation.
    ///
    /// # Errors
    ///
    /// Returns [`WorldWireError::LengthOverflow`] when `actual` does not fit the
    /// wire counter, or [`WorldWireError::CodecBudgetExceeded`] above the fixed
    /// schema limit.
    pub fn check_payload_bytes(self, actual: usize) -> WireResult<()> {
        let actual = u64::try_from(actual).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
        Self::check(
            SnapshotCodecResource::PayloadBytes,
            actual,
            self.max_payload_bytes(),
        )
    }

    /// Checks a collection count before allocating its elements.
    ///
    /// # Errors
    ///
    /// Returns [`WorldWireError::CodecBudgetExceeded`] above the fixed schema
    /// limit.
    pub fn check_collection_elements(self, actual: u64) -> WireResult<()> {
        Self::check(
            SnapshotCodecResource::CollectionElements,
            actual,
            u64::from(self.max_collection_elements()),
        )
    }

    /// Checks nesting depth before descending into another DTO/container.
    ///
    /// # Errors
    ///
    /// Returns [`WorldWireError::CodecBudgetExceeded`] above the fixed schema
    /// limit.
    pub fn check_nesting_depth(self, actual: u32) -> WireResult<()> {
        Self::check(
            SnapshotCodecResource::NestingDepth,
            u64::from(actual),
            u64::from(self.max_nesting_depth()),
        )
    }

    fn check(resource: SnapshotCodecResource, actual: u64, maximum: u64) -> WireResult<()> {
        if actual > maximum {
            Err(WorldWireError::CodecBudgetExceeded {
                resource,
                actual,
                maximum,
            })
        } else {
            Ok(())
        }
    }
}

/// Hard ceilings applied before allocating or decoding variable-size input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    clippy::struct_field_names,
    reason = "the shared max prefix makes every independently configured hard ceiling explicit"
)]
pub struct WorldWireLimits {
    max_dimension_id_bytes: u16,
    max_record_owner_id_bytes: u16,
    max_schema_id_bytes: u16,
    max_snapshot_owner_bytes: u16,
    max_snapshot_payload_bytes: u64,
}

impl WorldWireLimits {
    /// Bootstrap limits for the first playable vertical slice.
    ///
    /// Identifier ceilings are intentionally much smaller than the `u16`
    /// representational maximum. The payload ceiling matches the D3 in-flight
    /// uncompressed commit-byte safety ceiling; storage may apply a lower
    /// per-record policy.
    pub const BOOTSTRAP: Self = Self {
        max_dimension_id_bytes: 1_024,
        max_record_owner_id_bytes: 1_024,
        max_schema_id_bytes: 1_024,
        max_snapshot_owner_bytes: 255,
        max_snapshot_payload_bytes: 64 * 1024 * 1024,
    };

    /// Creates non-zero wire ceilings.
    ///
    /// # Errors
    ///
    /// Returns [`WorldWireError::InvalidLimit`] when any ceiling is zero, or
    /// [`WorldWireError::LimitExceedsHardMaximum`] when a ceiling would relax
    /// the version-one absolute maximum.
    pub const fn new(
        max_dimension_id_bytes: u16,
        max_record_owner_id_bytes: u16,
        max_schema_id_bytes: u16,
        max_snapshot_owner_bytes: u16,
        max_snapshot_payload_bytes: u64,
    ) -> WireResult<Self> {
        if max_dimension_id_bytes == 0 {
            return Err(WorldWireError::InvalidLimit {
                name: "max_dimension_id_bytes",
                value: 0,
            });
        }
        if max_record_owner_id_bytes == 0 {
            return Err(WorldWireError::InvalidLimit {
                name: "max_record_owner_id_bytes",
                value: 0,
            });
        }
        if max_schema_id_bytes == 0 {
            return Err(WorldWireError::InvalidLimit {
                name: "max_schema_id_bytes",
                value: 0,
            });
        }
        if max_snapshot_owner_bytes == 0 {
            return Err(WorldWireError::InvalidLimit {
                name: "max_snapshot_owner_bytes",
                value: 0,
            });
        }
        if max_snapshot_payload_bytes == 0 {
            return Err(WorldWireError::InvalidLimit {
                name: "max_snapshot_payload_bytes",
                value: 0,
            });
        }
        let proposed = [
            (
                "max_dimension_id_bytes",
                max_dimension_id_bytes as u64,
                Self::BOOTSTRAP.max_dimension_id_bytes as u64,
            ),
            (
                "max_record_owner_id_bytes",
                max_record_owner_id_bytes as u64,
                Self::BOOTSTRAP.max_record_owner_id_bytes as u64,
            ),
            (
                "max_schema_id_bytes",
                max_schema_id_bytes as u64,
                Self::BOOTSTRAP.max_schema_id_bytes as u64,
            ),
            (
                "max_snapshot_owner_bytes",
                max_snapshot_owner_bytes as u64,
                Self::BOOTSTRAP.max_snapshot_owner_bytes as u64,
            ),
            (
                "max_snapshot_payload_bytes",
                max_snapshot_payload_bytes,
                Self::BOOTSTRAP.max_snapshot_payload_bytes,
            ),
        ];
        let mut index = 0;
        while index < proposed.len() {
            let (name, value, maximum) = proposed[index];
            if value > maximum {
                return Err(WorldWireError::LimitExceedsHardMaximum {
                    name,
                    value,
                    maximum,
                });
            }
            index += 1;
        }
        Ok(Self {
            max_dimension_id_bytes,
            max_record_owner_id_bytes,
            max_schema_id_bytes,
            max_snapshot_owner_bytes,
            max_snapshot_payload_bytes,
        })
    }

    /// Returns the dimension stable-ID byte ceiling.
    #[must_use]
    pub const fn max_dimension_id_bytes(self) -> u16 {
        self.max_dimension_id_bytes
    }

    /// Returns the record-owner stable-ID byte ceiling.
    #[must_use]
    pub const fn max_record_owner_id_bytes(self) -> u16 {
        self.max_record_owner_id_bytes
    }

    /// Returns the snapshot schema-ID byte ceiling.
    #[must_use]
    pub const fn max_schema_id_bytes(self) -> u16 {
        self.max_schema_id_bytes
    }

    /// Returns the snapshot package-owner byte ceiling.
    #[must_use]
    pub const fn max_snapshot_owner_bytes(self) -> u16 {
        self.max_snapshot_owner_bytes
    }

    /// Returns the uncompressed postcard payload byte ceiling.
    #[must_use]
    pub const fn max_snapshot_payload_bytes(self) -> u64 {
        self.max_snapshot_payload_bytes
    }

    pub(crate) fn check(self, segment: WireSegment, actual: usize) -> WireResult<()> {
        let maximum = match segment {
            WireSegment::DimensionId => u64::from(self.max_dimension_id_bytes),
            WireSegment::RecordOwnerId => u64::from(self.max_record_owner_id_bytes),
            WireSegment::SchemaId => u64::from(self.max_schema_id_bytes),
            WireSegment::SnapshotOwner => u64::from(self.max_snapshot_owner_bytes),
            WireSegment::SnapshotPayload => self.max_snapshot_payload_bytes,
        };
        let actual =
            u64::try_from(actual).map_err(|_| WorldWireError::LengthOverflow { segment })?;
        if actual > maximum {
            Err(WorldWireError::SegmentTooLong {
                segment,
                actual,
                maximum,
            })
        } else {
            Ok(())
        }
    }

    pub(crate) const fn check_u64(self, segment: WireSegment, actual: u64) -> WireResult<()> {
        let maximum = match segment {
            WireSegment::DimensionId => self.max_dimension_id_bytes as u64,
            WireSegment::RecordOwnerId => self.max_record_owner_id_bytes as u64,
            WireSegment::SchemaId => self.max_schema_id_bytes as u64,
            WireSegment::SnapshotOwner => self.max_snapshot_owner_bytes as u64,
            WireSegment::SnapshotPayload => self.max_snapshot_payload_bytes,
        };
        if actual > maximum {
            Err(WorldWireError::SegmentTooLong {
                segment,
                actual,
                maximum,
            })
        } else {
            Ok(())
        }
    }
}

impl Default for WorldWireLimits {
    fn default() -> Self {
        Self::BOOTSTRAP
    }
}
