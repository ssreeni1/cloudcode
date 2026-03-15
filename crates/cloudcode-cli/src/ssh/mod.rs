pub mod connection;
pub mod health;
pub mod tunnel;

use anyhow::{Context, Result};
use std::path::PathBuf;
use crate::config::Config;

/// Return the base SSH args used by all SSH invocations.
/// Includes key path, host key checking disabled, and ControlMaster config.
pub fn ssh_base_args(ip: &str) -> Result<Vec<String>> {
    let key_path = Config::ssh_key_path()?;
    let control_path = control_socket_path()?;

    Ok(vec![
        "-i".to_string(),
        key_path.to_string_lossy().to_string(),
        "-o".to_string(), "StrictHostKeyChecking=no".to_string(),
        "-o".to_string(), "UserKnownHostsFile=/dev/null".to_string(),
        "-o".to_string(), "LogLevel=ERROR".to_string(),
        "-o".to_string(), "ConnectTimeout=10".to_string(),
        "-o".to_string(), "ControlMaster=auto".to_string(),
        "-o".to_string(), format!("ControlPath={}", control_path.display()),
        "-o".to_string(), "ControlPersist=300".to_string(),
    ])
}

/// Path to the SSH ControlMaster socket
pub fn control_socket_path() -> Result<PathBuf> {
    let dir = Config::dir()?;
    Ok(dir.join("ssh-control-%C"))
}

/// Path to the daemon forwarding socket for a given server
pub fn daemon_socket_path(server_id: u64) -> Result<PathBuf> {
    Ok(PathBuf::from(format!("/tmp/cloudcode-daemon-{}.sock", server_id)))
}
