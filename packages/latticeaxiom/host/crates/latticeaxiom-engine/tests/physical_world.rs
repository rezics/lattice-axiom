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
    prepare(&workspace, root, "profiles/dev.toml", "latticeaxiom.lock");
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
    let entry = disk.entries().expect("catalog").remove(0);
    disk.publish(&storage, &entry)
        .expect("physical Immediate commit");
    drop((spine, instance, start, writer, storage, disk));
    let reopened_disk = DiskWorldStore::open(&path).expect("independent physical reopen");
    let entries = reopened_disk.entries().expect("persisted catalog");
    assert_eq!(entries[0].world, world);
    assert!(entries[0].durable_revision > 0);
    let mut reopened = ProductionMemoryStart::from_disk_images(&shell, game, reopened_disk)
        .expect("restored library");
    let host = reopened
        .play_reopened_headless(world, 3, Duration::from_secs_f64(1.0 / 60.0))
        .expect("restore persisted world");
    let restored = host.app().world().resource::<ProductionSpine>();
    assert_eq!(restored.world_id(), Some(world));
    assert_eq!(
        restored
            .inventory_view()
            .expect("inventory")
            .count_item(&item),
        7
    );
    assert!(restored.player_pose().translation.distance(expected_pose) < 0.002);
}
