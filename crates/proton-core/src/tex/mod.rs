//! The LaTeX mode's core: the machine's TeX, as this workspace sees it, and
//! the project's builds.
//!
//! TeX is the machine's — TinyTeX, latexmk and texlab in `~/.proton/tex`,
//! made once with the person's say-so twice — and each workspace draws it
//! when it changes. A project is typeset each time one of its files is
//! saved: its main file found, latexmk run in the sandbox with the build's
//! folder outside the project, and what came of it — the PDF, copied where
//! the window may read it, and the log, read — said when it is done. A
//! save while a build runs stops it and starts again.
//!
//! ```text
//! ~/.proton/tex/builds/<project>-<hash>/<main>/
//!   out/        what TeX wrote
//!   view/       the PDF as last made whole, which the window reads
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use proton_lab::tex::build::{Build, BuildSpec, Engine};
use proton_lab::tex::{Bundle, TexEnv, TexState};
use proton_lab::RunState;
use proton_tex::outline::Outline;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;

/// How long a build may take.
pub const TIMEOUT: Duration = Duration::from_secs(180);

/// The files whose saving typesets the project again.
pub const SOURCES: &[&str] = &["tex", "bib", "sty", "cls", "bst", "bbx", "cbx", "ltx", "def", "cfg"];

/// Whether saving `path` is saving some of a LaTeX project.
pub fn is_source(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| SOURCES.contains(&e))
}

/// Where a build stands, for the window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TexBuild {
    /// `running`, `done` (a PDF was made, perhaps with errors), `failed`
    /// (none was), `stopped`, `timedOut`, or `noMain` when no file of the
    /// project begins a document.
    pub state: String,
    /// The main file, as the project names it.
    pub main: Option<String>,
    pub engine: String,
    pub seconds: f32,
    /// The PDF the window may read, once one is made.
    pub pdf: Option<String>,
    /// Goes up with each PDF made, so the window reads it again.
    #[ts(type = "number")]
    pub version: u64,
    pub errors: u32,
    pub warnings: u32,
    pub boxes: u32,
    pub pages: Option<u32>,
    /// What went wrong when there is no log to say: latexmk's last words.
    pub said: Option<String>,
}

/// The machine's TeX, in `~/.proton/tex`, made the first time it is asked for.
static TEX: OnceLock<Arc<Mutex<TexEnv>>> = OnceLock::new();

fn shared_tex() -> Arc<Mutex<TexEnv>> {
    TEX.get_or_init(|| Arc::new(Mutex::new(TexEnv::at_home()))).clone()
}

/// Proton is closing: TeX, if it is being made, stops; what the making left
/// is taken away the next time Proton looks.
pub fn quit_tex() {
    if let Some(tex) = TEX.get() {
        tex.lock().unwrap_or_else(|e| e.into_inner()).quit();
    }
}

/// A build under way.
struct Building {
    build: Build,
    spec: BuildSpec,
    root: PathBuf,
    engine: Engine,
}

/// One workspace's LaTeX: the machine's TeX, what this window last drew of
/// it, and the project's builds.
#[derive(Default)]
pub struct Tex {
    /// Given for a test; the machine's, otherwise, once first asked for.
    env: Option<Arc<Mutex<TexEnv>>>,
    /// The version of TeX's state last drawn.
    seen: Option<u64>,
    building: Option<Building>,
    /// What the last build came to.
    last: Option<TexBuild>,
    /// Goes up with each PDF made.
    version: u64,
    /// Whether this workspace has typeset anything: a save of a LaTeX file
    /// typesets it again even outside the LaTeX mode, once it has.
    used: bool,
    /// The version of the packages' state last drawn.
    packages_seen: Option<u64>,
    /// What was last typeset — the project, the file being edited, the
    /// engine asked for — to typeset again once a package it lacked is in.
    again: Option<(PathBuf, Option<PathBuf>, Option<String>)>,
    /// SyncTeX's map of the last PDF made, and the project as TeX named it.
    synctex: Option<(proton_tex::synctex::Map, PathBuf)>,
    /// The outline last drawn, and the main file it is of.
    outline: Option<(Option<String>, Outline)>,
    /// What the last build's log said, for the agent.
    log: Option<proton_tex::log::Log>,
}

impl std::fmt::Debug for Tex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tex").field("seen", &self.seen).finish()
    }
}

impl Tex {
    /// With this TeX rather than the machine's: for a test.
    pub fn with_env(env: TexEnv) -> Tex {
        Tex { env: Some(Arc::new(Mutex::new(env))), ..Tex::default() }
    }

    /// With a TeX another workspace has too.
    pub fn sharing(env: Arc<Mutex<TexEnv>>) -> Tex {
        Tex { env: Some(env), ..Tex::default() }
    }

    fn env(&mut self) -> Arc<Mutex<TexEnv>> {
        self.env.get_or_insert_with(shared_tex).clone()
    }

    /// TeX, looked at — its marker read, nothing started — when the mode is
    /// entered; drawn when it has changed since this window last drew it,
    /// or `always`.
    pub fn env_event(&mut self, always: bool) -> Vec<Event> {
        let env = self.env();
        let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
        let stale = always && matches!(env.state(), TexState::Unknown | TexState::Missing { .. });
        if stale && !env.busy() {
            env.check();
        }
        if !always && self.seen == Some(env.version()) {
            return Vec::new();
        }
        self.seen = Some(env.version());
        vec![Event::TexEnv { env: env.state().clone() }]
    }

    /// Do something to TeX, and draw it.
    pub fn env_do(&mut self, act: impl FnOnce(&mut TexEnv) -> Result<(), String>) -> Vec<Event> {
        let done = {
            let env = self.env();
            let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
            act(&mut env)
        };
        let mut events = self.env_event(true);
        if let Err(why) = done {
            events.push(notice(why, NoticeLevel::Warn));
        }
        events
    }

    /// The first say-so: plan the bundle asked for.
    pub fn plan(&mut self, bundle: Bundle) -> Vec<Event> {
        self.env_do(|env| env.plan(bundle))
    }

    /// What has happened since the last beat: TeX being planned, made or
    /// taken away, drawn when it changed; and a build that ended, said —
    /// with the files the window may now read.
    pub fn poll(&mut self) -> (Vec<Event>, Vec<PathBuf>) {
        let mut events = Vec::new();
        if let Some(env) = self.env.clone() {
            {
                let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
                if env.working() {
                    env.poll();
                }
            }
            events.extend(self.env_event(false));
            events.extend(self.packages_event(false));
        }
        let (built, readable) = self.poll_build();
        events.extend(built);
        (events, readable)
    }

    /// Finding or installing what a document lacks, drawn when it changed,
    /// or `always`. A package installed typesets the project again.
    pub fn packages_event(&mut self, always: bool) -> Vec<Event> {
        let env = self.env();
        let (packages, version) = {
            let env = env.lock().unwrap_or_else(|e| e.into_inner());
            (env.packages().clone(), env.packages_version())
        };
        if !always && self.packages_seen == Some(version) {
            return Vec::new();
        }
        let first = self.packages_seen.is_none();
        self.packages_seen = Some(version);
        let mut events = vec![Event::TexPackages { packages: packages.clone() }];
        if !first && matches!(packages, proton_lab::tex::tlmgr::Packages::Installed { .. }) {
            if let Some((root, active, engine)) = self.again.clone() {
                events.extend(self.compile(&root, active.as_deref(), engine.as_deref()));
            }
        }
        events
    }

    /// Do something to packages, and draw it.
    pub fn packages_do(&mut self, act: impl FnOnce(&mut TexEnv) -> Result<(), String>) -> Vec<Event> {
        let done = {
            let env = self.env();
            let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
            act(&mut env)
        };
        let mut events = self.packages_event(true);
        if let Err(why) = done {
            events.push(notice(why, NoticeLevel::Warn));
        }
        events
    }

    /// Proton's texlab, when TeX is installed.
    pub fn texlab(&mut self) -> Option<PathBuf> {
        let env = self.env();
        let env = env.lock().unwrap_or_else(|e| e.into_inner());
        env.marker().map(|m| m.texlab_bin).filter(|p| p.is_file())
    }

    /// Whether this workspace has typeset anything yet.
    pub fn used(&self) -> bool {
        self.used
    }

    /// What the last build came to, for a window that has just arrived.
    pub fn last_event(&self) -> Option<Event> {
        self.last.clone().map(|build| Event::TexBuild { build })
    }

    /// Typeset the project at `root`, from the file being edited, `active`:
    /// its main file found, a build of it started — one running stopped
    /// first — and said.
    pub fn compile(&mut self, root: &Path, active: Option<&Path>, engine_setting: Option<&str>) -> Vec<Event> {
        let env = self.env();
        let (marker, sandbox, tex_home, person) = {
            let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(env.state(), TexState::Unknown) {
                env.check();
            }
            (env.marker(), env.sandbox(), env.home().to_path_buf(), env.person().to_path_buf())
        };
        let Some(marker) = marker else {
            let mut events = vec![notice("Install TeX to typeset: it is in the LaTeX pane.".into(), NoticeLevel::Warn)];
            events.extend(self.env_event(true));
            return events;
        };
        self.again = Some((root.to_path_buf(), active.map(Path::to_path_buf), engine_setting.map(str::to_string)));
        let pinned = pinned_main(root);
        let Some(main) = proton_tex::project::find_main(root, active, pinned.as_deref()) else {
            let build = TexBuild { state: "noMain".into(), main: None, engine: String::new(), seconds: 0.0, pdf: None, version: self.version, errors: 0, warnings: 0, boxes: 0, pages: None, said: None };
            self.last = Some(build.clone());
            return vec![Event::TexBuild { build }];
        };
        let text = std::fs::read_to_string(&main).unwrap_or_default();
        let engine = Engine::from_name(&proton_tex::project::engine(&text, engine_setting)).unwrap_or_default();
        // A save while a build runs: that one stops, with everything it started.
        self.building = None;
        let spec = BuildSpec {
            root: root.to_path_buf(),
            main: main.clone(),
            dir: build_dir(&tex_home, root, &main),
            bin: marker.bin.clone(),
            tex_home,
            home: person,
            engine,
            timeout: TIMEOUT,
            sandbox,
            shell_escape: false,
            reads: Vec::new(),
        };
        self.used = true;
        match Build::start(&spec) {
            Ok(build) => {
                self.building = Some(Building { build, spec, root: root.to_path_buf(), engine });
                let state = TexBuild {
                    state: "running".into(),
                    main: Some(relative(root, &main)),
                    engine: engine.name().into(),
                    seconds: 0.0,
                    pdf: self.last.as_ref().and_then(|l| l.pdf.clone()),
                    version: self.version,
                    errors: 0,
                    warnings: 0,
                    boxes: 0,
                    pages: None,
                    said: None,
                };
                vec![Event::TexBuild { build: state }]
            }
            Err(why) => vec![notice(format!("TeX could not be started: {why}"), NoticeLevel::Error)],
        }
    }

    /// The document's outline and word count, read from its files as saved:
    /// drawn when it has changed since it was last drawn, or `always`.
    pub fn outline_event(&mut self, root: &Path, active: Option<&Path>, always: bool) -> Vec<Event> {
        let pinned = pinned_main(root);
        let main = proton_tex::project::find_main(root, active, pinned.as_deref());
        let outline = main.as_deref().map(|m| proton_tex::outline::outline(root, m)).unwrap_or_default();
        let now = (main.as_deref().map(|m| relative(root, m)), outline);
        if !always && self.outline.as_ref() == Some(&now) {
            return Vec::new();
        }
        self.outline = Some(now.clone());
        vec![Event::TexOutline { main: now.0, outline: now.1 }]
    }

    /// A project just made from a template, before its first build: what
    /// its main file asks TeX for and TeX lacks is offered, as a build's
    /// missing files are — the build waiting until it is installed — and
    /// with nothing lacking it is typeset at once. Without TeX, the pane
    /// says to install it.
    pub fn begin(&mut self, root: &Path, main: &Path, engine_setting: Option<&str>) -> Vec<Event> {
        let env = self.env();
        let lacking = {
            let mut env = env.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(env.state(), TexState::Unknown) {
                env.check();
            }
            if env.marker().is_none() {
                None
            } else {
                let wants = proton_tex::project::wants(&std::fs::read_to_string(main).unwrap_or_default());
                Some(env.lacking(&wants).unwrap_or_default())
            }
        };
        match lacking {
            None => self.env_event(true),
            Some(missing) if !missing.is_empty() => {
                self.again = Some((root.to_path_buf(), Some(main.to_path_buf()), engine_setting.map(str::to_string)));
                let log = proton_tex::log::Log { missing, ..Default::default() };
                vec![Event::TexLog { log, root: root.display().to_string() }]
            }
            Some(_) => self.compile(root, Some(main), engine_setting),
        }
    }

    /// The last build, as the agent reads it: what was typeset and how, then
    /// its log — each error with the source round it, read from the editor's
    /// buffers by `source` so an unsaved fix is what it sees.
    pub fn describe(&self, source: impl Fn(&str) -> Option<String>) -> String {
        let Some(last) = &self.last else {
            return "Nothing has been typeset in this project yet.".into();
        };
        let main = last.main.as_deref().unwrap_or("the document");
        let header = match last.state.as_str() {
            "running" => format!("`{main}` is being typeset with {} now; this is the build before it.", last.engine),
            "noMain" => return "No file in this project begins a document with \\documentclass, so nothing was typeset.".into(),
            "stopped" => format!("Typesetting `{main}` was stopped."),
            "timedOut" => format!("Typesetting `{main}` took too long and was stopped."),
            state => {
                let pages = last.pages.map(|n| format!(", {n} page{}", if n == 1 { "" } else { "s" })).unwrap_or_default();
                let made = if state == "done" { "made a PDF" } else { "made no PDF" };
                format!(
                    "`{main}`, typeset with {} in {:.1} s, {made}: {} error{}, {} warning{}, {} bad box{}{pages}.",
                    last.engine,
                    last.seconds,
                    last.errors,
                    if last.errors == 1 { "" } else { "s" },
                    last.warnings,
                    if last.warnings == 1 { "" } else { "s" },
                    last.boxes,
                    if last.boxes == 1 { "" } else { "es" },
                )
            }
        };
        match &self.log {
            Some(log) => proton_tex::log::describe(log, &header, source),
            None => header,
        }
    }

    /// Copy the last PDF made beside its main file, in the project — the
    /// one write a build makes there, and only when asked.
    pub fn save_pdf(&mut self, root: &Path) -> Vec<Event> {
        let Some(last) = &self.last else { return vec![notice("Nothing has been typeset yet.".into(), NoticeLevel::Warn)] };
        let (Some(pdf), Some(main)) = (&last.pdf, &last.main) else { return vec![notice("Nothing has been typeset yet.".into(), NoticeLevel::Warn)] };
        let target = root.join(main).with_extension("pdf");
        match std::fs::copy(pdf, &target) {
            Ok(_) => vec![notice(format!("saved {} beside {main}", relative(root, &target)), NoticeLevel::Info)],
            Err(e) => vec![notice(format!("{}: {e}", target.display()), NoticeLevel::Error)],
        }
    }

    /// Where `line` (one-based) of `file`, a path in the project, is in the
    /// PDF last made.
    pub fn sync_forward(&self, root: &Path, file: &Path, line: u32) -> Vec<Event> {
        let Some((map, _)) = &self.synctex else { return vec![notice("Typeset the document first.".into(), NoticeLevel::Info)] };
        let rel = relative(root, file);
        match map.forward(&rel, line) {
            Some(found) => vec![Event::TexSync { found }],
            None => vec![notice(format!("{rel}:{line} is not in the PDF."), NoticeLevel::Info)],
        }
    }

    /// The file and line (one-based) that put something at a point on a
    /// page of the PDF last made, as the project names the file.
    pub fn sync_inverse(&self, page: u32, x: f64, y: f64) -> Option<(String, u32)> {
        let (map, real_root) = self.synctex.as_ref()?;
        let source = map.inverse(page, x, y)?;
        let file = PathBuf::from(normalise(&source.file));
        let file = file.strip_prefix(real_root).map(Path::to_path_buf).unwrap_or(file);
        Some((file.display().to_string(), source.line))
    }

    /// Stop the build under way, if there is one.
    pub fn stop(&mut self) -> Vec<Event> {
        match &mut self.building {
            Some(building) => {
                building.build.stop();
                Vec::new()
            }
            None => Vec::new(),
        }
    }

    /// A build that ended: its log read, its PDF copied where the window
    /// reads it, and both said.
    fn poll_build(&mut self) -> (Vec<Event>, Vec<PathBuf>) {
        let Some(building) = &mut self.building else { return (Vec::new(), Vec::new()) };
        if !building.build.poll() {
            return (Vec::new(), Vec::new());
        }
        let Building { build, spec, root, engine } = self.building.take().expect("a build");
        // As TeX names them: its log has the paths it was given, resolved.
        let real_root = proton_lab::sandbox::real(&root);
        let main_dir = proton_lab::sandbox::real(spec.main.parent().unwrap_or(&root)).display().to_string();
        let root_text = real_root.display().to_string();
        let mut log = proton_tex::log::read(&std::fs::read_to_string(spec.log()).unwrap_or_default(), &main_dir, &root_text);
        let blg = spec.log().with_extension("blg");
        if let Ok(text) = std::fs::read_to_string(&blg) {
            let bib = proton_tex::log::read_bib(&text, &main_dir, &root_text);
            log.entries.extend(bib.entries);
            for missing in bib.missing {
                if !log.missing.contains(&missing) {
                    log.missing.push(missing);
                }
            }
        }
        let mut readable = vec![spec.log()];
        let made = spec.pdf();
        // SyncTeX's map goes with the PDF it describes.
        if made.is_file() {
            if let Ok(map) = std::fs::read(spec.synctex()).map_err(|e| e.to_string()).and_then(|b| proton_tex::synctex::Map::read(&b)) {
                self.synctex = Some((map, real_root.clone()));
            }
        }
        let pdf = match build.state() {
            RunState::Done | RunState::Failed if made.is_file() => match show(&spec, &made) {
                Ok(view) => {
                    self.version += 1;
                    readable.push(view.clone());
                    Some(view.display().to_string())
                }
                Err(_) => None,
            },
            _ => None,
        };
        let state = match (build.state(), &pdf) {
            (RunState::Done | RunState::Failed, Some(_)) => "done",
            (RunState::Stopped, _) => "stopped",
            (RunState::TimedOut, _) => "timedOut",
            _ => "failed",
        };
        let said = (pdf.is_none() && log.entries.is_empty()).then(|| build.said()).filter(|s| !s.is_empty());
        let result = TexBuild {
            state: state.into(),
            main: Some(relative(&root, &spec.main)),
            engine: engine.name().into(),
            seconds: build.seconds() as f32,
            pdf: pdf.or_else(|| self.last.as_ref().and_then(|l| l.pdf.clone())),
            version: self.version,
            errors: log.errors() as u32,
            warnings: log.warnings() as u32,
            boxes: log.boxes() as u32,
            pages: log.pages,
            said,
        };
        self.last = Some(result.clone());
        // A build stopped for another says nothing: the next one will.
        if state == "stopped" {
            return (Vec::new(), Vec::new());
        }
        self.log = Some(log.clone());
        (vec![Event::TexBuild { build: result }, Event::TexLog { log, root: root.display().to_string() }], readable)
    }
}

/// A path with its `.` and `..` worked out.
fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("{}{}", if path.starts_with('/') { "/" } else { "" }, parts.join("/"))
}

/// The main file pinned for the project, in `.proton/tex.json`.
fn pinned_main(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join(".proton").join("tex.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value["main"].as_str().map(PathBuf::from)
}

/// A path as the project names it.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

/// Where a project's main file is built: under TeX's folder, never the
/// project's — `<project>-<hash>/<main, as a name>`.
pub fn build_dir(tex_home: &Path, root: &Path, main: &Path) -> PathBuf {
    let name: String = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "project".into());
    let name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let hash = proton_lab::run::digest(root.display().to_string().as_bytes());
    let main: String = relative(root, main).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    tex_home.join("builds").join(format!("{name}-{}", &hash[..10])).join(main)
}

/// The PDF, copied whole to where the window reads it: a read under way
/// keeps the PDF before, and never sees one half written.
fn show(spec: &BuildSpec, made: &Path) -> Result<PathBuf, String> {
    let view = spec.dir.join("view");
    std::fs::create_dir_all(&view).map_err(|e| e.to_string())?;
    let name = made.file_name().unwrap_or_default();
    let target = view.join(name);
    let partial = view.join(format!(".{}.part", name.to_string_lossy()));
    std::fs::copy(made, &partial).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &target).map_err(|e| e.to_string())?;
    Ok(target)
}
