//! Locked terrain texture-layer compilation contract.

use std::collections::BTreeSet;

use latticeaxiom_core::StableId;
use latticeaxiom_render_contracts::{
    AuthoredTerrainLayerDeclV1, CompiledTerrainLayerRowV1, LayerResolutionV1,
    LockedContentPresentationV1, PresentationPresenceV1, TERRAIN_LAYER_FALLBACK_ASSET_V1,
    TERRAIN_LAYER_TABLE_SCHEMA_MAJOR, TerrainFaceMapV1, TerrainFaceV1, TerrainLayerCompileError,
    TerrainLayerCompileInputV1, TerrainLayerDiagnosticCodeV1, TerrainLayerLimitsV1,
    TerrainMaterialPolicyV1, VoxelAddressModeV1, VoxelColorSpaceV1, VoxelFilterModeV1,
    VoxelMipmapPolicyV1, VoxelSamplerPolicyV1, compile_terrain_layer_table,
};

fn id(value: &str) -> StableId {
    match value.parse() {
        Ok(value) => value,
        Err(error) => panic!("test StableId must be valid: {error}"),
    }
}

fn content(value: &str, binding: Option<&str>) -> LockedContentPresentationV1 {
    LockedContentPresentationV1 {
        content: id(value),
        binding: binding.map(id),
    }
}

fn uniform(
    value: &str,
    policy: TerrainMaterialPolicyV1,
    layer: &str,
) -> AuthoredTerrainLayerDeclV1 {
    AuthoredTerrainLayerDeclV1 {
        content: id(value),
        policy,
        faces: TerrainFaceMapV1::Uniform { layer: id(layer) },
    }
}

fn cube_column(value: &str) -> AuthoredTerrainLayerDeclV1 {
    AuthoredTerrainLayerDeclV1 {
        content: id(value),
        policy: TerrainMaterialPolicyV1::Opaque,
        faces: TerrainFaceMapV1::CubeColumn {
            top: id("demo:asset/grass-top"),
            side: id("demo:asset/grass-side"),
            bottom: id("demo:asset/grass-bottom"),
        },
    }
}

fn available(layers: &[&str]) -> BTreeSet<StableId> {
    layers.iter().copied().map(id).collect()
}

fn compile(
    presence: PresentationPresenceV1,
    content_rows: Vec<LockedContentPresentationV1>,
    declarations: Vec<AuthoredTerrainLayerDeclV1>,
    available_layers: BTreeSet<StableId>,
) -> latticeaxiom_render_contracts::CompiledTerrainLayerTableV1 {
    compile_terrain_layer_table(
        TerrainLayerCompileInputV1 {
            schema_major: TERRAIN_LAYER_TABLE_SCHEMA_MAJOR,
            presence,
            content: content_rows,
            declarations,
            available_layers,
            sampler: VoxelSamplerPolicyV1::TERRAIN_V1,
            claims_authoritative: false,
        },
        TerrainLayerLimitsV1::default(),
    )
    .unwrap_or_else(|error| panic!("layer table must compile: {error}"))
}

fn deterministic_shuffle<T>(values: &mut [T], state: &mut u64) {
    for last in (1..values.len()).rev() {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let bound = match u64::try_from(last + 1) {
            Ok(value) => value,
            Err(error) => panic!("test permutation bound must fit u64: {error}"),
        };
        let selected = match usize::try_from(*state % bound) {
            Ok(value) => value,
            Err(error) => panic!("test permutation index must fit usize: {error}"),
        };
        values.swap(last, selected);
    }
}

#[test]
fn policies_use_stable_mesh_group_order() {
    assert_eq!(
        TerrainMaterialPolicyV1::ALL.map(TerrainMaterialPolicyV1::index),
        [0, 1, 2, 3]
    );
    assert_eq!(
        TerrainFaceV1::ALL.map(TerrainFaceV1::layer_variant),
        [0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        VoxelSamplerPolicyV1::TERRAIN_V1,
        VoxelSamplerPolicyV1 {
            filter: VoxelFilterModeV1::Nearest,
            address: VoxelAddressModeV1::ClampToEdge,
            mipmaps: VoxelMipmapPolicyV1::None,
            color_space: VoxelColorSpaceV1::Srgb,
        }
    );
}

#[test]
fn authored_rows_preserve_face_specific_layers_and_policies() {
    let table = compile(
        PresentationPresenceV1::Present,
        vec![
            content("demo:block/glass", Some("demo:asset/glass")),
            content("demo:block/grass", Some("demo:asset/grass")),
            content("demo:block/leaves", Some("demo:asset/leaves")),
            content("demo:fluid/lava", Some("demo:asset/lava")),
        ],
        vec![
            uniform(
                "demo:block/glass",
                TerrainMaterialPolicyV1::Translucent,
                "demo:asset/glass",
            ),
            cube_column("demo:block/grass"),
            uniform(
                "demo:block/leaves",
                TerrainMaterialPolicyV1::Cutout,
                "demo:asset/leaves",
            ),
            uniform(
                "demo:fluid/lava",
                TerrainMaterialPolicyV1::Emissive,
                "demo:asset/lava",
            ),
        ],
        available(&[
            "demo:asset/glass",
            "demo:asset/grass-top",
            "demo:asset/grass-side",
            "demo:asset/grass-bottom",
            "demo:asset/leaves",
            "demo:asset/lava",
        ]),
    );

    assert!(table.diagnostics().is_empty());
    let grass = table
        .row(&id("demo:block/grass"))
        .unwrap_or_else(|| panic!("grass row missing"));
    assert_eq!(grass.policy(), TerrainMaterialPolicyV1::Opaque);
    assert_eq!(grass.resolution(), LayerResolutionV1::Authored);
    assert_eq!(
        grass.faces().layer(TerrainFaceV1::Up).as_str(),
        "demo:asset/grass-top"
    );
    assert_eq!(
        grass.faces().layer(TerrainFaceV1::East).as_str(),
        "demo:asset/grass-side"
    );
    assert_eq!(
        grass.faces().layer(TerrainFaceV1::Down).as_str(),
        "demo:asset/grass-bottom"
    );
    assert_ne!(
        grass.faces().variant(TerrainFaceV1::Up),
        grass.faces().variant(TerrainFaceV1::East)
    );
    assert_eq!(
        table
            .row(&id("demo:block/leaves"))
            .expect("leaves")
            .policy(),
        TerrainMaterialPolicyV1::Cutout
    );
    assert_eq!(
        table.row(&id("demo:block/glass")).expect("glass").policy(),
        TerrainMaterialPolicyV1::Translucent
    );
    assert_eq!(
        table.row(&id("demo:fluid/lava")).expect("lava").policy(),
        TerrainMaterialPolicyV1::Emissive
    );
}

#[test]
fn discovery_order_does_not_change_rows_or_fingerprint() {
    let content_rows = vec![
        content("demo:block/b", None),
        content("demo:block/a", None),
        content("demo:block/c", None),
    ];
    let declarations = vec![
        uniform(
            "demo:block/c",
            TerrainMaterialPolicyV1::Opaque,
            "demo:asset/c",
        ),
        uniform(
            "demo:block/a",
            TerrainMaterialPolicyV1::Cutout,
            "demo:asset/a",
        ),
        uniform(
            "demo:block/b",
            TerrainMaterialPolicyV1::Translucent,
            "demo:asset/b",
        ),
    ];
    let available_layers = available(&["demo:asset/a", "demo:asset/b", "demo:asset/c"]);
    let forward = compile(
        PresentationPresenceV1::Present,
        content_rows.clone(),
        declarations.clone(),
        available_layers.clone(),
    );

    let mut reversed_content = content_rows;
    reversed_content.reverse();
    let mut reversed_declarations = declarations;
    reversed_declarations.reverse();
    let mut shuffled_available = available_layers.into_iter().collect::<Vec<_>>();
    let mut state = 7_u64;
    deterministic_shuffle(&mut shuffled_available, &mut state);
    let reversed = compile(
        PresentationPresenceV1::Present,
        reversed_content,
        reversed_declarations,
        shuffled_available.into_iter().collect(),
    );

    assert_eq!(forward.fingerprint(), reversed.fingerprint());
    let ids = forward
        .rows()
        .iter()
        .map(|row| row.content().as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["demo:block/a", "demo:block/b", "demo:block/c"]);
    assert_eq!(
        forward
            .rows()
            .iter()
            .map(CompiledTerrainLayerRowV1::table_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}

#[test]
fn missing_and_invalid_layers_use_the_platform_fallback() {
    let table = compile(
        PresentationPresenceV1::Present,
        vec![
            content("demo:block/missing", None),
            content("demo:block/invalid", None),
        ],
        vec![uniform(
            "demo:block/invalid",
            TerrainMaterialPolicyV1::Opaque,
            "demo:asset/absent",
        )],
        available(&["demo:asset/present"]),
    );

    let fallback = id(TERRAIN_LAYER_FALLBACK_ASSET_V1);
    for row in table.rows() {
        assert_eq!(row.resolution(), LayerResolutionV1::Fallback);
        assert_eq!(row.policy(), TerrainMaterialPolicyV1::Opaque);
        assert_eq!(row.faces().layer(TerrainFaceV1::East), &fallback);
        assert_eq!(row.sampler(), VoxelSamplerPolicyV1::TERRAIN_V1);
    }
    let codes = table
        .diagnostics()
        .iter()
        .map(latticeaxiom_render_contracts::TerrainLayerDiagnosticV1::code)
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        [
            TerrainLayerDiagnosticCodeV1::MissingDeclaration,
            TerrainLayerDiagnosticCodeV1::UnknownLayerReference
        ]
    );
}

#[test]
fn headless_omission_compiles_fallback_rows_without_assets() {
    let table = compile(
        PresentationPresenceV1::Omitted,
        vec![
            content("demo:block/stone", Some("demo:asset/stone")),
            content("demo:fluid/water", Some("demo:asset/water")),
        ],
        vec![uniform(
            "demo:block/stone",
            TerrainMaterialPolicyV1::Opaque,
            "demo:asset/stone",
        )],
        BTreeSet::new(),
    );

    assert_eq!(table.presence(), PresentationPresenceV1::Omitted);
    assert_eq!(table.rows().len(), 2);
    assert_eq!(
        table.diagnostics()[0].code(),
        TerrainLayerDiagnosticCodeV1::HeadlessOmission
    );
    for row in table.rows() {
        assert_eq!(row.resolution(), LayerResolutionV1::Omitted);
        assert_eq!(
            row.faces().layer(TerrainFaceV1::Up).as_str(),
            TERRAIN_LAYER_FALLBACK_ASSET_V1
        );
    }
}

#[test]
fn omitted_and_present_fingerprints_differ() {
    let content_rows = vec![content("demo:block/stone", Some("demo:asset/stone"))];
    let declarations = vec![uniform(
        "demo:block/stone",
        TerrainMaterialPolicyV1::Opaque,
        "demo:asset/stone",
    )];
    let present = compile(
        PresentationPresenceV1::Present,
        content_rows.clone(),
        declarations.clone(),
        available(&["demo:asset/stone"]),
    );
    let omitted = compile(
        PresentationPresenceV1::Omitted,
        content_rows,
        declarations,
        BTreeSet::new(),
    );
    assert_ne!(present.fingerprint(), omitted.fingerprint());
}

#[test]
fn extra_declarations_and_authoritative_claims_are_rejected_or_diagnosed() {
    let table = compile(
        PresentationPresenceV1::Present,
        vec![content("demo:block/stone", None)],
        vec![
            uniform(
                "demo:block/stone",
                TerrainMaterialPolicyV1::Opaque,
                "demo:asset/stone",
            ),
            uniform(
                "demo:block/unknown",
                TerrainMaterialPolicyV1::Opaque,
                "demo:asset/stone",
            ),
        ],
        available(&["demo:asset/stone"]),
    );
    assert_eq!(
        table.diagnostics()[0].code(),
        TerrainLayerDiagnosticCodeV1::UnknownContent
    );
    assert_eq!(table.rows().len(), 1);

    let error = compile_terrain_layer_table(
        TerrainLayerCompileInputV1 {
            schema_major: TERRAIN_LAYER_TABLE_SCHEMA_MAJOR,
            presence: PresentationPresenceV1::Present,
            content: vec![content("demo:block/stone", None)],
            declarations: Vec::new(),
            available_layers: BTreeSet::new(),
            sampler: VoxelSamplerPolicyV1::TERRAIN_V1,
            claims_authoritative: true,
        },
        TerrainLayerLimitsV1::default(),
    )
    .expect_err("authoritative claim must fail");
    assert!(matches!(
        error,
        TerrainLayerCompileError::AuthoritativeClaim
    ));
}
