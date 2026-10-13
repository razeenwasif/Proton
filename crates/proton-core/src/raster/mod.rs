//! Canvas mode: the canvas open in this project, its history, and its saves.
//!
//! `proton-raster` owns the pixels and what a change to them means. What lives
//! here is the editor's half: which canvas is open, the undo history the
//! window's edits and the agent's share, which numbered state was last saved,
//! and the canvases a project holds, for the home page shown when none is.
//!
//! The window keeps its own copy of the canvas, in the same engine built as
//! WebAssembly, and paints an edit there the moment it is made. It sends the
//! edit here, numbered; this applies it to the canvas that is saved and
//! answers with a hash of the tiles it touched, which the window checks
//! against its own. A change made here — undo, redo, the agent — is pushed
//! as the tiles it changed, which the window fetches through `read_raster`.
//! `generation` numbers each canvas opened, so an answer about the last one
//! is never taken for this one.
//!
//! ⌘S writes the canvas on a worker: the tiles are shared, not copied, so the
//! snapshot costs nothing, and encoding a large layer's PNG never holds up
//! the window. Quitting saves at once, after any save already under way.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use proton_raster::{file, Canvas, History, LayerInfo, Op, Preset, Tile};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::workspace::notice;

/// A canvas's folder ends so.
pub const EXTENSION: &str = "canvas";

/// Whether a command is Canvas mode's.
pub fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::RasterOpen { .. }
            | Command::RasterNew { .. }
            | Command::RasterClose { .. }
            | Command::RasterEdit { .. }
            | Command::RasterUndo
            | Command::RasterRedo
            | Command::RasterSave
    )
}

/// A canvas found in the project, for the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct RasterFound {
    /// Its folder, as the project names it.
    pub path: String,
    pub name: String,
    pub preset: Preset,
    pub size: [u32; 2],
}

/// The canvas open: everything but its pixels, which the window fetches by
/// the tiles each layer has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct RasterView {
    pub path: String,
    pub name: String,
    pub preset: Preset,
    pub width: u32,
    pub height: u32,
    pub palette: Vec<[u8; 4]>,
    /// Bottom to top.
    pub layers: Vec<LayerInfo>,
    /// Each layer's tiles that hold anything, by index: what to fetch.
    pub tiles: Vec<(u32, Vec<u32>)>,
    pub generation: u32,
    #[ts(type = "number")]
    pub version: u64,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// What undo and redo would do.
    pub undo: Option<String>,
    pub redo: Option<String>,
    pub saving: bool,
}

/// Tiles of one layer, shared with the canvas, for the shell to write out
/// after it has let go of the lock.
pub struct RasterTiles {
    pub generation: u32,
    pub version: u64,
    pub last_seq: u32,
    pub tiles: Vec<(u32, Option<Arc<Tile>>)>,
}

struct Saving {
    state: u64,
    done: Receiver<Result<Vec<String>, String>>,
    /// ⌘S again while this one was writing: write once more after.
    again: bool,
}

struct Open {
    dir: PathBuf,
    shown: String,
    name: String,
    canvas: Canvas,
    history: History,
    /// The state last written.
    saved: u64,
    generation: u32,
    version: u64,
    last_seq: u32,
    saving: Option<Saving>,
}

impl std::fmt::Debug for Open {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Open").field("shown", &self.shown).field("generation", &self.generation).field("version", &self.version).finish()
    }
}

/// The canvas open in a project, and the editor's place in it.
#[derive(Debug, Default)]
pub struct Raster {
    open: Option<Open>,
    /// The last generation given out, across every canvas opened here.
    generations: u32,
}

impl Raster {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Entering Canvas mode: the home page, and the canvas open if one is.
    pub fn enter(&mut self, root: &Path) -> Vec<Event> {
        self.events(root)
    }

    /// Everything the window needs to draw the mode.
    pub fn events(&self, root: &Path) -> Vec<Event> {
        vec![self.home_event(root), self.canvas_event()]
    }

    pub fn apply(&mut self, root: &Path, command: Command) -> Vec<Event> {
        match command {
            Command::RasterNew { name, preset, width, height, folder } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return vec![notice("a new canvas needs a name".into(), NoticeLevel::Warn)];
                }
                if self.dirty() {
                    return vec![notice("save or discard the open canvas's changes before making another".into(), NoticeLevel::Warn)];
                }
                let folder = folder.filter(|f| !f.trim().is_empty()).unwrap_or_else(|| format!("{}.{EXTENSION}", slug(&name)));
                let dir = match inside(root, &folder) {
                    Ok(dir) => dir,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                if dir.exists() {
                    return vec![notice(format!("{folder} is already there"), NoticeLevel::Warn)];
                }
                let made = Canvas::new(width, height, preset).and_then(|canvas| file::save(&dir, &canvas).map(|_| canvas));
                match made {
                    Ok(canvas) => {
                        self.show(root, dir, canvas);
                        let mut events = self.events(root);
                        events.push(notice(format!("made {name} in {folder}"), NoticeLevel::Info));
                        events
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::RasterOpen { path } => {
                let dir = match inside(root, &path) {
                    Ok(dir) => dir,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                if self.open.as_ref().is_some_and(|o| o.dir == dir) {
                    return self.events(root);
                }
                if self.dirty() {
                    return vec![notice("save or discard the open canvas's changes before opening another".into(), NoticeLevel::Warn)];
                }
                match file::load(&dir) {
                    Ok(canvas) => {
                        self.show(root, dir, canvas);
                        self.events(root)
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::RasterClose { discard } => {
                if !discard && self.dirty() {
                    return vec![notice("the canvas has unsaved changes".into(), NoticeLevel::Warn)];
                }
                self.open = None;
                self.events(root)
            }
            Command::RasterEdit { generation, seq, op, label } => self.edit(generation, seq, &op, &label),
            Command::RasterUndo => self.step(true),
            Command::RasterRedo => self.step(false),
            Command::RasterSave => self.save(),
            _ => Vec::new(),
        }
    }

    fn show(&mut self, root: &Path, dir: PathBuf, canvas: Canvas) {
        self.generations += 1;
        let name = dir.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.open = Some(Open {
            shown: dir.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| dir.display().to_string()),
            dir,
            name,
            history: History::new(canvas.preset),
            saved: canvas.state,
            canvas,
            generation: self.generations,
            version: 0,
            last_seq: 0,
            saving: None,
        });
    }

    /// An edit from the window or the agent, applied and answered.
    fn edit(&mut self, generation: u32, seq: u32, op: &Op, label: &str) -> Vec<Event> {
        let Some(open) = &mut self.open else {
            return vec![notice("no canvas is open".into(), NoticeLevel::Warn)];
        };
        if generation != open.generation {
            // Meant for a canvas since closed: answered, so the window lets
            // it go, and not applied.
            return vec![Event::RasterApplied {
                generation,
                seq,
                version: open.version,
                hash: String::new(),
                refused: Some("that canvas is no longer open".into()),
                dirty: open.canvas.state != open.saved,
                can_undo: open.history.can_undo(),
                can_redo: open.history.can_redo(),
            }];
        }
        open.last_seq = seq;
        match open.history.apply(&mut open.canvas, op, label) {
            Ok(applied) => {
                if applied.changed {
                    open.version += 1;
                }
                let hash = format!("{:016x}", open.canvas.hash_touched(&applied.touched));
                let mut events = vec![Event::RasterApplied {
                    generation,
                    seq,
                    version: open.version,
                    hash,
                    refused: None,
                    dirty: open.canvas.state != open.saved,
                    can_undo: open.history.can_undo(),
                    can_redo: open.history.can_redo(),
                }];
                if applied.meta {
                    events.push(self.canvas_event());
                }
                events
            }
            Err(why) => vec![
                Event::RasterApplied {
                    generation,
                    seq,
                    version: open.version,
                    hash: String::new(),
                    refused: Some(why.clone()),
                    dirty: open.canvas.state != open.saved,
                    can_undo: open.history.can_undo(),
                    can_redo: open.history.can_redo(),
                },
                notice(why, NoticeLevel::Warn),
            ],
        }
    }

    /// Undo or redo the newest step: what changed goes to the window.
    fn step(&mut self, back: bool) -> Vec<Event> {
        let Some(open) = &mut self.open else { return Vec::new() };
        let done = match back {
            true => open.history.undo(&mut open.canvas),
            false => open.history.redo(&mut open.canvas),
        };
        let Some((applied, _label)) = done else { return Vec::new() };
        open.version += 1;
        let changed = Event::RasterChanged { generation: open.generation, version: open.version, tiles: by_layer(&applied.touched), meta: applied.meta };
        // The layers first: a layer back from removal has to be in the
        // window's copy before its tiles can be put there.
        vec![self.canvas_event(), changed]
    }

    /// Write the canvas on a worker; what it says arrives through `poll`.
    fn save(&mut self) -> Vec<Event> {
        let Some(open) = &mut self.open else { return Vec::new() };
        if let Some(saving) = &mut open.saving {
            saving.again = true;
            return Vec::new();
        }
        start_save(open);
        vec![self.canvas_event()]
    }

    /// A save that has finished, said.
    pub fn poll(&mut self) -> Vec<Event> {
        let Some(open) = &mut self.open else { return Vec::new() };
        let Some(saving) = &open.saving else { return Vec::new() };
        let Ok(result) = saving.done.try_recv() else { return Vec::new() };
        let (state, again) = (saving.state, saving.again);
        open.saving = None;
        let mut events = Vec::new();
        match result {
            Ok(_) => {
                open.saved = state;
                if again && open.canvas.state != open.saved {
                    start_save(open);
                } else {
                    events.push(notice(format!("saved {}", open.name), NoticeLevel::Info));
                }
            }
            Err(why) => events.push(notice(format!("{} was not saved: {why}", open.name), NoticeLevel::Error)),
        }
        events.push(self.canvas_event());
        events
    }

    fn dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.canvas.state != o.saved)
    }

    /// The canvas, by its folder, when it has changes not saved.
    pub fn unsaved(&self) -> Vec<String> {
        match &self.open {
            Some(open) if open.canvas.state != open.saved => vec![open.shown.clone()],
            _ => Vec::new(),
        }
    }

    /// Save now, for quitting: after any save already writing. What could
    /// not be saved.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let Some(open) = &mut self.open else { return Vec::new() };
        if let Some(saving) = open.saving.take() {
            if let Ok(Ok(_)) = saving.done.recv() {
                open.saved = saving.state;
            }
        }
        if open.canvas.state == open.saved {
            return Vec::new();
        }
        match file::save(&open.dir, &open.canvas) {
            Ok(_) => {
                open.saved = open.canvas.state;
                Vec::new()
            }
            Err(why) => vec![format!("{}: {why}", open.shown)],
        }
    }

    /// Tiles of a layer, for `read_raster`: shared, not copied.
    pub fn tiles(&self, generation: u32, layer: u32, indices: &[u32]) -> Option<RasterTiles> {
        let open = self.open.as_ref().filter(|o| o.generation == generation)?;
        let layer = open.canvas.layer(layer)?;
        let tiles = indices.iter().filter(|&&i| i < layer.grid.cols * layer.grid.rows).map(|&i| (i, layer.grid.tile(i).cloned())).collect();
        Some(RasterTiles { generation, version: open.version, last_seq: open.last_seq, tiles })
    }

    pub fn home_event(&self, root: &Path) -> Event {
        Event::RasterHome { canvases: find(root) }
    }

    pub fn canvas_event(&self) -> Event {
        let Some(open) = &self.open else { return Event::RasterCanvas { canvas: None } };
        let c = &open.canvas;
        let (undo, redo) = open.history.labels();
        Event::RasterCanvas {
            canvas: Some(RasterView {
                path: open.shown.clone(),
                name: open.name.clone(),
                preset: c.preset,
                width: c.width,
                height: c.height,
                palette: c.palette.clone(),
                layers: c.infos(),
                tiles: c
                    .layers
                    .iter()
                    .map(|l| (l.info.id, (0..l.grid.cols * l.grid.rows).filter(|&i| l.grid.tile(i).is_some()).collect()))
                    .collect(),
                generation: open.generation,
                version: open.version,
                dirty: c.state != open.saved,
                can_undo: open.history.can_undo(),
                can_redo: open.history.can_redo(),
                undo: undo.map(str::to_string),
                redo: redo.map(str::to_string),
                saving: open.saving.is_some(),
            }),
        }
    }
}

fn start_save(open: &mut Open) {
    let (tx, rx) = channel();
    let snapshot = open.canvas.clone();
    let dir = open.dir.clone();
    std::thread::spawn(move || {
        let _ = tx.send(file::save(&dir, &snapshot));
    });
    open.saving = Some(Saving { state: open.canvas.state, done: rx, again: false });
}

/// Touched tiles, as (layer, its tiles).
fn by_layer(touched: &[(u32, u32)]) -> Vec<(u32, Vec<u32>)> {
    let mut out: Vec<(u32, Vec<u32>)> = Vec::new();
    for &(layer, index) in touched {
        match out.iter_mut().find(|(l, _)| *l == layer) {
            Some((_, tiles)) => {
                if !tiles.contains(&index) {
                    tiles.push(index);
                }
            }
            None => out.push((layer, vec![index])),
        }
    }
    out
}

/// A folder in the project, held to it.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || !named.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{rel} is not a folder in the project"));
    }
    if named.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
        return Err(format!("{rel} is not a canvas: a canvas's folder ends .{EXTENSION}"));
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

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "canvas".into()
    } else {
        out
    }
}

/// The project's canvases: `*.canvas` folders, a few levels down, not inside
/// what is hidden or built.
fn find(root: &Path) -> Vec<RasterFound> {
    const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor"];
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut looked = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            looked += 1;
            if looked > 5000 {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !path.is_dir() || SKIP.contains(&name.as_str()) {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some(EXTENSION) {
                if let Ok(text) = std::fs::read_to_string(path.join(file::MANIFEST)) {
                    let meta: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                    let preset = serde_json::from_value(meta["preset"].clone()).unwrap_or_default();
                    let size = serde_json::from_value(meta["size"].clone()).unwrap_or([0, 0]);
                    out.push(RasterFound {
                        path: path.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default(),
                        name: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                        preset,
                        size,
                    });
                }
            } else if depth < 3 {
                stack.push((path, depth + 1));
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}
