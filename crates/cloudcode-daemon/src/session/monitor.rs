use anyhow::Result;
use super::manager::SessionManager;
use cloudcode_common::session::SessionInfo;

pub struct SessionMonitor {
    manager: SessionManager,
}

impl SessionMonitor {
    pub fn new(manager: SessionManager) -> Self {
        Self { manager }
    }

    /// Reconcile session states — detect dead sessions
    pub async fn reconcile(&self) -> Result<Vec<SessionInfo>> {
        self.manager.list().await
    }
}
