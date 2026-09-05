//! Official settings page: categories, sections, detail, and transaction bar.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_client_ui::{
    ButtonWidget, SemanticAction, SemanticKey, SemanticNode, SemanticRole, SemanticState,
    application_root,
};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_runtime_contracts::{
    RuntimeApplyImpact, ScopeOverlay, SettingScope, SettingSpec, SettingTransactionRevision,
    SettingWriter, SettingsDurabilityDomain, StoreRevision, ValidatedSettingsCatalog,
    resolve_effective_settings,
};
use serde_json::Value;

use crate::baseline::{
    SettingsPageCatalogKind, compile_baseline_page_catalog, section_for_setting,
};
use crate::category::SettingsCategoryV1;
use crate::error::SettingsPageError;
use crate::host::{SettingsPageHost, SettingsValueAdmission};
use crate::layout::SettingsSectionV1;
use crate::surface::{
    SettingsReadOnlyReason, SettingsSurfaceApplyRequest, SettingsSurfaceAuthority,
    SettingsSurfaceCommand, SettingsSurfaceModel, SettingsSurfaceOutcome, SettingsSurfaceRow,
    SettingsSurfaceScope, SettingsSurfaceState,
};

/// Frozen rebindable action projected into Controls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsBindingRow {
    /// Action stable identity.
    pub action: StableId,
    /// Input context label.
    pub context: String,
    /// Owning package display name.
    pub owner: String,
    /// Default binding label.
    pub default_label: String,
    /// Effective binding label.
    pub effective_label: String,
}

/// Surface operation that is not a persisted setting value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsPageOperation {
    /// Filter catalog rows by owning package.
    FilterByOwner,
    /// Reset one package's non-authoritative values.
    ResetPackage,
    /// Export non-sensitive user and device values.
    Export,
    /// Import a canonical overlay after a diff preview.
    Import,
    /// Inspect retained orphan values.
    InspectOrphans,
    /// Open the profile draft for graph-affecting composition.
    OpenProfileDraft,
}

impl SettingsPageOperation {
    /// Frozen Packages-category operations.
    pub const ALL: [Self; 6] = [
        Self::FilterByOwner,
        Self::ResetPackage,
        Self::Export,
        Self::Import,
        Self::InspectOrphans,
        Self::OpenProfileDraft,
    ];

    /// Returns the semantic key suffix.
    #[must_use]
    pub const fn key_suffix(self) -> &'static str {
        match self {
            Self::FilterByOwner => "filter-owner",
            Self::ResetPackage => "reset-package",
            Self::Export => "export",
            Self::Import => "import",
            Self::InspectOrphans => "orphans",
            Self::OpenProfileDraft => "profile-draft",
        }
    }

    /// Returns the row label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FilterByOwner => "Filter by owner package",
            Self::ResetPackage => "Reset package values",
            Self::Export => "Export non-sensitive values",
            Self::Import => "Import values",
            Self::InspectOrphans => "Inspect orphan values",
            Self::OpenProfileDraft => "Open profile draft",
        }
    }
}

/// How the page is opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsPageOpen {
    /// Shell versus in-game filtering.
    pub scope: SettingsSurfaceScope,
    /// Writer authority for world rows.
    pub authority: SettingsSurfaceAuthority,
    /// Compact layout used at 800×600 or UI scale 2.0.
    pub compact: bool,
    /// Whether developer rows are compiled.
    pub catalog: SettingsPageCatalogKind,
}

impl SettingsPageOpen {
    /// Shell settings with no world writer.
    #[must_use]
    pub const fn shell() -> Self {
        Self {
            scope: SettingsSurfaceScope::Shell,
            authority: SettingsSurfaceAuthority::shell(),
            compact: false,
            catalog: SettingsPageCatalogKind::Playable,
        }
    }

    /// Pause-menu settings. Packages composition is hidden.
    #[must_use]
    pub fn in_game(has_world_writer: bool) -> Self {
        Self {
            scope: SettingsSurfaceScope::InGame,
            authority: SettingsSurfaceAuthority {
                has_world_writer,
                writer: if has_world_writer {
                    Some(SettingWriter::WorldOwner)
                } else {
                    None
                },
            },
            compact: false,
            catalog: SettingsPageCatalogKind::Playable,
        }
    }
}

/// Typed user intent for the complete settings page.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsPageCommand {
    /// Open or resume an editing session from applied values.
    BeginEdit,
    /// Select a first-level category.
    SelectCategory(SettingsCategoryV1),
    /// Select a section inside the current category.
    SelectSection(SettingsSectionV1),
    /// Select one setting row and open its detail.
    SelectSetting(StableId),
    /// Select a binding row.
    SelectBinding(StableId),
    /// Update the cross-category search query.
    SetSearch(String),
    /// Replace one editable draft value.
    SetValue {
        /// Target setting.
        setting: StableId,
        /// Proposed JSON value.
        value: Value,
    },
    /// Reset one row to its catalog default.
    ResetRow(StableId),
    /// Reset every editable row in the current section.
    ResetSection,
    /// Reset every editable row in the current category.
    ResetCategory,
    /// Apply the single-domain dirty draft through the host.
    Apply,
    /// Restore applied values without closing.
    Undo,
    /// Restore applied values and close.
    Cancel,
}

/// Observable result of one page command.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsPageOutcome {
    /// Navigation or search changed without a draft mutation.
    Navigated,
    /// A surface draft command completed.
    Surface(SettingsSurfaceOutcome),
    /// Reset restored catalog defaults onto the draft.
    Reset {
        /// Settings restored to defaults.
        settings: Vec<StableId>,
    },
}

/// Selected row detail shown beside or below the list.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsRowDetail {
    /// Stable setting identity.
    pub id: StableId,
    /// Label localization key.
    pub label_key: String,
    /// Description localization key.
    pub description_key: String,
    /// Owning package.
    pub owner: String,
    /// Catalog default.
    pub default: Value,
    /// Last applied value.
    pub applied: Value,
    /// Current draft.
    pub draft: Value,
    /// Host admission of the draft.
    pub admission: SettingsValueAdmission,
    /// Persistence scope.
    pub scope: SettingsDurabilityDomain,
    /// Apply impact.
    pub impact: RuntimeApplyImpact,
    /// Whether the row is editable.
    pub editable: bool,
    /// Read-only reason when not editable.
    pub read_only_reason: Option<SettingsReadOnlyReason>,
}

/// Official settings page session.
#[derive(Clone, Debug)]
pub struct SettingsPageSession {
    catalog: ValidatedSettingsCatalog,
    defaults: BTreeMap<StableId, Value>,
    specs: BTreeMap<StableId, SettingSpec>,
    surface: SettingsSurfaceModel,
    selected_category: SettingsCategoryV1,
    selected_section: SettingsSectionV1,
    selected_setting: Option<StableId>,
    selected_binding: Option<StableId>,
    compact: bool,
    scope: SettingsSurfaceScope,
    bindings: Vec<SettingsBindingRow>,
}

impl SettingsPageSession {
    /// Opens the official baseline settings page.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsPageError`] when the catalog cannot compile or the
    /// presentation model cannot be built.
    pub fn open(
        open: SettingsPageOpen,
        host: &impl SettingsPageHost,
    ) -> Result<Self, SettingsPageError> {
        let catalog = compile_baseline_page_catalog(open.catalog)?;
        Self::open_catalog(catalog, open, host)
    }

    /// Opens a page over an already compiled catalog.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsPageError`] when overlays or projection fail.
    pub fn open_catalog(
        catalog: ValidatedSettingsCatalog,
        open: SettingsPageOpen,
        host: &impl SettingsPageHost,
    ) -> Result<Self, SettingsPageError> {
        let stored = host.load()?;
        let overlays = overlays_from_store(&catalog, stored);
        let resolved = resolve_effective_settings(
            &catalog,
            overlays,
            CanonicalHash::digest(b"settings-page"),
        )?;
        let mut surface = SettingsSurfaceModel::from_snapshot(
            &catalog.as_catalog().runtime,
            resolved.snapshot(),
            open.authority,
        )?;
        surface.apply_scope_filter(open.scope);
        surface.handle(SettingsSurfaceCommand::BeginEdit)?;
        let defaults = catalog
            .as_catalog()
            .runtime
            .iter()
            .map(|(id, spec)| (id.clone(), spec.default.clone()))
            .collect();
        let specs = catalog.as_catalog().runtime.clone();
        let mut page = Self {
            catalog,
            defaults,
            specs,
            surface,
            selected_category: SettingsCategoryV1::Accessibility,
            selected_section: SettingsSectionV1::first_in(SettingsCategoryV1::Accessibility),
            selected_setting: None,
            selected_binding: None,
            compact: open.compact,
            scope: open.scope,
            bindings: default_binding_rows(),
        };
        page.ensure_selection();
        Ok(page)
    }

    /// Returns the compiled catalog.
    #[must_use]
    pub const fn catalog(&self) -> &ValidatedSettingsCatalog {
        &self.catalog
    }

    /// Returns the draft surface.
    #[must_use]
    pub const fn surface(&self) -> &SettingsSurfaceModel {
        &self.surface
    }

    /// Returns the selected category.
    #[must_use]
    pub const fn selected_category(&self) -> SettingsCategoryV1 {
        self.selected_category
    }

    /// Returns the selected section.
    #[must_use]
    pub const fn selected_section(&self) -> SettingsSectionV1 {
        self.selected_section
    }

    /// Returns whether a compact layout is active.
    #[must_use]
    pub const fn compact(&self) -> bool {
        self.compact
    }

    /// Updates compact layout used at 800×600 or UI scale 2.0.
    pub const fn set_compact(&mut self, compact: bool) {
        self.compact = compact;
    }

    /// Returns the compiled spec for one setting on this page.
    #[must_use]
    pub fn spec(&self, setting: &StableId) -> Option<&SettingSpec> {
        self.specs.get(setting)
    }

    /// Returns whether Advanced sections are visible.
    #[must_use]
    pub fn show_advanced(&self) -> bool {
        self.surface
            .draft_value(&advanced_setting_id())
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Returns categories available in the current page.
    #[must_use]
    pub fn visible_categories(&self) -> Vec<SettingsCategoryV1> {
        SettingsCategoryV1::ALL
            .into_iter()
            .filter(|category| {
                if *category == SettingsCategoryV1::Packages {
                    return self.scope == SettingsSurfaceScope::Shell;
                }
                !self.rows_in_category(*category).is_empty()
                    || (*category == SettingsCategoryV1::Controls && !self.bindings.is_empty())
            })
            .collect()
    }

    /// Returns sections available in the selected category.
    #[must_use]
    pub fn visible_sections(&self) -> Vec<SettingsSectionV1> {
        SettingsSectionV1::for_category(self.selected_category)
            .into_iter()
            .filter(|section| self.section_visible(*section))
            .collect()
    }

    /// Returns setting rows in the selected section after search filtering.
    #[must_use]
    pub fn visible_rows(&self) -> Vec<&SettingsSurfaceRow> {
        self.surface
            .visible_rows()
            .into_iter()
            .filter(|row| {
                section_for_setting(&row.id) == self.selected_section && self.row_visible(row)
            })
            .collect()
    }

    /// Returns binding rows when the Keyboard section is selected.
    #[must_use]
    pub fn visible_bindings(&self) -> &[SettingsBindingRow] {
        if self.selected_section == SettingsSectionV1::ControlsKeyboard {
            &self.bindings
        } else {
            &[]
        }
    }

    /// Returns Packages operations when that section is selected.
    #[must_use]
    pub fn visible_operations(&self) -> &[SettingsPageOperation] {
        if self.selected_section == SettingsSectionV1::PackagesCatalog {
            &SettingsPageOperation::ALL
        } else {
            &[]
        }
    }

    /// Returns detail for the selected setting.
    #[must_use]
    pub fn selected_detail(&self, host: &impl SettingsPageHost) -> Option<SettingsRowDetail> {
        let id = self.selected_setting.as_ref()?;
        let row = self.surface.rows().iter().find(|row| row.id == *id)?;
        let default = self.defaults.get(id).cloned()?;
        let applied = self.surface.applied_value(id).cloned()?;
        let admission = host.admission(id, &row.value);
        Some(SettingsRowDetail {
            id: id.clone(),
            label_key: row.label_key.clone(),
            description_key: row.description_key.clone(),
            owner: row.owner.clone(),
            default,
            applied,
            draft: row.value.clone(),
            admission,
            scope: row.domain,
            impact: row.impact,
            editable: row.editable,
            read_only_reason: row.read_only_reason,
        })
    }

    /// Applies one page command.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsPageError`] when navigation is illegal or the draft
    /// rejects the mutation.
    pub fn handle(
        &mut self,
        command: SettingsPageCommand,
        host: &mut impl SettingsPageHost,
    ) -> Result<SettingsPageOutcome, SettingsPageError> {
        match command {
            SettingsPageCommand::BeginEdit => Ok(SettingsPageOutcome::Surface(
                self.surface.handle(SettingsSurfaceCommand::BeginEdit)?,
            )),
            SettingsPageCommand::SelectCategory(category) => {
                if !self.visible_categories().contains(&category) {
                    return Err(SettingsPageError::UnknownTarget {
                        target: category.stable_id().to_owned(),
                    });
                }
                self.selected_category = category;
                self.selected_section = self
                    .visible_sections()
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| SettingsSectionV1::first_in(category));
                self.ensure_selection();
                Ok(SettingsPageOutcome::Navigated)
            }
            SettingsPageCommand::SelectSection(section) => {
                if section.category() != self.selected_category || !self.section_visible(section) {
                    return Err(SettingsPageError::UnknownTarget {
                        target: section.stable_id().to_owned(),
                    });
                }
                self.selected_section = section;
                self.ensure_selection();
                Ok(SettingsPageOutcome::Navigated)
            }
            SettingsPageCommand::SelectSetting(setting) => {
                if !self
                    .surface
                    .rows()
                    .iter()
                    .any(|row| row.id == setting && self.row_visible(row))
                {
                    return Err(SettingsPageError::UnknownSetting { setting });
                }
                self.selected_setting = Some(setting);
                self.selected_binding = None;
                Ok(SettingsPageOutcome::Navigated)
            }
            SettingsPageCommand::SelectBinding(action) => {
                if !self.bindings.iter().any(|row| row.action == action) {
                    return Err(SettingsPageError::UnknownSetting { setting: action });
                }
                self.selected_binding = Some(action);
                self.selected_setting = None;
                Ok(SettingsPageOutcome::Navigated)
            }
            SettingsPageCommand::SetSearch(query) => {
                self.surface.set_search(query);
                self.ensure_selection();
                Ok(SettingsPageOutcome::Navigated)
            }
            SettingsPageCommand::SetValue { setting, value } => {
                let outcome = self
                    .surface
                    .handle(SettingsSurfaceCommand::SetValue { setting, value })?;
                self.ensure_selection();
                Ok(SettingsPageOutcome::Surface(outcome))
            }
            SettingsPageCommand::ResetRow(setting) => {
                let default = self.defaults.get(&setting).cloned().ok_or_else(|| {
                    SettingsPageError::UnknownSetting {
                        setting: setting.clone(),
                    }
                })?;
                let outcome = self.surface.handle(SettingsSurfaceCommand::SetValue {
                    setting: setting.clone(),
                    value: default,
                })?;
                Ok(SettingsPageOutcome::Surface(outcome))
            }
            SettingsPageCommand::ResetSection => {
                let section = self.selected_section;
                self.reset_matching(|row| section_for_setting(&row.id) == section && row.editable)
            }
            SettingsPageCommand::ResetCategory => {
                let category = self.selected_category;
                self.reset_matching(|row| row.category == category && row.editable)
            }
            SettingsPageCommand::Apply => {
                let outcome = self.surface.handle(SettingsSurfaceCommand::Apply)?;
                if let SettingsSurfaceOutcome::ApplyRequested(request) = &outcome {
                    let resolution = host.apply(request)?;
                    let completed = self.surface.finish_apply(resolution)?;
                    return Ok(SettingsPageOutcome::Surface(completed));
                }
                Ok(SettingsPageOutcome::Surface(outcome))
            }
            SettingsPageCommand::Undo => Ok(SettingsPageOutcome::Surface(
                self.surface.handle(SettingsSurfaceCommand::Undo)?,
            )),
            SettingsPageCommand::Cancel => Ok(SettingsPageOutcome::Surface(
                self.surface.handle(SettingsSurfaceCommand::Cancel)?,
            )),
        }
    }

    /// Returns the pending apply request when the surface is waiting.
    #[must_use]
    pub fn pending_apply(&self) -> Option<&SettingsSurfaceApplyRequest> {
        match self.surface.state() {
            SettingsSurfaceState::ApplyPending(request) => Some(request),
            _ => None,
        }
    }

    /// Projects the unique settings page into the shared semantic tree.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsPageError`] when a semantic key is invalid.
    pub fn semantic_fragment(
        &self,
        host: &impl SettingsPageHost,
    ) -> Result<SemanticNode, SettingsPageError> {
        let interaction_enabled = self.surface.is_editing();
        let mut children = vec![
            ButtonWidget {
                key: semantic_key("settings/back")?,
                name: "Cancel".to_owned(),
                description: Some("Discard the draft and return".to_owned()),
                enabled: interaction_enabled,
            }
            .semantic_node(false),
            SemanticNode {
                key: semantic_key("settings/search")?,
                role: SemanticRole::TextInput,
                name: "Search settings".to_owned(),
                value: Some(search_value(self)),
                description: Some(
                    "Search category, section, label, owner, and stable ID".to_owned(),
                ),
                state: SemanticState {
                    focusable: true,
                    focused: false,
                    disabled: false,
                    expanded: None,
                    busy: false,
                },
                actions: BTreeSet::from([SemanticAction::Activate]),
                children: Vec::new(),
            },
            self.category_nav(interaction_enabled)?,
            self.section_nav(interaction_enabled)?,
            self.content_pane(host, interaction_enabled)?,
        ];
        if !self.compact {
            children.push(self.detail_pane(host)?);
        }
        children.push(self.transaction_bar(interaction_enabled)?);
        Ok(SemanticNode {
            key: semantic_key("modal/settings")?,
            role: SemanticRole::Group,
            name: "Settings".to_owned(),
            value: self
                .surface
                .restart_impact()
                .strongest_impact
                .map(|impact| format!("{impact:?}").to_lowercase()),
            description: Some(
                "Category, section, detail, and transaction bar. Apply never writes files."
                    .to_owned(),
            ),
            state: SemanticState {
                focusable: false,
                focused: false,
                disabled: false,
                expanded: Some(true),
                busy: self.surface.apply_pending(),
            },
            actions: BTreeSet::from([SemanticAction::Cancel, SemanticAction::Back]),
            children,
        })
    }

    /// Projects the page as the unique application root.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsPageError`] when projection would create a second root.
    pub fn semantic_tree(
        &self,
        host: &impl SettingsPageHost,
    ) -> Result<SemanticNode, SettingsPageError> {
        application_root(
            semantic_key("settings")?,
            "Settings",
            vec![self.semantic_fragment(host)?],
        )
        .map_err(|_| SettingsPageError::SecondUiRoot)
    }

    fn category_nav(&self, interaction_enabled: bool) -> Result<SemanticNode, SettingsPageError> {
        let mut children = Vec::new();
        for category in self.visible_categories() {
            children.push(
                ButtonWidget {
                    key: semantic_key(&format!("settings/nav/{}", category_slug(category)))?,
                    name: category.label_key().to_owned(),
                    description: Some(category.stable_id().to_owned()),
                    enabled: interaction_enabled,
                }
                .semantic_node(false),
            );
        }
        Ok(SemanticNode {
            key: semantic_key("settings/nav")?,
            role: SemanticRole::Navigation,
            name: "Settings categories".to_owned(),
            value: Some(self.selected_category.label_key().to_owned()),
            description: Some("Fixed category order".to_owned()),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        })
    }

    fn section_nav(&self, interaction_enabled: bool) -> Result<SemanticNode, SettingsPageError> {
        let mut children = Vec::new();
        for section in self.visible_sections() {
            children.push(
                ButtonWidget {
                    key: semantic_key(&format!("settings/section/{}", section_slug(section)))?,
                    name: section.label_key().to_owned(),
                    description: Some(section.stable_id().to_owned()),
                    enabled: interaction_enabled,
                }
                .semantic_node(false),
            );
        }
        Ok(SemanticNode {
            key: semantic_key("settings/sections")?,
            role: SemanticRole::Navigation,
            name: "Settings sections".to_owned(),
            value: Some(self.selected_section.label_key().to_owned()),
            description: Some("Category sections and subpages".to_owned()),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        })
    }

    fn content_pane(
        &self,
        host: &impl SettingsPageHost,
        interaction_enabled: bool,
    ) -> Result<SemanticNode, SettingsPageError> {
        let mut children = Vec::new();
        for row in self.visible_rows() {
            children.push(setting_row_node(
                row,
                interaction_enabled,
                self.selected_setting.as_ref() == Some(&row.id),
            )?);
        }
        for binding in self.visible_bindings() {
            children.push(SemanticNode {
                key: semantic_key(&format!(
                    "settings/controls/{}",
                    sanitize_id(binding.action.as_str())
                ))?,
                role: SemanticRole::Button,
                name: binding.action.as_str().to_owned(),
                value: Some(binding.effective_label.clone()),
                description: Some(format!(
                    "Owner {}; context {}; default {}",
                    binding.owner, binding.context, binding.default_label
                )),
                state: SemanticState {
                    focusable: interaction_enabled,
                    focused: self.selected_binding.as_ref() == Some(&binding.action),
                    disabled: !interaction_enabled,
                    expanded: None,
                    busy: false,
                },
                actions: BTreeSet::from([SemanticAction::Activate]),
                children: Vec::new(),
            });
        }
        for operation in self.visible_operations() {
            children.push(
                ButtonWidget {
                    key: semantic_key(&format!("settings/operation/{}", operation.key_suffix()))?,
                    name: operation.label().to_owned(),
                    description: Some(
                        "Surface operation. Not a persisted setting value.".to_owned(),
                    ),
                    enabled: interaction_enabled,
                }
                .semantic_node(false),
            );
        }
        if self.compact {
            children.push(self.detail_pane(host)?);
        }
        Ok(SemanticNode {
            key: semantic_key("settings/content")?,
            role: SemanticRole::Group,
            name: self.selected_section.label_key().to_owned(),
            value: None,
            description: Some(format!(
                "Category {}; section {}",
                self.selected_category.label_key(),
                self.selected_section.label_key()
            )),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        })
    }

    fn detail_pane(&self, host: &impl SettingsPageHost) -> Result<SemanticNode, SettingsPageError> {
        let detail = self.selected_detail(host);
        let (name, value, description, children) = match detail {
            Some(detail) => {
                let clamp = detail
                    .admission
                    .clamp_reason
                    .clone()
                    .unwrap_or_else(|| "no clamp".to_owned());
                (
                    detail.label_key.clone(),
                    Some(detail.draft.to_string()),
                    Some(format!(
                        "Owner {}; default {}; applied {}; requested {}; admitted {}; effective {}; {}; impact {:?}; scope {:?}; id {}",
                        detail.owner,
                        detail.default,
                        detail.applied,
                        detail.admission.requested,
                        detail.admission.admitted,
                        detail.admission.effective,
                        clamp,
                        detail.impact,
                        detail.scope,
                        detail.id
                    )),
                    vec![
                        ButtonWidget {
                            key: semantic_key("settings/detail/reset")?,
                            name: "Reset row".to_owned(),
                            description: Some("Restore the catalog default".to_owned()),
                            enabled: detail.editable && self.surface.is_editing(),
                        }
                        .semantic_node(false),
                    ],
                )
            }
            None => (
                "No selection".to_owned(),
                None,
                Some("Select a setting to inspect owner, impact, and values".to_owned()),
                Vec::new(),
            ),
        };
        Ok(SemanticNode {
            key: semantic_key("settings/detail")?,
            role: SemanticRole::Group,
            name,
            value,
            description,
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        })
    }

    fn transaction_bar(
        &self,
        interaction_enabled: bool,
    ) -> Result<SemanticNode, SettingsPageError> {
        let dirty = self.surface.is_dirty();
        let mixed = self.surface.mixed_durability_domains();
        let impact = self.surface.restart_impact();
        let apply_description = if mixed {
            "Dirty draft mixes durability domains and must be split"
        } else if impact.process_restart {
            "Process restart required after persistence. The UI never writes the file."
        } else if impact.world_reactivate {
            "World reactivation required after persistence. The UI never writes the file."
        } else {
            "Validate the complete draft and show impact before atomic persistence"
        };
        Ok(SemanticNode {
            key: semantic_key("settings/transaction")?,
            role: SemanticRole::Group,
            name: "Settings transaction".to_owned(),
            value: Some(if dirty {
                "dirty".to_owned()
            } else {
                "clean".to_owned()
            }),
            description: Some(apply_description.to_owned()),
            state: SemanticState {
                focusable: false,
                focused: false,
                disabled: false,
                expanded: Some(dirty),
                busy: self.surface.apply_pending(),
            },
            actions: BTreeSet::new(),
            children: vec![
                ButtonWidget {
                    key: semantic_key("settings/undo")?,
                    name: "Undo changes".to_owned(),
                    description: Some("Restore the last applied values".to_owned()),
                    enabled: interaction_enabled && dirty,
                }
                .semantic_node(false),
                ButtonWidget {
                    key: semantic_key("settings/apply")?,
                    name: "Apply settings".to_owned(),
                    description: Some(apply_description.to_owned()),
                    enabled: dirty && interaction_enabled && !mixed,
                }
                .semantic_node(false),
                ButtonWidget {
                    key: semantic_key("settings/reset-section")?,
                    name: "Reset section".to_owned(),
                    description: Some("Restore catalog defaults for this section".to_owned()),
                    enabled: interaction_enabled,
                }
                .semantic_node(false),
            ],
        })
    }

    fn reset_matching(
        &mut self,
        predicate: impl Fn(&SettingsSurfaceRow) -> bool,
    ) -> Result<SettingsPageOutcome, SettingsPageError> {
        let settings = self
            .surface
            .rows()
            .iter()
            .filter(|row| predicate(row))
            .map(|row| row.id.clone())
            .collect::<Vec<_>>();
        for setting in &settings {
            let Some(default) = self.defaults.get(setting).cloned() else {
                continue;
            };
            match self.surface.handle(SettingsSurfaceCommand::SetValue {
                setting: setting.clone(),
                value: default,
            }) {
                Ok(_) | Err(crate::surface::SettingsSurfaceError::ReadOnlySetting { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(SettingsPageOutcome::Reset { settings })
    }

    fn rows_in_category(&self, category: SettingsCategoryV1) -> Vec<&SettingsSurfaceRow> {
        self.surface
            .rows()
            .iter()
            .filter(|row| row.category == category && self.row_visible(row))
            .collect()
    }

    fn section_visible(&self, section: SettingsSectionV1) -> bool {
        if section.advanced() && !self.show_advanced() {
            return false;
        }
        match section {
            SettingsSectionV1::ControlsKeyboard => !self.bindings.is_empty(),
            SettingsSectionV1::ControlsPackageActions | SettingsSectionV1::PackagesCatalog => true,
            _ => self
                .surface
                .rows()
                .iter()
                .any(|row| section_for_setting(&row.id) == section && self.row_visible(row)),
        }
    }

    fn row_visible(&self, row: &SettingsSurfaceRow) -> bool {
        if section_for_setting(&row.id).advanced() && !self.show_advanced() {
            return false;
        }
        let Some(spec) = self.specs.get(&row.id) else {
            return false;
        };
        if let Some(latticeaxiom_runtime_contracts::SettingPredicate::Equals { setting, value }) =
            &spec.visibility
        {
            self.surface.draft_value(setting) == Some(value)
        } else {
            true
        }
    }

    fn ensure_selection(&mut self) {
        if !self.visible_categories().contains(&self.selected_category) {
            self.selected_category = self
                .visible_categories()
                .into_iter()
                .next()
                .unwrap_or(SettingsCategoryV1::Accessibility);
        }
        if !self.visible_sections().contains(&self.selected_section) {
            self.selected_section = self
                .visible_sections()
                .into_iter()
                .next()
                .unwrap_or_else(|| SettingsSectionV1::first_in(self.selected_category));
        }
        if let Some(id) = &self.selected_setting
            && !self.visible_rows().iter().any(|row| row.id == *id)
        {
            self.selected_setting = None;
        }
        if self.selected_setting.is_none() {
            self.selected_setting = self.visible_rows().first().map(|row| row.id.clone());
        }
    }
}

fn overlays_from_store(
    catalog: &ValidatedSettingsCatalog,
    stored: BTreeMap<StableId, Value>,
) -> Vec<ScopeOverlay> {
    let mut by_scope: BTreeMap<SettingScope, BTreeMap<StableId, Value>> = BTreeMap::new();
    for (id, value) in stored {
        let Some(spec) = catalog.as_catalog().runtime.get(&id) else {
            continue;
        };
        by_scope
            .entry(spec.default_scope)
            .or_default()
            .insert(id, value);
    }
    by_scope
        .into_iter()
        .map(|(scope, values)| {
            let writer = match scope {
                SettingScope::World | SettingScope::PlayerWorld => SettingWriter::WorldOwner,
                SettingScope::Device | SettingScope::User | SettingScope::Session => {
                    SettingWriter::LocalUser
                }
            };
            ScopeOverlay::new(
                scope,
                StoreRevision::new(1),
                SettingTransactionRevision::new(1),
                writer,
                None,
                values,
            )
        })
        .collect()
}

#[allow(
    clippy::too_many_lines,
    reason = "frozen first-playable action catalog is a closed table"
)]
fn default_binding_rows() -> Vec<SettingsBindingRow> {
    const ACTIONS: [(&str, &str, &str, &str); 31] = [
        (
            "latticeaxiom:action/gameplay/move@1",
            "gameplay",
            "WASD",
            "WASD",
        ),
        (
            "latticeaxiom:action/gameplay/look@1",
            "gameplay",
            "Mouse",
            "Mouse",
        ),
        (
            "latticeaxiom:action/gameplay/jump@1",
            "gameplay",
            "Space",
            "Space",
        ),
        (
            "latticeaxiom:action/gameplay/break-block@1",
            "gameplay",
            "MouseLeft",
            "MouseLeft",
        ),
        (
            "latticeaxiom:action/gameplay/place-block@1",
            "gameplay",
            "MouseRight",
            "MouseRight",
        ),
        (
            "latticeaxiom:action/gameplay/inspect@1",
            "gameplay",
            "F",
            "F",
        ),
        (
            "latticeaxiom:action/gameplay/pause@1",
            "gameplay",
            "Escape",
            "Escape",
        ),
        (
            "latticeaxiom:action/gameplay/pick-block@1",
            "gameplay",
            "MouseMiddle",
            "MouseMiddle",
        ),
        (
            "latticeaxiom:action/gameplay/sprint@1",
            "gameplay",
            "Ctrl",
            "Ctrl",
        ),
        (
            "latticeaxiom:action/gameplay/sneak@1",
            "gameplay",
            "Shift",
            "Shift",
        ),
        (
            "latticeaxiom:action/ui/nav-up@1",
            "surface",
            "ArrowUp",
            "ArrowUp",
        ),
        (
            "latticeaxiom:action/ui/nav-down@1",
            "surface",
            "ArrowDown",
            "ArrowDown",
        ),
        (
            "latticeaxiom:action/ui/nav-left@1",
            "surface",
            "ArrowLeft",
            "ArrowLeft",
        ),
        (
            "latticeaxiom:action/ui/nav-right@1",
            "surface",
            "ArrowRight",
            "ArrowRight",
        ),
        ("latticeaxiom:action/ui/nav-next@1", "surface", "Tab", "Tab"),
        (
            "latticeaxiom:action/ui/nav-previous@1",
            "surface",
            "Shift+Tab",
            "Shift+Tab",
        ),
        (
            "latticeaxiom:action/ui/activate@1",
            "surface",
            "Enter",
            "Enter",
        ),
        (
            "latticeaxiom:action/ui/back@1",
            "surface",
            "Escape",
            "Escape",
        ),
        (
            "latticeaxiom:action/hud/toggle-inventory@1",
            "hud-overlay",
            "E",
            "E",
        ),
        (
            "latticeaxiom:action/hud/toggle-workbench@1",
            "hud-overlay",
            "C",
            "C",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-1@1",
            "gameplay",
            "Digit1",
            "Digit1",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-2@1",
            "gameplay",
            "Digit2",
            "Digit2",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-3@1",
            "gameplay",
            "Digit3",
            "Digit3",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-4@1",
            "gameplay",
            "Digit4",
            "Digit4",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-5@1",
            "gameplay",
            "Digit5",
            "Digit5",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-6@1",
            "gameplay",
            "Digit6",
            "Digit6",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-7@1",
            "gameplay",
            "Digit7",
            "Digit7",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-8@1",
            "gameplay",
            "Digit8",
            "Digit8",
        ),
        (
            "latticeaxiom:action/hud/hotbar-slot-9@1",
            "gameplay",
            "Digit9",
            "Digit9",
        ),
        (
            "latticeaxiom:action/hud/hotbar-next@1",
            "gameplay",
            "WheelDown",
            "WheelDown",
        ),
        (
            "latticeaxiom:action/hud/hotbar-previous@1",
            "gameplay",
            "WheelUp",
            "WheelUp",
        ),
    ];
    ACTIONS
        .into_iter()
        .map(
            |(action, context, default_label, effective_label)| SettingsBindingRow {
                action: match action.parse() {
                    Ok(id) => id,
                    Err(error) => panic!("{action} is a compile-time-valid action ID: {error}"),
                },
                context: context.to_owned(),
                owner: if action.contains("/gameplay/") {
                    "@latticeaxiom/input".to_owned()
                } else {
                    "@latticeaxiom/front-end".to_owned()
                },
                default_label: default_label.to_owned(),
                effective_label: effective_label.to_owned(),
            },
        )
        .collect()
}

fn setting_row_node(
    row: &SettingsSurfaceRow,
    interaction_enabled: bool,
    selected: bool,
) -> Result<SemanticNode, SettingsPageError> {
    let prefix = if matches!(row.control, crate::control::SettingsControlKind::KeyBinding) {
        "settings/controls"
    } else {
        "settings/row"
    };
    Ok(SemanticNode {
        key: semantic_key(&format!("{prefix}/{}", sanitize_id(row.id.as_str())))?,
        role: match row.control {
            crate::control::SettingsControlKind::Toggle => SemanticRole::Switch,
            crate::control::SettingsControlKind::IntegerSlider
            | crate::control::SettingsControlKind::FloatSlider => SemanticRole::Slider,
            crate::control::SettingsControlKind::Text
            | crate::control::SettingsControlKind::Path => SemanticRole::TextInput,
            crate::control::SettingsControlKind::Command
            | crate::control::SettingsControlKind::KeyBinding => SemanticRole::Button,
            _ => SemanticRole::Group,
        },
        name: row.label_key.clone(),
        value: Some(row.value.to_string()),
        description: Some(format!(
            "Owner {}; control {}; impact {:?}; editable {}; section {}",
            row.owner,
            row.control.vocabulary_id(),
            row.impact,
            row.editable,
            section_for_setting(&row.id).label_key()
        )),
        state: SemanticState {
            focusable: row.editable && interaction_enabled,
            focused: selected,
            disabled: !row.editable || !interaction_enabled,
            expanded: None,
            busy: false,
        },
        actions: if row.editable && interaction_enabled {
            BTreeSet::from([SemanticAction::Activate])
        } else {
            BTreeSet::new()
        },
        children: Vec::new(),
    })
}

fn search_value(page: &SettingsPageSession) -> String {
    page.surface.search().to_owned()
}

/// Returns a player-facing label for a stable setting or action identity.
#[must_use]
pub fn setting_display_name(id: &StableId) -> String {
    if id.as_str() == "latticeaxiom:setting/view-distance" {
        return "Render Distance".to_owned();
    }
    let leaf = id.path().rsplit('/').next().unwrap_or(id.path());
    title_case(leaf)
}

/// Returns a player-facing category label.
#[must_use]
pub fn category_display_name(category: SettingsCategoryV1) -> &'static str {
    match category {
        SettingsCategoryV1::Accessibility => "Accessibility",
        SettingsCategoryV1::Controls => "Controls",
        SettingsCategoryV1::Audio => "Audio",
        SettingsCategoryV1::Video => "Video",
        SettingsCategoryV1::Interface => "Interface",
        SettingsCategoryV1::Gameplay => "Gameplay",
        SettingsCategoryV1::World => "World",
        SettingsCategoryV1::Packages => "Packages",
        SettingsCategoryV1::Developer => "Developer",
    }
}

/// Returns a player-facing section label.
#[must_use]
pub fn section_display_name(section: SettingsSectionV1) -> &'static str {
    match section {
        SettingsSectionV1::AccessibilityVision => "Vision",
        SettingsSectionV1::AccessibilityHearing => "Hearing",
        SettingsSectionV1::AccessibilityMotion => "Motion",
        SettingsSectionV1::ControlsMouse => "Mouse",
        SettingsSectionV1::ControlsKeyboard => "Keyboard",
        SettingsSectionV1::ControlsGamepad => "Gamepad",
        SettingsSectionV1::ControlsPackageActions => "Package actions",
        SettingsSectionV1::AudioVolumes => "Volumes",
        SettingsSectionV1::AudioDevices => "Devices",
        SettingsSectionV1::VideoGeneral | SettingsSectionV1::GameplayGeneral => "General",
        SettingsSectionV1::VideoQuality => "Quality",
        SettingsSectionV1::VideoPerformance => "Performance",
        SettingsSectionV1::VideoAdvanced => "Advanced",
        SettingsSectionV1::InterfaceDisplay => "Display",
        SettingsSectionV1::InterfaceLanguage => "Language",
        SettingsSectionV1::InterfaceInspect => "Inspect",
        SettingsSectionV1::WorldLibrary => "Library",
        SettingsSectionV1::WorldRules => "Rules",
        SettingsSectionV1::PackagesCatalog => "Catalog",
        SettingsSectionV1::DeveloperOverlay => "Overlay",
    }
}

fn title_case(value: &str) -> String {
    value
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut text = first.to_uppercase().collect::<String>();
                    text.push_str(chars.as_str());
                    text
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn advanced_setting_id() -> StableId {
    match "latticeaxiom:setting/interface/show-advanced-settings".parse() {
        Ok(id) => id,
        Err(error) => {
            panic!("advanced-settings identity is a compile-time fixture: {error}")
        }
    }
}

fn category_slug(category: SettingsCategoryV1) -> &'static str {
    match category {
        SettingsCategoryV1::Accessibility => "accessibility",
        SettingsCategoryV1::Controls => "controls",
        SettingsCategoryV1::Audio => "audio",
        SettingsCategoryV1::Video => "video",
        SettingsCategoryV1::Interface => "interface",
        SettingsCategoryV1::Gameplay => "gameplay",
        SettingsCategoryV1::World => "world",
        SettingsCategoryV1::Packages => "packages",
        SettingsCategoryV1::Developer => "developer",
    }
}

fn section_slug(section: SettingsSectionV1) -> &'static str {
    match section {
        SettingsSectionV1::AccessibilityVision => "accessibility-vision",
        SettingsSectionV1::AccessibilityHearing => "accessibility-hearing",
        SettingsSectionV1::AccessibilityMotion => "accessibility-motion",
        SettingsSectionV1::ControlsMouse => "controls-mouse",
        SettingsSectionV1::ControlsKeyboard => "controls-keyboard",
        SettingsSectionV1::ControlsGamepad => "controls-gamepad",
        SettingsSectionV1::ControlsPackageActions => "controls-package-actions",
        SettingsSectionV1::AudioVolumes => "audio-volumes",
        SettingsSectionV1::AudioDevices => "audio-devices",
        SettingsSectionV1::VideoGeneral => "video-general",
        SettingsSectionV1::VideoQuality => "video-quality",
        SettingsSectionV1::VideoPerformance => "video-performance",
        SettingsSectionV1::VideoAdvanced => "video-advanced",
        SettingsSectionV1::InterfaceDisplay => "interface-display",
        SettingsSectionV1::InterfaceLanguage => "interface-language",
        SettingsSectionV1::InterfaceInspect => "interface-inspect",
        SettingsSectionV1::GameplayGeneral => "gameplay-general",
        SettingsSectionV1::WorldLibrary => "world-library",
        SettingsSectionV1::WorldRules => "world-rules",
        SettingsSectionV1::PackagesCatalog => "packages-catalog",
        SettingsSectionV1::DeveloperOverlay => "developer-overlay",
    }
}

fn semantic_key(value: &str) -> Result<SemanticKey, SettingsPageError> {
    SemanticKey::new(value).map_err(|_| SettingsPageError::InvalidSemanticKey)
}

fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '/' | '-' | '_' | ':')
            {
                character
            } else {
                '-'
            }
        })
        .collect()
}
