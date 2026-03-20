//! Live demo - sends commands while recording
//!
//! Run with: cargo test --test live_demo_recording -- --ignored --nocapture

use plexus_locus::recording::{RecordingSession, LayoutJournal};
use plexus_locus::compositor::{CompositeWriter, CompositeOpts, BorderStyle};
use std::process::Command;
use std::time::Instant;
use tokio::time::{sleep, Duration};

#[tokio::test]
#[ignore]
async fn live_demo_with_commands() -> anyhow::Result<()> {
    let session_name = "live-demo";
    let output_dir = "/tmp/live-demo";

    // Cleanup
    Command::new("tmux").args(&["kill-session", "-t", session_name]).status().ok();
    let _ = std::fs::remove_dir_all(output_dir);
    std::fs::create_dir_all(output_dir)?;

    println!("\n🎬 Creating demo session");

    // Create tmux session with 2 panes
    Command::new("tmux")
        .args(&["new-session", "-d", "-s", session_name, "-x", "100", "-y", "30"])
        .status()?;

    Command::new("tmux")
        .args(&["split-window", "-h", "-t", session_name])
        .status()?;

    println!("✅ Created session with 2 panes");

    // Start recording FIRST
    let start_time = Instant::now();
    let mut journal = LayoutJournal::new(output_dir, start_time)?;
    journal.snapshot(session_name).await?;

    let recording = RecordingSession::start(session_name, output_dir).await?;
    println!("🔴 Recording started");

    // NOW send commands while recording
    sleep(Duration::from_millis(500)).await;

    println!("📝 Sending commands to left pane...");
    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.0", session_name),
                "echo '=== LEFT PANE ===='", "C-m"])
        .status()?;
    sleep(Duration::from_millis(300)).await;

    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.0", session_name),
                "date", "C-m"])
        .status()?;
    sleep(Duration::from_millis(300)).await;

    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.0", session_name),
                "echo 'Multi-pane recording demo'", "C-m"])
        .status()?;
    sleep(Duration::from_millis(500)).await;

    println!("📝 Sending commands to right pane...");
    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.1", session_name),
                "echo '=== RIGHT PANE ===='", "C-m"])
        .status()?;
    sleep(Duration::from_millis(300)).await;

    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.1", session_name),
                "ls -lh /workspace/plexus-zelij/src/ | head -10", "C-m"])
        .status()?;
    sleep(Duration::from_millis(500)).await;

    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.1", session_name),
                "echo 'Done!'", "C-m"])
        .status()?;
    sleep(Duration::from_millis(500)).await;

    // Stop recording
    println!("🛑 Stopping recording...");
    let cast_files = recording.stop().await?;
    journal.close()?;

    println!("✅ Recorded {} panes", cast_files.len());
    for path in &cast_files {
        let size = std::fs::metadata(path)?.len();
        let contents = std::fs::read_to_string(path)?;
        let event_count = contents.lines().count() - 1; // subtract header
        println!("  - {:?}: {} bytes, {} events",
            path.file_name().unwrap(), size, event_count);
    }

    // Render composite
    println!("\n🎨 Rendering composite...");
    let output_path = std::path::PathBuf::from(output_dir).join("live-demo.cast");

    let opts = CompositeOpts {
        fps: 30.0,
        idle_time_limit: Some(0.5),
        border_style: BorderStyle::Single,
        title: Some("Live Demo - Multi-pane Recording".to_string()),
        theme: None,
    };

    let writer = CompositeWriter::new(output_dir, &output_path, opts);
    let result = writer.run()?;

    println!("✅ Composite rendered:");
    println!("  📹 {:?}", result.output_path);
    println!("  ⏱️  Duration: {:.2}s", result.duration_secs);
    println!("  🎞️  Frames: {}", result.frame_count);
    println!("  💾 Size: {} bytes", result.total_bytes);

    // Show a sample of the output
    println!("\n📄 Sample of composite output:");
    let composite_content = std::fs::read_to_string(&result.output_path)?;
    for (i, line) in composite_content.lines().take(3).enumerate() {
        if i == 0 {
            println!("  Header: {}", &line[..100.min(line.len())]);
        } else {
            println!("  Event {}: {}", i, &line[..150.min(line.len())]);
        }
    }

    println!("\n✨ Play with: asciinema play {:?}", result.output_path);

    // Cleanup
    Command::new("tmux").args(&["kill-session", "-t", session_name]).status().ok();

    Ok(())
}
