//! A STEP file's solids read into the kernel: every `MANIFOLD_SOLID_BREP`
//! and `BREP_WITH_VOIDS` in it, on the surfaces and curves the kernel has —
//! planes, cylinders, cones, spheres and tori, and free-form B-spline and
//! NURBS patches (kept for show: [`crate::freeform`]); lines, circles,
//! ellipses and B-splines. Lengths are made millimetres from the file's
//! unit, and each solid is checked to the file's own stated tolerance.
//!
//! An assembly's solids are placed where it puts them: each
//! representation's placement in its parent's (an
//! `ITEM_DEFINED_TRANSFORMATION` between two of their frames) followed up
//! to the top, and a solid used in several places is read once for each.

use std::collections::HashMap;
use std::f64::consts::TAU;

use super::p21::{File, Param};
use crate::brep::{Coedge, Edge, Face, Shell, Solid, Vertex};
use crate::bspline::BSplineCurve;
use crate::curve::{Circle, Curve, Ellipse, Line};
use crate::frame::Frame;
use crate::frame::Transform;
use crate::surface::{Cone, Cylinder, Free, Plane, Sphere, Surface, Torus};
use crate::vec::Vec3;

struct Ctx<'a> {
    f: &'a File,
    /// Millimetres in the file's length unit.
    k: f64,
    /// The file's stated tolerance, mm.
    tol: f64,
}

impl Ctx<'_> {
    fn get(&self, id: u64) -> Result<(&str, &[Param]), String> {
        let i = self.f.instances.get(&id).ok_or_else(|| format!("#{id} is named but not in the file"))?;
        // A complex instance's geometry is its most particular part.
        if let Some(s) = i.simple() {
            return Ok(s);
        }
        for ty in ["B_SPLINE_CURVE_WITH_KNOTS", "B_SPLINE_CURVE", "B_SPLINE_SURFACE_WITH_KNOTS", "B_SPLINE_SURFACE"] {
            if let Some(p) = i.part(ty) {
                return Ok((ty, p));
            }
        }
        Err(format!("#{id} is of several types at once, which is not read here"))
    }

    fn reference(&self, p: &Param, what: &str) -> Result<u64, String> {
        p.reference().ok_or_else(|| format!("expected {what} to be an instance"))
    }

    fn arg<'b>(&self, ps: &'b [Param], i: usize, ty: &str) -> Result<&'b Param, String> {
        ps.get(i).ok_or_else(|| format!("`{ty}` has too few parameters"))
    }

    fn point(&self, id: u64) -> Result<Vec3, String> {
        let (ty, ps) = self.get(id)?;
        if ty != "CARTESIAN_POINT" {
            return Err(format!("#{id} is a {ty}, not a point"));
        }
        let c = self.arg(ps, 1, ty)?.list().ok_or("a point's coordinates are a list")?;
        let x = |i: usize| c.get(i).and_then(Param::real).unwrap_or(0.0) * self.k;
        Ok(Vec3::new(x(0), x(1), x(2)))
    }

    fn direction(&self, id: u64) -> Result<Vec3, String> {
        let (ty, ps) = self.get(id)?;
        if ty != "DIRECTION" {
            return Err(format!("#{id} is a {ty}, not a direction"));
        }
        let c = self.arg(ps, 1, ty)?.list().ok_or("a direction is a list")?;
        let x = |i: usize| c.get(i).and_then(Param::real).unwrap_or(0.0);
        Vec3::new(x(0), x(1), x(2)).normalized().ok_or_else(|| format!("#{id} is a direction of no length"))
    }

    fn placement(&self, id: u64) -> Result<Frame, String> {
        let (ty, ps) = self.get(id)?;
        if ty != "AXIS2_PLACEMENT_3D" {
            return Err(format!("#{id} is a {ty}, not a placement"));
        }
        let o = self.point(self.reference(self.arg(ps, 1, ty)?, "a placement's point")?)?;
        let z = match self.arg(ps, 2, ty)? {
            Param::Ref(r) => self.direction(*r)?,
            _ => Vec3::Z,
        };
        let x = match ps.get(3) {
            Some(Param::Ref(r)) => self.direction(*r)?,
            _ => z.perpendicular(),
        };
        Ok(Frame::new(o, z, x))
    }

    fn length(&self, p: &Param) -> Result<f64, String> {
        p.real().map(|x| x * self.k).ok_or_else(|| "expected a length".into())
    }

    fn curve(&self, id: u64) -> Result<Curve, String> {
        let (ty, ps) = self.get(id)?;
        Ok(match ty {
            "LINE" => {
                let p = self.point(self.reference(self.arg(ps, 1, ty)?, "a line's point")?)?;
                let (vt, vp) = self.get(self.reference(self.arg(ps, 2, ty)?, "a line's vector")?)?;
                if vt != "VECTOR" {
                    return Err("a line's direction is not a vector".into());
                }
                let d = self.direction(self.reference(self.arg(vp, 1, vt)?, "a vector's direction")?)?;
                Curve::Line(Line { origin: p, dir: d })
            }
            "CIRCLE" => Curve::Circle(Circle { frame: self.placement(self.reference(self.arg(ps, 1, ty)?, "a circle's placement")?)?, radius: self.length(self.arg(ps, 2, ty)?)? }),
            "ELLIPSE" => {
                let frame = self.placement(self.reference(self.arg(ps, 1, ty)?, "an ellipse's placement")?)?;
                let (a, b) = (self.length(self.arg(ps, 2, ty)?)?, self.length(self.arg(ps, 3, ty)?)?);
                if a >= b {
                    Curve::Ellipse(Ellipse { frame, major: a, minor: b })
                } else {
                    // The kernel's major axis is along its x: turned a quarter.
                    let f = Frame::new(frame.origin, frame.z, frame.y);
                    Curve::Ellipse(Ellipse { frame: f, major: b, minor: a })
                }
            }
            "B_SPLINE_CURVE_WITH_KNOTS" => {
                let inst = &self.f.instances[&id];
                let mut weights = None;
                if let Some(w) = inst.part("RATIONAL_B_SPLINE_CURVE") {
                    let ws: Vec<f64> = w.first().and_then(Param::list).map(|l| l.iter().filter_map(Param::real).collect()).unwrap_or_default();
                    if ws.iter().any(|x| (x - ws[0]).abs() > 1e-12) {
                        weights = Some(ws);
                    }
                }
                // A complex instance splits its parameters between its parts.
                let (degree, poles, mults, knots) = match inst.simple() {
                    Some(_) => (self.arg(ps, 1, ty)?, self.arg(ps, 2, ty)?, self.arg(ps, 6, ty)?, self.arg(ps, 7, ty)?),
                    None => {
                        let base = inst.part("B_SPLINE_CURVE").ok_or("a B-spline without its curve")?;
                        (self.arg(base, 0, ty)?, self.arg(base, 1, ty)?, self.arg(ps, 0, ty)?, self.arg(ps, 1, ty)?)
                    }
                };
                let degree = degree.int().ok_or("a B-spline's degree")? as usize;
                let poles: Vec<Vec3> = poles.list().ok_or("a B-spline's poles")?.iter().map(|p| self.point(p.reference().unwrap_or(0))).collect::<Result<_, _>>()?;
                let mults: Vec<usize> = mults.list().ok_or("a B-spline's knots")?.iter().filter_map(|m| m.int().map(|x| x as usize)).collect();
                let values: Vec<f64> = knots.list().ok_or("a B-spline's knots")?.iter().filter_map(Param::real).collect();
                let knots: Vec<f64> = mults.iter().zip(&values).flat_map(|(&m, &v)| std::iter::repeat_n(v, m)).collect();
                match weights {
                    None => Curve::BSpline(BSplineCurve::new(degree, knots, poles)?),
                    Some(ws) => Curve::BSpline(rational_as_plain(degree, &knots, &poles, &ws, self.tol).map_err(|e| format!("#{id}: {e}"))?),
                }
            }
            "SURFACE_CURVE" | "SEAM_CURVE" => self.curve(self.reference(self.arg(ps, 1, ty)?, "a surface curve's curve")?)?,
            other => return Err(format!("#{id} is a {other}, a curve the kernel does not have")),
        })
    }

    fn surface(&self, id: u64) -> Result<Surface, String> {
        let (ty, ps) = self.get(id)?;
        let frame = || self.placement(self.reference(self.arg(ps, 1, ty)?, "a surface's placement")?);
        Ok(match ty {
            "PLANE" => Surface::Plane(Plane { frame: frame()? }),
            "CYLINDRICAL_SURFACE" => Surface::Cylinder(Cylinder { frame: frame()?, radius: self.length(self.arg(ps, 2, ty)?)? }),
            "CONICAL_SURFACE" => Surface::Cone(Cone { frame: frame()?, radius: self.length(self.arg(ps, 2, ty)?)?, half_angle: self.arg(ps, 3, ty)?.real().ok_or("a cone's angle")? * self.angle() }),
            "SPHERICAL_SURFACE" => Surface::Sphere(Sphere { frame: frame()?, radius: self.length(self.arg(ps, 2, ty)?)? }),
            "TOROIDAL_SURFACE" => Surface::Torus(Torus { frame: frame()?, major: self.length(self.arg(ps, 2, ty)?)?, minor: self.length(self.arg(ps, 3, ty)?)? }),
            "B_SPLINE_SURFACE_WITH_KNOTS" | "B_SPLINE_SURFACE" => Surface::Free(Free { id: crate::freeform::keep(self.patch(id)?), frame: Frame::world() }),
            other => return Err(format!("a face is on a {other}, a surface the kernel does not read")),
        })
    }

    /// A B-spline surface: its degrees, poles and knots, and its weights
    /// if it is rational — simple, or split between a complex instance's
    /// parts.
    fn patch(&self, id: u64) -> Result<crate::freeform::Patch, String> {
        let inst = &self.f.instances[&id];
        let (du, dv, poles, mu, mv, ku, kv) = match inst.simple() {
            Some(("B_SPLINE_SURFACE_WITH_KNOTS", ps)) => {
                let a = |i: usize| self.arg(ps, i, "B_SPLINE_SURFACE_WITH_KNOTS");
                (a(1)?, a(2)?, a(3)?, a(8)?, a(9)?, a(10)?, a(11)?)
            }
            Some((other, _)) => return Err(format!("#{id} is a {other}: only B-spline surfaces with their knots are read")),
            None => {
                let base = inst.part("B_SPLINE_SURFACE").ok_or_else(|| format!("#{id}: a B-spline surface without its degrees and poles"))?;
                let k = inst.part("B_SPLINE_SURFACE_WITH_KNOTS").ok_or_else(|| format!("#{id}: a B-spline surface without its knots"))?;
                let ty = "B_SPLINE_SURFACE";
                // A complex instance's B_SPLINE_SURFACE carries no name.
                (self.arg(base, 0, ty)?, self.arg(base, 1, ty)?, self.arg(base, 2, ty)?, self.arg(k, 0, ty)?, self.arg(k, 1, ty)?, self.arg(k, 2, ty)?, self.arg(k, 3, ty)?)
            }
        };
        let du = du.int().ok_or("a B-spline surface's degree")? as usize;
        let dv = dv.int().ok_or("a B-spline surface's degree")? as usize;
        let rows = poles.list().ok_or("a B-spline surface's poles")?;
        let nu = rows.len();
        let mut nv = 0;
        let mut pts = Vec::new();
        for r in rows {
            let r = r.list().ok_or("a B-spline surface's row of poles")?;
            nv = r.len();
            for p in r {
                pts.push(self.point(p.reference().ok_or("a B-spline surface's pole")?)?);
            }
        }
        let expand = |m: &Param, k: &Param| -> Result<Vec<f64>, String> {
            let m: Vec<usize> = m.list().ok_or("a B-spline surface's multiplicities")?.iter().filter_map(|x| x.int().map(|x| x as usize)).collect();
            let k: Vec<f64> = k.list().ok_or("a B-spline surface's knots")?.iter().filter_map(Param::real).collect();
            Ok(m.iter().zip(&k).flat_map(|(&n, &x)| std::iter::repeat_n(x, n)).collect())
        };
        let weights = match inst.part("RATIONAL_B_SPLINE_SURFACE") {
            Some(w) => {
                let rows = w.first().and_then(Param::list).ok_or("a rational B-spline surface's weights")?;
                Some(rows.iter().flat_map(|r| r.list().unwrap_or(&[]).iter().filter_map(Param::real)).collect::<Vec<f64>>())
            }
            None => None,
        };
        crate::freeform::Patch::new(du, dv, expand(mu, ku)?, expand(mv, kv)?, nu, nv, pts, weights).map_err(|e| format!("#{id}: {e}"))
    }

    /// Radians in the file's plane angle unit.
    fn angle(&self) -> f64 {
        let degrees = self.f.instances.values().any(|i| i.part("PLANE_ANGLE_UNIT").is_some() && i.part("CONVERSION_BASED_UNIT").is_some_and(|p| p.first().and_then(Param::text).is_some_and(|n| n.eq_ignore_ascii_case("degree"))));
        if degrees {
            std::f64::consts::PI / 180.0
        } else {
            1.0
        }
    }
}

/// A rational B-spline curve as a plain one through its points, to within
/// `tol`: the kernel's curves carry no weights. Cubic, through more points
/// until the curve between them is within half the tolerance.
fn rational_as_plain(degree: usize, knots: &[f64], poles: &[Vec3], ws: &[f64], tol: f64) -> Result<BSplineCurve, String> {
    if ws.len() != poles.len() || knots.len() != poles.len() + degree + 1 {
        return Err("a rational B-spline's weights or knots do not match its poles".into());
    }
    let (t0, t1) = (knots[degree], knots[poles.len()]);
    let at = |t: f64| {
        let span = crate::bspline::span_of(knots, degree, poles.len(), t);
        let b = crate::bspline::basis_of(knots, degree, span, t, 0);
        let (mut s, mut w) = (Vec3::ZERO, 0.0);
        for (j, &n) in b[0].iter().enumerate() {
            let k = span - degree + j;
            s += poles[k] * (ws[k] * n);
            w += ws[k] * n;
        }
        s / w
    };
    let spans = knots.windows(2).filter(|w| w[0] >= t0 && w[1] <= t1 && w[1] > w[0]).count().max(1);
    let mut n = (spans * 8).max(16);
    loop {
        let pts: Vec<Vec3> = (0..=n).map(|i| at(t0 + (t1 - t0) * i as f64 / n as f64)).collect();
        let (c, _) = BSplineCurve::interpolate(&pts, 3)?;
        let curve = Curve::BSpline(c.clone());
        let worst = (0..n).map(|i| curve.distance(at(t0 + (t1 - t0) * (i as f64 + 0.5) / n as f64))).fold(0.0, f64::max);
        if worst <= tol / 2.0 || n >= 4096 {
            return Ok(c);
        }
        n *= 2;
    }
}

/// Millimetres in the file's length unit — the one its geometry's context
/// names, else the first there is: SI prefixes of the metre, or inches and
/// feet by name.
fn length_unit(f: &File) -> f64 {
    let named: Vec<u64> = f
        .order
        .iter()
        .filter_map(|id| f.instances[id].part("GLOBAL_UNIT_ASSIGNED_CONTEXT"))
        .flat_map(|p| p.first().and_then(Param::list).unwrap_or(&[]).iter().filter_map(Param::reference).collect::<Vec<_>>())
        .collect();
    let order: Vec<u64> = named.iter().copied().chain(f.order.iter().copied()).collect();
    for id in &order {
        let Some(i) = f.instances.get(id) else { continue };
        if i.part("LENGTH_UNIT").is_none() {
            continue;
        }
        if let Some(si) = i.part("SI_UNIT") {
            return match si.first().and_then(Param::word) {
                Some("MILLI") => 1.0,
                Some("CENTI") => 10.0,
                Some("DECI") => 100.0,
                Some("KILO") => 1e6,
                Some("MICRO") => 1e-3,
                _ => 1000.0,
            };
        }
        if let Some(c) = i.part("CONVERSION_BASED_UNIT") {
            return match c.first().and_then(Param::text).map(str::to_ascii_lowercase).as_deref() {
                Some("inch") => 25.4,
                Some("foot") => 304.8,
                _ => 1.0,
            };
        }
    }
    1.0
}

/// The file's stated tolerance, mm: its `UNCERTAINTY_MEASURE_WITH_UNIT`
/// for lengths, the largest if it states several.
fn uncertainty(f: &File, k: f64) -> f64 {
    f.order
        .iter()
        .filter_map(|id| f.instances[id].part("UNCERTAINTY_MEASURE_WITH_UNIT"))
        .filter_map(|ps| match ps.first() {
            Some(Param::Typed(ty, inner)) if ty == "LENGTH_MEASURE" => inner.real(),
            Some(p) => p.real(),
            None => None,
        })
        .fold(0.0, f64::max)
        * k
}

/// The representations' places: which are the same shape (a plain
/// `SHAPE_REPRESENTATION_RELATIONSHIP`), and each one's place in its parent.
struct Places {
    class: HashMap<u64, u64>,
    parents: HashMap<u64, Vec<(u64, Transform)>>,
}

impl Places {
    fn read(ctx: &Ctx) -> Result<Places, String> {
        let f = ctx.f;
        let mut up: HashMap<u64, u64> = HashMap::new();
        fn root(up: &HashMap<u64, u64>, mut x: u64) -> u64 {
            while let Some(&p) = up.get(&x) {
                if p == x {
                    break;
                }
                x = p;
            }
            x
        }
        let mut links = Vec::new();
        for &id in &f.order {
            let inst = &f.instances[&id];
            let Some(rr) = inst.part("REPRESENTATION_RELATIONSHIP").or_else(|| inst.part("SHAPE_REPRESENTATION_RELATIONSHIP").filter(|p| p.len() >= 4)) else { continue };
            let (Some(r1), Some(r2)) = (rr.get(2).and_then(Param::reference), rr.get(3).and_then(Param::reference)) else { continue };
            match inst.part("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION") {
                Some(t) => {
                    let tid = t.first().and_then(Param::reference).ok_or("a placement's transformation")?;
                    let (tt, tp) = ctx.get(tid)?;
                    if tt != "ITEM_DEFINED_TRANSFORMATION" {
                        return Err(format!("an assembly places a part by a {tt}, which is not read"));
                    }
                    let f1 = ctx.placement(ctx.reference(ctx.arg(tp, 2, tt)?, "a transformation's first frame")?)?;
                    let f2 = ctx.placement(ctx.reference(ctx.arg(tp, 3, tt)?, "a transformation's second frame")?)?;
                    // The child's frame f1 put on the parent's f2.
                    links.push((r1, r2, Transform::between(&f1, &f2)));
                }
                None => {
                    let (a, b) = (root(&up, r1), root(&up, r2));
                    up.entry(a).or_insert(a);
                    up.insert(b, a);
                }
            }
        }
        let mut class = HashMap::new();
        let mut parents: HashMap<u64, Vec<(u64, Transform)>> = HashMap::new();
        for &id in &f.order {
            class.insert(id, root(&up, id));
        }
        for (child, parent, t) in links {
            parents.entry(root(&up, child)).or_default().push((root(&up, parent), t));
        }
        Ok(Places { class, parents })
    }

    /// Every place a representation's shape is put, through its parents
    /// up to the top.
    fn of(&self, rep: u64, depth: usize) -> Vec<Transform> {
        let c = self.class.get(&rep).copied().unwrap_or(rep);
        match self.parents.get(&c) {
            Some(ps) if depth < 32 => ps.iter().flat_map(|(p, t)| self.of(*p, depth + 1).into_iter().map(move |w| t.then(&w))).collect(),
            _ => vec![Transform::identity()],
        }
    }
}

/// Every solid in the file, `(name, solid)`, checked, each where its
/// assembly puts it.
pub fn read(text: &str) -> Result<Vec<(String, Solid)>, String> {
    let file = super::p21::parse(text)?;
    let k = length_unit(&file);
    let tolerance = uncertainty(&file, k).clamp(crate::tol::LINEAR, 0.05);
    // Solids are read for show and measuring, so checked no closer than
    // 0.05 mm: other programs keep a tolerance for each edge and vertex,
    // and write files a little looser than the one they state.
    let show = tolerance.max(0.05);
    let ctx = Ctx { f: &file, k, tol: tolerance };
    let places = Places::read(&ctx)?;
    // Which representations hold each solid, and the name of the shape
    // each is: any of its class's that has one.
    let mut named: HashMap<u64, String> = HashMap::new();
    let mut held_by: HashMap<u64, Vec<u64>> = HashMap::new();
    for &id in &file.order {
        let Some((ty, ps)) = file.instances[&id].simple() else { continue };
        if !ty.ends_with("REPRESENTATION") || ty.contains("RELATIONSHIP") {
            continue;
        }
        let name = ps.first().and_then(Param::text).unwrap_or("").trim().to_string();
        let class = places.class.get(&id).copied().unwrap_or(id);
        if !name.is_empty() && name != "NONE" {
            named.entry(class).or_insert(name);
        }
        for item in ps.get(1).and_then(Param::list).unwrap_or(&[]) {
            if let Some(r) = item.reference() {
                held_by.entry(r).or_default().push(id);
            }
        }
    }
    // Each holder: its own name, else the name of the product shape it is
    // linked to.
    let own_name = |r: u64| file.instances[&r].simple().and_then(|(_, ps)| ps.first().and_then(Param::text)).unwrap_or("").trim().to_string();
    let holders: HashMap<u64, Vec<(u64, String, bool)>> = held_by
        .into_iter()
        .map(|(solid, reps)| {
            let list = reps
                .into_iter()
                .map(|r| {
                    let own = own_name(r);
                    let linked = own.is_empty() || own == "NONE";
                    let name = if linked { named.get(&places.class.get(&r).copied().unwrap_or(r)).cloned().unwrap_or_default() } else { own };
                    (r, name, linked)
                })
                .collect();
            (solid, list)
        })
        .collect();
    let mut out = Vec::new();
    for &id in &file.order {
        let Some((ty, ps)) = file.instances[&id].simple() else { continue };
        let (outer, voids) = match ty {
            "MANIFOLD_SOLID_BREP" => (ctx.reference(ctx.arg(ps, 1, ty)?, "a solid's shell")?, Vec::new()),
            "BREP_WITH_VOIDS" => {
                let voids = ctx.arg(ps, 2, ty)?.list().ok_or("a solid's voids")?.iter().filter_map(Param::reference).collect();
                (ctx.reference(ctx.arg(ps, 1, ty)?, "a solid's shell")?, voids)
            }
            _ => continue,
        };
        // The solid's own name, unless its representation is an unnamed one
        // standing for a named product: then the product's — a solid's own is
        // often the feature that made it last ("Fillet2"), or bytes in some
        // program's own encoding.
        let held = holders.get(&id).cloned().unwrap_or_default();
        let own = ps.first().and_then(Param::text).unwrap_or("").trim();
        let product = held.iter().find(|(_, n, linked)| *linked && !n.trim().is_empty()).map(|(_, n, _)| n.trim());
        let good = |n: &str| !n.is_empty() && n != "NONE" && n.chars().all(|c| c.is_ascii_graphic() || c == ' ');
        let name = product
            .or(Some(own).filter(|n| good(n)))
            .or(held.iter().map(|(_, n, _)| n.trim()).find(|n| good(n)))
            .unwrap_or("solid")
            .to_string();
        let mut b = Build { ctx: &ctx, s: Solid::default(), vertices: HashMap::new(), edges: HashMap::new() };
        b.shell(outer, false).map_err(|e| format!("`{name}`: {e}"))?;
        for v in voids {
            let (vt, vp) = ctx.get(v)?;
            let (shell, flip) = match vt {
                "ORIENTED_CLOSED_SHELL" => (ctx.reference(ctx.arg(vp, 2, vt)?, "a void's shell")?, !ctx.arg(vp, 3, vt)?.flag().unwrap_or(true)),
                "CLOSED_SHELL" => (v, true),
                other => return Err(format!("`{name}`: a void is a {other}")),
            };
            b.shell(shell, flip).map_err(|e| format!("`{name}`: {e}"))?;
        }
        let s = b.s;
        if let Some(p) = crate::brep::check::check_read(&s, show).first() {
            return Err(format!("`{name}` in the file is not a valid solid: {p}"));
        }
        let mut at: Vec<Transform> = held.iter().flat_map(|(rep, _, _)| places.of(*rep, 0)).collect();
        if at.is_empty() {
            at.push(Transform::identity());
        }
        let many = at.len() > 1;
        for (k, t) in at.iter().enumerate() {
            let label = if many { format!("{name} {}", k + 1) } else { name.clone() };
            out.push((label, s.transformed(t)));
        }
    }
    if out.is_empty() {
        return Err("the file has no solids: no MANIFOLD_SOLID_BREP or BREP_WITH_VOIDS".into());
    }
    Ok(out)
}

struct Build<'a> {
    ctx: &'a Ctx<'a>,
    s: Solid,
    vertices: HashMap<u64, usize>,
    /// Each edge read, and whether it runs against its curve in the file
    /// (the kernel's run along theirs).
    edges: HashMap<u64, (usize, bool)>,
}

impl Build<'_> {
    fn vertex(&mut self, id: u64) -> Result<usize, String> {
        if let Some(&v) = self.vertices.get(&id) {
            return Ok(v);
        }
        let (ty, ps) = self.ctx.get(id)?;
        if ty != "VERTEX_POINT" {
            return Err(format!("#{id} is a {ty}, not a vertex"));
        }
        let p = self.ctx.point(self.ctx.reference(self.ctx.arg(ps, 1, ty)?, "a vertex's point")?)?;
        self.s.vertices.push(Vertex { point: p });
        let v = self.s.vertices.len() - 1;
        self.vertices.insert(id, v);
        Ok(v)
    }

    fn edge(&mut self, id: u64) -> Result<(usize, bool), String> {
        if let Some(&e) = self.edges.get(&id) {
            return Ok(e);
        }
        let (ty, ps) = self.ctx.get(id)?;
        if ty != "EDGE_CURVE" {
            return Err(format!("#{id} is a {ty}, not an edge"));
        }
        let (a, b) = (self.vertex(self.ctx.reference(self.ctx.arg(ps, 1, ty)?, "an edge's start")?)?, self.vertex(self.ctx.reference(self.ctx.arg(ps, 2, ty)?, "an edge's end")?)?);
        let curve = self.ctx.curve(self.ctx.reference(self.ctx.arg(ps, 3, ty)?, "an edge's curve")?)?;
        let along = self.ctx.arg(ps, 4, ty)?.flag().unwrap_or(true);
        // Run along the curve: from the start, or from the end when the
        // edge runs against it.
        let (from, to) = if along { (a, b) } else { (b, a) };
        let (pf, pt) = (self.s.vertices[from].point, self.s.vertices[to].point);
        let at = |p: Vec3| match &curve {
            Curve::BSpline(_) => {
                let (d0, d1) = curve.domain();
                curve.project_in(p, d0, d1).0
            }
            c => c.project(p).0,
        };
        let (mut t0, mut t1) = (at(pf), at(pt));
        if curve.period().is_none() {
            let (d0, d1) = curve.domain();
            if from == to {
                // A closed edge on a curve that does not come round: all of it.
                (t0, t1) = (d0, d1);
            } else if curve.point(d0).distance(curve.point(d1)) <= 10.0 * crate::tol::LINEAR {
                // A curve that ends where it starts: a point there is at
                // either end, whichever the edge's other end leaves room for.
                let near = |t: f64, at: f64| (t - at).abs() <= 1e-9 * (1.0 + (d1 - d0).abs());
                if t1 <= t0 && near(t1, d0) {
                    t1 = d1;
                }
                if t1 <= t0 && near(t0, d1) {
                    t0 = d0;
                }
            }
        }
        if let Some(per) = curve.period() {
            t1 = t0 + (t1 - t0).rem_euclid(per);
            if from == to && t1 - t0 <= 1e-9 {
                t1 = t0 + TAU;
            }
        }
        if t1 <= t0 {
            return Err(format!("edge #{id} runs backward along its curve"));
        }
        self.s.edges.push(Edge { curve, t0, t1, start: from, end: to });
        let e = (self.s.edges.len() - 1, !along);
        self.edges.insert(id, e);
        Ok(e)
    }

    /// A closed shell's faces, turned over if `flip`.
    fn shell(&mut self, id: u64, flip: bool) -> Result<(), String> {
        let (ty, ps) = self.ctx.get(id)?;
        if ty != "CLOSED_SHELL" {
            return Err(format!("#{id} is a {ty}, not a closed shell"));
        }
        let mut faces = Vec::new();
        for f in self.ctx.arg(ps, 1, ty)?.list().ok_or("a shell's faces")? {
            let fid = f.reference().ok_or("a shell's face")?;
            let (ft, fp) = self.ctx.get(fid)?;
            if ft != "ADVANCED_FACE" && ft != "FACE_SURFACE" {
                return Err(format!("#{fid} is a {ft}, not a face"));
            }
            let name = fp.first().and_then(Param::text).unwrap_or("").to_string();
            let surface = self.ctx.surface(self.ctx.reference(self.ctx.arg(fp, 2, ft)?, "a face's surface")?)?;
            let same = self.ctx.arg(fp, 3, ft)?.flag().unwrap_or(true);
            let mut loops: Vec<(bool, Vec<Coedge>)> = Vec::new();
            for bnd in self.ctx.arg(fp, 1, ft)?.list().ok_or("a face's bounds")? {
                let bid = bnd.reference().ok_or("a face's bound")?;
                let (bt, bp) = self.ctx.get(bid)?;
                let outer = bt == "FACE_OUTER_BOUND";
                let lid = self.ctx.reference(self.ctx.arg(bp, 1, bt)?, "a bound's loop")?;
                let keep = self.ctx.arg(bp, 2, bt)?.flag().unwrap_or(true);
                let (lt, lp) = self.ctx.get(lid)?;
                if lt == "VERTEX_LOOP" {
                    // A pole: the kernel's loops pass through it.
                    continue;
                }
                if lt != "EDGE_LOOP" {
                    return Err(format!("#{lid} is a {lt}, not a loop"));
                }
                let mut run = Vec::new();
                for oe in self.ctx.arg(lp, 1, lt)?.list().ok_or("a loop's edges")? {
                    let oid = oe.reference().ok_or("a loop's edge")?;
                    let (ot, op) = self.ctx.get(oid)?;
                    if ot != "ORIENTED_EDGE" {
                        return Err(format!("#{oid} is a {ot}, not an oriented edge"));
                    }
                    let (e, against) = self.edge(self.ctx.reference(self.ctx.arg(op, 3, ot)?, "an oriented edge's edge")?)?;
                    let fwd = self.ctx.arg(op, 4, ot)?.flag().unwrap_or(true);
                    run.push(Coedge { edge: e, forward: fwd != against });
                }
                if !keep {
                    run = run.into_iter().rev().map(|c| Coedge { forward: !c.forward, ..c }).collect();
                }
                loops.push((outer, run));
            }
            if flip {
                for (_, l) in &mut loops {
                    *l = l.iter().rev().map(|c| Coedge { forward: !c.forward, ..*c }).collect();
                }
            }
            // The bounding loop first. A file need not say which it is (each
            // bound a plain FACE_BOUND): then, on a surface that does not
            // turn, it is the loop enclosing the most in the parameters.
            if loops.len() > 1 && !loops.iter().any(|(outer, _)| *outer) && surface.periodic() == (false, false) {
                let area = |run: &[Coedge]| {
                    let mut pts = Vec::new();
                    for c in run {
                        let mut s = self.s.edges[c.edge].sample(8);
                        if !c.forward {
                            s.reverse();
                        }
                        s.pop();
                        pts.extend(s.into_iter().map(|p| surface.project(p)));
                    }
                    let n = pts.len();
                    (0..n).map(|i| pts[i].0 * pts[(i + 1) % n].1 - pts[(i + 1) % n].0 * pts[i].1).sum::<f64>().abs() / 2.0
                };
                let biggest = (0..loops.len()).max_by(|&i, &j| area(&loops[i].1).total_cmp(&area(&loops[j].1))).unwrap_or(0);
                loops[biggest].0 = true;
            }
            loops.sort_by_key(|(outer, _)| !*outer);
            self.s.faces.push(Face { surface, reversed: (!same) != flip, loops: loops.into_iter().map(|l| l.1).collect(), name });
            faces.push(self.s.faces.len() - 1);
        }
        self.s.shells.push(Shell { faces });
        Ok(())
    }
}
