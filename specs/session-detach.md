# Spec: Session Detach Without Kill

## Problem

When users `/open` a session (which runs `tmux attach-session` via SSH), they need to return to the TUI without killing the session. The tmux session already survives all exit paths — the actual problem is **users don't know how to detach**.

Current behavior:
1. **Ctrl-b d** — detaches from tmux, session stays alive. Works perfectly but users don't know the keybinding.
2. **Closing terminal** — kills SSH, tmux auto-detaches. Session survives. But TUI also dies.
3. **Ctrl-C** — forwarded to the AI process as SIGINT. The retry loop (`while true; do claude...; sleep 3; done`) restarts the process, but current conversation context is lost.

**Root cause**: This is a UX problem, not a technical one. The session is never killed — users just don't know how to get back.

## Requirements

- User can exit `/open` back to TUI (or CLI prompt) cleanly
- tmux session stays alive (Claude/Codex keeps running)
- Works for users who don't know tmux keybindings
- Works from both TUI (`/open`) and CLI (`cloudcode open`)
- No interference with standard tmux/terminal keybindings
- No global tmux config changes (affects all sessions, not just cloudcode)

## Decision: Option A+D — Better messaging + tmux status bar hint

### Rejected alternatives

**Option B (Escape-Escape or Ctrl-] binding)**: Rejected. Escape is used constantly in terminal apps (vim, less, Claude Code UI). Double-escape fires accidentally. Ctrl-] is the telnet escape character and can be intercepted by SSH intermediaries.

**Option C (Custom .tmux.conf)**: Rejected. Writing `~/.tmux.conf` affects ALL tmux sessions on the VPS, not just cloudcode ones. Per-session options (`tmux set-option -t SESSION`) are safer but add complexity for marginal benefit.

### Implementation

Three changes, all in `crates/cloudcode-cli/src/commands/attach.rs`:

#### 1. Improve pre-attach message (attach.rs:76-79)

```
Before:
  (Detach with Ctrl-b d, or close terminal to disconnect)

After:
  To return to cloudcode: press Ctrl-b, then d (two separate keypresses)
  Session stays alive in the background after detaching.
```

#### 2. Show tmux display-message on attach

Before running `tmux attach-session`, send a display-message so the user sees a reminder inside the session:

```rust
// In attach_ssh_args, change the remote command to:
format!(
    "tmux display-message -t {} -d 5000 'Detach: Ctrl-b then d' && tmux attach-session -t {}",
    quoted_session, quoted_session
)
```

The `-d 5000` shows the message for 5 seconds in the tmux status line.

#### 3. Set tmux status-right per-session (not global)

On session spawn (manager.rs), set a persistent status-right hint:

```rust
// After tmux new-session succeeds, run:
Command::new("tmux")
    .args(["set-option", "-t", &name, "status-right", " Detach: Ctrl-b d "])
    .status()
    .await;
```

This shows "Detach: Ctrl-b d" in the bottom-right of the tmux status bar, only for cloudcode sessions. Does not affect other tmux sessions.

### Also document: SSH escape sequence

As a secondary method, update the pre-attach message to mention:

```
  Alternative: press Enter, then ~, then . to disconnect SSH (session survives)
```

## Error handling

### Ctrl-C during OAuth
If the user Ctrl-C's while Claude/Codex is showing an OAuth login URL, the process receives SIGINT and dies. The retry loop restarts it after 3 seconds, and the OAuth flow restarts. Credentials from a partially-completed OAuth are not persisted, so this is safe — the user just needs to re-do the login flow.

No code change needed — the retry loop already handles this.

### Ctrl-C during AI execution
SIGINT kills the current AI subprocess. The retry loop restarts. Conversation context in print mode (`claude -p --continue`) is preserved via `--continue`. Interactive mode (tmux) restarts fresh.

No code change needed.

## Files to modify

| File | Change |
|------|--------|
| `crates/cloudcode-cli/src/commands/attach.rs` | Update pre-attach message, add display-message to remote command |
| `crates/cloudcode-daemon/src/session/manager.rs` | Add `tmux set-option status-right` after session spawn |

## Testing

1. `/open session` → see display-message for 5s → see status-right hint → Ctrl-b d → back at TUI
2. `/open session` → kill terminal → relaunch cloudcode → session still listed in `/list`
3. `/open session` → Ctrl-C inside AI → verify retry loop restarts
4. `cloudcode open session` from CLI → same detach behavior

## Out of scope

- Custom keybindings beyond tmux defaults
- Multi-session switching (detach one, attach another in single flow)
- Runtime switching between sessions without detaching
