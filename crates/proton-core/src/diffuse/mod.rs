//! Diffuse in the core: the flows open in this project, where the gateway
//! is, and the key to it.
//!
//! The flow format, its edits, the client and what forge served last are
//! `proton-diffuse`'s. What lives here is the editor's half — which flows
//! are open, as tabs, and which is shown; their edits held until ⌘S and
//! named in the quit prompt; the project's flows, for the Flows list — and
//! the person's. Their gateway's
//! addresses are their own settings — `diffuse.gateway` at home and
//! `diffuse.gatewayAway` over Tailscale, in `~/.config/proton/settings.json`
//! — since atlas is where they live, not where a project does. The key is
//! `NEXUS_KEY`: Proton's own environment, then the login shell's, as Fish's
//! is found. It is looked for once and kept in memory only, and looked for
//! again when the person asks — after they have just exported it.

use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use proton_diffuse::{Catalogue, Client, Flow, FlowDoc, Health, Key, Known, Problem};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::settings::Settings;
use crate::workspace::notice;

/// The variable the gateway key is kept in.
pub const KEY_NAME: &str = "NEXUS_KEY";

/// The gateway at home: Nexus's coordinator on atlas, the Mac mini.
pub const HOME: &str = "http://atlas:7420/img/v1";

/// The gateway's addresses, in the order they are tried: home, then away
/// when one is set. Only `http` and `https` addresses are taken.
pub fn gateways(person: &Settings) -> Vec<String> {
    let home = person.string("diffuse.gateway", HOME);
    let away = person.string("diffuse.gatewayAway", "");
    [home, away]
        .into_iter()
        .map(|address| address.trim().trim_end_matches('/').to_string())
        .filter(|address| address.starts_with("http://") || address.starts_with("https://"))
        .collect()
}

/// Where a project keeps what forge served last.
pub fn kept_in(root: &Path) -> PathBuf {
    root.join(".proton").join("diffuse")
}

/// The key to the gateway, and the client it makes.
#[derive(Debug, Clone, Default)]
pub struct Gateway {
    /// The key once looked for: whether it was found.
    found: Option<Option<Key>>,
}

impl Gateway {
    /// The key: Proton's environment, then the login shell's.
    pub fn key(&mut self) -> Option<Key> {
        self.found
            .get_or_insert_with(|| {
                let own = std::env::var(KEY_NAME).ok().and_then(|v| Key::new(&v));
                own.or_else(|| {
                    let vars = proton_claude::process::login_vars(&[KEY_NAME]);
                    vars.get(KEY_NAME).and_then(|v| Key::new(v))
                })
            })
            .clone()
    }

    /// Look for the key again, as when it has just been exported.
    pub fn forget(&mut self) {
        self.found = None;
    }

    /// A client for the person's gateway, when there is a key.
    pub fn client(&mut self, person: &Settings) -> Option<Client> {
        let key = self.key()?;
        Some(Client::new(gateways(person), key))
    }

    /// How forge is, and its catalogue and models — just now, or as the
    /// project kept them. It asks over the network: call it off the lock.
    pub fn look(&mut self, root: &Path, person: &Settings) -> Known {
        let client = self.client(person);
        proton_diffuse::cache::look(client.as_ref(), &kept_in(root))
    }
}

/// A flow's file ends so.
pub const EXTENSION: &str = ".flow.json";

/// Whether a command is Diffuse's.
pub fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::DiffuseOpen { .. }
            | Command::DiffuseNew { .. }
            | Command::DiffuseClose { .. }
            | Command::DiffuseEdit { .. }
            | Command::DiffuseUndo
            | Command::DiffuseRedo
            | Command::DiffuseSave
            | Command::DiffuseLook { .. }
    )
}

/// What the window draws Diffuse from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DiffuseView {
    /// The project's flows, as it names them.
    pub flows: Vec<String>,
    /// The flows open, as tabs, in the order they were opened.
    pub open: Vec<DiffuseTab>,
    /// The flow shown.
    pub shown: Option<DiffuseShown>,
    /// The nodes the edit just answered added, so the window can select them.
    pub added: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct DiffuseTab {
    pub path: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DiffuseShown {
    pub path: String,
    pub flow: Flow,
    /// What stops it running, against the catalogue known.
    pub problems: Vec<Problem>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Diffuse's state in one project.
#[derive(Debug, Default)]
pub struct Diffuse {
    gateway: Gateway,
    /// The person's own settings file: where the gateway is.
    person: Option<PathBuf>,
    /// The flows open, by the project's name for each.
    open: Vec<(String, FlowDoc)>,
    shown: Option<String>,
    /// What forge is and has, as last looked or kept.
    known: Option<Known>,
    /// A look under way, on a worker.
    looking: Option<Receiver<(Gateway, Known)>>,
}

impl Diffuse {
    /// Where the person's own settings are, for the gateway's addresses.
    pub fn set_person(&mut self, file: Option<PathBuf>) {
        self.person = file;
    }

    /// The mode came on screen: what the project kept of forge, if nothing
    /// is known yet, and everything to draw.
    pub fn enter(&mut self, root: &Path) -> Vec<Event> {
        if self.known.is_none() {
            self.known = Some(proton_diffuse::cache::kept(Health::looking(), &kept_in(root)));
        }
        self.events(root)
    }

    /// Everything the window needs to draw the mode.
    pub fn events(&self, root: &Path) -> Vec<Event> {
        let mut events = vec![self.view_event(root, Vec::new())];
        if let Some(known) = &self.known {
            events.push(self.known_event(known.clone()));
        }
        events
    }

    pub fn apply(&mut self, root: &Path, command: Command) -> Vec<Event> {
        match command {
            Command::DiffuseOpen { path } => {
                if self.open.iter().all(|(rel, _)| rel != &path) {
                    let doc = match inside(root, &path).and_then(|file| FlowDoc::read(&file)) {
                        Ok(doc) => doc,
                        Err(why) => return vec![notice(why, NoticeLevel::Error)],
                    };
                    self.open.push((path.clone(), doc));
                }
                self.shown = Some(path);
                vec![self.view_event(root, Vec::new())]
            }
            Command::DiffuseNew { name, template } => {
                let name = name.trim().trim_end_matches(EXTENSION).trim().to_string();
                if name.is_empty() || name.contains(['/', '\\', ':', '\0']) || name.starts_with('.') {
                    return vec![notice(
                        "a flow's name is a plain file name, such as lighthouse".into(),
                        NoticeLevel::Warn,
                    )];
                }
                let rel = format!("{name}{EXTENSION}");
                let file = match inside(root, &rel) {
                    Ok(file) => file,
                    Err(why) => return vec![notice(why, NoticeLevel::Error)],
                };
                if file.exists() {
                    return vec![notice(format!("{rel} is already here"), NoticeLevel::Warn)];
                }
                let mut flow = Flow::default();
                if let Some(template) = template {
                    let Some(ops) = proton_diffuse::templates::ops(&template) else {
                        return vec![notice(format!("there is no starter called {template}"), NoticeLevel::Warn)];
                    };
                    if let Err(why) = proton_diffuse::op::apply(&mut flow, &ops, &self.catalogue()) {
                        return vec![notice(format!("the starter needs forge's nodes: {why}"), NoticeLevel::Warn)];
                    }
                }
                let mut doc = FlowDoc::fresh(&file, flow);
                if let Err(why) = doc.save() {
                    return vec![notice(why, NoticeLevel::Error)];
                }
                self.open.push((rel.clone(), doc));
                self.shown = Some(rel);
                vec![self.view_event(root, Vec::new())]
            }
            Command::DiffuseClose { path, discard } => {
                let Some(at) = self.open.iter().position(|(rel, _)| rel == &path) else { return Vec::new() };
                if self.open[at].1.is_dirty() && !discard {
                    return vec![notice(
                        format!("save or discard {path}'s changes before closing it"),
                        NoticeLevel::Warn,
                    )];
                }
                self.open.remove(at);
                if self.shown.as_deref() == Some(path.as_str()) {
                    self.shown = self.open.last().map(|(rel, _)| rel.clone());
                }
                vec![self.view_event(root, Vec::new())]
            }
            Command::DiffuseEdit { path, ops } => {
                let catalogue = self.catalogue();
                let Some((_, doc)) = self.open.iter_mut().find(|(rel, _)| rel == &path) else {
                    return vec![notice(format!("{path} is not open"), NoticeLevel::Warn)];
                };
                match doc.apply(&ops, &catalogue) {
                    Ok(applied) => vec![self.view_event(root, applied.added)],
                    // The window shows what the core says: the flow as it
                    // was, and why the edit was refused.
                    Err(why) => vec![notice(why, NoticeLevel::Warn), self.view_event(root, Vec::new())],
                }
            }
            Command::DiffuseUndo | Command::DiffuseRedo => {
                let undo = matches!(command, Command::DiffuseUndo);
                if let Some(doc) = self.shown_mut() {
                    let _ = if undo { doc.undo() } else { doc.redo() };
                }
                vec![self.view_event(root, Vec::new())]
            }
            Command::DiffuseSave => {
                let Some(doc) = self.shown_mut() else { return Vec::new() };
                match doc.save() {
                    Ok(()) => vec![self.view_event(root, Vec::new())],
                    Err(why) => vec![notice(why, NoticeLevel::Error)],
                }
            }
            Command::DiffuseLook { again } => {
                if again {
                    self.gateway.forget();
                }
                if self.looking.is_some() {
                    return Vec::new();
                }
                let (tell, told) = channel();
                let mut gateway = self.gateway.clone();
                let person = self.person.clone().map(Settings::at).unwrap_or_default();
                let root = root.to_path_buf();
                std::thread::spawn(move || {
                    let known = gateway.look(&root, &person);
                    let _ = tell.send((gateway, known));
                });
                self.looking = Some(told);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// A look that has finished: what forge is and has, and the flow's
    /// problems against it.
    pub fn poll(&mut self, root: &Path) -> Vec<Event> {
        let Some(told) = &self.looking else { return Vec::new() };
        match told.try_recv() {
            Ok((gateway, known)) => {
                self.looking = None;
                self.gateway = gateway;
                self.known = Some(known.clone());
                vec![self.known_event(known), self.view_event(root, Vec::new())]
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Vec::new(),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.looking = None;
                Vec::new()
            }
        }
    }

    /// The flows with changes not saved, as the project names them.
    pub fn unsaved(&self) -> Vec<String> {
        self.open.iter().filter(|(_, doc)| doc.is_dirty()).map(|(rel, _)| rel.clone()).collect()
    }

    /// Save every flow with changes, for quitting: what could not be.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let mut failed = Vec::new();
        for (rel, doc) in self.open.iter_mut().filter(|(_, doc)| doc.is_dirty()) {
            if let Err(why) = doc.save() {
                failed.push(format!("{rel}: {why}"));
            }
        }
        failed
    }

    /// The catalogue edits are checked against: forge's, or none known.
    fn catalogue(&self) -> Catalogue {
        self.known.as_ref().and_then(|k| k.catalogue.clone()).unwrap_or(Catalogue {
            version: 1,
            types: Vec::new(),
            nodes: Vec::new(),
            lists: Default::default(),
        })
    }

    fn known_event(&self, known: Known) -> Event {
        let person = self.person.clone().map(Settings::at).unwrap_or_default();
        Event::DiffuseKnown { known, gateways: gateways(&person) }
    }

    fn shown_mut(&mut self) -> Option<&mut FlowDoc> {
        let shown = self.shown.clone()?;
        self.open.iter_mut().find(|(rel, _)| rel == &shown).map(|(_, doc)| doc)
    }

    fn view_event(&self, root: &Path, added: Vec<String>) -> Event {
        let known = self.known.as_ref();
        let shown = self.shown.as_ref().and_then(|path| {
            let (_, doc) = self.open.iter().find(|(rel, _)| rel == path)?;
            let problems = match known.and_then(|k| k.catalogue.as_ref()) {
                Some(catalogue) => proton_diffuse::check(doc.get(), catalogue, known.and_then(|k| k.models.as_ref())),
                None => vec![Problem {
                    node: None,
                    input: None,
                    message: "forge's nodes are not known yet: it has not been reached".into(),
                }],
            };
            Some(DiffuseShown {
                path: path.clone(),
                flow: doc.get().clone(),
                problems,
                dirty: doc.is_dirty(),
                can_undo: doc.can_undo(),
                can_redo: doc.can_redo(),
            })
        });
        let open =
            self.open.iter().map(|(path, doc)| DiffuseTab { path: path.clone(), dirty: doc.is_dirty() }).collect();
        Event::DiffuseView { view: DiffuseView { flows: find(root), open, shown, added } }
    }
}

/// `rel` inside `root`: plain parts, a `.flow.json`, and still inside once
/// links are followed — it came from the window, or from a file.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if !rel.ends_with(EXTENSION) || rel.contains('\\') || rel.contains('\0') {
        return Err(format!("{rel:?} is not a flow in the project"));
    }
    if !named.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err(format!("{rel} reaches outside the project"));
    }
    let base = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let path = base.join(named);
    let mut held = path.as_path();
    let real = loop {
        match std::fs::canonicalize(held) {
            Ok(real) => break real,
            Err(_) => match held.parent() {
                Some(up) => held = up,
                None => return Err(format!("{rel} is nowhere")),
            },
        }
    };
    if !real.starts_with(&base) {
        return Err(format!("{rel} leads outside the project"));
    }
    Ok(path)
}

/// The project's flows: `.flow.json` files up to three folders down, as
/// Canvas looks for canvases.
fn find(root: &Path) -> Vec<String> {
    const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor", "outputs"];
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut looked = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            looked += 1;
            if looked > 5000 || out.len() >= 500 {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            if path.is_dir() {
                if depth < 3 {
                    stack.push((path, depth + 1));
                }
            } else if name.ends_with(EXTENSION) {
                out.push(path.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default());
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn home_is_atlas_unless_the_person_says_otherwise() {
        assert_eq!(gateways(&Settings::saying(json!({}))), [HOME]);
        let both = Settings::saying(json!({"diffuse": {
            "gateway": "http://192.168.68.10:7420/img/v1/",
            "gatewayAway": " https://atlas.example.ts.net/img/v1 "
        }}));
        assert_eq!(gateways(&both), ["http://192.168.68.10:7420/img/v1", "https://atlas.example.ts.net/img/v1"]);
        // Nothing that is not a web address is tried.
        let odd = Settings::saying(json!({"diffuse": {"gateway": "atlas:7420", "gatewayAway": "file:///etc"}}));
        assert!(gateways(&odd).is_empty());
    }

    #[test]
    fn a_project_keeps_forges_answers_in_its_own_proton_folder() {
        assert_eq!(kept_in(Path::new("/p")), Path::new("/p/.proton/diffuse"));
    }
}
