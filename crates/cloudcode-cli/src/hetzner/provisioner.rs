use anyhow::{Context, Result, bail};

use crate::config::Config;
use crate::hetzner::client::HetznerClient;
use crate::state::VpsState;

pub fn generate_cloud_init(ssh_pub_key: &str, config: &Config) -> String {
    // Build write_files entries for provider-specific install scripts
    let mut extra_write_files = String::new();
    // Build extra nohup launches and status file inits
    let mut extra_status_inits = String::new();
    let mut extra_nohup_launches = String::new();

    if config.amp.is_some() {
        extra_write_files.push_str(
            r#"  - path: /opt/cloudcode-amp-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-amp.log 2>&1
      set -euo pipefail

      STATUS_FILE=/home/claude/.cloudcode/amp-status.json

      mkdir -p /home/claude/.cloudcode
      chown -R claude:claude /home/claude/.cloudcode

      echo '{"status":"installing"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"

      for attempt in 1 2 3; do
        echo "Amp install attempt $attempt..."
        if timeout 15m npm install -g @sourcegraph/amp && command -v amp >/dev/null 2>&1; then
          echo '{"status":"ready"}' > "$STATUS_FILE"
          chown claude:claude "$STATUS_FILE"
          chmod 0600 "$STATUS_FILE"
          exit 0
        fi
        echo "Amp attempt $attempt failed, waiting 15s..."
        sleep 15
      done

      echo '{"status":"failed","error":"Amp CLI install failed after 3 attempts. Check /var/log/cloudcode-amp.log"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
"#,
        );
        extra_status_inits.push_str(
            "      echo '{\"status\":\"pending\"}' > /home/claude/.cloudcode/amp-status.json\n\
             \x20     chown claude:claude /home/claude/.cloudcode/amp-status.json\n\
             \x20     chmod 0600 /home/claude/.cloudcode/amp-status.json\n",
        );
        extra_nohup_launches
            .push_str("      nohup /opt/cloudcode-amp-setup.sh >/dev/null 2>&1 &\n");
    }

    if config.opencode.is_some() {
        extra_write_files.push_str(
            r#"  - path: /opt/cloudcode-opencode-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-opencode.log 2>&1
      set -euo pipefail

      STATUS_FILE=/home/claude/.cloudcode/opencode-status.json

      mkdir -p /home/claude/.cloudcode
      chown -R claude:claude /home/claude/.cloudcode

      echo '{"status":"installing"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"

      for attempt in 1 2 3; do
        echo "OpenCode install attempt $attempt..."
        if timeout 15m bash -c 'export HOME=/home/claude && curl -fsSL https://opencode.ai/install | bash' && command -v opencode >/dev/null 2>&1; then
          echo '{"status":"ready"}' > "$STATUS_FILE"
          chown claude:claude "$STATUS_FILE"
          chmod 0600 "$STATUS_FILE"
          exit 0
        fi
        echo "OpenCode attempt $attempt failed, waiting 15s..."
        sleep 15
      done

      echo '{"status":"failed","error":"OpenCode install failed after 3 attempts. Check /var/log/cloudcode-opencode.log"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
"#,
        );
        extra_status_inits.push_str(
            "      echo '{\"status\":\"pending\"}' > /home/claude/.cloudcode/opencode-status.json\n\
             \x20     chown claude:claude /home/claude/.cloudcode/opencode-status.json\n\
             \x20     chmod 0600 /home/claude/.cloudcode/opencode-status.json\n",
        );
        extra_nohup_launches
            .push_str("      nohup /opt/cloudcode-opencode-setup.sh >/dev/null 2>&1 &\n");
    }

    if config.pi.is_some() {
        extra_write_files.push_str(
            r#"  - path: /opt/cloudcode-pi-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-pi.log 2>&1
      set -euo pipefail

      STATUS_FILE=/home/claude/.cloudcode/pi-status.json

      mkdir -p /home/claude/.cloudcode
      chown -R claude:claude /home/claude/.cloudcode

      echo '{"status":"installing"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"

      for attempt in 1 2 3; do
        echo "Pi install attempt $attempt..."
        if timeout 15m npm install -g @mariozechner/pi-coding-agent && command -v pi >/dev/null 2>&1; then
          echo '{"status":"ready"}' > "$STATUS_FILE"
          chown claude:claude "$STATUS_FILE"
          chmod 0600 "$STATUS_FILE"
          exit 0
        fi
        echo "Pi attempt $attempt failed, waiting 15s..."
        sleep 15
      done

      echo '{"status":"failed","error":"Pi CLI install failed after 3 attempts. Check /var/log/cloudcode-pi.log"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
"#,
        );
        extra_status_inits.push_str(
            "      echo '{\"status\":\"pending\"}' > /home/claude/.cloudcode/pi-status.json\n\
             \x20     chown claude:claude /home/claude/.cloudcode/pi-status.json\n\
             \x20     chmod 0600 /home/claude/.cloudcode/pi-status.json\n",
        );
        extra_nohup_launches
            .push_str("      nohup /opt/cloudcode-pi-setup.sh >/dev/null 2>&1 &\n");
    }

    if config.cursor.is_some() {
        extra_write_files.push_str(
            r#"  - path: /opt/cloudcode-cursor-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-cursor.log 2>&1
      set -euo pipefail

      STATUS_FILE=/home/claude/.cloudcode/cursor-status.json

      mkdir -p /home/claude/.cloudcode
      chown -R claude:claude /home/claude/.cloudcode

      echo '{"status":"installing"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"

      for attempt in 1 2 3; do
        echo "Cursor install attempt $attempt..."
        if timeout 15m bash -c 'export HOME=/home/claude && curl https://cursor.com/install -fsSL | bash' && (command -v cursor-agent >/dev/null 2>&1 || test -x /home/claude/.local/bin/cursor-agent); then
          echo '{"status":"ready"}' > "$STATUS_FILE"
          chown claude:claude "$STATUS_FILE"
          chmod 0600 "$STATUS_FILE"
          exit 0
        fi
        echo "Cursor attempt $attempt failed, waiting 15s..."
        sleep 15
      done

      echo '{"status":"failed","error":"Cursor CLI install failed after 3 attempts. Check /var/log/cloudcode-cursor.log"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
"#,
        );
        extra_status_inits.push_str(
            "      echo '{\"status\":\"pending\"}' > /home/claude/.cloudcode/cursor-status.json\n\
             \x20     chown claude:claude /home/claude/.cloudcode/cursor-status.json\n\
             \x20     chmod 0600 /home/claude/.cloudcode/cursor-status.json\n",
        );
        extra_nohup_launches
            .push_str("      nohup /opt/cloudcode-cursor-setup.sh >/dev/null 2>&1 &\n");
    }

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
  - ca-certificates
  - gnupg

runcmd:
  - curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
  - apt-get install -y nodejs

write_files:
  - path: /opt/cloudcode-playwright-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-playwright.log 2>&1
      set -euo pipefail

      STATUS_FILE=/home/claude/.cloudcode/playwright-status.json

      mkdir -p /home/claude/.cloudcode
      chown -R claude:claude /home/claude/.cloudcode

      echo '{{"status":"installing"}}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"

      for attempt in 1 2 3; do
        echo "Playwright install attempt $attempt..."
        if timeout 20m su - claude -c 'export PATH="$HOME/.local/bin:$PATH" && npx playwright install --with-deps chromium'; then
          echo '{{"status":"ready"}}' > "$STATUS_FILE"
          chown claude:claude "$STATUS_FILE"
          chmod 0600 "$STATUS_FILE"
          exit 0
        fi
        echo "Playwright attempt $attempt failed, waiting 15s..."
        sleep 15
      done

      echo '{{"status":"failed","error":"Playwright browser install failed after 3 attempts. Check /var/log/cloudcode-playwright.log"}}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
  - path: /opt/cloudcode-codex-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-codex.log 2>&1
      set -euo pipefail

      STATUS_FILE=/home/claude/.cloudcode/codex-status.json

      mkdir -p /home/claude/.cloudcode
      chown -R claude:claude /home/claude/.cloudcode

      echo '{{"status":"installing"}}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"

      for attempt in 1 2 3; do
        echo "Codex install attempt $attempt..."
        if timeout 15m npm install -g @openai/codex && command -v codex >/dev/null 2>&1; then
          echo '{{"status":"ready"}}' > "$STATUS_FILE"
          chown claude:claude "$STATUS_FILE"
          chmod 0600 "$STATUS_FILE"
          exit 0
        fi
        echo "Codex attempt $attempt failed, waiting 15s..."
        sleep 15
      done

      echo '{{"status":"failed","error":"Codex CLI install failed after 3 attempts. Check /var/log/cloudcode-codex.log"}}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
{extra_write_files}  - path: /opt/cloudcode-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-setup.log 2>&1
      set -euo pipefail

      echo "=== cloudcode setup started at $(date) ==="

      # Install Claude Code via npm (global install as root)
      CLAUDE_INSTALLED=false
      for attempt in 1 2 3; do
        echo "Claude Code install attempt $attempt..."
        if /usr/bin/npm install -g @anthropic-ai/claude-code; then
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

      # Add ~/.local/bin and npm global bin to PATH for claude user
      su - claude -c 'echo '\''export PATH="$HOME/.local/bin:$(npm config get prefix 2>/dev/null)/bin:$PATH"'\'' >> ~/.bashrc'

      # Verify claude is available
      if ! su - claude -c 'export PATH="$HOME/.local/bin:$(npm config get prefix 2>/dev/null)/bin:$PATH" && which claude'; then
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
      mkdir -p /home/claude/.cloudcode
      chown -R claude:claude /home/claude/.cloudcode
      echo '{{"status":"pending"}}' > /home/claude/.cloudcode/playwright-status.json
      chown claude:claude /home/claude/.cloudcode/playwright-status.json
      chmod 0600 /home/claude/.cloudcode/playwright-status.json
      echo '{{"status":"pending"}}' > /home/claude/.cloudcode/codex-status.json
      chown claude:claude /home/claude/.cloudcode/codex-status.json
      chmod 0600 /home/claude/.cloudcode/codex-status.json
{extra_status_inits}
      # Browser automation is non-blocking for first-use readiness. Install it in the background.
      nohup /opt/cloudcode-playwright-setup.sh >/dev/null 2>&1 &

      # Codex CLI install is also non-blocking for first-use readiness.
      nohup /opt/cloudcode-codex-setup.sh >/dev/null 2>&1 &

      # Additional provider installs (non-blocking background)
{extra_nohup_launches}

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
    use crate::config::{AiProviderConfig, AuthMethod, ClaudeConfig};

    fn dummy_config() -> Config {
        Config {
            claude: Some(ClaudeConfig {
                auth_method: AuthMethod::ApiKey,
                api_key: Some("sk-test".to_string()),
                oauth_token: None,
            }),
            ..Config::default()
        }
    }

    #[test]
    fn cloud_init_runcmd_chown_home_before_setup_script() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_config());
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
        let output = generate_cloud_init(key, &dummy_config());
        assert!(output.contains(key));
    }

    #[test]
    fn cloud_init_installs_required_packages() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_config());
        assert!(output.contains("- tmux"));
        assert!(output.contains("- curl"));
        assert!(output.contains("- git"));
        assert!(output.contains("nodesource.com/setup_22.x"));
        assert!(output.contains("apt-get install -y nodejs"));
        assert!(output.contains("/opt/cloudcode-playwright-setup.sh"));
    }

    #[test]
    fn cloud_init_creates_claude_user_with_sudo() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_config());
        assert!(output.contains("name: claude"));
        assert!(output.contains("groups: sudo"));
        assert!(output.contains("NOPASSWD:ALL"));
    }

    #[test]
    fn cloud_init_setup_script_sets_up_ufw() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_config());
        assert!(output.contains("ufw default deny incoming"));
        assert!(output.contains("ufw allow 22/tcp"));
    }

    #[test]
    fn cloud_init_includes_amp_when_configured() {
        let mut config = dummy_config();
        config.amp = Some(AiProviderConfig {
            auth_method: AuthMethod::ApiKey,
            api_key: Some("amp-key".to_string()),
        });
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &config);
        assert!(output.contains("cloudcode-amp-setup.sh"));
        assert!(output.contains("@sourcegraph/amp"));
        assert!(output.contains("amp-status.json"));
    }

    #[test]
    fn cloud_init_includes_opencode_when_configured() {
        let mut config = dummy_config();
        config.opencode = Some(AiProviderConfig {
            auth_method: AuthMethod::ApiKey,
            api_key: Some("oc-key".to_string()),
        });
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &config);
        assert!(output.contains("cloudcode-opencode-setup.sh"));
        assert!(output.contains("opencode.ai/install"));
        assert!(output.contains("opencode-status.json"));
    }

    #[test]
    fn cloud_init_includes_pi_when_configured() {
        let mut config = dummy_config();
        config.pi = Some(AiProviderConfig {
            auth_method: AuthMethod::ApiKey,
            api_key: Some("pi-key".to_string()),
        });
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &config);
        assert!(output.contains("cloudcode-pi-setup.sh"));
        assert!(output.contains("@mariozechner/pi-coding-agent"));
        assert!(output.contains("pi-status.json"));
    }

    #[test]
    fn cloud_init_includes_cursor_when_configured() {
        let mut config = dummy_config();
        config.cursor = Some(AiProviderConfig {
            auth_method: AuthMethod::ApiKey,
            api_key: Some("cursor-key".to_string()),
        });
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &config);
        assert!(output.contains("cloudcode-cursor-setup.sh"));
        assert!(output.contains("cursor.com/install"));
        assert!(output.contains("cursor-status.json"));
    }

    #[test]
    fn cloud_init_excludes_new_providers_when_not_configured() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_config());
        assert!(!output.contains("cloudcode-amp-setup.sh"));
        assert!(!output.contains("cloudcode-opencode-setup.sh"));
        assert!(!output.contains("cloudcode-pi-setup.sh"));
        assert!(!output.contains("cloudcode-cursor-setup.sh"));
    }
}

pub async fn deprovision(state: &VpsState, config: &Config) -> Result<()> {
    let hetzner_config = config.hetzner.as_ref().context("Hetzner not configured")?;

    let client = HetznerClient::new(hetzner_config.api_token.clone());
    let mut errors = Vec::new();

    if let Some(ref server_id_str) = state.server_id {
        match server_id_str.parse::<u64>() {
            Ok(server_id) => {
                if let Err(e) = client.delete_server(server_id).await {
                    errors.push(format!("Failed to delete server {}: {}", server_id, e));
                }
            }
            Err(_) => {
                errors.push(format!(
                    "Invalid server_id '{}' (expected numeric for Hetzner)",
                    server_id_str
                ));
            }
        }
    }

    if let Some(ref ssh_key_id_str) = state.ssh_key_id {
        match ssh_key_id_str.parse::<u64>() {
            Ok(ssh_key_id) => {
                if let Err(e) = client.delete_ssh_key(ssh_key_id).await {
                    errors.push(format!("Failed to delete SSH key {}: {}", ssh_key_id, e));
                }
            }
            Err(_) => {
                errors.push(format!(
                    "Invalid ssh_key_id '{}' (expected numeric for Hetzner)",
                    ssh_key_id_str
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        bail!("{}", errors.join("; "))
    }
}
