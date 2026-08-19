//! In-memory reference implementation of [`crate::WorldStorage`].

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::RwLock;

use crate::backend::{
    CommitPreparation, decode_memory_checkpoint, decode_scanned_receipt, encode_memory_checkpoint,
    load_chunk_with, prepare_commit_with, receipt_scan_prefix,
};
use crate::{
    ArtifactKey, ArtifactReceipt, CheckpointId, CheckpointTarget, ChunkCommit, ChunkKey,
    CommitReceipt, ReceiptQuery, StorageError, StorageResult, StoredChunk, WorldStorage,
};

const CHECKPOINT_FILE_NAME: &str = "memory-world-storage.laxe";

#[derive(Clone, Debug, Default)]
struct MemoryState {
    records: BTreeMap<Vec<u8>, Vec<u8>>,
}

/// Atomic in-memory reference implementation used by tests and headless runs.
///
/// The implementation stores the same versioned key/value records as the
/// `RocksDB` backend. Every commit is first applied to a cloned staging map and
/// becomes visible under one write lock, so errors cannot expose a partial
/// chunk even to concurrent readers.
#[derive(Debug, Default)]
pub struct MemoryWorldStorage {
    state: RwLock<MemoryState>,
}

impl MemoryWorldStorage {
    /// Creates an empty storage instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a checkpoint previously created by this memory backend.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or its versioned envelope
    /// is malformed or unsupported.
    pub fn open_checkpoint(directory: impl AsRef<Path>) -> StorageResult<(CheckpointId, Self)> {
        let path = directory.as_ref().join(CHECKPOINT_FILE_NAME);
        let mut file = fs::File::open(&path).map_err(|source| StorageError::CheckpointIo {
            path: path.clone(),
            source,
        })?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| StorageError::CheckpointIo {
                path: path.clone(),
                source,
            })?;
        let checkpoint = decode_memory_checkpoint(&bytes)?;
        Ok((
            checkpoint.id,
            Self {
                state: RwLock::new(MemoryState {
                    records: checkpoint.records,
                }),
            },
        ))
    }

    fn read_state(&self) -> StorageResult<std::sync::RwLockReadGuard<'_, MemoryState>> {
        self.state.read().map_err(|_| StorageError::LockPoisoned {
            operation: "read memory world storage",
        })
    }

    fn write_state(&self) -> StorageResult<std::sync::RwLockWriteGuard<'_, MemoryState>> {
        self.state.write().map_err(|_| StorageError::LockPoisoned {
            operation: "write memory world storage",
        })
    }
}

impl WorldStorage for MemoryWorldStorage {
    fn load_chunk(&self, key: ChunkKey) -> StorageResult<Option<StoredChunk>> {
        let state = self.read_state()?;
        load_chunk_with(key, |persistent_key| {
            Ok(state.records.get(persistent_key).cloned())
        })
    }

    fn commit_chunk(&self, commit: ChunkCommit) -> StorageResult<CommitReceipt> {
        let mut state = self.write_state()?;
        let preparation = prepare_commit_with(&commit, |persistent_key| {
            Ok(state.records.get(persistent_key).cloned())
        })?;
        match preparation {
            CommitPreparation::Replay(receipt) => Ok(receipt),
            CommitPreparation::Write(write) => {
                let parts = write.into_parts();
                let mut staged = state.records.clone();
                for key in parts.deletes {
                    staged.remove(&key);
                }
                for (key, value) in parts.puts {
                    staged.insert(key, value);
                }
                state.records = staged;
                Ok(parts.receipt)
            }
        }
    }

    fn scan_receipts(
        &self,
        query: &ReceiptQuery,
    ) -> StorageResult<Vec<(ChunkKey, ArtifactKey, ArtifactReceipt)>> {
        let prefix = receipt_scan_prefix(query)?;
        let state = self.read_state()?;
        let mut receipts = Vec::new();
        for (key, value) in state.records.range(prefix.clone()..) {
            if !key.starts_with(&prefix) {
                break;
            }
            receipts.push(decode_scanned_receipt(key, value)?);
        }
        Ok(receipts)
    }

    fn create_checkpoint(&self, target: &CheckpointTarget) -> StorageResult<CheckpointId> {
        let state = self.read_state()?;
        let bytes = encode_memory_checkpoint(target.id(), &state.records)?;
        fs::create_dir(target.directory()).map_err(|source| StorageError::CheckpointIo {
            path: target.directory().to_path_buf(),
            source,
        })?;
        let path = target.directory().join(CHECKPOINT_FILE_NAME);
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(&bytes)?;
            file.sync_all()
        })();
        if let Err(source) = result {
            return Err(StorageError::CheckpointIo { path, source });
        }
        Ok(target.id())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::conformance;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn conforms_to_world_storage_contract() {
        conformance::run_all(MemoryWorldStorage::new);
    }

    #[test]
    fn checkpoint_is_independently_openable() {
        let storage = MemoryWorldStorage::new();
        let commit = conformance::sample_commit(100);
        storage
            .commit_chunk(commit.clone())
            .expect("sample commit must succeed");

        let directory = temporary_path("checkpoint");
        let target = CheckpointTarget::new(CheckpointId::from_u128(44), &directory);
        let id = storage
            .create_checkpoint(&target)
            .expect("checkpoint creation must succeed");
        assert_eq!(id, target.id());

        let (restored_id, restored) = MemoryWorldStorage::open_checkpoint(&directory)
            .expect("checkpoint must open independently");
        assert_eq!(restored_id, target.id());
        assert_eq!(
            restored
                .load_chunk(commit.key)
                .expect("restored read must succeed")
                .expect("committed chunk must exist"),
            storage
                .load_chunk(commit.key)
                .expect("source read must succeed")
                .expect("committed chunk must exist")
        );

        fs::remove_dir_all(&directory).expect("test checkpoint directory must be removable");
    }

    fn temporary_path(label: &str) -> std::path::PathBuf {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "latticeaxiom-storage-{label}-{}-{sequence}",
            std::process::id()
        ))
    }
}
