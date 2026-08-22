//! Versioned input-binding DTOs and user binding-profile overlays.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{CanonicalJsonError, StableId, canonical_json_bytes};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use thiserror::Error;
use unicode_normalization::is_nfc;

/// Schema version for [`BindingProfileV1`].
pub const BINDING_PROFILE_SCHEMA_VERSION: u32 = 1;

/// One physical binding that never serializes Bevy, Leafwing, or winit types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputBindingV1 {
    /// A well-known V1 binding.
    Known(KnownInputBindingV1),
    /// A newer-minor binding retained for diagnosable round-trip.
    Unknown(UnknownInputBindingV1),
}

/// Closed V1 vocabulary for keyboard, mouse, and standard gamepad bindings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum KnownInputBindingV1 {
    /// A physical keyboard usage with an explicit modifier set.
    Keyboard {
        /// Physical key usage such as `KeyE` or `Escape`; not an IME character.
        usage: String,
        /// Explicit modifier set; text editing does not use this binding.
        modifiers: KeyboardModifiersV1,
    },
    /// A standard mouse button.
    MouseButton {
        /// Button identity.
        button: MouseButtonV1,
    },
    /// Unparameterized mouse motion.
    MouseMotion,
    /// A mouse-wheel axis.
    MouseWheel {
        /// Wheel axis.
        axis: MouseWheelAxisV1,
    },
    /// A standard gamepad button.
    GamepadButton {
        /// Button identity.
        button: GamepadButtonV1,
    },
    /// A standard gamepad stick component with bounded deadzone and sensitivity.
    GamepadStick {
        /// Stick identity.
        stick: GamepadStickV1,
        /// Stick component compared for conflict detection.
        axis: GamepadStickAxisV1,
        /// Inclusive `0..=1` linear deadzone.
        deadzone: Number,
        /// Positive finite sensitivity multiplier.
        sensitivity: Number,
    },
}

/// Newer-minor binding retained verbatim until a compatible owner exists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnknownInputBindingV1 {
    /// Unknown kind token.
    pub kind: String,
    /// Remaining canonical fields.
    pub fields: BTreeMap<String, Value>,
}

/// Explicit keyboard modifiers; absence means the modifier is not required.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "ADR 0033 freezes an explicit four-modifier set rather than a packed mask"
)]
pub struct KeyboardModifiersV1 {
    /// Left or right Shift.
    #[serde(default)]
    pub shift: bool,
    /// Left or right Control.
    #[serde(default)]
    pub control: bool,
    /// Left or right Alt.
    #[serde(default)]
    pub alt: bool,
    /// Left or right Super/Meta.
    #[serde(default, rename = "super")]
    pub super_key: bool,
}

/// Standard mouse buttons.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseButtonV1 {
    /// Primary button.
    Left,
    /// Secondary button.
    Right,
    /// Middle / wheel button.
    Middle,
    /// Extra mouse button identified by a stable index.
    Extra(u16),
}

/// Mouse-wheel axes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseWheelAxisV1 {
    /// Vertical wheel.
    Vertical,
    /// Horizontal wheel.
    Horizontal,
}

/// Standard gamepad buttons.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GamepadButtonV1 {
    /// Bottom face button.
    South,
    /// Right face button.
    East,
    /// West face button.
    West,
    /// Top face button.
    North,
    /// Start / menu.
    Start,
    /// Select / back / view.
    Select,
    /// D-pad up.
    DPadUp,
    /// D-pad down.
    DPadDown,
    /// D-pad left.
    DPadLeft,
    /// D-pad right.
    DPadRight,
    /// Left shoulder.
    LeftShoulder,
    /// Right shoulder.
    RightShoulder,
    /// Left trigger as a button.
    LeftTrigger,
    /// Right trigger as a button.
    RightTrigger,
}

/// Standard gamepad sticks.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GamepadStickV1 {
    /// Left stick.
    Left,
    /// Right stick.
    Right,
}

/// Stick component used for conflict comparison.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GamepadStickAxisV1 {
    /// Horizontal component.
    X,
    /// Vertical component.
    Y,
    /// Combined magnitude.
    Magnitude,
}

/// User overrides over package default bindings.
///
/// A missing action inherits the package default. An empty list is an explicit
/// unbind. Unknown action IDs are retained so a temporarily missing package
/// cannot delete user data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BindingProfileV1 {
    schema_version: u32,
    overrides: BTreeMap<StableId, Vec<InputBindingV1>>,
}

impl Default for BindingProfileV1 {
    fn default() -> Self {
        Self::empty()
    }
}

/// Defaults overlaid with a user profile, plus inactive unknown actions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveBindingsV1 {
    bindings: BTreeMap<StableId, Vec<InputBindingV1>>,
    orphans: BTreeMap<StableId, Vec<InputBindingV1>>,
}

/// Same-context occupancy of one normalized button-like binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingConflictV1 {
    context: String,
    occupants: Vec<StableId>,
}

/// Input-binding decode or constraint failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum InputBindingError {
    /// The JSON value was not an object with a `kind` field.
    #[error("input binding must be a tagged object with a kind")]
    MissingKind,
    /// A known kind was malformed rather than newer-minor unknown.
    #[error("input binding `{kind}` is malformed: {reason}")]
    MalformedKnown {
        /// Known kind that failed.
        kind: String,
        /// Decoder diagnostic.
        reason: String,
    },
    /// An unknown kind was empty or not Unicode NFC.
    #[error("input binding kind is not canonical Unicode NFC")]
    NonCanonicalKind,
    /// A keyboard usage was empty or not Unicode NFC.
    #[error("keyboard usage is empty or not Unicode NFC")]
    InvalidKeyboardUsage,
    /// Deadzone or sensitivity violated the closed numeric range.
    #[error("gamepad stick numeric constraint failed: {reason}")]
    InvalidStickNumber {
        /// Constraint diagnostic.
        reason: &'static str,
    },
}

impl InputBindingV1 {
    /// Decodes one binding from canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns [`InputBindingError`] when the value is not a tagged object, a
    /// known kind is malformed, or a constraint fails.
    pub fn from_json(value: &Value) -> Result<Self, InputBindingError> {
        let object = value.as_object().ok_or(InputBindingError::MissingKind)?;
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .ok_or(InputBindingError::MissingKind)?;
        match kind {
            "keyboard" | "mouse-button" | "mouse-motion" | "mouse-wheel" | "gamepad-button"
            | "gamepad-stick" => {
                let known = KnownInputBindingV1::deserialize(value.clone()).map_err(|source| {
                    InputBindingError::MalformedKnown {
                        kind: kind.to_owned(),
                        reason: source.to_string(),
                    }
                })?;
                validate_known(&known)?;
                Ok(Self::Known(known))
            }
            other => {
                if other.is_empty() || !is_nfc(other) {
                    return Err(InputBindingError::NonCanonicalKind);
                }
                let mut fields = BTreeMap::new();
                for (key, field) in object {
                    if key != "kind" {
                        fields.insert(key.clone(), field.clone());
                    }
                }
                Ok(Self::Unknown(UnknownInputBindingV1 {
                    kind: other.to_owned(),
                    fields,
                }))
            }
        }
    }
}

impl Serialize for InputBindingV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Known(known) => known.serialize(serializer),
            Self::Unknown(unknown) => {
                let mut object = Map::new();
                object.insert("kind".to_owned(), Value::String(unknown.kind.clone()));
                for (key, value) in &unknown.fields {
                    object.insert(key.clone(), value.clone());
                }
                Value::Object(object).serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for InputBindingV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::from_json(&value).map_err(serde::de::Error::custom)
    }
}

impl BindingProfileV1 {
    /// Creates an empty profile that inherits every package default.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: BINDING_PROFILE_SCHEMA_VERSION,
            overrides: BTreeMap::new(),
        }
    }

    /// Creates a profile from ordered user overrides.
    ///
    /// # Errors
    ///
    /// Returns [`InputBindingError`] when `schema_version` is not the supported
    /// V1 value.
    pub fn new(
        schema_version: u32,
        overrides: BTreeMap<StableId, Vec<InputBindingV1>>,
    ) -> Result<Self, InputBindingError> {
        if schema_version != BINDING_PROFILE_SCHEMA_VERSION {
            return Err(InputBindingError::MalformedKnown {
                kind: "binding-profile".to_owned(),
                reason: format!("unsupported schema version {schema_version}"),
            });
        }
        Ok(Self {
            schema_version,
            overrides,
        })
    }

    /// Returns the profile schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns user overrides in stable action-ID order.
    #[must_use]
    pub const fn overrides(&self) -> &BTreeMap<StableId, Vec<InputBindingV1>> {
        &self.overrides
    }

    /// Overlays package defaults without deleting unknown user actions.
    #[must_use]
    pub fn overlay_defaults(
        &self,
        defaults: &BTreeMap<StableId, Vec<InputBindingV1>>,
    ) -> EffectiveBindingsV1 {
        let mut bindings = defaults.clone();
        let mut orphans = BTreeMap::new();
        for (action, override_bindings) in &self.overrides {
            if defaults.contains_key(action) {
                bindings.insert(action.clone(), override_bindings.clone());
            } else {
                orphans.insert(action.clone(), override_bindings.clone());
            }
        }
        EffectiveBindingsV1 { bindings, orphans }
    }

    /// Encodes the profile as recursively key-sorted compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }
}

impl EffectiveBindingsV1 {
    /// Returns effective bindings, including explicit empty unbinds.
    #[must_use]
    pub const fn bindings(&self) -> &BTreeMap<StableId, Vec<InputBindingV1>> {
        &self.bindings
    }

    /// Returns unknown action overrides retained for a future owner.
    #[must_use]
    pub const fn orphans(&self) -> &BTreeMap<StableId, Vec<InputBindingV1>> {
        &self.orphans
    }
}

impl BindingConflictV1 {
    /// Returns the shared input context identity.
    #[must_use]
    pub fn context(&self) -> &str {
        &self.context
    }

    /// Returns occupying action IDs in stable order.
    #[must_use]
    pub fn occupants(&self) -> &[StableId] {
        &self.occupants
    }
}

/// Detects same-context occupancy of identical button-like bindings.
///
/// Axis/stick conflicts compare stick, component, and modifier-free identity.
/// Unknown newer-minor bindings are ignored because they cannot be normalized.
#[must_use]
pub fn detect_binding_conflicts(
    bindings: &BTreeMap<StableId, Vec<InputBindingV1>>,
    contexts: &BTreeMap<StableId, String>,
) -> Vec<BindingConflictV1> {
    let mut occupancy = BTreeMap::<(String, BindingIdentity), BTreeSet<StableId>>::new();
    for (action, action_bindings) in bindings {
        let Some(context) = contexts.get(action) else {
            continue;
        };
        for binding in action_bindings {
            if let Some(identity) = BindingIdentity::from_binding(binding) {
                occupancy
                    .entry((context.clone(), identity))
                    .or_default()
                    .insert(action.clone());
            }
        }
    }
    occupancy
        .into_iter()
        .filter_map(|((context, _), occupants)| {
            (occupants.len() > 1).then_some(BindingConflictV1 {
                context,
                occupants: occupants.into_iter().collect(),
            })
        })
        .collect()
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum BindingIdentity {
    Keyboard {
        usage: String,
        shift: bool,
        control: bool,
        alt: bool,
        super_key: bool,
    },
    MouseButton {
        button: MouseButtonV1,
    },
    MouseMotion,
    MouseWheel {
        axis: MouseWheelAxisV1,
    },
    GamepadButton {
        button: GamepadButtonV1,
    },
    GamepadStick {
        stick: GamepadStickV1,
        axis: GamepadStickAxisV1,
    },
}

impl BindingIdentity {
    fn from_binding(binding: &InputBindingV1) -> Option<Self> {
        match binding {
            InputBindingV1::Unknown(_) => None,
            InputBindingV1::Known(KnownInputBindingV1::Keyboard { usage, modifiers }) => {
                Some(Self::Keyboard {
                    usage: usage.clone(),
                    shift: modifiers.shift,
                    control: modifiers.control,
                    alt: modifiers.alt,
                    super_key: modifiers.super_key,
                })
            }
            InputBindingV1::Known(KnownInputBindingV1::MouseButton { button }) => {
                Some(Self::MouseButton { button: *button })
            }
            InputBindingV1::Known(KnownInputBindingV1::MouseMotion) => Some(Self::MouseMotion),
            InputBindingV1::Known(KnownInputBindingV1::MouseWheel { axis }) => {
                Some(Self::MouseWheel { axis: *axis })
            }
            InputBindingV1::Known(KnownInputBindingV1::GamepadButton { button }) => {
                Some(Self::GamepadButton { button: *button })
            }
            InputBindingV1::Known(KnownInputBindingV1::GamepadStick { stick, axis, .. }) => {
                Some(Self::GamepadStick {
                    stick: *stick,
                    axis: *axis,
                })
            }
        }
    }
}

fn validate_known(binding: &KnownInputBindingV1) -> Result<(), InputBindingError> {
    match binding {
        KnownInputBindingV1::Keyboard { usage, .. } => {
            if usage.is_empty() || !is_nfc(usage) {
                Err(InputBindingError::InvalidKeyboardUsage)
            } else {
                Ok(())
            }
        }
        KnownInputBindingV1::GamepadStick {
            deadzone,
            sensitivity,
            ..
        } => {
            validate_unit_interval(deadzone, "deadzone must lie in the inclusive range 0..1")?;
            if !positive_number(sensitivity) {
                return Err(InputBindingError::InvalidStickNumber {
                    reason: "sensitivity must be a positive finite decimal",
                });
            }
            Ok(())
        }
        KnownInputBindingV1::MouseButton { .. }
        | KnownInputBindingV1::MouseMotion
        | KnownInputBindingV1::MouseWheel { .. }
        | KnownInputBindingV1::GamepadButton { .. } => Ok(()),
    }
}

fn validate_unit_interval(number: &Number, reason: &'static str) -> Result<(), InputBindingError> {
    let Some(value) = number.as_f64() else {
        return Err(InputBindingError::InvalidStickNumber { reason });
    };
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(InputBindingError::InvalidStickNumber { reason })
    }
}

fn positive_number(number: &Number) -> bool {
    number.as_f64().is_some_and(|value| value > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(value: &str) -> StableId {
        match value.parse() {
            Ok(value) => value,
            Err(error) => panic!("invalid action fixture `{value}`: {error}"),
        }
    }

    fn keyboard(usage: &str) -> InputBindingV1 {
        InputBindingV1::Known(KnownInputBindingV1::Keyboard {
            usage: usage.to_owned(),
            modifiers: KeyboardModifiersV1::default(),
        })
    }

    #[test]
    fn missing_override_inherits_and_empty_list_unbinds() {
        let jump = action("latticeaxiom:action/gameplay/jump@1");
        let pause = action("latticeaxiom:action/gameplay/pause@1");
        let defaults = BTreeMap::from([
            (jump.clone(), vec![keyboard("Space")]),
            (pause.clone(), vec![keyboard("Escape")]),
        ]);
        let profile = match BindingProfileV1::new(
            BINDING_PROFILE_SCHEMA_VERSION,
            BTreeMap::from([(pause.clone(), Vec::new())]),
        ) {
            Ok(value) => value,
            Err(error) => panic!("valid profile fixture failed: {error}"),
        };
        let effective = profile.overlay_defaults(&defaults);
        assert_eq!(
            effective.bindings().get(&jump),
            Some(&vec![keyboard("Space")])
        );
        assert_eq!(effective.bindings().get(&pause), Some(&Vec::new()));
        assert!(effective.orphans().is_empty());
    }

    #[test]
    fn unknown_action_overrides_are_retained() {
        let known = action("latticeaxiom:action/gameplay/pause@1");
        let missing = action("latticeaxiom:action/gameplay/sprint@1");
        let defaults = BTreeMap::from([(known, vec![keyboard("Escape")])]);
        let profile = match BindingProfileV1::new(
            BINDING_PROFILE_SCHEMA_VERSION,
            BTreeMap::from([(missing.clone(), vec![keyboard("KeyG")])]),
        ) {
            Ok(value) => value,
            Err(error) => panic!("valid profile fixture failed: {error}"),
        };
        let effective = profile.overlay_defaults(&defaults);
        assert_eq!(
            effective.orphans().get(&missing),
            Some(&vec![keyboard("KeyG")])
        );
    }

    #[test]
    fn same_context_conflicts_are_stable_sorted() {
        let pause = action("latticeaxiom:action/gameplay/pause@1");
        let back = action("latticeaxiom:action/ui/back@1");
        let inventory = action("latticeaxiom:action/hud/toggle-inventory@1");
        let bindings = BTreeMap::from([
            (inventory.clone(), vec![keyboard("Escape")]),
            (back.clone(), vec![keyboard("Escape")]),
            (pause.clone(), vec![keyboard("Escape")]),
        ]);
        let contexts = BTreeMap::from([
            (pause, "gameplay".to_owned()),
            (back, "surface".to_owned()),
            (inventory.clone(), "gameplay".to_owned()),
        ]);
        let conflicts = detect_binding_conflicts(&bindings, &contexts);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].context(), "gameplay");
        assert_eq!(
            conflicts[0]
                .occupants()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec![
                "latticeaxiom:action/gameplay/pause@1".to_owned(),
                "latticeaxiom:action/hud/toggle-inventory@1".to_owned()
            ]
        );
    }

    #[test]
    fn unknown_newer_minor_bindings_round_trip() {
        let json = serde_json::json!({
            "kind": "touch-gesture",
            "gesture": "pinch",
            "schema_minor": 2
        });
        let binding = match InputBindingV1::from_json(&json) {
            Ok(value) => value,
            Err(error) => panic!("unknown binding fixture failed: {error}"),
        };
        let encoded = match serde_json::to_value(&binding) {
            Ok(value) => value,
            Err(error) => panic!("unknown binding encode failed: {error}"),
        };
        assert_eq!(encoded, json);
    }

    #[test]
    fn malformed_known_kind_does_not_become_unknown() {
        let json = serde_json::json!({ "kind": "keyboard" });
        assert!(matches!(
            InputBindingV1::from_json(&json),
            Err(InputBindingError::MalformedKnown { kind, .. }) if kind == "keyboard"
        ));
    }
}
