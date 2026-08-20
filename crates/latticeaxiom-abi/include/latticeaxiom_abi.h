/* Generated from schema/portable_native_abi_v0_1.rs. Do not edit. */
#ifndef LATTICEAXIOM_ABI_H
#define LATTICEAXIOM_ABI_H

#include <limits.h>
#include <stddef.h>
#include <stdint.h>

#if CHAR_BIT != 8
#error "Portable Native ABI 0.x requires 8-bit bytes"
#endif
#if UINTPTR_MAX != UINT64_MAX
#error "Portable Native ABI 0.x requires 64-bit pointers"
#endif
#if defined(__BYTE_ORDER__) && (__BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__)
#error "Portable Native ABI 0.x is little-endian only"
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef uint32_t LaxStatus;

/* Memory bytes `LAXB` interpreted as a little-endian `u32`. */
#define LAX_ABI_MAGIC UINT32_C(0x4258414c)
/* Bootstrap ABI 0.x major. */
#define LAX_ABI_MAJOR UINT16_C(0)
/* Bootstrap ABI 0.1 minor epoch. */
#define LAX_ABI_MINOR UINT16_C(1)
/* Portable ABI little-endian target discriminator. */
#define LAX_ENDIANNESS_LITTLE UINT8_C(1)
/* Portable ABI pointer width. */
#define LAX_POINTER_WIDTH_BITS UINT8_C(64)
/* Platform C calling convention discriminator. */
#define LAX_C_CALL_ABI_PLATFORM UINT16_C(1)
/* Successful ABI call. */
#define LAX_STATUS_OK UINT32_C(0)
/* Malformed argument or record. */
#define LAX_STATUS_INVALID_ARGUMENT UINT32_C(1)
/* Unsupported ABI or interface version. */
#define LAX_STATUS_UNSUPPORTED_VERSION UINT32_C(2)
/* ABI ownership, layout, permission, or lifecycle contract violation. */
#define LAX_STATUS_CONTRACT_VIOLATION UINT32_C(3)
/* A declared hard resource budget was exhausted. */
#define LAX_STATUS_RESOURCE_EXHAUSTED UINT32_C(4)
/* The package lacks the required permission. */
#define LAX_STATUS_PERMISSION_DENIED UINT32_C(5)
/* A generational handle is stale or belongs to the wrong table. */
#define LAX_STATUS_STALE_HANDLE UINT32_C(6)
/* A declared recoverable domain operation rejected its staged result. */
#define LAX_STATUS_DOMAIN_REJECTED UINT32_C(7)
/* The owning engine instance has failed. */
#define LAX_STATUS_INSTANCE_FAILED UINT32_C(8)
/* A callback returned after its soft monotonic deadline. */
#define LAX_STATUS_DEADLINE_OVERRUN UINT32_C(9)
/* An SDK boundary caught a language panic or exception. */
#define LAX_STATUS_INTERNAL_PANIC UINT32_C(10)
/* A caller-provided output structure or buffer is too small. */
#define LAX_STATUS_BUFFER_TOO_SMALL UINT32_C(11)
/* Header low flags that inspectors preserve even when unknown. */
#define LAX_HEADER_ADVISORY_MASK UINT32_C(0x0000ffff)
/* Header high flags that require explicit understanding. */
#define LAX_HEADER_REQUIRED_MASK UINT32_C(0xffff0000)
/* Interface callback may run on a host worker. */
#define LAX_INTERFACE_FLAG_HOST_WORKER_SAFE UINT32_C(1)
/* Interface callback permits concurrent batches for one instance. */
#define LAX_INTERFACE_FLAG_CONCURRENT_BATCHES UINT32_C(2)
/* Interface callback may block. */
#define LAX_INTERFACE_FLAG_MAY_BLOCK UINT32_C(4)
/* Interface callback permits synchronous host re-entry. */
#define LAX_INTERFACE_FLAG_REENTRANT UINT32_C(8)
/* All interface flags understood by ABI 0.1. */
#define LAX_INTERFACE_FLAGS_KNOWN UINT32_C(15)
/* Column may be read by the foreign callback. */
#define LAX_COLUMN_FLAG_READ UINT32_C(1)
/* Column may be written by the foreign callback. */
#define LAX_COLUMN_FLAG_WRITE UINT32_C(2)
/* Column may be absent from a batch. */
#define LAX_COLUMN_FLAG_OPTIONAL UINT32_C(4)
/* All column flags understood by ABI 0.1. */
#define LAX_COLUMN_FLAGS_KNOWN UINT32_C(7)
/* Spawn an entity through a versioned payload. */
#define LAX_COMMAND_OPCODE_SPAWN UINT32_C(1)
/* Despawn an existing entity. */
#define LAX_COMMAND_OPCODE_DESPAWN UINT32_C(2)
/* Add a component through a versioned payload. */
#define LAX_COMMAND_OPCODE_ADD_COMPONENT UINT32_C(3)
/* Remove a component. */
#define LAX_COMMAND_OPCODE_REMOVE_COMPONENT UINT32_C(4)
/* Submit a versioned asset command. */
#define LAX_COMMAND_OPCODE_ASSET UINT32_C(5)
/* Submit a versioned world command. */
#define LAX_COMMAND_OPCODE_WORLD UINT32_C(6)
/* Informational diagnostic severity. */
#define LAX_DIAGNOSTIC_INFO UINT32_C(1)
/* Warning diagnostic severity. */
#define LAX_DIAGNOSTIC_WARNING UINT32_C(2)
/* Error diagnostic severity. */
#define LAX_DIAGNOSTIC_ERROR UINT32_C(3)
/* Fatal diagnostic severity. */
#define LAX_DIAGNOSTIC_FATAL UINT32_C(4)

typedef struct LaxHash256 LaxHash256;
typedef struct LaxBytes LaxBytes;
typedef struct LaxMutBytes LaxMutBytes;
typedef struct LaxInterfaceId LaxInterfaceId;
typedef struct LaxAbiHeader LaxAbiHeader;
typedef struct LaxOpaqueHandle LaxOpaqueHandle;
typedef struct LaxEngineInstanceHandle LaxEngineInstanceHandle;
typedef struct LaxEntityHandle LaxEntityHandle;
typedef struct LaxAssetHandle LaxAssetHandle;
typedef struct LaxTaskHandle LaxTaskHandle;
typedef struct LaxOwnedBuffer LaxOwnedBuffer;
typedef struct LaxScratchArenaV0_1 LaxScratchArenaV0_1;
typedef struct LaxInterfaceTableHeader LaxInterfaceTableHeader;
typedef struct LaxInterfaceRequestV0 LaxInterfaceRequestV0;
typedef struct LaxInterfaceResponseV0 LaxInterfaceResponseV0;
typedef struct LaxEntryRequestV0 LaxEntryRequestV0;
typedef struct LaxEntryResponseV0 LaxEntryResponseV0;
typedef struct LaxInstanceCreateRequestV0 LaxInstanceCreateRequestV0;
typedef struct LaxDiagnosticFieldV0_1 LaxDiagnosticFieldV0_1;
typedef struct LaxDiagnosticRecordV0_1 LaxDiagnosticRecordV0_1;
typedef struct LaxColumnViewV0_1 LaxColumnViewV0_1;
typedef struct LaxBatchViewV0_1 LaxBatchViewV0_1;
typedef struct LaxCommandSinkV0_1 LaxCommandSinkV0_1;
typedef struct LaxCommandRecordV0_1 LaxCommandRecordV0_1;
typedef struct LaxMessageRecordV0_1 LaxMessageRecordV0_1;
typedef struct LaxMessageBatchV0_1 LaxMessageBatchV0_1;
typedef struct LaxSystemCallV0_1 LaxSystemCallV0_1;
typedef struct LaxDiagnosticsTableV0_1 LaxDiagnosticsTableV0_1;
typedef struct LaxEcsBatchTableV0_1 LaxEcsBatchTableV0_1;
typedef struct LaxCommandBufferTableV0_1 LaxCommandBufferTableV0_1;
typedef struct LaxMessagesTableV0_1 LaxMessagesTableV0_1;

/* Negotiates the single exported module entry point. */
typedef LaxStatus (*LaxModuleEntryFn)(const LaxEntryRequestV0* request, LaxEntryResponseV0* response);
/* Queries a versioned capability table. */
typedef LaxStatus (*LaxQueryInterfaceFn)(void* context, const LaxInterfaceRequestV0* request, LaxInterfaceResponseV0* response);
/* Emits one structured diagnostic record. */
typedef LaxStatus (*LaxEmitDiagnosticFn)(void* context, const LaxDiagnosticRecordV0_1* record);
/* Releases an owned buffer through its allocator domain. */
typedef LaxStatus (*LaxOwnedBufferReleaseFn)(void* context, uint8_t* data, uint64_t capacity, uint32_t alignment);
/* Allocates callback-lifetime bytes from the host scratch arena. */
typedef LaxStatus (*LaxScratchAllocFn)(void* context, uint64_t size, uint32_t alignment, LaxMutBytes* out);
/* Creates one module context for one engine instance. */
typedef LaxStatus (*LaxCreateInstanceFn)(void* module_context, const LaxInstanceCreateRequestV0* request, void** out_instance_context);
/* Starts, stops, or destroys one module instance. */
typedef LaxStatus (*LaxInstanceLifecycleFn)(void* instance_context);
/* Appends one command to host-owned staging. */
typedef LaxStatus (*LaxAppendCommandFn)(void* context, const LaxCommandRecordV0_1* command);
/* Publishes a bounded batch of typed messages. */
typedef LaxStatus (*LaxPublishMessagesFn)(void* context, const LaxMessageRecordV0_1* records, uint64_t count);
/* Borrows a bounded batch of typed messages. */
typedef LaxStatus (*LaxConsumeMessagesFn)(void* context, uint32_t schema_numeric_id, LaxMessageBatchV0_1* out);
/* Invokes one bounded ECS system call. */
typedef LaxStatus (*LaxSystemCallbackFn)(void* context, const LaxSystemCallV0_1* call);

/* A complete 256-bit cryptographic hash. */
struct LaxHash256 {
    uint8_t bytes[32];
};
_Static_assert(sizeof(LaxHash256) == 32, "LaxHash256 size");
_Static_assert(_Alignof(LaxHash256) == 1, "LaxHash256 alignment");
_Static_assert(offsetof(LaxHash256, bytes) == 0, "LaxHash256.bytes offset");

/* A callback-lifetime borrowed byte span. */
struct LaxBytes {
    const uint8_t* data;
    uint64_t len;
};
_Static_assert(sizeof(LaxBytes) == 16, "LaxBytes size");
_Static_assert(_Alignof(LaxBytes) == 8, "LaxBytes alignment");
_Static_assert(offsetof(LaxBytes, data) == 0, "LaxBytes.data offset");
_Static_assert(offsetof(LaxBytes, len) == 8, "LaxBytes.len offset");

/* A callback-lifetime borrowed writable byte span. */
struct LaxMutBytes {
    uint8_t* data;
    uint64_t len;
};
_Static_assert(sizeof(LaxMutBytes) == 16, "LaxMutBytes size");
_Static_assert(_Alignof(LaxMutBytes) == 8, "LaxMutBytes alignment");
_Static_assert(offsetof(LaxMutBytes, data) == 0, "LaxMutBytes.data offset");
_Static_assert(offsetof(LaxMutBytes, len) == 8, "LaxMutBytes.len offset");

/* The 128-bit lookup token derived from a canonical interface name. */
struct LaxInterfaceId {
    uint64_t hi;
    uint64_t lo;
};
_Static_assert(sizeof(LaxInterfaceId) == 16, "LaxInterfaceId size");
_Static_assert(_Alignof(LaxInterfaceId) == 8, "LaxInterfaceId alignment");
_Static_assert(offsetof(LaxInterfaceId, hi) == 0, "LaxInterfaceId.hi offset");
_Static_assert(offsetof(LaxInterfaceId, lo) == 8, "LaxInterfaceId.lo offset");

/* The fixed 16-byte prefix of every versioned ABI structure. */
struct LaxAbiHeader {
    uint32_t magic;
    uint16_t abi_major;
    uint16_t abi_minor;
    uint32_t struct_size;
    uint32_t flags;
};
_Static_assert(sizeof(LaxAbiHeader) == 16, "LaxAbiHeader size");
_Static_assert(_Alignof(LaxAbiHeader) == 4, "LaxAbiHeader alignment");
_Static_assert(offsetof(LaxAbiHeader, magic) == 0, "LaxAbiHeader.magic offset");
_Static_assert(offsetof(LaxAbiHeader, abi_major) == 4, "LaxAbiHeader.abi_major offset");
_Static_assert(offsetof(LaxAbiHeader, abi_minor) == 6, "LaxAbiHeader.abi_minor offset");
_Static_assert(offsetof(LaxAbiHeader, struct_size) == 8, "LaxAbiHeader.struct_size offset");
_Static_assert(offsetof(LaxAbiHeader, flags) == 12, "LaxAbiHeader.flags offset");

/* The fixed 16-byte generational handle representation. */
struct LaxOpaqueHandle {
    uint64_t table_id;
    uint32_t slot;
    uint32_t generation;
};
_Static_assert(sizeof(LaxOpaqueHandle) == 16, "LaxOpaqueHandle size");
_Static_assert(_Alignof(LaxOpaqueHandle) == 8, "LaxOpaqueHandle alignment");
_Static_assert(offsetof(LaxOpaqueHandle, table_id) == 0, "LaxOpaqueHandle.table_id offset");
_Static_assert(offsetof(LaxOpaqueHandle, slot) == 8, "LaxOpaqueHandle.slot offset");
_Static_assert(offsetof(LaxOpaqueHandle, generation) == 12, "LaxOpaqueHandle.generation offset");

/* A kind-safe engine-instance handle. */
struct LaxEngineInstanceHandle {
    LaxOpaqueHandle raw;
};
_Static_assert(sizeof(LaxEngineInstanceHandle) == 16, "LaxEngineInstanceHandle size");
_Static_assert(_Alignof(LaxEngineInstanceHandle) == 8, "LaxEngineInstanceHandle alignment");
_Static_assert(offsetof(LaxEngineInstanceHandle, raw) == 0, "LaxEngineInstanceHandle.raw offset");

/* A kind-safe entity handle. */
struct LaxEntityHandle {
    LaxOpaqueHandle raw;
};
_Static_assert(sizeof(LaxEntityHandle) == 16, "LaxEntityHandle size");
_Static_assert(_Alignof(LaxEntityHandle) == 8, "LaxEntityHandle alignment");
_Static_assert(offsetof(LaxEntityHandle, raw) == 0, "LaxEntityHandle.raw offset");

/* A kind-safe asset handle. */
struct LaxAssetHandle {
    LaxOpaqueHandle raw;
};
_Static_assert(sizeof(LaxAssetHandle) == 16, "LaxAssetHandle size");
_Static_assert(_Alignof(LaxAssetHandle) == 8, "LaxAssetHandle alignment");
_Static_assert(offsetof(LaxAssetHandle, raw) == 0, "LaxAssetHandle.raw offset");

/* A kind-safe task handle reserved for a future task table. */
struct LaxTaskHandle {
    LaxOpaqueHandle raw;
};
_Static_assert(sizeof(LaxTaskHandle) == 16, "LaxTaskHandle size");
_Static_assert(_Alignof(LaxTaskHandle) == 8, "LaxTaskHandle alignment");
_Static_assert(offsetof(LaxTaskHandle, raw) == 0, "LaxTaskHandle.raw offset");

/* An explicit allocator-domain ownership transfer. */
struct LaxOwnedBuffer {
    uint8_t* data;
    uint64_t len;
    uint64_t capacity;
    uint32_t alignment;
    uint32_t flags;
    void* release_context;
    LaxOwnedBufferReleaseFn release;
};
_Static_assert(sizeof(LaxOwnedBuffer) == 48, "LaxOwnedBuffer size");
_Static_assert(_Alignof(LaxOwnedBuffer) == 8, "LaxOwnedBuffer alignment");
_Static_assert(offsetof(LaxOwnedBuffer, data) == 0, "LaxOwnedBuffer.data offset");
_Static_assert(offsetof(LaxOwnedBuffer, len) == 8, "LaxOwnedBuffer.len offset");
_Static_assert(offsetof(LaxOwnedBuffer, capacity) == 16, "LaxOwnedBuffer.capacity offset");
_Static_assert(offsetof(LaxOwnedBuffer, alignment) == 24, "LaxOwnedBuffer.alignment offset");
_Static_assert(offsetof(LaxOwnedBuffer, flags) == 28, "LaxOwnedBuffer.flags offset");
_Static_assert(offsetof(LaxOwnedBuffer, release_context) == 32, "LaxOwnedBuffer.release_context offset");
_Static_assert(offsetof(LaxOwnedBuffer, release) == 40, "LaxOwnedBuffer.release offset");

/* A host-owned, per-callback bounded bump arena. */
struct LaxScratchArenaV0_1 {
    void* context;
    uint64_t byte_budget;
    LaxScratchAllocFn alloc;
};
_Static_assert(sizeof(LaxScratchArenaV0_1) == 24, "LaxScratchArenaV0_1 size");
_Static_assert(_Alignof(LaxScratchArenaV0_1) == 8, "LaxScratchArenaV0_1 alignment");
_Static_assert(offsetof(LaxScratchArenaV0_1, context) == 0, "LaxScratchArenaV0_1.context offset");
_Static_assert(offsetof(LaxScratchArenaV0_1, byte_budget) == 8, "LaxScratchArenaV0_1.byte_budget offset");
_Static_assert(offsetof(LaxScratchArenaV0_1, alloc) == 16, "LaxScratchArenaV0_1.alloc offset");

/* The common prefix of every capability table. */
struct LaxInterfaceTableHeader {
    LaxAbiHeader header;
    LaxInterfaceId interface_id;
    uint16_t interface_major;
    uint16_t interface_minor;
    uint32_t interface_flags;
    uint32_t reserved;
    uint8_t padding_0[4];
    void* context;
};
_Static_assert(sizeof(LaxInterfaceTableHeader) == 56, "LaxInterfaceTableHeader size");
_Static_assert(_Alignof(LaxInterfaceTableHeader) == 8, "LaxInterfaceTableHeader alignment");
_Static_assert(offsetof(LaxInterfaceTableHeader, header) == 0, "LaxInterfaceTableHeader.header offset");
_Static_assert(offsetof(LaxInterfaceTableHeader, interface_id) == 16, "LaxInterfaceTableHeader.interface_id offset");
_Static_assert(offsetof(LaxInterfaceTableHeader, interface_major) == 32, "LaxInterfaceTableHeader.interface_major offset");
_Static_assert(offsetof(LaxInterfaceTableHeader, interface_minor) == 34, "LaxInterfaceTableHeader.interface_minor offset");
_Static_assert(offsetof(LaxInterfaceTableHeader, interface_flags) == 36, "LaxInterfaceTableHeader.interface_flags offset");
_Static_assert(offsetof(LaxInterfaceTableHeader, reserved) == 40, "LaxInterfaceTableHeader.reserved offset");
_Static_assert(offsetof(LaxInterfaceTableHeader, context) == 48, "LaxInterfaceTableHeader.context offset");

/* A bounded interface-version negotiation request. */
struct LaxInterfaceRequestV0 {
    LaxAbiHeader header;
    LaxInterfaceId interface_id;
    uint16_t min_major;
    uint16_t max_major;
    uint16_t min_minor;
    uint16_t max_minor;
};
_Static_assert(sizeof(LaxInterfaceRequestV0) == 40, "LaxInterfaceRequestV0 size");
_Static_assert(_Alignof(LaxInterfaceRequestV0) == 8, "LaxInterfaceRequestV0 alignment");
_Static_assert(offsetof(LaxInterfaceRequestV0, header) == 0, "LaxInterfaceRequestV0.header offset");
_Static_assert(offsetof(LaxInterfaceRequestV0, interface_id) == 16, "LaxInterfaceRequestV0.interface_id offset");
_Static_assert(offsetof(LaxInterfaceRequestV0, min_major) == 32, "LaxInterfaceRequestV0.min_major offset");
_Static_assert(offsetof(LaxInterfaceRequestV0, max_major) == 34, "LaxInterfaceRequestV0.max_major offset");
_Static_assert(offsetof(LaxInterfaceRequestV0, min_minor) == 36, "LaxInterfaceRequestV0.min_minor offset");
_Static_assert(offsetof(LaxInterfaceRequestV0, max_minor) == 38, "LaxInterfaceRequestV0.max_minor offset");

/* A selected borrowed interface table. */
struct LaxInterfaceResponseV0 {
    LaxAbiHeader header;
    LaxInterfaceId interface_id;
    uint16_t selected_major;
    uint16_t selected_minor;
    uint32_t reserved;
    const void* table;
};
_Static_assert(sizeof(LaxInterfaceResponseV0) == 48, "LaxInterfaceResponseV0 size");
_Static_assert(_Alignof(LaxInterfaceResponseV0) == 8, "LaxInterfaceResponseV0 alignment");
_Static_assert(offsetof(LaxInterfaceResponseV0, header) == 0, "LaxInterfaceResponseV0.header offset");
_Static_assert(offsetof(LaxInterfaceResponseV0, interface_id) == 16, "LaxInterfaceResponseV0.interface_id offset");
_Static_assert(offsetof(LaxInterfaceResponseV0, selected_major) == 32, "LaxInterfaceResponseV0.selected_major offset");
_Static_assert(offsetof(LaxInterfaceResponseV0, selected_minor) == 34, "LaxInterfaceResponseV0.selected_minor offset");
_Static_assert(offsetof(LaxInterfaceResponseV0, reserved) == 36, "LaxInterfaceResponseV0.reserved offset");
_Static_assert(offsetof(LaxInterfaceResponseV0, table) == 40, "LaxInterfaceResponseV0.table offset");

/* The host-to-module bootstrap request for ABI 0.x. */
struct LaxEntryRequestV0 {
    LaxAbiHeader header;
    uint16_t host_min_minor;
    uint16_t host_max_minor;
    uint8_t endianness;
    uint8_t pointer_width_bits;
    uint16_t target_reserved;
    uint16_t c_call_abi;
    uint16_t entry_reserved;
    LaxHash256 engine_build_id;
    LaxHash256 locked_graph_hash;
    LaxHash256 expected_registration_manifest_hash;
    uint8_t padding_0[4];
    LaxBytes target_triple;
    LaxBytes locked_package_id;
    LaxBytes locked_realization_id;
    void* host_context;
    LaxQueryInterfaceFn host_query_interface;
    void* diagnostic_context;
    LaxEmitDiagnosticFn emit_diagnostic;
};
_Static_assert(sizeof(LaxEntryRequestV0) == 208, "LaxEntryRequestV0 size");
_Static_assert(_Alignof(LaxEntryRequestV0) == 8, "LaxEntryRequestV0 alignment");
_Static_assert(offsetof(LaxEntryRequestV0, header) == 0, "LaxEntryRequestV0.header offset");
_Static_assert(offsetof(LaxEntryRequestV0, host_min_minor) == 16, "LaxEntryRequestV0.host_min_minor offset");
_Static_assert(offsetof(LaxEntryRequestV0, host_max_minor) == 18, "LaxEntryRequestV0.host_max_minor offset");
_Static_assert(offsetof(LaxEntryRequestV0, endianness) == 20, "LaxEntryRequestV0.endianness offset");
_Static_assert(offsetof(LaxEntryRequestV0, pointer_width_bits) == 21, "LaxEntryRequestV0.pointer_width_bits offset");
_Static_assert(offsetof(LaxEntryRequestV0, target_reserved) == 22, "LaxEntryRequestV0.target_reserved offset");
_Static_assert(offsetof(LaxEntryRequestV0, c_call_abi) == 24, "LaxEntryRequestV0.c_call_abi offset");
_Static_assert(offsetof(LaxEntryRequestV0, entry_reserved) == 26, "LaxEntryRequestV0.entry_reserved offset");
_Static_assert(offsetof(LaxEntryRequestV0, engine_build_id) == 28, "LaxEntryRequestV0.engine_build_id offset");
_Static_assert(offsetof(LaxEntryRequestV0, locked_graph_hash) == 60, "LaxEntryRequestV0.locked_graph_hash offset");
_Static_assert(offsetof(LaxEntryRequestV0, expected_registration_manifest_hash) == 92, "LaxEntryRequestV0.expected_registration_manifest_hash offset");
_Static_assert(offsetof(LaxEntryRequestV0, target_triple) == 128, "LaxEntryRequestV0.target_triple offset");
_Static_assert(offsetof(LaxEntryRequestV0, locked_package_id) == 144, "LaxEntryRequestV0.locked_package_id offset");
_Static_assert(offsetof(LaxEntryRequestV0, locked_realization_id) == 160, "LaxEntryRequestV0.locked_realization_id offset");
_Static_assert(offsetof(LaxEntryRequestV0, host_context) == 176, "LaxEntryRequestV0.host_context offset");
_Static_assert(offsetof(LaxEntryRequestV0, host_query_interface) == 184, "LaxEntryRequestV0.host_query_interface offset");
_Static_assert(offsetof(LaxEntryRequestV0, diagnostic_context) == 192, "LaxEntryRequestV0.diagnostic_context offset");
_Static_assert(offsetof(LaxEntryRequestV0, emit_diagnostic) == 200, "LaxEntryRequestV0.emit_diagnostic offset");

/* The caller-allocated module bootstrap response for ABI 0.x. */
struct LaxEntryResponseV0 {
    LaxAbiHeader header;
    uint32_t module_flags;
    uint32_t reserved;
    LaxHash256 actual_registration_manifest_hash;
    LaxBytes module_id;
    LaxBytes realization_id;
    void* module_context;
    LaxCreateInstanceFn create_instance;
    LaxInstanceLifecycleFn start_instance;
    LaxInstanceLifecycleFn stop_instance;
    LaxInstanceLifecycleFn destroy_instance;
    LaxQueryInterfaceFn module_query_interface;
};
_Static_assert(sizeof(LaxEntryResponseV0) == 136, "LaxEntryResponseV0 size");
_Static_assert(_Alignof(LaxEntryResponseV0) == 8, "LaxEntryResponseV0 alignment");
_Static_assert(offsetof(LaxEntryResponseV0, header) == 0, "LaxEntryResponseV0.header offset");
_Static_assert(offsetof(LaxEntryResponseV0, module_flags) == 16, "LaxEntryResponseV0.module_flags offset");
_Static_assert(offsetof(LaxEntryResponseV0, reserved) == 20, "LaxEntryResponseV0.reserved offset");
_Static_assert(offsetof(LaxEntryResponseV0, actual_registration_manifest_hash) == 24, "LaxEntryResponseV0.actual_registration_manifest_hash offset");
_Static_assert(offsetof(LaxEntryResponseV0, module_id) == 56, "LaxEntryResponseV0.module_id offset");
_Static_assert(offsetof(LaxEntryResponseV0, realization_id) == 72, "LaxEntryResponseV0.realization_id offset");
_Static_assert(offsetof(LaxEntryResponseV0, module_context) == 88, "LaxEntryResponseV0.module_context offset");
_Static_assert(offsetof(LaxEntryResponseV0, create_instance) == 96, "LaxEntryResponseV0.create_instance offset");
_Static_assert(offsetof(LaxEntryResponseV0, start_instance) == 104, "LaxEntryResponseV0.start_instance offset");
_Static_assert(offsetof(LaxEntryResponseV0, stop_instance) == 112, "LaxEntryResponseV0.stop_instance offset");
_Static_assert(offsetof(LaxEntryResponseV0, destroy_instance) == 120, "LaxEntryResponseV0.destroy_instance offset");
_Static_assert(offsetof(LaxEntryResponseV0, module_query_interface) == 128, "LaxEntryResponseV0.module_query_interface offset");

/* The validated context for creating one module instance. */
struct LaxInstanceCreateRequestV0 {
    LaxAbiHeader header;
    LaxEngineInstanceHandle engine_instance;
    LaxHash256 locked_graph_hash;
    LaxHash256 registration_image_hash;
};
_Static_assert(sizeof(LaxInstanceCreateRequestV0) == 96, "LaxInstanceCreateRequestV0 size");
_Static_assert(_Alignof(LaxInstanceCreateRequestV0) == 8, "LaxInstanceCreateRequestV0 alignment");
_Static_assert(offsetof(LaxInstanceCreateRequestV0, header) == 0, "LaxInstanceCreateRequestV0.header offset");
_Static_assert(offsetof(LaxInstanceCreateRequestV0, engine_instance) == 16, "LaxInstanceCreateRequestV0.engine_instance offset");
_Static_assert(offsetof(LaxInstanceCreateRequestV0, locked_graph_hash) == 32, "LaxInstanceCreateRequestV0.locked_graph_hash offset");
_Static_assert(offsetof(LaxInstanceCreateRequestV0, registration_image_hash) == 64, "LaxInstanceCreateRequestV0.registration_image_hash offset");

/* One structured UTF-8 diagnostic field. */
struct LaxDiagnosticFieldV0_1 {
    LaxBytes key;
    LaxBytes value;
};
_Static_assert(sizeof(LaxDiagnosticFieldV0_1) == 32, "LaxDiagnosticFieldV0_1 size");
_Static_assert(_Alignof(LaxDiagnosticFieldV0_1) == 8, "LaxDiagnosticFieldV0_1 alignment");
_Static_assert(offsetof(LaxDiagnosticFieldV0_1, key) == 0, "LaxDiagnosticFieldV0_1.key offset");
_Static_assert(offsetof(LaxDiagnosticFieldV0_1, value) == 16, "LaxDiagnosticFieldV0_1.value offset");

/* An owner-aware structured diagnostic record. */
struct LaxDiagnosticRecordV0_1 {
    LaxAbiHeader header;
    uint32_t severity;
    uint32_t code;
    LaxBytes package_id;
    LaxBytes realization_id;
    LaxBytes callback_key;
    LaxInterfaceId interface_id;
    uint16_t abi_major;
    uint16_t abi_minor;
    uint32_t schema_version;
    LaxBytes schema_context;
    LaxBytes message;
    const LaxDiagnosticFieldV0_1* fields;
    uint64_t field_count;
    LaxBytes remediation;
};
_Static_assert(sizeof(LaxDiagnosticRecordV0_1) == 160, "LaxDiagnosticRecordV0_1 size");
_Static_assert(_Alignof(LaxDiagnosticRecordV0_1) == 8, "LaxDiagnosticRecordV0_1 alignment");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, header) == 0, "LaxDiagnosticRecordV0_1.header offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, severity) == 16, "LaxDiagnosticRecordV0_1.severity offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, code) == 20, "LaxDiagnosticRecordV0_1.code offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, package_id) == 24, "LaxDiagnosticRecordV0_1.package_id offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, realization_id) == 40, "LaxDiagnosticRecordV0_1.realization_id offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, callback_key) == 56, "LaxDiagnosticRecordV0_1.callback_key offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, interface_id) == 72, "LaxDiagnosticRecordV0_1.interface_id offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, abi_major) == 88, "LaxDiagnosticRecordV0_1.abi_major offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, abi_minor) == 90, "LaxDiagnosticRecordV0_1.abi_minor offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, schema_version) == 92, "LaxDiagnosticRecordV0_1.schema_version offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, schema_context) == 96, "LaxDiagnosticRecordV0_1.schema_context offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, message) == 112, "LaxDiagnosticRecordV0_1.message offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, fields) == 128, "LaxDiagnosticRecordV0_1.fields offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, field_count) == 136, "LaxDiagnosticRecordV0_1.field_count offset");
_Static_assert(offsetof(LaxDiagnosticRecordV0_1, remediation) == 144, "LaxDiagnosticRecordV0_1.remediation offset");

/* One callback-lifetime ABI-POD ECS column view. */
struct LaxColumnViewV0_1 {
    uint32_t component_numeric_id;
    uint32_t flags;
    LaxHash256 layout_hash;
    uint8_t* data;
    uint64_t count;
    uint64_t stride;
    uint64_t element_size;
    uint32_t alignment;
    uint32_t reserved;
};
_Static_assert(sizeof(LaxColumnViewV0_1) == 80, "LaxColumnViewV0_1 size");
_Static_assert(_Alignof(LaxColumnViewV0_1) == 8, "LaxColumnViewV0_1 alignment");
_Static_assert(offsetof(LaxColumnViewV0_1, component_numeric_id) == 0, "LaxColumnViewV0_1.component_numeric_id offset");
_Static_assert(offsetof(LaxColumnViewV0_1, flags) == 4, "LaxColumnViewV0_1.flags offset");
_Static_assert(offsetof(LaxColumnViewV0_1, layout_hash) == 8, "LaxColumnViewV0_1.layout_hash offset");
_Static_assert(offsetof(LaxColumnViewV0_1, data) == 40, "LaxColumnViewV0_1.data offset");
_Static_assert(offsetof(LaxColumnViewV0_1, count) == 48, "LaxColumnViewV0_1.count offset");
_Static_assert(offsetof(LaxColumnViewV0_1, stride) == 56, "LaxColumnViewV0_1.stride offset");
_Static_assert(offsetof(LaxColumnViewV0_1, element_size) == 64, "LaxColumnViewV0_1.element_size offset");
_Static_assert(offsetof(LaxColumnViewV0_1, alignment) == 72, "LaxColumnViewV0_1.alignment offset");
_Static_assert(offsetof(LaxColumnViewV0_1, reserved) == 76, "LaxColumnViewV0_1.reserved offset");

/* One entity batch and its ABI-POD columns. */
struct LaxBatchViewV0_1 {
    const LaxEntityHandle* entities;
    uint64_t entity_count;
    const LaxColumnViewV0_1* columns;
    uint64_t column_count;
};
_Static_assert(sizeof(LaxBatchViewV0_1) == 32, "LaxBatchViewV0_1 size");
_Static_assert(_Alignof(LaxBatchViewV0_1) == 8, "LaxBatchViewV0_1 alignment");
_Static_assert(offsetof(LaxBatchViewV0_1, entities) == 0, "LaxBatchViewV0_1.entities offset");
_Static_assert(offsetof(LaxBatchViewV0_1, entity_count) == 8, "LaxBatchViewV0_1.entity_count offset");
_Static_assert(offsetof(LaxBatchViewV0_1, columns) == 16, "LaxBatchViewV0_1.columns offset");
_Static_assert(offsetof(LaxBatchViewV0_1, column_count) == 24, "LaxBatchViewV0_1.column_count offset");

/* A bounded host-owned structural command staging sink. */
struct LaxCommandSinkV0_1 {
    void* context;
    uint64_t max_commands;
    uint64_t max_payload_bytes;
    LaxAppendCommandFn append;
};
_Static_assert(sizeof(LaxCommandSinkV0_1) == 32, "LaxCommandSinkV0_1 size");
_Static_assert(_Alignof(LaxCommandSinkV0_1) == 8, "LaxCommandSinkV0_1 alignment");
_Static_assert(offsetof(LaxCommandSinkV0_1, context) == 0, "LaxCommandSinkV0_1.context offset");
_Static_assert(offsetof(LaxCommandSinkV0_1, max_commands) == 8, "LaxCommandSinkV0_1.max_commands offset");
_Static_assert(offsetof(LaxCommandSinkV0_1, max_payload_bytes) == 16, "LaxCommandSinkV0_1.max_payload_bytes offset");
_Static_assert(offsetof(LaxCommandSinkV0_1, append) == 24, "LaxCommandSinkV0_1.append offset");

/* One versioned staged structural command. */
struct LaxCommandRecordV0_1 {
    uint64_t sequence;
    uint32_t opcode;
    uint32_t flags;
    LaxEntityHandle target;
    uint32_t subject_numeric_id;
    uint32_t reserved;
    LaxHash256 layout_hash;
    LaxBytes payload;
};
_Static_assert(sizeof(LaxCommandRecordV0_1) == 88, "LaxCommandRecordV0_1 size");
_Static_assert(_Alignof(LaxCommandRecordV0_1) == 8, "LaxCommandRecordV0_1 alignment");
_Static_assert(offsetof(LaxCommandRecordV0_1, sequence) == 0, "LaxCommandRecordV0_1.sequence offset");
_Static_assert(offsetof(LaxCommandRecordV0_1, opcode) == 8, "LaxCommandRecordV0_1.opcode offset");
_Static_assert(offsetof(LaxCommandRecordV0_1, flags) == 12, "LaxCommandRecordV0_1.flags offset");
_Static_assert(offsetof(LaxCommandRecordV0_1, target) == 16, "LaxCommandRecordV0_1.target offset");
_Static_assert(offsetof(LaxCommandRecordV0_1, subject_numeric_id) == 32, "LaxCommandRecordV0_1.subject_numeric_id offset");
_Static_assert(offsetof(LaxCommandRecordV0_1, reserved) == 36, "LaxCommandRecordV0_1.reserved offset");
_Static_assert(offsetof(LaxCommandRecordV0_1, layout_hash) == 40, "LaxCommandRecordV0_1.layout_hash offset");
_Static_assert(offsetof(LaxCommandRecordV0_1, payload) == 72, "LaxCommandRecordV0_1.payload offset");

/* One typed versioned message payload. */
struct LaxMessageRecordV0_1 {
    uint32_t schema_numeric_id;
    uint32_t schema_version;
    uint64_t sequence;
    LaxBytes payload;
};
_Static_assert(sizeof(LaxMessageRecordV0_1) == 32, "LaxMessageRecordV0_1 size");
_Static_assert(_Alignof(LaxMessageRecordV0_1) == 8, "LaxMessageRecordV0_1 alignment");
_Static_assert(offsetof(LaxMessageRecordV0_1, schema_numeric_id) == 0, "LaxMessageRecordV0_1.schema_numeric_id offset");
_Static_assert(offsetof(LaxMessageRecordV0_1, schema_version) == 4, "LaxMessageRecordV0_1.schema_version offset");
_Static_assert(offsetof(LaxMessageRecordV0_1, sequence) == 8, "LaxMessageRecordV0_1.sequence offset");
_Static_assert(offsetof(LaxMessageRecordV0_1, payload) == 16, "LaxMessageRecordV0_1.payload offset");

/* A callback-lifetime batch of typed messages. */
struct LaxMessageBatchV0_1 {
    const LaxMessageRecordV0_1* records;
    uint64_t count;
};
_Static_assert(sizeof(LaxMessageBatchV0_1) == 16, "LaxMessageBatchV0_1 size");
_Static_assert(_Alignof(LaxMessageBatchV0_1) == 8, "LaxMessageBatchV0_1 alignment");
_Static_assert(offsetof(LaxMessageBatchV0_1, records) == 0, "LaxMessageBatchV0_1.records offset");
_Static_assert(offsetof(LaxMessageBatchV0_1, count) == 8, "LaxMessageBatchV0_1.count offset");

/* One bounded system invocation across the native ABI. */
struct LaxSystemCallV0_1 {
    LaxAbiHeader header;
    LaxEngineInstanceHandle engine_instance;
    uint64_t tick;
    uint32_t stage_numeric_id;
    uint32_t system_numeric_id;
    const LaxBatchViewV0_1* batches;
    uint64_t batch_count;
    LaxCommandSinkV0_1* command_sink;
    const LaxMessagesTableV0_1* messages;
    LaxScratchArenaV0_1* scratch;
    uint64_t monotonic_deadline_ns;
    uint32_t flags;
    uint32_t reserved;
};
_Static_assert(sizeof(LaxSystemCallV0_1) == 104, "LaxSystemCallV0_1 size");
_Static_assert(_Alignof(LaxSystemCallV0_1) == 8, "LaxSystemCallV0_1 alignment");
_Static_assert(offsetof(LaxSystemCallV0_1, header) == 0, "LaxSystemCallV0_1.header offset");
_Static_assert(offsetof(LaxSystemCallV0_1, engine_instance) == 16, "LaxSystemCallV0_1.engine_instance offset");
_Static_assert(offsetof(LaxSystemCallV0_1, tick) == 32, "LaxSystemCallV0_1.tick offset");
_Static_assert(offsetof(LaxSystemCallV0_1, stage_numeric_id) == 40, "LaxSystemCallV0_1.stage_numeric_id offset");
_Static_assert(offsetof(LaxSystemCallV0_1, system_numeric_id) == 44, "LaxSystemCallV0_1.system_numeric_id offset");
_Static_assert(offsetof(LaxSystemCallV0_1, batches) == 48, "LaxSystemCallV0_1.batches offset");
_Static_assert(offsetof(LaxSystemCallV0_1, batch_count) == 56, "LaxSystemCallV0_1.batch_count offset");
_Static_assert(offsetof(LaxSystemCallV0_1, command_sink) == 64, "LaxSystemCallV0_1.command_sink offset");
_Static_assert(offsetof(LaxSystemCallV0_1, messages) == 72, "LaxSystemCallV0_1.messages offset");
_Static_assert(offsetof(LaxSystemCallV0_1, scratch) == 80, "LaxSystemCallV0_1.scratch offset");
_Static_assert(offsetof(LaxSystemCallV0_1, monotonic_deadline_ns) == 88, "LaxSystemCallV0_1.monotonic_deadline_ns offset");
_Static_assert(offsetof(LaxSystemCallV0_1, flags) == 96, "LaxSystemCallV0_1.flags offset");
_Static_assert(offsetof(LaxSystemCallV0_1, reserved) == 100, "LaxSystemCallV0_1.reserved offset");

/* The `core.diagnostics@0.1` capability table. */
struct LaxDiagnosticsTableV0_1 {
    LaxInterfaceTableHeader prefix;
    LaxEmitDiagnosticFn emit;
};
_Static_assert(sizeof(LaxDiagnosticsTableV0_1) == 64, "LaxDiagnosticsTableV0_1 size");
_Static_assert(_Alignof(LaxDiagnosticsTableV0_1) == 8, "LaxDiagnosticsTableV0_1 alignment");
_Static_assert(offsetof(LaxDiagnosticsTableV0_1, prefix) == 0, "LaxDiagnosticsTableV0_1.prefix offset");
_Static_assert(offsetof(LaxDiagnosticsTableV0_1, emit) == 56, "LaxDiagnosticsTableV0_1.emit offset");

/* The `ecs.batch@0.1` capability table. */
struct LaxEcsBatchTableV0_1 {
    LaxInterfaceTableHeader prefix;
    LaxSystemCallbackFn invoke_system;
};
_Static_assert(sizeof(LaxEcsBatchTableV0_1) == 64, "LaxEcsBatchTableV0_1 size");
_Static_assert(_Alignof(LaxEcsBatchTableV0_1) == 8, "LaxEcsBatchTableV0_1 alignment");
_Static_assert(offsetof(LaxEcsBatchTableV0_1, prefix) == 0, "LaxEcsBatchTableV0_1.prefix offset");
_Static_assert(offsetof(LaxEcsBatchTableV0_1, invoke_system) == 56, "LaxEcsBatchTableV0_1.invoke_system offset");

/* The `ecs.command-buffer@0.1` capability table. */
struct LaxCommandBufferTableV0_1 {
    LaxInterfaceTableHeader prefix;
    LaxAppendCommandFn append;
};
_Static_assert(sizeof(LaxCommandBufferTableV0_1) == 64, "LaxCommandBufferTableV0_1 size");
_Static_assert(_Alignof(LaxCommandBufferTableV0_1) == 8, "LaxCommandBufferTableV0_1 alignment");
_Static_assert(offsetof(LaxCommandBufferTableV0_1, prefix) == 0, "LaxCommandBufferTableV0_1.prefix offset");
_Static_assert(offsetof(LaxCommandBufferTableV0_1, append) == 56, "LaxCommandBufferTableV0_1.append offset");

/* The `messages@0.1` capability table. */
struct LaxMessagesTableV0_1 {
    LaxInterfaceTableHeader prefix;
    LaxPublishMessagesFn publish;
    LaxConsumeMessagesFn consume;
};
_Static_assert(sizeof(LaxMessagesTableV0_1) == 72, "LaxMessagesTableV0_1 size");
_Static_assert(_Alignof(LaxMessagesTableV0_1) == 8, "LaxMessagesTableV0_1 alignment");
_Static_assert(offsetof(LaxMessagesTableV0_1, prefix) == 0, "LaxMessagesTableV0_1.prefix offset");
_Static_assert(offsetof(LaxMessagesTableV0_1, publish) == 56, "LaxMessagesTableV0_1.publish offset");
_Static_assert(offsetof(LaxMessagesTableV0_1, consume) == 64, "LaxMessagesTableV0_1.consume offset");

LaxStatus latticeaxiom_module_entry(const LaxEntryRequestV0* request, LaxEntryResponseV0* response);

#ifdef __cplusplus
}
#endif

#endif
