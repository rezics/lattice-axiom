//! Rust/C layout and schema-regeneration conformance tests.

use std::{fs, mem};

use latticeaxiom_abi::{
    GENERATED_C_HEADER, LAX_ABI_MAGIC, LaxAbiHeader, LaxCommandBufferTableV0_1,
    LaxDiagnosticsTableV0_1, LaxEcsBatchTableV0_1, LaxEntryRequestV0, LaxEntryResponseV0,
    LaxInterfaceTableHeader, LaxMessagesTableV0_1, LaxOpaqueHandle, LaxOwnedBuffer,
};

#[test]
fn generated_header_snapshot_is_current() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/include/latticeaxiom_abi.h");
    let committed = fs::read_to_string(path);
    assert!(
        committed.is_ok(),
        "committed generated header must be readable"
    );
    assert_eq!(committed.ok().as_deref(), Some(GENERATED_C_HEADER));
}

#[test]
fn frozen_rust_layouts_match_the_wire_contract() {
    assert_eq!(mem::size_of::<LaxAbiHeader>(), 16);
    assert_eq!(mem::align_of::<LaxAbiHeader>(), 4);
    assert_eq!(mem::offset_of!(LaxAbiHeader, magic), 0);
    assert_eq!(mem::offset_of!(LaxAbiHeader, abi_major), 4);
    assert_eq!(mem::offset_of!(LaxAbiHeader, abi_minor), 6);
    assert_eq!(mem::offset_of!(LaxAbiHeader, struct_size), 8);
    assert_eq!(mem::offset_of!(LaxAbiHeader, flags), 12);
    assert_eq!(LAX_ABI_MAGIC.to_le_bytes(), *b"LAXB");

    assert_eq!(mem::size_of::<LaxOpaqueHandle>(), 16);
    assert_eq!(mem::align_of::<LaxOpaqueHandle>(), 8);
    assert_eq!(mem::offset_of!(LaxOpaqueHandle, table_id), 0);
    assert_eq!(mem::offset_of!(LaxOpaqueHandle, slot), 8);
    assert_eq!(mem::offset_of!(LaxOpaqueHandle, generation), 12);

    assert_eq!(mem::size_of::<LaxOwnedBuffer>(), 48);
    assert_eq!(mem::align_of::<LaxOwnedBuffer>(), 8);
    assert_eq!(mem::offset_of!(LaxOwnedBuffer, release), 40);
    assert_eq!(mem::size_of::<LaxInterfaceTableHeader>(), 56);
    assert_eq!(mem::offset_of!(LaxInterfaceTableHeader, context), 48);
    assert_eq!(mem::size_of::<LaxEntryRequestV0>(), 208);
    assert_eq!(mem::size_of::<LaxEntryResponseV0>(), 136);
    assert_eq!(mem::size_of::<LaxDiagnosticsTableV0_1>(), 64);
    assert_eq!(mem::size_of::<LaxEcsBatchTableV0_1>(), 64);
    assert_eq!(mem::size_of::<LaxCommandBufferTableV0_1>(), 64);
    assert_eq!(mem::size_of::<LaxMessagesTableV0_1>(), 72);
}
