use std::{fmt, str::FromStr};

use latticeaxiom_core::{SchemaId, StableId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

/// Stable ID for the Core 3D prepass semantic slot, major 1.
pub const CORE3D_PREPASS_V1: &str = "latticeaxiom:render-slot/core3d/prepass@1";
/// Stable ID for the Core 3D after-main semantic slot, major 1.
pub const CORE3D_AFTER_MAIN_V1: &str = "latticeaxiom:render-slot/core3d/after-main@1";
/// Stable ID for the Core 3D early-post semantic slot, major 1.
pub const CORE3D_EARLY_POST_V1: &str = "latticeaxiom:render-slot/core3d/early-post@1";
/// Stable ID for the Core 3D before-tonemap semantic slot, major 1.
pub const CORE3D_BEFORE_TONEMAP_V1: &str = "latticeaxiom:render-slot/core3d/before-tonemap@1";
/// Stable ID for the Core 3D after-tonemap semantic slot, major 1.
pub const CORE3D_AFTER_TONEMAP_V1: &str = "latticeaxiom:render-slot/core3d/after-tonemap@1";

/// The complete Core 3D semantic slot catalog for contract major 1.
///
/// These values are portable semantic positions. They are not Bevy schedule,
/// graph-node, or system-set names.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Core3dSlotV1 {
    /// Declared prepass outputs exist; main color has not executed.
    Prepass,
    /// Main 3D color is complete and post-processing has not begun.
    AfterMain,
    /// Deterministic composition point for early post features.
    EarlyPost,
    /// HDR color is available and tonemapping has not executed.
    BeforeTonemap,
    /// Tonemapped LDR color is available; only LDR-compatible work follows.
    AfterTonemap,
}

impl Core3dSlotV1 {
    /// Returns the canonical versioned slot identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepass => CORE3D_PREPASS_V1,
            Self::AfterMain => CORE3D_AFTER_MAIN_V1,
            Self::EarlyPost => CORE3D_EARLY_POST_V1,
            Self::BeforeTonemap => CORE3D_BEFORE_TONEMAP_V1,
            Self::AfterTonemap => CORE3D_AFTER_TONEMAP_V1,
        }
    }

    pub(crate) const fn ordinal(self) -> u8 {
        match self {
            Self::Prepass => 0,
            Self::AfterMain => 1,
            Self::EarlyPost => 2,
            Self::BeforeTonemap => 3,
            Self::AfterTonemap => 4,
        }
    }
}

impl fmt::Display for Core3dSlotV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Core3dSlotV1 {
    type Err = Core3dSlotParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            CORE3D_PREPASS_V1 => Ok(Self::Prepass),
            CORE3D_AFTER_MAIN_V1 => Ok(Self::AfterMain),
            CORE3D_EARLY_POST_V1 => Ok(Self::EarlyPost),
            CORE3D_BEFORE_TONEMAP_V1 => Ok(Self::BeforeTonemap),
            CORE3D_AFTER_TONEMAP_V1 => Ok(Self::AfterTonemap),
            _ => Err(Core3dSlotParseError {
                value: value.to_owned(),
            }),
        }
    }
}

impl Serialize for Core3dSlotV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Core3dSlotV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// Error returned when a stable ID is not in the Core 3D slot catalog v1.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("unknown Core 3D slot v1 `{value}`")]
pub struct Core3dSlotParseError {
    value: String,
}

/// The four declaration kinds have deliberately different responsibilities.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderDeclarationKind {
    /// Typed presentation input understood by an existing renderer mechanism.
    Data,
    /// Additive, independently enabled render behavior owning zero or more passes.
    Feature,
    /// A feature-owned execution node in a semantic slot.
    Pass,
    /// An exactly-one owner of a complete render mechanism.
    Provider,
}

/// A typed presentation input registration that does not establish a pass.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderDataDecl {
    /// Stable identity of the presentation row.
    pub id: StableId,
    /// Stable schema identity interpreted by the host adapter.
    pub schema: SchemaId,
}

/// An additive render feature registration.
///
/// An optional feature may compile to zero passes when its selected fallback is
/// disabled. Pass membership is still explicit when passes are present.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderFeatureDecl {
    /// Stable identity of the feature.
    pub id: StableId,
    /// Whether presentation may remain usable with this feature disabled.
    pub presentation_optional: bool,
}
