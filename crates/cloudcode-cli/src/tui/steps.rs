/// Wizard steps in the onboarding flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Welcome,
    Hetzner,
    Claude,
    ClaudeApiKey,
    OAuthWarning,
    Telegram,
    Generating,
    Complete,
}

impl WizardStep {
    /// The total number of visible steps (used for progress display).
    pub fn total_steps() -> usize {
        4
    }

    /// The current step number (1-indexed) for the progress indicator.
    pub fn step_number(&self) -> Option<usize> {
        match self {
            Self::Welcome => None,
            Self::Hetzner => Some(1),
            Self::Claude | Self::ClaudeApiKey | Self::OAuthWarning => Some(2),
            Self::Telegram => Some(3),
            Self::Generating => Some(4),
            Self::Complete => None,
        }
    }

    /// Label shown in the header bar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Hetzner => "Hetzner Cloud",
            Self::Claude => "Claude Authentication",
            Self::ClaudeApiKey => "Claude API Key",
            Self::OAuthWarning => "OAuth Setup Guide",
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
#[derive(Debug)]
pub enum ValidationEvent {
    HetznerResult(Result<(), String>),
    GenerationComplete,
}

/// Top-level mode of the TUI application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Wizard,
    Main,
}
