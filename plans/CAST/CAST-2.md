# CAST-2: Asciicast v2 Types & I/O

blocked_by: []
unlocks: [CAST-3, CAST-7]

## Scope

Define Rust types for the asciicast v2 format and provide read/write functions. This is the shared foundation used by both the recording layer (writes per-pane `.cast`) and the compositor (reads per-pane `.cast`, writes composite `.cast`).

## Acceptance Criteria

- [ ] `CastHeader` struct with all v2 fields (version, width, height, timestamp, env, title, idle_time_limit, theme)
- [ ] `CastEvent` enum: `Output(f64, String)`, `Input(f64, String)`, `Resize(f64, u16, u16)`, `Marker(f64, String)`
- [ ] `CastWriter` — append-only writer: `write_header()`, `write_event()`, `flush()`, `finish()`
- [ ] `CastReader` — streaming reader: `header()`, `events() -> impl Iterator<Item = CastEvent>`
- [ ] Correct JSON escaping of non-printable characters in output strings
- [ ] Unit tests: roundtrip write→read, edge cases (empty output, unicode, escape sequences)

## Implementation Notes

- Put in `src/cast/mod.rs` (new module)
- `CastWriter` wraps `BufWriter<File>`, writes NDJSON
- `CastReader` wraps `BufReader<File>`, parses line-by-line
- Times are `f64` seconds relative to recording start
- Header `timestamp` is Unix epoch (integer)
- No schemars derive needed — these are file format types, not RPC types
