//! Language servers, from the core's side of the seam.
//!
//! Everything asynchronous is on the other side of this module: `proton-lsp`
//! owns the processes and the threads, and hands back a mailbox. What lives
//! here is state — which server speaks which language, what each of them has
//! been told, and what they have said — plus the translation between LSP's
//! coordinates and Proton's.
//!
//! That translation is the part worth being careful about. LSP counts columns
//! in UTF-16 code units; a rope counts characters. They agree for ASCII and
//! diverge on the first emoji, which is why they are converted here, once,
//! against the buffer that knows.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use proton_lsp::client::Incoming;
use proton_lsp::{Client, Position};
use proton_text::{Buffer, Cursor, Edit};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/* ── what the front end sees ──────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub severity: Severity,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

impl Severity {
    fn from_lsp(code: Option<u8>) -> Self {
        match code {
            Some(1) => Severity::Error,
            Some(2) => Severity::Warning,
            Some(4) => Severity::Hint,
            // Including `None`. The spec leaves an unlabelled diagnostic to
            // the client, and treating it as an error would let a stray note
            // make a clean file look broken.
            _ => Severity::Info,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    pub label: String,
    /// The dim trailing text: a signature, or the module it came from.
    pub hint: Option<String>,
    pub kind: CompletionKind,
}

/// LSP has twenty-five completion kinds. A reader scanning a popup can tell
/// about eight things apart, so this is the set the UI actually draws, and the
/// rest collapse into `Other` rather than each earning an icon nobody reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum CompletionKind {
    Function,
    Method,
    Field,
    Variable,
    Type,
    Module,
    Keyword,
    Constant,
    Snippet,
    Other,
}

impl CompletionKind {
    fn from_lsp(code: Option<u8>) -> Self {
        match code {
            Some(2) => CompletionKind::Method,
            Some(3) => CompletionKind::Function,
            Some(4) => CompletionKind::Function, // constructor
            Some(5) => CompletionKind::Field,
            Some(6) => CompletionKind::Variable,
            Some(7 | 8 | 22 | 25) => CompletionKind::Type, // class, interface, struct, param
            Some(9) => CompletionKind::Module,
            Some(14) => CompletionKind::Keyword,
            Some(15) => CompletionKind::Snippet,
            Some(21) => CompletionKind::Constant,
            _ => CompletionKind::Other,
        }
    }
}

/// How a server is doing, for the statusline. Not an error type: a language
/// with no server installed is a normal state, not a fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Starting,
    Ready,
    /// Indexing, or otherwise busy with work it has told us about.
    Busy,
    Unavailable,
    Stopped,
}

/* ── which server speaks what ─────────────────────────────────────── */

pub struct Language {
    pub id: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub extensions: &'static [&'static str],
}

/// The table is deliberately short and hard-coded for now. It becomes user
/// config in M4 along with the rest of the settings; hard-coding it until then
/// keeps a half-designed config format from being the thing that blocks
/// go-to-definition from working at all.
pub const LANGUAGES: &[Language] = &[
    Language {
        id: "rust",
        program: "rust-analyzer",
        args: &[],
        extensions: &["rs"],
    },
    Language {
        id: "typescript",
        program: "typescript-language-server",
        args: &["--stdio"],
        extensions: &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
    },
    Language {
        id: "python",
        program: "pyright-langserver",
        args: &["--stdio"],
        extensions: &["py", "pyi"],
    },
    Language {
        id: "go",
        program: "gopls",
        args: &[],
        extensions: &["go"],
    },
];

fn language_for(path: &Path) -> Option<&'static Language> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|l| l.extensions.contains(&extension.as_str()))
}

/* ── the servers we are talking to ────────────────────────────────── */

/// What a server was last told about a file.
#[derive(Debug)]
struct Announced {
    language: &'static str,
    /// The version we last sent. LSP only requires this to increase, so it is
    /// ours to count rather than the buffer's — an undo moves a buffer's
    /// version in ways a server must not have to reason about.
    sent: u64,
    /// The buffer version behind that send, so an unchanged buffer is not
    /// re-sent on every keystroke that turned out to be a motion.
    buffer: u64,
}

#[derive(Debug, Default)]
pub struct Servers {
    root: PathBuf,
    /// Off unless asked for. A core test must not spawn rust-analyzer as a
    /// side effect of opening a `.rs` file, and every core test opens one.
    enabled: bool,
    clients: HashMap<&'static str, Client>,
    /// Languages whose server would not start. Remembered so that opening a
    /// second Python file is not a second failed spawn, and a fourth, and a
    /// tenth.
    unavailable: HashSet<&'static str>,
    announced: HashMap<PathBuf, Announced>,
    diagnostics: HashMap<PathBuf, Vec<proton_lsp::Diagnostic>>,
    status: HashMap<&'static str, ServerStatus>,
    /// What each server last said it was working on. Kept rather than emitted
    /// per message: progress reports arrive several times a second, and a
    /// statusline does not need to see every one.
    notes: HashMap<&'static str, String>,
    awaiting_definition: Option<i64>,
}

impl Servers {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    /// Start talking to servers. Explicit, and off by default, because the
    /// alternative is a test suite that spawns four language servers.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Every server we have tried to start, and how it is doing. Sorted so the
    /// statusline does not reshuffle itself between polls.
    pub fn status(&self) -> Vec<(&'static str, ServerStatus, Option<String>)> {
        let mut all: Vec<_> = self
            .status
            .iter()
            .map(|(id, status)| (*id, *status, self.notes.get(id).cloned()))
            .collect();
        all.sort_by_key(|(id, ..)| *id);
        all
    }

    /// The server for this file, started if it is not already running.
    fn ensure(&mut self, path: &Path) -> Option<&'static str> {
        if !self.enabled {
            return None;
        }
        let language = language_for(path)?;
        if self.unavailable.contains(language.id) {
            return None;
        }
        if !self.clients.contains_key(language.id) {
            let args: Vec<String> = language.args.iter().map(|s| (*s).to_string()).collect();
            match Client::spawn(language.id, language.program, &args, &self.root) {
                Ok(client) => {
                    self.clients.insert(language.id, client);
                    self.status.insert(language.id, ServerStatus::Starting);
                }
                Err(_) => {
                    // Not an error worth interrupting anyone over: most people
                    // do not have gopls installed, and most of them are not
                    // writing Go today either.
                    self.unavailable.insert(language.id);
                    self.status.insert(language.id, ServerStatus::Unavailable);
                    return None;
                }
            }
        }
        Some(language.id)
    }

    /// Tell the server about a buffer — opening it the first time, and
    /// re-syncing it after that.
    pub fn sync(&mut self, buffer: &Buffer) {
        let Some(path) = buffer.path().map(proton_lsp::uri::plain) else {
            return;
        };
        let Some(language) = self.ensure(&path) else {
            return;
        };

        let buffer_version = buffer.version();
        let previous = self.announced.get(&path);
        if previous.is_some_and(|a| a.buffer == buffer_version) {
            return;
        }
        let sent = previous.map_or(1, |a| a.sent + 1);
        let first = previous.is_none();
        let text = buffer.contents();

        if let Some(client) = self.clients.get_mut(language) {
            if first {
                client.did_open(&path, language, sent, &text);
            } else {
                client.did_change(&path, sent, &text);
            }
        }
        self.announced.insert(
            path,
            Announced {
                language,
                sent,
                buffer: buffer_version,
            },
        );
    }

    pub fn saved(&mut self, buffer: &Buffer) {
        let Some(path) = buffer.path().map(proton_lsp::uri::plain) else {
            return;
        };
        let Some(language) = self.announced.get(&path).map(|a| a.language) else {
            return;
        };
        let text = buffer.contents();
        if let Some(client) = self.clients.get_mut(language) {
            client.did_save(&path, &text);
        }
    }

    /// The client responsible for a file, if it is one we have announced.
    /// Asking about a file no server has been told about is not a question
    /// worth answering.
    fn client_for(&mut self, path: &Path) -> Option<&mut Client> {
        let language = self.announced.get(path)?.language;
        self.clients.get_mut(language)
    }

    pub fn completion(&mut self, path: &Path, at: Position) -> Option<i64> {
        let path = proton_lsp::uri::plain(path);
        Some(self.client_for(&path)?.completion(&path, at))
    }

    pub fn definition(&mut self, path: &Path, at: Position) -> Option<i64> {
        let path = proton_lsp::uri::plain(path);
        let id = self.client_for(&path)?.definition(&path, at);
        self.awaiting_definition = Some(id);
        Some(id)
    }

    /// True if this is the jump we are still waiting for, rather than one from
    /// a keypress the user has since changed their mind about.
    pub fn wanted_definition(&mut self, id: i64) -> bool {
        if self.awaiting_definition == Some(id) {
            self.awaiting_definition = None;
            return true;
        }
        false
    }

    /// Take everything the servers have said. Never blocks.
    pub fn poll(&mut self) -> Vec<(&'static str, Incoming)> {
        let mut all = Vec::new();
        for (language, client) in self.clients.iter_mut() {
            for message in client.poll() {
                all.push((*language, message));
            }
        }

        for (language, message) in &all {
            match message {
                Incoming::Ready { .. } => {
                    self.status.insert(language, ServerStatus::Ready);
                }
                Incoming::Progress {
                    title,
                    message,
                    done,
                } => {
                    self.status.insert(
                        language,
                        if *done {
                            ServerStatus::Ready
                        } else {
                            ServerStatus::Busy
                        },
                    );
                    if *done {
                        self.notes.remove(language);
                    } else {
                        // "indexing 40%" reads better than either half alone.
                        self.notes.insert(
                            language,
                            match message {
                                Some(detail) => format!("{title} {detail}"),
                                None => title.clone(),
                            },
                        );
                    }
                }
                Incoming::Exited => {
                    self.status.insert(language, ServerStatus::Stopped);
                }
                Incoming::Diagnostics { path, items } => {
                    self.diagnostics.insert(path.clone(), items.clone());
                }
                _ => {}
            }
        }
        all
    }

    pub fn diagnostics_for(&self, path: &Path) -> &[proton_lsp::Diagnostic] {
        self.diagnostics
            .get(&proton_lsp::uri::plain(path))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

/* ── coordinates ──────────────────────────────────────────────────── */

/// Where the cursor is, as a server counts it.
pub fn position_of(buffer: &Buffer, offset: usize) -> Position {
    Position {
        line: buffer.position(offset).line as u32,
        character: buffer.utf16_column(offset) as u32,
    }
}

/// A diagnostic in the buffer's own coordinates.
///
/// Converted against the buffer as it is *now*, not as it was when the server
/// spoke. Diagnostics always arrive slightly late; clamping them to something
/// real beats holding a stale copy of the text to be exactly right about a
/// squiggle.
pub fn diagnostic(buffer: &Buffer, raw: &proton_lsp::Diagnostic) -> Diagnostic {
    let start = buffer.position(buffer.offset_at_utf16(
        raw.range.start.line as usize,
        raw.range.start.character as usize,
    ));
    let end = buffer.position(buffer.offset_at_utf16(
        raw.range.end.line as usize,
        raw.range.end.character as usize,
    ));
    Diagnostic {
        line: start.line,
        column: start.column,
        end_line: end.line,
        end_column: end.column,
        severity: Severity::from_lsp(raw.severity),
        message: raw.message.clone(),
        source: raw.source.clone(),
        code: raw.code.as_ref().map(ToString::to_string),
    }
}

/* ── the completion popup ─────────────────────────────────────────── */

/// A server can answer with several hundred candidates. Past the first screen
/// nobody is reading them, and every one costs a row in an event that has to
/// cross the bridge on a keystroke.
const MOST: usize = 200;

#[derive(Debug, Default)]
pub struct Popup {
    /// Asked for, though possibly not yet showing anything.
    wanted: bool,
    /// The request whose answer we still want. A reply to an older one is
    /// stale — the cursor has moved since we asked.
    awaiting: Option<i64>,
    items: Vec<Completion>,
    raw: Vec<proton_lsp::CompletionItem>,
    selected: usize,
}

impl Popup {
    /// Whether the popup owns the keyboard. Deliberately false while the
    /// first reply is still in flight: stealing Enter from someone who is
    /// typing, on the chance that a list might arrive, is worse than being
    /// slow.
    pub fn is_open(&self) -> bool {
        self.wanted && !self.items.is_empty()
    }

    pub fn expect(&mut self, id: i64) {
        self.wanted = true;
        self.awaiting = Some(id);
    }

    /// Accept a reply, unless it answers a question we have stopped asking.
    pub fn fill(&mut self, id: i64, items: Vec<proton_lsp::CompletionItem>) -> bool {
        if self.awaiting != Some(id) {
            return false;
        }
        self.awaiting = None;
        self.raw = items;

        // Servers rank by `sortText`, not by label — that is the whole point
        // of the field. Ignoring it is the difference between the obvious
        // candidate being first and being fortieth.
        self.raw.sort_by(|a, b| {
            let key = |i: &proton_lsp::CompletionItem| {
                i.sort_text.clone().unwrap_or_else(|| i.label.clone())
            };
            key(a).cmp(&key(b))
        });
        self.raw.truncate(MOST);

        self.items = self
            .raw
            .iter()
            .map(|item| Completion {
                label: item.label.clone(),
                hint: item.hint(),
                kind: CompletionKind::from_lsp(item.kind),
            })
            .collect();
        self.selected = 0;
        true
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    pub fn move_by(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as i32;
        self.selected = (((self.selected as i32 + delta) % len + len) % len) as usize;
    }

    /// Pick a row outright, for a click. The keyboard moves by delta; a mouse
    /// already knows which row it is on.
    pub fn select(&mut self, index: usize) {
        if index < self.items.len() {
            self.selected = index;
        }
    }

    pub fn items(&self) -> &[Completion] {
        &self.items
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn chosen(&self) -> Option<&proton_lsp::CompletionItem> {
        self.raw.get(self.selected)
    }
}

/// Put a chosen completion in the buffer.
///
/// Pure, and separate from the workspace, because the interesting part is not
/// the insertion but the clamping — and that is worth being able to test
/// without a language server in the room.
pub fn apply_completion(buffer: &mut Buffer, item: &proton_lsp::CompletionItem) {
    let (range, text) = item.edit();
    let cursor = buffer.cursor.offset;

    let (start, end) = match range {
        // A `textEdit` is how a server says "you typed `HashM`, replace all of
        // it". Appending instead is how you get `HashMHashMap`.
        Some(range) => (
            buffer.offset_at_utf16(range.start.line as usize, range.start.character as usize),
            buffer.offset_at_utf16(range.end.line as usize, range.end.character as usize),
        ),
        None => (cursor, cursor),
    };

    // The list can be a keystroke or two behind the buffer, so the span is
    // clamped to end at the cursor. Replacing what was typed is the point;
    // eating what comes after it never is.
    let start = start.min(cursor);
    let end = end.clamp(start, cursor);
    let landing = start + text.chars().count();
    buffer.apply(vec![Edit { start, end, text }], Cursor::at(landing));
}
