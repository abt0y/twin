//! dt-schema: Schema registry, validation, and resolution.
//!
//! Loads JSON Schema + Avro definitions from the filesystem registry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod registry;
pub mod validate;

/// A loaded schema definition.
#[derive(Debug, Clone)]
pub struct SchemaDef {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub raw: String,
    pub parsed: serde_json::Value,
    pub description: String,
    pub codegen_targets: Vec<String>,
}

/// Registry of all known schemas.
#[derive(Debug, Default)]
pub struct SchemaRegistry {
    schemas: HashMap<String, SchemaDef>,
    by_path: HashMap<PathBuf, String>,
}

impl SchemaRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load all schemas from the registry.toml + filesystem.
    pub fn load_from_dir<P: AsRef<Path>>(dir: P) -> Result<Self, dt_core::DTError> {
        registry::load_registry(dir)
    }

    /// Insert a schema definition.
    pub fn insert(&mut self, def: SchemaDef) {
        let key = format!("{}@{}", def.name, def.version);
        self.by_path.insert(def.path.clone(), key.clone());
        self.schemas.insert(key, def);
    }

    /// Get schema by name@version.
    pub fn get(&self, name: &str, version: &str) -> Option<&SchemaDef> {
        self.schemas.get(&format!("{}@{}", name, version))
    }

    /// Get schema by name (latest version if multiple).
    pub fn get_latest(&self, name: &str) -> Option<&SchemaDef> {
        self.schemas
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{}@", name)))
            .max_by_key(|(k, _)| k.as_str())
            .map(|(_, v)| v)
    }

    /// List all schema names.
    pub fn list_names(&self) -> Vec<String> {
        self.schemas
            .values()
            .map(|s| s.name.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    }

    /// Count of loaded schemas.
    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_ops() {
        let mut reg = SchemaRegistry::new();
        let def = SchemaDef {
            name: "test".into(),
            version: "1.0.0".into(),
            path: PathBuf::from("test.json"),
            raw: "{}".into(),
            parsed: serde_json::json!({}),
            description: "test".into(),
            codegen_targets: vec!["rust".into()],
        };
        reg.insert(def);
        assert_eq!(reg.len(), 1);
        assert!(reg.get("test", "1.0.0").is_some());
    }
}
