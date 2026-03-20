//! Composite .cast writer - orchestrates the full pipeline from per-pane recordings to composite output.

use super::diff::{diff_frames, render_full_frame};
use super::frame::CompositeFrame;
use super::{Compositor, CompositorError, Result};
use crate::cast::{CastEvent, CastHeader, CastWriter, Theme};
use std::path::{Path, PathBuf};

/// Border style for drawing between panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    /// Single-line box drawing characters (─ │ ┌ ┐ └ ┘).
    Single,
    /// Double-line box drawing characters (═ ║ ╔ ╗ ╚ ╝).
    Double,
    /// Heavy/bold box drawing characters (━ ┃ ┏ ┓ ┗ ┛).
    Heavy,
    /// No borders.
    None,
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self::Single
    }
}

/// Type alias for CastTheme (re-exported from cast module).
pub type CastTheme = Theme;

/// Options for composite recording.
#[derive(Debug, Clone)]
pub struct CompositeOpts {
    /// Maximum frame rate in frames per second.
    ///
    /// Events within the same 1/fps window are coalesced into a single frame.
    /// Default: 30.0
    pub fps: f64,

    /// Maximum idle time between events in seconds.
    ///
    /// Long pauses are compressed to this duration.
    /// Default: Some(2.0)
    pub idle_time_limit: Option<f64>,

    /// Border style between panes.
    ///
    /// Default: BorderStyle::Single
    pub border_style: BorderStyle,

    /// Optional title for the .cast header.
    pub title: Option<String>,

    /// Optional color theme for the .cast header.
    pub theme: Option<CastTheme>,
}

impl Default for CompositeOpts {
    fn default() -> Self {
        Self {
            fps: 30.0,
            idle_time_limit: Some(2.0),
            border_style: BorderStyle::Single,
            title: None,
            theme: None,
        }
    }
}

/// Result of composite recording.
#[derive(Debug, Clone)]
pub struct CompositeResult {
    /// Path to the output .cast file.
    pub output_path: PathBuf,

    /// Total duration in seconds.
    pub duration_secs: f64,

    /// Number of frames written.
    pub frame_count: usize,

    /// Total bytes written.
    pub total_bytes: u64,
}

/// Progress callback for long-running compositing operations.
///
/// Called periodically with percentage complete (0.0 - 1.0).
pub type ProgressCallback = Box<dyn Fn(f64) + Send>;

/// Main compositor writer - orchestrates the full pipeline.
///
/// This drives the compositing process from start to finish:
/// 1. Loads recording directory and builds timeline
/// 2. Iterates through timeline events with frame rate limiting
/// 3. Renders composite frames at appropriate intervals
/// 4. Diffs consecutive frames to minimize output
/// 5. Writes composite .cast file incrementally
pub struct CompositeWriter {
    recording_dir: PathBuf,
    output_path: PathBuf,
    opts: CompositeOpts,
    progress_callback: Option<ProgressCallback>,
}

impl CompositeWriter {
    /// Create a new composite writer.
    ///
    /// # Arguments
    /// * `recording_dir` - Directory containing pane-*.cast and layout.jsonl
    /// * `output_path` - Path where composite .cast file will be written
    /// * `opts` - Compositing options (fps, idle time limit, etc.)
    pub fn new(
        recording_dir: impl AsRef<Path>,
        output_path: impl AsRef<Path>,
        opts: CompositeOpts,
    ) -> Self {
        Self {
            recording_dir: recording_dir.as_ref().to_path_buf(),
            output_path: output_path.as_ref().to_path_buf(),
            opts,
            progress_callback: None,
        }
    }

    /// Set a progress callback for long recordings.
    ///
    /// The callback receives a percentage (0.0 - 1.0) as compositing progresses.
    pub fn with_progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(f64) + Send + 'static,
    {
        self.progress_callback = Some(Box::new(callback));
        self
    }

    /// Run the full compositing pipeline.
    ///
    /// This is the main entry point that orchestrates all steps:
    /// - Build timeline from per-pane recordings
    /// - Iterate through timeline with frame rate limiting
    /// - Render composite frames
    /// - Diff consecutive frames
    /// - Write composite .cast file
    ///
    /// Returns statistics about the compositing operation.
    pub fn run(self) -> Result<CompositeResult> {
        // Create compositor and build timeline
        let mut compositor = Compositor::new(&self.recording_dir)?;
        compositor.build_timeline()?;

        // Clone the timeline to avoid borrow issues
        let timeline: Vec<_> = compositor.timeline().to_vec();

        if timeline.is_empty() {
            return Err(CompositorError::InvalidDirectory(
                "No events found in timeline".to_string()
            ));
        }

        // Determine initial dimensions from first layout
        let initial_layout = compositor.layout_reader.layout_at(0.0);
        let (initial_width, initial_height) = if initial_layout.is_empty() {
            (80, 24) // Default fallback
        } else {
            Compositor::calculate_dimensions(&initial_layout)
        };

        // Create output writer
        let mut writer = CastWriter::create(&self.output_path)?;

        // Write header
        let header = CastHeader {
            version: 2,
            width: initial_width,
            height: initial_height,
            timestamp: Some(chrono::Utc::now().timestamp()),
            env: None,
            title: self.opts.title.clone(),
            idle_time_limit: self.opts.idle_time_limit,
            theme: self.opts.theme.clone(),
        };
        writer.write_header(&header)?;

        // Pipeline state
        let mut prev_frame: Option<CompositeFrame> = None;
        let mut last_frame_time = 0.0;
        let mut last_event_time = 0.0;
        let mut frame_count = 0;
        let frame_interval = 1.0 / self.opts.fps;

        let total_duration = timeline.last().map(|(t, _)| *t).unwrap_or(0.0);
        let mut last_progress_report = 0.0;

        // Process timeline in chronological order
        let mut current_time = 0.0;
        let mut event_idx = 0;

        while event_idx < timeline.len() {
            // Collect all events up to next frame time
            let next_frame_time = last_frame_time + frame_interval;
            let mut events_to_process = Vec::new();

            while event_idx < timeline.len() {
                let (event_time, event) = &timeline[event_idx];

                if *event_time > next_frame_time {
                    break;
                }

                events_to_process.push((*event_time, event.clone()));
                current_time = *event_time;
                event_idx += 1;
            }

            // If no events collected and we haven't reached the end, advance to next frame time
            if events_to_process.is_empty() {
                if event_idx < timeline.len() {
                    current_time = timeline[event_idx].0;
                    last_frame_time = current_time;
                } else {
                    break;
                }
                continue;
            }

            // Process collected events (compositor maintains state)
            for (_event_time, event) in &events_to_process {
                match event {
                    super::TimelineEvent::PaneOutput { pane_id, bytes } => {
                        if let Some(state) = compositor.pane_states.get_mut(pane_id) {
                            state.process(bytes);
                        }
                    }
                    super::TimelineEvent::LayoutChange { event: layout_event } => {
                        match layout_event {
                            super::LayoutEvent::PaneResized { pane_id, width, height, .. } => {
                                if let Some(state) = compositor.pane_states.get_mut(pane_id) {
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

            // Calculate time for this frame (last event time in the batch)
            let frame_time = events_to_process.last().map(|(t, _)| *t).unwrap_or(current_time);

            // Apply idle time compression
            let mut time_delta = frame_time - last_event_time;
            if let Some(idle_limit) = self.opts.idle_time_limit {
                if time_delta > idle_limit {
                    time_delta = idle_limit;
                }
            }
            let write_time = last_event_time + time_delta;

            // Render frame at current state
            let current_layout = compositor.layout_reader.layout_at(frame_time);

            if !current_layout.is_empty() {
                // Initialize pane states for any new panes
                for pane_geo in &current_layout {
                    if !compositor.pane_states.contains_key(&pane_geo.pane_id) {
                        compositor.pane_states.insert(
                            pane_geo.pane_id.clone(),
                            super::pane_state::PaneState::new(
                                pane_geo.pane_id.clone(),
                                pane_geo.width,
                                pane_geo.height,
                            ),
                        );
                    }
                }

                // Calculate composite dimensions
                let (comp_width, comp_height) = Compositor::calculate_dimensions(&current_layout);

                // Create composite frame
                let mut frame = CompositeFrame::new(comp_width, comp_height);

                // Draw each pane
                for pane_geo in &current_layout {
                    if let Some(state) = compositor.pane_states.get(&pane_geo.pane_id) {
                        let cells = state.cells();
                        for (pane_row, row_cells) in cells.iter().enumerate() {
                            for (pane_col, cell) in row_cells.iter().enumerate() {
                                let frame_row = pane_geo.y + pane_row as u16;
                                let frame_col = pane_geo.x + pane_col as u16;
                                frame.set_cell(frame_row, frame_col, cell.clone());
                            }
                        }
                    }
                }

                // Draw borders if needed and if there are multiple panes
                if self.opts.border_style != BorderStyle::None && current_layout.len() > 1 {
                    self.draw_borders(&mut frame, &current_layout);
                }

                // Generate output for this frame
                let output = if let Some(prev) = &prev_frame {
                    // Check for dimension change (requires resize event + full render)
                    if prev.width != frame.width || prev.height != frame.height {
                        // Emit resize event
                        writer.write_event(&CastEvent::Resize(
                            write_time,
                            frame.width,
                            frame.height,
                        ))?;

                        // Full re-render after resize
                        render_full_frame(&frame)
                    } else {
                        // Diff consecutive frames
                        diff_frames(prev, &frame)
                    }
                } else {
                    // First frame - always full render
                    render_full_frame(&frame)
                };

                // Write output event if there's content
                if !output.is_empty() {
                    writer.write_event(&CastEvent::Output(write_time, output))?;
                    frame_count += 1;
                }

                // Flush periodically (every ~100 frames)
                if frame_count % 100 == 0 {
                    writer.flush()?;
                }

                prev_frame = Some(frame);
            }

            last_frame_time = frame_time;
            last_event_time = write_time;

            // Report progress
            if let Some(ref callback) = self.progress_callback {
                let progress = frame_time / total_duration;
                if progress - last_progress_report >= 0.01 {
                    // Report every 1%
                    callback(progress);
                    last_progress_report = progress;
                }
            }
        }

        // Final flush
        writer.flush()?;

        // Get file size
        let total_bytes = std::fs::metadata(&self.output_path)?.len();

        // Finish writing
        writer.finish()?;

        // Report 100% complete
        if let Some(ref callback) = self.progress_callback {
            callback(1.0);
        }

        Ok(CompositeResult {
            output_path: self.output_path,
            duration_secs: last_event_time,
            frame_count,
            total_bytes,
        })
    }

    /// Draw borders between panes based on border style.
    fn draw_borders(&self, frame: &mut CompositeFrame, layout: &[super::PaneGeometry]) {
        use super::frame::Cell;

        // Border characters for each style
        let (h_line, v_line, tl_corner, tr_corner, bl_corner, br_corner) = match self.opts.border_style {
            BorderStyle::Single => ('─', '│', '┌', '┐', '└', '┘'),
            BorderStyle::Double => ('═', '║', '╔', '╗', '╚', '╝'),
            BorderStyle::Heavy => ('━', '┃', '┏', '┓', '┗', '┛'),
            BorderStyle::None => return, // No borders
        };

        for pane in layout {
            // Top border
            if pane.y > 0 {
                for col in pane.x..pane.x + pane.width {
                    if col < frame.width {
                        frame.set_cell(pane.y.saturating_sub(1), col, Cell::new(h_line));
                    }
                }
            }

            // Bottom border
            let bottom = pane.y + pane.height;
            if bottom < frame.height {
                for col in pane.x..pane.x + pane.width {
                    if col < frame.width {
                        frame.set_cell(bottom, col, Cell::new(h_line));
                    }
                }
            }

            // Left border
            if pane.x > 0 {
                for row in pane.y..pane.y + pane.height {
                    if row < frame.height {
                        frame.set_cell(row, pane.x.saturating_sub(1), Cell::new(v_line));
                    }
                }
            }

            // Right border
            let right = pane.x + pane.width;
            if right < frame.width {
                for row in pane.y..pane.y + pane.height {
                    if row < frame.height {
                        frame.set_cell(row, right, Cell::new(v_line));
                    }
                }
            }

            // Corners
            if pane.y > 0 && pane.x > 0 {
                frame.set_cell(pane.y.saturating_sub(1), pane.x.saturating_sub(1), Cell::new(tl_corner));
            }

            if pane.y > 0 && pane.x + pane.width < frame.width {
                frame.set_cell(pane.y.saturating_sub(1), pane.x + pane.width, Cell::new(tr_corner));
            }

            if pane.y + pane.height < frame.height && pane.x > 0 {
                frame.set_cell(pane.y + pane.height, pane.x.saturating_sub(1), Cell::new(bl_corner));
            }

            if pane.y + pane.height < frame.height && pane.x + pane.width < frame.width {
                frame.set_cell(pane.y + pane.height, pane.x + pane.width, Cell::new(br_corner));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast::{CastReader, CastWriter};
    use std::fs;

    #[test]
    fn test_composite_opts_default() {
        let opts = CompositeOpts::default();
        assert_eq!(opts.fps, 30.0);
        assert_eq!(opts.idle_time_limit, Some(2.0));
        assert_eq!(opts.border_style, BorderStyle::Single);
        assert!(opts.title.is_none());
        assert!(opts.theme.is_none());
    }

    #[test]
    fn test_border_style() {
        assert_eq!(BorderStyle::default(), BorderStyle::Single);
    }

    #[test]
    fn test_composite_writer_creation() {
        let temp_dir = std::env::temp_dir();
        let recording_dir = temp_dir.join("test_composite_writer");
        let output_path = temp_dir.join("output.cast");

        let writer = CompositeWriter::new(&recording_dir, &output_path, CompositeOpts::default());

        assert_eq!(writer.recording_dir, recording_dir);
        assert_eq!(writer.output_path, output_path);
        assert_eq!(writer.opts.fps, 30.0);
    }

    #[test]
    fn test_composite_writer_with_progress() {
        let temp_dir = std::env::temp_dir();
        let recording_dir = temp_dir.join("test_progress");
        let output_path = temp_dir.join("output.cast");

        let writer = CompositeWriter::new(&recording_dir, &output_path, CompositeOpts::default())
            .with_progress(|progress| {
                println!("Progress: {:.1}%", progress * 100.0);
            });

        assert!(writer.progress_callback.is_some());
    }

    #[test]
    fn test_composite_writer_end_to_end() {
        // Create a test recording directory
        let temp_dir = std::env::temp_dir().join("test_composite_e2e");
        fs::create_dir_all(&temp_dir).unwrap();

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
        fs::write(&layout_path, serde_json::to_string(&layout_snapshot).unwrap()).unwrap();

        // Create pane-1.cast
        let cast_path = temp_dir.join("pane-1.cast");
        let mut cast_writer = CastWriter::create(&cast_path).unwrap();

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

        cast_writer.write_header(&header).unwrap();
        cast_writer.write_event(&CastEvent::Output(0.1, "Hello, World!\n".to_string())).unwrap();
        cast_writer.write_event(&CastEvent::Output(0.5, "This is a test.\n".to_string())).unwrap();
        cast_writer.finish().unwrap();

        // Create composite writer and run
        let output_path = temp_dir.join("composite.cast");
        let opts = CompositeOpts {
            fps: 10.0,
            idle_time_limit: Some(2.0),
            border_style: BorderStyle::None,
            title: Some("Composite Test".to_string()),
            theme: None,
        };

        let writer = CompositeWriter::new(&temp_dir, &output_path, opts);
        let result = writer.run().unwrap();

        // Verify result
        assert_eq!(result.output_path, output_path);
        assert!(result.duration_secs > 0.0);
        assert!(result.frame_count > 0);
        assert!(result.total_bytes > 0);

        // Verify output file exists and is valid
        assert!(output_path.exists());

        let reader = CastReader::open(&output_path).unwrap();
        assert_eq!(reader.header().version, 2);
        assert_eq!(reader.header().title, Some("Composite Test".to_string()));

        // Count events
        let events: Vec<_> = reader.events().collect::<crate::cast::Result<Vec<_>>>().unwrap();
        assert!(!events.is_empty());

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_composite_writer_multi_pane() {
        // Create a test recording directory with two panes
        let temp_dir = std::env::temp_dir().join("test_composite_multi");
        fs::create_dir_all(&temp_dir).unwrap();

        // Create layout.jsonl with two panes side by side
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
                    "x": 21, // Leave space for border
                    "y": 0,
                    "width": 20,
                    "height": 10,
                    "tab_index": 0
                }
            ]
        });
        fs::write(&layout_path, serde_json::to_string(&layout_snapshot).unwrap()).unwrap();

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
        writer1.write_event(&CastEvent::Output(0.1, "Left Pane\n".to_string())).unwrap();
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
        writer2.write_event(&CastEvent::Output(0.2, "Right Pane\n".to_string())).unwrap();
        writer2.finish().unwrap();

        // Create composite with borders
        let output_path = temp_dir.join("composite.cast");
        let opts = CompositeOpts {
            fps: 10.0,
            idle_time_limit: Some(2.0),
            border_style: BorderStyle::Double,
            title: Some("Multi-Pane Test".to_string()),
            theme: None,
        };

        let writer = CompositeWriter::new(&temp_dir, &output_path, opts);
        let result = writer.run().unwrap();

        // Verify result
        assert!(result.frame_count > 0);
        assert!(output_path.exists());

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_composite_writer_idle_compression() {
        // Create a test recording with a long pause
        let temp_dir = std::env::temp_dir().join("test_composite_idle");
        fs::create_dir_all(&temp_dir).unwrap();

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
        fs::write(&layout_path, serde_json::to_string(&layout_snapshot).unwrap()).unwrap();

        // Create pane-1.cast with long pause
        let cast_path = temp_dir.join("pane-1.cast");
        let mut cast_writer = CastWriter::create(&cast_path).unwrap();
        cast_writer.write_header(&CastHeader {
            version: 2,
            width: 40,
            height: 10,
            timestamp: Some(1234567890),
            env: None,
            title: None,
            idle_time_limit: None,
            theme: None,
        }).unwrap();
        cast_writer.write_event(&CastEvent::Output(0.1, "Before pause\n".to_string())).unwrap();
        cast_writer.write_event(&CastEvent::Output(10.0, "After 10s pause\n".to_string())).unwrap();
        cast_writer.finish().unwrap();

        // Create composite with idle limit
        let output_path = temp_dir.join("composite.cast");
        let opts = CompositeOpts {
            fps: 10.0,
            idle_time_limit: Some(2.0), // Compress to 2s max
            border_style: BorderStyle::None,
            title: None,
            theme: None,
        };

        let writer = CompositeWriter::new(&temp_dir, &output_path, opts);
        let result = writer.run().unwrap();

        // Duration should be compressed (much less than 10s)
        assert!(result.duration_secs < 5.0, "Expected compressed duration, got {}", result.duration_secs);

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_composite_writer_empty_recording() {
        let temp_dir = std::env::temp_dir().join("test_composite_empty");
        fs::create_dir_all(&temp_dir).unwrap();

        // Create empty layout.jsonl
        let layout_path = temp_dir.join("layout.jsonl");
        fs::write(&layout_path, "").unwrap();

        let output_path = temp_dir.join("composite.cast");
        let writer = CompositeWriter::new(&temp_dir, &output_path, CompositeOpts::default());

        // Should fail with no cast files
        let result = writer.run();
        assert!(result.is_err());

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }
}
