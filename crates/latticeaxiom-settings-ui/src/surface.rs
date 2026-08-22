//! Mechanical settings catalog projection. Packages contribute typed rows only.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_client_ui::{
    ButtonWidget, SemanticAction, SemanticKey, SemanticNode, SemanticRole, SemanticState,
    application_root,
};
use latticeaxiom_core::StableId;
use latticeaxiom_runtime_contracts::{
    EffectiveSettingsSnapshot, RuntimeApplyImpact, SettingAuthority, SettingSpec, SettingWriter,
    SettingsDurabilityDomain,
};
use serde_json::Value;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::category::{SettingsCategoryError, SettingsCategoryV1};
use crate::control::SettingsControlKind;

/// Client/tool authority used to mark world rows read-only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsSurfaceAuthority {
    /// Whether a sealed world writer is open.
    pub has_world_writer: bool,
    /// Authenticated writer kind when a writer is open.
    pub writer: Option<SettingWriter>,
}

impl SettingsSurfaceAuthority {
    /// Shell/tool authority without a world writer.
    #[must_use]
    pub const fn shell() -> Self {
        Self {
            has_world_writer: false,
            writer: None,
        }
    }
}

/// One mechanically generated settings row.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsSurfaceRow {
    /// Stable setting identity.
    pub id: StableId,
    /// Declaring package.
    pub owner: String,
    /// Frozen category.
    pub category: SettingsCategoryV1,
    /// Owner-local order.
    pub order: i32,
    /// Control vocabulary used for this row.
    pub control: SettingsControlKind,
    /// Current typed value rendered as JSON.
    pub value: Value,
    /// Label localization key.
    pub label_key: String,
    /// Description localization key.
    pub description_key: String,
    /// Whether the row may be edited.
    pub editable: bool,
    /// Read-only reason when `editable` is false.
    pub read_only_reason: Option<SettingsReadOnlyReason>,
    /// Restart or world-reactivation impact disclosed before apply.
    pub impact: RuntimeApplyImpact,
    /// Owning durability domain.
    pub domain: SettingsDurabilityDomain,
}

/// Why a row is displayed read-only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsReadOnlyReason {
    /// World-authoritative value without writer authority.
    MissingWriterAuthority,
    /// Sensitivity or authority forbids client mutation.
    AuthorityDenied,
}

/// Stable category-grouped settings surface.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsSurfaceModel {
    rows: Vec<SettingsSurfaceRow>,
    search: String,
}

impl SettingsSurfaceModel {
    /// Builds generic rows from the effective snapshot and compiled specs.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] when a required category is unknown or
    /// a row is missing from the compiled catalog.
    pub fn from_snapshot(
        catalog: &BTreeMap<StableId, SettingSpec>,
        snapshot: &EffectiveSettingsSnapshot,
        authority: SettingsSurfaceAuthority,
    ) -> Result<Self, SettingsSurfaceError> {
        let mut rows = snapshot
            .values()
            .values()
            .map(|effective| {
                let spec = catalog.get(&effective.id).ok_or_else(|| {
                    SettingsSurfaceError::MissingSpec {
                        setting: effective.id.clone(),
                    }
                })?;
                let category = SettingsCategoryV1::parse_id(&spec.category)?;
                let editable = row_editable(spec, authority);
                Ok(SettingsSurfaceRow {
                    id: effective.id.clone(),
                    owner: effective.declared_by.to_string(),
                    category,
                    order: spec.order,
                    control: SettingsControlKind::from_value_type(&spec.value_type),
                    value: effective.value.clone(),
                    label_key: spec.label_key.clone(),
                    description_key: spec.description_key.clone(),
                    editable,
                    read_only_reason: (!editable).then_some(read_only_reason(spec, authority)),
                    impact: effective.apply_impact,
                    domain: spec.default_scope.into(),
                })
            })
            .collect::<Result<Vec<_>, SettingsSurfaceError>>()?;
        rows.sort_by(|left, right| {
            left.category
                .cmp(&right.category)
                .then(left.order.cmp(&right.order))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(Self {
            rows,
            search: String::new(),
        })
    }

    /// Returns rows in category/order/ID order.
    #[must_use]
    pub fn rows(&self) -> &[SettingsSurfaceRow] {
        &self.rows
    }

    /// Returns rows matching a NFC search query.
    #[must_use]
    pub fn visible_rows(&self) -> Vec<&SettingsSurfaceRow> {
        if self.search.is_empty() {
            return self.rows.iter().collect();
        }
        let needle = self.search.nfc().collect::<String>().to_lowercase();
        self.rows
            .iter()
            .filter(|row| {
                row.id.as_str().to_lowercase().contains(&needle)
                    || row.label_key.to_lowercase().contains(&needle)
                    || row.owner.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// Updates the search query used by [`Self::visible_rows`].
    pub fn set_search(&mut self, query: impl Into<String>) {
        self.search = query.into().nfc().collect();
    }

    /// Returns whether visible editable rows span more than one durability domain.
    #[must_use]
    pub fn mixed_durability_domains(&self) -> bool {
        let mut domains = Vec::new();
        for row in self.visible_rows().into_iter().filter(|row| row.editable) {
            if !domains.contains(&row.domain) {
                domains.push(row.domain);
            }
        }
        domains.len() > 1
    }

    /// Rejects a combined apply when multiple durability domains are dirty.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError::MixedDurabilityDomains`] when the draft
    /// would pretend cross-store atomicity.
    pub fn require_single_domain_apply(&self) -> Result<(), SettingsSurfaceError> {
        if self.mixed_durability_domains() {
            Err(SettingsSurfaceError::MixedDurabilityDomains)
        } else {
            Ok(())
        }
    }

    /// Projects the unique settings surface into the shared semantic tree.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] when a semantic key is invalid or a
    /// second UI root would be created.
    pub fn semantic_tree(&self) -> Result<SemanticNode, SettingsSurfaceError> {
        let mut children = vec![
            ButtonWidget {
                key: semantic_key("settings/back")?,
                name: "Back".to_owned(),
                description: Some("Return without applying".to_owned()),
                enabled: true,
            }
            .semantic_node(false),
        ];
        for category in SettingsCategoryV1::ALL {
            let rows = self
                .visible_rows()
                .into_iter()
                .filter(|row| row.category == category)
                .collect::<Vec<_>>();
            if rows.is_empty() {
                continue;
            }
            let mut group_children = Vec::new();
            for row in rows {
                group_children.push(row_node(row)?);
            }
            children.push(SemanticNode {
                key: semantic_key(&format!("settings/category/{category:?}").to_lowercase())?,
                role: SemanticRole::Group,
                name: category.label_key().to_owned(),
                value: None,
                description: None,
                state: SemanticState::default(),
                actions: BTreeSet::new(),
                children: group_children,
            });
        }
        children.push(
            ButtonWidget {
                key: semantic_key("settings/apply")?,
                name: "Apply settings".to_owned(),
                description: Some(
                    "Validate the complete draft and show impact before atomic persistence"
                        .to_owned(),
                ),
                enabled: !self.mixed_durability_domains(),
            }
            .semantic_node(false),
        );
        application_root(semantic_key("settings")?, "Settings", children)
            .map_err(|_| SettingsSurfaceError::SecondUiRoot)
    }
}

fn row_editable(spec: &SettingSpec, authority: SettingsSurfaceAuthority) -> bool {
    match spec.authority {
        SettingAuthority::LocalUser => true,
        SettingAuthority::WorldOwner | SettingAuthority::Server | SettingAuthority::AdminOnly => {
            authority.has_world_writer
                && authority.writer.is_some_and(|writer| match spec.authority {
                    SettingAuthority::WorldOwner => writer == SettingWriter::WorldOwner,
                    SettingAuthority::Server => writer == SettingWriter::Server,
                    SettingAuthority::AdminOnly => writer == SettingWriter::VerifiedAdmin,
                    SettingAuthority::LocalUser | SettingAuthority::FixedByProfile => false,
                })
        }
        SettingAuthority::FixedByProfile => false,
    }
}

fn read_only_reason(
    spec: &SettingSpec,
    authority: SettingsSurfaceAuthority,
) -> SettingsReadOnlyReason {
    if matches!(
        spec.authority,
        SettingAuthority::WorldOwner | SettingAuthority::Server | SettingAuthority::AdminOnly
    ) && !authority.has_world_writer
    {
        SettingsReadOnlyReason::MissingWriterAuthority
    } else {
        SettingsReadOnlyReason::AuthorityDenied
    }
}

fn row_node(row: &SettingsSurfaceRow) -> Result<SemanticNode, SettingsSurfaceError> {
    let key = semantic_key(&format!("settings/row/{}", sanitize_id(row.id.as_str())))?;
    Ok(SemanticNode {
        key,
        role: match row.control {
            SettingsControlKind::Toggle => SemanticRole::Switch,
            SettingsControlKind::IntegerSlider | SettingsControlKind::FloatSlider => {
                SemanticRole::Slider
            }
            SettingsControlKind::Text | SettingsControlKind::Path => SemanticRole::TextInput,
            SettingsControlKind::Command => SemanticRole::Button,
            _ => SemanticRole::Group,
        },
        name: row.label_key.clone(),
        value: Some(row.value.to_string()),
        description: Some(format!(
            "Owner {}; control {}; impact {:?}; editable {}",
            row.owner,
            row.control.vocabulary_id(),
            row.impact,
            row.editable
        )),
        state: SemanticState {
            focusable: row.editable,
            focused: false,
            disabled: !row.editable,
            expanded: None,
            busy: false,
        },
        actions: if row.editable {
            BTreeSet::from([SemanticAction::Activate])
        } else {
            BTreeSet::new()
        },
        children: Vec::new(),
    })
}

fn semantic_key(value: &str) -> Result<SemanticKey, SettingsSurfaceError> {
    SemanticKey::new(value).map_err(|_| SettingsSurfaceError::InvalidSemanticKey)
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

/// Settings surface projection failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SettingsSurfaceError {
    /// Effective snapshot referenced a setting absent from the catalog.
    #[error("settings surface is missing spec `{setting}`")]
    MissingSpec {
        /// Missing setting.
        setting: StableId,
    },
    /// Required category is not in the frozen vocabulary.
    #[error(transparent)]
    Category(#[from] SettingsCategoryError),
    /// A generated semantic key was invalid.
    #[error("settings surface generated an invalid semantic key")]
    InvalidSemanticKey,
    /// Projection attempted to spawn a second UI root.
    #[error("settings surface cannot create a second UI root")]
    SecondUiRoot,
    /// A UI draft mixed durability domains instead of splitting them.
    #[error("settings draft mixes durability domains and must be split")]
    MixedDurabilityDomains,
}
