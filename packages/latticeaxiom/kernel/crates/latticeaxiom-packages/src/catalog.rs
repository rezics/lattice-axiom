//! Local catalog check, pack, publish-to-directory, and acquire.
//!
//! Path and workspace packages are acquisition locators only. Runnable identity
//! is the catalogued source and manifest digests plus CAS object locators.
//! The same `PackageName` and exact version cannot be published as two source
//! digests. Install and build scripts are never executed.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use latticeaxiom_compose::{
    AuthorizedRoot, AuthorizedRootKind, PACKAGE_SOURCE_MANIFEST_FILE_NAME, PackageSourceManifestV1,
    SourceScanLimits, SourceSnapshot, scan_included_source_snapshot,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageName, PackageVersion, SourceId, canonical_json_bytes,
    canonical_json_hash,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::cas::{CasObjectId, CasObjectKind, CasObjectStore, FilesystemCas};
use crate::error::CatalogError;

/// Schema version for [`LocalCatalogIndexV1`].
pub const LOCAL_CATALOG_SCHEMA_VERSION: u32 = 1;

/// Deterministic catalog index file written by publish-to-directory.
pub const LOCAL_CATALOG_INDEX_FILE_NAME: &str = "latticeaxiom-catalog.json";

/// Object-store directory beside the catalog index.
pub const LOCAL_CATALOG_CAS_DIRECTORY: &str = "cas";

const PACKAGE_SCAN_LIMITS: SourceScanLimits = SourceScanLimits {
    maximum_files: 4_096,
    maximum_bytes: 64 * 1024 * 1024,
};

/// Kind-qualified locator of one catalogued CAS object.
///
/// Locators are metadata. They do not grant activation authority and are not
/// acquisition paths.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CatalogObjectLocatorV1 {
    kind: CasObjectKind,
    digest: CanonicalHash,
}

impl CatalogObjectLocatorV1 {
    /// Addresses a CAS object by kind and payload digest.
    #[must_use]
    pub const fn new(kind: CasObjectKind, digest: CanonicalHash) -> Self {
        Self { kind, digest }
    }

    /// Copies kind and digest from a CAS object identity.
    #[must_use]
    pub const fn from_object_id(id: CasObjectId) -> Self {
        Self::new(id.kind(), id.digest())
    }

    /// Returns the object kind.
    #[must_use]
    pub const fn kind(self) -> CasObjectKind {
        self.kind
    }

    /// Returns the payload digest.
    #[must_use]
    pub const fn digest(self) -> CanonicalHash {
        self.digest
    }

    /// Returns the CAS identity used for `get` and `put`.
    #[must_use]
    pub const fn object_id(self) -> CasObjectId {
        CasObjectId::new(self.kind, self.digest)
    }
}

impl From<CasObjectId> for CatalogObjectLocatorV1 {
    fn from(id: CasObjectId) -> Self {
        Self::from_object_id(id)
    }
}

impl fmt::Display for CatalogObjectLocatorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.object_id())
    }
}

impl Serialize for CatalogObjectLocatorV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawCatalogObjectLocator {
            kind: self.kind.as_str().to_owned(),
            digest: self.digest,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CatalogObjectLocatorV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawCatalogObjectLocator::deserialize(deserializer)?;
        let kind = cas_kind_from_token(&raw.kind).map_err(de::Error::custom)?;
        Ok(Self::new(kind, raw.digest))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawCatalogObjectLocator {
    kind: String,
    digest: CanonicalHash,
}

/// One exact `PackageName` + version row in a local catalog.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntryV1 {
    /// Logical package identity.
    pub package: PackageName,
    /// Exact immutable version.
    pub version: PackageVersion,
    /// Canonical digest of the packed package source manifest.
    pub manifest_digest: CanonicalHash,
    /// Domain-separated source-table digest of the packed snapshot.
    pub source_digest: CanonicalHash,
    /// CAS locator of the package-manifest object.
    pub manifest_object: CatalogObjectLocatorV1,
    /// CAS locator of the source-tree object.
    pub source_object: CatalogObjectLocatorV1,
    /// Explicit yank metadata. Yanked versions remain immutable catalog rows.
    pub yanked: bool,
}

/// Deterministic local catalog index.
///
/// Entries are unique by package name and exact version and are stored in
/// canonical-byte identity order. Enumeration does not depend on publish order.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalCatalogIndexV1 {
    /// Catalog index schema version.
    pub schema_version: u32,
    /// Catalog rows in canonical-byte identity order.
    pub entries: Vec<CatalogEntryV1>,
}

impl LocalCatalogIndexV1 {
    /// Creates an empty schema-1 catalog.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema_version: LOCAL_CATALOG_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }

    /// Returns catalog rows in canonical-byte identity order.
    #[must_use]
    pub fn entries(&self) -> &[CatalogEntryV1] {
        &self.entries
    }

    /// Looks up one exact package version.
    #[must_use]
    pub fn get(&self, package: &PackageName, version: &PackageVersion) -> Option<&CatalogEntryV1> {
        self.entries
            .iter()
            .find(|entry| &entry.package == package && &entry.version == version)
    }

    /// Inserts a packed package row.
    ///
    /// Republishing the same source digest is idempotent and may set `yanked`.
    /// A different source digest for the same identity is rejected.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::DuplicatePublishedSource`] when the identity
    /// already exists with a different source digest, or
    /// [`CatalogError::InvalidCatalogIndex`] when locators disagree with an
    /// existing identical source.
    pub fn insert_packed(
        &mut self,
        packed: &PackedPackageV1,
        yanked: bool,
    ) -> Result<(), CatalogError> {
        self.validate_schema()?;
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|entry| entry.package == packed.package && entry.version == packed.version)
        {
            if existing.source_digest != packed.source_digest {
                return Err(CatalogError::DuplicatePublishedSource {
                    package: packed.package.clone(),
                    version: Box::new(packed.version.clone()),
                    existing: existing.source_digest,
                    published: packed.source_digest,
                });
            }
            if existing.manifest_digest != packed.manifest_digest
                || existing.manifest_object != packed.manifest_object
                || existing.source_object != packed.source_object
            {
                return Err(CatalogError::InvalidCatalogIndex {
                    reason: format!(
                        "package {} version {} is already published with matching source digest but conflicting locators",
                        packed.package, packed.version
                    ),
                });
            }
            existing.yanked |= yanked;
            return Ok(());
        }

        self.entries.push(CatalogEntryV1 {
            package: packed.package.clone(),
            version: packed.version.clone(),
            manifest_digest: packed.manifest_digest,
            source_digest: packed.source_digest,
            manifest_object: packed.manifest_object,
            source_object: packed.source_object,
            yanked,
        });
        sort_catalog_entries(&mut self.entries)?;
        Ok(())
    }

    /// Encodes this index using canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when the DTO cannot be encoded.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }

    /// Validates schema, uniqueness, and canonical-byte order.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for an unsupported schema or malformed index.
    pub fn validate(&self) -> Result<(), CatalogError> {
        self.validate_schema()?;
        let mut seen = BTreeSet::new();
        let mut previous_key: Option<Vec<u8>> = None;
        for entry in &self.entries {
            if entry.manifest_object.kind() != CasObjectKind::PackageManifest {
                return Err(unexpected_kind(
                    entry.manifest_object,
                    CasObjectKind::PackageManifest,
                ));
            }
            if entry.source_object.kind() != CasObjectKind::SourceTree {
                return Err(unexpected_kind(
                    entry.source_object,
                    CasObjectKind::SourceTree,
                ));
            }
            let key = identity_sort_key(&CatalogIdentityV1 {
                package: entry.package.clone(),
                version: entry.version.clone(),
            })?;
            if !seen.insert(key.clone()) {
                return Err(CatalogError::InvalidCatalogIndex {
                    reason: format!(
                        "duplicate catalog identity {} {}",
                        entry.package, entry.version
                    ),
                });
            }
            if let Some(previous) = &previous_key
                && key <= *previous
            {
                return Err(CatalogError::InvalidCatalogIndex {
                    reason: "catalog entries are not in canonical-byte identity order".to_owned(),
                });
            }
            previous_key = Some(key);
        }
        Ok(())
    }

    /// Decodes canonical JSON and validates the resulting index.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] for malformed JSON, a non-canonical encoding,
    /// an unsupported schema, or a structural invariant failure.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, CatalogError> {
        let index = serde_json::from_slice::<Self>(bytes).map_err(|error| {
            CatalogError::InvalidCatalogIndex {
                reason: error.to_string(),
            }
        })?;
        if index.canonical_bytes()?.as_slice() != bytes {
            return Err(CatalogError::NonCanonicalIndex);
        }
        index.validate()?;
        Ok(index)
    }

    fn validate_schema(&self) -> Result<(), CatalogError> {
        if self.schema_version == LOCAL_CATALOG_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(CatalogError::UnsupportedCatalogSchema {
                found: self.schema_version,
                supported: LOCAL_CATALOG_SCHEMA_VERSION,
            })
        }
    }
}

/// Validated workspace or path package ready to pack.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckedPackageV1 {
    /// Graph-affecting static package source manifest.
    pub manifest: PackageSourceManifestV1,
    /// Immutable included source-tree snapshot.
    pub snapshot: SourceSnapshot,
}

/// Source-tree and manifest objects stored in CAS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedPackageV1 {
    /// Logical package identity.
    pub package: PackageName,
    /// Exact package version.
    pub version: PackageVersion,
    /// Canonical digest of the packed manifest object.
    pub manifest_digest: CanonicalHash,
    /// Source-table digest recorded as catalog identity.
    pub source_digest: CanonicalHash,
    /// CAS locator of the package-manifest object.
    pub manifest_object: CatalogObjectLocatorV1,
    /// CAS locator of the source-tree object.
    pub source_object: CatalogObjectLocatorV1,
}

/// Package reconstructed from catalog metadata and CAS only.
#[derive(Clone, Debug, PartialEq)]
pub struct AcquiredPackageV1 {
    /// Catalog row that selected this exact version.
    pub entry: CatalogEntryV1,
    /// Validated package source manifest recovered from CAS.
    pub manifest: PackageSourceManifestV1,
    /// Immutable source snapshot recovered from CAS.
    pub snapshot: SourceSnapshot,
}

/// Validates a workspace or path package against [`PackageSourceManifestV1`].
///
/// `root` is an acquisition locator. The returned snapshot is the included
/// source tree, not a live directory handle. Manifest scripts, imports,
/// functions, environment expansion, and network locators are rejected by
/// the static manifest parser and are never executed.
///
/// # Errors
///
/// Returns [`CatalogError`] when the root is unreadable, the manifest is
/// invalid, an included path or Nickel entrypoint is missing, or the source
/// scan fails closed.
pub fn check_path_package(root: impl AsRef<Path>) -> Result<CheckedPackageV1, CatalogError> {
    check_path_package_with_limits(root.as_ref(), PACKAGE_SCAN_LIMITS)
}

fn check_path_package_with_limits(
    root: &Path,
    limits: SourceScanLimits,
) -> Result<CheckedPackageV1, CatalogError> {
    let root = acquisition_root(root)?;
    let manifest_path = root.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let manifest_text =
        fs::read_to_string(&manifest_path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => CatalogError::MissingManifest {
                path: manifest_path.clone(),
            },
            _ => CatalogError::io(&manifest_path, source),
        })?;
    let manifest = PackageSourceManifestV1::from_toml_str(&manifest_text)?;
    verify_included_paths_exist(&root, &manifest)?;

    let source_id = catalog_source_id(&manifest.name, &manifest.version)?;
    let authorized = AuthorizedRoot::new(source_id, AuthorizedRootKind::Package, root)?;
    let snapshot = scan_included_source_snapshot(&authorized, limits, &manifest.source_inclusion)?;
    if snapshot.files().is_empty() {
        return Err(CatalogError::EmptyIncludedSnapshot {
            package: manifest.name.clone(),
        });
    }
    verify_entrypoints_present(&manifest, &snapshot)?;

    Ok(CheckedPackageV1 { manifest, snapshot })
}

/// Snapshots included source bytes and the package manifest into CAS.
///
/// Path is not an identity: only the already checked snapshot and canonical
/// manifest bytes are stored. Repeating the same bytes is idempotent.
///
/// # Errors
///
/// Returns [`CatalogError`] when canonical encoding fails or the CAS store
/// rejects the put.
pub fn pack_package<S: CasObjectStore>(
    checked: &CheckedPackageV1,
    store: &mut S,
) -> Result<PackedPackageV1, CatalogError> {
    let manifest_bytes = canonical_json_bytes(&checked.manifest)?;
    let manifest_digest = CanonicalHash::digest(&manifest_bytes);
    let manifest_id = store.put(CasObjectKind::PackageManifest, &manifest_bytes)?;

    let source_bytes = canonical_json_bytes(&checked.snapshot)?;
    let source_id = store.put(CasObjectKind::SourceTree, &source_bytes)?;

    Ok(PackedPackageV1 {
        package: checked.manifest.name.clone(),
        version: checked.manifest.version.clone(),
        manifest_digest,
        source_digest: checked.snapshot.source_hash(),
        manifest_object: CatalogObjectLocatorV1::from_object_id(manifest_id),
        source_object: CatalogObjectLocatorV1::from_object_id(source_id),
    })
}

/// Writes packed packages into a deterministic local catalog directory.
///
/// The catalog stores metadata at [`LOCAL_CATALOG_INDEX_FILE_NAME`] and CAS
/// objects under [`LOCAL_CATALOG_CAS_DIRECTORY`]. Existing exact versions may
/// be republished only with the same source digest. Objects are copied from
/// `store` into the catalog CAS so acquire can run from that directory alone.
///
/// # Errors
///
/// Returns [`CatalogError`] when the directory cannot be written, an existing
/// index is invalid, uniqueness is violated, or a required CAS object is
/// missing from `store`.
pub fn publish_to_directory(
    catalog_root: impl AsRef<Path>,
    store: &impl CasObjectStore,
    packages: impl IntoIterator<Item = PackedPackageV1>,
) -> Result<LocalCatalogIndexV1, CatalogError> {
    let catalog_root = catalog_root.as_ref();
    fs::create_dir_all(catalog_root).map_err(|source| CatalogError::io(catalog_root, source))?;
    let cas_root = catalog_root.join(LOCAL_CATALOG_CAS_DIRECTORY);
    let mut catalog_store = FilesystemCas::open(&cas_root)?;
    let mut index = load_or_empty_index(catalog_root)?;

    for packed in packages {
        copy_packed_objects(store, &mut catalog_store, &packed)?;
        index.insert_packed(&packed, false)?;
    }
    index.validate()?;
    write_index_file(catalog_root, &index)?;
    Ok(index)
}

/// Reconstructs one exact package version from catalog metadata and CAS.
///
/// The original acquisition path is never consulted. Missing or mismatched
/// objects fail closed.
///
/// # Errors
///
/// Returns [`CatalogError`] when the identity is absent, a CAS object is
/// missing, or stored bytes do not match the catalogued digests and identity.
pub fn acquire_package(
    index: &LocalCatalogIndexV1,
    store: &impl CasObjectStore,
    package: &PackageName,
    version: &PackageVersion,
) -> Result<AcquiredPackageV1, CatalogError> {
    index.validate()?;
    let entry =
        index
            .get(package, version)
            .cloned()
            .ok_or_else(|| CatalogError::MissingCatalogEntry {
                package: package.clone(),
                version: version.clone(),
            })?;
    acquire_entry(&entry, store)
}

/// Opens a published catalog directory and acquires one exact package version.
///
/// # Errors
///
/// Returns [`CatalogError`] when the index or object store is missing, the
/// index is invalid, or acquire fails closed on a missing CAS object.
pub fn acquire_from_directory(
    catalog_root: impl AsRef<Path>,
    package: &PackageName,
    version: &PackageVersion,
) -> Result<AcquiredPackageV1, CatalogError> {
    let catalog_root = catalog_root.as_ref();
    let index = load_index(catalog_root)?;
    let cas_root = catalog_root.join(LOCAL_CATALOG_CAS_DIRECTORY);
    if !cas_directory_exists(&cas_root)? {
        return Err(CatalogError::MissingObjectStore { path: cas_root });
    }
    let store = FilesystemCas::open(&cas_root)?;
    acquire_package(&index, &store, package, version)
}

fn acquire_entry(
    entry: &CatalogEntryV1,
    store: &impl CasObjectStore,
) -> Result<AcquiredPackageV1, CatalogError> {
    if entry.manifest_object.kind() != CasObjectKind::PackageManifest {
        return Err(unexpected_kind(
            entry.manifest_object,
            CasObjectKind::PackageManifest,
        ));
    }
    if entry.source_object.kind() != CasObjectKind::SourceTree {
        return Err(unexpected_kind(
            entry.source_object,
            CasObjectKind::SourceTree,
        ));
    }

    let manifest_bytes = store.get(&entry.manifest_object.object_id())?;
    let manifest: PackageSourceManifestV1 =
        serde_json::from_slice(&manifest_bytes).map_err(|error| {
            CatalogError::InvalidObjectPayload {
                locator: entry.manifest_object.to_string(),
                reason: error.to_string(),
            }
        })?;
    manifest.validate()?;
    let manifest_digest = manifest.canonical_hash()?;
    if manifest_digest != entry.manifest_digest {
        return Err(CatalogError::ManifestDigestMismatch {
            package: entry.package.clone(),
            version: Box::new(entry.version.clone()),
            expected: entry.manifest_digest,
            actual: manifest_digest,
        });
    }
    if manifest.name != entry.package || manifest.version != entry.version {
        return Err(CatalogError::PackageIdentityMismatch {
            catalog_package: entry.package.clone(),
            catalog_version: Box::new(entry.version.clone()),
            object_package: manifest.name.clone(),
            object_version: Box::new(manifest.version.clone()),
        });
    }

    let source_bytes = store.get(&entry.source_object.object_id())?;
    let snapshot: SourceSnapshot = serde_json::from_slice(&source_bytes).map_err(|error| {
        CatalogError::InvalidObjectPayload {
            locator: entry.source_object.to_string(),
            reason: error.to_string(),
        }
    })?;
    snapshot
        .verify()
        .map_err(|error| CatalogError::InvalidObjectPayload {
            locator: entry.source_object.to_string(),
            reason: error.to_string(),
        })?;
    if snapshot.source_hash() != entry.source_digest {
        return Err(CatalogError::SourceDigestMismatch {
            package: entry.package.clone(),
            version: Box::new(entry.version.clone()),
            expected: entry.source_digest,
            actual: snapshot.source_hash(),
        });
    }

    Ok(AcquiredPackageV1 {
        entry: entry.clone(),
        manifest,
        snapshot,
    })
}

fn copy_packed_objects(
    source: &impl CasObjectStore,
    destination: &mut impl CasObjectStore,
    packed: &PackedPackageV1,
) -> Result<(), CatalogError> {
    for locator in [packed.manifest_object, packed.source_object] {
        let bytes = source.get(&locator.object_id())?;
        destination.put(locator.kind(), &bytes)?;
    }
    Ok(())
}

fn load_or_empty_index(catalog_root: &Path) -> Result<LocalCatalogIndexV1, CatalogError> {
    let path = catalog_root.join(LOCAL_CATALOG_INDEX_FILE_NAME);
    match fs::read(&path) {
        Ok(bytes) => LocalCatalogIndexV1::from_canonical_bytes(&bytes),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(LocalCatalogIndexV1::new()),
        Err(source) => Err(CatalogError::io(path, source)),
    }
}

fn load_index(catalog_root: &Path) -> Result<LocalCatalogIndexV1, CatalogError> {
    let path = catalog_root.join(LOCAL_CATALOG_INDEX_FILE_NAME);
    match fs::read(&path) {
        Ok(bytes) => LocalCatalogIndexV1::from_canonical_bytes(&bytes),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Err(CatalogError::MissingCatalogIndex { path })
        }
        Err(source) => Err(CatalogError::io(path, source)),
    }
}

fn write_index_file(catalog_root: &Path, index: &LocalCatalogIndexV1) -> Result<(), CatalogError> {
    let dest = catalog_root.join(LOCAL_CATALOG_INDEX_FILE_NAME);
    let bytes = index.canonical_bytes()?;
    let temp = dest.with_extension("json.tmp");
    if let Err(error) = write_temporary(&temp, &bytes) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    match fs::rename(&temp, &dest) {
        Ok(()) => Ok(()),
        Err(source) => {
            let _ = fs::remove_file(&temp);
            Err(CatalogError::io(dest, source))
        }
    }
}

fn write_temporary(path: &Path, bytes: &[u8]) -> Result<(), CatalogError> {
    let mut file = File::create(path).map_err(|source| CatalogError::io(path, source))?;
    file.write_all(bytes)
        .map_err(|source| CatalogError::io(path, source))?;
    file.sync_all()
        .map_err(|source| CatalogError::io(path, source))?;
    Ok(())
}

fn cas_directory_exists(path: &Path) -> Result<bool, CatalogError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(CatalogError::io(path, source)),
    }
}

fn acquisition_root(root: &Path) -> Result<PathBuf, CatalogError> {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::path::absolute(root).map_err(|source| CatalogError::io(root, source))?
    };
    let metadata =
        fs::symlink_metadata(&absolute).map_err(|source| CatalogError::io(&absolute, source))?;
    if !metadata.is_dir() {
        return Err(CatalogError::RootNotDirectory { path: absolute });
    }
    Ok(absolute)
}

fn verify_included_paths_exist(
    root: &Path,
    manifest: &PackageSourceManifestV1,
) -> Result<(), CatalogError> {
    for path in &manifest.source_inclusion.include {
        let physical = join_logical(root, path.as_str());
        match fs::symlink_metadata(&physical) {
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Err(CatalogError::MissingIncludedPath {
                    package: manifest.name.clone(),
                    path: path.clone(),
                });
            }
            Err(source) => return Err(CatalogError::io(physical, source)),
        }
    }
    Ok(())
}

fn verify_entrypoints_present(
    manifest: &PackageSourceManifestV1,
    snapshot: &SourceSnapshot,
) -> Result<(), CatalogError> {
    for path in manifest.nickel_public_entrypoints.values() {
        if snapshot.files().contains_key(path.as_str()) {
            continue;
        }
        return Err(CatalogError::MissingNickelEntrypoint {
            package: manifest.name.clone(),
            path: path.clone(),
        });
    }
    Ok(())
}

fn join_logical(root: &Path, logical_path: &str) -> PathBuf {
    let mut dest = root.to_path_buf();
    for segment in logical_path.split('/') {
        dest.push(segment);
    }
    dest
}

fn catalog_source_id(
    package: &PackageName,
    version: &PackageVersion,
) -> Result<SourceId, CatalogError> {
    let digest = canonical_json_hash(&CatalogIdentityV1 {
        package: package.clone(),
        version: version.clone(),
    })?;
    let value = format!("latticeaxiom:source/local-catalog/{digest}");
    value
        .parse()
        .map_err(|source| CatalogError::InvalidSourceId { value, source })
}

fn sort_catalog_entries(entries: &mut [CatalogEntryV1]) -> Result<(), CatalogError> {
    let mut keyed = entries
        .iter()
        .map(|entry| {
            identity_sort_key(&CatalogIdentityV1 {
                package: entry.package.clone(),
                version: entry.version.clone(),
            })
            .map(|key| (key, entry.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    for (slot, (_, entry)) in entries.iter_mut().zip(keyed) {
        *slot = entry;
    }
    Ok(())
}

fn identity_sort_key(identity: &CatalogIdentityV1) -> Result<Vec<u8>, CatalogError> {
    Ok(canonical_json_bytes(identity)?)
}

fn unexpected_kind(locator: CatalogObjectLocatorV1, expected: CasObjectKind) -> CatalogError {
    CatalogError::UnexpectedObjectKind {
        locator: locator.to_string(),
        expected: expected.as_str().to_owned(),
        actual: locator.kind().as_str().to_owned(),
    }
}

fn cas_kind_from_token(kind: &str) -> Result<CasObjectKind, CatalogError> {
    match kind {
        "source-tree" => Ok(CasObjectKind::SourceTree),
        "package-manifest" => Ok(CasObjectKind::PackageManifest),
        "package-archive" => Ok(CasObjectKind::PackageArchive),
        "realized-artifact" => Ok(CasObjectKind::RealizedArtifact),
        _ => Err(CatalogError::UnknownObjectKind {
            kind: kind.to_owned(),
        }),
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CatalogIdentityV1 {
    package: PackageName,
    version: PackageVersion,
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use latticeaxiom_compose::ForbiddenManifestConstruct;

    use super::*;
    use crate::cas::MemoryCas;
    use crate::error::CasError;

    #[derive(Debug)]
    struct TestDirectory(tempfile::TempDir);

    impl TestDirectory {
        fn create() -> Self {
            Self(
                tempfile::Builder::new()
                    .prefix("latticeaxiom-packages-catalog-")
                    .tempdir()
                    .unwrap_or_else(|error| panic!("test directory was not created: {error}")),
            )
        }

        fn child(&self, name: &str) -> PathBuf {
            self.0.path().join(name)
        }
    }

    fn succeeded<T, E>(result: Result<T, E>) -> T
    where
        E: Debug,
    {
        result.unwrap_or_else(|error| panic!("operation unexpectedly failed: {error:?}"))
    }

    fn write_package(root: &Path, name: &str, version: &str, nickel: &str) {
        succeeded(fs::create_dir_all(root));
        let manifest = format!(
            r#"
schema_version = 1
name = "{name}"
version = "{version}"
domains = ["authoritative"]
trust = "data-only"

[realizations.data]
id = "data"
kind = "data"
domains = ["authoritative"]
artifact = {{ kind = "data-root", path = "package.ncl" }}
trust = "data-only"

[nickel_public_entrypoints]
default = "package.ncl"

[source_inclusion]
include = ["package.ncl"]
"#
        );
        succeeded(fs::write(
            root.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME),
            manifest,
        ));
        succeeded(fs::write(root.join("package.ncl"), nickel));
    }

    fn check_pack(root: &Path, store: &mut impl CasObjectStore) -> PackedPackageV1 {
        let checked = succeeded(check_path_package(root));
        succeeded(pack_package(&checked, store))
    }

    #[test]
    fn offline_check_pack_publish_acquire_from_clean_temp_dir() {
        let workspace = TestDirectory::create();
        let package_root = workspace.child("package");
        write_package(&package_root, "terrain", "1.0.0", "{ nickel = true }\n");
        succeeded(fs::write(package_root.join("install.sh"), "exit 1\n"));

        let mut memory = MemoryCas::new();
        let packed = check_pack(&package_root, &mut memory);
        assert_eq!(packed.package.as_str(), "terrain");
        assert_eq!(packed.version.to_string(), "1.0.0");
        assert!(!workspace.child("package").join("ran.txt").exists());

        let catalog_root = workspace.child("catalog");
        let index = succeeded(publish_to_directory(
            &catalog_root,
            &memory,
            [packed.clone()],
        ));
        assert_eq!(index.entries().len(), 1);
        assert!(!index.entries()[0].yanked);

        succeeded(fs::remove_dir_all(&package_root));
        assert!(!package_root.exists());

        let acquired = succeeded(acquire_from_directory(
            &catalog_root,
            &packed.package,
            &packed.version,
        ));
        assert_eq!(acquired.manifest.name, packed.package);
        assert_eq!(acquired.manifest.version, packed.version);
        assert_eq!(acquired.snapshot.source_hash(), packed.source_digest);
        assert_eq!(
            acquired
                .snapshot
                .resolve_path("package.ncl")
                .map(latticeaxiom_compose::SourceFileSnapshot::bytes),
            Ok(b"{ nickel = true }\n".as_slice())
        );
        assert!(!acquired.snapshot.files().contains_key("install.sh"));
        assert!(!package_root.exists());
    }

    #[test]
    fn check_prunes_excluded_target_before_source_budgets() {
        let workspace = TestDirectory::create();
        let package_root = workspace.child("package");
        write_package(&package_root, "terrain", "1.0.0", "{}\n");
        let target = package_root.join("target").join("cache");
        succeeded(fs::create_dir_all(&target));
        succeeded(fs::write(target.join("first.bin"), b"excluded-first"));
        succeeded(fs::write(target.join("second.bin"), b"excluded-second"));

        let checked = succeeded(check_path_package_with_limits(
            &package_root,
            SourceScanLimits {
                maximum_files: 1,
                maximum_bytes: 3,
            },
        ));
        assert_eq!(checked.snapshot.total_source_bytes(), 3);
        assert_eq!(
            checked
                .snapshot
                .files()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["package.ncl"]
        );
    }

    #[test]
    fn same_name_and_version_cannot_publish_two_source_digests() {
        let workspace = TestDirectory::create();
        let first_root = workspace.child("first");
        let second_root = workspace.child("second");
        write_package(&first_root, "terrain", "1.0.0", "let a = 1 in a\n");
        write_package(&second_root, "terrain", "1.0.0", "let a = 2 in a\n");

        let mut store = MemoryCas::new();
        let first = check_pack(&first_root, &mut store);
        let second = check_pack(&second_root, &mut store);
        assert_ne!(first.source_digest, second.source_digest);

        let mut index = LocalCatalogIndexV1::new();
        succeeded(index.insert_packed(&first, false));
        succeeded(index.insert_packed(&first, true));
        assert!(
            index
                .get(&first.package, &first.version)
                .is_some_and(|entry| entry.yanked),
            "yank metadata must be recorded on an otherwise identical republish"
        );

        match index.insert_packed(&second, false) {
            Err(CatalogError::DuplicatePublishedSource {
                package,
                version,
                existing,
                published,
            }) => {
                assert_eq!(package.as_str(), "terrain");
                assert_eq!(version.to_string(), "1.0.0");
                assert_eq!(existing, first.source_digest);
                assert_eq!(published, second.source_digest);
            }
            other => panic!("expected duplicate published source, got {other:?}"),
        }

        let catalog_root = workspace.child("catalog");
        succeeded(publish_to_directory(&catalog_root, &store, [first]));
        match publish_to_directory(&catalog_root, &store, [second]) {
            Err(CatalogError::DuplicatePublishedSource { .. }) => {}
            other => panic!("directory publish must reject a second source digest, got {other:?}"),
        }
    }

    #[test]
    fn enumeration_is_canonical_byte_sorted_not_publish_or_semver_order() {
        let workspace = TestDirectory::create();
        let mut store = MemoryCas::new();
        let zeta = check_pack(
            &{
                let root = workspace.child("zeta");
                write_package(&root, "zeta", "1.0.0", "zeta\n");
                root
            },
            &mut store,
        );
        let alpha_two = check_pack(
            &{
                let root = workspace.child("alpha-two");
                write_package(&root, "alpha", "2.0.0", "alpha-two\n");
                root
            },
            &mut store,
        );
        let alpha_ten = check_pack(
            &{
                let root = workspace.child("alpha-ten");
                write_package(&root, "alpha", "10.0.0", "alpha-ten\n");
                root
            },
            &mut store,
        );

        let catalog_root = workspace.child("catalog");
        let index = succeeded(publish_to_directory(
            &catalog_root,
            &store,
            [zeta, alpha_two, alpha_ten],
        ));
        let names: Vec<(String, String)> = index
            .entries()
            .iter()
            .map(|entry| (entry.package.to_string(), entry.version.to_string()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("alpha".to_owned(), "10.0.0".to_owned()),
                ("alpha".to_owned(), "2.0.0".to_owned()),
                ("zeta".to_owned(), "1.0.0".to_owned()),
            ]
        );

        let mut keys = Vec::new();
        for entry in index.entries() {
            keys.push(succeeded(canonical_json_bytes(&CatalogIdentityV1 {
                package: entry.package.clone(),
                version: entry.version.clone(),
            })));
        }
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn missing_object_fails_closed_without_reading_acquisition_path() {
        let workspace = TestDirectory::create();
        let package_root = workspace.child("package");
        write_package(&package_root, "terrain", "1.0.0", "source-v1\n");

        let catalog_root = workspace.child("catalog");
        let mut store = succeeded(FilesystemCas::open(
            catalog_root.join(LOCAL_CATALOG_CAS_DIRECTORY),
        ));
        let packed = check_pack(&package_root, &mut store);
        succeeded(publish_to_directory(
            &catalog_root,
            &store,
            [packed.clone()],
        ));

        let source_path = catalog_root
            .join(LOCAL_CATALOG_CAS_DIRECTORY)
            .join(CasObjectKind::SourceTree.as_str())
            .join(packed.source_object.digest().to_string());
        succeeded(fs::remove_file(&source_path));
        succeeded(fs::write(
            package_root.join("package.ncl"),
            "mutated-source\n",
        ));

        match acquire_from_directory(&catalog_root, &packed.package, &packed.version) {
            Err(CatalogError::Cas(CasError::MissingObject { id })) => {
                assert_eq!(id, packed.source_object.object_id());
            }
            other => panic!("missing CAS object must fail closed, got {other:?}"),
        }
        assert_eq!(
            succeeded(fs::read_to_string(package_root.join("package.ncl"))),
            "mutated-source\n"
        );
    }

    #[test]
    fn check_rejects_manifest_scripts_and_never_executes_tree_scripts() {
        let workspace = TestDirectory::create();
        let package_root = workspace.child("package");
        write_package(&package_root, "terrain", "1.0.0", "{}\n");
        let manifest_path = package_root.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
        let original = succeeded(fs::read_to_string(&manifest_path));
        succeeded(fs::write(
            &manifest_path,
            format!("{original}\n[scripts]\nbuild = \"exit 1\"\n"),
        ));
        match check_path_package(&package_root) {
            Err(CatalogError::InvalidManifest { source }) => {
                assert!(
                    matches!(
                        source,
                        latticeaxiom_compose::BootstrapManifestError::ForbiddenConstruct {
                            construct: ForbiddenManifestConstruct::Script,
                        }
                    ),
                    "unexpected manifest error: {source}"
                );
            }
            other => panic!("scripts must be rejected, got {other:?}"),
        }

        succeeded(fs::write(&manifest_path, original));
        succeeded(fs::write(
            package_root.join("build.sh"),
            "echo RAN > ran.txt\n",
        ));
        let checked = succeeded(check_path_package(&package_root));
        assert!(!package_root.join("ran.txt").exists());
        assert!(!checked.snapshot.files().contains_key("build.sh"));

        let mut store = MemoryCas::new();
        let packed = succeeded(pack_package(&checked, &mut store));
        let mut index = LocalCatalogIndexV1::new();
        succeeded(index.insert_packed(&packed, false));
        let acquired = succeeded(acquire_package(
            &index,
            &store,
            &packed.package,
            &packed.version,
        ));
        assert!(!package_root.join("ran.txt").exists());
        assert!(!acquired.snapshot.files().contains_key("build.sh"));
        assert_eq!(acquired.snapshot.source_hash(), packed.source_digest);
    }
}
