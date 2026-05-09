//! Agent registry for managing registered agents.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Agent metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub name: String,
    pub wasm_path: PathBuf,
    pub registered_at: String,
}

/// Agent registry.
pub struct AgentRegistry {
    agents_dir: PathBuf,
    agents: HashMap<String, AgentMetadata>,
}

impl AgentRegistry {
    /// Create a new agent registry.
    pub fn new(agents_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&agents_dir)?;
        
        let mut agents = HashMap::new();
        
        // Load existing agents
        if let Ok(entries) = fs::read_dir(&agents_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = Self::load_agent_metadata(&entry.path()) {
                    agents.insert(meta.name.clone(), meta);
                }
            }
        }
        
        Ok(Self {
            agents_dir,
            agents,
        })
    }
    
    /// Register an agent.
    pub fn register(&mut self, name: &str, wasm_path: &Path) -> Result<()> {
        if !wasm_path.exists() {
            return Err(anyhow::anyhow!("WASM file not found: {}", wasm_path.display()));
        }
        
        let meta = AgentMetadata {
            name: name.to_string(),
            wasm_path: wasm_path.to_path_buf(),
            registered_at: chrono::Utc::now().to_rfc3339(),
        };
        
        // Save metadata
        let meta_path = self.agents_dir.join(format!("{}.json", name));
        let meta_json = serde_json::to_string_pretty(&meta)?;
        fs::write(&meta_path, meta_json)?;
        
        self.agents.insert(name.to_string(), meta);
        
        Ok(())
    }
    
    /// Unregister an agent.
    pub fn unregister(&mut self, name: &str) -> Result<()> {
        self.agents.remove(name);
        
        let meta_path = self.agents_dir.join(format!("{}.json", name));
        if meta_path.exists() {
            fs::remove_file(&meta_path)?;
        }
        
        Ok(())
    }
    
    /// List registered agents.
    pub fn list(&self) -> Result<Vec<String>> {
        Ok(self.agents.keys().cloned().collect())
    }
    
    /// Get WASM path for an agent.
    pub fn get_wasm_path(&self, name: &str) -> Result<PathBuf> {
        self.agents
            .get(name)
            .map(|meta| meta.wasm_path.clone())
            .ok_or_else(|| anyhow::anyhow!("Agent not found: {}", name))
    }
    
    /// Execute an agent (placeholder - actual execution happens in sandbox).
    pub fn execute_agent(&self, name: &str, input: &[u8]) -> Result<Vec<u8>> {
        let _wasm_path = self.get_wasm_path(name)?;
        
        // In production, this would use the sandbox
        // For now, return a placeholder response
        Ok(format!("Executed {} with {} bytes of input", name, input.len()).into_bytes())
    }
    
    /// Load agent metadata from file.
    fn load_agent_metadata(path: &Path) -> Result<AgentMetadata> {
        let content = fs::read_to_string(path)?;
        let meta: AgentMetadata = serde_json::from_str(&content)?;
        Ok(meta)
    }
}

impl Clone for AgentRegistry {
    fn clone(&self) -> Self {
        Self {
            agents_dir: self.agents_dir.clone(),
            agents: self.agents.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_registry_creation() {
        let dir = TempDir::new().unwrap();
        let registry = AgentRegistry::new(dir.path().join("agents"));
        assert!(registry.is_ok());
    }
    
    #[test]
    fn test_register_agent() {
        let dir = TempDir::new().unwrap();
        let mut registry = AgentRegistry::new(dir.path().join("agents")).unwrap();
        
        // Create a dummy WASM file
        let wasm_path = dir.path().join("test.wasm");
        fs::write(&wasm_path, b"dummy wasm content").unwrap();
        
        let result = registry.register("test-agent", &wasm_path);
        assert!(result.is_ok());
    }
}
