# CAST-4: Layout Event Journal

blocked_by: [CAST-3]
unlocks: [CAST-5, CAST-6]

## Scope

A JSONL event log that records layout changes (pane create/close/resize, tab switch) with timestamps. The compositor reads this to reconstruct pane positions at any point during the recording.

## Acceptance Criteria

- [ ] `LayoutEvent` enum (serde-tagged):
  - `PaneOpened { pane_id, x, y, width, height, tab_index }` — new pane appeared
  - `PaneClosed { pane_id }` — pane was closed
  - `PaneResized { pane_id, x, y, width, height }` — pane moved or resized
  - `TabSwitched { tab_index }` — active tab changed
  - `LayoutSnapshot { panes: Vec<PaneGeometry> }` — full layout at a point in time
- [ ] `PaneGeometry` struct: `pane_id, x, y, width, height, tab_index`
- [ ] `LayoutJournal` struct:
  - `new(output_dir)` — creates/opens `layout.jsonl`
  - `write_event(event)` — appends timestamped event
  - `snapshot(backend)` — queries current layout from tmux and writes a `LayoutSnapshot`
  - `close()` — flushes and closes
- [ ] `LayoutJournalReader`:
  - `events() -> impl Iterator<Item = (f64, LayoutEvent)>` — reads back
  - `layout_at(time: f64) -> Vec<PaneGeometry>` — reconstructs layout state at a given time by replaying events
- [ ] Timestamps relative to recording start (same base as `.cast` files)
- [ ] Snapshot written at recording start and periodically (configurable interval, default 5s)

## Implementation Notes

- Put in `src/recording/layout.rs`
- Layout query: `tmux list-panes -F '#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height} #{window_index}'`
- Periodic snapshots act as keyframes — the compositor can seek to any keyframe and replay forward, avoiding full replay from start
- The `layout_at()` function is used by the compositor (CAST-7), not the recording layer
