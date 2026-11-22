//! Discovery, JSONC parsing, variable expansion, and validation for
//! `devcontainer.json`.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{de, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::jsonc;

/// Discover candidate `devcontainer.json` files inside `root`, in standard
/// precedence order:
/// 1. `.devcontainer/devcontainer.json`
/// 2. `.devcontainer.json`
/// 3. `.devcontainer/<subdir>/devcontainer.json` (sorted)
/// 4. `.proton/devcontainer.json`
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();

    let push_if_file = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if p.is_file() && !out.contains(&p) {
            out.push(p);
        }
    };

    push_if_file(root.join(".devcontainer").join("devcontainer.json"), &mut found);
    push_if_file(root.join(".devcontainer.json"), &mut found);

    let dc_dir = root.join(".devcontainer");
    if let Ok(entries) = std::fs::read_dir(&dc_dir) {
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            push_if_file(sub.join("devcontainer.json"), &mut found);
        }
    }

    push_if_file(root.join(".proton").join("devcontainer.json"), &mut found);
    found
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    One(String),
    Many(Vec<String>),
}

impl StringOrVec {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            StringOrVec::One(s) => vec![s],
            StringOrVec::Many(v) => v,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BuildConfig {
    #[serde(default, alias = "dockerFile")]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec_opt")]
    pub cache_from: Vec<String>,
}

fn deserialize_string_or_vec_opt<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<StringOrVec>::deserialize(deserializer)?;
    Ok(opt.map(StringOrVec::into_vec).unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MountSpec {
    pub source: Option<String>,
    pub target: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub consistency: Option<String>,
    pub read_only: bool,
}

impl MountSpec {
    /// Format as a `--mount` flag argument for `docker`/`podman`/`nerdctl`.
    pub fn to_docker_arg(&self) -> String {
        let mut parts = vec![format!("type={}", self.kind)];
        if let Some(src) = &self.source {
            parts.push(format!("source={src}"));
        }
        parts.push(format!("target={}", self.target));
        if let Some(cons) = &self.consistency {
            parts.push(format!("consistency={cons}"));
        }
        if self.read_only {
            parts.push("readonly".to_string());
        }
        parts.join(",")
    }

    fn expand(mut self, ctx: &ExpandCtx) -> Self {
        self.source = self.source.map(|s| ctx.expand(&s));
        self.target = ctx.expand(&self.target);
        self.kind = ctx.expand(&self.kind);
        self.consistency = self.consistency.map(|s| ctx.expand(&s));
        self
    }
}

impl<'de> Deserialize<'de> for MountSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Obj {
            source: Option<String>,
            target: String,
            #[serde(rename = "type", default = "default_bind")]
            kind: String,
            consistency: Option<String>,
            #[serde(default)]
            read_only: bool,
        }
        fn default_bind() -> String {
            "bind".to_string()
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Str(String),
            Obj(Obj),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Obj(o) => Ok(MountSpec {
                source: o.source,
                target: o.target,
                kind: o.kind,
                consistency: o.consistency,
                read_only: o.read_only,
            }),
            Raw::Str(s) => parse_mount_string(&s).map_err(de::Error::custom),
        }
    }
}

fn parse_mount_string(raw: &str) -> Result<MountSpec, String> {
    let mut source = None;
    let mut target = None;
    let mut kind = "bind".to_string();
    let mut consistency = None;
    let mut read_only = false;

    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if matches!(part, "readonly" | "ro") {
            read_only = true;
            continue;
        }
        let Some((k, v)) = part.split_once('=') else {
            return Err(format!("invalid mount segment: {part}"));
        };
        match k.trim() {
            "type" => kind = v.trim().to_string(),
            "source" | "src" => source = Some(v.trim().to_string()),
            "target" | "dst" | "destination" => target = Some(v.trim().to_string()),
            "consistency" => consistency = Some(v.trim().to_string()),
            "readonly" | "ro" => read_only = v.trim() != "false" && v.trim() != "0",
            _ => {}
        }
    }

    let target = target.ok_or_else(|| format!("mount missing target: {raw}"))?;
    Ok(MountSpec {
        source,
        target,
        kind,
        consistency,
        read_only,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PortSpec {
    pub host_port: u16,
    pub container_port: u16,
}

impl<'de> Deserialize<'de> for PortSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Num(u16),
            Str(String),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Num(p) => Ok(PortSpec {
                host_port: p,
                container_port: p,
            }),
            Raw::Str(s) => {
                let s = s.trim();
                if let Some((h, c)) = s.split_once(':') {
                    let host_port: u16 = h
                        .trim()
                        .parse()
                        .map_err(|_| de::Error::custom(format!("invalid host port: {s}")))?;
                    let container_port: u16 = c
                        .trim()
                        .parse()
                        .map_err(|_| de::Error::custom(format!("invalid container port: {s}")))?;
                    Ok(PortSpec {
                        host_port,
                        container_port,
                    })
                } else {
                    let p: u16 = s
                        .parse()
                        .map_err(|_| de::Error::custom(format!("invalid port: {s}")))?;
                    Ok(PortSpec {
                        host_port: p,
                        container_port: p,
                    })
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PortAttributes {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub on_auto_forward: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SingleCommand {
    Shell(String),
    Args(Vec<String>),
}

impl SingleCommand {
    fn expand(self, ctx: &ExpandCtx) -> Self {
        match self {
            SingleCommand::Shell(s) => SingleCommand::Shell(ctx.expand(&s)),
            SingleCommand::Args(v) => SingleCommand::Args(v.into_iter().map(|s| ctx.expand(&s)).collect()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LifecycleCommand {
    Shell(String),
    Args(Vec<String>),
    Parallel(BTreeMap<String, SingleCommand>),
}

impl LifecycleCommand {
    fn expand(self, ctx: &ExpandCtx) -> Self {
        match self {
            LifecycleCommand::Shell(s) => LifecycleCommand::Shell(ctx.expand(&s)),
            LifecycleCommand::Args(v) => {
                LifecycleCommand::Args(v.into_iter().map(|s| ctx.expand(&s)).collect())
            }
            LifecycleCommand::Parallel(map) => LifecycleCommand::Parallel(
                map.into_iter()
                    .map(|(k, v)| (k, v.expand(ctx)))
                    .collect(),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShutdownAction {
    StopContainer,
    StopCompose,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LspOverride {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProtonCustomizations {
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub python: Option<String>,
    #[serde(default)]
    pub lsp: BTreeMap<String, LspOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Customizations {
    #[serde(default)]
    pub proton: Option<ProtonCustomizations>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DevContainerConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub build: Option<BuildConfig>,
    #[serde(default, alias = "dockerfile", rename = "dockerFile")]
    pub docker_file: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default, rename = "dockerComposeFile")]
    pub docker_compose_file: Option<StringOrVec>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub run_services: Vec<String>,
    #[serde(default)]
    pub workspace_folder: Option<String>,
    #[serde(default)]
    pub workspace_mount: Option<String>,
    #[serde(default)]
    pub mounts: Vec<MountSpec>,
    #[serde(default)]
    pub run_args: Vec<String>,
    #[serde(default)]
    pub container_env: BTreeMap<String, String>,
    #[serde(default)]
    pub remote_env: BTreeMap<String, String>,
    #[serde(default)]
    pub remote_user: Option<String>,
    #[serde(default)]
    pub container_user: Option<String>,
    #[serde(default)]
    pub forward_ports: Vec<PortSpec>,
    #[serde(default)]
    pub ports_attributes: BTreeMap<String, PortAttributes>,
    #[serde(default)]
    pub other_ports_attributes: Option<PortAttributes>,
    #[serde(default)]
    pub features: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub override_feature_install_order: Vec<String>,
    #[serde(default)]
    pub cap_add: Vec<String>,
    #[serde(default)]
    pub security_opt: Vec<String>,
    #[serde(default)]
    pub privileged: bool,
    #[serde(default)]
    pub init: bool,
    #[serde(default)]
    pub initialize_command: Option<LifecycleCommand>,
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
    #[serde(default)]
    pub shutdown_action: Option<ShutdownAction>,
    #[serde(default)]
    pub override_command: Option<bool>,
    #[serde(default)]
    pub customizations: Option<Customizations>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerSource {
    Image(String),
    Dockerfile {
        dockerfile: PathBuf,
        context: PathBuf,
        args: BTreeMap<String, String>,
        target: Option<String>,
        cache_from: Vec<String>,
    },
    Compose {
        files: Vec<PathBuf>,
        service: String,
        run_services: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub host_root: PathBuf,
    pub config_path: PathBuf,
    pub relative_config_path: String,
    pub name: String,
    pub source: ContainerSource,
    pub workspace_folder: String,
    pub workspace_mount: Option<String>,
    pub mounts: Vec<MountSpec>,
    pub run_args: Vec<String>,
    pub container_env: BTreeMap<String, String>,
    pub remote_env: BTreeMap<String, String>,
    pub effective_user: Option<String>,
    pub forward_ports: Vec<PortSpec>,
    pub ports_attributes: BTreeMap<String, PortAttributes>,
    pub other_ports_attributes: Option<PortAttributes>,
    pub features: BTreeMap<String, serde_json::Value>,
    pub override_feature_install_order: Vec<String>,
    pub cap_add: Vec<String>,
    pub security_opt: Vec<String>,
    pub privileged: bool,
    pub init: bool,
    pub initialize_command: Option<LifecycleCommand>,
    pub on_create_command: Option<LifecycleCommand>,
    pub update_content_command: Option<LifecycleCommand>,
    pub post_create_command: Option<LifecycleCommand>,
    pub post_start_command: Option<LifecycleCommand>,
    pub post_attach_command: Option<LifecycleCommand>,
    pub shutdown_action: ShutdownAction,
    pub override_command: bool,
    pub shell: Option<String>,
    pub python: Option<String>,
    pub lsp_overrides: BTreeMap<String, LspOverride>,
    pub container_name: String,
    pub image_tag: String,
    pub config_hash: String,
    pub devcontainer_id: String,
}

impl ResolvedConfig {
    /// Load, validate, expand variables, and compute deterministic identifiers
    /// for a `devcontainer.json` file.
    pub fn load(host_root: &Path, config_path: &Path) -> Result<Self, String> {
        let raw_text = std::fs::read_to_string(config_path)
            .map_err(|e| format!("could not read {}: {e}", config_path.display()))?;
        let cleaned = jsonc::strip(&raw_text);
        let cfg: DevContainerConfig = serde_json::from_str(&cleaned)
            .map_err(|e| format!("invalid {}: {e}", config_path.display()))?;

        Self::from_parsed(host_root, config_path, &raw_text, cfg)
    }

    pub fn from_parsed(
        host_root: &Path,
        config_path: &Path,
        raw_text: &str,
        cfg: DevContainerConfig,
    ) -> Result<Self, String> {
        let config_dir = config_path.parent().unwrap_or(host_root);
        let rel_config_path = config_path
            .strip_prefix(host_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| config_path.to_string_lossy().replace('\\', "/"));

        let local_basename = host_root
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("workspace")
            .to_string();

        let id_digest = sha256_hex(&format!(
            "{}:{}",
            host_root.to_string_lossy(),
            rel_config_path
        ));
        let devcontainer_id = id_digest[..16].to_string();
        let slug = sanitize_slug(&local_basename);
        let hash8 = &id_digest[..8];
        let container_name = format!("proton-dev-{slug}-{hash8}");
        let image_tag = format!("proton-dev-img-{slug}-{hash8}");

        // First expand workspaceFolder using local context so
        // ${containerWorkspaceFolder} is available to every other field.
        let mut ctx = ExpandCtx {
            local_workspace_folder: host_root.to_string_lossy().into_owned(),
            local_workspace_folder_basename: local_basename.clone(),
            container_workspace_folder: String::new(),
            container_workspace_folder_basename: String::new(),
            devcontainer_id: devcontainer_id.clone(),
            container_env: BTreeMap::new(),
        };

        let workspace_folder = match cfg.workspace_folder {
            Some(ref wf) => ctx.expand(wf),
            None => format!("/workspaces/{local_basename}"),
        };
        let container_basename = workspace_folder
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(&local_basename)
            .to_string();
        ctx.container_workspace_folder = workspace_folder.clone();
        ctx.container_workspace_folder_basename = container_basename;

        // Expand containerEnv values sequentially so later entries can refer to
        // earlier ones via ${containerEnv:KEY}.
        let mut container_env = BTreeMap::new();
        for (k, v) in cfg.container_env {
            let expanded = ctx.expand(&v);
            ctx.container_env.insert(k.clone(), expanded.clone());
            container_env.insert(k, expanded);
        }

        let mut remote_env = BTreeMap::new();
        for (k, v) in cfg.remote_env {
            remote_env.insert(k, ctx.expand(&v));
        }

        let mut config_hasher = Sha256::new();
        config_hasher.update(raw_text.as_bytes());
        config_hasher.update(b"\0");
        config_hasher.update(workspace_folder.as_bytes());

        let source = if let Some(compose) = cfg.docker_compose_file {
            let service = cfg
                .service
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "dockerComposeFile requires \"service\"".to_string())?
                .to_string();
            let files: Vec<PathBuf> = compose
                .into_vec()
                .into_iter()
                .map(|f| resolve_rel(config_dir, &ctx.expand(&f)))
                .collect();
            for f in &files {
                if let Ok(bytes) = std::fs::read(f) {
                    config_hasher.update(b"\0compose\0");
                    config_hasher.update(&bytes);
                }
            }
            ContainerSource::Compose {
                files,
                service,
                run_services: cfg.run_services,
            }
        } else if cfg.build.is_some() || cfg.docker_file.is_some() {
            let build = cfg.build.unwrap_or_default();
            let raw_df = build
                .dockerfile
                .or(cfg.docker_file)
                .ok_or_else(|| "\"build\" requires \"dockerfile\"".to_string())?;
            let dockerfile = resolve_rel(config_dir, &ctx.expand(&raw_df));
            let raw_ctx = build.context.or(cfg.context).unwrap_or_else(|| ".".into());
            let context = resolve_rel(config_dir, &ctx.expand(&raw_ctx));

            if let Ok(bytes) = std::fs::read(&dockerfile) {
                config_hasher.update(b"\0dockerfile\0");
                config_hasher.update(&bytes);
            }

            let args = build
                .args
                .into_iter()
                .map(|(k, v)| (k, ctx.expand(&v)))
                .collect();
            let cache_from = build
                .cache_from
                .into_iter()
                .map(|s| ctx.expand(&s))
                .collect();
            ContainerSource::Dockerfile {
                dockerfile,
                context,
                args,
                target: build.target.map(|t| ctx.expand(&t)),
                cache_from,
            }
        } else if let Some(image) = cfg.image {
            let img = ctx.expand(&image);
            if img.trim().is_empty() {
                return Err("devcontainer.json \"image\" must not be empty".to_string());
            }
            ContainerSource::Image(img)
        } else {
            return Err(
                "devcontainer.json must specify \"image\", \"build\"/\"dockerFile\", or \"dockerComposeFile\" + \"service\""
                    .to_string(),
            );
        };

        let features: BTreeMap<String, serde_json::Value> = cfg
            .features
            .into_iter()
            .map(|(k, v)| (ctx.expand(&k), expand_json_value(&ctx, v)))
            .collect();
        let override_feature_install_order: Vec<String> = cfg
            .override_feature_install_order
            .into_iter()
            .map(|s| ctx.expand(&s))
            .collect();
        let cap_add: Vec<String> = cfg.cap_add.into_iter().map(|s| ctx.expand(&s)).collect();
        let security_opt: Vec<String> = cfg
            .security_opt
            .into_iter()
            .map(|s| ctx.expand(&s))
            .collect();

        config_hasher.update(b"\0features\0");
        if let Ok(bytes) = serde_json::to_vec(&(
            &features,
            &override_feature_install_order,
            &cap_add,
            &security_opt,
            cfg.privileged,
            cfg.init,
        )) {
            config_hasher.update(&bytes);
        }
        for feat_key in features.keys() {
            if feat_key.starts_with("./") || feat_key.starts_with("../") {
                let fdir = resolve_rel(config_dir, feat_key);
                for fname in ["devcontainer-feature.json", "install.sh"] {
                    if let Ok(bytes) = std::fs::read(fdir.join(fname)) {
                        config_hasher.update(b"\0local-feature\0");
                        config_hasher.update(feat_key.as_bytes());
                        config_hasher.update(&bytes);
                    }
                }
            }
        }

        let config_hash = hex_encode(&config_hasher.finalize())[..16].to_string();

        let default_shutdown = match &source {
            ContainerSource::Compose { .. } => ShutdownAction::StopCompose,
            ContainerSource::Image(_) | ContainerSource::Dockerfile { .. } => {
                ShutdownAction::StopContainer
            }
        };
        let shutdown_action = cfg.shutdown_action.unwrap_or(default_shutdown);
        let override_command = cfg
            .override_command
            .unwrap_or(!matches!(source, ContainerSource::Compose { .. }));

        let name = cfg
            .name
            .map(|n| ctx.expand(&n))
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| local_basename.clone());

        let proton = cfg.customizations.and_then(|c| c.proton).unwrap_or_default();
        let lsp_overrides = proton
            .lsp
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    LspOverride {
                        program: ctx.expand(&v.program),
                        args: v.args.into_iter().map(|a| ctx.expand(&a)).collect(),
                    },
                )
            })
            .collect();

        Ok(ResolvedConfig {
            host_root: host_root.to_path_buf(),
            config_path: config_path.to_path_buf(),
            relative_config_path: rel_config_path,
            name,
            source,
            workspace_folder,
            workspace_mount: cfg.workspace_mount.map(|m| ctx.expand(&m)),
            mounts: cfg.mounts.into_iter().map(|m| m.expand(&ctx)).collect(),
            run_args: cfg.run_args.into_iter().map(|a| ctx.expand(&a)).collect(),
            container_env,
            remote_env,
            effective_user: cfg.remote_user.or(cfg.container_user),
            forward_ports: cfg.forward_ports,
            ports_attributes: cfg.ports_attributes,
            other_ports_attributes: cfg.other_ports_attributes,
            features,
            override_feature_install_order,
            cap_add,
            security_opt,
            privileged: cfg.privileged,
            init: cfg.init,
            initialize_command: cfg.initialize_command.map(|c| c.expand(&ctx)),
            on_create_command: cfg.on_create_command.map(|c| c.expand(&ctx)),
            update_content_command: cfg.update_content_command.map(|c| c.expand(&ctx)),
            post_create_command: cfg.post_create_command.map(|c| c.expand(&ctx)),
            post_start_command: cfg.post_start_command.map(|c| c.expand(&ctx)),
            post_attach_command: cfg.post_attach_command.map(|c| c.expand(&ctx)),
            shutdown_action,
            override_command,
            shell: proton.shell.map(|s| ctx.expand(&s)),
            python: proton.python.map(|p| ctx.expand(&p)),
            lsp_overrides,
            container_name,
            image_tag,
            config_hash,
            devcontainer_id,
        })
    }
}

fn expand_json_value(ctx: &ExpandCtx, value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(ctx.expand(&s)),
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(|v| expand_json_value(ctx, v)).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, expand_json_value(ctx, v)))
                .collect(),
        ),
        other => other,
    }
}

struct ExpandCtx {
    local_workspace_folder: String,
    local_workspace_folder_basename: String,
    container_workspace_folder: String,
    container_workspace_folder_basename: String,
    devcontainer_id: String,
    container_env: BTreeMap<String, String>,
}

impl ExpandCtx {
    fn expand(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after_open = &rest[start + 2..];
            let Some(end) = after_open.find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let expr = &after_open[..end];
            if let Some(val) = self.eval_var(expr) {
                out.push_str(&val);
            } else {
                out.push_str("${");
                out.push_str(expr);
                out.push('}');
            }
            rest = &after_open[end + 1..];
        }
        out.push_str(rest);
        out
    }

    fn eval_var(&self, expr: &str) -> Option<String> {
        match expr {
            "localWorkspaceFolder" => Some(self.local_workspace_folder.clone()),
            "localWorkspaceFolderBasename" => Some(self.local_workspace_folder_basename.clone()),
            "containerWorkspaceFolder" => Some(self.container_workspace_folder.clone()),
            "containerWorkspaceFolderBasename" => {
                Some(self.container_workspace_folder_basename.clone())
            }
            "devcontainerId" => Some(self.devcontainer_id.clone()),
            _ if expr.starts_with("localEnv:") => {
                let spec = &expr["localEnv:".len()..];
                let (key, default) = match spec.split_once(':') {
                    Some((k, d)) => (k, d),
                    None => (spec, ""),
                };
                Some(std::env::var(key).unwrap_or_else(|_| default.to_string()))
            }
            _ if expr.starts_with("containerEnv:") => {
                let spec = &expr["containerEnv:".len()..];
                let (key, default) = match spec.split_once(':') {
                    Some((k, d)) => (k, d),
                    None => (spec, ""),
                };
                Some(
                    self.container_env
                        .get(key)
                        .cloned()
                        .unwrap_or_else(|| default.to_string()),
                )
            }
            _ => None,
        }
    }
}

fn resolve_rel(base: &Path, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    };
    normalize_lexical(&joined)
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(comp);
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn sanitize_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 24 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "ws".to_string()
    } else {
        trimmed
    }
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-cfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    #[test]
    fn discovers_configs_in_standard_precedence_order() {
        let root = mk_tmp("discover");
        std::fs::create_dir_all(root.join(".devcontainer/zeta")).unwrap();
        std::fs::create_dir_all(root.join(".devcontainer/alpha")).unwrap();
        std::fs::create_dir_all(root.join(".proton")).unwrap();

        std::fs::write(root.join(".proton/devcontainer.json"), "{}").unwrap();
        std::fs::write(root.join(".devcontainer/zeta/devcontainer.json"), "{}").unwrap();
        std::fs::write(root.join(".devcontainer/alpha/devcontainer.json"), "{}").unwrap();
        std::fs::write(root.join(".devcontainer.json"), "{}").unwrap();
        std::fs::write(root.join(".devcontainer/devcontainer.json"), "{}").unwrap();

        let found = discover(&root);
        let rels: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            rels,
            vec![
                ".devcontainer/devcontainer.json",
                ".devcontainer.json",
                ".devcontainer/alpha/devcontainer.json",
                ".devcontainer/zeta/devcontainer.json",
                ".proton/devcontainer.json",
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parses_build_ports_mounts_hooks_and_expands_variables() {
        let root = mk_tmp("parse");
        std::fs::create_dir_all(root.join(".devcontainer")).unwrap();
        std::fs::write(root.join(".devcontainer/Dockerfile"), "FROM alpine:3.20\n").unwrap();
        let cfg_path = root.join(".devcontainer/devcontainer.json");
        std::fs::write(
            &cfg_path,
            r#"{
              // JSONC comment
              "name": "App (${localWorkspaceFolderBasename})",
              "build": {
                "dockerfile": "Dockerfile",
                "context": "..",
                "args": { "ROOT": "${localWorkspaceFolder}" },
              },
              "workspaceFolder": "/workspaces/custom-${localWorkspaceFolderBasename}",
              "mounts": [
                "source=${localWorkspaceFolder}/.cache,target=/cache,type=bind",
                { "source": "vol-${devcontainerId}", "target": "${containerWorkspaceFolder}/data", "type": "volume" }
              ],
              "containerEnv": {
                "BASE": "/opt/app",
                "SUB": "${containerEnv:BASE}/bin:${localEnv:PROTON_NO_SUCH_VAR:fallback}"
              },
              "forwardPorts": [3000, "8080:80"],
              "portsAttributes": {
                "80": { "label": "web", "protocol": "http" }
              },
              "remoteUser": "dev",
              "postCreateCommand": ["echo", "${containerWorkspaceFolder}"],
              "customizations": {
                "proton": {
                  "shell": "/bin/zsh",
                  "lsp": { "rust": { "program": "/usr/local/bin/rust-analyzer", "args": [] } }
                }
              }
            }"#,
        )
        .unwrap();

        let resolved = ResolvedConfig::load(&root, &cfg_path).unwrap();
        let base = root.file_name().unwrap().to_str().unwrap();
        assert_eq!(resolved.workspace_folder, format!("/workspaces/custom-{base}"));
        assert_eq!(resolved.effective_user.as_deref(), Some("dev"));
        assert_eq!(resolved.shutdown_action, ShutdownAction::StopContainer);
        assert!(resolved.override_command);
        assert_eq!(
            resolved.container_env.get("SUB").map(String::as_str),
            Some("/opt/app/bin:fallback")
        );
        assert_eq!(resolved.forward_ports.len(), 2);
        assert_eq!(resolved.forward_ports[1].host_port, 8080);
        assert_eq!(resolved.forward_ports[1].container_port, 80);
        assert_eq!(resolved.mounts.len(), 2);
        assert_eq!(
            resolved.mounts[0].to_docker_arg(),
            format!(
                "type=bind,source={}/.cache,target=/cache",
                root.to_string_lossy()
            )
        );
        assert_eq!(
            resolved.mounts[1].target,
            format!("/workspaces/custom-{base}/data")
        );
        match &resolved.source {
            ContainerSource::Dockerfile {
                dockerfile,
                context,
                args,
                ..
            } => {
                assert_eq!(dockerfile, &root.join(".devcontainer/Dockerfile"));
                assert_eq!(context, &root);
                assert_eq!(args["ROOT"], root.to_string_lossy());
            }
            other => panic!("expected Dockerfile source, got {other:?}"),
        }
        assert_eq!(resolved.shell.as_deref(), Some("/bin/zsh"));
        assert_eq!(
            resolved.lsp_overrides["rust"].program,
            "/usr/local/bin/rust-analyzer"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validates_compose_requires_service_and_empty_config_rejected() {
        let root = mk_tmp("invalid");
        let cfg_path = root.join("devcontainer.json");
        std::fs::write(&cfg_path, r#"{ "dockerComposeFile": "compose.yml" }"#).unwrap();
        let err = ResolvedConfig::load(&root, &cfg_path).unwrap_err();
        assert!(err.contains("service"));

        std::fs::write(&cfg_path, r#"{ "name": "empty" }"#).unwrap();
        let err2 = ResolvedConfig::load(&root, &cfg_path).unwrap_err();
        assert!(err2.contains("image"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
