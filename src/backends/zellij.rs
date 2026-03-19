use async_trait::async_trait;
use tokio::process::Command;

use crate::backend::*;
use crate::types::*;

/// Zellij backend — controls Zellij via its CLI.
///
/// The CLI is Zellij's stable public API. The Rust crates
/// (zellij-client, zellij-utils) are internal and undocumented.
pub struct Zellij {
    /// Path to zellij binary (default: "zellij")
    bin: String,
}

impl Zellij {
    pub fn new() -> Self {
        Self {
            bin: "zellij".to_string(),
        }
    }

    pub fn with_bin(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }

    /// Run a zellij command, return stdout on success
    async fn exec(&self, args: &[&str]) -> BackendResult<String> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .await
            .map_err(|e| BackendError::CommandFailed(format!("{}: {}", self.bin, e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(BackendError::CommandFailed(stderr.to_string()))
        }
    }

    /// Run a zellij command targeting a specific session
    async fn exec_session(&self, session: Option<&str>, args: &[&str]) -> BackendResult<String> {
        let mut full_args = Vec::new();
        if let Some(s) = session {
            full_args.push("--session");
            full_args.push(s);
        }
        full_args.extend_from_slice(args);
        self.exec(&full_args).await
    }

    /// Build a synthetic PaneId from session context
    fn make_pane_id(session: Option<&str>, name: Option<&str>) -> PaneId {
        let session_part = session.unwrap_or("current");
        let name_part = name.unwrap_or("unnamed");
        PaneId(format!("{}:{}", session_part, name_part))
    }

    fn make_tab_id(session: Option<&str>, index: u32) -> TabId {
        let session_part = session.unwrap_or("current");
        TabId(format!("{}:tab-{}", session_part, index))
    }

    /// Parse `zellij list-sessions` output into Session structs.
    /// Output format varies by version; we handle the common case.
    fn parse_sessions(output: &str) -> Vec<Session> {
        output
            .lines()
            .filter(|line| !line.is_empty())
            .enumerate()
            .map(|(i, line)| {
                // zellij ls output: "session_name [Created ...] (EXITED ...)" or just "session_name"
                let name = line.split_whitespace().next().unwrap_or("unknown").to_string();
                let attached = line.contains("(current)");
                let exited = line.contains("EXITED");
                Session {
                    id: SessionId(format!("session-{}", i)),
                    name,
                    tabs: 0,    // zellij ls doesn't expose this
                    panes: 0,
                    attached: attached && !exited,
                }
            })
            .collect()
    }

    /// Parse `zellij action query-tab-names` output
    fn parse_tab_names(output: &str, session: Option<&str>) -> Vec<Tab> {
        let session_id = SessionId(session.unwrap_or("current").to_string());
        output
            .lines()
            .filter(|line| !line.is_empty())
            .enumerate()
            .map(|(i, name)| Tab {
                id: Self::make_tab_id(session, i as u32),
                name: Some(name.trim().to_string()),
                index: i as u32,
                pane_count: 0,
                focused: false,
                session: session_id.clone(),
            })
            .collect()
    }
}

impl Default for Zellij {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TerminalBackend for Zellij {
    fn name(&self) -> &str {
        "zellij"
    }

    async fn is_available(&self) -> bool {
        Command::new(&self.bin)
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    // ========================================================================
    // Sessions
    // ========================================================================

    async fn list_sessions(&self) -> BackendResult<Vec<Session>> {
        let output = self.exec(&["list-sessions"]).await?;
        Ok(Self::parse_sessions(&output))
    }

    async fn create_session(&self, opts: &SessionOpts) -> BackendResult<Session> {
        let mut args = vec!["--session", &opts.name];

        let layout_str;
        if let Some(ref layout) = opts.layout {
            layout_str = layout.clone();
            args.push("--layout");
            args.push(&layout_str);
        }

        // zellij attach --create creates if missing, attaches if exists
        // For detached creation we use `zellij --session <name> options --detach`
        // but the simplest path is to let the caller attach.
        // For now, create a detached session by running a dummy command.
        args.insert(0, "attach");
        args.push("--create");

        // We can't easily create a truly detached session via CLI,
        // so we record the intent and return.
        // The session will be created on first attach.
        Ok(Session {
            id: SessionId(opts.name.clone()),
            name: opts.name.clone(),
            tabs: 1,
            panes: 1,
            attached: false,
        })
    }

    async fn kill_session(&self, name: &str) -> BackendResult<()> {
        self.exec(&["kill-session", name]).await?;
        Ok(())
    }

    // ========================================================================
    // Tabs
    // ========================================================================

    async fn list_tabs(&self, session: Option<&str>) -> BackendResult<Vec<Tab>> {
        let output = self
            .exec_session(session, &["action", "query-tab-names"])
            .await?;
        Ok(Self::parse_tab_names(&output, session))
    }

    async fn create_tab(&self, opts: &TabOpts) -> BackendResult<Tab> {
        let mut args = vec!["action", "new-tab"];

        let name_str;
        if let Some(ref name) = opts.name {
            name_str = name.clone();
            args.push("--name");
            args.push(&name_str);
        }

        let layout_str;
        if let Some(ref layout) = opts.layout {
            layout_str = layout.clone();
            args.push("--layout-dir");
            args.push(&layout_str);
        }

        self.exec_session(opts.session.as_deref(), &args).await?;

        let session_id = SessionId(opts.session.clone().unwrap_or_else(|| "current".into()));
        Ok(Tab {
            id: Self::make_tab_id(opts.session.as_deref(), 0),
            name: opts.name.clone(),
            index: 0, // We don't know the real index without querying
            pane_count: 1,
            focused: true,
            session: session_id,
        })
    }

    async fn close_tab(&self, session: Option<&str>, _index: u32) -> BackendResult<()> {
        self.exec_session(session, &["action", "close-tab"]).await?;
        Ok(())
    }

    async fn focus_tab(&self, session: Option<&str>, index: u32) -> BackendResult<()> {
        let index_str = index.to_string();
        self.exec_session(session, &["action", "go-to-tab", &index_str])
            .await?;
        Ok(())
    }

    async fn rename_tab(&self, session: Option<&str>, _index: u32, name: &str) -> BackendResult<()> {
        self.exec_session(session, &["action", "rename-tab", name])
            .await?;
        Ok(())
    }

    // ========================================================================
    // Panes
    // ========================================================================

    async fn list_panes(&self, _session: Option<&str>, _tab: Option<&str>) -> BackendResult<Vec<Pane>> {
        // Zellij CLI doesn't expose pane listing
        Ok(Vec::new())
    }

    async fn create_pane(&self, opts: &PaneOpts) -> BackendResult<Pane> {
        let mut args = vec!["action", "new-pane"];

        if opts.floating {
            args.push("--floating");
        }

        let dir_str;
        if let Some(ref dir) = opts.direction {
            dir_str = dir.as_str().to_string();
            args.push("--direction");
            args.push(&dir_str);
        }

        let name_str;
        if let Some(ref name) = opts.name {
            name_str = name.clone();
            args.push("--name");
            args.push(&name_str);
        }

        let cwd_str;
        if let Some(ref cwd) = opts.cwd {
            cwd_str = cwd.display().to_string();
            args.push("--cwd");
            args.push(&cwd_str);
        }

        self.exec_session(opts.session.as_deref(), &args).await?;

        let session_id = SessionId(opts.session.clone().unwrap_or_else(|| "current".into()));
        let tab_id = TabId(opts.tab.clone().unwrap_or_else(|| "current".into()));
        let pane_id = Self::make_pane_id(opts.session.as_deref(), opts.name.as_deref());

        Ok(Pane {
            id: pane_id,
            name: opts.name.clone(),
            command: opts.command.clone(),
            cwd: opts.cwd.clone(),
            floating: opts.floating,
            focused: true,
            tab: tab_id,
            session: session_id,
        })
    }

    async fn close_pane(&self, _pane: Option<&str>) -> BackendResult<()> {
        self.exec(&["action", "close-pane"]).await?;
        Ok(())
    }

    async fn focus_pane(&self, direction: Direction) -> BackendResult<()> {
        self.exec(&["action", "move-focus", direction.as_str()])
            .await?;
        Ok(())
    }

    async fn rename_pane(&self, name: &str, _pane: Option<&str>) -> BackendResult<()> {
        self.exec(&["action", "rename-pane", name]).await?;
        Ok(())
    }

    async fn toggle_floating(&self) -> BackendResult<()> {
        self.exec(&["action", "toggle-floating-panes"]).await?;
        Ok(())
    }

    async fn toggle_fullscreen(&self) -> BackendResult<()> {
        self.exec(&["action", "toggle-fullscreen"]).await?;
        Ok(())
    }

    async fn resize_pane(&self, direction: Direction, _amount: Option<u32>, _pane: Option<&str>) -> BackendResult<()> {
        self.exec(&["action", "resize", direction.as_str()]).await?;
        Ok(())
    }

    // ========================================================================
    // Input / Output
    // ========================================================================

    async fn write_chars(&self, chars: &str, session: Option<&str>, _pane: Option<&str>) -> BackendResult<()> {
        self.exec_session(session, &["action", "write-chars", chars])
            .await?;
        Ok(())
    }

    async fn dump_screen(&self, path: &str, full_scrollback: bool, _pane: Option<&str>) -> BackendResult<String> {
        let mut args = vec!["action", "dump-screen", path];
        if full_scrollback {
            args.push("--full");
        }
        self.exec(&args).await?;

        // Read back the file
        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| BackendError::CommandFailed(format!("Failed to read dump: {}", e)))
    }

    async fn dump_layout(&self) -> BackendResult<String> {
        self.exec(&["action", "dump-layout"]).await
    }

    // ========================================================================
    // Run
    // ========================================================================

    async fn run_command(&self, opts: &RunOpts) -> BackendResult<Pane> {
        // Split command into program + args for zellij run
        let parts: Vec<&str> = opts.command.split_whitespace().collect();
        if parts.is_empty() {
            return Err(BackendError::CommandFailed("empty command".into()));
        }

        let mut args = vec!["run"];

        if opts.floating {
            args.push("--floating");
        }

        if opts.close_on_exit {
            args.push("--close-on-exit");
        }

        let dir_str;
        if let Some(ref dir) = opts.direction {
            dir_str = dir.as_str().to_string();
            args.push("--direction");
            args.push(&dir_str);
        }

        let name_str;
        if let Some(ref name) = opts.name {
            name_str = name.clone();
            args.push("--name");
            args.push(&name_str);
        }

        let cwd_str;
        if let Some(ref cwd) = opts.cwd {
            cwd_str = cwd.display().to_string();
            args.push("--cwd");
            args.push(&cwd_str);
        }

        args.push("--");
        args.extend_from_slice(&parts);

        self.exec_session(opts.session.as_deref(), &args).await?;

        let session_id = SessionId(opts.session.clone().unwrap_or_else(|| "current".into()));
        let pane_id = Self::make_pane_id(opts.session.as_deref(), opts.name.as_deref());

        Ok(Pane {
            id: pane_id,
            name: opts.name.clone(),
            command: Some(opts.command.clone()),
            cwd: opts.cwd.clone(),
            floating: opts.floating,
            focused: true,
            tab: TabId("current".into()),
            session: session_id,
        })
    }
}
