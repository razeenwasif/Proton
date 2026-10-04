//! The Chat tab: Claude Code — and Codex, Gemini and Antigravity beside it —
//! a project's conversations at a time.
//!
//! Each project has its own: the sessions Claude Code keeps for its folder,
//! the conversations with the other engines Proton keeps for it, and the
//! ones open here — one on screen, any number running. A conversation that
//! is running carries on while another is looked at, as sessions do in
//! Claude Code's own app; only the one on screen is drawn, and the others
//! are drawn whole when they are next looked at.
//!
//! The engines are `proton-claude`'s. What is here is the part that is
//! Proton's: which conversation is on screen, what the list says, the engine
//! and settings a new conversation starts with, keeping each artifact the
//! moment it is published, and keeping each conversation with an engine whose
//! own files Proton does not read once a turn of it ends.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use proton_claude::agy::Antigravity;
use proton_claude::chat::{self, Answer, Chat, Image, Settings};
use proton_claude::codex::Codex;
use proton_claude::folders::{FolderView, Folders, Member};
use proton_claude::gemini::Gemini;
use proton_claude::history::{self, Titles};
use proton_claude::{engine, saved, Activity, Capabilities, Conversation, Engine, SessionSummary, Store};

use crate::bus::{ClaudeAnswer, ClaudeImage, Event};

/// Said after Claude Code's own system prompt, so it knows where its words
/// are read.
const SYSTEM: &str = "You are running inside Proton, a code editor, in its Chat tab. The person \
reads your replies there as rendered markdown. Artifacts you publish with the Artifact tool open \
in a pane beside the chat, so a one-line pointer to a new artifact is enough.";

/// How many conversations stay in memory that are neither running nor on
/// screen. Reading one back again is cheap; keeping every one ever opened
/// is not.
const KEPT: usize = 6;

/// What new conversations start with.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Defaults {
    /// Which engine answers a new conversation.
    pub engine: Engine,
    /// The permission mode, shared: each engine takes the nearest it has.
    pub mode: String,
    /// Each engine's model and effort.
    pub models: HashMap<Engine, (Option<String>, Option<String>)>,
}

pub struct Claude {
    root: PathBuf,
    chats: Vec<Box<dyn Conversation>>,
    active: Option<String>,
    titles: Titles,
    /// Claude Code's own sessions for this project.
    listed: Vec<SessionSummary>,
    /// The conversations with the other engines Proton has kept for it.
    kept: Vec<SessionSummary>,
    /// The desktop app's sessions from other folders.
    desktop: Vec<history::DesktopSession>,
    /// Sessions another Claude Code has open right now.
    elsewhere: HashSet<String>,
    /// The folders conversations are grouped into, shared by every project.
    folders: Folders,
    /// What could be read of the conversations in folders that this
    /// project's list does not have — another project's, say.
    away: HashMap<String, SessionSummary>,
    /// Which conversations were working and which were waiting on you, as
    /// the list last said.
    busy: Vec<String>,
    waiting: Vec<String>,
    /// Sessions read back without their start.
    shortened: Vec<String>,
    store: Option<Store>,
    capabilities: HashMap<Engine, Capabilities>,
    /// Claude Code's handshake, asked of a `claude` on a thread.
    probing: Option<Receiver<Capabilities>>,
    /// The other engines, likewise; asked once a run.
    probing_others: Option<Receiver<Capabilities>>,
    probed_others: bool,
    /// An import reading the sessions on a thread, until it answers.
    importing: Option<Receiver<Vec<proton_claude::import::Found>>>,
    defaults: Defaults,
}

impl std::fmt::Debug for Claude {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Claude")
            .field("root", &self.root)
            .field("open", &self.chats.len())
            .field("active", &self.active)
            .finish()
    }
}

impl Default for Claude {
    fn default() -> Claude {
        Claude::new(Path::new("."))
    }
}

/// The models on offer before any `claude` has said which it has.
fn fallback() -> Capabilities {
    let efforts: Vec<String> = ["low", "medium", "high", "xhigh", "max"].iter().map(|s| s.to_string()).collect();
    let model = |value: &str, name: &str, description: &str| proton_claude::ModelChoice {
        value: value.into(),
        resolved: String::new(),
        name: name.into(),
        description: description.into(),
        efforts: efforts.clone(),
    };
    Capabilities {
        engine: Engine::Claude,
        models: vec![
            model("default", "Default (recommended)", "Claude Code's choice"),
            model("opus", "Opus", "For everyday, complex tasks"),
            model("fable", "Fable", "For the hardest, longest-running tasks"),
            model("sonnet", "Sonnet", "Fast and capable"),
            model("haiku", "Haiku", "Fastest, for quick answers"),
        ],
        modes: Engine::Claude.modes(),
        ..Capabilities::default()
    }
}

impl Claude {
    pub fn new(root: &Path) -> Claude {
        let mut capabilities = HashMap::new();
        let mut claude = chat::remembered().unwrap_or_else(fallback);
        claude.engine = Engine::Claude;
        claude.modes = Engine::Claude.modes();
        capabilities.insert(Engine::Claude, claude);
        for engine in [Engine::Codex, Engine::Gemini, Engine::Antigravity] {
            capabilities.insert(engine, engine::fallback(engine));
        }
        for known in engine::remembered() {
            if known.engine != Engine::Claude {
                capabilities.insert(known.engine, known);
            }
        }
        Claude {
            root: root.to_path_buf(),
            chats: Vec::new(),
            active: None,
            titles: Titles::new(),
            listed: Vec::new(),
            kept: Vec::new(),
            desktop: Vec::new(),
            elsewhere: HashSet::new(),
            folders: Folders::open(),
            away: HashMap::new(),
            busy: Vec::new(),
            waiting: Vec::new(),
            shortened: Vec::new(),
            store: None,
            capabilities,
            probing: None,
            probing_others: None,
            probed_others: false,
            importing: None,
            defaults: Defaults { mode: "default".into(), ..Defaults::default() },
        }
    }

    /// Keep folders somewhere else than everyone's — for a test.
    #[cfg(test)]
    fn with_folders(mut self, folders: Folders) -> Claude {
        self.folders = folders;
        self
    }

    /// What new conversations start with, from the project's settings.
    pub fn configure(&mut self, defaults: Defaults) {
        self.defaults = defaults;
    }

    /// The settings a new conversation with an engine starts with.
    fn settings_for(&self, engine: Engine) -> Settings {
        let (model, effort) = self.defaults.models.get(&engine).cloned().unwrap_or_default();
        Settings { model, mode: engine.mode(&self.defaults.mode), effort }
    }

    fn make(&self, engine: Engine) -> Box<dyn Conversation> {
        let settings = self.settings_for(engine);
        match engine {
            Engine::Claude => Box::new(Chat::new(self.root.clone(), settings, Some(SYSTEM.into()))),
            Engine::Codex => Box::new(Codex::new(self.root.clone(), settings)),
            Engine::Gemini => Box::new(Gemini::new(self.root.clone(), settings)),
            Engine::Antigravity => Box::new(Antigravity::new(self.root.clone(), settings)),
        }
    }

    fn store(&mut self) -> &mut Store {
        self.store.get_or_insert_with(Store::open)
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.chats.iter().position(|chat| chat.id() == id)
    }

    fn active_index(&self) -> Option<usize> {
        self.active.as_deref().and_then(|id| self.index(id))
    }

    /// The engine of the conversation on screen, or the one a new
    /// conversation would have.
    pub fn active_engine(&self) -> Engine {
        self.active_index().map(|i| self.chats[i].engine()).unwrap_or(self.defaults.engine)
    }

    /// The conversation on screen, making a new one if there is none.
    fn active_chat(&mut self) -> &mut Box<dyn Conversation> {
        let index = match self.active_index() {
            Some(index) => index,
            None => {
                let chat = self.make(self.defaults.engine);
                self.active = Some(chat.id().to_string());
                self.chats.insert(0, chat);
                0
            }
        };
        &mut self.chats[index]
    }

    /* ── what the window is told ─────────────────────────────── */

    fn sessions_event(&mut self) -> Event {
        let listed = |id: &str| {
            self.listed.iter().any(|s| s.id == id) || self.kept.iter().any(|s| s.id == id) || self.desktop.iter().any(|d| d.id == id)
        };
        // The open conversations nothing has a file for yet — a new one, or
        // one whose first prompt has only just gone — lead the list.
        let mut sessions: Vec<SessionSummary> = Vec::new();
        for chat in &self.chats {
            if listed(chat.id()) {
                continue;
            }
            let title = chat.fold().status().title.clone();
            if title.is_empty() && !chat.running() && Some(chat.id()) != self.active.as_deref() {
                continue;
            }
            sessions.push(SessionSummary {
                id: chat.id().to_string(),
                title: if title.is_empty() { "New session".into() } else { title },
                updated: u64::MAX,
                cwd: if chat.cwd() == self.root.as_path() { String::new() } else { chat.cwd().display().to_string() },
                elsewhere: false,
                engine: chat.engine(),
            });
        }
        // Then the project's own, whichever engine had them, newest first.
        let mut here: Vec<SessionSummary> = self.listed.iter().chain(self.kept.iter()).cloned().collect();
        for summary in &mut here {
            summary.elsewhere = self.elsewhere.contains(&summary.id);
            // A title the conversation has given itself since the list was
            // read is the newer one.
            if let Some(chat) = self.chats.iter().find(|c| c.id() == summary.id) {
                let title = &chat.fold().status().title;
                if !title.is_empty() && (summary.title.is_empty() || chat.engine() != Engine::Claude) {
                    summary.title = title.clone();
                }
            }
        }
        here.sort_by_key(|s| std::cmp::Reverse(s.updated));
        sessions.extend(here);
        // Then the desktop app's, from every other folder.
        sessions.extend(self.desktop.iter().map(|d| SessionSummary {
            id: d.id.clone(),
            title: d.title.clone(),
            updated: d.updated,
            cwd: d.cwd.display().to_string(),
            elsewhere: self.elsewhere.contains(&d.id),
            engine: Engine::Claude,
        }));
        // What is in a folder is drawn there, from whichever project it is,
        // and not in the list as well.
        let mut folders = Vec::new();
        let mut foldered = HashSet::new();
        for folder in self.folders.list().to_vec() {
            let mut inside: Vec<SessionSummary> = folder
                .members
                .iter()
                .map(|member| {
                    foldered.insert(member.id.clone());
                    sessions.iter().find(|s| s.id == member.id).cloned().unwrap_or_else(|| self.member_summary(member))
                })
                .collect();
            inside.sort_by_key(|s| std::cmp::Reverse(s.updated));
            folders.push(FolderView { id: folder.id, name: folder.name, collapsed: folder.collapsed, sessions: inside });
        }
        sessions.retain(|s| !foldered.contains(&s.id));
        Event::ClaudeSessions {
            sessions,
            folders,
            active: self.active.clone().unwrap_or_default(),
            busy: self.busy.clone(),
            waiting: self.waiting.clone(),
            engine: self.defaults.engine,
        }
    }

    /// A conversation in a folder that this project's list does not have,
    /// as well as it can be read.
    fn member_summary(&self, member: &Member) -> SessionSummary {
        let mut summary = self.away.get(&member.id).cloned().unwrap_or_else(|| SessionSummary {
            id: member.id.clone(),
            title: if member.title.is_empty() { "Untitled".into() } else { member.title.clone() },
            updated: member.updated,
            cwd: String::new(),
            elsewhere: false,
            engine: member.engine,
        });
        summary.cwd = if same_folder(&member.cwd, &self.root) { String::new() } else { member.cwd.display().to_string() };
        summary.elsewhere = self.elsewhere.contains(&member.id);
        summary.engine = member.engine;
        summary
    }

    /// The conversation on screen, whole.
    fn snapshot(&self) -> Vec<Event> {
        let Some(index) = self.active_index() else {
            return vec![Event::ClaudeItems { session: String::new(), items: Vec::new(), reset: true, earlier: false }];
        };
        let chat = &self.chats[index];
        vec![
            Event::ClaudeItems {
                session: chat.id().to_string(),
                items: chat.fold().items().to_vec(),
                reset: true,
                earlier: self.shortened.iter().any(|id| id == chat.id()),
            },
            Event::ClaudeStatus { status: Claude::stamped(chat.as_ref(), chat.fold().status().clone()) },
        ]
    }

    fn capabilities_events(&self) -> Vec<Event> {
        Engine::ALL
            .iter()
            .filter_map(|engine| self.capabilities.get(engine))
            .map(|capabilities| Event::ClaudeCapabilities { capabilities: capabilities.clone() })
            .collect()
    }

    fn artifacts_event(&mut self) -> Event {
        let importing = self.importing.is_some();
        Event::Artifacts { artifacts: self.store().list().to_vec(), importing }
    }

    /// Read the sessions for artifacts published before Proton kept them —
    /// on a thread, because it is a gigabyte of files on a long-used machine.
    pub fn import_artifacts(&mut self) -> Vec<Event> {
        if self.importing.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new().name("proton-artifact-import".into()).spawn(move || {
                let _ = tx.send(proton_claude::import::discover());
            });
            self.importing = Some(rx);
        }
        vec![self.artifacts_event()]
    }

    /// An import that has finished reading: keep what it found.
    fn imported(&mut self) -> Vec<Event> {
        let Some(importing) = &self.importing else { return Vec::new() };
        let found = match importing.try_recv() {
            Ok(found) => found,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Vec::new(),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        self.importing = None;
        let done = self.store().import(found);
        let text = match (done.added, done.updated) {
            (0, 0) => "No artifacts to import: every one published from this machine is already here.".to_string(),
            (added, updated) => {
                let mut parts = Vec::new();
                if added > 0 {
                    parts.push(format!("{added} artifact{}", if added == 1 { "" } else { "s" }));
                }
                if updated > 0 {
                    parts.push(format!("{updated} newer version{}", if updated == 1 { "" } else { "s" }));
                }
                let remote = if done.remote > 0 {
                    format!(" {} of them no longer have a page on this machine and open on claude.ai.", done.remote)
                } else {
                    String::new()
                };
                format!("Imported {}.{remote}", parts.join(" and "))
            }
        };
        vec![self.artifacts_event(), crate::workspace::notice(text, crate::bus::NoticeLevel::Info)]
    }

    fn rescan(&mut self) {
        self.listed = history::sessions(&self.root, &mut self.titles);
        self.kept = saved::list(&self.root);
        self.elsewhere = history::running_sessions();
        // The desktop app's sessions, less the ones already listed as this
        // project's and the ones it has archived.
        let here = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        let listed: HashSet<&str> = self.listed.iter().map(|s| s.id.as_str()).collect();
        self.desktop = history::desktop_sessions()
            .into_iter()
            .filter(|d| !d.archived && !listed.contains(d.id.as_str()))
            .filter(|d| d.cwd.canonicalize().unwrap_or_else(|_| d.cwd.clone()) != here)
            .collect();
        self.refresh_folders();
    }

    /// Read again what the folders hold: from this project's lists where they
    /// have it, from the conversation's own file where they do not. What is
    /// read is kept with the folder, for the next project to show.
    fn refresh_folders(&mut self) {
        self.away.clear();
        let members: Vec<Member> = self.folders.list().iter().flat_map(|f| f.members.clone()).collect();
        for member in members {
            let known = self
                .listed
                .iter()
                .chain(self.kept.iter())
                .find(|s| s.id == member.id)
                .map(|s| (s.title.clone(), s.updated))
                .or_else(|| self.desktop.iter().find(|d| d.id == member.id).map(|d| (d.title.clone(), d.updated)));
            let (title, updated) = match known {
                Some(known) => known,
                None => {
                    let read = match member.engine {
                        Engine::Claude => history::summary(&member.cwd, &member.id, &mut self.titles),
                        _ => saved::summary(&member.id),
                    };
                    let Some(read) = read else { continue };
                    let known = (read.title.clone(), read.updated);
                    self.away.insert(member.id.clone(), read);
                    known
                }
            };
            self.folders.remember(&member.id, &title, updated);
        }
    }

    /// Everything, for a window that has just connected.
    pub fn sync(&mut self) -> Vec<Event> {
        self.rescan();
        let claude_known = self.capabilities.get(&Engine::Claude).is_some_and(|c| !c.commands.is_empty());
        if !claude_known && self.probing.is_none() && self.root.is_dir() {
            self.probing = Some(chat::probe(self.root.clone()));
        }
        if !self.probed_others && self.root.is_dir() {
            self.probed_others = true;
            self.probing_others = Some(engine::probe_others(self.root.clone()));
        }
        let mut events = vec![self.sessions_event()];
        events.extend(self.snapshot());
        events.extend(self.capabilities_events());
        events.push(self.artifacts_event());
        events
    }

    /// Read the project's sessions again.
    pub fn refresh(&mut self) -> Vec<Event> {
        self.rescan();
        vec![self.sessions_event()]
    }

    /* ── what the window asks for ────────────────────────────── */

    /// A new conversation on screen, with the engine asked for or the one new
    /// conversations start with. The one that was there carries on if it was
    /// running, and is a click away in the list.
    pub fn start(&mut self, engine: Option<Engine>) -> Vec<Event> {
        let engine = engine.unwrap_or(self.defaults.engine);
        // An empty one already on screen is the new conversation — or, with
        // another engine, gives way to it.
        if let Some(index) = self.active_index() {
            let chat = &self.chats[index];
            if chat.fold().items().is_empty() && !chat.running() {
                if chat.engine() == engine {
                    return self.snapshot();
                }
                self.chats.remove(index);
            }
        }
        let chat = self.make(engine);
        self.active = Some(chat.id().to_string());
        self.chats.insert(0, chat);
        self.forget_idle();
        let mut events = vec![self.sessions_event()];
        events.extend(self.snapshot());
        events
    }

    /// The engine new conversations start with, and a new conversation with it.
    pub fn set_engine(&mut self, engine: Engine) -> Vec<Event> {
        self.defaults.engine = engine;
        self.start(Some(engine))
    }

    /// Show a conversation, reading it back if it is not open.
    pub fn open(&mut self, id: &str) -> Vec<Event> {
        let mut events = Vec::new();
        if self.index(id).is_none() {
            let chat: Box<dyn Conversation> = match saved::load(id) {
                // One Proton kept: read back as it was drawn.
                Some(record) => match record.engine {
                    Engine::Codex => Box::new(Codex::restore(record)),
                    Engine::Gemini => Box::new(Gemini::restore(record)),
                    Engine::Antigravity => Box::new(Antigravity::restore(record)),
                    Engine::Claude => {
                        return vec![crate::workspace::notice("That conversation could not be read back.".into(), crate::bus::NoticeLevel::Warn)]
                    }
                },
                // One of Claude Code's own: read back from its file.
                None => match self.claude_session(id, &mut events) {
                    Some(chat) => Box::new(chat),
                    None => return events,
                },
            };
            if !chat.cwd().is_dir() {
                events.push(crate::workspace::notice(
                    format!("{} is gone, so this conversation can be read but not continued.", chat.cwd().display()),
                    crate::bus::NoticeLevel::Warn,
                ));
            }
            self.chats.insert(0, chat);
        }
        self.active = Some(id.to_string());
        self.forget_idle();
        events.push(self.sessions_event());
        events.extend(self.snapshot());
        events
    }

    /// A Claude Code session, read back from the file it keeps.
    fn claude_session(&mut self, id: &str, events: &mut Vec<Event>) -> Option<Chat> {
        // A desktop session runs in its own folder, not this project's, and so
        // does one in a folder that another project's list has.
        let cwd = self
            .desktop
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.cwd.clone())
            .or_else(|| self.folders.find(id).map(|(_, m)| m.cwd.clone()).filter(|cwd| !cwd.as_os_str().is_empty()))
            .unwrap_or_else(|| self.root.clone());
        let Some(file) = history::file(&cwd, id) else {
            events.push(crate::workspace::notice("That session is not in this project any more.".into(), crate::bus::NoticeLevel::Warn));
            return None;
        };
        let (lines, shortened) = match history::load(&file) {
            Ok(read) => read,
            Err(err) => {
                events.push(crate::workspace::notice(format!("Could not read that session: {err}"), crate::bus::NoticeLevel::Error));
                return None;
            }
        };
        let mut chat = Chat::resume(cwd, id, &lines, self.settings_for(Engine::Claude), Some(SYSTEM.into()));
        // Read back is not news: none of it needs sending until it is drawn,
        // and then it is drawn whole.
        let _ = chat.take();
        // Open in another Claude Code — the desktop app, a terminal — so two
        // writers would share one file: carry on from a copy instead.
        if history::running_sessions().contains(id) {
            chat.fork();
            events.push(crate::workspace::notice(
                "This session is open in another Claude Code, so what you send here continues a copy of it.".into(),
                crate::bus::NoticeLevel::Info,
            ));
        }
        if shortened {
            self.shortened.push(id.to_string());
        }
        Some(chat)
    }

    /// Conversations past the first few that are not running and not on
    /// screen are let go; they read back from their files.
    fn forget_idle(&mut self) {
        let active = self.active.clone();
        let mut kept = 0;
        self.chats.retain(|chat| {
            if chat.running() || Some(chat.id()) == active.as_deref() {
                return true;
            }
            kept += 1;
            kept <= KEPT
        });
    }

    pub fn send(&mut self, text: &str, images: &[ClaudeImage]) -> Vec<Event> {
        let images: Vec<Image> = images
            .iter()
            .map(|image| Image { media: image.media.clone(), data: image.data.clone() })
            .collect();
        let chat = self.active_chat();
        let _ = chat.send(text.trim(), &images);
        // Kept as soon as it is said, so a conversation with an engine whose
        // files Proton does not read survives Proton quitting mid-turn.
        let id = chat.id().to_string();
        self.keep(&id);
        let mut events = self.flush_active();
        events.push(self.sessions_event());
        events
    }

    pub fn interrupt(&mut self) -> Vec<Event> {
        if let Some(index) = self.active_index() {
            self.chats[index].interrupt();
        }
        self.flush_active()
    }

    pub fn answer(&mut self, request: &str, answer: ClaudeAnswer) -> Vec<Event> {
        let answer = match answer {
            ClaudeAnswer::Allow { remember } => Answer::Allow { remember },
            ClaudeAnswer::Deny { message } => Answer::Deny { message },
            ClaudeAnswer::Questions { answers } => Answer::Questions { answers },
        };
        // The question is the on-screen conversation's, but it could have
        // come from any that was running: find the one that asked.
        let index = self.chats.iter().position(|chat| {
            chat.fold().asking(request)
                || chat.fold().items().iter().any(|item| matches!(item, proton_claude::Item::Tool(t) if t.ask.as_ref().is_some_and(|a| a.request == request)))
        });
        let Some(index) = index else {
            return vec![crate::workspace::notice("That has already been answered.".into(), crate::bus::NoticeLevel::Info)];
        };
        let mut events = Vec::new();
        if let Err(why) = self.chats[index].answer(request, answer) {
            events.push(crate::workspace::notice(why, crate::bus::NoticeLevel::Warn));
        }
        events.extend(self.flush_active());
        events
    }

    pub fn set_mode(&mut self, mode: &str) -> Vec<Event> {
        self.defaults.mode = mode.to_string();
        self.active_chat().set_mode(mode);
        self.flush_active()
    }

    pub fn set_model(&mut self, model: &str) -> Vec<Event> {
        let engine = self.active_engine();
        let chosen = (!model.is_empty() && model != "default").then(|| model.to_string());
        self.defaults.models.entry(engine).or_default().0 = chosen;
        self.active_chat().set_model(model);
        self.flush_active()
    }

    pub fn set_effort(&mut self, effort: Option<&str>) -> Vec<Event> {
        let engine = self.active_engine();
        self.defaults.models.entry(engine).or_default().1 = effort.filter(|e| !e.is_empty()).map(String::from);
        self.active_chat().set_effort(effort);
        self.flush_active()
    }

    pub fn forget_artifact(&mut self, id: &str) -> Vec<Event> {
        self.store().remove(id);
        vec![self.artifacts_event()]
    }

    /* ── folders ─────────────────────────────────────────────── */

    /// A new folder, named or not, with conversations put in it.
    pub fn folder_new(&mut self, name: &str, sessions: &[String]) -> Vec<Event> {
        let folder = self.folders.create(name);
        for session in sessions {
            if let Some(member) = self.member_for(session) {
                self.folders.put(&folder, member);
            }
        }
        self.refresh_folders();
        vec![self.sessions_event()]
    }

    pub fn folder_rename(&mut self, folder: &str, name: &str) -> Vec<Event> {
        self.folders.rename(folder, name);
        vec![self.sessions_event()]
    }

    /// Delete a folder. Its conversations go back to the list.
    pub fn folder_delete(&mut self, folder: &str) -> Vec<Event> {
        self.folders.delete(folder);
        vec![self.sessions_event()]
    }

    pub fn folder_collapse(&mut self, folder: &str, collapsed: bool) -> Vec<Event> {
        self.folders.collapse(folder, collapsed);
        vec![self.sessions_event()]
    }

    /// Put a conversation in a folder — or, with none, take it out of the
    /// one it is in.
    pub fn folder_move(&mut self, session: &str, folder: Option<&str>) -> Vec<Event> {
        match folder {
            Some(folder) => {
                if let Some(member) = self.member_for(session) {
                    self.folders.put(folder, member);
                }
            }
            None => {
                self.folders.take_out(session);
            }
        }
        self.refresh_folders();
        vec![self.sessions_event()]
    }

    /// Enough of a conversation to keep it in a folder: which engine, which
    /// folder it runs in, and what it is called.
    fn member_for(&mut self, session: &str) -> Option<Member> {
        let member = |engine: Engine, cwd: PathBuf, title: &str, updated: u64| Member {
            id: session.to_string(),
            engine,
            cwd,
            title: title.to_string(),
            updated,
        };
        if let Some(chat) = self.chats.iter().find(|c| c.id() == session) {
            let title = chat.fold().status().title.clone();
            let listed = self.listed.iter().chain(self.kept.iter()).find(|s| s.id == session).map(|s| s.updated);
            return Some(member(chat.engine(), chat.cwd().to_path_buf(), &title, listed.unwrap_or_else(now)));
        }
        if let Some(s) = self.listed.iter().find(|s| s.id == session) {
            return Some(member(Engine::Claude, self.root.clone(), &s.title, s.updated));
        }
        if let Some(s) = self.kept.iter().find(|s| s.id == session) {
            return Some(member(s.engine, self.root.clone(), &s.title, s.updated));
        }
        if let Some(d) = self.desktop.iter().find(|d| d.id == session) {
            return Some(member(Engine::Claude, d.cwd.clone(), &d.title, d.updated));
        }
        self.folders.find(session).map(|(_, m)| m.clone())
    }

    /// The defaults, as they are to be saved.
    pub fn defaults(&self) -> &Defaults {
        &self.defaults
    }

    /* ── the heartbeat ───────────────────────────────────────── */

    /// What changed in the conversation on screen, as events.
    fn flush_active(&mut self) -> Vec<Event> {
        let Some(index) = self.active_index() else { return Vec::new() };
        let mut changes = self.chats[index].take();
        let kept = self.keep_published(index, std::mem::take(&mut changes.published));
        if kept {
            // Marking a row with its artifact changed it again: it goes now,
            // as it is now, rather than a beat later.
            for item in self.chats[index].take().items {
                match changes.items.iter_mut().find(|i| i.id() == item.id()) {
                    Some(old) => *old = item,
                    None => changes.items.push(item),
                }
            }
        }
        let mut events = Vec::new();
        if !changes.items.is_empty() {
            events.push(Event::ClaudeItems {
                session: self.chats[index].id().to_string(),
                items: changes.items,
                reset: false,
                earlier: false,
            });
        }
        if let Some(status) = changes.status {
            events.push(Event::ClaudeStatus { status: Claude::stamped(self.chats[index].as_ref(), status) });
        }
        if let Some(capabilities) = changes.capabilities {
            events.extend(self.learned(capabilities));
        }
        if kept {
            events.push(self.artifacts_event());
        }
        events
    }

    /// A status, named by the conversation it belongs to — the id the window
    /// knows it by — whatever the engine reported its session as.
    fn stamped(chat: &dyn Conversation, mut status: proton_claude::Status) -> proton_claude::Status {
        status.session = chat.id().to_string();
        status.engine = chat.engine();
        status
    }

    /// Keep what a conversation published, and mark its rows with it.
    fn keep_published(&mut self, index: usize, published: Vec<(String, serde_json::Value, serde_json::Value)>) -> bool {
        if published.is_empty() {
            return false;
        }
        let project = self.root.display().to_string();
        let session = self.chats[index].id().to_string();
        let store = self.store.get_or_insert_with(Store::open);
        let mut kept = false;
        for (tool, input, result) in published {
            if let Some(artifact) = store.record(&input, &result, &project, &session) {
                self.chats[index].published(&tool, artifact);
                kept = true;
            }
        }
        kept
    }

    /// What an engine offers, as it last said: remembered, and told.
    fn learned(&mut self, mut capabilities: Capabilities) -> Vec<Event> {
        let engine = capabilities.engine;
        let known = self.capabilities.get(&engine);
        // A live conversation says what it knows a piece at a time — whose
        // account, then which models — so a piece fills in what is known.
        if capabilities.models.is_empty() {
            match known {
                Some(known) if !known.models.is_empty() => capabilities.models = known.models.clone(),
                _ if engine == Engine::Claude => return Vec::new(),
                _ => {}
            }
        }
        if let Some(known) = known {
            if capabilities.plan.is_empty() && capabilities.email.is_empty() {
                capabilities.plan = known.plan.clone();
                capabilities.email = known.email.clone();
            }
            if capabilities.commands.is_empty() {
                capabilities.commands = known.commands.clone();
            }
        }
        if capabilities.modes.is_empty() {
            capabilities.modes = engine.modes();
        }
        if Some(&capabilities) == known {
            return Vec::new();
        }
        match engine {
            Engine::Claude => chat::remember(&capabilities),
            _ => engine::remember(&capabilities),
        }
        self.capabilities.insert(engine, capabilities.clone());
        vec![Event::ClaudeCapabilities { capabilities }]
    }

    /// Keep a conversation whose own files Proton does not read, as it is now.
    fn keep(&mut self, id: &str) {
        let Some(chat) = self.chats.iter().find(|c| c.id() == id) else { return };
        let Some(record) = chat.record() else { return };
        if record.items.is_empty() {
            return;
        }
        if let Err(why) = saved::save(&record) {
            eprintln!("proton: {why}");
        }
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = self.imported();
        if let Some(probing) = &self.probing {
            match probing.try_recv() {
                Ok(capabilities) => {
                    self.probing = None;
                    events.extend(self.learned(capabilities));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.probing = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        while let Some(probing) = &self.probing_others {
            match probing.try_recv() {
                Ok(capabilities) => events.extend(self.learned(capabilities)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.probing_others = None;
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }

        let active = self.active_index();
        let mut kept = false;
        for index in 0..self.chats.len() {
            if !self.chats[index].running() || Some(index) == active {
                continue;
            }
            self.chats[index].poll();
            // A conversation off screen is drawn whole when it is next looked
            // at, so its rows are not sent — but what it publishes is kept
            // now, and what it offers is learned now.
            let changes = self.chats[index].take();
            kept |= self.keep_published(index, changes.published);
            if let Some(capabilities) = changes.capabilities {
                events.extend(self.learned(capabilities));
            }
        }
        if kept {
            events.push(self.artifacts_event());
        }
        if let Some(index) = active {
            self.chats[index].poll();
            events.extend(self.flush_active());
        }
        // A fork has its own id now: the conversation is known by it, and the
        // window redraws it under that name.
        let mut renamed = false;
        for index in 0..self.chats.len() {
            if let Some(old) = self.chats[index].take_renamed() {
                renamed = true;
                let new = self.chats[index].id().to_string();
                self.folders.follow(&old, &new);
                if self.active.as_deref() == Some(old.as_str()) {
                    self.active = Some(self.chats[index].id().to_string());
                    events.extend(self.snapshot());
                }
            }
        }
        if renamed {
            self.rescan();
            events.push(self.sessions_event());
        }

        // The list's spinners and dots: who is working, who is waiting.
        let busy: Vec<String> = self
            .chats
            .iter()
            .filter(|c| matches!(c.fold().status().activity, Activity::Working | Activity::Starting))
            .map(|c| c.id().to_string())
            .collect();
        let waiting: Vec<String> = self
            .chats
            .iter()
            .filter(|c| c.fold().status().activity == Activity::Waiting)
            .map(|c| c.id().to_string())
            .collect();
        if busy != self.busy || waiting != self.waiting {
            let finished: Vec<String> =
                self.busy.iter().filter(|id| !busy.contains(id) && !waiting.contains(id)).cloned().collect();
            self.busy = busy;
            self.waiting = waiting;
            // A turn that has ended has written to its file — or, for an
            // engine whose files Proton does not read, is kept now — and may
            // have given the conversation a title.
            if !finished.is_empty() {
                for id in &finished {
                    self.keep(id);
                }
                self.rescan();
            }
            events.push(self.sessions_event());
        }
        events
    }

    /// Whether anything is running, for a host deciding how often to look.
    pub fn is_busy(&self) -> bool {
        self.probing.is_some()
            || self.probing_others.is_some()
            || self.importing.is_some()
            || self.chats.iter().any(|chat| chat.running())
    }
}

/// Whether two paths are one folder, however each was spelled.
fn same_folder(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a, b)| a == b)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(event: &Event) -> (Vec<String>, Vec<(String, Vec<String>)>) {
        let Event::ClaudeSessions { sessions, folders, .. } = event else { panic!("{event:?}") };
        let ids = sessions.iter().map(|s| s.id.clone()).collect();
        let folders = folders.iter().map(|f| (f.name.clone(), f.sessions.iter().map(|s| s.id.clone()).collect())).collect();
        (ids, folders)
    }

    #[test]
    fn a_conversation_put_in_a_folder_is_listed_there_and_not_twice() {
        let dir = std::env::temp_dir().join(format!("proton-core-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let mut claude = Claude::new(&root).with_folders(Folders::at(&dir));
        claude.start(Some(Engine::Codex));
        let chat = claude.active.clone().unwrap();

        let events = claude.folder_new("", std::slice::from_ref(&chat));
        let (top, folders) = listed(events.last().unwrap());
        assert_eq!(folders, [("New folder".to_string(), vec![chat.clone()])]);
        assert!(!top.contains(&chat), "listed in its folder and out of it");

        let Event::ClaudeSessions { folders: views, .. } = events.last().unwrap() else { unreachable!() };
        assert_eq!(views[0].sessions[0].engine, Engine::Codex, "a folder keeps which engine a conversation is with");

        let folder = views[0].id.clone();
        claude.folder_rename(&folder, "Optimisation");
        let (top, folders) = listed(&claude.folder_move(&chat, None)[0]);
        assert_eq!(folders, [("Optimisation".to_string(), Vec::<String>::new())]);
        assert!(top.contains(&chat), "taken out, it is back in the list");
        let (_, folders) = listed(&claude.folder_delete(&folder)[0]);
        assert!(folders.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
