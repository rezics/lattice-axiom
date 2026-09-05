//! Malformed, overflow, exact-boundary, and boundary-plus-one corpus.

use core::ptr;

use latticeaxiom_abi::{
    AbiContractError, BatchLimits, BatchShape, ColumnShape, CommandContract, CommandKind,
    CommandLimits, CommandRecord, HandleGeneration, HandleTableSnapshot, HeaderPolicy,
    LAX_ABI_MAGIC, LAX_COLUMN_FLAG_READ, LAX_HEADER_REQUIRED_MASK, LaxAbiHeader, LaxBytes,
    LaxEntityHandle, MessageContract, MessageLimits, MessageRecord, OwnedBufferShape,
    ScratchRequest, TargetPolicy, TargetRequirement, validate_batches, validate_borrowed_bytes,
    validate_commands, validate_header, validate_messages, validate_owned_buffer,
    validate_scratch_request, validate_target,
};

const HEADER_POLICY: HeaderPolicy = HeaderPolicy {
    expected_major: 0,
    min_minor: 1,
    max_minor: 1,
    minimum_struct_size: 16,
    known_required_flags: 0,
};

#[test]
fn header_and_target_malformed_corpus_fails_closed() {
    let good = LaxAbiHeader::new(0, 1, 16, 0x0000_a55a);
    let inspection = validate_header(&good, HEADER_POLICY);
    assert!(inspection.is_ok());
    assert_eq!(
        inspection.ok().map(|value| value.advisory_flags),
        Some(0xa55a)
    );

    let wrong_magic = LaxAbiHeader { magic: 0, ..good };
    assert!(matches!(
        validate_header(&wrong_magic, HEADER_POLICY),
        Err(AbiContractError::WrongMagic { .. })
    ));
    let wrong_major = LaxAbiHeader {
        abi_major: 1,
        ..good
    };
    assert!(matches!(
        validate_header(&wrong_major, HEADER_POLICY),
        Err(AbiContractError::UnsupportedMajor { .. })
    ));
    let wrong_minor = LaxAbiHeader {
        abi_minor: 2,
        ..good
    };
    assert!(matches!(
        validate_header(&wrong_minor, HEADER_POLICY),
        Err(AbiContractError::UnsupportedMinor { .. })
    ));
    let short = LaxAbiHeader {
        struct_size: 15,
        ..good
    };
    assert!(matches!(
        validate_header(&short, HEADER_POLICY),
        Err(AbiContractError::ShortStructure { .. })
    ));
    let unknown_required = LaxAbiHeader {
        flags: LAX_HEADER_REQUIRED_MASK & (1 << 31),
        ..good
    };
    assert!(matches!(
        validate_header(&unknown_required, HEADER_POLICY),
        Err(AbiContractError::UnknownRequiredFlags(_))
    ));
    assert_eq!(good.magic, LAX_ABI_MAGIC);

    let expected = TargetPolicy {
        endianness: 1,
        pointer_width_bits: 64,
        c_call_abi: 1,
    };
    assert_eq!(validate_target(expected, expected, 0, 0), Ok(()));
    assert!(matches!(
        validate_target(
            TargetPolicy {
                pointer_width_bits: 32,
                ..expected
            },
            expected,
            0,
            0
        ),
        Err(AbiContractError::WrongPointerWidth { .. })
    ));
    assert!(matches!(
        validate_target(expected, expected, 1, 0),
        Err(AbiContractError::ReservedFieldNonZero { .. })
    ));
}

#[test]
fn span_owned_buffer_and_scratch_boundary_plus_one() {
    let empty = LaxBytes {
        data: ptr::null(),
        len: 0,
    };
    assert_eq!(validate_borrowed_bytes(&empty), Ok(()));
    let null_nonempty = LaxBytes {
        data: ptr::null(),
        len: 1,
    };
    assert!(matches!(
        validate_borrowed_bytes(&null_nonempty),
        Err(AbiContractError::NullNonemptySpan(_))
    ));
    let overflowing = LaxBytes {
        data: ptr::without_provenance(usize::MAX),
        len: 2,
    };
    assert!(matches!(
        validate_borrowed_bytes(&overflowing),
        Err(AbiContractError::AddressRangeOverflow(_))
    ));

    let valid = OwnedBufferShape {
        data_address: Some(0x1000),
        len: 16,
        capacity: 16,
        alignment: 16,
        flags: 0,
        has_release: true,
    };
    assert_eq!(validate_owned_buffer(valid), Ok(()));
    assert_eq!(
        validate_owned_buffer(OwnedBufferShape {
            data_address: None,
            len: 0,
            capacity: 0,
            alignment: 1,
            flags: 0,
            has_release: false,
        }),
        Ok(())
    );
    assert!(matches!(
        validate_owned_buffer(OwnedBufferShape { len: 17, ..valid }),
        Err(AbiContractError::LengthExceedsCapacity { .. })
    ));
    assert!(matches!(
        validate_owned_buffer(OwnedBufferShape {
            has_release: false,
            ..valid
        }),
        Err(AbiContractError::MissingOwnedBufferRelease)
    ));

    let scratch = ScratchRequest {
        used_bytes: 8,
        requested_bytes: 8,
        alignment: 8,
        byte_budget: 16,
    };
    assert_eq!(validate_scratch_request(scratch), Ok(16));
    assert!(matches!(
        validate_scratch_request(ScratchRequest {
            requested_bytes: 9,
            ..scratch
        }),
        Err(AbiContractError::BudgetExceeded { .. })
    ));
}

#[test]
fn batch_bounds_and_checked_extent_are_enforced() {
    let column = ColumnShape {
        component_numeric_id: 7,
        layout_hash: [3; 32],
        data_address: Some(0x1000),
        count: 2,
        stride: 16,
        element_size: 12,
        alignment: 16,
        flags: LAX_COLUMN_FLAG_READ,
        reserved: 0,
    };
    let columns = [column];
    let batches = [BatchShape {
        entity_count: 2,
        columns: &columns,
    }];
    let limits = BatchLimits {
        max_batches: 1,
        max_entities_per_batch: 2,
        max_total_entities: 2,
        max_columns_per_batch: 1,
        max_total_columns: 1,
        max_total_bytes: 28,
    };
    assert_eq!(validate_batches(&batches, limits), Ok(()));
    assert!(matches!(
        validate_batches(
            &batches,
            BatchLimits {
                max_total_bytes: 27,
                ..limits
            }
        ),
        Err(AbiContractError::BudgetExceeded { .. })
    ));

    let overflow_column = ColumnShape {
        count: u64::MAX,
        stride: u64::MAX,
        ..column
    };
    let overflow_columns = [overflow_column];
    let overflow_batches = [BatchShape {
        entity_count: u64::MAX,
        columns: &overflow_columns,
    }];
    let unlimited = BatchLimits {
        max_batches: 1,
        max_entities_per_batch: u64::MAX,
        max_total_entities: u64::MAX,
        max_columns_per_batch: 1,
        max_total_columns: 1,
        max_total_bytes: u64::MAX,
    };
    assert!(matches!(
        validate_batches(&overflow_batches, unlimited),
        Err(AbiContractError::ByteCountOverflow(_))
    ));
}

#[test]
fn command_batch_rejects_previous_generation_and_boundary_plus_one() {
    let hash = [9; 32];
    let contracts = [CommandContract {
        kind: CommandKind::AddComponent,
        subject_numeric_id: 12,
        layout_hash: hash,
        min_payload_bytes: 4,
        max_payload_bytes: 4,
        target: TargetRequirement::ExistingEntity,
    }];
    let live = [HandleGeneration {
        slot: 5,
        generation: 3,
    }];
    let table = HandleTableSnapshot {
        table_id: 41,
        live: &live,
    };
    let payload = [1, 2, 3, 4];
    let valid = CommandRecord {
        sequence: 1,
        opcode: CommandKind::AddComponent.wire_code(),
        flags: 0,
        target: LaxEntityHandle::from_wire(41, 5, 3),
        subject_numeric_id: 12,
        reserved: 0,
        layout_hash: hash,
        payload: &payload,
    };
    let limits = CommandLimits {
        max_commands: 1,
        max_contracts: 1,
        max_live_handles: 1,
        max_payload_bytes_per_command: 4,
        max_total_payload_bytes: 4,
    };
    assert_eq!(
        validate_commands(&[valid], &contracts, table, limits),
        Ok(())
    );
    assert!(matches!(
        validate_commands(
            &[valid],
            &contracts,
            table,
            CommandLimits {
                max_contracts: 0,
                ..limits
            }
        ),
        Err(AbiContractError::BudgetExceeded { .. })
    ));
    assert!(matches!(
        validate_commands(
            &[valid],
            &contracts,
            table,
            CommandLimits {
                max_live_handles: 0,
                ..limits
            }
        ),
        Err(AbiContractError::BudgetExceeded { .. })
    ));

    let previous_generation = CommandRecord {
        target: LaxEntityHandle::from_wire(41, 5, 2),
        ..valid
    };
    assert!(matches!(
        validate_commands(&[previous_generation], &contracts, table, limits),
        Err(AbiContractError::StaleHandleGeneration {
            expected: 3,
            actual: 2,
            ..
        })
    ));
    assert!(matches!(
        validate_commands(
            &[valid],
            &contracts,
            table,
            CommandLimits {
                max_total_payload_bytes: 3,
                ..limits
            }
        ),
        Err(AbiContractError::BudgetExceeded { .. })
    ));
}

#[test]
fn message_schema_version_order_and_limits_are_bounded() {
    let contracts = [MessageContract {
        schema_numeric_id: 3,
        min_schema_version: 1,
        max_schema_version: 2,
        max_payload_bytes: 3,
    }];
    let payload = [1, 2, 3];
    let message = MessageRecord {
        schema_numeric_id: 3,
        schema_version: 2,
        sequence: 7,
        payload: &payload,
    };
    let limits = MessageLimits {
        max_messages: 1,
        max_contracts: 1,
        max_total_payload_bytes: 3,
    };
    assert_eq!(validate_messages(&[message], &contracts, limits), Ok(()));
    assert!(matches!(
        validate_messages(
            &[message],
            &contracts,
            MessageLimits {
                max_contracts: 0,
                ..limits
            }
        ),
        Err(AbiContractError::BudgetExceeded { .. })
    ));
    assert!(matches!(
        validate_messages(
            &[MessageRecord {
                schema_version: 3,
                ..message
            }],
            &contracts,
            limits
        ),
        Err(AbiContractError::MessageVersionMismatch { .. })
    ));
    assert!(matches!(
        validate_messages(
            &[message],
            &contracts,
            MessageLimits {
                max_total_payload_bytes: 2,
                ..limits
            }
        ),
        Err(AbiContractError::BudgetExceeded { .. })
    ));
}
