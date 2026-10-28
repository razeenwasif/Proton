//! Zip archives, as much of them as `.xlsx` needs: entries stored or
//! deflated, read from a file's central directory and written with fixed
//! times, so the same workbook is the same bytes.

use std::io::{Read, Write};

/// No entry of a workbook is near this; a file claiming more is refused
/// rather than unpacked into all of memory.
const MAX_ENTRY: u64 = 1 << 30;

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// CRC-32 as zip has it (the IEEE polynomial, reflected).
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
    let mut c = !0u32;
    for b in data {
        c = table[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// An archive read: each entry's name and where its bytes are.
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
}

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    local: usize,
}

impl<'a> Archive<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Archive<'a>, String> {
        // The end record is in the last 64 KiB and a bit: its comment can be
        // that long.
        let from = bytes.len().saturating_sub(22 + 65_535);
        let end = (from..bytes.len().saturating_sub(21))
            .rev()
            .find(|&i| u32_at(bytes, i) == Some(0x0605_4b50))
            .ok_or("not a zip archive: it has no end record")?;
        let count = u16_at(bytes, end + 10).ok_or("a cut-off zip")? as usize;
        let offset = u32_at(bytes, end + 16).ok_or("a cut-off zip")?;
        if offset == u32::MAX || count == u16::MAX as usize {
            return Err("a zip64 archive, which is not read: save it again smaller".into());
        }
        let mut entries = Vec::with_capacity(count);
        let mut i = offset as usize;
        for _ in 0..count {
            if u32_at(bytes, i) != Some(0x0201_4b50) {
                return Err("a zip whose directory is damaged".into());
            }
            let bad = || "a cut-off zip directory".to_string();
            let method = u16_at(bytes, i + 10).ok_or_else(bad)?;
            let crc = u32_at(bytes, i + 16).ok_or_else(bad)?;
            let compressed = u32_at(bytes, i + 20).ok_or_else(bad)? as u64;
            let size = u32_at(bytes, i + 24).ok_or_else(bad)? as u64;
            let name_len = u16_at(bytes, i + 28).ok_or_else(bad)? as usize;
            let extra_len = u16_at(bytes, i + 30).ok_or_else(bad)? as usize;
            let comment_len = u16_at(bytes, i + 32).ok_or_else(bad)? as usize;
            let local = u32_at(bytes, i + 42).ok_or_else(bad)? as usize;
            let name = bytes.get(i + 46..i + 46 + name_len).ok_or_else(bad)?;
            entries.push(Entry { name: String::from_utf8_lossy(name).replace('\\', "/"), method, crc, compressed, size, local });
            i += 46 + name_len + extra_len + comment_len;
        }
        Ok(Archive { bytes, entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name == name)
    }

    /// An entry's bytes, unpacked and checked.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let e = self.entries.iter().find(|e| e.name == name).ok_or_else(|| format!("{name} is not in the archive"))?;
        if e.size > MAX_ENTRY {
            return Err(format!("{name} is too large to read"));
        }
        let b = self.bytes;
        if u32_at(b, e.local) != Some(0x0403_4b50) {
            return Err(format!("{name}: its header is damaged"));
        }
        let name_len = u16_at(b, e.local + 26).ok_or("a cut-off zip")? as usize;
        let extra_len = u16_at(b, e.local + 28).ok_or("a cut-off zip")? as usize;
        let start = e.local + 30 + name_len + extra_len;
        let data = b.get(start..start + e.compressed as usize).ok_or_else(|| format!("{name} is cut off"))?;
        let out = match e.method {
            0 => data.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(e.size as usize);
                flate2::read::DeflateDecoder::new(data).take(MAX_ENTRY).read_to_end(&mut out).map_err(|err| format!("{name}: {err}"))?;
                out
            }
            m => return Err(format!("{name} is packed a way that is not read (method {m})")),
        };
        if crc32(&out) != e.crc {
            return Err(format!("{name} is damaged: its checksum does not match"));
        }
        Ok(out)
    }

    pub fn text(&self, name: &str) -> Result<String, String> {
        let bytes = self.read(name)?;
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        String::from_utf8(bytes.to_vec()).map_err(|_| format!("{name} is not UTF-8"))
    }
}

/// An archive of `(name, bytes)` entries, each deflated, with the times
/// zip's epoch gives so nothing in it says when it was made.
pub fn write(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    let date: u16 = (1 << 5) | 1; // 1980-01-01
    for (name, data) in entries {
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
        enc.write_all(data).map_err(|e| e.to_string())?;
        let packed = enc.finish().map_err(|e| e.to_string())?;
        let crc = crc32(data);
        let (size, csize) = (u32::try_from(data.len()), u32::try_from(packed.len()));
        let (Ok(size), Ok(csize)) = (size, csize) else {
            return Err(format!("{name} is too large for a zip"));
        };
        let offset = u32::try_from(out.len()).map_err(|_| "the workbook is too large for a zip".to_string())?;
        let name_bytes = name.as_bytes();
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // needed
            h.extend(0x0800u16.to_le_bytes()); // names in UTF-8
            h.extend(8u16.to_le_bytes()); // deflate
            h.extend(0u16.to_le_bytes()); // time
            h.extend(date.to_le_bytes());
            h.extend(crc.to_le_bytes());
            h.extend(csize.to_le_bytes());
            h.extend(size.to_le_bytes());
            h.extend((name_bytes.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes()); // extra
            if central {
                h.extend(0u16.to_le_bytes()); // comment
                h.extend(0u16.to_le_bytes()); // disk
                h.extend(0u16.to_le_bytes()); // internal attributes
                h.extend(0u32.to_le_bytes()); // external attributes
                h.extend(offset.to_le_bytes());
            }
            h.extend(name_bytes);
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(&packed);
        directory.extend(header(0x0201_4b50, true));
    }
    let cd_offset = out.len() as u32;
    let cd_len = directory.len() as u32;
    out.extend(directory);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // disks
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend(cd_len.to_le_bytes());
    out.extend(cd_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back_and_the_same_twice() {
        let entries = vec![("a.txt".to_string(), b"hello hello hello".to_vec()), ("dir/b.xml".to_string(), "<x>é</x>".repeat(100).into_bytes())];
        let zip = write(&entries).unwrap();
        assert_eq!(zip, write(&entries).unwrap(), "byte for byte");
        let a = Archive::new(&zip).unwrap();
        assert_eq!(a.names().collect::<Vec<_>>(), vec!["a.txt", "dir/b.xml"]);
        assert_eq!(a.read("a.txt").unwrap(), b"hello hello hello");
        assert_eq!(a.text("dir/b.xml").unwrap(), "<x>é</x>".repeat(100));
        assert!(a.read("nope").is_err());
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let mut bad = zip.clone();
        bad[40] ^= 0xFF;
        assert!(Archive::new(&bad).and_then(|a| a.read("a.txt")).is_err(), "damage is found");
        assert!(Archive::new(b"not a zip").is_err());
    }

    #[test]
    fn the_systems_unzip_reads_what_is_written() {
        let dir = std::env::temp_dir().join(format!("proton-sheet-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.zip");
        std::fs::write(&path, write(&[("x/y.txt".into(), b"inside".to_vec())]).unwrap()).unwrap();
        match std::process::Command::new("unzip").arg("-p").arg(&path).arg("x/y.txt").output() {
            Ok(out) => {
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
                assert_eq!(out.stdout, b"inside");
            }
            Err(_) => eprintln!("no unzip here; skipped"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
