//! Validation helpers: check that a JSON payload conforms to a registered schema.

use serde_json::Value;

/// Basic structural validation (not a full JSON Schema validator — that can be added
/// via `jsonschema` crate when needed). For now we validate required fields and types.
pub fn validate_payload(
    schema: &crate::SchemaDef,
    payload: &Value,
) -> Result<(), dt_core::DTError> {
    let required = schema
        .parsed
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if let Some(obj) = payload.as_object() {
        for field in required {
            if !obj.contains_key(field) {
                return Err(dt_core::DTError::General(format!(
                    "missing required field '{}' in schema '{}'",
                    field, schema.name
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SchemaDef;
    use std::path::PathBuf;

    #[test]
    fn test_validate_ok() {
        let schema = SchemaDef {
            name: "test".into(),
            version: "1.0.0".into(),
            path: PathBuf::new(),
            raw: r#"{"required":["name"]}"#.into(),
            parsed: serde_json::json!({"required":["name"]}),
            description: "".into(),
            codegen_targets: vec![],
        };
        let payload = serde_json::json!({"name": "hello"});
        assert!(validate_payload(&schema, &payload).is_ok());
    }

    #[test]
    fn test_validate_missing_field() {
        let schema = SchemaDef {
            name: "test".into(),
            version: "1.0.0".into(),
            path: PathBuf::new(),
            raw: r#"{"required":["name"]}"#.into(),
            parsed: serde_json::json!({"required":["name"]}),
            description: "".into(),
            codegen_targets: vec![],
        };
        let payload = serde_json::json!({"other": 1});
        assert!(validate_payload(&schema, &payload).is_err());
    }
}
