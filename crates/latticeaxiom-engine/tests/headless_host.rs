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
    ChunkLifecycle, ChunkMeshCursor, ChunkPresentation, ContainerId, DropEntityId, EngineInstance,
    EngineInstanceError, FluidFlowV1, FluidLevelV1, FluidStateV1, GameplayReject,
    HeadlessTargetInspectV1, INVENTORY_SLOTS, ItemId, ItemStackV1, LockVerifiedComposeImages,
    MAX_TICKS_PER_ADVANCE, PlayerActionButtonsV1, PlayerActionFrameV1, PlayerActionV1,
    PreparationError, ProductionInspectSurface, ProductionMemoryStart, ProductionSpine,
    ProductionWorldList, ProductionWorldStorage, RecipeId, SealedWorldWriterHost,
    SealedWriterHostError, SlotIndex, StructurallyValidatedComposeImages, VerifiedProductLockHash,
    WorkingSetDiagnosticsV1, WorkstationId, authored_gameplay_catalog, empty_gameplay_catalog,
};
use latticeaxiom_gameplay::BlockId;
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

    let dirt = parse_block("terrenia:block/dirt");
    let dirt_item = parse_item("terrenia:item/dirt");
    let broken_pos = spine
        .first_resident_block(&dirt)
        .expect("generated dirt exists in the streamed set");
    let occupancy_before = spine
        .inspect_occupancy(broken_pos)
        .expect("dirt cell is inspectable before the break");
    assert_eq!(occupancy_before.solid.as_ref(), Some(&dirt));
    let broken = mine_until_broken(&spine, broken_pos);
    pickup_remaining(&spine);
    let occupancy_gone = spine
        .inspect_occupancy(broken.position)
        .expect("broken cell remains inspectable");
    assert_ne!(
        occupancy_gone.solid.as_ref(),
        Some(&dirt),
        "break must clear the dirt cell before flush"
    );

    let place_target = spine
        .first_resident_block(&dirt)
        .expect("a second dirt cell remains after the first break");
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
    assert!(
        !inspect.block_display_name.is_empty(),
        "inspect DTO must carry a targeted block display name"
    );
    assert_ne!(inspect.block_display_name, inspect.block_id.as_str());
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
        240,
    );
    generation = idle_at_hole(&mut instance, &spine, generation, 120);
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
            let mut y = surface.y;
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
