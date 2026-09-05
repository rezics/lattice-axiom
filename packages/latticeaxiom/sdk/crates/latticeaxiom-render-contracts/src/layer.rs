//! Locked terrain texture-layer tables.

#![allow(
    clippy::result_large_err,
    reason = "activation-time diagnostics retain complete stable identities"
)]

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageName, SchemaId, StableId, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Schema major emitted by the terrain layer-table compiler.
pub const TERRAIN_LAYER_TABLE_SCHEMA_MAJOR: u32 = 1;
/// Schema identity for a compiled terrain layer table.
pub const TERRAIN_LAYER_TABLE_SCHEMA_V1: &str = "latticeaxiom:schema/terrain-layer-table@1";
/// `RenderData` identity for a compiled terrain layer table.
pub const TERRAIN_LAYER_TABLE_DATA_V1: &str = "latticeaxiom:render-data/terrain-layer-table@1";
/// Platform fallback layer used when a locked row cannot resolve an authored texture.
pub const TERRAIN_LAYER_FALLBACK_ASSET_V1: &str = "latticeaxiom:asset/terrain-layer-fallback";

/// Caller-supplied compilation safety limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerrainLayerLimitsV1 {
    /// Maximum locked content rows.
    pub max_content_rows: usize,
    /// Maximum authored layer declarations.
    pub max_declarations: usize,
    /// Maximum known texture-layer identities.
    pub max_available_layers: usize,
}

impl Default for TerrainLayerLimitsV1 {
    fn default() -> Self {
        Self {
            max_content_rows: 4_096,
            max_declarations: 4_096,
            max_available_layers: 16_384,
        }
    }
}

/// Whether the client presentation package is present in the locked graph.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentationPresenceV1 {
    /// Compile against authored layer declarations and known assets.
    Present,
    /// Headless omission: emit fallback rows without requiring GPU assets.
    Omitted,
}

/// Coverage and pass policy for one compiled terrain layer.
///
/// Discriminant order matches `latticeaxiom_voxel_mesh::MeshGroup`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerrainMaterialPolicyV1 {
    /// Fully opaque surfaces.
    Opaque,
    /// Alpha-tested cutout surfaces.
    Cutout,
    /// Blended translucent surfaces.
    Translucent,
    /// Emissive terrain surfaces.
    Emissive,
}

impl TerrainMaterialPolicyV1 {
    /// All policies in stable mesh-group order.
    pub const ALL: [Self; 4] = [
        Self::Opaque,
        Self::Cutout,
        Self::Translucent,
        Self::Emissive,
    ];

    /// Stable index aligned with `MeshGroup::ALL`.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Opaque => 0,
            Self::Cutout => 1,
            Self::Translucent => 2,
            Self::Emissive => 3,
        }
    }
}

/// Cardinal voxel face used by face-specific layer maps.
///
/// Order matches `latticeaxiom_voxel_mesh::Face::ALL`: east `+X`, west `-X`,
/// up `+Y`, down `-Y`, south `+Z`, north `-Z`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerrainFaceV1 {
    /// East, `+X`.
    East,
    /// West, `-X`.
    West,
    /// Up, `+Y`.
    Up,
    /// Down, `-Y`.
    Down,
    /// South, `+Z`.
    South,
    /// North (conventional forward), `-Z`.
    North,
}

impl TerrainFaceV1 {
    /// All faces in stable output order.
    pub const ALL: [Self; 6] = [
        Self::East,
        Self::West,
        Self::Up,
        Self::Down,
        Self::South,
        Self::North,
    ];

    /// Stable index of this face within [`Self::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::East => 0,
            Self::West => 1,
            Self::Up => 2,
            Self::Down => 3,
            Self::South => 4,
            Self::North => 5,
        }
    }

    /// Merge-key discriminant for this face slot.
    #[must_use]
    pub const fn layer_variant(self) -> u8 {
        match self {
            Self::East => 0,
            Self::West => 1,
            Self::Up => 2,
            Self::Down => 3,
            Self::South => 4,
            Self::North => 5,
        }
    }
}

/// Voxel sampler contract frozen for terrain layers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoxelFilterModeV1 {
    /// Nearest-neighbor sampling.
    Nearest,
}

/// Address mode frozen for terrain layers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoxelAddressModeV1 {
    /// Clamp to the edge texel.
    ClampToEdge,
}

/// Mipmap policy frozen for terrain layers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoxelMipmapPolicyV1 {
    /// No mip chain; voxel texels stay pixel-crisp.
    None,
}

/// Color space frozen for terrain layers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoxelColorSpaceV1 {
    /// sRGB encoded color.
    Srgb,
}

/// Complete sampler contract applied to every compiled terrain layer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoxelSamplerPolicyV1 {
    /// Magnification and minification filter.
    pub filter: VoxelFilterModeV1,
    /// UV address mode.
    pub address: VoxelAddressModeV1,
    /// Mipmap generation policy.
    pub mipmaps: VoxelMipmapPolicyV1,
    /// Encoded color space.
    #[serde(rename = "color-space")]
    pub color_space: VoxelColorSpaceV1,
}

impl VoxelSamplerPolicyV1 {
    /// Nearest, clamp-to-edge, no mipmaps, sRGB.
    pub const TERRAIN_V1: Self = Self {
        filter: VoxelFilterModeV1::Nearest,
        address: VoxelAddressModeV1::ClampToEdge,
        mipmaps: VoxelMipmapPolicyV1::None,
        color_space: VoxelColorSpaceV1::Srgb,
    };
}

/// Face-specific texture mapping for one content row.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TerrainFaceMapV1 {
    /// One layer on every face.
    Uniform {
        /// Shared texture-layer identity.
        layer: StableId,
    },
    /// Distinct top, side, and bottom layers.
    CubeColumn {
        /// `+Y` layer.
        top: StableId,
        /// Horizontal layers.
        side: StableId,
        /// `-Y` layer.
        bottom: StableId,
    },
    /// Independent layer per cardinal face.
    SixFace {
        /// `+X` layer.
        east: StableId,
        /// `-X` layer.
        west: StableId,
        /// `+Y` layer.
        up: StableId,
        /// `-Y` layer.
        down: StableId,
        /// `+Z` layer.
        south: StableId,
        /// `-Z` layer.
        north: StableId,
    },
}

impl TerrainFaceMapV1 {
    fn referenced_layers(&self) -> Vec<&StableId> {
        match self {
            Self::Uniform { layer } => vec![layer],
            Self::CubeColumn { top, side, bottom } => vec![top, side, bottom],
            Self::SixFace {
                east,
                west,
                up,
                down,
                south,
                north,
            } => vec![east, west, up, down, south, north],
        }
    }

    fn resolve(&self) -> ResolvedFaceLayersV1 {
        let (layers, variants) = match self {
            Self::Uniform { layer } => (
                [
                    layer.clone(),
                    layer.clone(),
                    layer.clone(),
                    layer.clone(),
                    layer.clone(),
                    layer.clone(),
                ],
                [0, 0, 0, 0, 0, 0],
            ),
            Self::CubeColumn { top, side, bottom } => (
                [
                    side.clone(),
                    side.clone(),
                    top.clone(),
                    bottom.clone(),
                    side.clone(),
                    side.clone(),
                ],
                [0, 0, 1, 2, 0, 0],
            ),
            Self::SixFace {
                east,
                west,
                up,
                down,
                south,
                north,
            } => (
                [
                    east.clone(),
                    west.clone(),
                    up.clone(),
                    down.clone(),
                    south.clone(),
                    north.clone(),
                ],
                [0, 1, 2, 3, 4, 5],
            ),
        };
        ResolvedFaceLayersV1 { layers, variants }
    }
}

/// Resolved per-face layer identities in [`TerrainFaceV1::ALL`] order.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ResolvedFaceLayersV1 {
    layers: [StableId; 6],
    variants: [u8; 6],
}

impl ResolvedFaceLayersV1 {
    fn fallback(layer: &StableId) -> Self {
        Self {
            layers: [
                layer.clone(),
                layer.clone(),
                layer.clone(),
                layer.clone(),
                layer.clone(),
                layer.clone(),
            ],
            variants: [0, 0, 0, 0, 0, 0],
        }
    }

    /// Layer identity selected for `face`.
    #[must_use]
    pub fn layer(&self, face: TerrainFaceV1) -> &StableId {
        &self.layers[face.index()]
    }

    /// Merge-key variant selected for `face`.
    #[must_use]
    pub const fn variant(&self, face: TerrainFaceV1) -> u8 {
        self.variants[face.index()]
    }

    /// Six layer identities in stable face order.
    #[must_use]
    pub const fn layers(&self) -> &[StableId; 6] {
        &self.layers
    }
}

/// How a compiled row was produced.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerResolutionV1 {
    /// Authored declaration resolved against known layers.
    Authored,
    /// Missing or invalid declaration replaced by the platform fallback.
    Fallback,
    /// Presentation package omitted for a headless graph.
    Omitted,
}

/// Stable diagnostic emitted while compiling a layer table.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerrainLayerDiagnosticCodeV1 {
    /// Headless graph omitted the presentation package.
    HeadlessOmission,
    /// Locked content has no authored layer declaration.
    MissingDeclaration,
    /// A face map referenced an unknown texture layer.
    UnknownLayerReference,
    /// An authored declaration named content outside the locked set.
    UnknownContent,
}

/// Owner-aware presentation diagnostic. Never enters an authoritative hash.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct TerrainLayerDiagnosticV1 {
    code: TerrainLayerDiagnosticCodeV1,
    content: Option<StableId>,
    layer: Option<StableId>,
}

impl TerrainLayerDiagnosticV1 {
    fn new(
        code: TerrainLayerDiagnosticCodeV1,
        content: Option<StableId>,
        layer: Option<StableId>,
    ) -> Self {
        Self {
            code,
            content,
            layer,
        }
    }

    /// Stable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> TerrainLayerDiagnosticCodeV1 {
        self.code
    }

    /// Content identity, when the diagnostic is row-scoped.
    #[must_use]
    pub const fn content(&self) -> Option<&StableId> {
        self.content.as_ref()
    }

    /// Layer identity, when a reference failed.
    #[must_use]
    pub const fn layer(&self) -> Option<&StableId> {
        self.layer.as_ref()
    }
}

/// Locked content row accepted by the layer compiler.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedContentPresentationV1 {
    /// Exact block or fluid identity.
    pub content: StableId,
    /// Optional presentation binding from the content catalog.
    pub binding: Option<StableId>,
}

/// Authored layer declaration owned by a presentation package.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredTerrainLayerDeclV1 {
    /// Exact content identity.
    pub content: StableId,
    /// Mesh-group policy.
    pub policy: TerrainMaterialPolicyV1,
    /// Face-specific layer map.
    pub faces: TerrainFaceMapV1,
}

/// Package-owned authored layer document.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredTerrainLayerDocumentV1 {
    /// Owner schema for this document.
    pub authoring_schema: SchemaId,
    /// Owner-managed presentation revision.
    pub layers_revision: u32,
    /// Logical package that authored the rows.
    pub owner_package: PackageName,
    /// Must be false; presentation cannot claim world authority.
    pub authoritative: bool,
    /// Sampler contract applied to every row.
    pub sampler: VoxelSamplerPolicyV1,
    /// Layer declarations in arbitrary discovery order.
    pub layers: Vec<AuthoredTerrainLayerDeclV1>,
}

/// Input accepted by [`compile_terrain_layer_table`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerrainLayerCompileInputV1 {
    /// Requested compiler schema major.
    pub schema_major: u32,
    /// Whether presentation assets are in the locked graph.
    pub presence: PresentationPresenceV1,
    /// Locked content rows in arbitrary discovery order.
    pub content: Vec<LockedContentPresentationV1>,
    /// Authored declarations in arbitrary discovery order.
    pub declarations: Vec<AuthoredTerrainLayerDeclV1>,
    /// Known texture-layer identities from the locked asset graph.
    pub available_layers: BTreeSet<StableId>,
    /// Sampler requested by the presentation package.
    pub sampler: VoxelSamplerPolicyV1,
    /// Rejected when true; presentation cannot own world authority.
    pub claims_authoritative: bool,
}

/// One compiled layer-table row.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CompiledTerrainLayerRowV1 {
    content: StableId,
    table_index: u16,
    policy: TerrainMaterialPolicyV1,
    faces: ResolvedFaceLayersV1,
    sampler: VoxelSamplerPolicyV1,
    resolution: LayerResolutionV1,
}

impl CompiledTerrainLayerRowV1 {
    /// Exact content identity.
    #[must_use]
    pub const fn content(&self) -> &StableId {
        &self.content
    }

    /// Stable index of this row inside the compiled table.
    #[must_use]
    pub const fn table_index(&self) -> u16 {
        self.table_index
    }

    /// Mesh-group policy.
    #[must_use]
    pub const fn policy(&self) -> TerrainMaterialPolicyV1 {
        self.policy
    }

    /// Resolved per-face layers.
    #[must_use]
    pub const fn faces(&self) -> &ResolvedFaceLayersV1 {
        &self.faces
    }

    /// Sampler applied to this row.
    #[must_use]
    pub const fn sampler(&self) -> VoxelSamplerPolicyV1 {
        self.sampler
    }

    /// Whether the row is authored, fallback, or omitted.
    #[must_use]
    pub const fn resolution(&self) -> LayerResolutionV1 {
        self.resolution
    }
}

/// Compiled, canonically ordered terrain layer table.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompiledTerrainLayerTableV1 {
    schema_major: u32,
    presence: PresentationPresenceV1,
    rows: Vec<CompiledTerrainLayerRowV1>,
    diagnostics: Vec<TerrainLayerDiagnosticV1>,
    fingerprint: CanonicalHash,
}

impl CompiledTerrainLayerTableV1 {
    /// Compiler schema major.
    #[must_use]
    pub const fn schema_major(&self) -> u32 {
        self.schema_major
    }

    /// Presentation presence used during compilation.
    #[must_use]
    pub const fn presence(&self) -> PresentationPresenceV1 {
        self.presence
    }

    /// Compiled rows in stable content-identity order.
    #[must_use]
    pub fn rows(&self) -> &[CompiledTerrainLayerRowV1] {
        &self.rows
    }

    /// Deterministic diagnostics in stable code/content/layer order.
    #[must_use]
    pub fn diagnostics(&self) -> &[TerrainLayerDiagnosticV1] {
        &self.diagnostics
    }

    /// Presentation-only fingerprint. Never an authoritative world hash.
    #[must_use]
    pub const fn fingerprint(&self) -> CanonicalHash {
        self.fingerprint
    }

    /// Finds the compiled row for an exact content identity.
    #[must_use]
    pub fn row(&self, content: &StableId) -> Option<&CompiledTerrainLayerRowV1> {
        self.rows
            .binary_search_by(|candidate| candidate.content.cmp(content))
            .ok()
            .map(|index| &self.rows[index])
    }
}

/// Deterministic failure produced while compiling a terrain layer table.
#[derive(Debug, Error)]
pub enum TerrainLayerCompileError {
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    CanonicalEncoding(#[from] CanonicalJsonError),
    /// The input requests a schema major this compiler does not implement.
    #[error("unsupported terrain-layer schema major {actual}; expected {expected}")]
    UnsupportedSchema {
        /// Supported major.
        expected: u32,
        /// Rejected major.
        actual: u32,
    },
    /// A bounded collection exceeded its configured limit.
    #[error("{resource} count {actual} exceeds limit {limit}")]
    LimitExceeded {
        /// Bounded resource name.
        resource: &'static str,
        /// Observed count.
        actual: usize,
        /// Inclusive limit.
        limit: usize,
    },
    /// Two locked content rows used the same identity.
    #[error("duplicate locked content `{id}`")]
    DuplicateContent {
        /// Duplicated identity.
        id: StableId,
    },
    /// Two authored declarations used the same content identity.
    #[error("duplicate terrain-layer declaration `{id}`")]
    DuplicateDeclaration {
        /// Duplicated identity.
        id: StableId,
    },
    /// Presentation data claimed to be authoritative.
    #[error("terrain layer tables cannot claim authoritative world ownership")]
    AuthoritativeClaim,
    /// Sampler contract is outside the frozen terrain v1 policy.
    #[error("unsupported terrain sampler policy")]
    UnsupportedSampler,
}

/// Compiles a locked terrain texture-layer table.
///
/// Discovery order of content, declarations, and available layers does not
/// affect rows, diagnostics, or the presentation fingerprint. Missing and
/// invalid references become the platform fallback plus a stable diagnostic.
/// Headless omission compiles the same locked content set without GPU assets.
///
/// # Errors
///
/// Returns a typed error for unsupported schema majors, exceeded limits,
/// duplicate identities, an authoritative claim, an unsupported sampler, or
/// canonical encoding failure.
///
/// # Panics
///
/// Panics only if the platform fallback asset identity fails to parse. That
/// identity is a crate constant and failure is a programmer error.
pub fn compile_terrain_layer_table(
    mut input: TerrainLayerCompileInputV1,
    limits: TerrainLayerLimitsV1,
) -> Result<CompiledTerrainLayerTableV1, TerrainLayerCompileError> {
    validate_compile_input(&mut input, limits)?;
    let fallback_asset = platform_fallback_layer();
    let (rows, diagnostics) = match input.presence {
        PresentationPresenceV1::Omitted => compile_omitted_rows(&input.content, &fallback_asset)?,
        PresentationPresenceV1::Present => compile_present_rows(&input, &fallback_asset)?,
    };
    finish_table(input.presence, rows, diagnostics)
}

fn validate_compile_input(
    input: &mut TerrainLayerCompileInputV1,
    limits: TerrainLayerLimitsV1,
) -> Result<(), TerrainLayerCompileError> {
    if input.schema_major != TERRAIN_LAYER_TABLE_SCHEMA_MAJOR {
        return Err(TerrainLayerCompileError::UnsupportedSchema {
            expected: TERRAIN_LAYER_TABLE_SCHEMA_MAJOR,
            actual: input.schema_major,
        });
    }
    if input.claims_authoritative {
        return Err(TerrainLayerCompileError::AuthoritativeClaim);
    }
    enforce_count(
        "locked_content_rows",
        input.content.len(),
        limits.max_content_rows,
    )?;
    enforce_count(
        "terrain_layer_declarations",
        input.declarations.len(),
        limits.max_declarations,
    )?;
    enforce_count(
        "available_terrain_layers",
        input.available_layers.len(),
        limits.max_available_layers,
    )?;
    input
        .content
        .sort_by(|left, right| left.content.cmp(&right.content));
    reject_duplicate_content(&input.content)?;
    input
        .declarations
        .sort_by(|left, right| left.content.cmp(&right.content));
    reject_duplicate_declarations(&input.declarations)
}

fn platform_fallback_layer() -> StableId {
    match TERRAIN_LAYER_FALLBACK_ASSET_V1.parse::<StableId>() {
        Ok(id) => id,
        Err(error) => {
            panic!("invariant: `{TERRAIN_LAYER_FALLBACK_ASSET_V1}` is a valid StableId: {error}")
        }
    }
}

fn compile_omitted_rows(
    content: &[LockedContentPresentationV1],
    fallback_asset: &StableId,
) -> Result<
    (
        Vec<CompiledTerrainLayerRowV1>,
        Vec<TerrainLayerDiagnosticV1>,
    ),
    TerrainLayerCompileError,
> {
    let diagnostics = vec![TerrainLayerDiagnosticV1::new(
        TerrainLayerDiagnosticCodeV1::HeadlessOmission,
        None,
        None,
    )];
    let mut rows = Vec::with_capacity(content.len());
    for (index, content) in content.iter().enumerate() {
        rows.push(fallback_row(
            content.content.clone(),
            table_index(index)?,
            LayerResolutionV1::Omitted,
            fallback_asset,
        ));
    }
    Ok((rows, diagnostics))
}

fn compile_present_rows(
    input: &TerrainLayerCompileInputV1,
    fallback_asset: &StableId,
) -> Result<
    (
        Vec<CompiledTerrainLayerRowV1>,
        Vec<TerrainLayerDiagnosticV1>,
    ),
    TerrainLayerCompileError,
> {
    if input.sampler != VoxelSamplerPolicyV1::TERRAIN_V1 {
        return Err(TerrainLayerCompileError::UnsupportedSampler);
    }
    let declarations = input
        .declarations
        .iter()
        .map(|declaration| (declaration.content.clone(), declaration))
        .collect::<BTreeMap<_, _>>();
    let locked = input
        .content
        .iter()
        .map(|row| row.content.clone())
        .collect::<BTreeSet<_>>();
    let mut diagnostics = Vec::new();
    for content in declarations.keys() {
        if !locked.contains(content) {
            diagnostics.push(TerrainLayerDiagnosticV1::new(
                TerrainLayerDiagnosticCodeV1::UnknownContent,
                Some(content.clone()),
                None,
            ));
        }
    }
    let mut rows = Vec::with_capacity(input.content.len());
    for (index, content) in input.content.iter().enumerate() {
        rows.push(resolve_present_row(
            content,
            table_index(index)?,
            declarations.get(&content.content).copied(),
            &input.available_layers,
            fallback_asset,
            &mut diagnostics,
        ));
    }
    Ok((rows, diagnostics))
}

fn resolve_present_row(
    content: &LockedContentPresentationV1,
    table_index: u16,
    declaration: Option<&AuthoredTerrainLayerDeclV1>,
    available_layers: &BTreeSet<StableId>,
    fallback_asset: &StableId,
    diagnostics: &mut Vec<TerrainLayerDiagnosticV1>,
) -> CompiledTerrainLayerRowV1 {
    let Some(declaration) = declaration else {
        diagnostics.push(TerrainLayerDiagnosticV1::new(
            TerrainLayerDiagnosticCodeV1::MissingDeclaration,
            Some(content.content.clone()),
            None,
        ));
        return fallback_row(
            content.content.clone(),
            table_index,
            LayerResolutionV1::Fallback,
            fallback_asset,
        );
    };
    let unknown = unknown_layers(&declaration.faces, available_layers);
    if unknown.is_empty() {
        return CompiledTerrainLayerRowV1 {
            content: content.content.clone(),
            table_index,
            policy: declaration.policy,
            faces: declaration.faces.resolve(),
            sampler: VoxelSamplerPolicyV1::TERRAIN_V1,
            resolution: LayerResolutionV1::Authored,
        };
    }
    for layer in unknown {
        diagnostics.push(TerrainLayerDiagnosticV1::new(
            TerrainLayerDiagnosticCodeV1::UnknownLayerReference,
            Some(content.content.clone()),
            Some(layer),
        ));
    }
    fallback_row(
        content.content.clone(),
        table_index,
        LayerResolutionV1::Fallback,
        fallback_asset,
    )
}

fn finish_table(
    presence: PresentationPresenceV1,
    rows: Vec<CompiledTerrainLayerRowV1>,
    mut diagnostics: Vec<TerrainLayerDiagnosticV1>,
) -> Result<CompiledTerrainLayerTableV1, TerrainLayerCompileError> {
    diagnostics.sort();
    diagnostics.dedup();
    let fingerprint = canonical_json_hash(&FingerprintProjection {
        schema_major: TERRAIN_LAYER_TABLE_SCHEMA_MAJOR,
        presence,
        rows: &rows,
        diagnostics: &diagnostics,
    })?;
    Ok(CompiledTerrainLayerTableV1 {
        schema_major: TERRAIN_LAYER_TABLE_SCHEMA_MAJOR,
        presence,
        rows,
        diagnostics,
        fingerprint,
    })
}

fn fallback_row(
    content: StableId,
    table_index: u16,
    resolution: LayerResolutionV1,
    fallback_asset: &StableId,
) -> CompiledTerrainLayerRowV1 {
    CompiledTerrainLayerRowV1 {
        content,
        table_index,
        policy: TerrainMaterialPolicyV1::Opaque,
        faces: ResolvedFaceLayersV1::fallback(fallback_asset),
        sampler: VoxelSamplerPolicyV1::TERRAIN_V1,
        resolution,
    }
}

fn unknown_layers(faces: &TerrainFaceMapV1, available: &BTreeSet<StableId>) -> Vec<StableId> {
    let mut unknown = faces
        .referenced_layers()
        .into_iter()
        .filter(|layer| !available.contains(layer))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    unknown.dedup();
    unknown
}

fn table_index(index: usize) -> Result<u16, TerrainLayerCompileError> {
    u16::try_from(index).map_err(|_| TerrainLayerCompileError::LimitExceeded {
        resource: "terrain_layer_table_index",
        actual: index,
        limit: usize::from(u16::MAX),
    })
}

fn enforce_count(
    resource: &'static str,
    actual: usize,
    limit: usize,
) -> Result<(), TerrainLayerCompileError> {
    if actual > limit {
        return Err(TerrainLayerCompileError::LimitExceeded {
            resource,
            actual,
            limit,
        });
    }
    Ok(())
}

fn reject_duplicate_content(
    rows: &[LockedContentPresentationV1],
) -> Result<(), TerrainLayerCompileError> {
    let mut previous: Option<&StableId> = None;
    for row in rows {
        if previous == Some(&row.content) {
            return Err(TerrainLayerCompileError::DuplicateContent {
                id: row.content.clone(),
            });
        }
        previous = Some(&row.content);
    }
    Ok(())
}

fn reject_duplicate_declarations(
    rows: &[AuthoredTerrainLayerDeclV1],
) -> Result<(), TerrainLayerCompileError> {
    let mut previous: Option<&StableId> = None;
    for row in rows {
        if previous == Some(&row.content) {
            return Err(TerrainLayerCompileError::DuplicateDeclaration {
                id: row.content.clone(),
            });
        }
        previous = Some(&row.content);
    }
    Ok(())
}

#[derive(Serialize)]
struct FingerprintProjection<'a> {
    schema_major: u32,
    presence: PresentationPresenceV1,
    rows: &'a [CompiledTerrainLayerRowV1],
    diagnostics: &'a [TerrainLayerDiagnosticV1],
}
