//! Versioned control vocabulary mapped from typed setting specs.

use latticeaxiom_client_ui::{CONTROL_VOCABULARY_MAJOR, WidgetKind};
use latticeaxiom_runtime_contracts::ValueType;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// First-version settings-control vocabulary major.
pub const SETTINGS_CONTROL_VOCABULARY_MAJOR: u32 = 1;

/// Mechanical widget chosen for one setting row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsControlKind {
    /// Boolean toggle.
    Toggle,
    /// Bounded integer slider.
    IntegerSlider,
    /// Bounded float slider.
    FloatSlider,
    /// Enum cycle.
    EnumCycle,
    /// UTF-8 text, including IME.
    Text,
    /// Local path. Optional in v1.
    Path,
    /// Linear RGBA color.
    Color,
    /// Key-binding capture.
    KeyBinding,
    /// Read-only value.
    ReadOnly,
    /// Command row.
    Command,
}

impl SettingsControlKind {
    /// Returns the package vocabulary identity.
    #[must_use]
    pub const fn vocabulary_id(self) -> &'static str {
        match self {
            Self::Toggle => "latticeaxiom:ui-control/toggle@1",
            Self::IntegerSlider => "latticeaxiom:ui-control/integer-slider@1",
            Self::FloatSlider => "latticeaxiom:ui-control/float-slider@1",
            Self::EnumCycle => "latticeaxiom:ui-control/enum-cycle@1",
            Self::Text => "latticeaxiom:ui-control/text@1",
            Self::Path => "latticeaxiom:ui-control/path@1",
            Self::Color => "latticeaxiom:ui-control/color@1",
            Self::KeyBinding => "latticeaxiom:ui-control/key-binding@1",
            Self::ReadOnly => "latticeaxiom:ui-control/read-only@1",
            Self::Command => "latticeaxiom:ui-control/command@1",
        }
    }

    /// Returns whether this control is required to construct the v1 surface.
    #[must_use]
    pub const fn required(self) -> bool {
        !matches!(self, Self::Path)
    }

    /// Returns the shared widget kind used by the client UI crate.
    #[must_use]
    pub const fn widget_kind(self) -> WidgetKind {
        match self {
            Self::Toggle => WidgetKind::Toggle,
            Self::IntegerSlider | Self::FloatSlider => WidgetKind::Slider,
            Self::EnumCycle => WidgetKind::Cycle,
            Self::Text | Self::Path => WidgetKind::Text,
            Self::Color | Self::ReadOnly => WidgetKind::ReadOnly,
            Self::KeyBinding => WidgetKind::KeyBinding,
            Self::Command => WidgetKind::Command,
        }
    }

    /// Maps a finite value schema onto the control vocabulary.
    #[must_use]
    pub const fn from_value_type(value_type: &ValueType) -> Self {
        match value_type {
            ValueType::Bool => Self::Toggle,
            ValueType::Integer { .. } => Self::IntegerSlider,
            ValueType::Number { .. } => Self::FloatSlider,
            ValueType::String { .. } => Self::Text,
            ValueType::Enum { .. } => Self::EnumCycle,
            ValueType::Color => Self::Color,
            ValueType::KeyBinding => Self::KeyBinding,
        }
    }
}

/// One published control-vocabulary row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsControlRecord {
    /// Vocabulary identity.
    pub id: String,
    /// Whether surface construction fails closed without this control.
    pub required: bool,
}

/// Package-published control vocabulary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsControlDocument {
    /// Document schema identity.
    pub schema: String,
    /// Vocabulary major.
    pub vocabulary_major: u32,
    /// Controls in stable identity order.
    pub controls: Vec<SettingsControlRecord>,
}

impl SettingsControlDocument {
    /// Validates package data against this crate.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsControlError`] when the major is unsupported or a
    /// required identity drifted.
    pub fn validate(&self) -> Result<(), SettingsControlError> {
        if self.vocabulary_major > SETTINGS_CONTROL_VOCABULARY_MAJOR
            || self.vocabulary_major > CONTROL_VOCABULARY_MAJOR
        {
            return Err(SettingsControlError::UnsupportedRequiredMajor {
                requested: self.vocabulary_major,
                supported: SETTINGS_CONTROL_VOCABULARY_MAJOR,
            });
        }
        let expected = [
            SettingsControlKind::Toggle,
            SettingsControlKind::IntegerSlider,
            SettingsControlKind::FloatSlider,
            SettingsControlKind::EnumCycle,
            SettingsControlKind::Text,
            SettingsControlKind::Path,
            SettingsControlKind::Color,
            SettingsControlKind::KeyBinding,
            SettingsControlKind::ReadOnly,
            SettingsControlKind::Command,
        ];
        if self.controls.len() != expected.len() {
            return Err(SettingsControlError::CountMismatch {
                observed: self.controls.len(),
                expected: expected.len(),
            });
        }
        for (index, kind) in expected.into_iter().enumerate() {
            let row = &self.controls[index];
            if row.id != kind.vocabulary_id() || row.required != kind.required() {
                return Err(SettingsControlError::IdentityDrift {
                    observed: row.id.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Invalid control vocabulary.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SettingsControlError {
    /// A required control major is newer than this crate.
    #[error(
        "required settings control vocabulary major {requested} is unsupported; crate supports {supported}"
    )]
    UnsupportedRequiredMajor {
        /// Requested major.
        requested: u32,
        /// Supported major.
        supported: u32,
    },
    /// Package control count drifted.
    #[error("settings control count is {observed}; expected {expected}")]
    CountMismatch {
        /// Observed count.
        observed: usize,
        /// Expected count.
        expected: usize,
    },
    /// Package identity drifted.
    #[error("settings control identity drifted to `{observed}`")]
    IdentityDrift {
        /// Observed identity.
        observed: String,
    },
}
