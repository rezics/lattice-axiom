//! Settings rows and apply/preview/rollback transaction surface.

use latticeaxiom_core::{PackageName, StableId};
use latticeaxiom_runtime_contracts::{
    EffectiveSettingSource, EffectiveSettingsSnapshot, RuntimeApplyImpact, SettingScope,
    SettingTransactionRevision, SettingsDiff, SettingsRollbackPlan,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Durability boundary for one honest settings transaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsDurabilityDomain {
    /// Device-local application settings.
    Device,
    /// User-local application settings.
    User,
    /// One authoritative world metadata transaction.
    World,
    /// One player's world-owned transaction.
    PlayerWorld,
    /// Memory-only session state.
    Session,
}

impl From<SettingScope> for SettingsDurabilityDomain {
    fn from(value: SettingScope) -> Self {
        match value {
            SettingScope::Device => Self::Device,
            SettingScope::User => Self::User,
            SettingScope::World => Self::World,
            SettingScope::PlayerWorld => Self::PlayerWorld,
            SettingScope::Session => Self::Session,
        }
    }
}

/// Reversible presentation-preview policy declared outside apply impact.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "policy")]
pub enum PreviewPolicy {
    /// No preview callback is used.
    None,
    /// Preview must roll back after a non-zero timeout unless confirmed.
    Reversible {
        /// Timeout in milliseconds.
        timeout_ms: u32,
    },
}

/// One mechanically generated settings row.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsSurfaceRow {
    /// Stable setting identity.
    pub id: StableId,
    /// Package owning the declaration and callbacks.
    pub owner: PackageName,
    /// Current typed value rendered through the generic vocabulary.
    pub value: String,
    /// Winning scope, or default when absent.
    pub source_scope: Option<SettingScope>,
    /// Runtime impact shown before apply.
    pub impact: RuntimeApplyImpact,
}

/// Stable-ID ordered settings surface built from the effective snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsSurfaceModel {
    rows: Vec<SettingsSurfaceRow>,
}

impl SettingsSurfaceModel {
    /// Builds generic rows without custom package widget trees.
    #[must_use]
    pub fn from_snapshot(snapshot: &EffectiveSettingsSnapshot) -> Self {
        let rows = snapshot
            .values()
            .values()
            .map(|setting| SettingsSurfaceRow {
                id: setting.id.clone(),
                owner: setting.declared_by.clone(),
                value: setting.value.to_string(),
                source_scope: match setting.provenance.winner {
                    EffectiveSettingSource::Default => None,
                    EffectiveSettingSource::Scope { scope, .. } => Some(scope),
                },
                impact: setting.apply_impact,
            })
            .collect();
        Self { rows }
    }

    /// Returns rows in stable setting-ID order.
    #[must_use]
    pub fn rows(&self) -> &[SettingsSurfaceRow] {
        &self.rows
    }
}

/// Current apply transaction phase exposed to the UI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsTransactionPhase {
    /// Typed draft exists but no participant has prepared it.
    Draft,
    /// Participants prepared in stable owner/key order.
    Prepared,
    /// Reversible preview is active and timed.
    Previewing,
    /// Atomic store persistence has been requested.
    Persisting,
    /// Durable value is published; rollback must not retract it.
    Committed,
    /// Pre-persist rollback is being executed.
    RollingBack,
    /// Draft/preview was rolled back completely.
    RolledBack,
    /// Rollback could not be proven; safe process restart is required.
    SafeProcessRestartRequired,
}

/// Command emitted to generated settings participants or store adapter.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsTransactionCommand {
    /// Prepare all changes; callback must have no visible side effect.
    Prepare,
    /// Begin a timed reversible preview.
    BeginPreview {
        /// Non-zero timeout.
        timeout_ms: u32,
    },
    /// Restore prepared values in reverse stable owner/setting order.
    Rollback(SettingsRollbackPlan),
    /// Atomically persist one durability domain.
    Persist {
        /// Strongest runtime impact disclosed before persistence.
        impact: Option<RuntimeApplyImpact>,
    },
    /// Enter recovery shell through a safe process restart.
    SafeProcessRestart,
}

/// Presentation-neutral one-domain settings transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsTransactionSurface {
    domain: SettingsDurabilityDomain,
    diff: SettingsDiff,
    rollback: SettingsRollbackPlan,
    preview: PreviewPolicy,
    phase: SettingsTransactionPhase,
    committed_revision: Option<SettingTransactionRevision>,
}

impl SettingsTransactionSurface {
    /// Creates a draft and its deterministic pre-persist rollback plan.
    #[must_use]
    pub fn new(
        domain: SettingsDurabilityDomain,
        before: &EffectiveSettingsSnapshot,
        proposed: &EffectiveSettingsSnapshot,
        preview: PreviewPolicy,
    ) -> Self {
        let diff = before.diff(proposed);
        let rollback = diff.pre_persist_rollback_plan();
        Self {
            domain,
            diff,
            rollback,
            preview,
            phase: SettingsTransactionPhase::Draft,
            committed_revision: None,
        }
    }

    /// Returns the single durability domain.
    #[must_use]
    pub const fn domain(&self) -> SettingsDurabilityDomain {
        self.domain
    }

    /// Returns the stable complete diff shown before apply.
    #[must_use]
    pub const fn diff(&self) -> &SettingsDiff {
        &self.diff
    }

    /// Returns the current phase.
    #[must_use]
    pub const fn phase(&self) -> SettingsTransactionPhase {
        self.phase
    }

    /// Starts participant preparation.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] if the draft is empty or not in Draft.
    pub fn prepare(&mut self) -> Result<SettingsTransactionCommand, SettingsSurfaceError> {
        if self.phase != SettingsTransactionPhase::Draft {
            return Err(SettingsSurfaceError::InvalidPhase);
        }
        if self.diff.is_empty() {
            return Err(SettingsSurfaceError::EmptyDraft);
        }
        self.phase = SettingsTransactionPhase::Prepared;
        Ok(SettingsTransactionCommand::Prepare)
    }

    /// Begins a declared reversible preview after preparation.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] if no valid reversible policy exists or
    /// the transaction is not prepared.
    pub fn begin_preview(&mut self) -> Result<SettingsTransactionCommand, SettingsSurfaceError> {
        if self.phase != SettingsTransactionPhase::Prepared {
            return Err(SettingsSurfaceError::InvalidPhase);
        }
        let PreviewPolicy::Reversible { timeout_ms } = self.preview else {
            return Err(SettingsSurfaceError::PreviewNotDeclared);
        };
        if timeout_ms == 0 {
            return Err(SettingsSurfaceError::ZeroPreviewTimeout);
        }
        self.phase = SettingsTransactionPhase::Previewing;
        Ok(SettingsTransactionCommand::BeginPreview { timeout_ms })
    }

    /// Cancels preparation or handles preview timeout before persistence.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] after persistence or in another phase.
    pub fn rollback_before_persist(
        &mut self,
    ) -> Result<SettingsTransactionCommand, SettingsSurfaceError> {
        if !matches!(
            self.phase,
            SettingsTransactionPhase::Prepared | SettingsTransactionPhase::Previewing
        ) {
            return Err(SettingsSurfaceError::InvalidPhase);
        }
        self.phase = SettingsTransactionPhase::RollingBack;
        Ok(SettingsTransactionCommand::Rollback(self.rollback.clone()))
    }

    /// Records whether every reverse rollback step completed.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] outside `RollingBack`.
    pub fn finish_rollback(
        &mut self,
        complete: bool,
    ) -> Result<Option<SettingsTransactionCommand>, SettingsSurfaceError> {
        if self.phase != SettingsTransactionPhase::RollingBack {
            return Err(SettingsSurfaceError::InvalidPhase);
        }
        if complete {
            self.phase = SettingsTransactionPhase::RolledBack;
            Ok(None)
        } else {
            self.phase = SettingsTransactionPhase::SafeProcessRestartRequired;
            Ok(Some(SettingsTransactionCommand::SafeProcessRestart))
        }
    }

    /// Requests atomic persistence after prepare or confirmed preview.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] outside a pre-persist prepared state.
    pub fn persist(&mut self) -> Result<SettingsTransactionCommand, SettingsSurfaceError> {
        if !matches!(
            self.phase,
            SettingsTransactionPhase::Prepared | SettingsTransactionPhase::Previewing
        ) {
            return Err(SettingsSurfaceError::InvalidPhase);
        }
        self.phase = SettingsTransactionPhase::Persisting;
        Ok(SettingsTransactionCommand::Persist {
            impact: self.diff.required_impact(),
        })
    }

    /// Records successful store commit; subsequent rollback is forbidden.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsSurfaceError`] outside Persisting.
    pub fn finish_persist(
        &mut self,
        revision: SettingTransactionRevision,
    ) -> Result<(), SettingsSurfaceError> {
        if self.phase != SettingsTransactionPhase::Persisting {
            return Err(SettingsSurfaceError::InvalidPhase);
        }
        self.committed_revision = Some(revision);
        self.phase = SettingsTransactionPhase::Committed;
        Ok(())
    }

    /// Returns the committed runtime-contract revision, if any.
    #[must_use]
    pub const fn committed_revision(&self) -> Option<SettingTransactionRevision> {
        self.committed_revision
    }
}

/// Settings transaction surface failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SettingsSurfaceError {
    /// Operation does not apply to the current phase.
    #[error("settings transaction operation is invalid in the current phase")]
    InvalidPhase,
    /// No changes exist to prepare.
    #[error("settings draft is empty")]
    EmptyDraft,
    /// Preview was requested without a reversible policy.
    #[error("settings preview was not declared reversible")]
    PreviewNotDeclared,
    /// A reversible preview requires a non-zero timeout.
    #[error("settings preview timeout must be non-zero")]
    ZeroPreviewTimeout,
}
