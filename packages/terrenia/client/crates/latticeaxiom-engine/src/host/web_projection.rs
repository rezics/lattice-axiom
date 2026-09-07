//! Cached package presentation and bounded live Web projections.

use std::collections::BTreeMap;

use bevy::prelude::{Resource, World};
use latticeaxiom_client_ui::GameOverlayV1;
use latticeaxiom_gameplay::{GameplayModeV1, ItemStateV1, RecipeId};
use serde_json::{Value, json};

use super::{ProductionSpine, ProductionSurfaceRouter};

const ITEM_PAGE_SIZE: usize = 96;
const RECIPE_PAGE_SIZE: usize = 32;
const QUERY_CHAR_LIMIT: usize = 128;
const STATIC_PAGE_BYTES: usize = 256 * 1024;

#[derive(Debug)]
struct CachedItem {
    category: String,
    search: String,
    row: Value,
    durability: Option<u32>,
}

#[derive(Debug, Default, Resource)]
struct CatalogProjection {
    items: Vec<CachedItem>,
    matching: Vec<usize>,
    item_bytes: Vec<usize>,
    item_pages: Vec<Vec<usize>>,
    recipe_pages: Vec<Vec<usize>>,
    error: Option<String>,
    by_id: BTreeMap<String, usize>,
    categories: Vec<Value>,
    recipes: Vec<(RecipeId, Value)>,
    query: String,
    category: String,
    page: usize,
    recipe_page: usize,
}

/// Rebuilds immutable presentation metadata only when the selected world changes.
pub(super) fn refresh_catalog(world: &mut World) {
    let mut cache = CatalogProjection::default();
    if let Some(spine) = world.get_resource::<ProductionSpine>()
        && let Some(catalog) = spine.gameplay_catalog()
    {
        let resources = world.get_resource::<crate::resource_packs::ClientResourcePacks>();
        let mut categories = catalog.categories().values().collect::<Vec<_>>();
        categories.sort_by(|a, b| (a.sort_order, &a.id).cmp(&(b.sort_order, &b.id)));
        cache.categories = categories.iter().map(|category| json!({"id":category.id.as_str(),"name":bounded(&category.display_name,80)})).collect();
        for (id, definition) in catalog.items() {
            let name = bounded(&spine.content_display(id.as_str()).name, 160);
            let category_id = catalog.category_for_item(id);
            let category = category_id
                .and_then(|id| catalog.categories().get(id))
                .map(|c| bounded(&c.display_name, 80))
                .unwrap_or_else(|| "Other".to_owned());
            let material = definition
                .placement_block
                .as_ref()
                .map_or(id.as_str(), |block| block.as_str());
            let color = resources
                .and_then(|resources| resources.0.material(material))
                .map_or_else(
                    || css_rgba(super::chunk_mesh::block_color(material)),
                    |material| css_rgb(material.color),
                );
            let row = json!({"id":id.as_str(),"name":name,"category":category,"color":color});
            cache
                .by_id
                .insert(id.as_str().to_owned(), cache.items.len());
            cache.items.push(CachedItem {
                category: category_id.map_or_else(String::new, |id| id.as_str().to_owned()),
                search: format!("{} {} {}", id.as_str(), name, category).to_lowercase(),
                row,
                durability: definition.durability.map(std::num::NonZeroU32::get),
            });
        }
        if cache.items.iter().any(|item| item.category.is_empty()) {
            cache
                .categories
                .push(json!({"id":"uncategorized","name":"Other"}));
        }
        cache.recipes = catalog.recipes().iter().map(|(id, definition)| (id.clone(),json!({
            "id":id.as_str(),"name":recipe_name(&spine.content_display(id.as_str()).name),
            "workstation":definition.workstation.as_ref().map(|id| id.as_str()),"output":catalog.resolve_output(&definition.output).ok().map(|stack|stack.item().to_string())
        }))).collect();
    }
    cache.matching = (0..cache.items.len()).collect();
    cache.item_bytes = cache
        .items
        .iter()
        .map(|item| item.row.to_string().len() + 128)
        .collect();
    let recipe_bytes = cache
        .recipes
        .iter()
        .map(|(_, row)| row.to_string().len() + 152)
        .collect::<Vec<_>>();
    match partition_pages(
        &(0..cache.recipes.len()).collect::<Vec<_>>(),
        &recipe_bytes,
        RECIPE_PAGE_SIZE,
    ) {
        Ok(pages) => cache.recipe_pages = pages,
        Err(error) => cache.error = Some(error),
    }
    if serde_json::to_vec(&cache.categories).map_or(true, |bytes| bytes.len() > STATIC_PAGE_BYTES) {
        cache.categories.clear();
        cache.error = Some(
            "The selected catalog's category metadata exceeds the UI payload budget".to_owned(),
        );
    }
    rebuild_item_pages(&mut cache);
    world.insert_resource(cache);
}

/// Changes the cached catalog view without sending an unbounded list to JavaScript.
pub(super) fn handle_catalog_query(world: &mut World, params: &Value) -> Result<(), String> {
    if !world.contains_resource::<ProductionSpine>() {
        return Err("No active world".to_owned());
    }
    let Some(mut cache) = world.get_resource_mut::<CatalogProjection>() else {
        return Err("Catalog is loading".to_owned());
    };
    let mut query = cache.query.clone();
    let mut category = cache.category.clone();
    if let Some(value) = params.get("query") {
        let value = value
            .as_str()
            .ok_or_else(|| "Catalog query must be text".to_owned())?;
        if value.chars().count() > QUERY_CHAR_LIMIT {
            return Err("Catalog query exceeds 128 characters".to_owned());
        }
        query = value.to_owned();
    }
    if let Some(value) = params.get("category") {
        category = value
            .as_str()
            .ok_or_else(|| "Catalog category must be text".to_owned())?
            .to_owned();
        if category == "all" {
            category.clear();
        }
        if !category.is_empty()
            && !cache
                .categories
                .iter()
                .any(|row| row["id"].as_str() == Some(category.as_str()))
        {
            return Err("Unknown item category".to_owned());
        }
    }
    let changed = query != cache.query || category != cache.category;
    let page = optional_page(params, "page")?.unwrap_or(if changed { 0 } else { cache.page });
    let recipe_page = optional_page(params, "recipePage")?.unwrap_or(cache.recipe_page);
    cache.query = query;
    cache.category = category;
    cache.page = page;
    cache.recipe_page = recipe_page;
    if changed {
        cache.matching = matching_indices(&cache);
        rebuild_item_pages(&mut cache);
    }
    Ok(())
}

fn optional_page(params: &Value, key: &str) -> Result<Option<usize>, String> {
    params
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| format!("{key} must be a nonnegative page number"))
        })
        .transpose()
}

fn matching_indices(cache: &CatalogProjection) -> Vec<usize> {
    let query = cache.query.to_lowercase();
    cache
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            (cache.category.is_empty()
                || cache.category == item.category
                || cache.category == "uncategorized" && item.category.is_empty())
                && (query.is_empty() || item.search.contains(&query))
        })
        .map(|(index, _)| index)
        .collect()
}

fn partition_pages(
    indices: &[usize],
    bytes: &[usize],
    max_rows: usize,
) -> Result<Vec<Vec<usize>>, String> {
    let mut pages = vec![Vec::new()];
    let mut page_bytes = 0;
    for &index in indices {
        let size = bytes[index];
        if size > STATIC_PAGE_BYTES {
            return Err("A catalog declaration exceeds the UI payload budget; shorten its identity or metadata".to_owned());
        }
        let last = pages.len() - 1;
        if pages[last].len() == max_rows || page_bytes + size > STATIC_PAGE_BYTES {
            pages.push(Vec::new());
            page_bytes = 0;
        }
        let last = pages.len() - 1;
        pages[last].push(index);
        page_bytes += size;
    }
    Ok(pages)
}

fn rebuild_item_pages(cache: &mut CatalogProjection) {
    match partition_pages(&cache.matching, &cache.item_bytes, ITEM_PAGE_SIZE) {
        Ok(pages) => cache.item_pages = pages,
        Err(error) => {
            cache.error = Some(error);
            cache.item_pages.clear();
        }
    }
}

/// Projects the inventory using cached item names and resource-pack colors.
pub(super) fn inventory_slots(world: &World) -> Value {
    let Some(spine) = world.get_resource::<ProductionSpine>() else {
        return json!([]);
    };
    let cache = world.get_resource::<CatalogProjection>();
    json!(spine.inventory_view().map(|view|view.slots().iter().enumerate().map(|(index,stack)| {
        let item = stack.as_ref().map(|stack|stack.item().as_str());
        let metadata = item.and_then(|id|cache.and_then(|cache|cache.by_id.get(id).and_then(|index|cache.items.get(*index))));
        json!({"index":index,"item":item,"name":metadata.map(|item|item.row["name"].as_str().unwrap_or_default()).unwrap_or_else(||item.unwrap_or_default()),
            "quantity":stack.as_ref().map_or(0,|stack|stack.quantity()),"preview":item.and_then(|id|super::item_models::preview_url(world,id)),"color":metadata.map(|item|item.row["color"].as_str().unwrap_or("#9aa7aa")).unwrap_or("#9aa7aa")})
    }).collect::<Vec<_>>()).unwrap_or_default())
}

/// Projects at most one catalog page and the currently visible diagnostic panels.
pub(super) fn game(world: &World, debug: bool) -> Option<Value> {
    let spine = world.get_resource::<ProductionSpine>()?;
    let route = world
        .get_resource::<ProductionSurfaceRouter>()?
        .inner()
        .route();
    let target = super::web_settings::effective(world,"latticeaxiom:setting/inspect/enabled").and_then(|value|value.as_bool()).unwrap_or(true)
        .then(||spine.current_target()).flatten().map(|hit|json!({
            "name":bounded(&hit.block_display_name,160),"id":hit.block_id.as_str(),"owner":hit.declared_by,
            "position":format!("{:?}",hit.observation),"harvestTool":hit.harvest_tool,"hardnessTicks":hit.hardness_ticks,
            "anchor":super::web_settings::effective(world,"latticeaxiom:setting/inspect/anchor").unwrap_or(json!("top-center"))
        }));
    let cache = world.get_resource::<CatalogProjection>();
    let (items, recipes, page) = if route.overlay() != GameOverlayV1::None {
        cache.map_or((Vec::new(),Vec::new(),Value::Null),|cache| {
            let pages=cache.item_pages.len().max(1);
            let page=cache.page.min(pages-1);
            let items=cache.item_pages.get(page).into_iter().flatten().map(|index| { let mut row=cache.items[*index].row.clone(); row["preview"]=json!(super::item_models::preview_url(world,row["id"].as_str().unwrap_or_default())); row }).collect::<Vec<_>>();
            let recipe_pages=cache.recipe_pages.len().max(1);
            let recipe_page=cache.recipe_page.min(recipe_pages-1);
            let workstation = (route.overlay()==GameOverlayV1::Workbench).then(super::hud::crafting_workstation);
            let recipes = cache.recipe_pages.get(recipe_page).into_iter().flatten().map(|index| {
                let (id,row)=&cache.recipes[*index];
                let mut row = row.clone(); row["preview"]=json!(row["output"].as_str().and_then(|item| super::item_models::preview_url(world,item))); row["craftable"]=json!(spine.recipe_is_craftable(id,workstation.as_ref())); row
            }).collect::<Vec<_>>();
            (items,recipes,json!({"page":page,"pageSize":ITEM_PAGE_SIZE,"total":cache.matching.len(),"pages":pages,"query":cache.query,"category":if cache.category.is_empty(){"all"}else{cache.category.as_str()},"categories":cache.categories,
                "recipePage":recipe_page,"recipePages":recipe_pages,"recipeTotal":cache.recipes.len(),"error":cache.error}))
        })
    } else {
        (Vec::new(), Vec::new(), Value::Null)
    };
    let sections = if debug {
        debug_sections(world, spine)
    } else {
        Vec::new()
    };
    let hint = match spine.last_reject() {
        Some(latticeaxiom_player::BlockEditRejectV1::RequiresTool { required }) => {
            Some(format!("Requires {required}"))
        }
        Some(latticeaxiom_player::BlockEditRejectV1::ToolBroken) => {
            Some("Your tool is broken".to_owned())
        }
        Some(latticeaxiom_player::BlockEditRejectV1::NotBreakable) => {
            Some("This block cannot be mined".to_owned())
        }
        _ => None,
    };
    let inventory = spine.inventory_view();
    let selected = inventory
        .as_ref()
        .and_then(|inventory| inventory.selected());
    let durability = selected.and_then(|stack| match stack.state() {
        ItemStateV1::ToolDurability { remaining } => Some(remaining.get()),
        ItemStateV1::Plain => None,
    });
    let tool = selected.zip(durability).map(|(stack,remaining)| {
        let metadata = cache.and_then(|cache|cache.by_id.get(stack.item().as_str()).and_then(|index|cache.items.get(*index)));
        json!({"name":metadata.map(|item|item.row["name"].as_str().unwrap_or_default()).unwrap_or(stack.item().as_str()),"durability":remaining,"maxDurability":metadata.and_then(|item|item.durability).unwrap_or(remaining)})
    });
    Some(
        json!({"overlay":route.overlay(),"modal":route.modal(),"transition":route.transition(),"creative":spine.gameplay_mode()==Some(GameplayModeV1::Creative),
        "hotbar":inventory.as_ref().map_or(0,|inventory|inventory.hotbar_slot()),"slots":inventory_slots(world),"items":items,"recipes":recipes,"catalog":page,
        "target":target,"hint":hint,"tool":tool,"toolDurability":durability,"debug":{"visible":debug,"sections":sections},
        "saving":world.get_resource::<super::persistent::PersistentGameSession>().map(|session|session.web_status())}),
    )
}

fn debug_sections(world: &World, spine: &ProductionSpine) -> Vec<Value> {
    let pose = spine.player_pose();
    let mut sections = vec![
        json!({"title":"Performance","rows":world.get_resource::<crate::frame_monitor::FrameMonitor>().map(crate::frame_monitor::FrameMonitor::web_rows).unwrap_or_default()}),
        json!({"title":"Player","rows":[{"label":"Position","value":format!("{:.2} / {:.2} / {:.2}",pose.translation.x,pose.translation.y,pose.translation.z)},{"label":"Grounded","value":pose.grounded.to_string()}]}),
    ];
    if super::web_settings::effective(world, "latticeaxiom:setting/observability/detail")
        .as_ref()
        .and_then(Value::as_str)
        == Some("detailed")
    {
        let diagnostics = spine.working_set_diagnostics();
        sections.push(json!({"title":"World streaming","rows":[
            {"label":"Resident / visible","value":format!("{} / {}",diagnostics.resident(),diagnostics.visible())},
            {"label":"Active / in flight","value":format!("{} / {}",diagnostics.active(),diagnostics.in_flight())},
            {"label":"Edited chunks","value":diagnostics.dirty().to_string()},
            {"label":"Cached / pending save","value":format!("{} / {}",spine.cached_chunk_count(),spine.pending_persistence_count())},
            {"label":"Reserved / budget MiB","value":format!("{} / {}",diagnostics.reserved_bytes()/1048576,diagnostics.byte_budget()/1048576)},
            {"label":"Render / full detail","value":format!("{} / {} chunks",spine.target_render_distance(),spine.full_detail_distance())}
        ]}));
    }
    sections
}

/// Returns only recognized CSS custom properties with numeric RGB color values.
pub(super) fn theme(world: &World) -> Value {
    let mut values = serde_json::Map::new();
    if let Some(resources) = world.get_resource::<crate::resource_packs::ClientResourcePacks>() {
        for token in [
            "background",
            "surface",
            "surface-hover",
            "line",
            "muted",
            "accent",
            "danger",
            "text",
        ] {
            if let Some(material) = resources.0.material(&format!("latticeaxiom:ui/{token}")) {
                values.insert(format!("--{token}"), json!(css_rgb(material.color)));
            }
        }
    }
    Value::Object(values)
}

fn css_rgb(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}
fn css_rgba(color: [f32; 4]) -> String {
    format!(
        "rgb({:.0} {:.0} {:.0})",
        color[0].clamp(0.0, 1.0) * 255.0,
        color[1].clamp(0.0, 1.0) * 255.0,
        color[2].clamp(0.0, 1.0) * 255.0
    )
}
fn recipe_name(label: &str) -> String {
    let label = label
        .rsplit_once('@')
        .filter(|(_, version)| {
            !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .map_or(label, |(name, _)| name);
    bounded(label, 160)
}

fn bounded(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_covers_all_entries_with_both_row_and_payload_bounds() {
        let count = ITEM_PAGE_SIZE * 2 + 1;
        let pages = partition_pages(
            &(0..count).collect::<Vec<_>>(),
            &vec![40; count],
            ITEM_PAGE_SIZE,
        )
        .expect("small rows");
        assert_eq!(pages.len(), 3);
        assert_eq!(
            pages.into_iter().flatten().collect::<Vec<_>>(),
            (0..count).collect::<Vec<_>>()
        );
        let sizes = [STATIC_PAGE_BYTES / 2 + 1; 3];
        let pages = partition_pages(&[0, 1, 2], &sizes, ITEM_PAGE_SIZE)
            .expect("large but representable rows");
        assert_eq!(pages, vec![vec![0], vec![1], vec![2]]);
        assert!(partition_pages(&[0], &[STATIC_PAGE_BYTES + 1], ITEM_PAGE_SIZE).is_err());
    }

    #[test]
    fn category_search_reaches_items_beyond_the_first_page() {
        let items = (0..ITEM_PAGE_SIZE + 1)
            .map(|index| CachedItem {
                category: "blocks".into(),
                search: format!("item {index}"),
                row: json!({}),
                durability: None,
            })
            .collect();
        let mut cache = CatalogProjection {
            items,
            query: format!("item {ITEM_PAGE_SIZE}"),
            category: "blocks".into(),
            ..Default::default()
        };
        let found = matching_indices(&cache);
        assert_eq!(found.len(), 1);
        assert_eq!(
            cache.items[found[0]].search,
            format!("item {ITEM_PAGE_SIZE}")
        );
        cache.category = "tools".into();
        assert!(matching_indices(&cache).is_empty());
    }

    #[test]
    fn theme_only_exposes_recognized_material_tokens_as_css_colors() {
        let pack: latticeaxiom_render_contracts::ResourcePackV1 = serde_json::from_value(json!({
            "schema_version":1,"id":"example:resource-pack/theme@1","kind":"textures","priority":1,
            "materials":{"latticeaxiom:ui/accent":{"color":[0,128,255]},"latticeaxiom:ui/unsupported":{"color":[255,0,0]}}
        })).expect("theme descriptor");
        let mut resources = latticeaxiom_render_contracts::ResolvedResourcePacks::default();
        resources.apply(&pack, None).expect("resource pack applies");
        let mut world = World::new();
        world.insert_resource(crate::resource_packs::ClientResourcePacks(resources));
        assert_eq!(theme(&world), json!({"--accent":"#0080ff"}));
        assert_eq!(
            bounded("\u{6676}\u{683c}\u{4e16}\u{754c}", 3),
            "\u{6676}\u{683c}\u{4e16}"
        );
    }
}
