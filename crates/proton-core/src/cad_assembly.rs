//! CAD's assemblies: the `.assembly` open, built, solved and drawn — the
//! editor's half of `proton_cad::assembly`.
//!
//! An assembly is edited as a part is: its text held until ⌘S, the
//! window's edits lines of its language added, replaced or taken out, one
//! step each to undo. After each the mates are solved and every placement
//! the solver moved is written back, so the file says where everything is.
//! The parts it holds are read from their files as saved and built once
//! until their files change.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use proton_cad::assembly::lang::{self, Item, Source};
use proton_cad::assembly::model::{Built, Model};
use proton_cad::assembly::{self as asm, fit};
use proton_cad::brep::Solid;
use proton_cad::frame::Transform;
use proton_cad::mesh::{tessellate, Mesh, Quality};
use proton_cad::part::ast::{Diag, Noted};
use proton_cad::part::print;
use proton_cad::surface::Surface;
use proton_cad::vec::{Aabb, Vec3};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cad::{CadBody, CadDiag, CadFace, CadOp};

/// An assembly's file ends so.
pub const EXTENSION: &str = "assembly";

/// An instance of the assembly: what it is of, where the solver left it,
/// and whether anything still lets it move.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadInstance {
    pub index: u32,
    pub line: u32,
    pub name: String,
    /// `part` or `assembly`.
    pub kind: String,
    pub path: String,
    pub ground: bool,
    pub free: bool,
    /// Where it is: its origin in the assembly's units, and its turns about
    /// x, y and z in degrees.
    pub at: Vec<f64>,
    pub turn: Vec<f64>,
    pub error: Option<String>,
    /// Its parts as the view names them: `case`, or `loop.pump` for an
    /// assembly's.
    pub parts: Vec<String>,
}

/// A mate: its line's fields, and how near it is to holding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadMate {
    pub index: u32,
    pub line: u32,
    pub name: String,
    pub kind: String,
    pub a: String,
    pub b: String,
    /// A distance's or angle's value, or an offset, as written.
    pub value: Option<String>,
    pub residual: f64,
    pub error: Option<String>,
    /// It holds nothing the others do not.
    pub redundant: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadMove {
    pub instance: String,
    /// Millimetres.
    pub by: Vec<f64>,
}

/// An exploded view: how far each instance moves in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadExplode {
    pub index: u32,
    pub line: u32,
    pub name: String,
    pub text: String,
    pub moves: Vec<CadMove>,
}

/// A row of the bill of materials.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadBomRow {
    pub part: String,
    pub path: String,
    pub material: Option<String>,
    pub count: u32,
    pub instances: Vec<String>,
    /// Tube to cut in all, mm.
    pub length: Option<f64>,
}

/// A bend of a hard tube: where it starts along the tube as cut, how far it
/// turns, its radius, and how far it is turned from the bend before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadBend {
    pub at: f64,
    pub angle: f64,
    pub radius: f64,
    pub twist: f64,
}

/// A route: its line's fields, and its tube as built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadRoute {
    pub index: u32,
    pub line: u32,
    pub name: String,
    /// `hard` or `soft`.
    pub kind: String,
    pub size: String,
    pub from: String,
    pub to: String,
    /// Its points, as written.
    pub via: Vec<Vec<f64>>,
    pub bend: Option<String>,
    pub lead: Option<String>,
    pub insert: Option<String>,
    /// Along its centre line, and to cut, mm.
    pub length: Option<f64>,
    pub cut: Option<f64>,
    pub bends: Vec<CadBend>,
    pub error: Option<String>,
    pub text: String,
}

/// The loop: its parts in the order the coolant meets them, each with the
/// route that leaves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadLoop {
    pub parts: Vec<String>,
    pub routes: Vec<Option<String>>,
    pub closed: bool,
    pub notes: Vec<String>,
}

/// Two parts that do not fit: overlapping, or nearer than the clearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadClash {
    pub a: String,
    pub b: String,
    pub overlap: bool,
    /// Cubic millimetres, where it could be worked out.
    pub volume: Option<f64>,
    /// Millimetres apart, for a gap.
    pub gap: f64,
    /// The nearest points, one on each: six numbers.
    pub at: Vec<f64>,
}

/// The assembly open: its text, its instances and mates solved, its parts
/// drawn where they are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadAssemblyView {
    pub path: String,
    pub name: String,
    pub title: Option<String>,
    pub units: String,
    pub generation: u32,
    pub dirty: bool,
    pub text: String,
    pub error: Option<CadDiag>,
    pub instances: Vec<CadInstance>,
    pub mates: Vec<CadMate>,
    pub explodes: Vec<CadExplode>,
    pub routes: Vec<CadRoute>,
    #[serde(rename = "loop")]
    #[ts(rename = "loop")]
    pub coolant: CadLoop,
    /// Every part's bodies where they are, each named for its part:
    /// `board`, `loop.pump`.
    pub bodies: Vec<CadBody>,
    pub dof: u32,
    pub solved: bool,
    pub conflicting: Vec<String>,
    pub bom: Vec<CadBomRow>,
    /// What the last fit check found; none until one is asked for.
    pub clashes: Option<Vec<CadClash>>,
    /// The least gap wanted, millimetres.
    pub clearance: f64,
    pub errors: Vec<CadDiag>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub bounds: Vec<f64>,
}

/// A parameter of a library family, for the window's form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadFamilyParam {
    pub name: String,
    pub default: String,
    /// Empty when it takes any value.
    pub choices: Vec<String>,
    pub note: String,
}

/// A family of the parts library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadFamily {
    pub name: String,
    pub title: String,
    pub group: String,
    pub params: Vec<CadFamilyParam>,
}

/// The library's families, as the window lists them.
pub fn families() -> Vec<CadFamily> {
    proton_cad::library::families()
        .into_iter()
        .map(|f| CadFamily {
            name: f.name.into(),
            title: f.title.into(),
            group: f.group.into(),
            params: f.params.iter().map(|p| CadFamilyParam { name: p.name.into(), default: p.default.into(), choices: p.choices.iter().map(|c| c.to_string()).collect(), note: p.note.into() }).collect(),
        })
        .collect()
}

/// A library part written into the project: `library/STEM.part`, or the
/// same with a number if a different part has the name; the path as the
/// project names it.
pub fn write_library_part(root: &Path, family: &str, params: &[(String, String)]) -> Result<String, String> {
    let made = proton_cad::library::make(family, params)?;
    let dir = root.join("library");
    std::fs::create_dir_all(&dir).map_err(|e| format!("library/: {e}"))?;
    for k in 1.. {
        let name = if k == 1 { format!("{}.part", made.stem) } else { format!("{}-{k}.part", made.stem) };
        let file = dir.join(&name);
        match std::fs::read_to_string(&file) {
            Ok(text) if text == made.text => return Ok(format!("library/{name}")),
            Ok(_) => continue,
            Err(_) => {
                std::fs::write(&file, &made.text).map_err(|e| format!("library/{name}: {e}"))?;
                return Ok(format!("library/{name}"));
            }
        }
    }
    unreachable!()
}

/// An instance's name for a file: its stem, as a name, not one taken.
pub fn instance_name(a: &lang::Assembly, path: &str) -> String {
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "part".into());
    let mut base: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if base.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        base.insert(0, '_');
    }
    let taken: Vec<&str> = a.items.iter().filter_map(|n| item_name(&n.item)).collect();
    if !taken.contains(&base.as_str()) {
        return base;
    }
    (2..).map(|k| format!("{base}{k}")).find(|x| !taken.contains(&x.as_str())).expect("a name")
}

/// The path from an assembly (as the project names it) to a file of the
/// project.
pub fn relative(from: &str, to: &str) -> String {
    let dir: Vec<&str> = from.rsplit_once('/').map_or(Vec::new(), |(d, _)| d.split('/').collect());
    let parts: Vec<&str> = to.split('/').collect();
    let mut k = 0;
    while k < dir.len() && k + 1 < parts.len() && dir[k] == parts[k] {
        k += 1;
    }
    let mut out: Vec<String> = dir[k..].iter().map(|_| "..".to_string()).collect();
    out.extend(parts[k..].iter().map(|s| s.to_string()));
    out.join("/")
}

/// Parts built, kept until their files change.
#[derive(Debug, Default)]
pub struct PartCache {
    built: HashMap<PathBuf, (Option<SystemTime>, Arc<Built>)>,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// A file the assembly names, read if it is in the project.
fn load_inside(root: &Path) -> impl Fn(&Path) -> Result<String, String> {
    let base = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    move |p: &Path| {
        let real = std::fs::canonicalize(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if !real.starts_with(&base) {
            return Err(format!("{} is outside the project", p.display()));
        }
        std::fs::read_to_string(&real).map_err(|e| format!("{}: {e}", p.display()))
    }
}

impl PartCache {
    /// A drawing laid out, an assembly it is of built from the parts here
    /// while their files are as they were.
    pub fn drawing(&mut self, d: &proton_cad::drawing::lang::Drawing, here: &Path, root: &Path) -> proton_cad::drawing::build::Built {
        let mut parts: HashMap<PathBuf, Arc<Built>> = self.built.iter().filter(|(p, (t, _))| mtime(p) == *t).map(|(p, (_, b))| (p.clone(), b.clone())).collect();
        let load = load_inside(root);
        let b = proton_cad::drawing::build::build_with(d, here, &load, &mut parts);
        self.built = parts.into_iter().map(|(p, b)| (p.clone(), (mtime(&p), b))).collect();
        b
    }

    /// The assembly built, the parts it holds taken from here while their
    /// files are as they were.
    pub fn build(&mut self, a: &lang::Assembly, here: &Path, root: &Path) -> Model {
        let mut parts: HashMap<PathBuf, Arc<Built>> = self.built.iter().filter(|(p, (t, _))| mtime(p) == *t).map(|(p, (_, b))| (p.clone(), b.clone())).collect();
        let load = load_inside(root);
        let m = asm::build_with(a, here, &load, &mut parts);
        self.built = parts.into_iter().map(|(p, b)| (p.clone(), (mtime(&p), b))).collect();
        m
    }
}

/* ── edits ────────────────────────────────────────────────────────── */

fn item_name(i: &Item) -> Option<&str> {
    match i {
        Item::Instance(x) => Some(&x.name),
        Item::Mate(m) => Some(&m.name),
        Item::Explode(e) => Some(&e.name),
        Item::Route(r) => Some(&r.name),
        _ => None,
    }
}

/// An instance renamed wherever the assembly names it.
fn rename(a: &mut lang::Assembly, old: &str, new: &str) {
    let fix = |r: &mut String| {
        if r == old {
            *r = new.to_string();
        } else if let Some(rest) = r.strip_prefix(&format!("{old}.")) {
            *r = format!("{new}.{rest}");
        }
    };
    for n in &mut a.items {
        match &mut n.item {
            Item::Ground(g) => fix(g),
            Item::Place(p) => fix(&mut p.name),
            Item::Mate(m) => {
                fix(&mut m.a);
                fix(&mut m.b);
            }
            Item::Explode(e) => e.moves.iter_mut().for_each(|(w, _)| fix(w)),
            Item::Route(r) => {
                fix(&mut r.from);
                fix(&mut r.to);
            }
            _ => {}
        }
    }
}

/// One of the window's edits made to the assembly.
pub fn edit(a: &mut lang::Assembly, op: CadOp) -> Result<(), String> {
    let bad = |d: Diag| format!("line {} of the edit: {}", d.line, d.message);
    match op {
        CadOp::Insert { at, text } => {
            let items = lang::read(&text).map_err(bad)?.items;
            let at = at.map_or(a.items.len(), |x| (x as usize).min(a.items.len()));
            a.items.splice(at..at, items);
        }
        CadOp::Replace { index, text } => {
            let i = index as usize;
            let old = a.items.get(i).map(|n| n.item.clone()).ok_or_else(|| format!("there is no item {index}"))?;
            let mut read = lang::read(&text).map_err(bad)?.items;
            if read.len() != 1 {
                return Err("a step is one line".into());
            }
            let new = read.remove(0).item;
            if let (Item::Instance(x), Item::Instance(y)) = (&old, &new) {
                if x.name != y.name {
                    rename(a, &x.name, &y.name);
                }
            }
            let _ = item_name(&old);
            a.items[i].item = new;
        }
        CadOp::Remove { index } => {
            let i = index as usize;
            if i >= a.items.len() {
                return Err(format!("there is no item {index}"));
            }
            a.items.remove(i);
        }
        _ => return Err("that edit is a part's: an assembly takes lines added, replaced or taken out".into()),
    }
    Ok(())
}

/* ── the view ─────────────────────────────────────────────────────── */

/// A part's body drawn where an instance puts it, named for the instance.
fn body_at(name: &str, line: usize, s: &Solid, place: &Transform, mesh: &Mesh) -> CadBody {
    let mut index = HashMap::new();
    let (mut positions, mut normals, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    for (t, (tri, ns)) in mesh.triangles.iter().zip(&mesh.normals).enumerate() {
        let face = mesh.face_of[t];
        for k in 0..3 {
            let i = *index.entry((tri[k], face)).or_insert_with(|| {
                let p = place.apply_point(mesh.positions[tri[k] as usize]);
                let n = place.apply_vec(ns[k]);
                positions.extend([p.x as f32, p.y as f32, p.z as f32]);
                normals.extend([n.x as f32, n.y as f32, n.z as f32]);
                (positions.len() / 3 - 1) as u32
            });
            indices.push(i);
        }
    }
    let faces = s
        .faces
        .iter()
        .map(|f| CadFace {
            name: f.name.clone(),
            surface: match f.surface {
                Surface::Plane(_) => "plane",
                Surface::Cylinder(_) => "cylinder",
                Surface::Cone(_) => "cone",
                Surface::Sphere(_) => "sphere",
                Surface::Torus(_) => "torus",
            }
            .into(),
            plane: None,
        })
        .collect();
    let ms = proton_cad::mass::mass(s).ok();
    CadBody {
        name: name.to_string(),
        line: line as u32,
        positions,
        normals,
        indices,
        face_of: mesh.face_of.clone(),
        faces,
        edges: mesh.edges.iter().map(|e| e.iter().flat_map(|p| {
            let q = place.apply_point(*p);
            [q.x as f32, q.y as f32, q.z as f32]
        }).collect()).collect(),
        edge_picks: vec![None; mesh.edges.len()],
        edge_turns: vec!["smooth".into(); mesh.edges.len()],
        volume: ms.map_or(0.0, |m| m.volume),
        area: ms.map_or(0.0, |m| m.area),
        centroid: ms.map_or(vec![0.0; 3], |m| {
            let c = place.apply_point(m.centroid);
            vec![c.x, c.y, c.z]
        }),
    }
}

/// The assembly as the window draws it.
#[allow(clippy::too_many_arguments)]
pub fn view(a: &lang::Assembly, m: &Model, path: &str, name: &str, generation: u32, clashes: Option<Vec<CadClash>>) -> CadAssemblyView {
    let k = m.units.mm();
    let item_at = |pred: &dyn Fn(&Item) -> bool| a.items.iter().position(|n| pred(&n.item)).unwrap_or(0);
    let line_of = |item: &Item| lang::write(&lang::Assembly { items: vec![Noted::new(item.clone())] }).trim_end().to_string();
    let error_at = |line: usize| m.errors.iter().find(|d| d.line == line).map(|d| d.message.clone());
    let instances = m
        .instances
        .iter()
        .map(|i| {
            let (at, turn) = i.place.placement();
            CadInstance {
                index: item_at(&|x| matches!(x, Item::Instance(y) if y.name == i.name)) as u32,
                line: i.line as u32,
                name: i.name.clone(),
                kind: if i.source == Source::Part { "part" } else { "assembly" }.into(),
                path: i.path.clone(),
                ground: i.ground,
                free: i.free,
                at: vec![at.x / k, at.y / k, at.z / k],
                turn: turn.to_vec(),
                error: error_at(i.line),
                parts: i.leaves.iter().map(|l| if i.source == Source::Part { i.name.clone() } else { format!("{}.{}", i.name, l.name) }).collect(),
            }
        })
        .collect();
    let mut mates = Vec::new();
    let mut explodes = Vec::new();
    let mut routes = Vec::new();
    for (idx, n) in a.items.iter().enumerate() {
        match &n.item {
            Item::Mate(x) => {
                let out = m.mates.iter().find(|o| o.name == x.name && o.line == n.line);
                mates.push(CadMate {
                    index: idx as u32,
                    line: n.line as u32,
                    name: x.name.clone(),
                    kind: x.kind.word().into(),
                    a: x.a.clone(),
                    b: x.b.clone(),
                    value: x.value.as_ref().map(print::expr),
                    residual: out.map_or(f64::NAN, |o| o.residual),
                    error: out.and_then(|o| o.error.clone()).or_else(|| error_at(n.line)),
                    redundant: out.is_some_and(|o| o.redundant),
                    text: line_of(&n.item),
                });
            }
            Item::Route(r) => {
                let out = m.routes.iter().find(|o| o.name == r.name && o.line == n.line);
                let b = out.and_then(|o| o.built.as_ref());
                routes.push(CadRoute {
                    index: idx as u32,
                    line: n.line as u32,
                    name: r.name.clone(),
                    kind: if r.tube == lang::Tube::Hard { "hard" } else { "soft" }.into(),
                    size: r.size.clone(),
                    from: r.from.clone(),
                    to: r.to.clone(),
                    via: r.via.iter().map(|v| v.to_vec()).collect(),
                    bend: r.bend.as_ref().map(print::expr),
                    lead: r.lead.as_ref().map(print::expr),
                    insert: r.insert.as_ref().map(print::expr),
                    length: b.map(|b| b.length),
                    cut: b.map(|b| b.cut),
                    bends: b.map(|b| b.bends.iter().map(|x| CadBend { at: x.at, angle: x.angle, radius: x.radius, twist: x.twist }).collect()).unwrap_or_default(),
                    error: out.and_then(|o| o.error.clone()),
                    text: line_of(&n.item),
                });
            }
            Item::Explode(e) => {
                let moves = m.explodes.iter().find(|x| x.0 == e.name).map(|x| x.1.iter().map(|(w, v)| CadMove { instance: w.clone(), by: vec![v.x, v.y, v.z] }).collect()).unwrap_or_default();
                explodes.push(CadExplode { index: idx as u32, line: n.line as u32, name: e.name.clone(), text: line_of(&n.item), moves });
            }
            _ => {}
        }
    }
    // Every part drawn where it is: each body cut into triangles once, then
    // put where each instance of it is.
    let mut bounds = Aabb::empty();
    let mut meshes: HashMap<(PathBuf, usize), Mesh> = HashMap::new();
    let mut bodies = Vec::new();
    for l in m.leaves() {
        let line = m.instances.iter().find(|i| l.name == i.name || l.name.starts_with(&format!("{}.", i.name))).map_or(0, |i| i.line);
        for (bi, (_, s)) in l.part.bodies.iter().enumerate() {
            let b = s.bounds();
            if b.is_empty() {
                continue;
            }
            for c in b.corners() {
                bounds.include(l.place.apply_point(c));
            }
            let mesh = meshes.entry((l.part.path.clone(), bi)).or_insert_with(|| tessellate(s, &Quality::view(b.diagonal().max(1.0))).unwrap_or_default());
            bodies.push(body_at(&l.name, line, s, &l.place, mesh));
        }
    }
    let units = a.items.iter().find_map(|n| if let Item::Units(u) = n.item { Some(u.word().to_string()) } else { None }).unwrap_or_else(|| "mm".into());
    CadAssemblyView {
        path: path.to_string(),
        name: name.to_string(),
        title: m.name.clone(),
        units,
        generation,
        dirty: false,
        text: String::new(),
        error: None,
        instances,
        mates,
        explodes,
        routes,
        coolant: CadLoop { parts: m.order.parts.iter().map(|p| p.0.clone()).collect(), routes: m.order.parts.iter().map(|p| p.1.clone()).collect(), closed: m.order.closed, notes: m.order.notes.clone() },
        bodies,
        dof: m.dof as u32,
        solved: m.solved,
        conflicting: m.conflicting.clone(),
        bom: m.bom().into_iter().map(|r| CadBomRow { part: r.part, path: r.path, material: r.material, count: r.count as u32, instances: r.instances, length: r.length }).collect(),
        clashes,
        clearance: m.clearance,
        errors: m.errors.iter().map(CadDiag::from).collect(),
        can_undo: false,
        can_redo: false,
        bounds: if bounds.is_empty() { vec![-50.0, -50.0, -50.0, 50.0, 50.0, 50.0] } else { vec![bounds.min.x, bounds.min.y, bounds.min.z, bounds.max.x, bounds.max.y, bounds.max.z] },
    }
}

/// Where the parts do not fit.
pub fn clashes(m: &Model) -> Vec<CadClash> {
    fit::fit(&m.leaves(), m.clearance)
        .into_iter()
        .map(|c| CadClash { a: c.a, b: c.b, overlap: c.overlap, volume: c.volume, gap: c.gap, at: vec![c.at.0.x, c.at.0.y, c.at.0.z, c.at.1.x, c.at.1.y, c.at.1.z] })
        .collect()
}

/// Every part's bodies where they are, named `part` or `part/body`.
pub fn placed_solids(m: &Model) -> Vec<(String, Solid)> {
    let mut out = Vec::new();
    for l in m.leaves() {
        let one = l.part.bodies.len() == 1;
        for (bn, s) in &l.part.bodies {
            out.push((if one { l.name.clone() } else { format!("{}/{bn}", l.name) }, s.transformed(&l.place)));
        }
    }
    out
}

/// The last instance's line, for a new one to follow; or the end.
pub fn after_instances(a: &lang::Assembly) -> u32 {
    a.items.iter().rposition(|n| matches!(n.item, Item::Instance(_) | Item::Ground(_) | Item::Place(_))).map_or(a.items.len(), |i| i + 1) as u32
}

/// A vector's three numbers.
pub fn xyz(v: Vec3) -> Vec<f64> {
    vec![v.x, v.y, v.z]
}

#[cfg(test)]
mod tests {
    use crate::bus::{Command, Event};
    use crate::cad::{CadOp, Cads};

    use super::CadAssemblyView;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-core-asm-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        std::fs::write(dir.join("parts/plate.part"), "part \"Plate\"\nmaterial aluminium-6061\nsketch s on XY\n  rect r corner origin size = (100, 60)\nend\nextrude plate from s distance 4\n").unwrap();
        std::fs::write(dir.join("parts/block.part"), "part \"Block\"\nsketch s on XY\n  rect r corner origin size = (20, 10)\nend\nextrude b from s distance 8\n").unwrap();
        dir
    }

    fn view(events: &[Event]) -> CadAssemblyView {
        events.iter().rev().find_map(|e| if let Event::CadAssembly { assembly: Some(a) } = e { Some((**a).clone()) } else { None }).expect("an assembly")
    }

    #[test]
    fn an_assembly_is_made_mated_solved_written_back_checked_and_written_out() {
        let root = scratch("build");
        let mut c = Cads::default();
        let v = view(&c.apply(&root, Command::CadNewAssembly { name: "Test rig".into() }));
        assert_eq!(v.title.as_deref(), Some("Test rig"));
        assert!(root.join("test-rig.assembly").exists());
        // The part it is not: none.
        let said = c.apply(&root, Command::CadSave);
        assert!(said.iter().any(|e| matches!(e, Event::CadPart { part: None })));
        let g = v.generation;
        let v = view(&c.apply(&root, Command::CadEdit { generation: g, ops: vec![CadOp::Insert { at: None, text: "clearance 1\npart plate from \"parts/plate.part\"\npart block from \"parts/block.part\"\nplace block at (30, 20, 15) turn (0, 0, 10)\nmate seat planar block.b.bottom to plate.plate.top\nmate left flush block.b.side(r.left) to plate.plate.side(r.left)\nmate front flush block.b.side(r.bottom) to plate.plate.side(r.bottom)".into() }] }));
        assert!(v.errors.is_empty(), "{:?}", v.errors);
        assert!(v.solved && v.dof == 0);
        assert!(v.text.contains("place block at (0, 0, 4)\n"), "the placement was not written back: {}", v.text);
        let block = v.instances.iter().find(|i| i.name == "block").unwrap();
        assert!(!block.free && block.at == [0.0, 0.0, 4.0]);
        assert!(v.instances[0].ground);
        assert_eq!(v.bodies.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["plate", "block"]);
        assert_eq!(v.bom.iter().map(|r| (r.part.as_str(), r.count)).collect::<Vec<_>>(), [("Plate", 1), ("Block", 1)]);
        assert!(v.clashes.is_none());
        // Renamed: every mate and placement follows.
        let i = v.instances.iter().find(|i| i.name == "block").unwrap().index;
        let v = view(&c.apply(&root, Command::CadEdit { generation: g, ops: vec![CadOp::Replace { index: i, text: "part brick from \"parts/block.part\"".into() }] }));
        assert!(v.text.contains("place brick at") && v.text.contains("mate seat planar brick.b.bottom") && !v.text.contains("block.b"), "{}", v.text);
        // Pushed into the plate: the fit check finds it.
        let place = v.text.lines().position(|l| l.starts_with("place brick")).unwrap();
        let idx = v.text.lines().take(place).count() as u32;
        let v = view(&c.apply(&root, Command::CadEdit { generation: g, ops: vec![CadOp::Remove { index: idx + 1 }, CadOp::Remove { index: idx + 1 }, CadOp::Remove { index: idx + 1 }, CadOp::Replace { index: idx, text: "place brick at (30, 20, 2)".into() }] }));
        assert!(v.mates.is_empty(), "{}", v.text);
        let v = view(&c.apply(&root, Command::CadFit { generation: g }));
        let clashes = v.clashes.unwrap();
        assert!(clashes.len() == 1 && clashes[0].overlap && (clashes[0].volume.unwrap() - 400.0).abs() < 1e-6, "{clashes:?}");
        // Written out exact, and its parts read back where they are.
        c.apply(&root, Command::CadExport { generation: g, format: "step".into() });
        let solids = proton_cad::step::read(&std::fs::read_to_string(root.join("test-rig.step")).unwrap()).unwrap();
        assert_eq!(solids.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["plate", "brick"]);
        assert!((proton_cad::mass::mass(&solids[1].1).unwrap().centroid.z - 6.0).abs() < 1e-9);
        // Unsaved until ⌘S; the home page lists it and its parts.
        assert_eq!(c.unsaved(), ["test-rig.assembly"]);
        c.apply(&root, Command::CadSave);
        assert!(c.unsaved().is_empty());
        let home = c.events(&root);
        let Some(Event::CadHome { parts }) = home.first() else { panic!() };
        assert_eq!(parts.iter().map(|p| (p.path.as_str(), p.kind.as_str())).collect::<Vec<_>>(), [("parts/block.part", "part"), ("parts/plate.part", "part"), ("test-rig.assembly", "assembly")]);
        // Opened again, it is as saved.
        c.apply(&root, Command::CadClose { discard: false });
        let v = view(&c.apply(&root, Command::CadOpen { path: "test-rig.assembly".into() }));
        assert!(v.text.contains("place brick at (30, 20, 2)") && !v.dirty);
        // A part's edit is not an assembly's.
        let said = c.apply(&root, Command::CadEdit { generation: v.generation, ops: vec![CadOp::Param { name: "x".into(), value: "1".into() }] });
        assert!(said.iter().any(|e| matches!(e, Event::Notice { .. })));
    }

    /// `cargo test -p proton-core --lib dump_assembly_for_the_preview -- --ignored`
    /// writes `ui/preview-cad-assembly.js`.
    #[test]
    #[ignore]
    fn dump_assembly_for_the_preview() {
        let root = scratch("preview");
        std::fs::write(root.join("parts/pin.part"), "part \"Pin\"\nmaterial steel\nsketch s on XY\n  circle c center (0, 0) radius = 2\nend\nextrude p from s distance 10\n").unwrap();
        std::fs::write(root.join("parts/fit.part"), proton_cad::library::make("fitting", &[]).unwrap().text).unwrap();
        let text = "assembly \"Test rig\"\nunits mm\nclearance 0.5\n\npart plate from \"parts/plate.part\"\npart block from \"parts/block.part\"\npart pin from \"parts/pin.part\"\nplace block at (0, 0, 4)\nplace pin at (60, 30, 4)\nmate seat planar block.b.bottom to plate.plate.top\nmate left flush block.b.side(r.left) to plate.plate.side(r.left)\nmate stand planar pin.p.bottom to plate.plate.top\nexplode apart block by (0, 0, 30), pin by (0, 0, 60)\npart f1 from \"parts/fit.part\"\npart f2 from \"parts/fit.part\"\nplace f1 at (12, 45, 4)\nplace f2 at (92, 45, 4)\nroute t1 hard 14/16 from f1.socket to f2.socket via (12, 45, 90), (92, 45, 90) bend 30\n";
        std::fs::write(root.join("rig.assembly"), text).unwrap();
        let mut c = Cads::default();
        let mut v = view(&c.apply(&root, Command::CadOpen { path: "rig.assembly".into() }));
        for b in &mut v.bodies {
            for x in b.positions.iter_mut().chain(b.normals.iter_mut()) {
                *x = (*x * 1e4).round() / 1e4;
            }
            for e in &mut b.edges {
                for x in e.iter_mut() {
                    *x = (*x * 1e4).round() / 1e4;
                }
            }
        }
        v.generation = 0;
        let json = serde_json::to_string(&v).unwrap();
        let out = format!("/* The sample assembly the browser preview shows, as the core draws it —\n * written by `cargo test -p proton-core --lib dump_assembly_for_the_preview -- --ignored`;\n * not edited by hand. */\n\n/** @type {{any}} */\nexport const ASSEMBLY = {json};\n");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/preview-cad-assembly.js");
        std::fs::write(&path, out).unwrap();
        println!("wrote {} ({} bytes)", path.display(), std::fs::metadata(&path).unwrap().len());
    }

    /// `cargo test -p proton-core --lib dump_drawing_for_the_preview -- --ignored`
    /// writes `ui/preview-cad-drawing.js`: the sample assembly's blueprint.
    #[test]
    #[ignore]
    fn dump_drawing_for_the_preview() {
        let root = scratch("preview-drawing");
        std::fs::write(root.join("parts/pin.part"), "part \"Pin\"\nmaterial steel\nsketch s on XY\n  circle c center (0, 0) radius = 2\nend\nextrude p from s distance 10\n").unwrap();
        let text = "assembly \"Test rig\"\nunits mm\nclearance 0.5\n\npart plate from \"parts/plate.part\"\npart block from \"parts/block.part\"\npart pin from \"parts/pin.part\"\nplace block at (0, 0, 4)\nplace pin at (60, 30, 4)\nmate seat planar block.b.bottom to plate.plate.top\nmate left flush block.b.side(r.left) to plate.plate.side(r.left)\nmate stand planar pin.p.bottom to plate.plate.top\nexplode apart block by (0, 0, 30), pin by (0, 0, 60)\n";
        std::fs::write(root.join("rig.assembly"), text).unwrap();
        let mut c = Cads::default();
        let g = view(&c.apply(&root, Command::CadOpen { path: "rig.assembly".into() })).generation;
        let mut d = c.apply(&root, Command::CadDraw { generation: g, blueprint: true }).into_iter().rev().find_map(|e| if let Event::CadDrawing { drawing: Some(d) } = e { Some(*d) } else { None }).expect("the blueprint");
        assert!(d.errors.is_empty(), "{:?}", d.errors);
        d.generation = 0;
        let json = serde_json::to_string(&d).unwrap();
        let out = format!("/* The sample assembly's blueprint the browser preview shows, as the core draws it —\n * written by `cargo test -p proton-core --lib dump_drawing_for_the_preview -- --ignored`;\n * not edited by hand. */\n\n/** @type {{any}} */\nexport const DRAWING = {json};\n");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/preview-cad-drawing.js");
        std::fs::write(&path, out).unwrap();
        println!("wrote {} ({} bytes)", path.display(), std::fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn a_library_part_is_written_into_the_project_and_opened_or_added() {
        let root = scratch("library");
        let mut c = Cads::default();
        let said = c.apply(&root, Command::CadLibrary { family: "fan".into(), params: vec![("size".into(), "140".into())], add: false });
        assert!(root.join("library/fan-140x25.part").exists());
        let part = said.iter().find_map(|e| if let Event::CadPart { part: Some(p) } = e { Some(p.clone()) } else { None }).expect("the part opened");
        assert_eq!(part.path, "library/fan-140x25.part");
        assert!(part.errors.is_empty() && part.bodies.len() == 1);
        // Made again the same: the same file; different: another.
        c.apply(&root, Command::CadClose { discard: false });
        c.apply(&root, Command::CadLibrary { family: "fan".into(), params: vec![("size".into(), "140".into())], add: false });
        assert!(!root.join("library/fan-140x25-2.part").exists());
        // Into an assembly, by its path from there.
        c.apply(&root, Command::CadClose { discard: false });
        std::fs::create_dir_all(root.join("builds")).unwrap();
        std::fs::write(root.join("builds/rig.assembly"), "assembly \"Rig\"\nunits mm\n").unwrap();
        let g = view(&c.apply(&root, Command::CadOpen { path: "builds/rig.assembly".into() })).generation;
        let v = view(&c.apply(&root, Command::CadLibrary { family: "fan".into(), params: vec![("size".into(), "140".into())], add: true }));
        assert_eq!(v.generation, g);
        assert!(v.text.contains("part fan_140x25 from \"../library/fan-140x25.part\""), "{}", v.text);
        assert!(v.errors.is_empty() && v.bodies.len() == 1, "{:?}", v.errors);
        let v = view(&c.apply(&root, Command::CadLibrary { family: "fan".into(), params: vec![("size".into(), "140".into())], add: true }));
        assert!(v.text.contains("part fan_140x252 from"), "{}", v.text);
        // The families are sent with the home page.
        assert!(c.events(&root).iter().any(|e| matches!(e, Event::CadFamilies { families } if families.iter().any(|f| f.name == "formd-t1"))));
        assert_eq!(super::relative("rig.assembly", "library/x.part"), "library/x.part");
        assert_eq!(super::relative("a/b/rig.assembly", "a/library/x.part"), "../library/x.part");
    }

    #[test]
    fn a_part_changed_on_disk_is_built_again() {
        let root = scratch("cache");
        let mut c = Cads::default();
        let g = view(&c.apply(&root, Command::CadNewAssembly { name: "c".into() })).generation;
        let v = view(&c.apply(&root, Command::CadEdit { generation: g, ops: vec![CadOp::Insert { at: None, text: "part block from \"parts/block.part\"".into() }] }));
        assert!((v.bodies[0].volume - 1600.0).abs() < 1e-6);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(root.join("parts/block.part"), "part \"Block\"\nsketch s on XY\n  rect r corner origin size = (20, 10)\nend\nextrude b from s distance 4\n").unwrap();
        let v = view(&c.apply(&root, Command::CadText { generation: g, text: format!("{}# looked at again\n", v.text) }));
        assert!((v.bodies[0].volume - 800.0).abs() < 1e-6, "{}", v.bodies[0].volume);
    }
}
