//! Diffuse's files sealed on disk, so a picture, a mask or a prompt is not
//! anyone's to open who can open the folder: Finder, Quick Look, Spotlight
//! and a backup see only noise.
//!
//! A sealed file is its plain one's name with `.sealed` after it —
//! `image_0001.png.sealed` — holding a header, a nonce made afresh for each
//! write, and the bytes under AES-256-GCM, which also says whether they were
//! changed. The key is the person's, kept in their keychain by the app's
//! shell and handed to the core when they unlock Diffuse; nothing here
//! writes it anywhere.
//!
//! Everything Diffuse reads goes through [`read`], which takes a sealed file
//! where there is one and a plain file where there is not — a picture put
//! there by hand, or one from before sealing — so nothing goes missing on
//! the way over.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};

/// What a sealed file's name ends with.
pub const SEALED: &str = ".sealed";
/// What a sealed file starts with: what it is, and which way it was sealed.
const MAGIC: &[u8; 8] = b"PXSEAL1\0";
const NONCE: usize = 12;

/// The key Diffuse's files are sealed with. Its bytes are never printed.
#[derive(Clone)]
pub struct Seal(Arc<[u8; 32]>);

impl std::fmt::Debug for Seal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Seal(…)")
    }
}

impl Seal {
    pub fn new(key: [u8; 32]) -> Seal {
        Seal(Arc::new(key))
    }

    /// A key made afresh, from the operating system's randomness.
    pub fn generate() -> Seal {
        let key = Aes256Gcm::generate_key(&mut OsRng);
        Seal::new(key.into())
    }

    /// The key's bytes, for the keychain to keep.
    pub fn bytes(&self) -> [u8; 32] {
        *self.0
    }

    /// Whether `other` is the same key.
    pub fn same(&self, other: &Seal) -> bool {
        // Not in constant time: both keys are the person's own, in memory.
        self.0 == other.0
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.0[..]))
    }

    /// `plain`, sealed.
    pub fn seal(&self, plain: &[u8]) -> Vec<u8> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let sealed = self
            .cipher()
            .encrypt(&nonce, Payload { msg: plain, aad: MAGIC })
            .expect("AES-GCM seals anything under 64 GB");
        let mut out = Vec::with_capacity(MAGIC.len() + NONCE + sealed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        out
    }

    /// What `sealed` holds — or why it cannot be had: not a sealed file, or
    /// sealed with another key, or changed since.
    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, String> {
        if !is_sealed(sealed) {
            return Err("not a sealed file".into());
        }
        let nonce = Nonce::from_slice(&sealed[MAGIC.len()..MAGIC.len() + NONCE]);
        self.cipher()
            .decrypt(nonce, Payload { msg: &sealed[MAGIC.len() + NONCE..], aad: MAGIC })
            .map_err(|_| "sealed with another key, or changed since it was sealed".into())
    }
}

/// Whether `bytes` are a sealed file's.
pub fn is_sealed(bytes: &[u8]) -> bool {
    bytes.len() >= MAGIC.len() + NONCE + 16 && bytes.starts_with(MAGIC)
}

/// Where the sealed twin of `path` is.
pub fn sealed_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(SEALED);
    PathBuf::from(name)
}

/// The plain name a sealed file stands for; `None` for a name that is not
/// a sealed file's.
pub fn plain_name(name: &str) -> Option<&str> {
    name.strip_suffix(SEALED)
}

/// Whether there is a file for `path`, sealed or plain.
pub fn exists(path: &Path) -> bool {
    sealed_path(path).is_file() || path.is_file()
}

/// The bytes of the file `path` names: its sealed twin opened when there is
/// one — which needs the key — or the plain file.
pub fn read(path: &Path, seal: Option<&Seal>) -> Result<Vec<u8>, String> {
    let twin = sealed_path(path);
    if twin.is_file() {
        let seal = seal.ok_or_else(|| format!("{} is sealed: unlock Diffuse to read it", path.display()))?;
        let bytes = std::fs::read(&twin).map_err(|e| format!("{}: {e}", twin.display()))?;
        return seal.open(&bytes).map_err(|why| format!("{}: {why}", twin.display()));
    }
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write `bytes` for `path`, whole or not at all: sealed beside it with a
/// key, plain without. Where it was written.
pub fn write(path: &Path, bytes: &[u8], seal: Option<&Seal>) -> Result<PathBuf, String> {
    let (to, data) = match seal {
        Some(seal) => (sealed_path(path), seal.seal(bytes)),
        None => (path.to_path_buf(), bytes.to_vec()),
    };
    crate::doc::write_atomically(&to, &data).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_sealed_opens_with_its_key_only_and_not_once_changed() {
        let seal = Seal::new([7; 32]);
        let sealed = seal.seal(b"a lighthouse at dusk");
        assert!(is_sealed(&sealed));
        assert!(!sealed.windows(10).any(|w| w == b"lighthouse"), "the plain text shows through");
        assert_eq!(seal.open(&sealed).unwrap(), b"a lighthouse at dusk");
        // A nonce of its own each time.
        assert_ne!(seal.seal(b"same"), seal.seal(b"same"));
        assert!(Seal::new([8; 32]).open(&sealed).is_err());
        let mut changed = sealed.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(seal.open(&changed).is_err());
        assert!(seal.open(b"\x89PNG plain").is_err());
        assert_eq!(format!("{seal:?}"), "Seal(…)");
        assert!(Seal::generate().bytes() != Seal::generate().bytes());
    }

    #[test]
    fn a_file_is_read_sealed_where_it_is_and_plain_where_it_is_not() {
        let dir = std::env::temp_dir().join(format!("proton-diffuse-seal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let seal = Seal::new([3; 32]);
        let plain = dir.join("old.png");
        std::fs::write(&plain, b"plain bytes").unwrap();
        assert_eq!(read(&plain, None).unwrap(), b"plain bytes");
        let file = dir.join("new.png");
        let at = write(&file, b"new bytes", Some(&seal)).unwrap();
        assert_eq!(at, dir.join("new.png.sealed"));
        assert!(!file.exists() && exists(&file));
        assert_eq!(read(&file, Some(&seal)).unwrap(), b"new bytes");
        assert!(read(&file, None).unwrap_err().contains("unlock Diffuse"));
        assert_eq!(plain_name("new.png.sealed"), Some("new.png"));
        assert_eq!(plain_name("new.png"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
