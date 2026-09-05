//! GPU-independent render registration, planning, provider, command, and
//! terrain material contracts.
//!
//! This crate deliberately contains no Bevy, wgpu, window, device, or submission
//! code. A headless package kernel can use it to validate presentation-only
//! declarations and compile locked texture-layer tables before package code is
//! activated. The Bevy host adapter remains the sole owner of physical
//! resources, barriers, renderer schedules, and GPU submission.

mod command;
mod declaration;
mod graph;
mod layer;
mod provider;
mod registration;

pub use command::{
    Command, CommandCatalog, CommandFrame, CommandFrameLimits, CommandHandle, CommandList,
    CommandOwnerId, CommandValidationError, DeclaredMarker, DeclaredParameterLayout,
    DeclaredPipeline, DeclaredResourceSet, DeclaredUploadAllocation, FeatureFrameSubmission,
    FrameCommandValidator, HandleGeneration, HandleIndex, PipelineKind, UploadCommit, UploadRange,
    ValidatedCommandFrame, ValidatedFeatureTrace, ValidatedListTrace, ValidatedTraceCommand,
    ViewId,
};
pub use declaration::{
    Core3dSlotParseError, Core3dSlotV1, RenderDataDecl, RenderDeclarationKind, RenderFeatureDecl,
};
pub use graph::{
    CompiledPass, CompiledRenderPlan, ExtentPolicy, GraphCompileError, PassResourceUse,
    RenderGraphCompiler, RenderPassDecl, RenderPlanInput, RenderResourceDecl, RenderResourceFormat,
    RenderResourceKind, RenderResourceLifetime, RenderResourceProducer, RenderResourceScope,
    RenderResourceUsage, RenderResourceVersion, TextureFormatV1,
};
pub use layer::{
    AuthoredTerrainLayerDeclV1, AuthoredTerrainLayerDocumentV1, CompiledTerrainLayerRowV1,
    CompiledTerrainLayerTableV1, LayerResolutionV1, LockedContentPresentationV1,
    PresentationPresenceV1, ResolvedFaceLayersV1, TERRAIN_LAYER_FALLBACK_ASSET_V1,
    TERRAIN_LAYER_TABLE_DATA_V1, TERRAIN_LAYER_TABLE_SCHEMA_MAJOR, TERRAIN_LAYER_TABLE_SCHEMA_V1,
    TerrainFaceMapV1, TerrainFaceV1, TerrainLayerCompileError, TerrainLayerCompileInputV1,
    TerrainLayerDiagnosticCodeV1, TerrainLayerDiagnosticV1, TerrainLayerLimitsV1,
    TerrainMaterialPolicyV1, VoxelAddressModeV1, VoxelColorSpaceV1, VoxelFilterModeV1,
    VoxelMipmapPolicyV1, VoxelSamplerPolicyV1, compile_terrain_layer_table,
};
pub use provider::{
    GpuCapabilities, GpuFeatureV1, GpuLimitV1, ProviderFallback, ProviderRealization,
    ProviderRequirement, ProviderRequirementFailure, ProviderSelectError, ProviderSelection,
    ProviderSelectionRequest, RejectedRealization, RenderProviderDecl, select_provider,
};
pub use registration::RenderRegistration;
