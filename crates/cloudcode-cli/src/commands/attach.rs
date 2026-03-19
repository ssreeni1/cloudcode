use crate::config::Config;
use crate::ssh::ssh_base_args;
use crate::ssh::tunnel::DaemonClient;
use crate::state::VpsState;
use anyhow::{Context, Result};
use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use colored::Colorize;

fn shell_single_quote(value: &str) -> String {
    // SSH remote commands are parsed by a shell on the remote side.
    // Wrap the session name in single quotes and escape embedded quotes.
    let escaped = value.replace('\'', r#"'\''"#);
    format!("'{}'", escaped)
}

/// Build the attach-specific SSH args that are appended after the base SSH args.
///
/// These disable ControlMaster (to avoid stale control sockets from prior
/// operations), force PTY allocation, and specify the remote tmux attach command.
fn attach_ssh_args(ip: &str, session: &str) -> Vec<String> {
    let quoted_session = shell_single_quote(session);
    vec![
        "-o".to_string(),
        "ControlMaster=no".to_string(),
        "-o".to_string(),
        "ControlPath=none".to_string(),
        "-t".to_string(), // force PTY allocation
        format!("claude@{}", ip),
        format!("tmux attach-session -t {}", quoted_session),
    ]
}

pub async fn run(session: String) -> Result<()> {
    let state = VpsState::load()?;
    if !state.is_provisioned() {
        anyhow::bail!("No VPS provisioned. Run /up or `cloudcode up` to provision.");
    }

    let ip = state.server_ip.as_ref().context("No server IP in state")?;

    // Pre-attach: check if the session exists via daemon query
    if let Ok(config) = Config::load() {
        if let Ok(mut client) = DaemonClient::connect(&state, &config) {
            if let Ok(DaemonResponse::Sessions { sessions }) = client.request(&DaemonRequest::List)
            {
                let exists = sessions.iter().any(|s| s.name == session);
                if !exists {
                    eprintln!("{} Session '{}' not found.", "Error:".red(), session);
                    if sessions.is_empty() {
                        eprintln!(
                            "No active sessions. Create one with /spawn or `cloudcode spawn`."
                        );
                    } else {
                        eprintln!("Available sessions:");
                        for s in &sessions {
                            eprintln!(
                                "  {} [{}]",
                                s.name.green(),
                                format!("{:?}", s.state).yellow()
                            );
                        }
                    }
                    return Ok(());
                }
            }
            // If the daemon query failed, fall through to attempt attach anyway
        }
    }

    println!(
        "{} Attaching to session '{}' on {}...",
        "→".cyan(),
        session.green(),
        ip
    );
    println!(
        "{}",
        "  (Detach with Ctrl-b d, or close terminal to disconnect)".dimmed()
    );

    let mut args = ssh_base_args(ip)?;
    args.extend(attach_ssh_args(ip, &session));

    let mut cmd = std::process::Command::new("ssh");
    cmd.args(&args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    // If the local TERM isn't widely supported (e.g. xterm-ghostty),
    // override to xterm-256color so tmux works on the remote.
    if let Ok(term) = std::env::var("TERM") {
        if !term.starts_with("xterm-256")
            && !term.starts_with("screen")
            && !term.starts_with("tmux")
        {
            cmd.env("TERM", "xterm-256color");
        }
    }

    let status = cmd.status().context("Failed to start SSH")?;

    if status.success() {
        println!("\n{} Detached from session '{}'", "✓".green(), session);
    } else {
        let code = status.code().unwrap_or(-1);
        if code == 1 {
            eprintln!(
                "{} Session '{}' not found. Use /list or `cloudcode list` to see available sessions.",
                "Error:".red(),
                session
            );
        } else {
            eprintln!("{} SSH exited with code {}", "Error:".red(), code);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{attach_ssh_args, shell_single_quote};

    #[test]
    fn shell_single_quote_wraps_plain_text() {
        assert_eq!(shell_single_quote("session-123"), "'session-123'");
    }

    #[test]
    fn shell_single_quote_escapes_inner_quotes() {
        assert_eq!(
            shell_single_quote("a'b\"c; rm -rf /"),
            "'a'\\''b\"c; rm -rf /'"
        );
    }

    // -----------------------------------------------------------------------
    // attach_ssh_args tests
    // -----------------------------------------------------------------------

    #[test]
    fn attach_ssh_args_includes_control_master_no() {
        let args = attach_ssh_args("1.2.3.4", "my-session");
        // ControlMaster=no must appear to disable connection multiplexing
        let cm_idx = args.iter().position(|a| a == "ControlMaster=no");
        assert!(
            cm_idx.is_some(),
            "ControlMaster=no must be present in attach args"
        );
        // It should be preceded by "-o"
        let idx = cm_idx.unwrap();
        assert!(idx > 0, "ControlMaster=no should be preceded by -o");
        assert_eq!(args[idx - 1], "-o");
    }

    #[test]
    fn attach_ssh_args_includes_control_path_none() {
        let args = attach_ssh_args("1.2.3.4", "my-session");
        let cp_idx = args.iter().position(|a| a == "ControlPath=none");
        assert!(
            cp_idx.is_some(),
            "ControlPath=none must be present in attach args"
        );
        let idx = cp_idx.unwrap();
        assert!(idx > 0);
        assert_eq!(args[idx - 1], "-o");
    }

    #[test]
    fn attach_ssh_args_includes_pty_flag() {
        let args = attach_ssh_args("1.2.3.4", "my-session");
        assert!(
            args.contains(&"-t".to_string()),
            "PTY allocation flag -t must be present"
        );
    }

    #[test]
    fn attach_ssh_args_includes_user_and_host() {
        let args = attach_ssh_args("10.0.0.5", "test-session");
        assert!(
            args.contains(&"claude@10.0.0.5".to_string()),
            "Should include claude@<ip>"
        );
    }

    #[test]
    fn attach_ssh_args_includes_tmux_attach_command() {
        let args = attach_ssh_args("1.2.3.4", "my-session");
        let last = args.last().unwrap();
        assert!(
            last.starts_with("tmux attach-session -t "),
            "Last arg should be the tmux attach command, got: {}",
            last
        );
        assert!(
            last.contains("my-session"),
            "tmux command should reference the session name"
        );
    }

    #[test]
    fn attach_ssh_args_quotes_session_name() {
        // Session name should be shell-quoted in the tmux command
        let args = attach_ssh_args("1.2.3.4", "sess-123");
        let tmux_cmd = args.last().unwrap();
        assert_eq!(tmux_cmd, "tmux attach-session -t 'sess-123'");
    }

    #[test]
    fn attach_ssh_args_control_master_overrides_base() {
        // The base args set ControlMaster=auto. The attach args must set
        // ControlMaster=no AFTER the base args so SSH uses the last value.
        // Verify the attach args contain the override (the caller extends
        // base args with these, so they appear later in the final arg list).
        let args = attach_ssh_args("1.2.3.4", "s");
        assert!(args.contains(&"ControlMaster=no".to_string()));
        assert!(args.contains(&"ControlPath=none".to_string()));
        // Verify ControlMaster=auto is NOT in the attach args
        assert!(
            !args.contains(&"ControlMaster=auto".to_string()),
            "Attach args must not contain ControlMaster=auto"
        );
    }
}
