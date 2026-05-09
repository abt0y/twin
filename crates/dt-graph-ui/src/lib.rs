//! `dt-graph-ui` — terminal + headless UI for the DT knowledge graph.
//!
//! Two layers:
//! - [`dashboard`] — stats + breakdowns (testable, headless).
//! - [`tui`] — interactive ratatui app driven by a [`KnowledgeRepository`].
//!
//! The headless layer is fully unit-testable; the TUI layer is exercised
//! manually via `dt graph tui`.

pub mod dashboard;
pub mod tui;

pub use dashboard::{Dashboard, DashboardStats};
