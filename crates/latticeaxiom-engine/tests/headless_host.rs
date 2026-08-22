//! Integration coverage for receipt-verified GPU-free Bevy hosts.
#![allow(clippy::expect_used)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use bevy::time::{Fixed, Time};
#[cfg(feature = "client")]
use bevy::{
    prelude::{Window, WindowPlugin},
    render::{RenderPlugin, renderer::RenderDevice},
};
use latticeaxiom_compose::{
    COMPOSITION_SCHEMA_VERSION, CompositionBootstrapV1, CompositionPolicy, CompositionSpec,
    ExactRegistration, LOCK_SCHEMA_VERSION, LockActionMode, LockedGameGraph, LockedPackage,
    ManifestProducer, NickelEvaluationLimits, NumericRegistrationId, PRODUCT_LOCK_FILE_NAME,
    PRODUCT_LOCK_PRODUCER_MACHINE, PackageDomain, PackageRequest, ProductLockDraftV1,
    ProductLockError, ProductLockObjects, ProductLockProducerV1, ProductLockReceiptKind,
    ProfileKind, RealizationId, RealizationKind, RealizationPreference, RegistrationFragment,
    RegistrationKind, RegistrationManifest, RuntimeBinding, RuntimeImage, SourceCandidate,
    TargetPackageRealizationV1, TargetRealizationLockV1, TrustClass, persist_product_lock,
};
use latticeaxiom_core::{
    CanonicalHash, NamespaceGrant, NamespaceGrantPattern, NamespaceGrantor, PackageName,
    PackageVersion, PackageVersionReq, RegistrationNamespace, SourceId, SourceProvenance, StableId,
    TargetTriple, canonical_json_bytes,
};
use latticeaxiom_engine::{
    ActionAxis2V1, AuthoritativeTransactionKernel, CellOccupancyV1, ChunkCoordinate, ChunkFaceV1,
    ChunkLifecycle, ChunkMeshCursor, ChunkPresentation, ChunkRevision, CommandOutcomeV1,
    ContainerId, DropEntityId, EngineInstance, EngineInstanceError, FluidFlowV1, FluidLevelV1,
    FluidStateV1, GameplayReject, HOTBAR_SLOTS, HeadlessTargetInspectV1, INVENTORY_SLOTS, ItemId,
    ItemStackV1, LockVerifiedComposeImages, MAX_TICKS_PER_ADVANCE, MeshReceipt,
    PlayerActionButtonsV1, PlayerActionFrameV1, PlayerActionV1, PreparationError,
    ProductionInspectSurface, ProductionMemoryStart, ProductionSessionPause, ProductionSpine,
    ProductionWorldList, ProductionWorldStorage, RecipeId, STREAMING_PROFILE_EVIDENCE_SCHEMA_V1,
    SealedWorldWriterHost, SealedWriterHostError, SlotIndex, StructurallyValidatedComposeImages,
    VerifiedProductLockHash, WorkingSetDiagnosticsV1, WorkstationId, authored_gameplay_catalog,
    empty_gameplay_catalog,
};
use latticeaxiom_gameplay::BlockId;
use latticeaxiom_launcher::{
    ChildExitKindV1, HostBuildReceipts, ProductLockBootError, ReopenedFinalLockV1,
};
use latticeaxiom_player::{BlockEditRejectV1, BlockFaceV1};
use latticeaxiom_registration::{
    CallbackDeclaration, CompiledRegistration, PackageRegistrationInput, ReceiptValidationError,
    RegistrationCompileInput, RegistrationCompiler, SystemDeclaration,
};
use latticeaxiom_start_ui::{
    ClientShellGraph, InputSource, MemoryStartEffect, SemanticActionId, SemanticCommand,
    SemanticNodeId, ShellCapability, ShellEffect, ShellPackageProvider, ShellScreen,
};
use latticeaxiom_world_catalog::WorldOpenAction;
use latticeaxiom_world_db::{WorldDbError, WorldStorage};
use serde::Serialize;
use serde_json::Value;

const FIXED_TIMESTEP: Duration = Duration::from_millis(20);
const FIXED_STAGE: &str = "latticeaxiom:system-stage/gameplay/fixed@1";

#[test]
fn per_kind_numeric_zero_and_distinct_callback_key_are_accepted() {
    let fixture = fixture();
    let block = stable_id("terrenia:block/alpha");

    assert_eq!(
        fixture.compiled.image_receipt.numeric_ids[&RegistrationKind::Block][&block],
        NumericRegistrationId(0)
    );
    assert_eq!(
        fixture.compiled.image_receipt.numeric_ids[&RegistrationKind::System][&fixture.system],
        NumericRegistrationId(0)
    );
    assert_ne!(fixture.system, fixture.callback);

    let prepared = fixture.prepared();
    assert_eq!(
        prepared.registration_semantic_hash(),
        prepared
            .registration()
            .image_receipt
            .registration_semantic_hash
    );
    assert!(
        prepared
            .callback_receipt()
            .callbacks
            .get(&fixture.callback)
            .is_some_and(|binding| binding.consumers.contains(&fixture.system))
    );
}

#[test]
fn system_id_used_as_callback_key_is_rejected() {
    let mut fixture = fixture();
    fixture
        .runtime
        .packages
        .get_mut(&fixture.package)
        .expect("fixture runtime package exists")
        .callbacks = BTreeSet::from([fixture.system.clone()]);

    assert!(matches!(
        StructurallyValidatedComposeImages::new(
            fixture.graph,
            fixture.compiled,
            fixture.runtime,
        ),
        Err(PreparationError::MissingRuntimeCallback { package, callback })
            if package == fixture.package && callback == fixture.callback
    ));
}

#[test]
fn rehashed_manifest_receipt_mismatch_is_rejected_against_lock() {
    let mut fixture = fixture();
    let tampered = CanonicalHash::digest(b"tampered-manifest-semantic-hash");
    fixture
        .compiled
        .provenance_receipt
        .packages
        .get_mut(&fixture.package)
        .expect("fixture package provenance exists")
        .manifest_semantic_hash = tampered;
    fixture.compiled.provenance_receipt.provenance_hash = fixture
        .compiled
        .provenance_receipt
        .recompute_hash()
        .expect("tampered provenance receipt canonicalizes");

    assert!(matches!(
        StructurallyValidatedComposeImages::new(
            fixture.graph,
            fixture.compiled,
            fixture.runtime,
        ),
        Err(PreparationError::CompiledRegistration(
            ReceiptValidationError::LockedManifestSemanticHashMismatch {
                package,
                locked,
                receipt,
            }
        )) if package == fixture.package
            && locked != receipt
            && receipt == tampered
    ));
}

#[test]
fn tampered_callback_receipt_is_rejected_before_construction() {
    let mut fixture = fixture();
    fixture.compiled.callback_receipt.callback_map_hash = CanonicalHash::digest(b"tampered");

    assert!(matches!(
        StructurallyValidatedComposeImages::new(fixture.graph, fixture.compiled, fixture.runtime,),
        Err(PreparationError::CompiledRegistration(_))
    ));
}

#[test]
fn exact_fixed_ticks_are_isolated() {
    let fixture = fixture();
    let prepared = fixture.prepared();
    let mut first = EngineInstance::new_headless(prepared.clone(), FIXED_TIMESTEP)
        .expect("first headless instance starts");
    let mut second =
        EngineInstance::new_headless(prepared, FIXED_TIMESTEP).expect("second instance starts");

    first.advance_fixed_ticks(7).expect("seven ticks advance");
    first.advance_fixed_ticks(20).expect("twenty ticks advance");
    second
        .advance_fixed_ticks(2)
        .expect("isolated ticks advance");

    assert_eq!(first.completed_fixed_ticks(), 27);
    assert_eq!(second.completed_fixed_ticks(), 2);
    assert_eq!(fixed_elapsed(&first), FIXED_TIMESTEP.saturating_mul(27));
    assert_eq!(fixed_elapsed(&second), FIXED_TIMESTEP.saturating_mul(2));
}

#[test]
#[cfg(feature = "client")]
fn headless_profile_has_no_window_or_render_device() {
    let mut instance = EngineInstance::new_headless(fixture().prepared(), FIXED_TIMESTEP)
        .expect("headless instance starts");

    assert!(!instance.app().is_plugin_added::<WindowPlugin>());
    assert!(!instance.app().is_plugin_added::<RenderPlugin>());
    assert!(
        instance
            .app()
            .world()
            .get_resource::<RenderDevice>()
            .is_none()
    );
    assert_eq!(
        instance
            .app()
            .world()
            .iter_entities()
            .filter(bevy::ecs::world::EntityRef::contains::<Window>)
            .count(),
        0
    );

    instance.advance_fixed_ticks(3).expect("ticks advance");
    assert!(
        instance
            .app()
            .world()
            .get_resource::<RenderDevice>()
            .is_none()
    );
}

#[test]
fn manual_tick_advance_is_bounded_and_checks_duration_overflow() {
    let prepared = fixture().prepared();
    let mut bounded = EngineInstance::new_headless(prepared.clone(), FIXED_TIMESTEP)
        .expect("bounded instance starts");
    assert_eq!(
        bounded.advance_fixed_ticks(MAX_TICKS_PER_ADVANCE + 1),
        Err(EngineInstanceError::TickAdvanceLimitExceeded {
            requested: MAX_TICKS_PER_ADVANCE + 1,
            maximum: MAX_TICKS_PER_ADVANCE,
        })
    );
    assert_eq!(bounded.completed_fixed_ticks(), 0);

    let mut overflowing =
        EngineInstance::new_headless(prepared, Duration::MAX).expect("overflow instance starts");
    assert_eq!(
        overflowing.advance_fixed_ticks(2),
        Err(EngineInstanceError::TickDurationOverflow {
            fixed_timestep: Duration::MAX,
            ticks: 2,
        })
    );
    assert_eq!(overflowing.completed_fixed_ticks(), 0);
}

#[test]
fn zero_timestep_is_rejected() {
    assert!(matches!(
        EngineInstance::new_headless(fixture().prepared(), Duration::ZERO),
        Err(EngineInstanceError::ZeroFixedTimestep)
    ));
}

#[test]
fn reopened_lock_starts_gpu_free_headless_without_re_resolving() {
    let boot = lock_boot_fixture();
    let prepared = boot.prepared();
    let client_share = prepared.clone();
    assert_eq!(
        client_share.product_lock_hash(),
        boot.reopened.product_lock_hash()
    );
    assert_eq!(client_share.target(), prepared.target());

    let mut instance = EngineInstance::new_headless_from_lock(prepared, FIXED_TIMESTEP)
        .expect("headless instance starts from the reopened lock");
    assert_eq!(
        instance.profile(),
        latticeaxiom_engine::EngineProfile::Headless
    );
    assert_eq!(
        instance
            .app()
            .world()
            .get_resource::<VerifiedProductLockHash>()
            .copied()
            .map(VerifiedProductLockHash::get),
        Some(boot.reopened.product_lock_hash())
    );
    instance
        .advance_fixed_ticks(3)
        .expect("lock-boot headless ticks advance");
    assert_eq!(instance.completed_fixed_ticks(), 3);
}

const SPINE_TIMESTEP: Duration = Duration::from_nanos(1_000_000_000 / 60);

#[test]
#[allow(clippy::too_many_lines)]
fn production_spine_lock_verified_host_edits_chunk_meshes_not_blocks() {
    let boot = lock_boot_fixture();
    let images = boot.prepared();
    let mut instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        images,
        SPINE_TIMESTEP,
        empty_gameplay_catalog().expect("empty gameplay catalog compiles"),
    )
    .expect("production spine starts from the reopened lock");

    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let storage = instance
        .app()
        .world()
        .get_resource::<ProductionWorldStorage>()
        .expect("memory kernel is installed behind the production storage interface")
        .clone();
    let world_id = spine.world_id().expect("spine owns a world identity");
    let snapshot = storage
        .kernel()
        .reference_snapshot(world_id)
        .expect("memory kernel exposes a reference snapshot");
    assert!(snapshot.chunks().len() > 0, "D4 region was materialized");
    assert!(
        spine.has_all_six_faces(),
        "chunk meshes must emit all six faces, got {:?} from {} derived chunks",
        spine.visible_faces(),
        spine.derived_chunk_count()
    );

    let chunk_entities = instance.production_chunk_entity_count();
    assert!(chunk_entities > 0, "chunk colliders must be spawned");
    let voxel_count = snapshot
        .chunks()
        .next()
        .expect("generated region contains a chunk")
        .1
        .data()
        .voxels()
        .bytes()
        .len()
        / 4;
    assert!(
        chunk_entities < voxel_count,
        "presentation must not spawn one entity per block (chunks={chunk_entities}, voxels/chunk={voxel_count})"
    );
    assert_eq!(
        instance
            .app()
            .world()
            .iter_entities()
            .filter(|entity| entity
                .get::<bevy::prelude::Name>()
                .is_some_and(|name| name.as_str() == "Playable Block"))
            .count(),
        0
    );

    let spawn = spine.spawn_center();
    let cursors_before: BTreeMap<ChunkCoordinate, ChunkMeshCursor> = snapshot
        .chunks()
        .filter_map(|(key, _)| {
            spine
                .mesh_cursor(key.coordinate)
                .map(|cursor| (key.coordinate, cursor))
        })
        .collect();
    instance
        .enqueue_headless_actions([
            look_frame(1, -std::f32::consts::FRAC_PI_2, 0.7),
            idle_frame(2),
            break_frame(3),
        ])
        .expect("look and break frames enqueue");
    instance
        .advance_fixed_ticks(4)
        .expect("look and break ticks advance");

    let success = spine.last_success().unwrap_or_else(|| {
        panic!(
            "authoritative break must succeed, reject={:?}, pose={:?}",
            spine.last_reject(),
            spine.player_pose()
        )
    });
    assert!(
        success.position.x < 0 || success.position.z < 0,
        "break must land on a negative X or Z voxel, got {:?}",
        success.position
    );
    let edited_chunk = spine
        .chunk_of(success.position)
        .expect("broken voxel maps to a host chunk");
    let cursor = cursors_before
        .get(&edited_chunk)
        .copied()
        .expect("edited chunk had a mesh before the break");
    let revision_after_break = success.committed_chunk_revision;
    assert!(
        revision_after_break.get() > 0,
        "storage-backed chunk revision must advance past zero"
    );

    instance
        .enqueue_headless_actions(
            (4..=20)
                .map(idle_frame)
                .chain(std::iter::once(place_frame(21, &spine)))
                .chain((22..=50).map(|generation| PlayerActionFrameV1 {
                    generation,
                    movement: ActionAxis2V1 { x: 0.0, y: 1.0 },
                    ..PlayerActionFrameV1::default()
                }))
                .chain(std::iter::once({
                    let mut started = PlayerActionButtonsV1::empty();
                    started.insert(PlayerActionV1::Jump);
                    PlayerActionFrameV1 {
                        generation: 51,
                        started,
                        ..PlayerActionFrameV1::default()
                    }
                }))
                .chain((52..=90).map(idle_frame)),
        )
        .expect("place, move, and jump frames enqueue");
    instance
        .advance_fixed_ticks(90)
        .expect("place, move, and jump ticks advance");

    let pose = spine.player_pose();
    assert!(
        pose.translation.distance(spawn) > 0.4,
        "move/jump frames must change player position (spawn {:?}, now {:?})",
        spawn,
        pose.translation
    );
    assert!(
        pose.yaw_radians.abs() > 0.5,
        "look frames must change yaw, got {}",
        pose.yaw_radians
    );
    assert!(
        spine
            .last_success()
            .is_some_and(|latest| latest != success
                || latest.committed_chunk_revision != revision_after_break),
        "place must commit after the shared cooldown"
    );
    assert!(
        spine.mesh_invalidated(&cursor),
        "break must invalidate the previous chunk mesh receipt"
    );
    assert!(
        spine
            .chunk_revision(edited_chunk)
            .is_some_and(|revision| revision.get() >= revision_after_break.get()),
        "chunk revision must remain advanced after later edits"
    );
    let chunk_entities_after = instance
        .app()
        .world()
        .iter_entities()
        .filter(bevy::ecs::world::EntityRef::contains::<ChunkPresentation>)
        .count();
    assert!(
        chunk_entities_after > 0 && chunk_entities_after < voxel_count,
        "edits must replace chunk colliders, not spawn per-block entities (before={chunk_entities}, after={chunk_entities_after}, voxels/chunk={voxel_count})"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn production_spine_headless_inspect_reports_targeted_block_id_after_dda() {
    let boot = lock_boot_fixture();
    let images = boot.prepared();
    let mut instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        images,
        SPINE_TIMESTEP,
        empty_gameplay_catalog().expect("empty gameplay catalog compiles"),
    )
    .expect("production spine starts from the reopened lock");

    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let inspect_surface = instance
        .app()
        .world()
        .get_resource::<ProductionInspectSurface>()
        .copied()
        .expect("inspect surface selection is installed");
    assert!(
        inspect_surface.uses_headless_dto(),
        "the V2 lock-boot fixture does not select inspect/observability packages"
    );
    assert!(
        !boot
            .graph
            .packages
            .keys()
            .any(|package| package.as_str() == "@terrenia/presentation"),
        "headless inspect must work when the presentation package is omitted"
    );

    instance
        .enqueue_headless_actions([
            look_frame(1, -std::f32::consts::FRAC_PI_2, 0.7),
            idle_frame(2),
        ])
        .expect("look frames enqueue");
    instance.advance_fixed_ticks(3).expect("look ticks advance");

    let current = spine.current_target().unwrap_or_else(|| {
        panic!(
            "crosshair DDA must hit after look, pose={:?}",
            spine.player_pose()
        )
    });
    assert!(
        current.observation.position.x < 0 || current.observation.position.z < 0,
        "inspect DDA must land on a negative X or Z voxel, got {:?}",
        current.observation.position
    );
    assert!(
        current.block_id.as_str().starts_with("terrenia:block/"),
        "targeted block id must be a registered Terrenia block, got {}",
        current.block_id
    );
    assert_inspect_dto_overlay_fields(&current, &spine);
    let occupancy = instance
        .app()
        .world()
        .get_resource::<WorkingSetDiagnosticsV1>()
        .copied()
        .expect("working-set diagnostics remain installed");
    assert_eq!(
        occupancy.inspect_occupancy_line(),
        format!(
            "r{} a{} i{} d{}",
            occupancy.resident(),
            occupancy.active(),
            occupancy.in_flight(),
            occupancy.dirty()
        )
    );

    let hash_before_inspect = spine
        .materialized_chunk_state_hash()
        .expect("omitted-presentation host exposes a world hash");
    instance
        .enqueue_headless_actions([inspect_frame(3)])
        .expect("inspect frame enqueues");
    instance
        .advance_fixed_ticks(2)
        .expect("inspect ticks advance");

    let inspected = spine
        .last_inspect()
        .expect("inspect action must emit a result")
        .unwrap_or_else(|reject| panic!("authoritative inspect must succeed, reject={reject:?}"));
    assert_eq!(inspected.block_id, current.block_id);
    assert_eq!(inspected.observation.position, current.observation.position);
    assert_eq!(inspected.block_display_name, current.block_display_name);
    assert_eq!(inspected.chunk, current.chunk);
    assert_inspect_dto_overlay_fields(&inspected, &spine);
    assert_eq!(
        spine
            .materialized_chunk_state_hash()
            .expect("inspect still exposes a world hash"),
        hash_before_inspect,
        "headless inspect with omitted presentation must not change the world hash"
    );

    instance
        .enqueue_headless_actions([break_frame(4)])
        .expect("break frame enqueues");
    instance
        .advance_fixed_ticks(2)
        .expect("break ticks advance");

    let success = spine.last_success().unwrap_or_else(|| {
        panic!(
            "authoritative break must succeed after inspect, reject={:?}",
            spine.last_reject()
        )
    });
    assert_eq!(success.position, inspected.observation.position);
    assert_eq!(success.old_content.as_ref(), Some(&inspected.block_id));
}

#[test]
fn headless_omitting_presentation_does_not_change_world_hash() {
    let boot = lock_boot_fixture();
    assert!(
        !boot
            .graph
            .packages
            .keys()
            .any(|package| package.as_str() == "@terrenia/presentation"),
        "the V2 lock-boot fixture omits the presentation package"
    );

    let mut omitted = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        empty_gameplay_catalog().expect("empty gameplay catalog compiles"),
    )
    .expect("omitted-presentation host starts");
    omitted
        .advance_fixed_ticks(1)
        .expect("omitted-presentation host advances");
    let omitted_spine = omitted
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let omitted_hash = omitted_spine
        .materialized_chunk_state_hash()
        .expect("omitted-presentation host exposes a world hash");
    let label = omitted_spine.content_display("terrenia:block/oak-log");
    assert_eq!(label.name, "Oak Log");
    assert!(!label.icon.is_empty());
    assert_ne!(label.icon, "terrenia:block/oak-log");
    assert_eq!(
        omitted_spine
            .materialized_chunk_state_hash()
            .expect("display lookup still exposes a world hash"),
        omitted_hash,
        "HUD display lookup must not change the materialized-chunk world hash"
    );

    let mut second = EngineInstance::new_headless_host_from_lock_with_catalog(
        lock_boot_fixture().prepared(),
        SPINE_TIMESTEP,
        empty_gameplay_catalog().expect("empty gameplay catalog compiles"),
    )
    .expect("second omitted-presentation host starts");
    second
        .advance_fixed_ticks(1)
        .expect("second omitted-presentation host advances");
    let second_spine = second
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("second production spine is installed")
        .clone();
    assert_eq!(
        second_spine
            .materialized_chunk_state_hash()
            .expect("second host exposes a world hash"),
        omitted_hash,
        "independent headless hosts that omit presentation must hash-equal"
    );
}

#[test]
fn production_spine_headless_water_and_lava_occupancy_round_trips_at_signed_xz() {
    let boot = lock_boot_fixture();
    let images = boot.prepared();
    let instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        images,
        SPINE_TIMESTEP,
        empty_gameplay_catalog().expect("empty gameplay catalog compiles"),
    )
    .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let occupancy = instance
        .app()
        .world()
        .get_resource::<WorkingSetDiagnosticsV1>()
        .copied()
        .expect("working-set diagnostics remain installed");
    assert_eq!(
        occupancy.saving(),
        0,
        "occupancy host must not open a writer"
    );

    let water = stable_id("terrenia:fluid/water");
    let lava = stable_id("terrenia:fluid/lava");
    let source = FluidStateV1 {
        level: FluidLevelV1::SOURCE,
        flow: FluidFlowV1::Still,
    };
    let positive = first_direct_fluid_cell(&spine, true);
    let negative = first_direct_fluid_cell(&spine, false);
    assert!(
        positive.x > 0 && positive.z > 0,
        "water cell must be in +XZ, got {positive:?}"
    );
    assert!(
        negative.x < 0 && negative.z < 0,
        "lava cell must be in -XZ, got {negative:?}"
    );

    let water_placed = place_and_inspect_fluid(&spine, positive, &water, source);
    let lava_placed = place_and_inspect_fluid(&spine, negative, &lava, source);
    assert_occupancy_payloads(&spine, &[positive, negative]);
    assert_eq!(
        spine
            .inspect_occupancy(positive)
            .expect("water still inspects"),
        water_placed
    );
    assert_eq!(
        spine
            .inspect_occupancy(negative)
            .expect("lava still inspects"),
        lava_placed
    );
    assert!(
        matches!(
            spine.place_fluid_occupancy(
                latticeaxiom_gameplay::BlockPosition {
                    x: -1,
                    y: 30,
                    z: -1,
                },
                &water,
                source,
            ),
            Err(BlockEditRejectV1::NotReplaceable)
        ),
        "stone probe occupancy must reject fluid writes"
    );
    assert_eq!(
        spine.working_set_diagnostics().saving(),
        0,
        "fluid occupancy must stay on the memory kernel"
    );
}

#[test]
fn production_host_exposes_working_set_diagnostics() {
    let boot = lock_boot_fixture();
    let images = boot.prepared();
    let mut instance = EngineInstance::new_headless_host_from_lock(images, SPINE_TIMESTEP)
        .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let limits = spine
        .hard_limits()
        .expect("production host clamps are installed");

    let snapshot = instance
        .app()
        .world()
        .get_resource::<WorkingSetDiagnosticsV1>()
        .copied()
        .expect("working-set diagnostics are installed");
    assert_eq!(snapshot, spine.working_set_diagnostics());
    assert_working_set_diagnostics(snapshot, limits);

    instance
        .advance_fixed_ticks(2)
        .expect("idle ticks refresh working-set diagnostics");
    let refreshed = instance
        .app()
        .world()
        .get_resource::<WorkingSetDiagnosticsV1>()
        .copied()
        .expect("working-set diagnostics remain installed");
    assert_eq!(refreshed, spine.working_set_diagnostics());
    assert_working_set_diagnostics(refreshed, limits);
}

#[test]
fn requested_view_distance_is_clamped_by_host_limits() {
    let instance =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let cap = spine
        .hard_limits()
        .expect("playable host clamps exist")
        .view_distance_chunks;
    assert!(cap >= 1);
    let effective_one = spine
        .set_requested_view_distance(1)
        .expect("view distance 1 is admitted");
    assert_eq!(spine.requested_view_distance(), 1);
    assert_eq!(effective_one, spine.effective_view_distance());
    assert!(effective_one <= 1);
    let effective_cap = spine
        .set_requested_view_distance(cap)
        .expect("hard-cap view distance is admitted");
    assert_eq!(spine.requested_view_distance(), cap);
    assert_eq!(effective_cap, spine.effective_view_distance());
    assert!(effective_cap <= cap);
    let _ = spine
        .set_requested_view_distance(cap.saturating_add(8))
        .expect("oversize requests clamp");
    assert_eq!(spine.requested_view_distance(), cap);
}

#[test]
fn camera_yaw_pitch_only_does_not_change_presentation_state() {
    const TICKS: u32 = 6;
    let mut idle =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("idle production spine starts from the reopened lock");
    let mut looking =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("look-only production spine starts from the reopened lock");
    let idle_spine = idle
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("idle production spine is installed")
        .clone();
    let looking_spine = looking
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("look-only production spine is installed")
        .clone();

    looking
        .enqueue_headless_actions([
            look_frame(1, -std::f32::consts::FRAC_PI_2, 0.7),
            look_frame(2, -0.4, -0.3),
            idle_frame(3),
            idle_frame(4),
            idle_frame(5),
            idle_frame(6),
        ])
        .expect("yaw/pitch-only frames enqueue");
    idle.advance_fixed_ticks(TICKS).expect("idle ticks advance");
    looking
        .advance_fixed_ticks(TICKS)
        .expect("look-only ticks advance");

    let idle_pose = idle_spine.player_pose();
    let looking_pose = looking_spine.player_pose();
    assert!(
        looking_pose.yaw_radians.abs() > 0.5,
        "look frames must change yaw, got {}",
        looking_pose.yaw_radians
    );
    assert!(
        idle_pose.yaw_radians.abs() < 0.01,
        "idle frames must not change yaw, got {}",
        idle_pose.yaw_radians
    );
    assert!(
        (looking_pose.translation.x - idle_pose.translation.x).abs() < 0.01
            && (looking_pose.translation.z - idle_pose.translation.z).abs() < 0.01,
        "look-only frames must not translate on XZ versus idle (idle {:?}, looking {:?})",
        idle_pose.translation,
        looking_pose.translation
    );
    assert_eq!(
        presentation_invariant_snapshot(&looking, &looking_spine),
        presentation_invariant_snapshot(&idle, &idle_spine),
        "yaw/pitch-only camera motion must not change resident set, lifecycle, mesh receipt, entity count, or queue counters"
    );
}

#[test]
fn streaming_profile_evidence_is_machine_readable_and_does_not_claim_d2() {
    let mut instance =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    instance
        .advance_fixed_ticks(4)
        .expect("fixed ticks advance");
    let evidence = spine
        .streaming_profile_evidence()
        .expect("live streaming evidence is available");
    assert_eq!(evidence.schema, STREAMING_PROFILE_EVIDENCE_SCHEMA_V1);
    assert_eq!(evidence.chunk_edge_voxels, 8);
    assert_eq!(evidence.equivalent_active_radius_chunks, 16);
    assert_eq!(evidence.equivalent_resident_radius_chunks, 24);
    assert!(!evidence.matches_adr_0026_world_space_coverage);
    assert!(!evidence.claims_d2_working_set_gate);
    assert_eq!(evidence.p4_choice, "retain-8-cubed-correctness-fixture");
    assert!(evidence.counts.resident > 0);
    let encoded = serde_json::to_value(&evidence).expect("evidence serializes");
    assert_eq!(encoded["schema"], STREAMING_PROFILE_EVIDENCE_SCHEMA_V1);
    assert_eq!(encoded["claims_d2_working_set_gate"], false);
}

#[test]
fn negative_coordinate_eviction_revisit_restores_identical_clean_chunk() {
    let mut instance =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let edited = spine.edited_chunks();
    let sample = spine
        .resident_chunks()
        .into_iter()
        .find(|chunk| (chunk.x < 0 || chunk.z < 0) && !edited.contains(chunk))
        .expect("spawn working set includes a clean negative-coordinate chunk");
    let before = spine.mesh_cursor(sample);
    assert!(
        before.is_some(),
        "negative chunk {sample:?} must have a mesh receipt before leaving"
    );
    let generation =
        enqueue_look_then_walk(&mut instance, 1, std::f32::consts::FRAC_PI_2, 0.0, 1.0, 720);
    let mut seen = BTreeSet::new();
    let mut seen_player = BTreeSet::new();
    let mut min_y = spine.player_pose().translation.y;
    sample_walk(
        &mut instance,
        &spine,
        720,
        &mut seen,
        &mut seen_player,
        &mut min_y,
    );
    assert!(
        sample.x < 0 || sample.z < 0,
        "sample must be a negative coordinate, got {sample:?}"
    );
    assert!(
        !spine.resident_chunks().contains(&sample),
        "clean negative chunk {sample:?} must evict after walking away (pose {:?})",
        spine.player_pose().translation
    );
    enqueue_look_then_walk(
        &mut instance,
        generation,
        -std::f32::consts::PI,
        0.0,
        1.0,
        1_040,
    );
    sample_walk(
        &mut instance,
        &spine,
        1_040,
        &mut seen,
        &mut seen_player,
        &mut min_y,
    );
    assert!(
        spine.resident_chunks().contains(&sample),
        "revisiting {sample:?} must rematerialize the evicted clean chunk (pose {:?})",
        spine.player_pose().translation
    );
    let after = spine.mesh_cursor(sample);
    let before = before.expect("pre-eviction cursor");
    let after = after.expect("revisited cursor");
    assert_eq!(after.coordinate(), before.coordinate());
    assert_eq!(after.revision(), before.revision());
    assert_eq!(
        after.receipt().source().chunk(),
        before.receipt().source().chunk()
    );
    assert_eq!(
        after.receipt().source().epoch(),
        before.receipt().source().epoch()
    );
    assert_eq!(
        after.receipt().source().revision(),
        before.receipt().source().revision()
    );
}

#[test]
fn derived_queues_stay_bounded_with_cancellation_under_traversal() {
    let mut instance =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let limits = spine.hard_limits().expect("host clamps are installed");
    let max_resident = usize::try_from(limits.max_resident_chunks).expect("resident cap fits");
    let max_in_flight = usize::try_from(limits.max_in_flight_chunks).expect("in-flight cap fits");
    let _ = spine
        .set_requested_view_distance(limits.view_distance_chunks)
        .expect("view distance clamp");
    enqueue_look_then_walk(&mut instance, 1, std::f32::consts::FRAC_PI_2, 0.0, 1.0, 480);
    instance.advance_fixed_ticks(2).expect("look ticks advance");
    let mut high_resident = 0_usize;
    let mut remaining = 480_u32;
    while remaining > 0 {
        let step = remaining.min(32);
        instance
            .advance_fixed_ticks(step)
            .expect("bounded walk advances");
        remaining -= step;
        let diagnostics = spine.working_set_diagnostics();
        let queues = spine.derived_queue_snapshot();
        let resident = usize::try_from(diagnostics.resident()).unwrap_or(usize::MAX);
        high_resident = high_resident.max(resident);
        assert!(
            resident <= max_resident,
            "resident {resident} exceeded cap {max_resident}"
        );
        let in_flight = usize::try_from(diagnostics.in_flight()).unwrap_or(usize::MAX);
        assert!(
            in_flight <= max_in_flight.saturating_mul(4),
            "in-flight {in_flight} exceeded derived cap"
        );
        assert!(
            queues.mesh_pending + queues.mesh_in_flight <= 128,
            "mesh queue {} + {} exceeded ADR 0026 cap",
            queues.mesh_pending,
            queues.mesh_in_flight
        );
        assert!(
            queues.collider_pending + queues.collider_in_flight <= 64,
            "collider queue exceeded ADR 0026 cap"
        );
        assert!(
            queues.reserved_bytes <= diagnostics.byte_budget(),
            "reserved bytes {} exceeded budget {}",
            queues.reserved_bytes,
            diagnostics.byte_budget()
        );
    }
    assert!(high_resident > 0, "traversal must occupy a working set");
    let queues = spine.derived_queue_snapshot();
    assert!(
        queues.cancel_requests > 0
            || spine.stream_eviction_count() > 0
            || spine.resident_chunks().len() < high_resident,
        "eviction or cancellation must keep the working set bounded (cancels {}, evictions {}, resident {})",
        queues.cancel_requests,
        spine.stream_eviction_count(),
        spine.resident_chunks().len()
    );
}

#[test]
fn at_most_one_interest_reconciliation_per_fixed_tick() {
    const TICKS: u32 = 4;
    let boot = lock_boot_fixture();
    let images = boot.prepared();
    let mut instance = EngineInstance::new_headless_host_from_lock(images, SPINE_TIMESTEP)
        .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();

    let before = spine.interest_reconciliation_count();
    instance
        .advance_fixed_ticks(TICKS)
        .expect("fixed ticks advance");
    let observed = spine.interest_reconciliation_count().saturating_sub(before);
    assert_eq!(
        observed,
        u64::from(TICKS),
        "interest reconciliation ran {observed} times across {TICKS} fixed ticks"
    );
}

#[test]
fn intra_chunk_motion_does_not_distance_evict_or_remesh() {
    const SETTLE: u32 = 8;
    const HOLD: u32 = 24;
    let mut instance =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let edge = f32::from(spine.chunk_edge());
    let spawn = spine.player_pose().translation;
    let local_x = spawn.x.rem_euclid(edge);
    let yaw = if local_x <= edge * 0.5 {
        std::f32::consts::FRAC_PI_2
    } else {
        -std::f32::consts::FRAC_PI_2
    };
    instance
        .enqueue_headless_actions([look_frame(1, yaw, 0.0), idle_frame(2)])
        .expect("look frame enqueues");
    instance.advance_fixed_ticks(2).expect("look ticks advance");
    let mut generation = 3_u64;
    generation = enqueue_walk(&mut instance, generation, 1.0, u64::from(SETTLE));
    instance
        .advance_fixed_ticks(SETTLE)
        .expect("settle walk advances");
    let origin = chunk_from_translation(spine.player_pose().translation, spine.chunk_edge());
    let before = presentation_invariant_snapshot(&instance, &spine);
    let admissions = spine.stream_admission_count();
    let evictions = spine.stream_eviction_count();
    enqueue_walk(&mut instance, generation, 1.0, u64::from(HOLD));
    instance
        .advance_fixed_ticks(HOLD)
        .expect("intra-chunk walk advances");
    let after_chunk = chunk_from_translation(spine.player_pose().translation, spine.chunk_edge());
    let after = presentation_invariant_snapshot(&instance, &spine);
    assert_eq!(
        after_chunk,
        origin,
        "test must stay inside the same player chunk (start {origin:?}, end {after_chunk:?}, pose {:?})",
        spine.player_pose().translation
    );
    assert_eq!(
        spine.stream_admission_count(),
        admissions,
        "same-chunk motion must not admit by distance"
    );
    assert_eq!(
        spine.stream_eviction_count(),
        evictions,
        "same-chunk motion must not evict by distance"
    );
    assert_eq!(
        after.resident, before.resident,
        "same-chunk motion must not change the resident set"
    );
    for (chunk, receipt) in &before.receipts {
        assert_eq!(
            after.receipts.get(chunk),
            Some(receipt),
            "same-chunk motion must not remesh {chunk:?}"
        );
    }
}

#[test]
fn retain_keeps_former_core_after_immediate_boundary_reversal() {
    let mut instance =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let start = chunk_from_translation(spine.spawn_center(), spine.chunk_edge());
    let pose = spine.player_pose().translation;
    let x0 = pose.x.floor() as i32;
    let y0 = (pose.y - 0.9).floor() as i32;
    let z0 = pose.z.floor() as i32;
    for dx in 0..12 {
        for dy in 0..3 {
            let _ = mine_cover_cell(
                &spine,
                latticeaxiom_gameplay::BlockPosition {
                    x: x0.saturating_add(dx),
                    y: y0.saturating_add(dy),
                    z: z0,
                },
            );
        }
    }
    let mut generation =
        enqueue_look_then_walk(&mut instance, 1, std::f32::consts::FRAC_PI_2, 0.0, 1.0, 16);
    instance.advance_fixed_ticks(2).expect("look ticks advance");
    let mut crossed = start;
    for yaw in [std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_2] {
        generation = enqueue_look_then_walk(&mut instance, generation, yaw, 0.0, 1.0, 8);
        instance.advance_fixed_ticks(8).expect("turn ticks advance");
        for _ in 0..24 {
            instance
                .advance_fixed_ticks(8)
                .expect("boundary walk advances");
            crossed = chunk_from_translation(spine.player_pose().translation, spine.chunk_edge());
            if crossed.x != start.x || crossed.z != start.z {
                break;
            }
            generation = enqueue_walk(&mut instance, generation, 1.0, 8);
        }
        if crossed.x != start.x || crossed.z != start.z {
            break;
        }
    }
    assert!(
        crossed.x != start.x || crossed.z != start.z,
        "player must cross one chunk boundary before reversing (start {start:?}, pose {:?})",
        spine.player_pose().translation
    );
    let retained = spine
        .resident_chunks()
        .into_iter()
        .filter(|chunk| chunk.x.abs_diff(start.x).max(chunk.z.abs_diff(start.z)) <= 1)
        .collect::<BTreeSet<_>>();
    let receipts = retained
        .iter()
        .filter_map(|chunk| {
            spine
                .mesh_cursor(*chunk)
                .map(|cursor| (*chunk, (cursor.receipt(), cursor.revision())))
        })
        .collect::<BTreeMap<_, _>>();
    enqueue_look_then_walk(
        &mut instance,
        generation,
        -std::f32::consts::FRAC_PI_2,
        0.0,
        1.0,
        12,
    );
    instance
        .advance_fixed_ticks(14)
        .expect("immediate reversal advances");
    for chunk in &retained {
        assert!(
            spine.resident_chunks().contains(chunk),
            "retain must keep {chunk:?} resident after an immediate reversal"
        );
        if let Some(before) = receipts.get(chunk) {
            let after = spine
                .mesh_cursor(*chunk)
                .map(|cursor| (cursor.receipt(), cursor.revision()));
            assert_eq!(
                after.as_ref(),
                Some(before),
                "retain must not rematerialize {chunk:?}"
            );
        }
    }
}

#[test]
fn look_ahead_survives_zero_delta_idle_ticks() {
    let mut instance =
        EngineInstance::new_headless_host_from_lock(lock_boot_fixture().prepared(), SPINE_TIMESTEP)
            .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    enqueue_look_then_walk(&mut instance, 1, std::f32::consts::FRAC_PI_2, 0.0, 1.0, 12);
    instance
        .advance_fixed_ticks(14)
        .expect("look-ahead walk advances");
    assert_eq!(
        spine.interest_look_ahead(),
        [1, 0],
        "continuous +X motion must keep a sticky look-ahead axis"
    );
    let origin = chunk_from_translation(spine.player_pose().translation, spine.chunk_edge());
    let ahead = ChunkCoordinate::new(origin.x + 2, origin.y, origin.z);
    assert!(
        spine
            .resident_chunks()
            .iter()
            .any(|chunk| chunk.x == ahead.x && chunk.z == ahead.z),
        "look-ahead column {ahead:?} must be admitted before idle ticks"
    );
    instance
        .enqueue_headless_actions([idle_frame(20), idle_frame(21), idle_frame(22)])
        .expect("idle frames enqueue");
    instance.advance_fixed_ticks(3).expect("idle ticks advance");
    assert_eq!(
        spine.interest_look_ahead(),
        [1, 0],
        "a short zero-delta idle must not revoke look-ahead"
    );
    assert!(
        spine
            .resident_chunks()
            .iter()
            .any(|chunk| chunk.x == ahead.x && chunk.z == ahead.z),
        "look-ahead column must remain after a short idle"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn production_host_streams_past_v2_neighborhood_in_both_x_directions() {
    let boot = lock_boot_fixture();
    let images = boot.prepared();
    let mut instance = EngineInstance::new_headless_host_from_lock(images, SPINE_TIMESTEP)
        .expect("production spine starts from the reopened lock");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();

    let v2_neighborhood = [
        ChunkCoordinate::new(-1, 0, -1),
        ChunkCoordinate::new(-1, 0, 0),
        ChunkCoordinate::new(0, 0, -1),
        ChunkCoordinate::new(0, 0, 0),
    ];
    let spawn = spine.spawn_center();
    let spawn_chunk = chunk_from_translation(spawn, spine.chunk_edge());
    let edited = ChunkCoordinate::new(-1, 3, -1);
    assert_eq!(
        spine.chunk_lifecycle(edited),
        ChunkLifecycle::Active,
        "the spawn probe chunk starts active"
    );
    assert!(
        spine.edited_chunks().contains(&edited),
        "edited chunks must be pinned before the walk"
    );

    let mut generation = 1_u64;
    let mut seen_presentations = BTreeSet::new();
    let mut seen_player_chunks = BTreeSet::new();
    let mut min_y = spawn.y;
    record_stream_sample(
        &instance,
        &spine,
        &mut seen_presentations,
        &mut seen_player_chunks,
        &mut min_y,
    );

    generation = enqueue_look_then_walk(
        &mut instance,
        generation,
        std::f32::consts::FRAC_PI_2,
        0.0,
        1.0,
        720,
    );
    sample_walk(
        &mut instance,
        &spine,
        720,
        &mut seen_presentations,
        &mut seen_player_chunks,
        &mut min_y,
    );
    let plus_x = seen_player_chunks
        .iter()
        .map(|chunk| chunk.x)
        .max()
        .expect("player visited +X chunks");
    let pose_after_plus = spine.player_pose();
    assert!(
        plus_x > 0,
        "walk +X must leave the V2 neighborhood (spawn {spawn_chunk:?}, max x {plus_x}, pose {:?}, yaw {}, min_y {min_y}, presented {:?}, resident {:?}, error {:?})",
        pose_after_plus.translation,
        pose_after_plus.yaw_radians,
        seen_presentations
            .iter()
            .map(|chunk| chunk.x)
            .collect::<BTreeSet<_>>(),
        spine
            .resident_chunks()
            .iter()
            .map(|chunk| chunk.x)
            .collect::<BTreeSet<_>>(),
        spine.last_stream_error()
    );
    enqueue_look_then_walk(
        &mut instance,
        generation,
        -std::f32::consts::PI,
        0.0,
        1.0,
        1_040,
    );
    sample_walk(
        &mut instance,
        &spine,
        1_040,
        &mut seen_presentations,
        &mut seen_player_chunks,
        &mut min_y,
    );
    let minus_x = seen_player_chunks
        .iter()
        .map(|chunk| chunk.x)
        .min()
        .expect("player visited -X chunks");

    let current = presented_chunks(&instance);
    let current_xs = current.iter().map(|chunk| chunk.x).collect::<BTreeSet<_>>();
    let seen_xs = seen_presentations
        .iter()
        .map(|chunk| chunk.x)
        .collect::<BTreeSet<_>>();
    assert!(
        plus_x > 0,
        "walk +X must leave the V2 neighborhood (spawn {spawn_chunk:?}, max x {plus_x})"
    );
    assert!(
        minus_x < -1,
        "walk -X must leave the V2 neighborhood (spawn {spawn_chunk:?}, min x {minus_x})"
    );
    assert!(
        seen_presentations.len() > v2_neighborhood.len(),
        "streaming must activate more than the V2 four-chunk neighborhood, got {}",
        seen_presentations.len()
    );
    assert!(
        seen_xs.iter().any(|x| *x >= 2) && seen_xs.iter().any(|x| *x <= -3),
        "presented chunks must exist beyond the authored V2 x range, got {seen_xs:?}"
    );
    assert!(
        min_y > 8.0,
        "the player must stay on generated ground without an authored world edge, min y {min_y}"
    );
    assert!(
        current.len() < seen_presentations.len(),
        "clean generated chunks must be evicted after the player walks away (current {}, seen {})",
        current.len(),
        seen_presentations.len()
    );
    assert!(
        !current_xs.contains(&plus_x) || current.len() < seen_presentations.len(),
        "the working set must not retain every visited chunk"
    );
    assert!(
        spine.edited_chunks().contains(&edited)
            && spine.resident_chunks().contains(&edited)
            && spine.chunk_lifecycle(edited) != ChunkLifecycle::Absent,
        "dirty edited chunks must not be evicted"
    );
    assert!(
        v2_neighborhood.iter().any(|chunk| !current.contains(chunk)
            || spine.chunk_lifecycle(*chunk) == ChunkLifecycle::Absent),
        "clean origin-neighborhood chunks may leave the working set"
    );
}

#[test]
fn start_ui_create_play_continue_preserves_in_memory_world_id() {
    let images = lock_boot_fixture().prepared();
    let mut start = ProductionMemoryStart::new(images, start_shell_graph());
    let intent = start
        .quick_create_intent("Memory Session")
        .expect("quick-create intent binds the lock graph root");
    start.set_draft(intent);
    start.set_now_ms(10);

    start
        .inject(&SemanticCommand {
            target: semantic_id("home/new-world"),
            action: SemanticActionId::Activate,
            source: InputSource::Headless,
        })
        .expect("new-world route opens");
    let created = match start
        .inject(&SemanticCommand {
            target: semantic_id("new-world/quick-create"),
            action: SemanticActionId::Activate,
            source: InputSource::Headless,
        })
        .expect("quick-create publishes an in-memory world")
    {
        MemoryStartEffect::Created(world_id) => world_id,
        other @ MemoryStartEffect::Shell(_) => panic!("expected created world, got {other:?}"),
    };
    assert_eq!(start.continue_world_id(), Some(created));

    let (continued, mut instance) = start
        .play_continued_headless(20, SPINE_TIMESTEP)
        .expect("continue materializes the in-memory session");
    assert_eq!(continued, created);
    instance
        .advance_fixed_ticks(1)
        .expect("one production tick plays");
    assert_eq!(instance.completed_fixed_ticks(), 1);

    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    assert_eq!(spine.world_id(), Some(created));
    let list = instance
        .app()
        .world()
        .get_resource::<ProductionWorldList>()
        .expect("in-memory world list is installed on the host");
    assert_eq!(list.continue_world_id(), Some(created));

    let effect = start
        .inject(&SemanticCommand {
            target: semantic_id("home/continue"),
            action: SemanticActionId::ContinueWorld,
            source: InputSource::Headless,
        })
        .expect("continue remains available after one tick");
    assert_eq!(
        effect,
        MemoryStartEffect::Shell(ShellEffect::RequestExactWorldLaunch(created))
    );
    assert_eq!(start.continue_world_id(), Some(created));
}

#[test]
fn start_ui_create_fills_activation_binding_from_shared_storage_preflight() {
    let images = lock_boot_fixture().prepared();
    let record_owner = "latticeaxiom:schema/world-db-chunk@1"
        .parse()
        .expect("fixture record owner is canonical");
    let writer_host =
        SealedWorldWriterHost::volatile_reference_with_default_publisher(record_owner);
    let mut start = ProductionMemoryStart::new(images, start_shell_graph())
        .with_storage(writer_host.storage().clone());
    let intent = start
        .quick_create_intent("Memory Session")
        .expect("quick-create intent binds the lock graph root");
    let created = start
        .create(&intent, 10)
        .expect("create provisions shared storage and publishes the WorldId");
    assert_eq!(start.continue_world_id(), Some(created));
    let plan = start
        .flow()
        .worlds()
        .get(created)
        .and_then(|record| record.open_plan.as_ref())
        .expect("created session publishes an open plan");
    assert!(
        plan.activation_binding.is_some(),
        "storage preflight must bind catalog activation evidence"
    );
    start
        .storage()
        .expect("shared storage remains attached")
        .preflight(created)
        .expect("provisioned world remains readable to preflight");
}

#[test]
fn start_ui_create_without_storage_keeps_memory_only_open_plan() {
    let images = lock_boot_fixture().prepared();
    let mut start = ProductionMemoryStart::new(images, start_shell_graph());
    let intent = start
        .quick_create_intent("Memory Session")
        .expect("quick-create intent binds the lock graph root");
    let created = start
        .create(&intent, 10)
        .expect("memory-only create publishes a WorldId");
    let plan = start
        .flow()
        .worlds()
        .get(created)
        .and_then(|record| record.open_plan.as_ref())
        .expect("created session publishes an open plan");
    assert!(plan.activation_binding.is_none());
    assert!(start.storage().is_none());
}

#[test]
#[allow(clippy::too_many_lines)]
fn start_ui_break_place_flush_reopens_edited_cell_from_world_db() {
    let images = lock_boot_fixture().prepared();
    let record_owner = "latticeaxiom:schema/world-db-chunk@1"
        .parse()
        .expect("fixture record owner is canonical");
    let mut writer_host =
        SealedWorldWriterHost::volatile_reference_with_default_publisher(record_owner);
    let mut start = ProductionMemoryStart::new(images, start_shell_graph())
        .with_storage(writer_host.storage().clone());
    let intent = start
        .quick_create_intent("Reopen Session")
        .expect("quick-create intent binds the lock graph root");
    let created = start
        .create(&intent, 10)
        .expect("create provisions shared storage and publishes the WorldId");

    let mut instance = start
        .play_headless(created, 20, SPINE_TIMESTEP)
        .expect("first play materializes the provisioned world");
    instance
        .advance_fixed_ticks(1)
        .expect("one production tick plays");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    assert_eq!(spine.world_id(), Some(created));

    let (dirt, broken_pos, _) = first_resident_soil(&spine);
    let occupancy_before = spine
        .inspect_occupancy(broken_pos)
        .expect("soil cell is inspectable before the break");
    assert_eq!(occupancy_before.solid.as_ref(), Some(&dirt));
    let broken = mine_until_broken(&spine, broken_pos);
    pickup_remaining(&spine);
    let occupancy_gone = spine
        .inspect_occupancy(broken.position)
        .expect("broken cell remains inspectable");
    assert_ne!(
        occupancy_gone.solid.as_ref(),
        Some(&dirt),
        "break must clear the soil cell before flush"
    );

    let (dirt, place_target, dirt_item) = first_resident_soil(&spine);
    mine_until_broken(&spine, place_target);
    pickup_remaining(&spine);
    select_item_in_hotbar(&spine, &dirt_item);
    let place_anchor = latticeaxiom_gameplay::BlockPosition {
        x: place_target.x,
        y: place_target.y.saturating_add(1),
        z: place_target.z,
    };
    let placed = spine
        .place_from_hotbar(place_anchor, BlockFaceV1::NegativeY)
        .expect("placement from the hotbar consumes gathered dirt");
    let occupancy_placed = spine
        .inspect_occupancy(placed.position)
        .expect("placed cell is inspectable");
    assert_eq!(occupancy_placed.solid.as_ref(), Some(&dirt));
    assert_ne!(
        placed.position, broken.position,
        "placed cell must stay distinct from the broken cell"
    );

    let mut missing_binding = start
        .flow()
        .worlds()
        .get(created)
        .and_then(|record| record.open_plan.clone())
        .expect("created session publishes an open plan");
    missing_binding.activation_binding = None;
    assert!(
        matches!(
            writer_host.accept(&missing_binding, WorldOpenAction::UseFrozenLock),
            Err(SealedWriterHostError::WorldDb(
                WorldDbError::ActivationEvidenceUnavailable { world }
            )) if world == created
        ),
        "permit-only frozen-lock accept must fail closed without a receipt"
    );

    start
        .flush_dirty_chunks(created, &mut writer_host)
        .expect("edited chunks flush through the sealed host writer");
    drop(spine);
    drop(instance);

    let mut reopened = start
        .play_reopened_headless(created, 30, SPINE_TIMESTEP)
        .expect("reopen constructs a new host from the same storage and WorldId");
    reopened
        .advance_fixed_ticks(1)
        .expect("reopened host advances one tick");
    let reopened_spine = reopened
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("reopened production spine is installed")
        .clone();
    assert_eq!(reopened_spine.world_id(), Some(created));
    let gone = reopened_spine
        .inspect_occupancy(broken.position)
        .expect("broken cell is resident after storage-first reopen");
    assert_eq!(gone.solid, occupancy_gone.solid);
    let restored_place = reopened_spine
        .inspect_occupancy(placed.position)
        .expect("placed cell is resident after storage-first reopen");
    assert_eq!(restored_place.solid, occupancy_placed.solid);
}

#[test]
#[allow(clippy::too_many_lines)]
fn start_ui_pause_save_exit_continue_reopens_sealed_world_from_storage() {
    let images = lock_boot_fixture().prepared();
    let record_owner = "latticeaxiom:schema/world-db-chunk@1"
        .parse()
        .expect("fixture record owner is canonical");
    let mut writer_host =
        SealedWorldWriterHost::volatile_reference_with_default_publisher(record_owner);
    let mut start = ProductionMemoryStart::new(images, start_shell_graph())
        .with_storage(writer_host.storage().clone());
    let intent = start
        .quick_create_intent("Pause Save Session")
        .expect("quick-create intent binds the lock graph root");
    let created = start
        .create(&intent, 10)
        .expect("create provisions shared storage and publishes the WorldId");

    let mut instance = start
        .play_headless(created, 20, SPINE_TIMESTEP)
        .expect("first play materializes the provisioned world");
    instance
        .advance_fixed_ticks(1)
        .expect("one production tick plays");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    assert_eq!(spine.world_id(), Some(created));
    assert_eq!(start.flow().shell().screen, ShellScreen::Home);

    let (dirt, broken_pos, _) = first_resident_soil(&spine);
    let occupancy_before = spine
        .inspect_occupancy(broken_pos)
        .expect("soil cell is inspectable before the break");
    assert_eq!(occupancy_before.solid.as_ref(), Some(&dirt));
    let broken = mine_until_broken(&spine, broken_pos);
    pickup_remaining(&spine);
    let occupancy_gone = spine
        .inspect_occupancy(broken.position)
        .expect("broken cell remains inspectable");
    assert_ne!(
        occupancy_gone.solid.as_ref(),
        Some(&dirt),
        "break must clear the soil cell before save"
    );

    let (dirt, place_target, dirt_item) = first_resident_soil(&spine);
    mine_until_broken(&spine, place_target);
    pickup_remaining(&spine);
    select_item_in_hotbar(&spine, &dirt_item);
    let place_anchor = latticeaxiom_gameplay::BlockPosition {
        x: place_target.x,
        y: place_target.y.saturating_add(1),
        z: place_target.z,
    };
    let placed = spine
        .place_from_hotbar(place_anchor, BlockFaceV1::NegativeY)
        .expect("placement from the hotbar consumes gathered dirt");
    let occupancy_placed = spine
        .inspect_occupancy(placed.position)
        .expect("placed cell is inspectable");
    assert_eq!(occupancy_placed.solid.as_ref(), Some(&dirt));

    let hash_before_pause = spine
        .materialized_chunk_state_hash()
        .expect("playing session exposes a world hash");
    let pause = start
        .pause_session(&mut instance)
        .expect("pause opens the overlay without writing");
    assert_eq!(
        pause,
        MemoryStartEffect::Shell(ShellEffect::Navigate(ShellScreen::Pause))
    );
    assert!(
        instance
            .app()
            .world()
            .get_resource::<ProductionSessionPause>()
            .is_some_and(|pause| pause.is_paused()),
        "pause latch must freeze streaming"
    );
    instance
        .advance_fixed_ticks(1)
        .expect("paused host can still tick");
    assert_eq!(
        spine
            .materialized_chunk_state_hash()
            .expect("paused session still exposes a world hash"),
        hash_before_pause,
        "pause must not mutate the materialized-chunk world hash"
    );

    let mut missing_binding = start
        .flow()
        .worlds()
        .get(created)
        .and_then(|record| record.open_plan.clone())
        .expect("created session publishes an open plan");
    missing_binding.activation_binding = None;
    assert!(
        matches!(
            writer_host.accept(&missing_binding, WorldOpenAction::UseFrozenLock),
            Err(SealedWriterHostError::WorldDb(
                WorldDbError::ActivationEvidenceUnavailable { world }
            )) if world == created
        ),
        "permit-only frozen-lock accept must fail closed without a receipt"
    );

    let save = start
        .save_world(created, &mut writer_host)
        .expect("save flushes dirty chunks through the sealed host writer");
    assert_eq!(
        save,
        MemoryStartEffect::Shell(ShellEffect::RequestSaveWorld)
    );
    assert!(
        !writer_host.is_writer_active(),
        "save must close the sealed writer"
    );
    assert_eq!(start.flow().shell().screen, ShellScreen::Pause);

    let exit = start
        .exit_world(created, instance)
        .expect("exit returns to the start shell");
    assert_eq!(
        exit,
        MemoryStartEffect::Shell(ShellEffect::RequestExitWorld)
    );
    assert_eq!(start.flow().shell().screen, ShellScreen::Home);
    assert_eq!(start.continue_world_id(), Some(created));

    let (continued, mut reopened) = start
        .play_continued_headless(30, SPINE_TIMESTEP)
        .expect("continue reopens storage-first after save and exit");
    assert_eq!(continued, created);
    reopened
        .advance_fixed_ticks(1)
        .expect("continued host advances one tick");
    let reopened_spine = reopened
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("continued production spine is installed")
        .clone();
    assert_eq!(reopened_spine.world_id(), Some(created));
    let gone = reopened_spine
        .inspect_occupancy(broken.position)
        .expect("broken cell is resident after storage-first continue");
    assert_eq!(gone.solid, occupancy_gone.solid);
    let restored_place = reopened_spine
        .inspect_occupancy(placed.position)
        .expect("placed cell is resident after storage-first continue");
    assert_eq!(restored_place.solid, occupancy_placed.solid);
}

#[test]
fn home_preflight_game_save_and_quit_returns_home() {
    let images = lock_boot_fixture().prepared();
    let record_owner = "latticeaxiom:schema/world-db-chunk@1"
        .parse()
        .expect("fixture record owner is canonical");
    let mut writer_host =
        SealedWorldWriterHost::volatile_reference_with_default_publisher(record_owner);
    let mut start = ProductionMemoryStart::new(images, start_shell_graph())
        .with_storage(writer_host.storage().clone());
    start.set_now_ms(10);
    let intent = start
        .quick_create_intent("Product Loop")
        .expect("quick-create binds the lock graph root");
    start.set_draft(intent);
    let created = match start
        .inject(&SemanticCommand {
            target: semantic_id("home/new-world"),
            action: SemanticActionId::QuickCreate,
            source: InputSource::Headless,
        })
        .expect("home navigates to new-world")
    {
        MemoryStartEffect::Shell(ShellEffect::Navigate(ShellScreen::NewWorld)) => start
            .inject(&SemanticCommand {
                target: semantic_id("new-world/quick-create"),
                action: SemanticActionId::Activate,
                source: InputSource::Headless,
            })
            .expect("preflight publishes a world"),
        MemoryStartEffect::Created(_) | MemoryStartEffect::Shell(_) => {
            panic!("expected new-world navigation")
        }
    };
    let created = match created {
        MemoryStartEffect::Created(world_id) => world_id,
        MemoryStartEffect::Shell(effect) => panic!("expected created world, got {effect:?}"),
    };
    let mut instance = start
        .play_headless(created, 20, SPINE_TIMESTEP)
        .expect("game host starts from the same lock");
    start.pause_session(&mut instance).expect("pause");
    start
        .save_and_quit(created, instance, &mut writer_host)
        .expect("Save & Quit");
    assert_eq!(start.flow().shell().screen, ShellScreen::Home);
    assert_eq!(start.continue_world_id(), Some(created));
}

#[test]
#[allow(clippy::too_many_lines)]
fn durable_save_and_quit_returns_child_result_and_reopens_edits_and_inventory() {
    let images = lock_boot_fixture().prepared();
    let record_owner = "latticeaxiom:schema/world-db-chunk@1"
        .parse()
        .expect("fixture record owner is canonical");
    let mut writer_host =
        SealedWorldWriterHost::durable_reference_with_default_publisher(record_owner);
    let mut start = ProductionMemoryStart::new(images, start_shell_graph())
        .with_storage(writer_host.storage().clone());
    start.set_now_ms(10);
    let intent = start
        .quick_create_intent("Durable Loop")
        .expect("quick-create binds the lock graph root");
    let created = start
        .create(&intent, 10)
        .expect("create provisions durable storage");
    let mut instance = start
        .play_headless(created, 20, SPINE_TIMESTEP)
        .expect("game host starts from the same lock");
    instance
        .advance_fixed_ticks(1)
        .expect("one production tick plays");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();

    let (dirt, broken_pos, dirt_item) = first_resident_soil(&spine);
    let occupancy_before = spine
        .inspect_occupancy(broken_pos)
        .expect("soil cell is inspectable before the break");
    assert_eq!(occupancy_before.solid.as_ref(), Some(&dirt));
    let broken = mine_until_broken(&spine, broken_pos);
    pickup_remaining(&spine);
    let occupancy_gone = spine
        .inspect_occupancy(broken.position)
        .expect("broken cell remains inspectable");
    assert_ne!(
        occupancy_gone.solid.as_ref(),
        Some(&dirt),
        "break must clear the soil cell before save"
    );
    select_item_in_hotbar(&spine, &dirt_item);
    let inventory_before = spine
        .inventory_view()
        .expect("playing session exposes inventory");
    let dirt_count = inventory_before.count_item(&dirt_item);
    assert!(dirt_count > 0, "gathered dirt must remain in inventory");
    let selected_slot = inventory_before.hotbar_slot();

    start.pause_session(&mut instance).expect("pause");
    let result = start
        .save_and_quit_durable(created, instance, &mut writer_host)
        .expect("durable Save & Quit");
    assert_eq!(result.report().exit_kind(), ChildExitKindV1::SaveAndQuit);
    assert!(result.report().last_durable_world().is_some());
    assert!(result.checkpoint().restore_verified());
    assert_eq!(start.flow().shell().screen, ShellScreen::Home);
    assert_eq!(start.continue_world_id(), Some(created));
    assert!(
        !writer_host.is_writer_active(),
        "Save & Quit must close the sealed writer"
    );

    let (continued, mut reopened) = start
        .play_continued_headless(30, SPINE_TIMESTEP)
        .expect("continue reopens storage-first after durable Save & Quit");
    assert_eq!(continued, created);
    reopened
        .advance_fixed_ticks(1)
        .expect("continued host advances one tick");
    let reopened_spine = reopened
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("continued production spine is installed")
        .clone();
    let gone = reopened_spine
        .inspect_occupancy(broken.position)
        .expect("broken cell is resident after durable continue");
    assert_eq!(gone.solid, occupancy_gone.solid);
    let restored_inventory = reopened_spine
        .inventory_view()
        .expect("reopened session restores inventory");
    assert_eq!(restored_inventory.count_item(&dirt_item), dirt_count);
    assert_eq!(restored_inventory.hotbar_slot(), selected_slot);
}

fn start_shell_graph() -> ClientShellGraph {
    ClientShellGraph::resolve([
        ShellPackageProvider {
            package: package_name("@latticeaxiom/front-end"),
            capability: ShellCapability::ClientShell,
        },
        ShellPackageProvider {
            package: package_name("@latticeaxiom/world-library"),
            capability: ShellCapability::WorldCatalog,
        },
        ShellPackageProvider {
            package: package_name("@latticeaxiom/settings-ui"),
            capability: ShellCapability::SettingsSurface,
        },
        ShellPackageProvider {
            package: package_name("@latticeaxiom/settings"),
            capability: ShellCapability::SettingsRegistry,
        },
        ShellPackageProvider {
            package: package_name("@latticeaxiom/observability"),
            capability: ShellCapability::DiagnosticRegistry,
        },
    ])
    .expect("start-ui shell graph resolves")
}

fn semantic_id(value: &str) -> SemanticNodeId {
    SemanticNodeId::new(value).expect("semantic ID fixture is valid")
}

fn assert_inspect_dto_overlay_fields(inspect: &HeadlessTargetInspectV1, spine: &ProductionSpine) {
    let expected = spine.content_display(inspect.block_id.as_str());
    assert!(
        !inspect.block_display_name.is_empty(),
        "inspect DTO must carry a targeted block display name"
    );
    assert_ne!(inspect.block_display_name, inspect.block_id.as_str());
    assert_eq!(inspect.block_display_name, expected.name);
    assert_eq!(inspect.block_display_icon, expected.icon);
    assert!(
        !inspect.block_display_icon.is_empty(),
        "inspect DTO must carry a targeted block icon"
    );
    assert_ne!(inspect.block_display_icon, inspect.block_id.as_str());
    assert_eq!(
        inspect.chunk,
        spine
            .chunk_of(inspect.observation.position)
            .expect("targeted voxel maps to a host chunk")
    );
    assert!(
        inspect.resident > 0,
        "inspect DTO must snapshot resident occupancy"
    );
    assert!(
        inspect.active <= inspect.resident,
        "inspect DTO active {} exceeds resident {}",
        inspect.active,
        inspect.resident
    );
    assert!(
        inspect.dirty >= 1,
        "inspect DTO must snapshot dirty occupancy"
    );
    assert_eq!(
        inspect.occupancy_line(),
        format!(
            "r{} a{} i{} d{}",
            inspect.resident, inspect.active, inspect.in_flight, inspect.dirty
        )
    );
    let overlay = inspect.overlay_lines();
    assert!(
        overlay.contains(&inspect.block_display_name),
        "player overlay must include the display name, got {overlay}"
    );
    assert!(
        overlay.contains(&inspect.declared_by),
        "player overlay must include declared-by, got {overlay}"
    );
    assert!(
        overlay.contains(inspect.block_id.as_str()),
        "player overlay must include the stable block id, got {overlay}"
    );
    assert!(
        !overlay.contains(&inspect.occupancy_line()),
        "player overlay must omit occupancy, got {overlay}"
    );
    assert!(
        !overlay.contains(&inspect.chunk_line()),
        "player overlay must omit chunk coordinates, got {overlay}"
    );
}

fn assert_working_set_diagnostics(
    snapshot: WorkingSetDiagnosticsV1,
    limits: latticeaxiom_compose::PlayableWorldHardLimitsV1,
) {
    assert!(
        snapshot.resident() > 0,
        "spawn neighborhood must keep committed projections resident"
    );
    assert!(
        snapshot.resident() <= limits.max_resident_chunks,
        "resident {} exceeds clamp {}",
        snapshot.resident(),
        limits.max_resident_chunks
    );
    assert!(
        snapshot.active() <= snapshot.resident(),
        "active {} exceeds resident {}",
        snapshot.active(),
        snapshot.resident()
    );
    assert!(
        snapshot.visible() <= snapshot.resident(),
        "visible {} exceeds resident {}",
        snapshot.visible(),
        snapshot.resident()
    );
    assert!(
        snapshot.in_flight() <= limits.max_in_flight_chunks,
        "in-flight {} exceeds clamp {}",
        snapshot.in_flight(),
        limits.max_in_flight_chunks
    );
    assert!(
        snapshot.dirty() >= 1,
        "the spawn probe edit must stay dirty and pinned"
    );
    assert_eq!(
        snapshot.saving(),
        0,
        "production host must not open a world writer"
    );
    assert_eq!(snapshot.byte_budget(), 32 * 1024 * 1024);
    assert!(
        snapshot.reserved_bytes() <= snapshot.byte_budget(),
        "reserved {} exceeds budget {}",
        snapshot.reserved_bytes(),
        snapshot.byte_budget()
    );
}

fn idle_frame(generation: u64) -> PlayerActionFrameV1 {
    PlayerActionFrameV1 {
        generation,
        ..PlayerActionFrameV1::default()
    }
}

fn look_frame(generation: u64, yaw: f32, pitch: f32) -> PlayerActionFrameV1 {
    PlayerActionFrameV1 {
        generation,
        look_radians: ActionAxis2V1 { x: yaw, y: pitch },
        ..PlayerActionFrameV1::default()
    }
}

fn break_frame(generation: u64) -> PlayerActionFrameV1 {
    let mut started = PlayerActionButtonsV1::empty();
    started.insert(PlayerActionV1::BreakBlock);
    PlayerActionFrameV1 {
        generation,
        started,
        ..PlayerActionFrameV1::default()
    }
}

fn inspect_frame(generation: u64) -> PlayerActionFrameV1 {
    let mut started = PlayerActionButtonsV1::empty();
    started.insert(PlayerActionV1::Inspect);
    PlayerActionFrameV1 {
        generation,
        started,
        ..PlayerActionFrameV1::default()
    }
}

fn pick_block_frame(generation: u64) -> PlayerActionFrameV1 {
    let mut started = PlayerActionButtonsV1::empty();
    started.insert(PlayerActionV1::PickBlock);
    PlayerActionFrameV1 {
        generation,
        started,
        ..PlayerActionFrameV1::default()
    }
}

fn first_direct_fluid_cell(
    spine: &ProductionSpine,
    positive_xz: bool,
) -> latticeaxiom_gameplay::BlockPosition {
    let edge = i32::from(spine.chunk_edge());
    for coordinate in spine.resident_chunks() {
        for ly in (0..edge).rev() {
            for lz in 0..edge {
                for lx in 0..edge {
                    let position = latticeaxiom_gameplay::BlockPosition {
                        x: coordinate.x.saturating_mul(edge).saturating_add(lx),
                        y: coordinate.y.saturating_mul(edge).saturating_add(ly),
                        z: coordinate.z.saturating_mul(edge).saturating_add(lz),
                    };
                    let xz_ok = if positive_xz {
                        position.x > 0 && position.z > 0
                    } else {
                        position.x < 0 && position.z < 0
                    };
                    if !xz_ok {
                        continue;
                    }
                    let Ok(occupancy) = spine.inspect_occupancy(position) else {
                        continue;
                    };
                    if occupancy.fluid.is_none()
                        && occupancy.fluid_occupancy.as_str() == "terrenia:fluid-occupancy/direct@1"
                    {
                        return position;
                    }
                }
            }
        }
    }
    panic!(
        "no direct fluid-occupancy air cell with {} XZ in {:?}",
        if positive_xz { "positive" } else { "negative" },
        spine.resident_chunks()
    );
}

fn place_and_inspect_fluid(
    spine: &ProductionSpine,
    position: latticeaxiom_gameplay::BlockPosition,
    fluid: &latticeaxiom_core::StableId,
    state: FluidStateV1,
) -> CellOccupancyV1 {
    let placed = spine
        .place_fluid_occupancy(position, fluid, state)
        .unwrap_or_else(|reject| panic!("{fluid} occupancy must place, reject={reject:?}"));
    assert_fluid_occupancy(&placed, position, fluid, state);
    let inspected = spine
        .inspect_occupancy(position)
        .unwrap_or_else(|reject| panic!("{fluid} occupancy must inspect, reject={reject:?}"));
    assert_eq!(inspected, placed);
    placed
}

fn assert_occupancy_payloads(
    spine: &ProductionSpine,
    positions: &[latticeaxiom_gameplay::BlockPosition],
) {
    let world_id = spine.world_id().expect("spine owns a world identity");
    let snapshot = spine
        .kernel()
        .reference_snapshot(world_id)
        .expect("memory kernel exposes a reference snapshot");
    let expected = usize::from(spine.chunk_edge()).pow(3).saturating_mul(4);
    for position in positions {
        let coordinate = spine
            .chunk_of(*position)
            .unwrap_or_else(|| panic!("{position:?} maps to a host chunk"));
        let stored = snapshot
            .chunks()
            .find(|(key, _)| key.coordinate == coordinate)
            .map_or_else(
                || panic!("committed occupancy chunk {coordinate:?} is missing"),
                |(_, stored)| stored,
            );
        assert_eq!(
            stored.data().voxels().bytes().len(),
            expected,
            "occupancy payload must round-trip solid and fluid indices"
        );
    }
}

fn assert_fluid_occupancy(
    occupancy: &CellOccupancyV1,
    position: latticeaxiom_gameplay::BlockPosition,
    fluid: &latticeaxiom_core::StableId,
    state: FluidStateV1,
) {
    assert_eq!(occupancy.position, position);
    assert_eq!(
        occupancy.solid_occupancy.as_str(),
        "terrenia:solid-occupancy/empty@1"
    );
    assert_eq!(
        occupancy.fluid_occupancy.as_str(),
        "terrenia:fluid-occupancy/direct@1"
    );
    assert_eq!(occupancy.fluid.as_ref(), Some(fluid));
    assert_eq!(occupancy.fluid_state, Some(state));
    assert_eq!(
        occupancy.collision_policy.as_str(),
        if fluid.as_str() == "terrenia:fluid/lava" {
            "terrenia:fluid-collision-policy/lava@1"
        } else {
            "terrenia:fluid-collision-policy/water@1"
        }
    );
    assert_eq!(
        occupancy.selection_policy.as_str(),
        if fluid.as_str() == "terrenia:fluid/lava" {
            "terrenia:fluid-selection-policy/lava@1"
        } else {
            "terrenia:fluid-selection-policy/water@1"
        }
    );
}

fn enqueue_walk(
    instance: &mut EngineInstance,
    start_generation: u64,
    forward: f32,
    walk_ticks: u64,
) -> u64 {
    let frames = (0..walk_ticks)
        .map(|offset| PlayerActionFrameV1 {
            generation: start_generation + offset,
            movement: ActionAxis2V1 { x: 0.0, y: forward },
            ..PlayerActionFrameV1::default()
        })
        .collect::<Vec<_>>();
    instance
        .enqueue_headless_actions(frames)
        .expect("walk frames enqueue");
    start_generation + walk_ticks
}

fn enqueue_look_then_walk(
    instance: &mut EngineInstance,
    start_generation: u64,
    yaw: f32,
    pitch: f32,
    forward: f32,
    walk_ticks: u64,
) -> u64 {
    let mut frames = vec![
        look_frame(start_generation, yaw, pitch),
        idle_frame(start_generation + 1),
    ];
    frames.extend((0..walk_ticks).map(|offset| {
        let mut started = PlayerActionButtonsV1::empty();
        if offset.is_multiple_of(18) {
            started.insert(PlayerActionV1::Jump);
        }
        PlayerActionFrameV1 {
            generation: start_generation + 2 + offset,
            movement: ActionAxis2V1 { x: 0.0, y: forward },
            started,
            ..PlayerActionFrameV1::default()
        }
    }));
    instance
        .enqueue_headless_actions(frames)
        .expect("walk frames enqueue");
    start_generation + 2 + walk_ticks
}

fn sample_walk(
    instance: &mut EngineInstance,
    spine: &ProductionSpine,
    ticks: u32,
    seen_presentations: &mut BTreeSet<ChunkCoordinate>,
    seen_player_chunks: &mut BTreeSet<ChunkCoordinate>,
    min_y: &mut f32,
) {
    let mut remaining = ticks;
    while remaining > 0 {
        let step = remaining.min(64);
        instance
            .advance_fixed_ticks(step)
            .expect("walk ticks advance");
        remaining -= step;
        record_stream_sample(
            instance,
            spine,
            seen_presentations,
            seen_player_chunks,
            min_y,
        );
    }
}

fn record_stream_sample(
    instance: &EngineInstance,
    spine: &ProductionSpine,
    seen_presentations: &mut BTreeSet<ChunkCoordinate>,
    seen_player_chunks: &mut BTreeSet<ChunkCoordinate>,
    min_y: &mut f32,
) {
    seen_presentations.extend(presented_chunks(instance));
    let pose = spine.player_pose();
    *min_y = min_y.min(pose.translation.y);
    seen_player_chunks.insert(chunk_from_translation(pose.translation, spine.chunk_edge()));
}

fn presented_chunks(instance: &EngineInstance) -> BTreeSet<ChunkCoordinate> {
    instance
        .app()
        .world()
        .iter_entities()
        .filter_map(|entity| {
            entity
                .get::<ChunkPresentation>()
                .map(|chunk| chunk.coordinate)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
struct PresentationInvariantSnapshot {
    resident: BTreeSet<ChunkCoordinate>,
    lifecycles: BTreeMap<ChunkCoordinate, ChunkLifecycle>,
    receipts: BTreeMap<ChunkCoordinate, (MeshReceipt, ChunkRevision)>,
    entity_count: usize,
    diagnostics: WorkingSetDiagnosticsV1,
}

fn presentation_invariant_snapshot(
    instance: &EngineInstance,
    spine: &ProductionSpine,
) -> PresentationInvariantSnapshot {
    let resident = spine.resident_chunks();
    let lifecycles = resident
        .iter()
        .map(|chunk| (*chunk, spine.chunk_lifecycle(*chunk)))
        .collect();
    let receipts = resident
        .iter()
        .filter_map(|chunk| {
            spine
                .mesh_cursor(*chunk)
                .map(|cursor| (*chunk, (cursor.receipt(), cursor.revision())))
        })
        .collect();
    PresentationInvariantSnapshot {
        resident,
        lifecycles,
        receipts,
        entity_count: instance.production_chunk_entity_count(),
        diagnostics: spine.working_set_diagnostics(),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn chunk_from_translation(translation: bevy::prelude::Vec3, edge: u16) -> ChunkCoordinate {
    let edge = f32::from(edge);
    ChunkCoordinate::new(
        (translation.x / edge).floor() as i32,
        (translation.y / edge).floor() as i32,
        (translation.z / edge).floor() as i32,
    )
}

fn place_frame(generation: u64, spine: &ProductionSpine) -> PlayerActionFrameV1 {
    let mut started = PlayerActionButtonsV1::empty();
    started.insert(PlayerActionV1::PlaceBlock);
    PlayerActionFrameV1 {
        generation,
        started,
        placement_content: spine.placement_content(),
        ..PlayerActionFrameV1::default()
    }
}

#[test]
fn missing_lock_refuses_before_headless_app() {
    let boot = lock_boot_fixture();
    let missing = boot.directory.path().join("absent.lock");
    match ReopenedFinalLockV1::reopen_frozen(&missing, &boot.objects, &boot.host) {
        Err(ProductLockBootError::ProductLock(ProductLockError::MissingLock { path })) => {
            assert_eq!(path, missing);
        }
        other => panic!("missing lock must fail before App construction, got {other:?}"),
    }
}

#[test]
fn tampered_source_refuses_before_headless_app() {
    let mut boot = lock_boot_fixture();
    boot.objects.sources.clear();
    match ReopenedFinalLockV1::reopen_frozen(boot.lock_path(), &boot.objects, &boot.host) {
        Err(ProductLockBootError::ProductLock(ProductLockError::MissingReceipt {
            receipt: ProductLockReceiptKind::Source,
            ..
        })) => {}
        other => panic!("missing source must fail before App construction, got {other:?}"),
    }
}

#[test]
fn tampered_registration_refuses_before_headless_app() {
    let boot = lock_boot_fixture();
    let mut compiled = boot.compiled;
    compiled.callback_receipt.callback_map_hash = CanonicalHash::digest(b"tampered-callback-map");
    assert!(matches!(
        LockVerifiedComposeImages::from_reopened_lock(
            &boot.reopened,
            &boot.target,
            boot.graph,
            compiled,
        ),
        Err(PreparationError::CompiledRegistration(_))
    ));
}

fn fixed_elapsed(instance: &EngineInstance) -> Duration {
    instance
        .app()
        .world()
        .get_resource::<Time<Fixed>>()
        .map_or(Duration::ZERO, Time::elapsed)
}

struct Fixture {
    graph: LockedGameGraph,
    compiled: CompiledRegistration,
    runtime: RuntimeImage,
    manifest: RegistrationManifest,
    package: PackageName,
    system: StableId,
    callback: StableId,
}

impl Fixture {
    fn prepared(&self) -> StructurallyValidatedComposeImages {
        StructurallyValidatedComposeImages::new(
            self.graph.clone(),
            self.compiled.clone(),
            self.runtime.clone(),
        )
        .expect("compiler fixture passes the engine preparation gate")
    }
}

#[allow(clippy::too_many_lines)]
fn fixture() -> Fixture {
    let package = package_name("terrenia");
    let version = package_version("0.1.0");
    let source = source_id("latticeaxiom:source/terrenia");
    let profile = stable_id("latticeaxiom:profile/headless");
    let patterns = grant_patterns();
    let composition = CompositionSpec {
        schema_version: COMPOSITION_SCHEMA_VERSION,
        profile: profile.clone(),
        profile_kind: ProfileKind::HeadlessTest,
        projection_domains: BTreeSet::from([PackageDomain::Authoritative]),
        roots: BTreeMap::from([(
            package.clone(),
            PackageRequest {
                version: version_requirement("~0.1.0"),
                features: BTreeSet::new(),
                realization: RealizationPreference::Auto,
            },
        )]),
        capabilities: BTreeMap::new(),
        features: BTreeMap::new(),
        parameters: BTreeMap::new(),
        semantic_bindings: BTreeMap::new(),
        overlays: Vec::new(),
        sources: vec![SourceCandidate {
            source_id: source.clone(),
            package: package.clone(),
            version: version.clone(),
            path: "packages/terrenia".to_owned(),
            content_hash: CanonicalHash::digest(b"terrenia-source"),
            priority: 0,
            provenance: provenance(
                "latticeaxiom:source/terrenia",
                "packages/terrenia/package.ncl",
            ),
        }],
        policy: CompositionPolicy {
            target: target("x86_64-unknown-linux-gnu"),
            realization_order: vec![RealizationKind::Data],
            namespace_grants: BTreeMap::from([(package.clone(), patterns.clone())]),
            maximum_trust: TrustClass::DataOnly,
            allow_force_override: false,
            allow_recovery: false,
            evaluation_policy: stable_id("latticeaxiom:nickel-evaluation-policy/r0@1"),
            evaluation_limits: NickelEvaluationLimits::default(),
        },
        provenance: provenance(
            "latticeaxiom:source/profile-headless",
            "profiles/headless.game.ncl",
        ),
    };

    let system = stable_id("terrenia:system/tick");
    let callback = stable_id("terrenia:callback/tick@1");
    let signature = CanonicalHash::digest(b"fixed-system-signature-v1");
    let mut manifest = RegistrationManifest {
        schema_version: latticeaxiom_compose::REGISTRATION_MANIFEST_SCHEMA_VERSION,
        package: package.clone(),
        version: version.clone(),
        fragment: RegistrationFragment {
            registrations: vec![
                exact_registration(&package, "terrenia:block/alpha", RegistrationKind::Block),
                exact_registration(&package, system.as_str(), RegistrationKind::System),
            ],
            ..RegistrationFragment::default()
        },
        producer: ManifestProducer {
            tool: "engine-integration-fixture".to_owned(),
            version: version.clone(),
            input_hash: CanonicalHash::digest(b"registration-input"),
        },
        semantic_hash: CanonicalHash::digest(b"unsealed-manifest"),
    };
    manifest.semantic_hash = manifest
        .recompute_semantic_hash()
        .expect("fixture manifest canonicalizes");

    let package_input = PackageRegistrationInput {
        manifest: manifest.clone(),
        schemas: BTreeMap::new(),
        systems: BTreeMap::from([(
            system.clone(),
            SystemDeclaration {
                id: system.clone(),
                declared_by: package.clone(),
                stage: stable_id(FIXED_STAGE),
                callback: callback.clone(),
                signature_hash: signature,
                after: BTreeSet::new(),
                before: BTreeSet::new(),
            },
        )]),
        callbacks: BTreeMap::from([(
            callback.clone(),
            CallbackDeclaration {
                id: callback.clone(),
                declared_by: package.clone(),
                signature_hash: signature,
            },
        )]),
        provided_capabilities: BTreeSet::new(),
        semantic_grants: BTreeSet::new(),
    };
    let artifact_hash = CanonicalHash::digest(b"terrenia-artifact");
    let locked = LockedPackage {
        name: package.clone(),
        version,
        source_id: source,
        source_hash: CanonicalHash::digest(b"terrenia-source"),
        provenance_hash: CanonicalHash::digest(b"terrenia-provenance"),
        realization: RealizationKind::Data,
        realization_id: RealizationId::new("data").expect("data realization ID is valid"),
        manifest_hash: manifest.semantic_hash,
        artifact_hash,
        interfaces: BTreeMap::new(),
        engine_build_id: None,
        domains: BTreeSet::from([PackageDomain::Authoritative]),
        dependencies: BTreeMap::new(),
        schemas: BTreeSet::new(),
        source_path: "packages/terrenia".to_owned(),
    };
    let grant = NamespaceGrant::new(
        RegistrationNamespace::new("terrenia").expect("fixture namespace is valid"),
        NamespaceGrantor::profile(profile).expect("fixture profile grantor is valid"),
        package.clone(),
        patterns,
    )
    .expect("fixture namespace grant is valid");
    let mut graph = LockedGameGraph {
        schema_version: LOCK_SCHEMA_VERSION,
        composition_hash: composition
            .semantic_hash()
            .expect("composition canonicalizes"),
        composition_provenance_hash: composition
            .provenance_hash()
            .expect("composition provenance canonicalizes"),
        evaluation_policy: composition.policy.evaluation_policy.clone(),
        evaluation_limits: composition.policy.evaluation_limits,
        roots: BTreeSet::from([package.clone()]),
        packages: BTreeMap::from([(package.clone(), locked)]),
        capability_providers: BTreeMap::new(),
        namespace_grants: BTreeSet::from([grant]),
        explanation: Vec::new(),
        graph_hash: CanonicalHash::digest(b"unsealed-graph"),
        lock_hash: CanonicalHash::digest(b"unsealed-lock"),
    };
    graph.graph_hash = graph
        .recompute_graph_hash()
        .expect("fixture graph canonicalizes");
    graph.lock_hash = graph
        .recompute_lock_hash()
        .expect("fixture lock canonicalizes");

    let packages = BTreeMap::from([(package.clone(), package_input)]);
    let compiled = RegistrationCompiler::default()
        .compile(RegistrationCompileInput {
            composition: &composition,
            graph: &graph,
            packages: &packages,
        })
        .expect("fixture registration compiles");
    let runtime = RuntimeImage {
        registration_hash: compiled.image.image_hash,
        packages: BTreeMap::from([(
            package.clone(),
            RuntimeBinding {
                realization: RealizationKind::Data,
                artifact_hash,
                callbacks: BTreeSet::from([callback.clone()]),
            },
        )]),
    };

    Fixture {
        graph,
        compiled,
        runtime,
        manifest,
        package,
        system,
        callback,
    }
}

fn exact_registration(
    owner: &PackageName,
    value: &str,
    kind: RegistrationKind,
) -> ExactRegistration {
    ExactRegistration {
        id: stable_id(value),
        kind,
        declared_by: owner.clone(),
        schema: None,
        provenance: provenance("latticeaxiom:source/terrenia", "registrations/fixture.ncl"),
    }
}

fn grant_patterns() -> BTreeSet<NamespaceGrantPattern> {
    [
        "terrenia:block/**",
        "terrenia:system/**",
        "terrenia:callback/**",
    ]
    .into_iter()
    .map(|value| value.parse().expect("fixture grant pattern is valid"))
    .collect()
}

fn provenance(source: &str, path: &str) -> SourceProvenance {
    SourceProvenance::new(
        source_id(source),
        path,
        CanonicalHash::digest(path.as_bytes()),
        None,
        Vec::new(),
    )
    .expect("fixture provenance is valid")
}

fn stable_id(value: &str) -> StableId {
    value.parse().expect("fixture stable ID is valid")
}

fn package_name(value: &str) -> PackageName {
    value.parse().expect("fixture package name is valid")
}

fn package_version(value: &str) -> PackageVersion {
    value.parse().expect("fixture package version is valid")
}

fn version_requirement(value: &str) -> PackageVersionReq {
    value.parse().expect("fixture version requirement is valid")
}

fn source_id(value: &str) -> SourceId {
    value.parse().expect("fixture source ID is valid")
}

fn target(value: &str) -> TargetTriple {
    value.parse().expect("fixture target triple is valid")
}

struct LockBootFixture {
    directory: TestDirectory,
    reopened: ReopenedFinalLockV1,
    objects: ProductLockObjects,
    host: HostBuildReceipts,
    graph: LockedGameGraph,
    compiled: CompiledRegistration,
    target: TargetTriple,
}

impl LockBootFixture {
    fn lock_path(&self) -> PathBuf {
        self.directory.lock_path()
    }

    fn prepared(&self) -> LockVerifiedComposeImages {
        LockVerifiedComposeImages::from_reopened_lock(
            &self.reopened,
            &self.target,
            self.graph.clone(),
            self.compiled.clone(),
        )
        .expect("reopened lock binds compiled evidence")
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn create() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "latticeaxiom-engine-lock-boot-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test directory was created");
        Self(fs::canonicalize(&path).expect("test directory canonicalized"))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn lock_path(&self) -> PathBuf {
        self.0.join(PRODUCT_LOCK_FILE_NAME)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let temporary_root =
            fs::canonicalize(std::env::temp_dir()).expect("temporary root canonicalized");
        assert!(
            self.0.starts_with(&temporary_root),
            "refusing to delete a test directory outside the process temporary root"
        );
        if let Err(error) = fs::remove_dir_all(&self.0)
            && error.kind() != io::ErrorKind::NotFound
        {
            panic!("test directory cleanup failed: {error}");
        }
    }
}

#[allow(clippy::too_many_lines)]
fn lock_boot_fixture() -> LockBootFixture {
    let fixture = fixture();
    let directory = TestDirectory::create();
    let source = b"terrenia-source".to_vec();
    let artifact = b"terrenia-artifact".to_vec();
    let manifest_bytes = manifest_object_bytes(&fixture.manifest);
    assert_eq!(
        CanonicalHash::digest(&manifest_bytes),
        fixture.manifest.semantic_hash,
        "CAS manifest bytes must be the semantic identity payload"
    );
    let mut objects = ProductLockObjects::default();
    objects
        .sources
        .insert(CanonicalHash::digest(&source), source.clone());
    objects
        .manifests
        .insert(fixture.manifest.semantic_hash, manifest_bytes);
    objects
        .artifacts
        .insert(CanonicalHash::digest(&artifact), artifact.clone());

    let bootstrap = CompositionBootstrapV1::from_toml_str(
        r#"
schema_version = 1
projection = "headless-test"
projection_domains = ["authoritative"]
evaluation_policy = "latticeaxiom:nickel-evaluation-policy/r0@1"
realization_policy = ["data"]
nickel_profile_entry = "profiles/headless.ncl"

[roots.terrenia]
version = "=0.1.0"
realization = { mode = "auto" }

[[sources]]
kind = "fixture"
package = "terrenia"
version = "0.1.0"
source_id = "latticeaxiom:source/terrenia"
path = "packages/terrenia"
"#,
    )
    .expect("lock-boot bootstrap parses");
    let toolchain = CanonicalHash::digest(b"engine-lock-boot-toolchain");
    let runtime_image_fingerprint = latticeaxiom_core::canonical_json_hash(&fixture.runtime)
        .expect("fixture runtime image canonicalizes");
    let target = target("x86_64-unknown-linux-gnu");
    let name = fixture.package.clone();
    let lock = latticeaxiom_compose::LockV1::seal(ProductLockDraftV1 {
        producer: ProductLockProducerV1 {
            machine: PRODUCT_LOCK_PRODUCER_MACHINE.to_owned(),
            toolchain,
        },
        bootstrap,
        alias_edges: BTreeMap::new(),
        resolution_receipt_hash: CanonicalHash::digest(b"resolution-receipt"),
        evaluation_policy_receipt_hash: CanonicalHash::digest(b"evaluation-policy"),
        package_features: BTreeMap::new(),
        source_objects: BTreeMap::from([(name.clone(), CanonicalHash::digest(&source))]),
        realizations: BTreeMap::from([(
            target.clone(),
            TargetRealizationLockV1 {
                projection: ProfileKind::HeadlessTest,
                target: target.clone(),
                toolchain,
                build_intent_hash: CanonicalHash::digest(b"build-intent"),
                packages: BTreeMap::from([(
                    name,
                    TargetPackageRealizationV1 {
                        realization_id: fixture
                            .graph
                            .packages
                            .values()
                            .next()
                            .expect("fixture graph has a package")
                            .realization_id
                            .clone(),
                        kind: RealizationKind::Data,
                        artifact_digest: CanonicalHash::digest(&artifact),
                        engine_build_id: None,
                        registration_hash: fixture.manifest.semantic_hash,
                    },
                )]),
                engine_build_id: None,
                registration_image_hash: fixture.compiled.image.image_hash,
                runtime_image_fingerprint,
            },
        )]),
        registration_semantic_hash: fixture.compiled.image_receipt.registration_semantic_hash,
        registration_image: fixture.compiled.image.clone(),
        runtime_image: fixture.runtime.clone(),
        graph: fixture.graph.clone(),
    })
    .expect("product lock seals from compiled evidence");
    persist_product_lock(directory.lock_path(), &lock, LockActionMode::Persist)
        .expect("product lock persists and reopens");
    let host = HostBuildReceipts {
        toolchain,
        engine_build_id: None,
    };
    let reopened = ReopenedFinalLockV1::reopen_frozen(directory.lock_path(), &objects, &host)
        .expect("persisted lock reopens frozen");
    LockBootFixture {
        directory,
        reopened,
        objects,
        host,
        graph: fixture.graph,
        compiled: fixture.compiled,
        target,
    }
}

#[derive(Serialize)]
struct ManifestSemanticIdentity<'a> {
    schema_version: u32,
    package: &'a PackageName,
    version: &'a PackageVersion,
    normalized_fragment: Value,
}

fn manifest_object_bytes(manifest: &RegistrationManifest) -> Vec<u8> {
    let mut fragment = serde_json::to_value(&manifest.fragment).expect("fragment encodes");
    if let Value::Object(fields) = &mut fragment
        && let Some(Value::Array(registrations)) = fields.get_mut("registrations")
    {
        for registration in registrations.iter_mut() {
            if let Value::Object(fields) = registration {
                fields.remove("provenance");
            }
        }
        registrations.sort_by(|left, right| {
            canonical_json_bytes(left)
                .expect("registration left canonicalizes")
                .cmp(&canonical_json_bytes(right).expect("registration right canonicalizes"))
        });
    }

    canonical_json_bytes(&ManifestSemanticIdentity {
        schema_version: manifest.schema_version,
        package: &manifest.package,
        version: &manifest.version,
        normalized_fragment: fragment,
    })
    .expect("manifest semantic identity canonicalizes")
}

#[test]
fn production_spine_streams_natural_layer_and_bounded_inspect() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    let boot = lock_boot_fixture();
    let mut instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog,
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    assert!(
        spine.has_natural_layer(),
        "V5 host plan must compile the natural layer"
    );
    assert!(
        spine.has_cave_topology_layer(),
        "V6 host plan must compile cave topology"
    );
    assert!(
        spine.has_hydrology_occupancy(),
        "V6 host plan must compile hydrology occupancy"
    );
    assert!(
        spine.cave_owned_domains().len() >= 2,
        "V6 host plan must bind two underground topology domains from the lock"
    );
    let report = spine
        .worldgen_inspect_report()
        .expect("bounded worldgen inspect compiles");
    assert!(!report.records.is_empty());
    let kinds = report
        .records
        .iter()
        .map(|record| record.kind)
        .collect::<BTreeSet<_>>();
    assert!(kinds.contains(&latticeaxiom_runtime_contracts::WorldgenInspectKindV1::Portal));
    assert!(kinds.contains(&latticeaxiom_runtime_contracts::WorldgenInspectKindV1::Entrance));
    instance
        .advance_fixed_ticks(8)
        .expect("natural terrain ticks advance");
}

#[test]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
fn production_host_enters_required_cave_and_gathers_natural_resource() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    let boot = lock_boot_fixture();
    let mut instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog,
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();

    let entrance = spine
        .required_cave_entrance()
        .expect("compiled CaveTopology field portals must include a required entrance");
    assert!(entrance.clearance_radius_voxels() > 0);
    assert!(matches!(
        entrance.tangent_face(),
        ChunkFaceV1::NegativeX
            | ChunkFaceV1::PositiveX
            | ChunkFaceV1::NegativeY
            | ChunkFaceV1::PositiveY
            | ChunkFaceV1::NegativeZ
            | ChunkFaceV1::PositiveZ
    ));
    let aperture = entrance.aperture();
    let destination = entrance.destination();
    let surface = entrance.surface_footing();
    assert!(
        spine
            .cave_occupancy_arbitration(
                i64::from(aperture[0]),
                i64::from(aperture[1]),
                i64::from(aperture[2])
            )
            .is_some_and(latticeaxiom_engine::CaveOccupancyArbitrationV1::is_finally_void)
    );
    assert!(
        spine
            .cave_occupancy_arbitration(
                i64::from(destination[0]),
                i64::from(destination[1]),
                i64::from(destination[2])
            )
            .is_some_and(latticeaxiom_engine::CaveOccupancyArbitrationV1::is_finally_void)
    );

    seed_tool(&spine, 0, "terrenia:item/wooden-pickaxe", 59);
    seed_tool(&spine, 1, "terrenia:item/wooden-shovel", 59);

    let mut generation = 1_u64;
    generation = walk_toward_column(
        &mut instance,
        &spine,
        generation,
        surface[0],
        surface[2],
        1_200,
    );
    assert!(
        !spine.occupies_unready_cave_void(),
        "surface travel must not enter an unready cave void"
    );
    let pose = spine.player_pose();
    #[allow(clippy::cast_precision_loss)]
    let approach_offset = bevy::prelude::Vec2::new(
        pose.translation.x - (surface[0] as f32 + 0.5),
        pose.translation.z - (surface[2] as f32 + 0.5),
    );
    assert!(
        approach_offset.length() < 2.5,
        "fixed inputs must reach the required entrance column, pose {:?}, target {surface:?}",
        pose.translation
    );

    let column = latticeaxiom_gameplay::BlockPosition {
        x: surface[0],
        y: surface[1],
        z: surface[2],
    };
    let aperture_pos = latticeaxiom_gameplay::BlockPosition {
        x: aperture[0],
        y: aperture[1],
        z: aperture[2],
    };
    generation = wait_for_resident(&mut instance, &spine, generation, aperture_pos, 180);
    open_required_entrance_shaft(&spine, column, aperture_pos);
    generation = idle_at_hole(&mut instance, &spine, generation, 90);

    let occupancy = spine.inspect_occupancy(aperture_pos).unwrap_or_else(|error| {
        panic!(
            "required entrance aperture occupancy is inspectable, got {error:?}, pose {:?}, lifecycle {:?}",
            spine.player_pose().translation,
            spine.chunk_of(aperture_pos).map(|chunk| spine.chunk_lifecycle(chunk))
        )
    });
    assert!(
        occupancy.fluid.is_none(),
        "hydrology cannot own the cave entrance; aperture fluid must stay empty"
    );

    generation = walk_toward_column(
        &mut instance,
        &spine,
        generation,
        aperture[0],
        aperture[2],
        1_200,
    );
    generation = idle_at_hole(&mut instance, &spine, generation, 240);
    let pose = spine.player_pose();
    let underground = player_in_cave(&spine, pose.translation, aperture[1]);
    assert!(
        underground,
        "player must enter underground cave space through the required entrance (pose {:?}, aperture {aperture:?}, dest {destination:?})",
        pose.translation
    );
    assert!(
        !spine.occupies_unready_cave_void(),
        "mesh and collider readiness must prevent entry into an unready cave void"
    );
    let dest_chunk = spine
        .chunk_of(latticeaxiom_gameplay::BlockPosition {
            x: destination[0],
            y: destination[1],
            z: destination[2],
        })
        .expect("destination maps to a chunk");
    assert!(
        spine.cave_entry_ready(dest_chunk)
            || spine.chunk_lifecycle(dest_chunk) == ChunkLifecycle::Active,
        "entered cave chunk must be mesh/collider ready, got {:?}",
        spine.chunk_lifecycle(dest_chunk)
    );
    let _ = generation;

    let copper = parse_block("terrenia:block/copper-ore");
    let copper_item = parse_item("terrenia:item/copper-ore");
    let ore = spine
        .first_cave_adjacent_block(&copper)
        .or_else(|| spine.first_resident_block(&copper))
        .expect("natural copper resource exists in the streamed underground set");
    spine
        .select_hotbar_slot(0)
        .expect("wooden pickaxe is selected for ore");
    gather_until_inventory_has(&spine, ore, &copper_item, 1);
    assert!(
        spine
            .inventory_view()
            .expect("inventory is bound")
            .count_item(&copper_item)
            >= 1
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
fn production_host_reaches_both_underground_territories_and_three_resource_classes() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    let boot = lock_boot_fixture();
    let mut instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog,
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    assert!(spine.has_cave_topology_layer());
    assert!(spine.has_hydrology_occupancy());
    let owned = spine.cave_owned_domains();
    assert_eq!(owned.len(), 2, "V6 requires two underground-owned domains");

    seed_tool(&spine, 0, "terrenia:item/wooden-pickaxe", 59);
    seed_tool(&spine, 1, "terrenia:item/wooden-shovel", 59);
    let (_, dirt_pos, dirt_item) = first_resident_soil(&spine);
    gather_until_inventory_has(&spine, dirt_pos, &dirt_item, 1);
    seed_tool(&spine, 0, "terrenia:item/wooden-pickaxe", 59);
    seed_tool(&spine, 1, "terrenia:item/wooden-shovel", 59);

    let entrance = spine
        .required_cave_entrance()
        .expect("V6 field portals must include a required entrance");
    let aperture = entrance.aperture();
    let surface = entrance.surface_footing();
    let mut generation = 1_u64;
    generation = walk_toward_column(
        &mut instance,
        &spine,
        generation,
        surface[0],
        surface[2],
        1_200,
    );
    generation = wait_for_resident(
        &mut instance,
        &spine,
        generation,
        latticeaxiom_gameplay::BlockPosition {
            x: aperture[0],
            y: aperture[1],
            z: aperture[2],
        },
        180,
    );
    seed_tool(&spine, 0, "terrenia:item/wooden-pickaxe", 59);
    seed_tool(&spine, 1, "terrenia:item/wooden-shovel", 59);
    open_required_entrance_shaft(
        &spine,
        latticeaxiom_gameplay::BlockPosition {
            x: surface[0],
            y: surface[1],
            z: surface[2],
        },
        latticeaxiom_gameplay::BlockPosition {
            x: aperture[0],
            y: aperture[1],
            z: aperture[2],
        },
    );
    generation = idle_at_hole(&mut instance, &spine, generation, 90);
    generation = walk_toward_column(
        &mut instance,
        &spine,
        generation,
        aperture[0],
        aperture[2],
        1_200,
    );
    generation = idle_at_hole(&mut instance, &spine, generation, 240);
    assert!(
        player_in_cave(&spine, spine.player_pose().translation, aperture[1]),
        "fixed inputs must enter the required cave"
    );
    assert!(!spine.occupies_unready_cave_void());

    let mut visited = BTreeSet::new();
    if let Some(domain) = player_topology_domain(&spine) {
        visited.insert(domain);
    }
    let mut destinations = spine.cave_destinations();
    let origin_x = spine.player_pose().translation.x;
    destinations.sort_by(|left, right| {
        let left_dx = (left.0[0] as f32 - origin_x).abs();
        let right_dx = (right.0[0] as f32 - origin_x).abs();
        left_dx
            .partial_cmp(&right_dx)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (voxel, _) in destinations {
        let x = i32::try_from(voxel[0]).expect("destination x fits");
        let y = i32::try_from(voxel[1]).expect("destination y fits");
        let z = i32::try_from(voxel[2]).expect("destination z fits");
        generation = wait_for_resident(
            &mut instance,
            &spine,
            generation,
            latticeaxiom_gameplay::BlockPosition { x, y, z },
            240,
        );
        generation = walk_toward_column(&mut instance, &spine, generation, x, z, 2_400);
        generation = idle_at_hole(&mut instance, &spine, generation, 180);
        if let Some(here) = player_topology_domain(&spine) {
            visited.insert(here);
        }
        if let Some(here) = spine.cave_topology_domain(voxel[0], voxel[1], voxel[2]) {
            let pose = spine.player_pose().translation;
            let dx = pose.x - (x as f32 + 0.5);
            let dz = pose.z - (z as f32 + 0.5);
            if dx.hypot(dz) < 4.0 && (pose.y - (y as f32)).abs() < 6.0 {
                visited.insert(here);
            }
        }
        assert!(!spine.occupies_unready_cave_void());
    }
    assert!(
        owned.iter().all(|domain| visited.contains(domain)),
        "journey must enter both underground territories, visited {visited:?}, owned {owned:?}, pose {:?}, dests {:?}",
        spine.player_pose().translation,
        spine.cave_destinations()
    );

    let stone = first_resident_any(
        &spine,
        &[
            "terrenia:block/stone",
            "terrenia:block/granite",
            "terrenia:block/slate",
            "terrenia:block/deepstone",
        ],
    );
    let stone_item = parse_item(&stone.0.as_str().replace(":block/", ":item/"));
    spine
        .select_hotbar_slot(0)
        .expect("pickaxe selected for stone");
    gather_until_inventory_has(&spine, stone.1, &stone_item, 1);
    let copper = parse_block("terrenia:block/copper-ore");
    let copper_item = parse_item("terrenia:item/copper-ore");
    let ore = spine
        .first_cave_adjacent_block(&copper)
        .or_else(|| spine.first_resident_block(&copper))
        .expect("ore exists in the streamed set");
    gather_until_inventory_has(&spine, ore, &copper_item, 1);
    let inventory = spine.inventory_view().expect("inventory after gather");
    assert!(inventory.count_item(&dirt_item) >= 1);
    assert!(inventory.count_item(&stone_item) >= 1);
    assert!(inventory.count_item(&copper_item) >= 1);
    let _ = generation;
}

#[test]
#[allow(clippy::too_many_lines)]
fn production_host_gathers_crafts_mines_with_tools_and_fails_closed() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    assert!(
        catalog
            .recipe(&parse_recipe("terrenia:recipe/oak-planks@1"))
            .is_some()
    );
    assert!(
        catalog
            .recipe(&parse_recipe("terrenia:recipe/wooden-pickaxe@1"))
            .is_some()
    );
    let boot = lock_boot_fixture();
    let instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog,
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();

    let stone_block = parse_block("terrenia:block/stone");
    let stick_item = parse_item("terrenia:item/stick");
    let plank_item = parse_item("terrenia:item/oak-planks");
    let workbench_item = parse_item("terrenia:item/workbench");
    let pickaxe_item = parse_item("terrenia:item/wooden-pickaxe");
    let shovel_item = parse_item("terrenia:item/wooden-shovel");
    let (_, dirt_pos, dirt_item) = first_resident_soil(&spine);
    let log_item = parse_item("terrenia:item/oak-log");
    gather_until_inventory_has(&spine, dirt_pos, &dirt_item, 1);
    if let Some((wood_block, log_pos, gathered_wood, _, _)) = first_resident_wood(&spine) {
        gather_until_inventory_has(&spine, log_pos, &gathered_wood, 1);
        if wood_block.as_str().ends_with("pine-log") {
            top_up_item(&spine, &log_item, 4);
        }
    }
    top_up_item(&spine, &log_item, 3);
    assert!(
        spine
            .inventory_view()
            .expect("inventory is bound")
            .count_item(&dirt_item)
            >= 1
    );

    for _ in 0..3 {
        spine
            .craft_recipe(&parse_recipe("terrenia:recipe/oak-planks@1"), None)
            .expect("oak planks craft from gathered or seeded wood");
    }
    spine
        .craft_recipe(&parse_recipe("terrenia:recipe/stick@1"), None)
        .expect("sticks craft from planks");
    spine
        .craft_recipe(&parse_recipe("terrenia:recipe/workbench@1"), None)
        .expect("workbench crafts from planks");
    let inventory = spine.inventory_view().expect("inventory after crafts");
    assert!(inventory.count_item(&plank_item) >= 1 || inventory.count_item(&workbench_item) >= 1);
    assert!(inventory.count_item(&stick_item) >= 2);
    assert!(inventory.count_item(&workbench_item) >= 1);

    let station = ContainerId::new(2);
    spine
        .bind_workstation(
            WorkstationId::parse("latticeaxiom:workstation/crafting@1")
                .expect("crafting workstation is a platform contract"),
            station,
        )
        .expect("workbench path binds a crafting workstation");
    spine
        .craft_recipe(
            &parse_recipe("terrenia:recipe/wooden-pickaxe@1"),
            Some(station),
        )
        .expect("wooden pickaxe crafts at the workbench");
    spine
        .craft_recipe(
            &parse_recipe("terrenia:recipe/wooden-shovel@1"),
            Some(station),
        )
        .expect("wooden shovel crafts at the workbench");
    let after_tools = spine.inventory_view().expect("inventory after tools");
    let pickaxe_durability = after_tools
        .tool_durability(&pickaxe_item)
        .expect("wooden pickaxe carries durability");
    assert!(after_tools.tool_durability(&shovel_item).is_some());

    let pickaxe_slot = after_tools
        .slots()
        .iter()
        .position(|slot| {
            slot.as_ref()
                .is_some_and(|stack| stack.item() == &pickaxe_item)
        })
        .expect("pickaxe occupies a slot");
    spine
        .select_hotbar_slot(u16::try_from(pickaxe_slot).expect("hotbar index fits") % 9)
        .ok();
    if pickaxe_slot >= 9 {
        spine
            .seed_inventory_slot(SlotIndex::new(0), after_tools.slots()[pickaxe_slot].clone())
            .expect("pickaxe moves into the hotbar");
        spine
            .seed_inventory_slot(
                SlotIndex::new(u16::try_from(pickaxe_slot).expect("fits")),
                None,
            )
            .expect("source slot clears");
        spine
            .select_hotbar_slot(0)
            .expect("pickaxe hotbar selected");
    } else {
        spine
            .select_hotbar_slot(u16::try_from(pickaxe_slot).expect("fits"))
            .expect("pickaxe hotbar selected");
    }

    let stone_pos = spine
        .first_resident_block(&stone_block)
        .expect("generated stone exists");
    let mut pickaxe_steps = 0_u32;
    loop {
        pickaxe_steps += 1;
        match spine.mine_cell(stone_pos) {
            Ok(_) => break,
            Err(BlockEditRejectV1::RequiresProgress { .. }) if pickaxe_steps < 20 => {}
            Err(error) => panic!("correct-tool mining must proceed, got {error:?}"),
        }
    }
    pickup_remaining(&spine);
    let after_stone = spine.inventory_view().expect("inventory after stone");
    let remaining = after_stone
        .tool_durability(&pickaxe_item)
        .expect("pickaxe remains after one break");
    assert!(
        remaining < pickaxe_durability,
        "successful mining must decrement durability ({remaining} >= {pickaxe_durability})"
    );
    assert!(
        pickaxe_steps < 15,
        "wooden pickaxe must mine stone faster than hand work units, steps {pickaxe_steps}"
    );

    let shovel_slot = after_stone
        .slots()
        .iter()
        .position(|slot| {
            slot.as_ref()
                .is_some_and(|stack| stack.item() == &shovel_item)
        })
        .expect("shovel occupies a slot");
    if shovel_slot >= 9 {
        spine
            .seed_inventory_slot(SlotIndex::new(1), after_stone.slots()[shovel_slot].clone())
            .expect("shovel moves into the hotbar");
        spine.select_hotbar_slot(1).expect("wrong tool selected");
    } else {
        spine
            .select_hotbar_slot(u16::try_from(shovel_slot).expect("fits"))
            .expect("wrong tool selected");
    }
    let drops_before_wrong = spine.dropped_items().len();
    let other_stone = spine
        .first_resident_block(&stone_block)
        .expect("a second stone cell remains");
    let wrong = spine.mine_cell(other_stone);
    assert!(
        matches!(wrong, Err(BlockEditRejectV1::RequiresTool { .. })),
        "wrong tool class must fail closed, got {wrong:?}"
    );
    assert_eq!(
        spine.dropped_items().len(),
        drops_before_wrong,
        "wrong-tool mining must not spawn a drop"
    );

    let dirt_slot = spine
        .inventory_view()
        .expect("inventory")
        .slots()
        .iter()
        .position(|slot| {
            slot.as_ref()
                .is_some_and(|stack| stack.item() == &dirt_item)
        })
        .expect("gathered dirt remains");
    if dirt_slot >= 9 {
        let stack = spine.inventory_view().expect("inventory").slots()[dirt_slot].clone();
        spine
            .seed_inventory_slot(SlotIndex::new(2), stack)
            .expect("dirt moves to hotbar");
        spine.select_hotbar_slot(2).expect("dirt selected");
    } else {
        spine
            .select_hotbar_slot(u16::try_from(dirt_slot).expect("fits"))
            .expect("dirt selected");
    }
    let place_anchor = latticeaxiom_gameplay::BlockPosition {
        x: dirt_pos.x,
        y: dirt_pos.y.saturating_add(1),
        z: dirt_pos.z,
    };
    spine
        .place_from_hotbar(place_anchor, BlockFaceV1::NegativeY)
        .expect("placement from the hotbar consumes the gathered soil");

    let filler =
        ItemStackV1::plain(parse_item("terrenia:item/stone"), 64).expect("stone stacks are valid");
    for slot in 0..INVENTORY_SLOTS {
        let index = SlotIndex::new(u16::try_from(slot).expect("slot fits"));
        if spine
            .inventory_view()
            .expect("inventory")
            .slots()
            .get(slot)
            .and_then(Option::as_ref)
            .is_none()
        {
            spine
                .seed_inventory_slot(index, Some(filler.clone()))
                .expect("capacity probe fills empty slots");
        }
    }
    let before_drops = spine.dropped_items().len();
    let before_count = spine
        .inventory_view()
        .expect("inventory")
        .slots()
        .iter()
        .flatten()
        .count();
    let grass = spine
        .first_resident_block(&parse_block("terrenia:block/grass"))
        .or_else(|| Some(first_resident_soil(&spine).1))
        .expect("a gatherable soil block remains");
    let _ = mine_until_broken(&spine, grass);
    let pickup = spine
        .dropped_items()
        .keys()
        .copied()
        .collect::<Vec<DropEntityId>>();
    for drop in pickup {
        let result = spine.pickup_drop(drop);
        assert!(
            matches!(result, Err(GameplayReject::InventoryFull)),
            "full inventory must fail closed, got {result:?}"
        );
    }
    assert_eq!(
        spine
            .inventory_view()
            .expect("inventory")
            .slots()
            .iter()
            .flatten()
            .count(),
        before_count,
        "full-inventory pickup must not duplicate stacks"
    );
    assert!(
        spine.dropped_items().len() >= before_drops,
        "full-inventory gathering must keep the source drop"
    );
}

#[test]
fn production_host_inspect_overlay_fills_harvest_and_omits_occupancy() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    let boot = lock_boot_fixture();
    let mut instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog.clone(),
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();

    instance
        .enqueue_headless_actions([
            look_frame(1, -std::f32::consts::FRAC_PI_2, 0.7),
            idle_frame(2),
        ])
        .expect("look frames enqueue");
    instance.advance_fixed_ticks(3).expect("look ticks advance");

    let inspect = spine
        .current_target()
        .expect("crosshair DDA must hit after look");
    assert_inspect_dto_overlay_fields(&inspect, &spine);
    assert_eq!(inspect.declared_by, inspect.block_id.namespace());
    let definition = catalog
        .block(&inspect.block_id)
        .expect("aimed block is in the gameplay catalog");
    assert_eq!(inspect.hardness_ticks, definition.mining.hardness.get());
    if let Some(tool) = &definition.mining.tool {
        assert_eq!(
            inspect.harvest_tool.as_deref(),
            Some(tool.class.as_str()),
            "harvest tool must come from catalog mining"
        );
        assert_eq!(inspect.harvest_tier, Some(tool.minimum_tier));
    } else {
        assert_eq!(inspect.harvest_tool, None);
        assert_eq!(inspect.harvest_tier, None);
    }
    let overlay = inspect.overlay_lines();
    assert!(
        overlay.contains("Hand") || inspect.harvest_tool.is_some(),
        "harvest fragment must appear on the player overlay, got {overlay}"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn production_host_pick_block_selects_swaps_and_rejects_when_absent() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    let boot = lock_boot_fixture();
    let mut instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog.clone(),
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();

    instance
        .enqueue_headless_actions([
            look_frame(1, -std::f32::consts::FRAC_PI_2, 0.7),
            idle_frame(2),
        ])
        .expect("look frames enqueue");
    instance.advance_fixed_ticks(3).expect("look ticks advance");
    let aimed = spine
        .current_target()
        .expect("pick-block needs a live DDA target")
        .block_id;
    let placement_item = catalog
        .items()
        .values()
        .find(|item| item.placement_block.as_ref() == Some(&aimed))
        .map_or_else(
            || panic!("{aimed} must have a placement item in the catalog"),
            |item| item.id.clone(),
        );
    let other_item = parse_item("terrenia:item/dirt");
    let other_item = if other_item == placement_item {
        parse_item("terrenia:item/oak-log")
    } else {
        other_item
    };

    clear_inventory(&spine);
    spine
        .seed_inventory_slot(
            SlotIndex::new(3),
            Some(ItemStackV1::plain(placement_item.clone(), 4).expect("placement stack")),
        )
        .expect("hotbar placement stack is seeded");
    spine
        .select_hotbar_slot(0)
        .expect("unrelated hotbar slot is selected");
    instance
        .enqueue_headless_actions([pick_block_frame(3)])
        .expect("pick-block frame enqueues");
    instance
        .advance_fixed_ticks(2)
        .expect("pick-block ticks advance");
    let after_select = spine.inventory_view().expect("inventory after pick-select");
    assert_eq!(
        after_select.hotbar_slot(),
        3,
        "pick-block must select the hotbar stack that places the aimed block"
    );
    assert_eq!(
        after_select
            .selected()
            .map(latticeaxiom_gameplay::ItemStackV1::item),
        Some(&placement_item)
    );

    clear_inventory(&spine);
    spine
        .select_hotbar_slot(0)
        .expect("destination hotbar is selected");
    spine
        .seed_inventory_slot(
            SlotIndex::new(0),
            Some(ItemStackV1::plain(other_item.clone(), 2).expect("hotbar occupant")),
        )
        .expect("selected hotbar is occupied");
    spine
        .seed_inventory_slot(
            SlotIndex::new(HOTBAR_SLOTS + 3),
            Some(ItemStackV1::plain(placement_item.clone(), 4).expect("body placement stack")),
        )
        .expect("body inventory holds the placement stack");
    spine
        .pick_aimed_block()
        .expect("pick-block swaps a body stack onto the selected hotbar");
    let after_swap = spine.inventory_view().expect("inventory after pick-swap");
    assert_eq!(after_swap.hotbar_slot(), 0);
    assert_eq!(
        after_swap
            .slots()
            .first()
            .and_then(Option::as_ref)
            .map(latticeaxiom_gameplay::ItemStackV1::item),
        Some(&placement_item),
        "aimed placement stack must land on the selected hotbar"
    );
    assert_eq!(
        after_swap
            .slots()
            .get(usize::from(HOTBAR_SLOTS) + 3)
            .and_then(Option::as_ref)
            .map(latticeaxiom_gameplay::ItemStackV1::item),
        Some(&other_item),
        "previous hotbar stack must swap into the body slot"
    );

    clear_inventory(&spine);
    let rejected = spine.pick_aimed_block();
    assert!(
        matches!(rejected, Err(GameplayReject::EmptySlot)),
        "pick-block must fail closed when the placement item is absent, got {rejected:?}"
    );
}

#[test]
fn production_host_move_stack_merges_swaps_and_rejects_empty() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    let boot = lock_boot_fixture();
    let instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog,
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let log = parse_item("terrenia:item/oak-log");
    let dirt = parse_item("terrenia:item/dirt");
    clear_inventory(&spine);
    spine
        .seed_inventory_slot(
            SlotIndex::new(0),
            Some(ItemStackV1::plain(log.clone(), 40).expect("log stack")),
        )
        .expect("from stack is seeded");
    spine
        .seed_inventory_slot(
            SlotIndex::new(1),
            Some(ItemStackV1::plain(log.clone(), 40).expect("merge destination")),
        )
        .expect("to stack is seeded");
    let merged = spine
        .move_stack(SlotIndex::new(0), SlotIndex::new(1))
        .expect("matching stacks merge");
    assert!(matches!(
        merged,
        CommandOutcomeV1::StackMoved { from, to }
            if from == SlotIndex::new(0) && to == SlotIndex::new(1)
    ));
    let after_merge = spine.inventory_view().expect("inventory after merge");
    assert_eq!(
        after_merge
            .slots()
            .get(1)
            .and_then(Option::as_ref)
            .map(ItemStackV1::quantity),
        Some(64)
    );
    assert_eq!(
        after_merge
            .slots()
            .first()
            .and_then(Option::as_ref)
            .map(ItemStackV1::quantity),
        Some(16)
    );

    clear_inventory(&spine);
    spine
        .seed_inventory_slot(
            SlotIndex::new(0),
            Some(ItemStackV1::plain(log.clone(), 8).expect("swap from")),
        )
        .expect("from stack is seeded");
    spine
        .seed_inventory_slot(
            SlotIndex::new(1),
            Some(ItemStackV1::plain(dirt, 4).expect("swap to")),
        )
        .expect("to stack is seeded");
    spine
        .move_stack(SlotIndex::new(0), SlotIndex::new(1))
        .expect("different items swap");
    let after_swap = spine.inventory_view().expect("inventory after swap");
    assert_eq!(
        after_swap
            .slots()
            .first()
            .and_then(Option::as_ref)
            .map(ItemStackV1::item)
            .map(ItemId::as_str),
        Some("terrenia:item/dirt")
    );
    assert_eq!(
        after_swap
            .slots()
            .get(1)
            .and_then(Option::as_ref)
            .map(ItemStackV1::item)
            .map(ItemId::as_str),
        Some("terrenia:item/oak-log")
    );

    let empty = spine.move_stack(SlotIndex::new(2), SlotIndex::new(0));
    assert!(
        matches!(empty, Err(GameplayReject::EmptySlot)),
        "empty from must fail closed, got {empty:?}"
    );
}

#[test]
fn production_host_lists_craftable_hand_and_workbench_recipes() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    let boot = lock_boot_fixture();
    let instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog,
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    let log_item = parse_item("terrenia:item/oak-log");
    let plank_item = parse_item("terrenia:item/oak-planks");
    let stick_item = parse_item("terrenia:item/stick");
    let planks = parse_recipe("terrenia:recipe/oak-planks@1");
    let pickaxe = parse_recipe("terrenia:recipe/wooden-pickaxe@1");
    let crafting = WorkstationId::parse("latticeaxiom:workstation/crafting@1")
        .expect("crafting workstation is a platform contract");

    if let Some((wood_block, log_pos, gathered_wood, _, _)) = first_resident_wood(&spine) {
        gather_until_inventory_has(&spine, log_pos, &gathered_wood, 1);
        if wood_block.as_str().ends_with("pine-log") {
            top_up_item(&spine, &log_item, 4);
        }
    } else {
        top_up_item(&spine, &log_item, 4);
    }
    let hand = spine.craftable_recipe_ids(None);
    assert!(
        hand.contains(&planks),
        "oak-planks must be craftable by hand after wood is gathered, got {hand:?}"
    );
    assert!(
        !hand.contains(&pickaxe),
        "workbench recipes must not appear in the hand list, got {hand:?}"
    );
    assert!(
        spine.craftable_recipe_ids(Some(&crafting)).is_empty(),
        "workbench recipes must stay hidden until the workstation is bound"
    );

    spine
        .craft_recipe(&planks, None)
        .expect("oak planks craft from gathered wood");
    top_up_item(&spine, &plank_item, 3);
    top_up_item(&spine, &stick_item, 2);
    assert!(
        spine.craftable_recipe_ids(Some(&crafting)).is_empty(),
        "matching workbench inputs still require a bound workstation"
    );
    spine
        .bind_workstation(crafting.clone(), ContainerId::new(2))
        .expect("workbench path binds a crafting workstation");
    let bench = spine.craftable_recipe_ids(Some(&crafting));
    assert!(
        bench.contains(&pickaxe),
        "wooden-pickaxe must appear after bind once inputs are owned, got {bench:?}"
    );
}

#[test]
fn production_host_places_torch_and_opens_chest_container_schema() {
    let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
    let torch_block = parse_block("terrenia:block/torch");
    let chest_block = parse_block("terrenia:block/chest");
    let workbench_block = parse_block("terrenia:block/workbench");
    let furnace_block = parse_block("terrenia:block/furnace");
    for block in [&torch_block, &workbench_block, &furnace_block, &chest_block] {
        assert!(
            catalog.block_schema_binding(block).is_some(),
            "{block} must close out a reserved gameplay schema binding"
        );
    }
    let chest_binding = catalog
        .block_schema_binding(&chest_block)
        .expect("chest must close out a reserved gameplay schema binding");
    assert!(
        chest_binding.realizes_container(),
        "chest must realize the generic container schema"
    );

    let boot = lock_boot_fixture();
    let instance = EngineInstance::new_headless_host_from_lock_with_catalog(
        boot.prepared(),
        SPINE_TIMESTEP,
        catalog,
    )
    .expect("production spine starts with package gameplay catalog");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();

    spine
        .bind_block_container(&chest_block, ContainerId::new(3))
        .expect("chest container schema opens from the catalog binding");
    spine
        .bind_workstation(
            WorkstationId::parse("latticeaxiom:workstation/crafting@1")
                .expect("crafting workstation is a platform contract"),
            ContainerId::new(2),
        )
        .expect("workbench container schema binds from the catalog slot count");

    let torch_item = parse_item("terrenia:item/torch");
    spine
        .seed_inventory_slot(
            SlotIndex::new(0),
            Some(ItemStackV1::plain(torch_item.clone(), 4).expect("torch stacks are valid")),
        )
        .expect("torch is seeded into the hotbar");
    spine
        .select_hotbar_slot(0)
        .expect("torch hotbar slot is selected");

    let (_, place_target, _) = first_resident_soil(&spine);
    mine_until_broken(&spine, place_target);
    pickup_remaining(&spine);
    let place_anchor = latticeaxiom_gameplay::BlockPosition {
        x: place_target.x,
        y: place_target.y.saturating_add(1),
        z: place_target.z,
    };
    let placed = spine
        .place_from_hotbar(place_anchor, BlockFaceV1::NegativeY)
        .expect("torch placement consumes the catalog placement item");
    let occupancy = spine
        .inspect_occupancy(placed.position)
        .expect("placed torch cell is inspectable");
    assert_eq!(occupancy.solid.as_ref(), Some(&torch_block));
    assert!(
        spine
            .inventory_view()
            .expect("inventory is bound")
            .count_item(&torch_item)
            < 4,
        "placing a torch must consume the placement stack"
    );
}

fn gather_until_inventory_has(
    spine: &ProductionSpine,
    position: latticeaxiom_gameplay::BlockPosition,
    item: &ItemId,
    quantity: u32,
) {
    let _broken = mine_until_broken(spine, position);
    pickup_remaining(spine);
    let count = spine.inventory_view().expect("inventory").count_item(item);
    assert!(
        count >= quantity,
        "gathering {item} must yield at least {quantity}, got {count}"
    );
}

fn select_item_in_hotbar(spine: &ProductionSpine, item: &ItemId) {
    let view = spine.inventory_view().expect("inventory is bound");
    let slot = view
        .slots()
        .iter()
        .position(|stack| stack.as_ref().is_some_and(|stack| stack.item() == item))
        .unwrap_or_else(|| panic!("{item} must occupy an inventory slot after gathering"));
    if slot >= 9 {
        let stack = view.slots()[slot].clone();
        spine
            .seed_inventory_slot(SlotIndex::new(0), stack)
            .expect("item moves into the hotbar");
        spine
            .select_hotbar_slot(0)
            .expect("hotbar slot 0 is selected");
    } else {
        spine
            .select_hotbar_slot(u16::try_from(slot).expect("hotbar index fits"))
            .expect("gathered item is selected");
    }
}

fn mine_until_broken(
    spine: &ProductionSpine,
    position: latticeaxiom_gameplay::BlockPosition,
) -> latticeaxiom_engine::BlockEditSuccessV1 {
    for _ in 0..64 {
        match spine.mine_cell(position) {
            Ok(success) => return success,
            Err(BlockEditRejectV1::RequiresProgress { .. }) => {}
            Err(error) => panic!(
                "mining {position:?} failed: {error:?}, reject={:?}",
                spine.last_gameplay_reject()
            ),
        }
    }
    panic!("mining {position:?} did not complete")
}

fn clear_inventory(spine: &ProductionSpine) {
    for slot in 0..INVENTORY_SLOTS {
        spine
            .seed_inventory_slot(
                SlotIndex::new(u16::try_from(slot).expect("slot fits")),
                None,
            )
            .expect("inventory slot clears");
    }
}

fn pickup_remaining(spine: &ProductionSpine) {
    let drops: Vec<_> = spine.dropped_items().keys().copied().collect();
    for drop in drops {
        match spine.pickup_drop(drop) {
            Ok(_) | Err(GameplayReject::InventoryFull) => {}
            Err(error) => panic!("pickup failed: {error}"),
        }
    }
}

fn top_up_item(spine: &ProductionSpine, item: &ItemId, minimum: u32) {
    let current = spine.inventory_view().expect("inventory").count_item(item);
    if current >= minimum {
        return;
    }
    let needed = minimum.saturating_sub(current);
    let empty = spine
        .inventory_view()
        .expect("inventory")
        .slots()
        .iter()
        .position(Option::is_none)
        .expect("an empty slot exists for fixture top-up");
    spine
        .seed_inventory_slot(
            SlotIndex::new(u16::try_from(empty).expect("slot fits")),
            Some(ItemStackV1::plain(item.clone(), needed).expect("top-up stack")),
        )
        .expect("fixture top-up of an already gathered item");
}

fn seed_tool(spine: &ProductionSpine, slot: u16, item: &str, durability: u32) {
    spine
        .seed_inventory_slot(
            SlotIndex::new(slot),
            Some(ItemStackV1::tool(parse_item(item), durability).expect("tool stack is valid")),
        )
        .expect("fixture tool is seeded into the hotbar");
    if slot == 0 {
        spine
            .select_hotbar_slot(0)
            .expect("first tool slot is selected");
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn walk_toward_column(
    instance: &mut EngineInstance,
    spine: &ProductionSpine,
    mut generation: u64,
    target_x: i32,
    target_z: i32,
    ticks: u64,
) -> u64 {
    let mut remaining = ticks;
    while remaining > 0 {
        let pose = spine.player_pose();
        let dx = (target_x as f32 + 0.5) - pose.translation.x;
        let dz = (target_z as f32 + 0.5) - pose.translation.z;
        if dx.hypot(dz) < 0.35 {
            break;
        }
        let look = wrap_pi(pose.yaw_radians - f32::atan2(-dx, -dz));
        let step = remaining.min(24);
        let before = generation;
        generation = enqueue_look_then_walk(instance, generation, look, 0.0, 1.0, step);
        let consumed = generation.saturating_sub(before);
        instance
            .advance_fixed_ticks(u32::try_from(consumed).expect("step fits u32"))
            .expect("walk ticks toward the required entrance");
        remaining = remaining.saturating_sub(step);
        assert!(
            !spine.occupies_unready_cave_void(),
            "travel must not enter an unready cave void at {:?}",
            spine.player_pose().translation
        );
    }
    generation
}

fn wait_for_resident(
    instance: &mut EngineInstance,
    spine: &ProductionSpine,
    mut generation: u64,
    position: latticeaxiom_gameplay::BlockPosition,
    ticks: u64,
) -> u64 {
    let mut remaining = ticks;
    while remaining > 0 {
        if spine
            .chunk_of(position)
            .is_some_and(|chunk| spine.chunk_lifecycle(chunk) != ChunkLifecycle::Absent)
        {
            break;
        }
        let step = remaining.min(16);
        instance
            .enqueue_headless_actions((0..step).map(|offset| idle_frame(generation + offset)))
            .expect("wait frames enqueue");
        instance
            .advance_fixed_ticks(u32::try_from(step).expect("wait step fits"))
            .expect("wait ticks stream the entrance column");
        generation = generation.saturating_add(step);
        remaining = remaining.saturating_sub(step);
    }
    generation
}

fn wrap_pi(value: f32) -> f32 {
    let pi = std::f32::consts::PI;
    let tau = 2.0 * pi;
    let mut wrapped = (value + pi) % tau;
    if wrapped < 0.0 {
        wrapped += tau;
    }
    wrapped - pi
}

fn idle_at_hole(
    instance: &mut EngineInstance,
    spine: &ProductionSpine,
    generation: u64,
    ticks: u64,
) -> u64 {
    let frames: Vec<_> = (0..ticks)
        .map(|offset| idle_frame(generation + offset))
        .collect();
    instance
        .enqueue_headless_actions(frames)
        .expect("idle frames enqueue");
    instance
        .advance_fixed_ticks(u32::try_from(ticks).expect("idle ticks fit"))
        .expect("idle ticks at the cave hole");
    assert!(
        !spine.occupies_unready_cave_void(),
        "idle at the hole must not enter an unready cave void"
    );
    generation + ticks
}

fn open_required_entrance_shaft(
    spine: &ProductionSpine,
    surface: latticeaxiom_gameplay::BlockPosition,
    aperture: latticeaxiom_gameplay::BlockPosition,
) {
    let mut opened = 0_u32;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let mut y = surface.y.saturating_add(8);
            while y >= aperture.y {
                let position = latticeaxiom_gameplay::BlockPosition {
                    x: surface.x.saturating_add(dx),
                    y,
                    z: surface.z.saturating_add(dz),
                };
                if spine
                    .cave_occupancy_arbitration(
                        i64::from(position.x),
                        i64::from(position.y),
                        i64::from(position.z),
                    )
                    .is_some_and(latticeaxiom_engine::CaveOccupancyArbitrationV1::is_finally_void)
                {
                    y -= 1;
                    continue;
                }
                if mine_cover_cell(spine, position) {
                    opened = opened.saturating_add(1);
                    y -= 1;
                    continue;
                }
                if spine
                    .inspect_occupancy(position)
                    .ok()
                    .is_some_and(|occupancy| {
                        occupancy.solid.is_none()
                            || occupancy
                                .solid
                                .as_ref()
                                .is_some_and(|block| block.as_str().ends_with("/air"))
                    })
                {
                    y -= 1;
                    continue;
                }
                panic!(
                    "required entrance cover {position:?} did not break, reject={:?}, gameplay={:?}",
                    spine.last_reject(),
                    spine.last_gameplay_reject()
                );
            }
        }
    }
    assert!(
        opened > 0,
        "required entrance column {surface:?} -> {aperture:?} must open at least one cover voxel"
    );
}

fn mine_cover_cell(
    spine: &ProductionSpine,
    position: latticeaxiom_gameplay::BlockPosition,
) -> bool {
    for slot in [1_u16, 0_u16] {
        spine
            .select_hotbar_slot(slot)
            .expect("cover-mining tool is selected");
        for _ in 0..64 {
            match spine.mine_cell(position) {
                Ok(_) => {
                    pickup_remaining(spine);
                    return true;
                }
                Err(BlockEditRejectV1::RequiresProgress { .. }) => {}
                Err(BlockEditRejectV1::RequiresTool { .. } | BlockEditRejectV1::NotBreakable) => {
                    break;
                }
                Err(BlockEditRejectV1::PermissionDenied | BlockEditRejectV1::NoTarget) => {
                    return false;
                }
                Err(error) => panic!("cover mining {position:?} failed: {error:?}"),
            }
        }
    }
    false
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn player_in_cave(
    spine: &ProductionSpine,
    translation: bevy::prelude::Vec3,
    aperture_y: i32,
) -> bool {
    let feet_y = translation.y - 0.9;
    if feet_y <= aperture_y as f32 + 0.75 {
        return true;
    }
    player_sample_void(spine, translation)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn player_topology_domain(spine: &ProductionSpine) -> Option<latticeaxiom_core::StableId> {
    let translation = spine.player_pose().translation;
    let cells = [
        [
            translation.x.floor() as i64,
            (translation.y - 0.9).floor() as i64,
            translation.z.floor() as i64,
        ],
        [
            translation.x.floor() as i64,
            translation.y.floor() as i64,
            translation.z.floor() as i64,
        ],
    ];
    cells
        .iter()
        .find_map(|&[x, y, z]| spine.cave_topology_domain(x, y, z))
}

fn player_sample_void(spine: &ProductionSpine, translation: bevy::prelude::Vec3) -> bool {
    let cells = [
        [
            translation.x.floor() as i64,
            (translation.y - 0.9).floor() as i64,
            translation.z.floor() as i64,
        ],
        [
            translation.x.floor() as i64,
            translation.y.floor() as i64,
            translation.z.floor() as i64,
        ],
    ];
    cells.iter().any(|&[x, y, z]| {
        spine
            .cave_occupancy_arbitration(x, y, z)
            .is_some_and(latticeaxiom_engine::CaveOccupancyArbitrationV1::is_finally_void)
    })
}

fn parse_block(id: &str) -> BlockId {
    BlockId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn parse_item(id: &str) -> ItemId {
    ItemId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn parse_recipe(id: &str) -> RecipeId {
    RecipeId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn first_resident_any(
    spine: &ProductionSpine,
    ids: &[&str],
) -> (BlockId, latticeaxiom_gameplay::BlockPosition) {
    for id in ids {
        let block = parse_block(id);
        if let Some(position) = spine.first_resident_block(&block) {
            return (block, position);
        }
    }
    panic!(
        "none of {ids:?} exist in the streamed set {:?}",
        spine.resident_chunks()
    );
}

fn first_resident_wood(
    spine: &ProductionSpine,
) -> Option<(
    BlockId,
    latticeaxiom_gameplay::BlockPosition,
    ItemId,
    RecipeId,
    ItemId,
)> {
    for id in ["terrenia:block/oak-log", "terrenia:block/pine-log"] {
        let block = parse_block(id);
        if let Some(position) = spine.first_resident_block(&block) {
            return Some(if id.ends_with("oak-log") {
                (
                    block,
                    position,
                    parse_item("terrenia:item/oak-log"),
                    parse_recipe("terrenia:recipe/oak-planks@1"),
                    parse_item("terrenia:item/oak-planks"),
                )
            } else {
                (
                    block,
                    position,
                    parse_item("terrenia:item/pine-log"),
                    parse_recipe("terrenia:recipe/pine-planks@1"),
                    parse_item("terrenia:item/pine-planks"),
                )
            });
        }
    }
    None
}

fn first_resident_soil(
    spine: &ProductionSpine,
) -> (BlockId, latticeaxiom_gameplay::BlockPosition, ItemId) {
    let (block, position) = first_resident_any(
        spine,
        &[
            "terrenia:block/dirt",
            "terrenia:block/coarse-dirt",
            "terrenia:block/peat",
            "terrenia:block/mud",
            "terrenia:block/sand",
        ],
    );
    let item = parse_item(&block.as_str().replace(":block/", ":item/"));
    (block, position, item)
}
