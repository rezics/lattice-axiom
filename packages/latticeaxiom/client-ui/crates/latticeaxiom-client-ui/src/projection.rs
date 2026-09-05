//! Epoch-gated semantic-tree diff. Stale async results cannot reopen a surface.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::semantic::{SemanticKey, SemanticNode};
use crate::surface::SurfaceEpoch;

/// Snapshot of one projected surface tree bound to a route epoch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticSnapshot {
    epoch: SurfaceEpoch,
    root: SemanticNode,
}

impl SemanticSnapshot {
    /// Creates a snapshot for `epoch`.
    #[must_use]
    pub const fn new(epoch: SurfaceEpoch, root: SemanticNode) -> Self {
        Self { epoch, root }
    }

    /// Returns the owning epoch.
    #[must_use]
    pub const fn epoch(&self) -> SurfaceEpoch {
        self.epoch
    }

    /// Returns the projected tree.
    #[must_use]
    pub const fn root(&self) -> &SemanticNode {
        &self.root
    }

    /// Returns every node key in stable order.
    #[must_use]
    pub fn keys(&self) -> BTreeSet<SemanticKey> {
        let mut keys = BTreeSet::new();
        collect_keys(&self.root, &mut keys);
        keys
    }
}

fn collect_keys(node: &SemanticNode, keys: &mut BTreeSet<SemanticKey>) {
    keys.insert(node.key.clone());
    for child in &node.children {
        collect_keys(child, keys);
    }
}

/// Deterministic insert/update/remove sets for one epoch apply.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionDiff {
    /// Keys present only in the new tree.
    pub inserted: BTreeSet<SemanticKey>,
    /// Keys present in both trees.
    pub updated: BTreeSet<SemanticKey>,
    /// Keys present only in the previous tree.
    pub removed: BTreeSet<SemanticKey>,
}

impl ProjectionDiff {
    /// Returns whether the previous tree was fully replaced.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inserted.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

/// Diffs `next` against `previous` and rejects stale epochs.
///
/// # Errors
///
/// Returns [`ProjectionError::StaleEpoch`] when `next` belongs to an older
/// epoch than `previous`, or [`ProjectionError::DuplicateKey`] when a snapshot
/// contains duplicate keys.
pub fn diff_snapshots(
    previous: Option<&SemanticSnapshot>,
    next: &SemanticSnapshot,
) -> Result<ProjectionDiff, ProjectionError> {
    let next_keys = next.keys();
    if next_keys.len() != count_nodes(&next.root) {
        return Err(ProjectionError::DuplicateKey);
    }
    let Some(previous) = previous else {
        return Ok(ProjectionDiff {
            inserted: next_keys,
            updated: BTreeSet::new(),
            removed: BTreeSet::new(),
        });
    };
    if next.epoch.get() < previous.epoch.get() {
        return Err(ProjectionError::StaleEpoch {
            current: previous.epoch,
            stale: next.epoch,
        });
    }
    let previous_keys = previous.keys();
    Ok(ProjectionDiff {
        inserted: next_keys.difference(&previous_keys).cloned().collect(),
        updated: next_keys.intersection(&previous_keys).cloned().collect(),
        removed: previous_keys.difference(&next_keys).cloned().collect(),
    })
}

fn count_nodes(node: &SemanticNode) -> usize {
    1_usize.saturating_add(node.children.iter().map(count_nodes).sum::<usize>())
}

/// Applies an async payload only when it still matches the live epoch.
///
/// # Errors
///
/// Returns [`ProjectionError::StaleEpoch`] when `payload_epoch` is not the
/// live epoch. Closed surfaces therefore cannot be rebuilt by a late result.
pub fn accept_async_epoch(
    live: SurfaceEpoch,
    payload_epoch: SurfaceEpoch,
) -> Result<(), ProjectionError> {
    if payload_epoch == live {
        Ok(())
    } else {
        Err(ProjectionError::StaleEpoch {
            current: live,
            stale: payload_epoch,
        })
    }
}

/// Semantic projection failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProjectionError {
    /// An async or previous snapshot used a stale epoch.
    #[error("semantic projection epoch {stale} is stale; live epoch is {current}")]
    StaleEpoch {
        /// Live epoch.
        current: SurfaceEpoch,
        /// Rejected epoch.
        stale: SurfaceEpoch,
    },
    /// Focus restoration would be ambiguous.
    #[error("semantic projection contains a duplicate stable key")]
    DuplicateKey,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{ButtonWidget, application_root};

    fn key(value: &str) -> SemanticKey {
        SemanticKey::new(value).expect("static projection key")
    }

    fn snapshot(epoch: u64, button: &str) -> SemanticSnapshot {
        let button = ButtonWidget {
            key: key(button),
            name: button.to_owned(),
            description: None,
            enabled: true,
        }
        .semantic_node(true);
        let root = application_root(key("root"), "root", vec![button]).expect("single root");
        SemanticSnapshot::new(SurfaceEpoch::from_raw(epoch), root)
    }

    #[test]
    fn closing_a_surface_removes_its_keys_and_stale_epochs_are_rejected() {
        let open = snapshot(2, "settings/apply");
        let closed = snapshot(3, "pause/resume");
        let diff = diff_snapshots(Some(&open), &closed).expect("diff");
        assert!(
            diff.removed
                .iter()
                .any(|key| key.as_str() == "settings/apply")
        );
        assert_eq!(
            accept_async_epoch(closed.epoch(), open.epoch()),
            Err(ProjectionError::StaleEpoch {
                current: closed.epoch(),
                stale: open.epoch(),
            })
        );
    }
}
