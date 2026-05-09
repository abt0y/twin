//! IPC over Unix socket with CBOR serialization.
//!
//! Stub: full async implementation would use tokio::net::UnixListener
//! and frame the CBOR bytes with a length prefix.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Send a CBOR-encoded message over a Unix socket.
pub fn send_message<P: AsRef<Path>>(
    socket_path: P,
    msg: &crate::BusMessage,
) -> Result<(), dt_core::DTError> {
    let encoded = serde_cbor::to_vec(msg)
        .map_err(|e| dt_core::DTError::General(format!("cbor encode: {}", e)))?;
    let len = encoded.len() as u32;
    let mut stream = UnixStream::connect(socket_path.as_ref())?;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(&encoded)?;
    Ok(())
}

/// Read a CBOR-encoded message from a Unix stream.
pub fn read_message(stream: &mut UnixStream) -> Result<crate::BusMessage, dt_core::DTError> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    let msg = serde_cbor::from_slice(&buf)
        .map_err(|e| dt_core::DTError::General(format!("cbor decode: {}", e)))?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use tempfile::TempDir;

    #[test]
    fn test_cbor_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ipc.sock");

        let msg = crate::BusMessage::Action {
            agent_id: "agent-1".into(),
            tool: "search".into(),
            payload: serde_json::json!({"q": "hello"}),
        };

        // Spawn a simple listener in a thread
        let listener = UnixListener::bind(&path).unwrap();
        let path_clone = path.clone();
        let msg_clone = msg.clone();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let received = read_message(&mut stream).unwrap();
            assert!(matches!(received, crate::BusMessage::Action { .. }));
            send_message(&path_clone, &crate::BusMessage::Pong).unwrap();
        });

        send_message(&path, &msg_clone).unwrap();
    }
}
