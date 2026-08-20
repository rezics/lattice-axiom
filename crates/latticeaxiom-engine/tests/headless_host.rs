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
    ActionAxis2V1, AuthoritativeTransactionKernel, ChunkCoordinate, ChunkLifecycle,
    ChunkMeshCursor, ChunkPresentation, EngineInstance, EngineInstanceError,
    LockVerifiedComposeImages, MAX_TICKS_PER_ADVANCE, PlayerActionButtonsV1, PlayerActionFrameV1,
    PlayerActionV1, PreparationError, ProductionInspectSurface, ProductionSpine,
    ProductionWorldStorage, StructurallyValidatedComposeImages, VerifiedProductLockHash,
    WorkingSetDiagnosticsV1,
};
use latticeaxiom_launcher::{HostBuildReceipts, ProductLockBootError, ReopenedFinalLockV1};
use latticeaxiom_registration::{
    CallbackDeclaration, CompiledRegistration, PackageRegistrationInput, ReceiptValidationError,
    RegistrationCompileInput, RegistrationCompiler, SystemDeclaration,
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
