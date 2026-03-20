//! Integration tests for RenderActivation.
//!
//! Tests the render activation with real recording directories.

use plexus_locus::activations::RenderActivation;
use plexus_locus::cast::{CastEvent, CastHeader, CastWriter};
use plexus_locus::plexus::Activation;
use std::fs;
use std::path::PathBuf;
use futures::StreamExt;

/// Helper to collect events from a PlexusStream (items are already JSON values)
async fn collect_stream_items(stream: plexus_locus::plexus::PlexusStream) -> Vec<serde_json::Value> {
    stream
        .map(|item| {
            // Convert PlexusStreamItem to serde_json::Value
            serde_json::to_value(item).unwrap_or_default()
        })
        .collect()
        .await
}

/// Create a test recording directory with sample data.
fn create_test_recording() -> anyhow::Result<PathBuf> {
    let temp_dir = std::env::temp_dir().join(format!("test_render_activation_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::create_dir_all(&temp_dir)?;

    // Create layout.jsonl
    let layout_path = temp_dir.join("layout.jsonl");
    let layout_snapshot = serde_json::json!({
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
    });
    fs::write(&layout_path, serde_json::to_string(&layout_snapshot)?)?;

    // Create pane-1.cast
    let cast_path = temp_dir.join("pane-1.cast");
    let mut writer = CastWriter::create(&cast_path)?;

    let header = CastHeader {
        version: 2,
        width: 80,
        height: 24,
        timestamp: Some(1234567890),
        env: None,
        title: Some("Test Pane".to_string()),
        idle_time_limit: None,
        theme: None,
    };

    writer.write_header(&header)?;
    writer.write_event(&CastEvent::Output(0.1, "Hello, World!\n".to_string()))?;
    writer.write_event(&CastEvent::Output(0.5, "This is a test.\n".to_string()))?;
    writer.write_event(&CastEvent::Output(1.0, "Goodbye!\n".to_string()))?;
    writer.finish()?;

    Ok(temp_dir)
}

#[tokio::test]
async fn test_render_activation_info() {
    let recording_dir = create_test_recording().unwrap();

    let activation = RenderActivation::new();

    let params = serde_json::json!({
        "recording_dir": recording_dir.to_string_lossy().to_string()
    });

    let stream = Activation::call(&activation, "info", params).await.unwrap();
    let items = collect_stream_items(stream).await;

    // Should have events (may include a "done" message at the end)
    assert!(!items.is_empty());

    // Find the RecordingInfo event (content is nested)
    let info_event = items.iter().find(|item| {
        item.get("type").and_then(|v| v.as_str()) == Some("data") &&
        item.get("content").and_then(|c| c.get("type")).and_then(|v| v.as_str()) == Some("recording_info")
    });

    assert!(info_event.is_some(), "Expected to find a recording_info event");
    let info = info_event.unwrap().get("content").unwrap();

    // Verify it has expected fields
    assert!(info.get("recording_id").is_some());
    assert!(info.get("pane_count").is_some());
    assert!(info.get("duration_secs").is_some());
    assert!(info.get("layout_events").is_some());

    // Cleanup
    fs::remove_dir_all(&recording_dir).ok();
}

#[tokio::test]
async fn test_render_activation_preview() {
    let recording_dir = create_test_recording().unwrap();

    let activation = RenderActivation::new();

    let params = serde_json::json!({
        "recording_dir": recording_dir.to_string_lossy().to_string(),
        "time": 0.2
    });

    let stream = Activation::call(&activation, "preview", params).await.unwrap();
    let items = collect_stream_items(stream).await;

    // Should have events
    assert!(!items.is_empty());

    // Find the PreviewFrame event (content is nested)
    let preview_event = items.iter().find(|item| {
        item.get("type").and_then(|v| v.as_str()) == Some("data") &&
        item.get("content").and_then(|c| c.get("type")).and_then(|v| v.as_str()) == Some("preview_frame")
    });

    assert!(preview_event.is_some(), "Expected to find a preview_frame event");
    let preview = preview_event.unwrap().get("content").unwrap();

    // Verify it has expected fields
    assert!(preview.get("content").is_some());
    assert_eq!(preview.get("width").and_then(|v| v.as_u64()), Some(80));
    assert_eq!(preview.get("height").and_then(|v| v.as_u64()), Some(24));

    // Cleanup
    fs::remove_dir_all(&recording_dir).ok();
}

#[tokio::test]
async fn test_render_activation_render() {
    let recording_dir = create_test_recording().unwrap();
    let output_path = recording_dir.join("test_output.cast");

    let activation = RenderActivation::new();

    let params = serde_json::json!({
        "recording_dir": recording_dir.to_string_lossy().to_string(),
        "output_path": output_path.to_string_lossy().to_string(),
        "fps": 10.0,
        "idle_time_limit": 1.0,
        "border_style": "single"
    });

    let stream = Activation::call(&activation, "render", params).await.unwrap();
    let items = collect_stream_items(stream).await;

    // Should have progress events and a final complete event
    assert!(!items.is_empty());

    // Find the RenderComplete event (content is nested)
    let complete_event = items.iter().find(|item| {
        item.get("type").and_then(|v| v.as_str()) == Some("data") &&
        item.get("content").and_then(|c| c.get("type")).and_then(|v| v.as_str()) == Some("render_complete")
    });

    assert!(complete_event.is_some(), "Expected to find a render_complete event");
    let complete = complete_event.unwrap().get("content").unwrap();

    assert!(complete.get("output_path").is_some());
    assert!(complete.get("frame_count").and_then(|v| v.as_u64()).unwrap() > 0);

    // Check that output file was created
    assert!(output_path.exists());

    // Verify it's a valid cast file
    use plexus_locus::cast::CastReader;
    let reader = CastReader::open(&output_path).unwrap();
    assert_eq!(reader.header().version, 2);

    // Cleanup
    fs::remove_dir_all(&recording_dir).ok();
}

#[tokio::test]
async fn test_render_activation_missing_recording() {
    let activation = RenderActivation::new();

    let params = serde_json::json!({
        "recording_dir": "/nonexistent/recording"
    });

    let stream = Activation::call(&activation, "info", params).await.unwrap();
    let items = collect_stream_items(stream).await;

    // Should have events
    assert!(!items.is_empty());

    // Find the Error event (content is nested)
    let error_event = items.iter().find(|item| {
        item.get("type").and_then(|v| v.as_str()) == Some("data") &&
        item.get("content").and_then(|c| c.get("type")).and_then(|v| v.as_str()) == Some("error")
    });

    assert!(error_event.is_some(), "Expected to find an error event");
    let error = error_event.unwrap().get("content").unwrap();
    assert!(error.get("message").is_some());
}

#[tokio::test]
async fn test_render_activation_router() {
    let activation = RenderActivation::new();

    // Test router namespace
    use plexus_locus::plexus::ChildRouter;
    assert_eq!(activation.router_namespace(), "render");

    // Test that get_child returns None (render has no sub-namespaces)
    assert!(activation.get_child("nonexistent").await.is_none());
}

#[tokio::test]
async fn test_render_with_border_styles() {
    let recording_dir = create_test_recording().unwrap();

    let activation = RenderActivation::new();

    for border_style in &["single", "double", "heavy", "none"] {
        let output_path = recording_dir.join(format!("test_{}.cast", border_style));

        let params = serde_json::json!({
            "recording_dir": recording_dir.to_string_lossy().to_string(),
            "output_path": output_path.to_string_lossy().to_string(),
            "fps": 10.0,
            "idle_time_limit": 1.0,
            "border_style": border_style.to_string()
        });

        let stream = Activation::call(&activation, "render", params).await.unwrap();
        let items = collect_stream_items(stream).await;

        // Find RenderComplete event (content is nested)
        assert!(!items.is_empty());
        let complete_event = items.iter().find(|item| {
            item.get("type").and_then(|v| v.as_str()) == Some("data") &&
            item.get("content").and_then(|c| c.get("type")).and_then(|v| v.as_str()) == Some("render_complete")
        });

        assert!(
            complete_event.is_some(),
            "Expected RenderComplete for border {}",
            border_style
        );

        assert!(output_path.exists());
    }

    // Cleanup
    fs::remove_dir_all(&recording_dir).ok();
}
