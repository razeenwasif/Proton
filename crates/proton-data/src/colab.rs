//! Google Colab integration: OAuth 2.0 PKCE authentication, Colab REST API client,
//! and Jupyter Server REST + WebSocket kernel transport.
//!
//! Uses only synchronous std + `ureq` + `tungstenite` on background threads so
//! `proton-data` and `proton-core` stay free of async runtime coupling.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use tungstenite::http::HeaderValue;
use tungstenite::Message as WsMessage;

use crate::wire::{Header, Message};

pub const DEFAULT_PUBLIC_API_BASE: &str = "https://colaboratory.googleapis.com";
pub const DEFAULT_GAPI_BASE: &str = "https://colab.pa.googleapis.com";
pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
pub const COLAB_SCOPES: &str =
    "openid email profile https://www.googleapis.com/auth/colaboratory";
pub const COLAB_CLIENT_AGENT: &str = "vscode";

const BEAT: Duration = Duration::from_millis(50);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(60);
const PROXY_REFRESH_LEAD_SECS: u64 = 300;
const OAUTH_REFRESH_LEAD_SECS: u64 = 120;
const OAUTH_WAIT_TIMEOUT: Duration = Duration::from_secs(300);
const LRO_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);

/* ── Public wire & API types ─────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ColabRuntimeSpec {
    pub variant: String,
    pub accelerator: String,
    pub shape: String,
    pub eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColabConnectionInfo {
    pub url: String,
    pub token: String,
    pub expire_time: String,
    #[serde(default)]
    pub endpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColabRuntime {
    pub name: String,
    pub runtime_id: String,
    pub variant: String,
    pub accelerator: String,
    pub shape: String,
    pub connection_info: ColabConnectionInfo,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColabAccountSummary {
    pub email: String,
    pub tier: String,
    pub ccu_balance: Option<f64>,
    pub ccu_rate_hourly: Option<f64>,
    pub assignments_count: u32,
    pub specs: Vec<ColabRuntimeSpec>,
    pub runtimes: Vec<ColabRuntime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColabOAuthConfig {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColabTokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_at_unix: u64,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthEndpoints {
    pub auth_url: String,
    pub token_url: String,
    pub userinfo_url: String,
}

impl Default for OAuthEndpoints {
    fn default() -> Self {
        Self {
            auth_url: GOOGLE_AUTH_URL.into(),
            token_url: GOOGLE_TOKEN_URL.into(),
            userinfo_url: GOOGLE_USERINFO_URL.into(),
        }
    }
}

/* ── Helpers ─────────────────────────────────────────────────────────────── */

/// Google APIs sometimes prefix JSON payloads with `)]}'\n` to prevent XSSI.
pub fn strip_xssi(text: &str) -> &str {
    let trimmed = text.trim_start_matches('\u{feff}');
    if let Some(rest) = trimmed.strip_prefix(")]}'\r\n") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix(")]}'\n") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix(")]}'") {
        rest.trim_start_matches(['\r', '\n'])
    } else {
        text
    }
}

/// Parse Google Cloud Console Desktop/Web `client_secret_*.json` or flat JSON.
pub fn parse_oauth_client_json(raw: &str) -> Result<(String, String), String> {
    let value: serde_json::Value =
        serde_json::from_str(raw.trim()).map_err(|e| format!("invalid OAuth JSON: {e}"))?;
    let obj = value
        .get("installed")
        .or_else(|| value.get("web"))
        .unwrap_or(&value);
    let client_id = obj
        .get("client_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("OAuth JSON missing `client_id`")?
        .to_string();
    let client_secret = obj
        .get("client_secret")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    Ok((client_id, client_secret))
}

pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let unreserved =
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(hex_nibble(byte >> 4));
            out.push(hex_nibble(byte & 0x0f));
        }
    }
    out
}

fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + (n - 10)) as char,
        _ => '0',
    }
}

pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                    out.push((hi << 4) | lo);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(percent_decode(k), percent_decode(v));
    }
    map
}

pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_url_token(bytes_len: usize) -> String {
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..bytes_len).map(|_| rng.random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn uuid4() -> String {
    let mut rng = rand::rng();
    let mut b: [u8; 16] = rng.random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parse an RFC 3339 UTC timestamp (e.g. `2026-11-25T01:45:00.123456Z`) into
/// seconds since Unix epoch.
pub fn parse_rfc3339_unix(ts: &str) -> Option<u64> {
    let s = ts.trim();
    let (date, rest) = s.split_once('T').or_else(|| s.split_once('t'))?;
    let mut dp = date.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: u32 = dp.next()?.parse().ok()?;
    let day: u32 = dp.next()?.parse().ok()?;
    if dp.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (time_part, offset_secs) = if let Some(core) = rest.strip_suffix('Z').or_else(|| rest.strip_suffix('z')) {
        (core, 0i64)
    } else if let Some((core, tz)) = rest.rsplit_once('+') {
        (core, parse_tz_offset(tz)?)
    } else if let Some((core, tz)) = rest.rsplit_once('-') {
        (core, -parse_tz_offset(tz)?)
    } else {
        return None;
    };

    let main_time = time_part.split('.').next()?;
    let mut tp = main_time.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let minute: i64 = tp.next()?.parse().ok()?;
    let second: i64 = tp.next()?.parse().ok()?;
    if tp.next().is_some() || !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let total = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_secs;
    u64::try_from(total).ok()
}

fn parse_tz_offset(tz: &str) -> Option<i64> {
    let (hh, mm) = tz.split_once(':')?;
    let h: i64 = hh.parse().ok()?;
    let m: i64 = mm.parse().ok()?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(h * 3600 + m * 60)
}

/// Howard Hinnant's civil-from-days inverse: days since 1970-01-01.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn normalize_variant(raw: &str) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "GPU" | "VARIANT_GPU" => "VARIANT_GPU".into(),
        "TPU" | "VARIANT_TPU" => "VARIANT_TPU".into(),
        _ => "VARIANT_CPU".into(),
    }
}

pub fn normalize_shape(raw: &str) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "HIGHMEM" | "HIGH_MEM" | "SHAPE_HIGHMEM" => "SHAPE_HIGHMEM".into(),
        _ => "SHAPE_STANDARD".into(),
    }
}

pub fn runtime_label(rt: &ColabRuntime) -> String {
    let base = match rt.variant.as_str() {
        "VARIANT_TPU" => {
            if rt.accelerator.is_empty() || rt.accelerator == "NONE" {
                "TPU".to_string()
            } else {
                format!("TPU {}", format_accelerator(&rt.accelerator))
            }
        }
        "VARIANT_GPU" => {
            if rt.accelerator.is_empty() || rt.accelerator == "NONE" {
                "GPU".to_string()
            } else {
                format_accelerator(&rt.accelerator)
            }
        }
        _ => "CPU".to_string(),
    };
    if rt.shape == "SHAPE_HIGHMEM" {
        format!("Colab · {base} High-RAM")
    } else {
        format!("Colab · {base}")
    }
}

fn format_accelerator(acc: &str) -> String {
    match acc.to_ascii_uppercase().as_str() {
        "V2_8" | "V28" => "v2-8".into(),
        "V5E1" | "V5E_1" => "v5e-1".into(),
        "V6E1" | "V6E_1" => "v6e-1".into(),
        other => other.to_string(),
    }
}

/* ── HTTP agent & error formatting ───────────────────────────────────────── */

fn http_agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(35)))
        .build()
        .into()
}

fn quick_http_agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .into()
}

/* ── Local config & token persistence ────────────────────────────────────── */

pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PROTON_CONFIG_DIR") {
        let p = PathBuf::from(dir);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("proton"))
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            let p = PathBuf::from(xdg);
            if !p.as_os_str().is_empty() {
                return Some(p.join("proton"));
            }
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config").join("proton"))
    }
}

fn oauth_path_in(dir: &Path) -> PathBuf {
    dir.join("colab-oauth.json")
}

fn tokens_path_in(dir: &Path) -> PathBuf {
    dir.join("colab-auth.json")
}

fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path.parent().ok_or("config path has no parent")?;
    ensure_private_dir(parent)?;
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let _ = std::fs::remove_file(&part);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(&part)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .and_then(|()| std::fs::rename(&part, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&part);
            format!("{}: {e}", path.display())
        })
}

pub fn load_oauth_client() -> Option<ColabOAuthConfig> {
    if let Ok(id) = std::env::var("PROTON_COLAB_CLIENT_ID") {
        let id = id.trim().to_string();
        if !id.is_empty() {
            let secret = std::env::var("PROTON_COLAB_CLIENT_SECRET")
                .unwrap_or_default()
                .trim()
                .to_string();
            return Some(ColabOAuthConfig {
                client_id: id,
                client_secret: secret,
            });
        }
    }
    let dir = config_dir()?;
    load_oauth_client_in(&dir)
}

pub fn load_oauth_client_in(dir: &Path) -> Option<ColabOAuthConfig> {
    let text = std::fs::read_to_string(oauth_path_in(dir)).ok()?;
    let cfg: ColabOAuthConfig = serde_json::from_str(&text).ok()?;
    (!cfg.client_id.trim().is_empty()).then_some(cfg)
}

pub fn save_oauth_client(client_id: &str, client_secret: &str) -> Result<(), String> {
    let dir = config_dir().ok_or("could not locate user config directory")?;
    save_oauth_client_in(&dir, client_id, client_secret)
}

pub fn save_oauth_client_in(
    dir: &Path,
    client_id: &str,
    client_secret: &str,
) -> Result<(), String> {
    let client_id = client_id.trim().to_string();
    if client_id.is_empty() {
        return Err("OAuth Client ID cannot be empty".into());
    }
    let cfg = ColabOAuthConfig {
        client_id,
        client_secret: client_secret.trim().to_string(),
    };
    write_private_json(&oauth_path_in(dir), &cfg)
}

pub fn load_tokens() -> Option<ColabTokens> {
    let dir = config_dir()?;
    load_tokens_in(&dir)
}

pub fn load_tokens_in(dir: &Path) -> Option<ColabTokens> {
    let text = std::fs::read_to_string(tokens_path_in(dir)).ok()?;
    let tokens: ColabTokens = serde_json::from_str(&text).ok()?;
    (!tokens.access_token.trim().is_empty()).then_some(tokens)
}

pub fn save_tokens(tokens: &ColabTokens) -> Result<(), String> {
    let dir = config_dir().ok_or("could not locate user config directory")?;
    save_tokens_in(&dir, tokens)
}

pub fn save_tokens_in(dir: &Path, tokens: &ColabTokens) -> Result<(), String> {
    write_private_json(&tokens_path_in(dir), tokens)
}

pub fn sign_out() -> Result<(), String> {
    let Some(dir) = config_dir() else {
        return Ok(());
    };
    let path = tokens_path_in(&dir);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/* ── OAuth 2.0 PKCE loopback & token exchange ────────────────────────────── */

pub fn ensure_fresh_token() -> Result<ColabTokens, String> {
    let dir = config_dir().ok_or("could not locate user config directory")?;
    ensure_fresh_token_with(&dir, &OAuthEndpoints::default())
}

pub fn ensure_fresh_token_with(
    dir: &Path,
    endpoints: &OAuthEndpoints,
) -> Result<ColabTokens, String> {
    let mut tokens = load_tokens_in(dir).ok_or("Not signed in to Google Colab")?;
    if tokens.expires_at_unix > unix_now().saturating_add(OAUTH_REFRESH_LEAD_SECS) {
        return Ok(tokens);
    }
    let Some(refresh_token) = tokens.refresh_token.clone().filter(|r| !r.is_empty()) else {
        if tokens.expires_at_unix > unix_now() {
            return Ok(tokens);
        }
        return Err("Colab access token expired — please sign in again".into());
    };
    let oauth = load_oauth_client()
        .or_else(|| load_oauth_client_in(dir))
        .ok_or("OAuth client credentials not configured for token refresh")?;

    let refreshed = exchange_refresh_token(&oauth, &refresh_token, &endpoints.token_url)?;
    tokens.access_token = refreshed.access_token;
    tokens.expires_at_unix = refreshed.expires_at_unix;
    if refreshed.refresh_token.is_some() {
        tokens.refresh_token = refreshed.refresh_token;
    }
    if tokens.email.as_deref().unwrap_or("").is_empty() {
        tokens.email = fetch_userinfo_email(&tokens.access_token, &endpoints.userinfo_url);
    }
    let _ = save_tokens_in(dir, &tokens);
    Ok(tokens)
}

pub fn set_direct_token(access_token: &str) -> Result<ColabTokens, String> {
    let dir = config_dir().ok_or("could not locate user config directory")?;
    set_direct_token_with(&dir, access_token, &OAuthEndpoints::default())
}

pub fn set_direct_token_with(
    dir: &Path,
    access_token: &str,
    endpoints: &OAuthEndpoints,
) -> Result<ColabTokens, String> {
    let access_token = access_token.trim().to_string();
    if access_token.is_empty() {
        return Err("Access token cannot be empty".into());
    }
    let email = fetch_userinfo_email(&access_token, &endpoints.userinfo_url);
    let tokens = ColabTokens {
        access_token,
        refresh_token: None,
        expires_at_unix: unix_now().saturating_add(3500),
        email,
    };
    save_tokens_in(dir, &tokens)?;
    Ok(tokens)
}

/// Start a local loopback HTTP listener for OAuth 2.0 PKCE. Returns the
/// browser sign-in URL and a receiver that resolves once the callback completes.
pub fn start_loopback_auth(
    oauth: ColabOAuthConfig,
) -> Result<(String, Receiver<Result<ColabTokens, String>>), String> {
    let dir = config_dir().ok_or("could not locate user config directory")?;
    start_loopback_auth_with(dir, oauth, OAuthEndpoints::default())
}

pub fn start_loopback_auth_with(
    dir: PathBuf,
    oauth: ColabOAuthConfig,
    endpoints: OAuthEndpoints,
) -> Result<(String, Receiver<Result<ColabTokens, String>>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("could not bind local OAuth callback port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("could not read OAuth callback port: {e}"))?
        .port();
    let _ = listener.set_nonblocking(true);

    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let state = random_url_token(24);
    let verifier = random_url_token(48);
    let challenge = pkce_challenge(&verifier);

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&code_challenge={}&code_challenge_method=S256&state={}",
        endpoints.auth_url,
        percent_encode(&oauth.client_id),
        percent_encode(&redirect_uri),
        percent_encode(COLAB_SCOPES),
        percent_encode(&challenge),
        percent_encode(&state),
    );

    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("proton-colab-oauth".into())
        .spawn(move || {
            let result = wait_for_oauth_callback(
                listener,
                &dir,
                &oauth,
                &endpoints,
                &redirect_uri,
                &verifier,
                &state,
            );
            let _ = tx.send(result);
        })
        .map_err(|e| format!("could not start OAuth thread: {e}"))?;

    Ok((auth_url, rx))
}

fn wait_for_oauth_callback(
    listener: TcpListener,
    dir: &Path,
    oauth: &ColabOAuthConfig,
    endpoints: &OAuthEndpoints,
    redirect_uri: &str,
    verifier: &str,
    expected_state: &str,
) -> Result<ColabTokens, String> {
    let deadline = Instant::now() + OAUTH_WAIT_TIMEOUT;
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("Timed out waiting for Google sign-in in browser".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("OAuth callback accept failed: {e}")),
        }
    };

    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 8192];
    let n = stream
        .read(&mut buf)
        .map_err(|e| format!("failed reading OAuth callback: {e}"))?;
    let req_text = String::from_utf8_lossy(&buf[..n]);
    let request_line = req_text.lines().next().unwrap_or("");
    let raw_target = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = raw_target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let params = parse_query(query);

    if let Some(err) = params.get("error") {
        let msg = params
            .get("error_description")
            .cloned()
            .unwrap_or_else(|| err.clone());
        write_oauth_html(
            &mut stream,
            400,
            "Sign-in cancelled",
            &format!("Google OAuth returned an error: {msg}"),
        );
        return Err(format!("Google sign-in failed: {msg}"));
    }

    if params.get("state").map(String::as_str) != Some(expected_state) {
        write_oauth_html(
            &mut stream,
            400,
            "Invalid OAuth state",
            "The callback state parameter did not match.",
        );
        return Err("OAuth state mismatch".into());
    }

    let Some(code) = params.get("code").filter(|c| !c.is_empty()) else {
        write_oauth_html(
            &mut stream,
            400,
            "Missing authorization code",
            "Google did not return an authorization code.",
        );
        return Err("OAuth callback missing code".into());
    };

    match exchange_code_for_tokens(
        oauth,
        code,
        verifier,
        redirect_uri,
        &endpoints.token_url,
        &endpoints.userinfo_url,
    ) {
        Ok(tokens) => {
            save_tokens_in(dir, &tokens)?;
            write_oauth_html(
                &mut stream,
                200,
                "Signed in to Google Colab",
                "Authentication succeeded. You can close this tab and return to Proton.",
            );
            Ok(tokens)
        }
        Err(why) => {
            write_oauth_html(&mut stream, 500, "Token exchange failed", &why);
            Err(why)
        }
    }
}

fn write_oauth_html(stream: &mut impl Write, status: u16, title: &str, detail: &str) {
    let reason = if status == 200 { "OK" } else { "Error" };
    let safe_title = escape_html(title);
    let safe_detail = escape_html(detail);
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{safe_title}</title>\
         <style>body{{font-family:-apple-system,BlinkMacSystemFont,sans-serif;background:#111216;\
         color:#e6e7eb;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0}}\
         .card{{max-width:440px;padding:28px 32px;border-radius:12px;background:#1a1c22;\
         border:1px solid #2b2f3a}}h1{{font-size:18px;margin:0 0 8px}}p{{color:#9da3b0;margin:0;line-height:1.5}}</style>\
         </head><body><div class=\"card\"><h1>{safe_title}</h1><p>{safe_detail}</p></div></body></html>"
    );
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[derive(Deserialize)]
struct TokenEndpointResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
}

fn exchange_code_for_tokens(
    oauth: &ColabOAuthConfig,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    token_url: &str,
    userinfo_url: &str,
) -> Result<ColabTokens, String> {
    let mut form = format!(
        "client_id={}&code={}&code_verifier={}&grant_type=authorization_code&redirect_uri={}",
        percent_encode(&oauth.client_id),
        percent_encode(code),
        percent_encode(verifier),
        percent_encode(redirect_uri),
    );
    if !oauth.client_secret.is_empty() {
        form.push_str("&client_secret=");
        form.push_str(&percent_encode(&oauth.client_secret));
    }

    let resp = post_form(token_url, &form)?;
    let expires_at_unix = unix_now().saturating_add(resp.expires_in.unwrap_or(3600));
    let email = fetch_userinfo_email(&resp.access_token, userinfo_url);
    Ok(ColabTokens {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at_unix,
        email,
    })
}

fn exchange_refresh_token(
    oauth: &ColabOAuthConfig,
    refresh_token: &str,
    token_url: &str,
) -> Result<ColabTokens, String> {
    let mut form = format!(
        "client_id={}&refresh_token={}&grant_type=refresh_token",
        percent_encode(&oauth.client_id),
        percent_encode(refresh_token),
    );
    if !oauth.client_secret.is_empty() {
        form.push_str("&client_secret=");
        form.push_str(&percent_encode(&oauth.client_secret));
    }

    let resp = post_form(token_url, &form)?;
    let expires_at_unix = unix_now().saturating_add(resp.expires_in.unwrap_or(3600));
    Ok(ColabTokens {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at_unix,
        email: None,
    })
}

fn post_form(url: &str, form_body: &str) -> Result<TokenEndpointResponse, String> {
    let agent = http_agent();
    let response = agent
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .send(form_body)
        .map_err(|e| format!("OAuth request failed: {e}"))?;
    let status = response.status().as_u16();
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("failed reading OAuth response: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("OAuth error ({status}): {}", extract_error_summary(&body)));
    }
    serde_json::from_str::<TokenEndpointResponse>(&body)
        .map_err(|e| format!("invalid OAuth token JSON: {e}"))
}

fn fetch_userinfo_email(access_token: &str, userinfo_url: &str) -> Option<String> {
    let agent = quick_http_agent();
    let response = agent
        .get(userinfo_url)
        .header("Authorization", &format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .call()
        .ok()?;
    if !(200..300).contains(&response.status().as_u16()) {
        return None;
    }
    let body = response.into_body().read_to_string().ok()?;
    let v: serde_json::Value = serde_json::from_str(strip_xssi(&body)).ok()?;
    v.get("email")
        .and_then(|e| e.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn extract_error_summary(body: &str) -> String {
    let cleaned = strip_xssi(body).trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(cleaned) {
        if let Some(msg) = v
            .get("error")
            .and_then(|e| e.get("message").or_else(|| e.as_str().map(|_| e)))
            .and_then(|m| m.as_str())
        {
            return msg.to_string();
        }
        if let Some(desc) = v.get("error_description").and_then(|d| d.as_str()) {
            return desc.to_string();
        }
    }
    let snippet: String = cleaned.chars().take(240).collect();
    if snippet.is_empty() {
        "empty error response".into()
    } else {
        snippet
    }
}

/* ── Colab REST API Client ───────────────────────────────────────────────── */

#[derive(Debug, Clone)]
pub struct ColabClient {
    public_base: String,
    gapi_base: String,
    agent: ureq::Agent,
}

impl Default for ColabClient {
    fn default() -> Self {
        Self::production()
    }
}

impl ColabClient {
    pub fn production() -> Self {
        Self::with_bases(DEFAULT_PUBLIC_API_BASE, DEFAULT_GAPI_BASE)
    }

    pub fn with_bases(public_base: impl Into<String>, gapi_base: impl Into<String>) -> Self {
        Self {
            public_base: public_base.into().trim_end_matches('/').to_string(),
            gapi_base: gapi_base.into().trim_end_matches('/').to_string(),
            agent: http_agent(),
        }
    }

    pub fn production_quick() -> Self {
        Self {
            public_base: DEFAULT_PUBLIC_API_BASE.into(),
            gapi_base: DEFAULT_GAPI_BASE.into(),
            agent: quick_http_agent(),
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        access_token: &str,
    ) -> Result<T, String> {
        let response = self
            .agent
            .get(url)
            .header("Authorization", &format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
            .call()
            .map_err(|e| format!("Colab request failed ({url}): {e}"))?;
        parse_colab_response(response, url)
    }

    pub fn get_runtime_specs(&self, access_token: &str) -> Result<Vec<ColabRuntimeSpec>, String> {
        let url = format!("{}/v1beta/runtimespecs", self.public_base);
        let parsed: ApiListRuntimeSpecs = self.get_json(&url, access_token)?;
        Ok(parsed
            .runtime_specs
            .into_iter()
            .map(|spec| ColabRuntimeSpec {
                variant: spec.key.variant,
                accelerator: spec.key.accelerator,
                shape: spec.key.shape,
                eligible: spec.eligible,
            })
            .collect())
    }

    pub fn list_runtimes(&self, access_token: &str) -> Result<Vec<ColabRuntime>, String> {
        let url = format!("{}/v1beta/runtimes", self.public_base);
        let parsed: ApiListRuntimes = self.get_json(&url, access_token)?;
        Ok(parsed
            .runtimes
            .into_iter()
            .filter_map(ApiRuntime::into_colab_runtime)
            .collect())
    }

    pub fn get_subscription_tier(&self, access_token: &str) -> Result<String, String> {
        let url = format!("{}/v1beta/subscription", self.public_base);
        let parsed: ApiSubscription = self.get_json(&url, access_token)?;
        Ok(parsed.tier)
    }

    pub fn get_ccu_info(&self, access_token: &str) -> Result<ApiUserInfo, String> {
        let url = format!(
            "{}/v1/user-info?get_ccu_consumption_info=true",
            self.gapi_base
        );
        self.get_json(&url, access_token)
    }

    pub fn account_summary(
        &self,
        access_token: &str,
        email: Option<String>,
    ) -> Result<ColabAccountSummary, String> {
        let specs = self.get_runtime_specs(access_token)?;
        let runtimes = self.list_runtimes(access_token)?;
        let mut tier = self
            .get_subscription_tier(access_token)
            .unwrap_or_else(|_| "SUBSCRIPTION_TIER_UNSPECIFIED".into());

        let (ccu_balance, ccu_rate_hourly, assignments_count) =
            match self.get_ccu_info(access_token) {
                Ok(info) => {
                    if let Some(t) = info.subscription_tier.as_deref() {
                        if tier == "SUBSCRIPTION_TIER_UNSPECIFIED" && !t.is_empty() {
                            tier = t.to_string();
                        }
                    }
                    (
                        info.paid_compute_units_balance,
                        info.consumption_rate_hourly,
                        info.assignments_count.unwrap_or(runtimes.len() as u32),
                    )
                }
                Err(_) => (None, None, runtimes.len() as u32),
            };

        Ok(ColabAccountSummary {
            email: email.unwrap_or_default(),
            tier,
            ccu_balance,
            ccu_rate_hourly,
            assignments_count,
            specs,
            runtimes,
        })
    }

    pub fn get_runtime(&self, access_token: &str, runtime_id: &str) -> Result<ColabRuntime, String> {
        let id = runtime_id.strip_prefix("runtimes/").unwrap_or(runtime_id);
        let url = format!("{}/v1beta/runtimes/{}", self.public_base, percent_encode(id));
        let raw: ApiRuntime = self.get_json(&url, access_token)?;
        raw.into_colab_runtime()
            .ok_or_else(|| format!("Colab runtime `{id}` is missing connection info"))
    }

    pub fn delete_runtime(&self, access_token: &str, runtime_id: &str) -> Result<(), String> {
        let id = runtime_id.strip_prefix("runtimes/").unwrap_or(runtime_id);
        let url = format!(
            "{}/v1beta/runtimes/{}?allowMissing=true",
            self.public_base,
            percent_encode(id)
        );
        let response = self
            .agent
            .delete(&url)
            .header("Authorization", &format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
            .call()
            .map_err(|e| format!("failed deleting Colab runtime `{id}`: {e}"))?;
        let status = response.status().as_u16();
        let body = response.into_body().read_to_string().unwrap_or_default();
        if !(200..300).contains(&status) && status != 404 {
            return Err(format!(
                "Colab delete runtime failed ({status}): {}",
                extract_error_summary(&body)
            ));
        }
        Ok(())
    }

    pub fn create_runtime(
        &self,
        access_token: &str,
        variant: &str,
        accelerator: &str,
        shape: &str,
    ) -> Result<ColabRuntime, String> {
        let variant_wire = normalize_variant(variant);
        let shape_wire = normalize_shape(shape);
        let acc = if variant_wire == "VARIANT_CPU" || accelerator.trim().is_empty() {
            "NONE".to_string()
        } else {
            accelerator.trim().to_ascii_uppercase()
        };

        let request_id = uuid4();
        let url = format!(
            "{}/v1beta/runtimes?requestId={}",
            self.public_base,
            percent_encode(&request_id)
        );
        let payload = serde_json::json!({
            "runtimeSpec": {
                "variant": variant_wire,
                "accelerator": acc,
                "shape": shape_wire,
            }
        });

        let response = self
            .agent
            .post(&url)
            .header("Authorization", &format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
            .send_json(&payload)
            .map_err(|e| format!("Colab CreateRuntime failed: {e}"))?;
        let mut op: ApiOperation = parse_colab_response(response, &url)?;

        let deadline = Instant::now() + LRO_TOTAL_TIMEOUT;
        let mut use_wait = true;
        while !op.done {
            if Instant::now() >= deadline {
                return Err("Timed out waiting for Colab runtime to start".into());
            }
            let op_name = op
                .name
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or("Colab CreateRuntime operation has no name")?;
            let op_id = op_name.strip_prefix("operations/").unwrap_or(op_name);
            let wait_url = if use_wait {
                format!(
                    "{}/v1/operations/{}:wait?timeout=25s",
                    self.public_base,
                    percent_encode(op_id)
                )
            } else {
                format!(
                    "{}/v1/operations/{}",
                    self.public_base,
                    percent_encode(op_id)
                )
            };

            let resp = self
                .agent
                .get(&wait_url)
                .header("Authorization", &format!("Bearer {access_token}"))
                .header("Accept", "application/json")
                .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
                .call()
                .map_err(|e| format!("Colab operation poll failed: {e}"))?;
            let status = resp.status().as_u16();
            if use_wait && (status == 404 || status == 501) {
                use_wait = false;
                continue;
            }
            op = parse_colab_response(resp, &wait_url)?;
            if !op.done && !use_wait {
                std::thread::sleep(Duration::from_secs(1));
            }
        }

        if let Some(err) = op.error {
            return Err(format_operation_error(&err, &acc));
        }

        if let Some(rt) = op.response.and_then(ApiRuntime::into_colab_runtime) {
            return Ok(rt);
        }
        if let Some(rt_name) = op.metadata.and_then(|m| m.runtime) {
            return self.get_runtime(access_token, &rt_name);
        }
        Err("Colab CreateRuntime completed without runtime connection info".into())
    }
}

fn parse_colab_response<T: for<'de> Deserialize<'de>>(
    response: ureq::http::Response<ureq::Body>,
    url: &str,
) -> Result<T, String> {
    let status = response.status().as_u16();
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("failed reading Colab response from {url}: {e}"))?;
    let cleaned = strip_xssi(&body);
    if !(200..300).contains(&status) {
        return Err(format!(
            "Colab API error ({status}): {}",
            extract_error_summary(cleaned)
        ));
    }
    serde_json::from_str::<T>(cleaned)
        .map_err(|e| format!("invalid Colab API JSON from {url}: {e}"))
}

fn format_operation_error(err: &ApiStatus, accelerator: &str) -> String {
    for detail in &err.details {
        if let Some(reason) = detail.get("reason").and_then(|r| r.as_str()) {
            match reason {
                "TOO_MANY_ACTIVE_RUNTIMES" => {
                    return "Too many active Colab runtimes — disconnect or terminate an existing runtime first.".into();
                }
                "DENYLISTED" => {
                    return "This Google account is currently blocked from assigning Colab runtimes.".into();
                }
                "QUOTA_EXCEEDED_USAGE_TIME" => {
                    return "Insufficient Colab compute units or usage quota for this runtime.".into();
                }
                _ if err.code == Some(9) && accelerator != "NONE" => {
                    return format!(
                        "Accelerator `{accelerator}` is currently unavailable ({reason}). Try another GPU/TPU or try again shortly."
                    );
                }
                _ => {
                    let msg = err.message.as_deref().unwrap_or("Colab operation failed");
                    return format!("{msg} ({reason})");
                }
            }
        }
    }
    if err.code == Some(9) && accelerator != "NONE" {
        return format!(
            "Accelerator `{accelerator}` is currently unavailable: {}",
            err.message.as_deref().unwrap_or("precondition failed")
        );
    }
    err.message
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "Colab operation failed".into())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiListRuntimeSpecs {
    #[serde(default)]
    runtime_specs: Vec<ApiRuntimeSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRuntimeSpec {
    #[serde(default)]
    eligible: bool,
    key: ApiRuntimeKey,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRuntimeKey {
    #[serde(default)]
    variant: String,
    #[serde(default)]
    accelerator: String,
    #[serde(default)]
    shape: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiListRuntimes {
    #[serde(default)]
    runtimes: Vec<ApiRuntime>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRuntime {
    #[serde(default)]
    name: String,
    runtime_spec: ApiRuntimeKey,
    connection_info: Option<ColabConnectionInfo>,
}

impl ApiRuntime {
    fn into_colab_runtime(self) -> Option<ColabRuntime> {
        let connection_info = self.connection_info?;
        let runtime_id = self
            .name
            .strip_prefix("runtimes/")
            .unwrap_or(&self.name)
            .to_string();
        Some(ColabRuntime {
            name: self.name,
            runtime_id,
            variant: self.runtime_spec.variant,
            accelerator: self.runtime_spec.accelerator,
            shape: self.runtime_spec.shape,
            connection_info,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiSubscription {
    #[serde(default)]
    tier: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiUserInfo {
    #[serde(default)]
    pub subscription_tier: Option<String>,
    #[serde(default)]
    pub paid_compute_units_balance: Option<f64>,
    #[serde(default)]
    pub consumption_rate_hourly: Option<f64>,
    #[serde(default)]
    pub assignments_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiOperation {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<ApiStatus>,
    #[serde(default)]
    metadata: Option<ApiCreateRuntimeMetadata>,
    #[serde(default)]
    response: Option<ApiRuntime>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiCreateRuntimeMetadata {
    #[serde(default)]
    runtime: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiStatus {
    #[serde(default)]
    code: Option<i32>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    details: Vec<serde_json::Value>,
}

/* ── Jupyter Server REST + WebSocket Kernel Transport ────────────────────── */

pub type ProxyTokenRefresher =
    Arc<dyn Fn(&str) -> Result<ColabConnectionInfo, String> + Send + Sync>;

pub struct ColabKernelHandle {
    runtime_id: String,
    base_url: String,
    kernel_id: String,
    proxy_token: Arc<Mutex<String>>,
    shutdown: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    last_error: Arc<Mutex<String>>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

impl ColabKernelHandle {
    pub fn start(
        runtime: &ColabRuntime,
        session: &str,
        to_send: Receiver<Message>,
        found: Sender<Message>,
        refresher: Option<ProxyTokenRefresher>,
    ) -> Result<Self, String> {
        let base_url = runtime.connection_info.url.trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err("Colab runtime connection URL is empty".into());
        }

        let agent = http_agent();
        let start_url = format!("{base_url}/api/kernels");
        let start_resp = agent
            .post(&start_url)
            .header(
                "X-Colab-Runtime-Proxy-Token",
                &runtime.connection_info.token,
            )
            .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .send_json(&serde_json::json!({ "name": "python3" }))
            .map_err(|e| format!("could not start Jupyter kernel on Colab runtime: {e}"))?;
        let status = start_resp.status().as_u16();
        let body = start_resp
            .into_body()
            .read_to_string()
            .map_err(|e| format!("failed reading Colab /api/kernels response: {e}"))?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "Colab kernel start failed ({status}): {}",
                extract_error_summary(&body)
            ));
        }
        let started: serde_json::Value = serde_json::from_str(strip_xssi(&body))
            .map_err(|e| format!("invalid /api/kernels response: {e}"))?;
        let kernel_id = started
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("Colab /api/kernels response missing kernel id")?
            .to_string();

        let ws_base = if let Some(rest) = base_url.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = base_url.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            return Err(format!("unsupported Colab proxy URL scheme: {base_url}"));
        };
        let ws_url = format!(
            "{ws_base}/api/kernels/{}/channels?session_id={}",
            percent_encode(&kernel_id),
            percent_encode(session)
        );

        let mut request = tungstenite::client::IntoClientRequest::into_client_request(ws_url.as_str())
            .map_err(|e| format!("invalid kernel websocket URL: {e}"))?;
        request.headers_mut().insert(
            "X-Colab-Runtime-Proxy-Token",
            HeaderValue::from_str(&runtime.connection_info.token)
                .map_err(|e| format!("invalid proxy token header: {e}"))?,
        );
        request.headers_mut().insert(
            "X-Colab-Client-Agent",
            HeaderValue::from_static(COLAB_CLIENT_AGENT),
        );

        let (mut socket, _) = tungstenite::connect(request)
            .map_err(|e| format!("could not open Colab kernel WebSocket: {e}"))?;
        match socket.get_ref() {
            tungstenite::stream::MaybeTlsStream::Plain(tcp) => {
                let _ = tcp.set_read_timeout(Some(BEAT));
            }
            tungstenite::stream::MaybeTlsStream::Rustls(tls) => {
                let _ = tls.sock.set_read_timeout(Some(BEAT));
            }
            _ => {}
        }

        let proxy_token = Arc::new(Mutex::new(runtime.connection_info.token.clone()));
        let expire_time = Arc::new(Mutex::new(runtime.connection_info.expire_time.clone()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let last_error = Arc::new(Mutex::new(String::new()));

        let thread_runtime_id = runtime.runtime_id.clone();
        let thread_base_url = base_url.clone();
        let thread_proxy_token = Arc::clone(&proxy_token);
        let thread_expire_time = Arc::clone(&expire_time);
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_alive = Arc::clone(&alive);
        let thread_err = Arc::clone(&last_error);

        let handle = std::thread::Builder::new()
            .name("proton-kernel-colab".into())
            .spawn(move || {
                pump_colab_websocket(
                    &mut socket,
                    to_send,
                    found,
                    &thread_runtime_id,
                    &thread_base_url,
                    thread_proxy_token,
                    thread_expire_time,
                    refresher,
                    thread_shutdown,
                    thread_alive,
                    thread_err,
                );
            })
            .map_err(|e| format!("could not spawn Colab WebSocket thread: {e}"))?;

        Ok(Self {
            runtime_id: runtime.runtime_id.clone(),
            base_url,
            kernel_id,
            proxy_token,
            shutdown,
            alive,
            last_error,
            _thread: Some(handle),
        })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn said(&self) -> String {
        let text = self
            .last_error
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            "Colab WebSocket closed".into()
        } else {
            trimmed.to_string()
        }
    }

    pub fn interrupt(&self) -> Result<(), String> {
        let token = self
            .proxy_token
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let url = format!(
            "{}/api/kernels/{}/interrupt",
            self.base_url,
            percent_encode(&self.kernel_id)
        );
        let resp = http_agent()
            .post(&url)
            .header("X-Colab-Runtime-Proxy-Token", &token)
            .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
            .send_empty()
            .map_err(|e| format!("interrupt failed: {e}"))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            return Err(format!(
                "kernel interrupt failed ({status}): {}",
                extract_error_summary(&body)
            ));
        }
        Ok(())
    }

    pub fn restart(&self) -> Result<(), String> {
        let token = self
            .proxy_token
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let url = format!(
            "{}/api/kernels/{}/restart",
            self.base_url,
            percent_encode(&self.kernel_id)
        );
        let resp = http_agent()
            .post(&url)
            .header("X-Colab-Runtime-Proxy-Token", &token)
            .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
            .send_empty()
            .map_err(|e| format!("restart failed: {e}"))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp.into_body().read_to_string().unwrap_or_default();
            return Err(format!(
                "kernel restart failed ({status}): {}",
                extract_error_summary(&body)
            ));
        }
        Ok(())
    }

    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }
}

impl Drop for ColabKernelHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let token = self
            .proxy_token
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let url = format!(
            "{}/api/kernels/{}",
            self.base_url,
            percent_encode(&self.kernel_id)
        );
        // Best-effort quick cleanup so quitting Proton never stalls waiting on remote HTTP.
        let _ = quick_http_agent()
            .delete(&url)
            .header("X-Colab-Runtime-Proxy-Token", &token)
            .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
            .call();
    }
}

#[allow(clippy::too_many_arguments)]
fn pump_colab_websocket<S: Read + Write>(
    socket: &mut tungstenite::WebSocket<S>,
    to_send: Receiver<Message>,
    found: Sender<Message>,
    runtime_id: &str,
    base_url: &str,
    proxy_token: Arc<Mutex<String>>,
    expire_time: Arc<Mutex<String>>,
    refresher: Option<ProxyTokenRefresher>,
    shutdown: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    last_error: Arc<Mutex<String>>,
) {
    let mut last_keepalive = Instant::now();
    let mut last_refresh_check = Instant::now();
    let background_busy = Arc::new(AtomicBool::new(false));

    loop {
        if shutdown.load(Ordering::SeqCst) {
            let _ = socket.close(None);
            break;
        }

        loop {
            match to_send.try_recv() {
                Ok(message) => {
                    let wire = encode_ws_message(&message);
                    if let Err(err) = socket.send(WsMessage::Text(wire.into())) {
                        set_ws_error(&last_error, &format!("Colab WebSocket send error: {err}"));
                        alive.store(false, Ordering::SeqCst);
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    alive.store(false, Ordering::SeqCst);
                    return;
                }
            }
        }

        if last_keepalive.elapsed() >= KEEPALIVE_INTERVAL {
            last_keepalive = Instant::now();
            if !background_busy.swap(true, Ordering::SeqCst) {
                let ka_url = format!("{base_url}/api/sessions");
                let token = proxy_token.lock().map(|g| g.clone()).unwrap_or_default();
                let busy = Arc::clone(&background_busy);
                let _ = std::thread::Builder::new()
                    .name("proton-colab-keepalive".into())
                    .spawn(move || {
                        let _ = quick_http_agent()
                            .get(&ka_url)
                            .header("X-Colab-Runtime-Proxy-Token", &token)
                            .header("X-Colab-Client-Agent", COLAB_CLIENT_AGENT)
                            .call();
                        busy.store(false, Ordering::SeqCst);
                    });
            }
        }

        if let Some(ref refresh_fn) = refresher {
            if last_refresh_check.elapsed() >= Duration::from_secs(30) {
                last_refresh_check = Instant::now();
                let exp = expire_time.lock().map(|g| g.clone()).unwrap_or_default();
                let near_expiry = parse_rfc3339_unix(&exp)
                    .is_some_and(|ts| unix_now().saturating_add(PROXY_REFRESH_LEAD_SECS) >= ts);
                if near_expiry {
                    let refresh_fn = Arc::clone(refresh_fn);
                    let rt_id = runtime_id.to_string();
                    let tok_arc = Arc::clone(&proxy_token);
                    let exp_arc = Arc::clone(&expire_time);
                    let _ = std::thread::Builder::new()
                        .name("proton-colab-refresh".into())
                        .spawn(move || {
                            if let Ok(info) = refresh_fn(&rt_id) {
                                if let Ok(mut t) = tok_arc.lock() {
                                    *t = info.token;
                                }
                                if let Ok(mut e) = exp_arc.lock() {
                                    *e = info.expire_time;
                                }
                            }
                        });
                }
            }
        }

        match socket.read() {
            Ok(WsMessage::Text(text)) => {
                if let Some(message) = decode_ws_message(text.as_str()) {
                    if found.send(message).is_err() {
                        break;
                    }
                }
            }
            Ok(WsMessage::Ping(payload)) => {
                if let Err(err) = socket.send(WsMessage::Pong(payload)) {
                    set_ws_error(&last_error, &format!("Colab WebSocket pong error: {err}"));
                    break;
                }
            }
            Ok(WsMessage::Close(frame)) => {
                let reason = frame
                    .map(|f| f.reason.to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "remote kernel closed the connection".into());
                set_ws_error(&last_error, &reason);
                break;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(err))
                if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            Err(err) => {
                set_ws_error(&last_error, &format!("Colab WebSocket error: {err}"));
                break;
            }
        }
    }

    alive.store(false, Ordering::SeqCst);
}

fn set_ws_error(dst: &Arc<Mutex<String>>, msg: &str) {
    if let Ok(mut guard) = dst.lock() {
        *guard = msg.to_string();
    }
}

pub fn encode_ws_message(message: &Message) -> String {
    let parent = match &message.parent {
        Some(p) => serde_json::to_value(p).unwrap_or_else(|_| serde_json::json!({})),
        None => serde_json::json!({}),
    };
    let payload = serde_json::json!({
        "header": message.header,
        "parent_header": parent,
        "metadata": message.metadata,
        "content": message.content,
        "channel": "shell",
        "buffers": [],
    });
    payload.to_string()
}

pub fn decode_ws_message(raw: &str) -> Option<Message> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let header: Header = serde_json::from_value(value.get("header")?.clone()).ok()?;
    let parent = value
        .get("parent_header")
        .and_then(|v| serde_json::from_value::<Header>(v.clone()).ok());
    let metadata = value
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let content = value
        .get("content")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Some(Message {
        header,
        parent,
        metadata,
        content,
        blobs: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    #[test]
    fn strip_xssi_removes_google_prefix_only() {
        assert_eq!(strip_xssi(")]}'\n{\"ok\":true}"), "{\"ok\":true}");
        assert_eq!(strip_xssi(")]}'\r\n[1,2]"), "[1,2]");
        assert_eq!(strip_xssi("{\"ok\":true}"), "{\"ok\":true}");
    }

    #[test]
    fn parse_oauth_client_json_supports_installed_web_and_flat_formats() {
        let installed = r#"{"installed":{"client_id":"cid-1.apps.googleusercontent.com","client_secret":"sec-1"}}"#;
        assert_eq!(
            parse_oauth_client_json(installed).unwrap(),
            ("cid-1.apps.googleusercontent.com".into(), "sec-1".into())
        );

        let web = r#"{"web":{"client_id":"cid-2","client_secret":"sec-2"}}"#;
        assert_eq!(
            parse_oauth_client_json(web).unwrap(),
            ("cid-2".into(), "sec-2".into())
        );

        let flat = r#"{"client_id":"cid-3"}"#;
        assert_eq!(
            parse_oauth_client_json(flat).unwrap(),
            ("cid-3".into(), String::new())
        );
    }

    #[test]
    fn pkce_challenge_matches_rfc7636_test_vector() {
        // RFC 7636 Appendix B test vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn parse_rfc3339_unix_parses_utc_and_fractions() {
        assert_eq!(parse_rfc3339_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_unix("2026-11-25T00:00:00.5Z"), Some(1_795_564_800));
        assert_eq!(parse_rfc3339_unix("bad"), None);
    }

    fn read_http_request(stream: &mut TcpStream) -> (String, String, HashMap<String, String>, String) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(hdr_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let hdr_str = String::from_utf8_lossy(&buf[..hdr_end]).into_owned();
                let content_len = hdr_str
                    .lines()
                    .find_map(|line| {
                        let (k, v) = line.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                let body_start = hdr_end + 4;
                while buf.len() < body_start + content_len {
                    let m = stream.read(&mut chunk).unwrap_or(0);
                    if m == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..m]);
                }
                let mut lines = hdr_str.lines();
                let req_line = lines.next().unwrap_or("");
                let mut parts = req_line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let mut headers = HashMap::new();
                for line in lines {
                    if let Some((k, v)) = line.split_once(':') {
                        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                    }
                }
                let body = String::from_utf8_lossy(
                    &buf[body_start..buf.len().min(body_start + content_len)],
                )
                .into_owned();
                return (method, path, headers, body);
            }
        }
        (String::new(), String::new(), HashMap::new(), String::new())
    }

    fn write_http_json(stream: &mut TcpStream, status: u16, body: &str) {
        let resp = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
    }

    #[test]
    fn colab_client_parses_specs_ccu_and_create_runtime_lro() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

        let server = std::thread::spawn(move || {
            for _ in 0..6 {
                let (mut stream, _) = listener.accept().unwrap();
                let (method, path, headers, body) = read_http_request(&mut stream);
                assert_eq!(headers.get("authorization").map(String::as_str), Some("Bearer tok-1"));
                assert_eq!(headers.get("x-colab-client-agent").map(String::as_str), Some("vscode"));
                match (method.as_str(), path.split('?').next().unwrap()) {
                    ("GET", "/v1beta/runtimespecs") => write_http_json(
                        &mut stream,
                        200,
                        ")]}'\n{\"runtimeSpecs\":[{\"eligible\":true,\"key\":{\"variant\":\"VARIANT_TPU\",\"accelerator\":\"V5E1\",\"shape\":\"SHAPE_STANDARD\"}},{\"eligible\":true,\"key\":{\"variant\":\"VARIANT_GPU\",\"accelerator\":\"A100\",\"shape\":\"SHAPE_HIGHMEM\"}}]}",
                    ),
                    ("GET", "/v1beta/runtimes") => write_http_json(&mut stream, 200, "{\"runtimes\":[]}"),
                    ("GET", "/v1beta/subscription") => write_http_json(&mut stream, 200, "{\"name\":\"subscription\",\"tier\":\"SUBSCRIPTION_TIER_PRO_PLUS\"}"),
                    ("GET", "/v1/user-info") => write_http_json(
                        &mut stream,
                        200,
                        ")]}'\n{\"subscriptionTier\":\"SUBSCRIPTION_TIER_PRO_PLUS\",\"paidComputeUnitsBalance\":421.5,\"consumptionRateHourly\":5.3,\"assignmentsCount\":0}",
                    ),
                    ("POST", "/v1beta/runtimes") => {
                        let req: serde_json::Value = serde_json::from_str(&body).unwrap();
                        assert_eq!(req["runtimeSpec"]["variant"], "VARIANT_TPU");
                        assert_eq!(req["runtimeSpec"]["accelerator"], "V5E1");
                        write_http_json(&mut stream, 200, "{\"name\":\"operations/op-1\",\"done\":false}");
                    }
                    ("GET", "/v1/operations/op-1:wait") => {
                        let resp_body = serde_json::json!({
                            "name": "operations/op-1",
                            "done": true,
                            "response": {
                                "name": "runtimes/rt-99",
                                "runtimeSpec": {
                                    "variant": "VARIANT_TPU",
                                    "accelerator": "V5E1",
                                    "shape": "SHAPE_STANDARD"
                                },
                                "connectionInfo": {
                                    "url": "https://rt-99.prod.colab.dev",
                                    "token": "proxy-abc",
                                    "expireTime": "2026-11-25T03:00:00Z",
                                    "endpoint": "m-s-99"
                                }
                            }
                        })
                        .to_string();
                        write_http_json(&mut stream, 200, &resp_body);
                    }
                    other => panic!("unexpected mock request: {other:?}"),
                }
            }
        });

        let client = ColabClient::with_bases(&base, &base);
        let summary = client
            .account_summary("tok-1", Some("user@example.com".into()))
            .unwrap();
        assert_eq!(summary.tier, "SUBSCRIPTION_TIER_PRO_PLUS");
        assert_eq!(summary.ccu_balance, Some(421.5));
        assert_eq!(summary.specs.len(), 2);
        assert_eq!(summary.specs[0].accelerator, "V5E1");

        let runtime = client
            .create_runtime("tok-1", "TPU", "V5E1", "STANDARD")
            .unwrap();
        assert_eq!(runtime.runtime_id, "rt-99");
        assert_eq!(runtime.connection_info.token, "proxy-abc");
        assert_eq!(runtime_label(&runtime), "Colab · TPU v5e-1");

        server.join().unwrap();
    }

    #[test]
    fn colab_kernel_executes_interrupts_and_cleans_up_over_websocket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

        let server = std::thread::spawn(move || {
            // 1. POST /api/kernels
            let (mut s1, _) = listener.accept().unwrap();
            let (m1, p1, h1, _) = read_http_request(&mut s1);
            assert_eq!((m1.as_str(), p1.as_str()), ("POST", "/api/kernels"));
            assert_eq!(
                h1.get("x-colab-runtime-proxy-token").map(String::as_str),
                Some("proxy-secret")
            );
            assert_eq!(
                h1.get("x-colab-client-agent").map(String::as_str),
                Some("vscode")
            );
            write_http_json(&mut s1, 201, "{\"id\":\"kern-1\",\"name\":\"python3\"}");

            // 2. WebSocket /api/kernels/kern-1/channels
            let (s2, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept_hdr(
                s2,
                |req: &tungstenite::handshake::server::Request,
                 resp: tungstenite::handshake::server::Response| {
                    assert!(req.uri().path().ends_with("/api/kernels/kern-1/channels"));
                    assert_eq!(
                        req.headers()
                            .get("X-Colab-Runtime-Proxy-Token")
                            .and_then(|v| v.to_str().ok()),
                        Some("proxy-secret")
                    );
                    assert_eq!(
                        req.headers()
                            .get("X-Colab-Client-Agent")
                            .and_then(|v| v.to_str().ok()),
                        Some("vscode")
                    );
                    Ok(resp)
                },
            )
            .unwrap();

            // Receive execute_request and send stream + execute_result + execute_reply + idle
            let incoming = ws.read().unwrap();
            let parsed: serde_json::Value = match incoming {
                WsMessage::Text(txt) => serde_json::from_str(txt.as_str()).unwrap(),
                other => panic!("expected text frame, got {other:?}"),
            };
            assert_eq!(parsed["header"]["msg_type"], "execute_request");
            assert_eq!(parsed["content"]["code"], "print('hi'); 1 + 1");
            let parent = parsed["header"].clone();

            let send_msg = |ws: &mut tungstenite::WebSocket<TcpStream>, msg_type: &str, content: serde_json::Value| {
                let frame = serde_json::json!({
                    "header": {
                        "msg_id": format!("srv-{msg_type}"),
                        "session": "sess",
                        "username": "kernel",
                        "msg_type": msg_type,
                        "version": "5.3",
                    },
                    "parent_header": parent,
                    "metadata": {},
                    "content": content,
                    "channel": "iopub",
                })
                .to_string();
                ws.send(WsMessage::Text(frame.into())).unwrap();
            };

            send_msg(&mut ws, "stream", serde_json::json!({"name": "stdout", "text": "hi\n"}));
            send_msg(
                &mut ws,
                "execute_result",
                serde_json::json!({"execution_count": 1, "data": {"text/plain": "2"}}),
            );
            send_msg(
                &mut ws,
                "execute_reply",
                serde_json::json!({"status": "ok", "execution_count": 1}),
            );
            send_msg(&mut ws, "status", serde_json::json!({"execution_state": "idle"}));

            // 3. POST /api/kernels/kern-1/interrupt
            let (mut s3, _) = listener.accept().unwrap();
            let (m3, p3, _, _) = read_http_request(&mut s3);
            assert_eq!((m3.as_str(), p3.as_str()), ("POST", "/api/kernels/kern-1/interrupt"));
            write_http_json(&mut s3, 204, "");

            // 4. DELETE /api/kernels/kern-1 on drop
            let (mut s4, _) = listener.accept().unwrap();
            let (m4, p4, _, _) = read_http_request(&mut s4);
            assert_eq!((m4.as_str(), p4.as_str()), ("DELETE", "/api/kernels/kern-1"));
            write_http_json(&mut s4, 204, "");
        });

        let runtime = ColabRuntime {
            name: "runtimes/rt-1".into(),
            runtime_id: "rt-1".into(),
            variant: "VARIANT_GPU".into(),
            accelerator: "A100".into(),
            shape: "SHAPE_STANDARD".into(),
            connection_info: ColabConnectionInfo {
                url: base,
                token: "proxy-secret".into(),
                expire_time: "2099-01-01T00:00:00Z".into(),
                endpoint: "m-s-1".into(),
            },
        };

        let mut kernel = crate::Kernel::start_colab(&runtime, None).unwrap();
        let req_id = kernel.execute("print('hi'); 1 + 1").unwrap();

        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while seen.len() < 4 && Instant::now() < deadline {
            seen.extend(kernel.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(seen.len(), 4, "expected 4 Jupyter messages, got {seen:?}");
        assert_eq!(seen[0].msg_type(), "stream");
        assert_eq!(seen[0].parent_id(), Some(req_id.as_str()));
        assert_eq!(seen[1].msg_type(), "execute_result");
        assert_eq!(seen[2].msg_type(), "execute_reply");
        assert_eq!(seen[3].msg_type(), "status");

        kernel.interrupt().unwrap();
        drop(kernel);

        server.join().unwrap();
    }
}
