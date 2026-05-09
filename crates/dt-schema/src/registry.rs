//! Registry loading: parse `registry.toml`, discover schemas, compute content hashes.

use std::path::Path;

use crate::{SchemaDef, SchemaRegistry};

/// Load the schema registry from a directory containing `registry.toml`.
pub fn load_registry<P: AsRef<Path>>(dir: P) -> Result<SchemaRegistry, dt_core::DTError> {
    let dir = dir.as_ref();
    let registry_toml = fs::read_to_string(dir.join("registry.toml"))?;
    let manifest: RegistryManifest = toml::from_str(&registry_toml)
        .map_err(|e| dt_core::DTError::General(format!("parse registry.toml: {}", e)))?;

    let mut reg = SchemaRegistry::new();
    for entry in &manifest.schemas {
        let path = dir.join(&entry.path);
        let raw = fs::read_to_string(&path)?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| dt_core::DTError::General(format!("parse {}: {}", path.display(), e)))?;

        let def = SchemaDef {
            name: entry.name.clone(),
            version: entry.version.clone(),
            path,
            raw,
            parsed,
            description: entry.description.clone(),
            codegen_targets: entry.codegen.clone(),
        };
        reg.insert(def);
    }

    Ok(reg)
}

#[derive(Debug, serde::Deserialize)]
struct RegistryManifest {
    registry: RegistryMeta,
    schemas: Vec<SchemaEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct RegistryMeta {
    version: String,
    format: String,
    last_updated: String,
}

#[derive(Debug, serde::Deserialize)]
struct SchemaEntry {
    name: String,
    version: String,
    path: String,
    description: String,
    codegen: Vec<String>,
}

use std::fs;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_load_registry() {
        let dir = TempDir::new().unwrap();
        let mut reg_file = fs::File::create(dir.path().join("registry.toml")).unwrap();
        reg_file.write_all(
            br#"
[registry]
version = "1.0"
format = "json-schema-2020-12"
last_updated = "2025-01-01T00:00:00Z"

[[schemas]]
name = "test_schema"
version = "1.0.0"
path = "test_schema.json"
description = "A test schema"
codegen = ["rust"]
"#,
        ).unwrap();

        let mut schema_file = fs::File::create(dir.path().join("test_schema.json")).unwrap();
        schema_file.write_all(b"{\"type\":\"object\"}").unwrap();

        let reg = load_registry(dir.path()).unwrap();
        assert_eq!(reg.len(), 1);
        assert!(reg.get("test_schema", "1.0.0").is_some());
    }
}
