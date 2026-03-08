use async_trait::async_trait;

use crate::types::*;

/// Errors from a terminal backend.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("backend not available: {0}")]
    NotAvailable(String),

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("pane not found: {0}")]
    PaneNotFound(String),

    #[error("tab not found: {0}")]
    TabNotFound(String),

    #[error("command failed: {0}")]
    CommandFailed(String),

    #[error("unsupported operation: {0}")]
    Unsupported(String),

    #[error("{0}")]
    Other(String),
}

pub type BackendResult<T> = Result<T, BackendError>;

/// Abstract terminal workspace backend.
///
/// Zellij, tmux, WezTerm, or anything that can manage panes/tabs/sessions.
/// Locus doesn't care which — it talks through this trait.
#[async_trait]
pub trait TerminalBackend: Send + Sync + 'static {
    /// Human-readable backend name (e.g. "zellij", "tmux")
    fn name(&self) -> &str;

    /// Check if the backend is available on this system
    async fn is_available(&self) -> bool;

    // ========================================================================
    // Sessions
    // ========================================================================

    async fn list_sessions(&self) -> BackendResult<Vec<Session>>;
    async fn create_session(&self, opts: &SessionOpts) -> BackendResult<Session>;
    async fn kill_session(&self, name: &str) -> BackendResult<()>;

    // ========================================================================
    // Tabs
    // ========================================================================

    async fn list_tabs(&self, session: Option<&str>) -> BackendResult<Vec<Tab>>;
    async fn create_tab(&self, opts: &TabOpts) -> BackendResult<Tab>;
    async fn close_tab(&self, session: Option<&str>, index: u32) -> BackendResult<()>;
    async fn focus_tab(&self, session: Option<&str>, index: u32) -> BackendResult<()>;
    async fn rename_tab(&self, session: Option<&str>, index: u32, name: &str) -> BackendResult<()>;

    // ========================================================================
    // Panes
    // ========================================================================

    async fn create_pane(&self, opts: &PaneOpts) -> BackendResult<Pane>;
    async fn close_pane(&self) -> BackendResult<()>;
    async fn focus_pane(&self, direction: Direction) -> BackendResult<()>;
    async fn rename_pane(&self, name: &str) -> BackendResult<()>;
    async fn toggle_floating(&self) -> BackendResult<()>;
    async fn toggle_fullscreen(&self) -> BackendResult<()>;
    async fn resize_pane(&self, direction: Direction, amount: Option<u32>) -> BackendResult<()>;

    // ========================================================================
    // Input / Output
    // ========================================================================

    /// Send keystrokes to the focused pane
    async fn write_chars(&self, chars: &str, session: Option<&str>) -> BackendResult<()>;

    /// Capture the screen content of the current pane
    async fn dump_screen(&self, path: &str, full_scrollback: bool) -> BackendResult<String>;

    /// Dump the current layout definition
    async fn dump_layout(&self) -> BackendResult<String>;

    // ========================================================================
    // Run
    // ========================================================================

    /// Run a command in a new pane
    async fn run_command(&self, opts: &RunOpts) -> BackendResult<Pane>;
}
