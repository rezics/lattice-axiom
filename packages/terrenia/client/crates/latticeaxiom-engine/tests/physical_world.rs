//! Real disk-backed creation and inventory reopen through the production spine.
#![allow(
    clippy::expect_used,
    reason = "acceptance assertions describe fixture invariants"
)]

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use bevy::prelude::{App, MinimalPlugins};
use latticeaxiom_compose::{LockRequest, lock_workspace};
use latticeaxiom_core::CanonicalHash;
use latticeaxiom_engine::{
    LockVerifiedComposeImages, ProductionMemoryStart, ProductionSpine, SealedWorldWriterHost,
};
use latticeaxiom_gameplay::{ItemId, ItemStackV1, SlotIndex};
use latticeaxiom_world_db::DiskWorldStore;

fn load_lock_verified_images_from(
    root: &Path,
    path: &Path,
) -> Result<LockVerifiedComposeImages, Box<dyn std::error::Error>> {
    let lock = latticeaxiom_compose::reopen_product_lock(path)?;
    let host = latticeaxiom_launcher::HostBuildReceipts::sealed_by_lock(&lock)?;
    let store = latticeaxiom_packages::FilesystemCas::open(root.join("catalog/cas"))?;
    let reopened =
        latticeaxiom_launcher::ReopenedFinalLockV1::reopen_frozen_from_cas(path, &store, &host)?;
    let target = lock.realizations.keys().next().expect("one test target");
    Ok(LockVerifiedComposeImages::from_reopened_product_lock(
        &reopened, target,
    )?)
}

fn prepare(workspace: &Path, output: &Path, bootstrap: &str, lock: &str) {
    let target = if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    lock_workspace(&LockRequest {
        workspace_root: workspace.to_owned(),
        bootstrap_path: PathBuf::from(bootstrap),
        catalog_root: output.join("catalog"),
        lock_path: output.join(lock),
        target: target.parse().expect("supported test target"),
        toolchain: CanonicalHash::digest(b"physical-world-test"),
    })
    .expect("prepare frozen fixture lock");
}

#[test]
fn disk_world_reopens_after_the_original_engine_and_store_are_dropped() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../..");
    let directory = tempfile::tempdir().expect("isolated acceptance workspace");
    let root = directory.path();
    prepare(&workspace, root, "profiles/play.toml", "latticeaxiom.lock");
    prepare(&workspace, root, "profiles/shell.toml", "shell.lock");
    let game =
        load_lock_verified_images_from(root, &root.join("latticeaxiom.lock")).expect("game lock");
    let shell = load_lock_verified_images_from(root, &root.join("shell.lock")).expect("shell lock");
    let path = root.join("worlds.redb");
    let disk = DiskWorldStore::open(&path).expect("physical repository");
    let mut start = ProductionMemoryStart::from_disk_images(&shell, game.clone(), disk.clone())
        .expect("disk library");
    let intent = start
        .quick_create_intent("Disk acceptance")
        .expect("world intent");
    let world = start.create(&intent, 1).expect("physical creation");
    // Initialize Bevy's shared task pools before the standalone spine materializes.
    {
        let mut pools = App::new();
        pools.add_plugins(MinimalPlugins);
    }
    let instance = start
        .play_headless(world, 2, Duration::from_secs_f64(1.0 / 60.0))
        .expect("named world");
    let spine = instance.app().world().resource::<ProductionSpine>().clone();
    assert_eq!(
        spine.gameplay_mode(),
        Some(latticeaxiom_gameplay::GameplayModeV1::Survival)
    );
    assert!(
        spine
            .inventory_view()
            .expect("new player inventory")
            .slots()
            .iter()
            .all(Option::is_none)
    );
    let item: ItemId = "terrenia:item/copper-ore".parse().expect("item identity");
    let stack = ItemStackV1::plain(item.clone(), 7).expect("test stack");
    spine
        .seed_inventory_slot(SlotIndex::new(4), Some(stack))
        .expect("persistence fixture inventory");
    let expected_pose = spine.player_pose().translation;
    let storage = start.storage().expect("physical working store").clone();
    let mut writer = SealedWorldWriterHost::new(storage.clone());
    start
        .flush_dirty_chunks(world, &mut writer)
        .expect("sealed writer publication");
    writer.flush_durable().expect("logical durable frontier");
    writer.close().expect("writer closed before publication");
    assert_eq!(
        spine.pending_persistence_count(),
        0,
        "all generated chunks were saved"
    );
    assert_eq!(
        storage
            .keyspace_stats()
            .expect("resident store payloads")
            .record_entries(),
        0
    );
    let explored = spine.resident_chunks();
    assert!(
        explored.len() > 1,
        "acceptance includes pristine terrain outside the player record"
    );
    let view = latticeaxiom_world_db::WorldStorage::begin_read(&storage, world).expect("disk view");
    for coordinate in &explored {
        let key = latticeaxiom_storage::ChunkKey::new(
            world,
            spine.dimension_id().expect("dimension"),
            *coordinate,
        );
        assert!(
            view.load_chunk(&key).expect("stored terrain").is_some(),
            "untouched chunk {coordinate:?} missing"
        );
    }
    drop(view);
    spine
        .move_stack(SlotIndex::new(4), SlotIndex::new(5))
        .expect("gameplay continues after a physical save without stale chunk revisions");
    let mut next_writer = SealedWorldWriterHost::new(storage.clone());
    start
        .flush_dirty_chunks(world, &mut next_writer)
        .expect("save post-autosave gameplay");
    next_writer.close().expect("close post-autosave writer");
    drop(next_writer);
    let entry = disk.entries().expect("catalog").remove(0);
    disk.publish(&storage, &entry)
        .expect("physical Immediate commit");
    drop((spine, instance, start, writer, storage, disk));
    // Changing the default product must not reinterpret an existing world's rules.
    prepare(&workspace, root, "profiles/dev.toml", "latticeaxiom.lock");
    let replacement_game = load_lock_verified_images_from(root, &root.join("latticeaxiom.lock"))
        .expect("replacement default game");
    assert_ne!(
        replacement_game.product_lock_hash(),
        game.product_lock_hash()
    );
    let reopened_disk = DiskWorldStore::open(&path).expect("independent physical reopen");
    let entries = reopened_disk.entries().expect("persisted catalog");
    assert_eq!(entries[0].world, world);
    assert!(entries[0].durable_revision > 0);
    let mut reopened =
        ProductionMemoryStart::from_disk_images(&shell, replacement_game, reopened_disk)
            .expect("restored library")
            .with_frozen_lock_catalog(root.join("catalog"))
            .expect("original game lock library");
    let handoff = reopened
        .launch_handoff_for_ready_exact(
            world,
            3,
            latticeaxiom_launcher::SettingTransactionRevision::new(0),
        )
        .expect("shell selects the original world closure");
    assert_eq!(
        handoff.intent.world_lock_hash(),
        Some(game.product_lock_hash())
    );
    let host = reopened
        .play_reopened_headless(world, 3, Duration::from_secs_f64(1.0 / 60.0))
        .expect("restore persisted world");
    let restored = host.app().world().resource::<ProductionSpine>();
    assert_eq!(
        restored.gameplay_mode(),
        Some(latticeaxiom_gameplay::GameplayModeV1::Survival)
    );
    assert_eq!(restored.world_id(), Some(world));
    assert_eq!(
        restored
            .inventory_view()
            .expect("inventory")
            .count_item(&item),
        7
    );
    assert!(restored.player_pose().translation.distance(expected_pose) < 0.002);
    assert!(restored.inventory_view().expect("inventory").slots()[4].is_none());
    assert_eq!(
        restored.inventory_view().expect("inventory").slots()[5]
            .as_ref()
            .expect("moved stack")
            .quantity(),
        7
    );
    assert_eq!(
        restored.pending_persistence_count(),
        0,
        "stored startup terrain was hydrated without regeneration"
    );
    exercise_traversal(restored, &mut reopened, world);
}

fn exercise_traversal(
    spine: &ProductionSpine,
    start: &mut ProductionMemoryStart,
    world: latticeaxiom_core::WorldId,
) {
    use std::collections::BTreeSet;
    let initial_pose = spine.player_pose();
    let mut seen = BTreeSet::new();
    let mut high_cached = 0;
    let mut tick = 10_000;
    let maximum = usize::try_from(spine.hard_limits().expect("limits").max_resident_chunks)
        .expect("count fits");
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    for station in 0..8_u16 {
        let mut pose = initial_pose;
        pose.translation.x += f32::from(station) * 256.0;
        spine.record_player_pose(pose);
        tick += 1_000;
        let before = seen.len();
        for _ in 0..600 {
            tick += 1;
            spine.sync_interest(tick).expect("traversal interest");
            spine
                .pump_background_work(tick)
                .expect("bounded background work");
            seen.extend(spine.resident_chunks());
            high_cached = high_cached.max(spine.cached_chunk_count());
            assert!(
                spine.cached_chunk_count() <= maximum,
                "kernel must honor the resident ceiling"
            );
            assert!(
                spine.pending_persistence_count() <= maximum,
                "pending writes cannot retain an unbounded exploration history"
            );
            if spine.pending_persistence_count() >= 8 || seen.len() >= before + 8 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "traversal must progress"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut writer = SealedWorldWriterHost::new(start.storage().expect("storage").clone());
        start
            .flush_dirty_chunks(world, &mut writer)
            .expect("incremental traversal save");
        writer.close().expect("close traversal writer");
        assert_eq!(spine.pending_persistence_count(), 0);
        assert!(
            seen.len() > before,
            "each distant region must contribute stored terrain"
        );
    }
    assert!(
        spine.stream_eviction_count() > 0,
        "traversal exercised eviction"
    );
    assert!(
        seen.len() > spine.cached_chunk_count(),
        "evicted world data must leave the kernel cache"
    );
    assert!(high_cached <= maximum);
}
