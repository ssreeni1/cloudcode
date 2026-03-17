use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatId;

use crate::config::TelegramConfig;
use crate::session::manager::SessionManager;

use super::default_session::DefaultSessionStore;
use super::handlers;

/// State shared across all telegram handlers
pub struct BotState {
    pub session_mgr: Arc<SessionManager>,
    pub owner_id: ChatId,
    pub default_session: Arc<DefaultSessionStore>,
}

pub async fn run(tg_config: &TelegramConfig, session_mgr: Arc<SessionManager>) {
    log::info!("Starting Telegram bot...");

    let bot = Bot::new(&tg_config.bot_token);
    let default_session = match DefaultSessionStore::load() {
        Ok(store) => Arc::new(store),
        Err(err) => {
            log::warn!(
                "Failed to load persisted Telegram default session, starting empty: {}",
                err
            );
            Arc::new(DefaultSessionStore::empty())
        }
    };

    let state = Arc::new(BotState {
        session_mgr,
        owner_id: ChatId(tg_config.owner_id),
        default_session,
    });

    let handler =
        Update::filter_message().endpoint(move |bot: Bot, msg: Message, state: Arc<BotState>| {
            handlers::handle_message(bot, msg, state)
        });

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
