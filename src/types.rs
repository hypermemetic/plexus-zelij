use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Identity types — survive re-layouts, backend-agnostic
// ============================================================================

/// Logical pane identifier. Assigned by Locus, not the backend.
/// Backends map this to their native pane ID internally.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct PaneId(pub String);

/// A pane reference — either a name or a %id. Must be resolved via
/// `backend.resolve_pane()` before use. This type enforces that callers
/// go through resolution rather than passing raw strings.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(transparent)]
pub struct PaneRef(pub String);

impl PaneRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PaneRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Logical tab identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct TabId(pub String);

/// Session identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SessionId(pub String);

// ============================================================================
// Descriptors — what Locus knows about panes/tabs/sessions
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Pane {
    pub id: PaneId,
    pub name: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<PathBuf>,
    pub floating: bool,
    pub focused: bool,
    pub tab: TabId,
    pub session: SessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Tab {
    pub id: TabId,
    pub name: Option<String>,
    pub index: u32,
    pub pane_count: u32,
    pub focused: bool,
    pub session: SessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub tabs: u32,
    pub panes: u32,
    pub attached: bool,
}

// ============================================================================
// Options — what you can request when creating things
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PaneOpts {
    /// Human-readable name (Locus tracks this for lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Command to run in the pane
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Working directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,

    /// Split direction relative to focused pane
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,

    /// Open as floating pane
    #[serde(default)]
    pub floating: bool,

    /// Close pane when command exits
    #[serde(default)]
    pub close_on_exit: bool,

    /// Target session (default: current)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,

    /// Target tab (default: current)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,

    /// Pane to split from (e.g. "%5"). Default: current/focused pane
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TabOpts {
    pub name: Option<String>,
    pub layout: Option<String>,
    pub cwd: Option<PathBuf>,
    pub session: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SessionOpts {
    pub name: String,
    pub layout: Option<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RunOpts {
    /// Command string to execute
    pub command: String,

    /// Pane name for tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Working directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,

    /// Split direction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,

    /// Open as floating
    #[serde(default)]
    pub floating: bool,

    /// Close when done
    #[serde(default)]
    pub close_on_exit: bool,

    /// Target session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,

    /// Pane to split from (e.g. "%5"). Default: current/focused pane
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Result of a single operation in a batch
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BatchEntry {
    pub pane: PaneId,
    pub command: String,
    pub output: Option<String>,
    pub success: bool,
}

// ============================================================================
// Enums
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

// ============================================================================
// Events — what the activation streams back
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LocusEvent {
    /// Session listing result
    Sessions { sessions: Vec<Session> },

    /// Tab listing result
    Tabs { tabs: Vec<Tab> },

    /// Pane listing result
    Panes { panes: Vec<Pane> },

    /// A pane was created
    PaneCreated { pane: Pane },

    /// A tab was created
    TabCreated { tab: Tab, initial_pane: Option<PaneId> },

    /// A session was created
    SessionCreated { session: Session },

    /// Screen content captured from a pane
    ScreenCapture { pane: PaneId, content: String, lines: u32 },

    /// Layout dumped
    Layout { content: String },

    /// A command was launched in a pane
    CommandLaunched { pane: PaneId, command: String },

    /// A command was executed in an existing pane and confirmed running
    CommandStarted { pane: PaneId, command: String, capture: Option<String> },

    /// A command ran and exited
    CommandExited { pane: PaneId, command: String, exit_code: Option<i32>, capture: Option<String> },

    /// Input was sent to a pane
    InputSent { pane: PaneId, chars: u32 },

    /// A grid layout was created with multiple panes
    LayoutCreated { tab: TabId, panes: Vec<Pane>, rows: u32, cols: u32 },

    /// Batch results from multiple pane operations
    BatchResult { results: Vec<BatchEntry> },

    /// Screen diff after sending a command to a foreign shell
    ScreenDiff { pane: PaneId, before_lines: u32, after_lines: u32, new_content: String },

    /// Screen content from terminal state (instant, no file I/O)
    ScreenContent { pane: PaneId, contents: String },

    /// Screen content changed (event-driven notification)
    ScreenChanged { pane: PaneId, contents: String, sequence: u64 },

    /// Cursor position
    CursorPosition { pane: PaneId, row: u16, col: u16 },

    /// Region content (specific rows)
    RegionContent { pane: PaneId, content: String },

    /// Terminal dimensions
    Dimensions { pane: PaneId, width: u16, height: u16 },

    /// No changes since requested sequence
    NoChanges { sequence: u64 },

    /// Sequence number update
    SequenceUpdate { pane: PaneId, sequence: u64 },

    /// List of tracked panes
    TrackedPanes { panes: Vec<String> },

    /// Pane state info list
    PaneStateInfos { infos: Vec<crate::observation::PaneStateInfo> },

    /// Operation timed out
    Timeout,

    /// Action completed successfully
    Ok { message: String },

    /// Something went wrong
    Error { message: String },
}

// ============================================================================
// Recording Types
// ============================================================================

/// Information about a past recording
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecordingInfo {
    pub recording_id: String,
    pub output_dir: String,
    pub pane_count: u32,
    pub has_layout: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
}

/// Events for recording operations
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordingEvent {
    /// Recording started successfully
    RecordingStarted { recording_id: String, pane_count: u32, output_dir: String },

    /// Recording stopped successfully
    RecordingStopped {
        recording_id: String,
        cast_files: Vec<String>,
        layout_file: String,
        duration_secs: f64,
    },

    /// Recording status
    RecordingStatus {
        active: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        recording_id: Option<String>,
        pane_ids: Vec<String>,
        elapsed_secs: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_dir: Option<String>,
    },

    /// List of recordings
    Recordings { recordings: Vec<RecordingInfo> },

    /// Action completed successfully
    Ok { message: String },

    /// Something went wrong
    Error { message: String },
}

// ============================================================================
// Render Types
// ============================================================================

/// Events for render operations
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RenderEvent {
    /// Render progress update
    RenderProgress { percent: f64, frames_written: u64, elapsed_secs: f64 },

    /// Render completed successfully
    RenderComplete { output_path: String, duration_secs: f64, frame_count: u64, bytes: u64 },

    /// Preview frame at timestamp
    PreviewFrame { content: String, width: u16, height: u16, time: f64 },

    /// Recording metadata
    RecordingInfo { recording_id: String, pane_count: u32, duration_secs: f64, layout_events: u32 },

    /// Something went wrong
    Error { message: String },
}
