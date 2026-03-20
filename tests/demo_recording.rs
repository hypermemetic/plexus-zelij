//! Demo recording test - creates an actual visual recording
//!
//! Run with: cargo test --test demo_recording -- --ignored --nocapture

use plexus_locus::recording::{RecordingSession, LayoutJournal};
use plexus_locus::compositor::{CompositeWriter, CompositeOpts, BorderStyle};
use std::time::Instant;
use tokio::time::{sleep, Duration};

#[tokio::test]
#[ignore]
async fn demo_multi_pane_recording() -> anyhow::Result<()> {
    let session_name = "cast-demo";
    let output_dir = "/tmp/cast-demo";

    println!("\n🎬 Recording session '{}'", session_name);
    let start_time = Instant::now();

    // Create layout journal
    let mut journal = LayoutJournal::new(output_dir, start_time)?;
    journal.snapshot(session_name).await?;

    // Start recording
    let recording = RecordingSession::start(session_name, output_dir).await?;
    println!("✅ Recording started (3 panes)");

    // Record for 3 seconds
    println!("⏱️  Recording...");
    sleep(Duration::from_secs(3)).await;

    // Stop
    let cast_files = recording.stop().await?;
    journal.close()?;

    println!("✅ Recorded {} panes:", cast_files.len());
    for path in &cast_files {
        let size = std::fs::metadata(path)?.len();
        println!("  - {:?} ({} bytes)", path.file_name().unwrap(), size);
    }

    // Render composite
    println!("\n🎨 Rendering composite...");
    let output_path = std::path::PathBuf::from(output_dir).join("demo.cast");

    let opts = CompositeOpts {
        fps: 10.0,
        idle_time_limit: Some(1.0),
        border_style: BorderStyle::Single,
        title: Some("CAST Demo - Multi-pane Recording".to_string()),
        theme: None,
    };

    let writer = CompositeWriter::new(output_dir, &output_path, opts);
    let result = writer.run()?;

    println!("✅ Rendered:");
    println!("  📹 {:?}", result.output_path);
    println!("  ⏱️  {:.2}s", result.duration_secs);
    println!("  🎞️  {} frames", result.frame_count);
    println!("  💾 {} bytes", result.total_bytes);

    println!("\n✨ Play with: asciinema play {:?}", result.output_path);

    Ok(())
}
