# CAST-0: Implementation Notes & Known Issues

## panes.exec doesn't work inside Docker containers

`panes.exec` writes a temp script to the **host's** `/tmp/` and sends `bash /tmp/script.sh` via tmux send-keys. When the target pane is inside a Docker container, the container's filesystem doesn't see the host `/tmp/` — the script is not found.

**Workaround**: Use `panes.write` to send commands as raw keystrokes when targeting panes running inside containers.

**Fix**: `panes.exec` could detect containerized panes, or allow specifying a temp dir path that maps into the container.

## Quoting loss through panes.write → tmux send-keys → shell layers

When launching `claude-container -- "long prompt with spaces"` via `panes.write`, the outer quotes are stripped by the intermediate shell interpretation layer (tmux send-keys evaluates the string through bash before injecting it). By the time `claude-container` sees the args, each word is a separate argument. The base64 passthrough encoding preserves this broken state.

**Impact**: `claude-container --` passthrough args work fine when invoked directly from a shell prompt, but break when sent programmatically through locus.

**Workaround**: Launch claude-container without `--` args, then send the prompt as a second `panes.write` after Claude is running interactively.

**Fix options**:
1. `panes.write` could support a raw/literal mode that avoids send-keys shell interpretation
2. `panes.exec` could support a container-aware mode that copies the script into the container's `/tmp/` before executing
3. A `panes.paste` method using tmux's paste-buffer (set-buffer + paste-buffer) would bypass shell interpretation entirely

## Untracked files not available in claude-container sessions

`claude-container` clones only git-tracked files into the session volume. Untracked files (like `plans/CAST/` before committing) are not present in the container workspace.

**Workaround**: Either commit files before creating the session, or use `claude-container --dirty` to capture uncommitted changes as a WIP commit.
