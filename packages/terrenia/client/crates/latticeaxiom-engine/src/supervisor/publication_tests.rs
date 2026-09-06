//! Real filesystem intent-slot rollover after a world exits.
#![allow(
    clippy::expect_used,
    reason = "fixed protocol fixtures are asserted explicitly"
)]

use super::*;
use latticeaxiom_core::{canonical_json_bytes, canonical_json_hash};
use latticeaxiom_launcher::{
    ChildExitKindV1, ChildExitReportDraftV1, ChildExitReportV1, ChildRoleV1,
    DurableWorldRevisionV1, LaunchAttempt, LaunchFailureCodeV1, LaunchFailureReceiptV1,
    LaunchGeneration, LaunchIntentDraftV1, LaunchPhaseV1, ProcessSupervisorIdentityV1,
    RECOVERY_REQUEST_SCHEMA_VERSION, RecoveryReasonV1, SlotDisposition, WorldRevision,
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

#[test]
fn recovery_shell_handoff_replaces_the_recovery_claim_without_an_empty_gap() {
    let directory = tempfile::tempdir().expect("test directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical store root");
    let world = WorldId::new_v4();
    let shell_hash = CanonicalHash::digest(b"shell");
    let world_hash = CanonicalHash::digest(b"world");
    let plan_hash = CanonicalHash::digest(b"plan");
    let generation = LaunchGeneration::FIRST;
    let mut store = FileLaunchIntentStore::open(&root).expect("intent store");
    let previous_hash = persist_recovery_claim(&mut store, generation, shell_hash);
    let next = LaunchIntentV1::seal(LaunchIntentDraftV1 {
        generation: generation.next().expect("world generation"),
        attempt: LaunchAttempt::FIRST,
        issued_at_ms: 200,
        expires_at_ms: 300,
        target: LaunchTargetV1::World { world_id: world },
        shell_lock_hash: shell_hash,
        world_lock_hash: Some(world_hash),
        world_open_plan_hash: Some(plan_hash),
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
    })
    .expect("world handoff intent");
    let report = ChildExitReportV1::seal(ChildExitReportDraftV1 {
        child_generation: generation,
        process_epoch: ProcessEpoch::FIRST,
        role: ChildRoleV1::Recovery,
        exit_kind: ChildExitKindV1::Handoff,
        intent_generation: Some(next.generation()),
        intent_checksum: Some(next.checksum()),
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
        last_written_world: None,
        last_durable_world: None,
        shell_lock_hash: shell_hash,
        world_lock_hash: None,
        world_open_plan_hash: None,
        diagnostic_ref: None,
    })
    .expect("recovery handoff report");
    publish_child_exit(&root, &report, Some(&next)).expect("replace recovery claim");
    publish_child_exit(&root, &report, Some(&next)).expect("idempotent publication");
    let slot = store.read().expect("new pending slot");
    assert_eq!(
        slot.bytes(),
        Some(next.canonical_bytes().expect("next bytes").as_slice())
    );
    assert_eq!(slot.disposition(), Some(SlotDisposition::Pending));
    assert_eq!(
        slot.predecessor()
            .map(latticeaxiom_launcher::TerminalPredecessor::blob_hash),
        Some(previous_hash)
    );
}

#[test]
fn first_shell_handoff_publishes_into_an_empty_slot() {
    let directory = tempfile::tempdir().expect("test directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical store root");
    let world = WorldId::new_v4();
    let shell_hash = CanonicalHash::digest(b"shell");
    let next = world_handoff_intent(
        world,
        LaunchGeneration::FIRST.next().expect("next"),
        shell_hash,
    );
    let report = shell_handoff_report(LaunchGeneration::FIRST, &next, ChildRoleV1::Shell);
    publish_child_exit(&root, &report, Some(&next)).expect("publish first handoff");
    let mut store = FileLaunchIntentStore::open(&root).expect("intent store");
    let slot = store.read().expect("pending slot");
    assert_eq!(slot.disposition(), Some(SlotDisposition::Pending));
    assert_eq!(
        slot.bytes(),
        Some(next.canonical_bytes().expect("next bytes").as_slice())
    );
    assert!(slot.predecessor().is_none());
}

#[test]
fn continue_retries_reuse_a_matching_pending_intent() {
    let directory = tempfile::tempdir().expect("test directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical store root");
    let world = WorldId::new_v4();
    let shell_hash = CanonicalHash::digest(b"shell");
    let generation = LaunchGeneration::FIRST.next().expect("next");
    let first = world_handoff_intent_at(world, generation, shell_hash, 200, 300);
    let retry = world_handoff_intent_at(world, generation, shell_hash, 400, 500);
    assert_ne!(first.checksum(), retry.checksum());
    let durable =
        persist_supervised_intent(&root, &first, LaunchGeneration::FIRST).expect("first persist");
    let reused =
        persist_supervised_intent(&root, &retry, LaunchGeneration::FIRST).expect("retry persist");
    assert_eq!(reused.checksum(), durable.checksum());
    let report = shell_handoff_report(LaunchGeneration::FIRST, &reused, ChildRoleV1::Shell);
    publish_child_exit(&root, &report, Some(&reused)).expect("publish reused handoff");
    publish_child_exit(&root, &report, Some(&reused)).expect("idempotent exit report");
}

#[test]
fn quarantined_recovery_bytes_can_be_replaced_by_continue() {
    let directory = tempfile::tempdir().expect("test directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical store root");
    let world = WorldId::new_v4();
    let shell_hash = CanonicalHash::digest(b"shell");
    let generation = LaunchGeneration::FIRST;
    let recovery_bytes = sealed_recovery_request_bytes(generation, shell_hash);
    let recovery_hash = CanonicalHash::digest(&recovery_bytes);
    let mut store = FileLaunchIntentStore::open(&root).expect("intent store");
    store
        .publish(&recovery_bytes)
        .expect("publish recovery bytes");
    store.claim(recovery_hash).expect("claim");
    store.quarantine(recovery_hash).expect("quarantine");
    let next = world_handoff_intent(world, generation.next().expect("next"), shell_hash);
    let report = shell_handoff_report(generation, &next, ChildRoleV1::Recovery);
    publish_child_exit(&root, &report, Some(&next)).expect("replace quarantined recovery bytes");
    let slot = store.read().expect("pending slot");
    assert_eq!(slot.disposition(), Some(SlotDisposition::Pending));
    assert_eq!(
        slot.bytes(),
        Some(next.canonical_bytes().expect("next bytes").as_slice())
    );
}

fn world_handoff_intent(
    world: WorldId,
    generation: LaunchGeneration,
    shell_hash: CanonicalHash,
) -> LaunchIntentV1 {
    world_handoff_intent_at(world, generation, shell_hash, 200, 300)
}

fn world_handoff_intent_at(
    world: WorldId,
    generation: LaunchGeneration,
    shell_hash: CanonicalHash,
    issued_at_ms: u64,
    expires_at_ms: u64,
) -> LaunchIntentV1 {
    LaunchIntentV1::seal(LaunchIntentDraftV1 {
        generation,
        attempt: LaunchAttempt::FIRST,
        issued_at_ms,
        expires_at_ms,
        target: LaunchTargetV1::World { world_id: world },
        shell_lock_hash: shell_hash,
        world_lock_hash: Some(CanonicalHash::digest(b"world")),
        world_open_plan_hash: Some(CanonicalHash::digest(b"plan")),
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
    })
    .expect("world handoff intent")
}

fn shell_handoff_report(
    child_generation: LaunchGeneration,
    next: &LaunchIntentV1,
    role: ChildRoleV1,
) -> ChildExitReportV1 {
    ChildExitReportV1::seal(ChildExitReportDraftV1 {
        child_generation,
        process_epoch: ProcessEpoch::FIRST,
        role,
        exit_kind: ChildExitKindV1::Handoff,
        intent_generation: Some(next.generation()),
        intent_checksum: Some(next.checksum()),
        confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
        last_written_world: None,
        last_durable_world: None,
        shell_lock_hash: next.shell_lock_hash(),
        world_lock_hash: None,
        world_open_plan_hash: None,
        diagnostic_ref: None,
    })
    .expect("shell handoff report")
}

fn persist_recovery_claim(
    store: &mut FileLaunchIntentStore,
    generation: LaunchGeneration,
    shell_lock_hash: CanonicalHash,
) -> CanonicalHash {
    let bytes = sealed_recovery_request_bytes(generation, shell_lock_hash);
    store
        .claim_recovery(None, &bytes)
        .expect("claim recovery slot");
    CanonicalHash::digest(&bytes)
}

fn sealed_recovery_request_bytes(
    generation: LaunchGeneration,
    shell_lock_hash: CanonicalHash,
) -> Vec<u8> {
    let failure = LaunchFailureReceiptV1::new(
        None,
        None,
        None,
        LaunchPhaseV1::IntentRead,
        LaunchFailureCodeV1::IntentMissing,
    );
    let mut body = serde_json::json!({
        "schema_version": RECOVERY_REQUEST_SCHEMA_VERSION,
        "recovery_generation": generation,
        "source_generation": serde_json::Value::Null,
        "source_blob_hash": serde_json::Value::Null,
        "reason": RecoveryReasonV1::InvalidIntent,
        "shell_lock_hash": shell_lock_hash,
        "confirmed_setting_transaction_revision": SettingTransactionRevision::new(0),
        "supervisor_identity": ProcessSupervisorIdentityV1::new(CanonicalHash::digest(b"test-supervisor")),
        "failure": failure,
    });
    let checksum = canonical_json_hash(&body).expect("recovery body hash");
    let Some(object) = body.as_object_mut() else {
        panic!("recovery body is an object");
    };
    object.insert(
        "checksum".to_owned(),
        serde_json::to_value(checksum).expect("checksum value"),
    );
    let bytes = canonical_json_bytes(&body).expect("canonical recovery bytes");
    RecoveryLaunchRequestV1::authenticate_at_rest(&bytes).expect("recovery request authenticates");
    bytes
}
