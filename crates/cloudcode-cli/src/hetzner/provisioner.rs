use anyhow::{Context, Result, bail};

use crate::config::Config;
use crate::hetzner::client::HetznerClient;
use crate::state::VpsState;

pub fn generate_cloud_init(ssh_pub_key: &str, config: &Config) -> String {
    // Build the list of npm packages to batch-install in a single background call.
    // Claude Code is installed separately (synchronous, critical path).
    let mut npm_packages = Vec::new();
    let mut npm_status_names = Vec::new(); // provider names that get status files from npm install

    // Codex is always installed as a background provider
    npm_packages.push("@openai/codex".to_string());
    npm_status_names.push("codex");

    if config.amp.is_some() {
        npm_packages.push("@sourcegraph/amp".to_string());
        npm_status_names.push("amp");
    }
    if config.pi.is_some() {
        npm_packages.push("@mariozechner/pi-coding-agent".to_string());
        npm_status_names.push("pi");
    }

    let npm_packages_str = npm_packages.join(" ");

    // Build curl-based installer background jobs (these can't be batched with npm)
    let mut curl_write_files = String::new();
    let mut curl_status_inits = String::new();
    let mut curl_nohup_launches = String::new();

    if config.opencode.is_some() {
        curl_write_files.push_str(
            r#"  - path: /opt/cloudcode-opencode-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-opencode.log 2>&1
      set -euo pipefail
      STATUS_FILE=/home/claude/.cloudcode/opencode-status.json
      echo '{"status":"installing"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
      for attempt in 1 2; do
        echo "OpenCode install attempt $attempt..."
        if timeout 10m bash -c 'export HOME=/home/claude && curl -fsSL https://opencode.ai/install | bash'; then
          # Fix ownership — curl installer creates dirs as root
          chown -R claude:claude /home/claude/.local /home/claude/.opencode 2>/dev/null || true
          # Symlink into PATH
          ln -sf /home/claude/.opencode/bin/opencode /home/claude/.local/bin/opencode 2>/dev/null || true
          if command -v opencode >/dev/null 2>&1 || test -x /home/claude/.local/bin/opencode; then
            echo '{"status":"ready"}' > "$STATUS_FILE"
            chown claude:claude "$STATUS_FILE"
            chmod 0600 "$STATUS_FILE"
            exit 0
          fi
        fi
        echo "OpenCode attempt $attempt failed, waiting 5s..."
        sleep 5
      done
      echo '{"status":"failed","error":"OpenCode install failed after 2 attempts. Check /var/log/cloudcode-opencode.log"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
"#,
        );
        curl_status_inits.push_str(
            "      echo '{\"status\":\"pending\"}' > /home/claude/.cloudcode/opencode-status.json\n      chown claude:claude /home/claude/.cloudcode/opencode-status.json\n      chmod 0600 /home/claude/.cloudcode/opencode-status.json\n",
        );
        curl_nohup_launches
            .push_str("      nohup /opt/cloudcode-opencode-setup.sh >/dev/null 2>&1 &\n");
    }

    if config.cursor.is_some() {
        curl_write_files.push_str(
            r#"  - path: /opt/cloudcode-cursor-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-cursor.log 2>&1
      set -euo pipefail
      STATUS_FILE=/home/claude/.cloudcode/cursor-status.json
      echo '{"status":"installing"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
      for attempt in 1 2; do
        echo "Cursor install attempt $attempt..."
        if timeout 10m bash -c 'export HOME=/home/claude && curl https://cursor.com/install -fsSL | bash'; then
          # Fix ownership — curl installer creates dirs as root
          chown -R claude:claude /home/claude/.local /home/claude/.cursor 2>/dev/null || true
          if command -v cursor-agent >/dev/null 2>&1 || test -x /home/claude/.local/bin/cursor-agent; then
            echo '{"status":"ready"}' > "$STATUS_FILE"
            chown claude:claude "$STATUS_FILE"
            chmod 0600 "$STATUS_FILE"
            exit 0
          fi
        fi
        echo "Cursor attempt $attempt failed, waiting 5s..."
        sleep 5
      done
      echo '{"status":"failed","error":"Cursor CLI install failed after 2 attempts. Check /var/log/cloudcode-cursor.log"}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
"#,
        );
        curl_status_inits.push_str(
            "      echo '{\"status\":\"pending\"}' > /home/claude/.cloudcode/cursor-status.json\n      chown claude:claude /home/claude/.cloudcode/cursor-status.json\n      chmod 0600 /home/claude/.cloudcode/cursor-status.json\n",
        );
        curl_nohup_launches
            .push_str("      nohup /opt/cloudcode-cursor-setup.sh >/dev/null 2>&1 &\n");
    }

    // Build status init lines for npm-installed providers
    let mut npm_status_inits = String::new();
    for name in &npm_status_names {
        npm_status_inits.push_str(&format!(
            "      echo '{{\"status\":\"pending\"}}' > /home/claude/.cloudcode/{name}-status.json\n      chown claude:claude /home/claude/.cloudcode/{name}-status.json\n      chmod 0600 /home/claude/.cloudcode/{name}-status.json\n"
        ));
    }

    // Build the npm status update lines (mark each provider ready after single npm install succeeds)
    let mut npm_status_ready_lines = String::new();
    for name in &npm_status_names {
        npm_status_ready_lines.push_str(&format!(
            "          echo '{{\"status\":\"ready\"}}' > /home/claude/.cloudcode/{name}-status.json\n          chown claude:claude /home/claude/.cloudcode/{name}-status.json\n          chmod 0600 /home/claude/.cloudcode/{name}-status.json\n"
        ));
    }

    let mut npm_status_failed_lines = String::new();
    for name in &npm_status_names {
        npm_status_failed_lines.push_str(&format!(
            "      echo '{{\"status\":\"failed\",\"error\":\"{name} CLI install failed (npm batch install failed after 2 attempts). Check /var/log/cloudcode-npm-providers.log\"}}' > /home/claude/.cloudcode/{name}-status.json\n      chown claude:claude /home/claude/.cloudcode/{name}-status.json\n      chmod 0600 /home/claude/.cloudcode/{name}-status.json\n"
        ));
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

write_files:
  - path: /home/claude/.tmux.conf
    permissions: '0644'
    content: |
      set -g mouse on
      set -g set-clipboard on
  - path: /opt/cloudcode-playwright-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-playwright.log 2>&1
      set -euo pipefail
      STATUS_FILE=/home/claude/.cloudcode/playwright-status.json
      echo '{{"status":"installing"}}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
      for attempt in 1 2; do
        echo "Playwright install attempt $attempt..."
        if timeout 20m su - claude -c 'export PATH="$HOME/.local/bin:$PATH" && npx playwright install --with-deps chromium'; then
          echo '{{"status":"ready"}}' > "$STATUS_FILE"
          chown claude:claude "$STATUS_FILE"
          chmod 0600 "$STATUS_FILE"
          exit 0
        fi
        echo "Playwright attempt $attempt failed, waiting 5s..."
        sleep 5
      done
      echo '{{"status":"failed","error":"Playwright browser install failed after 2 attempts. Check /var/log/cloudcode-playwright.log"}}' > "$STATUS_FILE"
      chown claude:claude "$STATUS_FILE"
      chmod 0600 "$STATUS_FILE"
  - path: /opt/cloudcode-npm-providers-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-npm-providers.log 2>&1
      set -euo pipefail
      # Mark all npm providers as installing
{npm_status_inits}
      for name in {npm_status_names_space}; do
        echo '{{"status":"installing"}}' > "/home/claude/.cloudcode/${{name}}-status.json"
        chown claude:claude "/home/claude/.cloudcode/${{name}}-status.json"
        chmod 0600 "/home/claude/.cloudcode/${{name}}-status.json"
      done

      for attempt in 1 2; do
        echo "Batch npm provider install attempt $attempt..."
        if timeout 15m npm install -g {npm_packages_str}; then
          echo "Batch npm install succeeded on attempt $attempt"
{npm_status_ready_lines}
          exit 0
        fi
        echo "Batch npm install attempt $attempt failed, waiting 5s..."
        sleep 5
      done

      echo "Batch npm install failed after 2 attempts"
{npm_status_failed_lines}
{curl_write_files}  - path: /opt/cloudcode-setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      exec > /var/log/cloudcode-setup.log 2>&1
      set -euo pipefail

      echo "=== cloudcode setup started at $(date) ==="

      # Fix ownership of files written before user creation
      chown claude:claude /home/claude/.tmux.conf 2>/dev/null || true

      # Install Node.js 22 via NodeSource
      curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
      apt-get install -y nodejs

      # Install Claude Code via npm (global install as root)
      CLAUDE_INSTALLED=false
      for attempt in 1 2; do
        echo "Claude Code install attempt $attempt..."
        if /usr/bin/npm install -g @anthropic-ai/claude-code; then
          CLAUDE_INSTALLED=true
          break
        fi
        echo "Attempt $attempt failed, waiting 5s..."
        sleep 5
      done

      if [ "$CLAUDE_INSTALLED" = false ]; then
        echo '{{"status":"error","error":"Claude Code install failed after 2 attempts"}}' > /home/claude/.cloudcode-status.json
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
{curl_status_inits}
      # Background installs (all run in parallel)
      nohup /opt/cloudcode-playwright-setup.sh >/dev/null 2>&1 &
      nohup /opt/cloudcode-npm-providers-setup.sh >/dev/null 2>&1 &
{curl_nohup_launches}
      echo "=== cloudcode setup completed at $(date) ==="

      # Clean up cloud-init data (may contain sensitive info)
      rm -f /var/lib/cloud/instance/user-data.txt
      rm -rf /var/lib/cloud/instance/scripts
      rm -f /opt/cloudcode-setup.sh

runcmd:
  - chown -R claude:claude /home/claude
  - /opt/cloudcode-setup.sh
"##,
        npm_status_names_space = npm_status_names.join(" "),
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
        // Amp is npm-based, so it should be in the batched npm install
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
        // OpenCode uses curl installer, so it gets its own script
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
        // Pi is npm-based, so it should be in the batched npm install
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
        // Cursor uses curl installer, so it gets its own script
        assert!(output.contains("cloudcode-cursor-setup.sh"));
        assert!(output.contains("cursor.com/install"));
        assert!(output.contains("cursor-status.json"));
    }

    #[test]
    fn cloud_init_excludes_optional_providers_when_not_configured() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_config());
        // Curl-based providers should not have scripts
        assert!(!output.contains("cloudcode-opencode-setup.sh"));
        assert!(!output.contains("cloudcode-cursor-setup.sh"));
        // npm-based optional providers should not be in the package list
        assert!(!output.contains("@sourcegraph/amp"));
        assert!(!output.contains("@mariozechner/pi-coding-agent"));
    }

    #[test]
    fn cloud_init_batches_npm_packages() {
        let mut config = dummy_config();
        config.amp = Some(AiProviderConfig {
            auth_method: AuthMethod::ApiKey,
            api_key: Some("amp-key".to_string()),
        });
        config.pi = Some(AiProviderConfig {
            auth_method: AuthMethod::ApiKey,
            api_key: Some("pi-key".to_string()),
        });
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &config);
        // All npm packages should be in a single npm install command
        assert!(output.contains("npm install -g @openai/codex @sourcegraph/amp @mariozechner/pi-coding-agent"));
        // Should use the batched setup script, not individual ones
        assert!(output.contains("cloudcode-npm-providers-setup.sh"));
    }

    #[test]
    fn cloud_init_has_single_runcmd_section() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_config());
        // There should be exactly one runcmd: key in the YAML
        let count = output.matches("\nruncmd:").count();
        assert_eq!(count, 1, "Should have exactly one runcmd section, found {}", count);
    }

    #[test]
    fn cloud_init_node_install_in_setup_script() {
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &dummy_config());
        // NodeSource install should be inside the setup script, not in runcmd
        let setup_script_start = output.find("cloudcode-setup.sh").unwrap();
        let nodesource_pos = output.find("nodesource.com/setup_22.x").unwrap();
        assert!(nodesource_pos > setup_script_start,
            "NodeSource install should be inside the setup script");
    }

    #[test]
    fn cloud_init_yaml_indentation_is_consistent() {
        // Regression test: inside `content: |` blocks, shell command lines
        // (echo, chown, chmod, etc.) must maintain >= the block's base indent.
        // Lines that drop below cause YAML to end the block prematurely.
        let mut config = dummy_config();
        config.amp = Some(AiProviderConfig { auth_method: AuthMethod::ApiKey, api_key: Some("k".into()) });
        config.opencode = Some(AiProviderConfig { auth_method: AuthMethod::ApiKey, api_key: Some("k".into()) });
        config.pi = Some(AiProviderConfig { auth_method: AuthMethod::ApiKey, api_key: Some("k".into()) });
        config.cursor = Some(AiProviderConfig { auth_method: AuthMethod::ApiKey, api_key: Some("k".into()) });
        let output = generate_cloud_init("ssh-ed25519 AAAA test@test", &config);

        let mut in_content_block = false;
        let mut block_indent: Option<usize> = None;
        let mut content_line_num = 0usize;
        for (line_num, line) in output.lines().enumerate() {
            if line.trim_end().ends_with("content: |") {
                in_content_block = true;
                block_indent = None;
                content_line_num = line_num;
                continue;
            }
            if in_content_block {
                if line.trim().is_empty() {
                    continue;
                }
                let indent = line.len() - line.trim_start().len();
                if let Some(base) = block_indent {
                    if indent < base {
                        // Block ended — this line is back at YAML level
                        in_content_block = false;
                        // But verify the line IS a valid YAML construct, not a
                        // shell command that accidentally broke out
                        let trimmed = line.trim();
                        let is_shell_cmd = trimmed.starts_with("echo ")
                            || trimmed.starts_with("chown ")
                            || trimmed.starts_with("chmod ")
                            || trimmed.starts_with("for ")
                            || trimmed.starts_with("if ")
                            || trimmed.starts_with("fi")
                            || trimmed.starts_with("done")
                            || trimmed.starts_with("exit ")
                            || trimmed.starts_with("sleep ")
                            || trimmed.starts_with("nohup ");
                        if is_shell_cmd {
                            panic!(
                                "Cloud-init YAML: shell command escaped content: | block!\n\
                                 Line {}: {:?} (indent {}, block base {})\n\
                                 Block started at line {}",
                                line_num + 1, line, indent, base, content_line_num + 1
                            );
                        }
                    }
                } else {
                    block_indent = Some(indent);
                }
            }
        }
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
