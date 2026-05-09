//! dt-embeddings: Local embedding pipeline using Ollama / llama.cpp.
//!
//! Supports local text embedding via ONNX Runtime (nomic-embed-text) and
//! remote embedding via Ollama API.

use chrono::Utc;
use ndarray::{Array1, Array2};
use thiserror::Error;

pub mod local;
pub mod remote;

/// Embedding model type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelType {
    /// Nomic-embed-text-v1 (ONNX)
    Nomic,
    /// All-MiniLM-L6-v2 (ONNX)
    MiniLM,
    /// Custom ONNX model
    Custom,
}

/// Embedding dimension.
pub const EMBEDDING_DIM: usize = 768;

/// Embedding vector.
pub type Embedding = Array1<f32>;

/// Batch of embeddings.
pub type Embeddings = Array2<f32>;

/// Embedding error.
#[derive(Error, Debug)]
pub enum EmbeddingError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    
    #[error("ONNX Runtime error: {0}")]
    OnnxError(String),
    
    #[error("Ollama API error: {0}")]
    OllamaError(String),
    
    #[error("Input validation error: {0}")]
    ValidationError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Embedding provider enum.
pub enum EmbeddingProvider {
    Local(local::OnnxProvider),
    Remote(remote::OllamaProvider),
}

impl EmbeddingProvider {
    /// Generate embedding for a single text.
    pub async fn embed(&self, text: &str) -> Result<Embedding, EmbeddingError> {
        match self {
            Self::Local(p) => p.embed(text).await,
            Self::Remote(p) => p.embed(text).await,
        }
    }
    
    /// Generate embeddings for a batch of texts.
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Embeddings, EmbeddingError> {
        match self {
            Self::Local(p) => p.embed_batch(texts).await,
            Self::Remote(p) => p.embed_batch(texts).await,
        }
    }
    
    /// Get embedding dimension.
    pub fn dim(&self) -> usize {
        match self {
            Self::Local(p) => p.dim(),
            Self::Remote(p) => p.dim(),
        }
    }
}

/// Embedding pipeline configuration.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// Model type.
    pub model: ModelType,
    
    /// Maximum batch size.
    pub max_batch_size: usize,
    
    /// Whether to use GPU acceleration (if available).
    pub use_gpu: bool,
    
    /// Ollama endpoint URL (for remote provider).
    pub ollama_url: Option<String>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: ModelType::Nomic,
            max_batch_size: 32,
            use_gpu: false,
            ollama_url: None,
        }
    }
}

/// Create an embedding provider based on configuration.
pub fn create_provider(config: EmbeddingConfig) -> Result<EmbeddingProvider, EmbeddingError> {
    if config.ollama_url.is_some() {
        Ok(EmbeddingProvider::Remote(remote::OllamaProvider::new(config)?))
    } else {
        Ok(EmbeddingProvider::Local(local::OnnxProvider::new(config)?))
    }
}

/// Compute cosine similarity between two embeddings.
pub fn cosine_similarity(a: &Embedding, b: &Embedding) -> f32 {
    let dot_product = a.dot(b);
    let norm_a = a.dot(a).sqrt();
    let norm_b = b.dot(b).sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

/// Compute Euclidean distance between two embeddings.
pub fn euclidean_distance(a: &Embedding, b: &Embedding) -> f32 {
    let diff = a - b;
    diff.dot(&diff).sqrt()
}

/// Find top-k most similar embeddings.
pub fn find_top_k(query: &Embedding, candidates: &[Embedding], k: usize) -> Vec<(usize, f32)> {
    let mut similarities: Vec<(usize, f32)> = candidates
        .iter()
        .enumerate()
        .map(|(i, emb)| (i, cosine_similarity(query, emb)))
        .collect();
    
    similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    similarities.truncate(k);
    similarities
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cosine_similarity() {
        let a = Array1::from_vec(vec![1.0, 0.0, 0.0]);
        let b = Array1::from_vec(vec![1.0, 0.0, 0.0]);
        let c = Array1::from_vec(vec![0.0, 1.0, 0.0]);
        
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 1e-6);
    }
    
    #[test]
    fn test_euclidean_distance() {
        let a = Array1::from_vec(vec![0.0, 0.0]);
        let b = Array1::from_vec(vec![3.0, 4.0]);
        
        assert!((euclidean_distance(&a, &b) - 5.0).abs() < 1e-6);
    }
    
    #[test]
    fn test_find_top_k() {
        let query = Array1::from_vec(vec![1.0, 0.0]);
        let candidates = vec![
            Array1::from_vec(vec![1.0, 0.0]),
            Array1::from_vec(vec![0.0, 1.0]),
            Array1::from_vec(vec![0.9, 0.1]),
        ];
        
        let top_k = find_top_k(&query, &candidates, 2);
        assert_eq!(top_k.len(), 2);
        assert_eq!(top_k[0].0, 0); // Most similar
    }
}
