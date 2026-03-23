use std::path::PathBuf;
use std::sync::Arc;

use cloudcode_common::provider::AiProvider;

use crate::session::manager::SessionManager;
use crate::telegram::default_session::DefaultSessionStore;
use crate::telegram::question_poller::QuestionStates;
use crate::telegram::session_resolution::{
    FreeTextSessionTarget, ReplyTarget, clear_waiting_state_from, resolve_command_session_with,
    resolve_free_text_session_with, resolve_reply_target_with, resolve_type_target_with,
    session_exists_with, waiting_sessions_from,
};

// ---------------------------------------------------------------------------
// Dispatch result types (no Bot/Message dependency)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageFormat {
    Plain,
    Html,
    Markdown,
    Preformatted,
}

#[derive(Debug, Clone)]
pub struct DispatchMessage {
    pub text: String,
    pub format: MessageFormat,
}

#[derive(Debug, Clone, Default)]
pub struct DispatchResult {
    pub messages: Vec<DispatchMessage>,
    pub files: Vec<PathBuf>,
}

impl DispatchResult {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            messages: vec![DispatchMessage {
                text: text.into(),
                format: MessageFormat::Plain,
            }],
            files: vec![],
        }
    }

    pub fn markdown(text: impl Into<String>) -> Self {
        Self {
            messages: vec![DispatchMessage {
                text: text.into(),
                format: MessageFormat::Markdown,
            }],
            files: vec![],
        }
    }

    #[allow(dead_code)]
    pub fn html(text: impl Into<String>) -> Self {
        Self {
            messages: vec![DispatchMessage {
                text: text.into(),
                format: MessageFormat::Html,
            }],
            files: vec![],
        }
    }

    pub fn preformatted(text: impl Into<String>) -> Self {
        Self {
            messages: vec![DispatchMessage {
                text: text.into(),
                format: MessageFormat::Preformatted,
            }],
            files: vec![],
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::plain(format!("❌ {}", text.into()))
    }

    pub fn push_plain(&mut self, text: impl Into<String>) {
        self.messages.push(DispatchMessage {
            text: text.into(),
            format: MessageFormat::Plain,
        });
    }
}

// ---------------------------------------------------------------------------
// DaemonState — shared state bundle for dispatch logic
// ---------------------------------------------------------------------------

pub struct DaemonState {
    pub session_mgr: Arc<SessionManager>,
    pub default_session: Arc<DefaultSessionStore>,
    pub question_states: QuestionStates,
}

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

const HELP_TEXT: &str = "🤖 cloudcode Telegram Bot\n\n\
    /spawn [name] — Create a new session\n\
    /list — List active sessions\n\
    /kill <name> — Kill a session\n\
    /use <name> — Set default session\n\
    /provider [claude|codex] — Check or switch AI provider\n\
    /waiting — List sessions waiting for input\n\
    /reply [session] <text> — Reply to a waiting session\n\
    /context [session] — View session context\n\
    /peek [session] — View raw tmux pane\n\
    /type [session] <text> — Type into tmux session\n\
    /status — Show daemon status\n\n\
    Waiting prompts are routed with /reply, not ordinary chat text.\n\
    Send any text to interact with the default session.";

// ---------------------------------------------------------------------------
// Command routing
// ---------------------------------------------------------------------------

pub async fn route_command(state: &DaemonState, text: &str) -> DispatchResult {
    let parts: Vec<&str> = text.splitn(2, ' ').collect();
    let command = parts[0].to_lowercase();
    let args = parts.get(1).map(|v| v.trim()).unwrap_or("");

    match command.as_str() {
        "/start" | "/help" => help_logic(state).await,
        "/spawn" => spawn_logic(state, args).await,
        "/list" => list_logic(state).await,
        "/kill" => kill_logic(state, args).await,
        "/use" => use_logic(state, args).await,
        "/status" => status_logic(state).await,
        "/provider" => provider_logic(state, args).await,
        "/waiting" => waiting_logic(state).await,
        "/reply" => reply_logic(state, args).await,
        "/context" => context_logic(state, args).await,
        "/peek" => peek_logic(state, args).await,
        "/type" => type_logic(state, args).await,
        _ => DispatchResult::plain("Unknown command. Send /help for available commands."),
    }
}

// ---------------------------------------------------------------------------
// Handler logic functions (no Bot/Message dependency)
// ---------------------------------------------------------------------------

pub async fn help_logic(state: &DaemonState) -> DispatchResult {
    let mut result = DispatchResult {
        messages: vec![DispatchMessage {
            text: HELP_TEXT.to_string(),
            format: MessageFormat::Plain,
        }],
        files: vec![],
    };

    if let Ok(sessions) = state.session_mgr.list().await {
        if !sessions.is_empty() {
            let default = state.default_session.current();
            let mut session_list = String::from("📋 Existing sessions:\n");
            for session in &sessions {
                let marker = if default.as_deref() == Some(&session.name) {
                    " ← default"
                } else {
                    ""
                };
                session_list.push_str(&format!("• {}{}\n", session.name, marker));
            }
            session_list.push_str("\nUse /use <name> to set a default session.");
            result.push_plain(session_list);
        }
    }

    result
}

pub async fn spawn_logic(state: &DaemonState, args: &str) -> DispatchResult {
    let name = if args.is_empty() {
        None
    } else {
        Some(args.to_string())
    };

    match state.session_mgr.spawn(name).await {
        Ok(session) => {
            if let Err(err) = state.default_session.set(Some(session.name.clone())) {
                return DispatchResult::plain(format!(
                    "⚠️ Session '{}' created, but failed to persist default session: {}",
                    session.name, err
                ));
            }

            let provider = state.session_mgr.current_provider();
            let has_auth = super::handlers::provider_has_auth(provider);

            if has_auth {
                DispatchResult::plain(format!(
                    "✅ Session '{}' created and set as default. Send any message to start chatting.",
                    session.name
                ))
            } else {
                let auth_hint = match provider {
                    AiProvider::Claude => {
                        "Complete login from your terminal:\n\
                         1. cloudcode open {session}\n\
                         2. Copy the login URL and paste in your browser\n\
                         3. Complete the OAuth flow"
                    }
                    AiProvider::Codex => {
                        "Complete login from your terminal:\n\
                         1. cloudcode open {session}\n\
                         2. Select 'Device code' when prompted\n\
                         3. Visit the URL in your browser to authorize"
                    }
                };
                let hint = auth_hint.replace("{session}", &session.name);
                DispatchResult::plain(format!(
                    "⚠️ Session '{}' created, but {} needs authentication first.\n\n{}\n\nTelegram will work once login is complete.",
                    session.name, provider, hint
                ))
            }
        }
        Err(err) => {
            let provider = state.session_mgr.current_provider();
            if super::handlers::is_auth_error(&err, provider) {
                let auth_hint = match provider {
                    AiProvider::Claude => {
                        "Run `cloudcode open <session>` from your terminal to complete the OAuth login."
                    }
                    AiProvider::Codex => {
                        "Run `cloudcode open <session>` from your terminal, select 'Device code', and authorize in your browser."
                    }
                };
                DispatchResult::plain(format!(
                    "❌ {} needs authentication.\n\n{}",
                    provider, auth_hint
                ))
            } else {
                DispatchResult::error(err.to_string())
            }
        }
    }
}

pub async fn list_logic(state: &DaemonState) -> DispatchResult {
    match state.session_mgr.list().await {
        Ok(sessions) => {
            if sessions.is_empty() {
                DispatchResult::plain("No active sessions.")
            } else {
                let default = state.default_session.current();
                let mut text = String::from("📋 Active sessions:\n");
                for session in &sessions {
                    let marker = if default.as_deref() == Some(&session.name) {
                        " ← default"
                    } else {
                        ""
                    };
                    text.push_str(&format!(
                        "• {} [{}]{}\n",
                        session.name,
                        format!("{:?}", session.state),
                        marker
                    ));
                }
                DispatchResult::plain(text)
            }
        }
        Err(err) => DispatchResult::error(err.to_string()),
    }
}

pub async fn kill_logic(state: &DaemonState, args: &str) -> DispatchResult {
    if args.is_empty() {
        return DispatchResult::plain("Usage: /kill <session-name>");
    }

    match state.session_mgr.kill(args).await {
        Ok(()) => {
            if state.default_session.current().as_deref() == Some(args) {
                if let Err(err) = state.default_session.clear() {
                    return DispatchResult::plain(format!(
                        "⚠️ Session '{}' killed, but failed to clear persisted default session: {}",
                        args, err
                    ));
                }
            }
            DispatchResult::plain(format!("✅ Session '{}' killed.", args))
        }
        Err(err) => DispatchResult::error(err.to_string()),
    }
}

pub async fn use_logic(state: &DaemonState, args: &str) -> DispatchResult {
    if args.is_empty() {
        return DispatchResult::plain("Usage: /use <session-name>");
    }

    match session_exists_with(&state.session_mgr, args).await {
        Ok(true) => {
            if let Err(err) = state.default_session.set(Some(args.to_string())) {
                return DispatchResult::plain(format!(
                    "⚠️ Default session updated in memory, but failed to persist: {}",
                    err
                ));
            }
            DispatchResult::plain(format!("✅ Default session set to '{}'.", args))
        }
        Ok(false) => DispatchResult::plain(format!("❌ Session '{}' not found.", args)),
        Err(err) => DispatchResult::error(err.to_string()),
    }
}

pub async fn provider_logic(state: &DaemonState, args: &str) -> DispatchResult {
    if args.is_empty() {
        let current = state.session_mgr.current_provider();
        let claude_status = super::handlers::provider_status(AiProvider::Claude);
        let codex_status = super::handlers::provider_status(AiProvider::Codex);
        DispatchResult::plain(format!(
            "🤖 Current provider: {}\n\n\
             Claude: {}\n\
             Codex: {}\n\n\
             Use /provider claude or /provider codex to switch.",
            current.display_name(),
            claude_status.summary,
            codex_status.summary,
        ))
    } else {
        let target: AiProvider = match args.parse() {
            Ok(p) => p,
            Err(_) => {
                return DispatchResult::plain(
                    "Unknown provider. Use /provider claude or /provider codex",
                );
            }
        };

        let status = super::handlers::provider_status(target);
        if !status.switchable {
            return DispatchResult::plain(format!(
                "❌ Cannot switch to {}: {}.\nRun `cloudcode init --reauth` to configure.",
                target.display_name(),
                status.reason
            ));
        }

        state.session_mgr.set_provider(target);
        DispatchResult::plain(format!(
            "✅ Switched to {}. New sessions will use this provider.",
            target.display_name()
        ))
    }
}

pub async fn status_logic(state: &DaemonState) -> DispatchResult {
    match state.session_mgr.list().await {
        Ok(sessions) => DispatchResult::plain(format!(
            "📊 Status:\n• Sessions: {}\n• Daemon: running",
            sessions.len()
        )),
        Err(err) => DispatchResult::error(err.to_string()),
    }
}

pub async fn waiting_logic(state: &DaemonState) -> DispatchResult {
    let waiting = waiting_sessions_from(&state.question_states);
    if waiting.is_empty() {
        return DispatchResult::plain("No sessions are waiting for input.");
    }

    let mut text = String::from("⏳ Waiting sessions:\n");
    for (name, question) in &waiting {
        let summary = question.lines().last().unwrap_or("").trim();
        text.push_str(&format!("• {} — {}\n", name, summary));
    }
    text.push_str("\nUse /reply <session> <text> to answer.");
    DispatchResult::plain(text)
}

pub async fn reply_logic(state: &DaemonState, args: &str) -> DispatchResult {
    if args.is_empty() {
        return DispatchResult::plain("Usage: /reply [session] <text>");
    }

    match resolve_reply_target_with(&state.question_states, args).await {
        Ok(ReplyTarget::NoneWaiting) => {
            DispatchResult::plain("No sessions are currently waiting for input.")
        }
        Ok(ReplyTarget::Ready {
            session_name,
            reply_text,
        }) => {
            // Capture pane before replying so we can extract the response
            let pane_before = state
                .session_mgr
                .capture_pane_full(&session_name)
                .await
                .unwrap_or_default();

            match state
                .session_mgr
                .send_keys(&session_name, &reply_text)
                .await
            {
                Ok(()) => {
                    clear_waiting_state_from(&state.question_states, &session_name);

                    // Wait briefly for output to appear, then capture
                    tokio::time::sleep(tokio::time::Duration::from_secs(6)).await;
                    let pane_after = state
                        .session_mgr
                        .capture_pane_full(&session_name)
                        .await
                        .unwrap_or_default();

                    if pane_after != pane_before {
                        // There's new output — include it in the reply
                        let new_lines: Vec<&str> = pane_after
                            .lines()
                            .skip(pane_before.lines().count())
                            .filter(|l| {
                                let t = l.trim();
                                !t.is_empty() && t != ">" && t != "❯" && t != "$"
                            })
                            .collect();
                        if !new_lines.is_empty() {
                            let output = new_lines.join("\n");
                            DispatchResult::plain(format!(
                                "✅ Replied to '{}'.\n\nOutput:\n{}",
                                session_name, output
                            ))
                        } else {
                            DispatchResult::plain(format!("✅ Replied to '{}'.", session_name))
                        }
                    } else {
                        DispatchResult::plain(format!(
                            "✅ Replied to '{}'. Output still processing — use /peek to check.",
                            session_name
                        ))
                    }
                }
                Err(err) => DispatchResult::error(err.to_string()),
            }
        }
        Ok(ReplyTarget::Ambiguous(waiting)) => {
            let mut text =
                String::from("Multiple sessions are waiting. Use /reply <session> <text>.\n");
            for (name, question) in &waiting {
                let summary = question.lines().last().unwrap_or("").trim();
                text.push_str(&format!("• {} — {}\n", name, summary));
            }
            DispatchResult::plain(text)
        }
        Err(err) => DispatchResult::error(err.to_string()),
    }
}

pub async fn context_logic(state: &DaemonState, args: &str) -> DispatchResult {
    let explicit = (!args.is_empty()).then_some(args);
    match resolve_command_session_with(&state.session_mgr, &state.default_session, explicit).await {
        Ok(Some(name)) => {
            let context_path = format!("/home/claude/.cloudcode/contexts/context_{}.md", name);
            match tokio::fs::read_to_string(&context_path).await {
                Ok(content) if content.trim().is_empty() => {
                    DispatchResult::plain(format!("Context file for '{}' is empty.", name))
                }
                Ok(content) => DispatchResult::markdown(content),
                Err(_) => {
                    DispatchResult::plain(format!("No context file for session '{}' yet.", name))
                }
            }
        }
        Ok(None) => DispatchResult::plain(
            "No default session. Use /context <session> or /use <session> first.",
        ),
        Err(err) => DispatchResult::error(err.to_string()),
    }
}

pub async fn peek_logic(state: &DaemonState, args: &str) -> DispatchResult {
    let explicit = (!args.is_empty()).then_some(args);
    match resolve_command_session_with(&state.session_mgr, &state.default_session, explicit).await {
        Ok(Some(name)) => match state.session_mgr.capture_pane(&name).await {
            Ok(content) if content.trim().is_empty() => DispatchResult::plain("(pane is empty)"),
            Ok(content) => DispatchResult::preformatted(content),
            Err(err) => DispatchResult::error(err.to_string()),
        },
        Ok(None) => DispatchResult::plain(
            "No default session. Use /peek <session> or /use <session> first.",
        ),
        Err(err) => DispatchResult::error(err.to_string()),
    }
}

pub async fn type_logic(state: &DaemonState, args: &str) -> DispatchResult {
    if args.is_empty() {
        return DispatchResult::plain("Usage: /type [session] <text>");
    }

    match resolve_type_target_with(&state.session_mgr, &state.default_session, args).await {
        Ok(Some((name, text_to_type))) => {
            match state.session_mgr.send_keys(&name, &text_to_type).await {
                Ok(()) => {
                    clear_waiting_state_from(&state.question_states, &name);
                    DispatchResult::plain(format!("✅ Typed into '{}'.", name))
                }
                Err(err) => DispatchResult::error(err.to_string()),
            }
        }
        Ok(None) => DispatchResult::plain(
            "No default session. Use /type <session> <text> or /use <session> first.",
        ),
        Err(err) => DispatchResult::error(err.to_string()),
    }
}

pub async fn free_text_logic(state: &DaemonState, text: &str) -> DispatchResult {
    let mut auto_select_msg: Option<String> = None;
    let session_name = match resolve_free_text_session_with(
        &state.session_mgr,
        &state.default_session,
    )
    .await
    {
        Ok(FreeTextSessionTarget::Selected {
            name,
            auto_selected,
        }) => {
            if auto_selected {
                if let Err(err) = state.default_session.set(Some(name.clone())) {
                    return DispatchResult::plain(format!(
                        "⚠️ Auto-selected session '{}', but failed to persist default session: {}",
                        name, err
                    ));
                }
                auto_select_msg = Some(format!("📌 Auto-selected session '{}'.", name));
            }
            name
        }
        Ok(FreeTextSessionTarget::NoSessions) => {
            return DispatchResult::plain("No sessions available. Use /spawn to create one.");
        }
        Ok(FreeTextSessionTarget::MultipleSessions(sessions)) => {
            let mut list = String::from("No default session set. Available sessions:\n");
            for session in &sessions {
                list.push_str(&format!("• {}\n", session));
            }
            list.push_str("\nUse /use <name> to pick one.");
            return DispatchResult::plain(list);
        }
        Err(err) => {
            return DispatchResult::error(err.to_string());
        }
    };

    let provider = state.session_mgr.current_provider();
    let send_result = match provider {
        AiProvider::Codex => {
            // Codex: route through tmux for session sync
            state.session_mgr.send_via_tmux(&session_name, text).await
        }
        AiProvider::Claude => {
            // Claude: use print mode (clean stdout, --continue preserves context)
            // Retry transient errors up to 3 times
            let mut result = Err(anyhow::anyhow!("not started"));
            for attempt in 0..3 {
                result = state.session_mgr.send(&session_name, text).await;
                match &result {
                    Ok(_) => break,
                    Err(err) => {
                        let err_str = err.to_string();
                        if super::handlers::is_auth_error(err, state.session_mgr.current_provider())
                            || err_str.contains("does not exist")
                            || err_str.contains("timed out")
                        {
                            break;
                        }
                        if attempt < 2 {
                            log::warn!(
                                "Send attempt {} failed ({}), retrying in 5s...",
                                attempt + 1,
                                err_str
                            );
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            }
            result
        }
    };

    let mut final_result = match send_result {
        Ok(result) => {
            // Claude notification bridge: echo TG activity into tmux
            // so CLI users see what happened
            if provider == AiProvider::Claude {
                let summary = if result.text.len() > 200 {
                    format!("{}...", &result.text[..200])
                } else {
                    result.text.clone()
                };
                let bridge_msg = format!(
                    "# [TG] {}: {}",
                    if text.len() > 80 { &text[..80] } else { text },
                    summary.replace('\n', " ")
                );
                // Fire-and-forget — non-fatal if CLI user isn't attached
                let _ = state.session_mgr.send_keys(&session_name, &bridge_msg).await;
            }

            let mut dispatch = DispatchResult::markdown(result.text);
            dispatch.files = result.files;
            dispatch
        }
        Err(err) => {
            let provider = state.session_mgr.current_provider();
            if super::handlers::is_auth_error(&err, provider) {
                let auth_hint = match provider {
                    AiProvider::Claude => {
                        "Complete login from your terminal:\n\
                         1. cloudcode open <session>\n\
                         2. Copy the login URL and paste in your browser\n\
                         3. Complete the OAuth flow"
                    }
                    AiProvider::Codex => {
                        "Complete login from your terminal:\n\
                         1. cloudcode open <session>\n\
                         2. Select 'Device code' when prompted\n\
                         3. Visit the URL in your browser to authorize"
                    }
                };
                DispatchResult::plain(format!(
                    "❌ {} needs authentication.\n\n{}\n\nTelegram will work once login is complete.",
                    provider, auth_hint
                ))
            } else {
                DispatchResult::error(err.to_string())
            }
        }
    };

    // Prepend auto-select notification if applicable
    if let Some(msg) = auto_select_msg {
        final_result.messages.insert(
            0,
            DispatchMessage {
                text: msg,
                format: MessageFormat::Plain,
            },
        );
    }

    final_result
}
