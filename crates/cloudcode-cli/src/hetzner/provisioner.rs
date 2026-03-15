use anyhow::{bail, Context, Result};
use std::fs;

use crate::config::{ClaudeConfig, Config};
use crate::hetzner::client::HetznerClient;
use crate::state::VpsState;

pub fn generate_cloud_init(ssh_pub_key: &str, claude_auth: &ClaudeConfig) -> String {
    let api_key_lines = if claude_auth.auth_method == "api_key" {
        if let Some(ref key) = claude_auth.api_key {
            format!(
                r#"echo 'export ANTHROPIC_API_KEY="{key}"' >> /home/claude/.bashrc
echo 'export ANTHROPIC_API_KEY="{key}"' >> /home/claude/.profile"#
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    format!(
        r##"#cloud-config
users:
  - name: claude
    groups: sudo
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    ssh_authorized_keys:
      - {ssh_pub_key}

package_update: true
packages:
  - tmux
  - curl
  - jq
  - build-essential
  - pkg-config
  - libssl-dev
  - git

runcmd:
  - |
    exec > /var/log/cloudcode-setup.log 2>&1
    set -euo pipefail

    echo "=== cloudcode setup started at $(date) ==="

    # Set API key environment
    {api_key_lines}

    # Install Claude Code with retries
    CLAUDE_INSTALLED=false
    for attempt in 1 2 3; do
      echo "Claude Code install attempt $attempt..."
      if su - claude -c 'curl -fsSL https://claude.ai/install.sh | sh'; then
        CLAUDE_INSTALLED=true
        break
      fi
      echo "Attempt $attempt failed, waiting 10s..."
      sleep 10
    done

    if [ "$CLAUDE_INSTALLED" = false ]; then
      echo '{{"status":"error","error":"Claude Code install failed after 3 attempts"}}' > /home/claude/.cloudcode-status.json
      chown claude:claude /home/claude/.cloudcode-status.json
      exit 1
    fi

    # Verify claude is available
    if ! su - claude -c 'which claude'; then
      echo '{{"status":"error","error":"claude binary not found after install"}}' > /home/claude/.cloudcode-status.json
      chown claude:claude /home/claude/.cloudcode-status.json
      exit 1
    fi

    # Install Rust toolchain for claude user
    echo "Installing Rust toolchain..."
    su - claude -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'

    # Set up UFW
    ufw default deny incoming
    ufw default allow outgoing
    ufw allow 22/tcp
    ufw --force enable

    # Write success marker
    echo '{{"status":"ready"}}' > /home/claude/.cloudcode-status.json
    chown claude:claude /home/claude/.cloudcode-status.json
    echo "=== cloudcode setup completed at $(date) ==="
"##
    )
}

pub async fn provision(config: &Config) -> Result<VpsState> {
    let hetzner_config = config
        .hetzner
        .as_ref()
        .context("Hetzner not configured. Run `cloudcode init` first.")?;
    let claude_config = config
        .claude
        .as_ref()
        .context("Claude not configured. Run `cloudcode init` first.")?;

    let client = HetznerClient::new(hetzner_config.api_token.clone());

    // Read SSH public key
    let ssh_pub_key_path = Config::ssh_pub_key_path()?;
    if !ssh_pub_key_path.exists() {
        bail!(
            "SSH public key not found at {}. Run `cloudcode init` first.",
            ssh_pub_key_path.display()
        );
    }
    let ssh_pub_key = fs::read_to_string(&ssh_pub_key_path)
        .context("Failed to read SSH public key")?
        .trim()
        .to_string();

    // Create SSH key in Hetzner
    let ssh_key_id = client
        .create_ssh_key("cloudcode", &ssh_pub_key)
        .await
        .context("Failed to register SSH key with Hetzner")?;

    // Generate cloud-init
    let cloud_init = generate_cloud_init(&ssh_pub_key, claude_config);

    // Get server params from config or defaults
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

    // Create server
    let (server_id, server_ip) = client
        .create_server(
            "cloudcode",
            server_type,
            image,
            location,
            vec![ssh_key_id],
            &cloud_init,
        )
        .await
        .context("Failed to create server")?;

    let state = VpsState {
        server_id: Some(server_id),
        server_ip: Some(server_ip),
        ssh_key_id: Some(ssh_key_id),
        status: Some("initializing".to_string()),
    };

    Ok(state)
}

pub async fn deprovision(state: &VpsState, config: &Config) -> Result<()> {
    let hetzner_config = config
        .hetzner
        .as_ref()
        .context("Hetzner not configured")?;

    let client = HetznerClient::new(hetzner_config.api_token.clone());

    if let Some(server_id) = state.server_id {
        client
            .delete_server(server_id)
            .await
            .context("Failed to delete server")?;
    }

    if let Some(ssh_key_id) = state.ssh_key_id {
        client
            .delete_ssh_key(ssh_key_id)
            .await
            .context("Failed to delete SSH key")?;
    }

    Ok(())
}
