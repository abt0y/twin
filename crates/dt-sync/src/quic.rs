//! QUIC transport layer for sync.
//!
//! Provides QUIC-based peer-to-peer sync using quinn.

use thiserror::Error;
use tokio::sync::mpsc;

/// QUIC transport error.
#[derive(Error, Debug)]
pub enum QuicError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("TLS error: {0}")]
    TlsError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// QUIC sync message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SyncMessage {
    /// Delta sync request
    DeltaRequest {
        from_clock: crate::vector_clock::VectorClock,
    },
    
    /// Delta sync response
    DeltaResponse {
        bundle: crate::delta::DeltaBundle,
    },
    
    /// Full sync request
    FullRequest,
    
    /// Full sync response
    FullResponse {
        state: crate::SyncState,
    },
}

/// QUIC transport configuration.
#[derive(Debug, Clone)]
pub struct QuicConfig {
    /// Server address
    pub server_addr: String,
    
    /// Server port
    pub server_port: u16,
    
    /// Certificate path
    pub cert_path: Option<String>,
    
    /// Private key path
    pub key_path: Option<String>,
    
    /// Whether to use self-signed certificates for development
    pub dev_mode: bool,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            server_addr: "0.0.0.0".to_string(),
            server_port: 8443,
            cert_path: None,
            key_path: None,
            dev_mode: true,
        }
    }
}

/// QUIC sync server (stub implementation).
pub struct QuicServer {
    config: QuicConfig,
}

impl QuicServer {
    /// Create a new QUIC server.
    pub async fn new(config: QuicConfig) -> Result<Self, QuicError> {
        Ok(Self { config })
    }
    
    /// Accept incoming connections (stub).
    pub async fn accept(&self) -> Result<(mpsc::UnboundedSender<SyncMessage>, mpsc::UnboundedReceiver<SyncMessage>), QuicError> {
        let (tx, rx) = mpsc::unbounded_channel();
        Ok((tx, rx))
    }
}

/// QUIC sync client (stub implementation).
pub struct QuicClient {
    config: QuicConfig,
}

impl QuicClient {
    /// Create a new QUIC client.
    pub async fn new(config: QuicConfig) -> Result<Self, QuicError> {
        Ok(Self { config })
    }
    
    /// Connect to a peer (stub).
    pub async fn connect(&self, _peer_addr: &str) -> Result<mpsc::UnboundedSender<SyncMessage>, QuicError> {
        let (tx, _) = mpsc::unbounded_channel();
        Ok(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_quic_config_default() {
        let config = QuicConfig::default();
        assert_eq!(config.server_addr, "0.0.0.0");
        assert_eq!(config.server_port, 8443);
        assert!(config.dev_mode);
    }
}
