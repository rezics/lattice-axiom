//! Host-owned durable player/session envelope stored as a spawn-chunk entity.
//!
//! The envelope is a versioned JSON payload inside a chunk persistent-entity
//! record. It never carries Bevy, ABI, or physics handles.

use latticeaxiom_core::{SchemaId, canonical_json_bytes};
use latticeaxiom_gameplay::{
    AuthorityTick, BlockKey, BlockPosition, ContainerId, ContainerOwnerComponentV1,
    ContainerStateV1, DimensionChunkKey, DimensionId, DropEntityId, DroppedItemV1,
    FurnaceContinuationV1, GameplayEditTarget, GameplayStorageDomain, ItemId, ItemStackV1,
    ItemStateV1, ProcessId, SlotIndex, WorkstationId,
};
use latticeaxiom_storage::{
    ChunkCoordinate, ContinuationId, PayloadSchemaVersion, PersistentEntityId, VersionedPayload,
};
use serde::{Deserialize, Serialize};

use super::{ProductionHostError, ProductionPlayerPose};

/// Schema identity for [`DurablePlayerSessionV1`].
pub(super) const PLAYER_SESSION_SCHEMA: &str = "latticeaxiom:schema/player-session@1";
/// Schema major stored with the session payload.
pub(super) const PLAYER_SESSION_SCHEMA_VERSION: u32 = 1;
/// Reserved persistent-entity identity for the local-player session envelope.
pub(super) const PLAYER_SESSION_ENTITY: PersistentEntityId =
    PersistentEntityId::from_u128(0x4C41_5853_4553_5301);

/// Canonical player, inventory, container, and scheduled state for V3 reopen.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DurablePlayerSessionV1 {
    schema_version: u32,
    translation_mm: [i32; 3],
    yaw_milliradians: i32,
    hotbar_slot: u16,
    inventory: Vec<Option<DurableItemStackV1>>,
    containers: Vec<DurableContainerV1>,
    scheduled: Vec<DurableScheduledV1>,
    #[serde(default)]
    drops: Vec<DurableDroppedItemV1>,
    #[serde(default)]
    next_drop: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DurableItemStackV1 {
    item: String,
    quantity: u32,
    remaining_durability: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DurableContainerV1 {
    entity: [u8; 16],
    dimension: String,
    chunk: [i32; 3],
    workstation: Option<String>,
    revision: u64,
    slots: Vec<Option<DurableItemStackV1>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DurableScheduledV1 {
    id: [u8; 16],
    process: String,
    container: [u8; 16],
    dimension: String,
    chunk: [i32; 3],
    output_slot: u16,
    pending_output: DurableItemStackV1,
    due_tick: u64,
    revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DurableDroppedItemV1 {
    entity: [u8; 16],
    dimension: String,
    position: [i32; 3],
    stack: DurableItemStackV1,
}

impl DurablePlayerSessionV1 {
    pub(super) fn capture(
        pose: ProductionPlayerPose,
        hotbar_slot: u16,
        inventory: &[Option<ItemStackV1>],
        containers: &[(ContainerId, &ContainerStateV1)],
        scheduled: &[(ContinuationId, &FurnaceContinuationV1)],
        drops: &[(DropEntityId, &DroppedItemV1)],
        next_drop: u64,
    ) -> Self {
        let mut containers = containers
            .iter()
            .map(|(id, container)| DurableContainerV1::from_state(*id, container))
            .collect::<Vec<_>>();
        containers.sort_by_key(|left| left.entity);
        let mut scheduled = scheduled
            .iter()
            .map(|(id, continuation)| DurableScheduledV1::from_state(*id, continuation))
            .collect::<Vec<_>>();
        scheduled.sort_by_key(|left| left.id);
        let mut drops = drops
            .iter()
            .map(|(id, drop)| DurableDroppedItemV1::from_state(*id, drop))
            .collect::<Vec<_>>();
        drops.sort_by_key(|left| left.entity);
        Self {
            schema_version: PLAYER_SESSION_SCHEMA_VERSION,
            translation_mm: [
                meters_to_mm(pose.translation.x),
                meters_to_mm(pose.translation.y),
                meters_to_mm(pose.translation.z),
            ],
            yaw_milliradians: meters_to_mm(pose.yaw_radians),
            hotbar_slot,
            inventory: inventory
                .iter()
                .map(|slot| encode_stack_ref(slot.as_ref()))
                .collect(),
            containers,
            scheduled,
            drops,
            next_drop,
        }
    }

    pub(super) fn encode_payload(&self) -> Result<VersionedPayload, ProductionHostError> {
        let schema: SchemaId = PLAYER_SESSION_SCHEMA.parse()?;
        let version = PayloadSchemaVersion::new(PLAYER_SESSION_SCHEMA_VERSION)
            .map_err(ProductionHostError::from)?;
        let bytes =
            canonical_json_bytes(self).map_err(|_| ProductionHostError::InvalidPlayerSession)?;
        Ok(VersionedPayload::new(schema, version, bytes))
    }

    pub(super) fn decode_payload(payload: &VersionedPayload) -> Result<Self, ProductionHostError> {
        if payload.schema().as_str() != PLAYER_SESSION_SCHEMA
            || payload.schema_version().get() != PLAYER_SESSION_SCHEMA_VERSION
        {
            return Err(ProductionHostError::InvalidPlayerSession);
        }
        let session = serde_json::from_slice::<Self>(payload.bytes())
            .map_err(|_| ProductionHostError::InvalidPlayerSession)?;
        if session.schema_version != PLAYER_SESSION_SCHEMA_VERSION {
            return Err(ProductionHostError::InvalidPlayerSession);
        }
        Ok(session)
    }

    pub(super) fn pose(&self) -> ProductionPlayerPose {
        ProductionPlayerPose {
            translation: bevy::prelude::Vec3::new(
                mm_to_meters(self.translation_mm[0]),
                mm_to_meters(self.translation_mm[1]),
                mm_to_meters(self.translation_mm[2]),
            ),
            yaw_radians: mm_to_meters(self.yaw_milliradians),
            grounded: true,
        }
    }

    pub(super) const fn hotbar_slot(&self) -> u16 {
        self.hotbar_slot
    }

    pub(super) fn inventory_stacks(&self) -> Result<Vec<Option<ItemStackV1>>, ProductionHostError> {
        self.inventory
            .iter()
            .map(|slot| decode_stack_ref(slot.as_ref()))
            .collect()
    }

    pub(super) fn container_states(
        &self,
    ) -> Result<Vec<(ContainerId, ContainerStateV1)>, ProductionHostError> {
        self.containers
            .iter()
            .map(DurableContainerV1::to_state)
            .collect()
    }

    pub(super) fn scheduled_states(
        &self,
    ) -> Result<Vec<(ContinuationId, FurnaceContinuationV1)>, ProductionHostError> {
        self.scheduled
            .iter()
            .map(DurableScheduledV1::to_state)
            .collect()
    }

    pub(super) fn dropped_states(
        &self,
    ) -> Result<Vec<(DropEntityId, DroppedItemV1)>, ProductionHostError> {
        self.drops
            .iter()
            .map(DurableDroppedItemV1::to_state)
            .collect()
    }

    pub(super) const fn next_drop(&self) -> u64 {
        self.next_drop
    }
}

impl DurableContainerV1 {
    fn from_state(id: ContainerId, container: &ContainerStateV1) -> Self {
        Self {
            entity: id.as_bytes(),
            dimension: container.owner().dimension.as_str().to_owned(),
            chunk: [
                container.owner().chunk.x,
                container.owner().chunk.y,
                container.owner().chunk.z,
            ],
            workstation: container.workstation().map(|id| id.as_str().to_owned()),
            revision: container.revision(),
            slots: container
                .slots()
                .iter()
                .map(|slot| encode_stack_ref(slot.as_ref()))
                .collect(),
        }
    }

    fn to_state(&self) -> Result<(ContainerId, ContainerStateV1), ProductionHostError> {
        let dimension =
            DimensionId::new(self.dimension.parse()?).map_err(ProductionHostError::from)?;
        let workstation = self
            .workstation
            .as_deref()
            .map(WorkstationId::parse)
            .transpose()?;
        let mut container = ContainerStateV1::empty(
            ContainerOwnerComponentV1 {
                dimension,
                chunk: ChunkCoordinate::new(self.chunk[0], self.chunk[1], self.chunk[2]),
                entity: PersistentEntityId::from_bytes(self.entity),
            },
            workstation,
            self.slots.len(),
        )?;
        for (index, slot) in self.slots.iter().enumerate() {
            let slot_index =
                u16::try_from(index).map_err(|_| ProductionHostError::InvalidPlayerSession)?;
            container.seed_slot(SlotIndex::new(slot_index), decode_stack_ref(slot.as_ref())?)?;
        }
        Ok((ContainerId::from_bytes(self.entity), container))
    }
}

impl DurableDroppedItemV1 {
    fn from_state(id: DropEntityId, drop: &DroppedItemV1) -> Self {
        Self {
            entity: id.as_bytes(),
            dimension: drop.location.dimension.as_str().to_owned(),
            position: [
                drop.location.position.x,
                drop.location.position.y,
                drop.location.position.z,
            ],
            stack: encode_stack(&drop.stack),
        }
    }

    fn to_state(&self) -> Result<(DropEntityId, DroppedItemV1), ProductionHostError> {
        let dimension =
            DimensionId::new(self.dimension.parse()?).map_err(ProductionHostError::from)?;
        Ok((
            DropEntityId::from_bytes(self.entity),
            DroppedItemV1 {
                location: BlockKey::new(
                    dimension,
                    BlockPosition {
                        x: self.position[0],
                        y: self.position[1],
                        z: self.position[2],
                    },
                ),
                stack: decode_stack(&self.stack)?,
            },
        ))
    }
}

impl DurableScheduledV1 {
    fn from_state(id: ContinuationId, continuation: &FurnaceContinuationV1) -> Self {
        Self {
            id: *id.as_bytes(),
            process: continuation.process.as_str().to_owned(),
            container: continuation.container.as_bytes(),
            dimension: continuation.target.chunk.dimension.as_str().to_owned(),
            chunk: [
                continuation.target.chunk.coordinate.x,
                continuation.target.chunk.coordinate.y,
                continuation.target.chunk.coordinate.z,
            ],
            output_slot: continuation.output_slot.get(),
            pending_output: encode_stack(&continuation.pending_output),
            due_tick: continuation.due_tick.get(),
            revision: continuation.revision,
        }
    }

    fn to_state(&self) -> Result<(ContinuationId, FurnaceContinuationV1), ProductionHostError> {
        let dimension =
            DimensionId::new(self.dimension.parse()?).map_err(ProductionHostError::from)?;
        Ok((
            ContinuationId::from_bytes(self.id),
            FurnaceContinuationV1 {
                process: ProcessId::parse(&self.process)?,
                container: ContainerId::from_bytes(self.container),
                target: GameplayEditTarget::new(
                    DimensionChunkKey::new(
                        dimension,
                        ChunkCoordinate::new(self.chunk[0], self.chunk[1], self.chunk[2]),
                    ),
                    GameplayStorageDomain::Continuations,
                ),
                output_slot: SlotIndex::new(self.output_slot),
                pending_output: decode_stack(&self.pending_output)?,
                due_tick: AuthorityTick::new(self.due_tick),
                revision: self.revision,
            },
        ))
    }
}

fn encode_stack_ref(stack: Option<&ItemStackV1>) -> Option<DurableItemStackV1> {
    stack.map(encode_stack)
}

fn encode_stack(stack: &ItemStackV1) -> DurableItemStackV1 {
    DurableItemStackV1 {
        item: stack.item().as_str().to_owned(),
        quantity: stack.quantity(),
        remaining_durability: match stack.state() {
            ItemStateV1::Plain => None,
            ItemStateV1::ToolDurability { remaining } => Some(remaining.get()),
        },
    }
}

fn decode_stack_ref(
    stack: Option<&DurableItemStackV1>,
) -> Result<Option<ItemStackV1>, ProductionHostError> {
    stack.map(decode_stack).transpose()
}

fn decode_stack(stack: &DurableItemStackV1) -> Result<ItemStackV1, ProductionHostError> {
    let item = ItemId::parse(&stack.item)?;
    match stack.remaining_durability {
        Some(remaining) => Ok(ItemStackV1::tool(item, remaining)?),
        None => Ok(ItemStackV1::plain(item, stack.quantity)?),
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "session pose is stored as bounded millimetres, not IEEE bits"
)]
fn meters_to_mm(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let millimetres = value * 1000.0;
    if millimetres >= f32::from(i16::MAX) * 1000.0 {
        i32::from(i16::MAX) * 1000
    } else if millimetres <= f32::from(i16::MIN) * 1000.0 {
        i32::from(i16::MIN) * 1000
    } else {
        millimetres.round() as i32
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "restored pose is a presentation seed, not an authoritative voxel"
)]
fn mm_to_meters(value: i32) -> f32 {
    value as f32 / 1000.0
}
