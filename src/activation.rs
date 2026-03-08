use async_stream::stream;
use futures::Stream;
use std::sync::Arc;

use crate::backend::TerminalBackend;
use crate::types::*;

/// Locus — terminal workspace orchestration activation.
///
/// Generic over backend: `Locus<Zellij>`, `Locus<Tmux>`, etc.
#[derive(Clone)]
pub struct Locus {
    backend: Arc<dyn TerminalBackend>,
}

impl Locus {
    pub fn new(backend: impl TerminalBackend) -> Self {
        Self {
            backend: Arc::new(backend),
        }
    }
}

#[plexus_macros::hub_methods(
    namespace = "locus",
    version = "0.1.0",
    description = "Terminal workspace orchestration with pluggable backends"
)]
impl Locus {
    // ========================================================================
    // Sessions
    // ========================================================================

    #[plexus_macros::hub_method(
        description = "List all terminal sessions",
    )]
    async fn list_sessions(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.list_sessions().await {
                Ok(sessions) => yield LocusEvent::Sessions { sessions },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Create a new terminal session",
        params(
            name = "Session name",
            layout = "Optional layout file path",
            cwd = "Working directory"
        )
    )]
    async fn create_session(
        &self,
        name: String,
        layout: Option<String>,
        cwd: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let opts = SessionOpts {
                name,
                layout,
                cwd: cwd.map(Into::into),
            };
            match backend.create_session(&opts).await {
                Ok(session) => yield LocusEvent::SessionCreated { session },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Kill a terminal session",
        params(name = "Session name to kill")
    )]
    async fn kill_session(
        &self,
        name: String,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.kill_session(&name).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Killed session: {}", name) },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    // ========================================================================
    // Tabs
    // ========================================================================

    #[plexus_macros::hub_method(
        description = "List tabs in a session",
        params(session = "Target session (default: current)")
    )]
    async fn list_tabs(
        &self,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.list_tabs(session.as_deref()).await {
                Ok(tabs) => yield LocusEvent::Tabs { tabs },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Create a new tab",
        params(
            name = "Tab name",
            layout = "Layout file path",
            session = "Target session (default: current)"
        )
    )]
    async fn create_tab(
        &self,
        name: Option<String>,
        layout: Option<String>,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let opts = TabOpts {
                name,
                layout,
                cwd: None,
                session,
            };
            match backend.create_tab(&opts).await {
                Ok(tab) => yield LocusEvent::TabCreated { tab },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Focus a tab by index",
        params(
            index = "Tab index (1-based)",
            session = "Target session (default: current)"
        )
    )]
    async fn focus_tab(
        &self,
        index: u32,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.focus_tab(session.as_deref(), index).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Focused tab {}", index) },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    // ========================================================================
    // Panes
    // ========================================================================

    #[plexus_macros::hub_method(
        description = "Create a new pane",
        params(
            name = "Pane name for tracking",
            command = "Command to run",
            cwd = "Working directory",
            direction = "Split direction: up, down, left, right",
            floating = "Open as floating pane",
            session = "Target session (default: current)"
        )
    )]
    async fn create_pane(
        &self,
        name: Option<String>,
        command: Option<String>,
        cwd: Option<String>,
        direction: Option<String>,
        floating: Option<bool>,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let dir = direction.and_then(|d| match d.as_str() {
                "up" => Some(Direction::Up),
                "down" => Some(Direction::Down),
                "left" => Some(Direction::Left),
                "right" => Some(Direction::Right),
                _ => None,
            });

            let opts = PaneOpts {
                name,
                command,
                cwd: cwd.map(Into::into),
                direction: dir,
                floating: floating.unwrap_or(false),
                close_on_exit: false,
                session,
                tab: None,
            };
            match backend.create_pane(&opts).await {
                Ok(pane) => yield LocusEvent::PaneCreated { pane },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Move focus to an adjacent pane",
        params(direction = "Direction: up, down, left, right")
    )]
    async fn focus_pane(
        &self,
        direction: String,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let dir = match direction.as_str() {
                "up" => Direction::Up,
                "down" => Direction::Down,
                "left" => Direction::Left,
                "right" => Direction::Right,
                _ => {
                    yield LocusEvent::Error { message: format!("Invalid direction: {}", direction) };
                    return;
                }
            };
            match backend.focus_pane(dir).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Focused {}", direction) },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Close the currently focused pane",
    )]
    async fn close_pane(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.close_pane().await {
                Ok(()) => yield LocusEvent::Ok { message: "Pane closed".into() },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Toggle floating panes visibility",
    )]
    async fn toggle_floating(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.toggle_floating().await {
                Ok(()) => yield LocusEvent::Ok { message: "Toggled floating".into() },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Toggle fullscreen for focused pane",
    )]
    async fn toggle_fullscreen(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.toggle_fullscreen().await {
                Ok(()) => yield LocusEvent::Ok { message: "Toggled fullscreen".into() },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    // ========================================================================
    // Input / Output
    // ========================================================================

    #[plexus_macros::hub_method(
        description = "Send keystrokes to the focused pane",
        params(
            chars = "Characters to send (use \\n for enter)",
            session = "Target session (default: current)"
        )
    )]
    async fn write_chars(
        &self,
        chars: String,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let len = chars.len() as u32;
            match backend.write_chars(&chars, session.as_deref()).await {
                Ok(()) => yield LocusEvent::InputSent {
                    pane: PaneId("focused".into()),
                    chars: len,
                },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Capture the screen content of the current pane",
        params(
            full = "Include full scrollback history (default: false)"
        )
    )]
    async fn capture(
        &self,
        full: Option<bool>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let tmp = format!("/tmp/locus-capture-{}", uuid::Uuid::new_v4());
            match backend.dump_screen(&tmp, full.unwrap_or(false)).await {
                Ok(content) => {
                    let lines = content.lines().count() as u32;
                    yield LocusEvent::ScreenCapture {
                        pane: PaneId("focused".into()),
                        content,
                        lines,
                    };
                    // Clean up temp file
                    let _ = tokio::fs::remove_file(&tmp).await;
                }
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Dump the current layout definition",
    )]
    async fn dump_layout(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.dump_layout().await {
                Ok(content) => yield LocusEvent::Layout { content },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    // ========================================================================
    // Run
    // ========================================================================

    #[plexus_macros::hub_method(
        description = "Run a command in a new pane",
        params(
            command = "Command to execute",
            name = "Pane name for tracking",
            cwd = "Working directory",
            direction = "Split direction: up, down, left, right",
            floating = "Open as floating pane",
            close_on_exit = "Close pane when command exits",
            session = "Target session (default: current)"
        )
    )]
    async fn run(
        &self,
        command: String,
        name: Option<String>,
        cwd: Option<String>,
        direction: Option<String>,
        floating: Option<bool>,
        close_on_exit: Option<bool>,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let dir = direction.and_then(|d| match d.as_str() {
                "up" => Some(Direction::Up),
                "down" => Some(Direction::Down),
                "left" => Some(Direction::Left),
                "right" => Some(Direction::Right),
                _ => None,
            });

            let opts = RunOpts {
                command: command.clone(),
                name,
                cwd: cwd.map(Into::into),
                direction: dir,
                floating: floating.unwrap_or(false),
                close_on_exit: close_on_exit.unwrap_or(false),
                session,
            };
            match backend.run_command(&opts).await {
                Ok(pane) => yield LocusEvent::CommandLaunched {
                    pane: pane.id,
                    command,
                },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    // ========================================================================
    // Meta
    // ========================================================================

    #[plexus_macros::hub_method(
        description = "Check which terminal backend is active and if it's available",
    )]
    async fn status(&self) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let available = backend.is_available().await;
            let name = backend.name().to_string();
            if available {
                yield LocusEvent::Ok { message: format!("Backend '{}' is available", name) };
            } else {
                yield LocusEvent::Error { message: format!("Backend '{}' is not available", name) };
            }
        }
    }
}
