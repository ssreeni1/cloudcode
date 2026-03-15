use cloudcode_common::protocol::{DaemonRequest, DaemonResponse};
use crate::session::manager::SessionManager;
use std::time::{SystemTime, UNIX_EPOCH};

static START_TIME: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

pub fn init_start_time() {
    START_TIME.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });
}

pub async fn handle(request: DaemonRequest, mgr: &SessionManager) -> DaemonResponse {
    match request {
        DaemonRequest::Spawn { name } => match mgr.spawn(name).await {
            Ok(session) => DaemonResponse::Spawned { session },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::List => match mgr.list().await {
            Ok(sessions) => DaemonResponse::Sessions { sessions },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::Kill { session } => match mgr.kill(&session).await {
            Ok(()) => DaemonResponse::Killed { session },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::Send { session, message } => match mgr.send(&session, &message).await {
            Ok(output) => DaemonResponse::SendResult { output },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },
        DaemonRequest::Status => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let start = *START_TIME.get_or_init(|| now);
            let uptime_secs = now - start;
            match mgr.list().await {
                Ok(sessions) => DaemonResponse::Status {
                    uptime_secs,
                    sessions,
                },
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }
    }
}
