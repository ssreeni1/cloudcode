use anyhow::{bail, Context, Result};
use cloudcode_common::session::{SessionInfo, SessionState};
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::sync::Mutex;

pub struct SessionManager {
    output_counter: AtomicU64,
    send_locks: std::sync::Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

fn strip_ansi(input: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\].*?\x07|\x1b\(B|\x1b\[[0-9;]*m").unwrap();
    re.replace_all(input, "").to_string()
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

    // Join the response lines
    let response: String = lines[start_idx..end_idx].join("\n");

    response.trim().to_string()
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            output_counter: AtomicU64::new(0),
            send_locks: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Spawn a new Claude Code session in tmux
    pub async fn spawn(&self, name: Option<String>) -> Result<SessionInfo> {
        let name = name.unwrap_or_else(|| {
            format!(
                "session-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    % 10000
            )
        });

        // Check if session already exists
        if self.session_exists(&name).await {
            bail!("Session '{}' already exists", name);
        }

        // Create tmux session running claude
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
                "claude",
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

    /// Send keys to a session and capture output
    pub async fn send(&self, session: &str, message: &str) -> Result<String> {
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

        // Start capturing output
        let output_path = self.start_output_capture(session).await?;

        // Send the message
        let status = Command::new("tmux")
            .args(["send-keys", "-t", session, message, "Enter"])
            .status()
            .await
            .context("Failed to send keys")?;

        if !status.success() {
            self.stop_output_capture(session).await?;
            let _ = tokio::fs::remove_file(&output_path).await;
            bail!("tmux send-keys failed");
        }

        // Wait for response to complete
        self.wait_for_output(session).await?;

        // Stop capturing and read output
        self.stop_output_capture(session).await?;
        let raw_output = tokio::fs::read_to_string(&output_path)
            .await
            .unwrap_or_default();
        let _ = tokio::fs::remove_file(&output_path).await;

        // Post-process: strip ANSI, remove echoed input, trim
        let clean = strip_ansi(&raw_output);
        let response = extract_response(&clean, message);

        Ok(response)
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
        let path = PathBuf::from(format!("/tmp/cloudcode-{}-{}.log", session, id));

        // Ensure clean file
        let _ = tokio::fs::remove_file(&path).await;
        tokio::fs::write(&path, b"").await?;

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
