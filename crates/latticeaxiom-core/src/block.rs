//! Stable numeric block identity used by the simulation hot path.

use core::fmt;

/// Stable numeric identity of a registered block kind.
///
/// Composition assigns these values deterministically. Runtime world data
/// carries only this compact identity; package string keys never enter the
/// per-voxel hot path. Raw value zero is permanently reserved for [`Self::AIR`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(u32);

impl BlockId {
    /// Empty space, permanently encoded as raw value zero.
    pub const AIR: Self = Self::from_raw(0);

    /// Creates a block identity from its stable raw representation.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the stable raw representation.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        self.0
    }

    /// Returns whether this identity denotes empty space.
    #[must_use]
    pub const fn is_air(self) -> bool {
        self.0 == Self::AIR.0
    }
}

impl From<u32> for BlockId {
    fn from(value: u32) -> Self {
        Self::from_raw(value)
    }
}

impl From<BlockId> for u32 {
    fn from(value: BlockId) -> Self {
        value.to_raw()
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_is_permanently_zero() {
        assert_eq!(BlockId::AIR.to_raw(), 0);
        assert!(BlockId::from_raw(0).is_air());
        assert!(!BlockId::from_raw(1).is_air());
    }

    #[test]
    fn raw_conversion_round_trips() {
        let block = BlockId::from_raw(42);
        assert_eq!(BlockId::from(u32::from(block)), block);
    }
}
