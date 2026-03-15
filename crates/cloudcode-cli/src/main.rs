mod cli;
mod commands;
mod config;
mod hetzner;
mod ssh;
mod state;

use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { auto, reauth } => commands::init::run(auto, reauth).await,
        Command::Up => commands::up::run().await,
        Command::Down { force } => commands::down::run(force).await,
        Command::Status => commands::status::run().await,
        Command::Spawn { name } => commands::spawn::run(name).await,
        Command::List => commands::list::run().await,
        Command::Attach { session } => commands::attach::run(session).await,
        Command::Send { session, message } => commands::send::run(session, message).await,
        Command::Kill { session } => commands::kill::run(session).await,
    }
}
