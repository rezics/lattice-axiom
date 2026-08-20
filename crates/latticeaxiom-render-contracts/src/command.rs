use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::StableId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Core3dSlotV1;

const MAX_COMMAND_LISTS: u32 = 4;
const MAX_COMMANDS_PER_LIST: u32 = 256;
const MAX_ENCODED_COMMAND_BYTES: u64 = 64 * 1024;
const MAX_COMMITTED_UPLOAD_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DISPATCH_COMMANDS: u32 = 32;
const MAX_CUMULATIVE_WORKGROUPS: u64 = 1_048_576;
const MAX_FULLSCREEN_DRAWS: u32 = 8;
const MAX_GLOBAL_UPLOAD_BYTES: u64 = 16 * 1024 * 1024;

/// Dense slot within a versioned opaque command table.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct HandleIndex(
    /// Zero-based table slot.
    pub u32,
);

/// Generation protecting an opaque table slot from stale reuse.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct HandleGeneration(
    /// Non-zero generation; `u32::MAX` is retired and cannot be used.
    pub u32,
);

/// Stable opaque command reference used by the portable ABI schema.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandHandle {
    /// Non-zero table identity owned by one engine instance and plan image.
    pub table_id: u64,
    /// Slot in that table.
    pub slot: HandleIndex,
    /// Current slot generation.
    pub generation: HandleGeneration,
}

/// Numeric owner assigned to a feature in the active registration image.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CommandOwnerId(
    /// Dense feature owner ID.
    pub u32,
);

/// Numeric view identity valid for one presentation frame.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ViewId(
    /// Host-assigned view ID.
    pub u32,
);

/// Pipeline execution kind exposed by command-list contract major 1.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PipelineKind {
    /// Compute dispatch pipeline.
    Compute,
    /// Fullscreen-triangle graphics pipeline.
    FullscreenTriangle,
}

/// Pipeline declared in the compiled render plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredPipeline {
    /// Typed opaque handle.
    pub handle: CommandHandle,
    /// Stable ID used in validated equivalence traces.
    pub id: StableId,
    /// Feature owner.
    pub owner: CommandOwnerId,
    /// Semantic slot where the pipeline may execute.
    pub slot: Core3dSlotV1,
    /// Compute or fullscreen execution kind.
    pub kind: PipelineKind,
    /// Resource sets that must be bound before execution.
    pub required_resource_sets: BTreeSet<CommandHandle>,
    /// Exact parameter layout required before execution, when present.
    pub parameter_layout: Option<CommandHandle>,
}

/// Resource set declared in the compiled render plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredResourceSet {
    /// Typed opaque handle.
    pub handle: CommandHandle,
    /// Stable ID used in validated equivalence traces.
    pub id: StableId,
    /// Feature owner.
    pub owner: CommandOwnerId,
    /// Semantic slot where the resource set is valid.
    pub slot: Core3dSlotV1,
}

/// Parameter block layout declared in the compiled render plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredParameterLayout {
    /// Typed opaque handle.
    pub handle: CommandHandle,
    /// Stable ID used in validated equivalence traces.
    pub id: StableId,
    /// Feature owner.
    pub owner: CommandOwnerId,
    /// Semantic slot where the layout is valid.
    pub slot: Core3dSlotV1,
    /// Exact byte size accepted by `SetParameterBlock`.
    pub byte_size: u64,
    /// Required non-zero power-of-two byte alignment.
    pub alignment: u64,
}

/// Host-owned upload allocation scoped to one callback.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredUploadAllocation {
    /// Typed opaque handle.
    pub handle: CommandHandle,
    /// Feature owner.
    pub owner: CommandOwnerId,
    /// Frame in which the allocation is valid.
    pub frame: u64,
    /// View in which the allocation is valid.
    pub view: ViewId,
    /// Callback epoch after which the allocation expires.
    pub callback_epoch: u64,
    /// Allocation capacity in bytes.
    pub byte_capacity: u64,
    /// Guaranteed non-zero power-of-two base alignment.
    pub alignment: u64,
}

/// Bounded debug marker declared before code activation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredMarker {
    /// Typed opaque handle.
    pub handle: CommandHandle,
    /// Stable ID used instead of an unbounded module-provided string.
    pub id: StableId,
    /// Feature owner.
    pub owner: CommandOwnerId,
    /// Semantic slot where the marker is valid.
    pub slot: Core3dSlotV1,
}

/// Host-built command catalog derived from a compiled plan and registration image.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandCatalog {
    /// Non-zero table identity; changing plan/instance creates another table.
    pub table_id: u64,
    /// Active plan generation used to reject old command lists.
    pub plan_generation: u64,
    /// Declared pipelines.
    pub pipelines: Vec<DeclaredPipeline>,
    /// Declared resource sets.
    pub resource_sets: Vec<DeclaredResourceSet>,
    /// Declared parameter layouts.
    pub parameter_layouts: Vec<DeclaredParameterLayout>,
    /// Callback-scoped upload allocations.
    pub upload_allocations: Vec<DeclaredUploadAllocation>,
    /// Declared bounded debug markers.
    pub markers: Vec<DeclaredMarker>,
    /// Per-axis device limit for one dispatch.
    pub max_workgroups_per_dimension: [u32; 3],
}

/// A range in a host-provided upload allocation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UploadRange {
    /// Upload allocation handle.
    pub allocation: CommandHandle,
    /// Byte offset from the allocation base.
    pub offset: u64,
    /// Non-zero byte length.
    pub length: u64,
}

/// One checked commit of a host-provided upload allocation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UploadCommit {
    /// Upload allocation handle.
    pub allocation: CommandHandle,
    /// Committed byte offset.
    pub offset: u64,
    /// Non-zero committed byte length.
    pub length: u64,
}

/// Portable render command vocabulary frozen for D5.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "opcode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Command {
    /// Select a manifest-declared pipeline and clear previous bindings.
    SelectDeclaredPipeline {
        /// Pipeline handle.
        pipeline: CommandHandle,
    },
    /// Bind a manifest-declared resource set compatible with the pipeline.
    BindDeclaredResourceSet {
        /// Resource-set handle.
        resource_set: CommandHandle,
    },
    /// Bind one exact-size parameter block from a committed upload range.
    SetParameterBlock {
        /// Parameter layout handle.
        layout: CommandHandle,
        /// Committed upload range containing the block.
        upload: UploadRange,
    },
    /// Dispatch the selected compute pipeline.
    Dispatch {
        /// Workgroup count in X.
        x: u32,
        /// Workgroup count in Y.
        y: u32,
        /// Workgroup count in Z.
        z: u32,
    },
    /// Draw one fullscreen triangle with the selected graphics pipeline.
    DrawFullscreenTriangle {},
    /// Emit a predeclared bounded debug marker.
    DebugMarker {
        /// Marker handle.
        marker: CommandHandle,
    },
}

impl Command {
    const fn minimum_encoded_bytes(&self) -> u64 {
        match self {
            Self::SelectDeclaredPipeline { .. }
            | Self::BindDeclaredResourceSet { .. }
            | Self::DebugMarker { .. } => 17,
            Self::SetParameterBlock { .. } => 49,
            Self::Dispatch { .. } => 13,
            Self::DrawFullscreenTriangle {} => 1,
        }
    }
}

/// One fully decoded command list.
///
/// `encoded_bytes` is measured by the bounded decoder, not supplied by module
/// business logic. Validation checks both a structural minimum and the hard cap.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandList {
    /// Semantic slot for every command in this list.
    pub slot: Core3dSlotV1,
    /// Actual encoded byte length measured by the decoder.
    pub encoded_bytes: u32,
    /// Fully decoded commands.
    pub commands: Vec<Command>,
}

/// All portable command output for one feature, view, and frame.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureFrameSubmission {
    /// Feature owner from the active registration image.
    pub owner: CommandOwnerId,
    /// Active plan generation.
    pub plan_generation: u64,
    /// Presentation frame.
    pub frame: u64,
    /// Target view.
    pub view: ViewId,
    /// Callback epoch owning every upload allocation.
    pub callback_epoch: u64,
    /// At most four command lists across all feature passes.
    pub lists: Vec<CommandList>,
    /// One-shot upload commits accepted atomically with the lists.
    pub upload_commits: Vec<UploadCommit>,
}

/// Command submissions decoded for one presentation frame.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandFrame {
    /// Presentation frame shared by every submission.
    pub frame: u64,
    /// Feature/view submissions in arbitrary discovery order.
    pub submissions: Vec<FeatureFrameSubmission>,
}

/// Effective command quotas; values above contract maxima are clamped.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandFrameLimits {
    /// Maximum command lists per feature/view/frame.
    pub command_lists: u32,
    /// Maximum commands per list.
    pub commands_per_list: u32,
    /// Maximum encoded bytes across a feature submission.
    pub encoded_command_bytes: u64,
    /// Maximum committed upload bytes per feature/view/frame.
    pub committed_upload_bytes: u64,
    /// Maximum dispatch commands per feature/view/frame.
    pub dispatch_commands: u32,
    /// Maximum cumulative workgroups per feature/view/frame.
    pub cumulative_workgroups: u64,
    /// Maximum fullscreen draws per feature/view/frame.
    pub fullscreen_draws: u32,
    /// Maximum committed upload bytes across the entire frame.
    pub global_upload_bytes: u64,
}

impl Default for CommandFrameLimits {
    fn default() -> Self {
        Self {
            command_lists: MAX_COMMAND_LISTS,
            commands_per_list: MAX_COMMANDS_PER_LIST,
            encoded_command_bytes: MAX_ENCODED_COMMAND_BYTES,
            committed_upload_bytes: MAX_COMMITTED_UPLOAD_BYTES,
            dispatch_commands: MAX_DISPATCH_COMMANDS,
            cumulative_workgroups: MAX_CUMULATIVE_WORKGROUPS,
            fullscreen_draws: MAX_FULLSCREEN_DRAWS,
            global_upload_bytes: MAX_GLOBAL_UPLOAD_BYTES,
        }
    }
}

impl CommandFrameLimits {
    fn clamped(self) -> Self {
        let hard = Self::default();
        Self {
            command_lists: self.command_lists.min(hard.command_lists),
            commands_per_list: self.commands_per_list.min(hard.commands_per_list),
            encoded_command_bytes: self.encoded_command_bytes.min(hard.encoded_command_bytes),
            committed_upload_bytes: self.committed_upload_bytes.min(hard.committed_upload_bytes),
            dispatch_commands: self.dispatch_commands.min(hard.dispatch_commands),
            cumulative_workgroups: self.cumulative_workgroups.min(hard.cumulative_workgroups),
            fullscreen_draws: self.fullscreen_draws.min(hard.fullscreen_draws),
            global_upload_bytes: self.global_upload_bytes.min(hard.global_upload_bytes),
        }
    }
}

/// Stable, handle-free command emitted after complete validation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "opcode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ValidatedTraceCommand {
    /// Select a declared pipeline by stable identity.
    SelectDeclaredPipeline {
        /// Pipeline stable ID.
        pipeline: StableId,
    },
    /// Bind a declared resource set by stable identity.
    BindDeclaredResourceSet {
        /// Resource-set stable ID.
        resource_set: StableId,
    },
    /// Bind a validated parameter block.
    SetParameterBlock {
        /// Parameter-layout stable ID.
        layout: StableId,
        /// Exact byte length.
        byte_length: u64,
    },
    /// Validated compute dispatch dimensions.
    Dispatch {
        /// Workgroups in X.
        x: u32,
        /// Workgroups in Y.
        y: u32,
        /// Workgroups in Z.
        z: u32,
    },
    /// One validated fullscreen triangle.
    DrawFullscreenTriangle {},
    /// Predeclared debug marker by stable identity.
    DebugMarker {
        /// Marker stable ID.
        marker: StableId,
    },
}

/// Stable command trace for one list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedListTrace {
    /// Semantic slot.
    pub slot: Core3dSlotV1,
    /// Stable, handle-free command trace.
    pub commands: Vec<ValidatedTraceCommand>,
}

/// Stable trace for one feature/view/frame after full atomic validation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedFeatureTrace {
    /// Feature owner numeric ID.
    pub owner: CommandOwnerId,
    /// Presentation frame.
    pub frame: u64,
    /// Target view.
    pub view: ViewId,
    /// Stable list traces.
    pub lists: Vec<ValidatedListTrace>,
    /// Validated encoded bytes.
    pub encoded_bytes: u64,
    /// Validated committed upload bytes.
    pub upload_bytes: u64,
    /// Validated cumulative workgroups.
    pub workgroups: u64,
}

/// Entire frame accepted only after every submission and global quota pass.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedCommandFrame {
    /// Presentation frame.
    pub frame: u64,
    /// Traces sorted by owner then view for deterministic comparison.
    pub features: Vec<ValidatedFeatureTrace>,
    /// Global committed upload bytes.
    pub global_upload_bytes: u64,
}

/// Pure validator for decoded D5 command frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameCommandValidator {
    limits: CommandFrameLimits,
}

impl FrameCommandValidator {
    /// Creates a validator whose profile limits can only lower contract maxima.
    #[must_use]
    pub fn new(profile_limits: CommandFrameLimits) -> Self {
        Self {
            limits: profile_limits.clamped(),
        }
    }

    /// Validates an entire frame without performing uploads or GPU submission.
    ///
    /// The method builds all traces locally, checks the global upload ceiling,
    /// then returns the accepted frame. Therefore any error rejects the complete
    /// frame and cannot expose a validated prefix.
    ///
    /// # Errors
    ///
    /// Returns [`CommandValidationError`] for malformed catalogs, stale/foreign
    /// handles, scope/range/alignment violations, pipeline incompatibility,
    /// arithmetic overflow, or any effective quota breach.
    pub fn validate(
        &self,
        catalog: &CommandCatalog,
        frame: &CommandFrame,
    ) -> Result<ValidatedCommandFrame, CommandValidationError> {
        validate_catalog(catalog)?;
        let mut seen = BTreeSet::new();
        let mut features = Vec::with_capacity(frame.submissions.len());
        let mut global_upload_bytes = 0_u64;
        for submission in &frame.submissions {
            if submission.frame != frame.frame {
                return Err(CommandValidationError::FrameMismatch {
                    expected: frame.frame,
                    actual: submission.frame,
                });
            }
            if !seen.insert((submission.owner, submission.view)) {
                return Err(CommandValidationError::DuplicateFeatureViewSubmission {
                    owner: submission.owner,
                    view: submission.view,
                });
            }
            let trace = validate_submission(catalog, submission, self.limits)?;
            global_upload_bytes = global_upload_bytes.checked_add(trace.upload_bytes).ok_or(
                CommandValidationError::CounterOverflow {
                    counter: "global-upload-bytes",
                },
            )?;
            features.push(trace);
        }
        if global_upload_bytes > self.limits.global_upload_bytes {
            return Err(CommandValidationError::GlobalUploadQuota {
                actual: global_upload_bytes,
                maximum: self.limits.global_upload_bytes,
            });
        }
        features.sort_by_key(|entry| (entry.owner, entry.view));
        Ok(ValidatedCommandFrame {
            frame: frame.frame,
            features,
            global_upload_bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HandleKind {
    Pipeline,
    ResourceSet,
    ParameterLayout,
    UploadAllocation,
    Marker,
}

impl HandleKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pipeline => "pipeline",
            Self::ResourceSet => "resource-set",
            Self::ParameterLayout => "parameter-layout",
            Self::UploadAllocation => "upload-allocation",
            Self::Marker => "marker",
        }
    }
}

fn validate_catalog(catalog: &CommandCatalog) -> Result<(), CommandValidationError> {
    if catalog.table_id == 0 {
        return Err(CommandValidationError::InvalidCatalogTable);
    }
    if catalog.max_workgroups_per_dimension.contains(&0) {
        return Err(CommandValidationError::InvalidDeviceLimit);
    }
    let mut slots = BTreeMap::new();
    for pipeline in &catalog.pipelines {
        validate_catalog_handle(catalog, pipeline.handle, HandleKind::Pipeline, &mut slots)?;
    }
    for resource_set in &catalog.resource_sets {
        validate_catalog_handle(
            catalog,
            resource_set.handle,
            HandleKind::ResourceSet,
            &mut slots,
        )?;
    }
    for layout in &catalog.parameter_layouts {
        validate_catalog_handle(
            catalog,
            layout.handle,
            HandleKind::ParameterLayout,
            &mut slots,
        )?;
        if layout.byte_size == 0 || !valid_alignment(layout.alignment) {
            return Err(CommandValidationError::InvalidLayout {
                layout: layout.id.clone(),
            });
        }
    }
    for allocation in &catalog.upload_allocations {
        validate_catalog_handle(
            catalog,
            allocation.handle,
            HandleKind::UploadAllocation,
            &mut slots,
        )?;
        if allocation.byte_capacity == 0 || !valid_alignment(allocation.alignment) {
            return Err(CommandValidationError::InvalidUploadAllocation {
                handle: allocation.handle,
            });
        }
    }
    for marker in &catalog.markers {
        validate_catalog_handle(catalog, marker.handle, HandleKind::Marker, &mut slots)?;
    }
    for pipeline in &catalog.pipelines {
        for resource_set in &pipeline.required_resource_sets {
            let declared = lookup_resource_set(catalog, *resource_set)?;
            validate_owner_and_slot(
                pipeline.owner,
                pipeline.slot,
                declared.owner,
                declared.slot,
                *resource_set,
            )?;
        }
        if let Some(layout) = pipeline.parameter_layout {
            let declared = lookup_parameter_layout(catalog, layout)?;
            validate_owner_and_slot(
                pipeline.owner,
                pipeline.slot,
                declared.owner,
                declared.slot,
                layout,
            )?;
        }
    }
    Ok(())
}

fn validate_catalog_handle(
    catalog: &CommandCatalog,
    handle: CommandHandle,
    kind: HandleKind,
    slots: &mut BTreeMap<HandleIndex, HandleKind>,
) -> Result<(), CommandValidationError> {
    if handle.table_id != catalog.table_id
        || handle.generation.0 == 0
        || handle.generation.0 == u32::MAX
    {
        return Err(CommandValidationError::InvalidCatalogHandle {
            kind: kind.as_str(),
            handle,
        });
    }
    if let Some(previous) = slots.insert(handle.slot, kind) {
        return Err(CommandValidationError::DuplicateCatalogSlot {
            slot: handle.slot,
            first_kind: previous.as_str(),
            second_kind: kind.as_str(),
        });
    }
    Ok(())
}

fn validate_submission(
    catalog: &CommandCatalog,
    submission: &FeatureFrameSubmission,
    limits: CommandFrameLimits,
) -> Result<ValidatedFeatureTrace, CommandValidationError> {
    if submission.plan_generation != catalog.plan_generation {
        return Err(CommandValidationError::PlanGenerationMismatch {
            expected: catalog.plan_generation,
            actual: submission.plan_generation,
        });
    }
    let list_count = u32::try_from(submission.lists.len()).map_err(|_| {
        CommandValidationError::CountConversion {
            counter: "command-lists",
        }
    })?;
    if list_count > limits.command_lists {
        return Err(CommandValidationError::ListQuota {
            actual: list_count,
            maximum: limits.command_lists,
        });
    }

    let (commits, upload_bytes) = validate_commits(catalog, submission, limits)?;
    let mut encoded_bytes = 0_u64;
    let mut dispatches = 0_u32;
    let mut workgroups = 0_u64;
    let mut fullscreen_draws = 0_u32;
    let mut traces = Vec::with_capacity(submission.lists.len());
    for (list_index, list) in submission.lists.iter().enumerate() {
        let command_count = u32::try_from(list.commands.len()).map_err(|_| {
            CommandValidationError::CountConversion {
                counter: "commands-per-list",
            }
        })?;
        if command_count > limits.commands_per_list {
            return Err(CommandValidationError::CommandQuota {
                list: list_index,
                actual: command_count,
                maximum: limits.commands_per_list,
            });
        }
        let minimum = list.commands.iter().try_fold(8_u64, |total, command| {
            total.checked_add(command.minimum_encoded_bytes()).ok_or(
                CommandValidationError::CounterOverflow {
                    counter: "minimum-encoded-bytes",
                },
            )
        })?;
        if u64::from(list.encoded_bytes) < minimum {
            return Err(CommandValidationError::EncodedLengthTooSmall {
                list: list_index,
                actual: u64::from(list.encoded_bytes),
                minimum,
            });
        }
        encoded_bytes = encoded_bytes
            .checked_add(u64::from(list.encoded_bytes))
            .ok_or(CommandValidationError::CounterOverflow {
                counter: "encoded-command-bytes",
            })?;
        if encoded_bytes > limits.encoded_command_bytes {
            return Err(CommandValidationError::EncodedByteQuota {
                actual: encoded_bytes,
                maximum: limits.encoded_command_bytes,
            });
        }

        let mut state = ListState::default();
        let mut commands = Vec::with_capacity(list.commands.len());
        for (command_index, command) in list.commands.iter().enumerate() {
            let trace = validate_command(
                catalog,
                submission,
                list,
                command,
                &commits,
                &mut state,
                &mut dispatches,
                &mut workgroups,
                &mut fullscreen_draws,
                limits,
            )
            .map_err(|source| CommandValidationError::CommandFault {
                list: list_index,
                command: command_index,
                source: Box::new(source),
            })?;
            commands.push(trace);
        }
        traces.push(ValidatedListTrace {
            slot: list.slot,
            commands,
        });
    }
    Ok(ValidatedFeatureTrace {
        owner: submission.owner,
        frame: submission.frame,
        view: submission.view,
        lists: traces,
        encoded_bytes,
        upload_bytes,
        workgroups,
    })
}

fn validate_commits(
    catalog: &CommandCatalog,
    submission: &FeatureFrameSubmission,
    limits: CommandFrameLimits,
) -> Result<(BTreeMap<CommandHandle, UploadCommit>, u64), CommandValidationError> {
    let mut commits = BTreeMap::new();
    let mut upload_bytes = 0_u64;
    for commit in &submission.upload_commits {
        let allocation = lookup_upload_allocation(catalog, commit.allocation)?;
        validate_upload_scope(allocation, submission)?;
        if commit.length == 0 {
            return Err(CommandValidationError::ZeroUploadLength {
                handle: commit.allocation,
            });
        }
        if commit.offset % allocation.alignment != 0 {
            return Err(CommandValidationError::UploadAlignment {
                handle: commit.allocation,
                offset: commit.offset,
                required: allocation.alignment,
            });
        }
        let end = checked_range_end(commit.offset, commit.length, commit.allocation)?;
        if end > allocation.byte_capacity {
            return Err(CommandValidationError::UploadOutOfBounds {
                handle: commit.allocation,
                end,
                capacity: allocation.byte_capacity,
            });
        }
        if commits.insert(commit.allocation, *commit).is_some() {
            return Err(CommandValidationError::DuplicateUploadCommit {
                handle: commit.allocation,
            });
        }
        upload_bytes = upload_bytes.checked_add(commit.length).ok_or(
            CommandValidationError::CounterOverflow {
                counter: "committed-upload-bytes",
            },
        )?;
        if upload_bytes > limits.committed_upload_bytes {
            return Err(CommandValidationError::UploadQuota {
                actual: upload_bytes,
                maximum: limits.committed_upload_bytes,
            });
        }
    }
    Ok((commits, upload_bytes))
}

#[derive(Default)]
struct ListState {
    pipeline: Option<CommandHandle>,
    bound_resource_sets: BTreeSet<CommandHandle>,
    parameter_bound: bool,
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the six-opcode state machine is kept together to make atomic validation auditable"
)]
fn validate_command(
    catalog: &CommandCatalog,
    submission: &FeatureFrameSubmission,
    list: &CommandList,
    command: &Command,
    commits: &BTreeMap<CommandHandle, UploadCommit>,
    state: &mut ListState,
    dispatches: &mut u32,
    workgroups: &mut u64,
    fullscreen_draws: &mut u32,
    limits: CommandFrameLimits,
) -> Result<ValidatedTraceCommand, CommandValidationError> {
    match command {
        Command::SelectDeclaredPipeline { pipeline } => {
            let declared = lookup_pipeline(catalog, *pipeline)?;
            validate_owner_and_slot(
                submission.owner,
                list.slot,
                declared.owner,
                declared.slot,
                *pipeline,
            )?;
            state.pipeline = Some(*pipeline);
            state.bound_resource_sets.clear();
            state.parameter_bound = false;
            Ok(ValidatedTraceCommand::SelectDeclaredPipeline {
                pipeline: declared.id.clone(),
            })
        }
        Command::BindDeclaredResourceSet { resource_set } => {
            let pipeline = selected_pipeline(catalog, state)?;
            let declared = lookup_resource_set(catalog, *resource_set)?;
            validate_owner_and_slot(
                submission.owner,
                list.slot,
                declared.owner,
                declared.slot,
                *resource_set,
            )?;
            if !pipeline.required_resource_sets.contains(resource_set) {
                return Err(CommandValidationError::IncompatibleResourceSet {
                    pipeline: pipeline.id.clone(),
                    resource_set: Box::new(declared.id.clone()),
                });
            }
            state.bound_resource_sets.insert(*resource_set);
            Ok(ValidatedTraceCommand::BindDeclaredResourceSet {
                resource_set: declared.id.clone(),
            })
        }
        Command::SetParameterBlock { layout, upload } => {
            let pipeline = selected_pipeline(catalog, state)?;
            let declared = lookup_parameter_layout(catalog, *layout)?;
            validate_owner_and_slot(
                submission.owner,
                list.slot,
                declared.owner,
                declared.slot,
                *layout,
            )?;
            if pipeline.parameter_layout != Some(*layout) {
                return Err(CommandValidationError::IncompatibleParameterLayout {
                    pipeline: pipeline.id.clone(),
                    layout: Box::new(declared.id.clone()),
                });
            }
            validate_parameter_range(catalog, submission, declared, *upload, commits)?;
            state.parameter_bound = true;
            Ok(ValidatedTraceCommand::SetParameterBlock {
                layout: declared.id.clone(),
                byte_length: upload.length,
            })
        }
        Command::Dispatch { x, y, z } => {
            let pipeline = selected_pipeline(catalog, state)?;
            if pipeline.kind != PipelineKind::Compute {
                return Err(CommandValidationError::WrongPipelineKind {
                    pipeline: pipeline.id.clone(),
                    expected: PipelineKind::Compute,
                    actual: pipeline.kind,
                });
            }
            validate_required_bindings(pipeline, state)?;
            let dimensions = [*x, *y, *z];
            if dimensions.contains(&0) {
                return Err(CommandValidationError::ZeroWorkgroupDimension);
            }
            for (axis, (actual, maximum)) in dimensions
                .into_iter()
                .zip(catalog.max_workgroups_per_dimension)
                .enumerate()
            {
                if actual > maximum {
                    return Err(CommandValidationError::WorkgroupDimensionLimit {
                        axis,
                        actual,
                        maximum,
                    });
                }
            }
            let command_workgroups = u64::from(*x)
                .checked_mul(u64::from(*y))
                .and_then(|value| value.checked_mul(u64::from(*z)))
                .ok_or(CommandValidationError::CounterOverflow {
                    counter: "dispatch-workgroups",
                })?;
            *workgroups = workgroups.checked_add(command_workgroups).ok_or(
                CommandValidationError::CounterOverflow {
                    counter: "cumulative-workgroups",
                },
            )?;
            if *workgroups > limits.cumulative_workgroups {
                return Err(CommandValidationError::WorkgroupQuota {
                    actual: *workgroups,
                    maximum: limits.cumulative_workgroups,
                });
            }
            *dispatches =
                dispatches
                    .checked_add(1)
                    .ok_or(CommandValidationError::CounterOverflow {
                        counter: "dispatch-commands",
                    })?;
            if *dispatches > limits.dispatch_commands {
                return Err(CommandValidationError::DispatchQuota {
                    actual: *dispatches,
                    maximum: limits.dispatch_commands,
                });
            }
            Ok(ValidatedTraceCommand::Dispatch {
                x: *x,
                y: *y,
                z: *z,
            })
        }
        Command::DrawFullscreenTriangle {} => {
            let pipeline = selected_pipeline(catalog, state)?;
            if pipeline.kind != PipelineKind::FullscreenTriangle {
                return Err(CommandValidationError::WrongPipelineKind {
                    pipeline: pipeline.id.clone(),
                    expected: PipelineKind::FullscreenTriangle,
                    actual: pipeline.kind,
                });
            }
            validate_required_bindings(pipeline, state)?;
            *fullscreen_draws =
                fullscreen_draws
                    .checked_add(1)
                    .ok_or(CommandValidationError::CounterOverflow {
                        counter: "fullscreen-draws",
                    })?;
            if *fullscreen_draws > limits.fullscreen_draws {
                return Err(CommandValidationError::FullscreenDrawQuota {
                    actual: *fullscreen_draws,
                    maximum: limits.fullscreen_draws,
                });
            }
            Ok(ValidatedTraceCommand::DrawFullscreenTriangle {})
        }
        Command::DebugMarker { marker } => {
            let declared = lookup_marker(catalog, *marker)?;
            validate_owner_and_slot(
                submission.owner,
                list.slot,
                declared.owner,
                declared.slot,
                *marker,
            )?;
            Ok(ValidatedTraceCommand::DebugMarker {
                marker: declared.id.clone(),
            })
        }
    }
}

fn selected_pipeline<'a>(
    catalog: &'a CommandCatalog,
    state: &ListState,
) -> Result<&'a DeclaredPipeline, CommandValidationError> {
    let handle = state
        .pipeline
        .ok_or(CommandValidationError::PipelineNotSelected)?;
    lookup_pipeline(catalog, handle)
}

fn validate_required_bindings(
    pipeline: &DeclaredPipeline,
    state: &ListState,
) -> Result<(), CommandValidationError> {
    if let Some(missing) = pipeline
        .required_resource_sets
        .difference(&state.bound_resource_sets)
        .next()
    {
        return Err(CommandValidationError::RequiredResourceSetMissing {
            pipeline: pipeline.id.clone(),
            handle: *missing,
        });
    }
    if pipeline.parameter_layout.is_some() && !state.parameter_bound {
        return Err(CommandValidationError::RequiredParameterBlockMissing {
            pipeline: pipeline.id.clone(),
        });
    }
    Ok(())
}

fn validate_parameter_range(
    catalog: &CommandCatalog,
    submission: &FeatureFrameSubmission,
    layout: &DeclaredParameterLayout,
    range: UploadRange,
    commits: &BTreeMap<CommandHandle, UploadCommit>,
) -> Result<(), CommandValidationError> {
    let allocation = lookup_upload_allocation(catalog, range.allocation)?;
    validate_upload_scope(allocation, submission)?;
    if range.length != layout.byte_size {
        return Err(CommandValidationError::ParameterSize {
            layout: layout.id.clone(),
            actual: range.length,
            expected: layout.byte_size,
        });
    }
    if !range.offset.is_multiple_of(layout.alignment) || allocation.alignment < layout.alignment {
        return Err(CommandValidationError::ParameterAlignment {
            layout: layout.id.clone(),
            offset: range.offset,
            required: layout.alignment,
            available: allocation.alignment,
        });
    }
    let range_end = checked_range_end(range.offset, range.length, range.allocation)?;
    if range_end > allocation.byte_capacity {
        return Err(CommandValidationError::UploadOutOfBounds {
            handle: range.allocation,
            end: range_end,
            capacity: allocation.byte_capacity,
        });
    }
    let commit =
        commits
            .get(&range.allocation)
            .ok_or(CommandValidationError::UploadNotCommitted {
                handle: range.allocation,
            })?;
    let commit_end = checked_range_end(commit.offset, commit.length, commit.allocation)?;
    if range.offset < commit.offset || range_end > commit_end {
        return Err(CommandValidationError::RangeOutsideCommit {
            handle: range.allocation,
        });
    }
    Ok(())
}

fn validate_upload_scope(
    allocation: &DeclaredUploadAllocation,
    submission: &FeatureFrameSubmission,
) -> Result<(), CommandValidationError> {
    if allocation.owner != submission.owner {
        return Err(CommandValidationError::ForeignOwner {
            handle: allocation.handle,
            expected: submission.owner,
            actual: allocation.owner,
        });
    }
    if allocation.frame != submission.frame
        || allocation.view != submission.view
        || allocation.callback_epoch != submission.callback_epoch
    {
        return Err(CommandValidationError::ExpiredUploadScope {
            handle: allocation.handle,
        });
    }
    Ok(())
}

fn checked_range_end(
    offset: u64,
    length: u64,
    handle: CommandHandle,
) -> Result<u64, CommandValidationError> {
    offset
        .checked_add(length)
        .ok_or(CommandValidationError::RangeOverflow { handle })
}

const fn valid_alignment(value: u64) -> bool {
    value != 0 && value.is_power_of_two()
}

fn validate_owner_and_slot(
    expected_owner: CommandOwnerId,
    expected_slot: Core3dSlotV1,
    actual_owner: CommandOwnerId,
    actual_slot: Core3dSlotV1,
    handle: CommandHandle,
) -> Result<(), CommandValidationError> {
    if actual_owner != expected_owner {
        return Err(CommandValidationError::ForeignOwner {
            handle,
            expected: expected_owner,
            actual: actual_owner,
        });
    }
    if actual_slot != expected_slot {
        return Err(CommandValidationError::WrongSemanticSlot {
            handle,
            expected: expected_slot,
            actual: actual_slot,
        });
    }
    Ok(())
}

fn lookup_pipeline(
    catalog: &CommandCatalog,
    handle: CommandHandle,
) -> Result<&DeclaredPipeline, CommandValidationError> {
    let row = catalog
        .pipelines
        .iter()
        .find(|entry| entry.handle.slot == handle.slot)
        .ok_or_else(|| unknown_handle(catalog, handle, HandleKind::Pipeline))?;
    validate_handle(handle, row.handle, HandleKind::Pipeline)?;
    Ok(row)
}

fn lookup_resource_set(
    catalog: &CommandCatalog,
    handle: CommandHandle,
) -> Result<&DeclaredResourceSet, CommandValidationError> {
    let row = catalog
        .resource_sets
        .iter()
        .find(|entry| entry.handle.slot == handle.slot)
        .ok_or_else(|| unknown_handle(catalog, handle, HandleKind::ResourceSet))?;
    validate_handle(handle, row.handle, HandleKind::ResourceSet)?;
    Ok(row)
}

fn lookup_parameter_layout(
    catalog: &CommandCatalog,
    handle: CommandHandle,
) -> Result<&DeclaredParameterLayout, CommandValidationError> {
    let row = catalog
        .parameter_layouts
        .iter()
        .find(|entry| entry.handle.slot == handle.slot)
        .ok_or_else(|| unknown_handle(catalog, handle, HandleKind::ParameterLayout))?;
    validate_handle(handle, row.handle, HandleKind::ParameterLayout)?;
    Ok(row)
}

fn lookup_upload_allocation(
    catalog: &CommandCatalog,
    handle: CommandHandle,
) -> Result<&DeclaredUploadAllocation, CommandValidationError> {
    let row = catalog
        .upload_allocations
        .iter()
        .find(|entry| entry.handle.slot == handle.slot)
        .ok_or_else(|| unknown_handle(catalog, handle, HandleKind::UploadAllocation))?;
    validate_handle(handle, row.handle, HandleKind::UploadAllocation)?;
    Ok(row)
}

fn lookup_marker(
    catalog: &CommandCatalog,
    handle: CommandHandle,
) -> Result<&DeclaredMarker, CommandValidationError> {
    let row = catalog
        .markers
        .iter()
        .find(|entry| entry.handle.slot == handle.slot)
        .ok_or_else(|| unknown_handle(catalog, handle, HandleKind::Marker))?;
    validate_handle(handle, row.handle, HandleKind::Marker)?;
    Ok(row)
}

fn validate_handle(
    actual: CommandHandle,
    expected: CommandHandle,
    kind: HandleKind,
) -> Result<(), CommandValidationError> {
    if actual.table_id == 0 || actual.generation.0 == 0 {
        return Err(CommandValidationError::NullHandle {
            kind: kind.as_str(),
        });
    }
    if actual.generation.0 == u32::MAX {
        return Err(CommandValidationError::RetiredHandle {
            kind: kind.as_str(),
            handle: actual,
        });
    }
    if actual.table_id != expected.table_id {
        return Err(CommandValidationError::ForeignTable {
            kind: kind.as_str(),
            expected: expected.table_id,
            actual: actual.table_id,
        });
    }
    if actual.generation != expected.generation {
        return Err(CommandValidationError::StaleGeneration {
            kind: kind.as_str(),
            handle: actual,
            expected: expected.generation,
        });
    }
    Ok(())
}

fn unknown_handle(
    catalog: &CommandCatalog,
    handle: CommandHandle,
    expected: HandleKind,
) -> CommandValidationError {
    if let Some(actual) = kind_at_slot(catalog, handle.slot) {
        CommandValidationError::WrongHandleKind {
            slot: handle.slot,
            expected: expected.as_str(),
            actual: actual.as_str(),
        }
    } else {
        CommandValidationError::UnknownHandle {
            kind: expected.as_str(),
            handle,
        }
    }
}

fn kind_at_slot(catalog: &CommandCatalog, slot: HandleIndex) -> Option<HandleKind> {
    if catalog
        .pipelines
        .iter()
        .any(|entry| entry.handle.slot == slot)
    {
        Some(HandleKind::Pipeline)
    } else if catalog
        .resource_sets
        .iter()
        .any(|entry| entry.handle.slot == slot)
    {
        Some(HandleKind::ResourceSet)
    } else if catalog
        .parameter_layouts
        .iter()
        .any(|entry| entry.handle.slot == slot)
    {
        Some(HandleKind::ParameterLayout)
    } else if catalog
        .upload_allocations
        .iter()
        .any(|entry| entry.handle.slot == slot)
    {
        Some(HandleKind::UploadAllocation)
    } else if catalog
        .markers
        .iter()
        .any(|entry| entry.handle.slot == slot)
    {
        Some(HandleKind::Marker)
    } else {
        None
    }
}

/// Atomic command frame validation failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CommandValidationError {
    /// Catalog table identity is zero.
    #[error("command catalog has an invalid zero table ID")]
    InvalidCatalogTable,
    /// A per-axis device dispatch limit is zero.
    #[error("command catalog has an invalid zero device workgroup limit")]
    InvalidDeviceLimit,
    /// A catalog row has a foreign, zero, or retired handle.
    #[error("catalog {kind} has invalid handle {handle:?}")]
    InvalidCatalogHandle {
        /// Stable handle kind.
        kind: &'static str,
        /// Invalid handle.
        handle: CommandHandle,
    },
    /// Two typed rows occupy the same raw table slot.
    #[error("catalog slot {slot:?} is both {first_kind} and {second_kind}")]
    DuplicateCatalogSlot {
        /// Conflicted raw slot.
        slot: HandleIndex,
        /// First row kind.
        first_kind: &'static str,
        /// Second row kind.
        second_kind: &'static str,
    },
    /// Parameter layout size or alignment is invalid.
    #[error("parameter layout `{layout}` has invalid size or alignment")]
    InvalidLayout {
        /// Invalid layout.
        layout: StableId,
    },
    /// Upload allocation capacity or alignment is invalid.
    #[error("upload allocation {handle:?} has invalid capacity or alignment")]
    InvalidUploadAllocation {
        /// Invalid allocation handle.
        handle: CommandHandle,
    },
    /// Submission frame does not match its containing frame.
    #[error("submission frame {actual} does not match frame {expected}")]
    FrameMismatch {
        /// Container frame.
        expected: u64,
        /// Submission frame.
        actual: u64,
    },
    /// More than one submission attempts to split one feature/view budget.
    #[error("duplicate submission for owner {owner:?}, view {view:?}")]
    DuplicateFeatureViewSubmission {
        /// Feature owner.
        owner: CommandOwnerId,
        /// Target view.
        view: ViewId,
    },
    /// Submission references an old or future plan generation.
    #[error("plan generation {actual} does not match active {expected}")]
    PlanGenerationMismatch {
        /// Active generation.
        expected: u64,
        /// Submitted generation.
        actual: u64,
    },
    /// A platform-size count cannot fit the contract counter.
    #[error("{counter} count cannot be represented by contract major 1")]
    CountConversion {
        /// Stable counter name.
        counter: &'static str,
    },
    /// Command-list count exceeds the effective quota.
    #[error("command list count {actual} exceeds {maximum}")]
    ListQuota {
        /// Actual count.
        actual: u32,
        /// Effective maximum.
        maximum: u32,
    },
    /// Commands in one list exceed the effective quota.
    #[error("command list {list} count {actual} exceeds {maximum}")]
    CommandQuota {
        /// Zero-based list index.
        list: usize,
        /// Actual count.
        actual: u32,
        /// Effective maximum.
        maximum: u32,
    },
    /// Decoder-measured length is smaller than the structural encoding minimum.
    #[error("command list {list} encoded length {actual} is below minimum {minimum}")]
    EncodedLengthTooSmall {
        /// Zero-based list index.
        list: usize,
        /// Decoder-measured bytes.
        actual: u64,
        /// Structural minimum bytes.
        minimum: u64,
    },
    /// Encoded bytes exceed the effective quota.
    #[error("encoded command bytes {actual} exceed {maximum}")]
    EncodedByteQuota {
        /// Actual bytes.
        actual: u64,
        /// Effective maximum.
        maximum: u64,
    },
    /// One feature/view commits too many upload bytes.
    #[error("committed upload bytes {actual} exceed {maximum}")]
    UploadQuota {
        /// Actual bytes.
        actual: u64,
        /// Effective maximum.
        maximum: u64,
    },
    /// All features together commit too many frame upload bytes.
    #[error("global upload bytes {actual} exceed {maximum}")]
    GlobalUploadQuota {
        /// Actual bytes.
        actual: u64,
        /// Effective maximum.
        maximum: u64,
    },
    /// Dispatch count exceeds the effective quota.
    #[error("dispatch count {actual} exceeds {maximum}")]
    DispatchQuota {
        /// Actual count.
        actual: u32,
        /// Effective maximum.
        maximum: u32,
    },
    /// Cumulative workgroups exceed the effective quota.
    #[error("cumulative workgroups {actual} exceed {maximum}")]
    WorkgroupQuota {
        /// Actual workgroups.
        actual: u64,
        /// Effective maximum.
        maximum: u64,
    },
    /// Fullscreen draw count exceeds the effective quota.
    #[error("fullscreen draw count {actual} exceeds {maximum}")]
    FullscreenDrawQuota {
        /// Actual count.
        actual: u32,
        /// Effective maximum.
        maximum: u32,
    },
    /// Checked counter arithmetic overflowed.
    #[error("checked counter `{counter}` overflowed")]
    CounterOverflow {
        /// Stable counter name.
        counter: &'static str,
    },
    /// One command failed; the entire frame remains rejected.
    #[error("command {command} in list {list} failed: {source}")]
    CommandFault {
        /// Zero-based list index.
        list: usize,
        /// Zero-based command index.
        command: usize,
        /// Bounded structured cause.
        source: Box<Self>,
    },
    /// A null opaque handle was submitted.
    #[error("null {kind} handle")]
    NullHandle {
        /// Expected handle kind.
        kind: &'static str,
    },
    /// A retired generation was submitted.
    #[error("retired {kind} handle {handle:?}")]
    RetiredHandle {
        /// Handle kind.
        kind: &'static str,
        /// Retired handle.
        handle: CommandHandle,
    },
    /// Handle belongs to another table/instance.
    #[error("{kind} handle table {actual} does not match {expected}")]
    ForeignTable {
        /// Handle kind.
        kind: &'static str,
        /// Active table ID.
        expected: u64,
        /// Submitted table ID.
        actual: u64,
    },
    /// Handle generation is stale or forged.
    #[error("stale {kind} handle {handle:?}; current generation is {expected:?}")]
    StaleGeneration {
        /// Handle kind.
        kind: &'static str,
        /// Submitted handle.
        handle: CommandHandle,
        /// Current generation.
        expected: HandleGeneration,
    },
    /// Slot exists but has another typed handle kind.
    #[error("slot {slot:?} is {actual}, not {expected}")]
    WrongHandleKind {
        /// Raw slot.
        slot: HandleIndex,
        /// Expected kind.
        expected: &'static str,
        /// Actual kind.
        actual: &'static str,
    },
    /// Slot is absent from the active catalog.
    #[error("unknown {kind} handle {handle:?}")]
    UnknownHandle {
        /// Expected kind.
        kind: &'static str,
        /// Unknown handle.
        handle: CommandHandle,
    },
    /// Handle is owned by another feature.
    #[error("handle {handle:?} owner {actual:?} does not match {expected:?}")]
    ForeignOwner {
        /// Foreign handle.
        handle: CommandHandle,
        /// Submission owner.
        expected: CommandOwnerId,
        /// Catalog owner.
        actual: CommandOwnerId,
    },
    /// Handle cannot be used in this semantic slot.
    #[error("handle {handle:?} slot {actual} does not match {expected}")]
    WrongSemanticSlot {
        /// Mis-scoped handle.
        handle: CommandHandle,
        /// Command-list slot.
        expected: Core3dSlotV1,
        /// Catalog slot.
        actual: Core3dSlotV1,
    },
    /// Upload allocation belongs to another frame, view, or callback.
    #[error("upload allocation {handle:?} is outside its callback scope")]
    ExpiredUploadScope {
        /// Expired allocation.
        handle: CommandHandle,
    },
    /// Upload commit has zero length.
    #[error("upload allocation {handle:?} has a zero-length commit")]
    ZeroUploadLength {
        /// Allocation handle.
        handle: CommandHandle,
    },
    /// Upload range is misaligned.
    #[error("upload {handle:?} offset {offset} is not aligned to {required}")]
    UploadAlignment {
        /// Allocation handle.
        handle: CommandHandle,
        /// Rejected offset.
        offset: u64,
        /// Required alignment.
        required: u64,
    },
    /// Range end overflowed checked arithmetic.
    #[error("upload range overflow for {handle:?}")]
    RangeOverflow {
        /// Allocation handle.
        handle: CommandHandle,
    },
    /// Upload range exceeds allocation capacity.
    #[error("upload {handle:?} end {end} exceeds capacity {capacity}")]
    UploadOutOfBounds {
        /// Allocation handle.
        handle: CommandHandle,
        /// Checked range end.
        end: u64,
        /// Allocation capacity.
        capacity: u64,
    },
    /// An allocation was committed more than once to bypass accounting.
    #[error("duplicate upload commit for {handle:?}")]
    DuplicateUploadCommit {
        /// Allocation handle.
        handle: CommandHandle,
    },
    /// Parameter command references an allocation that was never committed.
    #[error("upload allocation {handle:?} was not committed")]
    UploadNotCommitted {
        /// Allocation handle.
        handle: CommandHandle,
    },
    /// Parameter range lies outside the atomically committed range.
    #[error("parameter range is outside upload commit {handle:?}")]
    RangeOutsideCommit {
        /// Allocation handle.
        handle: CommandHandle,
    },
    /// Parameter range length differs from the declared exact layout size.
    #[error("parameter layout `{layout}` size {actual} does not match {expected}")]
    ParameterSize {
        /// Declared layout.
        layout: StableId,
        /// Submitted bytes.
        actual: u64,
        /// Exact expected bytes.
        expected: u64,
    },
    /// Parameter offset or upload base alignment is insufficient.
    #[error(
        "parameter layout `{layout}` offset {offset} needs alignment {required}, allocation provides {available}"
    )]
    ParameterAlignment {
        /// Declared layout.
        layout: StableId,
        /// Submitted offset.
        offset: u64,
        /// Required alignment.
        required: u64,
        /// Allocation base alignment.
        available: u64,
    },
    /// Pipeline must be selected before bind, set, dispatch, or draw.
    #[error("no pipeline selected")]
    PipelineNotSelected,
    /// Resource set is not declared compatible with the selected pipeline.
    #[error("resource set `{resource_set}` is incompatible with pipeline `{pipeline}`")]
    IncompatibleResourceSet {
        /// Selected pipeline.
        pipeline: StableId,
        /// Submitted resource set.
        resource_set: Box<StableId>,
    },
    /// Parameter layout is not declared compatible with the selected pipeline.
    #[error("parameter layout `{layout}` is incompatible with pipeline `{pipeline}`")]
    IncompatibleParameterLayout {
        /// Selected pipeline.
        pipeline: StableId,
        /// Submitted layout.
        layout: Box<StableId>,
    },
    /// A required resource set was not bound before execution.
    #[error("pipeline `{pipeline}` is missing required resource-set handle {handle:?}")]
    RequiredResourceSetMissing {
        /// Selected pipeline.
        pipeline: StableId,
        /// Missing set handle.
        handle: CommandHandle,
    },
    /// A required parameter block was not bound before execution.
    #[error("pipeline `{pipeline}` is missing its required parameter block")]
    RequiredParameterBlockMissing {
        /// Selected pipeline.
        pipeline: StableId,
    },
    /// Selected pipeline kind cannot execute this command.
    #[error("pipeline `{pipeline}` kind {actual:?} does not match {expected:?}")]
    WrongPipelineKind {
        /// Selected pipeline.
        pipeline: StableId,
        /// Required pipeline kind.
        expected: PipelineKind,
        /// Actual pipeline kind.
        actual: PipelineKind,
    },
    /// Dispatch contains a zero workgroup dimension.
    #[error("dispatch workgroup dimensions must be non-zero")]
    ZeroWorkgroupDimension,
    /// One dispatch dimension exceeds the device-advertised limit.
    #[error("dispatch axis {axis} value {actual} exceeds device limit {maximum}")]
    WorkgroupDimensionLimit {
        /// Zero-based axis.
        axis: usize,
        /// Submitted count.
        actual: u32,
        /// Device limit.
        maximum: u32,
    },
}
