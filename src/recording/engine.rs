//! Pipe-pane recording engine for tmux.
//!
//! Uses tmux `pipe-pane` to capture raw terminal output from panes, timestamps the bytes
//! using a FIFO (named pipe) approach, and writes per-pane `.cast` files.
//!
//! Also feeds data to the TerminalStateManager for in-memory terminal state tracking.

use crate::cast::{CastError, CastEvent, CastHeader, CastWriter};
use crate::observation::TerminalStateManager;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

#[derive(Debug, Error)]
pub enum RecordingError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Cast format error: {0}")]
    Cast(#[from] CastError),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Tmux command failed: {0}")]
    TmuxFailed(String),

    #[error("Pane not found: {0}")]
    PaneNotFound(String),

    #[error("Pane already has active pipe: {0}")]
    PaneHasPipe(String),

    #[error("Recording not active for pane: {0}")]
    NotRecording(String),

    #[error("Invalid pane ID: {0}")]
    InvalidPaneId(String),
}

pub type Result<T> = std::result::Result<T, RecordingError>;

/// Manages recording for a single tmux pane.
///
/// Uses a FIFO (named pipe) to capture output with timestamps:
/// 1. Create a FIFO at {output_dir}/pane-{id}.fifo
/// 2. Start tmux pipe-pane writing to the FIFO
/// 3. Spawn tokio task to read from FIFO, timestamp each chunk, write to .cast
#[allow(dead_code)]
pub struct PaneRecorder {
    pane_id: String,
    fifo_path: PathBuf,
    cast_path: PathBuf,
    start_time: Instant,
    start_timestamp: i64,
    reader_task: Option<JoinHandle<Result<()>>>,
    stop_sender: Option<mpsc::Sender<()>>,
}

impl PaneRecorder {
    /// Start recording a pane.
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane ID (e.g., "%5")
    /// * `output_dir` - Directory for output files
    /// * `width` - Terminal width in columns
    /// * `height` - Terminal height in rows
    /// * `terminal_state` - Optional terminal state manager for in-memory tracking
    pub async fn start(
        pane_id: String,
        output_dir: impl AsRef<Path>,
        width: u16,
        height: u16,
        terminal_state: Option<Arc<TerminalStateManager>>,
    ) -> Result<Self> {
        let output_dir = output_dir.as_ref();

        // Validate pane ID format
        if !pane_id.starts_with('%') {
            return Err(RecordingError::InvalidPaneId(pane_id));
        }

        // Check if pane already has a pipe
        if Self::check_existing_pipe(&pane_id).await? {
            warn!("Pane {} already has active pipe, skipping", pane_id);
            return Err(RecordingError::PaneHasPipe(pane_id));
        }

        // Create output directory
        fs::create_dir_all(output_dir).await?;

        // Setup paths
        let pane_id_clean = pane_id.trim_start_matches('%');
        let fifo_path = output_dir.join(format!("pane-{}.fifo", pane_id_clean));
        let cast_path = output_dir.join(format!("pane-{}.cast", pane_id_clean));

        // Remove existing FIFO if present
        let _ = fs::remove_file(&fifo_path).await;

        // Create FIFO (named pipe)
        let fifo_path_str = fifo_path.to_str()
            .ok_or_else(|| RecordingError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid FIFO path"
            )))?;

        debug!("Creating FIFO at {}", fifo_path_str);
        let mkfifo_status = Command::new("mkfifo")
            .arg(fifo_path_str)
            .status()
            .await?;

        if !mkfifo_status.success() {
            return Err(RecordingError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("mkfifo failed for {}", fifo_path_str)
            )));
        }

        // Record start time
        let start_time = Instant::now();
        let start_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Start pipe-pane (this will block until something reads from the FIFO)
        let pane_id_clone = pane_id.clone();
        let fifo_path_clone = fifo_path.clone();
        tokio::spawn(async move {
            debug!("Starting pipe-pane for {}", pane_id_clone);
            let result = Command::new("tmux")
                .args(&[
                    "pipe-pane",
                    "-t",
                    &pane_id_clone,
                    &format!("cat > {}", fifo_path_clone.display()),
                ])
                .status()
                .await;

            match result {
                Ok(status) if status.success() => {
                    debug!("pipe-pane started for {}", pane_id_clone);
                }
                Ok(status) => {
                    error!("pipe-pane failed for {}: exit code {:?}", pane_id_clone, status.code());
                }
                Err(e) => {
                    error!("pipe-pane command error for {}: {}", pane_id_clone, e);
                }
            }
        });

        // Wait a bit for pipe-pane to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create cast writer
        let header = CastHeader {
            version: 2,
            width,
            height,
            timestamp: Some(start_timestamp),
            env: None,
            title: Some(format!("Pane {}", pane_id)),
            idle_time_limit: None,
            theme: None,
        };

        let mut writer = CastWriter::create(&cast_path)?;
        writer.write_header(&header)?;
        writer.flush()?;

        // Track in terminal state manager if provided
        if let Some(ref state_mgr) = terminal_state {
            state_mgr.track_pane(&pane_id, width, height).await
                .map_err(|e| RecordingError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to track pane in state manager: {}", e)
                )))?;
            debug!("Tracking pane {} in terminal state manager", pane_id);
        }

        // Spawn reader task to read from FIFO and write timestamped events
        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        let fifo_path_for_task = fifo_path.clone();
        let cast_path_for_task = cast_path.clone();
        let start_time_for_task = start_time;
        let terminal_state_for_task = terminal_state;

        let pane_id_for_task = pane_id.clone();
        let reader_task = tokio::spawn(async move {
            debug!("Opening FIFO for reading: {}", fifo_path_for_task.display());

            // Open FIFO for reading (this will block until pipe-pane opens it for writing)
            let fifo_file = tokio::fs::File::open(&fifo_path_for_task).await?;
            let mut reader = BufReader::new(fifo_file);

            // Open cast file for appending events
            use std::io::Write as StdWrite;
            let cast_file = std::fs::OpenOptions::new()
                .append(true)
                .open(&cast_path_for_task)?;
            let mut file_writer = std::io::BufWriter::new(cast_file);

            let mut buffer = vec![0u8; 8192];

            loop {
                tokio::select! {
                    // Check for stop signal
                    _ = stop_rx.recv() => {
                        debug!("Received stop signal, finishing recording");
                        break;
                    }

                    // Read from FIFO
                    result = reader.read(&mut buffer) => {
                        match result {
                            Ok(0) => {
                                // EOF - pipe-pane was closed
                                debug!("FIFO EOF, finishing recording");
                                break;
                            }
                            Ok(n) => {
                                // Got data, timestamp it and write to .cast
                                let elapsed = start_time_for_task.elapsed();
                                let timestamp = elapsed.as_secs_f64();

                                // Feed raw bytes to terminal state manager (BEFORE converting to string)
                                if let Some(ref state_mgr) = terminal_state_for_task {
                                    if let Err(e) = state_mgr.process_output(&pane_id_for_task, &buffer[..n]).await {
                                        warn!("Failed to update terminal state for {}: {}", pane_id_for_task, e);
                                    }
                                }

                                // Convert bytes to string (may contain binary data, but JSON will escape it)
                                let data = String::from_utf8_lossy(&buffer[..n]).to_string();

                                let event = CastEvent::Output(timestamp, data);
                                let json = serde_json::to_string(&event)?;
                                writeln!(file_writer, "{}", json)?;
                                file_writer.flush()?;
                            }
                            Err(e) => {
                                error!("Error reading from FIFO: {}", e);
                                return Err(RecordingError::Io(e));
                            }
                        }
                    }
                }
            }

            file_writer.flush()?;
            Ok(())
        });

        Ok(Self {
            pane_id,
            fifo_path,
            cast_path,
            start_time,
            start_timestamp,
            reader_task: Some(reader_task),
            stop_sender: Some(stop_tx),
        })
    }

    /// Stop recording.
    ///
    /// Closes the pipe-pane, waits for the reader task to finish, and cleans up the FIFO.
    pub async fn stop(mut self) -> Result<PathBuf> {
        debug!("Stopping recording for {}", self.pane_id);

        // Close pipe-pane (no command = close existing pipe)
        let result = Command::new("tmux")
            .args(&["pipe-pane", "-t", &self.pane_id])
            .status()
            .await;

        match result {
            Ok(status) if status.success() => {
                debug!("pipe-pane stopped for {}", self.pane_id);
            }
            Ok(status) => {
                warn!("pipe-pane stop failed for {}: exit code {:?}", self.pane_id, status.code());
            }
            Err(e) => {
                warn!("pipe-pane stop command error for {}: {}", self.pane_id, e);
            }
        }

        // Signal reader task to stop
        if let Some(stop_tx) = self.stop_sender.take() {
            let _ = stop_tx.send(()).await;
        }

        // Wait for reader task to finish
        if let Some(task) = self.reader_task.take() {
            match task.await {
                Ok(Ok(())) => {
                    debug!("Reader task finished successfully for {}", self.pane_id);
                }
                Ok(Err(e)) => {
                    error!("Reader task error for {}: {}", self.pane_id, e);
                    return Err(e);
                }
                Err(e) => {
                    error!("Reader task panicked for {}: {}", self.pane_id, e);
                    return Err(RecordingError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Reader task panicked: {}", e)
                    )));
                }
            }
        }

        // Clean up FIFO
        let _ = fs::remove_file(&self.fifo_path).await;

        debug!("Recording stopped for {}, output: {}", self.pane_id, self.cast_path.display());
        Ok(self.cast_path)
    }

    /// Check if a pane already has an active pipe.
    async fn check_existing_pipe(pane_id: &str) -> Result<bool> {
        let output = Command::new("tmux")
            .args(&[
                "list-panes",
                "-t",
                pane_id,
                "-F",
                "#{pane_id} #{pane_pipe}",  // Use space as separator, not \t
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RecordingError::TmuxFailed(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();  // Use split_whitespace instead of split('\t')
            if fields.len() >= 2 && fields[0] == pane_id {
                // #{pane_pipe} is 1 if pipe is active, 0 otherwise
                return Ok(fields[1] == "1");
            }
        }

        Err(RecordingError::PaneNotFound(pane_id.to_string()))
    }
}

/// Manages recording for multiple panes in a session.
///
/// Tracks active `PaneRecorder` instances and coordinates starting/stopping.
pub struct RecordingSession {
    session_id: String,
    output_dir: PathBuf,
    recorders: HashMap<String, PaneRecorder>,
    terminal_state: Arc<TerminalStateManager>,
}

impl RecordingSession {
    /// Start recording a session.
    ///
    /// Discovers all panes in the session and starts recording each one.
    ///
    /// # Arguments
    /// * `session_id` - Tmux session ID or name
    /// * `output_dir` - Directory for output files
    pub async fn start(
        session_id: impl Into<String>,
        output_dir: impl AsRef<Path>,
    ) -> Result<Self> {
        let session_id = session_id.into();
        let output_dir = output_dir.as_ref().to_path_buf();

        debug!("Starting recording session for {}", session_id);

        // Create output directory
        fs::create_dir_all(&output_dir).await?;

        // Create terminal state manager
        let terminal_state = Arc::new(TerminalStateManager::new());
        debug!("Created terminal state manager for session {}", session_id);

        // Discover all panes in the session
        let panes = Self::list_panes(&session_id).await?;
        debug!("Found {} panes in session {}", panes.len(), session_id);

        let mut recorders = HashMap::new();

        for (pane_id, width, height) in panes {
            match PaneRecorder::start(
                pane_id.clone(),
                &output_dir,
                width,
                height,
                Some(terminal_state.clone()),
            )
            .await
            {
                Ok(recorder) => {
                    debug!("Started recording for pane {}", pane_id);
                    recorders.insert(pane_id, recorder);
                }
                Err(RecordingError::PaneHasPipe(id)) => {
                    warn!("Pane {} already has pipe, skipping", id);
                }
                Err(e) => {
                    error!("Failed to start recording for pane {}: {}", pane_id, e);
                    // Continue with other panes
                }
            }
        }

        // Write layout snapshot
        Self::write_layout_snapshot(&session_id, &output_dir).await?;

        Ok(Self {
            session_id,
            output_dir,
            recorders,
            terminal_state,
        })
    }

    /// Stop recording all panes.
    ///
    /// Returns the paths to all `.cast` files.
    pub async fn stop(self) -> Result<Vec<PathBuf>> {
        debug!("Stopping recording session for {}", self.session_id);

        let mut cast_files = Vec::new();

        for (pane_id, recorder) in self.recorders {
            match recorder.stop().await {
                Ok(path) => {
                    debug!("Stopped recording for {}", pane_id);
                    cast_files.push(path);
                }
                Err(e) => {
                    error!("Failed to stop recording for {}: {}", pane_id, e);
                }
            }
        }

        debug!("Recording session stopped, {} files written", cast_files.len());
        Ok(cast_files)
    }

    /// Add a pane to the recording session.
    ///
    /// Used when a new pane is created mid-session.
    pub async fn add_pane(&mut self, pane_id: impl Into<String>) -> Result<()> {
        let pane_id = pane_id.into();

        if self.recorders.contains_key(&pane_id) {
            debug!("Pane {} already being recorded", pane_id);
            return Ok(());
        }

        // Get pane dimensions
        let (_, width, height) = Self::get_pane_info(&pane_id).await?;

        let recorder = PaneRecorder::start(
            pane_id.clone(),
            &self.output_dir,
            width,
            height,
            Some(self.terminal_state.clone()),
        )
        .await?;
        self.recorders.insert(pane_id.clone(), recorder);

        debug!("Added pane {} to recording session", pane_id);

        // Update layout
        Self::write_layout_snapshot(&self.session_id, &self.output_dir).await?;

        Ok(())
    }

    /// Remove a pane from the recording session.
    ///
    /// Used when a pane is closed mid-session.
    pub async fn remove_pane(&mut self, pane_id: impl Into<String>) -> Result<PathBuf> {
        let pane_id = pane_id.into();

        let recorder = self.recorders.remove(&pane_id)
            .ok_or_else(|| RecordingError::NotRecording(pane_id.clone()))?;

        let path = recorder.stop().await?;

        debug!("Removed pane {} from recording session", pane_id);

        // Update layout
        Self::write_layout_snapshot(&self.session_id, &self.output_dir).await?;

        Ok(path)
    }

    /// Get status of which panes are being recorded.
    pub fn status(&self) -> Vec<String> {
        self.recorders.keys().cloned().collect()
    }

    /// Get the terminal state manager.
    ///
    /// Provides access to in-memory terminal state for all tracked panes.
    pub fn terminal_state(&self) -> Arc<TerminalStateManager> {
        self.terminal_state.clone()
    }

    /// List all panes in a session with their dimensions.
    async fn list_panes(session_id: &str) -> Result<Vec<(String, u16, u16)>> {
        let output = Command::new("tmux")
            .args(&[
                "list-panes",
                "-s",
                "-t",
                session_id,
                "-F",
                "#{pane_id} #{pane_width} #{pane_height}",  // Use space separator
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RecordingError::TmuxFailed(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut panes = Vec::new();

        for line in stdout.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();  // Use split_whitespace
            if fields.len() >= 3 {
                let pane_id = fields[0].to_string();
                let width: u16 = fields[1].parse().unwrap_or(80);
                let height: u16 = fields[2].parse().unwrap_or(24);
                panes.push((pane_id, width, height));
            }
        }

        Ok(panes)
    }

    /// Get info for a single pane.
    async fn get_pane_info(pane_id: &str) -> Result<(String, u16, u16)> {
        let output = Command::new("tmux")
            .args(&[
                "list-panes",
                "-t",
                pane_id,
                "-F",
                "#{pane_id} #{pane_width} #{pane_height}",  // Use space separator
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RecordingError::TmuxFailed(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout.lines().next()
            .ok_or_else(|| RecordingError::PaneNotFound(pane_id.to_string()))?;

        let fields: Vec<&str> = line.split_whitespace().collect();  // Use split_whitespace
        if fields.len() >= 3 {
            let pane_id = fields[0].to_string();
            let width: u16 = fields[1].parse().unwrap_or(80);
            let height: u16 = fields[2].parse().unwrap_or(24);
            Ok((pane_id, width, height))
        } else {
            Err(RecordingError::PaneNotFound(pane_id.to_string()))
        }
    }

    /// Write a layout snapshot to layout.jsonl.
    async fn write_layout_snapshot(session_id: &str, output_dir: &Path) -> Result<()> {
        let layout_path = output_dir.join("layout.jsonl");

        // Get current layout from tmux
        let output = Command::new("tmux")
            .args(&[
                "list-panes",
                "-s",
                "-t",
                session_id,
                "-F",
                "#{pane_id} #{pane_index} #{window_index} #{pane_width} #{pane_height} #{pane_top} #{pane_left}",
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RecordingError::TmuxFailed(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        // Parse panes and create layout event
        let mut panes = Vec::new();
        for line in stdout.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 7 {
                let pane = serde_json::json!({
                    "pane_id": fields[0],
                    "pane_index": fields[1].parse::<u32>().unwrap_or(0),
                    "tab_index": fields[2].parse::<u32>().unwrap_or(0),  // Renamed from window_index
                    "width": fields[3].parse::<u16>().unwrap_or(80),
                    "height": fields[4].parse::<u16>().unwrap_or(24),
                    "y": fields[5].parse::<u16>().unwrap_or(0),  // Renamed from top
                    "x": fields[6].parse::<u16>().unwrap_or(0),  // Renamed from left
                });
                panes.push(pane);
            }
        }

        let layout_event = serde_json::json!({
            "timestamp": timestamp,
            "event": "layout_snapshot",
            "panes": panes,
        });

        // Append to layout.jsonl
        let json_line = serde_json::to_string(&layout_event)? + "\n";
        tokio::fs::write(&layout_path, json_line.as_bytes()).await?;

        debug!("Wrote layout snapshot to {}", layout_path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pane_id_validation() {
        let result = PaneRecorder::start("invalid".to_string(), "/tmp", 80, 24, None).await;
        assert!(matches!(result, Err(RecordingError::InvalidPaneId(_))));
    }
}
