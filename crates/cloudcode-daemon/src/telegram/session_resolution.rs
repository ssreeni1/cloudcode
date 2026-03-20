use std::sync::Arc;

use anyhow::Result;

use super::default_session::DefaultSessionStore;
use super::question_poller::{QuestionStates, SessionQuestionState};
use crate::session::manager::SessionManager;

pub enum FreeTextSessionTarget {
    Selected { name: String, auto_selected: bool },
    NoSessions,
    MultipleSessions(Vec<String>),
}

pub enum ReplyTarget {
    NoneWaiting,
    Ready {
        session_name: String,
        reply_text: String,
    },
    Ambiguous(Vec<(String, String)>),
}

// ---------------------------------------------------------------------------
// Decoupled helpers (used by dispatch logic — no BotState dependency)
// ---------------------------------------------------------------------------

pub fn waiting_sessions_from(states: &QuestionStates) -> Vec<(String, String)> {
    let states = states.lock().unwrap();
    states
        .iter()
        .filter_map(|(name, session_state)| {
            if let SessionQuestionState::WaitingForInput { question, .. } = session_state {
                Some((name.clone(), question.clone()))
            } else {
                None
            }
        })
        .collect()
}

pub fn clear_waiting_state_from(states: &QuestionStates, session: &str) {
    let mut states = states.lock().unwrap();
    states.insert(session.to_string(), SessionQuestionState::Idle);
}

pub async fn session_exists_with(mgr: &Arc<SessionManager>, name: &str) -> Result<bool> {
    let sessions = mgr.list().await?;
    Ok(sessions.iter().any(|session| session.name == name))
}

pub async fn resolve_command_session_with(
    mgr: &Arc<SessionManager>,
    default_session: &Arc<DefaultSessionStore>,
    explicit: Option<&str>,
) -> Result<Option<String>> {
    if let Some(name) = explicit.filter(|name| !name.trim().is_empty()) {
        return Ok(Some(name.to_string()));
    }

    match default_session.current() {
        Some(name) => {
            let sessions = mgr.list().await?;
            if sessions.iter().any(|session| session.name == name) {
                Ok(Some(name))
            } else {
                if let Err(err) = default_session.clear() {
                    log::warn!(
                        "Failed to clear stale persisted Telegram default session: {}",
                        err
                    );
                }
                Ok(None)
            }
        }
        None => Ok(None),
    }
}

pub async fn resolve_free_text_session_with(
    mgr: &Arc<SessionManager>,
    default_session: &Arc<DefaultSessionStore>,
) -> Result<FreeTextSessionTarget> {
    if let Some(name) = resolve_command_session_with(mgr, default_session, None).await? {
        return Ok(FreeTextSessionTarget::Selected {
            name,
            auto_selected: false,
        });
    }

    let sessions = mgr.list().await?;
    match sessions.len() {
        0 => Ok(FreeTextSessionTarget::NoSessions),
        1 => {
            let name = sessions[0].name.clone();
            Ok(FreeTextSessionTarget::Selected {
                name,
                auto_selected: true,
            })
        }
        _ => Ok(FreeTextSessionTarget::MultipleSessions(
            sessions.into_iter().map(|session| session.name).collect(),
        )),
    }
}

pub async fn resolve_reply_target_with(
    question_states: &QuestionStates,
    args: &str,
) -> Result<ReplyTarget> {
    let waiting = waiting_sessions_from(question_states);
    if waiting.is_empty() {
        return Ok(ReplyTarget::NoneWaiting);
    }

    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let target = if parts.len() == 2 && waiting.iter().any(|(name, _)| name == parts[0]) {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else if waiting.len() == 1 {
        Some((waiting[0].0.clone(), args.to_string()))
    } else {
        None
    };

    match target {
        Some((session_name, reply_text)) if !reply_text.trim().is_empty() => {
            Ok(ReplyTarget::Ready {
                session_name,
                reply_text,
            })
        }
        _ => Ok(ReplyTarget::Ambiguous(waiting)),
    }
}

pub async fn resolve_type_target_with(
    mgr: &Arc<SessionManager>,
    default_session: &Arc<DefaultSessionStore>,
    args: &str,
) -> Result<Option<(String, String)>> {
    if args.trim().is_empty() {
        return Ok(None);
    }

    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() == 2 && session_exists_with(mgr, parts[0]).await? {
        return Ok(Some((parts[0].to_string(), parts[1].to_string())));
    }

    Ok(resolve_command_session_with(mgr, default_session, None)
        .await?
        .map(|name| (name, args.to_string())))
}
