# cloudcode

Persistent cloud Claude Code sessions on your own Hetzner VPS. Run Claude from anywhere — your terminal, your phone via Telegram, or both at once.

## What it does

cloudcode provisions a cheap Hetzner VPS, deploys a daemon that manages Claude Code sessions via tmux, and gives you three ways to interact:

- **TUI** — a terminal interface with command history, server type picker, and full session management
- **CLI** — individual commands (`cloudcode spawn`, `cloudcode open`, `cloudcode send`)
- **Telegram** — message a bot on your phone, get Claude responses back with file attachments

Sessions persist across disconnects. You can start a task from your laptop, check progress from your phone, and pick it back up later.

## Install

### Homebrew (recommended)

```bash
brew install ssreeni1/tap/cloudcode
```

### One-liner (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/ssreeni1/cloudcode/main/install.sh | bash
```

This downloads the latest release binary for your platform and installs it to `/usr/local/bin/cloudcode`.

### From source

```bash
git clone https://github.com/ssreeni1/cloudcode.git
cd cloudcode
cargo install --path crates/cloudcode-cli
```

Requires Rust 2024 edition (1.85+).

## Prerequisites

Before running cloudcode, you need:

1. **Hetzner Cloud account** — sign up at [console.hetzner.cloud](https://console.hetzner.cloud). Create an API token with **Read & Write** access under Security > API Tokens.

2. **Claude authentication** — either:
   - An **API key** from [console.anthropic.com/settings/keys](https://console.anthropic.com/settings/keys), or
   - **OAuth** — you'll log in via browser after provisioning (no key needed upfront)

3. **Telegram bot** (optional) — create one via [@BotFather](https://t.me/BotFather) on Telegram. You'll need the bot token and your numeric user ID (get it from [@userinfobot](https://t.me/userinfobot)).

4. **ssh** and **rsync** on your local machine (pre-installed on macOS and most Linux).

## Quick start

```bash
# Launch the TUI — the setup wizard runs automatically on first use
cloudcode
```

The wizard walks you through:
1. Hetzner API token (validated live)
2. Claude authentication method
3. Telegram bot setup (optional)
4. SSH keypair generation

Once configured:

```
/up          # Provision a VPS (pick server type interactively)
/spawn       # Create a Claude Code session
/open <name> # Attach to the session
```

That's it. Claude is running on your VPS.

## Commands

### TUI mode

Run `cloudcode` with no arguments to launch the TUI. Type commands with a `/` prefix:

| Command | Description |
|---------|-------------|
| `/up` | Provision VPS (shows server type picker) |
| `/down` | Destroy VPS (with confirmation) |
| `/status` | Show VPS and session status |
| `/spawn [name]` | Create a Claude session |
| `/list` | List active sessions |
| `/open <session>` | Attach to session interactively |
| `/send <session> <msg>` | Send message to session |
| `/kill <session>` | Kill a session |
| `/restart` | Restart daemon on VPS |
| `/logs [setup\|daemon]` | View VPS logs |
| `/ssh [cmd]` | SSH to VPS |
| `/init` | Re-run setup wizard |
| `/help` | Show command reference |

### CLI mode

Every TUI command has a CLI equivalent:

```bash
cloudcode up                    # Provision VPS
cloudcode spawn                 # Create session
cloudcode open my-session       # Attach interactively
cloudcode send my-session "fix the login bug"
cloudcode status                # Show status
cloudcode list                  # List sessions
cloudcode kill my-session       # Kill session
cloudcode down                  # Destroy VPS
cloudcode logs daemon           # View daemon logs
cloudcode ssh                   # Interactive SSH
cloudcode ssh -- ls /home/claude  # Run remote command
```

### Telegram

Once configured, message your bot:

| Command | Description |
|---------|-------------|
| `/spawn [name]` | Create a session |
| `/list` | List sessions |
| `/kill <name>` | Kill a session |
| `/use <name>` | Set default session |
| `/status` | Show daemon status |
| `/help` | Show commands |

Send any text message to interact with the default session. Claude responds with text and any files it creates (images, documents, etc.).

## Configuration

All configuration is stored in `~/.cloudcode/`:

```
~/.cloudcode/
  config.toml      # Hetzner token, Claude auth, Telegram bot config
  id_ed25519       # SSH private key (generated during setup)
  id_ed25519.pub   # SSH public key
  state.json       # Current VPS state (server ID, IP, status)
```

### config.toml structure

```toml
[hetzner]
api_token = "your-hetzner-api-token"

[claude]
auth_method = "api_key"  # or "oauth"
api_key = "sk-ant-..."   # only if auth_method = "api_key"

[telegram]  # optional
bot_token = "123456:ABC..."
owner_id = 123456789

[vps]  # optional overrides
server_type = "cx23"        # default: cx23
location = "nbg1"           # default: nbg1 (Nuremberg)
image = "ubuntu-24.04"      # default: ubuntu-24.04
```

## How it works

### Architecture

```
Your machine                         Hetzner VPS
+-----------------+                  +----------------------------+
| cloudcode CLI   |  SSH tunnel      | cloudcode-daemon           |
| (TUI or CLI)    | <=============> | (systemd service)          |
+-----------------+                  |   |                        |
                                     |   +-- tmux sessions        |
+-----------------+                  |   |     +-- claude (1)     |
| Telegram bot    |  Telegram API    |   |     +-- claude (2)     |
| (on your phone) | <=============> |   |     +-- ...             |
+-----------------+                  +----------------------------+
```

- The **CLI** communicates with the daemon over an SSH-forwarded Unix socket
- The **daemon** manages tmux sessions, each running a Claude Code instance
- **Telegram** connects directly to the daemon's bot module
- Sessions persist in tmux — disconnect and reconnect anytime

### Provisioning flow (`/up`)

1. Generate cloud-init config
2. Create SSH key in Hetzner
3. Provision server (your chosen type and location)
4. Wait for SSH connectivity
5. Wait for cloud-init (installs tmux, Claude Code)
6. Verify installation
7. Prepare daemon binary (extracted from embedded binary or cross-compiled)
8. Upload daemon binary via scp
9. Install daemon config + systemd service
10. Verify daemon is running

For release builds, the daemon binary is pre-compiled and embedded in the CLI binary — no compilation happens on the VPS. Provisioning takes ~3-5 minutes.

## Security model

**Read this before using cloudcode.** The security model makes deliberate tradeoffs for unattended remote operation.

### What cloudcode provisions

When you run `/up`, cloudcode creates a Hetzner VPS with:

- A `claude` user with **passwordless sudo** (`NOPASSWD:ALL`)
- Claude Code running in **bypass-permissions mode** (`--dangerously-skip-permissions`)
- A daemon listening on **localhost only** (127.0.0.1:7700)
- UFW firewall allowing **SSH only** (port 22)
- All communication over SSH tunnels (no exposed ports besides SSH)

### Why these tradeoffs exist

Claude Code needs to install packages, edit system files, and run arbitrary commands to be useful as an autonomous coding agent. Passwordless sudo and bypass-permissions mode enable this without interactive prompts that would break unattended sessions.

The daemon binds to localhost — it's only reachable through the SSH tunnel, which requires your private key.

### Trust boundaries

| Component | Trust level |
|-----------|-------------|
| Your local machine | Full trust (holds SSH key, API tokens) |
| SSH tunnel | Encrypted, key-authenticated |
| VPS daemon | Trusted (you deployed it, localhost-only) |
| Claude Code on VPS | **High privilege** — can do anything as the `claude` user with sudo |
| Telegram bot | Messages only accepted from your `owner_id` |
| Hetzner | Infrastructure provider — has physical access to the VPS |

### What's protected

- **SSH key** (`~/.cloudcode/id_ed25519`) — file permissions 0600, generated locally, never leaves your machine
- **Config file** (`~/.cloudcode/config.toml`) — file permissions 0600, contains API tokens
- **VPS secrets** (`/home/claude/.cloudcode-env`) — file permissions 0600, contains API key on the VPS
- **Daemon config** (`/etc/cloudcode/daemon.toml`) — file permissions 0600, contains Telegram bot token
- **Embedded binaries** — SHA256 verified at build time, checked against checksums

### What's NOT protected

- **Claude has root-equivalent access** on the VPS via sudo. A prompt injection or malicious instruction to Claude could compromise the VPS.
- **The VPS is a single-tenant throwaway.** Don't store sensitive data on it that you wouldn't want Claude to access. Use `/down` to destroy it when done.
- **OAuth tokens** (if using OAuth auth) are stored on the VPS after login. Anyone with SSH access to the VPS can access them.

### Recommendations

- **Use API keys** instead of OAuth when possible — they're scoped and revocable without accessing the VPS
- **Destroy the VPS** (`/down`) when not in use — you only pay while it's running and it eliminates persistent attack surface
- **Rotate your Hetzner API token** periodically
- **Don't run cloudcode on a VPS with other workloads** — treat it as a disposable Claude sandbox
- **Review Claude's work** before deploying anything it produces to production

### Revoking access

- **Destroy the VPS**: `/down` or `cloudcode down` — deletes the server and SSH key from Hetzner
- **Rotate Hetzner token**: Generate a new one in the Hetzner console, run `/init` to update
- **Rotate Claude API key**: Generate a new one in the Anthropic console, run `/init --reauth`
- **Revoke Telegram bot**: Message @BotFather with `/deletebot`
- **Delete local state**: `rm -rf ~/.cloudcode`

## Server types

cloudcode supports all current Hetzner shared CPU types. The `/up` command shows an interactive picker with live pricing:

| Type | CPUs | RAM | Disk | ~Cost/mo |
|------|------|-----|------|----------|
| cx23 | 2 | 4 GB | 40 GB | $3.99 |
| cx33 | 2 | 8 GB | 80 GB | $6.49 |
| cx43 | 4 | 16 GB | 160 GB | $14.99 |
| cx53 | 8 | 32 GB | 240 GB | $29.99 |
| cax11 | 2 | 4 GB | 40 GB | $3.49 |
| cax21 | 4 | 8 GB | 80 GB | $5.49 |
| cax31 | 8 | 16 GB | 160 GB | $9.49 |
| cax41 | 16 | 32 GB | 320 GB | $16.49 |

`cx` types are x86_64, `cax` types are ARM (Ampere). ARM types are slightly cheaper. Both work identically with cloudcode.

The default is `cx23` ($3.99/mo) which is plenty for most Claude Code sessions.

## Development

```bash
git clone https://github.com/ssreeni1/cloudcode.git
cd cloudcode

# Build everything
cargo build

# Run tests
cargo test --workspace

# Run the CLI locally
cargo run -p cloudcode-cli

# Run with a specific command
cargo run -p cloudcode-cli -- status
```

### Project structure

```
crates/
  cloudcode-cli/       # CLI + TUI (the main binary users install)
  cloudcode-daemon/    # Daemon that runs on the VPS
  cloudcode-common/    # Shared types (protocol, session info)
```

### Release builds

Releases are automated via GitHub Actions. Tag a version to trigger:

```bash
git tag v0.1.1
git push origin v0.1.1
```

The CI pipeline:
1. Cross-compiles the daemon for x86_64 and aarch64 Linux
2. Embeds both daemon binaries into the CLI via `include_bytes!()`
3. Builds the CLI for macOS (arm64, x86) and Linux (x86, arm64)
4. Publishes binaries + SHA256 checksums to GitHub Releases

## License

MIT — see [LICENSE](LICENSE).
