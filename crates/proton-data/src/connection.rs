//! The connection file, and the ports in it.
//!
//! A kernel is not told where to listen — it is *handed* a JSON file naming
//! five ports and a signing key, and it binds them. Which means the ports have
//! to be chosen before the kernel exists, and chosen so that nothing else
//! takes them in between.
//!
//! The honest way to do that is to bind them here first and hold them until
//! the moment the kernel starts. Asking the OS for a free port, closing it and
//! writing the number down is the version that works on a quiet machine and
//! fails on a busy one — the gap between closing and the kernel binding is a
//! race, and losing it means a kernel that exits with "address in use".

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What a kernel is told, and what a client reads back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub transport: String,
    pub ip: String,
    pub shell_port: u16,
    pub iopub_port: u16,
    pub stdin_port: u16,
    pub control_port: u16,
    pub hb_port: u16,
    /// The HMAC key. Empty means unsigned, which the protocol allows.
    pub key: String,
    pub signature_scheme: String,
    #[serde(default)]
    pub kernel_name: String,
    /// The kernel's process, once it has one, and the Proton that started
    /// it. Not Jupyter's, and ignored by a kernel: what lets a Proton started
    /// after one that crashed find the kernels nobody can reach — see
    /// `kernel::reap`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
}

impl Connection {
    /// Five ports nothing else is using, and a fresh key.
    ///
    /// The ports come from `reserve`, which binds them to find out. They are
    /// released just before the kernel is launched — see `Reserved`.
    pub fn open(key: String) -> std::io::Result<(Connection, Reserved)> {
        let held = Reserved::five()?;
        let ports = held.ports();
        Ok((
            Connection {
                transport: "tcp".into(),
                ip: "127.0.0.1".into(),
                shell_port: ports[0],
                iopub_port: ports[1],
                stdin_port: ports[2],
                control_port: ports[3],
                hb_port: ports[4],
                key,
                signature_scheme: "hmac-sha256".into(),
                kernel_name: String::new(),
                pid: None,
                parent: None,
            },
            held,
        ))
    }

    pub fn read(path: &Path) -> Result<Connection, String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Written whole or not at all, by a rename: it is written again once the
    /// kernel has a pid, and the kernel may be reading it as it starts. And
    /// readable by its owner alone, since it names the key that lets anybody
    /// who reads it run code in the kernel.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        let mut part = path.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        let _ = std::fs::remove_file(&part);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options
            .open(&part)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .and_then(|()| std::fs::rename(&part, path))
            .map_err(|err| {
                let _ = std::fs::remove_file(&part);
                format!("{}: {err}", path.display())
            })
    }

    pub fn address(&self, port: u16) -> String {
        format!("{}://{}:{}", self.transport, self.ip, port)
    }
}

/// Ports held open so nothing else can take them.
///
/// Dropping this releases them. That is the whole point: they are held from
/// the moment they are chosen until the moment the kernel is spawned, so the
/// window in which another process could take one is as small as it can be
/// made without the kernel choosing its own ports — which the protocol does
/// not allow.
#[derive(Debug)]
pub struct Reserved {
    listeners: Vec<std::net::TcpListener>,
}

impl Reserved {
    fn five() -> std::io::Result<Reserved> {
        let listeners = (0..5)
            .map(|_| std::net::TcpListener::bind("127.0.0.1:0"))
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(Reserved { listeners })
    }

    fn ports(&self) -> Vec<u16> {
        self.listeners
            .iter()
            .map(|l| l.local_addr().map(|a| a.port()).unwrap_or(0))
            .collect()
    }

    /// Let them go, just before the kernel binds them.
    pub fn release(self) {
        drop(self);
    }
}

/// Where a connection file for this kernel should live.
///
/// Under the OS temp directory rather than the project: it names a signing key
/// and dies with the kernel, so it is neither the user's business nor
/// something a repository should ever see.
pub fn connection_file(id: &str) -> PathBuf {
    connection_dir().join(format!("{PREFIX}{id}.json"))
}

/// Where every connection file is.
pub(crate) fn connection_dir() -> PathBuf {
    std::env::temp_dir()
}

/// What every connection file's name starts with.
const PREFIX: &str = "proton-kernel-";

/// Whether a file's name is one `connection_file` gives.
#[cfg(unix)]
pub(crate) fn is_connection_file(name: &str) -> bool {
    name.starts_with(PREFIX) && name.ends_with(".json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    /// The five ports a kernel is told to bind.
    fn ports_of(connection: &Connection) -> [u16; 5] {
        [
            connection.shell_port,
            connection.iopub_port,
            connection.stdin_port,
            connection.control_port,
            connection.hb_port,
        ]
    }

    #[test]
    fn five_ports_are_reserved_and_all_different() {
        let (connection, held) = Connection::open("k".into()).unwrap();
        let ports = ports_of(&connection);
        assert!(ports.iter().all(|p| *p != 0), "{ports:?}");

        let mut unique = ports.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            5,
            "a port used twice is a kernel that cannot start"
        );
        held.release();
    }

    #[test]
    fn the_ports_really_are_held_until_released() {
        // The property the whole approach rests on. If binding again while
        // they are held succeeded, they would not be reserved at all and the
        // race this avoids would still be there.
        let (connection, held) = Connection::open("k".into()).unwrap();
        let ports = ports_of(&connection);
        for port in ports {
            assert!(
                TcpListener::bind(("127.0.0.1", port)).is_err(),
                "port {port} was not actually held"
            );
        }

        held.release();
        // And once released, a kernel can have them — given a moment. A
        // kernel test running beside this one forks to start its kernel, and
        // until the child execs it holds a copy of every socket open in this
        // process, these among them: for that moment, closing ours leaves the
        // port taken. One still taken after all of the wait is one `release`
        // kept.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut kept = ports.to_vec();
        loop {
            kept.retain(|port| TcpListener::bind(("127.0.0.1", *port)).is_err());
            if kept.is_empty() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(kept.is_empty(), "ports {kept:?} were never released");
    }

    #[test]
    fn a_connection_file_round_trips() {
        let (connection, held) = Connection::open("sekrit".into()).unwrap();
        held.release();
        let path = std::env::temp_dir().join(format!(
            "proton-conn-test-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        connection.write(&path).unwrap();

        let read = Connection::read(&path).unwrap();
        assert_eq!(read.key, "sekrit");
        assert_eq!(read.shell_port, connection.shell_port);
        assert_eq!(read.signature_scheme, "hmac-sha256");
        assert_eq!(
            read.address(read.shell_port),
            format!("tcp://127.0.0.1:{}", read.shell_port)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_connection_file_is_an_error_not_a_panic() {
        assert!(Connection::read(Path::new("/no/such/kernel.json")).is_err());
    }
}
