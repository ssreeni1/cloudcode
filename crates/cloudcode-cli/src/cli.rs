use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cloudcode",
    about = "Persistent cloud Claude Code sessions",
    version,
    after_help = r#"Quick start:
  cloudcode              # Launch interactive TUI
  cloudcode init         # Run setup wizard
  cloudcode up           # Provision VPS
  cloudcode spawn        # Create a Claude Code session
  cloudcode open <name>  # Connect interactively

Run `cloudcode` with no arguments for the full TUI experience."#
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
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
        /// Use classic (non-TUI) interactive prompts
        #[arg(long)]
        classic: bool,
    },
    /// Provision a Hetzner VPS
    Up {
        /// Don't wait for cloud-init to complete
        #[arg(long)]
        no_wait: bool,
        /// Server type to provision (e.g. cx23, cax11)
        #[arg(long)]
        server_type: Option<String>,
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
    /// Inspect local security-relevant state
    Doctor,
    /// Explain the trust model and revoke/rotate steps
    Security,
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
