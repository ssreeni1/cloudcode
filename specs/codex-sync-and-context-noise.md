# Spec: Codex TG Sync Fix + Context Sharing Noise Reduction

## Problem 1: Codex TG → tmux send doesn't execute

`send_via_tmux()` types into Codex's interactive TUI via `tmux send-keys`, but Codex doesn't execute — the user has to manually press Enter in CLI.

**Fix**: Revert Codex to print mode (`send()`). `send_via_tmux` doesn't work with Codex's TUI input handler. Also revert `/reply` for Codex sessions — use `codex exec` to send the reply text instead of `send_keys`, which also fails in Codex's TUI.

**Impact on sync**: Codex TG sends are isolated from the interactive session (same as before sync feature). Completion detection via the poller still works for CLI→TG.

## Problem 2: Context sharing is noisy

CLAUDE.md instructs Claude to run shell commands to read context files before every response. This adds 3-5 noisy tool calls ("echo $CLOUDCODE_SESSION_NAME", "printf '%s' contexts/*.md") that appear in TG output.

**Fix**: Simplify the instructions — keep context sharing but make it less noisy:
- Remove the "read at start of every task" instruction
- Keep the "update YOUR context file after significant work" instruction
- Add "only read other sessions' context files when the user explicitly asks about cross-session work"

This reduces noise while preserving the mechanism. A future enhancement can move context aggregation into the daemon.

## Changes

### dispatch.rs — Codex back to print mode

```rust
// In free_text_logic():
let send_result = match provider {
    AiProvider::Codex => {
        // Codex: use print mode (codex exec).
        // send_via_tmux doesn't work with Codex's TUI input handler.
        // Same retry logic as Claude.
        let mut result = Err(anyhow::anyhow!("not started"));
        for attempt in 0..3 { ... }
        result
    }
    AiProvider::Claude => { ... } // unchanged
};
```

Also in `reply_logic()`: for Codex sessions, use `session_mgr.send()` with the reply text instead of `send_keys()`.

### deploy.rs — Simplify CLAUDE.md instructions

```markdown
## Shared Context
- Context files from all sessions are at /home/claude/.cloudcode/contexts/
- After completing significant work, update YOUR context file
- Only read other sessions' context files when asked about cross-session work
- Keep your context file under 10KB
```

Remove:
- "At the start of a new task, read the Summary section of other sessions' context files"
- The detailed formatting instructions

### Live VPS update

After deploying, update CLAUDE.md on the running VPS via SSH.

## Files to modify

| File | Change |
|------|--------|
| `crates/cloudcode-daemon/src/telegram/dispatch.rs` | Codex: `send()` not `send_via_tmux()`. Reply: `send()` for Codex. |
| `crates/cloudcode-cli/src/deploy.rs` | Simplify context instructions in CLAUDE.md |

## Testing

1. Codex TG free text → uses `send()`, gets response, no tmux interaction
2. Codex TG `/reply` → uses `send()`, gets response
3. Claude TG free text → unchanged (print mode + bridge echo)
4. Claude responses don't include context-reading shell commands for simple questions
5. Claude still updates context file after significant work

## Out of scope

- Daemon-owned context aggregation (future)
- Codex interactive TUI input debugging
- Real-time context sync between sessions
