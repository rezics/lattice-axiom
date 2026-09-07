use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    num::{NonZeroU16, NonZeroU32},
};

use thiserror::Error;

use crate::{
    BlockId, BlockKey, ChunkCoordinate, ChunkRevision, CommandFingerprintV1, ContainerId,
    ContinuationId, DimensionChunkKey, DimensionId, DropEntityId, GameplayEditTarget,
    GameplayPlanHashV1, GameplayStorageDomain, ItemId, PersistentEntityId, PlayerId, ProcessId,
    RecipeId, ReferenceGameplayStateHashV1, ToolClassId, TransactionId, WorkstationId,
    WorldRevision,
};

/// Absolute implementation ceiling for player inventory slots.
pub const ABSOLUTE_MAX_INVENTORY_SLOTS: usize = 256;
/// Default hotbar prefix length for a player inventory.
pub const DEFAULT_HOTBAR_SLOTS: u16 = 9;
/// Absolute implementation ceiling for container slots.
pub const ABSOLUTE_MAX_CONTAINER_SLOTS: usize = 256;
/// Absolute implementation ceiling for mutation intents in one command.
pub const ABSOLUTE_MAX_MUTATIONS_PER_COMMAND: usize = 256;
/// Absolute implementation ceiling for scheduled completions in one command.
pub const ABSOLUTE_MAX_SCHEDULED_COMPLETIONS: usize = 64;
/// Maximum canonical 60 Hz mining steps accepted by one batched command.
pub const MAX_MINING_STEPS_PER_COMMAND: u16 = 60;

macro_rules! scalar_id {
    ($(#[$meta:meta])* $name:ident, $inner:ty) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name($inner);

        impl $name {
            /// Creates the typed value.
            #[must_use]
            pub const fn new(value: $inner) -> Self {
                Self(value)
            }

            /// Returns the primitive value.
            #[must_use]
            pub const fn get(self) -> $inner {
                self.0
            }
        }
    };
}

scalar_id!(
    /// Authoritative fixed tick supplied by the Bevy host.
    AuthorityTick,
    u64
);

/// Non-zero number of canonical 60 Hz mining steps batched into one command.
///
/// A low simulation rate may need to catch up more than one mining step at a
/// fixed boundary. Keeping the batch bounded makes that cost explicit while
/// preserving mining duration when the simulation rate changes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MiningStepCountV1(NonZeroU16);

impl MiningStepCountV1 {
    /// One canonical mining step.
    pub const ONE: Self = Self(NonZeroU16::MIN);

    /// Creates a bounded mining-step batch.
    #[must_use]
    pub const fn new(value: u16) -> Option<Self> {
        if value == 0 || value > MAX_MINING_STEPS_PER_COMMAND {
            return None;
        }
        match NonZeroU16::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the number of canonical mining steps.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// Slot index in an inventory or container.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SlotIndex(u16);

impl SlotIndex {
    /// Creates a slot index.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the zero-based index.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    pub(crate) const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Integer voxel position in right-handed Y-up world coordinates.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockPosition {
    /// Horizontal +X coordinate.
    pub x: i32,
    /// Vertical +Y coordinate.
    pub y: i32,
    /// Horizontal +Z coordinate; conventional forward is -Z.
    pub z: i32,
}

impl BlockPosition {
    /// Returns the authoritative 32-cubed chunk coordinate.
    #[must_use]
    pub fn chunk(self) -> ChunkCoordinate {
        self.chunk_in(32)
    }

    /// Returns the chunk coordinate for a host-selected cubic edge.
    #[must_use]
    pub fn chunk_in(self, edge: u16) -> ChunkCoordinate {
        let edge = i32::from(edge.max(1));
        ChunkCoordinate::new(
            self.x.div_euclid(edge),
            self.y.div_euclid(edge),
            self.z.div_euclid(edge),
        )
    }
}

/// Versioned state carried by one item stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemStateV1 {
    /// Item instances have no per-stack authoritative state.
    Plain,
    /// A singleton tool carries remaining durability.
    ToolDurability {
        /// Remaining successful uses.
        remaining: NonZeroU32,
    },
}

/// A version-one concrete item stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemStackV1 {
    pub(crate) item: ItemId,
    pub(crate) quantity: NonZeroU32,
    pub(crate) state: ItemStateV1,
}

impl ItemStackV1 {
    /// Reserved schema identity; this crate registers no world-wire codec.
    pub const SCHEMA_ID: &'static str = "latticeaxiom:schema/item-stack@1";
    /// Version marker included in the canonical reference-state projection.
    pub const SCHEMA_MAJOR: u16 = 1;

    /// Creates an ordinary stack.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject::ZeroQuantity`] when `quantity` is zero.
    pub fn plain(item: ItemId, quantity: u32) -> Result<Self, GameplayReject> {
        let quantity = NonZeroU32::new(quantity).ok_or(GameplayReject::ZeroQuantity)?;
        Ok(Self {
            item,
            quantity,
            state: ItemStateV1::Plain,
        })
    }

    /// Creates a singleton tool stack.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection when durability is zero.
    pub fn tool(item: ItemId, durability: u32) -> Result<Self, GameplayReject> {
        let remaining = NonZeroU32::new(durability).ok_or(GameplayReject::ToolBroken)?;
        Ok(Self {
            item,
            quantity: NonZeroU32::MIN,
            state: ItemStateV1::ToolDurability { remaining },
        })
    }

    /// Returns the concrete item identity.
    #[must_use]
    pub fn item(&self) -> &ItemId {
        &self.item
    }

    /// Returns the non-zero quantity.
    #[must_use]
    pub const fn quantity(&self) -> u32 {
        self.quantity.get()
    }

    /// Returns the versioned stack state.
    #[must_use]
    pub const fn state(&self) -> &ItemStateV1 {
        &self.state
    }

    /// Returns a copy with `quantity`, or `None` when the quantity is zero.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection when a stateful stack would have quantity
    /// other than one.
    pub fn with_quantity(&self, quantity: u32) -> Result<Option<Self>, GameplayReject> {
        if quantity == 0 {
            return Ok(None);
        }
        if !matches!(self.state, ItemStateV1::Plain) && quantity != 1 {
            return Err(GameplayReject::StatefulStackQuantity { quantity });
        }
        Ok(Some(Self {
            item: self.item.clone(),
            quantity: NonZeroU32::new(quantity).ok_or(GameplayReject::ZeroQuantity)?,
            state: self.state.clone(),
        }))
    }

    pub(crate) fn tool_after_use(&self) -> Result<Option<Self>, GameplayReject> {
        let ItemStateV1::ToolDurability { remaining } = self.state else {
            return Err(GameplayReject::ToolStateMissing {
                item: self.item.clone(),
            });
        };
        let next = remaining.get() - 1;
        if next == 0 {
            return Ok(None);
        }
        Self::tool(self.item.clone(), next).map(Some)
    }
}

/// Version-one fixed-slot inventory state.
///
/// The hotbar is the selected prefix of this container, not a second client
/// array. Selected-slot changes are authoritative inventory mutations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryStateV1 {
    pub(crate) target: GameplayEditTarget,
    pub(crate) revision: u64,
    pub(crate) hotbar_slots: u16,
    pub(crate) selected_hotbar: u16,
    pub(crate) slots: Box<[Option<ItemStackV1>]>,
}

impl InventoryStateV1 {
    /// Reserved schema identity; this crate registers no world-wire codec.
    pub const SCHEMA_ID: &'static str = "latticeaxiom:schema/inventory@1";

    /// Creates an empty bounded inventory.
    ///
    /// # Errors
    ///
    /// Rejects zero slots and slot counts above the absolute implementation cap.
    pub fn empty(target: GameplayEditTarget, slot_count: usize) -> Result<Self, GameplayReject> {
        let hotbar_slots = u16::try_from(slot_count.min(usize::from(DEFAULT_HOTBAR_SLOTS)))
            .map_err(|_| GameplayReject::LimitExceeded {
                resource: "hotbar_slots",
                limit: usize::from(u16::MAX),
                actual: slot_count,
            })?;
        Self::empty_with_hotbar(target, slot_count, hotbar_slots)
    }

    /// Creates an empty bounded inventory with an explicit hotbar prefix.
    ///
    /// # Errors
    ///
    /// Rejects zero slots, slot counts above the absolute implementation cap,
    /// a hotbar prefix of zero or larger than the inventory, or a selected
    /// hotbar index outside that prefix.
    pub fn empty_with_hotbar(
        target: GameplayEditTarget,
        slot_count: usize,
        hotbar_slots: u16,
    ) -> Result<Self, GameplayReject> {
        if slot_count == 0 || slot_count > ABSOLUTE_MAX_INVENTORY_SLOTS {
            return Err(GameplayReject::LimitExceeded {
                resource: "inventory_slots",
                limit: ABSOLUTE_MAX_INVENTORY_SLOTS,
                actual: slot_count,
            });
        }
        if target.domain != GameplayStorageDomain::PersistentEntities {
            return Err(GameplayReject::InvalidStorageTarget {
                expected: GameplayStorageDomain::PersistentEntities,
                actual: target.domain,
            });
        }
        validate_hotbar(slot_count, hotbar_slots, 0)?;
        Ok(Self {
            target,
            revision: 0,
            hotbar_slots,
            selected_hotbar: 0,
            slots: vec![None; slot_count].into_boxed_slice(),
        })
    }

    /// Returns the explicit persistent-entity storage target.
    #[must_use]
    pub const fn target(&self) -> &GameplayEditTarget {
        &self.target
    }

    /// Returns the inventory revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the hotbar prefix length.
    #[must_use]
    pub const fn hotbar_slots(&self) -> u16 {
        self.hotbar_slots
    }

    /// Returns the selected hotbar index in `0..hotbar_slots`.
    #[must_use]
    pub const fn selected_hotbar(&self) -> SlotIndex {
        SlotIndex::new(self.selected_hotbar)
    }

    /// Returns the selected hotbar stack, if any.
    #[must_use]
    pub fn selected_stack(&self) -> Option<&ItemStackV1> {
        self.slots
            .get(usize::from(self.selected_hotbar))
            .and_then(Option::as_ref)
    }

    /// Returns the hotbar prefix in index order.
    #[must_use]
    pub fn hotbar(&self) -> &[Option<ItemStackV1>] {
        let end = usize::from(self.hotbar_slots).min(self.slots.len());
        &self.slots[..end]
    }

    /// Returns the slot count.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.slots.len()
    }

    /// Returns whether the inventory has no slots.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Returns one slot.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject::SlotOutOfRange`] for an invalid index.
    pub fn slot(&self, slot: SlotIndex) -> Result<Option<&ItemStackV1>, GameplayReject> {
        self.slots
            .get(slot.as_usize())
            .map(Option::as_ref)
            .ok_or(GameplayReject::SlotOutOfRange {
                slot,
                slots: self.slots.len(),
            })
    }

    /// Returns every inventory slot in index order.
    #[must_use]
    pub const fn slots(&self) -> &[Option<ItemStackV1>] {
        &self.slots
    }

    /// Seeds a slot while constructing a fixture or decoded snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for an invalid index.
    pub fn seed_slot(
        &mut self,
        slot: SlotIndex,
        stack: Option<ItemStackV1>,
    ) -> Result<(), GameplayReject> {
        let slots = self.slots.len();
        let target = self
            .slots
            .get_mut(slot.as_usize())
            .ok_or(GameplayReject::SlotOutOfRange { slot, slots })?;
        *target = stack;
        Ok(())
    }

    /// Seeds the selected hotbar index while constructing a fixture or decoded snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject::SlotOutOfRange`] when `slot` is outside the hotbar prefix.
    pub fn seed_selected_hotbar(&mut self, slot: SlotIndex) -> Result<(), GameplayReject> {
        validate_hotbar(self.slots.len(), self.hotbar_slots, slot.get())?;
        self.selected_hotbar = slot.get();
        Ok(())
    }

    /// Validates slot count and hotbar prefix invariants.
    ///
    /// # Errors
    ///
    /// Rejects an empty inventory, a slot count above the absolute cap, or an
    /// invalid hotbar prefix or selected index.
    pub(crate) fn validate_structure(&self) -> Result<(), GameplayReject> {
        if self.slots.is_empty() || self.slots.len() > ABSOLUTE_MAX_INVENTORY_SLOTS {
            return Err(GameplayReject::LimitExceeded {
                resource: "inventory_slots",
                limit: ABSOLUTE_MAX_INVENTORY_SLOTS,
                actual: self.slots.len(),
            });
        }
        validate_hotbar(self.slots.len(), self.hotbar_slots, self.selected_hotbar)
    }
}

fn validate_hotbar(
    slot_count: usize,
    hotbar_slots: u16,
    selected_hotbar: u16,
) -> Result<(), GameplayReject> {
    let hotbar = usize::from(hotbar_slots);
    if hotbar == 0 || hotbar > slot_count {
        return Err(GameplayReject::LimitExceeded {
            resource: "hotbar_slots",
            limit: slot_count.max(1),
            actual: hotbar,
        });
    }
    if usize::from(selected_hotbar) >= hotbar {
        return Err(GameplayReject::SlotOutOfRange {
            slot: SlotIndex::new(selected_hotbar),
            slots: hotbar,
        });
    }
    Ok(())
}

/// Owner component persisted alongside a container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerOwnerComponentV1 {
    /// Owning dimension, without any domain-specific default.
    pub dimension: DimensionId,
    /// Materialized chunk containing the persistent entity.
    pub chunk: ChunkCoordinate,
    /// Canonical storage persistent-entity key; never a Bevy `Entity`.
    pub entity: PersistentEntityId,
}

impl ContainerOwnerComponentV1 {
    /// Returns the explicit chunk target used when capturing a storage transaction.
    #[must_use]
    pub fn chunk_key(&self) -> DimensionChunkKey {
        DimensionChunkKey::new(self.dimension.clone(), self.chunk)
    }
}

impl ContainerOwnerComponentV1 {
    /// Reserved schema identity; this crate registers no world-wire codec.
    pub const SCHEMA_ID: &'static str = "latticeaxiom:schema/container-owner@1";
}

/// Version-one persistent container state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerStateV1 {
    pub(crate) owner: ContainerOwnerComponentV1,
    pub(crate) workstation: Option<WorkstationId>,
    pub(crate) revision: u64,
    pub(crate) slots: Box<[Option<ItemStackV1>]>,
}

impl ContainerStateV1 {
    /// Reserved schema identity; this crate registers no world-wire codec.
    pub const SCHEMA_ID: &'static str = "latticeaxiom:schema/container@1";

    /// Creates an empty persistent container.
    ///
    /// # Errors
    ///
    /// Rejects zero slots and counts above the absolute container cap.
    pub fn empty(
        owner: ContainerOwnerComponentV1,
        workstation: Option<WorkstationId>,
        slot_count: usize,
    ) -> Result<Self, GameplayReject> {
        if slot_count == 0 || slot_count > ABSOLUTE_MAX_CONTAINER_SLOTS {
            return Err(GameplayReject::LimitExceeded {
                resource: "container_slots",
                limit: ABSOLUTE_MAX_CONTAINER_SLOTS,
                actual: slot_count,
            });
        }
        Ok(Self {
            owner,
            workstation,
            revision: 0,
            slots: vec![None; slot_count].into_boxed_slice(),
        })
    }

    /// Returns the stable owner component.
    #[must_use]
    pub const fn owner(&self) -> &ContainerOwnerComponentV1 {
        &self.owner
    }

    /// Returns the workstation contract, if any.
    #[must_use]
    pub const fn workstation(&self) -> Option<&WorkstationId> {
        self.workstation.as_ref()
    }

    /// Returns the component revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the number of slots.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.slots.len()
    }

    /// Returns every container slot in index order.
    #[must_use]
    pub const fn slots(&self) -> &[Option<ItemStackV1>] {
        &self.slots
    }

    /// Returns whether this container has no slots.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Returns one slot.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for an invalid index.
    pub fn slot(&self, slot: SlotIndex) -> Result<Option<&ItemStackV1>, GameplayReject> {
        self.slots
            .get(slot.as_usize())
            .map(Option::as_ref)
            .ok_or(GameplayReject::SlotOutOfRange {
                slot,
                slots: self.slots.len(),
            })
    }

    /// Seeds a slot while constructing a fixture or decoded snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for an invalid index.
    pub fn seed_slot(
        &mut self,
        slot: SlotIndex,
        stack: Option<ItemStackV1>,
    ) -> Result<(), GameplayReject> {
        let slots = self.slots.len();
        let target = self
            .slots
            .get_mut(slot.as_usize())
            .ok_or(GameplayReject::SlotOutOfRange { slot, slots })?;
        *target = stack;
        Ok(())
    }
}

/// Persistent dropped-item entity state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DroppedItemV1 {
    /// Dimension-qualified voxel anchor.
    pub location: BlockKey,
    /// Concrete item stack.
    pub stack: ItemStackV1,
}

/// In-progress mining state for one player and block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BreakProgressV1 {
    /// Concrete target observed when progress began.
    pub block: BlockId,
    /// Accumulated deterministic work units.
    pub accumulated_work: u32,
}

/// Key for one player's progress against one dimension-qualified block.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BreakProgressKey {
    /// Player performing the operation.
    pub player: PlayerId,
    /// Complete target block identity.
    pub block: BlockKey,
}

/// Version-one scheduled furnace continuation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FurnaceContinuationV1 {
    /// Process contract used to reserve the output.
    pub process: ProcessId,
    /// Persistent machine container.
    pub container: ContainerId,
    /// Explicit continuation-domain storage capture target.
    pub target: GameplayEditTarget,
    /// Concrete output slot.
    pub output_slot: SlotIndex,
    /// Concrete output frozen when input and fuel were consumed.
    pub pending_output: ItemStackV1,
    /// Authoritative tick at which completion becomes eligible.
    pub due_tick: AuthorityTick,
    /// Monotonic continuation revision.
    pub revision: u64,
}

impl FurnaceContinuationV1 {
    /// Reserved schema identity; this crate registers no world-wire codec.
    pub const SCHEMA_ID: &'static str = "latticeaxiom:schema/furnace-continuation@1";
}

/// Runtime-independent safety ceilings for the reference aggregate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameplayLimits {
    /// Maximum player inventories.
    pub players: usize,
    /// Maximum persistent containers.
    pub containers: usize,
    /// Maximum tracked block overrides in the conformance aggregate.
    pub tracked_blocks: usize,
    /// Maximum dropped item entities.
    pub drops: usize,
    /// Maximum simultaneous break-progress entries.
    pub break_progress: usize,
    /// Maximum scheduled continuations.
    pub continuations: usize,
    /// Maximum mutation intents per command.
    pub mutations_per_command: usize,
    /// Maximum scheduled completions per command.
    pub scheduled_completions: usize,
    /// Number of exact command receipts retained for idempotent retries.
    pub recent_receipts: usize,
}

impl Default for GameplayLimits {
    fn default() -> Self {
        Self {
            players: 64,
            containers: 1_024,
            tracked_blocks: 65_536,
            drops: 4_096,
            break_progress: 1_024,
            continuations: 1_024,
            mutations_per_command: 128,
            scheduled_completions: 32,
            recent_receipts: 256,
        }
    }
}

impl GameplayLimits {
    pub(crate) fn validate(self) -> Result<Self, GameplayReject> {
        let non_zero = [
            ("players", self.players),
            ("containers", self.containers),
            ("tracked_blocks", self.tracked_blocks),
            ("drops", self.drops),
            ("break_progress", self.break_progress),
            ("continuations", self.continuations),
            ("mutations_per_command", self.mutations_per_command),
            ("scheduled_completions", self.scheduled_completions),
            ("recent_receipts", self.recent_receipts),
        ];
        for (resource, value) in non_zero {
            if value == 0 {
                return Err(GameplayReject::LimitExceeded {
                    resource,
                    limit: 1,
                    actual: 0,
                });
            }
        }
        if self.mutations_per_command > ABSOLUTE_MAX_MUTATIONS_PER_COMMAND {
            return Err(GameplayReject::LimitExceeded {
                resource: "mutations_per_command",
                limit: ABSOLUTE_MAX_MUTATIONS_PER_COMMAND,
                actual: self.mutations_per_command,
            });
        }
        if self.scheduled_completions > ABSOLUTE_MAX_SCHEDULED_COMPLETIONS {
            return Err(GameplayReject::LimitExceeded {
                resource: "scheduled_completions",
                limit: ABSOLUTE_MAX_SCHEDULED_COMPLETIONS,
                actual: self.scheduled_completions,
            });
        }
        Ok(self)
    }
}

/// Bounded reference gameplay aggregate used by conformance tests and adapters.
///
/// The type is a bounded state model, not a durable backend. Production world
/// storage remains responsible for atomic persistence, checkpoints, wire
/// encoding, durability, and world revision ownership.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceGameplayState {
    pub(crate) observed_world_revision: WorldRevision,
    pub(crate) inventories: BTreeMap<PlayerId, InventoryStateV1>,
    pub(crate) blocks: BTreeMap<BlockKey, BlockId>,
    pub(crate) loaded_chunks: BTreeMap<DimensionChunkKey, ChunkRevision>,
    pub(crate) drops: BTreeMap<DropEntityId, DroppedItemV1>,
    pub(crate) containers: BTreeMap<ContainerId, ContainerStateV1>,
    pub(crate) break_progress: BTreeMap<BreakProgressKey, BreakProgressV1>,
    pub(crate) continuations: BTreeMap<ContinuationId, FurnaceContinuationV1>,
    pub(crate) recent_receipts: BTreeMap<TransactionId, RuntimePlanReceiptV1>,
    pub(crate) receipt_order: VecDeque<TransactionId>,
    pub(crate) pending_receipt: Option<RuntimePlanReceiptV1>,
    pub(crate) oldest_replayable_world_revision: WorldRevision,
    pub(crate) limits: GameplayLimits,
    pub(crate) chunk_edge: u16,
}

impl ReferenceGameplayState {
    /// Creates an empty bounded reference aggregate.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection when a configured limit is zero or exceeds an
    /// absolute implementation ceiling.
    pub fn new(limits: GameplayLimits) -> Result<Self, GameplayReject> {
        Ok(Self {
            observed_world_revision: WorldRevision::ZERO,
            inventories: BTreeMap::new(),
            blocks: BTreeMap::new(),
            loaded_chunks: BTreeMap::new(),
            drops: BTreeMap::new(),
            containers: BTreeMap::new(),
            break_progress: BTreeMap::new(),
            continuations: BTreeMap::new(),
            recent_receipts: BTreeMap::new(),
            receipt_order: VecDeque::new(),
            pending_receipt: None,
            oldest_replayable_world_revision: WorldRevision::ZERO,
            limits: limits.validate()?,
            chunk_edge: 32,
        })
    }

    /// Sets the cubic chunk edge used to map blocks onto storage chunks.
    ///
    /// # Errors
    ///
    /// Rejects a zero edge.
    pub fn set_chunk_edge(&mut self, edge: u16) -> Result<(), GameplayReject> {
        if edge == 0 {
            return Err(GameplayReject::LimitExceeded {
                resource: "chunk_edge",
                limit: 1,
                actual: 0,
            });
        }
        self.chunk_edge = edge;
        Ok(())
    }

    /// Returns the cubic chunk edge used by this reference state.
    #[must_use]
    pub const fn chunk_edge(&self) -> u16 {
        self.chunk_edge
    }

    pub(crate) fn block_chunk(&self, key: &BlockKey) -> DimensionChunkKey {
        key.chunk_in(self.chunk_edge)
    }

    /// Returns the storage-owned world revision observed when this state was loaded.
    #[must_use]
    pub const fn observed_world_revision(&self) -> WorldRevision {
        self.observed_world_revision
    }

    /// Rebinds the reference oracle to a storage-owned world revision after the
    /// host has committed a complete [`latticeaxiom_storage::WorldTransaction`].
    ///
    /// # Errors
    ///
    /// Rejects a revision older than the currently observed storage revision.
    pub fn observe_world_revision(
        &mut self,
        revision: WorldRevision,
    ) -> Result<(), GameplayReject> {
        if revision < self.observed_world_revision {
            return Err(GameplayReject::StaleWorldRevision {
                expected: revision.get(),
                actual: self.observed_world_revision.get(),
            });
        }
        self.observed_world_revision = revision;
        Ok(())
    }

    /// Returns a player inventory.
    #[must_use]
    pub fn inventory(&self, player: PlayerId) -> Option<&InventoryStateV1> {
        self.inventories.get(&player)
    }

    /// Returns a persistent container.
    #[must_use]
    pub fn container(&self, container: ContainerId) -> Option<&ContainerStateV1> {
        self.containers.get(&container)
    }

    /// Returns a loaded block override; absence is an empty cell only when its
    /// dimension-qualified chunk is present in [`Self::loaded_chunk_revision`].
    #[must_use]
    pub fn block(&self, key: &BlockKey) -> Option<&BlockId> {
        self.blocks.get(key)
    }

    /// Returns the storage-owned revision observed for a loaded chunk.
    #[must_use]
    pub fn loaded_chunk_revision(&self, chunk: &DimensionChunkKey) -> Option<ChunkRevision> {
        self.loaded_chunks.get(chunk).copied()
    }

    /// Marks one complete chunk snapshot as loaded at its storage-owned revision.
    ///
    /// # Errors
    ///
    /// Rejects duplicate/conflicting chunk observations. A refresh must rebuild
    /// the complete reference snapshot instead of relabeling one live chunk.
    pub fn seed_loaded_chunk(
        &mut self,
        chunk: DimensionChunkKey,
        revision: ChunkRevision,
    ) -> Result<(), GameplayReject> {
        if self.loaded_chunks.contains_key(&chunk) {
            return Err(GameplayReject::DuplicateStateKey {
                kind: "loaded_chunk",
            });
        }
        self.loaded_chunks.insert(chunk, revision);
        Ok(())
    }

    /// Returns all loaded dimension-qualified chunks and storage-owned revisions.
    #[must_use]
    pub const fn loaded_chunks(&self) -> &BTreeMap<DimensionChunkKey, ChunkRevision> {
        &self.loaded_chunks
    }

    /// Returns one dropped item.
    #[must_use]
    pub fn dropped_item(&self, id: DropEntityId) -> Option<&DroppedItemV1> {
        self.drops.get(&id)
    }

    /// Returns every dropped item in identity order.
    #[must_use]
    pub const fn dropped_items(&self) -> &BTreeMap<DropEntityId, DroppedItemV1> {
        &self.drops
    }

    /// Returns in-progress mining entries in identity order.
    #[must_use]
    pub const fn break_progress(&self) -> &BTreeMap<BreakProgressKey, BreakProgressV1> {
        &self.break_progress
    }

    /// Returns the runtime receipt waiting for a storage capture, if any.
    #[must_use]
    pub const fn pending_receipt(&self) -> Option<&RuntimePlanReceiptV1> {
        self.pending_receipt.as_ref()
    }

    /// Seeds or confirms a loaded chunk at its storage-owned revision.
    ///
    /// # Errors
    ///
    /// Rejects a conflicting revision for an already-loaded chunk.
    pub fn ensure_loaded_chunk(
        &mut self,
        chunk: DimensionChunkKey,
        revision: ChunkRevision,
    ) -> Result<(), GameplayReject> {
        match self.loaded_chunks.get(&chunk) {
            Some(existing) if *existing == revision => Ok(()),
            Some(existing) => Err(GameplayReject::StaleChunkRevision {
                expected: existing.get(),
                actual: revision.get(),
            }),
            None => self.seed_loaded_chunk(chunk, revision),
        }
    }

    /// Releases loaded observations after the host durably saves and unloads
    /// a chunk. Live gameplay owners retain their chunks until their own
    /// entity unload protocol releases them.
    ///
    /// # Errors
    /// Refuses to invalidate an outstanding commit or live gameplay ownership.
    pub fn release_loaded_chunk(
        &mut self,
        chunk: &DimensionChunkKey,
    ) -> Result<bool, GameplayReject> {
        if let Some(pending) = &self.pending_receipt {
            return Err(GameplayReject::StorageCommitPending {
                transaction_id: *pending.transaction_id.as_bytes(),
                observed_world_revision: pending.observed_world_revision.get(),
            });
        }
        let edge = self.chunk_edge;
        if self
            .inventories
            .values()
            .any(|inventory| inventory.target.chunk == *chunk)
            || self
                .containers
                .values()
                .any(|container| container.owner.chunk_key() == *chunk)
            || self
                .drops
                .values()
                .any(|drop| drop.location.chunk_in(edge) == *chunk)
            || self
                .continuations
                .values()
                .any(|continuation| continuation.target.chunk == *chunk)
        {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "chunk_owns_live_gameplay_state",
            });
        }
        self.blocks.retain(|key, _| key.chunk_in(edge) != *chunk);
        self.break_progress
            .retain(|key, _| key.block.chunk_in(edge) != *chunk);
        Ok(self.loaded_chunks.remove(chunk).is_some())
    }

    /// Inserts or confirms one occupied block cell before planning a command.
    ///
    /// # Errors
    ///
    /// Rejects the tracked-block boundary plus one, an unloaded chunk, or a
    /// conflicting occupied identity.
    pub fn sync_occupied_block(
        &mut self,
        key: BlockKey,
        block: BlockId,
    ) -> Result<(), GameplayReject> {
        match self.blocks.get(&key) {
            Some(existing) if existing == &block => Ok(()),
            Some(_) => Err(GameplayReject::DuplicateStateKey { kind: "block" }),
            None => self.seed_block(key, block),
        }
    }

    /// Clears a previously tracked occupied cell when the voxel world is empty.
    pub fn clear_occupied_block(&mut self, key: &BlockKey) {
        self.blocks.remove(key);
    }

    /// Seeds one inventory slot on an already-loaded player.
    ///
    /// # Errors
    ///
    /// Rejects an unknown player or an invalid slot index.
    pub fn seed_player_slot(
        &mut self,
        player: PlayerId,
        slot: SlotIndex,
        stack: Option<ItemStackV1>,
    ) -> Result<(), GameplayReject> {
        let inventory = self
            .inventories
            .get_mut(&player)
            .ok_or(GameplayReject::UnknownPlayer {
                player: player.as_bytes(),
            })?;
        inventory.seed_slot(slot, stack)
    }

    /// Returns one continuation.
    #[must_use]
    pub fn continuation(&self, id: ContinuationId) -> Option<&FurnaceContinuationV1> {
        self.continuations.get(&id)
    }

    /// Returns persistent containers in stable identity order.
    #[must_use]
    pub const fn containers(&self) -> &BTreeMap<ContainerId, ContainerStateV1> {
        &self.containers
    }

    /// Returns scheduled continuations in stable identity order.
    #[must_use]
    pub const fn continuations(&self) -> &BTreeMap<ContinuationId, FurnaceContinuationV1> {
        &self.continuations
    }

    /// Returns the canonical gameplay state hash.
    #[must_use]
    pub fn canonical_hash(&self) -> ReferenceGameplayStateHashV1 {
        crate::hash::state_hash(self)
    }

    /// Validates every loaded reference value against the compiled catalog and
    /// its explicit storage capture target before command planning.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for an unloaded chunk, invalid target domain,
    /// unknown catalog value, invalid decoded stack/tool state, or orphaned
    /// continuation.
    #[allow(
        clippy::too_many_lines,
        reason = "one exhaustive load barrier keeps cross-collection invariants visibly auditable"
    )]
    pub fn validate_loaded(&self, catalog: &crate::GameplayCatalog) -> Result<(), GameplayReject> {
        let _ = self.limits.validate()?;
        for (resource, actual, limit) in [
            ("players", self.inventories.len(), self.limits.players),
            ("containers", self.containers.len(), self.limits.containers),
            (
                "tracked_blocks",
                self.blocks.len(),
                self.limits.tracked_blocks,
            ),
            ("drops", self.drops.len(), self.limits.drops),
            (
                "break_progress",
                self.break_progress.len(),
                self.limits.break_progress,
            ),
            (
                "continuations",
                self.continuations.len(),
                self.limits.continuations,
            ),
            (
                "recent_receipts",
                self.recent_receipts.len(),
                self.limits.recent_receipts,
            ),
        ] {
            if actual > limit {
                return Err(GameplayReject::LimitExceeded {
                    resource,
                    limit,
                    actual,
                });
            }
        }
        if self
            .loaded_chunks
            .values()
            .any(|revision| revision.get() > self.observed_world_revision.get())
        {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "loaded_chunk_revision",
            });
        }
        if self.receipt_order.len() != self.recent_receipts.len() {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "retry_ledger",
            });
        }
        let mut receipt_ids = BTreeSet::new();
        let mut previous_receipt_revision = None;
        for id in &self.receipt_order {
            let receipt =
                self.recent_receipts
                    .get(id)
                    .ok_or(GameplayReject::MutationPreconditionFailed {
                        resource: "retry_ledger",
                    })?;
            if !receipt_ids.insert(*id)
                || receipt.transaction_id != *id
                || receipt.observed_world_revision > self.observed_world_revision
                || previous_receipt_revision
                    .is_some_and(|previous| previous > receipt.observed_world_revision)
            {
                return Err(GameplayReject::MutationPreconditionFailed {
                    resource: "retry_ledger",
                });
            }
            previous_receipt_revision = Some(receipt.observed_world_revision);
        }
        match self.receipt_order.front() {
            Some(oldest_id) => {
                let oldest = self.recent_receipts.get(oldest_id).ok_or(
                    GameplayReject::MutationPreconditionFailed {
                        resource: "retry_ledger",
                    },
                )?;
                if self.oldest_replayable_world_revision != oldest.observed_world_revision {
                    return Err(GameplayReject::MutationPreconditionFailed {
                        resource: "retry_ledger",
                    });
                }
            }
            None if self.oldest_replayable_world_revision != WorldRevision::ZERO => {
                return Err(GameplayReject::MutationPreconditionFailed {
                    resource: "retry_ledger",
                });
            }
            None => {}
        }
        if let Some(pending) = &self.pending_receipt
            && (pending.observed_world_revision != self.observed_world_revision
                || self.recent_receipts.contains_key(&pending.transaction_id)
                || previous_receipt_revision
                    .is_some_and(|previous| previous > pending.observed_world_revision))
        {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "pending_retry_receipt",
            });
        }

        let mut persistent_entity_ids = BTreeSet::new();
        for player in self.inventories.keys() {
            if !persistent_entity_ids.insert(player.as_bytes()) {
                return Err(GameplayReject::DuplicateStateKey {
                    kind: "persistent_entity",
                });
            }
        }
        for container in self.containers.keys() {
            if !persistent_entity_ids.insert(container.as_bytes()) {
                return Err(GameplayReject::DuplicateStateKey {
                    kind: "persistent_entity",
                });
            }
        }
        for drop in self.drops.keys() {
            if !persistent_entity_ids.insert(drop.as_bytes()) {
                return Err(GameplayReject::DuplicateStateKey {
                    kind: "persistent_entity",
                });
            }
        }
        for inventory in self.inventories.values() {
            inventory.validate_structure()?;
            require_target_domain(&inventory.target, GameplayStorageDomain::PersistentEntities)?;
            self.require_loaded_chunk(&inventory.target.chunk)?;
            for stack in inventory.slots.iter().flatten() {
                catalog.validate_stack(stack)?;
            }
        }
        for (key, block) in &self.blocks {
            self.require_loaded_chunk(&self.block_chunk(key))?;
            if catalog.block(block).is_none() {
                return Err(GameplayReject::UnknownReference {
                    kind: "block",
                    id: block.as_str().to_owned(),
                });
            }
        }
        for drop in self.drops.values() {
            self.require_loaded_chunk(&self.block_chunk(&drop.location))?;
            catalog.validate_stack(&drop.stack)?;
        }
        for (id, container) in &self.containers {
            if id.as_persistent_entity_id() != &container.owner.entity {
                return Err(GameplayReject::MutationPreconditionFailed {
                    resource: "container_owner_identity",
                });
            }
            if container.slots.is_empty() || container.slots.len() > ABSOLUTE_MAX_CONTAINER_SLOTS {
                return Err(GameplayReject::LimitExceeded {
                    resource: "container_slots",
                    limit: ABSOLUTE_MAX_CONTAINER_SLOTS,
                    actual: container.slots.len(),
                });
            }
            let chunk = container.owner.chunk_key();
            self.require_loaded_chunk(&chunk)?;
            if let Some(workstation) = &container.workstation
                && !catalog.workstations.contains(workstation)
            {
                return Err(GameplayReject::UnknownReference {
                    kind: "workstation",
                    id: workstation.as_str().to_owned(),
                });
            }
            for stack in container.slots.iter().flatten() {
                catalog.validate_stack(stack)?;
            }
        }
        let mut mining_players = BTreeSet::new();
        for (key, progress) in &self.break_progress {
            self.require_loaded_chunk(&self.block_chunk(&key.block))?;
            if !self.inventories.contains_key(&key.player) {
                return Err(GameplayReject::UnknownPlayer {
                    player: key.player.as_bytes(),
                });
            }
            if !mining_players.insert(key.player) {
                return Err(GameplayReject::MutationPreconditionFailed {
                    resource: "multiple_mining_targets_per_player",
                });
            }
            if self.blocks.get(&key.block) != Some(&progress.block) {
                return Err(GameplayReject::MutationPreconditionFailed {
                    resource: "break_progress_block",
                });
            }
            if catalog.block(&progress.block).is_none() {
                return Err(GameplayReject::UnknownReference {
                    kind: "block",
                    id: progress.block.as_str().to_owned(),
                });
            }
        }
        let mut continuation_owners = BTreeSet::new();
        for continuation in self.continuations.values() {
            require_target_domain(&continuation.target, GameplayStorageDomain::Continuations)?;
            self.require_loaded_chunk(&continuation.target.chunk)?;
            if !continuation_owners.insert(continuation.container) {
                return Err(GameplayReject::ProcessAlreadyScheduled);
            }
            let container = self
                .containers
                .get(&continuation.container)
                .ok_or(GameplayReject::ContinuationOwnerMissing)?;
            if continuation.target.chunk != container.owner.chunk_key() {
                return Err(GameplayReject::MutationPreconditionFailed {
                    resource: "continuation_target",
                });
            }
            let process = catalog.process(&continuation.process).ok_or_else(|| {
                GameplayReject::UnknownReference {
                    kind: "process",
                    id: continuation.process.as_str().to_owned(),
                }
            })?;
            if container.workstation.as_ref() != Some(&process.workstation) {
                return Err(GameplayReject::WorkstationRequired {
                    required: process.workstation.clone(),
                });
            }
            let _ = container.slot(continuation.output_slot)?;
            catalog.validate_stack(&continuation.pending_output)?;
        }
        if let Some(pending) = &self.pending_receipt
            && pending.state_hash != self.canonical_hash()
        {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "pending_state_hash",
            });
        }
        Ok(())
    }
    pub(crate) fn persistent_entity_id_in_use(&self, entity: &PersistentEntityId) -> bool {
        self.inventories
            .keys()
            .any(|player| player.as_persistent_entity_id() == entity)
            || self
                .containers
                .keys()
                .any(|container| container.as_persistent_entity_id() == entity)
            || self
                .drops
                .keys()
                .any(|drop| drop.as_persistent_entity_id() == entity)
    }
    fn require_loaded_chunk(&self, chunk: &DimensionChunkKey) -> Result<(), GameplayReject> {
        if self.loaded_chunks.contains_key(chunk) {
            Ok(())
        } else {
            Err(GameplayReject::ChunkNotLoaded {
                dimension: chunk.dimension.as_str().to_owned(),
                x: chunk.coordinate.x,
                y: chunk.coordinate.y,
                z: chunk.coordinate.z,
            })
        }
    }
    /// Adds a decoded or fixture player inventory before command execution.
    ///
    /// # Errors
    ///
    /// Rejects duplicate players and the configured player boundary plus one.
    pub fn seed_player(
        &mut self,
        player: PlayerId,
        inventory: InventoryStateV1,
    ) -> Result<(), GameplayReject> {
        if self.inventories.contains_key(&player) {
            return Err(GameplayReject::DuplicateStateKey { kind: "player" });
        }
        if self.persistent_entity_id_in_use(player.as_persistent_entity_id()) {
            return Err(GameplayReject::DuplicateStateKey {
                kind: "persistent_entity",
            });
        }
        if self.inventories.len() >= self.limits.players {
            return Err(GameplayReject::LimitExceeded {
                resource: "players",
                limit: self.limits.players,
                actual: checked_capacity_plus_one(self.inventories.len(), "players")?,
            });
        }
        self.require_loaded_chunk(&inventory.target.chunk)?;
        self.inventories.insert(player, inventory);
        Ok(())
    }

    /// Adds a decoded or fixture container before command execution.
    ///
    /// # Errors
    ///
    /// Rejects duplicate containers and the configured boundary plus one.
    pub fn seed_container(
        &mut self,
        id: ContainerId,
        container: ContainerStateV1,
    ) -> Result<(), GameplayReject> {
        if self.containers.contains_key(&id) {
            return Err(GameplayReject::DuplicateStateKey { kind: "container" });
        }
        if self.persistent_entity_id_in_use(id.as_persistent_entity_id()) {
            return Err(GameplayReject::DuplicateStateKey {
                kind: "persistent_entity",
            });
        }
        if self.containers.len() >= self.limits.containers {
            return Err(GameplayReject::LimitExceeded {
                resource: "containers",
                limit: self.limits.containers,
                actual: checked_capacity_plus_one(self.containers.len(), "containers")?,
            });
        }
        self.require_loaded_chunk(&container.owner.chunk_key())?;
        if id.as_persistent_entity_id() != &container.owner.entity {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "container_owner_identity",
            });
        }
        self.containers.insert(id, container);
        Ok(())
    }

    /// Adds a decoded or fixture block override before command execution.
    ///
    /// # Errors
    ///
    /// Rejects the configured tracked-block boundary plus one.
    pub fn seed_block(&mut self, key: BlockKey, block: BlockId) -> Result<(), GameplayReject> {
        self.require_loaded_chunk(&self.block_chunk(&key))?;
        if self.blocks.contains_key(&key) {
            return Err(GameplayReject::DuplicateStateKey { kind: "block" });
        }
        if self.blocks.len() >= self.limits.tracked_blocks {
            return Err(GameplayReject::LimitExceeded {
                resource: "tracked_blocks",
                limit: self.limits.tracked_blocks,
                actual: checked_capacity_plus_one(self.blocks.len(), "tracked_blocks")?,
            });
        }
        self.blocks.insert(key, block);
        Ok(())
    }
    /// Adds a decoded or fixture dropped stack before command execution.
    ///
    /// # Errors
    ///
    /// Rejects duplicate drop identities, colliding persistent-entity IDs, and
    /// the configured drop boundary plus one.
    pub fn seed_drop(
        &mut self,
        id: DropEntityId,
        drop: DroppedItemV1,
    ) -> Result<(), GameplayReject> {
        if self.drops.contains_key(&id) {
            return Err(GameplayReject::DuplicateStateKey { kind: "drop" });
        }
        if self.persistent_entity_id_in_use(id.as_persistent_entity_id()) {
            return Err(GameplayReject::DuplicateStateKey {
                kind: "persistent_entity",
            });
        }
        if self.drops.len() >= self.limits.drops {
            return Err(GameplayReject::LimitExceeded {
                resource: "drops",
                limit: self.limits.drops,
                actual: checked_capacity_plus_one(self.drops.len(), "drops")?,
            });
        }
        self.require_loaded_chunk(&self.block_chunk(&drop.location))?;
        self.drops.insert(id, drop);
        Ok(())
    }

    /// Adds a decoded scheduled continuation before command execution.
    ///
    /// # Errors
    ///
    /// Rejects duplicates and the configured continuation boundary plus one.
    pub fn seed_continuation(
        &mut self,
        id: ContinuationId,
        continuation: FurnaceContinuationV1,
    ) -> Result<(), GameplayReject> {
        if self.continuations.contains_key(&id) {
            return Err(GameplayReject::DuplicateStateKey {
                kind: "continuation",
            });
        }
        if self.continuations.len() >= self.limits.continuations {
            return Err(GameplayReject::LimitExceeded {
                resource: "continuations",
                limit: self.limits.continuations,
                actual: checked_capacity_plus_one(self.continuations.len(), "continuations")?,
            });
        }

        self.require_loaded_chunk(&continuation.target.chunk)?;
        self.continuations.insert(id, continuation);

        Ok(())
    }
}

fn require_target_domain(
    target: &GameplayEditTarget,
    expected: GameplayStorageDomain,
) -> Result<(), GameplayReject> {
    if target.domain == expected {
        Ok(())
    } else {
        Err(GameplayReject::InvalidStorageTarget {
            expected,
            actual: target.domain,
        })
    }
}
fn checked_capacity_plus_one(
    current: usize,
    counter: &'static str,
) -> Result<usize, GameplayReject> {
    current
        .checked_add(1)
        .ok_or(GameplayReject::RevisionOverflow { counter })
}
/// Version-one runtime gameplay command envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandEnvelopeV1 {
    /// Ordered command key backed by the storage transaction identity.
    pub transaction_id: TransactionId,
    /// Storage-owned world revision observed by the caller.
    pub expected_world_revision: WorldRevision,
    /// Typed command payload.
    pub command: GameplayCommandV1,
}

/// Authoritative player capability mode selected by the host profile.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GameplayModeV1 {
    /// Ordinary inventory costs and no authority-only item creation.
    #[default]
    Survival,
    /// Placement retains inventory, one mining command atomically satisfies
    /// authored hardness without tool qualification or durability cost, and
    /// pick-block may create a catalog item stack. Authored drops remain
    /// authoritative.
    Creative,
}

/// Immutable gameplay rules bound to an authoritative planner instance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GameplayRulesV1 {
    /// Mode applied to the local authoritative player.
    pub player_mode: GameplayModeV1,
}

/// Version-one sandbox command set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameplayCommandV1 {
    /// Advance deterministic mining progress and possibly break a block.
    Mine(MineCommandV1),
    /// Cancel the player's transient mining target and accumulated progress.
    CancelMining(CancelMiningCommandV1),
    /// Drop part of an inventory stack into the world.
    DropItem(DropItemCommandV1),
    /// Pick up a complete dropped stack atomically.
    Pickup(PickupCommandV1),
    /// Set an empty block cell, consuming the item when rules require it.
    Place(PlaceCommandV1),
    /// Fill one hotbar slot from the catalog under creative authority.
    CreativePick(CreativePickCommandV1),
    /// Execute a shaped or shapeless recipe.
    Craft(RecipeCraftCommandV1),
    /// Transfer a quantity between a player and persistent container.
    Transfer(TransferCommandV1),
    /// Move or merge a player-inventory stack between two slots.
    MoveStack(MoveStackCommandV1),
    /// Select the hotbar prefix slot used by subsequent mine/place commands.
    SelectHotbar(SelectHotbarCommandV1),
    /// Atomically consume furnace input and fuel and schedule output.
    StartProcess(StartProcessCommandV1),
    /// Complete a bounded prefix of due scheduled processes.
    AdvanceScheduled(ScheduledAdvanceCommandV1),
}

/// Mining command payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MineCommandV1 {
    /// Player inventory containing an optional tool.
    pub player: PlayerId,
    /// Complete target block identity.
    pub target: BlockKey,
    /// Storage-owned chunk revision observed by the raycast/selection layer.
    pub expected_chunk_revision: ChunkRevision,
    /// Optional inventory slot holding a singleton tool.
    pub tool_slot: Option<SlotIndex>,
    /// World-scoped persistent entity identity reserved by the authoritative host.
    pub reserved_drop: DropEntityId,
    /// Canonical 60 Hz work steps represented by this simulation boundary.
    pub steps: MiningStepCountV1,
}

/// Mining cancellation payload emitted on button release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancelMiningCommandV1 {
    /// Player whose one active mining target is cancelled.
    pub player: PlayerId,
}

/// Explicit player-drop command payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DropItemCommandV1 {
    /// Source player inventory.
    pub player: PlayerId,
    /// Source slot.
    pub slot: SlotIndex,
    /// Non-zero quantity to drop.
    pub quantity: NonZeroU32,
    /// Complete dimension-qualified world anchor.
    pub location: BlockKey,
    /// World-scoped persistent entity identity reserved by the authoritative host.
    pub reserved_drop: DropEntityId,
}

/// Dropped-item pickup command payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PickupCommandV1 {
    /// Destination player inventory.
    pub player: PlayerId,
    /// Dropped entity to consume.
    pub drop: DropEntityId,
}

/// Block placement command payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceCommandV1 {
    /// Source player inventory.
    pub player: PlayerId,
    /// Slot holding the registered placement item.
    pub slot: SlotIndex,
    /// Complete empty target cell.
    pub target: BlockKey,
    /// Storage-owned chunk revision observed by the placement query.
    pub expected_chunk_revision: ChunkRevision,
}

/// Authority-only creative pick-block payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreativePickCommandV1 {
    /// Destination player inventory.
    pub player: PlayerId,
    /// Catalog item to place in the hotbar.
    pub item: ItemId,
    /// Destination hotbar slot.
    pub slot: SlotIndex,
    /// Observed player inventory revision.
    pub expected_inventory_revision: u64,
}

/// Recipe command payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeCraftCommandV1 {
    /// Player inventory used for input and output.
    pub player: PlayerId,
    /// Versioned recipe identity.
    pub recipe: RecipeId,
    /// Ordered crafting-grid slots for shaped recipes, or candidate slots for
    /// shapeless recipes.
    pub input_slots: Box<[SlotIndex]>,
    /// Optional workstation container satisfying the recipe contract.
    pub workstation: Option<ContainerId>,
}

/// Direction of a player/container transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferDirectionV1 {
    /// Player inventory to persistent container.
    PlayerToContainer,
    /// Persistent container to player inventory.
    ContainerToPlayer,
}

/// Atomic player/container transfer payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferCommandV1 {
    /// Player inventory.
    pub player: PlayerId,
    /// Persistent container.
    pub container: ContainerId,
    /// Player slot.
    pub player_slot: SlotIndex,
    /// Container slot.
    pub container_slot: SlotIndex,
    /// Non-zero quantity.
    pub quantity: NonZeroU32,
    /// Transfer direction.
    pub direction: TransferDirectionV1,
    /// Observed player inventory revision.
    pub expected_inventory_revision: u64,
    /// Observed container revision.
    pub expected_container_revision: u64,
}

/// Atomic player-inventory stack move payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveStackCommandV1 {
    /// Player inventory.
    pub player: PlayerId,
    /// Source slot.
    pub from: SlotIndex,
    /// Destination slot.
    pub to: SlotIndex,
    /// Quantity to move; `None` moves the entire `from` stack.
    pub quantity: Option<NonZeroU32>,
    /// Observed player inventory revision.
    pub expected_inventory_revision: u64,
}

/// Authoritative hotbar-selection command payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectHotbarCommandV1 {
    /// Player inventory.
    pub player: PlayerId,
    /// Hotbar prefix slot to select.
    pub slot: SlotIndex,
    /// Observed player inventory revision.
    pub expected_inventory_revision: u64,
}

/// Start-process command payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartProcessCommandV1 {
    /// Persistent machine container.
    pub container: ContainerId,
    /// Versioned process contract.
    pub process: ProcessId,
    /// Concrete input slot.
    pub input_slot: SlotIndex,
    /// Concrete fuel slot.
    pub fuel_slot: SlotIndex,
    /// Concrete output slot reserved by the continuation.
    pub output_slot: SlotIndex,
    /// Observed container revision.
    pub expected_container_revision: u64,
    /// Authoritative start tick supplied by the host.
    pub started_at: AuthorityTick,
}

/// Bounded scheduler command payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledAdvanceCommandV1 {
    /// Complete continuations due at or before this authoritative tick.
    pub through_tick: AuthorityTick,
    /// Caller-selected positive completion budget.
    pub max_completions: NonZeroU16,
}

/// Runtime-staged gameplay edit emitted by the reference planner.
///
/// Every edit names the exact dimension-qualified chunk and storage domain the
/// host must capture. These edits are not [`latticeaxiom_storage::ChunkMutation`]
/// values and never stand in for a storage commit receipt.
#[allow(
    clippy::large_enum_variant,
    reason = "inline before/after values make rollback allocation-free and the plan edit cap bounds memory"
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameplayMutationIntentV1 {
    /// Replace one player inventory slot after checking its prior value.
    InventorySlot {
        /// Explicit persistent-entity capture target.
        target: GameplayEditTarget,
        /// Player inventory.
        player: PlayerId,
        /// Slot index.
        slot: SlotIndex,
        /// Expected value.
        before: Option<ItemStackV1>,
        /// Replacement value.
        after: Option<ItemStackV1>,
    },
    /// Advance one player inventory component revision.
    InventoryRevision {
        /// Explicit persistent-entity capture target.
        target: GameplayEditTarget,
        /// Player inventory.
        player: PlayerId,
        /// Expected revision.
        before: u64,
        /// Replacement revision.
        after: u64,
    },
    /// Replace the selected hotbar prefix index.
    InventoryHotbar {
        /// Explicit persistent-entity capture target.
        target: GameplayEditTarget,
        /// Player inventory.
        player: PlayerId,
        /// Expected selected hotbar index.
        before: u16,
        /// Replacement selected hotbar index.
        after: u16,
    },
    /// Replace one persistent container slot.
    ContainerSlot {
        /// Explicit persistent-entity capture target.
        target: GameplayEditTarget,
        /// Persistent container.
        container: ContainerId,
        /// Slot index.
        slot: SlotIndex,
        /// Expected value.
        before: Option<ItemStackV1>,
        /// Replacement value.
        after: Option<ItemStackV1>,
    },
    /// Advance one persistent container component revision.
    ContainerRevision {
        /// Explicit persistent-entity capture target.
        target: GameplayEditTarget,
        /// Persistent container.
        container: ContainerId,
        /// Expected revision.
        before: u64,
        /// Replacement revision.
        after: u64,
    },
    /// Replace one block cell with a concrete block or the empty cell.
    Block {
        /// Explicit voxel capture target.
        target: GameplayEditTarget,
        /// Complete dimension-qualified voxel key.
        key: BlockKey,
        /// Expected concrete block.
        before: Option<BlockId>,
        /// Replacement concrete block.
        after: Option<BlockId>,
    },
    /// Insert, update, or remove one persistent dropped-item entity.
    DropEntity {
        /// Explicit persistent-entity capture target.
        target: GameplayEditTarget,
        /// Stable dropped entity key.
        id: DropEntityId,
        /// Expected value.
        before: Option<DroppedItemV1>,
        /// Replacement value.
        after: Option<DroppedItemV1>,
    },
    /// Update deterministic runtime-only mining progress.
    ///
    /// The explicit target scopes validation and hashing, but this transient
    /// input-session state is excluded from storage capture. A completed break
    /// persists the resulting voxel, inventory, and drop mutations atomically.
    BreakProgress {
        /// Explicit persistent-entity capture target.
        target: GameplayEditTarget,
        /// Player/block key.
        key: BreakProgressKey,
        /// Expected value.
        before: Option<BreakProgressV1>,
        /// Replacement value.
        after: Option<BreakProgressV1>,
    },
    /// Insert or remove deterministic scheduled work.
    Continuation {
        /// Explicit continuation-domain capture target.
        target: GameplayEditTarget,
        /// Canonical storage continuation key.
        id: ContinuationId,
        /// Expected value.
        before: Option<FurnaceContinuationV1>,
        /// Replacement value.
        after: Option<FurnaceContinuationV1>,
    },
}

impl GameplayMutationIntentV1 {
    /// Reserved schema identity; this crate registers no world-wire codec.
    pub const SCHEMA_ID: &'static str = "latticeaxiom:schema/gameplay-staged-edit@1";

    /// Returns the explicit storage capture target.
    #[must_use]
    pub const fn target(&self) -> &GameplayEditTarget {
        match self {
            Self::InventorySlot { target, .. }
            | Self::InventoryRevision { target, .. }
            | Self::InventoryHotbar { target, .. }
            | Self::ContainerSlot { target, .. }
            | Self::ContainerRevision { target, .. }
            | Self::Block { target, .. }
            | Self::DropEntity { target, .. }
            | Self::BreakProgress { target, .. }
            | Self::Continuation { target, .. } => target,
        }
    }

    /// Returns the target that must cross the storage capture boundary.
    ///
    /// Runtime-only mining progress remains validated and hashed through
    /// [`Self::target`] but deliberately returns `None` here.
    #[must_use]
    pub const fn storage_capture_target(&self) -> Option<&GameplayEditTarget> {
        match self {
            Self::BreakProgress { .. } => None,
            Self::InventorySlot { target, .. }
            | Self::InventoryRevision { target, .. }
            | Self::InventoryHotbar { target, .. }
            | Self::ContainerSlot { target, .. }
            | Self::ContainerRevision { target, .. }
            | Self::Block { target, .. }
            | Self::DropEntity { target, .. }
            | Self::Continuation { target, .. } => Some(target),
        }
    }
}
/// Typed successful command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcomeV1 {
    /// Mining progress did not yet break the target.
    MiningProgress {
        /// Accumulated work.
        accumulated: u32,
        /// Required work.
        required: u32,
    },
    /// The player's active mining target was cleared, if one existed.
    MiningCancelled {
        /// Whether an accumulated target was removed.
        had_progress: bool,
    },
    /// The block was staged for removal and a drop entity was staged.
    BlockBroken {
        /// Concrete drop entity.
        drop: DropEntityId,
        /// Storage-owned chunk whose voxel payload must be captured.
        affected_chunk: DimensionChunkKey,
    },
    /// A player-created drop was staged.
    ItemDropped {
        /// Concrete drop entity.
        drop: DropEntityId,
    },
    /// A drop was fully inserted and removed from the world.
    ItemPickedUp {
        /// Removed drop entity.
        drop: DropEntityId,
    },
    /// A concrete block placement was staged.
    BlockPlaced {
        /// Storage-owned chunk whose voxel payload must be captured.
        affected_chunk: DimensionChunkKey,
    },
    /// A recipe consumed inputs and inserted its frozen concrete output.
    Crafted {
        /// Recipe used.
        recipe: RecipeId,
        /// Frozen concrete output.
        output: ItemStackV1,
    },
    /// A player/container transfer completed.
    Transferred {
        /// Quantity transferred.
        quantity: u32,
    },
    /// A player-inventory stack move completed.
    StackMoved {
        /// Source slot.
        from: SlotIndex,
        /// Destination slot.
        to: SlotIndex,
    },
    /// The selected hotbar prefix slot changed or was confirmed.
    HotbarSelected {
        /// Selected hotbar index.
        slot: SlotIndex,
    },
    /// A creative catalog stack replaced the selected hotbar slot.
    CreativeStackPicked {
        /// Destination hotbar slot.
        slot: SlotIndex,
        /// Concrete catalog item selected.
        item: ItemId,
    },
    /// Input and fuel were consumed and a continuation was staged.
    ProcessScheduled {
        /// New continuation.
        continuation: ContinuationId,
        /// Eligible completion tick.
        due_tick: AuthorityTick,
    },
    /// A bounded prefix of due processes completed.
    ScheduledAdvanced {
        /// Number of completed continuations.
        completed: u16,
        /// Whether additional work was due at the same frontier.
        more_due: bool,
    },
}

/// Immutable runtime-staged gameplay plan.
///
/// The host applies this bounded plan to its validated loaded runtime state,
/// then captures every affected chunk as a complete
/// [`latticeaxiom_storage::WorldTransaction`]. The host-provided loaded state
/// supplies world scope; this plan intentionally does not invent a `WorldId`.
/// This type is not a storage transaction and its reference receipt is not a
/// durability acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameplayPlanV1 {
    pub(crate) transaction_id: TransactionId,
    pub(crate) expected_world_revision: WorldRevision,
    pub(crate) envelope_fingerprint: CommandFingerprintV1,
    pub(crate) edits: Box<[GameplayMutationIntentV1]>,
    pub(crate) plan_fingerprint: GameplayPlanHashV1,
    pub(crate) outcome: CommandOutcomeV1,
}

impl GameplayPlanV1 {
    /// Returns the ordered command/transaction identity.
    #[must_use]
    pub const fn transaction_id(&self) -> TransactionId {
        self.transaction_id
    }

    /// Returns the storage-owned world revision observed during planning.
    #[must_use]
    pub const fn expected_world_revision(&self) -> WorldRevision {
        self.expected_world_revision
    }

    /// Returns the canonical command-envelope fingerprint.
    #[must_use]
    pub const fn envelope_fingerprint(&self) -> CommandFingerprintV1 {
        self.envelope_fingerprint
    }

    /// Returns runtime-staged gameplay edits.
    #[must_use]
    pub const fn edits(&self) -> &[GameplayMutationIntentV1] {
        &self.edits
    }

    /// Returns the canonical staged-plan fingerprint.
    #[must_use]
    pub const fn plan_fingerprint(&self) -> GameplayPlanHashV1 {
        self.plan_fingerprint
    }

    /// Returns the typed result calculated during planning.
    #[must_use]
    pub const fn outcome(&self) -> &CommandOutcomeV1 {
        &self.outcome
    }
}

/// Reference-oracle acknowledgement published after a complete in-memory apply.
///
/// This receipt carries no durability level and must never be exposed as a
/// [`latticeaxiom_storage::CommitReceipt`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePlanReceiptV1 {
    /// Canonical storage transaction identity.
    pub transaction_id: TransactionId,
    /// Storage-owned world revision observed while the plan was produced.
    pub observed_world_revision: WorldRevision,
    /// Fingerprint of the exact command envelope accepted by this command ID.
    pub envelope_fingerprint: CommandFingerprintV1,
    /// Fingerprint of the exact staged gameplay plan.
    pub plan_fingerprint: GameplayPlanHashV1,
    /// Canonical reference-state hash after the runtime apply barrier.
    pub state_hash: ReferenceGameplayStateHashV1,
    /// Typed command result.
    pub outcome: CommandOutcomeV1,
}
/// Deterministic conformance-only fault injection point.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FaultInjection {
    /// Apply normally.
    #[default]
    None,
    /// Fail after the given positive number of mutation intents and roll back.
    AfterMutation(NonZeroU16),
}

/// Typed gameplay or catalog rejection.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum GameplayReject {
    /// A zero-valued runtime identifier was supplied.
    #[error("{kind} must be non-zero")]
    ZeroIdentifier {
        /// Identifier class.
        kind: &'static str,
    },
    /// A bounded collection exceeded its configured hard ceiling.
    #[error("{resource} has {actual} entries, exceeding hard limit {limit}")]
    LimitExceeded {
        /// Metered resource.
        resource: &'static str,
        /// Inclusive accepted limit.
        limit: usize,
        /// Observed value.
        actual: usize,
    },
    /// An input quantity was zero.
    #[error("item quantity must be non-zero")]
    ZeroQuantity,
    /// A stateful item attempted to form a multi-item stack.
    #[error("stateful item stack quantity must be one, found {quantity}")]
    StatefulStackQuantity {
        /// Rejected quantity.
        quantity: u32,
    },
    /// A catalog or decoded state key was duplicated.
    #[error("duplicate {kind} key")]
    DuplicateStateKey {
        /// Key class.
        kind: &'static str,
    },
    /// A catalog registration was duplicated.
    #[error("duplicate {kind} registration `{id}`")]
    DuplicateRegistration {
        /// Registration class.
        kind: &'static str,
        /// Canonical stable ID.
        id: String,
    },
    /// A catalog reference did not resolve.
    #[error("unknown {kind} reference `{id}`")]
    UnknownReference {
        /// Referenced registry kind.
        kind: &'static str,
        /// Canonical stable ID or typed key.
        id: String,
    },
    /// A closed predicate is malformed or exceeds its budget.
    #[error("invalid item predicate: {reason}")]
    InvalidPredicate {
        /// Stable, non-localized reason.
        reason: &'static str,
    },
    /// A recipe pattern is malformed.
    #[error("invalid recipe pattern: {reason}")]
    InvalidRecipe {
        /// Stable, non-localized reason.
        reason: &'static str,
    },
    /// A package-authored primary item-browser category is malformed.
    #[error("invalid item category `{category}`: {reason}")]
    InvalidItemCategory {
        /// Versioned category identity.
        category: String,
        /// Stable, non-localized reason.
        reason: &'static str,
    },
    /// A block-to-schema binding is malformed or incomplete.
    #[error("invalid block schema binding `{block}`: {reason}")]
    InvalidSchemaBinding {
        /// Exact catalog block.
        block: BlockId,
        /// Stable, non-localized reason.
        reason: &'static str,
    },
    /// A frozen output role is missing or incompatible.
    #[error("frozen role binding rejected for `{role}`")]
    FrozenRoleRejected {
        /// Versioned role contract.
        role: String,
    },
    /// An arithmetic revision or allocator counter overflowed.
    #[error("checked counter `{counter}` overflowed")]
    RevisionOverflow {
        /// Counter class.
        counter: &'static str,
    },
    /// The caller observed a different storage-owned world revision.
    #[error("stale world revision: expected {expected}, actual {actual}")]
    StaleWorldRevision {
        /// Caller-provided revision.
        expected: u64,
        /// Loaded storage revision.
        actual: u64,
    },
    /// A retry fell before the bounded storage-revision replay horizon.
    #[error(
        "transaction retry {transaction_id:?} is older than replayable world revision {oldest_replayable_world_revision}"
    )]
    RetryWindowExpired {
        /// Retried canonical transaction identity.
        transaction_id: [u8; 16],
        /// Oldest storage base revision still replayable.
        oldest_replayable_world_revision: u64,
    },
    /// A retained transaction ID was retried with a different canonical envelope.
    #[error("transaction {transaction_id:?} was reused with a different payload")]
    RetryPayloadMismatch {
        /// Reused canonical transaction identity.
        transaction_id: [u8; 16],
    },
    /// A staged plan is waiting for the host's storage commit observation.
    #[error(
        "storage commit for transaction {transaction_id:?} at observed world revision {observed_world_revision} is still pending"
    )]
    StorageCommitPending {
        /// Runtime-staged transaction awaiting authoritative storage capture.
        transaction_id: [u8; 16],
        /// Storage-owned base revision used by the pending plan.
        observed_world_revision: u64,
    },
    /// No runtime-staged gameplay plan is awaiting a storage acknowledgement.
    #[error("no runtime-staged gameplay plan is awaiting a storage commit")]
    StorageCommitNotPending,
    /// A storage commit receipt does not match the runtime-staged gameplay plan.
    #[error("storage commit receipt does not match pending gameplay plan: {resource}")]
    StorageCommitMismatch {
        /// Receipt field or collection that failed reconciliation.
        resource: &'static str,
    },
    /// A command targeted a dimension-qualified chunk absent from the loaded snapshot.
    #[error("target chunk is not loaded: {dimension} ({x}, {y}, {z})")]
    ChunkNotLoaded {
        /// Registered dimension identity.
        dimension: String,
        /// Horizontal chunk coordinate.
        x: i32,
        /// Vertical chunk coordinate.
        y: i32,
        /// Depth chunk coordinate.
        z: i32,
    },
    /// A staged edit named the wrong storage domain for its payload.
    #[error("invalid storage target domain: expected {expected:?}, found {actual:?}")]
    InvalidStorageTarget {
        /// Required domain.
        expected: GameplayStorageDomain,
        /// Supplied domain.
        actual: GameplayStorageDomain,
    },
    /// A player key does not exist.
    #[error("unknown player {player:?}")]
    UnknownPlayer {
        /// Player key.
        player: [u8; 16],
    },
    /// A persistent container key does not exist.
    #[error("unknown persistent container")]
    UnknownContainer,
    /// A dropped entity key does not exist.
    #[error("unknown dropped-item entity {drop:?}")]
    UnknownDrop {
        /// Dropped entity key.
        drop: [u8; 16],
    },
    /// A block cell is empty.
    #[error("target block cell is empty")]
    BlockMissing,
    /// A placement cell is occupied.
    #[error("target block cell is occupied")]
    BlockOccupied,
    /// A selected inventory or container slot is out of range.
    #[error("slot {slot:?} is outside {slots} slots")]
    SlotOutOfRange {
        /// Rejected index.
        slot: SlotIndex,
        /// Slot count.
        slots: usize,
    },
    /// An authority-only creative operation was requested under survival rules.
    #[error("operation requires creative gameplay authority")]
    CreativeModeRequired,
    /// A required slot is empty.
    #[error("required slot is empty")]
    EmptySlot,
    /// A slot contains a different concrete item or state.
    #[error("slot content does not match the requested operation")]
    SlotMismatch,
    /// A quantity exceeds a definition's stack limit.
    #[error("stack quantity {quantity} exceeds limit {limit} for `{item}`")]
    StackLimitExceeded {
        /// Concrete item.
        item: ItemId,
        /// Rejected quantity.
        quantity: u32,
        /// Inclusive limit.
        limit: u32,
    },
    /// Checked quantity arithmetic overflowed.
    #[error("checked item quantity arithmetic overflowed")]
    QuantityOverflow,
    /// The destination inventory cannot accept the complete operation.
    #[error("player inventory has no capacity for the complete output")]
    InventoryFull,
    /// A machine/container output slot cannot accept the complete output.
    #[error("container output slot has no capacity for the complete output")]
    OutputFull,
    /// The selected item does not place the requested concrete block.
    #[error("item `{item}` has no placement block")]
    NotPlacementItem {
        /// Concrete item.
        item: ItemId,
    },
    /// A mining rule requires a tool but none was supplied.
    #[error("mining target requires a tool")]
    ToolRequired,
    /// A selected item is not a registered tool.
    #[error("item `{item}` is not a registered tool")]
    NotATool {
        /// Concrete item.
        item: ItemId,
    },
    /// The selected tool has the wrong class.
    #[error("tool class mismatch: required `{required}`, found `{actual}`")]
    ToolClassMismatch {
        /// Required class.
        required: Box<ToolClassId>,
        /// Actual class.
        actual: Box<ToolClassId>,
    },
    /// The selected tool tier is too low.
    #[error("tool tier {actual} is below required tier {required}")]
    ToolTierTooLow {
        /// Required minimum tier.
        required: u8,
        /// Actual tier.
        actual: u8,
    },
    /// A tool has already exhausted its durability.
    #[error("tool durability is exhausted")]
    ToolBroken,
    /// A registered tool item lacks its singleton durability state.
    #[error("tool item `{item}` lacks durability state")]
    ToolStateMissing {
        /// Concrete tool item.
        item: ItemId,
    },
    /// An item stack carries state incompatible with its exact definition.
    #[error("item `{item}` carries state incompatible with its definition")]
    ItemStateMismatch {
        /// Concrete item.
        item: ItemId,
    },
    /// Persistent durability exceeds the definition maximum.
    #[error("item `{item}` durability {remaining} exceeds maximum {maximum}")]
    DurabilityOutOfRange {
        /// Concrete item.
        item: ItemId,
        /// Rejected remaining durability.
        remaining: u32,
        /// Inclusive definition maximum.
        maximum: u32,
    },
    /// The observed chunk revision is stale.
    #[error("stale chunk revision: expected {expected}, actual {actual}")]
    StaleChunkRevision {
        /// Caller-provided revision.
        expected: u64,
        /// Current revision.
        actual: u64,
    },
    /// The observed inventory revision is stale.
    #[error("stale inventory revision: expected {expected}, actual {actual}")]
    StaleInventoryRevision {
        /// Caller-provided revision.
        expected: u64,
        /// Current revision.
        actual: u64,
    },
    /// The observed container revision is stale.
    #[error("stale container revision: expected {expected}, actual {actual}")]
    StaleContainerRevision {
        /// Caller-provided revision.
        expected: u64,
        /// Current revision.
        actual: u64,
    },
    /// Recipe input slots do not satisfy the closed pattern.
    #[error("selected input slots do not match recipe `{recipe}`")]
    RecipeMismatch {
        /// Versioned recipe identity.
        recipe: RecipeId,
    },
    /// A required workstation is absent or mismatched.
    #[error("recipe or process requires workstation `{required}`")]
    WorkstationRequired {
        /// Versioned workstation contract.
        required: WorkstationId,
    },
    /// A fuel slot is not explicitly admitted by any process fuel rule.
    #[error("item `{item}` is not qualified fuel for this process")]
    FuelRejected {
        /// Concrete item.
        item: ItemId,
    },
    /// One fuel unit does not cover the process duration.
    #[error("fuel provides {available} ticks but process requires {required}")]
    InsufficientFuel {
        /// Available burn ticks.
        available: u32,
        /// Required process ticks.
        required: u32,
    },
    /// A machine already owns an unfinished continuation.
    #[error("container already has an active scheduled process")]
    ProcessAlreadyScheduled,
    /// A continuation refers to a missing machine container.
    #[error("scheduled continuation refers to a missing container")]
    ContinuationOwnerMissing,
    /// One mutation's expected value no longer matches authority state.
    #[error("mutation precondition failed for {resource}")]
    MutationPreconditionFailed {
        /// Mutated resource class.
        resource: &'static str,
    },
    /// Deterministic conformance fault fired and the transaction was rolled back.
    #[error("injected transaction fault after mutation {after_mutation}")]
    InjectedFault {
        /// One-based applied-mutation index.
        after_mutation: u16,
    },
}

impl GameplayReject {
    /// Returns a stable machine diagnostic code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ZeroIdentifier { .. } => "gameplay.zero_identifier",
            Self::LimitExceeded { .. } => "gameplay.limit_exceeded",
            Self::ZeroQuantity => "gameplay.zero_quantity",
            Self::StatefulStackQuantity { .. } => "gameplay.stateful_stack_quantity",
            Self::DuplicateStateKey { .. } => "gameplay.duplicate_state_key",
            Self::DuplicateRegistration { .. } => "gameplay.duplicate_registration",
            Self::UnknownReference { .. } => "gameplay.unknown_reference",
            Self::InvalidPredicate { .. } => "gameplay.invalid_predicate",
            Self::InvalidRecipe { .. } => "gameplay.invalid_recipe",
            Self::InvalidItemCategory { .. } => "gameplay.invalid_item_category",
            Self::InvalidSchemaBinding { .. } => "gameplay.invalid_schema_binding",
            Self::FrozenRoleRejected { .. } => "gameplay.frozen_role_rejected",
            Self::RevisionOverflow { .. } => "gameplay.revision_overflow",
            Self::StaleWorldRevision { .. } => "gameplay.stale_world_revision",
            Self::RetryWindowExpired { .. } => "gameplay.retry_window_expired",
            Self::RetryPayloadMismatch { .. } => "gameplay.retry_payload_mismatch",
            Self::StorageCommitPending { .. } => "gameplay.storage_commit_pending",
            Self::StorageCommitNotPending => "gameplay.storage_commit_not_pending",
            Self::StorageCommitMismatch { .. } => "gameplay.storage_commit_mismatch",
            Self::ChunkNotLoaded { .. } => "gameplay.chunk_not_loaded",
            Self::InvalidStorageTarget { .. } => "gameplay.invalid_storage_target",
            Self::UnknownPlayer { .. } => "gameplay.unknown_player",
            Self::UnknownContainer => "gameplay.unknown_container",
            Self::UnknownDrop { .. } => "gameplay.unknown_drop",
            Self::BlockMissing => "gameplay.block_missing",
            Self::BlockOccupied => "gameplay.block_occupied",
            Self::SlotOutOfRange { .. } => "gameplay.slot_out_of_range",
            Self::CreativeModeRequired => "gameplay.creative_mode_required",
            Self::EmptySlot => "gameplay.empty_slot",
            Self::SlotMismatch => "gameplay.slot_mismatch",
            Self::StackLimitExceeded { .. } => "gameplay.stack_limit_exceeded",
            Self::QuantityOverflow => "gameplay.quantity_overflow",
            Self::InventoryFull => "gameplay.inventory_full",
            Self::OutputFull => "gameplay.output_full",
            Self::NotPlacementItem { .. } => "gameplay.not_placement_item",
            Self::ToolRequired => "gameplay.tool_required",
            Self::NotATool { .. } => "gameplay.not_a_tool",
            Self::ToolClassMismatch { .. } => "gameplay.tool_class_mismatch",
            Self::ToolTierTooLow { .. } => "gameplay.tool_tier_too_low",
            Self::ToolBroken => "gameplay.tool_broken",
            Self::ToolStateMissing { .. } => "gameplay.tool_state_missing",
            Self::ItemStateMismatch { .. } => "gameplay.item_state_mismatch",
            Self::DurabilityOutOfRange { .. } => "gameplay.durability_out_of_range",
            Self::StaleChunkRevision { .. } => "gameplay.stale_chunk_revision",
            Self::StaleInventoryRevision { .. } => "gameplay.stale_inventory_revision",
            Self::StaleContainerRevision { .. } => "gameplay.stale_container_revision",
            Self::RecipeMismatch { .. } => "gameplay.recipe_mismatch",
            Self::WorkstationRequired { .. } => "gameplay.workstation_required",
            Self::FuelRejected { .. } => "gameplay.fuel_rejected",
            Self::InsufficientFuel { .. } => "gameplay.insufficient_fuel",
            Self::ProcessAlreadyScheduled => "gameplay.process_already_scheduled",
            Self::ContinuationOwnerMissing => "gameplay.continuation_owner_missing",
            Self::MutationPreconditionFailed { .. } => "gameplay.mutation_precondition_failed",
            Self::InjectedFault { .. } => "gameplay.injected_fault",
        }
    }
}

#[cfg(test)]
mod residency_tests {
    #![allow(clippy::expect_used, reason = "validated residency fixtures")]
    use super::*;
    #[test]
    fn a_durably_unloaded_chunk_can_be_hydrated_into_a_fresh_cache_revision() {
        let mut state = ReferenceGameplayState::new(GameplayLimits::default()).expect("state");
        let key = DimensionChunkKey::new(
            "example:dimension/world".parse().expect("dimension"),
            latticeaxiom_storage::ChunkCoordinate::new(1, 0, 1),
        );
        state
            .ensure_loaded_chunk(key.clone(), ChunkRevision::new(9))
            .expect("old observation");
        assert!(state.release_loaded_chunk(&key).expect("release"));
        assert!(state.loaded_chunks().is_empty());
        state
            .ensure_loaded_chunk(key.clone(), ChunkRevision::new(1))
            .expect("fresh hydration");
        assert_eq!(
            state.loaded_chunk_revision(&key),
            Some(ChunkRevision::new(1))
        );
    }
}
