//! Same-context occupancy conflicts with stable occupying-action diagnostics.

use std::collections::BTreeMap;

use latticeaxiom_core::StableId;

use crate::{InputBindingV1, InputContextV1, OccupancyTokenV1};

/// One same-context occupancy claimed by two or more actions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingConflictV1 {
    /// Context that forbids sharing this occupancy.
    pub context: InputContextV1,
    /// Normalized occupancy token.
    pub occupancy: OccupancyTokenV1,
    /// Occupying action IDs sorted in stable order.
    pub actions: Vec<StableId>,
}

/// Detects same-context button and axis occupancy conflicts.
///
/// Cross-context reuse is allowed. Unknown bindings contribute no occupancy.
#[must_use]
pub fn detect_same_context_conflicts(
    actions: &[(StableId, InputContextV1, &[InputBindingV1])],
) -> Vec<BindingConflictV1> {
    let mut occupancy: BTreeMap<(InputContextV1, OccupancyTokenV1), Vec<StableId>> =
        BTreeMap::new();
    for (action, context, bindings) in actions {
        for binding in *bindings {
            for token in binding.occupancy_tokens() {
                occupancy
                    .entry((*context, token))
                    .or_default()
                    .push(action.clone());
            }
        }
    }
    let mut conflicts = Vec::new();
    for ((context, occupancy_token), mut actions) in occupancy {
        actions.sort();
        actions.dedup();
        if actions.len() > 1 {
            conflicts.push(BindingConflictV1 {
                context,
                occupancy: occupancy_token,
                actions,
            });
        }
    }
    conflicts
}

impl BindingConflictV1 {
    /// Occupying action IDs in stable order.
    #[must_use]
    pub fn occupying_actions(&self) -> &[StableId] {
        &self.actions
    }
}
