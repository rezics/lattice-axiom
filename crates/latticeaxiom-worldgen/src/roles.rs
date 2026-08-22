use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::StableId;
use serde::{Deserialize, Serialize};

use crate::{WorldgenError, WorldgenResult};

/// Minimum number of unique D4 catalog blocks frozen by the roadmap.
pub const D4_MINIMUM_BLOCK_COUNT: usize = 18;

/// Cumulative D7 natural-world catalog size before D9's exact 72 content.
pub const D7_NATURAL_BLOCK_COUNT: usize = 40;

/// Absolute number of frozen Role bindings accepted before any map insertion.
pub const D4_MAX_ROLE_BINDING_COUNT: usize = 256;

/// Absolute number of catalog blocks accepted before validation or sorting.
pub const D4_MAX_CATALOG_BLOCK_COUNT: usize = 512;

/// Functional material outputs consumed by the minimal D4 generator.
///
/// These are generator input slots, not content identities. A package-owned
/// [`D4RoleVocabularyV1`] supplies each semantic role ID and frozen
/// registration bindings resolve it to concrete block IDs before a plan can
/// produce a world command.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum D4MaterialRoleV1 {
    /// Empty solid-layer cell.
    Empty,
    /// Temperate woodland surface.
    TemperateSurface,
    /// Temperate woodland shallow subsurface.
    TemperateSubsurface,
    /// Temperate woodland primary rock.
    TemperateBaseRock,
    /// Temperate woodland secondary rock.
    TemperateSecondaryRock,
    /// Temperate shallow clay patch.
    TemperateClay,
    /// Temperate shallow gravel patch.
    TemperateGravel,
    /// Woodland tree trunk.
    WoodlandLog,
    /// Woodland canopy.
    WoodlandLeaves,
    /// Woodland ground-cover plant.
    WoodlandGroundCover,
    /// Arid pale surface sand.
    AridSand,
    /// Arid red surface sand.
    AridRedSand,
    /// Arid pale subsurface rock.
    AridSandstone,
    /// Arid red subsurface rock.
    AridRedSandstone,
    /// Arid deep base rock.
    AridBaseRock,
    /// Copper-bearing resource replacement.
    CopperResource,
    /// Coarse temperate soil.
    CoarseDirt,
    /// Rooted temperate soil.
    RootedDirt,
    /// Wetland mud.
    Mud,
    /// River-silt bed.
    Silt,
    /// Boreal peat.
    Peat,
    /// Cold surface snow.
    Snow,
    /// Frozen river or wetland ice.
    Ice,
    /// Wetland moss cover.
    Moss,
    /// Deep basement rock.
    Deepstone,
    /// Mid-depth granite body.
    Granite,
    /// Mid-depth slate body.
    Slate,
    /// Volcanic tuff intrusion.
    Tuff,
    /// Calcite replacement.
    Calcite,
    /// Dripstone replacement.
    Dripstone,
    /// Coal-bearing resource replacement.
    CoalResource,
    /// Iron-bearing resource replacement.
    IronResource,
    /// Tin-bearing resource replacement.
    TinResource,
    /// Gold-bearing resource replacement.
    GoldResource,
    /// Sulfur-bearing resource replacement.
    SulfurResource,
    /// Crystal-cluster resource replacement.
    CrystalResource,
    /// Boreal tree trunk.
    BorealLog,
    /// Boreal canopy.
    BorealLeaves,
}

impl D4MaterialRoleV1 {
    /// Canonical order of all required D4 generator roles.
    pub const ALL: [Self; 16] = [
        Self::Empty,
        Self::TemperateSurface,
        Self::TemperateSubsurface,
        Self::TemperateBaseRock,
        Self::TemperateSecondaryRock,
        Self::TemperateClay,
        Self::TemperateGravel,
        Self::WoodlandLog,
        Self::WoodlandLeaves,
        Self::WoodlandGroundCover,
        Self::AridSand,
        Self::AridRedSand,
        Self::AridSandstone,
        Self::AridRedSandstone,
        Self::AridBaseRock,
        Self::CopperResource,
    ];

    /// Additional D7 natural-world purposes. Required only by the V5 layer.
    pub const NATURAL: [Self; 22] = [
        Self::CoarseDirt,
        Self::RootedDirt,
        Self::Mud,
        Self::Silt,
        Self::Peat,
        Self::Snow,
        Self::Ice,
        Self::Moss,
        Self::Deepstone,
        Self::Granite,
        Self::Slate,
        Self::Tuff,
        Self::Calcite,
        Self::Dripstone,
        Self::CoalResource,
        Self::IronResource,
        Self::TinResource,
        Self::GoldResource,
        Self::SulfurResource,
        Self::CrystalResource,
        Self::BorealLog,
        Self::BorealLeaves,
    ];

    /// Returns the stable canonical purpose name used in diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TemperateSurface => "temperate-surface",
            Self::TemperateSubsurface => "temperate-subsurface",
            Self::TemperateBaseRock => "temperate-base-rock",
            Self::TemperateSecondaryRock => "temperate-secondary-rock",
            Self::TemperateClay => "temperate-clay",
            Self::TemperateGravel => "temperate-gravel",
            Self::WoodlandLog => "woodland-log",
            Self::WoodlandLeaves => "woodland-leaves",
            Self::WoodlandGroundCover => "woodland-ground-cover",
            Self::AridSand => "arid-sand",
            Self::AridRedSand => "arid-red-sand",
            Self::AridSandstone => "arid-sandstone",
            Self::AridRedSandstone => "arid-red-sandstone",
            Self::AridBaseRock => "arid-base-rock",
            Self::CopperResource => "copper-resource",
            Self::CoarseDirt => "coarse-dirt",
            Self::RootedDirt => "rooted-dirt",
            Self::Mud => "mud",
            Self::Silt => "silt",
            Self::Peat => "peat",
            Self::Snow => "snow",
            Self::Ice => "ice",
            Self::Moss => "moss",
            Self::Deepstone => "deepstone",
            Self::Granite => "granite",
            Self::Slate => "slate",
            Self::Tuff => "tuff",
            Self::Calcite => "calcite",
            Self::Dripstone => "dripstone",
            Self::CoalResource => "coal-resource",
            Self::IronResource => "iron-resource",
            Self::TinResource => "tin-resource",
            Self::GoldResource => "gold-resource",
            Self::SulfurResource => "sulfur-resource",
            Self::CrystalResource => "crystal-resource",
            Self::BorealLog => "boreal-log",
            Self::BorealLeaves => "boreal-leaves",
        }
    }

    /// Returns the `@terrenia/worldgen` catalog Role path for a natural purpose.
    #[must_use]
    pub const fn authored_catalog_path(self) -> Option<&'static str> {
        match self {
            Self::CoarseDirt => Some("catalog-coarse-dirt"),
            Self::RootedDirt => Some("catalog-rooted-dirt"),
            Self::Mud => Some("catalog-mud"),
            Self::Silt => Some("catalog-silt"),
            Self::Peat => Some("catalog-peat"),
            Self::Snow => Some("catalog-snow"),
            Self::Ice => Some("catalog-ice"),
            Self::Moss => Some("catalog-moss"),
            Self::Deepstone => Some("catalog-deepstone"),
            Self::Granite => Some("catalog-granite"),
            Self::Slate => Some("catalog-slate"),
            Self::Tuff => Some("catalog-tuff"),
            Self::Calcite => Some("catalog-calcite"),
            Self::Dripstone => Some("catalog-dripstone"),
            Self::CoalResource => Some("catalog-coal-ore"),
            Self::IronResource => Some("catalog-iron-ore"),
            Self::TinResource => Some("catalog-tin-ore"),
            Self::GoldResource => Some("catalog-gold-ore"),
            Self::SulfurResource => Some("catalog-sulfur-ore"),
            Self::CrystalResource => Some("catalog-crystal-cluster"),
            Self::BorealLog => Some("catalog-pine-log"),
            Self::BorealLeaves => Some("catalog-pine-leaves"),
            Self::Empty
            | Self::TemperateSurface
            | Self::TemperateSubsurface
            | Self::TemperateBaseRock
            | Self::TemperateSecondaryRock
            | Self::TemperateClay
            | Self::TemperateGravel
            | Self::WoodlandLog
            | Self::WoodlandLeaves
            | Self::WoodlandGroundCover
            | Self::AridSand
            | Self::AridRedSand
            | Self::AridSandstone
            | Self::AridRedSandstone
            | Self::AridBaseRock
            | Self::CopperResource => None,
        }
    }
}

impl std::fmt::Display for D4MaterialRoleV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Package-owned role identities assigned to each D4 generator purpose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct D4RoleVocabularyV1 {
    roles: BTreeMap<D4MaterialRoleV1, StableId>,
}

impl D4RoleVocabularyV1 {
    /// Builds an exact vocabulary containing every required D4 purpose once.
    ///
    /// # Errors
    ///
    /// Returns an error for missing or repeated purposes, reused identities,
    /// or identities whose registration kind is not `block-role`.
    pub fn new(
        entries: impl IntoIterator<Item = (D4MaterialRoleV1, StableId)>,
    ) -> WorldgenResult<Self> {
        let mut roles = BTreeMap::new();
        let mut identities = BTreeSet::new();
        for (purpose, role) in entries {
            if role.kind() != "block-role" {
                return Err(WorldgenError::InvalidRoleIdentityKind {
                    purpose: purpose.as_str(),
                    role,
                });
            }
            if roles.contains_key(&purpose) {
                return Err(WorldgenError::DuplicateRolePurpose {
                    purpose: purpose.as_str(),
                });
            }
            if !identities.insert(role.clone()) {
                return Err(WorldgenError::DuplicateRoleIdentity { role });
            }
            roles.insert(purpose, role);
        }
        for purpose in D4MaterialRoleV1::ALL {
            if !roles.contains_key(&purpose) {
                return Err(WorldgenError::MissingRolePurpose {
                    purpose: purpose.as_str(),
                });
            }
        }
        Ok(Self { roles })
    }

    /// Returns the package-owned role ID for a functional purpose.
    #[must_use]
    pub fn role(&self, purpose: D4MaterialRoleV1) -> &StableId {
        self.roles
            .get(&purpose)
            .unwrap_or_else(|| unreachable_role(purpose))
    }

    /// Iterates required purposes and role IDs in canonical purpose order.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (D4MaterialRoleV1, &StableId)> {
        D4MaterialRoleV1::ALL
            .into_iter()
            .map(|purpose| (purpose, self.role(purpose)))
    }
}

fn unreachable_role(purpose: D4MaterialRoleV1) -> &'static StableId {
    panic!(
        "validated D4RoleVocabularyV1 lost required purpose `{}`",
        purpose.as_str()
    )
}

/// Package-owned role identities assigned to each D7 natural purpose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NaturalRoleVocabularyV1 {
    roles: BTreeMap<D4MaterialRoleV1, StableId>,
}

impl NaturalRoleVocabularyV1 {
    /// Builds an exact vocabulary containing every natural purpose once.
    ///
    /// # Errors
    ///
    /// Returns an error for missing or repeated purposes, reused identities,
    /// or identities whose registration kind is not `block-role`.
    pub fn new(
        entries: impl IntoIterator<Item = (D4MaterialRoleV1, StableId)>,
    ) -> WorldgenResult<Self> {
        let mut roles = BTreeMap::new();
        let mut identities = BTreeSet::new();
        for (purpose, role) in entries {
            if !D4MaterialRoleV1::NATURAL.contains(&purpose) {
                return Err(WorldgenError::InvalidRoleIdentityKind {
                    purpose: purpose.as_str(),
                    role,
                });
            }
            if role.kind() != "block-role" {
                return Err(WorldgenError::InvalidRoleIdentityKind {
                    purpose: purpose.as_str(),
                    role,
                });
            }
            if roles.contains_key(&purpose) {
                return Err(WorldgenError::DuplicateRolePurpose {
                    purpose: purpose.as_str(),
                });
            }
            if !identities.insert(role.clone()) {
                return Err(WorldgenError::DuplicateRoleIdentity { role });
            }
            roles.insert(purpose, role);
        }
        for purpose in D4MaterialRoleV1::NATURAL {
            if !roles.contains_key(&purpose) {
                return Err(WorldgenError::MissingRolePurpose {
                    purpose: purpose.as_str(),
                });
            }
        }
        Ok(Self { roles })
    }

    /// Returns the package-owned role ID for a natural purpose.
    #[must_use]
    pub fn role(&self, purpose: D4MaterialRoleV1) -> &StableId {
        self.roles
            .get(&purpose)
            .unwrap_or_else(|| unreachable_natural_role(purpose))
    }

    /// Iterates natural purposes and role IDs in canonical purpose order.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (D4MaterialRoleV1, &StableId)> {
        D4MaterialRoleV1::NATURAL
            .into_iter()
            .map(|purpose| (purpose, self.role(purpose)))
    }
}

fn unreachable_natural_role(purpose: D4MaterialRoleV1) -> &'static StableId {
    panic!(
        "validated NaturalRoleVocabularyV1 lost required purpose `{}`",
        purpose.as_str()
    )
}

/// Frozen semantic Role-to-concrete-block bindings supplied by registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenRoleBindingsV1 {
    bindings: BTreeMap<StableId, StableId>,
}

impl FrozenRoleBindingsV1 {
    /// Validates and canonicalizes role bindings.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate roles, non-`block-role` keys, or
    /// non-`block` targets.
    pub fn new(entries: impl IntoIterator<Item = (StableId, StableId)>) -> WorldgenResult<Self> {
        let mut bindings = BTreeMap::new();
        let mut entries_seen = 0_usize;
        for (role, target) in entries {
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen > D4_MAX_ROLE_BINDING_COUNT {
                return Err(WorldgenError::CollectionLimitExceeded {
                    kind: "frozen role bindings",
                    actual: entries_seen,
                    limit: D4_MAX_ROLE_BINDING_COUNT,
                });
            }
            if role.kind() != "block-role" {
                return Err(WorldgenError::InvalidRoleIdentityKind {
                    purpose: "frozen-binding",
                    role,
                });
            }
            if target.kind() != "block" {
                return Err(WorldgenError::InvalidRoleTargetKind {
                    role,
                    target: Box::new(target),
                });
            }
            if bindings.contains_key(&role) {
                return Err(WorldgenError::DuplicateRoleBinding { role });
            }
            bindings.insert(role, target);
        }
        Ok(Self { bindings })
    }

    /// Returns the concrete block target for a role, when present.
    #[must_use]
    pub fn target(&self, role: &StableId) -> Option<&StableId> {
        self.bindings.get(role)
    }

    /// Iterates frozen Role-to-block bindings in canonical identity order.
    #[must_use]
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = (&StableId, &StableId)> {
        self.bindings.iter()
    }

    /// Returns the exact frozen binding count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Returns whether this frozen binding collection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

/// Externally owned content definitions available to the D4 worldgen slice.
///
/// The crate validates only stable identities and closure membership. Block
/// schemas, definitions, namespace grants, and numeric registration IDs remain
/// owned by package composition and registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct D4BlockCatalogClosureV1 {
    blocks: Vec<StableId>,
}

impl D4BlockCatalogClosureV1 {
    /// Validates at least 18 unique block identities into canonical order.
    ///
    /// # Errors
    ///
    /// Returns an error for a too-small closure, duplicate identity, or a
    /// registration kind other than `block`.
    pub fn new(blocks: impl IntoIterator<Item = StableId>) -> WorldgenResult<Self> {
        let mut bounded_blocks = Vec::new();
        for block in blocks {
            let actual = bounded_blocks.len().saturating_add(1);
            if actual > D4_MAX_CATALOG_BLOCK_COUNT {
                return Err(WorldgenError::CollectionLimitExceeded {
                    kind: "D4 catalog blocks",
                    actual,
                    limit: D4_MAX_CATALOG_BLOCK_COUNT,
                });
            }
            bounded_blocks.push(block);
        }
        if bounded_blocks.len() < D4_MINIMUM_BLOCK_COUNT {
            return Err(WorldgenError::IncompleteCatalogClosure {
                minimum: D4_MINIMUM_BLOCK_COUNT,
                actual: bounded_blocks.len(),
            });
        }
        for block in &bounded_blocks {
            if block.kind() != "block" {
                return Err(WorldgenError::InvalidCatalogBlockKind {
                    content: block.clone(),
                });
            }
        }
        let mut blocks = bounded_blocks;
        blocks.sort();
        for pair in blocks.windows(2) {
            if pair[0] == pair[1] {
                return Err(WorldgenError::DuplicateCatalogBlock {
                    block: pair[0].clone(),
                });
            }
        }
        Ok(Self { blocks })
    }

    /// Returns canonically sorted package-owned block identities.
    #[must_use]
    pub fn blocks(&self) -> &[StableId] {
        &self.blocks
    }

    /// Returns whether the closure contains a concrete block identity.
    #[must_use]
    pub fn contains(&self, block: &StableId) -> bool {
        self.blocks.binary_search(block).is_ok()
    }
}
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn frozen_binding_constructor_rejects_absolute_boundary_plus_one_early() {
        let at_limit = (0..D4_MAX_ROLE_BINDING_COUNT).map(binding);
        assert_eq!(
            FrozenRoleBindingsV1::new(at_limit)
                .expect("the documented absolute binding boundary is valid")
                .len(),
            D4_MAX_ROLE_BINDING_COUNT
        );
        assert!(matches!(
            FrozenRoleBindingsV1::new((0..=D4_MAX_ROLE_BINDING_COUNT).map(binding)),
            Err(WorldgenError::CollectionLimitExceeded {
                kind: "frozen role bindings",
                actual,
                limit: D4_MAX_ROLE_BINDING_COUNT,
            }) if actual == D4_MAX_ROLE_BINDING_COUNT + 1
        ));
    }

    #[test]
    fn catalog_constructor_rejects_absolute_boundary_plus_one_before_sorting() {
        assert_eq!(
            D4BlockCatalogClosureV1::new((0..D4_MAX_CATALOG_BLOCK_COUNT).map(block))
                .expect("the documented absolute catalog boundary is valid")
                .blocks()
                .len(),
            D4_MAX_CATALOG_BLOCK_COUNT
        );
        assert!(matches!(
            D4BlockCatalogClosureV1::new((0..=D4_MAX_CATALOG_BLOCK_COUNT).map(block)),
            Err(WorldgenError::CollectionLimitExceeded {
                kind: "D4 catalog blocks",
                actual,
                limit: D4_MAX_CATALOG_BLOCK_COUNT,
            }) if actual == D4_MAX_CATALOG_BLOCK_COUNT + 1
        ));
    }

    fn binding(index: usize) -> (StableId, StableId) {
        (
            stable_id(&format!("fixture:block-role/bounded/role-{index}@1")),
            stable_id(&format!("fixture:block/bounded/block-{index}")),
        )
    }

    fn block(index: usize) -> StableId {
        stable_id(&format!("fixture:block/bounded/catalog-{index}"))
    }

    fn stable_id(value: &str) -> StableId {
        StableId::from_str(value).expect("test stable IDs follow the canonical grammar")
    }
}
