//! Integration tests for composite .cast writer.

use plexus_locus::cast::{CastEvent, CastHeader, CastReader, CastWriter};
use plexus_locus::compositor::{BorderStyle, CompositeOpts, CompositeWriter};
use std::fs;

#[test]
fn test_composite_writer_integration() {
    // Create a temporary recording directory
    let temp_dir = std::env::temp_dir().join("test_composite_integration");
    fs::create_dir_all(&temp_dir).unwrap();

    // Create layout.jsonl with two panes
    let layout_path = temp_dir.join("layout.jsonl");
    let events = vec![
        serde_json::json!({
            "timestamp": 0.0,
            "event": "layout_snapshot",
            "panes": [{
                "pane_id": "%1",
                "x": 0,
                "y": 0,
                "width": 40,
                "height": 20,
                "tab_index": 0
            }]
        }),
        serde_json::json!({
            "timestamp": 1.0,
            "event": "pane_opened",
            "pane_id": "%2",
            "x": 41,
            "y": 0,
            "width": 40,
            "height": 20,
            "tab_index": 0
        }),
    ];

    let content = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(&layout_path, content).unwrap();

    // Create pane-1.cast
    let cast_path1 = temp_dir.join("pane-1.cast");
    let mut writer1 = CastWriter::create(&cast_path1).unwrap();
    writer1
        .write_header(&CastHeader {
            version: 2,
            width: 40,
            height: 20,
            timestamp: Some(1234567890),
            env: None,
            title: Some("Pane 1".to_string()),
            idle_time_limit: None,
            theme: None,
        })
        .unwrap();
    writer1
        .write_event(&CastEvent::Output(0.1, "First pane output\n".to_string()))
        .unwrap();
    writer1
        .write_event(&CastEvent::Output(0.5, "More output\n".to_string()))
        .unwrap();
    writer1.finish().unwrap();

    // Create pane-2.cast (starts at t=1.0 when pane opens)
    let cast_path2 = temp_dir.join("pane-2.cast");
    let mut writer2 = CastWriter::create(&cast_path2).unwrap();
    writer2
        .write_header(&CastHeader {
            version: 2,
            width: 40,
            height: 20,
            timestamp: Some(1234567890),
            env: None,
            title: Some("Pane 2".to_string()),
            idle_time_limit: None,
            theme: None,
        })
        .unwrap();
    writer2
        .write_event(&CastEvent::Output(1.1, "Second pane\n".to_string()))
        .unwrap();
    writer2.finish().unwrap();

    // Create composite with various options
    let output_path = temp_dir.join("composite.cast");
    let opts = CompositeOpts {
        fps: 20.0,
        idle_time_limit: Some(3.0),
        border_style: BorderStyle::Single,
        title: Some("Integration Test Recording".to_string()),
        theme: None,
    };

    // Test with progress callback
    let writer = CompositeWriter::new(&temp_dir, &output_path, opts).with_progress(|p| {
        // Simple progress callback that just prints
        if p >= 1.0 {
            println!("Composite writing complete");
        }
    });

    let result = writer.run().unwrap();

    // Verify result
    assert_eq!(result.output_path, output_path);
    assert!(result.duration_secs > 0.0);
    assert!(result.frame_count > 0);
    assert!(result.total_bytes > 0);

    // Verify output file is valid
    assert!(output_path.exists());
    let reader = CastReader::open(&output_path).unwrap();

    let header = reader.header();
    assert_eq!(header.version, 2);
    assert_eq!(
        header.title,
        Some("Integration Test Recording".to_string())
    );
    // Initial layout has only one pane (40x20), so that's the initial dimension
    assert_eq!(header.width, 40);
    assert_eq!(header.height, 20);

    // Verify events exist
    let events: Vec<_> = reader
        .events()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!events.is_empty());

    // Should have at least some output events
    let output_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, CastEvent::Output(_, _)))
        .collect();
    assert!(!output_events.is_empty());

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_composite_writer_frame_rate_limiting() {
    let temp_dir = std::env::temp_dir().join("test_composite_fps");
    fs::create_dir_all(&temp_dir).unwrap();

    // Create layout
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
    fs::write(&layout_path, serde_json::to_string(&layout_snapshot).unwrap()).unwrap();

    // Create pane with many rapid events (100 events in 1 second)
    let cast_path = temp_dir.join("pane-1.cast");
    let mut writer = CastWriter::create(&cast_path).unwrap();
    writer
        .write_header(&CastHeader {
            version: 2,
            width: 80,
            height: 24,
            timestamp: Some(1234567890),
            env: None,
            title: None,
            idle_time_limit: None,
            theme: None,
        })
        .unwrap();

    // Write 100 events in 1 second (every 0.01s)
    for i in 0..100 {
        let time = 0.01 * i as f64;
        writer
            .write_event(&CastEvent::Output(time, format!("Event {}\n", i)))
            .unwrap();
    }
    writer.finish().unwrap();

    // Composite with 10 fps (should reduce 100 events to ~10 frames)
    let output_path = temp_dir.join("composite.cast");
    let opts = CompositeOpts {
        fps: 10.0,
        idle_time_limit: None,
        border_style: BorderStyle::None,
        title: None,
        theme: None,
    };

    let writer = CompositeWriter::new(&temp_dir, &output_path, opts);
    let result = writer.run().unwrap();

    // With 10 fps, 1 second of recording should produce roughly 10 frames
    // (allow some tolerance for edge cases)
    assert!(
        result.frame_count >= 8 && result.frame_count <= 15,
        "Expected ~10 frames, got {}",
        result.frame_count
    );

    // Cleanup
    fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_composite_writer_border_styles() {
    for style in [
        BorderStyle::Single,
        BorderStyle::Double,
        BorderStyle::Heavy,
        BorderStyle::None,
    ] {
        let temp_dir = std::env::temp_dir().join(format!("test_composite_border_{:?}", style));
        fs::create_dir_all(&temp_dir).unwrap();

        // Create layout with two panes
        let layout_path = temp_dir.join("layout.jsonl");
        let layout_snapshot = serde_json::json!({
            "timestamp": 0.0,
            "event": "layout_snapshot",
            "panes": [
                {
                    "pane_id": "%1",
                    "x": 0,
                    "y": 0,
                    "width": 40,
                    "height": 10,
                    "tab_index": 0
                },
                {
                    "pane_id": "%2",
                    "x": 41,
                    "y": 0,
                    "width": 40,
                    "height": 10,
                    "tab_index": 0
                }
            ]
        });
        fs::write(&layout_path, serde_json::to_string(&layout_snapshot).unwrap()).unwrap();

        // Create minimal cast files
        for pane_id in [1, 2] {
            let cast_path = temp_dir.join(format!("pane-{}.cast", pane_id));
            let mut writer = CastWriter::create(&cast_path).unwrap();
            writer
                .write_header(&CastHeader {
                    version: 2,
                    width: 40,
                    height: 10,
                    timestamp: Some(1234567890),
                    env: None,
                    title: None,
                    idle_time_limit: None,
                    theme: None,
                })
                .unwrap();
            writer
                .write_event(&CastEvent::Output(0.1, format!("Pane {}\n", pane_id)))
                .unwrap();
            writer.finish().unwrap();
        }

        // Create composite with this border style
        let output_path = temp_dir.join("composite.cast");
        let opts = CompositeOpts {
            fps: 10.0,
            idle_time_limit: Some(2.0),
            border_style: style,
            title: Some(format!("Border test {:?}", style)),
            theme: None,
        };

        let writer = CompositeWriter::new(&temp_dir, &output_path, opts);
        let result = writer.run().unwrap();

        // All border styles should produce valid output
        assert!(result.frame_count > 0);
        assert!(output_path.exists());

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }
}
