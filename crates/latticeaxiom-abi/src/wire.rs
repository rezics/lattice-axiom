//! Generated wire bindings and stable numeric constants.

use core::{cmp::Ordering, hash::Hash};

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

impl LaxAbiHeader {
    /// Creates an ABI header for a structure with an explicitly supplied size.
    #[must_use]
    pub const fn new(abi_major: u16, abi_minor: u16, struct_size: u32, flags: u32) -> Self {
        Self {
            magic: LAX_ABI_MAGIC,
            abi_major,
            abi_minor,
            struct_size,
            flags,
        }
    }
}

impl LaxHash256 {
    /// The all-zero hash, used only where the wire contract defines a sentinel.
    pub const ZERO: Self = Self { bytes: [0; 32] };

    /// Returns whether every digest byte is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.bytes == [0; 32]
    }
}

impl PartialEq for LaxHash256 {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for LaxHash256 {}

impl Hash for LaxHash256 {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

impl LaxInterfaceId {
    /// The all-zero invalid interface token.
    pub const INVALID: Self = Self { hi: 0, lo: 0 };

    /// Returns whether this is the all-zero invalid token.
    #[must_use]
    pub const fn is_invalid(self) -> bool {
        self.hi == 0 && self.lo == 0
    }
}

impl PartialEq for LaxInterfaceId {
    fn eq(&self, other: &Self) -> bool {
        self.hi == other.hi && self.lo == other.lo
    }
}

impl Eq for LaxInterfaceId {}

impl PartialOrd for LaxInterfaceId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LaxInterfaceId {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.hi, self.lo).cmp(&(other.hi, other.lo))
    }
}

impl Hash for LaxInterfaceId {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.hi.hash(state);
        self.lo.hash(state);
    }
}

impl LaxOpaqueHandle {
    /// The all-zero invalid handle.
    pub const INVALID: Self = Self {
        table_id: 0,
        slot: 0,
        generation: 0,
    };

    /// Creates a non-normalized raw handle value for boundary validation.
    #[must_use]
    pub const fn from_wire(table_id: u64, slot: u32, generation: u32) -> Self {
        Self {
            table_id,
            slot,
            generation,
        }
    }

    /// Returns whether all handle fields are zero.
    #[must_use]
    pub const fn is_invalid(self) -> bool {
        self.table_id == 0 && self.slot == 0 && self.generation == 0
    }
}

macro_rules! typed_handle {
    ($name:ident) => {
        impl $name {
            /// Creates a kind-specific handle from wire fields for validation.
            #[must_use]
            pub const fn from_wire(table_id: u64, slot: u32, generation: u32) -> Self {
                Self {
                    raw: LaxOpaqueHandle::from_wire(table_id, slot, generation),
                }
            }

            /// Returns the underlying representation without changing handle kind.
            #[must_use]
            pub const fn raw(self) -> LaxOpaqueHandle {
                self.raw
            }
        }
    };
}

typed_handle!(LaxEngineInstanceHandle);
typed_handle!(LaxEntityHandle);
typed_handle!(LaxAssetHandle);
typed_handle!(LaxTaskHandle);
