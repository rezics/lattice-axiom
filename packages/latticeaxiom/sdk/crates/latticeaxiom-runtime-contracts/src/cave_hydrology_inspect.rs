//! Deterministic V6 cave/hydrology command-path inspect fixtures.
//!
//! These contracts describe a bounded headless command sequence from a surface
//! spawn through a required cave entrance into both underground territories,
//! collecting three frozen resource classes. They do not inject player input,
//! stream chunks, or change authoritative world state.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, IdentifierError, SchemaId, StableId, canonical_json_bytes,
    canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{WorldgenInspectError, WorldgenInspectKindV1};

/// Canonical schema of a cave command-path fixture.
pub const CAVE_COMMAND_PATH_SCHEMA_V1: &str = "latticeaxiom:schema/cave-command-path@1";

/// Hard cap on waypoints retained by one command-path fixture.
pub const MAX_CAVE_COMMAND_WAYPOINTS: usize = 16;
/// Hard cap on command steps retained by one command-path fixture.
pub const MAX_CAVE_COMMAND_STEPS: usize = 32;
/// V6 underground territories that the path must enter.
pub const REQUIRED_UNDERGROUND_TERRITORIES: usize = 2;
/// V6 natural resource classes the path must obtain.
pub const REQUIRED_RESOURCE_CLASSES: usize = 3;

/// Closed waypoint stages on the V6 cave command path.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaveCommandStageV1 {
    /// Validated surface spawn.
    Surface,
    /// Required cave entrance.
    Entrance,
    /// Underground topology domain.
    Underground,
    /// Resource-class gather location.
    Resource,
}

/// Closed command-path actions. Numeric player enums stay in the player crate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaveCommandActionV1 {
    /// Walk toward the next waypoint.
    Move,
    /// Look at the targeted voxel.
    Look,
    /// Inspect the targeted voxel without mutation.
    Inspect,
    /// Break the targeted resource voxel.
    BreakBlock,
}

/// One bounded waypoint on the cave command path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveCommandWaypointV1 {
    /// Stable waypoint identity.
    pub id: StableId,
    /// Path stage.
    pub stage: CaveCommandStageV1,
    /// Planning-cell x; may be negative.
    pub cell_x: i64,
    /// Planning-cell z; may be negative.
    pub cell_z: i64,
    /// World voxel x.
    pub voxel_x: i64,
    /// World voxel y.
    pub voxel_y: i64,
    /// World voxel z.
    pub voxel_z: i64,
    /// Surface biome or underground topology domain at this waypoint.
    pub territory: StableId,
}

/// One obtained resource class bound to frozen Role/Predicate/block IDs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveCommandResourceClassV1 {
    /// Resource-class identity (`wood`, `stone`, or `ore` path).
    pub class: StableId,
    /// Frozen placement Role.
    pub role: StableId,
    /// Frozen Predicate that accepted the gather.
    pub predicate: StableId,
    /// Concrete block bound from the Role.
    pub candidate: StableId,
    /// Planning-cell x of the gather.
    pub cell_x: i64,
    /// Planning-cell z of the gather.
    pub cell_z: i64,
    /// World voxel x of the gather.
    pub voxel_x: i64,
    /// World voxel y of the gather.
    pub voxel_y: i64,
    /// World voxel z of the gather.
    pub voxel_z: i64,
}

/// One bounded headless command step. Integrators wire this to player input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveCommandStepV1 {
    /// Closed action.
    pub action: CaveCommandActionV1,
    /// Waypoint this step occupies or targets.
    pub waypoint: StableId,
    /// Optional targeted content identity.
    pub target: Option<StableId>,
}

/// Deterministic V6 command-path fixture from surface into both caves.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveCommandPathFixtureV1 {
    /// Fixture schema identity.
    pub schema: SchemaId,
    /// Fixture schema version, currently one.
    pub schema_version: u32,
    /// Output-affecting generation input hash.
    pub generation_input_hash: CanonicalHash,
    /// Package/artifact-aware generation provenance hash.
    pub generation_provenance_hash: CanonicalHash,
    /// Planning-cell origin x; the V6 fixture uses a negative origin.
    pub origin_cell_x: i64,
    /// Planning-cell origin z; the V6 fixture uses a negative origin.
    pub origin_cell_z: i64,
    /// Required surface-entrance identity.
    pub entrance: StableId,
    /// Ordered waypoints covering surface, entrance, and both underground domains.
    pub waypoints: Vec<CaveCommandWaypointV1>,
    /// Two underground topology domains, stored in canonical order.
    pub underground_territories: Vec<StableId>,
    /// Three resource classes obtained along the path.
    pub resource_classes: Vec<CaveCommandResourceClassV1>,
    /// Bounded command sequence. Production wiring is left to the integrator.
    pub commands: Vec<CaveCommandStepV1>,
}

impl CaveCommandPathFixtureV1 {
    /// Validates coverage, bounds, frozen identity kinds, and canonical order.
    ///
    /// # Errors
    ///
    /// Returns [`CaveCommandPathError`] when the fixture is empty, over budget,
    /// missing a required stage, or not HashMap-stable.
    pub fn validate(&self) -> Result<(), CaveCommandPathError> {
        self.validate_envelope()?;
        let waypoint_ids = self.validate_waypoints()?;
        self.validate_resources()?;
        self.validate_commands(&waypoint_ids)?;
        Ok(())
    }

    fn validate_envelope(&self) -> Result<(), CaveCommandPathError> {
        if self.schema.to_string() != CAVE_COMMAND_PATH_SCHEMA_V1 {
            return Err(CaveCommandPathError::UnexpectedSchema {
                observed: self.schema.to_string(),
            });
        }
        if self.schema_version != 1 {
            return Err(CaveCommandPathError::UnsupportedSchemaVersion {
                observed: self.schema_version,
            });
        }
        if self.waypoints.is_empty() || self.waypoints.len() > MAX_CAVE_COMMAND_WAYPOINTS {
            return Err(CaveCommandPathError::WaypointCount {
                observed: self.waypoints.len(),
                maximum: MAX_CAVE_COMMAND_WAYPOINTS,
            });
        }
        if self.commands.is_empty() || self.commands.len() > MAX_CAVE_COMMAND_STEPS {
            return Err(CaveCommandPathError::CommandCount {
                observed: self.commands.len(),
                maximum: MAX_CAVE_COMMAND_STEPS,
            });
        }
        if self.underground_territories.len() != REQUIRED_UNDERGROUND_TERRITORIES
            || self
                .underground_territories
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(CaveCommandPathError::UndergroundCoverage);
        }
        if self.resource_classes.len() != REQUIRED_RESOURCE_CLASSES
            || self
                .resource_classes
                .windows(2)
                .any(|pair| pair[0].class >= pair[1].class)
        {
            return Err(CaveCommandPathError::ResourceCoverage);
        }
        Ok(())
    }

    fn validate_waypoints(
        &self,
    ) -> Result<BTreeMap<StableId, &CaveCommandWaypointV1>, CaveCommandPathError> {
        let mut waypoint_ids = BTreeMap::new();
        let mut stages = BTreeSet::new();
        let mut visited_underground = BTreeSet::new();
        for waypoint in &self.waypoints {
            if waypoint_ids.insert(waypoint.id.clone(), waypoint).is_some() {
                return Err(CaveCommandPathError::DuplicateWaypoint {
                    id: waypoint.id.clone(),
                });
            }
            stages.insert(waypoint.stage);
            if waypoint.stage == CaveCommandStageV1::Underground {
                visited_underground.insert(waypoint.territory.clone());
            }
        }
        if !stages.contains(&CaveCommandStageV1::Surface)
            || !stages.contains(&CaveCommandStageV1::Entrance)
            || !stages.contains(&CaveCommandStageV1::Underground)
            || !stages.contains(&CaveCommandStageV1::Resource)
        {
            return Err(CaveCommandPathError::MissingStage);
        }
        if visited_underground.len() != REQUIRED_UNDERGROUND_TERRITORIES
            || visited_underground
                .iter()
                .ne(self.underground_territories.iter())
        {
            return Err(CaveCommandPathError::UndergroundCoverage);
        }
        if !waypoint_ids.contains_key(&self.entrance) {
            return Err(CaveCommandPathError::UnknownWaypoint {
                id: self.entrance.clone(),
            });
        }
        if waypoint_ids[&self.entrance].stage != CaveCommandStageV1::Entrance {
            return Err(CaveCommandPathError::EntranceStage {
                id: self.entrance.clone(),
            });
        }
        Ok(waypoint_ids)
    }

    fn validate_resources(&self) -> Result<(), CaveCommandPathError> {
        let mut resource_classes = BTreeSet::new();
        for resource in &self.resource_classes {
            if resource.class.kind() != "resource-class" {
                return Err(CaveCommandPathError::InvalidStableKind {
                    value: resource.class.clone(),
                    expected: "resource-class",
                });
            }
            if resource.role.kind() != "block-role" {
                return Err(CaveCommandPathError::InvalidStableKind {
                    value: resource.role.clone(),
                    expected: "block-role",
                });
            }
            if resource.predicate.kind() != "predicate" {
                return Err(CaveCommandPathError::InvalidStableKind {
                    value: resource.predicate.clone(),
                    expected: "predicate",
                });
            }
            if !resource_classes.insert(resource.class.clone()) {
                return Err(CaveCommandPathError::DuplicateResourceClass {
                    class: resource.class.clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_commands(
        &self,
        waypoint_ids: &BTreeMap<StableId, &CaveCommandWaypointV1>,
    ) -> Result<(), CaveCommandPathError> {
        let mut obtained = BTreeSet::new();
        let mut entered = BTreeSet::new();
        for step in &self.commands {
            let waypoint = waypoint_ids.get(&step.waypoint).ok_or_else(|| {
                CaveCommandPathError::UnknownWaypoint {
                    id: step.waypoint.clone(),
                }
            })?;
            if waypoint.stage == CaveCommandStageV1::Underground {
                entered.insert(waypoint.territory.clone());
            }
            if step.action == CaveCommandActionV1::BreakBlock {
                let Some(target) = &step.target else {
                    return Err(CaveCommandPathError::MissingBreakTarget {
                        waypoint: step.waypoint.clone(),
                    });
                };
                if let Some(resource) = self
                    .resource_classes
                    .iter()
                    .find(|resource| resource.candidate == *target)
                {
                    obtained.insert(resource.class.clone());
                }
            }
        }
        if entered.len() != REQUIRED_UNDERGROUND_TERRITORIES {
            return Err(CaveCommandPathError::UndergroundCoverage);
        }
        if obtained.len() != REQUIRED_RESOURCE_CLASSES {
            return Err(CaveCommandPathError::ResourceCoverage);
        }
        Ok(())
    }

    /// Returns the V6 inspect kinds this fixture is expected to exercise.
    #[must_use]
    pub fn inspect_kinds() -> BTreeSet<WorldgenInspectKindV1> {
        BTreeSet::from(WorldgenInspectKindV1::CAVE_HYDROLOGY)
    }

    /// Returns canonical compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }

    /// Returns the hash of canonical fixture bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(self)
    }
}

/// Invalid cave command-path fixture.
#[derive(Debug, Error)]
pub enum CaveCommandPathError {
    /// Schema identity did not match the compiled-in V6 fixture schema.
    #[error("cave command path schema `{observed}` is not `{CAVE_COMMAND_PATH_SCHEMA_V1}`")]
    UnexpectedSchema {
        /// Observed schema text.
        observed: String,
    },
    /// Schema version was not one.
    #[error("cave command path schema version {observed} is unsupported")]
    UnsupportedSchemaVersion {
        /// Observed version.
        observed: u32,
    },
    /// Waypoint count was zero or above the hard cap.
    #[error("cave command path has {observed} waypoints; maximum is {maximum}")]
    WaypointCount {
        /// Observed count.
        observed: usize,
        /// Hard maximum.
        maximum: usize,
    },
    /// Command count was zero or above the hard cap.
    #[error("cave command path has {observed} commands; maximum is {maximum}")]
    CommandCount {
        /// Observed count.
        observed: usize,
        /// Hard maximum.
        maximum: usize,
    },
    /// Two waypoints shared one identity.
    #[error("cave command path repeats waypoint `{id}`")]
    DuplicateWaypoint {
        /// Repeated identity.
        id: StableId,
    },
    /// Surface, entrance, underground, or resource stage was missing.
    #[error("cave command path is missing a required stage")]
    MissingStage,
    /// Underground territory coverage was not two unique sorted domains.
    #[error("cave command path must enter two sorted underground territories")]
    UndergroundCoverage,
    /// Resource-class coverage was not three unique sorted classes.
    #[error("cave command path must obtain three sorted resource classes")]
    ResourceCoverage,
    /// Entrance identity did not name an entrance-stage waypoint.
    #[error("cave command path entrance `{id}` is not an entrance waypoint")]
    EntranceStage {
        /// Rejected identity.
        id: StableId,
    },
    /// A command or entrance referred to an unknown waypoint.
    #[error("cave command path refers to unknown waypoint `{id}`")]
    UnknownWaypoint {
        /// Missing identity.
        id: StableId,
    },
    /// A stable identity used the wrong registration kind.
    #[error("cave command path identity `{value}` must use kind `{expected}`")]
    InvalidStableKind {
        /// Rejected identity.
        value: StableId,
        /// Expected registration kind.
        expected: &'static str,
    },
    /// Two resource rows shared one class identity.
    #[error("cave command path repeats resource class `{class}`")]
    DuplicateResourceClass {
        /// Repeated class.
        class: StableId,
    },
    /// A break command omitted its resource candidate.
    #[error("cave command path break at `{waypoint}` is missing a target")]
    MissingBreakTarget {
        /// Waypoint identity.
        waypoint: StableId,
    },
    /// A compiled-in identifier failed canonical parsing.
    #[error("invalid cave command path identifier `{value}`: {source}")]
    InvalidIdentifier {
        /// Rejected text.
        value: String,
        /// Identifier grammar error.
        #[source]
        source: IdentifierError,
    },
    /// Worldgen inspect projection of the path failed.
    #[error(transparent)]
    Inspect(#[from] WorldgenInspectError),
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
}

/// Parses the compiled-in command-path schema identity.
///
/// # Errors
///
/// Returns [`CaveCommandPathError::InvalidIdentifier`] if the compiled-in
/// schema ID is not a canonical [`SchemaId`].
pub fn cave_command_path_schema() -> Result<SchemaId, CaveCommandPathError> {
    CAVE_COMMAND_PATH_SCHEMA_V1
        .parse()
        .map_err(|source| CaveCommandPathError::InvalidIdentifier {
            value: CAVE_COMMAND_PATH_SCHEMA_V1.to_owned(),
            source,
        })
}
