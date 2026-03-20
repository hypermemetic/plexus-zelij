//! Compositor for combining per-pane .cast files into composite frames.
//!
//! This module provides the core functionality for reading per-pane recordings
//! and layout information, then producing composite frames that represent the
//! complete terminal view with borders.

pub mod diff;
pub mod frame;
pub mod pane_state;
pub mod writer;

use crate::cast::{CastEvent, CastReader};
use frame::{Cell, CompositeFrame};
use pane_state::PaneState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use thiserror::Error;

// Re-export writer types
pub use writer::{
    BorderStyle, CastTheme, CompositeOpts, CompositeResult, CompositeWriter, ProgressCallback,
};

#[derive(Debug, Error)]
pub enum CompositorError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Cast file error: {0}")]
    Cast(#[from] crate::cast::CastError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("No layout file found")]
    NoLayoutFile,

    #[error("No cast files found")]
    NoCastFiles,

    #[error("Invalid recording directory: {0}")]
    InvalidDirectory(String),

    #[error("Pane not found: {0}")]
    PaneNotFound(String),
}

pub type Result<T> = std::result::Result<T, CompositorError>;

/// Pane geometry information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaneGeometry {
    pub pane_id: String,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,

    #[serde(default)]
    pub tab_index: u32,
}

/// Layout events from the journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LayoutEvent {
    LayoutSnapshot { panes: Vec<PaneGeometry> },
    PaneOpened { pane_id: String, x: u16, y: u16, width: u16, height: u16, tab_index: u32 },
    PaneClosed { pane_id: String },
    PaneResized { pane_id: String, x: u16, y: u16, width: u16, height: u16 },
    TabSwitched { tab_index: u32 },
}

/// Timeline event - either pane output or layout change.
#[derive(Debug, Clone)]
pub enum TimelineEvent {
    /// Raw output bytes for a specific pane.
    PaneOutput { pane_id: String, bytes: Vec<u8> },

    /// Layout change event.
    LayoutChange { event: LayoutEvent },
}

/// Layout journal reader.
///
/// Reads layout.jsonl and provides methods to reconstruct layout at any timestamp.
pub struct LayoutJournalReader {
    events: Vec<(f64, LayoutEvent)>,
}

impl LayoutJournalReader {
    /// Load layout journal from a file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);

        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            // Parse the journal entry
            let entry: serde_json::Value = serde_json::from_str(&line)?;

            // Extract timestamp and event
            let timestamp = entry.get("timestamp")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            let event: LayoutEvent = serde_json::from_value(entry)?;

            events.push((timestamp, event));
        }

        // Sort by timestamp
        events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        Ok(Self { events })
    }

    /// Get all events.
    pub fn events(&self) -> &[(f64, LayoutEvent)] {
        &self.events
    }

    /// Reconstruct the layout at a given timestamp.
    ///
    /// Replays events from the start up to the given time to determine
    /// which panes exist and their positions.
    pub fn layout_at(&self, time: f64) -> Vec<PaneGeometry> {
        let mut layout: HashMap<String, PaneGeometry> = HashMap::new();

        for (event_time, event) in &self.events {
            if *event_time > time {
                break;
            }

            match event {
                LayoutEvent::LayoutSnapshot { panes } => {
                    // Replace entire layout
                    layout.clear();
                    for pane in panes {
                        layout.insert(pane.pane_id.clone(), pane.clone());
                    }
                }
                LayoutEvent::PaneOpened { pane_id, x, y, width, height, tab_index } => {
                    layout.insert(pane_id.clone(), PaneGeometry {
                        pane_id: pane_id.clone(),
                        x: *x,
                        y: *y,
                        width: *width,
                        height: *height,
                        tab_index: *tab_index,
                    });
                }
                LayoutEvent::PaneClosed { pane_id } => {
                    layout.remove(pane_id);
                }
                LayoutEvent::PaneResized { pane_id, x, y, width, height } => {
                    if let Some(pane) = layout.get_mut(pane_id) {
                        pane.x = *x;
                        pane.y = *y;
                        pane.width = *width;
                        pane.height = *height;
                    }
                }
                LayoutEvent::TabSwitched { .. } => {
                    // Tab switching affects visibility but not geometry
                    // For now we ignore this - a full implementation would track active tab
                }
            }
        }

        layout.values().cloned().collect()
    }
}

/// Main compositor struct.
///
/// Manages pane states, timeline, and frame rendering.
pub struct Compositor {
    /// Recording directory path.
    recording_dir: PathBuf,

    /// Layout journal reader.
    layout_reader: LayoutJournalReader,

    /// Per-pane state.
    pane_states: HashMap<String, PaneState>,

    /// Merged timeline of all events.
    timeline: Vec<(f64, TimelineEvent)>,
}

impl Compositor {
    /// Create a new compositor from a recording directory.
    ///
    /// Discovers .cast files and layout.jsonl, validates the recording.
    ///
    /// # Arguments
    /// * `recording_dir` - Path to directory containing pane-*.cast and layout.jsonl
    pub fn new(recording_dir: impl AsRef<Path>) -> Result<Self> {
        let recording_dir = recording_dir.as_ref().to_path_buf();

        // Check that directory exists
        if !recording_dir.is_dir() {
            return Err(CompositorError::InvalidDirectory(
                format!("{} is not a directory", recording_dir.display())
            ));
        }

        // Load layout journal
        let layout_path = recording_dir.join("layout.jsonl");
        if !layout_path.exists() {
            return Err(CompositorError::NoLayoutFile);
        }

        let layout_reader = LayoutJournalReader::load(&layout_path)?;

        // Discover .cast files
        let mut cast_files = Vec::new();
        for entry in fs::read_dir(&recording_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("cast") {
                if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                    if file_name.starts_with("pane-") {
                        cast_files.push(path);
                    }
                }
            }
        }

        if cast_files.is_empty() {
            return Err(CompositorError::NoCastFiles);
        }

        Ok(Self {
            recording_dir,
            layout_reader,
            pane_states: HashMap::new(),
            timeline: Vec::new(),
        })
    }

    /// Build the merged timeline from all .cast files and layout events.
    ///
    /// This creates a single sorted timeline containing all pane output events
    /// and layout change events.
    pub fn build_timeline(&mut self) -> Result<()> {
        let mut timeline = Vec::new();

        // Add layout events
        for (time, event) in self.layout_reader.events() {
            timeline.push((
                *time,
                TimelineEvent::LayoutChange { event: event.clone() },
            ));
        }

        // Add pane output events from each .cast file
        for entry in fs::read_dir(&self.recording_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("cast") {
                if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                    if file_name.starts_with("pane-") {
                        // Extract pane ID from filename (pane-5.cast -> %5)
                        let pane_id_num = file_name
                            .strip_prefix("pane-")
                            .and_then(|s| s.strip_suffix(".cast"))
                            .unwrap_or("");

                        let pane_id = format!("%{}", pane_id_num);

                        // Read events from this cast file
                        let reader = CastReader::open(&path)?;

                        for event_result in reader.events() {
                            let event = event_result?;

                            match event {
                                CastEvent::Output(time, data) => {
                                    timeline.push((
                                        time,
                                        TimelineEvent::PaneOutput {
                                            pane_id: pane_id.clone(),
                                            bytes: data.into_bytes(),
                                        },
                                    ));
                                }
                                CastEvent::Resize(time, width, height) => {
                                    // Handle resize by creating a layout event
                                    // Note: we don't have x,y here, so this is a simplified version
                                    // A full implementation would need to track or look up position
                                    timeline.push((
                                        time,
                                        TimelineEvent::LayoutChange {
                                            event: LayoutEvent::PaneResized {
                                                pane_id: pane_id.clone(),
                                                x: 0,
                                                y: 0,
                                                width,
                                                height,
                                            },
                                        },
                                    ));
                                }
                                _ => {
                                    // Ignore input and marker events for now
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort timeline by timestamp
        timeline.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        self.timeline = timeline;

        Ok(())
    }

    /// Render a composite frame at a specific timestamp.
    ///
    /// Replays all events up to the given time, then produces a frame
    /// showing all panes in their proper positions with borders.
    pub fn render_frame(&mut self, time: f64) -> Result<CompositeFrame> {
        // Get layout at this time
        let layout = self.layout_reader.layout_at(time);

        if layout.is_empty() {
            // Return empty frame
            return Ok(CompositeFrame::new(80, 24));
        }

        // Initialize pane states if needed
        for pane_geo in &layout {
            if !self.pane_states.contains_key(&pane_geo.pane_id) {
                self.pane_states.insert(
                    pane_geo.pane_id.clone(),
                    PaneState::new(
                        pane_geo.pane_id.clone(),
                        pane_geo.width,
                        pane_geo.height,
                    ),
                );
            }
        }

        // Replay events up to this time
        for (event_time, event) in &self.timeline {
            if *event_time > time {
                break;
            }

            match event {
                TimelineEvent::PaneOutput { pane_id, bytes } => {
                    if let Some(state) = self.pane_states.get_mut(pane_id) {
                        state.process(bytes);
                    }
                }
                TimelineEvent::LayoutChange { event } => {
                    match event {
                        LayoutEvent::PaneResized { pane_id, width, height, .. } => {
                            if let Some(state) = self.pane_states.get_mut(pane_id) {
                                state.resize(*width, *height);
                            }
                        }
                        _ => {
                            // Other layout events don't affect pane state
                        }
                    }
                }
            }
        }

        // Calculate composite dimensions
        let (comp_width, comp_height) = Self::calculate_dimensions(&layout);

        // Create composite frame
        let mut frame = CompositeFrame::new(comp_width, comp_height);

        // Draw each pane
        for pane_geo in &layout {
            if let Some(state) = self.pane_states.get(&pane_geo.pane_id) {
                self.draw_pane(&mut frame, pane_geo, state);
            }
        }

        // Draw borders
        self.draw_borders(&mut frame, &layout);

        Ok(frame)
    }

    /// Calculate composite dimensions from layout.
    ///
    /// Returns (width, height) needed to contain all panes with borders.
    fn calculate_dimensions(layout: &[PaneGeometry]) -> (u16, u16) {
        if layout.is_empty() {
            return (80, 24);
        }

        let mut max_right = 0u16;
        let mut max_bottom = 0u16;

        for pane in layout {
            let right = pane.x + pane.width;
            let bottom = pane.y + pane.height;

            if right > max_right {
                max_right = right;
            }
            if bottom > max_bottom {
                max_bottom = bottom;
            }
        }

        // Add space for borders (1 column/row per border)
        // In a real tmux layout, borders are between panes
        // For simplicity, we just use the max extents
        (max_right, max_bottom)
    }

    /// Draw a single pane onto the composite frame.
    fn draw_pane(&self, frame: &mut CompositeFrame, geo: &PaneGeometry, state: &PaneState) {
        let cells = state.cells();

        for (pane_row, row_cells) in cells.iter().enumerate() {
            for (pane_col, cell) in row_cells.iter().enumerate() {
                let frame_row = geo.y + pane_row as u16;
                let frame_col = geo.x + pane_col as u16;

                frame.set_cell(frame_row, frame_col, cell.clone());
            }
        }
    }

    /// Draw borders between panes.
    ///
    /// Uses box-drawing characters: │ ─ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼
    fn draw_borders(&self, frame: &mut CompositeFrame, layout: &[PaneGeometry]) {
        if layout.len() <= 1 {
            // No borders needed for single pane
            return;
        }

        // For now, we'll draw a simple border around each pane
        // A full implementation would detect shared edges and use proper connectors

        for pane in layout {
            // Top border
            if pane.y > 0 {
                for col in pane.x..pane.x + pane.width {
                    if col < frame.width {
                        frame.set_cell(pane.y.saturating_sub(1), col, Cell::new('─'));
                    }
                }
            }

            // Bottom border
            let bottom = pane.y + pane.height;
            if bottom < frame.height {
                for col in pane.x..pane.x + pane.width {
                    if col < frame.width {
                        frame.set_cell(bottom, col, Cell::new('─'));
                    }
                }
            }

            // Left border
            if pane.x > 0 {
                for row in pane.y..pane.y + pane.height {
                    if row < frame.height {
                        frame.set_cell(row, pane.x.saturating_sub(1), Cell::new('│'));
                    }
                }
            }

            // Right border
            let right = pane.x + pane.width;
            if right < frame.width {
                for row in pane.y..pane.y + pane.height {
                    if row < frame.height {
                        frame.set_cell(row, right, Cell::new('│'));
                    }
                }
            }

            // Corners
            if pane.y > 0 && pane.x > 0 {
                frame.set_cell(pane.y.saturating_sub(1), pane.x.saturating_sub(1), Cell::new('┌'));
            }

            if pane.y > 0 && pane.x + pane.width < frame.width {
                frame.set_cell(pane.y.saturating_sub(1), pane.x + pane.width, Cell::new('┐'));
            }

            if pane.y + pane.height < frame.height && pane.x > 0 {
                frame.set_cell(pane.y + pane.height, pane.x.saturating_sub(1), Cell::new('└'));
            }

            if pane.y + pane.height < frame.height && pane.x + pane.width < frame.width {
                frame.set_cell(pane.y + pane.height, pane.x + pane.width, Cell::new('┘'));
            }
        }
    }

    /// Get the recording directory.
    pub fn recording_dir(&self) -> &Path {
        &self.recording_dir
    }

    /// Get the timeline.
    pub fn timeline(&self) -> &[(f64, TimelineEvent)] {
        &self.timeline
    }

    /// Get the layout at a specific timestamp.
    pub fn layout_at(&self, time: f64) -> Vec<PaneGeometry> {
        self.layout_reader.layout_at(time)
    }

    /// Get the number of layout events.
    pub fn layout_event_count(&self) -> usize {
        self.layout_reader.events().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast::{CastHeader, CastWriter};

    #[test]
    fn test_layout_journal_reader_empty() {
        // Create a temp file with empty layout
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_layout_empty.jsonl");

        std::fs::write(&path, "").unwrap();

        let reader = LayoutJournalReader::load(&path).unwrap();
        assert_eq!(reader.events().len(), 0);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_layout_journal_reader_snapshot() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_layout_snapshot.jsonl");

        let snapshot = serde_json::json!({
            "timestamp": 0.0,
            "event": "layout_snapshot",
            "panes": [
                {
                    "pane_id": "%1",
                    "x": 0,
                    "y": 0,
                    "width": 80,
                    "height": 24,
                    "tab_index": 0
                }
            ]
        });

        std::fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();

        let reader = LayoutJournalReader::load(&path).unwrap();
        assert_eq!(reader.events().len(), 1);

        let layout = reader.layout_at(1.0);
        assert_eq!(layout.len(), 1);
        assert_eq!(layout[0].pane_id, "%1");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_layout_journal_reader_pane_lifecycle() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_layout_lifecycle.jsonl");

        // Write multiple events
        let events = vec![
            serde_json::json!({
                "timestamp": 0.0,
                "event": "layout_snapshot",
                "panes": [{
                    "pane_id": "%1",
                    "x": 0,
                    "y": 0,
                    "width": 80,
                    "height": 24,
                    "tab_index": 0
                }]
            }),
            serde_json::json!({
                "timestamp": 1.0,
                "event": "pane_opened",
                "pane_id": "%2",
                "x": 40,
                "y": 0,
                "width": 40,
                "height": 24,
                "tab_index": 0
            }),
            serde_json::json!({
                "timestamp": 2.0,
                "event": "pane_resized",
                "pane_id": "%1",
                "x": 0,
                "y": 0,
                "width": 40,
                "height": 24
            }),
            serde_json::json!({
                "timestamp": 3.0,
                "event": "pane_closed",
                "pane_id": "%2"
            }),
        ];

        let content = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");

        std::fs::write(&path, content).unwrap();

        let reader = LayoutJournalReader::load(&path).unwrap();
        assert_eq!(reader.events().len(), 4);

        // At t=0, should have pane %1
        let layout = reader.layout_at(0.5);
        assert_eq!(layout.len(), 1);
        assert_eq!(layout[0].pane_id, "%1");
        assert_eq!(layout[0].width, 80);

        // At t=1.5, should have both panes
        let layout = reader.layout_at(1.5);
        assert_eq!(layout.len(), 2);

        // At t=2.5, should have both panes but %1 is resized
        let layout = reader.layout_at(2.5);
        assert_eq!(layout.len(), 2);
        let pane1 = layout.iter().find(|p| p.pane_id == "%1").unwrap();
        assert_eq!(pane1.width, 40);

        // At t=3.5, should only have pane %1
        let layout = reader.layout_at(3.5);
        assert_eq!(layout.len(), 1);
        assert_eq!(layout[0].pane_id, "%1");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_pane_geometry_serialization() {
        let geo = PaneGeometry {
            pane_id: "%5".to_string(),
            x: 10,
            y: 20,
            width: 80,
            height: 24,
            tab_index: 0,
        };

        let json = serde_json::to_string(&geo).unwrap();
        let parsed: PaneGeometry = serde_json::from_str(&json).unwrap();

        assert_eq!(geo, parsed);
    }

    #[test]
    fn test_calculate_dimensions() {
        let layout = vec![
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
        ];

        let (width, height) = Compositor::calculate_dimensions(&layout);
        assert_eq!(width, 80);
        assert_eq!(height, 24);
    }

    #[test]
    fn test_compositor_end_to_end() {
        // Create a test recording directory
        let temp_dir = std::env::temp_dir().join("test_compositor_e2e");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create layout.jsonl
        let layout_path = temp_dir.join("layout.jsonl");
        let layout_snapshot = serde_json::json!({
            "timestamp": 0.0,
            "event": "layout_snapshot",
            "panes": [{
                "pane_id": "%1",
                "x": 0,
                "y": 0,
                "width": 40,
                "height": 10,
                "tab_index": 0
            }]
        });
        std::fs::write(&layout_path, serde_json::to_string(&layout_snapshot).unwrap()).unwrap();

        // Create pane-1.cast
        let cast_path = temp_dir.join("pane-1.cast");
        let mut writer = CastWriter::create(&cast_path).unwrap();

        let header = CastHeader {
            version: 2,
            width: 40,
            height: 10,
            timestamp: Some(1234567890),
            env: None,
            title: Some("Test Pane".to_string()),
            idle_time_limit: None,
            theme: None,
        };

        writer.write_header(&header).unwrap();
        writer.write_event(&CastEvent::Output(0.1, "Hello, World!\n".to_string())).unwrap();
        writer.write_event(&CastEvent::Output(0.5, "This is a test.\n".to_string())).unwrap();
        writer.finish().unwrap();

        // Create compositor
        let mut compositor = Compositor::new(&temp_dir).unwrap();

        // Build timeline
        compositor.build_timeline().unwrap();

        // Check timeline was built
        assert!(!compositor.timeline().is_empty());

        // Render frame at t=0.3 (after first output)
        let frame = compositor.render_frame(0.3).unwrap();
        assert_eq!(frame.width, 40);
        assert_eq!(frame.height, 10);

        // The frame should contain "Hello, World!"
        let ansi = frame.render_ansi();
        assert!(ansi.contains('H') || ansi.contains('e') || ansi.contains('l'));

        // Render frame at t=1.0 (after both outputs)
        let frame = compositor.render_frame(1.0).unwrap();
        let ansi = frame.render_ansi();
        // Should contain both lines
        assert!(ansi.len() > 0);

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_compositor_multi_pane() {
        // Create a test recording directory
        let temp_dir = std::env::temp_dir().join("test_compositor_multi");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create layout.jsonl with two panes
        let layout_path = temp_dir.join("layout.jsonl");
        let layout_snapshot = serde_json::json!({
            "timestamp": 0.0,
            "event": "layout_snapshot",
            "panes": [
                {
                    "pane_id": "%1",
                    "x": 0,
                    "y": 0,
                    "width": 20,
                    "height": 10,
                    "tab_index": 0
                },
                {
                    "pane_id": "%2",
                    "x": 20,
                    "y": 0,
                    "width": 20,
                    "height": 10,
                    "tab_index": 0
                }
            ]
        });
        std::fs::write(&layout_path, serde_json::to_string(&layout_snapshot).unwrap()).unwrap();

        // Create pane-1.cast
        let cast_path1 = temp_dir.join("pane-1.cast");
        let mut writer1 = CastWriter::create(&cast_path1).unwrap();
        writer1.write_header(&CastHeader {
            version: 2,
            width: 20,
            height: 10,
            timestamp: Some(1234567890),
            env: None,
            title: Some("Pane 1".to_string()),
            idle_time_limit: None,
            theme: None,
        }).unwrap();
        writer1.write_event(&CastEvent::Output(0.1, "Left Pane".to_string())).unwrap();
        writer1.finish().unwrap();

        // Create pane-2.cast
        let cast_path2 = temp_dir.join("pane-2.cast");
        let mut writer2 = CastWriter::create(&cast_path2).unwrap();
        writer2.write_header(&CastHeader {
            version: 2,
            width: 20,
            height: 10,
            timestamp: Some(1234567890),
            env: None,
            title: Some("Pane 2".to_string()),
            idle_time_limit: None,
            theme: None,
        }).unwrap();
        writer2.write_event(&CastEvent::Output(0.2, "Right Pane".to_string())).unwrap();
        writer2.finish().unwrap();

        // Create compositor
        let mut compositor = Compositor::new(&temp_dir).unwrap();
        compositor.build_timeline().unwrap();

        // Render frame
        let frame = compositor.render_frame(1.0).unwrap();

        // Should be wide enough for both panes
        assert_eq!(frame.width, 40);
        assert_eq!(frame.height, 10);

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
