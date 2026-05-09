//! ID generation: ULID for lexicographically sortable, unique identifiers.

use ulid::Ulid;

/// Generate a new ULID string.
pub fn new_ulid() -> String {
    Ulid::new().to_string()
}

/// Validate a ULID string.
pub fn is_valid_ulid(s: &str) -> bool {
    s.parse::<Ulid>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ulid_roundtrip() {
        let id = new_ulid();
        assert!(is_valid_ulid(&id));
        assert_eq!(id.len(), 26);
    }
}
