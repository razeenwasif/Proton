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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use proton_mesh::{Mesh, Step};
use proton_scene::{Material, Modifier, Node, ParamValue, Part, Scene};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How many triangles one mesh may have before the viewport stops drawing it.
///
/// ## This is no longer about the frame
///
/// It used to be. A 2D canvas filled one path per triangle and the budget was
/// wherever that stopped holding thirty frames a second. The renderer is now
/// WebGL, and on this machine the frame time is *flat* — measured through
/// `gl.finish()`, so it is what the GPU really took and not what it took to
/// queue the work:
///
/// | triangles | frame |
/// |---|---|
/// |   100,000 | 0.3 ms |
/// |   250,000 | 0.3 ms |
/// |   500,000 | 0.2 ms |
/// | 1,000,000 | 0.3 ms |
///
/// A million triangles is not a number this viewport notices. The old budget
/// was 90,000 and it was the right number for the renderer that set it.
///
/// ## It is about the seam
///
/// What crosses is JSON, once per mesh — positions, normals and triangles.
/// Measured at 13,257 KB for a 270,400-triangle sphere, which is about 50 bytes
/// a triangle for an ordinary smooth indexed mesh; a faceted one splits more
/// vertices at its creases and costs more. So the budget is a *size*,
/// converted: eight megabytes is as much as is reasonable to serialise, hand
/// across an IPC boundary and parse for one object, and that is 160,000
/// triangles.
///
/// This was 250,000 until vertex normals arrived. Normals are three more
/// floats a vertex, which took the rate from 32 bytes a triangle to 50, and
/// the limit is the megabytes rather than the triangle count — so the count
/// came down rather than the line moving to keep a number that had stopped
/// meaning what it said.
///
/// The honest next move is not a larger number here either. It is to stop
/// sending geometry as JSON — a binary channel would make this limit several
/// times larger without changing anything a person would notice.
///
/// Past the budget the viewport draws the mesh's real bounding box and says
/// how many triangles it has, which is a smaller lie than a scene that stops
/// responding while several megabytes are parsed.
pub const BUDGET: usize = 160_000;

/// Geometry as the front end receives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Geometry {
    /// The path exactly as the scene writes it, which is how both ends key it.
    pub path: String,
    /// Absent when the file could not be read, and when it is over [`BUDGET`].
    pub mesh: Option<Mesh>,
    /// The box it fills, sent even when the mesh itself is not, so a viewport
    /// that cannot draw the surface can still draw the real extent instead of
    /// a unit cube pretending to be one.
    pub bounds: [f32; 6],
    /// How many triangles the file has, whether or not they are being sent.
    pub triangles: usize,
    /// Why there is no geometry, when there is a reason worth showing.
    pub problem: String,
}

/// One result already worked out, and what it depended on.
///
/// Both halves matter. The file can change under us — a re-export from
/// Blender — and so can the stack, without the key moving: a key is the mesh
/// and the object, not the settings, so turning a mirror's weld off has to be
/// noticed here rather than by the front end asking again.
#[derive(Debug, Clone)]
struct Cached {
    at: Option<SystemTime>,
    steps: Vec<Step>,
    /// The mesh path, kept so a re-check does not have to re-split the key.
    path: String,
    what: Geometry,
}

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
    /// Geometry already read, by the path the scene writes.
    ///
    /// Kept because a mesh is the one thing here big enough that re-reading it
    /// on every event would be felt, and because holding the file's modified
    /// time is what lets a re-export from Blender be noticed.
    meshes: HashMap<String, Cached>,
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

    /// Read one mesh, run its stack, or answer why neither happened.
    ///
    /// The key is a mesh path, optionally followed by `@` and an object's id
    /// when that object has a stack to run — see `Node::geometry`. Cached on
    /// the file's modified time *and* the steps, so a re-export from Blender
    /// and a change to the stack are both noticed.
    pub fn geometry(&mut self, key: &str) -> Geometry {
        let missing = |why: String| Geometry {
            path: key.to_string(),
            mesh: None,
            bounds: [0.0; 6],
            triangles: 0,
            problem: why,
        };

        // `chair.obj@chair-3` is that object's copy; `chair.obj` is the file
        // as it sits on disk. Split from the right, and only when the tail is
        // really an object, because nothing stops a folder being called `@`.
        let (path, steps) = match key.rsplit_once('@') {
            Some((path, id)) => match self.nodes().into_iter().find(|one| one.id == id) {
                Some(node) => (path, steps_of(&node.modifiers)),
                None => (key, Vec::new()),
            },
            None => (key, Vec::new()),
        };

        // Checked here as well as on the way in. `set_mesh` guards what this
        // editor writes; this guards what it reads, and the scene on disk may
        // have been written by anything at all.
        let clean = match proton_scene::mesh_path(path) {
            Ok(clean) if !clean.is_empty() => clean,
            Ok(_) => return missing(String::new()),
            Err(why) => return missing(why),
        };
        let Some(beside) = self.path.as_ref().and_then(|one| one.parent()) else {
            return missing("no scene is open".into());
        };
        let full = beside.join(&clean);
        let at = std::fs::metadata(&full).and_then(|one| one.modified()).ok();

        if let Some(had) = self.meshes.get(key) {
            if had.at == at && had.steps == steps {
                return had.what.clone();
            }
        }

        let what = match Mesh::read(&full) {
            Err(why) => missing(why),
            Ok(base) => {
                // How far a stack grows a mesh is exactly known before running
                // it — a mirror doubles, an array multiplies by its count, and
                // each level of subdivision is four times the last. So a stack
                // that would make ten million triangles is refused with the
                // number in the message, rather than by running out of memory
                // on the way to finding out.
                let would = base.count() as f64 * proton_mesh::growth(&steps);
                match would > BUDGET as f64 {
                    true => Geometry {
                        path: key.to_string(),
                        mesh: None,
                        // The base mesh's box, which is the honest thing to
                        // draw: where the stack would have put the result is
                        // not known without running it.
                        bounds: base.bounds,
                        triangles: would as usize,
                        problem: match steps.is_empty() {
                            true => format!(
                                "{} triangles — more than this viewport can draw",
                                base.count()
                            ),
                            false => format!(
                                "this stack would make {} triangles from {} — more \
                                 than this viewport can draw",
                                would as usize,
                                base.count()
                            ),
                        },
                    },
                    false => {
                        let skipped = base.skipped;
                        let mesh = match steps.is_empty() {
                            true => base,
                            false => base.apply(&steps),
                        }
                        // Normals last, after every step: any step before them
                        // would have made them wrong.
                        .shade(proton_mesh::CREASE);
                        Geometry {
                            path: key.to_string(),
                            bounds: mesh.bounds,
                            triangles: mesh.count(),
                            problem: match skipped {
                                0 => String::new(),
                                n => format!("{n} faces in this file could not be read"),
                            },
                            mesh: Some(mesh),
                        }
                    }
                }
            }
        };
        self.meshes.insert(
            key.to_string(),
            Cached {
                at,
                steps,
                path: path.to_string(),
                what: what.clone(),
            },
        );
        what
    }

    /// Every piece of geometry the scene needs, in outliner order and without
    /// repeats — by key, so two objects wearing the same mesh with the same
    /// stack ask for it once.
    pub fn wanted(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for node in self.nodes() {
            if !node.geometry.is_empty() && !out.contains(&node.geometry) {
                out.push(node.geometry);
            }
        }
        out
    }

    /// The meshes whose files have moved since they were read.
    ///
    /// Stat'd rather than watched: one `metadata` call per distinct mesh, on
    /// the occasions something else already re-read the scene. A file watcher
    /// would be a thread and a dependency to notice the same thing sooner than
    /// anybody needs it noticed.
    pub fn restale(&mut self) -> Vec<Geometry> {
        let Some(beside) = self.path.as_ref().and_then(|one| one.parent()) else {
            return Vec::new();
        };
        let beside = beside.to_path_buf();
        let moved: Vec<String> = self
            .meshes
            .iter()
            .filter(|(_, had)| {
                let Ok(clean) = proton_scene::mesh_path(&had.path) else {
                    return false;
                };
                let at = std::fs::metadata(beside.join(clean))
                    .and_then(|one| one.modified())
                    .ok();
                at != had.at
            })
            .map(|(key, _)| key.clone())
            .collect();
        moved
            .into_iter()
            .map(|key| {
                self.meshes.remove(&key);
                self.geometry(&key)
            })
            .collect()
    }

    /// Every material, compiled into a program a renderer can run.
    ///
    /// All of them, like the materials themselves and for the same reason: a
    /// program is a dozen instructions, the selection moves between them on
    /// every click, and sending one at a time would be a round trip each time.
    pub fn shaders(&self) -> Vec<proton_scene::Shader> {
        self.materials().iter().map(Material::compile).collect()
    }

    /// The stack of the node at a row, as operations to run.
    ///
    /// A modifier that is switched off, or of a kind this build has never
    /// heard of, produces no step — which is what makes an unknown one
    /// visibly *not applied* rather than silently ignored.
    pub fn steps(&self, id: &str) -> Vec<Step> {
        self.nodes()
            .into_iter()
            .find(|one| one.id == id)
            .map(|node| steps_of(&node.modifiers))
            .unwrap_or_default()
    }

    /// Change the stack of one object, and write the file.
    pub fn modify(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut Scene, &str) -> Result<bool, String>,
    ) -> Result<bool, String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no scene is open".into());
        };
        if !change(scene, id)? {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// Point an object at its geometry, and write the file.
    pub fn set_mesh(&mut self, id: &str, path: &str) -> Result<bool, String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no scene is open".into());
        };
        if !scene.set_mesh(id, path)? {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
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

/// Turn a stack into the operations to run, dropping what cannot be run.
fn steps_of(stack: &[Modifier]) -> Vec<Step> {
    stack
        .iter()
        .filter(|one| one.known && !one.off)
        .filter_map(|one| match one.kind.as_str() {
            "mirror" => Some(Step::Mirror {
                axis: match one.axis.as_str() {
                    "y" => 1,
                    "z" => 2,
                    _ => 0,
                },
                weld: one.weld,
            }),
            "array" => Some(Step::Array {
                count: one.count,
                offset: [
                    one.offset[0] as f32,
                    one.offset[1] as f32,
                    one.offset[2] as f32,
                ],
            }),
            "subdivide" => Some(Step::Subdivide { levels: one.levels }),
            // `known` said this build could apply it and this match says it
            // cannot, which would be a bug in one of the two.
            _ => None,
        })
        .collect()
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
