use async_trait::async_trait;
use tokio::process::Command;

use crate::backend::*;
use crate::types::*;

/// Tmux backend — controls tmux via its CLI.
///
/// Uses stable `%id` pane references for targeted operations.
/// Detects its own pane via `$TMUX_PANE` to prevent self-destruction.
pub struct TmuxBackend {
    bin: String,
    /// The pane ID of the process running locus (from $TMUX_PANE).
    /// Operations that would kill this pane are refused.
    self_pane: Option<String>,
}

impl TmuxBackend {
    pub fn new() -> Self {
        let self_pane = std::env::var("TMUX_PANE").ok();
        Self {
            bin: "tmux".to_string(),
            self_pane,
        }
    }

    /// Run a tmux command, return stdout on success
    async fn exec(&self, args: &[&str]) -> BackendResult<String> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .await
            .map_err(|e| BackendError::CommandFailed(format!("{}: {}", self.bin, e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(BackendError::CommandFailed(stderr.trim().to_string()))
        }
    }

    /// Parse space-separated tmux format output into rows of fields
    fn parse_format_output(output: &str) -> Vec<Vec<&str>> {
        output
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.split_whitespace().collect())
            .collect()
    }
}

impl Default for TmuxBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TerminalBackend for TmuxBackend {
    fn name(&self) -> &str {
        "tmux"
    }

    async fn is_available(&self) -> bool {
        Command::new(&self.bin)
            .arg("-V")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    // ========================================================================
    // Sessions
    // ========================================================================

    async fn list_sessions(&self) -> BackendResult<Vec<Session>> {
        let format = "#{session_id} #{session_name} #{session_windows} #{session_attached}";
        let output = self.exec(&["list-sessions", "-F", format]).await?;

        Ok(Self::parse_format_output(&output)
            .into_iter()
            .filter_map(|fields| {
                if fields.len() < 4 {
                    return None;
                }
                Some(Session {
                    id: SessionId(fields[0].to_string()),
                    name: fields[1].to_string(),
                    tabs: fields[2].parse().unwrap_or(0),
                    panes: 0, // tmux doesn't give total panes in session list
                    attached: fields[3] == "1",
                })
            })
            .collect())
    }

    async fn create_session(&self, opts: &SessionOpts) -> BackendResult<Session> {
        let mut args = vec!["new-session", "-d", "-s", &opts.name, "-P", "-F", "#{session_id}"];

        let cwd_str;
        if let Some(ref cwd) = opts.cwd {
            cwd_str = cwd.display().to_string();
            args.push("-c");
            args.push(&cwd_str);
        }

        let output = self.exec(&args).await?;
        let session_id = output.trim().to_string();

        Ok(Session {
            id: SessionId(session_id),
            name: opts.name.clone(),
            tabs: 1,
            panes: 1,
            attached: false,
        })
    }

    async fn kill_session(&self, name: &str) -> BackendResult<()> {
        self.exec(&["kill-session", "-t", name]).await?;
        Ok(())
    }

    // ========================================================================
    // Tabs (tmux windows)
    // ========================================================================

    async fn list_tabs(&self, session: Option<&str>) -> BackendResult<Vec<Tab>> {
        let format = "#{window_id} #{window_name} #{window_index} #{window_panes} #{window_active} #{session_name}";
        let mut args = vec!["list-windows", "-F", format];

        let target;
        if let Some(s) = session {
            target = s.to_string();
            args.push("-t");
            args.push(&target);
        }

        let output = self.exec(&args).await?;
        let session_id = SessionId(session.unwrap_or("current").to_string());

        Ok(Self::parse_format_output(&output)
            .into_iter()
            .filter_map(|fields| {
                if fields.len() < 5 {
                    return None;
                }
                Some(Tab {
                    id: TabId(fields[0].to_string()),
                    name: Some(fields[1].to_string()),
                    index: fields[2].parse().unwrap_or(0),
                    pane_count: fields[3].parse().unwrap_or(0),
                    focused: fields[4] == "1",
                    session: session_id.clone(),
                })
            })
            .collect())
    }

    async fn create_tab(&self, opts: &TabOpts) -> BackendResult<Tab> {
        // -d: don't switch to the new window
        let mut args = vec!["new-window", "-d", "-P", "-F", "#{window_id} #{window_index}"];

        let target;
        if let Some(ref s) = opts.session {
            target = s.clone();
            args.push("-t");
            args.push(&target);
        }

        let name_str;
        if let Some(ref name) = opts.name {
            name_str = name.clone();
            args.push("-n");
            args.push(&name_str);
        }

        let cwd_str;
        if let Some(ref cwd) = opts.cwd {
            cwd_str = cwd.display().to_string();
            args.push("-c");
            args.push(&cwd_str);
        }

        let output = self.exec(&args).await?;
        let parts: Vec<&str> = output.trim().split_whitespace().collect();

        let session_id = SessionId(opts.session.clone().unwrap_or_else(|| "current".into()));
        Ok(Tab {
            id: TabId(parts.first().unwrap_or(&"").to_string()),
            name: opts.name.clone(),
            index: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
            pane_count: 1,
            focused: false, // -d flag: don't steal focus
            session: session_id,
        })
    }

    async fn close_tab(&self, session: Option<&str>, index: u32) -> BackendResult<()> {
        let target = if let Some(s) = session {
            format!("{}:{}", s, index)
        } else {
            format!(":{}", index)
        };
        self.exec(&["kill-window", "-t", &target]).await?;
        Ok(())
    }

    async fn focus_tab(&self, session: Option<&str>, index: u32) -> BackendResult<()> {
        let target = if let Some(s) = session {
            format!("{}:{}", s, index)
        } else {
            format!(":{}", index)
        };
        self.exec(&["select-window", "-t", &target]).await?;
        Ok(())
    }

    async fn rename_tab(&self, session: Option<&str>, index: u32, name: &str) -> BackendResult<()> {
        let target = if let Some(s) = session {
            format!("{}:{}", s, index)
        } else {
            format!(":{}", index)
        };
        self.exec(&["rename-window", "-t", &target, name]).await?;
        Ok(())
    }

    // ========================================================================
    // Panes
    // ========================================================================

    async fn list_panes(&self, session: Option<&str>, _tab: Option<&str>) -> BackendResult<Vec<Pane>> {
        let format = "#{pane_id} #{pane_title} #{pane_current_command} #{pane_current_path} #{pane_active} #{window_id} #{window_index} #{session_name}";
        let mut args = vec!["list-panes", "-F", format];

        // -s lists all panes in session, -a lists all panes across all sessions
        let target;
        if let Some(s) = session {
            target = s.to_string();
            args.push("-s");
            args.push("-t");
            args.push(&target);
        } else {
            args.push("-a");
        }

        let output = self.exec(&args).await?;

        Ok(Self::parse_format_output(&output)
            .into_iter()
            .filter_map(|fields| {
                if fields.len() < 8 {
                    return None;
                }
                let cwd = if fields[3].is_empty() {
                    None
                } else {
                    Some(std::path::PathBuf::from(fields[3]))
                };
                Some(Pane {
                    id: PaneId(fields[0].to_string()),
                    name: Some(fields[1].to_string()),
                    command: Some(fields[2].to_string()),
                    cwd,
                    floating: false, // tmux doesn't have persistent floating panes
                    focused: fields[4] == "1",
                    tab: TabId(fields[5].to_string()),
                    session: SessionId(fields[7].to_string()),
                })
            })
            .collect())
    }

    async fn create_pane(&self, opts: &PaneOpts) -> BackendResult<Pane> {
        // -d: don't switch focus to the new pane
        let mut args = vec!["split-window", "-d", "-P", "-F", "#{pane_id} #{window_id} #{session_name}"];

        // Direction: -h for horizontal split (left/right), default is vertical (up/down)
        match opts.direction {
            Some(Direction::Left) | Some(Direction::Right) => args.push("-h"),
            _ => {} // vertical split is default
        }

        // For "before" splits (up/left)
        match opts.direction {
            Some(Direction::Up) | Some(Direction::Left) => args.push("-b"),
            _ => {}
        }

        // Target pane to split from (most specific wins)
        let pane_target;
        if let Some(ref t) = opts.target {
            pane_target = t.clone();
            args.push("-t");
            args.push(&pane_target);
        } else if let Some(ref s) = opts.session {
            pane_target = s.clone();
            args.push("-t");
            args.push(&pane_target);
        }

        let cwd_str;
        if let Some(ref cwd) = opts.cwd {
            cwd_str = cwd.display().to_string();
            args.push("-c");
            args.push(&cwd_str);
        }

        let output = self.exec(&args).await?;
        let parts: Vec<&str> = output.trim().split_whitespace().collect();

        let pane_id_str = parts.first().unwrap_or(&"").to_string();
        let tab_id = TabId(parts.get(1).unwrap_or(&"current").to_string());
        let session_id = SessionId(parts.get(2).unwrap_or(&"current").to_string());

        // Set pane title if name was provided
        if let Some(ref name) = opts.name {
            let _ = self.exec(&["select-pane", "-t", &pane_id_str, "-T", name]).await;
        }

        Ok(Pane {
            id: PaneId(pane_id_str),
            name: opts.name.clone(),
            command: opts.command.clone(),
            cwd: opts.cwd.clone(),
            floating: false,
            focused: false, // -d flag: don't steal focus
            tab: tab_id,
            session: session_id,
        })
    }

    async fn close_pane(&self, pane: Option<&str>) -> BackendResult<()> {
        // Refuse to close our own pane
        if let (Some(target), Some(ref self_pane)) = (pane, &self.self_pane) {
            if target == self_pane {
                return Err(BackendError::CommandFailed(
                    "refusing to close locus's own pane".into(),
                ));
            }
        }

        if let Some(target) = pane {
            self.exec(&["kill-pane", "-t", target]).await?;
        } else {
            self.exec(&["kill-pane"]).await?;
        }
        Ok(())
    }

    async fn focus_pane(&self, direction: Direction) -> BackendResult<()> {
        let flag = match direction {
            Direction::Up => "-U",
            Direction::Down => "-D",
            Direction::Left => "-L",
            Direction::Right => "-R",
        };
        self.exec(&["select-pane", flag]).await?;
        Ok(())
    }

    async fn rename_pane(&self, name: &str, pane: Option<&str>) -> BackendResult<()> {
        // tmux: select-pane -T "title"
        let mut args = vec!["select-pane", "-T", name];
        if let Some(target) = pane {
            args.push("-t");
            args.push(target);
        }
        self.exec(&args).await?;
        Ok(())
    }

    async fn toggle_floating(&self) -> BackendResult<()> {
        // tmux has popup windows but not persistent floating panes like zellij
        Err(BackendError::Unsupported(
            "tmux does not have persistent floating panes (use popup for transient windows)".into(),
        ))
    }

    async fn toggle_fullscreen(&self) -> BackendResult<()> {
        // tmux zoom = toggle fullscreen
        self.exec(&["resize-pane", "-Z"]).await?;
        Ok(())
    }

    async fn resize_pane(
        &self,
        direction: Direction,
        amount: Option<u32>,
        pane: Option<&str>,
    ) -> BackendResult<()> {
        let flag = match direction {
            Direction::Up => "-U",
            Direction::Down => "-D",
            Direction::Left => "-L",
            Direction::Right => "-R",
        };
        let amount_str = amount.unwrap_or(5).to_string();
        let mut args = vec!["resize-pane", flag, &amount_str];

        if let Some(target) = pane {
            args.push("-t");
            args.push(target);
        }
        self.exec(&args).await?;
        Ok(())
    }

    // ========================================================================
    // Input / Output
    // ========================================================================

    async fn write_chars(
        &self,
        chars: &str,
        _session: Option<&str>,
        pane: Option<&str>,
    ) -> BackendResult<()> {
        let mut args = vec!["send-keys"];
        if let Some(target) = pane {
            args.push("-t");
            args.push(target);
        }
        args.push(chars);
        self.exec(&args).await?;
        Ok(())
    }

    async fn dump_screen(
        &self,
        path: &str,
        full_scrollback: bool,
        pane: Option<&str>,
    ) -> BackendResult<String> {
        // capture-pane → save to buffer → pipe to file
        let mut args = vec!["capture-pane", "-p"];

        if full_scrollback {
            args.push("-S");
            args.push("-");
        }

        if let Some(target) = pane {
            args.push("-t");
            args.push(target);
        }

        let content = self.exec(&args).await?;

        // Also write to the path for compatibility with the trait contract
        tokio::fs::write(path, &content)
            .await
            .map_err(|e| BackendError::CommandFailed(format!("Failed to write capture: {}", e)))?;

        Ok(content)
    }

    async fn dump_layout(&self) -> BackendResult<String> {
        let format = "#{window_index} #{window_name} #{window_layout}";
        self.exec(&["list-windows", "-F", format]).await
    }

    // ========================================================================
    // Run
    // ========================================================================

    async fn run_command(&self, opts: &RunOpts) -> BackendResult<Pane> {
        // split-window with command: splits and runs in one step
        // -d: don't steal focus from the current pane
        let mut args = vec!["split-window", "-d", "-P", "-F", "#{pane_id} #{window_id} #{session_name}"];

        match opts.direction {
            Some(Direction::Left) | Some(Direction::Right) => args.push("-h"),
            _ => {}
        }
        match opts.direction {
            Some(Direction::Up) | Some(Direction::Left) => args.push("-b"),
            _ => {}
        }

        // Target pane to split from (most specific wins)
        let pane_target;
        if let Some(ref t) = opts.target {
            pane_target = t.clone();
            args.push("-t");
            args.push(&pane_target);
        } else if let Some(ref s) = opts.session {
            pane_target = s.clone();
            args.push("-t");
            args.push(&pane_target);
        }

        let cwd_str;
        if let Some(ref cwd) = opts.cwd {
            cwd_str = cwd.display().to_string();
            args.push("-c");
            args.push(&cwd_str);
        }

        // The command to run in the new pane
        args.push(&opts.command);

        let output = self.exec(&args).await?;
        let parts: Vec<&str> = output.trim().split_whitespace().collect();

        let pane_id = PaneId(parts.first().unwrap_or(&"").to_string());
        let tab_id = TabId(parts.get(1).unwrap_or(&"current").to_string());
        let session_id = SessionId(parts.get(2).unwrap_or(&"current").to_string());

        Ok(Pane {
            id: pane_id,
            name: opts.name.clone(),
            command: Some(opts.command.clone()),
            cwd: opts.cwd.clone(),
            floating: false,
            focused: false, // -d flag: don't steal focus
            tab: tab_id,
            session: session_id,
        })
    }
}
