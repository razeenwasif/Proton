//! Why an agent task failed, from what its engine said.
//!
//! The kind decides what the round does next: a seat that is missing or
//! signed out sits the round out, one that hit its usage limit cools until
//! its window resets, and a crash is tried once more.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "TaskFailure")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Failure {
    /// The answer would not read, even after its repair.
    Unparseable { why: String },
    TimedOut,
    /// Not installed, not signed in, or not able to run unattended.
    Unavailable { why: String },
    /// A usage or rate limit: try again after `resets` (seconds since the epoch), if known.
    Limited { why: String, resets: Option<u64> },
    Crashed { why: String },
    Cancelled,
}

impl Failure {
    pub fn describe(&self) -> String {
        match self {
            Failure::Unparseable { why } => format!("Its answer could not be read: {why}"),
            Failure::TimedOut => "It ran out of time.".into(),
            Failure::Unavailable { why } | Failure::Crashed { why } => why.clone(),
            Failure::Limited { why, .. } => why.clone(),
            Failure::Cancelled => "Cancelled.".into(),
        }
    }
}

/// What an engine's error means.
pub fn classify(error: &str) -> Failure {
    let lower = error.to_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    let why = error.trim().to_string();
    if any(&["rate limit", "rate-limit", "usage limit", "too many requests", "429", "quota", "limit reached", "resource_exhausted", "resource exhausted", "hit your limit"]) {
        Failure::Limited { why, resets: None }
    } else if any(&[
        "not installed",
        "could not be found",
        "no such file",
        "not signed in",
        "not logged in",
        "/login",
        "sign in",
        "log in",
        "authenticat",
        "unauthorized",
        "401",
        "invalid api key",
        "could not be switched off",
        "cannot take part unattended",
    ]) {
        Failure::Unavailable { why }
    } else {
        Failure::Crashed { why }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_engine_error_reads_as_what_it_is() {
        let table: &[(&str, &str)] = &[
            ("Claude Code is not installed. Install it with `npm i -g @anthropic-ai/claude-code`.", "unavailable"),
            ("Invalid API key · Please run /login", "unavailable"),
            ("Not logged in", "unavailable"),
            ("Codex's config (/Users/p/.codex/config.toml) could not be read, so its MCP servers could not be switched off: bad toml", "unavailable"),
            ("The Gemini CLI cannot take part unattended yet.", "unavailable"),
            ("You've hit your usage limit. Upgrade to Pro or try again at 3:05 PM.", "limited"),
            ("API Error: 429 {\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\"}}", "limited"),
            ("RESOURCE_EXHAUSTED: quota exceeded for gemini-3.8", "limited"),
            ("Codex stopped unexpectedly.", "crashed"),
            ("Antigravity stopped: exit status 1", "crashed"),
        ];
        for (error, kind) in table {
            let got = match classify(error) {
                Failure::Unavailable { .. } => "unavailable",
                Failure::Limited { .. } => "limited",
                Failure::Crashed { .. } => "crashed",
                other => panic!("{other:?}"),
            };
            assert_eq!(got, *kind, "{error}");
        }
        assert_eq!(classify("x").describe(), "x");
    }
}
