//! Modeling mode: the scene the outliner shows.
//!
//! `proton-scene` owns the file format. What lives here is the editor's half —
//! which scene is open, which node the outliner is on, and what happens to the
//! file when somebody renames or moves one.
//!
//! Deliberately the same shape as [`crate::design`], because it is the same
//! job: a document with a tree in it, a panel that lists the tree, and edits
//! that reach the disk without a save step. Where the two differ, the reason
//! is in the difference and not in the author's mood.
//!
//! ## A `.json` is not hijacked
//!
//! Opening a notebook switches to Data mode, because a notebook is not a thing
//! you usefully edit as text. A scene is — it is JSON designed to be read in a
//! diff — so Codex keeps being able to open one. Modeling is pointed at a
//! scene explicitly, or finds one when the mode is first entered.
//!
//! ## An edit reaches the disk
//!
//! The outliner is an editor of a file, not a view of one. There is no
//! separate save, for the reason the theme inspector has none: a scene with
//! unsaved changes is a tree that disagrees with the viewport drawing it.

use std::path::{Path, PathBuf};

use proton_scene::{Material, Node, ParamValue, Part, Scene};

/// The scene that is open, and where the outliner is in it.
#[derive(Debug, Default)]
pub struct Modeling {
    scene: Option<Scene>,
    path: Option<PathBuf>,
    /// Which node is selected. `usize::MAX` for none — opening a file should
    /// not assert that its first object is interesting.
    selected: usize,
    /// Why there is no scene, when there is a reason worth showing.
    problem: Option<String>,
}

impl Modeling {
    pub fn is_open(&self) -> bool {
        self.scene.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The tree, as the outliner draws it.
    pub fn nodes(&self) -> Vec<Node> {
        self.scene.as_ref().map(Scene::objects).unwrap_or_default()
    }

    /// The id of the selected node, if one is.
    pub fn selected_id(&self) -> Option<String> {
        self.nodes()
            .into_iter()
            .nth(self.selected)
            .map(|one| one.id)
    }

    /// Read a scene.
    ///
    /// A failure is remembered as well as returned: pointing Modeling at a
    /// `package.json` is an ordinary mistake, and the answer belongs in the
    /// panel where somebody is looking rather than only in a notice that
    /// scrolls away.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        match Scene::read(path) {
            Ok(scene) => {
                self.scene = Some(scene);
                self.path = Some(path.to_path_buf());
                self.problem = None;
                self.selected = usize::MAX;
                Ok(())
            }
            Err(why) => {
                // The old scene is dropped. Leaving it up would mean the
                // outliner showing one file while the header named another.
                self.scene = None;
                self.path = Some(path.to_path_buf());
                self.problem = Some(why.clone());
                self.selected = usize::MAX;
                Err(why)
            }
        }
    }

    /// Pick a node. Out of range clears the selection rather than clamping: a
    /// click on a tree that has changed underneath should select nothing, not
    /// something else.
    pub fn select(&mut self, index: usize) -> bool {
        let to = match index < self.nodes().len() {
            true => index,
            false => usize::MAX,
        };
        let moved = to != self.selected;
        self.selected = to;
        moved
    }

    /// Every material in the scene, with its graph.
    pub fn materials(&self) -> Vec<Material> {
        self.scene
            .as_ref()
            .map(Scene::materials)
            .unwrap_or_default()
    }

    pub fn set_param(
        &mut self,
        material: &str,
        node: &str,
        param: &str,
        value: ParamValue,
    ) -> Result<bool, String> {
        self.write(|scene| scene.set_param(material, node, param, value))
    }

    pub fn move_graph_node(
        &mut self,
        material: &str,
        node: &str,
        at: [f64; 2],
    ) -> Result<bool, String> {
        self.write(|scene| scene.move_node(material, node, at))
    }

    pub fn connect(&mut self, material: &str, from: &str, to: &str) -> Result<bool, String> {
        self.write(|scene| scene.connect(material, from, to))
    }

    pub fn rewire(&mut self, material: &str, from: &str, to: &str) -> Result<bool, String> {
        self.write(|scene| scene.rewire(material, from, to))
    }

    pub fn disconnect(&mut self, material: &str, to: &str) -> Result<bool, String> {
        self.write(|scene| scene.disconnect(material, to))
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<bool, String> {
        self.write(|scene| scene.rename(id, name))
    }

    pub fn set_transform(&mut self, id: &str, what: Part, to: [f64; 3]) -> Result<bool, String> {
        self.write(|scene| scene.set_transform(id, what, to))
    }

    pub fn reparent(&mut self, id: &str, parent: &str) -> Result<bool, String> {
        // The tree changes shape, so the index the outliner had means
        // something else now.
        let done = self.write(|scene| scene.reparent(id, parent))?;
        if done {
            self.follow(id);
        }
        Ok(done)
    }

    /// Add a node under the selection, answering its id.
    pub fn add(&mut self, kind: &str, name: &str, parent: &str) -> Result<String, String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no scene is open".into());
        };
        let id = scene.add(kind, name, parent);
        self.save()?;
        // Selected, because adding something and leaving the panel pointing at
        // whatever was there before is a click somebody has to make anyway.
        self.follow(&id);
        Ok(id)
    }

    /// Remove a node and everything under it, answering how many went.
    pub fn remove(&mut self, id: &str) -> Result<usize, String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no scene is open".into());
        };
        let gone = scene.remove(id)?;
        self.save()?;
        // What was selected may be one of the things just deleted, and an
        // index into a shorter list points at a different object.
        self.selected = usize::MAX;
        Ok(gone)
    }

    /// Re-read the file from disk.
    ///
    /// The other direction: a scene edited as text in Codex, or patched by the
    /// agent, has to reach the outliner rather than leaving it showing the
    /// tree as it was when the mode was opened.
    pub fn reload(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        match Scene::read(&path) {
            Ok(fresh) if self.scene.as_ref() != Some(&fresh) => {
                self.scene = Some(fresh);
                self.problem = None;
                // The tree changed under the selection, so the index means
                // something else. Cleared rather than kept.
                self.selected = usize::MAX;
                true
            }
            Ok(_) => false,
            Err(why) => {
                let changed = self.problem.as_deref() != Some(why.as_str());
                self.scene = None;
                self.problem = Some(why);
                self.selected = usize::MAX;
                changed
            }
        }
    }

    /// Put the selection on a node by id, wherever the tree moved it to.
    fn follow(&mut self, id: &str) {
        self.selected = self
            .nodes()
            .iter()
            .position(|one| one.id == id)
            .unwrap_or(usize::MAX);
    }

    fn write(
        &mut self,
        change: impl FnOnce(&mut Scene) -> Result<bool, String>,
    ) -> Result<bool, String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no scene is open".into());
        };
        if !change(scene)? {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    fn save(&self) -> Result<(), String> {
        let (Some(scene), Some(path)) = (&self.scene, &self.path) else {
            return Err("no scene is open".into());
        };
        scene.write(path)
    }
}

/// Look for a scene under the project.
///
/// Shallow and conventional, like [`crate::design::discover`]. The point is to
/// have something in the outliner the first time Modeling is opened, not to
/// guess right about somebody's project — and a walk deep enough to find any
/// `.json` would find every one a dependency shipped.
pub fn discover(root: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &["scene.proton.json", "scene.json"];
    const DIRS: &[&str] = &["", "scenes", "assets", "src", "models"];

    for dir in DIRS {
        for name in NAMES {
            let path = match dir.is_empty() {
                true => root.join(name),
                false => root.join(dir).join(name),
            };
            // Read rather than only stat'd: `scene.json` is a common enough
            // name that finding one proves nothing, and an outliner that opens
            // somebody's Three.js export and says it is broken is worse than
            // one that keeps looking.
            if path.is_file() && Scene::read(&path).is_ok() {
                return Some(path);
            }
        }
    }
    None
}
