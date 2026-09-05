//! Typed source schema for Portable Native ABI 0.1.
//!
//! `build.rs` consumes this file to generate both the Rust `repr(C)` bindings
//! and the public C header. Keep semantic validation outside this schema: this
//! file freezes only wire names, field order, sizes, alignment, and callbacks.

#[derive(Clone, Copy)]
pub(crate) enum WireType {
    U8,
    U16,
    U32,
    U64,
    ByteArray(usize),
    Named(&'static str, usize, usize),
    ConstPointer(&'static str, &'static str),
    MutPointer(&'static str, &'static str),
    Callback(&'static str),
}

impl WireType {
    pub(crate) const fn size(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 | Self::ConstPointer(_, _) | Self::MutPointer(_, _) | Self::Callback(_) => 8,
            Self::ByteArray(length) => length,
            Self::Named(_, size, _) => size,
        }
    }

    pub(crate) const fn alignment(self) -> usize {
        match self {
            Self::U8 | Self::ByteArray(_) => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 | Self::ConstPointer(_, _) | Self::MutPointer(_, _) | Self::Callback(_) => 8,
            Self::Named(_, _, alignment) => alignment,
        }
    }

    pub(crate) fn rust_type(self) -> String {
        match self {
            Self::U8 => "u8".into(),
            Self::U16 => "u16".into(),
            Self::U32 => "u32".into(),
            Self::U64 => "u64".into(),
            Self::ByteArray(length) => format!("[u8; {length}]"),
            Self::Named(name, _, _) | Self::Callback(name) => name.into(),
            Self::ConstPointer(rust, _) => format!("*const {rust}"),
            Self::MutPointer(rust, _) => format!("*mut {rust}"),
        }
    }

    pub(crate) fn c_declaration(self, field_name: &str) -> String {
        match self {
            Self::U8 => format!("uint8_t {field_name}"),
            Self::U16 => format!("uint16_t {field_name}"),
            Self::U32 => format!("uint32_t {field_name}"),
            Self::U64 => format!("uint64_t {field_name}"),
            Self::ByteArray(length) => format!("uint8_t {field_name}[{length}]"),
            Self::Named(name, _, _) | Self::Callback(name) => format!("{name} {field_name}"),
            Self::ConstPointer(_, c) => format!("const {c}* {field_name}"),
            Self::MutPointer(_, c) => format!("{c}* {field_name}"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Field {
    pub(crate) name: &'static str,
    pub(crate) docs: &'static str,
    pub(crate) wire_type: WireType,
    pub(crate) offset: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct StructDef {
    pub(crate) name: &'static str,
    pub(crate) docs: &'static str,
    pub(crate) size: usize,
    pub(crate) alignment: usize,
    pub(crate) fields: &'static [Field],
}

#[derive(Clone, Copy)]
pub(crate) struct CallbackDef {
    pub(crate) name: &'static str,
    pub(crate) docs: &'static str,
    pub(crate) rust_signature: &'static str,
    pub(crate) c_signature: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct ConstantDef {
    pub(crate) name: &'static str,
    pub(crate) docs: &'static str,
    pub(crate) rust_type: &'static str,
    pub(crate) rust_value: &'static str,
    pub(crate) c_value: &'static str,
}

macro_rules! constant {
    ($name:literal, $docs:literal, $rust_type:literal, $rust_value:literal, $c_value:literal) => {
        ConstantDef {
            name: $name,
            docs: $docs,
            rust_type: $rust_type,
            rust_value: $rust_value,
            c_value: $c_value,
        }
    };
}

pub(crate) const CONSTANTS: &[ConstantDef] = &[
    constant!(
        "LAX_ABI_MAGIC",
        "Memory bytes `LAXB` interpreted as a little-endian `u32`.",
        "u32",
        "0x4258_414c",
        "UINT32_C(0x4258414c)"
    ),
    constant!(
        "LAX_ABI_MAJOR",
        "Bootstrap ABI 0.x major.",
        "u16",
        "0",
        "UINT16_C(0)"
    ),
    constant!(
        "LAX_ABI_MINOR",
        "Bootstrap ABI 0.1 minor epoch.",
        "u16",
        "1",
        "UINT16_C(1)"
    ),
    constant!(
        "LAX_ENDIANNESS_LITTLE",
        "Portable ABI little-endian target discriminator.",
        "u8",
        "1",
        "UINT8_C(1)"
    ),
    constant!(
        "LAX_POINTER_WIDTH_BITS",
        "Portable ABI pointer width.",
        "u8",
        "64",
        "UINT8_C(64)"
    ),
    constant!(
        "LAX_C_CALL_ABI_PLATFORM",
        "Platform C calling convention discriminator.",
        "u16",
        "1",
        "UINT16_C(1)"
    ),
    constant!(
        "LAX_STATUS_OK",
        "Successful ABI call.",
        "LaxStatus",
        "0",
        "UINT32_C(0)"
    ),
    constant!(
        "LAX_STATUS_INVALID_ARGUMENT",
        "Malformed argument or record.",
        "LaxStatus",
        "1",
        "UINT32_C(1)"
    ),
    constant!(
        "LAX_STATUS_UNSUPPORTED_VERSION",
        "Unsupported ABI or interface version.",
        "LaxStatus",
        "2",
        "UINT32_C(2)"
    ),
    constant!(
        "LAX_STATUS_CONTRACT_VIOLATION",
        "ABI ownership, layout, permission, or lifecycle contract violation.",
        "LaxStatus",
        "3",
        "UINT32_C(3)"
    ),
    constant!(
        "LAX_STATUS_RESOURCE_EXHAUSTED",
        "A declared hard resource budget was exhausted.",
        "LaxStatus",
        "4",
        "UINT32_C(4)"
    ),
    constant!(
        "LAX_STATUS_PERMISSION_DENIED",
        "The package lacks the required permission.",
        "LaxStatus",
        "5",
        "UINT32_C(5)"
    ),
    constant!(
        "LAX_STATUS_STALE_HANDLE",
        "A generational handle is stale or belongs to the wrong table.",
        "LaxStatus",
        "6",
        "UINT32_C(6)"
    ),
    constant!(
        "LAX_STATUS_DOMAIN_REJECTED",
        "A declared recoverable domain operation rejected its staged result.",
        "LaxStatus",
        "7",
        "UINT32_C(7)"
    ),
    constant!(
        "LAX_STATUS_INSTANCE_FAILED",
        "The owning engine instance has failed.",
        "LaxStatus",
        "8",
        "UINT32_C(8)"
    ),
    constant!(
        "LAX_STATUS_DEADLINE_OVERRUN",
        "A callback returned after its soft monotonic deadline.",
        "LaxStatus",
        "9",
        "UINT32_C(9)"
    ),
    constant!(
        "LAX_STATUS_INTERNAL_PANIC",
        "An SDK boundary caught a language panic or exception.",
        "LaxStatus",
        "10",
        "UINT32_C(10)"
    ),
    constant!(
        "LAX_STATUS_BUFFER_TOO_SMALL",
        "A caller-provided output structure or buffer is too small.",
        "LaxStatus",
        "11",
        "UINT32_C(11)"
    ),
    constant!(
        "LAX_HEADER_ADVISORY_MASK",
        "Header low flags that inspectors preserve even when unknown.",
        "u32",
        "0x0000_ffff",
        "UINT32_C(0x0000ffff)"
    ),
    constant!(
        "LAX_HEADER_REQUIRED_MASK",
        "Header high flags that require explicit understanding.",
        "u32",
        "0xffff_0000",
        "UINT32_C(0xffff0000)"
    ),
    constant!(
        "LAX_INTERFACE_FLAG_HOST_WORKER_SAFE",
        "Interface callback may run on a host worker.",
        "u32",
        "1",
        "UINT32_C(1)"
    ),
    constant!(
        "LAX_INTERFACE_FLAG_CONCURRENT_BATCHES",
        "Interface callback permits concurrent batches for one instance.",
        "u32",
        "2",
        "UINT32_C(2)"
    ),
    constant!(
        "LAX_INTERFACE_FLAG_MAY_BLOCK",
        "Interface callback may block.",
        "u32",
        "4",
        "UINT32_C(4)"
    ),
    constant!(
        "LAX_INTERFACE_FLAG_REENTRANT",
        "Interface callback permits synchronous host re-entry.",
        "u32",
        "8",
        "UINT32_C(8)"
    ),
    constant!(
        "LAX_INTERFACE_FLAGS_KNOWN",
        "All interface flags understood by ABI 0.1.",
        "u32",
        "15",
        "UINT32_C(15)"
    ),
    constant!(
        "LAX_COLUMN_FLAG_READ",
        "Column may be read by the foreign callback.",
        "u32",
        "1",
        "UINT32_C(1)"
    ),
    constant!(
        "LAX_COLUMN_FLAG_WRITE",
        "Column may be written by the foreign callback.",
        "u32",
        "2",
        "UINT32_C(2)"
    ),
    constant!(
        "LAX_COLUMN_FLAG_OPTIONAL",
        "Column may be absent from a batch.",
        "u32",
        "4",
        "UINT32_C(4)"
    ),
    constant!(
        "LAX_COLUMN_FLAGS_KNOWN",
        "All column flags understood by ABI 0.1.",
        "u32",
        "7",
        "UINT32_C(7)"
    ),
    constant!(
        "LAX_COMMAND_OPCODE_SPAWN",
        "Spawn an entity through a versioned payload.",
        "u32",
        "1",
        "UINT32_C(1)"
    ),
    constant!(
        "LAX_COMMAND_OPCODE_DESPAWN",
        "Despawn an existing entity.",
        "u32",
        "2",
        "UINT32_C(2)"
    ),
    constant!(
        "LAX_COMMAND_OPCODE_ADD_COMPONENT",
        "Add a component through a versioned payload.",
        "u32",
        "3",
        "UINT32_C(3)"
    ),
    constant!(
        "LAX_COMMAND_OPCODE_REMOVE_COMPONENT",
        "Remove a component.",
        "u32",
        "4",
        "UINT32_C(4)"
    ),
    constant!(
        "LAX_COMMAND_OPCODE_ASSET",
        "Submit a versioned asset command.",
        "u32",
        "5",
        "UINT32_C(5)"
    ),
    constant!(
        "LAX_COMMAND_OPCODE_WORLD",
        "Submit a versioned world command.",
        "u32",
        "6",
        "UINT32_C(6)"
    ),
    constant!(
        "LAX_DIAGNOSTIC_INFO",
        "Informational diagnostic severity.",
        "u32",
        "1",
        "UINT32_C(1)"
    ),
    constant!(
        "LAX_DIAGNOSTIC_WARNING",
        "Warning diagnostic severity.",
        "u32",
        "2",
        "UINT32_C(2)"
    ),
    constant!(
        "LAX_DIAGNOSTIC_ERROR",
        "Error diagnostic severity.",
        "u32",
        "3",
        "UINT32_C(3)"
    ),
    constant!(
        "LAX_DIAGNOSTIC_FATAL",
        "Fatal diagnostic severity.",
        "u32",
        "4",
        "UINT32_C(4)"
    ),
];

macro_rules! field {
    ($name:literal, $docs:literal, $wire_type:expr, $offset:literal) => {
        Field {
            name: $name,
            docs: $docs,
            wire_type: $wire_type,
            offset: $offset,
        }
    };
}

const HASH_256_FIELDS: &[Field] = &[field!(
    "bytes",
    "The digest bytes in canonical order.",
    WireType::ByteArray(32),
    0
)];

const BYTES_FIELDS: &[Field] = &[
    field!(
        "data",
        "Borrowed bytes; null is allowed only when `len` is zero.",
        WireType::ConstPointer("u8", "uint8_t"),
        0
    ),
    field!("len", "The number of borrowed bytes.", WireType::U64, 8),
];

const MUT_BYTES_FIELDS: &[Field] = &[
    field!(
        "data",
        "Borrowed writable bytes; null is allowed only when `len` is zero.",
        WireType::MutPointer("u8", "uint8_t"),
        0
    ),
    field!("len", "The writable byte count.", WireType::U64, 8),
];

const INTERFACE_ID_FIELDS: &[Field] = &[
    field!(
        "hi",
        "The high 64 bits of the interface token.",
        WireType::U64,
        0
    ),
    field!(
        "lo",
        "The low 64 bits of the interface token.",
        WireType::U64,
        8
    ),
];

const ABI_HEADER_FIELDS: &[Field] = &[
    field!(
        "magic",
        "The fixed little-endian `LAXB` magic.",
        WireType::U32,
        0
    ),
    field!("abi_major", "The ABI major version.", WireType::U16, 4),
    field!("abi_minor", "The ABI minor epoch.", WireType::U16, 6),
    field!(
        "struct_size",
        "The complete caller-visible structure size.",
        WireType::U32,
        8
    ),
    field!(
        "flags",
        "Required high flags and advisory low flags.",
        WireType::U32,
        12
    ),
];

const OPAQUE_HANDLE_FIELDS: &[Field] = &[
    field!(
        "table_id",
        "The process-unique service table identity.",
        WireType::U64,
        0
    ),
    field!(
        "slot",
        "The slot within the service table.",
        WireType::U32,
        8
    ),
    field!(
        "generation",
        "The nonzero slot generation.",
        WireType::U32,
        12
    ),
];

const HANDLE_WRAPPER_FIELDS: &[Field] = &[field!(
    "raw",
    "The kind-specific opaque handle representation.",
    WireType::Named("LaxOpaqueHandle", 16, 8),
    0
)];

const OWNED_BUFFER_FIELDS: &[Field] = &[
    field!(
        "data",
        "Owned allocation pointer.",
        WireType::MutPointer("u8", "uint8_t"),
        0
    ),
    field!("len", "Initialized byte length.", WireType::U64, 8),
    field!(
        "capacity",
        "Allocation capacity returned to the releaser.",
        WireType::U64,
        16
    ),
    field!(
        "alignment",
        "Allocation alignment returned to the releaser.",
        WireType::U32,
        24
    ),
    field!("flags", "Owned-buffer flags.", WireType::U32, 28),
    field!(
        "release_context",
        "Allocator-owned release context.",
        WireType::MutPointer("c_void", "void"),
        32
    ),
    field!(
        "release",
        "Exactly-once allocator-side release callback.",
        WireType::Callback("LaxOwnedBufferReleaseFn"),
        40
    ),
];

const SCRATCH_ARENA_FIELDS: &[Field] = &[
    field!(
        "context",
        "Host-owned scratch allocator context.",
        WireType::MutPointer("c_void", "void"),
        0
    ),
    field!(
        "byte_budget",
        "Hard per-callback scratch byte budget.",
        WireType::U64,
        8
    ),
    field!(
        "alloc",
        "Bump allocation callback; there is no per-allocation free.",
        WireType::Callback("LaxScratchAllocFn"),
        16
    ),
];

const INTERFACE_TABLE_HEADER_FIELDS: &[Field] = &[
    field!(
        "header",
        "The common 16-byte ABI header.",
        WireType::Named("LaxAbiHeader", 16, 4),
        0
    ),
    field!(
        "interface_id",
        "The selected interface identity.",
        WireType::Named("LaxInterfaceId", 16, 8),
        16
    ),
    field!(
        "interface_major",
        "The selected interface major.",
        WireType::U16,
        32
    ),
    field!(
        "interface_minor",
        "The selected interface minor.",
        WireType::U16,
        34
    ),
    field!(
        "interface_flags",
        "Explicit threading and execution opt-ins.",
        WireType::U32,
        36
    ),
    field!(
        "reserved",
        "Required-zero reserved bits.",
        WireType::U32,
        40
    ),
    field!(
        "context",
        "Provider-owned table context.",
        WireType::MutPointer("c_void", "void"),
        48
    ),
];

const INTERFACE_REQUEST_FIELDS: &[Field] = &[
    field!(
        "header",
        "The common 16-byte ABI header.",
        WireType::Named("LaxAbiHeader", 16, 4),
        0
    ),
    field!(
        "interface_id",
        "The requested interface identity.",
        WireType::Named("LaxInterfaceId", 16, 8),
        16
    ),
    field!(
        "min_major",
        "The minimum accepted major.",
        WireType::U16,
        32
    ),
    field!(
        "max_major",
        "The maximum accepted major.",
        WireType::U16,
        34
    ),
    field!(
        "min_minor",
        "The minimum accepted minor.",
        WireType::U16,
        36
    ),
    field!(
        "max_minor",
        "The maximum accepted minor.",
        WireType::U16,
        38
    ),
];

const INTERFACE_RESPONSE_FIELDS: &[Field] = &[
    field!(
        "header",
        "The common 16-byte ABI header.",
        WireType::Named("LaxAbiHeader", 16, 4),
        0
    ),
    field!(
        "interface_id",
        "The selected interface identity.",
        WireType::Named("LaxInterfaceId", 16, 8),
        16
    ),
    field!(
        "selected_major",
        "The selected interface major.",
        WireType::U16,
        32
    ),
    field!(
        "selected_minor",
        "The selected interface minor.",
        WireType::U16,
        34
    ),
    field!(
        "reserved",
        "Required-zero reserved field.",
        WireType::U32,
        36
    ),
    field!(
        "table",
        "Borrowed interface table pointer.",
        WireType::ConstPointer("c_void", "void"),
        40
    ),
];

const ENTRY_REQUEST_FIELDS: &[Field] = &[
    field!(
        "header",
        "The bootstrap ABI header.",
        WireType::Named("LaxAbiHeader", 16, 4),
        0
    ),
    field!(
        "host_min_minor",
        "Minimum bootstrap minor accepted by the host.",
        WireType::U16,
        16
    ),
    field!(
        "host_max_minor",
        "Maximum bootstrap minor accepted by the host.",
        WireType::U16,
        18
    ),
    field!(
        "endianness",
        "Target byte order discriminator.",
        WireType::U8,
        20
    ),
    field!(
        "pointer_width_bits",
        "Target pointer width in bits.",
        WireType::U8,
        21
    ),
    field!(
        "target_reserved",
        "Required-zero target field.",
        WireType::U16,
        22
    ),
    field!(
        "c_call_abi",
        "C calling convention discriminator.",
        WireType::U16,
        24
    ),
    field!(
        "entry_reserved",
        "Required-zero bootstrap field.",
        WireType::U16,
        26
    ),
    field!(
        "engine_build_id",
        "All-zero for portable artifacts; exact for coupled artifacts.",
        WireType::Named("LaxHash256", 32, 1),
        28
    ),
    field!(
        "locked_graph_hash",
        "The selected locked graph hash.",
        WireType::Named("LaxHash256", 32, 1),
        60
    ),
    field!(
        "expected_registration_manifest_hash",
        "The manifest hash expected by the host.",
        WireType::Named("LaxHash256", 32, 1),
        92
    ),
    field!(
        "target_triple",
        "Entry-lifetime UTF-8 target triple.",
        WireType::Named("LaxBytes", 16, 8),
        128
    ),
    field!(
        "locked_package_id",
        "Entry-lifetime UTF-8 package ID.",
        WireType::Named("LaxBytes", 16, 8),
        144
    ),
    field!(
        "locked_realization_id",
        "Entry-lifetime UTF-8 realization ID.",
        WireType::Named("LaxBytes", 16, 8),
        160
    ),
    field!(
        "host_context",
        "Host query context.",
        WireType::MutPointer("c_void", "void"),
        176
    ),
    field!(
        "host_query_interface",
        "Host interface query callback.",
        WireType::Callback("LaxQueryInterfaceFn"),
        184
    ),
    field!(
        "diagnostic_context",
        "Host diagnostic sink context.",
        WireType::MutPointer("c_void", "void"),
        192
    ),
    field!(
        "emit_diagnostic",
        "Host diagnostic sink callback.",
        WireType::Callback("LaxEmitDiagnosticFn"),
        200
    ),
];

const ENTRY_RESPONSE_FIELDS: &[Field] = &[
    field!(
        "header",
        "The bootstrap ABI header.",
        WireType::Named("LaxAbiHeader", 16, 4),
        0
    ),
    field!("module_flags", "Module execution flags.", WireType::U32, 16),
    field!("reserved", "Required-zero module field.", WireType::U32, 20),
    field!(
        "actual_registration_manifest_hash",
        "The manifest hash embedded by the module.",
        WireType::Named("LaxHash256", 32, 1),
        24
    ),
    field!(
        "module_id",
        "Library-mapping-lifetime UTF-8 module ID.",
        WireType::Named("LaxBytes", 16, 8),
        56
    ),
    field!(
        "realization_id",
        "Library-mapping-lifetime UTF-8 realization ID.",
        WireType::Named("LaxBytes", 16, 8),
        72
    ),
    field!(
        "module_context",
        "Module-owned library context.",
        WireType::MutPointer("c_void", "void"),
        88
    ),
    field!(
        "create_instance",
        "Per-engine-instance create callback.",
        WireType::Callback("LaxCreateInstanceFn"),
        96
    ),
    field!(
        "start_instance",
        "Per-engine-instance start callback.",
        WireType::Callback("LaxInstanceLifecycleFn"),
        104
    ),
    field!(
        "stop_instance",
        "Per-engine-instance stop callback.",
        WireType::Callback("LaxInstanceLifecycleFn"),
        112
    ),
    field!(
        "destroy_instance",
        "Per-engine-instance destroy callback.",
        WireType::Callback("LaxInstanceLifecycleFn"),
        120
    ),
    field!(
        "module_query_interface",
        "Module interface query callback.",
        WireType::Callback("LaxQueryInterfaceFn"),
        128
    ),
];

const INSTANCE_CREATE_REQUEST_FIELDS: &[Field] = &[
    field!(
        "header",
        "The bootstrap ABI header.",
        WireType::Named("LaxAbiHeader", 16, 4),
        0
    ),
    field!(
        "engine_instance",
        "The host engine-instance handle.",
        WireType::Named("LaxEngineInstanceHandle", 16, 8),
        16
    ),
    field!(
        "locked_graph_hash",
        "The selected locked graph hash.",
        WireType::Named("LaxHash256", 32, 1),
        32
    ),
    field!(
        "registration_image_hash",
        "The compiled registration image hash.",
        WireType::Named("LaxHash256", 32, 1),
        64
    ),
];

const DIAGNOSTIC_FIELD_FIELDS: &[Field] = &[
    field!(
        "key",
        "Entry-lifetime UTF-8 field key.",
        WireType::Named("LaxBytes", 16, 8),
        0
    ),
    field!(
        "value",
        "Entry-lifetime UTF-8 field value.",
        WireType::Named("LaxBytes", 16, 8),
        16
    ),
];

const DIAGNOSTIC_RECORD_FIELDS: &[Field] = &[
    field!(
        "header",
        "The record ABI header.",
        WireType::Named("LaxAbiHeader", 16, 4),
        0
    ),
    field!("severity", "Stable numeric severity.", WireType::U32, 16),
    field!("code", "Stable numeric diagnostic code.", WireType::U32, 20),
    field!(
        "package_id",
        "UTF-8 package context.",
        WireType::Named("LaxBytes", 16, 8),
        24
    ),
    field!(
        "realization_id",
        "UTF-8 realization context.",
        WireType::Named("LaxBytes", 16, 8),
        40
    ),
    field!(
        "callback_key",
        "UTF-8 callback context.",
        WireType::Named("LaxBytes", 16, 8),
        56
    ),
    field!(
        "interface_id",
        "Related interface identity, or all-zero.",
        WireType::Named("LaxInterfaceId", 16, 8),
        72
    ),
    field!("abi_major", "Related ABI major.", WireType::U16, 88),
    field!("abi_minor", "Related ABI minor.", WireType::U16, 90),
    field!(
        "schema_version",
        "Related schema version, or zero.",
        WireType::U32,
        92
    ),
    field!(
        "schema_context",
        "UTF-8 schema context.",
        WireType::Named("LaxBytes", 16, 8),
        96
    ),
    field!(
        "message",
        "Short UTF-8 diagnostic message.",
        WireType::Named("LaxBytes", 16, 8),
        112
    ),
    field!(
        "fields",
        "Borrowed structured diagnostic fields.",
        WireType::ConstPointer("LaxDiagnosticFieldV0_1", "LaxDiagnosticFieldV0_1"),
        128
    ),
    field!("field_count", "Structured field count.", WireType::U64, 136),
    field!(
        "remediation",
        "Optional UTF-8 remediation action.",
        WireType::Named("LaxBytes", 16, 8),
        144
    ),
];

const COLUMN_VIEW_FIELDS: &[Field] = &[
    field!(
        "component_numeric_id",
        "Process-local component numeric ID.",
        WireType::U32,
        0
    ),
    field!(
        "flags",
        "Read, write, and optional column flags.",
        WireType::U32,
        4
    ),
    field!(
        "layout_hash",
        "The complete ABI layout hash.",
        WireType::Named("LaxHash256", 32, 1),
        8
    ),
    field!(
        "data",
        "Callback-lifetime column data.",
        WireType::MutPointer("u8", "uint8_t"),
        40
    ),
    field!("count", "Element count.", WireType::U64, 48),
    field!("stride", "Byte stride between elements.", WireType::U64, 56),
    field!(
        "element_size",
        "Initialized bytes per element.",
        WireType::U64,
        64
    ),
    field!("alignment", "Column base alignment.", WireType::U32, 72),
    field!("reserved", "Required-zero field.", WireType::U32, 76),
];

const BATCH_VIEW_FIELDS: &[Field] = &[
    field!(
        "entities",
        "Callback-lifetime entity handles.",
        WireType::ConstPointer("LaxEntityHandle", "LaxEntityHandle"),
        0
    ),
    field!("entity_count", "Entity handle count.", WireType::U64, 8),
    field!(
        "columns",
        "Callback-lifetime column views.",
        WireType::ConstPointer("LaxColumnViewV0_1", "LaxColumnViewV0_1"),
        16
    ),
    field!("column_count", "Column view count.", WireType::U64, 24),
];

const COMMAND_SINK_FIELDS: &[Field] = &[
    field!(
        "context",
        "Host-owned command staging context.",
        WireType::MutPointer("c_void", "void"),
        0
    ),
    field!(
        "max_commands",
        "Hard command count budget.",
        WireType::U64,
        8
    ),
    field!(
        "max_payload_bytes",
        "Hard aggregate payload byte budget.",
        WireType::U64,
        16
    ),
    field!(
        "append",
        "Append-one staged command callback.",
        WireType::Callback("LaxAppendCommandFn"),
        24
    ),
];

const COMMAND_RECORD_FIELDS: &[Field] = &[
    field!(
        "sequence",
        "Strictly increasing callback-local sequence.",
        WireType::U64,
        0
    ),
    field!(
        "opcode",
        "Stable structural command opcode.",
        WireType::U32,
        8
    ),
    field!("flags", "Command flags.", WireType::U32, 12),
    field!(
        "target",
        "Typed entity target; all-zero only when the opcode permits it.",
        WireType::Named("LaxEntityHandle", 16, 8),
        16
    ),
    field!(
        "subject_numeric_id",
        "Process-local component or payload-schema ID, or zero when not applicable.",
        WireType::U32,
        32
    ),
    field!("reserved", "Required-zero field.", WireType::U32, 36),
    field!(
        "layout_hash",
        "Component layout hash, or all-zero when not applicable.",
        WireType::Named("LaxHash256", 32, 1),
        40
    ),
    field!(
        "payload",
        "Callback-lifetime versioned command payload.",
        WireType::Named("LaxBytes", 16, 8),
        72
    ),
];

const MESSAGE_RECORD_FIELDS: &[Field] = &[
    field!(
        "schema_numeric_id",
        "Process-local message schema numeric ID.",
        WireType::U32,
        0
    ),
    field!(
        "schema_version",
        "Versioned payload schema.",
        WireType::U32,
        4
    ),
    field!(
        "sequence",
        "Strictly increasing publisher-local sequence.",
        WireType::U64,
        8
    ),
    field!(
        "payload",
        "Callback-lifetime message payload.",
        WireType::Named("LaxBytes", 16, 8),
        16
    ),
];

const MESSAGE_BATCH_FIELDS: &[Field] = &[
    field!(
        "records",
        "Borrowed message records.",
        WireType::ConstPointer("LaxMessageRecordV0_1", "LaxMessageRecordV0_1"),
        0
    ),
    field!("count", "Message record count.", WireType::U64, 8),
];

const SYSTEM_CALL_FIELDS: &[Field] = &[
    field!(
        "header",
        "The call ABI header.",
        WireType::Named("LaxAbiHeader", 16, 4),
        0
    ),
    field!(
        "engine_instance",
        "The engine instance that owns every borrowed value.",
        WireType::Named("LaxEngineInstanceHandle", 16, 8),
        16
    ),
    field!("tick", "The fixed-update tick.", WireType::U64, 32),
    field!(
        "stage_numeric_id",
        "Process-local compiled stage numeric ID.",
        WireType::U32,
        40
    ),
    field!(
        "system_numeric_id",
        "Process-local compiled system numeric ID.",
        WireType::U32,
        44
    ),
    field!(
        "batches",
        "One or more bounded batch views.",
        WireType::ConstPointer("LaxBatchViewV0_1", "LaxBatchViewV0_1"),
        48
    ),
    field!("batch_count", "Batch view count.", WireType::U64, 56),
    field!(
        "command_sink",
        "Host-owned staged command sink.",
        WireType::MutPointer("LaxCommandSinkV0_1", "LaxCommandSinkV0_1"),
        64
    ),
    field!(
        "messages",
        "Borrowed messages interface table.",
        WireType::ConstPointer("LaxMessagesTableV0_1", "LaxMessagesTableV0_1"),
        72
    ),
    field!(
        "scratch",
        "Host-owned per-callback scratch arena.",
        WireType::MutPointer("LaxScratchArenaV0_1", "LaxScratchArenaV0_1"),
        80
    ),
    field!(
        "monotonic_deadline_ns",
        "Host monotonic deadline; it is not a cancellation guarantee.",
        WireType::U64,
        88
    ),
    field!("flags", "Call flags.", WireType::U32, 96),
    field!("reserved", "Required-zero field.", WireType::U32, 100),
];

macro_rules! table_fields {
    ($callback_name:literal, $callback_docs:literal, $callback_type:literal) => {
        &[
            field!(
                "prefix",
                "The common interface table prefix.",
                WireType::Named("LaxInterfaceTableHeader", 56, 8),
                0
            ),
            field!(
                $callback_name,
                $callback_docs,
                WireType::Callback($callback_type),
                56
            ),
        ]
    };
}

const DIAGNOSTICS_TABLE_FIELDS: &[Field] = table_fields!(
    "emit",
    "Emit one structured diagnostic record.",
    "LaxEmitDiagnosticFn"
);
const ECS_BATCH_TABLE_FIELDS: &[Field] = table_fields!(
    "invoke_system",
    "Invoke one system with bounded batches.",
    "LaxSystemCallbackFn"
);
const COMMAND_BUFFER_TABLE_FIELDS: &[Field] = table_fields!(
    "append",
    "Append one staged structural command.",
    "LaxAppendCommandFn"
);
const MESSAGES_TABLE_FIELDS: &[Field] = &[
    field!(
        "prefix",
        "The common interface table prefix.",
        WireType::Named("LaxInterfaceTableHeader", 56, 8),
        0
    ),
    field!(
        "publish",
        "Publish a bounded typed message batch.",
        WireType::Callback("LaxPublishMessagesFn"),
        56
    ),
    field!(
        "consume",
        "Consume a bounded typed message batch.",
        WireType::Callback("LaxConsumeMessagesFn"),
        64
    ),
];

pub(crate) const CALLBACKS: &[CallbackDef] = &[
    CallbackDef {
        name: "LaxModuleEntryFn",
        docs: "Negotiates the single exported module entry point.",
        rust_signature: "Option<unsafe extern \"C\" fn(request: *const LaxEntryRequestV0, response: *mut LaxEntryResponseV0) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxModuleEntryFn)(const LaxEntryRequestV0* request, LaxEntryResponseV0* response)",
    },
    CallbackDef {
        name: "LaxQueryInterfaceFn",
        docs: "Queries a versioned capability table.",
        rust_signature: "Option<unsafe extern \"C\" fn(context: *mut c_void, request: *const LaxInterfaceRequestV0, response: *mut LaxInterfaceResponseV0) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxQueryInterfaceFn)(void* context, const LaxInterfaceRequestV0* request, LaxInterfaceResponseV0* response)",
    },
    CallbackDef {
        name: "LaxEmitDiagnosticFn",
        docs: "Emits one structured diagnostic record.",
        rust_signature: "Option<unsafe extern \"C\" fn(context: *mut c_void, record: *const LaxDiagnosticRecordV0_1) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxEmitDiagnosticFn)(void* context, const LaxDiagnosticRecordV0_1* record)",
    },
    CallbackDef {
        name: "LaxOwnedBufferReleaseFn",
        docs: "Releases an owned buffer through its allocator domain.",
        rust_signature: "Option<unsafe extern \"C\" fn(context: *mut c_void, data: *mut u8, capacity: u64, alignment: u32) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxOwnedBufferReleaseFn)(void* context, uint8_t* data, uint64_t capacity, uint32_t alignment)",
    },
    CallbackDef {
        name: "LaxScratchAllocFn",
        docs: "Allocates callback-lifetime bytes from the host scratch arena.",
        rust_signature: "Option<unsafe extern \"C\" fn(context: *mut c_void, size: u64, alignment: u32, out: *mut LaxMutBytes) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxScratchAllocFn)(void* context, uint64_t size, uint32_t alignment, LaxMutBytes* out)",
    },
    CallbackDef {
        name: "LaxCreateInstanceFn",
        docs: "Creates one module context for one engine instance.",
        rust_signature: "Option<unsafe extern \"C\" fn(module_context: *mut c_void, request: *const LaxInstanceCreateRequestV0, out_instance_context: *mut *mut c_void) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxCreateInstanceFn)(void* module_context, const LaxInstanceCreateRequestV0* request, void** out_instance_context)",
    },
    CallbackDef {
        name: "LaxInstanceLifecycleFn",
        docs: "Starts, stops, or destroys one module instance.",
        rust_signature: "Option<unsafe extern \"C\" fn(instance_context: *mut c_void) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxInstanceLifecycleFn)(void* instance_context)",
    },
    CallbackDef {
        name: "LaxAppendCommandFn",
        docs: "Appends one command to host-owned staging.",
        rust_signature: "Option<unsafe extern \"C\" fn(context: *mut c_void, command: *const LaxCommandRecordV0_1) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxAppendCommandFn)(void* context, const LaxCommandRecordV0_1* command)",
    },
    CallbackDef {
        name: "LaxPublishMessagesFn",
        docs: "Publishes a bounded batch of typed messages.",
        rust_signature: "Option<unsafe extern \"C\" fn(context: *mut c_void, records: *const LaxMessageRecordV0_1, count: u64) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxPublishMessagesFn)(void* context, const LaxMessageRecordV0_1* records, uint64_t count)",
    },
    CallbackDef {
        name: "LaxConsumeMessagesFn",
        docs: "Borrows a bounded batch of typed messages.",
        rust_signature: "Option<unsafe extern \"C\" fn(context: *mut c_void, schema_numeric_id: u32, out: *mut LaxMessageBatchV0_1) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxConsumeMessagesFn)(void* context, uint32_t schema_numeric_id, LaxMessageBatchV0_1* out)",
    },
    CallbackDef {
        name: "LaxSystemCallbackFn",
        docs: "Invokes one bounded ECS system call.",
        rust_signature: "Option<unsafe extern \"C\" fn(context: *mut c_void, call: *const LaxSystemCallV0_1) -> LaxStatus>",
        c_signature: "LaxStatus (*LaxSystemCallbackFn)(void* context, const LaxSystemCallV0_1* call)",
    },
];

pub(crate) const STRUCTS: &[StructDef] = &[
    StructDef {
        name: "LaxHash256",
        docs: "A complete 256-bit cryptographic hash.",
        size: 32,
        alignment: 1,
        fields: HASH_256_FIELDS,
    },
    StructDef {
        name: "LaxBytes",
        docs: "A callback-lifetime borrowed byte span.",
        size: 16,
        alignment: 8,
        fields: BYTES_FIELDS,
    },
    StructDef {
        name: "LaxMutBytes",
        docs: "A callback-lifetime borrowed writable byte span.",
        size: 16,
        alignment: 8,
        fields: MUT_BYTES_FIELDS,
    },
    StructDef {
        name: "LaxInterfaceId",
        docs: "The 128-bit lookup token derived from a canonical interface name.",
        size: 16,
        alignment: 8,
        fields: INTERFACE_ID_FIELDS,
    },
    StructDef {
        name: "LaxAbiHeader",
        docs: "The fixed 16-byte prefix of every versioned ABI structure.",
        size: 16,
        alignment: 4,
        fields: ABI_HEADER_FIELDS,
    },
    StructDef {
        name: "LaxOpaqueHandle",
        docs: "The fixed 16-byte generational handle representation.",
        size: 16,
        alignment: 8,
        fields: OPAQUE_HANDLE_FIELDS,
    },
    StructDef {
        name: "LaxEngineInstanceHandle",
        docs: "A kind-safe engine-instance handle.",
        size: 16,
        alignment: 8,
        fields: HANDLE_WRAPPER_FIELDS,
    },
    StructDef {
        name: "LaxEntityHandle",
        docs: "A kind-safe entity handle.",
        size: 16,
        alignment: 8,
        fields: HANDLE_WRAPPER_FIELDS,
    },
    StructDef {
        name: "LaxAssetHandle",
        docs: "A kind-safe asset handle.",
        size: 16,
        alignment: 8,
        fields: HANDLE_WRAPPER_FIELDS,
    },
    StructDef {
        name: "LaxTaskHandle",
        docs: "A kind-safe task handle reserved for a future task table.",
        size: 16,
        alignment: 8,
        fields: HANDLE_WRAPPER_FIELDS,
    },
    StructDef {
        name: "LaxOwnedBuffer",
        docs: "An explicit allocator-domain ownership transfer.",
        size: 48,
        alignment: 8,
        fields: OWNED_BUFFER_FIELDS,
    },
    StructDef {
        name: "LaxScratchArenaV0_1",
        docs: "A host-owned, per-callback bounded bump arena.",
        size: 24,
        alignment: 8,
        fields: SCRATCH_ARENA_FIELDS,
    },
    StructDef {
        name: "LaxInterfaceTableHeader",
        docs: "The common prefix of every capability table.",
        size: 56,
        alignment: 8,
        fields: INTERFACE_TABLE_HEADER_FIELDS,
    },
    StructDef {
        name: "LaxInterfaceRequestV0",
        docs: "A bounded interface-version negotiation request.",
        size: 40,
        alignment: 8,
        fields: INTERFACE_REQUEST_FIELDS,
    },
    StructDef {
        name: "LaxInterfaceResponseV0",
        docs: "A selected borrowed interface table.",
        size: 48,
        alignment: 8,
        fields: INTERFACE_RESPONSE_FIELDS,
    },
    StructDef {
        name: "LaxEntryRequestV0",
        docs: "The host-to-module bootstrap request for ABI 0.x.",
        size: 208,
        alignment: 8,
        fields: ENTRY_REQUEST_FIELDS,
    },
    StructDef {
        name: "LaxEntryResponseV0",
        docs: "The caller-allocated module bootstrap response for ABI 0.x.",
        size: 136,
        alignment: 8,
        fields: ENTRY_RESPONSE_FIELDS,
    },
    StructDef {
        name: "LaxInstanceCreateRequestV0",
        docs: "The validated context for creating one module instance.",
        size: 96,
        alignment: 8,
        fields: INSTANCE_CREATE_REQUEST_FIELDS,
    },
    StructDef {
        name: "LaxDiagnosticFieldV0_1",
        docs: "One structured UTF-8 diagnostic field.",
        size: 32,
        alignment: 8,
        fields: DIAGNOSTIC_FIELD_FIELDS,
    },
    StructDef {
        name: "LaxDiagnosticRecordV0_1",
        docs: "An owner-aware structured diagnostic record.",
        size: 160,
        alignment: 8,
        fields: DIAGNOSTIC_RECORD_FIELDS,
    },
    StructDef {
        name: "LaxColumnViewV0_1",
        docs: "One callback-lifetime ABI-POD ECS column view.",
        size: 80,
        alignment: 8,
        fields: COLUMN_VIEW_FIELDS,
    },
    StructDef {
        name: "LaxBatchViewV0_1",
        docs: "One entity batch and its ABI-POD columns.",
        size: 32,
        alignment: 8,
        fields: BATCH_VIEW_FIELDS,
    },
    StructDef {
        name: "LaxCommandSinkV0_1",
        docs: "A bounded host-owned structural command staging sink.",
        size: 32,
        alignment: 8,
        fields: COMMAND_SINK_FIELDS,
    },
    StructDef {
        name: "LaxCommandRecordV0_1",
        docs: "One versioned staged structural command.",
        size: 88,
        alignment: 8,
        fields: COMMAND_RECORD_FIELDS,
    },
    StructDef {
        name: "LaxMessageRecordV0_1",
        docs: "One typed versioned message payload.",
        size: 32,
        alignment: 8,
        fields: MESSAGE_RECORD_FIELDS,
    },
    StructDef {
        name: "LaxMessageBatchV0_1",
        docs: "A callback-lifetime batch of typed messages.",
        size: 16,
        alignment: 8,
        fields: MESSAGE_BATCH_FIELDS,
    },
    StructDef {
        name: "LaxSystemCallV0_1",
        docs: "One bounded system invocation across the native ABI.",
        size: 104,
        alignment: 8,
        fields: SYSTEM_CALL_FIELDS,
    },
    StructDef {
        name: "LaxDiagnosticsTableV0_1",
        docs: "The `core.diagnostics@0.1` capability table.",
        size: 64,
        alignment: 8,
        fields: DIAGNOSTICS_TABLE_FIELDS,
    },
    StructDef {
        name: "LaxEcsBatchTableV0_1",
        docs: "The `ecs.batch@0.1` capability table.",
        size: 64,
        alignment: 8,
        fields: ECS_BATCH_TABLE_FIELDS,
    },
    StructDef {
        name: "LaxCommandBufferTableV0_1",
        docs: "The `ecs.command-buffer@0.1` capability table.",
        size: 64,
        alignment: 8,
        fields: COMMAND_BUFFER_TABLE_FIELDS,
    },
    StructDef {
        name: "LaxMessagesTableV0_1",
        docs: "The `messages@0.1` capability table.",
        size: 72,
        alignment: 8,
        fields: MESSAGES_TABLE_FIELDS,
    },
];
