use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::process::Command as ProcessCommand;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::mpsc;
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

use crate::config::{
    AiProvider, AuthMethod, ClaudeConfig, CodexConfig, Config, HetznerConfig, TelegramConfig,
};
use crate::hetzner::client::{HetznerClient, ServerTypeInfo};
use crate::state::VpsState;

use super::steps::{AppMode, InputFocus, ValidationEvent, ValidationStatus, WizardStep};

// ── Log types ───────────────────────────────────────────────────────────

pub struct LogLine {
    pub text: String,
    pub is_error: bool,
}

/// A completed command and its output, kept for history.
pub struct HistoryEntry {
    pub command: String,
    pub lines: Vec<LogLine>,
    pub exit_ok: bool,
}

pub enum LogEvent {
    Stdout(String),
    Stderr(String),
    Done(Option<i32>),
}

// ── Inline prompt ───────────────────────────────────────────────────────

/// An inline prompt shown in the input bar (e.g. "Session name:").
/// `callback` is called with the user's input when they press Enter.
pub struct InlinePrompt {
    pub label: String,
    pub callback: PromptCallback,
}

pub enum PromptCallback {
    Spawn,
    DownConfirm,
}

// ── Server type picker ──────────────────────────────────────────────────

pub struct ServerTypePicker {
    pub types: Vec<ServerTypeInfo>,
    pub selected: usize,
    pub loading: bool,
    /// The configured location, used to show availability.
    pub location: String,
}

// ── Slash commands ──────────────────────────────────────────────────────

pub enum SlashCommand {
    Up,
    Down,
    Spawn(Option<String>),
    List,
    Open(String),
    Send(String, String),
    Kill(String),
    Status,
    Provider(Option<String>),
    Restart,
    Logs(Option<String>),
    Ssh(Vec<String>),
    Init,
    Help,
    Quit,
}

impl SlashCommand {
    /// Commands that need a real terminal (interactive stdin/tty).
    pub fn is_interactive(&self) -> bool {
        match self {
            Self::Open(_) => true,
            Self::Ssh(args) => args.is_empty(), // no args = interactive shell
            _ => false,
        }
    }

    /// Convert to CLI args for subprocess invocation.
    pub fn to_cli_args(&self) -> Vec<String> {
        match self {
            Self::Up => vec!["up".into()],
            Self::Spawn(name) => {
                let mut a = vec!["spawn".into()];
                if let Some(n) = name {
                    a.push(n.clone());
                }
                a
            }
            Self::List => vec!["list".into()],
            Self::Kill(s) => vec!["kill".into(), s.clone()],
            Self::Send(s, m) => vec!["send".into(), s.clone(), m.clone()],
            Self::Status => vec!["status".into()],
            Self::Provider(p) => {
                let mut a = vec!["provider".into()];
                if let Some(p) = p {
                    a.push(p.clone());
                }
                a
            }
            Self::Restart => vec!["restart".into()],
            Self::Logs(t) => {
                let mut a = vec!["logs".into()];
                if let Some(t) = t {
                    a.push(t.clone());
                }
                a
            }
            Self::Ssh(args) => {
                let mut a = vec!["ssh".into()];
                a.extend(args.iter().cloned());
                a
            }
            // Interactive commands use direct function calls, not subprocess
            Self::Down => vec!["down".into()],
            Self::Open(s) => vec!["open".into(), s.clone()],
            // Internal
            Self::Init | Self::Help | Self::Quit => vec![],
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Up => "/up".into(),
            Self::Down => "/down".into(),
            Self::Spawn(Some(n)) => format!("/spawn {n}"),
            Self::Spawn(None) => "/spawn".into(),
            Self::List => "/list".into(),
            Self::Open(s) => format!("/open {s}"),
            Self::Send(s, _) => format!("/send {s} ..."),
            Self::Kill(s) => format!("/kill {s}"),
            Self::Status => "/status".into(),
            Self::Provider(Some(p)) => format!("/provider {p}"),
            Self::Provider(None) => "/provider".into(),
            Self::Restart => "/restart".into(),
            Self::Logs(Some(t)) => format!("/logs {t}"),
            Self::Logs(None) => "/logs".into(),
            Self::Ssh(args) if args.is_empty() => "/ssh".into(),
            Self::Ssh(args) => format!("/ssh {}", args.join(" ")),
            Self::Init => "/init".into(),
            Self::Help => "/help".into(),
            Self::Quit => "/quit".into(),
        }
    }
}

pub enum ParseResult {
    Ok(SlashCommand),
    MissingArg(&'static str),
    Unknown(String),
    Empty,
}

pub fn parse_slash_command(input: &str) -> ParseResult {
    let input = input.trim().trim_start_matches('/');
    if input.is_empty() {
        return ParseResult::Empty;
    }

    let mut parts = input.splitn(3, ' ');
    let cmd = parts.next().unwrap_or("");
    let arg1 = parts.next().map(str::trim).filter(|s| !s.is_empty());
    let arg2 = parts.next().map(str::trim).filter(|s| !s.is_empty());

    match cmd {
        "up" => ParseResult::Ok(SlashCommand::Up),
        "down" => ParseResult::Ok(SlashCommand::Down),
        "spawn" => ParseResult::Ok(SlashCommand::Spawn(arg1.map(String::from))),
        "list" | "ls" => ParseResult::Ok(SlashCommand::List),
        "open" | "attach" => match arg1 {
            Some(s) => ParseResult::Ok(SlashCommand::Open(s.to_string())),
            None => ParseResult::MissingArg("/open <session>"),
        },
        "send" => match (arg1, arg2) {
            (Some(s), Some(m)) => ParseResult::Ok(SlashCommand::Send(s.to_string(), m.to_string())),
            _ => ParseResult::MissingArg("/send <session> <message>"),
        },
        "kill" => match arg1 {
            Some(s) => ParseResult::Ok(SlashCommand::Kill(s.to_string())),
            None => ParseResult::MissingArg("/kill <session>"),
        },
        "status" | "st" => ParseResult::Ok(SlashCommand::Status),
        "provider" => ParseResult::Ok(SlashCommand::Provider(arg1.map(String::from))),
        "restart" => ParseResult::Ok(SlashCommand::Restart),
        "logs" | "log" => ParseResult::Ok(SlashCommand::Logs(arg1.map(String::from))),
        "ssh" => {
            let rest = input.strip_prefix("ssh").unwrap_or("").trim();
            let args: Vec<String> = if rest.is_empty() {
                vec![]
            } else {
                rest.split_whitespace().map(String::from).collect()
            };
            ParseResult::Ok(SlashCommand::Ssh(args))
        }
        "init" | "setup" => ParseResult::Ok(SlashCommand::Init),
        "help" | "h" | "?" => ParseResult::Ok(SlashCommand::Help),
        "quit" | "q" | "exit" => ParseResult::Ok(SlashCommand::Quit),
        other => ParseResult::Unknown(other.to_string()),
    }
}

// ── App state ───────────────────────────────────────────────────────────

pub struct App {
    pub mode: AppMode,

    // ── Wizard state ──
    pub step: WizardStep,
    pub config: Config,
    pub existing_config: bool,
    pub vps_state: VpsState,

    // Hetzner
    pub hetzner_input: Input,
    pub hetzner_status: ValidationStatus,

    // Provider
    pub provider_choice: usize, // 0=Claude, 1=Codex, 2=Both
    pub wants_claude: bool,
    pub wants_codex: bool,

    // Claude
    pub auth_choice: usize,
    pub api_key_input: Input,

    // Codex
    pub codex_auth_choice: usize,
    pub codex_api_key_input: Input,

    // Telegram
    pub telegram_enabled: bool,
    pub telegram_choice: usize,
    pub telegram_token_input: Input,
    pub telegram_id_input: Input,
    pub telegram_focus: InputFocus,

    // SSH
    pub ssh_key_exists: bool,

    // Generation status
    pub gen_ssh_done: bool,
    pub gen_config_done: bool,

    // Async channels (wizard validation)
    pub validation_tx: mpsc::UnboundedSender<ValidationEvent>,
    pub validation_rx: mpsc::UnboundedReceiver<ValidationEvent>,

    // ── Main view state ──
    pub command_input: Input,
    pub error_message: Option<String>,
    /// Set when an interactive command needs TUI suspension.
    pub pending_command: Option<SlashCommand>,
    /// Inline prompt: (label, callback that takes the input string).
    /// When set, the input bar shows the label instead of `> /`.
    pub inline_prompt: Option<InlinePrompt>,

    // Console log
    pub log_tx: mpsc::UnboundedSender<LogEvent>,
    pub log_rx: mpsc::UnboundedReceiver<LogEvent>,
    pub log_lines: Vec<LogLine>,
    pub running_command: Option<String>,
    pub command_done: bool,
    pub show_help: bool,
    pub log_scroll: usize,
    /// History of completed commands and their output.
    pub history: Vec<HistoryEntry>,
    /// Server type picker for /up command.
    pub server_type_picker: Option<ServerTypePicker>,
    /// PID of the running subprocess (0 = none). Used by Ctrl+C to kill it.
    pub child_pid: Arc<AtomicU32>,

    pub should_quit: bool,
    pub spinner_tick: usize,
    /// Timestamp of last Ctrl+C press (for double-press to quit).
    pub last_ctrl_c: Option<std::time::Instant>,
}

impl App {
    pub fn new(force_wizard: bool) -> Result<Self> {
        let config = Config::load()?;
        let existing_config = config.hetzner.is_some() && config.claude.is_some();
        let vps_state = VpsState::load().unwrap_or_default();
        let ssh_key_exists = Config::ssh_key_path().map(|p| p.exists()).unwrap_or(false);

        let (validation_tx, validation_rx) = mpsc::unbounded_channel();
        let (log_tx, log_rx) = mpsc::unbounded_channel();

        let mode = if force_wizard || !existing_config {
            AppMode::Wizard
        } else {
            AppMode::Main
        };

        Ok(Self {
            mode,
            step: WizardStep::Welcome,
            config,
            existing_config,
            vps_state,

            hetzner_input: Input::default(),
            hetzner_status: ValidationStatus::Idle,

            provider_choice: 0,
            wants_claude: true,
            wants_codex: false,

            auth_choice: 0,
            api_key_input: Input::default(),

            codex_auth_choice: 0,
            codex_api_key_input: Input::default(),

            telegram_enabled: false,
            telegram_choice: 1,
            telegram_token_input: Input::default(),
            telegram_id_input: Input::default(),
            telegram_focus: InputFocus::Primary,

            ssh_key_exists,

            gen_ssh_done: false,
            gen_config_done: false,

            validation_tx,
            validation_rx,

            command_input: Input::default(),
            error_message: None,
            pending_command: None,
            inline_prompt: None,

            log_tx,
            log_rx,
            log_lines: Vec::new(),
            running_command: None,
            command_done: false,
            show_help: true,
            log_scroll: 0,
            history: Vec::new(),
            server_type_picker: None,
            child_pid: Arc::new(AtomicU32::new(0)),

            should_quit: false,
            spinner_tick: 0,
            last_ctrl_c: None,
        })
    }

    pub fn tick(&mut self) {
        self.spinner_tick = self.spinner_tick.wrapping_add(1);
    }

    pub fn spinner_char(&self) -> char {
        const CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        CHARS[self.spinner_tick % CHARS.len()]
    }

    pub fn is_command_running(&self) -> bool {
        self.running_command.is_some() && !self.command_done
    }

    fn kill_running_command(&mut self) {
        let pid = self.child_pid.load(Ordering::SeqCst);
        if pid != 0 {
            // Send SIGTERM to the subprocess
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            self.child_pid.store(0, Ordering::SeqCst);
        }
        self.command_done = true;
        self.log_lines.push(LogLine {
            text: "Cancelled.".to_string(),
            is_error: true,
        });
    }

    // ── Key dispatch ────────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            match self.mode {
                AppMode::Wizard => {
                    // Single Ctrl+C quits during wizard
                    self.should_quit = true;
                }
                AppMode::Main => {
                    if self.is_command_running() {
                        // Ctrl+C kills the subprocess
                        self.kill_running_command();
                        self.last_ctrl_c = None;
                    } else {
                        // Double Ctrl+C within 2 seconds to quit
                        let now = std::time::Instant::now();
                        if let Some(prev) = self.last_ctrl_c {
                            if now.duration_since(prev) < std::time::Duration::from_secs(2) {
                                self.should_quit = true;
                                return;
                            }
                        }
                        self.last_ctrl_c = Some(now);
                        self.error_message =
                            Some("Press Ctrl+C again to exit cloudcode.".to_string());
                    }
                }
            }
            return;
        }

        // Clear the Ctrl+C hint on any other keypress
        if self.last_ctrl_c.is_some() {
            self.last_ctrl_c = None;
            self.error_message = None;
        }

        match self.mode {
            AppMode::Wizard => self.handle_wizard_key(key),
            AppMode::Main => self.handle_main_key(key),
        }
    }

    fn handle_wizard_key(&mut self, key: KeyEvent) {
        match self.step {
            WizardStep::Welcome => self.handle_welcome_key(key),
            WizardStep::Hetzner => self.handle_hetzner_key(key),
            WizardStep::Provider => self.handle_provider_key(key),
            WizardStep::Claude => self.handle_claude_key(key),
            WizardStep::ClaudeApiKey => self.handle_api_key_key(key),
            WizardStep::OAuthWarning => self.handle_oauth_warning_key(key),
            WizardStep::Codex => self.handle_codex_key(key),
            WizardStep::CodexApiKey => self.handle_codex_api_key_key(key),
            WizardStep::CodexOAuthWarning => self.handle_codex_oauth_warning_key(key),
            WizardStep::Telegram => self.handle_telegram_key(key),
            WizardStep::Generating => {}
            WizardStep::Complete => {
                if key.code == KeyCode::Enter {
                    self.mode = AppMode::Main;
                    self.error_message = None;
                    self.command_input = Input::default();
                    self.show_help = true;
                    self.log_lines.clear();
                }
            }
        }
    }

    // ── Wizard step handlers ────────────────────────────────────────────

    fn handle_welcome_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.step = WizardStep::Hetzner,
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            _ => {}
        }
    }

    fn handle_hetzner_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let token = self.hetzner_input.value().trim().to_string();
                if !token.is_empty() && self.hetzner_status != ValidationStatus::Validating {
                    self.hetzner_status = ValidationStatus::Validating;
                    let tx = self.validation_tx.clone();
                    let token_clone = token.clone();
                    tokio::spawn(async move {
                        let client = HetznerClient::new(token_clone);
                        let result = client.validate_token().await;
                        let event = match result {
                            Ok(()) => ValidationEvent::HetznerResult(Ok(())),
                            Err(e) => ValidationEvent::HetznerResult(Err(e.to_string())),
                        };
                        let _ = tx.send(event);
                    });
                }
            }
            KeyCode::Esc => self.step = WizardStep::Welcome,
            _ => {
                if self.hetzner_status != ValidationStatus::Validating {
                    self.hetzner_input
                        .handle_event(&crossterm::event::Event::Key(key));
                    if matches!(self.hetzner_status, ValidationStatus::Failed(_)) {
                        self.hetzner_status = ValidationStatus::Idle;
                    }
                }
            }
        }
    }

    fn handle_provider_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.provider_choice > 0 {
                    self.provider_choice -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.provider_choice < 2 {
                    self.provider_choice += 1;
                }
            }
            KeyCode::Enter => {
                match self.provider_choice {
                    0 => {
                        // Claude only
                        self.wants_claude = true;
                        self.wants_codex = false;
                        self.config.default_provider = Some(AiProvider::Claude);
                        self.step = WizardStep::Claude;
                    }
                    1 => {
                        // Codex only
                        self.wants_claude = false;
                        self.wants_codex = true;
                        self.config.default_provider = Some(AiProvider::Codex);
                        self.step = WizardStep::Codex;
                    }
                    2 => {
                        // Both
                        self.wants_claude = true;
                        self.wants_codex = true;
                        self.config.default_provider = Some(AiProvider::Claude);
                        self.step = WizardStep::Claude;
                    }
                    _ => unreachable!(),
                }
            }
            KeyCode::Esc => self.step = WizardStep::Hetzner,
            _ => {}
        }
    }

    fn handle_claude_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.auth_choice > 0 {
                    self.auth_choice -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.auth_choice < 1 {
                    self.auth_choice += 1;
                }
            }
            KeyCode::Enter => {
                if self.auth_choice == 0 {
                    self.step = WizardStep::ClaudeApiKey;
                } else {
                    self.step = WizardStep::OAuthWarning;
                }
            }
            KeyCode::Esc => self.step = WizardStep::Provider,
            _ => {}
        }
    }

    fn next_step_after_claude(&self) -> WizardStep {
        if self.wants_codex {
            WizardStep::Codex
        } else {
            WizardStep::Telegram
        }
    }

    fn handle_api_key_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let api_key = self.api_key_input.value().trim().to_string();
                if !api_key.is_empty() {
                    self.config.claude = Some(ClaudeConfig {
                        auth_method: AuthMethod::ApiKey,
                        api_key: Some(api_key),
                        oauth_token: None,
                    });
                    self.step = self.next_step_after_claude();
                }
            }
            KeyCode::Esc => self.step = WizardStep::Claude,
            _ => {
                self.api_key_input
                    .handle_event(&crossterm::event::Event::Key(key));
            }
        }
    }

    fn handle_oauth_warning_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.config.claude = Some(ClaudeConfig {
                    auth_method: AuthMethod::Oauth,
                    api_key: None,
                    oauth_token: None,
                });
                self.step = self.next_step_after_claude();
            }
            KeyCode::Esc => self.step = WizardStep::Claude,
            _ => {}
        }
    }

    fn handle_codex_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.codex_auth_choice > 0 {
                    self.codex_auth_choice -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.codex_auth_choice < 1 {
                    self.codex_auth_choice += 1;
                }
            }
            KeyCode::Enter => {
                if self.codex_auth_choice == 0 {
                    self.step = WizardStep::CodexApiKey;
                } else {
                    self.step = WizardStep::CodexOAuthWarning;
                }
            }
            KeyCode::Esc => self.step = WizardStep::Provider,
            _ => {}
        }
    }

    fn handle_codex_api_key_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let api_key = self.codex_api_key_input.value().trim().to_string();
                if !api_key.is_empty() {
                    self.config.codex = Some(CodexConfig {
                        auth_method: AuthMethod::ApiKey,
                        api_key: Some(api_key),
                    });
                    self.step = WizardStep::Telegram;
                }
            }
            KeyCode::Esc => self.step = WizardStep::Codex,
            _ => {
                self.codex_api_key_input
                    .handle_event(&crossterm::event::Event::Key(key));
            }
        }
    }

    fn handle_codex_oauth_warning_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.config.codex = Some(CodexConfig {
                    auth_method: AuthMethod::Oauth,
                    api_key: None,
                });
                self.step = WizardStep::Telegram;
            }
            KeyCode::Esc => self.step = WizardStep::Codex,
            _ => {}
        }
    }

    fn handle_telegram_key(&mut self, key: KeyEvent) {
        if !self.telegram_enabled {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.telegram_choice > 0 {
                        self.telegram_choice -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.telegram_choice < 1 {
                        self.telegram_choice += 1;
                    }
                }
                KeyCode::Enter => {
                    if self.telegram_choice == 0 {
                        self.telegram_enabled = true;
                        self.telegram_focus = InputFocus::Primary;
                    } else {
                        self.start_generating();
                    }
                }
                KeyCode::Esc => {
                    if self.wants_codex && self.config.codex.is_some() {
                        self.step = WizardStep::Codex;
                    } else if self.wants_claude {
                        self.step = WizardStep::Claude;
                    } else {
                        self.step = WizardStep::Provider;
                    }
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Tab | KeyCode::BackTab => {
                    self.telegram_focus = match self.telegram_focus {
                        InputFocus::Primary => InputFocus::Secondary,
                        InputFocus::Secondary => InputFocus::Primary,
                    };
                }
                KeyCode::Enter => {
                    let token = self.telegram_token_input.value().trim().to_string();
                    let id_str = self.telegram_id_input.value().trim().to_string();
                    if !token.is_empty() && !id_str.is_empty() {
                        if let Ok(owner_id) = id_str.parse::<i64>() {
                            self.config.telegram = Some(TelegramConfig {
                                bot_token: token,
                                owner_id,
                            });
                            self.start_generating();
                        }
                    }
                }
                KeyCode::Esc => self.telegram_enabled = false,
                _ => {
                    let event = crossterm::event::Event::Key(key);
                    match self.telegram_focus {
                        InputFocus::Primary => {
                            self.telegram_token_input.handle_event(&event);
                        }
                        InputFocus::Secondary => {
                            self.telegram_id_input.handle_event(&event);
                        }
                    }
                }
            }
        }
    }

    fn start_generating(&mut self) {
        self.step = WizardStep::Generating;
        let tx = self.validation_tx.clone();
        let ssh_key_exists = self.ssh_key_exists;

        tokio::spawn(async move {
            if !ssh_key_exists {
                if let Ok(ssh_key_path) = Config::ssh_key_path() {
                    let _ = Config::ensure_dir();
                    let _ = ProcessCommand::new("ssh-keygen")
                        .args([
                            "-t",
                            "ed25519",
                            "-f",
                            ssh_key_path.to_str().unwrap_or_default(),
                            "-N",
                            "",
                            "-C",
                            "cloudcode",
                        ])
                        .output();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let _ = tx.send(ValidationEvent::GenerationComplete);
        });
    }

    // ── Validation event handler ────────────────────────────────────────

    pub fn handle_validation(&mut self, event: ValidationEvent) {
        match event {
            ValidationEvent::HetznerResult(result) => match result {
                Ok(()) => {
                    self.hetzner_status = ValidationStatus::Success;
                    let token = self.hetzner_input.value().trim().to_string();
                    self.config.hetzner = Some(HetznerConfig { api_token: token });
                    self.step = WizardStep::Provider;
                }
                Err(e) => {
                    self.hetzner_status = ValidationStatus::Failed(e);
                }
            },
            ValidationEvent::GenerationComplete => {
                self.gen_ssh_done = true;
                if self.config.save().is_ok() {
                    self.gen_config_done = true;
                }
                self.step = WizardStep::Complete;
            }
            ValidationEvent::ServerTypes(result) => {
                if let Some(ref mut picker) = self.server_type_picker {
                    picker.loading = false;
                    match result {
                        Ok(mut types) => {
                            // Sort by cores then memory for a logical ordering
                            types.sort_by(|a, b| {
                                a.cores.cmp(&b.cores).then(
                                    a.memory
                                        .partial_cmp(&b.memory)
                                        .unwrap_or(std::cmp::Ordering::Equal),
                                )
                            });
                            picker.types = types;
                        }
                        Err(e) => {
                            self.server_type_picker = None;
                            self.error_message = Some(format!("Failed to fetch server types: {e}"));
                        }
                    }
                }
            }
        }
    }

    // ── Log event handler ───────────────────────────────────────────────

    pub fn handle_log_event(&mut self, event: LogEvent) {
        self.log_scroll = 0; // auto-scroll to bottom
        match event {
            LogEvent::Stdout(line) => {
                self.log_lines.push(LogLine {
                    text: line,
                    is_error: false,
                });
            }
            LogEvent::Stderr(line) => {
                self.log_lines.push(LogLine {
                    text: line,
                    is_error: true,
                });
            }
            LogEvent::Done(code) => {
                self.command_done = true;
                if let Some(code) = code {
                    if code != 0 {
                        self.log_lines.push(LogLine {
                            text: format!("exited with code {code}"),
                            is_error: true,
                        });
                    }
                }
                // Reload VPS state in case /up or /down changed it
                if let Ok(state) = VpsState::load() {
                    self.vps_state = state;
                }
            }
        }
    }

    // ── Spawn captured command ──────────────────────────────────────────

    /// Move current command output into history (if any).
    fn flush_to_history(&mut self) {
        if let Some(cmd_name) = self.running_command.take() {
            let lines = std::mem::take(&mut self.log_lines);
            self.history.push(HistoryEntry {
                command: cmd_name,
                lines,
                exit_ok: self.command_done,
            });
            self.command_done = false;
        }
    }

    pub fn spawn_captured_command(&mut self, cmd: SlashCommand) {
        let display = cmd.display_name();
        let args = cmd.to_cli_args();
        self.spawn_with_display_and_args(display, args);
    }

    /// Shared subprocess spawning logic: sets up TUI state, spawns a child
    /// process with the given CLI `args`, streams stdout/stderr into `log_tx`,
    /// and sends `LogEvent::Done` when complete.
    fn spawn_with_display_and_args(&mut self, display: String, args: Vec<String>) {
        let tx = self.log_tx.clone();
        let pid_ref = self.child_pid.clone();

        self.flush_to_history();
        self.running_command = Some(display);
        self.command_done = false;
        self.show_help = false;
        self.log_scroll = 0;
        self.error_message = None;
        self.child_pid.store(0, Ordering::SeqCst);

        tokio::spawn(async move {
            let exe = match std::env::current_exe() {
                Ok(e) => e,
                Err(e) => {
                    let _ = tx.send(LogEvent::Stderr(format!("Failed to find executable: {e}")));
                    let _ = tx.send(LogEvent::Done(None));
                    return;
                }
            };

            let mut child = match ProcessCommand::new(&exe)
                .args(&args)
                .env("NO_COLOR", "1")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(LogEvent::Stderr(format!("Failed to run: {e}")));
                    let _ = tx.send(LogEvent::Done(None));
                    return;
                }
            };

            // Store PID so Ctrl+C can kill it
            pid_ref.store(child.id(), Ordering::SeqCst);

            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();

            let tx2 = tx.clone();
            let stdout_handle = tokio::task::spawn_blocking(move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(l) => {
                            let _ = tx2.send(LogEvent::Stdout(l));
                        }
                        Err(_) => break,
                    }
                }
            });

            let tx3 = tx.clone();
            let stderr_handle = tokio::task::spawn_blocking(move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(l) => {
                            let _ = tx3.send(LogEvent::Stderr(l));
                        }
                        Err(_) => break,
                    }
                }
            });

            let _ = stdout_handle.await;
            let _ = stderr_handle.await;

            let status = child.wait().ok().and_then(|s| s.code());
            pid_ref.store(0, Ordering::SeqCst);
            let _ = tx.send(LogEvent::Done(status));
        });
    }

    // ── Main view handler ───────────────────────────────────────────────

    fn handle_main_key(&mut self, key: KeyEvent) {
        // If server type picker is active, handle it
        if self.server_type_picker.is_some() {
            self.handle_server_picker_key(key);
            return;
        }

        // If an inline prompt is active, handle it separately
        if self.inline_prompt.is_some() {
            self.handle_prompt_key(key);
            return;
        }

        match key.code {
            KeyCode::PageUp => {
                self.log_scroll = self.log_scroll.saturating_add(10);
            }
            KeyCode::PageDown => {
                self.log_scroll = self.log_scroll.saturating_sub(10);
            }
            KeyCode::Enter => {
                let input = self.command_input.value().trim().to_string();
                self.command_input.reset();

                if self.is_command_running() {
                    self.error_message = Some("Command still running...".to_string());
                    return;
                }

                match parse_slash_command(&input) {
                    ParseResult::Empty => {}
                    ParseResult::Ok(SlashCommand::Quit) => {
                        // /quit is not available in the TUI. Use Ctrl+C twice to exit.
                        self.error_message =
                            Some("Press Ctrl+C twice to exit cloudcode.".to_string());
                    }
                    ParseResult::Ok(SlashCommand::Help) => {
                        self.flush_to_history();
                        self.show_help = true;
                        self.error_message = None;
                    }
                    ParseResult::Ok(SlashCommand::Init) => {
                        if self.vps_state.is_provisioned() {
                            self.error_message = Some(
                                "VPS is running. Run /down first to tear it down before re-initializing.".to_string(),
                            );
                        } else {
                            self.mode = AppMode::Wizard;
                            self.step = WizardStep::Welcome;
                            self.reset_wizard_state();
                        }
                    }
                    ParseResult::Ok(SlashCommand::Up) => {
                        // Show server type picker before provisioning
                        self.error_message = None;
                        self.show_help = false;
                        let location = self
                            .config
                            .vps
                            .as_ref()
                            .and_then(|v| v.location.as_deref())
                            .unwrap_or("nbg1")
                            .to_string();
                        self.server_type_picker = Some(ServerTypePicker {
                            types: Vec::new(),
                            selected: 0,
                            loading: true,
                            location: location.clone(),
                        });
                        // Fetch server types asynchronously
                        if let Some(ref hetzner) = self.config.hetzner {
                            let tx = self.validation_tx.clone();
                            let token = hetzner.api_token.clone();
                            tokio::spawn(async move {
                                let client = HetznerClient::new(token);
                                let result = client.list_server_types(Some(&location)).await;
                                let event = match result {
                                    Ok(types) => {
                                        super::steps::ValidationEvent::ServerTypes(Ok(types))
                                    }
                                    Err(e) => super::steps::ValidationEvent::ServerTypes(Err(
                                        e.to_string()
                                    )),
                                };
                                let _ = tx.send(event);
                            });
                        } else {
                            self.server_type_picker = None;
                            self.error_message =
                                Some("Hetzner not configured. Run /init first.".to_string());
                        }
                    }
                    ParseResult::Ok(SlashCommand::Down) => {
                        // Inline confirmation before destroying VPS
                        self.inline_prompt = Some(InlinePrompt {
                            label: "Destroy VPS? This cannot be undone. Type 'yes' to confirm: "
                                .to_string(),
                            callback: PromptCallback::DownConfirm,
                        });
                        self.command_input.reset();
                        self.error_message = None;
                    }
                    ParseResult::Ok(SlashCommand::Spawn(None)) => {
                        // Prompt for session name inline
                        self.inline_prompt = Some(InlinePrompt {
                            label: "Session name (empty to auto-generate): ".to_string(),
                            callback: PromptCallback::Spawn,
                        });
                        self.command_input.reset();
                        self.error_message = None;
                    }
                    ParseResult::Ok(cmd) => {
                        if cmd.is_interactive() {
                            self.pending_command = Some(cmd);
                            self.error_message = None;
                        } else {
                            self.spawn_captured_command(cmd);
                        }
                    }
                    ParseResult::MissingArg(usage) => {
                        self.error_message = Some(format!("Usage: {usage}"));
                    }
                    ParseResult::Unknown(cmd) => {
                        self.error_message =
                            Some(format!("Unknown command: /{cmd}. Type /help for commands."));
                    }
                }
            }
            KeyCode::Esc => {
                // Esc clears the input, doesn't quit
                self.command_input.reset();
            }
            _ => {
                self.command_input
                    .handle_event(&crossterm::event::Event::Key(key));
                if self.error_message.is_some() {
                    self.error_message = None;
                }
            }
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let input = self.command_input.value().trim().to_string();
                self.command_input.reset();
                let prompt = self.inline_prompt.take().unwrap();
                match prompt.callback {
                    PromptCallback::Spawn => {
                        let name = if input.is_empty() { None } else { Some(input) };
                        self.spawn_captured_command(SlashCommand::Spawn(name));
                    }
                    PromptCallback::DownConfirm => {
                        if input.eq_ignore_ascii_case("yes") {
                            // Run /down --force as a captured command
                            self.spawn_down_force();
                        } else {
                            self.error_message = Some("Aborted.".to_string());
                        }
                    }
                }
            }
            KeyCode::Esc => {
                self.inline_prompt = None;
                self.command_input.reset();
            }
            _ => {
                self.command_input
                    .handle_event(&crossterm::event::Event::Key(key));
            }
        }
    }

    fn handle_server_picker_key(&mut self, key: KeyEvent) {
        let picker = self.server_type_picker.as_mut().unwrap();
        if picker.loading {
            if key.code == KeyCode::Esc {
                self.server_type_picker = None;
            }
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if picker.selected > 0 {
                    picker.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !picker.types.is_empty() && picker.selected < picker.types.len() - 1 {
                    picker.selected += 1;
                }
            }
            KeyCode::Enter => {
                let location = picker.location.clone();
                if let Some(selected) = picker.types.get(picker.selected) {
                    if !selected.available_locations.contains(&location) {
                        self.error_message = Some(format!(
                            "'{}' is not available in location '{}'. Pick another type.",
                            selected.name, location
                        ));
                        return;
                    }
                    let server_type = selected.name.clone();
                    self.server_type_picker = None;
                    self.spawn_up_with_server_type(server_type);
                }
            }
            KeyCode::Esc => {
                self.server_type_picker = None;
            }
            _ => {}
        }
    }

    fn spawn_up_with_server_type(&mut self, server_type: String) {
        let display = format!("/up --server-type {server_type}");
        let args = vec!["up".to_string(), "--server-type".to_string(), server_type];
        self.spawn_with_display_and_args(display, args);
    }

    fn spawn_down_force(&mut self) {
        self.spawn_with_display_and_args(
            "/down --force".to_string(),
            vec!["down".to_string(), "--force".to_string()],
        );
    }

    fn reset_wizard_state(&mut self) {
        self.hetzner_input = Input::default();
        self.hetzner_status = ValidationStatus::Idle;
        self.auth_choice = 0;
        self.api_key_input = Input::default();
        self.telegram_enabled = false;
        self.telegram_choice = 1;
        self.telegram_token_input = Input::default();
        self.telegram_id_input = Input::default();
        self.telegram_focus = InputFocus::Primary;
        self.gen_ssh_done = false;
        self.gen_config_done = false;
        self.error_message = None;
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    pub fn mask_secret(s: &str) -> String {
        if s.len() <= 4 {
            "****".to_string()
        } else {
            format!("{}...", &s[..4])
        }
    }

    pub fn is_oauth(&self) -> bool {
        self.config
            .claude
            .as_ref()
            .map(|c| c.uses_oauth())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_no_arg() {
        match parse_slash_command("provider") {
            ParseResult::Ok(SlashCommand::Provider(None)) => {}
            other => panic!(
                "Expected Provider(None), got {:?}",
                std::mem::discriminant(&match other {
                    ParseResult::Ok(c) => c,
                    _ => panic!("Expected Ok"),
                })
            ),
        }
    }

    #[test]
    fn parse_provider_with_arg() {
        match parse_slash_command("provider codex") {
            ParseResult::Ok(SlashCommand::Provider(Some(p))) => assert_eq!(p, "codex"),
            _ => panic!("Expected Provider(Some(\"codex\"))"),
        }
    }

    #[test]
    fn parse_basic_commands() {
        assert!(matches!(
            parse_slash_command("up"),
            ParseResult::Ok(SlashCommand::Up)
        ));
        assert!(matches!(
            parse_slash_command("down"),
            ParseResult::Ok(SlashCommand::Down)
        ));
        assert!(matches!(
            parse_slash_command("list"),
            ParseResult::Ok(SlashCommand::List)
        ));
        assert!(matches!(
            parse_slash_command("ls"),
            ParseResult::Ok(SlashCommand::List)
        ));
        assert!(matches!(
            parse_slash_command("status"),
            ParseResult::Ok(SlashCommand::Status)
        ));
        assert!(matches!(
            parse_slash_command("st"),
            ParseResult::Ok(SlashCommand::Status)
        ));
        assert!(matches!(
            parse_slash_command("help"),
            ParseResult::Ok(SlashCommand::Help)
        ));
        assert!(matches!(
            parse_slash_command("quit"),
            ParseResult::Ok(SlashCommand::Quit)
        ));
    }

    #[test]
    fn parse_empty_and_unknown() {
        assert!(matches!(parse_slash_command(""), ParseResult::Empty));
        assert!(matches!(
            parse_slash_command("nonexistent"),
            ParseResult::Unknown(_)
        ));
    }

    #[test]
    fn parse_commands_with_args() {
        assert!(matches!(
            parse_slash_command("spawn myname"),
            ParseResult::Ok(SlashCommand::Spawn(Some(_)))
        ));
        assert!(matches!(
            parse_slash_command("open sess1"),
            ParseResult::Ok(SlashCommand::Open(_))
        ));
        assert!(matches!(
            parse_slash_command("kill sess1"),
            ParseResult::Ok(SlashCommand::Kill(_))
        ));
        assert!(matches!(
            parse_slash_command("open"),
            ParseResult::MissingArg(_)
        ));
        assert!(matches!(
            parse_slash_command("kill"),
            ParseResult::MissingArg(_)
        ));
    }
}
