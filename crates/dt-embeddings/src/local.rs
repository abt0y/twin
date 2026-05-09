//! Local ONNX-based embedding provider (stub).

use crate::{Embedding, Embeddings, EmbeddingConfig, EmbeddingError, ModelType, EMBEDDING_DIM};
use ndarray::{Array1, Array2};

/// ONNX-based local embedding provider (stub).
pub struct OnnxProvider {
    model_type: ModelType,
    max_batch_size: usize,
}

impl OnnxProvider {
    /// Create a new ONNX provider (stub).
    pub fn new(config: EmbeddingConfig) -> Result<Self, EmbeddingError> {
        Ok(Self {
            model_type: config.model,
            max_batch_size: config.max_batch_size,
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
        let batch_size = texts.len().min(self.max_batch_size);
        let mut embeddings = Array2::zeros((batch_size, EMBEDDING_DIM));
        
        for (i, text) in texts.iter().take(batch_size).enumerate() {
            for j in 0..EMBEDDING_DIM {
                embeddings[[i, j]] = (j as f32 + text.len() as f32) / (EMBEDDING_DIM as f32);
            }
        }
        
        Ok(embeddings)
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
    fn test_onnx_provider_creation() {
        let config = EmbeddingConfig::default();
        let provider = OnnxProvider::new(config);
        assert!(provider.is_ok());
    }
}
