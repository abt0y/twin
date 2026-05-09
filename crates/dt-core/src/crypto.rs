//! Cryptographic primitives: hashing, signing, key derivation.

use crate::DTError;
use sha3::{Digest, Sha3_256};

/// Hash bytes with SHA3-256.
pub fn sha3_256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Hash bytes with Blake3.
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    blake3::hash(data).into()
}

/// Content-addressed storage hash: SHA3-256 of canonical JSON bytes.
pub fn content_hash(data: &[u8]) -> String {
    hex::encode(sha3_256(data))
}

/// Verify a content hash against raw bytes.
pub fn verify_content_hash(data: &[u8], expected_hex: &str) -> Result<(), DTError> {
    let actual = content_hash(data);
    if actual.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(DTError::Crypto(format!(
            "hash mismatch: expected {} got {}",
            expected_hex, actual
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_hash_deterministic() {
        let data = b"deterministic test";
        let h1 = content_hash(data);
        let h2 = content_hash(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_verify_content_hash_ok() {
        let data = b"hello world";
        let h = content_hash(data);
        assert!(verify_content_hash(data, &h).is_ok());
    }

    #[test]
    fn test_verify_content_hash_fail() {
        let data = b"hello world";
        assert!(verify_content_hash(data, "badhash").is_err());
    }
}
