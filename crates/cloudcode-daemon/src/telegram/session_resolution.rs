use std::sync::Arc;

use anyhow::Result;

use super::bot::BotState;
use super::question_poller::SessionQuestionState;

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

pub fn waiting_sessions(state: &Arc<BotState>) -> Vec<(String, String)> {
    let states = state.question_states.lock().unwrap();
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

pub fn clear_waiting_state(state: &Arc<BotState>, session: &str) {
    let mut states = state.question_states.lock().unwrap();
    states.insert(session.to_string(), SessionQuestionState::Idle);
}

pub async fn session_exists(state: &Arc<BotState>, name: &str) -> Result<bool> {
    let sessions = state.session_mgr.list().await?;
    Ok(sessions.iter().any(|session| session.name == name))
}

pub async fn resolve_command_session(
    state: &Arc<BotState>,
    explicit: Option<&str>,
) -> Result<Option<String>> {
    if let Some(name) = explicit.filter(|name| !name.trim().is_empty()) {
        return Ok(Some(name.to_string()));
    }

    match state.default_session.current() {
        Some(name) => {
            let sessions = state.session_mgr.list().await?;
            if sessions.iter().any(|session| session.name == name) {
                Ok(Some(name))
            } else {
                if let Err(err) = state.default_session.clear() {
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

pub async fn resolve_free_text_session(state: &Arc<BotState>) -> Result<FreeTextSessionTarget> {
    if let Some(name) = resolve_command_session(state, None).await? {
        return Ok(FreeTextSessionTarget::Selected {
            name,
            auto_selected: false,
        });
    }

    let sessions = state.session_mgr.list().await?;
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

pub async fn resolve_reply_target(state: &Arc<BotState>, args: &str) -> Result<ReplyTarget> {
    let waiting = waiting_sessions(state);
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

pub async fn resolve_type_target(
    state: &Arc<BotState>,
    args: &str,
) -> Result<Option<(String, String)>> {
    if args.trim().is_empty() {
        return Ok(None);
    }

    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() == 2 && session_exists(state, parts[0]).await? {
        return Ok(Some((parts[0].to_string(), parts[1].to_string())));
    }

    Ok(resolve_command_session(state, None)
        .await?
        .map(|name| (name, args.to_string())))
}
