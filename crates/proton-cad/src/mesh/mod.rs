//! Tessellation: a solid as triangles, for the screen and for printers.
//!
//! Every edge is cut into pieces once — few enough to be quick, enough that
//! no piece strays further than [`Quality::chord`] from the curve or turns
//! more than [`Quality::angle`] — and both faces beside it use those same
//! points, so the triangles close up with no cracks along an edge. Each
//! face is then triangulated in its surface's parameters ([`uv`]), its loops
//! kept as edges ([`cdt`]); a sphere or torus starts from an even grid, and
//! any triangle further than the chord from the surface is split until none
//! is.
//!
//! What comes out is one mesh, its triangles anticlockwise seen from
//! outside, each with the face it came from and the surface's exact normal
//! at its corners for shading.

pub mod cdt;
pub mod uv;

use std::collections::HashMap;
use std::f64::consts::TAU;

use crate::brep::Solid;
use crate::curve::Curve;
use crate::surface::Surface;
use crate::vec::{Aabb, Vec2, Vec3};
use uv::EdgePts;

/// How close triangles keep to the exact geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    /// The furthest a triangle may be from its surface, in millimetres.
    pub chord: f64,
    /// The most an edge's direction may turn along one piece, in radians.
    pub angle: f64,
}

impl Quality {
    /// For the screen: a part `size` across, a two-thousandth of it — half
    /// a pixel with the part filling a window — and 15°.
    pub fn view(size: f64) -> Quality {
        Quality { chord: (size / 2000.0).clamp(0.002, 0.5), angle: 15f64.to_radians() }
    }

    /// For a printer: 10 µm, a tenth of a fine layer, and 10°.
    pub fn print() -> Quality {
        Quality { chord: 0.01, angle: 10f64.to_radians() }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    /// Anticlockwise seen from outside.
    pub triangles: Vec<[u32; 3]>,
    /// The face each triangle is from.
    pub face_of: Vec<u32>,
    /// The surface's normal at each triangle's corners.
    pub normals: Vec<[Vec3; 3]>,
    /// Each edge as the points it was cut at.
    pub edges: Vec<Vec<Vec3>>,
}

impl Mesh {
    /// The volume the triangles enclose: positive when they face out.
    pub fn volume(&self) -> f64 {
        self.triangles.iter().map(|t| {
            let [a, b, c] = t.map(|i| self.positions[i as usize]);
            a.dot(b.cross(c)) / 6.0
        }).sum()
    }

    pub fn area(&self) -> f64 {
        self.triangles.iter().map(|t| {
            let [a, b, c] = t.map(|i| self.positions[i as usize]);
            (b - a).cross(c - a).length() / 2.0
        }).sum()
    }

    /// How many of the triangles' edges are not met, the other way, by
    /// exactly one other: none, for a closed mesh facing one way.
    pub fn open_edges(&self) -> usize {
        let mut count: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.triangles {
            for k in 0..3 {
                *count.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
            }
        }
        count.iter().filter(|(&(a, b), &n)| n != 1 || count.get(&(b, a)) != Some(&1)).count()
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::of(self.triangles.iter().flatten().map(|&i| self.positions[i as usize]))
    }

    /// The points triangles use, and the triangles renumbered to them:
    /// what a file holds.
    pub fn compact(&self) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        let mut map = vec![u32::MAX; self.positions.len()];
        let mut pts = Vec::new();
        let tris = self
            .triangles
            .iter()
            .map(|t| {
                t.map(|i| {
                    if map[i as usize] == u32::MAX {
                        map[i as usize] = pts.len() as u32;
                        pts.push(self.positions[i as usize]);
                    }
                    map[i as usize]
                })
            })
            .collect();
        (pts, tris)
    }
}

/// The turn of a piece of a circle of radius `r` that keeps within the
/// chord and the angle.
pub fn turn(r: f64, q: &Quality) -> f64 {
    if r <= q.chord / 2.0 {
        q.angle
    } else {
        q.angle.min(2.0 * (1.0 - q.chord / r).acos())
    }
}

/// Where to cut a stretch of a curve, its ends included.
pub fn cuts(curve: &Curve, t0: f64, t1: f64, q: &Quality) -> Vec<f64> {
    let even = |n: usize| (0..=n).map(|k| t0 + (t1 - t0) * k as f64 / n as f64).collect();
    match curve {
        Curve::Line(_) => vec![t0, t1],
        Curve::Circle(c) => {
            let closed = (t1 - t0) >= TAU - 1e-12;
            let n = ((t1 - t0) / turn(c.radius, q)).ceil().max(if closed { 3.0 } else { 1.0 }) as usize;
            even(n)
        }
        Curve::Ellipse(e) => {
            let n = ((t1 - t0) / turn(e.minor * e.minor / e.major, q)).ceil().max(3.0) as usize;
            even(n)
        }
        Curve::BSpline(b) => {
            // Evenly by spans, then each piece halved while its middle is
            // off the chord or it turns too far.
            let start: Vec<f64> = even(4 * b.spans().max(1));
            let mut out = vec![t0];
            for w in start.windows(2) {
                split(curve, w[0], w[1], q, 0, &mut out);
            }
            out
        }
    }
}

fn split(curve: &Curve, a: f64, b: f64, q: &Quality, depth: u32, out: &mut Vec<f64>) {
    let (pa, pb, pm) = (curve.point(a), curve.point(b), curve.point(0.5 * (a + b)));
    let off = (pm - (pa + pb) * 0.5).length();
    let (ta, tb) = (curve.tangent(a), curve.tangent(b));
    let bend = ta.dot(tb).clamp(-1.0, 1.0).acos();
    if depth < 16 && (off > q.chord || bend > q.angle) {
        split(curve, a, 0.5 * (a + b), q, depth + 1, out);
        split(curve, 0.5 * (a + b), b, q, depth + 1, out);
    } else {
        out.push(b);
    }
}

/// How finely a face is cut in each parameter, how its parameters are
/// stretched into lengths for the triangulation, and whether it starts
/// from a grid.
struct Steps {
    du: f64,
    dv: f64,
    scale: (f64, f64),
    grid: bool,
}

fn steps(s: &Surface, rho: (f64, f64), q: &Quality) -> Steps {
    let full = |d: f64| TAU / (TAU / d).ceil();
    match s {
        Surface::Plane(_) => Steps { du: f64::INFINITY, dv: f64::INFINITY, scale: (1.0, 1.0), grid: false },
        Surface::Cylinder(c) => Steps { du: full(turn(c.radius, q)), dv: f64::INFINITY, scale: (c.radius, 1.0), grid: false },
        Surface::Cone(_) => Steps { du: full(turn(rho.1, q)), dv: f64::INFINITY, scale: (0.5 * (rho.0 + rho.1), 1.0), grid: false },
        Surface::Sphere(sp) => {
            let d = full(turn(sp.radius, q));
            Steps { du: d, dv: d, scale: (sp.radius, sp.radius), grid: true }
        }
        Surface::Torus(t) => Steps { du: full(turn(t.major + t.minor, q)), dv: full(turn(t.minor, q)), scale: (t.major, t.minor), grid: true },
    }
}

/// The solid as triangles, to the quality asked.
pub fn tessellate(solid: &Solid, q: &Quality) -> Result<Mesh, String> {
    let mut m = Mesh { positions: solid.vertices.iter().map(|v| v.point).collect(), ..Mesh::default() };
    let eps = edge_points(solid, q, &mut m.positions);
    m.edges = eps.iter().map(|e| e.ids.iter().map(|&i| m.positions[i as usize]).collect()).collect();
    for f in 0..solid.faces.len() {
        face(solid, f, &eps, q, &mut m)?;
    }
    Ok(m)
}

/// Every edge cut, its points added to `pos` after the vertices.
pub fn edge_points(solid: &Solid, q: &Quality, pos: &mut Vec<Vec3>) -> Vec<EdgePts> {
    solid
        .edges
        .iter()
        .map(|e| {
            let t = cuts(&e.curve, e.t0, e.t1, q);
            let mut ids = vec![e.start as u32];
            for &x in &t[1..t.len() - 1] {
                pos.push(e.point(x));
                ids.push(pos.len() as u32 - 1);
            }
            ids.push(e.end as u32);
            EdgePts { t, ids }
        })
        .collect()
}

/// How far from the surface's axis the face's edges come, nearest and
/// furthest: what sets how finely it is cut round.
fn reach(s: &Surface, pts: impl Iterator<Item = Vec3>) -> (f64, f64) {
    let Some((o, z)) = s.axis() else { return (0.0, 0.0) };
    pts.map(|p| {
        let d = p - o;
        (d - z * d.dot(z)).length()
    })
    .fold((f64::INFINITY, 0.0), |(lo, hi), r| (lo.min(r), hi.max(r)))
}

fn face(solid: &Solid, f: usize, eps: &[EdgePts], q: &Quality, m: &mut Mesh) -> Result<(), String> {
    let fc = &solid.faces[f];
    let s = &fc.surface;
    let rho = reach(s, fc.loops.iter().flatten().flat_map(|c| eps[c.edge].ids.iter().map(|&i| m.positions[i as usize])));
    let st = steps(s, rho, q);
    let fu = uv::face(solid, f, eps, &m.positions, if st.du.is_finite() { st.du } else { 0.2 })?;
    let (lo, hi) = bounds(&fu.loops);
    // A cylinder or a cone is straight along v: only how far a triangle
    // reaches round in u takes it off the surface. Stretched round until a
    // step round is as long as the face is tall, the Delaunay rule joins
    // each point to its nearest neighbours round the axis, and a split
    // edge is one that reaches round.
    let stretch = if st.dv.is_infinite() && st.du.is_finite() { ((hi.y - lo.y) / (st.scale.0 * st.du)).max(1.0) } else { 1.0 };
    let scale = (st.scale.0 * stretch, st.scale.1);
    let to_plane = |uv: Vec2| Vec2::new((uv.x - lo.x) * scale.0, (uv.y - lo.y) * scale.1);

    // The triangulation's points: the loops', then any added inside.
    let mut uvs: Vec<Vec2> = Vec::new();
    let mut ids: Vec<Option<u32>> = Vec::new();
    let mut p3: Vec<Vec3> = Vec::new();
    let mut loops: Vec<Vec<usize>> = Vec::new();
    for l in &fu.loops {
        let mut idx = Vec::new();
        for p in l {
            idx.push(uvs.len());
            uvs.push(p.uv);
            ids.push(Some(p.id));
            p3.push(m.positions[p.id as usize]);
        }
        loops.push(idx);
    }
    let plane: Vec<Vec2> = uvs.iter().map(|&p| to_plane(p)).collect();
    let mut cdt = cdt::polygon(&plane, &loops).map_err(|e| format!("`{}` would not triangulate: {e}", fc.name))?;
    let add = |cdt: &mut cdt::Cdt, uv: Vec2, clear: f64, uvs: &mut Vec<Vec2>, ids: &mut Vec<Option<u32>>, p3: &mut Vec<Vec3>| -> bool {
        match cdt.insert_inside(to_plane(uv), clear) {
            Some(i) => {
                debug_assert_eq!(i, uvs.len());
                uvs.push(uv);
                ids.push(None);
                p3.push(s.point(uv.x, uv.y));
                true
            }
            None => false,
        }
    };

    if st.grid {
        let (lo, hi) = bounds(&fu.loops);
        let (nu, nv) = (((hi.x - lo.x) / st.du).ceil() as usize, ((hi.y - lo.y) / st.dv).ceil() as usize);
        let clear = 0.3 * (st.du * scale.0).min(st.dv * scale.1);
        for i in 1..nu {
            for j in 1..nv {
                let uv = Vec2::new(lo.x + i as f64 * st.du, lo.y + j as f64 * st.dv);
                add(&mut cdt, uv, clear, &mut uvs, &mut ids, &mut p3);
            }
        }
    }

    // Split what strays from the surface.
    let limit = 400_000;
    for _ in 0..40 {
        let mut added = 0;
        for t in cdt.triangles() {
            let [a, b, c] = t;
            // Against a pole's line two corners are one point: no triangle.
            if [(a, b), (b, c), (c, a)].iter().any(|&(i, j)| ids[i].is_some() && ids[i] == ids[j]) {
                continue;
            }
            let centroid = (uvs[a] + uvs[b] + uvs[c]) * (1.0 / 3.0);
            let mut worst = (s.point(centroid.x, centroid.y) - (p3[a] + p3[b] + p3[c]) * (1.0 / 3.0)).length();
            // Split at the middle of the longest edge that may be split, so
            // long thin triangles get shorter rather than thinner; at the
            // centroid if every edge is a loop's. A loop's own edges are as
            // near their curve as its cutting made them, and do not count.
            let mut at = centroid;
            let mut longest = 0.0;
            for (i, j) in [(a, b), (b, c), (c, a)] {
                if cdt.is_kept(i, j) {
                    continue;
                }
                let mid = (uvs[i] + uvs[j]) * 0.5;
                worst = worst.max((s.point(mid.x, mid.y) - (p3[i] + p3[j]) * 0.5).length());
                let len = (to_plane(uvs[i]) - to_plane(uvs[j])).length();
                if len > longest {
                    longest = len;
                    at = mid;
                }
            }
            if worst > q.chord && add(&mut cdt, at, 0.0, &mut uvs, &mut ids, &mut p3) {
                added += 1;
            }
        }
        if added == 0 {
            break;
        }
        if uvs.len() > limit {
            return Err(format!("`{}` needs more than {limit} points to keep within {} mm", fc.name, q.chord));
        }
    }

    // Out to the mesh.
    let mut mesh_id = |i: usize, m: &mut Mesh| -> u32 {
        if let Some(id) = ids[i] {
            return id;
        }
        m.positions.push(p3[i]);
        let id = m.positions.len() as u32 - 1;
        ids[i] = Some(id);
        id
    };
    let sign = if fc.reversed { -1.0 } else { 1.0 };
    for t in cdt.triangles() {
        let t = if fc.reversed { [t[0], t[2], t[1]] } else { t };
        let v = t.map(|i| mesh_id(i, m));
        if v[0] == v[1] || v[1] == v[2] || v[2] == v[0] {
            continue;
        }
        m.triangles.push(v);
        m.face_of.push(f as u32);
        m.normals.push(t.map(|i| s.normal(uvs[i].x, uvs[i].y) * sign));
    }
    Ok(())
}

fn bounds(loops: &[Vec<uv::UvPt>]) -> (Vec2, Vec2) {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in loops.iter().flatten() {
        lo = Vec2::new(lo.x.min(p.uv.x), lo.y.min(p.uv.y));
        hi = Vec2::new(hi.x.max(p.uv.x), hi.y.max(p.uv.y));
    }
    (lo, hi)
}
