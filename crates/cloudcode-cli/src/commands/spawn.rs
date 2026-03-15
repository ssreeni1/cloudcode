use anyhow::Result;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;

use crate::config::Config;
use crate::ssh::tunnel::DaemonClient;
use crate::state::VpsState;

pub async fn run(name: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run `cloudcode up` first.");
    }

    let mut client = DaemonClient::connect(&state, &config)?;
    let response = client.request(&DaemonRequest::Spawn { name })?;

    match response {
        DaemonResponse::Spawned { session } => {
            println!("{} Session '{}' created", "✓".green(), session.name);
            println!("  Attach with: cloudcode attach {}", session.name);
        }
        DaemonResponse::Error { message } => {
            eprintln!("{} {}", "Error:".red(), message);
        }
        _ => eprintln!("Unexpected response from daemon"),
    }
    Ok(())
}
