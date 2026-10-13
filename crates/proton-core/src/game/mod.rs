//! Game mode: the game open in this project, and the editor around it.
//!
//! `proton-game` owns the files and what an edit to them means. What lives
//! here is the editor's half: which game is open, which of its rooms are open
//! as tabs and which is on screen, the order edits were made in — so ⌘Z
//! undoes the last one wherever it was — and the games a project holds, for
//! the home page shown when none is open.
//!
//! A game belongs to its project, as a scene does to Modeling: this is per
//! workspace, and switching projects switches games.

use std::path::{Path, PathBuf};

use proton_game::format::{GameFile, Niche, Prefab, Room, World};
use proton_game::project::{Project, Target};
use proton_game::Op;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::workspace::notice;

mod agent;
pub use agent::{GamePathPart, TOOLS};

/// Whether a command is Game mode's.
pub fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::GameOpen { .. }
            | Command::GameNew { .. }
            | Command::GameClose { .. }
            | Command::GameOpenRoom { .. }
            | Command::GameCloseRoom { .. }
            | Command::GameFocus { .. }
            | Command::GameEdit { .. }
            | Command::GameTrace { .. }
            | Command::GamePlay { .. }
            | Command::GameRecorded { .. }
            | Command::GameUndo
            | Command::GameRedo
            | Command::GameSave
    )
}

/// A game found in the project, for the home page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GameFound {
    /// Its folder, as the project names it: `.` for the project itself.
    pub path: String,
    pub name: String,
    pub niche: Niche,
    pub rooms: usize,
}

/// A template a new game can be made from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GameTemplate {
    pub id: String,
    pub name: String,
}

/// A run played in the window, kept: what the Run tab lists to play back, and
/// what the agent can replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GameRecording {
    pub id: u32,
    pub room: String,
    pub ticks: u32,
    pub hash: String,
    /// Whether the core, running it again, ended where the window did.
    pub same: bool,
    #[serde(skip)]
    #[ts(skip)]
    pub start: (u16, [i32; 2], u32),
    #[serde(skip)]
    #[ts(skip)]
    pub replay: Vec<(u32, u32)>,
}

/// A room as the World tab lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GameRoomRow {
    pub id: String,
    pub area: String,
    pub size: [i32; 2],
    /// Changed and not saved.
    pub dirty: bool,
}

/// The game that is open, as the window draws everything but its rooms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GameProjectView {
    /// Its folder, as the project names it.
    pub path: String,
    pub game: GameFile,
    pub world: World,
    pub prefabs: Vec<Prefab>,
    pub rooms: Vec<GameRoomRow>,
    /// The rooms open as tabs, and the one on screen.
    pub open: Vec<String>,
    pub focus: Option<String>,
    /// Whether the game's own file, the world, or a prefab is unsaved.
    pub game_dirty: bool,
    pub world_dirty: bool,
    /// Whether anything at all is.
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// What is wrong across its files, in words.
    pub problems: Vec<String>,
    /// Runs played, newest first.
    pub recordings: Vec<GameRecording>,
    /// The images and sounds in its folder, for the Assets tab.
    pub assets: Vec<String>,
}

/// The game open in a project, and the editor's place in it.
#[derive(Debug, Default)]
pub struct Game {
    project: Option<Project>,
    /// Its folder, as the project names it.
    shown: String,
    open: Vec<String>,
    focus: Option<String>,
    /// Which documents edits went to, oldest first: ⌘Z undoes the newest,
    /// wherever it was.
    done: Vec<Target>,
    undone: Vec<Target>,
    /// Bumped with every change to the room on screen, so the window can tell
    /// a room it has from a room changed since.
    revision: u64,
    /// Runs played, newest last, and the next one's number.
    recordings: Vec<GameRecording>,
    recorded: u32,
    /// What the agent's tools changed on screen, for the window.
    pending: Vec<Event>,
}

impl Game {
    pub fn is_open(&self) -> bool {
        self.project.is_some()
    }

    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    /// Entering Game mode: a project that is itself a game opens it, once.
    pub fn enter(&mut self, root: &Path) -> Vec<Event> {
        if self.project.is_none() && Project::is_game(root) {
            if let Err(why) = self.open_at(root, root) {
                return vec![self.home_event(root), notice(why, NoticeLevel::Warn)];
            }
        }
        self.events(root)
    }

    /// Everything the window needs to draw the mode.
    pub fn events(&self, root: &Path) -> Vec<Event> {
        vec![self.home_event(root), self.project_event(), self.room_event()]
    }

    pub fn apply(&mut self, root: &Path, command: Command) -> Vec<Event> {
        match command {
            Command::GameOpen { path } => {
                let dir = resolve(root, &path);
                if let Some(project) = &self.project {
                    if project.root() == dir {
                        return self.events(root);
                    }
                    if !project.unsaved().is_empty() {
                        return vec![notice("save or discard the open game's changes before opening another".into(), NoticeLevel::Warn)];
                    }
                }
                match self.open_at(root, &dir) {
                    Ok(()) => self.events(root),
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::GameNew { name, template, folder } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return vec![notice("a new game needs a name".into(), NoticeLevel::Warn)];
                }
                if self.project.as_ref().is_some_and(|p| !p.unsaved().is_empty()) {
                    return vec![notice("save or discard the open game's changes before making another".into(), NoticeLevel::Warn)];
                }
                let folder = folder.filter(|f| !f.trim().is_empty()).unwrap_or_else(|| slug(&name));
                let dir = resolve(root, &folder);
                if !dir.starts_with(root) {
                    return vec![notice(format!("{folder} is outside the project"), NoticeLevel::Warn)];
                }
                let made = proton_game::template::files(&template, &name).and_then(|files| proton_game::template::write(&dir, &files));
                if let Err(why) = made {
                    return vec![notice(why, NoticeLevel::Warn)];
                }
                match self.open_at(root, &dir) {
                    Ok(()) => {
                        let mut events = self.events(root);
                        events.push(notice(format!("made {name} in {folder}"), NoticeLevel::Info));
                        events
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::GameClose { discard } => {
                if let Some(project) = &self.project {
                    if !discard && !project.unsaved().is_empty() {
                        return vec![notice("the game has unsaved changes".into(), NoticeLevel::Warn)];
                    }
                }
                *self = Game::default();
                self.events(root)
            }
            Command::GameOpenRoom { room } | Command::GameFocus { room } => {
                let Some(project) = &self.project else { return Vec::new() };
                if project.room(&room).is_none() {
                    return vec![notice(format!("there is no room {room}"), NoticeLevel::Warn)];
                }
                if !self.open.contains(&room) {
                    self.open.push(room.clone());
                }
                self.focus = Some(room);
                self.revision += 1;
                vec![self.project_event(), self.room_event()]
            }
            Command::GameCloseRoom { room } => {
                let Some(at) = self.open.iter().position(|r| *r == room) else { return Vec::new() };
                self.open.remove(at);
                if self.focus.as_deref() == Some(room.as_str()) {
                    self.focus = self.open.get(at.min(self.open.len().saturating_sub(1))).cloned();
                    self.revision += 1;
                }
                vec![self.project_event(), self.room_event()]
            }
            Command::GameEdit { target, ops, label: _ } => {
                let Some(target) = Target::parse(&target) else {
                    return vec![notice(format!("{target} is not one of the game's documents"), NoticeLevel::Warn)];
                };
                self.edit(&target, &ops)
            }
            Command::GameTrace { room, item, threshold, tolerance } => match self.traced(&room, &item, threshold, tolerance) {
                Ok(ops) if ops.is_empty() => vec![notice(format!("{item} has nothing opaque enough to trace"), NoticeLevel::Warn)],
                Ok(ops) => {
                    let n = ops.len();
                    let mut events = self.edit(&Target::Room(room), &ops);
                    events.push(notice(format!("traced {n} shape{} from {item}", if n == 1 { "" } else { "s" }), NoticeLevel::Info));
                    events
                }
                Err(why) => vec![notice(why, NoticeLevel::Warn)],
            },
            Command::GamePlay { room, at, abilities, recording } => match self.play(room, at, abilities, recording) {
                Ok(event) => vec![event],
                Err(why) => vec![notice(why, NoticeLevel::Warn)],
            },
            Command::GameRecorded { room, at, abilities, replay, hash } => self.recorded(room, at, abilities, replay, hash),
            Command::GameUndo => self.step(true),
            Command::GameRedo => self.step(false),
            Command::GameSave => {
                let Some(project) = &mut self.project else { return Vec::new() };
                match project.save() {
                    Ok(wrote) if wrote.is_empty() => Vec::new(),
                    Ok(wrote) => {
                        let n = wrote.len();
                        vec![self.project_event(), self.room_event(), notice(format!("saved {n} file{}", if n == 1 { "" } else { "s" }), NoticeLevel::Info)]
                    }
                    Err(why) => vec![self.project_event(), self.room_event(), notice(why, NoticeLevel::Error)],
                }
            }
            _ => Vec::new(),
        }
    }

    /// Apply a batch of ops to one of the game's documents.
    pub fn edit(&mut self, target: &Target, ops: &[Op]) -> Vec<Event> {
        let Some(project) = &mut self.project else {
            return vec![notice("no game is open".into(), NoticeLevel::Warn)];
        };
        match project.apply(target, ops) {
            Ok(false) => Vec::new(),
            Ok(true) => {
                self.done.push(target.clone());
                self.undone.clear();
                self.changed(target)
            }
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// The shapes tracing an image on the play plane makes, as ops that add
    /// them to its room: the outline of where it is opaque, in room pixels —
    /// placed, scaled, flipped and turned as the image is.
    fn traced(&self, room: &str, item: &str, threshold: Option<u8>, tolerance: Option<f64>) -> Result<Vec<Op>, String> {
        use proton_game::trace::{outlines, Mask, Tracing};
        let project = self.project.as_ref().ok_or("no game is open")?;
        let doc = project.room(room).ok_or_else(|| format!("there is no room {room}"))?;
        let room_now = doc.get();
        let (plane, found) = room_now
            .planes
            .iter()
            .find_map(|p| p.items.iter().find(|i| i.id == item).map(|i| (p, i)))
            .ok_or_else(|| format!("there is no image {item} in room {room}"))?;
        if !plane.gameplay {
            return Err(format!("{item} is on {}, and collision belongs to the plane the game is played on", plane.name));
        }
        let path = proton_game::path::inside(project.root(), &found.image)?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", found.image))?;
        let mask = Mask::of_png(&bytes).map_err(|e| format!("{}: {e}", found.image))?;
        let how = Tracing {
            threshold: threshold.unwrap_or(128),
            tolerance: tolerance.unwrap_or(2.0).max(0.25),
            ..Tracing::default()
        };
        let (w, h) = (mask.width as f64 * found.scale, mask.height as f64 * found.scale);
        let (cx, cy) = (found.at[0] as f64 + w / 2.0, found.at[1] as f64 + h / 2.0);
        let (sin, cos) = found.rot.to_radians().sin_cos();
        let taken: std::collections::HashSet<&str> = room_now.collision.iter().map(|s| s.id.as_str()).collect();
        let mut ops = Vec::new();
        let mut n = 0;
        for ring in outlines(&mask, &how) {
            let mut points: Vec<[i32; 2]> = Vec::new();
            for p in ring {
                let x = if found.flip { mask.width as f64 - p[0] } else { p[0] } * found.scale - w / 2.0;
                let y = p[1] * found.scale - h / 2.0;
                let at = [(cx + x * cos - y * sin).round() as i32, (cy + x * sin + y * cos).round() as i32];
                if points.last() != Some(&at) {
                    points.push(at);
                }
            }
            if points.len() < 3 {
                continue;
            }
            let id = loop {
                n += 1;
                let id = format!("{item}_outline_{n}");
                if !taken.contains(id.as_str()) {
                    break id;
                }
            };
            ops.push(Op::Insert {
                path: vec!["collision".into()],
                value: serde_json::json!({ "id": id, "kind": "solid", "closed": true, "points": points }),
                at: None,
            });
        }
        Ok(ops)
    }

    /// A run to play, cooked from the game as it is now.
    fn play(&self, room: Option<String>, at: Option<[i32; 2]>, abilities: Vec<String>, recording: Option<u32>) -> Result<Event, String> {
        let project = self.project.as_ref().ok_or("no game is open")?;
        let world = proton_game::sim::World::cook(project)?;
        let (start, replay) = match recording.and_then(|id| self.recordings.iter().find(|r| r.id == id)) {
            Some(r) => ((r.start.0, r.start.1, r.start.2), Some(r.replay.clone())),
            None => {
                let (index, place, bits) = agent::start_of(&world, room.as_deref(), at, &abilities)?;
                ((index, place, bits), None)
            }
        };
        let json = serde_json::to_string(&world).map_err(|e| e.to_string())?;
        let game = project.game.get();
        let mut bindings = proton_game::format::default_input();
        bindings.extend(game.input.clone());
        let keys = proton_game::sim::input::ACTIONS
            .iter()
            .flat_map(|(action, bit)| bindings.get(*action).cloned().unwrap_or_default().into_iter().map(move |key| (key, *bit)))
            .collect();
        Ok(Event::GamePlay {
            world: json,
            start: [start.0 as i64, start.1[0] as i64, start.1[1] as i64, start.2 as i64],
            rooms: project.rooms.iter().map(|d| d.get().clone()).collect(),
            keys,
            replay,
        })
    }

    /// A run played in the window, kept — and run again here, to see that the
    /// core's simulation ends where the window's did.
    fn recorded(&mut self, room: u32, at: [i32; 2], abilities: u32, replay: Vec<(u32, u32)>, hash: String) -> Vec<Event> {
        let Some(project) = &self.project else { return Vec::new() };
        let Ok(world) = proton_game::sim::World::cook(project) else { return Vec::new() };
        let Some(name) = world.rooms.get(room as usize).map(|r| r.id.clone()) else { return Vec::new() };
        let mut sim = proton_game::sim::Sim::at(world, room as u16, at, abilities);
        proton_game::sim::replay(&mut sim, &replay, |_| {});
        let same = format!("{:016x}", sim.hash()) == hash;
        self.recorded += 1;
        let ticks = replay.iter().map(|(_, n)| n).sum();
        self.recordings.push(GameRecording { id: self.recorded, room: name, ticks, hash, same, start: (room as u16, at, abilities), replay });
        if self.recordings.len() > 10 {
            self.recordings.remove(0);
        }
        let mut events = vec![self.project_event()];
        if !same {
            events.push(notice("the run played here and the run in the window ended differently — the simulation is not the same in both".into(), NoticeLevel::Error));
        }
        events
    }

    /// Undo (or redo) the newest edit, in whichever document it was made.
    fn step(&mut self, back: bool) -> Vec<Event> {
        let Some(project) = &mut self.project else { return Vec::new() };
        let (from, to) = match back {
            true => (&mut self.done, &mut self.undone),
            false => (&mut self.undone, &mut self.done),
        };
        let Some(target) = from.pop() else { return Vec::new() };
        let moved = match back {
            true => project.undo(&target),
            false => project.redo(&target),
        };
        match moved {
            Ok(true) => {
                to.push(target.clone());
                self.changed(&target)
            }
            Ok(false) => Vec::new(),
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// What the window is sent after `target` changed.
    fn changed(&mut self, target: &Target) -> Vec<Event> {
        match target {
            Target::Room(id) if self.focus.as_deref() == Some(id.as_str()) => {
                self.revision += 1;
                vec![self.project_event(), self.room_event()]
            }
            _ => vec![self.project_event()],
        }
    }

    fn open_at(&mut self, root: &Path, dir: &Path) -> Result<(), String> {
        let project = Project::open(dir)?;
        let first = project.game.get().start.room.clone();
        let first = match project.room(&first) {
            Some(_) => Some(first),
            None => project.rooms.first().map(|r| r.get().id.clone()),
        };
        *self = Game {
            shown: shown(root, dir),
            project: Some(project),
            open: first.iter().cloned().collect(),
            focus: first,
            done: Vec::new(),
            undone: Vec::new(),
            revision: self.revision + 1,
            recordings: Vec::new(),
            recorded: 0,
            pending: Vec::new(),
        };
        Ok(())
    }

    /// Where a file of the open game is, for the window to draw: only inside
    /// its folder, only an image or the sidecar that cuts one up, and only
    /// one that is there.
    pub fn file(&self, rel: &str) -> Option<PathBuf> {
        const DRAWN: &[&str] = &[".png", ".jpg", ".jpeg", ".webp", ".img.json"];
        let lower = rel.to_ascii_lowercase();
        if !DRAWN.iter().any(|kind| lower.ends_with(kind)) {
            return None;
        }
        let project = self.project.as_ref()?;
        let path = proton_game::path::inside(project.root(), rel).ok()?;
        path.is_file().then_some(path)
    }

    /// The files with unsaved changes, as the project names them.
    pub fn unsaved(&self) -> Vec<String> {
        let Some(project) = &self.project else { return Vec::new() };
        project
            .unsaved_files()
            .into_iter()
            .map(|rel| match self.shown.as_str() {
                "." => rel,
                folder => format!("{folder}/{rel}"),
            })
            .collect()
    }

    /// Save everything unsaved; what could not be, and why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        match self.project.as_mut().map(Project::save) {
            Some(Err(why)) => vec![why],
            _ => Vec::new(),
        }
    }

    /* ── events ──────────────────────────────────────────────────── */

    pub fn home_event(&self, root: &Path) -> Event {
        Event::GameHome {
            games: find(root),
            templates: proton_game::template::TEMPLATES
                .iter()
                .map(|(id, name)| GameTemplate { id: id.to_string(), name: name.to_string() })
                .collect(),
        }
    }

    pub fn project_event(&self) -> Event {
        let Some(project) = &self.project else {
            return Event::GameProject { project: None };
        };
        let dirty = project.unsaved();
        let world = project.world.get();
        let rooms = project
            .rooms
            .iter()
            .map(|doc| {
                let room = doc.get();
                GameRoomRow {
                    id: room.id.clone(),
                    area: world.rooms.iter().find(|r| r.id == room.id).map(|r| r.area.clone()).unwrap_or_default(),
                    size: room.size,
                    dirty: doc.is_dirty(),
                }
            })
            .collect();
        Event::GameProject {
            project: Some(GameProjectView {
                path: self.shown.clone(),
                game: project.game.get().clone(),
                world: world.clone(),
                prefabs: project.prefabs.iter().map(|p| p.get().clone()).collect(),
                rooms,
                open: self.open.clone(),
                focus: self.focus.clone(),
                game_dirty: dirty.contains(&Target::Game),
                world_dirty: dirty.contains(&Target::World),
                dirty: !dirty.is_empty(),
                can_undo: !self.done.is_empty(),
                can_redo: !self.undone.is_empty(),
                problems: project.problems(),
                recordings: self.recordings.iter().rev().cloned().collect(),
                assets: assets(project.root()),
            }),
        }
    }

    pub fn room_event(&self) -> Event {
        let room: Option<Room> = self
            .project
            .as_ref()
            .zip(self.focus.as_deref())
            .and_then(|(project, id)| project.room(id))
            .map(|doc| doc.get().clone());
        Event::GameRoom { room, revision: self.revision }
    }
}

/// `path` as a folder: the project's own for `.` or nothing, else under it
/// unless absolute.
fn resolve(root: &Path, path: &str) -> PathBuf {
    match path.trim() {
        "" | "." => root.to_path_buf(),
        path if Path::new(path).is_absolute() => PathBuf::from(path),
        path => root.join(path),
    }
}

/// `dir` as the project names it.
fn shown(root: &Path, dir: &Path) -> String {
    match dir.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".into(),
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => dir.to_string_lossy().into_owned(),
    }
}

/// A folder name for a game called `name`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    match out.is_empty() {
        true => "game".into(),
        false => out,
    }
}

/// The games in a project: itself, or folders up to two below it — a
/// `games/` folder of several is ordinary.
fn find(root: &Path) -> Vec<GameFound> {
    let mut dirs = vec![root.to_path_buf()];
    let skip = |name: &str| name.starts_with('.') || matches!(name, "node_modules" | "target" | "build" | "dist");
    let children = |dir: &Path| -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|d| d.flatten().filter(|e| e.path().is_dir() && !skip(&e.file_name().to_string_lossy())).map(|e| e.path()).collect())
            .unwrap_or_default();
        found.sort();
        found
    };
    for child in children(root) {
        dirs.push(child.clone());
        dirs.extend(children(&child));
    }
    dirs.into_iter()
        .filter(|dir| Project::is_game(dir))
        .filter_map(|dir| {
            let game: GameFile = proton_game::doc::parse(proton_game::project::GAME, &std::fs::read_to_string(dir.join(proton_game::project::GAME)).ok()?).ok()?;
            let rooms = std::fs::read_to_string(dir.join(proton_game::project::WORLD))
                .ok()
                .and_then(|text| proton_game::doc::parse::<World>(proton_game::project::WORLD, &text).ok())
                .map(|w| w.rooms.len())
                .unwrap_or(0);
            Some(GameFound { path: shown(root, &dir), name: game.name, niche: game.niche, rooms })
        })
        .take(50)
        .collect()
}

/// The images and sounds in a game's folder, relative to it, for the Assets
/// tab — hidden folders and the game's own JSON left out.
fn assets(root: &Path) -> Vec<String> {
    const SHOWN: &[&str] = &["png", "jpg", "jpeg", "webp", "psd", "aseprite", "ase", "glb", "gltf", "wav", "mp3", "m4a"];
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| SHOWN.contains(&e.to_ascii_lowercase().as_str())) {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
            if out.len() >= 2000 {
                break;
            }
        }
    }
    out.sort();
    out
}
