//! Client HUD display names and icons for locked content identities.
//!
//! Presentation package rows overlay locale-independent fallbacks from the
//! blocks and tools packages. Omitting `@terrenia/presentation` still yields a
//! deterministic name and icon for every locked block, tool, and fluid.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::ProductionHostError;
use crate::LockVerifiedComposeImages;

const AUTHORED_BLOCK_DISPLAY_JSON: &str =
    include_str!("../../../../packages/terrenia/blocks/data/authored-display-v1.json");
const AUTHORED_TOOL_DISPLAY_JSON: &str =
    include_str!("../../../../packages/terrenia/tools/data/authored-display-v1.json");
const AUTHORED_TOOL_CATALOG_JSON: &str =
    include_str!("../../../../packages/terrenia/tools/data/authored-tools-v1.json");
const AUTHORED_PRESENTATION_DISPLAY_JSON: &str =
    include_str!("../../../../packages/terrenia/presentation/data/authored-display-v1.json");
const AUTHORED_PRESENTATION_ASSETS_JSON: &str =
    include_str!("../../../../packages/terrenia/presentation/data/authored-assets-v1.json");
const D9_BLOCK_IDS: &str =
    include_str!("../../../../packages/terrenia/blocks/data/goldens/d9-block-ids.txt");
const FLUID_IDS: &str =
    include_str!("../../../../packages/terrenia/blocks/data/goldens/fluid-ids.txt");

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

/// Compiles HUD display labels for every locked block, tool, and fluid.
///
/// `include_presentation` applies `@terrenia/presentation` overlay rows. When
/// false, names come from blocks/tools authored fallbacks and icons are the
/// derived missing-presentation identities.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when authored JSON is invalid, a locked
/// identity is missing, or a selected presentation icon is absent from the
/// presentation asset table.
pub fn authored_content_display_catalog(
    include_presentation: bool,
) -> Result<ContentDisplayCatalogV1, ProductionHostError> {
    let locked = locked_content_ids()?;
    let mut rows = BTreeMap::new();
    for id in &locked {
        rows.insert(id.clone(), ContentDisplayLabelV1::missing_presentation(id));
    }
    overlay_display_file(AUTHORED_BLOCK_DISPLAY_JSON, "blocks-display", &mut rows)?;
    overlay_display_file(AUTHORED_TOOL_DISPLAY_JSON, "tools-display", &mut rows)?;
    if include_presentation {
        overlay_display_file(
            AUTHORED_PRESENTATION_DISPLAY_JSON,
            "presentation-display",
            &mut rows,
        )?;
        require_presentation_closure(&locked, &rows)?;
        require_presentation_icons(&rows)?;
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

/// Returns whether the frozen lock graph selected `@terrenia/presentation`.
#[must_use]
pub(super) fn presentation_package_selected(images: &LockVerifiedComposeImages) -> bool {
    images
        .images()
        .graph()
        .packages
        .keys()
        .any(|package| package.as_str() == "@terrenia/presentation")
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
) -> Result<(), ProductionHostError> {
    let file: AuthoredDisplayFile = serde_json::from_str(AUTHORED_PRESENTATION_DISPLAY_JSON)
        .map_err(|source| ProductionHostError::InvalidAuthoredCatalog {
            name: "presentation-display",
            source,
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
) -> Result<(), ProductionHostError> {
    let file: AuthoredAssetsFile = serde_json::from_str(AUTHORED_PRESENTATION_ASSETS_JSON)
        .map_err(|source| ProductionHostError::InvalidAuthoredCatalog {
            name: "presentation-assets",
            source,
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

fn locked_content_ids() -> Result<BTreeSet<String>, ProductionHostError> {
    let mut ids = BTreeSet::new();
    ingest_golden_ids(D9_BLOCK_IDS, &mut ids)?;
    ingest_golden_ids(FLUID_IDS, &mut ids)?;
    let tools: AuthoredToolCatalogFile =
        serde_json::from_str(AUTHORED_TOOL_CATALOG_JSON).map_err(|source| {
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
        include_str!("../../../../packages/terrenia/blocks/data/authored-catalog-v1.json");

    #[test]
    fn omitted_presentation_covers_every_locked_identity() {
        let catalog =
            authored_content_display_catalog(false).expect("omitted presentation catalog compiles");
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
        let omitted =
            authored_content_display_catalog(false).expect("omitted presentation catalog compiles");
        let presented =
            authored_content_display_catalog(true).expect("presentation catalog compiles");
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
