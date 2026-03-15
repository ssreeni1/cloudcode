use anyhow::{bail, Context, Result};
use cloudcode_common::session::{SessionInfo, SessionState};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;

pub struct SessionManager;

impl SessionManager {
    pub fn new() -> Self {
        Self
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

        // Send the message via tmux send-keys
        let status = Command::new("tmux")
            .args(["send-keys", "-t", session, message, "Enter"])
            .status()
            .await
            .context("Failed to send keys")?;

        if !status.success() {
            bail!("tmux send-keys failed");
        }

        // Wait for Claude to process (poll capture-pane until output stabilizes)
        let output = self.wait_for_output(session).await?;
        Ok(output)
    }

    /// Capture current pane content
    pub async fn capture(&self, session: &str) -> Result<String> {
        let output = Command::new("tmux")
            .args(["capture-pane", "-t", session, "-p", "-S", "-100"])
            .output()
            .await
            .context("Failed to capture pane")?;

        if !output.status.success() {
            bail!("tmux capture-pane failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Wait for output to stabilize (Claude finished responding)
    async fn wait_for_output(&self, session: &str) -> Result<String> {
        // Capture the initial state before we sent the message
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let mut last_output = self.capture(session).await?;
        let mut stable_count = 0;
        let max_iterations = 240; // 120 seconds max at 500ms intervals

        for i in 0..max_iterations {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let current = self.capture(session).await?;

            if current == last_output {
                stable_count += 1;
                // More patience early on (Claude might be thinking)
                // After initial wait, consider stable after 3 consecutive matches
                let threshold = if i < 10 { 6 } else { 3 };
                if stable_count >= threshold {
                    return Ok(current);
                }
            } else {
                stable_count = 0;
                last_output = current;
            }
        }

        // Return whatever we have after timeout
        Ok(last_output)
    }

    async fn session_exists(&self, name: &str) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", name])
            .status()
            .await
            .is_ok_and(|s| s.success())
    }
}
