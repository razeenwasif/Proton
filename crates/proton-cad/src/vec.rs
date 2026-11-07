//! Points, directions and boxes, in millimetres.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// A point or a direction in space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A [`Vec3`] read as a place rather than a direction.
pub type Point3 = Vec3;

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    pub const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
    pub const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    pub const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    pub fn distance(self, o: Vec3) -> f64 {
        (self - o).length()
    }

    /// The unit vector this way, or `None` for one too short to have a way.
    pub fn normalized(self) -> Option<Vec3> {
        let l = self.length();
        (l.is_finite() && l > 1e-200).then(|| self / l)
    }

    /// The unit vector this way, or `fallback` for one too short to have a way.
    pub fn unit_or(self, fallback: Vec3) -> Vec3 {
        self.normalized().unwrap_or(fallback)
    }

    /// A unit vector at right angles to this one: crossed with the axis it
    /// lies least along, so the answer is never a near-zero cross product.
    pub fn perpendicular(self) -> Vec3 {
        let (ax, ay, az) = (self.x.abs(), self.y.abs(), self.z.abs());
        let other = if ax <= ay && ax <= az {
            Vec3::X
        } else if ay <= az {
            Vec3::Y
        } else {
            Vec3::Z
        };
        self.cross(other).unit_or(Vec3::X)
    }

    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self + (o - self) * t
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn min(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    pub fn max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }

    /// The largest of the three, ignoring sign.
    pub fn max_abs(self) -> f64 {
        self.x.abs().max(self.y.abs()).max(self.z.abs())
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Mul<Vec3> for f64 {
    type Output = Vec3;
    fn mul(self, v: Vec3) -> Vec3 {
        v * self
    }
}

impl Div<f64> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f64) -> Vec3 {
        Vec3::new(self.x / s, self.y / s, self.z / s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// A point or direction in a plane: a sketch's, or a meridian's.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2::new(0.0, 0.0);

    pub const fn new(x: f64, y: f64) -> Vec2 {
        Vec2 { x, y }
    }

    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// The z of the cross product: positive when `o` is anticlockwise of this.
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn distance(self, o: Vec2) -> f64 {
        (self - o).length()
    }

    pub fn normalized(self) -> Option<Vec2> {
        let l = self.length();
        (l.is_finite() && l > 1e-200).then(|| self * (1.0 / l))
    }

    /// Turned a quarter anticlockwise.
    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

/// A box with its sides along the axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// The box with these two corners, whichever way round they are given.
    pub fn new(a: Vec3, b: Vec3) -> Aabb {
        Aabb { min: a.min(b), max: a.max(b) }
    }

    /// The box with nothing in it, which any point grows.
    pub fn empty() -> Aabb {
        Aabb { min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY) }
    }

    pub fn of(points: impl IntoIterator<Item = Vec3>) -> Aabb {
        let mut b = Aabb::empty();
        for p in points {
            b.include(p);
        }
        b
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn include(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    /// Grown by `by` on every side.
    pub fn expanded(&self, by: f64) -> Aabb {
        let d = Vec3::new(by, by, by);
        Aabb { min: self.min - d, max: self.max + d }
    }

    /// Whether `p` is in it, or within `tol` of it.
    pub fn contains(&self, p: Vec3, tol: f64) -> bool {
        p.x >= self.min.x - tol && p.x <= self.max.x + tol && p.y >= self.min.y - tol && p.y <= self.max.y + tol && p.z >= self.min.z - tol && p.z <= self.max.z + tol
    }

    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.min.x <= o.max.x && o.min.x <= self.max.x && self.min.y <= o.max.y && o.min.y <= self.max.y && self.min.z <= o.max.z && o.min.z <= self.max.z
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// The length of the box's diagonal: how big it is, in one number.
    pub fn diagonal(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            (self.max - self.min).length()
        }
    }

    pub fn corners(&self) -> [Vec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            Vec3::new(a.x, a.y, a.z),
            Vec3::new(b.x, a.y, a.z),
            Vec3::new(a.x, b.y, a.z),
            Vec3::new(b.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z),
            Vec3::new(b.x, a.y, b.z),
            Vec3::new(a.x, b.y, b.z),
            Vec3::new(b.x, b.y, b.z),
        ]
    }

    /// The part of the line `origin + t·dir` inside the box, as its two
    /// parameters, or `None` if the line misses it.
    pub fn clip_line(&self, origin: Vec3, dir: Vec3) -> Option<(f64, f64)> {
        let mut t0 = f64::NEG_INFINITY;
        let mut t1 = f64::INFINITY;
        for (o, d, lo, hi) in [
            (origin.x, dir.x, self.min.x, self.max.x),
            (origin.y, dir.y, self.min.y, self.max.y),
            (origin.z, dir.z, self.min.z, self.max.z),
        ] {
            if d.abs() < 1e-300 {
                if o < lo || o > hi {
                    return None;
                }
                continue;
            }
            let (a, b) = ((lo - o) / d, (hi - o) / d);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        (t0 <= t1).then_some((t0, t1))
    }
}
