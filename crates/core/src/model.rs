use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Tool {
    Claude,
    Codex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Source {
    ClaudeCli,
    ClaudeDesktop,
    CodexCli,
    CodexDesktop,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_write_unspecified: u64,
}

impl TokenCounts {
    pub fn add_saturating(&mut self, other: &Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.reasoning = self.reasoning.saturating_add(other.reasoning);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write_5m = self.cache_write_5m.saturating_add(other.cache_write_5m);
        self.cache_write_1h = self.cache_write_1h.saturating_add(other.cache_write_1h);
        self.cache_write_unspecified = self
            .cache_write_unspecified
            .saturating_add(other.cache_write_unspecified);
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UsageEvent {
    pub tool: Tool,
    pub source: Source,
    pub session_id: String,
    pub request_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub model: String,
    pub project: Option<String>,
    pub is_subagent: bool,
    pub tokens: TokenCounts,
}
