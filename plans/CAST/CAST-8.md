# CAST-8: Frame Differ

blocked_by: [CAST-7]
unlocks: [CAST-9]

## Scope

Diff two consecutive `CompositeFrame`s and emit the minimal ANSI escape sequence string to update from the old frame to the new one. This keeps composite `.cast` files small by only encoding changes.

## Acceptance Criteria

- [ ] `diff_frames(prev: &CompositeFrame, next: &CompositeFrame) -> String`
  - Compares cell-by-cell
  - Emits cursor movement + attribute changes + character writes only for changed cells
  - Coalesces adjacent changed cells into single writes (avoid per-cell cursor moves)
  - Handles attribute changes efficiently (only emit SGR codes when style actually changes)
- [ ] `render_full_frame(frame: &CompositeFrame) -> String`
  - Renders the entire frame from scratch (used for first frame / after resize)
  - Moves cursor to 0,0, clears screen, writes all cells
- [ ] Handles dimension changes between frames (full re-render if size changed)
- [ ] SGR attribute encoding:
  - `\x1b[0m` reset, `\x1b[1m` bold, `\x1b[4m` underline, `\x1b[7m` inverse
  - `\x1b[38;5;Nm` / `\x1b[38;2;R;G;Bm` for fg color
  - `\x1b[48;5;Nm` / `\x1b[48;2;R;G;Bm` for bg color
- [ ] Cursor positioning: `\x1b[{row};{col}H`
- [ ] Unit tests: identical frames → empty diff, single cell change, full row change, color transitions

## Implementation Notes

- Put in `src/compositor/diff.rs`
- Track "current style state" during diff to avoid redundant SGR codes
- Run-length optimization: if a run of N consecutive cells changed in the same row, emit one cursor move + the full run
- Consider dirty-rectangle approach: find bounding box of changes, but cell-by-cell is simpler and likely sufficient
