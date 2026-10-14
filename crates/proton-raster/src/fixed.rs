//! Arithmetic in whole numbers, the same in every build.

/// `n / d`, rounded half up. `d` must not be zero.
pub fn div_round(n: u64, d: u64) -> u64 {
    (n + d / 2) / d
}

/// `a × b / 255`, rounded: the product of two 8-bit fractions.
pub fn mul255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}

/// `a × b / 65535`, rounded: the product of two 16-bit fractions, with no
/// division.
pub fn mul16(a: u32, b: u32) -> u32 {
    let t = a as u64 * b as u64 + 32768;
    ((t + (t >> 16)) >> 16) as u32
}

/// `x / 65535`, rounded, for `x` up to 65535²: [`div_round`] without a
/// division. With x = 65535q + r, the shifts make q plus one exactly when
/// r ≥ 32768.
pub fn div_m(x: u64) -> u64 {
    let t = x + 32768;
    (t + (t >> 16)) >> 16
}

/// `v / 257`, rounded, for `v` up to 65535: 65281 / 2²⁴ is 1/257 to within
/// a part in 2²⁴, too little to move a quotient whose fraction is a 257th.
pub fn div257(v: u64) -> u64 {
    ((v + 128) * 65281) >> 24
}

/// The whole square root of `n`, rounded down.
pub fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    // Newton's method from above: an estimate that only ever falls, begun
    // at the power of two just over the root — a few steps, not dozens.
    let bits = 64 - n.leading_zeros();
    let mut x = 1u64 << bits.div_ceil(2);
    let mut y = (x + n / x) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// FNV-1a over 64-bit words, read little-endian: a fingerprint two builds
/// agree on, for telling whether two tiles hold the same pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hash(pub u64);

impl Default for Hash {
    fn default() -> Hash {
        Hash(0xcbf2_9ce4_8422_2325)
    }
}

impl Hash {
    pub fn word(&mut self, w: u64) {
        self.0 ^= w;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        let mut words = bytes.chunks_exact(8);
        for w in &mut words {
            self.word(u64::from_le_bytes([w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]]));
        }
        let mut tail = [0u8; 8];
        let rest = words.remainder();
        tail[..rest.len()].copy_from_slice(rest);
        self.word(u64::from_le_bytes(tail));
        self.word(bytes.len() as u64);
    }

    pub fn hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_and_roots_are_exact() {
        for a in 0..=255u32 {
            for b in 0..=255u32 {
                assert_eq!(mul255(a, b), (a * b * 2 + 255) / 510, "{a} × {b}");
            }
        }
        for n in (0..100_000u64).chain([65535 * 65535, u32::MAX as u64, (1 << 62) + 12345, u64::MAX, u64::MAX - 1, (1 << 63) - 1]) {
            let r = isqrt(n);
            let (r, n) = (r as u128, n as u128);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "√{n} = {r}");
        }
        // Every product of a 16-bit fraction with a few hundred others, and
        // the ends.
        let mut b = 0u32;
        while b <= 65535 {
            for a in (0..=65535u32).step_by(97).chain([65534, 65535]) {
                assert_eq!(mul16(a, b) as u64, div_round(a as u64 * b as u64, 65535), "{a} × {b}");
            }
            b += if b < 300 { 1 } else { 251 };
        }
        assert_eq!(mul16(65535, 65535), 65535);
        for v in 0..=65535u64 {
            assert_eq!(div257(v), div_round(v, 257), "{v} / 257");
        }
        // Either side of every multiple of 65535 up to 65535², where a
        // rounding would go wrong, and a spread between.
        for q in 0..=65535u64 {
            for r in [0, 1, 32766, 32767, 32768, 32769, 65534] {
                let x = q * 65535 + r;
                if x <= 65535 * 65535 {
                    assert_eq!(div_m(x), div_round(x, 65535), "{x} / 65535");
                }
            }
        }
        let mut x = 0u64;
        while x <= 65535 * 65535 {
            assert_eq!(div_m(x), div_round(x, 65535), "{x} / 65535");
            x += 104_729;
        }
        assert_eq!(div_round(7, 2), 4);
        assert_eq!(div_round(5, 3), 2);
    }
}
