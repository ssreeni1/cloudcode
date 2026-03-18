mod cli;
mod commands;
mod config;
mod deploy;
mod hetzner;
mod paths;
mod ssh;
mod state;
mod tui;

use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        // No subcommand → launch full TUI (wizard if needed, then command view)
        None => tui::run_tui(false).await,

        // `cloudcode init` → TUI wizard (always), or classic with --classic/--reauth
        Some(Command::Init {
            auto,
            reauth,
            classic,
        }) => {
            if classic || reauth || !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                commands::init::run(auto, reauth).await
            } else {
                tui::run_tui(true).await
            }
        }

        // All other subcommands run directly (no TUI)
        Some(Command::Up {
            no_wait,
            server_type,
        }) => commands::up::run(no_wait, server_type).await,
        Some(Command::Down { force }) => commands::down::run(force).await,
        Some(Command::Status) => commands::status::run().await,
        Some(Command::Doctor) => commands::doctor::run().await,
        Some(Command::Security) => commands::security::run().await,
        Some(Command::Spawn { name }) => commands::spawn::run(name).await,
        Some(Command::List) => commands::list::run().await,
        Some(Command::Open { session }) => commands::attach::run(session).await,
        Some(Command::Send { session, message }) => commands::send::run(session, message).await,
        Some(Command::Kill { session }) => commands::kill::run(session).await,
        Some(Command::Restart) => commands::restart::run().await,
        Some(Command::Logs { target }) => commands::logs::run(target).await,
        Some(Command::Ssh { command }) => commands::ssh_cmd::run(command).await,
    }
}
