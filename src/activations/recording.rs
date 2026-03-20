use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::backend::TerminalBackend;
use crate::plexus::{Activation, ChildRouter, PlexusError, PlexusStream};
use crate::recording::{LayoutJournal, RecordingSession};
use crate::types::*;

/// Recording sub-activation — manages terminal session recordings.
///
/// Accessed as `locus.recording.start`, `locus.recording.stop`, etc.
#[derive(Clone)]
pub struct RecordingActivation {
    pub(crate) backend: Arc<dyn TerminalBackend>,
    pub(crate) active_recording: Arc<Mutex<Option<ActiveRecording>>>,
}

/// State for an active recording session
pub(crate) struct ActiveRecording {
    recording_id: String,
    session_id: String,
    session: RecordingSession,
    journal: LayoutJournal,
    start_time: Instant,
    output_dir: PathBuf,
}

impl RecordingActivation {
    pub fn new(backend: Arc<dyn TerminalBackend>) -> Self {
        Self {
            backend,
            active_recording: Arc::new(Mutex::new(None)),
        }
    }

    /// Generate a recording ID as ISO 8601 compact timestamp
    fn generate_recording_id() -> String {
        let now = chrono::Local::now();
        now.format("%Y%m%dT%H%M%S").to_string()
    }

    /// Get default output directory for recordings
    fn default_output_dir(recording_id: &str) -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".local/share/locus/recordings")
            .join(recording_id)
    }

    /// List past recordings by scanning the recordings directory
    async fn list_recordings() -> Vec<RecordingInfo> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let recordings_dir = PathBuf::from(home).join(".local/share/locus/recordings");

        let mut recordings = Vec::new();

        if let Ok(entries) = tokio::fs::read_dir(&recordings_dir).await {
            let mut entries = entries;
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.path().is_dir() {
                    if let Some(recording_id) = entry.file_name().to_str() {
                        let layout_file = entry.path().join("layout.jsonl");

                        // Count .cast files
                        let mut cast_count = 0;
                        if let Ok(mut files) = tokio::fs::read_dir(&entry.path()).await {
                            while let Ok(Some(file_entry)) = files.next_entry().await {
                                if let Some(ext) = file_entry.path().extension() {
                                    if ext == "cast" {
                                        cast_count += 1;
                                    }
                                }
                            }
                        }

                        // Get directory metadata for creation time
                        let created_at = entry
                            .metadata()
                            .await
                            .and_then(|m| m.created())
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs());

                        recordings.push(RecordingInfo {
                            recording_id: recording_id.to_string(),
                            output_dir: entry.path().to_string_lossy().to_string(),
                            pane_count: cast_count,
                            has_layout: layout_file.exists(),
                            created_at,
                        });
                    }
                }
            }
        }

        // Sort by recording_id descending (most recent first)
        recordings.sort_by(|a, b| b.recording_id.cmp(&a.recording_id));
        recordings
    }
}

#[plexus_macros::hub_methods(
    namespace = "recording",
    version = "0.1.0",
    description = "Terminal session recording management"
)]
impl RecordingActivation {
    #[plexus_macros::hub_method(
        description = "Start recording all panes in a session",
        params(
            session = "Session name to record (default: current session)",
            output_dir = "Output directory for recording files (default: ~/.local/share/locus/recordings/{timestamp})"
        )
    )]
    async fn start(
        &self,
        session: Option<String>,
        output_dir: Option<String>,
    ) -> impl Stream<Item = RecordingEvent> + Send + 'static {
        let backend = self.backend.clone();
        let active_recording = self.active_recording.clone();

        stream! {
            // Check if backend is tmux
            if backend.name() != "tmux" {
                yield RecordingEvent::Error {
                    message: "Recording requires tmux backend (uses pipe-pane)".to_string(),
                };
                return;
            }

            // Check if already recording
            {
                let guard = active_recording.lock().await;
                if guard.is_some() {
                    yield RecordingEvent::Error {
                        message: "Recording already in progress. Stop current recording first.".to_string(),
                    };
                    return;
                }
            }

            // Get session ID
            let session_id = if let Some(s) = session {
                s
            } else {
                // Use current session (get from $TMUX or default to first session)
                match backend.list_sessions().await {
                    Ok(sessions) => {
                        if sessions.is_empty() {
                            yield RecordingEvent::Error {
                                message: "No tmux sessions found".to_string(),
                            };
                            return;
                        }
                        sessions[0].name.clone()
                    }
                    Err(e) => {
                        yield RecordingEvent::Error {
                            message: format!("Failed to list sessions: {}", e),
                        };
                        return;
                    }
                }
            };

            // Generate recording ID and output directory
            let recording_id = Self::generate_recording_id();
            let output_dir = if let Some(dir) = output_dir {
                PathBuf::from(dir)
            } else {
                Self::default_output_dir(&recording_id)
            };

            // Start recording session
            let start_time = Instant::now();
            match RecordingSession::start(&session_id, &output_dir).await {
                Ok(recording_session) => {
                    let pane_count = recording_session.status().len();

                    // Create layout journal
                    match LayoutJournal::new(&output_dir, start_time) {
                        Ok(mut journal) => {
                            // Write initial snapshot
                            if let Err(e) = journal.snapshot(&session_id).await {
                                yield RecordingEvent::Error {
                                    message: format!("Failed to write initial layout snapshot: {}", e),
                                };
                                return;
                            }

                            // Store active recording
                            {
                                let mut guard = active_recording.lock().await;
                                *guard = Some(ActiveRecording {
                                    recording_id: recording_id.clone(),
                                    session_id: session_id.clone(),
                                    session: recording_session,
                                    journal,
                                    start_time,
                                    output_dir: output_dir.clone(),
                                });
                            }

                            yield RecordingEvent::RecordingStarted {
                                recording_id,
                                pane_count: pane_count as u32,
                                output_dir: output_dir.to_string_lossy().to_string(),
                            };
                        }
                        Err(e) => {
                            yield RecordingEvent::Error {
                                message: format!("Failed to create layout journal: {}", e),
                            };
                        }
                    }
                }
                Err(e) => {
                    yield RecordingEvent::Error {
                        message: format!("Failed to start recording: {}", e),
                    };
                }
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Stop the active recording",
        params(
            recording_id = "Recording ID to stop (default: active recording)"
        )
    )]
    async fn stop(
        &self,
        recording_id: Option<String>,
    ) -> impl Stream<Item = RecordingEvent> + Send + 'static {
        let active_recording = self.active_recording.clone();

        stream! {
            // Take the active recording
            let recording_state = {
                let mut guard = active_recording.lock().await;
                guard.take()
            };

            match recording_state {
                Some(active) => {
                    // Validate recording_id if provided
                    if let Some(ref id) = recording_id {
                        if id != &active.recording_id {
                            yield RecordingEvent::Error {
                                message: format!("Recording ID mismatch: expected {}, got {}", active.recording_id, id),
                            };
                            // Restore the recording
                            let mut guard = active_recording.lock().await;
                            *guard = Some(active);
                            return;
                        }
                    }

                    // Close the journal
                    if let Err(e) = active.journal.close() {
                        yield RecordingEvent::Error {
                            message: format!("Failed to close layout journal: {}", e),
                        };
                    }

                    let layout_file = active.output_dir.join("layout.jsonl");

                    // Stop the recording session
                    match active.session.stop().await {
                        Ok(cast_files) => {
                            let duration_secs = active.start_time.elapsed().as_secs_f64();
                            let cast_file_paths: Vec<String> = cast_files
                                .iter()
                                .map(|p| p.to_string_lossy().to_string())
                                .collect();

                            yield RecordingEvent::RecordingStopped {
                                recording_id: active.recording_id,
                                cast_files: cast_file_paths,
                                layout_file: layout_file.to_string_lossy().to_string(),
                                duration_secs,
                            };
                        }
                        Err(e) => {
                            yield RecordingEvent::Error {
                                message: format!("Failed to stop recording: {}", e),
                            };
                        }
                    }
                }
                None => {
                    yield RecordingEvent::Error {
                        message: "No active recording".to_string(),
                    };
                }
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Get status of the active recording"
    )]
    async fn status(&self) -> impl Stream<Item = RecordingEvent> + Send + 'static {
        let active_recording = self.active_recording.clone();

        stream! {
            let guard = active_recording.lock().await;
            match &*guard {
                Some(active) => {
                    let pane_ids = active.session.status();
                    let elapsed_secs = active.start_time.elapsed().as_secs_f64();

                    yield RecordingEvent::RecordingStatus {
                        active: true,
                        recording_id: Some(active.recording_id.clone()),
                        pane_ids,
                        elapsed_secs,
                        output_dir: Some(active.output_dir.to_string_lossy().to_string()),
                    };
                }
                None => {
                    yield RecordingEvent::RecordingStatus {
                        active: false,
                        recording_id: None,
                        pane_ids: Vec::new(),
                        elapsed_secs: 0.0,
                        output_dir: None,
                    };
                }
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Manually trigger a layout snapshot",
        params(
            recording_id = "Recording ID (default: active recording)"
        )
    )]
    async fn snapshot_layout(
        &self,
        recording_id: Option<String>,
    ) -> impl Stream<Item = RecordingEvent> + Send + 'static {
        let active_recording = self.active_recording.clone();

        stream! {
            let mut guard = active_recording.lock().await;
            match &mut *guard {
                Some(ref mut active) => {
                    // Validate recording_id if provided
                    if let Some(ref id) = recording_id {
                        if id != &active.recording_id {
                            yield RecordingEvent::Error {
                                message: format!("Recording ID mismatch: expected {}, got {}", active.recording_id, id),
                            };
                            return;
                        }
                    }

                    // Write layout snapshot
                    match active.journal.snapshot(&active.session_id).await {
                        Ok(()) => {
                            yield RecordingEvent::Ok {
                                message: "Layout snapshot written".to_string(),
                            };
                        }
                        Err(e) => {
                            yield RecordingEvent::Error {
                                message: format!("Failed to write layout snapshot: {}", e),
                            };
                        }
                    }
                }
                None => {
                    yield RecordingEvent::Error {
                        message: "No active recording".to_string(),
                    };
                }
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "List past recordings"
    )]
    async fn list(&self) -> impl Stream<Item = RecordingEvent> + Send + 'static {
        stream! {
            let recordings = Self::list_recordings().await;
            yield RecordingEvent::Recordings { recordings };
        }
    }
}

#[async_trait]
impl ChildRouter for RecordingActivation {
    fn router_namespace(&self) -> &str {
        "recording"
    }

    async fn router_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, _name: &str) -> Option<Box<dyn ChildRouter>> {
        None
    }
}
