//! A game's files, open for editing: changed by ops, undone and redone, and
//! written when the person saves.
//!
//! ## Ops, by id
//!
//! The window sends a gesture as a batch of ops, each naming where it acts by
//! a path through the document. A path addresses a list's member by its id —
//! `["planes", "#sky", "depth"]` — rather than by its place, because places
//! move: a plane dragged to the back while an edit to it was on its way would
//! otherwise have the edit land on whatever took its place. A number still
//! addresses a list with no ids, like a shape's points.
//!
//! ## A batch is whole or nothing
//!
//! The ops are applied to a copy, the copy is read back as the document's own
//! type and checked, and only then does it replace the document. A batch that
//! leaves a room with two planes called `sky`, or a depth that is a string,
//! changes nothing and says why.
//!
//! ## Held until saved
//!
//! As in PDF: an edit changes the document in memory, the tab shows a dot,
//! and the file is written on ⌘S — atomically, beside it and renamed over it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::format::{GameFile, Image, Prefab, Room, World, FORMAT};

/// How many steps back undo goes, per document.
const UNDO: usize = 200;

/// One change, at a path through a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "GameOp")]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    /// Put `value` at `path`: replace what is there, or add a key to an object.
    Set {
        path: Vec<String>,
        #[ts(type = "unknown")]
        value: Value,
    },
    /// Put `value` into the list at `path`, at `at` or on the end.
    Insert {
        path: Vec<String>,
        #[ts(type = "unknown")]
        value: Value,
        #[serde(default)]
        #[ts(optional)]
        at: Option<usize>,
    },
    /// Take away what is at `path`.
    Remove { path: Vec<String> },
    /// Move the list member at `path` to place `to` in its list.
    Move { path: Vec<String>, to: usize },
}

/// What a game's documents must be: read and written as JSON, compared, and
/// checked for what the type alone cannot say.
pub trait Document: Serialize + DeserializeOwned + Clone + PartialEq {
    /// What is wrong with it, in words; empty when nothing is.
    fn problems(&self) -> Vec<String>;
}

/// One file of a game, as it is now and as it was saved.
#[derive(Debug, Clone)]
pub struct Doc<T: Document> {
    /// Relative to the game's folder.
    rel: String,
    value: T,
    /// As the file has it; `None` for a document not written yet.
    saved: Option<T>,
    undo: Vec<T>,
    redo: Vec<T>,
}

impl<T: Document> Doc<T> {
    /// Read `rel` from the game at `root`.
    pub fn read(root: &Path, rel: &str) -> Result<Doc<T>, String> {
        let path = crate::path::inside(root, rel)?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
        let value = parse::<T>(rel, &text)?;
        Ok(Doc { rel: rel.to_string(), saved: Some(value.clone()), value, undo: Vec::new(), redo: Vec::new() })
    }

    /// A document that exists only in memory until it is saved.
    pub fn fresh(rel: &str, value: T) -> Doc<T> {
        Doc { rel: rel.to_string(), value, saved: None, undo: Vec::new(), redo: Vec::new() }
    }

    pub fn rel(&self) -> &str {
        &self.rel
    }

    pub fn get(&self) -> &T {
        &self.value
    }

    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.value)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply a batch: all of it, or — when any op cannot be done or the
    /// result is not a valid document — none of it, and why. `Ok(false)`
    /// when it changed nothing, which is not an undo step.
    pub fn apply(&mut self, ops: &[Op]) -> Result<bool, String> {
        let mut tree = serde_json::to_value(&self.value).map_err(|e| e.to_string())?;
        for op in ops {
            apply(&mut tree, op)?;
        }
        let next: T = serde_json::from_value(tree).map_err(|e| format!("{}: {e}", self.rel))?;
        let problems = next.problems();
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        if next == self.value {
            return Ok(false);
        }
        self.replace(next);
        Ok(true)
    }

    /// Replace the whole document as one undo step — for a change made other
    /// than by ops, like a template's room.
    pub fn replace(&mut self, next: T) {
        let was = std::mem::replace(&mut self.value, next);
        self.undo.push(was);
        if self.undo.len() > UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some(was) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.value, was));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.value, next));
        true
    }

    /// The document as its file's text.
    pub fn text(&self) -> String {
        to_text(&self.value)
    }

    /// Write it, whole or not at all: beside the file, then renamed over it.
    /// Undo survives a save, as in PDF: undoing past it makes the tab dirty
    /// again.
    pub fn save(&mut self, root: &Path) -> Result<(), String> {
        let path = crate::path::inside(root, &self.rel)?;
        write_atomically(&path, self.text().as_bytes()).map_err(|e| format!("{}: {e}", self.rel))?;
        self.saved = Some(self.value.clone());
        Ok(())
    }

    /// The file changed under it — read again — and nothing is unsaved here:
    /// take what the file says, as a step that can be undone.
    pub fn reread(&mut self, root: &Path) -> Result<bool, String> {
        let path = crate::path::inside(root, &self.rel)?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", self.rel))?;
        let value = parse::<T>(&self.rel, &text)?;
        if value == self.value {
            self.saved = Some(value);
            return Ok(false);
        }
        self.replace(value.clone());
        self.saved = Some(value);
        Ok(true)
    }
}

/// A value as its file's text. The format's types always serialise, so the
/// error is unreachable for them.
pub fn to_text<T: Serialize>(value: &T) -> String {
    crate::text::to_text(value).unwrap_or_else(|e| format!("{{\"error\": {}}}\n", serde_json::Value::String(e)))
}

/// A file's text as a document, refusing a later format than this one reads.
pub fn parse<T: Document>(rel: &str, text: &str) -> Result<T, String> {
    let tree: Value = serde_json::from_str(text).map_err(|e| format!("{rel}: {e}"))?;
    if let Some(format) = tree.get("format").and_then(Value::as_u64) {
        if format > FORMAT as u64 {
            return Err(format!("{rel} is format {format}, from a later Proton than this one (which reads {FORMAT})"));
        }
    }
    let value: T = serde_json::from_value(tree).map_err(|e| format!("{rel}: {e}"))?;
    let problems = value.problems();
    match problems.is_empty() {
        true => Ok(value),
        false => Err(format!("{rel}: {}", problems.join("; "))),
    }
}

/// Write `bytes` to `path` so that a reader sees the old file or the new one
/// and never half of either.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut part = path.as_os_str().to_owned();
    part.push(".proton-saving");
    let part = PathBuf::from(part);
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}

/* ── ops on a tree ────────────────────────────────────────────────── */

fn apply(tree: &mut Value, op: &Op) -> Result<(), String> {
    match op {
        Op::Set { path, value } => {
            let (last, parent) = split(path)?;
            match at(tree, parent)? {
                Value::Object(map) => {
                    map.insert(last.clone(), value.clone());
                }
                list @ Value::Array(_) => {
                    let place = member(list, last)?;
                    list.as_array_mut().unwrap()[place] = value.clone();
                }
                _ => return Err(format!("{} is not inside an object or a list", show(path))),
            }
        }
        Op::Insert { path, value, at: place } => {
            let id = value.get("id").and_then(Value::as_str).map(str::to_string);
            let Value::Array(list) = at(tree, path)? else {
                return Err(format!("{} is not a list", show(path)));
            };
            if let Some(id) = id {
                if list.iter().any(|m| m.get("id").and_then(Value::as_str) == Some(&id)) {
                    return Err(format!("{} already has a {id}", show(path)));
                }
            }
            let place = place.unwrap_or(list.len()).min(list.len());
            list.insert(place, value.clone());
        }
        Op::Remove { path } => {
            let (last, parent) = split(path)?;
            match at(tree, parent)? {
                Value::Object(map) => {
                    if map.remove(last).is_none() {
                        return Err(format!("there is no {}", show(path)));
                    }
                }
                list @ Value::Array(_) => {
                    let place = member(list, last)?;
                    list.as_array_mut().unwrap().remove(place);
                }
                _ => return Err(format!("{} is not inside an object or a list", show(path))),
            }
        }
        Op::Move { path, to } => {
            let (last, parent) = split(path)?;
            let list = at(tree, parent)?;
            if !list.is_array() {
                return Err(format!("{} is not in a list", show(path)));
            }
            let place = member(list, last)?;
            let list = list.as_array_mut().unwrap();
            let moving = list.remove(place);
            list.insert((*to).min(list.len()), moving);
        }
    }
    Ok(())
}

fn split(path: &[String]) -> Result<(&String, &[String]), String> {
    path.split_last().ok_or_else(|| "an op needs a path".to_string())
}

/// What `path` leads to.
fn at<'a>(tree: &'a mut Value, path: &[String]) -> Result<&'a mut Value, String> {
    let mut here = tree;
    for (depth, step) in path.iter().enumerate() {
        here = match here {
            Value::Object(map) => map
                .get_mut(step)
                .ok_or_else(|| format!("there is no {}", show(&path[..=depth])))?,
            list @ Value::Array(_) => {
                let place = member(list, step)?;
                &mut list.as_array_mut().unwrap()[place]
            }
            _ => return Err(format!("{} is not inside an object or a list", show(&path[..=depth]))),
        };
    }
    Ok(here)
}

/// Where in a list `step` is: `#id` by id, a number by place.
fn member(list: &Value, step: &str) -> Result<usize, String> {
    let members = list.as_array().ok_or("not a list")?;
    if let Some(id) = step.strip_prefix('#') {
        return members
            .iter()
            .position(|m| m.get("id").and_then(Value::as_str) == Some(id))
            .ok_or_else(|| format!("there is no {id}"));
    }
    let place: usize = step.parse().map_err(|_| format!("{step} is neither #id nor a place in the list"))?;
    match place < members.len() {
        true => Ok(place),
        false => Err(format!("there is nothing at {place}")),
    }
}

fn show(path: &[String]) -> String {
    path.join("/")
}

/* ── what each document checks ────────────────────────────────────── */

/// Ids that appear twice in a list, said as a problem.
fn twice<'a>(what: &str, ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut twice = BTreeSet::new();
    for id in ids {
        if id.is_empty() {
            twice.insert(format!("a {what} has no id"));
        } else if !seen.insert(id) {
            twice.insert(format!("two {what}s are called {id}"));
        }
    }
    twice.into_iter().collect()
}

impl Document for GameFile {
    fn problems(&self) -> Vec<String> {
        let mut out = twice("ability", self.abilities.iter().map(|a| a.id.as_str()));
        if self.view.width == 0 || self.view.height == 0 {
            out.push("the view has no size".into());
        }
        if self.view.focal <= 0.0 {
            out.push("the camera's focal length must be more than 0".into());
        }
        if self.physics.tick == 0 {
            out.push("the game must step at least once a second".into());
        }
        if self.controller.jump.height <= 0 || self.controller.jump.apex <= 0.0 {
            out.push("a jump must go up, and take time to".into());
        }
        out
    }
}

impl Document for World {
    fn problems(&self) -> Vec<String> {
        let mut out = twice("area", self.areas.iter().map(|a| a.id.as_str()));
        out.extend(twice("room", self.rooms.iter().map(|r| r.id.as_str())));
        out
    }
}

impl Document for Room {
    fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.size[0] <= 0 || self.size[1] <= 0 {
            out.push("a room needs a width and a height".into());
        }
        out.extend(twice("plane", self.planes.iter().map(|p| p.id.as_str())));
        out.extend(twice("item", self.planes.iter().flat_map(|p| &p.items).map(|i| i.id.as_str())));
        out.extend(twice("shape", self.collision.iter().map(|s| s.id.as_str())));
        out.extend(twice("entity", self.entities.iter().map(|e| e.id.as_str())));
        out.extend(twice("light", self.lights.iter().map(|l| l.id.as_str())));
        out.extend(twice("spawn", self.spawns.iter().map(|s| s.id.as_str())));
        if self.planes.iter().filter(|p| p.gameplay).count() > 1 {
            out.push("only one plane can be the one the game is played on".into());
        }
        for shape in &self.collision {
            if shape.points.len() < 2 {
                out.push(format!("shape {} needs at least two points", shape.id));
            }
        }
        for light in &self.lights {
            if !self.planes.iter().any(|p| p.id == light.plane) {
                out.push(format!("light {} is on plane {}, which is not in the room", light.id, light.plane));
            }
        }
        for item in self.planes.iter().flat_map(|p| &p.items) {
            if item.scale <= 0.0 {
                out.push(format!("item {} must have a size", item.id));
            }
        }
        out
    }
}

impl Document for Prefab {
    fn problems(&self) -> Vec<String> {
        match self.size[0] <= 0 || self.size[1] <= 0 {
            true => vec![format!("prefab {} needs a size", self.id)],
            false => Vec::new(),
        }
    }
}

impl Document for Image {
    fn problems(&self) -> Vec<String> {
        Vec::new()
    }
}
