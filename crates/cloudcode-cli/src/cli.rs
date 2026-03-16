use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cloudcode",
    about = "Persistent cloud Claude Code sessions",
    version,
    after_help = r#"Quick start:
  1. cloudcode init          # Configure Hetzner + Claude + Telegram
  2. cloudcode up            # Provision VPS (~5-10 min)
  3. cloudcode spawn         # Create a Claude Code session
  4. cloudcode open <name>    # Connect interactively"#
)]
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
    Up {
        /// Don't wait for cloud-init to complete
        #[arg(long)]
        no_wait: bool,
    },
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
    /// Open a session interactively
    #[command(alias = "attach")]
    Open {
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
    /// Restart the cloudcode daemon on the VPS
    Restart,
    /// View logs from the VPS
    Logs {
        /// Log target: "setup" (default) or "daemon"
        target: Option<String>,
    },
    /// Raw SSH access to the VPS
    Ssh {
        /// Command to run (interactive shell if omitted)
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },
}
