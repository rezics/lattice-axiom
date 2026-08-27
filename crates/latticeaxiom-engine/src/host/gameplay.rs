//! Inventory, hotbar, mining progress, drops, pickup, and recipes on the spine.
//!
//! Package-authored catalogs are supplied by the caller. This module does not
//! embed Terrenia item, recipe, or block identifiers.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_gameplay::{
    AuthorityTick, BlockId, BlockKey, BlockPosition, ChunkRevision, CommandEnvelopeV1,
    CommandOutcomeV1, ContainerId, ContainerOwnerComponentV1, ContainerStateV1, ContinuationId,
    CreativePickCommandV1, DimensionChunkKey, DimensionId, DropEntityId, DroppedItemV1,
    FaultInjection, FurnaceContinuationV1, GameplayCatalog, GameplayCommandV1, GameplayEditTarget,
    GameplayKernel, GameplayLimits, GameplayModeV1, GameplayReject, GameplayRulesV1,
    GameplayStorageDomain, IngredientV1, InventoryInspectV1, InventoryStateV1, ItemId, ItemStackV1,
    ItemStateV1, MineCommandV1, MoveStackCommandV1, PickupCommandV1, PlaceCommandV1, PlayerId,
    ProcessId, RecipeCraftCommandV1, RecipeId, RecipeInspectV1, RecipePatternV1,
    ReferenceGameplayState, ReferencePlanApplier, RuntimePlanReceiptV1, ScheduledAdvanceCommandV1,
    SlotIndex, StartProcessCommandV1, ToolClassId, TransactionId, TransferCommandV1, WorkstationId,
    WorldId, WorldRevision,
};
use latticeaxiom_player::BlockEditRejectV1;
use latticeaxiom_storage::{ChangedDomains, PublicationReceipt};

/// Player inventory size used by the production host.
pub const INVENTORY_SLOTS: usize = 36;
/// Hotbar prefix of [`INVENTORY_SLOTS`].
pub const HOTBAR_SLOTS: u16 = 9;
const CRAFTING_GRID_ORIGIN: u16 = 27;

/// In-memory gameplay session bound to [`MemoryTransactionKernel`] commits.
#[derive(Clone, Debug)]
pub(super) struct ProductionGameplay {
    applier: ReferencePlanApplier,
    mode: GameplayModeV1,
    player: PlayerId,
    dimension: DimensionId,
    hotbar_slot: u16,
    next_drop: u64,
    last_outcome: Option<CommandOutcomeV1>,
    last_reject: Option<GameplayReject>,
    bound_workstations: BTreeSet<WorkstationId>,
}

/// Snapshot of the local inventory and selected hotbar slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductionInventoryView {
    hotbar_slot: u16,
    slots: Box<[Option<ItemStackV1>]>,
}

impl ProductionInventoryView {
    /// Returns the selected hotbar index in `0..HOTBAR_SLOTS`.
    #[must_use]
    pub const fn hotbar_slot(&self) -> u16 {
        self.hotbar_slot
    }

    /// Returns every inventory slot in index order.
    #[must_use]
    pub const fn slots(&self) -> &[Option<ItemStackV1>] {
        &self.slots
    }

    /// Returns the selected hotbar stack.
    #[must_use]
    pub fn selected(&self) -> Option<&ItemStackV1> {
        self.slots.get(usize::from(self.hotbar_slot))?.as_ref()
    }

    /// Counts copies of `item` across every slot.
    #[must_use]
    pub fn count_item(&self, item: &ItemId) -> u32 {
        self.slots.iter().flatten().fold(0, |total, stack| {
            if stack.item() == item {
                total.saturating_add(stack.quantity())
            } else {
                total
            }
        })
    }

    /// Returns remaining durability of the first matching tool stack.
    #[must_use]
    pub fn tool_durability(&self, item: &ItemId) -> Option<u32> {
        self.slots.iter().flatten().find_map(|stack| {
            if stack.item() != item {
                return None;
            }
            match stack.state() {
                ItemStateV1::ToolDurability { remaining } => Some(remaining.get()),
                ItemStateV1::Plain => None,
            }
        })
    }
}

impl ProductionGameplay {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        world: WorldId,
        catalog: GameplayCatalog,
        player: PlayerId,
        dimension: DimensionId,
        spawn_chunk: DimensionChunkKey,
        loaded: BTreeMap<DimensionChunkKey, ChunkRevision>,
        world_revision: WorldRevision,
        chunk_edge: u16,
        mode: GameplayModeV1,
    ) -> Result<Self, GameplayReject> {
        let mut state = ReferenceGameplayState::new(GameplayLimits::default())?;
        state.set_chunk_edge(chunk_edge)?;
        for (chunk, revision) in loaded {
            state.ensure_loaded_chunk(chunk, revision)?;
        }
        state.observe_world_revision(world_revision)?;
        let inventory = InventoryStateV1::empty(
            GameplayEditTarget::new(spawn_chunk, GameplayStorageDomain::PersistentEntities),
            INVENTORY_SLOTS,
        )?;
        state.seed_player(player, inventory)?;
        let mut session = Self {
            applier: ReferencePlanApplier::try_new_with_rules(
                world,
                state,
                catalog,
                GameplayRulesV1 { player_mode: mode },
            )?,
            mode,
            player,
            dimension,
            hotbar_slot: 0,
            next_drop: 10_000,
            last_outcome: None,
            last_reject: None,
            bound_workstations: BTreeSet::new(),
        };
        session.seed_starter_inventory()?;
        Ok(session)
    }

    pub(super) fn catalog(&self) -> &GameplayCatalog {
        self.applier.catalog()
    }

    pub(super) const fn mode(&self) -> GameplayModeV1 {
        self.mode
    }

    /// Seeds a small, catalog-derived starting kit so a new session can place
    /// a block and begin the gather/craft loop immediately.  No package ID is
    /// embedded here: replacement content decides which placeable item and
    /// tool are available through its compiled catalog.
    fn seed_starter_inventory(&mut self) -> Result<(), GameplayReject> {
        let starter_item = self
            .catalog()
            .items()
            .values()
            .find(|item| item.placement_block.is_some())
            .map(|item| (item.id.clone(), item.stack_limit.get().min(16)));
        if let Some((item_id, quantity)) = starter_item {
            self.seed_slot(
                SlotIndex::new(0),
                Some(ItemStackV1::plain(item_id, quantity)?),
            )?;
        }
        let starter_tool = self
            .catalog()
            .tools()
            .values()
            .next()
            .map(|tool| (tool.item.clone(), tool.maximum_durability.get()));
        if let Some((tool_id, durability)) = starter_tool {
            self.seed_slot(
                SlotIndex::new(1),
                Some(ItemStackV1::tool(tool_id, durability)?),
            )?;
        }
        Ok(())
    }
    pub(super) fn select_hotbar_slot(&mut self, slot: u16) -> Result<(), GameplayReject> {
        if slot >= HOTBAR_SLOTS {
            return Err(GameplayReject::SlotOutOfRange {
                slot: SlotIndex::new(slot),
                slots: usize::from(HOTBAR_SLOTS),
            });
        }
        self.hotbar_slot = slot;
        Ok(())
    }

    pub(super) fn inventory_view(&self) -> Option<ProductionInventoryView> {
        let inventory = self.applier.state().inventory(self.player)?;
        Some(ProductionInventoryView {
            hotbar_slot: self.hotbar_slot,
            slots: inventory.slots().to_vec().into_boxed_slice(),
        })
    }

    pub(super) fn containers(&self) -> &std::collections::BTreeMap<ContainerId, ContainerStateV1> {
        self.applier.state().containers()
    }

    pub(super) fn continuations(
        &self,
    ) -> &std::collections::BTreeMap<ContinuationId, FurnaceContinuationV1> {
        self.applier.state().continuations()
    }

    pub(super) fn restore_inventory(
        &mut self,
        slots: &[Option<ItemStackV1>],
    ) -> Result<(), GameplayReject> {
        for (index, stack) in slots.iter().enumerate() {
            let slot = u16::try_from(index).map_err(|_| GameplayReject::SlotOutOfRange {
                slot: SlotIndex::new(0),
                slots: slots.len(),
            })?;
            self.seed_slot(SlotIndex::new(slot), stack.clone())?;
        }
        Ok(())
    }

    pub(super) fn restore_container(
        &mut self,
        id: ContainerId,
        container: ContainerStateV1,
    ) -> Result<(), GameplayReject> {
        if self.applier.state().container(id).is_some() {
            return Ok(());
        }
        self.applier.state_mut().seed_container(id, container)
    }

    pub(super) fn restore_continuation(
        &mut self,
        id: ContinuationId,
        continuation: FurnaceContinuationV1,
    ) -> Result<(), GameplayReject> {
        if self.applier.state().continuation(id).is_some() {
            return Ok(());
        }
        self.applier.state_mut().seed_continuation(id, continuation)
    }

    pub(super) fn restore_drop(
        &mut self,
        id: DropEntityId,
        drop: DroppedItemV1,
    ) -> Result<(), GameplayReject> {
        if self.applier.state().dropped_item(id).is_some() {
            return Ok(());
        }
        self.applier.state_mut().seed_drop(id, drop)
    }

    pub(super) fn set_next_drop(&mut self, next_drop: u64) {
        self.next_drop = self.next_drop.max(next_drop);
    }

    pub(super) const fn next_drop(&self) -> u64 {
        self.next_drop
    }

    pub(super) fn dropped_items(&self) -> &BTreeMap<DropEntityId, DroppedItemV1> {
        self.applier.state().dropped_items()
    }

    pub(super) fn inventory_inspect(&self) -> Result<InventoryInspectV1, GameplayReject> {
        GameplayKernel::new(self.catalog()).inspect_inventory(self.applier.state(), self.player)
    }

    pub(super) fn recipe_inspect(
        &self,
        workstation: Option<&WorkstationId>,
    ) -> Result<Vec<RecipeInspectV1>, GameplayReject> {
        GameplayKernel::new(self.catalog()).inspect_recipes(
            self.applier.state(),
            self.player,
            workstation,
        )
    }

    pub(super) fn last_outcome(&self) -> Option<&CommandOutcomeV1> {
        self.last_outcome.as_ref()
    }

    pub(super) fn last_reject(&self) -> Option<&GameplayReject> {
        self.last_reject.as_ref()
    }

    pub(super) fn pending_storage_chunks(
        &self,
    ) -> Option<&BTreeMap<DimensionChunkKey, ChangedDomains>> {
        self.applier.pending_storage_chunks()
    }

    pub(super) fn observe_storage_publication(
        &mut self,
        receipt: &PublicationReceipt,
    ) -> Result<(), GameplayReject> {
        self.applier.observe_storage_publication(receipt)
    }

    pub(super) fn sync_loaded_world(
        &mut self,
        world_revision: WorldRevision,
        chunks: impl IntoIterator<Item = (DimensionChunkKey, ChunkRevision)>,
    ) -> Result<(), GameplayReject> {
        if let Some(pending) = self.applier.state().pending_receipt() {
            return Err(GameplayReject::StorageCommitPending {
                transaction_id: *pending.transaction_id.as_bytes(),
                observed_world_revision: pending.observed_world_revision.get(),
            });
        }
        let state = self.applier.state_mut();
        for (chunk, revision) in chunks {
            state.ensure_loaded_chunk(chunk, revision)?;
        }
        state.observe_world_revision(world_revision)
    }

    pub(super) fn prepare_mine(
        &mut self,
        target: BlockPosition,
        block: BlockId,
        expected_chunk_revision: ChunkRevision,
    ) -> Result<GameplayCommandV1, GameplayReject> {
        let key = BlockKey::new(self.dimension.clone(), target);
        self.applier
            .state_mut()
            .sync_occupied_block(key.clone(), block)?;
        let reserved_drop = DropEntityId::new(self.next_drop);
        self.next_drop = self.next_drop.saturating_add(1);
        Ok(GameplayCommandV1::Mine(MineCommandV1 {
            player: self.player,
            target: key,
            expected_chunk_revision,
            tool_slot: self.selected_tool_slot(),
            reserved_drop,
        }))
    }

    pub(super) fn prepare_place(
        &mut self,
        target: BlockPosition,
        expected_chunk_revision: ChunkRevision,
        requested_block: Option<&BlockId>,
    ) -> Result<(GameplayCommandV1, BlockId), GameplayReject> {
        let key = BlockKey::new(self.dimension.clone(), target);
        self.applier.state_mut().clear_occupied_block(&key);
        let slot = self.placement_slot(requested_block)?;
        let inventory =
            self.applier
                .state()
                .inventory(self.player)
                .ok_or(GameplayReject::UnknownPlayer {
                    player: self.player.as_bytes(),
                })?;
        let stack = inventory.slot(slot)?.ok_or(GameplayReject::EmptySlot)?;
        let item =
            self.catalog()
                .item(stack.item())
                .ok_or_else(|| GameplayReject::UnknownReference {
                    kind: "item",
                    id: stack.item().as_str().to_owned(),
                })?;
        let block =
            item.placement_block
                .clone()
                .ok_or_else(|| GameplayReject::NotPlacementItem {
                    item: stack.item().clone(),
                })?;
        Ok((
            GameplayCommandV1::Place(PlaceCommandV1 {
                player: self.player,
                slot,
                target: key,
                expected_chunk_revision,
            }),
            block,
        ))
    }

    pub(super) fn execute(
        &mut self,
        transaction_id: TransactionId,
        command: GameplayCommandV1,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        let envelope = CommandEnvelopeV1 {
            transaction_id,
            expected_world_revision: self.applier.state().observed_world_revision(),
            command,
        };
        match self.applier.execute(&envelope, FaultInjection::None) {
            Ok(receipt) => {
                self.last_outcome = Some(receipt.outcome.clone());
                self.last_reject = None;
                Ok(receipt)
            }
            Err(error) => {
                self.last_reject = Some(error.clone());
                Err(error)
            }
        }
    }

    pub(super) fn pickup(
        &mut self,
        transaction_id: TransactionId,
        drop: DropEntityId,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        self.execute(
            transaction_id,
            GameplayCommandV1::Pickup(PickupCommandV1 {
                player: self.player,
                drop,
            }),
        )
    }

    pub(super) fn craft(
        &mut self,
        transaction_id: TransactionId,
        recipe: &RecipeId,
        workstation: Option<ContainerId>,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        let input_slots = self.layout_recipe(recipe)?;
        self.execute(
            transaction_id,
            GameplayCommandV1::Craft(RecipeCraftCommandV1 {
                player: self.player,
                recipe: recipe.clone(),
                input_slots,
                workstation,
            }),
        )
    }

    pub(super) fn start_process(
        &mut self,
        transaction_id: TransactionId,
        process: &ProcessId,
        container: ContainerId,
        expected_container_revision: u64,
        started_at: AuthorityTick,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        self.execute(
            transaction_id,
            GameplayCommandV1::StartProcess(StartProcessCommandV1 {
                container,
                process: process.clone(),
                input_slot: SlotIndex::new(0),
                fuel_slot: SlotIndex::new(1),
                output_slot: SlotIndex::new(2),
                expected_container_revision,
                started_at,
            }),
        )
    }

    pub(super) fn advance_scheduled(
        &mut self,
        transaction_id: TransactionId,
        through_tick: AuthorityTick,
        max_completions: std::num::NonZeroU16,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        self.execute(
            transaction_id,
            GameplayCommandV1::AdvanceScheduled(ScheduledAdvanceCommandV1 {
                through_tick,
                max_completions,
            }),
        )
    }

    pub(super) fn transfer(
        &mut self,
        transaction_id: TransactionId,
        command: TransferCommandV1,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        self.execute(transaction_id, GameplayCommandV1::Transfer(command))
    }

    /// Moves or merges the `from` stack onto `to`. `quantity == None` (the full stack).
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the source is empty, the slots are out of
    /// range, or the observed inventory revision is stale.
    pub(super) fn move_stack(
        &mut self,
        transaction_id: TransactionId,
        from: SlotIndex,
        to: SlotIndex,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        let inventory =
            self.applier
                .state()
                .inventory(self.player)
                .ok_or(GameplayReject::UnknownPlayer {
                    player: self.player.as_bytes(),
                })?;
        let expected_inventory_revision = inventory.revision();
        self.execute(
            transaction_id,
            GameplayCommandV1::MoveStack(MoveStackCommandV1 {
                player: self.player,
                from,
                to,
                quantity: None,
                expected_inventory_revision,
            }),
        )
    }

    /// Selects, swaps, or creates the inventory stack whose placement block equals `aimed`.
    ///
    /// Hotbar hits only change the selected slot. Under survival rules,
    /// body-inventory hits move onto the selected hotbar slot and missing stacks
    /// fail closed. Under creative rules, other cases replace the selected slot
    /// with a full catalog stack through the authoritative command pipeline.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the block has no placement item or the
    /// authoritative inventory command fails.
    pub(super) fn pick_aimed_block(
        &mut self,
        transaction_id: TransactionId,
        aimed: &BlockId,
    ) -> Result<Option<RuntimePlanReceiptV1>, GameplayReject> {
        if let Ok(source) = self.placing_slot(aimed) {
            if source.get() < HOTBAR_SLOTS {
                self.select_hotbar_slot(source.get())?;
                return Ok(None);
            }
            if self.mode == GameplayModeV1::Survival {
                let destination = SlotIndex::new(self.hotbar_slot);
                return self
                    .move_stack(transaction_id, source, destination)
                    .map(Some);
            }
        }
        if self.mode == GameplayModeV1::Survival {
            return Err(GameplayReject::EmptySlot);
        }
        let item = self
            .catalog()
            .items()
            .values()
            .find(|item| item.placement_block.as_ref() == Some(aimed))
            .map(|item| item.id.clone())
            .ok_or_else(|| GameplayReject::UnknownReference {
                kind: "placement_item",
                id: aimed.as_str().to_owned(),
            })?;
        let inventory =
            self.applier
                .state()
                .inventory(self.player)
                .ok_or(GameplayReject::UnknownPlayer {
                    player: self.player.as_bytes(),
                })?;
        self.execute(
            transaction_id,
            GameplayCommandV1::CreativePick(CreativePickCommandV1 {
                player: self.player,
                item,
                slot: SlotIndex::new(self.hotbar_slot),
                expected_inventory_revision: inventory.revision(),
            }),
        )
        .map(Some)
    }

    /// Recipe identities whose workstation matches and whose inputs are currently owned.
    ///
    /// Workstation recipes stay empty until [`Self::bind_workstation`] (or a
    /// block-container bind) has seeded that contract. Identity order follows
    /// [`GameplayCatalog::recipes`].
    #[must_use]
    pub(super) fn craftable_recipe_ids(
        &self,
        workstation: Option<&WorkstationId>,
    ) -> Vec<RecipeId> {
        if let Some(required) = workstation
            && !self.bound_workstations.contains(required)
        {
            return Vec::new();
        }
        self.catalog()
            .recipes()
            .iter()
            .filter_map(|(id, recipe)| {
                if recipe.workstation.as_ref() != workstation {
                    return None;
                }
                match self.recipe_matches(id) {
                    Ok(()) => Some(id.clone()),
                    Err(_) => None,
                }
            })
            .collect()
    }

    pub(super) fn bind_workstation(
        &mut self,
        workstation: WorkstationId,
        owner_chunk: &DimensionChunkKey,
        entity: ContainerId,
    ) -> Result<(), GameplayReject> {
        if !self.catalog().workstations().contains(&workstation) {
            return Err(GameplayReject::UnknownReference {
                kind: "workstation",
                id: workstation.as_str().to_owned(),
            });
        }
        let slots = self
            .catalog()
            .workstation_container_slots(&workstation)
            .ok_or_else(|| GameplayReject::UnknownReference {
                kind: "workstation_schema_binding",
                id: workstation.as_str().to_owned(),
            })?;
        self.seed_container(owner_chunk, entity, Some(workstation.clone()), slots)?;
        self.bound_workstations.insert(workstation);
        Ok(())
    }

    pub(super) fn bind_block_container(
        &mut self,
        block: &BlockId,
        owner_chunk: &DimensionChunkKey,
        entity: ContainerId,
    ) -> Result<(), GameplayReject> {
        let binding = self.catalog().block_schema_binding(block).ok_or_else(|| {
            GameplayReject::UnknownReference {
                kind: "block_schema_binding",
                id: block.as_str().to_owned(),
            }
        })?;
        if !binding.realizes_container() {
            return Err(GameplayReject::InvalidSchemaBinding {
                block: block.clone(),
                reason: "block does not realize the container schema",
            });
        }
        let slots =
            binding
                .container_slot_count()
                .ok_or_else(|| GameplayReject::InvalidSchemaBinding {
                    block: block.clone(),
                    reason: "container schema requires a non-zero slot count",
                })?;
        let workstation = binding.workstation.clone();
        self.seed_container(owner_chunk, entity, workstation.clone(), slots)?;
        if let Some(workstation) = workstation {
            self.bound_workstations.insert(workstation);
        }
        Ok(())
    }

    fn seed_container(
        &mut self,
        owner_chunk: &DimensionChunkKey,
        entity: ContainerId,
        workstation: Option<WorkstationId>,
        slots: usize,
    ) -> Result<(), GameplayReject> {
        if self.applier.state().container(entity).is_some() {
            return Ok(());
        }
        let container = ContainerStateV1::empty(
            ContainerOwnerComponentV1 {
                dimension: owner_chunk.dimension.clone(),
                chunk: owner_chunk.coordinate,
                entity: entity.into_persistent_entity_id(),
            },
            workstation,
            slots,
        )?;
        self.applier.state_mut().seed_container(entity, container)
    }

    pub(super) fn seed_slot(
        &mut self,
        slot: SlotIndex,
        stack: Option<ItemStackV1>,
    ) -> Result<(), GameplayReject> {
        if let Some(stack) = &stack {
            self.catalog().validate_stack(stack)?;
        }
        self.applier
            .state_mut()
            .seed_player_slot(self.player, slot, stack)
    }

    fn selected_tool_slot(&self) -> Option<SlotIndex> {
        let inventory = self.applier.state().inventory(self.player)?;
        let slot = SlotIndex::new(self.hotbar_slot);
        let stack = inventory.slot(slot).ok().flatten()?;
        self.catalog().tool(stack.item())?;
        Some(slot)
    }

    fn placing_slot(&self, aimed: &BlockId) -> Result<SlotIndex, GameplayReject> {
        let inventory =
            self.applier
                .state()
                .inventory(self.player)
                .ok_or(GameplayReject::UnknownPlayer {
                    player: self.player.as_bytes(),
                })?;
        for (index, stack) in inventory.slots().iter().enumerate() {
            let Some(stack) = stack else {
                continue;
            };
            if self
                .catalog()
                .item(stack.item())
                .and_then(|item| item.placement_block.as_ref())
                == Some(aimed)
            {
                let slot = u16::try_from(index).map_err(|_| GameplayReject::LimitExceeded {
                    resource: "inventory_slots",
                    limit: usize::from(u16::MAX),
                    actual: index,
                })?;
                return Ok(SlotIndex::new(slot));
            }
        }
        Err(GameplayReject::EmptySlot)
    }

    fn recipe_matches(&self, recipe: &RecipeId) -> Result<(), GameplayReject> {
        let pattern = self
            .catalog()
            .recipe(recipe)
            .ok_or_else(|| GameplayReject::UnknownReference {
                kind: "recipe",
                id: recipe.as_str().to_owned(),
            })?
            .pattern
            .clone();
        match pattern {
            RecipePatternV1::Shapeless { ingredients } => self
                .shapeless_slots(recipe, ingredients.as_ref())
                .map(|_| ()),
            RecipePatternV1::Shaped { cells, .. } => {
                let ingredients: Vec<IngredientV1> =
                    cells.iter().filter_map(Clone::clone).collect();
                self.cover_ingredients(recipe, &ingredients)
            }
        }
    }

    fn cover_ingredients(
        &self,
        recipe: &RecipeId,
        ingredients: &[IngredientV1],
    ) -> Result<(), GameplayReject> {
        if ingredients.is_empty() {
            return Err(GameplayReject::RecipeMismatch {
                recipe: recipe.clone(),
            });
        }
        let inventory =
            self.applier
                .state()
                .inventory(self.player)
                .ok_or(GameplayReject::UnknownPlayer {
                    player: self.player.as_bytes(),
                })?;
        let mut available: Vec<(ItemId, u32)> = inventory
            .slots()
            .iter()
            .flatten()
            .map(|stack| (stack.item().clone(), stack.quantity()))
            .collect();
        for ingredient in ingredients {
            let mut remaining = ingredient.quantity.get();
            for (item, quantity) in &mut available {
                if remaining == 0 {
                    break;
                }
                if !self.catalog().matches(&ingredient.accepts, item) {
                    continue;
                }
                let take = remaining.min(*quantity);
                *quantity -= take;
                remaining -= take;
            }
            if remaining > 0 {
                return Err(GameplayReject::RecipeMismatch {
                    recipe: recipe.clone(),
                });
            }
        }
        Ok(())
    }

    fn placement_slot(
        &self,
        requested_block: Option<&BlockId>,
    ) -> Result<SlotIndex, GameplayReject> {
        let inventory =
            self.applier
                .state()
                .inventory(self.player)
                .ok_or(GameplayReject::UnknownPlayer {
                    player: self.player.as_bytes(),
                })?;
        if let Some(requested) = requested_block {
            for (index, stack) in inventory.slots().iter().enumerate() {
                let Some(stack) = stack else {
                    continue;
                };
                if self
                    .catalog()
                    .item(stack.item())
                    .and_then(|item| item.placement_block.as_ref())
                    == Some(requested)
                {
                    let slot = u16::try_from(index).map_err(|_| GameplayReject::LimitExceeded {
                        resource: "inventory_slots",
                        limit: usize::from(u16::MAX),
                        actual: index,
                    })?;
                    return Ok(SlotIndex::new(slot));
                }
            }
        }
        let selected = SlotIndex::new(self.hotbar_slot);
        if inventory.slot(selected)?.is_some() {
            return Ok(selected);
        }
        Err(GameplayReject::EmptySlot)
    }

    fn layout_recipe(&mut self, recipe: &RecipeId) -> Result<Box<[SlotIndex]>, GameplayReject> {
        let pattern = self
            .catalog()
            .recipe(recipe)
            .ok_or_else(|| GameplayReject::UnknownReference {
                kind: "recipe",
                id: recipe.as_str().to_owned(),
            })?
            .pattern
            .clone();
        match pattern {
            RecipePatternV1::Shapeless { ingredients } => {
                self.shapeless_slots(recipe, ingredients.as_ref())
            }
            RecipePatternV1::Shaped { cells, .. } => {
                self.layout_shaped_grid(recipe, cells.as_ref())
            }
        }
    }

    fn shapeless_slots(
        &self,
        recipe: &RecipeId,
        ingredients: &[IngredientV1],
    ) -> Result<Box<[SlotIndex]>, GameplayReject> {
        let inventory =
            self.applier
                .state()
                .inventory(self.player)
                .ok_or(GameplayReject::UnknownPlayer {
                    player: self.player.as_bytes(),
                })?;
        let mut slots = Vec::new();
        for ingredient in ingredients {
            let mut remaining = ingredient.quantity.get();
            for (index, stack) in inventory.slots().iter().enumerate() {
                if remaining == 0 {
                    break;
                }
                let Some(stack) = stack else {
                    continue;
                };
                if !self.catalog().matches(&ingredient.accepts, stack.item()) {
                    continue;
                }
                let slot = u16::try_from(index).map_err(|_| GameplayReject::LimitExceeded {
                    resource: "inventory_slots",
                    limit: usize::from(u16::MAX),
                    actual: index,
                })?;
                slots.push(SlotIndex::new(slot));
                remaining = remaining.saturating_sub(stack.quantity());
            }
            if remaining > 0 {
                return Err(GameplayReject::RecipeMismatch {
                    recipe: recipe.clone(),
                });
            }
        }
        if slots.is_empty() {
            return Err(GameplayReject::RecipeMismatch {
                recipe: recipe.clone(),
            });
        }
        Ok(slots.into_boxed_slice())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "shaped grid layout must stay one conservation-preserving flow"
    )]
    fn layout_shaped_grid(
        &mut self,
        recipe: &RecipeId,
        cells: &[Option<IngredientV1>],
    ) -> Result<Box<[SlotIndex]>, GameplayReject> {
        if cells.len() > 9 {
            return Err(GameplayReject::LimitExceeded {
                resource: "shaped_cells",
                limit: 9,
                actual: cells.len(),
            });
        }
        let catalog = self.catalog().clone();
        let player = self.player;
        let inventory = self
            .applier
            .state()
            .inventory(player)
            .ok_or(GameplayReject::UnknownPlayer {
                player: player.as_bytes(),
            })?
            .clone();
        let mut working = inventory.slots().to_vec();
        let grid_start = usize::from(CRAFTING_GRID_ORIGIN);
        for offset in 0..cells.len() {
            let grid_index = grid_start
                .checked_add(offset)
                .ok_or(GameplayReject::QuantityOverflow)?;
            let offset_u16 = u16::try_from(offset).map_err(|_| GameplayReject::QuantityOverflow)?;
            let Some(grid_slot) = working.get_mut(grid_index) else {
                return Err(GameplayReject::SlotOutOfRange {
                    slot: SlotIndex::new(CRAFTING_GRID_ORIGIN.saturating_add(offset_u16)),
                    slots: working.len(),
                });
            };
            if let Some(stack) = grid_slot.take() {
                insert_into_working(&catalog, &mut working, &stack, grid_index)?;
            }
        }
        for (offset, cell) in cells.iter().enumerate() {
            let Some(ingredient) = cell else {
                continue;
            };
            let grid_index = grid_start
                .checked_add(offset)
                .ok_or(GameplayReject::QuantityOverflow)?;
            let mut remaining = ingredient.quantity.get();
            for source in 0..working.len() {
                if remaining == 0 || source == grid_index {
                    continue;
                }
                let Some(stack) = working[source].clone() else {
                    continue;
                };
                if !catalog.matches(&ingredient.accepts, stack.item()) {
                    continue;
                }
                let take = remaining.min(stack.quantity());
                working[source] = stack.with_quantity(stack.quantity() - take)?;
                let existing = working[grid_index].clone();
                let placed = match existing {
                    None => stack
                        .with_quantity(take)?
                        .ok_or(GameplayReject::ZeroQuantity)?,
                    Some(current) if current.item() == stack.item() => current
                        .with_quantity(
                            current
                                .quantity()
                                .checked_add(take)
                                .ok_or(GameplayReject::QuantityOverflow)?,
                        )?
                        .ok_or(GameplayReject::ZeroQuantity)?,
                    Some(_) => {
                        return Err(GameplayReject::SlotMismatch);
                    }
                };
                working[grid_index] = Some(placed);
                remaining -= take;
            }
            if remaining > 0 {
                return Err(GameplayReject::RecipeMismatch {
                    recipe: recipe.clone(),
                });
            }
        }
        for (index, stack) in working.iter().enumerate() {
            let slot = u16::try_from(index).map_err(|_| GameplayReject::LimitExceeded {
                resource: "inventory_slots",
                limit: usize::from(u16::MAX),
                actual: index,
            })?;
            self.applier.state_mut().seed_player_slot(
                player,
                SlotIndex::new(slot),
                stack.clone(),
            )?;
        }
        (0..cells.len())
            .map(|offset| {
                u16::try_from(offset).map(|offset_u16| {
                    SlotIndex::new(CRAFTING_GRID_ORIGIN.saturating_add(offset_u16))
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Vec::into_boxed_slice)
            .map_err(|_| GameplayReject::QuantityOverflow)
    }
}

fn insert_into_working(
    catalog: &GameplayCatalog,
    slots: &mut [Option<ItemStackV1>],
    stack: &ItemStackV1,
    skip: usize,
) -> Result<(), GameplayReject> {
    catalog.validate_stack(stack)?;
    let definition =
        catalog
            .item(stack.item())
            .ok_or_else(|| GameplayReject::UnknownReference {
                kind: "item",
                id: stack.item().as_str().to_owned(),
            })?;
    let limit = definition.stack_limit.get();
    let mut remaining = stack.quantity();
    for (index, slot) in slots.iter_mut().enumerate() {
        if remaining == 0 || index == skip {
            continue;
        }
        if let Some(existing) = slot
            && existing.item() == stack.item()
            && existing.state() == stack.state()
        {
            let capacity = limit
                .checked_sub(existing.quantity())
                .ok_or(GameplayReject::QuantityOverflow)?;
            let add = remaining.min(capacity);
            if add == 0 {
                continue;
            }
            *existing = existing
                .with_quantity(
                    existing
                        .quantity()
                        .checked_add(add)
                        .ok_or(GameplayReject::QuantityOverflow)?,
                )?
                .ok_or(GameplayReject::ZeroQuantity)?;
            remaining -= add;
        }
    }
    for (index, slot) in slots.iter_mut().enumerate() {
        if remaining == 0 || index == skip {
            continue;
        }
        if slot.is_none() {
            *slot = stack.with_quantity(remaining)?;
            return Ok(());
        }
    }
    Err(GameplayReject::InventoryFull)
}

/// Maps a gameplay rejection onto the block-edit DTO used by the player plugin.
#[must_use]
pub(super) fn block_edit_reject(
    reject: &GameplayReject,
    required_tool: Option<&ToolClassId>,
) -> BlockEditRejectV1 {
    match reject {
        GameplayReject::ToolBroken => BlockEditRejectV1::ToolBroken,
        GameplayReject::ToolRequired
        | GameplayReject::ToolClassMismatch { .. }
        | GameplayReject::ToolTierTooLow { .. }
        | GameplayReject::NotATool { .. } => BlockEditRejectV1::RequiresTool {
            required: match reject {
                GameplayReject::ToolClassMismatch { required, .. } => required.as_ref().clone(),
                _ => required_tool.cloned().unwrap_or_else(fallback_tool_class),
            },
        },
        GameplayReject::StaleChunkRevision { expected, actual } => {
            BlockEditRejectV1::StaleRevision {
                expected: *expected,
                actual: *actual,
            }
        }
        GameplayReject::BlockOccupied => BlockEditRejectV1::NotReplaceable,
        GameplayReject::BlockMissing => BlockEditRejectV1::NotBreakable,
        GameplayReject::EmptySlot | GameplayReject::NotPlacementItem { .. } => {
            BlockEditRejectV1::NoPlacementContent
        }
        GameplayReject::ChunkNotLoaded { .. } => BlockEditRejectV1::PermissionDenied,
        GameplayReject::UnknownReference { .. } => BlockEditRejectV1::ContentUnavailable,
        _ => BlockEditRejectV1::StorageUnavailable,
    }
}

fn fallback_tool_class() -> ToolClassId {
    match ToolClassId::parse("latticeaxiom:tool-class/hand@1") {
        Ok(class) => class,
        Err(error) => {
            panic!("platform hand tool class is a canonical semantic contract: {error}")
        }
    }
}

/// Remaining work advertised when a mine command only advanced progress.
#[must_use]
pub(super) const fn remaining_work(accumulated: u32, required: u32) -> u32 {
    required.saturating_sub(accumulated)
}

#[cfg(test)]
mod tests {
    use latticeaxiom_gameplay::GameplayReject;
    use latticeaxiom_player::BlockEditRejectV1;

    use super::block_edit_reject;

    #[test]
    fn tool_broken_maps_to_the_typed_block_edit_reject() {
        assert_eq!(
            block_edit_reject(&GameplayReject::ToolBroken, None),
            BlockEditRejectV1::ToolBroken
        );
    }
}
