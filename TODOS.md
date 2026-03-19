# TODOS

## File detection for /reply and /type paths
**Priority:** Medium
**Why:** When users answer interactive prompts via Telegram `/reply` or `/type`, the daemon calls `send_keys()` but never snapshots files before/after. Files generated in response (screenshots, code) are silently missed. Users expect parity with `/send`.
**Approach:** Reuse the question poller's output stabilization logic. After `send_keys()`, wait for pane output to stabilize, then run `snapshot_files()` diff and upload new files to Telegram.
**Depends on:** Question poller stabilization heuristic (already implemented in `question_poller.rs`).
**Files:** `crates/cloudcode-daemon/src/telegram/handlers.rs`, `crates/cloudcode-daemon/src/session/manager.rs`

## Telegram user-scoped access control
**Priority:** Medium
**Why:** `owner_id` is matched against `msg.chat.id`, not the sender's user ID. If a group chat ID is configured, all participants can control the VPS. Users may not realize this distinction.
**Approach:** Check `msg.from().map(|u| u.id)` against the configured owner_id in addition to the chat_id check. Both must match for commands to execute.
**Depends on:** Nothing.
**Files:** `crates/cloudcode-daemon/src/telegram/handlers.rs`, `crates/cloudcode-daemon/src/telegram/bot.rs`
