//! Finding a credential, in the order the design settled on.
//!
//! `ANTHROPIC_API_KEY`, else `ANTHROPIC_AUTH_TOKEN`, else the OAuth profile
//! the Claude CLI leaves on disk. All three are checked before anything
//! prompts, because a user who has already signed in elsewhere should not be
//! asked again.
//!
//! Nothing here ever logs a credential, and [`Auth`]'s `Debug` prints the
//! source rather than the secret — an API key in a panic message is an API key
//! in a bug report.

use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    /// A console API key. Goes in `x-api-key`.
    Key(String),
    /// An OAuth access token. Goes in `Authorization: Bearer`, and needs its
    /// own beta header — the two schemes are not interchangeable.
    Bearer(String),
}

/// Where a credential came from, for the statusline and for errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    ApiKey,
    AuthToken,
    OauthProfile,
    CodexLogin,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deliberately not the value.
        match self {
            Auth::Key(_) => f.write_str("Auth::Key(<redacted>)"),
            Auth::Bearer(_) => f.write_str("Auth::Bearer(<redacted>)"),
        }
    }
}

#[derive(Debug)]
pub struct Found {
    pub auth: Auth,
    pub source: Source,
}

/// The base URL, so a gateway or a proxy can stand in for the API.
pub fn base_url() -> String {
    std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .map(|u| format!("{}/v1/messages", u.trim_end_matches('/')))
        .unwrap_or_else(|| crate::wire::API.to_string())
}

/// Resolve a credential for a provider, or explain what to do about it.
///
/// Gemini has one scheme and one place to look, which is why it is a few lines
/// rather than a ladder: no OAuth profile on disk, and its key goes in a
/// header of its own rather than as a bearer token.
pub fn resolve_for(provider: crate::provider::Provider) -> Result<Found, String> {
    use crate::provider::Provider;
    if provider == Provider::Codex {
        return Err("Codex manages its own sign-in; use the Codex app or codex login".into());
    }
    if provider == Provider::Claude {
        return resolve();
    }

    for name in provider.key_vars() {
        if let Some(key) = std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            return Ok(Found {
                auth: Auth::Key(key),
                source: Source::ApiKey,
            });
        }
    }
    Err(format!(
        "no credential for {}: set {}",
        provider.label(),
        provider.key_vars().join(" or ")
    ))
}

/// Resolve a credential, or explain what to do about it.
pub fn resolve() -> Result<Found, String> {
    let from_env = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };

    if let Some(key) = from_env("ANTHROPIC_API_KEY") {
        return Ok(Found {
            auth: Auth::Key(key),
            source: Source::ApiKey,
        });
    }
    if let Some(token) = from_env("ANTHROPIC_AUTH_TOKEN") {
        return Ok(Found {
            auth: Auth::Bearer(token),
            source: Source::AuthToken,
        });
    }
    if let Some(stored) = oauth_profile() {
        // Said rather than sent. An expired token comes back as a 401 —
        // "the credential was rejected, check ANTHROPIC_API_KEY" — which is
        // the wrong instruction entirely: there is no key to check, the
        // sign-in lapsed, and the fix is one command. The file says when it
        // expires, so there is no reason to find out from the API.
        if stored.expired() {
            return Err(
                "the Claude CLI's sign-in has expired — run `claude` and sign in again, \
                 or set ANTHROPIC_API_KEY to use the API instead"
                    .into(),
            );
        }
        return Ok(Found {
            auth: Auth::Bearer(stored.token),
            source: Source::OauthProfile,
        });
    }

    Err("no credential: set ANTHROPIC_API_KEY, or sign in with the Claude CLI".into())
}

fn profile_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".claude").join(".credentials.json"))
}

/// Best-effort read of the CLI's stored token.
///
/// Best-effort on purpose: the file is the CLI's, its shape is not ours to
/// depend on, and guessing wrong should degrade to "set an API key" rather
/// than to a confusing failure.
fn oauth_profile() -> Option<Stored> {
    let text = std::fs::read_to_string(profile_path()?).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;

    // The documented place first, then one level down for any of the
    // spellings the field has had.
    if let Some(token) = json
        .pointer("/claudeAiOauth/accessToken")
        .and_then(|v| v.as_str())
    {
        return Some(Stored {
            token: token.to_string(),
            expires_at: json
                .pointer("/claudeAiOauth/expiresAt")
                .and_then(serde_json::Value::as_u64),
        });
    }
    json.as_object()?.values().find_map(|section| {
        let token = section
            .get("accessToken")
            .or_else(|| section.get("access_token"))
            .and_then(|v| v.as_str())?;
        Some(Stored {
            token: token.to_string(),
            expires_at: section
                .get("expiresAt")
                .or_else(|| section.get("expires_at"))
                .and_then(serde_json::Value::as_u64),
        })
    })
}

/// A token found in the CLI's file, and when it stops working.
struct Stored {
    token: String,
    /// Absent when the file does not say, which is treated as "still good" —
    /// refusing to use a token because its expiry was not stated would break
    /// a working setup over a missing field.
    expires_at: Option<u64>,
}

/// How long before the stated expiry a token is treated as already gone.
///
/// A turn takes tens of seconds. Starting one against a token with four
/// seconds left produces a 401 halfway through an answer, which is a worse
/// failure than not starting: the partial reply stays on screen and nothing
/// says why it stopped.
const MARGIN: u64 = 120;

impl Stored {
    fn expired(&self) -> bool {
        let Some(at) = self.expires_at else {
            return false;
        };
        // Milliseconds in the file this was written for, but the shape is the
        // CLI's rather than ours — a plain seconds value would otherwise read
        // as 1970 and every token would look expired.
        let at = match at > 100_000_000_000 {
            true => at / 1000,
            false => at,
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now + MARGIN >= at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(offset: i64) -> Stored {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        Stored {
            token: "t".into(),
            expires_at: Some(((now + offset) * 1000) as u64),
        }
    }

    #[test]
    fn an_expired_sign_in_is_known_before_the_api_says_so() {
        // The failure this replaces: a lapsed session came back as a 401,
        // which Proton reported as "the credential was rejected — check
        // ANTHROPIC_API_KEY". There is no key to check. The file says when it
        // expires, so there is no reason to ask the API.
        assert!(at(-60).expired());
        assert!(!at(3600).expired());
    }

    #[test]
    fn a_token_about_to_expire_counts_as_expired() {
        // A turn takes tens of seconds. Starting one with four seconds left
        // produces a 401 halfway through an answer — the partial reply stays
        // on screen and nothing says why it stopped.
        assert!(
            at(30).expired(),
            "it should not start a turn it cannot finish"
        );
        assert!(!at(600).expired());
    }

    #[test]
    fn seconds_are_read_as_seconds_and_not_as_1970() {
        // The file this was written for stores milliseconds, but the shape is
        // the CLI's rather than ours. Reading a plain seconds value as
        // milliseconds would put every expiry in 1970 and refuse every token.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let seconds = Stored {
            token: "t".into(),
            expires_at: Some(now + 3600),
        };
        assert!(!seconds.expired());
    }

    #[test]
    fn a_file_that_does_not_say_when_is_treated_as_still_good() {
        // Refusing to use a token because its expiry was not stated would
        // break a working setup over a missing field.
        let quiet = Stored {
            token: "t".into(),
            expires_at: None,
        };
        assert!(!quiet.expired());
    }

    #[test]
    fn a_credential_never_prints_itself() {
        // The failure this prevents is a key pasted into an issue by someone
        // who copied a panic message.
        let auth = Auth::Key("sk-ant-not-a-real-key".into());
        let shown = format!("{auth:?}");
        assert!(!shown.contains("sk-ant"), "the key leaked into Debug");
        assert_eq!(shown, "Auth::Key(<redacted>)");

        let bearer = format!("{:?}", Auth::Bearer("oauth-token".into()));
        assert!(!bearer.contains("oauth-token"));
    }

    #[test]
    fn the_base_url_can_be_pointed_somewhere_else() {
        // Not just for tests: a gateway in front of the API is a normal
        // deployment, and it must not need a recompile.
        let restore = std::env::var("ANTHROPIC_BASE_URL").ok();

        std::env::set_var("ANTHROPIC_BASE_URL", "http://localhost:4010/");
        assert_eq!(base_url(), "http://localhost:4010/v1/messages");

        std::env::set_var("ANTHROPIC_BASE_URL", "  ");
        assert_eq!(base_url(), crate::wire::API, "blank means unset");

        std::env::remove_var("ANTHROPIC_BASE_URL");
        assert_eq!(base_url(), crate::wire::API);

        if let Some(value) = restore {
            std::env::set_var("ANTHROPIC_BASE_URL", value);
        }
    }
}
