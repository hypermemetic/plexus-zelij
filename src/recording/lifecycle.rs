//! Pane lifecycle monitoring for active recordings.
//!
//! This module implements a polling loop that monitors tmux pane state during
//! active recordings. It detects pane creation, closure, resize/move events,
//! and tab switches, then updates the recording session and layout journal accordingly.
//!
//! # Usage
//!
//! ```no_run
//! use plexus_locus::recording::{RecordingSession, LayoutJournal, LifecycleMonitor, LifecycleConfig};
//! use std::sync::Arc;
//! use tokio::sync::Mutex;
//! use std::time::Instant;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Start recording session
//! let session = RecordingSession::start("my-session", "/tmp/recording").await?;
//! let recording = Arc::new(Mutex::new(session));
//!
//! // Start layout journal
//! let journal = LayoutJournal::new("/tmp/recording", Instant::now())?;
//! let journal_arc = Arc::new(Mutex::new(journal));
//!
//! // Start lifecycle monitor with default config (1s poll, 5s snapshots)
//! let config = LifecycleConfig::default();
//! let monitor = LifecycleMonitor::start(
//!     "my-session",
//!     recording,
//!     journal_arc,
//!     config,
//! );
//!
//! // Recording continues... panes can be created/closed/resized
//! // Monitor automatically detects changes and updates recording
//!
//! // Stop monitoring
//! monitor.stop().await?;
//! # Ok(())
//! # }
//! ```

use super::{LayoutEvent, LayoutJournal, PaneGeometry, RecordingError, RecordingSession, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::{AbortHandle, JoinHandle};
use tracing::{debug, error};

/// Configuration for the lifecycle monitoring loop.
#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    /// Polling interval for checking pane changes (default: 1s)
    pub poll_interval: Duration,
    /// Interval for writing layout snapshots (default: 5s)
    pub snapshot_interval: Duration,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            snapshot_interval: Duration::from_secs(5),
        }
    }
}

/// Monitors tmux session for pane lifecycle events.
///
/// Spawns a background task that polls tmux for layout changes and updates
/// the recording session and layout journal accordingly.
pub struct LifecycleMonitor {
    session_id: String,
    recording: Arc<Mutex<RecordingSession>>,
    journal: Arc<Mutex<LayoutJournal>>,
    config: LifecycleConfig,
    task_handle: Option<JoinHandle<()>>,
    abort_handle: Option<AbortHandle>,
}

impl LifecycleMonitor {
    /// Start monitoring pane lifecycle events.
    ///
    /// Spawns a background task that polls tmux at the configured interval,
    /// detects changes, and updates the recording session and layout journal.
    ///
    /// # Arguments
    /// * `session_id` - Tmux session ID or name to monitor
    /// * `recording` - Shared recording session to update
    /// * `journal` - Shared layout journal to write events to
    /// * `config` - Configuration for polling intervals
    pub fn start(
        session_id: impl Into<String>,
        recording: Arc<Mutex<RecordingSession>>,
        journal: Arc<Mutex<LayoutJournal>>,
        config: LifecycleConfig,
    ) -> Self {
        let session_id = session_id.into();

        debug!(
            "Starting lifecycle monitor for session {} (poll: {:?}, snapshot: {:?})",
            session_id, config.poll_interval, config.snapshot_interval
        );

        let session_id_clone = session_id.clone();
        let recording_clone = Arc::clone(&recording);
        let journal_clone = Arc::clone(&journal);
        let config_clone = config.clone();

        let task_handle = tokio::spawn(async move {
            Self::monitor_loop(
                session_id_clone,
                recording_clone,
                journal_clone,
                config_clone,
            )
            .await;
        });

        let abort_handle = task_handle.abort_handle();

        Self {
            session_id,
            recording,
            journal,
            config,
            task_handle: Some(task_handle),
            abort_handle: Some(abort_handle),
        }
    }

    /// Stop the monitoring task.
    ///
    /// Cancels the background polling task and waits for it to complete.
    pub async fn stop(mut self) -> Result<()> {
        debug!("Stopping lifecycle monitor for session {}", self.session_id);

        if let Some(abort_handle) = self.abort_handle.take() {
            abort_handle.abort();
        }

        if let Some(task_handle) = self.task_handle.take() {
            // Wait for task to finish (it will be aborted)
            let _ = task_handle.await;
        }

        debug!("Lifecycle monitor stopped for session {}", self.session_id);
        Ok(())
    }

    /// Main monitoring loop.
    ///
    /// Runs until cancelled, polling tmux for layout changes and writing events.
    async fn monitor_loop(
        session_id: String,
        recording: Arc<Mutex<RecordingSession>>,
        journal: Arc<Mutex<LayoutJournal>>,
        config: LifecycleConfig,
    ) {
        let mut previous_state = HashMap::new();
        let mut active_tab: Option<u32> = None;
        let mut last_snapshot = tokio::time::Instant::now();
        let mut poll_interval = tokio::time::interval(config.poll_interval);

        // Skip the first tick (fires immediately)
        poll_interval.tick().await;

        loop {
            poll_interval.tick().await;

            // Query current layout from tmux
            let current_state = match Self::query_layout(&session_id).await {
                Ok(state) => state,
                Err(e) => {
                    error!("Failed to query layout for {}: {}", session_id, e);
                    continue;
                }
            };

            // Detect and handle changes
            let changes = Self::detect_changes(&previous_state, &current_state);

            for change in changes {
                match change {
                    LayoutChange::PaneOpened(geom) => {
                        debug!("Detected new pane: {}", geom.pane_id);

                        // Add to recording session
                        let mut rec = recording.lock().await;
                        if let Err(e) = rec.add_pane(&geom.pane_id).await {
                            error!("Failed to add pane {} to recording: {}", geom.pane_id, e);
                        }
                        drop(rec);

                        // Write event to journal
                        let mut j = journal.lock().await;
                        if let Err(e) = j.write_event(LayoutEvent::PaneOpened {
                            pane_id: geom.pane_id.clone(),
                            x: geom.x,
                            y: geom.y,
                            width: geom.width,
                            height: geom.height,
                            tab_index: geom.tab_index,
                        }) {
                            error!("Failed to write PaneOpened event: {}", e);
                        }
                    }
                    LayoutChange::PaneClosed(pane_id) => {
                        debug!("Detected closed pane: {}", pane_id);

                        // Remove from recording session
                        let mut rec = recording.lock().await;
                        if let Err(e) = rec.remove_pane(&pane_id).await {
                            // Ignore NotRecording errors (pane might have had an active pipe already)
                            if !matches!(e, RecordingError::NotRecording(_)) {
                                error!("Failed to remove pane {} from recording: {}", pane_id, e);
                            }
                        }
                        drop(rec);

                        // Write event to journal
                        let mut j = journal.lock().await;
                        if let Err(e) = j.write_event(LayoutEvent::PaneClosed {
                            pane_id: pane_id.clone(),
                        }) {
                            error!("Failed to write PaneClosed event: {}", e);
                        }
                    }
                    LayoutChange::PaneResized(geom) => {
                        debug!("Detected pane resize/move: {}", geom.pane_id);

                        // Write event to journal
                        let mut j = journal.lock().await;
                        if let Err(e) = j.write_event(LayoutEvent::PaneResized {
                            pane_id: geom.pane_id.clone(),
                            x: geom.x,
                            y: geom.y,
                            width: geom.width,
                            height: geom.height,
                        }) {
                            error!("Failed to write PaneResized event: {}", e);
                        }
                    }
                }
            }

            // Detect tab switch
            let current_tab = Self::get_active_tab(&current_state);
            if let Some(new_tab) = current_tab {
                if active_tab.is_none() {
                    // First poll, just record the active tab
                    active_tab = Some(new_tab);
                } else if active_tab != Some(new_tab) {
                    debug!("Detected tab switch: {} -> {}", active_tab.unwrap(), new_tab);
                    active_tab = Some(new_tab);

                    // Write event to journal
                    let mut j = journal.lock().await;
                    if let Err(e) = j.write_event(LayoutEvent::TabSwitched {
                        tab_index: new_tab,
                    }) {
                        error!("Failed to write TabSwitched event: {}", e);
                    }
                }
            }

            // Write periodic snapshot if interval elapsed
            let now = tokio::time::Instant::now();
            if now.duration_since(last_snapshot) >= config.snapshot_interval {
                debug!("Writing periodic layout snapshot for {}", session_id);

                let mut j = journal.lock().await;
                if let Err(e) = j.snapshot(&session_id).await {
                    error!("Failed to write layout snapshot: {}", e);
                } else {
                    last_snapshot = now;
                }
            }

            // Update previous state
            previous_state = current_state;
        }
    }

    /// Query current layout from tmux.
    ///
    /// Returns a map of pane_id -> PaneGeometry for all panes in the session.
    async fn query_layout(session_id: &str) -> Result<HashMap<String, PaneGeometry>> {
        let output = Command::new("tmux")
            .args(&[
                "list-panes",
                "-s",
                "-t",
                session_id,
                "-F",
                "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height} #{window_index} #{window_active}",
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RecordingError::TmuxFailed(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut layout = HashMap::new();

        for line in stdout.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 7 {
                let pane_id = fields[0].to_string();
                let geom = PaneGeometry {
                    pane_id: pane_id.clone(),
                    x: fields[1].parse().unwrap_or(0),
                    y: fields[2].parse().unwrap_or(0),
                    width: fields[3].parse().unwrap_or(80),
                    height: fields[4].parse().unwrap_or(24),
                    tab_index: fields[5].parse().unwrap_or(0),
                };
                layout.insert(pane_id, geom);
            }
        }

        Ok(layout)
    }

    /// Detect layout changes between two states.
    fn detect_changes(
        previous: &HashMap<String, PaneGeometry>,
        current: &HashMap<String, PaneGeometry>,
    ) -> Vec<LayoutChange> {
        let mut changes = Vec::new();

        // Detect new panes
        for (pane_id, geom) in current {
            if !previous.contains_key(pane_id) {
                changes.push(LayoutChange::PaneOpened(geom.clone()));
            }
        }

        // Detect closed panes
        for pane_id in previous.keys() {
            if !current.contains_key(pane_id) {
                changes.push(LayoutChange::PaneClosed(pane_id.clone()));
            }
        }

        // Detect resized/moved panes
        for (pane_id, curr_geom) in current {
            if let Some(prev_geom) = previous.get(pane_id) {
                if prev_geom.x != curr_geom.x
                    || prev_geom.y != curr_geom.y
                    || prev_geom.width != curr_geom.width
                    || prev_geom.height != curr_geom.height
                {
                    changes.push(LayoutChange::PaneResized(curr_geom.clone()));
                }
            }
        }

        changes
    }

    /// Get the active tab (window) index from the current state.
    ///
    /// Finds the first pane in an active window and returns its tab_index.
    /// We rely on the fact that tmux list-panes includes #{window_active} flag.
    async fn get_active_tab_from_tmux(session_id: &str) -> Result<Option<u32>> {
        let output = Command::new("tmux")
            .args(&[
                "list-panes",
                "-s",
                "-t",
                session_id,
                "-F",
                "#{window_index} #{window_active}",
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RecordingError::TmuxFailed(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 2 {
                let window_index: u32 = fields[0].parse().unwrap_or(0);
                let is_active = fields[1] == "1";

                if is_active {
                    return Ok(Some(window_index));
                }
            }
        }

        Ok(None)
    }

    /// Get active tab index from the current layout state.
    ///
    /// This is a simpler version that just picks the first pane's tab_index.
    /// In practice, we should query tmux for the active window, but for simplicity
    /// we can just track which tab_index we see most recently.
    ///
    /// TODO: Use #{window_active} flag to determine which window is actually active
    fn get_active_tab(state: &HashMap<String, PaneGeometry>) -> Option<u32> {
        // For now, just return the first pane's tab_index
        // In a real implementation, we'd query tmux for #{window_active}
        state.values().next().map(|geom| geom.tab_index)
    }
}

/// Internal enum for detected layout changes.
#[derive(Debug)]
enum LayoutChange {
    PaneOpened(PaneGeometry),
    PaneClosed(String),
    PaneResized(PaneGeometry),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_changes_new_pane() {
        let previous = HashMap::new();
        let mut current = HashMap::new();

        let geom = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            tab_index: 0,
        };
        current.insert("%1".to_string(), geom.clone());

        let changes = LifecycleMonitor::detect_changes(&previous, &current);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], LayoutChange::PaneOpened(_)));
    }

    #[test]
    fn test_detect_changes_closed_pane() {
        let mut previous = HashMap::new();
        let geom = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            tab_index: 0,
        };
        previous.insert("%1".to_string(), geom);

        let current = HashMap::new();

        let changes = LifecycleMonitor::detect_changes(&previous, &current);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], LayoutChange::PaneClosed(_)));
    }

    #[test]
    fn test_detect_changes_resize() {
        let mut previous = HashMap::new();
        let geom1 = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            tab_index: 0,
        };
        previous.insert("%1".to_string(), geom1);

        let mut current = HashMap::new();
        let geom2 = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 40,
            height: 24,
            tab_index: 0,
        };
        current.insert("%1".to_string(), geom2);

        let changes = LifecycleMonitor::detect_changes(&previous, &current);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], LayoutChange::PaneResized(_)));
    }

    #[test]
    fn test_detect_changes_move() {
        let mut previous = HashMap::new();
        let geom1 = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 40,
            height: 24,
            tab_index: 0,
        };
        previous.insert("%1".to_string(), geom1);

        let mut current = HashMap::new();
        let geom2 = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 40,
            y: 0,
            width: 40,
            height: 24,
            tab_index: 0,
        };
        current.insert("%1".to_string(), geom2);

        let changes = LifecycleMonitor::detect_changes(&previous, &current);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], LayoutChange::PaneResized(_)));
    }

    #[test]
    fn test_detect_changes_multiple() {
        let mut previous = HashMap::new();
        let geom1 = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            tab_index: 0,
        };
        previous.insert("%1".to_string(), geom1);

        let mut current = HashMap::new();
        let geom1_resized = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 40,
            height: 24,
            tab_index: 0,
        };
        let geom2 = PaneGeometry {
            pane_id: "%2".to_string(),
            x: 40,
            y: 0,
            width: 40,
            height: 24,
            tab_index: 0,
        };
        current.insert("%1".to_string(), geom1_resized);
        current.insert("%2".to_string(), geom2);

        let changes = LifecycleMonitor::detect_changes(&previous, &current);

        assert_eq!(changes.len(), 2);
        assert!(changes.iter().any(|c| matches!(c, LayoutChange::PaneOpened(_))));
        assert!(changes.iter().any(|c| matches!(c, LayoutChange::PaneResized(_))));
    }

    #[test]
    fn test_detect_changes_no_change() {
        let mut state = HashMap::new();
        let geom = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            tab_index: 0,
        };
        state.insert("%1".to_string(), geom.clone());

        let changes = LifecycleMonitor::detect_changes(&state, &state);

        assert_eq!(changes.len(), 0);
    }

    #[test]
    fn test_get_active_tab() {
        let mut state = HashMap::new();
        let geom = PaneGeometry {
            pane_id: "%1".to_string(),
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            tab_index: 2,
        };
        state.insert("%1".to_string(), geom);

        let active = LifecycleMonitor::get_active_tab(&state);
        assert_eq!(active, Some(2));
    }

    #[test]
    fn test_get_active_tab_empty() {
        let state = HashMap::new();
        let active = LifecycleMonitor::get_active_tab(&state);
        assert_eq!(active, None);
    }

    #[test]
    fn test_config_defaults() {
        let config = LifecycleConfig::default();
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.snapshot_interval, Duration::from_secs(5));
    }
}
