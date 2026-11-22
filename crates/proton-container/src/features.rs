//! Dev Container Features resolution, ordering, OCI registry fetching, and
//! wrapper `Dockerfile` synthesis.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::config::{LifecycleCommand, MountSpec, ResolvedConfig};
use crate::jsonc;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeatureOptionDef {
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeatureMetadata {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, FeatureOptionDef>,
    #[serde(default)]
    pub installs_after: Vec<String>,
    #[serde(default)]
    pub depends_on: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub container_env: BTreeMap<String, String>,
    #[serde(default)]
    pub mounts: Vec<MountSpec>,
    #[serde(default)]
    pub cap_add: Vec<String>,
    #[serde(default)]
    pub security_opt: Vec<String>,
    #[serde(default)]
    pub privileged: bool,
    #[serde(default)]
    pub init: bool,
    #[serde(default)]
    pub on_create_command: Option<LifecycleCommand>,
    #[serde(default)]
    pub update_content_command: Option<LifecycleCommand>,
    #[serde(default)]
    pub post_create_command: Option<LifecycleCommand>,
    #[serde(default)]
    pub post_start_command: Option<LifecycleCommand>,
    #[serde(default)]
    pub post_attach_command: Option<LifecycleCommand>,
}

#[derive(Debug, Clone)]
pub struct ResolvedFeature {
    pub ref_id: String,
    pub dir: PathBuf,
    pub metadata: FeatureMetadata,
    pub env_options: BTreeMap<String, String>,
}

/// Prepared build context directory and merged metadata for building a base
/// image layered with one or more Dev Container Features.
pub struct PreparedFeatures {
    pub context_dir: PathBuf,
    pub dockerfile: PathBuf,
    pub container_env: BTreeMap<String, String>,
    pub mounts: Vec<MountSpec>,
    pub cap_add: Vec<String>,
    pub security_opt: Vec<String>,
    pub privileged: bool,
    pub init: bool,
    pub on_create_commands: Vec<LifecycleCommand>,
    pub update_content_commands: Vec<LifecycleCommand>,
    pub post_create_commands: Vec<LifecycleCommand>,
    pub post_start_commands: Vec<LifecycleCommand>,
    pub post_attach_commands: Vec<LifecycleCommand>,
}

impl Drop for PreparedFeatures {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.context_dir);
    }
}

/// Normalize a Dev Container Feature option key into the POSIX environment
/// variable name expected by `install.sh`.
pub fn normalize_option_name(raw: &str) -> String {
    let mut mapped: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    while mapped.starts_with(|c: char| c.is_ascii_digit() || c == '_') {
        mapped.remove(0);
    }
    if mapped.is_empty() {
        "OPTION".to_string()
    } else {
        mapped
    }
}

fn scalar_to_env_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Merge `devcontainer-feature.json` defaults with user-supplied option value
/// into normalized `ENV_NAME -> value` pairs.
pub fn resolve_option_env(
    meta: &FeatureMetadata,
    user_value: &serde_json::Value,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for (k, def) in &meta.options {
        if let Some(default_val) = &def.default {
            env.insert(normalize_option_name(k), scalar_to_env_string(default_val));
        }
    }

    match user_value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                env.insert(normalize_option_name(k), scalar_to_env_string(v));
            }
        }
        serde_json::Value::String(s) => {
            env.insert("VERSION".to_string(), s.clone());
        }
        serde_json::Value::Number(n) => {
            env.insert("VERSION".to_string(), n.to_string());
        }
        serde_json::Value::Bool(_) | serde_json::Value::Null => {}
        other => {
            env.insert("VERSION".to_string(), other.to_string());
        }
    }
    env
}

fn strip_tag(ref_id: &str) -> &str {
    if ref_id.starts_with("./") || ref_id.starts_with("../") {
        return ref_id;
    }
    match ref_id.rsplit_once(':') {
        Some((prefix, _)) if !prefix.contains('/') => ref_id,
        Some((prefix, tag)) if !tag.contains('/') => prefix,
        _ => ref_id,
    }
}

fn ref_matches(a: &str, b: &str) -> bool {
    a == b || strip_tag(a) == strip_tag(b)
}

/// Sort resolved features obeying `overrideFeatureInstallOrder`, `dependsOn`,
/// and `installsAfter`, falling back deterministically on cycles.
pub fn order_features(
    features: Vec<ResolvedFeature>,
    override_order: &[String],
) -> Vec<ResolvedFeature> {
    let n = features.len();
    if n <= 1 {
        return features;
    }

    let priority = |ref_id: &str| -> (usize, String) {
        let pos = override_order
            .iter()
            .position(|o| ref_matches(ref_id, o))
            .unwrap_or(usize::MAX);
        (pos, ref_id.to_string())
    };

    // Build directed edges u -> v meaning feature u must install before feature v.
    let mut after: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (i, feat) in features.iter().enumerate() {
        for dep in feat
            .metadata
            .installs_after
            .iter()
            .chain(feat.metadata.depends_on.keys())
        {
            if let Some(dep_idx) = features.iter().position(|f| ref_matches(&f.ref_id, dep)) {
                if dep_idx != i {
                    after[dep_idx].insert(i);
                }
            }
        }
    }

    for pair in override_order.windows(2) {
        if let (Some(a), Some(b)) = (
            features.iter().position(|f| ref_matches(&f.ref_id, &pair[0])),
            features.iter().position(|f| ref_matches(&f.ref_id, &pair[1])),
        ) {
            if a != b {
                after[a].insert(b);
            }
        }
    }

    let mut indeg = vec![0usize; n];
    for edges in &after {
        for &v in edges {
            indeg[v] += 1;
        }
    }

    let mut remaining: BTreeSet<usize> = (0..n).collect();
    let mut ordered_indices = Vec::with_capacity(n);

    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .copied()
            .filter(|&i| indeg[i] == 0)
            .min_by_key(|&i| priority(&features[i].ref_id))
            .or_else(|| {
                // Break cycles deterministically using override priority then ref_id.
                remaining
                    .iter()
                    .copied()
                    .min_by_key(|&i| priority(&features[i].ref_id))
            })
            .expect("remaining is non-empty");

        remaining.remove(&next);
        ordered_indices.push(next);
        for &v in &after[next] {
            if remaining.contains(&v) && indeg[v] > 0 {
                indeg[v] -= 1;
            }
        }
    }

    let mut slots: Vec<Option<ResolvedFeature>> = features.into_iter().map(Some).collect();
    ordered_indices
        .into_iter()
        .map(|i| slots[i].take().expect("unique index"))
        .collect()
}

/// Resolve, fetch/copy, order, and synthesize a wrapper `Dockerfile` for
/// `resolved.features` layered on top of `base_image`.
pub fn prepare<F>(
    resolved: &ResolvedConfig,
    base_image: &str,
    mut on_log: F,
) -> Result<PreparedFeatures, String>
where
    F: FnMut(String),
{
    let config_dir = resolved.config_path.parent().unwrap_or(&resolved.host_root);
    let mut loaded = Vec::with_capacity(resolved.features.len());

    for (feat_ref, user_val) in &resolved.features {
        on_log(format!("Resolving feature {feat_ref}…"));
        let dir = if feat_ref.starts_with("./") || feat_ref.starts_with("../") {
            resolve_local_feature(config_dir, feat_ref)?
        } else {
            fetch_oci_feature(feat_ref, &mut on_log)?
        };
        let meta = read_feature_metadata(&dir, feat_ref)?;
        let env_options = resolve_option_env(&meta, user_val);
        loaded.push(ResolvedFeature {
            ref_id: feat_ref.clone(),
            dir,
            metadata: meta,
            env_options,
        });
    }

    let ordered = order_features(loaded, &resolved.override_feature_install_order);

    let tag_hash = &resolved.config_hash[..resolved.config_hash.len().min(8)];
    let context_dir = std::env::temp_dir().join(format!(
        "proton-dc-features-{}-{tag_hash}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&context_dir);
    fs::create_dir_all(context_dir.join("features"))
        .map_err(|e| format!("failed creating features build context: {e}"))?;

    let remote_user = resolved.effective_user.as_deref().unwrap_or("root");
    let home_dir = if remote_user == "root" {
        "/root".to_string()
    } else {
        format!("/home/{remote_user}")
    };

    let mut dockerfile_lines = vec![format!("FROM {base_image}")];
    let mut container_env = BTreeMap::new();
    let mut mounts = Vec::new();
    let mut cap_add = Vec::new();
    let mut security_opt = Vec::new();
    let mut privileged = false;
    let mut init = false;
    let mut on_create_commands = Vec::new();
    let mut update_content_commands = Vec::new();
    let mut post_create_commands = Vec::new();
    let mut post_start_commands = Vec::new();
    let mut post_attach_commands = Vec::new();

    for (idx, feat) in ordered.iter().enumerate() {
        let dest = context_dir.join("features").join(idx.to_string());
        copy_dir_recursive(&feat.dir, &dest)?;

        let target_dir = format!("/tmp/dev-container-features/{idx}");
        dockerfile_lines.push(format!("COPY features/{idx} {target_dir}"));

        let mut env_prefix = vec![
            format!("_REMOTE_USER={}", shell_quote(remote_user)),
            format!("_CONTAINER_USER={}", shell_quote(remote_user)),
            format!("_REMOTE_USER_HOME={}", shell_quote(&home_dir)),
            format!("_CONTAINER_USER_HOME={}", shell_quote(&home_dir)),
        ];
        for (k, v) in &feat.env_options {
            env_prefix.push(format!("{k}={}", shell_quote(v)));
        }
        dockerfile_lines.push(format!(
            "RUN cd {target_dir} && chmod +x ./install.sh && {} ./install.sh",
            env_prefix.join(" ")
        ));

        for (k, v) in &feat.metadata.container_env {
            container_env.insert(k.clone(), v.clone());
        }
        mounts.extend(feat.metadata.mounts.clone());
        for c in &feat.metadata.cap_add {
            if !cap_add.contains(c) {
                cap_add.push(c.clone());
            }
        }
        for s in &feat.metadata.security_opt {
            if !security_opt.contains(s) {
                security_opt.push(s.clone());
            }
        }
        privileged |= feat.metadata.privileged;
        init |= feat.metadata.init;
        if let Some(cmd) = &feat.metadata.on_create_command {
            on_create_commands.push(cmd.clone());
        }
        if let Some(cmd) = &feat.metadata.update_content_command {
            update_content_commands.push(cmd.clone());
        }
        if let Some(cmd) = &feat.metadata.post_create_command {
            post_create_commands.push(cmd.clone());
        }
        if let Some(cmd) = &feat.metadata.post_start_command {
            post_start_commands.push(cmd.clone());
        }
        if let Some(cmd) = &feat.metadata.post_attach_command {
            post_attach_commands.push(cmd.clone());
        }
    }

    dockerfile_lines.push("RUN rm -rf /tmp/dev-container-features".to_string());
    let dockerfile = context_dir.join("Dockerfile");
    fs::write(&dockerfile, format!("{}\n", dockerfile_lines.join("\n")))
        .map_err(|e| format!("failed writing synthesized features Dockerfile: {e}"))?;

    Ok(PreparedFeatures {
        context_dir,
        dockerfile,
        container_env,
        mounts,
        cap_add,
        security_opt,
        privileged,
        init,
        on_create_commands,
        update_content_commands,
        post_create_commands,
        post_start_commands,
        post_attach_commands,
    })
}

fn resolve_local_feature(config_dir: &Path, feat_ref: &str) -> Result<PathBuf, String> {
    let dir = config_dir.join(feat_ref);
    if !dir.is_dir() {
        return Err(format!(
            "local feature directory not found: {}",
            dir.display()
        ));
    }
    if !dir.join("install.sh").is_file() {
        return Err(format!(
            "local feature missing install.sh: {}",
            dir.display()
        ));
    }
    Ok(dir)
}

fn read_feature_metadata(dir: &Path, feat_ref: &str) -> Result<FeatureMetadata, String> {
    let meta_path = dir.join("devcontainer-feature.json");
    if !meta_path.is_file() {
        return Ok(FeatureMetadata::default());
    }
    let raw = fs::read_to_string(&meta_path)
        .map_err(|e| format!("failed reading {feat_ref} metadata: {e}"))?;
    let stripped = jsonc::strip(&raw);
    serde_json::from_str::<FeatureMetadata>(&stripped)
        .map_err(|e| format!("invalid {}: {e}", meta_path.display()))
}

fn parse_oci_ref(raw_ref: &str) -> Result<(String, String, String), String> {
    let (without_tag, tag) = match raw_ref.rsplit_once(':') {
        Some((left, t)) if !t.contains('/') && !t.is_empty() => (left, t.to_string()),
        _ => (raw_ref, "latest".to_string()),
    };
    let (registry, repo) = without_tag
        .split_once('/')
        .ok_or_else(|| format!("feature reference '{raw_ref}' must be <registry>/<repo>[:<tag>]"))?;
    if registry.is_empty() || repo.is_empty() {
        return Err(format!("invalid OCI feature reference: '{raw_ref}'"));
    }
    Ok((registry.to_string(), repo.to_string(), tag))
}

fn feature_cache_dir(raw_ref: &str) -> PathBuf {
    if let Some(dir) = std::env::var_os("PROTON_FEATURE_CACHE") {
        let s = dir.to_string_lossy();
        if !s.trim().is_empty() {
            let mut h = Sha256::new();
            h.update(raw_ref.as_bytes());
            let key = format!("{:x}", h.finalize());
            return PathBuf::from(s.trim()).join(&key[..16]);
        }
    }
    let mut h = Sha256::new();
    h.update(raw_ref.as_bytes());
    let key = format!("{:x}", h.finalize());
    std::env::temp_dir()
        .join("proton-devcontainer-features-cache")
        .join(&key[..16])
}

fn fetch_oci_feature<F>(raw_ref: &str, on_log: &mut F) -> Result<PathBuf, String>
where
    F: FnMut(String),
{
    let (registry, repo, tag) = parse_oci_ref(raw_ref)?;
    let dest = feature_cache_dir(raw_ref);
    if tag != "latest" && dest.join("install.sh").is_file() {
        on_log(format!("Using cached feature {raw_ref}"));
        return Ok(dest);
    }

    let scheme = if registry.starts_with("127.0.0.1") || registry.starts_with("localhost") {
        "http"
    } else {
        "https"
    };
    let manifest_url = format!("{scheme}://{registry}/v2/{repo}/manifests/{tag}");
    let accept = "application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";

    let mut token: Option<String> = None;
    let manifest_bytes = match curl_get_with_headers(&manifest_url, &[("Accept", accept)], None)? {
        CurlResp::Ok(bytes) => bytes,
        CurlResp::Unauthorized(www_auth) => {
            token = Some(fetch_bearer_token(&www_auth)?);
            match curl_get_with_headers(
                &manifest_url,
                &[("Accept", accept)],
                token.as_deref(),
            )? {
                CurlResp::Ok(bytes) => bytes,
                CurlResp::Unauthorized(_) => {
                    return Err(format!("OCI registry authentication failed for {raw_ref}"))
                }
            }
        }
    };

    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("invalid OCI manifest for {raw_ref}: {e}"))?;
    let layers = manifest["layers"]
        .as_array()
        .ok_or_else(|| format!("OCI manifest for {raw_ref} has no layers"))?;
    let digest = layers
        .iter()
        .find(|l| {
            l["mediaType"]
                .as_str()
                .is_some_and(|m| m.contains("tar") || m.contains("devcontainer"))
        })
        .or_else(|| layers.first())
        .and_then(|l| l["digest"].as_str())
        .ok_or_else(|| format!("OCI manifest for {raw_ref} missing layer digest"))?;

    let blob_url = format!("{scheme}://{registry}/v2/{repo}/blobs/{digest}");
    let tgz_bytes = match curl_get_with_headers(&blob_url, &[], token.as_deref())? {
        CurlResp::Ok(bytes) => bytes,
        CurlResp::Unauthorized(www_auth) => {
            let tok = fetch_bearer_token(&www_auth)?;
            match curl_get_with_headers(&blob_url, &[], Some(&tok))? {
                CurlResp::Ok(bytes) => bytes,
                CurlResp::Unauthorized(_) => {
                    return Err(format!("OCI blob auth failed for {raw_ref}"))
                }
            }
        }
    };

    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest)
        .map_err(|e| format!("failed creating feature cache dir: {e}"))?;
    let tgz_path = dest.join("feature.tgz");
    fs::write(&tgz_path, &tgz_bytes)
        .map_err(|e| format!("failed writing feature archive: {e}"))?;

    let untar = Command::new("tar")
        .args(["-xzf"])
        .arg(&tgz_path)
        .arg("-C")
        .arg(&dest)
        .output()
        .map_err(|e| format!("failed running tar to extract {raw_ref}: {e}"))?;
    let _ = fs::remove_file(&tgz_path);
    if !untar.status.success() {
        return Err(format!(
            "tar failed extracting {raw_ref}: {}",
            String::from_utf8_lossy(&untar.stderr).trim()
        ));
    }

    if !dest.join("install.sh").is_file() {
        return Err(format!(
            "downloaded feature {raw_ref} missing install.sh"
        ));
    }
    Ok(dest)
}

enum CurlResp {
    Ok(Vec<u8>),
    Unauthorized(String),
}

fn curl_get_with_headers(
    url: &str,
    headers: &[(&str, &str)],
    bearer: Option<&str>,
) -> Result<CurlResp, String> {
    let hdr_file = std::env::temp_dir().join(format!(
        "proton-oci-hdr-{}.txt",
        std::process::id()
    ));
    let _ = fs::remove_file(&hdr_file);

    let mut cmd = Command::new("curl");
    cmd.args(["-sSL", "-D"]).arg(&hdr_file);
    for (k, v) in headers {
        cmd.args(["-H", &format!("{k}: {v}")]);
    }
    if let Some(tok) = bearer {
        cmd.args(["-H", &format!("Authorization: Bearer {tok}")]);
    }
    cmd.args(["-w", "\n__STATUS__:%{http_code}", url]);

    let out = cmd
        .output()
        .map_err(|e| format!("failed to run curl for {url}: {e}"))?;
    let hdrs = fs::read_to_string(&hdr_file).unwrap_or_default();
    let _ = fs::remove_file(&hdr_file);

    if !out.status.success() {
        return Err(format!(
            "curl failed for {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    let marker = b"\n__STATUS__:";
    let Some(pos) = out
        .stdout
        .windows(marker.len())
        .rposition(|w| w == marker)
    else {
        return Err(format!("unexpected response framing from {url}"));
    };
    let body = out.stdout[..pos].to_vec();
    let code_str = String::from_utf8_lossy(&out.stdout[pos + marker.len()..])
        .trim()
        .to_string();
    let code: u16 = code_str.parse().unwrap_or(0);

    if code == 401 {
        if let Some(auth_line) = hdrs
            .lines()
            .rev()
            .find(|l| l.to_ascii_lowercase().starts_with("www-authenticate:"))
        {
            return Ok(CurlResp::Unauthorized(auth_line.to_string()));
        }
        return Err(format!("HTTP 401 without WWW-Authenticate for {url}"));
    }
    if !(200..300).contains(&code) {
        return Err(format!("HTTP {code} fetching {url}"));
    }
    Ok(CurlResp::Ok(body))
}

fn fetch_bearer_token(www_auth: &str) -> Result<String, String> {
    let extract = |key: &str| -> Option<String> {
        let needle = format!("{key}=\"");
        let start = www_auth.find(&needle)? + needle.len();
        let rest = &www_auth[start..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    };
    let realm = extract("realm")
        .ok_or_else(|| format!("WWW-Authenticate missing realm: {www_auth}"))?;
    let mut url = realm;
    let mut sep = if url.contains('?') { '&' } else { '?' };
    if let Some(svc) = extract("service") {
        url.push(sep);
        url.push_str(&format!("service={svc}"));
        sep = '&';
    }
    if let Some(scope) = extract("scope") {
        url.push(sep);
        url.push_str(&format!("scope={scope}"));
    }

    let body = match curl_get_with_headers(&url, &[], None)? {
        CurlResp::Ok(bytes) => bytes,
        CurlResp::Unauthorized(_) => {
            return Err("bearer token endpoint returned 401".to_string())
        }
    };
    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| format!("invalid bearer token JSON: {e}"))?;
    parsed["token"]
        .as_str()
        .or_else(|| parsed["access_token"].as_str())
        .map(str::to_string)
        .ok_or_else(|| "OCI token response missing token".to_string())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {e}", dst.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("readdir {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            fs::copy(&path, &target)
                .map_err(|e| format!("copy {} -> {}: {e}", path.display(), target.display()))?;
        }
    }
    Ok(())
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_option_names_and_merges_defaults() {
        assert_eq!(normalize_option_name("version"), "VERSION");
        assert_eq!(normalize_option_name("install-zsh"), "INSTALL_ZSH");
        assert_eq!(normalize_option_name("12_my.opt"), "MY_OPT");

        let mut meta = FeatureMetadata::default();
        meta.options.insert(
            "version".into(),
            FeatureOptionDef {
                default: Some(serde_json::json!("latest")),
            },
        );
        meta.options.insert(
            "install-extra".into(),
            FeatureOptionDef {
                default: Some(serde_json::json!(false)),
            },
        );

        let merged = resolve_option_env(
            &meta,
            &serde_json::json!({ "install-extra": true }),
        );
        assert_eq!(merged.get("VERSION").map(String::as_str), Some("latest"));
        assert_eq!(
            merged.get("INSTALL_EXTRA").map(String::as_str),
            Some("true")
        );

        let short = resolve_option_env(&meta, &serde_json::json!("3.12"));
        assert_eq!(short.get("VERSION").map(String::as_str), Some("3.12"));
    }

    #[test]
    fn orders_features_by_dependencies_and_override_order() {
        let make = |id: &str, after: &[&str]| ResolvedFeature {
            ref_id: id.into(),
            dir: PathBuf::from("."),
            metadata: FeatureMetadata {
                installs_after: after.iter().map(|s| (*s).to_string()).collect(),
                ..Default::default()
            },
            env_options: BTreeMap::new(),
        };

        let ordered = order_features(
            vec![
                make("ghcr.io/a/node:1", &["ghcr.io/a/common"]),
                make("ghcr.io/a/common:2", &[]),
                make("ghcr.io/a/rust:1", &[]),
            ],
            &["ghcr.io/a/rust".into()],
        );
        let ids: Vec<_> = ordered.iter().map(|f| f.ref_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["ghcr.io/a/rust:1", "ghcr.io/a/common:2", "ghcr.io/a/node:1"]
        );
    }

    #[test]
    fn prepares_local_feature_wrapper_dockerfile_and_merges_metadata() {
        let ws = std::env::temp_dir().join(format!("proton-feat-prep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&ws);
        let feat_dir = ws.join(".devcontainer/hello");
        fs::create_dir_all(&feat_dir).unwrap();
        fs::write(feat_dir.join("install.sh"), "#!/bin/sh\ntouch /ok\n").unwrap();
        fs::write(
            feat_dir.join("devcontainer-feature.json"),
            r#"{
              "id": "hello",
              "options": { "greeting": { "default": "hi" } },
              "containerEnv": { "HELLO_ENV": "1" },
              "capAdd": ["SYS_PTRACE"],
              "init": true,
              "postCreateCommand": "echo feat-post-create"
            }"#,
        )
        .unwrap();

        let cfg_path = ws.join(".devcontainer/devcontainer.json");
        let raw = r#"{
          "image": "alpine:3.22",
          "remoteUser": "vscode",
          "features": { "./hello": { "greeting": "hola" } }
        }"#;
        fs::write(&cfg_path, raw).unwrap();
        let resolved = ResolvedConfig::load(&ws, &cfg_path).unwrap();

        let mut logs = Vec::new();
        let prepared = prepare(&resolved, "alpine:3.22", |l| logs.push(l)).unwrap();
        let df = fs::read_to_string(&prepared.dockerfile).unwrap();
        assert!(df.contains("FROM alpine:3.22"));
        assert!(df.contains("COPY features/0 /tmp/dev-container-features/0"));
        assert!(df.contains("GREETING='hola'"));
        assert!(df.contains("_REMOTE_USER='vscode'"));
        assert!(df.contains("_REMOTE_USER_HOME='/home/vscode'"));
        assert_eq!(
            prepared.container_env.get("HELLO_ENV").map(String::as_str),
            Some("1")
        );
        assert_eq!(prepared.cap_add, vec!["SYS_PTRACE"]);
        assert!(prepared.init);
        assert_eq!(prepared.post_create_commands.len(), 1);

        let ctx_dir = prepared.context_dir.clone();
        drop(prepared);
        assert!(!ctx_dir.exists(), "Drop must clean up temporary context_dir");
        let _ = fs::remove_dir_all(&ws);
    }
}
