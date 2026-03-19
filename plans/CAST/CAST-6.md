# CAST-6: Pane Lifecycle Hooks

blocked_by: [CAST-4, CAST-5]
unlocks: [CAST-10]

## Scope

Automatically detect pane create/close/resize events during an active recording and update both the recording engine (add/remove pane recorders) and the layout journal (write events). Without this, recordings only capture panes that existed at `record.start` time.

## Acceptance Criteria

- [ ] Polling loop (configurable interval, default 1s) that runs during active recording
  - Calls `tmux list-panes -F '#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height} #{window_index}'`
  - Diffs against previous snapshot
  - Detects: new panes, removed panes, resized/moved panes, tab switches
- [ ] On new pane: calls `RecordingSession::add_pane()` + writes `PaneOpened` to layout journal
- [ ] On closed pane: calls `RecordingSession::remove_pane()` + writes `PaneClosed` to layout journal
- [ ] On resize/move: writes `PaneResized` to layout journal
- [ ] On tab switch (active window changed): writes `TabSwitched` to layout journal
- [ ] Periodic `LayoutSnapshot` keyframes written at configurable interval (default 5s)
- [ ] Polling task spawned by `RecordingActivation::start()`, cancelled by `stop()`
- [ ] No polling overhead when no recording is active

## Implementation Notes

- Put in `src/recording/lifecycle.rs`
- Spawns a `tokio::task` that holds `Arc<Mutex<RecordingSession>>` + `Arc<Mutex<LayoutJournal>>`
- Returns a `tokio::task::JoinHandle` or `AbortHandle` for cancellation
- Previous pane state stored as `HashMap<String, PaneGeometry>` — diff against fresh query each tick
- Tab detection: track `#{window_active_flag}` or compare active window index
- Keep it simple: polling is fine at 1s intervals. No inotify/fswatch needed.
