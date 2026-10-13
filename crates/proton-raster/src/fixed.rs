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

/// The whole square root of `n`, rounded down.
pub fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    // Newton's method from above: an estimate that only ever falls.
    let mut x = n;
    let mut y = (x + 1) / 2;
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
        for n in [0u64, 1, 2, 3, 4, 15, 16, 17, 65535 * 65535, u32::MAX as u64, (1 << 62) + 12345] {
            let r = isqrt(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "√{n} = {r}");
        }
        assert_eq!(div_round(7, 2), 4);
        assert_eq!(div_round(5, 3), 2);
    }
}
