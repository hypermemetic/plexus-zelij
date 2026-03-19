use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use std::sync::Arc;

use crate::backend::TerminalBackend;
use crate::plexus::{ChildRouter, PlexusError, PlexusStream, Activation};
use crate::types::*;

/// Shell-escape a string for safe inclusion in a bash script
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Resolve an optional PaneRef to a tmux %id, returning (resolved_id, target_for_backend)
async fn resolve_pane_opt(
    backend: &std::sync::Arc<dyn crate::backend::TerminalBackend>,
    pane: &Option<PaneRef>,
) -> Result<(String, Option<String>), String> {
    match pane {
        Some(p) => match backend.resolve_pane(p.as_str()).await {
            Ok(id) => {
                let target = Some(id.clone());
                Ok((id, target))
            }
            Err(e) => Err(e.to_string()),
        },
        None => Ok(("focused".into(), None)),
    }
}

/// Resolve a required PaneRef
async fn resolve_pane_req(
    backend: &std::sync::Arc<dyn crate::backend::TerminalBackend>,
    pane: &PaneRef,
) -> Result<(String, String), String> {
    match backend.resolve_pane(pane.as_str()).await {
        Ok(id) => Ok((id.clone(), id)),
        Err(e) => Err(e.to_string()),
    }
}

/// Panes sub-activation — manages terminal panes with targeting.
///
/// Accessed as `locus.panes.list`, `locus.panes.create`, etc.
/// Supports pane targeting via name or `%id`.
#[derive(Clone)]
pub struct PanesActivation {
    pub(crate) backend: Arc<dyn TerminalBackend>,
}

impl PanesActivation {
    pub fn new(backend: Arc<dyn TerminalBackend>) -> Self {
        Self { backend }
    }
}

#[plexus_macros::hub_methods(
    namespace = "panes",
    version = "0.1.0",
    description = "Terminal pane management with targeting"
)]
impl PanesActivation {
    #[plexus_macros::hub_method(
        description = "List all panes",
        params(
            session = "Filter by session (default: all sessions)",
            tab = "Filter by tab/window"
        )
    )]
    async fn list(
        &self,
        session: Option<String>,
        tab: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            match backend.list_panes(session.as_deref(), tab.as_deref()).await {
                Ok(panes) => yield LocusEvent::Panes { panes },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Create a new pane",
        params(
            name = "Pane name for tracking",
            command = "Command to run",
            cwd = "Working directory",
            direction = "Split direction: up, down, left, right",
            floating = "Open as floating pane (zellij only)",
            session = "Target session (default: current)",
            target = "Pane ID to split from (e.g. %5). Default: focused pane"
        )
    )]
    async fn create(
        &self,
        name: Option<String>,
        command: Option<String>,
        cwd: Option<String>,
        direction: Option<String>,
        floating: Option<bool>,
        session: Option<String>,
        target: Option<String>,
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
                target,
            };
            match backend.create_pane(&opts).await {
                Ok(pane) => yield LocusEvent::PaneCreated { pane },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Close a pane",
        params(pane = "Pane name or ID (e.g. 'my-pane' or '%5'). Default: focused pane")
    )]
    async fn close(
        &self,
        pane: Option<PaneRef>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let (_, target) = match resolve_pane_opt(&backend, &pane).await {
                Ok(v) => v, Err(e) => { yield LocusEvent::Error { message: e }; return; }
            };
            match backend.close_pane(target.as_deref()).await {
                Ok(()) => yield LocusEvent::Ok { message: "Pane closed".into() },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Move focus to an adjacent pane",
        params(direction = "Direction: up, down, left, right")
    )]
    async fn focus(
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
        description = "Rename a pane",
        params(
            name = "New pane name/title",
            pane = "Pane name or ID to rename (default: focused)"
        )
    )]
    async fn rename(
        &self,
        name: String,
        pane: Option<PaneRef>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let (_, target) = match resolve_pane_opt(&backend, &pane).await {
                Ok(v) => v, Err(e) => { yield LocusEvent::Error { message: e }; return; }
            };
            match backend.rename_pane(&name, target.as_deref()).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Renamed pane to {}", name) },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Toggle floating panes visibility (zellij only)"
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
        description = "Toggle fullscreen/zoom for a pane"
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

    #[plexus_macros::hub_method(
        description = "Resize a pane",
        params(
            direction = "Direction: up, down, left, right",
            amount = "Number of cells to resize (default: 5)",
            pane = "Pane ID to resize (default: focused)"
        )
    )]
    async fn resize(
        &self,
        direction: String,
        amount: Option<u32>,
        pane: Option<PaneRef>,
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
            let (_, target) = match resolve_pane_opt(&backend, &pane).await {
                Ok(v) => v, Err(e) => { yield LocusEvent::Error { message: e }; return; }
            };
            match backend.resize_pane(dir, amount, target.as_deref()).await {
                Ok(()) => yield LocusEvent::Ok { message: format!("Resized pane {}", direction) },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Send keystrokes to a pane",
        params(
            chars = "Characters to send (use \\n for enter)",
            pane = "Pane ID to target (default: focused)",
            session = "Target session (default: current)"
        )
    )]
    async fn write(
        &self,
        chars: String,
        pane: Option<PaneRef>,
        session: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let (pane_id, target) = match resolve_pane_opt(&backend, &pane).await {
                Ok(v) => v, Err(e) => { yield LocusEvent::Error { message: e }; return; }
            };
            let len = chars.len() as u32;
            match backend.write_chars(&chars, session.as_deref(), target.as_deref()).await {
                Ok(()) => yield LocusEvent::InputSent {
                    pane: PaneId(pane_id),
                    chars: len,
                },
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Send a command to any shell (including containers, ssh, etc.) and return the screen diff. Works everywhere — no hooks needed, pure screen observation.",
        params(
            command = "Command text to send (Enter is appended automatically)",
            pane = "Pane name or ID (e.g. 'my-container' or '%5')",
            settle_ms = "Time to wait after screen stops changing (default: 500ms)",
            timeout_ms = "Max time to wait for output to settle (default: 10000ms)"
        )
    )]
    async fn send(
        &self,
        command: String,
        pane: Option<PaneRef>,
        settle_ms: Option<u64>,
        timeout_ms: Option<u64>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let settle = std::time::Duration::from_millis(settle_ms.unwrap_or(500));
            let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(10000));

            let (pane_id, target) = match resolve_pane_opt(&backend, &pane).await {
                Ok(v) => v, Err(e) => { yield LocusEvent::Error { message: e }; return; }
            };
            let pane_target = target.as_deref();

            // Capture before
            let before = {
                let tmp = format!("/tmp/locus-capture-{}", uuid::Uuid::new_v4());
                match backend.dump_screen(&tmp, false, pane_target).await {
                    Ok(content) => { let _ = tokio::fs::remove_file(&tmp).await; content }
                    Err(e) => { yield LocusEvent::Error { message: e.to_string() }; return; }
                }
            };
            let before_line_count = before.lines().count();

            // Send command + Enter
            if let Err(e) = backend.write_chars(&command, None, pane_target).await {
                yield LocusEvent::Error { message: e.to_string() };
                return;
            }
            if let Err(e) = backend.write_chars("Enter", None, pane_target).await {
                yield LocusEvent::Error { message: e.to_string() };
                return;
            }

            // Wait for quiescence: screen stops changing for `settle` duration
            let start = std::time::Instant::now();
            let mut last_content = before.clone();
            let mut last_change = std::time::Instant::now();

            // Small initial delay for command to start producing output
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                if start.elapsed() > timeout {
                    break; // Return whatever we have
                }

                let current = {
                    let tmp = format!("/tmp/locus-capture-{}", uuid::Uuid::new_v4());
                    match backend.dump_screen(&tmp, false, pane_target).await {
                        Ok(content) => { let _ = tokio::fs::remove_file(&tmp).await; content }
                        Err(_) => break,
                    }
                };

                if current != last_content {
                    last_content = current;
                    last_change = std::time::Instant::now();
                } else if last_change.elapsed() >= settle {
                    break; // Screen settled
                }
            }

            // Diff: find new lines (after the before content)
            let before_lines: Vec<&str> = before.lines().collect();
            let after_lines: Vec<&str> = last_content.lines().collect();
            let after_line_count = after_lines.len();

            // Find where new content starts by matching the tail of `before`
            // against the beginning of `after` (screen scrolls)
            let new_content = if after_line_count > before_line_count {
                // Simple case: screen grew — new lines are at the end
                after_lines[before_line_count..].join("\n")
            } else {
                // Screen same size or smaller (scrolled) — diff the content
                let mut first_diff = 0;
                for (i, (a, b)) in before_lines.iter().zip(after_lines.iter()).enumerate() {
                    if a != b {
                        first_diff = i;
                        break;
                    }
                    first_diff = i + 1;
                }
                after_lines[first_diff..].join("\n")
            };

            yield LocusEvent::ScreenDiff {
                pane: PaneId(pane_id),
                before_lines: before_line_count as u32,
                after_lines: after_line_count as u32,
                new_content,
            };
        }
    }

    #[plexus_macros::hub_method(
        description = "Capture the screen content of a pane",
        params(
            full = "Include full scrollback history (default: false)",
            pane = "Pane ID to capture (default: focused)"
        )
    )]
    async fn capture(
        &self,
        full: Option<bool>,
        pane: Option<PaneRef>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let (pane_id, target) = match resolve_pane_opt(&backend, &pane).await {
                Ok(v) => v, Err(e) => { yield LocusEvent::Error { message: e }; return; }
            };
            let tmp = format!("/tmp/locus-capture-{}", uuid::Uuid::new_v4());
            match backend.dump_screen(&tmp, full.unwrap_or(false), target.as_deref()).await {
                Ok(content) => {
                    let lines = content.lines().count() as u32;
                    yield LocusEvent::ScreenCapture {
                        pane: PaneId(pane_id),
                        content,
                        lines,
                    };
                    let _ = tokio::fs::remove_file(&tmp).await;
                }
                Err(e) => yield LocusEvent::Error { message: e.to_string() },
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Execute a command in an existing pane via temp script (avoids send-keys length/quoting issues). Waits for the command to start and optionally streams until completion.",
        params(
            command = "Command to execute (can be arbitrarily long, handles all quoting)",
            pane = "Pane name or ID (e.g. 'my-pane' or '%5'). Default: focused pane",
            cwd = "Working directory to cd into before running",
            name = "Set pane title (for future lookup by name)",
            timeout_ms = "Max time to wait for command to start (default: 5000ms)",
            capture_lines = "Number of lines to capture (default: 0, no capture)",
            wait = "Keep stream open until command exits (default: false)"
        )
    )]
    async fn exec(
        &self,
        command: String,
        pane: Option<PaneRef>,
        cwd: Option<String>,
        name: Option<String>,
        timeout_ms: Option<u64>,
        capture_lines: Option<u32>,
        wait: Option<bool>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(5000));
            let capture_count = capture_lines.unwrap_or(0);
            let wait_for_exit = wait.unwrap_or(false);

            let (pane_id, target) = match resolve_pane_opt(&backend, &pane).await {
                Ok(v) => v, Err(e) => { yield LocusEvent::Error { message: e }; return; }
            };
            let pane_target = target.as_deref();

            // Set pane title if requested
            if let Some(ref title) = name {
                let _ = backend.rename_pane(title, pane_target).await;
            }

            // State file: keyed by pane ID so `poll` can query later.
            // Each exec overwrites the previous state for that pane.
            let state_dir = "/tmp/plexus_locus_exec_state";
            let _ = tokio::fs::create_dir_all(state_dir).await;
            let exec_id = uuid::Uuid::new_v4();
            // Pane IDs start with % — strip it for filename safety
            let safe_pane = pane_id.replace('%', "pane_");
            let state_file = format!("{}/{}", state_dir, safe_pane);

            // Build the wrapper script:
            // 1. Writes "started" to state file (shell hook: preexec equivalent)
            // 2. Runs the user command
            // 3. Writes "exited:<exit_code>" to state file (shell hook: precmd equivalent)
            // 4. Cleans up
            let script_path = format!("/tmp/plexus_locus_exec_script_{}.sh", exec_id);
            let mut script = String::new();
            script.push_str("#!/bin/bash\n");
            script.push_str(&format!(
                "__PLEXUS_LOCUS_EXEC_STATE_FILE={}\n", shell_escape(&state_file)
            ));
            script.push_str(&format!(
                "__PLEXUS_LOCUS_EXEC_SCRIPT_FILE={}\n", shell_escape(&script_path)
            ));
            // Self-delete the script
            script.push_str("rm -f \"$__PLEXUS_LOCUS_EXEC_SCRIPT_FILE\"\n");
            if let Some(ref dir) = cwd {
                script.push_str(&format!("cd {} || {{ echo \"exited:1\" > \"$__PLEXUS_LOCUS_EXEC_STATE_FILE\"; exit 1; }}\n", shell_escape(dir)));
            }
            // Signal: command is starting (heredoc avoids all quoting issues)
            script.push_str("cat > \"$__PLEXUS_LOCUS_EXEC_STATE_FILE\" << '__PLEXUS_LOCUS_STATE_EOF__'\n");
            script.push_str("started\n");
            script.push_str(&command);
            script.push_str("\n__PLEXUS_LOCUS_STATE_EOF__\n");
            // Run the command (not exec — we need the exit code)
            script.push_str(&command);
            script.push('\n');
            // Signal: command finished with exit code
            script.push_str("__PLEXUS_LOCUS_EXEC_EXIT_CODE=$?\n");
            script.push_str("cat > \"$__PLEXUS_LOCUS_EXEC_STATE_FILE\" << __PLEXUS_LOCUS_STATE_EOF__\n");
            script.push_str("exited:$__PLEXUS_LOCUS_EXEC_EXIT_CODE\n");
            script.push_str(&command);
            script.push_str("\n__PLEXUS_LOCUS_STATE_EOF__\n");
            script.push_str("exit $__PLEXUS_LOCUS_EXEC_EXIT_CODE\n");

            if let Err(e) = tokio::fs::write(&script_path, &script).await {
                yield LocusEvent::Error { message: format!("Failed to write exec script: {}", e) };
                return;
            }

            // Send short command via send-keys
            let exec_cmd = format!("bash {}", &script_path);
            if let Err(e) = backend.write_chars(&exec_cmd, None, pane_target).await {
                let _ = tokio::fs::remove_file(&script_path).await;
                yield LocusEvent::Error { message: e.to_string() };
                return;
            }
            if let Err(e) = backend.write_chars("Enter", None, pane_target).await {
                let _ = tokio::fs::remove_file(&script_path).await;
                yield LocusEvent::Error { message: e.to_string() };
                return;
            }

            // Helper: read state file and parse
            let read_state = |sf: &str| -> Option<(String, Option<i32>)> {
                let content = std::fs::read_to_string(sf).ok()?;
                let state_line = content.lines().next()?.trim().to_string();
                if state_line == "started" {
                    Some(("started".into(), None))
                } else if let Some(code_str) = state_line.strip_prefix("exited:") {
                    Some(("exited".into(), code_str.parse().ok()))
                } else {
                    None
                }
            };

            // Helper: capture screen tail
            let do_capture = |backend: &Arc<dyn TerminalBackend>, pane_target: Option<&str>, count: u32| {
                let backend = backend.clone();
                let pane_target = pane_target.map(|s| s.to_string());
                async move {
                    let tmp = format!("/tmp/locus-capture-{}", uuid::Uuid::new_v4());
                    match backend.dump_screen(&tmp, false, pane_target.as_deref()).await {
                        Ok(content) => {
                            let _ = tokio::fs::remove_file(&tmp).await;
                            let lines: Vec<&str> = content.lines().collect();
                            let tail = lines.len().saturating_sub(count as usize);
                            Some(lines[tail..].join("\n"))
                        }
                        Err(_) => None,
                    }
                }
            };

            // Phase 1: wait for command to start
            let start = std::time::Instant::now();
            let mut started = false;
            let mut exited = false;
            let mut exit_code: Option<i32> = None;
            let mut pane_gone = false;
            let mut poll_count = 0u32;

            while start.elapsed() < timeout {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                poll_count += 1;

                if let Some((state, code)) = read_state(&state_file) {
                    match state.as_str() {
                        "started" => { started = true; break; }
                        "exited" => { started = true; exited = true; exit_code = code; break; }
                        _ => {}
                    }
                }

                // Check pane still exists every ~500ms
                if poll_count % 25 == 0 && !backend.pane_exists(&pane_id).await {
                    pane_gone = true;
                    break;
                }
            }

            if pane_gone {
                yield LocusEvent::Error {
                    message: format!("Pane {} was destroyed before command started", pane_id),
                };
                return;
            }

            if !started {
                yield LocusEvent::Error {
                    message: format!("Timed out waiting for command to start in pane {}", pane_id),
                };
                return;
            }

            // Yield started event
            if !exited {
                let capture = if capture_count > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    do_capture(&backend, pane_target, capture_count).await
                } else {
                    None
                };
                yield LocusEvent::CommandStarted {
                    pane: PaneId(pane_id.clone()),
                    command: command.clone(),
                    capture,
                };
            }

            // Phase 2: if wait=true or already exited, wait for completion
            if !exited && wait_for_exit {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

                    // Check state file
                    if let Some((state, code)) = read_state(&state_file) {
                        if state == "exited" {
                            exited = true;
                            exit_code = code;
                            break;
                        }
                    }

                    // Check pane still alive
                    if !backend.pane_exists(&pane_id).await {
                        yield LocusEvent::Error {
                            message: format!("Pane {} was destroyed while command was running", pane_id),
                        };
                        return;
                    }
                }
            }

            if exited {
                let capture = {
                    let count = if capture_count > 0 { capture_count } else { 20 };
                    do_capture(&backend, pane_target, count).await
                };
                yield LocusEvent::CommandExited {
                    pane: PaneId(pane_id),
                    command: command.clone(),
                    exit_code,
                    capture,
                };
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Check the state of the last exec'd command in a pane. Returns command_started (still running), command_exited (finished with exit code), or error (no exec in this pane).",
        params(
            pane = "Pane ID to check (e.g. %5)",
            capture_lines = "Number of screen lines to capture (default: 0)"
        )
    )]
    async fn poll(
        &self,
        pane: PaneRef,
        capture_lines: Option<u32>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let (pane_id, _) = match resolve_pane_req(&backend, &pane).await {
                Ok(v) => v, Err(e) => { yield LocusEvent::Error { message: e }; return; }
            };
            let safe_pane = pane_id.replace('%', "pane_");
            let state_file = format!("/tmp/plexus_locus_exec_state/{}", safe_pane);

            let content = match tokio::fs::read_to_string(&state_file).await {
                Ok(c) => c,
                Err(_) => {
                    yield LocusEvent::Error {
                        message: format!("No exec state for pane {}", pane),
                    };
                    return;
                }
            };

            let mut lines_iter = content.lines();
            let state_line = lines_iter.next().unwrap_or("").trim();
            let command = lines_iter.next().unwrap_or("").to_string();

            let capture = if let Some(count) = capture_lines {
                if count > 0 {
                    let tmp = format!("/tmp/locus-capture-{}", uuid::Uuid::new_v4());
                    match backend.dump_screen(&tmp, false, Some(&pane_id)).await {
                        Ok(screen) => {
                            let _ = tokio::fs::remove_file(&tmp).await;
                            let all: Vec<&str> = screen.lines().collect();
                            let tail = all.len().saturating_sub(count as usize);
                            Some(all[tail..].join("\n"))
                        }
                        Err(_) => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if state_line == "started" {
                yield LocusEvent::CommandStarted {
                    pane: PaneId(pane_id),
                    command,
                    capture,
                };
            } else if let Some(code_str) = state_line.strip_prefix("exited:") {
                let exit_code = code_str.parse().ok();
                yield LocusEvent::CommandExited {
                    pane: PaneId(pane_id),
                    command,
                    exit_code,
                    capture,
                };
            } else {
                yield LocusEvent::Error {
                    message: format!("Unknown exec state for pane {}: {}", pane_id, state_line),
                };
            }
        }
    }

    #[plexus_macros::hub_method(
        description = "Run a command in a new pane",
        params(
            command = "Command to execute",
            name = "Pane name for tracking",
            cwd = "Working directory",
            direction = "Split direction: up, down, left, right",
            floating = "Open as floating pane",
            close_on_exit = "Close pane when command exits",
            session = "Target session (default: current)",
            target = "Pane ID to split from (e.g. %5). Default: focused pane"
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
        target: Option<String>,
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
                target,
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
}

#[async_trait]
impl ChildRouter for PanesActivation {
    fn router_namespace(&self) -> &str {
        "panes"
    }

    async fn router_call(&self, method: &str, params: serde_json::Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, _name: &str) -> Option<Box<dyn ChildRouter>> {
        None
    }
}
