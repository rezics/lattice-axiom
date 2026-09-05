//! Public D9 content-schema, palette, golden, negative, and property contracts.

use std::collections::BTreeSet;

use latticeaxiom_content::{
    BlockFormV1, CompiledFluidPaletteEntryV1, CompiledFluidPaletteV1, CompiledSolidPaletteV1,
    ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1, ContentError, ContentHeaderV1,
    FluidFlowV1, FluidLevelV1, FluidPaletteEntryV1, FluidStateV1, PaletteLimitsV1,
    SolidPaletteEntryV1,
};
use latticeaxiom_core::{CanonicalHash, StableId};
use proptest::prelude::*;

const FIXTURE: &str = include_str!("../fixtures/terrenia/representative-catalog-v1.json");
const BLOCK_INVENTORY: &str = include_str!("../fixtures/terrenia/expected-block-ids.txt");

fn stable_id(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("fixture StableId `{value}` is invalid: {error}"))
}

fn fixture_input() -> ContentCatalogInputV1 {
    serde_json::from_str(FIXTURE)
        .unwrap_or_else(|error| panic!("representative catalog fixture is invalid: {error}"))
}

fn fixture_catalog() -> ContentCatalogV1 {
    ContentCatalogV1::compile(fixture_input(), ContentCatalogLimitsV1::default())
        .unwrap_or_else(|error| panic!("representative catalog did not compile: {error}"))
}

#[test]
fn terrenia_inventory_has_exactly_72_unique_block_identities() {
    let ids = BLOCK_INVENTORY
        .lines()
        .filter(|line| !line.is_empty())
        .map(stable_id)
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 72);
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 72);
    assert!(
        ids.iter()
            .all(|id| id.kind() == "block" && id.major().is_none())
    );
    assert_eq!(
        CanonicalHash::digest(BLOCK_INVENTORY.as_bytes()).to_string(),
        "c9207aa78461efefe82dfd53be21e2b8626d73426e5aea37bd526ef751759e52"
    );

    for block in fixture_catalog().blocks() {
        assert!(ids.contains(&block.definition().header.stable_id));
    }
}

#[test]
fn representative_fixture_compiles_to_canonical_order() {
    let catalog = fixture_catalog();
    let block_ids = catalog
        .blocks()
        .iter()
        .map(|block| block.definition().header.stable_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        block_ids,
        [
            "terrenia:block/air",
            "terrenia:block/dirt",
            "terrenia:block/furnace",
            "terrenia:block/grass",
            "terrenia:block/oak-log",
            "terrenia:block/stone"
        ]
    );
    assert_eq!(catalog.fluids().len(), 2);
    assert_eq!(catalog.biomes().len(), 2);
    assert_eq!(catalog.material_role_bindings().len(), 4);

    let oak = catalog
        .block(&stable_id("terrenia:block/oak-log"))
        .unwrap_or_else(|| panic!("fixture lost oak-log"));
    assert_eq!(oak.states().len(), 3);
    assert_eq!(oak.default_state_index(), 1);
}

#[test]
fn authored_schema_round_trips_and_keeps_exact_fluid_identities() {
    let input = fixture_input();
    let json = serde_json::to_vec(&input)
        .unwrap_or_else(|error| panic!("authored catalog serialization failed: {error}"));
    let decoded = serde_json::from_slice::<ContentCatalogInputV1>(&json)
        .unwrap_or_else(|error| panic!("authored catalog round trip failed: {error}"));
    assert_eq!(decoded, input);

    let catalog = fixture_catalog();
    let fluid_ids = catalog
        .fluids()
        .iter()
        .map(|fluid| fluid.header.stable_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(fluid_ids, ["terrenia:fluid/lava", "terrenia:fluid/water"]);
}

#[test]
fn discovery_order_does_not_change_authoritative_bytes_or_hash() {
    let forward = fixture_catalog();
    let mut reversed_input = fixture_input();
    reversed_input.blocks.reverse();
    reversed_input.fluids.reverse();
    reversed_input.biomes.reverse();
    reversed_input.material_role_bindings.reverse();
    for block in &mut reversed_input.blocks {
        block.state_schema.reverse();
        block.states.reverse();
    }
    for biome in &mut reversed_input.biomes {
        biome.channel_offers.reverse();
        biome.emitted_intents.reverse();
    }
    let reversed = ContentCatalogV1::compile(reversed_input, ContentCatalogLimitsV1::default())
        .unwrap_or_else(|error| panic!("reversed fixture did not compile: {error}"));

    let forward_hash = forward
        .canonical_authoritative_hash()
        .unwrap_or_else(|error| panic!("forward canonical hash failed: {error}"));
    let reversed_hash = reversed
        .canonical_authoritative_hash()
        .unwrap_or_else(|error| panic!("reversed canonical hash failed: {error}"));
    assert_eq!(forward_hash, reversed_hash);
    let forward_bytes = forward
        .canonical_authoritative_bytes()
        .unwrap_or_else(|error| panic!("forward canonical encoding failed: {error}"));
    let reversed_bytes = reversed
        .canonical_authoritative_bytes()
        .unwrap_or_else(|error| panic!("reversed canonical encoding failed: {error}"));
    assert_eq!(forward_bytes, reversed_bytes);
}

#[test]
fn presentation_bindings_do_not_change_authoritative_hash() {
    let original = fixture_catalog();
    let mut input = fixture_input();
    for block in &mut input.blocks {
        block.presentation_binding = Some(stable_id("terrenia:asset/reloaded-block"));
    }
    for fluid in &mut input.fluids {
        fluid.presentation_binding = Some(stable_id("terrenia:asset/reloaded-fluid"));
    }
    let changed = ContentCatalogV1::compile(input, ContentCatalogLimitsV1::default())
        .unwrap_or_else(|error| panic!("presentation-only fixture failed: {error}"));
    let original_hash = original
        .canonical_authoritative_hash()
        .unwrap_or_else(|error| panic!("original canonical hash failed: {error}"));
    let changed_hash = changed
        .canonical_authoritative_hash()
        .unwrap_or_else(|error| panic!("presentation-only canonical hash failed: {error}"));
    assert_eq!(original_hash, changed_hash);
}

#[test]
fn canonical_axis_state_has_stable_golden_bytes() {
    let catalog = fixture_catalog();
    let oak = catalog
        .block(&stable_id("terrenia:block/oak-log"))
        .unwrap_or_else(|| panic!("fixture lost oak-log"));
    let bytes = oak
        .definition()
        .default_state
        .canonical_bytes()
        .unwrap_or_else(|error| panic!("axis state encoding failed: {error}"));
    assert_eq!(
        String::from_utf8(bytes).ok().as_deref(),
        Some(r#"{"values":{"latticeaxiom:block-state/axis@1":{"kind":"axis","value":"y"}}}"#)
    );
}

#[test]
fn representative_catalog_hash_matches_golden() {
    assert_eq!(
        fixture_catalog()
            .canonical_authoritative_hash()
            .unwrap_or_else(|error| panic!("catalog golden hash failed: {error}"))
            .to_string(),
        "7dd293d93c96a5a9a59f8c0ec11682d70b5527472602096ad98be5fc6d241253"
    );
}

#[test]
fn unknown_json_fields_are_rejected_before_compilation() {
    let json = r#"{
        "stable_id":"terrenia:block/air",
        "schema_id_and_major":"latticeaxiom:schema/block-definition@1",
        "content_revision":1,
        "declared_by_package":"terrenia",
        "surprise":true
    }"#;
    assert!(serde_json::from_str::<ContentHeaderV1>(json).is_err());
    assert!(
        serde_json::from_str::<latticeaxiom_content::BiomeScopeV1>(
            r#""terrenia:biome-scope/overworld""#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<latticeaxiom_content::FluidCollisionPolicyV1>(
            r#""terrenia:fluid-collision-policy/volume""#
        )
        .is_err()
    );
}

#[test]
fn duplicate_definition_wrong_schema_and_missing_default_are_rejected() {
    let mut duplicate = fixture_input();
    duplicate.blocks.push(duplicate.blocks[0].clone());
    assert!(matches!(
        ContentCatalogV1::compile(duplicate, ContentCatalogLimitsV1::default()),
        Err(ContentError::Duplicate {
            resource: "block definition",
            ..
        })
    ));

    let mut wrong_schema = fixture_input();
    wrong_schema.blocks[0].header.schema_id_and_major = "latticeaxiom:schema/fluid-definition@1"
        .parse()
        .unwrap_or_else(|error| panic!("test schema is invalid: {error}"));
    assert!(matches!(
        ContentCatalogV1::compile(wrong_schema, ContentCatalogLimitsV1::default()),
        Err(ContentError::WrongDefinitionSchema { .. })
    ));

    let mut missing_default = fixture_input();
    let furnace = missing_default
        .blocks
        .iter_mut()
        .find(|block| block.header.stable_id.as_str() == "terrenia:block/furnace")
        .unwrap_or_else(|| panic!("fixture lost furnace"));
    furnace
        .states
        .retain(|row| row.state != furnace.default_state);
    assert!(matches!(
        ContentCatalogV1::compile(missing_default, ContentCatalogLimitsV1::default()),
        Err(ContentError::InvalidBlockState { .. })
    ));
}

#[test]
fn wrong_identity_kind_state_subset_and_fluid_budget_are_rejected() {
    let mut wrong_kind = fixture_input();
    wrong_kind.fluids[0].header.stable_id = stable_id("terrenia:block/not-a-fluid");
    assert!(matches!(
        ContentCatalogV1::compile(wrong_kind, ContentCatalogLimitsV1::default()),
        Err(ContentError::WrongIdentityKind {
            expected: "fluid",
            ..
        })
    ));

    let mut missing_allowed_default = fixture_input();
    let oak = missing_allowed_default
        .blocks
        .iter_mut()
        .find(|block| block.header.stable_id.as_str() == "terrenia:block/oak-log")
        .unwrap_or_else(|| panic!("fixture lost oak-log"));
    let default_value = oak.state_schema[0].default_value.clone();
    oak.state_schema[0]
        .allowed_values
        .retain(|value| value != &default_value);
    assert!(matches!(
        ContentCatalogV1::compile(missing_allowed_default, ContentCatalogLimitsV1::default()),
        Err(ContentError::InvalidStateProperty { .. })
    ));

    let limits = ContentCatalogLimitsV1 {
        max_fluid_cells_per_tick: 1_024,
        ..ContentCatalogLimitsV1::default()
    };
    assert!(matches!(
        ContentCatalogV1::compile(fixture_input(), limits),
        Err(ContentError::LimitExceeded {
            resource: "fluid_cells_per_tick",
            actual: 2_048,
            limit: 1_024
        })
    ));
}

#[test]
fn role_target_fallback_cycle_and_collection_limit_are_rejected() {
    let mut missing_role_target = fixture_input();
    missing_role_target.material_role_bindings[0].block =
        stable_id("terrenia:block/not-registered");
    assert!(matches!(
        ContentCatalogV1::compile(missing_role_target, ContentCatalogLimitsV1::default()),
        Err(ContentError::UnknownReference {
            resource: "block",
            ..
        })
    ));

    let mut cycle = fixture_input();
    let woodland = cycle
        .biomes
        .iter_mut()
        .find(|biome| biome.header.stable_id.as_str() == "terrenia:biome/temperate-woodland")
        .unwrap_or_else(|| panic!("fixture lost woodland biome"));
    woodland.fallback = Some(stable_id("terrenia:biome/arid-badlands"));
    assert!(matches!(
        ContentCatalogV1::compile(cycle, ContentCatalogLimitsV1::default()),
        Err(ContentError::BiomeFallbackCycle { .. })
    ));

    let limits = ContentCatalogLimitsV1 {
        max_blocks: 5,
        ..ContentCatalogLimitsV1::default()
    };
    assert!(matches!(
        ContentCatalogV1::compile(fixture_input(), limits),
        Err(ContentError::LimitExceeded {
            resource: "block_definitions",
            actual: 6,
            limit: 5
        })
    ));
}

#[test]
fn block_form_is_definition_scoped_instead_of_a_frozen_public_enum() {
    let slab = BlockFormV1::new("slab")
        .unwrap_or_else(|error| panic!("definition-scoped slab token failed: {error}"));
    let owner_form = BlockFormV1::new("arched-owner-form")
        .unwrap_or_else(|error| panic!("owner form token failed: {error}"));
    assert_eq!(slab.as_str(), "slab");
    assert_eq!(owner_form.as_str(), "arched-owner-form");
    assert!(BlockFormV1::new("Not-Kebab").is_err());
    assert!(BlockFormV1::new("double--separator").is_err());
}

#[test]
fn fluid_level_and_flow_exist_only_on_fluid_state() {
    let catalog = fixture_catalog();
    for definition in catalog.fluids() {
        let value = serde_json::to_value(definition)
            .unwrap_or_else(|error| panic!("fluid definition serialization failed: {error}"));
        assert!(value.get("level").is_none());
        assert!(value.get("flow").is_none());
        assert_eq!(
            value
                .get("state_schema")
                .and_then(serde_json::Value::as_str),
            Some("latticeaxiom:schema/fluid-state@1")
        );
    }
    let state = FluidStateV1 {
        level: FluidLevelV1::SOURCE,
        flow: FluidFlowV1::Still,
    };
    let value = serde_json::to_value(state)
        .unwrap_or_else(|error| panic!("fluid state serialization failed: {error}"));
    assert!(value.get("level").is_some());
    assert!(value.get("flow").is_some());
}

#[test]
fn solid_and_fluid_palettes_validate_sort_and_reserve_empty() {
    let catalog = fixture_catalog();
    let air = catalog
        .block(&stable_id("terrenia:block/air"))
        .unwrap_or_else(|| panic!("fixture lost air"));
    let oak = catalog
        .block(&stable_id("terrenia:block/oak-log"))
        .unwrap_or_else(|| panic!("fixture lost oak-log"));
    let solid = CompiledSolidPaletteV1::compile(
        &catalog,
        vec![
            SolidPaletteEntryV1 {
                block: stable_id("terrenia:block/oak-log"),
                state: oak.definition().default_state.clone(),
            },
            SolidPaletteEntryV1 {
                block: stable_id("terrenia:block/air"),
                state: air.definition().default_state.clone(),
            },
        ],
        PaletteLimitsV1::default(),
    )
    .unwrap_or_else(|error| panic!("solid palette failed: {error}"));
    assert_eq!(solid.len(), 2);
    assert_eq!(solid.entries()[0].block().as_str(), "terrenia:block/air");

    let fluid = CompiledFluidPaletteV1::compile(
        &catalog,
        vec![
            FluidPaletteEntryV1::Fluid {
                fluid: stable_id("terrenia:fluid/water"),
                state: FluidStateV1 {
                    level: FluidLevelV1::SOURCE,
                    flow: FluidFlowV1::Still,
                },
            },
            FluidPaletteEntryV1::Empty,
            FluidPaletteEntryV1::Fluid {
                fluid: stable_id("terrenia:fluid/water"),
                state: FluidStateV1 {
                    level: FluidLevelV1::new(7)
                        .unwrap_or_else(|error| panic!("level seven failed: {error}")),
                    flow: FluidFlowV1::Down,
                },
            },
            FluidPaletteEntryV1::Fluid {
                fluid: stable_id("terrenia:fluid/lava"),
                state: FluidStateV1 {
                    level: FluidLevelV1::new(7)
                        .unwrap_or_else(|error| panic!("level seven failed: {error}")),
                    flow: FluidFlowV1::Down,
                },
            },
        ],
        PaletteLimitsV1::default(),
    )
    .unwrap_or_else(|error| panic!("fluid palette failed: {error}"));
    assert!(
        fluid
            .entries()
            .first()
            .is_some_and(CompiledFluidPaletteEntryV1::is_empty)
    );
    assert_eq!(
        fluid
            .entries()
            .get(1)
            .and_then(CompiledFluidPaletteEntryV1::fluid)
            .map(StableId::as_str),
        Some("terrenia:fluid/lava")
    );
    assert_eq!(
        fluid
            .entries()
            .get(2)
            .and_then(CompiledFluidPaletteEntryV1::state)
            .map(|state| (state.flow, state.level.get())),
        Some((FluidFlowV1::Down, 7))
    );
    assert_eq!(
        fluid
            .entries()
            .get(3)
            .and_then(CompiledFluidPaletteEntryV1::state)
            .map(|state| (state.flow, state.level.get())),
        Some((FluidFlowV1::Still, 0))
    );
}

#[test]
fn palette_rejects_duplicates_unknown_ids_and_hard_limits() {
    let catalog = fixture_catalog();
    let state = FluidStateV1 {
        level: FluidLevelV1::SOURCE,
        flow: FluidFlowV1::Still,
    };
    let duplicate = vec![
        FluidPaletteEntryV1::Fluid {
            fluid: stable_id("terrenia:fluid/water"),
            state,
        },
        FluidPaletteEntryV1::Fluid {
            fluid: stable_id("terrenia:fluid/water"),
            state,
        },
    ];
    assert!(matches!(
        CompiledFluidPaletteV1::compile(&catalog, duplicate, PaletteLimitsV1::default()),
        Err(ContentError::Duplicate {
            resource: "fluid palette entry",
            ..
        })
    ));
    assert!(matches!(
        CompiledFluidPaletteV1::compile(
            &catalog,
            vec![FluidPaletteEntryV1::Fluid {
                fluid: stable_id("terrenia:fluid/oil"),
                state,
            }],
            PaletteLimitsV1::default(),
        ),
        Err(ContentError::UnknownFluidPaletteEntry { .. })
    ));
    assert!(matches!(
        CompiledFluidPaletteV1::compile(
            &catalog,
            Vec::new(),
            PaletteLimitsV1 {
                max_solid_entries: 1,
                max_fluid_entries: 0,
            },
        ),
        Err(ContentError::LimitExceeded {
            resource: "fluid_palette_entries",
            actual: 1,
            limit: 0
        })
    ));
}

proptest! {
    #[test]
    fn every_valid_fluid_level_round_trips(level in 0_u8..=7) {
        let level = FluidLevelV1::new(level)
            .unwrap_or_else(|error| panic!("generated valid level failed: {error}"));
        let json = serde_json::to_string(&level)
            .unwrap_or_else(|error| panic!("level serialization failed: {error}"));
        let decoded = serde_json::from_str::<FluidLevelV1>(&json)
            .unwrap_or_else(|error| panic!("level round trip failed: {error}"));
        prop_assert_eq!(decoded, level);
    }

    #[test]
    fn every_out_of_range_fluid_level_is_rejected(level in 8_u8..=u8::MAX) {
        prop_assert!(FluidLevelV1::new(level).is_err());
        prop_assert!(serde_json::from_str::<FluidLevelV1>(&level.to_string()).is_err());
    }
}
