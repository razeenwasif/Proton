//! A drawing laid out on its sheet: each view projected and placed by its
//! middle, sections cut and hatched, details enlarged, dimensions measured
//! on the exact geometry and drawn where they are seen, the title block,
//! and for an assembly its bill of materials and balloons.
//!
//! [`standard`] and [`blueprint`] write a drawing's text for a part and for
//! an assembly: the views a sheet holds at a scale it holds them at, and for
//! an assembly its exploded view, a section through it, balloons, the bill
//! of materials and its tightest clearances dimensioned — the layout of the
//! build, its blueprint.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::assembly;
use crate::brep::Solid;
use crate::part::ast::{Diag, Expr};
use crate::part::expr::{eval, Env};
use crate::surface::Surface;
use crate::vec::{Aabb, Vec3};

use super::lang::{DimKind, Drawing, Item};
use super::view::{Cut, Dir, Lines, Scene};
use super::{size, Anchor, Pen, Prim, Sheet, Style, P2};

/// What a drawing is of, built.
enum Target {
    Part(Box<crate::model::Model>),
    Assembly(Box<assembly::Model>),
}

/// A solid where it is drawn, and whose it is: a body's name, or an
/// instance's.
struct Piece {
    owner: String,
    solid: Solid,
}

/// Scenes and views kept between builds of a drawing: neither depends on
/// where a view sits on the sheet, its scale or the dimensions, only on the
/// solids and which way they are seen — so moving a view or adding a
/// dimension projects nothing again.
#[derive(Default)]
pub struct Cache {
    scenes: HashMap<u64, Arc<Scene>>,
    lines: HashMap<u64, Arc<Lines>>,
}

impl Cache {
    /// How many scenes and views are kept: what a test reads to see a
    /// build made nothing again.
    pub fn sizes(&self) -> (usize, usize) {
        (self.scenes.len(), self.lines.len())
    }

    /// Kept small: the last few drawings' worth.
    fn trim(&mut self) {
        if self.scenes.len() > 24 {
            self.scenes.clear();
        }
        if self.lines.len() > 96 {
            self.lines.clear();
        }
    }
}

fn hash_of(parts: impl FnOnce(&mut std::collections::hash_map::DefaultHasher)) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts(&mut h);
    h.finish()
}

impl Target {
    fn pieces(&self) -> Vec<Piece> {
        match self {
            Target::Part(m) => m.bodies.iter().map(|b| Piece { owner: b.name.clone(), solid: b.solid.clone() }).collect(),
            Target::Assembly(m) => m.leaves().iter().flat_map(|l| l.part.bodies.iter().map(move |(_, s)| Piece { owner: l.name.clone(), solid: s.transformed(&l.place) })).collect(),
        }
    }
}

/// A view as placed: how a point of the model lands on the sheet.
#[derive(Clone, Copy)]
struct Place {
    eye: Vec3,
    u: Vec3,
    v: Vec3,
    /// The model's point that lands at `at`, on the view's plane.
    mid: P2,
    at: P2,
    scale: f64,
    /// Its box on the sheet.
    lo: P2,
    hi: P2,
}

impl Place {
    fn sheet(&self, p: Vec3) -> P2 {
        self.at + (P2::new(p.dot(self.u), p.dot(self.v)) - self.mid) * self.scale
    }

    fn map(&self, q: P2) -> P2 {
        self.at + (q - self.mid) * self.scale
    }
}

/// A drawing built: its sheet, and what went wrong at which line.
#[derive(Debug, Clone)]
pub struct Built {
    pub sheet: Sheet,
    pub errors: Vec<Diag>,
    pub title: Option<String>,
    /// The faces a dimension or note can name, as it names them.
    pub faces: Vec<String>,
    /// An assembly's parts, as a clearance names them.
    pub parts: Vec<String>,
}

/// Reads a file the drawing names, relative to it.
pub type Load<'a> = &'a dyn Fn(&Path) -> Result<String, String>;

fn text(prims: &mut Vec<Prim>, at: P2, size: f64, s: impl Into<String>, anchor: Anchor, angle: f64, pen: Pen) {
    prims.push(Prim::Text { at, size, text: s.into(), anchor, angle, pen });
}

fn line(prims: &mut Vec<Prim>, a: P2, b: P2, pen: Pen) {
    prims.push(Prim::Line { a, b, pen });
}

/// An arrowhead at `tip`, pointing along `dir`.
fn arrow(prims: &mut Vec<Prim>, tip: P2, dir: P2) {
    let l = (dir.x.hypot(dir.y)).max(1e-12);
    let d = P2::new(dir.x / l, dir.y / l);
    let n = P2::new(-d.y, d.x);
    let base = tip - d * 3.0;
    prims.push(Prim::Fill { pts: vec![tip, base + n * 0.6, base - n * 0.6], pen: Pen::Dim });
}

/// A number as a dimension says it: to a hundredth, the zeros dropped.
pub fn measure(x: f64) -> String {
    let s = format!("{:.2}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

/// A dimension between `a` and `b` on the sheet: its line `off` out along
/// `side` (a unit vector), extension lines, arrows, and its text.
fn aligned(prims: &mut Vec<Prim>, a: P2, b: P2, side: P2, off: f64, words: &str) {
    let (da, db) = (a + side * off, b + side * off);
    if off.abs() > 0.5 {
        let over = side * 1.5 * off.signum();
        line(prims, a + side * (0.8 * off.signum()), da + over, Pen::Dim);
        line(prims, b + side * (0.8 * off.signum()), db + over, Pen::Dim);
    }
    line(prims, da, db, Pen::Dim);
    let dir = db - da;
    arrow(prims, db, dir);
    arrow(prims, da, dir * -1.0);
    let mut angle = dir.y.atan2(dir.x).to_degrees();
    if angle > 90.0 + 1e-9 {
        angle -= 180.0;
    } else if angle <= -90.0 + 1e-9 {
        angle += 180.0;
    }
    let up = P2::new(-angle.to_radians().sin(), angle.to_radians().cos());
    text(prims, (da + db) * 0.5 + up * 1.2, 3.5, words, Anchor::Middle, angle, Pen::Dim);
}

/// A leader from `to` out to `from`, a shelf, and the words.
fn leader(prims: &mut Vec<Prim>, to: P2, from: P2, words: &str) {
    line(prims, from, to, Pen::Dim);
    arrow(prims, to, to - from);
    let right = from.x >= to.x;
    let end = from + P2::new(if right { 6.0 } else { -6.0 }, 0.0);
    line(prims, from, end, Pen::Dim);
    text(prims, end + P2::new(if right { 1.0 } else { -1.0 }, -1.2), 3.5, words, if right { Anchor::Start } else { Anchor::End }, 0.0, Pen::Dim);
}

/// A face by reference — `plate.top`, a hole `m3`, an assembly's
/// `board.plate.top` — and the solid it is on, where it is drawn.
fn face<'a>(pieces: &'a [Piece], r: &str) -> Option<(&'a Solid, usize)> {
    let tries: Vec<(Option<&str>, String)> = {
        let mut t = vec![(None, r.to_string()), (None, format!("{r}.side(wall1)"))];
        // An instance's face: the instance's name first.
        for p in pieces {
            if let Some(rest) = r.strip_prefix(&format!("{}.", p.owner)) {
                t.push((Some(p.owner.as_str()), rest.to_string()));
                t.push((Some(p.owner.as_str()), format!("{rest}.side(wall1)")));
            }
        }
        t
    };
    for (owner, name) in &tries {
        for p in pieces {
            if owner.is_some_and(|o| o != p.owner) {
                continue;
            }
            if let Some(f) = p.solid.named(name).next() {
                return Some((&p.solid, f));
            }
        }
    }
    None
}

/// A face's middle: the centroid of its edges' points.
fn middle(s: &Solid, f: usize) -> Vec3 {
    let pts: Vec<Vec3> = s.faces[f].loops.iter().flatten().flat_map(|c| s.edges[c.edge].sample(8)).collect();
    pts.iter().fold(Vec3::ZERO, |a, &b| a + b) / pts.len().max(1) as f64
}

fn outward(s: &Solid, f: usize, p: Vec3) -> Vec3 {
    let g = s.faces[f].surface.gradient(p);
    if s.faces[f].reversed {
        -g
    } else {
        g
    }
}

/// The scale an expression says: `1/2`.
fn scale_of(e: &Option<Expr>, env: &Env, default: f64) -> Result<f64, String> {
    match e {
        None => Ok(default),
        Some(e) => {
            let s = eval(e, env)?;
            if s > 0.0 {
                Ok(s)
            } else {
                Err(format!("a scale is more than nothing, not {s}"))
            }
        }
    }
}

/// Lines of a view, on the sheet.
fn draw_lines(prims: &mut Vec<Prim>, lines: &Lines, place: &Place) {
    for (runs, pen) in [(&lines.hatch, Pen::Thin), (&lines.hidden, Pen::Hidden), (&lines.center, Pen::Center), (&lines.visible, Pen::Visible)] {
        for r in runs {
            prims.push(Prim::Poly { pts: r.iter().map(|&q| place.map(q)).collect(), closed: false, pen });
        }
        if matches!(pen, Pen::Hidden | Pen::Visible) {
            for &(c, r, seen) in &lines.circles {
                if seen == (pen == Pen::Visible) {
                    prims.push(Prim::Circle { c: place.map(c), r: r * place.scale, pen });
                }
            }
        }
    }
}

/// A circle as a closed run of points, for clipping.
fn ring(c: P2, r: f64) -> Vec<P2> {
    (0..=120).map(|k| {
        let a = std::f64::consts::TAU * k as f64 / 120.0;
        c + P2::new(a.cos(), a.sin()) * r
    }).collect()
}

/// The drawing `d`, which is the file `here`, laid out.
pub fn build(d: &Drawing, here: &Path, load: Load) -> Built {
    build_with(d, here, load, &mut HashMap::new())
}

/// The drawing laid out, an assembly's parts taken from `parts` where
/// they are built already, and those it builds left there.
pub fn build_with(d: &Drawing, here: &Path, load: Load, parts: &mut HashMap<PathBuf, Arc<assembly::model::Built>>) -> Built {
    build_cached(d, here, load, parts, &mut Cache::default())
}

/// As [`build_with`], scenes and views taken from `cache` where what they
/// show is as it was, and those made left there.
pub fn build_cached(d: &Drawing, here: &Path, load: Load, parts: &mut HashMap<PathBuf, Arc<assembly::model::Built>>, cache: &mut Cache) -> Built {
    cache.trim();
    let mut errors: Vec<Diag> = Vec::new();
    let mut title = None;
    let (mut sheet_name, mut scale_expr, mut first, mut blue) = ("A3".to_string(), None, false, false);
    let mut of: Option<(String, usize)> = None;
    for n in &d.items {
        match &n.item {
            Item::Name(s) => title = Some(s.clone()),
            Item::Of(s) => of = Some((s.clone(), n.line)),
            Item::Sheet(s) => sheet_name = s.replace('_', "-"),
            Item::Scale(e) => scale_expr = Some(e.clone()),
            Item::Angle(f) => first = *f,
            Item::Blueprint(b) => blue = *b,
            _ => {}
        }
    }
    let env = Env::new();
    let (w, h) = size(&sheet_name).unwrap_or((420.0, 297.0));
    let mut sheet = Sheet { width: w, height: h, style: if blue { Style::Blueprint } else { Style::Paper }, prims: Vec::new(), boxes: Vec::new() };
    let base = scale_of(&scale_expr, &env, 1.0).unwrap_or_else(|e| {
        errors.push(Diag::new(1, 1, e));
        1.0
    });
    // What it is of.
    let dir = here.parent().unwrap_or(Path::new("")).to_path_buf();
    let mut text_hash = 0u64;
    let target = match &of {
        None => {
            errors.push(Diag::new(1, 1, "a drawing says what it is of: `of \"bracket.part\"`"));
            None
        }
        Some((file, line)) => {
            let path = dir.join(file);
            let got = (|| -> Result<Target, String> {
                let t = load(&path)?;
                use std::hash::Hash;
                text_hash = hash_of(|h| t.hash(h));
                if file.ends_with(".assembly") {
                    let a = assembly::read(&t).map_err(|d| format!("line {} of {file} does not read: {}", d.line, d.message))?;
                    Ok(Target::Assembly(Box::new(assembly::model::build_with(&a, &path, load, parts))))
                } else {
                    let p = crate::part::read(&t).map_err(|d| format!("line {} of {file} does not read: {}", d.line, d.message))?;
                    Ok(Target::Part(Box::new(crate::model::build(&p))))
                }
            })();
            match got {
                Ok(t) => Some(t),
                Err(e) => {
                    errors.push(Diag::new(*line, 1, e));
                    None
                }
            }
        }
    };
    let pieces = target.as_ref().map(|t| t.pieces()).unwrap_or_default();
    // What the solids are: the file's text, and each part an assembly holds
    // — what it was built from, and where it is — so a part changed is a
    // new scene.
    let solids_hash = hash_of(|h| {
        use std::hash::Hash;
        text_hash.hash(h);
        if let Some(Target::Assembly(m)) = &target {
            for l in m.leaves() {
                l.name.hash(h);
                // What it was built from, not where it sits in memory: a
                // part read again may land where another was.
                l.part.stamp.hash(h);
                l.part.path.hash(h);
                let (at, turn) = l.place.placement();
                for x in [at.x, at.y, at.z, turn[0], turn[1], turn[2]] {
                    x.to_bits().hash(h);
                }
            }
        }
    });
    let assembly = matches!(target, Some(Target::Assembly(_)));
    let mut faces = std::collections::BTreeSet::new();
    let mut parts = std::collections::BTreeSet::new();
    for p in &pieces {
        for f in p.solid.faces.iter().filter(|f| !f.name.is_empty()) {
            faces.insert(if assembly { format!("{}.{}", p.owner, f.name) } else { f.name.clone() });
        }
        if assembly {
            parts.insert(p.owner.clone());
            parts.insert(p.owner.split('.').next().unwrap_or("").to_string());
        }
    }
    let (faces, parts): (Vec<String>, Vec<String>) = (faces.into_iter().take(4000).collect(), parts.into_iter().collect());
    frame(&mut sheet, title.as_deref().unwrap_or(""), of.as_ref().map_or("", |o| o.0.as_str()), &target, base, &sheet_name, first);
    if pieces.is_empty() {
        return Built { sheet, errors, title, faces, parts };
    }

    // The views.
    let mut places: HashMap<String, Place> = HashMap::new();
    let mut view_lines: HashMap<String, Lines> = HashMap::new();
    // Each view's name goes under it, and under its dimensions: after them.
    let mut labels: Vec<(String, String)> = Vec::new();
    let mut below: HashMap<String, f64> = HashMap::new();
    // First what each view is — its solids, which way they are seen, and
    // the keys what is drawn of them is kept by; then whatever is not kept
    // yet made at once, every view on its own core; then each placed on the
    // sheet in the file's order.
    struct Job<'a> {
        item: &'a crate::part::ast::Noted<Item>,
        name: String,
        dir: Dir,
        at: [f64; 2],
        scale: f64,
        hidden: bool,
        exploded: bool,
        cut: Option<Cut>,
        moved: Vec<Solid>,
        scene_key: u64,
        line_key: u64,
    }
    let mut jobs: Vec<Job> = Vec::new();
    let mut named = std::collections::HashSet::new();
    for n in &d.items {
        let line_no = n.line;
        let (name, dir_, at, scale, hidden, exploded, cut) = match &n.item {
            Item::View(v) => (v.name.clone(), v.dir, v.at, &v.scale, v.hidden, v.exploded.clone(), None),
            Item::Section(s) => {
                let value = match eval(&s.value, &env) {
                    Ok(x) => x,
                    Err(e) => {
                        errors.push(Diag::new(line_no, 1, e));
                        continue;
                    }
                };
                let axis = match s.axis {
                    'x' => Vec3::X,
                    'y' => Vec3::Y,
                    _ => Vec3::Z,
                };
                // Seen from the low side: what is on that side is taken away.
                let toward = if s.high { axis } else { -axis };
                (s.name.clone(), Dir::along(toward), s.at, &s.scale, false, None, Some(Cut { point: axis * value, toward }))
            }
            _ => continue,
        };
        if !named.insert(name.clone()) {
            errors.push(Diag::new(line_no, 1, format!("`{name}` is a view's name already")));
            continue;
        }
        let s = match scale_of(scale, &env, base) {
            Ok(s) => s,
            Err(e) => {
                errors.push(Diag::new(line_no, 1, e));
                continue;
            }
        };
        // An exploded view moves the instances.
        let moved: Vec<Solid> = match (&exploded, &target) {
            (Some(e), Some(Target::Assembly(m))) => match m.explodes.iter().find(|x| &x.0 == e) {
                Some((_, moves)) => pieces
                    .iter()
                    .map(|p| {
                        let top = p.owner.split('.').next().unwrap_or("");
                        let by = moves.iter().find(|m| m.0 == top || m.0 == p.owner).map_or(Vec3::ZERO, |m| m.1);
                        p.solid.transformed(&crate::frame::Transform::translation(by))
                    })
                    .collect(),
                None => {
                    errors.push(Diag::new(line_no, 1, format!("no exploded view called `{e}`")));
                    continue;
                }
            },
            (Some(_), _) => {
                errors.push(Diag::new(line_no, 1, "only an assembly comes apart"));
                continue;
            }
            _ => Vec::new(),
        };
        // The same solids are made ready once for all their views; a
        // section's are its own.
        let key = match (&cut, &exploded) {
            (Some(_), _) => format!("\u{0}{name}"),
            (None, e) => e.clone().unwrap_or_default(),
        };
        let scene_key = hash_of(|h| {
            use std::hash::Hash;
            solids_hash.hash(h);
            key.hash(h);
            if let Some(c) = &cut {
                for x in [c.point.x, c.point.y, c.point.z, c.toward.x, c.toward.y, c.toward.z] {
                    x.to_bits().hash(h);
                }
            }
        });
        let line_key = hash_of(|h| {
            use std::hash::Hash;
            scene_key.hash(h);
            format!("{dir_:?}").hash(h);
            hidden.hash(h);
            (3.0 / s).to_bits().hash(h);
        });
        jobs.push(Job { item: n, name, dir: dir_, at, scale: s, hidden, exploded: exploded.is_some(), cut, moved, scene_key, line_key });
    }
    // The scenes not kept, each once.
    let mut wanted: Vec<&Job> = Vec::new();
    for j in &jobs {
        if !cache.scenes.contains_key(&j.scene_key) && !wanted.iter().any(|w| w.scene_key == j.scene_key) {
            wanted.push(j);
        }
    }
    let mut failed: HashMap<u64, String> = HashMap::new();
    let made = crate::par::map(&wanted, |j| {
        let refs: Vec<&Solid> = if j.moved.is_empty() { pieces.iter().map(|p| &p.solid).collect() } else { j.moved.iter().collect() };
        Scene::new(&refs, j.cut)
    });
    for (j, sc) in wanted.iter().zip(made) {
        match sc {
            Ok(sc) => {
                cache.scenes.insert(j.scene_key, Arc::new(sc));
            }
            Err(e) => {
                failed.insert(j.scene_key, e);
            }
        }
    }
    // The views not kept, each once.
    let mut wanted: Vec<&Job> = Vec::new();
    for j in &jobs {
        if !failed.contains_key(&j.scene_key) && !cache.lines.contains_key(&j.line_key) && !wanted.iter().any(|w| w.line_key == j.line_key) {
            wanted.push(j);
        }
    }
    let drawn = crate::par::map(&wanted, |j| cache.scenes[&j.scene_key].lines(j.dir, j.hidden, 3.0 / j.scale));
    for (j, l) in wanted.iter().zip(drawn) {
        cache.lines.insert(j.line_key, Arc::new(l));
    }
    for j in jobs {
        let (name, s, cut) = (j.name, j.scale, j.cut);
        if let Some(e) = failed.get(&j.scene_key) {
            errors.push(Diag::new(j.item.line, 1, format!("`{name}` cannot be drawn: {e}")));
            continue;
        }
        let lines = (*cache.lines[&j.line_key]).clone();
        let (eye, u, v) = match &cut {
            Some(c) => Dir::along(c.toward).axes(),
            None => j.dir.axes(),
        };
        let mid = (lines.min + lines.max) * 0.5;
        let at = P2::new(j.at[0], j.at[1]);
        let place = Place { eye, u, v, mid, at, scale: s, lo: at + (lines.min - mid) * s, hi: at + (lines.max - mid) * s };
        draw_lines(&mut sheet.prims, &lines, &place);
        // Its name under it.
        let label = match &j.item.item {
            Item::Section(_) => format!("SECTION {}–{}", name.to_uppercase(), name.to_uppercase()),
            _ if j.exploded => format!("{} — EXPLODED", name.to_uppercase()),
            _ => name.to_uppercase(),
        };
        let scale_words = if (s - base).abs() > 1e-9 { format!(" ({})", ratio(s)) } else { String::new() };
        labels.push((name.clone(), format!("{label}{scale_words}")));
        sheet.boxes.push((name.clone(), [place.lo.x, place.lo.y, place.hi.x, place.hi.y]));
        places.insert(name.clone(), place);
        view_lines.insert(name, lines);
    }

    // Details: a circle of a view, larger.
    for n in &d.items {
        let Item::Detail(x) = &n.item else { continue };
        let (Some(parent), Some(lines)) = (places.get(&x.of).copied(), view_lines.get(&x.of)) else {
            errors.push(Diag::new(n.line, 1, format!("no view called `{}` to take a detail of", x.of)));
            continue;
        };
        let (r, k) = match (eval(&x.radius, &env), eval(&x.scale, &env)) {
            (Ok(r), Ok(k)) if r > 0.0 && k > 0.0 => (r, k),
            _ => {
                errors.push(Diag::new(n.line, 1, "a detail's radius and scale are more than nothing"));
                continue;
            }
        };
        let c = P2::new(x.at[0], x.at[1]);
        let to = P2::new(x.place[0], x.place[1]);
        sheet.prims.push(Prim::Circle { c, r, pen: Pen::Thin });
        text(&mut sheet.prims, c + P2::new(r * 0.75, r * 0.75 + 1.0), 3.5, x.name.to_uppercase(), Anchor::Start, 0.0, Pen::Dim);
        let mut put = |runs: &Vec<Vec<P2>>, pen: Pen| {
            for run in runs {
                let pts: Vec<P2> = run.iter().map(|&q| parent.map(q)).collect();
                for piece in clip_circle(&pts, c, r) {
                    sheet.prims.push(Prim::Poly { pts: piece.into_iter().map(|p| to + (p - c) * k).collect(), closed: false, pen });
                }
            }
        };
        let rings = |seen: bool| -> Vec<Vec<P2>> { lines.circles.iter().filter(|c| c.2 == seen).map(|&(c, r, _)| ring(c, r)).collect() };
        put(&lines.visible, Pen::Visible);
        put(&rings(true), Pen::Visible);
        put(&lines.hidden, Pen::Hidden);
        put(&rings(false), Pen::Hidden);
        put(&lines.center, Pen::Center);
        put(&lines.hatch, Pen::Thin);
        sheet.prims.push(Prim::Circle { c: to, r: r * k, pen: Pen::Thin });
        text(&mut sheet.prims, P2::new(to.x, to.y - r * k - 7.0), 3.5, format!("DETAIL {} ({})", x.name.to_uppercase(), ratio(k * parent.scale)), Anchor::Middle, 0.0, Pen::Dim);
        sheet.boxes.push((x.name.clone(), [to.x - r * k, to.y - r * k, to.x + r * k, to.y + r * k]));
    }

    // Dimensions and notes.
    for n in &d.items {
        let (view, what) = match &n.item {
            Item::Dim(x) => (&x.view, x.name.as_str()),
            Item::Note(x) => (&x.view, x.name.as_str()),
            _ => continue,
        };
        let Some(pl) = places.get(view).copied() else {
            errors.push(Diag::new(n.line, 1, format!("no view called `{view}`")));
            continue;
        };
        let before = sheet.prims.len();
        let drawn = match &n.item {
            Item::Dim(x) => dimension(&mut sheet.prims, &pieces, &pl, x, &env, &target),
            Item::Note(x) => face(&pieces, &x.at).map(|(s, f)| {
                let p = pl.sheet(middle(s, f));
                leader(&mut sheet.prims, p, p + P2::new(10.0, 10.0), &x.text);
            }).ok_or_else(|| format!("`{}` names no face or hole", x.at)),
            _ => unreachable!(),
        };
        if let Err(e) = drawn {
            errors.push(Diag::new(n.line, 1, format!("`{what}`: {e}")));
        }
        // What it drew under the view pushes the view's name down.
        if let Some((lo, hi)) = extent(&sheet.prims[before..]) {
            if hi.y <= pl.lo.y + 1.0 && lo.x < pl.hi.x && hi.x > pl.lo.x {
                let b = below.entry(view.clone()).or_insert(pl.lo.y);
                *b = b.min(lo.y);
            }
        }
    }
    for (name, words) in labels {
        let pl = &places[&name];
        let y = below.get(&name).copied().unwrap_or(pl.lo.y).min(pl.lo.y) - 7.0;
        text(&mut sheet.prims, P2::new(pl.at.x, y), 3.5, words, Anchor::Middle, 0.0, Pen::Dim);
    }

    // The bill of materials, and its items pointed at.
    let bom = match &target {
        Some(Target::Assembly(m)) => m.bom(),
        _ => Vec::new(),
    };
    for n in &d.items {
        match &n.item {
            Item::Bom(at) => {
                if bom.is_empty() {
                    errors.push(Diag::new(n.line, 1, "only an assembly has a bill of materials"));
                    continue;
                }
                table(&mut sheet, &bom, P2::new(at[0], at[1]));
            }
            Item::Balloons(v) => {
                let Some(pl) = places.get(v).copied() else {
                    errors.push(Diag::new(n.line, 1, format!("no view called `{v}`")));
                    continue;
                };
                if bom.is_empty() {
                    errors.push(Diag::new(n.line, 1, "only an assembly's parts are ballooned"));
                    continue;
                }
                balloons(&mut sheet.prims, &pieces, &bom, &pl, (sheet.width, sheet.height));
            }
            _ => {}
        }
    }
    Built { sheet, errors, title, faces, parts }
}

/// `0.5` → `1:2`; `2` → `2:1`.
pub fn ratio(s: f64) -> String {
    if s >= 1.0 {
        format!("{}:1", measure(s))
    } else {
        format!("1:{}", measure(1.0 / s))
    }
}

/// The box round what is drawn: a text by its baseline and its height.
fn extent(prims: &[Prim]) -> Option<(P2, P2)> {
    let (mut lo, mut hi) = (P2::new(f64::INFINITY, f64::INFINITY), P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    let mut take = |p: P2| {
        lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
    };
    for p in prims {
        match p {
            Prim::Line { a, b, .. } => {
                take(*a);
                take(*b);
            }
            Prim::Poly { pts, .. } | Prim::Fill { pts, .. } => pts.iter().for_each(|&q| take(q)),
            Prim::Circle { c, r, .. } => {
                take(*c - P2::new(*r, *r));
                take(*c + P2::new(*r, *r));
            }
            Prim::Text { at, size, .. } => {
                take(*at - P2::new(0.0, size * 0.25));
                take(*at + P2::new(0.0, *size));
            }
        }
    }
    lo.x.is_finite().then_some((lo, hi))
}

/// The parts of a run of points inside a circle.
fn clip_circle(pts: &[P2], c: P2, r: f64) -> Vec<Vec<P2>> {
    let mut out = Vec::new();
    let mut cur: Vec<P2> = Vec::new();
    let inside = |p: P2| p.distance(c) <= r;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ia, ib) = (inside(a), inside(b));
        // Where the segment meets the circle.
        let d = b - a;
        let f = a - c;
        let (qa, qb, qc) = (d.x * d.x + d.y * d.y, 2.0 * (f.x * d.x + f.y * d.y), f.x * f.x + f.y * f.y - r * r);
        let disc = qb * qb - 4.0 * qa * qc;
        let ts: Vec<f64> = if qa > 1e-18 && disc >= 0.0 { [(-qb - disc.sqrt()) / (2.0 * qa), (-qb + disc.sqrt()) / (2.0 * qa)].into_iter().filter(|t| (0.0..=1.0).contains(t)).collect() } else { Vec::new() };
        match (ia, ib) {
            (true, true) => {
                if cur.is_empty() {
                    cur.push(a);
                }
                cur.push(b);
            }
            (true, false) => {
                if cur.is_empty() {
                    cur.push(a);
                }
                if let Some(t) = ts.last() {
                    cur.push(a + d * *t);
                }
                out.push(std::mem::take(&mut cur));
            }
            (false, true) => {
                if let Some(t) = ts.first() {
                    cur.push(a + d * *t);
                }
                cur.push(b);
            }
            (false, false) => {
                if ts.len() == 2 {
                    out.push(vec![a + d * ts[0], a + d * ts[1]]);
                }
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out.retain(|r| r.len() >= 2);
    out
}

/// A dimension measured on the model and drawn in a view.
fn dimension(prims: &mut Vec<Prim>, pieces: &[Piece], pl: &Place, x: &super::lang::Dim, env: &Env, target: &Option<Target>) -> Result<(), String> {
    let off = match &x.offset {
        Some(e) => eval(e, env)?,
        None => 10.0,
    };
    let find = |r: &str| face(pieces, r).ok_or_else(|| format!("`{r}` names no face or hole"));
    // Out from the view's middle, past its box.
    let out_past = |a: P2, b: P2| -> (P2, f64) {
        let d = b - a;
        let l = d.x.hypot(d.y).max(1e-12);
        let mut side = P2::new(-d.y / l, d.x / l);
        let mid = (a + b) * 0.5;
        let c = (pl.lo + pl.hi) * 0.5;
        if (mid - c).x * side.x + (mid - c).y * side.y < 0.0 {
            side = side * -1.0;
        }
        let corners = [pl.lo, pl.hi, P2::new(pl.lo.x, pl.hi.y), P2::new(pl.hi.x, pl.lo.y)];
        let far = corners.iter().map(|k| (*k - a).x * side.x + (*k - a).y * side.y).fold(f64::NEG_INFINITY, f64::max);
        (side, far.max(0.0) + off)
    };
    match x.kind {
        DimKind::Distance => {
            let (sa, fa) = find(&x.a)?;
            let (sb, fb) = find(x.b.as_deref().unwrap_or(""))?;
            let (pa, pb) = (middle(sa, fa), middle(sb, fb));
            let (Surface::Plane(_), Surface::Plane(_)) = (&sa.faces[fa].surface, &sb.faces[fb].surface) else {
                return Err("a distance is between flat faces".into());
            };
            let n = outward(sa, fa, pa);
            let nb = outward(sb, fb, pb);
            if n.cross(nb).length() > 1e-6 {
                return Err("the faces are not parallel: measure an angle".into());
            }
            let d = (pb - pa).dot(n);
            let (a, b) = (pl.sheet(pa), pl.sheet(pa + n * d));
            if a.distance(b) < 1e-6 {
                return Err("it is not seen in this view: the faces are one behind the other".into());
            }
            let (side, dist) = out_past(a, b);
            aligned(prims, a, b, side, dist, &measure(d.abs()));
        }
        DimKind::Diameter | DimKind::Radius => {
            let (s, f) = find(&x.a)?;
            let Surface::Cylinder(c) = s.faces[f].surface else { return Err("a diameter or radius is a round face's".into()) };
            let word = if x.kind == DimKind::Diameter { format!("Ø{}", measure(c.radius * 2.0)) } else { format!("R{}", measure(c.radius)) };
            let axis = c.frame.z;
            let mid = middle(s, f);
            let on_axis = c.frame.origin + axis * (mid - c.frame.origin).dot(axis);
            if axis.cross(pl.eye).length() < 1e-6 {
                // End on: a leader to its circle.
                let centre = pl.sheet(on_axis);
                let r = c.radius * pl.scale;
                let d45 = P2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);
                leader(prims, centre + d45 * r, centre + d45 * (r + off), &word);
            } else {
                let across = axis.cross(pl.eye).unit_or(pl.u);
                let (a, b) = (pl.sheet(on_axis + across * c.radius), pl.sheet(on_axis - across * c.radius));
                let (side, dist) = out_past(a, b);
                aligned(prims, a, b, side, dist, &word);
            }
        }
        DimKind::Angle => {
            let (sa, fa) = find(&x.a)?;
            let (sb, fb) = find(x.b.as_deref().unwrap_or(""))?;
            let (pa, pb) = (middle(sa, fa), middle(sb, fb));
            let (na, nb) = (outward(sa, fa, pa), outward(sb, fb, pb));
            let deg = na.dot(nb).clamp(-1.0, 1.0).acos().to_degrees();
            let (a, b) = (pl.sheet(pa), pl.sheet(pb));
            let m = (a + b) * 0.5 + P2::new(off, off) * 0.5;
            line(prims, a, m, Pen::Dim);
            leader(prims, b, m, &format!("{}°", measure(180.0 - deg)));
        }
        DimKind::Clearance => {
            let Some(Target::Assembly(_)) = target else { return Err("a clearance is between an assembly's parts".into()) };
            let of = |r: &str| -> Vec<&Solid> { pieces.iter().filter(|p| p.owner == r || p.owner.starts_with(&format!("{r}."))).map(|p| &p.solid).collect() };
            let (a, b) = (of(&x.a), of(x.b.as_deref().unwrap_or("")));
            if a.is_empty() || b.is_empty() {
                return Err(format!("no parts called `{}` and `{}`", x.a, x.b.as_deref().unwrap_or("")));
            }
            let (d, pa, pb) = assembly::fit::nearest_between(&a, &b, 0.05).ok_or("the parts have no surfaces")?;
            let (sa, sb) = (pl.sheet(pa), pl.sheet(pb));
            if sa.distance(sb) < 0.5 {
                // Too close on the sheet for arrows: a leader says it.
                leader(prims, sa, sa + P2::new(off, off), &format!("gap {}", measure(d)));
            } else {
                aligned(prims, sa, sb, P2::new(0.0, 0.0), 0.0, &measure(d));
            }
        }
    }
    Ok(())
}

/// The sheet's frame and title block, with its projection symbol.
fn frame(sheet: &mut Sheet, title: &str, of: &str, target: &Option<Target>, scale: f64, size_name: &str, first: bool) {
    let (w, h) = (sheet.width, sheet.height);
    let p = &mut sheet.prims;
    p.push(Prim::Poly { pts: vec![P2::new(10.0, 10.0), P2::new(w - 10.0, 10.0), P2::new(w - 10.0, h - 10.0), P2::new(10.0, h - 10.0)], closed: true, pen: Pen::Frame });
    let (x0, y0, x1, y1) = (w - 190.0, 10.0, w - 10.0, 40.0);
    p.push(Prim::Poly { pts: vec![P2::new(x0, y0), P2::new(x1, y0), P2::new(x1, y1), P2::new(x0, y1)], closed: true, pen: Pen::Frame });
    line(p, P2::new(x0, 25.0), P2::new(x1, 25.0), Pen::Frame);
    for x in [x0 + 70.0, x0 + 110.0, x0 + 145.0] {
        line(p, P2::new(x, y0), P2::new(x, 25.0), Pen::Frame);
    }
    text(p, P2::new(x0 + 3.0, 30.0), 5.0, title, Anchor::Start, 0.0, Pen::Frame);
    text(p, P2::new(x1 - 3.0, 30.5), 2.5, of, Anchor::End, 0.0, Pen::Frame);
    let material = match target {
        Some(Target::Part(m)) => m.material.clone().unwrap_or_else(|| "—".into()),
        Some(Target::Assembly(_)) => "assembly".into(),
        None => "—".into(),
    };
    let cells = [("MATERIAL", material), ("SCALE", ratio(scale)), ("SHEET", size_name.to_string()), ("UNITS", "mm".into())];
    for (k, (label, value)) in cells.iter().enumerate() {
        let x = [x0, x0 + 70.0, x0 + 110.0, x0 + 145.0][k] + 2.0;
        text(p, P2::new(x, 21.5), 2.0, *label, Anchor::Start, 0.0, Pen::Frame);
        text(p, P2::new(x, 14.0), 3.5, value.clone(), Anchor::Start, 0.0, Pen::Frame);
    }
    // The projection symbol: a cone's end and side, set out as the views are.
    let (sx, sy) = (x0 - 22.0, 17.0);
    let (side, end) = if first { (P2::new(sx + 12.0, sy), P2::new(sx, sy)) } else { (P2::new(sx, sy), P2::new(sx + 12.0, sy)) };
    p.push(Prim::Poly { pts: vec![side + P2::new(-4.0, -2.0), side + P2::new(4.0, -4.0), side + P2::new(4.0, 4.0), side + P2::new(-4.0, 2.0)], closed: true, pen: Pen::Frame });
    p.push(Prim::Circle { c: end, r: 4.0, pen: Pen::Frame });
    p.push(Prim::Circle { c: end, r: 2.0, pen: Pen::Frame });
    text(p, P2::new(sx + 6.0, sy - 9.5), 2.0, if first { "FIRST ANGLE" } else { "THIRD ANGLE" }, Anchor::Middle, 0.0, Pen::Frame);
    text(p, P2::new(15.0, 14.0), 2.5, "Proton CAD", Anchor::Start, 0.0, Pen::Frame);
    sheet.boxes.push(("title".into(), [x0, y0, x1, y1]));
}

/// The bill of materials as a table, its top right at `at`.
fn table(sheet: &mut Sheet, bom: &[assembly::model::BomRow], at: P2) {
    let cells: Vec<[String; 5]> = bom
        .iter()
        .enumerate()
        .map(|(i, r)| [(i + 1).to_string(), r.count.to_string(), r.part.clone(), r.material.clone().unwrap_or_default(), r.length.map(|l| format!("{} mm", l.ceil())).unwrap_or_default()])
        .collect();
    // Each column as wide as its widest text, at least as it was; the
    // part's no wider than 120 mm, a longer name set smaller to fit it.
    let mins = [("ITEM", 12.0), ("QTY", 12.0), ("PART", 74.0), ("MATERIAL", 30.0), ("LENGTH", 22.0)];
    let size = 2.5;
    let cols: Vec<(&str, f64)> = mins
        .iter()
        .enumerate()
        .map(|(k, &(label, min))| {
            let widest = cells.iter().map(|c| text_width(&c[k], size)).fold(text_width(label, size), f64::max);
            let w = (widest + 3.0).max(min);
            (label, if k == 2 { w.min(120.0) } else { w })
        })
        .collect();
    let width: f64 = cols.iter().map(|c| c.1).sum();
    let row = 6.0;
    let x0 = at.x - width;
    let top = at.y;
    let rows = bom.len() + 1;
    let p = &mut sheet.prims;
    p.push(Prim::Poly { pts: vec![P2::new(x0, top), P2::new(at.x, top), P2::new(at.x, top - row * rows as f64), P2::new(x0, top - row * rows as f64)], closed: true, pen: Pen::Frame });
    for k in 1..rows {
        let y = top - row * k as f64;
        line(p, P2::new(x0, y), P2::new(at.x, y), Pen::Frame);
    }
    let mut x = x0;
    for (k, (label, cw)) in cols.iter().enumerate() {
        if k > 0 {
            line(p, P2::new(x, top), P2::new(x, top - row * rows as f64), Pen::Frame);
        }
        text(p, P2::new(x + 1.5, top - row + 1.8), 2.5, *label, Anchor::Start, 0.0, Pen::Frame);
        x += cw;
    }
    for (i, row_cells) in cells.iter().enumerate() {
        let y = top - row * (i + 2) as f64 + 1.8;
        let mut x = x0;
        for (k, c) in row_cells.iter().enumerate() {
            let room = cols[k].1 - 3.0;
            let fits = (size * room / text_width(c, size).max(1e-9)).min(size).max(1.5);
            text(p, P2::new(x + 1.5, y), fits, c.clone(), Anchor::Start, 0.0, Pen::Frame);
            x += cols[k].1;
        }
    }
    sheet.boxes.push(("bom".into(), [x0, top - row * rows as f64, at.x, top]));
}

/// How wide `s` is set `size` high, mm: Helvetica's average letter, a
/// little over half its height.
fn text_width(s: &str, size: f64) -> f64 {
    s.chars().map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() { 0.62 } else if c == ' ' || c == '.' || c == ',' || c == '(' || c == ')' { 0.3 } else { 0.55 }).sum::<f64>() * size
}

/// Each row's first part pointed at by a balloon numbered for its row,
/// every balloon kept on the sheet, inside its border.
fn balloons(prims: &mut Vec<Prim>, pieces: &[Piece], bom: &[assembly::model::BomRow], pl: &Place, sheet: (f64, f64)) {
    let (w, h) = sheet;
    let inside = |b: P2| P2::new(b.x.clamp(20.0, w - 20.0), b.y.clamp(20.0, h - 20.0));
    let c = (pl.lo + pl.hi) * 0.5;
    let reach = (pl.hi - pl.lo).x.hypot((pl.hi - pl.lo).y) * 0.5 + 12.0;
    let mut placed: Vec<P2> = Vec::new();
    for (i, r) in bom.iter().enumerate() {
        let Some(first) = r.instances.first() else { continue };
        let mine: Vec<&Solid> = pieces.iter().filter(|p| &p.owner == first).map(|p| &p.solid).collect();
        let Some(s) = mine.first() else { continue };
        let Ok(m) = crate::mass::mass(s) else { continue };
        let at = pl.sheet(m.centroid);
        // Out from the view's middle through the part; a part at the
        // middle, up and to the left.
        let d = if at.distance(c) < 1e-6 { P2::new(-0.6, 0.8) } else { at - c };
        let l = d.x.hypot(d.y);
        let mut ball = inside(c + P2::new(d.x / l, d.y / l) * reach);
        // Clear of the others: further out, or where the border stops
        // that, along it.
        let mut tries = 0;
        while placed.iter().any(|q| q.distance(ball) < 11.0) && tries < 64 {
            let out = inside(ball + P2::new(d.x / l, d.y / l) * 11.0);
            ball = if out.distance(ball) > 1.0 { out } else { inside(ball + P2::new(-d.y / l, d.x / l) * 11.0) };
            tries += 1;
        }
        placed.push(ball);
        let to_ball = ball - at;
        let lb = to_ball.x.hypot(to_ball.y).max(1e-9);
        line(prims, at, ball - P2::new(to_ball.x / lb, to_ball.y / lb) * 5.0, Pen::Thin);
        prims.push(Prim::Circle { c: at, r: 0.6, pen: Pen::Thin });
        prims.push(Prim::Circle { c: ball, r: 5.0, pen: Pen::Dim });
        text(prims, ball + P2::new(0.0, -1.3), 3.5, (i + 1).to_string(), Anchor::Middle, 0.0, Pen::Dim);
    }
}

/* ── drawings written ─────────────────────────────────────────────── */

/// The scale a box `w` × `h` needs to fit in `room` × `room_h`, from the
/// usual ones.
fn fitting_scale(extent: f64, room: f64) -> f64 {
    const SCALES: [f64; 12] = [5.0, 2.0, 1.0, 0.5, 0.4, 0.25, 0.2, 0.1, 0.05, 0.04, 0.02, 0.01];
    SCALES.into_iter().find(|&s| extent * s <= room).unwrap_or(0.01)
}

/// A scale as the language writes it: `1/2`, `2`.
fn scale_text(s: f64) -> String {
    if s >= 1.0 {
        measure(s)
    } else {
        format!("1/{}", measure(1.0 / s))
    }
}

/// A part's drawing: front, top, right and an isometric view on an A3, at
/// a scale they fit.
pub fn standard(of: &str, title: &str, bounds: &Aabb) -> String {
    let e = bounds.max - bounds.min;
    let (w, d, h) = (e.x.max(1.0), e.y.max(1.0), e.z.max(1.0));
    let s = fitting_scale(w.max(d).max(h), 110.0);
    let (c1, c2, c3) = (100.0, 240.0, 340.0);
    let (r1, r2) = (90.0, 205.0);
    format!(
        "drawing \"{title}\"\nof \"{of}\"\nsheet A3\nscale {}\nangle third\nstyle paper\nview front front at ({c1}, {r1}) hidden\nview top top at ({c1}, {r2})\nview right right at ({c2}, {r1}) hidden\nview iso iso at ({c3}, {r2})\n",
        scale_text(s)
    )
}

/// An assembly's blueprint: its layout on an A2 — front, top and right,
/// an isometric view ballooned, an exploded one if it has one, a section
/// through its middle, the bill of materials, and its tightest gaps
/// dimensioned in the front view.
pub fn blueprint(of: &str, title: &str, m: &assembly::Model, blue: bool) -> String {
    let mut b = Aabb::empty();
    for l in m.leaves() {
        for (_, s) in &l.part.bodies {
            let x = s.bounds();
            if !x.is_empty() {
                for c in x.corners() {
                    b.include(l.place.apply_point(c));
                }
            }
        }
    }
    let e = if b.is_empty() { Vec3::new(100.0, 100.0, 100.0) } else { b.max - b.min };
    // Each view in its cell, 125 wide: the three square ones, and the
    // isometric one, which is as wide as its depth and width together.
    let iso = std::f64::consts::FRAC_1_SQRT_2 * (e.x + e.y);
    let s = fitting_scale(e.x.max(e.y).max(e.z).max(iso), 125.0);
    let mid_y = if b.is_empty() { 0.0 } else { (b.min.y + b.max.y) / 2.0 };
    let (c1, c2, c3, c4) = (85.0, 225.0, 365.0, 500.0);
    let (r1, r2) = (130.0, 310.0);
    let mut t = format!("drawing \"{title}\"\nof \"{of}\"\nsheet A2\nscale {}\nangle third\nstyle {}\n", scale_text(s), if blue { "blueprint" } else { "paper" });
    t.push_str(&format!("view front front at ({c1}, {r1})\nview top top at ({c1}, {r2})\nview right right at ({c2}, {r1})\nview iso iso at ({c3}, {r2})\n"));
    if let Some((name, _)) = m.explodes.first() {
        t.push_str(&format!("view apart iso at ({c2}, {r2}) exploded {name}\n"));
    }
    t.push_str(&format!("section a along y = {} at ({c4}, {r2})\n", measure(mid_y)));
    t.push_str("balloons iso\n");
    t.push_str(&format!("bom at (584, {})\n", 50.0 + 6.0 * (m.bom().len() + 1) as f64));
    // The tightest gaps, up to six.
    let mut gaps = m.clashes_at(m.clearance.max(5.0)).into_iter().filter(|c| !c.overlap).collect::<Vec<_>>();
    gaps.sort_by(|x, y| x.gap.total_cmp(&y.gap));
    for (k, g) in gaps.iter().take(6).enumerate() {
        t.push_str(&format!("dim gap{} front clearance {} to {}\n", k + 1, g.a, g.b));
    }
    t
}
