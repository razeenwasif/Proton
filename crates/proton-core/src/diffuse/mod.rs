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

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

use proton_diffuse::client::NodeState;
use proton_diffuse::png::Made;
use proton_diffuse::{Catalogue, Client, Event as RunEvent, Flow, FlowDoc, Health, Key, Known, Op, Problem, SeedMode};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::settings::Settings;
use crate::workspace::notice;

mod agent;
mod run;

pub use agent::{Said, TOOLS};
use run::{Control, Job, News};
pub use run::{DiffuseRun, NodeRun, RunOutput, RunProblem, RunProgress, RunState};

/// How many runs the Queue keeps, the newest.
const RUNS_KEPT: usize = 50;
/// How many images the gallery lists, the newest.
const GALLERY: usize = 300;

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
            | Command::DiffuseRun { .. }
            | Command::DiffuseCancel { .. }
            | Command::DiffuseGallery
            | Command::DiffuseDelete { .. }
            | Command::DiffuseDeleteImages { .. }
            | Command::DiffuseOpenImage { .. }
            | Command::DiffuseOpenGraph { .. }
            | Command::DiffuseGateways { .. }
    )
}

/// A starter graph a new flow can begin from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct DiffuseStarter {
    pub name: String,
    pub title: String,
    pub about: String,
}

/// An image in the gallery: one of the project's outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DiffuseImage {
    /// As the project names it.
    pub path: String,
    /// The folder it is in: its flow's outputs.
    pub folder: String,
    /// When it was written, in seconds since 1970.
    #[ts(type = "number")]
    pub made_at: u64,
    pub width: u32,
    pub height: u32,
    /// What made it, from the graph in its PNG.
    pub made: Made,
    /// Whether its PNG holds the graph that made it.
    pub graph: bool,
}

/// What the window draws Diffuse from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DiffuseView {
    /// Diffuse's folder, as the person would type it.
    pub folder: String,
    /// The flows in it, by their path in it.
    pub flows: Vec<String>,
    /// The flows open, as tabs, in the order they were opened.
    pub open: Vec<DiffuseTab>,
    /// The flow shown.
    pub shown: Option<DiffuseShown>,
    /// The nodes the edit just answered added, so the window can select them.
    pub added: Vec<String>,
    /// The starters a new flow can begin from.
    pub starters: Vec<DiffuseStarter>,
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
    /// The runs this session, oldest first.
    runs: Vec<DiffuseRun>,
    next_run: u32,
    /// How each run still going is stopped.
    controls: BTreeMap<u32, Control>,
    /// What the runs' workers say.
    news: Option<(Sender<News>, Receiver<News>)>,
    /// When each node of each run began, to say how long it took.
    began: BTreeMap<(u32, String), Instant>,
    /// The gallery's images, by path, with when each was written: its head
    /// is read again only when the file changes.
    gallery: BTreeMap<String, (std::time::SystemTime, DiffuseImage)>,
}

impl Diffuse {
    /// Diffuse's folder moved: nothing of the old one is kept — its flows
    /// (none with changes, the session saw to that), its gallery, and what
    /// it kept of forge.
    pub fn moved(&mut self) {
        self.open.clear();
        self.shown = None;
        self.gallery.clear();
        self.known = None;
    }

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
    pub fn events(&mut self, root: &Path) -> Vec<Event> {
        let mut events = vec![self.view_event(root, Vec::new())];
        if let Some(known) = &self.known {
            events.push(self.known_event(known.clone()));
        }
        events.push(self.runs_event());
        events.push(self.gallery_event(root));
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
                // Diffuse's folder is made when its first flow is.
                if let Err(e) = std::fs::create_dir_all(root) {
                    return vec![notice(format!("{}: {e}", root.display()), NoticeLevel::Error)];
                }
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
            Command::DiffuseRun { path } => self.run(root, &path),
            Command::DiffuseCancel { id } => {
                let Some(control) = self.controls.get(&id) else { return Vec::new() };
                control.cancel.store(true, Ordering::Relaxed);
                let forge = control.run.lock().unwrap_or_else(|e| e.into_inner()).clone();
                match forge {
                    // Queued at forge: cancelled there, which ends its stream.
                    Some(forge) => {
                        let mut gateway = self.gateway.clone();
                        let person = self.person_settings();
                        std::thread::spawn(move || {
                            if let Some(client) = gateway.client(&person) {
                                let _ = client.cancel(&forge);
                            }
                        });
                        Vec::new()
                    }
                    // Not there yet: its worker cancels it once it is.
                    None => {
                        if let Some(run) = self.runs.iter_mut().find(|r| r.id == id) {
                            run.state = RunState::Cancelled;
                        }
                        vec![self.runs_event()]
                    }
                }
            }
            Command::DiffuseGallery => vec![self.gallery_event(root)],
            Command::DiffuseOpenImage { path } => {
                let Some(file) = self.file(root, &path) else {
                    return vec![notice(format!("{path} is not an image Diffuse may open"), NoticeLevel::Warn)];
                };
                let read =
                    std::fs::File::open(&file).ok().and_then(|f| proton_diffuse::png::head(std::io::BufReader::new(f)));
                let Some(graph) = read.and_then(|head| head.flow).as_deref().and_then(proton_diffuse::png::graph_of)
                else {
                    return vec![notice(
                        format!("{path} holds no graph: it was not made by a flow on forge"),
                        NoticeLevel::Warn,
                    )];
                };
                let name = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".png").to_string();
                self.open_graph(root, &name, &graph)
            }
            Command::DiffuseOpenGraph { name, graph } => self.open_graph(root, &name, &graph),
            Command::DiffuseDelete { path } => {
                if !find(root).contains(&path) {
                    return vec![notice(format!("{path} is not a flow in the project"), NoticeLevel::Warn)];
                }
                let file = match inside(root, &path) {
                    Ok(file) => file,
                    Err(why) => return vec![notice(why, NoticeLevel::Error)],
                };
                if let Err(why) = to_trash(&[file]) {
                    return vec![notice(why, NoticeLevel::Error)];
                }
                self.open.retain(|(rel, _)| rel != &path);
                if self.shown.as_deref() == Some(path.as_str()) {
                    self.shown = self.open.last().map(|(rel, _)| rel.clone());
                }
                vec![
                    notice(format!("Moved {path} to the Trash."), NoticeLevel::Info),
                    self.view_event(root, Vec::new()),
                ]
            }
            Command::DiffuseDeleteImages { paths } => {
                // Only what the window was shown: listed by the gallery or made by a run.
                let mut files = Vec::new();
                for rel in &paths {
                    match self.listed(root, rel) {
                        Some(file) => files.push(file),
                        None => {
                            return vec![notice(format!("{rel} is not an image Diffuse may delete"), NoticeLevel::Warn)]
                        }
                    }
                }
                if files.is_empty() {
                    return Vec::new();
                }
                if let Err(why) = to_trash(&files) {
                    return vec![notice(why, NoticeLevel::Error)];
                }
                for run in &mut self.runs {
                    run.outputs.retain(|o| !paths.contains(&o.path));
                }
                let said = match paths.len() {
                    1 => format!("Moved {} to the Trash.", paths[0]),
                    n => format!("Moved {n} images to the Trash."),
                };
                vec![notice(said, NoticeLevel::Info), self.runs_event(), self.gallery_event(root)]
            }
            Command::DiffuseGateways { home, away } => {
                let Some(file) = self.person.clone() else {
                    return vec![notice(
                        "Proton has no settings of your own to keep the gateway in".into(),
                        NoticeLevel::Warn,
                    )];
                };
                let mut person = Settings::at(file);
                let tidy =
                    |a: Option<String>| a.map(|a| a.trim().trim_end_matches('/').to_string()).filter(|a| !a.is_empty());
                for (key, address) in [("diffuse.gateway", tidy(home)), ("diffuse.gatewayAway", tidy(away))] {
                    if let Some(address) = &address {
                        if !address.starts_with("http://") && !address.starts_with("https://") {
                            return vec![notice(
                                format!("{address} is not a web address: it starts http:// or https://"),
                                NoticeLevel::Warn,
                            )];
                        }
                    }
                    if let Err(why) =
                        person.set(key, address.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null))
                    {
                        return vec![notice(why, NoticeLevel::Error)];
                    }
                }
                // Looked at again, at the addresses just set.
                let mut events = self.apply(root, Command::DiffuseLook { again: false });
                if let Some(known) = &self.known {
                    events.push(self.known_event(known.clone()));
                }
                events
            }
            _ => Vec::new(),
        }
    }

    /// A look that has finished: what forge is and has, and the flow's
    /// problems against it.
    pub fn poll(&mut self, root: &Path) -> Vec<Event> {
        let mut events = self.poll_runs(root);
        let Some(told) = &self.looking else { return events };
        match told.try_recv() {
            Ok((gateway, known)) => {
                self.looking = None;
                self.gateway = gateway;
                self.known = Some(known.clone());
                events.push(self.known_event(known));
                events.push(self.view_event(root, Vec::new()));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.looking = None,
        }
        events
    }

    /// Send the flow at `path` to forge, and set each seed that moves after
    /// a run to its next value — an edit, undone as any is.
    fn run(&mut self, root: &Path, path: &str) -> Vec<Event> {
        let known = self.known.clone();
        let Some(catalogue) = known.as_ref().and_then(|k| k.catalogue.clone()) else {
            return vec![notice("forge's nodes are not known yet: it has not been reached".into(), NoticeLevel::Warn)];
        };
        if let Some(health) = known.as_ref().map(|k| &k.health) {
            use proton_diffuse::HealthState::*;
            if matches!(health.state, Offline | NoKey | BadKey | Unreachable) {
                return vec![notice(health.message.clone(), NoticeLevel::Warn)];
            }
        }
        let Some((_, doc)) = self.open.iter().find(|(rel, _)| rel == path) else {
            return vec![notice(format!("{path} is not open"), NoticeLevel::Warn)];
        };
        let flow = doc.get().clone();
        let graph = match flow.graph(&catalogue, known.as_ref().and_then(|k| k.models.as_ref())) {
            Ok(graph) => graph,
            Err(problems) => {
                let first = problems.first().map(|p| p.message.clone()).unwrap_or_default();
                return vec![notice(format!("{path} can't run yet: {first}"), NoticeLevel::Warn)];
            }
        };
        // Where its images go, and which nodes keep theirs.
        let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(EXTENSION).to_string();
        let outputs = flow.outputs.clone().unwrap_or_else(|| format!("outputs/{stem}"));
        let saves = graph
            .nodes
            .iter()
            .filter(|n| catalogue.node(&n.kind).is_some_and(|k| k.is_output()))
            .map(|n| {
                let prefix = n.values.get("prefix").and_then(serde_json::Value::as_str).unwrap_or(&stem).to_string();
                (n.id.clone(), (n.kind == "SaveImage", prefix))
            })
            .collect();
        let inputs = match Self::pictures(root, path, &graph, &catalogue) {
            Ok(inputs) => inputs,
            Err(why) => return vec![notice(format!("{path} can't run yet: {why}"), NoticeLevel::Warn)],
        };
        self.next_run += 1;
        let id = self.next_run;
        let control = Control::default();
        self.controls.insert(id, control.clone());
        self.runs.push(DiffuseRun {
            id,
            run: None,
            flow: path.to_string(),
            state: RunState::Sending,
            position: None,
            nodes: Vec::new(),
            progress: None,
            preview: None,
            outputs: Vec::new(),
            message: None,
            problems: Vec::new(),
            seconds: None,
            started: run::now_ms(),
        });
        if self.runs.len() > RUNS_KEPT {
            if let Some(at) = self.runs.iter().position(|r| r.state.ended()) {
                self.runs.remove(at);
            }
        }
        let tell = self.news.get_or_insert_with(channel).0.clone();
        run::start(
            Job {
                id,
                gateway: self.gateway.clone(),
                person: self.person_settings(),
                graph,
                root: root.to_path_buf(),
                outputs,
                saves,
                inputs,
                control,
            },
            tell,
        );
        // Seeds that move after a run, moved.
        let moves: Vec<Op> = flow
            .nodes
            .iter()
            .filter_map(|n| {
                let mode = n.seed?;
                let field = catalogue.node(&n.kind)?.fields().find(|f| f.kind.as_deref() == Some("seed"))?.name.clone();
                let now = n.values.get(&field).and_then(serde_json::Value::as_u64).unwrap_or(0);
                let next = match mode {
                    SeedMode::Fixed => return None,
                    SeedMode::Increment => (now + 1) % (proton_diffuse::catalogue::SEED_MAX + 1),
                    SeedMode::Random => random_seed(),
                };
                Some(Op::Set { id: n.id.clone(), name: field, value: serde_json::Value::from(next) })
            })
            .collect();
        if !moves.is_empty() {
            if let Some((_, doc)) = self.open.iter_mut().find(|(rel, _)| rel == path) {
                let _ = doc.apply(&moves, &catalogue);
            }
        }
        vec![self.runs_event(), self.view_event(root, Vec::new())]
    }

    /// A graph opened as a new flow, laid out, named after `name` and held
    /// until saved: `<name>.flow.json`, or `<name>-2.flow.json` and on when
    /// that is taken.
    fn open_graph(&mut self, root: &Path, name: &str, graph: &proton_diffuse::Graph) -> Vec<Event> {
        if graph.nodes.is_empty() {
            return vec![notice("that graph has no nodes".into(), NoticeLevel::Warn)];
        }
        let stem: String = name
            .trim()
            .trim_end_matches(".png")
            .trim_end_matches(EXTENSION)
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect();
        let stem = if stem.trim_matches('-').is_empty() { "flow".to_string() } else { stem };
        let taken = |rel: &str| root.join(rel).exists() || self.open.iter().any(|(r, _)| r == rel);
        let mut rel = format!("{stem}{EXTENSION}");
        let mut n = 2;
        while taken(&rel) {
            rel = format!("{stem}-{n}{EXTENSION}");
            n += 1;
        }
        if let Err(e) = std::fs::create_dir_all(root) {
            return vec![notice(format!("{}: {e}", root.display()), NoticeLevel::Error)];
        }
        let file = match inside(root, &rel) {
            Ok(file) => file,
            Err(why) => return vec![notice(why, NoticeLevel::Error)],
        };
        let flow = proton_diffuse::layout::flow_from_graph(graph, &self.catalogue());
        self.open.push((rel.clone(), FlowDoc::fresh(&file, flow)));
        self.shown = Some(rel.clone());
        vec![
            notice(format!("Opened its graph as {rel} — ⌘S keeps it."), NoticeLevel::Info),
            self.view_event(root, Vec::new()),
        ]
    }

    /// What the runs' workers have said since last asked.
    fn poll_runs(&mut self, root: &Path) -> Vec<Event> {
        let Some((_, told)) = &self.news else { return Vec::new() };
        let news: Vec<News> = told.try_iter().collect();
        if news.is_empty() {
            return Vec::new();
        }
        let mut saved = false;
        for item in news {
            match item {
                News::Queued { id, run, position } => {
                    if let Some(r) = self.runs.iter_mut().find(|r| r.id == id) {
                        r.run = Some(run);
                        r.position = Some(position);
                        if r.state == RunState::Sending {
                            r.state = RunState::Queued;
                        }
                    }
                }
                News::Saved { id, output } => {
                    saved |= output.kept;
                    if let Some(r) = self.runs.iter_mut().find(|r| r.id == id) {
                        r.outputs.push(output);
                    }
                }
                News::Failed { id, message } => {
                    if let Some(r) = self.runs.iter_mut().find(|r| r.id == id) {
                        if !r.state.ended() {
                            r.state = RunState::Failed;
                            r.message = Some(message);
                            r.progress = None;
                            r.preview = None;
                        }
                    }
                    self.controls.remove(&id);
                }
                News::Refused { id, message, nodes } => {
                    if let Some(r) = self.runs.iter_mut().find(|r| r.id == id) {
                        r.state = RunState::Failed;
                        r.message = Some(message);
                        r.problems =
                            nodes.into_iter().map(|(node, message)| RunProblem { node: Some(node), message }).collect();
                    }
                    self.controls.remove(&id);
                }
                News::Lost { id, message } => {
                    if let Some(r) = self.runs.iter_mut().find(|r| r.id == id) {
                        if !r.state.ended() {
                            r.state = RunState::Lost;
                            r.message = Some(message);
                            r.progress = None;
                            r.preview = None;
                        }
                    }
                    self.controls.remove(&id);
                }
                News::Event { id, event } => self.heard(id, event),
            }
        }
        let mut events = vec![self.runs_event()];
        if saved {
            events.push(self.gallery_event(root));
        }
        events
    }

    /// One event of a run, taken into what the Queue shows.
    fn heard(&mut self, id: u32, event: RunEvent) {
        let Some(r) = self.runs.iter_mut().find(|r| r.id == id) else { return };
        match event {
            RunEvent::Queued { position } => {
                r.position = Some(position);
                if !r.state.ended() {
                    r.state = RunState::Queued;
                }
            }
            RunEvent::Started => {
                r.state = RunState::Running;
                r.position = None;
            }
            RunEvent::Node { id: node, state } => {
                let seconds = match state {
                    NodeState::Running => {
                        self.began.insert((id, node.clone()), Instant::now());
                        None
                    }
                    NodeState::Done => {
                        self.began.remove(&(id, node.clone())).map(|t| (t.elapsed().as_millis() as f64) / 1000.0)
                    }
                    NodeState::Cached => None,
                };
                match r.nodes.iter_mut().find(|n| n.id == node) {
                    Some(n) => {
                        n.state = state;
                        n.seconds = seconds.or(n.seconds);
                    }
                    None => r.nodes.push(NodeRun { id: node, state, seconds }),
                }
                if state == NodeState::Done
                    && r.progress
                        .as_ref()
                        .is_some_and(|p| r.nodes.iter().any(|n| n.id == p.node && n.state == NodeState::Done))
                {
                    r.progress = None;
                    r.preview = None;
                }
            }
            RunEvent::Progress { id: node, step, steps } => r.progress = Some(RunProgress { node, step, steps }),
            RunEvent::Preview { jpeg, .. } => r.preview = Some(format!("data:image/jpeg;base64,{jpeg}")),
            RunEvent::Image { .. } => {}
            RunEvent::Error { id: node, error, message } => {
                r.state = RunState::Failed;
                r.problems.push(RunProblem { node, message: message.clone() });
                r.message = Some(match error.as_str() {
                    "oom" => format!("forge ran out of GPU memory: {message}"),
                    "missing_model" => format!("forge hasn't that model: {message}"),
                    _ => message,
                });
                r.progress = None;
                r.preview = None;
            }
            RunEvent::Done { seconds } => {
                r.state = RunState::Done;
                r.seconds = Some(seconds);
                r.progress = None;
                r.preview = None;
            }
            RunEvent::Cancelled => {
                r.state = RunState::Cancelled;
                r.progress = None;
                r.preview = None;
            }
        }
        if r.state.ended() {
            self.controls.remove(&id);
            self.began.retain(|(run, _), _| *run != id);
        }
    }

    fn runs_event(&self) -> Event {
        Event::DiffuseRuns { runs: self.runs.iter().rev().cloned().collect() }
    }

    /// The project's outputs, newest first: `outputs/` and each open flow's
    /// own folder, three deep, each PNG's head read once until it changes.
    pub fn gallery_event(&mut self, root: &Path) -> Event {
        let mut folders = vec!["outputs".to_string()];
        for (_, doc) in &self.open {
            if let Some(own) = &doc.get().outputs {
                if !folders.iter().any(|f| own.starts_with(f.as_str())) {
                    folders.push(own.clone());
                }
            }
        }
        let mut seen = BTreeMap::new();
        for folder in folders {
            let mut stack = vec![(root.join(&folder), 0)];
            while let Some((dir, depth)) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else { continue };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    if path.is_dir() {
                        if depth < 3 {
                            stack.push((path, depth + 1));
                        }
                        continue;
                    }
                    if !name.to_ascii_lowercase().ends_with(".png") {
                        continue;
                    }
                    let Ok(meta) = entry.metadata() else { continue };
                    let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    let rel =
                        path.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                    let image = match self.gallery.get(&rel) {
                        Some((when, image)) if *when == modified => image.clone(),
                        _ => {
                            let Some(head) = std::fs::File::open(&path)
                                .ok()
                                .and_then(|f| proton_diffuse::png::head(std::io::BufReader::new(f)))
                            else {
                                continue;
                            };
                            let graph = head.flow.as_deref().and_then(proton_diffuse::png::graph_of);
                            DiffuseImage {
                                folder: rel.rsplit_once('/').map(|(f, _)| f.to_string()).unwrap_or_default(),
                                path: rel.clone(),
                                made_at: modified
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0),
                                width: head.width,
                                height: head.height,
                                made: graph.as_ref().map(proton_diffuse::png::made).unwrap_or_default(),
                                graph: graph.is_some(),
                            }
                        }
                    };
                    seen.insert(rel, (modified, image));
                }
            }
        }
        self.gallery = seen;
        let mut images: Vec<DiffuseImage> = self.gallery.values().map(|(_, image)| image.clone()).collect();
        images.sort_by(|a, b| b.made_at.cmp(&a.made_at).then(b.path.cmp(&a.path)));
        images.truncate(GALLERY);
        Event::DiffuseGallery { images }
    }

    /// An image the window may read: one in the gallery, or one a run made.
    pub fn file(&self, root: &Path, rel: &str) -> Option<PathBuf> {
        // Any picture in Diffuse's folder: outputs, inputs and masks are all
        // its own.
        if !rel.to_ascii_lowercase().ends_with(".png") {
            return None;
        }
        let path = root.join(rel);
        let base = std::fs::canonicalize(root).ok()?;
        std::fs::canonicalize(&path).ok().filter(|real| real.starts_with(&base))
    }

    /// An image Diffuse may move to the Trash: one the gallery or a run listed.
    fn listed(&self, root: &Path, rel: &str) -> Option<PathBuf> {
        let listed =
            self.gallery.contains_key(rel) || self.runs.iter().any(|r| r.outputs.iter().any(|o| o.path == rel));
        listed.then(|| self.file(root, rel)).flatten()
    }

    /// A picture the window hands over — chosen, dropped, a mask painted —
    /// written into Diffuse's folder: `inputs/<name>.png` (another name if
    /// that one holds another picture), or a flow's `<flow>.<node>.mask.png`
    /// beside it. Only a PNG. Where it was written.
    pub fn put(&self, root: &Path, rel: &str, bytes: &[u8]) -> Result<String, String> {
        proton_diffuse::png::head(bytes).ok_or("only a PNG is kept")?;
        let plain = |part: &str| !part.is_empty() && !part.starts_with('.') && !part.contains(['\\', '\0', ':']);
        let parts: Vec<&str> = rel.split('/').collect();
        let input = parts.len() == 2 && parts[0] == "inputs" && rel.ends_with(".png") && plain(parts[1]);
        let mask = rel.ends_with(".mask.png") && parts.iter().all(|p| plain(p) && *p != "..");
        if !input && !mask {
            return Err(format!("{rel} is not where Diffuse keeps a picture"));
        }
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let mut rel = rel.to_string();
        if input {
            let stem = rel.trim_end_matches(".png").to_string();
            let mut n = 2;
            while let Ok(there) = std::fs::read(root.join(&rel)) {
                if there == bytes {
                    return Ok(rel);
                }
                rel = format!("{stem}-{n}.png");
                n += 1;
            }
        }
        let path = root.join(&rel);
        let base = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
        if !std::fs::canonicalize(path.parent().unwrap_or(root)).unwrap_or(base.clone()).starts_with(&base) {
            return Err(format!("{rel} leads outside Diffuse's folder"));
        }
        proton_diffuse::doc::write_atomically(&path, bytes).map_err(|e| format!("{rel}: {e}"))?;
        Ok(rel)
    }

    /// The mask painted over a flow's Load Image, where it is kept.
    pub fn mask_of(flow: &str, node: &str) -> String {
        format!("{}.{node}.mask.png", flow.trim_end_matches(EXTENSION))
    }

    /// The pictures a flow's graph sends: each upload field's file, and its
    /// node's mask when one was painted.
    fn pictures(
        root: &Path,
        path: &str,
        graph: &proton_diffuse::Graph,
        catalogue: &Catalogue,
    ) -> Result<Vec<run::Picture>, String> {
        let mut out = Vec::new();
        for node in &graph.nodes {
            let Some(kind) = catalogue.node(&node.kind) else { continue };
            for field in kind.fields().filter(|f| f.kind() == proton_diffuse::Kind::Upload) {
                let Some(rel) = node.values.get(&field.name).and_then(serde_json::Value::as_str) else { continue };
                let file = root.join(rel);
                if !file.is_file() {
                    return Err(format!("{rel} is not in Diffuse's folder: choose the picture again"));
                }
                let mask = root.join(Self::mask_of(path, &node.id));
                out.push(run::Picture {
                    node: node.id.clone(),
                    field: field.name.clone(),
                    file,
                    mask: mask.is_file().then_some(mask),
                });
            }
        }
        Ok(out)
    }

    fn person_settings(&self) -> Settings {
        self.person.clone().map(Settings::at).unwrap_or_default()
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
        Event::DiffuseKnown { known, gateways: gateways(&self.person_settings()) }
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
                Some(catalogue) => {
                    let mut found = proton_diffuse::check(doc.get(), catalogue, known.and_then(|k| k.models.as_ref()));
                    // A picture a Load Image names, gone from the folder.
                    for node in &doc.get().nodes {
                        let Some(kind) = catalogue.node(&node.kind) else { continue };
                        for field in kind.fields().filter(|f| f.kind() == proton_diffuse::Kind::Upload) {
                            if let Some(rel) = node.values.get(&field.name).and_then(serde_json::Value::as_str) {
                                if !root.join(rel).is_file() {
                                    found.push(Problem {
                                        node: Some(node.id.clone()),
                                        input: Some(field.name.clone()),
                                        message: format!("{rel} is not in Diffuse's folder"),
                                    });
                                }
                            }
                        }
                    }
                    found
                }
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
        let starters = proton_diffuse::templates::TEMPLATES
            .iter()
            .map(|(name, title, about)| DiffuseStarter {
                name: name.to_string(),
                title: title.to_string(),
                about: about.to_string(),
            })
            .collect();
        Event::DiffuseView {
            view: DiffuseView { folder: crate::folder::tilde(root), flows: find(root), open, shown, added, starters },
        }
    }
}

/// Move files to the Trash, whence they can be put back. `PROTON_TRASH`
/// names a folder to move them to instead, for tests: a test's files never
/// reach the person's Trash.
fn to_trash(files: &[PathBuf]) -> Result<(), String> {
    match std::env::var_os("PROTON_TRASH") {
        Some(bin) => {
            let bin = PathBuf::from(bin);
            std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
            for file in files {
                let name = file.file_name().ok_or("a file with no name")?;
                std::fs::rename(file, bin.join(name)).map_err(|e| format!("{}: {e}", file.display()))?;
            }
            Ok(())
        }
        None => trash::delete_all(files).map_err(|e| format!("Could not move it to the Trash: {e}")),
    }
}

/// A seed nobody chose: the standard library's hasher, keyed afresh for
/// each process, over the time — no random crate for one number.
fn random_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
    );
    hasher.finish() & proton_diffuse::catalogue::SEED_MAX
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
