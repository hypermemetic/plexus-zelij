//! Plexus Locus - Terminal workspace orchestration and recording
//!
//! This crate provides a standalone Plexus RPC server for managing terminal multiplexer
//! sessions (tmux/zellij), recording terminal output, and compositing multi-pane recordings
//! into asciicast format.
//!
//! # Features
//!
//! - **Terminal Backend Abstraction**: Unified interface for tmux and zellij
//! - **Session Management**: Create, list, and control terminal sessions, tabs, and panes
//! - **Terminal Recording**: Capture terminal output using pipe-pane with timestamps
//! - **Multi-pane Compositing**: Merge multiple pane recordings into side-by-side layouts
//! - **Terminal State Observation**: Track in-memory terminal state with VT100 parsing
//! - **Web Viewer**: Real-time browser-based terminal session viewer
//! - **Workspace Configuration**: Define and activate pre-configured terminal layouts
//!
//! # Quick Start
//!
//! ```no_run
//! use plexus_locus::{Locus, TmuxBackend};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create Locus instance with tmux backend
//!     let locus = Locus::new(TmuxBackend::new());
//!
//!     // Start the RPC server
//!     locus.serve("127.0.0.1:7777").await?;
//!     Ok(())
//! }
//! ```

/// Terminal backend activation and RPC hub
pub mod activation;
/// Plexus activation implementations for sessions, tabs, panes, workspace, etc.
pub mod activations;
/// Terminal backend trait and common types
pub mod backend;
/// Backend implementations for tmux and zellij
pub mod backends;
/// Asciicast v2 format types and I/O
pub mod cast;
/// Multi-pane recording compositor
pub mod compositor;
/// Terminal state observation and VT100 parsing
pub mod observation;
/// Terminal recording engine using pipe-pane
pub mod recording;
/// Rhai scripting engine for workspace templates
pub mod scripting;
/// Common types used across the crate
pub mod types;

// Re-exports required by plexus_macros generated code.
// The hub_methods macro references crate::plexus::* and crate::serde_helpers.
/// Plexus core re-exports for macro-generated code
pub mod plexus {
    pub use plexus_core::plexus::*;
    pub use plexus_core::types::Handle;
}
pub use plexus_core::serde_helpers;

// Public API
pub use activation::Locus;
pub use backend::TerminalBackend;
pub use backends::{TmuxBackend, Zellij};
