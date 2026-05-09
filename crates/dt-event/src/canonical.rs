//! Canonical JSON serialization.
//!
//! Per spec: SHA3-256 content hashes must be deterministic. We achieve this by
//! - sorting all object keys lexicographically
//! - using compact (no whitespace) JSON
//! - never re-encoding floats with locale-specific separators
//!
//! This module is the SINGLE place where canonical bytes are produced.
//! Any change here is a hard breaking change to all on-disk content hashes.

use serde::Serialize;
use serde_json::Value;

use crate::error::EventError;

/// Serialize a value to canonical JSON bytes.
///
/// Object keys are sorted lexicographically; arrays preserve their order
/// (arrays are ordered structures, not sets).
pub fn to_canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, EventError> {
    let v: Value = serde_json::to_value(value)?;
    let canonical = canonicalize(&v);
    Ok(serde_json::to_vec(&canonical)?)
}

/// Recursively sort all object keys.
fn canonicalize(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut sorted: std::collections::BTreeMap<String, Value> =
                std::collections::BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k.clone(), canonicalize(v));
            }
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

/// Compute SHA3-256 hex digest over canonical JSON bytes.
pub fn canonical_hash<T: Serialize>(value: &T) -> Result<String, EventError> {
    let bytes = to_canonical_bytes(value)?;
    Ok(dt_core::sha3_256_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_key_ordering_deterministic() {
        let a = json!({"b": 1, "a": 2, "c": 3});
        let b = json!({"c": 3, "a": 2, "b": 1});
        assert_eq!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn test_nested_ordering() {
        let a = json!({"outer": {"z": 1, "a": 2}, "first": 0});
        let b = json!({"first": 0, "outer": {"a": 2, "z": 1}});
        assert_eq!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn test_array_order_preserved() {
        let a = json!([1, 2, 3]);
        let b = json!([3, 2, 1]);
        assert_ne!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn test_hash_length() {
        let v = json!({"x": 1});
        assert_eq!(canonical_hash(&v).unwrap().len(), 64);
    }
}
