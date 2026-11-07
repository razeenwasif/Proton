//! Zip archives, stored rather than compressed: what a 3MF package is.
//! Every entry is dated 1980-01-01, so the same parts make the same bytes.

/// The CRC-32 a zip entry carries (IEEE, reflected).
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// An archive of `(name, contents)`, in order.
pub fn write(entries: &[(&str, &[u8])]) -> Vec<u8> {
    const DATE: u16 = (1 << 5) | 1; // 1980-01-01
    let mut out = Vec::new();
    let mut central = Vec::new();
    let w16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let w32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let (n, len) = (name.len() as u16, data.len() as u32);
        w32(&mut out, 0x0403_4b50);
        for x in [20, 0x0800, 0, 0, DATE] {
            w16(&mut out, x);
        }
        for x in [crc, len, len] {
            w32(&mut out, x);
        }
        w16(&mut out, n);
        w16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        w32(&mut central, 0x0201_4b50);
        for x in [20, 20, 0x0800, 0, 0, DATE] {
            w16(&mut central, x);
        }
        for x in [crc, len, len] {
            w32(&mut central, x);
        }
        for x in [n, 0, 0, 0, 0] {
            w16(&mut central, x);
        }
        w32(&mut central, 0);
        w32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let (at, size) = (out.len() as u32, central.len() as u32);
    out.extend_from_slice(&central);
    w32(&mut out, 0x0605_4b50);
    for x in [0, 0, entries.len() as u16, entries.len() as u16] {
        w16(&mut out, x);
    }
    w32(&mut out, size);
    w32(&mut out, at);
    w16(&mut out, 0);
    out
}

/// A stored archive's entries, each checked against its CRC.
pub fn read(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let u16_at = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes(b.try_into().unwrap()) as usize).ok_or("cut short");
    let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or("cut short");
    let end = (0..bytes.len().saturating_sub(21)).rev().find(|&i| u32_at(i) == Ok(0x0605_4b50)).ok_or("no end of the central directory")?;
    let (count, mut at) = (u16_at(end + 10)?, u32_at(end + 16)?);
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return Err("a broken central directory".into());
        }
        let (method, crc, size) = (u16_at(at + 10)?, u32_at(at + 16)? as u32, u32_at(at + 20)?);
        let (n, extra, comment, local) = (u16_at(at + 28)?, u16_at(at + 30)?, u16_at(at + 32)?, u32_at(at + 42)?);
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + n).ok_or("cut short")?).into_owned();
        if method != 0 {
            return Err(format!("`{name}` is compressed"));
        }
        let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let data = bytes.get(start..start + size).ok_or("cut short")?.to_vec();
        if crc32(&data) != crc {
            return Err(format!("`{name}` fails its CRC"));
        }
        out.push((name, data));
        at += 46 + n + extra + comment;
    }
    Ok(out)
}
