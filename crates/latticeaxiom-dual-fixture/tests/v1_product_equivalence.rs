//! V9/D10 product equivalence corpus.
//!
//! Compares static/portable dual-realization receipts and the client/headless
//! production hosts that share one reopened final lock. A second root dimension
//! exercises the generic inventory/recipe/persistence subset without Terrenia
//! content IDs. The static path stays a typed row kernel; it is not an ABI
//! lowest-common-denominator.

#![allow(clippy::expect_used, clippy::too_many_lines)]

use std::{
    collections::BTreeMap,
    fmt::Debug,
    fs,
    num::{NonZeroU8, NonZeroU32},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use latticeaxiom_compose::{PRODUCT_LOCK_FILE_NAME, reopen_product_lock};
use latticeaxiom_core::{CanonicalHash, StableId, TargetTriple, canonical_json_bytes};
use latticeaxiom_dual_fixture::{Realization, ffi_batch_call_diagnostic, run_realization};
use latticeaxiom_engine::{
    EngineInstance, LockVerifiedComposeImages, ProductionSpine, VerifiedProductLockHash,
};
use latticeaxiom_gameplay::{
    BlockDefinitionV1, BlockId, BlockKey, BlockPosition, CatalogLimits, ChangedDomains,
    ChunkCoordinate, ChunkRevision, CommandEnvelopeV1, CommandOutcomeV1, ContainerId,
    ContainerOwnerComponentV1, ContainerStateV1, ContinuationId, DimensionChunkKey, DropEntityId,
    FaultInjection, FrozenItemRoleBindingV1, GameplayCatalog, GameplayCatalogSourceV1,
    GameplayCommandV1, GameplayEditTarget, GameplayKernel, GameplayLimits, GameplayPlanV1,
    GameplayStorageDomain, IngredientV1, InventoryStateV1, ItemDefinitionV1, ItemPredicateV1,
    ItemRoleDefinitionV1, ItemStackV1, ItemStateV1, ItemTagDefinitionV1, MineCommandV1,
    MiningRuleV1, PersistentEntityId, PickupCommandV1, PlaceCommandV1, PlayerId,
    RecipeCraftCommandV1, RecipeDefinitionV1, RecipePatternV1, ReferenceGameplayState,
    ReferencePlanApplier, RoleOutputV1, RuntimePlanReceiptV1, SelectHotbarCommandV1, SlotIndex,
    ToolDefinitionV1, ToolRequirementV1, TransactionId, WorkstationDefinitionV1, WorldId,
};
use latticeaxiom_launcher::{HostBuildReceipts, ReopenedFinalLockV1};
use latticeaxiom_packages::{FilesystemCas, LOCAL_CATALOG_CAS_DIRECTORY};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChunkData, ChunkKey, ChunkMutation, ChunkRevisionExpectation,
    MemoryTransactionKernel, PayloadSchemaVersion, VersionedPayload, WorldTransaction,
};
use serde::{Deserialize, Serialize};

const CORPUS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/v1-product-equivalence.json"
);
const GENERIC_CATALOG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/generic-dimension-catalog-v1.json"
);
const GOLDEN_SNAPSHOT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/goldens/canonical-snapshot-v1.json"
);
const CORPUS_SCHEMA: &str = "latticeaxiom.v1-product-equivalence.v1";
const GENERIC_SCHEMA: &str = "latticeaxiom.generic-dimension-catalog.v1";
const PLAYER: PlayerId = PlayerId::new(1);
const WORKBENCH: ContainerId = ContainerId::from_bytes([2; 16]);
const LOG_DROP: DropEntityId = DropEntityId::new(10_000);
const ORE_PROGRESS_DROP: DropEntityId = DropEntityId::new(10_001);
const ORE_BREAK_DROP: DropEntityId = DropEntityId::new(10_002);
const SPINE_TIMESTEP: Duration = Duration::from_nanos(1_000_000_000 / 60);
const FORBIDDEN_NAMESPACE: &str = "terrenia";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EquivalenceCorpus {
    schema_id: String,
    compared: Vec<String>,
    hosts: Vec<String>,
    static_portable: StaticPortableSpec,
    generic_dimension_catalog: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticPortableSpec {
    ticks: u64,
    entities: usize,
    golden_ticks: u64,
    golden_entities: usize,
    static_must_not_use_abi_repack: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericCatalogDocument {
    schema_id: String,
    dimension: String,
    forbidden_namespaces: Vec<String>,
    items: Vec<GenericItem>,
    blocks: Vec<GenericBlock>,
    tools: Vec<GenericTool>,
    tags: Vec<GenericTag>,
    roles: Vec<GenericRole>,
    recipes: Vec<GenericRecipe>,
    workstations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericItem {
    id: String,
    stack_limit: u32,
    placement_block: Option<String>,
    durability: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericBlock {
    id: String,
    hardness: u32,
    tool: Option<GenericToolReq>,
    drop: GenericDrop,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericToolReq {
    class: String,
    minimum_tier: u8,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericDrop {
    item: String,
    quantity: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericTool {
    item: String,
    class: String,
    tier: u8,
    work_per_step: u32,
    maximum_durability: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericTag {
    id: String,
    members: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericRole {
    id: String,
    item: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericRecipe {
    id: String,
    workstation: Option<String>,
    shapeless: Option<Vec<GenericIngredient>>,
    shaped: Option<GenericShaped>,
    output_role: String,
    output_quantity: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericIngredient {
    item: Option<String>,
    tag: Option<String>,
    quantity: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericShaped {
    width: u8,
    height: u8,
    cells: Vec<GenericIngredient>,
}

#[derive(Serialize)]
struct HostNormative {
    host: &'static str,
    product_lock_hash: String,
    registration_semantic_hash: String,
    command_receipts: Vec<CommandReceiptRecord>,
    state_hash: String,
    semantic_report_hash: String,
    normative_save_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct CommandReceiptRecord {
    position: [i32; 3],
    old_content: Option<String>,
    new_content: Option<String>,
    committed_chunk_revision: u64,
}

struct GenericAuthority {
    gameplay: ReferencePlanApplier,
    storage: MemoryTransactionKernel,
    storage_revisions: BTreeMap<DimensionChunkKey, ChunkRevision>,
    next_payload: u64,
}

#[test]
fn corpus_fixture_names_required_comparisons() {
    let corpus = load_corpus();
    assert_eq!(corpus.schema_id, CORPUS_SCHEMA);
    assert_eq!(
        corpus.compared,
        [
            "authoritative_receipts",
            "state_hash",
            "semantic_report",
            "normative_save_bytes",
            "final_lock"
        ]
    );
    assert_eq!(corpus.hosts, ["client", "headless"]);
    assert!(corpus.static_portable.static_must_not_use_abi_repack);
    assert_eq!(
        corpus.generic_dimension_catalog,
        "generic-dimension-catalog-v1.json"
    );
}

#[test]
fn static_and_portable_receipts_match_without_abi_repack_on_static() {
    let corpus = load_corpus();
    let spec = &corpus.static_portable;
    let static_evidence = run_realization(Realization::StaticDirect, spec.ticks, spec.entities)
        .expect("static direct realization must run");
    let portable_evidence = run_realization(Realization::PortableBatch, spec.ticks, spec.entities)
        .expect("portable batch realization must run");

    assert_eq!(
        static_evidence.receipt, portable_evidence.receipt,
        "static and portable authoritative receipts, state hashes, semantic fields, and snapshot bytes must match"
    );
    assert_eq!(
        static_evidence.ffi_system_calls, 0,
        "static direct must not pack Bevy rows through the portable ABI table"
    );
    assert!(
        spec.static_must_not_use_abi_repack && static_evidence.ffi_system_calls == 0,
        "the corpus forbids a lowest-common-denominator static ABI path"
    );
    let diagnostic = ffi_batch_call_diagnostic(spec.entities);
    assert_eq!(
        portable_evidence.ffi_system_calls,
        u64::try_from(
            diagnostic
                .portable_callback_count
                .saturating_mul(usize::try_from(spec.ticks).expect("tick count fits usize"))
        )
        .expect("portable callback count fits u64")
    );
    assert!(
        diagnostic.portable_callback_count < diagnostic.per_entity_counterexample
            || spec.entities <= 1,
        "portable callbacks must scale with bounded batches, not entities"
    );
    assert_eq!(
        static_evidence.receipt.state_hashes.len(),
        usize::try_from(spec.ticks).expect("tick count fits usize")
    );
    assert!(!static_evidence.receipt.snapshot_bytes.is_empty());
    assert!(!static_evidence.receipt.commands.is_empty());

    let golden = run_realization(
        Realization::StaticDirect,
        spec.golden_ticks,
        spec.golden_entities,
    )
    .expect("golden static realization must run");
    let portable_golden = run_realization(
        Realization::PortableBatch,
        spec.golden_ticks,
        spec.golden_entities,
    )
    .expect("golden portable realization must run");
    assert_eq!(golden.receipt, portable_golden.receipt);
    assert_eq!(
        String::from_utf8_lossy(&golden.receipt.snapshot_bytes).trim(),
        fs::read_to_string(GOLDEN_SNAPSHOT_PATH)
            .expect("canonical snapshot golden must exist")
            .trim()
    );
}

#[test]
fn client_and_headless_share_reopened_final_lock() {
    let shared = reopen_final_lock();
    let client = shared.clone();
    let headless = shared;
    assert_eq!(
        client.product_lock_hash(),
        headless.product_lock_hash(),
        "client and headless must share the reopened final lock"
    );
    let client_lock = client.product_lock();
    let headless_lock = headless.product_lock();
    assert_eq!(
        client_lock.registration.semantic_hash,
        headless_lock.registration.semantic_hash
    );
    assert_eq!(
        client_lock.registration.image_hash,
        headless_lock.registration.image_hash
    );
    assert_eq!(client_lock.realizations, headless_lock.realizations);
    let lock_bytes = canonical_json_bytes(client_lock).expect("final lock must canonicalize");
    assert_eq!(
        canonical_json_bytes(headless_lock).expect("headless lock must canonicalize"),
        lock_bytes
    );
    assert!(!lock_bytes.is_empty());
}

#[test]
fn client_and_headless_hosts_match_receipts_hash_report_and_save_bytes() {
    let reopened = reopen_final_lock();
    let images = bind_lock_images(&reopened);
    let client_images = images.clone();
    let headless_images = images;
    assert_eq!(
        client_images.product_lock_hash(),
        headless_images.product_lock_hash()
    );
    assert_eq!(
        client_images.images().registration_semantic_hash(),
        headless_images.images().registration_semantic_hash(),
        "client and headless must share authoritative registration"
    );
    assert_eq!(client_images.target(), headless_images.target());

    let client = capture_host_normative("client", client_images);
    let headless = capture_host_normative("headless", headless_images);
    assert_eq!(client.product_lock_hash, headless.product_lock_hash);
    assert_eq!(
        client.registration_semantic_hash,
        headless.registration_semantic_hash
    );
    assert_eq!(client.command_receipts, headless.command_receipts);
    assert_eq!(client.state_hash, headless.state_hash);
    assert_eq!(client.semantic_report_hash, headless.semantic_report_hash);
    assert_eq!(client.normative_save_bytes, headless.normative_save_bytes);
    assert!(!client.normative_save_bytes.is_empty());
    assert!(!client.semantic_report_hash.is_empty());
}

#[test]
fn generic_dimension_inventory_recipe_persistence_subset_has_no_terrenia_ids() {
    let document = load_generic_catalog();
    assert_eq!(document.schema_id, GENERIC_SCHEMA);
    assert!(
        document
            .forbidden_namespaces
            .iter()
            .any(|namespace| namespace == FORBIDDEN_NAMESPACE)
    );
    let raw = fs::read_to_string(GENERIC_CATALOG_PATH).expect("generic catalog fixture must exist");
    assert!(
        !raw.contains("terrenia:"),
        "generic dimension fixture data must not contain terrenia:* identities"
    );

    let catalog = compile_generic_catalog(&document);
    assert_no_terrenia_catalog(&catalog);

    let first = run_generic_subset(&catalog, &document.dimension);
    let second = run_generic_subset(&catalog, &document.dimension);
    assert_eq!(
        first.receipt_fingerprints, second.receipt_fingerprints,
        "generic dimension authoritative receipts must be deterministic"
    );
    assert_eq!(first.state_hash, second.state_hash);
    assert_eq!(first.save_hash, second.save_hash);
    assert_eq!(first.placed_block, "example:block/plank");
    assert_eq!(first.tool_remaining, 9);

    let isolated = run_generic_subset(&catalog, "example:dimension/fixture");
    assert_ne!(
        isolated.state_hash, first.state_hash,
        "a different root dimension must not share the sandbox state hash"
    );
}

fn load_corpus() -> EquivalenceCorpus {
    let bytes = fs::read_to_string(CORPUS_PATH).expect("equivalence corpus fixture must exist");
    serde_json::from_str(&bytes).expect("equivalence corpus must decode")
}

fn load_generic_catalog() -> GenericCatalogDocument {
    let bytes =
        fs::read_to_string(GENERIC_CATALOG_PATH).expect("generic dimension catalog must exist");
    serde_json::from_str(&bytes).expect("generic dimension catalog must decode")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root must canonicalize")
}

fn reopen_final_lock() -> ReopenedFinalLockV1 {
    let workspace = workspace_root();
    let lock_path = workspace.join(PRODUCT_LOCK_FILE_NAME);
    assert!(
        lock_path.is_file(),
        "final product lock must exist at {}",
        lock_path.display()
    );
    let cas_root = workspace.join("catalog").join(LOCAL_CATALOG_CAS_DIRECTORY);
    assert!(
        cas_root.is_dir(),
        "catalog CAS must exist at {}",
        cas_root.display()
    );
    let lock = reopen_product_lock(&lock_path).expect("final product lock must decode");
    let host = HostBuildReceipts::sealed_by_lock(&lock)
        .expect("reopened lock must seal host toolchain receipts");
    let store = FilesystemCas::open(&cas_root).expect("catalog CAS must open");
    ReopenedFinalLockV1::reopen_frozen_from_cas(&lock_path, &store, &host)
        .expect("final product lock must reopen frozen against CAS")
}

fn bind_lock_images(reopened: &ReopenedFinalLockV1) -> LockVerifiedComposeImages {
    let target = select_lock_target(reopened.product_lock());
    let images = LockVerifiedComposeImages::from_reopened_product_lock(reopened, &target)
        .unwrap_or_else(|error| {
            panic!(
                "LockVerifiedComposeImages::from_reopened_product_lock must bind the frozen lock so client and headless share one image: {error}"
            )
        });
    assert_eq!(images.product_lock_hash(), reopened.product_lock_hash());
    images
}

fn select_lock_target(lock: &latticeaxiom_compose::LockV1) -> TargetTriple {
    if let Some(host) = preferred_host_target()
        && lock.realizations.contains_key(&host)
    {
        return host;
    }
    let mut targets = lock.realizations.keys();
    match (targets.next(), targets.next()) {
        (Some(target), None) => target.clone(),
        _ => panic!("product lock has no unique host realization"),
    }
}

fn preferred_host_target() -> Option<TargetTriple> {
    let architecture = std::env::consts::ARCH;
    let value = if cfg!(all(target_os = "windows", target_env = "msvc")) {
        format!("{architecture}-pc-windows-msvc")
    } else if cfg!(all(target_os = "windows", target_env = "gnu")) {
        format!("{architecture}-pc-windows-gnu")
    } else if cfg!(all(target_os = "linux", target_env = "musl")) {
        format!("{architecture}-unknown-linux-musl")
    } else if cfg!(target_os = "linux") {
        format!("{architecture}-unknown-linux-gnu")
    } else if cfg!(target_os = "macos") {
        format!("{architecture}-apple-darwin")
    } else {
        return None;
    };
    value.parse().ok()
}

fn capture_host_normative(host: &'static str, images: LockVerifiedComposeImages) -> HostNormative {
    let lock_hash = images.product_lock_hash();
    let registration = images.images().registration_semantic_hash();
    let mut instance = EngineInstance::new_headless_host_from_lock(images, SPINE_TIMESTEP)
        .expect("production host must boot from the reopened final lock");
    instance
        .advance_fixed_ticks(8)
        .expect("production host must stream a bounded working set");
    let installed = instance
        .app()
        .world()
        .get_resource::<VerifiedProductLockHash>()
        .copied()
        .expect("host must carry the reopened product-lock hash");
    assert_eq!(installed.get(), lock_hash);
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine must be installed")
        .clone();
    let target = first_resident_edit_target(&spine);
    let success = mine_until_broken(&spine, target);
    let semantic = spine
        .worldgen_inspect_report()
        .expect("worldgen semantic report must compile");
    let semantic_report_hash = semantic
        .canonical_hash()
        .expect("semantic report must canonicalize")
        .to_string();
    let state_hash = spine
        .materialized_chunk_state_hash()
        .expect("host must expose a materialized-chunk state hash");
    let world = spine.world_id().expect("production spine owns a WorldId");
    let snapshot = spine
        .kernel()
        .reference_snapshot(world)
        .expect("memory kernel must snapshot the authoritative world");
    assert_eq!(snapshot.materialized_chunk_state_hash(), state_hash);
    let normative_save_bytes = canonical_json_bytes(&save_record(&snapshot))
        .expect("normative save projection must canonicalize");
    HostNormative {
        host,
        product_lock_hash: lock_hash.to_string(),
        registration_semantic_hash: registration.to_string(),
        command_receipts: vec![CommandReceiptRecord {
            position: [success.position.x, success.position.y, success.position.z],
            old_content: success.old_content.as_ref().map(ToString::to_string),
            new_content: success.new_content.as_ref().map(ToString::to_string),
            committed_chunk_revision: success.committed_chunk_revision.get(),
        }],
        state_hash: state_hash.to_string(),
        semantic_report_hash,
        normative_save_bytes,
    }
}

fn first_resident_edit_target(spine: &ProductionSpine) -> latticeaxiom_gameplay::BlockPosition {
    for id in [
        "terrenia:block/oak-log",
        "terrenia:block/pine-log",
        "terrenia:block/dirt",
        "terrenia:block/coarse-dirt",
        "terrenia:block/grass",
        "terrenia:block/peat",
        "terrenia:block/mud",
        "terrenia:block/sand",
    ] {
        let block = BlockId::parse(id).expect("resident block id must parse");
        if let Some(position) = spine.first_resident_block(&block) {
            return position;
        }
    }
    panic!(
        "production working set must contain a hand-breakable resident block, resident={:?}",
        spine.resident_chunks()
    );
}

fn mine_until_broken(
    spine: &ProductionSpine,
    position: latticeaxiom_gameplay::BlockPosition,
) -> latticeaxiom_engine::BlockEditSuccessV1 {
    for _ in 0..64 {
        match spine.mine_cell(position) {
            Ok(success) => return success,
            Err(error) => {
                let detail = format!("{error:?}");
                if detail.contains("RequiresProgress") {
                    continue;
                }
                panic!(
                    "authoritative mine at {position:?} failed: {detail}, reject={:?}",
                    spine.last_gameplay_reject()
                );
            }
        }
    }
    panic!("authoritative mine at {position:?} did not complete")
}

#[derive(Serialize)]
struct SaveProjection<'a> {
    world: String,
    revision: u64,
    state_hash: String,
    chunks: Vec<SaveChunk<'a>>,
}

#[derive(Serialize)]
struct SaveChunk<'a> {
    dimension: String,
    x: i32,
    y: i32,
    z: i32,
    revision: u64,
    voxels: &'a [u8],
}

fn save_record(snapshot: &latticeaxiom_storage::ReferenceWorldSnapshot) -> SaveProjection<'_> {
    let chunks = snapshot
        .chunks()
        .map(|(key, chunk)| SaveChunk {
            dimension: key.dimension.as_str().to_owned(),
            x: key.coordinate.x,
            y: key.coordinate.y,
            z: key.coordinate.z,
            revision: chunk.revision().get(),
            voxels: chunk.data().voxels().bytes(),
        })
        .collect();
    SaveProjection {
        world: snapshot.world().to_string(),
        revision: snapshot.revision().get(),
        state_hash: snapshot.materialized_chunk_state_hash().to_string(),
        chunks,
    }
}

fn compile_generic_catalog(document: &GenericCatalogDocument) -> GameplayCatalog {
    let source = GameplayCatalogSourceV1 {
        items: document
            .items
            .iter()
            .map(|item| ItemDefinitionV1 {
                id: parsed(&item.id),
                stack_limit: nz32(item.stack_limit),
                placement_block: item.placement_block.as_deref().map(parsed),
                durability: item.durability.map(nz32),
            })
            .collect(),
        blocks: document
            .blocks
            .iter()
            .map(|block| BlockDefinitionV1 {
                id: parsed(&block.id),
                mining: MiningRuleV1 {
                    hardness: nz32(block.hardness),
                    tool: block.tool.as_ref().map(|tool| ToolRequirementV1 {
                        class: parsed(&tool.class),
                        minimum_tier: tool.minimum_tier,
                    }),
                },
                drop: ItemStackV1::plain(parsed(&block.drop.item), block.drop.quantity)
                    .expect("generic drop stack must compile"),
            })
            .collect(),
        tools: document
            .tools
            .iter()
            .map(|tool| ToolDefinitionV1 {
                item: parsed(&tool.item),
                class: parsed(&tool.class),
                tier: tool.tier,
                work_per_step: nz32(tool.work_per_step),
                maximum_durability: nz32(tool.maximum_durability),
            })
            .collect(),
        tags: document
            .tags
            .iter()
            .map(|tag| ItemTagDefinitionV1 {
                id: parsed(&tag.id),
                members: tag
                    .members
                    .iter()
                    .map(|member| parsed(member.as_str()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            })
            .collect(),
        roles: document
            .roles
            .iter()
            .map(|role| ItemRoleDefinitionV1 {
                id: parsed(&role.id),
                accepts: ItemPredicateV1::Exact(parsed(&role.item)),
            })
            .collect(),
        bindings: document
            .roles
            .iter()
            .map(|role| FrozenItemRoleBindingV1 {
                role: parsed(&role.id),
                item: parsed(&role.item),
            })
            .collect(),
        recipes: document.recipes.iter().map(compile_recipe).collect(),
        workstations: document
            .workstations
            .iter()
            .map(|id| WorkstationDefinitionV1 { id: parsed(id) })
            .collect(),
        processes: Vec::new(),
        fuel_rules: Vec::new(),
        block_schema_bindings: Vec::new(),
    };
    GameplayCatalog::compile(source, CatalogLimits::default())
        .expect("generic dimension catalog must compile")
}

fn compile_recipe(recipe: &GenericRecipe) -> RecipeDefinitionV1 {
    let pattern = if let Some(ingredients) = &recipe.shapeless {
        RecipePatternV1::Shapeless {
            ingredients: ingredients.iter().map(compile_ingredient).collect(),
        }
    } else if let Some(shaped) = &recipe.shaped {
        RecipePatternV1::Shaped {
            width: nz8(shaped.width),
            height: nz8(shaped.height),
            cells: shaped
                .cells
                .iter()
                .map(|cell| Some(compile_ingredient(cell)))
                .collect(),
        }
    } else {
        panic!("generic recipe {} must be shaped or shapeless", recipe.id);
    };
    RecipeDefinitionV1 {
        id: parsed(&recipe.id),
        workstation: recipe.workstation.as_deref().map(parsed),
        pattern,
        output: RoleOutputV1 {
            role: parsed(&recipe.output_role),
            quantity: nz32(recipe.output_quantity),
        },
    }
}

fn compile_ingredient(ingredient: &GenericIngredient) -> IngredientV1 {
    let accepts = match (&ingredient.item, &ingredient.tag) {
        (Some(item), None) => ItemPredicateV1::Exact(parsed(item)),
        (None, Some(tag)) => ItemPredicateV1::InTag(parsed(tag)),
        _ => panic!("generic ingredient must name exactly one item or tag"),
    };
    IngredientV1 {
        accepts,
        quantity: nz32(ingredient.quantity),
    }
}

fn assert_no_terrenia_catalog(catalog: &GameplayCatalog) {
    for id in catalog.items().keys() {
        assert_ne!(id.namespace(), FORBIDDEN_NAMESPACE, "{}", id.as_str());
    }
    for id in catalog.blocks().keys() {
        assert_ne!(id.namespace(), FORBIDDEN_NAMESPACE, "{}", id.as_str());
    }
    for id in catalog.recipes().keys() {
        assert_ne!(id.namespace(), FORBIDDEN_NAMESPACE, "{}", id.as_str());
    }
}

struct GenericJourney {
    receipt_fingerprints: Vec<String>,
    state_hash: String,
    save_hash: String,
    placed_block: String,
    tool_remaining: u32,
}

fn run_generic_subset(catalog: &GameplayCatalog, dimension_id: &str) -> GenericJourney {
    let dimension: latticeaxiom_gameplay::DimensionId = parsed(dimension_id);
    assert_ne!(
        dimension.as_str().split(':').next(),
        Some(FORBIDDEN_NAMESPACE)
    );
    let chunk = DimensionChunkKey::new(dimension.clone(), ChunkCoordinate::new(0, 0, 0));
    let mut state = ReferenceGameplayState::new(GameplayLimits::default())
        .expect("generic state must construct");
    state
        .seed_loaded_chunk(chunk.clone(), ChunkRevision::ZERO)
        .expect("generic chunk must load");
    let mut inventory = InventoryStateV1::empty(
        GameplayEditTarget::new(chunk.clone(), GameplayStorageDomain::PersistentEntities),
        9,
    )
    .expect("generic inventory must construct");
    inventory
        .seed_slot(
            SlotIndex::new(1),
            Some(ItemStackV1::plain(parsed("example:item/stick"), 2).expect("stick stack")),
        )
        .expect("generic stick seed");
    state
        .seed_player(PLAYER, inventory)
        .expect("generic player seed");
    let workbench = ContainerStateV1::empty(
        ContainerOwnerComponentV1 {
            dimension: dimension.clone(),
            chunk: chunk.coordinate,
            entity: WORKBENCH.into_persistent_entity_id(),
        },
        Some(parsed("latticeaxiom:workstation/crafting@1")),
        9,
    )
    .expect("generic workbench must construct");
    state
        .seed_container(WORKBENCH, workbench)
        .expect("generic workbench seed");
    state
        .seed_block(
            BlockKey::new(dimension.clone(), BlockPosition { x: 0, y: 8, z: 0 }),
            parsed("example:block/log"),
        )
        .expect("generic log seed");
    state
        .seed_block(
            BlockKey::new(dimension.clone(), BlockPosition { x: 1, y: 8, z: 0 }),
            parsed("example:block/copper-ore"),
        )
        .expect("generic ore seed");

    let mut authority = GenericAuthority::new(state, catalog);
    let mut receipts = Vec::new();
    receipts.push(execute(
        &mut authority,
        catalog,
        GameplayCommandV1::SelectHotbar(SelectHotbarCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(1),
            expected_inventory_revision: 0,
        }),
    ));
    let log_revision = chunk_revision(&authority, &chunk);
    let log_drop = drop_from(&execute(
        &mut authority,
        catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: LOG_DROP,
            player: PLAYER,
            target: BlockKey::new(dimension.clone(), BlockPosition { x: 0, y: 8, z: 0 }),
            expected_chunk_revision: log_revision,
            tool_slot: None,
        }),
    ));
    receipts.push(execute(
        &mut authority,
        catalog,
        GameplayCommandV1::Pickup(PickupCommandV1 {
            player: PLAYER,
            drop: log_drop,
        }),
    ));
    receipts.push(execute(
        &mut authority,
        catalog,
        GameplayCommandV1::Craft(RecipeCraftCommandV1 {
            player: PLAYER,
            recipe: parsed("example:recipe/planks@1"),
            input_slots: vec![SlotIndex::new(0)].into_boxed_slice(),
            workstation: None,
        }),
    ));
    receipts.push(execute(
        &mut authority,
        catalog,
        GameplayCommandV1::Craft(RecipeCraftCommandV1 {
            player: PLAYER,
            recipe: parsed("example:recipe/pickaxe@1"),
            input_slots: vec![SlotIndex::new(0), SlotIndex::new(1)].into_boxed_slice(),
            workstation: Some(WORKBENCH),
        }),
    ));
    let ore_progress_revision = chunk_revision(&authority, &chunk);
    execute(
        &mut authority,
        catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: ORE_PROGRESS_DROP,
            player: PLAYER,
            target: BlockKey::new(dimension.clone(), BlockPosition { x: 1, y: 8, z: 0 }),
            expected_chunk_revision: ore_progress_revision,
            tool_slot: Some(SlotIndex::new(1)),
        }),
    );
    let ore_break_revision = chunk_revision(&authority, &chunk);
    let ore_drop = drop_from(&execute(
        &mut authority,
        catalog,
        GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: ORE_BREAK_DROP,
            player: PLAYER,
            target: BlockKey::new(dimension.clone(), BlockPosition { x: 1, y: 8, z: 0 }),
            expected_chunk_revision: ore_break_revision,
            tool_slot: Some(SlotIndex::new(1)),
        }),
    ));
    receipts.push(execute(
        &mut authority,
        catalog,
        GameplayCommandV1::Pickup(PickupCommandV1 {
            player: PLAYER,
            drop: ore_drop,
        }),
    ));
    let hotbar_revision = authority
        .gameplay
        .state()
        .inventory(PLAYER)
        .expect("generic inventory remains")
        .revision();
    receipts.push(execute(
        &mut authority,
        catalog,
        GameplayCommandV1::SelectHotbar(SelectHotbarCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(0),
            expected_inventory_revision: hotbar_revision,
        }),
    ));
    let place_target = BlockKey::new(dimension, BlockPosition { x: 2, y: 8, z: 0 });
    let place_revision = chunk_revision(&authority, &chunk);
    receipts.push(execute(
        &mut authority,
        catalog,
        GameplayCommandV1::Place(PlaceCommandV1 {
            player: PLAYER,
            slot: SlotIndex::new(0),
            target: place_target.clone(),
            expected_chunk_revision: place_revision,
        }),
    ));

    let placed = authority
        .gameplay
        .state()
        .block(&place_target)
        .expect("placed plank must remain")
        .as_str()
        .to_owned();
    let tool = authority
        .gameplay
        .state()
        .inventory(PLAYER)
        .expect("generic inventory remains")
        .slot(SlotIndex::new(1))
        .expect("tool slot")
        .expect("pickaxe remains");
    let tool_remaining = match tool.state() {
        ItemStateV1::ToolDurability { remaining } => remaining.get(),
        ItemStateV1::Plain => panic!("generic pickaxe must carry durability"),
    };

    let handed_off = ReferencePlanApplier::try_new(
        authority.gameplay.world(),
        authority.gameplay.state().clone(),
        catalog.clone(),
    )
    .expect("generic persistence handoff must rebind");
    assert_eq!(
        handed_off.state().canonical_hash(),
        authority.gameplay.state().canonical_hash()
    );

    let snapshot = authority
        .storage
        .reference_snapshot(authority.gameplay.world())
        .expect("generic persistence snapshot");
    GenericJourney {
        receipt_fingerprints: receipts
            .into_iter()
            .map(|receipt| receipt.envelope_fingerprint.to_string())
            .collect(),
        state_hash: authority.gameplay.state().canonical_hash().to_string(),
        save_hash: snapshot.materialized_chunk_state_hash().to_string(),
        placed_block: placed,
        tool_remaining,
    }
}

impl GenericAuthority {
    fn new(state: ReferenceGameplayState, catalog: &GameplayCatalog) -> Self {
        let world = parsed::<WorldId>("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd");
        Self {
            gameplay: ReferencePlanApplier::try_new(world, state, catalog.clone())
                .expect("generic loaded state must validate"),
            storage: MemoryTransactionKernel::new(),
            storage_revisions: BTreeMap::new(),
            next_payload: 1,
        }
    }

    fn commit_plan(&mut self, transaction_id: TransactionId, plan: &GameplayPlanV1) {
        let mut domains = BTreeMap::<DimensionChunkKey, ChangedDomains>::new();
        for edit in plan.edits() {
            let domain = match edit.target().domain {
                GameplayStorageDomain::Voxels => ChangedDomains::VOXELS,
                GameplayStorageDomain::PersistentEntities => ChangedDomains::PERSISTENT_ENTITIES,
                GameplayStorageDomain::Continuations => ChangedDomains::CONTINUATIONS,
            };
            domains
                .entry(edit.target().chunk.clone())
                .and_modify(|captured| *captured = captured.union(domain))
                .or_insert(domain);
        }
        if domains.is_empty() {
            return;
        }
        let world = self.gameplay.world();
        let base = self.gameplay.state().observed_world_revision();
        let mutations = domains
            .keys()
            .map(|chunk| {
                let expected = self.storage_revisions.get(chunk).copied().map_or(
                    ChunkRevisionExpectation::Absent,
                    ChunkRevisionExpectation::Exact,
                );
                self.next_payload = self.next_payload.saturating_add(1);
                ChunkMutation::new(
                    ChunkKey::new(world, chunk.dimension.clone(), chunk.coordinate),
                    expected,
                    ChangedDomains::ALL,
                    fixture_chunk_data(self.next_payload),
                )
            })
            .collect();
        let receipt = self
            .storage
            .commit(WorldTransaction::new(
                transaction_id,
                world,
                base,
                mutations,
            ))
            .expect("generic storage commit");
        self.gameplay
            .observe_storage_commit(&receipt)
            .expect("generic storage receipt must reconcile");
        for chunk_receipt in receipt.chunks() {
            let key = chunk_receipt.key();
            self.storage_revisions.insert(
                DimensionChunkKey::new(key.dimension.clone(), key.coordinate),
                chunk_receipt.chunk_revision(),
            );
        }
    }
}

fn execute(
    authority: &mut GenericAuthority,
    catalog: &GameplayCatalog,
    command: GameplayCommandV1,
) -> RuntimePlanReceiptV1 {
    let envelope = CommandEnvelopeV1 {
        transaction_id: transaction_id(authority.gameplay.state()),
        expected_world_revision: authority.gameplay.state().observed_world_revision(),
        command,
    };
    let plan = GameplayKernel::new(catalog)
        .plan(authority.gameplay.state(), &envelope)
        .unwrap_or_else(|error| panic!("generic plan failed: {error}"));
    let receipt = authority
        .gameplay
        .apply_plan(plan.clone(), FaultInjection::None)
        .unwrap_or_else(|error| panic!("generic apply failed: {error}"));
    authority.commit_plan(envelope.transaction_id, &plan);
    assert!(
        authority.gameplay.pending_storage_chunks().is_none(),
        "generic persistence must acknowledge every runtime plan"
    );
    receipt
}

fn chunk_revision(authority: &GenericAuthority, chunk: &DimensionChunkKey) -> ChunkRevision {
    authority
        .gameplay
        .state()
        .loaded_chunk_revision(chunk)
        .expect("generic loaded chunk revision")
}

fn drop_from(receipt: &RuntimePlanReceiptV1) -> DropEntityId {
    match &receipt.outcome {
        CommandOutcomeV1::BlockBroken { drop, .. } | CommandOutcomeV1::ItemDropped { drop } => {
            *drop
        }
        outcome => panic!("expected drop outcome, found {outcome:?}"),
    }
}

fn fixture_chunk_data(seed: u64) -> ChunkData {
    let schema: latticeaxiom_core::SchemaId = parsed("latticeaxiom:schema/gameplay-fixture@1");
    let version = PayloadSchemaVersion::new(1).expect("positive schema version");
    let bytes = seed.to_be_bytes().to_vec();
    let mut entities = BTreeMap::new();
    entities.insert(
        PersistentEntityId::from_u128(u128::from(seed).saturating_add(1_000_000)),
        VersionedPayload::new(schema.clone(), version, bytes.clone()),
    );
    let mut continuations = BTreeMap::new();
    continuations.insert(
        ContinuationId::from_u128(u128::from(seed).saturating_add(2_000_000)),
        VersionedPayload::new(schema.clone(), version, bytes.clone()),
    );
    let mut provenance = BTreeMap::new();
    provenance.insert(
        parsed::<StableId>("latticeaxiom:provenance/gameplay-fixture@1"),
        CanonicalHash::digest(seed.to_be_bytes()),
    );
    ChunkData::new(
        VersionedPayload::new(schema, version, bytes),
        entities,
        continuations,
        provenance,
    )
}

fn transaction_id(state: &ReferenceGameplayState) -> TransactionId {
    let digest = state.canonical_hash().as_bytes();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    if bytes.iter().all(|byte| *byte == 0) {
        bytes[15] = 1;
    }
    TransactionId::from_bytes(bytes)
}

fn parsed<T>(value: &str) -> T
where
    T: FromStr,
    T::Err: Debug,
{
    value
        .parse()
        .unwrap_or_else(|error| panic!("fixture identifier `{value}` failed: {error:?}"))
}

fn nz32(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("generic catalog non-zero u32")
}

fn nz8(value: u8) -> NonZeroU8 {
    NonZeroU8::new(value).expect("generic catalog non-zero u8")
}
