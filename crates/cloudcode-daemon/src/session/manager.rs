use anyhow::{Context, Result, bail};
use cloudcode_common::session::{SessionInfo, SessionState};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::sync::Mutex;

/// Output from a send operation, including text and any files created.
pub struct SendOutput {
    pub text: String,
    pub files: Vec<PathBuf>,
}

/// Snapshot files in watched directories (non-recursive, just top-level).
fn snapshot_files(dirs: &[PathBuf]) -> HashSet<PathBuf> {
    let mut files = HashSet::new();
    for dir in dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    files.insert(path);
                }
            }
        }
    }
    files
}

/// Check if a file is a type we should send via Telegram.
fn is_sendable_file(path: &PathBuf) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext.to_lowercase().as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "svg"
            | "pdf"
            | "md"
            | "txt"
            | "json"
            | "csv"
            | "html"
            | "log"
    )
}

/// Validate that a session name contains only safe characters.
/// Rejects anything not matching `^[a-zA-Z0-9_-]+$` or longer than 64 chars.
fn validate_session_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Session name must not be empty");
    }
    if name.len() > 64 {
        bail!(
            "Session name too long ({} chars, max 64): '{}'",
            name.len(),
            name
        );
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!(
            "Session name contains invalid characters (only a-zA-Z0-9_- allowed): '{}'",
            name
        );
    }
    Ok(())
}

fn daemon_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/claude"))
}

fn session_workspace_dir_from_home(home: &Path, session: &str) -> PathBuf {
    home.join(".cloudcode")
        .join("sessions")
        .join(session)
        .join("workspace")
}

fn session_workspace_dir(session: &str) -> Result<PathBuf> {
    validate_session_name(session)?;
    let dir = session_workspace_dir_from_home(&daemon_home_dir(), session);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create workspace dir {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(parent) = dir.parent() {
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("Failed to secure {}", parent.display()))?;
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("Failed to secure {}", dir.display()))?;
    }
    Ok(dir)
}

async fn session_runtime_workdir(session: &str) -> Result<PathBuf> {
    let workspace = session_workspace_dir_from_home(&daemon_home_dir(), session);
    if workspace.exists() {
        return Ok(workspace);
    }

    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            session,
            "#{pane_current_path}",
        ])
        .output()
        .await
        .context("Failed to query tmux pane current path")?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    std::fs::create_dir_all(&workspace)
        .with_context(|| format!("Failed to create workspace dir {}", workspace.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(parent) = workspace.parent() {
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
        let _ = std::fs::set_permissions(&workspace, std::fs::Permissions::from_mode(0o700));
    }
    Ok(workspace)
}

fn session_snapshot_dirs_from_workdir(workdir: &Path) -> Vec<PathBuf> {
    vec![
        workdir.to_path_buf(),
        workdir.join("screenshots"),
        workdir.join("output"),
        PathBuf::from("/tmp"),
    ]
}

pub struct SessionManager {
    send_locks: std::sync::Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            send_locks: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Spawn a new Claude Code session in tmux
    pub async fn spawn(&self, name: Option<String>) -> Result<SessionInfo> {
        let name = name.unwrap_or_else(|| {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            // Generate 4 random alphanumeric chars using timestamp nanos + pid
            let seed = ts
                .wrapping_mul(6364136223846793005)
                .wrapping_add(std::process::id() as u64);
            let chars: Vec<char> = (0..4u64)
                .map(|i| {
                    let v = seed.wrapping_mul(i.wrapping_add(1)).wrapping_add(i * 7919);
                    let idx = (v % 36) as u8;
                    if idx < 10 {
                        (b'0' + idx) as char
                    } else {
                        (b'a' + idx - 10) as char
                    }
                })
                .collect();
            let suffix: String = chars.into_iter().collect();
            format!("session-{}-{}", ts % 10000, suffix)
        });

        validate_session_name(&name)?;
        let workspace = session_workspace_dir(&name)?;
        let workspace_arg = workspace.to_string_lossy().to_string();

        // Check if session already exists
        if self.session_exists(&name).await {
            bail!("Session '{}' already exists", name);
        }

        // Create tmux session running claude
        // Claude Code installs to ~/.local/bin/claude
        // Use shell wrapper to pass all flags properly to claude
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/claude".to_string());
        let claude_bin = format!("{}/.local/bin/claude", home);
        let home_env = format!("HOME={}", home);
        let shell_cmd = format!(
            "exec {} --dangerously-skip-permissions --permission-mode bypassPermissions",
            claude_bin
        );
        let status = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &name,
                "-x",
                "200",
                "-y",
                "50",
                "-c",
                &workspace_arg,
                "-e",
                &home_env,
                "sh",
                "-lc",
                &shell_cmd,
            ])
            .status()
            .await
            .context("Failed to start tmux session")?;

        if !status.success() {
            bail!("tmux new-session failed with status {}", status);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(SessionInfo {
            name,
            state: SessionState::Running,
            created_at: now,
            last_activity: now,
        })
    }

    /// List all tmux sessions
    pub async fn list(&self) -> Result<Vec<SessionInfo>> {
        let output = Command::new("tmux")
            .args([
                "list-sessions",
                "-F",
                "#{session_name}:#{session_created}:#{session_activity}",
            ])
            .output()
            .await;

        match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let sessions = stdout
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| {
                        let parts: Vec<&str> = line.splitn(3, ':').collect();
                        let name = parts.first().unwrap_or(&"unknown").to_string();
                        let created_at = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                        let last_activity = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                        SessionInfo {
                            name,
                            state: SessionState::Running,
                            created_at,
                            last_activity,
                        }
                    })
                    .collect();
                Ok(sessions)
            }
            // tmux returns error when no sessions exist
            _ => Ok(Vec::new()),
        }
    }

    /// Kill a tmux session
    pub async fn kill(&self, session: &str) -> Result<()> {
        validate_session_name(session)?;
        if !self.session_exists(session).await {
            bail!("Session '{}' does not exist", session);
        }

        let status = Command::new("tmux")
            .args(["kill-session", "-t", session])
            .status()
            .await
            .context("Failed to kill tmux session")?;

        if !status.success() {
            bail!("tmux kill-session failed");
        }
        Ok(())
    }

    /// Send a message to a session using claude -p (print mode) for clean output.
    /// The tmux session is kept alive for interactive `cloudcode open` access,
    /// but programmatic sends use print mode with --continue for clean text output.
    /// Returns text response and paths to any new files created during execution.
    pub async fn send(&self, session: &str, message: &str) -> Result<SendOutput> {
        validate_session_name(session)?;
        if !self.session_exists(session).await {
            bail!("Session '{}' does not exist", session);
        }

        // Acquire per-session lock (serialize concurrent sends)
        let lock = {
            let mut locks = self.send_locks.lock().unwrap();
            locks
                .entry(session.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;

        let home = daemon_home_dir().to_string_lossy().to_string();
        let claude_bin = format!("{}/.local/bin/claude", home);
        let workdir = session_runtime_workdir(session).await?;

        // Snapshot existing files before running claude
        let watch_dirs = session_snapshot_dirs_from_workdir(&workdir);
        let files_before = snapshot_files(&watch_dirs);

        // Use claude -p (print mode) for clean text output.
        let output = Command::new(&claude_bin)
            .args([
                "-p",
                "--dangerously-skip-permissions",
                "--continue",
                message,
            ])
            .current_dir(&workdir)
            .output()
            .await
            .context("Failed to run claude in print mode")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("claude -p failed: {}", stderr);
        }

        let response = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Find new files created during execution
        let files_after = snapshot_files(&watch_dirs);
        let new_files: Vec<PathBuf> = files_after
            .into_iter()
            .filter(|f| !files_before.contains(f))
            .filter(|f| is_sendable_file(f))
            .collect();

        Ok(SendOutput {
            text: response,
            files: new_files,
        })
    }

    async fn session_exists(&self, name: &str) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", name])
            .status()
            .await
            .is_ok_and(|s| s.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // validate_session_name tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_session_name_valid() {
        assert!(validate_session_name("my-session").is_ok());
        assert!(validate_session_name("session_123").is_ok());
        assert!(validate_session_name("ABC").is_ok());
        assert!(validate_session_name("a").is_ok());
        assert!(validate_session_name("a-b_c-1").is_ok());
    }

    #[test]
    fn test_validate_session_name_empty() {
        assert!(validate_session_name("").is_err());
    }

    #[test]
    fn test_validate_session_name_too_long() {
        let long_name = "a".repeat(65);
        assert!(validate_session_name(&long_name).is_err());
        // Exactly 64 should be fine
        let max_name = "a".repeat(64);
        assert!(validate_session_name(&max_name).is_ok());
    }

    #[test]
    fn test_validate_session_name_rejects_special_chars() {
        assert!(validate_session_name("foo;bar").is_err());
        assert!(validate_session_name("foo bar").is_err());
        assert!(validate_session_name("foo/bar").is_err());
        assert!(validate_session_name("$(whoami)").is_err());
        assert!(validate_session_name("foo\nbar").is_err());
        assert!(validate_session_name("foo`id`").is_err());
        assert!(validate_session_name("foo|bar").is_err());
        assert!(validate_session_name("foo&bar").is_err());
        assert!(validate_session_name("name.with.dots").is_err());
    }

    // -----------------------------------------------------------------------
    // SessionManager struct tests (dead code removed)
    // -----------------------------------------------------------------------

    #[test]
    fn session_manager_new_has_no_extra_fields() {
        // SessionManager should only have send_locks now
        // (output_counter and output_dir were removed)
        let mgr = SessionManager::new();
        // Verify send_locks is accessible and empty
        let locks = mgr.send_locks.lock().unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn session_workspace_dir_is_namespaced_by_session() {
        let home = PathBuf::from("/home/claude");
        let dir = session_workspace_dir_from_home(&home, "alpha");
        assert_eq!(
            dir,
            PathBuf::from("/home/claude/.cloudcode/sessions/alpha/workspace")
        );
    }

    // -----------------------------------------------------------------------
    // is_sendable_file tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_sendable_file_images() {
        assert!(is_sendable_file(&PathBuf::from("/tmp/photo.png")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/photo.jpg")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/photo.jpeg")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/photo.gif")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/photo.webp")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/photo.svg")));
    }

    #[test]
    fn test_is_sendable_file_documents() {
        assert!(is_sendable_file(&PathBuf::from("/tmp/doc.pdf")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/doc.md")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/doc.txt")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/data.json")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/data.csv")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/page.html")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/output.log")));
    }

    #[test]
    fn test_is_sendable_file_case_insensitive() {
        assert!(is_sendable_file(&PathBuf::from("/tmp/PHOTO.PNG")));
        assert!(is_sendable_file(&PathBuf::from("/tmp/Doc.PDF")));
    }

    #[test]
    fn test_is_sendable_file_rejects_binaries() {
        assert!(!is_sendable_file(&PathBuf::from("/tmp/program.exe")));
        assert!(!is_sendable_file(&PathBuf::from("/tmp/lib.so")));
        assert!(!is_sendable_file(&PathBuf::from("/tmp/archive.tar.gz")));
        assert!(!is_sendable_file(&PathBuf::from("/tmp/binary")));
        assert!(!is_sendable_file(&PathBuf::from("/tmp/script.rs")));
        assert!(!is_sendable_file(&PathBuf::from("/tmp/code.py")));
    }

    #[test]
    fn test_is_sendable_file_no_extension() {
        assert!(!is_sendable_file(&PathBuf::from("/tmp/Makefile")));
        assert!(!is_sendable_file(&PathBuf::from("/tmp/.hidden")));
    }

    // -----------------------------------------------------------------------
    // snapshot_files tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_snapshot_files_nonexistent_dir() {
        let result = snapshot_files(&[PathBuf::from("/nonexistent/dir/12345")]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_snapshot_files_empty_list() {
        let result = snapshot_files(&[]);
        assert!(result.is_empty());
    }
}
