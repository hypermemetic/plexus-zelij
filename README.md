# Locus

Terminal workspace orchestration over Plexus RPC.

Locus is a standalone Plexus RPC server that provides higher-level terminal workspace control through a pluggable backend abstraction. Zellij is the first backend; tmux, WezTerm, or anything else can be added by implementing the `TerminalBackend` trait.

## Why

Terminal multiplexers have good CLIs. What they don't have:

- **Named pane identity** that survives re-layouts
- **Semantic workspace operations** ("set up a Rust dev workspace" as one call)
- **Structured streaming output** for agent consumption
- **Backend-agnostic abstraction** (swap Zellij for tmux without changing callers)

Locus sits above the multiplexer and exposes these capabilities over Plexus RPC, making terminal workspaces controllable by both humans (via Synapse CLI) and agents.

## Architecture

```
┌────────────────────────────┐
│  Synapse CLI / Agents      │
├────────────────────────────┤
│  Locus (Plexus RPC)        │  ← DynamicHub on port 4448
│    └─ TerminalBackend      │  ← trait
│         └─ Zellij impl     │  ← shells out to `zellij` CLI
├────────────────────────────┤
│  plexus-transport          │  ← WebSocket / stdio / MCP HTTP
└────────────────────────────┘
```

Locus runs as its own process with its own DynamicHub. It is not hosted on Substrate.

## Build & Run

```bash
cargo build
cargo run                     # WebSocket on port 4448
cargo run -- --port 4450      # Custom port
cargo run -- --stdio          # MCP-compatible stdio mode
```

## Usage via Synapse

```bash
# Check backend
synapse -P 4448 locus status

# Sessions
synapse -P 4448 locus list_sessions
synapse -P 4448 locus create_session --name myproject
synapse -P 4448 locus kill_session --name myproject

# Tabs
synapse -P 4448 locus list_tabs
synapse -P 4448 locus create_tab --name testing
synapse -P 4448 locus focus_tab --index 2

# Panes
synapse -P 4448 locus create_pane --name editor --direction right
synapse -P 4448 locus focus_pane --direction left
synapse -P 4448 locus close_pane
synapse -P 4448 locus toggle_floating
synapse -P 4448 locus toggle_fullscreen

# Run commands
synapse -P 4448 locus run --command "cargo test" --floating --name tests
synapse -P 4448 locus run --command "tail -f app.log" --direction down --close-on-exit

# I/O
synapse -P 4448 locus write_chars --chars "cargo build\n"
synapse -P 4448 locus capture --full
synapse -P 4448 locus dump_layout
```

## Adding a Backend

Implement `TerminalBackend` in `src/backends/`:

```rust
#[async_trait]
pub trait TerminalBackend: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn is_available(&self) -> bool;

    // Sessions
    async fn list_sessions(&self) -> BackendResult<Vec<Session>>;
    async fn create_session(&self, opts: &SessionOpts) -> BackendResult<Session>;
    async fn kill_session(&self, name: &str) -> BackendResult<()>;

    // Tabs
    async fn list_tabs(&self, session: Option<&str>) -> BackendResult<Vec<Tab>>;
    async fn create_tab(&self, opts: &TabOpts) -> BackendResult<Tab>;
    async fn close_tab(&self, session: Option<&str>, index: u32) -> BackendResult<()>;
    async fn focus_tab(&self, session: Option<&str>, index: u32) -> BackendResult<()>;
    async fn rename_tab(&self, session: Option<&str>, index: u32, name: &str) -> BackendResult<()>;

    // Panes
    async fn create_pane(&self, opts: &PaneOpts) -> BackendResult<Pane>;
    async fn close_pane(&self) -> BackendResult<()>;
    async fn focus_pane(&self, direction: Direction) -> BackendResult<()>;
    async fn rename_pane(&self, name: &str) -> BackendResult<()>;
    async fn toggle_floating(&self) -> BackendResult<()>;
    async fn toggle_fullscreen(&self) -> BackendResult<()>;
    async fn resize_pane(&self, direction: Direction, amount: Option<u32>) -> BackendResult<()>;

    // I/O
    async fn write_chars(&self, chars: &str, session: Option<&str>) -> BackendResult<()>;
    async fn dump_screen(&self, path: &str, full_scrollback: bool) -> BackendResult<String>;
    async fn dump_layout(&self) -> BackendResult<String>;

    // Run
    async fn run_command(&self, opts: &RunOpts) -> BackendResult<Pane>;
}
```

Then swap the constructor in `main.rs`:

```rust
let locus = Locus::new(YourBackend::new());
```

## Source Layout

```
src/
  main.rs            — Binary: DynamicHub + TransportServer
  lib.rs             — Crate root, re-exports
  backend.rs         — TerminalBackend trait (the abstraction)
  types.rs           — Pane, Tab, Session, LocusEvent, option types
  activation.rs      — Locus Plexus activation (#[hub_methods])
  backends/
    mod.rs
    zellij.rs        — Zellij implementation (CLI-based)
```

## License

AGPL-3.0-only
