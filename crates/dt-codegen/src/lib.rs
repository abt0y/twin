//! dt-codegen: Generate Rust / Python / TypeScript types and migrations from JSON Schema.
//!
//! Uses Handlebars templates for now; can be swapped for a dedicated generator later.

use std::collections::HashMap;
use std::path::Path;

pub mod rust;
pub mod python;
pub mod ts;

/// Code generation target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Rust,
    Python,
    TypeScript,
}

/// Generate code for a schema definition into the output directory.
pub fn generate(
    target: Target,
    schema: &dt_schema::SchemaDef,
    out_dir: &Path,
) -> Result<String, dt_core::DTError> {
    match target {
        Target::Rust => rust::generate(schema, out_dir),
        Target::Python => python::generate(schema, out_dir),
        Target::TypeScript => ts::generate(schema, out_dir),
    }
}

/// Generate all registered schemas for all their declared targets.
pub fn generate_all(
    registry: &dt_schema::SchemaRegistry,
    out_base: &Path,
) -> Result<HashMap<String, Vec<String>>, dt_core::DTError> {
    let mut results: HashMap<String, Vec<String>> = HashMap::new();

    for name in registry.list_names() {
        let def = registry.get_latest(&name).unwrap();
        for target_str in &def.codegen_targets {
            let target = match target_str.as_str() {
                "rust" => Target::Rust,
                "python" => Target::Python,
                "ts" | "typescript" => Target::TypeScript,
                _ => continue,
            };
            let target_dir = out_base.join(target_str);
            std::fs::create_dir_all(&target_dir)?;
            let generated = generate(target, def, &target_dir)?;
            results.entry(name.clone()).or_default().push(generated);
        }
    }

    Ok(results)
}
