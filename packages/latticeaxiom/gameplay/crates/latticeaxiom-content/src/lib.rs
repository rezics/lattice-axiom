//! Deterministic, engine-independent content-state contracts for D9.
//!
//! This crate validates typed block, biome, and fluid definitions, expands
//! bounded discrete block-state palettes, freezes material Role bindings,
//! compiles stable solid and fluid palettes, arbitrates orthogonal fluid
//! occupancy, and extracts the presentation binding index consumed by terrain
//! layer compilation. It owns no renderer, gameplay rules, package authoring
//! files, persistence backend, scheduler, or Bevy integration.
//!
//! Process-local numeric IDs are deliberately absent. Compiled palettes retain
//! concrete [`latticeaxiom_core::StableId`] values and canonical state, so a
//! caller can hand them to the independently versioned world-wire owner.

#![allow(
    clippy::result_large_err,
    reason = "activation-time diagnostics retain complete stable identities"
)]

mod biome;
mod block;
mod catalog;
mod error;
mod fluid;
mod header;
mod occupancy;
mod palette;
mod presentation;
mod state;

pub use biome::{
    BiomeChannelOfferV1, BiomeDefinitionV1, BiomeEmittedIntentV1, BiomeScopeV1, SpatialInfluenceV1,
    TerritoryParticipationV1,
};
pub use block::{
    BlockDefinitionV1, BlockFormV1, BlockRuleReferencesV1, BlockStateSemanticsV1, CollisionShapeV1,
    FluidOccupancyPolicyV1, OcclusionV1, ReplaceabilityV1, SelectionShapeV1, SolidOccupancyV1,
    ValidatedBlockDefinitionV1,
};
pub use catalog::{
    CONTENT_CATALOG_SCHEMA_MAJOR, ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1,
};
pub use error::{ContentError, ContentResult, FluidLevelError};
pub use fluid::fluid_semantics::{
    CellInspectKindV1, FluidCollisionKindV1, FluidInspectFragmentV1, FluidSelectionKindV1,
    cell_inspect_kind, classify_fluid_collision, classify_fluid_selection,
    fluid_cell_blocks_collision, fluid_cell_is_selectable, inspect_fluid_cell,
};
pub use fluid::solid_fluid_volume::{
    CHUNK_EDGE_V1, SOLID_FLUID_VOLUME_SCHEMA_V1, SolidFluidVolumeCellV1, SolidFluidVolumeV1,
    VolumeChunkCoordinateV1, volume_linear_index,
};
pub use fluid::{
    BoundedFluidUpdatePolicyV1, FluidCollisionPolicyV1, FluidDefinitionV1, FluidFlowV1,
    FluidLevelV1, FluidSelectionPolicyV1, FluidStateV1,
};
pub use header::{
    BIOME_DEFINITION_SCHEMA_V1, BLOCK_DEFINITION_SCHEMA_V1, ContentHeaderV1, ContentRevisionV1,
    FLUID_DEFINITION_SCHEMA_V1, FLUID_STATE_SCHEMA_V1,
};
pub use occupancy::{
    FluidOccupancyKindV1, FluidWorkAccountingV1, OccupancyArbitrationContextV1,
    OccupancyCandidateV1, OccupancyCellIntentV1, OccupancyCellV1, OccupancyRejectV1,
    ReplaceabilityKindV1, SOLID_FLUID_OCCUPANCY_CANDIDATE_SCHEMA_V1, SolidFluidArbitrationV1,
    SolidOccupancyKindV1, arbitrate_cell,
};
pub use palette::{
    CompiledFluidPaletteEntryV1, CompiledFluidPaletteV1, CompiledSolidPaletteEntryV1,
    CompiledSolidPaletteV1, FluidPaletteEntryV1, PaletteLimitsV1, SolidPaletteEntryV1,
};
pub use presentation::{ContentPresentationBindingV1, ContentPresentationKindV1};
pub use state::{
    AXIS_STATE_KEY_V1, AxisV1, BlockHalfV1, BlockStatePropertyV1, BlockStateV1,
    BlockStateValueKindV1, BlockStateValueV1, BlockStateVariantV1, FACING_STATE_KEY_V1, FacingV1,
    LIT_STATE_KEY_V1,
};

pub use catalog::{MaterialRoleBindingV1, ValidatedMaterialRoleBindingV1};
