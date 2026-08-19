//! Stable numeric registration and runtime image construction.
//!
//! Package keys exist only while constructing a [`RuntimeImage`]. Simulation,
//! voxel storage, meshing, and rendering consume the canonical
//! [`BlockId`] newtype from `latticeaxiom-core`.

use std::collections::BTreeMap;
use std::path::Path;

pub use latticeaxiom_core::BlockId;
use latticeaxiom_packages::{LockedBlock, LockedGameGraph, PackageKernel};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Current in-memory semantic version of [`RuntimeImage`].
pub const RUNTIME_IMAGE_SCHEMA_VERSION: u32 = 1;

/// Evaluates a development profile, resolves exact local packages, and builds
/// its runtime image in one shared host/CLI pipeline.
///
/// Production startup can instead read a verified lock and call
/// [`RuntimeImage::from_locked_graph`] directly. This convenience is intended
/// for the first demo and development profiles.
///
/// # Errors
///
/// Returns [`RuntimeLoadError`] when Nickel evaluation, package resolution, or
/// numeric registration fails.
pub fn load_runtime_image(
    project_root: impl AsRef<Path>,
    profile: impl AsRef<Path>,
) -> Result<RuntimeImage, RuntimeLoadError> {
    let project_root = project_root.as_ref();
    let profile = if profile.as_ref().is_absolute() {
        profile.as_ref().to_path_buf()
    } else {
        project_root.join(profile)
    };
    let evaluator = latticeaxiom_compose::Evaluator::new(project_root.join("nickel"));
    let spec = evaluator.evaluate_game(profile)?;
    let graph = PackageKernel::new(project_root, evaluator).resolve(&spec)?;
    RuntimeImage::from_locked_graph(&graph).map_err(RuntimeLoadError::Runtime)
}

/// One block registry row compiled for runtime use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeBlock {
    id: BlockId,
    key: String,
    display_name: String,
    solid: bool,
    is_air: bool,
    color: [u8; 4],
}

impl RuntimeBlock {
    /// Returns the stable numeric hot-path identity.
    #[must_use]
    pub const fn id(&self) -> BlockId {
        self.id
    }

    /// Returns the package-qualified registration key used at composition time.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the user-facing fallback name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns whether the block is solid for collision and opaque meshing.
    #[must_use]
    pub const fn solid(&self) -> bool {
        self.solid
    }

    /// Returns whether this row is the unique empty-space registration.
    #[must_use]
    pub const fn is_air(&self) -> bool {
        self.is_air
    }

    /// Returns the first-demo linear RGBA color.
    #[must_use]
    pub const fn color(&self) -> [u8; 4] {
        self.color
    }
}

/// Runtime-only tables compiled from one exact locked game graph.
///
/// Rows are sorted by numeric ID so per-ID lookup is a binary search. The
/// package-qualified string keys remain available for startup wiring and
/// diagnostics, but callers should retain `BlockId` for hot-path work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeImage {
    schema_version: u32,
    graph_sha256: String,
    blocks: Vec<RuntimeBlock>,
}

impl RuntimeImage {
    /// Compiles stable numeric IDs and read-only block rows from a locked graph.
    ///
    /// IDs other than air are the first 32 bits of SHA-256 over a versioned,
    /// domain-separated package-qualified key. This makes discovery order and
    /// addition of unrelated registrations irrelevant. Hash collisions fail
    /// before gameplay rather than becoming order-dependent.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeImageError`] unless exactly one block is marked as air,
    /// or when two stable keys collide in the 32-bit runtime ID space.
    pub fn from_locked_graph(graph: &LockedGameGraph) -> Result<Self, RuntimeImageError> {
        let mut declarations = graph
            .packages
            .iter()
            .flat_map(|package| package.blocks.iter())
            .collect::<Vec<_>>();
        declarations.sort_by(|left, right| left.key.cmp(&right.key));

        let air_keys = declarations
            .iter()
            .filter(|block| block.is_air)
            .map(|block| block.key.clone())
            .collect::<Vec<_>>();
        match air_keys.as_slice() {
            [] => return Err(RuntimeImageError::MissingAir),
            [_] => {}
            _ => return Err(RuntimeImageError::MultipleAir { keys: air_keys }),
        }

        let mut occupied = BTreeMap::<BlockId, String>::new();
        let mut blocks = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let id = if declaration.is_air {
                BlockId::AIR
            } else {
                stable_block_id(&declaration.key)
            };
            if id.is_air() && !declaration.is_air {
                return Err(RuntimeImageError::ReservedAirCollision {
                    key: declaration.key.clone(),
                });
            }
            if let Some(existing) = occupied.insert(id, declaration.key.clone()) {
                return Err(RuntimeImageError::NumericIdCollision {
                    id,
                    first: existing,
                    second: declaration.key.clone(),
                });
            }
            blocks.push(runtime_block(id, declaration));
        }
        blocks.sort_by_key(RuntimeBlock::id);
        Ok(Self {
            schema_version: RUNTIME_IMAGE_SCHEMA_VERSION,
            graph_sha256: graph.graph_sha256.clone(),
            blocks,
        })
    }

    /// Returns the runtime semantic schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the exact lock graph identity that produced this image.
    #[must_use]
    pub fn graph_sha256(&self) -> &str {
        &self.graph_sha256
    }

    /// Returns every block row in stable numeric-ID order.
    #[must_use]
    pub fn blocks(&self) -> &[RuntimeBlock] {
        &self.blocks
    }

    /// Finds a block row by its hot-path numeric identity.
    #[must_use]
    pub fn block(&self, id: BlockId) -> Option<&RuntimeBlock> {
        self.blocks
            .binary_search_by_key(&id, RuntimeBlock::id)
            .ok()
            .and_then(|index| self.blocks.get(index))
    }

    /// Resolves a package-qualified key during startup or tooling.
    #[must_use]
    pub fn block_by_key(&self, key: &str) -> Option<&RuntimeBlock> {
        self.blocks.iter().find(|block| block.key == key)
    }
}

/// Failure to compile a valid and unambiguous runtime registry.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RuntimeImageError {
    /// No package registered the mandatory empty-space block.
    #[error(
        "the game closure has no air block; mark exactly one block declaration with `is_air = true`"
    )]
    MissingAir,
    /// More than one package registered an empty-space block.
    #[error("the game closure has multiple air blocks {keys:?}; exactly one may map to BlockId(0)")]
    MultipleAir {
        /// Conflicting registration keys in stable order.
        keys: Vec<String>,
    },
    /// A non-air key's stable hash produced the reserved raw value zero.
    #[error("block `{key}` hashes to reserved air ID zero; rename the stable key")]
    ReservedAirCollision {
        /// Registration key that collided with air.
        key: String,
    },
    /// Two registration keys produced the same stable 32-bit ID.
    #[error("blocks `{first}` and `{second}` collide at numeric ID {id}; rename one stable key")]
    NumericIdCollision {
        /// Colliding runtime ID.
        id: BlockId,
        /// First stable key in lexical registration order.
        first: String,
        /// Second stable key in lexical registration order.
        second: String,
    },
}

/// Failure in the shared development-profile to runtime-image pipeline.
#[derive(Debug, Error)]
pub enum RuntimeLoadError {
    /// Nickel evaluation or direct typed conversion failed.
    #[error(transparent)]
    Compose(#[from] latticeaxiom_compose::ComposeError),
    /// Exact local package resolution failed.
    #[error(transparent)]
    Package(#[from] latticeaxiom_packages::PackageError),
    /// Stable numeric registry construction failed.
    #[error(transparent)]
    Runtime(#[from] RuntimeImageError),
}

fn stable_block_id(key: &str) -> BlockId {
    let mut digest = Sha256::new();
    digest.update(b"latticeaxiom.block-id.v1\0");
    digest.update(key.as_bytes());
    let bytes = digest.finalize();
    BlockId::from_raw(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn runtime_block(id: BlockId, declaration: &LockedBlock) -> RuntimeBlock {
    RuntimeBlock {
        id,
        key: declaration.key.clone(),
        display_name: declaration.display_name.clone(),
        solid: declaration.solid,
        is_air: declaration.is_air,
        color: declaration.color,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use latticeaxiom_compose::{PackageId, PackageVersion, ProfileDeclaration, Realization};
    use latticeaxiom_packages::{LockedBlock, LockedGameGraph, LockedPackage, LockedSource};
    use proptest::prelude::*;

    use super::{BlockId, RuntimeImage};

    fn package_id(value: &str) -> PackageId {
        PackageId::new(value).expect("test package id is canonical")
    }

    fn block(key: &str, is_air: bool) -> LockedBlock {
        LockedBlock {
            key: key.to_owned(),
            display_name: key.to_owned(),
            solid: !is_air,
            is_air,
            color: [1, 2, 3, 255],
        }
    }

    fn graph(blocks: Vec<LockedBlock>) -> LockedGameGraph {
        LockedGameGraph {
            schema_version: 1,
            composition_schema_version: 1,
            contract_schema_version: 1,
            nickel_evaluator: "nickel-lang-test".to_owned(),
            contract_sha256: "contract".to_owned(),
            graph_sha256: "test-graph".to_owned(),
            root_profile: ProfileDeclaration {
                id: package_id("example.game"),
                version: PackageVersion::new("1").expect("test version is exact"),
            },
            target: "test-target".to_owned(),
            toolchain: "test-toolchain".to_owned(),
            parameters: BTreeMap::new(),
            packages: vec![LockedPackage {
                id: package_id("example.content"),
                version: PackageVersion::new("1").expect("test version is exact"),
                source: LockedSource::Path {
                    path: "packages/content".to_owned(),
                },
                content_sha256: "content".to_owned(),
                dependencies: Vec::new(),
                capabilities: Vec::new(),
                blocks,
                realization: Realization::Data,
            }],
            capability_bindings: Vec::new(),
        }
    }

    proptest! {
        #[test]
        fn registration_order_does_not_change_ids(order in prop::collection::vec(0usize..4, 4)) {
            let all = [
                block("example.content:air", true),
                block("example.content:stone", false),
                block("example.content:dirt", false),
                block("example.content:grass", false),
            ];
            let baseline = RuntimeImage::from_locked_graph(&graph(all.to_vec()))
                .expect("baseline registry is valid");
            let mut shuffled = Vec::new();
            for index in order {
                if !shuffled.iter().any(|entry: &LockedBlock| entry.key == all[index].key) {
                    shuffled.push(all[index].clone());
                }
            }
            for entry in &all {
                if !shuffled.iter().any(|seen| seen.key == entry.key) {
                    shuffled.push(entry.clone());
                }
            }
            let image = RuntimeImage::from_locked_graph(&graph(shuffled))
                .expect("shuffled registry is valid");
            prop_assert_eq!(baseline, image);
        }
    }

    #[test]
    fn unrelated_registration_does_not_renumber_existing_blocks() {
        let baseline = RuntimeImage::from_locked_graph(&graph(vec![
            block("example.content:air", true),
            block("example.content:stone", false),
        ]))
        .expect("baseline registry is valid");
        let extended = RuntimeImage::from_locked_graph(&graph(vec![
            block("example.content:air", true),
            block("another.package:copper", false),
            block("example.content:stone", false),
        ]))
        .expect("extended registry is valid");

        assert_eq!(
            baseline
                .block_by_key("example.content:stone")
                .map(super::RuntimeBlock::id),
            extended
                .block_by_key("example.content:stone")
                .map(super::RuntimeBlock::id)
        );
        assert_eq!(
            extended
                .block_by_key("example.content:air")
                .map(super::RuntimeBlock::id),
            Some(BlockId::AIR)
        );
    }
}
