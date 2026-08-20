//! Headless acceptance tests for the package-driven start surface.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{CanonicalHash, PackageName, StableId, WorldId};
use latticeaxiom_launcher::{
    LaunchGeneration, LaunchTargetV1, SettingTransactionRevision as LaunchSettingRevision,
};
use latticeaxiom_runtime_contracts::{
    RuntimeApplyImpact, ScopeOverlay, SettingAuthority, SettingScope, SettingSensitivity,
    SettingSpec, SettingTransactionRevision, SettingWriter, SettingsCatalogFragment,
    SettingsCatalogPolicy, StoreRevision, ValidatedSettingsCatalog, ValueType,
    resolve_effective_settings,
};
use latticeaxiom_start_ui::*;
use latticeaxiom_world_catalog::{
    CatalogDiagnosticCode, CatalogEntry, CatalogEntryFailure, CatalogEntryState, CatalogProjection,
    DiagnosticCode, DisplayName, LiveWorldLocation, ManagedTrashLocation, PackagePreparation,
    ReconciliationState, RestoreMode, RestorePlanningOutcome, StoragePressureState, TrashEntryId,
    TrashRetentionPolicy, TrashTombstone, WorldDiagnostic, WorldOpenAction, WorldOpenPlan,
    WorldOpenRisk, WorldOpenStatus, WorldRootId, WriterBarrier,
};
use serde_json::json;

fn world(value: &str) -> WorldId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid world fixture: {error}"))
}

fn package(value: &str) -> PackageName {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid package fixture: {error}"))
}

fn stable_id(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid stable ID fixture: {error}"))
}

fn location(root: u32, id: WorldId) -> LiveWorldLocation {
    LiveWorldLocation::new(WorldRootId(root), id)
}

fn plan(id: WorldId, status: WorldOpenStatus) -> WorldOpenPlan {
    let actions = match status {
        WorldOpenStatus::ReadyExact => vec![WorldOpenAction::UseFrozenLock],
        WorldOpenStatus::ReadyCompatible => vec![WorldOpenAction::ResolveCompatibleGraph],
        WorldOpenStatus::NeedsDownloadOrBuild => vec![WorldOpenAction::PreparePackage {
            package: package("@example/game"),
        }],
        WorldOpenStatus::NeedsMigration => vec![WorldOpenAction::OpenReadOnly],
        WorldOpenStatus::RecoverableReadOnly => {
            vec![WorldOpenAction::OpenReadOnly, WorldOpenAction::Export]
        }
        WorldOpenStatus::Blocked => Vec::new(),
    };
    WorldOpenPlan {
        world_id: id,
        status,
        risk: if status == WorldOpenStatus::ReadyExact {
            WorldOpenRisk::None
        } else {
            WorldOpenRisk::Elevated
        },
        reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
        next_safe_step: actions.first().cloned(),
        actions,
        diagnostics: Vec::new(),
    }
}

fn projected_record(
    id: WorldId,
    name: &str,
    last_played_at_ms: u64,
    status: WorldOpenStatus,
) -> WorldShellRecord {
    let entry = CatalogEntry {
        location: location(1, id),
        state: CatalogEntryState::Projected(CatalogProjection {
            world_id: id,
            display_name: DisplayName::new(name)
                .unwrap_or_else(|error| panic!("display fixture: {error}")),
            metadata_epoch: 1,
            clean_shutdown: true,
            durable_frontier: 4,
        }),
    };
    WorldShellRecord::new(
        entry,
        WorldCardMetadata {
            created_at_ms: last_played_at_ms.saturating_sub(1),
            last_played_at_ms,
            physical_bytes: Some(last_played_at_ms),
            game_summary: Some("Example Game".to_owned()),
            dimension_summary: Some("Overworld".to_owned()),
        },
        Some(plan(id, status)),
    )
    .unwrap_or_else(|error| panic!("record fixture: {error}"))
}

fn failed_record(id: WorldId) -> WorldShellRecord {
    WorldShellRecord::new(
        CatalogEntry {
            location: location(1, id),
            state: CatalogEntryState::Failed(CatalogEntryFailure::BadChecksum),
        },
        WorldCardMetadata {
            last_played_at_ms: 30,
            ..WorldCardMetadata::default()
        },
        None,
    )
    .unwrap_or_else(|error| panic!("failed record fixture: {error}"))
}

fn shell_graph() -> ClientShellGraph {
    ClientShellGraph::resolve([
        ShellPackageProvider {
            package: package("@latticeaxiom/front-end"),
            capability: ShellCapability::ClientShell,
        },
        ShellPackageProvider {
            package: package("@latticeaxiom/world-library"),
            capability: ShellCapability::WorldCatalog,
        },
        ShellPackageProvider {
            package: package("@latticeaxiom/settings-ui"),
            capability: ShellCapability::SettingsSurface,
        },
        ShellPackageProvider {
            package: package("@latticeaxiom/settings"),
            capability: ShellCapability::SettingsRegistry,
        },
        ShellPackageProvider {
            package: package("@latticeaxiom/observability"),
            capability: ShellCapability::DiagnosticRegistry,
        },
    ])
    .unwrap_or_else(|error| panic!("shell graph fixture: {error}"))
}

#[test]
fn corrupt_header_remains_visible_and_does_not_block_healthy_world() {
    let healthy = projected_record(
        world("123e4567-e89b-42d3-a456-426614174000"),
        "Healthy",
        20,
        WorldOpenStatus::ReadyExact,
    );
    let failed = failed_record(world("223e4567-e89b-42d3-a456-426614174000"));
    let list = WorldListModel::new(vec![healthy, failed], WorldSort::Health);

    assert_eq!(list.records().len(), 2);
    assert!(matches!(
        list.records()[0].health(),
        WorldHealth::OpenStatus(WorldOpenStatus::ReadyExact)
    ));
    assert!(matches!(
        list.records()[1].health(),
        WorldHealth::CatalogFailure(CatalogDiagnosticCode::BadChecksum)
    ));
    assert!(list.records()[1].display_label().starts_with("Recovery "));
    assert!(
        list.records()[1]
            .actions()
            .contains(&WorldCardAction::InspectRecovery)
    );
}

#[test]
fn recent_non_exact_world_becomes_review_and_does_not_fall_through_to_continue() {
    let exact = projected_record(
        world("123e4567-e89b-42d3-a456-426614174000"),
        "Older exact",
        10,
        WorldOpenStatus::ReadyExact,
    );
    let missing = projected_record(
        world("223e4567-e89b-42d3-a456-426614174000"),
        "Recent missing content",
        20,
        WorldOpenStatus::NeedsDownloadOrBuild,
    );
    let list = WorldListModel::new(vec![exact, missing], WorldSort::LastPlayed);

    assert!(matches!(
        list.home_primary_action(),
        HomePrimaryAction::Review { .. }
    ));
}

#[test]
fn missing_content_and_low_disk_expose_explicit_recovery_actions() {
    let missing_id = world("123e4567-e89b-42d3-a456-426614174000");
    let mut missing = projected_record(
        missing_id,
        "Missing",
        1,
        WorldOpenStatus::NeedsDownloadOrBuild,
    );
    missing.open_plan = Some(WorldOpenPlan {
        diagnostics: vec![WorldDiagnostic::PackagePreparationRequired {
            packages: vec![PackagePreparation {
                package: package("@example/game"),
                expected_artifact: None,
                estimated_bytes: 42,
                build_required: false,
            }],
        }],
        ..plan(missing_id, WorldOpenStatus::NeedsDownloadOrBuild)
    });
    assert!(missing.actions().iter().any(|action| matches!(
        action,
        WorldCardAction::Preflight(WorldOpenAction::PreparePackage { .. })
    )));

    let low_id = world("223e4567-e89b-42d3-a456-426614174000");
    let mut low = projected_record(low_id, "Low disk", 2, WorldOpenStatus::RecoverableReadOnly);
    low.open_plan = Some(WorldOpenPlan {
        diagnostics: vec![WorldDiagnostic::LowDisk {
            state: StoragePressureState::MutationPaused,
            usable_free_bytes: 1,
            mutation_paused_below: 2,
        }],
        ..plan(low_id, WorldOpenStatus::RecoverableReadOnly)
    });
    assert!(
        low.actions()
            .contains(&WorldCardAction::OpenStorageLocation)
    );
    assert!(
        low.actions()
            .contains(&WorldCardAction::Preflight(WorldOpenAction::OpenReadOnly))
    );
    assert_eq!(
        low.open_plan
            .as_ref()
            .map(|value| value.diagnostics[0].code()),
        Some(DiagnosticCode::LowDisk)
    );
}

#[test]
fn move_to_managed_trash_and_restore_is_recoverable() {
    let id = world("123e4567-e89b-42d3-a456-426614174000");
    let record = projected_record(id, "Recover me", 1, WorldOpenStatus::ReadyExact);
    let mut library = WorldLibraryState::new([record.clone()]);
    let trash_location = ManagedTrashLocation {
        root: WorldRootId(1),
        world_id: id,
        entry_id: TrashEntryId::new("delete-1")
            .unwrap_or_else(|error| panic!("trash ID fixture: {error}")),
    };
    let move_plan = library
        .plan_move_to_trash(
            record.entry.location,
            trash_location.clone(),
            WriterBarrier::ClosedAndDrained,
            true,
        )
        .unwrap_or_else(|error| panic!("trash plan: {error}"));
    library
        .complete_move_to_trash(
            move_plan,
            TrashTombstone {
                original_root: WorldRootId(1),
                world_id: id,
                display_name: DisplayName::new("Recover me")
                    .unwrap_or_else(|error| panic!("name fixture: {error}")),
                deleted_at_ms: 1,
                header_checksum: CanonicalHash::digest(b"header"),
                metadata_checksum: CanonicalHash::digest(b"metadata"),
                physical_bytes: 10,
                last_checkpoint: None,
                retention: TrashRetentionPolicy::ManualPurgeOnly,
            },
        )
        .unwrap_or_else(|error| panic!("trash completion: {error}"));
    assert!(library.live().is_empty());
    assert_eq!(library.trash().len(), 1);

    let outcome = library
        .plan_restore(
            &trash_location,
            WorldRootId(1),
            RestoreMode::OriginalIdentity,
        )
        .unwrap_or_else(|error| panic!("restore plan: {error}"));
    assert!(matches!(outcome, RestorePlanningOutcome::Ready(_)));
    library
        .complete_restore(&trash_location, record)
        .unwrap_or_else(|error| panic!("restore completion: {error}"));
    assert_eq!(library.live().len(), 1);
    assert!(library.trash().is_empty());
}

#[test]
fn quick_create_is_only_a_validated_transaction_intent_and_accepts_cjk() {
    let intent = QuickCreateIntent::new(
        "山海世界",
        stable_id("example:world-template/safe"),
        package("@example/game"),
        CanonicalHash::digest(b"profile"),
    )
    .unwrap_or_else(|error| panic!("quick create: {error}"));
    assert_eq!(intent.display_name.as_str(), "山海世界");
}

#[test]
fn loading_stages_disclose_writer_safe_cancel_boundary() {
    let mut loading = LoadingState::new();
    assert_eq!(
        loading.cancel_disposition(),
        LoadingCancelDisposition::CancelBeforeWriter
    );
    for stage in [
        LoadingStage::ResolvingPackages,
        LoadingStage::BuildingOrLoading,
        LoadingStage::ValidatingContent,
        LoadingStage::LoadingSpawn,
    ] {
        loading
            .advance(stage, LoadingProgress::Indeterminate, None)
            .unwrap_or_else(|error| panic!("loading transition: {error}"));
    }
    assert_eq!(
        loading.cancel_disposition(),
        LoadingCancelDisposition::ShutdownBarrierRequired
    );
    loading
        .advance(
            LoadingStage::Playing,
            LoadingProgress::Determinate {
                completed: 1,
                total: 1,
            },
            None,
        )
        .unwrap_or_else(|error| panic!("playing transition: {error}"));
    assert_eq!(
        loading.cancel_disposition(),
        LoadingCancelDisposition::AlreadyPlaying
    );
}

fn setting(owner: &str, path: &str) -> SettingSpec {
    SettingSpec {
        id: stable_id(&format!("example:setting/{path}")),
        declared_by: package(owner),
        schema_version: 1,
        value_type: ValueType::Bool,
        default: json!(false),
        allowed_scopes: BTreeSet::from([SettingScope::User]),
        default_scope: SettingScope::User,
        authority: SettingAuthority::LocalUser,
        apply_impact: RuntimeApplyImpact::Immediate,
        category: stable_id("example:setting-category/general"),
        order: 0,
        label_key: "label".to_owned(),
        description_key: "description".to_owned(),
        visibility: None,
        enabled_when: None,
        sensitivity: SettingSensitivity::Ordinary,
        replacement: None,
    }
}

#[test]
fn settings_preview_cancel_rolls_back_in_reverse_stable_order() {
    let alpha = setting("@example/settings", "alpha");
    let beta = setting("@example/settings", "beta");
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![alpha.clone(), beta.clone()],
        )],
        SettingsCatalogPolicy::default(),
    )
    .unwrap_or_else(|error| panic!("catalog fixture: {error}"));
    let lock = CanonicalHash::digest(b"lock");
    let before = resolve_effective_settings(&catalog, [], lock)
        .unwrap_or_else(|error| panic!("before snapshot: {error}"));
    let proposed = resolve_effective_settings(
        &catalog,
        [ScopeOverlay::new(
            SettingScope::User,
            StoreRevision::new(1),
            SettingTransactionRevision::new(1),
            SettingWriter::LocalUser,
            None,
            BTreeMap::from([(alpha.id, json!(true)), (beta.id, json!(true))]),
        )],
        lock,
    )
    .unwrap_or_else(|error| panic!("proposed snapshot: {error}"));
    let mut transaction = SettingsTransactionSurface::new(
        SettingsDurabilityDomain::User,
        before.snapshot(),
        proposed.snapshot(),
        PreviewPolicy::Reversible { timeout_ms: 10_000 },
    );
    assert_eq!(
        transaction.prepare(),
        Ok(SettingsTransactionCommand::Prepare)
    );
    assert_eq!(
        transaction.begin_preview(),
        Ok(SettingsTransactionCommand::BeginPreview { timeout_ms: 10_000 })
    );
    let rollback = transaction
        .rollback_before_persist()
        .unwrap_or_else(|error| panic!("rollback command: {error}"));
    let SettingsTransactionCommand::Rollback(rollback) = rollback else {
        panic!("expected rollback command");
    };
    assert_eq!(
        rollback
            .steps()
            .iter()
            .map(|step| step.id.to_string())
            .collect::<Vec<_>>(),
        vec![
            "example:setting/beta".to_owned(),
            "example:setting/alpha".to_owned()
        ]
    );
    assert_eq!(transaction.finish_rollback(true), Ok(None));
    assert_eq!(transaction.phase(), SettingsTransactionPhase::RolledBack);
}

#[test]
fn keyboard_controller_and_headless_use_the_same_semantic_tree() {
    let record = projected_record(
        world("123e4567-e89b-42d3-a456-426614174000"),
        "Exact",
        10,
        WorldOpenStatus::ReadyExact,
    );
    let mut shell = StartShellModel::new(
        shell_graph(),
        WorldListModel::new(vec![record], WorldSort::LastPlayed),
    );
    assert_eq!(
        keyboard_action(KeyboardSemanticInput::Activate),
        controller_action(ControllerSemanticInput::Activate)
    );
    let tree = shell.semantic_tree();
    let target = SemanticNodeId::new("home/worlds")
        .unwrap_or_else(|error| panic!("target fixture: {error}"));
    let node = tree
        .find(&target)
        .unwrap_or_else(|| panic!("worlds node missing"));
    assert_eq!(node.role, SemanticRole::Button);
    assert!(node.description.is_some());
    assert!(node.actions.contains(&SemanticActionId::Activate));

    let effect = shell
        .inject(&SemanticCommand {
            target,
            action: keyboard_action(KeyboardSemanticInput::Activate),
            source: InputSource::Keyboard,
        })
        .unwrap_or_else(|error| panic!("keyboard injection: {error}"));
    assert_eq!(effect, ShellEffect::Navigate(ShellScreen::Worlds));
    let world_target = shell.worlds.records()[0].semantic_id();
    let effect = shell
        .inject(&SemanticCommand {
            target: world_target,
            action: controller_action(ControllerSemanticInput::Activate),
            source: InputSource::Controller,
        })
        .unwrap_or_else(|error| panic!("controller injection: {error}"));
    assert!(matches!(effect, ShellEffect::ReviewWorld(_)));
}

#[test]
fn layout_contract_keeps_primary_and_recovery_actions_reachable_at_both_scales() {
    let exact = projected_record(
        world("123e4567-e89b-42d3-a456-426614174000"),
        "Exact",
        1,
        WorldOpenStatus::ReadyExact,
    );
    let failed = failed_record(world("223e4567-e89b-42d3-a456-426614174000"));
    let mut shell = StartShellModel::new(
        shell_graph(),
        WorldListModel::new(vec![exact, failed], WorldSort::Health),
    );
    shell.screen = ShellScreen::Worlds;
    let order = shell.semantic_tree().focus_order();
    let essentials = vec![
        SemanticNodeId::new("worlds/back").unwrap_or_else(|error| panic!("ID fixture: {error}")),
        shell.worlds.records()[1].semantic_id(),
    ];
    for scale in [UiScale::One, UiScale::Two] {
        let evidence = verify_scroll_layout(
            LogicalViewport {
                width: 800,
                height: 600,
            },
            scale,
            &order,
        )
        .unwrap_or_else(|error| panic!("layout evidence: {error}"));
        assert!(evidence.essentials_reachable(&essentials));
        assert!(evidence.scroll_viewport_height > 0);
    }
}

#[test]
fn ime_cjk_accessibility_status_and_no_pixel_evidence_are_explicit() {
    let mut text = ImeTextState::default();
    text.set_composition("世");
    assert_eq!(text.composing(), Some("世"));
    text.commit("世界");
    assert_eq!(text.committed(), "世界");
    assert_eq!(text.composing(), None);

    let report = StartSurfaceCapabilityReport::default();
    assert_eq!(
        report.accessibility_tree,
        CapabilityEvidenceStatus::HeadlessVerified
    );
    assert_eq!(
        report.cjk_fallback_font,
        CapabilityEvidenceStatus::AdapterPending
    );
    assert_eq!(report.visual_evidence, VisualEvidenceStatus::NotCollected);
}

#[test]
fn ready_exact_world_handoff_is_launch_intent_for_replacement_process_only() {
    let record = projected_record(
        world("123e4567-e89b-42d3-a456-426614174000"),
        "Exact",
        1,
        WorldOpenStatus::ReadyExact,
    );
    let handoff = LaunchHandoff::for_ready_exact(
        &record,
        LaunchHandoffContext {
            generation: LaunchGeneration::FIRST,
            issued_at_ms: 1_000,
            expires_at_ms: 61_000,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: CanonicalHash::digest(b"world"),
            confirmed_setting_transaction_revision: LaunchSettingRevision::new(4),
        },
    )
    .unwrap_or_else(|error| panic!("launch handoff: {error}"));
    assert_eq!(
        handoff.intent.target(),
        LaunchTargetV1::World {
            world_id: record.world_id()
        }
    );
    assert_eq!(
        handoff.disposition,
        ClientProcessDisposition::ExitAfterAtomicIntentPublish
    );

    let compatible = projected_record(
        world("223e4567-e89b-42d3-a456-426614174000"),
        "Compatible",
        2,
        WorldOpenStatus::ReadyCompatible,
    );
    assert!(matches!(
        LaunchHandoff::for_ready_exact(
            &compatible,
            LaunchHandoffContext {
                generation: LaunchGeneration::FIRST,
                issued_at_ms: 1_000,
                expires_at_ms: 61_000,
                shell_lock_hash: CanonicalHash::digest(b"shell"),
                world_lock_hash: CanonicalHash::digest(b"world"),
                confirmed_setting_transaction_revision: LaunchSettingRevision::new(4),
            },
        ),
        Err(LaunchHandoffError::NotReadyExact)
    ));
}

#[test]
fn package_graph_is_exactly_one_and_order_independent() {
    let graph = shell_graph();
    assert_eq!(graph.providers().len(), 5);
    let mut providers = graph
        .providers()
        .iter()
        .map(|(capability, provider)| ShellPackageProvider {
            package: provider.clone(),
            capability: *capability,
        })
        .collect::<Vec<_>>();
    providers.reverse();
    let reversed = ClientShellGraph::resolve(providers)
        .unwrap_or_else(|error| panic!("reverse package graph: {error}"));
    assert_eq!(graph, reversed);
    assert!(matches!(
        ClientShellGraph::resolve([]),
        Err(ClientShellGraphError::MissingProvider { .. })
    ));
}

#[test]
fn async_refresh_restores_focus_by_world_key_not_row_index() {
    let alpha = projected_record(
        world("123e4567-e89b-42d3-a456-426614174000"),
        "Alpha",
        1,
        WorldOpenStatus::ReadyExact,
    );
    let beta = projected_record(
        world("223e4567-e89b-42d3-a456-426614174000"),
        "Beta",
        2,
        WorldOpenStatus::ReadyExact,
    );
    let beta_key = beta.semantic_id();
    let mut list = WorldListModel::new(vec![alpha.clone(), beta.clone()], WorldSort::Name);
    list.focus(beta_key.clone())
        .unwrap_or_else(|error| panic!("focus fixture: {error}"));
    list.refresh(vec![beta, alpha]);
    assert_eq!(list.focused(), Some(&beta_key));
}
