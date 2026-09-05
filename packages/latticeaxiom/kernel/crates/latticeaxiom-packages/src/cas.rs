//! Immutable content-addressed object store for local package acquisition.
//!
//! Path sources are acquisition locators only. Runnable identity is the
//! kind-qualified payload digest recorded by this store. Missing objects fail
//! closed and are never reconstructed from an original path.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use latticeaxiom_core::CanonicalHash;

use crate::error::CasError;

/// Kind of an immutable CAS object.
///
/// Kind is part of the object address, so identical payloads stored under
/// different kinds remain distinct.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CasObjectKind {
    /// Complete immutable source-tree snapshot bytes.
    SourceTree,
    /// Package source or catalog manifest bytes.
    PackageManifest,
    /// Packed package archive bytes.
    PackageArchive,
    /// Realized artifact bytes.
    RealizedArtifact,
}

impl CasObjectKind {
    /// Returns the stable directory and address token for this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceTree => "source-tree",
            Self::PackageManifest => "package-manifest",
            Self::PackageArchive => "package-archive",
            Self::RealizedArtifact => "realized-artifact",
        }
    }
}

impl fmt::Display for CasObjectKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Kind-qualified content address of one CAS object.
///
/// The digest covers payload bytes only. Kind remains in the address so two
/// objects with the same payload stay distinct.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CasObjectId {
    kind: CasObjectKind,
    digest: CanonicalHash,
}

impl CasObjectId {
    /// Addresses an object of `kind` whose payload hashes to `digest`.
    #[must_use]
    pub const fn new(kind: CasObjectKind, digest: CanonicalHash) -> Self {
        Self { kind, digest }
    }

    /// Addresses `kind` by hashing the exact payload bytes.
    #[must_use]
    pub fn from_payload(kind: CasObjectKind, bytes: &[u8]) -> Self {
        Self::new(kind, CanonicalHash::digest(bytes))
    }

    /// Returns the object kind that qualifies this address.
    #[must_use]
    pub const fn kind(self) -> CasObjectKind {
        self.kind
    }

    /// Returns the payload digest.
    #[must_use]
    pub const fn digest(self) -> CanonicalHash {
        self.digest
    }
}

impl fmt::Display for CasObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.kind, self.digest)
    }
}

/// Immutable, append-only content-addressed object store.
pub trait CasObjectStore {
    /// Stores `bytes` under a kind-qualified digest.
    ///
    /// Repeating the same kind and bytes is idempotent. Path is not an
    /// identity: callers snapshot source bytes first, then put those bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CasError::DigestCollision`] when this identity is already
    /// bound to different bytes, or [`CasError::Io`] from a filesystem
    /// backend.
    fn put(&mut self, kind: CasObjectKind, bytes: &[u8]) -> Result<CasObjectId, CasError>;

    /// Returns the exact stored bytes for `id`.
    ///
    /// # Errors
    ///
    /// Returns [`CasError::MissingObject`] when the identity is absent,
    /// [`CasError::DigestMismatch`] when stored bytes do not match `id`,
    /// or [`CasError::Io`] from a filesystem backend.
    fn get(&self, id: &CasObjectId) -> Result<Vec<u8>, CasError>;
}

/// In-memory append-only CAS used by tests and in-memory fixtures.
#[derive(Clone, Debug, Default)]
pub struct MemoryCas {
    objects: BTreeMap<CasObjectId, Vec<u8>>,
}

impl MemoryCas {
    /// Creates an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl CasObjectStore for MemoryCas {
    fn put(&mut self, kind: CasObjectKind, bytes: &[u8]) -> Result<CasObjectId, CasError> {
        let id = CasObjectId::from_payload(kind, bytes);
        match self.objects.get(&id) {
            Some(existing) if existing.as_slice() == bytes => Ok(id),
            Some(_) => Err(CasError::DigestCollision { id }),
            None => {
                self.objects.insert(id, bytes.to_vec());
                Ok(id)
            }
        }
    }

    fn get(&self, id: &CasObjectId) -> Result<Vec<u8>, CasError> {
        match self.objects.get(id) {
            Some(bytes) => verified_payload(id, bytes.clone()),
            None => Err(CasError::MissingObject { id: *id }),
        }
    }
}

/// Filesystem append-only CAS rooted at a caller-provided directory.
///
/// Objects are stored as `{root}/{kind}/{digest}`. The root is a deletable
/// cache, not lock authority. Lookups never consult an acquisition path.
#[derive(Clone, Debug)]
pub struct FilesystemCas {
    root: PathBuf,
}

impl FilesystemCas {
    /// Opens or creates an append-only object store under `root`.
    ///
    /// # Errors
    ///
    /// Returns [`CasError::Io`] when the root cannot be created, inspected, or
    /// canonicalized, or when it exists and is not a directory.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, CasError> {
        let root = root.as_ref();
        fs::create_dir_all(root).map_err(|source| CasError::io(root, source))?;
        let metadata = fs::metadata(root).map_err(|source| CasError::io(root, source))?;
        if !metadata.is_dir() {
            return Err(CasError::io(
                root,
                io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "CAS store root is not a directory",
                ),
            ));
        }
        let canonical = fs::canonicalize(root).map_err(|source| CasError::io(root, source))?;
        Ok(Self { root: canonical })
    }

    /// Returns the canonical store root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn object_path(&self, id: &CasObjectId) -> PathBuf {
        self.root.join(id.kind.as_str()).join(id.digest.to_string())
    }

    fn read_existing(&self, id: &CasObjectId) -> Result<Vec<u8>, CasError> {
        let path = self.object_path(id);
        match fs::read(&path) {
            Ok(bytes) => verified_payload(id, bytes),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                Err(CasError::MissingObject { id: *id })
            }
            Err(source) => Err(CasError::io(path, source)),
        }
    }

    fn publish(&self, id: &CasObjectId, bytes: &[u8]) -> Result<(), CasError> {
        let dest = self.object_path(id);
        match fs::read(&dest) {
            Ok(existing) => return compare_existing(id, &existing, bytes),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(CasError::io(&dest, source)),
        }

        let Some(directory) = dest.parent() else {
            return Err(CasError::io(
                &dest,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "CAS object path missing parent directory",
                ),
            ));
        };
        fs::create_dir_all(directory).map_err(|source| CasError::io(directory, source))?;

        let temp = dest.with_extension("tmp");
        if let Err(source) = write_temporary(&temp, bytes) {
            let _ = fs::remove_file(&temp);
            return Err(source);
        }

        match fs::read(&dest) {
            Ok(existing) => {
                let _ = fs::remove_file(&temp);
                return compare_existing(id, &existing, bytes);
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                let _ = fs::remove_file(&temp);
                return Err(CasError::io(&dest, source));
            }
        }

        match fs::rename(&temp, &dest) {
            Ok(()) => Ok(()),
            Err(source) => {
                let _ = fs::remove_file(&temp);
                match fs::read(&dest) {
                    Ok(existing) => compare_existing(id, &existing, bytes),
                    Err(_) => Err(CasError::io(&dest, source)),
                }
            }
        }
    }
}

impl CasObjectStore for FilesystemCas {
    fn put(&mut self, kind: CasObjectKind, bytes: &[u8]) -> Result<CasObjectId, CasError> {
        let id = CasObjectId::from_payload(kind, bytes);
        self.publish(&id, bytes)?;
        Ok(id)
    }

    fn get(&self, id: &CasObjectId) -> Result<Vec<u8>, CasError> {
        self.read_existing(id)
    }
}

fn verified_payload(id: &CasObjectId, bytes: Vec<u8>) -> Result<Vec<u8>, CasError> {
    let actual = CanonicalHash::digest(&bytes);
    if actual == id.digest {
        Ok(bytes)
    } else {
        Err(CasError::DigestMismatch { id: *id, actual })
    }
}

fn compare_existing(id: &CasObjectId, existing: &[u8], bytes: &[u8]) -> Result<(), CasError> {
    if existing == bytes {
        Ok(())
    } else {
        Err(CasError::DigestCollision { id: *id })
    }
}

fn write_temporary(path: &Path, bytes: &[u8]) -> Result<(), CasError> {
    let mut file = File::create(path).map_err(|source| CasError::io(path, source))?;
    file.write_all(bytes)
        .map_err(|source| CasError::io(path, source))?;
    file.sync_all()
        .map_err(|source| CasError::io(path, source))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use super::*;

    #[derive(Debug)]
    struct TestDirectory(tempfile::TempDir);

    impl TestDirectory {
        fn create() -> Self {
            Self(
                tempfile::Builder::new()
                    .prefix("latticeaxiom-packages-cas-")
                    .tempdir()
                    .unwrap_or_else(|error| panic!("test directory was not created: {error}")),
            )
        }

        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    fn succeeded<T, E>(result: Result<T, E>) -> T
    where
        E: Debug,
    {
        result.unwrap_or_else(|error| panic!("operation unexpectedly failed: {error:?}"))
    }

    fn filesystem_store(root: &Path) -> FilesystemCas {
        succeeded(FilesystemCas::open(root))
    }

    fn assert_round_trip(store: &mut impl CasObjectStore, kind: CasObjectKind, payload: &[u8]) {
        let first = succeeded(store.put(kind, payload));
        let second = succeeded(store.put(kind, payload));
        assert_eq!(first, second);
        assert_eq!(first, CasObjectId::from_payload(kind, payload));
        assert_eq!(succeeded(store.get(&first)), payload);
    }

    fn assert_missing(store: &impl CasObjectStore) {
        let id = CasObjectId::from_payload(CasObjectKind::PackageManifest, b"absent-object");
        match store.get(&id) {
            Err(CasError::MissingObject { id: missing }) => assert_eq!(missing, id),
            other => panic!("expected missing object, got {other:?}"),
        }
    }

    fn assert_distinct_kinds(store: &mut impl CasObjectStore) {
        let payload = b"shared-payload";
        let source = succeeded(store.put(CasObjectKind::SourceTree, payload));
        let manifest = succeeded(store.put(CasObjectKind::PackageManifest, payload));
        assert_ne!(source, manifest);
        assert_eq!(source.digest(), manifest.digest());
        assert_eq!(source.kind(), CasObjectKind::SourceTree);
        assert_eq!(manifest.kind(), CasObjectKind::PackageManifest);
        assert_eq!(succeeded(store.get(&source)), payload);
        assert_eq!(succeeded(store.get(&manifest)), payload);
        match store.get(&CasObjectId::new(
            CasObjectKind::PackageArchive,
            source.digest(),
        )) {
            Err(CasError::MissingObject { .. }) => {}
            other => panic!("same payload under a third kind must stay missing: {other:?}"),
        }
    }

    #[test]
    fn memory_put_get_round_trip() {
        let mut store = MemoryCas::new();
        assert_round_trip(&mut store, CasObjectKind::SourceTree, b"source-tree-bytes");
        assert_round_trip(
            &mut store,
            CasObjectKind::RealizedArtifact,
            b"realized-artifact-bytes",
        );
        assert_round_trip(&mut store, CasObjectKind::PackageArchive, &[]);
    }

    #[test]
    fn filesystem_put_get_round_trip() {
        let directory = TestDirectory::create();
        let mut store = filesystem_store(directory.path());
        assert_round_trip(
            &mut store,
            CasObjectKind::PackageArchive,
            b"package-archive-bytes",
        );
        let id = CasObjectId::from_payload(CasObjectKind::PackageArchive, b"package-archive-bytes");
        drop(store);
        let mut reopened = filesystem_store(directory.path());
        assert_eq!(
            succeeded(reopened.get(&id)),
            b"package-archive-bytes".as_slice()
        );
        assert_round_trip(
            &mut reopened,
            CasObjectKind::PackageArchive,
            b"package-archive-bytes",
        );
    }

    #[test]
    fn memory_missing_object() {
        assert_missing(&MemoryCas::new());
    }

    #[test]
    fn filesystem_missing_object() {
        let directory = TestDirectory::create();
        assert_missing(&filesystem_store(directory.path()));
    }

    #[test]
    fn memory_digest_collision_and_mismatch_reject() {
        let mut store = MemoryCas::new();
        let payload = b"canonical-bytes";
        let id = succeeded(store.put(CasObjectKind::PackageManifest, payload));
        store.objects.insert(id, b"tampered-bytes".to_vec());
        match store.get(&id) {
            Err(CasError::DigestMismatch {
                id: mismatched,
                actual,
            }) => {
                assert_eq!(mismatched, id);
                assert_eq!(actual, CanonicalHash::digest(b"tampered-bytes"));
            }
            other => panic!("expected digest mismatch, got {other:?}"),
        }
        match store.put(CasObjectKind::PackageManifest, payload) {
            Err(CasError::DigestCollision { id: collided }) => assert_eq!(collided, id),
            other => panic!("expected digest collision, got {other:?}"),
        }
        assert_eq!(
            store.objects.get(&id).map(Vec::as_slice),
            Some(b"tampered-bytes".as_slice())
        );
    }

    #[test]
    fn filesystem_digest_collision_and_mismatch_reject() {
        let directory = TestDirectory::create();
        let mut store = filesystem_store(directory.path());
        let payload = b"canonical-bytes";
        let id = succeeded(store.put(CasObjectKind::SourceTree, payload));
        let path = store.object_path(&id);
        succeeded(fs::write(&path, b"tampered-bytes"));
        match store.get(&id) {
            Err(CasError::DigestMismatch {
                id: mismatched,
                actual,
            }) => {
                assert_eq!(mismatched, id);
                assert_eq!(actual, CanonicalHash::digest(b"tampered-bytes"));
            }
            other => panic!("expected digest mismatch, got {other:?}"),
        }
        match store.put(CasObjectKind::SourceTree, payload) {
            Err(CasError::DigestCollision { id: collided }) => assert_eq!(collided, id),
            other => panic!("expected digest collision, got {other:?}"),
        }
        assert_eq!(succeeded(fs::read(&path)), b"tampered-bytes");
    }

    #[test]
    fn memory_kinds_with_same_payload_remain_distinct() {
        assert_distinct_kinds(&mut MemoryCas::new());
    }

    #[test]
    fn filesystem_kinds_with_same_payload_remain_distinct() {
        let directory = TestDirectory::create();
        assert_distinct_kinds(&mut filesystem_store(directory.path()));
    }

    #[test]
    fn filesystem_does_not_fall_back_to_acquisition_path() {
        let directory = TestDirectory::create();
        let acquisition = directory.path().join("workspace").join("package.toml");
        succeeded(fs::create_dir_all(
            acquisition
                .parent()
                .unwrap_or_else(|| panic!("acquisition path must have a parent")),
        ));
        let payload = b"[package]\nname = \"terrain\"\n";
        succeeded(fs::write(&acquisition, payload));

        let mut store = filesystem_store(&directory.path().join("cas"));
        let id = CasObjectId::from_payload(CasObjectKind::PackageManifest, payload);
        match store.get(&id) {
            Err(CasError::MissingObject { id: missing }) => assert_eq!(missing, id),
            other => panic!("CAS must not read the acquisition path, got {other:?}"),
        }

        let stored = succeeded(store.put(CasObjectKind::PackageManifest, payload));
        assert_eq!(stored, id);
        succeeded(fs::write(&acquisition, b"mutated-source"));
        assert_eq!(succeeded(store.get(&id)), payload);
        assert_eq!(succeeded(fs::read(&acquisition)), b"mutated-source");
    }
}
