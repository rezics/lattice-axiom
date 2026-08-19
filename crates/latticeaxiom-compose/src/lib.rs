//! Nickel evaluation boundary for Lattice Axiom composition.
//!
//! [`Evaluator`] is the only workspace API that evaluates `game.ncl` and
//! `package.ncl`. Fully evaluated Nickel expressions are deserialized directly
//! into the versioned Rust models in this crate; no JSON intermediate is
//! produced between composition and package resolution.

mod model;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

pub use model::{
    BlockDeclaration, CapabilityId, CapabilityRequirement, CompositionPolicy, CompositionSpec,
    DependencyRequest, NativeCodePolicy, PackageDeclaration, PackageId, PackageManifest,
    PackageRequest, PackageVersion, ProfileDeclaration, ProvidedContent, Realization,
    SourceRequest, ValidationError,
};
use nickel_lang::{Context, ErrorFormat};
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Semantic version of the checked-in `latticeaxiom.lib` contract family.
pub const CONTRACT_SCHEMA_VERSION: u32 = 1;

/// Exact embedded evaluator identity recorded in milestone 2 lock graphs.
pub const NICKEL_EVALUATOR_ID: &str = "nickel-lang-2.2.0";

/// Errors produced while reading, evaluating, or decoding a Nickel input.
#[derive(Debug, Error)]
pub enum ComposeError {
    /// The source file could not be read.
    #[error("failed to read Nickel source `{path}`: {source}")]
    Read {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying file-system error.
        source: std::io::Error,
    },
    /// Nickel parsing, import resolution, evaluation, or a contract failed.
    #[error("Nickel evaluation failed for `{path}`:\n{diagnostic}")]
    Evaluation {
        /// Path being evaluated.
        path: PathBuf,
        /// Source-aware diagnostic rendered by Nickel.
        diagnostic: String,
    },
    /// The evaluated value did not match the Rust semantic model.
    #[error("Nickel result for `{path}` does not match the semantic model: {diagnostic}")]
    Decode {
        /// Path whose result could not be decoded.
        path: PathBuf,
        /// Direct serde conversion diagnostic.
        diagnostic: String,
    },
    /// Rust-side semantic validation rejected the decoded value.
    #[error("invalid composition from `{path}`: {source}")]
    Validation {
        /// Path whose value was invalid.
        path: PathBuf,
        /// Precise semantic validation error.
        source: ValidationError,
    },
}

/// Controlled evaluator configured with the versioned `latticeaxiom.lib` root.
///
/// The embedded evaluator intentionally does not honor `NICKEL_IMPORT_PATH`.
/// Only the contract library root and the directory containing the evaluated
/// source are placed on Nickel's import search path.
#[derive(Clone, Debug)]
pub struct Evaluator {
    library_root: PathBuf,
}

impl Evaluator {
    /// Creates an evaluator using `library_root` as the only shared import root.
    #[must_use]
    pub fn new(library_root: impl Into<PathBuf>) -> Self {
        Self {
            library_root: library_root.into(),
        }
    }

    /// Returns the configured shared contract-library import root.
    #[must_use]
    pub fn library_root(&self) -> &Path {
        &self.library_root
    }

    /// Evaluates a root `game.ncl` into a typed [`CompositionSpec`].
    ///
    /// # Errors
    ///
    /// Returns [`ComposeError`] if the file cannot be read, Nickel evaluation
    /// or a contract fails, direct serde conversion fails, or the resulting
    /// semantic model violates a version or uniqueness invariant.
    pub fn evaluate_game(&self, path: impl AsRef<Path>) -> Result<CompositionSpec, ComposeError> {
        let path = path.as_ref();
        let spec: CompositionSpec = self.evaluate(path)?;
        spec.validate().map_err(|source| ComposeError::Validation {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(spec)
    }

    /// Evaluates a package `package.ncl` into a typed [`PackageManifest`].
    ///
    /// # Errors
    ///
    /// Returns [`ComposeError`] under the same conditions as
    /// [`Self::evaluate_game`].
    pub fn evaluate_package(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<PackageManifest, ComposeError> {
        let path = path.as_ref();
        let manifest: PackageManifest = self.evaluate(path)?;
        manifest
            .validate()
            .map_err(|source| ComposeError::Validation {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(manifest)
    }

    fn evaluate<T: DeserializeOwned>(&self, path: &Path) -> Result<T, ComposeError> {
        let source = fs::read_to_string(path).map_err(|source| ComposeError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let source_parent = path.parent().unwrap_or_else(|| Path::new("."));
        let import_paths = vec![
            OsString::from(&self.library_root),
            OsString::from(source_parent),
        ];
        let mut context = Context::new()
            .with_added_import_paths(import_paths)
            .with_source_name(path.to_string_lossy().into_owned());
        let expression =
            context
                .eval_deep_for_export(&source)
                .map_err(|error| ComposeError::Evaluation {
                    path: path.to_path_buf(),
                    diagnostic: format_nickel_error(&error),
                })?;
        expression.to_serde().map_err(|error| ComposeError::Decode {
            path: path.to_path_buf(),
            diagnostic: error.to_string(),
        })
    }
}

fn format_nickel_error(error: &nickel_lang::Error) -> String {
    let mut bytes = Vec::new();
    if error.format(&mut bytes, ErrorFormat::Text).is_err() {
        return format!("{error:?}");
    }
    String::from_utf8_lossy(&bytes).trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::Evaluator;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn repository_profile_evaluates_directly_to_rust() {
        let root = workspace_root();
        let spec = Evaluator::new(root.join("nickel"))
            .evaluate_game(root.join("profiles/dev.ncl"))
            .expect("the checked-in profile and contract must stay in sync");

        assert_eq!(spec.schema_version, 1);
        assert_eq!(spec.root_profile.id.as_str(), "latticeaxiom.demo");
        assert_eq!(spec.package_requests.len(), 1);
    }

    #[test]
    fn repository_package_evaluates_directly_to_rust() {
        let root = workspace_root();
        let package = Evaluator::new(root.join("nickel"))
            .evaluate_package(root.join("packages/official/package.ncl"))
            .expect("the checked-in package and contract must stay in sync");

        assert_eq!(package.package.id.as_str(), "latticeaxiom.official");
        assert_eq!(package.provides.blocks.len(), 4);
    }

    #[test]
    fn contract_diagnostic_keeps_the_source_name() {
        let root = workspace_root();
        let path = std::env::temp_dir().join(format!(
            "latticeaxiom-invalid-profile-{}.ncl",
            std::process::id()
        ));
        fs::write(
            &path,
            "let latticeaxiom = import \"latticeaxiom/game.ncl\" in {} | latticeaxiom.GameProfile",
        )
        .expect("test source can be staged");
        let error = Evaluator::new(root.join("nickel"))
            .evaluate_game(&path)
            .expect_err("missing required fields must fail the Nickel contract");
        let diagnostic = error.to_string();
        let _ = fs::remove_file(&path);

        assert!(
            diagnostic.contains("latticeaxiom-invalid-profile"),
            "diagnostic must retain its source name: {diagnostic}"
        );
        assert!(
            diagnostic.contains("schema_version"),
            "diagnostic must identify the missing field: {diagnostic}"
        );
    }
}
