use anyhow::{Context, Result, bail};
use cloudcode_common::provider::AiProvider;
use cloudcode_common::session::{SessionInfo, SessionState};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

/// Output from a send operation, including text and any files created.
pub struct SendOutput {
    pub text: String,
    pub files: Vec<PathBuf>,
}

/// Directories to skip during recursive file scanning (heavy/irrelevant dirs).
const SCAN_EXCLUDE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    "target",
    ".venv",
    "venv",
];

/// Recursively walk a directory up to a depth limit, collecting files.
fn walk_dir(dir: &Path, depth: usize, files: &mut HashMap<PathBuf, (SystemTime, u64)>) {
    if depth == 0 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip known heavy directories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if SCAN_EXCLUDE_DIRS.contains(&name) {
                    continue;
                }
            }
            walk_dir(&path, depth - 1, files);
        } else if path.is_file() {
            if let Ok(meta) = path.metadata() {
                let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
                let size = meta.len();
                files.insert(path, (mtime, size));
            }
        }
    }
}

/// Snapshot files in watched directories (recursive, depth limit 5).
fn snapshot_files(dirs: &[PathBuf]) -> HashMap<PathBuf, (SystemTime, u64)> {
    let mut files = HashMap::new();
    for dir in dirs {
        walk_dir(dir, 5, &mut files);
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

/// Find the claude binary — checks ~/.local/bin/claude (curl installer)
/// then /usr/local/bin/claude (npm installer).
fn find_claude_bin() -> String {
    let home = daemon_home_dir();
    let local_bin = home.join(".local/bin/claude");
    if local_bin.exists() {
        return local_bin.to_string_lossy().to_string();
    }
    // npm global install path
    "/usr/local/bin/claude".to_string()
}

/// Strip ANSI escape codes from a string.
///
/// Removes sequences matching `\x1b\[[0-9;]*[a-zA-Z]`, which covers standard
/// SGR (color/style), cursor movement, and erase codes.
fn strip_ansi_codes(input: &str) -> String {
    static ANSI_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());
    ANSI_RE.replace_all(input, "").to_string()
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

fn session_snapshot_dirs_from_workdir(workdir: &Path, session_tmp: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![
        workdir.to_path_buf(),
        workdir.join("screenshots"),
        workdir.join("output"),
    ];
    if let Some(tmp) = session_tmp {
        dirs.push(tmp.to_path_buf());
    }
    dirs
}

pub struct SessionManager {
    send_locks: std::sync::Mutex<HashMap<String, Arc<Mutex<()>>>>,
    default_provider: RwLock<AiProvider>,
}

impl SessionManager {
    pub fn new(provider: AiProvider) -> Self {
        Self {
            send_locks: std::sync::Mutex::new(HashMap::new()),
            default_provider: RwLock::new(provider),
        }
    }

    /// Get the current default provider.
    pub fn current_provider(&self) -> AiProvider {
        *self.default_provider.read().unwrap()
    }

    /// Set the default provider (persists to file).
    pub fn set_provider(&self, provider: AiProvider) {
        *self.default_provider.write().unwrap() = provider;
        let path = daemon_home_dir()
            .join(".cloudcode")
            .join("default-provider");
        let _ = std::fs::write(&path, provider.as_str());
    }

    /// Get the provider for a specific session (reads per-session file, falls back to default).
    fn session_provider(&self, session: &str) -> AiProvider {
        let path = daemon_home_dir()
            .join(".cloudcode")
            .join("sessions")
            .join(session)
            .join("provider");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or_else(|| self.current_provider())
    }

    /// Spawn a new AI coding session in tmux
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

        // Ensure workspace is a git repo (Codex requires it)
        let git_dir = workspace.join(".git");
        if !git_dir.exists() {
            let _ = Command::new("git")
                .args(["init"])
                .current_dir(&workspace)
                .output()
                .await;
        }

        // Check if session already exists
        if self.session_exists(&name).await {
            bail!("Session '{}' already exists", name);
        }

        // Create tmux session running claude
        // Claude Code may be at ~/.local/bin/claude (curl installer) or
        // /usr/local/bin/claude (npm installer). Use whichever exists.
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/claude".to_string());
        let home_env = format!("HOME={}", home);
        let provider = self.current_provider();

        // Record provider for this session
        let provider_file = daemon_home_dir()
            .join(".cloudcode")
            .join("sessions")
            .join(&name)
            .join("provider");
        let _ = std::fs::write(&provider_file, provider.as_str());

        // Wrap in a retry loop so the tmux session survives if the provider
        // exits (e.g. OAuth login required). The user can `open` the session,
        // complete OAuth, and the provider restarts automatically.
        let shell_cmd = match provider {
            AiProvider::Claude => {
                let claude_bin = find_claude_bin();
                format!(
                    "while true; do {} --dangerously-skip-permissions --permission-mode bypassPermissions; \
                     echo '\\n[cloudcode] Claude exited. Restarting in 3s... (Ctrl-C to stop)'; \
                     sleep 3; done",
                    claude_bin
                )
            }
            AiProvider::Codex => {
                // Use device-auth for OAuth login on remote VPS (localhost redirect won't work).
                // Check login status first; only prompt if not authenticated.
                "if ! /usr/local/bin/codex login status >/dev/null 2>&1; then \
                   echo '[cloudcode] Codex needs authentication. Starting device auth flow...'; \
                   /usr/local/bin/codex login --device-auth; \
                 fi; \
                 while true; do /usr/local/bin/codex --full-auto --skip-git-repo-check --add-dir /home/claude/.cloudcode/contexts; \
                 echo '\\n[cloudcode] Codex exited. Restarting in 3s... (Ctrl-C to stop)'; \
                 sleep 3; done".to_string()
            }
        };
        // Create session-scoped temp dir
        let session_tmp = daemon_home_dir()
            .join(".cloudcode")
            .join("sessions")
            .join(&name)
            .join("tmp");
        std::fs::create_dir_all(&session_tmp).with_context(|| {
            format!("Failed to create session tmp dir {}", session_tmp.display())
        })?;

        // Create instruction file symlinks in workspace
        for filename in &["CLAUDE.md", "AGENTS.md"] {
            let workspace_file = workspace.join(filename);
            let global_file = daemon_home_dir().join(filename);
            if global_file.exists() && !workspace_file.exists() {
                #[cfg(unix)]
                {
                    let _ = std::os::unix::fs::symlink(&global_file, &workspace_file);
                }
            }
        }

        // Archive stale context file if session name is being reused
        let context_file = daemon_home_dir()
            .join(".cloudcode")
            .join("contexts")
            .join(format!("context_{}.md", name));
        if context_file.exists() {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let bak = context_file.with_extension(format!("{}.bak", ts));
            let _ = std::fs::rename(&context_file, &bak);
            log::info!(
                "Archived stale context file for reused session '{}' to {:?}",
                name,
                bak
            );
        }

        let tmpdir_env = format!("TMPDIR={}", session_tmp.display());
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
                "-e",
                &format!("CLOUDCODE_SESSION_NAME={}", name),
                "-e",
                &tmpdir_env,
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
        let workdir = session_runtime_workdir(session).await?;
        let provider = self.session_provider(session);

        // Ensure workdir is a git repo (Codex requires it)
        if !workdir.join(".git").exists() {
            let _ = Command::new("git")
                .args(["init"])
                .current_dir(&workdir)
                .output()
                .await;
        }

        // Session-scoped temp dir for file detection
        let session_tmp = daemon_home_dir()
            .join(".cloudcode")
            .join("sessions")
            .join(session)
            .join("tmp");
        std::fs::create_dir_all(&session_tmp).with_context(|| {
            format!("Failed to create session tmp dir {}", session_tmp.display())
        })?;

        // Snapshot existing files before running claude
        let watch_dirs = session_snapshot_dirs_from_workdir(&workdir, Some(&session_tmp));
        let files_before = snapshot_files(&watch_dirs);

        // Run provider in print/exec mode for clean text output.
        // Wrap in a 5-minute timeout so a hanging subprocess cannot hold the
        // per-session lock forever.  When the timeout fires the future is
        // dropped, which drops the Child and kills the process.
        const AI_TIMEOUT: Duration = Duration::from_secs(300);

        let output = match provider {
            AiProvider::Claude => {
                let claude_bin = find_claude_bin();
                let fut = Command::new(&claude_bin)
                    .args([
                        "-p",
                        "--dangerously-skip-permissions",
                        "--continue",
                        message,
                    ])
                    .env("CLOUDCODE_SESSION_NAME", session)
                    .env("TMPDIR", &session_tmp)
                    .current_dir(&workdir)
                    .output();
                timeout(AI_TIMEOUT, fut)
                    .await
                    .map_err(|_| anyhow::anyhow!("AI subprocess timed out after 5 minutes"))?
                    .context("Failed to run claude in print mode")?
            }
            AiProvider::Codex => {
                let contexts_dir = daemon_home_dir()
                    .join(".cloudcode")
                    .join("contexts")
                    .to_string_lossy()
                    .to_string();
                let fut = Command::new("/usr/local/bin/codex")
                    .args([
                        "exec",
                        "--full-auto",
                        "--skip-git-repo-check",
                        "--add-dir",
                        &contexts_dir,
                        message,
                    ])
                    .env("CLOUDCODE_SESSION_NAME", session)
                    .env("TMPDIR", &session_tmp)
                    .current_dir(&workdir)
                    .output();
                timeout(AI_TIMEOUT, fut)
                    .await
                    .map_err(|_| anyhow::anyhow!("AI subprocess timed out after 5 minutes"))?
                    .context("Failed to run codex exec")?
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("{} failed: {}", provider, stderr);
        }

        // Log stderr at debug level (Codex streams progress to stderr)
        if !output.stderr.is_empty() {
            log::debug!(
                "{} stderr: {}",
                provider,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let response = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Find new or modified files
        let files_after = snapshot_files(&watch_dirs);
        let new_files: Vec<PathBuf> = files_after
            .into_iter()
            .filter(|(path, (mtime, size))| {
                match files_before.get(path) {
                    None => true,                                                          // new file
                    Some((old_mtime, old_size)) => mtime != old_mtime || size != old_size, // modified
                }
            })
            .map(|(path, _)| path)
            .filter(|f| is_sendable_file(f))
            .collect();

        if !new_files.is_empty() {
            log::info!(
                "Session '{}': detected {} new/modified file(s): {:?}",
                session,
                new_files.len(),
                new_files
            );
        }

        Ok(SendOutput {
            text: response,
            files: new_files,
        })
    }

    /// Capture the current tmux pane content for a session.
    pub async fn capture_pane(&self, session: &str) -> Result<String> {
        validate_session_name(session)?;
        if !self.session_exists(session).await {
            bail!("Session '{}' does not exist", session);
        }

        let output = Command::new("tmux")
            .args(["capture-pane", "-p", "-S", "-50", "-t", session])
            .output()
            .await
            .context("Failed to capture tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux capture-pane failed: {}", stderr);
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let clean = strip_ansi_codes(&raw);
        Ok(clean)
    }

    /// Send keystrokes to a tmux session (literal text + Enter).
    pub async fn send_keys(&self, session: &str, text: &str) -> Result<()> {
        validate_session_name(session)?;
        if !self.session_exists(session).await {
            bail!("Session '{}' does not exist", session);
        }

        // Validate input
        if text.len() > 4096 {
            bail!("Input too long ({} chars, max 4096)", text.len());
        }
        if text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            bail!("Input contains disallowed control characters");
        }

        // Send literal text
        let status = Command::new("tmux")
            .args(["send-keys", "-l", "-t", session, "--", text])
            .status()
            .await
            .context("Failed to send keys to tmux session")?;

        if !status.success() {
            bail!("tmux send-keys failed");
        }

        // Send Enter
        let status = Command::new("tmux")
            .args(["send-keys", "-t", session, "Enter"])
            .status()
            .await
            .context("Failed to send Enter to tmux session")?;

        if !status.success() {
            bail!("tmux send-keys Enter failed");
        }

        Ok(())
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
    fn session_manager_new_has_correct_defaults() {
        let mgr = SessionManager::new(AiProvider::Claude);
        let locks = mgr.send_locks.lock().unwrap();
        assert!(locks.is_empty());
        assert_eq!(mgr.current_provider(), AiProvider::Claude);
    }

    #[test]
    fn session_manager_provider_switch() {
        let mgr = SessionManager::new(AiProvider::Claude);
        assert_eq!(mgr.current_provider(), AiProvider::Claude);
        mgr.set_provider(AiProvider::Codex);
        assert_eq!(mgr.current_provider(), AiProvider::Codex);
    }

    #[test]
    fn ai_provider_from_str() {
        assert_eq!("claude".parse::<AiProvider>().unwrap(), AiProvider::Claude);
        assert_eq!("codex".parse::<AiProvider>().unwrap(), AiProvider::Codex);
        assert_eq!("Claude".parse::<AiProvider>().unwrap(), AiProvider::Claude);
        assert!("unknown".parse::<AiProvider>().is_err());
    }

    #[test]
    fn ai_provider_display() {
        assert_eq!(AiProvider::Claude.to_string(), "claude");
        assert_eq!(AiProvider::Codex.to_string(), "codex");
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

    #[test]
    fn test_scan_exclude_dirs_contains_expected() {
        assert!(SCAN_EXCLUDE_DIRS.contains(&".git"));
        assert!(SCAN_EXCLUDE_DIRS.contains(&"node_modules"));
        assert!(SCAN_EXCLUDE_DIRS.contains(&"__pycache__"));
        assert!(SCAN_EXCLUDE_DIRS.contains(&"target"));
    }

    #[test]
    fn test_walk_dir_skips_excluded_dirs() {
        // Create a temp dir with an excluded subdirectory
        let tmp = std::env::temp_dir().join("cloudcode_test_walk_dir_exclude");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("node_modules")).unwrap();
        std::fs::create_dir_all(tmp.join("output")).unwrap();
        std::fs::write(tmp.join("node_modules").join("pkg.json"), "{}").unwrap();
        std::fs::write(tmp.join("output").join("result.txt"), "ok").unwrap();
        std::fs::write(tmp.join("top.txt"), "top").unwrap();

        let mut files = HashMap::new();
        walk_dir(&tmp, 3, &mut files);

        // Should find top.txt and output/result.txt but NOT node_modules/pkg.json
        let paths: Vec<String> = files
            .keys()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert!(paths.contains(&"top.txt".to_string()));
        assert!(paths.contains(&"result.txt".to_string()));
        assert!(!paths.contains(&"pkg.json".to_string()));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // -----------------------------------------------------------------------
    // strip_ansi_codes tests
    // -----------------------------------------------------------------------

    #[test]
    fn strip_ansi_codes_empty_string() {
        assert_eq!(strip_ansi_codes(""), "");
    }

    #[test]
    fn strip_ansi_codes_plain_text_unchanged() {
        let input = "Hello, world! 123 #$%";
        assert_eq!(strip_ansi_codes(input), input);
    }

    #[test]
    fn strip_ansi_codes_basic_color_red() {
        assert_eq!(strip_ansi_codes("\x1b[31mred text\x1b[0m"), "red text");
    }

    #[test]
    fn strip_ansi_codes_reset() {
        assert_eq!(strip_ansi_codes("\x1b[0m"), "");
    }

    #[test]
    fn strip_ansi_codes_256_color() {
        // 256-color foreground: ESC[38;5;196m
        assert_eq!(
            strip_ansi_codes("\x1b[38;5;196mcolored\x1b[0m"),
            "colored"
        );
    }

    #[test]
    fn strip_ansi_codes_bold_and_underline() {
        assert_eq!(strip_ansi_codes("\x1b[1mbold\x1b[0m"), "bold");
        assert_eq!(strip_ansi_codes("\x1b[4munderline\x1b[0m"), "underline");
    }

    #[test]
    fn strip_ansi_codes_cursor_movement_clear_screen() {
        // ESC[2J clears the screen
        assert_eq!(strip_ansi_codes("\x1b[2J"), "");
        // ESC[H moves cursor to home
        assert_eq!(strip_ansi_codes("\x1b[H"), "");
        // ESC[10;20H moves cursor to row 10, col 20
        assert_eq!(strip_ansi_codes("before\x1b[10;20Hafter"), "beforeafter");
    }

    #[test]
    fn strip_ansi_codes_mixed_text_and_codes() {
        assert_eq!(
            strip_ansi_codes("Hello \x1b[31mworld\x1b[0m!"),
            "Hello world!"
        );
    }

    #[test]
    fn strip_ansi_codes_multiple_codes_in_sequence() {
        // Bold + red + text + reset
        assert_eq!(
            strip_ansi_codes("\x1b[1m\x1b[31mhello\x1b[0m"),
            "hello"
        );
    }

    #[test]
    fn strip_ansi_codes_preserves_newlines_and_whitespace() {
        let input = "\x1b[32mline1\x1b[0m\nline2\n  \x1b[33mline3\x1b[0m";
        assert_eq!(strip_ansi_codes(input), "line1\nline2\n  line3");
    }

    #[test]
    fn strip_ansi_codes_combined_sgr_parameters() {
        // ESC[1;31;4m = bold + red + underline
        assert_eq!(
            strip_ansi_codes("\x1b[1;31;4mstyled\x1b[0m"),
            "styled"
        );
    }

    #[test]
    fn strip_ansi_codes_only_codes_no_text() {
        assert_eq!(strip_ansi_codes("\x1b[31m\x1b[0m\x1b[2J"), "");
    }
}
