# CAST-9: Composite .cast Writer

blocked_by: [CAST-7, CAST-8]
unlocks: [CAST-10]

## Scope

Orchestrate the compositor pipeline end-to-end: read per-pane `.cast` files and `layout.jsonl`, drive the compositor through the merged timeline, diff consecutive frames, and write a single composite `.cast` file playable in asciinema / svg-term.

## Acceptance Criteria

- [ ] `CompositeWriter` struct:
  - `new(recording_dir: &Path, output_path: &Path, opts: CompositeOpts)` — set up compositor + cast writer
  - `run()` — drive the full pipeline: build timeline → iterate events → render frames → diff → write
  - Returns `CompositeResult { output_path, duration_secs, frame_count, total_bytes }`
- [ ] `CompositeOpts`:
  - `fps: f64` — maximum frame rate (default 30.0). Coalesce events within the same frame window into a single composite frame rather than emitting per-event
  - `idle_time_limit: Option<f64>` — cap gaps between events (default 2.0s). Long pauses compressed
  - `border_style: BorderStyle` — `Single` (default), `Double`, `Heavy`, `None`
  - `title: Option<String>` — written to `.cast` header
  - `theme: Option<CastTheme>` — optional color theme in header
- [ ] Frame rate limiting: accumulate pane output events within a 1/fps window, then render one composite frame. Avoids thousands of frames for bursts of output
- [ ] Idle compression: if the gap between two events exceeds `idle_time_limit`, clamp it. Write an `"o"` event with the clamped timestamp
- [ ] First frame is always a full render (`render_full_frame`). Subsequent frames use `diff_frames`
- [ ] Resize handling: if layout changes dimensions, emit an `"r"` event in the composite `.cast` followed by a full re-render
- [ ] Header: `version: 2`, `width`/`height` from initial layout, `timestamp` from recording start
- [ ] Flushes incrementally — don't buffer the entire composite in memory
- [ ] Progress reporting: yield periodic status (e.g., percentage through timeline) for long recordings

## Implementation Notes

- Put in `src/compositor/writer.rs`
- Pipeline pseudocode:
  ```
  let compositor = Compositor::new(recording_dir)?;
  let timeline = compositor.build_timeline()?;
  let mut cast = CastWriter::new(output_path)?;
  cast.write_header(header)?;
  let mut prev_frame: Option<CompositeFrame> = None;
  let mut frame_time = 0.0;
  let mut pending_events = Vec::new();

  for (time, event) in timeline {
      pending_events.push((time, event));
      if time - frame_time < 1.0 / opts.fps {
          continue; // accumulate
      }
      // Process all pending events
      for (_, ev) in pending_events.drain(..) {
          compositor.apply_event(ev);
      }
      let frame = compositor.render_frame(time);
      let output = match &prev_frame {
          None => render_full_frame(&frame),
          Some(prev) => {
              if prev.dimensions() != frame.dimensions() {
                  // resize event + full render
              } else {
                  diff_frames(prev, &frame)
              }
          }
      };
      if !output.is_empty() {
          cast.write_event(CastEvent::Output(clamped_time, output))?;
      }
      prev_frame = Some(frame);
      frame_time = time;
  }
  ```
- Memory: only two frames in memory at a time (prev + current). Per-pane `vt100::Parser` state is the main memory cost (~rows*cols*cell_size per pane)
- For a typical 4-pane 200x50 terminal: ~4 * 200 * 50 * ~32 bytes ≈ 1.2 MB. Negligible
