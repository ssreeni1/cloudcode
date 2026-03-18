use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::AuthMethod;
use crate::ssh::ssh_base_args;
use crate::state::VpsState;

pub mod provision;

// Embedded daemon binaries (populated by build.rs when pre-built binaries are available)
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_daemons.rs"));
}

fn cloudcode_cache_dir() -> Result<PathBuf> {
    let dir = crate::paths::embedded_daemon_cache_dir()?;
    std::fs::create_dir_all(&dir).context("Failed to create embedded daemon cache")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

fn ssh_command_args(ip: &str, command: &str) -> Result<Vec<String>> {
    let mut args = ssh_base_args(ip)?;
    args.extend([format!("claude@{}", ip), command.to_string()]);
    Ok(args)
}

fn cached_file_path(prefix: &str, suffix: &str, checksum: &str) -> Result<PathBuf> {
    let cache_dir = cloudcode_cache_dir()?;
    Ok(cache_dir.join(format!("{prefix}-{checksum}{suffix}")))
}

/// Map a Hetzner server type name to a Rust target triple.
/// CAX = ARM64, CX/CPX = x86_64.
pub fn target_triple_for_server_type(server_type: &str) -> &'static str {
    if server_type.starts_with("cax") {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

/// Get the daemon binary for a target.
/// Tries in order: embedded binary → remote build on VPS.
/// If both fail, shows hint about installing cargo-zigbuild for faster future builds.
pub fn get_daemon_binary(target: &str) -> Result<PathBuf> {
    // 1. Try embedded binary (release builds have these baked in)
    if let Some(path) = extract_embedded_daemon(target)? {
        return Ok(path);
    }

    // 2. Fall back to remote build on the VPS
    eprintln!("  No embedded daemon binary (dev build). Building on VPS instead (3-5 min)...");
    match remote_build_daemon(target) {
        Ok(path) => return Ok(path),
        Err(e) => {
            eprintln!("  Remote build failed: {}", e);
        }
    }

    // 3. Last resort: try local cross-compilation
    eprintln!("  Trying local cross-compilation...");
    match cross_compile_daemon(target) {
        Ok(path) => return Ok(path),
        Err(_) => {}
    }

    anyhow::bail!(
        "Could not prepare daemon binary.\n\
         \n  To speed up future runs, install local cross-compilation tools:\n\
         \n      brew install zig\n\
         \n      cargo install cargo-zigbuild\n\
         \n      rustup target add {target}\n\
         \n  Then run /up again."
    )
}

/// Build the daemon on the VPS by uploading source and compiling remotely.
/// This is the slowest path but requires no local cross-compilation tools.
fn remote_build_daemon(target: &str) -> Result<PathBuf> {
    // We need the VPS state to SSH into it
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned — cannot do remote build");
    }
    let ip = state.server_ip.as_ref().context("No server IP")?;

    // Upload packaged source
    upload_source_bundle(&state)?;

    // Install Rust on VPS if not present
    let check = std::process::Command::new("ssh")
        .args(ssh_command_args(
            ip,
            "source $HOME/.cargo/env 2>/dev/null; which cargo",
        )?)
        .output()
        .context("Failed to check for Rust on VPS")?;

    if !check.status.success() {
        eprintln!("  {} Installing Rust toolchain on VPS...", "→".to_string());
        let install = std::process::Command::new("ssh")
            .args(ssh_command_args(
                ip,
                "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y",
            )?)
            .status()
            .context("Failed to install Rust on VPS")?;
        if !install.success() {
            anyhow::bail!("Failed to install Rust toolchain on VPS");
        }
    }

    // Ensure a native linker/toolchain exists for cargo builds.
    let build_tools = std::process::Command::new("ssh")
        .args(ssh_command_args(ip, "which cc >/dev/null 2>&1")?)
        .status()
        .context("Failed to check for native build tools on VPS")?;
    if !build_tools.success() {
        eprintln!(
            "  {} Installing native build tools on VPS...",
            "→".to_string()
        );
        let install = std::process::Command::new("ssh")
            .args(ssh_command_args(
                ip,
                "sudo apt-get update && sudo apt-get install -y build-essential pkg-config",
            )?)
            .status()
            .context("Failed to install native build tools on VPS")?;
        if !install.success() {
            anyhow::bail!("Failed to install native build tools on VPS");
        }
    }

    // Build on VPS
    let output = std::process::Command::new("ssh")
        .args(ssh_command_args(
            ip,
            "source $HOME/.cargo/env && cd /home/claude/cloudcode-src && cargo build --release -p cloudcode-daemon 2>&1",
        )?)
        .output()
        .context("Failed to build daemon on VPS")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!("Remote daemon build failed:\n{}\n{}", stdout, stderr);
    }

    // Download the binary from VPS to local temp
    let tmp_path = cached_file_path("cloudcode-daemon-remote", "", target)?;
    let mut scp_args = ssh_base_args(ip)?;
    scp_args.extend([
        format!("claude@{ip}:/home/claude/cloudcode-src/target/release/cloudcode-daemon"),
        tmp_path.to_string_lossy().to_string(),
    ]);
    let status = std::process::Command::new("scp")
        .args(&scp_args)
        .status()
        .context("Failed to download built binary from VPS")?;

    if !status.success() {
        anyhow::bail!("Failed to download daemon binary from VPS");
    }

    Ok(tmp_path)
}

fn source_bundle_path() -> Result<PathBuf> {
    let final_path = cached_file_path(
        "cloudcode-source",
        ".tar.gz",
        embedded::SOURCE_BUNDLE_SHA256,
    )?;
    if final_path.exists() {
        return Ok(final_path);
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = final_path.with_extension(format!("{nonce}.tmp"));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .context("Failed to create source bundle temp file")?;
    drop(file);
    std::fs::write(&tmp_path, embedded::SOURCE_BUNDLE)
        .context("Failed to write embedded source bundle")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }

    std::fs::rename(&tmp_path, &final_path)
        .or_else(|err| {
            if final_path.exists() {
                let _ = std::fs::remove_file(&tmp_path);
                Ok(())
            } else {
                Err(err)
            }
        })
        .context("Failed to finalize source bundle")?;

    Ok(final_path)
}

/// Upload packaged source to VPS for the remote build fallback.
fn upload_source_bundle(state: &VpsState) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP")?;
    let source_bundle = source_bundle_path()?;

    // Create target directory
    let mkdir_status = std::process::Command::new("ssh")
        .args(ssh_command_args(
            ip,
            "rm -rf /home/claude/cloudcode-src && mkdir -p /home/claude",
        )?)
        .status()
        .context("Failed to create remote directory")?;

    if !mkdir_status.success() {
        anyhow::bail!("Failed to create remote directory");
    }

    let mut scp_args = ssh_base_args(ip)?;
    scp_args.extend([
        source_bundle.to_string_lossy().to_string(),
        format!("claude@{ip}:/tmp/cloudcode-source.tar.gz"),
    ]);
    let status = std::process::Command::new("scp")
        .args(&scp_args)
        .status()
        .context("Failed to upload source bundle")?;

    if !status.success() {
        anyhow::bail!("scp failed while uploading source bundle");
    }

    let extract_status = std::process::Command::new("ssh")
        .args(ssh_command_args(
            ip,
            "tar -xzf /tmp/cloudcode-source.tar.gz -C /home/claude && rm -f /tmp/cloudcode-source.tar.gz",
        )?)
        .status()
        .context("Failed to unpack source bundle on VPS")?;

    if !extract_status.success() {
        anyhow::bail!("Failed to unpack source bundle on VPS");
    }

    Ok(())
}

/// Extract an embedded daemon binary to a temp file, if one was baked in at compile time.
fn extract_embedded_daemon(target: &str) -> Result<Option<PathBuf>> {
    let (bytes, checksum) = match target {
        "x86_64-unknown-linux-gnu" => (embedded::DAEMON_X86_64, embedded::DAEMON_X86_64_SHA256),
        "aarch64-unknown-linux-gnu" => (embedded::DAEMON_AARCH64, embedded::DAEMON_AARCH64_SHA256),
        _ => (None, None),
    };

    let (bytes, checksum) = match (bytes, checksum) {
        (Some(bytes), Some(checksum)) => (bytes, checksum),
        (None, None) => return Ok(None),
        _ => anyhow::bail!("Embedded daemon metadata is incomplete for {target}"),
    };

    let cache_dir = cloudcode_cache_dir()?;
    let final_path = cache_dir.join(format!("cloudcode-daemon-{target}-{checksum}"));
    if final_path.exists() {
        return Ok(Some(final_path));
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = cache_dir.join(format!("cloudcode-daemon-{target}-{checksum}.{nonce}.tmp"));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .context("Failed to create embedded daemon temp file")?;
    drop(file);
    std::fs::write(&tmp_path, bytes).context("Failed to write embedded daemon binary")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    std::fs::rename(&tmp_path, &final_path)
        .or_else(|err| {
            if final_path.exists() {
                let _ = std::fs::remove_file(&tmp_path);
                Ok(())
            } else {
                Err(err)
            }
        })
        .context("Failed to finalize embedded daemon binary")?;

    Ok(Some(final_path))
}

/// Cross-compile the daemon binary locally using cargo-zigbuild.
/// Returns the path to the compiled binary.
pub fn cross_compile_daemon(target: &str) -> Result<PathBuf> {
    // Check that cargo-zigbuild is available
    let zigbuild_check = std::process::Command::new("cargo")
        .args(["zigbuild", "--version"])
        .output();
    match zigbuild_check {
        Ok(out) if out.status.success() => {}
        _ => {
            anyhow::bail!(
                "cargo-zigbuild is not installed. Install it with:\n  \
                 brew install zig && cargo install cargo-zigbuild\n  \
                 rustup target add {target}"
            );
        }
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .context("Could not determine workspace root")?;

    let output = std::process::Command::new("cargo")
        .args([
            "zigbuild",
            "--release",
            "-p",
            "cloudcode-daemon",
            "--target",
            target,
        ])
        .current_dir(workspace_root)
        .output()
        .context("Failed to run cargo zigbuild")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!("Cross-compilation failed:\n{}\n{}", stdout, stderr);
    }

    let binary = workspace_root
        .join("target")
        .join(target)
        .join("release")
        .join("cloudcode-daemon");
    if !binary.exists() {
        anyhow::bail!("Binary not found at {}", binary.display());
    }
    Ok(binary)
}

/// Upload the pre-compiled daemon binary to the VPS via scp.
pub fn upload_binary(state: &VpsState, local_binary: &Path) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP")?;
    let mut scp_args = ssh_base_args(ip)?;

    // scp binary to /tmp on the VPS
    scp_args.extend([
        local_binary.to_string_lossy().to_string(),
        format!("claude@{}:/tmp/cloudcode-daemon", ip),
    ]);
    let status = std::process::Command::new("scp")
        .args(&scp_args)
        .status()
        .context("Failed to scp binary")?;

    if !status.success() {
        anyhow::bail!("scp failed");
    }

    // Move into place
    let output = std::process::Command::new("ssh")
        .args(ssh_command_args(
            ip,
            "sudo mv /tmp/cloudcode-daemon /usr/local/bin/cloudcode-daemon && sudo chmod 755 /usr/local/bin/cloudcode-daemon",
        )?)
        .output()
        .context("Failed to install binary on VPS")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to install binary: {}", stderr);
    }
    Ok(())
}

/// Install daemon config, env file, and systemd unit (binary already in place).
pub fn install_daemon(state: &VpsState, config: &crate::config::Config) -> Result<()> {
    let ip = state.server_ip.as_ref().context("No server IP")?;

    // Generate daemon config TOML
    let mut daemon_toml = String::from("listen_addr = \"127.0.0.1\"\nlisten_port = 7700\n");
    if let Some(ref tg) = config.telegram {
        daemon_toml.push_str(&format!(
            "\n[telegram]\nbot_token = \"{}\"\nowner_id = {}\n",
            tg.bot_token, tg.owner_id
        ));
    }

    // Build the secrets env file content
    let mut env_file_content = String::new();
    if let Some(ref claude) = config.claude {
        if matches!(claude.auth_method, AuthMethod::ApiKey) {
            if let Some(ref key) = claude.api_key {
                env_file_content.push_str(&format!("ANTHROPIC_API_KEY={}\n", key));
            }
        }
    }

    // Generate systemd unit
    let unit = r#"[Unit]
Description=cloudcode daemon
After=network.target

[Service]
Type=simple
User=claude
ExecStart=/usr/local/bin/cloudcode-daemon /etc/cloudcode/daemon.toml
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info
EnvironmentFile=/home/claude/.cloudcode-env
WorkingDirectory=/home/claude
ProtectSystem=strict
PrivateTmp=false
NoNewPrivileges=true
ReadWritePaths=/home/claude /tmp

[Install]
WantedBy=multi-user.target
"#;

    let install_script = format!(
        r#"set -e
sudo mkdir -p /etc/cloudcode
cat << 'DAEMON_TOML' | sudo tee /etc/cloudcode/daemon.toml > /dev/null
{daemon_toml}
DAEMON_TOML
sudo chown claude:claude /etc/cloudcode/daemon.toml
sudo chmod 0600 /etc/cloudcode/daemon.toml
cat << 'ENV_FILE' > /home/claude/.cloudcode-env
{env_file_content}
ENV_FILE
chown claude:claude /home/claude/.cloudcode-env
chmod 0600 /home/claude/.cloudcode-env
mkdir -p /home/claude/.claude
cat << 'SETTINGS_JSON' > /home/claude/.claude/settings.json
{{"permissions":{{"allow":[],"deny":[]}},"hasCompletedOnboarding":true,"skipDangerousModePermissionPrompt":true}}
SETTINGS_JSON
chown -R claude:claude /home/claude/.claude
cat << 'UNIT_FILE' | sudo tee /etc/systemd/system/cloudcode-daemon.service > /dev/null
{unit}
UNIT_FILE
sudo systemctl daemon-reload
sudo systemctl enable cloudcode-daemon
sudo systemctl restart cloudcode-daemon
"#
    );

    let output = std::process::Command::new("ssh")
        .args(ssh_command_args(ip, &install_script)?)
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

    std::thread::sleep(std::time::Duration::from_secs(2));

    let output = std::process::Command::new("ssh")
        .args(ssh_command_args(
            ip,
            "systemctl is-active cloudcode-daemon",
        )?)
        .output()
        .context("Failed to check daemon status")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() != "active" {
        anyhow::bail!(
            "Daemon is not running. Status: {}. Check with /ssh or `cloudcode ssh` then `systemctl status cloudcode-daemon`",
            stdout.trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_triple_cx_is_x86() {
        assert_eq!(
            target_triple_for_server_type("cx23"),
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            target_triple_for_server_type("cx53"),
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            target_triple_for_server_type("cpx31"),
            "x86_64-unknown-linux-gnu"
        );
    }

    #[test]
    fn target_triple_cax_is_arm() {
        assert_eq!(
            target_triple_for_server_type("cax11"),
            "aarch64-unknown-linux-gnu"
        );
        assert_eq!(
            target_triple_for_server_type("cax41"),
            "aarch64-unknown-linux-gnu"
        );
    }

    #[test]
    fn install_script_contains_settings_json() {
        let template_fragment = r#"mkdir -p /home/claude/.claude
cat << 'SETTINGS_JSON' > /home/claude/.claude/settings.json"#;
        assert!(template_fragment.contains(".claude/settings.json"));
    }

    #[test]
    fn install_script_settings_json_has_correct_keys() {
        let expanded = r#"{"permissions":{"allow":[],"deny":[]},"hasCompletedOnboarding":true,"skipDangerousModePermissionPrompt":true}"#;
        let parsed: serde_json::Value = serde_json::from_str(expanded).unwrap();
        assert_eq!(parsed["hasCompletedOnboarding"], true);
        assert_eq!(parsed["skipDangerousModePermissionPrompt"], true);
    }
}
