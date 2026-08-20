//! Integration coverage for receipt-verified GPU-free Bevy hosts.
#![allow(clippy::expect_used)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    num::{NonZeroU8, NonZeroU32},
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
    ActionAxis2V1, AuthoritativeTransactionKernel, ChunkCoordinate, ChunkLifecycle,
    ChunkMeshCursor, ChunkPresentation, ContainerId, DropEntityId, EngineInstance,
    EngineInstanceError, GameplayCatalog, GameplayReject, INVENTORY_SLOTS, ItemId, ItemStackV1,
    LockVerifiedComposeImages, MAX_TICKS_PER_ADVANCE, PlayerActionButtonsV1, PlayerActionFrameV1,
    PlayerActionV1, PreparationError, ProductionInspectSurface, ProductionMemoryStart,
    ProductionSpine, ProductionWorldList, ProductionWorldStorage, RecipeId, SlotIndex,
    StructurallyValidatedComposeImages, VerifiedProductLockHash, WorkingSetDiagnosticsV1,
    WorkstationId,
};
use latticeaxiom_gameplay::{
    BlockDefinitionV1, BlockId, CatalogLimits, FrozenItemRoleBindingV1, GameplayCatalogSourceV1,
    IngredientV1, ItemDefinitionV1, ItemPredicateV1, ItemRoleDefinitionV1, ItemRoleId,
    MiningRuleV1, RecipeDefinitionV1, RecipePatternV1, RoleOutputV1, ToolClassId, ToolDefinitionV1,
    ToolRequirementV1, WorkstationDefinitionV1,
};
use latticeaxiom_launcher::{HostBuildReceipts, ProductLockBootError, ReopenedFinalLockV1};
use latticeaxiom_player::{BlockEditRejectV1, BlockFaceV1};
use latticeaxiom_registration::{
    CallbackDeclaration, CompiledRegistration, PackageRegistrationInput, ReceiptValidationError,
    RegistrationCompileInput, RegistrationCompiler, SystemDeclaration,
};
use latticeaxiom_start_ui::{
    ClientShellGraph, InputSource, MemoryStartEffect, SemanticActionId, SemanticCommand,
    SemanticNodeId, ShellCapability, ShellEffect, ShellPackageProvider,
};
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
    let mut instance = EngineInstance::new_headless_host_from_lock(images, SPINE_TIMESTEP)
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
        / 2;
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
fn production_spine_headless_inspect_reports_targeted_block_id_after_dda() {
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

const TERRENIA_BLOCKS_JSON: &str =
    include_str!("../../../packages/terrenia/blocks/data/authored-catalog-v1.json");
const TERRENIA_RULES_JSON: &str =
    include_str!("../../../packages/terrenia/gameplay/data/authored-rules-v1.json");
const TERRENIA_TOOLS_JSON: &str =
    include_str!("../../../packages/terrenia/tools/data/authored-tools-v1.json");

#[test]
#[allow(clippy::too_many_lines)]
fn production_host_gathers_crafts_mines_with_tools_and_fails_closed() {
    let catalog = package_gameplay_catalog();
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

    let dirt_block = parse_block("terrenia:block/dirt");
    let log_block = parse_block("terrenia:block/oak-log");
    let stone_block = parse_block("terrenia:block/stone");
    let dirt_item = parse_item("terrenia:item/dirt");
    let log_item = parse_item("terrenia:item/oak-log");
    let stick_item = parse_item("terrenia:item/stick");
    let plank_item = parse_item("terrenia:item/oak-planks");
    let workbench_item = parse_item("terrenia:item/workbench");
    let pickaxe_item = parse_item("terrenia:item/wooden-pickaxe");
    let shovel_item = parse_item("terrenia:item/wooden-shovel");

    let dirt_pos = spine
        .first_resident_block(&dirt_block)
        .expect("generated soil exists in the streamed set");
    let log_pos = spine
        .first_resident_block(&log_block)
        .expect("generated wood exists in the streamed set");
    gather_until_inventory_has(&spine, dirt_pos, &dirt_item, 1);
    gather_until_inventory_has(&spine, log_pos, &log_item, 1);
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
            .expect("oak planks craft from gathered wood");
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
        .or_else(|| spine.first_resident_block(&dirt_block))
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

fn parse_block(id: &str) -> BlockId {
    BlockId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn parse_item(id: &str) -> ItemId {
    ItemId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn parse_recipe(id: &str) -> RecipeId {
    RecipeId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn package_gameplay_catalog() -> GameplayCatalog {
    let blocks = serde_json::from_str(TERRENIA_BLOCKS_JSON).expect("blocks JSON");
    let rules = serde_json::from_str(TERRENIA_RULES_JSON).expect("rules JSON");
    let tools = serde_json::from_str(TERRENIA_TOOLS_JSON).expect("tools JSON");
    GameplayCatalog::compile(
        authored_catalog_source(&blocks, &rules, &tools),
        CatalogLimits::default(),
    )
    .unwrap_or_else(|error| panic!("package gameplay catalog must compile: {error}"))
}

fn authored_catalog_source(
    blocks: &Value,
    rules: &Value,
    tools: &Value,
) -> GameplayCatalogSourceV1 {
    let mut items = BTreeMap::new();
    let mut item_roles = Vec::new();
    let mut bindings = Vec::new();
    let mut recipes = Vec::new();
    let mut workstations = BTreeMap::new();
    ingest_items(json_array(rules, "items"), &mut items);
    ingest_items(json_array(tools, "items"), &mut items);
    ingest_roles(
        json_array(rules, "recipe_output_roles"),
        &mut item_roles,
        &mut bindings,
    );
    ingest_roles(
        json_array(tools, "recipe_output_roles"),
        &mut item_roles,
        &mut bindings,
    );
    ingest_recipes(
        json_array(rules, "recipes"),
        &mut recipes,
        &mut workstations,
        false,
    );
    ingest_recipes(
        json_array(tools, "recipes"),
        &mut recipes,
        &mut workstations,
        true,
    );
    let drop_tables = drop_table_map(json_array(rules, "drop_tables"));
    let tool_requirements = tool_requirement_map(json_array(rules, "tool_requirements"));
    GameplayCatalogSourceV1 {
        items: items.into_values().collect(),
        blocks: compile_blocks(
            json_array(blocks, "blocks"),
            &drop_tables,
            &tool_requirements,
        ),
        tools: compile_tools(json_array(tools, "tools")),
        roles: item_roles,
        bindings,
        recipes,
        workstations: workstations.into_values().collect(),
        ..GameplayCatalogSourceV1::default()
    }
}

fn ingest_items(rows: &[Value], items: &mut BTreeMap<String, ItemDefinitionV1>) {
    for row in rows {
        let id = json_text(row, "id");
        let item = ItemId::parse(id).unwrap_or_else(|error| panic!("{id}: {error}"));
        let durability = row
            .get("durability")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .and_then(NonZeroU32::new);
        let placement = row
            .get("placement_block")
            .and_then(Value::as_str)
            .map(|value| BlockId::parse(value).expect("placement block parses"));
        let stack = row
            .get("maximum_stack")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .and_then(NonZeroU32::new)
            .unwrap_or(NonZeroU32::MIN);
        items.insert(
            id.to_owned(),
            ItemDefinitionV1 {
                id: item,
                stack_limit: if durability.is_some() {
                    NonZeroU32::MIN
                } else {
                    stack
                },
                placement_block: placement,
                durability,
            },
        );
    }
}

fn ingest_roles(
    rows: &[Value],
    roles: &mut Vec<ItemRoleDefinitionV1>,
    bindings: &mut Vec<FrozenItemRoleBindingV1>,
) {
    for row in rows {
        let id = json_text(row, "id");
        let item = json_text(row, "concrete_item");
        let role = ItemRoleId::parse(id).unwrap_or_else(|error| panic!("{id}: {error}"));
        let concrete = ItemId::parse(item).unwrap_or_else(|error| panic!("{item}: {error}"));
        roles.push(ItemRoleDefinitionV1 {
            id: role.clone(),
            accepts: ItemPredicateV1::Exact(concrete.clone()),
        });
        bindings.push(FrozenItemRoleBindingV1 {
            role,
            item: concrete,
        });
    }
}

fn ingest_recipes(
    rows: &[Value],
    recipes: &mut Vec<RecipeDefinitionV1>,
    workstations: &mut BTreeMap<String, WorkstationDefinitionV1>,
    require_pattern: bool,
) {
    for row in rows {
        let kind = json_text(row, "kind");
        if kind == "tool-interaction" {
            continue;
        }
        let id = json_text(row, "id");
        let Some(pattern) = recipe_pattern(row, kind, require_pattern) else {
            continue;
        };
        let recipe = RecipeId::parse(id).unwrap_or_else(|error| panic!("{id}: {error}"));
        let workstation = row.get("workstation").and_then(Value::as_str).map(|value| {
            let workstation =
                WorkstationId::parse(value).unwrap_or_else(|error| panic!("{value}: {error}"));
            workstations.insert(
                value.to_owned(),
                WorkstationDefinitionV1 {
                    id: workstation.clone(),
                },
            );
            workstation
        });
        recipes.push(RecipeDefinitionV1 {
            id: recipe,
            workstation,
            pattern,
            output: RoleOutputV1 {
                role: ItemRoleId::parse(json_text(row, "output_role"))
                    .expect("recipe output role parses"),
                quantity: json_quantity(&row["output"], "quantity"),
            },
        });
    }
}

fn recipe_pattern(row: &Value, kind: &str, require_pattern: bool) -> Option<RecipePatternV1> {
    if kind == "shapeless" {
        let ingredients = json_array(row, "inputs")
            .iter()
            .map(|input| IngredientV1 {
                accepts: ItemPredicateV1::Exact(
                    ItemId::parse(json_text(input, "item")).expect("ingredient item parses"),
                ),
                quantity: json_quantity(input, "quantity"),
            })
            .collect();
        return Some(RecipePatternV1::Shapeless { ingredients });
    }
    if kind == "shaped" {
        if let Some(pattern) = row.get("pattern").and_then(Value::as_array) {
            let width = u8::try_from(row["width"].as_u64().unwrap_or(1)).ok()?;
            let height = u8::try_from(row["height"].as_u64().unwrap_or(1)).ok()?;
            let mut cells = Vec::new();
            for line in pattern {
                for cell in line.as_array().expect("pattern row") {
                    cells.push(cell.as_str().map(|item| IngredientV1 {
                        accepts: ItemPredicateV1::Exact(ItemId::parse(item).expect("cell item")),
                        quantity: NonZeroU32::MIN,
                    }));
                }
            }
            return Some(RecipePatternV1::Shaped {
                width: NonZeroU8::new(width)?,
                height: NonZeroU8::new(height)?,
                cells: cells.into_boxed_slice(),
            });
        }
        if require_pattern {
            return None;
        }
        let inputs = json_array(row, "inputs");
        if json_text(row, "id").ends_with("/workbench@1")
            && inputs.len() == 1
            && json_quantity(&inputs[0], "quantity").get() == 4
        {
            let plank = ItemId::parse(json_text(&inputs[0], "item")).expect("workbench plank");
            let cell = Some(IngredientV1 {
                accepts: ItemPredicateV1::Exact(plank),
                quantity: NonZeroU32::MIN,
            });
            return Some(RecipePatternV1::Shaped {
                width: NonZeroU8::new(2)?,
                height: NonZeroU8::new(2)?,
                cells: vec![cell.clone(), cell.clone(), cell.clone(), cell].into_boxed_slice(),
            });
        }
    }
    None
}

fn compile_blocks(
    rows: &[Value],
    drop_tables: &BTreeMap<String, ItemStackV1>,
    tool_requirements: &BTreeMap<String, Option<ToolRequirementV1>>,
) -> Vec<BlockDefinitionV1> {
    let mut blocks = Vec::new();
    for row in rows {
        let id = json_text(&row["definition"]["header"], "stable_id");
        let hardness = row["physical"]["hardness_ticks"].as_u64().unwrap_or(0);
        let Some(hardness) = u32::try_from(hardness).ok().and_then(NonZeroU32::new) else {
            continue;
        };
        let drop_id = json_text(&row["definition"]["rules"], "drop_table");
        let Some(drop) = drop_tables.get(drop_id).cloned() else {
            continue;
        };
        let tool_id = json_text(row, "tool_requirement");
        let tool = tool_requirements.get(tool_id).cloned().flatten();
        blocks.push(BlockDefinitionV1 {
            id: BlockId::parse(id).expect("block id parses"),
            mining: MiningRuleV1 { hardness, tool },
            drop,
        });
    }
    blocks
}

fn compile_tools(rows: &[Value]) -> Vec<ToolDefinitionV1> {
    rows.iter()
        .map(|row| {
            let item = json_text(row, "item");
            let class = json_text(row, "class");
            let durability = json_quantity(row, "durability");
            let multiplier = row
                .get("mining_multiplier")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .and_then(NonZeroU32::new)
                .unwrap_or(NonZeroU32::MIN);
            ToolDefinitionV1 {
                item: ItemId::parse(item).expect("tool item parses"),
                class: ToolClassId::parse(class).expect("tool class parses"),
                tier: u8::try_from(row["tier"].as_u64().unwrap_or(1)).unwrap_or(1),
                work_per_step: multiplier,
                maximum_durability: durability,
            }
        })
        .collect()
}

fn drop_table_map(rows: &[Value]) -> BTreeMap<String, ItemStackV1> {
    let mut tables = BTreeMap::new();
    for row in rows {
        let outputs = json_array(row, "outputs");
        if outputs.len() != 1 {
            continue;
        }
        let item = json_text(&outputs[0], "item");
        let quantity = json_quantity(&outputs[0], "quantity");
        tables.insert(
            json_text(row, "id").to_owned(),
            ItemStackV1::plain(ItemId::parse(item).expect("drop item"), quantity.get())
                .expect("drop stack"),
        );
    }
    tables
}

fn tool_requirement_map(rows: &[Value]) -> BTreeMap<String, Option<ToolRequirementV1>> {
    let mut requirements = BTreeMap::new();
    for row in rows {
        let class = json_text(row, "tool_class");
        let minimum_tier = u8::try_from(row["minimum_tier"].as_u64().unwrap_or(0)).unwrap_or(0);
        let required =
            if matches!(class, "none" | "hand") || (class != "pickaxe" && minimum_tier == 0) {
                None
            } else {
                let id = if class.contains(':') {
                    class.to_owned()
                } else {
                    format!("latticeaxiom:tool-class/{class}@1")
                };
                Some(ToolRequirementV1 {
                    class: ToolClassId::parse(&id).expect("required tool class"),
                    minimum_tier,
                })
            };
        requirements.insert(json_text(row, "id").to_owned(), required);
    }
    requirements
}

fn json_array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn json_text<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{key} is text"))
}

fn json_quantity(value: &Value, key: &str) -> NonZeroU32 {
    let quantity = value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(1);
    NonZeroU32::new(quantity).expect("quantity is non-zero")
}
