//! User binding profile overlay, unbind, and unknown-action preservation.

use std::{collections::BTreeMap, str::FromStr};

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, StableId, canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Serialize};

use crate::{InputBindingV1, InputError, ids};

/// User overrides keyed by action stable ID.
///
/// A missing key inherits package defaults. A present empty list is an explicit
/// keyboard/mouse unbind. Unknown action keys are retained so a temporarily
/// missing package cannot delete user data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BindingProfileV1 {
    /// Versioned schema identifier.
    pub schema: String,
    /// Stable-ID-ordered overrides.
    #[serde(default)]
    pub overrides: BTreeMap<StableId, Vec<InputBindingV1>>,
}

impl BindingProfileV1 {
    /// Creates an empty profile that inherits every package default.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema: ids::BINDING_PROFILE_SCHEMA.to_owned(),
            overrides: BTreeMap::new(),
        }
    }

    /// Parses a canonical profile document.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when JSON is invalid, the schema major is not 1,
    /// or a retained binding cannot be decoded.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, InputError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|error| InputError::InvalidProfile {
                reason: error.to_string(),
            })?;
        Self::from_value(value)
    }

    /// Parses a JSON value as a profile.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] for schema or binding failures.
    pub fn from_value(value: serde_json::Value) -> Result<Self, InputError> {
        let serde_json::Value::Object(mut object) = value else {
            return Err(InputError::InvalidProfile {
                reason: "a binding profile must be a JSON object".to_owned(),
            });
        };
        let schema = match object.remove("schema") {
            Some(serde_json::Value::String(schema)) => schema,
            None => ids::BINDING_PROFILE_SCHEMA.to_owned(),
            Some(_) => {
                return Err(InputError::InvalidProfile {
                    reason: "profile schema must be a string".to_owned(),
                });
            }
        };
        let major = ids::schema_major(&schema, "binding-profile")?;
        if major != 1 {
            return Err(InputError::UnknownRequiredMajor {
                schema,
                reason: "unknown required binding-profile major",
            });
        }
        let overrides = match object.remove("overrides") {
            None => BTreeMap::new(),
            Some(serde_json::Value::Object(entries)) => decode_overrides(entries)?,
            Some(_) => {
                return Err(InputError::InvalidProfile {
                    reason: "overrides must be an object".to_owned(),
                });
            }
        };
        if !object.is_empty() {
            // Newer-minor diagnostic fields are retained by ignoring them here
            // only after schema major 1 is confirmed. They are not re-emitted
            // because v1 canonical form is schema plus overrides.
        }
        Ok(Self {
            schema: ids::BINDING_PROFILE_SCHEMA.to_owned(),
            overrides,
        })
    }

    /// Returns the canonical JSON encoding of this profile.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when the profile cannot be encoded.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }

    /// Returns the canonical JSON hash of this profile.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when the profile cannot be encoded.
    pub fn canonical_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(self)
    }

    /// Returns the override list for `action`, if the user supplied one.
    #[must_use]
    pub fn override_for(&self, action: &StableId) -> Option<&[InputBindingV1]> {
        self.overrides.get(action).map(Vec::as_slice)
    }

    /// Inserts or replaces the keyboard/mouse override for `action`.
    ///
    /// An empty `bindings` list is an explicit unbind. Gamepad bindings are
    /// ignored in first version and are not stored as applied overrides.
    pub fn set_override(&mut self, action: StableId, bindings: Vec<InputBindingV1>) {
        let keyboard_mouse = bindings
            .into_iter()
            .filter(InputBindingV1::is_keyboard_mouse)
            .collect();
        self.overrides.insert(action, keyboard_mouse);
    }

    /// Removes a user override so the action inherits package defaults.
    pub fn clear_override(&mut self, action: &StableId) {
        self.overrides.remove(action);
    }
}

fn decode_overrides(
    entries: serde_json::Map<String, serde_json::Value>,
) -> Result<BTreeMap<StableId, Vec<InputBindingV1>>, InputError> {
    let mut overrides = BTreeMap::new();
    for (key, value) in entries {
        let action = StableId::from_str(&key).map_err(|error| InputError::InvalidProfile {
            reason: error.to_string(),
        })?;
        let serde_json::Value::Array(items) = value else {
            return Err(InputError::InvalidProfile {
                reason: format!("override `{key}` must be an array"),
            });
        };
        let mut bindings = Vec::with_capacity(items.len());
        for item in items {
            bindings.push(InputBindingV1::try_from(item)?);
        }
        overrides.insert(action, bindings);
    }
    Ok(overrides)
}
