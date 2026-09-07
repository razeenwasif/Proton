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
    if let Some(token) = oauth_profile() {
        return Ok(Found {
            auth: Auth::Bearer(token),
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
/// than to a confusing failure. An expired token is not detected here — it
/// comes back as a 401, which says so.
fn oauth_profile() -> Option<String> {
    let text = std::fs::read_to_string(profile_path()?).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;

    // Look in the documented place first, then one level down for any of the
    // spellings the field has had.
    let direct = json
        .pointer("/claudeAiOauth/accessToken")
        .and_then(|v| v.as_str());
    if let Some(token) = direct {
        return Some(token.to_string());
    }
    json.as_object()?.values().find_map(|section| {
        section
            .get("accessToken")
            .or_else(|| section.get("access_token"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
