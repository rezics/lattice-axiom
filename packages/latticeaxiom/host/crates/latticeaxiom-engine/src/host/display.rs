//! Client HUD display names and icons for locked content identities.
//!
//! Presentation-provider rows overlay locale-independent fallbacks from the
//! blocks and tools providers. Omitting the presentation capability still
//! yields a deterministic name and icon for every locked block, tool, and
//! fluid.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use latticeaxiom_compose::RealizedDataRootV1;
use serde::Deserialize;

use super::{
    ProductionHostError,
    catalog::{
        CONTENT_BLOCKS_CAPABILITY, SANDBOX_TOOLS_CAPABILITY, exactly_one_lock_provider,
        required_data_text, required_provider_data,
    },
};
use crate::LockVerifiedComposeImages;

pub(super) const CONTENT_PRESENTATION_CAPABILITY: &str =
    "latticeaxiom:capability/content-presentation@1";
const AUTHORED_DISPLAY_PATH: &str = "data/authored-display-v1.json";
const AUTHORED_TOOL_CATALOG_PATH: &str = "data/authored-tools-v1.json";
pub(super) const AUTHORED_PRESENTATION_ASSETS_PATH: &str = "data/authored-assets-v1.json";
pub(super) const AUTHORED_PRESENTATION_LAYERS_PATH: &str = "data/authored-layers-v1.json";
const D9_BLOCK_IDS_PATH: &str = "data/goldens/d9-block-ids.txt";
const FLUID_IDS_PATH: &str = "data/goldens/fluid-ids.txt";

/// HUD label resolved for one locked content identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentDisplayLabelV1 {
    /// Locale-independent fallback display name.
    pub name: String,
    /// Presentation icon identity, or the missing-presentation derived icon.
    pub icon: String,
}

impl ContentDisplayLabelV1 {
    /// Deterministic name and icon used when presentation data is omitted.
    #[must_use]
    pub fn missing_presentation(content_id: &str) -> Self {
        Self {
            name: title_case_path(content_id),
            icon: derived_icon_id(content_id),
        }
    }
}

/// Compiled HUD display table for locked blocks, tools, and fluids.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentDisplayCatalogV1 {
    rows: BTreeMap<String, ContentDisplayLabelV1>,
    locked: BTreeSet<String>,
    includes_presentation: bool,
}

impl ContentDisplayCatalogV1 {
    /// Returns whether presentation overlay rows were applied.
    #[must_use]
    pub const fn includes_presentation(&self) -> bool {
        self.includes_presentation
    }

    /// Stable locked content identities covered by this table.
    pub fn locked_ids(&self) -> impl Iterator<Item = &str> {
        self.locked.iter().map(String::as_str)
    }

    /// Resolves a HUD label, including item-to-block aliases and the
    /// missing-presentation fallback.
    #[must_use]
    pub fn lookup(&self, content_id: &str) -> ContentDisplayLabelV1 {
        if let Some(row) = self.rows.get(content_id) {
            return row.clone();
        }
        for alias in content_aliases(content_id) {
            if let Some(row) = self.rows.get(&alias) {
                return row.clone();
            }
        }
        ContentDisplayLabelV1::missing_presentation(content_id)
    }
}

/// Optional presentation overlay sources selected by a capability provider.
#[derive(Clone, Copy, Debug)]
pub struct AuthoredPresentationCatalogSourcesV1<'a> {
    /// Display labels and icon bindings.
    pub display: &'a str,
    /// Asset identities referenced by display labels.
    pub assets: &'a str,
}

/// Explicit package-owned sources accepted by the HUD display compiler.
#[derive(Clone, Copy, Debug)]
pub struct AuthoredContentDisplayCatalogSourcesV1<'a> {
    /// Fallback block display labels.
    pub block_display: &'a str,
    /// Fallback tool display labels.
    pub tool_display: &'a str,
    /// Tool identities included in the locked content set.
    pub tool_catalog: &'a str,
    /// Required D9 block identities.
    pub d9_block_ids: &'a str,
    /// Required fluid identities.
    pub fluid_ids: &'a str,
    /// Optional presentation overlay selected by the graph.
    pub presentation: Option<AuthoredPresentationCatalogSourcesV1<'a>>,
}

/// Compiles HUD display labels for every locked block, tool, and fluid.
///
/// Supplying presentation sources applies the graph-selected provider's
/// overlay rows. Without them, names come from blocks/tools authored fallbacks
/// and icons are the derived missing-presentation identities.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when authored JSON is invalid, a locked
/// identity is missing, or a selected presentation icon is absent from the
/// presentation asset table.
pub fn compile_authored_content_display_catalog(
    sources: AuthoredContentDisplayCatalogSourcesV1<'_>,
) -> Result<ContentDisplayCatalogV1, ProductionHostError> {
    let include_presentation = sources.presentation.is_some();
    let locked = locked_content_ids(
        sources.d9_block_ids,
        sources.fluid_ids,
        sources.tool_catalog,
    )?;
    let mut rows = BTreeMap::new();
    for id in &locked {
        rows.insert(id.clone(), ContentDisplayLabelV1::missing_presentation(id));
    }
    overlay_display_file(sources.block_display, "blocks-display", &mut rows)?;
    overlay_display_file(sources.tool_display, "tools-display", &mut rows)?;
    if let Some(presentation) = sources.presentation {
        overlay_display_file(presentation.display, "presentation-display", &mut rows)?;
        require_presentation_closure(&locked, &rows, presentation.display)?;
        require_presentation_icons(&rows, presentation.assets)?;
    }
    for id in &locked {
        let Some(row) = rows.get(id) else {
            return Err(missing_display("locked-content", id));
        };
        if row.name.is_empty() {
            return Err(missing_display("display-name", id));
        }
        if row.icon.is_empty() {
            return Err(missing_display("display-icon", id));
        }
    }
    Ok(ContentDisplayCatalogV1 {
        rows,
        locked,
        includes_presentation: include_presentation,
    })
}

/// Returns presentation data selected by the optional presentation capability.
pub(super) fn presentation_data_root(
    images: &LockVerifiedComposeImages,
) -> Result<Option<Arc<RealizedDataRootV1>>, ProductionHostError> {
    let Some(package) = exactly_one_lock_provider(images, CONTENT_PRESENTATION_CAPABILITY)? else {
        return Ok(None);
    };
    Ok(Some(images.locked_artifacts().data_root(&package.name)?))
}

/// Compiles HUD display labels selected by a reopened product lock.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when a capability provider, artifact,
/// required file, UTF-8 payload, JSON document, or display closure is invalid.
pub fn lock_selected_content_display_catalog(
    images: &LockVerifiedComposeImages,
) -> Result<ContentDisplayCatalogV1, ProductionHostError> {
    let (_, blocks) = required_provider_data(images, CONTENT_BLOCKS_CAPABILITY)?;
    let (_, tools) = required_provider_data(images, SANDBOX_TOOLS_CAPABILITY)?;
    let presentation = presentation_data_root(images)?;
    let presentation = match presentation.as_deref() {
        Some(data) => Some(AuthoredPresentationCatalogSourcesV1 {
            display: required_data_text(data, AUTHORED_DISPLAY_PATH)?,
            assets: required_data_text(data, AUTHORED_PRESENTATION_ASSETS_PATH)?,
        }),
        None => None,
    };
    compile_authored_content_display_catalog(AuthoredContentDisplayCatalogSourcesV1 {
        block_display: required_data_text(&blocks, AUTHORED_DISPLAY_PATH)?,
        tool_display: required_data_text(&tools, AUTHORED_DISPLAY_PATH)?,
        tool_catalog: required_data_text(&tools, AUTHORED_TOOL_CATALOG_PATH)?,
        d9_block_ids: required_data_text(&blocks, D9_BLOCK_IDS_PATH)?,
        fluid_ids: required_data_text(&blocks, FLUID_IDS_PATH)?,
        presentation,
    })
}

fn overlay_display_file(
    source: &str,
    name: &'static str,
    rows: &mut BTreeMap<String, ContentDisplayLabelV1>,
) -> Result<(), ProductionHostError> {
    let file: AuthoredDisplayFile = serde_json::from_str(source)
        .map_err(|source| ProductionHostError::InvalidAuthoredCatalog { name, source })?;
    for entry in file.entries {
        if entry.content.is_empty() || entry.fallback.is_empty() || entry.display_key.is_empty() {
            return Err(ProductionHostError::InvalidCatalogField {
                field: "display-entry",
            });
        }
        let row = rows
            .entry(entry.content.clone())
            .or_insert_with(|| ContentDisplayLabelV1::missing_presentation(&entry.content));
        row.name = entry.fallback;
        if let Some(icon) = entry.icon.filter(|icon| !icon.is_empty()) {
            row.icon = icon;
        }
    }
    Ok(())
}

fn require_presentation_closure(
    locked: &BTreeSet<String>,
    rows: &BTreeMap<String, ContentDisplayLabelV1>,
    source: &str,
) -> Result<(), ProductionHostError> {
    let file: AuthoredDisplayFile = serde_json::from_str(source).map_err(|source| {
        ProductionHostError::InvalidAuthoredCatalog {
            name: "presentation-display",
            source,
        }
    })?;
    let present = file
        .entries
        .iter()
        .map(|entry| entry.content.clone())
        .collect::<BTreeSet<_>>();
    missing_catalog_ids(
        "presentation-display",
        locked.iter().filter(|id| !present.contains(*id)).cloned(),
    )?;
    for entry in &file.entries {
        if !locked.contains(&entry.content) {
            continue;
        }
        if entry.icon.as_ref().is_none_or(String::is_empty) {
            return Err(missing_display("presentation-icon", &entry.content));
        }
        if rows.get(&entry.content).is_none() {
            return Err(missing_display("presentation-display", &entry.content));
        }
    }
    Ok(())
}

fn require_presentation_icons(
    rows: &BTreeMap<String, ContentDisplayLabelV1>,
    source: &str,
) -> Result<(), ProductionHostError> {
    let file: AuthoredAssetsFile = serde_json::from_str(source).map_err(|source| {
        ProductionHostError::InvalidAuthoredCatalog {
            name: "presentation-assets",
            source,
        }
    })?;
    let assets = file
        .assets
        .into_iter()
        .map(|asset| asset.id)
        .collect::<BTreeSet<_>>();
    missing_catalog_ids(
        "presentation-icon-asset",
        rows.values()
            .map(|row| row.icon.clone())
            .filter(|icon| !assets.contains(icon)),
    )
}

fn locked_content_ids(
    d9_block_ids: &str,
    fluid_ids: &str,
    tool_catalog: &str,
) -> Result<BTreeSet<String>, ProductionHostError> {
    let mut ids = BTreeSet::new();
    ingest_golden_ids(d9_block_ids, &mut ids)?;
    ingest_golden_ids(fluid_ids, &mut ids)?;
    let tools: AuthoredToolCatalogFile = serde_json::from_str(tool_catalog).map_err(|source| {
        ProductionHostError::InvalidAuthoredCatalog {
            name: "tools",
            source,
        }
    })?;
    for item in tools.items {
        if item.id.is_empty() {
            return Err(ProductionHostError::InvalidCatalogField { field: "item-id" });
        }
        if !ids.insert(item.id.clone()) {
            return Err(missing_display("unique-tool", &item.id));
        }
    }
    if ids.is_empty() {
        return Err(missing_display("locked-content", "goldens"));
    }
    Ok(ids)
}

fn ingest_golden_ids(source: &str, ids: &mut BTreeSet<String>) -> Result<(), ProductionHostError> {
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !ids.insert(line.to_owned()) {
            return Err(missing_display("unique-locked-content", line));
        }
    }
    Ok(())
}

fn missing_catalog_ids(
    kind: &'static str,
    missing: impl IntoIterator<Item = String>,
) -> Result<(), ProductionHostError> {
    let missing = missing.into_iter().collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(ProductionHostError::MissingCatalogDefinition {
            kind,
            id: missing.join(","),
        })
    }
}

fn missing_display(kind: &'static str, id: &str) -> ProductionHostError {
    ProductionHostError::MissingCatalogDefinition {
        kind,
        id: id.to_owned(),
    }
}

fn content_aliases(content_id: &str) -> Vec<String> {
    let Some((namespace, rest)) = content_id.split_once(':') else {
        return Vec::new();
    };
    let Some((kind, path)) = rest.split_once('/') else {
        return Vec::new();
    };
    if kind != "item" {
        return Vec::new();
    }
    ["block", "fluid"]
        .into_iter()
        .map(|alias| format!("{namespace}:{alias}/{path}"))
        .collect()
}

fn title_case_path(content_id: &str) -> String {
    let path = content_id
        .rsplit_once('/')
        .map_or(content_id, |(_, path)| path);
    let mut display = String::new();
    for segment in path.split('-').filter(|part| !part.is_empty()) {
        if !display.is_empty() {
            display.push(' ');
        }
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            display.extend(first.to_uppercase());
            display.push_str(chars.as_str());
        }
    }
    if display.is_empty() {
        content_id.to_owned()
    } else {
        display
    }
}

fn derived_icon_id(content_id: &str) -> String {
    let Some((namespace, rest)) = content_id.split_once(':') else {
        return content_id.to_owned();
    };
    let Some((kind, path)) = rest.split_once('/') else {
        return content_id.to_owned();
    };
    format!("{namespace}:asset/icon-{kind}-{path}")
}

#[derive(Clone, Debug, Deserialize)]
struct AuthoredDisplayFile {
    entries: Vec<AuthoredDisplayEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct AuthoredDisplayEntry {
    content: String,
    display_key: String,
    fallback: String,
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct AuthoredAssetsFile {
    assets: Vec<AuthoredAssetEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct AuthoredAssetEntry {
    id: String,
}

#[derive(Clone, Debug, Deserialize)]
struct AuthoredToolCatalogFile {
    items: Vec<AuthoredToolItem>,
}

#[derive(Clone, Debug, Deserialize)]
struct AuthoredToolItem {
    id: String,
}

#[cfg(test)]
mod tests {
    use latticeaxiom_content::{ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1};
    use serde_json::Value;

    use super::*;

    const AUTHORED_BLOCKS_JSON: &str =
        include_str!("../../../../../../terrenia/blocks/data/authored-catalog-v1.json");
    const AUTHORED_BLOCK_DISPLAY_JSON: &str =
        include_str!("../../../../../../terrenia/blocks/data/authored-display-v1.json");
    const AUTHORED_TOOL_DISPLAY_JSON: &str =
        include_str!("../../../../../../terrenia/tools/data/authored-display-v1.json");
    const AUTHORED_TOOLS_JSON: &str =
        include_str!("../../../../../../terrenia/tools/data/authored-tools-v1.json");
    const D9_BLOCK_IDS: &str =
        include_str!("../../../../../../terrenia/blocks/data/goldens/d9-block-ids.txt");
    const FLUID_IDS: &str =
        include_str!("../../../../../../terrenia/blocks/data/goldens/fluid-ids.txt");
    const PRESENTATION_DISPLAY_JSON: &str =
        include_str!("../../../../../../terrenia/presentation/data/authored-display-v1.json");
    const PRESENTATION_ASSETS_JSON: &str =
        include_str!("../../../../../../terrenia/presentation/data/authored-assets-v1.json");

    fn test_display_catalog(
        include_presentation: bool,
    ) -> Result<ContentDisplayCatalogV1, ProductionHostError> {
        let presentation = include_presentation.then_some(AuthoredPresentationCatalogSourcesV1 {
            display: PRESENTATION_DISPLAY_JSON,
            assets: PRESENTATION_ASSETS_JSON,
        });
        compile_authored_content_display_catalog(AuthoredContentDisplayCatalogSourcesV1 {
            block_display: AUTHORED_BLOCK_DISPLAY_JSON,
            tool_display: AUTHORED_TOOL_DISPLAY_JSON,
            tool_catalog: AUTHORED_TOOLS_JSON,
            d9_block_ids: D9_BLOCK_IDS,
            fluid_ids: FLUID_IDS,
            presentation,
        })
    }

    #[test]
    fn omitted_presentation_covers_every_locked_identity() {
        let catalog = test_display_catalog(false).expect("omitted presentation catalog compiles");
        assert!(!catalog.includes_presentation());
        let locked = catalog.locked_ids().collect::<Vec<_>>();
        assert_eq!(locked.len(), 81);
        for id in locked {
            let label = catalog.lookup(id);
            assert!(!label.name.is_empty(), "{id}");
            assert_ne!(label.name, id, "{id}");
            assert_eq!(label.icon, derived_icon_id(id), "{id}");
        }
        assert_eq!(catalog.lookup("terrenia:block/oak-log").name, "Oak Log");
        assert_eq!(
            catalog.lookup("terrenia:item/wooden-pickaxe").name,
            "Wooden Pickaxe"
        );
        assert_eq!(catalog.lookup("terrenia:fluid/lava").name, "Lava");
        assert_eq!(
            catalog.lookup("terrenia:item/dirt").name,
            catalog.lookup("terrenia:block/dirt").name
        );
    }

    #[test]
    fn presentation_overlay_matches_omitted_fallbacks() {
        let omitted = test_display_catalog(false).expect("omitted presentation catalog compiles");
        let presented = test_display_catalog(true).expect("presentation catalog compiles");
        assert!(presented.includes_presentation());
        let omitted_ids = omitted.locked_ids().collect::<BTreeSet<_>>();
        let presented_ids = presented.locked_ids().collect::<BTreeSet<_>>();
        assert_eq!(omitted_ids, presented_ids);
        for id in omitted_ids {
            assert_eq!(omitted.lookup(id), presented.lookup(id), "{id}");
        }
    }

    #[test]
    fn omitting_presentation_does_not_change_authoritative_content_hash() {
        let authored: Value = serde_json::from_str(AUTHORED_BLOCKS_JSON)
            .expect("Terrenia authored catalog is valid JSON");
        let input = ContentCatalogInputV1 {
            schema_major: 1,
            blocks: authored["blocks"]
                .as_array()
                .expect("blocks is an array")
                .iter()
                .map(|row| {
                    serde_json::from_value(row["definition"].clone())
                        .expect("block definition decodes")
                })
                .collect(),
            fluids: authored["fluids"]
                .as_array()
                .expect("fluids is an array")
                .iter()
                .map(|row| {
                    serde_json::from_value(row["definition"].clone())
                        .expect("fluid definition decodes")
                })
                .collect(),
            biomes: Vec::new(),
            material_role_bindings: serde_json::from_value(
                authored["material_role_bindings"].clone(),
            )
            .expect("material Role bindings decode"),
        };
        let with_presentation =
            ContentCatalogV1::compile(input.clone(), ContentCatalogLimitsV1::default())
                .expect("catalog with presentation bindings compiles");
        let mut omitted = input;
        for block in &mut omitted.blocks {
            block.presentation_binding = None;
        }
        for fluid in &mut omitted.fluids {
            fluid.presentation_binding = None;
        }
        let omitted = ContentCatalogV1::compile(omitted, ContentCatalogLimitsV1::default())
            .expect("catalog without presentation bindings compiles");
        assert_eq!(
            with_presentation
                .canonical_authoritative_hash()
                .expect("presented hash"),
            omitted
                .canonical_authoritative_hash()
                .expect("omitted hash")
        );
    }
}
