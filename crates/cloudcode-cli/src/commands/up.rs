use anyhow::{bail, Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::time::Duration;

use crate::config::Config;
use crate::hetzner::client::HetznerClient;
use crate::hetzner::provisioner;
use crate::state::VpsState;

fn step_bar(step: u8, total: u8, msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template(&format!("  {{spinner:.green}} [{step}/{total}] {{msg}}"))
            .expect("invalid template"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

fn finish_step(pb: &ProgressBar, step: u8, total: u8, msg: &str) {
    pb.set_style(
        ProgressStyle::default_spinner()
            .template(&format!(
                "  {} [{step}/{total}] {{msg}}",
                "✓".green()
            ))
            .expect("invalid template"),
    );
    pb.finish_with_message(msg.to_string());
}

fn fail_step(pb: &ProgressBar, step: u8, total: u8, msg: &str) {
    pb.set_style(
        ProgressStyle::default_spinner()
            .template(&format!(
                "  {} [{step}/{total}] {{msg}}",
                "✗".red()
            ))
            .expect("invalid template"),
    );
    pb.finish_with_message(msg.to_string());
}

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

    let hetzner_config = config
        .hetzner
        .as_ref()
        .context("Hetzner not configured. Run `cloudcode init` first.")?;
    let claude_config = config
        .claude
        .as_ref()
        .context("Claude not configured. Run `cloudcode init` first.")?;

    let vps_config = config.vps.as_ref();
    let server_type = vps_config
        .and_then(|v| v.server_type.as_deref())
        .unwrap_or("cx22");
    let location = vps_config
        .and_then(|v| v.location.as_deref())
        .unwrap_or("nbg1");
    let image = vps_config
        .and_then(|v| v.image.as_deref())
        .unwrap_or("ubuntu-24.04");

    println!("{}", "cloudcode up".bold().cyan());

    // Step 1: Generate cloud-init config
    let pb = step_bar(1, 5, "Generating cloud-init config...");
    let ssh_pub_key_path = Config::ssh_pub_key_path()?;
    if !ssh_pub_key_path.exists() {
        fail_step(&pb, 1, 5, "SSH public key not found");
        bail!(
            "SSH public key not found at {}. Run `cloudcode init` first.",
            ssh_pub_key_path.display()
        );
    }
    let ssh_pub_key = fs::read_to_string(&ssh_pub_key_path)
        .context("Failed to read SSH public key")?
        .trim()
        .to_string();
    let cloud_init = provisioner::generate_cloud_init(&ssh_pub_key, claude_config);
    let _ = &cloud_init; // ensure it's used
    finish_step(&pb, 1, 5, "Generating cloud-init config...");

    // Step 2: Create SSH key in Hetzner
    let pb = step_bar(2, 5, "Creating SSH key in Hetzner...");
    let client = HetznerClient::new(hetzner_config.api_token.clone());
    let ssh_key_id = match client.create_ssh_key("cloudcode", &ssh_pub_key).await {
        Ok(id) => {
            finish_step(&pb, 2, 5, "Creating SSH key in Hetzner...");
            id
        }
        Err(e) => {
            fail_step(&pb, 2, 5, "Creating SSH key in Hetzner...");
            return Err(e.context("Failed to register SSH key with Hetzner"));
        }
    };

    // Step 3: Provision server
    let pb = step_bar(
        3,
        5,
        &format!("Provisioning server ({server_type} in {location})..."),
    );
    let (server_id, server_ip) = match client
        .create_server("cloudcode", server_type, image, location, vec![ssh_key_id], &cloud_init)
        .await
    {
        Ok(result) => {
            finish_step(
                &pb,
                3,
                5,
                &format!("Provisioning server ({server_type} in {location})..."),
            );
            result
        }
        Err(e) => {
            fail_step(
                &pb,
                3,
                5,
                &format!("Provisioning server ({server_type} in {location})..."),
            );
            return Err(e.context("Failed to create server"));
        }
    };

    // Step 4: Waiting for server to be ready (report IP)
    let pb = step_bar(4, 5, "Waiting for server to be ready...");
    // The server is created but may still be initializing; we report IP immediately
    finish_step(
        &pb,
        4,
        5,
        &format!("Waiting for server to be ready...  (IP: {})", server_ip.bold()),
    );

    // Step 5: Save state
    let pb = step_bar(5, 5, "Saving state...");
    let state = VpsState {
        server_id: Some(server_id),
        server_ip: Some(server_ip),
        ssh_key_id: Some(ssh_key_id),
        status: Some("initializing".to_string()),
    };
    state.save()?;
    finish_step(&pb, 5, 5, "Saving state...");

    println!(
        "\n{}",
        "VPS provisioned successfully!".bold().green(),
    );
    println!(
        "{}",
        "Cloud-init is setting up the server. It may take a few minutes to be fully ready."
            .yellow()
    );

    Ok(())
}
