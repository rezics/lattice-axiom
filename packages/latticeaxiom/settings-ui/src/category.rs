//! Fixed first-version settings categories. Packages cannot add arbitrary editors.

use latticeaxiom_core::StableId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// First-version category vocabulary major.
pub const SETTINGS_CATEGORY_VOCABULARY_MAJOR: u32 = 1;

/// Fixed settings-surface category order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsCategoryV1 {
    /// Accessibility.
    Accessibility,
    /// Controls and rebinding.
    Controls,
    /// Audio.
    Audio,
    /// Video.
    Video,
    /// Interface, including UI scale.
    Interface,
    /// Gameplay.
    Gameplay,
    /// World-authoritative rules.
    World,
    /// Packages and profile drafts.
    Packages,
    /// Developer.
    Developer,
}

impl SettingsCategoryV1 {
    /// Categories in the frozen display order.
    pub const ALL: [Self; 9] = [
        Self::Accessibility,
        Self::Controls,
        Self::Audio,
        Self::Video,
        Self::Interface,
        Self::Gameplay,
        Self::World,
        Self::Packages,
        Self::Developer,
    ];

    /// Returns the stable category identity.
    #[must_use]
    pub const fn stable_id(self) -> &'static str {
        match self {
            Self::Accessibility => "latticeaxiom:setting-category/accessibility",
            Self::Controls => "latticeaxiom:setting-category/controls",
            Self::Audio => "latticeaxiom:setting-category/audio",
            Self::Video => "latticeaxiom:setting-category/video",
            Self::Interface => "latticeaxiom:setting-category/interface",
            Self::Gameplay => "latticeaxiom:setting-category/gameplay",
            Self::World => "latticeaxiom:setting-category/world",
            Self::Packages => "latticeaxiom:setting-category/packages",
            Self::Developer => "latticeaxiom:setting-category/developer",
        }
    }

    /// Returns the localization key.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Accessibility => "settings.category.accessibility",
            Self::Controls => "settings.category.controls",
            Self::Audio => "settings.category.audio",
            Self::Video => "settings.category.video",
            Self::Interface => "settings.category.interface",
            Self::Gameplay => "settings.category.gameplay",
            Self::World => "settings.category.world",
            Self::Packages => "settings.category.packages",
            Self::Developer => "settings.category.developer",
        }
    }

    /// Parses a setting-category stable ID.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsCategoryError::UnknownRequired`] for an unknown ID.
    pub fn parse_id(id: &StableId) -> Result<Self, SettingsCategoryError> {
        if id.kind() != "setting-category" {
            return Err(SettingsCategoryError::UnknownRequired {
                category: id.to_string(),
            });
        }
        Self::ALL
            .into_iter()
            .find(|category| category.stable_id().rsplit('/').next() == Some(id.path()))
            .ok_or_else(|| SettingsCategoryError::UnknownRequired {
                category: id.to_string(),
            })
    }
}

/// One published category row from the settings-ui package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsCategoryRecord {
    /// Stable category identity.
    pub id: String,
    /// Display order, matching [`SettingsCategoryV1::ALL`].
    pub order: i32,
    /// Localization key.
    pub label_key: String,
}

/// Package-published category vocabulary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsCategoryDocument {
    /// Document schema identity.
    pub schema: String,
    /// Vocabulary major.
    pub vocabulary_major: u32,
    /// Categories in frozen order.
    pub categories: Vec<SettingsCategoryRecord>,
}

impl SettingsCategoryDocument {
    /// Validates that package data matches the crate vocabulary.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsCategoryError`] on major mismatch, count mismatch, or
    /// identity drift.
    pub fn validate(&self) -> Result<(), SettingsCategoryError> {
        if self.vocabulary_major != SETTINGS_CATEGORY_VOCABULARY_MAJOR {
            return Err(SettingsCategoryError::UnsupportedMajor {
                requested: self.vocabulary_major,
                supported: SETTINGS_CATEGORY_VOCABULARY_MAJOR,
            });
        }
        if self.categories.len() != SettingsCategoryV1::ALL.len() {
            return Err(SettingsCategoryError::CountMismatch {
                observed: self.categories.len(),
                expected: SettingsCategoryV1::ALL.len(),
            });
        }
        for (index, expected) in SettingsCategoryV1::ALL.iter().enumerate() {
            let row = &self.categories[index];
            if row.id != expected.stable_id()
                || row.order != i32::try_from(index).unwrap_or(i32::MAX)
                || row.label_key != expected.label_key()
            {
                return Err(SettingsCategoryError::IdentityDrift {
                    index,
                    observed: row.id.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Invalid category vocabulary.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SettingsCategoryError {
    /// Package vocabulary major is newer than this crate.
    #[error(
        "settings category vocabulary major {requested} is unsupported; crate supports {supported}"
    )]
    UnsupportedMajor {
        /// Requested major.
        requested: u32,
        /// Supported major.
        supported: u32,
    },
    /// A required row named an unknown category.
    #[error("required settings category `{category}` is unknown")]
    UnknownRequired {
        /// Unknown category ID.
        category: String,
    },
    /// Package category count drifted from the crate.
    #[error("settings category count is {observed}; expected {expected}")]
    CountMismatch {
        /// Observed count.
        observed: usize,
        /// Expected count.
        expected: usize,
    },
    /// Package identity drifted from the crate.
    #[error("settings category {index} drifted to `{observed}`")]
    IdentityDrift {
        /// Index in frozen order.
        index: usize,
        /// Observed identity.
        observed: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_order_starts_with_accessibility_and_controls() {
        assert_eq!(
            SettingsCategoryV1::ALL[0],
            SettingsCategoryV1::Accessibility
        );
        assert_eq!(SettingsCategoryV1::ALL[1], SettingsCategoryV1::Controls);
        assert_eq!(SettingsCategoryV1::ALL[8], SettingsCategoryV1::Developer);
    }
}
