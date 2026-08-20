//! GPU-independent render registration, planning, provider, and command contracts.
//!
//! This crate deliberately contains no Bevy, wgpu, window, device, or submission
//! code. A headless package kernel can use it to validate presentation-only
//! declarations before package code is activated. The Bevy host adapter remains
//! the sole owner of physical resources, barriers, renderer schedules, and GPU
//! submission.

mod command;
mod declaration;
mod graph;
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
pub use provider::{
    GpuCapabilities, GpuFeatureV1, GpuLimitV1, ProviderFallback, ProviderRealization,
    ProviderRequirement, ProviderRequirementFailure, ProviderSelectError, ProviderSelection,
    ProviderSelectionRequest, RejectedRealization, RenderProviderDecl, select_provider,
};
pub use registration::RenderRegistration;
