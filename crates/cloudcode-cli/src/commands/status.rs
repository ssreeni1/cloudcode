use anyhow::Result;
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;
use std::time::Duration;

use crate::config::Config;
use crate::hetzner::client::{HetznerClient, estimate_monthly_cost};
use crate::ssh::health::{self, BrowserAutomationStatus, CloudInitStatus};
use crate::ssh::tunnel::DaemonClient;
use crate::state::{VpsState, VpsStatus};

pub async fn run() -> Result<()> {
    let state = VpsState::load()?;

    if !state.is_provisioned() {
        println!("{}", "No VPS is currently provisioned.".yellow());
        println!(
            "Run {} or {} to provision one.",
            "/up".bold(),
            "cloudcode up".bold()
        );
        return Ok(());
    }

    let config = Config::load()?;
    let hetzner_config = config
        .hetzner
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Hetzner not configured"))?;

    let client = HetznerClient::new(hetzner_config.api_token.clone());
    let server_id = state.server_id.unwrap();

    let vps_config = config.vps.as_ref();
    let server_type = vps_config
        .and_then(|v| v.server_type.as_deref())
        .unwrap_or("cx23");
    let location = vps_config
        .and_then(|v| v.location.as_deref())
        .unwrap_or("nbg1");

    println!("{}", "cloudcode status".bold().cyan());
    println!();

    match client.get_server(server_id).await {
        Ok(info) => {
            println!(
                "  {:<12} {} ({} in {})",
                "VPS:".bold(),
                colorize_status(&info.status),
                server_type.dimmed(),
                location.dimmed()
            );
            println!("  {:<12} {}", "IP:".bold(), info.ip);
            println!("  {:<12} {}", "Server ID:".bold(), info.id);
            println!("  {:<12} {}", "Name:".bold(), info.name);

            if let Some(cost) = estimate_monthly_cost(server_type) {
                println!(
                    "  {:<12} {}",
                    "Cost:".bold(),
                    format!("~${:.2}/mo", cost).yellow()
                );
            }
        }
        Err(e) => {
            println!(
                "  {} {}",
                "✗".red().bold(),
                format!("Failed to query server status: {e}").red()
            );
            println!("  {:<12} {}", "Server ID:".bold(), server_id);
            println!(
                "  {:<12} {}",
                "IP:".bold(),
                state.server_ip.as_deref().unwrap_or("unknown")
            );
        }
    }

    // If state is initializing, check cloud-init status
    if state.status == Some(VpsStatus::Initializing) {
        println!();
        println!(
            "  {:<12} {}",
            "Setup:".bold(),
            "checking cloud-init...".yellow()
        );
        match health::wait_for_cloud_init(&state, Duration::from_secs(5)).await {
            Ok(CloudInitStatus::Ready) => {
                println!("  {:<12} {}", "Cloud-init:".bold(), "ready".green());
            }
            Ok(CloudInitStatus::Failed { error }) => {
                println!(
                    "  {:<12} {} ({})",
                    "Cloud-init:".bold(),
                    "failed".red(),
                    error.dimmed()
                );
            }
            Err(e) => {
                println!(
                    "  {:<12} {} ({})",
                    "Cloud-init:".bold(),
                    "unknown".yellow(),
                    format!("{e}").dimmed()
                );
            }
        }
    }

    println!();
    match health::browser_automation_status(&state).await {
        Ok(BrowserAutomationStatus::Ready) => {
            println!("  {:<12} {}", "Browser:".bold(), "ready".green());
        }
        Ok(BrowserAutomationStatus::Installing) => {
            println!(
                "  {:<12} {}",
                "Browser:".bold(),
                "installing Playwright in background".yellow()
            );
        }
        Ok(BrowserAutomationStatus::Pending) => {
            println!(
                "  {:<12} {}",
                "Browser:".bold(),
                "queued for background setup".yellow()
            );
        }
        Ok(BrowserAutomationStatus::Failed { error }) => {
            println!(
                "  {:<12} {} ({})",
                "Browser:".bold(),
                "failed".red(),
                error.dimmed()
            );
        }
        Ok(BrowserAutomationStatus::Unknown) | Err(_) => {
            println!("  {:<12} {}", "Browser:".bold(), "unknown".dimmed());
        }
    }

    // Query daemon status if VPS is provisioned
    println!();
    match DaemonClient::connect(&state, &config) {
        Ok(mut daemon) => match daemon.request(&DaemonRequest::Status) {
            Ok(DaemonResponse::Status {
                uptime_secs,
                sessions,
                telegram,
            }) => {
                println!(
                    "  {:<12} {} (uptime: {})",
                    "Daemon:".bold(),
                    "connected".green(),
                    format_uptime(uptime_secs).dimmed()
                );
                println!(
                    "  {:<12} {}",
                    "Sessions:".bold(),
                    if sessions.is_empty() {
                        "none".dimmed().to_string()
                    } else {
                        format!("{} active", sessions.len())
                    }
                );
                for s in &sessions {
                    println!(
                        "    {} [{}]",
                        s.name.green(),
                        format!("{:?}", s.state).yellow()
                    );
                }
                if let Some(tg) = telegram {
                    println!(
                        "  {:<12} {} (mode: {})",
                        "Telegram:".bold(),
                        if tg.connected {
                            "connected".green().to_string()
                        } else {
                            "disconnected".yellow().to_string()
                        },
                        tg.mode.dimmed()
                    );
                }
            }
            Ok(DaemonResponse::Error { message }) => {
                println!(
                    "  {:<12} {} {}",
                    "Daemon:".bold(),
                    "error".red(),
                    message.dimmed()
                );
            }
            Ok(_) => {
                println!(
                    "  {:<12} {}",
                    "Daemon:".bold(),
                    "unexpected response".yellow()
                );
            }
            Err(e) => {
                println!(
                    "  {:<12} {} ({})",
                    "Daemon:".bold(),
                    "unreachable".yellow(),
                    format!("{e}").dimmed()
                );
            }
        },
        Err(e) => {
            println!(
                "  {:<12} {} ({})",
                "Daemon:".bold(),
                "disconnected".yellow(),
                format!("{e}").dimmed()
            );
        }
    }

    Ok(())
}

fn colorize_status(status: &str) -> String {
    match status {
        "running" => status.green().bold().to_string(),
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
