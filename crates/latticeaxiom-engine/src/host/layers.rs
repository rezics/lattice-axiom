//! Locked terrain-layer compilation for production presentation.
//!
//! Layer tables are presentation-only. Compilation cannot change an
//! authoritative catalog hash, snapshot bytes, or world identity.

use std::collections::BTreeSet;

use latticeaxiom_content::{
    CompiledFluidPaletteV1, ContentCatalogV1, ContentPresentationBindingV1, SolidOccupancyKindV1,
};
use latticeaxiom_core::StableId;
use latticeaxiom_gameplay::BlockId;
use latticeaxiom_render_contracts::{
    AuthoredTerrainLayerDocumentV1, CompiledTerrainLayerTableV1, LockedContentPresentationV1,
    PresentationPresenceV1, TERRAIN_LAYER_TABLE_SCHEMA_MAJOR, TerrainFaceV1,
    TerrainLayerCompileInputV1, TerrainLayerLimitsV1, TerrainMaterialPolicyV1,
    VoxelSamplerPolicyV1, compile_terrain_layer_table,
};
use latticeaxiom_voxel_mesh::{Face, FaceDescriptor, FaceOcclusion, LayerMergeKey, MeshGroup};
use serde::Deserialize;

use super::{
    ProductionHostError,
    catalog::required_data_text,
    display::{
        AUTHORED_PRESENTATION_ASSETS_PATH, AUTHORED_PRESENTATION_LAYERS_PATH,
        presentation_data_root,
    },
};
use crate::LockVerifiedComposeImages;

/// Compact face presentation copied onto each halo sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HostFaceStyle {
    group: MeshGroup,
    layer_index: u16,
    variants: [u8; 6],
    occlusion: FaceOcclusion,
}

impl HostFaceStyle {
    /// Face descriptor used by the greedy mesher.
    #[must_use]
    pub(super) fn descriptor(self, face: Face) -> FaceDescriptor<LayerMergeKey> {
        FaceDescriptor::new(
            self.group,
            LayerMergeKey::new(self.layer_index, self.variants[face.index()]),
            self.occlusion,
        )
    }
}

/// Palette-index lookup of compiled layer-table rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HostPresentationIndex {
    table: CompiledTerrainLayerTableV1,
    solids: Vec<Option<HostFaceStyle>>,
    solid_collision: Vec<bool>,
    fluids: Vec<Option<HostFaceStyle>>,
}

impl HostPresentationIndex {
    /// Compiles the locked layer table and indexes it by host palettes.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when authored JSON is invalid or the
    /// layer compiler rejects the locked graph.
    pub(super) fn compile(
        images: &LockVerifiedComposeImages,
        catalog: &ContentCatalogV1,
        solid_palette: &[BlockId],
        fluid_palette: &CompiledFluidPaletteV1,
    ) -> Result<Self, ProductionHostError> {
        let table = compile_locked_layer_table(images, catalog)?;
        Self::from_table(table, catalog, solid_palette, fluid_palette)
    }

    fn from_table(
        table: CompiledTerrainLayerTableV1,
        catalog: &ContentCatalogV1,
        solid_palette: &[BlockId],
        fluid_palette: &CompiledFluidPaletteV1,
    ) -> Result<Self, ProductionHostError> {
        let mut solids = Vec::with_capacity(solid_palette.len());
        let mut solid_collision = Vec::with_capacity(solid_palette.len());
        for block in solid_palette {
            let id = block.as_str().parse::<StableId>()?;
            let definition = catalog.block(&id).ok_or_else(|| {
                ProductionHostError::MissingCatalogDefinition {
                    kind: "block",
                    id: id.to_string(),
                }
            })?;
            let state = definition
                .semantics_for(&definition.definition().default_state)
                .ok_or_else(|| ProductionHostError::MissingCatalogDefinition {
                    kind: "block-state",
                    id: id.to_string(),
                })?;
            let occupied = matches!(
                SolidOccupancyKindV1::classify(&state.solid_occupancy)?,
                SolidOccupancyKindV1::Full | SolidOccupancyKindV1::Partial
            );
            solids.push(style_for(&table, &id));
            solid_collision.push(occupied);
        }
        let fluids = fluid_palette
            .entries()
            .iter()
            .map(|entry| entry.fluid().and_then(|id| style_for(&table, id)))
            .collect();
        Ok(Self {
            table,
            solids,
            solid_collision,
            fluids,
        })
    }

    /// Compiled layer table retained for GPU material routing.
    #[must_use]
    #[cfg_attr(not(feature = "client"), allow(dead_code))]
    pub(super) const fn table(&self) -> &CompiledTerrainLayerTableV1 {
        &self.table
    }

    /// Solid-layer style for a host palette index.
    #[must_use]
    pub(super) fn solid(&self, palette_index: u16) -> Option<HostFaceStyle> {
        self.solids
            .get(usize::from(palette_index))
            .copied()
            .flatten()
    }

    /// Returns the authoritative solid occupancy bit for a palette entry.
    #[must_use]
    pub(super) fn solid_collision(&self, palette_index: u16) -> bool {
        self.solid_collision
            .get(usize::from(palette_index))
            .copied()
            .unwrap_or(false)
    }

    /// Fluid-layer style for a host fluid palette index.
    #[must_use]
    pub(super) fn fluid(&self, palette_index: u16) -> Option<HostFaceStyle> {
        self.fluids
            .get(usize::from(palette_index))
            .copied()
            .flatten()
    }
}

fn style_for(table: &CompiledTerrainLayerTableV1, content: &StableId) -> Option<HostFaceStyle> {
    let row = table.row(content)?;
    let mut variants = [0_u8; 6];
    for face in TerrainFaceV1::ALL {
        variants[face.index()] = row.faces().variant(face);
    }
    Some(HostFaceStyle {
        group: mesh_group(row.policy()),
        layer_index: row.table_index(),
        variants,
        occlusion: face_occlusion(row.policy()),
    })
}

const fn mesh_group(policy: TerrainMaterialPolicyV1) -> MeshGroup {
    MeshGroup::ALL[policy.index()]
}

const fn face_occlusion(policy: TerrainMaterialPolicyV1) -> FaceOcclusion {
    match policy {
        TerrainMaterialPolicyV1::Opaque => FaceOcclusion::Full,
        TerrainMaterialPolicyV1::Cutout
        | TerrainMaterialPolicyV1::Translucent
        | TerrainMaterialPolicyV1::Emissive => FaceOcclusion::Matching,
    }
}

fn compile_locked_layer_table(
    images: &LockVerifiedComposeImages,
    catalog: &ContentCatalogV1,
) -> Result<CompiledTerrainLayerTableV1, ProductionHostError> {
    let presentation = presentation_data_root(images)?;
    let (presence, declarations, available_layers, sampler, claims_authoritative) =
        match presentation.as_deref() {
            Some(data) => {
                let document: AuthoredTerrainLayerDocumentV1 = serde_json::from_str(
                    required_data_text(data, AUTHORED_PRESENTATION_LAYERS_PATH)?,
                )
                .map_err(|source| ProductionHostError::InvalidAuthoredCatalog {
                    name: "authored-layers",
                    source,
                })?;
                let available_layers =
                    available_layers(required_data_text(data, AUTHORED_PRESENTATION_ASSETS_PATH)?)?;
                (
                    PresentationPresenceV1::Present,
                    document.layers,
                    available_layers,
                    document.sampler,
                    document.authoritative,
                )
            }
            None => (
                PresentationPresenceV1::Omitted,
                Vec::new(),
                BTreeSet::new(),
                VoxelSamplerPolicyV1::TERRAIN_V1,
                false,
            ),
        };
    compile_terrain_layer_table(
        TerrainLayerCompileInputV1 {
            schema_major: TERRAIN_LAYER_TABLE_SCHEMA_MAJOR,
            presence,
            content: locked_content(catalog),
            declarations,
            available_layers,
            sampler,
            claims_authoritative,
        },
        TerrainLayerLimitsV1::default(),
    )
    .map_err(|error| ProductionHostError::TerrainLayerTable {
        reason: error.to_string(),
    })
}

fn locked_content(catalog: &ContentCatalogV1) -> Vec<LockedContentPresentationV1> {
    catalog
        .presentation_bindings()
        .into_iter()
        .map(
            |row: ContentPresentationBindingV1| LockedContentPresentationV1 {
                content: row.content().clone(),
                binding: row.binding().cloned(),
            },
        )
        .collect()
}

fn available_layers(source: &str) -> Result<BTreeSet<StableId>, ProductionHostError> {
    let file: AuthoredAssetsFile = serde_json::from_str(source).map_err(|source| {
        ProductionHostError::InvalidAuthoredCatalog {
            name: "presentation-assets",
            source,
        }
    })?;
    let mut layers = BTreeSet::new();
    for asset in file.assets {
        if matches!(
            asset.asset_kind.as_str(),
            "block-material-set" | "fluid-material-set" | "texture-layer"
        ) {
            let id = asset
                .id
                .parse::<StableId>()
                .map_err(ProductionHostError::from)?;
            layers.insert(id);
        }
    }
    Ok(layers)
}

#[derive(Deserialize)]
struct AuthoredAssetsFile {
    assets: Vec<AuthoredAssetRow>,
}

#[derive(Deserialize)]
struct AuthoredAssetRow {
    id: String,
    asset_kind: String,
}

#[cfg(test)]
mod tests {
    use latticeaxiom_render_contracts::TerrainMaterialPolicyV1;
    use latticeaxiom_voxel_mesh::{FaceOcclusion, MeshGroup};

    use super::{face_occlusion, mesh_group};

    #[test]
    fn policy_maps_to_mesh_group_and_does_not_full_occlude_cutout_or_fluids() {
        assert_eq!(
            mesh_group(TerrainMaterialPolicyV1::Opaque),
            MeshGroup::Opaque
        );
        assert_eq!(
            mesh_group(TerrainMaterialPolicyV1::Cutout),
            MeshGroup::Cutout
        );
        assert_eq!(
            mesh_group(TerrainMaterialPolicyV1::Translucent),
            MeshGroup::Translucent
        );
        assert_eq!(
            mesh_group(TerrainMaterialPolicyV1::Emissive),
            MeshGroup::Emissive
        );
        assert_eq!(
            face_occlusion(TerrainMaterialPolicyV1::Opaque),
            FaceOcclusion::Full
        );
        assert_eq!(
            face_occlusion(TerrainMaterialPolicyV1::Cutout),
            FaceOcclusion::Matching
        );
        assert_eq!(
            face_occlusion(TerrainMaterialPolicyV1::Translucent),
            FaceOcclusion::Matching
        );
    }
}
