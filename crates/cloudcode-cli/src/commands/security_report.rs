use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::ssh;
use crate::state::VpsState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Plain,
    Muted,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustLine {
    pub kind: LineKind,
    pub text: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustSummary {
    pub compact: bool,
    pub lines: Vec<TrustLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityGuide {
    pub trust_model: Vec<&'static str>,
    pub revoke_rotate: Vec<&'static str>,
    pub verify: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCheck {
    pub label: &'static str,
    pub path: PathBuf,
    pub mode: Option<u32>,
    pub entries: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeAuthKind {
    ApiKey,
    OAuth,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub files: Vec<FileCheck>,
    pub hetzner_configured: bool,
    pub claude_auth: Option<ClaudeAuthKind>,
    pub telegram_enabled: bool,
    pub provisioned: bool,
    pub server_id: Option<u64>,
    pub server_ip: Option<String>,
    pub status: Option<String>,
}

fn build_doctor_report(config: &Config, state: &VpsState) -> Result<DoctorReport> {
    let config_path = Config::path()?;
    let state_path = VpsState::path()?;
    let ssh_key_path = Config::ssh_key_path()?;
    let ssh_pub_key_path = Config::ssh_pub_key_path()?;
    let known_hosts_path = ssh::known_hosts_path()?;
    let config_dir = Config::dir()?;

    let claude_auth = config.claude.as_ref().map(|claude| {
        if claude.uses_api_key() {
            ClaudeAuthKind::ApiKey
        } else if claude.uses_oauth() {
            ClaudeAuthKind::OAuth
        } else {
            ClaudeAuthKind::Other(claude.auth_label().to_string())
        }
    });

    Ok(DoctorReport {
        files: vec![
            file_check("config.toml", config_path, false),
            file_check("state.json", state_path, false),
            file_check("ssh private key", ssh_key_path, false),
            file_check("ssh public key", ssh_pub_key_path, false),
            file_check("known_hosts", known_hosts_path, true),
            file_check("config dir", config_dir, false),
        ],
        hetzner_configured: config.hetzner.is_some(),
        claude_auth,
        telegram_enabled: config.telegram.is_some(),
        provisioned: state.is_provisioned(),
        server_id: state.server_id,
        server_ip: state.server_ip.clone(),
        status: state.status_name().map(str::to_string),
    })
}

fn count_non_comment_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|content| {
            content
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    !trimmed.is_empty() && !trimmed.starts_with('#')
                })
                .count()
        })
        .unwrap_or(0)
}

#[cfg(unix)]
fn mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .ok()
        .map(|meta| meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn mode(_path: &Path) -> Option<u32> {
    None
}

fn file_check(label: &'static str, path: PathBuf, count_entries: bool) -> FileCheck {
    let entries = if count_entries && path.exists() {
        Some(count_non_comment_lines(&path))
    } else {
        None
    };

    FileCheck {
        label,
        mode: mode(&path),
        path,
        entries,
    }
}

pub fn trust_summary(compact: bool) -> TrustSummary {
    let lines = if compact {
        vec![
            TrustLine {
                kind: LineKind::Muted,
                text: "cloudcode provisions a personal VPS for persistent Claude sessions that you can reach from desktop or mobile.",
            },
            TrustLine {
                kind: LineKind::Muted,
                text: "The default setup uses a managed SSH known_hosts file, stores local secrets under ~/.cloudcode, and enables unattended remote control features on the VPS.",
            },
            TrustLine {
                kind: LineKind::Warning,
                text: "That convenience means the remote 'claude' user has elevated local control on the VPS, and Telegram/mobile access should be treated like full operator access.",
            },
            TrustLine {
                kind: LineKind::Muted,
                text: "Run `cloudcode security` for revoke and rotation steps.",
            },
        ]
    } else {
        vec![
            TrustLine {
                kind: LineKind::Plain,
                text: "cloudcode is an operator tool, not a sandbox. Its default goal is seamless setup for persistent remote Claude sessions reachable from desktop or mobile.",
            },
            TrustLine {
                kind: LineKind::Plain,
                text: "Your local machine stores keys, config, known_hosts, and state in ~/.cloudcode.",
            },
            TrustLine {
                kind: LineKind::Plain,
                text: "The provisioned VPS is configured for unattended remote operation, so the remote 'claude' user has elevated local control and Claude runs with bypass-permissions mode.",
            },
            TrustLine {
                kind: LineKind::Plain,
                text: "If Telegram is enabled, your Telegram chat effectively becomes a remote-control surface for that VPS.",
            },
        ]
    };

    TrustSummary { compact, lines }
}

pub fn security_guide() -> SecurityGuide {
    SecurityGuide {
        trust_model: vec![
            "cloudcode is an operator tool, not a sandbox. Its default goal is seamless setup for persistent remote Claude sessions reachable from desktop or mobile.",
            "Your local machine stores keys, config, known_hosts, and state in ~/.cloudcode.",
            "The provisioned VPS is configured for unattended remote operation, so the remote 'claude' user has elevated local control and Claude runs with bypass-permissions mode.",
            "If Telegram is enabled, your Telegram chat effectively becomes a remote-control surface for that VPS.",
        ],
        revoke_rotate: vec![
            "Run `cloudcode down` to destroy the VPS if you no longer trust it.",
            "Remove or rotate local SSH keys under `~/.cloudcode/id_ed25519*`.",
            "Remove stale host trust entries by deleting `~/.cloudcode/known_hosts`.",
            "Rotate your Hetzner API token and Anthropic API key if they may have been exposed.",
            "Regenerate your Telegram bot token in BotFather if Telegram access may have been exposed.",
        ],
        verify: vec![
            "Run `cloudcode doctor` to inspect local security-relevant state.",
            "Run `cloudcode status` to confirm whether a VPS is still provisioned.",
        ],
    }
}

pub fn doctor_report() -> Result<DoctorReport> {
    let config = Config::load()?;
    let state = VpsState::load()?;
    build_doctor_report(&config, &state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMethod, ClaudeConfig, Config, HetznerConfig};
    use crate::state::{VpsState, VpsStatus};

    #[test]
    fn compact_trust_summary_has_warning() {
        let summary = trust_summary(true);
        assert!(summary.compact);
        assert_eq!(summary.lines.len(), 4);
        assert_eq!(summary.lines[2].kind, LineKind::Warning);
    }

    #[test]
    fn security_guide_contains_revoke_and_verify_sections() {
        let guide = security_guide();
        assert!(!guide.trust_model.is_empty());
        assert!(!guide.revoke_rotate.is_empty());
        assert!(!guide.verify.is_empty());
    }

    #[test]
    fn doctor_report_from_loaded_state_has_expected_shape() {
        let config = Config {
            hetzner: Some(HetznerConfig {
                api_token: "token".to_string(),
            }),
            claude: Some(ClaudeConfig {
                auth_method: AuthMethod::ApiKey,
                api_key: Some("key".to_string()),
                oauth_token: None,
            }),
            telegram: None,
            vps: None,
        };
        let state = VpsState {
            server_id: Some(42),
            server_ip: Some("1.2.3.4".to_string()),
            ssh_key_id: None,
            status: Some(VpsStatus::Running),
        };

        let report = build_doctor_report(&config, &state);
        assert!(report.is_ok());
        let report = report.unwrap();
        let labels: Vec<&str> = report.files.iter().map(|f| f.label).collect();
        assert!(labels.contains(&"config.toml"));
        assert!(labels.contains(&"state.json"));
        assert!(labels.contains(&"known_hosts"));
        assert!(report.hetzner_configured);
        assert!(matches!(report.claude_auth, Some(ClaudeAuthKind::ApiKey)));
        assert!(report.provisioned);
        assert_eq!(report.status.as_deref(), Some("running"));
    }
}
