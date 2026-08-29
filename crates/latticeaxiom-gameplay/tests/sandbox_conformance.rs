//! Headless D8 conformance fixtures for the generic sandbox kernel.
//!
//! These tests intentionally use only `example:*`, `other:*`, and versioned
//! platform semantic contracts so a domain package can be replaced wholesale.
use std::{
    collections::BTreeMap,
    fmt::Debug,
    num::{NonZeroU8, NonZeroU16, NonZeroU32},
    ops::{Deref, DerefMut},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

use latticeaxiom_core::{CanonicalHash, SchemaId, StableId};
use latticeaxiom_gameplay::{
    AuthorityTick, BlockDefinitionV1, BlockId, BlockKey, BlockPosition, CancelMiningCommandV1,
    CatalogLimits, ChunkCoordinate, ChunkRevision, CommandEnvelopeV1, CommandOutcomeV1,
    ContainerId, ContainerOwnerComponentV1, ContainerStateV1, CreativePickCommandV1,
    DimensionChunkKey, DimensionId, DropEntityId, DropItemCommandV1, FaultInjection,
    FrozenItemRoleBindingV1, FuelRuleV1, GameplayCatalog, GameplayCatalogSourceV1,
    GameplayCommandV1, GameplayEditTarget, GameplayKernel, GameplayLimits, GameplayModeV1,
    GameplayPlanV1, GameplayReject, GameplayRulesV1, GameplayStorageDomain, IngredientV1,
    InventoryStateV1, ItemDefinitionV1, ItemId, ItemPredicateV1, ItemRoleDefinitionV1, ItemRoleId,
    ItemStackV1, ItemStateV1, ItemTagDefinitionV1, ItemTagId, MineCommandV1, MiningRuleV1,
    MiningStepCountV1, MoveStackCommandV1, PersistentEntityId, PickupCommandV1, PlaceCommandV1,
    PlayerId, ProcessDefinitionV1, RecipeCraftCommandV1, RecipeDefinitionV1, RecipeId,
    RecipePatternV1, ReferenceGameplayState, ReferencePlanApplier, RoleOutputV1,
    RuntimePlanReceiptV1, ScheduledAdvanceCommandV1, SelectHotbarCommandV1, SlotIndex,
    StartProcessCommandV1, ToolClassId, ToolDefinitionV1, ToolRequirementV1, TransactionId,
    WorkstationDefinitionV1, WorkstationId, WorldRevision,
};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkData, ChunkKey, ChunkMutation,
    ChunkRevisionExpectation, CommitReceipt, ContinuationId, MemoryTransactionKernel,
    PayloadSchemaVersion, VersionedPayload, WorldTransaction,
};
use proptest::prelude::*;

const PLAYER: PlayerId = PlayerId::new(1);
const OTHER_PLAYER: PlayerId = PlayerId::new(2);
const WORKBENCH_CONTAINER: ContainerId = ContainerId::from_bytes([2; 16]);
const FURNACE_CONTAINER: ContainerId = ContainerId::from_bytes([4; 16]);
static NEXT_RESERVED_DROP: AtomicU64 = AtomicU64::new(10_000);

fn fixture_world() -> latticeaxiom_gameplay::WorldId {
    parsed("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
}

fn reserved_drop() -> DropEntityId {
    DropEntityId::new(NEXT_RESERVED_DROP.fetch_add(1, Ordering::Relaxed))
}

fn parsed<T>(value: &str) -> T
where
    T: FromStr,
    T::Err: Debug,
{
    match value.parse() {
        Ok(value) => value,
        Err(error) => panic!("fixture identifier `{value}` failed: {error:?}"),
    }
}

fn nz8(value: u8) -> NonZeroU8 {
    match NonZeroU8::new(value) {
        Some(value) => value,
        None => panic!("fixture non-zero u8 was zero"),
    }
}

fn nz16(value: u16) -> NonZeroU16 {
    match NonZeroU16::new(value) {
        Some(value) => value,
        None => panic!("fixture non-zero u16 was zero"),
    }
}

fn nz32(value: u32) -> NonZeroU32 {
    match NonZeroU32::new(value) {
        Some(value) => value,
        None => panic!("fixture non-zero u32 was zero"),
    }
}

fn plain(item: &str, quantity: u32) -> ItemStackV1 {
    match ItemStackV1::plain(parsed(item), quantity) {
        Ok(stack) => stack,
        Err(error) => panic!("fixture stack failed: {error}"),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one complete cross-mechanic fixture is easier to audit"
)]
fn catalog_source() -> GameplayCatalogSourceV1 {
    let log: ItemId = parsed("example:item/log");
    let foreign_log: ItemId = parsed("other:item/compatible-log");
    let plank: ItemId = parsed("example:item/plank");
    let stick: ItemId = parsed("example:item/stick");
    let pickaxe: ItemId = parsed("example:item/pickaxe");
    let ore: ItemId = parsed("example:item/copper-ore");
    let foreign_ore: ItemId = parsed("other:item/compatible-copper");
    let ingot: ItemId = parsed("example:item/copper-ingot");
    let charcoal: ItemId = parsed("example:item/charcoal");
    let broad_rock: ItemId = parsed("other:item/broad-material-only");

    let log_block: BlockId = parsed("example:block/log");
    let plank_block: BlockId = parsed("example:block/plank");
    let ore_block: BlockId = parsed("example:block/copper-ore");
    let pickaxe_class: ToolClassId = parsed("latticeaxiom:tool-class/pickaxe@1");
    let logs: ItemTagId = parsed("latticeaxiom:item-tag/logs@1");
    let copper_inputs: ItemTagId = parsed("latticeaxiom:item-tag/copper-inputs@1");
    let broad_materials: ItemTagId = parsed("latticeaxiom:item-tag/broad-materials@1");
    let plank_role: ItemRoleId = parsed("latticeaxiom:item-role/plank-output@1");
    let tool_role: ItemRoleId = parsed("latticeaxiom:item-role/pickaxe-output@1");
    let ingot_role: ItemRoleId = parsed("latticeaxiom:item-role/copper-ingot-output@1");
    let workbench: WorkstationId = parsed("latticeaxiom:workstation/crafting@1");
    let furnace: WorkstationId = parsed("latticeaxiom:workstation/furnace@1");

    GameplayCatalogSourceV1 {
        items: vec![
            ItemDefinitionV1 {
                id: log.clone(),
                stack_limit: nz32(64),
                placement_block: Some(log_block.clone()),
                durability: None,
            },
            ItemDefinitionV1 {
                id: foreign_log.clone(),
                stack_limit: nz32(64),
                placement_block: None,
                durability: None,
            },
            ItemDefinitionV1 {
                id: plank.clone(),
                stack_limit: nz32(64),
                placement_block: Some(plank_block.clone()),
                durability: None,
            },
            ItemDefinitionV1 {
                id: stick.clone(),
                stack_limit: nz32(64),
                placement_block: None,
                durability: None,
            },
            ItemDefinitionV1 {
                id: pickaxe.clone(),
                stack_limit: nz32(1),
                placement_block: None,
                durability: Some(nz32(10)),
            },
            ItemDefinitionV1 {
                id: ore.clone(),
                stack_limit: nz32(64),
                placement_block: None,
                durability: None,
            },
            ItemDefinitionV1 {
                id: foreign_ore.clone(),
                stack_limit: nz32(64),
                placement_block: None,
                durability: None,
            },
            ItemDefinitionV1 {
                id: ingot.clone(),
                stack_limit: nz32(64),
                placement_block: None,
                durability: None,
            },
            ItemDefinitionV1 {
                id: charcoal.clone(),
                stack_limit: nz32(64),
                placement_block: None,
                durability: None,
            },
            ItemDefinitionV1 {
                id: broad_rock.clone(),
                stack_limit: nz32(64),
                placement_block: None,
                durability: None,
            },
        ],
        blocks: vec![
            BlockDefinitionV1 {
                id: log_block,
                mining: MiningRuleV1 {
                    hardness: nz32(1),
                    tool: None,
                },
                drop: plain("example:item/log", 1),
            },
            BlockDefinitionV1 {
                id: plank_block,
                mining: MiningRuleV1 {
                    hardness: nz32(1),
                    tool: None,
                },
                drop: plain("example:item/plank", 1),
            },
            BlockDefinitionV1 {
                id: ore_block,
                mining: MiningRuleV1 {
                    hardness: nz32(6),
                    tool: Some(ToolRequirementV1 {
                        class: pickaxe_class.clone(),
                        minimum_tier: 1,
                    }),
                },
                drop: plain("example:item/copper-ore", 1),
            },
        ],
        tools: vec![ToolDefinitionV1 {
            item: pickaxe.clone(),
            class: pickaxe_class,
            tier: 1,
            work_per_step: nz32(3),
            maximum_durability: nz32(10),
        }],
        tags: vec![
            ItemTagDefinitionV1 {
                id: logs.clone(),
                members: vec![log.clone(), foreign_log.clone()].into_boxed_slice(),
            },
            ItemTagDefinitionV1 {
                id: copper_inputs.clone(),
                members: vec![ore.clone(), foreign_ore.clone()].into_boxed_slice(),
            },
            ItemTagDefinitionV1 {
                id: broad_materials,
                members: vec![charcoal.clone(), broad_rock].into_boxed_slice(),
            },
        ],
        categories: Vec::new(),
        roles: vec![
            ItemRoleDefinitionV1 {
                id: plank_role.clone(),
                accepts: ItemPredicateV1::Exact(plank.clone()),
            },
            ItemRoleDefinitionV1 {
                id: tool_role.clone(),
                accepts: ItemPredicateV1::Exact(pickaxe.clone()),
            },
            ItemRoleDefinitionV1 {
                id: ingot_role.clone(),
                accepts: ItemPredicateV1::Exact(ingot.clone()),
            },
        ],
        bindings: vec![
            FrozenItemRoleBindingV1 {
                role: plank_role.clone(),
                item: plank.clone(),
            },
            FrozenItemRoleBindingV1 {
                role: tool_role.clone(),
                item: pickaxe,
            },
            FrozenItemRoleBindingV1 {
                role: ingot_role.clone(),
                item: ingot,
            },
        ],
        recipes: vec![
            RecipeDefinitionV1 {
                id: parsed("example:recipe/planks@1"),
                workstation: None,
                pattern: RecipePatternV1::Shapeless {
                    ingredients: vec![IngredientV1 {
                        accepts: ItemPredicateV1::InTag(logs),
                        quantity: nz32(1),
                    }]
                    .into_boxed_slice(),
                },
                output: RoleOutputV1 {
                    role: plank_role,
                    quantity: nz32(4),
                },
            },
            RecipeDefinitionV1 {
                id: parsed("example:recipe/pickaxe@1"),
                workstation: Some(workbench.clone()),
                pattern: RecipePatternV1::Shaped {
                    width: nz8(2),
                    height: nz8(1),
                    cells: vec![
                        Some(IngredientV1 {
                            accepts: ItemPredicateV1::Exact(plank),
                            quantity: nz32(3),
                        }),
                        Some(IngredientV1 {
                            accepts: ItemPredicateV1::Exact(stick),
                            quantity: nz32(2),
                        }),
                    ]
                    .into_boxed_slice(),
                },
                output: RoleOutputV1 {
                    role: tool_role,
                    quantity: nz32(1),
                },
            },
        ],
        workstations: vec![
            WorkstationDefinitionV1 { id: workbench },
            WorkstationDefinitionV1 {
                id: furnace.clone(),
            },
        ],
        processes: vec![ProcessDefinitionV1 {
            id: parsed("example:process/smelt-copper@1"),
            workstation: furnace,
            input: IngredientV1 {
                accepts: ItemPredicateV1::InTag(copper_inputs),
                quantity: nz32(1),
            },
            output: RoleOutputV1 {
                role: ingot_role,
                quantity: nz32(1),
            },
            duration_ticks: nz32(5),
        }],
        fuel_rules: vec![FuelRuleV1 {
            accepts: ItemPredicateV1::Exact(charcoal),
            burn_ticks: nz32(10),
        }],
        block_schema_bindings: Vec::new(),
    }
}

fn catalog() -> GameplayCatalog {
    match GameplayCatalog::compile(catalog_source(), CatalogLimits::default()) {
        Ok(catalog) => catalog,
        Err(error) => panic!("fixture catalog failed: {error}"),
    }
}

fn fixture_dimension() -> DimensionId {
    parsed("example:dimension/fixture")
}

fn fixture_chunk() -> DimensionChunkKey {
    DimensionChunkKey::new(fixture_dimension(), ChunkCoordinate::new(0, 0, 0))
}

fn block_key(dimension: &DimensionId, position: BlockPosition) -> BlockKey {
    BlockKey::new(dimension.clone(), position)
}

fn persistent_target(chunk: DimensionChunkKey) -> GameplayEditTarget {
    GameplayEditTarget::new(chunk, GameplayStorageDomain::PersistentEntities)
}

fn fixture_transaction_id(state: &ReferenceGameplayState) -> TransactionId {
    let digest = state.canonical_hash().as_bytes();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    if bytes.iter().all(|byte| *byte == 0) {
        bytes[15] = 1;
    }
    TransactionId::from_bytes(bytes)
}
fn owner(entity_byte: u8) -> ContainerOwnerComponentV1 {
    ContainerOwnerComponentV1 {
        dimension: fixture_dimension(),
        chunk: ChunkCoordinate::new(0, 0, 0),
        entity: PersistentEntityId::from_bytes([entity_byte; 16]),
    }
}

fn state_with_inventory(slots: usize) -> ReferenceGameplayState {
    let mut state = match ReferenceGameplayState::new(GameplayLimits::default()) {
        Ok(state) => state,
        Err(error) => panic!("fixture state failed: {error}"),
    };
    let chunk = fixture_chunk();
    if let Err(error) = state.seed_loaded_chunk(chunk.clone(), ChunkRevision::ZERO) {
        panic!("fixture loaded chunk failed: {error}");
    }
    let inventory = match InventoryStateV1::empty(persistent_target(chunk), slots) {
        Ok(inventory) => inventory,
        Err(error) => panic!("fixture inventory failed: {error}"),
    };
    if let Err(error) = state.seed_player(PLAYER, inventory) {
        panic!("fixture player failed: {error}");
    }
    state
}

fn seed_stack(state: &mut ReferenceGameplayState, player: PlayerId, slot: u16, stack: ItemStackV1) {
    let mut inventory = match state.inventory(player) {
        Some(inventory) => inventory.clone(),
        None => panic!("fixture player missing"),
    };
    if let Err(error) = inventory.seed_slot(SlotIndex::new(slot), Some(stack)) {
        panic!("fixture slot failed: {error}");
    }
    let mut rebuilt = match ReferenceGameplayState::new(GameplayLimits::default()) {
        Ok(value) => value,
        Err(error) => panic!("fixture rebuild failed: {error}"),
    };
    if let Err(error) = rebuilt.seed_loaded_chunk(fixture_chunk(), ChunkRevision::ZERO) {
        panic!("fixture rebuilt chunk failed: {error}");
    }
    if let Err(error) = rebuilt.seed_player(player, inventory) {
        panic!("fixture rebuild player failed: {error}");
    }
    // This helper is only used before any other aggregate state is seeded.
    *state = rebuilt;
}

fn envelope(state: &ReferenceGameplayState, command: GameplayCommandV1) -> CommandEnvelopeV1 {
    CommandEnvelopeV1 {
        transaction_id: fixture_transaction_id(state),
        expected_world_revision: state.observed_world_revision(),
        command,
    }
}

#[derive(Debug)]
struct FixtureAuthority {
    gameplay: ReferencePlanApplier,
    rules: GameplayRulesV1,
    storage: MemoryTransactionKernel,
    storage_revisions: BTreeMap<DimensionChunkKey, ChunkRevision>,
    next_transaction: u128,
}

impl Deref for FixtureAuthority {
    type Target = ReferencePlanApplier;

    fn deref(&self) -> &Self::Target {
        &self.gameplay
    }
}

impl DerefMut for FixtureAuthority {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.gameplay
    }
}

fn fixture_chunk_data(seed: u64) -> ChunkData {
    let schema: SchemaId = parsed("latticeaxiom:schema/gameplay-fixture@1");
    let schema_version = match PayloadSchemaVersion::new(1) {
        Ok(value) => value,
        Err(error) => panic!("fixture schema version failed: {error}"),
    };
    let seed_bytes = seed.to_be_bytes().to_vec();
    let voxels = VersionedPayload::new(schema.clone(), schema_version, seed_bytes.clone());
    let mut entities = BTreeMap::new();
    entities.insert(
        PersistentEntityId::from_u128(u128::from(seed).saturating_add(1_000_000)),
        VersionedPayload::new(schema.clone(), schema_version, seed_bytes.clone()),
    );
    let mut continuations = BTreeMap::new();
    continuations.insert(
        ContinuationId::from_u128(u128::from(seed).saturating_add(2_000_000)),
        VersionedPayload::new(schema, schema_version, seed_bytes),
    );
    let mut provenance = BTreeMap::new();
    provenance.insert(
        parsed::<StableId>("latticeaxiom:provenance/gameplay-fixture@1"),
        CanonicalHash::digest(seed.to_be_bytes()),
    );
    ChunkData::new(voxels, entities, continuations, provenance)
}

fn distinct_transaction_id(transaction_id: TransactionId, discriminator: u8) -> TransactionId {
    let mut bytes = *transaction_id.as_bytes();
    bytes[0] ^= discriminator;
    if bytes.iter().all(|byte| *byte == 0) {
        bytes[15] = 1;
    }
    TransactionId::from_bytes(bytes)
}

fn fixture_mutation(
    world: latticeaxiom_gameplay::WorldId,
    chunk: DimensionChunkKey,
    expected: ChunkRevisionExpectation,
    seed: u64,
) -> ChunkMutation {
    ChunkMutation::new(
        ChunkKey::new(world, chunk.dimension, chunk.coordinate),
        expected,
        ChangedDomains::ALL,
        fixture_chunk_data(seed),
    )
}

fn fixture_commit_receipt(
    storage: &MemoryTransactionKernel,
    transaction_id: TransactionId,
    world: latticeaxiom_gameplay::WorldId,
    base_world_revision: WorldRevision,
    mutations: Vec<ChunkMutation>,
) -> CommitReceipt {
    storage
        .commit(WorldTransaction::new(
            transaction_id,
            world,
            base_world_revision,
            mutations,
        ))
        .unwrap_or_else(|error| panic!("fixture storage receipt failed: {error}"))
}

fn capture_domains(plan: &GameplayPlanV1) -> BTreeMap<DimensionChunkKey, ChangedDomains> {
    let mut domains: BTreeMap<DimensionChunkKey, ChangedDomains> = BTreeMap::new();
    for edit in plan.edits() {
        let Some(target) = edit.storage_capture_target() else {
            continue;
        };
        let domain = match target.domain {
            GameplayStorageDomain::Voxels => ChangedDomains::VOXELS,
            GameplayStorageDomain::PersistentEntities => ChangedDomains::PERSISTENT_ENTITIES,
            GameplayStorageDomain::Continuations => ChangedDomains::CONTINUATIONS,
        };
        domains
            .entry(target.chunk.clone())
            .and_modify(|captured| *captured = captured.union(domain))
            .or_insert(domain);
    }
    domains
}

impl FixtureAuthority {
    fn next_envelope(&mut self, command: GameplayCommandV1) -> CommandEnvelopeV1 {
        let transaction_id = TransactionId::from_u128(self.next_transaction);
        self.next_transaction = self.next_transaction.saturating_add(1);
        CommandEnvelopeV1 {
            transaction_id,
            expected_world_revision: self.gameplay.state().observed_world_revision(),
            command,
        }
    }

    fn commit_domains(
        &mut self,
        transaction_id: TransactionId,
        domains: &BTreeMap<DimensionChunkKey, ChangedDomains>,
    ) {
        if domains.is_empty() {
            return;
        }
        let base_world_revision = self.gameplay.state().observed_world_revision();
        let mutations = domains
            .keys()
            .map(|chunk| {
                let expected = self.storage_revisions.get(chunk).copied().map_or(
                    ChunkRevisionExpectation::Absent,
                    ChunkRevisionExpectation::Exact,
                );
                ChunkMutation::new(
                    ChunkKey::new(
                        self.gameplay.world(),
                        chunk.dimension.clone(),
                        chunk.coordinate,
                    ),
                    expected,
                    ChangedDomains::ALL,
                    fixture_chunk_data(NEXT_RESERVED_DROP.fetch_add(1, Ordering::Relaxed)),
                )
            })
            .collect();
        let storage_receipt = match self.storage.publish(WorldTransaction::new(
            transaction_id,
            self.gameplay.world(),
            base_world_revision,
            mutations,
        )) {
            Ok(receipt) => receipt,
            Err(error) => panic!("fixture storage commit failed: {error}"),
        };
        if let Err(error) = self.gameplay.observe_storage_publication(&storage_receipt) {
            panic!("fixture storage receipt reconciliation failed: {error}");
        }
        for chunk_receipt in storage_receipt.chunks() {
            let key = chunk_receipt.key();
            self.storage_revisions.insert(
                DimensionChunkKey::new(key.dimension.clone(), key.coordinate),
                chunk_receipt.chunk_revision(),
            );
        }
    }

    fn execute_committed(
        &mut self,
        catalog: &GameplayCatalog,
        envelope: &CommandEnvelopeV1,
    ) -> Result<RuntimePlanReceiptV1, GameplayReject> {
        let plan = GameplayKernel::with_rules(catalog, self.rules)
            .plan(self.gameplay.state(), envelope)?;
        let domains = capture_domains(&plan);
        let receipt = self.gameplay.apply_plan(plan, FaultInjection::None)?;
        self.commit_domains(envelope.transaction_id, &domains);
        Ok(receipt)
    }
}

fn execute(
    authority: &mut FixtureAuthority,
    catalog: &GameplayCatalog,
    command: GameplayCommandV1,
) -> RuntimePlanReceiptV1 {
    let envelope = authority.next_envelope(command);
    match authority.execute_committed(catalog, &envelope) {
        Ok(receipt) => receipt,
        Err(error) => panic!("fixture command failed: {} ({})", error, error.code()),
    }
}

fn applier(state: ReferenceGameplayState, catalog: &GameplayCatalog) -> FixtureAuthority {
    applier_with_rules(state, catalog, GameplayRulesV1::default())
}

fn applier_with_rules(
    state: ReferenceGameplayState,
    catalog: &GameplayCatalog,
    rules: GameplayRulesV1,
) -> FixtureAuthority {
    let next_transaction = u128::from(state.observed_world_revision().get()).saturating_add(1);
    let world = fixture_world();
    let storage = MemoryTransactionKernel::new();
    let mut storage_revisions = BTreeMap::new();
    let desired_chunks = state.loaded_chunks().clone();
    let desired_world_revision = state.observed_world_revision();
    let frontier_chunk = DimensionChunkKey::new(
        parsed("example:dimension/storage-fixture-frontier"),
        ChunkCoordinate::new(i32::MIN, i32::MIN, i32::MIN),
    );
    for step in 0..desired_world_revision.get() {
        let mut step_chunks: Vec<_> = desired_chunks
            .iter()
            .filter(|(_, revision)| revision.get() > step)
            .map(|(chunk, _)| chunk.clone())
            .collect();
        if step_chunks.is_empty() {
            step_chunks.push(frontier_chunk.clone());
        }
        let mutations = step_chunks
            .into_iter()
            .map(|chunk| {
                let expected = storage_revisions.get(&chunk).copied().map_or(
                    ChunkRevisionExpectation::Absent,
                    ChunkRevisionExpectation::Exact,
                );
                ChunkMutation::new(
                    ChunkKey::new(world, chunk.dimension.clone(), chunk.coordinate),
                    expected,
                    ChangedDomains::ALL,
                    fixture_chunk_data(NEXT_RESERVED_DROP.fetch_add(1, Ordering::Relaxed)),
                )
            })
            .collect();
        let receipt = match storage.commit(WorldTransaction::new(
            TransactionId::from_u128(u128::from(step).saturating_add(1)),
            world,
            WorldRevision::new(step),
            mutations,
        )) {
            Ok(receipt) => receipt,
            Err(error) => panic!("fixture storage seeding failed: {error}"),
        };
        for chunk_receipt in receipt.chunks() {
            let key = chunk_receipt.key();
            storage_revisions.insert(
                DimensionChunkKey::new(key.dimension.clone(), key.coordinate),
                chunk_receipt.chunk_revision(),
            );
        }
    }
    for (chunk, expected) in &desired_chunks {
        let actual = storage_revisions
            .get(chunk)
            .copied()
            .unwrap_or(ChunkRevision::ZERO);
        assert_eq!(
            actual, *expected,
            "fixture storage revision mismatch for {chunk:?}: {actual:?} != {expected:?}"
        );
    }
    let gameplay = ReferencePlanApplier::try_new_with_rules(world, state, catalog.clone(), rules)
        .unwrap_or_else(|error| panic!("fixture loaded-state validation failed: {error}"));
    FixtureAuthority {
        gameplay,
        rules,
        storage,
        storage_revisions,
        next_transaction,
    }
}
fn drop_from(receipt: &RuntimePlanReceiptV1) -> latticeaxiom_gameplay::DropEntityId {
    match &receipt.outcome {
        CommandOutcomeV1::BlockBroken { drop, .. } | CommandOutcomeV1::ItemDropped { drop } => {
            *drop
        }
        outcome => panic!("expected drop outcome, found {outcome:?}"),
    }
}

fn seed_workbench(state: &mut ReferenceGameplayState) {
    let container = match ContainerStateV1::empty(
        owner(2),
        Some(parsed("latticeaxiom:workstation/crafting@1")),
        3,
    ) {
        Ok(container) => container,
        Err(error) => panic!("fixture workbench failed: {error}"),
    };
    if let Err(error) = state.seed_container(WORKBENCH_CONTAINER, container) {
        panic!("fixture workbench seed failed: {error}");
    }
}

fn furnace_container(
    input: ItemStackV1,
    fuel: ItemStackV1,
    output: Option<ItemStackV1>,
) -> ContainerStateV1 {
    let mut container = match ContainerStateV1::empty(
        owner(4),
        Some(parsed("latticeaxiom:workstation/furnace@1")),
        3,
    ) {
        Ok(container) => container,
        Err(error) => panic!("fixture furnace failed: {error}"),
    };
    for (slot, value) in [(0, Some(input)), (1, Some(fuel)), (2, output)] {
        if let Err(error) = container.seed_slot(SlotIndex::new(slot), value) {
            panic!("fixture furnace slot failed: {error}");
        }
    }
    container
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the test intentionally shows the complete command journey"
)]
fn command_input_journey_collects_crafts_mines_and_places_without_loss() {
    let catalog = catalog();
    let mut state = state_with_inventory(8);
    seed_stack(&mut state, PLAYER, 1, plain("example:item/stick", 2));
    seed_workbench(&mut state);
    let dimension = fixture_dimension();
    let log_position = BlockPosition { x: 0, y: 8, z: 0 };
    let ore_position = BlockPosition { x: 1, y: 8, z: 0 };
    if let Err(error) = state.seed_block(
        block_key(&dimension, log_position),
        parsed("example:block/log"),
    ) {
        panic!("fixture log block failed: {error}");
    }
    if let Err(error) = state.seed_block(
        block_key(&dimension, ore_position),
        parsed("example:block/copper-ore"),
    ) {
        panic!("fixture ore block failed: {error}");
    }
    let mut authority = applier(state, &catalog);

    let log_drop = drop_from(&execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, log_position),
            expected_chunk_revision: ChunkRevision::ZERO,
            tool_slot: None,
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    ));
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Pickup(PickupCommandV1 {
            player: PLAYER,
            drop: log_drop,
        }),
    );
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Craft(RecipeCraftCommandV1 {
            player: PLAYER,
            recipe: parsed("example:recipe/planks@1"),
            input_slots: vec![SlotIndex::new(0)].into_boxed_slice(),
            workstation: None,
        }),
    );
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Craft(RecipeCraftCommandV1 {
            player: PLAYER,
            recipe: parsed("example:recipe/pickaxe@1"),
            input_slots: vec![SlotIndex::new(0), SlotIndex::new(1)].into_boxed_slice(),
            workstation: Some(WORKBENCH_CONTAINER),
        }),
    );
    let first_ore_revision = authority
        .state()
        .loaded_chunk_revision(&block_key(&dimension, ore_position).chunk())
        .unwrap_or_else(|| panic!("journey ore chunk missing"));
    let first = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, ore_position),
            expected_chunk_revision: first_ore_revision,
            tool_slot: Some(SlotIndex::new(1)),
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    );
    assert!(matches!(
        first.outcome,
        CommandOutcomeV1::MiningProgress {
            accumulated: 3,
            required: 6
        }
    ));
    let second_ore_revision = authority
        .state()
        .loaded_chunk_revision(&block_key(&dimension, ore_position).chunk())
        .unwrap_or_else(|| panic!("journey ore chunk missing"));
    let ore_drop = drop_from(&execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, ore_position),
            expected_chunk_revision: second_ore_revision,
            tool_slot: Some(SlotIndex::new(1)),
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    ));
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Pickup(PickupCommandV1 {
            player: PLAYER,
            drop: ore_drop,
        }),
    );
    let place_position = BlockPosition { x: 2, y: 8, z: 0 };
    let place_target = block_key(&dimension, place_position);
    let place_chunk_revision = authority
        .state()
        .loaded_chunk_revision(&place_target.chunk())
        .unwrap_or_else(|| panic!("journey place chunk missing"));
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Place(PlaceCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(0),
            target: place_target,
            expected_chunk_revision: place_chunk_revision,
        }),
    );

    let Some(inventory) = authority.state().inventory(PLAYER) else {
        panic!("journey inventory missing");
    };
    let tool = match inventory.slot(SlotIndex::new(1)) {
        Ok(Some(tool)) => tool,
        other => panic!("journey tool missing: {other:?}"),
    };
    assert_eq!(tool.item().as_str(), "example:item/pickaxe");
    assert!(
        matches!(tool.state(), ItemStateV1::ToolDurability { remaining } if remaining.get() == 9)
    );
    assert_eq!(
        authority
            .state()
            .block(&block_key(&dimension, place_position))
            .map(BlockId::as_str),
        Some("example:block/plank")
    );
    assert_eq!(
        authority.state().loaded_chunk_revision(&fixture_chunk()),
        Some(ChunkRevision::new(7))
    );
    assert_eq!(
        authority.state().observed_world_revision(),
        WorldRevision::new(7)
    );
}

#[test]
fn mining_release_cancels_progress_and_low_rate_batches_preserve_work() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    seed_stack(
        &mut state,
        PLAYER,
        0,
        tool_stack("example:item/pickaxe", 10),
    );
    let dimension = fixture_dimension();
    let position = BlockPosition { x: 1, y: 8, z: 0 };
    let target = block_key(&dimension, position);
    state
        .seed_block(target.clone(), parsed("example:block/copper-ore"))
        .unwrap_or_else(|error| panic!("mining cancellation block seed failed: {error}"));
    let mut authority = applier(state, &catalog);
    let initial_world_revision = authority.state().observed_world_revision();

    let progress = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            player: PLAYER,
            target: target.clone(),
            expected_chunk_revision: ChunkRevision::ZERO,
            tool_slot: Some(SlotIndex::new(0)),
            reserved_drop: reserved_drop(),
            steps: MiningStepCountV1::ONE,
        }),
    );
    assert!(matches!(
        progress.outcome,
        CommandOutcomeV1::MiningProgress {
            accumulated: 3,
            required: 6
        }
    ));
    assert_eq!(authority.state().break_progress().len(), 1);
    assert_eq!(
        authority.state().observed_world_revision(),
        initial_world_revision,
        "transient mining progress must not publish a storage transaction"
    );

    let cancelled = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::CancelMining(CancelMiningCommandV1 { player: PLAYER }),
    );
    assert_eq!(
        cancelled.outcome,
        CommandOutcomeV1::MiningCancelled { had_progress: true }
    );
    assert!(authority.state().break_progress().is_empty());
    assert_eq!(
        authority.state().observed_world_revision(),
        initial_world_revision,
        "mining cancellation must remain runtime-only"
    );

    let revision = authority
        .state()
        .loaded_chunk_revision(&target.chunk())
        .unwrap_or_else(|| panic!("cancelled mining chunk remains loaded"));
    let two_steps = MiningStepCountV1::new(2)
        .unwrap_or_else(|| panic!("two canonical mining steps are bounded"));
    let broken = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            player: PLAYER,
            target,
            expected_chunk_revision: revision,
            tool_slot: Some(SlotIndex::new(0)),
            reserved_drop: reserved_drop(),
            steps: two_steps,
        }),
    );
    assert!(matches!(
        broken.outcome,
        CommandOutcomeV1::BlockBroken { .. }
    ));
}

#[test]
fn third_party_tag_input_is_accepted_but_output_is_frozen_concrete_role() {
    let catalog = catalog();
    let mut state = state_with_inventory(4);
    seed_stack(&mut state, PLAYER, 0, plain("other:item/compatible-log", 1));
    let mut authority = applier(state, &catalog);
    let receipt = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Craft(RecipeCraftCommandV1 {
            player: PLAYER,
            recipe: parsed("example:recipe/planks@1"),
            input_slots: vec![SlotIndex::new(0)].into_boxed_slice(),
            workstation: None,
        }),
    );
    assert!(
        matches!(receipt.outcome, CommandOutcomeV1::Crafted { ref output, .. } if output.item().as_str() == "example:item/plank" && output.quantity() == 4)
    );
}

#[test]
fn broad_material_tag_does_not_grant_fuel_mechanic() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    let container = furnace_container(
        plain("example:item/copper-ore", 1),
        plain("other:item/broad-material-only", 1),
        None,
    );
    if let Err(error) = state.seed_container(FURNACE_CONTAINER, container) {
        panic!("fixture furnace seed failed: {error}");
    }
    let before = state.clone();
    let mut authority = applier(state, &catalog);
    let command = envelope(
        authority.state(),
        GameplayCommandV1::StartProcess(StartProcessCommandV1 {
            container: FURNACE_CONTAINER,
            process: parsed("example:process/smelt-copper@1"),
            input_slot: SlotIndex::new(0),
            fuel_slot: SlotIndex::new(1),
            output_slot: SlotIndex::new(2),
            expected_container_revision: 0,
            started_at: AuthorityTick::new(10),
        }),
    );
    assert!(matches!(
        authority.execute(&command, FaultInjection::None),
        Err(GameplayReject::FuelRejected { .. })
    ));
    assert_eq!(authority.state(), &before);
}

#[test]
fn process_continuation_survives_in_memory_authority_handoff() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    let container = furnace_container(
        plain("other:item/compatible-copper", 1),
        plain("example:item/charcoal", 1),
        None,
    );
    if let Err(error) = state.seed_container(FURNACE_CONTAINER, container) {
        panic!("fixture furnace seed failed: {error}");
    }
    let mut authority = applier(state, &catalog);
    let scheduled = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::StartProcess(StartProcessCommandV1 {
            container: FURNACE_CONTAINER,
            process: parsed("example:process/smelt-copper@1"),
            input_slot: SlotIndex::new(0),
            fuel_slot: SlotIndex::new(1),
            output_slot: SlotIndex::new(2),
            expected_container_revision: 0,
            started_at: AuthorityTick::new(10),
        }),
    );
    let continuation = match scheduled.outcome {
        CommandOutcomeV1::ProcessScheduled {
            continuation,
            due_tick,
        } => {
            assert_eq!(due_tick.get(), 15);
            continuation
        }
        outcome => panic!("expected scheduled process, found {outcome:?}"),
    };
    let handed_off = authority.gameplay.into_state();
    let mut authority = applier(handed_off, &catalog);
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::AdvanceScheduled(ScheduledAdvanceCommandV1 {
            through_tick: AuthorityTick::new(100),
            max_completions: nz16(1),
        }),
    );
    assert!(authority.state().continuation(continuation).is_none());
    let Some(container) = authority.state().container(FURNACE_CONTAINER) else {
        panic!("handed-off furnace missing");
    };
    assert!(matches!(container.slot(SlotIndex::new(0)), Ok(None)));
    assert!(matches!(container.slot(SlotIndex::new(1)), Ok(None)));
    assert!(
        matches!(container.slot(SlotIndex::new(2)), Ok(Some(stack)) if stack.item().as_str() == "example:item/copper-ingot" && stack.quantity() == 1)
    );
}

#[test]
fn full_process_output_rejects_without_consuming_input_or_fuel() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    let container = furnace_container(
        plain("example:item/copper-ore", 1),
        plain("example:item/charcoal", 1),
        Some(plain("example:item/copper-ingot", 64)),
    );
    if let Err(error) = state.seed_container(FURNACE_CONTAINER, container) {
        panic!("fixture furnace seed failed: {error}");
    }
    let before = state.clone();
    let mut authority = applier(state, &catalog);
    let command = envelope(
        authority.state(),
        GameplayCommandV1::StartProcess(StartProcessCommandV1 {
            container: FURNACE_CONTAINER,
            process: parsed("example:process/smelt-copper@1"),
            input_slot: SlotIndex::new(0),
            fuel_slot: SlotIndex::new(1),
            output_slot: SlotIndex::new(2),
            expected_container_revision: 0,
            started_at: AuthorityTick::new(0),
        }),
    );
    assert!(matches!(
        authority.execute(&command, FaultInjection::None),
        Err(GameplayReject::OutputFull)
    ));
    assert_eq!(authority.state(), &before);
}

#[test]
fn mid_transaction_fault_rolls_back_and_exact_retry_applies_once() {
    let catalog = catalog();
    let mut state = state_with_inventory(4);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 3));
    let before = state.clone();
    let mut authority = applier(state, &catalog);
    let command = envelope(
        authority.state(),
        GameplayCommandV1::DropItem(DropItemCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            slot: SlotIndex::new(0),
            quantity: nz32(2),
            location: block_key(&fixture_dimension(), BlockPosition { x: 1, y: 2, z: 3 }),
        }),
    );
    assert!(matches!(
        authority.execute(&command, FaultInjection::AfterMutation(nz16(2))),
        Err(GameplayReject::InjectedFault { after_mutation: 2 })
    ));
    assert_eq!(authority.state(), &before);
    let receipt = match authority.execute(&command, FaultInjection::None) {
        Ok(receipt) => receipt,
        Err(error) => panic!("retry failed: {error}"),
    };
    let repeated = match authority.execute(&command, FaultInjection::None) {
        Ok(receipt) => receipt,
        Err(error) => panic!("idempotent retry failed: {error}"),
    };
    assert_eq!(receipt, repeated);
    let mut mismatched = command.clone();
    mismatched.command = GameplayCommandV1::DropItem(DropItemCommandV1 {
        reserved_drop: reserved_drop(),
        player: PLAYER,
        slot: SlotIndex::new(0),
        quantity: nz32(1),
        location: block_key(&fixture_dimension(), BlockPosition { x: 1, y: 2, z: 3 }),
    });
    let before_mismatch = authority.state().clone();
    assert!(matches!(
        authority.execute(&mismatched, FaultInjection::None),
        Err(GameplayReject::RetryPayloadMismatch { .. })
    ));
    assert_eq!(authority.state(), &before_mismatch);
    let slot = match authority.state().inventory(PLAYER) {
        Some(inventory) => inventory.slot(SlotIndex::new(0)),
        None => panic!("inventory missing after retry"),
    };
    assert!(matches!(slot, Ok(Some(stack)) if stack.quantity() == 1));
}

#[test]
fn voxel_and_drop_plan_fault_rolls_back_without_touching_loaded_revision() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    let position = BlockPosition {
        x: -1,
        y: -1,
        z: -1,
    };
    let fault_chunk = DimensionChunkKey::new(parsed("example:dimension/fault"), position.chunk());
    if let Err(error) = state.seed_loaded_chunk(fault_chunk.clone(), ChunkRevision::ZERO) {
        panic!("fault loaded chunk seed failed: {error}");
    }
    let target = BlockKey::new(fault_chunk.dimension.clone(), position);
    if let Err(error) = state.seed_block(target.clone(), parsed("example:block/log")) {
        panic!("fault block seed failed: {error}");
    }
    let before = state.clone();
    let mut authority = applier(state, &catalog);
    let command = envelope(
        authority.state(),
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target,
            expected_chunk_revision: ChunkRevision::ZERO,
            tool_slot: None,
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    );
    assert!(matches!(
        authority.execute(&command, FaultInjection::AfterMutation(nz16(2)),),
        Err(GameplayReject::InjectedFault { after_mutation: 2 })
    ));
    assert_eq!(authority.state(), &before);
    assert_eq!(authority.state().canonical_hash(), before.canonical_hash());
}

#[test]
fn persistent_tool_state_must_match_definition_and_durability_range() {
    let catalog = catalog();
    let plain_tool = plain("example:item/pickaxe", 1);
    assert!(matches!(
        catalog.validate_stack(&plain_tool),
        Err(GameplayReject::ItemStateMismatch { .. })
    ));
    let mut decoded_plain = state_with_inventory(1);
    seed_stack(&mut decoded_plain, PLAYER, 0, plain_tool);
    assert!(matches!(
        ReferencePlanApplier::try_new(fixture_world(), decoded_plain, catalog.clone()),
        Err(GameplayReject::ItemStateMismatch { .. })
    ));
    let excessive = match ItemStackV1::tool(parsed("example:item/pickaxe"), 11) {
        Ok(value) => value,
        Err(error) => panic!("excessive durability fixture failed: {error}"),
    };
    assert!(matches!(
        catalog.validate_stack(&excessive),
        Err(GameplayReject::DurabilityOutOfRange {
            remaining: 11,
            maximum: 10,
            ..
        })
    ));
    let mut decoded_excessive = state_with_inventory(1);
    seed_stack(&mut decoded_excessive, PLAYER, 0, excessive);
    assert!(matches!(
        ReferencePlanApplier::try_new(fixture_world(), decoded_excessive, catalog),
        Err(GameplayReject::DurabilityOutOfRange { .. })
    ));
}

#[test]
fn full_inventory_pickup_and_stale_revision_are_atomic_rejections() {
    let catalog = catalog();
    let mut state = state_with_inventory(1);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 1));
    let mut other = match InventoryStateV1::empty(persistent_target(fixture_chunk()), 1) {
        Ok(value) => value,
        Err(error) => panic!("other inventory failed: {error}"),
    };
    if let Err(error) = other.seed_slot(
        SlotIndex::new(0),
        Some(plain("other:item/broad-material-only", 64)),
    ) {
        panic!("other inventory slot failed: {error}");
    }
    if let Err(error) = state.seed_player(OTHER_PLAYER, other) {
        panic!("other player seed failed: {error}");
    }
    let mut authority = applier(state, &catalog);
    let dropped = drop_from(&execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::DropItem(DropItemCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            slot: SlotIndex::new(0),
            quantity: nz32(1),
            location: block_key(&fixture_dimension(), BlockPosition { x: 0, y: 0, z: 0 }),
        }),
    ));
    let before_pickup = authority.state().clone();
    let pickup = envelope(
        authority.state(),
        GameplayCommandV1::Pickup(PickupCommandV1 {
            player: OTHER_PLAYER,
            drop: dropped,
        }),
    );
    assert!(matches!(
        authority.execute(&pickup, FaultInjection::None),
        Err(GameplayReject::InventoryFull)
    ));
    assert_eq!(authority.state(), &before_pickup);

    let mut stale_envelope = pickup;
    stale_envelope.transaction_id = TransactionId::from_bytes([99; 16]);
    stale_envelope.expected_world_revision = WorldRevision::ZERO;
    assert!(matches!(
        authority.execute(&stale_envelope, FaultInjection::None),
        Err(GameplayReject::StaleWorldRevision { .. })
    ));
}

#[test]
fn receipt_horizon_is_bounded_and_old_replay_fails_closed() {
    let limits = GameplayLimits {
        recent_receipts: 1,
        ..GameplayLimits::default()
    };
    let mut state = match ReferenceGameplayState::new(limits) {
        Ok(value) => value,
        Err(error) => panic!("bounded state failed: {error}"),
    };
    let mut inventory = match InventoryStateV1::empty(persistent_target(fixture_chunk()), 2) {
        Ok(value) => value,
        Err(error) => panic!("bounded inventory failed: {error}"),
    };
    if let Err(error) = inventory.seed_slot(SlotIndex::new(0), Some(plain("example:item/log", 2))) {
        panic!("bounded inventory seed failed: {error}");
    }
    if let Err(error) = state.seed_loaded_chunk(fixture_chunk(), ChunkRevision::ZERO) {
        panic!("bounded loaded chunk seed failed: {error}");
    }
    if let Err(error) = state.seed_player(PLAYER, inventory) {
        panic!("bounded player seed failed: {error}");
    }
    let catalog = catalog();
    let mut authority = applier(state, &catalog);
    let first = envelope(
        authority.state(),
        GameplayCommandV1::DropItem(DropItemCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            slot: SlotIndex::new(0),
            quantity: nz32(1),
            location: block_key(&fixture_dimension(), BlockPosition { x: 0, y: 0, z: 0 }),
        }),
    );
    let first_receipt = match authority.execute_committed(&catalog, &first) {
        Ok(value) => value,
        Err(error) => panic!("first bounded command failed: {error}"),
    };

    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Pickup(PickupCommandV1 {
            player: PLAYER,
            drop: drop_from(&first_receipt),
        }),
    );
    assert!(matches!(
        authority.execute(&first, FaultInjection::None),
        Err(GameplayReject::RetryWindowExpired {
            oldest_replayable_world_revision: 1,
            ..
        })
    ));
}

#[test]
fn catalog_and_scheduler_hard_limits_accept_boundary_and_reject_plus_one() {
    let source = GameplayCatalogSourceV1 {
        items: vec![
            ItemDefinitionV1 {
                id: parsed("example:item/a"),
                stack_limit: nz32(1),
                placement_block: None,
                durability: None,
            },
            ItemDefinitionV1 {
                id: parsed("example:item/b"),
                stack_limit: nz32(1),
                placement_block: None,
                durability: None,
            },
        ],
        tags: vec![ItemTagDefinitionV1 {
            id: parsed("example:item-tag/pair@1"),
            members: vec![parsed("example:item/a"), parsed("example:item/b")].into_boxed_slice(),
        }],
        ..GameplayCatalogSourceV1::default()
    };
    let limits = CatalogLimits {
        tag_members: 1,
        ..CatalogLimits::default()
    };
    assert!(matches!(
        GameplayCatalog::compile(source, limits),
        Err(GameplayReject::LimitExceeded {
            resource: "catalog_tag_members",
            limit: 1,
            actual: 2
        })
    ));

    let catalog = catalog();
    let limits = GameplayLimits {
        scheduled_completions: 1,
        ..GameplayLimits::default()
    };
    let mut state = match ReferenceGameplayState::new(limits) {
        Ok(value) => value,
        Err(error) => panic!("scheduler limit state failed: {error}"),
    };
    if let Err(error) = state.seed_loaded_chunk(fixture_chunk(), ChunkRevision::ZERO) {
        panic!("scheduler loaded chunk failed: {error}");
    }
    if let Err(error) = state.seed_player(
        PLAYER,
        InventoryStateV1::empty(persistent_target(fixture_chunk()), 1)
            .unwrap_or_else(|error| panic!("inventory failed: {error}")),
    ) {
        panic!("scheduler player failed: {error}");
    }
    let mut authority = applier(state, &catalog);
    let command = envelope(
        authority.state(),
        GameplayCommandV1::AdvanceScheduled(ScheduledAdvanceCommandV1 {
            through_tick: AuthorityTick::new(0),
            max_completions: nz16(2),
        }),
    );
    assert!(matches!(
        authority.execute(&command, FaultInjection::None),
        Err(GameplayReject::LimitExceeded {
            resource: "scheduled_completions",
            limit: 1,
            actual: 2
        })
    ));
}

#[test]
fn source_and_member_permutation_produce_identical_intents_and_state() {
    let source = catalog_source();
    let mut permuted = source.clone();
    permuted.items.reverse();
    permuted.blocks.reverse();
    permuted.tools.reverse();
    permuted.tags.reverse();
    for tag in &mut permuted.tags {
        tag.members.reverse();
    }
    permuted.roles.reverse();
    permuted.bindings.reverse();
    permuted.recipes.reverse();
    permuted.workstations.reverse();
    permuted.processes.reverse();
    permuted.fuel_rules.reverse();
    let first = match GameplayCatalog::compile(source, CatalogLimits::default()) {
        Ok(value) => value,
        Err(error) => panic!("first catalog failed: {error}"),
    };
    let second = match GameplayCatalog::compile(permuted, CatalogLimits::default()) {
        Ok(value) => value,
        Err(error) => panic!("permuted catalog failed: {error}"),
    };
    assert_eq!(first, second);

    let mut initial = state_with_inventory(2);
    seed_stack(
        &mut initial,
        PLAYER,
        0,
        plain("other:item/compatible-log", 1),
    );
    let command = GameplayCommandV1::Craft(RecipeCraftCommandV1 {
        player: PLAYER,
        recipe: parsed("example:recipe/planks@1"),
        input_slots: vec![SlotIndex::new(0)].into_boxed_slice(),
        workstation: None,
    });
    let mut left = applier(initial.clone(), &first);
    let mut right = applier(initial, &second);
    let left_receipt = execute(&mut left, &first, command.clone());
    let right_receipt = execute(&mut right, &second, command);
    assert_eq!(
        left_receipt.plan_fingerprint,
        right_receipt.plan_fingerprint
    );
    assert_eq!(
        left.state().canonical_hash(),
        right.state().canonical_hash()
    );
}

#[test]
fn another_dimension_reuses_all_mechanics_without_domain_defaults() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    seed_stack(&mut state, PLAYER, 0, plain("other:item/compatible-log", 1));
    let mut authority = applier(state, &catalog);
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Craft(RecipeCraftCommandV1 {
            player: PLAYER,
            recipe: parsed("example:recipe/planks@1"),
            input_slots: vec![SlotIndex::new(0)].into_boxed_slice(),
            workstation: None,
        }),
    );
    let output = match authority.state().inventory(PLAYER) {
        Some(inventory) => inventory.slot(SlotIndex::new(0)),
        None => panic!("fixture-dimension inventory missing"),
    };
    assert!(matches!(output, Ok(Some(stack)) if stack.item().namespace() == "example"));
}

#[test]
fn dimension_qualified_block_keys_do_not_alias() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    let position = BlockPosition { x: 2, y: 3, z: 4 };
    let fixture_key = block_key(&fixture_dimension(), position);
    let other_dimension: DimensionId = parsed("other:dimension/parallel");
    let other_key = block_key(&other_dimension, position);
    if let Err(error) = state.seed_loaded_chunk(other_key.chunk(), ChunkRevision::ZERO) {
        panic!("parallel dimension chunk failed: {error}");
    }
    if let Err(error) = state.seed_block(fixture_key.clone(), parsed("example:block/log")) {
        panic!("fixture-dimension block failed: {error}");
    }
    if let Err(error) = state.seed_block(other_key.clone(), parsed("example:block/plank")) {
        panic!("parallel-dimension block failed: {error}");
    }

    let mut authority = applier(state, &catalog);
    let receipt = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: fixture_key.clone(),
            expected_chunk_revision: ChunkRevision::ZERO,
            tool_slot: None,
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    );
    assert!(matches!(
        receipt.outcome,
        CommandOutcomeV1::BlockBroken { ref affected_chunk, .. }
            if affected_chunk == &fixture_key.chunk()
    ));
    assert!(authority.state().block(&fixture_key).is_none());
    assert_eq!(
        authority.state().block(&other_key).map(BlockId::as_str),
        Some("example:block/plank")
    );
    assert_eq!(
        authority.state().loaded_chunk_revision(&other_key.chunk()),
        Some(ChunkRevision::ZERO)
    );
}

#[test]
fn loaded_empty_cell_is_distinct_from_an_unloaded_cell() {
    let catalog = catalog();
    let mut unloaded = state_with_inventory(2);
    seed_stack(&mut unloaded, PLAYER, 0, plain("example:item/plank", 1));
    let target = block_key(&fixture_dimension(), BlockPosition { x: 32, y: 0, z: 0 });
    let place = GameplayCommandV1::Place(PlaceCommandV1 {
        player: PLAYER,
        slot: SlotIndex::new(0),
        target: target.clone(),
        expected_chunk_revision: ChunkRevision::ZERO,
    });
    let unloaded_hash = unloaded.canonical_hash();
    let mut unloaded_authority = applier(unloaded.clone(), &catalog);
    let unloaded_envelope = envelope(unloaded_authority.state(), place.clone());
    assert!(matches!(
        unloaded_authority.execute(&unloaded_envelope, FaultInjection::None),
        Err(GameplayReject::ChunkNotLoaded { .. })
    ));

    let mut loaded = unloaded;
    if let Err(error) = state_seed_loaded_target(&mut loaded, &target, ChunkRevision::ZERO) {
        panic!("empty target chunk failed: {error}");
    }
    assert_ne!(loaded.canonical_hash(), unloaded_hash);
    let mut loaded_authority = applier(loaded, &catalog);
    let loaded_envelope = envelope(loaded_authority.state(), place);
    if let Err(error) = loaded_authority.execute(&loaded_envelope, FaultInjection::None) {
        panic!("placement into a loaded empty cell failed: {error}");
    }
    assert_eq!(
        loaded_authority.state().block(&target).map(BlockId::as_str),
        Some("example:block/plank")
    );
}

#[test]
fn gameplay_rules_keep_survival_costs_and_authorize_creative_pick() {
    let catalog = catalog();
    let mut initial = state_with_inventory(9);
    seed_stack(&mut initial, PLAYER, 0, plain("example:item/plank", 2));
    let target = block_key(&fixture_dimension(), BlockPosition { x: 3, y: 1, z: 0 });
    let place = GameplayCommandV1::Place(PlaceCommandV1 {
        player: PLAYER,
        slot: SlotIndex::new(0),
        target: target.clone(),
        expected_chunk_revision: ChunkRevision::ZERO,
    });

    let mut survival = applier(initial.clone(), &catalog);
    execute(&mut survival, &catalog, place.clone());
    assert!(matches!(
        survival
            .state()
            .inventory(PLAYER)
            .and_then(|inventory| inventory.slot(SlotIndex::new(0)).ok().flatten()),
        Some(stack) if stack.quantity() == 1
    ));

    let rules = GameplayRulesV1 {
        player_mode: GameplayModeV1::Creative,
    };
    let mut creative = applier_with_rules(initial, &catalog, rules);
    execute(&mut creative, &catalog, place);
    assert!(matches!(
        creative
            .state()
            .inventory(PLAYER)
            .and_then(|inventory| inventory.slot(SlotIndex::new(0)).ok().flatten()),
        Some(stack) if stack.quantity() == 2
    ));

    let inventory_revision = creative.state().inventory(PLAYER).map_or_else(
        || panic!("creative inventory missing"),
        InventoryStateV1::revision,
    );
    let item: ItemId = parsed("example:item/log");
    let picked = execute(
        &mut creative,
        &catalog,
        GameplayCommandV1::CreativePick(CreativePickCommandV1 {
            player: PLAYER,
            item: item.clone(),
            slot: SlotIndex::new(1),
            expected_inventory_revision: inventory_revision,
        }),
    );
    assert!(matches!(
        picked.outcome,
        CommandOutcomeV1::CreativeStackPicked { slot, item: ref picked_item }
            if slot == SlotIndex::new(1) && picked_item == &item
    ));
    let stack_limit = catalog.item(&item).map_or_else(
        || panic!("creative item definition missing"),
        |item| item.stack_limit.get(),
    );
    assert!(matches!(
        creative
            .state()
            .inventory(PLAYER)
            .and_then(|inventory| inventory.slot(SlotIndex::new(1)).ok().flatten()),
        Some(stack) if stack.item() == &item && stack.quantity() == stack_limit
    ));

    let survival_inventory_revision = survival.state().inventory(PLAYER).map_or_else(
        || panic!("survival inventory missing"),
        InventoryStateV1::revision,
    );
    let rejected = GameplayCommandV1::CreativePick(CreativePickCommandV1 {
        player: PLAYER,
        item,
        slot: SlotIndex::new(1),
        expected_inventory_revision: survival_inventory_revision,
    });
    let rejected_envelope = envelope(survival.state(), rejected);
    assert!(matches!(
        survival.execute_committed(&catalog, &rejected_envelope),
        Err(GameplayReject::CreativeModeRequired)
    ));
}

fn state_seed_loaded_target(
    state: &mut ReferenceGameplayState,
    target: &BlockKey,
    revision: ChunkRevision,
) -> Result<(), GameplayReject> {
    state.seed_loaded_chunk(target.chunk(), revision)
}

#[test]
fn reference_state_hash_excludes_limits_but_covers_storage_observations() {
    let default_state = ReferenceGameplayState::new(GameplayLimits::default())
        .unwrap_or_else(|error| panic!("default hash state failed: {error}"));
    let alternate_limits = GameplayLimits {
        recent_receipts: 1,
        drops: 1,
        ..GameplayLimits::default()
    };
    let alternate_state = ReferenceGameplayState::new(alternate_limits)
        .unwrap_or_else(|error| panic!("alternate hash state failed: {error}"));
    assert_eq!(
        default_state.canonical_hash(),
        alternate_state.canonical_hash()
    );

    let mut observed = default_state.clone();
    if let Err(error) = observed.observe_world_revision(WorldRevision::new(1)) {
        panic!("world observation failed: {error}");
    }
    assert_ne!(default_state.canonical_hash(), observed.canonical_hash());
    assert!(matches!(
        observed.observe_world_revision(WorldRevision::ZERO),
        Err(GameplayReject::StaleWorldRevision { .. })
    ));

    let mut loaded = default_state.clone();
    if let Err(error) = loaded.seed_loaded_chunk(fixture_chunk(), ChunkRevision::ZERO) {
        panic!("loaded hash chunk failed: {error}");
    }
    assert_ne!(default_state.canonical_hash(), loaded.canonical_hash());

    let mut impossible_ledger = default_state;
    impossible_ledger
        .seed_loaded_chunk(fixture_chunk(), ChunkRevision::new(1))
        .unwrap_or_else(|error| panic!("impossible ledger chunk failed: {error}"));
    let catalog = catalog();
    assert!(matches!(
        ReferencePlanApplier::try_new(fixture_world(), impossible_ledger, catalog),
        Err(GameplayReject::MutationPreconditionFailed {
            resource: "loaded_chunk_revision"
        })
    ));
}

fn assert_cross_type_persistent_identifier_collision(
    state: &mut ReferenceGameplayState,
    player: PlayerId,
) {
    let colliding_container_id = ContainerId::from_bytes(player.as_bytes());
    let colliding_container = ContainerStateV1::empty(
        ContainerOwnerComponentV1 {
            dimension: fixture_dimension(),
            chunk: ChunkCoordinate::new(0, 0, 0),
            entity: PersistentEntityId::from_bytes(player.as_bytes()),
        },
        None,
        1,
    )
    .unwrap_or_else(|error| panic!("colliding identifier container failed: {error}"));
    assert!(matches!(
        state.seed_container(colliding_container_id, colliding_container),
        Err(GameplayReject::DuplicateStateKey {
            kind: "persistent_entity"
        })
    ));
}

#[test]
fn high_64_identifier_bits_affect_state_command_and_plan_but_not_reserved_drop() {
    let mut first_player_bytes = [0_u8; 16];
    first_player_bytes[0] = 1;
    first_player_bytes[15] = 7;
    let mut second_player_bytes = first_player_bytes;
    second_player_bytes[0] = 2;
    let first_player = PlayerId::from_bytes(first_player_bytes);
    let second_player = PlayerId::from_bytes(second_player_bytes);

    let mut first_state = ReferenceGameplayState::new(GameplayLimits::default())
        .unwrap_or_else(|error| panic!("first identifier state failed: {error}"));
    first_state
        .seed_loaded_chunk(fixture_chunk(), ChunkRevision::ZERO)
        .unwrap_or_else(|error| panic!("first identifier chunk failed: {error}"));
    first_state
        .seed_player(
            first_player,
            InventoryStateV1::empty(persistent_target(fixture_chunk()), 1)
                .unwrap_or_else(|error| panic!("first identifier inventory failed: {error}")),
        )
        .unwrap_or_else(|error| panic!("first identifier player failed: {error}"));
    assert_cross_type_persistent_identifier_collision(&mut first_state, first_player);
    let mut second_state = ReferenceGameplayState::new(GameplayLimits::default())
        .unwrap_or_else(|error| panic!("second identifier state failed: {error}"));
    second_state
        .seed_loaded_chunk(fixture_chunk(), ChunkRevision::ZERO)
        .unwrap_or_else(|error| panic!("second identifier chunk failed: {error}"));
    second_state
        .seed_player(
            second_player,
            InventoryStateV1::empty(persistent_target(fixture_chunk()), 1)
                .unwrap_or_else(|error| panic!("second identifier inventory failed: {error}")),
        )
        .unwrap_or_else(|error| panic!("second identifier player failed: {error}"));
    assert_ne!(first_state.canonical_hash(), second_state.canonical_hash());

    let catalog = catalog();
    let mut command_state = state_with_inventory(2);
    seed_stack(&mut command_state, PLAYER, 0, plain("example:item/log", 2));
    let colliding_drop = envelope(
        &command_state,
        GameplayCommandV1::DropItem(DropItemCommandV1 {
            reserved_drop: DropEntityId::from_bytes(PLAYER.as_bytes()),
            player: PLAYER,
            slot: SlotIndex::new(0),
            quantity: nz32(1),
            location: block_key(&fixture_dimension(), BlockPosition { x: 1, y: 0, z: 1 }),
        }),
    );
    assert!(matches!(
        GameplayKernel::new(&catalog).plan(&command_state, &colliding_drop),
        Err(GameplayReject::DuplicateStateKey {
            kind: "persistent_entity"
        })
    ));
    let host_reserved_drop = reserved_drop();
    let command = GameplayCommandV1::DropItem(DropItemCommandV1 {
        reserved_drop: host_reserved_drop,
        player: PLAYER,
        slot: SlotIndex::new(0),
        quantity: nz32(1),
        location: block_key(&fixture_dimension(), BlockPosition { x: 1, y: 0, z: 1 }),
    });
    let mut first_transaction_bytes = [0_u8; 16];
    first_transaction_bytes[0] = 1;
    first_transaction_bytes[15] = 9;
    let mut second_transaction_bytes = first_transaction_bytes;
    second_transaction_bytes[0] = 2;
    let first_envelope = CommandEnvelopeV1 {
        transaction_id: TransactionId::from_bytes(first_transaction_bytes),
        expected_world_revision: WorldRevision::ZERO,
        command: command.clone(),
    };
    let second_envelope = CommandEnvelopeV1 {
        transaction_id: TransactionId::from_bytes(second_transaction_bytes),
        expected_world_revision: WorldRevision::ZERO,
        command,
    };
    let mut first_authority = applier(command_state.clone(), &catalog);
    let mut second_authority = applier(command_state, &catalog);
    let first_receipt = first_authority
        .execute(&first_envelope, FaultInjection::None)
        .unwrap_or_else(|error| panic!("first high-bit command failed: {error}"));
    let second_receipt = second_authority
        .execute(&second_envelope, FaultInjection::None)
        .unwrap_or_else(|error| panic!("second high-bit command failed: {error}"));
    assert_ne!(
        first_receipt.envelope_fingerprint,
        second_receipt.envelope_fingerprint
    );
    assert_ne!(
        first_receipt.plan_fingerprint,
        second_receipt.plan_fingerprint
    );
    assert_eq!(drop_from(&first_receipt), host_reserved_drop);
    assert_eq!(drop_from(&second_receipt), host_reserved_drop);
}

fn mismatched_storage_receipts(
    transaction_id: TransactionId,
) -> [(&'static str, CommitReceipt); 4] {
    let other_world = parsed("018f1e2d-3c4b-4a59-8c6d-7e8f9012abce");
    let wrong_world = fixture_commit_receipt(
        &MemoryTransactionKernel::new(),
        transaction_id,
        other_world,
        WorldRevision::ZERO,
        vec![fixture_mutation(
            other_world,
            fixture_chunk(),
            ChunkRevisionExpectation::Absent,
            31,
        )],
    );
    let wrong_transaction = fixture_commit_receipt(
        &MemoryTransactionKernel::new(),
        distinct_transaction_id(transaction_id, 4),
        fixture_world(),
        WorldRevision::ZERO,
        vec![fixture_mutation(
            fixture_world(),
            fixture_chunk(),
            ChunkRevisionExpectation::Absent,
            32,
        )],
    );
    let wrong_chunk = fixture_commit_receipt(
        &MemoryTransactionKernel::new(),
        transaction_id,
        fixture_world(),
        WorldRevision::ZERO,
        vec![fixture_mutation(
            fixture_world(),
            DimensionChunkKey::new(fixture_dimension(), ChunkCoordinate::new(1, 0, 0)),
            ChunkRevisionExpectation::Absent,
            33,
        )],
    );
    let advanced_storage = MemoryTransactionKernel::new();
    fixture_commit_receipt(
        &advanced_storage,
        distinct_transaction_id(transaction_id, 8),
        fixture_world(),
        WorldRevision::ZERO,
        vec![fixture_mutation(
            fixture_world(),
            DimensionChunkKey::new(fixture_dimension(), ChunkCoordinate::new(2, 0, 0)),
            ChunkRevisionExpectation::Absent,
            34,
        )],
    );
    let wrong_world_revision = fixture_commit_receipt(
        &advanced_storage,
        transaction_id,
        fixture_world(),
        WorldRevision::new(1),
        vec![fixture_mutation(
            fixture_world(),
            fixture_chunk(),
            ChunkRevisionExpectation::Absent,
            35,
        )],
    );
    [
        ("world", wrong_world),
        ("transaction_id", wrong_transaction),
        ("unexpected_chunk", wrong_chunk),
        ("world_revision", wrong_world_revision),
    ]
}

fn assert_chunk_revision_mismatch_is_atomic(catalog: &GameplayCatalog) {
    let mut state = state_with_inventory(2);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 2));
    state
        .observe_world_revision(WorldRevision::new(1))
        .unwrap_or_else(|error| panic!("revision mismatch world seed failed: {error}"));
    let mut authority = applier(state, catalog);
    let command = envelope(
        authority.state(),
        GameplayCommandV1::DropItem(DropItemCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            slot: SlotIndex::new(0),
            quantity: nz32(1),
            location: block_key(&fixture_dimension(), BlockPosition { x: 0, y: 0, z: 0 }),
        }),
    );
    let plan = GameplayKernel::new(catalog)
        .plan(authority.state(), &command)
        .unwrap_or_else(|error| panic!("revision mismatch planning failed: {error}"));
    let domains = capture_domains(&plan);
    authority
        .apply_plan(plan, FaultInjection::None)
        .unwrap_or_else(|error| panic!("revision mismatch apply failed: {error}"));
    let pending_state = authority.state().clone();

    let storage = MemoryTransactionKernel::new();
    fixture_commit_receipt(
        &storage,
        distinct_transaction_id(command.transaction_id, 1),
        fixture_world(),
        WorldRevision::ZERO,
        vec![fixture_mutation(
            fixture_world(),
            fixture_chunk(),
            ChunkRevisionExpectation::Absent,
            41,
        )],
    );
    let wrong_revision = fixture_commit_receipt(
        &storage,
        command.transaction_id,
        fixture_world(),
        WorldRevision::new(1),
        vec![fixture_mutation(
            fixture_world(),
            fixture_chunk(),
            ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
            42,
        )],
    );
    assert!(matches!(
        authority.observe_storage_commit(&wrong_revision),
        Err(GameplayReject::StorageCommitMismatch {
            resource: "chunk_revision"
        })
    ));
    assert_eq!(authority.state(), &pending_state);
    authority.commit_domains(command.transaction_id, &domains);
    assert_eq!(
        authority.state().loaded_chunk_revision(&fixture_chunk()),
        Some(ChunkRevision::new(1))
    );
    assert_eq!(
        authority.state().observed_world_revision(),
        WorldRevision::new(2)
    );
}

#[test]
fn storage_commit_receipt_mismatches_are_atomic_and_keep_pending_barrier() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 2));
    let mut authority = applier(state, &catalog);
    let command = envelope(
        authority.state(),
        GameplayCommandV1::DropItem(DropItemCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            slot: SlotIndex::new(0),
            quantity: nz32(1),
            location: block_key(&fixture_dimension(), BlockPosition { x: 0, y: 0, z: 0 }),
        }),
    );
    let plan = GameplayKernel::new(&catalog)
        .plan(authority.state(), &command)
        .unwrap_or_else(|error| panic!("pending barrier planning failed: {error}"));
    let domains = capture_domains(&plan);
    let pending_receipt = authority
        .apply_plan(plan, FaultInjection::None)
        .unwrap_or_else(|error| panic!("pending barrier apply failed: {error}"));
    let pending_state = authority.state().clone();
    assert_eq!(
        authority
            .execute(&command, FaultInjection::None)
            .unwrap_or_else(|error| panic!("exact pending retry failed: {error}")),
        pending_receipt
    );
    let mut blocked = command.clone();
    blocked.transaction_id = distinct_transaction_id(command.transaction_id, 2);
    assert!(matches!(
        authority.execute(&blocked, FaultInjection::None),
        Err(GameplayReject::StorageCommitPending { .. })
    ));
    assert_eq!(authority.state(), &pending_state);

    for (resource, receipt) in mismatched_storage_receipts(command.transaction_id) {
        assert!(matches!(
            authority.observe_storage_commit(&receipt),
            Err(GameplayReject::StorageCommitMismatch { resource: actual })
                if actual == resource
        ));
        assert_eq!(authority.state(), &pending_state);
    }

    authority.commit_domains(command.transaction_id, &domains);
    assert_eq!(
        authority.state().loaded_chunk_revision(&fixture_chunk()),
        Some(ChunkRevision::new(1))
    );
    assert_eq!(
        authority.state().observed_world_revision(),
        WorldRevision::new(1)
    );
    assert_chunk_revision_mismatch_is_atomic(&catalog);
}
#[test]
fn direct_plan_retry_binds_both_envelope_and_plan_fingerprints() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 2));
    let transaction_id = TransactionId::from_bytes([11; 16]);
    let first_envelope = CommandEnvelopeV1 {
        transaction_id,
        expected_world_revision: WorldRevision::ZERO,
        command: GameplayCommandV1::DropItem(DropItemCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            slot: SlotIndex::new(0),
            quantity: nz32(1),
            location: block_key(&fixture_dimension(), BlockPosition { x: 1, y: 0, z: 1 }),
        }),
    };
    let mut changed_envelope = first_envelope.clone();
    changed_envelope.command = GameplayCommandV1::DropItem(DropItemCommandV1 {
        reserved_drop: reserved_drop(),
        player: PLAYER,
        slot: SlotIndex::new(0),
        quantity: nz32(2),
        location: block_key(&fixture_dimension(), BlockPosition { x: 1, y: 0, z: 1 }),
    });
    let kernel = GameplayKernel::new(&catalog);
    let first_plan = kernel
        .plan(&state, &first_envelope)
        .unwrap_or_else(|error| panic!("first direct plan failed: {error}"));
    let changed_plan = kernel
        .plan(&state, &changed_envelope)
        .unwrap_or_else(|error| panic!("changed direct plan failed: {error}"));
    let mut authority = applier(state, &catalog);
    let receipt = authority
        .apply_plan(first_plan.clone(), FaultInjection::None)
        .unwrap_or_else(|error| panic!("first direct apply failed: {error}"));
    let replayed = authority
        .apply_plan(first_plan, FaultInjection::None)
        .unwrap_or_else(|error| panic!("exact direct replay failed: {error}"));
    assert_eq!(receipt, replayed);
    assert!(matches!(
        authority.apply_plan(changed_plan, FaultInjection::None),
        Err(GameplayReject::RetryPayloadMismatch { .. })
    ));
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn drop_then_pickup_conserves_every_quantity(quantity in 1_u32..=64) {
        let catalog = catalog();
        let mut state = state_with_inventory(2);
        seed_stack(&mut state, PLAYER, 0, plain("example:item/log", quantity));
        let mut authority = applier(state, &catalog);
        let receipt = execute(
            &mut authority,
            &catalog,
            GameplayCommandV1::DropItem(DropItemCommandV1 {
            reserved_drop: reserved_drop(),
                player: PLAYER,
                slot: SlotIndex::new(0),
                quantity: nz32(quantity),
                location: block_key(
                    &fixture_dimension(),
                    BlockPosition { x: 1, y: 0, z: 1 },
                ),
            }),
        );
        let drop = drop_from(&receipt);
        execute(&mut authority, &catalog, GameplayCommandV1::Pickup(PickupCommandV1 { player: PLAYER, drop }));
        let Some(inventory) = authority.state().inventory(PLAYER) else {
            panic!("property inventory missing");
        };
        prop_assert!(matches!(inventory.slot(SlotIndex::new(0)), Ok(Some(stack)) if stack.quantity() == quantity));
        prop_assert!(authority.state().dropped_item(drop).is_none());
    }
}

#[test]
fn ten_thousand_command_soak_has_bounded_receipts_and_no_item_loss() {
    let catalog = catalog();
    let mut state = state_with_inventory(1);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 1));
    let mut authority = applier(state, &catalog);
    for index in 0..5_000_i32 {
        let receipt = execute(
            &mut authority,
            &catalog,
            GameplayCommandV1::DropItem(DropItemCommandV1 {
                reserved_drop: reserved_drop(),
                player: PLAYER,
                slot: SlotIndex::new(0),
                quantity: nz32(1),
                location: block_key(
                    &fixture_dimension(),
                    BlockPosition {
                        x: index.rem_euclid(16),
                        y: 0,
                        z: index.rem_euclid(16),
                    },
                ),
            }),
        );
        execute(
            &mut authority,
            &catalog,
            GameplayCommandV1::Pickup(PickupCommandV1 {
                player: PLAYER,
                drop: drop_from(&receipt),
            }),
        );
    }
    let Some(inventory) = authority.state().inventory(PLAYER) else {
        panic!("soak inventory missing");
    };
    assert!(matches!(inventory.slot(SlotIndex::new(0)), Ok(Some(stack)) if stack.quantity() == 1));
}

#[test]
fn move_stack_merges_matching_item_and_state_up_to_stack_limit() {
    let catalog = catalog();
    let mut state = state_with_inventory(4);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 40));
    seed_stack(&mut state, PLAYER, 1, plain("example:item/log", 40));
    let mut authority = applier(state, &catalog);
    let receipt = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::MoveStack(MoveStackCommandV1 {
            player: PLAYER,
            from: SlotIndex::new(0),
            to: SlotIndex::new(1),
            quantity: None,
            expected_inventory_revision: 0,
        }),
    );
    assert!(matches!(
        receipt.outcome,
        CommandOutcomeV1::StackMoved { from, to }
            if from == SlotIndex::new(0) && to == SlotIndex::new(1)
    ));
    let Some(inventory) = authority.state().inventory(PLAYER) else {
        panic!("move-stack merge inventory missing");
    };
    assert!(
        matches!(inventory.slot(SlotIndex::new(0)), Ok(Some(stack)) if stack.item().as_str() == "example:item/log" && stack.quantity() == 16)
    );
    assert!(
        matches!(inventory.slot(SlotIndex::new(1)), Ok(Some(stack)) if stack.item().as_str() == "example:item/log" && stack.quantity() == 64)
    );
}

#[test]
fn move_stack_swaps_different_items_when_quantity_is_the_full_stack() {
    let catalog = catalog();
    let mut state = state_with_inventory(4);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 8));
    seed_stack(&mut state, PLAYER, 1, plain("example:item/plank", 4));
    let mut authority = applier(state, &catalog);
    let receipt = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::MoveStack(MoveStackCommandV1 {
            player: PLAYER,
            from: SlotIndex::new(0),
            to: SlotIndex::new(1),
            quantity: None,
            expected_inventory_revision: 0,
        }),
    );
    assert!(matches!(
        receipt.outcome,
        CommandOutcomeV1::StackMoved { from, to }
            if from == SlotIndex::new(0) && to == SlotIndex::new(1)
    ));
    let Some(inventory) = authority.state().inventory(PLAYER) else {
        panic!("move-stack swap inventory missing");
    };
    assert!(
        matches!(inventory.slot(SlotIndex::new(0)), Ok(Some(stack)) if stack.item().as_str() == "example:item/plank" && stack.quantity() == 4)
    );
    assert!(
        matches!(inventory.slot(SlotIndex::new(1)), Ok(Some(stack)) if stack.item().as_str() == "example:item/log" && stack.quantity() == 8)
    );
}

#[test]
fn move_stack_rejects_empty_from() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    seed_stack(&mut state, PLAYER, 1, plain("example:item/log", 1));
    let mut authority = applier(state, &catalog);
    let before = authority.state().clone();
    let envelope = envelope(
        authority.state(),
        GameplayCommandV1::MoveStack(MoveStackCommandV1 {
            player: PLAYER,
            from: SlotIndex::new(0),
            to: SlotIndex::new(1),
            quantity: None,
            expected_inventory_revision: 0,
        }),
    );
    assert!(matches!(
        authority.execute(&envelope, FaultInjection::None),
        Err(GameplayReject::EmptySlot)
    ));
    assert_eq!(authority.state(), &before);
}

#[test]
fn move_stack_rejects_stale_inventory_revision() {
    let catalog = catalog();
    let mut state = state_with_inventory(2);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 1));
    let mut authority = applier(state, &catalog);
    let before = authority.state().clone();
    let envelope = envelope(
        authority.state(),
        GameplayCommandV1::MoveStack(MoveStackCommandV1 {
            player: PLAYER,
            from: SlotIndex::new(0),
            to: SlotIndex::new(1),
            quantity: None,
            expected_inventory_revision: 1,
        }),
    );
    assert!(matches!(
        authority.execute(&envelope, FaultInjection::None),
        Err(GameplayReject::StaleInventoryRevision {
            expected: 1,
            actual: 0
        })
    ));
    assert_eq!(authority.state(), &before);
}

#[test]
fn canonical_empty_state_hash_matches_golden() {
    let state = match ReferenceGameplayState::new(GameplayLimits::default()) {
        Ok(state) => state,
        Err(error) => panic!("golden state failed: {error}"),
    };
    assert_eq!(
        state.canonical_hash().to_hex(),
        "c8953c45654d8738433998af95652b44f2aa9d8788333ef8ebd780e02686ccd3"
    );
}

fn inventory_quantity(state: &ReferenceGameplayState, player: PlayerId) -> u64 {
    let Some(inventory) = state.inventory(player) else {
        panic!("quantity inventory missing");
    };
    inventory
        .slots()
        .iter()
        .flatten()
        .map(|stack| u64::from(stack.quantity()))
        .sum()
}

fn tool_stack(item: &str, durability: u32) -> ItemStackV1 {
    match ItemStackV1::tool(parsed(item), durability) {
        Ok(stack) => stack,
        Err(error) => panic!("fixture tool stack failed: {error}"),
    }
}

#[test]
fn inventory_hotbar_is_a_selected_prefix_not_a_second_array() {
    let inventory = match InventoryStateV1::empty(persistent_target(fixture_chunk()), 36) {
        Ok(inventory) => inventory,
        Err(error) => panic!("hotbar inventory failed: {error}"),
    };
    assert_eq!(inventory.hotbar_slots(), 9);
    assert_eq!(inventory.selected_hotbar(), SlotIndex::new(0));
    assert_eq!(inventory.hotbar().len(), 9);
    assert!(inventory.selected_stack().is_none());
    assert!(matches!(
        InventoryStateV1::empty_with_hotbar(persistent_target(fixture_chunk()), 8, 9),
        Err(GameplayReject::LimitExceeded {
            resource: "hotbar_slots",
            ..
        })
    ));
}

#[test]
fn select_hotbar_is_durable_and_conserves_every_stack() {
    let catalog = catalog();
    let mut state = state_with_inventory(9);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 2));
    seed_stack(&mut state, PLAYER, 3, plain("example:item/plank", 4));
    let before_hash = state.canonical_hash();
    let before_quantity = inventory_quantity(&state, PLAYER);
    let mut authority = applier(state, &catalog);
    let receipt = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::SelectHotbar(SelectHotbarCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(3),
            expected_inventory_revision: 0,
        }),
    );
    assert!(matches!(
        receipt.outcome,
        CommandOutcomeV1::HotbarSelected { slot } if slot == SlotIndex::new(3)
    ));
    let Some(inventory) = authority.state().inventory(PLAYER) else {
        panic!("hotbar inventory missing");
    };
    assert_eq!(inventory.selected_hotbar(), SlotIndex::new(3));
    assert_eq!(inventory.revision(), 1);
    assert_eq!(
        inventory.selected_stack().map(ItemStackV1::quantity),
        Some(4)
    );
    assert_eq!(
        inventory_quantity(authority.state(), PLAYER),
        before_quantity
    );
    assert_ne!(authority.state().canonical_hash(), before_hash);

    let same = execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::SelectHotbar(SelectHotbarCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(3),
            expected_inventory_revision: 1,
        }),
    );
    assert!(matches!(
        same.outcome,
        CommandOutcomeV1::HotbarSelected { .. }
    ));
    assert_eq!(
        authority
            .state()
            .inventory(PLAYER)
            .map(InventoryStateV1::revision),
        Some(1)
    );
}

#[test]
fn select_hotbar_rejects_body_slots_and_stale_revision_without_mutation() {
    let catalog = catalog();
    let mut state = state_with_inventory(12);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 1));
    let before = state.clone();
    let mut authority = applier(state, &catalog);
    let body = envelope(
        authority.state(),
        GameplayCommandV1::SelectHotbar(SelectHotbarCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(9),
            expected_inventory_revision: 0,
        }),
    );
    assert!(matches!(
        authority.execute(&body, FaultInjection::None),
        Err(GameplayReject::SlotOutOfRange { slot, slots: 9 }) if slot == SlotIndex::new(9)
    ));
    assert_eq!(authority.state(), &before);

    let stale_envelope = envelope(
        authority.state(),
        GameplayCommandV1::SelectHotbar(SelectHotbarCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(1),
            expected_inventory_revision: 7,
        }),
    );
    assert!(matches!(
        authority.execute(&stale_envelope, FaultInjection::None),
        Err(GameplayReject::StaleInventoryRevision {
            expected: 7,
            actual: 0
        })
    ));
    assert_eq!(authority.state(), &before);
}

#[test]
fn inspect_fragments_cover_inventory_recipe_machine_and_container() {
    let catalog = catalog();
    let mining = catalog
        .mining_inspect(&parsed("example:block/copper-ore"))
        .unwrap_or_else(|| panic!("ore mining inspect missing"));
    assert_eq!(mining.hardness_ticks(), 6);
    assert_eq!(
        mining.tool_class().map(ToolClassId::as_str),
        Some("latticeaxiom:tool-class/pickaxe@1")
    );
    assert_eq!(mining.minimum_tier(), Some(1));
    assert!(
        catalog
            .fuel_ticks(&parsed("example:item/charcoal"))
            .is_some()
    );
    assert!(
        catalog
            .fuel_ticks(&parsed("other:item/broad-material-only"))
            .is_none()
    );

    let mut state = state_with_inventory(8);
    seed_stack(&mut state, PLAYER, 0, plain("example:item/log", 1));
    seed_stack(&mut state, PLAYER, 1, plain("example:item/stick", 2));
    seed_workbench(&mut state);
    let kernel = GameplayKernel::new(&catalog);
    let inventory = kernel
        .inspect_inventory(&state, PLAYER)
        .unwrap_or_else(|error| panic!("inventory inspect failed: {error}"));
    assert_eq!(inventory.occupied_slots(), 2);
    assert_eq!(inventory.total_quantity(), 3);
    assert_eq!(inventory.hotbar_slots(), 8);
    let recipes = kernel
        .inspect_recipes(&state, PLAYER, None)
        .unwrap_or_else(|error| panic!("recipe inspect failed: {error}"));
    assert!(recipes.iter().any(|fragment| {
        fragment.recipe().as_str() == "example:recipe/planks@1" && fragment.craftable()
    }));
    assert!(recipes.iter().any(|fragment| {
        fragment.recipe().as_str() == "example:recipe/pickaxe@1" && !fragment.craftable()
    }));
    let workbench: WorkstationId = parsed("latticeaxiom:workstation/crafting@1");
    let craftable = kernel
        .craftable_recipes(&state, PLAYER, None)
        .unwrap_or_else(|error| panic!("craftable recipes failed: {error}"));
    assert_eq!(
        craftable.iter().map(RecipeId::as_str).collect::<Vec<_>>(),
        vec!["example:recipe/planks@1"]
    );
    let bound = kernel
        .inspect_recipes(&state, PLAYER, Some(&workbench))
        .unwrap_or_else(|error| panic!("bound recipe inspect failed: {error}"));
    assert!(bound.iter().any(|fragment| {
        fragment.recipe().as_str() == "example:recipe/pickaxe@1" && !fragment.craftable()
    }));
    let container = kernel
        .inspect_container(&state, WORKBENCH_CONTAINER)
        .unwrap_or_else(|error| panic!("container inspect failed: {error}"));
    assert_eq!(container.workstation(), Some(&workbench));
    assert!(kernel.inspect_furnace(&state, FURNACE_CONTAINER).is_none());
}

#[test]
fn wrong_tool_and_exhausted_tool_are_atomic() {
    let catalog = catalog();
    let dimension = fixture_dimension();
    let ore = BlockPosition { x: 4, y: 8, z: 0 };
    let log = BlockPosition { x: 5, y: 8, z: 0 };
    let mut state = state_with_inventory(4);
    seed_stack(&mut state, PLAYER, 0, tool_stack("example:item/pickaxe", 1));
    if let Err(error) = state.seed_block(
        block_key(&dimension, ore),
        parsed("example:block/copper-ore"),
    ) {
        panic!("wrong-tool ore failed: {error}");
    }
    if let Err(error) = state.seed_block(block_key(&dimension, log), parsed("example:block/log")) {
        panic!("wrong-tool log failed: {error}");
    }
    let before = state.clone();
    let mut authority = applier(state, &catalog);
    let missing_tool = envelope(
        authority.state(),
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, ore),
            expected_chunk_revision: ChunkRevision::ZERO,
            tool_slot: None,
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    );
    assert!(matches!(
        authority.execute(&missing_tool, FaultInjection::None),
        Err(GameplayReject::ToolRequired)
    ));
    assert_eq!(authority.state(), &before);

    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, log),
            expected_chunk_revision: ChunkRevision::ZERO,
            tool_slot: Some(SlotIndex::new(0)),
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    );
    let Some(inventory) = authority.state().inventory(PLAYER) else {
        panic!("exhausted-tool inventory missing");
    };
    assert!(matches!(inventory.slot(SlotIndex::new(0)), Ok(None)));
    let after_break = authority.state().clone();
    let exhausted = envelope(
        authority.state(),
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, ore),
            expected_chunk_revision: authority
                .state()
                .loaded_chunk_revision(&fixture_chunk())
                .unwrap_or_else(|| panic!("exhausted-tool chunk missing")),
            tool_slot: Some(SlotIndex::new(0)),
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    );
    assert!(matches!(
        authority.execute(&exhausted, FaultInjection::None),
        Err(GameplayReject::EmptySlot)
    ));
    assert_eq!(authority.state(), &after_break);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the fixture-dimension journey is kept as one auditable command sequence"
)]
fn fixture_dimension_reuses_gather_craft_mine_place_without_terrenia_ids() {
    let catalog = catalog();
    assert!(
        catalog
            .items()
            .keys()
            .all(|item| item.namespace() != "terrenia")
    );
    assert!(
        catalog
            .blocks()
            .keys()
            .all(|block| block.namespace() != "terrenia")
    );
    assert!(
        catalog
            .recipes()
            .keys()
            .all(|recipe| recipe.namespace() != "terrenia")
    );

    let dimension: DimensionId = parsed("other:dimension/sandbox");
    let chunk = DimensionChunkKey::new(dimension.clone(), ChunkCoordinate::new(0, 0, 0));
    let mut state = match ReferenceGameplayState::new(GameplayLimits::default()) {
        Ok(state) => state,
        Err(error) => panic!("fixture-dimension state failed: {error}"),
    };
    if let Err(error) = state.seed_loaded_chunk(chunk.clone(), ChunkRevision::ZERO) {
        panic!("fixture-dimension chunk failed: {error}");
    }
    let mut inventory = match InventoryStateV1::empty(
        GameplayEditTarget::new(chunk.clone(), GameplayStorageDomain::PersistentEntities),
        9,
    ) {
        Ok(inventory) => inventory,
        Err(error) => panic!("fixture-dimension inventory failed: {error}"),
    };
    if let Err(error) = inventory.seed_slot(SlotIndex::new(1), Some(plain("example:item/stick", 2)))
    {
        panic!("fixture-dimension stick failed: {error}");
    }
    if let Err(error) = state.seed_player(PLAYER, inventory) {
        panic!("fixture-dimension player failed: {error}");
    }
    let workbench = match ContainerStateV1::empty(
        ContainerOwnerComponentV1 {
            dimension: dimension.clone(),
            chunk: chunk.coordinate,
            entity: WORKBENCH_CONTAINER.into_persistent_entity_id(),
        },
        Some(parsed("latticeaxiom:workstation/crafting@1")),
        9,
    ) {
        Ok(container) => container,
        Err(error) => panic!("fixture-dimension workbench failed: {error}"),
    };
    if let Err(error) = state.seed_container(WORKBENCH_CONTAINER, workbench) {
        panic!("fixture-dimension workbench seed failed: {error}");
    }
    let log_position = BlockPosition { x: 0, y: 8, z: 0 };
    let ore_position = BlockPosition { x: 1, y: 8, z: 0 };
    if let Err(error) = state.seed_block(
        block_key(&dimension, log_position),
        parsed("example:block/log"),
    ) {
        panic!("fixture-dimension log failed: {error}");
    }
    if let Err(error) = state.seed_block(
        block_key(&dimension, ore_position),
        parsed("example:block/copper-ore"),
    ) {
        panic!("fixture-dimension ore failed: {error}");
    }

    let mut authority = applier(state, &catalog);
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::SelectHotbar(SelectHotbarCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(1),
            expected_inventory_revision: 0,
        }),
    );
    let log_revision = authority
        .state()
        .loaded_chunk_revision(&chunk)
        .unwrap_or_else(|| panic!("fixture-dimension log chunk missing"));
    let log_drop = drop_from(&execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, log_position),
            expected_chunk_revision: log_revision,
            tool_slot: None,
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    ));
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Pickup(PickupCommandV1 {
            player: PLAYER,
            drop: log_drop,
        }),
    );
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Craft(RecipeCraftCommandV1 {
            player: PLAYER,
            recipe: parsed("example:recipe/planks@1"),
            input_slots: vec![SlotIndex::new(0)].into_boxed_slice(),
            workstation: None,
        }),
    );
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Craft(RecipeCraftCommandV1 {
            player: PLAYER,
            recipe: parsed("example:recipe/pickaxe@1"),
            input_slots: vec![SlotIndex::new(0), SlotIndex::new(1)].into_boxed_slice(),
            workstation: Some(WORKBENCH_CONTAINER),
        }),
    );
    let ore_revision = authority
        .state()
        .loaded_chunk_revision(&chunk)
        .unwrap_or_else(|| panic!("fixture-dimension ore chunk missing"));
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, ore_position),
            expected_chunk_revision: ore_revision,
            tool_slot: Some(SlotIndex::new(1)),
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    );
    let ore_revision = authority
        .state()
        .loaded_chunk_revision(&chunk)
        .unwrap_or_else(|| panic!("fixture-dimension ore chunk missing"));
    let ore_drop = drop_from(&execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: reserved_drop(),
            player: PLAYER,
            target: block_key(&dimension, ore_position),
            expected_chunk_revision: ore_revision,
            tool_slot: Some(SlotIndex::new(1)),
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    ));
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Pickup(PickupCommandV1 {
            player: PLAYER,
            drop: ore_drop,
        }),
    );
    let hotbar_revision = authority.state().inventory(PLAYER).map_or_else(
        || panic!("fixture-dimension inventory missing"),
        InventoryStateV1::revision,
    );
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::SelectHotbar(SelectHotbarCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(0),
            expected_inventory_revision: hotbar_revision,
        }),
    );
    let place_target = block_key(&dimension, BlockPosition { x: 2, y: 8, z: 0 });
    let place_revision = authority
        .state()
        .loaded_chunk_revision(&chunk)
        .unwrap_or_else(|| panic!("fixture-dimension place chunk missing"));
    execute(
        &mut authority,
        &catalog,
        GameplayCommandV1::Place(PlaceCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(0),
            target: place_target.clone(),
            expected_chunk_revision: place_revision,
        }),
    );
    assert_eq!(
        authority.state().block(&place_target).map(BlockId::as_str),
        Some("example:block/plank")
    );
    let Some(inventory) = authority.state().inventory(PLAYER) else {
        panic!("fixture-dimension inventory missing after place");
    };
    assert_eq!(inventory.selected_hotbar(), SlotIndex::new(0));
    let tool = match inventory.slot(SlotIndex::new(1)) {
        Ok(Some(tool)) => tool,
        other => panic!("fixture-dimension tool missing: {other:?}"),
    };
    assert!(matches!(
        tool.state(),
        ItemStateV1::ToolDurability { remaining } if remaining.get() == 9
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn move_stack_conserves_quantity_across_merge_and_swap(
        left in 1_u32..=40,
        right in 1_u32..=24,
    ) {
        let catalog = catalog();
        let mut state = state_with_inventory(4);
        seed_stack(&mut state, PLAYER, 0, plain("example:item/log", left));
        seed_stack(&mut state, PLAYER, 1, plain("example:item/plank", right));
        let before = inventory_quantity(&state, PLAYER);
        let mut authority = applier(state, &catalog);
        execute(
            &mut authority,
            &catalog,
            GameplayCommandV1::MoveStack(MoveStackCommandV1 {
                player: PLAYER,
                from: SlotIndex::new(0),
                to: SlotIndex::new(2),
                quantity: None,
                expected_inventory_revision: 0,
            }),
        );
        execute(
            &mut authority,
            &catalog,
            GameplayCommandV1::MoveStack(MoveStackCommandV1 {
                player: PLAYER,
                from: SlotIndex::new(1),
                to: SlotIndex::new(2),
                quantity: None,
                expected_inventory_revision: 1,
            }),
        );
        prop_assert_eq!(inventory_quantity(authority.state(), PLAYER), before);
        prop_assert_eq!(authority.state().dropped_items().len(), 0);
    }
}
