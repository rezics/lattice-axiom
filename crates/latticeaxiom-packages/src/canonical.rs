use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::model::{
    BuildPlan, LOCK_SCHEMA_VERSION, LockedGameGraph, PackageError, PublishedPackageDescriptor,
};

/// Serializes a published package descriptor to canonical bytes.
///
/// # Errors
///
/// Returns [`PackageError::Json`] if the typed descriptor cannot be serialized.
pub fn canonical_descriptor_bytes(
    descriptor: &PublishedPackageDescriptor,
) -> Result<Vec<u8>, PackageError> {
    canonical_pretty_bytes(descriptor)
}

/// Serializes a lock graph to its canonical human-reviewable bytes.
///
/// The graph's vectors and maps are already sorted by the package kernel. A
/// trailing LF is part of the format so files are stable on every platform.
///
/// # Errors
///
/// Returns [`PackageError::Json`] if a versioned model cannot be serialized.
pub fn canonical_lock_bytes(graph: &LockedGameGraph) -> Result<Vec<u8>, PackageError> {
    canonical_pretty_bytes(graph)
}

/// Serializes a build plan to canonical human-reviewable bytes.
///
/// # Errors
///
/// Returns [`PackageError::Json`] if a versioned model cannot be serialized.
pub fn canonical_plan_bytes(plan: &BuildPlan) -> Result<Vec<u8>, PackageError> {
    canonical_pretty_bytes(plan)
}

/// Writes a canonical `latticeaxiom.lock`.
///
/// # Errors
///
/// Returns [`PackageError`] if serialization or file-system access fails.
pub fn write_lock(path: impl AsRef<Path>, graph: &LockedGameGraph) -> Result<(), PackageError> {
    let path = path.as_ref();
    let bytes = canonical_lock_bytes(graph)?;
    write_bytes(path, &bytes)
}

/// Writes a canonical `latticeaxiom-package.json` descriptor.
///
/// # Errors
///
/// Returns [`PackageError`] if serialization or file-system access fails.
pub fn write_descriptor(
    path: impl AsRef<Path>,
    descriptor: &PublishedPackageDescriptor,
) -> Result<(), PackageError> {
    let path = path.as_ref();
    let bytes = canonical_descriptor_bytes(descriptor)?;
    write_bytes(path, &bytes)
}

/// Reads and verifies a typed `latticeaxiom.lock`.
///
/// # Errors
///
/// Returns [`PackageError`] if the file cannot be read, is not the supported
/// schema, cannot be decoded, or has a stale semantic hash.
pub fn read_lock(path: impl AsRef<Path>) -> Result<LockedGameGraph, PackageError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|source| PackageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let graph: LockedGameGraph = serde_json::from_slice(&bytes)?;
    if graph.schema_version != LOCK_SCHEMA_VERSION {
        return Err(PackageError::UnsupportedLockSchema {
            found: graph.schema_version,
            supported: LOCK_SCHEMA_VERSION,
        });
    }
    let calculated = graph_hash(&graph)?;
    if graph.graph_sha256 != calculated {
        return Err(PackageError::LockHashMismatch {
            recorded: graph.graph_sha256.clone(),
            calculated,
        });
    }
    Ok(graph)
}

/// Hashes every regular file in a package directory using stable relative paths.
///
/// Directory enumeration order never affects the result. Symlinks are rejected
/// so the source closure cannot silently escape its declared directory.
///
/// # Errors
///
/// Returns [`PackageError`] if traversal or reading fails, or if a symlink is
/// present.
pub fn hash_directory(root: impl AsRef<Path>) -> Result<String, PackageError> {
    hash_directory_with_domain(b"latticeaxiom.package-source.v1\0", root.as_ref())
}

/// Hashes the complete shared Nickel contract library with a distinct domain.
///
/// # Errors
///
/// Returns [`PackageError`] if traversal or reading fails, or if a symlink is
/// present.
pub fn hash_contract_library(root: impl AsRef<Path>) -> Result<String, PackageError> {
    hash_directory_with_domain(b"latticeaxiom.contract-library.v1\0", root.as_ref())
}

fn hash_directory_with_domain(domain: &[u8], root: &Path) -> Result<String, PackageError> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut digest = Sha256::new();
    digest.update(domain);
    for (relative, absolute) in files {
        let bytes = fs::read(&absolute).map_err(|source| PackageError::Io {
            path: absolute,
            source,
        })?;
        digest.update((relative.len() as u64).to_be_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(lower_hex(&digest.finalize()))
}

pub(crate) fn graph_hash(graph: &LockedGameGraph) -> Result<String, PackageError> {
    let mut payload = graph.clone();
    payload.graph_sha256.clear();
    semantic_hash(b"latticeaxiom.lock-graph.v1\0", &payload)
}

pub(crate) fn plan_hash(plan: &BuildPlan) -> Result<String, PackageError> {
    let mut payload = plan.clone();
    payload.plan_sha256.clear();
    semantic_hash(b"latticeaxiom.build-plan.v1\0", &payload)
}

pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), PackageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| PackageError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, bytes).map_err(|source| PackageError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn semantic_hash<T: Serialize>(domain: &[u8], value: &T) -> Result<String, PackageError> {
    let bytes = serde_json::to_vec(value)?;
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(bytes);
    Ok(lower_hex(&digest.finalize()))
}

fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn canonical_pretty_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, PackageError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn collect_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<(String, PathBuf)>,
) -> Result<(), PackageError> {
    let entries = fs::read_dir(directory).map_err(|source| PackageError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| PackageError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| PackageError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(PackageError::SourceSymlink { path });
        }
        if file_type.is_dir() {
            collect_files(root, &path, output)?;
        } else if file_type.is_file() {
            let relative =
                path.strip_prefix(root)
                    .map_err(|_| PackageError::SourceOutsideRoot {
                        path: path.clone(),
                        root: root.to_path_buf(),
                    })?;
            let relative = relative.to_string_lossy().replace('\\', "/");
            output.push((relative, path));
        }
    }
    Ok(())
}
