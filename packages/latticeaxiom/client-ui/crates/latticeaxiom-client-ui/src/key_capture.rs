//! Binding-capture widget contract. Physical input stays in the input adapter.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::semantic::SemanticKey;
use crate::surface::SurfaceEpoch;

/// Versioned binding candidate that does not serialize Bevy or Leafwing types.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputBindingCandidateV1 {
    /// Candidate schema major.
    pub schema_version: u32,
    /// Canonical encoding owned by the input contract.
    pub encoding: String,
}

/// Occupying action reported for a same-context conflict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BindingConflict {
    /// Occupying action stable ID text.
    pub occupying_action: String,
    /// Conflicting candidate.
    pub candidate: InputBindingCandidateV1,
}

/// Binding-capture phases. Capture is a child of Settings, not a new UI root.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyCapturePhase {
    /// No capture is active.
    Idle,
    /// Exclusive capture of one candidate.
    Capturing,
    /// Same-context conflict must be confirmed or cancelled.
    ConfirmingConflict,
    /// Candidate accepted; apply still belongs to the settings transaction.
    Accepted,
    /// Capture cancelled.
    Cancelled,
    /// Explicit unbind.
    Cleared,
}

/// One Controls-row capture session bound to a surface epoch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeyCaptureSession {
    epoch: SurfaceEpoch,
    row: SemanticKey,
    phase: KeyCapturePhase,
    candidate: Option<InputBindingCandidateV1>,
    conflict: Option<BindingConflict>,
}

impl KeyCaptureSession {
    /// Starts exclusive capture for `row` in `epoch`.
    #[must_use]
    pub const fn begin(epoch: SurfaceEpoch, row: SemanticKey) -> Self {
        Self {
            epoch,
            row,
            phase: KeyCapturePhase::Capturing,
            candidate: None,
            conflict: None,
        }
    }

    /// Returns the owning settings row.
    #[must_use]
    pub const fn row(&self) -> &SemanticKey {
        &self.row
    }

    /// Returns the current phase.
    #[must_use]
    pub const fn phase(&self) -> KeyCapturePhase {
        self.phase
    }

    /// Returns the owning epoch.
    #[must_use]
    pub const fn epoch(&self) -> SurfaceEpoch {
        self.epoch
    }

    /// Receives one candidate. Conflicting encodings enter confirmation.
    ///
    /// # Errors
    ///
    /// Returns [`KeyCaptureError`] outside `Capturing` or when the candidate
    /// schema is unsupported.
    pub fn receive_candidate(
        &mut self,
        candidate: InputBindingCandidateV1,
        conflict: Option<BindingConflict>,
    ) -> Result<KeyCapturePhase, KeyCaptureError> {
        if self.phase != KeyCapturePhase::Capturing {
            return Err(KeyCaptureError::InvalidPhase);
        }
        if candidate.schema_version == 0 {
            return Err(KeyCaptureError::UnsupportedBindingMajor {
                requested: candidate.schema_version,
            });
        }
        self.candidate = Some(candidate);
        if conflict.is_some() {
            self.conflict = conflict;
            self.phase = KeyCapturePhase::ConfirmingConflict;
        } else {
            self.phase = KeyCapturePhase::Accepted;
        }
        Ok(self.phase)
    }

    /// Confirms a conflict replacement.
    ///
    /// # Errors
    ///
    /// Returns [`KeyCaptureError::InvalidPhase`] outside conflict confirmation.
    pub fn confirm_conflict(&mut self) -> Result<KeyCapturePhase, KeyCaptureError> {
        if self.phase != KeyCapturePhase::ConfirmingConflict {
            return Err(KeyCaptureError::InvalidPhase);
        }
        self.phase = KeyCapturePhase::Accepted;
        Ok(self.phase)
    }

    /// Cancels capture without applying a binding.
    ///
    /// # Errors
    ///
    /// Returns [`KeyCaptureError::InvalidPhase`] after accept/clear.
    pub fn cancel(&mut self) -> Result<KeyCapturePhase, KeyCaptureError> {
        if !matches!(
            self.phase,
            KeyCapturePhase::Capturing | KeyCapturePhase::ConfirmingConflict
        ) {
            return Err(KeyCaptureError::InvalidPhase);
        }
        self.candidate = None;
        self.conflict = None;
        self.phase = KeyCapturePhase::Cancelled;
        Ok(self.phase)
    }

    /// Clears the current binding (explicit unbind).
    ///
    /// # Errors
    ///
    /// Returns [`KeyCaptureError::InvalidPhase`] outside capture.
    pub fn clear(&mut self) -> Result<KeyCapturePhase, KeyCaptureError> {
        if self.phase != KeyCapturePhase::Capturing {
            return Err(KeyCaptureError::InvalidPhase);
        }
        self.candidate = None;
        self.conflict = None;
        self.phase = KeyCapturePhase::Cleared;
        Ok(self.phase)
    }
}

/// Binding-capture failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum KeyCaptureError {
    /// Operation does not apply to the current phase.
    #[error("key capture operation is invalid in the current phase")]
    InvalidPhase,
    /// Binding schema major is not supported.
    #[error("input binding schema version {requested} is unsupported")]
    UnsupportedBindingMajor {
        /// Requested schema version.
        requested: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::SemanticKey;
    use crate::surface::SurfaceEpoch;

    #[test]
    fn cancel_and_conflict_do_not_half_apply() {
        let row = SemanticKey::new("settings/controls/pause").expect("row");
        let mut session = KeyCaptureSession::begin(SurfaceEpoch::FIRST, row);
        let candidate = InputBindingCandidateV1 {
            schema_version: 1,
            encoding: "keyboard:escape".to_owned(),
        };
        session
            .receive_candidate(
                candidate.clone(),
                Some(BindingConflict {
                    occupying_action: "latticeaxiom:action/ui/back@1".to_owned(),
                    candidate,
                }),
            )
            .expect("conflict");
        assert_eq!(session.phase(), KeyCapturePhase::ConfirmingConflict);
        assert_eq!(session.cancel(), Ok(KeyCapturePhase::Cancelled));
        assert_eq!(session.phase(), KeyCapturePhase::Cancelled);
    }
}
