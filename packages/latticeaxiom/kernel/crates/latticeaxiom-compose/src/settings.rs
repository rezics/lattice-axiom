//! Package-injected runtime settings and graph parameter models.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{PackageName, StableId};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use thiserror::Error;

/// Persistence scope for a runtime setting.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingScope {
    /// Machine-local device state.
    Device,
    /// User state shared across worlds.
    User,
    /// Authoritative world metadata.
    World,
    /// Per-player state owned by one world.
    PlayerWorld,
    /// In-memory state discarded at shutdown.
    Session,
}

/// Authority permitted to change a runtime setting.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingAuthority {
    /// The local user may change the value.
    LocalUser,
    /// The active world owner controls the value.
    WorldOwner,
    /// A server controls the value.
    Server,
    /// Only an administrator may change the value.
    AdminOnly,
    /// The active profile fixes the value.
    FixedByProfile,
}

/// Runtime effect of applying a setting transaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeApplyImpact {
    /// Apply a reversible presentation preview before persistence.
    Preview,
    /// Apply after the transaction commits.
    Immediate,
    /// Rebuild the active world while keeping the package graph fixed.
    WorldReactivate,
    /// Restart the process without changing the package graph.
    ProcessRestart,
}

/// Effect of changing a composition parameter draft.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionApplyImpact {
    /// Resolve a new graph, show its diff, and publish a new lock.
    GraphRecompose,
}

/// Finite value schema shared by settings and composition parameters.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "type")]
pub enum ValueType {
    /// Boolean value.
    Bool,
    /// Bounded integer value.
    Integer {
        /// Inclusive minimum.
        min: Option<i64>,
        /// Inclusive maximum.
        max: Option<i64>,
        /// UI and validation step.
        step: Option<u64>,
    },
    /// Exact finite decimal fitting a signed 18-decimal fixed-point value.
    Number {
        /// Inclusive minimum.
        min: Option<Number>,
        /// Inclusive maximum.
        max: Option<Number>,
        /// UI and validation step.
        step: Option<Number>,
    },
    /// UTF-8 string value.
    String {
        /// Minimum Unicode scalar count.
        min_length: Option<u32>,
        /// Maximum Unicode scalar count.
        max_length: Option<u32>,
    },
    /// One value from a stable closed vocabulary.
    Enum {
        /// Allowed stable values.
        values: BTreeSet<String>,
    },
    /// Linear RGBA color encoded as four finite numbers.
    Color,
    /// Named input binding serialized independently of Bevy handles.
    KeyBinding,
}

impl ValueType {
    /// Validates the schema constraints and one typed value against them.
    ///
    /// # Errors
    ///
    /// Returns [`ValueValidationError`] when constraints are contradictory or
    /// `value` has the wrong JSON type, lies outside a bound, or violates an
    /// integer step.
    pub fn validate_value(&self, value: &Value) -> Result<(), ValueValidationError> {
        match self {
            Self::Bool => {
                value
                    .is_boolean()
                    .then_some(())
                    .ok_or(ValueValidationError::InvalidValue {
                        reason: "expected a Boolean",
                    })
            }
            Self::Integer { min, max, step } => validate_integer(value, *min, *max, *step),
            Self::Number { min, max, step } => {
                validate_number(value, min.as_ref(), max.as_ref(), step.as_ref())
            }
            Self::String {
                min_length,
                max_length,
            } => validate_string(value, *min_length, *max_length),
            Self::Enum { values } => validate_enum(value, values),
            Self::Color => validate_color(value),
            Self::KeyBinding => value
                .as_str()
                .is_some_and(|binding| !binding.is_empty())
                .then_some(())
                .ok_or(ValueValidationError::InvalidValue {
                    reason: "expected a non-empty named key binding",
                }),
        }
    }
}

fn validate_integer(
    value: &Value,
    min: Option<i64>,
    max: Option<i64>,
    step: Option<u64>,
) -> Result<(), ValueValidationError> {
    validate_ordered_bounds(min, max, "integer minimum exceeds maximum")?;
    if step == Some(0) {
        return Err(ValueValidationError::InvalidSchema {
            reason: "integer step must be positive",
        });
    }
    let integer = value.as_i64().ok_or(ValueValidationError::InvalidValue {
        reason: "expected a signed JSON integer",
    })?;
    validate_ordered_value(integer, min, max)?;
    if let Some(step) = step {
        let origin = i128::from(min.unwrap_or(0));
        if (i128::from(integer) - origin).rem_euclid(i128::from(step)) != 0 {
            return Err(ValueValidationError::InvalidValue {
                reason: "integer does not align with its validation step",
            });
        }
    }
    Ok(())
}

fn validate_number(
    value: &Value,
    min: Option<&Number>,
    max: Option<&Number>,
    step: Option<&Number>,
) -> Result<(), ValueValidationError> {
    let min = min.map(number_as_fixed).transpose()?;
    let max = max.map(number_as_fixed).transpose()?;
    validate_ordered_bounds(min, max, "number minimum exceeds maximum")?;
    let step = step.map(number_as_fixed).transpose()?;
    if step.is_some_and(|step| step <= 0) {
        return Err(ValueValidationError::InvalidSchema {
            reason: "number step must be positive",
        });
    }
    let number = value
        .as_number()
        .ok_or(ValueValidationError::InvalidValue {
            reason: "expected an exact finite decimal number",
        })
        .and_then(number_as_fixed)?;
    validate_ordered_value(number, min, max)?;
    if let Some(step) = step {
        let origin = min.unwrap_or(0);
        let offset = number
            .checked_sub(origin)
            .ok_or(ValueValidationError::InvalidValue {
                reason: "number step offset exceeds the supported range",
            })?;
        if offset.rem_euclid(step) != 0 {
            return Err(ValueValidationError::InvalidValue {
                reason: "number does not align with its validation step",
            });
        }
    }
    Ok(())
}

fn validate_string(
    value: &Value,
    min: Option<u32>,
    max: Option<u32>,
) -> Result<(), ValueValidationError> {
    validate_ordered_bounds(min, max, "string minimum length exceeds maximum length")?;
    let text = value.as_str().ok_or(ValueValidationError::InvalidValue {
        reason: "expected a JSON string",
    })?;
    let length =
        u32::try_from(text.chars().count()).map_err(|_| ValueValidationError::InvalidValue {
            reason: "string length exceeds the supported range",
        })?;
    validate_ordered_value(length, min, max)
}

fn validate_enum(value: &Value, values: &BTreeSet<String>) -> Result<(), ValueValidationError> {
    if values.is_empty() {
        return Err(ValueValidationError::InvalidSchema {
            reason: "enum vocabulary cannot be empty",
        });
    }
    let selected = value.as_str().ok_or(ValueValidationError::InvalidValue {
        reason: "expected an enum string",
    })?;
    values
        .contains(selected)
        .then_some(())
        .ok_or(ValueValidationError::InvalidValue {
            reason: "enum value is not in the closed vocabulary",
        })
}

fn validate_ordered_bounds<T>(
    min: Option<T>,
    max: Option<T>,
    reason: &'static str,
) -> Result<(), ValueValidationError>
where
    T: PartialOrd,
{
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        Err(ValueValidationError::InvalidSchema { reason })
    } else {
        Ok(())
    }
}

fn validate_ordered_value<T>(
    value: T,
    min: Option<T>,
    max: Option<T>,
) -> Result<(), ValueValidationError>
where
    T: Copy + PartialOrd,
{
    if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
        Err(ValueValidationError::InvalidValue {
            reason: "value lies outside its inclusive bounds",
        })
    } else {
        Ok(())
    }
}

const FIXED_DECIMAL_SCALE: u32 = 18;

fn number_as_fixed(number: &Number) -> Result<i128, ValueValidationError> {
    parse_fixed_decimal(&number.to_string()).ok_or(ValueValidationError::InvalidValue {
        reason: "number must fit a signed 18-decimal fixed-point value",
    })
}

fn parse_fixed_decimal(text: &str) -> Option<i128> {
    let (mantissa, exponent) = if let Some((mantissa, exponent)) = text.split_once(['e', 'E']) {
        (mantissa, exponent.parse::<i32>().ok()?)
    } else {
        (text, 0_i32)
    };
    let negative = mantissa.starts_with('-');
    let unsigned = mantissa
        .strip_prefix('-')
        .or_else(|| mantissa.strip_prefix('+'))
        .unwrap_or(mantissa);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let mut coefficient = digits.parse::<i128>().ok()?;
    if negative {
        coefficient = coefficient.checked_neg()?;
    }
    let fraction_digits = i32::try_from(fraction.len()).ok()?;
    let multiplier = i32::try_from(FIXED_DECIMAL_SCALE)
        .ok()?
        .checked_sub(fraction_digits)?
        .checked_add(exponent)?;
    if multiplier >= 0 {
        coefficient.checked_mul(10_i128.checked_pow(u32::try_from(multiplier).ok()?)?)
    } else {
        let divisor = 10_i128.checked_pow(multiplier.unsigned_abs())?;
        (coefficient.rem_euclid(divisor) == 0).then(|| coefficient / divisor)
    }
}

fn validate_color(value: &Value) -> Result<(), ValueValidationError> {
    let channels = value
        .as_array()
        .filter(|channels| channels.len() == 4)
        .ok_or(ValueValidationError::InvalidValue {
            reason: "expected four linear RGBA channels",
        })?;
    for channel in channels {
        let channel = channel
            .as_number()
            .ok_or(ValueValidationError::InvalidValue {
                reason: "color channels must be exact finite decimal numbers",
            })
            .and_then(number_as_fixed)?;
        if !(0..=10_i128.pow(FIXED_DECIMAL_SCALE)).contains(&channel) {
            return Err(ValueValidationError::InvalidValue {
                reason: "color channels must lie in the inclusive range 0..1",
            });
        }
    }
    Ok(())
}

/// A finite value schema or value failed validation.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ValueValidationError {
    /// Schema constraints contradict the declared finite type.
    #[error("invalid value schema: {reason}")]
    InvalidSchema {
        /// Violated schema invariant.
        reason: &'static str,
    },
    /// A value does not satisfy its declared finite type.
    #[error("invalid typed value: {reason}")]
    InvalidValue {
        /// Violated value invariant.
        reason: &'static str,
    },
}

/// Declarative condition over effective setting values.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "op")]
pub enum SettingPredicate {
    /// Compare one setting to a typed value.
    Equals {
        /// Referenced setting ID.
        setting: StableId,
        /// Expected typed value.
        value: Value,
    },
    /// Require every child condition.
    All {
        /// Child conditions.
        predicates: Vec<Self>,
    },
    /// Require at least one child condition.
    Any {
        /// Child conditions.
        predicates: Vec<Self>,
    },
    /// Invert a child condition.
    Not {
        /// Child condition.
        predicate: Box<Self>,
    },
}

/// Privacy classification for setting persistence and display.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingSensitivity {
    /// Ordinary non-secret value.
    Ordinary,
    /// A private local path that must be redacted from reports.
    PrivatePath,
}

/// A package-injected runtime setting declaration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingSpec {
    /// Stable setting ID.
    pub id: StableId,
    /// Logical package owning schema and migration.
    pub declared_by: PackageName,
    /// Owner-controlled value schema version.
    pub schema_version: u32,
    /// Finite value schema.
    pub value_type: ValueType,
    /// Typed default value.
    pub default: Value,
    /// Stores in which the value may persist.
    pub allowed_scopes: BTreeSet<SettingScope>,
    /// Initial persistence scope.
    pub default_scope: SettingScope,
    /// Authority allowed to modify the value.
    pub authority: SettingAuthority,
    /// Runtime apply behavior. Graph recomposition is deliberately absent.
    pub apply_impact: RuntimeApplyImpact,
    /// Stable semantic category ID.
    pub category: StableId,
    /// Owner-local deterministic display order.
    pub order: i32,
    /// Localization key for the setting label.
    pub label_key: String,
    /// Localization key for the setting description.
    pub description_key: String,
    /// Declarative visibility condition.
    pub visibility: Option<SettingPredicate>,
    /// Declarative enabled condition.
    pub enabled_when: Option<SettingPredicate>,
    /// Persistence sensitivity.
    pub sensitivity: SettingSensitivity,
    /// Optional replacement for a deprecated ID.
    pub replacement: Option<StableId>,
}

/// A graph-affecting parameter edited as a profile draft.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionParameterSpec {
    /// Stable parameter ID.
    pub id: StableId,
    /// Logical package owning the parameter schema.
    pub declared_by: PackageName,
    /// Finite value schema.
    pub value_type: ValueType,
    /// Typed default value.
    pub default: Value,
    /// Graph-recomposition behavior.
    pub apply_impact: CompositionApplyImpact,
}

impl CompositionParameterSpec {
    /// Validates the parameter schema and typed default.
    ///
    /// # Errors
    ///
    /// Returns [`ValueValidationError`] when the schema or default is invalid.
    pub fn validate(&self) -> Result<(), ValueValidationError> {
        self.value_type.validate_value(&self.default)
    }
}

/// Compiled setting catalog in deterministic registration order.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsCatalog {
    /// Runtime settings keyed by stable ID.
    pub runtime: BTreeMap<StableId, SettingSpec>,
    /// Composition parameters keyed by stable ID.
    pub composition: BTreeMap<StableId, CompositionParameterSpec>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_value_types_validate_schema_and_defaults() {
        let integer = ValueType::Integer {
            min: Some(2),
            max: Some(8),
            step: Some(2),
        };
        assert!(integer.validate_value(&serde_json::json!(6)).is_ok());
        assert!(integer.validate_value(&serde_json::json!(5)).is_err());

        let invalid_bounds = ValueType::String {
            min_length: Some(5),
            max_length: Some(2),
        };
        assert!(
            invalid_bounds
                .validate_value(&serde_json::json!("value"))
                .is_err()
        );

        let choices = ValueType::Enum {
            values: BTreeSet::from(["fast".to_owned(), "safe".to_owned()]),
        };
        assert!(choices.validate_value(&serde_json::json!("safe")).is_ok());
        assert!(
            choices
                .validate_value(&serde_json::json!("unknown"))
                .is_err()
        );

        assert!(
            ValueType::Color
                .validate_value(&serde_json::json!([0, 0.5, 1, 1]))
                .is_ok()
        );
        assert!(
            ValueType::Color
                .validate_value(&serde_json::json!([0, 2, 1, 1]))
                .is_err()
        );

        let exact_number = ValueType::Number {
            min: None,
            max: Some(Number::from(9_007_199_254_740_992_u64)),
            step: Some(
                Number::from_f64(0.1)
                    .unwrap_or_else(|| panic!("fixture step must be a finite JSON number")),
            ),
        };
        assert!(
            exact_number
                .validate_value(&serde_json::json!(9_007_199_254_740_993_u64))
                .is_err()
        );
        assert!(exact_number.validate_value(&serde_json::json!(0.2)).is_ok());
        assert!(
            exact_number
                .validate_value(&serde_json::json!(0.15))
                .is_err()
        );
    }
}
