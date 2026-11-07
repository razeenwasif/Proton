//! Placements, and the rigid motions between them.
//!
//! A [`Frame`] is where a sketch, a surface or a part sits: an origin and
//! three axes at right angles, right-handed. A [`Transform`] moves things
//! rigidly — turns and shifts, never a stretch or a mirror — so a circle
//! stays a circle of the same radius, and a frame stays right-handed.

use crate::vec::Vec3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
}

impl Default for Frame {
    fn default() -> Frame {
        Frame::world()
    }
}

impl Frame {
    /// The world's own axes, at the origin.
    pub fn world() -> Frame {
        Frame { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y, z: Vec3::Z }
    }

    /// The frame at `origin` whose z is `z`, and whose x is as near `x` as a
    /// direction at right angles to z can be (any such direction when `x` is
    /// along z). `z` need not be a unit vector, but must not be zero.
    pub fn new(origin: Vec3, z: Vec3, x: Vec3) -> Frame {
        let z = z.unit_or(Vec3::Z);
        let x = (x - z * x.dot(z)).normalized().unwrap_or_else(|| z.perpendicular());
        Frame { origin, x, y: z.cross(x), z }
    }

    /// The frame at `origin` whose z is `z`, its x chosen.
    pub fn from_z(origin: Vec3, z: Vec3) -> Frame {
        let z = z.unit_or(Vec3::Z);
        Frame::new(origin, z, z.perpendicular())
    }

    /// A world point, in this frame's coordinates.
    pub fn to_local(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    /// A world direction, in this frame's coordinates.
    pub fn vec_to_local(&self, v: Vec3) -> Vec3 {
        Vec3::new(v.dot(self.x), v.dot(self.y), v.dot(self.z))
    }

    /// A point in this frame's coordinates, in the world's.
    pub fn to_world(&self, p: Vec3) -> Vec3 {
        self.origin + self.vec_to_world(p)
    }

    /// A direction in this frame's coordinates, in the world's.
    pub fn vec_to_world(&self, v: Vec3) -> Vec3 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    pub fn transformed(&self, t: &Transform) -> Frame {
        Frame { origin: t.apply_point(self.origin), x: t.apply_vec(self.x), y: t.apply_vec(self.y), z: t.apply_vec(self.z) }
    }

    /// Whether the axes are unit length, at right angles and right-handed,
    /// to `tol`.
    pub fn is_orthonormal(&self, tol: f64) -> bool {
        let unit = |v: Vec3| (v.length() - 1.0).abs() <= tol;
        unit(self.x) && unit(self.y) && unit(self.z) && self.x.dot(self.y).abs() <= tol && self.y.dot(self.z).abs() <= tol && self.z.dot(self.x).abs() <= tol && (self.x.cross(self.y) - self.z).length() <= tol
    }
}

/// A rigid motion: `p ↦ R·p + offset`, R a rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// The rotation's rows.
    pub rows: [Vec3; 3],
    pub offset: Vec3,
}

impl Default for Transform {
    fn default() -> Transform {
        Transform::identity()
    }
}

impl Transform {
    pub fn identity() -> Transform {
        Transform { rows: [Vec3::X, Vec3::Y, Vec3::Z], offset: Vec3::ZERO }
    }

    pub fn translation(by: Vec3) -> Transform {
        Transform { offset: by, ..Transform::identity() }
    }

    /// A turn of `angle` radians about the line through `origin` along
    /// `axis`, anticlockwise looking down the axis toward its origin.
    pub fn rotation(origin: Vec3, axis: Vec3, angle: f64) -> Transform {
        let k = axis.unit_or(Vec3::Z);
        let (s, c) = angle.sin_cos();
        let t = 1.0 - c;
        let rows = [
            Vec3::new(c + k.x * k.x * t, k.x * k.y * t - k.z * s, k.x * k.z * t + k.y * s),
            Vec3::new(k.y * k.x * t + k.z * s, c + k.y * k.y * t, k.y * k.z * t - k.x * s),
            Vec3::new(k.z * k.x * t - k.y * s, k.z * k.y * t + k.x * s, c + k.z * k.z * t),
        ];
        let turn = Transform { rows, offset: Vec3::ZERO };
        Transform { rows, offset: origin - turn.apply_vec(origin) }
    }

    /// What takes a frame's own coordinates to the world's.
    pub fn from_frame(f: &Frame) -> Transform {
        Transform { rows: [Vec3::new(f.x.x, f.y.x, f.z.x), Vec3::new(f.x.y, f.y.y, f.z.y), Vec3::new(f.x.z, f.y.z, f.z.z)], offset: f.origin }
    }

    /// What takes frame `from` to frame `to`: anything placed in the first
    /// sits the same way in the second.
    pub fn between(from: &Frame, to: &Frame) -> Transform {
        Transform::from_frame(&from.clone()).inverse().then(&Transform::from_frame(to))
    }

    pub fn apply_vec(&self, v: Vec3) -> Vec3 {
        Vec3::new(self.rows[0].dot(v), self.rows[1].dot(v), self.rows[2].dot(v))
    }

    pub fn apply_point(&self, p: Vec3) -> Vec3 {
        self.apply_vec(p) + self.offset
    }

    /// This motion, then `next`.
    pub fn then(&self, next: &Transform) -> Transform {
        let col = |j: usize| Vec3::new(self.rows[0].component(j), self.rows[1].component(j), self.rows[2].component(j));
        let rows = [0, 1, 2].map(|i| Vec3::new(next.rows[i].dot(col(0)), next.rows[i].dot(col(1)), next.rows[i].dot(col(2))));
        Transform { rows, offset: next.apply_point(self.offset) }
    }

    pub fn inverse(&self) -> Transform {
        let r = &self.rows;
        let rows = [Vec3::new(r[0].x, r[1].x, r[2].x), Vec3::new(r[0].y, r[1].y, r[2].y), Vec3::new(r[0].z, r[1].z, r[2].z)];
        let back = Transform { rows, offset: Vec3::ZERO };
        Transform { rows, offset: -back.apply_vec(self.offset) }
    }
}

impl Vec3 {
    /// The x, y or z, by 0, 1 or 2.
    pub fn component(self, i: usize) -> f64 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
}
