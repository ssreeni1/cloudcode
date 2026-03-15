use anyhow::{Context, Result};
use std::path::Path;

use crate::config::Config;
use crate::state::VpsState;

/// Upload project source to VPS via rsync
pub fn upload_source(state: &VpsState) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP")?;
    let key_path = Config::ssh_key_path()?;

    // Find workspace root from compile-time CARGO_MANIFEST_DIR
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .context("Could not determine workspace root")?;

    let ssh_opts = format!(
        "ssh -i {} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR",
        key_path.display()
    );

    // First create the target directory
    let mkdir_status = std::process::Command::new("ssh")
        .args([
            "-i",
            &key_path.to_string_lossy(),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "LogLevel=ERROR",
            "-o",
            "ConnectTimeout=10",
            &format!("claude@{}", ip),
            "mkdir -p /home/claude/cloudcode-src",
        ])
        .status()
        .context("Failed to create remote directory")?;

    if !mkdir_status.success() {
        anyhow::bail!("Failed to create remote directory");
    }

    let status = std::process::Command::new("rsync")
        .args([
            "-az",
            "--exclude",
            "target/",
            "--exclude",
            ".git/",
            "-e",
            &ssh_opts,
            &format!("{}/", workspace_root.display()),
            &format!("claude@{}:/home/claude/cloudcode-src/", ip),
        ])
        .status()
        .context("Failed to rsync source. Is rsync installed?")?;

    if !status.success() {
        anyhow::bail!("rsync failed");
    }
    Ok(())
}

/// Build daemon on the VPS
pub fn build_daemon(state: &VpsState) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP")?;
    let key_path = Config::ssh_key_path()?;

    let output = std::process::Command::new("ssh")
        .args([
            "-i",
            &key_path.to_string_lossy(),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "LogLevel=ERROR",
            "-o",
            "ConnectTimeout=10",
            &format!("claude@{}", ip),
            "source $HOME/.cargo/env && cd /home/claude/cloudcode-src && cargo build --release -p cloudcode-daemon 2>&1",
        ])
        .output()
        .context("Failed to build daemon")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!("Daemon build failed:\n{}\n{}", stdout, stderr);
    }
    Ok(())
}

/// Install daemon binary, config, and systemd unit
pub fn install_daemon(state: &VpsState, config: &crate::config::Config) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP")?;
    let key_path = Config::ssh_key_path()?;

    // Generate daemon config TOML
    let mut daemon_toml = String::from("listen_addr = \"127.0.0.1\"\nlisten_port = 7700\n");
    if let Some(ref tg) = config.telegram {
        daemon_toml.push_str(&format!(
            "\n[telegram]\nbot_token = \"{}\"\nowner_id = {}\n",
            tg.bot_token, tg.owner_id
        ));
    }

    // Build the API key environment line for systemd
    let api_key_env = if let Some(ref claude) = config.claude {
        if claude.auth_method == "api_key" {
            if let Some(ref key) = claude.api_key {
                format!("Environment=ANTHROPIC_API_KEY={}", key)
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Generate systemd unit
    let unit = format!(
        r#"[Unit]
Description=cloudcode daemon
After=network.target

[Service]
Type=simple
User=claude
ExecStart=/usr/local/bin/cloudcode-daemon /etc/cloudcode/daemon.toml
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info
{api_key_env}
WorkingDirectory=/home/claude

[Install]
WantedBy=multi-user.target
"#
    );

    // Run install commands via SSH
    let install_script = format!(
        r#"set -e
sudo cp /home/claude/cloudcode-src/target/release/cloudcode-daemon /usr/local/bin/cloudcode-daemon
sudo chmod 755 /usr/local/bin/cloudcode-daemon
sudo mkdir -p /etc/cloudcode
cat << 'DAEMON_TOML' | sudo tee /etc/cloudcode/daemon.toml > /dev/null
{daemon_toml}
DAEMON_TOML
cat << 'UNIT_FILE' | sudo tee /etc/systemd/system/cloudcode-daemon.service > /dev/null
{unit}
UNIT_FILE
sudo systemctl daemon-reload
sudo systemctl enable cloudcode-daemon
sudo systemctl restart cloudcode-daemon
"#
    );

    let output = std::process::Command::new("ssh")
        .args([
            "-i",
            &key_path.to_string_lossy(),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "LogLevel=ERROR",
            "-o",
            "ConnectTimeout=10",
            &format!("claude@{}", ip),
            &install_script,
        ])
        .output()
        .context("Failed to install daemon")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Daemon install failed: {}", stderr);
    }
    Ok(())
}

/// Verify daemon is running
pub fn verify_daemon(state: &VpsState) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP")?;
    let key_path = Config::ssh_key_path()?;

    // Wait a moment for systemd to start the service
    std::thread::sleep(std::time::Duration::from_secs(2));

    let output = std::process::Command::new("ssh")
        .args([
            "-i",
            &key_path.to_string_lossy(),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "LogLevel=ERROR",
            "-o",
            "ConnectTimeout=10",
            &format!("claude@{}", ip),
            "systemctl is-active cloudcode-daemon",
        ])
        .output()
        .context("Failed to check daemon status")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() != "active" {
        anyhow::bail!(
            "Daemon is not running. Status: {}. Check with `cloudcode ssh` then `systemctl status cloudcode-daemon`",
            stdout.trim()
        );
    }
    Ok(())
}
