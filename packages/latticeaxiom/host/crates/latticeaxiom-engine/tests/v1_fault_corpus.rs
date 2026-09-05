//! V9/D10 fault corpus through public package, schema, ABI, storage, async,
//! device, and shutdown boundaries.
//!
//! Production fail-closed behavior is not weakened. Exact missing host hooks
//! are recorded in `fixtures/faults/corpus.json` for the V9 hardener.
#![allow(clippy::expect_used)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    str::FromStr,
};

use latticeaxiom_abi::{
    AbiContractError, CallbackFault, CallbackPolicy, CallbackRecoveryInput, FailureScope,
    HeaderPolicy, LaxAbiHeader, RecoveryAction, StagedOutputState, WriteCommitMode,
    decide_recovery, validate_header,
};
use latticeaxiom_compose::{PRODUCT_LOCK_FILE_NAME, ProductLockError, reopen_product_lock};
use latticeaxiom_core::{CanonicalHash, CapabilityId, PackageName, SchemaId, StableId, WorldId};
use latticeaxiom_engine::{
    AuthoredContentDisplayCatalogSourcesV1, AuthoredPresentationCatalogSourcesV1,
    ContentDisplayCatalogV1, FluidRevisionStamp, ProductionHostError, SealedWorldWriterHost,
    SealedWriterHostError, admit_fluid_completion, compile_authored_content_display_catalog,
    sealed_activation_binding,
};
use latticeaxiom_launcher::{
    ChildExitKindV1, ChildExitReportDraftV1, ChildExitReportV1, ChildRoleV1,
    DurableWorldRevisionV1, LaunchGeneration, LaunchModelError, ProcessEpoch,
    SettingTransactionRevision, WorldRevision as LauncherWorldRevision,
};
use latticeaxiom_render_contracts::{
    GpuCapabilities, GpuFeatureV1, ProviderFallback, ProviderRealization, ProviderRequirement,
    ProviderSelectionRequest, RenderProviderDecl, select_provider,
};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey,
    ChunkMutation, ChunkRevision, ChunkRevisionExpectation, ContinuationId, DimensionId,
    FaultPoint, MemoryTransactionKernel, PayloadSchemaVersion, PersistentEntityId, TransactionId,
    VersionedPayload, VoxelRevision, WorldRevision, WorldTransaction,
};
use latticeaxiom_voxel_runtime::{
    ApplyByteDeclaration, ColliderFailure, ColliderSafetyState, ColliderSemanticFingerprint,
    CommittedChunkProjection, DerivedKind, DerivedMemoryBudget, DerivedOwner, DerivedPriority,
    DerivedQueueLimits, DerivedRequest, DerivedRequestSet, DispatchOutcome, ExecutorFinish,
    ExecutorOutcome, FixedTick, FluidRuntimeError, MeshSemanticFingerprint, RuntimeGeneration,
    RuntimeLimits, VoxelRuntime, WorkerAbortOutcome, WorkingSetScope, WorldEpoch,
};
use latticeaxiom_world_catalog::{
    DirtyDrainState, DiskSample, GIB, HeaderCodecError, HeadroomInputs, ReconciliationState,
    StorageFailure, WorldHeaderV1, WorldOpenAction, WorldOpenPlan, WorldOpenRisk, WorldOpenStatus,
};
use latticeaxiom_world_db::{
    ActivationPermitV1, AuthoritativeMetadataInputV1, CheckpointId, CheckpointKindV1,
    CheckpointRequestV1, CommitDurabilityV1, DatabaseFaultPointV1, DigestV1, DisplayName,
    FrozenLockReceiptV1, FrozenPackageReceiptV1, PersistedChunkV1, RealizationKindV1,
    SchemaRequirementV1, StorageDurabilityCapabilityV1, StoragePreflightStatusV1, StoreId,
    WorldCommitRequestV1, WorldCreateRequestV1, WorldDbError, WorldReadView,
    WorldRequirementClosureV1, WriterActivationV1,
};
use latticeaxiom_world_wire::{
    FluidPaletteOpenDispositionV1, SOLID_FLUID_PALETTE_SCHEMA_ID_V1, classify_fluid_palette_open,
};
use serde::Deserialize;

const CORPUS_SCHEMA_ID: &str = "latticeaxiom.v1-fault-corpus.v1";
const AUTHORED_BLOCK_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/blocks/data/authored-display-v1.json");
const AUTHORED_TOOL_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/tools/data/authored-display-v1.json");
const AUTHORED_TOOLS_JSON: &str =
    include_str!("../../../../../terrenia/tools/data/authored-tools-v1.json");
const D9_BLOCK_IDS: &str =
    include_str!("../../../../../terrenia/blocks/data/goldens/d9-block-ids.txt");
const FLUID_IDS: &str = include_str!("../../../../../terrenia/blocks/data/goldens/fluid-ids.txt");
const PRESENTATION_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-display-v1.json");
const PRESENTATION_ASSETS_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-assets-v1.json");
const REQUIRED_CASE_IDS: [&str; 15] = [
    "package.missing-lock",
    "package.tampered-engine-coupled",
    "package.corrupt-lock",
    "schema.unknown-fluid-palette",
    "schema.invalid-version-range",
    "abi.wrong-magic",
    "abi.panic-or-exception",
    "abi.process-fault",
    "storage.low-disk",
    "storage.crash-before-publication",
    "storage.corrupt-header",
    "async.stale-fluid-completion",
    "async.executor-panic",
    "device.render-loss-fallback",
    "shutdown.timeout",
];
const REQUIRED_CLASSES: [&str; 7] = [
    "package", "schema", "abi", "storage", "async", "device", "shutdown",
];
const REQUIRED_HOOKS: [&str; 0] = [];
const ABI_HEADER_POLICY: HeaderPolicy = HeaderPolicy {
    expected_major: 0,
    min_minor: 1,
    max_minor: 1,
    minimum_struct_size: 16,
    known_required_flags: 0,
};
const STAGED_OUTPUT: StagedOutputState = StagedOutputState {
    writable_column_bytes: 64,
    command_count: 2,
    message_count: 1,
    validation_passed: true,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultCorpusDocument {
    schema_id: String,
    document_id: String,
    invariants: FaultInvariants,
    missing_hooks: Vec<MissingHook>,
    cases: Vec<FaultCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultInvariants {
    recoverable_world: String,
    world_hash: String,
    fail_closed: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code, reason = "corpus schema keeps hook identity fields")]
struct MissingHook {
    id: String,
    owner: String,
    signature: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultCase {
    id: String,
    class: String,
    description: String,
    public_boundary: String,
    input: Option<String>,
    expected: String,
    writer_must_not_open: bool,
    world_hash_must_be_unchanged: bool,
    recoverable_world_must_survive: bool,
    missing_hook: Option<String>,
}

#[test]
fn corpus_covers_documented_v9_d10_faults_and_names_exact_missing_hooks() {
    let corpus = load_corpus();
    assert_eq!(corpus.schema_id, CORPUS_SCHEMA_ID);
    assert_eq!(corpus.document_id, "delivery.v1-fault-corpus");
    assert!(
        corpus
            .invariants
            .recoverable_world
            .contains("durable recoverable world")
    );
    assert!(corpus.invariants.world_hash.contains("world hash"));
    assert!(corpus.invariants.fail_closed.contains("writer"));

    for class in REQUIRED_CLASSES {
        assert!(
            corpus.cases.iter().any(|case| case.class == class),
            "fault corpus must cover class {class}"
        );
    }
    for required in REQUIRED_CASE_IDS {
        let case = corpus
            .cases
            .iter()
            .find(|case| case.id == required)
            .unwrap_or_else(|| panic!("missing corpus case {required}"));
        assert!(
            !case.public_boundary.is_empty() && !case.description.is_empty(),
            "{required} must name a public boundary and description"
        );
        assert!(
            case.world_hash_must_be_unchanged,
            "{required} must keep the world-hash invariant"
        );
        assert!(
            case.recoverable_world_must_survive,
            "{required} must keep the latest durable world recoverable"
        );
    }

    assert!(
        corpus.missing_hooks.is_empty(),
        "V9 production hooks must be public; the corpus must not leave named gaps"
    );
    let hook_ids = corpus
        .missing_hooks
        .iter()
        .map(|hook| hook.id.as_str())
        .collect::<BTreeSet<_>>();
    for required in REQUIRED_HOOKS {
        assert!(
            hook_ids.contains(required),
            "hardener hook {required} must be named exactly"
        );
    }
    for case in &corpus.cases {
        assert!(
            case.missing_hook.is_none(),
            "{} must not name a missing production hook",
            case.id
        );
    }
}

#[test]
fn package_lock_and_receipt_faults_fail_closed_before_a_writer_opens() {
    let corpus = require_class("package");
    let missing = require_case(&corpus, "package.missing-lock");
    let tampered = require_case(&corpus, "package.tampered-engine-coupled");
    let corrupt = require_case(&corpus, "package.corrupt-lock");
    assert!(missing.writer_must_not_open && tampered.writer_must_not_open);
    assert_eq!(
        missing.public_boundary,
        "latticeaxiom_compose::reopen_product_lock"
    );
    assert_eq!(tampered.expected, "WorldDbError::MissingEngineBuildId");

    let directory = TestDirectory::create();
    let missing_path = directory.0.path().join(PRODUCT_LOCK_FILE_NAME);
    match reopen_product_lock(&missing_path) {
        Err(ProductLockError::MissingLock { path }) => assert_eq!(path, missing_path),
        other => panic!("missing lock must fail closed, got {other:?}"),
    }

    let corrupt_path = directory.0.path().join("corrupt.lock");
    fs::write(
        &corrupt_path,
        load_input(corrupt.input.as_deref().expect("corrupt-lock input")),
    )
    .expect("corrupt lock fixture copies");
    assert!(
        reopen_product_lock(&corrupt_path).is_err(),
        "truncated lock bytes must not reopen as a complete product lock"
    );

    let package = PackageName::from_str("@example/native").expect("canonical package name");
    let error = FrozenPackageReceiptV1::new(
        package.clone(),
        "1.0.0",
        fixture_digest(b"source"),
        Some(fixture_digest(b"artifact")),
        RealizationKindV1::EngineCoupledNative,
        Some("latticeaxiom-abi/0.1".to_owned()),
        None,
    )
    .expect_err("engine-coupled receipts require an engine build identity");
    assert!(
        matches!(
            error,
            WorldDbError::MissingEngineBuildId { package: ref found } if found == &package.to_string()
        ),
        "tampered engine-coupled receipt must fail closed, got {error:?}"
    );
}

#[test]
fn schema_faults_never_open_a_writable_world() {
    let corpus = require_class("schema");
    let unknown = require_case(&corpus, "schema.unknown-fluid-palette");
    let invalid = require_case(&corpus, "schema.invalid-version-range");
    assert!(unknown.writer_must_not_open && invalid.writer_must_not_open);

    let schema_text = String::from_utf8(load_input(
        unknown.input.as_deref().expect("unknown schema input"),
    ))
    .expect("schema identity is UTF-8")
    .trim()
    .to_owned();
    let schema = SchemaId::from_str(&schema_text).expect("unknown schema identity is canonical");
    assert_ne!(schema.as_str(), SOLID_FLUID_PALETTE_SCHEMA_ID_V1);
    assert_eq!(
        classify_fluid_palette_open(&schema),
        FluidPaletteOpenDispositionV1::ReadOnlyRecovery
    );
    assert_eq!(
        classify_fluid_palette_open(
            &SchemaId::from_str(SOLID_FLUID_PALETTE_SCHEMA_ID_V1)
                .expect("v1 palette schema is canonical")
        ),
        FluidPaletteOpenDispositionV1::Writable
    );

    let owner = PackageName::from_str("latticeaxiom").expect("canonical owner package");
    assert!(matches!(
        SchemaRequirementV1::new(schema.clone(), owner.clone(), 0, 1),
        Err(WorldDbError::InvalidSchemaVersionRange {
            minimum: 0,
            maximum: 1
        })
    ));
    assert!(matches!(
        SchemaRequirementV1::new(schema, owner, 2, 1),
        Err(WorldDbError::InvalidSchemaVersionRange {
            minimum: 2,
            maximum: 1
        })
    ));
    assert!(matches!(
        PayloadSchemaVersion::new(0),
        Err(latticeaxiom_storage::StorageError::InvalidSchemaVersion)
    ));
}

#[test]
fn abi_header_and_callback_faults_discard_staging_without_publishing() {
    let corpus = require_class("abi");
    require_case(&corpus, "abi.wrong-magic");
    require_case(&corpus, "abi.panic-or-exception");
    require_case(&corpus, "abi.process-fault");

    let good = LaxAbiHeader::new(0, 1, 16, 0x0000_a55a);
    validate_header(&good, ABI_HEADER_POLICY).expect("valid ABI header is accepted");
    let wrong_magic = LaxAbiHeader { magic: 0, ..good };
    assert!(matches!(
        validate_header(&wrong_magic, ABI_HEADER_POLICY),
        Err(AbiContractError::WrongMagic { actual: 0 })
    ));

    let staged = CallbackPolicy::new(WriteCommitMode::Staged, FailureScope::InstanceFatal, true)
        .expect("staged instance-fatal policy is valid");
    assert_eq!(
        decide_recovery(CallbackRecoveryInput {
            policy: staged,
            staging: STAGED_OUTPUT,
            fault: CallbackFault::PanicOrException,
            deadline_overrun_is_fatal: true,
        }),
        RecoveryAction::FailInstanceAndDiscardStaging
    );
    assert_eq!(
        decide_recovery(CallbackRecoveryInput {
            policy: staged,
            staging: STAGED_OUTPUT,
            fault: CallbackFault::ProcessFault,
            deadline_overrun_is_fatal: true,
        }),
        RecoveryAction::TerminateProcess
    );

    let direct = CallbackPolicy::new(WriteCommitMode::Direct, FailureScope::InstanceFatal, true)
        .expect("direct instance-fatal policy is valid");
    assert_eq!(
        decide_recovery(CallbackRecoveryInput {
            policy: direct,
            staging: STAGED_OUTPUT,
            fault: CallbackFault::InvalidOutput,
            deadline_overrun_is_fatal: true,
        }),
        RecoveryAction::FailInstanceAndInvalidateTick
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn storage_low_disk_crash_and_corrupt_header_preserve_the_durable_world() {
    let corpus = require_class("storage");
    let low_disk = require_case(&corpus, "storage.low-disk");
    let crash = require_case(&corpus, "storage.crash-before-publication");
    let header = require_case(&corpus, "storage.corrupt-header");
    assert!(low_disk.missing_hook.is_none());
    assert!(crash.missing_hook.is_none());
    assert_eq!(
        low_disk.public_boundary,
        "latticeaxiom_engine::SealedWorldWriterHost::observe_disk"
    );
    assert_eq!(
        crash.public_boundary,
        "latticeaxiom_engine::SealedWorldWriterHost::inject_database_fault_once"
    );

    let bytes = load_input(header.input.as_deref().expect("corrupt header input"));
    assert!(matches!(
        WorldHeaderV1::decode_canonical(&bytes),
        Err(HeaderCodecError::Malformed(_) | HeaderCodecError::BadChecksum { .. })
    ));

    let kernel = MemoryTransactionKernel::new();
    let world = fixture_world();
    let before = kernel
        .reference_snapshot(world)
        .expect("empty snapshot")
        .materialized_chunk_state_hash();
    kernel
        .inject_fault_once(FaultPoint::AfterStagingBeforePublish)
        .expect("memory failpoint installs");
    assert!(matches!(
        kernel.commit(memory_transaction(
            world,
            3,
            21,
            ChunkRevisionExpectation::Absent
        )),
        Err(latticeaxiom_storage::StorageError::InjectedFault {
            point: FaultPoint::AfterStagingBeforePublish
        })
    ));
    assert_eq!(
        kernel
            .reference_snapshot(world)
            .expect("failed staging remains unpublished")
            .materialized_chunk_state_hash(),
        before,
        "a pre-publish memory fault must not change the world hash"
    );

    let (mut host, world, metadata) = fixture_durable_host();
    activate_ready_writer(&mut host, world, &metadata);
    host.commit(durable_commit(
        world,
        &metadata,
        11,
        TransactionId::from_u128(9),
        WorldRevision::ZERO,
        ChunkRevisionExpectation::Absent,
    ))
    .expect("first durable commit publishes");
    host.create_checkpoint(CheckpointRequestV1::new(
        CheckpointId::from_u128(1),
        CheckpointKindV1::Protected,
        "fault-corpus",
    ))
    .expect("checkpoint at the durable frontier is retained");
    let durable_hash = loaded_chunk(&host, world)
        .expect("durable chunk exists")
        .data()
        .clone();

    host.observe_disk(
        DiskSample {
            usable_free_bytes: 4 * GIB,
            capacity_bytes: 100 * GIB,
        },
        HeadroomInputs::default(),
        None,
        DirtyDrainState::Clean,
    )
    .expect("low-disk sample is admitted");
    assert!(matches!(
        host.commit(durable_commit(
            world,
            &metadata,
            22,
            TransactionId::from_u128(10),
            WorldRevision::new(1),
            ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
        )),
        Err(SealedWriterHostError::WorldDb(
            WorldDbError::LowDiskMutationPaused { world: found, .. }
        )) if found == world
    ));
    assert_eq!(
        loaded_chunk(&host, world)
            .expect("paused mutation leaves the durable chunk")
            .data(),
        &durable_hash
    );

    host.observe_disk(
        DiskSample {
            usable_free_bytes: 0,
            capacity_bytes: 100 * GIB,
        },
        HeadroomInputs::default(),
        Some(StorageFailure::NoSpace),
        DirtyDrainState::FailedReadOnlyAvailable,
    )
    .expect("failed drain escalates to read-only recovery");
    let preflight = host
        .preflight(world)
        .expect("read-only recovery still reads metadata");
    assert!(matches!(
        preflight.status(),
        StoragePreflightStatusV1::RecoverableReadOnly { .. }
    ));
    assert!(preflight.activation_permit().is_none());
    if host.is_writer_active() {
        host.close().expect("paused writer still closes");
    }
    let permit = match host.preflight(world) {
        Ok(ready) => ready.activation_permit().cloned(),
        Err(_) => None,
    };
    assert!(
        permit.is_none(),
        "recoverable read-only must not yield a writer permit"
    );
    assert_eq!(
        loaded_chunk(&host, world)
            .expect("read-only recovery remains readable")
            .data(),
        &durable_hash
    );

    let (mut crash_host, crash_world, crash_metadata) = fixture_durable_host();
    activate_ready_writer(&mut crash_host, crash_world, &crash_metadata);
    crash_host
        .commit(durable_commit(
            crash_world,
            &crash_metadata,
            11,
            TransactionId::from_u128(9),
            WorldRevision::ZERO,
            ChunkRevisionExpectation::Absent,
        ))
        .expect("durable crash-corpus commit publishes");
    crash_host
        .create_checkpoint(CheckpointRequestV1::new(
            CheckpointId::from_u128(2),
            CheckpointKindV1::Protected,
            "crash-before-publication",
        ))
        .expect("crash-corpus checkpoint is retained");
    crash_host
        .inject_database_fault_once(DatabaseFaultPointV1::BeforeBatchPublication)
        .expect("database failpoint installs");
    assert!(matches!(
        crash_host.commit(durable_commit(
            crash_world,
            &crash_metadata,
            33,
            TransactionId::from_u128(12),
            WorldRevision::new(1),
            ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
        )),
        Err(SealedWriterHostError::WorldDb(
            WorldDbError::InjectedDatabaseFault { .. }
        ))
    ));
    crash_host
        .canonical_reopen()
        .expect("canonical reopen restores the last durable image");
    assert!(!crash_host.is_writer_active());
    crash_host
        .verify_crash_recovery(crash_world)
        .expect("unclean durable frontier verifies without a writer");
    let restored =
        loaded_chunk(&crash_host, crash_world).expect("pre-crash durable chunk survived reopen");
    assert_eq!(restored.chunk_revision(), ChunkRevision::new(1));
    assert_eq!(restored.data(), &fixture_data(11));
}

#[test]
fn stale_async_and_executor_panic_do_not_mutate_authoritative_state() {
    let corpus = require_class("async");
    require_case(&corpus, "async.stale-fluid-completion");
    let panic_case = require_case(&corpus, "async.executor-panic");
    assert!(panic_case.missing_hook.is_none());

    let captured = FluidRevisionStamp::new(
        WorldRevision::new(4),
        ChunkRevision::new(2),
        VoxelRevision::new(2),
    );
    let current = FluidRevisionStamp::new(
        WorldRevision::new(4),
        ChunkRevision::new(3),
        VoxelRevision::new(3),
    );
    assert!(matches!(
        admit_fluid_completion(captured, current),
        Err(FluidRuntimeError::Stale { .. })
    ));
    admit_fluid_completion(captured, captured).expect("matching stamps are admitted");

    let kernel = MemoryTransactionKernel::new();
    let runtime_world = WorldId::from_str("00000000-0000-4000-8000-000000000001")
        .expect("runtime fixture world UUID is canonical");
    let dimension = DimensionId::from_str("terrenia:dimension/terrenia")
        .expect("runtime fixture dimension is canonical");
    let scope = WorkingSetScope::new(runtime_world, dimension, WorldEpoch::new(7));
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, stored) = commit_runtime_chunk(&kernel, &scope, coordinate, vec![1; 64], 1);
    let before = kernel
        .reference_snapshot(runtime_world)
        .expect("committed runtime snapshot")
        .materialized_chunk_state_hash();
    let mut runtime = VoxelRuntime::new(scope, RuntimeGeneration::new(11), 4, 0, runtime_limits())
        .expect("derived runtime accepts fixture limits");
    runtime
        .project_committed(
            CommittedChunkProjection::from_stored_chunk(
                &stored,
                4,
                stored.data().voxels().bytes().to_vec(),
                MeshSemanticFingerprint::new([1; 32]),
                ColliderSemanticFingerprint::new([1; 32]),
            )
            .expect("stored fixture decodes"),
            FixedTick::new(1),
            derived_requests(),
        )
        .expect("projection fits");
    let DispatchOutcome::Started(input) = runtime
        .dispatch_next(DerivedKind::Collider)
        .expect("collider job starts")
    else {
        panic!("executor panic corpus requires an in-flight collider job");
    };
    let abort = runtime.complete_executor::<u8, (), ()>(
        ExecutorOutcome::Panicked { input },
        ApplyByteDeclaration::new(0),
        FixedTick::new(1),
        |_| Ok(()),
    );
    let ExecutorFinish::Aborted(WorkerAbortOutcome::Panicked(_)) = abort else {
        panic!("executor panic must abort without applying");
    };
    assert!(matches!(
        runtime.collider_safety(coordinate),
        Some(ColliderSafetyState::FailedConservative {
            failure: ColliderFailure::ExecutorPanicked,
            ..
        })
    ));
    assert!(
        runtime
            .last_applied_key(coordinate, DerivedKind::Collider)
            .is_none()
    );
    assert_eq!(
        kernel
            .reference_snapshot(runtime_world)
            .expect("kernel snapshot after derived panic")
            .materialized_chunk_state_hash(),
        before,
        "derived executor panic must not change the authoritative world hash"
    );
}

#[test]
fn device_loss_fallback_and_omitted_presentation_do_not_change_world_hash() {
    let corpus = require_class("device");
    let case = require_case(&corpus, "device.render-loss-fallback");
    assert!(case.missing_hook.is_none());
    let spec: UnsupportedGpuSpec = serde_json::from_slice(&load_input(
        case.input.as_deref().expect("unsupported GPU input"),
    ))
    .expect("unsupported GPU fixture parses");

    let capability = CapabilityId::from_str(&spec.capability).expect("capability is canonical");
    let provider = StableId::from_str(&spec.provider).expect("provider is canonical");
    let realization = StableId::from_str(&spec.realization).expect("realization is canonical");
    let selection = select_provider(&ProviderSelectionRequest {
        capability: capability.clone(),
        providers: vec![RenderProviderDecl {
            id: provider.clone(),
            capability,
            presentation_optional: spec.presentation_optional,
            realizations: vec![ProviderRealization {
                id: realization,
                stable_priority: 1,
                requirements: ProviderRequirement {
                    features: BTreeSet::from([GpuFeatureV1::ComputeShaders]),
                    minimum_limits: BTreeMap::new(),
                    formats: BTreeSet::new(),
                },
                fallback: ProviderFallback::Disabled,
            }],
        }],
        explicit_provider: Some(provider),
        gpu: GpuCapabilities::default(),
    })
    .expect("presentation-optional device loss falls back");
    assert_eq!(spec.required_feature, "compute-shaders");
    assert!(selection.disabled);
    assert!(selection.realization.is_none());

    let omitted = fixture_display_catalog(false).expect("omitted presentation compiles");
    let presented = fixture_display_catalog(true).expect("presentation compiles");
    assert_eq!(
        omitted.locked_ids().collect::<Vec<_>>(),
        presented.locked_ids().collect::<Vec<_>>(),
        "omitting presentation must not change locked content identities"
    );

    let kernel = MemoryTransactionKernel::new();
    let world = fixture_world();
    kernel
        .commit(memory_transaction(
            world,
            7,
            11,
            ChunkRevisionExpectation::Absent,
        ))
        .expect("authoritative commit is independent of GPU");
    let hash = kernel
        .reference_snapshot(world)
        .expect("committed snapshot")
        .materialized_chunk_state_hash();
    assert_eq!(
        kernel
            .reference_snapshot(world)
            .expect("post-device-loss snapshot")
            .materialized_chunk_state_hash(),
        hash,
        "device/presentation failure must not change the world hash"
    );
}

#[test]
fn shutdown_timeout_recovers_the_latest_durable_world_and_cannot_masquerade_as_save_and_quit() {
    let corpus = require_class("shutdown");
    let case = require_case(&corpus, "shutdown.timeout");
    assert!(case.missing_hook.is_none());

    let (mut host, world, metadata) = fixture_durable_host();
    assert_eq!(
        host.durability_capability(),
        StorageDurabilityCapabilityV1::WalSyncCheckpoint
    );
    activate_ready_writer(&mut host, world, &metadata);
    host.commit(durable_commit(
        world,
        &metadata,
        11,
        TransactionId::from_u128(9),
        WorldRevision::ZERO,
        ChunkRevisionExpectation::Absent,
    ))
    .expect("durable shutdown-corpus commit publishes");
    host.create_checkpoint(CheckpointRequestV1::new(
        CheckpointId::from_u128(3),
        CheckpointKindV1::Protected,
        "shutdown-timeout",
    ))
    .expect("shutdown-corpus checkpoint is retained");
    let lock_hash = CanonicalHash::digest(b"fault-corpus-shell-lock");
    let plan_hash = CanonicalHash::digest(b"fault-corpus-open-plan");
    let durable = DurableWorldRevisionV1::new(world, LauncherWorldRevision::new(1));
    let timeout = ChildExitReportV1::seal(ChildExitReportDraftV1 {
        child_generation: LaunchGeneration::FIRST,
        process_epoch: ProcessEpoch::FIRST,
        role: ChildRoleV1::World { world_id: world },
        exit_kind: ChildExitKindV1::ShutdownTimeout,
        intent_generation: None,
        intent_checksum: None,
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
        last_written_world: Some(durable),
        last_durable_world: Some(durable),
        shell_lock_hash: lock_hash,
        world_lock_hash: Some(lock_hash),
        world_open_plan_hash: Some(plan_hash),
        diagnostic_ref: None,
    })
    .expect("shutdown-timeout reports a durable frontier without a handoff intent");
    assert_eq!(timeout.exit_kind(), ChildExitKindV1::ShutdownTimeout);
    assert!(!timeout.exit_kind().is_normal_handoff());

    let masquerade = ChildExitReportV1::seal(ChildExitReportDraftV1 {
        child_generation: LaunchGeneration::FIRST,
        process_epoch: ProcessEpoch::FIRST,
        role: ChildRoleV1::World { world_id: world },
        exit_kind: ChildExitKindV1::SaveAndQuit,
        intent_generation: None,
        intent_checksum: None,
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
        last_written_world: Some(durable),
        last_durable_world: Some(durable),
        shell_lock_hash: lock_hash,
        world_lock_hash: Some(lock_hash),
        world_open_plan_hash: Some(plan_hash),
        diagnostic_ref: None,
    });
    assert!(
        matches!(masquerade, Err(LaunchModelError::InvalidChildExitShape)),
        "shutdown-timeout must not masquerade as Save & Quit"
    );

    host.canonical_reopen()
        .expect("timeout recovery reopens the last durable image");
    assert!(!host.is_writer_active());
    host.verify_crash_recovery(world)
        .expect("timeout recovery verifies without a writer");
    let restored = loaded_chunk(&host, world).expect("durable chunk survived timeout");
    assert_eq!(restored.chunk_revision(), ChunkRevision::new(1));
    assert_eq!(restored.data(), &fixture_data(11));
}

fn load_corpus() -> FaultCorpusDocument {
    let path = fixtures_root().join("corpus.json");
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("parse corpus: {error}"))
}

fn fixture_display_catalog(
    include_presentation: bool,
) -> Result<ContentDisplayCatalogV1, ProductionHostError> {
    let presentation = include_presentation.then_some(AuthoredPresentationCatalogSourcesV1 {
        display: PRESENTATION_DISPLAY_JSON,
        assets: PRESENTATION_ASSETS_JSON,
    });
    compile_authored_content_display_catalog(AuthoredContentDisplayCatalogSourcesV1 {
        block_display: AUTHORED_BLOCK_DISPLAY_JSON,
        tool_display: AUTHORED_TOOL_DISPLAY_JSON,
        tool_catalog: AUTHORED_TOOLS_JSON,
        d9_block_ids: D9_BLOCK_IDS,
        fluid_ids: FLUID_IDS,
        presentation,
    })
}

fn require_class(class: &str) -> FaultCorpusDocument {
    let corpus = load_corpus();
    assert!(
        corpus.cases.iter().any(|case| case.class == class),
        "corpus missing class {class}"
    );
    corpus
}

fn require_case<'a>(corpus: &'a FaultCorpusDocument, id: &str) -> &'a FaultCase {
    corpus
        .cases
        .iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("corpus missing case {id}"))
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../../fixtures/faults")
}

fn load_input(relative: &str) -> Vec<u8> {
    let path = fixtures_root().join(relative);
    fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn fixture_world() -> WorldId {
    WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
        .expect("fixture world UUID is canonical")
}

fn fixture_durable_host() -> (SealedWorldWriterHost, WorldId, AuthoritativeMetadataInputV1) {
    let record_owner = StableId::from_str("latticeaxiom:schema/world-db-chunk@1")
        .expect("fixture record owner is canonical");
    (
        SealedWorldWriterHost::durable_reference_with_default_publisher(record_owner),
        fixture_world(),
        fixture_metadata(),
    )
}

fn fixture_metadata() -> AuthoritativeMetadataInputV1 {
    let lock = FrozenLockReceiptV1::new(
        br#"{"version":1,"packages":[]}"#.to_vec(),
        BTreeMap::new(),
        fixture_digest(b"registration"),
        fixture_digest(b"semantic"),
        fixture_digest(b"bundles"),
        fixture_digest(b"roles"),
        fixture_digest(b"settings"),
    )
    .expect("fixture frozen-lock metadata is valid");
    let closure = WorldRequirementClosureV1::new(
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeSet::new(),
        BTreeMap::new(),
        fixture_digest(b"bundle-receipts"),
        fixture_digest(b"role-bindings"),
        BTreeMap::new(),
    )
    .expect("fixture requirement closure is valid");
    AuthoritativeMetadataInputV1::new(lock, closure)
}

fn fixture_digest(label: &[u8]) -> DigestV1 {
    DigestV1::hash(b"latticeaxiom/world-db-test/v1", label)
}

fn fixture_key(world: WorldId) -> ChunkKey {
    ChunkKey::new(
        world,
        DimensionId::from_str("terrenia:dimension/terrenia")
            .expect("fixture dimension is canonical"),
        ChunkCoordinate::new(1, -2, 3),
    )
}

fn fixture_data(seed: u8) -> ChunkData {
    let schema = SchemaId::from_str("latticeaxiom:schema/chunk-voxels@1")
        .expect("fixture payload schema is canonical");
    let version = PayloadSchemaVersion::new(1).expect("fixture payload schema version is positive");
    let voxels = VersionedPayload::new(schema.clone(), version, vec![seed, seed ^ 0x5a]);
    let entities = BTreeMap::from([(
        PersistentEntityId::from_u128(u128::from(seed)),
        VersionedPayload::new(schema.clone(), version, vec![seed.wrapping_add(1)]),
    )]);
    let continuations = BTreeMap::from([(
        ContinuationId::from_u128(u128::from(seed)),
        VersionedPayload::new(schema, version, vec![seed.wrapping_add(2)]),
    )]);
    let provenance = BTreeMap::from([(
        StableId::from_str("latticeaxiom:provenance/worldgen@1")
            .expect("fixture provenance is canonical"),
        CanonicalHash::digest([seed]),
    )]);
    ChunkData::new(voxels, entities, continuations, provenance)
}

fn durable_commit(
    world: WorldId,
    metadata: &AuthoritativeMetadataInputV1,
    seed: u8,
    transaction: TransactionId,
    base: WorldRevision,
    expectation: ChunkRevisionExpectation,
) -> WorldCommitRequestV1 {
    WorldCommitRequestV1::new(
        WorldTransaction::new(
            transaction,
            world,
            base,
            vec![ChunkMutation::new(
                fixture_key(world),
                expectation,
                ChangedDomains::ALL,
                fixture_data(seed),
            )],
        ),
        metadata.clone(),
        CommitDurabilityV1::Durable,
    )
}

fn memory_transaction(
    world: WorldId,
    transaction: u128,
    seed: u8,
    expectation: ChunkRevisionExpectation,
) -> WorldTransaction {
    WorldTransaction::new(
        TransactionId::from_u128(transaction),
        world,
        WorldRevision::ZERO,
        vec![ChunkMutation::new(
            fixture_key(world),
            expectation,
            ChangedDomains::ALL,
            fixture_data(seed),
        )],
    )
}

fn provision_ready(
    host: &SealedWorldWriterHost,
    world: WorldId,
    metadata: &AuthoritativeMetadataInputV1,
) -> ActivationPermitV1 {
    host.provision_world(WorldCreateRequestV1::new(
        world,
        DisplayName::new("Deterministic World").expect("fixture display name is valid"),
        StoreId::new("store-generation-1").expect("fixture store ID is valid"),
        metadata.clone(),
    ))
    .expect("fresh host store provisions");
    host.preflight(world)
        .expect("published header cross-checks authoritative metadata")
        .activation_permit()
        .expect("ready preflight carries one activation permit")
        .clone()
}

fn ready_exact_plan(world: WorldId, permit: &ActivationPermitV1) -> WorldOpenPlan {
    let action = WorldOpenAction::UseFrozenLock;
    WorldOpenPlan {
        world_id: world,
        status: WorldOpenStatus::ReadyExact,
        risk: WorldOpenRisk::None,
        reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
        next_safe_step: Some(action.clone()),
        actions: vec![action],
        diagnostics: Vec::new(),
        activation_binding: Some(sealed_activation_binding(permit)),
    }
}

fn activate_ready_writer(
    host: &mut SealedWorldWriterHost,
    world: WorldId,
    metadata: &AuthoritativeMetadataInputV1,
) {
    let permit = provision_ready(host, world, metadata);
    let plan = ready_exact_plan(world, &permit);
    let accepted = host
        .accept(&plan, WorldOpenAction::UseFrozenLock)
        .expect("frozen-lock accept requires bound catalog evidence");
    host.activate_writer(
        WriterActivationV1::new(accepted, permit).expect("sealed accept is writable"),
    )
    .expect("sealed durable writer activation succeeds");
}

fn loaded_chunk(host: &SealedWorldWriterHost, world: WorldId) -> Option<PersistedChunkV1> {
    let view: Box<dyn WorldReadView> = host.begin_read(world).expect("world remains readable");
    view.load_chunk(&fixture_key(world))
        .expect("portable record decodes")
}

fn runtime_limits() -> RuntimeLimits {
    let queue = DerivedQueueLimits::new(128, 32, 1024 * 1024).expect("queue limits are nonzero");
    RuntimeLimits::new(64, 2 * 1024 * 1024, queue, queue).expect("runtime limits are nonzero")
}

fn derived_requests() -> DerivedRequestSet {
    let request = DerivedRequest::new(
        DerivedPriority::new(0),
        DerivedOwner::new(1),
        DerivedMemoryBudget::new(512, 256),
    );
    DerivedRequestSet::new(request, request)
}

fn commit_runtime_chunk(
    storage: &MemoryTransactionKernel,
    scope: &WorkingSetScope,
    coordinate: ChunkCoordinate,
    cells: Vec<u8>,
    transaction: u128,
) -> (
    latticeaxiom_storage::CommitReceipt,
    latticeaxiom_storage::StoredChunk,
) {
    let schema = SchemaId::from_str("latticeaxiom:schema/chunk-voxels@1")
        .expect("runtime payload schema is canonical");
    let version = PayloadSchemaVersion::new(1).expect("runtime payload schema version is positive");
    let key = ChunkKey::new(scope.world(), scope.dimension().clone(), coordinate);
    let receipt = storage
        .commit(WorldTransaction::new(
            TransactionId::from_u128(transaction),
            scope.world(),
            WorldRevision::ZERO,
            vec![ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                ChunkData::new(
                    VersionedPayload::new(schema, version, cells),
                    BTreeMap::new(),
                    BTreeMap::new(),
                    BTreeMap::new(),
                ),
            )],
        ))
        .expect("runtime fixture transaction commits");
    let stored = storage
        .reference_snapshot(scope.world())
        .expect("committed snapshot remains available")
        .chunk(&key)
        .expect("committed chunk is present")
        .clone();
    (receipt, stored)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnsupportedGpuSpec {
    capability: String,
    provider: String,
    realization: String,
    required_feature: String,
    presentation_optional: bool,
}

struct TestDirectory(tempfile::TempDir);

impl TestDirectory {
    fn create() -> Self {
        Self(
            tempfile::Builder::new()
                .prefix("latticeaxiom-engine-v1-fault-corpus-")
                .tempdir()
                .expect("temp dir"),
        )
    }
}
