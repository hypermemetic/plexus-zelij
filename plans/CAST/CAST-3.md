# CAST-3: Pipe-Pane Recording Engine

blocked_by: [CAST-2]
unlocks: [CAST-5]

## Scope

Core recording engine that uses `tmux pipe-pane` to capture raw terminal output from panes, timestamps the bytes, and writes per-pane `.cast` files.

## Acceptance Criteria

- [ ] `PaneRecorder` struct: manages pipe-pane for a single pane
  - `start(pane_id, output_dir, width, height)` — runs `tmux pipe-pane -t {pane_id} 'cat >> {path}'`
  - `stop()` — runs `tmux pipe-pane -t {pane_id}` (no command = close pipe)
  - Converts raw capture to `.cast` on stop (adds timestamps)
- [ ] `RecordingSession` struct: manages multiple `PaneRecorder`s
  - `start(session, output_dir)` — discovers all panes via `list-panes`, starts recording each
  - `stop()` — stops all pane recorders, returns list of `.cast` file paths
  - `add_pane(pane_id)` — start recording a new pane mid-session
  - `remove_pane(pane_id)` — stop recording a pane that was closed
  - `status()` — which panes are being recorded
- [ ] Raw output timestamping: use a wrapper script that prefixes each chunk with a timestamp, or use a named pipe with a tokio reader that timestamps on arrival
- [ ] Output directory structure: `{output_dir}/pane-{id}.cast`, `{output_dir}/layout.jsonl`
- [ ] Handles panes that already have pipes (warns and skips)

## Implementation Notes

- Put in `src/recording/engine.rs`
- **Timestamping approach**: Use a FIFO (named pipe) instead of a file. `pipe-pane` writes to the FIFO, a tokio task reads from it and timestamps each read. This gives sub-second precision without a wrapper script.
  ```
  mkfifo /tmp/rec/pane-%5.fifo
  tmux pipe-pane -t %5 'cat > /tmp/rec/pane-%5.fifo'
  tokio::task → read fifo → timestamp → write .cast events
  ```
- Alternative simpler approach: `pipe-pane` to a script that does `while IFS= read -r line; do echo "$(date +%s.%N) $line"; done` but this loses binary data. FIFO approach is better.
- `RecordingSession` holds `HashMap<String, PaneRecorder>` keyed by pane ID
- tmux-specific: this module imports from `backends::tmux` or shells out directly
- Must be `Send + Sync` for use from async activation code
