use anyhow::{Context, Result};
use std::path::PathBuf;

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("Could not determine home directory")
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".cloudcode"))
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn state_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("state.json"))
}

pub fn ssh_key() -> Result<PathBuf> {
    Ok(config_dir()?.join("id_ed25519"))
}

pub fn ssh_pub_key() -> Result<PathBuf> {
    Ok(config_dir()?.join("id_ed25519.pub"))
}

pub fn known_hosts() -> Result<PathBuf> {
    Ok(config_dir()?.join("known_hosts"))
}

pub fn sockets_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("sockets"))
}

pub fn embedded_daemon_cache_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("cache").join("embedded-daemons"))
}
