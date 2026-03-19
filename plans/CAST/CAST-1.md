# CAST-1: Terminal Recording & Compositing Epic

## Goal

Record all tmux pane output as per-pane asciicast v2 (`.cast`) files using `tmux pipe-pane`, maintain a layout event JSONL log, then composite them into a single `.cast` file showing the full tmux layout with borders using vt100 terminal emulation.

## Why

Terminal recordings today are single-pane. Real development happens across split panes — tests running alongside code alongside logs. Locus already orchestrates the workspace; recording is the missing observability layer. The composite output lets you replay an entire multi-pane session as a single asciinema-compatible file.

## Architecture

```
Phase 1: Recording Layer
┌──────────────────────────────────────────────────────┐
│  tmux pipe-pane -t %5 'cat >> /tmp/rec/pane-%5.raw'  │
│  tmux pipe-pane -t %8 'cat >> /tmp/rec/pane-%8.raw'  │
│                                                      │
│  RecordingState (in-memory)                          │
│    ├─ session_id, start_time                         │
│    ├─ per-pane writers (raw → .cast on flush/stop)   │
│    └─ layout event journal → layout.jsonl            │
│                                                      │
│  Activation methods:                                 │
│    record.start  → begins pipe-pane on all panes     │
│    record.stop   → closes pipes, finalizes .cast     │
│    record.status → is recording? which panes?        │
│    record.snapshot_layout → write layout event now   │
└──────────────────────────────────────────────────────┘

Phase 2: Compositor (locus-render)
┌──────────────────────────────────────────────────────┐
│  Inputs:                                             │
│    per-pane .cast files + layout.jsonl                │
│                                                      │
│  Processing:                                         │
│    1. Parse all .cast files into event timelines      │
│    2. Merge into global timeline (sorted by time)     │
│    3. For each frame:                                 │
│       a. Feed bytes to per-pane vt100::Parser         │
│       b. Read cell state from each parser's Screen    │
│       c. Composite into single frame buffer           │
│       d. Diff against previous frame → emit .cast "o" │
│                                                      │
│  Output:                                             │
│    single composite.cast (asciicast v2)               │
│    playable in asciinema, svg-term, etc.              │
└──────────────────────────────────────────────────────┘
```

## Dependency DAG

```
CAST-2 (asciicast types)
  │
  ├──→ CAST-3 (pipe-pane recording)
  │      │
  │      ├──→ CAST-4 (layout journal)
  │      │      │
  │      │      └──→ CAST-5 (recording activation) ──→ CAST-6 (pane lifecycle hooks)
  │      │
  │      └──→ CAST-5 (recording activation)
  │
  └──→ CAST-7 (vt100 compositor core)
         │
         ├──→ CAST-8 (frame differ)
         │      │
         │      └──→ CAST-9 (composite .cast writer)
         │
         └──→ CAST-9 (composite .cast writer)
                │
                └──→ CAST-10 (locus-render binary / activation)
```

## Phases

### Phase 1: Recording Layer (CAST-2 through CAST-6)

Record raw pane output via `tmux pipe-pane`, wrap in asciicast v2 format, and maintain a layout event journal. Unlocks: replay of individual panes.

### Phase 2: Compositor (CAST-7 through CAST-10)

Read per-pane `.cast` files + `layout.jsonl`, use vt100 crate for terminal emulation, render composite frames with pane borders, and output a single `.cast` file. Unlocks: full multi-pane replay.

## Key Design Decisions

1. **Raw capture first, .cast wrapping second**: `pipe-pane` outputs raw terminal bytes. We write them to `.raw` files with timestamps, then convert to `.cast` on stop. This avoids JSON encoding overhead during capture.

2. **Layout as event log, not snapshots**: `layout.jsonl` records layout *changes* (pane created, resized, closed, moved) as timestamped events. The compositor replays these to reconstruct layout state at any point in time.

3. **vt100 crate for terminal emulation**: Each pane gets its own `vt100::Parser` sized to the pane's dimensions. The compositor reads cell state from each parser's `Screen` and composes into a single buffer.

4. **Frame diffing for output efficiency**: Instead of writing the entire composite screen every frame, diff against the previous frame and emit only the ANSI escape sequences needed to update changed cells. This keeps `.cast` files small.

5. **tmux-only for Phase 1**: Zellij doesn't expose `pipe-pane` equivalent. Recording is tmux-specific. The compositor is backend-agnostic (reads `.cast` + `.jsonl`).

## Asciicast v2 Format Reference

```
Line 1 (header): {"version": 2, "width": 120, "height": 40, "timestamp": 1695000000}
Line 2+: [time, "o", "output bytes"]    ← stdout
          [time, "r", "COLSxROWS"]      ← resize
```

- `time` is seconds (float) since recording start
- `"o"` events contain raw terminal output (escape sequences included, JSON-escaped)
- File extension: `.cast`
