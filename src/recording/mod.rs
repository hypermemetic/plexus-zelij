//! Terminal recording functionality.
//!
//! This module provides the recording engine for capturing tmux pane output
//! as asciicast v2 (.cast) files, along with layout event tracking.

pub mod engine;
pub mod layout;
pub mod lifecycle;

pub use engine::{PaneRecorder, RecordingError, RecordingSession, Result};
pub use layout::{
    LayoutError, LayoutEvent, LayoutJournal, LayoutJournalReader, PaneGeometry,
};
pub use lifecycle::{LifecycleConfig, LifecycleMonitor};
