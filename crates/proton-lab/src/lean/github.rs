//! What GitHub says there is to install: Lean's releases, the last of them
//! Mathlib has tagged, and elan's. Read from its API and from Mathlib's own
//! `lean-toolchain` file — a few kilobytes, asked only when the scientist
//! asks for a plan.

use std::io::Read;
use std::time::Duration;

use serde_json::Value;

use super::{LeanDownload, LeanPlan, Platform, MATHLIB_DOWNLOAD, MATHLIB_SOURCES, MATHLIB_UNPACKED, TOOLCHAIN_GROWTH};

/// Lean's newest releases.
pub const LEAN_RELEASES: &str = "https://api.github.com/repos/leanprover/lean4/releases?per_page=20";
/// elan's newest release.
pub const ELAN_LATEST: &str = "https://api.github.com/repos/leanprover/elan/releases/latest";

/// Where Mathlib, at `tag`, says which Lean it builds with.
pub fn mathlib_toolchain_url(tag: &str) -> String {
    format!("https://raw.githubusercontent.com/leanprover-community/mathlib4/{tag}/lean-toolchain")
}

/// What asks: GitHub over the network, or a test's answers.
pub trait Fetch: Send + Sync {
    /// The status and body of a GET.
    fn get(&self, url: &str) -> Result<(u16, String), String>;
}

/// Nobody: what a Lean that must never reach the network asks, in tests.
pub struct Offline;

impl Fetch for Offline {
    fn get(&self, _: &str) -> Result<(u16, String), String> {
        Err("GitHub is not asked here".into())
    }
}

/// The network, as the literature reaches it: rustls with the platform's
/// certificates, https only, a name that says who is asking.
pub struct Live {
    agent: ureq::Agent,
}

impl Default for Live {
    fn default() -> Live {
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .tls_config(tls)
            .timeout_global(Some(Duration::from_secs(20)))
            .max_redirects(5)
            .build()
            .into();
        Live { agent }
    }
}

impl Fetch for Live {
    fn get(&self, url: &str) -> Result<(u16, String), String> {
        let response = self
            .agent
            .get(url)
            .header("User-Agent", &format!("Proton/{} (+https://github.com/razeenwasif/Proton)", env!("CARGO_PKG_VERSION")))
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let mut body = String::new();
        response.into_body().into_with_config().limit(8 * 1024 * 1024).reader().read_to_string(&mut body).map_err(|e| e.to_string())?;
        Ok((status, body))
    }
}

/// A release, as much of it as a plan needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub prerelease: bool,
    pub draft: bool,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub url: String,
    /// Its SHA-256, in hex, when GitHub gives one.
    pub sha256: Option<String>,
}

fn release_of(v: &Value) -> Option<Release> {
    let assets = v["assets"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: a["name"].as_str()?.to_string(),
                        size: a["size"].as_u64().unwrap_or(0),
                        url: a["browser_download_url"].as_str()?.to_string(),
                        sha256: a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).filter(|h| h.len() == 64).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release {
        tag: v["tag_name"].as_str()?.to_string(),
        prerelease: v["prerelease"].as_bool().unwrap_or(false),
        draft: v["draft"].as_bool().unwrap_or(false),
        assets,
    })
}

pub fn releases(json: &str) -> Result<Vec<Release>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("GitHub's list of releases did not read: {e}"))?;
    Ok(v.as_array().map(|list| list.iter().filter_map(release_of).collect()).unwrap_or_default())
}

pub fn release(json: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("GitHub's release did not read: {e}"))?;
    release_of(&v).ok_or_else(|| "GitHub's release named no tag".to_string())
}

/// Whether a tag is a stable release's, `v4.34.1`, and not a candidate's.
pub fn stable(tag: &str) -> bool {
    let Some(numbers) = tag.strip_prefix('v') else { return false };
    let parts: Vec<&str> = numbers.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The Lean version a `lean-toolchain` file names: `leanprover/lean4:v4.34.1` → `v4.34.1`.
pub fn toolchain(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let version = line.strip_prefix("leanprover/lean4:")?;
    (!version.is_empty() && !version.contains(char::is_whitespace)).then(|| version.to_string())
}

/// What GitHub says when it will not answer.
fn refused(status: u16, what: &str) -> String {
    match status {
        403 | 429 => "GitHub is limiting how often this address may ask it; try again in an hour".into(),
        _ => format!("GitHub answered {status} for {what}"),
    }
}

/// How many of Lean's newest stable releases are tried for one Mathlib has tagged.
const TRIED: usize = 6;

/// The plan: the newest stable Lean that Mathlib has tagged — Mathlib's tag
/// lags a Lean release by days — its toolchain for this machine, elan to
/// install it with, and what all of it costs to download and keep.
pub fn plan(fetch: &dyn Fetch, platform: &Platform, home: &str, free: u64, git: &str) -> Result<LeanPlan, String> {
    let (status, body) = fetch.get(LEAN_RELEASES)?;
    if status != 200 {
        return Err(refused(status, "Lean's releases"));
    }
    let candidates: Vec<Release> = releases(&body)?.into_iter().filter(|r| !r.draft && !r.prerelease && stable(&r.tag)).take(TRIED).collect();
    let mut chosen = None;
    for release in candidates {
        let (status, body) = fetch.get(&mathlib_toolchain_url(&release.tag))?;
        match status {
            200 if toolchain(&body).as_deref() == Some(release.tag.as_str()) => {
                chosen = Some(release);
                break;
            }
            // Not tagged yet, or tagged for another Lean.
            200 | 404 => continue,
            other => return Err(refused(other, "Mathlib's toolchain")),
        }
    }
    let lean = chosen.ok_or("none of Lean's newest releases has a Mathlib to go with it yet")?;
    let name = format!("lean-{}-{}.tar.zst", lean.tag.trim_start_matches('v'), platform.lean);
    let toolchain_asset = lean.assets.iter().find(|a| a.name == name).ok_or_else(|| format!("Lean {} has no toolchain for this machine ({name})", lean.tag))?;

    let (status, body) = fetch.get(ELAN_LATEST)?;
    if status != 200 {
        return Err(refused(status, "elan's release"));
    }
    let elan = release(&body)?;
    let elan_name = format!("elan-{}.tar.gz", platform.elan);
    let elan_asset = elan.assets.iter().find(|a| a.name == elan_name).ok_or_else(|| format!("elan {} has no build for this machine ({elan_name})", elan.tag))?;

    let downloads = vec![
        LeanDownload { what: format!("elan {}", elan.tag), host: "github.com".into(), bytes: elan_asset.size, exact: true, verified: elan_asset.sha256.is_some() },
        // elan downloads and unpacks it — macOS's tar cannot read zstd — and
        // checks it against no digest.
        LeanDownload { what: format!("Lean {} toolchain", lean.tag), host: "github.com".into(), bytes: toolchain_asset.size, exact: true, verified: false },
        LeanDownload { what: format!("Mathlib {} sources", lean.tag), host: "github.com".into(), bytes: MATHLIB_SOURCES, exact: false, verified: false },
        LeanDownload { what: "Mathlib's prebuilt library".into(), host: "cache.mathlib.org".into(), bytes: MATHLIB_DOWNLOAD, exact: false, verified: false },
    ];
    let unpacked = (toolchain_asset.size as f64 * TOOLCHAIN_GROWTH) as u64;
    let download: u64 = downloads.iter().map(|d| d.bytes).sum();
    // At its fullest, while the cache is unpacked and before what was downloaded is deleted.
    let disk = elan_asset.size + toolchain_asset.size + unpacked + MATHLIB_SOURCES + MATHLIB_DOWNLOAD + MATHLIB_UNPACKED;
    let kept = unpacked + MATHLIB_SOURCES + MATHLIB_UNPACKED;
    Ok(LeanPlan {
        lean: lean.tag.clone(),
        mathlib: lean.tag.clone(),
        elan: elan.tag.clone(),
        toolchain: format!("leanprover/lean4:{}", lean.tag),
        downloads,
        download,
        disk,
        kept,
        free,
        fits: free >= disk,
        home: home.to_string(),
        git: git.to_string(),
        elan_url: elan_asset.url.clone(),
        elan_sha256: elan_asset.sha256.clone(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    pub const RELEASES: &str = include_str!("../../fixtures/github/lean4-releases.json");
    pub const ELAN: &str = include_str!("../../fixtures/github/elan-latest.json");

    /// Answers by URL, and remembers what was asked.
    #[derive(Default)]
    pub struct Script {
        pub answers: BTreeMap<String, (u16, String)>,
        pub asked: Mutex<Vec<String>>,
    }

    impl Script {
        /// GitHub as recorded on 2026-10-06, with Mathlib's tags as `tagged` says.
        pub fn recorded(tagged: &[&str]) -> Script {
            let mut answers = BTreeMap::new();
            answers.insert(LEAN_RELEASES.to_string(), (200, RELEASES.to_string()));
            answers.insert(ELAN_LATEST.to_string(), (200, ELAN.to_string()));
            for tag in tagged {
                answers.insert(mathlib_toolchain_url(tag), (200, format!("leanprover/lean4:{tag}\n")));
            }
            Script { answers, asked: Mutex::default() }
        }
    }

    impl Fetch for Script {
        fn get(&self, url: &str) -> Result<(u16, String), String> {
            self.asked.lock().unwrap().push(url.to_string());
            Ok(self.answers.get(url).cloned().unwrap_or((404, "404: Not Found".into())))
        }
    }

    const MAC: Platform = Platform { elan: "aarch64-apple-darwin", lean: "darwin_aarch64" };

    #[test]
    fn releases_are_read_and_only_stable_ones_count() {
        let all = releases(RELEASES).unwrap();
        assert_eq!(all.len(), 8);
        let stable_tags: Vec<&str> = all.iter().filter(|r| !r.prerelease && stable(&r.tag)).map(|r| r.tag.as_str()).collect();
        assert_eq!(stable_tags, ["v4.34.1", "v4.34.0", "v4.33.1"]);
        let asset = all[1].assets.iter().find(|a| a.name == "lean-4.34.1-darwin_aarch64.tar.zst").unwrap();
        assert!(asset.size > 400_000_000 && asset.sha256.as_deref().is_some_and(|h| h.len() == 64), "{asset:?}");
        assert!(!stable("v4.35.0-rc3") && !stable("4.34.1") && !stable("v4.34"));
        assert_eq!(toolchain("leanprover/lean4:v4.34.1\n"), Some("v4.34.1".into()));
        assert_eq!(toolchain("404: Not Found"), None);
    }

    #[test]
    fn the_plan_is_the_newest_lean_mathlib_has_tagged_with_what_it_costs() {
        // Mathlib has not tagged v4.34.1 yet: the plan is v4.34.0, and no candidate is ever planned.
        let script = Script::recorded(&["v4.34.0", "v4.33.1"]);
        let plan = plan(&script, &MAC, "/home/you/.proton/lean", 80_000_000_000, "/opt/homebrew/bin/git").unwrap();
        assert_eq!((plan.lean.as_str(), plan.mathlib.as_str(), plan.toolchain.as_str()), ("v4.34.0", "v4.34.0", "leanprover/lean4:v4.34.0"));
        assert_eq!(plan.elan, "v4.2.4");
        let asked = script.asked.lock().unwrap().clone();
        assert!(asked.contains(&mathlib_toolchain_url("v4.34.1")) && !asked.iter().any(|u| u.contains("-rc")), "{asked:?}");
        let toolchain = &plan.downloads[1];
        assert!(toolchain.exact && !toolchain.verified && toolchain.bytes > 400_000_000, "elan checks the toolchain against no digest: {toolchain:?}");
        assert!(plan.downloads[0].verified && plan.downloads[0].bytes == 2_190_845, "elan, from its own release");
        assert!(!plan.downloads[3].exact && plan.downloads[3].host == "cache.mathlib.org", "Mathlib's library is an estimate, and says so");
        assert_eq!(plan.download, plan.downloads.iter().map(|d| d.bytes).sum::<u64>());
        assert!(plan.disk > plan.kept && plan.kept > 10_000_000_000 && plan.fits);
        assert!(plan.elan_url.ends_with("elan-aarch64-apple-darwin.tar.gz") && plan.elan_sha256.is_some());

        let short = super::plan(&Script::recorded(&["v4.34.1"]), &MAC, "/h", 5_000_000_000, "git").unwrap();
        assert_eq!((short.lean.as_str(), short.fits), ("v4.34.1", false), "too little room is said, not refused");
    }

    #[test]
    fn a_download_github_gives_no_digest_for_is_said_to_be_unverified() {
        let mut script = Script::recorded(&["v4.34.1"]);
        assert!(ELAN.contains("\"digest\""));
        script.answers.insert(ELAN_LATEST.to_string(), (200, ELAN.replace("\"digest\"", "\"withheld\"")));
        let plan = plan(&script, &MAC, "/h", u64::MAX, "git").unwrap();
        assert!(!plan.downloads[0].verified && plan.elan_sha256.is_none(), "{:?}", plan.downloads[0]);
    }

    #[test]
    fn what_github_will_not_say_is_said() {
        let mut script = Script::recorded(&[]);
        let why = plan(&script, &MAC, "/h", u64::MAX, "git").unwrap_err();
        assert!(why.contains("none of Lean's newest releases"), "{why}");
        script.answers.insert(LEAN_RELEASES.to_string(), (403, "{\"message\": \"API rate limit exceeded\"}".into()));
        assert!(plan(&script, &MAC, "/h", u64::MAX, "git").unwrap_err().contains("try again in an hour"));
        let windows = Platform { elan: "x86_64-pc-windows-msvc", lean: "windows_arm" };
        let why = plan(&Script::recorded(&["v4.34.1"]), &windows, "/h", u64::MAX, "git").unwrap_err();
        assert!(why.contains("no toolchain for this machine"), "{why}");
    }
}
