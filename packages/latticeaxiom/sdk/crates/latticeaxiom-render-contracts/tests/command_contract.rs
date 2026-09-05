//! Portable command validation contract tests.

use std::collections::BTreeSet;

use latticeaxiom_core::StableId;
use latticeaxiom_render_contracts::{
    Command, CommandCatalog, CommandFrame, CommandFrameLimits, CommandHandle, CommandList,
    CommandOwnerId, CommandValidationError, Core3dSlotV1, DeclaredMarker, DeclaredParameterLayout,
    DeclaredPipeline, DeclaredResourceSet, DeclaredUploadAllocation, FeatureFrameSubmission,
    FrameCommandValidator, HandleGeneration, HandleIndex, PipelineKind, UploadCommit, UploadRange,
    ValidatedTraceCommand, ViewId,
};

const TABLE: u64 = 77;
const PLAN_GENERATION: u64 = 9;
const FRAME: u64 = 42;
const CALLBACK: u64 = 3;
const OWNER: CommandOwnerId = CommandOwnerId(7);
const VIEW: ViewId = ViewId(1);

fn id(value: &str) -> StableId {
    match value.parse() {
        Ok(value) => value,
        Err(error) => panic!("test StableId must be valid: {error}"),
    }
}

const fn handle(slot: u32) -> CommandHandle {
    CommandHandle {
        table_id: TABLE,
        slot: HandleIndex(slot),
        generation: HandleGeneration(1),
    }
}

fn catalog() -> CommandCatalog {
    CommandCatalog {
        table_id: TABLE,
        plan_generation: PLAN_GENERATION,
        pipelines: vec![
            DeclaredPipeline {
                handle: handle(1),
                id: id("demo:render-pipeline/outline-compute@1"),
                owner: OWNER,
                slot: Core3dSlotV1::BeforeTonemap,
                kind: PipelineKind::Compute,
                required_resource_sets: BTreeSet::from([handle(2)]),
                parameter_layout: Some(handle(3)),
            },
            DeclaredPipeline {
                handle: handle(6),
                id: id("demo:render-pipeline/outline-composite@1"),
                owner: OWNER,
                slot: Core3dSlotV1::BeforeTonemap,
                kind: PipelineKind::FullscreenTriangle,
                required_resource_sets: BTreeSet::new(),
                parameter_layout: None,
            },
        ],
        resource_sets: vec![DeclaredResourceSet {
            handle: handle(2),
            id: id("demo:render-resource-set/outline@1"),
            owner: OWNER,
            slot: Core3dSlotV1::BeforeTonemap,
        }],
        parameter_layouts: vec![DeclaredParameterLayout {
            handle: handle(3),
            id: id("demo:render-parameter-layout/outline@1"),
            owner: OWNER,
            slot: Core3dSlotV1::BeforeTonemap,
            byte_size: 64,
            alignment: 16,
        }],
        upload_allocations: vec![DeclaredUploadAllocation {
            handle: handle(4),
            owner: OWNER,
            frame: FRAME,
            view: VIEW,
            callback_epoch: CALLBACK,
            byte_capacity: 64,
            alignment: 16,
        }],
        markers: vec![DeclaredMarker {
            handle: handle(5),
            id: id("demo:render-marker/outline-dispatch@1"),
            owner: OWNER,
            slot: Core3dSlotV1::BeforeTonemap,
        }],
        max_workgroups_per_dimension: [1024, 1024, 64],
    }
}

fn valid_commands() -> Vec<Command> {
    vec![
        Command::SelectDeclaredPipeline {
            pipeline: handle(1),
        },
        Command::BindDeclaredResourceSet {
            resource_set: handle(2),
        },
        Command::SetParameterBlock {
            layout: handle(3),
            upload: UploadRange {
                allocation: handle(4),
                offset: 0,
                length: 64,
            },
        },
        Command::Dispatch { x: 4, y: 4, z: 1 },
        Command::DebugMarker { marker: handle(5) },
    ]
}

fn submission(commands: Vec<Command>) -> FeatureFrameSubmission {
    FeatureFrameSubmission {
        owner: OWNER,
        plan_generation: PLAN_GENERATION,
        frame: FRAME,
        view: VIEW,
        callback_epoch: CALLBACK,
        lists: vec![CommandList {
            slot: Core3dSlotV1::BeforeTonemap,
            encoded_bytes: 121,
            commands,
        }],
        upload_commits: vec![UploadCommit {
            allocation: handle(4),
            offset: 0,
            length: 64,
        }],
    }
}

fn frame(submission: FeatureFrameSubmission) -> CommandFrame {
    CommandFrame {
        frame: FRAME,
        submissions: vec![submission],
    }
}

fn root_error(error: &CommandValidationError) -> &CommandValidationError {
    match error {
        CommandValidationError::CommandFault { source, .. } => root_error(source),
        other => other,
    }
}

#[test]
fn valid_outline_trace_is_handle_free_and_atomic() {
    let validated = match FrameCommandValidator::default()
        .validate(&catalog(), &frame(submission(valid_commands())))
    {
        Ok(value) => value,
        Err(error) => panic!("valid command frame must pass: {error}"),
    };
    assert_eq!(validated.global_upload_bytes, 64);
    assert_eq!(validated.features[0].workgroups, 16);
    assert!(matches!(
        &validated.features[0].lists[0].commands[0],
        ValidatedTraceCommand::SelectDeclaredPipeline { pipeline }
            if pipeline.as_str() == "demo:render-pipeline/outline-compute@1"
    ));
}

#[test]
fn handle_ownership_generation_kind_and_scope_faults_are_rejected() {
    let mut stale = submission(valid_commands());
    if let Command::SelectDeclaredPipeline { pipeline } = &mut stale.lists[0].commands[0] {
        pipeline.generation = HandleGeneration(2);
    }
    let error = match FrameCommandValidator::default().validate(&catalog(), &frame(stale)) {
        Ok(value) => panic!("stale handle must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::StaleGeneration { .. }
    ));

    let mut wrong_kind = submission(valid_commands());
    wrong_kind.lists[0].commands[0] = Command::SelectDeclaredPipeline {
        pipeline: handle(2),
    };
    let error = match FrameCommandValidator::default().validate(&catalog(), &frame(wrong_kind)) {
        Ok(value) => panic!("wrong handle kind must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::WrongHandleKind { .. }
    ));

    let mut foreign = submission(valid_commands());
    foreign.owner = CommandOwnerId(99);
    let error = match FrameCommandValidator::default().validate(&catalog(), &frame(foreign)) {
        Ok(value) => panic!("foreign owner must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::ForeignOwner { .. }
    ));

    let mut expired_catalog = catalog();
    expired_catalog.upload_allocations[0].callback_epoch += 1;
    let error = match FrameCommandValidator::default()
        .validate(&expired_catalog, &frame(submission(valid_commands())))
    {
        Ok(value) => panic!("expired callback allocation must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::ExpiredUploadScope { .. }
    ));
}

#[test]
fn upload_range_alignment_overflow_and_commit_fault_corpus_is_rejected() {
    let mut misaligned = submission(valid_commands());
    misaligned.upload_commits[0].offset = 1;
    let error = match FrameCommandValidator::default().validate(&catalog(), &frame(misaligned)) {
        Ok(value) => panic!("misaligned commit must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::UploadAlignment { .. }
    ));

    let mut overflow = submission(valid_commands());
    overflow.upload_commits[0].offset = u64::MAX - 31;
    overflow.upload_commits[0].length = 64;
    let error = match FrameCommandValidator::default().validate(&catalog(), &frame(overflow)) {
        Ok(value) => panic!("overflowing commit must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::RangeOverflow { .. }
    ));

    let mut uncommitted = submission(valid_commands());
    uncommitted.upload_commits.clear();
    let error = match FrameCommandValidator::default().validate(&catalog(), &frame(uncommitted)) {
        Ok(value) => panic!("uncommitted parameter range must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::UploadNotCommitted { .. }
    ));

    let mut duplicate = submission(valid_commands());
    duplicate.upload_commits.push(duplicate.upload_commits[0]);
    assert!(matches!(
        FrameCommandValidator::default().validate(&catalog(), &frame(duplicate)),
        Err(CommandValidationError::DuplicateUploadCommit { .. })
    ));
}

#[test]
fn pipeline_state_and_second_select_cannot_reuse_old_bindings() {
    let no_select = submission(vec![Command::Dispatch { x: 1, y: 1, z: 1 }]);
    let error = match FrameCommandValidator::default().validate(&catalog(), &frame(no_select)) {
        Ok(value) => panic!("dispatch without pipeline must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::PipelineNotSelected
    ));

    let mut commands = valid_commands();
    let _ = commands.pop();
    commands.push(Command::SelectDeclaredPipeline {
        pipeline: handle(1),
    });
    commands.push(Command::Dispatch { x: 1, y: 1, z: 1 });
    let mut reset = submission(commands);
    reset.lists[0].encoded_bytes = 155;
    let error = match FrameCommandValidator::default().validate(&catalog(), &frame(reset)) {
        Ok(value) => panic!("second select must clear bindings, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::RequiredResourceSetMissing { .. }
    ));
}

#[test]
fn every_d5_quota_has_boundary_plus_one_rejection() {
    let mut five_lists = submission(Vec::new());
    five_lists.upload_commits.clear();
    five_lists.lists = (0..5)
        .map(|_| CommandList {
            slot: Core3dSlotV1::BeforeTonemap,
            encoded_bytes: 8,
            commands: Vec::new(),
        })
        .collect();
    assert!(matches!(
        FrameCommandValidator::default().validate(&catalog(), &frame(five_lists)),
        Err(CommandValidationError::ListQuota { .. })
    ));

    let mut too_many_commands = submission(Vec::new());
    too_many_commands.upload_commits.clear();
    too_many_commands.lists[0].commands = (0..257)
        .map(|_| Command::DebugMarker { marker: handle(5) })
        .collect();
    too_many_commands.lists[0].encoded_bytes = 4377;
    assert!(matches!(
        FrameCommandValidator::default().validate(&catalog(), &frame(too_many_commands)),
        Err(CommandValidationError::CommandQuota { .. })
    ));

    let mut encoded = submission(Vec::new());
    encoded.upload_commits.clear();
    encoded.lists[0].encoded_bytes = 65_537;
    assert!(matches!(
        FrameCommandValidator::default().validate(&catalog(), &frame(encoded)),
        Err(CommandValidationError::EncodedByteQuota { .. })
    ));

    let mut upload_catalog = catalog();
    upload_catalog.upload_allocations[0].byte_capacity = 4 * 1024 * 1024 + 1;
    let mut upload = submission(Vec::new());
    upload.lists.clear();
    upload.upload_commits[0].length = 4 * 1024 * 1024 + 1;
    assert!(matches!(
        FrameCommandValidator::default().validate(&upload_catalog, &frame(upload)),
        Err(CommandValidationError::UploadQuota { .. })
    ));

    let low = CommandFrameLimits {
        dispatch_commands: 1,
        ..CommandFrameLimits::default()
    };
    let mut two_dispatches = submission(valid_commands());
    two_dispatches.lists[0]
        .commands
        .insert(4, Command::Dispatch { x: 1, y: 1, z: 1 });
    two_dispatches.lists[0].encoded_bytes = 134;
    let error = match FrameCommandValidator::new(low).validate(&catalog(), &frame(two_dispatches)) {
        Ok(value) => panic!("dispatch profile +1 must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::DispatchQuota { .. }
    ));

    let low = CommandFrameLimits {
        cumulative_workgroups: 15,
        ..CommandFrameLimits::default()
    };
    let error = match FrameCommandValidator::new(low)
        .validate(&catalog(), &frame(submission(valid_commands())))
    {
        Ok(value) => panic!("workgroup profile +1 must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::WorkgroupQuota { .. }
    ));

    let low = CommandFrameLimits {
        fullscreen_draws: 0,
        ..CommandFrameLimits::default()
    };
    let mut draw = submission(vec![
        Command::SelectDeclaredPipeline {
            pipeline: handle(6),
        },
        Command::DrawFullscreenTriangle {},
    ]);
    draw.upload_commits.clear();
    draw.lists[0].encoded_bytes = 26;
    let error = match FrameCommandValidator::new(low).validate(&catalog(), &frame(draw)) {
        Ok(value) => panic!("draw profile +1 must fail, got {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        root_error(&error),
        CommandValidationError::FullscreenDrawQuota { .. }
    ));
}

#[test]
fn global_upload_ceiling_cannot_be_bypassed_by_multiple_features() {
    let mut catalog = catalog();
    catalog.upload_allocations.push(DeclaredUploadAllocation {
        handle: handle(7),
        owner: CommandOwnerId(8),
        frame: FRAME,
        view: VIEW,
        callback_epoch: CALLBACK,
        byte_capacity: 64,
        alignment: 16,
    });
    let first = FeatureFrameSubmission {
        lists: Vec::new(),
        ..submission(Vec::new())
    };
    let second = FeatureFrameSubmission {
        owner: CommandOwnerId(8),
        plan_generation: PLAN_GENERATION,
        frame: FRAME,
        view: VIEW,
        callback_epoch: CALLBACK,
        lists: Vec::new(),
        upload_commits: vec![UploadCommit {
            allocation: handle(7),
            offset: 0,
            length: 64,
        }],
    };
    let limits = CommandFrameLimits {
        global_upload_bytes: 100,
        ..CommandFrameLimits::default()
    };
    assert!(matches!(
        FrameCommandValidator::new(limits).validate(
            &catalog,
            &CommandFrame {
                frame: FRAME,
                submissions: vec![second, first],
            }
        ),
        Err(CommandValidationError::GlobalUploadQuota { .. })
    ));
}

#[test]
fn unknown_opcode_and_unknown_command_fields_are_not_in_the_vocabulary() {
    let unknown_opcode = r#"{"opcode":"copy-resource","source":1,"target":2}"#;
    assert!(serde_json::from_str::<Command>(unknown_opcode).is_err());

    let unknown_field = r#"{"opcode":"draw-fullscreen-triangle","vertices":6}"#;
    assert!(serde_json::from_str::<Command>(unknown_field).is_err());
}
