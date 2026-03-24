use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    Claude,
    Codex,
    Amp,
    OpenCode,
    Pi,
    Cursor,
}

/// Static metadata for an AI provider — binary locations, auth detection,
/// idle/prompt patterns for Telegram polling, and behavioral flags.
pub struct ProviderMeta {
    /// Binary name or path (e.g. "claude", "codex", "amp")
    pub binary: &'static str,
    /// Human-readable name
    pub display_name: &'static str,
    /// Shell command to install this provider
    pub install_cmd: &'static str,
    /// Shell command to check installed version
    pub version_cmd: &'static str,
    /// Auth credential files to check (relative to $HOME). ANY existing = authed.
    pub auth_files: &'static [&'static str],
    /// Env vars that indicate auth is configured. ANY set = authed.
    pub auth_env_vars: &'static [&'static str],
    /// Whether this provider's headless auth is confirmed working
    pub stable: bool,
    /// Strings that indicate the provider is idle (ready for input).
    /// Used by question_poller to detect when the provider finished working.
    pub idle_patterns: &'static [&'static str],
    /// Strings that indicate a startup/prompt screen (not a question).
    /// Used by question_poller to avoid false-positive question detection.
    pub prompt_patterns: &'static [&'static str],
    /// Whether echoing Telegram messages into the tmux pane is safe.
    /// False for providers whose TUI gets confused by injected text.
    pub supports_bridge_echo: bool,
}

static CLAUDE_META: ProviderMeta = ProviderMeta {
    binary: "claude",
    display_name: "Claude",
    install_cmd: "curl -fsSL https://claude.ai/install.sh | sh",
    version_cmd: "claude --version",
    auth_files: &[".claude/.credentials.json"],
    auth_env_vars: &["ANTHROPIC_API_KEY"],
    stable: true,
    idle_patterns: &["❯", "❯ "],
    prompt_patterns: &[
        "bypass permissions",
        "shift+tab to cycle",
        "Claude Code v",
    ],
    supports_bridge_echo: true,
};

static CODEX_META: ProviderMeta = ProviderMeta {
    binary: "codex",
    display_name: "Codex",
    install_cmd: "npm install -g @openai/codex",
    version_cmd: "codex --version",
    auth_files: &[".codex/auth.json"],
    auth_env_vars: &["OPENAI_API_KEY"],
    stable: true,
    idle_patterns: &["›"],
    prompt_patterns: &["gpt-5.4", "% left ·"],
    supports_bridge_echo: false,
};

static AMP_META: ProviderMeta = ProviderMeta {
    binary: "amp",
    display_name: "Amp",
    install_cmd: "curl -fsSL https://ampcode.com/install.sh | sh",
    version_cmd: "amp --version",
    auth_files: &[".config/amp/credentials.json"],
    auth_env_vars: &["AMP_API_KEY"],
    stable: false,
    idle_patterns: &["⚡", "⚡ "],
    prompt_patterns: &["Amp v", "amp ready"],
    supports_bridge_echo: true,
};

static OPENCODE_META: ProviderMeta = ProviderMeta {
    binary: "opencode",
    display_name: "OpenCode",
    install_cmd: "curl -fsSL https://opencode.ai/install.sh | sh",
    version_cmd: "opencode --version",
    auth_files: &[".config/opencode/auth.json"],
    auth_env_vars: &["OPENCODE_API_KEY"],
    stable: false,
    idle_patterns: &["opencode>", "opencode> "],
    prompt_patterns: &["OpenCode v", "opencode ready"],
    supports_bridge_echo: true,
};

static PI_META: ProviderMeta = ProviderMeta {
    binary: "pi",
    display_name: "Pi",
    install_cmd: "npm install -g @anthropic/pi",
    version_cmd: "pi --version",
    auth_files: &[".config/pi/credentials.json"],
    auth_env_vars: &["PI_API_KEY"],
    stable: false,
    idle_patterns: &["λ", "λ "],
    prompt_patterns: &["Pi v", "pi ready"],
    supports_bridge_echo: true,
};

static CURSOR_META: ProviderMeta = ProviderMeta {
    binary: "cursor",
    display_name: "Cursor",
    install_cmd: "npm install -g @cursor/cli",
    version_cmd: "cursor --version",
    auth_files: &[".config/cursor/auth.json"],
    auth_env_vars: &["CURSOR_API_KEY"],
    stable: false,
    idle_patterns: &["▶", "▶ "],
    prompt_patterns: &["Cursor v", "cursor ready"],
    supports_bridge_echo: true,
};

impl Default for AiProvider {
    fn default() -> Self {
        Self::Claude
    }
}

impl AiProvider {
    /// All known provider variants.
    pub const ALL: &'static [AiProvider] = &[
        AiProvider::Claude,
        AiProvider::Codex,
        AiProvider::Amp,
        AiProvider::OpenCode,
        AiProvider::Pi,
        AiProvider::Cursor,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Amp => "amp",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
            Self::Cursor => "cursor",
        }
    }

    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Amp => "Amp",
            Self::OpenCode => "OpenCode",
            Self::Pi => "Pi",
            Self::Cursor => "Cursor",
        }
    }

    /// Static metadata for this provider.
    pub fn meta(&self) -> &'static ProviderMeta {
        match self {
            Self::Claude => &CLAUDE_META,
            Self::Codex => &CODEX_META,
            Self::Amp => &AMP_META,
            Self::OpenCode => &OPENCODE_META,
            Self::Pi => &PI_META,
            Self::Cursor => &CURSOR_META,
        }
    }

    /// Build the shell command to spawn this provider in a tmux session.
    ///
    /// `home` is the $HOME directory on the remote VPS (e.g. "/home/claude").
    pub fn spawn_command(&self, home: &str) -> String {
        match self {
            Self::Claude => {
                let bin = format!("{home}/.local/bin/claude");
                format!(
                    "while true; do {bin} --dangerously-skip-permissions --permission-mode bypassPermissions; \
                     echo '\\n[cloudcode] Claude exited. Restarting in 3s... (Ctrl-C to stop)'; \
                     sleep 3; done"
                )
            }
            Self::Codex => {
                format!(
                    "if ! /usr/local/bin/codex login status >/dev/null 2>&1; then \
                       echo '[cloudcode] Codex needs authentication. Starting device auth flow...'; \
                       /usr/local/bin/codex login --device-auth; \
                     fi; \
                     while true; do /usr/local/bin/codex --add-dir {home}/.cloudcode/contexts; \
                     echo '\\n[cloudcode] Codex exited. Restarting in 3s... (Ctrl-C to stop)'; \
                     sleep 3; done"
                )
            }
            Self::Amp => {
                format!(
                    "while true; do {home}/.local/bin/amp --non-interactive; \
                     echo '\\n[cloudcode] Amp exited. Restarting in 3s... (Ctrl-C to stop)'; \
                     sleep 3; done"
                )
            }
            Self::OpenCode => {
                format!(
                    "while true; do {home}/.local/bin/opencode; \
                     echo '\\n[cloudcode] OpenCode exited. Restarting in 3s... (Ctrl-C to stop)'; \
                     sleep 3; done"
                )
            }
            Self::Pi => {
                format!(
                    "while true; do /usr/local/bin/pi --non-interactive; \
                     echo '\\n[cloudcode] Pi exited. Restarting in 3s... (Ctrl-C to stop)'; \
                     sleep 3; done"
                )
            }
            Self::Cursor => {
                format!(
                    "while true; do /usr/local/bin/cursor --non-interactive; \
                     echo '\\n[cloudcode] Cursor exited. Restarting in 3s... (Ctrl-C to stop)'; \
                     sleep 3; done"
                )
            }
        }
    }
}

impl std::fmt::Display for AiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for AiProvider {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "amp" => Ok(Self::Amp),
            "opencode" | "open_code" | "open-code" => Ok(Self::OpenCode),
            "pi" => Ok(Self::Pi),
            "cursor" => Ok(Self::Cursor),
            _ => anyhow::bail!(
                "Unknown provider '{}'. Valid: claude, codex, amp, opencode, pi, cursor",
                s
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AiProvider;

    #[test]
    fn default_is_claude() {
        assert_eq!(AiProvider::default(), AiProvider::Claude);
    }

    #[test]
    fn parses_case_insensitively() {
        assert_eq!("claude".parse::<AiProvider>().unwrap(), AiProvider::Claude);
        assert_eq!("CODEX".parse::<AiProvider>().unwrap(), AiProvider::Codex);
        assert_eq!("Amp".parse::<AiProvider>().unwrap(), AiProvider::Amp);
        assert_eq!(
            "OpenCode".parse::<AiProvider>().unwrap(),
            AiProvider::OpenCode
        );
        assert_eq!(
            "open_code".parse::<AiProvider>().unwrap(),
            AiProvider::OpenCode
        );
        assert_eq!(
            "open-code".parse::<AiProvider>().unwrap(),
            AiProvider::OpenCode
        );
        assert_eq!("PI".parse::<AiProvider>().unwrap(), AiProvider::Pi);
        assert_eq!("cursor".parse::<AiProvider>().unwrap(), AiProvider::Cursor);
        assert!("other".parse::<AiProvider>().is_err());
    }

    #[test]
    fn serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&AiProvider::Claude).unwrap(),
            "\"claude\""
        );
        assert_eq!(
            serde_json::to_string(&AiProvider::Codex).unwrap(),
            "\"codex\""
        );
        assert_eq!(
            serde_json::to_string(&AiProvider::Amp).unwrap(),
            "\"amp\""
        );
        assert_eq!(
            serde_json::to_string(&AiProvider::OpenCode).unwrap(),
            "\"open_code\""
        );
        assert_eq!(
            serde_json::to_string(&AiProvider::Pi).unwrap(),
            "\"pi\""
        );
        assert_eq!(
            serde_json::to_string(&AiProvider::Cursor).unwrap(),
            "\"cursor\""
        );
    }

    #[test]
    fn deserializes_all_variants() {
        assert_eq!(
            serde_json::from_str::<AiProvider>("\"claude\"").unwrap(),
            AiProvider::Claude
        );
        assert_eq!(
            serde_json::from_str::<AiProvider>("\"codex\"").unwrap(),
            AiProvider::Codex
        );
        assert_eq!(
            serde_json::from_str::<AiProvider>("\"amp\"").unwrap(),
            AiProvider::Amp
        );
        assert_eq!(
            serde_json::from_str::<AiProvider>("\"open_code\"").unwrap(),
            AiProvider::OpenCode
        );
        assert_eq!(
            serde_json::from_str::<AiProvider>("\"pi\"").unwrap(),
            AiProvider::Pi
        );
        assert_eq!(
            serde_json::from_str::<AiProvider>("\"cursor\"").unwrap(),
            AiProvider::Cursor
        );
    }

    #[test]
    fn serde_roundtrip_all_variants() {
        for &provider in AiProvider::ALL {
            let json = serde_json::to_string(&provider).unwrap();
            let parsed: AiProvider = serde_json::from_str(&json).unwrap();
            assert_eq!(provider, parsed, "roundtrip failed for {:?}", provider);
        }
    }

    #[test]
    fn as_str_roundtrip() {
        for &provider in AiProvider::ALL {
            let s = provider.as_str();
            let parsed: AiProvider = s.parse().unwrap();
            assert_eq!(provider, parsed, "as_str roundtrip failed for {:?}", provider);
        }
    }

    #[test]
    fn display_name_is_nonempty() {
        for &provider in AiProvider::ALL {
            assert!(!provider.display_name().is_empty());
        }
    }

    #[test]
    fn meta_binary_matches_as_str() {
        // Claude and Codex binary names match their as_str
        assert_eq!(AiProvider::Claude.meta().binary, "claude");
        assert_eq!(AiProvider::Codex.meta().binary, "codex");
        assert_eq!(AiProvider::Amp.meta().binary, "amp");
        assert_eq!(AiProvider::OpenCode.meta().binary, "opencode");
        assert_eq!(AiProvider::Pi.meta().binary, "pi");
        assert_eq!(AiProvider::Cursor.meta().binary, "cursor");
    }

    #[test]
    fn meta_auth_files_nonempty() {
        for &provider in AiProvider::ALL {
            let meta = provider.meta();
            assert!(
                !meta.auth_files.is_empty() || !meta.auth_env_vars.is_empty(),
                "{:?} has no auth detection",
                provider
            );
        }
    }

    #[test]
    fn meta_idle_patterns_nonempty() {
        for &provider in AiProvider::ALL {
            assert!(
                !provider.meta().idle_patterns.is_empty(),
                "{:?} has no idle patterns",
                provider
            );
        }
    }

    #[test]
    fn stable_providers() {
        assert!(AiProvider::Claude.meta().stable);
        assert!(AiProvider::Codex.meta().stable);
        assert!(!AiProvider::Amp.meta().stable);
        assert!(!AiProvider::OpenCode.meta().stable);
        assert!(!AiProvider::Pi.meta().stable);
        assert!(!AiProvider::Cursor.meta().stable);
    }

    #[test]
    fn spawn_command_contains_binary() {
        let home = "/home/claude";
        for &provider in AiProvider::ALL {
            let cmd = provider.spawn_command(home);
            assert!(
                cmd.contains(provider.meta().binary),
                "{:?} spawn_command doesn't contain binary name",
                provider
            );
            assert!(
                cmd.contains("Restarting in 3s"),
                "{:?} spawn_command missing restart loop",
                provider
            );
        }
    }

    #[test]
    fn bridge_echo_per_provider() {
        assert!(AiProvider::Claude.meta().supports_bridge_echo);
        assert!(!AiProvider::Codex.meta().supports_bridge_echo);
    }

    #[test]
    fn all_variants_count() {
        assert_eq!(AiProvider::ALL.len(), 6);
    }
}
