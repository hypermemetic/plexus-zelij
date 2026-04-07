//! Rhai scripting engine for workspace templates
//!
//! Templates are `.rhai` scripts that define a function returning a workspace config.
//! The function receives user-provided parameters as a Map and returns a Map
//! matching the WorkspaceConfig structure.
//!
//! Example template (`container-dev.rhai`):
//! ```rhai
//! fn workspace(params) {
//!     let repo = params.repo;
//!     let session = if "session" in params { params.session } else { basename(repo) };
//!     let run_cmd = if "run_command" in params { params.run_command } else { "shell" };
//!
//!     #{
//!         tabs: [
//!             #{
//!                 name: session,
//!                 layout: [2, 2],
//!                 panes: [
//!                     #{ name: "claude", command: `claude-container -s ${session} --discover-repos ${repo}` },
//!                     #{ name: "sandbox", command: `git-sandbox session -s ${session} start -a -l`, cwd: repo },
//!                     #{ name: "watch", command: `claude-container watch -s ${session} -- claude-container pull -s ${session}` },
//!                     #{ name: "dev", command: run_cmd, cwd: repo },
//!                 ]
//!             }
//!         ]
//!     }
//! }
//! ```

use rhai::{Engine, Scope, Dynamic, Map};
use crate::activations::workspace::{WorkspaceConfig, TabConfig, PaneConfig};

/// Create a Rhai engine with built-in helper functions
pub fn create_engine() -> Engine {
    let mut engine = Engine::new();

    // Raise limits for complex workspace definitions (deeply nested maps/arrays)
    engine.set_max_expr_depths(128, 128);

    // basename("/foo/bar/baz") => "baz"
    engine.register_fn("basename", |path: &str| -> String {
        std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string()
    });

    // dirname("/foo/bar/baz") => "/foo/bar"
    engine.register_fn("dirname", |path: &str| -> String {
        std::path::Path::new(path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string()
    });

    // join_path("/foo", "bar") => "/foo/bar"
    engine.register_fn("join_path", |base: &str, child: &str| -> String {
        std::path::Path::new(base).join(child).display().to_string()
    });

    // env("HOME") => "/Users/shmendez"
    engine.register_fn("env", |name: &str| -> String {
        std::env::var(name).unwrap_or_default()
    });

    // home() => "/Users/shmendez"
    engine.register_fn("home", || -> String {
        std::env::var("HOME").unwrap_or_else(|_| "~".into())
    });

    engine
}

/// Evaluate a Rhai template script with the given parameters.
/// Returns a WorkspaceConfig.
pub fn evaluate_template(
    script: &str,
    params: std::collections::HashMap<String, String>,
) -> Result<WorkspaceConfig, String> {
    let engine = create_engine();
    let mut scope = Scope::new();

    // Convert params to a Rhai Map
    let mut rhai_params = Map::new();
    for (k, v) in &params {
        rhai_params.insert(k.clone().into(), Dynamic::from(v.clone()));
    }

    // Compile and run the script to define functions
    let ast = engine.compile(script).map_err(|e| format!("Script compile error: {e}"))?;

    // Call the `workspace` function with params
    let result: Dynamic = engine
        .call_fn(&mut scope, &ast, "workspace", (rhai_params,))
        .map_err(|e| format!("Script execution error: {e}"))?;

    // Convert the Rhai result (Dynamic Map) to WorkspaceConfig
    dynamic_to_workspace_config(result)
}

/// Convert a Rhai Dynamic value to WorkspaceConfig
fn dynamic_to_workspace_config(value: Dynamic) -> Result<WorkspaceConfig, String> {
    let map = value.try_cast::<Map>().ok_or("workspace() must return a map")?;

    let tabs_dyn = map.get("tabs").ok_or("workspace result missing 'tabs'")?;
    let tabs_arr = tabs_dyn.clone().into_array().map_err(|_| "'tabs' must be an array")?;

    let mut tabs = Vec::new();
    for tab_dyn in tabs_arr {
        tabs.push(dynamic_to_tab_config(tab_dyn)?);
    }

    Ok(WorkspaceConfig { tabs })
}

fn dynamic_to_tab_config(value: Dynamic) -> Result<TabConfig, String> {
    let map = value.try_cast::<Map>().ok_or("tab entry must be a map")?;

    let name = map.get("name")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_else(|| "unnamed".into());

    let layout = if let Some(layout_dyn) = map.get("layout") {
        let arr = layout_dyn.clone().into_array().map_err(|_| "'layout' must be [rows, cols]")?;
        if arr.len() >= 2 {
            [
                arr[0].as_int().map_err(|_| "layout[0] must be integer")? as u32,
                arr[1].as_int().map_err(|_| "layout[1] must be integer")? as u32,
            ]
        } else {
            [1, 1]
        }
    } else {
        [1, 1]
    };

    let panes = if let Some(panes_dyn) = map.get("panes") {
        let arr = panes_dyn.clone().into_array().map_err(|_| "'panes' must be an array")?;
        arr.into_iter()
            .map(dynamic_to_pane_config)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };

    Ok(TabConfig { name, layout, panes })
}

fn dynamic_to_pane_config(value: Dynamic) -> Result<PaneConfig, String> {
    let map = value.try_cast::<Map>().ok_or("pane entry must be a map")?;

    Ok(PaneConfig {
        name: map.get("name").and_then(|v| v.clone().into_string().ok()),
        command: map.get("command").and_then(|v| v.clone().into_string().ok()),
        cwd: map.get("cwd").and_then(|v| v.clone().into_string().ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_template() {
        let script = r#"
            fn workspace(params) {
                let name = params.name;
                #{
                    tabs: [
                        #{
                            name: name,
                            layout: [1, 2],
                            panes: [
                                #{ name: "left", command: "echo hello" },
                                #{ name: "right", command: `echo ${name}` },
                            ]
                        }
                    ]
                }
            }
        "#;

        let mut params = std::collections::HashMap::new();
        params.insert("name".into(), "test-ws".into());

        let config = evaluate_template(script, params).unwrap();
        assert_eq!(config.tabs.len(), 1);
        assert_eq!(config.tabs[0].name, "test-ws");
        assert_eq!(config.tabs[0].layout, [1, 2]);
        assert_eq!(config.tabs[0].panes.len(), 2);
        assert_eq!(config.tabs[0].panes[0].name.as_deref(), Some("left"));
        assert_eq!(config.tabs[0].panes[1].command.as_deref(), Some("echo test-ws"));
    }

    #[test]
    fn test_container_dev_template() {
        let script = r#"
            fn workspace(params) {
                let repo = params.repo;
                let session = if "session" in params { params.session } else { basename(repo) };
                let run_cmd = if "run_command" in params { params.run_command } else { "zsh" };

                #{
                    tabs: [
                        #{
                            name: session,
                            layout: [2, 2],
                            panes: [
                                #{ name: "claude", command: `claude-container -s ${session} --discover-repos ${repo}` },
                                #{ name: "sandbox", command: `git-sandbox session -s ${session} start -a -l`, cwd: repo },
                                #{ name: "watch", command: `claude-container watch -s ${session} -- claude-container pull -s ${session}` },
                                #{ name: "dev", command: run_cmd, cwd: repo },
                            ]
                        }
                    ]
                }
            }
        "#;

        let mut params = std::collections::HashMap::new();
        params.insert("repo".into(), "/Users/shmendez/dev/controlflow/hypermemetic".into());
        params.insert("run_command".into(), "bun dev".into());

        let config = evaluate_template(script, params).unwrap();
        assert_eq!(config.tabs[0].name, "hypermemetic");
        assert_eq!(config.tabs[0].panes[0].command.as_deref(),
            Some("claude-container -s hypermemetic --discover-repos /Users/shmendez/dev/controlflow/hypermemetic"));
        assert_eq!(config.tabs[0].panes[3].command.as_deref(), Some("bun dev"));
        assert_eq!(config.tabs[0].panes[1].cwd.as_deref(),
            Some("/Users/shmendez/dev/controlflow/hypermemetic"));
    }

    #[test]
    fn test_basename_helper() {
        let engine = create_engine();
        let result: String = engine.eval(r#"basename("/foo/bar/baz")"#).unwrap();
        assert_eq!(result, "baz");
    }
}
