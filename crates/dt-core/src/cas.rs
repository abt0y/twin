//! Content-Addressable Storage (CAS)
//!
//! Design: Flat store keyed by SHA3-256 hex digest. Files stored under
//! `<data_dir>/cas/<first-2-hex>/<rest-hex>`. Content is immutable;
//! writes are idempotent.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::crypto;
use crate::DTError;

/// CAS layer for immutable, content-addressed blob storage.
pub struct CasStore {
    root: PathBuf,
}

impl CasStore {
    /// Open or create a CAS store at the given root directory.
    pub fn open<P: AsRef<Path>>(root: P) -> Result<Self, DTError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(CasStore { root })
    }

    /// Store raw bytes and return the content hash.
    pub fn put(&self, data: &[u8]) -> Result<String, DTError> {
        let hash = crypto::content_hash(data);
        let path = self.path_for_hash(&hash);

        if path.exists() {
            // Already stored — CAS deduplication.
            return Ok(hash);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Atomic write: write to temp then rename.
        let temp_path = path.with_extension("tmp");
        {
            let mut file = fs::File::create(&temp_path)?;
            file.write_all(data)?;
            file.sync_all()?;
        }
        fs::rename(&temp_path, &path)?;

        Ok(hash)
    }

    /// Retrieve bytes by content hash.
    pub fn get(&self, hash: &str) -> Result<Vec<u8>, DTError> {
        let path = self.path_for_hash(hash);
        if !path.exists() {
            return Err(DTError::Cas(format!("hash not found: {}", hash)));
        }
        Ok(fs::read(&path)?)
    }

    /// Check if a hash exists.
    pub fn contains(&self, hash: &str) -> bool {
        self.path_for_hash(hash).exists()
    }

    /// Delete a blob. Rarely used; CAS is append-only by design.
    pub fn delete(&self, hash: &str) -> Result<(), DTError> {
        let path = self.path_for_hash(hash);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Resolve the filesystem path for a given hash.
    fn path_for_hash(&self, hash: &str) -> PathBuf {
        let prefix = &hash[..2];
        let suffix = &hash[2..];
        self.root.join(prefix).join(suffix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cas_put_get() {
        let dir = TempDir::new().unwrap();
        let cas = CasStore::open(dir.path()).unwrap();

        let data = b"immutable content";
        let hash = cas.put(data).unwrap();
        assert_eq!(hash.len(), 64);

        let retrieved = cas.get(&hash).unwrap();
        assert_eq!(retrieved, data);

        // Deduplication: second put returns same hash, no duplicate file.
        let hash2 = cas.put(data).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_cas_missing() {
        let dir = TempDir::new().unwrap();
        let cas = CasStore::open(dir.path()).unwrap();
        assert!(cas.get("0000000000000000000000000000000000000000000000000000000000000000").is_err());
    }
}
