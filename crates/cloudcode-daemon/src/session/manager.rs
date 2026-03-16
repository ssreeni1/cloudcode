use anyhow::{bail, Context, Result};
use cloudcode_common::session::{SessionInfo, SessionState};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
            | "pdf" | "md" | "txt" | "json" | "csv"
            | "html" | "log"
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

pub struct SessionManager {
    output_counter: AtomicU64,
    send_locks: std::sync::Mutex<HashMap<String, Arc<Mutex<()>>>>,
    output_dir: PathBuf,
}

fn strip_ansi(input: &str) -> String {
    // Match all common terminal escape sequences:
    // - CSI sequences: \x1b[ ... (letter)  — including \x1b[?... variants
    // - OSC sequences: \x1b] ... \x07
    // - Character set: \x1b(B
    // - SGR (color): \x1b[ ... m
    let re = Regex::new(
        r"(?x)
        \x1b \[ [?]? [0-9;]* [a-zA-Z]  |  # CSI sequences (including ?2026h etc)
        \x1b \] .*? \x07                |  # OSC sequences
        \x1b \( B                        |  # Character set selection
        \x1b \[ [0-9;]* m               |  # SGR (color/style)
        \x1b [78]                        |  # Save/restore cursor
        \x1b =                           |  # Set keypad mode
        \r                                  # Carriage returns
        "
    ).unwrap();
    let stripped = re.replace_all(input, "").to_string();

    // Filter out lines that are purely decorative (spinners, box drawing, progress)
    stripped
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            // Skip empty lines only if they'd create excessive gaps
            if trimmed.is_empty() {
                return true;
            }
            // Skip lines that are only box-drawing chars, spinners, or decorative
            let decorative_only = trimmed.chars().all(|c| {
                matches!(c,
                    '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' |
                    '━' | '┃' | '┏' | '┓' | '┗' | '┛' | '┣' | '┫' | '┳' | '┻' | '╋' |
                    '═' | '║' | '╔' | '╗' | '╚' | '╝' | '╠' | '╣' | '╦' | '╩' | '╬' |
                    '◐' | '◑' | '◒' | '◓' | '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏' |
                    '✶' | '✻' | '✽' | '✢' | '●' | '·' | '…' |
                    ' '
                )
            });
            !decorative_only
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Check if a line looks like Claude Code's input prompt.
/// Claude Code shows various prompt indicators - we look for common patterns
/// at the end of visible output that indicate it's waiting for input.
fn is_prompt_line(line: &str) -> bool {
    let stripped = strip_ansi(line);
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Claude Code prompt patterns:
    // - Ends with ">" or "❯" (common prompt chars)
    // - Ends with "$" (shell prompt if claude exited)
    // - Contains "(/help" which appears in Claude's prompt hint
    trimmed.ends_with('>')
        || trimmed.ends_with('❯')
        || trimmed.ends_with('$')
        || trimmed.contains("/help")
}

/// Check if a line is Claude Code TUI chrome (not actual response content)
fn is_ui_noise(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false; // Keep empty lines (collapsed later)
    }
    // Box drawing borders
    if t.chars().all(|c| matches!(c, '╭' | '╮' | '╰' | '╯' | '│' | '─' | '┌' | '┐' | '└' | '┘' | '├' | '┤'
        | '┬' | '┴' | '┼' | '━' | '┃' | '┏' | '┓' | '┗' | '┛' | '═' | '║' | '╔' | '╗' | '╚' | '╝'
        | '╠' | '╣' | '╦' | '╩' | '╬' | '◐' | '◑' | '◒' | '◓' | '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴'
        | '⠦' | '⠧' | '⠇' | '⠏' | '✶' | '✻' | '✽' | '✢' | '●' | '·' | '…' | ' ' | '▐' | '▛' | '▜'
        | '▌' | '▝' | '▘' | '█')) {
        return true;
    }
    // Claude Code welcome/chrome patterns
    t.starts_with("╭") || t.starts_with("╰") || t.starts_with("│")
        || t.contains("Claude Code v")
        || t.contains("Welcome back")
        || t.contains("Tips for getting started")
        || t.contains("/init to create")
        || t.contains("CLAUDE.md")
        || t.contains("launched claude in your home")
        || t.contains("project directory instead")
        || t.contains("Recent activity")
        || t.contains("No recent activity")
        || t.contains("Opus 4.6")
        || t.contains("Claude Max")
        || t.contains("Organization")
        || t.contains("1M context")
        || t.contains("more room, same pricing")
        || t.contains("bypass permissions")
        || t.contains("shift+tab to cycle")
        || t.contains("esc to")
        || t.contains("to interrupt")
        || t.contains("/effort")
        || t.contains("for shortcuts")
        || t.contains("? for shortcuts")
        || t.contains("Enter to confirm")
        || t.contains("Esc to cancel")
        || t.contains("Security guide")
        || t.contains("trust this folder")
        || t.contains("[>0q")
        || (t.starts_with("↑") && t.len() < 5)
        || t == "❯"
        || t == ">"
}

fn extract_response(output: &str, sent_message: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();

    // Find where the echoed input ends and response begins
    let mut start_idx = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.contains(sent_message) || line.trim() == sent_message.trim() {
            start_idx = i + 1;
            break;
        }
    }

    // Find where the response ends (last prompt line)
    let mut end_idx = lines.len();
    for i in (start_idx..lines.len()).rev() {
        let trimmed = lines[i].trim();
        if !trimmed.is_empty() {
            if is_prompt_line(lines[i]) {
                end_idx = i;
            }
            break;
        }
    }

    // Filter out all UI noise
    let content_lines: Vec<&str> = lines[start_idx..end_idx]
        .iter()
        .filter(|line| !is_ui_noise(line))
        .copied()
        .collect();

    // Collapse multiple blank lines into one
    let mut result = String::new();
    let mut prev_blank = false;
    for line in &content_lines {
        if line.trim().is_empty() {
            if !prev_blank {
                result.push('\n');
            }
            prev_blank = true;
        } else {
            if prev_blank && !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
            result.push('\n');
            prev_blank = false;
        }
    }

    result.trim().to_string()
}

impl SessionManager {
    pub fn new() -> Self {
        let output_dir = PathBuf::from("/home/claude/.cloudcode/output");

        // Create output directory with mode 0700
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let _ = std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&output_dir);
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::create_dir_all(&output_dir);
        }

        Self {
            output_counter: AtomicU64::new(0),
            send_locks: std::sync::Mutex::new(HashMap::new()),
            output_dir,
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
            "{} --dangerously-skip-permissions --permission-mode bypassPermissions",
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
                "-e",
                &home_env,
                &shell_cmd,
            ])
            .status()
            .await
            .context("Failed to start tmux session")?;

        if !status.success() {
            bail!("tmux new-session failed with status {}", status);
        }

        // Auto-accept the workspace trust prompt by sending Enter after a short delay
        let accept_name = name.clone();
        tokio::spawn(async move {
            // Wait for Claude Code to show the trust prompt
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            let _ = Command::new("tmux")
                .args(["send-keys", "-t", &accept_name, "Enter"])
                .status()
                .await;
        });

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
                        let created_at =
                            parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                        let last_activity =
                            parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
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

        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/claude".to_string());
        let claude_bin = format!("{}/.local/bin/claude", home);

        // Snapshot existing files before running claude
        let watch_dirs = vec![
            PathBuf::from(&home),
            PathBuf::from(format!("{}/screenshots", home)),
            PathBuf::from(format!("{}/output", home)),
            PathBuf::from("/tmp"),
        ];
        let files_before = snapshot_files(&watch_dirs);

        // Use claude -p (print mode) for clean text output.
        let output = Command::new(&claude_bin)
            .args([
                "-p",
                "--dangerously-skip-permissions",
                "--continue",
                message,
            ])
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

    /// Capture current pane content
    pub async fn capture(&self, session: &str) -> Result<String> {
        let output = Command::new("tmux")
            .args(["capture-pane", "-t", session, "-p", "-J", "-S", "-100"])
            .output()
            .await
            .context("Failed to capture pane")?;

        if !output.status.success() {
            bail!("tmux capture-pane failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn start_output_capture(&self, session: &str) -> Result<PathBuf> {
        let id = self.output_counter.fetch_add(1, Ordering::Relaxed);
        let path = self.output_dir.join(format!("{}-{}.log", session, id));

        // Pre-create output file with mode 0600
        let _ = tokio::fs::remove_file(&path).await;
        {
            #[cfg(unix)]
            {
                use std::fs::OpenOptions;
                use std::os::unix::fs::OpenOptionsExt;
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&path)
                    .context("Failed to create output capture file")?;
            }
            #[cfg(not(unix))]
            {
                tokio::fs::write(&path, b"").await?;
            }
        }

        // Start piping pane output to file
        let status = Command::new("tmux")
            .args([
                "pipe-pane",
                "-o",
                "-t",
                session,
                &format!("cat >> {}", path.display()),
            ])
            .status()
            .await
            .context("Failed to start pipe-pane")?;

        if !status.success() {
            bail!("tmux pipe-pane failed");
        }
        Ok(path)
    }

    async fn stop_output_capture(&self, session: &str) -> Result<()> {
        let _ = Command::new("tmux")
            .args(["pipe-pane", "-t", session])
            .status()
            .await;
        Ok(())
    }

    async fn capture_last_lines(&self, session: &str, n: u32) -> Result<String> {
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-p",
                "-J",
                "-t",
                session,
                "-S",
                &format!("-{}", n),
            ])
            .output()
            .await
            .context("Failed to capture pane")?;

        if !output.status.success() {
            bail!("tmux capture-pane failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Wait for output to stabilize (Claude finished responding)
    async fn wait_for_output(&self, session: &str) -> Result<()> {
        let mut last_capture = String::new();
        let mut stable_count = 0u32;
        let start = std::time::Instant::now();
        let max_duration = std::time::Duration::from_secs(170); // under client's 180s timeout

        // Initial delay to let Claude start processing
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        loop {
            let elapsed = start.elapsed();
            if elapsed > max_duration {
                break; // Timeout - return whatever we have
            }

            // Adaptive poll interval
            let interval = if elapsed.as_secs() < 2 {
                100 // 100ms for first 2 seconds (fast responses)
            } else if elapsed.as_secs() < 10 {
                250 // 250ms for 2-10 seconds
            } else {
                500 // 500ms after 10 seconds
            };

            tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;

            let capture = self.capture_last_lines(session, 5).await?;
            let clean = strip_ansi(&capture);

            // Check for prompt in last non-empty line
            if let Some(last_line) = clean.lines().rev().find(|l| !l.trim().is_empty()) {
                if is_prompt_line(last_line) {
                    // Confirm stability - wait one more interval
                    tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
                    let confirm = self.capture_last_lines(session, 5).await?;
                    let confirm_clean = strip_ansi(&confirm);
                    if let Some(confirm_line) =
                        confirm_clean.lines().rev().find(|l| !l.trim().is_empty())
                    {
                        if is_prompt_line(confirm_line) {
                            break; // Prompt confirmed stable
                        }
                    }
                }
            }

            // Fallback: stabilization check (output unchanged)
            if clean == last_capture {
                stable_count += 1;
                let threshold = if elapsed.as_secs() < 5 { 8 } else { 5 };
                if stable_count >= threshold {
                    break;
                }
            } else {
                stable_count = 0;
                last_capture = clean;
            }
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

    #[test]
    fn test_strip_ansi_removes_color_codes() {
        assert_eq!(strip_ansi("\x1b[32mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("\x1b[1;31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("no codes here"), "no codes here");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_strip_ansi_removes_cursor_codes() {
        assert_eq!(strip_ansi("\x1b[2Jhello"), "hello");
        assert_eq!(strip_ansi("\x1b[Hhello"), "hello");
    }

    #[test]
    fn test_is_prompt_line_detects_prompts() {
        assert!(is_prompt_line(">"));
        assert!(is_prompt_line("❯"));
        assert!(is_prompt_line("$ "));
        assert!(is_prompt_line("Type /help for help"));
        assert!(is_prompt_line("\x1b[32m>\x1b[0m")); // colored prompt
    }

    #[test]
    fn test_is_prompt_line_rejects_non_prompts() {
        assert!(!is_prompt_line(""));
        assert!(!is_prompt_line("   "));
        assert!(!is_prompt_line("Hello, how can I help?"));
        assert!(!is_prompt_line("The answer is 42"));
    }

    #[test]
    fn test_extract_response_basic() {
        let output = "hello\nThe answer is 42.\n>";
        let result = extract_response(output, "hello");
        assert_eq!(result, "The answer is 42.");
    }

    #[test]
    fn test_extract_response_multiline() {
        let output =
            "what is rust\nRust is a systems programming language.\nIt focuses on safety and performance.\n>";
        let result = extract_response(output, "what is rust");
        assert_eq!(
            result,
            "Rust is a systems programming language.\nIt focuses on safety and performance."
        );
    }

    #[test]
    fn test_extract_response_no_prompt() {
        let output = "hello\nworld";
        let result = extract_response(output, "hello");
        assert_eq!(result, "world");
    }

    #[test]
    fn test_extract_response_empty() {
        let output = "hello\n>";
        let result = extract_response(output, "hello");
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_response_message_not_found() {
        let output = "some output\nmore output\n>";
        let result = extract_response(output, "not-in-output");
        assert_eq!(result, "some output\nmore output");
    }
}
