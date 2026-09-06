//! Physical-world boot and publication, sharing the sealed storage contracts.

use std::sync::Arc;

use latticeaxiom_core::{CanonicalHash, WorldId};
use latticeaxiom_start_ui::{WorldCardMetadata, WorldShellRecord};
use latticeaxiom_world_catalog::{
    CatalogEntry, CatalogEntryState, CatalogProjection, LiveWorldLocation, ReconciliationState,
    WorldOpenAction, WorldOpenPlan, WorldOpenRisk, WorldOpenStatus, WorldRootId,
};
use latticeaxiom_world_db::{
    DeterministicHeaderPublisher, DeterministicWorldStorage, DiskWorldEntryV1, DiskWorldStore,
    WorldStorage, WorldStorageLimitsV1,
};
use latticeaxiom_world_wire::WorldWireLimits;

use super::ProductionMemoryStartError;

#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
pub(crate) use client::{
    InProcessShellPlay, PersistentGameSession, ReturnToShell, insert_disk_session_on_world,
    install_disk_session,
};
#[cfg(feature = "client")]
pub(super) fn add_save_systems(app: &mut bevy::prelude::App) {
    client::add_save_systems(app);
}

pub(super) fn new_working_store() -> Result<DeterministicWorldStorage, ProductionMemoryStartError> {
    Ok(DeterministicWorldStorage::durable(
        "latticeaxiom:schema/world-db-chunk@1".parse()?,
        WorldWireLimits::default(),
        WorldStorageLimitsV1::D3_BOOTSTRAP,
        Arc::new(DeterministicHeaderPublisher::new()),
    ))
}

pub(crate) fn load_disk_world(
    disk: &DiskWorldStore,
    world: WorldId,
) -> Result<DeterministicWorldStorage, ProductionMemoryStartError> {
    let image = disk.load(world)?;
    let storage = DeterministicWorldStorage::from_durable_image(
        &image,
        "latticeaxiom:schema/world-db-chunk@1".parse()?,
        WorldWireLimits::default(),
        WorldStorageLimitsV1::D3_BOOTSTRAP,
        Arc::new(DeterministicHeaderPublisher::new()),
    )?;
    let preflight = storage.preflight(world)?;
    if let Some(permit) = preflight.header_repair_permit() {
        storage.repair_header(permit.clone())?;
    }
    Ok(storage)
}

pub(super) fn catalog_record(
    entry: &DiskWorldEntryV1,
    game_lock: CanonicalHash,
) -> Result<WorldShellRecord, ProductionMemoryStartError> {
    let plan = (entry.game_lock == game_lock).then(|| WorldOpenPlan {
        world_id: entry.world,
        status: WorldOpenStatus::ReadyExact,
        risk: WorldOpenRisk::None,
        reconciliation: ReconciliationState::InSync {
            metadata_epoch: entry.metadata_epoch,
        },
        next_safe_step: Some(WorldOpenAction::UseFrozenLock),
        actions: vec![WorldOpenAction::UseFrozenLock],
        diagnostics: Vec::new(),
        activation_binding: None,
    });
    Ok(WorldShellRecord::new(
        CatalogEntry {
            location: LiveWorldLocation::new(WorldRootId(1), entry.world),
            state: CatalogEntryState::Projected(CatalogProjection {
                world_id: entry.world,
                display_name: entry.display_name.clone(),
                metadata_epoch: entry.metadata_epoch,
                clean_shutdown: true,
                durable_frontier: entry.durable_revision,
            }),
        },
        WorldCardMetadata {
            created_at_ms: entry.created_at_ms,
            last_played_at_ms: entry.last_played_at_ms,
            physical_bytes: None,
            game_summary: None,
            dimension_summary: None,
        },
        plan,
    )?)
}
