//! CAD mode's kernel, Proton's own: see `docs/CAD.md`.
//!
//! Built so far (K0), the geometry everything else stands on:
//! - [`vec`]: points, directions and boxes, in millimetres.
//! - [`tol`]: the one tolerance, and what follows from it.
//! - [`frame`]: placements, and the rigid motions between them.
//! - [`poly`]: the real roots of a polynomial, for the closed forms.
//! - [`curve`] and [`bspline`]: lines, circles, ellipses and B-splines —
//!   evaluated, differentiated, measured, and a point's nearest found.
//! - [`surface`]: planes, cylinders, cones, spheres and tori — evaluated,
//!   each with its exact signed distance, and a point's nearest found.
//! - [`intersect`]: where curves meet surfaces and each other, and where
//!   surfaces meet: by formula for every pair that has one, by [`march`]ing
//!   and fitting a B-spline for the rest.
//!
//! K1, sketches:
//! - [`part`]: the `.part` language — a part's head, parameters, sketches
//!   and features — read and written back unchanged.
//! - [`sketch`]: a sketch's shapes moved until its constraints hold, its
//!   degrees of freedom counted, its redundant and conflicting constraints
//!   named.

pub mod bspline;
pub mod curve;
pub mod frame;
pub mod intersect;
pub mod march;
pub mod part;
pub mod poly;
pub mod sketch;
pub mod surface;
pub mod tol;
pub mod vec;

pub use bspline::BSplineCurve;
pub use curve::{Circle, Curve, Ellipse, Line};
pub use frame::{Frame, Transform};
pub use intersect::{CurveHit, Hit, Section};
pub use surface::{Cone, Cylinder, Plane, Side, Sphere, Surface, Torus};
pub use vec::{Aabb, Point3, Vec2, Vec3};
