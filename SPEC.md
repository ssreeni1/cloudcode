# cloudcode — Technical Specification

> Provision a Hetzner VPS, deploy a daemon managing AI coding sessions via tmux, and interact through TUI, CLI, or Telegram.

**Version**: 0.1.5
**License**: MIT

---

## Table of Contents

1. [Overview](#1-overview)
2. [Architecture](#2-architecture)
3. [Project Structure](#3-project-structure)
4. [Configuration](#4-configuration)
5. [CLI Commands](#5-cli-commands)
6. [TUI](#6-tui)
7. [Provisioning Flow](#7-provisioning-flow)
8. [Session Management](#8-session-management)
9. [Daemon API](#9-daemon-api)
10. [Provider System](#10-provider-system)
11. [Telegram Bot](#11-telegram-bot)
12. [Question Forwarding](#12-question-forwarding)
13. [File Detection & Upload](#13-file-detection--upload)
14. [SSH Tunneling](#14-ssh-tunneling)
15. [Hetzner Integration](#15-hetzner-integration)
16. [Security Model](#16-security-model)
17. [Build System](#17-build-system)

---

## 1. Overview

cloudcode provisions a single-tenant Hetzner VPS, deploys a long-running daemon that manages Claude Code and Codex CLI sessions via tmux, and exposes three interaction modes:

- **TUI** — interactive terminal interface with wizard and command bar
- **CLI** — direct commands (`cloudcode spawn`, `cloudcode send`, etc.)
- **Telegram** — phone-based access with automatic question forwarding

Sessions persist across disconnects. The daemon runs as a systemd service on the VPS, communicating with the local CLI over an SSH-tunneled Unix socket.

---

## 2. Architecture

```
┌─────────────────────┐         SSH Tunnel          ┌──────────────────────────┐
│   Local Machine      │◄──────────────────────────►│   Hetzner VPS             │
│                      │                             │                           │
│  cloudcode CLI/TUI   │  Newline-delimited JSON     │  cloudcode-daemon         │
│  ~/.cloudcode/       │◄───────────────────────────►│    ├─ API server          │
│    config.toml       │                             │    ├─ Session manager      │
│    state.json        │                             │    ├─ Telegram bot         │
│    id_ed25519        │                             │    └─ Question poller      │
│                      │                             │                           │
└─────────────────────┘                              │  tmux sessions             │
                                                     │    ├─ claude --bypass      │
                                                     │    └─ codex --full-auto    │
                                                     └──────────────────────────┘
```

**Crates**:
- `cloudcode-cli` — CLI binary, TUI, Hetzner client, SSH, deployment
- `cloudcode-daemon` — VPS daemon: API, sessions, Telegram, question polling
- `cloudcode-common` — Shared types: protocol, session info, provider enum

---

## 3. Project Structure

```
crates/
├── cloudcode-cli/src/
│   ├── main.rs              # Entry point: routes to TUI or subcommand
│   ├── cli.rs               # Clap command definitions
│   ├── config.rs            # Config structs (Hetzner, Claude, Codex, Telegram, VPS)
│   ├── state.rs             # VPS state persistence (state.json)
│   ├── paths.rs             # File paths (~/.cloudcode/)
│   ├── deploy.rs            # Daemon binary management (embed, remote build, cross-compile)
│   ├── commands/
│   │   ├── init.rs          # Setup wizard
│   │   ├── up.rs            # Provision VPS
│   │   ├── down.rs          # Destroy VPS
│   │   ├── spawn.rs         # Create session
│   │   ├── attach.rs        # Attach to session (handles /open and /attach)
│   │   ├── send.rs          # Send message to session
│   │   ├── list.rs          # List sessions
│   │   ├── kill.rs          # Kill session
│   │   ├── status.rs        # VPS/session status
│   │   ├── restart.rs       # Restart daemon
│   │   ├── logs.rs          # Fetch VPS logs
│   │   ├── ssh_cmd.rs       # SSH shell access
│   │   ├── doctor.rs        # Diagnostics
│   │   ├── security.rs      # Security model explanation
│   │   └── security_report.rs # Security audit
│   ├── hetzner/
│   │   ├── client.rs        # Hetzner Cloud API client
│   │   └── provisioner.rs   # Cloud-init generation
│   ├── ssh/
│   │   ├── connection.rs    # SSH connection management
│   │   ├── tunnel.rs        # Unix socket forwarding over SSH
│   │   └── health.rs        # SSH readiness checks
│   └── tui/
│       ├── mod.rs           # TUI runner and event loop
│       ├── app.rs           # App state, slash command parsing
│       ├── steps.rs         # Wizard step definitions
│       └── ui.rs            # Rendering (ratatui widgets)
│
├── cloudcode-daemon/src/
│   ├── main.rs              # Entry: API server + Telegram bot + question poller
│   ├── config.rs            # Daemon config (/etc/cloudcode/daemon.toml)
│   ├── session/
│   │   ├── manager.rs       # SessionManager: spawn, list, kill, send, capture
│   │   └── monitor.rs       # Periodic cleanup of dead sessions
│   ├── api/
│   │   ├── server.rs        # Unix socket listener
│   │   └── handlers.rs      # Request dispatch
│   └── telegram/
│       ├── bot.rs           # Bot setup and dispatcher
│       ├── handlers.rs      # Command handlers
│       ├── question_poller.rs # Auto-detect tmux questions
│       ├── session_resolution.rs # Session targeting logic
│       ├── default_session.rs # Per-user default session
│       ├── files.rs         # File upload to Telegram
│       ├── formatter.rs     # Message chunking (4096 char limit)
│       └── replies.rs       # Text/markdown message helpers
│
└── cloudcode-common/src/
    ├── lib.rs
    ├── protocol.rs          # DaemonRequest/DaemonResponse enums
    ├── session.rs           # SessionInfo, SessionState
    ├── provider.rs          # AiProvider enum (Claude, Codex)
    ├── auth.rs              # AuthMethod enum (ApiKey, Oauth)
    └── constants.rs         # Shared constants
```

---

## 4. Configuration

### Local Config (`~/.cloudcode/config.toml`, 0600)

```toml
[hetzner]
api_token = "hcloud-token"

[claude]
auth_method = "api_key"       # "api_key" | "oauth"
api_key = "sk-ant-..."        # only for api_key method

[codex]
auth_method = "api_key"       # "api_key" | "oauth"
api_key = "sk-..."

[telegram]
bot_token = "123456:ABC-..."
owner_id = 987654321

[vps]
server_type = "cx23"          # optional override
location = "nbg1"
image = "ubuntu-24.04"

default_provider = "claude"   # "claude" | "codex"
```

### State File (`~/.cloudcode/state.json`, 0600)

```json
{
  "server_id": 123456,
  "ssh_key_id": 789012,
  "server_ip": "203.0.113.42",
  "status": "running"
}
```

### SSH Keys

- Private: `~/.cloudcode/id_ed25519` (0600)
- Public: `~/.cloudcode/id_ed25519.pub`
- Generated via `ssh-keygen` during init

### Daemon Config (`/etc/cloudcode/daemon.toml` on VPS)

```toml
listen_addr = "127.0.0.1"
listen_port = 7700

[telegram]
bot_token = "..."
owner_id = 12345
```

### VPS State Files

| Path | Purpose |
|------|---------|
| `~/.cloudcode/default-provider` | Current default AI provider |
| `~/.cloudcode/sessions/<name>/provider` | Per-session provider |
| `~/.cloudcode/contexts/context_<name>.md` | Session context file |
| `~/.claude/credentials.json` | Claude OAuth credentials (created on VPS during login) |
| `~/.codex/auth.json` | Codex OAuth credentials (created on VPS during login) |
| `~/.cloudcode-env` | API keys (sourced by sessions) |
| `~/.cloudcode-status.json` | Cloud-init completion marker |
| `~/.cloudcode/playwright-status.json` | Playwright install status |
| `~/.cloudcode/codex-status.json` | Codex install status |

---

## 5. CLI Commands

```
cloudcode                         # Launch TUI (default)
cloudcode init [--auto] [--reauth] [--classic]
cloudcode up [--no-wait] [--server-type TYPE]
cloudcode down [--force]
cloudcode spawn [name]
cloudcode open <session>
cloudcode send <session> <message>
cloudcode list
cloudcode kill <session>
cloudcode status
cloudcode doctor
cloudcode security
cloudcode restart
cloudcode logs [setup|daemon]
cloudcode ssh [command...]
```

| Command | Description |
|---------|-------------|
| `init` | Setup wizard — configure Hetzner, providers, Telegram, SSH keys |
| `up` | Provision VPS — create server, deploy daemon, wait for readiness |
| `down` | Destroy VPS — delete server and SSH key from Hetzner |
| `spawn` | Create a new tmux session running Claude or Codex |
| `open` | Attach interactively to a tmux session |
| `send` | Send a message to a session (non-interactive, returns output) |
| `list` | List active sessions with state and timestamps |
| `kill` | Terminate a session |
| `status` | Show VPS info and session overview |
| `doctor` | Run system diagnostics |
| `security` | Display security model explanation |
| `restart` | Restart the daemon on the VPS |
| `logs` | Fetch VPS setup or daemon logs |
| `ssh` | Raw SSH access (interactive shell if no args) |

---

## 6. TUI

Built with **ratatui** + **crossterm** + **tui-input**.

### Modes

**Wizard Mode** — first-run setup flow:
1. Welcome
2. Hetzner token
3. Provider selection (Claude / Codex / Both)
4. Claude auth (API key or OAuth)
5. Codex auth (if selected)
6. Telegram (optional)
7. SSH key generation
8. Complete

**Main Mode** — command interface:
- Input bar for `/command [args]` entry
- Scrollable log window for command output
- Interactive server type picker with live Hetzner pricing

### Slash Commands

All CLI commands available as `/command`, plus:

| Command | Description |
|---------|-------------|
| `/wait` | Wait for setup completion |
| `/use <name>` | Set default session |
| `/init` | Re-run setup wizard |
| `/help` | Show command reference |
| `/quit` | Exit TUI |

Aliases: `/ls` → `/list`, `/attach` → `/open`

### Interaction

- **Mouse scroll**: scroll log output up/down (3 lines per tick)
- **PageUp/PageDown**: scroll log output by 10 lines
- **Esc**: clear input field
- **Ctrl+C**: kill running subprocess; double-press within 2s to quit
- **Arrow keys**: navigate wizard selections (Up/Down/j/k)

---

## 7. Provisioning Flow

Triggered by `cloudcode up` or `/up`.

**Limitation**: Provisioning currently requires Claude configuration even for Codex-only setups. `DeploymentContext::load()` fails if `config.claude` is `None`.

### Readiness Phases

Provisioning completes in phases. The success marker (`~/.cloudcode-status.json`) indicates base readiness only:

| Phase | When | What's ready |
|-------|------|-------------|
| Base | After cloud-init success marker | SSH, tmux, Claude Code, daemon |
| Codex | After `codex-status.json` = ready | Codex CLI sessions |
| Playwright | After `playwright-status.json` = ready | Browser automation in sessions |

Codex and Playwright install in background after the base phase. Sessions using these tools may fail until their respective status files indicate readiness.

### Steps

1. **Validate** — Hetzner token, provider auth, SSH key
2. **Cloud-init** — Generate user-data script:
   - Create `claude` user with passwordless sudo
   - Install packages: tmux, curl, jq, git, nodejs, npm
   - Write setup scripts to `/opt/cloudcode-*.sh`
3. **Create SSH key** on Hetzner (POST `/ssh_keys`)
4. **Create server** on Hetzner (POST `/servers`) with cloud-init
5. **Wait for server** — poll status until "running"
6. **SSH health check** — retry up to 10 times, 5s backoff
7. **Wait for cloud-init** — poll `~/.cloudcode-status.json` every 10s
8. **Deploy daemon**:
   - Try embedded binary → remote build on VPS → local cross-compile
   - Target detection: cx*/cpx* → x86_64, cax* → aarch64
   - Write `/etc/cloudcode/daemon.toml`
   - Create and start systemd service
9. **Deploy user configs**:
   - `~/.cloudcode-env` (API keys)
   - `~/.cloudcode/default-provider`
   - `~/.codex/config.toml` (if Codex configured)

### Cloud-Init Scripts

**Main** (`/opt/cloudcode-setup.sh`):
- Install Claude Code (3 retries)
- Configure UFW firewall (allow SSH only)
- Spawn background: Playwright install, Codex install
- Write success marker

**Playwright** (`/opt/cloudcode-playwright-setup.sh`):
- `npx playwright install --with-deps chromium` (3 attempts, 20m timeout)

**Codex** (`/opt/cloudcode-codex-setup.sh`):
- `npm install -g @openai/codex` (3 attempts, 15m timeout)

---

## 8. Session Management

Each session is a tmux session with fixed dimensions (200x50).

### Spawn

1. Validate name: `[a-zA-Z0-9_-]`, max 64 chars
2. Create workspace: `/home/claude/.cloudcode/sessions/<name>/workspace/` (0700)
3. Create session-scoped tmpdir
4. Symlink CLAUDE.md and AGENTS.md into workspace
5. Archive stale context file if reusing name
6. Record provider in `sessions/<name>/provider`
7. Launch tmux with provider command:
   - **Claude**: `claude --dangerously-skip-permissions --permission-mode bypassPermissions`
   - **Codex**: `codex --full-auto`

### Send

1. Acquire per-session lock (serialize concurrent sends)
2. Snapshot files before execution
3. Invoke in print mode:
   - **Claude**: `claude -p --dangerously-skip-permissions --continue <message>`
   - **Codex**: `codex exec --full-auto "<message>"`
4. Capture stdout and stderr
5. Snapshot files after execution
6. Return output + list of new/modified files

**Concurrency**: A per-session mutex serializes concurrent sends. If the AI subprocess hangs, the lock is held indefinitely, blocking all subsequent sends to that session. The CLI-side read timeout is 180 seconds.

### Other Operations

| Operation | Implementation |
|-----------|---------------|
| **List** | `tmux list-sessions -F` → name, created_at, last_activity |
| **Kill** | `tmux kill-session -t <name>` |
| **Capture** | `tmux capture-pane -p -S -50 -t <name>`, strip ANSI codes |
| **Send keys** | `tmux send-keys -l -t <name> -- <text>` + Enter (max 4096 chars) |

### File Scanning

- Recursive walk, depth limit 5
- Watched dirs: workspace, screenshots/, output/, session tmpdir
- Excludes: `.git`, `node_modules`, `__pycache__`, `target/`, `.venv`, `venv/`
- Change detection: mtime + size comparison (before/after)
- Sendable extensions: png, jpg, jpeg, gif, webp, svg, pdf, md, txt, json, csv, html, log

---

## 9. Daemon API

**Transport**: Newline-delimited JSON over Unix socket. The CLI creates a local Unix socket via SSH tunnel (`-L`) forwarding to the daemon's TCP port (`127.0.0.1:7700`). Each request/response is a single JSON line terminated by `\n`. This is NOT JSON-RPC — there are no `id`, `method`, or `jsonrpc` fields.

### Protocol

```rust
enum DaemonRequest {
    Spawn { name: Option<String> },
    List,
    Kill { session: String },
    Send { session: String, message: String },
    Status,
    Cleanup,
}

enum DaemonResponse {
    Spawned { session: SessionInfo },
    Sessions { sessions: Vec<SessionInfo> },
    Killed { session: String },
    SendResult { output: String, files: Vec<String> },
    Status { uptime_secs: u64, sessions: Vec<SessionInfo> },
    CleanedUp { sessions: Vec<String> },
    Error { message: String },
}
```

### Session Info

```rust
struct SessionInfo {
    name: String,
    state: SessionState,    // Starting | Running | Idle | Dead
    created_at: u64,        // Unix timestamp (seconds)
    last_activity: u64,     // Unix timestamp (seconds)
}
```

### Health Monitor

Background tokio task runs every 60 seconds, cleaning up dead/stale tmux sessions.

---

## 10. Provider System

```rust
enum AiProvider {
    Claude,   // default
    Codex,
}
```

Implements: `Display`, `FromStr` (case-insensitive), `Serialize`, `Deserialize`.

### Provider Resolution

1. Per-session file: `~/.cloudcode/sessions/<name>/provider`
2. Global default: `~/.cloudcode/default-provider`
3. Fallback: Claude

### Provider Status Detection

| Provider | Ready When |
|----------|-----------|
| Claude (API key) | `ANTHROPIC_API_KEY` env var set |
| Claude (OAuth) | `~/.claude/credentials.json` exists |
| Codex (API key) | `OPENAI_API_KEY` env var set + binary exists |
| Codex (OAuth) | `~/.codex/auth.json` exists + binary exists |

---

## 11. Telegram Bot

Built with **teloxide 0.13**.

### Commands

| Command | Description |
|---------|-------------|
| `/start`, `/help` | Show command reference |
| `/spawn [name]` | Create session |
| `/list` | List sessions |
| `/kill <name>` | Kill session |
| `/use <name>` | Set default session |
| `/status` | Daemon uptime + session count |
| `/provider [claude\|codex]` | Show or switch provider |
| `/waiting` | List sessions waiting for input |
| `/reply [session] <text>` | Answer a waiting session (via send_keys) |
| `/context [session]` | View session context file |
| `/peek [session]` | Raw tmux pane content (last 50 lines) |
| `/type [session] <text>` | Type directly into tmux session |

### Free Text Messages

Routed to the active session via `send()` (not `send_keys()`):
1. If default session set → use it
2. If exactly 1 session exists → use it
3. Otherwise → list sessions and ask user to pick

**Note**: Free text does NOT auto-route to sessions waiting for input. Use `/reply` or `/type` explicitly to answer interactive prompts.

### Access Control

The `owner_id` in config is matched against `msg.chat.id`, not the sender's user ID. This means control is **chat-scoped**: if a group chat ID is configured, all participants can control the VPS. For single-user authorization, always use a private chat ID.

### Formatting

- Messages chunked to 4096 characters (Telegram limit)
- HTML formatting for question notifications
- Files up to 50MB sent as documents

---

## 12. Question Forwarding

Background poller detects when Claude/Codex asks a question in an interactive tmux session and forwards it to Telegram.

### State Machine

```
Idle ──[question detected]──► WaitingForInput(question, detected_at)
  ▲                                    │
  └────[/reply or /type sent]──────────┘
  └────[5 minute timeout]─────────────┘
```

### Detection

- Poll every 3 seconds: `capture_pane()` for each active session
- Wait for output stabilization: 2 consecutive identical polls (~6s)
- Pattern match on last 5 non-empty lines:
  - `(y/n)`, `(yes/no)`, `[y/N]`, `[Y/n]`
  - Line ends with `>`
  - Contains: "Do you want", "Would you like", "Shall I", "Should I"
  - Contains: "plan mode", "Plan mode", "ExitPlanMode"
  - Starts with "Enter " and ends with `:`
  - Contains: "Ready to proceed", "proceed with", "Grant permission"
- Deduplication: content hash prevents resending same question

### Telegram Notification (HTML)

```
🔔 [session-name] Claude is waiting for input:

<last 20 lines of pane>

Use /reply session <text> or /type session <text>
```

---

## 13. File Detection & Upload

### Trigger Points

- `cloudcode send` / Telegram free text → snapshot before/after
- `/reply` and `/type` do **NOT** trigger file snapshots — files generated after answering interactive prompts are not auto-detected

### Flow

1. Snapshot watched directories recursively (depth 5)
2. Compare mtime + size tuples
3. Filter new/modified files by sendable extension
4. Upload to Telegram (max 50MB per file)
5. Per-file error handling: skip failures, continue with remaining

### Watched Directories

- `sessions/<name>/workspace/`
- `sessions/<name>/workspace/screenshots/`
- `sessions/<name>/workspace/output/`
- `sessions/<name>/tmp/`

---

## 14. SSH Tunneling

### Architecture

Local CLI ↔ SSH tunnel ↔ Remote daemon Unix socket

```
ssh -N -L /tmp/cloudcode.sock:/home/claude/.cloudcode/socket claude@<ip>
```

### Connection Details

- Binary: OpenSSH `ssh`
- Identity: `~/.cloudcode/id_ed25519`
- User: `claude`
- Port: 22
- Known hosts: `~/.cloudcode/known_hosts` (dedicated file, NOT `~/.ssh/known_hosts`)
- `StrictHostKeyChecking=accept-new` — auto-accepts on first connect, rejects changes
- `GlobalKnownHostsFile=/dev/null` — ignores system-wide known hosts
- Timeout: 10 seconds per connection (`ConnectTimeout=10`)

### SSH Connection Multiplexing

`ssh_base_args()` configures `ControlMaster=auto` with `ControlPersist=300` for connection reuse across health checks, list, status, etc. However, specific commands override this:

| Command | ControlMaster | Reason |
|---------|--------------|--------|
| Health checks, list, status | `auto` | Benefits from connection reuse |
| DaemonClient tunnel | `no` | Dedicated forwarding connection, must not share |
| Interactive attach (`/open`) | `no` | Stale control sockets cause PTY allocation failures |
| Interactive SSH (`/ssh`) | `auto` | Standard behavior acceptable |

### Health Checks

- Echo command via SSH
- Up to 10 retries, 5s backoff
- Clears stale known_hosts entries before retry on timeout

---

## 15. Hetzner Integration

### API

- Base: `https://api.hetzner.cloud/v1/`
- Auth: Bearer token

### Endpoints Used

| Endpoint | Purpose |
|----------|---------|
| `GET /servers` | List servers |
| `POST /servers` | Create server with cloud-init |
| `DELETE /servers/{id}` | Delete server |
| `GET /ssh_keys` | List SSH keys |
| `POST /ssh_keys` | Create SSH key |
| `DELETE /ssh_keys/{id}` | Delete SSH key |
| `GET /server_types` | List types with pricing |

### Server Types

| Architecture | Types |
|-------------|-------|
| x86_64 shared | cx23, cx33, cx43, cx53 |
| x86_64 AMD shared | cpx11, cpx12, cpx21, cpx22, cpx31, cpx32, cpx41, cpx42, cpx51, cpx52, cpx62 |
| x86_64 dedicated | ccx13, ccx23, ccx33, ccx43, ccx53, ccx63 |
| ARM64 shared | cax11, cax21, cax31, cax41 |

Default: `cx23` (~$3.49/month). Pricing fetched live from API. The TUI server picker shows all available types with real-time pricing.

---

## 16. Security Model

### Trust Boundaries

| Component | Trust Level |
|-----------|------------|
| Local machine | Full — holds SSH key, API tokens |
| SSH tunnel | Encrypted, key-authenticated |
| VPS daemon | Localhost-only Unix socket |
| Claude/Codex on VPS | High privilege — passwordless sudo, permission bypass |
| Hetzner | Infrastructure provider |

### File Permissions

| File | Permissions |
|------|------------|
| `~/.cloudcode/config.toml` | 0600 |
| `~/.cloudcode/state.json` | 0600 |
| `~/.cloudcode/id_ed25519` | 0600 |
| `/home/claude/.cloudcode-env` (VPS) | 0600 |
| `/etc/cloudcode/daemon.toml` (VPS) | 0600 |

### Daemon Binding

The daemon MUST bind to `127.0.0.1:7700` only (localhost). It is never exposed on `0.0.0.0`. UFW (allow SSH only) provides defense-in-depth.

### Autonomy Tradeoffs

- `claude` user has `NOPASSWD:ALL` sudo configured — sessions spawned by the daemon **can use sudo** for installing packages, modifying system config, etc.
- Claude Code runs with `--dangerously-skip-permissions --permission-mode bypassPermissions`
- Codex runs with `--full-auto`
- VPS is single-tenant and disposable (destroyed with `down`)

---

## 17. Build System

### Workspace

3 crates with shared workspace dependencies. Version 0.1.5, Rust 2024 edition.

### Key Dependencies

| Crate | Notable Dependencies |
|-------|---------------------|
| cloudcode-cli | clap 4, ratatui 0.29, crossterm 0.28, reqwest 0.12, dialoguer 0.11, indicatif 0.17, tokio 1 |
| cloudcode-daemon | teloxide 0.13, tokio 1, regex 1, serde_json |
| cloudcode-common | serde, anyhow |

### Daemon Binary Embedding

1. `build.rs` embeds pre-compiled daemon binaries for x86_64 and aarch64
2. Included via `include_bytes!()` in release builds
3. SHA256 checksums verified on deployment
4. Fallback chain: embedded → remote build on VPS → local cross-compile

### Targets

- macOS: arm64, x86_64
- Linux: x86_64, aarch64
- Cross-compilation via `cargo-zigbuild` in CI

---

## Known Limitations

- **Codex-only deploy**: Provisioning requires Claude config even for Codex-only setups
- **File detection**: Snapshot-based (before/after send) only; `/reply` and `/type` do not trigger file detection; not real-time for interactive sessions
- **No conversation persistence**: Session history lost on daemon restart; question forwarding state is ephemeral (in-memory)
- **Codex stateless**: No `--continue` equivalent — each send is independent
- **Question detection**: Heuristic-based pattern matching; false positives possible from log output matching prompt patterns
- **OAuth tokens**: Stored on VPS in `~/.claude/credentials.json` / `~/.codex/auth.json` (persistent but revocable)
- **Telegram access control**: Chat-scoped, not user-scoped — group chats expose VPS control to all participants
- **Send timeout**: A hung AI subprocess holds the per-session lock indefinitely; no daemon-side subprocess timeout
