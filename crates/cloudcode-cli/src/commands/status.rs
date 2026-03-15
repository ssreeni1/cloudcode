use anyhow::Result;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;

use crate::config::Config;
use crate::hetzner::client::HetznerClient;
use crate::ssh::tunnel::DaemonClient;
use crate::state::VpsState;

pub async fn run() -> Result<()> {
    let state = VpsState::load()?;

    if !state.is_provisioned() {
        println!("{}", "No VPS is currently provisioned.".yellow());
        println!("Run {} to provision one.", "cloudcode up".bold());
        return Ok(());
    }

    let config = Config::load()?;
    let hetzner_config = config
        .hetzner
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Hetzner not configured"))?;

    let client = HetznerClient::new(hetzner_config.api_token.clone());
    let server_id = state.server_id.unwrap();

    match client.get_server(server_id).await {
        Ok(info) => {
            println!("{}", "VPS Status".bold().cyan());
            println!("  Name:   {}", info.name);
            println!("  ID:     {}", info.id);
            println!("  Status: {}", colorize_status(&info.status));
            println!("  IP:     {}", info.ip);
        }
        Err(e) => {
            println!("{}: {}", "Failed to query server status".red(), e);
            println!("  Cached ID: {}", server_id);
            println!(
                "  Cached IP: {}",
                state.server_ip.as_deref().unwrap_or("unknown")
            );
        }
    }

    // Query daemon status if VPS is provisioned
    println!();
    match DaemonClient::connect(&state, &config) {
        Ok(daemon) => match daemon.request(&DaemonRequest::Status) {
            Ok(DaemonResponse::Status {
                uptime_secs,
                sessions,
            }) => {
                println!("{}", "Daemon Status".bold().cyan());
                println!("  Uptime:   {}", format_uptime(uptime_secs));
                println!("  Sessions: {}", sessions.len());
                for s in &sessions {
                    println!(
                        "    {} [{}]",
                        s.name.green(),
                        format!("{:?}", s.state).yellow()
                    );
                }
            }
            Ok(DaemonResponse::Error { message }) => {
                println!("{} Daemon error: {}", "!".red(), message);
            }
            Ok(_) => {
                println!("{} Unexpected daemon response", "!".yellow());
            }
            Err(e) => {
                println!("{} Could not query daemon: {}", "!".yellow(), e);
            }
        },
        Err(e) => {
            println!(
                "{} Could not connect to daemon: {}",
                "!".yellow(),
                e
            );
        }
    }

    Ok(())
}

fn colorize_status(status: &str) -> String {
    match status {
        "running" => status.green().to_string(),
        "initializing" | "starting" => status.yellow().to_string(),
        "off" | "stopping" => status.red().to_string(),
        _ => status.to_string(),
    }
}

fn format_uptime(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}
