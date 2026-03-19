use super::manager::SessionManager;
use anyhow::Result;
use cloudcode_common::session::{SessionInfo, SessionState};
use std::sync::Arc;
use tokio::process::Command;

pub struct SessionMonitor {
    manager: Arc<SessionManager>,
}

impl SessionMonitor {
    pub fn new(manager: Arc<SessionManager>) -> Self {
        Self { manager }
    }

    /// Check which sessions are alive vs dead by querying tmux
    pub async fn check_health(&self) -> Result<Vec<SessionInfo>> {
        let sessions = self.manager.list().await?;
        let mut result = Vec::new();

        for mut session in sessions {
            // Check if the session's window has an active process
            let output = Command::new("tmux")
                .args(["list-panes", "-t", &session.name, "-F", "#{pane_dead}"])
                .output()
                .await;

            match output {
                Ok(out) if out.status.success() => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if stdout.trim() == "1" {
                        session.state = SessionState::Dead;
                    }
                }
                _ => {
                    session.state = SessionState::Dead;
                }
            }
            result.push(session);
        }

        Ok(result)
    }

    /// Kill all dead sessions
    pub async fn cleanup_dead(&self) -> Result<Vec<String>> {
        let sessions = self.check_health().await?;
        let mut cleaned = Vec::new();

        for session in sessions {
            if session.state == SessionState::Dead {
                // Clean orphaned output files for dead sessions
                let _ = tokio::process::Command::new("sh")
                    .args([
                        "-c",
                        &format!("rm -f /tmp/cloudcode-{}-*.log", session.name),
                    ])
                    .status()
                    .await;
                if self.manager.kill(&session.name).await.is_ok() {
                    cleaned.push(session.name);
                }
            }
        }

        Ok(cleaned)
    }
}
