# CAST-5: Recording Activation (RPC Methods)

blocked_by: [CAST-3, CAST-4]
unlocks: [CAST-6, CAST-10]

## Scope

A new `RecordingActivation` registered on the Locus DynamicHub, exposing recording control via Plexus RPC. Users invoke via Synapse to start/stop/query recordings.

## Acceptance Criteria

- [ ] `RecordingActivation` struct with `#[hub_methods(namespace = "recording")]`
- [ ] Methods:
  - `start(session?, output_dir?)` — begin recording all panes in a tmux session. Default output_dir: `~/.local/share/locus/recordings/{timestamp}/`. Returns `RecordingStarted { recording_id, pane_count, output_dir }`
  - `stop(recording_id?)` — stop active recording (or most recent). Finalizes `.cast` files. Returns `RecordingStopped { recording_id, cast_files: Vec<String>, layout_file: String, duration_secs: f64 }`
  - `status()` — is recording active? Which panes? Duration so far. Returns `RecordingStatus { active, recording_id?, pane_ids, elapsed_secs, output_dir? }`
  - `snapshot_layout(recording_id?)` — manually trigger a layout snapshot now
  - `list()` — list past recordings (scan output dirs). Returns `Recordings { recordings: Vec<RecordingInfo> }`
- [ ] `RecordingEvent` enum for streaming responses
- [ ] Holds `Arc<Mutex<Option<RecordingSession>>>` for active recording state
- [ ] Error if backend is not tmux (recording requires pipe-pane)
- [ ] Registered on the DynamicHub in `main.rs` alongside sessions/tabs/panes/info

## Synapse Commands

```bash
synapse locus recording start
synapse locus recording start --output-dir /tmp/my-recording
synapse locus recording stop
synapse locus recording status
synapse locus recording snapshot_layout
synapse locus recording list
```

## Implementation Notes

- Put in `src/activations/recording.rs`, add to `src/activations/mod.rs`
- The activation needs access to the backend (to check if tmux) and the recording engine
- `Locus` factory gains a `pub recording: RecordingActivation` field
- `main.rs` registers `.register(locus.recording)` on the hub
- Only one recording at a time per Locus instance (simplifies state management)
