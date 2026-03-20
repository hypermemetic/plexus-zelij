//! Terminal observation and state management
//!
//! Provides in-memory terminal state tracking using vt100 emulation.
//! Enables efficient, event-driven observation of terminal panes without file I/O.

pub mod terminal_state;

pub use terminal_state::{PaneStateInfo, PaneTerminalState, TerminalStateManager};
