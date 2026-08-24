//! Stable wire contract for realized package data roots.
//!
//! A data-root artifact is immutable canonical JSON stored as one realized
//! artifact object. Paths remain package-relative logical paths; this module
//! never materializes them on a host filesystem.

use std::collections::BTreeMap;

use latticeaxiom_core::{
    CanonicalJsonError, CanonicalLogicalPath, CanonicalLogicalPathError, PackageName,
    canonical_json_bytes,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Wire schema of a canonical realized data-root artifact.
pub const REALIZED_DATA_ROOT_SCHEMA_V1: &str = "latticeaxiom.realized-data-root.v1";

/// One validated, immutable package data root decoded from a realized artifact.
///
/// Construction proves that every key is a canonical package-relative path
/// contained by [`Self::root`]. The artifact digest remains a separate
/// product-lock receipt and must be verified before decoding this DTO.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealizedDataRootV1 {
    package: PackageName,
    root: CanonicalLogicalPath,
    files: BTreeMap<CanonicalLogicalPath, Vec<u8>>,
}

impl RealizedDataRootV1 {
    /// Creates a data-root artifact from validated paths.
    ///
    /// # Errors
    ///
    /// Returns [`RealizedDataRootError::Empty`] when `files` is empty or
    /// [`RealizedDataRootError::FileOutsideRoot`] when a file lies outside
    /// `root`.
    pub fn new(
        package: PackageName,
        root: CanonicalLogicalPath,
        files: BTreeMap<CanonicalLogicalPath, Vec<u8>>,
    ) -> Result<Self, RealizedDataRootError> {
        validate_files(&root, &files)?;
        Ok(Self {
            package,
            root,
            files,
        })
    }

    /// Refines string file keys into canonical paths and creates an artifact.
    ///
    /// # Errors
    ///
    /// Returns [`RealizedDataRootError`] when a path or root relationship is
    /// invalid or no files are supplied.
    pub fn from_file_bytes(
        package: PackageName,
        root: CanonicalLogicalPath,
        files: BTreeMap<String, Vec<u8>>,
    ) -> Result<Self, RealizedDataRootError> {
        let files = files
            .into_iter()
            .map(|(logical_path, bytes)| {
                CanonicalLogicalPath::new(logical_path.clone())
                    .map(|path| (path, bytes))
                    .map_err(|source| RealizedDataRootError::InvalidPath {
                        logical_path,
                        source,
                    })
            })
            .collect::<Result<_, _>>()?;
        Self::new(package, root, files)
    }

    /// Decodes and validates exact canonical artifact bytes.
    ///
    /// This proves the wire schema and path containment, not the product-lock
    /// artifact digest. Callers must obtain `bytes` from a verified receipt.
    ///
    /// # Errors
    ///
    /// Returns [`RealizedDataRootError`] when JSON, schema, paths, containment,
    /// or canonical encoding is invalid.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, RealizedDataRootError> {
        let wire: RealizedDataRootWire = serde_json::from_slice(bytes)?;
        if wire.schema != REALIZED_DATA_ROOT_SCHEMA_V1 {
            return Err(RealizedDataRootError::UnsupportedSchema {
                actual: wire.schema,
            });
        }
        if canonical_json_bytes(&wire)? != bytes {
            return Err(RealizedDataRootError::NonCanonicalEncoding);
        }
        Self::new(wire.package, wire.root, wire.files)
    }

    /// Decodes canonical bytes and binds their embedded package identity.
    ///
    /// # Errors
    ///
    /// Returns [`RealizedDataRootError::PackageMismatch`] when the embedded
    /// package differs from `expected`, or another decode error.
    pub fn from_canonical_bytes_for_package(
        bytes: &[u8],
        expected: &PackageName,
    ) -> Result<Self, RealizedDataRootError> {
        let artifact = Self::from_canonical_bytes(bytes)?;
        if artifact.package != *expected {
            return Err(RealizedDataRootError::PackageMismatch {
                expected: expected.clone(),
                actual: artifact.package,
            });
        }
        Ok(artifact)
    }

    /// Encodes this artifact as deterministic compact canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns [`RealizedDataRootError::Canonical`] when encoding fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, RealizedDataRootError> {
        Ok(canonical_json_bytes(&RealizedDataRootWireRef {
            schema: REALIZED_DATA_ROOT_SCHEMA_V1,
            package: &self.package,
            root: &self.root,
            files: &self.files,
        })?)
    }

    /// Returns the package identity sealed inside the artifact.
    #[must_use]
    pub const fn package(&self) -> &PackageName {
        &self.package
    }

    /// Returns the canonical package-relative root covered by this artifact.
    #[must_use]
    pub const fn root(&self) -> &CanonicalLogicalPath {
        &self.root
    }

    /// Returns exact bytes at `path`, if present.
    #[must_use]
    pub fn file(&self, path: &CanonicalLogicalPath) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Requires exact bytes at one canonical package-relative path.
    ///
    /// # Errors
    ///
    /// Returns [`RealizedDataRootError::MissingFile`] when `path` is absent.
    pub fn require_file(
        &self,
        path: &CanonicalLogicalPath,
    ) -> Result<&[u8], RealizedDataRootError> {
        self.file(path)
            .ok_or_else(|| RealizedDataRootError::MissingFile {
                package: self.package.clone(),
                logical_path: path.clone(),
            })
    }

    /// Iterates exact artifact files in canonical path order.
    #[must_use]
    pub fn files(&self) -> impl ExactSizeIterator<Item = (&CanonicalLogicalPath, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path, bytes.as_slice()))
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RealizedDataRootWire {
    schema: String,
    package: PackageName,
    root: CanonicalLogicalPath,
    files: BTreeMap<CanonicalLogicalPath, Vec<u8>>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RealizedDataRootWireRef<'a> {
    schema: &'static str,
    package: &'a PackageName,
    root: &'a CanonicalLogicalPath,
    files: &'a BTreeMap<CanonicalLogicalPath, Vec<u8>>,
}

fn validate_files(
    root: &CanonicalLogicalPath,
    files: &BTreeMap<CanonicalLogicalPath, Vec<u8>>,
) -> Result<(), RealizedDataRootError> {
    if files.is_empty() {
        return Err(RealizedDataRootError::Empty { root: root.clone() });
    }
    for path in files.keys() {
        if path != root && !is_descendant(root, path) {
            return Err(RealizedDataRootError::FileOutsideRoot {
                root: root.clone(),
                logical_path: path.clone(),
            });
        }
    }
    Ok(())
}

fn is_descendant(root: &CanonicalLogicalPath, path: &CanonicalLogicalPath) -> bool {
    path.as_str()
        .strip_prefix(root.as_str())
        .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Failure to construct or decode a realized data-root artifact.
#[derive(Debug, Error)]
pub enum RealizedDataRootError {
    /// Artifact bytes are not valid JSON for the closed wire schema.
    #[error("invalid realized data-root JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A string path is invalid.
    #[error("invalid data-root path `{logical_path}`: {source}")]
    InvalidPath {
        /// Rejected path.
        logical_path: String,
        /// Canonical-path validation failure.
        #[source]
        source: CanonicalLogicalPathError,
    },
    /// Artifact uses a schema this runtime does not implement.
    #[error("unsupported realized data-root schema `{actual}`")]
    UnsupportedSchema {
        /// Rejected wire schema.
        actual: String,
    },
    /// Canonical encoding could not be produced.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// Parsed JSON was not its unique canonical encoding.
    #[error("realized data-root artifact is not canonical JSON")]
    NonCanonicalEncoding,
    /// The artifact selected no files.
    #[error("realized data root `{root}` contains no files")]
    Empty {
        /// Empty root.
        root: CanonicalLogicalPath,
    },
    /// One artifact file escaped the declared root.
    #[error("realized data-root file `{logical_path}` is outside root `{root}`")]
    FileOutsideRoot {
        /// Declared root.
        root: CanonicalLogicalPath,
        /// Escaping path.
        logical_path: CanonicalLogicalPath,
    },
    /// Embedded package identity differs from the lock-selected package.
    #[error("realized data-root package `{actual}` does not match locked package `{expected}`")]
    PackageMismatch {
        /// Lock-selected package.
        expected: PackageName,
        /// Artifact-embedded package.
        actual: PackageName,
    },
    /// Required package-relative file is absent.
    #[error("realized data-root package `{package}` is missing `{logical_path}`")]
    MissingFile {
        /// Lock-selected package.
        package: PackageName,
        /// Required canonical path.
        logical_path: CanonicalLogicalPath,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> PackageName {
        "@terrenia/blocks"
            .parse()
            .unwrap_or_else(|error| panic!("fixture package is valid: {error}"))
    }

    fn path(value: &str) -> CanonicalLogicalPath {
        CanonicalLogicalPath::new(value)
            .unwrap_or_else(|error| panic!("fixture path is valid: {error}"))
    }

    fn artifact() -> RealizedDataRootV1 {
        RealizedDataRootV1::new(
            package(),
            path("data"),
            BTreeMap::from([
                (path("data/authored-catalog-v1.json"), b"{}".to_vec()),
                (path("data/goldens/d9-block-ids.txt"), b"stone\n".to_vec()),
            ]),
        )
        .unwrap_or_else(|error| panic!("fixture artifact is valid: {error}"))
    }

    #[test]
    fn canonical_round_trip_binds_package_and_files() {
        let expected = artifact();
        let bytes = expected
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("artifact encodes: {error}"));
        let decoded = RealizedDataRootV1::from_canonical_bytes_for_package(&bytes, &package())
            .unwrap_or_else(|error| panic!("artifact decodes: {error}"));
        assert_eq!(decoded, expected);
        assert_eq!(
            decoded.file(&path("data/authored-catalog-v1.json")),
            Some(b"{}".as_slice())
        );
    }

    #[test]
    fn rejects_wrong_package_root_and_noncanonical_encoding() {
        let bytes = artifact()
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("artifact encodes: {error}"));
        let other: PackageName = "@other/blocks"
            .parse()
            .unwrap_or_else(|error| panic!("fixture package is valid: {error}"));
        assert!(matches!(
            RealizedDataRootV1::from_canonical_bytes_for_package(&bytes, &other),
            Err(RealizedDataRootError::PackageMismatch { .. })
        ));
        assert!(matches!(
            RealizedDataRootV1::new(
                package(),
                path("data"),
                BTreeMap::from([(path("code/main.rs"), Vec::new())])
            ),
            Err(RealizedDataRootError::FileOutsideRoot { .. })
        ));
        let spaced = [b" ", bytes.as_slice()].concat();
        assert!(matches!(
            RealizedDataRootV1::from_canonical_bytes(&spaced),
            Err(RealizedDataRootError::NonCanonicalEncoding)
        ));
    }
}
