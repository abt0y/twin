//! dt-agent: Agent substrate and IPC daemon (dtd) stubs.
//!
//! Provides a shared memory bus over Unix socket using CBOR,
//! with isolated WASM-sandboxed agent runtimes.

pub mod ipc;
pub mod runtime;
pub mod sandbox;

/// Agent descriptor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Agent {
    pub agent_id: String,
    pub runtime: String,
    pub capabilities: Vec<String>,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AgentStatus {
    Idle,
    Running,
    Error(String),
    Suspended,
}

/// Bus message on the IPC channel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BusMessage {
    Ping,
    Pong,
    Action {
        agent_id: String,
        tool: String,
        payload: serde_json::Value,
    },
    Observation {
        agent_id: String,
        payload: serde_json::Value,
    },
    Shutdown,
}
