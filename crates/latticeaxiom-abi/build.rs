//! Generates Rust and C bindings from the typed Portable Native ABI schema.

#![allow(
    clippy::expect_used,
    reason = "build-script failures are programmer/schema invariants and String formatting is infallible"
)]

#[path = "schema/portable_native_abi_v0_1.rs"]
mod schema;

use std::{env, fmt::Write as _, fs, path::PathBuf};

const PREAMBLE: &str = "/* Generated from schema/portable_native_abi_v0_1.rs. Do not edit. */\n";

fn main() {
    println!("cargo:rerun-if-changed=schema/portable_native_abi_v0_1.rs");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo always sets OUT_DIR"));
    fs::write(out_dir.join("bindings.rs"), rust_bindings()).expect("OUT_DIR must be writable");
    fs::write(out_dir.join("latticeaxiom_abi.h"), c_header()).expect("OUT_DIR must be writable");
}

fn rust_bindings() -> String {
    let mut output =
        String::from("// Generated from schema/portable_native_abi_v0_1.rs. Do not edit.\n\n");
    output.push_str("use core::ffi::c_void;\n\n");
    output.push_str("/// The ABI status word. Zero is success.\npub type LaxStatus = u32;\n\n");
    for constant in schema::CONSTANTS {
        writeln!(output, "/// {}", constant.docs).expect("writing a String cannot fail");
        writeln!(
            output,
            "pub const {}: {} = {};\n",
            constant.name, constant.rust_type, constant.rust_value
        )
        .expect("writing a String cannot fail");
    }
    for callback in schema::CALLBACKS {
        writeln!(output, "/// {}", callback.docs).expect("writing a String cannot fail");
        writeln!(
            output,
            "pub type {} = {};\n",
            callback.name, callback.rust_signature
        )
        .expect("writing a String cannot fail");
    }
    for definition in schema::STRUCTS {
        validate_layout(definition);
        writeln!(output, "/// {}", definition.docs).expect("writing a String cannot fail");
        output.push_str("#[repr(C)]\n#[derive(Clone, Copy, Debug)]\n");
        writeln!(output, "pub struct {} {{", definition.name)
            .expect("writing a String cannot fail");
        let mut cursor = 0;
        let mut padding_index = 0;
        for field in definition.fields {
            if field.offset > cursor {
                let padding = field.offset - cursor;
                output.push_str("    /// Explicit zero-filled layout padding.\n");
                writeln!(output, "    pub padding_{padding_index}: [u8; {padding}],")
                    .expect("writing a String cannot fail");
                padding_index += 1;
            }
            writeln!(output, "    /// {}", field.docs).expect("writing a String cannot fail");
            writeln!(
                output,
                "    pub {}: {},",
                field.name,
                field.wire_type.rust_type()
            )
            .expect("writing a String cannot fail");
            cursor = field.offset + field.wire_type.size();
        }
        if definition.size > cursor {
            let padding = definition.size - cursor;
            output.push_str("    /// Explicit zero-filled trailing layout padding.\n");
            writeln!(output, "    pub padding_{padding_index}: [u8; {padding}],")
                .expect("writing a String cannot fail");
        }
        output.push_str("}\n");
        writeln!(
            output,
            "const _: [(); {}] = [(); core::mem::size_of::<{}>()];",
            definition.size, definition.name
        )
        .expect("writing a String cannot fail");
        writeln!(
            output,
            "const _: [(); {}] = [(); core::mem::align_of::<{}>()];",
            definition.alignment, definition.name
        )
        .expect("writing a String cannot fail");
        for field in definition.fields {
            writeln!(
                output,
                "const _: [(); {}] = [(); core::mem::offset_of!({}, {})];",
                field.offset, definition.name, field.name
            )
            .expect("writing a String cannot fail");
        }
        output.push('\n');
    }
    output
}

fn c_header() -> String {
    let mut output = String::from(PREAMBLE);
    output.push_str("#ifndef LATTICEAXIOM_ABI_H\n#define LATTICEAXIOM_ABI_H\n\n");
    output.push_str("#include <limits.h>\n#include <stddef.h>\n#include <stdint.h>\n\n");
    output.push_str(
        "#if CHAR_BIT != 8\n#error \"Portable Native ABI 0.x requires 8-bit bytes\"\n#endif\n",
    );
    output.push_str("#if UINTPTR_MAX != UINT64_MAX\n#error \"Portable Native ABI 0.x requires 64-bit pointers\"\n#endif\n");
    output.push_str("#if defined(__BYTE_ORDER__) && (__BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__)\n#error \"Portable Native ABI 0.x is little-endian only\"\n#endif\n\n");
    output.push_str("#ifdef __cplusplus\nextern \"C\" {\n#endif\n\n");
    output.push_str("typedef uint32_t LaxStatus;\n\n");
    for constant in schema::CONSTANTS {
        writeln!(output, "/* {} */", constant.docs).expect("writing a String cannot fail");
        writeln!(output, "#define {} {}", constant.name, constant.c_value)
            .expect("writing a String cannot fail");
    }
    output.push('\n');
    for definition in schema::STRUCTS {
        writeln!(
            output,
            "typedef struct {} {};",
            definition.name, definition.name
        )
        .expect("writing a String cannot fail");
    }
    output.push('\n');
    for callback in schema::CALLBACKS {
        writeln!(output, "/* {} */", callback.docs).expect("writing a String cannot fail");
        writeln!(output, "typedef {};", callback.c_signature)
            .expect("writing a String cannot fail");
    }
    output.push('\n');
    for definition in schema::STRUCTS {
        validate_layout(definition);
        writeln!(output, "/* {} */", definition.docs).expect("writing a String cannot fail");
        writeln!(output, "struct {} {{", definition.name).expect("writing a String cannot fail");
        let mut cursor = 0;
        let mut padding_index = 0;
        for field in definition.fields {
            if field.offset > cursor {
                let padding = field.offset - cursor;
                writeln!(output, "    uint8_t padding_{padding_index}[{padding}];")
                    .expect("writing a String cannot fail");
                padding_index += 1;
            }
            writeln!(output, "    {};", field.wire_type.c_declaration(field.name))
                .expect("writing a String cannot fail");
            cursor = field.offset + field.wire_type.size();
        }
        if definition.size > cursor {
            let padding = definition.size - cursor;
            writeln!(output, "    uint8_t padding_{padding_index}[{padding}];")
                .expect("writing a String cannot fail");
        }
        output.push_str("};\n");
        writeln!(
            output,
            "_Static_assert(sizeof({}) == {}, \"{} size\");",
            definition.name, definition.size, definition.name
        )
        .expect("writing a String cannot fail");
        writeln!(
            output,
            "_Static_assert(_Alignof({}) == {}, \"{} alignment\");",
            definition.name, definition.alignment, definition.name
        )
        .expect("writing a String cannot fail");
        for field in definition.fields {
            writeln!(
                output,
                "_Static_assert(offsetof({}, {}) == {}, \"{}.{} offset\");",
                definition.name, field.name, field.offset, definition.name, field.name
            )
            .expect("writing a String cannot fail");
        }
        output.push('\n');
    }
    output.push_str("LaxStatus latticeaxiom_module_entry(const LaxEntryRequestV0* request, LaxEntryResponseV0* response);\n\n");
    output.push_str("#ifdef __cplusplus\n}\n#endif\n\n#endif\n");
    output
}

fn validate_layout(definition: &schema::StructDef) {
    let mut cursor = 0;
    let mut max_alignment = 1;
    for field in definition.fields {
        let alignment = field.wire_type.alignment();
        max_alignment = max_alignment.max(alignment);
        cursor = align_up(cursor, alignment);
        assert_eq!(
            cursor, field.offset,
            "schema offset mismatch for {}.{}",
            definition.name, field.name
        );
        cursor = cursor
            .checked_add(field.wire_type.size())
            .expect("schema layout size overflow");
    }
    assert_eq!(
        max_alignment, definition.alignment,
        "schema alignment mismatch for {}",
        definition.name
    );
    assert_eq!(
        align_up(cursor, max_alignment),
        definition.size,
        "schema size mismatch for {}",
        definition.name
    );
}

fn align_up(value: usize, alignment: usize) -> usize {
    value
        .checked_add(alignment - 1)
        .expect("schema alignment overflow")
        / alignment
        * alignment
}
