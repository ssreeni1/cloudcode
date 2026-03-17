use anyhow::Result;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;

use crate::config::Config;
use crate::ssh::tunnel::DaemonClient;
use crate::state::VpsState;

pub async fn run(session: String) -> Result<()> {
    let config = Config::load()?;
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run /up or `cloudcode up` to provision.");
    }

    let mut client = DaemonClient::connect(&state, &config)?;
    let response = client.request(&DaemonRequest::Kill {
        session: session.clone(),
    })?;

    match response {
        DaemonResponse::Killed { session } => {
            println!("{} Session '{}' killed", "✓".green(), session);
        }
        DaemonResponse::Error { message } => {
            eprintln!("{} {}", "Error:".red(), message);
        }
        _ => eprintln!("Unexpected response from daemon"),
    }
    Ok(())
}
