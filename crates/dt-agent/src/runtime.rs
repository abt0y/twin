//! Agent runtime registry: maps runtime names to stubs.
//!
//! Runtime examples: .claude, .gemini, .local (local llama.cpp).

use std::collections::HashMap;

/// Runtime capabilities.
#[derive(Debug, Clone)]
pub struct RuntimeDef {
    pub name: String,
    pub wasm_path: Option<std::path::PathBuf>,
    pub env_vars: HashMap<String, String>,
    pub max_memory_mb: usize,
}

/// Registry of available agent runtimes.
#[derive(Debug, Default)]
pub struct RuntimeRegistry {
    runtimes: HashMap<String, RuntimeDef>,
}

impl RuntimeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, def: RuntimeDef) {
        self.runtimes.insert(def.name.clone(), def);
    }

    pub fn get(&self, name: &str) -> Option<&RuntimeDef> {
        self.runtimes.get(name)
    }

    pub fn default_runtimes() -> Self {
        let mut reg = Self::new();
        for name in &[".claude", ".gemini", ".local"] {
            reg.register(RuntimeDef {
                name: name.to_string(),
                wasm_path: None,
                env_vars: HashMap::new(),
                max_memory_mb: 512,
            });
        }
        reg
    }
}
