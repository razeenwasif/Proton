//! Several projects, open at once.
//!
//! A [`Workspace`] is one project: its own buffers, its own file index, its own
//! language servers, its own agent conversation. A `Session` holds a handful of
//! them and keeps one active — which is the whole of "multiple workspaces",
//! because a project that shares a file index with another project is not a
//! separate project, it is a confused one.
//!
//! Two rules make this cheap rather than expensive:
//!
//! - **Only the active workspace draws.** Every command goes to it, and only
//!   its events reach the front end. The others are state, not screen.
//! - **Every workspace still gets polled.** A background project's language
//!   servers keep answering, and if nobody drains the mailbox it fills up and
//!   the project is stale the moment you switch back. So `Poll` goes to all of
//!   them and only the active one's events are kept.
//!
//! ## Who claims a keystroke
//!
//! Switching projects is not something a project can do to itself, so the
//! session has to get a say — but the modal engine lives inside the workspace,
//! and that is where a leader sequence is assembled. The rule that resolves it:
//! **the session claims leader prefixes the workspace did not.** A workspace
//! that reports an unresolved `␣fp` is reporting that nothing below knew what
//! it meant, and the session takes it from there. Nothing about the modal
//! engine has to know the session exists.
//!
//! ## Each mode keeps projects of its own
//!
//! Every mode lists the projects opened in it and shows one of them, so the
//! folder open in Codex is not carried into Game, Canvas or Diffuse: going
//! to a mode brings back the project it had, and leaving it brings back the
//! one the next mode had. The first time a mode is entered with nothing of
//! its own, it is given a folder of its own — `~/Proton/<Mode>`, Diffuse's
//! own folder for Diffuse — made then. A folder open in several modes is
//! still one workspace, listed and shown by each: two would be two file
//! indexes over the same files. A session made on a folder (`Session::new`,
//! as tests make one) shows it in every mode until a mode opens another.

use std::path::{Path, PathBuf};

use proton_index::Matcher;
use serde::{Deserialize, Serialize};

use crate::bus::{Command, Event, NoticeLevel, WorkspaceTab};
use crate::folder;
use crate::key::Key;
use crate::mode::ModeId;
use crate::picker::{Accepted, Picker, PickerItem, PickerKey, PickerKind};
use crate::workspace::{notice, Workspace};

/// What a leader sequence means to the session.
///
/// The keymap lives here rather than in the front end for the same reason the
/// workspace's does: a second front end must not have to reinvent it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    /// `␣fp` — find project.
    Picker,
    /// `␣pn` and `␣pp` — the next and previous project.
    Cycle(i64),
}

impl Claim {
    fn of(prefix: &str) -> Option<Claim> {
        Some(match prefix {
            "fp" => Claim::Picker,
            "pn" => Claim::Cycle(1),
            "pp" => Claim::Cycle(-1),
            _ => return None,
        })
    }
}

/// What a `:` line means to the session: the handful that are about which
/// project you are in, rather than anything inside one.
///
/// They arrive the way an unresolved leader sequence does. The workspace runs
/// every `:` command it knows and hands the rest back out as `CmdlineRun`, and
/// the session reads those before they reach the front end — so a project can
/// go on owning `:w` and `:e` without knowing that other projects exist.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExClaim {
    /// `:cd {folder}` — open it, or go to it if it is already open. A bare
    /// `:cd` asks where, with the picker `␣fp` opens.
    Cd(Option<String>),
    /// `:pwd` — say which project this is.
    Pwd,
    /// `:projects` — the picker, by name.
    Projects,
    /// `:pnext` and `:pprevious` — `␣pn` and `␣pp`, as `:bn` and `:bp` are to
    /// buffers.
    Cycle(i64),
    /// `:pclose` — close the project you are in.
    Close,
}

impl ExClaim {
    fn of(line: &str) -> Option<ExClaim> {
        let ex = crate::ex::Ex::parse(line)?;
        // Vim's minimum abbreviations where vim has the command; for the ones
        // it does not, the shortest spelling that cannot be mistaken for one.
        Some(if ex.is("cd", 2) || ex.is("chdir", 3) {
            ExClaim::Cd(ex.arg)
        } else if ex.is("pwd", 2) {
            ExClaim::Pwd
        } else if ex.is("projects", 4) {
            ExClaim::Projects
        } else if ex.is("pnext", 2) {
            ExClaim::Cycle(1)
        } else if ex.is("pprevious", 2) || ex.is("pNext", 2) {
            ExClaim::Cycle(-1)
        } else if ex.is("pclose", 2) {
            ExClaim::Close
        } else {
            return None;
        })
    }
}

/// How many projects to remember having opened. Long enough that a project you
/// touched last month is still one keystroke away, short enough that the list
/// stays a list.
const RECENTS: usize = 40;

/// The format version of the stored session. Bumped when the shape changes;
/// anything else is ignored rather than guessed at.
const VERSION: u32 = 1;

/// Which modes list a workspace in their switcher: a bit each, by
/// `ModeId::index`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Listed(u32);

impl Listed {
    fn on(mode: ModeId) -> Listed {
        Listed(1 << mode.index())
    }

    /// Listed by every mode: a session's first folder, until a mode opens
    /// another.
    fn all() -> Listed {
        Listed((1 << ModeId::ALL.len()) - 1)
    }

    fn has(self, mode: ModeId) -> bool {
        self.0 & (1 << mode.index()) != 0
    }

    fn set(&mut self, mode: ModeId, on: bool) {
        match on {
            true => self.0 |= 1 << mode.index(),
            false => self.0 &= !(1 << mode.index()),
        }
    }

    /// Whether any mode but `mode` lists it.
    fn elsewhere(self, mode: ModeId) -> bool {
        self.0 & !(1 << mode.index()) != 0
    }

    /// The modes that list it.
    fn modes(self) -> Vec<ModeId> {
        ModeId::ALL.into_iter().filter(|m| self.has(*m)).collect()
    }
}

#[derive(Debug)]
pub struct Session {
    workspaces: Vec<Workspace>,
    active: usize,
    /// Which modes list each workspace, index for index with `workspaces`.
    listed: Vec<Listed>,
    /// The project each mode showed last, by root, by `ModeId::index`: what
    /// going back to it brings back.
    shown: Vec<Option<PathBuf>>,
    /// Where a mode with no project of its own is given one: `~/Proton`, in
    /// the app. `None` — a session made for a test — puts them under the
    /// system's temporary folder, never the person's home.
    homes: Option<PathBuf>,
    /// Whether `homes` is a test's: Diffuse's folder goes there too.
    test_homes: bool,
    /// Projects opened before, most recent first. Includes the open ones.
    recents: Vec<PathBuf>,
    /// The workspace picker, when it is open. The session's rather than a
    /// workspace's, because no workspace can see the others.
    picker: Option<Picker>,
    matcher: Matcher,
    /// The window height the front end last reported.
    ///
    /// A workspace that has just been created has never been told, and would
    /// otherwise draw with the 80-line default until the next resize. Only the
    /// height is kept — the scroll position belongs to each workspace.
    height: Option<usize>,
    /// Where the open set is remembered between runs. `None` in tests, which
    /// must not touch the user's real session.
    store: Option<PathBuf>,
    /// Whether workspaces opened from here get language servers.
    servers: bool,
    /// The loaded plugins, shared by every workspace.
    ///
    /// Loaded once for the session rather than once per project: a plugin
    /// belongs to the person and not to the checkout, and loading them per
    /// workspace would mean a second wasm engine and a second compile of
    /// every component the moment you opened a second project.
    plugins: std::sync::Arc<crate::plugins::Plugins>,
    /// The investigations, where the person keeps them in a folder of their
    /// own: one Research for every project here, rather than one per project
    /// over its `research/`. Polled and stopped here, once.
    research: Option<std::sync::Arc<std::sync::Mutex<crate::research::Research>>>,
    /// The person's own settings file, where the research folder is kept.
    /// `None` in tests, which must not touch the developer's.
    person: Option<PathBuf>,
    /// How a Research over a folder of its own is made: over the engines and
    /// the machine's Lab — or, in tests, over a Lab of its own.
    make_research: fn(&Path) -> crate::research::Research,
    /// Notebook: the person's notebooks and the knowledge base they draw
    /// from, one for every project here.
    study: crate::study::Study,
    /// Diffuse: the flows open, the runs, and what forge is — the person's,
    /// kept in Diffuse's own folder whichever project is open.
    diffuse: crate::diffuse::Diffuse,
    /// Runs the agent is waiting on: the project whose agent asked, its call,
    /// the run, and when to stop waiting.
    flow_waits: Vec<(PathBuf, String, u32, std::time::Instant)>,
}

/// How long the agent waits on a run before it is told to look again later.
const FLOW_WAIT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

impl Session {
    /// One workspace, no persistence, no language servers. What tests want.
    ///
    /// The root is used as given, as `Workspace::rooted` uses it. A folder
    /// somebody typed is normalised on the way in, by `restore_from` or `open`.
    pub fn new(root: impl AsRef<Path>) -> Session {
        Session {
            workspaces: vec![Workspace::rooted(root)],
            active: 0,
            listed: vec![Listed::all()],
            shown: vec![None; ModeId::ALL.len()],
            homes: None,
            test_homes: false,
            recents: Vec::new(),
            picker: None,
            matcher: Matcher::new(),
            height: None,
            store: None,
            servers: false,
            plugins: std::sync::Arc::default(),
            research: None,
            person: None,
            make_research: crate::research::Research::kept_in,
            study: crate::study::Study::new(),
            diffuse: crate::diffuse::Diffuse::default(),
            flow_waits: Vec::new(),
        }
    }

    /// Language servers, for every workspace here and every one opened later.
    pub fn with_language_servers(mut self) -> Session {
        self.servers = true;
        for workspace in &mut self.workspaces {
            workspace.enable_language_servers();
        }
        self
    }

    /// Load the installed plugins, once, for every workspace here and every
    /// one opened later.
    ///
    /// Off by default, like the language servers and for the same reason: a
    /// test must not start a wasm engine and compile whatever happens to be
    /// installed on the machine running it.
    pub fn with_plugins_from(mut self, folder: &Path) -> Session {
        let mut plugins = crate::plugins::Plugins::default();
        plugins.load(folder);
        self.plugins = std::sync::Arc::new(plugins);
        for workspace in &mut self.workspaces {
            workspace.share_plugins(&self.plugins);
        }
        self
    }

    /// The same, from where plugins actually live.
    pub fn with_plugins(self) -> Session {
        match crate::plugins::folder() {
            Some(folder) => self.with_plugins_from(&folder),
            None => self,
        }
    }

    pub fn plugins(&self) -> &crate::plugins::Plugins {
        &self.plugins
    }

    /// One of the agent's Diffuse tools, as the session answers them: now,
    /// or once a run has ended (`flow_waited`); and what the window draws
    /// again.
    pub fn flow_tool(&mut self, name: &str, input: &serde_json::Value) -> (crate::diffuse::Said, Vec<Event>) {
        let home = self.diffuse_home();
        self.diffuse.tool(&home, name, input)
    }

    /// What `@flow` sends: Diffuse, and the flow on screen.
    pub fn flow_about(&self) -> String {
        self.diffuse.about(&self.diffuse_home())
    }

    /// What run `id` came to, once it has ended.
    pub fn flow_waited(&self, id: u32) -> Option<Result<String, String>> {
        self.diffuse.waited(id)
    }

    /// Notebook's, made otherwise: for a test, one that says which of its
    /// files are in iCloud.
    pub fn with_study(mut self, study: crate::study::Study) -> Session {
        self.study = study;
        self
    }

    /// Keep the investigations in `folder`, each a folder in it, for every
    /// project here and every one opened later — rather than in each
    /// project's `research/`.
    pub fn with_research_in(self, folder: &Path) -> Session {
        let research = (self.make_research)(folder);
        self.sharing_research(research)
    }

    /// Make a Research over a folder of its own with `make`: for tests, one
    /// whose Lab is not the machine's.
    pub fn making_research_with(mut self, make: fn(&Path) -> crate::research::Research) -> Session {
        self.make_research = make;
        self
    }

    /// The same, with the Research given: in tests, one over scripted engines.
    pub fn sharing_research(mut self, research: crate::research::Research) -> Session {
        let research = std::sync::Arc::new(std::sync::Mutex::new(research));
        for workspace in &mut self.workspaces {
            workspace.share_research(&research);
        }
        self.research = Some(research);
        self
    }

    /// Where the person's own settings say investigations are kept, if they
    /// keep them in a folder of their own: `research.folder`.
    pub fn with_research(self) -> Session {
        match crate::folder::config() {
            Some(dir) => self.with_person_settings(dir.join("settings.json")),
            None => self,
        }
    }

    /// The same, from a named settings file — which is also where a folder
    /// chosen in the Research tab is written.
    pub fn with_person_settings(mut self, file: PathBuf) -> Session {
        let folder = crate::settings::Settings::at(file.clone()).research_folder();
        self.study = std::mem::take(&mut self.study).with_person_settings(&file);
        self.diffuse.set_person(Some(file.clone()));
        self.person = Some(file);
        match folder {
            Some(folder) => self.with_research_in(&folder),
            None => self,
        }
    }

    /// Keep the investigations in the folder somebody typed, for every
    /// project, or in each project's own `research/` given nothing.
    ///
    /// Refused while a round or a run is going, since the Research that has
    /// them is let go of. A folder that is not there is made only when its
    /// parent is, so a slip of the keyboard cannot make a tree of them.
    fn set_research_folder(&mut self, typed: &str) -> Vec<Event> {
        let lock = |r: &std::sync::Arc<std::sync::Mutex<crate::research::Research>>| r.lock().map(|r| r.has_work()).unwrap_or(false);
        if self.research.as_ref().is_some_and(lock) || self.workspaces.iter().any(Workspace::research_busy) {
            return vec![notice(
                "A round or a Lab run is going. Stop it before moving where investigations are kept.".into(),
                NoticeLevel::Warn,
            )];
        }
        let typed = typed.trim().trim_end_matches(['/', '\\']);
        let mut said = Vec::new();
        let folder = match crate::settings::research_path(typed) {
            None => None,
            Some(path) if path.is_dir() => Some(folder::normalise(&path)),
            Some(path) if path.exists() => {
                return vec![notice(format!("{}: not a folder", folder::tilde(&path)), NoticeLevel::Error)];
            }
            Some(path) => match path.parent().filter(|parent| parent.is_dir()) {
                Some(_) => match std::fs::create_dir(&path) {
                    Ok(()) => {
                        said.push(notice(format!("Made {}.", folder::tilde(&path)), NoticeLevel::Info));
                        Some(folder::normalise(&path))
                    }
                    Err(e) => return vec![notice(format!("{}: {e}", folder::tilde(&path)), NoticeLevel::Error)],
                },
                None => {
                    let parent = path.parent().unwrap_or(&path);
                    return vec![notice(format!("{}: no such folder", folder::tilde(parent)), NoticeLevel::Error)];
                }
            },
        };
        if let Some(file) = &self.person {
            let value = if folder.is_some() { serde_json::Value::from(typed) } else { serde_json::Value::Null };
            if let Err(why) = crate::settings::Settings::at(file.clone()).set("research.folder", value) {
                said.push(notice(format!("Where investigations are kept could not be saved: {why}"), NoticeLevel::Warn));
            }
        }
        if let Some(old) = self.research.take() {
            old.lock().unwrap_or_else(|e| e.into_inner()).shutdown();
        }
        match folder {
            Some(folder) => {
                let research = std::sync::Arc::new(std::sync::Mutex::new((self.make_research)(&folder)));
                for workspace in &mut self.workspaces {
                    workspace.share_research(&research);
                }
                self.research = Some(research);
            }
            None => {
                for workspace in &mut self.workspaces {
                    workspace.own_research();
                }
            }
        }
        let mut events = self.active_mut().research_events();
        events.extend(said);
        events
    }

    /// Remember the open set in `file`, and reload it next time.
    pub fn storing_in(mut self, file: impl Into<PathBuf>) -> Session {
        self.store = Some(file.into());
        self
    }

    /// What the shell starts with: last time's projects, plus whichever one was
    /// asked for on the command line.
    ///
    /// `requested` is a folder the user actually named — `proton C:\src\thing`.
    /// It is opened and focused on top of the restored set, because that is
    /// what naming it meant.
    ///
    /// `fallback` is consulted only when there is nothing to restore and
    /// nothing was named. The working directory belongs here rather than in
    /// `requested`: it says something on a first run from a terminal and
    /// nothing at all on the tenth, and treating it as a request is what would
    /// put a downloads folder in the projects list for good because somebody
    /// once started the editor from there.
    pub fn restore(requested: Option<PathBuf>, fallback: Option<PathBuf>) -> Session {
        // Kernels a Proton that crashed left running: nothing reconnects to
        // one, and each holds whatever its notebook loaded. Looked for beside
        // the start rather than in its way.
        let _ = std::thread::Builder::new()
            .name("proton-reap-kernels".into())
            .spawn(proton_data::kernel::reap);
        Session::restore_from(state_file(), requested, fallback)
            .with_language_servers()
            .with_plugins()
            .with_research()
    }

    /// [`Session::restore`], from a named store and without language servers.
    ///
    /// Public because it is the only way to test any of this: a test that
    /// reached for the real session file would reopen — and then overwrite —
    /// whatever the user actually had open.
    pub fn restore_from(
        store: Option<PathBuf>,
        requested: Option<PathBuf>,
        fallback: Option<PathBuf>,
    ) -> Session {
        let saved = store.as_deref().and_then(read_state);

        // Start from something valid, then replace it if the stored set has
        // anything in it. Building an empty `Session` and filling it would mean
        // a window with no workspace for the length of this function, and a
        // panic anywhere in between.
        let start = requested
            .clone()
            .or_else(|| fallback.filter(|path| path.is_dir()))
            .or_else(folder::home)
            .unwrap_or_else(|| PathBuf::from("."));
        // Rooted the way `open` roots a folder rather than as it was typed.
        // Typed, `proton .` is a project called `.`, saved as `.`, and reopened
        // next time against wherever the app was launched from — which from
        // the Dock is `/`.
        let mut session = Session::new(folder::normalise(&start));
        // The app's session: modes start with no project of their own, given
        // folders in ~/Proton; the folder it opens on is Codex's.
        if store.is_some() {
            session.homes = folder::home().map(|home| home.join("Proton"));
            session.listed = vec![Listed::on(ModeId::Codex)];
        }
        session.store = store;
        session.recents = saved
            .as_ref()
            .map(|s| {
                s.recents
                    .iter()
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute() && !folder::is_root(path))
                    .collect()
            })
            .unwrap_or_default();

        if let Some(saved) = saved {
            // A project can be deleted, renamed, or on a drive that is not
            // plugged in. Skipping it silently is right: refusing to start
            // because a folder from last week is gone would be absurd.
            //
            // A relative path is skipped too. Only a build that saved `proton .`
            // as typed wrote one, and the folder it meant — wherever that
            // terminal was — is recorded nowhere, so reading it against this
            // launch's working directory would open some other folder.
            //
            // So is the top of the disk, here and in the recents. Builds from
            // before the shell stopped offering `/` as the working folder wrote
            // it down on a launch from the Finder without anybody choosing it,
            // and reopening it would walk the whole disk at every launch.
            // Somebody who meant it can `:cd /` again.
            let alive: Vec<PathBuf> = saved
                .open
                .iter()
                .map(PathBuf::from)
                .filter(|path| path.is_absolute() && path.is_dir() && !folder::is_root(path))
                .collect();
            if !alive.is_empty() {
                session.workspaces = alive.iter().map(|path| session.build(path)).collect();
                session.listed = vec![Listed::on(ModeId::Codex); session.workspaces.len()];
                session.active = saved.open.get(saved.active)
                    .and_then(|root| alive.iter().position(|path| path == Path::new(root)))
                    .unwrap_or_else(|| saved.active.min(session.workspaces.len() - 1));

                // Matched by root rather than by position: the list above has
                // already dropped whichever projects have gone, so the indices
                // no longer line up with what was written.
                for workspace in &mut session.workspaces {
                    let root = workspace.root().display().to_string();
                    if let Some(stored) = saved.layouts.iter().find(|l| l.root == root) {
                        workspace.restore_layout(&stored.layout);
                    }
                }
                match (&saved.modes, &saved.sides) {
                    (Some(modes), _) => session.restore_modes(modes),
                    (None, Some(sides)) => session.restore_sides(sides),
                    (None, None) => {}
                }
            }
        }

        if let Some(path) = requested {
            session.open(&path.to_string_lossy());
        }
        // Whatever you land in counts as the most recent, however you got
        // there — named, restored, or fallen back to.
        let landed = session.active().root().to_path_buf();
        session.remember(&landed);

        // Write it back before anything else happens. Without this a first run
        // records nothing at all: opening the folder that was already the
        // fallback is a focus, focusing what is already active is a no-op, and
        // a no-op does not save — so there would be nothing to restore until
        // the user happened to open a *second* project.
        session.save();
        session
    }

    pub fn active(&self) -> &Workspace {
        &self.workspaces[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active]
    }

    /// One mesh as bytes, from the workspace on screen — the binary half of
    /// `Event::Geometry`. See `Modeling::mesh_bytes`.
    pub fn mesh_bytes(&mut self, key: &str) -> Option<Vec<u8>> {
        self.active_mut().mesh_bytes(key)
    }

    /// The files with unsaved changes in every open project, named with
    /// their project when more than one is open.
    pub fn unsaved(&self) -> Vec<String> {
        let many = self.workspaces.len() > 1;
        let mut files: Vec<String> = self
            .workspaces
            .iter()
            .flat_map(|workspace| {
                let project = folder::name_of(workspace.root());
                workspace
                    .unsaved()
                    .into_iter()
                    .map(move |file| if many { format!("{project}/{file}") } else { file })
            })
            .collect();
        // And Diffuse's flows, which are in its own folder.
        files.extend(self.diffuse.unsaved().into_iter().map(|name| format!("{name} (Diffuse)")));
        files
    }

    /// Save every file with unsaved changes in every project; the ones that
    /// could not be, and why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let mut failed: Vec<String> = self.workspaces.iter_mut().flat_map(|workspace| workspace.save_unsaved()).collect();
        failed.extend(self.diffuse.save_unsaved());
        failed
    }

    /// Proton is going: what every workspace has running that would outlive
    /// it — a notebook's kernel, Research's runs and calls — is stopped.
    pub fn shutdown(&mut self) {
        for workspace in &mut self.workspaces {
            workspace.shutdown();
        }
        if let Some(research) = &self.research {
            research.lock().unwrap_or_else(|e| e.into_inner()).shutdown();
        }
        self.study.shutdown();
        // The machine's Lean, if it is being made: here, not in Research's
        // shutdown, which also runs when the investigations' folder moves.
        crate::research::quit_lean();
    }

    /// Where a file a Lab run left is, in the workspace on screen.
    pub fn lab_file(&self, key: &str) -> Option<PathBuf> {
        self.active().lab_file(key)
    }

    /// Where modes with no project of their own are given one, in place of
    /// `~/Proton`: for a test, which must never make folders in the home.
    pub fn with_mode_homes(mut self, dir: impl Into<PathBuf>) -> Session {
        self.homes = Some(dir.into());
        self.test_homes = true;
        self
    }

    /// An image Diffuse may show, in its folder.
    pub fn diffuse_file(&self, rel: &str) -> Option<PathBuf> {
        self.diffuse_reader(rel).map(|(path, _)| path)
    }

    /// Where a picture the window may read is, and the key to open it with:
    /// read off the session's lock, as the shell does.
    pub fn diffuse_reader(&self, rel: &str) -> Option<(PathBuf, Option<proton_diffuse::Seal>)> {
        self.diffuse.reader(&self.diffuse_home(), rel)
    }

    /// Seal Diffuse's files on disk, as the app does: locked until the person
    /// unlocks it.
    pub fn sealing_diffuse(mut self) -> Session {
        self.diffuse.seal_files();
        self
    }

    /// The person unlocked Diffuse — and its Gallery, with `gallery` — and
    /// the shell has the key their keychain keeps.
    pub fn unlock_diffuse(&mut self, key: proton_diffuse::Seal, gallery: bool) -> Result<Vec<Event>, String> {
        let home = self.diffuse_home();
        self.diffuse.unlock(&home, key, gallery)
    }

    /// Whether Diffuse waits for its key.
    pub fn diffuse_locked(&self) -> bool {
        self.diffuse.locked()
    }

    /// Whether anything in Diffuse's folder is sealed already.
    pub fn diffuse_has_sealed(&self) -> bool {
        self.diffuse.has_sealed(&self.diffuse_home())
    }

    /// Diffuse's folder: the person's `diffuse.folder`, or `~/Diffuse`. Made
    /// only when something is written there. A test's session — one given
    /// homes of its own, or keeping no session file — never names the
    /// person's.
    pub fn diffuse_home(&self) -> PathBuf {
        let chosen = self.person.clone().map(crate::settings::Settings::at).map(|s| s.string("diffuse.folder", "")).unwrap_or_default();
        match chosen.trim() {
            "" if self.test_homes => self.homes.clone().unwrap_or_else(std::env::temp_dir).join("Diffuse"),
            "" if self.store.is_none() => std::env::temp_dir().join(format!("proton-modes-{}", std::process::id())).join("Diffuse"),
            "" => folder::home().unwrap_or_else(std::env::temp_dir).join("Diffuse"),
            typed => folder::expand(typed),
        }
    }

    /// A picture the window hands over, written into Diffuse's folder: see
    /// `diffuse::Diffuse::put`. Where it was written.
    pub fn diffuse_put(&mut self, rel: &str, bytes: &[u8]) -> Result<String, String> {
        let home = self.diffuse_home();
        self.diffuse.put(&home, rel, bytes)
    }

    /// An output to Canvas or Game, in the project that mode shows — whose
    /// own it is, now each mode has its own: Canvas makes a canvas of it, as
    /// of a picture dropped on it; Game writes it into the open game's `art/`
    /// and brings it into the room on screen.
    fn diffuse_send(&mut self, path: &str, to: &str) -> Vec<Event> {
        let home = self.diffuse_home();
        // Sent as a plain picture: Canvas and Game keep theirs so.
        let bytes = match self.diffuse.image(&home, path) {
            Some(Ok(bytes)) => bytes,
            Some(Err(why)) => return vec![notice(why, NoticeLevel::Error)],
            None => return vec![notice(format!("{path} is not an image Diffuse may send"), NoticeLevel::Warn)],
        };
        let name = path.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("image.png").to_string();
        match to {
            "canvas" => {
                let mut events = self.set_mode(ModeId::Raster);
                match self.active_mut().raster_put_blob(bytes) {
                    Ok(blob) => events.extend(self.delegate(Command::RasterImport {
                        path: None,
                        blob: Some(blob),
                        layer: false,
                        above: None,
                        name: Some(name),
                    })),
                    Err(why) => events.push(notice(why, NoticeLevel::Error)),
                }
                events
            }
            "game" => {
                let mut events = self.set_mode(ModeId::Game);
                let Some(game) = self.active().game_dir() else {
                    events.push(notice("open a game in Game, then send the picture to it again".into(), NoticeLevel::Warn));
                    return events;
                };
                let stem = name.trim_end_matches(".png");
                let mut rel = format!("art/{stem}.png");
                let mut n = 2;
                while game.join(&rel).exists() {
                    rel = format!("art/{stem}-{n}.png");
                    n += 1;
                }
                if let Err(e) = proton_diffuse::doc::write_atomically(&game.join(&rel), &bytes) {
                    events.push(notice(format!("{rel}: {e}"), NoticeLevel::Error));
                    return events;
                }
                events.extend(self.delegate(Command::GameImport { source: rel, room: None, scale: None, normals: false }));
                events
            }
            other => vec![notice(format!("Diffuse sends to Canvas or Game, not {other}"), NoticeLevel::Warn)],
        }
    }

    /// Keep Diffuse's flows and images in another folder — none, the default
    /// — written to the person's own settings. Refused while a flow has
    /// changes, which would be left behind.
    fn set_diffuse_folder(&mut self, folder: Option<String>) -> Vec<Event> {
        let unsaved = self.diffuse.unsaved();
        if !unsaved.is_empty() {
            return vec![notice(format!("save or discard {}'s changes before moving Diffuse's folder", unsaved.join(", ")), NoticeLevel::Warn)];
        }
        let Some(file) = self.person.clone() else {
            return vec![notice("Proton has no settings of your own to keep Diffuse's folder in".into(), NoticeLevel::Warn)];
        };
        let typed = folder.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
        if let Some(typed) = &typed {
            let path = folder::expand(typed);
            if path.exists() && !path.is_dir() {
                return vec![notice(format!("{typed} is a file, not a folder"), NoticeLevel::Warn)];
            }
        }
        let value = typed.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null);
        if let Err(why) = crate::settings::Settings::at(file).set("diffuse.folder", value) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        self.diffuse.moved();
        let home = self.diffuse_home();
        let mut events = self.diffuse.enter(&home);
        events.extend(self.diffuse.apply(&home, Command::DiffuseLook { again: false }));
        events
    }

    /// Where an image of the game open in the project on screen is.
    pub fn game_file(&self, key: &str) -> Option<PathBuf> {
        self.active().game_file(key)
    }

    /// A picture the window hands over — pasted, dropped — kept by Canvas in
    /// the project on screen, to bring in: its number.
    pub fn raster_put_blob(&mut self, bytes: Vec<u8>) -> Result<u32, String> {
        self.active_mut().raster_put_blob(bytes)
    }

    /// What Canvas last copied there, as a PNG, for the system's clipboard.
    pub fn raster_clip_png(&self) -> Option<Vec<u8>> {
        self.active().raster_clip_png()
    }

    /// Tiles of the canvas open in the project on screen, shared.
    pub fn raster_tiles(&self, generation: u32, layer: u32, tiles: &[u32]) -> Option<crate::raster::RasterTiles> {
        self.active().raster_tiles(generation, layer, tiles)
    }

    /// A file some workspace here opened in the document viewer — the page
    /// may still be showing one from a project it has since left.
    pub fn document_file(&self, path: &str) -> Option<PathBuf> {
        self.workspaces.iter().find_map(|w| w.document_file(Path::new(path))).or_else(|| self.study.document_file(Path::new(path)))
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn recents(&self) -> &[PathBuf] {
        &self.recents
    }

    /* ── the bus ──────────────────────────────────────────────────── */

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        let before = self.shape();
        let chats = self.chats();
        let events = self.dispatch(command);
        // Saved when the arrangement moved, rather than after a list of
        // commands somebody has to remember to extend. A command added next
        // year that opens a file in a pane is covered by this and would not
        // have been covered by an enumeration. The same goes for which
        // conversation each agent panel has open.
        if self.shape() != before || self.chats() != chats {
            self.save();
        }
        events
    }

    /// Every workspace's arrangement, as integers. See `Workspace::shape`.
    fn shape(&self) -> Vec<Vec<usize>> {
        self.workspaces.iter().map(Workspace::shape).collect()
    }

    /// The conversation each workspace's agent has open.
    fn chats(&self) -> Vec<Option<PathBuf>> {
        self.workspaces
            .iter()
            .map(|workspace| workspace.open_chat().map(Path::to_path_buf))
            .collect()
    }

    fn dispatch(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::Poll => self.poll(),

            Command::OpenWorkspace { path } => self.open(&path),
            // By place in the switcher, which lists the side on screen.
            Command::FocusWorkspace { index } => match self.listed_on(self.mode()).get(index) {
                Some(&index) => self.focus(index),
                None => vec![notice(format!("no workspace {index}"), NoticeLevel::Warn)],
            },
            Command::AskToQuit => {
                let files = self.unsaved();
                match files.is_empty() {
                    true => Vec::new(),
                    false => vec![Event::Unsaved { files }],
                }
            }
            Command::CloseWorkspace { index } => match self.listed_on(self.mode()).get(index) {
                Some(&index) => self.close(index),
                None => vec![notice(format!("no workspace {index}"), NoticeLevel::Warn)],
            },
            Command::CycleWorkspace { by } => self.cycle(by),
            Command::OpenWorkspacePicker => self.open_picker(),
            Command::SetMode { mode } => {
                let mut events = self.set_mode(mode);
                if mode == ModeId::Study {
                    events.extend(self.study.enter());
                }
                if mode == ModeId::Diffuse {
                    let home = self.diffuse_home();
                    events.extend(self.diffuse.enter(&home));
                }
                events
            }
            Command::ResearchFolder { folder } => self.set_research_folder(&folder),
            // Notebook's are the session's: a notebook is the person's, not a project's.
            command if crate::study::handles(&command) => self.study.apply(command),
            // As are Diffuse's: its flows and images are in a folder of its own.
            Command::DiffuseFolder { folder } => self.set_diffuse_folder(folder),
            Command::DiffuseSend { path, to } => self.diffuse_send(&path, &to),
            command if crate::diffuse::handles(&command) => {
                let home = self.diffuse_home();
                self.diffuse.apply(&home, command)
            }

            // While the workspace picker is open it owns the keyboard, exactly
            // as Telescope does inside a workspace — and for the same reason.
            Command::Key { key } if self.picker.is_some() => self.picker_key(&key),
            Command::PickerQuery { text } if self.picker.is_some() => {
                if let Some(picker) = &mut self.picker {
                    picker.set_query(text);
                }
                self.requery();
                vec![self.picker_event()]
            }
            Command::SelectPick { index } if self.picker.is_some() => {
                if let Some(picker) = &mut self.picker {
                    picker.select(index);
                }
                vec![self.picker_event()]
            }

            Command::Viewport {
                view,
                first_line,
                lines,
            } => {
                // Only the first split's height is remembered as the default
                // for a workspace opened later: a new one starts unsplit, so
                // that is the height it will have.
                if view == 0 {
                    self.height = Some(lines.max(1));
                }
                self.delegate(Command::Viewport {
                    view,
                    first_line,
                    lines,
                })
            }

            Command::Sync => {
                let mut events = self.delegate(Command::Sync);
                events.push(self.workspaces_event());
                let home = self.diffuse_home();
                events.extend(self.diffuse.events(&home));
                events
            }

            // What `@flow` stands for is the session's to say: the flows are
            // its.
            Command::Ask { text } => {
                let mentioned = crate::mention::found(&text);
                if mentioned.contains(&crate::mention::Mention::Flow) {
                    let about = self.flow_about();
                    self.active_mut().set_flow_about(about);
                }
                self.delegate(Command::Ask { text })
            }

            other => self.delegate(other),
        }
    }

    /// Hand a command to the active workspace, and take back anything it did
    /// not resolve.
    fn delegate(&mut self, command: Command) -> Vec<Event> {
        let mode = self.mode();
        let mut events = self.active_mut().apply(command);
        events.extend(self.crossed(mode));
        self.claim(events)
    }

    /// A workspace that changed mode by itself took its project along: a
    /// notebook opened from Codex is Data's document. The mode it left goes
    /// on showing that project and the mode
    /// it went to shows it now, so both list it — and the switcher on screen
    /// is the new mode's.
    fn crossed(&mut self, from: ModeId) -> Option<Event> {
        let to = self.mode();
        if to == from {
            return None;
        }
        let index = self.active;
        let root = self.workspaces[index].root().to_path_buf();
        self.listed[index].set(to, true);
        self.shown[from.index()] = Some(root.clone());
        self.shown[to.index()] = Some(root);
        self.save();
        Some(self.workspaces_event())
    }

    /// The session's right of first refusal on unresolved leader sequences.
    ///
    /// A workspace reporting `Leader { prefix: Some("fp") }` is reporting that
    /// nothing below knew what it meant — which-key is showing it and waiting
    /// for more. That is exactly the moment for the layer above to say it was
    /// theirs all along.
    fn claim(&mut self, events: Vec<Event>) -> Vec<Event> {
        // A `:` line the workspace did not know, which may be one of ours.
        // Anything else it did not know goes on to the front end untouched,
        // which is what says "not a command yet".
        if let Some(ex) = events.iter().find_map(|event| match event {
            Event::CmdlineRun { text } => ExClaim::of(text),
            _ => None,
        }) {
            let mut out: Vec<Event> = events
                .into_iter()
                .filter(|event| !matches!(event, Event::CmdlineRun { .. }))
                .collect();
            out.extend(self.ex(ex));
            return out;
        }

        let Some(claim) = events.iter().find_map(|event| match event {
            Event::Leader { prefix: Some(p) } => Claim::of(p),
            _ => None,
        }) else {
            return events;
        };

        // The workspace is still holding half a sequence that has now been
        // resolved somewhere it cannot see.
        self.active_mut().cancel_leader();
        let mut out: Vec<Event> = events
            .into_iter()
            .filter(|event| !matches!(event, Event::Leader { .. }))
            .collect();
        match claim {
            Claim::Picker => out.extend(self.open_picker()),
            Claim::Cycle(by) => {
                // Close which-key on the way: the sequence resolved.
                out.push(Event::Leader { prefix: None });
                out.extend(self.cycle(by));
            }
        }
        out
    }

    /// Run a `:` command that was the session's.
    fn ex(&mut self, ex: ExClaim) -> Vec<Event> {
        match ex {
            ExClaim::Cd(None) | ExClaim::Projects => self.open_picker(),
            // `cd -`, as a shell has it: back to wherever you were before.
            // The recents are most recent first, so that is the first one that
            // is not this — or, when the project the window started in was
            // never recorded as visited, the first other one that is open.
            ExClaim::Cd(Some(to)) if to == "-" => {
                // Compared canonically, for the reason `index_of` gives. Back
                // on this side: a project only the other side has open is not
                // where you were.
                let here = folder::normalise(self.active().root());
                let side = self.mode();
                let theirs: Vec<PathBuf> = (0..self.workspaces.len())
                    .filter(|&index| !self.listed[index].has(side))
                    .map(|index| folder::normalise(self.workspaces[index].root()))
                    .collect();
                let previous = self
                    .recents
                    .iter()
                    .map(|path| folder::normalise(path))
                    .find(|path| *path != here && !theirs.contains(path))
                    .or_else(|| {
                        self.listed_on(side)
                            .into_iter()
                            .map(|index| folder::normalise(self.workspaces[index].root()))
                            .find(|path| *path != here)
                    });
                match previous {
                    Some(previous) => self.open(&previous.display().to_string()),
                    None => vec![notice(
                        "no previous project to go back to".into(),
                        NoticeLevel::Warn,
                    )],
                }
            }
            ExClaim::Cd(Some(to)) => {
                // Relative to the project you are in, because that is the only
                // directory this window has. The process's own working
                // directory is wherever the app happened to be launched from.
                let path = folder::expand(&to);
                let path = match path.is_relative() {
                    true => self.active().root().join(path),
                    false => path,
                };
                self.open(&path.display().to_string())
            }
            ExClaim::Pwd => vec![notice(
                self.active().root().display().to_string(),
                NoticeLevel::Info,
            )],
            ExClaim::Cycle(by) => match self.listed_on(self.mode()).len() {
                // Saying so beats doing nothing: `:pn` with one project open
                // is usually someone who meant to open a second.
                1 => vec![notice(
                    "one project open — :cd opens another".into(),
                    NoticeLevel::Info,
                )],
                _ => self.cycle(by),
            },
            ExClaim::Close => {
                let index = self.active;
                self.close(index)
            }
        }
    }

    /// Drain every workspace, show only the active one.
    fn poll(&mut self) -> Vec<Event> {
        let active = self.active;
        let side = self.mode();
        let mut events = Vec::new();
        let mut drawn = Vec::new();
        let home = self.diffuse_home();
        for (index, workspace) in self.workspaces.iter_mut().enumerate() {
            workspace.set_quiet(index != active);
            let produced = workspace.apply(Command::Poll);
            workspace.set_quiet(false);
            // The agent's Diffuse tools, answered from the session's flows —
            // whichever project's agent asked, since they are none's. A run
            // it waits on is answered once it has ended.
            for (id, name, input) in workspace.take_session_calls() {
                let (said, events) = self.diffuse.tool(&home, &name, &input);
                match said {
                    crate::diffuse::Said::Now(answer) => workspace.answer_tool(&id, answer),
                    crate::diffuse::Said::Wait(run) => self.flow_waits.push((
                        workspace.root().to_path_buf(),
                        id,
                        run,
                        std::time::Instant::now() + FLOW_WAIT,
                    )),
                }
                drawn.extend(events);
            }
            if index == active {
                events = produced;
            }
            // A background project's diagnostics are not on screen, so they are
            // dropped here — but they had to be *collected*, or its servers
            // block and the project is stale when you come back to it.
        }
        // Investigations kept in a folder of their own are every project's,
        // so what their rounds said is on screen whichever one is.
        if let Some(research) = &self.research {
            events.extend(research.lock().unwrap_or_else(|e| e.into_inner()).poll());
        }
        events.extend(self.study.poll());
        events.extend(self.diffuse.poll(&home));
        // Runs the agent waited on that have ended — or that it has waited
        // on long enough.
        let mut waits = std::mem::take(&mut self.flow_waits);
        waits.retain(|(root, call, run, until)| {
            let answer = match self.diffuse.waited(*run) {
                Some(answer) => answer,
                None if std::time::Instant::now() >= *until => Ok(format!(
                    "Run {run} is still going after {} minutes; flow_info says how it goes.",
                    FLOW_WAIT.as_secs() / 60
                )),
                None => return true,
            };
            // A project closed meanwhile has no turn to answer.
            if let Some(workspace) = self.workspaces.iter_mut().find(|w| w.root() == root.as_path()) {
                workspace.answer_tool(call, answer);
            }
            false
        });
        self.flow_waits = waits;
        events.extend(drawn);
        events.extend(self.crossed(side));
        self.claim(events)
    }

    /* ── opening, closing, switching ──────────────────────────────── */

    fn build(&self, root: &Path) -> Workspace {
        let mut workspace = Workspace::rooted(root).sharing_plugins(&self.plugins).sharing_research(self.research.as_ref());
        if self.servers {
            workspace.enable_language_servers();
        }
        if let Some(height) = self.height {
            workspace.set_height(height);
        }
        workspace
    }

    /// Open a folder, or focus it if it is already open.
    ///
    /// Focusing rather than opening a second copy is not a nicety: two
    /// workspaces on one folder means two file indexes, two language servers,
    /// and two sets of buffers over the same files that can disagree about what
    /// is on disk.
    pub fn open(&mut self, path: &str) -> Vec<Event> {
        let root = folder::normalise(&folder::expand(path));
        if !root.is_dir() {
            return vec![notice(
                format!("{}: not a folder", root.display()),
                NoticeLevel::Error,
            )];
        }
        if let Some(index) = self.index_of(&root) {
            return self.focus(index);
        }

        // Opened in the mode on screen, and listed by it: a folder opened
        // from Data is Data's.
        let mode = self.active().mode();
        let workspace = self.build(&root);
        self.workspaces.push(workspace);
        self.listed.push(Listed::on(mode));
        self.shown[mode.index()] = Some(root.clone());
        self.active = self.workspaces.len() - 1;
        let mut events = self.active_mut().apply(Command::SetMode { mode });
        self.remember(&root);
        self.save();
        events.extend(self.enter());
        events
    }

    fn focus(&mut self, index: usize) -> Vec<Event> {
        if index >= self.workspaces.len() {
            return vec![notice(format!("no workspace {index}"), NoticeLevel::Warn)];
        }
        if index == self.active {
            return Vec::new();
        }
        // The project changes and the mode does not. It comes to the mode on
        // screen, whichever mode it was last shown in: picking a project is
        // not picking a mode.
        let mode = self.active().mode();
        self.listed[index].set(mode, true);
        self.shown[mode.index()] = Some(self.workspaces[index].root().to_path_buf());
        self.active = index;
        let mut events = self.active_mut().apply(Command::SetMode { mode });
        let root = self.active().root().to_path_buf();
        self.remember(&root);
        self.save();
        events.extend(self.enter());
        events
    }

    /// Step through the projects of the mode on screen, wrapping.
    fn cycle(&mut self, by: i64) -> Vec<Event> {
        let listed = self.listed_on(self.mode());
        let count = listed.len() as i64;
        if count < 2 {
            return Vec::new();
        }
        let at = listed.iter().position(|&index| index == self.active).unwrap_or(0) as i64;
        self.focus(listed[(at + by).rem_euclid(count) as usize])
    }

    /// Change mode, and with it the project: going to a mode brings back
    /// the one it showed last, or the first it lists, or — the first time,
    /// with none — a folder of its own.
    fn set_mode(&mut self, mode: ModeId) -> Vec<Event> {
        let from = self.mode();
        if from == mode {
            return self.delegate(Command::SetMode { mode });
        }
        let here = self.active().root().to_path_buf();
        self.shown[from.index()] = Some(here);
        let remembered =
            self.shown[mode.index()].clone().and_then(|root| self.index_of(&root)).filter(|&i| self.listed[i].has(mode));
        let there = match remembered.or_else(|| self.listed_on(mode).first().copied()) {
            Some(index) => index,
            None => match self.home_for(mode) {
                Ok(index) => index,
                Err(why) => {
                    // No folder of its own could be made: the mode shows the
                    // project on screen rather than nothing.
                    let mut events = vec![notice(why, NoticeLevel::Warn)];
                    let index = self.active;
                    self.listed[index].set(mode, true);
                    events.extend(self.active_mut().apply(Command::SetMode { mode }));
                    events.push(self.workspaces_event());
                    return events;
                }
            },
        };
        self.listed[there].set(mode, true);
        self.shown[mode.index()] = Some(self.workspaces[there].root().to_path_buf());

        // A prompt over one mode's projects is not a prompt over another's.
        let mut events = Vec::new();
        if self.picker.take().is_some() {
            events.push(Event::PickerClosed);
        }
        if there == self.active {
            events.extend(self.active_mut().apply(Command::SetMode { mode }));
            events.push(self.workspaces_event());
        } else {
            self.active = there;
            events.extend(self.active_mut().apply(Command::SetMode { mode }));
            events.extend(self.enter());
        }
        self.save();
        events
    }

    /// A mode's own folder, made and opened, listed by it: Diffuse's folder
    /// for Diffuse, `<homes>/<Mode>` for the rest.
    fn home_for(&mut self, mode: ModeId) -> Result<usize, String> {
        let dir = match mode {
            ModeId::Diffuse => self.diffuse_home(),
            _ => self
                .homes
                .clone()
                .unwrap_or_else(|| std::env::temp_dir().join(format!("proton-modes-{}", std::process::id())))
                .join(mode.label()),
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", folder::tilde(&dir)))?;
        let root = folder::normalise(&dir);
        if let Some(index) = self.index_of(&root) {
            return Ok(index);
        }
        let workspace = self.build(&root);
        self.workspaces.push(workspace);
        self.listed.push(Listed::on(mode));
        self.remember(&root);
        Ok(self.workspaces.len() - 1)
    }

    fn close(&mut self, index: usize) -> Vec<Event> {
        if index >= self.workspaces.len() {
            return vec![notice(format!("no workspace {index}"), NoticeLevel::Warn)];
        }
        let side = self.mode();
        let listed = self.listed_on(side);
        if listed.len() <= 1 {
            // There is no editor without a project, and an empty window with a
            // "+" in it is a worse answer than refusing. A mode is the same.
            return vec![notice(
                "that is the last workspace — open another first".into(),
                NoticeLevel::Warn,
            )];
        }
        // Where to land, if it is the one on screen: its neighbour in this
        // mode's list.
        let landing = (index == self.active).then(|| {
            let at = listed.iter().position(|&i| i == index).unwrap_or(0);
            listed[if at + 1 < listed.len() { at + 1 } else { at.saturating_sub(1) }]
        });

        // Open in another mode as well: it stays open there and only leaves
        // this mode's list. Nothing is discarded, so unsaved work is no reason
        // to refuse.
        if self.listed[index].elsewhere(side) {
            self.listed[index].set(side, false);
            return match landing {
                Some(next) => self.focus(next),
                None => {
                    self.save();
                    vec![self.workspaces_event()]
                }
            };
        }

        if self.workspaces[index].is_modified() {
            // Closing takes the buffers with it and there is no undo for that.
            let name = folder::name_of(self.workspaces[index].root());
            return vec![notice(
                format!("{name} has unsaved changes"),
                NoticeLevel::Warn,
            )];
        }

        let mode = self.active().mode();
        // Shut down rather than dropped: its notebook, if it has changes, is
        // saved, and its kernel stopped with what it started.
        self.workspaces.remove(index).shutdown();
        self.listed.remove(index);
        // Everything after it shifted down by one.
        let shifted = |i: usize| if i > index { i - 1 } else { i };
        match landing {
            Some(next) => {
                self.active = shifted(next);
                let mut events = self.active_mut().apply(Command::SetMode { mode });
                self.save();
                events.extend(self.enter());
                events
            }
            None => {
                // The same project is still active, and nothing needs
                // redrawing but the switcher.
                self.active = shifted(self.active);
                self.save();
                vec![self.workspaces_event()]
            }
        }
    }

    /// Everything the front end has to redraw when the active workspace
    /// changes — which is everything, because every panel was showing the other
    /// project.
    fn enter(&mut self) -> Vec<Event> {
        if let Some(height) = self.height {
            self.active_mut().set_height(height);
        }
        let mut events = self.active_mut().apply(Command::Sync);
        events.push(self.workspaces_event());
        events
    }

    /// Which open workspace is this folder, if any. `root` is normalised by the
    /// caller; each workspace's is normalised here, because not every root
    /// was — `Session::new` takes its root as given, and a session file written
    /// before `restore_from` normalised can name a folder through a symlink —
    /// and comparing that spelling with a canonical one would open the same
    /// folder twice.
    fn index_of(&self, root: &Path) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|w| w.root() == root || folder::normalise(w.root()) == root)
    }

    /// The mode on screen.
    fn mode(&self) -> ModeId {
        self.active().mode()
    }

    /// The workspaces a mode lists, in the order they were opened.
    fn listed_on(&self, side: ModeId) -> Vec<usize> {
        (0..self.workspaces.len())
            .filter(|&index| self.listed[index].has(side))
            .collect()
    }

    /// Which mode lists what, and what each shows, as they were saved.
    fn restore_modes(&mut self, modes: &[StoredMode]) {
        for listed in self.listed.iter_mut() {
            *listed = Listed::default();
        }
        for stored in modes {
            let Some(mode) = ModeId::from_slug(&stored.mode) else { continue };
            for root in &stored.listed {
                if let Some(index) = self.index_of(Path::new(root)) {
                    self.listed[index].set(mode, true);
                }
            }
            self.shown[mode.index()] = stored
                .shown
                .as_deref()
                .and_then(|root| self.index_of(Path::new(root)))
                .map(|i| self.workspaces[i].root().to_path_buf());
        }
        self.settle_listing();
    }

    /// The two sides of a session from before every mode kept its own:
    /// Codex's projects are Codex's, Data's are Data's, Research's and
    /// Notebook's — and every other mode starts on a folder of its own.
    fn restore_sides(&mut self, sides: &StoredSides) {
        const DATA_SIDE: [ModeId; 3] = [ModeId::Data, ModeId::Research, ModeId::Study];
        for (listed, workspace) in self.listed.iter_mut().zip(&self.workspaces) {
            let root = workspace.root().display().to_string();
            *listed = Listed::default();
            if sides.code.contains(&root) {
                listed.set(ModeId::Codex, true);
            }
            if sides.data.contains(&root) {
                for mode in DATA_SIDE {
                    listed.set(mode, true);
                }
            }
        }
        let shown = sides
            .shown
            .as_deref()
            .and_then(|root| self.index_of(Path::new(root)))
            .or_else(|| self.listed.iter().position(|listed| listed.has(ModeId::Data)));
        if let Some(index) = shown {
            for mode in DATA_SIDE {
                self.listed[index].set(mode, true);
                self.shown[mode.index()] = Some(self.workspaces[index].root().to_path_buf());
            }
        }
        self.settle_listing();
    }

    /// After a restore: nothing open and out of every mode's reach, and the
    /// window opens in Codex, on the project Codex had.
    fn settle_listing(&mut self) {
        for listed in self.listed.iter_mut() {
            if *listed == Listed::default() {
                listed.set(ModeId::Codex, true);
            }
        }
        let active = self.active;
        self.listed[active].set(ModeId::Codex, true);
        self.shown[ModeId::Codex.index()] = Some(self.workspaces[active].root().to_path_buf());
    }

    /* ── the workspace picker ─────────────────────────────────────── */

    fn open_picker(&mut self) -> Vec<Event> {
        self.picker = Some(Picker::over(PickerKind::Workspaces));
        self.requery();
        // Close which-key on the way: the leader sequence resolved.
        vec![Event::Leader { prefix: None }, self.picker_event()]
    }

    fn picker_key(&mut self, key: &Key) -> Vec<Event> {
        // Tab completes the highlighted folder and keeps going, the way `:e`
        // does. Without it, typing a path means typing every character of it.
        if key.key == "Tab" {
            return self.complete_selection();
        }

        match PickerKey::of(key) {
            PickerKey::Cancel => {
                self.picker = None;
                return vec![Event::PickerClosed];
            }
            PickerKey::Accept => return self.accept(),
            PickerKey::Move(by) => {
                if let Some(picker) = &mut self.picker {
                    picker.move_by(by);
                }
            }
            PickerKey::Backspace => {
                let emptied = match &mut self.picker {
                    Some(picker) => !picker.backspace(),
                    None => true,
                };
                if emptied {
                    self.picker = None;
                    return vec![Event::PickerClosed];
                }
                self.requery();
            }
            PickerKey::Type(c) => {
                if let Some(picker) = &mut self.picker {
                    picker.type_char(c);
                }
                self.requery();
            }
            PickerKey::Ignored => return Vec::new(),
        }
        vec![self.picker_event()]
    }

    /// Replace the query with the highlighted folder, ready for the next
    /// segment.
    fn complete_selection(&mut self) -> Vec<Event> {
        let Some(path) = self
            .picker
            .as_ref()
            .and_then(|p| p.selected_item())
            .and_then(|item| item.path.clone())
        else {
            return vec![self.picker_event()];
        };
        if let Some(picker) = &mut self.picker {
            // The trailing separator is what makes the next keystroke list the
            // folder's contents rather than re-matching its siblings.
            picker.set_query(format!("{path}{}", std::path::MAIN_SEPARATOR));
        }
        self.requery();
        vec![self.picker_event()]
    }

    fn accept(&mut self) -> Vec<Event> {
        let accepted = self
            .picker
            .as_ref()
            .map(|picker| picker.accept())
            .unwrap_or(Accepted::Nothing);
        self.picker = None;

        let mut events = vec![Event::PickerClosed];
        if let Accepted::OpenWorkspace { path } = accepted {
            events.extend(self.open(&path));
        }
        events
    }

    /// Rebuild the candidate list for the query as it stands.
    ///
    /// Two lists in one prompt, chosen by whether the query looks like a path:
    /// a name matches against projects you already know, and a path lists what
    /// is actually in that folder. Which is what the two questions "which of my
    /// projects" and "where on this disk" each need, and neither can answer for
    /// the other.
    fn requery(&mut self) {
        let started = std::time::Instant::now();
        let query = match &self.picker {
            Some(picker) => picker.query().to_string(),
            None => return,
        };

        let items = if folder::looks_like_path(&query) {
            folder::complete(&query, folder::LIMIT)
                .into_iter()
                .map(|path| PickerItem {
                    label: folder::name_of(&path),
                    detail: path.display().to_string(),
                    positions: Vec::new(),
                    path: Some(path.display().to_string()),
                    line: None,
                    column: None,
                    kind: None,
                })
                .collect()
        } else {
            self.known(&query)
        };

        if let Some(picker) = &mut self.picker {
            picker.fill(items, started.elapsed().as_secs_f64() * 1000.0);
        }
    }

    /// Projects already open, then ones opened before, fuzzy-matched by name.
    ///
    /// The open ones come first and say so. Choosing one is not an error — it
    /// is how you switch — and hiding them would make the picker useless for
    /// exactly the thing it is most often wanted for.
    fn known(&mut self, query: &str) -> Vec<PickerItem> {
        // This mode's projects, then the ones only other modes have open,
        // which say which: choosing one brings it to this mode as well.
        let side = self.mode();
        let mut open: Vec<PathBuf> = Vec::new();
        let mut theirs: Vec<PathBuf> = Vec::new();
        for (index, workspace) in self.workspaces.iter().enumerate() {
            match self.listed[index].has(side) {
                true => open.push(workspace.root().into()),
                false => theirs.push(workspace.root().into()),
            }
        }
        let mut paths = open.clone();
        paths.extend(theirs.iter().cloned());
        paths.extend(
            self.recents
                .iter()
                .filter(|path| !open.contains(path) && !theirs.contains(path))
                .cloned(),
        );
        // Which modes have each open, said before the matcher is borrowed.
        let elsewhere: Vec<String> = paths
            .iter()
            .map(|path| {
                let modes: Vec<&str> = self
                    .index_of(path)
                    .map(|index| self.listed[index].modes().into_iter().map(ModeId::label).collect())
                    .unwrap_or_default();
                format!("open in {}", modes.join(", "))
            })
            .collect();

        let names = folder::disambiguate(&paths);
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        self.matcher
            .rank(query, &refs, folder::LIMIT)
            .into_iter()
            .map(|found| {
                let path = &paths[found.index];
                PickerItem {
                    label: names[found.index].clone(),
                    detail: if open.contains(path) {
                        format!("open · {}", path.display())
                    } else if theirs.contains(path) {
                        format!("{} · {}", elsewhere[found.index], path.display())
                    } else {
                        path.display().to_string()
                    },
                    positions: found.positions,
                    path: Some(path.display().to_string()),
                    line: None,
                    column: None,
                    kind: None,
                }
            })
            .collect()
    }

    fn picker_event(&self) -> Event {
        let Some(picker) = &self.picker else {
            return Event::PickerClosed;
        };
        Event::Picker {
            kind: picker.kind(),
            title: picker.kind().title().to_string(),
            query: picker.query().to_string(),
            items: picker.items().to_vec(),
            selected: picker.selected(),
            total: picker.items().len(),
            elapsed_ms: picker.elapsed_ms(),
            // Nothing is walked in the background here: one directory read
            // answers the whole query, so the list is never "not yet".
            scanning: false,
            // Nor is anything here walked with a limit: the rows are projects,
            // or the folders in the one directory being typed.
            cut_short: false,
        }
    }

    /* ── events ───────────────────────────────────────────────────── */

    /// The switcher: what the mode on screen has open.
    fn workspaces_event(&self) -> Event {
        let listed = self.listed_on(self.mode());
        let paths: Vec<PathBuf> = listed
            .iter()
            .map(|&index| self.workspaces[index].root().into())
            .collect();
        let names = folder::disambiguate(&paths);
        Event::Workspaces {
            items: listed
                .iter()
                .zip(names)
                .map(|(&index, name)| WorkspaceTab {
                    name,
                    path: self.workspaces[index].root().display().to_string(),
                    modified: self.workspaces[index].is_modified(),
                })
                .collect(),
            active: listed
                .iter()
                .position(|&index| index == self.active)
                .unwrap_or(0),
        }
    }

    /* ── remembering ──────────────────────────────────────────────── */

    /// Move a project to the front of the recents list.
    fn remember(&mut self, root: &Path) {
        self.recents.retain(|path| path != root);
        self.recents.insert(0, root.to_path_buf());
        self.recents.truncate(RECENTS);
    }

    /// Write the open set out. Best effort by design: a session that cannot be
    /// saved is a session you have to reopen by hand, not a reason to fail the
    /// command that triggered it.
    pub fn save(&self) {
        let Some(file) = &self.store else { return };
        let root = |index: usize| self.workspaces[index].root().display().to_string();
        // `active` is what Codex shows, because that is where the window
        // opens — and what an older build reads as the project on screen.
        let shown_by = |mode: ModeId| match self.mode() == mode {
            true => Some(self.active),
            false => self.shown[mode.index()]
                .as_deref()
                .and_then(|r| self.index_of(r))
                .filter(|&i| self.listed[i].has(mode)),
        };
        let code = shown_by(ModeId::Codex);
        let state = Stored {
            version: VERSION,
            open: self
                .workspaces
                .iter()
                .map(|w| w.root().display().to_string())
                .collect(),
            active: code.unwrap_or(self.active),
            sides: None,
            modes: Some(
                ModeId::ALL
                    .into_iter()
                    .filter(|&mode| !self.listed_on(mode).is_empty())
                    .map(|mode| StoredMode {
                        mode: mode.slug().to_string(),
                        listed: self.listed_on(mode).into_iter().map(root).collect(),
                        shown: shown_by(mode).map(root),
                    })
                    .collect(),
            ),
            recents: self
                .recents
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            layouts: self
                .workspaces
                .iter()
                .map(|w| StoredLayout {
                    root: w.root().display().to_string(),
                    layout: w.layout(),
                })
                .collect(),
        };
        let Ok(json) = serde_json::to_string_pretty(&state) else {
            return;
        };
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Through a temporary file: a half-written session file is worse than
        // none, because it is the thing read at startup.
        let temp = file.with_extension("json.tmp");
        if std::fs::write(&temp, json).is_ok() {
            let _ = std::fs::rename(&temp, file);
        }
    }
}

/// The session as it is stored between runs.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    version: u32,
    /// What each open workspace had on screen, by root.
    ///
    /// Added without bumping `version`, and keyed by root rather than
    /// positional, so a session file from before this existed still restores
    /// its projects — it simply has no layouts, and every workspace opens the
    /// way it used to. Unknown fields are ignored on the way in, so an older
    /// build reading a newer file loses the layouts and nothing else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layouts: Vec<StoredLayout>,
    open: Vec<String>,
    /// The project the code side shows, in `open`.
    active: usize,
    recents: Vec<String>,
    /// Which side lists each project, and the one Data shows: a file from
    /// before every mode kept its own. Read, never written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sides: Option<StoredSides>,
    /// Which projects each mode lists, and the one it shows. Absent in an
    /// older file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modes: Option<Vec<StoredMode>>,
}

/// One mode's projects, by root. See `Listed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMode {
    mode: String,
    listed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shown: Option<String>,
}

/// The two sides' lists, by root, as a session from before every mode kept
/// its own wrote them.
#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredSides {
    code: Vec<String>,
    data: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shown: Option<String>,
}

/// Where the session lives.
fn state_file() -> Option<PathBuf> {
    Some(folder::config()?.join("session.json"))
}

fn read_state(file: &Path) -> Option<Stored> {
    let text = std::fs::read_to_string(file).ok()?;
    let stored: Stored = serde_json::from_str(&text).ok()?;
    // A file from a future version is ignored rather than guessed at.
    (stored.version == VERSION).then_some(stored)
}

/// One workspace's layout, tied to the folder it belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredLayout {
    root: String,
    #[serde(flatten)]
    layout: crate::workspace::Layout,
}
