//! Deterministic source-closure authorization and accounting.
//!
//! This module operates on an immutable source snapshot. It never consults the
//! ambient filesystem. Static Nickel import extraction is feature-gated, while
//! the authorization protocol and graph algorithm remain available without the
//! evaluator feature.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use latticeaxiom_core::{CanonicalHash, SourceId, canonical_json_bytes};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::{
    AuthorizedRootKind, NICKEL_LIBRARY_CONTRACT_MAJOR, NickelEvaluationLimits,
    R0_AUTHORING_CORPUS_MAJOR,
};

const CLOSURE_HASH_DOMAIN: &[u8] = b"latticeaxiom:source-closure/r0\0";
/// Nickel package alias reserved for the supported `latticeaxiom.lib` major.
pub const R0_LIBRARY_PACKAGE_ALIAS: &str = "latticeaxiom_lib_v2";
const NICKEL_RESERVED_IDENTIFIERS: &[&str] = &[
    "Dyn",
    "Number",
    "Bool",
    "String",
    "Array",
    "if",
    "then",
    "else",
    "forall",
    "in",
    "let",
    "rec",
    "match",
    "null",
    "true",
    "false",
    "fun",
    "import",
    "merge",
    "default",
    "doc",
    "optional",
    "priority",
    "force",
    "not_exported",
];

/// An immutable source location within one explicitly granted root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceAddress {
    source_id: SourceId,
    logical_path: String,
}

impl SourceAddress {
    /// Creates an address from a source-root ID and canonical logical path.
    ///
    /// # Errors
    ///
    /// Returns [`SourceClosureError::InvalidLogicalPath`] when the path is not
    /// an NFC, slash-separated, root-relative canonical path.
    pub fn new(
        source_id: SourceId,
        logical_path: impl Into<String>,
    ) -> Result<Self, SourceClosureError> {
        let logical_path = logical_path.into();
        let canonical = canonical_snapshot_path(&logical_path)?;
        if canonical != logical_path {
            return Err(SourceClosureError::InvalidLogicalPath {
                value: logical_path,
                reason: "snapshot addresses must already be canonical",
            });
        }

        Ok(Self {
            source_id,
            logical_path: canonical,
        })
    }

    /// Returns the stable identity of the containing source root.
    #[must_use]
    pub const fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Returns the NFC, slash-separated, root-relative logical path.
    #[must_use]
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }
}

impl fmt::Display for SourceAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}#{}", self.source_id, self.logical_path)
    }
}

impl<'de> Deserialize<'de> for SourceAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            source_id: SourceId,
            logical_path: String,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(fields.source_id, fields.logical_path).map_err(de::Error::custom)
    }
}

/// A package-local symbolic import name.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct PackageAlias(String);

impl PackageAlias {
    /// Creates a stable package alias.
    ///
    /// This follows Nickel 0.18's unquoted identifier grammar
    /// `_*[a-zA-Z][_a-zA-Z0-9-']*` and excludes its reserved keywords.
    /// Contextual identifiers such as `as`, `include`, and `or` remain valid.
    ///
    /// # Errors
    ///
    /// Returns [`SourceClosureError::InvalidPackageAlias`] when the alias is
    /// outside Nickel's unquoted identifier grammar or is a reserved keyword.
    pub fn new(value: impl Into<String>) -> Result<Self, SourceClosureError> {
        let value = value.into();
        let bytes = value.as_bytes();
        let first_non_underscore = bytes.iter().position(|byte| *byte != b'_');
        let valid = first_non_underscore.is_some_and(|index| {
            bytes[index].is_ascii_alphabetic()
                && bytes[index + 1..].iter().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'\'')
                })
        }) && !NICKEL_RESERVED_IDENTIFIERS.contains(&value.as_str());
        if !valid {
            return Err(SourceClosureError::InvalidPackageAlias { value });
        }

        Ok(Self(value))
    }

    /// Returns the canonical alias text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageAlias {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PackageAlias {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// A source format selected by an authored import.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceFormat {
    /// Nickel source evaluated as code.
    Nickel,
    /// JSON data imported as a value.
    Json,
    /// YAML data imported as a value.
    Yaml,
    /// TOML data imported as a value.
    Toml,
    /// Plain UTF-8 text imported as a string.
    Text,
}

/// Immutable authority granted to one source root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRootGrant {
    /// Stable source-root identity receiving authority.
    pub source_id: SourceId,
    /// Authorized root class recorded during acquisition.
    pub root_kind: AuthorizedRootKind,
    /// Hash of the complete canonical source table receiving authority.
    pub source_hash: CanonicalHash,
    /// Explicit logical entry used when this root is imported as a package.
    pub entry: SourceAddress,
    /// Optional globally unique Nickel package name for this root.
    #[serde(default)]
    pub package_alias: Option<PackageAlias>,
}

/// A request to compute one authorized source closure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClosureRequest {
    /// Exact `latticeaxiom.lib` contract major used to scan this request.
    pub library_contract_major: u32,
    /// Exact authoring corpus major shared by the controller and worker.
    pub corpus_major: u32,
    /// Nickel entry source at import depth zero.
    pub entry: SourceAddress,
    /// Complete root grants. Duplicate source IDs are rejected.
    pub root_grants: Vec<SourceRootGrant>,
    /// Effective composition limits recorded in the lock.
    pub limits: NickelEvaluationLimits,
}

impl SourceClosureRequest {
    /// Validates all request-local authority and resource-limit invariants.
    ///
    /// Snapshot identities and hashes are validated separately by the closure
    /// resolver because snapshots are not embedded in this DTO.
    ///
    /// # Errors
    ///
    /// Returns [`SourceClosureError`] for zero limits, malformed grants,
    /// duplicate roots or package aliases, a reserved library alias attached
    /// to a non-library root, or an ungranted entry root.
    pub fn validate(&self) -> Result<(), SourceClosureError> {
        if self.library_contract_major != NICKEL_LIBRARY_CONTRACT_MAJOR {
            return Err(SourceClosureError::UnsupportedLibraryContractMajor {
                found: self.library_contract_major,
                supported: NICKEL_LIBRARY_CONTRACT_MAJOR,
            });
        }
        if self.corpus_major != R0_AUTHORING_CORPUS_MAJOR {
            return Err(SourceClosureError::UnsupportedCorpusMajor {
                found: self.corpus_major,
                supported: R0_AUTHORING_CORPUS_MAJOR,
            });
        }
        self.limits
            .validate()
            .map_err(|error| SourceClosureError::InvalidEvaluationLimits {
                details: error.to_string(),
            })?;
        let grants = validate_grants(&self.root_grants)?;
        ensure_root_granted(&grants.by_source, self.entry.source_id())
    }
}

impl<'de> Deserialize<'de> for SourceClosureRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            library_contract_major: u32,
            corpus_major: u32,
            entry: SourceAddress,
            root_grants: Vec<SourceRootGrant>,
            limits: NickelEvaluationLimits,
        }

        let fields = Fields::deserialize(deserializer)?;
        let request = Self {
            library_contract_major: fields.library_contract_major,
            corpus_major: fields.corpus_major,
            entry: fields.entry,
            root_grants: fields.root_grants,
            limits: fields.limits,
        };
        request.validate().map_err(de::Error::custom)?;
        Ok(request)
    }
}

impl<'de> Deserialize<'de> for SourceRootGrant {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            source_id: SourceId,
            root_kind: AuthorizedRootKind,
            source_hash: CanonicalHash,
            entry: SourceAddress,
            #[serde(default)]
            package_alias: Option<PackageAlias>,
        }

        let fields = Fields::deserialize(deserializer)?;
        let grant = Self {
            source_id: fields.source_id,
            root_kind: fields.root_kind,
            source_hash: fields.source_hash,
            entry: fields.entry,
            package_alias: fields.package_alias,
        };
        validate_grant(&grant).map_err(de::Error::custom)?;
        Ok(grant)
    }
}

/// One static import extracted from authored Nickel syntax.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StaticSourceImport {
    /// A path resolved relative to the importing source and within its root.
    Relative {
        /// Authored import path before lexical normalization.
        path: String,
        /// Input format selected by Nickel syntax.
        format: SourceFormat,
    },
    /// A package-local alias resolved only through an explicit root grant.
    Package {
        /// Symbolic package dependency name.
        alias: PackageAlias,
    },
}

/// Stable root identity exposed by a frozen source snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSnapshotAuthority {
    /// Source identity carried inside the snapshot rather than its container.
    pub source_id: SourceId,
    /// Root class carried inside the snapshot.
    pub root_kind: AuthorizedRootKind,
    /// Complete canonical source-table hash carried inside the snapshot.
    pub source_hash: CanonicalHash,
}

/// Read-only access to exact bytes in a frozen source snapshot.
///
/// Implementations must return the same bytes for an address throughout one
/// closure computation and must not fall back to ambient filesystem reads.
pub trait SourceSnapshotView: sealed::Sealed {
    /// Returns the authority carried inside the selected snapshot root.
    fn source_authority(&self, source_id: &SourceId) -> Option<SourceSnapshotAuthority>;

    /// Returns the exact raw bytes at `address`, or `None` when absent.
    fn source_bytes(&self, address: &SourceAddress) -> Option<&[u8]>;
}

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for crate::SourceSnapshot {}

impl SourceSnapshotView for crate::SourceSnapshot {
    fn source_authority(&self, source_id: &SourceId) -> Option<SourceSnapshotAuthority> {
        (self.source_id() == source_id).then(|| SourceSnapshotAuthority {
            source_id: self.source_id().clone(),
            root_kind: self.root_kind(),
            source_hash: self.source_hash(),
        })
    }

    fn source_bytes(&self, address: &SourceAddress) -> Option<&[u8]> {
        if self.source_id() == address.source_id() {
            self.files()
                .get(address.logical_path())
                .map(crate::SourceFileSnapshot::bytes)
        } else {
            None
        }
    }
}

impl sealed::Sealed for BTreeMap<SourceId, crate::SourceSnapshot> {}

impl SourceSnapshotView for BTreeMap<SourceId, crate::SourceSnapshot> {
    fn source_authority(&self, source_id: &SourceId) -> Option<SourceSnapshotAuthority> {
        self.get(source_id).map(|snapshot| SourceSnapshotAuthority {
            source_id: snapshot.source_id().clone(),
            root_kind: snapshot.root_kind(),
            source_hash: snapshot.source_hash(),
        })
    }

    fn source_bytes(&self, address: &SourceAddress) -> Option<&[u8]> {
        self.get(address.source_id())
            .filter(|snapshot| snapshot.source_id() == address.source_id())
            .and_then(|snapshot| snapshot.files().get(address.logical_path()))
            .map(crate::SourceFileSnapshot::bytes)
    }
}

/// Extracts static imports from one Nickel source.
#[cfg(any(test, feature = "nickel-evaluator"))]
pub(crate) trait StaticImportScanner {
    /// Returns every syntactic import in the source.
    ///
    /// Implementations may return source order. The closure builder sorts and
    /// deduplicates imports before using them.
    ///
    /// # Errors
    ///
    /// Returns [`SourceClosureError`] when source decoding or parsing fails.
    fn scan_imports(
        &mut self,
        address: &SourceAddress,
        source: &[u8],
    ) -> Result<Vec<StaticSourceImport>, SourceClosureError>;
}

/// One source included in an authorized closure.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClosureMember {
    /// Stable source address.
    pub address: SourceAddress,
    /// Format under which Nickel imports this source.
    pub format: SourceFormat,
    /// Longest import distance from the entry.
    pub depth: u32,
    /// Exact raw source size.
    pub byte_length: u64,
    /// Digest of the exact raw source bytes.
    pub content_hash: CanonicalHash,
}

/// One resolved edge in the authorized import graph.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClosureEdge {
    /// Importing source.
    pub parent: SourceAddress,
    /// Resolved imported source.
    pub target: SourceAddress,
    /// Format selected at the import site.
    pub format: SourceFormat,
}

/// Deterministic receipt for one complete source closure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClosureReceipt {
    /// Entry source at depth zero.
    pub entry: SourceAddress,
    /// Sources in canonical address order.
    pub sources: Vec<SourceClosureMember>,
    /// Resolved edges in canonical parent/target/format order.
    pub edges: Vec<SourceClosureEdge>,
    /// Number of unique sources, including the entry.
    pub imported_files: u32,
    /// Aggregate raw bytes of unique sources, including the entry.
    pub source_bytes: u64,
    /// Longest import distance from the entry.
    pub maximum_depth: u32,
    /// Domain-separated canonical hash of this receipt excluding this field.
    pub closure_hash: CanonicalHash,
}

impl SourceClosureReceipt {
    /// Verifies the complete receipt graph, meters, ordering, and hash.
    ///
    /// # Errors
    ///
    /// Returns [`SourceClosureError::InvalidClosureReceipt`] when graph or
    /// accounting invariants fail, or a closure-hash error when the recorded
    /// hash does not authenticate the verified canonical body.
    pub fn verify(&self) -> Result<(), SourceClosureError> {
        verify_receipt_structure(self)?;
        self.verify_recorded_hash()
    }

    /// Verifies the complete receipt, including its domain-separated hash.
    ///
    /// This compatibility name intentionally performs full verification; a
    /// matching hash cannot make a structurally invalid receipt trustworthy.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::verify`].
    pub fn verify_hash(&self) -> Result<(), SourceClosureError> {
        self.verify()
    }

    /// Verifies every member length and content hash against frozen bytes.
    ///
    /// # Errors
    ///
    /// Returns a receipt verification error when the receipt is malformed, a
    /// member is absent, or retained bytes disagree with its receipt fields.
    pub fn verify_against_snapshot<S>(&self, snapshot: &S) -> Result<(), SourceClosureError>
    where
        S: SourceSnapshotView,
    {
        self.verify()?;
        for member in &self.sources {
            let bytes = snapshot.source_bytes(&member.address).ok_or_else(|| {
                SourceClosureError::SourceNotFound {
                    address: member.address.clone(),
                }
            })?;
            let byte_length = u64::try_from(bytes.len()).map_err(|_| {
                SourceClosureError::InvalidClosureReceipt {
                    reason: "snapshot member length exceeds portable representation",
                }
            })?;
            if byte_length != member.byte_length {
                return Err(SourceClosureError::ReceiptSnapshotMismatch {
                    address: member.address.clone(),
                    reason: "byte length differs from retained source bytes",
                });
            }
            if CanonicalHash::digest(bytes) != member.content_hash {
                return Err(SourceClosureError::ReceiptSnapshotMismatch {
                    address: member.address.clone(),
                    reason: "content hash differs from retained source bytes",
                });
            }
        }
        Ok(())
    }

    /// Verifies this receipt against a controller-computed expected receipt.
    ///
    /// A receipt hash authenticates only its own body. Exact comparison with
    /// the controller's independently computed closure is what proves that a
    /// worker did not omit or invent an authored import.
    ///
    /// # Errors
    ///
    /// Returns [`SourceClosureError`] when either receipt is invalid or their
    /// complete canonical bodies differ.
    pub fn verify_against_expected(
        &self,
        expected: &SourceClosureReceipt,
    ) -> Result<(), SourceClosureError> {
        expected.verify()?;
        self.verify()?;
        if self == expected {
            Ok(())
        } else {
            Err(SourceClosureError::ClosureReceiptMismatch)
        }
    }

    /// Recomputes the authored Nickel closure and verifies this worker receipt.
    ///
    /// This controller-boundary operation rescans the immutable source bytes
    /// with the fixed Nickel scanner, applies the request's grants and limits,
    /// and then requires exact receipt equality. A self-consistent receipt hash
    /// alone is never treated as proof of closure completeness.
    ///
    /// # Errors
    ///
    /// Returns [`SourceClosureError`] when preflight fails or the worker receipt
    /// differs from the independently computed expected closure.
    #[cfg(feature = "nickel-evaluator")]
    pub fn verify_against_request_and_snapshot<S>(
        &self,
        request: &SourceClosureRequest,
        snapshot: &S,
    ) -> Result<(), SourceClosureError>
    where
        S: SourceSnapshotView,
    {
        let expected = resolve_nickel_source_closure(snapshot, request)?;
        self.verify_against_expected(&expected)
    }

    fn verify_recorded_hash(&self) -> Result<(), SourceClosureError> {
        let actual = closure_hash(
            &self.entry,
            &self.sources,
            &self.edges,
            self.imported_files,
            self.source_bytes,
            self.maximum_depth,
        )?;
        if actual == self.closure_hash {
            Ok(())
        } else {
            Err(SourceClosureError::ClosureHashMismatch {
                receipt_hash: self.closure_hash,
                actual_hash: actual,
            })
        }
    }
}

impl<'de> Deserialize<'de> for SourceClosureReceipt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            entry: SourceAddress,
            sources: Vec<SourceClosureMember>,
            edges: Vec<SourceClosureEdge>,
            imported_files: u32,
            source_bytes: u64,
            maximum_depth: u32,
            closure_hash: CanonicalHash,
        }

        let fields = Fields::deserialize(deserializer)?;
        let receipt = Self {
            entry: fields.entry,
            sources: fields.sources,
            edges: fields.edges,
            imported_files: fields.imported_files,
            source_bytes: fields.source_bytes,
            maximum_depth: fields.maximum_depth,
            closure_hash: fields.closure_hash,
        };
        receipt.verify().map_err(de::Error::custom)?;
        Ok(receipt)
    }
}

/// A deterministic closed path proving an import cycle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCycle {
    /// Cycle members with the repeated start address as the final element.
    pub sources: Vec<SourceAddress>,
}

impl fmt::Display for SourceCycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, address) in self.sources.iter().enumerate() {
            if index > 0 {
                formatter.write_str(" -> ")?;
            }
            address.fmt(formatter)?;
        }
        Ok(())
    }
}

/// A stable failure while authorizing or accounting a source closure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SourceClosureError {
    /// A snapshot address or relative import path violated lexical policy.
    #[error("invalid logical source path `{value}`: {reason}")]
    InvalidLogicalPath {
        /// Rejected logical path.
        value: String,
        /// Violated lexical rule.
        reason: &'static str,
    },
    /// A package alias was not a stable symbolic name.
    #[error("invalid package alias `{value}`")]
    InvalidPackageAlias {
        /// Rejected package alias.
        value: String,
    },
    /// One or more effective evaluator limits were zero.
    #[error("invalid Nickel evaluation limits: {details}")]
    InvalidEvaluationLimits {
        /// Stable limit-validation diagnostic.
        details: String,
    },
    /// The request named an unsupported `latticeaxiom.lib` contract major.
    #[error("unsupported latticeaxiom.lib contract major {found}; supported major is {supported}")]
    UnsupportedLibraryContractMajor {
        /// Requested library contract major.
        found: u32,
        /// Library contract major implemented by this evaluator.
        supported: u32,
    },
    /// The request named an unsupported authoring corpus major.
    #[error("unsupported authoring corpus major {found}; supported major is {supported}")]
    UnsupportedCorpusMajor {
        /// Requested corpus major.
        found: u32,
        /// Corpus major implemented by this evaluator.
        supported: u32,
    },
    /// More than one grant described the same source root.
    #[error("duplicate source-root grant `{source_id}`")]
    DuplicateRootGrant {
        /// Duplicated source-root identity.
        source_id: SourceId,
    },
    /// A root grant entry named a different source root.
    #[error("source-root grant `{source_id}` has entry in different root `{entry_source_id}`")]
    GrantEntryRootMismatch {
        /// Source root receiving the grant.
        source_id: Box<SourceId>,
        /// Source root named by the entry address.
        entry_source_id: Box<SourceId>,
    },
    /// A source addressed a root outside the explicit grant set.
    #[error("source root `{source_id}` is not granted")]
    RootNotGranted {
        /// Denied source-root identity.
        source_id: SourceId,
    },
    /// Two granted roots declared the same globally unique package alias.
    #[error(
        "package alias `{alias}` is declared by both `{first_source_id}` and `{second_source_id}`"
    )]
    DuplicatePackageAlias {
        /// Duplicated package alias.
        alias: PackageAlias,
        /// First source root in canonical source-ID order.
        first_source_id: Box<SourceId>,
        /// Second source root in canonical source-ID order.
        second_source_id: Box<SourceId>,
    },
    /// The current Lattice library alias named a non-library root.
    #[error(
        "reserved package alias `{alias}` requires a versioned-library root, but `{source_id}` is {root_kind:?}"
    )]
    ReservedLibraryAliasWrongKind {
        /// Reserved library package alias.
        alias: PackageAlias,
        /// Source root incorrectly claiming the reserved alias.
        source_id: SourceId,
        /// Actual granted root kind.
        root_kind: AuthorizedRootKind,
    },
    /// A versioned-library root used an alias for a different contract major.
    #[error(
        "versioned-library root `{source_id}` uses alias `{alias}`; expected `{expected_alias}`"
    )]
    LibraryAliasMajorMismatch {
        /// Library source root with a mismatched alias.
        source_id: SourceId,
        /// Rejected alias.
        alias: PackageAlias,
        /// Alias bound to the request's supported library contract major.
        expected_alias: &'static str,
    },
    /// Distinct authored imports resolved to one canonical graph edge.
    #[error(
        "authored imports `{first_authored}` and `{second_authored}` from `{parent}` collide at `{target}`"
    )]
    PackageAliasCollision {
        /// Importing source containing both spellings.
        parent: Box<SourceAddress>,
        /// First authored import in scanner order.
        first_authored: String,
        /// Second distinct authored import in scanner order.
        second_authored: String,
        /// Shared canonical target.
        target: Box<SourceAddress>,
    },
    /// An authored package import had no explicit alias binding.
    #[error("package alias `{alias}` is not granted from source root `{source_id}`")]
    PackageAliasNotGranted {
        /// Importing source-root identity.
        source_id: SourceId,
        /// Missing alias.
        alias: PackageAlias,
    },
    /// An authorized address was absent from the immutable snapshot.
    #[error("source `{address}` is absent from the immutable snapshot")]
    SourceNotFound {
        /// Missing source address.
        address: SourceAddress,
    },
    /// A granted root had no corresponding immutable snapshot.
    #[error("granted source root `{source_id}` is absent from the snapshot set")]
    SnapshotRootMissing {
        /// Granted source root identity.
        source_id: SourceId,
    },
    /// A snapshot map key selected a snapshot carrying a different identity.
    #[error(
        "snapshot selected for `{requested_source_id}` carries source ID `{snapshot_source_id}`"
    )]
    SnapshotSourceIdMismatch {
        /// Source identity requested from the snapshot collection.
        requested_source_id: Box<SourceId>,
        /// Source identity stored inside the selected snapshot.
        snapshot_source_id: Box<SourceId>,
    },
    /// A grant root kind disagreed with immutable snapshot authority.
    #[error(
        "granted root `{source_id}` kind {grant_kind:?} differs from snapshot kind {snapshot_kind:?}"
    )]
    SnapshotRootKindMismatch {
        /// Granted source root identity.
        source_id: SourceId,
        /// Kind declared by the grant.
        grant_kind: AuthorizedRootKind,
        /// Kind carried by the snapshot.
        snapshot_kind: AuthorizedRootKind,
    },
    /// A grant source hash disagreed with immutable snapshot authority.
    #[error(
        "granted root `{source_id}` hash {grant_hash} differs from snapshot hash {snapshot_hash}"
    )]
    SnapshotSourceHashMismatch {
        /// Granted source root identity.
        source_id: Box<SourceId>,
        /// Hash declared by the grant.
        grant_hash: Box<CanonicalHash>,
        /// Hash carried by the snapshot.
        snapshot_hash: Box<CanonicalHash>,
    },
    /// One source was imported under incompatible formats.
    #[error("source `{address}` is imported as both {first:?} and {second:?}")]
    ConflictingSourceFormat {
        /// Ambiguous source address.
        address: SourceAddress,
        /// Format established by the first deterministic edge.
        first: SourceFormat,
        /// Conflicting later format.
        second: SourceFormat,
    },
    /// A Nickel source was not valid UTF-8.
    #[error("Nickel source `{address}` is not valid UTF-8")]
    NonUtf8NickelSource {
        /// Source that could not be decoded.
        address: SourceAddress,
    },
    /// A Nickel import path was not valid UTF-8.
    #[error("Nickel source `{address}` contains a non-UTF-8 import path")]
    NonUtf8ImportPath {
        /// Source containing the rejected import.
        address: SourceAddress,
    },
    /// Static Nickel parsing failed.
    #[error("static import scan failed for `{address}`: {details}")]
    StaticImportScanFailed {
        /// Source that failed parsing.
        address: SourceAddress,
        /// Stable parser failure summary.
        details: String,
    },
    /// A cycle was encountered in the reachable import graph.
    #[error("source import cycle: {cycle}")]
    ImportCycle {
        /// Deterministic closed cycle path.
        cycle: SourceCycle,
    },
    /// The unique closure source count exceeded policy.
    #[error("source closure file count {actual} exceeds limit {limit}")]
    ImportedFilesLimitExceeded {
        /// Configured inclusive maximum.
        limit: u32,
        /// First rejected unique source count.
        actual: u64,
    },
    /// Aggregate unique raw source bytes exceeded policy.
    #[error("source closure bytes {actual} exceed limit {limit}")]
    SourceBytesLimitExceeded {
        /// Configured inclusive maximum.
        limit: u64,
        /// First rejected aggregate byte count.
        actual: u64,
    },
    /// The longest reachable import distance exceeded policy.
    #[error("source closure import depth {actual} exceeds limit {limit}")]
    ImportDepthLimitExceeded {
        /// Configured inclusive maximum.
        limit: u32,
        /// First rejected import depth.
        actual: u64,
    },
    /// A portable closure counter could not represent the computed value.
    #[error("source closure counters exceed their portable representation")]
    CounterOverflow,
    /// The typed receipt body could not be canonically encoded.
    #[error("source closure hash encoding failed: {details}")]
    ClosureHashEncoding {
        /// Canonical encoder diagnostic.
        details: String,
    },
    /// The recorded closure hash did not match its receipt body.
    #[error("source closure hash mismatch: receipt {receipt_hash}, actual {actual_hash}")]
    ClosureHashMismatch {
        /// Hash carried by the receipt.
        receipt_hash: CanonicalHash,
        /// Hash recomputed from the receipt body.
        actual_hash: CanonicalHash,
    },
    /// A closure receipt violated structural or accounting invariants.
    #[error("invalid source closure receipt: {reason}")]
    InvalidClosureReceipt {
        /// Stable violated invariant.
        reason: &'static str,
    },
    /// A receipt member disagreed with retained immutable source bytes.
    #[error("source closure receipt member `{address}` is invalid: {reason}")]
    ReceiptSnapshotMismatch {
        /// Member whose bytes did not match.
        address: SourceAddress,
        /// Stable mismatch class.
        reason: &'static str,
    },
    /// A worker receipt differed from the controller-computed closure.
    #[error("worker source closure receipt differs from the controller preflight receipt")]
    ClosureReceiptMismatch,
}

impl SourceClosureError {
    /// Returns the stable Lattice diagnostic code for this failure class.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ImportCycle { .. } => "compose.import_cycle",
            Self::ImportedFilesLimitExceeded { .. } => "compose.import_files",
            Self::ImportDepthLimitExceeded { .. } => "compose.import_depth",
            Self::SourceBytesLimitExceeded { .. } | Self::CounterOverflow => "compose.source_limit",
            Self::ClosureHashEncoding { .. }
            | Self::ClosureHashMismatch { .. }
            | Self::InvalidClosureReceipt { .. }
            | Self::ReceiptSnapshotMismatch { .. }
            | Self::InvalidEvaluationLimits { .. }
            | Self::UnsupportedLibraryContractMajor { .. }
            | Self::UnsupportedCorpusMajor { .. } => "compose.policy_violation",
            Self::ClosureReceiptMismatch => "compose.worker_protocol",
            Self::StaticImportScanFailed { .. } | Self::NonUtf8NickelSource { .. } => {
                "compose.evaluation_failed"
            }
            Self::InvalidLogicalPath { .. }
            | Self::InvalidPackageAlias { .. }
            | Self::DuplicateRootGrant { .. }
            | Self::GrantEntryRootMismatch { .. }
            | Self::RootNotGranted { .. }
            | Self::DuplicatePackageAlias { .. }
            | Self::ReservedLibraryAliasWrongKind { .. }
            | Self::LibraryAliasMajorMismatch { .. }
            | Self::PackageAliasCollision { .. }
            | Self::PackageAliasNotGranted { .. }
            | Self::SourceNotFound { .. }
            | Self::SnapshotRootMissing { .. }
            | Self::SnapshotSourceIdMismatch { .. }
            | Self::SnapshotRootKindMismatch { .. }
            | Self::SnapshotSourceHashMismatch { .. }
            | Self::ConflictingSourceFormat { .. }
            | Self::NonUtf8ImportPath { .. } => "compose.import_denied",
        }
    }
}

#[cfg(any(test, feature = "nickel-evaluator"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisitState {
    Visiting,
    Complete,
}

#[cfg(any(test, feature = "nickel-evaluator"))]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ResolvedImport {
    target: SourceAddress,
    format: SourceFormat,
}

#[derive(Debug)]
struct ValidatedGrants<'a> {
    by_source: BTreeMap<SourceId, &'a SourceRootGrant>,
    #[cfg(any(test, feature = "nickel-evaluator"))]
    by_alias: BTreeMap<PackageAlias, &'a SourceRootGrant>,
}

#[cfg(any(test, feature = "nickel-evaluator"))]
#[derive(Debug)]
struct DfsFrame {
    address: SourceAddress,
    imports: Vec<ResolvedImport>,
    next_import: usize,
}

#[cfg(any(test, feature = "nickel-evaluator"))]
#[derive(Debug)]
struct ClosureGraph {
    nodes: BTreeMap<SourceAddress, NodeReceipt>,
    edges: BTreeMap<SourceAddress, BTreeSet<ResolvedImport>>,
    total_bytes: u64,
}

#[cfg(any(test, feature = "nickel-evaluator"))]
#[derive(Clone, Debug)]
struct NodeReceipt {
    format: SourceFormat,
    byte_length: u64,
    content_hash: CanonicalHash,
}

/// Resolves, authorizes, and accounts the complete static source closure.
///
/// The entry has depth zero. `imported_files` and `source_bytes` both include
/// the entry. Reimports and diamond-shaped dependencies are counted once.
/// Cycles are rejected before any evaluation begins.
///
/// # Errors
///
/// Returns [`SourceClosureError`] for invalid authority, paths, aliases,
/// missing snapshot data, static-import scan failures, cycles, format
/// conflicts, counter overflow, or an exceeded source limit.
#[allow(
    clippy::too_many_lines,
    reason = "the iterative DFS is clearer as one state machine with shared invariants"
)]
#[cfg(any(test, feature = "nickel-evaluator"))]
pub(crate) fn resolve_source_closure<S, P>(
    snapshot: &S,
    request: &SourceClosureRequest,
    scanner: &mut P,
) -> Result<SourceClosureReceipt, SourceClosureError>
where
    S: SourceSnapshotView,
    P: StaticImportScanner,
{
    request.validate()?;
    let grants = validate_grants(&request.root_grants)?;
    validate_snapshot_authority(snapshot, &grants)?;

    let mut graph = ClosureGraph {
        nodes: BTreeMap::new(),
        edges: BTreeMap::new(),
        total_bytes: 0,
    };
    let mut states = BTreeMap::new();
    let mut active_path = Vec::new();

    let entry_imports = discover_source(
        snapshot,
        scanner,
        &grants,
        &request.entry,
        SourceFormat::Nickel,
        request.limits,
        &mut graph,
    )?;
    states.insert(request.entry.clone(), VisitState::Visiting);
    active_path.push(request.entry.clone());
    let mut stack = vec![DfsFrame {
        address: request.entry.clone(),
        imports: entry_imports,
        next_import: 0,
    }];

    while !stack.is_empty() {
        let next = {
            let frame = stack
                .last_mut()
                .ok_or(SourceClosureError::CounterOverflow)?;
            if let Some(import) = frame.imports.get(frame.next_import).cloned() {
                frame.next_import = frame.next_import.saturating_add(1);
                Some((frame.address.clone(), import))
            } else {
                None
            }
        };

        let Some((parent, import)) = next else {
            let frame = stack.pop().ok_or(SourceClosureError::CounterOverflow)?;
            states.insert(frame.address.clone(), VisitState::Complete);
            let popped = active_path
                .pop()
                .ok_or(SourceClosureError::CounterOverflow)?;
            if popped != frame.address {
                return Err(SourceClosureError::CounterOverflow);
            }
            continue;
        };

        graph
            .edges
            .entry(parent)
            .or_default()
            .insert(import.clone());

        if let Some(existing) = graph.nodes.get(&import.target)
            && existing.format != import.format
        {
            return Err(SourceClosureError::ConflictingSourceFormat {
                address: import.target,
                first: existing.format,
                second: import.format,
            });
        }

        match states.get(&import.target).copied() {
            Some(VisitState::Visiting) => {
                let start = active_path
                    .iter()
                    .position(|address| address == &import.target)
                    .ok_or(SourceClosureError::CounterOverflow)?;
                let mut sources = active_path[start..].to_vec();
                sources.push(import.target);
                return Err(SourceClosureError::ImportCycle {
                    cycle: SourceCycle { sources },
                });
            }
            Some(VisitState::Complete) => {}
            None => {
                let imports = discover_source(
                    snapshot,
                    scanner,
                    &grants,
                    &import.target,
                    import.format,
                    request.limits,
                    &mut graph,
                )?;
                states.insert(import.target.clone(), VisitState::Visiting);
                active_path.push(import.target.clone());
                stack.push(DfsFrame {
                    address: import.target,
                    imports,
                    next_import: 0,
                });
            }
        }
    }

    let depths = longest_depths(&request.entry, &graph)?;
    let maximum_depth = depths.values().copied().max().unwrap_or(0);
    if maximum_depth > request.limits.import_depth {
        return Err(SourceClosureError::ImportDepthLimitExceeded {
            limit: request.limits.import_depth,
            actual: u64::from(maximum_depth),
        });
    }

    let imported_files =
        u32::try_from(graph.nodes.len()).map_err(|_| SourceClosureError::CounterOverflow)?;
    let sources = graph
        .nodes
        .iter()
        .map(|(address, node)| {
            let depth = depths
                .get(address)
                .copied()
                .ok_or(SourceClosureError::CounterOverflow)?;
            Ok(SourceClosureMember {
                address: address.clone(),
                format: node.format,
                depth,
                byte_length: node.byte_length,
                content_hash: node.content_hash,
            })
        })
        .collect::<Result<Vec<_>, SourceClosureError>>()?;
    let edges: Vec<SourceClosureEdge> = graph
        .edges
        .into_iter()
        .flat_map(|(parent, imports)| {
            imports.into_iter().map(move |import| SourceClosureEdge {
                parent: parent.clone(),
                target: import.target,
                format: import.format,
            })
        })
        .collect();

    let closure_hash = closure_hash(
        &request.entry,
        &sources,
        &edges,
        imported_files,
        graph.total_bytes,
        maximum_depth,
    )?;
    Ok(SourceClosureReceipt {
        entry: request.entry.clone(),
        sources,
        edges,
        imported_files,
        source_bytes: graph.total_bytes,
        maximum_depth,
        closure_hash,
    })
}

fn validate_grants(
    root_grants: &[SourceRootGrant],
) -> Result<ValidatedGrants<'_>, SourceClosureError> {
    let mut by_source = BTreeMap::new();
    let mut by_alias = BTreeMap::new();
    let mut ordered_grants = root_grants.iter().collect::<Vec<_>>();
    ordered_grants.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    for grant in ordered_grants {
        validate_grant(grant)?;
        if by_source.insert(grant.source_id.clone(), grant).is_some() {
            return Err(SourceClosureError::DuplicateRootGrant {
                source_id: grant.source_id.clone(),
            });
        }
        if let Some(alias) = &grant.package_alias
            && let Some(first) = by_alias.insert(alias.clone(), grant)
        {
            return Err(SourceClosureError::DuplicatePackageAlias {
                alias: alias.clone(),
                first_source_id: Box::new(first.source_id.clone()),
                second_source_id: Box::new(grant.source_id.clone()),
            });
        }
    }

    Ok(ValidatedGrants {
        by_source,
        #[cfg(any(test, feature = "nickel-evaluator"))]
        by_alias,
    })
}

fn validate_grant(grant: &SourceRootGrant) -> Result<(), SourceClosureError> {
    if grant.entry.source_id() != &grant.source_id {
        return Err(SourceClosureError::GrantEntryRootMismatch {
            source_id: Box::new(grant.source_id.clone()),
            entry_source_id: Box::new(grant.entry.source_id().clone()),
        });
    }
    if let Some(alias) = &grant.package_alias {
        let is_versioned_library_alias = library_alias_major(alias.as_str()).is_some();
        if is_versioned_library_alias && grant.root_kind != AuthorizedRootKind::Library {
            return Err(SourceClosureError::ReservedLibraryAliasWrongKind {
                alias: alias.clone(),
                source_id: grant.source_id.clone(),
                root_kind: grant.root_kind,
            });
        }
        if grant.root_kind == AuthorizedRootKind::Library
            && alias.as_str() != R0_LIBRARY_PACKAGE_ALIAS
        {
            return Err(SourceClosureError::LibraryAliasMajorMismatch {
                source_id: grant.source_id.clone(),
                alias: alias.clone(),
                expected_alias: R0_LIBRARY_PACKAGE_ALIAS,
            });
        }
    }
    Ok(())
}

fn library_alias_major(alias: &str) -> Option<u32> {
    let major = alias.strip_prefix("latticeaxiom_lib_v")?;
    if major.is_empty() || (major.len() > 1 && major.starts_with('0')) {
        return None;
    }
    major.parse().ok()
}

#[cfg(any(test, feature = "nickel-evaluator"))]
fn validate_snapshot_authority<S>(
    snapshot: &S,
    grants: &ValidatedGrants<'_>,
) -> Result<(), SourceClosureError>
where
    S: SourceSnapshotView,
{
    for (requested_source_id, grant) in &grants.by_source {
        let authority = snapshot
            .source_authority(requested_source_id)
            .ok_or_else(|| SourceClosureError::SnapshotRootMissing {
                source_id: requested_source_id.clone(),
            })?;
        if authority.source_id != *requested_source_id {
            return Err(SourceClosureError::SnapshotSourceIdMismatch {
                requested_source_id: Box::new(requested_source_id.clone()),
                snapshot_source_id: Box::new(authority.source_id),
            });
        }
        if authority.root_kind != grant.root_kind {
            return Err(SourceClosureError::SnapshotRootKindMismatch {
                source_id: requested_source_id.clone(),
                grant_kind: grant.root_kind,
                snapshot_kind: authority.root_kind,
            });
        }
        if authority.source_hash != grant.source_hash {
            return Err(SourceClosureError::SnapshotSourceHashMismatch {
                source_id: Box::new(requested_source_id.clone()),
                grant_hash: Box::new(grant.source_hash),
                snapshot_hash: Box::new(authority.source_hash),
            });
        }
    }
    Ok(())
}

fn ensure_root_granted(
    grants: &BTreeMap<SourceId, &SourceRootGrant>,
    source_id: &SourceId,
) -> Result<(), SourceClosureError> {
    if grants.contains_key(source_id) {
        Ok(())
    } else {
        Err(SourceClosureError::RootNotGranted {
            source_id: source_id.clone(),
        })
    }
}

#[cfg(any(test, feature = "nickel-evaluator"))]
fn authored_import_label(import: &StaticSourceImport) -> String {
    match import {
        StaticSourceImport::Relative { path, format } => {
            format!("relative:{}:{path}", source_format_label(*format))
        }
        StaticSourceImport::Package { alias } => format!("package:{alias}"),
    }
}

#[cfg(any(test, feature = "nickel-evaluator"))]
const fn source_format_label(format: SourceFormat) -> &'static str {
    match format {
        SourceFormat::Nickel => "nickel",
        SourceFormat::Json => "json",
        SourceFormat::Yaml => "yaml",
        SourceFormat::Toml => "toml",
        SourceFormat::Text => "text",
    }
}

#[cfg(any(test, feature = "nickel-evaluator"))]
fn insert_resolved_import(
    parent: &SourceAddress,
    resolved: &mut BTreeMap<SourceAddress, (ResolvedImport, StaticSourceImport)>,
    target: ResolvedImport,
    authored: StaticSourceImport,
) -> Result<(), SourceClosureError> {
    if let Some((_, first)) = resolved.get(&target.target) {
        if first != &authored {
            let first_label = authored_import_label(first);
            let second_label = authored_import_label(&authored);
            let (first_authored, second_authored) = if first_label <= second_label {
                (first_label, second_label)
            } else {
                (second_label, first_label)
            };
            return Err(SourceClosureError::PackageAliasCollision {
                parent: Box::new(parent.clone()),
                first_authored,
                second_authored,
                target: Box::new(target.target),
            });
        }
    } else {
        resolved.insert(target.target.clone(), (target, authored));
    }
    Ok(())
}

#[cfg(any(test, feature = "nickel-evaluator"))]
fn discover_source<S, P>(
    snapshot: &S,
    scanner: &mut P,
    grants: &ValidatedGrants<'_>,
    address: &SourceAddress,
    format: SourceFormat,
    limits: NickelEvaluationLimits,
    graph: &mut ClosureGraph,
) -> Result<Vec<ResolvedImport>, SourceClosureError>
where
    S: SourceSnapshotView,
    P: StaticImportScanner,
{
    ensure_root_granted(&grants.by_source, address.source_id())?;
    let bytes =
        snapshot
            .source_bytes(address)
            .ok_or_else(|| SourceClosureError::SourceNotFound {
                address: address.clone(),
            })?;
    let byte_length =
        u64::try_from(bytes.len()).map_err(|_| SourceClosureError::CounterOverflow)?;
    let actual_count = u64::try_from(graph.nodes.len())
        .map_err(|_| SourceClosureError::CounterOverflow)?
        .checked_add(1)
        .ok_or(SourceClosureError::CounterOverflow)?;
    if actual_count > u64::from(limits.imported_files) {
        return Err(SourceClosureError::ImportedFilesLimitExceeded {
            limit: limits.imported_files,
            actual: actual_count,
        });
    }

    let actual_bytes = graph.total_bytes.checked_add(byte_length).ok_or(
        SourceClosureError::SourceBytesLimitExceeded {
            limit: limits.source_bytes,
            actual: u64::MAX,
        },
    )?;
    if actual_bytes > limits.source_bytes {
        return Err(SourceClosureError::SourceBytesLimitExceeded {
            limit: limits.source_bytes,
            actual: actual_bytes,
        });
    }

    let imports = if format == SourceFormat::Nickel {
        scanner.scan_imports(address, bytes)?
    } else {
        Vec::new()
    };
    let resolved = resolve_imports(grants, address, imports)?;

    graph.nodes.insert(
        address.clone(),
        NodeReceipt {
            format,
            byte_length,
            content_hash: CanonicalHash::digest(bytes),
        },
    );
    graph.total_bytes = actual_bytes;
    graph.edges.entry(address.clone()).or_default();
    Ok(resolved)
}

#[cfg(any(test, feature = "nickel-evaluator"))]
fn resolve_imports(
    grants: &ValidatedGrants<'_>,
    parent: &SourceAddress,
    imports: Vec<StaticSourceImport>,
) -> Result<Vec<ResolvedImport>, SourceClosureError> {
    ensure_root_granted(&grants.by_source, parent.source_id())?;
    let mut resolved = BTreeMap::new();

    for import in imports {
        let target = match &import {
            StaticSourceImport::Relative { path, format } => ResolvedImport {
                target: SourceAddress {
                    source_id: parent.source_id().clone(),
                    logical_path: resolve_relative_path(parent.logical_path(), path)?,
                },
                format: *format,
            },
            StaticSourceImport::Package { alias } => {
                let target_grant = grants.by_alias.get(alias).copied().ok_or_else(|| {
                    SourceClosureError::PackageAliasNotGranted {
                        source_id: parent.source_id().clone(),
                        alias: alias.clone(),
                    }
                })?;
                ResolvedImport {
                    target: target_grant.entry.clone(),
                    format: SourceFormat::Nickel,
                }
            }
        };
        insert_resolved_import(parent, &mut resolved, target, import)?;
    }

    Ok(resolved.into_values().map(|(target, _)| target).collect())
}

fn canonical_snapshot_path(path: &str) -> Result<String, SourceClosureError> {
    validate_relative_path_prefix(path)?;
    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() {
            return Err(SourceClosureError::InvalidLogicalPath {
                value: path.to_owned(),
                reason: "path segments cannot be empty",
            });
        }
        match segment {
            "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(SourceClosureError::InvalidLogicalPath {
                        value: path.to_owned(),
                        reason: "the path escapes its source root",
                    });
                }
            }
            value => segments.push(value.to_owned()),
        }
    }
    if segments.is_empty() {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: path.to_owned(),
            reason: "the path must name a source file",
        });
    }
    Ok(segments.join("/"))
}

#[cfg(any(test, feature = "nickel-evaluator"))]
fn resolve_relative_path(parent: &str, import: &str) -> Result<String, SourceClosureError> {
    validate_relative_path_prefix(import)?;
    let mut segments = parent.split('/').map(str::to_owned).collect::<Vec<_>>();
    if segments.pop().is_none() {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: parent.to_owned(),
            reason: "the importing source path has no file segment",
        });
    }

    for segment in import.split('/') {
        if segment.is_empty() {
            return Err(SourceClosureError::InvalidLogicalPath {
                value: import.to_owned(),
                reason: "path segments cannot be empty",
            });
        }
        match segment {
            "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(SourceClosureError::InvalidLogicalPath {
                        value: import.to_owned(),
                        reason: "the import escapes its source root",
                    });
                }
            }
            value => segments.push(value.to_owned()),
        }
    }

    if segments.is_empty() {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: import.to_owned(),
            reason: "the import must resolve to a source file",
        });
    }
    Ok(segments.join("/"))
}

fn validate_relative_path_prefix(path: &str) -> Result<(), SourceClosureError> {
    if path.is_empty() {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: path.to_owned(),
            reason: "the path cannot be empty",
        });
    }
    if path.starts_with('/') {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: path.to_owned(),
            reason: "absolute paths are denied",
        });
    }
    if path.as_bytes().get(1) == Some(&b':') {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: path.to_owned(),
            reason: "drive-prefixed paths are denied",
        });
    }
    if path.contains('\\') {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: path.to_owned(),
            reason: "backslash separators are denied",
        });
    }
    if path.contains('\0') {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: path.to_owned(),
            reason: "NUL is denied",
        });
    }
    if !path.nfc().eq(path.chars()) {
        return Err(SourceClosureError::InvalidLogicalPath {
            value: path.to_owned(),
            reason: "logical paths must already be NFC",
        });
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "receipt verification keeps all mutually dependent graph invariants together"
)]
fn verify_receipt_structure(receipt: &SourceClosureReceipt) -> Result<(), SourceClosureError> {
    if !receipt
        .sources
        .windows(2)
        .all(|pair| pair[0].address < pair[1].address)
    {
        return Err(SourceClosureError::InvalidClosureReceipt {
            reason: "sources must be strictly ordered by canonical address",
        });
    }
    if !receipt.edges.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(SourceClosureError::InvalidClosureReceipt {
            reason: "edges must be strictly ordered by parent, target, and format",
        });
    }

    let imported_files = u32::try_from(receipt.sources.len()).map_err(|_| {
        SourceClosureError::InvalidClosureReceipt {
            reason: "source count exceeds portable representation",
        }
    })?;
    if receipt.imported_files != imported_files {
        return Err(SourceClosureError::InvalidClosureReceipt {
            reason: "imported_files does not equal the unique source count",
        });
    }

    let source_bytes = receipt.sources.iter().try_fold(0_u64, |total, member| {
        total
            .checked_add(member.byte_length)
            .ok_or(SourceClosureError::InvalidClosureReceipt {
                reason: "aggregate source byte count overflows",
            })
    })?;
    if receipt.source_bytes != source_bytes {
        return Err(SourceClosureError::InvalidClosureReceipt {
            reason: "source_bytes does not equal member byte lengths",
        });
    }

    let members = receipt
        .sources
        .iter()
        .map(|member| (member.address.clone(), member))
        .collect::<BTreeMap<_, _>>();
    let entry =
        members
            .get(&receipt.entry)
            .copied()
            .ok_or(SourceClosureError::InvalidClosureReceipt {
                reason: "entry is absent from sources",
            })?;
    if entry.format != SourceFormat::Nickel || entry.depth != 0 {
        return Err(SourceClosureError::InvalidClosureReceipt {
            reason: "entry must be Nickel at depth zero",
        });
    }

    let mut outgoing = members
        .keys()
        .cloned()
        .map(|address| (address, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut indegrees = members
        .keys()
        .cloned()
        .map(|address| (address, 0_u32))
        .collect::<BTreeMap<_, _>>();
    for edge in &receipt.edges {
        let parent = members.get(&edge.parent).copied().ok_or(
            SourceClosureError::InvalidClosureReceipt {
                reason: "edge parent is absent from sources",
            },
        )?;
        let target = members.get(&edge.target).copied().ok_or(
            SourceClosureError::InvalidClosureReceipt {
                reason: "edge target is absent from sources",
            },
        )?;
        if parent.format != SourceFormat::Nickel {
            return Err(SourceClosureError::InvalidClosureReceipt {
                reason: "non-Nickel source has outgoing imports",
            });
        }
        if target.format != edge.format {
            return Err(SourceClosureError::InvalidClosureReceipt {
                reason: "edge format differs from target member format",
            });
        }
        if edge.parent == edge.target {
            return Err(SourceClosureError::InvalidClosureReceipt {
                reason: "self import edge is forbidden",
            });
        }
        outgoing
            .get_mut(&edge.parent)
            .ok_or(SourceClosureError::InvalidClosureReceipt {
                reason: "edge parent adjacency is absent",
            })?
            .insert(edge.target.clone());
        let indegree =
            indegrees
                .get_mut(&edge.target)
                .ok_or(SourceClosureError::InvalidClosureReceipt {
                    reason: "edge target indegree is absent",
                })?;
        *indegree = indegree
            .checked_add(1)
            .ok_or(SourceClosureError::InvalidClosureReceipt {
                reason: "edge indegree overflows",
            })?;
    }

    let mut ready = indegrees
        .iter()
        .filter_map(|(address, indegree)| (*indegree == 0).then_some(address.clone()))
        .collect::<BTreeSet<_>>();
    if ready.len() != 1 || !ready.contains(&receipt.entry) {
        return Err(SourceClosureError::InvalidClosureReceipt {
            reason: "entry must be the only zero-indegree source",
        });
    }
    let mut depths = BTreeMap::from([(receipt.entry.clone(), 0_u32)]);
    let mut processed = 0_usize;
    while let Some(address) = ready.pop_first() {
        processed = processed
            .checked_add(1)
            .ok_or(SourceClosureError::InvalidClosureReceipt {
                reason: "processed source count overflows",
            })?;
        let parent_depth =
            depths
                .get(&address)
                .copied()
                .ok_or(SourceClosureError::InvalidClosureReceipt {
                    reason: "source is unreachable from entry",
                })?;
        let child_depth =
            parent_depth
                .checked_add(1)
                .ok_or(SourceClosureError::InvalidClosureReceipt {
                    reason: "source depth overflows",
                })?;
        if let Some(targets) = outgoing.get(&address) {
            for target in targets {
                depths
                    .entry(target.clone())
                    .and_modify(|depth| *depth = (*depth).max(child_depth))
                    .or_insert(child_depth);
                let indegree =
                    indegrees
                        .get_mut(target)
                        .ok_or(SourceClosureError::InvalidClosureReceipt {
                            reason: "target indegree is absent",
                        })?;
                *indegree =
                    indegree
                        .checked_sub(1)
                        .ok_or(SourceClosureError::InvalidClosureReceipt {
                            reason: "target indegree underflows",
                        })?;
                if *indegree == 0 {
                    ready.insert(target.clone());
                }
            }
        }
    }
    if processed != members.len() || depths.len() != members.len() {
        return Err(SourceClosureError::InvalidClosureReceipt {
            reason: "source graph contains a cycle or unreachable member",
        });
    }
    for member in &receipt.sources {
        if depths.get(&member.address).copied() != Some(member.depth) {
            return Err(SourceClosureError::InvalidClosureReceipt {
                reason: "member depth is not the longest path from entry",
            });
        }
    }
    let maximum_depth = depths.values().copied().max().unwrap_or(0);
    if receipt.maximum_depth != maximum_depth {
        return Err(SourceClosureError::InvalidClosureReceipt {
            reason: "maximum_depth does not equal the longest member depth",
        });
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SourceClosureHashPayload<'a> {
    entry: &'a SourceAddress,
    sources: &'a [SourceClosureMember],
    edges: &'a [SourceClosureEdge],
    imported_files: u32,
    source_bytes: u64,
    maximum_depth: u32,
}

fn closure_hash(
    entry: &SourceAddress,
    sources: &[SourceClosureMember],
    edges: &[SourceClosureEdge],
    imported_files: u32,
    source_bytes: u64,
    maximum_depth: u32,
) -> Result<CanonicalHash, SourceClosureError> {
    let payload = SourceClosureHashPayload {
        entry,
        sources,
        edges,
        imported_files,
        source_bytes,
        maximum_depth,
    };
    let canonical = canonical_json_bytes(&payload).map_err(|error| {
        SourceClosureError::ClosureHashEncoding {
            details: error.to_string(),
        }
    })?;
    let mut framed = Vec::with_capacity(CLOSURE_HASH_DOMAIN.len().saturating_add(canonical.len()));
    framed.extend_from_slice(CLOSURE_HASH_DOMAIN);
    framed.extend_from_slice(&canonical);
    Ok(CanonicalHash::digest(framed))
}

#[cfg(any(test, feature = "nickel-evaluator"))]
fn longest_depths(
    entry: &SourceAddress,
    graph: &ClosureGraph,
) -> Result<BTreeMap<SourceAddress, u32>, SourceClosureError> {
    let mut indegrees = graph
        .nodes
        .keys()
        .cloned()
        .map(|address| (address, 0_u32))
        .collect::<BTreeMap<_, _>>();
    for imports in graph.edges.values() {
        for import in imports {
            let indegree = indegrees
                .get_mut(&import.target)
                .ok_or(SourceClosureError::CounterOverflow)?;
            *indegree = indegree
                .checked_add(1)
                .ok_or(SourceClosureError::CounterOverflow)?;
        }
    }

    let mut ready = indegrees
        .iter()
        .filter_map(|(address, indegree)| (*indegree == 0).then_some(address.clone()))
        .collect::<BTreeSet<_>>();
    let mut depths = BTreeMap::from([(entry.clone(), 0_u32)]);
    let mut processed = 0_usize;
    while let Some(address) = ready.pop_first() {
        processed = processed
            .checked_add(1)
            .ok_or(SourceClosureError::CounterOverflow)?;
        let parent_depth = depths.get(&address).copied().unwrap_or(0);
        let child_depth = parent_depth
            .checked_add(1)
            .ok_or(SourceClosureError::CounterOverflow)?;
        if let Some(imports) = graph.edges.get(&address) {
            for import in imports {
                depths
                    .entry(import.target.clone())
                    .and_modify(|depth| *depth = (*depth).max(child_depth))
                    .or_insert(child_depth);
                let indegree = indegrees
                    .get_mut(&import.target)
                    .ok_or(SourceClosureError::CounterOverflow)?;
                *indegree = indegree
                    .checked_sub(1)
                    .ok_or(SourceClosureError::CounterOverflow)?;
                if *indegree == 0 {
                    ready.insert(import.target.clone());
                }
            }
        }
    }

    if processed != graph.nodes.len() {
        return Err(SourceClosureError::CounterOverflow);
    }
    Ok(depths)
}

/// Resolves a source closure with the fixed Nickel 0.18 production scanner.
///
/// # Errors
///
/// Returns [`SourceClosureError`] for invalid request or snapshot authority,
/// static parse failure, denied imports, graph cycles, or exceeded limits.
#[cfg(feature = "nickel-evaluator")]
pub fn resolve_nickel_source_closure<S>(
    snapshot: &S,
    request: &SourceClosureRequest,
) -> Result<SourceClosureReceipt, SourceClosureError>
where
    S: SourceSnapshotView,
{
    let mut scanner = NickelStaticImportScanner;
    resolve_source_closure(snapshot, request, &mut scanner)
}

/// Nickel 0.18 static-import scanner backed only by the supplied source bytes.
#[cfg(feature = "nickel-evaluator")]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NickelStaticImportScanner;

#[cfg(feature = "nickel-evaluator")]
impl StaticImportScanner for NickelStaticImportScanner {
    fn scan_imports(
        &mut self,
        address: &SourceAddress,
        source: &[u8],
    ) -> Result<Vec<StaticSourceImport>, SourceClosureError> {
        use nickel_lang_core::ast::{AstAlloc, Import, Node};
        use nickel_lang_core::cache::{SourceCache, SourcePath};
        use nickel_lang_core::traverse::{TraverseAlloc, TraverseControl};

        let source =
            std::str::from_utf8(source).map_err(|_| SourceClosureError::NonUtf8NickelSource {
                address: address.clone(),
            })?;
        let mut cache = SourceCache::new();
        let file_id = cache.add_string(
            SourcePath::Generated(address.to_string()),
            source.to_owned(),
        );
        let alloc = AstAlloc::new();
        let ast = cache.parse_nickel(&alloc, file_id).map_err(|error| {
            SourceClosureError::StaticImportScanFailed {
                address: address.clone(),
                details: format!("{} Nickel parse error(s)", error.errors.len()),
            }
        })?;
        let mut imports = Vec::new();
        let mut failure = None;
        ast.traverse_ref(
            &mut |node: &nickel_lang_core::ast::Ast<'_>, _scope: &()| {
                if let Node::Import(import) = &node.node {
                    let result = match import {
                        Import::Path { path, format } => path
                            .to_str()
                            .ok_or_else(|| SourceClosureError::NonUtf8ImportPath {
                                address: address.clone(),
                            })
                            .map(|path| StaticSourceImport::Relative {
                                path: path.to_owned(),
                                format: source_format(*format),
                            }),
                        Import::Package { id } => PackageAlias::new(id.label())
                            .map(|alias| StaticSourceImport::Package { alias }),
                    };
                    match result {
                        Ok(import) => imports.push(import),
                        Err(error) => {
                            failure = Some(error);
                            return TraverseControl::Return(());
                        }
                    }
                }
                TraverseControl::Continue
            },
            &(),
        );
        if let Some(error) = failure {
            Err(error)
        } else {
            Ok(imports)
        }
    }
}

#[cfg(feature = "nickel-evaluator")]
fn source_format(format: nickel_lang_core::cache::InputFormat) -> SourceFormat {
    use nickel_lang_core::cache::InputFormat;

    match format {
        InputFormat::Nickel => SourceFormat::Nickel,
        InputFormat::Json => SourceFormat::Json,
        InputFormat::Yaml => SourceFormat::Yaml,
        InputFormat::Toml => SourceFormat::Toml,
        InputFormat::Text => SourceFormat::Text,
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[derive(Debug, Default)]
    struct FixtureSnapshot {
        sources: BTreeMap<SourceAddress, Vec<u8>>,
    }

    impl FixtureSnapshot {
        fn add(&mut self, address: SourceAddress, bytes: &[u8]) {
            self.sources.insert(address, bytes.to_vec());
        }
    }

    impl sealed::Sealed for FixtureSnapshot {}

    impl SourceSnapshotView for FixtureSnapshot {
        fn source_authority(&self, source_id: &SourceId) -> Option<SourceSnapshotAuthority> {
            self.sources
                .keys()
                .any(|address| address.source_id() == source_id)
                .then(|| SourceSnapshotAuthority {
                    source_id: source_id.clone(),
                    root_kind: AuthorizedRootKind::Test,
                    source_hash: fixture_source_hash(source_id),
                })
        }

        fn source_bytes(&self, address: &SourceAddress) -> Option<&[u8]> {
            self.sources.get(address).map(Vec::as_slice)
        }
    }

    #[derive(Debug, Default)]
    struct FixtureScanner {
        imports: BTreeMap<SourceAddress, Vec<StaticSourceImport>>,
    }

    impl FixtureScanner {
        fn add(&mut self, address: SourceAddress, imports: Vec<StaticSourceImport>) {
            self.imports.insert(address, imports);
        }
    }

    impl StaticImportScanner for FixtureScanner {
        fn scan_imports(
            &mut self,
            address: &SourceAddress,
            _source: &[u8],
        ) -> Result<Vec<StaticSourceImport>, SourceClosureError> {
            Ok(self.imports.get(address).cloned().unwrap_or_default())
        }
    }

    fn source_id(value: &str) -> SourceId {
        SourceId::from_str(value)
            .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}"))
    }

    fn address(source_id: &SourceId, path: &str) -> SourceAddress {
        SourceAddress::new(source_id.clone(), path)
            .unwrap_or_else(|error| panic!("fixture source address is invalid: {error}"))
    }

    fn fixture_source_hash(source_id: &SourceId) -> CanonicalHash {
        CanonicalHash::digest(source_id.to_string())
    }

    fn relative(path: &str) -> StaticSourceImport {
        StaticSourceImport::Relative {
            path: path.to_owned(),
            format: SourceFormat::Nickel,
        }
    }

    fn request(
        entry: SourceAddress,
        grants: Vec<SourceRootGrant>,
        limits: NickelEvaluationLimits,
    ) -> SourceClosureRequest {
        SourceClosureRequest {
            library_contract_major: NICKEL_LIBRARY_CONTRACT_MAJOR,
            corpus_major: R0_AUTHORING_CORPUS_MAJOR,
            entry,
            root_grants: grants,
            limits,
        }
    }

    fn one_root_grant(source_id: &SourceId) -> SourceRootGrant {
        SourceRootGrant {
            source_id: source_id.clone(),
            root_kind: AuthorizedRootKind::Test,
            source_hash: fixture_source_hash(source_id),
            entry: address(source_id, "main.ncl"),
            package_alias: None,
        }
    }

    #[test]
    fn package_alias_matches_nickel_identifier_grammar_and_keywords() {
        let accepted = PackageAlias::new(R0_LIBRARY_PACKAGE_ALIAS)
            .unwrap_or_else(|error| panic!("Nickel alias was rejected: {error}"));
        assert_eq!(accepted.as_str(), R0_LIBRARY_PACKAGE_ALIAS);
        assert_eq!(
            R0_LIBRARY_PACKAGE_ALIAS,
            format!("latticeaxiom_lib_v{NICKEL_LIBRARY_CONTRACT_MAJOR}")
        );
        for valid in ["_leading", "__Upper", "has-dash", "ends'", "or", "as"] {
            assert!(PackageAlias::new(valid).is_ok(), "rejected `{valid}`");
        }
        for invalid in ["", "_", "1start", "with space", "naïve", "let", "Dyn"] {
            assert!(PackageAlias::new(invalid).is_err(), "accepted `{invalid}`");
        }
    }

    fn resolve(
        snapshot: &FixtureSnapshot,
        scanner: &mut FixtureScanner,
        request: &SourceClosureRequest,
    ) -> Result<SourceClosureReceipt, SourceClosureError> {
        resolve_source_closure(snapshot, request, scanner)
    }

    #[test]
    fn entry_is_depth_zero_and_inclusive_limits_accept_boundary() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"1234");
        let mut scanner = FixtureScanner::default();
        let limits = NickelEvaluationLimits {
            imported_files: 1,
            source_bytes: 4,
            import_depth: 1,
            ..NickelEvaluationLimits::default()
        };
        let receipt = resolve(
            &snapshot,
            &mut scanner,
            &request(entry.clone(), vec![one_root_grant(&root)], limits),
        )
        .unwrap_or_else(|error| panic!("boundary closure was rejected: {error}"));

        assert_eq!(receipt.entry, entry);
        assert_eq!(receipt.imported_files, 1);
        assert_eq!(receipt.source_bytes, 4);
        assert_eq!(receipt.maximum_depth, 0);
        assert_eq!(receipt.sources.first().map(|source| source.depth), Some(0));
    }

    #[test]
    fn file_count_plus_one_is_rejected_and_counts_entry() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let child = address(&root, "child.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"a");
        snapshot.add(child, b"b");
        let mut scanner = FixtureScanner::default();
        scanner.add(entry.clone(), vec![relative("child.ncl")]);
        let limits = NickelEvaluationLimits {
            imported_files: 1,
            ..NickelEvaluationLimits::default()
        };

        let error = resolve(
            &snapshot,
            &mut scanner,
            &request(entry, vec![one_root_grant(&root)], limits),
        )
        .err()
        .unwrap_or_else(|| panic!("file-count limit unexpectedly passed"));
        assert_eq!(
            error,
            SourceClosureError::ImportedFilesLimitExceeded {
                limit: 1,
                actual: 2,
            }
        );
        assert_eq!(error.code(), "compose.import_files");
    }

    #[test]
    fn byte_count_plus_one_is_rejected_and_counts_entry() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"12345");
        let mut scanner = FixtureScanner::default();
        let limits = NickelEvaluationLimits {
            source_bytes: 4,
            ..NickelEvaluationLimits::default()
        };

        let error = resolve(
            &snapshot,
            &mut scanner,
            &request(entry, vec![one_root_grant(&root)], limits),
        )
        .err()
        .unwrap_or_else(|| panic!("byte-count limit unexpectedly passed"));
        assert_eq!(
            error,
            SourceClosureError::SourceBytesLimitExceeded {
                limit: 4,
                actual: 5,
            }
        );
        assert_eq!(error.code(), "compose.source_limit");
    }

    #[test]
    fn diamond_and_reimport_are_counted_once_in_deterministic_dfs_order() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let left = address(&root, "left.ncl");
        let right = address(&root, "right.ncl");
        let leaf = address(&root, "shared/leaf.ncl");
        let mut snapshot = FixtureSnapshot::default();
        for source in [&entry, &left, &right, &leaf] {
            snapshot.add(source.clone(), b"x");
        }
        let mut scanner = FixtureScanner::default();
        scanner.add(
            entry.clone(),
            vec![
                relative("right.ncl"),
                relative("left.ncl"),
                relative("left.ncl"),
            ],
        );
        scanner.add(left.clone(), vec![relative("shared/leaf.ncl")]);
        scanner.add(right.clone(), vec![relative("shared/leaf.ncl")]);

        let receipt = resolve(
            &snapshot,
            &mut scanner,
            &request(
                entry.clone(),
                vec![one_root_grant(&root)],
                NickelEvaluationLimits::default(),
            ),
        )
        .unwrap_or_else(|error| panic!("diamond closure was rejected: {error}"));
        let order = receipt
            .sources
            .iter()
            .map(|source| source.address.clone())
            .collect::<Vec<_>>();
        assert_eq!(order, vec![left, entry, right, leaf]);
        assert_eq!(receipt.imported_files, 4);
        assert_eq!(receipt.source_bytes, 4);
        assert_eq!(receipt.edges.len(), 4);
        receipt
            .verify_hash()
            .unwrap_or_else(|error| panic!("closure hash was invalid: {error}"));

        let mut reordered_scanner = FixtureScanner::default();
        reordered_scanner.add(
            receipt.entry.clone(),
            vec![relative("left.ncl"), relative("right.ncl")],
        );
        reordered_scanner.add(
            address(&root, "left.ncl"),
            vec![relative("shared/leaf.ncl")],
        );
        reordered_scanner.add(
            address(&root, "right.ncl"),
            vec![relative("shared/leaf.ncl")],
        );
        let reordered = resolve(
            &snapshot,
            &mut reordered_scanner,
            &request(
                receipt.entry.clone(),
                vec![one_root_grant(&root)],
                NickelEvaluationLimits::default(),
            ),
        )
        .unwrap_or_else(|error| panic!("reordered closure was rejected: {error}"));
        assert_eq!(receipt.closure_hash, reordered.closure_hash);

        let mut tampered = receipt.clone();
        tampered.source_bytes = tampered.source_bytes.saturating_add(1);
        assert!(matches!(
            tampered.verify_hash(),
            Err(SourceClosureError::InvalidClosureReceipt { .. })
        ));
    }

    #[test]
    fn longest_diamond_path_drives_depth_limit() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let a = address(&root, "a.ncl");
        let b = address(&root, "b.ncl");
        let c = address(&root, "c.ncl");
        let d = address(&root, "d.ncl");
        let mut snapshot = FixtureSnapshot::default();
        for source in [&entry, &a, &b, &c, &d] {
            snapshot.add(source.clone(), b"x");
        }
        let mut scanner = FixtureScanner::default();
        scanner.add(entry.clone(), vec![relative("a.ncl"), relative("b.ncl")]);
        scanner.add(a, vec![relative("c.ncl")]);
        scanner.add(b, vec![relative("d.ncl")]);
        scanner.add(d, vec![relative("c.ncl")]);
        let boundary_limits = NickelEvaluationLimits {
            import_depth: 3,
            ..NickelEvaluationLimits::default()
        };
        let boundary_receipt = resolve(
            &snapshot,
            &mut scanner,
            &request(entry.clone(), vec![one_root_grant(&root)], boundary_limits),
        )
        .unwrap_or_else(|error| panic!("depth boundary was rejected: {error}"));
        assert_eq!(boundary_receipt.maximum_depth, 3);

        let plus_one_limits = NickelEvaluationLimits {
            import_depth: 2,
            ..NickelEvaluationLimits::default()
        };

        let error = resolve(
            &snapshot,
            &mut scanner,
            &request(entry, vec![one_root_grant(&root)], plus_one_limits),
        )
        .err()
        .unwrap_or_else(|| panic!("longest import path unexpectedly passed"));
        assert_eq!(
            error,
            SourceClosureError::ImportDepthLimitExceeded {
                limit: 2,
                actual: 3,
            }
        );
        assert_eq!(error.code(), "compose.import_depth");
    }

    #[test]
    fn cycle_is_rejected_with_stable_closed_path() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let a = address(&root, "a.ncl");
        let b = address(&root, "b.ncl");
        let mut snapshot = FixtureSnapshot::default();
        for source in [&entry, &a, &b] {
            snapshot.add(source.clone(), b"x");
        }
        let mut scanner = FixtureScanner::default();
        scanner.add(entry.clone(), vec![relative("a.ncl")]);
        scanner.add(a.clone(), vec![relative("b.ncl")]);
        scanner.add(b.clone(), vec![relative("a.ncl")]);

        let error = resolve(
            &snapshot,
            &mut scanner,
            &request(
                entry,
                vec![one_root_grant(&root)],
                NickelEvaluationLimits::default(),
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("source cycle unexpectedly passed"));
        assert_eq!(
            error,
            SourceClosureError::ImportCycle {
                cycle: SourceCycle {
                    sources: vec![a.clone(), b, a],
                },
            }
        );
        assert_eq!(error.code(), "compose.import_cycle");
    }

    #[test]
    fn relative_paths_stay_within_root_and_package_aliases_are_explicit() {
        let root = source_id("latticeaxiom:source/root");
        let dependency = source_id("latticeaxiom:source/dependency");
        let entry = address(&root, "nested/main.ncl");
        let sibling = address(&root, "sibling.ncl");
        let dependency_entry = address(&dependency, "package/entry.ncl");
        let alias = PackageAlias::new("dep")
            .unwrap_or_else(|error| panic!("fixture alias is invalid: {error}"));
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"a");
        snapshot.add(sibling, b"b");
        snapshot.add(dependency_entry.clone(), b"c");
        let mut scanner = FixtureScanner::default();
        scanner.add(
            entry.clone(),
            vec![
                relative("../sibling.ncl"),
                StaticSourceImport::Package {
                    alias: alias.clone(),
                },
            ],
        );
        let grants = vec![
            SourceRootGrant {
                source_id: root.clone(),
                root_kind: AuthorizedRootKind::Test,
                source_hash: fixture_source_hash(&root),
                entry: address(&root, "main.ncl"),
                package_alias: None,
            },
            SourceRootGrant {
                source_id: dependency.clone(),
                root_kind: AuthorizedRootKind::Test,
                source_hash: fixture_source_hash(&dependency),
                entry: dependency_entry.clone(),
                package_alias: Some(alias),
            },
        ];

        let receipt = resolve(
            &snapshot,
            &mut scanner,
            &request(entry, grants, NickelEvaluationLimits::default()),
        )
        .unwrap_or_else(|error| panic!("authorized aliases were rejected: {error}"));
        assert_eq!(receipt.imported_files, 3);
        assert!(receipt.sources.iter().any(|source| {
            source.address.source_id() == &dependency
                && source.address.logical_path() == "package/entry.ncl"
        }));
    }

    #[test]
    fn escaping_relative_path_and_missing_alias_are_denied() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"x");
        let mut escaping = FixtureScanner::default();
        escaping.add(entry.clone(), vec![relative("../outside.ncl")]);
        let closure_request = request(
            entry.clone(),
            vec![one_root_grant(&root)],
            NickelEvaluationLimits::default(),
        );
        let path_error = resolve(&snapshot, &mut escaping, &closure_request)
            .err()
            .unwrap_or_else(|| panic!("escaping import unexpectedly passed"));
        assert!(matches!(
            path_error,
            SourceClosureError::InvalidLogicalPath { .. }
        ));

        let alias = PackageAlias::new("missing")
            .unwrap_or_else(|error| panic!("fixture alias is invalid: {error}"));
        let mut missing_alias = FixtureScanner::default();
        missing_alias.add(entry, vec![StaticSourceImport::Package { alias }]);
        let alias_error = resolve(&snapshot, &mut missing_alias, &closure_request)
            .err()
            .unwrap_or_else(|| panic!("missing alias unexpectedly passed"));
        assert!(matches!(
            alias_error,
            SourceClosureError::PackageAliasNotGranted { .. }
        ));
    }

    #[test]
    fn decomposed_unicode_import_is_rejected_without_normalization() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"x");
        let mut scanner = FixtureScanner::default();
        scanner.add(entry.clone(), vec![relative("cafe\u{301}.ncl")]);

        let error = resolve(
            &snapshot,
            &mut scanner,
            &request(
                entry,
                vec![one_root_grant(&root)],
                NickelEvaluationLimits::default(),
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("non-NFC import unexpectedly passed"));
        assert!(matches!(
            error,
            SourceClosureError::InvalidLogicalPath {
                reason: "logical paths must already be NFC",
                ..
            }
        ));
    }

    #[test]
    fn grant_entry_root_mismatch_and_duplicate_global_alias_are_rejected() {
        let root = source_id("latticeaxiom:source/root");
        let dependency = source_id("latticeaxiom:source/dependency");
        let entry = address(&root, "main.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"x");
        let mut scanner = FixtureScanner::default();

        let mismatch = resolve(
            &snapshot,
            &mut scanner,
            &request(
                entry.clone(),
                vec![SourceRootGrant {
                    source_id: root.clone(),
                    root_kind: AuthorizedRootKind::Test,
                    source_hash: fixture_source_hash(&root),
                    entry: address(&dependency, "entry.ncl"),
                    package_alias: None,
                }],
                NickelEvaluationLimits::default(),
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("grant entry root mismatch unexpectedly passed"));
        assert!(matches!(
            mismatch,
            SourceClosureError::GrantEntryRootMismatch { .. }
        ));

        let alias = PackageAlias::new("shared")
            .unwrap_or_else(|error| panic!("fixture alias is invalid: {error}"));
        let collision = resolve(
            &snapshot,
            &mut scanner,
            &request(
                entry.clone(),
                vec![
                    SourceRootGrant {
                        source_id: root.clone(),
                        root_kind: AuthorizedRootKind::Test,
                        source_hash: fixture_source_hash(&root),
                        entry,
                        package_alias: Some(alias.clone()),
                    },
                    SourceRootGrant {
                        source_id: dependency.clone(),
                        root_kind: AuthorizedRootKind::Test,
                        source_hash: fixture_source_hash(&dependency),
                        entry: address(&dependency, "custom-entry.ncl"),
                        package_alias: Some(alias.clone()),
                    },
                ],
                NickelEvaluationLimits::default(),
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("package alias collision unexpectedly passed"));
        assert_eq!(
            collision,
            SourceClosureError::DuplicatePackageAlias {
                alias,
                first_source_id: Box::new(dependency),
                second_source_id: Box::new(root),
            }
        );
    }

    #[test]
    fn equivalent_authored_spellings_and_alias_origin_collisions_are_denied() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let child = address(&root, "child.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"entry");
        snapshot.add(child.clone(), b"child");
        let mut scanner = FixtureScanner::default();
        scanner.add(
            entry.clone(),
            vec![relative("./child.ncl"), relative("child.ncl")],
        );
        let spelling_error = resolve(
            &snapshot,
            &mut scanner,
            &request(
                entry,
                vec![one_root_grant(&root)],
                NickelEvaluationLimits::default(),
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("distinct authored spellings unexpectedly deduplicated"));
        assert!(matches!(
            spelling_error,
            SourceClosureError::PackageAliasCollision { .. }
        ));
        assert_eq!(spelling_error.code(), "compose.import_denied");

        let nested_entry = address(&root, "nested/main.ncl");
        let shared_target = address(&root, "nested/entry.ncl");
        let alias = PackageAlias::new("self-package")
            .unwrap_or_else(|error| panic!("fixture alias is invalid: {error}"));
        snapshot.add(nested_entry.clone(), b"nested");
        snapshot.add(shared_target, b"target");
        let mut origin_scanner = FixtureScanner::default();
        origin_scanner.add(
            nested_entry.clone(),
            vec![
                relative("entry.ncl"),
                StaticSourceImport::Package {
                    alias: alias.clone(),
                },
            ],
        );
        let grant = SourceRootGrant {
            source_id: root.clone(),
            root_kind: AuthorizedRootKind::Test,
            source_hash: fixture_source_hash(&root),
            entry: address(&root, "nested/entry.ncl"),
            package_alias: Some(alias),
        };
        let origin_error = resolve(
            &snapshot,
            &mut origin_scanner,
            &request(nested_entry, vec![grant], NickelEvaluationLimits::default()),
        )
        .err()
        .unwrap_or_else(|| panic!("relative/package origin collision unexpectedly deduplicated"));
        assert!(matches!(
            origin_error,
            SourceClosureError::PackageAliasCollision { .. }
        ));
    }

    #[test]
    fn request_validation_is_order_independent_and_binds_snapshot_authority() {
        let first = source_id("latticeaxiom:source/a");
        let second = source_id("latticeaxiom:source/b");
        let alias = PackageAlias::new("shared")
            .unwrap_or_else(|error| panic!("fixture alias is invalid: {error}"));
        let grant = |source_id: &SourceId| SourceRootGrant {
            source_id: source_id.clone(),
            root_kind: AuthorizedRootKind::Test,
            source_hash: fixture_source_hash(source_id),
            entry: address(source_id, "main.ncl"),
            package_alias: Some(alias.clone()),
        };
        let forward = request(
            address(&first, "main.ncl"),
            vec![grant(&first), grant(&second)],
            NickelEvaluationLimits::default(),
        )
        .validate()
        .err()
        .unwrap_or_else(|| panic!("duplicate alias unexpectedly passed"));
        let reverse = request(
            address(&first, "main.ncl"),
            vec![grant(&second), grant(&first)],
            NickelEvaluationLimits::default(),
        )
        .validate()
        .err()
        .unwrap_or_else(|| panic!("reordered duplicate alias unexpectedly passed"));
        assert_eq!(forward, reverse);

        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"entry");
        let mut scanner = FixtureScanner::default();
        let mut wrong_kind = one_root_grant(&root);
        wrong_kind.root_kind = AuthorizedRootKind::Package;
        let kind_error = resolve(
            &snapshot,
            &mut scanner,
            &request(
                entry.clone(),
                vec![wrong_kind],
                NickelEvaluationLimits::default(),
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("snapshot root-kind substitution unexpectedly passed"));
        assert!(matches!(
            kind_error,
            SourceClosureError::SnapshotRootKindMismatch { .. }
        ));

        let mut wrong_hash = one_root_grant(&root);
        wrong_hash.source_hash = CanonicalHash::digest(b"wrong source table");
        let hash_error = resolve(
            &snapshot,
            &mut scanner,
            &request(entry, vec![wrong_hash], NickelEvaluationLimits::default()),
        )
        .err()
        .unwrap_or_else(|| panic!("snapshot source-hash substitution unexpectedly passed"));
        assert!(matches!(
            hash_error,
            SourceClosureError::SnapshotSourceHashMismatch { .. }
        ));
    }

    #[test]
    fn request_deserialization_rejects_zero_limits_and_reserved_alias_misuse() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let valid = request(
            entry,
            vec![one_root_grant(&root)],
            NickelEvaluationLimits::default(),
        );
        let mut zero_limit = serde_json::to_value(&valid)
            .unwrap_or_else(|error| panic!("request serialization failed: {error}"));
        zero_limit["limits"]["import_depth"] = serde_json::Value::from(0_u64);
        assert!(serde_json::from_value::<SourceClosureRequest>(zero_limit).is_err());

        let mut reserved = valid.clone();
        reserved.root_grants[0].package_alias = Some(
            PackageAlias::new(R0_LIBRARY_PACKAGE_ALIAS)
                .unwrap_or_else(|error| panic!("reserved alias is invalid: {error}")),
        );
        assert!(matches!(
            reserved.validate(),
            Err(SourceClosureError::ReservedLibraryAliasWrongKind { .. })
        ));
        let serialized = serde_json::to_value(&reserved)
            .unwrap_or_else(|error| panic!("request serialization failed: {error}"));
        assert!(serde_json::from_value::<SourceClosureRequest>(serialized).is_err());

        let old_library_alias = PackageAlias::new("latticeaxiom_lib_v1")
            .unwrap_or_else(|error| panic!("old library alias is invalid: {error}"));
        let mut old_alias_on_fixture = valid.root_grants[0].clone();
        old_alias_on_fixture.package_alias = Some(old_library_alias.clone());
        assert!(matches!(
            validate_grant(&old_alias_on_fixture),
            Err(SourceClosureError::ReservedLibraryAliasWrongKind { .. })
        ));
        let mut old_library = old_alias_on_fixture;
        old_library.root_kind = AuthorizedRootKind::Library;
        assert!(matches!(
            validate_grant(&old_library),
            Err(SourceClosureError::LibraryAliasMajorMismatch { .. })
        ));

        let mut wrong_library_major = valid.clone();
        wrong_library_major.library_contract_major =
            NICKEL_LIBRARY_CONTRACT_MAJOR.saturating_add(1);
        assert!(matches!(
            wrong_library_major.validate(),
            Err(SourceClosureError::UnsupportedLibraryContractMajor { .. })
        ));

        let mut wrong_corpus_major = valid;
        wrong_corpus_major.corpus_major = R0_AUTHORING_CORPUS_MAJOR.saturating_add(1);
        assert!(matches!(
            wrong_corpus_major.validate(),
            Err(SourceClosureError::UnsupportedCorpusMajor { .. })
        ));
    }

    #[test]
    fn receipt_deserialization_rejects_graph_meter_and_hash_tampering() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let child = address(&root, "child.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"entry");
        snapshot.add(child, b"child");
        let mut scanner = FixtureScanner::default();
        scanner.add(entry.clone(), vec![relative("child.ncl")]);
        let closure_request = request(
            entry,
            vec![one_root_grant(&root)],
            NickelEvaluationLimits::default(),
        );
        let receipt = resolve(&snapshot, &mut scanner, &closure_request)
            .unwrap_or_else(|error| panic!("fixture closure failed: {error}"));
        let encoded = serde_json::to_value(&receipt)
            .unwrap_or_else(|error| panic!("receipt serialization failed: {error}"));
        serde_json::from_value::<SourceClosureReceipt>(encoded)
            .unwrap_or_else(|error| panic!("valid receipt was rejected: {error}"));

        let mut reordered = receipt.clone();
        reordered.sources.reverse();
        let mut wrong_meter = receipt.clone();
        wrong_meter.source_bytes = wrong_meter.source_bytes.saturating_add(1);
        let mut wrong_depth = receipt.clone();
        wrong_depth.sources[0].depth = 0;
        let mut missing_edge = receipt.clone();
        missing_edge.edges.clear();
        let mut wrong_hash = receipt.clone();
        wrong_hash.closure_hash = CanonicalHash::digest(b"tampered receipt");
        for tampered in [
            reordered,
            wrong_meter,
            wrong_depth,
            missing_edge,
            wrong_hash,
        ] {
            let value = serde_json::to_value(tampered)
                .unwrap_or_else(|error| panic!("tampered receipt serialization failed: {error}"));
            assert!(
                serde_json::from_value::<SourceClosureReceipt>(value).is_err(),
                "tampered receipt unexpectedly deserialized"
            );
        }

        let mut unknown = serde_json::to_value(receipt)
            .unwrap_or_else(|error| panic!("receipt serialization failed: {error}"));
        unknown["unexpected"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<SourceClosureReceipt>(unknown).is_err());
    }

    #[test]
    fn controller_receipt_validation_rejects_coherent_omissions_and_inventions() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let child = address(&root, "child.ncl");
        let unused = address(&root, "unused.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), b"entry");
        snapshot.add(child.clone(), b"child");
        snapshot.add(unused.clone(), b"unused");
        let mut scanner = FixtureScanner::default();
        scanner.add(entry.clone(), vec![relative("child.ncl")]);
        let closure_request = request(
            entry.clone(),
            vec![one_root_grant(&root)],
            NickelEvaluationLimits::default(),
        );
        let receipt = resolve(&snapshot, &mut scanner, &closure_request)
            .unwrap_or_else(|error| panic!("fixture closure failed: {error}"));
        receipt
            .verify_against_expected(&receipt)
            .unwrap_or_else(|error| panic!("valid controller receipt was rejected: {error}"));

        let entry_member = receipt
            .sources
            .iter()
            .find(|member| member.address == entry)
            .cloned()
            .unwrap_or_else(|| panic!("fixture receipt omitted its entry"));
        let mut omitted = SourceClosureReceipt {
            entry: entry.clone(),
            source_bytes: entry_member.byte_length,
            sources: vec![entry_member],
            edges: Vec::new(),
            imported_files: 1,
            maximum_depth: 0,
            closure_hash: CanonicalHash::digest(b"placeholder"),
        };
        rehash_receipt(&mut omitted);
        omitted
            .verify()
            .unwrap_or_else(|error| panic!("coherent omission was not self-consistent: {error}"));
        let omission_error = omitted
            .verify_against_expected(&receipt)
            .err()
            .unwrap_or_else(|| panic!("coherent import omission matched controller receipt"));
        assert_eq!(omission_error, SourceClosureError::ClosureReceiptMismatch);
        assert_eq!(omission_error.code(), "compose.worker_protocol");

        let mut invented = receipt.clone();
        invented.sources.push(SourceClosureMember {
            address: unused.clone(),
            format: SourceFormat::Nickel,
            depth: 1,
            byte_length: 6,
            content_hash: CanonicalHash::digest(b"unused"),
        });
        invented
            .sources
            .sort_by(|left, right| left.address.cmp(&right.address));
        invented.edges.push(SourceClosureEdge {
            parent: entry,
            target: unused,
            format: SourceFormat::Nickel,
        });
        invented.edges.sort();
        invented.imported_files = invented.imported_files.saturating_add(1);
        invented.source_bytes = invented.source_bytes.saturating_add(6);
        rehash_receipt(&mut invented);
        invented
            .verify_against_snapshot(&snapshot)
            .unwrap_or_else(|error| panic!("invented receipt was not self-consistent: {error}"));
        assert_eq!(
            invented.verify_against_expected(&receipt),
            Err(SourceClosureError::ClosureReceiptMismatch)
        );
    }

    #[cfg(feature = "nickel-evaluator")]
    #[test]
    fn controller_boundary_rescans_static_imports_before_accepting_a_receipt() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let child = address(&root, "child.ncl");
        let mut snapshot = FixtureSnapshot::default();
        snapshot.add(entry.clone(), br#"import "child.ncl""#);
        snapshot.add(child, b"{}");
        let closure_request = request(
            entry,
            vec![one_root_grant(&root)],
            NickelEvaluationLimits::default(),
        );
        let expected = resolve_nickel_source_closure(&snapshot, &closure_request)
            .unwrap_or_else(|error| panic!("Nickel fixture closure failed: {error}"));
        expected
            .verify_against_request_and_snapshot(&closure_request, &snapshot)
            .unwrap_or_else(|error| panic!("controller rejected its own closure: {error}"));

        let entry_member = expected
            .sources
            .iter()
            .find(|member| member.address == expected.entry)
            .cloned()
            .unwrap_or_else(|| panic!("Nickel fixture receipt omitted its entry"));
        let mut omitted = SourceClosureReceipt {
            entry: expected.entry.clone(),
            sources: vec![entry_member.clone()],
            edges: Vec::new(),
            imported_files: 1,
            source_bytes: entry_member.byte_length,
            maximum_depth: 0,
            closure_hash: CanonicalHash::digest(b"placeholder"),
        };
        rehash_receipt(&mut omitted);
        assert_eq!(
            omitted.verify_against_request_and_snapshot(&closure_request, &snapshot),
            Err(SourceClosureError::ClosureReceiptMismatch)
        );
    }

    fn rehash_receipt(receipt: &mut SourceClosureReceipt) {
        receipt.closure_hash = closure_hash(
            &receipt.entry,
            &receipt.sources,
            &receipt.edges,
            receipt.imported_files,
            receipt.source_bytes,
            receipt.maximum_depth,
        )
        .unwrap_or_else(|error| panic!("fixture receipt hash failed: {error}"));
    }

    #[test]
    fn source_address_deserialization_denies_unknown_and_noncanonical_fields() {
        let unknown = serde_json::from_str::<SourceAddress>(
            r#"{"source_id":"latticeaxiom:source/root","logical_path":"main.ncl","extra":1}"#,
        );
        assert!(unknown.is_err());
        let noncanonical = serde_json::from_str::<SourceAddress>(
            r#"{"source_id":"latticeaxiom:source/root","logical_path":"a/../main.ncl"}"#,
        );
        assert!(noncanonical.is_err());
    }

    #[cfg(feature = "nickel-evaluator")]
    #[test]
    fn nickel_scanner_finds_imports_in_nested_and_dead_syntax() {
        let root = source_id("latticeaxiom:source/root");
        let entry = address(&root, "main.ncl");
        let source = br#"
            let hidden = fun _ => import "nested.ncl" in
            if false then import dep else import "data.json"
        "#;
        let mut scanner = NickelStaticImportScanner;
        let mut imports = scanner
            .scan_imports(&entry, source)
            .unwrap_or_else(|error| panic!("Nickel static scan failed: {error}"));
        imports.sort();
        assert_eq!(imports.len(), 3);
        assert!(imports.contains(&relative("nested.ncl")));
        assert!(imports.contains(&StaticSourceImport::Relative {
            path: "data.json".to_owned(),
            format: SourceFormat::Json,
        }));
        assert!(
            imports.contains(&StaticSourceImport::Package {
                alias: PackageAlias::new("dep")
                    .unwrap_or_else(|error| panic!("fixture alias is invalid: {error}")),
            })
        );
    }
}
