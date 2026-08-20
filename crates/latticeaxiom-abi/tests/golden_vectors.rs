//! Immutable domain-separated identity and descriptor hash vectors.

use latticeaxiom_abi::{
    BuiltInInterface, InterfaceClaim, InterfaceIdentity, builtin_interface_descriptor,
    validate_interface_claims,
};

#[test]
fn interface_identity_golden_vectors() {
    let cases = [
        (
            "latticeaxiom:interface/core.diagnostics",
            "80fe7d863232fe4d1da2090dc85daa69455db271ff80eeb3546f80eca4620780",
            0x80fe_7d86_3232_fe4d_u64,
            0x1da2_090d_c85d_aa69_u64,
        ),
        (
            "latticeaxiom:interface/ecs.batch",
            "5f5566d439c5746438374fdcce19d3ee926de1139a0dbc5e45421c2581e19d27",
            0x5f55_66d4_39c5_7464_u64,
            0x3837_4fdc_ce19_d3ee_u64,
        ),
        (
            "latticeaxiom:interface/ecs.command-buffer",
            "02eb67f38e2fac365fe34d9d8f8d5e205793d60c68806c29104ae9cfd2fe7747",
            0x02eb_67f3_8e2f_ac36_u64,
            0x5fe3_4d9d_8f8d_5e20_u64,
        ),
        (
            "latticeaxiom:interface/messages",
            "f896ba63eea628887da431f4eeb79c247b3f4e6875b011ab86f07a83a0feb9ac",
            0xf896_ba63_eea6_2888_u64,
            0x7da4_31f4_eeb7_9c24_u64,
        ),
    ];
    for (name, expected_hash, expected_hi, expected_lo) in cases {
        let identity = InterfaceIdentity::derive(name);
        assert!(identity.is_ok(), "built-in identity must parse");
        let Some(identity) = identity.ok() else {
            continue;
        };
        assert_eq!(hex::encode(identity.full_hash()), expected_hash);
        assert_eq!(identity.token().hi, expected_hi);
        assert_eq!(identity.token().lo, expected_lo);
        let claim = InterfaceClaim {
            canonical_name: name,
            full_hash: identity.full_hash(),
            token: identity.token(),
        };
        assert!(validate_interface_claims(&[claim]).is_ok());
    }
}

#[test]
fn interface_descriptor_hash_golden_vectors() {
    let cases = [
        (
            BuiltInInterface::Diagnostics,
            "486f95f5c7508ef422a6379dca6059a80b2182895a98c81223f41f47f50f3a4c",
        ),
        (
            BuiltInInterface::EcsBatch,
            "f48054fb05a2d5c44930dbb954d631de20f2a1a9e600fbbcb8c9c483072908a5",
        ),
        (
            BuiltInInterface::EcsCommandBuffer,
            "f93c7e24c2d26ca0785ad511f3373f9e0cb38c0a192979bb5778d6028c7066f5",
        ),
        (
            BuiltInInterface::Messages,
            "b094712d991caae9821f88f0fb96bfee52306f030a1837781cb5a557b73dd8a2",
        ),
    ];
    for (interface, expected_hash) in cases {
        let descriptor = builtin_interface_descriptor(interface);
        assert!(descriptor.is_ok(), "built-in descriptor must be valid");
        let Some(descriptor) = descriptor.ok() else {
            continue;
        };
        assert_eq!(hex::encode(descriptor.hash()), expected_hash);
    }
}
