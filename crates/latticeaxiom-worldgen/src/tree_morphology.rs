//! Deterministic, bounded tree morphology for the natural vegetation layer.

use crate::{
    D4MaterialRoleV1, TerrainStyleV1, WorldgenSeedRootV2,
    hashes::{hash_u64, sample_hash_2d, sample_hash_3d},
};

const TREE_ELIGIBILITY_DOMAIN: &[u8] = b"latticeaxiom.tree-eligibility.v1\0";
const TREE_SPECIES_DOMAIN: &[u8] = b"latticeaxiom.tree-species.v1\0";
const TREE_ARCHETYPE_DOMAIN: &[u8] = b"latticeaxiom.tree-archetype.v1\0";
const TREE_DIMENSIONS_DOMAIN: &[u8] = b"latticeaxiom.tree-dimensions.v1\0";
const TREE_CANOPY_DOMAIN: &[u8] = b"latticeaxiom.tree-canopy.v1\0";
const TREE_BRANCH_DOMAIN: &[u8] = b"latticeaxiom.tree-branch.v1\0";
const TREE_PRIORITY_DOMAIN: &[u8] = b"latticeaxiom.tree-priority.v1\0";

pub(crate) const MAX_TREE_RADIUS_VOXELS: i64 = 3;
pub(crate) const MAX_TREE_HEIGHT_VOXELS: i64 = 10;
pub(crate) const MAX_TREE_BLUEPRINT_VOXELS: usize = 512;
pub(crate) const MIN_TREE_EXCLUSION_RADIUS_VOXELS: u16 = 6;
const MAX_TREE_BRANCHES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum TreeSpeciesV1 {
    Oak,
    Pine,
}

impl TreeSpeciesV1 {
    pub(crate) const fn roles(self) -> (D4MaterialRoleV1, D4MaterialRoleV1) {
        match self {
            Self::Oak => (
                D4MaterialRoleV1::WoodlandLog,
                D4MaterialRoleV1::WoodlandLeaves,
            ),
            Self::Pine => (D4MaterialRoleV1::BorealLog, D4MaterialRoleV1::BorealLeaves),
        }
    }

    const fn discriminant(self) -> i64 {
        match self {
            Self::Oak => 0,
            Self::Pine => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum TreeArchetypeV1 {
    OakRound,
    OakTall,
    OakBranched,
    PineConical,
    PineTall,
    PineOldGrowth,
}

impl TreeArchetypeV1 {
    #[cfg(test)]
    const ALL: [Self; 6] = [
        Self::OakRound,
        Self::OakTall,
        Self::OakBranched,
        Self::PineConical,
        Self::PineTall,
        Self::PineOldGrowth,
    ];

    const fn spec(self) -> TreeArchetypeSpecV1 {
        match self {
            Self::OakRound => TreeArchetypeSpecV1 {
                species: TreeSpeciesV1::Oak,
                trunk_height: 5..=6,
                crown_start: CrownStartPolicyV1::Fixed(3),
                crown_radius: 2,
                branch_count: 0..=0,
                branch_length: 0,
                crown_profile: TreeCrownProfileV1::Round,
            },
            Self::OakTall => TreeArchetypeSpecV1 {
                species: TreeSpeciesV1::Oak,
                trunk_height: 7..=8,
                crown_start: CrownStartPolicyV1::BelowTrunk(3),
                crown_radius: 2,
                branch_count: 0..=0,
                branch_length: 0,
                crown_profile: TreeCrownProfileV1::TallOval,
            },
            Self::OakBranched => TreeArchetypeSpecV1 {
                species: TreeSpeciesV1::Oak,
                trunk_height: 6..=7,
                crown_start: CrownStartPolicyV1::Fixed(4),
                crown_radius: 3,
                branch_count: 2..=3,
                branch_length: 2,
                crown_profile: TreeCrownProfileV1::Branched,
            },
            Self::PineConical => TreeArchetypeSpecV1 {
                species: TreeSpeciesV1::Pine,
                trunk_height: 6..=7,
                crown_start: CrownStartPolicyV1::Fixed(2),
                crown_radius: 3,
                branch_count: 0..=0,
                branch_length: 0,
                crown_profile: TreeCrownProfileV1::Conical,
            },
            Self::PineTall => TreeArchetypeSpecV1 {
                species: TreeSpeciesV1::Pine,
                trunk_height: 8..=9,
                crown_start: CrownStartPolicyV1::Fixed(4),
                crown_radius: 2,
                branch_count: 0..=0,
                branch_length: 0,
                crown_profile: TreeCrownProfileV1::Spired,
            },
            Self::PineOldGrowth => TreeArchetypeSpecV1 {
                species: TreeSpeciesV1::Pine,
                trunk_height: 8..=9,
                crown_start: CrownStartPolicyV1::Fixed(3),
                crown_radius: 3,
                branch_count: 3..=4,
                branch_length: 2,
                crown_profile: TreeCrownProfileV1::Tiered,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TreeArchetypeSpecV1 {
    species: TreeSpeciesV1,
    trunk_height: std::ops::RangeInclusive<u8>,
    crown_start: CrownStartPolicyV1,
    crown_radius: u8,
    branch_count: std::ops::RangeInclusive<u8>,
    branch_length: u8,
    crown_profile: TreeCrownProfileV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CrownStartPolicyV1 {
    Fixed(u8),
    BelowTrunk(u8),
}

impl CrownStartPolicyV1 {
    fn resolve(self, trunk_height: u8) -> Option<u8> {
        match self {
            Self::Fixed(value) => Some(value),
            Self::BelowTrunk(offset) => trunk_height.checked_sub(offset),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TreeCrownProfileV1 {
    Round,
    TallOval,
    Branched,
    Conical,
    Spired,
    Tiered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TreeBranchDirectionV1 {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl TreeBranchDirectionV1 {
    const ALL: [Self; 8] = [
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
        Self::NorthWest,
    ];

    const fn offset(self) -> (i64, i64) {
        match self {
            Self::North => (0, -1),
            Self::NorthEast => (1, -1),
            Self::East => (1, 0),
            Self::SouthEast => (1, 1),
            Self::South => (0, 1),
            Self::SouthWest => (-1, 1),
            Self::West => (-1, 0),
            Self::NorthWest => (-1, -1),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TreeBranchV1 {
    start_y: u8,
    direction: TreeBranchDirectionV1,
    length: u8,
    rise: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TreeDescriptorV1 {
    species: TreeSpeciesV1,
    archetype: TreeArchetypeV1,
    trunk_height: u8,
    crown_start: u8,
    crown_radius: u8,
    crown_profile: TreeCrownProfileV1,
    branches: [Option<TreeBranchV1>; MAX_TREE_BRANCHES],
}

impl TreeDescriptorV1 {
    pub(crate) const fn species(&self) -> TreeSpeciesV1 {
        self.species
    }

    pub(crate) const fn archetype(&self) -> TreeArchetypeV1 {
        self.archetype
    }

    pub(crate) fn footprint_radius(&self) -> i64 {
        i64::from(self.crown_radius)
    }

    #[cfg(test)]
    const fn trunk_height(&self) -> u8 {
        self.trunk_height
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum TreeVoxelRoleV1 {
    Log,
    Leaves,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct TreeVoxelV1 {
    x: i64,
    y: i64,
    z: i64,
    role: TreeVoxelRoleV1,
}

impl TreeVoxelV1 {
    pub(crate) const fn x(self) -> i64 {
        self.x
    }

    pub(crate) const fn y(self) -> i64 {
        self.y
    }

    pub(crate) const fn z(self) -> i64 {
        self.z
    }

    pub(crate) const fn role(self) -> TreeVoxelRoleV1 {
        self.role
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TreeBlueprintV1 {
    anchor_x: i64,
    support_y: i64,
    anchor_z: i64,
    species: TreeSpeciesV1,
    archetype: TreeArchetypeV1,
    priority: u64,
    voxels: Vec<TreeVoxelV1>,
    occupied_columns: Vec<(i64, i64)>,
}

impl TreeBlueprintV1 {
    pub(crate) const fn species(&self) -> TreeSpeciesV1 {
        self.species
    }

    pub(crate) const fn archetype(&self) -> TreeArchetypeV1 {
        self.archetype
    }

    pub(crate) const fn priority(&self) -> (u64, i64, i64) {
        (self.priority, self.anchor_x, self.anchor_z)
    }

    pub(crate) fn voxels(&self) -> &[TreeVoxelV1] {
        &self.voxels
    }

    pub(crate) fn occupied_columns(&self) -> &[(i64, i64)] {
        &self.occupied_columns
    }

    pub(crate) const fn support_y(&self) -> i64 {
        self.support_y
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TreeMorphologySamplerV1 {
    eligibility: u64,
    species: u64,
    archetype: u64,
    dimensions: u64,
    canopy: u64,
    branch: u64,
    priority: u64,
}

impl TreeMorphologySamplerV1 {
    pub(crate) fn new(seed_root: WorldgenSeedRootV2) -> Self {
        Self {
            eligibility: domain_seed(TREE_ELIGIBILITY_DOMAIN, seed_root),
            species: domain_seed(TREE_SPECIES_DOMAIN, seed_root),
            archetype: domain_seed(TREE_ARCHETYPE_DOMAIN, seed_root),
            dimensions: domain_seed(TREE_DIMENSIONS_DOMAIN, seed_root),
            canopy: domain_seed(TREE_CANOPY_DOMAIN, seed_root),
            branch: domain_seed(TREE_BRANCH_DOMAIN, seed_root),
            priority: domain_seed(TREE_PRIORITY_DOMAIN, seed_root),
        }
    }

    pub(crate) fn is_eligible(&self, x: i64, z: i64, threshold_per_1024: u16) -> bool {
        sample_hash_2d(self.eligibility, x, z) % 1_024 < u64::from(threshold_per_1024)
    }

    pub(crate) fn priority(&self, x: i64, z: i64) -> u64 {
        sample_hash_2d(self.priority, x, z)
    }

    pub(crate) fn descriptor(
        &self,
        x: i64,
        z: i64,
        style: TerrainStyleV1,
    ) -> Option<TreeDescriptorV1> {
        let species = self.species(x, z, style)?;
        let archetype = self.archetype(x, z, species);
        let spec = archetype.spec();
        if spec.species != species {
            return None;
        }
        let trunk_height = sample_inclusive(
            sample_hash_3d(self.dimensions, x, 0, z),
            *spec.trunk_height.start(),
            *spec.trunk_height.end(),
        )?;
        let crown_start = spec.crown_start.resolve(trunk_height)?;
        let branch_count = sample_inclusive(
            sample_hash_3d(self.dimensions, x, 1, z),
            *spec.branch_count.start(),
            *spec.branch_count.end(),
        )?;
        let branches = self.branches(
            x,
            z,
            crown_start,
            trunk_height,
            branch_count,
            spec.branch_length,
        )?;
        let descriptor = TreeDescriptorV1 {
            species,
            archetype,
            trunk_height,
            crown_start,
            crown_radius: spec.crown_radius,
            crown_profile: spec.crown_profile,
            branches,
        };
        descriptor_is_bounded(&descriptor).then_some(descriptor)
    }

    pub(crate) fn blueprint(
        &self,
        anchor_x: i64,
        support_y: i64,
        anchor_z: i64,
        descriptor: &TreeDescriptorV1,
    ) -> Option<TreeBlueprintV1> {
        let anchor_canopy_seed = sample_hash_2d(self.canopy, anchor_x, anchor_z);
        let mut local = Vec::with_capacity(192);
        for y in 1..=i64::from(descriptor.trunk_height) {
            local.push(LocalTreeVoxelV1::new(0, y, 0, TreeVoxelRoleV1::Log));
        }
        add_primary_crown(&mut local, descriptor, anchor_canopy_seed);
        for branch in descriptor.branches.iter().flatten().copied() {
            add_branch(&mut local, branch, anchor_canopy_seed);
        }
        local.sort_unstable_by_key(|voxel| (voxel.y, voxel.z, voxel.x, voxel.role));
        local.dedup_by(|right, left| left.x == right.x && left.y == right.y && left.z == right.z);
        if local.is_empty() || local.len() > MAX_TREE_BLUEPRINT_VOXELS {
            return None;
        }
        if local.iter().any(|voxel| !local_voxel_is_bounded(*voxel)) {
            return None;
        }
        if !local.iter().any(|voxel| {
            voxel.x == 0 && voxel.y == 1 && voxel.z == 0 && voxel.role == TreeVoxelRoleV1::Log
        }) {
            return None;
        }
        let voxels = local
            .into_iter()
            .map(|voxel| TreeVoxelV1 {
                x: anchor_x.saturating_add(voxel.x),
                y: support_y.saturating_add(voxel.y),
                z: anchor_z.saturating_add(voxel.z),
                role: voxel.role,
            })
            .collect::<Vec<_>>();
        let mut occupied_columns = voxels
            .iter()
            .map(|voxel| (voxel.x, voxel.z))
            .collect::<Vec<_>>();
        occupied_columns.sort_unstable();
        occupied_columns.dedup();
        Some(TreeBlueprintV1 {
            anchor_x,
            support_y,
            anchor_z,
            species: descriptor.species,
            archetype: descriptor.archetype,
            priority: self.priority(anchor_x, anchor_z),
            voxels,
            occupied_columns,
        })
    }

    fn species(&self, x: i64, z: i64, style: TerrainStyleV1) -> Option<TreeSpeciesV1> {
        let minority = sample_hash_2d(self.species, x, z).is_multiple_of(8);
        match style {
            TerrainStyleV1::TemperateWoodland => Some(if minority {
                TreeSpeciesV1::Pine
            } else {
                TreeSpeciesV1::Oak
            }),
            TerrainStyleV1::BorealWetland => Some(if minority {
                TreeSpeciesV1::Oak
            } else {
                TreeSpeciesV1::Pine
            }),
            TerrainStyleV1::Marine | TerrainStyleV1::AridBadlands => None,
        }
    }

    fn archetype(&self, x: i64, z: i64, species: TreeSpeciesV1) -> TreeArchetypeV1 {
        let roll = sample_hash_3d(self.archetype, x, species.discriminant(), z) % 3;
        match (species, roll) {
            (TreeSpeciesV1::Oak, 0) => TreeArchetypeV1::OakRound,
            (TreeSpeciesV1::Oak, 1) => TreeArchetypeV1::OakTall,
            (TreeSpeciesV1::Oak, _) => TreeArchetypeV1::OakBranched,
            (TreeSpeciesV1::Pine, 0) => TreeArchetypeV1::PineConical,
            (TreeSpeciesV1::Pine, 1) => TreeArchetypeV1::PineTall,
            (TreeSpeciesV1::Pine, _) => TreeArchetypeV1::PineOldGrowth,
        }
    }

    fn branches(
        &self,
        x: i64,
        z: i64,
        crown_start: u8,
        trunk_height: u8,
        count: u8,
        length: u8,
    ) -> Option<[Option<TreeBranchV1>; MAX_TREE_BRANCHES]> {
        if usize::from(count) > MAX_TREE_BRANCHES {
            return None;
        }
        let mut branches = [None; MAX_TREE_BRANCHES];
        if count == 0 {
            return Some(branches);
        }
        let base_roll = sample_hash_3d(self.branch, x, 0, z);
        let base = usize::try_from(base_roll % 8).ok()?;
        let stride = if sample_hash_3d(self.branch, x, 1, z) & 1 == 0 {
            3_usize
        } else {
            5_usize
        };
        let vertical_span = trunk_height.saturating_sub(crown_start).max(1);
        for (index, slot) in branches.iter_mut().take(usize::from(count)).enumerate() {
            let direction_index = base.saturating_add(index.saturating_mul(stride)) % 8;
            let direction = *TreeBranchDirectionV1::ALL.get(direction_index)?;
            let index_u8 = u8::try_from(index).ok()?;
            let vertical_offset = index_u8.saturating_mul(2) % vertical_span;
            let start_y = crown_start
                .saturating_add(vertical_offset)
                .min(trunk_height.saturating_sub(1));
            let field = i64::try_from(index).ok()?.saturating_add(2);
            let rise = u8::from(sample_hash_3d(self.branch, x, field, z) & 1 != 0);
            *slot = Some(TreeBranchV1 {
                start_y,
                direction,
                length,
                rise,
            });
        }
        Some(branches)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LocalTreeVoxelV1 {
    x: i64,
    y: i64,
    z: i64,
    role: TreeVoxelRoleV1,
}

impl LocalTreeVoxelV1 {
    const fn new(x: i64, y: i64, z: i64, role: TreeVoxelRoleV1) -> Self {
        Self { x, y, z, role }
    }
}

fn add_primary_crown(
    voxels: &mut Vec<LocalTreeVoxelV1>,
    descriptor: &TreeDescriptorV1,
    canopy_seed: u64,
) {
    let start = i64::from(descriptor.crown_start);
    let trunk = i64::from(descriptor.trunk_height);
    let top = trunk.saturating_add(1);
    let maximum_radius = i64::from(descriptor.crown_radius);
    for y in start..=top {
        let radius = match descriptor.crown_profile {
            TreeCrownProfileV1::Round => {
                if y == start || y == top {
                    maximum_radius.saturating_sub(1).max(1)
                } else {
                    maximum_radius
                }
            }
            TreeCrownProfileV1::TallOval => {
                if y == start || y >= trunk {
                    1
                } else {
                    maximum_radius
                }
            }
            TreeCrownProfileV1::Branched => {
                if y == top {
                    1
                } else {
                    maximum_radius.saturating_sub(1).max(1)
                }
            }
            TreeCrownProfileV1::Conical => conical_radius(y, start, top, maximum_radius),
            TreeCrownProfileV1::Spired => {
                if y >= trunk {
                    1
                } else if (y.saturating_sub(start)) % 2 == 0 {
                    maximum_radius
                } else {
                    maximum_radius.saturating_sub(1).max(1)
                }
            }
            TreeCrownProfileV1::Tiered => {
                let taper = conical_radius(y, start, top, maximum_radius);
                if y >= trunk || (y.saturating_sub(start)) % 2 != 0 {
                    taper.saturating_sub(1).max(1)
                } else {
                    taper
                }
            }
        };
        add_canopy_layer(voxels, y, radius, canopy_seed);
    }
}

fn conical_radius(y: i64, start: i64, top: i64, maximum_radius: i64) -> i64 {
    let span = top.saturating_sub(start).max(1);
    let remaining = top.saturating_sub(y);
    1_i64.saturating_add(
        remaining
            .saturating_mul(maximum_radius.saturating_sub(1))
            .div_euclid(span),
    )
}

fn add_canopy_layer(voxels: &mut Vec<LocalTreeVoxelV1>, y: i64, radius: i64, canopy_seed: u64) {
    let radius_squared = radius.saturating_mul(radius);
    let inner_radius = radius.saturating_sub(1);
    let inner_squared = inner_radius.saturating_mul(inner_radius);
    for z in -radius..=radius {
        for x in -radius..=radius {
            let distance_squared = x.saturating_mul(x).saturating_add(z.saturating_mul(z));
            if distance_squared > radius_squared.saturating_add(1) {
                continue;
            }
            if distance_squared > inner_squared
                && sample_hash_3d(canopy_seed, x, y, z).is_multiple_of(5)
            {
                continue;
            }
            voxels.push(LocalTreeVoxelV1::new(x, y, z, TreeVoxelRoleV1::Leaves));
        }
    }
}

fn add_branch(voxels: &mut Vec<LocalTreeVoxelV1>, branch: TreeBranchV1, canopy_seed: u64) {
    let (direction_x, direction_z) = branch.direction.offset();
    let length = i64::from(branch.length).max(1);
    let mut endpoint = (0_i64, i64::from(branch.start_y), 0_i64);
    for step in 1..=length {
        let previous_rise = i64::from(branch.rise)
            .saturating_mul(step.saturating_sub(1))
            .div_euclid(length);
        let next_rise = i64::from(branch.rise)
            .saturating_mul(step)
            .div_euclid(length);
        let previous_y = i64::from(branch.start_y).saturating_add(previous_rise);
        endpoint = (
            direction_x.saturating_mul(step),
            i64::from(branch.start_y).saturating_add(next_rise),
            direction_z.saturating_mul(step),
        );
        if direction_x != 0 && direction_z != 0 {
            voxels.push(LocalTreeVoxelV1::new(
                endpoint.0,
                previous_y,
                direction_z.saturating_mul(step.saturating_sub(1)),
                TreeVoxelRoleV1::Log,
            ));
        }
        voxels.push(LocalTreeVoxelV1::new(
            endpoint.0,
            previous_y,
            endpoint.2,
            TreeVoxelRoleV1::Log,
        ));
        if endpoint.1 != previous_y {
            voxels.push(LocalTreeVoxelV1::new(
                endpoint.0,
                endpoint.1,
                endpoint.2,
                TreeVoxelRoleV1::Log,
            ));
        }
    }
    add_leaf_cluster(voxels, endpoint, canopy_seed);
}

fn add_leaf_cluster(voxels: &mut Vec<LocalTreeVoxelV1>, center: (i64, i64, i64), canopy_seed: u64) {
    for y in -1_i64..=1 {
        for z in -1_i64..=1 {
            for x in -1_i64..=1 {
                let distance = x.abs().saturating_add(y.abs()).saturating_add(z.abs());
                if distance > 2 {
                    continue;
                }
                let world_relative = (
                    center.0.saturating_add(x),
                    center.1.saturating_add(y),
                    center.2.saturating_add(z),
                );
                if distance == 2
                    && sample_hash_3d(
                        canopy_seed,
                        world_relative.0,
                        world_relative.1,
                        world_relative.2,
                    )
                    .is_multiple_of(4)
                {
                    continue;
                }
                voxels.push(LocalTreeVoxelV1::new(
                    world_relative.0,
                    world_relative.1,
                    world_relative.2,
                    TreeVoxelRoleV1::Leaves,
                ));
            }
        }
    }
}

fn descriptor_is_bounded(descriptor: &TreeDescriptorV1) -> bool {
    descriptor.trunk_height > 0
        && i64::from(descriptor.trunk_height).saturating_add(1) <= MAX_TREE_HEIGHT_VOXELS
        && descriptor.crown_start > 0
        && descriptor.crown_start <= descriptor.trunk_height
        && i64::from(descriptor.crown_radius) <= MAX_TREE_RADIUS_VOXELS
        && descriptor
            .branches
            .iter()
            .flatten()
            .all(|branch| branch.length <= 2 && branch.rise <= 1)
}

fn local_voxel_is_bounded(voxel: LocalTreeVoxelV1) -> bool {
    voxel.x.unsigned_abs() <= MAX_TREE_RADIUS_VOXELS.unsigned_abs()
        && voxel.z.unsigned_abs() <= MAX_TREE_RADIUS_VOXELS.unsigned_abs()
        && voxel.y >= 1
        && voxel.y <= MAX_TREE_HEIGHT_VOXELS
}

fn sample_inclusive(hash: u64, minimum: u8, maximum: u8) -> Option<u8> {
    let width = maximum.checked_sub(minimum)?.checked_add(1)?;
    let offset = u8::try_from(hash % u64::from(width)).ok()?;
    minimum.checked_add(offset)
}

fn domain_seed(domain: &[u8], seed_root: WorldgenSeedRootV2) -> u64 {
    hash_u64(domain, &[seed_root.as_bytes()])
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    use crate::WorldSeedV1;

    use super::*;

    #[test]
    fn fixed_corpus_emits_all_archetypes_and_multiple_normalized_signatures() {
        let sampler = fixture_sampler();
        let mut blueprints = BTreeMap::new();
        'coordinates: for z in -96_i64..=96 {
            for x in -96_i64..=96 {
                for style in [
                    TerrainStyleV1::TemperateWoodland,
                    TerrainStyleV1::BorealWetland,
                ] {
                    let Some(descriptor) = sampler.descriptor(x, z, style) else {
                        continue;
                    };
                    let Some(blueprint) = sampler.blueprint(x, 32, z, &descriptor) else {
                        continue;
                    };
                    blueprints
                        .entry(descriptor.archetype())
                        .or_insert(blueprint);
                    if blueprints.len() == TreeArchetypeV1::ALL.len() {
                        break 'coordinates;
                    }
                }
            }
        }

        assert_eq!(
            blueprints.keys().copied().collect::<Vec<_>>(),
            TreeArchetypeV1::ALL
        );
        assert_eq!(
            blueprints
                .values()
                .map(TreeBlueprintV1::species)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([TreeSpeciesV1::Oak, TreeSpeciesV1::Pine])
        );
        let signatures = blueprints
            .values()
            .map(normalized_signature)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            signatures.len(),
            6,
            "the frozen corpus must keep every archetype's occupancy distinct"
        );
    }

    #[test]
    fn blueprints_are_sorted_deduplicated_bounded_and_repeatable() {
        let sampler = fixture_sampler();
        for (index, archetype) in TreeArchetypeV1::ALL.into_iter().enumerate() {
            let (x, z, descriptor) = find_archetype(&sampler, archetype);
            let first = sampler
                .blueprint(x, 48, z, &descriptor)
                .expect("bounded descriptor produces a blueprint");
            let second = sampler
                .blueprint(x, 48, z, &descriptor)
                .expect("same descriptor remains valid");
            assert_eq!(first, second, "archetype index {index} changed output");
            assert_eq!(first.archetype(), archetype);
            assert_eq!(first.support_y(), 48);
            assert!(first.voxels().len() <= MAX_TREE_BLUEPRINT_VOXELS);
            assert!(first.voxels().windows(2).all(|pair| {
                (pair[0].y(), pair[0].z(), pair[0].x(), pair[0].role())
                    < (pair[1].y(), pair[1].z(), pair[1].x(), pair[1].role())
            }));
            assert!(first.voxels().iter().all(|voxel| {
                voxel.x().abs_diff(x) <= MAX_TREE_RADIUS_VOXELS.unsigned_abs()
                    && voxel.z().abs_diff(z) <= MAX_TREE_RADIUS_VOXELS.unsigned_abs()
                    && voxel.y().saturating_sub(48) >= 1
                    && voxel.y().saturating_sub(48) <= MAX_TREE_HEIGHT_VOXELS
            }));
            for relative_y in 1..=i64::from(descriptor.trunk_height()) {
                assert!(first.voxels().iter().any(|voxel| {
                    voxel.x() == x
                        && voxel.y() == 48_i64.saturating_add(relative_y)
                        && voxel.z() == z
                        && voxel.role() == TreeVoxelRoleV1::Log
                }));
            }
            assert_log_structure_is_connected(&first);
            assert!(
                first
                    .occupied_columns()
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
            );
            assert!((5..=9).contains(&descriptor.trunk_height()));
        }
    }

    #[test]
    fn biome_species_weights_are_dominant_but_not_exclusive() {
        let sampler = fixture_sampler();
        let mut temperate = BTreeMap::<TreeSpeciesV1, u64>::new();
        let mut boreal = BTreeMap::<TreeSpeciesV1, u64>::new();
        for index in 0_i64..8_192 {
            let x = index.rem_euclid(257).saturating_sub(128);
            let z = index.div_euclid(257).saturating_sub(16);
            let temperate_species = sampler
                .descriptor(x, z, TerrainStyleV1::TemperateWoodland)
                .expect("terrestrial styles select a descriptor")
                .species();
            let boreal_species = sampler
                .descriptor(x, z, TerrainStyleV1::BorealWetland)
                .expect("terrestrial styles select a descriptor")
                .species();
            *temperate.entry(temperate_species).or_default() += 1;
            *boreal.entry(boreal_species).or_default() += 1;
        }
        let temperate_oak = temperate[&TreeSpeciesV1::Oak];
        let boreal_pine = boreal[&TreeSpeciesV1::Pine];
        assert!((6_800..=7_600).contains(&temperate_oak));
        assert!((6_800..=7_600).contains(&boreal_pine));
        assert!(temperate[&TreeSpeciesV1::Pine] > 0);
        assert!(boreal[&TreeSpeciesV1::Oak] > 0);
    }

    fn fixture_sampler() -> TreeMorphologySamplerV1 {
        TreeMorphologySamplerV1::new(WorldgenSeedRootV2::from_world_seed(
            WorldSeedV1::from_integer(42),
        ))
    }

    fn find_archetype(
        sampler: &TreeMorphologySamplerV1,
        target: TreeArchetypeV1,
    ) -> (i64, i64, TreeDescriptorV1) {
        for z in -64_i64..=64 {
            for x in -64_i64..=64 {
                for style in [
                    TerrainStyleV1::TemperateWoodland,
                    TerrainStyleV1::BorealWetland,
                ] {
                    let descriptor = sampler
                        .descriptor(x, z, style)
                        .expect("terrestrial styles select a descriptor");
                    if descriptor.archetype() == target {
                        return (x, z, descriptor);
                    }
                }
            }
        }
        panic!("fixed corpus did not select {target:?}");
    }

    fn normalized_signature(blueprint: &TreeBlueprintV1) -> Vec<(i64, i64, i64, TreeVoxelRoleV1)> {
        blueprint
            .voxels()
            .iter()
            .map(|voxel| {
                (
                    voxel.x().saturating_sub(blueprint.anchor_x),
                    voxel.y().saturating_sub(blueprint.support_y),
                    voxel.z().saturating_sub(blueprint.anchor_z),
                    voxel.role(),
                )
            })
            .collect()
    }

    fn assert_log_structure_is_connected(blueprint: &TreeBlueprintV1) {
        let logs = blueprint
            .voxels()
            .iter()
            .filter(|voxel| voxel.role() == TreeVoxelRoleV1::Log)
            .map(|voxel| (voxel.x(), voxel.y(), voxel.z()))
            .collect::<BTreeSet<_>>();
        let root = (
            blueprint.anchor_x,
            blueprint.support_y.saturating_add(1),
            blueprint.anchor_z,
        );
        let mut reached = BTreeSet::from([root]);
        let mut frontier = VecDeque::from([root]);
        while let Some((x, y, z)) = frontier.pop_front() {
            for neighbor in [
                (x.saturating_sub(1), y, z),
                (x.saturating_add(1), y, z),
                (x, y.saturating_sub(1), z),
                (x, y.saturating_add(1), z),
                (x, y, z.saturating_sub(1)),
                (x, y, z.saturating_add(1)),
            ] {
                if logs.contains(&neighbor) && reached.insert(neighbor) {
                    frontier.push_back(neighbor);
                }
            }
        }
        assert_eq!(reached, logs, "every branch log must be root-connected");
    }
}
