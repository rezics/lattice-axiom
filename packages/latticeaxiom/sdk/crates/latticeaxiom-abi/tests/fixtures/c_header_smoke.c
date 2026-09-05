#include "latticeaxiom_abi.h"

static LaxStatus no_op(void* context) {
    (void)context;
    return 0;
}

int main(void) {
    LaxAbiHeader header = {
        .magic = LAX_ABI_MAGIC,
        .abi_major = 0,
        .abi_minor = 1,
        .struct_size = sizeof(LaxAbiHeader),
        .flags = 0,
    };
    LaxOpaqueHandle handle = { .table_id = 7, .slot = 3, .generation = 1 };
    LaxInstanceLifecycleFn lifecycle = no_op;
    return header.magic == LAX_ABI_MAGIC && handle.generation == 1 && lifecycle != NULL && LAX_COMMAND_OPCODE_ADD_COMPONENT == UINT32_C(3)
        ? 0
        : 1;
}
