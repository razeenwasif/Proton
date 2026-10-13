//! A game, open: all of its documents at once.
//!
//! An edit names the document it is for — `game`, `world`, `room:<id>`,
//! `prefab:<id>` — and each document keeps its own undo, so ⌘Z in one room
//! does not reach into another.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::doc::{Doc, Op};
use crate::format::{GameFile, Prefab, Room, World};

/// Which of a game's documents an edit is for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameTarget")]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum Target {
    Game,
    World,
    Room(String),
    Prefab(String),
}

impl Target {
    /// `game`, `world`, `room:crossroads_01`, `prefab:crawler`.
    pub fn parse(text: &str) -> Option<Target> {
        match text.split_once(':') {
            None if text == "game" => Some(Target::Game),
            None if text == "world" => Some(Target::World),
            Some(("room", id)) if !id.is_empty() => Some(Target::Room(id.to_string())),
            Some(("prefab", id)) if !id.is_empty() => Some(Target::Prefab(id.to_string())),
            _ => None,
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Game => write!(f, "game"),
            Target::World => write!(f, "world"),
            Target::Room(id) => write!(f, "room:{id}"),
            Target::Prefab(id) => write!(f, "prefab:{id}"),
        }
    }
}

/// A game's documents, read from its folder.
#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    pub game: Doc<GameFile>,
    pub world: Doc<World>,
    /// In the world's order.
    pub rooms: Vec<Doc<Room>>,
    pub prefabs: Vec<Doc<Prefab>>,
}

/// Where a game's own files are, relative to its folder.
pub const GAME: &str = "game.json";
pub const WORLD: &str = "world.json";

impl Project {
    /// Whether `dir` is a game's folder.
    pub fn is_game(dir: &Path) -> bool {
        dir.join(GAME).is_file()
    }

    /// Open the game in `root`: its game, its world, every room the world
    /// names and every prefab in `prefabs/`.
    pub fn open(root: &Path) -> Result<Project, String> {
        let game = Doc::<GameFile>::read(root, GAME)?;
        let world = match root.join(WORLD).is_file() {
            true => Doc::<World>::read(root, WORLD)?,
            false => Doc::fresh(WORLD, World::default()),
        };
        let mut rooms = Vec::new();
        for room in &world.get().rooms {
            let doc = Doc::<Room>::read(root, &room.file)?;
            if doc.get().id != room.id {
                return Err(format!("{} says it is room {}, and the world calls it {}", room.file, doc.get().id, room.id));
            }
            rooms.push(doc);
        }
        let mut prefabs = Vec::new();
        let mut files: Vec<String> = std::fs::read_dir(root.join("prefabs"))
            .map(|dir| {
                dir.flatten()
                    .filter_map(|e| e.file_name().to_str().map(str::to_string))
                    .filter(|name| name.ends_with(".prefab.json"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for name in files {
            prefabs.push(Doc::<Prefab>::read(root, &format!("prefabs/{name}"))?);
        }
        Ok(Project { root: root.to_path_buf(), game, world, rooms, prefabs })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn room(&self, id: &str) -> Option<&Doc<Room>> {
        self.rooms.iter().find(|r| r.get().id == id)
    }

    pub fn prefab(&self, id: &str) -> Option<&Doc<Prefab>> {
        self.prefabs.iter().find(|p| p.get().id == id)
    }

    /// Apply a batch of ops to one document. `Ok(false)` when it changed
    /// nothing.
    pub fn apply(&mut self, target: &Target, ops: &[Op]) -> Result<bool, String> {
        match target {
            Target::Game => self.game.apply(ops),
            Target::World => self.world.apply(ops),
            Target::Room(id) => self.room_mut(id)?.apply(ops),
            Target::Prefab(id) => self.prefab_mut(id)?.apply(ops),
        }
    }

    pub fn undo(&mut self, target: &Target) -> Result<bool, String> {
        Ok(match target {
            Target::Game => self.game.undo(),
            Target::World => self.world.undo(),
            Target::Room(id) => self.room_mut(id)?.undo(),
            Target::Prefab(id) => self.prefab_mut(id)?.undo(),
        })
    }

    pub fn redo(&mut self, target: &Target) -> Result<bool, String> {
        Ok(match target {
            Target::Game => self.game.redo(),
            Target::World => self.world.redo(),
            Target::Room(id) => self.room_mut(id)?.redo(),
            Target::Prefab(id) => self.prefab_mut(id)?.redo(),
        })
    }

    fn room_mut(&mut self, id: &str) -> Result<&mut Doc<Room>, String> {
        self.rooms.iter_mut().find(|r| r.get().id == id).ok_or_else(|| format!("there is no room {id}"))
    }

    fn prefab_mut(&mut self, id: &str) -> Result<&mut Doc<Prefab>, String> {
        self.prefabs.iter_mut().find(|p| p.get().id == id).ok_or_else(|| format!("there is no prefab {id}"))
    }

    /// Every document with changes its file does not have.
    pub fn unsaved(&self) -> Vec<Target> {
        let mut out = Vec::new();
        if self.game.is_dirty() {
            out.push(Target::Game);
        }
        if self.world.is_dirty() {
            out.push(Target::World);
        }
        out.extend(self.rooms.iter().filter(|r| r.is_dirty()).map(|r| Target::Room(r.get().id.clone())));
        out.extend(self.prefabs.iter().filter(|p| p.is_dirty()).map(|p| Target::Prefab(p.get().id.clone())));
        out
    }

    /// The file each unsaved document is written to, for the quit prompt.
    pub fn unsaved_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.game.is_dirty() {
            out.push(self.game.rel().to_string());
        }
        if self.world.is_dirty() {
            out.push(self.world.rel().to_string());
        }
        out.extend(self.rooms.iter().filter(|r| r.is_dirty()).map(|r| r.rel().to_string()));
        out.extend(self.prefabs.iter().filter(|p| p.is_dirty()).map(|p| p.rel().to_string()));
        out
    }

    /// Write every unsaved document, the world after the rooms it names.
    /// What could not be written is said; the rest is written regardless.
    pub fn save(&mut self) -> Result<Vec<String>, String> {
        let root = self.root.clone();
        let mut wrote = Vec::new();
        let mut failed = Vec::new();
        let mut one = |rel: &str, result: Result<(), String>| match result {
            Ok(()) => wrote.push(rel.to_string()),
            Err(why) => failed.push(why),
        };
        for room in self.rooms.iter_mut().filter(|r| r.is_dirty()) {
            let result = room.save(&root);
            one(room.rel(), result);
        }
        for prefab in self.prefabs.iter_mut().filter(|p| p.is_dirty()) {
            let result = prefab.save(&root);
            one(prefab.rel(), result);
        }
        if self.world.is_dirty() {
            let result = self.world.save(&root);
            one(WORLD, result);
        }
        if self.game.is_dirty() {
            let result = self.game.save(&root);
            one(GAME, result);
        }
        match failed.is_empty() {
            true => Ok(wrote),
            false => Err(failed.join("; ")),
        }
    }

    /// What is wrong across documents — a start room that is not in the
    /// world, a door to a spawn that is not there, a gate asking for an
    /// ability the game does not have. Said rather than refused: a game is
    /// often half made.
    pub fn problems(&self) -> Vec<String> {
        let game = self.game.get();
        let mut out = Vec::new();
        let abilities: Vec<&str> = game.abilities.iter().map(|a| a.id.as_str()).collect();
        match self.room(&game.start.room) {
            None if game.start.room.is_empty() => out.push("the game does not say which room it starts in".into()),
            None => out.push(format!("the game starts in room {}, which is not in the world", game.start.room)),
            Some(room) if !room.get().spawns.iter().any(|s| s.id == game.start.spawn) => {
                out.push(format!("room {} has no spawn {} to start at", game.start.room, game.start.spawn))
            }
            Some(_) => {}
        }
        for room in &self.rooms {
            let room = room.get();
            if !self.world.get().areas.is_empty() {
                let area = &self.world.get().rooms.iter().find(|r| r.id == room.id).map(|r| r.area.clone()).unwrap_or_default();
                if !self.world.get().areas.iter().any(|a| &a.id == area) {
                    out.push(format!("room {} is in area {area}, which the world does not have", room.id));
                }
            }
            for entity in &room.entities {
                if let Some(to) = &entity.to {
                    match self.room(&to.room) {
                        None => out.push(format!("{} in room {} leads to room {}, which is not in the world", entity.id, room.id, to.room)),
                        Some(there) if !there.get().spawns.iter().any(|s| s.id == to.entry) => out.push(format!(
                            "{} in room {} leads to {} in room {}, which is not a spawn there",
                            entity.id, room.id, to.entry, to.room
                        )),
                        Some(_) => {}
                    }
                }
                for need in entity.requires.iter().chain(entity.grants.iter()) {
                    if !abilities.contains(&need.as_str()) {
                        out.push(format!("{} in room {} names ability {need}, which the game does not have", entity.id, room.id));
                    }
                }
                let built_in = ["transition", "save_point", "ability_gate", "ability_pickup"];
                if !built_in.contains(&entity.prefab.as_str()) && self.prefab(&entity.prefab).is_none() {
                    out.push(format!("{} in room {} is a {}, which is neither built in nor a prefab of the game", entity.id, room.id, entity.prefab));
                }
            }
        }
        out
    }
}
