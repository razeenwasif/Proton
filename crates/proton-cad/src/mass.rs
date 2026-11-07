//! Area, volume and centroid, from the exact geometry rather than the
//! triangles.
//!
//! The divergence theorem turns each into an integral over the boundary
//! faces: volume is `∬ p·n dA / 3`, the first moments `∬ ½x² n_x dA` and
//! so on. On a face each is `∬ f(u, v) du dv` over its region in the
//! surface's parameters, and Green's theorem turns that into `∮ F dv`
//! round the region's loops, `F` being `f` integrated along `u`. So
//! everything is integrated along the edges' own curves, by Gauss–Legendre
//! on each piece the edges were cut into, and along `u` the same way: the
//! answer is as exact as the curves and surfaces are, not as the
//! triangles are.

use std::f64::consts::{PI, TAU};

use crate::brep::Solid;
use crate::mesh::{edge_points, uv, Quality};
use crate::surface::Surface;
use crate::vec::Vec3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mass {
    /// Square millimetres of surface.
    pub area: f64,
    /// Cubic millimetres.
    pub volume: f64,
    pub centroid: Vec3,
}

const GX: [f64; 8] = [-0.960_289_856_497_536_3, -0.796_666_477_413_626_7, -0.525_532_409_916_329, -0.183_434_642_495_649_8, 0.183_434_642_495_649_8, 0.525_532_409_916_329, 0.796_666_477_413_626_7, 0.960_289_856_497_536_3];
const GW: [f64; 8] = [0.101_228_536_290_376_3, 0.222_381_034_453_374_5, 0.313_706_645_877_887_3, 0.362_683_783_378_362, 0.362_683_783_378_362, 0.313_706_645_877_887_3, 0.222_381_034_453_374_5, 0.101_228_536_290_376_3];

/// What is integrated, at a point of a surface: the area element, and the
/// volume's and first moments' integrands.
fn integrand(s: &Surface, u: f64, v: f64) -> [f64; 5] {
    let p = s.point(u, v);
    let (su, sv) = s.derivatives(u, v);
    let n = su.cross(sv);
    [n.length(), p.dot(n) / 3.0, 0.5 * p.x * p.x * n.x, 0.5 * p.y * p.y * n.y, 0.5 * p.z * p.z * n.z]
}

/// `∫ f du` from `from` to `u`, at `v`: in pieces of a radian and a half
/// where `u` turns (the integrands are sines and cosines of it, which eight
/// points integrate to far below 10⁻¹² over that), in one where it is a
/// length (a plane's integrands are polynomials of degree two in it, which
/// eight points integrate exactly).
fn along_u(s: &Surface, from: f64, u: f64, v: f64) -> [f64; 5] {
    let mut out = [0.0; 5];
    let pieces = if s.periodic().0 { ((u - from).abs() / 1.5).ceil().max(1.0) as usize } else { 1 };
    let h = (u - from) / pieces as f64;
    for k in 0..pieces {
        let (a, b) = (from + h * k as f64, from + h * (k + 1) as f64);
        let (m, r) = (0.5 * (a + b), 0.5 * (b - a));
        for (x, w) in GX.iter().zip(GW) {
            let f = integrand(s, m + r * x, v);
            for i in 0..5 {
                out[i] += w * r * f[i];
            }
        }
    }
    out
}

fn signed(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

/// The solid's area, volume and centroid.
pub fn mass(solid: &Solid) -> Result<Mass, String> {
    let q = Quality { chord: f64::INFINITY, angle: 20f64.to_radians() };
    let mut pos: Vec<Vec3> = solid.vertices.iter().map(|v| v.point).collect();
    let eps = edge_points(solid, &q, &mut pos);
    let mut total = [0.0; 5];
    for (f, face) in solid.faces.iter().enumerate() {
        let s = &face.surface;
        let (pu, pv) = s.periodic();
        let fu = uv::face(solid, f, &eps, &pos, 0.2)?;
        let from = fu.loops.iter().flatten().map(|p| p.uv.x).fold(f64::INFINITY, f64::min);
        let mut sum = [0.0; 5];
        for piece in fu.pieces.iter().flatten() {
            let e = &solid.edges[piece.edge];
            let mut ts = eps[piece.edge].t.clone();
            if !piece.forward {
                ts.reverse();
            }
            for k in 0..ts.len() - 1 {
                let (ta, tb) = (ts[k], ts[k + 1]);
                let (ua, ub) = (piece.uv[k], piece.uv[k + 1]);
                let (m, r) = (0.5 * (ta + tb), 0.5 * (tb - ta));
                for (x, w) in GX.iter().zip(GW) {
                    let t = m + r * x;
                    let (p, d, _) = e.curve.eval(t);
                    let (u, v) = s.project(p);
                    // On the branch the piece is on.
                    let near = ua + (ub - ua) * (0.5 * (1.0 + x));
                    let u = if pu { near.x + signed(u - near.x) } else { u };
                    let v = if pv { near.y + signed(v - near.y) } else { v };
                    let dv = dv_dt(s, u, v, d);
                    let f = along_u(s, from, u, v);
                    for i in 0..5 {
                        sum[i] += w * r * f[i] * dv;
                    }
                }
            }
        }
        if face.reversed {
            sum[0] = -sum[0];
        }
        for i in 0..5 {
            total[i] += sum[i];
        }
    }
    let volume = total[1];
    Ok(Mass { area: total[0], volume, centroid: Vec3::new(total[2], total[3], total[4]) / volume })
}

/// How fast `v` changes along a curve through `(u, v)` with derivative
/// `d`: from `d = S_u u' + S_v v'`.
fn dv_dt(s: &Surface, u: f64, v: f64, d: Vec3) -> f64 {
    let (su, sv) = s.derivatives(u, v);
    let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let det = a * c - b * b;
    if det.abs() <= 1e-14 * a.max(c) * a.max(c) {
        return sv.dot(d) / c;
    }
    (a * sv.dot(d) - b * su.dot(d)) / det
}
