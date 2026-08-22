//! Solid/fluid occupancy arbitration, dual-layer candidates, and accounting.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test fixtures fail immediately when catalog rows or identities are invalid"
)]

use std::num::NonZeroU32;

use latticeaxiom_content::{
    BlockStateV1, BoundedFluidUpdatePolicyV1, ContentCatalogInputV1, ContentCatalogLimitsV1,
    ContentCatalogV1, ContentError, FluidFlowV1, FluidLevelV1, FluidOccupancyKindV1,
    FluidPaletteEntryV1, FluidStateV1, OccupancyArbitrationContextV1, OccupancyCandidateV1,
    OccupancyCellIntentV1, OccupancyRejectV1, PaletteLimitsV1, ReplaceabilityKindV1,
    SOLID_FLUID_OCCUPANCY_CANDIDATE_SCHEMA_V1, SolidFluidArbitrationV1, SolidOccupancyKindV1,
    SolidPaletteEntryV1, arbitrate_cell,
};
use latticeaxiom_core::StableId;

const FIXTURE: &str = include_str!("../fixtures/terrenia/representative-catalog-v1.json");

fn stable_id(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("fixture StableId `{value}` is invalid: {error}"))
}

fn fixture_input() -> ContentCatalogInputV1 {
    serde_json::from_str(FIXTURE)
        .unwrap_or_else(|error| panic!("representative catalog fixture is invalid: {error}"))
}

fn occupancy_catalog() -> ContentCatalogV1 {
    let mut input = fixture_input();
    let mut reeds = input
        .blocks
        .iter()
        .find(|block| block.header.stable_id.as_str() == "terrenia:block/dirt")
        .cloned()
        .expect("fixture lost dirt");
    reeds.header.stable_id = stable_id("fixture:block/reeds");
    reeds.states[0].replaceability = latticeaxiom_content::ReplaceabilityV1::new(stable_id(
        "terrenia:replaceability/replaceable@1",
    ))
    .expect("replaceable policy");
    reeds.states[0].fluid_occupancy = latticeaxiom_content::FluidOccupancyPolicyV1::new(stable_id(
        "terrenia:fluid-occupancy/coexist@1",
    ))
    .expect("coexist policy");
    input.blocks.push(reeds);
    ContentCatalogV1::compile(input, ContentCatalogLimitsV1::default())
        .unwrap_or_else(|error| panic!("occupancy catalog did not compile: {error}"))
}

fn empty_state() -> BlockStateV1 {
    BlockStateV1::default()
}

fn context(catalog: &ContentCatalogV1) -> OccupancyArbitrationContextV1 {
    let air = catalog
        .block(&stable_id("terrenia:block/air"))
        .expect("air");
    OccupancyArbitrationContextV1::new(
        air.definition().header.stable_id.clone(),
        air.definition().default_state.clone(),
    )
    .expect("empty occupancy context")
}

fn water() -> FluidStateV1 {
    FluidStateV1 {
        level: FluidLevelV1::SOURCE,
        flow: FluidFlowV1::Still,
    }
}

fn lava_down() -> FluidStateV1 {
    FluidStateV1 {
        level: FluidLevelV1::SOURCE,
        flow: FluidFlowV1::Down,
    }
}

fn policy(cells: u32, queue: u32, bytes: u32) -> BoundedFluidUpdatePolicyV1 {
    BoundedFluidUpdatePolicyV1 {
        max_cells_per_tick: NonZeroU32::new(cells).expect("cells"),
        max_queue_depth: NonZeroU32::new(queue).expect("queue"),
        max_in_flight_bytes: NonZeroU32::new(bytes).expect("bytes"),
    }
}

fn solid(block: &str) -> SolidPaletteEntryV1 {
    SolidPaletteEntryV1 {
        block: stable_id(block),
        state: empty_state(),
    }
}

fn fluid_entry(fluid: &str, state: FluidStateV1) -> FluidPaletteEntryV1 {
    FluidPaletteEntryV1::Fluid {
        fluid: stable_id(fluid),
        state,
    }
}

#[test]
fn policy_kinds_classify_from_path_tokens_not_namespaces() {
    let catalog = occupancy_catalog();
    let air = catalog
        .block(&stable_id("terrenia:block/air"))
        .expect("air")
        .semantics_for(&empty_state())
        .expect("air semantics");
    let grass = catalog
        .block(&stable_id("terrenia:block/grass"))
        .expect("grass")
        .semantics_for(&empty_state())
        .expect("grass semantics");
    let reeds = catalog
        .block(&stable_id("fixture:block/reeds"))
        .expect("reeds")
        .semantics_for(&empty_state())
        .expect("reeds semantics");
    assert_eq!(
        FluidOccupancyKindV1::classify(&air.fluid_occupancy).expect("air occupancy"),
        FluidOccupancyKindV1::Direct
    );
    assert_eq!(
        SolidOccupancyKindV1::classify(&air.solid_occupancy).expect("air solid"),
        SolidOccupancyKindV1::Empty
    );
    assert_eq!(
        ReplaceabilityKindV1::classify(&air.replaceability).expect("air replace"),
        ReplaceabilityKindV1::Replaceable
    );
    assert_eq!(
        FluidOccupancyKindV1::classify(&grass.fluid_occupancy).expect("grass occupancy"),
        FluidOccupancyKindV1::Reject
    );
    assert_eq!(
        FluidOccupancyKindV1::classify(&reeds.fluid_occupancy).expect("reeds occupancy"),
        FluidOccupancyKindV1::Coexist
    );
}

#[test]
fn empty_solid_occupies_fluid_in_place() {
    let catalog = occupancy_catalog();
    let context = context(&catalog);
    let decision = arbitrate_cell(
        &catalog,
        &context,
        &solid("terrenia:block/air"),
        &FluidPaletteEntryV1::Empty,
        &fluid_entry("terrenia:fluid/water", water()),
    )
    .expect("air occupancy");
    assert!(matches!(decision, SolidFluidArbitrationV1::Occupied { .. }));
}

#[test]
fn reject_policy_and_mixing_fail_closed() {
    let catalog = occupancy_catalog();
    let context = context(&catalog);
    let rejected = arbitrate_cell(
        &catalog,
        &context,
        &solid("terrenia:block/grass"),
        &FluidPaletteEntryV1::Empty,
        &fluid_entry("terrenia:fluid/water", water()),
    )
    .expect("grass occupancy");
    assert!(matches!(
        rejected,
        SolidFluidArbitrationV1::Rejected {
            reason: OccupancyRejectV1::PolicyRejects,
            ..
        }
    ));
    let mixed = arbitrate_cell(
        &catalog,
        &context,
        &solid("terrenia:block/air"),
        &fluid_entry("terrenia:fluid/water", water()),
        &fluid_entry("terrenia:fluid/lava", lava_down()),
    )
    .expect("mixing occupancy");
    assert!(matches!(
        mixed,
        SolidFluidArbitrationV1::Rejected {
            reason: OccupancyRejectV1::Mixing,
            ..
        }
    ));
}

#[test]
fn replaceable_non_empty_solid_emits_replace_then_occupy() {
    let catalog = occupancy_catalog();
    let context = context(&catalog);
    let decision = arbitrate_cell(
        &catalog,
        &context,
        &solid("fixture:block/reeds"),
        &FluidPaletteEntryV1::Empty,
        &fluid_entry("terrenia:fluid/water", water()),
    )
    .expect("reeds occupancy");
    match decision {
        SolidFluidArbitrationV1::ReplaceThenOccupy {
            from_solid,
            to_solid,
            fluid,
            ..
        } => {
            assert_eq!(from_solid.as_str(), "fixture:block/reeds");
            assert_eq!(to_solid.as_str(), "terrenia:block/air");
            assert_eq!(fluid.as_str(), "terrenia:fluid/water");
        }
        other => panic!("expected replace-then-occupy, got {other:?}"),
    }
}

#[test]
fn occupancy_candidate_is_order_independent_and_versioned() {
    let catalog = occupancy_catalog();
    let context = context(&catalog);
    let intents = vec![
        OccupancyCellIntentV1 {
            x: 1,
            y: 0,
            z: 0,
            solid: solid("fixture:block/reeds"),
            current_fluid: FluidPaletteEntryV1::Empty,
            requested_fluid: fluid_entry("terrenia:fluid/water", water()),
        },
        OccupancyCellIntentV1 {
            x: 0,
            y: 0,
            z: 0,
            solid: solid("terrenia:block/air"),
            current_fluid: FluidPaletteEntryV1::Empty,
            requested_fluid: fluid_entry("terrenia:fluid/lava", lava_down()),
        },
        OccupancyCellIntentV1 {
            x: 2,
            y: 0,
            z: 0,
            solid: solid("terrenia:block/grass"),
            current_fluid: FluidPaletteEntryV1::Empty,
            requested_fluid: fluid_entry("terrenia:fluid/water", water()),
        },
    ];
    let mut reversed = intents.clone();
    reversed.reverse();
    let first = OccupancyCandidateV1::compile(
        &catalog,
        &context,
        intents,
        policy(32, 32, 65_536),
        PaletteLimitsV1::default(),
    )
    .expect("forward occupancy candidate");
    let second = OccupancyCandidateV1::compile(
        &catalog,
        &context,
        reversed,
        policy(32, 32, 65_536),
        PaletteLimitsV1::default(),
    )
    .expect("reversed occupancy candidate");
    assert_eq!(first.schema(), SOLID_FLUID_OCCUPANCY_CANDIDATE_SCHEMA_V1);
    assert_eq!(
        first.canonical_hash().expect("hash"),
        second.canonical_hash().expect("hash")
    );
    assert_eq!(first.occupied(), 1);
    assert_eq!(first.replaced(), 1);
    assert_eq!(first.rejected(), 1);
    assert!(first.fluid_palette().entries()[0].is_empty());
    assert_eq!(first.cells()[0].x(), 0);
    assert_eq!(first.cells()[1].x(), 1);
    assert_eq!(first.accounting().queue_depth(), 2);
}

#[test]
fn occupancy_accounting_hooks_fail_closed() {
    let catalog = occupancy_catalog();
    let context = context(&catalog);
    let intents = vec![
        OccupancyCellIntentV1 {
            x: 0,
            y: 0,
            z: 0,
            solid: solid("terrenia:block/air"),
            current_fluid: FluidPaletteEntryV1::Empty,
            requested_fluid: fluid_entry("terrenia:fluid/water", water()),
        },
        OccupancyCellIntentV1 {
            x: 1,
            y: 0,
            z: 0,
            solid: solid("terrenia:block/air"),
            current_fluid: FluidPaletteEntryV1::Empty,
            requested_fluid: fluid_entry("terrenia:fluid/water", water()),
        },
    ];
    let error = OccupancyCandidateV1::compile(
        &catalog,
        &context,
        intents,
        policy(1, 8, 65_536),
        PaletteLimitsV1::default(),
    )
    .expect_err("two occupied cells must exceed a one-cell bound");
    assert!(matches!(
        error,
        ContentError::LimitExceeded {
            resource: "fluid_cells_per_tick",
            limit: 1,
            ..
        }
    ));
}
