//! Settings transaction UI adapter. Apply semantics stay in the registry.

use latticeaxiom_runtime_contracts::{
    RestartImpactMetadata, SettingsApplyTransaction, SettingsTransactionError,
    SettingsTransactionPhase,
};

/// UI request that must be forwarded to one registry transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsSurfaceTransactionRequest {
    /// Prepare the draft.
    Prepare,
    /// Begin a reversible preview.
    BeginPreview,
    /// Disclose impact, then prepare. Persist remains registry-owned.
    Apply,
    /// Cancel, preview timeout, or leave-page rollback.
    Rollback,
    /// Persist through the owning-scope store. The UI never writes the file.
    Persist,
}

/// Impact disclosed by the surface before the host may persist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsApplyDisclosure {
    /// Transaction phase after the UI request.
    pub phase: SettingsTransactionPhase,
    /// Restart and reactivation impact.
    pub restart: RestartImpactMetadata,
}

impl SettingsSurfaceTransactionRequest {
    /// Forwards the UI request onto the registry transaction.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsTransactionError`] from the registry path. Leave-page
    /// and cancel use the same rollback as preview timeout.
    pub fn apply(
        self,
        transaction: &mut SettingsApplyTransaction,
    ) -> Result<SettingsTransactionPhase, SettingsTransactionError> {
        match self {
            Self::Prepare | Self::Apply => transaction.prepare()?,
            Self::BeginPreview => {
                transaction.begin_preview()?;
            }
            Self::Rollback => {
                transaction.rollback_before_persist()?;
            }
            Self::Persist => {
                return Err(SettingsTransactionError::InvalidPhase);
            }
        }
        Ok(transaction.phase())
    }

    /// Forwards the request and returns restart impact for the apply control.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsTransactionError`] from the registry path.
    pub fn disclose(
        self,
        transaction: &mut SettingsApplyTransaction,
    ) -> Result<SettingsApplyDisclosure, SettingsTransactionError> {
        let phase = self.apply(transaction)?;
        Ok(SettingsApplyDisclosure {
            phase,
            restart: transaction.restart_impact(),
        })
    }
}
