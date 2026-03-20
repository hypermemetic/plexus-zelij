//! Layout event journal for tracking pane geometry changes during recording.
//!
//! This module provides a JSONL-based event log that records layout changes
//! (pane create/close/resize, tab switch) with timestamps. The compositor reads
//! this to reconstruct pane positions at any point during the recording.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Tmux command failed: {0}")]
    TmuxFailed(String),

    #[error("Invalid layout format: {0}")]
    InvalidFormat(String),
}

pub type Result<T> = std::result::Result<T, LayoutError>;

/// Geometry information for a single pane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaneGeometry {
    /// Pane ID (e.g., "%5")
    pub pane_id: String,
    /// X coordinate (columns from left)
    pub x: u16,
    /// Y coordinate (rows from top)
    pub y: u16,
    /// Width in columns
    pub width: u16,
    /// Height in rows
    pub height: u16,
    /// Tab (window) index
    pub tab_index: u32,
}

/// Layout change events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayoutEvent {
    /// A new pane was created
    PaneOpened { pane_id: String, x: u16, y: u16, width: u16, height: u16, tab_index: u32 },

    /// A pane was closed
    PaneClosed { pane_id: String },

    /// A pane was moved or resized
    PaneResized { pane_id: String, x: u16, y: u16, width: u16, height: u16 },

    /// Active tab changed
    TabSwitched { tab_index: u32 },

    /// Full layout snapshot (keyframe)
    LayoutSnapshot { panes: Vec<PaneGeometry> },
}

/// Timestamped layout event (internal format for JSONL)
#[derive(Debug, Serialize, Deserialize)]
struct TimestampedEvent {
    time: f64,
    #[serde(flatten)]
    event: LayoutEvent,
}

/// Writer for layout event journal.
///
/// Writes timestamped layout events to a JSONL file (layout.jsonl).
pub struct LayoutJournal {
    output_path: PathBuf,
    writer: BufWriter<File>,
    start_time: Instant,
}

impl LayoutJournal {
    /// Create a new layout journal.
    ///
    /// Creates (or truncates) layout.jsonl in the output directory.
    ///
    /// # Arguments
    /// * `output_dir` - Directory for the layout.jsonl file
    /// * `start_time` - Recording start time (for relative timestamps)
    pub fn new(output_dir: impl AsRef<Path>, start_time: Instant) -> Result<Self> {
        let output_path = output_dir.as_ref().join("layout.jsonl");
        let file = File::create(&output_path)?;
        let writer = BufWriter::new(file);

        Ok(Self { output_path, writer, start_time })
    }

    /// Write a layout event with a timestamp.
    ///
    /// Timestamp is relative to recording start (same base as .cast files).
    pub fn write_event(&mut self, event: LayoutEvent) -> Result<()> {
        let elapsed = self.start_time.elapsed();
        let timestamp = elapsed.as_secs_f64();

        let timestamped = TimestampedEvent { time: timestamp, event };

        let json = serde_json::to_string(&timestamped)?;
        writeln!(self.writer, "{json}")?;
        self.writer.flush()?;

        Ok(())
    }

    /// Query current layout from tmux backend and write a `LayoutSnapshot` event.
    ///
    /// This should be called at recording start and periodically (e.g., every 5s)
    /// to create keyframes for seeking.
    ///
    /// # Arguments
    /// * `session_id` - Tmux session ID or name to query
    pub async fn snapshot(&mut self, session_id: &str) -> Result<()> {
        let panes = Self::query_tmux_layout(session_id).await?;
        self.write_event(LayoutEvent::LayoutSnapshot { panes })
    }

    /// Query tmux for current layout.
    ///
    /// Uses: `tmux list-panes -s -t <session> -F '#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height} #{window_index}'`
    async fn query_tmux_layout(session_id: &str) -> Result<Vec<PaneGeometry>> {
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-s",
                "-t",
                session_id,
                "-F",
                "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height} #{window_index}",
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LayoutError::TmuxFailed(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut panes = Vec::new();

        for line in stdout.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 6 {
                let pane = PaneGeometry {
                    pane_id: fields[0].to_string(),
                    x: fields[1].parse().unwrap_or(0),
                    y: fields[2].parse().unwrap_or(0),
                    width: fields[3].parse().unwrap_or(80),
                    height: fields[4].parse().unwrap_or(24),
                    tab_index: fields[5].parse().unwrap_or(0),
                };
                panes.push(pane);
            }
        }

        Ok(panes)
    }

    /// Flush and close the journal.
    pub fn close(mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    /// Get the path to the layout file.
    pub fn path(&self) -> &Path {
        &self.output_path
    }
}

/// Reader for layout event journal.
///
/// Reads timestamped layout events from a JSONL file.
pub struct LayoutJournalReader {
    path: PathBuf,
}

impl LayoutJournalReader {
    /// Open a layout journal for reading.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(LayoutError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Layout file not found: {}", path.display()),
            )));
        }
        Ok(Self { path })
    }

    /// Get an iterator over all events with their timestamps.
    pub fn events(&self) -> Result<impl Iterator<Item = (f64, LayoutEvent)>> {
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);

        let events: Result<Vec<(f64, LayoutEvent)>> = reader
            .lines()
            .filter_map(std::result::Result::ok)
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let timestamped: TimestampedEvent = serde_json::from_str(&line)?;
                Ok((timestamped.time, timestamped.event))
            })
            .collect();

        Ok(events?.into_iter())
    }

    /// Reconstruct the layout state at a given time.
    ///
    /// This replays events from the most recent snapshot before `time`,
    /// applying all subsequent events up to `time`.
    ///
    /// Returns the list of panes that exist at that time with their geometry.
    pub fn layout_at(&self, time: f64) -> Result<Vec<PaneGeometry>> {
        let all_events: Vec<(f64, LayoutEvent)> = self.events()?.collect();

        // Find the most recent snapshot before or at the target time
        let mut snapshot_idx = None;
        for (i, (event_time, event)) in all_events.iter().enumerate() {
            if *event_time > time {
                break;
            }
            if matches!(event, LayoutEvent::LayoutSnapshot { .. }) {
                snapshot_idx = Some(i);
            }
        }

        // Start from snapshot or empty state
        let mut layout: HashMap<String, PaneGeometry> = HashMap::new();
        let start_idx = if let Some(idx) = snapshot_idx {
            if let (_, LayoutEvent::LayoutSnapshot { panes }) = &all_events[idx] {
                for pane in panes {
                    layout.insert(pane.pane_id.clone(), pane.clone());
                }
            }
            idx + 1
        } else {
            0
        };

        // Replay events from snapshot to target time
        for (event_time, event) in all_events.iter().skip(start_idx) {
            if *event_time > time {
                break;
            }

            match event {
                LayoutEvent::PaneOpened { pane_id, x, y, width, height, tab_index } => {
                    layout.insert(
                        pane_id.clone(),
                        PaneGeometry {
                            pane_id: pane_id.clone(),
                            x: *x,
                            y: *y,
                            width: *width,
                            height: *height,
                            tab_index: *tab_index,
                        },
                    );
                },
                LayoutEvent::PaneClosed { pane_id } => {
                    layout.remove(pane_id);
                },
                LayoutEvent::PaneResized { pane_id, x, y, width, height } => {
                    if let Some(pane) = layout.get_mut(pane_id) {
                        pane.x = *x;
                        pane.y = *y;
                        pane.width = *width;
                        pane.height = *height;
                    }
                },
                LayoutEvent::LayoutSnapshot { panes } => {
                    // Replace entire state with snapshot
                    layout.clear();
                    for pane in panes {
                        layout.insert(pane.pane_id.clone(), pane.clone());
                    }
                },
                LayoutEvent::TabSwitched { .. } => {
                    // Tab switches don't affect geometry
                },
            }
        }

        let mut result: Vec<PaneGeometry> = layout.into_values().collect();
        result.sort_by_key(|p| p.pane_id.clone());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_pane_geometry_serialization() {
        let geom = PaneGeometry {
            pane_id: "%5".to_string(),
            x: 0,
            y: 0,
            width: 80,
            height: 24,
            tab_index: 0,
        };

        let json = serde_json::to_string(&geom).unwrap();
        let parsed: PaneGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(geom, parsed);
    }

    #[test]
    fn test_layout_event_serialization() {
        let events = vec![
            LayoutEvent::PaneOpened {
                pane_id: "%5".to_string(),
                x: 0,
                y: 0,
                width: 80,
                height: 24,
                tab_index: 0,
            },
            LayoutEvent::PaneClosed { pane_id: "%5".to_string() },
            LayoutEvent::PaneResized {
                pane_id: "%6".to_string(),
                x: 10,
                y: 5,
                width: 60,
                height: 20,
            },
            LayoutEvent::TabSwitched { tab_index: 1 },
            LayoutEvent::LayoutSnapshot { panes: vec![] },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let parsed: LayoutEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, parsed);
        }
    }

    #[test]
    fn test_timestamped_event_serialization() {
        let event = TimestampedEvent {
            time: 1.5,
            event: LayoutEvent::PaneOpened {
                pane_id: "%5".to_string(),
                x: 0,
                y: 0,
                width: 80,
                height: 24,
                tab_index: 0,
            },
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"time\":1.5"));
        assert!(json.contains("\"type\":\"pane_opened\""));

        let parsed: TimestampedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.time, 1.5);
    }

    #[test]
    fn test_journal_write_read() {
        let temp_dir = std::env::temp_dir();
        let output_dir = temp_dir.join("layout_test_write_read");
        std::fs::create_dir_all(&output_dir).ok();

        let start_time = Instant::now();

        // Write events
        {
            let mut journal = LayoutJournal::new(&output_dir, start_time).unwrap();

            journal
                .write_event(LayoutEvent::PaneOpened {
                    pane_id: "%5".to_string(),
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24,
                    tab_index: 0,
                })
                .unwrap();

            std::thread::sleep(Duration::from_millis(100));

            journal
                .write_event(LayoutEvent::PaneResized {
                    pane_id: "%5".to_string(),
                    x: 0,
                    y: 0,
                    width: 120,
                    height: 40,
                })
                .unwrap();

            journal.close().unwrap();
        }

        // Read events
        {
            let layout_path = output_dir.join("layout.jsonl");
            let reader = LayoutJournalReader::open(&layout_path).unwrap();
            let events: Vec<(f64, LayoutEvent)> = reader.events().unwrap().collect();

            assert_eq!(events.len(), 2);

            // First event should be near 0
            assert!(events[0].0 < 0.1);
            assert!(matches!(events[0].1, LayoutEvent::PaneOpened { .. }));

            // Second event should be ~100ms later
            assert!(events[1].0 >= 0.05);
            assert!(matches!(events[1].1, LayoutEvent::PaneResized { .. }));
        }

        // Cleanup
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn test_layout_at_basic() {
        let temp_dir = std::env::temp_dir();
        let output_dir = temp_dir.join("layout_test_layout_at");
        std::fs::create_dir_all(&output_dir).ok();

        let start_time = Instant::now();

        // Write a sequence of events
        {
            let mut journal = LayoutJournal::new(&output_dir, start_time).unwrap();

            // t=0: snapshot with one pane
            journal
                .write_event(LayoutEvent::LayoutSnapshot {
                    panes: vec![PaneGeometry {
                        pane_id: "%1".to_string(),
                        x: 0,
                        y: 0,
                        width: 80,
                        height: 24,
                        tab_index: 0,
                    }],
                })
                .unwrap();

            // t=1: open second pane
            std::thread::sleep(Duration::from_millis(50));
            journal
                .write_event(LayoutEvent::PaneOpened {
                    pane_id: "%2".to_string(),
                    x: 80,
                    y: 0,
                    width: 40,
                    height: 24,
                    tab_index: 0,
                })
                .unwrap();

            // t=2: resize first pane
            std::thread::sleep(Duration::from_millis(50));
            journal
                .write_event(LayoutEvent::PaneResized {
                    pane_id: "%1".to_string(),
                    x: 0,
                    y: 0,
                    width: 60,
                    height: 24,
                })
                .unwrap();

            // t=3: close second pane
            std::thread::sleep(Duration::from_millis(50));
            journal
                .write_event(LayoutEvent::PaneClosed { pane_id: "%2".to_string() })
                .unwrap();

            journal.close().unwrap();
        }

        // Read and reconstruct layout at various times
        {
            let layout_path = output_dir.join("layout.jsonl");
            let reader = LayoutJournalReader::open(&layout_path).unwrap();

            // At t=0: should have 1 pane (80x24)
            let layout = reader.layout_at(0.01).unwrap();
            assert_eq!(layout.len(), 1);
            assert_eq!(layout[0].pane_id, "%1");
            assert_eq!(layout[0].width, 80);

            // At t=1.5: should have 2 panes
            let layout = reader.layout_at(0.08).unwrap();
            assert_eq!(layout.len(), 2);

            // At t=2.5: should have 2 panes, first resized to 60
            let layout = reader.layout_at(0.12).unwrap();
            assert_eq!(layout.len(), 2);
            let pane1 = layout.iter().find(|p| p.pane_id == "%1").unwrap();
            assert_eq!(pane1.width, 60);

            // At t=4: should have 1 pane (second closed)
            let layout = reader.layout_at(0.20).unwrap();
            assert_eq!(layout.len(), 1);
            assert_eq!(layout[0].pane_id, "%1");
        }

        // Cleanup
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn test_layout_at_multiple_snapshots() {
        let temp_dir = std::env::temp_dir();
        let output_dir = temp_dir.join("layout_test_multiple_snapshots");
        std::fs::create_dir_all(&output_dir).ok();

        let start_time = Instant::now();

        // Write events with multiple snapshots
        {
            let mut journal = LayoutJournal::new(&output_dir, start_time).unwrap();

            // Snapshot 1: one pane
            journal
                .write_event(LayoutEvent::LayoutSnapshot {
                    panes: vec![PaneGeometry {
                        pane_id: "%1".to_string(),
                        x: 0,
                        y: 0,
                        width: 80,
                        height: 24,
                        tab_index: 0,
                    }],
                })
                .unwrap();

            std::thread::sleep(Duration::from_millis(50));

            // Snapshot 2: two panes (completely new state)
            journal
                .write_event(LayoutEvent::LayoutSnapshot {
                    panes: vec![
                        PaneGeometry {
                            pane_id: "%3".to_string(),
                            x: 0,
                            y: 0,
                            width: 40,
                            height: 24,
                            tab_index: 0,
                        },
                        PaneGeometry {
                            pane_id: "%4".to_string(),
                            x: 40,
                            y: 0,
                            width: 40,
                            height: 24,
                            tab_index: 0,
                        },
                    ],
                })
                .unwrap();

            std::thread::sleep(Duration::from_millis(50));

            // Event after second snapshot
            journal
                .write_event(LayoutEvent::PaneClosed { pane_id: "%4".to_string() })
                .unwrap();

            journal.close().unwrap();
        }

        // Read and check that second snapshot replaces state
        {
            let layout_path = output_dir.join("layout.jsonl");
            let reader = LayoutJournalReader::open(&layout_path).unwrap();

            // After second snapshot, should have panes %3 and %4, not %1
            let layout = reader.layout_at(0.08).unwrap();
            assert_eq!(layout.len(), 2);
            assert!(layout.iter().any(|p| p.pane_id == "%3"));
            assert!(layout.iter().any(|p| p.pane_id == "%4"));
            assert!(!layout.iter().any(|p| p.pane_id == "%1"));

            // After close event, should only have %3
            let layout = reader.layout_at(0.15).unwrap();
            assert_eq!(layout.len(), 1);
            assert_eq!(layout[0].pane_id, "%3");
        }

        // Cleanup
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn test_layout_at_empty() {
        let temp_dir = std::env::temp_dir();
        let output_dir = temp_dir.join("layout_test_empty");
        std::fs::create_dir_all(&output_dir).ok();

        let start_time = Instant::now();

        // Write empty snapshot
        {
            let mut journal = LayoutJournal::new(&output_dir, start_time).unwrap();
            journal.write_event(LayoutEvent::LayoutSnapshot { panes: vec![] }).unwrap();
            journal.close().unwrap();
        }

        // Read and check empty layout
        {
            let layout_path = output_dir.join("layout.jsonl");
            let reader = LayoutJournalReader::open(&layout_path).unwrap();
            let layout = reader.layout_at(0.0).unwrap();
            assert_eq!(layout.len(), 0);
        }

        // Cleanup
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn test_journal_nonexistent_file() {
        let temp_dir = std::env::temp_dir();
        let nonexistent = temp_dir.join("nonexistent_layout.jsonl");

        let result = LayoutJournalReader::open(&nonexistent);
        assert!(matches!(result, Err(LayoutError::Io(_))));
    }

    #[test]
    fn test_tab_switched_event() {
        let event = LayoutEvent::TabSwitched { tab_index: 2 };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: LayoutEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);

        // Tab switch shouldn't affect layout reconstruction
        let temp_dir = std::env::temp_dir();
        let output_dir = temp_dir.join("layout_test_tab_switch");
        std::fs::create_dir_all(&output_dir).ok();

        let start_time = Instant::now();

        {
            let mut journal = LayoutJournal::new(&output_dir, start_time).unwrap();
            journal
                .write_event(LayoutEvent::LayoutSnapshot {
                    panes: vec![PaneGeometry {
                        pane_id: "%1".to_string(),
                        x: 0,
                        y: 0,
                        width: 80,
                        height: 24,
                        tab_index: 0,
                    }],
                })
                .unwrap();
            journal.write_event(LayoutEvent::TabSwitched { tab_index: 1 }).unwrap();
            journal.close().unwrap();
        }

        {
            let layout_path = output_dir.join("layout.jsonl");
            let reader = LayoutJournalReader::open(&layout_path).unwrap();
            let layout = reader.layout_at(1.0).unwrap();
            assert_eq!(layout.len(), 1); // Tab switch doesn't remove panes
        }

        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn test_full_recording_workflow() {
        // This test simulates a full recording session workflow
        let temp_dir = std::env::temp_dir();
        let output_dir = temp_dir.join("layout_test_full_workflow");
        std::fs::create_dir_all(&output_dir).ok();

        let start_time = Instant::now();

        // Simulate recording session
        {
            let mut journal = LayoutJournal::new(&output_dir, start_time).unwrap();

            // Initial snapshot (recording start)
            journal
                .write_event(LayoutEvent::LayoutSnapshot {
                    panes: vec![PaneGeometry {
                        pane_id: "%1".to_string(),
                        x: 0,
                        y: 0,
                        width: 80,
                        height: 24,
                        tab_index: 0,
                    }],
                })
                .unwrap();

            std::thread::sleep(Duration::from_millis(10));

            // User opens a new pane
            journal
                .write_event(LayoutEvent::PaneOpened {
                    pane_id: "%2".to_string(),
                    x: 40,
                    y: 0,
                    width: 40,
                    height: 24,
                    tab_index: 0,
                })
                .unwrap();

            // First pane resizes as a result
            journal
                .write_event(LayoutEvent::PaneResized {
                    pane_id: "%1".to_string(),
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 24,
                })
                .unwrap();

            std::thread::sleep(Duration::from_millis(10));

            // Periodic snapshot (keyframe at ~20ms)
            journal
                .write_event(LayoutEvent::LayoutSnapshot {
                    panes: vec![
                        PaneGeometry {
                            pane_id: "%1".to_string(),
                            x: 0,
                            y: 0,
                            width: 40,
                            height: 24,
                            tab_index: 0,
                        },
                        PaneGeometry {
                            pane_id: "%2".to_string(),
                            x: 40,
                            y: 0,
                            width: 40,
                            height: 24,
                            tab_index: 0,
                        },
                    ],
                })
                .unwrap();

            std::thread::sleep(Duration::from_millis(10));

            // User closes second pane
            journal
                .write_event(LayoutEvent::PaneClosed { pane_id: "%2".to_string() })
                .unwrap();

            // First pane expands back
            journal
                .write_event(LayoutEvent::PaneResized {
                    pane_id: "%1".to_string(),
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24,
                })
                .unwrap();

            journal.close().unwrap();
        }

        // Playback: reconstruct layout at different points
        {
            let layout_path = output_dir.join("layout.jsonl");
            let reader = LayoutJournalReader::open(&layout_path).unwrap();

            // At start: 1 pane (80x24)
            let layout = reader.layout_at(0.001).unwrap();
            assert_eq!(layout.len(), 1);
            assert_eq!(layout[0].width, 80);

            // After split: 2 panes (both 40 wide)
            let layout = reader.layout_at(0.015).unwrap();
            assert_eq!(layout.len(), 2);
            assert!(layout.iter().all(|p| p.width == 40));

            // After close: 1 pane (80 wide again)
            let layout = reader.layout_at(0.035).unwrap();
            assert_eq!(layout.len(), 1);
            assert_eq!(layout[0].width, 80);

            // Verify keyframe seeking: reading from keyframe at t=0.02
            // should still give us correct state at t=0.035
            let layout = reader.layout_at(0.035).unwrap();
            assert_eq!(layout.len(), 1);
            assert_eq!(layout[0].pane_id, "%1");
        }

        // Cleanup
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn test_journal_path() {
        let temp_dir = std::env::temp_dir();
        let output_dir = temp_dir.join("layout_test_path");
        std::fs::create_dir_all(&output_dir).ok();

        let start_time = Instant::now();
        let journal = LayoutJournal::new(&output_dir, start_time).unwrap();
        let expected_path = output_dir.join("layout.jsonl");

        assert_eq!(journal.path(), expected_path.as_path());

        std::fs::remove_dir_all(&output_dir).ok();
    }
}
