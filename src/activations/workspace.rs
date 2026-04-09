use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::TerminalBackend;
use crate::plexus::{Activation, ChildRouter, PlexusError, PlexusStream};
use crate::types::{LocusEvent, Pane, TabOpts, PaneOpts, Direction, PaneId, SessionId};

const CONFIG_FILENAME: &str = "plexus_locus.config.json";

// ============================================================================
// Config types
// ============================================================================

/// Top-level Locus configuration file structure
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LocusConfig {
    /// Map of workspace names to workspace configurations
    pub workspaces: std::collections::HashMap<String, WorkspaceConfig>,
}

/// Workspace configuration defining tabs and their layouts
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct WorkspaceConfig {
    /// List of tabs (windows) to create in this workspace
    pub tabs: Vec<TabConfig>,
}

/// Tab configuration with name, layout grid, and pane definitions
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TabConfig {
    /// Tab name
    pub name: String,
    /// [rows, cols] — defaults to [1, 1] (single pane)
    #[serde(default = "default_layout")]
    pub layout: [u32; 2],
    /// Pane configurations within this tab
    #[serde(default)]
    pub panes: Vec<PaneConfig>,
}

/// Individual pane configuration
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PaneConfig {
    /// Optional pane name for targeting
    #[serde(default)]
    pub name: Option<String>,
    /// Command to run in the pane
    #[serde(default)]
    pub command: Option<String>,
    /// Working directory (supports ~/ and relative paths)
    #[serde(default)]
    pub cwd: Option<String>,
}

const fn default_layout() -> [u32; 2] {
    [1, 1]
}

/// Resolve a cwd relative to the config file's directory
fn resolve_cwd(cwd: &str, config_dir: &Path) -> PathBuf {
    let expanded = if cwd.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(&cwd[2..])
        } else {
            PathBuf::from(cwd)
        }
    } else {
        PathBuf::from(cwd)
    };

    if expanded.is_absolute() {
        expanded
    } else {
        config_dir.join(expanded)
    }
}

/// Search for a template .rhai file by name.
/// Looks in: <dir>/<name>.rhai, <dir>/templates/<name>.rhai, ~/.config/locus/templates/<name>.rhai
fn find_template(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidates = [
        dir.join(format!("{name}.rhai")),
        dir.join(format!("templates/{name}.rhai")),
        dirs_template_path(name),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn dirs_template_path(name: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(format!(".config/locus/templates/{name}.rhai"))
}

// ============================================================================
// Workspace Activation
// ============================================================================

/// Workspace sub-activation — manages workspace configuration and activation
///
/// Loads workspace definitions from `plexus_locus.config.json` and creates
/// multi-tab layouts with preconfigured panes, commands, and working directories.
#[derive(Clone)]
pub struct WorkspaceActivation {
    /// Terminal backend instance shared across all activations
    pub(crate) backend: Arc<dyn TerminalBackend>,
}

impl WorkspaceActivation {
    /// Create a new WorkspaceActivation with the specified backend
    pub fn new(backend: Arc<dyn TerminalBackend>) -> Self {
        Self { backend }
    }
}

#[allow(missing_docs)]
#[plexus_macros::activation(
    namespace = "workspace",
    version = "0.1.0",
    description = "Workspace lifecycle from plexus_locus.config.json"
)]
impl WorkspaceActivation {
    #[plexus_macros::method(
        description = "Show the config file contents",
        params(path = "Project directory containing plexus_locus.config.json (default: CWD)")
    )]
    async fn show(&self, path: Option<String>) -> impl Stream<Item = LocusEvent> + Send + 'static {
        stream! {
            let dir = PathBuf::from(path.unwrap_or_else(|| ".".into()));
            let config_path = dir.join(CONFIG_FILENAME);

            match tokio::fs::read_to_string(&config_path).await {
                Ok(content) => {
                    match serde_json::from_str::<LocusConfig>(&content) {
                        Ok(config) => {
                            let workspace_names: Vec<String> = config.workspaces.keys().cloned().collect();
                            yield LocusEvent::Ok {
                                message: format!(
                                    "Config: {}\nWorkspaces: {}",
                                    config_path.display(),
                                    workspace_names.join(", ")
                                ),
                            };
                        }
                        Err(e) => yield LocusEvent::Error {
                            message: format!("Invalid config {}: {}", config_path.display(), e),
                        },
                    }
                }
                Err(e) => yield LocusEvent::Error {
                    message: format!("Cannot read {}: {}", config_path.display(), e),
                },
            }
        }
    }

    #[plexus_macros::method(
        description = "List available Rhai templates",
        params(path = "Project directory to search (default: CWD)")
    )]
    async fn templates(&self, path: Option<String>) -> impl Stream<Item = LocusEvent> + Send + 'static {
        stream! {
            let dir = PathBuf::from(path.unwrap_or_else(|| ".".into()));
            let mut found: Vec<String> = Vec::new();

            // Search in dir, dir/templates/, and ~/.config/locus/templates/
            let search_dirs = [
                dir.clone(),
                dir.join("templates"),
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/locus/templates"),
            ];

            for search_dir in &search_dirs {
                if let Ok(mut entries) = tokio::fs::read_dir(search_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("rhai") {
                            let name = path.file_stem()
                                .and_then(|n| n.to_str())
                                .unwrap_or("?")
                                .to_string();
                            let location = search_dir.display().to_string();
                            found.push(format!("{name} ({location})"));
                        }
                    }
                }
            }

            if found.is_empty() {
                yield LocusEvent::Ok { message: "No templates found".into() };
            } else {
                yield LocusEvent::Ok { message: format!("Templates:\n{}", found.join("\n")) };
            }
        }
    }

    #[plexus_macros::method(
        description = "Materialize a workspace. Use --workspace for config-defined workspaces, or --template for Rhai script templates with --params.",
        params(
            workspace = "Workspace name from config (default: first workspace)",
            path = "Project directory containing config/templates (default: CWD)",
            template = "Rhai template name (looks for <name>.rhai in path or ~/.config/locus/templates/)",
            params = "Template parameters as JSON object (e.g. {\"repo\": \"/path\", \"session\": \"my-session\"})"
        )
    )]
    async fn up(
        &self,
        workspace: Option<String>,
        path: Option<String>,
        template: Option<String>,
        params: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let backend = self.backend.clone();
        stream! {
            let dir = PathBuf::from(path.unwrap_or_else(|| ".".into()));
            let config_dir = dir.clone();

            // Resolve workspace config — either from template or config file
            let (ws_name, ws) = if let Some(ref tmpl_name) = template {
                // Template mode: find and evaluate .rhai script
                let script_path = find_template(&dir, tmpl_name);
                let script_path = match script_path {
                    Some(p) => p,
                    None => {
                        yield LocusEvent::Error {
                            message: format!("Template '{tmpl_name}' not found (looked in {} and ~/.config/locus/templates/)", dir.display()),
                        };
                        return;
                    }
                };

                let script = match tokio::fs::read_to_string(&script_path).await {
                    Ok(s) => s,
                    Err(e) => {
                        yield LocusEvent::Error { message: format!("Cannot read template: {e}") };
                        return;
                    }
                };

                // Parse params JSON
                let template_params: std::collections::HashMap<String, String> = if let Some(ref p) = params {
                    match serde_json::from_str(p) {
                        Ok(m) => m,
                        Err(e) => {
                            yield LocusEvent::Error { message: format!("Invalid params JSON: {e}") };
                            return;
                        }
                    }
                } else {
                    std::collections::HashMap::new()
                };

                match crate::scripting::evaluate_template(&script, template_params) {
                    Ok(ws_config) => (tmpl_name.clone(), ws_config),
                    Err(e) => {
                        yield LocusEvent::Error { message: format!("Template error: {e}") };
                        return;
                    }
                }
            } else {
                // Config file mode
                let config_path = dir.join(CONFIG_FILENAME);
                let content = match tokio::fs::read_to_string(&config_path).await {
                    Ok(c) => c,
                    Err(e) => {
                        yield LocusEvent::Error {
                            message: format!("Cannot read {}: {}", config_path.display(), e),
                        };
                        return;
                    }
                };
                let config: LocusConfig = match serde_json::from_str(&content) {
                    Ok(c) => c,
                    Err(e) => {
                        yield LocusEvent::Error { message: format!("Invalid config: {e}") };
                        return;
                    }
                };

                let ws_name = workspace.unwrap_or_else(|| {
                    config.workspaces.keys().next().cloned().unwrap_or_default()
                });
                let ws = if let Some(w) = config.workspaces.get(&ws_name) {
                    w.clone()
                } else {
                    let available: Vec<&String> = config.workspaces.keys().collect();
                    yield LocusEvent::Error {
                        message: format!("Workspace '{ws_name}' not found. Available: {available:?}"),
                    };
                    return;
                };
                (ws_name, ws)
            };

            // Track all created panes for the state file
            let mut all_created_tabs: Vec<String> = Vec::new();
            let mut all_created_panes: Vec<Pane> = Vec::new();

            // Materialize each tab
            for tab_config in &ws.tabs {
                let [rows, cols] = tab_config.layout;
                let rows = rows.max(1);
                let cols = cols.max(1);

                // Create tab
                let tab_opts = TabOpts {
                    name: Some(tab_config.name.clone()),
                    layout: None,
                    cwd: None,
                    session: None,
                };
                let tab = match backend.create_tab(&tab_opts).await {
                    Ok(t) => t,
                    Err(e) => {
                        yield LocusEvent::Error {
                            message: format!("Failed to create tab '{}': {}", tab_config.name, e),
                        };
                        continue;
                    }
                };
                all_created_tabs.push(tab.id.0.clone());

                // Find initial pane
                let initial_pane = match backend.list_panes(None, None).await {
                    Ok(panes) => panes.iter()
                        .find(|p| p.tab == tab.id)
                        .map(|p| p.id.0.clone()),
                    Err(_) => None,
                };
                let initial_pane = if let Some(p) = initial_pane { p } else {
                    yield LocusEvent::Error {
                        message: format!("Could not find initial pane in tab '{}'", tab_config.name),
                    };
                    continue;
                };

                // Build grid: rows by splitting down, then cols by splitting right
                let mut left_column: Vec<String> = vec![initial_pane];
                for _ in 1..rows {
                    let target = left_column.last().unwrap().clone();
                    let opts = PaneOpts {
                        direction: Some(Direction::Down),
                        target: Some(target),
                        ..Default::default()
                    };
                    match backend.create_pane(&opts).await {
                        Ok(p) => left_column.push(p.id.0.clone()),
                        Err(e) => {
                            yield LocusEvent::Error { message: format!("Grid error: {e}") };
                            break;
                        }
                    }
                }

                let mut flat_panes: Vec<String> = Vec::new();
                for row_start in &left_column {
                    let mut row = vec![row_start.clone()];
                    for _ in 1..cols {
                        let target = row.last().unwrap().clone();
                        let opts = PaneOpts {
                            direction: Some(Direction::Right),
                            target: Some(target),
                            ..Default::default()
                        };
                        match backend.create_pane(&opts).await {
                            Ok(p) => row.push(p.id.0.clone()),
                            Err(e) => {
                                yield LocusEvent::Error { message: format!("Grid error: {e}") };
                                break;
                            }
                        }
                    }
                    flat_panes.extend(row);
                }

                // Apply names, cwds, and commands
                for (i, pane_id) in flat_panes.iter().enumerate() {
                    if let Some(pc) = tab_config.panes.get(i) {
                        // Name
                        if let Some(ref name) = pc.name {
                            let _ = backend.rename_pane(name, Some(pane_id)).await;
                        }
                        // Cwd
                        if let Some(ref cwd) = pc.cwd {
                            let resolved = resolve_cwd(cwd, &config_dir);
                            let cd_cmd = format!("cd {}", resolved.display());
                            let _ = backend.write_chars(&cd_cmd, None, Some(pane_id)).await;
                            let _ = backend.write_chars("Enter", None, Some(pane_id)).await;
                            // Small delay for cd to complete before sending command
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                        // Command
                        if let Some(ref cmd) = pc.command {
                            let _ = backend.write_chars(cmd, None, Some(pane_id)).await;
                            let _ = backend.write_chars("Enter", None, Some(pane_id)).await;
                        }
                    }

                    all_created_panes.push(Pane {
                        id: PaneId(pane_id.clone()),
                        name: tab_config.panes.get(i).and_then(|p| p.name.clone()),
                        command: tab_config.panes.get(i).and_then(|p| p.command.clone()),
                        cwd: tab_config.panes.get(i)
                            .and_then(|p| p.cwd.as_ref())
                            .map(|c| resolve_cwd(c, &config_dir)),
                        floating: false,
                        focused: false,
                        tab: tab.id.clone(),
                        session: SessionId("current".into()),
                        left: None, top: None, width: None, height: None,
                    });
                }
            }

            // Write state file so `down` knows what to tear down
            let state_dir = "/tmp/plexus_locus_workspaces";
            let _ = tokio::fs::create_dir_all(state_dir).await;
            let state = serde_json::json!({
                "workspace": ws_name,
                "config_dir": config_dir.display().to_string(),
                "tabs": all_created_tabs,
            });
            let state_path = format!("{state_dir}/{ws_name}.json");
            let _ = tokio::fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap_or_default()).await;

            yield LocusEvent::Ok {
                message: format!(
                    "Workspace '{}' up: {} tab(s), {} pane(s)",
                    ws_name,
                    all_created_tabs.len(),
                    all_created_panes.len(),
                ),
            };
        }
    }

    #[plexus_macros::method(
        description = "Tear down a workspace — closes all tabs created by `up`",
        params(
            workspace = "Workspace name to tear down",
            path = "Project directory (used to find workspace name if not specified)"
        )
    )]
    async fn down(
        &self,
        workspace: Option<String>,
        path: Option<String>,
    ) -> impl Stream<Item = LocusEvent> + Send + 'static {
        let _backend = self.backend.clone();
        stream! {
            // Find workspace name
            let ws_name = if let Some(name) = workspace {
                name
            } else {
                // Try to read config to get default workspace name
                let dir = PathBuf::from(path.unwrap_or_else(|| ".".into()));
                let config_path = dir.join(CONFIG_FILENAME);
                if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
                    if let Ok(config) = serde_json::from_str::<LocusConfig>(&content) { config.workspaces.keys().next().cloned().unwrap_or_default() } else {
                        yield LocusEvent::Error { message: "No workspace specified and config is invalid".into() };
                        return;
                    }
                } else {
                    yield LocusEvent::Error { message: "No workspace specified and no config found".into() };
                    return;
                }
            };

            // Read state file
            let state_path = format!("/tmp/plexus_locus_workspaces/{ws_name}.json");
            let state_content = if let Ok(c) = tokio::fs::read_to_string(&state_path).await { c } else {
                yield LocusEvent::Error {
                    message: format!("No running workspace '{ws_name}' (no state file)"),
                };
                return;
            };

            let state: serde_json::Value = match serde_json::from_str(&state_content) {
                Ok(v) => v,
                Err(e) => {
                    yield LocusEvent::Error { message: format!("Corrupt state file: {e}") };
                    return;
                }
            };

            // Kill each tab
            let mut killed = 0u32;
            if let Some(tabs) = state.get("tabs").and_then(|t| t.as_array()) {
                for tab_val in tabs {
                    if let Some(tab_id) = tab_val.as_str() {
                        // tmux kill-window -t @id
                        // We don't have a direct "kill tab by id" on the backend,
                        // but close_tab takes session + index. Use write_chars to tmux directly.
                        // Actually, we can use the tmux backend's exec:
                        let result = tokio::process::Command::new("tmux")
                            .args(["kill-window", "-t", tab_id])
                            .output()
                            .await;
                        if result.map(|o| o.status.success()).unwrap_or(false) {
                            killed += 1;
                        }
                    }
                }
            }

            // Remove state file
            let _ = tokio::fs::remove_file(&state_path).await;

            yield LocusEvent::Ok {
                message: format!("Workspace '{ws_name}' down: killed {killed} tab(s)"),
            };
        }
    }
}

#[async_trait]
impl ChildRouter for WorkspaceActivation {
    fn router_namespace(&self) -> &'static str {
        "workspace"
    }

    async fn router_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, _name: &str) -> Option<Box<dyn ChildRouter>> {
        None
    }
}
