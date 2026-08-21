//! Bounded, deterministic contracts for authoritative sandbox gameplay.
//!
//! The crate owns no Bevy app, scheduler, clock, thread pool, world writer,
//! revision allocator, or persistence codec. [`GameplayKernel`] validates a
//! versioned command against a catalog-bound [`ReferenceGameplayState`] and
//! produces a bounded [`GameplayPlanV1`] whose edits have explicit
//! dimension-qualified chunk/domain targets.
//!
//! A gameplay plan is only a runtime-staged edit plan. The host applies it to
//! validated loaded chunks and then captures complete chunk replacements in one
//! [`latticeaxiom_storage::WorldTransaction`]. Storage alone advances world and
//! chunk revisions and publishes durability. [`ReferencePlanApplier`] proves
//! in-memory atomic apply, rollback, and retry behavior; its
//! [`RuntimePlanReceiptV1`] is not a storage receipt.
//!
//! [`GameplayCatalog`] intentionally uses typed raw stable-ID text keys as a
//! reference oracle. Production registries may intern them while preserving the
//! same identity and order-independent semantics. Recipes accept exact item
//! identities, semantic tags, or closed predicates, while every output is
//! resolved through a frozen role binding before an edit is staged.
//!
//! Versioned schema identities in this crate are reserved. No gameplay
//! world-wire codec is registered here, so durable unload/reload, crash recovery,
//! and decoded-snapshot compatibility remain integration gates.

mod catalog;
mod hash;
mod id;
mod kernel;
mod model;
mod storage_scope;

pub use catalog::{
    BlockDefinitionV1, BlockSchemaBindingV1, CatalogLimits, FrozenItemRoleBindingV1, FuelRuleV1,
    GameplayCatalog, GameplayCatalogSourceV1, IngredientV1, ItemDefinitionV1, ItemPredicateV1,
    ItemRoleDefinitionV1, ItemTagDefinitionV1, MiningRuleV1, ProcessDefinitionV1,
    RecipeDefinitionV1, RecipePatternV1, RoleOutputV1, ToolDefinitionV1, ToolRequirementV1,
    WorkstationDefinitionV1, is_reserved_gameplay_schema,
};
pub use hash::{CommandFingerprintV1, GameplayPlanHashV1, ReferenceGameplayStateHashV1};
pub use id::{
    BlockId, GameplayIdError, ItemId, ItemRoleId, ItemTagId, ProcessId, RecipeId, ToolClassId,
    WorkstationId,
};
pub use kernel::{GameplayKernel, ReferencePlanApplier};
pub use latticeaxiom_core::{SchemaId, WorldId};
pub use latticeaxiom_storage::{
    ChangedDomains, ChunkCoordinate, ChunkRevision, CommitReceipt, ContinuationId, DimensionId,
    PersistentEntityId, TransactionId, WorldRevision,
};
pub use model::{
    AuthorityTick, BlockPosition, BreakProgressKey, BreakProgressV1, CommandEnvelopeV1,
    CommandOutcomeV1, ContainerOwnerComponentV1, ContainerStateV1, DropItemCommandV1,
    DroppedItemV1, FaultInjection, FurnaceContinuationV1, GameplayCommandV1, GameplayLimits,
    GameplayMutationIntentV1, GameplayPlanV1, GameplayReject, InventoryStateV1, ItemStackV1,
    ItemStateV1, MineCommandV1, PickupCommandV1, PlaceCommandV1, RecipeCraftCommandV1,
    ReferenceGameplayState, RuntimePlanReceiptV1, ScheduledAdvanceCommandV1, SlotIndex,
    StartProcessCommandV1, TransferCommandV1, TransferDirectionV1,
};
pub use storage_scope::{
    BlockKey, ContainerId, DimensionChunkKey, DropEntityId, GameplayEditTarget,
    GameplayStorageDomain, PlayerId,
};
