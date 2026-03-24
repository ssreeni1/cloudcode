/// Wizard steps in the onboarding flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Welcome,
    CloudProvider,
    CloudToken,
    Provider,
    Claude,
    ClaudeApiKey,
    OAuthWarning,
    Codex,
    CodexApiKey,
    CodexOAuthWarning,
    Telegram,
    Generating,
    Complete,
}

impl WizardStep {
    /// The total number of visible steps (used for progress display).
    pub fn total_steps() -> usize {
        6
    }

    /// The current step number (1-indexed) for the progress indicator.
    pub fn step_number(&self) -> Option<usize> {
        match self {
            Self::Welcome => None,
            Self::CloudProvider => Some(1),
            Self::CloudToken => Some(2),
            Self::Provider => Some(3),
            Self::Claude | Self::ClaudeApiKey | Self::OAuthWarning => Some(4),
            Self::Codex | Self::CodexApiKey | Self::CodexOAuthWarning => Some(4),
            Self::Telegram => Some(5),
            Self::Generating => Some(6),
            Self::Complete => None,
        }
    }

    /// Label shown in the header bar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::CloudProvider => "Cloud Provider",
            Self::CloudToken => "Cloud API Token",
            Self::Provider => "AI Provider",
            Self::Claude => "Claude Authentication",
            Self::ClaudeApiKey => "Claude API Key",
            Self::OAuthWarning => "OAuth Setup Guide",
            Self::Codex => "Codex Authentication",
            Self::CodexApiKey => "Codex API Key",
            Self::CodexOAuthWarning => "Codex OAuth Guide",
            Self::Telegram => "Telegram (optional)",
            Self::Generating => "Setup",
            Self::Complete => "Complete",
        }
    }
}

/// Which input field currently has focus (for steps with multiple fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFocus {
    Primary,
    Secondary,
}

/// Status of an async validation operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationStatus {
    Idle,
    Validating,
    Success,
    Failed(String),
}

/// Events sent back from async validation tasks.
pub enum ValidationEvent {
    HetznerResult(Result<(), String>),
    GenerationComplete,
    ServerTypes(Result<Vec<crate::hetzner::client::ServerTypeInfo>, String>),
}

/// Top-level mode of the TUI application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Wizard,
    Main,
}
