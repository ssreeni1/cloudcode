use anyhow::{Context, Result};
use colored::Colorize;

use crate::ssh::ssh_base_args;
use crate::state::VpsState;

pub async fn run(target: Option<String>) -> Result<()> {
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run /up or `cloudcode up` to provision.");
    }

    let ip = state.server_ip.as_ref().context("No server IP in state")?;
    let target = target.unwrap_or_else(|| "setup".to_string());

    let remote_cmd = match target.as_str() {
        "setup" => "tail -50 /var/log/cloudcode-setup.log".to_string(),
        "daemon" => "journalctl -u cloudcode-daemon -n 50 --no-pager".to_string(),
        other => {
            eprintln!(
                "{} Unknown log target '{}'. Valid targets: setup, daemon",
                "Error:".red(),
                other
            );
            return Ok(());
        }
    };

    println!(
        "{} Fetching {} logs from {}...",
        "→".cyan(),
        target.bold(),
        ip
    );

    let mut args = ssh_base_args(ip)?;
    args.extend([format!("claude@{}", ip), remote_cmd]);

    let status = std::process::Command::new("ssh")
        .args(&args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to start SSH")?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        eprintln!("{} SSH exited with code {}", "Error:".red(), code);
    }

    Ok(())
}
