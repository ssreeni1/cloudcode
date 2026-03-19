use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    Claude,
    Codex,
}

impl Default for AiProvider {
    fn default() -> Self {
        Self::Claude
    }
}

impl AiProvider {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
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
            _ => anyhow::bail!("Unknown provider '{}'. Valid: claude, codex", s),
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
    }
}
