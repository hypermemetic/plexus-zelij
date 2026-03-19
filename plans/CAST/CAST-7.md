# CAST-7: vt100 Compositor Core

blocked_by: [CAST-2]
unlocks: [CAST-8, CAST-9]

## Scope

The core compositor that reads per-pane `.cast` files and a `layout.jsonl`, maintains a `vt100::Parser` per pane, and produces composite frame buffers representing the full tmux layout with borders.

## Acceptance Criteria

- [ ] `Compositor` struct:
  - `new(recording_dir: &Path)` — discovers `.cast` files and `layout.jsonl`
  - `build_timeline()` — merges all pane events into a single sorted timeline
  - `render_frame(time: f64) -> CompositeFrame` — produce a composite frame at a given timestamp
- [ ] `PaneState` struct: holds `vt100::Parser` per pane, sized to pane dimensions
  - Feeds raw bytes from `.cast` output events into the parser
  - Handles resize events by creating a new parser with new dimensions
- [ ] `CompositeFrame` struct:
  - `width: u16, height: u16` — total terminal dimensions (computed from layout)
  - `cells: Vec<Vec<Cell>>` — 2D grid of cells with char + style
  - `render_ansi() -> String` — render the frame as ANSI escape sequences
- [ ] Layout reconstruction: uses `LayoutJournalReader::layout_at(time)` to get pane positions
- [ ] Border drawing: single-line box-drawing characters (`│`, `─`, `┌`, `┐`, `└`, `┘`, `├`, `┤`, `┬`, `┴`, `┼`) between panes
- [ ] Composite dimensions: computed from layout (max right edge + max bottom edge + border widths)
- [ ] Timeline merging: events from all `.cast` files + layout events interleaved by timestamp

## Implementation Notes

- Put in `src/compositor/mod.rs`, `src/compositor/pane_state.rs`, `src/compositor/frame.rs`
- `vt100::Parser::new(rows, cols, scrollback)` — scrollback=0 for compositor (we don't need it)
- `parser.process(bytes)` — feed raw output
- `parser.screen().cell(row, col)` — read cell state (char, fg, bg, bold, etc.)
- Total dimensions formula: `width = max(pane.x + pane.width for all panes) + border_adjustments`, similar for height
- Cell struct should capture: `char`, `fg: Color`, `bg: Color`, `bold: bool`, `underline: bool`, `inverse: bool`
- Color enum: `Default`, `Indexed(u8)`, `Rgb(u8, u8, u8)` — map from vt100's color types
- The timeline is a `Vec<(f64, TimelineEvent)>` where `TimelineEvent` is either `PaneOutput { pane_id, bytes }` or `LayoutChange { event }`
