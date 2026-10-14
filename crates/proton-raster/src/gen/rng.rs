//! Chance, the same every time: a seeded PCG for things made in order, and
//! a hash of a seed and a place for things made anywhere — noise's lattice,
//! which a pixel asks for in whatever order it is drawn.

/// O'Neill's PCG32 (XSH RR), from a seed.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

const STEP: u64 = 6364136223846793005;
const INC: u64 = 1442695040888963407;

impl Rng {
    pub fn new(seed: u32) -> Rng {
        let mut r = Rng { state: 0 };
        r.draw();
        r.state = r.state.wrapping_add(seed as u64 ^ 0x9e37_79b9_7f4a_7c15);
        r.draw();
        r
    }

    /// The next number.
    pub fn draw(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(STEP).wrapping_add(INC);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        xorshifted.rotate_right((old >> 59) as u32)
    }

    /// A number from 0 to `n` − 1.
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.draw() as u64 * n as u64) >> 32) as u32
    }
}

/// A number for a seed and a place, well mixed.
pub fn hash(seed: u32, x: i64, y: i64) -> u32 {
    let mut h = seed ^ (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca77) ^ ((x >> 32) as u32).wrapping_mul(0xc2b2_ae3d);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    h
}
