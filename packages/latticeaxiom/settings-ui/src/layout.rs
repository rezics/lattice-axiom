//! Frozen category sections for the official settings page.

use crate::category::SettingsCategoryV1;

/// First-version settings-page section vocabulary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SettingsSectionV1 {
    /// Accessibility vision and contrast.
    AccessibilityVision,
    /// Accessibility hearing and captions.
    AccessibilityHearing,
    /// Accessibility motion and camera effects.
    AccessibilityMotion,
    /// Mouse look and click preferences.
    ControlsMouse,
    /// Rebindable keyboard and mouse actions.
    ControlsKeyboard,
    /// Gamepad look and stick preferences.
    ControlsGamepad,
    /// Package-declared gameplay actions.
    ControlsPackageActions,
    /// Category volume sliders.
    AudioVolumes,
    /// Output device and mix policy.
    AudioDevices,
    /// Window, frame, FOV, and total render distance.
    VideoGeneral,
    /// Visual quality members.
    VideoQuality,
    /// Culling and update policy.
    VideoPerformance,
    /// Compatibility and expert rows. Hidden until Advanced is shown.
    VideoAdvanced,
    /// UI and text scale.
    InterfaceDisplay,
    /// Locale selection.
    InterfaceLanguage,
    /// Target-inspect presentation.
    InterfaceInspect,
    /// Client gameplay convenience.
    GameplayGeneral,
    /// Local world-library policy.
    WorldLibrary,
    /// World-authoritative rules.
    WorldRules,
    /// Catalog operations, not persisted values.
    PackagesCatalog,
    /// Developer overlay and logging.
    DeveloperOverlay,
}

impl SettingsSectionV1 {
    /// Sections in frozen display order.
    pub const ALL: [Self; 21] = [
        Self::AccessibilityVision,
        Self::AccessibilityHearing,
        Self::AccessibilityMotion,
        Self::ControlsMouse,
        Self::ControlsKeyboard,
        Self::ControlsGamepad,
        Self::ControlsPackageActions,
        Self::AudioVolumes,
        Self::AudioDevices,
        Self::VideoGeneral,
        Self::VideoQuality,
        Self::VideoPerformance,
        Self::VideoAdvanced,
        Self::InterfaceDisplay,
        Self::InterfaceLanguage,
        Self::InterfaceInspect,
        Self::GameplayGeneral,
        Self::WorldLibrary,
        Self::WorldRules,
        Self::PackagesCatalog,
        Self::DeveloperOverlay,
    ];

    /// Returns the parent category.
    #[must_use]
    pub const fn category(self) -> SettingsCategoryV1 {
        match self {
            Self::AccessibilityVision | Self::AccessibilityHearing | Self::AccessibilityMotion => {
                SettingsCategoryV1::Accessibility
            }
            Self::ControlsMouse
            | Self::ControlsKeyboard
            | Self::ControlsGamepad
            | Self::ControlsPackageActions => SettingsCategoryV1::Controls,
            Self::AudioVolumes | Self::AudioDevices => SettingsCategoryV1::Audio,
            Self::VideoGeneral
            | Self::VideoQuality
            | Self::VideoPerformance
            | Self::VideoAdvanced => SettingsCategoryV1::Video,
            Self::InterfaceDisplay | Self::InterfaceLanguage | Self::InterfaceInspect => {
                SettingsCategoryV1::Interface
            }
            Self::GameplayGeneral => SettingsCategoryV1::Gameplay,
            Self::WorldLibrary | Self::WorldRules => SettingsCategoryV1::World,
            Self::PackagesCatalog => SettingsCategoryV1::Packages,
            Self::DeveloperOverlay => SettingsCategoryV1::Developer,
        }
    }

    /// Returns whether this section is Advanced and hidden by default.
    #[must_use]
    pub const fn advanced(self) -> bool {
        matches!(self, Self::VideoAdvanced)
    }

    /// Returns the stable section identity.
    #[must_use]
    pub const fn stable_id(self) -> &'static str {
        match self {
            Self::AccessibilityVision => "latticeaxiom:setting-section/accessibility-vision",
            Self::AccessibilityHearing => "latticeaxiom:setting-section/accessibility-hearing",
            Self::AccessibilityMotion => "latticeaxiom:setting-section/accessibility-motion",
            Self::ControlsMouse => "latticeaxiom:setting-section/controls-mouse",
            Self::ControlsKeyboard => "latticeaxiom:setting-section/controls-keyboard",
            Self::ControlsGamepad => "latticeaxiom:setting-section/controls-gamepad",
            Self::ControlsPackageActions => "latticeaxiom:setting-section/controls-package-actions",
            Self::AudioVolumes => "latticeaxiom:setting-section/audio-volumes",
            Self::AudioDevices => "latticeaxiom:setting-section/audio-devices",
            Self::VideoGeneral => "latticeaxiom:setting-section/video-general",
            Self::VideoQuality => "latticeaxiom:setting-section/video-quality",
            Self::VideoPerformance => "latticeaxiom:setting-section/video-performance",
            Self::VideoAdvanced => "latticeaxiom:setting-section/video-advanced",
            Self::InterfaceDisplay => "latticeaxiom:setting-section/interface-display",
            Self::InterfaceLanguage => "latticeaxiom:setting-section/interface-language",
            Self::InterfaceInspect => "latticeaxiom:setting-section/interface-inspect",
            Self::GameplayGeneral => "latticeaxiom:setting-section/gameplay-general",
            Self::WorldLibrary => "latticeaxiom:setting-section/world-library",
            Self::WorldRules => "latticeaxiom:setting-section/world-rules",
            Self::PackagesCatalog => "latticeaxiom:setting-section/packages-catalog",
            Self::DeveloperOverlay => "latticeaxiom:setting-section/developer-overlay",
        }
    }

    /// Returns the localization key.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::AccessibilityVision => "settings.section.accessibility.vision",
            Self::AccessibilityHearing => "settings.section.accessibility.hearing",
            Self::AccessibilityMotion => "settings.section.accessibility.motion",
            Self::ControlsMouse => "settings.section.controls.mouse",
            Self::ControlsKeyboard => "settings.section.controls.keyboard",
            Self::ControlsGamepad => "settings.section.controls.gamepad",
            Self::ControlsPackageActions => "settings.section.controls.package-actions",
            Self::AudioVolumes => "settings.section.audio.volumes",
            Self::AudioDevices => "settings.section.audio.devices",
            Self::VideoGeneral => "settings.section.video.general",
            Self::VideoQuality => "settings.section.video.quality",
            Self::VideoPerformance => "settings.section.video.performance",
            Self::VideoAdvanced => "settings.section.video.advanced",
            Self::InterfaceDisplay => "settings.section.interface.display",
            Self::InterfaceLanguage => "settings.section.interface.language",
            Self::InterfaceInspect => "settings.section.interface.inspect",
            Self::GameplayGeneral => "settings.section.gameplay.general",
            Self::WorldLibrary => "settings.section.world.library",
            Self::WorldRules => "settings.section.world.rules",
            Self::PackagesCatalog => "settings.section.packages.catalog",
            Self::DeveloperOverlay => "settings.section.developer.overlay",
        }
    }

    /// Returns the first section of a category.
    #[must_use]
    pub const fn first_in(category: SettingsCategoryV1) -> Self {
        match category {
            SettingsCategoryV1::Accessibility => Self::AccessibilityVision,
            SettingsCategoryV1::Controls => Self::ControlsMouse,
            SettingsCategoryV1::Audio => Self::AudioVolumes,
            SettingsCategoryV1::Video => Self::VideoGeneral,
            SettingsCategoryV1::Interface => Self::InterfaceDisplay,
            SettingsCategoryV1::Gameplay => Self::GameplayGeneral,
            SettingsCategoryV1::World => Self::WorldLibrary,
            SettingsCategoryV1::Packages => Self::PackagesCatalog,
            SettingsCategoryV1::Developer => Self::DeveloperOverlay,
        }
    }

    /// Returns every section belonging to `category`.
    #[must_use]
    pub fn for_category(category: SettingsCategoryV1) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|section| section.category() == category)
            .collect()
    }
}
