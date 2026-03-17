use anyhow::{Context, Result, bail};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::time::Duration;

use crate::config::Config;
use crate::hetzner::client::HetznerClient;
use crate::hetzner::provisioner;
use crate::ssh::connection::wait_for_ssh;
use crate::ssh::health::{self, CloudInitStatus};
use crate::state::VpsState;

const TOTAL_STEPS: u8 = 10;

/// Whether to use fancy indicatif spinners (TTY) or plain println (piped).
fn is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

fn step_bar(step: u8, total: u8, msg: &str) -> ProgressBar {
    if !is_tty() {
        // When piped (e.g. from TUI subprocess), use a hidden progress bar
        // and print a plain line immediately so the TUI can capture it.
        println!("  ... [{step}/{total}] {msg}");
        ProgressBar::hidden()
    } else {
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
}

fn finish_step(pb: &ProgressBar, step: u8, total: u8, msg: &str) {
    if !is_tty() {
        println!("  ✓ [{step}/{total}] {msg}");
        return;
    }
    pb.set_style(
        ProgressStyle::default_spinner()
            .template(&format!("  {} [{step}/{total}] {{msg}}", "✓".green()))
            .expect("invalid template"),
    );
    pb.finish_with_message(msg.to_string());
}

fn fail_step(pb: &ProgressBar, step: u8, total: u8, msg: &str) {
    if !is_tty() {
        println!("  ✗ [{step}/{total}] {msg}");
        return;
    }
    pb.set_style(
        ProgressStyle::default_spinner()
            .template(&format!("  {} [{step}/{total}] {{msg}}", "✗".red()))
            .expect("invalid template"),
    );
    pb.finish_with_message(msg.to_string());
}

pub async fn run(no_wait: bool, server_type_override: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let existing_state = VpsState::load()?;

    if existing_state.is_provisioned() {
        bail!(
            "VPS already provisioned (server ID: {}, IP: {}). Run /down or `cloudcode down` first.",
            existing_state.server_id.unwrap(),
            existing_state.server_ip.as_deref().unwrap_or("unknown")
        );
    }

    let hetzner_config = config
        .hetzner
        .as_ref()
        .context("Hetzner not configured. Run /init or `cloudcode init` first.")?;
    let claude_config = config
        .claude
        .as_ref()
        .context("Claude not configured. Run /init or `cloudcode init` first.")?;

    let vps_config = config.vps.as_ref();
    let server_type = server_type_override
        .as_deref()
        .or_else(|| vps_config.and_then(|v| v.server_type.as_deref()))
        .unwrap_or("cx23");
    let location = vps_config
        .and_then(|v| v.location.as_deref())
        .unwrap_or("nbg1");
    let image = vps_config
        .and_then(|v| v.image.as_deref())
        .unwrap_or("ubuntu-24.04");

    // Cost confirmation prompt (skip if not a TTY)
    {
        use std::io::IsTerminal;
        if std::io::stdout().is_terminal() {
            let cost_str = crate::hetzner::client::estimate_monthly_cost(server_type)
                .map(|c| format!("~${:.2}/mo", c))
                .unwrap_or_else(|| "unknown cost".to_string());
            println!(
                "This will provision a {} server at {} on Hetzner. Continue? [Y/n]",
                server_type, cost_str
            );
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let trimmed = input.trim();
            if trimmed.eq_ignore_ascii_case("n") {
                println!("Aborted.");
                return Ok(());
            }

            println!(
                "Security model: the remote 'claude' user gets passwordless sudo, and Claude runs in bypass-permissions mode to support unattended remote control. Continue? [y/N]"
            );
            input.clear();
            std::io::stdin().read_line(&mut input)?;
            let trimmed = input.trim();
            if trimmed != "y" && trimmed != "Y" && !trimmed.eq_ignore_ascii_case("yes") {
                println!("Aborted.");
                return Ok(());
            }
        } else {
            println!(
                "{}",
                "Warning: cloudcode provisions a VPS where the 'claude' user has passwordless sudo and Claude runs in bypass-permissions mode."
                    .yellow()
            );
        }
    }

    println!("{}", "cloudcode up".bold().cyan());

    // Step 1: Generate cloud-init config
    let pb = step_bar(1, TOTAL_STEPS, "Generating cloud-init config...");
    let ssh_pub_key_path = Config::ssh_pub_key_path()?;
    if !ssh_pub_key_path.exists() {
        fail_step(&pb, 1, TOTAL_STEPS, "SSH public key not found");
        bail!(
            "SSH public key not found at {}. Run /init or `cloudcode init` first.",
            ssh_pub_key_path.display()
        );
    }
    let ssh_pub_key = fs::read_to_string(&ssh_pub_key_path)
        .context("Failed to read SSH public key")?
        .trim()
        .to_string();
    let cloud_init = provisioner::generate_cloud_init(&ssh_pub_key, claude_config);
    let _ = &cloud_init; // ensure it's used
    finish_step(&pb, 1, TOTAL_STEPS, "Generated cloud-init config");

    // Step 2: Create SSH key in Hetzner
    let pb = step_bar(2, TOTAL_STEPS, "Creating SSH key in Hetzner...");
    let client = HetznerClient::new(hetzner_config.api_token.clone());
    let ssh_key_id = match client.create_ssh_key("cloudcode", &ssh_pub_key).await {
        Ok(id) => {
            finish_step(&pb, 2, TOTAL_STEPS, "Created SSH key in Hetzner");
            id
        }
        Err(e) => {
            fail_step(&pb, 2, TOTAL_STEPS, "Failed to create SSH key in Hetzner");
            return Err(e.context("Failed to register SSH key with Hetzner"));
        }
    };

    // Save partial state immediately to prevent orphaned SSH keys
    let mut state = VpsState {
        server_id: None,
        server_ip: None,
        ssh_key_id: Some(ssh_key_id),
        status: Some("creating".to_string()),
    };
    state.save()?;

    // Step 3: Provision server
    let pb = step_bar(
        3,
        TOTAL_STEPS,
        &format!("Provisioning server ({server_type} in {location})..."),
    );
    let (server_id, server_ip) = match client
        .create_server(
            "cloudcode",
            server_type,
            image,
            location,
            vec![ssh_key_id],
            &cloud_init,
        )
        .await
    {
        Ok(result) => {
            finish_step(
                &pb,
                3,
                TOTAL_STEPS,
                &format!("Provisioned server ({server_type} in {location})"),
            );
            result
        }
        Err(e) => {
            fail_step(
                &pb,
                3,
                TOTAL_STEPS,
                &format!("Failed to provision server ({server_type} in {location})"),
            );
            return Err(e.context("Failed to create server"));
        }
    };

    // Save full state so `down` works even if later steps fail
    state.server_id = Some(server_id);
    state.server_ip = Some(server_ip.clone());
    state.status = Some("initializing".to_string());
    state.save()?;

    // Check no_wait flag
    if no_wait {
        println!(
            "\n{}",
            "VPS provisioned. Skipping cloud-init wait (--no-wait).".yellow()
        );
        println!(
            "{}",
            "Cloud-init is still running. Use /status or `cloudcode status` to check progress."
                .yellow()
        );
        return Ok(());
    }

    // Step 4: Wait for SSH connectivity
    let pb = step_bar(4, TOTAL_STEPS, "Waiting for SSH connectivity...");
    match wait_for_ssh(&state, Duration::from_secs(120)).await {
        Ok(()) => {
            finish_step(&pb, 4, TOTAL_STEPS, "SSH is reachable");
        }
        Err(e) => {
            fail_step(&pb, 4, TOTAL_STEPS, "SSH connectivity timed out");
            println!("\n{}: {}", "Warning".yellow().bold(), e);
            println!(
                "{}",
                "The server may still be starting. Try /status or `cloudcode status` later."
                    .yellow()
            );
            return Ok(());
        }
    }

    // Step 5: Wait for cloud-init completion
    let pb = step_bar(5, TOTAL_STEPS, "Waiting for cloud-init to complete...");
    match health::wait_for_cloud_init(&state, Duration::from_secs(600)).await? {
        CloudInitStatus::Ready => {
            finish_step(&pb, 5, TOTAL_STEPS, "Cloud-init completed successfully");
        }
        CloudInitStatus::Failed { error } => {
            fail_step(&pb, 5, TOTAL_STEPS, "Cloud-init failed");
            println!("\n{}: {}", "Error".red().bold(), error);
            println!(
                "{}",
                "Check logs with: /ssh -- cat /var/log/cloudcode-setup.log (or cloudcode ssh ...)"
                    .yellow()
            );
            state.status = Some("error".to_string());
            state.save()?;
            return Ok(());
        }
    }

    // Step 6: Verify installation
    let pb = step_bar(6, TOTAL_STEPS, "Verifying installed software...");
    match health::verify_installation(&state).await {
        Ok(results) => {
            let all_ok = results.iter().all(|(_, ok)| *ok);
            if all_ok {
                finish_step(&pb, 6, TOTAL_STEPS, "All software verified");
            } else {
                let missing: Vec<_> = results
                    .iter()
                    .filter(|(_, ok)| !ok)
                    .map(|(name, _)| name.as_str())
                    .collect();
                fail_step(
                    &pb,
                    6,
                    TOTAL_STEPS,
                    &format!("Missing software: {}", missing.join(", ")),
                );
                println!(
                    "\n{}: Some expected software is missing: {}",
                    "Warning".yellow().bold(),
                    missing.join(", ")
                );
            }
        }
        Err(e) => {
            fail_step(&pb, 6, TOTAL_STEPS, "Failed to verify installation");
            println!("\n{}: {}", "Warning".yellow().bold(), e);
        }
    }

    // Step 7: Prepare daemon binary (embedded or cross-compile fallback)
    let target = crate::deploy::target_triple_for_server_type(server_type);
    let pb = step_bar(7, TOTAL_STEPS, &format!("Preparing daemon for {target}..."));
    let binary_path = match crate::deploy::get_daemon_binary(target) {
        Ok(path) => {
            finish_step(&pb, 7, TOTAL_STEPS, &format!("Daemon ready for {target}"));
            path
        }
        Err(e) => {
            fail_step(&pb, 7, TOTAL_STEPS, "Cross-compilation failed");
            println!("\n{}: {}", "Error".red().bold(), e);
            state.status = Some("error".to_string());
            state.save()?;
            return Ok(());
        }
    };

    // Step 8: Upload daemon binary
    let pb = step_bar(8, TOTAL_STEPS, "Uploading daemon binary to VPS...");
    match crate::deploy::upload_binary(&state, &binary_path) {
        Ok(()) => {
            finish_step(&pb, 8, TOTAL_STEPS, "Daemon binary uploaded");
        }
        Err(e) => {
            fail_step(&pb, 8, TOTAL_STEPS, "Failed to upload binary");
            println!("\n{}: {}", "Error".red().bold(), e);
            state.status = Some("error".to_string());
            state.save()?;
            return Ok(());
        }
    }

    // Step 9: Install daemon config + systemd
    let pb = step_bar(9, TOTAL_STEPS, "Installing daemon service...");
    match crate::deploy::install_daemon(&state, &config) {
        Ok(()) => {
            finish_step(&pb, 9, TOTAL_STEPS, "Daemon service installed");
        }
        Err(e) => {
            fail_step(&pb, 9, TOTAL_STEPS, "Failed to install daemon service");
            println!("\n{}: {}", "Error".red().bold(), e);
            state.status = Some("error".to_string());
            state.save()?;
            return Ok(());
        }
    }

    // Step 10: Verify daemon + finalize
    let pb = step_bar(10, TOTAL_STEPS, "Verifying daemon is running...");
    match crate::deploy::verify_daemon(&state) {
        Ok(()) => {
            state.status = Some("running".to_string());
            state.save()?;
            finish_step(&pb, 10, TOTAL_STEPS, "Daemon is running");
        }
        Err(e) => {
            fail_step(&pb, 10, TOTAL_STEPS, "Daemon is not running");
            println!("\n{}: {}", "Warning".yellow().bold(), e);
            state.status = Some("running".to_string());
            state.save()?;
        }
    }

    println!(
        "\n  {} {}",
        "✓".green().bold(),
        "VPS provisioned and daemon deployed successfully!"
            .bold()
            .green(),
    );

    println!("\n  {}", "Next steps:".bold());
    println!(
        "    {}              # Create a Claude Code session",
        "/spawn".cyan().bold()
    );
    println!(
        "    {}  # Connect interactively",
        "/open <name>".cyan().bold()
    );
    println!(
        "    {}",
        "(or use cloudcode spawn / cloudcode open <name> from CLI)".dimmed()
    );

    if let Some(ref claude) = config.claude {
        if claude.auth_method == "oauth" {
            println!(
                "\n  {}  {}",
                "!".yellow().bold(),
                "OAuth login required".yellow().bold()
            );
            println!(
                "    Run {} after spawning to log in.",
                "/open <name>".cyan().bold()
            );
            println!(
                "    Claude will show a login URL — {} to copy it.",
                "highlight and copy the URL manually".bold()
            );
            println!(
                "    {}",
                "(Pressing 'c' copies to the VPS clipboard, not your local machine.)".dimmed()
            );
            println!("    Open the URL in your local browser to complete the auth flow.");
            if config.telegram.is_some() {
                println!(
                    "\n  {}  {}",
                    "!".yellow().bold(),
                    "Telegram will not work until OAuth login is complete.".yellow()
                );
            }
        }
    }

    if config.telegram.is_some() {
        println!("\n  {}", "Telegram:".bold());
        println!("    Your bot is active! Message it to start chatting.");
        println!(
            "    Send {} to create a session, then type any message.",
            "/spawn".cyan().bold()
        );
    }

    Ok(())
}
