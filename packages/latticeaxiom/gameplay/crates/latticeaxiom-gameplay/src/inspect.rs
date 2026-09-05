//! Typed inspect fragments for inventory, recipes, machines, and containers.
//!
//! Fragments are authoritative projections. They contain no presentation
//! strings, screen coordinates, or client-owned overlay state. Headless
//! omission of a presentation package must not change these values.

use crate::{
    BlockId, ContainerId, ContinuationId, FurnaceContinuationV1, GameplayCatalog, GameplayKernel,
    GameplayReject, InventoryStateV1, ItemStackV1, PlayerId, ProcessId, RecipeDefinitionV1,
    RecipeId, RecipePatternV1, ReferenceGameplayState, SlotIndex, ToolClassId, WorkstationId,
};

/// Harvestability fragment sourced from a compiled [`crate::MiningRuleV1`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MiningInspectV1 {
    hardness_ticks: u32,
    tool_class: Option<ToolClassId>,
    minimum_tier: Option<u8>,
}

impl MiningInspectV1 {
    /// Deterministic mining hardness in work units.
    #[must_use]
    pub const fn hardness_ticks(&self) -> u32 {
        self.hardness_ticks
    }

    /// Required versioned tool class, when a tool is required.
    #[must_use]
    pub const fn tool_class(&self) -> Option<&ToolClassId> {
        self.tool_class.as_ref()
    }

    /// Inclusive minimum tool tier, when a tool is required.
    #[must_use]
    pub const fn minimum_tier(&self) -> Option<u8> {
        self.minimum_tier
    }
}

/// Inventory inspect fragment. The hotbar is the selected prefix of the same container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryInspectV1 {
    player: PlayerId,
    revision: u64,
    slot_count: u16,
    hotbar_slots: u16,
    selected_hotbar: u16,
    occupied_slots: u16,
    total_quantity: u64,
}

impl InventoryInspectV1 {
    /// Player owning the inventory.
    #[must_use]
    pub const fn player(&self) -> PlayerId {
        self.player
    }

    /// Authoritative inventory revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Total slot count.
    #[must_use]
    pub const fn slot_count(&self) -> u16 {
        self.slot_count
    }

    /// Hotbar prefix length.
    #[must_use]
    pub const fn hotbar_slots(&self) -> u16 {
        self.hotbar_slots
    }

    /// Selected hotbar index.
    #[must_use]
    pub const fn selected_hotbar(&self) -> SlotIndex {
        SlotIndex::new(self.selected_hotbar)
    }

    /// Number of occupied slots.
    #[must_use]
    pub const fn occupied_slots(&self) -> u16 {
        self.occupied_slots
    }

    /// Sum of stack quantities across every slot.
    #[must_use]
    pub const fn total_quantity(&self) -> u64 {
        self.total_quantity
    }
}

/// Persistent container inspect fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerInspectV1 {
    container: ContainerId,
    revision: u64,
    workstation: Option<WorkstationId>,
    slot_count: u16,
    occupied_slots: u16,
    total_quantity: u64,
}

impl ContainerInspectV1 {
    /// Persistent container identity.
    #[must_use]
    pub const fn container(&self) -> ContainerId {
        self.container
    }

    /// Authoritative container revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Bound workstation contract, if any.
    #[must_use]
    pub const fn workstation(&self) -> Option<&WorkstationId> {
        self.workstation.as_ref()
    }

    /// Total slot count.
    #[must_use]
    pub const fn slot_count(&self) -> u16 {
        self.slot_count
    }

    /// Number of occupied slots.
    #[must_use]
    pub const fn occupied_slots(&self) -> u16 {
        self.occupied_slots
    }

    /// Sum of stack quantities across every slot.
    #[must_use]
    pub const fn total_quantity(&self) -> u64 {
        self.total_quantity
    }
}

/// Recipe inspect fragment. `craftable` means currently owned inputs, not a laid-out grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeInspectV1 {
    recipe: RecipeId,
    workstation: Option<WorkstationId>,
    craftable: bool,
}

impl RecipeInspectV1 {
    /// Versioned recipe identity.
    #[must_use]
    pub fn recipe(&self) -> &RecipeId {
        &self.recipe
    }

    /// Required workstation contract, if any.
    #[must_use]
    pub const fn workstation(&self) -> Option<&WorkstationId> {
        self.workstation.as_ref()
    }

    /// Whether the inspected inventory currently owns matching inputs.
    #[must_use]
    pub const fn craftable(&self) -> bool {
        self.craftable
    }
}

/// Furnace/machine inspect fragment for one scheduled continuation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FurnaceInspectV1 {
    container: ContainerId,
    continuation: ContinuationId,
    process: ProcessId,
    due_tick: u64,
    pending_output: ItemStackV1,
}

impl FurnaceInspectV1 {
    /// Persistent machine container.
    #[must_use]
    pub const fn container(&self) -> ContainerId {
        self.container
    }

    /// Canonical continuation identity.
    #[must_use]
    pub const fn continuation(&self) -> ContinuationId {
        self.continuation
    }

    /// Versioned process contract.
    #[must_use]
    pub fn process(&self) -> &ProcessId {
        &self.process
    }

    /// Authoritative tick at which completion becomes eligible.
    #[must_use]
    pub const fn due_tick(&self) -> u64 {
        self.due_tick
    }

    /// Concrete output frozen when input and fuel were consumed.
    #[must_use]
    pub const fn pending_output(&self) -> &ItemStackV1 {
        &self.pending_output
    }
}

impl GameplayCatalog {
    /// Projects harvestability for one exact block.
    #[must_use]
    pub fn mining_inspect(&self, block: &BlockId) -> Option<MiningInspectV1> {
        let definition = self.block(block)?;
        Some(MiningInspectV1 {
            hardness_ticks: definition.mining.hardness.get(),
            tool_class: definition
                .mining
                .tool
                .as_ref()
                .map(|requirement| requirement.class.clone()),
            minimum_tier: definition
                .mining
                .tool
                .as_ref()
                .map(|requirement| requirement.minimum_tier),
        })
    }
}

impl GameplayKernel<'_> {
    /// Projects one player inventory inspect fragment.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject::UnknownPlayer`] when the inventory is absent.
    #[allow(
        clippy::unused_self,
        reason = "inspect projections share the catalog-bound kernel surface"
    )]
    pub fn inspect_inventory(
        &self,
        state: &ReferenceGameplayState,
        player: PlayerId,
    ) -> Result<InventoryInspectV1, GameplayReject> {
        let inventory = state
            .inventory(player)
            .ok_or(GameplayReject::UnknownPlayer {
                player: player.as_bytes(),
            })?;
        inventory_inspect(player, inventory)
    }

    /// Projects one persistent container inspect fragment.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject::UnknownContainer`] when the container is absent.
    #[allow(
        clippy::unused_self,
        reason = "inspect projections share the catalog-bound kernel surface"
    )]
    pub fn inspect_container(
        &self,
        state: &ReferenceGameplayState,
        container: ContainerId,
    ) -> Result<ContainerInspectV1, GameplayReject> {
        let container_state = state
            .container(container)
            .ok_or(GameplayReject::UnknownContainer)?;
        container_inspect(container, container_state)
    }

    /// Projects the scheduled furnace continuation for one container, if any.
    #[must_use]
    #[allow(
        clippy::unused_self,
        reason = "inspect projections share the catalog-bound kernel surface"
    )]
    pub fn inspect_furnace(
        &self,
        state: &ReferenceGameplayState,
        container: ContainerId,
    ) -> Option<FurnaceInspectV1> {
        state
            .continuations()
            .iter()
            .find(|(_, continuation)| continuation.container == container)
            .map(|(id, continuation)| furnace_inspect(*id, continuation))
    }

    /// Projects recipe inspect fragments in catalog identity order.
    ///
    /// Workstation recipes stay `craftable: false` unless `bound_workstation`
    /// names a loaded container realizing that contract. Input ownership is
    /// quantity-based and independent of crafting-grid layout.
    ///
    /// # Errors
    ///
    /// Returns a player, container, or recipe-planning rejection.
    pub fn inspect_recipes(
        &self,
        state: &ReferenceGameplayState,
        player: PlayerId,
        bound_workstation: Option<&WorkstationId>,
    ) -> Result<Vec<RecipeInspectV1>, GameplayReject> {
        let inventory = state
            .inventory(player)
            .ok_or(GameplayReject::UnknownPlayer {
                player: player.as_bytes(),
            })?;
        let mut fragments = Vec::new();
        for (id, recipe) in self.catalog().recipes() {
            let workstation_ready = match recipe.workstation.as_ref() {
                None => true,
                Some(required) => {
                    bound_workstation == Some(required)
                        && state
                            .containers()
                            .values()
                            .any(|container| container.workstation.as_ref() == Some(required))
                }
            };
            let craftable = workstation_ready
                && recipe_inputs_owned(self.catalog(), inventory.slots(), recipe)?;
            fragments.push(RecipeInspectV1 {
                recipe: id.clone(),
                workstation: recipe.workstation.clone(),
                craftable,
            });
        }
        Ok(fragments)
    }

    /// Returns recipe identities whose workstation matches and whose inputs are owned.
    ///
    /// # Errors
    ///
    /// Returns a player or recipe-planning rejection.
    pub fn craftable_recipes(
        &self,
        state: &ReferenceGameplayState,
        player: PlayerId,
        workstation: Option<&WorkstationId>,
    ) -> Result<Vec<RecipeId>, GameplayReject> {
        Ok(self
            .inspect_recipes(state, player, workstation)?
            .into_iter()
            .filter(|fragment| fragment.craftable && fragment.workstation.as_ref() == workstation)
            .map(|fragment| fragment.recipe)
            .collect())
    }
}

fn inventory_inspect(
    player: PlayerId,
    inventory: &InventoryStateV1,
) -> Result<InventoryInspectV1, GameplayReject> {
    let (occupied_slots, total_quantity) = occupancy(inventory.slots())?;
    Ok(InventoryInspectV1 {
        player,
        revision: inventory.revision(),
        slot_count: u16::try_from(inventory.len()).map_err(|_| GameplayReject::LimitExceeded {
            resource: "inventory_slots",
            limit: usize::from(u16::MAX),
            actual: inventory.len(),
        })?,
        hotbar_slots: inventory.hotbar_slots(),
        selected_hotbar: inventory.selected_hotbar().get(),
        occupied_slots,
        total_quantity,
    })
}

fn container_inspect(
    container: ContainerId,
    state: &crate::ContainerStateV1,
) -> Result<ContainerInspectV1, GameplayReject> {
    let (occupied_slots, total_quantity) = occupancy(state.slots())?;
    Ok(ContainerInspectV1 {
        container,
        revision: state.revision(),
        workstation: state.workstation().cloned(),
        slot_count: u16::try_from(state.len()).map_err(|_| GameplayReject::LimitExceeded {
            resource: "container_slots",
            limit: usize::from(u16::MAX),
            actual: state.len(),
        })?,
        occupied_slots,
        total_quantity,
    })
}

fn furnace_inspect(
    continuation: ContinuationId,
    state: &FurnaceContinuationV1,
) -> FurnaceInspectV1 {
    FurnaceInspectV1 {
        container: state.container,
        continuation,
        process: state.process.clone(),
        due_tick: state.due_tick.get(),
        pending_output: state.pending_output.clone(),
    }
}

fn occupancy(slots: &[Option<ItemStackV1>]) -> Result<(u16, u64), GameplayReject> {
    let mut occupied = 0_u16;
    let mut total = 0_u64;
    for stack in slots.iter().flatten() {
        occupied = occupied
            .checked_add(1)
            .ok_or(GameplayReject::QuantityOverflow)?;
        total = total
            .checked_add(u64::from(stack.quantity()))
            .ok_or(GameplayReject::QuantityOverflow)?;
    }
    Ok((occupied, total))
}

fn recipe_inputs_owned(
    catalog: &GameplayCatalog,
    slots: &[Option<ItemStackV1>],
    recipe: &RecipeDefinitionV1,
) -> Result<bool, GameplayReject> {
    let mut working = slots.to_vec();
    let candidate_slots: Box<[SlotIndex]> = (0..slots.len())
        .map(|index| {
            u16::try_from(index)
                .map(SlotIndex::new)
                .map_err(|_| GameplayReject::LimitExceeded {
                    resource: "inventory_slots",
                    limit: usize::from(u16::MAX),
                    actual: index,
                })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let result = match &recipe.pattern {
        RecipePatternV1::Shapeless { ingredients } => crate::kernel::consume_shapeless(
            catalog,
            &mut working,
            &candidate_slots,
            ingredients,
            &recipe.id,
        ),
        RecipePatternV1::Shaped { cells, .. } => {
            let ingredients: Vec<_> = cells.iter().flatten().cloned().collect();
            crate::kernel::consume_shapeless(
                catalog,
                &mut working,
                &candidate_slots,
                &ingredients,
                &recipe.id,
            )
        }
    };
    match result {
        Ok(()) => Ok(true),
        Err(GameplayReject::RecipeMismatch { .. } | GameplayReject::EmptySlot) => Ok(false),
        Err(error) => Err(error),
    }
}
