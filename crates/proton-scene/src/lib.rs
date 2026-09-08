//! Modeling mode's document: `scene.proton.json`.
//!
//! Written before anything draws a viewport, for the reason
//! `proton-data::notebook` and `proton-design::theme` were: *every mode's
//! document is a text file that diffs in Git* is the load-bearing rule of this
//! project, and everything built on top of a format is only as trustworthy as
//! the file it writes back.
//!
//! ## The hierarchy is flat, and that is the point
//!
//! A scene graph is a tree, and the obvious encoding nests children inside
//! parents. This does not. Objects are a flat list and each names its parent
//! by id, because the difference shows up the moment anybody reads a diff:
//!
//! - **Reparenting is one line.** Nested, moving a lamp from one room to
//!   another deletes a block here and inserts it there, and Git shows a large
//!   removal beside a large addition that happen to be the same object.
//! - **Depth does not become indentation.** A rig eight joints deep would
//!   start every line half a screen in.
//! - **An object's diff is about that object.** Nested, adding a child changes
//!   the parent's lines too.
//!
//! What it costs is that reading the tree means a pass over the list. That is
//! a few microseconds and it happens in the outliner, not in the file.
//!
//! ## What a save promises
//!
//! Not that any JSON survives byte for byte — a file written by another tool
//! with different spacing is normalised the first time it is saved, and
//! pretending otherwise would be a promise this cannot keep. The two promises
//! that hold for every file are the useful ones, and they are the same two
//! `notebook` settled on:
//!
//! - **Nothing is lost.** Keys this does not understand are carried through
//!   untouched, so a scene from a later version, or one a plugin annotated,
//!   comes back whole.
//! - **The second save changes nothing the first did not.** Idempotent, so a
//!   file only ever appears in a diff because somebody changed it.

mod material;
mod write;

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ts_rs::TS;

/// What the `format` key says, so a file that is not one of these is refused
/// rather than half-read.
const FORMAT: &str = "proton.scene";

/// A scene, and the JSON it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub(crate) json: Value,
}

/// One object in the scene, as the outliner draws it.
///
/// `Node` rather than `Object`, which is what a modelling tool would call it:
/// `proton-core::action::Object` is the vim text-object enum and got there
/// first, and the generated TypeScript lives in one flat directory where the
/// second `Object.ts` silently overwrites the first. Two types with one name
/// is a collision wherever they meet; here it is a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// Stable for the life of the object, and what `parent` refers to.
    ///
    /// Not the name: renaming a thing must not reparent everything under it,
    /// and two objects are allowed to be called `Cube`.
    pub id: String,
    /// What a person calls it.
    pub name: String,
    /// `mesh`, `camera`, `light`, `empty` — free text rather than an enum,
    /// because a scene from a later version may hold a kind this build has
    /// never heard of and dropping it would lose the object.
    pub kind: String,
    /// Empty for a root object.
    pub parent: String,
    /// The material this object wears, by id. Empty for one that wears none,
    /// which is most objects and every light and camera.
    pub material: String,
    /// Metres, degrees, and a multiplier, in that order.
    pub position: [f64; 3],
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
    /// How deep it sits, worked out when the tree is read. Not stored in the
    /// file — it is a fact about the parents, and writing it down would be a
    /// second copy that can disagree with them.
    pub depth: usize,
    /// How many objects name it as a parent. Derived the same way, and for the
    /// same reason.
    pub children: usize,
}

impl Scene {
    /// An empty scene, as a new file.
    pub fn new() -> Scene {
        Scene {
            json: json!({ "format": FORMAT, "version": 1, "objects": [] }),
        }
    }

    /// Read a scene, refusing anything that is not one.
    ///
    /// A stricter door than the other documents in this project have, because
    /// a `.json` in somebody's project is almost never a scene, and quietly
    /// showing an empty outliner for one would be a worse answer than saying
    /// what the file is.
    pub fn parse(text: &str) -> Result<Scene, String> {
        let json: Value = serde_json::from_str(text).map_err(|err| format!("not JSON: {err}"))?;
        match json.get("format").and_then(Value::as_str) {
            Some(FORMAT) => {}
            Some(other) => return Err(format!("{other:?} is not a Proton scene")),
            None => return Err("no \"format\" key — this is not a Proton scene".into()),
        }
        if !json.get("objects").is_some_and(Value::is_array) {
            return Err("a scene needs an \"objects\" list".into());
        }
        Ok(Scene { json })
    }

    pub fn read(path: &Path) -> Result<Scene, String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Scene::parse(&text).map_err(|why| format!("{}: {why}", path.display()))
    }

    /// The file as it should be written.
    pub fn to_text(&self) -> String {
        write::to_text(&self.json)
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.to_text()).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Every object, parents before children, with depth filled in.
    ///
    /// Depth-first from the roots rather than in file order, because that is
    /// the order an outliner draws and the order a person thinks in. An object
    /// whose parent is missing — a file edited by hand, or a delete that left
    /// an orphan — is treated as a root rather than dropped: showing it where
    /// somebody can find and fix it beats hiding it.
    pub fn objects(&self) -> Vec<Node> {
        let raw = self.raw();
        let mut out: Vec<Node> = Vec::with_capacity(raw.len());
        let known: Vec<String> = raw.iter().map(|one| id_of(one)).collect();

        let children_of = |parent: &str| -> Vec<usize> {
            raw.iter()
                .enumerate()
                .filter(|(_, one)| {
                    let mine = parent_of(one);
                    match parent.is_empty() {
                        // A root is one with no parent, or one whose parent is
                        // not in the file.
                        true => mine.is_empty() || !known.contains(&mine),
                        false => mine == parent,
                    }
                })
                .map(|(i, _)| i)
                .collect()
        };

        // An explicit stack rather than recursion: a scene is somebody's file
        // and a cycle in it — `a` parented to `b` parented to `a` — must not
        // be a stack overflow. `seen` is what makes that a missing branch
        // instead.
        let mut seen: Vec<bool> = vec![false; raw.len()];
        let mut stack: Vec<(usize, usize)> =
            children_of("").into_iter().rev().map(|i| (i, 0)).collect();
        loop {
            let Some((i, depth)) = stack.pop() else {
                // Nothing reaches what is left, which means a cycle: `a`
                // inside `b` inside `a` has no root to start from, so the
                // walk above never sees either and they would simply not
                // appear. Shown as a root of its own instead — an object
                // nobody can see is an object nobody can fix.
                match seen.iter().position(|done| !done) {
                    Some(next) => stack.push((next, 0)),
                    None => break,
                }
                continue;
            };
            if std::mem::replace(&mut seen[i], true) {
                continue;
            }
            let one = &raw[i];
            let id = id_of(one);
            let kids = children_of(&id);
            out.push(Node {
                name: string_of(one, "name", &id),
                kind: string_of(one, "kind", "empty"),
                parent: parent_of(one),
                material: string_of(one, "material", ""),
                position: triple(one, "position", [0.0; 3]),
                rotation: triple(one, "rotation", [0.0; 3]),
                scale: triple(one, "scale", [1.0; 3]),
                children: kids.len(),
                depth,
                id,
            });
            stack.extend(kids.into_iter().rev().map(|k| (k, depth + 1)));
        }
        out
    }

    /// Move, turn or resize one object.
    ///
    /// Answers whether anything changed, so a drag that ended where it started
    /// does not write the file.
    pub fn set_transform(&mut self, id: &str, what: Part, to: [f64; 3]) -> Result<bool, String> {
        let key = what.key();
        let one = self.find_mut(id)?;
        let now = json!([to[0], to[1], to[2]]);
        // Compared as written rather than as given: two values that differ
        // only past the sixth decimal are the same file, and writing one over
        // the other would be a diff with no change in it.
        if write::to_text(one.get(key).unwrap_or(&Value::Null)) == write::to_text(&now) {
            return Ok(false);
        }
        one.insert(key.to_string(), now);
        Ok(true)
    }

    /// Rename an object. The id does not move, so nothing is reparented.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<bool, String> {
        let one = self.find_mut(id)?;
        if one.get("name").and_then(Value::as_str) == Some(name) {
            return Ok(false);
        }
        one.insert("name".into(), json!(name));
        Ok(true)
    }

    /// Put an object under another, or at the root when `parent` is empty.
    ///
    /// Refuses to make a cycle. A scene where `a` is inside `b` is inside `a`
    /// has no tree to draw, and the outliner would simply not show them —
    /// which looks like the objects were deleted.
    pub fn reparent(&mut self, id: &str, parent: &str) -> Result<bool, String> {
        if id == parent {
            return Err("an object cannot be its own parent".into());
        }
        if !parent.is_empty() && self.ancestors(parent).any(|up| up == id) {
            return Err(format!(
                "{parent:?} is already inside {id:?} — that would make a loop"
            ));
        }
        let one = self.find_mut(id)?;
        if parent_of_map(one) == parent {
            return Ok(false);
        }
        match parent.is_empty() {
            // Removed rather than set to `""`: a root object is one with no
            // parent, and an empty string in the file is a fact nobody needs.
            true => {
                one.remove("parent");
            }
            false => {
                one.insert("parent".into(), json!(parent));
            }
        }
        Ok(true)
    }

    /// Add an object, answering its id.
    pub fn add(&mut self, kind: &str, name: &str, parent: &str) -> String {
        let id = self.fresh_id(name);
        let mut one = Map::new();
        one.insert("id".into(), json!(id));
        one.insert("name".into(), json!(name));
        one.insert("kind".into(), json!(kind));
        if !parent.is_empty() {
            one.insert("parent".into(), json!(parent));
        }
        one.insert("position".into(), json!([0.0, 0.0, 0.0]));
        one.insert("rotation".into(), json!([0.0, 0.0, 0.0]));
        one.insert("scale".into(), json!([1.0, 1.0, 1.0]));
        if let Some(list) = self.json.get_mut("objects").and_then(Value::as_array_mut) {
            list.push(Value::Object(one));
        }
        id
    }

    /// Remove an object and everything under it.
    ///
    /// The descendants go too, rather than being left parented to something
    /// that is gone. They would still be *shown* — `objects` treats an orphan
    /// as a root — but as a pile at the top of the outliner, which is not what
    /// deleting a rig looks like to the person who did it.
    pub fn remove(&mut self, id: &str) -> Result<usize, String> {
        if !self.raw().iter().any(|one| id_of(one) == id) {
            return Err(format!("there is no object {id:?}"));
        }
        let doomed: Vec<String> = self
            .objects()
            .into_iter()
            .skip_while(|one| one.id != id)
            .scan(None::<usize>, |root, one| {
                match *root {
                    None => *root = Some(one.depth),
                    // Back out to the depth it started at: the subtree is over.
                    Some(at) if one.depth <= at => return None,
                    Some(_) => {}
                }
                Some(one.id)
            })
            .collect();

        if let Some(list) = self.json.get_mut("objects").and_then(Value::as_array_mut) {
            list.retain(|one| {
                !one.as_object()
                    .map(|map| doomed.contains(&id_of(map)))
                    .unwrap_or(false)
            });
        }
        Ok(doomed.len())
    }

    /// The ids above an object, nearest first.
    fn ancestors(&self, from: &str) -> impl Iterator<Item = String> + '_ {
        let raw = self.raw();
        let mut at = from.to_string();
        // Bounded by the number of objects, so a cycle ends rather than spins.
        let mut left = raw.len();
        std::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            let parent = raw
                .iter()
                .find(|one| id_of(one) == at)
                .map(parent_of)
                .unwrap_or_default();
            if parent.is_empty() {
                return None;
            }
            at = parent.clone();
            Some(parent)
        })
    }

    fn raw(&self) -> Vec<&Map<String, Value>> {
        self.json
            .get("objects")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_object).collect())
            .unwrap_or_default()
    }

    fn find_mut(&mut self, id: &str) -> Result<&mut Map<String, Value>, String> {
        self.json
            .get_mut("objects")
            .and_then(Value::as_array_mut)
            .and_then(|list| {
                list.iter_mut()
                    .filter_map(Value::as_object_mut)
                    .find(|one| one.get("id").and_then(Value::as_str) == Some(id))
            })
            .ok_or_else(|| format!("there is no object {id:?}"))
    }

    /// An id nothing else is using, built from the name so a file is readable.
    fn fresh_id(&self, name: &str) -> String {
        let stem: String = name
            .chars()
            .map(|c| match c.is_ascii_alphanumeric() {
                true => c.to_ascii_lowercase(),
                false => '-',
            })
            .collect();
        let stem = stem.trim_matches('-').to_string();
        let stem = match stem.is_empty() {
            true => "object".to_string(),
            false => stem,
        };
        let taken: Vec<String> = self.raw().iter().map(|one| id_of(one)).collect();
        (1..)
            .map(|n| format!("{stem}-{n}"))
            .find(|id| !taken.contains(id))
            .expect("the integers do not run out")
    }
}

impl Default for Scene {
    fn default() -> Scene {
        Scene::new()
    }
}

pub use material::{GraphNode, Link, Material, Param, ParamValue};

/// Which part of a transform is being set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum Part {
    Position,
    Rotation,
    Scale,
}

impl Part {
    fn key(self) -> &'static str {
        match self {
            Part::Position => "position",
            Part::Rotation => "rotation",
            Part::Scale => "scale",
        }
    }
}

fn id_of(one: &Map<String, Value>) -> String {
    one.get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn parent_of(one: &&Map<String, Value>) -> String {
    parent_of_map(one)
}

fn parent_of_map(one: &Map<String, Value>) -> String {
    one.get("parent")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_of(one: &Map<String, Value>, key: &str, fallback: &str) -> String {
    one.get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

/// Three numbers, or the default when the key is missing or malformed.
///
/// Missing is ordinary — an object that has never been moved need not say so —
/// and malformed is somebody's hand edit, which is better shown at the origin
/// than refused at the door.
fn triple(one: &Map<String, Value>, key: &str, fallback: [f64; 3]) -> [f64; 3] {
    let Some(list) = one.get(key).and_then(Value::as_array) else {
        return fallback;
    };
    if list.len() != 3 {
        return fallback;
    }
    let mut out = fallback;
    for (slot, item) in out.iter_mut().zip(list) {
        match item.as_f64() {
            Some(n) if n.is_finite() => *slot = n,
            _ => return fallback,
        }
    }
    out
}
