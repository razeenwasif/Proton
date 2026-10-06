//! Notebook's notebooks, as folders a person can read: what they are made
//! of, and the sources they read.
//!
//! A notebook is a folder in a folder of the person's own —
//! `~/Github/Notebooks/<slug>/` — of files that diff in Git:
//!
//! ```text
//! notebook.json        the title, the course, the exam it is for
//! sources.json         S01, S02 …: each source, where it came from, how it reads
//! notes/N001.md        a note: TOML front matter, then Markdown
//! chats/C001.json      a conversation with the tutor
//! studio/              what was made: guides, maps, decks, quizzes, exams
//! tasks/J0001.json     every call that made something, and its prompt
//! .notebook/           machine state, not committed: the words read from
//!                      each source, which are ticked, the lock
//! ```
//!
//! A source is read once, when it is added, and its words are kept in
//! `.notebook/sources/` — so a citation of `S07 p. 12` still says the same
//! thing when the PDF it came from is edited, moved or evicted to iCloud.
//! Ids are never reused: a removed source's citations never come to mean
//! another.

pub mod cite;
pub mod clock;
pub mod deck;
pub mod fsrs;
pub mod lock;
pub mod mastery;
pub mod model;
pub mod quiz;
pub mod role;
pub mod snapshot;
pub mod store;
pub mod studio;
pub mod tex;
pub mod tutor;
pub mod view;

pub use model::{Note, Notebook, Origin, Role, Source, SourceStatus, Sources, Units};
pub use store::{Book, Folder};

/// Seconds since the epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
