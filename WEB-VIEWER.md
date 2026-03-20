# Locus Web Viewer 🌐

Real-time terminal viewer in your browser. See all your tmux/zellij panes live!

## Quick Start

```bash
# Build and run
cargo build --bin plexus-locus-web
target/debug/plexus-locus-web

# Or specify port
target/debug/plexus-locus-web --port=8080

# Or run directly
cargo run --bin plexus-locus-web
```

Then open http://localhost:3000 in your browser!

## What It Shows

- **All active terminal panes** in a responsive grid layout
- **Live updates** - refreshes every 500ms
- **Terminal content** - exactly what's on each screen
- **Pane info** - IDs, names, sessions

## Architecture

### Backend (Rust)
- **Axum** HTTP server
- Queries tmux via backend API (`dump_screen`)
- Caches pane contents (100ms cache)
- REST API + Server-Sent Events (SSE)

### Frontend (Vanilla JS)
- Polls `/api/panes` for pane list
- Fetches content via `/api/pane/:id`
- Auto-updates every 500ms
- Responsive grid layout

## API Endpoints

### `GET /`
Serves the HTML frontend

### `GET /api/panes`
Returns list of all panes:
```json
[
  {
    "id": "%0",
    "name": "my-pane",
    "width": 80,
    "height": 24,
    "session": "main"
  }
]
```

### `GET /api/pane/:id`
Get specific pane content:
```json
{
  "id": "%0",
  "content": "terminal text here...",
  "html": "<div class=\"terminal-screen\">...</div>"
}
```

### `GET /api/stream` (SSE)
Server-Sent Events stream for all panes (experimental)

## Features

✅ **Multi-pane view** - All terminals in one screen
✅ **Live updates** - See output as it happens
✅ **Responsive** - Adapts to window size
✅ **Clean UI** - Terminal-style dark theme
✅ **No installation** - Just Rust + browser

## Future Enhancements

- [ ] WebSocket streaming (instead of polling)
- [ ] vt100 state integration for instant updates
- [ ] ANSI color support
- [ ] Click to focus pane
- [ ] Send input to panes
- [ ] Search/filter panes
- [ ] Layout save/restore
- [ ] Recording playback

## Technical Notes

### Performance
- Caches pane contents (100ms TTL)
- Uses temp files for screen capture (current limitation)
- Can handle ~10-20 panes smoothly

### With vt100 State (Future)
Once we integrate with the terminal state manager from CAST:
- **No temp files** - all in-memory
- **Instant updates** - <1ms latency
- **Event-driven** - only updates when content changes
- **Rich queries** - cursor position, colors, regions

## Development

### Project Structure
```
src/
├── bin/
│   └── locus-web.rs       # Web server binary
└── web/
    └── index.html         # Frontend UI
```

### Run in development
```bash
cargo watch -x 'run --bin plexus-locus-web'
```

### Build release
```bash
cargo build --release --bin plexus-locus-web
./target/release/plexus-locus-web
```

## Comparison

### vs tmux
- tmux: terminal-based, requires terminal access
- locus-web: browser-based, accessible from any device

### vs asciinema.org
- asciinema: recording playback
- locus-web: **live streaming** of active sessions

### vs screen sharing
- screen sharing: shares entire screen, high bandwidth
- locus-web: **text-only**, minimal bandwidth, perfect for terminals

## Use Cases

1. **Remote pair programming** - Share your terminal without screen sharing
2. **Monitoring** - Watch build/test output from phone
3. **Teaching** - Students can follow along in real-time
4. **CI/CD dashboards** - Live view of running jobs
5. **Debugging** - Watch multiple log streams simultaneously

## Contributing

This is a prototype! Ideas for improvements:
- Better ANSI escape sequence handling
- Click-to-focus pane integration
- Input streaming (type in browser → pane)
- Layout persistence
- Multi-user support (multiple viewers)

---

Built with ❤️ using Rust + Axum + vt100
