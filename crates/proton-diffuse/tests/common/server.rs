//! A fake gateway on a loopback port: it records each request and answers
//! as a test says, all at once or in pieces with pauses between — as a run's
//! event stream comes.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A request as the fake saw it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// An answer: a status, a type and a body, sent in pieces after pauses
/// when `pieces` is not empty.
pub struct Reply {
    pub status: u16,
    pub kind: &'static str,
    pub body: Vec<u8>,
    pub pieces: Vec<(Duration, Vec<u8>)>,
}

impl Reply {
    pub fn json(status: u16, body: &str) -> Reply {
        Reply { status, kind: "application/json", body: body.as_bytes().to_vec(), pieces: Vec::new() }
    }

    pub fn bytes(kind: &'static str, body: Vec<u8>) -> Reply {
        Reply { status: 200, kind, body, pieces: Vec::new() }
    }

    /// An event stream, sent piece by piece.
    pub fn stream(pieces: Vec<(Duration, &str)>) -> Reply {
        let pieces = pieces.into_iter().map(|(wait, text)| (wait, text.as_bytes().to_vec())).collect();
        Reply { status: 200, kind: "text/event-stream", body: Vec::new(), pieces }
    }
}

pub struct Fake {
    /// Its `/img/v1`, as a gateway address.
    pub base: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

type Answer = Arc<dyn Fn(&Seen) -> Reply + Send + Sync>;

impl Fake {
    pub fn start(answer: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/img/v1", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let answer: Answer = Arc::new(answer);
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (answer, log) = (answer.clone(), log.clone());
                std::thread::spawn(move || serve(stream, &answer, &log));
            }
        });
        Fake { base, seen }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// An address nothing listens on: a port taken and let go.
pub fn nowhere() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/img/v1", listener.local_addr().unwrap());
    drop(listener);
    base
}

fn serve(mut stream: TcpStream, answer: &Answer, log: &Mutex<Vec<Seen>>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if stream.read_exact(&mut body).is_err() {
        return;
    }
    let seen = Seen { method, path, headers, body };
    log.lock().unwrap().push(seen.clone());
    let reply = answer(&seen);
    let mut out = format!("HTTP/1.1 {} Fake\r\nContent-Type: {}\r\nConnection: close\r\n", reply.status, reply.kind);
    if reply.pieces.is_empty() {
        out.push_str(&format!("Content-Length: {}\r\n\r\n", reply.body.len()));
        let _ = stream.write_all(out.as_bytes());
        let _ = stream.write_all(&reply.body);
    } else {
        out.push_str("\r\n");
        let _ = stream.write_all(out.as_bytes());
        for (wait, piece) in reply.pieces {
            std::thread::sleep(wait);
            if stream.write_all(&piece).and_then(|_| stream.flush()).is_err() {
                return;
            }
        }
    }
    let _ = stream.flush();
}
