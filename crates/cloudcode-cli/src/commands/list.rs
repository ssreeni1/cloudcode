use anyhow::Result;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;

use crate::config::Config;
use crate::ssh::tunnel::DaemonClient;
use crate::state::VpsState;

pub async fn run() -> Result<()> {
    let config = Config::load()?;
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run `cloudcode up` first.");
    }

    let client = DaemonClient::connect(&state, &config)?;
    let response = client.request(&DaemonRequest::List)?;

    match response {
        DaemonResponse::Sessions { sessions } => {
            if sessions.is_empty() {
                println!("No active sessions.");
            } else {
                println!("{}", "Active sessions:".bold());
                for s in &sessions {
                    println!(
                        "  {} [{}]",
                        s.name.green(),
                        format!("{:?}", s.state).yellow()
                    );
                }
            }
        }
        DaemonResponse::Error { message } => {
            eprintln!("{} {}", "Error:".red(), message);
        }
        _ => eprintln!("Unexpected response from daemon"),
    }
    Ok(())
}
