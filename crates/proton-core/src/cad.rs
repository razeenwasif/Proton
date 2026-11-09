//! CAD mode: the `.part` open in this project, its edits held until ⌘S, and
//! what the window draws of it.
//!
//! `proton-cad` owns the language, the solver and the kernel. What lives
//! here is the editor's half: which part is open, its text as it is now and
//! as it was saved, the steps to undo, and the view the window draws — the
//! feature history with each step's mistake, each sketch's shapes,
//! constraints and dimensions where the solver put them, and each body as
//! triangles.
//!
//! The file's text is what is edited. The window's edits are lines of the
//! language itself — `line l3 l2.end (40, 10)`, `distance a b = 25`,
//! `extrude plate from base distance 3` — added, replaced or taken out, so
//! anything the mouse does is what typing it would have done, and the file
//! stays the one source; after each, every sketch is solved and written
//! back, so the file says where its shapes are. A drag moves a point with the solver holding the
//! rest; it is one step to undo when the mouse lets go. Typing into the
//! text is held there too, consecutive keystrokes one step.
//!
//! Edits are held until ⌘S, and the quit prompt names an unsaved part.
//! `generation` numbers each part opened, so an edit meant for the last is
//! never made to this.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use proton_cad::export::{stl, threemf};
use proton_cad::mass::mass;
use proton_cad::mesh::{tessellate, Mesh, Quality};
use proton_cad::model::{self, Model};
use proton_cad::part::ast::{Diag, EdgePick, Entity, Item, Noted, Part, Shape, Size, Sketch, SketchItem, Spot};
use proton_cad::part::{self, expr as pexpr, parse, print};
use proton_cad::sketch::{self as sketcher, Geom};
use proton_cad::surface::Surface;
use proton_cad::vec::Aabb;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::workspace::notice;

mod agent;
pub use agent::TOOLS;

/// A part's file ends so.
pub const EXTENSION: &str = "part";

/// A part or assembly found in the folder, for the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadFound {
    /// As the project names it.
    pub path: String,
    pub name: String,
    /// `part`, `assembly`, `drawing`, `schematic` or `board`.
    pub kind: String,
}

/// Something wrong, or not built yet, and where: lines and columns from 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadDiag {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

impl From<&Diag> for CadDiag {
    fn from(d: &Diag) -> CadDiag {
        CadDiag { line: d.line as u32, col: d.col as u32, message: d.message.clone() }
    }
}

/// A parameter: its expression as written, and what it comes to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadParam {
    pub index: u32,
    pub line: u32,
    pub name: String,
    pub value: String,
    pub now: Option<f64>,
}

/// A step of the feature history: a sketch or a feature, its fields as
/// written, and what went wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadStep {
    /// Its place among the part's items.
    pub index: u32,
    pub line: u32,
    /// `sketch`, `extrude`, `revolve`, or a later feature's keyword.
    pub kind: String,
    pub name: String,
    /// Its line as the file has it (a sketch's first line).
    pub text: String,
    pub error: Option<String>,
    /// Why it is not built: a feature of a later milestone.
    pub note: Option<String>,
    /// An extrude's or a revolve's sketch.
    pub sketch: Option<String>,
    pub distance: Option<String>,
    pub symmetric: bool,
    /// An extrude right through everything.
    pub through: bool,
    /// An extrude's or a revolve's `join`, `cut` or `intersect`, and the
    /// body it is made into; none for a body of its own.
    pub how: Option<String>,
    pub body: Option<String>,
    pub axis: Option<String>,
    pub angle: Option<String>,
    /// A sketch's plane, and its offset.
    pub on: Option<String>,
    pub offset: Option<String>,
    /// A fillet's or chamfer's edges, each as written: `a.top & a.side(r.left)`,
    /// `a.edges(parallel z)`.
    pub edges: Vec<String>,
    /// A fillet's radius, a chamfer's distance, a shell's thickness.
    pub value: Option<String>,
    /// A chamfer's second distance.
    pub other: Option<String>,
    /// What a sweep runs along, or the sketch a loft goes to.
    pub path: Option<String>,
    /// The faces a shell takes away.
    pub open: Vec<String>,
}

/// A face of a body: its name, its surface's kind, and for a flat one its
/// plane — origin, outward normal and x, nine numbers — to sketch on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadFace {
    pub name: String,
    pub surface: String,
    pub plane: Option<Vec<f64>>,
}

/// A body as triangles, for the 3D view: its points (x, y, z each) with the
/// exact normals of the face they are on, the triangles by point, each
/// triangle's face, and its edges as runs of points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadBody {
    pub name: String,
    pub line: u32,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub face_of: Vec<u32>,
    pub faces: Vec<CadFace>,
    pub edges: Vec<Vec<f32>>,
    /// Each edge's name to round or chamfer it by — `a.top & a.side(r.left)`,
    /// its two faces — or none for a seam or a smooth join, which is no
    /// corner; and whether it is `convex`, `concave`, or neither.
    pub edge_picks: Vec<Option<String>>,
    pub edge_turns: Vec<String>,
    /// Cubic millimetres, square millimetres, and where its centre of mass
    /// is, from the exact geometry.
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec<f64>,
}

/// A shape of a sketch where the solver put it, in the sketch's own units:
/// a point's `[p]`, a line's `[start, end]`, a circle's `[center]`, an
/// arc's `[center, start, end]`, a spline's control points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadShape {
    pub item: u32,
    pub line: u32,
    pub name: String,
    pub kind: String,
    pub construction: bool,
    pub pts: Vec<(f64, f64)>,
    pub radius: Option<f64>,
    /// Its line as the file writes it (a rectangle's, for each side).
    pub text: String,
}

/// A constraint or a dimension. A dimension written in a shape — a circle's
/// `diameter = hole`, a rectangle's `size = (w, h)` — is one too, `which`
/// saying which part of the shape holds it: `size`, `length`, `w` or `h`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadConstraint {
    pub item: u32,
    pub line: u32,
    pub kind: String,
    pub refs: Vec<String>,
    pub value: Option<String>,
    pub now: Option<f64>,
    pub which: Option<String>,
    pub redundant: bool,
    pub conflicting: bool,
    /// Its line as the file writes it.
    pub text: String,
}

/// A point of a sketch by one of its names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadPoint {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

/// A sketch as the window draws it, flat on its plane: its plane in the
/// world (origin, x, y and normal, twelve numbers, in millimetres) and the
/// millimetres in one of its units; its shapes, constraints and points;
/// and what the solver says of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadSketchView {
    pub index: u32,
    pub line: u32,
    pub name: String,
    pub on: String,
    pub frame: Vec<f64>,
    pub scale: f64,
    pub shapes: Vec<CadShape>,
    pub constraints: Vec<CadConstraint>,
    pub points: Vec<CadPoint>,
    pub dof: u32,
    pub solved: bool,
    pub free: Vec<String>,
    pub error: Option<CadDiag>,
    /// Why it has no profile to extrude, if it has none.
    pub profile: Option<String>,
    /// How many closed regions it bounds.
    pub regions: u32,
}

/// The part open: its text, what is wrong with it, and what it builds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadPartView {
    pub path: String,
    pub name: String,
    /// As its `part "…"` line names it.
    pub title: Option<String>,
    pub units: String,
    pub material: Option<String>,
    pub generation: u32,
    pub dirty: bool,
    pub text: String,
    /// The text as typed does not read: where, and why. What is shown is
    /// what it last built.
    pub error: Option<CadDiag>,
    pub params: Vec<CadParam>,
    pub steps: Vec<CadStep>,
    pub bodies: Vec<CadBody>,
    pub sketches: Vec<CadSketchView>,
    pub errors: Vec<CadDiag>,
    pub notes: Vec<CadDiag>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Everything built and drawn, as a box: low corner, then high.
    pub bounds: Vec<f64>,
}

/// An edit the window makes: lines of the language added, replaced or
/// taken out, by their place among the part's items (`index`) or among a
/// sketch's (`sketch`, then `index`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum CadOp {
    /// Statements — a sketch with its `end`, a feature, a parameter — put
    /// before item `at`, or at the end.
    Insert { at: Option<u32>, text: String },
    /// Item `index` replaced by the statement in `text`. For a sketch, its
    /// first line: its name and plane change, its shapes stay. A sketch or
    /// feature renamed is renamed where it is used.
    Replace { index: u32, text: String },
    Remove { index: u32 },
    /// Lines added to the end of the sketch at item `sketch`.
    SketchAdd { sketch: u32, text: String },
    SketchReplace { sketch: u32, index: u32, text: String },
    /// A sketch's items taken out, with the constraints on any shape taken
    /// out, and points drawn from its points left where they are.
    SketchRemove { sketch: u32, indices: Vec<u32> },
    /// A dimension's value: a constraint's, or the part of a shape `which`
    /// names (`size`, `length`, `w`, `h`).
    SetValue { sketch: u32, index: u32, which: Option<String>, value: String },
    /// A parameter set, or added if there is none of the name.
    Param { name: String, value: String },
}

/// What kind of file is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Doc {
    Part,
    Assembly,
    Drawing,
    Schematic,
    Board,
}

#[derive(Debug)]
struct Open {
    kind: Doc,
    /// The project it is in: an assembly reads its parts from there.
    root: PathBuf,
    /// An assembly's last fit check, until it changes.
    clashes: Option<Vec<crate::cad_assembly::CadClash>>,
    path: PathBuf,
    shown: String,
    name: String,
    text: String,
    saved: String,
    generation: u32,
    undo: Vec<String>,
    redo: Vec<String>,
    /// The last change was typed: the next typed joins its step.
    typing: bool,
    /// The text as a drag began, for its one step to undo.
    drag_from: Option<String>,
    /// What the text last built, shown while what is typed does not read.
    good: Option<CadPartView>,
    /// A drawing's sheet as last laid out, and the text it is of.
    drawn: Option<(String, Box<proton_cad::drawing::build::Built>)>,
}

/// The part open in CAD, in a project.
#[derive(Debug, Default)]
pub struct Cads {
    open: Option<Open>,
    generations: u32,
    parts: crate::cad_assembly::PartCache,
    /// What the agent's calls changed on screen, for the next poll.
    pending: Vec<Event>,
}

const START: &str = "units mm\n";

impl Cads {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn events(&mut self, root: &Path) -> Vec<Event> {
        let mut out = vec![Event::CadHome { parts: find(root) }, Event::CadFamilies { families: crate::cad_assembly::families() }];
        out.extend(self.doc_events());
        out
    }

    pub fn apply(&mut self, root: &Path, command: Command) -> Vec<Event> {
        match command {
            Command::CadNew { name } => self.new_doc(root, &name, Doc::Part),
            Command::CadNewAssembly { name } => self.new_doc(root, &name, Doc::Assembly),
            Command::CadNewSchematic { name } => self.new_doc(root, &name, Doc::Schematic),
            Command::CadLibrary { family, params, add } => {
                let path = match crate::cad_assembly::write_library_part(root, &family, &params) {
                    Ok(p) => p,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                let made = notice(format!("made {path}"), NoticeLevel::Info);
                let open_asm = self.open.as_ref().filter(|o| o.kind == Doc::Assembly).map(|o| (o.generation, o.shown.clone(), o.text.clone()));
                match (add, open_asm) {
                    (true, Some((generation, shown, text))) => {
                        let Ok(a) = proton_cad::assembly::read(&text) else { return vec![made, notice("the assembly's text does not read: mend it, then add the part".into(), NoticeLevel::Warn)] };
                        let name = crate::cad_assembly::instance_name(&a, &path);
                        let line = format!("part {name} from {}", quoted(&crate::cad_assembly::relative(&shown, &path)));
                        let mut out = vec![made, Event::CadHome { parts: find(root) }];
                        out.extend(self.apply(root, Command::CadEdit { generation, ops: vec![CadOp::Insert { at: None, text: line }] }));
                        out
                    }
                    _ => {
                        if self.dirty() {
                            return vec![made, Event::CadHome { parts: find(root) }];
                        }
                        let mut out = vec![made];
                        out.extend(self.apply(root, Command::CadOpen { path }));
                        out.push(Event::CadHome { parts: find(root) });
                        out
                    }
                }
            }
            Command::CadFit { generation } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation && o.kind == Doc::Assembly) else { return Vec::new() };
                let a = match proton_cad::assembly::read(&open.text) {
                    Ok(a) => a,
                    Err(d) => return vec![notice(format!("line {} does not read — mend it first: {}", d.line, d.message), NoticeLevel::Warn)],
                };
                let m = self.parts.build(&a, &open.path, &open.root);
                let found = crate::cad_assembly::clashes(&m);
                let said = match (found.iter().filter(|c| c.overlap).count(), found.iter().filter(|c| !c.overlap).count()) {
                    (0, 0) => "everything fits".to_string(),
                    (o, g) => format!("{o} overlapping, {g} nearer than the clearance"),
                };
                open.clashes = Some(found);
                let mut out = vec![notice(said, NoticeLevel::Info)];
                out.extend(self.doc_events());
                out
            }
            Command::CadOpen { path } => {
                let file = match inside(root, &path) {
                    Ok(f) => f,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                if self.open.as_ref().is_some_and(|o| o.path == file) {
                    return self.events(root);
                }
                if self.dirty() {
                    return vec![notice("save or discard the open part's changes before opening another".into(), NoticeLevel::Warn)];
                }
                match std::fs::read_to_string(&file) {
                    Ok(text) => {
                        let kind = match file.extension().and_then(|e| e.to_str()) {
                            Some(crate::cad_assembly::EXTENSION) => Doc::Assembly,
                            Some(crate::cad_drawing::EXTENSION) => Doc::Drawing,
                            Some(crate::cad_schematic::EXTENSION) => Doc::Schematic,
                            Some(crate::cad_board::EXTENSION) => Doc::Board,
                            _ => Doc::Part,
                        };
                        self.show(root, file, text, kind);
                        self.doc_events()
                    }
                    Err(e) => vec![notice(format!("{path}: {e}"), NoticeLevel::Warn)],
                }
            }
            Command::CadClose { discard } => {
                if !discard && self.dirty() {
                    return vec![notice("the part has unsaved changes".into(), NoticeLevel::Warn)];
                }
                self.open = None;
                self.events(root)
            }
            Command::CadSave => {
                if let Some(why) = self.save_unsaved().into_iter().next() {
                    return vec![notice(why, NoticeLevel::Warn)];
                }
                self.events(root)
            }
            Command::CadText { generation, text } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                if open.text == text {
                    return Vec::new();
                }
                if !open.typing {
                    open.undo.push(open.text.clone());
                    open.redo.clear();
                }
                open.clashes = None;
                open.text = text;
                open.typing = true;
                self.doc_events()
            }
            Command::CadDraw { generation, blueprint } => self.draw(root, generation, blueprint),
            Command::CadMakeBoard { generation } => self.make_board(root, generation),
            Command::CadEdit { generation, ops } if self.open.as_ref().is_some_and(|o| o.kind == Doc::Board) => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                let mut b = match proton_board::board_lang::read(&open.text) {
                    Ok(b) => b,
                    Err(e) => return vec![notice(format!("line {} does not read — mend it first: {}", e.line, e.message), NoticeLevel::Warn)],
                };
                for op in ops {
                    if let Err(why) = crate::cad_board::edit(&mut b, op) {
                        let mut out = vec![notice(why, NoticeLevel::Warn)];
                        out.extend(self.doc_events());
                        return out;
                    }
                }
                let text = proton_board::board_lang::write(&b);
                record(open, text);
                self.doc_events()
            }
            Command::CadEdit { generation, ops } if self.open.as_ref().is_some_and(|o| o.kind == Doc::Schematic) => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                let mut s = match proton_board::read(&open.text) {
                    Ok(s) => s,
                    Err(e) => return vec![notice(format!("line {} does not read — mend it first: {}", e.line, e.message), NoticeLevel::Warn)],
                };
                for op in ops {
                    if let Err(why) = crate::cad_schematic::edit(&mut s, op) {
                        let mut out = vec![notice(why, NoticeLevel::Warn)];
                        out.extend(self.doc_events());
                        return out;
                    }
                }
                let text = proton_board::write(&s);
                record(open, text);
                self.doc_events()
            }
            Command::CadEdit { generation, ops } if self.open.as_ref().is_some_and(|o| o.kind == Doc::Drawing) => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                let mut d = match proton_cad::drawing::lang::read(&open.text) {
                    Ok(d) => d,
                    Err(e) => return vec![notice(format!("line {} does not read — mend it first: {}", e.line, e.message), NoticeLevel::Warn)],
                };
                for op in ops {
                    if let Err(why) = crate::cad_drawing::edit(&mut d, op) {
                        let mut out = vec![notice(why, NoticeLevel::Warn)];
                        out.extend(self.doc_events());
                        return out;
                    }
                }
                let text = proton_cad::drawing::lang::write(&d);
                record(open, text);
                self.doc_events()
            }
            Command::CadEdit { generation, ops } if self.open.as_ref().is_some_and(|o| o.kind == Doc::Assembly) => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                let mut a = match proton_cad::assembly::read(&open.text) {
                    Ok(a) => a,
                    Err(d) => return vec![notice(format!("line {} does not read — mend it first: {}", d.line, d.message), NoticeLevel::Warn)],
                };
                for op in ops {
                    if let Err(why) = crate::cad_assembly::edit(&mut a, op) {
                        let mut out = vec![notice(why, NoticeLevel::Warn)];
                        out.extend(self.doc_events());
                        return out;
                    }
                }
                // Solved, and every placement the solver moved written back.
                let m = self.parts.build(&a, &open.path, &open.root);
                proton_cad::assembly::write_back(&mut a, &m);
                let text = proton_cad::assembly::write(&a);
                open.clashes = None;
                record(open, text);
                self.doc_events()
            }
            Command::CadEdit { generation, ops } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                let mut p = match part::read(&open.text) {
                    Ok(p) => p,
                    Err(d) => return vec![notice(format!("line {} does not read — mend it first: {}", d.line, d.message), NoticeLevel::Warn)],
                };
                for op in ops {
                    if let Err(why) = edit(&mut p, op) {
                        let mut out = vec![notice(why, NoticeLevel::Warn)];
                        out.extend(self.doc_events());
                        return out;
                    }
                }
                solve_all(&mut p);
                let text = part::write(&p);
                record(open, text);
                self.doc_events()
            }
            Command::CadDrag { generation, sketch, point, x, y, done } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation && o.kind == Doc::Part) else { return Vec::new() };
                let Ok(mut p) = part::read(&open.text) else { return Vec::new() };
                let (env, _) = pexpr::params(&p);
                let Some(sk) = p.items.iter_mut().find_map(|n| match &mut n.item {
                    Item::Sketch(s) if s.name == sketch => Some(s),
                    _ => None,
                }) else {
                    return Vec::new();
                };
                let moved = sketcher::drag(sk, &env, &point, (x, y)).unwrap_or(false);
                if moved {
                    let text = part::write(&p);
                    if open.drag_from.is_none() {
                        open.drag_from = Some(open.text.clone());
                    }
                    open.text = text;
                    open.typing = false;
                }
                if done {
                    if let Some(from) = open.drag_from.take() {
                        if from != open.text {
                            open.undo.push(from);
                            open.redo.clear();
                        }
                    }
                }
                if moved || done {
                    self.doc_events()
                } else {
                    Vec::new()
                }
            }
            Command::CadUndo { generation } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                let Some(back) = open.undo.pop() else { return Vec::new() };
                open.clashes = None;
                open.redo.push(std::mem::replace(&mut open.text, back));
                open.typing = false;
                self.doc_events()
            }
            Command::CadRedo { generation } => {
                let Some(open) = self.open.as_mut().filter(|o| o.generation == generation) else { return Vec::new() };
                let Some(fwd) = open.redo.pop() else { return Vec::new() };
                open.clashes = None;
                open.undo.push(std::mem::replace(&mut open.text, fwd));
                open.typing = false;
                self.doc_events()
            }
            Command::CadExport { generation, format } => {
                let Some(open) = self.open.as_ref().filter(|o| o.generation == generation) else { return Vec::new() };
                let done = match open.kind {
                    Doc::Assembly => export_assembly(open, &mut self.parts, &format),
                    Doc::Drawing => match proton_cad::drawing::lang::read(&open.text) {
                        Ok(d) => {
                            let built = self.parts.drawing(&d, &open.path, &open.root);
                            crate::cad_drawing::export(&built, &open.path, &open.shown, &format)
                        }
                        Err(e) => Err(format!("line {} does not read — mend it first: {}", e.line, e.message)),
                    },
                    Doc::Schematic => match proton_board::read(&open.text) {
                        Ok(s) => crate::cad_schematic::export(&proton_board::build(&s), &open.path, &open.shown, &format),
                        Err(e) => Err(format!("line {} does not read — mend it first: {}", e.line, e.message)),
                    },
                    Doc::Board => match proton_board::board_lang::read(&open.text) {
                        Ok(b) => {
                            let load = crate::cad_assembly::load_inside(&open.root);
                            let built = proton_board::board::build(&b, &open.path, &load);
                            let dir = open.path.parent().unwrap_or(Path::new("")).to_path_buf();
                            let sch = built.schematic.as_ref().and_then(|f| load(&dir.join(f)).ok()).and_then(|t| proton_board::read(&t).ok()).map(|s| proton_board::build(&s));
                            match sch {
                                Some(sch) => crate::cad_board::export(&built, &sch, &open.path, &open.shown, &format),
                                None => Err("its schematic does not read: mend it first".into()),
                            }
                        }
                        Err(e) => Err(format!("line {} does not read — mend it first: {}", e.line, e.message)),
                    },
                    Doc::Part => export(open, &format),
                };
                match done {
                    Ok(written) => vec![notice(format!("wrote {written}"), NoticeLevel::Info), Event::CadHome { parts: find(root) }],
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            _ => Vec::new(),
        }
    }

    fn new_doc(&mut self, root: &Path, name: &str, kind: Doc) -> Vec<Event> {
        let name = name.trim();
        let what = match kind {
            Doc::Part => "part",
            Doc::Assembly => "assembly",
            Doc::Schematic => "schematic",
            Doc::Drawing => "drawing",
            Doc::Board => "board",
        };
        if name.is_empty() {
            return vec![notice(format!("a new {what} needs a name"), NoticeLevel::Warn)];
        }
        if self.dirty() {
            return vec![notice("save or discard the open part's changes before making another".into(), NoticeLevel::Warn)];
        }
        let ext = match kind {
            Doc::Part => EXTENSION,
            Doc::Assembly => crate::cad_assembly::EXTENSION,
            Doc::Schematic => crate::cad_schematic::EXTENSION,
            Doc::Drawing => crate::cad_drawing::EXTENSION,
            Doc::Board => crate::cad_board::EXTENSION,
        };
        let rel = format!("{}.{}", slug(name), ext);
        let file = match inside(root, &rel) {
            Ok(f) => f,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        if file.exists() {
            return vec![notice(format!("{rel} is already there"), NoticeLevel::Warn)];
        }
        let start = if kind == Doc::Schematic { format!("schematic {}\nsheet A4\n", quoted(name)) } else { format!("{what} {}\n{START}", quoted(name)) };
        if let Err(e) = std::fs::write(&file, &start) {
            return vec![notice(format!("{rel}: {e}"), NoticeLevel::Warn)];
        }
        self.show(root, file, start, kind);
        let mut events = self.events(root);
        events.push(notice(format!("made {rel}"), NoticeLevel::Info));
        events
    }

    fn show(&mut self, root: &Path, path: PathBuf, text: String, kind: Doc) {
        self.generations += 1;
        let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let rel = path.strip_prefix(root).or_else(|_| path.strip_prefix(&real));
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.open = Some(Open {
            kind,
            root: root.to_path_buf(),
            clashes: None,
            shown: rel.map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.display().to_string()),
            path,
            name,
            saved: text.clone(),
            text,
            generation: self.generations,
            undo: Vec::new(),
            redo: Vec::new(),
            typing: false,
            drag_from: None,
            good: None,
            drawn: None,
        });
    }

    fn dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.text != o.saved)
    }

    /// The part with unsaved changes, as the project names it.
    pub fn unsaved(&self) -> Vec<String> {
        match &self.open {
            Some(o) if o.text != o.saved => vec![o.shown.clone()],
            _ => Vec::new(),
        }
    }

    /// Save it, as ⌘S and quitting do; what could not be, and why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let Some(open) = &mut self.open else { return Vec::new() };
        if open.text == open.saved {
            return Vec::new();
        }
        match std::fs::write(&open.path, &open.text) {
            Ok(()) => {
                open.saved = open.text.clone();
                Vec::new()
            }
            Err(e) => vec![format!("{}: {e}", open.shown)],
        }
    }

    /// What is open, as the window draws it: the part, or the assembly —
    /// the other said to be none.
    pub fn doc_events(&mut self) -> Vec<Event> {
        vec![self.part_event(), self.assembly_event(), self.drawing_event(), self.schematic_event(), self.board_event()]
    }

    /// The board on screen, built from its schematic, checked and drawn.
    pub fn board_event(&mut self) -> Event {
        let Some(open) = self.open.as_ref().filter(|o| o.kind == Doc::Board) else { return Event::CadBoard { board: None } };
        let (b, error) = match proton_board::board_lang::read(&open.text) {
            Ok(b) => (b, None),
            Err(e) => (proton_board::board_lang::read(&open.saved).unwrap_or_default(), Some(CadDiag::from(&e))),
        };
        let built = proton_board::board::build(&b, &open.path, &crate::cad_assembly::load_inside(&open.root));
        let mut v = crate::cad_board::view(&b, &built, &open.shown, &open.name, open.generation);
        v.text = open.text.clone();
        v.dirty = open.text != open.saved;
        v.can_undo = !open.undo.is_empty();
        v.can_redo = !open.redo.is_empty();
        v.error = error;
        Event::CadBoard { board: Some(Box::new(v)) }
    }

    /// A first board for the schematic open: its parts set out on a board
    /// sized to them, written beside it as `NAME.board` and opened; one
    /// there already is opened.
    fn make_board(&mut self, root: &Path, generation: u32) -> Vec<Event> {
        let Some(open) = self.open.as_ref().filter(|o| o.generation == generation && o.kind == Doc::Schematic) else { return Vec::new() };
        if open.text != open.saved {
            return vec![notice("save it first: its board reads it from its file".into(), NoticeLevel::Warn)];
        }
        let file = open.path.with_extension(crate::cad_board::EXTENSION);
        let rel = Path::new(&open.shown).with_extension(crate::cad_board::EXTENSION).to_string_lossy().replace('\\', "/");
        let (text, said) = if file.exists() {
            match std::fs::read_to_string(&file) {
                Ok(t) => (t, format!("opened {rel}, which was there")),
                Err(e) => return vec![notice(format!("{rel}: {e}"), NoticeLevel::Warn)],
            }
        } else {
            let s = match proton_board::read(&open.text) {
                Ok(s) => s,
                Err(e) => return vec![notice(format!("line {} does not read — mend it first: {}", e.line, e.message), NoticeLevel::Warn)],
            };
            let b = proton_board::build(&s);
            let of = open.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let text = proton_board::board::first_board(b.title.as_deref().unwrap_or(&open.name), &of, &b);
            if let Err(e) = std::fs::write(&file, &text) {
                return vec![notice(format!("{rel}: {e}"), NoticeLevel::Warn)];
            }
            (text, format!("made {rel}"))
        };
        self.show(root, file, text, Doc::Board);
        let mut out = vec![notice(said, NoticeLevel::Info), Event::CadHome { parts: find(root) }];
        out.extend(self.doc_events());
        out
    }

    /// The schematic on screen, built, checked and drawn.
    pub fn schematic_event(&mut self) -> Event {
        let Some(open) = self.open.as_ref().filter(|o| o.kind == Doc::Schematic) else { return Event::CadSchematic { schematic: None } };
        let (s, error) = match proton_board::read(&open.text) {
            Ok(s) => (s, None),
            Err(e) => (proton_board::read(&open.saved).unwrap_or_default(), Some(CadDiag::from(&e))),
        };
        let b = proton_board::build(&s);
        let mut v = crate::cad_schematic::view(&s, &b, &open.shown, &open.name, open.generation);
        v.text = open.text.clone();
        v.dirty = open.text != open.saved;
        v.can_undo = !open.undo.is_empty();
        v.can_redo = !open.redo.is_empty();
        v.error = error;
        Event::CadSchematic { schematic: Some(Box::new(v)) }
    }

    /// The drawing on screen, laid out on its sheet.
    pub fn drawing_event(&mut self) -> Event {
        let Some(open) = self.open.as_mut().filter(|o| o.kind == Doc::Drawing) else { return Event::CadDrawing { drawing: None } };
        let (text, error) = match proton_cad::drawing::lang::read(&open.text) {
            Ok(_) => (open.text.clone(), None),
            // What last read, with where the text stops reading.
            Err(e) => (open.drawn.as_ref().map(|d| d.0.clone()).unwrap_or_else(|| open.saved.clone()), Some(CadDiag::from(&e))),
        };
        let d = proton_cad::drawing::lang::read(&text).unwrap_or_default();
        if open.drawn.as_ref().is_none_or(|x| x.0 != text) {
            let built = self.parts.drawing(&d, &open.path, &open.root);
            open.drawn = Some((text, Box::new(built)));
        }
        let built = &open.drawn.as_ref().expect("laid out").1;
        let mut v = crate::cad_drawing::view(&d, built, &open.shown, &open.name, open.generation);
        v.text = open.text.clone();
        v.dirty = open.text != open.saved;
        v.can_undo = !open.undo.is_empty();
        v.can_redo = !open.redo.is_empty();
        v.error = error;
        Event::CadDrawing { drawing: Some(Box::new(v)) }
    }

    /// A drawing of the part or assembly open — its views, or for an
    /// assembly its blueprint — written beside it and opened; one there
    /// already is opened as it is.
    fn draw(&mut self, root: &Path, generation: u32, blue: bool) -> Vec<Event> {
        let Some(open) = self.open.as_ref().filter(|o| o.generation == generation && matches!(o.kind, Doc::Part | Doc::Assembly)) else { return Vec::new() };
        if open.text != open.saved {
            return vec![notice("save it first: its drawing reads it from its file".into(), NoticeLevel::Warn)];
        }
        let file = open.path.with_extension(crate::cad_drawing::EXTENSION);
        let of = open.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let rel = Path::new(&open.shown).with_extension(crate::cad_drawing::EXTENSION).to_string_lossy().replace('\\', "/");
        if !file.exists() {
            let text = match open.kind {
                Doc::Part => {
                    let p = match part::read(&open.text) {
                        Ok(p) => p,
                        Err(d) => return vec![notice(format!("line {} does not read — mend it first: {}", d.line, d.message), NoticeLevel::Warn)],
                    };
                    let title = p.items.iter().find_map(|n| if let Item::Name(t) = &n.item { Some(t.clone()) } else { None }).unwrap_or_else(|| open.name.clone());
                    let m = model::build(&p);
                    let mut b = Aabb::empty();
                    for body in &m.bodies {
                        let x = body.solid.bounds();
                        if !x.is_empty() {
                            b.include(x.min);
                            b.include(x.max);
                        }
                    }
                    if b.is_empty() {
                        return vec![notice("there is nothing built to draw yet".into(), NoticeLevel::Warn)];
                    }
                    crate::cad_drawing::for_part(&of, &title, &b, blue)
                }
                _ => {
                    let a = match proton_cad::assembly::read(&open.text) {
                        Ok(a) => a,
                        Err(d) => return vec![notice(format!("line {} does not read — mend it first: {}", d.line, d.message), NoticeLevel::Warn)],
                    };
                    let title = a.items.iter().find_map(|n| if let proton_cad::assembly::lang::Item::Name(t) = &n.item { Some(t.clone()) } else { None }).unwrap_or_else(|| open.name.clone());
                    let m = self.parts.build(&a, &open.path, &open.root);
                    crate::cad_drawing::for_assembly(&of, &title, &m, blue)
                }
            };
            if let Err(e) = std::fs::write(&file, &text) {
                return vec![notice(format!("{rel}: {e}"), NoticeLevel::Warn)];
            }
            self.show(root, file, text, Doc::Drawing);
            let mut out = vec![notice(format!("made {rel}"), NoticeLevel::Info), Event::CadHome { parts: find(root) }];
            out.extend(self.doc_events());
            return out;
        }
        match std::fs::read_to_string(&file) {
            Ok(text) => {
                self.show(root, file, text, Doc::Drawing);
                let mut out = vec![notice(format!("opened {rel}, which was there"), NoticeLevel::Info)];
                out.extend(self.doc_events());
                out
            }
            Err(e) => vec![notice(format!("{rel}: {e}"), NoticeLevel::Warn)],
        }
    }

    /// The assembly on screen, solved and drawn.
    pub fn assembly_event(&mut self) -> Event {
        let Some(open) = self.open.as_mut().filter(|o| o.kind == Doc::Assembly) else { return Event::CadAssembly { assembly: None } };
        let (a, error) = match proton_cad::assembly::read(&open.text) {
            Ok(a) => (a, None),
            Err(d) => (proton_cad::assembly::read(&open.saved).unwrap_or(proton_cad::assembly::Assembly { items: Vec::new() }), Some(CadDiag::from(&d))),
        };
        let m = self.parts.build(&a, &open.path, &open.root);
        let mut v = crate::cad_assembly::view(&a, &m, &open.shown, &open.name, open.generation, open.clashes.clone());
        v.text = open.text.clone();
        v.dirty = open.text != open.saved;
        v.can_undo = !open.undo.is_empty();
        v.can_redo = !open.redo.is_empty();
        v.error = error;
        Event::CadAssembly { assembly: Some(Box::new(v)) }
    }

    /// The part on screen, built and drawn.
    pub fn part_event(&mut self) -> Event {
        let Some(open) = self.open.as_mut().filter(|o| o.kind == Doc::Part) else { return Event::CadPart { part: None } };
        let view = match part::read(&open.text) {
            Ok(p) => {
                let v = view_of(open, &p);
                open.good = Some(v.clone());
                v
            }
            Err(d) => {
                // What it last built, with where the text stops reading.
                let mut v = open.good.clone().unwrap_or_else(|| view_of(open, &Part::default()));
                v.error = Some(CadDiag::from(&d));
                v
            }
        };
        let view = CadPartView { text: open.text.clone(), dirty: open.text != open.saved, can_undo: !open.undo.is_empty(), can_redo: !open.redo.is_empty(), ..view };
        Event::CadPart { part: Some(Box::new(view)) }
    }
}

/// Every sketch solved and written back where it solves, so the file says
/// where the shapes are drawn.
fn solve_all(p: &mut Part) {
    let (env, _) = pexpr::params(p);
    for n in &mut p.items {
        if let Item::Sketch(s) = &mut n.item {
            let _ = sketcher::solve(s, &env);
        }
    }
}

/// A change made as one step to undo.
fn record(open: &mut Open, text: String) {
    if text == open.text {
        return;
    }
    open.undo.push(std::mem::replace(&mut open.text, text));
    open.redo.clear();
    open.typing = false;
}

/* ── edits ────────────────────────────────────────────────────────── */

fn sketch_at(p: &mut Part, i: u32) -> Result<&mut Sketch, String> {
    match p.items.get_mut(i as usize).map(|n| &mut n.item) {
        Some(Item::Sketch(s)) => Ok(s),
        _ => Err(format!("item {i} is not a sketch")),
    }
}

fn item_name(i: &Item) -> Option<&str> {
    match i {
        Item::Sketch(s) => Some(&s.name),
        Item::Extrude(e) => Some(&e.name),
        Item::Revolve(r) => Some(&r.name),
        Item::Hole(h) => Some(&h.name),
        Item::Pattern(p) => Some(&p.name),
        Item::Mirror(m) => Some(&m.name),
        Item::Fillet(f) => Some(&f.name),
        Item::Chamfer(c) => Some(&c.name),
        Item::Shell(sh) => Some(&sh.name),
        Item::Sweep(sw) => Some(&sw.name),
        Item::Loft(l) => Some(&l.name),
        _ => None,
    }
}

/// `old` renamed `new` wherever the part uses it: a feature's sketch or
/// body, a sketch's or a hole's face, what a pattern or mirror repeats.
fn rename(p: &mut Part, old: &str, new: &str) {
    let face = |r: &mut String| {
        if r == old {
            *r = new.to_string();
        } else if let Some(rest) = r.strip_prefix(&format!("{old}.")) {
            *r = format!("{new}.{rest}");
        }
    };
    let body = |b: &mut Option<(proton_cad::part::ast::Combine, String)>| {
        if let Some((_, x)) = b {
            if x == old {
                *x = new.to_string();
            }
        }
    };
    let list = |l: &mut Vec<String>| l.iter_mut().filter(|x| *x == old).for_each(|x| *x = new.to_string());
    for n in &mut p.items {
        match &mut n.item {
            Item::Extrude(e) => {
                if e.sketch == old {
                    e.sketch = new.to_string();
                }
                body(&mut e.into);
            }
            Item::Revolve(r) => {
                if r.sketch == old {
                    r.sketch = new.to_string();
                }
                body(&mut r.into);
            }
            Item::Sketch(s) => face(&mut s.on.reference),
            Item::Hole(h) => face(&mut h.face),
            Item::Pattern(pt) => list(&mut pt.of),
            Item::Mirror(m) => {
                list(&mut m.of);
                face(&mut m.plane);
            }
            Item::Combined(c) => {
                for x in [&mut c.body, &mut c.with] {
                    if x == old {
                        *x = new.to_string();
                    }
                }
            }
            Item::Fillet(proton_cad::part::ast::Fillet { edges, .. }) | Item::Chamfer(proton_cad::part::ast::Chamfer { edges, .. }) => {
                for e in edges {
                    match e {
                        EdgePick::Between(a, b) => {
                            face(a);
                            face(b);
                        }
                        EdgePick::Of { of, .. } => face(of),
                    }
                }
            }
            Item::Shell(sh) => {
                if sh.body == old {
                    sh.body = new.to_string();
                }
                sh.open.iter_mut().for_each(face);
            }
            Item::Sweep(sw) => {
                for x in [&mut sw.sketch, &mut sw.path] {
                    if x == old {
                        *x = new.to_string();
                    }
                }
                body(&mut sw.into);
            }
            Item::Loft(l) => {
                for x in [&mut l.from, &mut l.to] {
                    if x == old {
                        *x = new.to_string();
                    }
                }
                body(&mut l.into);
            }
            _ => {}
        }
    }
}

fn edit(p: &mut Part, op: CadOp) -> Result<(), String> {
    let bad = |d: Diag| format!("line {} of the edit: {}", d.line, d.message);
    match op {
        CadOp::Insert { at, text } => {
            let items = part::read(&text).map_err(bad)?.items;
            let at = at.map_or(p.items.len(), |a| (a as usize).min(p.items.len()));
            p.items.splice(at..at, items);
        }
        CadOp::Replace { index, text } => {
            let i = index as usize;
            let Some(old) = p.items.get(i).map(|n| n.item.clone()) else { return Err(format!("there is no item {index}")) };
            let new = if let Item::Sketch(s) = &old {
                // A sketch's first line: its shapes kept.
                let read = part::read(&format!("{text}\nend")).map_err(bad)?;
                match read.items.into_iter().next().map(|n| n.item) {
                    Some(Item::Sketch(head)) => Item::Sketch(Sketch { items: s.items.clone(), end_note: s.end_note.clone(), ..head }),
                    _ => return Err("a sketch's line is replaced by a sketch's line".into()),
                }
            } else {
                let mut read = part::read(&text).map_err(bad)?.items;
                if read.len() != 1 {
                    return Err("one statement replaces one".into());
                }
                read.remove(0).item
            };
            if let (Some(a), Some(b)) = (item_name(&old), item_name(&new)) {
                if a != b {
                    let (a, b) = (a.to_string(), b.to_string());
                    rename(p, &a, &b);
                }
            }
            p.items[i].item = new;
        }
        CadOp::Remove { index } => {
            if (index as usize) >= p.items.len() {
                return Err(format!("there is no item {index}"));
            }
            p.items.remove(index as usize);
        }
        CadOp::SketchAdd { sketch, text } => {
            let lines = parse::sketch_lines(&text).map_err(bad)?;
            sketch_at(p, sketch)?.items.extend(lines);
        }
        CadOp::SketchReplace { sketch, index, text } => {
            let mut lines = parse::sketch_lines(&text).map_err(bad)?;
            if lines.len() != 1 {
                return Err("one line replaces one".into());
            }
            let s = sketch_at(p, sketch)?;
            let slot = s.items.get_mut(index as usize).ok_or(format!("the sketch has no item {index}"))?;
            let line = lines.remove(0);
            *slot = Noted { line: slot.line, ..line };
        }
        CadOp::SketchRemove { sketch, indices } => {
            let (env, _) = pexpr::params(p);
            let s = sketch_at(p, sketch)?;
            remove_from_sketch(s, &env, &indices);
        }
        CadOp::SetValue { sketch, index, which, value } => {
            let e = parse::expr(&value).map_err(|d| format!("`{value}` is not a value: {}", d.message))?;
            let s = sketch_at(p, sketch)?;
            let item = s.items.get_mut(index as usize).ok_or(format!("the sketch has no item {index}"))?;
            match (&mut item.item, which.as_deref()) {
                (SketchItem::Constraint(c), None) if c.kind.valued() => c.value = Some(e),
                (SketchItem::Entity(Entity { shape: Shape::Line { length, .. }, .. }), Some("length")) => *length = Some(e),
                (SketchItem::Entity(Entity { shape: Shape::Circle { size, .. } | Shape::Arc { size, .. }, .. }), Some("size")) => {
                    *size = Some(match size.take() {
                        Some(Size::Diameter(_)) => Size::Diameter(e),
                        _ => Size::Radius(e),
                    })
                }
                (SketchItem::Entity(Entity { shape: Shape::Rect { dims: Some((w, h)), .. }, .. }), Some(k @ ("w" | "h"))) => {
                    if k == "w" {
                        *w = e;
                    } else {
                        *h = e;
                    }
                }
                _ => return Err("that has no value to set".into()),
            }
        }
        CadOp::Param { name, value } => {
            let e = parse::expr(&value).map_err(|d| format!("`{value}` is not a value: {}", d.message))?;
            match p.items.iter_mut().find_map(|n| match &mut n.item {
                Item::Param { name: n2, value } if *n2 == name => Some(value),
                _ => None,
            }) {
                Some(v) => *v = e,
                None => {
                    // After the last parameter, or the head.
                    let at = p.items.iter().rposition(|n| matches!(n.item, Item::Param { .. } | Item::Name(_) | Item::Units(_) | Item::Material(_))).map_or(0, |i| i + 1);
                    p.items.insert(at, Noted::new(Item::Param { name, value: e }));
                }
            }
        }
    }
    Ok(())
}

/// A sketch's items taken out. Constraints on what goes go with it; a
/// shape drawn from a point that goes keeps the point where it is, as a
/// number of its own.
fn remove_from_sketch(s: &mut Sketch, env: &pexpr::Env, indices: &[u32]) {
    let gone: HashSet<usize> = indices.iter().map(|&i| i as usize).collect();
    let mut names: Vec<String> = Vec::new();
    for &i in &gone {
        if let Some(SketchItem::Entity(e)) = s.items.get(i).map(|n| &n.item) {
            names.push(e.name.clone());
        }
    }
    let dead = |r: &str| names.iter().any(|n| r == n || r.starts_with(&format!("{n}.")));
    // Where every point is now, for those drawn from a point that goes.
    let at: Vec<(String, (f64, f64))> = sketcher::build(s, env).map(|b| {
        let (_, x) = b.solve();
        b.named_points(&x)
    }).unwrap_or_default();
    let place = |r: &str| at.iter().find(|(n, _)| n == r).map(|(_, p)| *p).unwrap_or((0.0, 0.0));
    let mut kept = Vec::new();
    for (i, mut n) in std::mem::take(&mut s.items).into_iter().enumerate() {
        if gone.contains(&i) {
            continue;
        }
        match &mut n.item {
            SketchItem::Constraint(c) if c.refs.iter().any(|r| dead(r)) => continue,
            SketchItem::Entity(e) => {
                let detach = |spot: &mut Spot| {
                    if let Spot::Ref(r) = spot {
                        if dead(r) {
                            let (x, y) = place(r);
                            *spot = Spot::At(x, y);
                        }
                    }
                };
                match &mut e.shape {
                    Shape::Line { start, end, .. } => {
                        detach(start);
                        detach(end);
                    }
                    Shape::Circle { center, .. } => detach(center),
                    Shape::Arc { center, start, end, .. } => {
                        detach(center);
                        detach(start);
                        detach(end);
                    }
                    Shape::Rect { corner, .. } => detach(corner),
                    Shape::Spline { points } => points.iter_mut().for_each(detach),
                    Shape::Point { .. } => {}
                }
            }
            _ => {}
        }
        kept.push(n);
    }
    s.items = kept;
}

/* ── the view ─────────────────────────────────────────────────────── */

fn view_of(open: &Open, p: &Part) -> CadPartView {
    let m = model::build(p);
    let (env, _) = pexpr::params(p);
    let mut bounds = Aabb::empty();
    let bodies: Vec<CadBody> = m.bodies.iter().map(|b| body_view(b, &mut bounds)).collect();
    // The sketches' extent too, for a part with nothing extruded yet.
    for s in &m.sketches {
        let k = m.units.mm();
        for (_, q) in &s.points {
            bounds.include(s.frame.origin + s.frame.x * (q.x * k) + s.frame.y * (q.y * k));
        }
    }
    let sketches = p
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, n)| if let Item::Sketch(s) = &n.item { Some((i, n.line, s)) } else { None })
        .filter_map(|(i, line, s)| m.sketches.iter().find(|o| o.item == i).map(|o| sketch_view(i, line, s, o, &m, &env)))
        .collect();
    let mut steps = Vec::new();
    let mut params = Vec::new();
    for (i, n) in p.items.iter().enumerate() {
        let line = n.line as u32;
        let span = match &n.item {
            Item::Sketch(s) => (n.line, s.items.last().map_or(n.line + 1, |l| l.line + 1)),
            _ => (n.line, n.line),
        };
        let error = m.errors.iter().find(|d| d.line >= span.0 && d.line <= span.1).map(|d| d.message.clone());
        let note = m.notes.iter().find(|d| d.line == n.line).map(|d| d.message.clone());
        let blank = CadStep { index: i as u32, line, kind: String::new(), name: String::new(), text: String::new(), error, note, sketch: None, distance: None, symmetric: false, through: false, how: None, body: None, axis: None, angle: None, on: None, offset: None, edges: Vec::new(), value: None, other: None, path: None, open: Vec::new() };
        let step = match &n.item {
            Item::Sketch(s) => {
                let mut head = format!("sketch {} on {}", s.name, s.on.reference);
                if let Some(o) = &s.on.offset {
                    head.push_str(&format!(" offset {}", print::expr(o)));
                }
                CadStep { kind: "sketch".into(), name: s.name.clone(), text: head, on: Some(s.on.reference.clone()), offset: s.on.offset.as_ref().map(print::expr), ..blank }
            }
            Item::Extrude(e) => CadStep {
                kind: "extrude".into(),
                name: e.name.clone(),
                text: line_of(&n.item),
                sketch: Some(e.sketch.clone()),
                distance: e.distance.as_ref().map(print::expr),
                symmetric: e.symmetric,
                through: e.distance.is_none(),
                how: e.into.as_ref().map(|(h, _)| h.word().to_string()),
                body: e.into.as_ref().map(|(_, b)| b.clone()),
                ..blank
            },
            Item::Revolve(r) => CadStep {
                kind: "revolve".into(),
                name: r.name.clone(),
                text: line_of(&n.item),
                sketch: Some(r.sketch.clone()),
                axis: Some(r.axis.clone()),
                angle: r.angle.as_ref().map(print::expr),
                how: r.into.as_ref().map(|(h, _)| h.word().to_string()),
                body: r.into.as_ref().map(|(_, b)| b.clone()),
                ..blank
            },
            Item::Hole(h) => CadStep { kind: "hole".into(), name: h.name.clone(), text: line_of(&n.item), on: Some(h.face.clone()), ..blank },
            Item::Pattern(pt) => CadStep { kind: "pattern".into(), name: pt.name.clone(), text: line_of(&n.item), ..blank },
            Item::Mirror(mi) => CadStep { kind: "mirror".into(), name: mi.name.clone(), text: line_of(&n.item), ..blank },
            Item::Combined(c) => CadStep {
                kind: if c.how == proton_cad::part::ast::Combine::Join { "union".into() } else { c.how.word().into() },
                name: c.body.clone(),
                text: line_of(&n.item),
                how: Some(c.how.word().into()),
                body: Some(c.with.clone()),
                ..blank
            },
            Item::Fillet(f) => CadStep { kind: "fillet".into(), name: f.name.clone(), text: line_of(&n.item), edges: picks_of(&f.edges), value: Some(print::expr(&f.radius)), ..blank },
            Item::Chamfer(c) => CadStep { kind: "chamfer".into(), name: c.name.clone(), text: line_of(&n.item), edges: picks_of(&c.edges), value: Some(print::expr(&c.distance)), other: c.other.as_ref().map(print::expr), ..blank },
            Item::Shell(sh) => CadStep { kind: "shell".into(), name: sh.name.clone(), text: line_of(&n.item), body: Some(sh.body.clone()), value: Some(print::expr(&sh.thickness)), open: sh.open.clone(), ..blank },
            Item::Sweep(sw) => CadStep {
                kind: "sweep".into(),
                name: sw.name.clone(),
                text: line_of(&n.item),
                sketch: Some(sw.sketch.clone()),
                path: Some(sw.path.clone()),
                how: sw.into.as_ref().map(|(h, _)| h.word().to_string()),
                body: sw.into.as_ref().map(|(_, b)| b.clone()),
                ..blank
            },
            Item::Loft(l) => CadStep {
                kind: "loft".into(),
                name: l.name.clone(),
                text: line_of(&n.item),
                sketch: Some(l.from.clone()),
                path: Some(l.to.clone()),
                how: l.into.as_ref().map(|(h, _)| h.word().to_string()),
                body: l.into.as_ref().map(|(_, b)| b.clone()),
                ..blank
            },
            Item::Feature { keyword, text } => {
                // Its name, when its first word is one: `hole m3 on …`.
                let first = text.split_whitespace().next().unwrap_or("");
                let plain = first.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && first.chars().all(|c| c.is_alphanumeric() || c == '_');
                let name = if plain { first.to_string() } else { String::new() };
                CadStep { kind: keyword.clone(), name, text: line_of(&n.item), ..blank }
            }
            Item::Param { name, value } => {
                params.push(CadParam { index: i as u32, line, name: name.clone(), value: print::expr(value), now: env.get(name).copied() });
                continue;
            }
            _ => continue,
        };
        steps.push(step);
    }
    let units = p.items.iter().find_map(|n| if let Item::Units(u) = n.item { Some(u.word().to_string()) } else { None }).unwrap_or_else(|| "mm".into());
    CadPartView {
        path: open.shown.clone(),
        name: open.name.clone(),
        title: m.name.clone(),
        units,
        material: m.material.clone(),
        generation: open.generation,
        dirty: false,
        text: String::new(),
        error: None,
        params,
        steps,
        bodies,
        sketches,
        errors: m.errors.iter().map(CadDiag::from).collect(),
        notes: m.notes.iter().map(CadDiag::from).collect(),
        can_undo: false,
        can_redo: false,
        bounds: if bounds.is_empty() { vec![-50.0, -50.0, -50.0, 50.0, 50.0, 50.0] } else { vec![bounds.min.x, bounds.min.y, bounds.min.z, bounds.max.x, bounds.max.y, bounds.max.z] },
    }
}

/// A fillet's or chamfer's edges, each as the file writes it.
fn picks_of(edges: &[EdgePick]) -> Vec<String> {
    edges
        .iter()
        .map(|e| {
            let one = Item::Fillet(proton_cad::part::ast::Fillet { name: "x".into(), edges: vec![e.clone()], radius: proton_cad::part::ast::Expr::Num(1.0) });
            let line = line_of(&one);
            line.trim_start_matches("fillet x on ").trim_end_matches(" radius 1").to_string()
        })
        .collect()
}

/// An item's own line, as the file writes it.
fn line_of(item: &Item) -> String {
    part::write(&Part { items: vec![Noted::new(item.clone())] }).trim_end().to_string()
}

fn surface_kind(s: &Surface) -> &'static str {
    match s {
        Surface::Plane(_) => "plane",
        Surface::Cylinder(_) => "cylinder",
        Surface::Cone(_) => "cone",
        Surface::Sphere(_) => "sphere",
        Surface::Torus(_) => "torus",
    }
}

/// How finely the screen's triangles follow a part this size.
fn view_quality(b: &Aabb) -> Quality {
    Quality::view(if b.is_empty() { 100.0 } else { b.diagonal().max(1.0) })
}

fn body_view(b: &model::Body, bounds: &mut Aabb) -> CadBody {
    let box_ = b.solid.bounds();
    bounds.include(box_.min);
    bounds.include(box_.max);
    let ms = mass(&b.solid).ok();
    let mesh = tessellate(&b.solid, &view_quality(&box_)).unwrap_or_default();
    // A point for each mesh point and face it is on, so faces meet at a
    // sharp edge with their own normals.
    let mut index = std::collections::HashMap::new();
    let (mut positions, mut normals, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    for (t, (tri, ns)) in mesh.triangles.iter().zip(&mesh.normals).enumerate() {
        let face = mesh.face_of[t];
        for k in 0..3 {
            let key = (tri[k], face);
            let i = *index.entry(key).or_insert_with(|| {
                let p = mesh.positions[tri[k] as usize];
                positions.extend([p.x as f32, p.y as f32, p.z as f32]);
                normals.extend([ns[k].x as f32, ns[k].y as f32, ns[k].z as f32]);
                (positions.len() / 3 - 1) as u32
            });
            indices.push(i);
        }
    }
    // Each edge by its two faces, and which way it turns.
    let uses = proton_cad::blend::uses(&b.solid);
    let turns: Vec<(Option<String>, String)> = (0..b.solid.edges.len())
        .map(|e| match proton_cad::blend::corner(&b.solid, e, &uses[e]) {
            Some(convex) => {
                let (f, g) = (&b.solid.faces[uses[e][0].0].name, &b.solid.faces[uses[e][1].0].name);
                (Some(format!("{f} & {g}")), if convex { "convex" } else { "concave" }.to_string())
            }
            None => (None, if uses[e].len() == 2 && uses[e][0].0 == uses[e][1].0 { "seam" } else { "smooth" }.to_string()),
        })
        .collect();
    let faces = b
        .solid
        .faces
        .iter()
        .map(|f| CadFace {
            name: f.name.clone(),
            surface: surface_kind(&f.surface).into(),
            plane: match f.surface {
                Surface::Plane(p) => {
                    let z = if f.reversed { -p.frame.z } else { p.frame.z };
                    Some(vec![p.frame.origin.x, p.frame.origin.y, p.frame.origin.z, z.x, z.y, z.z, p.frame.x.x, p.frame.x.y, p.frame.x.z])
                }
                _ => None,
            },
        })
        .collect();
    CadBody {
        name: b.name.clone(),
        line: b.line as u32,
        positions,
        normals,
        indices,
        face_of: mesh.face_of.clone(),
        faces,
        edges: mesh.edges.iter().map(|e| e.iter().flat_map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect()).collect(),
        edge_picks: turns.iter().map(|t| t.0.clone()).collect(),
        edge_turns: turns.into_iter().map(|t| t.1).collect(),
        volume: ms.map_or(0.0, |m| m.volume),
        area: ms.map_or(0.0, |m| m.area),
        centroid: ms.map_or(vec![0.0; 3], |m| vec![m.centroid.x, m.centroid.y, m.centroid.z]),
    }
}

fn sketch_view(index: usize, line: usize, s: &Sketch, o: &model::SketchOut, m: &Model, env: &pexpr::Env) -> CadSketchView {
    let at = |p: (usize, usize)| (o.x[p.0], o.x[p.1]);
    let shapes = o
        .drawn
        .iter()
        .map(|d| {
            let (kind, pts, radius) = match &d.geom {
                Geom::Point(p) => ("point", vec![at(*p)], None),
                Geom::Line(a, b) => ("line", vec![at(*a), at(*b)], None),
                Geom::Circle { center, radius } => ("circle", vec![at(*center)], Some(o.x[*radius])),
                Geom::Arc { center, start, end } => {
                    let (c, s) = (at(*center), at(*start));
                    ("arc", vec![c, s, at(*end)], Some((s.0 - c.0).hypot(s.1 - c.1)))
                }
                Geom::Spline(ps) => ("spline", ps.iter().map(|p| at(*p)).collect(), None),
            };
            let text = match s.items.get(d.item).map(|n| &n.item) {
                Some(SketchItem::Entity(e)) => print::entity(e),
                _ => String::new(),
            };
            CadShape { item: d.item as u32, line: d.line as u32, name: d.name.clone(), kind: kind.into(), construction: d.construction, pts, radius, text }
        })
        .collect();
    let value = |e: &proton_cad::part::ast::Expr| (Some(print::expr(e)), pexpr::eval(e, env).ok());
    let mut constraints = Vec::new();
    for (i, n) in s.items.iter().enumerate() {
        let base = CadConstraint {
            item: i as u32,
            line: n.line as u32,
            kind: String::new(),
            refs: Vec::new(),
            value: None,
            now: None,
            which: None,
            redundant: o.report.redundant.contains(&i),
            conflicting: o.report.conflicting.contains(&i),
            text: match &n.item {
                SketchItem::Constraint(c) => print::constraint(c),
                SketchItem::Entity(e) => print::entity(e),
                _ => String::new(),
            },
        };
        match &n.item {
            SketchItem::Constraint(c) => {
                let (v, now) = c.value.as_ref().map_or((None, None), value);
                constraints.push(CadConstraint { kind: c.kind.word().into(), refs: c.refs.clone(), value: v, now, ..base });
            }
            SketchItem::Entity(e) => {
                let mut dim = |kind: &str, which: &str, ex: &proton_cad::part::ast::Expr| {
                    let (v, now) = value(ex);
                    constraints.push(CadConstraint { kind: kind.into(), refs: vec![e.name.clone()], value: v, now, which: Some(which.into()), ..base.clone() });
                };
                match &e.shape {
                    Shape::Line { length: Some(l), .. } => dim("length", "length", l),
                    Shape::Circle { size: Some(Size::Radius(r)), .. } | Shape::Arc { size: Some(Size::Radius(r)), .. } => dim("radius", "size", r),
                    Shape::Circle { size: Some(Size::Diameter(d)), .. } | Shape::Arc { size: Some(Size::Diameter(d)), .. } => dim("diameter", "size", d),
                    Shape::Rect { dims: Some((w, h)), .. } => {
                        dim("width", "w", w);
                        dim("height", "h", h);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let f = o.frame;
    CadSketchView {
        index: index as u32,
        line: line as u32,
        name: s.name.clone(),
        on: s.on.reference.clone(),
        frame: vec![f.origin.x, f.origin.y, f.origin.z, f.x.x, f.x.y, f.x.z, f.y.x, f.y.y, f.y.z, f.z.x, f.z.y, f.z.z],
        scale: m.units.mm(),
        shapes,
        constraints,
        points: o.points.iter().map(|(n, p)| CadPoint { name: n.clone(), x: p.x, y: p.y }).collect(),
        dof: o.report.dof as u32,
        solved: o.report.solved,
        free: o.report.free_points.clone(),
        error: o.error.as_ref().map(CadDiag::from),
        profile: o.profile.as_ref().err().map(|d| d.message.clone()),
        regions: o.profile.as_ref().map_or(0, |p| p.regions.len() as u32),
    }
}

/* ── files ────────────────────────────────────────────────────────── */

/// The part's bodies to a file beside it: `stl` (all in one) or `3mf`
/// (each its own object), to 10 µm, or `step`, exact. The file as the
/// project names it.
fn export(open: &Open, format: &str) -> Result<String, String> {
    let p = part::read(&open.text).map_err(|d| format!("line {} does not read: {}", d.line, d.message))?;
    let m = model::build(&p);
    if m.bodies.is_empty() {
        return Err("there is nothing built to write: extrude or revolve a sketch first".into());
    }
    if format == "step" {
        let bodies: Vec<(&str, &proton_cad::brep::Solid)> = m.bodies.iter().map(|b| (b.name.as_str(), &b.solid)).collect();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let text = proton_cad::step::write(m.name.as_deref().unwrap_or(&open.name), &bodies, &proton_cad::step::stamp(now));
        let file = open.path.with_extension("step");
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        return Ok(Path::new(&open.shown).with_extension("step").to_string_lossy().replace('\\', "/"));
    }
    let meshes: Vec<(String, Mesh)> = m.bodies.iter().map(|b| tessellate(&b.solid, &Quality::print()).map(|t| (b.name.clone(), t))).collect::<Result<_, _>>()?;
    let bytes = match format {
        "stl" => {
            let mut all = Mesh::default();
            for (_, t) in &meshes {
                let base = all.positions.len() as u32;
                all.positions.extend(&t.positions);
                all.triangles.extend(t.triangles.iter().map(|tri| tri.map(|i| i + base)));
            }
            stl::binary(&all, &open.name)
        }
        "3mf" => {
            let refs: Vec<(&str, &Mesh)> = meshes.iter().map(|(n, t)| (n.as_str(), t)).collect();
            threemf::write(m.name.as_deref().unwrap_or(&open.name), &refs)
        }
        other => return Err(format!("“{other}” is not a format to write: stl, 3mf or step")),
    };
    let file = open.path.with_extension(format);
    std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    let shown = Path::new(&open.shown).with_extension(format);
    Ok(shown.to_string_lossy().replace('\\', "/"))
}

/// An assembly's parts, where they are, to a file beside it: `step` exact,
/// or `stl` and `3mf` to 10 µm.
fn export_assembly(open: &Open, parts: &mut crate::cad_assembly::PartCache, format: &str) -> Result<String, String> {
    let a = proton_cad::assembly::read(&open.text).map_err(|d| format!("line {} does not read: {}", d.line, d.message))?;
    let m = parts.build(&a, &open.path, &open.root);
    let solids = crate::cad_assembly::placed_solids(&m);
    if solids.is_empty() {
        return Err("there is nothing in the assembly to write: add a part first".into());
    }
    let name = m.name.clone().unwrap_or_else(|| open.name.clone());
    let bytes = match format {
        "step" => {
            let refs: Vec<(&str, &proton_cad::brep::Solid)> = solids.iter().map(|(n, s)| (n.as_str(), s)).collect();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            proton_cad::step::write(&name, &refs, &proton_cad::step::stamp(now)).into_bytes()
        }
        "stl" | "3mf" => {
            let meshes: Vec<(String, Mesh)> = solids.iter().map(|(n, s)| tessellate(s, &Quality::print()).map(|t| (n.clone(), t))).collect::<Result<_, _>>()?;
            if format == "stl" {
                let mut all = Mesh::default();
                for (_, t) in &meshes {
                    let base = all.positions.len() as u32;
                    all.positions.extend(&t.positions);
                    all.triangles.extend(t.triangles.iter().map(|tri| tri.map(|i| i + base)));
                }
                stl::binary(&all, &open.name)
            } else {
                let refs: Vec<(&str, &Mesh)> = meshes.iter().map(|(n, t)| (n.as_str(), t)).collect();
                threemf::write(&name, &refs)
            }
        }
        other => return Err(format!("“{other}” is not a format to write: step, stl or 3mf")),
    };
    let file = open.path.with_extension(format);
    std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(Path::new(&open.shown).with_extension(format).to_string_lossy().replace('\\', "/"))
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The project's parts, for the home page.
fn find(root: &Path) -> Vec<CadFound> {
    const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor"];
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut looked = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            looked += 1;
            if looked > 5000 || out.len() >= 500 {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            if path.is_dir() {
                if depth < 3 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let kind = match path.extension().and_then(|e| e.to_str()) {
                Some(EXTENSION) => "part",
                Some(crate::cad_assembly::EXTENSION) => "assembly",
                Some(crate::cad_drawing::EXTENSION) => "drawing",
                Some(crate::cad_schematic::EXTENSION) => "schematic",
                Some(crate::cad_board::EXTENSION) => "board",
                _ => continue,
            };
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(CadFound { path: rel, name: stem, kind: kind.into() });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// A `.part` of the project by the path it names, or why not.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || !named.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{rel} is not in the project"));
    }
    if !matches!(named.extension().and_then(|e| e.to_str()), Some(EXTENSION | crate::cad_assembly::EXTENSION | crate::cad_drawing::EXTENSION | crate::cad_schematic::EXTENSION | crate::cad_board::EXTENSION)) {
        return Err(format!("{rel} is not a .part, .assembly, .drawing, .schematic or .board file"));
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
        "part".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-core-cad-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn view(events: &[Event]) -> CadPartView {
        events.iter().rev().find_map(|e| if let Event::CadPart { part: Some(p) } = e { Some((**p).clone()) } else { None }).expect("a part event")
    }

    fn edit(c: &mut Cads, root: &Path, generation: u32, ops: Vec<CadOp>) -> CadPartView {
        view(&c.apply(root, Command::CadEdit { generation, ops }))
    }

    fn near(a: f64, b: f64) {
        assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{a} against {b}");
    }

    fn add(sketch: u32, text: &str) -> CadOp {
        CadOp::SketchAdd { sketch, text: text.into() }
    }

    #[test]
    fn a_part_built_by_hand_is_saved_and_reads_back() {
        let root = scratch("by-hand");
        let mut c = Cads::default();
        let v = view(&c.apply(&root, Command::CadNew { name: "Pump bracket".into() }));
        assert_eq!((v.path.as_str(), v.title.as_deref(), v.steps.len(), v.dirty), ("pump-bracket.part", Some("Pump bracket"), 0, false));
        let g = v.generation;
        // A sketch on XY, and what the mouse would draw in it: a rectangle,
        // a hole, a dimension or two.
        let v = edit(&mut c, &root, g, vec![CadOp::Insert { at: None, text: "sketch base on XY\nend\n".into() }]);
        let base = v.steps[0].index;
        let v = edit(&mut c, &root, g, vec![add(base, "rect r corner origin size (80, 40)"), add(base, "circle h center (20, 20) radius 4")]);
        assert_eq!(v.sketches[0].shapes.len(), 5, "four sides and a circle");
        assert_eq!(v.sketches[0].regions, 1);
        let v = edit(&mut c, &root, g, vec![add(base, "distance r.bl r.br = 80"), add(base, "radius h = 5"), add(base, "vdistance r.bl r.tl = 40")]);
        let radius = v.sketches[0].constraints.iter().find(|k| k.kind == "radius").unwrap().clone();
        assert_eq!((radius.value.as_deref(), radius.now), (Some("5"), Some(5.0)));
        let v = edit(&mut c, &root, g, vec![CadOp::Insert { at: None, text: "extrude plate from base distance 3".into() }]);
        assert!(v.errors.is_empty(), "{:?}", v.errors);
        assert_eq!(v.steps.iter().map(|s| (s.kind.as_str(), s.name.as_str())).collect::<Vec<_>>(), [("sketch", "base"), ("extrude", "plate")]);
        near(v.bodies[0].volume, (3200.0 - std::f64::consts::PI * 25.0) * 3.0);
        assert!(v.dirty && v.can_undo);
        assert!(!v.bodies[0].indices.is_empty() && v.bodies[0].positions.len() == v.bodies[0].normals.len());

        // The hole dragged, then made bigger by its dimension.
        let v = view(&c.apply(&root, Command::CadDrag { generation: g, sketch: "base".into(), point: "h.center".into(), x: 30.0, y: 22.0, done: false }));
        let v2 = view(&c.apply(&root, Command::CadDrag { generation: g, sketch: "base".into(), point: "h.center".into(), x: 40.0, y: 20.0, done: true }));
        let hole = |v: &CadPartView| v.sketches[0].shapes.iter().find(|s| s.name == "h").unwrap().pts[0];
        assert_eq!(hole(&v), (30.0, 22.0));
        assert_eq!(hole(&v2), (40.0, 20.0));
        let v = edit(&mut c, &root, g, vec![CadOp::SetValue { sketch: base, index: radius.item, which: None, value: "6".into() }]);
        near(v.bodies[0].volume, (3200.0 - std::f64::consts::PI * 36.0) * 3.0);
        // The plate's thickness as a parameter.
        // (Each op's places are as the ones before it left the part.)
        let v = edit(&mut c, &root, g, vec![CadOp::Replace { index: v.steps[1].index, text: "extrude plate from base distance t".into() }, CadOp::Param { name: "t".into(), value: "2.5".into() }]);
        near(v.bodies[0].volume, (3200.0 - std::f64::consts::PI * 36.0) * 2.5);
        assert_eq!((v.params[0].name.as_str(), v.params[0].now), ("t", Some(2.5)));

        // Undo: the parameter goes, then the radius, then the drag in one step.
        let v = view(&c.apply(&root, Command::CadUndo { generation: g }));
        near(v.bodies[0].volume, (3200.0 - std::f64::consts::PI * 36.0) * 3.0);
        let v = view(&c.apply(&root, Command::CadUndo { generation: g }));
        near(v.bodies[0].volume, (3200.0 - std::f64::consts::PI * 25.0) * 3.0);
        let v = view(&c.apply(&root, Command::CadUndo { generation: g }));
        assert_eq!(hole(&v), (20.0, 20.0));
        for _ in 0..3 {
            c.apply(&root, Command::CadRedo { generation: g });
        }
        assert_eq!(c.unsaved(), ["pump-bracket.part"]);

        // Saved, and read back as it was written.
        let v = view(&c.apply(&root, Command::CadSave));
        assert!(!v.dirty && c.unsaved().is_empty());
        let disk = std::fs::read_to_string(root.join("pump-bracket.part")).unwrap();
        assert_eq!(disk, v.text);
        assert_eq!(
            disk,
            "part \"Pump bracket\"\nunits mm\nparam t = 2.5\n\nsketch base on XY\n  rect r corner origin size (80, 40)\n  circle h center (40, 20) radius 6\n  distance r.bl r.br = 80\n  radius h = 6\n  vdistance r.bl r.tl = 40\nend\nextrude plate from base distance t\n"
                .replace("\n\nsketch", "\nsketch")
        );
        let m = model::build(&part::read(&disk).unwrap());
        near(mass(&m.body("plate").unwrap().solid).unwrap().volume, (3200.0 - std::f64::consts::PI * 36.0) * 2.5);

        // And opened again, as left.
        c.apply(&root, Command::CadClose { discard: false });
        let v = view(&c.apply(&root, Command::CadOpen { path: "pump-bracket.part".into() }));
        assert_eq!(v.text, disk);
        assert!(v.generation > g && !v.can_undo);
    }

    #[test]
    fn edits_meant_for_another_part_are_not_made() {
        let root = scratch("generation");
        let mut c = Cads::default();
        let g = view(&c.apply(&root, Command::CadNew { name: "a".into() })).generation;
        assert!(c.apply(&root, Command::CadEdit { generation: g + 1, ops: vec![CadOp::Insert { at: None, text: "sketch s on XY\nend".into() }] }).is_empty());
        assert!(c.apply(&root, Command::CadText { generation: g + 1, text: "units in\n".into() }).is_empty());
        assert!(c.unsaved().is_empty());
    }

    #[test]
    fn typing_is_held_and_a_mistake_keeps_the_last_build() {
        let root = scratch("typing");
        let mut c = Cads::default();
        let g = view(&c.apply(&root, Command::CadNew { name: "t".into() })).generation;
        let good = "units mm\nsketch s on XY\n  circle c center (0, 0) radius 5\nend\nextrude e from s distance 2\n";
        let v = view(&c.apply(&root, Command::CadText { generation: g, text: good.into() }));
        near(v.bodies[0].volume, std::f64::consts::PI * 50.0);
        // A keystroke that leaves a line that does not read: said, and the
        // body still shown.
        let v = view(&c.apply(&root, Command::CadText { generation: g, text: good.replace("distance 2", "distance 2 +") }));
        assert_eq!(v.error.as_ref().map(|e| e.line), Some(5));
        assert_eq!(v.bodies.len(), 1);
        let v = view(&c.apply(&root, Command::CadText { generation: g, text: good.replace("distance 2", "distance 4") }));
        assert!(v.error.is_none());
        near(v.bodies[0].volume, std::f64::consts::PI * 100.0);
        // An edit with the mouse while the text does not read is refused.
        c.apply(&root, Command::CadText { generation: g, text: "units furlongs\n".into() });
        let said = c.apply(&root, Command::CadEdit { generation: g, ops: vec![CadOp::Param { name: "a".into(), value: "1".into() }] });
        assert!(said.iter().any(|e| matches!(e, Event::Notice { .. })));
        // The keystrokes in a row were one step: one undo is the new part.
        let v = view(&c.apply(&root, Command::CadUndo { generation: g }));
        assert_eq!(v.text, "part \"t\"\nunits mm\n");
        assert!(!v.can_undo);
    }

    #[test]
    fn a_shape_taken_out_takes_its_constraints_and_lets_go_of_its_points() {
        let root = scratch("remove");
        let mut c = Cads::default();
        let g = view(&c.apply(&root, Command::CadNew { name: "r".into() })).generation;
        let v = edit(&mut c, &root, g, vec![CadOp::Insert { at: None, text: "sketch s on XY\n  line a (0, 0) (10, 0)\n  line b a.end (10, 5)\n  horizontal a\n  length b = 5\n  distance a.start b.end = 12\nend".into() }]);
        let s = v.steps[0].index;
        let v = edit(&mut c, &root, g, vec![CadOp::SketchRemove { sketch: s, indices: vec![0] }]);
        let text = &v.text;
        // b's start was a's end: it stays where the solver had put it.
        let b = v.sketches[0].shapes.iter().find(|s| s.name == "b").unwrap();
        assert!(text.contains(&format!("  line b ({}, {}) ", print::num(b.pts[0].0), print::num(b.pts[0].1))), "{text}");
        assert!(!text.contains("horizontal a") && !text.contains("a.start") && !text.contains("line a"), "{text}");
        assert!(text.contains("length b = 5"), "{text}");
        assert!(v.errors.is_empty(), "{:?}", v.errors);
    }

    #[test]
    fn a_sketch_renamed_is_renamed_where_it_is_used() {
        let root = scratch("rename");
        let mut c = Cads::default();
        let g = view(&c.apply(&root, Command::CadNew { name: "n".into() })).generation;
        let v = edit(&mut c, &root, g, vec![CadOp::Insert { at: None, text: "sketch s on XY\n  rect r corner origin size (4, 4)\nend\nextrude e from s distance 1\nsketch t on e.top\n  circle c center (2, 2) radius 1\nend\nextrude peg from t distance 2".into() }]);
        assert!(v.errors.is_empty(), "{:?}", v.errors);
        let v = edit(&mut c, &root, g, vec![CadOp::Replace { index: v.steps[0].index, text: "sketch base on XZ".into() }, CadOp::Replace { index: v.steps[1].index, text: "extrude plate from base distance 1".into() }]);
        assert!(v.errors.is_empty(), "{:?}", v.errors);
        assert!(v.text.contains("sketch base on XZ\n  rect r") && v.text.contains("sketch t on plate.top") && v.text.contains("extrude plate from base"), "{}", v.text);
        assert_eq!(v.bodies.len(), 2);
        // A step that does not build says why, at its step.
        let v = edit(&mut c, &root, g, vec![CadOp::Replace { index: v.steps[3].index, text: "extrude peg from nowhere distance 2".into() }]);
        assert!(v.steps[3].error.as_deref().is_some_and(|e| e.contains("nowhere")), "{:?}", v.steps[3]);
        assert!(v.steps[..3].iter().all(|s| s.error.is_none()));
    }

    #[test]
    fn the_bodies_are_written_for_printers_and_machines_beside_the_part() {
        let root = scratch("export");
        let mut c = Cads::default();
        let g = view(&c.apply(&root, Command::CadNew { name: "Peg".into() })).generation;
        let said = c.apply(&root, Command::CadExport { generation: g, format: "stl".into() });
        assert!(said.iter().any(|e| matches!(e, Event::Notice { .. })) && !root.join("peg.stl").exists());
        edit(&mut c, &root, g, vec![CadOp::Insert { at: None, text: "sketch s on XY\n  circle c center (6, 0) radius 3\nend\nrevolve r from s about yaxis angle 90\nextrude e from s distance 5".into() }]);
        c.apply(&root, Command::CadExport { generation: g, format: "stl".into() });
        c.apply(&root, Command::CadExport { generation: g, format: "3mf".into() });
        c.apply(&root, Command::CadExport { generation: g, format: "step".into() });
        let solids = proton_cad::step::read(&std::fs::read_to_string(root.join("peg.step")).unwrap()).unwrap();
        // And exact, for machining: the same two bodies, whole.
        assert_eq!(solids.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["r", "e"]);
        assert!(solids.iter().all(|(_, s)| proton_cad::brep::check::check(s).is_empty()), "the STEP written does not read back whole");
        let tris = stl::read_binary(&std::fs::read(root.join("peg.stl")).unwrap()).unwrap();
        assert!(tris.len() > 100);
        let (title, objects) = threemf::read(&std::fs::read(root.join("peg.3mf")).unwrap()).unwrap();
        assert_eq!(title, "Peg");
        assert_eq!(objects.iter().map(|o| o.0.as_str()).collect::<Vec<_>>(), ["r", "e"]);
    }

    fn drawing(events: &[Event]) -> crate::cad_drawing::CadDrawingView {
        events.iter().rev().find_map(|e| if let Event::CadDrawing { drawing: Some(d) } = e { Some((**d).clone()) } else { None }).expect("a drawing event")
    }

    #[test]
    fn a_part_is_drawn_edited_and_written_for_the_shop() {
        let root = scratch("drawing");
        let mut c = Cads::default();
        let g = view(&c.apply(&root, Command::CadNew { name: "Plate".into() })).generation;
        edit(&mut c, &root, g, vec![CadOp::Insert { at: None, text: "sketch s on XY\n  rect r corner origin size = (100, 60)\n  circle h center (10, 10) radius 2.2\nend\nextrude plate from s distance 4".into() }]);
        // Unsaved, it is not drawn: the drawing reads the file.
        let said = c.apply(&root, Command::CadDraw { generation: g, blueprint: false });
        assert!(said.iter().any(|e| matches!(e, Event::Notice { text: message, .. } if message.contains("save it first"))));
        c.apply(&root, Command::CadSave);
        let out = c.apply(&root, Command::CadDraw { generation: g, blueprint: false });
        assert!(out.iter().any(|e| matches!(e, Event::CadHome { parts } if parts.iter().any(|p| p.kind == "drawing" && p.path == "plate.drawing"))));
        assert!(out.iter().any(|e| matches!(e, Event::CadPart { part: None })), "the part is put away");
        let d = drawing(&out);
        assert_eq!((d.path.as_str(), d.of.as_deref(), d.sheet.as_str(), d.width, d.blueprint, d.dirty), ("plate.drawing", Some("plate.part"), "A3", 420.0, false, false));
        assert!(d.errors.is_empty(), "{:?}", d.errors);
        assert_eq!(d.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["front", "top", "right", "iso"]);
        assert!(d.svg.starts_with("<svg") && d.boxes.iter().any(|b| b.name == "top"));
        // A dimension added: one step to undo, drawn, its line listed.
        let d = drawing(&c.apply(&root, Command::CadEdit { generation: d.generation, ops: vec![CadOp::Insert { at: None, text: "dim w top distance plate.side(r.left) to plate.side(r.right)".into() }] }));
        assert!(d.dirty && d.can_undo && d.errors.is_empty(), "{:?}", d.errors);
        assert!(d.svg.contains(">100</text>"));
        let w = d.items.iter().find(|i| i.name == "w").unwrap();
        assert_eq!((w.kind.as_str(), w.text.as_str()), ("dim", "dim w top distance plate.side(r.left) to plate.side(r.right)"));
        // A mistake typed keeps the last sheet, and says where.
        let bad = drawing(&c.apply(&root, Command::CadText { generation: d.generation, text: format!("{}view x sideways at (1, 2)\n", d.text) }));
        assert!(bad.error.as_ref().is_some_and(|e| e.line as usize == d.text.lines().count() + 1), "{:?}", bad.error);
        assert_eq!(bad.svg, d.svg);
        // One that reads but cannot be drawn is said at its line.
        let wrong = drawing(&c.apply(&root, Command::CadText { generation: d.generation, text: format!("{}dim z top diameter nothing\n", d.text) }));
        let z = wrong.items.iter().find(|i| i.name == "z").unwrap();
        assert!(z.error.as_ref().is_some_and(|e| e.contains("names no face")), "{z:?}");
        c.apply(&root, Command::CadUndo { generation: d.generation });
        c.apply(&root, Command::CadSave);
        assert!(std::fs::read_to_string(root.join("plate.drawing")).unwrap().contains("dim w top distance"));
        for f in ["pdf", "svg", "dxf"] {
            let said = c.apply(&root, Command::CadExport { generation: d.generation, format: f.into() });
            assert!(said.iter().any(|e| matches!(e, Event::Notice { text: message, .. } if message == &format!("wrote plate.{f}"))), "{said:?}");
        }
        assert!(std::fs::read(root.join("plate.pdf")).unwrap().starts_with(b"%PDF-1.4"));
        assert!(proton_cad::drawing::out::read_dxf(&std::fs::read_to_string(root.join("plate.dxf")).unwrap()).unwrap().iter().any(|e| e.0 == "CIRCLE"));
        // Opened from the home page, as any CAD file is.
        c.apply(&root, Command::CadClose { discard: false });
        let reopened = drawing(&c.apply(&root, Command::CadOpen { path: "plate.drawing".into() }));
        assert_eq!((reopened.path.as_str(), reopened.dirty), ("plate.drawing", false));
        assert!(reopened.items.iter().any(|i| i.name == "w"));
        // Drawn again, the one there is opened, not written over.
        c.apply(&root, Command::CadOpen { path: "plate.part".into() });
        let g = view(&c.apply(&root, Command::CadOpen { path: "plate.part".into() })).generation;
        let again = c.apply(&root, Command::CadDraw { generation: g, blueprint: true });
        assert!(again.iter().any(|e| matches!(e, Event::Notice { text: message, .. } if message.contains("which was there"))));
        assert!(drawing(&again).items.iter().any(|i| i.name == "w"));
    }

    fn schematic(events: &[Event]) -> crate::cad_schematic::CadSchematicView {
        events.iter().rev().find_map(|e| if let Event::CadSchematic { schematic: Some(s) } = e { Some((**s).clone()) } else { None }).expect("a schematic event")
    }

    #[test]
    fn a_schematic_is_made_edited_checked_and_written_out() {
        let root = scratch("schematic");
        let mut c = Cads::default();
        let out = c.apply(&root, Command::CadNewSchematic { name: "Fan hub".into() });
        let s = schematic(&out);
        assert_eq!((s.path.as_str(), s.sheet.as_str(), s.title.as_deref(), s.parts.len(), s.text.as_str()), ("fan-hub.schematic", "A4", Some("Fan hub"), 0, "schematic \"Fan hub\"\nsheet A4\n"));
        assert!(out.iter().any(|e| matches!(e, Event::CadHome { parts } if parts.iter().any(|p| p.kind == "schematic"))));
        assert!(s.kinds.iter().any(|k| k.kind == "atx-24") && s.kinds.iter().any(|k| k.kind == "ic" && k.takes.contains("pins")));
        let g = s.generation;
        let s = schematic(&c.apply(&root, Command::CadEdit { generation: g, ops: vec![CadOp::Insert { at: None, text: "part P1 sata-power at (40, 120)\npart J1 fan-4pin at (150, 120)\nnet +12V power P1.13, J1.2 current 2".into() }] }));
        assert!(s.dirty && s.can_undo);
        assert_eq!(s.parts.iter().map(|p| p.reference.as_str()).collect::<Vec<_>>(), ["P1", "J1"]);
        assert_eq!(s.parts[1].pins[1].net.as_deref(), Some("+12V"));
        assert_eq!((s.nets[0].pins.clone(), s.nets[0].current, s.nets[0].power), (vec!["P1.13".to_string(), "J1.2".to_string()], Some(2.0), true));
        assert!(s.findings.iter().any(|f| !f.error && f.message.starts_with("J1: pins 1, 3, 4")), "{:?}", s.findings);
        assert_eq!(s.items.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(), ["part", "part", "net"]);
        assert!(s.svg.contains(">+12V</text>") && s.boxes.iter().any(|b| b.name == "J1"));
        // A part moved by its line; a mistake typed keeps the last that read.
        let j1 = s.items.iter().find(|i| i.name == "J1").unwrap().index;
        let s = schematic(&c.apply(&root, Command::CadEdit { generation: g, ops: vec![CadOp::Replace { index: j1, text: "part J1 fan-4pin at (160, 100)".into() }] }));
        assert_eq!(s.parts[1].at, vec![160.0, 100.0]);
        let bad = schematic(&c.apply(&root, Command::CadText { generation: g, text: format!("{}part 9X resistor\n", s.text) }));
        assert!(bad.error.as_ref().is_some_and(|e| e.message.contains("not a reference")) && bad.parts.is_empty(), "the saved text is what is shown: {:?}", bad.error);
        c.apply(&root, Command::CadUndo { generation: g });
        c.apply(&root, Command::CadSave);
        for f in ["svg", "pdf", "net", "csv"] {
            let said = c.apply(&root, Command::CadExport { generation: g, format: f.into() });
            assert!(said.iter().any(|e| matches!(e, Event::Notice { text, .. } if text == &format!("wrote fan-hub.{f}"))), "{said:?}");
        }
        // The fan's footprint, its family's, as a KiCad library.
        assert_eq!((s.parts[1].pads, s.parts[1].land_source.as_deref()), (4, Some("KK 254's 2.54 mm pitch")));
        assert!(s.parts[1].land_svg.as_ref().is_some_and(|x| x.starts_with("<svg")) && s.parts[1].land_caution.is_some());
        let said = c.apply(&root, Command::CadExport { generation: g, format: "kicad".into() });
        assert!(said.iter().any(|e| matches!(e, Event::Notice { text, .. } if text == "wrote fan-hub.pretty (1 footprint)")), "{said:?}");
        assert!(std::fs::read_to_string(root.join("fan-hub.pretty/kk-254-4.kicad_mod")).unwrap().starts_with("(footprint \"kk-254-4\""));
        assert!(std::fs::read_to_string(root.join("fan-hub.net")).unwrap().contains("(name \"+12V\")"));
        assert!(std::fs::read_to_string(root.join("fan-hub.csv")).unwrap().contains("4-pin PWM fan"));
        // Not drawn as a part is: it is its own sheet.
        assert!(c.apply(&root, Command::CadDraw { generation: g, blueprint: false }).is_empty());
        c.apply(&root, Command::CadClose { discard: false });
        let again = schematic(&c.apply(&root, Command::CadOpen { path: "fan-hub.schematic".into() }));
        assert_eq!((again.parts.len(), again.dirty), (2, false));
    }

    fn board(events: &[Event]) -> crate::cad_board::CadBoardView {
        events.iter().rev().find_map(|e| if let Event::CadBoard { board: Some(b) } = e { Some((**b).clone()) } else { None }).expect("a board event")
    }

    #[test]
    fn a_board_is_made_from_its_schematic_routed_and_checked() {
        let root = scratch("board");
        std::fs::write(root.join("hub.schematic"), "schematic \"Hub\"\npart P1 header 2 footprint header-1x2\npart J1 fan-4pin\npart J2 fan-4pin\npart R1 resistor footprint 0805\nnet +12V power P1.1, J1.2, J2.2 current 2\nnet GND power P1.2, J1.1, J2.1\nnet PWM J1.4, J2.4, R1.1\nnet PU R1.2, J1.3\nnc J2.3\n").unwrap();
        let mut c = Cads::default();
        let g = schematic(&c.apply(&root, Command::CadOpen { path: "hub.schematic".into() })).generation;
        let out = c.apply(&root, Command::CadMakeBoard { generation: g });
        assert!(out.iter().any(|e| matches!(e, Event::Notice { text, .. } if text == "made hub.board")), "{out:?}");
        assert!(out.iter().any(|e| matches!(e, Event::CadSchematic { schematic: None })), "the schematic is put away");
        let b = board(&out);
        assert_eq!((b.path.as_str(), b.schematic.as_deref(), b.layers), ("hub.board", Some("hub.schematic"), 2));
        assert!(b.parts.iter().all(|p| p.placed), "every part set out: {:?}", b.parts);
        assert!(b.findings.iter().all(|f| !f.error), "a first board breaks no rule: {:?}", b.findings);
        let net = |b: &crate::cad_board::CadBoardView, n: &str| b.nets.iter().find(|x| x.name == n).unwrap().clone();
        assert_eq!(net(&b, "+12V").unrouted, 2);
        assert!(net(&b, "+12V").needs.as_ref().is_some_and(|w| (w[0] - 0.781).abs() < 0.01));
        assert!(b.svg.starts_with("<svg") && b.boxes.len() == 4 && b.pads.iter().any(|p| p.name == "J1.2" && p.net.as_deref() == Some("+12V") && p.layer == "through"));
        let at = |n: &str| b.pads.iter().find(|p| p.name == n).map(|p| (p.x, p.y)).unwrap();
        // Its Gerbers and drills, for a fab: what is left to route is said.
        let said = c.apply(&root, Command::CadExport { generation: b.generation, format: "gerber".into() });
        assert!(said.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.starts_with("wrote hub-fab/ (13 files) — ") && text.contains("not routed yet"))), "{said:?}");
        assert!(proton_board::gerber::read_gerber(&std::fs::read_to_string(root.join("hub-fab/hub-F_Cu.gbr")).unwrap()).is_ok());
        assert_eq!(proton_board::gerber::read_excellon(&std::fs::read_to_string(root.join("hub-fab/hub-PTH.drl")).unwrap()).unwrap().len(), 10);
        // A short, and it is refused.
        let (p, q) = (at("P1.1"), at("P1.2"));
        c.apply(&root, Command::CadEdit { generation: b.generation, ops: vec![CadOp::Insert { at: None, text: format!("track GND F.Cu ({}, {}) ({}, {})", p.0, p.1, q.0, q.1) }] });
        let refused = c.apply(&root, Command::CadExport { generation: b.generation, format: "gerber".into() });
        assert!(refused.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.starts_with("the board breaks") && text.contains("short"))), "{refused:?}");
        c.apply(&root, Command::CadUndo { generation: b.generation });
        c.apply(&root, Command::CadSave);
        // A track from the power pin to a fan's: one connection left.
        let (a, z) = (at("P1.1"), at("J1.2"));
        let line = format!("track +12V B.Cu width 1 ({}, {}) ({}, {}) ({}, {})", a.0, a.1, a.0, z.1, z.0, z.1);
        let b2 = board(&c.apply(&root, Command::CadEdit { generation: b.generation, ops: vec![CadOp::Insert { at: None, text: line }] }));
        assert!(b2.dirty && net(&b2, "+12V").unrouted == 1, "{:?}", b2.nets);
        assert!(b2.items.iter().any(|i| i.kind == "track" && i.name == "+12V"));
        // A part moved by its line.
        let r1 = b2.items.iter().find(|i| i.kind == "place" && i.name == "R1").unwrap().index;
        let b3 = board(&c.apply(&root, Command::CadEdit { generation: b.generation, ops: vec![CadOp::Replace { index: r1, text: "place R1 at (40, 40) turn 90".into() }] }));
        assert_eq!(b3.parts.iter().find(|p| p.reference == "R1").map(|p| (p.at.clone(), p.turn)), Some((vec![40.0, 40.0], 90)));
        c.apply(&root, Command::CadSave);
        let said = c.apply(&root, Command::CadExport { generation: b.generation, format: "svg".into() });
        assert!(said.iter().any(|e| matches!(e, Event::Notice { text, .. } if text == "wrote hub.svg")), "{said:?}");
        // Made again: the one there opened.
        c.apply(&root, Command::CadClose { discard: false });
        let g = schematic(&c.apply(&root, Command::CadOpen { path: "hub.schematic".into() })).generation;
        let again = c.apply(&root, Command::CadMakeBoard { generation: g });
        assert!(again.iter().any(|e| matches!(e, Event::Notice { text, .. } if text.contains("which was there"))));
        assert!(board(&again).items.iter().any(|i| i.kind == "track"));
    }

    #[test]
    fn an_assemblys_blueprint_is_drawn_on_blue() {
        let root = scratch("blueprint");
        std::fs::write(root.join("plate.part"), "part \"Plate\"\nsketch s on XY\n  rect r corner origin size = (100, 60)\nend\nextrude plate from s distance 4\n").unwrap();
        std::fs::write(root.join("block.part"), "part \"Block\"\nsketch s on XY\n  rect r corner origin size = (20, 10)\nend\nextrude b from s distance 8\n").unwrap();
        std::fs::write(root.join("bench.assembly"), "assembly \"Bench\"\nunits mm\npart plate from \"plate.part\"\npart block from \"block.part\"\nground plate\nplace block at (30, 20, 4)\n").unwrap();
        let mut c = Cads::default();
        let out = c.apply(&root, Command::CadOpen { path: "bench.assembly".into() });
        let g = out.iter().find_map(|e| if let Event::CadAssembly { assembly: Some(a) } = e { Some(a.generation) } else { None }).unwrap();
        let d = drawing(&c.apply(&root, Command::CadDraw { generation: g, blueprint: true }));
        assert!(d.blueprint && d.errors.is_empty(), "{:?}", d.errors);
        assert_eq!((d.path.as_str(), d.sheet.as_str(), d.title.as_deref()), ("bench.drawing", "A2", Some("Bench")));
        assert!(d.items.iter().any(|i| i.kind == "bom") && d.items.iter().any(|i| i.kind == "balloons"));
        assert!(d.svg.contains("#1d4e89") && d.svg.contains(">Block</text>"));
    }
}

#[cfg(test)]
mod preview {
    use super::*;

    /// The sample part the browser preview shows, as the core draws it:
    /// `cargo test -p proton-core --lib dump_part_for_the_preview -- --ignored`
    /// writes `ui/preview-cad-part.js`.
    pub const SAMPLE: &str = "part \"Pump bracket\"\nunits mm\nmaterial aluminium-6061\nparam t = 3\nparam hole = 3.4\n\n# The plate, with the pump's two screws.\nsketch base on XY\n  rect r corner origin size = (80, 40)\n  circle h1 center (10, 10) diameter = hole\n  circle h2 center (70, 10) diameter = hole\n  horizontal h1.center h2.center\n  hdistance r.bl h1.center = 10\n  vdistance r.bl h1.center = 10\n  hdistance h1.center h2.center = 60\nend\nextrude plate from base distance t\n\nsketch post on plate.top\n  circle p center (40, 26) radius = 6\nend\nextrude boss from post distance 8\nfillet corners on plate.edges(parallel z) radius 2\n";

    #[test]
    #[ignore]
    fn dump_part_for_the_preview() {
        let root = std::env::temp_dir().join(format!("proton-core-cad-preview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("pump-bracket.part"), SAMPLE).unwrap();
        let mut c = Cads::default();
        let events = c.apply(&root, Command::CadOpen { path: "pump-bracket.part".into() });
        let Some(Event::CadPart { part: Some(p) }) = events.into_iter().last() else { panic!("no part") };
        // Points to a hundredth of a micrometre are plenty for a picture.
        let mut p = *p;
        for b in &mut p.bodies {
            for x in b.positions.iter_mut().chain(b.normals.iter_mut()) {
                *x = (*x * 1e4).round() / 1e4;
            }
            for e in &mut b.edges {
                for x in e.iter_mut() {
                    *x = (*x * 1e4).round() / 1e4;
                }
            }
        }
        p.generation = 0;
        let json = serde_json::to_string(&p).unwrap();
        let out = format!("/* The sample part the browser preview shows, as the core draws it —\n * written by `cargo test -p proton-core --lib dump_part_for_the_preview -- --ignored`;\n * not edited by hand. */\n\n/** @type {{any}} */\nexport const PART = {json};\n");
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/preview-cad-part.js");
        std::fs::write(&path, out).unwrap();
        println!("wrote {} ({} bytes)", path.display(), std::fs::metadata(&path).unwrap().len());
    }
}
