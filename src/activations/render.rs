use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use std::path::PathBuf;
use std::time::Instant;

use crate::compositor::{BorderStyle, CompositeOpts, CompositeWriter, Compositor};
use crate::plexus::{Activation, ChildRouter, PlexusError, PlexusStream};
use crate::types::RenderEvent;

/// Render sub-activation — manages rendering recordings to composite .cast files.
///
/// Accessed as `locus.render.render`, `locus.render.preview`, `locus.render.info`.
/// This activation does NOT require a backend - it works purely from recorded files.
#[derive(Clone)]
pub struct RenderActivation;

impl Default for RenderActivation {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderActivation {
    /// Create a new RenderActivation (stateless, no backend required)
    pub const fn new() -> Self {
        Self
    }

    /// Resolve recording directory from either `recording_dir` or `recording_id`.
    fn resolve_recording_dir(
        recording_dir: Option<String>,
        recording_id: Option<String>,
    ) -> Result<PathBuf, String> {
        if let Some(dir) = recording_dir {
            Ok(PathBuf::from(dir))
        } else if let Some(id) = recording_id {
            // Resolve recording ID to default directory
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            Ok(PathBuf::from(home).join(".local/share/locus/recordings").join(id))
        } else {
            // Use most recent recording
            Self::find_most_recent_recording()
        }
    }

    /// Find the most recent recording in the default recordings directory.
    fn find_most_recent_recording() -> Result<PathBuf, String> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let recordings_dir = PathBuf::from(home).join(".local/share/locus/recordings");

        if !recordings_dir.exists() {
            return Err("No recordings directory found".to_string());
        }

        let mut recordings: Vec<_> = std::fs::read_dir(&recordings_dir)
            .map_err(|e| format!("Failed to read recordings directory: {e}"))?
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| {
                let file_name = entry.file_name();
                let recording_id = file_name.to_string_lossy();
                Some((recording_id.to_string(), entry.path()))
            })
            .collect();

        if recordings.is_empty() {
            return Err("No recordings found".to_string());
        }

        // Sort by recording_id (ISO 8601 timestamp) descending
        recordings.sort_by(|a, b| b.0.cmp(&a.0));

        Ok(recordings[0].1.clone())
    }

    /// Parse border style from string.
    fn parse_border_style(style: &str) -> BorderStyle {
        match style.to_lowercase().as_str() {
            "double" => BorderStyle::Double,
            "heavy" => BorderStyle::Heavy,
            "none" => BorderStyle::None,
            _ => BorderStyle::Single,
        }
    }
}

#[allow(missing_docs)]
#[plexus_macros::activation(
    namespace = "render",
    version = "0.1.0",
    description = "Render recordings to composite .cast files"
)]
impl RenderActivation {
    #[plexus_macros::method(
        description = "Render a recording to a composite .cast file",
        params(
            recording_dir = "Path to recording directory (default: most recent recording)",
            recording_id = "Recording ID to render (resolves to ~/.local/share/locus/recordings/{id}/)",
            output_path = "Output .cast file path (default: {recording_dir}/composite.cast)",
            fps = "Frame rate in frames per second (default: 30.0)",
            idle_time_limit = "Maximum idle time between events in seconds (default: 2.0)",
            border_style = "Border style: single, double, heavy, none (default: single)"
        )
    )]
    async fn render(
        &self,
        recording_dir: Option<String>,
        recording_id: Option<String>,
        output_path: Option<String>,
        fps: Option<f64>,
        idle_time_limit: Option<f64>,
        border_style: Option<String>,
    ) -> impl Stream<Item = RenderEvent> + Send + 'static {
        stream! {
            // Resolve recording directory
            let rec_dir = match Self::resolve_recording_dir(recording_dir, recording_id.clone()) {
                Ok(dir) => dir,
                Err(e) => {
                    yield RenderEvent::Error { message: e };
                    return;
                }
            };

            // Check that directory exists
            if !rec_dir.exists() || !rec_dir.is_dir() {
                yield RenderEvent::Error {
                    message: format!("Recording directory not found: {}", rec_dir.display()),
                };
                return;
            }

            // Determine output path
            let out_path = if let Some(path) = output_path {
                PathBuf::from(path)
            } else {
                rec_dir.join("composite.cast")
            };

            // Build composite options
            let opts = CompositeOpts {
                fps: fps.unwrap_or(30.0),
                idle_time_limit: Some(idle_time_limit.unwrap_or(2.0)),
                border_style: border_style
                    .as_ref()
                    .map_or(BorderStyle::Single, |s| Self::parse_border_style(s)),
                title: None,
                theme: None,
            };

            // Create progress tracking
            let start_time = Instant::now();
            let progress_tx = tokio::sync::mpsc::unbounded_channel::<f64>();
            let mut progress_rx = progress_tx.1;

            // Run compositor in a blocking task
            let rec_dir_clone = rec_dir.clone();
            let out_path_clone = out_path.clone();
            let mut render_task = tokio::task::spawn_blocking(move || {
                let writer = CompositeWriter::new(&rec_dir_clone, &out_path_clone, opts)
                    .with_progress(move |progress| {
                        let _ = progress_tx.0.send(progress);
                    });
                writer.run()
            });

            // Stream progress events
            let mut last_progress = 0.0;

            loop {
                tokio::select! {
                    Some(progress) = progress_rx.recv() => {
                        if progress > last_progress + 0.05 || progress >= 1.0 {
                            // Report every 5% or at completion
                            let elapsed = start_time.elapsed().as_secs_f64();
                            let frames_written = (progress * 1000.0) as u64; // Estimate

                            yield RenderEvent::RenderProgress {
                                percent: progress * 100.0,
                                frames_written,
                                elapsed_secs: elapsed,
                            };

                            last_progress = progress;
                        }
                    }
                    result = &mut render_task => {
                        match result {
                            Ok(Ok(composite_result)) => {
                                let duration_secs = start_time.elapsed().as_secs_f64();

                                yield RenderEvent::RenderComplete {
                                    output_path: composite_result.output_path.to_string_lossy().to_string(),
                                    duration_secs,
                                    frame_count: composite_result.frame_count as u64,
                                    bytes: composite_result.total_bytes,
                                };
                            }
                            Ok(Err(e)) => {
                                yield RenderEvent::Error {
                                    message: format!("Compositor error: {e}"),
                                };
                            }
                            Err(e) => {
                                yield RenderEvent::Error {
                                    message: format!("Task error: {e}"),
                                };
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    #[plexus_macros::method(
        description = "Render a single frame preview at a specific timestamp",
        params(
            recording_dir = "Path to recording directory (default: most recent recording)",
            recording_id = "Recording ID to preview (resolves to ~/.local/share/locus/recordings/{id}/)",
            time = "Timestamp in seconds (default: 0.0)"
        )
    )]
    async fn preview(
        &self,
        recording_dir: Option<String>,
        recording_id: Option<String>,
        time: Option<f64>,
    ) -> impl Stream<Item = RenderEvent> + Send + 'static {
        stream! {
            // Resolve recording directory
            let rec_dir = match Self::resolve_recording_dir(recording_dir, recording_id) {
                Ok(dir) => dir,
                Err(e) => {
                    yield RenderEvent::Error { message: e };
                    return;
                }
            };

            // Check that directory exists
            if !rec_dir.exists() || !rec_dir.is_dir() {
                yield RenderEvent::Error {
                    message: format!("Recording directory not found: {}", rec_dir.display()),
                };
                return;
            }

            let preview_time = time.unwrap_or(0.0);

            // Run preview in blocking task
            let result = tokio::task::spawn_blocking(move || {
                // Create compositor and build timeline
                let mut compositor = Compositor::new(&rec_dir)?;
                compositor.build_timeline()?;

                // Render frame at specified time
                let frame = compositor.render_frame(preview_time)?;

                Ok::<_, crate::compositor::CompositorError>((
                    frame.render_ansi(),
                    frame.width,
                    frame.height,
                ))
            })
            .await;

            match result {
                Ok(Ok((content, width, height))) => {
                    yield RenderEvent::PreviewFrame {
                        content,
                        width,
                        height,
                        time: preview_time,
                    };
                }
                Ok(Err(e)) => {
                    yield RenderEvent::Error {
                        message: format!("Preview error: {e}"),
                    };
                }
                Err(e) => {
                    yield RenderEvent::Error {
                        message: format!("Task error: {e}"),
                    };
                }
            }
        }
    }

    #[plexus_macros::method(
        description = "Get recording metadata and information",
        params(
            recording_dir = "Path to recording directory (default: most recent recording)",
            recording_id = "Recording ID to query (resolves to ~/.local/share/locus/recordings/{id}/)"
        )
    )]
    async fn info(
        &self,
        recording_dir: Option<String>,
        recording_id: Option<String>,
    ) -> impl Stream<Item = RenderEvent> + Send + 'static {
        stream! {
            // Resolve recording directory
            let rec_dir = match Self::resolve_recording_dir(recording_dir.clone(), recording_id.clone()) {
                Ok(dir) => dir,
                Err(e) => {
                    yield RenderEvent::Error { message: e };
                    return;
                }
            };

            // Check that directory exists
            if !rec_dir.exists() || !rec_dir.is_dir() {
                yield RenderEvent::Error {
                    message: format!("Recording directory not found: {}", rec_dir.display()),
                };
                return;
            }

            // Extract recording ID from path
            let id = recording_id.unwrap_or_else(|| {
                rec_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });

            // Run info gathering in blocking task
            let result = tokio::task::spawn_blocking(move || {
                // Create compositor to get info
                let mut compositor = Compositor::new(&rec_dir)?;
                compositor.build_timeline()?;

                // Count panes from layout
                let initial_layout = compositor.layout_at(0.0);
                let pane_count = initial_layout.len() as u32;

                // Get duration from timeline
                let duration_secs = compositor
                    .timeline()
                    .last()
                    .map_or(0.0, |(t, _)| *t);

                // Count layout events
                let layout_events = compositor.layout_event_count() as u32;

                Ok::<_, crate::compositor::CompositorError>((
                    pane_count,
                    duration_secs,
                    layout_events,
                ))
            })
            .await;

            match result {
                Ok(Ok((pane_count, duration_secs, layout_events))) => {
                    yield RenderEvent::RecordingInfo {
                        recording_id: id,
                        pane_count,
                        duration_secs,
                        layout_events,
                    };
                }
                Ok(Err(e)) => {
                    yield RenderEvent::Error {
                        message: format!("Info error: {e}"),
                    };
                }
                Err(e) => {
                    yield RenderEvent::Error {
                        message: format!("Task error: {e}"),
                    };
                }
            }
        }
    }
}

#[async_trait]
impl ChildRouter for RenderActivation {
    fn router_namespace(&self) -> &'static str {
        "render"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_border_style() {
        assert_eq!(RenderActivation::parse_border_style("single"), BorderStyle::Single);
        assert_eq!(RenderActivation::parse_border_style("double"), BorderStyle::Double);
        assert_eq!(RenderActivation::parse_border_style("heavy"), BorderStyle::Heavy);
        assert_eq!(RenderActivation::parse_border_style("none"), BorderStyle::None);
        assert_eq!(RenderActivation::parse_border_style("invalid"), BorderStyle::Single);
    }

    #[tokio::test]
    async fn test_render_activation_creation() {
        let activation = RenderActivation::new();
        assert_eq!(activation.router_namespace(), "render");
    }
}
