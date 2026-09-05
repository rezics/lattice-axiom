//! Concurrent publication of complete immutable catalog objects.
//!
//! Each writer owns a unique temporary file in the destination directory.
//! Publication never overwrites another writer's object; identical winners are
//! accepted, conflicting bytes are rejected. This synchronizes payload bytes,
//! but does not claim a directory durability barrier on every filesystem.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;
use thiserror::Error;

/// Failure to publish a complete immutable object.
#[derive(Debug, Error)]
pub enum ImmutableFileError {
    /// An existing object has different bytes and is left untouched.
    #[error("immutable object has different bytes at {path}")]
    Conflict {
        /// Destination of the conflicting object.
        path: PathBuf,
    },
    /// Creating, synchronizing or publishing the private temporary file failed.
    #[error("immutable object I/O failed at {path}: {source}")]
    Io {
        /// Path associated with the failed operation.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
}

fn io_error(path: &Path, source: io::Error) -> ImmutableFileError {
    ImmutableFileError::Io {
        path: path.to_owned(),
        source,
    }
}

fn compare(path: &Path, actual: &[u8], expected: &[u8]) -> Result<(), ImmutableFileError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ImmutableFileError::Conflict {
            path: path.to_owned(),
        })
    }
}

/// Publishes bytes once, or accepts an identical object published concurrently.
///
/// # Errors
/// Returns [`ImmutableFileError`] on I/O failure or conflicting existing bytes.
pub fn publish_immutable_file(path: &Path, bytes: &[u8]) -> Result<(), ImmutableFileError> {
    match fs::read(path) {
        Ok(existing) => return compare(path, &existing, bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(path, error)),
    }
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory).map_err(|error| io_error(directory, error))?;
    let mut temporary = NamedTempFile::with_prefix_in(".publish-", directory)
        .map_err(|error| io_error(directory, error))?;
    temporary
        .write_all(bytes)
        .map_err(|error| io_error(temporary.path(), error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| io_error(temporary.path(), error))?;
    publish_prepared(temporary, path, bytes)
}

fn publish_prepared(
    temporary: NamedTempFile,
    path: &Path,
    bytes: &[u8],
) -> Result<(), ImmutableFileError> {
    match temporary.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) => match fs::read(path) {
            Ok(existing) => compare(path, &existing, bytes),
            Err(_) => Err(io_error(path, error.error)),
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use std::sync::{Arc, Barrier};

    fn race(payloads: &[Vec<u8>]) -> (Vec<Result<(), ImmutableFileError>>, Vec<u8>, usize) {
        let directory = tempfile::tempdir().expect("isolated publication root");
        let path = directory.path().join("shared-object");
        let barrier = Arc::new(Barrier::new(payloads.len()));
        let results = std::thread::scope(|scope| {
            let handles = payloads
                .iter()
                .map(|bytes| {
                    let barrier = barrier.clone();
                    let path = &path;
                    let directory = directory.path();
                    scope.spawn(move || {
                        let mut temporary =
                            NamedTempFile::new_in(directory).expect("private temporary");
                        temporary.write_all(bytes).expect("complete payload");
                        temporary
                            .as_file()
                            .sync_all()
                            .expect("synchronized payload");
                        // Force every writer past the initial missing-object check.
                        barrier.wait();
                        publish_prepared(temporary, path, bytes)
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("writer thread"))
                .collect()
        });
        (
            results,
            fs::read(&path).expect("published object"),
            fs::read_dir(directory.path()).expect("directory").count(),
        )
    }

    #[test]
    fn simultaneous_identical_writers_all_succeed_and_leave_one_complete_object() {
        let bytes = vec![0x5a; 256 * 1024];
        let (results, published, entries) = race(&vec![bytes.clone(); 16]);
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        assert_eq!(published, bytes);
        assert_eq!(entries, 1);
    }

    #[test]
    fn conflicting_writers_never_replace_the_winner_or_publish_partial_bytes() {
        let left = vec![0x3a; 256 * 1024];
        let right = vec![0x7b; 128 * 1024];
        let (results, published, entries) = race(&[left.clone(), right.clone()]);
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(ImmutableFileError::Conflict { .. })))
                .count(),
            1
        );
        assert!(published == left || published == right);
        assert_eq!(entries, 1);
    }
}
