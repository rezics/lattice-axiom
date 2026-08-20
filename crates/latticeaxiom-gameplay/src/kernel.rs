use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    BreakProgressKey, BreakProgressV1, ChangedDomains, ChunkRevision, CommandEnvelopeV1,
    CommandOutcomeV1, CommitReceipt, ContainerId, ContinuationId, DimensionChunkKey, DropEntityId,
    DroppedItemV1, FaultInjection, FurnaceContinuationV1, GameplayCatalog, GameplayCommandV1,
    GameplayEditTarget, GameplayMutationIntentV1, GameplayPlanV1, GameplayReject,
    GameplayStorageDomain, InventoryStateV1, ItemStackV1, ItemStateV1, MineCommandV1,
    PlaceCommandV1, PlayerId, RecipeCraftCommandV1, RecipePatternV1, ReferenceGameplayState,
    RuntimePlanReceiptV1, ScheduledAdvanceCommandV1, SlotIndex, StartProcessCommandV1,
    TransactionId, TransferCommandV1, TransferDirectionV1,
};

/// Pure deterministic planner for version-one sandbox commands.
#[derive(Clone, Debug)]
pub struct GameplayKernel<'catalog> {
    catalog: &'catalog GameplayCatalog,
}

impl<'catalog> GameplayKernel<'catalog> {
    /// Creates a planner over one immutable compiled gameplay catalog.
    #[must_use]
    pub const fn new(catalog: &'catalog GameplayCatalog) -> Self {
        Self { catalog }
    }

    /// Produces a bounded runtime-staged plan without mutating loaded state.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for invalid decoded state, a stale storage
    /// revision, invalid content/mechanic input,
    /// arithmetic overflow, or a hard-limit violation.
    pub fn plan(
        &self,
        state: &ReferenceGameplayState,
        envelope: &CommandEnvelopeV1,
    ) -> Result<GameplayPlanV1, GameplayReject> {
        state.validate_loaded(self.catalog)?;
        validate_envelope(state, envelope)?;
        let (edits, outcome) = match &envelope.command {
            GameplayCommandV1::Mine(command) => self.plan_mine(state, command)?,
            GameplayCommandV1::DropItem(command) => self.plan_drop(state, command)?,
            GameplayCommandV1::Pickup(command) => self.plan_pickup(state, command)?,
            GameplayCommandV1::Place(command) => self.plan_place(state, command)?,
            GameplayCommandV1::Craft(command) => self.plan_craft(state, command)?,
            GameplayCommandV1::Transfer(command) => self.plan_transfer(state, command)?,
            GameplayCommandV1::StartProcess(command) => {
                self.plan_start_process(state, envelope.transaction_id, command)?
            }
            GameplayCommandV1::AdvanceScheduled(command) => {
                self.plan_advance_scheduled(state, command)?
            }
        };
        if edits.len() > state.limits.mutations_per_command {
            return Err(GameplayReject::LimitExceeded {
                resource: "mutations_per_command",
                limit: state.limits.mutations_per_command,
                actual: edits.len(),
            });
        }
        let envelope_fingerprint = crate::hash::envelope_hash(envelope);
        let plan_fingerprint = crate::hash::plan_hash(
            envelope.expected_world_revision,
            envelope_fingerprint,
            &edits,
            &outcome,
        );
        Ok(GameplayPlanV1 {
            transaction_id: envelope.transaction_id,
            expected_world_revision: envelope.expected_world_revision,
            envelope_fingerprint,
            edits: edits.into_boxed_slice(),
            plan_fingerprint,
            outcome,
        })
    }
    #[allow(
        clippy::too_many_lines,
        reason = "atomic mining plan stays auditable as one flow"
    )]
    fn plan_mine(
        &self,
        state: &ReferenceGameplayState,
        command: &MineCommandV1,
    ) -> Result<(Vec<GameplayMutationIntentV1>, CommandOutcomeV1), GameplayReject> {
        let inventory = player_inventory(state, command.player)?;
        let block = state
            .blocks
            .get(&command.target)
            .cloned()
            .ok_or(GameplayReject::BlockMissing)?;
        let block_definition =
            self.catalog
                .block(&block)
                .ok_or_else(|| GameplayReject::UnknownReference {
                    kind: "block",
                    id: block.as_str().to_owned(),
                })?;
        let chunk = command.target.chunk();
        let current_chunk_revision = require_chunk_revision(state, &chunk)?;
        if current_chunk_revision != command.expected_chunk_revision {
            return Err(GameplayReject::StaleChunkRevision {
                expected: command.expected_chunk_revision.get(),
                actual: current_chunk_revision.get(),
            });
        }

        let mut inventory_after = inventory.slots.to_vec();
        let work = if let Some(slot) = command.tool_slot {
            let stack = slot_ref(&inventory_after, slot)?
                .as_ref()
                .ok_or(GameplayReject::EmptySlot)?;
            let tool = self
                .catalog
                .tool(stack.item())
                .ok_or_else(|| GameplayReject::NotATool {
                    item: stack.item().clone(),
                })?;
            if let Some(requirement) = &block_definition.mining.tool {
                if tool.class != requirement.class {
                    return Err(GameplayReject::ToolClassMismatch {
                        required: Box::new(requirement.class.clone()),
                        actual: Box::new(tool.class.clone()),
                    });
                }
                if tool.tier < requirement.minimum_tier {
                    return Err(GameplayReject::ToolTierTooLow {
                        required: requirement.minimum_tier,
                        actual: tool.tier,
                    });
                }
            }
            if !matches!(stack.state(), ItemStateV1::ToolDurability { .. }) {
                return Err(GameplayReject::ToolStateMissing {
                    item: stack.item().clone(),
                });
            }
            tool.work_per_step.get()
        } else {
            if block_definition.mining.tool.is_some() {
                return Err(GameplayReject::ToolRequired);
            }
            1
        };

        let key = BreakProgressKey {
            player: command.player,
            block: command.target.clone(),
        };
        let before_progress = state.break_progress.get(&key).cloned();
        let previous_work = before_progress
            .as_ref()
            .filter(|progress| progress.block == block)
            .map_or(0, |progress| progress.accumulated_work);
        let accumulated = previous_work
            .checked_add(work)
            .ok_or(GameplayReject::QuantityOverflow)?;
        let required = block_definition.mining.hardness.get();
        if accumulated < required {
            if before_progress.is_none()
                && state.break_progress.len() >= state.limits.break_progress
            {
                return Err(GameplayReject::LimitExceeded {
                    resource: "break_progress",
                    limit: state.limits.break_progress,
                    actual: checked_capacity_plus_one(
                        state.break_progress.len(),
                        "break_progress",
                    )?,
                });
            }
            return Ok((
                vec![GameplayMutationIntentV1::BreakProgress {
                    target: inventory.target.clone(),
                    key,
                    before: before_progress,
                    after: Some(BreakProgressV1 {
                        block,
                        accumulated_work: accumulated,
                    }),
                }],
                CommandOutcomeV1::MiningProgress {
                    accumulated,
                    required,
                },
            ));
        }

        if state.drops.len() >= state.limits.drops {
            return Err(GameplayReject::LimitExceeded {
                resource: "drops",
                limit: state.limits.drops,
                actual: checked_capacity_plus_one(state.drops.len(), "drops")?,
            });
        }
        self.catalog.validate_stack(&block_definition.drop)?;
        let drop_id = command.reserved_drop;
        require_available_drop_id(state, drop_id)?;

        if let Some(tool_slot) = command.tool_slot {
            let tool = slot_ref(&inventory_after, tool_slot)?
                .as_ref()
                .ok_or(GameplayReject::EmptySlot)?
                .clone();
            inventory_after[tool_slot.as_usize()] = tool.tool_after_use()?;
        }

        let voxel_target = edit_target(chunk.clone(), GameplayStorageDomain::Voxels);
        let entity_target = edit_target(chunk.clone(), GameplayStorageDomain::PersistentEntities);
        let mut edits = inventory_diff(command.player, inventory, &inventory_after)?;
        edits.extend([
            GameplayMutationIntentV1::Block {
                target: voxel_target,
                key: command.target.clone(),
                before: Some(block),
                after: None,
            },
            GameplayMutationIntentV1::DropEntity {
                target: entity_target,
                id: drop_id,
                before: None,
                after: Some(DroppedItemV1 {
                    location: command.target.clone(),
                    stack: block_definition.drop.clone(),
                }),
            },
            GameplayMutationIntentV1::BreakProgress {
                target: inventory.target.clone(),
                key,
                before: before_progress,
                after: None,
            },
        ]);
        Ok((
            edits,
            CommandOutcomeV1::BlockBroken {
                drop: drop_id,
                affected_chunk: chunk,
            },
        ))
    }
    fn plan_drop(
        &self,
        state: &ReferenceGameplayState,
        command: &crate::DropItemCommandV1,
    ) -> Result<(Vec<GameplayMutationIntentV1>, CommandOutcomeV1), GameplayReject> {
        if state.drops.len() >= state.limits.drops {
            return Err(GameplayReject::LimitExceeded {
                resource: "drops",
                limit: state.limits.drops,
                actual: checked_capacity_plus_one(state.drops.len(), "drops")?,
            });
        }
        let chunk = command.location.chunk();
        let _ = require_chunk_revision(state, &chunk)?;
        let inventory = player_inventory(state, command.player)?;
        let mut after = inventory.slots.to_vec();
        let source = slot_ref(&after, command.slot)?
            .as_ref()
            .ok_or(GameplayReject::EmptySlot)?
            .clone();
        self.catalog.validate_stack(&source)?;
        if command.quantity.get() > source.quantity() {
            return Err(GameplayReject::SlotMismatch);
        }
        if !matches!(source.state(), ItemStateV1::Plain) && command.quantity.get() != 1 {
            return Err(GameplayReject::StatefulStackQuantity {
                quantity: command.quantity.get(),
            });
        }
        let dropped = source
            .with_quantity(command.quantity.get())?
            .ok_or(GameplayReject::ZeroQuantity)?;
        after[command.slot.as_usize()] =
            source.with_quantity(source.quantity() - command.quantity.get())?;
        let drop_id = command.reserved_drop;
        require_available_drop_id(state, drop_id)?;
        let mut edits = inventory_diff(command.player, inventory, &after)?;
        edits.push(GameplayMutationIntentV1::DropEntity {
            target: edit_target(chunk, GameplayStorageDomain::PersistentEntities),
            id: drop_id,
            before: None,
            after: Some(DroppedItemV1 {
                location: command.location.clone(),
                stack: dropped,
            }),
        });
        Ok((edits, CommandOutcomeV1::ItemDropped { drop: drop_id }))
    }
    fn plan_pickup(
        &self,
        state: &ReferenceGameplayState,
        command: &crate::PickupCommandV1,
    ) -> Result<(Vec<GameplayMutationIntentV1>, CommandOutcomeV1), GameplayReject> {
        let inventory = player_inventory(state, command.player)?;
        let drop = state
            .drops
            .get(&command.drop)
            .cloned()
            .ok_or(GameplayReject::UnknownDrop {
                drop: command.drop.as_bytes(),
            })?;
        let mut after = inventory.slots.to_vec();
        insert_stack(
            self.catalog,
            &mut after,
            &drop.stack,
            Destination::Inventory,
        )?;
        let drop_target = edit_target(
            drop.location.chunk(),
            GameplayStorageDomain::PersistentEntities,
        );
        let mut edits = inventory_diff(command.player, inventory, &after)?;
        edits.push(GameplayMutationIntentV1::DropEntity {
            target: drop_target,
            id: command.drop,
            before: Some(drop),
            after: None,
        });
        Ok((edits, CommandOutcomeV1::ItemPickedUp { drop: command.drop }))
    }

    fn plan_place(
        &self,
        state: &ReferenceGameplayState,
        command: &PlaceCommandV1,
    ) -> Result<(Vec<GameplayMutationIntentV1>, CommandOutcomeV1), GameplayReject> {
        if state.blocks.contains_key(&command.target) {
            return Err(GameplayReject::BlockOccupied);
        }
        if state.blocks.len() >= state.limits.tracked_blocks {
            return Err(GameplayReject::LimitExceeded {
                resource: "tracked_blocks",
                limit: state.limits.tracked_blocks,
                actual: checked_capacity_plus_one(state.blocks.len(), "tracked_blocks")?,
            });
        }
        let chunk = command.target.chunk();
        let current_chunk_revision = require_chunk_revision(state, &chunk)?;
        if current_chunk_revision != command.expected_chunk_revision {
            return Err(GameplayReject::StaleChunkRevision {
                expected: command.expected_chunk_revision.get(),
                actual: current_chunk_revision.get(),
            });
        }
        let inventory = player_inventory(state, command.player)?;
        let mut after = inventory.slots.to_vec();
        let source = slot_ref(&after, command.slot)?
            .as_ref()
            .ok_or(GameplayReject::EmptySlot)?
            .clone();
        let item =
            self.catalog
                .item(source.item())
                .ok_or_else(|| GameplayReject::UnknownReference {
                    kind: "item",
                    id: source.item().as_str().to_owned(),
                })?;
        let block =
            item.placement_block
                .clone()
                .ok_or_else(|| GameplayReject::NotPlacementItem {
                    item: source.item().clone(),
                })?;
        after[command.slot.as_usize()] = source.with_quantity(source.quantity() - 1)?;
        let mut edits = inventory_diff(command.player, inventory, &after)?;
        edits.push(GameplayMutationIntentV1::Block {
            target: edit_target(chunk.clone(), GameplayStorageDomain::Voxels),
            key: command.target.clone(),
            before: None,
            after: Some(block),
        });
        Ok((
            edits,
            CommandOutcomeV1::BlockPlaced {
                affected_chunk: chunk,
            },
        ))
    }
    fn plan_craft(
        &self,
        state: &ReferenceGameplayState,
        command: &RecipeCraftCommandV1,
    ) -> Result<(Vec<GameplayMutationIntentV1>, CommandOutcomeV1), GameplayReject> {
        let recipe = self.catalog.recipe(&command.recipe).ok_or_else(|| {
            GameplayReject::UnknownReference {
                kind: "recipe",
                id: command.recipe.as_str().to_owned(),
            }
        })?;
        validate_workstation(state, recipe.workstation.as_ref(), command.workstation)?;
        let inventory = player_inventory(state, command.player)?;
        let mut after = inventory.slots.to_vec();
        validate_unique_slots(&command.input_slots)?;
        match &recipe.pattern {
            RecipePatternV1::Shapeless { ingredients } => consume_shapeless(
                self.catalog,
                &mut after,
                &command.input_slots,
                ingredients,
                &command.recipe,
            )?,
            RecipePatternV1::Shaped { cells, .. } => consume_shaped(
                self.catalog,
                &mut after,
                &command.input_slots,
                cells,
                &command.recipe,
            )?,
        }
        let output = self.catalog.resolve_output(&recipe.output)?;
        insert_stack(self.catalog, &mut after, &output, Destination::Inventory)?;
        let intents = inventory_diff(command.player, inventory, &after)?;
        Ok((
            intents,
            CommandOutcomeV1::Crafted {
                recipe: command.recipe.clone(),
                output,
            },
        ))
    }

    fn plan_transfer(
        &self,
        state: &ReferenceGameplayState,
        command: &TransferCommandV1,
    ) -> Result<(Vec<GameplayMutationIntentV1>, CommandOutcomeV1), GameplayReject> {
        let inventory = player_inventory(state, command.player)?;
        let container = state
            .containers
            .get(&command.container)
            .ok_or(GameplayReject::UnknownContainer)?;
        if inventory.revision != command.expected_inventory_revision {
            return Err(GameplayReject::StaleInventoryRevision {
                expected: command.expected_inventory_revision,
                actual: inventory.revision,
            });
        }
        if container.revision != command.expected_container_revision {
            return Err(GameplayReject::StaleContainerRevision {
                expected: command.expected_container_revision,
                actual: container.revision,
            });
        }
        let mut inventory_after = inventory.slots.to_vec();
        let mut container_after = container.slots.to_vec();
        match command.direction {
            TransferDirectionV1::PlayerToContainer => move_quantity(
                self.catalog,
                &mut inventory_after,
                command.player_slot,
                &mut container_after,
                command.container_slot,
                command.quantity.get(),
                Destination::Output,
            )?,
            TransferDirectionV1::ContainerToPlayer => move_quantity(
                self.catalog,
                &mut container_after,
                command.container_slot,
                &mut inventory_after,
                command.player_slot,
                command.quantity.get(),
                Destination::Inventory,
            )?,
        }
        let mut intents = inventory_diff(command.player, inventory, &inventory_after)?;
        intents.extend(container_diff(
            command.container,
            container,
            &container_after,
        )?);
        Ok((
            intents,
            CommandOutcomeV1::Transferred {
                quantity: command.quantity.get(),
            },
        ))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "atomic process admission is kept together"
    )]
    fn plan_start_process(
        &self,
        state: &ReferenceGameplayState,
        transaction_id: TransactionId,
        command: &StartProcessCommandV1,
    ) -> Result<(Vec<GameplayMutationIntentV1>, CommandOutcomeV1), GameplayReject> {
        if command.input_slot == command.fuel_slot
            || command.input_slot == command.output_slot
            || command.fuel_slot == command.output_slot
        {
            return Err(GameplayReject::SlotMismatch);
        }
        if state.continuations.len() >= state.limits.continuations {
            return Err(GameplayReject::LimitExceeded {
                resource: "continuations",
                limit: state.limits.continuations,
                actual: checked_capacity_plus_one(state.continuations.len(), "continuations")?,
            });
        }
        if state
            .continuations
            .values()
            .any(|continuation| continuation.container == command.container)
        {
            return Err(GameplayReject::ProcessAlreadyScheduled);
        }
        let process = self.catalog.process(&command.process).ok_or_else(|| {
            GameplayReject::UnknownReference {
                kind: "process",
                id: command.process.as_str().to_owned(),
            }
        })?;
        let container = state
            .containers
            .get(&command.container)
            .ok_or(GameplayReject::UnknownContainer)?;
        if container.revision != command.expected_container_revision {
            return Err(GameplayReject::StaleContainerRevision {
                expected: command.expected_container_revision,
                actual: container.revision,
            });
        }
        if container.workstation.as_ref() != Some(&process.workstation) {
            return Err(GameplayReject::WorkstationRequired {
                required: process.workstation.clone(),
            });
        }
        let mut after = container.slots.to_vec();
        let input = slot_ref(&after, command.input_slot)?
            .as_ref()
            .ok_or(GameplayReject::EmptySlot)?
            .clone();
        if !self.catalog.matches(&process.input.accepts, input.item())
            || input.quantity() < process.input.quantity.get()
        {
            return Err(GameplayReject::SlotMismatch);
        }
        let fuel = slot_ref(&after, command.fuel_slot)?
            .as_ref()
            .ok_or(GameplayReject::EmptySlot)?
            .clone();
        let burn_ticks = self.catalog.best_fuel_ticks(fuel.item()).ok_or_else(|| {
            GameplayReject::FuelRejected {
                item: fuel.item().clone(),
            }
        })?;
        if burn_ticks < process.duration_ticks.get() {
            return Err(GameplayReject::InsufficientFuel {
                available: burn_ticks,
                required: process.duration_ticks.get(),
            });
        }
        let output = self.catalog.resolve_output(&process.output)?;
        can_merge_single_slot(
            self.catalog,
            slot_ref(&after, command.output_slot)?.as_ref(),
            &output,
            Destination::Output,
        )?;
        after[command.input_slot.as_usize()] =
            input.with_quantity(input.quantity() - process.input.quantity.get())?;
        after[command.fuel_slot.as_usize()] = fuel.with_quantity(fuel.quantity() - 1)?;

        let continuation_id = continuation_id_for(transaction_id);
        let continuation_target = edit_target(
            container.owner.chunk_key(),
            GameplayStorageDomain::Continuations,
        );
        let due_tick = command
            .started_at
            .get()
            .checked_add(u64::from(process.duration_ticks.get()))
            .map(crate::AuthorityTick::new)
            .ok_or(GameplayReject::RevisionOverflow {
                counter: "authority_tick",
            })?;
        let continuation = FurnaceContinuationV1 {
            process: command.process.clone(),
            container: command.container,
            target: continuation_target.clone(),
            output_slot: command.output_slot,
            pending_output: output,
            due_tick,
            revision: 0,
        };
        let mut intents = container_diff(command.container, container, &after)?;
        intents.push(GameplayMutationIntentV1::Continuation {
            target: continuation_target,
            id: continuation_id,
            before: None,
            after: Some(continuation),
        });
        Ok((
            intents,
            CommandOutcomeV1::ProcessScheduled {
                continuation: continuation_id,
                due_tick,
            },
        ))
    }

    fn plan_advance_scheduled(
        &self,
        state: &ReferenceGameplayState,
        command: &ScheduledAdvanceCommandV1,
    ) -> Result<(Vec<GameplayMutationIntentV1>, CommandOutcomeV1), GameplayReject> {
        let requested = usize::from(command.max_completions.get());
        if requested > state.limits.scheduled_completions {
            return Err(GameplayReject::LimitExceeded {
                resource: "scheduled_completions",
                limit: state.limits.scheduled_completions,
                actual: requested,
            });
        }
        let mut due: Vec<_> = state
            .continuations
            .iter()
            .filter(|(_, continuation)| continuation.due_tick <= command.through_tick)
            .map(|(id, continuation)| (continuation.due_tick, *id))
            .collect();
        due.sort_unstable();
        let more_due = due.len() > requested;
        due.truncate(requested);

        let mut container_after: BTreeMap<ContainerId, Vec<Option<ItemStackV1>>> = BTreeMap::new();
        for (_, id) in &due {
            let continuation =
                state
                    .continuations
                    .get(id)
                    .ok_or(GameplayReject::MutationPreconditionFailed {
                        resource: "continuation",
                    })?;
            let container = state
                .containers
                .get(&continuation.container)
                .ok_or(GameplayReject::ContinuationOwnerMissing)?;
            let slots = container_after
                .entry(continuation.container)
                .or_insert_with(|| container.slots.to_vec());
            merge_single_slot(
                self.catalog,
                slots,
                continuation.output_slot,
                &continuation.pending_output,
                Destination::Output,
            )?;
        }

        let mut intents = Vec::new();
        for (container_id, after) in container_after {
            let before = state
                .containers
                .get(&container_id)
                .ok_or(GameplayReject::ContinuationOwnerMissing)?;
            intents.extend(container_diff(container_id, before, &after)?);
        }
        for (_, id) in &due {
            let before = state.continuations.get(id).cloned();
            let target = before
                .as_ref()
                .ok_or(GameplayReject::MutationPreconditionFailed {
                    resource: "continuation",
                })?
                .target
                .clone();
            intents.push(GameplayMutationIntentV1::Continuation {
                target,
                id: *id,
                before,
                after: None,
            });
        }
        let completed = u16::try_from(due.len()).map_err(|_| GameplayReject::LimitExceeded {
            resource: "scheduled_completions",
            limit: usize::from(u16::MAX),
            actual: due.len(),
        })?;
        Ok((
            intents,
            CommandOutcomeV1::ScheduledAdvanced {
                completed,
                more_due,
            },
        ))
    }
}

/// Fault-injectable in-memory plan applier for conformance tests.
///
/// The applier is bound to one authoritative world and the exact catalog used
/// to validate its loaded state. It stages no disk/checkpoint work and publishes
/// no storage durability or revision receipt. Production hosts must capture the
/// affected chunks into a complete [`latticeaxiom_storage::WorldTransaction`]
/// after applying a plan, then reconcile its [`CommitReceipt`] here.
#[derive(Clone, Debug)]
pub struct ReferencePlanApplier<'catalog> {
    world: crate::WorldId,
    state: ReferenceGameplayState,
    kernel: GameplayKernel<'catalog>,
    pending_storage_commit: Option<PendingStorageCommit>,
}

#[derive(Clone, Debug)]
struct PendingStorageCommit {
    transaction_id: TransactionId,
    base_world_revision: crate::WorldRevision,
    chunks: BTreeMap<DimensionChunkKey, ChangedDomains>,
}

impl<'catalog> ReferencePlanApplier<'catalog> {
    /// Validates loaded state and binds it to one world and immutable catalog.
    ///
    /// # Errors
    ///
    /// Returns a loaded-state/catalog validation rejection. A handed-off state
    /// whose latest runtime plan still awaits storage capture is rejected.
    pub fn try_new(
        world: crate::WorldId,
        state: ReferenceGameplayState,
        catalog: &'catalog GameplayCatalog,
    ) -> Result<Self, GameplayReject> {
        state.validate_loaded(catalog)?;
        if let Some(pending) = &state.pending_receipt {
            return Err(GameplayReject::StorageCommitPending {
                transaction_id: *pending.transaction_id.as_bytes(),
                observed_world_revision: state.observed_world_revision.get(),
            });
        }
        Ok(Self {
            world,
            state,
            kernel: GameplayKernel::new(catalog),
            pending_storage_commit: None,
        })
    }

    /// Returns the authoritative world bound to this reference state.
    #[must_use]
    pub const fn world(&self) -> crate::WorldId {
        self.world
    }

    /// Returns immutable reference state.
    #[must_use]
    pub const fn state(&self) -> &ReferenceGameplayState {
        &self.state
    }

    /// Consumes the applier and returns state for an in-memory handoff fixture.
    ///
    /// A state containing an unacknowledged runtime receipt cannot be rebound
    /// through [`Self::try_new`].
    #[must_use]
    pub fn into_state(self) -> ReferenceGameplayState {
        self.state
    }

    /// Reconciles the authoritative storage acknowledgement for the pending plan.
    ///
    /// The receipt must name this applier's world and pending transaction, be
    /// the exact next world revision, cover every affected chunk, include every
    /// staged gameplay domain, and advance each affected chunk exactly once.
    /// Validation and retry-ledger staging complete before any live state changes.
    ///
    /// # Errors
    ///
    /// Rejects a receipt when no plan is pending or any world, transaction,
    /// revision, chunk, or changed-domain field disagrees with that plan.
    pub fn observe_storage_commit(
        &mut self,
        receipt: &CommitReceipt,
    ) -> Result<(), GameplayReject> {
        let pending = self
            .pending_storage_commit
            .as_ref()
            .ok_or(GameplayReject::StorageCommitNotPending)?;
        validate_pending_runtime_receipt(&self.state, pending)?;
        let next_world_revision =
            validate_storage_commit_header(self.world, &self.state, pending, receipt)?;
        let reconciled = reconcile_storage_chunks(self.world, &self.state, pending, receipt)?;

        let mut next_state = self.state.clone();
        for (chunk, revision) in reconciled {
            next_state.loaded_chunks.insert(chunk, revision);
        }
        next_state.observed_world_revision = next_world_revision;
        let finalized = next_state.pending_receipt.take().ok_or(
            GameplayReject::MutationPreconditionFailed {
                resource: "pending_retry_receipt",
            },
        )?;
        retain_finalized_receipt(&mut next_state, finalized)?;
        next_state.validate_loaded(self.kernel.catalog)?;

        self.state = next_state;
        self.pending_storage_commit = None;
        Ok(())
    }
    /// Plans and atomically applies one command to the in-memory reference state.
    ///
    /// Exact pending and finalized retries return their original receipt before
    /// the retired frontier, pending barrier, or current revision is considered.
    /// Injected mid-batch faults roll back every already-applied edit.
    ///
    /// # Errors
    ///
    /// Returns any loaded-state, planner, retry-window, retry-payload,
    /// precondition, fingerprint, pending-storage, or injected-fault rejection.
    pub fn execute(
        &mut self,
        envelope: &CommandEnvelopeV1,
        fault: FaultInjection,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        let fingerprint = crate::hash::envelope_hash(envelope);
        if let Some(receipt) = &self.state.pending_receipt
            && receipt.transaction_id == envelope.transaction_id
        {
            if fingerprint != receipt.envelope_fingerprint {
                return Err(GameplayReject::RetryPayloadMismatch {
                    transaction_id: *envelope.transaction_id.as_bytes(),
                });
            }
            return Ok(receipt.clone());
        }
        if let Some(receipt) = self.state.recent_receipts.get(&envelope.transaction_id) {
            if fingerprint != receipt.envelope_fingerprint {
                return Err(GameplayReject::RetryPayloadMismatch {
                    transaction_id: *envelope.transaction_id.as_bytes(),
                });
            }
            return Ok(receipt.clone());
        }
        if envelope.expected_world_revision < self.state.oldest_replayable_world_revision {
            return Err(GameplayReject::RetryWindowExpired {
                transaction_id: *envelope.transaction_id.as_bytes(),
                oldest_replayable_world_revision: self.state.oldest_replayable_world_revision.get(),
            });
        }
        if let Some(pending) = &self.pending_storage_commit {
            return Err(GameplayReject::StorageCommitPending {
                transaction_id: *pending.transaction_id.as_bytes(),
                observed_world_revision: self.state.observed_world_revision.get(),
            });
        }
        let plan = self.kernel.plan(&self.state, envelope)?;
        self.apply_plan(plan, fault)
    }

    /// Atomically applies a previously validated runtime-staged gameplay plan.
    ///
    /// # Errors
    ///
    /// Rejects a stale storage-owned world observation, command mismatch,
    /// target/precondition mismatch, fingerprint mismatch, pending storage
    /// capture, injected fault, or checked counter overflow.
    pub fn apply_plan(
        &mut self,
        plan: GameplayPlanV1,
        fault: FaultInjection,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        if let Some(receipt) = &self.state.pending_receipt
            && receipt.transaction_id == plan.transaction_id
        {
            if plan.envelope_fingerprint != receipt.envelope_fingerprint
                || plan.plan_fingerprint != receipt.plan_fingerprint
            {
                return Err(GameplayReject::RetryPayloadMismatch {
                    transaction_id: *plan.transaction_id.as_bytes(),
                });
            }
            return Ok(receipt.clone());
        }
        if let Some(receipt) = self.state.recent_receipts.get(&plan.transaction_id) {
            if plan.envelope_fingerprint != receipt.envelope_fingerprint
                || plan.plan_fingerprint != receipt.plan_fingerprint
            {
                return Err(GameplayReject::RetryPayloadMismatch {
                    transaction_id: *plan.transaction_id.as_bytes(),
                });
            }
            return Ok(receipt.clone());
        }
        if plan.expected_world_revision < self.state.oldest_replayable_world_revision {
            return Err(GameplayReject::RetryWindowExpired {
                transaction_id: *plan.transaction_id.as_bytes(),
                oldest_replayable_world_revision: self.state.oldest_replayable_world_revision.get(),
            });
        }
        if let Some(pending) = &self.pending_storage_commit {
            return Err(GameplayReject::StorageCommitPending {
                transaction_id: *pending.transaction_id.as_bytes(),
                observed_world_revision: self.state.observed_world_revision.get(),
            });
        }
        if plan.expected_world_revision != self.state.observed_world_revision {
            return Err(GameplayReject::StaleWorldRevision {
                expected: plan.expected_world_revision.get(),
                actual: self.state.observed_world_revision.get(),
            });
        }
        self.state.validate_loaded(self.kernel.catalog)?;
        if crate::hash::plan_hash(
            plan.expected_world_revision,
            plan.envelope_fingerprint,
            &plan.edits,
            &plan.outcome,
        ) != plan.plan_fingerprint
        {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "plan_fingerprint",
            });
        }
        validate_plan_targets(&self.state, &plan.edits)?;
        let mut applied = 0_usize;
        for edit in &plan.edits {
            if let Err(error) = apply_intent(&mut self.state, edit) {
                for rollback in plan.edits[..applied].iter().rev() {
                    rollback_intent(&mut self.state, rollback);
                }
                return Err(error);
            }
            applied = applied
                .checked_add(1)
                .ok_or(GameplayReject::RevisionOverflow {
                    counter: "applied_edits",
                })?;
            if let FaultInjection::AfterMutation(point) = fault
                && applied == usize::from(point.get())
            {
                for rollback in plan.edits[..applied].iter().rev() {
                    rollback_intent(&mut self.state, rollback);
                }
                return Err(GameplayReject::InjectedFault {
                    after_mutation: point.get(),
                });
            }
        }

        let chunks = storage_capture_domains(&plan.edits);
        let state_hash = self.state.canonical_hash();
        let receipt = RuntimePlanReceiptV1 {
            transaction_id: plan.transaction_id,
            observed_world_revision: plan.expected_world_revision,
            envelope_fingerprint: plan.envelope_fingerprint,
            plan_fingerprint: plan.plan_fingerprint,
            state_hash,
            outcome: plan.outcome,
        };
        if chunks.is_empty() {
            retain_finalized_receipt(&mut self.state, receipt.clone())?;
        } else {
            self.state.pending_receipt = Some(receipt.clone());
            self.pending_storage_commit = Some(PendingStorageCommit {
                transaction_id: plan.transaction_id,
                base_world_revision: plan.expected_world_revision,
                chunks,
            });
        }
        Ok(receipt)
    }
}

fn validate_pending_runtime_receipt(
    state: &ReferenceGameplayState,
    pending: &PendingStorageCommit,
) -> Result<(), GameplayReject> {
    let runtime_receipt =
        state
            .pending_receipt
            .as_ref()
            .ok_or(GameplayReject::MutationPreconditionFailed {
                resource: "pending_retry_receipt",
            })?;
    if runtime_receipt.transaction_id != pending.transaction_id
        || runtime_receipt.observed_world_revision != pending.base_world_revision
    {
        return Err(GameplayReject::MutationPreconditionFailed {
            resource: "pending_retry_receipt",
        });
    }
    Ok(())
}

fn validate_storage_commit_header(
    world: crate::WorldId,
    state: &ReferenceGameplayState,
    pending: &PendingStorageCommit,
    receipt: &CommitReceipt,
) -> Result<crate::WorldRevision, GameplayReject> {
    if receipt.world() != world {
        return Err(GameplayReject::StorageCommitMismatch { resource: "world" });
    }
    if receipt.transaction_id() != pending.transaction_id {
        return Err(GameplayReject::StorageCommitMismatch {
            resource: "transaction_id",
        });
    }
    if state.observed_world_revision != pending.base_world_revision {
        return Err(GameplayReject::StorageCommitMismatch {
            resource: "base_world_revision",
        });
    }
    let next_world_revision = pending
        .base_world_revision
        .get()
        .checked_add(1)
        .map(crate::WorldRevision::new)
        .ok_or(GameplayReject::RevisionOverflow {
            counter: "storage_world_revision",
        })?;
    if receipt.world_revision() != next_world_revision {
        return Err(GameplayReject::StorageCommitMismatch {
            resource: "world_revision",
        });
    }
    if receipt.chunks().len() != pending.chunks.len() {
        return Err(GameplayReject::StorageCommitMismatch { resource: "chunks" });
    }
    Ok(next_world_revision)
}

fn reconcile_storage_chunks(
    world: crate::WorldId,
    state: &ReferenceGameplayState,
    pending: &PendingStorageCommit,
    receipt: &CommitReceipt,
) -> Result<Vec<(DimensionChunkKey, crate::ChunkRevision)>, GameplayReject> {
    let mut reconciled = Vec::with_capacity(receipt.chunks().len());
    let mut seen = BTreeSet::new();
    for chunk_receipt in receipt.chunks() {
        let key = chunk_receipt.key();
        if key.world != world {
            return Err(GameplayReject::StorageCommitMismatch {
                resource: "chunk_world",
            });
        }
        let chunk = DimensionChunkKey::new(key.dimension.clone(), key.coordinate);
        if !seen.insert(chunk.clone()) {
            return Err(GameplayReject::StorageCommitMismatch {
                resource: "duplicate_chunk",
            });
        }
        let required_domains =
            pending
                .chunks
                .get(&chunk)
                .ok_or(GameplayReject::StorageCommitMismatch {
                    resource: "unexpected_chunk",
                })?;
        if !chunk_receipt.changed_domains().contains(*required_domains) {
            return Err(GameplayReject::StorageCommitMismatch {
                resource: "changed_domains",
            });
        }
        let previous_revision = state.loaded_chunks.get(&chunk).copied().ok_or(
            GameplayReject::StorageCommitMismatch {
                resource: "unloaded_chunk",
            },
        )?;
        let next_chunk_revision = previous_revision
            .get()
            .checked_add(1)
            .map(crate::ChunkRevision::new)
            .ok_or(GameplayReject::RevisionOverflow {
                counter: "storage_chunk_revision",
            })?;
        if chunk_receipt.chunk_revision() != next_chunk_revision {
            return Err(GameplayReject::StorageCommitMismatch {
                resource: "chunk_revision",
            });
        }
        reconciled.push((chunk, next_chunk_revision));
    }
    Ok(reconciled)
}
fn storage_capture_domains(
    edits: &[GameplayMutationIntentV1],
) -> BTreeMap<DimensionChunkKey, ChangedDomains> {
    let mut chunks: BTreeMap<DimensionChunkKey, ChangedDomains> = BTreeMap::new();
    for edit in edits {
        let target = edit.target();
        let domain = match target.domain {
            GameplayStorageDomain::Voxels => ChangedDomains::VOXELS,
            GameplayStorageDomain::PersistentEntities => ChangedDomains::PERSISTENT_ENTITIES,
            GameplayStorageDomain::Continuations => ChangedDomains::CONTINUATIONS,
        };
        chunks
            .entry(target.chunk.clone())
            .and_modify(|domains| *domains = domains.union(domain))
            .or_insert(domain);
    }
    chunks
}

fn retain_finalized_receipt(
    state: &mut ReferenceGameplayState,
    receipt: RuntimePlanReceiptV1,
) -> Result<(), GameplayReject> {
    if state.recent_receipts.contains_key(&receipt.transaction_id) {
        return Err(GameplayReject::MutationPreconditionFailed {
            resource: "retry_ledger",
        });
    }
    state.receipt_order.push_back(receipt.transaction_id);
    state
        .recent_receipts
        .insert(receipt.transaction_id, receipt);
    while state.receipt_order.len() > state.limits.recent_receipts {
        let retired_id =
            state
                .receipt_order
                .pop_front()
                .ok_or(GameplayReject::MutationPreconditionFailed {
                    resource: "retry_ledger",
                })?;
        if state.recent_receipts.remove(&retired_id).is_none() {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "retry_ledger",
            });
        }
    }
    state.oldest_replayable_world_revision = match state.receipt_order.front() {
        Some(oldest_id) => {
            state
                .recent_receipts
                .get(oldest_id)
                .ok_or(GameplayReject::MutationPreconditionFailed {
                    resource: "retry_ledger",
                })?
                .observed_world_revision
        }
        None => crate::WorldRevision::ZERO,
    };
    Ok(())
}
fn validate_envelope(
    state: &ReferenceGameplayState,
    envelope: &CommandEnvelopeV1,
) -> Result<(), GameplayReject> {
    if envelope.expected_world_revision != state.observed_world_revision {
        return Err(GameplayReject::StaleWorldRevision {
            expected: envelope.expected_world_revision.get(),
            actual: state.observed_world_revision.get(),
        });
    }
    Ok(())
}
fn require_chunk_revision(
    state: &ReferenceGameplayState,
    chunk: &DimensionChunkKey,
) -> Result<ChunkRevision, GameplayReject> {
    state
        .loaded_chunk_revision(chunk)
        .ok_or_else(|| GameplayReject::ChunkNotLoaded {
            dimension: chunk.dimension.as_str().to_owned(),
            x: chunk.coordinate.x,
            y: chunk.coordinate.y,
            z: chunk.coordinate.z,
        })
}

fn edit_target(chunk: DimensionChunkKey, domain: GameplayStorageDomain) -> GameplayEditTarget {
    GameplayEditTarget::new(chunk, domain)
}

fn require_available_drop_id(
    state: &ReferenceGameplayState,
    id: DropEntityId,
) -> Result<(), GameplayReject> {
    if state.persistent_entity_id_in_use(id.as_persistent_entity_id()) {
        Err(GameplayReject::DuplicateStateKey {
            kind: "persistent_entity",
        })
    } else {
        Ok(())
    }
}

fn continuation_id_for(transaction_id: TransactionId) -> ContinuationId {
    let mut bytes = *transaction_id.as_bytes();
    bytes[0] ^= 0xc8;
    ContinuationId::from_bytes(bytes)
}

fn checked_capacity_plus_one(
    current: usize,
    counter: &'static str,
) -> Result<usize, GameplayReject> {
    current
        .checked_add(1)
        .ok_or(GameplayReject::RevisionOverflow { counter })
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive target matcher keeps every edit variant visibly auditable"
)]
fn validate_plan_targets(
    state: &ReferenceGameplayState,
    edits: &[GameplayMutationIntentV1],
) -> Result<(), GameplayReject> {
    let mut logical_edits = BTreeSet::new();
    for edit in edits {
        let target = edit.target();
        let _ = require_chunk_revision(state, &target.chunk)?;
        let logical_key = match edit {
            GameplayMutationIntentV1::InventorySlot { player, slot, .. } => {
                let inventory = player_inventory(state, *player)?;
                require_exact_target(
                    target,
                    &inventory.target,
                    GameplayStorageDomain::PersistentEntities,
                    "inventory_target",
                )?;
                format!("inventory-slot:{:?}:{}", player.as_bytes(), slot.get())
            }
            GameplayMutationIntentV1::InventoryRevision { player, .. } => {
                let inventory = player_inventory(state, *player)?;
                require_exact_target(
                    target,
                    &inventory.target,
                    GameplayStorageDomain::PersistentEntities,
                    "inventory_target",
                )?;
                format!("inventory-revision:{:?}", player.as_bytes())
            }
            GameplayMutationIntentV1::ContainerSlot {
                container, slot, ..
            } => {
                let container_state = state
                    .containers
                    .get(container)
                    .ok_or(GameplayReject::UnknownContainer)?;
                let expected = edit_target(
                    container_state.owner.chunk_key(),
                    GameplayStorageDomain::PersistentEntities,
                );
                require_exact_target(
                    target,
                    &expected,
                    GameplayStorageDomain::PersistentEntities,
                    "container_target",
                )?;
                format!("container-slot:{:?}:{}", container.as_bytes(), slot.get())
            }
            GameplayMutationIntentV1::ContainerRevision { container, .. } => {
                let container_state = state
                    .containers
                    .get(container)
                    .ok_or(GameplayReject::UnknownContainer)?;
                let expected = edit_target(
                    container_state.owner.chunk_key(),
                    GameplayStorageDomain::PersistentEntities,
                );
                require_exact_target(
                    target,
                    &expected,
                    GameplayStorageDomain::PersistentEntities,
                    "container_target",
                )?;
                format!("container-revision:{:?}", container.as_bytes())
            }
            GameplayMutationIntentV1::Block { key, .. } => {
                let expected = edit_target(key.chunk(), GameplayStorageDomain::Voxels);
                require_exact_target(
                    target,
                    &expected,
                    GameplayStorageDomain::Voxels,
                    "block_target",
                )?;
                format!("block:{key:?}")
            }
            GameplayMutationIntentV1::DropEntity {
                id, before, after, ..
            } => {
                let location = before
                    .as_ref()
                    .map(|drop| &drop.location)
                    .or_else(|| after.as_ref().map(|drop| &drop.location))
                    .ok_or(GameplayReject::MutationPreconditionFailed {
                        resource: "drop_target",
                    })?;
                if before
                    .as_ref()
                    .zip(after.as_ref())
                    .is_some_and(|(left, right)| left.location != right.location)
                {
                    return Err(GameplayReject::MutationPreconditionFailed {
                        resource: "drop_move_requires_complete_storage_translation",
                    });
                }
                let expected =
                    edit_target(location.chunk(), GameplayStorageDomain::PersistentEntities);
                require_exact_target(
                    target,
                    &expected,
                    GameplayStorageDomain::PersistentEntities,
                    "drop_target",
                )?;
                if before.is_none() && after.is_some() {
                    require_available_drop_id(state, *id)?;
                }
                format!("drop:{:?}", id.as_bytes())
            }
            GameplayMutationIntentV1::BreakProgress { key, .. } => {
                let inventory = player_inventory(state, key.player)?;
                require_exact_target(
                    target,
                    &inventory.target,
                    GameplayStorageDomain::PersistentEntities,
                    "break_progress_target",
                )?;
                format!("break-progress:{key:?}")
            }
            GameplayMutationIntentV1::Continuation {
                id, before, after, ..
            } => {
                let continuation = before.as_ref().or(after.as_ref()).ok_or(
                    GameplayReject::MutationPreconditionFailed {
                        resource: "continuation_target",
                    },
                )?;
                if continuation.target != *target {
                    return Err(GameplayReject::MutationPreconditionFailed {
                        resource: "continuation_target",
                    });
                }
                let container = state
                    .containers
                    .get(&continuation.container)
                    .ok_or(GameplayReject::ContinuationOwnerMissing)?;
                let expected = edit_target(
                    container.owner.chunk_key(),
                    GameplayStorageDomain::Continuations,
                );
                require_exact_target(
                    target,
                    &expected,
                    GameplayStorageDomain::Continuations,
                    "continuation_target",
                )?;
                format!("continuation:{:?}", id.as_bytes())
            }
        };
        if !logical_edits.insert(logical_key) {
            return Err(GameplayReject::MutationPreconditionFailed {
                resource: "duplicate_plan_edit",
            });
        }
    }
    Ok(())
}

fn require_exact_target(
    actual: &GameplayEditTarget,
    expected: &GameplayEditTarget,
    domain: GameplayStorageDomain,
    resource: &'static str,
) -> Result<(), GameplayReject> {
    if actual.domain != domain {
        return Err(GameplayReject::InvalidStorageTarget {
            expected: domain,
            actual: actual.domain,
        });
    }
    if actual != expected {
        return Err(GameplayReject::MutationPreconditionFailed { resource });
    }
    Ok(())
}
fn player_inventory(
    state: &ReferenceGameplayState,
    player: PlayerId,
) -> Result<&InventoryStateV1, GameplayReject> {
    state
        .inventories
        .get(&player)
        .ok_or(GameplayReject::UnknownPlayer {
            player: player.as_bytes(),
        })
}

fn slot_ref<T>(slots: &[T], slot: SlotIndex) -> Result<&T, GameplayReject> {
    slots
        .get(slot.as_usize())
        .ok_or(GameplayReject::SlotOutOfRange {
            slot,
            slots: slots.len(),
        })
}

fn inventory_diff(
    player: PlayerId,
    before: &InventoryStateV1,
    after: &[Option<ItemStackV1>],
) -> Result<Vec<GameplayMutationIntentV1>, GameplayReject> {
    let mut intents = Vec::new();
    for (index, (old, new)) in before.slots.iter().zip(after).enumerate() {
        if old != new {
            let slot = u16::try_from(index).map(SlotIndex::new).map_err(|_| {
                GameplayReject::LimitExceeded {
                    resource: "inventory_slots",
                    limit: usize::from(u16::MAX),
                    actual: index,
                }
            })?;
            intents.push(GameplayMutationIntentV1::InventorySlot {
                target: before.target.clone(),
                player,
                slot,
                before: old.clone(),
                after: new.clone(),
            });
        }
    }
    if !intents.is_empty() {
        let revision = before
            .revision
            .checked_add(1)
            .ok_or(GameplayReject::RevisionOverflow {
                counter: "inventory_revision",
            })?;
        intents.push(GameplayMutationIntentV1::InventoryRevision {
            target: before.target.clone(),
            player,
            before: before.revision,
            after: revision,
        });
    }
    Ok(intents)
}

fn container_diff(
    container: ContainerId,
    before: &crate::ContainerStateV1,
    after: &[Option<ItemStackV1>],
) -> Result<Vec<GameplayMutationIntentV1>, GameplayReject> {
    let mut intents = Vec::new();
    for (index, (old, new)) in before.slots.iter().zip(after).enumerate() {
        if old != new {
            let slot = u16::try_from(index).map(SlotIndex::new).map_err(|_| {
                GameplayReject::LimitExceeded {
                    resource: "container_slots",
                    limit: usize::from(u16::MAX),
                    actual: index,
                }
            })?;
            intents.push(GameplayMutationIntentV1::ContainerSlot {
                target: edit_target(
                    before.owner.chunk_key(),
                    GameplayStorageDomain::PersistentEntities,
                ),
                container,
                slot,
                before: old.clone(),
                after: new.clone(),
            });
        }
    }
    if !intents.is_empty() {
        let revision = before
            .revision
            .checked_add(1)
            .ok_or(GameplayReject::RevisionOverflow {
                counter: "container_revision",
            })?;
        intents.push(GameplayMutationIntentV1::ContainerRevision {
            target: edit_target(
                before.owner.chunk_key(),
                GameplayStorageDomain::PersistentEntities,
            ),
            container,
            before: before.revision,
            after: revision,
        });
    }
    Ok(intents)
}

#[derive(Clone, Copy)]
enum Destination {
    Inventory,
    Output,
}

fn insert_stack(
    catalog: &GameplayCatalog,
    slots: &mut [Option<ItemStackV1>],
    stack: &ItemStackV1,
    destination: Destination,
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
    let total_capacity = slots.iter().try_fold(0_u64, |total, slot| {
        let capacity = match slot {
            Some(existing) if can_stack(existing, stack) => limit
                .checked_sub(existing.quantity())
                .ok_or(GameplayReject::QuantityOverflow)?,
            None => limit,
            Some(_) => 0,
        };
        total
            .checked_add(u64::from(capacity))
            .ok_or(GameplayReject::QuantityOverflow)
    })?;
    if total_capacity < u64::from(remaining) {
        return Err(match destination {
            Destination::Inventory => GameplayReject::InventoryFull,
            Destination::Output => GameplayReject::OutputFull,
        });
    }
    for slot in slots.iter_mut() {
        if remaining == 0 {
            break;
        }
        if let Some(existing) = slot
            && can_stack(existing, stack)
        {
            let capacity = limit
                .checked_sub(existing.quantity())
                .ok_or(GameplayReject::QuantityOverflow)?;
            let add = remaining.min(capacity);
            *existing = existing
                .with_quantity(
                    existing
                        .quantity()
                        .checked_add(add)
                        .ok_or(GameplayReject::QuantityOverflow)?,
                )?
                .ok_or(GameplayReject::QuantityOverflow)?;
            remaining = remaining
                .checked_sub(add)
                .ok_or(GameplayReject::QuantityOverflow)?;
        }
    }
    for slot in slots.iter_mut() {
        if remaining == 0 {
            break;
        }
        if slot.is_none() {
            let add = remaining.min(limit);
            *slot = stack.with_quantity(add)?;
            remaining = remaining
                .checked_sub(add)
                .ok_or(GameplayReject::QuantityOverflow)?;
        }
    }
    if remaining != 0 {
        return Err(GameplayReject::QuantityOverflow);
    }
    Ok(())
}

fn can_stack(left: &ItemStackV1, right: &ItemStackV1) -> bool {
    left.item() == right.item()
        && left.state() == right.state()
        && matches!(left.state(), ItemStateV1::Plain)
}

fn can_merge_single_slot(
    catalog: &GameplayCatalog,
    existing: Option<&ItemStackV1>,
    incoming: &ItemStackV1,
    destination: Destination,
) -> Result<(), GameplayReject> {
    catalog.validate_stack(incoming)?;
    let definition =
        catalog
            .item(incoming.item())
            .ok_or_else(|| GameplayReject::UnknownReference {
                kind: "item",
                id: incoming.item().as_str().to_owned(),
            })?;
    let accepted = match existing {
        None => incoming.quantity() <= definition.stack_limit.get(),
        Some(existing) if can_stack(existing, incoming) => existing
            .quantity()
            .checked_add(incoming.quantity())
            .is_some_and(|sum| sum <= definition.stack_limit.get()),
        Some(_) => false,
    };
    if !accepted {
        return Err(match destination {
            Destination::Inventory => GameplayReject::InventoryFull,
            Destination::Output => GameplayReject::OutputFull,
        });
    }
    Ok(())
}

fn merge_single_slot(
    catalog: &GameplayCatalog,
    slots: &mut [Option<ItemStackV1>],
    slot: SlotIndex,
    incoming: &ItemStackV1,
    destination: Destination,
) -> Result<(), GameplayReject> {
    let existing = slot_ref(slots, slot)?.as_ref();
    can_merge_single_slot(catalog, existing, incoming, destination)?;
    let replacement = match existing {
        Some(existing) => existing
            .with_quantity(
                existing
                    .quantity()
                    .checked_add(incoming.quantity())
                    .ok_or(GameplayReject::QuantityOverflow)?,
            )?
            .ok_or(GameplayReject::QuantityOverflow)?,
        None => incoming.clone(),
    };
    slots[slot.as_usize()] = Some(replacement);
    Ok(())
}

fn move_quantity(
    catalog: &GameplayCatalog,
    source_slots: &mut [Option<ItemStackV1>],
    source_slot: SlotIndex,
    target_slots: &mut [Option<ItemStackV1>],
    target_slot: SlotIndex,
    quantity: u32,
    destination: Destination,
) -> Result<(), GameplayReject> {
    let source = slot_ref(source_slots, source_slot)?
        .as_ref()
        .ok_or(GameplayReject::EmptySlot)?
        .clone();
    if quantity > source.quantity() {
        return Err(GameplayReject::SlotMismatch);
    }
    if !matches!(source.state(), ItemStateV1::Plain) && quantity != 1 {
        return Err(GameplayReject::StatefulStackQuantity { quantity });
    }
    let moved = source
        .with_quantity(quantity)?
        .ok_or(GameplayReject::ZeroQuantity)?;
    can_merge_single_slot(
        catalog,
        slot_ref(target_slots, target_slot)?.as_ref(),
        &moved,
        destination,
    )?;
    source_slots[source_slot.as_usize()] = source.with_quantity(source.quantity() - quantity)?;
    merge_single_slot(catalog, target_slots, target_slot, &moved, destination)
}

fn validate_workstation(
    state: &ReferenceGameplayState,
    required: Option<&crate::WorkstationId>,
    supplied: Option<ContainerId>,
) -> Result<(), GameplayReject> {
    let Some(required) = required else {
        return Ok(());
    };
    let supplied = supplied.ok_or_else(|| GameplayReject::WorkstationRequired {
        required: required.clone(),
    })?;
    let container = state
        .containers
        .get(&supplied)
        .ok_or(GameplayReject::UnknownContainer)?;
    if container.workstation.as_ref() != Some(required) {
        return Err(GameplayReject::WorkstationRequired {
            required: required.clone(),
        });
    }
    Ok(())
}

fn validate_unique_slots(slots: &[SlotIndex]) -> Result<(), GameplayReject> {
    let mut unique = BTreeSet::new();
    if slots.iter().any(|slot| !unique.insert(*slot)) {
        return Err(GameplayReject::InvalidRecipe {
            reason: "recipe input slots must be unique",
        });
    }
    Ok(())
}

fn consume_shaped(
    catalog: &GameplayCatalog,
    slots: &mut [Option<ItemStackV1>],
    input_slots: &[SlotIndex],
    cells: &[Option<crate::IngredientV1>],
    recipe: &crate::RecipeId,
) -> Result<(), GameplayReject> {
    if input_slots.len() != cells.len() {
        return Err(GameplayReject::RecipeMismatch {
            recipe: recipe.clone(),
        });
    }
    for (slot, cell) in input_slots.iter().copied().zip(cells) {
        let current = slot_ref(slots, slot)?.clone();
        match (cell, current) {
            (None, None) => {}
            (None, Some(_)) | (Some(_), None) => {
                return Err(GameplayReject::RecipeMismatch {
                    recipe: recipe.clone(),
                });
            }
            (Some(ingredient), Some(stack)) => {
                if !catalog.matches(&ingredient.accepts, stack.item())
                    || stack.quantity() < ingredient.quantity.get()
                {
                    return Err(GameplayReject::RecipeMismatch {
                        recipe: recipe.clone(),
                    });
                }
                slots[slot.as_usize()] =
                    stack.with_quantity(stack.quantity() - ingredient.quantity.get())?;
            }
        }
    }
    Ok(())
}

fn consume_shapeless(
    catalog: &GameplayCatalog,
    slots: &mut [Option<ItemStackV1>],
    input_slots: &[SlotIndex],
    ingredients: &[crate::IngredientV1],
    recipe: &crate::RecipeId,
) -> Result<(), GameplayReject> {
    for slot in input_slots {
        let _present = slot_ref(slots, *slot)?;
    }
    let mut flow = IngredientFlow::new(ingredients.len(), input_slots.len())?;
    for (ingredient_index, ingredient) in ingredients.iter().enumerate() {
        flow.set_demand(ingredient_index, ingredient.quantity.get())?;
        for (slot_index, slot) in input_slots.iter().enumerate() {
            if let Some(stack) = slot_ref(slots, *slot)?.as_ref()
                && catalog.matches(&ingredient.accepts, stack.item())
            {
                flow.connect(ingredient_index, slot_index)?;
            }
        }
    }
    for (slot_index, slot) in input_slots.iter().enumerate() {
        let capacity = slot_ref(slots, *slot)?
            .as_ref()
            .map_or(0, ItemStackV1::quantity);
        flow.set_slot_capacity(slot_index, capacity)?;
    }
    let allocation = flow
        .solve()?
        .ok_or_else(|| GameplayReject::RecipeMismatch {
            recipe: recipe.clone(),
        })?;
    for (slot_index, consumed) in allocation.into_iter().enumerate() {
        if consumed == 0 {
            continue;
        }
        let slot = input_slots[slot_index];
        let stack = slot_ref(slots, slot)?
            .as_ref()
            .ok_or(GameplayReject::RecipeMismatch {
                recipe: recipe.clone(),
            })?
            .clone();
        slots[slot.as_usize()] = stack.with_quantity(stack.quantity() - consumed)?;
    }
    Ok(())
}

/// Deterministic bounded Edmonds-Karp network for overlapping recipe inputs.
struct IngredientFlow {
    ingredients: usize,
    slots: usize,
    sink: usize,
    capacities: Vec<Vec<u64>>,
    total_demand: u64,
}

impl IngredientFlow {
    const ABSOLUTE_MAX_NODES: usize = 514;

    fn new(ingredients: usize, slots: usize) -> Result<Self, GameplayReject> {
        let nodes = ingredients
            .checked_add(slots)
            .and_then(|value| value.checked_add(2))
            .ok_or(GameplayReject::QuantityOverflow)?;
        if nodes > Self::ABSOLUTE_MAX_NODES {
            return Err(GameplayReject::LimitExceeded {
                resource: "ingredient_flow_nodes",
                limit: Self::ABSOLUTE_MAX_NODES,
                actual: nodes,
            });
        }
        let _matrix_cells = nodes
            .checked_mul(nodes)
            .ok_or(GameplayReject::QuantityOverflow)?;
        let sink = nodes
            .checked_sub(1)
            .ok_or(GameplayReject::QuantityOverflow)?;
        Ok(Self {
            ingredients,
            slots,
            sink,
            capacities: vec![vec![0; nodes]; nodes],
            total_demand: 0,
        })
    }

    fn set_demand(&mut self, ingredient: usize, quantity: u32) -> Result<(), GameplayReject> {
        let node = ingredient
            .checked_add(1)
            .ok_or(GameplayReject::QuantityOverflow)?;
        let capacity = self
            .capacities
            .first_mut()
            .and_then(|row| row.get_mut(node))
            .ok_or(GameplayReject::MutationPreconditionFailed {
                resource: "ingredient_flow_index",
            })?;
        *capacity = u64::from(quantity);
        self.total_demand = self
            .total_demand
            .checked_add(u64::from(quantity))
            .ok_or(GameplayReject::QuantityOverflow)?;
        Ok(())
    }

    fn connect(&mut self, ingredient: usize, slot: usize) -> Result<(), GameplayReject> {
        let ingredient_node = ingredient
            .checked_add(1)
            .ok_or(GameplayReject::QuantityOverflow)?;
        let slot_node = self
            .ingredients
            .checked_add(slot)
            .and_then(|value| value.checked_add(1))
            .ok_or(GameplayReject::QuantityOverflow)?;
        let capacity = self
            .capacities
            .get_mut(ingredient_node)
            .and_then(|row| row.get_mut(slot_node))
            .ok_or(GameplayReject::MutationPreconditionFailed {
                resource: "ingredient_flow_index",
            })?;
        *capacity = self.total_demand.max(1);
        Ok(())
    }

    fn set_slot_capacity(&mut self, slot: usize, quantity: u32) -> Result<(), GameplayReject> {
        let node = self
            .ingredients
            .checked_add(slot)
            .and_then(|value| value.checked_add(1))
            .ok_or(GameplayReject::QuantityOverflow)?;
        let capacity = self
            .capacities
            .get_mut(node)
            .and_then(|row| row.get_mut(self.sink))
            .ok_or(GameplayReject::MutationPreconditionFailed {
                resource: "ingredient_flow_index",
            })?;
        *capacity = u64::from(quantity);
        Ok(())
    }

    fn solve(mut self) -> Result<Option<Vec<u32>>, GameplayReject> {
        let node_count = self.capacities.len();
        let mut total_flow = 0_u64;
        loop {
            let mut parent = vec![usize::MAX; node_count];
            parent[0] = 0;
            let mut queue = VecDeque::from([0_usize]);
            while let Some(node) = queue.pop_front() {
                for (next, capacity) in self.capacities[node].iter().copied().enumerate() {
                    if capacity > 0 && parent[next] == usize::MAX {
                        parent[next] = node;
                        queue.push_back(next);
                    }
                }
            }
            if parent[self.sink] == usize::MAX {
                break;
            }
            let mut augment = u64::MAX;
            let mut node = self.sink;
            while node != 0 {
                let previous = parent[node];
                augment = augment.min(self.capacities[previous][node]);
                node = previous;
            }
            node = self.sink;
            while node != 0 {
                let previous = parent[node];
                self.capacities[previous][node] = self.capacities[previous][node]
                    .checked_sub(augment)
                    .ok_or(GameplayReject::QuantityOverflow)?;
                self.capacities[node][previous] = self.capacities[node][previous]
                    .checked_add(augment)
                    .ok_or(GameplayReject::QuantityOverflow)?;
                node = previous;
            }
            total_flow = total_flow
                .checked_add(augment)
                .ok_or(GameplayReject::QuantityOverflow)?;
        }
        if total_flow != self.total_demand {
            return Ok(None);
        }
        let mut allocation = Vec::with_capacity(self.slots);
        for slot in 0..self.slots {
            let node = self
                .ingredients
                .checked_add(slot)
                .and_then(|value| value.checked_add(1))
                .ok_or(GameplayReject::QuantityOverflow)?;
            let mut consumed = 0_u64;
            for ingredient in 0..self.ingredients {
                let ingredient_node = ingredient
                    .checked_add(1)
                    .ok_or(GameplayReject::QuantityOverflow)?;
                consumed = consumed
                    .checked_add(self.capacities[node][ingredient_node])
                    .ok_or(GameplayReject::QuantityOverflow)?;
            }
            allocation.push(u32::try_from(consumed).map_err(|_| GameplayReject::QuantityOverflow)?);
        }
        Ok(Some(allocation))
    }
}
#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive apply matcher keeps rollback preconditions visibly symmetric"
)]
fn apply_intent(
    state: &mut ReferenceGameplayState,
    intent: &GameplayMutationIntentV1,
) -> Result<(), GameplayReject> {
    match intent {
        GameplayMutationIntentV1::InventorySlot {
            player,
            slot,
            before,
            after,
            ..
        } => {
            let inventory =
                state
                    .inventories
                    .get_mut(player)
                    .ok_or(GameplayReject::UnknownPlayer {
                        player: player.as_bytes(),
                    })?;
            let slots = inventory.slots.len();
            let slot_value = inventory
                .slots
                .get_mut(slot.as_usize())
                .ok_or(GameplayReject::SlotOutOfRange { slot: *slot, slots })?;
            require_before(slot_value, before, "inventory_slot")?;
            slot_value.clone_from(after);
        }
        GameplayMutationIntentV1::InventoryRevision {
            player,
            before,
            after,
            ..
        } => {
            let inventory =
                state
                    .inventories
                    .get_mut(player)
                    .ok_or(GameplayReject::UnknownPlayer {
                        player: player.as_bytes(),
                    })?;
            if inventory.revision != *before {
                return Err(GameplayReject::MutationPreconditionFailed {
                    resource: "inventory_revision",
                });
            }
            inventory.revision = *after;
        }
        GameplayMutationIntentV1::ContainerSlot {
            container,
            slot,
            before,
            after,
            ..
        } => {
            let container = state
                .containers
                .get_mut(container)
                .ok_or(GameplayReject::UnknownContainer)?;
            let slots = container.slots.len();
            let slot_value = container
                .slots
                .get_mut(slot.as_usize())
                .ok_or(GameplayReject::SlotOutOfRange { slot: *slot, slots })?;
            require_before(slot_value, before, "container_slot")?;
            slot_value.clone_from(after);
        }
        GameplayMutationIntentV1::ContainerRevision {
            container,
            before,
            after,
            ..
        } => {
            let container = state
                .containers
                .get_mut(container)
                .ok_or(GameplayReject::UnknownContainer)?;
            if container.revision != *before {
                return Err(GameplayReject::MutationPreconditionFailed {
                    resource: "container_revision",
                });
            }
            container.revision = *after;
        }
        GameplayMutationIntentV1::Block {
            key, before, after, ..
        } => {
            require_before(&state.blocks.get(key).cloned(), before, "block")?;
            replace_map(&mut state.blocks, key.clone(), after.clone());
        }
        GameplayMutationIntentV1::DropEntity {
            id, before, after, ..
        } => {
            require_before(&state.drops.get(id).cloned(), before, "drop_entity")?;
            replace_map(&mut state.drops, *id, after.clone());
        }
        GameplayMutationIntentV1::BreakProgress {
            key, before, after, ..
        } => {
            require_before(
                &state.break_progress.get(key).cloned(),
                before,
                "break_progress",
            )?;
            replace_map(&mut state.break_progress, key.clone(), after.clone());
        }
        GameplayMutationIntentV1::Continuation {
            id, before, after, ..
        } => {
            require_before(
                &state.continuations.get(id).cloned(),
                before,
                "continuation",
            )?;
            replace_map(&mut state.continuations, *id, after.clone());
        }
    }
    Ok(())
}
fn require_before<T: Eq>(
    actual: &T,
    expected: &T,
    resource: &'static str,
) -> Result<(), GameplayReject> {
    if actual != expected {
        return Err(GameplayReject::MutationPreconditionFailed { resource });
    }
    Ok(())
}

fn replace_map<K: Ord, V>(map: &mut BTreeMap<K, V>, key: K, value: Option<V>) {
    match value {
        Some(value) => {
            map.insert(key, value);
        }
        None => {
            map.remove(&key);
        }
    }
}

fn rollback_intent(state: &mut ReferenceGameplayState, intent: &GameplayMutationIntentV1) {
    match intent {
        GameplayMutationIntentV1::InventorySlot {
            player,
            slot,
            before,
            ..
        } => {
            if let Some(inventory) = state.inventories.get_mut(player)
                && let Some(slot_value) = inventory.slots.get_mut(slot.as_usize())
            {
                slot_value.clone_from(before);
            }
        }
        GameplayMutationIntentV1::InventoryRevision { player, before, .. } => {
            if let Some(inventory) = state.inventories.get_mut(player) {
                inventory.revision = *before;
            }
        }
        GameplayMutationIntentV1::ContainerSlot {
            container,
            slot,
            before,
            ..
        } => {
            if let Some(container) = state.containers.get_mut(container)
                && let Some(slot_value) = container.slots.get_mut(slot.as_usize())
            {
                slot_value.clone_from(before);
            }
        }
        GameplayMutationIntentV1::ContainerRevision {
            container, before, ..
        } => {
            if let Some(container) = state.containers.get_mut(container) {
                container.revision = *before;
            }
        }
        GameplayMutationIntentV1::Block { key, before, .. } => {
            replace_map(&mut state.blocks, key.clone(), before.clone());
        }
        GameplayMutationIntentV1::DropEntity { id, before, .. } => {
            replace_map(&mut state.drops, *id, before.clone());
        }
        GameplayMutationIntentV1::BreakProgress { key, before, .. } => {
            replace_map(&mut state.break_progress, key.clone(), before.clone());
        }
        GameplayMutationIntentV1::Continuation { id, before, .. } => {
            replace_map(&mut state.continuations, *id, before.clone());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::{IngredientFlow, validate_plan_targets};
    use crate::{
        BlockId, BlockKey, BlockPosition, ChunkCoordinate, ChunkRevision, DimensionChunkKey,
        DimensionId, GameplayEditTarget, GameplayLimits, GameplayMutationIntentV1, GameplayReject,
        GameplayStorageDomain, InventoryStateV1, PlayerId, ReferenceGameplayState,
    };

    #[test]
    fn plan_target_validation_rejects_wrong_domains_keys_and_duplicates() {
        let dimension = "example:dimension/target-test"
            .parse::<DimensionId>()
            .expect("fixture dimension is canonical");
        let chunk = DimensionChunkKey::new(dimension.clone(), ChunkCoordinate::new(0, 0, 0));
        let inventory_target =
            GameplayEditTarget::new(chunk.clone(), GameplayStorageDomain::PersistentEntities);
        let mut state = ReferenceGameplayState::new(GameplayLimits::default())
            .expect("fixture limits are valid");
        state
            .seed_loaded_chunk(chunk.clone(), ChunkRevision::ZERO)
            .expect("fixture chunk is unique");
        state
            .seed_player(
                PlayerId::new(1),
                InventoryStateV1::empty(inventory_target.clone(), 1)
                    .expect("fixture inventory is bounded"),
            )
            .expect("fixture player is unique");

        let wrong_domain = GameplayMutationIntentV1::InventoryRevision {
            target: GameplayEditTarget::new(chunk.clone(), GameplayStorageDomain::Voxels),
            player: PlayerId::new(1),
            before: 0,
            after: 1,
        };
        assert!(matches!(
            validate_plan_targets(&state, &[wrong_domain]),
            Err(GameplayReject::InvalidStorageTarget { .. })
        ));

        let wrong_key = GameplayMutationIntentV1::Block {
            target: GameplayEditTarget::new(chunk, GameplayStorageDomain::Voxels),
            key: BlockKey::new(
                "other:dimension/target-test"
                    .parse::<DimensionId>()
                    .expect("fixture dimension is canonical"),
                BlockPosition { x: 0, y: 0, z: 0 },
            ),
            before: None,
            after: Some(
                BlockId::parse("example:block/target-test").expect("fixture block ID is canonical"),
            ),
        };
        assert!(matches!(
            validate_plan_targets(&state, &[wrong_key]),
            Err(GameplayReject::MutationPreconditionFailed {
                resource: "block_target"
            })
        ));

        let duplicate = GameplayMutationIntentV1::InventoryRevision {
            target: inventory_target,
            player: PlayerId::new(1),
            before: 0,
            after: 1,
        };
        assert!(matches!(
            validate_plan_targets(&state, &[duplicate.clone(), duplicate]),
            Err(GameplayReject::MutationPreconditionFailed {
                resource: "duplicate_plan_edit"
            })
        ));
    }

    #[test]
    fn flow_finds_non_greedy_overlapping_assignment() {
        let mut flow = IngredientFlow::new(2, 2).expect("fixture flow is within hard limits");
        flow.set_demand(0, 1).expect("ingredient zero exists");
        flow.set_demand(1, 1).expect("ingredient one exists");
        flow.connect(0, 0).expect("fixture edge is in range");
        flow.connect(0, 1).expect("fixture edge is in range");
        flow.connect(1, 0).expect("fixture edge is in range");
        flow.set_slot_capacity(0, 1).expect("fixture slot exists");
        flow.set_slot_capacity(1, 1).expect("fixture slot exists");
        assert_eq!(
            flow.solve().expect("fixture arithmetic is bounded"),
            Some(vec![1, 1])
        );
    }
}
