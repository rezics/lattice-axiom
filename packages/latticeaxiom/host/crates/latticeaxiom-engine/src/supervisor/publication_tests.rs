//! Real filesystem intent-slot rollover after a world exits.
#![allow(
    clippy::expect_used,
    reason = "fixed protocol fixtures are asserted explicitly"
)]

use super::*;
use latticeaxiom_launcher::{
    ChildExitKindV1, ChildExitReportDraftV1, ChildExitReportV1, ChildRoleV1,
    DurableWorldRevisionV1, LaunchAttempt, LaunchGeneration, LaunchIntentDraftV1, WorldRevision,
};

#[test]
fn world_return_replaces_its_exact_consumed_intent_without_an_empty_gap() {
    let directory = tempfile::tempdir().expect("test directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical store root");
    let world = WorldId::new_v4();
    let shell_hash = CanonicalHash::digest(b"shell");
    let world_hash = CanonicalHash::digest(b"world");
    let plan_hash = CanonicalHash::digest(b"plan");
    let generation = LaunchGeneration::FIRST.next().expect("world generation");
    let previous = LaunchIntentV1::seal(LaunchIntentDraftV1 {
        generation,
        attempt: LaunchAttempt::FIRST,
        issued_at_ms: 1,
        expires_at_ms: 100,
        target: LaunchTargetV1::World { world_id: world },
        shell_lock_hash: shell_hash,
        world_lock_hash: Some(world_hash),
        world_open_plan_hash: Some(plan_hash),
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
    })
    .expect("world intent");
    let previous_bytes = previous.canonical_bytes().expect("intent bytes");
    let previous_hash = CanonicalHash::digest(&previous_bytes);
    let mut store = FileLaunchIntentStore::open(&root).expect("intent store");
    store
        .publish(&previous_bytes)
        .expect("publish world intent");
    store.claim(previous_hash).expect("claim");
    store
        .consume(previous_hash)
        .expect("bootstrap acknowledgement");
    let next = LaunchIntentV1::seal(LaunchIntentDraftV1 {
        generation: generation.next().expect("return generation"),
        attempt: LaunchAttempt::FIRST,
        issued_at_ms: 200,
        expires_at_ms: 300,
        target: LaunchTargetV1::Shell,
        shell_lock_hash: shell_hash,
        world_lock_hash: None,
        world_open_plan_hash: None,
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
    })
    .expect("shell return intent");
    let durable = DurableWorldRevisionV1::new(world, WorldRevision::new(2));
    let report = ChildExitReportV1::seal(ChildExitReportDraftV1 {
        child_generation: generation,
        process_epoch: ProcessEpoch::new(2).expect("epoch"),
        role: ChildRoleV1::World { world_id: world },
        exit_kind: ChildExitKindV1::SaveAndQuit,
        intent_generation: Some(next.generation()),
        intent_checksum: Some(next.checksum()),
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
        last_written_world: Some(durable),
        last_durable_world: Some(durable),
        shell_lock_hash: shell_hash,
        world_lock_hash: Some(world_hash),
        world_open_plan_hash: Some(plan_hash),
        diagnostic_ref: None,
    })
    .expect("world exit report");
    publish_child_exit(&root, &report, Some(&next)).expect("replace consumed predecessor");
    publish_child_exit(&root, &report, Some(&next)).expect("idempotent publication");
    let slot = store.read().expect("new pending slot");
    assert_eq!(
        slot.bytes(),
        Some(next.canonical_bytes().expect("next bytes").as_slice())
    );
    assert_eq!(
        slot.predecessor()
            .map(latticeaxiom_launcher::TerminalPredecessor::blob_hash),
        Some(previous_hash)
    );
}
