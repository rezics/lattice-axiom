//! Read-only canonical source-table construction and lexical path preflight.
//!
//! This first implementation deliberately rejects every symbolic link,
//! junction, and reparse point. It does not claim to implement the later
//! follow-target policy: accepting links safely requires an identity-aware,
//! race-resistant filesystem gate.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use latticeaxiom_core::{CanonicalHash, SourceId};
use thiserror::Error;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

const SOURCE_TABLE_DOMAIN: &[u8] = b"latticeaxiom:canonical-source-table/r0\0";
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// The authority under which a filesystem root may be read by composition.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AuthorizedRootKind {
    /// The package currently being evaluated.
    Package,
    /// The versioned `latticeaxiom.lib` virtual or materialized root.
    Library,
    /// A profile-authorized overlay root.
    Overlay,
    /// A fixture-authorized test root.
    Test,
}

/// One immutable root explicitly granted to the composition evaluator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedRoot {
    source_id: SourceId,
    kind: AuthorizedRootKind,
    path: PathBuf,
}

impl AuthorizedRoot {
    /// Creates an authorized root with an absolute host path.
    ///
    /// This constructor performs lexical validation only. [`scan_source_root`]
    /// verifies the root's filesystem type without following links.
    ///
    /// # Errors
    ///
    /// Returns [`SourceScanError::RootNotAbsolute`] for a relative host path.
    pub fn new(
        source_id: SourceId,
        kind: AuthorizedRootKind,
        path: impl Into<PathBuf>,
    ) -> Result<Self, SourceScanError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(SourceScanError::RootNotAbsolute { path });
        }
        validate_supported_root_namespace(&path)?;
        Ok(Self {
            source_id,
            kind,
            path,
        })
    }

    /// Returns the stable source-universe identity of this root.
    #[must_use]
    pub const fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Returns the authority class of this root.
    #[must_use]
    pub const fn kind(&self) -> AuthorizedRootKind {
        self.kind
    }

    /// Returns the host path used only for acquisition and diagnostics.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Defensive limits for enumerating one complete declared source root.
///
/// These bounds cover every regular file in the root. They are deliberately
/// separate from the frozen Nickel imported-closure counters, which can only
/// be measured by a source-table-backed import resolver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceScanLimits {
    /// Maximum number of regular files enumerated from the declared root.
    pub maximum_files: u32,
    /// Maximum aggregate bytes read from the declared root.
    pub maximum_bytes: u64,
}

/// Receipt for one regular file in a canonical source table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileReceipt {
    logical_path: String,
    byte_length: u64,
    content_hash: CanonicalHash,
}

impl FileReceipt {
    /// Returns the NFC, slash-separated, root-relative logical path.
    #[must_use]
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    /// Returns the raw file length in bytes.
    #[must_use]
    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }

    /// Returns the SHA-256 digest of the exact raw file bytes.
    #[must_use]
    pub const fn content_hash(&self) -> CanonicalHash {
        self.content_hash
    }
}

/// Stable, content-addressed view of all regular files below one authorized root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalSourceTable {
    source_id: SourceId,
    root_kind: AuthorizedRootKind,
    files: BTreeMap<String, FileReceipt>,
    total_source_bytes: u64,
    source_hash: CanonicalHash,
}

impl CanonicalSourceTable {
    /// Returns the stable source-universe identity.
    #[must_use]
    pub const fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Returns the authority class used for the scan.
    #[must_use]
    pub const fn root_kind(&self) -> AuthorizedRootKind {
        self.root_kind
    }

    /// Returns receipts keyed in canonical logical-path byte order.
    #[must_use]
    pub const fn files(&self) -> &BTreeMap<String, FileReceipt> {
        &self.files
    }

    /// Returns the aggregate number of raw source bytes.
    #[must_use]
    pub const fn total_source_bytes(&self) -> u64 {
        self.total_source_bytes
    }

    /// Returns the source-content SHA-256 over the canonical file table.
    #[must_use]
    pub const fn source_hash(&self) -> CanonicalHash {
        self.source_hash
    }

    /// Resolves a root-relative path against this complete source table.
    ///
    /// The supplied path is normalized to NFC after rejecting absolute paths,
    /// backslashes, Windows drive prefixes, empty segments, and any `..` that
    /// would escape the root. `.` segments are removed and in-root `..`
    /// segments are resolved before lookup.
    ///
    /// # Errors
    ///
    /// Returns [`SourceScanError::InvalidLogicalPath`] for an unsafe path or
    /// [`SourceScanError::PathNotFound`] when the canonical table has no row.
    pub fn resolve_path(&self, logical_path: &str) -> Result<&FileReceipt, SourceScanError> {
        let canonical = canonical_logical_path(logical_path)?;
        self.files
            .get(&canonical)
            .ok_or(SourceScanError::PathNotFound {
                logical_path: canonical,
            })
    }
}

/// Recursively scans an authorized root while rejecting observed filesystem links.
///
/// Directory enumeration is sorted by normalized logical path before any file
/// receipt is emitted. Regular-file hashes cover raw bytes exactly. The source
/// hash uses a domain-separated binary encoding of each sorted tuple:
/// `(path-byte-length, path, regular-file-tag, byte-length, content-sha256)`.
/// This R0 scanner is intended for locally trusted package trees. It does not
/// provide a race-resistant security boundary against concurrent filesystem
/// mutation; untrusted sources must be copied into an immutable worker staging
/// area before this scan.
///
/// # Errors
///
/// Returns [`SourceScanError`] for I/O failures, non-regular entries, links or
/// reparse points, path collisions, or resource-limit violations.
pub fn scan_source_root(
    root: &AuthorizedRoot,
    limits: SourceScanLimits,
) -> Result<CanonicalSourceTable, SourceScanError> {
    reject_linked_path_components(root.path())?;
    let metadata = metadata_without_following(root.path(), "inspect authorized root")?;
    if is_link_or_reparse(&metadata) {
        return Err(SourceScanError::LinkOrReparsePoint {
            physical_path: root.path.clone(),
        });
    }
    if !metadata.is_dir() {
        return Err(SourceScanError::RootNotDirectory {
            path: root.path.clone(),
        });
    }

    let mut state = ScanState::new(limits);
    state.visit_directory(root.path(), "")?;
    let source_hash = hash_file_table(&state.files)?;
    Ok(CanonicalSourceTable {
        source_id: root.source_id.clone(),
        root_kind: root.kind,
        files: state.files,
        total_source_bytes: state.total_source_bytes,
        source_hash,
    })
}

#[derive(Debug)]
struct PendingEntry {
    physical_path: PathBuf,
    original_name: String,
    canonical_name: String,
}

#[derive(Debug)]
struct ScanState {
    limits: SourceScanLimits,
    files: BTreeMap<String, FileReceipt>,
    total_source_bytes: u64,
    normalized_paths: BTreeMap<String, String>,
    folded_paths: BTreeMap<String, String>,
}

impl ScanState {
    fn new(limits: SourceScanLimits) -> Self {
        Self {
            limits,
            files: BTreeMap::new(),
            total_source_bytes: 0,
            normalized_paths: BTreeMap::new(),
            folded_paths: BTreeMap::new(),
        }
    }

    fn visit_directory(
        &mut self,
        physical_directory: &Path,
        logical_parent: &str,
    ) -> Result<(), SourceScanError> {
        let directory = fs::read_dir(physical_directory).map_err(|error| SourceScanError::Io {
            operation: "enumerate directory",
            physical_path: physical_directory.to_path_buf(),
            kind: error.kind(),
        })?;
        let mut entries = Vec::new();
        for result in directory {
            let entry = result.map_err(|error| SourceScanError::Io {
                operation: "read directory entry",
                physical_path: physical_directory.to_path_buf(),
                kind: error.kind(),
            })?;
            let physical_path = entry.path();
            let original_name =
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| SourceScanError::NonUtf8Path {
                        physical_path: physical_path.clone(),
                    })?;
            let canonical_name = canonical_logical_segment(&original_name)?;
            entries.push(PendingEntry {
                physical_path,
                original_name,
                canonical_name,
            });
        }
        entries.sort_unstable_by(|left, right| {
            left.canonical_name
                .as_bytes()
                .cmp(right.canonical_name.as_bytes())
                .then_with(|| {
                    left.original_name
                        .as_bytes()
                        .cmp(right.original_name.as_bytes())
                })
        });

        for entry in entries {
            let logical_path = join_logical(logical_parent, &entry.canonical_name);
            let original_path = join_logical(logical_parent, &entry.original_name);
            self.register_path(&logical_path, &original_path)?;

            let metadata =
                metadata_without_following(&entry.physical_path, "inspect source entry")?;
            if is_link_or_reparse(&metadata) {
                return Err(SourceScanError::LinkOrReparsePoint {
                    physical_path: entry.physical_path,
                });
            }
            if metadata.is_dir() {
                self.visit_directory(&entry.physical_path, &logical_path)?;
            } else if metadata.is_file() {
                self.read_file(&entry.physical_path, logical_path, metadata.len())?;
            } else {
                return Err(SourceScanError::UnsupportedFileType {
                    physical_path: entry.physical_path,
                });
            }
        }
        Ok(())
    }

    fn register_path(
        &mut self,
        canonical_path: &str,
        original_path: &str,
    ) -> Result<(), SourceScanError> {
        if let Some(first) = self
            .normalized_paths
            .insert(canonical_path.to_owned(), original_path.to_owned())
            && first != original_path
        {
            return Err(SourceScanError::NfcCollision {
                canonical_path: canonical_path.to_owned(),
                first,
                second: original_path.to_owned(),
            });
        }

        let folded = case_fold_key(canonical_path);
        if let Some(first) = self
            .folded_paths
            .insert(folded.clone(), canonical_path.to_owned())
            && first != canonical_path
        {
            return Err(SourceScanError::CaseFoldCollision {
                folded_path: folded,
                first,
                second: canonical_path.to_owned(),
            });
        }
        Ok(())
    }

    fn read_file(
        &mut self,
        physical_path: &Path,
        logical_path: String,
        metadata_length: u64,
    ) -> Result<(), SourceScanError> {
        let next_count = u64::try_from(self.files.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        if next_count > u64::from(self.limits.maximum_files) {
            return Err(SourceScanError::RootFilesLimitExceeded {
                limit: self.limits.maximum_files,
                actual: next_count,
            });
        }
        let projected_bytes = self.total_source_bytes.checked_add(metadata_length).ok_or(
            SourceScanError::RootBytesLimitExceeded {
                limit: self.limits.maximum_bytes,
                actual: u64::MAX,
            },
        )?;
        if projected_bytes > self.limits.maximum_bytes {
            return Err(SourceScanError::RootBytesLimitExceeded {
                limit: self.limits.maximum_bytes,
                actual: projected_bytes,
            });
        }

        let bytes = fs::read(physical_path).map_err(|error| SourceScanError::Io {
            operation: "read source file",
            physical_path: physical_path.to_path_buf(),
            kind: error.kind(),
        })?;
        let byte_length =
            u64::try_from(bytes.len()).map_err(|_| SourceScanError::RootBytesLimitExceeded {
                limit: self.limits.maximum_bytes,
                actual: u64::MAX,
            })?;
        let actual_total = self.total_source_bytes.checked_add(byte_length).ok_or(
            SourceScanError::RootBytesLimitExceeded {
                limit: self.limits.maximum_bytes,
                actual: u64::MAX,
            },
        )?;
        if actual_total > self.limits.maximum_bytes {
            return Err(SourceScanError::RootBytesLimitExceeded {
                limit: self.limits.maximum_bytes,
                actual: actual_total,
            });
        }

        self.total_source_bytes = actual_total;
        let receipt = FileReceipt {
            logical_path: logical_path.clone(),
            byte_length,
            content_hash: CanonicalHash::digest(&bytes),
        };
        self.files.insert(logical_path, receipt);
        Ok(())
    }
}

fn canonical_logical_segment(segment: &str) -> Result<String, SourceScanError> {
    if segment.is_empty() || matches!(segment, "." | "..") {
        return Err(invalid_logical_path(
            segment,
            "path segments cannot be empty, `.` or `..`",
        ));
    }
    if segment.contains(['/', '\\']) {
        return Err(invalid_logical_path(
            segment,
            "path segments cannot contain separators",
        ));
    }
    if segment.contains('\0') {
        return Err(invalid_logical_path(
            segment,
            "path segments cannot contain NUL",
        ));
    }
    Ok(segment.nfc().collect())
}

fn canonical_logical_path(path: &str) -> Result<String, SourceScanError> {
    if path.is_empty() {
        return Err(invalid_logical_path(path, "the path cannot be empty"));
    }
    if path.starts_with('/') {
        return Err(invalid_logical_path(path, "the path must be root-relative"));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(invalid_logical_path(
            path,
            "the path cannot contain a Windows drive prefix",
        ));
    }
    if path.contains('\\') {
        return Err(invalid_logical_path(
            path,
            "the path must use `/` separators",
        ));
    }
    if path.contains('\0') {
        return Err(invalid_logical_path(path, "the path cannot contain NUL"));
    }

    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            return Err(invalid_logical_path(path, "path segments cannot be empty"));
        }
        match segment {
            "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(invalid_logical_path(path, "the path escapes its root"));
                }
            }
            value => segments.push(canonical_logical_segment(value)?),
        }
    }
    if segments.is_empty() {
        Err(invalid_logical_path(
            path,
            "the path must resolve to a file",
        ))
    } else {
        Ok(segments.join("/"))
    }
}

fn invalid_logical_path(value: &str, reason: &'static str) -> SourceScanError {
    SourceScanError::InvalidLogicalPath {
        value: value.to_owned(),
        reason,
    }
}

fn join_logical(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

fn case_fold_key(path: &str) -> String {
    path.case_fold().nfc().collect()
}

fn metadata_without_following(
    path: &Path,
    operation: &'static str,
) -> Result<fs::Metadata, SourceScanError> {
    fs::symlink_metadata(path).map_err(|error| SourceScanError::Io {
        operation,
        physical_path: path.to_path_buf(),
        kind: error.kind(),
    })
}

#[cfg(windows)]
fn validate_supported_root_namespace(path: &Path) -> Result<(), SourceScanError> {
    use std::path::{Component, Prefix};

    match path.components().next() {
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)) => Ok(()),
        Some(Component::Prefix(_)) => Err(SourceScanError::UnsupportedRootNamespace {
            path: path.to_path_buf(),
        }),
        _ => Ok(()),
    }
}

#[cfg(not(windows))]
fn validate_supported_root_namespace(_path: &Path) -> Result<(), SourceScanError> {
    Ok(())
}

fn reject_linked_path_components(path: &Path) -> Result<(), SourceScanError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if !current.is_absolute() {
            continue;
        }
        let metadata = metadata_without_following(&current, "inspect authorized-root component")?;
        if is_link_or_reparse(&metadata) {
            return Err(SourceScanError::LinkOrReparsePoint {
                physical_path: current,
            });
        }
    }
    Ok(())
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn hash_file_table(
    files: &BTreeMap<String, FileReceipt>,
) -> Result<CanonicalHash, SourceScanError> {
    let mut encoded = Vec::new();
    encoded.extend_from_slice(SOURCE_TABLE_DOMAIN);
    for (logical_path, receipt) in files {
        let path_length = u64::try_from(logical_path.len())
            .map_err(|_| SourceScanError::CanonicalTableTooLarge)?;
        encoded.extend_from_slice(&path_length.to_be_bytes());
        encoded.extend_from_slice(logical_path.as_bytes());
        encoded.push(0); // R0 file-type tag: regular file.
        encoded.extend_from_slice(&receipt.byte_length.to_be_bytes());
        encoded.extend_from_slice(receipt.content_hash.as_bytes());
    }
    Ok(CanonicalHash::digest(encoded))
}

/// A deterministic source-table or import-preflight failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SourceScanError {
    /// An authorized root used a relative host path.
    #[error("authorized root is not absolute: {path}", path = .path.display())]
    RootNotAbsolute {
        /// Rejected host path.
        path: PathBuf,
    },
    /// The authorized root was not a directory.
    #[error("authorized root is not a directory: {path}", path = .path.display())]
    RootNotDirectory {
        /// Rejected host path.
        path: PathBuf,
    },
    /// A Windows network, device, or verbatim root namespace was requested.
    #[error("unsupported authorized-root namespace: {path}", path = .path.display())]
    UnsupportedRootNamespace {
        /// Rejected host path.
        path: PathBuf,
    },
    /// A host filesystem operation failed.
    #[error("{operation} failed for {path} ({kind:?})", path = .physical_path.display())]
    Io {
        /// Stable operation name.
        operation: &'static str,
        /// Host path used only for diagnostics.
        physical_path: PathBuf,
        /// Portable I/O error classification.
        kind: ErrorKind,
    },
    /// A host filename was not valid UTF-8.
    #[error("source path is not valid UTF-8: {path}", path = .physical_path.display())]
    NonUtf8Path {
        /// Host path used only for diagnostics.
        physical_path: PathBuf,
    },
    /// A logical path was absolute, escaping, or non-canonical.
    #[error("invalid logical path `{value}`: {reason}")]
    InvalidLogicalPath {
        /// Rejected logical path.
        value: String,
        /// Violated lexical rule.
        reason: &'static str,
    },
    /// A link, junction, or reparse point was encountered.
    #[error("links and reparse points are rejected in R0: {path}", path = .physical_path.display())]
    LinkOrReparsePoint {
        /// Rejected host path.
        physical_path: PathBuf,
    },
    /// A directory entry was neither a regular file nor a directory.
    #[error("unsupported non-regular source entry: {path}", path = .physical_path.display())]
    UnsupportedFileType {
        /// Rejected host path.
        physical_path: PathBuf,
    },
    /// Two distinct host paths normalize to one NFC logical path.
    #[error("NFC path collision at `{canonical_path}` between `{first}` and `{second}`")]
    NfcCollision {
        /// Colliding normalized logical path.
        canonical_path: String,
        /// First original logical path in deterministic order.
        first: String,
        /// Second original logical path in deterministic order.
        second: String,
    },
    /// Two distinct NFC paths compare equal under full Unicode case folding.
    #[error("case-fold path collision at `{folded_path}` between `{first}` and `{second}`")]
    CaseFoldCollision {
        /// Shared folded path.
        folded_path: String,
        /// First NFC logical path in deterministic order.
        first: String,
        /// Second NFC logical path in deterministic order.
        second: String,
    },
    /// The number of files in the declared root exceeded the scan limit.
    #[error("source-root file count {actual} exceeds scan limit {limit}")]
    RootFilesLimitExceeded {
        /// Configured file limit.
        limit: u32,
        /// First rejected file count.
        actual: u64,
    },
    /// Aggregate bytes in the declared root exceeded the scan limit.
    #[error("source-root bytes {actual} exceed scan limit {limit}")]
    RootBytesLimitExceeded {
        /// Configured byte limit.
        limit: u64,
        /// First rejected aggregate size.
        actual: u64,
    },
    /// A canonical logical path did not exist in the scanned table.
    #[error("canonical source path `{logical_path}` was not found")]
    PathNotFound {
        /// Missing canonical logical path.
        logical_path: String,
    },
    /// The canonical table could not be length-framed on this host.
    #[error("canonical source table exceeds supported length framing")]
    CanonicalTableTooLarge,
}

impl SourceScanError {
    /// Returns a stable diagnostic code suitable for golden fixtures.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NfcCollision { .. } | Self::CaseFoldCollision { .. } => "compose.path_collision",
            Self::RootFilesLimitExceeded { .. }
            | Self::RootBytesLimitExceeded { .. }
            | Self::CanonicalTableTooLarge => "compose.source_limit",
            Self::RootNotAbsolute { .. }
            | Self::RootNotDirectory { .. }
            | Self::UnsupportedRootNamespace { .. }
            | Self::Io { .. }
            | Self::NonUtf8Path { .. }
            | Self::InvalidLogicalPath { .. }
            | Self::LinkOrReparsePoint { .. }
            | Self::UnsupportedFileType { .. }
            | Self::PathNotFound { .. } => "compose.import_denied",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn scan_is_stable_and_hashes_raw_bytes() {
        let first = TestDirectory::new("stable-first");
        first.write("z.ncl", b"z\r\n");
        first.write("nested/a.ncl", b"a\n");

        let second = TestDirectory::new("stable-second");
        second.write("nested/a.ncl", b"a\n");
        second.write("z.ncl", b"z\r\n");

        let first_table = scan(&first, limits(8, 64));
        let second_table = scan(&second, limits(8, 64));
        assert_eq!(first_table.source_hash(), second_table.source_hash());
        assert_eq!(
            first_table
                .files()
                .get("z.ncl")
                .map(FileReceipt::content_hash),
            Some(CanonicalHash::digest(b"z\r\n"))
        );
    }

    #[test]
    fn scan_enforces_file_and_source_byte_limits() {
        let directory = TestDirectory::new("limits");
        directory.write("a.ncl", b"abc");
        directory.write("b.ncl", b"def");

        assert!(matches!(
            try_scan(&directory, limits(1, 64)),
            Err(SourceScanError::RootFilesLimitExceeded { .. })
        ));
        assert!(matches!(
            try_scan(&directory, limits(8, 5)),
            Err(SourceScanError::RootBytesLimitExceeded { .. })
        ));
    }

    #[test]
    fn canonical_path_lookup_resolves_in_root_dot_segments_and_rejects_escape() {
        let directory = TestDirectory::new("imports");
        directory.write("nested/a.ncl", b"a");
        let table = scan(&directory, limits(8, 64));

        assert!(table.resolve_path("nested/a.ncl").is_ok());
        assert!(table.resolve_path("nested/./a.ncl").is_ok());
        assert!(table.resolve_path("nested/../nested/a.ncl").is_ok());
        assert!(matches!(
            table.resolve_path("../a.ncl"),
            Err(SourceScanError::InvalidLogicalPath { .. })
        ));
        for path in ["/nested/a.ncl", "C:/nested/a.ncl", "nested\\a.ncl"] {
            assert!(matches!(
                table.resolve_path(path),
                Err(SourceScanError::InvalidLogicalPath { .. })
            ));
        }
    }

    #[test]
    fn normalized_and_case_folded_collisions_are_deterministic() {
        let mut state = ScanState::new(limits(8, 64));
        assert!(state.register_path("café.ncl", "café.ncl").is_ok());
        assert!(matches!(
            state.register_path("café.ncl", "cafe\u{301}.ncl"),
            Err(SourceScanError::NfcCollision { .. })
        ));

        let mut state = ScanState::new(limits(8, 64));
        assert!(state.register_path("Straße.ncl", "Straße.ncl").is_ok());
        assert!(matches!(
            state.register_path("STRASSE.ncl", "STRASSE.ncl"),
            Err(SourceScanError::CaseFoldCollision { .. })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_network_device_and_verbatim_roots_are_rejected() {
        let source_id: SourceId = "latticeaxiom:source/test"
            .parse()
            .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}"));
        for path in [
            r"\\server\share\package",
            r"\\.\C:\package",
            r"\\?\C:\package",
            r"\\?\UNC\server\share\package",
        ] {
            assert!(matches!(
                AuthorizedRoot::new(source_id.clone(), AuthorizedRootKind::Test, path),
                Err(SourceScanError::UnsupportedRootNamespace { .. })
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn scan_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new("symlink");
        directory.write("target.ncl", b"target");
        let link = directory.path().join("link.ncl");
        symlink("target.ncl", &link)
            .unwrap_or_else(|error| panic!("failed to create test symlink: {error}"));

        assert!(matches!(
            try_scan(&directory, limits(8, 64)),
            Err(SourceScanError::LinkOrReparsePoint { .. })
        ));
    }

    fn limits(maximum_files: u32, maximum_bytes: u64) -> SourceScanLimits {
        SourceScanLimits {
            maximum_files,
            maximum_bytes,
        }
    }

    fn scan(directory: &TestDirectory, limits: SourceScanLimits) -> CanonicalSourceTable {
        try_scan(directory, limits).unwrap_or_else(|error| panic!("source scan failed: {error}"))
    }

    fn try_scan(
        directory: &TestDirectory,
        limits: SourceScanLimits,
    ) -> Result<CanonicalSourceTable, SourceScanError> {
        let root = AuthorizedRoot::new(
            "latticeaxiom:source/test"
                .parse()
                .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
            AuthorizedRootKind::Test,
            directory.path(),
        )?;
        scan_source_root(&root, limits)
    }

    #[derive(Debug)]
    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "latticeaxiom-imports-{}-{label}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap_or_else(|error| {
                panic!(
                    "failed to create test directory {}: {error}",
                    path.display()
                )
            });
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write(&self, logical_path: &str, bytes: &[u8]) {
            let path = self
                .path
                .join(logical_path.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap_or_else(|error| {
                    panic!("failed to create test parent {}: {error}", parent.display())
                });
            }
            fs::write(&path, bytes).unwrap_or_else(|error| {
                panic!("failed to write test source {}: {error}", path.display())
            });
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.path);
        }
    }
}
