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
        anyhow::bail!("No VPS provisioned. Run /up or `cloudcode up` to provision.");
    }

    let mut client = DaemonClient::connect(&state, &config)?;
    let response = client.request(&DaemonRequest::List)?;

    match response {
        DaemonResponse::Sessions { sessions } => {
            if sessions.is_empty() {
                println!("No active sessions.");
            } else {
                println!("{}", "Active sessions:".bold());
                for s in &sessions {
                    let provider = s.provider.as_deref().unwrap_or("unknown");
                    println!(
                        "  {} [{}] ({})",
                        s.name.green(),
                        format!("{:?}", s.state).yellow(),
                        provider.dimmed()
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
