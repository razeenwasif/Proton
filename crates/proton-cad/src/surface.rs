//! Surfaces: what faces lie on.
//!
//! Each is analytic, and has two forms. The parametric one, `point(u, v)`,
//! is how a face is drawn and how its edges are written. The implicit one,
//! `distance(p)`, is the signed distance from the surface — exact, not an
//! approximation, so "within a nanometre" means just that — positive on the
//! side the normal points to. The two agree: `distance(point(u, v))` is
//! zero, and `gradient` is `normal`.
//!
//! The normal points out: away from a plane's frame along its z, away from a
//! cylinder's, cone's or sphere's axis or centre, away from a torus's core
//! circle. A face that wants the other side says it is reversed.

use std::f64::consts::{FRAC_PI_2, TAU};

use crate::frame::{Frame, Transform};
use crate::tol;
use crate::vec::{Aabb, Vec3};

/// The plane through `frame.origin` with normal `frame.z`:
/// `origin + u·x + v·y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub frame: Frame,
}

/// The cylinder of `radius` about `frame.z` through `frame.origin`:
/// `origin + r·(cos u·x + sin u·y) + v·z`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder {
    pub frame: Frame,
    pub radius: f64,
}

/// The cone about `frame.z` with `radius` in the plane of `frame.origin`,
/// widening toward +z at `half_angle` (between 0 and π/2):
/// `origin + (r + v·sin a)·(cos u·x + sin u·y) + v·cos a·z`. One nappe: the
/// parameter `v` stops at the apex, where the radius is nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone {
    pub frame: Frame,
    pub radius: f64,
    pub half_angle: f64,
}

/// The sphere of `radius` about `frame.origin`: `u` is longitude about
/// `frame.z`, `v` latitude, `−π/2..π/2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pub frame: Frame,
    pub radius: f64,
}

/// The torus about `frame.z`: a circle of radius `minor` swept round one of
/// radius `major`. `origin + (R + r·cos v)·(cos u·x + sin u·y) + r·sin v·z`.
/// A fillet round a boss is one. Exact for `major > minor`, a ring torus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Torus {
    pub frame: Frame,
    pub major: f64,
    pub minor: f64,
}

/// A free-form patch ([`crate::freeform`]), kept in the store by number,
/// placed by `frame`: a point of the patch `q` is at `frame.to_world(q)`.
/// Read from other programs' files for show: the kernel does not build on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Free {
    pub id: u32,
    pub frame: Frame,
}

impl Free {
    pub fn patch(&self) -> std::sync::Arc<crate::freeform::Patch> {
        crate::freeform::patch(self.id)
    }

    fn eval(&self, u: f64, v: f64) -> (Vec3, Vec3, Vec3) {
        let (p, a, b) = self.patch().eval(u, v);
        (self.frame.to_world(p), self.frame.vec_to_world(a), self.frame.vec_to_world(b))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Surface {
    Plane(Plane),
    Cylinder(Cylinder),
    Cone(Cone),
    Sphere(Sphere),
    Torus(Torus),
    Free(Free),
}

/// Which side of a surface a point is on, to the tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Within [`tol::LINEAR`] of it.
    On,
    /// The side the normal points to.
    Front,
    Back,
}

impl Cone {
    /// The parameter `v` of the apex: where the radius comes to nothing.
    pub fn apex_v(&self) -> f64 {
        -self.radius / self.half_angle.sin()
    }

    pub fn apex(&self) -> Vec3 {
        self.frame.origin + self.frame.z * (self.apex_v() * self.half_angle.cos())
    }
}

/// `(cos u, sin u)` in a frame's xy-plane.
fn around(f: &Frame, u: f64) -> (Vec3, Vec3) {
    let (s, c) = u.sin_cos();
    (f.x * c + f.y * s, f.y * c - f.x * s)
}

/// A point's distance from a frame's z axis, its height along it, and the
/// way out from the axis to it (the frame's x on the axis itself).
fn polar(f: &Frame, p: Vec3) -> (f64, f64, Vec3) {
    let d = p - f.origin;
    let h = d.dot(f.z);
    let out = d - f.z * h;
    let rho = out.length();
    (rho, h, if rho > 1e-300 { out / rho } else { f.x })
}

impl Surface {
    pub fn frame(&self) -> &Frame {
        match self {
            Surface::Plane(s) => &s.frame,
            Surface::Cylinder(s) => &s.frame,
            Surface::Cone(s) => &s.frame,
            Surface::Sphere(s) => &s.frame,
            Surface::Torus(s) => &s.frame,
            Surface::Free(s) => &s.frame,
        }
    }

    pub fn point(&self, u: f64, v: f64) -> Vec3 {
        match self {
            Surface::Plane(s) => s.frame.origin + s.frame.x * u + s.frame.y * v,
            Surface::Cylinder(s) => s.frame.origin + around(&s.frame, u).0 * s.radius + s.frame.z * v,
            Surface::Cone(s) => {
                let (sa, ca) = s.half_angle.sin_cos();
                s.frame.origin + around(&s.frame, u).0 * (s.radius + v * sa) + s.frame.z * (v * ca)
            }
            Surface::Sphere(s) => {
                let (sv, cv) = v.sin_cos();
                s.frame.origin + around(&s.frame, u).0 * (s.radius * cv) + s.frame.z * (s.radius * sv)
            }
            Surface::Torus(s) => {
                let (sv, cv) = v.sin_cos();
                s.frame.origin + around(&s.frame, u).0 * (s.major + s.minor * cv) + s.frame.z * (s.minor * sv)
            }
            Surface::Free(s) => s.eval(u, v).0,
        }
    }

    /// The derivatives of `point` along u and along v.
    pub fn derivatives(&self, u: f64, v: f64) -> (Vec3, Vec3) {
        match self {
            Surface::Plane(s) => (s.frame.x, s.frame.y),
            Surface::Cylinder(s) => (around(&s.frame, u).1 * s.radius, s.frame.z),
            Surface::Cone(s) => {
                let (sa, ca) = s.half_angle.sin_cos();
                let (e, de) = around(&s.frame, u);
                (de * (s.radius + v * sa), e * sa + s.frame.z * ca)
            }
            Surface::Sphere(s) => {
                let (sv, cv) = v.sin_cos();
                let (e, de) = around(&s.frame, u);
                (de * (s.radius * cv), (s.frame.z * cv - e * sv) * s.radius)
            }
            Surface::Torus(s) => {
                let (sv, cv) = v.sin_cos();
                let (e, de) = around(&s.frame, u);
                (de * (s.major + s.minor * cv), (s.frame.z * cv - e * sv) * s.minor)
            }
            Surface::Free(s) => {
                let (_, a, b) = s.eval(u, v);
                (a, b)
            }
        }
    }

    /// The signed distance from the surface: exact, positive on the side the
    /// normal points to.
    pub fn distance(&self, p: Vec3) -> f64 {
        match self {
            Surface::Plane(s) => (p - s.frame.origin).dot(s.frame.z),
            Surface::Cylinder(s) => polar(&s.frame, p).0 - s.radius,
            Surface::Cone(s) => {
                let (rho, h, _) = polar(&s.frame, p);
                let (sa, ca) = s.half_angle.sin_cos();
                // Beyond the apex the nearest point of the one nappe is the
                // apex itself.
                let v = (rho - s.radius) * sa + h * ca;
                if v < s.apex_v() {
                    let d = p.distance(s.apex());
                    let side = (rho - s.radius) * ca - h * sa;
                    if side < 0.0 { -d } else { d }
                } else {
                    (rho - s.radius) * ca - h * sa
                }
            }
            Surface::Sphere(s) => p.distance(s.frame.origin) - s.radius,
            Surface::Torus(s) => {
                let (rho, h, _) = polar(&s.frame, p);
                (rho - s.major).hypot(h) - s.minor
            }
            Surface::Free(_) => {
                // From its nearest point, signed by the normal there.
                let (u, v) = self.project(p);
                let d = p - self.point(u, v);
                let len = d.length();
                if d.dot(self.normal(u, v)) < 0.0 { -len } else { len }
            }
        }
    }

    /// The gradient of [`distance`](Self::distance): the unit normal of the
    /// surface's nearest point, pointing out.
    pub fn gradient(&self, p: Vec3) -> Vec3 {
        match self {
            Surface::Plane(s) => s.frame.z,
            Surface::Cylinder(s) => polar(&s.frame, p).2,
            Surface::Cone(s) => {
                let (rho, h, out) = polar(&s.frame, p);
                let (sa, ca) = s.half_angle.sin_cos();
                if (rho - s.radius) * sa + h * ca < s.apex_v() {
                    return (p - s.apex()).unit_or(-s.frame.z);
                }
                out * ca - s.frame.z * sa
            }
            Surface::Sphere(s) => (p - s.frame.origin).unit_or(s.frame.z),
            Surface::Torus(s) => {
                let (_, _, out) = polar(&s.frame, p);
                let core = s.frame.origin + out * s.major;
                (p - core).unit_or(out)
            }
            Surface::Free(_) => {
                let (u, v) = self.project(p);
                self.normal(u, v)
            }
        }
    }

    /// The unit normal at `(u, v)`, pointing out.
    pub fn normal(&self, u: f64, v: f64) -> Vec3 {
        match self {
            Surface::Plane(s) => s.frame.z,
            Surface::Cylinder(s) => around(&s.frame, u).0,
            Surface::Cone(s) => {
                let (sa, ca) = s.half_angle.sin_cos();
                around(&s.frame, u).0 * ca - s.frame.z * sa
            }
            Surface::Sphere(s) => {
                let (sv, cv) = v.sin_cos();
                around(&s.frame, u).0 * cv + s.frame.z * sv
            }
            Surface::Torus(s) => {
                let (sv, cv) = v.sin_cos();
                around(&s.frame, u).0 * cv + s.frame.z * sv
            }
            Surface::Free(s) => s.frame.vec_to_world(s.patch().normal(u, v)),
        }
    }

    /// The parameters of the surface's point nearest `p`: `u` in `[0, 2π)`
    /// where it turns.
    pub fn project(&self, p: Vec3) -> (f64, f64) {
        let turn = |out: Vec3, f: &Frame| out.dot(f.y).atan2(out.dot(f.x)).rem_euclid(TAU);
        match self {
            Surface::Plane(s) => {
                let q = s.frame.to_local(p);
                (q.x, q.y)
            }
            Surface::Cylinder(s) => {
                let (_, h, out) = polar(&s.frame, p);
                (turn(out, &s.frame), h)
            }
            Surface::Cone(s) => {
                let (rho, h, out) = polar(&s.frame, p);
                let (sa, ca) = s.half_angle.sin_cos();
                (turn(out, &s.frame), ((rho - s.radius) * sa + h * ca).max(s.apex_v()))
            }
            Surface::Sphere(s) => {
                let (rho, h, out) = polar(&s.frame, p);
                (turn(out, &s.frame), if rho == 0.0 && h == 0.0 { 0.0 } else { h.atan2(rho) })
            }
            Surface::Torus(s) => {
                let (rho, h, out) = polar(&s.frame, p);
                (turn(out, &s.frame), h.atan2(rho - s.major).rem_euclid(TAU))
            }
            Surface::Free(s) => s.patch().project(s.frame.to_local(p)),
        }
    }

    /// The surface's point nearest `p`.
    pub fn closest(&self, p: Vec3) -> Vec3 {
        let (u, v) = self.project(p);
        self.point(u, v)
    }

    pub fn side(&self, p: Vec3) -> Side {
        let d = self.distance(p);
        if d.abs() <= tol::LINEAR {
            Side::On
        } else if d > 0.0 {
            Side::Front
        } else {
            Side::Back
        }
    }

    /// Whether `u`, and `v`, come round again after a whole turn.
    pub fn periodic(&self) -> (bool, bool) {
        match self {
            Surface::Plane(_) | Surface::Free(_) => (false, false),
            Surface::Cylinder(_) | Surface::Cone(_) | Surface::Sphere(_) => (true, false),
            Surface::Torus(_) => (true, true),
        }
    }

    /// The axis it turns about, for a surface of revolution: a point on it
    /// and its direction. A sphere turns about every line through its
    /// centre, and answers with its frame's.
    pub fn axis(&self) -> Option<(Vec3, Vec3)> {
        match self {
            Surface::Plane(_) | Surface::Free(_) => None,
            Surface::Cylinder(s) => Some((s.frame.origin, s.frame.z)),
            Surface::Cone(s) => Some((s.frame.origin, s.frame.z)),
            Surface::Sphere(s) => Some((s.frame.origin, s.frame.z)),
            Surface::Torus(s) => Some((s.frame.origin, s.frame.z)),
        }
    }

    /// The smallest radius of curvature it has: how fine a search over it
    /// must look. `None` for a plane, which has none.
    pub fn feature_size(&self) -> Option<f64> {
        match self {
            Surface::Plane(_) => None,
            Surface::Cylinder(s) => Some(s.radius),
            Surface::Cone(s) => Some(s.radius.max(1.0)),
            Surface::Sphere(s) => Some(s.radius),
            Surface::Torus(s) => Some(s.minor),
            Surface::Free(s) => {
                // A knot span's length, roughly: where it can turn.
                let p = s.patch();
                let d = p.hull().max - p.hull().min;
                Some((d.length() / (p.spans_u().max(p.spans_v()) as f64 * 4.0)).max(0.01))
            }
        }
    }

    /// The parameters covering every point of the surface inside `bounds`:
    /// what a search over it walks.
    pub fn param_box(&self, bounds: &Aabb) -> ((f64, f64), (f64, f64)) {
        let corners = bounds.corners();
        let along = |f: &Frame, axis: Vec3| {
            let hs = corners.iter().map(|&c| (c - f.origin).dot(axis));
            hs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| (lo.min(h), hi.max(h)))
        };
        match self {
            Surface::Plane(s) => (along(&s.frame, s.frame.x), along(&s.frame, s.frame.y)),
            Surface::Cylinder(s) => ((0.0, TAU), along(&s.frame, s.frame.z)),
            Surface::Cone(s) => {
                // On the cone, height along the axis is v·cos a.
                let (lo, hi) = along(&s.frame, s.frame.z);
                let ca = s.half_angle.cos();
                ((0.0, TAU), ((lo / ca).max(s.apex_v()), (hi / ca).max(s.apex_v())))
            }
            Surface::Sphere(_) => ((0.0, TAU), (-FRAC_PI_2, FRAC_PI_2)),
            Surface::Torus(_) => ((0.0, TAU), (0.0, TAU)),
            Surface::Free(s) => s.patch().domain(),
        }
    }

    /// The surface every point of which is `d` along the normal from this
    /// one (back from it for a negative `d`): a plane moved, a cylinder,
    /// sphere or torus's tube grown or shrunk, a cone moved along its axis.
    /// `None` where that turns it inside out — shrunk past its axis, its
    /// centre or its core circle.
    pub fn offset(&self, d: f64) -> Option<Surface> {
        Some(match *self {
            Surface::Plane(s) => Surface::Plane(Plane { frame: Frame { origin: s.frame.origin + s.frame.z * d, ..s.frame } }),
            Surface::Cylinder(s) => {
                let radius = s.radius + d;
                if radius <= tol::LINEAR {
                    return None;
                }
                Surface::Cylinder(Cylinder { radius, ..s })
            }
            Surface::Cone(s) => {
                // Each point moves along out·cos a − z·sin a; the circle
                // kept is moved along the axis until its radius is some.
                let (sa, ca) = s.half_angle.sin_cos();
                let mut origin = s.frame.origin - s.frame.z * (d * sa);
                let mut radius = s.radius + d * ca;
                if radius <= tol::LINEAR {
                    let h = (1.0 - radius) / s.half_angle.tan();
                    origin += s.frame.z * h;
                    radius += h * s.half_angle.tan();
                }
                Surface::Cone(Cone { frame: Frame { origin, ..s.frame }, radius, half_angle: s.half_angle })
            }
            Surface::Sphere(s) => {
                let radius = s.radius + d;
                if radius <= tol::LINEAR {
                    return None;
                }
                Surface::Sphere(Sphere { radius, ..s })
            }
            // A free-form patch's offset is not one: the kernel does not keep it.
            Surface::Free(_) => return None,
            Surface::Torus(s) => {
                let minor = s.minor + d;
                if minor <= tol::LINEAR || minor >= s.major - tol::LINEAR {
                    return None;
                }
                Surface::Torus(Torus { minor, ..s })
            }
        })
    }

    pub fn transformed(&self, t: &Transform) -> Surface {
        let mut s = *self;
        let f = match &mut s {
            Surface::Plane(x) => &mut x.frame,
            Surface::Cylinder(x) => &mut x.frame,
            Surface::Cone(x) => &mut x.frame,
            Surface::Sphere(x) => &mut x.frame,
            Surface::Torus(x) => &mut x.frame,
            Surface::Free(x) => &mut x.frame,
        };
        *f = f.transformed(t);
        s
    }
}
