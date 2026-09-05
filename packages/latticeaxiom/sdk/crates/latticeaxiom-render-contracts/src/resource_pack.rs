//! Presentation-only resource packs, independent of authoritative world identity.

use std::collections::BTreeMap;

use latticeaxiom_core::{CanonicalLogicalPath, StableId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Scope of an independently selectable resource pack.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourcePackKind {
    /// Material appearances without any gameplay properties.
    Textures,
    /// Shader source implementing a versioned renderer interface.
    Shaders,
}

/// Code-generated texture pattern; no binary texture payload is stored in Git.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaterialPattern {
    /// Uniform material color.
    #[default]
    Solid,
    /// Fine deterministic surface variation.
    Grain,
    /// Thin horizontal mineral bands.
    Strata,
}

/// Small editable material definition consumed by the voxel presenter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceMaterial {
    /// Authored sRGB channels; transparency comes from the content render group.
    pub color: [u8; 3],
    /// Procedural surface detail.
    #[serde(default)]
    pub pattern: MaterialPattern,
}

impl ResourceMaterial {
    /// Computes one repeatable texel without image files or global random state.
    #[must_use]
    pub fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        let delta: i16 = match self.pattern {
            MaterialPattern::Solid => 0,
            MaterialPattern::Grain => {
                i16::try_from((x.wrapping_mul(17) ^ y.wrapping_mul(31)) % 9).unwrap_or(0) - 4
            }
            MaterialPattern::Strata => {
                if y % 8 < 2 {
                    -12
                } else {
                    2
                }
            }
        };
        let channel =
            |value: u8| u8::try_from((i16::from(value) + delta).clamp(0, 255)).unwrap_or(value);
        [
            channel(self.color[0]),
            channel(self.color[1]),
            channel(self.color[2]),
            255,
        ]
    }

    /// Returns the base sRGB color for distant geometry and item previews.
    #[must_use]
    pub fn rgba(&self) -> [f32; 4] {
        [
            f32::from(self.color[0]) / 255.0,
            f32::from(self.color[1]) / 255.0,
            f32::from(self.color[2]) / 255.0,
            1.0,
        ]
    }
}

/// Resource descriptor stored inside a normal lock-selected data artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcePackV1 {
    /// Currently supported descriptor schema.
    pub schema_version: u32,
    /// Stable pack identity.
    pub id: StableId,
    /// Independent resource family.
    pub kind: ResourcePackKind,
    /// Explicit precedence; duplicate keys at equal precedence are rejected.
    pub priority: i32,
    /// Material appearance rows keyed by content identity.
    #[serde(default)]
    pub materials: BTreeMap<StableId, ResourceMaterial>,
    /// Data-root-relative water shader, when this is a shader pack.
    #[serde(default)]
    pub water_shader: Option<CanonicalLogicalPath>,
    /// Required water material binding contract, currently one.
    #[serde(default)]
    pub water_interface: Option<u32>,
}

/// Invalid resource descriptors cannot influence gameplay or silently override peers.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ResourcePackError {
    /// Unsupported schema, interface, or fields belonging to another resource kind.
    #[error("resource pack {0} has an unsupported schema, interface, or resource kind")]
    Invalid(StableId),
    /// Two selected resource packs claim the same key at the same priority.
    #[error("resource packs conflict on {0} at equal priority")]
    Conflict(String),
}

impl ResourcePackV1 {
    /// Validates the resource boundary and versioned shader binding interface.
    ///
    /// # Errors
    /// Returns [`ResourcePackError`] for unsupported or mixed declarations.
    pub fn validate(&self) -> Result<(), ResourcePackError> {
        let valid = self.schema_version == 1
            && match self.kind {
                ResourcePackKind::Textures => {
                    self.water_shader.is_none() && self.water_interface.is_none()
                }
                ResourcePackKind::Shaders => {
                    self.materials.is_empty()
                        && self.water_shader.is_some()
                        && self.water_interface == Some(1)
                }
            };
        if valid {
            Ok(())
        } else {
            Err(ResourcePackError::Invalid(self.id.clone()))
        }
    }
}

/// Deterministic appearance overlay, kept outside the authoritative world lock.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedResourcePacks {
    materials: BTreeMap<String, (i32, ResourceMaterial)>,
    water_shader: Option<(i32, String)>,
}

impl ResolvedResourcePacks {
    /// Applies a verified data pack with shader bytes already read from its artifact.
    ///
    /// # Errors
    /// Returns [`ResourcePackError`] for invalid declarations or equal-priority conflicts.
    pub fn apply(
        &mut self,
        pack: &ResourcePackV1,
        shader: Option<&str>,
    ) -> Result<(), ResourcePackError> {
        pack.validate()?;
        if pack.water_shader.is_some() != shader.is_some() {
            return Err(ResourcePackError::Invalid(pack.id.clone()));
        }
        let mut next = self.clone();
        for (id, material) in &pack.materials {
            let key = id.to_string();
            if let Some((priority, _)) = next.materials.get(&key) {
                if *priority == pack.priority {
                    return Err(ResourcePackError::Conflict(key));
                }
                if *priority > pack.priority {
                    continue;
                }
            }
            next.materials
                .insert(key, (pack.priority, material.clone()));
        }
        if let Some(source) = shader {
            match &next.water_shader {
                Some((priority, _)) if *priority == pack.priority => {
                    return Err(ResourcePackError::Conflict("water_shader".to_owned()));
                }
                Some((priority, _)) if *priority > pack.priority => {}
                _ => next.water_shader = Some((pack.priority, source.to_owned())),
            }
        }
        *self = next;
        Ok(())
    }

    /// Returns an optional material override for a stable content identity.
    #[must_use]
    pub fn material(&self, id: &str) -> Option<&ResourceMaterial> {
        self.materials.get(id).map(|(_, value)| value)
    }

    /// Returns the selected shader implementing the water interface.
    #[must_use]
    pub fn water_shader(&self) -> Option<&str> {
        self.water_shader
            .as_ref()
            .map(|(_, source)| source.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(priority: i32, color: [u8; 3]) -> ResourcePackV1 {
        ResourcePackV1 {
            schema_version: 1,
            id: "example:pack/texture".parse().expect("id"),
            kind: ResourcePackKind::Textures,
            priority,
            materials: BTreeMap::from([(
                "example:block/stone".parse().expect("id"),
                ResourceMaterial {
                    color,
                    pattern: MaterialPattern::Grain,
                },
            )]),
            water_shader: None,
            water_interface: None,
        }
    }

    #[test]
    fn overlay_priority_is_order_independent_and_conflicts_are_atomic() {
        let mut first = ResolvedResourcePacks::default();
        let mut second = ResolvedResourcePacks::default();
        first.apply(&pack(1, [20, 30, 40]), None).expect("base");
        first.apply(&pack(2, [50, 60, 70]), None).expect("overlay");
        second.apply(&pack(2, [50, 60, 70]), None).expect("overlay");
        second.apply(&pack(1, [20, 30, 40]), None).expect("base");
        assert_eq!(first, second);
        assert!(first.apply(&pack(2, [80, 90, 100]), None).is_err());
        assert_eq!(first, second);
    }

    #[test]
    fn texture_pack_cannot_supply_a_shader_or_authority_fields() {
        let mut value = pack(0, [100, 110, 120]);
        value.water_shader = Some("data/water.wgsl".parse().expect("path"));
        assert!(value.validate().is_err());
        let json = r#"{"schema_version":1,"id":"example:pack/a","kind":"textures","priority":0,"collision":true}"#;
        assert!(serde_json::from_str::<ResourcePackV1>(json).is_err());
    }
}
