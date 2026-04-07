// container-dev: Parameterized workspace for claude-container development
//
// Usage:
//   synapse locus workspace up --template container-dev --params '{"repo": "/path/to/repo", "session": "my-session", "run_command": "bun dev"}'
//
// Params:
//   repo         - Repository path (required)
//   session      - Container session name (default: basename of repo)
//   run_command  - Dev server command for bottom-right pane (default: "zsh")

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
                    #{
                        name: "claude",
                        command: `claude-container -s ${session} --discover-repos ${repo}`
                    },
                    #{
                        name: "sandbox",
                        command: `git-sandbox session -s ${session} start -a -l`,
                        cwd: repo
                    },
                    #{
                        name: "watch",
                        command: `claude-container watch -s ${session} -- claude-container pull -s ${session}`
                    },
                    #{
                        name: "dev",
                        command: run_cmd,
                        cwd: repo
                    }
                ]
            }
        ]
    }
}
