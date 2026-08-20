//! Allocation-free validation for copied wire metadata and staged records.
//!
//! These functions never dereference foreign pointers. A loader must first
//! establish that fixed headers are readable, copy metadata into host memory,
//! and only then use these validators before any payload dereference.

use thiserror::Error;

use crate::{
    LAX_ABI_MAGIC, LAX_COLUMN_FLAG_OPTIONAL, LAX_COLUMN_FLAG_READ, LAX_COLUMN_FLAG_WRITE,
    LAX_COLUMN_FLAGS_KNOWN, LAX_COMMAND_OPCODE_ADD_COMPONENT, LAX_COMMAND_OPCODE_ASSET,
    LAX_COMMAND_OPCODE_DESPAWN, LAX_COMMAND_OPCODE_REMOVE_COMPONENT, LAX_COMMAND_OPCODE_SPAWN,
    LAX_COMMAND_OPCODE_WORLD, LAX_HEADER_ADVISORY_MASK, LAX_HEADER_REQUIRED_MASK, LaxAbiHeader,
    LaxBytes, LaxColumnViewV0_1, LaxEntityHandle, LaxOpaqueHandle, LaxOwnedBuffer,
};

/// A copied-header acceptance policy for one versioned ABI structure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeaderPolicy {
    /// Required ABI major.
    pub expected_major: u16,
    /// Minimum accepted minor.
    pub min_minor: u16,
    /// Maximum accepted minor.
    pub max_minor: u16,
    /// Minimum structure size that covers all required fields.
    pub minimum_struct_size: u32,
    /// Required high flag bits understood by this reader.
    pub known_required_flags: u32,
}

/// Advisory information retained from a successfully validated header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeaderInspection {
    /// Unknown and known advisory low bits, preserved for inspector output.
    pub advisory_flags: u32,
    /// Caller-declared structure size, including an unknown append-only tail.
    pub struct_size: u32,
}

/// Exact target policy for Portable Native ABI 0.x.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetPolicy {
    /// Expected endianness discriminator.
    pub endianness: u8,
    /// Expected pointer width in bits.
    pub pointer_width_bits: u8,
    /// Expected platform C calling ABI discriminator.
    pub c_call_abi: u16,
}

/// Validates a fixed header copied from already-established readable memory.
///
/// Unknown tail bytes are accepted through `struct_size`; this function never
/// reads them. Unknown high must-understand flags fail closed, while all low
/// advisory bits are retained in the result.
///
/// # Errors
///
/// Returns the first failure in magic, version, size, then flag order.
pub fn validate_header(
    header: &LaxAbiHeader,
    policy: HeaderPolicy,
) -> Result<HeaderInspection, AbiContractError> {
    if policy.min_minor > policy.max_minor {
        return Err(AbiContractError::InvalidHeaderPolicy);
    }
    if header.magic != LAX_ABI_MAGIC {
        return Err(AbiContractError::WrongMagic {
            actual: header.magic,
        });
    }
    if header.abi_major != policy.expected_major {
        return Err(AbiContractError::UnsupportedMajor {
            expected: policy.expected_major,
            actual: header.abi_major,
        });
    }
    if !(policy.min_minor..=policy.max_minor).contains(&header.abi_minor) {
        return Err(AbiContractError::UnsupportedMinor {
            minimum: policy.min_minor,
            maximum: policy.max_minor,
            actual: header.abi_minor,
        });
    }
    if header.struct_size < policy.minimum_struct_size {
        return Err(AbiContractError::ShortStructure {
            minimum: policy.minimum_struct_size,
            actual: header.struct_size,
        });
    }
    let unknown_required = header.flags & LAX_HEADER_REQUIRED_MASK & !policy.known_required_flags;
    if unknown_required != 0 {
        return Err(AbiContractError::UnknownRequiredFlags(unknown_required));
    }
    Ok(HeaderInspection {
        advisory_flags: header.flags & LAX_HEADER_ADVISORY_MASK,
        struct_size: header.struct_size,
    })
}

/// Validates the duplicated target contract in an entry request.
///
/// # Errors
///
/// Returns an exact target or required-zero reserved-field mismatch.
pub fn validate_target(
    actual: TargetPolicy,
    expected: TargetPolicy,
    target_reserved: u16,
    entry_reserved: u16,
) -> Result<(), AbiContractError> {
    if target_reserved != 0 {
        return Err(AbiContractError::ReservedFieldNonZero {
            field: "target_reserved",
            actual: u64::from(target_reserved),
        });
    }
    if entry_reserved != 0 {
        return Err(AbiContractError::ReservedFieldNonZero {
            field: "entry_reserved",
            actual: u64::from(entry_reserved),
        });
    }
    if actual.endianness != expected.endianness {
        return Err(AbiContractError::WrongEndianness {
            expected: expected.endianness,
            actual: actual.endianness,
        });
    }
    if actual.pointer_width_bits != expected.pointer_width_bits {
        return Err(AbiContractError::WrongPointerWidth {
            expected: expected.pointer_width_bits,
            actual: actual.pointer_width_bits,
        });
    }
    if actual.c_call_abi != expected.c_call_abi {
        return Err(AbiContractError::WrongCallingAbi {
            expected: expected.c_call_abi,
            actual: actual.c_call_abi,
        });
    }
    Ok(())
}

/// Validates nullability and checked address extent for a borrowed span.
///
/// This function does not establish that the address is mapped or readable.
///
/// # Errors
///
/// Returns an error for a null nonempty span or address-range overflow.
pub fn validate_borrowed_bytes(bytes: &LaxBytes) -> Result<(), AbiContractError> {
    validate_address_extent(pointer_address(bytes.data), bytes.len, "borrowed bytes")
}

/// A safe shape copied from an owned-buffer wire record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedBufferShape {
    /// Allocation base address, or `None` for null.
    pub data_address: Option<u64>,
    /// Initialized bytes.
    pub len: u64,
    /// Allocation capacity.
    pub capacity: u64,
    /// Allocation alignment.
    pub alignment: u32,
    /// Wire flags.
    pub flags: u32,
    /// Whether an allocator-side release callback is present.
    pub has_release: bool,
}

impl From<&LaxOwnedBuffer> for OwnedBufferShape {
    fn from(buffer: &LaxOwnedBuffer) -> Self {
        Self {
            data_address: pointer_address(buffer.data),
            len: buffer.len,
            capacity: buffer.capacity,
            alignment: buffer.alignment,
            flags: buffer.flags,
            has_release: buffer.release.is_some(),
        }
    }
}

/// Validates owned-buffer shape before reading or accepting ownership.
///
/// # Errors
///
/// Returns an error for unknown flags, invalid length/capacity/alignment,
/// missing allocation or release callback, or address overflow.
pub fn validate_owned_buffer(shape: OwnedBufferShape) -> Result<(), AbiContractError> {
    if shape.flags != 0 {
        return Err(AbiContractError::UnknownOwnedBufferFlags(shape.flags));
    }
    if shape.len > shape.capacity {
        return Err(AbiContractError::LengthExceedsCapacity {
            len: shape.len,
            capacity: shape.capacity,
        });
    }
    if shape.alignment == 0 || !shape.alignment.is_power_of_two() {
        return Err(AbiContractError::InvalidAlignment(shape.alignment));
    }
    let Some(address) = shape.data_address else {
        if shape.capacity == 0 {
            return Ok(());
        }
        return Err(AbiContractError::NullNonemptySpan("owned buffer"));
    };
    if address % u64::from(shape.alignment) != 0 {
        return Err(AbiContractError::MisalignedAddress {
            address,
            alignment: shape.alignment,
        });
    }
    address
        .checked_add(shape.capacity)
        .ok_or(AbiContractError::AddressRangeOverflow("owned buffer"))?;
    if !shape.has_release {
        return Err(AbiContractError::MissingOwnedBufferRelease);
    }
    Ok(())
}

/// One request against a per-callback scratch arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScratchRequest {
    /// Bytes already allocated in this callback.
    pub used_bytes: u64,
    /// Requested additional bytes.
    pub requested_bytes: u64,
    /// Requested ABI-POD alignment.
    pub alignment: u32,
    /// Hard per-callback arena budget.
    pub byte_budget: u64,
}

/// Validates a scratch allocation without performing it.
///
/// # Errors
///
/// Returns an error for unsupported alignment, arithmetic overflow, or budget
/// exhaustion. Alignments are restricted to ABI-POD 0.1 values.
pub fn validate_scratch_request(request: ScratchRequest) -> Result<u64, AbiContractError> {
    validate_abi_alignment(request.alignment)?;
    let new_used = request
        .used_bytes
        .checked_add(request.requested_bytes)
        .ok_or(AbiContractError::ByteCountOverflow("scratch arena"))?;
    if new_used > request.byte_budget {
        return Err(AbiContractError::BudgetExceeded {
            domain: "scratch arena bytes",
            limit: request.byte_budget,
            actual: new_used,
        });
    }
    Ok(new_used)
}

/// Hard bounds for one ECS callback's batches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchLimits {
    /// Maximum batch views.
    pub max_batches: u64,
    /// Maximum entities in one batch.
    pub max_entities_per_batch: u64,
    /// Maximum entities across the callback.
    pub max_total_entities: u64,
    /// Maximum columns in one batch.
    pub max_columns_per_batch: u64,
    /// Maximum columns across the callback.
    pub max_total_columns: u64,
    /// Maximum borrowed column address extent across the callback.
    pub max_total_bytes: u64,
}

/// A pointer-free copy of one column's wire metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnShape {
    /// Process-local component numeric ID.
    pub component_numeric_id: u32,
    /// Complete ABI layout hash.
    pub layout_hash: [u8; 32],
    /// Column base address, or `None` for null.
    pub data_address: Option<u64>,
    /// Element count.
    pub count: u64,
    /// Byte stride.
    pub stride: u64,
    /// Initialized element size.
    pub element_size: u64,
    /// Declared base alignment.
    pub alignment: u32,
    /// Read/write/optional flags.
    pub flags: u32,
    /// Required-zero wire field.
    pub reserved: u32,
}

impl From<&LaxColumnViewV0_1> for ColumnShape {
    fn from(column: &LaxColumnViewV0_1) -> Self {
        Self {
            component_numeric_id: column.component_numeric_id,
            layout_hash: column.layout_hash.bytes,
            data_address: pointer_address(column.data),
            count: column.count,
            stride: column.stride,
            element_size: column.element_size,
            alignment: column.alignment,
            flags: column.flags,
            reserved: column.reserved,
        }
    }
}

/// One entity count paired with ordered copied column metadata.
#[derive(Clone, Copy, Debug)]
pub struct BatchShape<'a> {
    /// Entity count in the batch.
    pub entity_count: u64,
    /// Columns ordered by component numeric ID.
    pub columns: &'a [ColumnShape],
}

/// Validates bounded ECS batch and column metadata before any column dereference.
///
/// # Errors
///
/// Returns the first deterministic bound, ordering, layout, flag, pointer, or
/// checked-arithmetic failure.
#[allow(
    clippy::too_many_lines,
    reason = "keeping the ordered fail-closed validation pipeline together makes its precedence auditable"
)]
pub fn validate_batches(
    batches: &[BatchShape<'_>],
    limits: BatchLimits,
) -> Result<(), AbiContractError> {
    let batch_count = slice_len_u64(batches.len(), "batch count")?;
    enforce_limit("batch count", batch_count, limits.max_batches)?;
    let mut total_entities = 0_u64;
    let mut total_columns = 0_u64;
    let mut total_bytes = 0_u64;
    for (batch_index, batch) in batches.iter().enumerate() {
        enforce_limit(
            "entities per batch",
            batch.entity_count,
            limits.max_entities_per_batch,
        )?;
        total_entities = total_entities
            .checked_add(batch.entity_count)
            .ok_or(AbiContractError::CountOverflow("total entities"))?;
        enforce_limit("total entities", total_entities, limits.max_total_entities)?;
        let column_count = slice_len_u64(batch.columns.len(), "column count")?;
        enforce_limit(
            "columns per batch",
            column_count,
            limits.max_columns_per_batch,
        )?;
        total_columns = total_columns
            .checked_add(column_count)
            .ok_or(AbiContractError::CountOverflow("total columns"))?;
        enforce_limit("total columns", total_columns, limits.max_total_columns)?;
        let mut previous_component = None;
        for column in batch.columns {
            if column.reserved != 0 {
                return Err(AbiContractError::ReservedFieldNonZero {
                    field: "column.reserved",
                    actual: u64::from(column.reserved),
                });
            }
            if column.component_numeric_id == 0 {
                return Err(AbiContractError::ZeroNumericId("component"));
            }
            if let Some(previous) = previous_component
                && column.component_numeric_id <= previous
            {
                return Err(AbiContractError::NonCanonicalColumnOrder {
                    batch_index,
                    previous,
                    actual: column.component_numeric_id,
                });
            }
            previous_component = Some(column.component_numeric_id);
            if column.layout_hash == [0; 32] {
                return Err(AbiContractError::ZeroLayoutHash {
                    component_numeric_id: column.component_numeric_id,
                });
            }
            if column.flags & !LAX_COLUMN_FLAGS_KNOWN != 0 {
                return Err(AbiContractError::UnknownColumnFlags(
                    column.flags & !LAX_COLUMN_FLAGS_KNOWN,
                ));
            }
            if column.flags & (LAX_COLUMN_FLAG_READ | LAX_COLUMN_FLAG_WRITE) == 0 {
                return Err(AbiContractError::ColumnHasNoAccessMode {
                    component_numeric_id: column.component_numeric_id,
                });
            }
            let absent_optional = column.flags & LAX_COLUMN_FLAG_OPTIONAL != 0
                && column.data_address.is_none()
                && column.count == 0;
            if absent_optional {
                continue;
            }
            if column.count != batch.entity_count {
                return Err(AbiContractError::ColumnCountMismatch {
                    component_numeric_id: column.component_numeric_id,
                    entities: batch.entity_count,
                    column: column.count,
                });
            }
            validate_abi_alignment(column.alignment)?;
            if column.element_size == 0 || column.stride < column.element_size {
                return Err(AbiContractError::InvalidColumnStride {
                    stride: column.stride,
                    element_size: column.element_size,
                });
            }
            let extent = column_extent(column.count, column.stride, column.element_size)?;
            if extent > 0 {
                let Some(address) = column.data_address else {
                    return Err(AbiContractError::NullNonemptySpan("ECS column"));
                };
                if address % u64::from(column.alignment) != 0 {
                    return Err(AbiContractError::MisalignedAddress {
                        address,
                        alignment: column.alignment,
                    });
                }
                address
                    .checked_add(extent)
                    .ok_or(AbiContractError::AddressRangeOverflow("ECS column"))?;
            }
            total_bytes = total_bytes
                .checked_add(extent)
                .ok_or(AbiContractError::ByteCountOverflow("total column extent"))?;
            enforce_limit("total column bytes", total_bytes, limits.max_total_bytes)?;
        }
    }
    Ok(())
}

/// A stable structural command opcode.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CommandKind {
    /// Spawn an entity through a versioned payload.
    Spawn,
    /// Despawn an existing entity.
    Despawn,
    /// Add a component through a versioned payload.
    AddComponent,
    /// Remove a component.
    RemoveComponent,
    /// Submit a versioned asset command.
    Asset,
    /// Submit a versioned world command.
    World,
}

impl CommandKind {
    /// Returns the stable wire opcode.
    #[must_use]
    pub const fn wire_code(self) -> u32 {
        match self {
            Self::Spawn => LAX_COMMAND_OPCODE_SPAWN,
            Self::Despawn => LAX_COMMAND_OPCODE_DESPAWN,
            Self::AddComponent => LAX_COMMAND_OPCODE_ADD_COMPONENT,
            Self::RemoveComponent => LAX_COMMAND_OPCODE_REMOVE_COMPONENT,
            Self::Asset => LAX_COMMAND_OPCODE_ASSET,
            Self::World => LAX_COMMAND_OPCODE_WORLD,
        }
    }

    fn from_wire(value: u32) -> Result<Self, AbiContractError> {
        match value {
            LAX_COMMAND_OPCODE_SPAWN => Ok(Self::Spawn),
            LAX_COMMAND_OPCODE_DESPAWN => Ok(Self::Despawn),
            LAX_COMMAND_OPCODE_ADD_COMPONENT => Ok(Self::AddComponent),
            LAX_COMMAND_OPCODE_REMOVE_COMPONENT => Ok(Self::RemoveComponent),
            LAX_COMMAND_OPCODE_ASSET => Ok(Self::Asset),
            LAX_COMMAND_OPCODE_WORLD => Ok(Self::World),
            _ => Err(AbiContractError::UnknownCommandOpcode(value)),
        }
    }
}

/// Whether a command must target an existing entity or use the invalid sentinel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetRequirement {
    /// The target must be all-zero.
    None,
    /// The target must be live in the supplied entity table snapshot.
    ExistingEntity,
}

/// One permission and schema row in the compiled registration image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandContract {
    /// Permitted command kind.
    pub kind: CommandKind,
    /// Process-local subject/schema numeric ID.
    pub subject_numeric_id: u32,
    /// Expected layout/payload schema hash.
    pub layout_hash: [u8; 32],
    /// Minimum payload bytes.
    pub min_payload_bytes: u64,
    /// Maximum payload bytes.
    pub max_payload_bytes: u64,
    /// Target handle rule.
    pub target: TargetRequirement,
}

/// One safe staged command whose payload is already borrowed as a Rust slice.
#[derive(Clone, Copy, Debug)]
pub struct CommandRecord<'a> {
    /// Strictly increasing callback-local sequence.
    pub sequence: u64,
    /// Wire opcode.
    pub opcode: u32,
    /// ABI 0.1 requires zero flags.
    pub flags: u32,
    /// Entity target.
    pub target: LaxEntityHandle,
    /// Process-local subject/schema numeric ID.
    pub subject_numeric_id: u32,
    /// Required-zero field.
    pub reserved: u32,
    /// Layout/payload schema hash.
    pub layout_hash: [u8; 32],
    /// Versioned payload bytes.
    pub payload: &'a [u8],
}

/// One live slot generation in an immutable validation snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandleGeneration {
    /// Slot number.
    pub slot: u32,
    /// Current nonzero generation.
    pub generation: u32,
}

/// A process-local entity table snapshot used at a schedule barrier.
#[derive(Clone, Copy, Debug)]
pub struct HandleTableSnapshot<'a> {
    /// Nonzero process-unique table identity.
    pub table_id: u64,
    /// Live entries in strictly increasing slot order.
    pub live: &'a [HandleGeneration],
}

/// Hard staged-command bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandLimits {
    /// Maximum command records.
    pub max_commands: u64,
    /// Maximum compiled permission/schema rows inspected at the barrier.
    pub max_contracts: u64,
    /// Maximum live handle rows inspected at the barrier.
    pub max_live_handles: u64,
    /// Maximum payload bytes in one command.
    pub max_payload_bytes_per_command: u64,
    /// Maximum payload bytes across the entire staged batch.
    pub max_total_payload_bytes: u64,
}

/// Validates a staged structural command batch atomically before apply.
///
/// Count and byte limits are preflighted before schema, permission, handle, or
/// ordering work. The caller applies nothing unless this function returns
/// success.
///
/// # Errors
///
/// Returns the first deterministic bound, contract, permission, ordering,
/// schema, or handle-generation error.
pub fn validate_commands(
    commands: &[CommandRecord<'_>],
    contracts: &[CommandContract],
    entity_table: HandleTableSnapshot<'_>,
    limits: CommandLimits,
) -> Result<(), AbiContractError> {
    let command_count = slice_len_u64(commands.len(), "command count")?;
    enforce_limit("command count", command_count, limits.max_commands)?;
    let contract_count = slice_len_u64(contracts.len(), "command contract count")?;
    enforce_limit(
        "command contract count",
        contract_count,
        limits.max_contracts,
    )?;
    let live_handle_count = slice_len_u64(entity_table.live.len(), "live handle count")?;
    enforce_limit(
        "live handle count",
        live_handle_count,
        limits.max_live_handles,
    )?;
    let mut total_payload = 0_u64;
    for command in commands {
        let payload_len = slice_len_u64(command.payload.len(), "command payload")?;
        enforce_limit(
            "command payload bytes",
            payload_len,
            limits.max_payload_bytes_per_command,
        )?;
        total_payload = total_payload
            .checked_add(payload_len)
            .ok_or(AbiContractError::ByteCountOverflow("total command payload"))?;
    }
    enforce_limit(
        "total command payload bytes",
        total_payload,
        limits.max_total_payload_bytes,
    )?;
    validate_command_contract_order(contracts)?;
    validate_handle_snapshot(entity_table)?;

    let mut previous_sequence = None;
    for command in commands {
        if command.flags != 0 {
            return Err(AbiContractError::UnknownCommandFlags(command.flags));
        }
        if command.reserved != 0 {
            return Err(AbiContractError::ReservedFieldNonZero {
                field: "command.reserved",
                actual: u64::from(command.reserved),
            });
        }
        if let Some(previous) = previous_sequence
            && command.sequence <= previous
        {
            return Err(AbiContractError::NonIncreasingSequence {
                domain: "command",
                previous,
                actual: command.sequence,
            });
        }
        previous_sequence = Some(command.sequence);
        let kind = CommandKind::from_wire(command.opcode)?;
        let contract = contracts
            .binary_search_by_key(&(kind, command.subject_numeric_id), |contract| {
                (contract.kind, contract.subject_numeric_id)
            })
            .ok()
            .map(|index| &contracts[index])
            .ok_or(AbiContractError::CommandNotPermitted {
                opcode: command.opcode,
                subject_numeric_id: command.subject_numeric_id,
            })?;
        if contract.layout_hash != command.layout_hash {
            return Err(AbiContractError::CommandSchemaMismatch {
                opcode: command.opcode,
                subject_numeric_id: command.subject_numeric_id,
            });
        }
        let payload_len = slice_len_u64(command.payload.len(), "command payload")?;
        if !(contract.min_payload_bytes..=contract.max_payload_bytes).contains(&payload_len) {
            return Err(AbiContractError::CommandPayloadSize {
                minimum: contract.min_payload_bytes,
                maximum: contract.max_payload_bytes,
                actual: payload_len,
            });
        }
        match contract.target {
            TargetRequirement::None => validate_absent_handle(command.target.raw())?,
            TargetRequirement::ExistingEntity => {
                validate_live_handle(command.target.raw(), entity_table)?;
            }
        }
    }
    Ok(())
}

/// One permitted message schema row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageContract {
    /// Process-local schema numeric ID.
    pub schema_numeric_id: u32,
    /// Minimum accepted schema version.
    pub min_schema_version: u32,
    /// Maximum accepted schema version.
    pub max_schema_version: u32,
    /// Hard payload byte bound for this schema.
    pub max_payload_bytes: u64,
}

/// One safe typed message record.
#[derive(Clone, Copy, Debug)]
pub struct MessageRecord<'a> {
    /// Process-local schema numeric ID.
    pub schema_numeric_id: u32,
    /// Versioned payload schema.
    pub schema_version: u32,
    /// Strictly increasing publisher-local sequence.
    pub sequence: u64,
    /// Payload bytes.
    pub payload: &'a [u8],
}

/// Hard typed-message bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageLimits {
    /// Maximum message records.
    pub max_messages: u64,
    /// Maximum compiled message schema rows inspected at the barrier.
    pub max_contracts: u64,
    /// Maximum aggregate payload bytes.
    pub max_total_payload_bytes: u64,
}

/// Validates a bounded typed message batch.
///
/// # Errors
///
/// Returns the first count, aggregate byte, ordering, schema, version, or
/// per-schema payload violation.
pub fn validate_messages(
    messages: &[MessageRecord<'_>],
    contracts: &[MessageContract],
    limits: MessageLimits,
) -> Result<(), AbiContractError> {
    let count = slice_len_u64(messages.len(), "message count")?;
    enforce_limit("message count", count, limits.max_messages)?;
    let contract_count = slice_len_u64(contracts.len(), "message contract count")?;
    enforce_limit(
        "message contract count",
        contract_count,
        limits.max_contracts,
    )?;
    let mut total_payload = 0_u64;
    for message in messages {
        total_payload = total_payload
            .checked_add(slice_len_u64(message.payload.len(), "message payload")?)
            .ok_or(AbiContractError::ByteCountOverflow("total message payload"))?;
    }
    enforce_limit(
        "total message payload bytes",
        total_payload,
        limits.max_total_payload_bytes,
    )?;
    let mut previous_contract = None;
    for contract in contracts {
        if contract.schema_numeric_id == 0 {
            return Err(AbiContractError::ZeroNumericId("message schema"));
        }
        if contract.min_schema_version > contract.max_schema_version {
            return Err(AbiContractError::InvalidMessageContract {
                schema_numeric_id: contract.schema_numeric_id,
            });
        }
        if let Some(previous) = previous_contract
            && contract.schema_numeric_id <= previous
        {
            return Err(AbiContractError::NonCanonicalContractOrder("message"));
        }
        previous_contract = Some(contract.schema_numeric_id);
    }
    let mut previous_sequence = None;
    for message in messages {
        if let Some(previous) = previous_sequence
            && message.sequence <= previous
        {
            return Err(AbiContractError::NonIncreasingSequence {
                domain: "message",
                previous,
                actual: message.sequence,
            });
        }
        previous_sequence = Some(message.sequence);
        let contract = contracts
            .binary_search_by_key(&message.schema_numeric_id, |contract| {
                contract.schema_numeric_id
            })
            .ok()
            .map(|index| &contracts[index])
            .ok_or(AbiContractError::MessageSchemaNotPermitted(
                message.schema_numeric_id,
            ))?;
        if !(contract.min_schema_version..=contract.max_schema_version)
            .contains(&message.schema_version)
        {
            return Err(AbiContractError::MessageVersionMismatch {
                schema_numeric_id: message.schema_numeric_id,
                minimum: contract.min_schema_version,
                maximum: contract.max_schema_version,
                actual: message.schema_version,
            });
        }
        let payload_len = slice_len_u64(message.payload.len(), "message payload")?;
        enforce_limit(
            "message schema payload bytes",
            payload_len,
            contract.max_payload_bytes,
        )?;
    }
    Ok(())
}

/// Stable validation failures suitable for owner-aware diagnostic mapping.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum AbiContractError {
    /// Validator policy itself is inconsistent.
    #[error("header policy has an inverted minor range")]
    InvalidHeaderPolicy,
    /// ABI magic does not spell `LAXB` in memory.
    #[error("wrong ABI magic 0x{actual:08x}; expected 0x{LAX_ABI_MAGIC:08x}")]
    WrongMagic {
        /// Actual numeric magic.
        actual: u32,
    },
    /// ABI major is incompatible.
    #[error("unsupported ABI major {actual}; expected {expected}")]
    UnsupportedMajor {
        /// Expected major.
        expected: u16,
        /// Actual major.
        actual: u16,
    },
    /// ABI minor is outside the explicit accepted range.
    #[error("unsupported ABI minor {actual}; expected {minimum}..={maximum}")]
    UnsupportedMinor {
        /// Minimum accepted minor.
        minimum: u16,
        /// Maximum accepted minor.
        maximum: u16,
        /// Actual minor.
        actual: u16,
    },
    /// Structure does not cover required fields.
    #[error("short ABI structure: {actual} bytes; at least {minimum} required")]
    ShortStructure {
        /// Minimum required bytes.
        minimum: u32,
        /// Caller-declared bytes.
        actual: u32,
    },
    /// An unknown high must-understand header flag is set.
    #[error("unknown required ABI header flags 0x{0:08x}")]
    UnknownRequiredFlags(u32),
    /// Required-zero field is nonzero.
    #[error("required-zero field `{field}` is {actual}")]
    ReservedFieldNonZero {
        /// Stable field key.
        field: &'static str,
        /// Actual field value.
        actual: u64,
    },
    /// Target endianness does not match.
    #[error("target endianness {actual} does not match required {expected}")]
    WrongEndianness {
        /// Expected discriminator.
        expected: u8,
        /// Actual discriminator.
        actual: u8,
    },
    /// Target pointer width does not match.
    #[error("pointer width {actual} does not match required {expected}")]
    WrongPointerWidth {
        /// Expected width.
        expected: u8,
        /// Actual width.
        actual: u8,
    },
    /// C calling ABI does not match.
    #[error("C calling ABI {actual} does not match required {expected}")]
    WrongCallingAbi {
        /// Expected discriminator.
        expected: u16,
        /// Actual discriminator.
        actual: u16,
    },
    /// A nonempty span has a null pointer.
    #[error("{0} is nonempty but has a null pointer")]
    NullNonemptySpan(&'static str),
    /// Address plus capacity/extent overflowed 64 bits.
    #[error("{0} address range overflows 64 bits")]
    AddressRangeOverflow(&'static str),
    /// Owned initialized length exceeds capacity.
    #[error("owned buffer length {len} exceeds capacity {capacity}")]
    LengthExceedsCapacity {
        /// Initialized bytes.
        len: u64,
        /// Allocation capacity.
        capacity: u64,
    },

    /// Owned-buffer flags are unknown in ABI 0.1.
    #[error("unknown owned-buffer flags 0x{0:08x}")]
    UnknownOwnedBufferFlags(u32),
    /// Alignment is not a nonzero power of two or ABI-POD alignment.
    #[error("invalid ABI alignment {0}")]
    InvalidAlignment(u32),
    /// Address does not satisfy declared alignment.
    #[error("address 0x{address:x} is not aligned to {alignment}")]
    MisalignedAddress {
        /// Base address.
        address: u64,
        /// Required alignment.
        alignment: u32,
    },
    /// Nonempty owned buffer lacks an allocator-side release callback.
    #[error("nonempty owned buffer is missing its allocator-side release callback")]
    MissingOwnedBufferRelease,
    /// A checked byte total overflowed.
    #[error("{0} byte count overflow")]
    ByteCountOverflow(&'static str),
    /// A checked item count overflowed.
    #[error("{0} count overflow")]
    CountOverflow(&'static str),
    /// A declared hard bound was exceeded.
    #[error("{domain} exceeds hard limit {limit}: {actual}")]
    BudgetExceeded {
        /// Bound domain.
        domain: &'static str,
        /// Declared hard limit.
        limit: u64,
        /// Observed value.
        actual: u64,
    },
    /// Numeric IDs reserve zero.
    #[error("{0} numeric ID must be nonzero")]
    ZeroNumericId(&'static str),
    /// Columns must be strictly ordered and duplicate-free.
    #[error("batch {batch_index} column ID {actual} does not follow {previous}")]
    NonCanonicalColumnOrder {
        /// Batch index.
        batch_index: usize,
        /// Previous component ID.
        previous: u32,
        /// Invalid component ID.
        actual: u32,
    },
    /// Layout hash is the invalid all-zero sentinel.
    #[error("component {component_numeric_id} has an all-zero layout hash")]
    ZeroLayoutHash {
        /// Process-local component ID.
        component_numeric_id: u32,
    },
    /// Column flags contain unknown bits.
    #[error("unknown column flags 0x{0:08x}")]
    UnknownColumnFlags(u32),
    /// Column has neither read nor write permission.
    #[error("component {component_numeric_id} column has no read or write access mode")]
    ColumnHasNoAccessMode {
        /// Process-local component ID.
        component_numeric_id: u32,
    },
    /// Present column count differs from the entity count.
    #[error(
        "component {component_numeric_id} column count {column} differs from entity count {entities}"
    )]
    ColumnCountMismatch {
        /// Process-local component ID.
        component_numeric_id: u32,
        /// Entity count.
        entities: u64,
        /// Column count.
        column: u64,
    },
    /// Column stride cannot contain one initialized element.
    #[error("column stride {stride} is smaller than element size {element_size}")]
    InvalidColumnStride {
        /// Byte stride.
        stride: u64,
        /// Initialized element bytes.
        element_size: u64,
    },
    /// Structural command opcode is unknown.
    #[error("unknown structural command opcode {0}")]
    UnknownCommandOpcode(u32),
    /// ABI 0.1 command flags are nonzero.
    #[error("unknown command flags 0x{0:08x}")]
    UnknownCommandFlags(u32),
    /// Record sequence is not strictly increasing.
    #[error("{domain} sequence {actual} does not follow {previous}")]
    NonIncreasingSequence {
        /// Sequence domain.
        domain: &'static str,
        /// Previous sequence.
        previous: u64,
        /// Invalid next sequence.
        actual: u64,
    },
    /// Command permission/schema contract is absent.
    #[error("command opcode {opcode} subject {subject_numeric_id} is not permitted")]
    CommandNotPermitted {
        /// Wire opcode.
        opcode: u32,
        /// Process-local subject ID.
        subject_numeric_id: u32,
    },
    /// Command layout/payload schema hash differs.
    #[error("command opcode {opcode} subject {subject_numeric_id} has the wrong schema hash")]
    CommandSchemaMismatch {
        /// Wire opcode.
        opcode: u32,
        /// Process-local subject ID.
        subject_numeric_id: u32,
    },
    /// Command payload size is outside its schema contract.
    #[error("command payload size {actual} is outside {minimum}..={maximum}")]
    CommandPayloadSize {
        /// Minimum payload bytes.
        minimum: u64,
        /// Maximum payload bytes.
        maximum: u64,
        /// Actual payload bytes.
        actual: u64,
    },
    /// Contract rows are not strictly ordered and duplicate-free.
    #[error("{0} contracts are not in canonical order")]
    NonCanonicalContractOrder(&'static str),
    /// Handle is partially zero instead of the all-zero invalid sentinel.
    #[error("malformed partially-zero handle")]
    MalformedHandle,
    /// Command requiring no target received a live-looking handle.
    #[error("command requires the all-zero target sentinel")]
    UnexpectedHandle,
    /// Handle belongs to another engine-instance/service table.
    #[error("handle table {actual} does not match expected {expected}")]
    WrongHandleTable {
        /// Expected table ID.
        expected: u64,
        /// Actual table ID.
        actual: u64,
    },
    /// Handle slot is absent from the live snapshot.
    #[error("handle slot {slot} is not live")]
    UnknownHandleSlot {
        /// Missing slot.
        slot: u32,
    },
    /// Handle uses an older or otherwise mismatched generation.
    #[error("handle slot {slot} generation {actual} does not match current {expected}")]
    StaleHandleGeneration {
        /// Slot number.
        slot: u32,
        /// Current generation.
        expected: u32,
        /// Supplied generation.
        actual: u32,
    },
    /// Message schema is outside the compiled permission closure.
    #[error("message schema {0} is not permitted")]
    MessageSchemaNotPermitted(u32),
    /// Message schema version is outside its explicit accepted range.
    #[error("message schema {schema_numeric_id} version {actual} is outside {minimum}..={maximum}")]
    MessageVersionMismatch {
        /// Process-local schema ID.
        schema_numeric_id: u32,
        /// Minimum version.
        minimum: u32,
        /// Maximum version.
        maximum: u32,
        /// Actual version.
        actual: u32,
    },
    /// Message contract has an inverted version range.
    #[error("message schema {schema_numeric_id} contract has an inverted version range")]
    InvalidMessageContract {
        /// Process-local schema ID.
        schema_numeric_id: u32,
    },
}

fn validate_address_extent(
    address: Option<u64>,
    len: u64,
    domain: &'static str,
) -> Result<(), AbiContractError> {
    if len == 0 {
        return Ok(());
    }
    let address = address.ok_or(AbiContractError::NullNonemptySpan(domain))?;
    address
        .checked_add(len)
        .ok_or(AbiContractError::AddressRangeOverflow(domain))?;
    Ok(())
}

fn pointer_address<T>(pointer: *const T) -> Option<u64> {
    if pointer.is_null() {
        None
    } else {
        Some(u64::from_ne_bytes(pointer.addr().to_ne_bytes()))
    }
}

fn validate_abi_alignment(alignment: u32) -> Result<(), AbiContractError> {
    if matches!(alignment, 1 | 2 | 4 | 8 | 16) {
        Ok(())
    } else {
        Err(AbiContractError::InvalidAlignment(alignment))
    }
}

fn enforce_limit(domain: &'static str, actual: u64, limit: u64) -> Result<(), AbiContractError> {
    if actual > limit {
        Err(AbiContractError::BudgetExceeded {
            domain,
            limit,
            actual,
        })
    } else {
        Ok(())
    }
}

fn slice_len_u64(length: usize, domain: &'static str) -> Result<u64, AbiContractError> {
    u64::try_from(length).map_err(|_| AbiContractError::CountOverflow(domain))
}

fn column_extent(count: u64, stride: u64, element_size: u64) -> Result<u64, AbiContractError> {
    if count == 0 {
        return Ok(0);
    }
    (count - 1)
        .checked_mul(stride)
        .and_then(|prefix| prefix.checked_add(element_size))
        .ok_or(AbiContractError::ByteCountOverflow("column extent"))
}

fn validate_command_contract_order(contracts: &[CommandContract]) -> Result<(), AbiContractError> {
    let mut previous = None;
    for contract in contracts {
        if contract.min_payload_bytes > contract.max_payload_bytes {
            return Err(AbiContractError::NonCanonicalContractOrder("command"));
        }
        let key = (contract.kind, contract.subject_numeric_id);
        if let Some(previous) = previous
            && key <= previous
        {
            return Err(AbiContractError::NonCanonicalContractOrder("command"));
        }
        previous = Some(key);
    }
    Ok(())
}

fn validate_handle_snapshot(snapshot: HandleTableSnapshot<'_>) -> Result<(), AbiContractError> {
    if snapshot.table_id == 0 {
        return Err(AbiContractError::WrongHandleTable {
            expected: 1,
            actual: 0,
        });
    }
    let mut previous = None;
    for entry in snapshot.live {
        if entry.generation == 0 {
            return Err(AbiContractError::MalformedHandle);
        }
        if let Some(previous) = previous
            && entry.slot <= previous
        {
            return Err(AbiContractError::NonCanonicalContractOrder(
                "handle snapshot",
            ));
        }
        previous = Some(entry.slot);
    }
    Ok(())
}

fn validate_absent_handle(handle: LaxOpaqueHandle) -> Result<(), AbiContractError> {
    if handle.is_invalid() {
        return Ok(());
    }
    if handle.table_id == 0 || handle.generation == 0 {
        return Err(AbiContractError::MalformedHandle);
    }
    Err(AbiContractError::UnexpectedHandle)
}

fn validate_live_handle(
    handle: LaxOpaqueHandle,
    snapshot: HandleTableSnapshot<'_>,
) -> Result<(), AbiContractError> {
    if handle.table_id == 0 || handle.generation == 0 {
        return Err(AbiContractError::MalformedHandle);
    }
    if handle.table_id != snapshot.table_id {
        return Err(AbiContractError::WrongHandleTable {
            expected: snapshot.table_id,
            actual: handle.table_id,
        });
    }
    let index = snapshot
        .live
        .binary_search_by_key(&handle.slot, |entry| entry.slot)
        .map_err(|_| AbiContractError::UnknownHandleSlot { slot: handle.slot })?;
    let current = snapshot.live[index].generation;
    if handle.generation != current {
        return Err(AbiContractError::StaleHandleGeneration {
            slot: handle.slot,
            expected: current,
            actual: handle.generation,
        });
    }
    Ok(())
}
