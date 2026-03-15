use anyhow::{bail, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::config::Config;
use crate::hetzner::provisioner;
use crate::state::VpsState;

pub async fn run() -> Result<()> {
    let config = Config::load()?;
    let existing_state = VpsState::load()?;

    if existing_state.is_provisioned() {
        bail!(
            "VPS already provisioned (server ID: {}, IP: {}). Run `cloudcode down` first.",
            existing_state.server_id.unwrap(),
            existing_state.server_ip.as_deref().unwrap_or("unknown")
        );
    }

    println!("{}", "Provisioning VPS...".bold().cyan());

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("invalid template"),
    );
    pb.set_message("Creating server on Hetzner...");
    pb.enable_steady_tick(Duration::from_millis(100));

    let state = provisioner::provision(&config).await?;

    pb.finish_with_message("Server created!");

    state.save()?;

    println!(
        "\n{} {}",
        "VPS provisioned successfully!".bold().green(),
        format!(
            "(ID: {}, IP: {})",
            state.server_id.unwrap(),
            state.server_ip.as_deref().unwrap_or("pending")
        )
        .dimmed()
    );
    println!(
        "{}",
        "Cloud-init is setting up the server. It may take a few minutes to be fully ready."
            .yellow()
    );

    Ok(())
}
