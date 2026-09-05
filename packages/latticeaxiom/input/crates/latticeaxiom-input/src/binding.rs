//! Versioned physical bindings that never serialize Bevy or Leafwing types.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{ActionKindV1, InputError, ids};

/// Explicit modifier set. Shift/Control/Alt/Super are canonical names.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum KeyModifierV1 {
    /// Alt / Option.
    Alt,
    /// Control.
    Control,
    /// Shift.
    Shift,
    /// Super / Meta / Windows.
    Super,
}

/// Standard mouse buttons.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum MouseButtonV1 {
    /// Primary button.
    Left,
    /// Secondary button.
    Right,
    /// Wheel button.
    Middle,
    /// Browser-back button.
    Back,
    /// Browser-forward button.
    Forward,
}

/// Mouse wheel axis.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum MouseWheelAxisV1 {
    /// Horizontal wheel.
    X,
    /// Vertical wheel.
    Y,
}

/// Button-like mouse-wheel direction.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum MouseWheelDirectionV1 {
    /// Positive vertical wheel movement.
    Up,
    /// Negative vertical wheel movement.
    Down,
    /// Negative horizontal wheel movement.
    Left,
    /// Positive horizontal wheel movement.
    Right,
}

/// Standard gamepad stick.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GamepadStickV1 {
    /// Left analog stick.
    Left,
    /// Right analog stick.
    Right,
}

/// Project-owned version-one physical binding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Value", into = "Value")]
pub enum InputBindingV1 {
    /// Physical keyboard usage plus modifiers.
    Keyboard {
        /// Bevy `KeyCode`-shaped usage such as `KeyE` or `ArrowUp`.
        usage: String,
        /// Canonical modifier set.
        modifiers: BTreeSet<KeyModifierV1>,
    },
    /// Four-key virtual d-pad used by walk axes.
    KeyboardVirtualDPad {
        /// Forward key.
        up: String,
        /// Backward key.
        down: String,
        /// Left key.
        left: String,
        /// Right key.
        right: String,
        /// Canonical modifier set applied to every direction.
        modifiers: BTreeSet<KeyModifierV1>,
    },
    /// Standard mouse button.
    MouseButton {
        /// Button identity.
        button: MouseButtonV1,
    },
    /// Mouse motion used as a look axis.
    MouseMotion {
        /// Radians per pixel, or adapter-defined axis scale.
        sensitivity: f32,
    },
    /// Mouse wheel axis.
    MouseWheel {
        /// Wheel axis.
        axis: MouseWheelAxisV1,
    },
    /// One button-like mouse-wheel direction.
    MouseWheelDirection {
        /// Direction that produces a rising button edge.
        direction: MouseWheelDirectionV1,
    },
    /// Standard gamepad button, including digital triggers.
    GamepadButton {
        /// Standard mapping name such as `South` or `RightTrigger`.
        button: String,
    },
    /// Standard analog stick.
    GamepadStick {
        /// Left or right stick.
        stick: GamepadStickV1,
        /// Circle deadzone in `0..=1`.
        deadzone: f32,
        /// Finite sensitivity scale.
        sensitivity: f32,
    },
    /// Newer-minor or unknown kind retained for round-trip.
    Unknown {
        /// Declared schema identifier.
        schema: String,
        /// Declared kind tag.
        kind: String,
        /// Remaining canonical JSON object fields.
        payload: Map<String, Value>,
    },
}

impl InputBindingV1 {
    /// Returns whether this binding is a keyboard or mouse override target.
    #[must_use]
    pub const fn is_keyboard_mouse(&self) -> bool {
        matches!(
            self,
            Self::Keyboard { .. }
                | Self::KeyboardVirtualDPad { .. }
                | Self::MouseButton { .. }
                | Self::MouseMotion { .. }
                | Self::MouseWheel { .. }
                | Self::MouseWheelDirection { .. }
        )
    }

    /// Returns whether this binding is a first-version gamepad mapping.
    #[must_use]
    pub const fn is_gamepad(&self) -> bool {
        matches!(self, Self::GamepadButton { .. } | Self::GamepadStick { .. })
    }

    pub(crate) fn validate_for_kind(&self, kind: ActionKindV1) -> Result<(), InputError> {
        match (kind, self) {
            (ActionKindV1::Button, Self::Keyboard { usage, .. }) => validate_usage(usage),
            (ActionKindV1::Button, Self::MouseButton { .. } | Self::MouseWheelDirection { .. })
            | (ActionKindV1::Axis2, Self::MouseWheel { .. }) => Ok(()),
            (ActionKindV1::Button, Self::GamepadButton { button }) => validate_usage(button),
            (
                ActionKindV1::Axis2,
                Self::KeyboardVirtualDPad {
                    up,
                    down,
                    left,
                    right,
                    ..
                },
            ) => {
                validate_usage(up)?;
                validate_usage(down)?;
                validate_usage(left)?;
                validate_usage(right)
            }
            (ActionKindV1::Axis2, Self::MouseMotion { sensitivity }) => {
                validate_sensitivity(*sensitivity)
            }
            (
                ActionKindV1::Axis2,
                Self::GamepadStick {
                    deadzone,
                    sensitivity,
                    ..
                },
            ) => {
                validate_deadzone(*deadzone)?;
                validate_sensitivity(*sensitivity)
            }
            (_, Self::Unknown { schema, .. }) => Err(InputError::UnknownRequiredMajor {
                schema: schema.clone(),
                reason: "catalog defaults cannot include unknown binding kinds",
            }),
            (ActionKindV1::Button, _) => Err(InputError::InvalidBinding {
                reason: "button actions cannot use axis bindings".to_owned(),
            }),
            (ActionKindV1::Axis2, _) => Err(InputError::InvalidBinding {
                reason: "axis2 actions cannot use button bindings".to_owned(),
            }),
        }
    }

    pub(crate) fn occupancy_tokens(&self) -> Vec<OccupancyTokenV1> {
        match self {
            Self::Keyboard { usage, modifiers } => vec![OccupancyTokenV1::Button {
                code: format!("keyboard:{usage}"),
                modifiers: modifiers.iter().copied().collect(),
            }],
            Self::KeyboardVirtualDPad {
                up,
                down,
                left,
                right,
                modifiers,
            } => [up, down, left, right]
                .into_iter()
                .map(|usage| OccupancyTokenV1::Button {
                    code: format!("keyboard:{usage}"),
                    modifiers: modifiers.iter().copied().collect(),
                })
                .collect(),
            Self::MouseButton { button } => vec![OccupancyTokenV1::Button {
                code: format!("mouse:{button:?}"),
                modifiers: Vec::new(),
            }],
            Self::MouseMotion { .. } => vec![OccupancyTokenV1::Axis {
                source: "mouse-motion".to_owned(),
                component: "vector".to_owned(),
                direction: "bidirectional".to_owned(),
                deadzone_millis: 0,
                modifiers: Vec::new(),
            }],
            Self::MouseWheel { axis } => vec![OccupancyTokenV1::Axis {
                source: "mouse-wheel".to_owned(),
                component: format!("{axis:?}"),
                direction: "bidirectional".to_owned(),
                deadzone_millis: 0,
                modifiers: Vec::new(),
            }],
            Self::MouseWheelDirection { direction } => vec![OccupancyTokenV1::Button {
                code: format!("mouse-wheel:{direction:?}"),
                modifiers: Vec::new(),
            }],
            Self::GamepadButton { button } => vec![OccupancyTokenV1::Button {
                code: format!("gamepad:{button}"),
                modifiers: Vec::new(),
            }],
            Self::GamepadStick {
                stick, deadzone, ..
            } => vec![OccupancyTokenV1::Axis {
                source: format!("gamepad-stick:{stick:?}"),
                component: "vector".to_owned(),
                direction: "bidirectional".to_owned(),
                deadzone_millis: float_millis(*deadzone),
                modifiers: Vec::new(),
            }],
            Self::Unknown { .. } => Vec::new(),
        }
    }
}

/// Normalized occupancy token used for same-context conflict detection.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OccupancyTokenV1 {
    /// Button-like occupancy.
    Button {
        /// Device-qualified button code.
        code: String,
        /// Canonical modifiers.
        modifiers: Vec<KeyModifierV1>,
    },
    /// Axis occupancy keyed by the ADR 0033 tuple.
    Axis {
        /// Device source.
        source: String,
        /// Axis component.
        component: String,
        /// Direction.
        direction: String,
        /// Deadzone milli-units.
        deadzone_millis: i32,
        /// Canonical modifiers.
        modifiers: Vec<KeyModifierV1>,
    },
}

pub(crate) fn float_millis(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let scaled = f64::from(value) * 1000.0;
    let rounded = scaled.round();
    if rounded > f64::from(i32::MAX) {
        i32::MAX
    } else if rounded < f64::from(i32::MIN) {
        i32::MIN
    } else {
        #[allow(clippy::cast_possible_truncation)]
        {
            rounded as i32
        }
    }
}

fn validate_usage(usage: &str) -> Result<(), InputError> {
    if usage.is_empty() || !usage.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return Err(InputError::InvalidBinding {
            reason: format!("physical usage `{usage}` is not a canonical identifier"),
        });
    }
    if !unicode_normalization::is_nfc(usage) {
        return Err(InputError::InvalidBinding {
            reason: format!("physical usage `{usage}` is not Unicode NFC"),
        });
    }
    Ok(())
}

fn validate_deadzone(deadzone: f32) -> Result<(), InputError> {
    if !deadzone.is_finite() || !(0.0..=1.0).contains(&deadzone) {
        return Err(InputError::InvalidBinding {
            reason: format!("deadzone {deadzone} is not a finite value in 0..=1"),
        });
    }
    Ok(())
}

fn validate_sensitivity(sensitivity: f32) -> Result<(), InputError> {
    if !sensitivity.is_finite() || sensitivity <= 0.0 {
        return Err(InputError::InvalidBinding {
            reason: format!("sensitivity {sensitivity} is not a finite positive value"),
        });
    }
    Ok(())
}

impl TryFrom<Value> for InputBindingV1 {
    type Error = InputError;

    #[allow(
        clippy::too_many_lines,
        reason = "all closed binding variants remain visibly decoded in one schema boundary"
    )]
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let Value::Object(mut object) = value else {
            return Err(InputError::InvalidBinding {
                reason: "a binding must be a JSON object".to_owned(),
            });
        };
        let schema = string_field(&mut object, "schema")?
            .unwrap_or_else(|| ids::INPUT_BINDING_SCHEMA.to_owned());
        let major = ids::schema_major(&schema, "input-binding")?;
        if major != 1 {
            return Err(InputError::UnknownRequiredMajor {
                schema,
                reason: "unknown required input-binding major",
            });
        }
        let Some(kind) = string_field(&mut object, "kind")? else {
            return Err(InputError::InvalidBinding {
                reason: "binding kind is required".to_owned(),
            });
        };
        match kind.as_str() {
            "keyboard" => {
                let usage = required_string(&mut object, "usage")?;
                let modifiers = modifiers_field(&mut object)?;
                reject_leftovers(&object)?;
                validate_usage(&usage)?;
                Ok(Self::Keyboard { usage, modifiers })
            }
            "keyboard-virtual-dpad" => {
                let up = required_string(&mut object, "up")?;
                let down = required_string(&mut object, "down")?;
                let left = required_string(&mut object, "left")?;
                let right = required_string(&mut object, "right")?;
                let modifiers = modifiers_field(&mut object)?;
                reject_leftovers(&object)?;
                validate_usage(&up)?;
                validate_usage(&down)?;
                validate_usage(&left)?;
                validate_usage(&right)?;
                Ok(Self::KeyboardVirtualDPad {
                    up,
                    down,
                    left,
                    right,
                    modifiers,
                })
            }
            "mouse-button" => {
                let button = required_string(&mut object, "button")?;
                reject_leftovers(&object)?;
                Ok(Self::MouseButton {
                    button: parse_mouse_button(&button)?,
                })
            }
            "mouse-motion" => {
                let sensitivity = float_field(&object, "sensitivity")?.unwrap_or(1.0);
                object.remove("sensitivity");
                reject_leftovers(&object)?;
                validate_sensitivity(sensitivity)?;
                Ok(Self::MouseMotion { sensitivity })
            }
            "mouse-wheel" => {
                let axis = required_string(&mut object, "axis")?;
                reject_leftovers(&object)?;
                Ok(Self::MouseWheel {
                    axis: parse_wheel_axis(&axis)?,
                })
            }
            "mouse-wheel-direction" => {
                let direction = required_string(&mut object, "direction")?;
                reject_leftovers(&object)?;
                Ok(Self::MouseWheelDirection {
                    direction: parse_wheel_direction(&direction)?,
                })
            }
            "gamepad-button" => {
                let button = required_string(&mut object, "button")?;
                reject_leftovers(&object)?;
                validate_usage(&button)?;
                Ok(Self::GamepadButton { button })
            }
            "gamepad-stick" => {
                let stick = required_string(&mut object, "stick")?;
                let deadzone = float_field(&object, "deadzone")?.unwrap_or(0.1);
                let sensitivity = float_field(&object, "sensitivity")?.unwrap_or(1.0);
                object.remove("deadzone");
                object.remove("sensitivity");
                reject_leftovers(&object)?;
                validate_deadzone(deadzone)?;
                validate_sensitivity(sensitivity)?;
                Ok(Self::GamepadStick {
                    stick: parse_stick(&stick)?,
                    deadzone,
                    sensitivity,
                })
            }
            other => Ok(Self::Unknown {
                schema,
                kind: other.to_owned(),
                payload: object,
            }),
        }
    }
}

impl From<InputBindingV1> for Value {
    fn from(binding: InputBindingV1) -> Self {
        let mut object = Map::new();
        object.insert(
            "schema".to_owned(),
            Value::String(ids::INPUT_BINDING_SCHEMA.to_owned()),
        );
        match binding {
            InputBindingV1::Keyboard { usage, modifiers } => {
                object.insert("kind".to_owned(), Value::String("keyboard".to_owned()));
                object.insert("usage".to_owned(), Value::String(usage));
                object.insert("modifiers".to_owned(), modifiers_value(&modifiers));
            }
            InputBindingV1::KeyboardVirtualDPad {
                up,
                down,
                left,
                right,
                modifiers,
            } => {
                object.insert(
                    "kind".to_owned(),
                    Value::String("keyboard-virtual-dpad".to_owned()),
                );
                object.insert("up".to_owned(), Value::String(up));
                object.insert("down".to_owned(), Value::String(down));
                object.insert("left".to_owned(), Value::String(left));
                object.insert("right".to_owned(), Value::String(right));
                object.insert("modifiers".to_owned(), modifiers_value(&modifiers));
            }
            InputBindingV1::MouseButton { button } => {
                object.insert("kind".to_owned(), Value::String("mouse-button".to_owned()));
                object.insert("button".to_owned(), Value::String(format!("{button:?}")));
            }
            InputBindingV1::MouseMotion { sensitivity } => {
                object.insert("kind".to_owned(), Value::String("mouse-motion".to_owned()));
                insert_finite_number(&mut object, "sensitivity", sensitivity);
            }
            InputBindingV1::MouseWheel { axis } => {
                object.insert("kind".to_owned(), Value::String("mouse-wheel".to_owned()));
                object.insert("axis".to_owned(), Value::String(format!("{axis:?}")));
            }
            InputBindingV1::MouseWheelDirection { direction } => {
                object.insert(
                    "kind".to_owned(),
                    Value::String("mouse-wheel-direction".to_owned()),
                );
                object.insert(
                    "direction".to_owned(),
                    Value::String(format!("{direction:?}")),
                );
            }
            InputBindingV1::GamepadButton { button } => {
                object.insert(
                    "kind".to_owned(),
                    Value::String("gamepad-button".to_owned()),
                );
                object.insert("button".to_owned(), Value::String(button));
            }
            InputBindingV1::GamepadStick {
                stick,
                deadzone,
                sensitivity,
            } => {
                object.insert("kind".to_owned(), Value::String("gamepad-stick".to_owned()));
                let stick = match stick {
                    GamepadStickV1::Left => "left",
                    GamepadStickV1::Right => "right",
                };
                object.insert("stick".to_owned(), Value::String(stick.to_owned()));
                insert_finite_number(&mut object, "deadzone", deadzone);
                insert_finite_number(&mut object, "sensitivity", sensitivity);
            }
            InputBindingV1::Unknown {
                schema,
                kind,
                payload,
            } => {
                object = payload;
                object.insert("schema".to_owned(), Value::String(schema));
                object.insert("kind".to_owned(), Value::String(kind));
            }
        }
        Value::Object(object)
    }
}

fn string_field(object: &mut Map<String, Value>, key: &str) -> Result<Option<String>, InputError> {
    match object.remove(key) {
        None => Ok(None),
        Some(Value::String(value)) => {
            if !unicode_normalization::is_nfc(&value) {
                return Err(InputError::InvalidBinding {
                    reason: format!("field `{key}` is not Unicode NFC"),
                });
            }
            Ok(Some(value))
        }
        Some(_) => Err(InputError::InvalidBinding {
            reason: format!("field `{key}` must be a string"),
        }),
    }
}

fn required_string(object: &mut Map<String, Value>, key: &str) -> Result<String, InputError> {
    string_field(object, key)?.ok_or_else(|| InputError::InvalidBinding {
        reason: format!("field `{key}` is required"),
    })
}

fn modifiers_field(object: &mut Map<String, Value>) -> Result<BTreeSet<KeyModifierV1>, InputError> {
    let Some(value) = object.remove("modifiers") else {
        return Ok(BTreeSet::new());
    };
    let Value::Array(items) = value else {
        return Err(InputError::InvalidBinding {
            reason: "modifiers must be an array".to_owned(),
        });
    };
    let mut modifiers = BTreeSet::new();
    for item in items {
        let Value::String(name) = item else {
            return Err(InputError::InvalidBinding {
                reason: "modifiers must be strings".to_owned(),
            });
        };
        let modifier = match name.as_str() {
            "Alt" => KeyModifierV1::Alt,
            "Control" => KeyModifierV1::Control,
            "Shift" => KeyModifierV1::Shift,
            "Super" => KeyModifierV1::Super,
            other => {
                return Err(InputError::InvalidBinding {
                    reason: format!("unknown modifier `{other}`"),
                });
            }
        };
        modifiers.insert(modifier);
    }
    Ok(modifiers)
}

fn modifiers_value(modifiers: &BTreeSet<KeyModifierV1>) -> Value {
    Value::Array(
        modifiers
            .iter()
            .map(|modifier| Value::String(format!("{modifier:?}")))
            .collect(),
    )
}

fn float_field(object: &Map<String, Value>, key: &str) -> Result<Option<f32>, InputError> {
    match object.get(key) {
        None => Ok(None),
        Some(Value::Number(number)) => {
            let Some(value) = number.as_f64() else {
                return Err(InputError::InvalidBinding {
                    reason: format!("field `{key}` is not a finite number"),
                });
            };
            #[allow(clippy::cast_possible_truncation)]
            Ok(Some(value as f32))
        }
        Some(_) => Err(InputError::InvalidBinding {
            reason: format!("field `{key}` must be a number"),
        }),
    }
}

fn insert_finite_number(object: &mut Map<String, Value>, key: &str, value: f32) {
    if let Some(number) = serde_json::Number::from_f64(f64::from(value)) {
        object.insert(key.to_owned(), Value::Number(number));
    }
}

fn reject_leftovers(object: &Map<String, Value>) -> Result<(), InputError> {
    if object.is_empty() {
        Ok(())
    } else {
        let keys = object.keys().cloned().collect::<Vec<_>>().join(", ");
        Err(InputError::InvalidBinding {
            reason: format!("unknown required binding fields: {keys}"),
        })
    }
}

fn parse_mouse_button(value: &str) -> Result<MouseButtonV1, InputError> {
    match value {
        "Left" => Ok(MouseButtonV1::Left),
        "Right" => Ok(MouseButtonV1::Right),
        "Middle" => Ok(MouseButtonV1::Middle),
        "Back" => Ok(MouseButtonV1::Back),
        "Forward" => Ok(MouseButtonV1::Forward),
        other => Err(InputError::InvalidBinding {
            reason: format!("unknown mouse button `{other}`"),
        }),
    }
}

fn parse_wheel_axis(value: &str) -> Result<MouseWheelAxisV1, InputError> {
    match value {
        "X" => Ok(MouseWheelAxisV1::X),
        "Y" => Ok(MouseWheelAxisV1::Y),
        other => Err(InputError::InvalidBinding {
            reason: format!("unknown mouse-wheel axis `{other}`"),
        }),
    }
}

fn parse_wheel_direction(value: &str) -> Result<MouseWheelDirectionV1, InputError> {
    match value {
        "Up" => Ok(MouseWheelDirectionV1::Up),
        "Down" => Ok(MouseWheelDirectionV1::Down),
        "Left" => Ok(MouseWheelDirectionV1::Left),
        "Right" => Ok(MouseWheelDirectionV1::Right),
        other => Err(InputError::InvalidBinding {
            reason: format!("unknown mouse-wheel direction `{other}`"),
        }),
    }
}

fn parse_stick(value: &str) -> Result<GamepadStickV1, InputError> {
    match value {
        "left" => Ok(GamepadStickV1::Left),
        "right" => Ok(GamepadStickV1::Right),
        other => Err(InputError::InvalidBinding {
            reason: format!("unknown gamepad stick `{other}`"),
        }),
    }
}
