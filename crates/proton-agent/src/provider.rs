//! Which service the agent talks to.
//!
//! Everything above this module speaks one vocabulary — [`Message`], [`Block`],
//! [`Delta`], [`Usage`] — and each provider translates at its own edge. The
//! vocabulary is Anthropic-shaped because that is what was here first, and
//! keeping it rather than inventing a neutral third one means the provider
//! that is used most does no translation at all.
//!
//! ## What actually differs
//!
//! Not much of the idea, and almost all of the spelling:
//!
//! - **Shape.** Anthropic sends `messages[].content[]` of typed blocks; Gemini
//!   sends `contents[].parts[]` where the type is which key is present.
//! - **Roles.** `assistant` against `model`, and Gemini has no system role —
//!   the system prompt is a field beside the conversation.
//! - **Tool results.** Anthropic matches a result to a call by `id`. Gemini's
//!   `functionCall` has no id at all and its `functionResponse` matches by
//!   *name*. Ids are therefore invented on the Gemini side and mapped back;
//!   see [`crate::gemini`].
//! - **Streaming.** Anthropic streams a block at a time with tool arguments as
//!   JSON fragments to be concatenated. Gemini sends whole `GenerateContent`
//!   chunks, and a function call arrives complete — which is less to get wrong,
//!   not more.
//!
//! [`Message`]: crate::wire::Message
//! [`Block`]: crate::wire::Block
//! [`Delta`]: crate::stream::Delta

use serde::{Deserialize, Serialize};

/// A service the agent can be pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Claude,
    Gemini,
}

impl Provider {
    /// Read a provider out of a settings value.
    ///
    /// An unrecognised name is Claude rather than an error: a settings file
    /// naming a provider this build has not heard of should start the editor
    /// and say so in the agent pane, not refuse to run.
    pub fn parse(name: &str) -> Provider {
        match name.trim().to_ascii_lowercase().as_str() {
            "gemini" | "google" => Provider::Gemini,
            _ => Provider::Claude,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Gemini => "gemini",
        }
    }

    /// What to call it on screen.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Gemini => "Gemini",
        }
    }

    /// The model used when the settings name none.
    ///
    /// `agent` is the careful one the pane runs on; `quick` is for completion
    /// and the semantic index, where latency costs more than depth buys.
    pub fn default_model(self, quick: bool) -> &'static str {
        match (self, quick) {
            (Provider::Claude, false) => "claude-opus-5",
            (Provider::Claude, true) => "claude-haiku-4-5-20251001",
            (Provider::Gemini, false) => "gemini-3-pro",
            (Provider::Gemini, true) => "gemini-2.5-flash",
        }
    }

    /// The environment variables that hold a key for it, in order.
    pub fn key_vars(self) -> &'static [&'static str] {
        match self {
            Provider::Claude => &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
            // `GEMINI_API_KEY` first: `GOOGLE_API_KEY` is used by half of
            // Google's other SDKs and is more likely to be set to something
            // that is not a Gemini key at all.
            Provider::Gemini => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        }
    }

    /// Where to send a streaming turn.
    ///
    /// Gemini puts the model in the path rather than in the body, so this
    /// needs to know which one — the only place a URL depends on it.
    pub fn url(self, model: &str) -> String {
        match self {
            Provider::Claude => {
                std::env::var("ANTHROPIC_BASE_URL")
                    .unwrap_or_else(|_| "https://api.anthropic.com".into())
                    .trim_end_matches('/')
                    .to_string()
                    + "/v1/messages"
            }
            Provider::Gemini => {
                let base = std::env::var("GEMINI_BASE_URL")
                    .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".into());
                format!(
                    "{}/v1beta/models/{model}:streamGenerateContent?alt=sse",
                    base.trim_end_matches('/')
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_nobody_recognises_is_claude_rather_than_an_error() {
        // A settings file from a newer build should start the editor.
        assert_eq!(Provider::parse("wat"), Provider::Claude);
        assert_eq!(Provider::parse(""), Provider::Claude);
    }

    #[test]
    fn the_name_is_read_the_way_a_person_would_write_it() {
        assert_eq!(Provider::parse("Gemini"), Provider::Gemini);
        assert_eq!(Provider::parse(" gemini "), Provider::Gemini);
        assert_eq!(Provider::parse("google"), Provider::Gemini);
    }

    #[test]
    fn gemini_puts_the_model_in_the_path() {
        let url = Provider::Gemini.url("gemini-3-pro");
        assert!(
            url.contains("/models/gemini-3-pro:streamGenerateContent"),
            "{url}"
        );
        // Without `alt=sse` the endpoint answers with a JSON array rather than
        // a stream, and the SSE reader would wait for an event that never came.
        assert!(url.ends_with("alt=sse"), "{url}");
    }

    #[test]
    fn claude_does_not() {
        assert!(Provider::Claude.url("anything").ends_with("/v1/messages"));
    }
}
