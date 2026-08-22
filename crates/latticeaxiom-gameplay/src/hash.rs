use std::fmt;

use sha2::{Digest, Sha256};

use crate::{
    BlockKey, CommandEnvelopeV1, CommandOutcomeV1, ContainerOwnerComponentV1, DimensionChunkKey,
    DropEntityId, GameplayCommandV1, GameplayEditTarget, GameplayMutationIntentV1,
    InventoryStateV1, ItemStackV1, ItemStateV1, ReferenceGameplayState, TransferDirectionV1,
};

macro_rules! digest_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Creates a digest from canonical bytes.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            /// Returns canonical bytes.
            #[must_use]
            pub const fn as_bytes(self) -> [u8; 32] {
                self.0
            }

            /// Returns lowercase hexadecimal text.
            #[must_use]
            pub fn to_hex(self) -> String {
                const DIGITS: &[u8; 16] = b"0123456789abcdef";
                let mut output = String::with_capacity(64);
                for byte in self.0 {
                    output.push(char::from(DIGITS[usize::from(byte >> 4)]));
                    output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
                }
                output
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($name))
                    .field(&self.to_hex())
                    .finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.to_hex())
            }
        }
    };
}

digest_type!(
    /// Domain-separated fingerprint of a canonical gameplay command envelope.
    CommandFingerprintV1
);
digest_type!(
    /// Domain-separated hash of one complete runtime-staged gameplay plan.
    GameplayPlanHashV1
);
digest_type!(
    /// Deterministic projection hash of validated reference gameplay state.
    ///
    /// This is not a complete authoritative-world hash or canonical snapshot bytes.
    ReferenceGameplayStateHashV1
);
pub(crate) fn state_hash(state: &ReferenceGameplayState) -> ReferenceGameplayStateHashV1 {
    let mut encoder = Encoder::new(b"latticeaxiom.gameplay-reference-state.v2\0");
    encoder.u64(state.observed_world_revision.get());
    encoder.len(state.loaded_chunks.len());
    for (chunk, revision) in &state.loaded_chunks {
        encoder.chunk_key(chunk);
        encoder.u64(revision.get());
    }
    encoder.len(state.inventories.len());
    for (player, inventory) in &state.inventories {
        encoder.bytes(&player.as_bytes());
        encoder.inventory(inventory);
    }
    encoder.len(state.blocks.len());
    for (key, block) in &state.blocks {
        encoder.block_key(key);
        encoder.text(block.as_str());
    }
    encoder.len(state.drops.len());
    for (id, drop) in &state.drops {
        encoder.drop_id(*id);
        encoder.block_key(&drop.location);
        encoder.stack(&drop.stack);
    }
    encoder.len(state.containers.len());
    for (id, container) in &state.containers {
        encoder.bytes(&id.as_bytes());
        encoder.container_owner(&container.owner);
        encoder.optional_text(
            container
                .workstation
                .as_ref()
                .map(crate::WorkstationId::as_str),
        );
        encoder.u64(container.revision);
        encoder.len(container.slots.len());
        for slot in &container.slots {
            encoder.stack_option(slot.as_ref());
        }
    }
    encoder.len(state.break_progress.len());
    for (key, progress) in &state.break_progress {
        encoder.bytes(&key.player.as_bytes());
        encoder.block_key(&key.block);
        encoder.text(progress.block.as_str());
        encoder.u32(progress.accumulated_work);
    }
    encoder.len(state.continuations.len());
    for (id, continuation) in &state.continuations {
        encoder.bytes(id.as_bytes());
        encoder.text(continuation.process.as_str());
        encoder.bytes(&continuation.container.as_bytes());
        encoder.target(&continuation.target);
        encoder.u16(continuation.output_slot.get());
        encoder.stack(&continuation.pending_output);
        encoder.u64(continuation.due_tick.get());
        encoder.u64(continuation.revision);
    }
    // GameplayLimits and retained retry receipts are reference-oracle policy/
    // operational metadata and intentionally excluded from this projection.
    ReferenceGameplayStateHashV1::from_bytes(encoder.finish_bytes())
}

pub(crate) fn envelope_hash(envelope: &CommandEnvelopeV1) -> CommandFingerprintV1 {
    let mut encoder = Encoder::new(b"latticeaxiom.gameplay-command-envelope.v1\0");
    encoder.bytes(envelope.transaction_id.as_bytes());
    encoder.u64(envelope.expected_world_revision.get());
    encoder.command(&envelope.command);
    CommandFingerprintV1::from_bytes(encoder.finish_bytes())
}

pub(crate) fn plan_hash(
    expected_world_revision: crate::WorldRevision,
    command_fingerprint: CommandFingerprintV1,
    edits: &[GameplayMutationIntentV1],
    outcome: &CommandOutcomeV1,
) -> GameplayPlanHashV1 {
    let mut encoder = Encoder::new(b"latticeaxiom.gameplay-runtime-plan.v1\0");
    encoder.u64(expected_world_revision.get());
    encoder.bytes(&command_fingerprint.as_bytes());
    encoder.len(edits.len());
    for edit in edits {
        encoder.edit(edit);
    }
    encoder.outcome(outcome);
    GameplayPlanHashV1::from_bytes(encoder.finish_bytes())
}

struct Encoder(Sha256);

impl Encoder {
    fn new(domain: &[u8]) -> Self {
        let mut digest = Sha256::new();
        digest.update(domain);
        Self(digest)
    }

    fn finish_bytes(self) -> [u8; 32] {
        self.0.finalize().into()
    }

    fn bytes(&mut self, value: &[u8]) {
        self.len(value.len());
        self.0.update(value);
    }

    fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn optional_text(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.text(value);
            }
            None => self.u8(0),
        }
    }

    #[allow(
        clippy::expect_used,
        reason = "usize is no wider than u64 on every supported gameplay target"
    )]
    fn len(&mut self, value: usize) {
        self.u64(
            u64::try_from(value).expect("usize values fit u64 on every supported gameplay target"),
        );
    }

    fn u8(&mut self, value: u8) {
        self.0.update([value]);
    }

    fn u16(&mut self, value: u16) {
        self.0.update(value.to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.update(value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.update(value.to_be_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.0.update(value.to_be_bytes());
    }

    fn block_key(&mut self, key: &BlockKey) {
        self.text(key.dimension.as_str());
        self.i32(key.position.x);
        self.i32(key.position.y);
        self.i32(key.position.z);
    }

    fn chunk_key(&mut self, key: &DimensionChunkKey) {
        self.text(key.dimension.as_str());
        self.i32(key.coordinate.x);
        self.i32(key.coordinate.y);
        self.i32(key.coordinate.z);
    }

    fn target(&mut self, target: &GameplayEditTarget) {
        self.chunk_key(&target.chunk);
        self.u8(match target.domain {
            crate::GameplayStorageDomain::Voxels => 0,
            crate::GameplayStorageDomain::PersistentEntities => 1,
            crate::GameplayStorageDomain::Continuations => 2,
        });
    }

    fn inventory(&mut self, inventory: &InventoryStateV1) {
        self.target(&inventory.target);
        self.u64(inventory.revision);
        self.u16(inventory.hotbar_slots);
        self.u16(inventory.selected_hotbar);
        self.len(inventory.slots.len());
        for slot in &inventory.slots {
            self.stack_option(slot.as_ref());
        }
    }

    fn stack_option(&mut self, value: Option<&ItemStackV1>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.stack(value);
            }
            None => self.u8(0),
        }
    }

    fn stack(&mut self, stack: &ItemStackV1) {
        self.u16(ItemStackV1::SCHEMA_MAJOR);
        self.text(stack.item.as_str());
        self.u32(stack.quantity.get());
        match stack.state {
            ItemStateV1::Plain => self.u8(0),
            ItemStateV1::ToolDurability { remaining } => {
                self.u8(1);
                self.u32(remaining.get());
            }
        }
    }

    fn container_owner(&mut self, owner: &ContainerOwnerComponentV1) {
        self.text(owner.dimension.as_str());
        self.i32(owner.chunk.x);
        self.i32(owner.chunk.y);
        self.i32(owner.chunk.z);
        self.bytes(owner.entity.as_bytes());
    }

    fn drop_id(&mut self, id: DropEntityId) {
        self.bytes(&id.as_bytes());
    }

    #[allow(
        clippy::too_many_lines,
        reason = "every command variant stays visibly and canonically encoded"
    )]
    fn command(&mut self, command: &GameplayCommandV1) {
        match command {
            GameplayCommandV1::Mine(command) => {
                self.u8(0);
                self.bytes(&command.player.as_bytes());
                self.block_key(&command.target);
                self.u64(command.expected_chunk_revision.get());
                match command.tool_slot {
                    Some(slot) => {
                        self.u8(1);
                        self.u16(slot.get());
                    }
                    None => self.u8(0),
                }
                self.drop_id(command.reserved_drop);
            }
            GameplayCommandV1::DropItem(command) => {
                self.u8(1);
                self.bytes(&command.player.as_bytes());
                self.u16(command.slot.get());
                self.u32(command.quantity.get());
                self.block_key(&command.location);
                self.drop_id(command.reserved_drop);
            }
            GameplayCommandV1::Pickup(command) => {
                self.u8(2);
                self.bytes(&command.player.as_bytes());
                self.drop_id(command.drop);
            }
            GameplayCommandV1::Place(command) => {
                self.u8(3);
                self.bytes(&command.player.as_bytes());
                self.u16(command.slot.get());
                self.block_key(&command.target);
                self.u64(command.expected_chunk_revision.get());
            }
            GameplayCommandV1::Craft(command) => {
                self.u8(4);
                self.bytes(&command.player.as_bytes());
                self.text(command.recipe.as_str());
                self.len(command.input_slots.len());
                for slot in &command.input_slots {
                    self.u16(slot.get());
                }
                match command.workstation {
                    Some(container) => {
                        self.u8(1);
                        self.bytes(&container.as_bytes());
                    }
                    None => self.u8(0),
                }
            }
            GameplayCommandV1::Transfer(command) => {
                self.u8(5);
                self.bytes(&command.player.as_bytes());
                self.bytes(&command.container.as_bytes());
                self.u16(command.player_slot.get());
                self.u16(command.container_slot.get());
                self.u32(command.quantity.get());
                self.u8(match command.direction {
                    TransferDirectionV1::PlayerToContainer => 0,
                    TransferDirectionV1::ContainerToPlayer => 1,
                });
                self.u64(command.expected_inventory_revision);
                self.u64(command.expected_container_revision);
            }
            GameplayCommandV1::MoveStack(command) => {
                self.u8(8);
                self.bytes(&command.player.as_bytes());
                self.u16(command.from.get());
                self.u16(command.to.get());
                match command.quantity {
                    Some(quantity) => {
                        self.u8(1);
                        self.u32(quantity.get());
                    }
                    None => self.u8(0),
                }
                self.u64(command.expected_inventory_revision);
            }
            GameplayCommandV1::SelectHotbar(command) => {
                self.u8(9);
                self.bytes(&command.player.as_bytes());
                self.u16(command.slot.get());
                self.u64(command.expected_inventory_revision);
            }
            GameplayCommandV1::StartProcess(command) => {
                self.u8(6);
                self.bytes(&command.container.as_bytes());
                self.text(command.process.as_str());
                self.u16(command.input_slot.get());
                self.u16(command.fuel_slot.get());
                self.u16(command.output_slot.get());
                self.u64(command.expected_container_revision);
                self.u64(command.started_at.get());
            }
            GameplayCommandV1::AdvanceScheduled(command) => {
                self.u8(7);
                self.u64(command.through_tick.get());
                self.u16(command.max_completions.get());
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "every staged edit variant stays visibly and canonically encoded"
    )]
    fn edit(&mut self, edit: &GameplayMutationIntentV1) {
        self.target(edit.target());
        match edit {
            GameplayMutationIntentV1::InventorySlot {
                player,
                slot,
                before,
                after,
                ..
            } => {
                self.u8(0);
                self.bytes(&player.as_bytes());
                self.u16(slot.get());
                self.stack_option(before.as_ref());
                self.stack_option(after.as_ref());
            }
            GameplayMutationIntentV1::InventoryRevision {
                player,
                before,
                after,
                ..
            } => {
                self.u8(1);
                self.bytes(&player.as_bytes());
                self.u64(*before);
                self.u64(*after);
            }
            GameplayMutationIntentV1::InventoryHotbar {
                player,
                before,
                after,
                ..
            } => {
                self.u8(8);
                self.bytes(&player.as_bytes());
                self.u16(*before);
                self.u16(*after);
            }
            GameplayMutationIntentV1::ContainerSlot {
                container,
                slot,
                before,
                after,
                ..
            } => {
                self.u8(2);
                self.bytes(&container.as_bytes());
                self.u16(slot.get());
                self.stack_option(before.as_ref());
                self.stack_option(after.as_ref());
            }
            GameplayMutationIntentV1::ContainerRevision {
                container,
                before,
                after,
                ..
            } => {
                self.u8(3);
                self.bytes(&container.as_bytes());
                self.u64(*before);
                self.u64(*after);
            }
            GameplayMutationIntentV1::Block {
                key, before, after, ..
            } => {
                self.u8(4);
                self.block_key(key);
                self.optional_text(before.as_ref().map(crate::BlockId::as_str));
                self.optional_text(after.as_ref().map(crate::BlockId::as_str));
            }
            GameplayMutationIntentV1::DropEntity {
                id, before, after, ..
            } => {
                self.u8(5);
                self.drop_id(*id);
                self.drop_option(before.as_ref());
                self.drop_option(after.as_ref());
            }
            GameplayMutationIntentV1::BreakProgress {
                key, before, after, ..
            } => {
                self.u8(6);
                self.bytes(&key.player.as_bytes());
                self.block_key(&key.block);
                self.progress_option(before.as_ref());
                self.progress_option(after.as_ref());
            }
            GameplayMutationIntentV1::Continuation {
                id, before, after, ..
            } => {
                self.u8(7);
                self.bytes(id.as_bytes());
                self.continuation_option(before.as_ref());
                self.continuation_option(after.as_ref());
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "every plan outcome stays included in the plan fingerprint"
    )]
    fn outcome(&mut self, outcome: &CommandOutcomeV1) {
        match outcome {
            CommandOutcomeV1::MiningProgress {
                accumulated,
                required,
            } => {
                self.u8(0);
                self.u32(*accumulated);
                self.u32(*required);
            }
            CommandOutcomeV1::BlockBroken {
                drop,
                affected_chunk,
            } => {
                self.u8(1);
                self.drop_id(*drop);
                self.chunk_key(affected_chunk);
            }
            CommandOutcomeV1::ItemDropped { drop } => {
                self.u8(2);
                self.drop_id(*drop);
            }
            CommandOutcomeV1::ItemPickedUp { drop } => {
                self.u8(3);
                self.drop_id(*drop);
            }
            CommandOutcomeV1::BlockPlaced { affected_chunk } => {
                self.u8(4);
                self.chunk_key(affected_chunk);
            }
            CommandOutcomeV1::Crafted { recipe, output } => {
                self.u8(5);
                self.text(recipe.as_str());
                self.stack(output);
            }
            CommandOutcomeV1::Transferred { quantity } => {
                self.u8(6);
                self.u32(*quantity);
            }
            CommandOutcomeV1::StackMoved { from, to } => {
                self.u8(9);
                self.u16(from.get());
                self.u16(to.get());
            }
            CommandOutcomeV1::HotbarSelected { slot } => {
                self.u8(10);
                self.u16(slot.get());
            }
            CommandOutcomeV1::ProcessScheduled {
                continuation,
                due_tick,
            } => {
                self.u8(7);
                self.bytes(continuation.as_bytes());
                self.u64(due_tick.get());
            }
            CommandOutcomeV1::ScheduledAdvanced {
                completed,
                more_due,
            } => {
                self.u8(8);
                self.u16(*completed);
                self.u8(u8::from(*more_due));
            }
        }
    }

    fn drop_option(&mut self, value: Option<&crate::DroppedItemV1>) {
        match value {
            Some(drop) => {
                self.u8(1);
                self.block_key(&drop.location);
                self.stack(&drop.stack);
            }
            None => self.u8(0),
        }
    }

    fn progress_option(&mut self, value: Option<&crate::BreakProgressV1>) {
        match value {
            Some(progress) => {
                self.u8(1);
                self.text(progress.block.as_str());
                self.u32(progress.accumulated_work);
            }
            None => self.u8(0),
        }
    }

    fn continuation_option(&mut self, value: Option<&crate::FurnaceContinuationV1>) {
        match value {
            Some(continuation) => {
                self.u8(1);
                self.text(continuation.process.as_str());
                self.bytes(&continuation.container.as_bytes());
                self.target(&continuation.target);
                self.u16(continuation.output_slot.get());
                self.stack(&continuation.pending_output);
                self.u64(continuation.due_tick.get());
                self.u64(continuation.revision);
            }
            None => self.u8(0),
        }
    }
}
