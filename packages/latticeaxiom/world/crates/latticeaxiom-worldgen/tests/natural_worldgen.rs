//! V5 natural-layer determinism, negative coordinates, and epoch-policy corpus.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test fixtures fail immediately when authored IDs or invariants are invalid"
)]

mod support;

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    AdjacentCellEpochV1, AdjacentEpochSnapshotV1, AuthoredWorldgenBindingsV1,
    BoundaryAdapterDeclarationV1, BoundaryReceiptV1, CellEpochStateV1, ChunkCoordinate,
    ChunkGenerationOutcomeV1, ChunkGenerationRequestV1, D4MaterialRoleV1, D7_NATURAL_BLOCK_COUNT,
    DimensionId, ExistingSnapshotEvidenceV1, GenerationPlanInputV1, GenerationPlanV1,
    NaturalLayerConfigV1, NaturalLayerInputV1, PlanActivationIdV1, PlanningCellCoordinateV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, TerrainStyleV1, WorldSeedV1,
    WorldgenConfigV1, WorldgenError, WorldgenLimitsV1,
};
use proptest::prelude::*;
use support::surface_terrain_programs;

const AUTHORED_BINDINGS_JSON: &str =
    include_str!("../../../../../terrenia/worldgen/data/authored-block-bindings-v1.json");

#[test]
fn authored_bindings_close_the_d7_natural_scope() {
    let bindings = authored_bindings();
    let catalog = bindings
        .catalog_closure()
        .expect("authored candidates form a catalog");
    assert!(catalog.blocks().len() >= D7_NATURAL_BLOCK_COUNT);
    let vocabulary = bindings
        .natural_vocabulary()
        .expect("catalog-* rows close every natural purpose");
    assert_eq!(vocabulary.iter().count(), D4MaterialRoleV1::NATURAL.len());
}

#[test]
fn natural_layer_is_deterministic_under_randomized_chunk_and_offer_order() {
    let chunks = [
        ChunkCoordinate::new(-4, 0, -3),
        ChunkCoordinate::new(-1, 1, 2),
        ChunkCoordinate::new(0, 0, 0),
        ChunkCoordinate::new(3, -1, -5),
        ChunkCoordinate::new(5, 2, 4),
    ];
    let forward = natural_plan(42, false);
    let reversed = natural_plan(42, true);
    assert_eq!(
        forward.generation_input_hash(),
        reversed.generation_input_hash()
    );
    assert_ne!(
        forward.generation_input_hash(),
        natural_plan(43, false).generation_input_hash()
    );

    let first = generate_all(&forward, chunks);
    let second = generate_all(&reversed, chunks.into_iter().rev());
    assert_eq!(first, second);
}

#[test]
fn negative_coordinates_match_independent_regeneration() {
    let plan = natural_plan(11, false);
    let coordinate = ChunkCoordinate::new(-7, 0, -9);
    let first = prepared(&plan, coordinate);
    let second = prepared(&natural_plan(11, true), coordinate);
    assert_eq!(first.0, second.0);
    assert_eq!(first.1, second.1);
    assert!(coordinate.x < 0 && coordinate.z < 0);
}

#[test]
fn far_surface_query_matches_final_natural_material_profile() {
    let plan = natural_plan(42, false);
    let edge = i64::from(plan.config().chunk_edge_voxels);
    for (x, z) in [(-31_i64, -17_i64), (-1, 0), (0, -1), (19, 37)] {
        let sample = plan
            .far_terrain_surface_sample(x, z)
            .expect("natural far-surface query succeeds");
        let coordinate = ChunkCoordinate::new(
            i32::try_from(x.div_euclid(edge)).expect("fixture chunk X fits"),
            sample
                .solid_y()
                .div_euclid(i32::from(plan.config().chunk_edge_voxels)),
            i32::try_from(z.div_euclid(edge)).expect("fixture chunk Z fits"),
        );
        let outcome = plan
            .generate(
                plan.vacant_generation_request(coordinate)
                    .expect("vacant request is valid"),
            )
            .expect("natural surface chunk materializes");
        let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
            panic!("natural surface fixture must prepare a candidate");
        };
        let local_x = u16::try_from(x.rem_euclid(edge)).expect("local X fits");
        let local_y = u16::try_from(
            sample
                .solid_y()
                .rem_euclid(i32::from(plan.config().chunk_edge_voxels)),
        )
        .expect("local Y fits");
        let local_z = u16::try_from(z.rem_euclid(edge)).expect("local Z fits");
        assert_eq!(
            candidate.draft().block_at(local_x, local_y, local_z),
            Some(plan.role_target(sample.material())),
            "far terrain must reuse the final slope-aware natural top at ({x}, {}, {z})",
            sample.solid_y()
        );
    }
}

#[test]
fn river_plan_is_continuous_across_style_boundaries() {
    let plan = natural_plan(42, false);
    let mut found_crossing = false;
    for z in -64..64 {
        for x in -64..64 {
            let current = plan.territory_query(x, z);
            let east = plan.territory_query(x.saturating_add(1), z);
            if current.winner() == east.winner() {
                continue;
            }
            let Some(left) = plan.river_sample(x, z) else {
                continue;
            };
            let Some(right) = plan.river_sample(x.saturating_add(1), z) else {
                continue;
            };
            if left.in_channel() && right.in_channel() {
                assert!(
                    left.distance_voxels().abs_diff(right.distance_voxels()) <= 1,
                    "channel distance must change by at most one voxel across a territory edge"
                );
                found_crossing = true;
            }
        }
    }
    if !found_crossing {
        let origin = plan
            .river_sample(0, 0)
            .expect("natural plans expose river samples");
        let neighbor = plan
            .river_sample(1, 0)
            .expect("adjacent columns remain queryable");
        assert_eq!(origin.basin(), neighbor.basin());
    }
}

#[test]
fn terrain_density_uses_the_river_adjusted_surface() {
    let plan = natural_plan(42, false);
    let (x, z) = (-256_i64..=256)
        .flat_map(|z| (-256_i64..=256).map(move |x| (x, z)))
        .find(|&(x, z)| {
            plan.river_sample(x, z)
                .is_some_and(latticeaxiom_worldgen::RiverSampleV1::in_channel)
        })
        .expect("fixture contains a surface river channel");
    let surface_y = i64::from(plan.terrain_height(x, z));

    assert!(plan.terrain_density(x, surface_y, z) >= 0);
    assert!(
        plan.terrain_density(x, surface_y.saturating_add(1), z) < 0,
        "the first voxel above an incised river bed must be void"
    );
}

#[test]
fn combined_terrain_column_matches_independent_queries() {
    let plan = natural_plan(42, false);
    for (x, z) in [(-257, -129), (-1, 0), (0, 0), (193, 511)] {
        let column = plan.terrain_column(x, z);
        assert_eq!(column.height(), plan.terrain_height(x, z));
        assert_eq!(column.family(), plan.terrain_family(x, z));
        assert_eq!(column.surface_water_y(), plan.surface_water_level(x, z));
    }
}

#[test]
fn river_distance_is_lipschitz_across_coarse_cell_boundaries() {
    let plan = natural_plan(42, false);
    for z in -96..96 {
        for x in -96..96 {
            let here = plan.river_sample(x, z).expect("natural river sample");
            let east = plan
                .river_sample(x.saturating_add(1), z)
                .expect("east river sample");
            let south = plan
                .river_sample(x, z.saturating_add(1))
                .expect("south river sample");
            assert!(
                here.distance_voxels().abs_diff(east.distance_voxels()) <= 1,
                "river distance must be one-voxel Lipschitz on X at ({x},{z})"
            );
            assert!(
                here.distance_voxels().abs_diff(south.distance_voxels()) <= 1,
                "river distance must be one-voxel Lipschitz on Z at ({x},{z})"
            );
        }
    }
}

#[test]
fn exclusion_radius_rejects_closer_tree_anchors() {
    let plan = natural_plan(42, false);
    let radius = i64::from(NaturalLayerConfigV1::default().tree_exclusion_radius_voxels);
    let planning_edge = i64::from(plan.config().chunk_edge_voxels)
        .saturating_mul(i64::from(plan.config().planning_cell_edge_chunks));
    let sea = plan.terrain_config().world.sea_level_y;
    let (center_x, center_z) = (-32_i64..=32)
        .flat_map(|cell_z| (-32_i64..=32).map(move |cell_x| (cell_x, cell_z)))
        .find_map(|(cell_x, cell_z)| {
            let x = cell_x.saturating_mul(planning_edge);
            let z = cell_z.saturating_mul(planning_edge);
            (plan.territory_query(x, z).winner() == TerrainStyleV1::TemperateWoodland
                && plan.terrain_height(x, z) > sea)
                .then_some((x, z))
        })
        .expect("fixture seed contains dry temperate terrain");
    let mut drafts = std::collections::BTreeMap::new();
    let mut accepted = Vec::new();
    for z in center_z.saturating_sub(32)..center_z.saturating_add(32) {
        for x in center_x.saturating_sub(32)..center_x.saturating_add(32) {
            if is_natural_tree_column(&plan, x, z, &mut drafts) {
                accepted.push((x, z));
            }
        }
    }
    assert!(!accepted.is_empty(), "fixture seed must place trees");
    for (index, (x, z)) in accepted.iter().enumerate() {
        for (ox, oz) in accepted.iter().skip(index.saturating_add(1)) {
            let chebyshev = (x - ox).abs().max((z - oz).abs());
            assert!(
                chebyshev > radius,
                "trees at ({x},{z}) and ({ox},{oz}) violate exclusion radius {radius}"
            );
        }
    }
}

#[test]
fn tree_morphology_config_reserves_disjoint_canopies_and_full_headroom() {
    let spine = WorldgenConfigV1::default();
    let too_close = NaturalLayerConfigV1 {
        tree_exclusion_radius_voxels: 5,
        ..NaturalLayerConfigV1::default()
    };
    assert!(matches!(
        too_close.validate(&spine),
        Err(WorldgenError::InvalidConfig {
            field: "tree_exclusion_radius_voxels",
            ..
        })
    ));

    let maximum_surface = spine
        .temperate_base_height
        .saturating_add(i32::from(spine.temperate_relief))
        .max(
            spine
                .arid_base_height
                .saturating_add(i32::from(spine.arid_relief)),
        );
    let short_spine = WorldgenConfigV1 {
        world_ceiling_y: maximum_surface.saturating_add(9),
        ..spine
    };
    assert!(matches!(
        NaturalLayerConfigV1::default().validate(&short_spine),
        Err(WorldgenError::InvalidConfig {
            field: "world_ceiling_y",
            ..
        })
    ));
}

#[test]
fn marine_columns_never_receive_terrestrial_vegetation() {
    let plan = natural_plan(42, false);
    let (boundary_x, boundary_z) = marine_land_boundary(&plan);
    let edge = i64::from(plan.config().chunk_edge_voxels);
    let center_chunk_x = boundary_x.div_euclid(edge);
    let center_chunk_z = boundary_z.div_euclid(edge);
    let vegetation = [
        D4MaterialRoleV1::WoodlandLog,
        D4MaterialRoleV1::WoodlandLeaves,
        D4MaterialRoleV1::WoodlandGroundCover,
        D4MaterialRoleV1::BorealLog,
        D4MaterialRoleV1::BorealLeaves,
        D4MaterialRoleV1::Moss,
        D4MaterialRoleV1::Peat,
    ]
    .into_iter()
    .map(|role| plan.role_target(role).clone())
    .collect::<std::collections::BTreeSet<_>>();
    let mut marine_columns = 0_u64;

    for chunk_z in center_chunk_z.saturating_sub(1)..=center_chunk_z.saturating_add(1) {
        for chunk_x in center_chunk_x.saturating_sub(1)..=center_chunk_x.saturating_add(1) {
            let origin_x = chunk_x.saturating_mul(edge);
            let origin_z = chunk_z.saturating_mul(edge);
            let mut minimum_y = i64::MAX;
            let mut maximum_y = i64::MIN;
            for local_z in 0..edge {
                for local_x in 0..edge {
                    let x = origin_x.saturating_add(local_x);
                    let z = origin_z.saturating_add(local_z);
                    let height = i64::from(plan.terrain_height(x, z));
                    minimum_y = minimum_y.min(height.saturating_add(1));
                    maximum_y = maximum_y.max(height.saturating_add(6));
                }
            }
            for chunk_y in minimum_y.div_euclid(edge)..=maximum_y.div_euclid(edge) {
                let coordinate = ChunkCoordinate::new(
                    i32::try_from(chunk_x).expect("test chunk X fits i32"),
                    i32::try_from(chunk_y).expect("test chunk Y fits i32"),
                    i32::try_from(chunk_z).expect("test chunk Z fits i32"),
                );
                let outcome = plan
                    .generate(plan.vacant_generation_request(coordinate).unwrap())
                    .expect("shore chunk generates");
                let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
                    panic!("shore scan requires a new candidate");
                };
                for local_z in 0..edge {
                    for local_x in 0..edge {
                        let x = origin_x.saturating_add(local_x);
                        let z = origin_z.saturating_add(local_z);
                        if plan.material_style(x, z) != TerrainStyleV1::Marine {
                            continue;
                        }
                        marine_columns = marine_columns.saturating_add(1);
                        for local_y in 0..edge {
                            let block = candidate
                                .draft()
                                .block_at(
                                    u16::try_from(local_x).expect("local X fits u16"),
                                    u16::try_from(local_y).expect("local Y fits u16"),
                                    u16::try_from(local_z).expect("local Z fits u16"),
                                )
                                .expect("local voxel is inside the draft");
                            assert!(
                                !vegetation.contains(block),
                                "terrestrial vegetation leaked into marine column ({x},{z})"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(marine_columns > 0, "shore scan must cover marine columns");
}

#[test]
fn materialized_snapshot_is_reused_after_compatible_natural_provider_update() {
    let old_plan = natural_plan(42, false);
    let coordinate = ChunkCoordinate::new(-2, 0, 3);
    let old_outcome = old_plan
        .generate(old_plan.vacant_generation_request(coordinate).unwrap())
        .expect("old natural chunk generates");
    let ChunkGenerationOutcomeV1::Prepared(old_candidate) = old_outcome else {
        panic!("expected a new snapshot candidate");
    };
    let existing = ExistingSnapshotEvidenceV1::new(
        old_plan.dimension().clone(),
        coordinate,
        PlanningCellCoordinateV1::from_chunk(
            coordinate,
            old_plan.config().planning_cell_edge_chunks,
        ),
        old_plan.generation_epoch(),
        latticeaxiom_storage::ChunkRevision::new(4),
        1,
        old_candidate.snapshot_bytes().to_vec(),
        old_candidate.checksum(),
    )
    .expect("candidate bytes match checksum");

    let mut offers = natural_provider_offers(false);
    let position = offers
        .iter()
        .position(|offer| offer.slot() == ProviderSlotV1::Vegetation)
        .expect("natural vegetation provider exists");
    offers[position] = ProviderOfferV1::new(
        ProviderSlotV1::Vegetation,
        ProviderGenerationIdentityV1::new(
            stable_id("fixture:worldgen-provider/vegetation@1"),
            NonZeroU32::MIN,
            9,
            CanonicalHash::digest(b"vegetation-implementation-v9"),
        ),
    );
    let new_plan = compile_natural(42, d4_provider_offers(false), offers);
    assert_ne!(old_plan.generation_epoch(), new_plan.generation_epoch());
    let reused = new_plan
        .generate(ChunkGenerationRequestV1::new(
            coordinate,
            Some(existing.clone()),
            CellEpochStateV1::Frozen(old_plan.generation_epoch()),
            AdjacentEpochSnapshotV1::all_unassigned(PlanningCellCoordinateV1::from_chunk(
                coordinate,
                new_plan.config().planning_cell_edge_chunks,
            ))
            .unwrap(),
            Vec::new(),
        ))
        .expect("existing snapshot wins before regeneration");
    assert_eq!(reused, ChunkGenerationOutcomeV1::Existing(existing));
}

#[test]
fn verified_boundary_receipt_allows_adjacent_new_epoch_without_rewriting_old_bytes() {
    let old_plan = compile_natural_at_revision(
        7,
        7,
        d4_provider_offers(false),
        natural_provider_offers(false),
    );
    let old_chunk = ChunkCoordinate::new(0, 0, 0);
    let old_outcome = old_plan
        .generate(old_plan.vacant_generation_request(old_chunk).unwrap())
        .expect("old cell materializes");
    let ChunkGenerationOutcomeV1::Prepared(old_candidate) = old_outcome else {
        panic!("old cell requires a new candidate");
    };
    let old_bytes = old_candidate.snapshot_bytes().to_vec();
    let old_checksum = old_candidate.checksum();

    let new_plan = compile_natural_at_revision(
        7,
        8,
        d4_provider_offers(false),
        natural_provider_offers(false),
    );
    assert_ne!(old_plan.generation_epoch(), new_plan.generation_epoch());
    let new_chunk =
        ChunkCoordinate::new(i32::from(old_plan.config().planning_cell_edge_chunks), 0, 0);
    let old_cell = PlanningCellCoordinateV1::from_chunk(
        old_chunk,
        old_plan.config().planning_cell_edge_chunks,
    );
    let new_cell = PlanningCellCoordinateV1::from_chunk(
        new_chunk,
        new_plan.config().planning_cell_edge_chunks,
    );
    let declaration = BoundaryAdapterDeclarationV1::new(
        stable_id("fixture:transition-adapter/natural-epoch@1"),
        NonZeroU32::MIN,
        CanonicalHash::digest(b"natural-adapter-artifact"),
        old_plan.generation_epoch(),
        new_plan.generation_epoch(),
        NonZeroU32::MIN,
        CanonicalHash::digest(b"natural-terrain-signature"),
        Vec::new(),
    )
    .expect("declaration is bounded");
    let receipt =
        BoundaryReceiptV1::from_verified_declaration(&declaration, old_cell, new_cell).unwrap();
    let snapshot = AdjacentEpochSnapshotV1::all_unassigned(new_cell).unwrap();
    let neighbors = (*snapshot.neighbors()).map(|neighbor| {
        if neighbor.cell() == old_cell {
            AdjacentCellEpochV1::frozen(old_cell, old_plan.generation_epoch())
        } else {
            neighbor
        }
    });
    let adjacent = AdjacentEpochSnapshotV1::new(new_cell, neighbors).unwrap();
    let prepared = new_plan
        .generate(
            ChunkGenerationRequestV1::new(
                new_chunk,
                None,
                CellEpochStateV1::Unassigned,
                adjacent,
                vec![declaration],
            )
            .with_boundary_receipts(vec![receipt]),
        )
        .expect("verified receipt permits the new epoch cell");
    assert!(matches!(prepared, ChunkGenerationOutcomeV1::Prepared(_)));

    let existing = ExistingSnapshotEvidenceV1::new(
        old_plan.dimension().clone(),
        old_chunk,
        old_cell,
        old_plan.generation_epoch(),
        latticeaxiom_storage::ChunkRevision::new(1),
        1,
        old_bytes.clone(),
        old_checksum,
    )
    .unwrap();
    let reused = new_plan
        .generate(ChunkGenerationRequestV1::new(
            old_chunk,
            Some(existing.clone()),
            CellEpochStateV1::Frozen(old_plan.generation_epoch()),
            AdjacentEpochSnapshotV1::all_unassigned(old_cell).unwrap(),
            Vec::new(),
        ))
        .unwrap();
    let ChunkGenerationOutcomeV1::Existing(returned) = reused else {
        panic!("old snapshot must not regenerate");
    };
    assert_eq!(returned.bytes(), old_bytes);
    assert_eq!(returned.checksum(), old_checksum);
    assert_eq!(returned.writer_count(), 1);
}

#[test]
fn missing_natural_role_fails_closed_before_generation() {
    let bindings = authored_bindings();
    let Err(error) = GenerationPlanV1::compile(
        GenerationPlanInputV1::new(
            dimension_id(),
            WorldSeedV1::from_integer(42),
            WorldgenConfigV1::default(),
            7,
            PlanActivationIdV1::from_hash(CanonicalHash::digest(b"natural-activation")),
            d4_provider_offers(false),
            bindings.d4_vocabulary().unwrap(),
            bindings.role_bindings().unwrap(),
            bindings.catalog_closure().unwrap(),
            CanonicalHash::digest(b"authoritative-semantic-image"),
            vec![CanonicalHash::digest(b"lock-a")],
            WorldgenLimitsV1::default(),
        )
        .with_surface_biome_terrain_programs(surface_terrain_programs(true, false))
        .with_natural_layer(NaturalLayerInputV1::new(
            NaturalLayerConfigV1::default(),
            bindings.natural_vocabulary().unwrap(),
            natural_provider_offers(false)
                .into_iter()
                .filter(|offer| offer.slot() != ProviderSlotV1::Geology)
                .collect(),
        )),
    ) else {
        panic!("missing geology provider must fail closed");
    };
    assert!(matches!(
        error,
        WorldgenError::MissingProvider {
            slot: ProviderSlotV1::Geology
        }
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn shuffled_natural_chunks_are_byte_identical(permutation in 0_u8..24) {
        let plan = natural_plan(99, permutation % 2 == 1);
        let mut chunks = [
            ChunkCoordinate::new(-3, 0, 2),
            ChunkCoordinate::new(1, -1, -4),
            ChunkCoordinate::new(4, 1, 0),
            ChunkCoordinate::new(-6, 0, -1),
        ];
        rotate_by(&mut chunks, usize::from(permutation % 4));
        let first = generate_all(&plan, chunks);
        rotate_by(&mut chunks, 1);
        let second = generate_all(&plan, chunks);
        prop_assert_eq!(first, second);
    }
}

fn is_natural_tree_column(
    plan: &GenerationPlanV1,
    x: i64,
    z: i64,
    drafts: &mut std::collections::BTreeMap<ChunkCoordinate, latticeaxiom_worldgen::ChunkDraftV1>,
) -> bool {
    let height = i64::from(plan.terrain_height(x, z));
    let trunk_y = height.saturating_add(1);
    let chunk = column_chunk(x, trunk_y, z);
    let draft = drafts.entry(chunk).or_insert_with(|| {
        let outcome = plan
            .generate(plan.vacant_generation_request(chunk).unwrap())
            .expect("tree-search chunk generates");
        let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
            panic!("tree-search requires a new candidate");
        };
        candidate.draft().clone()
    });
    let edge = i64::from(plan.config().chunk_edge_voxels);
    let origin_x = i64::from(chunk.x).saturating_mul(edge);
    let origin_y = i64::from(chunk.y).saturating_mul(edge);
    let origin_z = i64::from(chunk.z).saturating_mul(edge);
    let Ok(local_x) = u16::try_from(x.saturating_sub(origin_x)) else {
        return false;
    };
    let Ok(local_z) = u16::try_from(z.saturating_sub(origin_z)) else {
        return false;
    };
    let Ok(local_y) = u16::try_from(trunk_y.saturating_sub(origin_y)) else {
        return false;
    };
    let Some(block) = draft.block_at(local_x, local_y, local_z) else {
        return false;
    };
    block == plan.role_target(D4MaterialRoleV1::WoodlandLog)
        || block == plan.role_target(D4MaterialRoleV1::BorealLog)
}

fn column_chunk(x: i64, y: i64, z: i64) -> ChunkCoordinate {
    ChunkCoordinate::new(
        i32::try_from(x.div_euclid(16)).unwrap_or_default(),
        i32::try_from(y.div_euclid(16)).unwrap_or_default(),
        i32::try_from(z.div_euclid(16)).unwrap_or_default(),
    )
}

fn generate_all(
    plan: &GenerationPlanV1,
    chunks: impl IntoIterator<Item = ChunkCoordinate>,
) -> Vec<(ChunkCoordinate, Vec<u8>, String)> {
    let mut generated = chunks
        .into_iter()
        .map(|coordinate| {
            let (bytes, checksum) = prepared(plan, coordinate);
            (coordinate, bytes, checksum)
        })
        .collect::<Vec<_>>();
    generated.sort_by_key(|(coordinate, _, _)| (coordinate.x, coordinate.y, coordinate.z));
    generated
}

fn prepared(plan: &GenerationPlanV1, coordinate: ChunkCoordinate) -> (Vec<u8>, String) {
    let outcome = plan
        .generate(plan.vacant_generation_request(coordinate).unwrap())
        .expect("natural chunk generates");
    let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
        panic!("expected a prepared snapshot candidate");
    };
    (
        candidate.snapshot_bytes().to_vec(),
        candidate.checksum().to_string(),
    )
}

fn natural_plan(seed: i64, reverse: bool) -> GenerationPlanV1 {
    compile_natural(
        seed,
        d4_provider_offers(reverse),
        natural_provider_offers(reverse),
    )
}

fn marine_land_boundary(plan: &GenerationPlanV1) -> (i64, i64) {
    let planning_edge = i64::from(plan.config().chunk_edge_voxels)
        .saturating_mul(i64::from(plan.config().planning_cell_edge_chunks));
    for cell_z in -32_i64..=32 {
        for cell_x in -32_i64..=32 {
            let start_x = cell_x.saturating_mul(planning_edge);
            let z = cell_z.saturating_mul(planning_edge);
            let start_marine = plan.material_style(start_x, z) == TerrainStyleV1::Marine;
            for offset in 1..=planning_edge {
                let x = start_x.saturating_add(offset);
                let marine = plan.material_style(x, z) == TerrainStyleV1::Marine;
                if marine != start_marine {
                    return (x, z);
                }
            }
        }
    }
    panic!("fixture seed must contain a marine/land boundary");
}

fn compile_natural(
    seed: i64,
    d4_offers: Vec<ProviderOfferV1>,
    natural_offers: Vec<ProviderOfferV1>,
) -> GenerationPlanV1 {
    compile_natural_at_revision(seed, 7, d4_offers, natural_offers)
}

fn compile_natural_at_revision(
    seed: i64,
    revision: u64,
    d4_offers: Vec<ProviderOfferV1>,
    natural_offers: Vec<ProviderOfferV1>,
) -> GenerationPlanV1 {
    let bindings = authored_bindings();
    GenerationPlanV1::compile(
        GenerationPlanInputV1::new(
            dimension_id(),
            WorldSeedV1::from_integer(seed),
            WorldgenConfigV1::default(),
            revision,
            PlanActivationIdV1::from_hash(CanonicalHash::digest(b"natural-activation")),
            d4_offers,
            bindings.d4_vocabulary().expect("D4 vocabulary"),
            bindings.role_bindings().expect("role bindings"),
            bindings.catalog_closure().expect("catalog"),
            CanonicalHash::digest(b"authoritative-semantic-image"),
            vec![CanonicalHash::digest(b"lock-a")],
            WorldgenLimitsV1::default(),
        )
        .with_surface_biome_terrain_programs(surface_terrain_programs(true, false))
        .with_natural_layer(NaturalLayerInputV1::new(
            NaturalLayerConfigV1::default(),
            bindings.natural_vocabulary().expect("natural vocabulary"),
            natural_offers,
        )),
    )
    .expect("natural plan compiles")
}

fn authored_bindings() -> AuthoredWorldgenBindingsV1 {
    AuthoredWorldgenBindingsV1::from_json(AUTHORED_BINDINGS_JSON.as_bytes())
        .expect("authored bindings decode")
}

fn d4_provider_offers(reverse: bool) -> Vec<ProviderOfferV1> {
    let mut offers = slot_offers([
        (ProviderSlotV1::GenerationCoordinator, "coordinator", 7),
        (ProviderSlotV1::StyleSelector, "selector", 7),
        (ProviderSlotV1::TerrainTransition, "transition", 7),
        (ProviderSlotV1::CaveTopology, "cave", 8),
        (ProviderSlotV1::Materializer, "materializer", 7),
    ]);
    if reverse {
        offers.reverse();
    }
    offers
}

fn natural_provider_offers(reverse: bool) -> Vec<ProviderOfferV1> {
    let mut offers = slot_offers([
        (ProviderSlotV1::Geology, "geology", 1),
        (ProviderSlotV1::Hydrology, "hydrology", 1),
        (ProviderSlotV1::Resources, "resources", 1),
        (ProviderSlotV1::Vegetation, "vegetation", 1),
    ]);
    if reverse {
        offers.reverse();
    }
    offers
}

fn slot_offers<const N: usize>(
    slots: [(ProviderSlotV1, &'static str, u32); N],
) -> Vec<ProviderOfferV1> {
    slots
        .into_iter()
        .map(|(slot, path, revision)| {
            ProviderOfferV1::new(
                slot,
                ProviderGenerationIdentityV1::new(
                    stable_id(&format!("fixture:worldgen-provider/{path}@1")),
                    NonZeroU32::MIN,
                    revision,
                    CanonicalHash::digest(format!("{path}-implementation-v{revision}")),
                ),
            )
        })
        .collect()
}

fn rotate_by(chunks: &mut [ChunkCoordinate], count: usize) {
    let len = chunks.len();
    if len == 0 {
        return;
    }
    for _ in 0..count % len {
        let first = chunks[0];
        chunks.copy_within(1.., 0);
        chunks[len - 1] = first;
    }
}

fn dimension_id() -> DimensionId {
    "fixture:dimension/natural"
        .parse()
        .expect("fixture dimension is valid")
}

fn stable_id(value: &str) -> StableId {
    value.parse().expect("fixture stable IDs are valid")
}
