use anyhow::{Context, Result, bail};

use crate::config::{ClaudeConfig, Config};
use crate::hetzner::client::HetznerClient;
use crate::state::VpsState;

pub fn generate_cloud_init(ssh_pub_key: &str, _claude_auth: &ClaudeConfig) -> String {
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
  - git

write_files:
  - path: /home/claude/.claude/settings.json
    permissions: '0600'
    content: |
      {{"permissions":{{"allow":[],"deny":[]}},"hasCompletedOnboarding":true,"skipDangerousModePermissionPrompt":true}}
  - path: /opt/cloudcode-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-setup.log 2>&1
      set -euo pipefail

      echo "=== cloudcode setup started at $(date) ==="

      # Install Claude Code with retries
      CLAUDE_INSTALLED=false
      for attempt in 1 2 3; do
        echo "Claude Code install attempt $attempt..."
        if su - claude -c 'curl -fsSL https://claude.ai/install.sh | bash'; then
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

      # Add ~/.local/bin to PATH for claude user (where Claude Code installs)
      su - claude -c 'echo '\''export PATH="$HOME/.local/bin:$PATH"'\'' >> ~/.bashrc'

      # Verify claude is available
      if ! su - claude -c 'export PATH="$HOME/.local/bin:$PATH" && which claude'; then
        echo '{{"status":"error","error":"claude binary not found after install"}}' > /home/claude/.cloudcode-status.json
        chown claude:claude /home/claude/.cloudcode-status.json
        exit 1
      fi

      # Set up UFW
      ufw default deny incoming
      ufw default allow outgoing
      ufw allow 22/tcp
      ufw --force enable

      # Write success marker
      echo '{{"status":"ready"}}' > /home/claude/.cloudcode-status.json
      chown claude:claude /home/claude/.cloudcode-status.json

      echo "=== cloudcode setup completed at $(date) ==="

      # Clean up cloud-init data (may contain sensitive info)
      rm -f /var/lib/cloud/instance/user-data.txt
      rm -rf /var/lib/cloud/instance/scripts
      rm -f /opt/cloudcode-setup.sh

runcmd:
  - chown -R claude:claude /home/claude
  - /opt/cloudcode-setup.sh
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMethod, ClaudeConfig};

    fn dummy_claude_config() -> ClaudeConfig {
        ClaudeConfig {
            auth_method: AuthMethod::ApiKey,
            api_key: Some("sk-test".to_string()),
            oauth_token: None,
        }
    }

    #[test]
    fn cloud_init_contains_settings_json_write_file() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_claude_config());
        assert!(output.contains("/home/claude/.claude/settings.json"));
        assert!(output.contains("hasCompletedOnboarding"));
        assert!(output.contains("skipDangerousModePermissionPrompt"));
    }

    #[test]
    fn cloud_init_settings_json_before_setup_script() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_claude_config());
        let settings_pos = output.find("settings.json").unwrap();
        let setup_pos = output.find("cloudcode-setup.sh").unwrap();
        assert!(
            settings_pos < setup_pos,
            "settings.json write_files entry should come before setup script"
        );
    }

    #[test]
    fn cloud_init_runcmd_chown_home_before_setup_script() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_claude_config());
        // Find the runcmd section specifically
        let runcmd_section = output.split("runcmd:").last().unwrap();
        let chown_pos = runcmd_section
            .find("chown -R claude:claude /home/claude")
            .expect("chown home dir should be in runcmd section");
        let setup_pos = runcmd_section
            .find("/opt/cloudcode-setup.sh")
            .expect("setup script should be in runcmd section");
        assert!(
            chown_pos < setup_pos,
            "chown home dir runcmd should come before setup script runcmd"
        );
    }

    #[test]
    fn cloud_init_contains_ssh_pub_key() {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 user@host";
        let output = generate_cloud_init(key, &dummy_claude_config());
        assert!(output.contains(key));
    }

    #[test]
    fn cloud_init_installs_required_packages() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_claude_config());
        assert!(output.contains("- tmux"));
        assert!(output.contains("- curl"));
        assert!(output.contains("- git"));
    }

    #[test]
    fn cloud_init_settings_json_is_valid_json_after_format_expansion() {
        // In the cloud-init template, {{ becomes { after Rust format!()
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_claude_config());
        // Extract the settings.json content line
        let lines: Vec<&str> = output.lines().collect();
        let settings_line = lines
            .iter()
            .find(|l| l.contains("hasCompletedOnboarding"))
            .expect("settings.json content not found");
        let trimmed = settings_line.trim();
        // Verify it parses as valid JSON
        let parsed: serde_json::Value = serde_json::from_str(trimmed).unwrap_or_else(|e| {
            panic!("settings.json content is not valid JSON: {e}\nContent: {trimmed}")
        });
        assert_eq!(parsed["hasCompletedOnboarding"], true);
        assert_eq!(parsed["skipDangerousModePermissionPrompt"], true);
        assert!(parsed["permissions"]["allow"].is_array());
        assert!(parsed["permissions"]["deny"].is_array());
    }

    #[test]
    fn cloud_init_creates_claude_user_with_sudo() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_claude_config());
        assert!(output.contains("name: claude"));
        assert!(output.contains("groups: sudo"));
        assert!(output.contains("NOPASSWD:ALL"));
    }

    #[test]
    fn cloud_init_setup_script_sets_up_ufw() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_claude_config());
        assert!(output.contains("ufw default deny incoming"));
        assert!(output.contains("ufw allow 22/tcp"));
    }
}

pub async fn deprovision(state: &VpsState, config: &Config) -> Result<()> {
    let hetzner_config = config.hetzner.as_ref().context("Hetzner not configured")?;

    let client = HetznerClient::new(hetzner_config.api_token.clone());
    let mut errors = Vec::new();

    if let Some(server_id) = state.server_id {
        if let Err(e) = client.delete_server(server_id).await {
            errors.push(format!("Failed to delete server {}: {}", server_id, e));
        }
    }

    if let Some(ssh_key_id) = state.ssh_key_id {
        if let Err(e) = client.delete_ssh_key(ssh_key_id).await {
            errors.push(format!("Failed to delete SSH key {}: {}", ssh_key_id, e));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        bail!("{}", errors.join("; "))
    }
}
