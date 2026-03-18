use colored::Colorize;

use crate::commands::security_report::{ClaudeAuthKind, FileCheck, doctor_report};

fn exists_label(exists: bool) -> &'static str {
    if exists { "present" } else { "missing" }
}

fn mode_label(mode: Option<u32>) -> String {
    mode.map(|m| format!("{:o}", m))
        .unwrap_or_else(|| "unknown".to_string())
}

fn entries_label(entries: usize) -> String {
    format!("{entries} entr{}", if entries == 1 { "y" } else { "ies" })
}

fn print_file_check(file: &FileCheck) {
    let state = if file.path.exists() {
        exists_label(true).green().to_string()
    } else {
        exists_label(false).yellow().to_string()
    };
    let mode = mode_label(file.mode);
    let suffix = file
        .entries
        .map(entries_label)
        .map(|entries| format!(", {entries}"))
        .unwrap_or_default();
    println!(
        "  {:<16} {} ({mode}{suffix})",
        format!("{}:", file.label),
        state
    );
}

pub async fn run() -> anyhow::Result<()> {
    let report = doctor_report()?;

    println!("{}", "cloudcode doctor".bold().cyan());
    println!();

    println!("{}", "Local Files".bold());
    for file in &report.files {
        print_file_check(file);
    }

    println!();
    println!("{}", "Configuration".bold());
    println!(
        "  {:<16} {}",
        "Hetzner token:",
        if report.hetzner_configured {
            "ok".green().to_string()
        } else {
            "attention".yellow().to_string()
        }
    );
    println!(
        "  {:<16} {}",
        "Claude auth:",
        match report.claude_auth {
            Some(ClaudeAuthKind::ApiKey) => "api_key".green().to_string(),
            Some(ClaudeAuthKind::OAuth) => "oauth".green().to_string(),
            Some(ClaudeAuthKind::Other(other)) => other.yellow().to_string(),
            None => "not configured".yellow().to_string(),
        }
    );
    println!(
        "  {:<16} {}",
        "Telegram:",
        if report.telegram_enabled {
            "enabled".green().to_string()
        } else {
            "disabled".dimmed().to_string()
        }
    );

    println!();
    println!("{}", "Provisioned State".bold());
    println!(
        "  {:<16} {}",
        "VPS:",
        if report.provisioned {
            format!(
                "provisioned (server_id={}, ip={})",
                report.server_id.unwrap_or_default(),
                report.server_ip.as_deref().unwrap_or("unknown")
            )
            .green()
            .to_string()
        } else {
            "not provisioned".dimmed().to_string()
        }
    );
    println!(
        "  {:<16} {}",
        "Status:",
        report.status.unwrap_or_else(|| "unknown".to_string())
    );

    println!();
    println!("{}", "Trust Reminders".bold());
    println!("  local machine: ~/.cloudcode contains keys, config, state, and host trust");
    println!("  remote machine: default VPS setup is for unattended operator access");
    println!("  mobile access: if Telegram is enabled, protect that account like operator access");
    println!("  next steps: run `cloudcode security` for revoke and rotation guidance");

    Ok(())
}
