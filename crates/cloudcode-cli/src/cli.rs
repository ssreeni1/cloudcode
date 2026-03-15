use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cloudcode", about = "Persistent cloud Claude Code sessions")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Set up cloudcode (Hetzner, Claude auth, Telegram)
    Init {
        /// AI-assisted setup mode
        #[arg(long)]
        auto: bool,
        /// Re-authenticate only
        #[arg(long)]
        reauth: bool,
    },
    /// Provision a Hetzner VPS
    Up,
    /// Destroy the VPS
    Down {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Create a new Claude Code session
    Spawn {
        /// Session name (auto-generated if omitted)
        name: Option<String>,
    },
    /// List active sessions
    List,
    /// Attach to a session (interactive PTY)
    Attach {
        /// Session name
        session: String,
    },
    /// Send a message to a session
    Send {
        /// Session name
        session: String,
        /// Message to send
        message: String,
    },
    /// Kill a session
    Kill {
        /// Session name
        session: String,
    },
    /// Show VPS and session status
    Status,
}
