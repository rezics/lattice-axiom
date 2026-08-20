//! Canonical interface names, tokens, descriptors, and collision governance.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{IdentifierError, StableId};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    LaxCommandBufferTableV0_1, LaxDiagnosticsTableV0_1, LaxEcsBatchTableV0_1, LaxInterfaceId,
    LaxMessagesTableV0_1,
};

const INTERFACE_ID_DOMAIN: &[u8] = b"latticeaxiom.interface-id\0";
const INTERFACE_DESCRIPTOR_DOMAIN: &[u8] = b"latticeaxiom.interface-descriptor.v1\0";

/// A canonical stable ID constrained to the unversioned `interface` kind.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InterfaceName(StableId);

impl InterfaceName {
    /// Parses and validates an unversioned canonical interface name.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid stable-ID grammar, a kind other than
    /// `interface`, or a name carrying a version suffix.
    pub fn parse(value: &str) -> Result<Self, InterfaceIdentityError> {
        let stable_id: StableId = value.parse()?;
        if stable_id.kind() != "interface" {
            return Err(InterfaceIdentityError::WrongKind {
                actual: stable_id.kind().to_owned(),
            });
        }
        if stable_id.major().is_some() {
            return Err(InterfaceIdentityError::VersionInCanonicalName);
        }
        Ok(Self(stable_id))
    }

    /// Returns the canonical ASCII identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// The complete canonical interface identity retained beside its lookup token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceIdentity {
    name: InterfaceName,
    full_hash: [u8; 32],
    token: LaxInterfaceId,
}

impl InterfaceIdentity {
    /// Derives the complete hash and wire token from a canonical name.
    ///
    /// # Errors
    ///
    /// Returns an error if `canonical_name` is not an unversioned stable
    /// `interface` identifier.
    pub fn derive(canonical_name: &str) -> Result<Self, InterfaceIdentityError> {
        let name = InterfaceName::parse(canonical_name)?;
        let full_hash = interface_digest(&name);
        let token = token_from_digest(full_hash);
        Ok(Self {
            name,
            full_hash,
            token,
        })
    }

    /// Returns the canonical unversioned name.
    #[must_use]
    pub fn name(&self) -> &InterfaceName {
        &self.name
    }

    /// Returns the full domain-separated SHA-256 digest.
    #[must_use]
    pub const fn full_hash(&self) -> [u8; 32] {
        self.full_hash
    }

    /// Returns the first 128 digest bits interpreted as two big-endian words.
    #[must_use]
    pub const fn token(&self) -> LaxInterfaceId {
        self.token
    }
}

/// An interface identity claim read from a manifest or schema registry.
#[derive(Clone, Copy, Debug)]
pub struct InterfaceClaim<'a> {
    /// Canonical full interface name.
    pub canonical_name: &'a str,
    /// Complete domain-separated digest retained for audit and collision checks.
    pub full_hash: [u8; 32],
    /// Compact token used for table lookup.
    pub token: LaxInterfaceId,
}

/// Errors in canonical interface identity or collision governance.
#[derive(Debug, Error)]
pub enum InterfaceIdentityError {
    /// The stable-ID grammar is invalid.
    #[error(transparent)]
    InvalidStableId(#[from] IdentifierError),
    /// The stable ID does not use the required `interface` kind.
    #[error("interface identity must use kind `interface`, found `{actual}`")]
    WrongKind {
        /// Actual stable-ID kind.
        actual: String,
    },
    /// Versions belong in negotiation fields, not the canonical name.
    #[error("canonical interface identity must not include a version suffix")]
    VersionInCanonicalName,
    /// A claim's complete digest does not match its name.
    #[error("interface `{canonical_name}` has a non-canonical full hash")]
    FullHashMismatch {
        /// Claimed canonical name.
        canonical_name: String,
    },
    /// A claim's token is not the big-endian prefix of the canonical digest.
    #[error("interface `{canonical_name}` has a non-canonical lookup token")]
    TokenMismatch {
        /// Claimed canonical name.
        canonical_name: String,
    },
    /// Two canonical names collide on the compact token.
    #[error("interface token collision between `{first}` and `{second}`")]
    TokenCollision {
        /// First canonical identity in deterministic lexical order.
        first: String,
        /// Second canonical identity in deterministic lexical order.
        second: String,
    },
}

/// Validates full hashes, compact tokens, duplicates, and token collisions.
///
/// # Errors
///
/// Returns the first deterministic invalid claim or collision. Exact duplicate
/// claims are accepted because they describe the same contract identity.
pub fn validate_interface_claims(
    claims: &[InterfaceClaim<'_>],
) -> Result<(), InterfaceIdentityError> {
    let mut verified = Vec::with_capacity(claims.len());
    for claim in claims {
        let identity = InterfaceIdentity::derive(claim.canonical_name)?;
        if identity.full_hash != claim.full_hash {
            return Err(InterfaceIdentityError::FullHashMismatch {
                canonical_name: claim.canonical_name.to_owned(),
            });
        }
        if identity.token != claim.token {
            return Err(InterfaceIdentityError::TokenMismatch {
                canonical_name: claim.canonical_name.to_owned(),
            });
        }
        verified.push((claim.token, claim.canonical_name));
    }
    validate_verified_tokens(&verified)
}

fn validate_verified_tokens(
    claims: &[(LaxInterfaceId, &str)],
) -> Result<(), InterfaceIdentityError> {
    let mut by_token = BTreeMap::new();
    for &(token, name) in claims {
        if let Some(previous) = by_token.insert(token, name)
            && previous != name
        {
            let (first, second) = if previous < name {
                (previous, name)
            } else {
                (name, previous)
            };
            return Err(InterfaceIdentityError::TokenCollision {
                first: first.to_owned(),
                second: second.to_owned(),
            });
        }
    }
    Ok(())
}

/// A stable field in a canonical interface table descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterfaceFieldDescriptor {
    /// Stable field key.
    pub stable_field_key: &'static str,
    /// Canonical wire type spelling.
    pub wire_type: &'static str,
    /// Frozen byte offset.
    pub offset: u32,
}

/// A canonical table descriptor with a domain-separated hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalInterfaceDescriptor {
    identity: InterfaceIdentity,
    major: u16,
    minor: u16,
    table_size: u32,
    table_alignment: u32,
    field_count: u32,
    fields: &'static [InterfaceFieldDescriptor],
}

impl CanonicalInterfaceDescriptor {
    /// Constructs a canonical descriptor and validates ordered unique fields.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid identity, zero table size/alignment,
    /// unsupported alignment, non-increasing offsets, or duplicate field keys.
    pub fn new(
        canonical_name: &str,
        major: u16,
        minor: u16,
        table_size: u32,
        table_alignment: u32,
        fields: &'static [InterfaceFieldDescriptor],
    ) -> Result<Self, InterfaceDescriptorError> {
        let identity = InterfaceIdentity::derive(canonical_name)?;
        if table_size == 0 {
            return Err(InterfaceDescriptorError::ZeroTableSize);
        }
        if !matches!(table_alignment, 1 | 2 | 4 | 8 | 16) {
            return Err(InterfaceDescriptorError::InvalidAlignment(table_alignment));
        }
        let field_count =
            u32::try_from(fields.len()).map_err(|_| InterfaceDescriptorError::TooManyFields)?;
        let mut previous_offset = None;
        let mut keys = BTreeSet::new();
        for field in fields {
            if field.stable_field_key.is_empty() || field.wire_type.is_empty() {
                return Err(InterfaceDescriptorError::EmptyFieldMetadata);
            }
            if let Some(previous) = previous_offset
                && field.offset <= previous
            {
                return Err(InterfaceDescriptorError::NonIncreasingOffset {
                    previous,
                    actual: field.offset,
                });
            }
            previous_offset = Some(field.offset);
            if !keys.insert(field.stable_field_key) {
                return Err(InterfaceDescriptorError::DuplicateFieldKey(
                    field.stable_field_key,
                ));
            }
            if field.offset >= table_size {
                return Err(InterfaceDescriptorError::FieldOutsideTable {
                    field: field.stable_field_key,
                    offset: field.offset,
                    table_size,
                });
            }
        }
        Ok(Self {
            identity,
            major,
            minor,
            table_size,
            table_alignment,
            field_count,
            fields,
        })
    }

    /// Returns the canonical interface identity.
    #[must_use]
    pub fn identity(&self) -> &InterfaceIdentity {
        &self.identity
    }

    /// Returns the interface major.
    #[must_use]
    pub const fn major(&self) -> u16 {
        self.major
    }

    /// Returns the interface minor.
    #[must_use]
    pub const fn minor(&self) -> u16 {
        self.minor
    }

    /// Returns the complete table size.
    #[must_use]
    pub const fn table_size(&self) -> u32 {
        self.table_size
    }

    /// Returns the table alignment.
    #[must_use]
    pub const fn table_alignment(&self) -> u32 {
        self.table_alignment
    }

    /// Returns ordered stable field descriptors.
    #[must_use]
    pub const fn fields(&self) -> &'static [InterfaceFieldDescriptor] {
        self.fields
    }

    /// Encodes the descriptor using big-endian integers and u64-length-prefixed
    /// UTF-8 text in stable field order.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(INTERFACE_DESCRIPTOR_DOMAIN);
        append_text(&mut bytes, self.identity.name.as_str());
        bytes.extend_from_slice(&self.major.to_be_bytes());
        bytes.extend_from_slice(&self.minor.to_be_bytes());
        bytes.extend_from_slice(&self.table_size.to_be_bytes());
        bytes.extend_from_slice(&self.table_alignment.to_be_bytes());
        bytes.extend_from_slice(&self.field_count.to_be_bytes());
        for field in self.fields {
            append_text(&mut bytes, field.stable_field_key);
            append_text(&mut bytes, field.wire_type);
            bytes.extend_from_slice(&field.offset.to_be_bytes());
        }
        bytes
    }

    /// Returns the complete domain-separated descriptor SHA-256.
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.canonical_bytes()).into()
    }
}

/// Errors in canonical interface descriptor construction.
#[derive(Debug, Error)]
pub enum InterfaceDescriptorError {
    /// The canonical interface identity is invalid.
    #[error(transparent)]
    InvalidIdentity(#[from] InterfaceIdentityError),
    /// A table cannot have zero bytes.
    #[error("interface table size must be nonzero")]
    ZeroTableSize,
    /// Alignment is not an ABI-POD 0.1 alignment.
    #[error("interface table alignment {0} is not one of 1, 2, 4, 8, 16")]
    InvalidAlignment(u32),
    /// A field lacks a stable key or wire type.
    #[error("interface field key and wire type must be nonempty")]
    EmptyFieldMetadata,
    /// The descriptor cannot encode more than `u32::MAX` fields.
    #[error("interface descriptor contains more than u32::MAX fields")]
    TooManyFields,
    /// Ordered fields must advance through the table.
    #[error("interface field offset {actual} does not follow offset {previous}")]
    NonIncreasingOffset {
        /// Previous field offset.
        previous: u32,
        /// Invalid next offset.
        actual: u32,
    },
    /// Stable field keys must be unique.
    #[error("duplicate interface field key `{0}`")]
    DuplicateFieldKey(&'static str),
    /// Field begins beyond the table extent.
    #[error("interface field `{field}` at offset {offset} is outside {table_size}-byte table")]
    FieldOutsideTable {
        /// Stable field key.
        field: &'static str,
        /// Invalid byte offset.
        offset: u32,
        /// Table size.
        table_size: u32,
    },
}

/// The four capability tables frozen for Portable Native ABI 0.1.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BuiltInInterface {
    /// `latticeaxiom:interface/core.diagnostics`.
    Diagnostics,
    /// `latticeaxiom:interface/ecs.batch`.
    EcsBatch,
    /// `latticeaxiom:interface/ecs.command-buffer`.
    EcsCommandBuffer,
    /// `latticeaxiom:interface/messages`.
    Messages,
}

const DIAGNOSTICS_FIELDS: &[InterfaceFieldDescriptor] = &[
    InterfaceFieldDescriptor {
        stable_field_key: "prefix",
        wire_type: "LaxInterfaceTableHeader",
        offset: 0,
    },
    InterfaceFieldDescriptor {
        stable_field_key: "emit",
        wire_type: "LaxEmitDiagnosticFn",
        offset: 56,
    },
];
const ECS_BATCH_FIELDS: &[InterfaceFieldDescriptor] = &[
    InterfaceFieldDescriptor {
        stable_field_key: "prefix",
        wire_type: "LaxInterfaceTableHeader",
        offset: 0,
    },
    InterfaceFieldDescriptor {
        stable_field_key: "invoke_system",
        wire_type: "LaxSystemCallbackFn",
        offset: 56,
    },
];
const COMMAND_BUFFER_FIELDS: &[InterfaceFieldDescriptor] = &[
    InterfaceFieldDescriptor {
        stable_field_key: "prefix",
        wire_type: "LaxInterfaceTableHeader",
        offset: 0,
    },
    InterfaceFieldDescriptor {
        stable_field_key: "append",
        wire_type: "LaxAppendCommandFn",
        offset: 56,
    },
];
const MESSAGES_FIELDS: &[InterfaceFieldDescriptor] = &[
    InterfaceFieldDescriptor {
        stable_field_key: "prefix",
        wire_type: "LaxInterfaceTableHeader",
        offset: 0,
    },
    InterfaceFieldDescriptor {
        stable_field_key: "publish",
        wire_type: "LaxPublishMessagesFn",
        offset: 56,
    },
    InterfaceFieldDescriptor {
        stable_field_key: "consume",
        wire_type: "LaxConsumeMessagesFn",
        offset: 64,
    },
];

/// Returns the canonical descriptor for one frozen ABI 0.1 table.
///
/// # Errors
///
/// Returns an error only if an internal frozen descriptor violates the same
/// validation applied to external descriptors.
pub fn builtin_interface_descriptor(
    interface: BuiltInInterface,
) -> Result<CanonicalInterfaceDescriptor, InterfaceDescriptorError> {
    let (name, size, fields) = match interface {
        BuiltInInterface::Diagnostics => (
            "latticeaxiom:interface/core.diagnostics",
            size_u32::<LaxDiagnosticsTableV0_1>()?,
            DIAGNOSTICS_FIELDS,
        ),
        BuiltInInterface::EcsBatch => (
            "latticeaxiom:interface/ecs.batch",
            size_u32::<LaxEcsBatchTableV0_1>()?,
            ECS_BATCH_FIELDS,
        ),
        BuiltInInterface::EcsCommandBuffer => (
            "latticeaxiom:interface/ecs.command-buffer",
            size_u32::<LaxCommandBufferTableV0_1>()?,
            COMMAND_BUFFER_FIELDS,
        ),
        BuiltInInterface::Messages => (
            "latticeaxiom:interface/messages",
            size_u32::<LaxMessagesTableV0_1>()?,
            MESSAGES_FIELDS,
        ),
    };
    CanonicalInterfaceDescriptor::new(name, 0, 1, size, 8, fields)
}

fn interface_digest(name: &InterfaceName) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(INTERFACE_ID_DOMAIN);
    hasher.update(name.as_str().as_bytes());
    hasher.finalize().into()
}

fn token_from_digest(digest: [u8; 32]) -> LaxInterfaceId {
    let hi = u64::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ]);
    let lo = u64::from_be_bytes([
        digest[8], digest[9], digest[10], digest[11], digest[12], digest[13], digest[14],
        digest[15],
    ]);
    LaxInterfaceId { hi, lo }
}

fn append_text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&value.len().to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn size_u32<T>() -> Result<u32, InterfaceDescriptorError> {
    u32::try_from(core::mem::size_of::<T>()).map_err(|_| InterfaceDescriptorError::ZeroTableSize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_verified_collision_fails_closed() {
        let token = LaxInterfaceId { hi: 7, lo: 11 };
        assert!(matches!(
            validate_verified_tokens(&[(token, "a:interface/one"), (token, "b:interface/two")]),
            Err(InterfaceIdentityError::TokenCollision { .. })
        ));
    }
}
