//! Collision, selection, and inspect semantics for v1 fluid cells.
//!
//! Integrator wiring (`lib.rs`):
//! ```ignore
//! pub use fluid::fluid_semantics::{
//!     FluidCollisionKindV1, FluidInspectFragmentV1, FluidSelectionKindV1,
//!     cell_inspect_kind, classify_fluid_collision, classify_fluid_selection,
//!     fluid_cell_blocks_collision, fluid_cell_is_selectable, inspect_fluid_cell,
//! };
//! ```
//!
//! Policy classification uses the registration kind and last path token so host
//! crates never embed package-owned concrete identities. Presentation bindings
//! are excluded from inspect fragments.

use latticeaxiom_core::StableId;
use serde::{Deserialize, Serialize};

use crate::{
    ContentError, ContentResult, FluidCollisionPolicyV1, FluidSelectionPolicyV1, FluidStateV1,
    SolidOccupancyKindV1,
};

/// Classified v1 fluid-collision policy kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FluidCollisionKindV1 {
    /// Fluid never occupies collision volume.
    None,
    /// Any non-empty fluid cell occupies collision volume.
    Volume,
}

impl FluidCollisionKindV1 {
    /// Classifies a versioned fluid-collision-policy identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity is not `fluid-collision-policy` or the
    /// path token is outside the closed v1 set.
    pub fn classify(policy: &FluidCollisionPolicyV1) -> ContentResult<Self> {
        classify_policy(
            policy.as_stable_id(),
            "fluid-collision-policy",
            "fluid collision policy",
            &[("none", Self::None), ("volume", Self::Volume)],
        )
    }
}

/// Classified v1 fluid-selection policy kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FluidSelectionKindV1 {
    /// Fluid is never a gameplay selection target.
    None,
    /// Only source/full cells (`level = 0`) are selectable.
    Source,
    /// Any non-empty fluid cell is selectable.
    Volume,
}

impl FluidSelectionKindV1 {
    /// Classifies a versioned fluid-selection-policy identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity is not `fluid-selection-policy` or the
    /// path token is outside the closed v1 set.
    pub fn classify(policy: &FluidSelectionPolicyV1) -> ContentResult<Self> {
        classify_policy(
            policy.as_stable_id(),
            "fluid-selection-policy",
            "fluid selection policy",
            &[
                ("none", Self::None),
                ("source", Self::Source),
                ("volume", Self::Volume),
            ],
        )
    }
}

/// Headless inspect fragment for one fluid cell.
///
/// Presentation assets, locale strings, and icons are deliberately absent so a
/// headless host can omit the presentation package without changing the
/// fragment bytes or world hash.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FluidInspectFragmentV1 {
    fluid: StableId,
    level: u8,
    flow: crate::FluidFlowV1,
    collision: FluidCollisionKindV1,
    selection: FluidSelectionKindV1,
    collision_occupied: bool,
    selectable: bool,
}

impl FluidInspectFragmentV1 {
    /// Returns the exact fluid identity.
    #[must_use]
    pub const fn fluid(&self) -> &StableId {
        &self.fluid
    }

    /// Returns the canonical numeric level.
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// Returns the explicit authoritative flow.
    #[must_use]
    pub const fn flow(&self) -> crate::FluidFlowV1 {
        self.flow
    }

    /// Returns classified collision semantics.
    #[must_use]
    pub const fn collision(&self) -> FluidCollisionKindV1 {
        self.collision
    }

    /// Returns classified selection semantics.
    #[must_use]
    pub const fn selection(&self) -> FluidSelectionKindV1 {
        self.selection
    }

    /// Returns whether this cell occupies collision volume.
    #[must_use]
    pub const fn collision_occupied(&self) -> bool {
        self.collision_occupied
    }

    /// Returns whether this cell is a gameplay selection target.
    #[must_use]
    pub const fn selectable(&self) -> bool {
        self.selectable
    }
}

/// Which layer a dual-layer cell should expose to inspect.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CellInspectKindV1 {
    /// No solid and no fluid.
    Empty,
    /// A full solid occupies the cell; fluid inspect is not the primary target.
    Solid,
    /// The solid layer is empty and a fluid cell is present.
    Fluid,
}

/// Classifies a fluid-collision policy identity.
///
/// # Errors
///
/// Returns [`ContentError::InvalidFluidPolicy`] or a kind/major mismatch.
pub fn classify_fluid_collision(
    policy: &FluidCollisionPolicyV1,
) -> ContentResult<FluidCollisionKindV1> {
    FluidCollisionKindV1::classify(policy)
}

/// Classifies a fluid-selection policy identity.
///
/// # Errors
///
/// Returns [`ContentError::InvalidFluidPolicy`] or a kind/major mismatch.
pub fn classify_fluid_selection(
    policy: &FluidSelectionPolicyV1,
) -> ContentResult<FluidSelectionKindV1> {
    FluidSelectionKindV1::classify(policy)
}

/// Returns whether a non-empty fluid cell occupies collision volume.
#[must_use]
pub const fn fluid_cell_blocks_collision(kind: FluidCollisionKindV1) -> bool {
    matches!(kind, FluidCollisionKindV1::Volume)
}

/// Returns whether a fluid cell is selectable under `kind` and `state`.
#[must_use]
pub const fn fluid_cell_is_selectable(kind: FluidSelectionKindV1, state: FluidStateV1) -> bool {
    match kind {
        FluidSelectionKindV1::None => false,
        FluidSelectionKindV1::Source => state.level.is_source(),
        FluidSelectionKindV1::Volume => true,
    }
}

/// Chooses the inspect layer for one dual-layer cell.
#[must_use]
pub const fn cell_inspect_kind(
    solid: SolidOccupancyKindV1,
    fluid_present: bool,
) -> CellInspectKindV1 {
    match solid {
        SolidOccupancyKindV1::Full => CellInspectKindV1::Solid,
        SolidOccupancyKindV1::Empty if fluid_present => CellInspectKindV1::Fluid,
        SolidOccupancyKindV1::Empty => CellInspectKindV1::Empty,
    }
}

/// Builds a presentation-free inspect fragment for one fluid cell.
///
/// # Errors
///
/// Returns an error when `fluid` is not an exact `fluid` identity or a policy
/// cannot be classified.
pub fn inspect_fluid_cell(
    fluid: StableId,
    state: FluidStateV1,
    collision_policy: &FluidCollisionPolicyV1,
    selection_policy: &FluidSelectionPolicyV1,
) -> ContentResult<FluidInspectFragmentV1> {
    crate::header::validate_exact_id(&fluid, "fluid", "fluid inspect identity")?;
    let collision = FluidCollisionKindV1::classify(collision_policy)?;
    let selection = FluidSelectionKindV1::classify(selection_policy)?;
    Ok(FluidInspectFragmentV1 {
        fluid,
        level: state.level.get(),
        flow: state.flow,
        collision,
        selection,
        collision_occupied: fluid_cell_blocks_collision(collision),
        selectable: fluid_cell_is_selectable(selection, state),
    })
}

fn classify_policy<T: Copy>(
    id: &StableId,
    expected_kind: &'static str,
    context: &'static str,
    table: &[(&str, T)],
) -> ContentResult<T> {
    if id.kind() != expected_kind {
        return Err(ContentError::WrongIdentityKind {
            context,
            id: id.clone(),
            expected: expected_kind,
            actual: id.kind().to_owned(),
        });
    }
    let token = last_path_token(id.path());
    table
        .iter()
        .find_map(|(name, value)| (*name == token).then_some(*value))
        .ok_or_else(|| ContentError::InvalidFluidPolicy {
            id: id.clone(),
            reason: "path token is outside the closed v1 fluid semantics vocabulary",
        })
}

fn last_path_token(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FluidFlowV1, FluidLevelV1, FluidStateV1};

    fn stable_id(value: &str) -> StableId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture StableId `{value}` is invalid: {error}"))
    }

    fn collision(token: &str) -> FluidCollisionPolicyV1 {
        FluidCollisionPolicyV1::new(stable_id(&format!(
            "fixture:fluid-collision-policy/{token}@1"
        )))
        .unwrap_or_else(|error| panic!("collision policy `{token}` is invalid: {error}"))
    }

    fn selection(token: &str) -> FluidSelectionPolicyV1 {
        FluidSelectionPolicyV1::new(stable_id(&format!(
            "fixture:fluid-selection-policy/{token}@1"
        )))
        .unwrap_or_else(|error| panic!("selection policy `{token}` is invalid: {error}"))
    }

    fn source_still() -> FluidStateV1 {
        FluidStateV1 {
            level: FluidLevelV1::SOURCE,
            flow: FluidFlowV1::Still,
        }
    }

    fn flowing(level: u8) -> FluidStateV1 {
        FluidStateV1 {
            level: FluidLevelV1::new(level)
                .unwrap_or_else(|error| panic!("level {level} is invalid: {error}")),
            flow: FluidFlowV1::East,
        }
    }

    #[test]
    fn collision_and_selection_classify_from_path_tokens() {
        assert_eq!(
            FluidCollisionKindV1::classify(&collision("volume"))
                .unwrap_or_else(|error| panic!("volume classify failed: {error}")),
            FluidCollisionKindV1::Volume
        );
        assert_eq!(
            FluidCollisionKindV1::classify(&collision("none"))
                .unwrap_or_else(|error| panic!("none classify failed: {error}")),
            FluidCollisionKindV1::None
        );
        assert_eq!(
            FluidSelectionKindV1::classify(&selection("source"))
                .unwrap_or_else(|error| panic!("source classify failed: {error}")),
            FluidSelectionKindV1::Source
        );
        assert_eq!(
            FluidSelectionKindV1::classify(&selection("volume"))
                .unwrap_or_else(|error| panic!("volume selection classify failed: {error}")),
            FluidSelectionKindV1::Volume
        );
    }

    #[test]
    fn unknown_policy_tokens_and_wrong_kind_fail_closed() {
        let unknown = FluidCollisionKindV1::classify(&collision("lava"));
        assert!(matches!(
            unknown,
            Err(ContentError::InvalidFluidPolicy { .. })
        ));
        let wrong_kind = FluidCollisionPolicyV1::new(stable_id("fixture:predicate/volume@1"));
        assert!(matches!(
            wrong_kind,
            Err(ContentError::WrongIdentityKind {
                expected: "fluid-collision-policy",
                ..
            })
        ));
    }

    #[test]
    fn inspect_fragment_excludes_presentation_and_honors_source_selection() {
        let source = inspect_fluid_cell(
            stable_id("fixture:fluid/alpha"),
            source_still(),
            &collision("volume"),
            &selection("source"),
        )
        .unwrap_or_else(|error| panic!("source inspect failed: {error}"));
        assert!(source.collision_occupied());
        assert!(source.selectable());
        assert_eq!(source.level(), 0);

        let flowing = inspect_fluid_cell(
            stable_id("fixture:fluid/alpha"),
            flowing(3),
            &collision("volume"),
            &selection("source"),
        )
        .unwrap_or_else(|error| panic!("flowing inspect failed: {error}"));
        assert!(flowing.collision_occupied());
        assert!(!flowing.selectable());
        assert_eq!(flowing.level(), 3);
    }

    #[test]
    fn empty_solid_inspects_fluid_and_full_solid_wins() {
        assert_eq!(
            cell_inspect_kind(SolidOccupancyKindV1::Empty, true),
            CellInspectKindV1::Fluid
        );
        assert_eq!(
            cell_inspect_kind(SolidOccupancyKindV1::Full, true),
            CellInspectKindV1::Solid
        );
        assert_eq!(
            cell_inspect_kind(SolidOccupancyKindV1::Empty, false),
            CellInspectKindV1::Empty
        );
    }
}
