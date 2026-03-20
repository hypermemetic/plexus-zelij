//! Demonstration of CompositeWriter usage.
//!
//! This example shows how to use the CompositeWriter to create a composite .cast file
//! from per-pane recordings.

use plexus_locus::cast::{CastEvent, CastHeader, CastWriter};
use plexus_locus::compositor::{BorderStyle, CompositeOpts, CompositeWriter};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a demo recording directory
    let demo_dir = std::env::temp_dir().join("composite_demo");
    fs::create_dir_all(&demo_dir)?;

    println!("Creating demo recording in: {}", demo_dir.display());

    // Step 1: Create layout.jsonl
    println!("\n1. Creating layout journal...");
    let layout_path = demo_dir.join("layout.jsonl");

    let layout_events = vec![
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
            "timestamp": 2.0,
            "event": "pane_opened",
            "pane_id": "%2",
            "x": 0,
            "y": 13,
            "width": 80,
            "height": 11,
            "tab_index": 0
        }),
        serde_json::json!({
            "timestamp": 2.0,
            "event": "pane_resized",
            "pane_id": "%1",
            "x": 0,
            "y": 0,
            "width": 80,
            "height": 12
        }),
    ];

    let layout_content = layout_events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(&layout_path, layout_content)?;
    println!("   ✓ Layout journal created");

    // Step 2: Create per-pane .cast files
    println!("\n2. Creating per-pane recordings...");

    // Pane 1 - A simple shell session
    let cast1_path = demo_dir.join("pane-1.cast");
    let mut writer1 = CastWriter::create(&cast1_path)?;
    writer1.write_header(&CastHeader {
        version: 2,
        width: 80,
        height: 24,
        timestamp: Some(chrono::Utc::now().timestamp()),
        env: None,
        title: Some("Shell Session".to_string()),
        idle_time_limit: None,
        theme: None,
    })?;

    // Simulate shell interaction
    writer1.write_event(&CastEvent::Output(0.1, "$ ls -la\r\n".to_string()))?;
    writer1.write_event(&CastEvent::Output(
        0.3,
        "total 24\r\ndrwxr-xr-x  5 user  staff   160 Mar 19 10:00 .\r\n".to_string(),
    ))?;
    writer1.write_event(&CastEvent::Output(
        0.4,
        "drwxr-xr-x  3 user  staff    96 Mar 19 09:00 ..\r\n".to_string(),
    ))?;
    writer1.write_event(&CastEvent::Output(1.0, "$ echo 'Hello, World!'\r\n".to_string()))?;
    writer1.write_event(&CastEvent::Output(1.1, "Hello, World!\r\n".to_string()))?;
    writer1.write_event(&CastEvent::Output(1.5, "$ ".to_string()))?;
    writer1.finish()?;
    println!("   ✓ Pane 1 created (shell session)");

    // Pane 2 - A log viewer
    let cast2_path = demo_dir.join("pane-2.cast");
    let mut writer2 = CastWriter::create(&cast2_path)?;
    writer2.write_header(&CastHeader {
        version: 2,
        width: 80,
        height: 11,
        timestamp: Some(chrono::Utc::now().timestamp()),
        env: None,
        title: Some("Logs".to_string()),
        idle_time_limit: None,
        theme: None,
    })?;

    // Simulate log output (starts when pane opens at t=2.0)
    writer2.write_event(&CastEvent::Output(
        2.1,
        "\x1b[32m[INFO]\x1b[0m Server started on port 8080\r\n".to_string(),
    ))?;
    writer2.write_event(&CastEvent::Output(
        2.5,
        "\x1b[32m[INFO]\x1b[0m Ready to accept connections\r\n".to_string(),
    ))?;
    writer2.write_event(&CastEvent::Output(
        3.0,
        "\x1b[33m[WARN]\x1b[0m High memory usage detected\r\n".to_string(),
    ))?;
    writer2.finish()?;
    println!("   ✓ Pane 2 created (log viewer)");

    // Step 3: Create composite recording
    println!("\n3. Creating composite recording...");
    let output_path = demo_dir.join("composite.cast");

    let opts = CompositeOpts {
        fps: 30.0,
        idle_time_limit: Some(2.0),
        border_style: BorderStyle::Double,
        title: Some("Composite Demo - Multi-pane Session".to_string()),
        theme: None,
    };

    let writer = CompositeWriter::new(&demo_dir, &output_path, opts).with_progress(|progress| {
        if progress == 1.0 {
            println!("   ✓ Compositing complete (100%)");
        } else if (progress * 100.0) as u32 % 25 == 0 {
            println!("   Processing... {:.0}%", progress * 100.0);
        }
    });

    let result = writer.run()?;

    // Step 4: Display results
    println!("\n4. Results:");
    println!("   Output file: {}", result.output_path.display());
    println!("   Duration: {:.2} seconds", result.duration_secs);
    println!("   Frame count: {}", result.frame_count);
    println!("   File size: {} bytes", result.total_bytes);

    println!("\n✓ Demo complete!");
    println!("\nYou can play the composite recording with:");
    println!("  asciinema play {}", result.output_path.display());
    println!("\nOr convert to SVG:");
    println!(
        "  svg-term --in {} --out output.svg",
        result.output_path.display()
    );

    // Optional: Clean up
    println!("\nCleaning up demo files...");
    fs::remove_dir_all(&demo_dir)?;
    println!("✓ Cleanup complete");

    Ok(())
}
