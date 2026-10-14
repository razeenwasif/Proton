//! Bringing art into the open game: a `.psd` or `.png` in its folder made
//! into a room's planes, or an Aseprite file or a sprite sheet's JSON made
//! into an animation (`proton_game::import` does the making).
//!
//! The images are written at once — they are new files beside the game's
//! art, and nothing else names them yet. The room's change is the person's
//! kind of edit: one step of their undo, held until they save. Importing a
//! file again replaces the planes and images it made the time before rather
//! than adding a second copy; `import.json` remembers what each file made.
//! An image an earlier import wrote and this one no longer does is said, not
//! deleted: it is the person's file, and they may have used it elsewhere.

use std::path::Path;

use proton_game::format::{Imported, Imports, Plane};
use proton_game::import::{import, Importing, Made};
use proton_game::project::Target;
use proton_game::Op;

use super::Game;
use crate::bus::Event;

/// Where a game keeps what was imported into it.
pub const IMPORTS: &str = "import.json";

/// What one import did, in words, and what the window is sent.
pub struct Done {
    pub said: String,
    pub notes: Vec<String>,
    pub events: Vec<Event>,
}

/// FNV-1a, hex: enough to tell a file changed, not a seal.
fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// A file's size and time, to tell cheaply whether it changed since.
pub fn stamp(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let time = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(format!("{}:{}", meta.len(), time.as_millis()))
}

/// A game's record of imports, or an empty one.
pub fn read_imports(root: &Path) -> Imports {
    std::fs::read_to_string(root.join(IMPORTS)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// `base`, or `base_2`, `base_3`… if `taken` has it.
fn free(taken: &[String], base: &str) -> String {
    let mut name = base.to_string();
    let mut n = 2;
    while taken.iter().any(|t| *t == name) {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

impl Game {
    /// Import `source` (in the game's folder) into a room: the one named, the
    /// one on screen, or the one the game starts in — or, an Aseprite file
    /// or a sheet's JSON, as an animation.
    pub fn import_art(&mut self, source: &str, room: Option<&str>, scale: Option<f64>, normals: bool) -> Result<Done, String> {
        let project = self.project.as_ref().ok_or("no game is open")?;
        let root = project.root().to_path_buf();
        let path = proton_game::path::inside(&root, source)?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{source}: {e}"))?;
        if proton_game::import::anim::is_anim_source(source, &bytes) {
            return self.import_anim(source, &bytes, &path);
        }
        let previous = self.imports.sources.iter().find(|s| s.source == source).cloned();
        let id = room
            .map(str::to_string)
            .or_else(|| previous.as_ref().map(|p| p.room.clone()).filter(|r| project.room(r).is_some()))
            .or_else(|| self.focus.clone())
            .unwrap_or_else(|| project.game.get().start.room.clone());
        let doc = project.room(&id).ok_or_else(|| format!("there is no room {id}"))?;
        let how = Importing { scale: scale.or(previous.as_ref().map(|p| p.scale)).unwrap_or(1.0), normals };
        let made = import(source, &bytes, &how)?;

        // The images first: a room naming images that failed to be written is
        // worse than images nothing names yet.
        for (rel, data) in &made.files {
            let at = proton_game::path::inside(&root, rel)?;
            if std::fs::read(&at).ok().as_deref() == Some(data.as_slice()) {
                continue;
            }
            if let Some(dir) = at.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            proton_game::doc::write_atomically(&at, data).map_err(|e| format!("{rel}: {e}"))?;
        }

        // The room: what this file made the time before taken out, what it
        // makes now put in, each plane where its depth puts it.
        let mut planes: Vec<Plane> = doc.get().planes.clone();
        let (gone_planes, gone_items) = match &previous {
            Some(p) if p.room == id => (p.planes.clone(), p.items.clone()),
            _ => (Vec::new(), Vec::new()),
        };
        planes.retain(|p| !gone_planes.contains(&p.id));
        for plane in &mut planes {
            plane.items.retain(|i| !gone_items.contains(&i.id));
        }
        let (added_planes, added_items) = place(&mut planes, &made);
        let edit = [Op::Set { path: vec!["planes".into()], value: serde_json::to_value(&planes).map_err(|e| e.to_string())? }];
        let mut events = self.edit(&Target::Room(id.clone()), &edit);
        if events.iter().any(|e| matches!(e, Event::Notice { level: crate::bus::NoticeLevel::Warn, .. })) {
            return Err(events
                .into_iter()
                .find_map(|e| match e {
                    Event::Notice { text, .. } => Some(text),
                    _ => None,
                })
                .unwrap_or_default());
        }

        // What it made, remembered for next time.
        let files: Vec<String> = made.files.iter().map(|(f, _)| f.clone()).collect();
        let unused: Vec<String> = previous.map(|p| p.files.into_iter().filter(|f| !files.contains(f)).collect()).unwrap_or_default();
        let record = Imported {
            source: source.to_string(),
            hash: hash(&bytes),
            room: id.clone(),
            scale: how.scale,
            normals,
            planes: added_planes.clone(),
            items: added_items.clone(),
            files: files.clone(),
            stamp: stamp(&path).unwrap_or_default(),
            anim: String::new(),
        };
        match self.imports.sources.iter_mut().find(|s| s.source == source) {
            Some(old) => *old = record,
            None => self.imports.sources.push(record),
        }
        let text = proton_game::doc::to_text(&self.imports);
        proton_game::doc::write_atomically(&root.join(IMPORTS), text.as_bytes()).map_err(|e| format!("{IMPORTS}: {e}"))?;

        // Shown: the room it went into, on screen.
        if !self.open.contains(&id) {
            self.open.push(id.clone());
        }
        if self.focus.as_deref() != Some(id.as_str()) {
            self.focus = Some(id.clone());
            self.revision += 1;
        }
        events.retain(|e| !matches!(e, Event::GameProject { .. } | Event::GameRoom { .. }));
        events.push(self.project_event());
        events.push(self.room_event());

        let file = source.rsplit('/').next().unwrap_or(source);
        let mut said = format!(
            "imported {file} into {id}: {} plane{}{}, {} image{} written{}",
            added_planes.len(),
            if added_planes.len() == 1 { "" } else { "s" },
            if added_items.is_empty() { String::new() } else { format!(" and {} image{} on the play plane", added_items.len(), if added_items.len() == 1 { "" } else { "s" }) },
            files.iter().filter(|f| f.ends_with(".png")).count(),
            if files.iter().filter(|f| f.ends_with(".png")).count() == 1 { "" } else { "s" },
            if made.cut > 0 { format!(" ({} cut into pieces)", made.cut) } else { String::new() },
        );
        let mut notes = made.notes;
        if !unused.is_empty() {
            notes.push(format!("the last import also wrote {}, which nothing from this one uses: delete {} if nothing else does", unused.join(", "), if unused.len() == 1 { "it" } else { "them" }));
        }
        if !notes.is_empty() {
            said.push_str(&format!(" — {} note{}", notes.len(), if notes.len() == 1 { "" } else { "s" }));
        }
        Ok(Done { said, notes, events })
    }
}

impl Game {
    /// An Aseprite file or a sheet's JSON made into an `.anim.json` beside
    /// it, its sheets with it: files written at once, as art's images are —
    /// nothing in a room changes. A prefab plays it once its sprite names it.
    fn import_anim(&mut self, source: &str, bytes: &[u8], path: &Path) -> Result<Done, String> {
        let project = self.project.as_ref().ok_or("no game is open")?;
        let root = project.root().to_path_buf();
        let made = proton_game::import::anim::import_anim(source, bytes)?;
        let mut notes = made.notes.clone();
        for sheet in &made.anim.sheets {
            let written = made.files.iter().any(|(f, _)| f == sheet);
            if !written && !proton_game::path::inside(&root, sheet).is_ok_and(|p| p.is_file()) {
                notes.push(format!("{source} says its frames are on {sheet}, which is not in the game's folder"));
            }
        }
        for (rel, data) in &made.files {
            let at = proton_game::path::inside(&root, rel)?;
            if std::fs::read(&at).ok().as_deref() == Some(data.as_slice()) {
                continue;
            }
            proton_game::doc::write_atomically(&at, data).map_err(|e| format!("{rel}: {e}"))?;
        }
        let files: Vec<String> = made.files.iter().map(|(f, _)| f.clone()).collect();
        let previous = self.imports.sources.iter().find(|s| s.source == source).cloned();
        let unused: Vec<String> = previous.map(|p| p.files.into_iter().filter(|f| !files.contains(f)).collect()).unwrap_or_default();
        let record = Imported { source: source.to_string(), hash: hash(bytes), files, stamp: stamp(path).unwrap_or_default(), anim: made.path.clone(), ..Imported::default() };
        match self.imports.sources.iter_mut().find(|s| s.source == source) {
            Some(old) => *old = record,
            None => self.imports.sources.push(record),
        }
        let text = proton_game::doc::to_text(&self.imports);
        proton_game::doc::write_atomically(&root.join(IMPORTS), text.as_bytes()).map_err(|e| format!("{IMPORTS}: {e}"))?;
        if !unused.is_empty() {
            notes.push(format!("the last import also wrote {}, which this one does not: delete {} if nothing else uses {}", unused.join(", "), if unused.len() == 1 { "it" } else { "them" }, if unused.len() == 1 { "it" } else { "them" }));
        }
        let a = &made.anim;
        let file = source.rsplit('/').next().unwrap_or(source);
        let users: Vec<String> = self
            .project
            .as_ref()
            .map(|p| p.prefabs.iter().map(|d| d.get()).filter(|f| f.sprite.as_deref() == Some(made.path.as_str())).map(|f| f.id.clone()).collect())
            .unwrap_or_default();
        let mut said = format!(
            "made {} of {file}: {} frame{}, clip{} {}{}{}",
            made.path,
            a.frames.len(),
            if a.frames.len() == 1 { "" } else { "s" },
            if a.clips.len() == 1 { "" } else { "s" },
            a.clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "),
            if a.boxes.is_empty() { String::new() } else { format!(", box{} {}", if a.boxes.len() == 1 { "" } else { "es" }, a.boxes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(", ")) },
            if a.cues.is_empty() { String::new() } else { format!(", {} cue{}", a.cues.len(), if a.cues.len() == 1 { "" } else { "s" }) },
        );
        said.push_str(&match users.as_slice() {
            [] => " — a prefab plays it once its sprite is this file".to_string(),
            ids => format!(" — played by {}", ids.join(", ")),
        });
        if !notes.is_empty() {
            said.push_str(&format!(" — {} note{}", notes.len(), if notes.len() == 1 { "" } else { "s" }));
        }
        Ok(Done { said, notes, events: vec![self.project_event()] })
    }
}

/// Put an import's planes into `planes` by depth (further first), and its
/// play-plane images onto the play plane; every id made free in the room.
/// The ids given, of planes and of play-plane items.
fn place(planes: &mut Vec<Plane>, made: &Made) -> (Vec<String>, Vec<String>) {
    let mut items: Vec<String> = planes.iter().flat_map(|p| p.items.iter().map(|i| i.id.clone())).collect();
    let mut added_planes = Vec::new();
    let mut added_items = Vec::new();
    fn free_item(id: &str, items: &mut Vec<String>) -> String {
        let id = free(items, id);
        items.push(id.clone());
        id
    }
    fn put(planes: &mut Vec<Plane>, plane: Plane) {
        let at = planes.iter().position(|p| p.depth < plane.depth).unwrap_or(planes.len());
        planes.insert(at, plane);
    }
    for plane in &made.planes {
        let mut plane = plane.clone();
        let ids: Vec<String> = planes.iter().map(|p| p.id.clone()).collect();
        plane.id = free(&ids, &plane.id);
        for item in &mut plane.items {
            item.id = free_item(&item.id, &mut items);
        }
        added_planes.push(plane.id.clone());
        put(planes, plane);
    }
    if !made.play.is_empty() {
        let mut play: Vec<_> = made.play.clone();
        for item in &mut play {
            item.id = free_item(&item.id, &mut items);
            added_items.push(item.id.clone());
        }
        match planes.iter_mut().find(|p| p.gameplay) {
            Some(plane) => plane.items.extend(play),
            None => {
                // A room with no play plane gets one, made of what was painted for it.
                let ids: Vec<String> = planes.iter().map(|p| p.id.clone()).collect();
                let plane = Plane { id: free(&ids, "play"), name: "Play".into(), gameplay: true, items: play, ..Plane::default() };
                added_planes.push(plane.id.clone());
                added_items.clear();
                put(planes, plane);
            }
        }
    }
    (added_planes, added_items)
}
