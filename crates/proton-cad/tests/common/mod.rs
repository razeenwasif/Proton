//! What the kernel's tests share: a seeded random source, so a failure
//! comes back the same on the next run, and the surfaces they use.

#![allow(dead_code)]

use proton_cad::{Cone, Cylinder, Frame, Plane, Sphere, Surface, Torus, Vec3};

/// SplitMix64: enough randomness to scatter points, the same every run.
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// In `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    pub fn point(&mut self, size: f64) -> Vec3 {
        Vec3::new(self.range(-size, size), self.range(-size, size), self.range(-size, size))
    }

    pub fn direction(&mut self) -> Vec3 {
        loop {
            let v = self.point(1.0);
            let l = v.length();
            if l > 0.1 && l <= 1.0 {
                return v / l;
            }
        }
    }

    /// A frame somewhere, turned some way.
    pub fn frame(&mut self, size: f64) -> Frame {
        let origin = self.point(size);
        let z = self.direction();
        let x = self.direction();
        Frame::new(origin, z, x)
    }
}

/// One of each surface, placed and sized at random.
pub fn surfaces(rng: &mut Rng) -> Vec<Surface> {
    let f = |rng: &mut Rng| rng.frame(20.0);
    vec![
        Surface::Plane(Plane { frame: f(rng) }),
        Surface::Cylinder(Cylinder { frame: f(rng), radius: rng.range(1.0, 15.0) }),
        Surface::Cone(Cone { frame: f(rng), radius: rng.range(0.5, 10.0), half_angle: rng.range(0.2, 1.2) }),
        Surface::Sphere(Sphere { frame: f(rng), radius: rng.range(1.0, 15.0) }),
        Surface::Torus({
            let minor = rng.range(0.5, 4.0);
            Torus { frame: f(rng), major: minor + rng.range(1.0, 12.0), minor }
        }),
    ]
}

/// The parameters of a random point of the surface, kept away from a
/// cone's apex and a sphere's poles, where the parameters bunch up.
pub fn params(s: &Surface, rng: &mut Rng) -> (f64, f64) {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let u = rng.range(0.0, TAU);
    let v = match s {
        Surface::Plane(_) | Surface::Cylinder(_) => rng.range(-20.0, 20.0),
        Surface::Cone(c) => c.apex_v() + rng.range(0.5, 25.0),
        Surface::Sphere(_) => rng.range(-FRAC_PI_2 + 0.05, FRAC_PI_2 - 0.05),
        Surface::Torus(_) => rng.range(0.0, TAU),
    };
    (u, v)
}

pub fn kind(s: &Surface) -> &'static str {
    match s {
        Surface::Plane(_) => "plane",
        Surface::Cylinder(_) => "cylinder",
        Surface::Cone(_) => "cone",
        Surface::Sphere(_) => "sphere",
        Surface::Torus(_) => "torus",
    }
}
