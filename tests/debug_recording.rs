//! Debug test to see exactly why recording fails
//!
//! Run with: RUST_LOG=debug cargo test --test debug_recording -- --nocapture

use plexus_locus::recording::engine::PaneRecorder;
use std::process::Command;

#[tokio::test]
async fn debug_pane_recorder_start() {
    // Initialize logging
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let session_name = "debug-recording";
    let output_dir = "/tmp/debug-recording";

    // Cleanup
    Command::new("tmux")
        .args(&["kill-session", "-t", session_name])
        .status()
        .ok();
    let _ = std::fs::remove_dir_all(output_dir);
    std::fs::create_dir_all(output_dir).unwrap();

    // Create tmux session
    println!("\n1. Creating tmux session...");
    let status = Command::new("tmux")
        .args(&["new-session", "-d", "-s", session_name, "-x", "80", "-y", "24"])
        .status()
        .unwrap();
    assert!(status.success(), "Failed to create tmux session");

    // Get pane ID
    println!("2. Getting pane ID...");
    let output = Command::new("tmux")
        .args(&["list-panes", "-t", session_name, "-F", "#{pane_id}"])
        .output()
        .unwrap();
    let pane_id = String::from_utf8(output.stdout).unwrap().trim().to_string();
    println!("   Pane ID: {}", pane_id);

    // Try to start recording
    println!("3. Starting PaneRecorder...");
    let result = PaneRecorder::start(pane_id.clone(), output_dir, 80, 24).await;

    match result {
        Ok(recorder) => {
            println!("✅ PaneRecorder started successfully!");

            // Send some test data
            println!("4. Sending test data to pane...");
            Command::new("tmux")
                .args(&["send-keys", "-t", &pane_id, "echo 'test output'", "C-m"])
                .status()
                .unwrap();

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Stop recording
            println!("5. Stopping recorder...");
            let cast_path = recorder.stop().await.unwrap();
            println!("   Cast file: {:?}", cast_path);

            // Check file contents
            if cast_path.exists() {
                let contents = std::fs::read_to_string(&cast_path).unwrap();
                println!("   File size: {} bytes", contents.len());
                println!("   Contents:\n{}", contents);
            } else {
                println!("   ❌ File doesn't exist!");
            }
        }
        Err(e) => {
            println!("❌ PaneRecorder::start failed: {:?}", e);
            println!("\nDebug info:");
            println!("   Pane ID: {}", pane_id);
            println!("   Output dir: {}", output_dir);
            println!("   Output dir exists: {}", std::path::Path::new(output_dir).exists());

            // Check if mkfifo is available
            let mkfifo_check = Command::new("which").arg("mkfifo").output();
            println!("   mkfifo available: {:?}", mkfifo_check.map(|o| o.status.success()));

            // Check tmux pipe-pane
            let pipe_check = Command::new("tmux")
                .args(&["pipe-pane", "-t", &pane_id])
                .output();
            println!("   tmux pipe-pane response: {:?}", pipe_check.map(|o| String::from_utf8_lossy(&o.stderr).to_string()));

            panic!("Recording failed!");
        }
    }

    // Cleanup
    Command::new("tmux")
        .args(&["kill-session", "-t", session_name])
        .status()
        .ok();
}
