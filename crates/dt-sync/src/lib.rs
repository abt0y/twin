//! dt-sync: Local-first sync engine.
//!
//! Delta sync, vector clocks, and CRDT stubs.

use std::collections::HashMap;

pub mod vector_clock;
pub mod crdt;
pub mod delta;
pub mod quic;

/// Peer sync state.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SyncState {
    pub node_id: String,
    pub peer_clocks: HashMap<String, vector_clock::VectorClock>,
    pub last_sync_at: Option<String>,
}
