//! Envelope-level unknown-schema fail-closed coverage for fluid palette opens.
//!
//! Integrator wiring: re-export `solid_fluid_palette` from `lib.rs` so packed
//! solid/fluid palette round-trips can move here from the crate-private module.

use std::{error::Error, str::FromStr};

use latticeaxiom_core::{PackageName, SchemaId};
use latticeaxiom_storage::{PayloadSchemaVersion, VersionedPayload};
use latticeaxiom_world_wire::{
    WorldWireError, WorldWireLimits, encode_snapshot, preflight_snapshot,
    read_snapshot_for_contract,
};

fn version() -> Result<PayloadSchemaVersion, Box<dyn Error>> {
    Ok(PayloadSchemaVersion::new(1)?)
}

#[test]
fn unknown_fluid_palette_schema_is_structurally_valid_and_not_writable()
-> Result<(), Box<dyn Error>> {
    let owner = PackageName::from_str("latticeaxiom")?;
    let expected = SchemaId::from_str("latticeaxiom:schema/solid-fluid-palette@1")?;
    let unknown = SchemaId::from_str("other:schema/solid-fluid-palette@1")?;
    let limits = WorldWireLimits::default();
    let encoded = encode_snapshot(
        &owner,
        &VersionedPayload::new(unknown.clone(), version()?, vec![1, 2, 3]),
        limits,
    )?;
    let structural = preflight_snapshot(&encoded, limits)?;
    assert_eq!(structural.schema(), &unknown);
    assert!(matches!(
        structural.validate_contract(latticeaxiom_world_wire::SnapshotContract::new(
            &expected,
            &owner,
            version()?,
        )),
        Err(WorldWireError::UnknownSchema { found }) if found == unknown
    ));
    assert!(matches!(
        read_snapshot_for_contract(
            &mut std::io::Cursor::new(encoded),
            latticeaxiom_world_wire::SnapshotContract::new(&expected, &owner, version()?,),
            limits
        ),
        Err(WorldWireError::UnknownSchema { .. })
    ));
    Ok(())
}
