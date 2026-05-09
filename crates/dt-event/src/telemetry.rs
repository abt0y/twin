//! Structured JSONL logging for the event subsystem.
//!
//! Writes one JSON object per line to `~/.dt/logs/events.jsonl` (or a
//! configurable path). Designed to be tail-friendly, grep-friendly, and
//! cheap to ingest with DuckDB or jq.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::error::EventError;
use crate::event::Event;

/// A single JSONL log line.
#[derive(Debug, Serialize)]
struct LogLine<'a> {
    ts: DateTime<Utc>,
    level: &'a str,
    target: &'a str,
    event_id: Option<&'a str>,
    event_type: Option<String>,
    node_id: Option<&'a str>,
    content_hash: Option<&'a str>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<serde_json::Value>,
}

/// Append-only JSONL log writer for events.
pub struct JsonlLogger {
    file: Mutex<File>,
    path: PathBuf,
}

impl JsonlLogger {
    /// Open or create the log file.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, EventError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            file: Mutex::new(file),
            path,
        })
    }

    /// Path to the underlying log file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append a structured log line for an event.
    pub fn log_event(&self, level: &str, message: &str, event: &Event) -> Result<(), EventError> {
        let line = LogLine {
            ts: Utc::now(),
            level,
            target: "dt-event",
            event_id: Some(&event.event_id),
            event_type: Some(event.event_type.to_string()),
            node_id: Some(&event.node_id),
            content_hash: event.content_hash.as_deref(),
            message,
            extra: None,
        };
        self.write_line(&line)
    }

    /// Append a structured log line without an event context.
    pub fn log(&self, level: &str, message: &str, extra: Option<serde_json::Value>) -> Result<(), EventError> {
        let line = LogLine {
            ts: Utc::now(),
            level,
            target: "dt-event",
            event_id: None,
            event_type: None,
            node_id: None,
            content_hash: None,
            message,
            extra,
        };
        self.write_line(&line)
    }

    fn write_line(&self, line: &LogLine<'_>) -> Result<(), EventError> {
        let mut buf = serde_json::to_vec(line)?;
        buf.push(b'\n');
        let mut guard = self
            .file
            .lock()
            .map_err(|e| EventError::Storage(format!("log mutex poisoned: {}", e)))?;
        guard.write_all(&buf)?;
        guard.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventBuilder, EventType};
    use tempfile::TempDir;

    #[test]
    fn test_log_event_writes_jsonl() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("events.jsonl");
        let logger = JsonlLogger::open(&log_path).unwrap();

        let ev = EventBuilder::new(EventType::KnowledgeCreate, "n1", "did:dt:u")
            .build()
            .unwrap();
        logger.log_event("info", "appended", &ev).unwrap();

        let contents = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["level"], "info");
        assert_eq!(parsed["event_type"], "knowledge.create");
        assert_eq!(parsed["node_id"], "n1");
        assert_eq!(parsed["message"], "appended");
    }

    #[test]
    fn test_log_appends() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("events.jsonl");
        let logger = JsonlLogger::open(&log_path).unwrap();
        logger.log("info", "first", None).unwrap();
        logger.log("info", "second", None).unwrap();
        let contents = std::fs::read_to_string(&log_path).unwrap();
        assert_eq!(contents.lines().count(), 2);
    }
}
