//! Understandable world-loading stages and cancellation boundaries.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Ordered loading stage exposed to players and narration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadingStage {
    /// Bounded header, metadata, durability, and checkpoint checks.
    CheckingWorld,
    /// Exact package lock and artifacts are resolved.
    ResolvingPackages,
    /// Artifacts are acquired, built, or mapped.
    BuildingOrLoading,
    /// Registration, semantics, settings, and content are validated.
    ValidatingContent,
    /// The writer is open and spawn data is being activated.
    LoadingSpawn,
    /// Safe bootstrap completed.
    Playing,
}

impl LoadingStage {
    const fn ordinal(self) -> u8 {
        match self {
            Self::CheckingWorld => 0,
            Self::ResolvingPackages => 1,
            Self::BuildingOrLoading => 2,
            Self::ValidatingContent => 3,
            Self::LoadingSpawn => 4,
            Self::Playing => 5,
        }
    }

    /// Human-readable stage label suitable for localization lookup fallback.
    #[must_use]
    pub const fn fallback_label(self) -> &'static str {
        match self {
            Self::CheckingWorld => "Checking world",
            Self::ResolvingPackages => "Resolving packages",
            Self::BuildingOrLoading => "Building or loading",
            Self::ValidatingContent => "Validating content",
            Self::LoadingSpawn => "Loading spawn",
            Self::Playing => "Playing",
        }
    }
}

/// Determinate or indeterminate progress for one loading stage.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum LoadingProgress {
    /// Work has no honest bounded total yet.
    Indeterminate,
    /// Completed and total units are known.
    Determinate {
        /// Completed units.
        completed: u64,
        /// Non-zero total units.
        total: u64,
    },
}

/// Safe response to a player cancellation request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadingCancelDisposition {
    /// Cancel directly because no world writer exists.
    CancelBeforeWriter,
    /// Run the normal durability/shutdown barrier before returning to shell.
    ShutdownBarrierRequired,
    /// Loading has completed and cancellation is no longer meaningful.
    AlreadyPlaying,
}

/// Presentation-neutral loading state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoadingState {
    /// Current understandable stage.
    pub stage: LoadingStage,
    /// Honest stage progress.
    pub progress: LoadingProgress,
    /// Optional current package, artifact, or chunk label.
    pub current_item: Option<String>,
}

impl LoadingState {
    /// Starts the loading state before any writer exists.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stage: LoadingStage::CheckingWorld,
            progress: LoadingProgress::Indeterminate,
            current_item: None,
        }
    }

    /// Advances monotonically and validates determinate progress.
    ///
    /// # Errors
    ///
    /// Returns [`LoadingStateError`] for backward/skipped stages or invalid
    /// determinate progress.
    pub fn advance(
        &mut self,
        stage: LoadingStage,
        progress: LoadingProgress,
        current_item: Option<String>,
    ) -> Result<(), LoadingStateError> {
        let current = self.stage.ordinal();
        let next = stage.ordinal();
        if next < current || next > current.saturating_add(1) {
            return Err(LoadingStateError::InvalidStageTransition {
                from: self.stage,
                to: stage,
            });
        }
        validate_progress(progress)?;
        self.stage = stage;
        self.progress = progress;
        self.current_item = current_item;
        Ok(())
    }

    /// Derives cancellation behavior from the writer boundary.
    #[must_use]
    pub const fn cancel_disposition(&self) -> LoadingCancelDisposition {
        match self.stage {
            LoadingStage::CheckingWorld
            | LoadingStage::ResolvingPackages
            | LoadingStage::BuildingOrLoading
            | LoadingStage::ValidatingContent => LoadingCancelDisposition::CancelBeforeWriter,
            LoadingStage::LoadingSpawn => LoadingCancelDisposition::ShutdownBarrierRequired,
            LoadingStage::Playing => LoadingCancelDisposition::AlreadyPlaying,
        }
    }
}

impl Default for LoadingState {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_progress(progress: LoadingProgress) -> Result<(), LoadingStateError> {
    if let LoadingProgress::Determinate { completed, total } = progress
        && (total == 0 || completed > total)
    {
        return Err(LoadingStateError::InvalidProgress { completed, total });
    }
    Ok(())
}

/// Invalid loading-state update.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LoadingStateError {
    /// Loading stages may only stay in place or advance by one.
    #[error("invalid loading-stage transition from {from:?} to {to:?}")]
    InvalidStageTransition {
        /// Current stage.
        from: LoadingStage,
        /// Requested stage.
        to: LoadingStage,
    },
    /// Determinate progress requires `0 <= completed <= total` and non-zero total.
    #[error("invalid determinate loading progress {completed}/{total}")]
    InvalidProgress {
        /// Completed units.
        completed: u64,
        /// Total units.
        total: u64,
    },
}
