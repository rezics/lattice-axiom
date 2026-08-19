//! Startup wiring from the Nickel composition graph into numeric sandbox data.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use latticeaxiom_compose::Evaluator;
use latticeaxiom_core::TerrainBlocks;
use latticeaxiom_modules::{RuntimeBlock, RuntimeImage};
use latticeaxiom_packages::PackageKernel;
use latticeaxiom_storage::ContentHash;
use sha2::{Digest as _, Sha256};

use crate::block_catalog::BlockCatalog;

const PROFILE_PATH: &str = "profiles/dev.ncl";
const DEFAULT_WORLD_SEED: &str = "lattice-axiom-demo";

/// Fully compiled startup values consumed by the sandbox hot path.
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// Numeric block registry and terrain selections.
    pub catalog: BlockCatalog,
    /// Deterministic first-materialization seed.
    pub seed: u64,
    /// Exact resolved package-graph identity stored in generation provenance.
    pub producer_hash: ContentHash,
    /// Human-readable lock identity for startup diagnostics.
    pub graph_sha256: String,
}

impl RuntimeConfig {
    /// Evaluates the checked-in profile and resolves its exact local closure.
    ///
    /// # Errors
    ///
    /// Fails when Nickel evaluation, package resolution, runtime registration,
    /// required official block selection, or lock digest decoding fails.
    pub fn load(project_root: &Path) -> Result<Self> {
        let evaluator = Evaluator::new(project_root.join("nickel"));
        let spec = evaluator
            .evaluate_game(project_root.join(PROFILE_PATH))
            .context("failed to evaluate the development game profile")?;
        let graph = PackageKernel::new(project_root, evaluator)
            .resolve(&spec)
            .context("failed to resolve the exact package closure")?;
        let image = RuntimeImage::from_locked_graph(&graph)
            .context("failed to compile stable runtime registrations")?;

        let stone = required_block(&image, "latticeaxiom.official:stone")?;
        let dirt = required_block(&image, "latticeaxiom.official:dirt")?;
        let grass = required_block(&image, "latticeaxiom.official:grass")?;
        let catalog = BlockCatalog::new(
            image
                .blocks()
                .iter()
                .map(|block| (block.id(), block.solid(), block.color())),
            TerrainBlocks::new(grass.id(), dirt.id(), stone.id()),
            dirt.id(),
        )?;
        let seed_text = spec
            .parameters
            .get("world_seed")
            .map_or(DEFAULT_WORLD_SEED, String::as_str);

        Ok(Self {
            catalog,
            seed: stable_seed(seed_text),
            producer_hash: decode_sha256(&graph.graph_sha256)?,
            graph_sha256: graph.graph_sha256,
        })
    }
}

/// Absolute repository root compiled into the development binary.
#[must_use]
pub fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn required_block<'image>(image: &'image RuntimeImage, key: &str) -> Result<&'image RuntimeBlock> {
    image
        .block_by_key(key)
        .with_context(|| format!("resolved package closure is missing required block `{key}`"))
}

fn stable_seed(value: &str) -> u64 {
    let mut digest = Sha256::new();
    digest.update(b"latticeaxiom.world-seed.v1\0");
    digest.update(value.as_bytes());
    let bytes = digest.finalize();
    u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

fn decode_sha256(value: &str) -> Result<ContentHash> {
    if value.len() != 64 {
        anyhow::bail!("resolved graph hash must contain exactly 64 hexadecimal characters");
    }
    let mut bytes = [0; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = decode_hex_digit(pair[0])?;
        let low = decode_hex_digit(pair[1])?;
        bytes[index] = (high << 4) | low;
    }
    Ok(ContentHash::from_bytes(bytes))
}

fn decode_hex_digit(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => anyhow::bail!("resolved graph hash contains a non-hexadecimal byte"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_profile_builds_the_sandbox_catalog() {
        let config = RuntimeConfig::load(&project_root())
            .expect("checked-in profile and official package must compose");
        assert!(config.catalog.is_solid(config.catalog.terrain().stone));
        assert!(config.catalog.is_solid(config.catalog.selected()));
        assert_ne!(config.seed, 0);
        assert_ne!(config.producer_hash, ContentHash::ZERO);
    }

    #[test]
    fn graph_hash_decoder_rejects_non_hex_input() {
        assert!(decode_sha256(&"z".repeat(64)).is_err());
        assert!(decode_sha256("00").is_err());
    }
}
