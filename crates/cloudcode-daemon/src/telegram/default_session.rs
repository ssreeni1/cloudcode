use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

fn daemon_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/claude"))
}

fn default_session_path() -> PathBuf {
    daemon_home_dir()
        .join(".cloudcode")
        .join("telegram-default-session.txt")
}

#[cfg(unix)]
fn set_private_permissions(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    match fs::set_permissions(path, fs::Permissions::from_mode(0o700)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(err) => Err(err).with_context(|| format!("Failed to secure {}", path.display())),
    }
}

fn read_default_session(path: &PathBuf) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => {
            let session = content.lines().next().unwrap_or("").trim().to_string();
            if session.is_empty() {
                Ok(None)
            } else {
                Ok(Some(session))
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("Failed to read {}", path.display())),
    }
}

fn write_atomic(path: &PathBuf, content: Option<&str>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
        #[cfg(unix)]
        {
            set_private_permissions(&parent.to_path_buf())?;
        }
    }

    match content {
        Some(value) => {
            let tmp_path = path.with_extension("tmp");
            fs::write(&tmp_path, value)
                .with_context(|| format!("Failed to write {}", tmp_path.display()))?;
            fs::rename(&tmp_path, path)
                .with_context(|| format!("Failed to replace {}", path.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                    .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
            }
        }
        None => {
            if path.exists() {
                fs::remove_file(path)
                    .with_context(|| format!("Failed to remove {}", path.display()))?;
            }
        }
    }

    Ok(())
}

pub struct DefaultSessionStore {
    path: PathBuf,
    value: Mutex<Option<String>>,
}

impl DefaultSessionStore {
    fn new(path: PathBuf, value: Option<String>) -> Self {
        Self {
            path,
            value: Mutex::new(value),
        }
    }

    pub fn load() -> Result<Self> {
        let path = default_session_path();
        let value = read_default_session(&path)?;
        Ok(Self::new(path, value))
    }

    pub fn empty() -> Self {
        Self::new(default_session_path(), None)
    }

    pub fn current(&self) -> Option<String> {
        self.value
            .lock()
            .expect("default session mutex poisoned")
            .clone()
    }

    pub fn set(&self, session: Option<String>) -> Result<()> {
        *self.value.lock().expect("default session mutex poisoned") = session.clone();

        match session {
            Some(ref value) => write_atomic(&self.path, Some(value))?,
            None => write_atomic(&self.path, None)?,
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.set(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_path() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "cloudcode-default-session-{}-{}.txt",
            std::process::id(),
            stamp
        ))
    }

    #[test]
    fn load_missing_file_returns_empty_store() {
        let path = unique_test_path();
        let store = DefaultSessionStore::new(path.clone(), read_default_session(&path).unwrap());
        assert!(store.current().is_none());
    }

    #[test]
    fn set_round_trips_to_disk() {
        let path = unique_test_path();
        let store = DefaultSessionStore::new(path.clone(), None);

        store.set(Some("alpha".to_string())).unwrap();
        let loaded = read_default_session(&path).unwrap();
        assert_eq!(loaded.as_deref(), Some("alpha"));
        assert_eq!(store.current().as_deref(), Some("alpha"));

        store.clear().unwrap();
        assert!(read_default_session(&path).unwrap().is_none());
    }
}
