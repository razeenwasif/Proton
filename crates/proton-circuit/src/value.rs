//! A net's value: 1 to 64 bits, each 0, 1 or high-Z — the bits no driver
//! drives, kept as a mask beside the value.

/// A value `bits` wide; a bit set in `z` is high-Z and its bit in `v` is 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value {
    pub v: u64,
    pub z: u64,
    pub bits: u8,
}

/// The bits of a width set.
pub fn mask(bits: u8) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

impl Value {
    pub fn new(v: u64, bits: u8) -> Value {
        Value { v: v & mask(bits), z: 0, bits }
    }

    /// No bit driven.
    pub fn high_z(bits: u8) -> Value {
        Value { v: 0, z: mask(bits), bits }
    }

    pub fn is_z(&self) -> bool {
        self.z != 0
    }

    /// As a number, high-Z bits read as 0 — what a gate makes of a
    /// floating input.
    pub fn get(&self) -> u64 {
        self.v & !self.z & mask(self.bits)
    }

    pub fn bit(&self, i: u8) -> bool {
        (self.get() >> i) & 1 == 1
    }

    /// As a signed number of its width.
    pub fn signed(&self) -> i64 {
        let b = self.bits.clamp(1, 64);
        if b == 64 {
            return self.get() as i64;
        }
        let v = self.get();
        if v >> (b - 1) & 1 == 1 { (v | !mask(b)) as i64 } else { v as i64 }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.z == mask(self.bits) {
            return f.write_str("Z");
        }
        if self.bits == 1 {
            return write!(f, "{}", self.get());
        }
        write!(f, "0x{:x}", self.get())
    }
}

/// Drivers of one net, one value each, resolved: bits only one drives are
/// that driver's, bits none drives stay high-Z (unless pulled), and bits two
/// drive differently are a conflict — `None` then, with the bits that clash.
pub fn resolve(bits: u8, drivers: &[Value], pull: Option<bool>) -> Result<Value, u64> {
    let m = mask(bits);
    let mut driven = 0u64;
    let mut v = 0u64;
    let mut clash = 0u64;
    for d in drivers {
        let on = !d.z & m;
        let both = driven & on;
        clash |= both & (v ^ d.v);
        v |= d.v & on;
        driven |= on;
    }
    if clash != 0 {
        return Err(clash);
    }
    let z = m & !driven;
    match pull {
        Some(true) => Ok(Value { v: v | z, z: 0, bits }),
        Some(false) => Ok(Value { v, z: 0, bits }),
        None => Ok(Value { v, z, bits }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drivers_resolve_and_clash() {
        let a = Value { v: 0b1010, z: 0b0101, bits: 4 };
        let b = Value { v: 0b0001, z: 0b1110, bits: 4 };
        assert_eq!(resolve(4, &[a, b], None), Ok(Value { v: 0b1011, z: 0b0100, bits: 4 }), "bit 2 is nobody's");
        assert_eq!(resolve(4, &[a, b], Some(true)), Ok(Value { v: 0b1111, z: 0, bits: 4 }));
        assert_eq!(resolve(4, &[Value::new(1, 4), Value::new(0, 4)], None), Err(1));
        assert_eq!(Value::new(0xff, 8).signed(), -1);
        assert_eq!(Value::new(0x7f, 8).signed(), 127);
        assert_eq!(format!("{}", Value::high_z(1)), "Z");
    }
}
