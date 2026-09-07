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
    /// Fine blades and clumped color on turf.
    Grass,
    /// Soil with an uneven grass rim at the upper local-V edge.
    GrassSide,
    /// Granular soil with sparse mineral inclusions.
    Soil,
    /// Soil with larger stones.
    CoarseSoil,
    /// Dark, low-contrast wet soil.
    Mud,
    /// Soil crossed by thin roots.
    RootedSoil,
    /// Overlapping broad leaves with actual cutout gaps.
    Leaves,
    /// Dense narrow conifer needles with cutout gaps.
    Needles,
    /// Longitudinal wood grain.
    Wood,
    /// Rough longitudinal bark.
    Bark,
    /// Growth rings on a cut log face.
    EndGrain,
}

/// Small editable material definition consumed by the voxel presenter.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceMaterial {
    /// Authored sRGB channels; transparency comes from the content render group.
    pub color: [u8; 3],
    /// Procedural surface detail.
    #[serde(default)]
    pub pattern: MaterialPattern,
    /// Optional secondary color for turf rims and other composite patterns.
    #[serde(default)]
    pub accent: Option<[u8; 3]>,
    /// Perceptual roughness in 0..255; absent keeps the matte baseline.
    #[serde(default)]
    pub roughness: Option<u8>,
    /// Metal fraction in 0..255; absent is dielectric.
    #[serde(default)]
    pub metallic: Option<u8>,
}

impl ResourceMaterial {
    /// Computes one repeatable texel without image files or global random state.
    #[must_use]
    pub fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        if !matches!(
            self.pattern,
            MaterialPattern::Solid | MaterialPattern::Grain | MaterialPattern::Strata
        ) {
            return natural_texel(self, x % 16, y % 16);
        }
        let delta: i16 = match self.pattern {
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
            _ => 0,
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
    /// References to code-provided presentation models; never gameplay code.
    #[serde(default)]
    pub models: BTreeMap<StableId, ResourceModel>,
    /// Data-root-relative water shader, when this is a shader pack.
    #[serde(default)]
    pub water_shader: Option<CanonicalLogicalPath>,
    /// Required water material binding contract, currently one.
    #[serde(default)]
    pub water_interface: Option<u32>,
}

/// Editable references into a product's explicitly installed model providers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceModel {
    /// Stable identity of an installed procedural model provider.
    pub provider: StableId,
    /// Optional primary material, such as a tool head's stone or metal.
    #[serde(default)]
    pub material: Option<StableId>,
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
        let valid = matches!(self.schema_version, 1 | 2)
            && match self.kind {
                ResourcePackKind::Textures => {
                    self.water_shader.is_none() && self.water_interface.is_none()
                }
                ResourcePackKind::Shaders => {
                    self.materials.is_empty()
                        && self.models.is_empty()
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
    models: BTreeMap<String, (i32, ResourceModel)>,
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
        for (id, model) in &pack.models {
            let key = id.to_string();
            if let Some((priority, _)) = next.models.get(&key) {
                if *priority == pack.priority {
                    return Err(ResourcePackError::Conflict(key));
                }
                if *priority > pack.priority {
                    continue;
                }
            }
            next.models.insert(key, (pack.priority, model.clone()));
        }
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

    /// A model reference never installs or activates its code provider.
    #[must_use]
    pub fn model(&self, id: &str) -> Option<&ResourceModel> {
        self.models.get(id).map(|(_, model)| model)
    }

    /// Resolves an authored face layer against a whole-content override.
    /// Higher-priority packs win; at equal priority the face is more specific.
    #[must_use]
    pub fn material_for_layer(&self, content: &str, layer: &str) -> Option<&ResourceMaterial> {
        match (self.materials.get(content), self.materials.get(layer)) {
            (Some((a, content)), Some((b, face))) => Some(if a > b { content } else { face }),
            (Some((_, value)), None) | (None, Some((_, value))) => Some(value),
            (None, None) => None,
        }
    }

    /// Stable material rows for derived presentation-cache identities.
    pub fn materials(&self) -> impl Iterator<Item = (&str, &ResourceMaterial)> {
        self.materials
            .iter()
            .map(|(id, (_, material))| (id.as_str(), material))
    }

    /// Returns the selected shader implementing the water interface.
    #[must_use]
    pub fn water_shader(&self) -> Option<&str> {
        self.water_shader
            .as_ref()
            .map(|(_, source)| source.as_str())
    }
}

fn surface_noise(x: u32, y: u32) -> u32 {
    let mut value = x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b) ^ 0x27d4_eb2d;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^ (value >> 15)
}

fn natural_texel(style: &ResourceMaterial, x: u32, y: u32) -> [u8; 4] {
    let noise = surface_noise(x, y);
    let grain = i16::try_from(noise % 23).unwrap_or(0) - 11;
    let clump = i16::try_from(surface_noise(x / 3, y / 3) % 21).unwrap_or(0) - 10;
    let mut color = style.color;
    let mut alpha = 255;
    let delta = match style.pattern {
        MaterialPattern::Grass => grain / 2 + clump + if noise % 19 < 2 { 16 } else { 0 },
        MaterialPattern::GrassSide => {
            if y >= 12 + surface_noise(x / 2, 0) % 3 {
                color = style.accent.unwrap_or([76, 118, 48]);
                grain + clump / 2
            } else {
                grain + clump + if noise.is_multiple_of(31) { 22 } else { 0 }
            }
        }
        MaterialPattern::Soil => grain + clump + if noise.is_multiple_of(31) { 24 } else { 0 },
        MaterialPattern::CoarseSoil => {
            grain
                + clump
                + if surface_noise(x / 2, y / 2).is_multiple_of(7) {
                    27
                } else {
                    0
                }
        }
        MaterialPattern::Mud => grain / 3 + clump / 2,
        MaterialPattern::RootedSoil => {
            grain
                + if (x + y / 2 + 2 * (y / 5)).is_multiple_of(11) {
                    34
                } else {
                    clump
                }
        }
        MaterialPattern::Leaves => {
            let leaf_x = (x + (y / 4) * 2) % 5;
            let leaf_y = y % 4;
            if (leaf_x == 0 && leaf_y < 2) || (leaf_x == 4 && leaf_y > 1) {
                alpha = 0;
            }
            grain / 2 + clump + if leaf_x == 2 { 14 } else { -4 }
        }
        MaterialPattern::Needles => {
            if (x + y * 2) % 7 > 4 && !noise.is_multiple_of(4) {
                alpha = 0;
            }
            grain / 2 + clump
        }
        MaterialPattern::Wood | MaterialPattern::Bark => {
            let ridge = surface_noise((x + y / 7) % 16, y / 8) % 7;
            grain / 3 + if ridge < 2 { -24 } else { 9 }
        }
        MaterialPattern::EndGrain => {
            let dx = i32::try_from(x).unwrap_or(0) - 7;
            let dy = i32::try_from(y).unwrap_or(0) - 7;
            let ring = (dx * dx + dy * dy).unsigned_abs().isqrt() % 3;
            grain / 3 + if ring == 0 { -24 } else { 8 }
        }
        _ => grain,
    };
    let channel =
        |value: u8| u8::try_from((i16::from(value) + delta).clamp(0, 255)).unwrap_or(value);
    [
        channel(color[0]),
        channel(color[1]),
        channel(color[2]),
        alpha,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_materials_have_real_leaf_gaps_and_a_soil_side_below_the_grass_rim() {
        let side = ResourceMaterial {
            color: [116, 83, 53],
            pattern: MaterialPattern::GrassSide,
            ..ResourceMaterial::default()
        };
        for x in 0..16 {
            let bottom = side.texel(x, 0);
            let top = side.texel(x, 15);
            assert!(bottom[0] > bottom[1]);
            assert!(top[1] > top[0]);
        }
        for pattern in [MaterialPattern::Leaves, MaterialPattern::Needles] {
            let leaves = ResourceMaterial {
                color: [76, 118, 48],
                pattern,
                ..ResourceMaterial::default()
            };
            let covered = (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|(x, y)| leaves.texel(*x, *y)[3] == 255)
                .count();
            assert!((128..240).contains(&covered), "leaf coverage: {covered}");
            assert_eq!(leaves.texel(3, 5), leaves.texel(19, 21));
        }
    }

    #[test]
    fn face_specificity_respects_whole_material_pack_priority() {
        let mut base = pack(0, [80, 60, 40]);
        base.materials.insert(
            "example:asset/top".parse().expect("layer"),
            ResourceMaterial {
                color: [40, 90, 30],
                pattern: MaterialPattern::Grass,
                ..ResourceMaterial::default()
            },
        );
        let mut resolved = ResolvedResourcePacks::default();
        resolved.apply(&base, None).expect("base");
        assert_eq!(
            resolved
                .material_for_layer("example:block/stone", "example:asset/top")
                .expect("face")
                .color,
            [40, 90, 30]
        );
        resolved
            .apply(&pack(10, [20, 30, 40]), None)
            .expect("override");
        assert_eq!(
            resolved
                .material_for_layer("example:block/stone", "example:asset/top")
                .expect("override")
                .color,
            [20, 30, 40]
        );
    }

    fn pack(priority: i32, color: [u8; 3]) -> ResourcePackV1 {
        ResourcePackV1 {
            schema_version: 1,
            id: "example:pack/texture".parse().expect("id"),
            kind: ResourcePackKind::Textures,
            priority,
            models: BTreeMap::new(),
            materials: BTreeMap::from([(
                "example:block/stone".parse().expect("id"),
                ResourceMaterial {
                    color,
                    pattern: MaterialPattern::Grain,
                    ..ResourceMaterial::default()
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
