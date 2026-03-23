# Spec: Telegram ↔ CLI Session Sync

## Problem

Telegram and CLI interact with sessions through completely independent paths:

```
CURRENT STATE — No sync between paths

Telegram User                          CLI User
    │                                     │
    ▼                                     ▼
free_text_logic()                    tmux attach-session
    │                                     │
    ▼                                     ▼
claude -p --continue (subprocess)    Interactive PTY in tmux
    │                                     │
    ▼                                     │
Response → TG message               question_poller watches
                                     for prompts → TG notification
```

**What's broken:**
1. User sends a task via Telegram → CLI user attached to tmux sees nothing
2. CLI user types a task in tmux → Telegram gets no notification of the result
3. When TG's `claude -p` finishes, only TG gets the response — no notification to CLI
4. When CLI user's interactive Claude finishes a task, TG gets no completion notification

## Architecture Decision: Provider-Aware Hybrid

Codex's independent review identified that routing Claude through tmux is the wrong approach — `claude -p --continue` already provides clean stdout, scoped file diffs, and shared conversation state. Tmux pane scraping adds fragility for no benefit.

**The hybrid approach:**
- **Claude**: Keep print mode (`claude -p --continue`). Add notification bridging — echo TG activity into the tmux pane so CLI users see it.
- **Codex**: Route through tmux (`send_via_tmux()`). Codex's `exec` mode doesn't share state with the interactive process, so tmux routing is the only way to sync.
- **Both providers**: Add completion detection to the question poller for CLI→TG notifications.

```
TARGET STATE — Provider-aware hybrid

CLAUDE:
  TG User                              CLI User
      │                                    │
      ▼                                    │
  claude -p --continue                     │
      │                                    │
      ├─► Response → TG message            │
      │                                    │
      └─► tmux send-keys "[TG] sent: X"   │
          tmux send-keys "[TG] done: Y" ──► CLI sees TG activity
                                            │
                                       poller detects completion
                                            │
                                            └─► TG: "✅ task completed"

CODEX:
  TG User                              CLI User
      │                                    │
      ▼                                    │
  send_via_tmux()                          │
      │                                    │
      ├─► send_keys(message) ──────────► tmux session (shared)
      │                                    │
      ├─► wait_for_output() polls          │
      │                                    │
      └─► Response → TG message       CLI sees it live
                                            │
                                       poller detects completion
                                            │
                                            └─► TG: "✅ task completed"
```

### Why this approach

1. **Claude stays reliable** — print mode gives deterministic stdout, scoped file diffs, no pane scraping. `--continue` preserves conversation context.
2. **CLI sees TG activity** — notification bridge echoes TG sends/responses into the tmux pane.
3. **Codex gets sync** — only provider that needs tmux routing gets it.
4. **TG sees CLI work** — completion detection in the poller notifies TG when tasks finish.
5. **Simpler** — avoids auth screen detection, restart banner parsing, and readiness state machines for Claude (the primary provider).

## Implementation

### Phase 1a: Claude notification bridge

After `send()` completes for Claude, echo a summary into the tmux pane so attached CLI users see what happened:

```rust
// In dispatch.rs, after send() returns for Claude:
if provider == AiProvider::Claude {
    // Echo TG activity into tmux for CLI visibility
    let _ = state.session_mgr.send_keys(
        &session_name,
        &format!("# [Telegram] {}: {}", truncate(text, 80), truncate(&result.text, 200))
    ).await;
    // The # prefix makes Claude treat it as a comment, not a prompt
}
```

This is fire-and-forget — if the CLI user isn't attached, the echo goes into the pane buffer and they'll see it when they attach. If `send_keys` fails, it's non-fatal.

### Phase 1b: Codex tmux-routed send

New `send_via_tmux()` method for Codex only:

```rust
pub async fn send_via_tmux(&self, session: &str, message: &str) -> Result<SendOutput> {
    validate_session_name(session)?;
    // ... lock, workdir, file snapshot ...

    // Check readiness (Codex-specific: look for codex prompt, not auth screen)
    let pane = self.capture_pane_full(session).await?;
    if !is_codex_ready(&pane) {
        bail!("Session not ready (may be authenticating or restarting)");
    }

    let pane_before = self.capture_pane_full(session).await?;
    self.send_keys(session, message).await?;
    let response = self.wait_for_output(session, &pane_before).await?;

    // File diff
    let new_files = /* snapshot diff */;
    Ok(SendOutput { text: response, files: new_files })
}
```

**Readiness check** (Codex-only, simpler than the original spec):
- Look for Codex's `>` prompt as the last non-empty line
- Reject if pane contains "device code", "authorize", or restart banner

**Output stabilization:**
1. Poll `capture_pane_full()` every 2 seconds
2. If unchanged for 2 consecutive polls AND not a question prompt → extract response
3. If question detected → set WaitingForInput, notify TG
4. Timeout 5 minutes

**`capture_pane_full()`:** Use `-S -500` instead of `-S -50` for response extraction.

### Phase 1c: `/reply` output capture

Route `/reply` through `send_via_tmux()` for Codex. For Claude, `/reply` already uses `send_keys()` — add the same notification bridge (echo result into pane).

For both providers: after `send_keys` for a reply, capture the subsequent output and send it back to TG. This addresses the TODOS.md item "File detection for /reply and /type paths."

### Phase 2: Completion notifications (CLI → TG)

Extend the question_poller with a per-session activity state machine:

```
  ┌──────┐  pane changed  ┌────────┐  unchanged 2x  ┌───────────┐
  │ IDLE │────────────────►│ ACTIVE │───────────────►│STABILIZED │
  └──────┘                └────────┘                └───────────┘
     ▲                                                    │
     │                              ┌─────────────────────┤
     │                     detect_question()         idle prompt?
     │                              │                     │
     │                              ▼                     ▼
     │                        WAITING_FOR_INPUT      COMPLETED
     │                              │                     │
     └──────────────────────────────┴─────────────────────┘
```

**Coordination with send_via_tmux:** Per-session `AtomicBool` (`sending_via_tmux`). Poller skips sessions where it's true. Prevents double-notification for TG-originated Codex sends.

**Completion notification to TG:**
```
✅ [session_name] Task completed. Use /peek to see output.
```
Plus any new files (per-session file baseline tracked by poller).

**Suppression:** Don't send completion notifications for TG-originated Claude sends (Phase 1a already returned the response). Only notify for CLI-originated work.

**False positive mitigation:**
- Ignore restart banners (lines containing `[cloudcode]`)
- Require the idle prompt to be provider-specific (Claude: line matching `^>` or `^❯`; Codex: line matching `^>` or `^❯` after a period of output)
- Require at least 10 seconds of ACTIVE state before considering completion (filters restart transitions)

### Phase 2b: Update /peek to use full capture

Change `/peek` to use `capture_pane_full()` (`-S -500`) instead of `capture_pane()` (`-S -50`). Otherwise the completion notification says "use /peek" but peek shows truncated output.

## Files to modify

| File | Change |
|------|--------|
| `crates/cloudcode-daemon/src/session/manager.rs` | Add `send_via_tmux()`, `capture_pane_full()`, `wait_for_output()`, `is_codex_ready()`. Set `history-limit 10000` on spawn. Update `/peek`'s `capture_pane` to use -S -500. |
| `crates/cloudcode-daemon/src/telegram/dispatch.rs` | Claude: add notification bridge after `send()`. Codex: use `send_via_tmux()`. `/reply`: add output capture for both providers. |
| `crates/cloudcode-daemon/src/telegram/question_poller.rs` | Add per-session activity state machine, completion detection, file baseline tracking, `AtomicBool` coordination, suppression for TG-originated sends. |

## Edge cases

1. **Claude session in auth flow** — `send()` (print mode) fails with auth error. Existing error handling returns auth hint to TG. No pane scraping needed.
2. **Codex session in auth flow** — `is_codex_ready()` detects auth flow text, returns error to TG.
3. **CLI user typing during TG Codex send** — inputs interleave. Accepted — same as CLI user experience. Mutex prevents programmatic interleave.
4. **Long Claude response** — print mode captures full stdout. No truncation concern.
5. **Long Codex response (>500 lines)** — `capture_pane_full` truncates to last 500 lines. Tail is still useful.
6. **Restart banner triggers false completion** — filtered by requiring 10s ACTIVE + ignoring `[cloudcode]` lines.
7. **Double notification** — TG-originated Claude sends: Phase 1a returns response, Phase 2 suppressed. TG-originated Codex sends: AtomicBool skips poller.
8. **Message > 4096 chars** — Claude: print mode handles any length. Codex: `send_keys` rejects >4096, return error.
9. **Retry dropped for Codex tmux sends** — can't retry `send_keys` without duplicating input. If `send_via_tmux()` fails, return error (no retry).

## Security

- `send_keys()` validates input: rejects control characters, caps at 4096 chars
- `send-keys -l` (literal mode) prevents tmux escape injection
- Claude notification bridge uses `#` prefix — Claude treats it as a comment
- Codex readiness check prevents typing into auth flows

## Testing

### Unit tests (all pure functions on string input):
1. `is_codex_ready()` — READY state (idle prompt)
2. `is_codex_ready()` — AUTH_FLOW state (device code text)
3. `is_codex_ready()` — RESTARTING state (restart banner)
4. `wait_for_output()` — output stabilizes, extract response
5. `wait_for_output()` — question detected during stabilization
6. `wait_for_output()` — timeout after 5 min
7. Response extraction — diff pane_before vs pane_after
8. Response extraction — strip echoed input
9. Response extraction — strip prompt lines
10. Completion detection — IDLE → ACTIVE → COMPLETED
11. Completion detection — restart banner filtered
12. Completion detection — 10s minimum ACTIVE duration
13. Suppression — TG-originated send skipped by poller
14. Notification bridge — echo format for Claude
15. `/reply` output capture for Claude
16. `/reply` output capture for Codex
17. `/peek` now returns 500 lines not 50

### Integration tests:
1. Claude: TG sends → response returned + bridge echo in tmux
2. Codex: TG sends → typed into tmux → response captured
3. CLI works → completion detected → TG notified
4. Codex `/reply` → output captured and returned to TG

## Migration / Rollback

- Claude path is additive (notification bridge) — remove the echo to roll back
- Codex `send_via_tmux` is new — fall back to current `send()` (loses sync but works)
- Completion detection is new — disable by removing state machine from poller
- No breaking protocol changes

## Out of scope

- Real-time streaming of tmux output to TG
- TG typing indicator during CLI work
- Multi-user support
- Merging conversation histories into single view
- Codex retry logic for tmux sends (can't retry without duplicating)
