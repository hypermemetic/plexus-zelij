# CAST-10: locus-render Activation & CLI

blocked_by: [CAST-9]
unlocks: []

## Scope

Expose the compositor as both a Locus RPC activation (render via synapse) and a standalone CLI mode. This is the user-facing entry point for Phase 2 — given a recording directory, produce a composite `.cast` file.

## Acceptance Criteria

- [ ] `RenderActivation` struct with `#[hub_methods(namespace = "render")]`
- [ ] RPC Methods:
  - `render(recording_dir?, recording_id?, output_path?, fps?, idle_time_limit?, border_style?)` — run compositor. If `recording_id` given, resolves to `~/.local/share/locus/recordings/{id}/`. Streams progress events, yields final `RenderComplete { output_path, duration_secs, frame_count }`
  - `preview(recording_dir?, recording_id?, time?)` — render a single composite frame at a given timestamp and return it as ANSI text. Useful for quick inspection without producing a full `.cast`
  - `info(recording_dir?, recording_id?)` — return recording metadata: pane count, duration, layout event count, total raw bytes
- [ ] `RenderEvent` enum for streaming responses:
  - `RenderProgress { percent: f64, frames_written: u64, elapsed_secs: f64 }`
  - `RenderComplete { output_path: String, duration_secs: f64, frame_count: u64, bytes: u64 }`
  - `PreviewFrame { content: String, width: u16, height: u16, time: f64 }`
  - `RecordingInfo { recording_id: String, pane_count: u32, duration_secs: f64, layout_events: u32 }`
  - `Error { message: String }`
- [ ] Standalone CLI mode: `plexus-locus render <recording_dir> [-o output.cast] [--fps 30] [--idle-limit 2.0] [--border single|double|heavy|none]`
  - Added as a subcommand in `main.rs` clap config
  - Runs compositor directly without starting the RPC server
  - Prints progress to stderr, writes `.cast` to output path
- [ ] Registered on the DynamicHub in `main.rs`: `hub.register(locus.render)`
- [ ] `Locus` factory gains `pub render: RenderActivation` field

## Synapse Commands

```bash
# Render most recent recording
synapse locus render render

# Render specific recording with options
synapse locus render render --recording_dir /path/to/recording --fps 24 --border_style heavy

# Quick preview at t=10s
synapse locus render preview --recording_id 20260319T143000 --time 10.0

# Recording metadata
synapse locus render info --recording_id 20260319T143000
```

## Standalone CLI

```bash
# Direct render without RPC
plexus-locus render ~/.local/share/locus/recordings/20260319T143000/ -o session.cast

# Preview at timestamp
plexus-locus render ~/.local/share/locus/recordings/20260319T143000/ --preview --time 10.0
```

## Implementation Notes

- `RenderActivation` in `src/activations/render.rs`
- CLI subcommand in `main.rs` using clap subcommands: `enum Command { Serve (default), Render { dir, opts } }`
- The activation does NOT need a backend reference — it works purely from recorded files
- Standalone mode skips all RPC/transport setup — just runs `CompositeWriter::run()` directly
- Progress: compositor yields progress via a channel, activation forwards as `RenderProgress` events
- Recording ID convention: ISO 8601 compact timestamp (e.g., `20260319T143000`) — matches the directory name created by `RecordingActivation::start()`
