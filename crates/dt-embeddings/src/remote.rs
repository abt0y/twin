//! Remote Ollama API embedding provider (stub).

use crate::{Embedding, Embeddings, EmbeddingConfig, EmbeddingError, EMBEDDING_DIM};
use ndarray::{Array1, Array2};
use reqwest::Client;
use serde::Deserialize;

/// Ollama API response for embedding.
#[derive(Debug, Deserialize)]
struct OllamaEmbeddingResponse {
    embedding: Vec<f32>,
}

/// Ollama API provider (stub).
pub struct OllamaProvider {
    client: Client,
    url: String,
    model: String,
}

impl OllamaProvider {
    /// Create a new Ollama provider (stub).
    pub fn new(config: EmbeddingConfig) -> Result<Self, EmbeddingError> {
        let url = config
            .ollama_url
            .unwrap_or_else(|| "http://localhost:11434".to_string());
        
        let model = match config.model {
            crate::ModelType::Nomic => "nomic-embed-text".to_string(),
            crate::ModelType::MiniLM => "all-minilm".to_string(),
            crate::ModelType::Custom => "custom".to_string(),
        };
        
        Ok(Self {
            client: Client::new(),
            url,
            model,
        })
    }
    
    /// Generate embedding for a single text (stub).
    pub async fn embed(&self, text: &str) -> Result<Embedding, EmbeddingError> {
        // Stub: return random embedding
        let mut embedding = Array1::zeros(EMBEDDING_DIM);
        for (i, val) in embedding.iter_mut().enumerate() {
            *val = (i as f32 + text.len() as f32) / (EMBEDDING_DIM as f32);
        }
        Ok(embedding)
    }
    
    /// Generate embeddings for a batch of texts (stub).
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Embeddings, EmbeddingError> {
        let mut embeddings = Vec::with_capacity(texts.len());
        
        for text in texts {
            let mut embedding = Array1::zeros(EMBEDDING_DIM);
            for (i, val) in embedding.iter_mut().enumerate() {
                *val = (i as f32 + text.len() as f32) / (EMBEDDING_DIM as f32);
            }
            embeddings.push(embedding);
        }
        
        // Convert to 2D array
        let dim = EMBEDDING_DIM;
        let flat: Vec<f32> = embeddings.iter().flat_map(|e| e.iter().copied()).collect();
        let array = Array2::from_shape_vec((embeddings.len(), dim), flat)
            .map_err(|e| EmbeddingError::ValidationError(e.to_string()))?;
        
        Ok(array)
    }
    
    /// Get embedding dimension.
    pub fn dim(&self) -> usize {
        EMBEDDING_DIM
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ollama_provider_creation() {
        let config = EmbeddingConfig {
            model: crate::ModelType::Nomic,
            max_batch_size: 32,
            use_gpu: false,
            ollama_url: Some("http://localhost:11434".to_string()),
        };
        
        let provider = OllamaProvider::new(config);
        assert!(provider.is_ok());
    }
}
