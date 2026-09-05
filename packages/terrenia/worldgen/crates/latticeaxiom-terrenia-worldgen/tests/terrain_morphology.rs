//! Fixed-corpus characterization and acceptance metrics for semantic terrain morphology.

#![allow(
    clippy::expect_used,
    reason = "a malformed package-owned terrain fixture must fail immediately"
)]

use latticeaxiom_core::CanonicalHash;
use latticeaxiom_terrenia_worldgen::{
    TerrainPresetV2, semantic_surface_biome_terrain_programs,
    semantic_surface_biome_terrain_programs_v1,
};
use latticeaxiom_worldgen::{SemanticTerrainFieldV1, WorldSeedV1, WorldgenSeedRootV2};

const ADJACENCY_EDGE: usize = 192;
const ADJACENCY_ORIGINS: [(i64, i64); 4] =
    [(-1_536, -1_024), (-256, 128), (1_024, -768), (-896, 1_536)];
const SHORELINE_ORIGINS: [(i64, i64); 6] = [
    (-15_008, -16_480),
    (-9_248, -12_384),
    (1_632, -8_288),
    (-5_024, -4_192),
    (9_184, -96),
    (16_224, 4_000),
];
const RELIEF_CENTERS: [(i64, i64); 4] = [(-2_048, -2_048), (0, 0), (2_048, 1_024), (-1_024, 3_072)];
const RELIEF_WINDOWS: [i64; 3] = [64, 256, 1_024];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ColumnMetricSample {
    height: i32,
    land: bool,
    continentalness_per_1024: i16,
    middle_relief_per_1024: i16,
    middle_relief_displacement_q8: i32,
    plateau_displacement_q8: i32,
    plateau_interior: bool,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct ShorelineDiagnostic {
    shoreline_edges: u64,
    shoreline_delta_ge_two_edges: u64,
    maximum_shoreline_jump: u32,
    p95_shoreline_jump: u32,
    shoreline_jump_sum: u64,
    coastal_land_edges: u64,
    coastal_delta_ge_two_edges: u64,
    inland_land_edges: u64,
    inland_delta_ge_two_edges: u64,
    delta_ge_two_edges: u64,
    middle_relief_displacement_delta_ge_two_edges: u64,
    plateau_displacement_delta_ge_two_edges: u64,
    delta_histogram: [u64; 13],
    saturated_middle_relief_columns: u64,
    saturated_middle_relief_per_million: u64,
    sampled_columns: u64,
}

#[derive(Debug, Eq, PartialEq)]
struct MorphologyMetrics {
    land_columns: u64,
    land_edges: u64,
    delta_ge_two_edges: u64,
    delta_ge_two_per_million: u64,
    maximum_adjacent_delta: u32,
    walkable_neighbor_per_million: u64,
    exposed_cliff_faces: u64,
    maximum_flat_run_x: u16,
    maximum_flat_run_z: u16,
    p95_flat_run_x: u16,
    p95_flat_run_z: u16,
    flat_interior_cells: u64,
    flat_metric_columns: u64,
    flat_interior_per_million: u64,
    maximum_flat_component_cells: u32,
    maximum_flat_component_span: u16,
    local_relief_voxels: [u32; 3],
    minimum_land_height: i32,
    maximum_land_height: i32,
}

#[test]
fn semantic_v1_morphology_baseline_is_frozen() {
    let field = semantic_v1_field();
    assert_eq!(
        field
            .policy()
            .canonical_hash()
            .expect("legacy policy canonicalizes")
            .to_string(),
        "9a6d39b7c82c0106161b1100e1be5119dd56ecf18617ffe12604f8e783f7b96d"
    );
    assert_eq!(
        density_digest(&field).to_string(),
        "1686547f6dcf9b03527d70515cad5f5c1adb4cadcad36ef1fb6a801b46cb614c"
    );
    let metrics = morphology_metrics(&field);
    assert_eq!(
        metrics,
        MorphologyMetrics {
            land_columns: 110_592,
            land_edges: 220_032,
            delta_ge_two_edges: 0,
            delta_ge_two_per_million: 0,
            maximum_adjacent_delta: 1,
            walkable_neighbor_per_million: 1_000_000,
            exposed_cliff_faces: 0,
            maximum_flat_run_x: 192,
            maximum_flat_run_z: 136,
            p95_flat_run_x: 13,
            p95_flat_run_z: 9,
            flat_interior_cells: 67_369,
            flat_metric_columns: 110_592,
            flat_interior_per_million: 609_167,
            maximum_flat_component_cells: 19_935,
            maximum_flat_component_span: 190,
            local_relief_voxels: [2, 7, 92],
            minimum_land_height: 93,
            maximum_land_height: 224,
        }
    );
}

#[test]
fn semantic_v2_morphology_meets_frozen_quality_gates() {
    let field = semantic_v2_field();
    assert!(field.policy().morphology().is_some());
    assert_eq!(
        field
            .policy()
            .canonical_hash()
            .expect("current policy canonicalizes")
            .to_string(),
        "830041efaa8ee32e09cf3b51cbea53552362b6554ad368bce9b55ff35b72dd85"
    );
    assert_eq!(
        density_digest(&field).to_string(),
        "3907697f97623c9c5f6e9eab10870d9c54c1be3f2a35eeeb4f32ef8d45fc5f52"
    );
    let metrics = morphology_metrics(&field);
    assert_eq!(metrics.land_edges, 220_032, "{metrics:#?}");
    assert!(
        (2_500..=75_000).contains(&metrics.delta_ge_two_per_million),
        "{metrics:#?}"
    );
    assert!(
        (2..=12).contains(&metrics.maximum_adjacent_delta),
        "{metrics:#?}"
    );
    assert!(metrics.exposed_cliff_faces >= 550, "{metrics:#?}");
    assert!(
        metrics.walkable_neighbor_per_million >= 925_000,
        "{metrics:#?}"
    );
    assert!(metrics.maximum_flat_run_x <= 128, "{metrics:#?}");
    assert!(metrics.maximum_flat_run_z <= 128, "{metrics:#?}");
    assert!(metrics.flat_interior_per_million <= 450_000, "{metrics:#?}");
    assert!(
        metrics.maximum_flat_component_cells <= 8_000,
        "{metrics:#?}"
    );
    assert!(metrics.maximum_flat_component_span < 190, "{metrics:#?}");
    assert!(metrics.local_relief_voxels[0] >= 3, "{metrics:#?}");
    assert!(metrics.local_relief_voxels[1] >= 12, "{metrics:#?}");
    assert!(
        (80..=160).contains(&metrics.local_relief_voxels[2]),
        "{metrics:#?}"
    );
    let world = TerrainPresetV2::Balanced.resolve().world;
    assert!(metrics.minimum_land_height > world.floor_y, "{metrics:#?}");
    assert!(
        metrics.maximum_land_height <= world.ceiling_y - 16,
        "{metrics:#?}"
    );
}

#[test]
fn semantic_v1_shoreline_discontinuity_is_frozen() {
    assert_eq!(
        terrain_diagnostic(&semantic_v1_field(), &SHORELINE_ORIGINS),
        ShorelineDiagnostic {
            shoreline_edges: 1_553,
            shoreline_delta_ge_two_edges: 1_553,
            maximum_shoreline_jump: 136,
            p95_shoreline_jump: 135,
            shoreline_jump_sum: 120_969,
            coastal_land_edges: 236_346,
            coastal_delta_ge_two_edges: 0,
            inland_land_edges: 0,
            inland_delta_ge_two_edges: 0,
            delta_ge_two_edges: 0,
            middle_relief_displacement_delta_ge_two_edges: 0,
            plateau_displacement_delta_ge_two_edges: 0,
            delta_histogram: [219_787, 16_559, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            saturated_middle_relief_columns: 0,
            saturated_middle_relief_per_million: 0,
            sampled_columns: 221_184,
        }
    );
}

#[test]
fn semantic_v2_keeps_shores_continuous_and_distributes_cliffs() {
    let field = semantic_v2_field();
    let shoreline = terrain_diagnostic(&field, &SHORELINE_ORIGINS);
    assert_eq!(shoreline.shoreline_edges, 1_553, "{shoreline:#?}");
    assert!(shoreline.p95_shoreline_jump <= 2, "{shoreline:#?}");
    assert!(shoreline.maximum_shoreline_jump <= 12, "{shoreline:#?}");
    assert!(
        ratio_per_million(
            shoreline.shoreline_delta_ge_two_edges,
            shoreline.shoreline_edges,
        ) <= 50_000,
        "{shoreline:#?}"
    );
    assert!(
        shoreline.saturated_middle_relief_per_million <= 10_000,
        "{shoreline:#?}"
    );

    let land = terrain_diagnostic(&field, &ADJACENCY_ORIGINS);
    assert!(
        land.saturated_middle_relief_per_million <= 10_000,
        "{land:#?}"
    );
    assert!(land.delta_ge_two_edges > 0, "{land:#?}");
    assert!(
        land.plateau_displacement_delta_ge_two_edges
            .saturating_mul(3)
            <= land.delta_ge_two_edges.saturating_mul(2),
        "{land:#?}"
    );
}

fn semantic_v1_field() -> SemanticTerrainFieldV1 {
    semantic_field_from_programs(semantic_surface_biome_terrain_programs_v1)
}

fn semantic_v2_field() -> SemanticTerrainFieldV1 {
    semantic_field_from_programs(semantic_surface_biome_terrain_programs)
}

fn semantic_field_from_programs(
    programs: fn(
        latticeaxiom_worldgen::TerrainConfigV2,
        CanonicalHash,
    ) -> latticeaxiom_worldgen::WorldgenResult<
        Vec<latticeaxiom_worldgen::SurfaceBiomeTerrainProgramV1>,
    >,
) -> SemanticTerrainFieldV1 {
    let terrain = TerrainPresetV2::Balanced.resolve();
    let programs = programs(terrain, CanonicalHash::digest("terrain-morphology-corpus"))
        .expect("semantic terrain programs compile");
    let policy = programs
        .first()
        .and_then(|program| program.semantic_policy())
        .cloned()
        .expect("semantic terrain program carries one policy");
    SemanticTerrainFieldV1::new(
        WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(0x51_7a)),
        terrain,
        policy,
    )
    .expect("semantic terrain field compiles")
}

fn density_digest(field: &SemanticTerrainFieldV1) -> CanonicalHash {
    let mut bytes = Vec::new();
    for (x, z) in [
        (-1_421_i64, -965_i64),
        (-128, 196),
        (1_103, -622),
        (-791, 1_722),
    ] {
        let surface = field
            .sample(x, z)
            .expect("density corpus surface samples")
            .initial_surface_y_q8()
            .div_euclid(256);
        for y_offset in -8_i64..=8 {
            for protected_water in [false, true] {
                let sample = field
                    .density(
                        x,
                        i64::from(surface).saturating_add(y_offset),
                        z,
                        surface,
                        protected_water,
                    )
                    .expect("density corpus coordinate is valid");
                bytes.extend_from_slice(&sample.final_density_q8().to_be_bytes());
                bytes.extend_from_slice(&sample.terrain_family_volume_q8().to_be_bytes());
                bytes.extend_from_slice(&sample.geologic_volume_q8().to_be_bytes());
                bytes.push(u8::from(sample.protected_water()));
            }
        }
    }
    CanonicalHash::digest(bytes)
}

fn morphology_metrics(field: &SemanticTerrainFieldV1) -> MorphologyMetrics {
    let mut land_columns = 0_u64;
    let mut land_edges = 0_u64;
    let mut delta_ge_two_edges = 0_u64;
    let mut maximum_adjacent_delta = 0_u32;
    let mut exposed_cliff_faces = 0_u64;
    let mut flat_runs_x = Vec::new();
    let mut flat_runs_z = Vec::new();
    let mut minimum_land_height = i32::MAX;
    let mut maximum_land_height = i32::MIN;
    let mut flat_interior_cells = 0_u64;
    let mut flat_metric_columns = 0_u64;
    let mut maximum_flat_component_cells = 0_u32;
    let mut maximum_flat_component_span = 0_u16;

    for (origin_x, origin_z) in ADJACENCY_ORIGINS {
        let samples = sample_adjacency_window(field, origin_x, origin_z);
        for sample in samples.iter().filter(|sample| sample.land) {
            land_columns = land_columns.saturating_add(1);
            minimum_land_height = minimum_land_height.min(sample.height);
            maximum_land_height = maximum_land_height.max(sample.height);
        }
        for z in 0..ADJACENCY_EDGE {
            for x in 0..ADJACENCY_EDGE {
                let here = samples[z * ADJACENCY_EDGE + x];
                if x + 1 < ADJACENCY_EDGE {
                    record_edge(
                        here,
                        samples[z * ADJACENCY_EDGE + x + 1],
                        &mut land_edges,
                        &mut delta_ge_two_edges,
                        &mut maximum_adjacent_delta,
                        &mut exposed_cliff_faces,
                    );
                }
                if z + 1 < ADJACENCY_EDGE {
                    record_edge(
                        here,
                        samples[(z + 1) * ADJACENCY_EDGE + x],
                        &mut land_edges,
                        &mut delta_ge_two_edges,
                        &mut maximum_adjacent_delta,
                        &mut exposed_cliff_faces,
                    );
                }
            }
        }
        for z in 0..ADJACENCY_EDGE {
            record_flat_runs(
                (0..ADJACENCY_EDGE).map(|x| samples[z * ADJACENCY_EDGE + x]),
                &mut flat_runs_x,
            );
        }
        for x in 0..ADJACENCY_EDGE {
            record_flat_runs(
                (0..ADJACENCY_EDGE).map(|z| samples[z * ADJACENCY_EDGE + x]),
                &mut flat_runs_z,
            );
        }
        let flat_patches = flat_patch_metrics(&samples);
        flat_interior_cells = flat_interior_cells.saturating_add(flat_patches.flat_interior_cells);
        flat_metric_columns = flat_metric_columns.saturating_add(flat_patches.flat_metric_columns);
        maximum_flat_component_cells =
            maximum_flat_component_cells.max(flat_patches.maximum_component_cells);
        maximum_flat_component_span =
            maximum_flat_component_span.max(flat_patches.maximum_component_span);
    }

    let delta_ge_two_per_million = ratio_per_million(delta_ge_two_edges, land_edges);
    MorphologyMetrics {
        land_columns,
        land_edges,
        delta_ge_two_edges,
        delta_ge_two_per_million,
        maximum_adjacent_delta,
        walkable_neighbor_per_million: 1_000_000_u64.saturating_sub(delta_ge_two_per_million),
        exposed_cliff_faces,
        maximum_flat_run_x: flat_runs_x.iter().copied().max().unwrap_or_default(),
        maximum_flat_run_z: flat_runs_z.iter().copied().max().unwrap_or_default(),
        p95_flat_run_x: percentile_95(&mut flat_runs_x),
        p95_flat_run_z: percentile_95(&mut flat_runs_z),
        flat_interior_cells,
        flat_metric_columns,
        flat_interior_per_million: ratio_per_million(flat_interior_cells, flat_metric_columns),
        maximum_flat_component_cells,
        maximum_flat_component_span,
        local_relief_voxels: RELIEF_WINDOWS.map(|window| local_relief(field, window)),
        minimum_land_height,
        maximum_land_height,
    }
}

fn sample_adjacency_window(
    field: &SemanticTerrainFieldV1,
    origin_x: i64,
    origin_z: i64,
) -> Vec<ColumnMetricSample> {
    let mut samples = Vec::with_capacity(ADJACENCY_EDGE.saturating_mul(ADJACENCY_EDGE));
    for z in 0..ADJACENCY_EDGE {
        for x in 0..ADJACENCY_EDGE {
            let world_x = origin_x.saturating_add(i64::try_from(x).expect("X fits i64"));
            let world_z = origin_z.saturating_add(i64::try_from(z).expect("Z fits i64"));
            let sample = field
                .sample(world_x, world_z)
                .expect("corpus coordinate is valid");
            samples.push(ColumnMetricSample {
                height: sample.initial_surface_y_q8().div_euclid(256),
                land: sample.continentalness_per_1024() >= 0,
                continentalness_per_1024: sample.continentalness_per_1024(),
                middle_relief_per_1024: sample.middle_relief_per_1024(),
                middle_relief_displacement_q8: sample.middle_relief_displacement_q8(),
                plateau_displacement_q8: sample.plateau_displacement_q8(),
                plateau_interior: sample.plateau_weight_per_1024() == 1_024,
            });
        }
    }
    samples
}

fn terrain_diagnostic(
    field: &SemanticTerrainFieldV1,
    origins: &[(i64, i64)],
) -> ShorelineDiagnostic {
    let mut diagnostic = ShorelineDiagnostic::default();
    let mut shoreline_jumps = Vec::new();
    for &(origin_x, origin_z) in origins {
        let samples = sample_adjacency_window(field, origin_x, origin_z);
        diagnostic.sampled_columns = diagnostic
            .sampled_columns
            .saturating_add(u64::try_from(samples.len()).expect("sample count fits u64"));
        diagnostic.saturated_middle_relief_columns =
            diagnostic.saturated_middle_relief_columns.saturating_add(
                u64::try_from(
                    samples
                        .iter()
                        .filter(|sample| sample.middle_relief_per_1024.unsigned_abs() == 1_024)
                        .count(),
                )
                .expect("saturated sample count fits u64"),
            );
        for z in 0..ADJACENCY_EDGE {
            for x in 0..ADJACENCY_EDGE {
                let here = samples[z * ADJACENCY_EDGE + x];
                if x + 1 < ADJACENCY_EDGE {
                    record_diagnostic_edge(
                        here,
                        samples[z * ADJACENCY_EDGE + x + 1],
                        &mut diagnostic,
                        &mut shoreline_jumps,
                    );
                }
                if z + 1 < ADJACENCY_EDGE {
                    record_diagnostic_edge(
                        here,
                        samples[(z + 1) * ADJACENCY_EDGE + x],
                        &mut diagnostic,
                        &mut shoreline_jumps,
                    );
                }
            }
        }
    }
    shoreline_jumps.sort_unstable();
    diagnostic.p95_shoreline_jump = shoreline_jumps
        .get(
            shoreline_jumps
                .len()
                .saturating_mul(95)
                .div_ceil(100)
                .saturating_sub(1),
        )
        .copied()
        .unwrap_or_default();
    diagnostic.saturated_middle_relief_per_million = ratio_per_million(
        diagnostic.saturated_middle_relief_columns,
        diagnostic.sampled_columns,
    );
    diagnostic
}

fn record_diagnostic_edge(
    left: ColumnMetricSample,
    right: ColumnMetricSample,
    diagnostic: &mut ShorelineDiagnostic,
    shoreline_jumps: &mut Vec<u32>,
) {
    let delta = left.height.abs_diff(right.height);
    if left.land != right.land {
        diagnostic.shoreline_edges = diagnostic.shoreline_edges.saturating_add(1);
        if delta >= 2 {
            diagnostic.shoreline_delta_ge_two_edges =
                diagnostic.shoreline_delta_ge_two_edges.saturating_add(1);
        }
        diagnostic.maximum_shoreline_jump = diagnostic.maximum_shoreline_jump.max(delta);
        diagnostic.shoreline_jump_sum = diagnostic
            .shoreline_jump_sum
            .saturating_add(u64::from(delta));
        shoreline_jumps.push(delta);
        return;
    }
    if !left.land {
        return;
    }
    let histogram_index = usize::try_from(delta.min(12)).expect("bounded delta fits usize");
    diagnostic.delta_histogram[histogram_index] =
        diagnostic.delta_histogram[histogram_index].saturating_add(1);
    if delta >= 2 {
        diagnostic.delta_ge_two_edges = diagnostic.delta_ge_two_edges.saturating_add(1);
    }
    let coastal = left.continentalness_per_1024 < 192 || right.continentalness_per_1024 < 192;
    if coastal {
        diagnostic.coastal_land_edges = diagnostic.coastal_land_edges.saturating_add(1);
        if delta >= 2 {
            diagnostic.coastal_delta_ge_two_edges =
                diagnostic.coastal_delta_ge_two_edges.saturating_add(1);
        }
    } else {
        diagnostic.inland_land_edges = diagnostic.inland_land_edges.saturating_add(1);
        if delta >= 2 {
            diagnostic.inland_delta_ge_two_edges =
                diagnostic.inland_delta_ge_two_edges.saturating_add(1);
        }
    }
    if delta >= 2
        && left
            .middle_relief_displacement_q8
            .abs_diff(right.middle_relief_displacement_q8)
            >= 2 * 256
    {
        diagnostic.middle_relief_displacement_delta_ge_two_edges = diagnostic
            .middle_relief_displacement_delta_ge_two_edges
            .saturating_add(1);
    }
    if delta >= 2
        && left
            .plateau_displacement_q8
            .abs_diff(right.plateau_displacement_q8)
            >= 2 * 256
    {
        diagnostic.plateau_displacement_delta_ge_two_edges = diagnostic
            .plateau_displacement_delta_ge_two_edges
            .saturating_add(1);
    }
}

fn record_edge(
    left: ColumnMetricSample,
    right: ColumnMetricSample,
    land_edges: &mut u64,
    delta_ge_two_edges: &mut u64,
    maximum_adjacent_delta: &mut u32,
    exposed_cliff_faces: &mut u64,
) {
    if !left.land || !right.land {
        return;
    }
    *land_edges = land_edges.saturating_add(1);
    let delta = left.height.abs_diff(right.height);
    *maximum_adjacent_delta = (*maximum_adjacent_delta).max(delta);
    if delta >= 2 {
        *delta_ge_two_edges = delta_ge_two_edges.saturating_add(1);
        *exposed_cliff_faces = exposed_cliff_faces.saturating_add(u64::from(delta - 1));
    }
}

fn record_flat_runs(samples: impl IntoIterator<Item = ColumnMetricSample>, runs: &mut Vec<u16>) {
    let mut previous = None;
    let mut run = 0_u16;
    for sample in samples {
        if sample.land && previous == Some(sample.height) {
            run = run.saturating_add(1);
        } else {
            if run > 0 {
                runs.push(run);
            }
            run = u16::from(sample.land);
        }
        previous = sample.land.then_some(sample.height);
    }
    if run > 0 {
        runs.push(run);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FlatPatchMetrics {
    flat_interior_cells: u64,
    flat_metric_columns: u64,
    maximum_component_cells: u32,
    maximum_component_span: u16,
}

fn flat_patch_metrics(samples: &[ColumnMetricSample]) -> FlatPatchMetrics {
    let mut flat = vec![false; ADJACENCY_EDGE.saturating_mul(ADJACENCY_EDGE)];
    for z in 1..ADJACENCY_EDGE - 1 {
        for x in 1..ADJACENCY_EDGE - 1 {
            let index = z * ADJACENCY_EDGE + x;
            let center = samples[index];
            flat[index] = center.land
                && !center.plateau_interior
                && [
                    index - 1,
                    index + 1,
                    index - ADJACENCY_EDGE,
                    index + ADJACENCY_EDGE,
                ]
                .into_iter()
                .all(|neighbor| {
                    samples[neighbor].land
                        && !samples[neighbor].plateau_interior
                        && samples[neighbor].height == center.height
                });
        }
    }

    let mut metrics = FlatPatchMetrics {
        flat_interior_cells: u64::try_from(flat.iter().filter(|&&value| value).count())
            .expect("flat-cell count fits u64"),
        flat_metric_columns: u64::try_from(
            samples
                .iter()
                .filter(|sample| sample.land && !sample.plateau_interior)
                .count(),
        )
        .expect("flat-metric column count fits u64"),
        ..FlatPatchMetrics::default()
    };
    let mut visited = vec![false; flat.len()];
    let mut stack = Vec::new();
    for origin in 0..flat.len() {
        if !flat[origin] || visited[origin] {
            continue;
        }
        visited[origin] = true;
        stack.push(origin);
        let mut cells = 0_u32;
        let mut minimum_x = ADJACENCY_EDGE;
        let mut maximum_x = 0_usize;
        let mut minimum_z = ADJACENCY_EDGE;
        let mut maximum_z = 0_usize;
        while let Some(index) = stack.pop() {
            cells = cells.saturating_add(1);
            let x = index % ADJACENCY_EDGE;
            let z = index / ADJACENCY_EDGE;
            minimum_x = minimum_x.min(x);
            maximum_x = maximum_x.max(x);
            minimum_z = minimum_z.min(z);
            maximum_z = maximum_z.max(z);
            for neighbor in [
                index - 1,
                index + 1,
                index - ADJACENCY_EDGE,
                index + ADJACENCY_EDGE,
            ] {
                if flat[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
        let span = maximum_x
            .saturating_sub(minimum_x)
            .saturating_add(1)
            .max(maximum_z.saturating_sub(minimum_z).saturating_add(1));
        metrics.maximum_component_cells = metrics.maximum_component_cells.max(cells);
        metrics.maximum_component_span = metrics
            .maximum_component_span
            .max(u16::try_from(span).expect("component span fits u16"));
    }
    metrics
}

fn percentile_95(values: &mut [u16]) -> u16 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let index = values
        .len()
        .saturating_mul(95)
        .div_ceil(100)
        .saturating_sub(1);
    values[index]
}

fn local_relief(field: &SemanticTerrainFieldV1, window: i64) -> u32 {
    let spacing = window.div_euclid(64).max(1);
    let mut relief = Vec::new();
    for (center_x, center_z) in RELIEF_CENTERS {
        let mut minimum = i32::MAX;
        let mut maximum = i32::MIN;
        let mut land_samples = 0_u32;
        for sample_z in 0_i64..=64 {
            for sample_x in 0_i64..=64 {
                let x = center_x
                    .saturating_sub(window.div_euclid(2))
                    .saturating_add(sample_x.saturating_mul(spacing));
                let z = center_z
                    .saturating_sub(window.div_euclid(2))
                    .saturating_add(sample_z.saturating_mul(spacing));
                let sample = field.sample(x, z).expect("relief coordinate is valid");
                if sample.continentalness_per_1024() < 0 {
                    continue;
                }
                let height = sample.initial_surface_y_q8().div_euclid(256);
                minimum = minimum.min(height);
                maximum = maximum.max(height);
                land_samples = land_samples.saturating_add(1);
            }
        }
        if land_samples >= 1_024 {
            relief.push(minimum.abs_diff(maximum));
        }
    }
    relief.sort_unstable();
    relief
        .get(relief.len().saturating_sub(1).div_euclid(2))
        .copied()
        .unwrap_or_default()
}

fn ratio_per_million(numerator: u64, denominator: u64) -> u64 {
    numerator
        .saturating_mul(1_000_000)
        .div_euclid(denominator.max(1))
}
