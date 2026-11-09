//! The agent's CAD (docs/CAD.md, K11): `cad_read`, `cad_write`,
//! `cad_measure` and `cad_check`, and what `@cad` sends.
//!
//! A file the agent writes is opened in CAD and held until the person saves
//! it, exactly as typing in the Text tab is: built, shown and checked the
//! same. What it is told back is what the window shows — the bodies and
//! their sizes, an assembly's freedoms and its fit, a drawing's views — and
//! every mistake at its line, so it can mend what it wrote.

use std::path::{Path, PathBuf};

use proton_cad::assembly::fit;
use proton_cad::brep::Solid;
use proton_cad::library::t1;
use proton_cad::mass::mass;
use proton_cad::mesh::tessellate;
use proton_cad::surface::Surface;
use proton_cad::vec::{Aabb, Vec3};

use super::{find, inside, model, part, CadDiag, Cads, Doc, Event, Item};

/// The agent's CAD tools.
pub const TOOLS: [&str; 4] = ["cad_read", "cad_write", "cad_measure", "cad_check"];

/// A number as the agent is told it: to a thousandth, the zeros dropped.
fn n(x: f64) -> String {
    let s = format!("{:.3}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn v3(p: Vec3) -> String {
    format!("({}, {}, {})", n(p.x), n(p.y), n(p.z))
}

/// What kind of CAD file a path is.
fn kind_of(path: &Path) -> Option<Doc> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(super::EXTENSION) => Some(Doc::Part),
        Some(crate::cad_assembly::EXTENSION) => Some(Doc::Assembly),
        Some(crate::cad_drawing::EXTENSION) => Some(Doc::Drawing),
        Some(crate::cad_schematic::EXTENSION) => Some(Doc::Schematic),
        Some(crate::cad_board::EXTENSION) => Some(Doc::Board),
        Some("step" | "stp" | "stl") => Some(Doc::Import),
        _ => None,
    }
}

fn kind_said(k: Doc) -> &'static str {
    match k {
        Doc::Part => "a part",
        Doc::Assembly => "an assembly",
        Doc::Drawing => "a drawing",
        Doc::Schematic => "a schematic",
        Doc::Board => "a board",
        Doc::Import => "another program's STEP or STL",
    }
}

/// A file's lines, numbered as the mistakes are.
fn numbered(text: &str) -> String {
    text.lines().enumerate().map(|(i, l)| format!("{:>4}  {l}", i + 1)).collect::<Vec<_>>().join("\n")
}

fn box_said(b: &Aabb) -> String {
    let e = b.max - b.min;
    format!("{} × {} × {} mm, from {} to {}", n(e.x), n(e.y), n(e.z), v3(b.min), v3(b.max))
}

/// A solid's size and mass properties, in words.
fn solid_said(s: &Solid) -> String {
    let b = s.bounds();
    match mass(s) {
        Ok(m) => format!("{} cm³, {} cm² of surface, its centre of volume at {}; {}", n(m.volume / 1000.0), n(m.area / 100.0), v3(m.centroid), box_said(&b)),
        Err(e) => format!("{}; its volume could not be found: {e}", box_said(&b)),
    }
}

/// A file read: the open one as typed, or from the project.
struct Source {
    file: PathBuf,
    shown: String,
    text: String,
    kind: Doc,
    open: bool,
}

/// A face found by name in solids `(owner, solid)`: an assembly's as
/// `INSTANCE.FACE`, a hole by its name alone.
fn face_in<'a>(solids: &'a [(String, Solid)], name: &str, assembly: bool) -> Option<(&'a str, &'a Solid, usize)> {
    let tries = |owner: Option<&str>, rest: &str| -> Option<(&'a str, &'a Solid, usize)> {
        for (o, s) in solids {
            if owner.is_some_and(|w| o != w && !o.starts_with(&format!("{w}/"))) {
                continue;
            }
            for n in [rest.to_string(), format!("{rest}.side(wall1)")] {
                if let Some(f) = s.named(&n).next() {
                    return Some((o.as_str(), s, f));
                }
            }
        }
        None
    };
    if assembly {
        let mut owners: Vec<&str> = solids.iter().map(|(o, _)| o.split('/').next().unwrap_or("")).collect();
        owners.sort_by_key(|o| std::cmp::Reverse(o.len()));
        owners.dedup();
        for o in owners {
            if let Some(rest) = name.strip_prefix(&format!("{o}.")) {
                if let Some(hit) = tries(Some(o), rest) {
                    return Some(hit);
                }
            }
        }
        None
    } else {
        tries(None, name)
    }
}

/// A face's outward normal at a point.
fn outward(s: &Solid, f: usize, p: Vec3) -> Vec3 {
    let g = s.faces[f].surface.gradient(p);
    if s.faces[f].reversed { -g } else { g }
}

/// A face's area from its triangles, and its middle.
fn face_area(s: &Solid, f: usize) -> (f64, Vec3) {
    let size = s.bounds().diagonal().max(1.0);
    let q = proton_cad::mesh::Quality { chord: size / 2000.0, angle: 6f64.to_radians() };
    let Ok(m) = tessellate(s, &q) else { return (0.0, Vec3::ZERO) };
    let (mut area, mut at) = (0.0, Vec3::ZERO);
    for (t, tri) in m.triangles.iter().enumerate() {
        if m.face_of[t] as usize != f {
            continue;
        }
        let [a, b, c] = tri.map(|i| m.positions[i as usize]);
        let w = (b - a).cross(c - a).length() / 2.0;
        area += w;
        at += (a + b + c) * (w / 3.0);
    }
    (area, if area > 0.0 { at / area } else { Vec3::ZERO })
}

/// A face in words: what surface it is, its size and where.
fn face_said(s: &Solid, f: usize) -> String {
    let (meshed, middle) = face_area(s, f);
    // Exact where it can be integrated; else from its triangles.
    let area = proton_cad::mass::face_areas(s).ok().and_then(|a| a.get(f).copied()).unwrap_or(meshed);
    let what = match &s.faces[f].surface {
        Surface::Plane(_) => format!("flat, facing {}", v3(outward(s, f, middle).unit_or(Vec3::Z))),
        Surface::Cylinder(c) => format!("a cylinder of diameter {} (radius {}) about the axis through {} along {}", n(c.radius * 2.0), n(c.radius), v3(c.frame.origin), v3(c.frame.z)),
        Surface::Cone(c) => format!("a cone of half-angle {}° about the axis through {} along {}", n(c.half_angle.to_degrees()), v3(c.frame.origin), v3(c.frame.z)),
        Surface::Sphere(sp) => format!("a sphere of radius {} about {}", n(sp.radius), v3(sp.frame.origin)),
        Surface::Torus(t) => format!("a torus, radii {} and {}, about {} along {}", n(t.major), n(t.minor), v3(t.frame.origin), v3(t.frame.z)),
        Surface::Free(_) => "a free-form surface, read from another program's file".to_string(),
    };
    format!("{what}; {} mm² ({} cm²), its middle near {}", n(area), n(area / 100.0), v3(middle))
}

/// Two faces measured: flat parallel faces' gap, axes' distance, the angle
/// between them, or the nearest their surfaces come.
fn between(a: (&Solid, usize), b: (&Solid, usize)) -> String {
    let (sa, fa) = a;
    let (sb, fb) = b;
    let (_, ma) = face_area(sa, fa);
    let (_, mb) = face_area(sb, fb);
    let (na, nb) = (outward(sa, fa, ma).unit_or(Vec3::Z), outward(sb, fb, mb).unit_or(Vec3::Z));
    let angle = na.dot(nb).clamp(-1.0, 1.0).acos().to_degrees();
    match (&sa.faces[fa].surface, &sb.faces[fb].surface) {
        (Surface::Plane(_), Surface::Plane(_)) if na.cross(nb).length() < 1e-9 => {
            format!("parallel, {} mm apart{}", n((mb - ma).dot(na).abs()), if na.dot(nb) < 0.0 { ", facing each other or back to back" } else { ", facing the same way" })
        }
        (Surface::Plane(_), Surface::Plane(_)) => format!("at {}° to each other (the angle between the faces, inside, is {}°)", n(angle), n(180.0 - angle)),
        (Surface::Cylinder(c), Surface::Cylinder(d)) => {
            let (u, w) = (c.frame.z, d.frame.z);
            if u.cross(w).length() < 1e-9 {
                let off = d.frame.origin - c.frame.origin;
                let dist = (off - u * off.dot(u)).length();
                if dist < 1e-6 { format!("on one axis (concentric), diameters {} and {}", n(c.radius * 2.0), n(d.radius * 2.0)) } else { format!("their axes parallel, {} mm apart", n(dist)) }
            } else {
                format!("their axes at {}° to each other", n(u.dot(w).abs().clamp(0.0, 1.0).acos().to_degrees()))
            }
        }
        (Surface::Cylinder(c), Surface::Plane(_)) | (Surface::Plane(_), Surface::Cylinder(c)) => {
            let n_ = if matches!(sa.faces[fa].surface, Surface::Plane(_)) { na } else { nb };
            let on = if matches!(sa.faces[fa].surface, Surface::Plane(_)) { ma } else { mb };
            if c.frame.z.dot(n_).abs() < 1e-9 {
                format!("the axis parallel to the face, {} mm from it", n((c.frame.origin - on).dot(n_).abs()))
            } else {
                format!("the axis at {}° to the face", n(90.0 - c.frame.z.dot(n_).abs().clamp(0.0, 1.0).acos().to_degrees()))
            }
        }
        _ => format!("at {}° to each other", n(angle)),
    }
}

impl Cads {
    /// What the agent's calls changed on screen since the last time asked.
    pub fn take_pending(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// An agent's call: its answer in words, or why there is none.
    pub fn tool(&mut self, root: &Path, name: &str, input: &serde_json::Value) -> Result<String, String> {
        let text = |k: &str| input.get(k).and_then(|v| v.as_str()).map(str::to_string);
        match name {
            "cad_read" => match text("path") {
                None => Ok(self.info(root)),
                Some(path) => {
                    let src = self.source(root, Some(&path))?;
                    Ok(format!("{} ({}){}\n{}\n\n{}", src.shown, kind_said(src.kind), if src.open { ", open in CAD as typed" } else { "" }, numbered(&src.text), self.said(root, &src)))
                }
            },
            "cad_write" => self.write(root, input),
            "cad_measure" => self.measure(root, input),
            "cad_check" => self.check(root, input),
            other => Err(format!("no such tool: {other}")),
        }
    }

    /// What `@cad` sends: the file open, numbered, and what it builds.
    pub fn about(&mut self, root: &Path) -> String {
        let Some(o) = &self.open else { return "Nothing is open in CAD.".into() };
        let src = Source { file: o.path.clone(), shown: o.shown.clone(), text: o.text.clone(), kind: o.kind, open: true };
        let unsaved = if o.text != o.saved { ", with unsaved changes" } else { "" };
        format!("CAD file {} ({}){unsaved}\n{}\n\n{}", src.shown, kind_said(src.kind), numbered(&src.text), self.said(root, &src))
    }

    /// The file open and the project's CAD files.
    fn info(&mut self, root: &Path) -> String {
        let files = find(root);
        let mut out = String::new();
        match &self.open {
            Some(o) => out.push_str(&format!("Open in CAD: {} ({}){}\n", o.shown, kind_said(o.kind), if o.text != o.saved { ", with unsaved changes" } else { "" })),
            None => out.push_str("Nothing is open in CAD.\n"),
        }
        if files.is_empty() {
            out.push_str("The project has no .part, .assembly, .drawing, .schematic or .board files: cad_write makes one.\n");
        } else {
            out.push_str("The project's CAD files:\n");
            for f in files.iter().take(200) {
                out.push_str(&format!("- {} ({})\n", f.path, f.kind));
            }
            if files.len() > 200 {
                out.push_str(&format!("and {} more\n", files.len() - 200));
            }
        }
        if self.open.is_some() {
            out.push('\n');
            out.push_str(&self.about(root));
        }
        out
    }

    /// The file named — the open one as typed — or the open one.
    fn source(&self, root: &Path, path: Option<&str>) -> Result<Source, String> {
        match path {
            None => {
                let o = self.open.as_ref().ok_or("nothing is open in CAD: give `path`")?;
                Ok(Source { file: o.path.clone(), shown: o.shown.clone(), text: o.text.clone(), kind: o.kind, open: true })
            }
            Some(p) => {
                let file = inside(root, p)?;
                let kind = kind_of(&file).ok_or_else(|| format!("{p}: a CAD file ends .part, .assembly, .drawing, .schematic or .board"))?;
                if let Some(o) = self.open.as_ref().filter(|o| o.path == file) {
                    return Ok(Source { file, shown: o.shown.clone(), text: o.text.clone(), kind, open: true });
                }
                let text = std::fs::read_to_string(&file).map_err(|e| format!("{p}: {e}"))?;
                Ok(Source { file, shown: p.replace('\\', "/"), text, kind, open: false })
            }
        }
    }

    /// What a file builds, and what is wrong with it, in words.
    fn said(&mut self, root: &Path, src: &Source) -> String {
        match src.kind {
            Doc::Part => part_said(&src.text),
            Doc::Assembly => {
                let a = match proton_cad::assembly::read(&src.text) {
                    Ok(a) => a,
                    Err(d) => return format!("It does not read: line {}: {}", d.line, d.message),
                };
                let m = self.parts.build(&a, &src.file, root);
                assembly_said(&m, root)
            }
            Doc::Schematic => match proton_board::read(&src.text) {
                Ok(s) => crate::cad_schematic::said(&proton_board::build::build_with(&s, &proton_board::kicad::Libraries::find(root))),
                Err(e) => format!("It does not read: line {}: {}", e.line, e.message),
            },
            Doc::Import => match super::read_import(&src.file, root) {
                Ok(bodies) => bodies.iter().map(|(n, s)| format!("- body {n}: {}\n", solid_said(s))).collect::<String>(),
                Err(e) => format!("It does not read: {e}"),
            },
            Doc::Board => match proton_board::board_lang::read(&src.text) {
                Ok(b) => crate::cad_board::said(&proton_board::board::build_with(&b, &src.file, &crate::cad_assembly::load_inside(root), &proton_board::kicad::Libraries::find(root))),
                Err(e) => format!("It does not read: line {}: {}", e.line, e.message),
            },
            Doc::Drawing => {
                let d = match proton_cad::drawing::lang::read(&src.text) {
                    Ok(d) => d,
                    Err(e) => return format!("It does not read: line {}: {}", e.line, e.message),
                };
                let built = self.parts.drawing(&d, &src.file, root);
                let v = crate::cad_drawing::view(&d, &built, &src.shown, "", 0);
                let mut out = format!("A {} sheet{}, of {}.\n", v.sheet, if v.blueprint { " drawn as a blueprint, white on blue" } else { "" }, v.of.as_deref().unwrap_or("nothing yet"));
                for it in &v.items {
                    out.push_str(&format!("- line {}: {}{}\n", it.line, it.text, it.error.as_ref().map_or(String::new(), |e| format!(" — {e}"))));
                }
                out.push_str(&mistakes(&v.errors));
                out
            }
        }
    }

    /// The agent's file written: opened in CAD, held until saved.
    fn write(&mut self, root: &Path, input: &serde_json::Value) -> Result<String, String> {
        let text = |k: &str| input.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let path = text("path").ok_or("cad_write: `path` is required")?;
        let file = inside(root, &path)?;
        let kind = kind_of(&file).ok_or_else(|| format!("{path}: a CAD file ends .part, .assembly, .drawing, .schematic or .board"))?;
        let whole = text("text");
        let (old, new) = (text("old_text"), text("new_text"));
        if whole.is_none() && (old.is_none() || new.is_none()) {
            return Err("cad_write: give `text`, the file whole, or `old_text` and `new_text`".into());
        }
        let opened = !self.open.as_ref().is_some_and(|o| o.path == file);
        let fresh = !file.exists();
        if opened {
            if let Some(o) = self.open.as_ref().filter(|o| o.text != o.saved) {
                return Err(format!("{} is open in CAD with unsaved changes: the person saves or discards them before another file is changed", o.shown));
            }
            let on_disk = match std::fs::read_to_string(&file) {
                Ok(t) => t,
                Err(_) if fresh && whole.is_some() => String::new(),
                Err(_) if fresh => return Err(format!("{path} is not there: give `text` to make it")),
                Err(e) => return Err(format!("{path}: {e}")),
            };
            if fresh {
                if let Some(dir) = file.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{path}: {e}"))?;
                }
            }
            self.show(root, file.clone(), on_disk, kind);
        }
        let open = self.open.as_mut().expect("opened");
        let changed = match whole {
            Some(t) => t,
            None => crate::review::substitute(&open.text, &old.unwrap_or_default(), &new.unwrap_or_default())?,
        };
        if changed != open.text {
            open.undo.push(std::mem::replace(&mut open.text, changed));
            open.redo.clear();
            open.typing = false;
            open.clashes = None;
        }
        let src = Source { file: open.path.clone(), shown: open.shown.clone(), text: open.text.clone(), kind, open: true };
        let lines = src.text.lines().count();
        let said = self.said(root, &src);
        let mut events = vec![Event::CadHome { parts: find(root) }];
        events.extend(self.doc_events());
        self.pending.extend(events);
        Ok(format!(
            "{path} ({}) {}: {lines} line{}, open in CAD and held until the person saves it with ⌘S{}.\n{said}",
            kind_said(kind),
            if fresh && opened { "made" } else { "changed" },
            if lines == 1 { "" } else { "s" },
            if fresh { " (the file is written then)" } else { "" }
        ))
    }

    /// Bodies, faces, or two of them, measured.
    fn measure(&mut self, root: &Path, input: &serde_json::Value) -> Result<String, String> {
        let text = |k: &str| input.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let src = self.source(root, text("path").as_deref())?;
        let (solids, assembly): (Vec<(String, Solid)>, bool) = match src.kind {
            Doc::Part => {
                let p = part::read(&src.text).map_err(|d| format!("line {} does not read: {}", d.line, d.message))?;
                (model::build(&p).bodies.into_iter().map(|b| (b.name, b.solid)).collect(), false)
            }
            Doc::Assembly => {
                let a = proton_cad::assembly::read(&src.text).map_err(|d| format!("line {} does not read: {}", d.line, d.message))?;
                let m = self.parts.build(&a, &src.file, root);
                (crate::cad_assembly::placed_solids(&m), true)
            }
            Doc::Import => (super::read_import(&src.file, root)?, false),
            Doc::Drawing => return Err("a drawing has nothing to measure: measure the part or assembly it is of".into()),
            Doc::Schematic => return Err("a schematic has no solids to measure: cad_check reads its nets and rules".into()),
            Doc::Board => return Err("a board is measured by its design rules: cad_check, or cad_read for its pads".into()),
        };
        if solids.is_empty() {
            return Err(format!("{} builds nothing yet to measure", src.shown));
        }
        // A body's, or an instance's, by its name.
        let whole = |name: &str| -> Vec<&Solid> { solids.iter().filter(|(o, _)| o == name || o.starts_with(&format!("{name}/"))).map(|(_, s)| s).collect() };
        let what = text("of");
        let to = text("to");
        match (what, to) {
            (None, Some(_)) => Err("cad_measure: `to` goes with `of`".into()),
            (None, None) => {
                let mut out = format!("{} — every {}:\n", src.shown, if assembly { "part where it is" } else { "body" });
                let mut all = Aabb::empty();
                for (name, s) in &solids {
                    let b = s.bounds();
                    if !b.is_empty() {
                        all.include(b.min);
                        all.include(b.max);
                    }
                    out.push_str(&format!("- {name}: {}\n", solid_said(s)));
                }
                if !all.is_empty() {
                    out.push_str(&format!("All of it: {}\n", box_said(&all)));
                }
                Ok(out)
            }
            (Some(a), None) => {
                let mine = whole(&a);
                if !mine.is_empty() {
                    return Ok(mine.iter().map(|s| format!("{a}: {}", solid_said(s))).collect::<Vec<_>>().join("\n"));
                }
                let (owner, s, f) = face_in(&solids, &a, assembly).ok_or_else(|| self.unknown(&solids, &a, assembly))?;
                Ok(format!("{a} (a face of {owner}): {}", face_said(s, f)))
            }
            (Some(a), Some(b)) => {
                let (wa, wb) = (whole(&a), whole(&b));
                if !wa.is_empty() && !wb.is_empty() {
                    let chord = 0.02;
                    let (d, pa, pb) = fit::nearest_between(&wa, &wb, chord).ok_or("they have no surfaces")?;
                    let touch = wa.iter().any(|x| wb.iter().any(|y| proton_cad::boolean::boolean(x, y, proton_cad::boolean::Op::Intersect).ok().and_then(|s| mass(&s).ok()).is_some_and(|m| m.volume > 1e-6)));
                    return Ok(if touch {
                        format!("{a} and {b} overlap: they share material")
                    } else {
                        format!("{a} and {b} come within {} mm of each other, at {} and {}", n(d), v3(pa), v3(pb))
                    });
                }
                let fa = face_in(&solids, &a, assembly).ok_or_else(|| self.unknown(&solids, &a, assembly))?;
                let fb = face_in(&solids, &b, assembly).ok_or_else(|| self.unknown(&solids, &b, assembly))?;
                Ok(format!("{a} and {b}: {}", between((fa.1, fa.2), (fb.1, fb.2))))
            }
        }
    }

    /// Why a name is not found, with the names there are.
    fn unknown(&self, solids: &[(String, Solid)], name: &str, assembly: bool) -> String {
        let mut names: Vec<String> = Vec::new();
        for (o, s) in solids {
            names.push(o.clone());
            for f in &s.faces {
                let n = if assembly { format!("{}.{}", o.split('/').next().unwrap_or(""), f.name) } else { f.name.clone() };
                if !f.name.is_empty() && !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        let shown: Vec<&str> = names.iter().map(String::as_str).take(80).collect();
        format!("`{name}` is no body{} or face here. There are: {}{}", if assembly { ", instance" } else { "" }, shown.join(", "), if names.len() > 80 { ", …" } else { "" })
    }

    /// The open file, or `path`, checked: a part's bodies closed and its
    /// sketches solved; an assembly's mates, fit, tubes and loop; and, if
    /// asked, the build against the FormD T1's published limits.
    fn check(&mut self, root: &Path, input: &serde_json::Value) -> Result<String, String> {
        let text = |k: &str| input.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let src = self.source(root, text("path").as_deref())?;
        let mut out = String::new();
        match src.kind {
            Doc::Part => {
                let p = part::read(&src.text).map_err(|d| format!("line {} does not read: {}", d.line, d.message))?;
                let m = model::build(&p);
                for s in &m.sketches {
                    let r = &s.report;
                    out.push_str(&match (&s.error, r.solved) {
                        (Some(e), _) => format!("- sketch {} (line {}): {}\n", s.name, s.line, e.message),
                        (None, false) => format!("- sketch {} (line {}): does not solve; its items {:?} cannot all hold\n", s.name, s.line, r.conflicting),
                        (None, true) if r.dof == 0 => format!("- sketch {} (line {}): solved, fully defined\n", s.name, s.line),
                        (None, true) => format!("- sketch {} (line {}): solved, {} degree{} of freedom left — free: {}\n", s.name, s.line, r.dof, if r.dof == 1 { "" } else { "s" }, r.free_points.join(", ")),
                    });
                }
                for b in &m.bodies {
                    let problems = proton_cad::brep::check::check(&b.solid);
                    out.push_str(&if problems.is_empty() { format!("- body {}: closed and whole\n", b.name) } else { format!("- body {}: {}\n", b.name, problems.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("; ")) });
                }
                if m.bodies.is_empty() {
                    out.push_str("It builds no body yet.\n");
                }
                out.push_str(&mistakes(&m.errors.iter().map(CadDiag::from).collect::<Vec<_>>()));
            }
            Doc::Assembly => {
                let a = proton_cad::assembly::read(&src.text).map_err(|d| format!("line {} does not read: {}", d.line, d.message))?;
                let m = self.parts.build(&a, &src.file, root);
                out.push_str(&if !m.solved {
                    format!("The mates cannot all hold: {} conflict.\n", m.conflicting.join(", "))
                } else if m.dof == 0 {
                    "The mates hold every part: nothing can move.\n".to_string()
                } else {
                    format!("The mates hold, with {} freedom{} left: free are {}.\n", m.dof, if m.dof == 1 { "" } else { "s" }, m.instances.iter().filter(|i| i.free).map(|i| i.name.as_str()).collect::<Vec<_>>().join(", "))
                });
                for mt in m.mates.iter().filter(|x| x.error.is_some() || x.redundant) {
                    out.push_str(&format!("- mate {} (line {}): {}\n", mt.name, mt.line, mt.error.clone().unwrap_or_else(|| "redundant: it holds nothing the others do not".into())));
                }
                let found = crate::cad_assembly::clashes(&m);
                if found.is_empty() {
                    out.push_str(&format!("Everything fits: no parts overlap, and none come nearer than the clearance, {} mm.\n", n(m.clearance)));
                } else {
                    for c in &found {
                        out.push_str(&if c.overlap {
                            format!("- {} and {} overlap{}\n", c.a, c.b, c.volume.map_or(String::new(), |v| format!(", by {} mm³", n(v))))
                        } else {
                            format!("- {} and {} are {} mm apart, nearer than the clearance\n", c.a, c.b, n(c.gap))
                        });
                    }
                }
                for r in &m.routes {
                    out.push_str(&match (&r.built, &r.error) {
                        (_, Some(e)) => format!("- tube {} (line {}): {e}\n", r.name, r.line),
                        (Some(b), None) => format!("- tube {}: {} from {} to {}, {} mm to cut, {} bend{}\n", r.name, r.size, r.from, r.to, n(b.cut), b.bends.len(), if b.bends.len() == 1 { "" } else { "s" }),
                        (None, None) => format!("- tube {}: not made\n", r.name),
                    });
                }
                if !m.routes.is_empty() {
                    out.push_str(&format!("The loop: {}{}\n", m.order.parts.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(" → "), if m.order.closed { ", closed" } else { "" }));
                    for note in &m.order.notes {
                        out.push_str(&format!("- {note}\n"));
                    }
                }
                out.push_str(&mistakes(&m.errors.iter().map(CadDiag::from).collect::<Vec<_>>()));
                // Shown on its page too, if it is the one open.
                if src.open {
                    if let Some(o) = self.open.as_mut() {
                        o.clashes = Some(found);
                    }
                    let shown = self.doc_events();
                    self.pending.extend(shown);
                }
            }
            Doc::Drawing | Doc::Schematic | Doc::Board => out.push_str(&self.said(root, &src)),
            Doc::Import => {
                for (n, s) in super::read_import(&src.file, root)? {
                    let problems = proton_cad::brep::check::check(&s);
                    out.push_str(&if problems.is_empty() { format!("- body {n}: closed and whole\n") } else { format!("- body {n}: {}\n", problems.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("; ")) });
                }
            }
        }
        if let Some(t) = input.get("t1") {
            out.push_str(&t1_said(t)?);
        }
        Ok(out)
    }
}

/// Mistakes, each at its line.
fn mistakes(errors: &[CadDiag]) -> String {
    if errors.is_empty() {
        return "No mistakes.\n".into();
    }
    let mut out = String::from("Mistakes:\n");
    for e in errors {
        out.push_str(&format!("- line {}: {}\n", e.line, e.message));
    }
    out
}

/// A part's bodies and sketches, and its mistakes.
fn part_said(text: &str) -> String {
    let p = match part::read(text) {
        Ok(p) => p,
        Err(d) => return format!("It does not read: line {}: {}", d.line, d.message),
    };
    let title = p.items.iter().find_map(|n| if let Item::Name(t) = &n.item { Some(t.clone()) } else { None });
    let m = model::build(&p);
    let mut out = String::new();
    if let Some(t) = title {
        out.push_str(&format!("\"{t}\""));
    }
    if let Some(mat) = &m.material {
        out.push_str(&format!(", {mat}"));
    }
    if !out.is_empty() {
        out.push('\n');
    }
    for s in &m.sketches {
        let r = &s.report;
        let state = match (&s.error, r.solved) {
            (Some(e), _) => e.message.clone(),
            (None, false) => "does not solve".into(),
            (None, true) if r.dof == 0 => "fully defined".into(),
            (None, true) => format!("{} free", r.dof),
        };
        out.push_str(&format!("- sketch {} (line {}): {state}\n", s.name, s.line));
    }
    if m.bodies.is_empty() {
        out.push_str("It builds no body yet.\n");
    }
    for b in &m.bodies {
        out.push_str(&format!("- body {} (line {}): {}\n", b.name, b.line, solid_said(&b.solid)));
    }
    if !m.threads.is_empty() {
        out.push_str(&format!("Tapped: {}\n", m.threads.iter().map(|(h, t)| format!("{h} {t}")).collect::<Vec<_>>().join(", ")));
    }
    for d in &m.notes {
        out.push_str(&format!("- line {} is not built yet: {}\n", d.line, d.message));
    }
    out.push_str(&mistakes(&m.errors.iter().map(CadDiag::from).collect::<Vec<_>>()));
    out
}

/// A path as the project names it.
fn relative(root: &Path, path: &str) -> String {
    let p = Path::new(path);
    let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    p.strip_prefix(root).or_else(|_| p.strip_prefix(&real)).map_or_else(|_| path.to_string(), |r| r.to_string_lossy().replace('\\', "/"))
}

/// An assembly's instances, mates, parts list and tubes, and its mistakes.
fn assembly_said(m: &proton_cad::assembly::Model, root: &Path) -> String {
    let mut out = String::new();
    for i in &m.instances {
        let (at, turn) = i.place.placement();
        let held = if i.ground { "held still" } else if i.free { "free to move" } else { "held by its mates" };
        out.push_str(&format!("- {} from {} (line {}): at {} turned ({}, {}, {})°, {held}\n", i.name, i.path, i.line, v3(at), n(turn[0]), n(turn[1]), n(turn[2])));
    }
    out.push_str(&format!(
        "{} mate{}; {}\n",
        m.mates.len(),
        if m.mates.len() == 1 { "" } else { "s" },
        if !m.solved { format!("they cannot all hold: {} conflict", m.conflicting.join(", ")) } else if m.dof == 0 { "nothing can move".into() } else { format!("{} freedom{} left", m.dof, if m.dof == 1 { "" } else { "s" }) }
    ));
    let bom = m.bom();
    if !bom.is_empty() {
        out.push_str("Bill of materials:\n");
        for (k, r) in bom.iter().enumerate() {
            out.push_str(&format!("{}. {} × {} ({}){}{}\n", k + 1, r.count, r.part, relative(root, &r.path), r.material.as_ref().map_or(String::new(), |x| format!(", {x}")), r.length.map_or(String::new(), |l| format!(", {} mm to cut", n(l)))));
        }
    }
    for r in &m.routes {
        out.push_str(&match (&r.built, &r.error) {
            (_, Some(e)) => format!("- tube {} (line {}): {e}\n", r.name, r.line),
            (Some(b), None) => format!("- tube {}: {} {} → {}, {} mm to cut\n", r.name, r.size, r.from, r.to, n(b.cut)),
            (None, None) => format!("- tube {}: not made\n", r.name),
        });
    }
    out.push_str(&mistakes(&m.errors.iter().map(CadDiag::from).collect::<Vec<_>>()));
    out
}

/// A build against the FormD T1's published limits.
fn t1_said(t: &serde_json::Value) -> Result<String, String> {
    let num = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_f64());
    let mut b = t1::Build::default();
    if let Some(g) = t.get("gpu") {
        let (Some(len), Some(slots)) = (num(g, "length"), num(g, "slots")) else { return Err("t1.gpu: give `length` and `slots`".into()) };
        b.gpu = Some((len, num(g, "height").unwrap_or(0.0), slots));
    }
    b.cooler = num(t, "cooler");
    b.psu = t.get("psu").and_then(|x| x.as_str()).map(str::to_lowercase);
    b.water = t.get("water").and_then(|x| x.as_bool()).unwrap_or(false);
    for r in t.get("radiators").and_then(|x| x.as_array()).into_iter().flatten() {
        let spot = match r.get("spot").and_then(|x| x.as_str()) {
            Some("top") => t1::Spot::Top,
            Some("bottom") => t1::Spot::Bottom,
            Some("side") => t1::Spot::Side,
            other => return Err(format!("t1.radiators: a spot is top, bottom or side, not {other:?}")),
        };
        let size = num(r, "size").ok_or("t1.radiators: give `size`, 120, 240 or 280")? as u32;
        b.radiators.push(t1::Radiator { spot, size, thickness: num(r, "thickness").unwrap_or(0.0) });
    }
    let found = t1::check(&b);
    if found.is_empty() {
        return Ok("Against the FormD T1's published limits: it fits.\n".into());
    }
    let mut out = String::from("Against the FormD T1's published limits:\n");
    for f in found {
        out.push_str(&format!("- {}{}\n", if f.fits { "caution: " } else { "does not fit: " }, f.says));
    }
    Ok(out)
}
