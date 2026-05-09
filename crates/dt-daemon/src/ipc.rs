//! IPC server for agent communication.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::registry::AgentRegistry;

/// IPC message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcMessage {
    /// Execute an agent
    Execute {
        agent_name: String,
        input: Vec<u8>,
    },
    
    /// Execute result
    ExecuteResult {
        output: Vec<u8>,
    },
    
    /// Stop daemon
    Stop,
    
    /// List agents
    List,
    
    /// List agents result
    ListResult {
        agents: Vec<String>,
    },
}

/// IPC server.
pub struct IpcServer {
    socket_path: PathBuf,
    registry: AgentRegistry,
}

impl IpcServer {
    /// Create a new IPC server.
    pub fn new(socket_path: PathBuf, registry: AgentRegistry) -> Result<Self> {
        Ok(Self {
            socket_path,
            registry,
        })
    }
    
    /// Run the IPC server.
    pub async fn run(&self) -> Result<()> {
        let listener = UnixListener::bind(&self.socket_path)?;
        
        loop {
            match listener.accept().await {
                Ok((mut stream, _)) => {
                    let registry = self.registry.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(&mut stream, registry).await {
                            eprintln!("Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }
    }
    
    /// Handle a single connection.
    async fn handle_connection(
        stream: &mut tokio::net::UnixStream,
        registry: AgentRegistry,
    ) -> Result<()> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;
        
        let message: IpcMessage = serde_cbor::from_slice(&buf)?;
        
        let response = match message {
            IpcMessage::Execute { agent_name, input } => {
                let _wasm_path = registry.get_wasm_path(&agent_name)?;
                let output = registry.execute_agent(&agent_name, &input)?;
                IpcMessage::ExecuteResult { output }
            }
            IpcMessage::Stop => {
                return Ok(()); // Shutdown
            }
            IpcMessage::List => {
                let agents = registry.list()?;
                IpcMessage::ListResult { agents }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown message type"));
            }
        };
        
        let response_bytes = serde_cbor::to_vec(&response)?;
        let response_len = (response_bytes.len() as u32).to_be_bytes();
        
        stream.write_all(&response_len).await?;
        stream.write_all(&response_bytes).await?;
        
        Ok(())
    }
}
