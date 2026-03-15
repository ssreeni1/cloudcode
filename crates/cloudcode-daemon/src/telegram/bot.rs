use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatId;

use crate::config::TelegramConfig;
use crate::session::manager::SessionManager;

use super::handlers;

/// State shared across all telegram handlers
pub struct BotState {
    pub session_mgr: Arc<SessionManager>,
    pub owner_id: ChatId,
    pub default_session: tokio::sync::Mutex<Option<String>>,
}

pub async fn run(tg_config: &TelegramConfig, session_mgr: Arc<SessionManager>) {
    log::info!("Starting Telegram bot...");

    let bot = Bot::new(&tg_config.bot_token);

    let state = Arc::new(BotState {
        session_mgr,
        owner_id: ChatId(tg_config.owner_id),
        default_session: tokio::sync::Mutex::new(None),
    });

    let handler = Update::filter_message()
        .endpoint(move |bot: Bot, msg: Message, state: Arc<BotState>| {
            handlers::handle_message(bot, msg, state)
        });

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
