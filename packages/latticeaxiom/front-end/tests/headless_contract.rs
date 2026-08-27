//! Headless acceptance tests for the package-driven start surface.

use std::collections::BTreeSet;

use latticeaxiom_core::{CanonicalHash, PackageName, StableId, WorldId};
use latticeaxiom_launcher::{
    LaunchGeneration, LaunchTargetV1, SettingTransactionRevision as LaunchSettingRevision,
};
use latticeaxiom_runtime_contracts::{
    RuntimeApplyImpact, SettingAuthority, SettingScope, SettingSensitivity, SettingSpec,
    SettingsCatalogFragment, SettingsCatalogPolicy, ValidatedSettingsCatalog, ValueType,
    resolve_effective_settings,
};
use latticeaxiom_start_ui::*;
use latticeaxiom_world_catalog::{
    CatalogCardState, CatalogDiagnosticCode, CatalogEntry, CatalogEntryFailure, CatalogEntryState,
    CatalogProjection, DiagnosticCode, DisplayName, LiveWorldLocation, ManagedTrashLocation,
    PackagePreparation, ReconciliationState, RestoreMode, RestorePlanningOutcome,
    SealedActivationBindingV1, StoragePressureState, StoreId, TrashEntryId, TrashRetentionPolicy,
    TrashTombstone, WorldDiagnostic, WorldOpenAction, WorldOpenPlan, WorldOpenRisk,
    WorldOpenStatus, WorldRootId, WriterBarrier, WriterLeaseState,
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
        activation_binding: (status == WorldOpenStatus::ReadyExact).then(|| {
            SealedActivationBindingV1 {
                store_id: StoreId::new("store-1").unwrap_or_else(|error| panic!("{error}")),
                metadata_epoch: 1,
                metadata_hash: CanonicalHash::digest(b"metadata"),
                projection_hash: CanonicalHash::digest(b"projection"),
                plan_generation: 1,
            }
        }),
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
    assert_eq!(list.records()[0].card_state(), CatalogCardState::ReadyExact);
    assert_eq!(list.records()[1].card_state(), CatalogCardState::Corrupt);
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
        category: stable_id("latticeaxiom:setting-category/gameplay"),
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
fn front_end_uses_the_package_owned_settings_authority() {
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
    let package_model = latticeaxiom_settings_ui::SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        before.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .unwrap_or_else(|error| panic!("package settings model: {error}"));
    let mut model: SettingsSurfaceModel = package_model;
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::BeginEdit),
        Ok(SettingsSurfaceOutcome::EditingBegan { .. })
    ));
    for setting in [alpha.id.clone(), beta.id.clone()] {
        assert!(matches!(
            model.handle(SettingsSurfaceCommand::SetValue {
                setting,
                value: json!(true),
            }),
            Ok(SettingsSurfaceOutcome::DraftChanged { .. })
        ));
    }
    let request = match model.handle(SettingsSurfaceCommand::Apply) {
        Ok(SettingsSurfaceOutcome::ApplyRequested(request)) => request,
        other => panic!("expected package apply request, got {other:?}"),
    };
    assert_eq!(request.domain, SettingsDurabilityDomain::User);
    assert_eq!(
        request
            .proposed
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec![
            "example:setting/alpha".to_owned(),
            "example:setting/beta".to_owned()
        ]
    );
    assert!(request.restart.live_immediate);
    assert!(model.apply_pending());
    assert!(matches!(
        model.finish_apply(SettingsSurfaceApplyResolution::Committed),
        Ok(SettingsSurfaceOutcome::ApplyCompleted {
            resolution: SettingsSurfaceApplyResolution::Committed
        })
    ));
    assert!(!model.is_dirty());

    let front_source = include_str!("../src/settings.rs");
    assert!(!front_source.contains("pub struct SettingsSurfaceModel"));
    assert!(!front_source.contains("pub struct SettingsTransactionSurface"));
    assert!(front_source.contains("pub use latticeaxiom_settings_ui::*"));
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

#[test]
fn memory_create_list_continue_semantic_flow_preserves_world_id() {
    let mut flow = MemoryStartFlow::new(shell_graph());
    flow.set_draft(
        QuickCreateIntent::new(
            "Memory Session",
            memory_session_template(),
            package("@example/game"),
            CanonicalHash::digest(b"profile"),
        )
        .unwrap_or_else(|error| panic!("quick create: {error}")),
    );
    flow.set_now_ms(10);

    let effect = flow
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("home/new-world")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::Activate,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("open new world: {error}"));
    assert_eq!(
        effect,
        MemoryStartEffect::Shell(ShellEffect::Navigate(ShellScreen::NewWorld))
    );

    let created = match flow
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("new-world/quick-create")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::Activate,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("quick create: {error}"))
    {
        MemoryStartEffect::Created(world_id) => world_id,
        other @ MemoryStartEffect::Shell(_) => panic!("expected created world, got {other:?}"),
    };
    assert_eq!(flow.continue_world_id(), Some(created));
    assert_eq!(flow.worlds().len(), 1);
    assert_eq!(flow.shell().screen, ShellScreen::Home);
    assert!(matches!(
        flow.shell().worlds.home_primary_action(),
        HomePrimaryAction::Continue { world_id, .. } if world_id == created
    ));

    flow.mark_played(created, 20)
        .unwrap_or_else(|error| panic!("mark played: {error}"));
    let effect = flow
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("home/continue")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::ContinueWorld,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("continue: {error}"));
    assert_eq!(
        effect,
        MemoryStartEffect::Shell(ShellEffect::RequestExactWorldLaunch(created))
    );
    assert_eq!(flow.continue_world_id(), Some(created));
}

#[test]
fn creation_profile_selection_is_accessible_and_updates_the_typed_intent() {
    let balanced = stable_id("example:worldgen-profile/balanced@2");
    let alpine = stable_id("example:worldgen-profile/alpine@2");
    let mut flow = MemoryStartFlow::new(shell_graph());
    flow.set_worldgen_profiles(
        vec![
            WorldgenProfileOption::new(balanced.clone(), "Balanced", "Mixed terrain"),
            WorldgenProfileOption::new(alpine.clone(), "Alpine", "High mountain systems"),
        ],
        &balanced,
    )
    .unwrap_or_else(|error| panic!("profile catalog: {error}"));
    flow.set_draft(
        QuickCreateIntent::new(
            "Profiled Session",
            memory_session_template(),
            package("@example/game"),
            CanonicalHash::digest(b"profile"),
        )
        .unwrap_or_else(|error| panic!("quick create: {error}"))
        .with_generation_profile(balanced),
    );
    flow.apply_shell_command(&SemanticCommand {
        target: SemanticNodeId::new("home/new-world")
            .unwrap_or_else(|error| panic!("new-world target: {error}")),
        action: SemanticActionId::Activate,
        source: InputSource::Headless,
    })
    .unwrap_or_else(|error| panic!("open creation: {error}"));
    let tree = flow.shell().semantic_tree();
    assert!(
        tree.find(&SemanticNodeId::new("new-world/profile-status").unwrap())
            .is_some()
    );

    let effect = flow
        .apply_shell_command(&SemanticCommand {
            target: SemanticNodeId::new("new-world/profile/1")
                .unwrap_or_else(|error| panic!("profile target: {error}")),
            action: SemanticActionId::SelectWorldgenProfile,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("select profile: {error}"));
    assert_eq!(effect, ShellEffect::WorldgenProfileSelected);
    assert_eq!(
        flow.draft()
            .and_then(|intent| intent.generation_profile.as_ref()),
        Some(&alpine)
    );
}

#[test]
fn memory_pause_save_exit_continue_semantic_flow_does_not_checkpoint() {
    let mut flow = MemoryStartFlow::new(shell_graph());
    let intent = QuickCreateIntent::new(
        "Memory Session",
        memory_session_template(),
        package("@example/game"),
        CanonicalHash::digest(b"profile"),
    )
    .unwrap_or_else(|error| panic!("quick create: {error}"));
    let created = flow
        .create(&intent, WorldId::new_v4(), 10)
        .unwrap_or_else(|error| panic!("create: {error}"));

    flow.enter_playing();
    assert_eq!(flow.shell().screen, ShellScreen::Playing);
    let pause = flow
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("playing/pause")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::PauseWorld,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("pause: {error}"));
    assert_eq!(
        pause,
        MemoryStartEffect::Shell(ShellEffect::Navigate(ShellScreen::Pause))
    );
    assert_eq!(flow.shell().screen, ShellScreen::Pause);

    let save = flow
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("pause/save")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::SaveWorld,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("save: {error}"));
    assert_eq!(
        save,
        MemoryStartEffect::Shell(ShellEffect::RequestSaveWorld)
    );
    assert_eq!(
        flow.shell().screen,
        ShellScreen::Pause,
        "save must not leave the pause overlay or write a checkpoint"
    );

    let exit = flow
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("pause/exit")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::ExitWorld,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("exit: {error}"));
    assert_eq!(
        exit,
        MemoryStartEffect::Shell(ShellEffect::RequestExitWorld)
    );
    assert_eq!(flow.shell().screen, ShellScreen::Home);
    assert_eq!(flow.continue_world_id(), Some(created));

    let continued = flow
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("home/continue")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::ContinueWorld,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("continue: {error}"));
    assert_eq!(
        continued,
        MemoryStartEffect::Shell(ShellEffect::RequestExactWorldLaunch(created))
    );
}

#[test]
fn world_library_create_continue_prepares_launch_without_a_writer() {
    let mut flow = WorldLibraryFlow::new(shell_graph());
    flow.set_now_ms(10);
    flow.set_launch_context(LaunchHandoffContext {
        generation: LaunchGeneration::FIRST,
        issued_at_ms: 10,
        expires_at_ms: 70_000,
        shell_lock_hash: CanonicalHash::digest(b"shell"),
        world_lock_hash: CanonicalHash::digest(b"world"),
        confirmed_setting_transaction_revision: LaunchSettingRevision::new(1),
    });
    let world = WorldId::new_v4();
    flow.create(
        &QuickCreateIntent::new(
            "Library",
            memory_session_template(),
            package("@example/game"),
            CanonicalHash::digest(b"profile"),
        )
        .unwrap_or_else(|error| panic!("quick create: {error}")),
        world,
    )
    .unwrap_or_else(|error| panic!("create: {error}"));
    assert!(flow.continue_world_id().is_none());
    flow.attach_preflight(world, plan(world, WorldOpenStatus::ReadyExact))
        .unwrap_or_else(|error| panic!("preflight: {error}"));
    let effect = flow
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("home/continue")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::ContinueWorld,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("continue: {error}"));
    assert!(matches!(effect, WorldLibraryEffect::PreparedLaunch(_)));
    assert_eq!(flow.shell().screen, ShellScreen::Loading);
    assert_eq!(
        flow.shell()
            .loading
            .as_ref()
            .map(LoadingState::cancel_disposition),
        Some(LoadingCancelDisposition::CancelBeforeWriter)
    );
}

#[test]
fn crash_lease_and_low_disk_cues_are_actionable_and_block_continue() {
    let id = world("123e4567-e89b-42d3-a456-426614174000");
    let mut crash = projected_record(id, "Crash", 20, WorldOpenStatus::RecoverableReadOnly);
    crash.open_plan = Some(WorldOpenPlan {
        diagnostics: vec![
            WorldDiagnostic::UncleanShutdown,
            WorldDiagnostic::StaleWriterLease,
            WorldDiagnostic::LowDisk {
                state: StoragePressureState::MutationPaused,
                usable_free_bytes: 1,
                mutation_paused_below: 2,
            },
        ],
        actions: vec![
            WorldOpenAction::RecoverStaleLease,
            WorldOpenAction::OpenReadOnly,
            WorldOpenAction::Export,
        ],
        next_safe_step: Some(WorldOpenAction::RecoverStaleLease),
        ..plan(id, WorldOpenStatus::RecoverableReadOnly)
    });
    let cues = crash.recovery_cues();
    assert!(
        cues.iter()
            .any(|cue| cue.code == DiagnosticCode::UncleanShutdown)
    );
    assert!(
        cues.iter()
            .any(|cue| cue.code == DiagnosticCode::StaleWriterLease)
    );
    assert!(cues.iter().any(|cue| cue.code == DiagnosticCode::LowDisk));
    assert!(
        cues.iter()
            .all(|cue| !cue.description.contains("cannot open"))
    );

    let list = WorldListModel::new(vec![crash.clone()], WorldSort::LastPlayed);
    assert!(matches!(
        list.home_primary_action(),
        HomePrimaryAction::Review { .. }
    ));
    assert!(crash.actions().iter().any(|action| matches!(
        action,
        WorldCardAction::Preflight(WorldOpenAction::RecoverStaleLease)
    )));

    let mut shell = StartShellModel::new(shell_graph(), list);
    shell.screen = ShellScreen::Worlds;
    let tree = shell.semantic_tree();
    let node = tree
        .find(&crash.semantic_id())
        .unwrap_or_else(|| panic!("world row missing"));
    assert_eq!(node.role, SemanticRole::Alert);
    assert!(
        node.description
            .as_ref()
            .is_some_and(|text| text.contains("Stale writer lease"))
    );
    assert!(
        node.children
            .iter()
            .any(|child| child.role == SemanticRole::Alert && child.name == "Stale writer lease")
    );
    assert_eq!(WriterLeaseState::Absent, WriterLeaseState::default());
}

#[test]
fn home_uses_shared_route_vocabulary_and_client_ui_projection() {
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
        shell.typed_shell_route(),
        Some(latticeaxiom_client_ui::ShellRouteV1::Home)
    );
    let snapshot = shell
        .typed_home_snapshot()
        .unwrap_or_else(|error| panic!("typed home: {error}"));
    latticeaxiom_client_ui::check_accesskit_tree(snapshot.root())
        .unwrap_or_else(|error| panic!("a11y: {error}"));
    assert!(
        snapshot
            .root()
            .find(
                &latticeaxiom_client_ui::SemanticKey::new("home/packages-profiles")
                    .unwrap_or_else(|error| panic!("{error}"))
            )
            .is_some()
    );

    let effect = shell
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("home/quit")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::Activate,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("quit: {error}"));
    assert_eq!(effect, ShellEffect::Navigate(ShellScreen::QuitConfirm));
    assert_eq!(
        shell.typed_shell_route(),
        Some(latticeaxiom_client_ui::ShellRouteV1::QuitConfirm)
    );
    let quit = shell
        .inject(&SemanticCommand {
            target: SemanticNodeId::new("modal/quit/confirm")
                .unwrap_or_else(|error| panic!("target fixture: {error}")),
            action: SemanticActionId::Activate,
            source: InputSource::Keyboard,
        })
        .unwrap_or_else(|error| panic!("confirm quit: {error}"));
    assert_eq!(quit, ShellEffect::RequestQuitProduct);
    let snapshot = shell
        .typed_snapshot()
        .unwrap_or_else(|error| panic!("typed snapshot: {error}"));
    assert!(snapshot.is_some());
}
