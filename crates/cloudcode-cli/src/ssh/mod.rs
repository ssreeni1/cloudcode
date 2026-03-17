pub mod connection;
pub mod health;
pub mod tunnel;

use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::path::PathBuf;

use crate::config::Config;

/// Return the base SSH args used by all SSH invocations.
/// Uses a managed known_hosts file under ~/.cloudcode and bootstraps new hosts
/// with `accept-new` so first connect works without disabling verification.
pub fn ssh_base_args(_ip: &str) -> Result<Vec<String>> {
    let key_path = Config::ssh_key_path()?;
    let control_path = control_socket_path()?;
    let known_hosts_path = known_hosts_path()?;
    ensure_known_hosts_file(&known_hosts_path)?;

    Ok(vec![
        "-i".to_string(),
        key_path.to_string_lossy().to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        format!("UserKnownHostsFile={}", known_hosts_path.display()),
        "-o".to_string(),
        "GlobalKnownHostsFile=/dev/null".to_string(),
        "-o".to_string(),
        "LogLevel=ERROR".to_string(),
        "-o".to_string(),
        "ConnectTimeout=10".to_string(),
        "-o".to_string(),
        "ControlMaster=auto".to_string(),
        "-o".to_string(),
        format!("ControlPath={}", control_path.display()),
        "-o".to_string(),
        "ControlPersist=300".to_string(),
    ])
}

/// Path to the SSH ControlMaster socket
pub fn control_socket_path() -> Result<PathBuf> {
    let dir = Config::dir()?;
    Ok(dir.join("ssh-control-%C"))
}

/// Path to the managed known_hosts file used by cloudcode.
pub fn known_hosts_path() -> Result<PathBuf> {
    Ok(Config::dir()?.join("known_hosts"))
}

fn ensure_known_hosts_file(path: &PathBuf) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create cloudcode config directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
    }

    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .context("Failed to create managed known_hosts file")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Quote a shell argument for use in `rsync -e`.
pub fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }

    if arg.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '_' | '-' | '.' | '/' | ':' | '%' | '=' | ',' | '@')
    }) {
        return arg.to_string();
    }

    format!("'{}'", arg.replace('\'', r#"'"'"'"#))
}

/// Render a command line suitable for passing to a shell, such as rsync's `-e`.
pub fn shell_command(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Path to the daemon forwarding socket for a given server
pub fn daemon_socket_path(server_id: u64) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    let dir = home.join(".cloudcode").join("sockets");
    std::fs::create_dir_all(&dir).context("Failed to create sockets directory")?;
    Ok(dir.join(format!("daemon-{}.sock", server_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_hosts_path_stays_under_cloudcode_dir() {
        let path = known_hosts_path().unwrap();
        assert!(path.ends_with("known_hosts"));
        assert!(path.to_string_lossy().contains(".cloudcode"));
    }

    #[test]
    fn shell_quote_leaves_simple_args_unquoted() {
        assert_eq!(shell_quote("ssh"), "ssh");
        assert_eq!(shell_quote("ControlPersist=300"), "ControlPersist=300");
    }

    #[test]
    fn shell_quote_handles_spaces_and_quotes() {
        assert_eq!(shell_quote("hello world"), "'hello world'");
        assert_eq!(shell_quote("it's fine"), "'it'\"'\"'s fine'");
    }

    #[test]
    fn shell_command_quotes_each_argument() {
        let rendered = shell_command(&vec![
            "ssh".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            "hello world".to_string(),
        ]);
        assert_eq!(
            rendered,
            "ssh -o StrictHostKeyChecking=accept-new 'hello world'"
        );
    }
}
