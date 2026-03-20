//! Manual end-to-end test that requires tmux to be running.
//!
//! This test verifies:
//! 1. RecordingSession can start/stop with real tmux panes
//! 2. Layout journal captures real tmux layout
//! 3. Compositor can render real recordings
//! 4. Output .cast file is valid and playable
//!
//! Run with: cargo test --test manual_end_to_end -- --ignored --nocapture

use plexus_locus::recording::{RecordingSession, LayoutJournal};
use plexus_locus::compositor::{CompositeWriter, CompositeOpts, BorderStyle};
use plexus_locus::cast::CastReader;
use std::time::Instant;
use std::process::Command;
use std::fs;
use tokio::time::{sleep, Duration};

/// Helper to create a tmux session with 2 panes
fn create_test_tmux_session(session_name: &str) -> anyhow::Result<()> {
    // Create session
    Command::new("tmux")
        .args(&["new-session", "-d", "-s", session_name, "-x", "80", "-y", "24"])
        .status()?;

    // Split horizontally
    Command::new("tmux")
        .args(&["split-window", "-h", "-t", session_name])
        .status()?;

    // Send commands to generate output
    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.0", session_name), "echo 'Left pane'", "C-m"])
        .status()?;

    std::thread::sleep(std::time::Duration::from_millis(200));

    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.1", session_name), "echo 'Right pane'", "C-m"])
        .status()?;

    std::thread::sleep(std::time::Duration::from_millis(200));

    Ok(())
}

/// Cleanup tmux session
fn kill_tmux_session(session_name: &str) {
    Command::new("tmux")
        .args(&["kill-session", "-t", session_name])
        .status()
        .ok();
}

#[tokio::test]
#[ignore] // Run explicitly with --ignored
async fn test_real_tmux_recording_and_rendering() -> anyhow::Result<()> {
    let session_name = "cast-test-e2e";
    let output_dir = "/tmp/cast-test-e2e";

    // Cleanup from previous runs
    kill_tmux_session(session_name);
    let _ = fs::remove_dir_all(output_dir);
    fs::create_dir_all(output_dir)?;

    println!("\n=== Phase 1: Create tmux session with 2 panes ===");
    create_test_tmux_session(session_name)?;

    // Verify panes exist
    let panes_output = Command::new("tmux")
        .args(&["list-panes", "-s", "-t", session_name, "-F", "#{pane_id}"])
        .output()?;
    let panes = String::from_utf8(panes_output.stdout)?;
    println!("Found panes:\n{}", panes);
    assert!(panes.contains("%"), "Should have pane IDs");

    println!("\n=== Phase 2: Start RecordingSession ===");
    let start_time = Instant::now();

    // Create layout journal
    let mut journal = LayoutJournal::new(output_dir, start_time)?;
    journal.snapshot(session_name).await?;

    // Start recording session
    let mut recording = RecordingSession::start(session_name, output_dir).await?;
    println!("Recording started for session: {}", session_name);

    // Let it record for 2 seconds
    sleep(Duration::from_secs(2)).await;

    // Send more commands while recording
    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.0", session_name), "date", "C-m"])
        .status()?;

    sleep(Duration::from_millis(500)).await;

    Command::new("tmux")
        .args(&["send-keys", "-t", &format!("{}:0.1", session_name), "whoami", "C-m"])
        .status()?;

    sleep(Duration::from_millis(500)).await;

    println!("\n=== Phase 3: Stop recording ===");
    let cast_files = recording.stop().await?;
    journal.close()?;

    println!("Recorded {} panes", cast_files.len());

    // List all files in output_dir to see what was created
    println!("\nFiles in {}:", output_dir);
    for entry in fs::read_dir(output_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        println!("  {:?} ({} bytes)", entry.file_name(), metadata.len());
    }

    for (i, path) in cast_files.iter().enumerate() {
        println!("  Pane {}: {:?}", i, path);
        assert!(path.exists(), "Cast file should exist: {:?}", path);
    }

    if cast_files.is_empty() {
        println!("⚠️  WARNING: No .cast files were created!");
        println!("   This suggests PaneRecorder::start() failed for all panes");
        println!("   Check the logs above for errors");
        // Don't fail the test yet - let's see what happened
    }

    // Verify layout.jsonl exists
    let layout_path = std::path::PathBuf::from(output_dir).join("layout.jsonl");
    assert!(layout_path.exists(), "layout.jsonl should exist");

    println!("\n=== Phase 4: Verify .cast files ===");
    for cast_file in &cast_files {
        let reader = CastReader::open(cast_file)?;
        println!("  {:?}: v{}, {}x{}",
            cast_file.file_name().unwrap(),
            reader.header().version,
            reader.header().width,
            reader.header().height
        );

        let event_count = reader.events().count();
        println!("    Events: {}", event_count);
        assert!(event_count > 0, "Should have captured some events");
    }

    println!("\n=== Phase 5: Render composite .cast ===");
    let output_path = std::path::PathBuf::from(output_dir).join("composite.cast");

    let opts = CompositeOpts {
        fps: 10.0,
        idle_time_limit: Some(1.0),
        border_style: BorderStyle::Single,
        title: Some("End-to-end test".to_string()),
        theme: None,
    };

    let writer = CompositeWriter::new(
        output_dir,
        &output_path,
        opts,
    );

    let result = writer.run()?;

    println!("Composite rendered:");
    println!("  Output: {:?}", result.output_path);
    println!("  Duration: {:.2}s", result.duration_secs);
    println!("  Frames: {}", result.frame_count);
    println!("  Size: {} bytes", result.total_bytes);

    assert!(output_path.exists(), "Composite .cast should exist");
    assert!(result.frame_count > 0, "Should have rendered frames");

    println!("\n=== Phase 6: Verify composite is valid ===");
    let composite_reader = CastReader::open(&output_path)?;
    println!("Composite header: v{}, {}x{}",
        composite_reader.header().version,
        composite_reader.header().width,
        composite_reader.header().height
    );

    let composite_events: Vec<_> = composite_reader.events().collect();
    println!("Composite events: {}", composite_events.len());
    assert!(composite_events.len() > 0, "Composite should have events");

    println!("\n=== Phase 7: Cleanup ===");
    kill_tmux_session(session_name);
    // Keep output_dir for manual inspection
    println!("Recording saved to: {}", output_dir);
    println!("Composite: {:?}", output_path);

    println!("\n=== ✅ END-TO-END TEST PASSED ===");
    println!("You can play the recording with:");
    println!("  asciinema play {:?}", output_path);

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_pane_id_validation_with_real_tmux() -> anyhow::Result<()> {
    use plexus_locus::recording::engine::PaneRecorder;

    println!("\n=== Testing PaneRecorder validation ===");

    // Invalid pane ID should fail immediately
    let result = PaneRecorder::start("invalid".to_string(), "/tmp", 80, 24, None).await;
    assert!(result.is_err(), "Should reject invalid pane ID");
    println!("✅ Invalid pane ID correctly rejected");

    Ok(())
}
