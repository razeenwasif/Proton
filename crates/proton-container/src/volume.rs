//! Named-volume helpers and cloning a Git repository directly into a named
//! container volume.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::config::MountSpec;
use crate::runtime::Runtime;

/// Extract unique named volumes referenced with `type=volume,source=<name>`
/// from `workspaceMount` and `mounts`.
pub fn extract_volume_names(workspace_mount: Option<&str>, mounts: &[MountSpec]) -> Vec<String> {
    let mut out = Vec::new();
    let mut push_unique = |name: &str| {
        let trimmed = name.trim();
        if !trimmed.is_empty() && !out.iter().any(|existing| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    };

    if let Some(wm) = workspace_mount {
        let mut is_volume = false;
        let mut source: Option<&str> = None;
        for part in wm.split(',') {
            let Some((k, v)) = part.trim().split_once('=') else {
                continue;
            };
            match k.trim() {
                "type" if v.trim() == "volume" => is_volume = true,
                "source" | "src" => source = Some(v.trim()),
                _ => {}
            }
        }
        if is_volume {
            if let Some(src) = source {
                push_unique(src);
            }
        }
    }

    for m in mounts {
        if m.kind == "volume" {
            if let Some(src) = &m.source {
                push_unique(src);
            }
        }
    }

    out
}

/// Derive a deterministic Docker/Graviton volume name and repository slug from
/// a Git remote URL.
pub fn volume_name_for_url(url: &str) -> (String, String) {
    let trimmed = url.trim().trim_end_matches('/');
    let without_git = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let tail = without_git
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("repo");
    let mut slug: String = tail
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches(['-', '_']).to_string();
    let slug = if slug.is_empty() {
        "repo".to_string()
    } else {
        slug
    };

    let mut hasher = Sha256::new();
    hasher.update(url.trim().as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    (format!("proton-vol-{slug}-{}", &digest[..8]), slug)
}

/// Ensure each named volume exists via `<runtime> volume create <name>`.
pub fn ensure_volumes<F>(
    rt: &Runtime,
    volumes: &[String],
    cwd: &Path,
    mut on_log: F,
) -> Result<(), String>
where
    F: FnMut(String),
{
    for vol in volumes {
        on_log(format!("Ensuring volume {vol}…"));
        let out = rt
            .command()
            .args(["volume", "create", vol])
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("failed to run {} volume create {vol}: {e}", rt.program.display()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("volume create {vol} failed: {err}"));
        }
    }
    Ok(())
}

/// Clone `url` (optionally `branch`) into a named container volume and write a
/// starter `.devcontainer/devcontainer.json` inside `workspace_dir` if none exists.
///
/// Returns `(volume_name, container_workspace_folder)`.
pub fn clone_into_volume<F>(
    rt: &Runtime,
    url: &str,
    branch: Option<&str>,
    workspace_dir: &Path,
    mut on_log: F,
) -> Result<(String, String), String>
where
    F: FnMut(String),
{
    let clean_url = url.trim();
    if clean_url.is_empty() {
        return Err("repository URL must not be empty".to_string());
    }

    let (vol, slug) = volume_name_for_url(clean_url);
    ensure_volumes(rt, std::slice::from_ref(&vol), workspace_dir, &mut on_log)?;

    let container_folder = format!("/workspaces/{slug}");
    on_log(format!("Cloning {clean_url} into volume {vol} ({container_folder})…"));

    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--mount".to_string(),
        format!("type=volume,source={vol},target={container_folder}"),
        "alpine/git".to_string(),
        "clone".to_string(),
    ];
    if let Some(br) = branch.map(str::trim).filter(|b| !b.is_empty()) {
        args.push("--branch".to_string());
        args.push(br.to_string());
    }
    args.push(clean_url.to_string());
    args.push(container_folder.clone());

    let out = rt
        .command()
        .args(&args)
        .current_dir(workspace_dir)
        .output()
        .map_err(|e| format!("failed to run clone container: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("clone into volume failed: {msg}"));
    }

    let dc_dir = workspace_dir.join(".devcontainer");
    let cfg_path = dc_dir.join("devcontainer.json");
    if !cfg_path.is_file() {
        fs::create_dir_all(&dc_dir)
            .map_err(|e| format!("failed creating {}: {e}", dc_dir.display()))?;
        let doc = serde_json::json!({
            "name": slug,
            "image": "mcr.microsoft.com/devcontainers/base:bookworm",
            "workspaceMount": format!("type=volume,source={vol},target={container_folder}"),
            "workspaceFolder": container_folder,
        });
        let text = format!(
            "{}\n",
            serde_json::to_string_pretty(&doc).unwrap_or_default()
        );
        fs::write(&cfg_path, text)
            .map_err(|e| format!("failed writing {}: {e}", cfg_path.display()))?;
    }

    Ok((vol, container_folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_deterministic_volume_names_from_git_urls() {
        let (v1, slug1) = volume_name_for_url("https://github.com/org/My.App.git");
        let (v2, slug2) = volume_name_for_url("git@github.com:org/My.App.git");
        assert_eq!(slug1, "my-app");
        assert_eq!(slug2, "my-app");
        assert!(v1.starts_with("proton-vol-my-app-"));
        assert!(v2.starts_with("proton-vol-my-app-"));
        assert_ne!(v1, v2);
    }

    #[test]
    fn extracts_volume_names_from_workspace_mount_and_extra_mounts() {
        let mounts = vec![
            MountSpec {
                source: Some("/host/path".into()),
                target: "/target".into(),
                kind: "bind".into(),
                consistency: None,
                read_only: false,
            },
            MountSpec {
                source: Some("cargo-cache".into()),
                target: "/usr/local/cargo".into(),
                kind: "volume".into(),
                consistency: None,
                read_only: false,
            },
            MountSpec {
                source: Some("ws-vol".into()),
                target: "/dup".into(),
                kind: "volume".into(),
                consistency: None,
                read_only: false,
            },
        ];
        let vols = extract_volume_names(
            Some("type=volume,source=ws-vol,target=/workspaces/demo"),
            &mounts,
        );
        assert_eq!(vols, vec!["ws-vol", "cargo-cache"]);
    }
}
