//! Terrenia locked terrain-layer compilation.

use std::collections::BTreeSet;

use latticeaxiom_content::{
    ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1, ContentPresentationKindV1,
};
use latticeaxiom_render_contracts::{
    AuthoredTerrainLayerDocumentV1, LayerResolutionV1, LockedContentPresentationV1,
    PresentationPresenceV1, TERRAIN_LAYER_TABLE_SCHEMA_MAJOR, TerrainFaceV1,
    TerrainLayerCompileInputV1, TerrainLayerDiagnosticCodeV1, TerrainLayerLimitsV1,
    TerrainMaterialPolicyV1, VoxelSamplerPolicyV1, compile_terrain_layer_table,
};
use latticeaxiom_voxel_mesh::MeshGroup;
use serde_json::{Value, json};

const CATALOG_JSON: &str =
    include_str!("../../../packages/terrenia/blocks/data/authored-catalog-v1.json");
const LAYERS_JSON: &str =
    include_str!("../../../packages/terrenia/presentation/data/authored-layers-v1.json");
const ASSETS_JSON: &str =
    include_str!("../../../packages/terrenia/presentation/data/authored-assets-v1.json");

fn catalog() -> ContentCatalogV1 {
    let authored: Value = serde_json::from_str(CATALOG_JSON)
        .unwrap_or_else(|error| panic!("Terrenia authored catalog is invalid: {error}"));
    let blocks = authored["blocks"]
        .as_array()
        .unwrap_or_else(|| panic!("blocks is not an array"))
        .iter()
        .map(|row| row["definition"].clone())
        .collect::<Vec<_>>();
    let fluids = authored["fluids"]
        .as_array()
        .unwrap_or_else(|| panic!("fluids is not an array"))
        .iter()
        .map(|row| row["definition"].clone())
        .collect::<Vec<_>>();
    let input = serde_json::from_value::<ContentCatalogInputV1>(json!({
        "schema_major": 1,
        "blocks": blocks,
        "fluids": fluids,
        "biomes": [],
        "material_role_bindings": authored["material_role_bindings"].clone()
    }))
    .unwrap_or_else(|error| panic!("Terrenia compiler-shaped definitions are invalid: {error}"));
    ContentCatalogV1::compile(input, ContentCatalogLimitsV1::default())
        .unwrap_or_else(|error| panic!("Terrenia authored catalog did not compile: {error}"))
}

fn document() -> AuthoredTerrainLayerDocumentV1 {
    serde_json::from_str(LAYERS_JSON)
        .unwrap_or_else(|error| panic!("Terrenia authored layers are invalid: {error}"))
}

fn available_layers() -> BTreeSet<latticeaxiom_core::StableId> {
    let assets: Value = serde_json::from_str(ASSETS_JSON)
        .unwrap_or_else(|error| panic!("Terrenia presentation assets are invalid: {error}"));
    assets["assets"]
        .as_array()
        .unwrap_or_else(|| panic!("presentation assets is not an array"))
        .iter()
        .filter_map(|row| {
            let kind = row["asset_kind"]
                .as_str()
                .unwrap_or_else(|| panic!("presentation asset kind is not text"));
            if matches!(
                kind,
                "block-material-set" | "fluid-material-set" | "texture-layer"
            ) {
                let asset_id = row["id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("presentation asset ID is not text"));
                Some(asset_id.parse().unwrap_or_else(|error| {
                    panic!("presentation asset ID `{asset_id}` is invalid: {error}")
                }))
            } else {
                None
            }
        })
        .collect()
}

fn compile(
    catalog: &ContentCatalogV1,
    document: &AuthoredTerrainLayerDocumentV1,
    presence: PresentationPresenceV1,
) -> latticeaxiom_render_contracts::CompiledTerrainLayerTableV1 {
    let content = catalog
        .presentation_bindings()
        .into_iter()
        .map(|row| LockedContentPresentationV1 {
            content: row.content().clone(),
            binding: row.binding().cloned(),
        })
        .collect();
    compile_terrain_layer_table(
        TerrainLayerCompileInputV1 {
            schema_major: TERRAIN_LAYER_TABLE_SCHEMA_MAJOR,
            presence,
            content,
            declarations: document.layers.clone(),
            available_layers: available_layers(),
            sampler: document.sampler,
            claims_authoritative: document.authoritative,
        },
        TerrainLayerLimitsV1::default(),
    )
    .unwrap_or_else(|error| panic!("Terrenia layer table must compile: {error}"))
}

#[test]
fn presentation_bindings_are_excluded_from_the_authoritative_hash() {
    let compiled = catalog();
    let bindings = compiled.presentation_bindings();
    assert_eq!(bindings.len(), 74);
    assert!(
        bindings
            .iter()
            .any(|row| row.kind() == ContentPresentationKindV1::Fluid)
    );
    assert!(bindings.iter().all(|row| row.binding().is_some()));

    let authoritative = compiled
        .canonical_authoritative_hash()
        .unwrap_or_else(|error| panic!("authoritative hash failed: {error}"));
    let presentation = compiled
        .presentation_binding_hash()
        .unwrap_or_else(|error| panic!("presentation hash failed: {error}"));
    assert_ne!(authoritative, presentation);

    let mut rebound = serde_json::from_str::<Value>(CATALOG_JSON)
        .unwrap_or_else(|error| panic!("catalog JSON: {error}"));
    rebound["blocks"][0]["definition"]["presentation_binding"] =
        json!("terrenia:asset/block-stone");
    let blocks = rebound["blocks"]
        .as_array()
        .expect("blocks")
        .iter()
        .map(|row| row["definition"].clone())
        .collect::<Vec<_>>();
    let fluids = rebound["fluids"]
        .as_array()
        .expect("fluids")
        .iter()
        .map(|row| row["definition"].clone())
        .collect::<Vec<_>>();
    let input = serde_json::from_value::<ContentCatalogInputV1>(json!({
        "schema_major": 1,
        "blocks": blocks,
        "fluids": fluids,
        "biomes": [],
        "material_role_bindings": rebound["material_role_bindings"].clone()
    }))
    .unwrap_or_else(|error| panic!("rebound catalog invalid: {error}"));
    let rebound = ContentCatalogV1::compile(input, ContentCatalogLimitsV1::default())
        .unwrap_or_else(|error| panic!("rebound catalog did not compile: {error}"));
    assert_eq!(
        rebound
            .canonical_authoritative_hash()
            .unwrap_or_else(|error| panic!("rebound authoritative hash: {error}")),
        authoritative
    );
    assert_ne!(
        rebound
            .presentation_binding_hash()
            .unwrap_or_else(|error| panic!("rebound presentation hash: {error}")),
        presentation
    );
}

#[test]
fn locked_terrenia_layers_compile_with_face_policies_and_nearest_sampler() {
    let compiled_catalog = catalog();
    let document = document();
    assert!(!document.authoritative);
    assert_eq!(document.sampler, VoxelSamplerPolicyV1::TERRAIN_V1);
    assert_eq!(document.owner_package.as_str(), "@terrenia/presentation");

    let table = compile(
        &compiled_catalog,
        &document,
        PresentationPresenceV1::Present,
    );
    assert!(
        table
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.code() != TerrainLayerDiagnosticCodeV1::HeadlessOmission)
    );
    assert_eq!(table.rows().len(), 74);
    assert!(
        table
            .rows()
            .iter()
            .all(|row| row.resolution() == LayerResolutionV1::Authored)
    );
    assert!(table.diagnostics().is_empty());

    let grass = table
        .row(&"terrenia:block/grass".parse().expect("grass id"))
        .unwrap_or_else(|| panic!("grass layer missing"));
    assert_eq!(grass.policy(), TerrainMaterialPolicyV1::Opaque);
    assert_eq!(
        grass.faces().layer(TerrainFaceV1::Up).as_str(),
        "terrenia:asset/layer-grass-top"
    );
    assert_eq!(
        grass.faces().layer(TerrainFaceV1::East).as_str(),
        "terrenia:asset/layer-grass-side"
    );
    assert_ne!(
        grass.faces().variant(TerrainFaceV1::Up),
        grass.faces().variant(TerrainFaceV1::East)
    );

    let leaves = table
        .row(&"terrenia:block/oak-leaves".parse().expect("leaves id"))
        .expect("leaves");
    let water = table
        .row(&"terrenia:fluid/water".parse().expect("water id"))
        .expect("water");
    let lava = table
        .row(&"terrenia:fluid/lava".parse().expect("lava id"))
        .expect("lava");
    let torch = table
        .row(&"terrenia:block/torch".parse().expect("torch id"))
        .expect("torch");
    assert_eq!(leaves.policy(), TerrainMaterialPolicyV1::Cutout);
    assert_eq!(water.policy(), TerrainMaterialPolicyV1::Translucent);
    assert_eq!(lava.policy(), TerrainMaterialPolicyV1::Emissive);
    assert_eq!(torch.policy(), TerrainMaterialPolicyV1::Emissive);
    assert_eq!(leaves.policy().index(), MeshGroup::Cutout.index());
    assert_eq!(water.policy().index(), MeshGroup::Translucent.index());
    assert_eq!(lava.policy().index(), MeshGroup::Emissive.index());
    assert_eq!(grass.sampler(), VoxelSamplerPolicyV1::TERRAIN_V1);
}

#[test]
fn headless_omission_keeps_locked_content_and_does_not_change_world_hash() {
    let compiled_catalog = catalog();
    let document = document();
    let present = compile(
        &compiled_catalog,
        &document,
        PresentationPresenceV1::Present,
    );
    let omitted = compile(
        &compiled_catalog,
        &document,
        PresentationPresenceV1::Omitted,
    );
    assert_eq!(present.rows().len(), omitted.rows().len());
    assert_eq!(
        omitted.diagnostics()[0].code(),
        TerrainLayerDiagnosticCodeV1::HeadlessOmission
    );
    assert!(
        omitted
            .rows()
            .iter()
            .all(|row| row.resolution() == LayerResolutionV1::Omitted)
    );
    assert_ne!(present.fingerprint(), omitted.fingerprint());
    let world_hash = compiled_catalog
        .canonical_authoritative_hash()
        .unwrap_or_else(|error| panic!("world hash: {error}"));
    assert_ne!(world_hash, present.fingerprint());
    assert_ne!(world_hash, omitted.fingerprint());
}

#[test]
fn discovery_order_does_not_change_the_terrenia_layer_table() {
    let compiled_catalog = catalog();
    let forward_document = document();
    let mut reversed_document = forward_document.clone();
    reversed_document.layers.reverse();
    let reversed = compile(
        &compiled_catalog,
        &reversed_document,
        PresentationPresenceV1::Present,
    );
    let forward = compile(
        &compiled_catalog,
        &forward_document,
        PresentationPresenceV1::Present,
    );
    assert_eq!(forward.fingerprint(), reversed.fingerprint());
}
