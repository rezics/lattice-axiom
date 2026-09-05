//! Mechanical settings catalog projection. Packages contribute typed rows only.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_client_ui::{
    ButtonWidget, SemanticAction, SemanticKey, SemanticNode, SemanticRole, SemanticState,
    application_root,
};
use latticeaxiom_core::StableId;
use latticeaxiom_runtime_contracts::{
    EffectiveSettingsSnapshot, RestartImpactMetadata, RuntimeApplyImpact, SettingAuthority,
    SettingSpec, SettingWriter, SettingsDurabilityDomain, ValueType,
};
use serde_json::Value;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::category::{SettingsCategoryError, SettingsCategoryV1};
use crate::control::{SettingsControlKind, SettingsSliderConstraint};

/// Client/tool authority used to mark world rows read-only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsSurfaceAuthority {
    /// Whether a sealed world writer is open.
    pub has_world_writer: bool,
    /// Authenticated writer kind when a writer is open.
    pub writer: Option<SettingWriter>,
}

/// Scope filter applied to the typed catalog projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsSurfaceScope {
    /// Shell shows every frozen category, including Packages.
    Shell,
    /// In-game Settings from Pause hides composition Packages rows.
    InGame,
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
    /// Proven range for a bounded numeric slider.
    pub slider: Option<SettingsSliderConstraint>,
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

/// Direction of one catalog-sized slider step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsSliderDirection {
    /// Move toward the inclusive minimum.
    Decrement,
    /// Move toward the inclusive maximum.
    Increment,
}

/// Catalog-proved integer-slider state consumed by a renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsIntegerSliderState {
    /// Inclusive minimum.
    pub min: i64,
    /// Greatest step-aligned value not exceeding the schema maximum.
    pub max: i64,
    /// Positive catalog step.
    pub step: u64,
    /// Last host-confirmed or visibly published value.
    pub applied: i64,
    /// Current presentation draft.
    pub draft: i64,
}

/// Typed user intent accepted by the settings surface model.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsSurfaceCommand {
    /// Open a fresh editing session from the applied values.
    BeginEdit,
    /// Replace one editable value after schema validation.
    SetValue {
        /// Stable setting identity.
        setting: StableId,
        /// Proposed typed JSON value.
        value: Value,
    },
    /// Snap a renderer-provided value to an integer slider's catalog range.
    SetIntegerSliderValue {
        /// Stable setting identity.
        setting: StableId,
        /// Finite renderer value, before catalog snapping.
        value: f32,
    },
    /// Move an integer slider by exactly one catalog step.
    StepIntegerSlider {
        /// Stable setting identity.
        setting: StableId,
        /// Requested direction.
        direction: SettingsSliderDirection,
    },
    /// Freeze the complete one-domain draft for host execution.
    Apply,
    /// Restore every draft value while keeping the surface open.
    Undo,
    /// Restore every draft value and request that the surface close.
    Cancel,
}

/// Immutable apply request emitted to the runtime/persistence executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSurfaceApplyRequest {
    /// Single durability domain proved for all proposed values.
    pub domain: SettingsDurabilityDomain,
    /// Dirty values in stable setting-ID order.
    pub proposed: BTreeMap<StableId, Value>,
    /// Impact that must be disclosed before execution.
    pub restart: RestartImpactMetadata,
}

/// Host result fed back into the presentation authority after execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsSurfaceApplyResolution {
    /// Runtime application and durable persistence both completed.
    Committed,
    /// The executor rejected the request before visible publication.
    Rejected,
    /// The proposed value is visible, but durability requires a safe restart.
    PublicationVisibleRestartRequired,
    /// Visible publication cannot be determined and a safe restart is required.
    PublicationUnknownRestartRequired,
}

impl SettingsSurfaceApplyResolution {
    const fn adopts_proposed(self) -> bool {
        matches!(
            self,
            Self::Committed | Self::PublicationVisibleRestartRequired
        )
    }
}

/// Typed lifecycle of the settings presentation authority.
///
/// The pending request is part of the state so an apply cannot coexist with a
/// closed or safe-restart-locked surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsSurfaceState {
    /// No editing session is open.
    Closed,
    /// Draft commands are accepted.
    Editing,
    /// The host is executing this immutable request; input is suspended.
    ApplyPending(SettingsSurfaceApplyRequest),
    /// The live publication state is not safe to edit before process restart.
    SafeRestartLocked,
}

/// Observable result of one accepted surface command or host completion.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsSurfaceOutcome {
    /// A fresh edit session was opened; stale draft IDs were discarded.
    EditingBegan {
        /// Settings restored to their applied values.
        discarded: Vec<StableId>,
    },
    /// One draft value changed.
    DraftChanged {
        /// Changed setting.
        setting: StableId,
        /// Catalog-validated, possibly snapped value.
        value: Value,
    },
    /// Input was valid but did not change the draft.
    Unchanged,
    /// The host must execute this exact frozen request.
    ApplyRequested(SettingsSurfaceApplyRequest),
    /// Undo restored these settings while leaving the surface open.
    DraftRestored {
        /// Settings restored to their applied values.
        discarded: Vec<StableId>,
    },
    /// Cancel restored the draft and requests that the surface close.
    Cancelled {
        /// Settings restored to their applied values.
        discarded: Vec<StableId>,
    },
    /// A pending host execution completed.
    ApplyCompleted {
        /// Typed publication result.
        resolution: SettingsSurfaceApplyResolution,
    },
    /// Editing is locked because the host cannot prove a coherent live state.
    SafeRestartLocked,
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
    applied: BTreeMap<StableId, Value>,
    value_types: BTreeMap<StableId, ValueType>,
    state: SettingsSurfaceState,
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
        let mut applied = BTreeMap::new();
        let mut value_types = BTreeMap::new();
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
                let slider = SettingsSliderConstraint::from_value_type(
                    effective.id.as_str(),
                    &spec.value_type,
                )?;
                let editable = row_editable(spec, authority);
                applied.insert(effective.id.clone(), effective.value.clone());
                value_types.insert(effective.id.clone(), spec.value_type.clone());
                Ok(SettingsSurfaceRow {
                    id: effective.id.clone(),
                    owner: effective.declared_by.to_string(),
                    category,
                    order: spec.order,
                    control: SettingsControlKind::from_value_type(&spec.value_type),
                    value: effective.value.clone(),
                    slider,
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
            applied,
            value_types,
            state: SettingsSurfaceState::Closed,
            search: String::new(),
        })
    }

    /// Returns rows in category/order/ID order.
    #[must_use]
    pub fn rows(&self) -> &[SettingsSurfaceRow] {
        &self.rows
    }

    /// Returns whether at least one draft value differs from its applied value.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.rows.iter().any(|row| self.row_is_dirty(row))
    }

    /// Returns whether an immutable apply request is awaiting host completion.
    #[must_use]
    pub const fn apply_pending(&self) -> bool {
        matches!(&self.state, SettingsSurfaceState::ApplyPending(_))
    }

    /// Returns whether the surface owns an open editing session.
    ///
    /// An immutable apply remains part of the same session even though new
    /// commands are suspended until the host reports completion.
    #[must_use]
    pub const fn is_editing(&self) -> bool {
        matches!(
            &self.state,
            SettingsSurfaceState::Editing | SettingsSurfaceState::ApplyPending(_)
        )
    }

    /// Returns the typed presentation lifecycle.
    #[must_use]
    pub const fn state(&self) -> &SettingsSurfaceState {
        &self.state
    }

    /// Returns whether editing is terminally locked until process restart.
    #[must_use]
    pub const fn is_safe_restart_locked(&self) -> bool {
        matches!(&self.state, SettingsSurfaceState::SafeRestartLocked)
    }

    /// Returns one applied value.
    #[must_use]
    pub fn applied_value(&self, setting: &StableId) -> Option<&Value> {
        self.applied.get(setting)
    }

    /// Returns one current draft value.
    #[must_use]
    pub fn draft_value(&self, setting: &StableId) -> Option<&Value> {
        self.row(setting).map(|row| &row.value)
    }

    /// Returns the complete state needed to render one integer slider.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting is absent, is not an integer slider,
    /// or its values contradict the compiled catalog.
    pub fn integer_slider(
        &self,
        setting: &StableId,
    ) -> Result<SettingsIntegerSliderState, SettingsSurfaceError> {
        let row = self
            .row(setting)
            .ok_or_else(|| SettingsSurfaceError::UnknownSetting {
                setting: setting.clone(),
            })?;
        let Some(SettingsSliderConstraint::Integer { min, max, step }) = row.slider.as_ref() else {
            return Err(SettingsSurfaceError::NotIntegerSlider {
                setting: setting.clone(),
            });
        };
        let applied = self
            .applied
            .get(setting)
            .and_then(Value::as_i64)
            .ok_or_else(|| SettingsSurfaceError::InvalidModelValue {
                setting: setting.clone(),
            })?;
        let draft = row
            .value
            .as_i64()
            .ok_or_else(|| SettingsSurfaceError::InvalidModelValue {
                setting: setting.clone(),
            })?;
        Ok(SettingsIntegerSliderState {
            min: *min,
            max: *max,
            step: *step,
            applied,
            draft,
        })
    }

    /// Applies one typed user command to the presentation state.
    ///
    /// Apply emits an immutable request only. Runtime mutation and persistence
    /// remain the host's responsibility and are reported through completion.
    ///
    /// # Errors
    ///
    /// Returns an error when no editing session is open, safe restart has
    /// locked the surface, the command targets an unknown or read-only row,
    /// supplies an invalid value, mixes durability domains, or races a pending
    /// apply.
    pub fn handle(
        &mut self,
        command: SettingsSurfaceCommand,
    ) -> Result<SettingsSurfaceOutcome, SettingsSurfaceError> {
        match &self.state {
            SettingsSurfaceState::SafeRestartLocked => {
                return Err(SettingsSurfaceError::SafeRestartLocked);
            }
            SettingsSurfaceState::ApplyPending(_) => {
                return Err(SettingsSurfaceError::ApplyPending);
            }
            SettingsSurfaceState::Closed | SettingsSurfaceState::Editing => {}
        }
        if !matches!(&command, SettingsSurfaceCommand::BeginEdit)
            && !matches!(&self.state, SettingsSurfaceState::Editing)
        {
            return Err(SettingsSurfaceError::NotEditing);
        }
        match command {
            SettingsSurfaceCommand::BeginEdit => {
                let discarded = self.restore_draft();
                self.state = SettingsSurfaceState::Editing;
                Ok(SettingsSurfaceOutcome::EditingBegan { discarded })
            }
            SettingsSurfaceCommand::SetValue { setting, value } => self.set_value(setting, value),
            SettingsSurfaceCommand::SetIntegerSliderValue { setting, value } => {
                let state = self.integer_slider(&setting)?;
                if !value.is_finite() {
                    return Err(SettingsSurfaceError::NonFiniteSliderInput { setting });
                }
                let snapped = snap_integer_slider(value, state).ok_or_else(|| {
                    SettingsSurfaceError::UnrepresentableSliderConstraint {
                        setting: setting.clone(),
                    }
                })?;
                self.set_value(setting, Value::from(snapped))
            }
            SettingsSurfaceCommand::StepIntegerSlider { setting, direction } => {
                let state = self.integer_slider(&setting)?;
                let next = step_integer_slider(state, direction).ok_or_else(|| {
                    SettingsSurfaceError::InvalidModelValue {
                        setting: setting.clone(),
                    }
                })?;
                self.set_value(setting, Value::from(next))
            }
            SettingsSurfaceCommand::Apply => self.request_apply(),
            SettingsSurfaceCommand::Undo => {
                let discarded = self.restore_draft();
                Ok(SettingsSurfaceOutcome::DraftRestored { discarded })
            }
            SettingsSurfaceCommand::Cancel => {
                let discarded = self.restore_draft();
                self.state = SettingsSurfaceState::Closed;
                Ok(SettingsSurfaceOutcome::Cancelled { discarded })
            }
        }
    }

    /// Records the host result for the one pending immutable apply request.
    ///
    /// # Errors
    ///
    /// Returns an error when no request is in flight. A visible proposed
    /// publication becomes the new applied value; a rejection or unknown
    /// publication retains the draft for diagnostics. Either restart-required
    /// resolution enters the terminal safe-restart-locked state.
    pub fn finish_apply(
        &mut self,
        resolution: SettingsSurfaceApplyResolution,
    ) -> Result<SettingsSurfaceOutcome, SettingsSurfaceError> {
        let prior_state = std::mem::replace(&mut self.state, SettingsSurfaceState::Closed);
        let pending = match prior_state {
            SettingsSurfaceState::ApplyPending(request) => request,
            state => {
                self.state = state;
                return Err(SettingsSurfaceError::NoPendingApply);
            }
        };
        if resolution.adopts_proposed() {
            for (setting, value) in pending.proposed {
                self.applied.insert(setting, value);
            }
        }
        self.state = match resolution {
            SettingsSurfaceApplyResolution::Committed
            | SettingsSurfaceApplyResolution::Rejected => SettingsSurfaceState::Editing,
            SettingsSurfaceApplyResolution::PublicationVisibleRestartRequired
            | SettingsSurfaceApplyResolution::PublicationUnknownRestartRequired => {
                SettingsSurfaceState::SafeRestartLocked
            }
        };
        Ok(SettingsSurfaceOutcome::ApplyCompleted { resolution })
    }

    /// Locks editing after the host detects an unrecoverable live-state split.
    ///
    /// This does not guess which draft is durable. It only clears an unusable
    /// pending request and prevents the presentation from reopening before the
    /// host performs a safe process restart.
    pub fn lock_for_safe_restart(&mut self) -> SettingsSurfaceOutcome {
        self.state = SettingsSurfaceState::SafeRestartLocked;
        SettingsSurfaceOutcome::SafeRestartLocked
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

    /// Returns the NFC search query used by [`Self::visible_rows`].
    #[must_use]
    pub fn search(&self) -> &str {
        &self.search
    }

    /// Updates the search query used by [`Self::visible_rows`].
    pub fn set_search(&mut self, query: impl Into<String>) {
        self.search = query.into().nfc().collect();
    }

    /// Applies shell versus in-game category filtering.
    pub fn apply_scope_filter(&mut self, scope: SettingsSurfaceScope) {
        if scope == SettingsSurfaceScope::InGame {
            self.rows
                .retain(|row| row.category != SettingsCategoryV1::Packages);
        }
    }

    /// Strongest apply impact among dirty editable rows.
    #[must_use]
    pub fn restart_impact(&self) -> RestartImpactMetadata {
        let strongest = self
            .rows
            .iter()
            .filter(|row| row.editable && self.row_is_dirty(row))
            .map(|row| row.impact)
            .max_by_key(|impact| match impact {
                RuntimeApplyImpact::Preview => 0_u8,
                RuntimeApplyImpact::Immediate => 1,
                RuntimeApplyImpact::WorldReactivate => 2,
                RuntimeApplyImpact::ProcessRestart => 3,
            });
        RestartImpactMetadata {
            strongest_impact: strongest,
            process_restart: matches!(strongest, Some(RuntimeApplyImpact::ProcessRestart)),
            world_reactivate: matches!(
                strongest,
                Some(RuntimeApplyImpact::WorldReactivate | RuntimeApplyImpact::ProcessRestart)
            ),
            live_immediate: matches!(strongest, Some(RuntimeApplyImpact::Immediate)),
        }
    }

    /// Returns whether dirty editable rows span more than one durability domain.
    #[must_use]
    pub fn mixed_durability_domains(&self) -> bool {
        let mut domains = Vec::new();
        for row in self
            .rows
            .iter()
            .filter(|row| row.editable && self.row_is_dirty(row))
        {
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

    fn row(&self, setting: &StableId) -> Option<&SettingsSurfaceRow> {
        self.rows.iter().find(|row| &row.id == setting)
    }

    fn row_mut(&mut self, setting: &StableId) -> Option<&mut SettingsSurfaceRow> {
        self.rows.iter_mut().find(|row| &row.id == setting)
    }

    fn row_is_dirty(&self, row: &SettingsSurfaceRow) -> bool {
        self.applied.get(&row.id) != Some(&row.value)
    }

    fn dirty_setting_ids(&self) -> Vec<StableId> {
        self.rows
            .iter()
            .filter(|row| self.row_is_dirty(row))
            .map(|row| row.id.clone())
            .collect()
    }

    fn restore_draft(&mut self) -> Vec<StableId> {
        let discarded = self.dirty_setting_ids();
        for setting in &discarded {
            if let Some(value) = self.applied.get(setting).cloned()
                && let Some(row) = self.row_mut(setting)
            {
                row.value = value;
            }
        }
        discarded
    }

    fn set_value(
        &mut self,
        setting: StableId,
        value: Value,
    ) -> Result<SettingsSurfaceOutcome, SettingsSurfaceError> {
        let row = self
            .row(&setting)
            .ok_or_else(|| SettingsSurfaceError::UnknownSetting {
                setting: setting.clone(),
            })?;
        if !row.editable {
            return Err(SettingsSurfaceError::ReadOnlySetting { setting });
        }
        let value_type =
            self.value_types
                .get(&setting)
                .ok_or_else(|| SettingsSurfaceError::UnknownSetting {
                    setting: setting.clone(),
                })?;
        value_type.validate_value(&value).map_err(|source| {
            SettingsSurfaceError::InvalidDraftValue {
                setting: setting.clone(),
                reason: source.to_string(),
            }
        })?;
        if row.value == value {
            return Ok(SettingsSurfaceOutcome::Unchanged);
        }
        let row = self
            .row_mut(&setting)
            .ok_or_else(|| SettingsSurfaceError::UnknownSetting {
                setting: setting.clone(),
            })?;
        row.value = value.clone();
        Ok(SettingsSurfaceOutcome::DraftChanged { setting, value })
    }

    fn request_apply(&mut self) -> Result<SettingsSurfaceOutcome, SettingsSurfaceError> {
        let dirty = self.dirty_setting_ids();
        if dirty.is_empty() {
            return Err(SettingsSurfaceError::EmptyDraft);
        }
        self.require_single_domain_apply()?;
        let first = dirty
            .first()
            .and_then(|setting| self.row(setting))
            .ok_or(SettingsSurfaceError::EmptyDraft)?;
        let domain = first.domain;
        let proposed = dirty
            .iter()
            .filter_map(|setting| {
                self.row(setting)
                    .map(|row| (setting.clone(), row.value.clone()))
            })
            .collect();
        let request = SettingsSurfaceApplyRequest {
            domain,
            proposed,
            restart: self.restart_impact(),
        };
        self.state = SettingsSurfaceState::ApplyPending(request.clone());
        Ok(SettingsSurfaceOutcome::ApplyRequested(request))
    }

    /// Projects a settings fragment that can be hosted under the unique UI root.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] when a semantic key is invalid.
    pub fn semantic_fragment(&self) -> Result<SemanticNode, SettingsSurfaceError> {
        let impact = self.restart_impact();
        let interaction_enabled = matches!(&self.state, SettingsSurfaceState::Editing);
        let apply_description = if self.is_safe_restart_locked() {
            "Settings state requires a safe process restart; editing is locked."
        } else if impact.process_restart {
            "Process restart required after persistence. The UI never writes the file."
        } else if impact.world_reactivate {
            "World reactivation required after persistence. The UI never writes the file."
        } else {
            "Validate the complete draft and show impact before atomic persistence"
        };
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
                value: Some(self.search.clone()),
                description: Some("IME and CJK search over identity, label, and owner".to_owned()),
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
                group_children.push(row_node(row, interaction_enabled)?);
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
                key: semantic_key("settings/undo")?,
                name: "Undo changes".to_owned(),
                description: Some("Restore the last applied values".to_owned()),
                enabled: interaction_enabled && self.is_dirty(),
            }
            .semantic_node(false),
        );
        children.push(
            ButtonWidget {
                key: semantic_key("settings/apply")?,
                name: "Apply settings".to_owned(),
                description: Some(apply_description.to_owned()),
                enabled: self.is_dirty() && interaction_enabled && !self.mixed_durability_domains(),
            }
            .semantic_node(false),
        );
        Ok(SemanticNode {
            key: semantic_key("modal/settings")?,
            role: SemanticRole::Group,
            name: "Settings".to_owned(),
            value: impact
                .strongest_impact
                .map(|impact| format!("{impact:?}").to_lowercase()),
            description: Some(apply_description.to_owned()),
            state: SemanticState {
                focusable: false,
                focused: false,
                disabled: false,
                expanded: Some(true),
                busy: self.apply_pending(),
            },
            actions: BTreeSet::from([SemanticAction::Cancel, SemanticAction::Back]),
            children,
        })
    }

    /// Projects the unique settings surface into the shared semantic tree.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] when a semantic key is invalid or a
    /// second UI root would be created.
    pub fn semantic_tree(&self) -> Result<SemanticNode, SettingsSurfaceError> {
        application_root(
            semantic_key("settings")?,
            "Settings",
            vec![self.semantic_fragment()?],
        )
        .map_err(|_| SettingsSurfaceError::SecondUiRoot)
    }
}

fn snap_integer_slider(value: f32, state: SettingsIntegerSliderState) -> Option<i64> {
    if !value.is_finite() || state.step == 0 || state.min > state.max {
        return None;
    }
    let minimum = f64::from(i32::try_from(state.min).ok()?);
    let maximum = f64::from(i32::try_from(state.max).ok()?);
    let step = f64::from(u32::try_from(state.step).ok()?);
    let clamped = f64::from(value).clamp(minimum, maximum);
    let step_index = ((clamped - minimum) / step).round();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the finite index is clamped to an i32-bounded, positive-step slider"
    )]
    let step_index = step_index as u64;
    let snapped = i128::from(state.min)
        .saturating_add(i128::from(step_index).saturating_mul(i128::from(state.step)))
        .clamp(i128::from(state.min), i128::from(state.max));
    i64::try_from(snapped).ok()
}

fn step_integer_slider(
    state: SettingsIntegerSliderState,
    direction: SettingsSliderDirection,
) -> Option<i64> {
    let draft = i128::from(state.draft);
    let step = i128::from(state.step);
    let candidate = match direction {
        SettingsSliderDirection::Decrement => draft.saturating_sub(step),
        SettingsSliderDirection::Increment => draft.saturating_add(step),
    };
    i64::try_from(candidate.clamp(i128::from(state.min), i128::from(state.max))).ok()
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

fn row_node(
    row: &SettingsSurfaceRow,
    interaction_enabled: bool,
) -> Result<SemanticNode, SettingsSurfaceError> {
    let prefix = if row.control == SettingsControlKind::KeyBinding {
        "settings/controls"
    } else {
        "settings/row"
    };
    let key = semantic_key(&format!("{prefix}/{}", sanitize_id(row.id.as_str())))?;
    Ok(SemanticNode {
        key,
        role: match row.control {
            SettingsControlKind::Toggle => SemanticRole::Switch,
            SettingsControlKind::IntegerSlider | SettingsControlKind::FloatSlider => {
                SemanticRole::Slider
            }
            SettingsControlKind::Text | SettingsControlKind::Path => SemanticRole::TextInput,
            SettingsControlKind::Command | SettingsControlKind::KeyBinding => SemanticRole::Button,
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
            focusable: row.editable && interaction_enabled,
            focused: false,
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
    /// A numeric schema cannot be projected into the required slider vocabulary.
    #[error(transparent)]
    Control(#[from] crate::control::SettingsControlError),
    /// A generated semantic key was invalid.
    #[error("settings surface generated an invalid semantic key")]
    InvalidSemanticKey,
    /// Projection attempted to spawn a second UI root.
    #[error("settings surface cannot create a second UI root")]
    SecondUiRoot,
    /// A UI draft mixed durability domains instead of splitting them.
    #[error("settings draft mixes durability domains and must be split")]
    MixedDurabilityDomains,
    /// A typed command targeted a setting absent from the surface.
    #[error("settings surface does not contain {setting}")]
    UnknownSetting {
        /// Missing setting.
        setting: StableId,
    },
    /// A typed command attempted to edit a read-only row.
    #[error("settings surface row {setting} is read-only")]
    ReadOnlySetting {
        /// Read-only setting.
        setting: StableId,
    },
    /// A slider command targeted another control kind.
    #[error("settings surface row {setting} is not an integer slider")]
    NotIntegerSlider {
        /// Mismatched setting.
        setting: StableId,
    },
    /// A renderer supplied NaN or infinity to a slider command.
    #[error("settings slider {setting} received a non-finite value")]
    NonFiniteSliderInput {
        /// Target slider.
        setting: StableId,
    },
    /// A catalog range cannot be represented by the renderer slider channel.
    #[error("settings slider {setting} exceeds the renderer integer range")]
    UnrepresentableSliderConstraint {
        /// Target slider.
        setting: StableId,
    },
    /// A draft value failed its compiled setting schema.
    #[error("settings draft {setting} is invalid: {reason}")]
    InvalidDraftValue {
        /// Invalid setting.
        setting: StableId,
        /// Stable schema diagnostic.
        reason: String,
    },
    /// An applied or draft value contradicted the model's catalog proof.
    #[error("settings model value for {setting} contradicts its integer slider schema")]
    InvalidModelValue {
        /// Contradictory setting.
        setting: StableId,
    },
    /// Apply was requested without any changed values.
    #[error("settings draft has no changes")]
    EmptyDraft,
    /// A draft command arrived without an open editing session.
    #[error("settings surface is not editing")]
    NotEditing,
    /// Another input command raced an immutable apply request.
    #[error("settings apply request is awaiting host completion")]
    ApplyPending,
    /// Editing is terminally locked until the host performs a safe restart.
    #[error("settings surface requires a safe process restart before editing")]
    SafeRestartLocked,
    /// A host completion arrived without an apply request.
    #[error("settings apply completion has no pending request")]
    NoPendingApply,
}
