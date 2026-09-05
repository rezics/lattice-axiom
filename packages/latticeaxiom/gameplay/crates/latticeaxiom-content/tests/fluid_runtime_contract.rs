//! Public fluid-runtime contracts that already export from crate root.
//!
//! Integrator wiring: re-export `fluid_semantics` and `solid_fluid_volume`
//! types from `lib.rs` so collision/selection/inspect and dense volume tests
//! can move here from the parent module.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test fixtures fail immediately when catalog rows or identities are invalid"
)]

use std::num::NonZeroU32;

use latticeaxiom_content::{
    BlockStateV1, BoundedFluidUpdatePolicyV1, CompiledFluidPaletteV1, ContentCatalogInputV1,
    ContentCatalogLimitsV1, ContentCatalogV1, ContentError, FluidCollisionKindV1,
    FluidCollisionPolicyV1, FluidFlowV1, FluidLevelV1, FluidPaletteEntryV1, FluidSelectionKindV1,
    FluidSelectionPolicyV1, FluidStateV1, OccupancyCandidateV1, OccupancyCellIntentV1,
    PaletteLimitsV1, SolidPaletteEntryV1, classify_fluid_collision, classify_fluid_selection,
    inspect_fluid_cell,
};
use latticeaxiom_core::StableId;

const FIXTURE: &str = include_str!("../fixtures/terrenia/representative-catalog-v1.json");

fn stable_id(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("fixture StableId `{value}` is invalid: {error}"))
}

fn catalog() -> ContentCatalogV1 {
    let input = serde_json::from_str::<ContentCatalogInputV1>(FIXTURE)
        .unwrap_or_else(|error| panic!("representative catalog fixture is invalid: {error}"));
    ContentCatalogV1::compile(input, ContentCatalogLimitsV1::default())
        .unwrap_or_else(|error| panic!("representative catalog did not compile: {error}"))
}

#[test]
fn fluid_collision_and_selection_policies_require_a_contract_major() {
    assert!(matches!(
        FluidCollisionPolicyV1::new(stable_id("fixture:fluid-collision-policy/volume")),
        Err(ContentError::ContractIdentityMissingMajor { .. })
    ));
    assert!(matches!(
        FluidSelectionPolicyV1::new(stable_id("fixture:fluid-selection-policy/source")),
        Err(ContentError::ContractIdentityMissingMajor { .. })
    ));
    assert!(
        FluidCollisionPolicyV1::new(stable_id("fixture:fluid-collision-policy/volume@1")).is_ok()
    );
    assert!(
        FluidSelectionPolicyV1::new(stable_id("fixture:fluid-selection-policy/source@1")).is_ok()
    );
}

#[test]
fn inspect_fragment_classifies_policy_tokens_without_presentation() {
    let collision =
        FluidCollisionPolicyV1::new(stable_id("fixture:fluid-collision-policy/volume@1"))
            .expect("volume");
    let selection =
        FluidSelectionPolicyV1::new(stable_id("fixture:fluid-selection-policy/source@1"))
            .expect("source");
    assert_eq!(
        classify_fluid_collision(&collision).expect("classify"),
        FluidCollisionKindV1::Volume
    );
    assert_eq!(
        classify_fluid_selection(&selection).expect("classify"),
        FluidSelectionKindV1::Source
    );
    let fragment = inspect_fluid_cell(
        stable_id("terrenia:fluid/water"),
        FluidStateV1 {
            level: FluidLevelV1::SOURCE,
            flow: FluidFlowV1::Still,
        },
        &collision,
        &selection,
    )
    .expect("inspect");
    assert!(fragment.collision_occupied());
    assert!(fragment.selectable());
    let encoded = serde_json::to_string(&fragment).expect("json");
    assert!(!encoded.contains("presentation"));
}

#[test]
fn compiled_fluid_palette_keeps_empty_at_zero_and_sorts_identity_then_state() {
    let catalog = catalog();
    let source = FluidStateV1 {
        level: FluidLevelV1::SOURCE,
        flow: FluidFlowV1::Still,
    };
    let flowing = FluidStateV1 {
        level: FluidLevelV1::new(3).expect("level three"),
        flow: FluidFlowV1::East,
    };
    let compiled = CompiledFluidPaletteV1::compile(
        &catalog,
        vec![
            FluidPaletteEntryV1::Fluid {
                fluid: stable_id("terrenia:fluid/water"),
                state: flowing,
            },
            FluidPaletteEntryV1::Fluid {
                fluid: stable_id("terrenia:fluid/lava"),
                state: source,
            },
        ],
        PaletteLimitsV1::default(),
    )
    .expect("fluid palette");
    assert!(compiled.entries()[0].is_empty());
    assert_eq!(
        compiled.entries()[1].fluid().map(StableId::as_str),
        Some("terrenia:fluid/lava")
    );
    assert_eq!(
        compiled.entries()[2].fluid().map(StableId::as_str),
        Some("terrenia:fluid/water")
    );
}

#[test]
fn occupancy_queue_depth_bound_fail_closes() {
    let catalog = catalog();
    let air = catalog
        .block(&stable_id("terrenia:block/air"))
        .expect("air");
    let context = latticeaxiom_content::OccupancyArbitrationContextV1::new(
        air.definition().header.stable_id.clone(),
        air.definition().default_state.clone(),
    )
    .expect("empty context");
    let water = FluidPaletteEntryV1::Fluid {
        fluid: stable_id("terrenia:fluid/water"),
        state: FluidStateV1 {
            level: FluidLevelV1::SOURCE,
            flow: FluidFlowV1::Still,
        },
    };
    let intents = vec![
        OccupancyCellIntentV1 {
            x: 0,
            y: 0,
            z: 0,
            solid: SolidPaletteEntryV1 {
                block: stable_id("terrenia:block/air"),
                state: BlockStateV1::empty(),
            },
            current_fluid: FluidPaletteEntryV1::Empty,
            requested_fluid: water.clone(),
        },
        OccupancyCellIntentV1 {
            x: 1,
            y: 0,
            z: 0,
            solid: SolidPaletteEntryV1 {
                block: stable_id("terrenia:block/air"),
                state: BlockStateV1::empty(),
            },
            current_fluid: FluidPaletteEntryV1::Empty,
            requested_fluid: water,
        },
    ];
    let error = OccupancyCandidateV1::compile(
        &catalog,
        &context,
        intents,
        BoundedFluidUpdatePolicyV1 {
            max_cells_per_tick: NonZeroU32::new(32).expect("cells"),
            max_queue_depth: NonZeroU32::new(1).expect("queue"),
            max_in_flight_bytes: NonZeroU32::new(65_536).expect("bytes"),
        },
        PaletteLimitsV1::default(),
    )
    .expect_err("two queued occupancy cells exceed a one-entry queue");
    assert!(matches!(
        error,
        ContentError::LimitExceeded {
            resource: "fluid_queue_depth",
            limit: 1,
            ..
        }
    ));
}
