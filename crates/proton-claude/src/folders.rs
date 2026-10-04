//! Folders the person groups conversations into, as Claude's own apps let
//! them: named, collapsible, and holding conversations from any project and
//! any engine. A conversation is in one folder at most. Deleting a folder
//! lets its conversations go back to the list; it never deletes them.
//!
//! One file for every project, beside the conversations Proton keeps, so a
//! folder made in one project is there in the next. Each project's Chat tab
//! has its own copy in memory, so the file is read again whenever it has
//! changed and written the moment anything changes — two projects never
//! write over each other's folders.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::engine::Engine;
use crate::history::SessionSummary;

/// A folder, as kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub members: Vec<Member>,
}

/// A conversation in a folder: enough to list it and open it from any
/// project, whether or not that project's list has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    #[serde(default)]
    pub engine: Engine,
    /// The folder it runs in.
    pub cwd: PathBuf,
    /// As last seen, for when nothing newer can be read.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub updated: u64,
}

/// A folder as the list shows it, with its conversations newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "ChatFolder")]
#[serde(rename_all = "camelCase")]
pub struct FolderView {
    pub id: String,
    pub name: String,
    pub collapsed: bool,
    pub sessions: Vec<SessionSummary>,
}

/// The name a folder has until it is given one.
pub const UNNAMED: &str = "New folder";

pub struct Folders {
    file: PathBuf,
    list: Vec<Folder>,
    /// When the file was last read or written, to know when another
    /// project's Chat tab has changed it since.
    seen: Option<SystemTime>,
}

impl Folders {
    /// The folders, kept beside the conversations Proton keeps.
    pub fn open() -> Folders {
        Folders::at(&crate::saved::dir())
    }

    pub fn at(dir: &Path) -> Folders {
        let mut folders = Folders { file: dir.join("folders.json"), list: Vec::new(), seen: None };
        folders.fresh();
        folders
    }

    fn modified(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.file).and_then(|m| m.modified()).ok()
    }

    /// Read the file again if it has changed since it was last read.
    fn fresh(&mut self) {
        let modified = self.modified();
        if modified.is_some() && modified == self.seen {
            return;
        }
        self.list = std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        self.seen = modified;
    }

    fn save(&mut self) {
        if let Some(dir) = self.file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(text) = serde_json::to_string_pretty(&self.list) else { return };
        // Whole or not at all: to a neighbour, then renamed over.
        let partial = self.file.with_extension("json.partial");
        if std::fs::write(&partial, text + "\n").is_ok() && std::fs::rename(&partial, &self.file).is_ok() {
            self.seen = self.modified();
        }
    }

    /// Change the folders as they are on disk now, and write them back.
    fn change<T>(&mut self, edit: impl FnOnce(&mut Vec<Folder>) -> T) -> T {
        self.fresh();
        let out = edit(&mut self.list);
        self.save();
        out
    }

    /// Every folder, in the order they were made.
    pub fn list(&mut self) -> &[Folder] {
        self.fresh();
        &self.list
    }

    /// A new folder, by a name it is given or one that says it has none.
    /// Answers its id.
    pub fn create(&mut self, name: &str) -> String {
        let id = crate::process::uuid();
        let name = name.trim();
        let folder = id.clone();
        self.change(|list| {
            let name = if name.is_empty() { unique(list, UNNAMED) } else { name.to_string() };
            list.push(Folder { id: folder, name, collapsed: false, members: Vec::new() });
        });
        id
    }

    pub fn rename(&mut self, id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.change(|list| match list.iter_mut().find(|f| f.id == id) {
            Some(folder) => {
                folder.name = name.to_string();
                true
            }
            None => false,
        })
    }

    /// Delete a folder; its conversations go back to the list.
    pub fn delete(&mut self, id: &str) -> bool {
        self.change(|list| {
            let before = list.len();
            list.retain(|f| f.id != id);
            list.len() != before
        })
    }

    pub fn collapse(&mut self, id: &str, collapsed: bool) -> bool {
        self.change(|list| match list.iter_mut().find(|f| f.id == id) {
            Some(folder) => {
                folder.collapsed = collapsed;
                true
            }
            None => false,
        })
    }

    /// Put a conversation in a folder, taking it out of any other.
    pub fn put(&mut self, folder: &str, member: Member) -> bool {
        self.change(|list| {
            if !list.iter().any(|f| f.id == folder) {
                return false;
            }
            for f in list.iter_mut() {
                f.members.retain(|m| m.id != member.id);
            }
            if let Some(f) = list.iter_mut().find(|f| f.id == folder) {
                f.members.push(member);
            }
            true
        })
    }

    /// Take a conversation out of whichever folder has it.
    pub fn take_out(&mut self, session: &str) -> bool {
        self.change(|list| {
            let mut found = false;
            for f in list.iter_mut() {
                let before = f.members.len();
                f.members.retain(|m| m.id != session);
                found |= f.members.len() != before;
            }
            found
        })
    }

    /// The folder a conversation is in, and how it was put there.
    pub fn find(&mut self, session: &str) -> Option<(&Folder, &Member)> {
        self.fresh();
        self.list.iter().find_map(|f| f.members.iter().find(|m| m.id == session).map(|m| (f, m)))
    }

    /// What is known of a conversation now, kept for when nothing newer can
    /// be read — only written if it changed.
    pub fn remember(&mut self, session: &str, title: &str, updated: u64) {
        self.fresh();
        let stale = self
            .list
            .iter()
            .flat_map(|f| f.members.iter())
            .any(|m| m.id == session && (m.title != title || m.updated != updated) && !title.is_empty());
        if stale {
            self.change(|list| {
                for m in list.iter_mut().flat_map(|f| f.members.iter_mut()).filter(|m| m.id == session) {
                    m.title = title.to_string();
                    m.updated = updated;
                }
            });
        }
    }

    /// A conversation that has carried on under a new id — a fork of one the
    /// desktop app had open — is in the folder its original was in. The
    /// original stays: it is still a conversation of its own.
    pub fn follow(&mut self, old: &str, new: &str) {
        let Some((folder, member)) = self.find(old).map(|(f, m)| (f.id.clone(), m.clone())) else { return };
        self.change(|list| {
            if let Some(f) = list.iter_mut().find(|f| f.id == folder) {
                if !f.members.iter().any(|m| m.id == new) {
                    f.members.push(Member { id: new.to_string(), ..member });
                }
            }
        });
    }
}

/// A name no folder has yet: "New folder", then "New folder 2"…
fn unique(list: &[Folder], base: &str) -> String {
    if !list.iter().any(|f| f.name == base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} {n}")).find(|name| !list.iter().any(|f| &f.name == name)).unwrap_or_else(|| base.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str) -> Member {
        Member { id: id.into(), engine: Engine::Claude, cwd: PathBuf::from("/tmp/p"), title: format!("Chat {id}"), updated: 1 }
    }

    fn dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-folders-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_conversation_is_in_one_folder_and_a_deleted_folder_lets_it_go() {
        let dir = dir("one");
        let mut folders = Folders::at(&dir);
        let research = folders.create("Research");
        let unnamed = folders.create("  ");
        let again = folders.create("");
        let names: Vec<String> = folders.list().iter().map(|f| f.name.clone()).collect();
        assert_eq!(names, ["Research", "New folder", "New folder 2"]);

        assert!(folders.put(&research, member("a")));
        assert!(folders.put(&unnamed, member("a")), "moving it takes it out of the first");
        assert_eq!(folders.find("a").map(|(f, _)| f.id.clone()), Some(unnamed.clone()));
        assert!(folders.list().iter().find(|f| f.id == research).unwrap().members.is_empty());
        assert!(!folders.put("no-such-folder", member("b")));

        assert!(folders.rename(&again, "Proton"));
        assert!(!folders.rename(&again, "   "), "a folder keeps a name");
        assert!(folders.collapse(&unnamed, true));
        assert!(folders.delete(&unnamed));
        assert!(folders.find("a").is_none(), "its conversation is back in the list");
        assert!(folders.take_out("nothing") == false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_projects_each_see_what_the_other_changed() {
        let dir = dir("two");
        let mut here = Folders::at(&dir);
        let mut there = Folders::at(&dir);
        let shared = here.create("Shared");
        // A file's time is only so fine; a changed one has to look changed.
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(there.put(&shared, member("x")), "the other project sees the folder made here");
        std::thread::sleep(std::time::Duration::from_millis(20));
        here.rename(&shared, "Still shared");
        let folder = there.list().iter().find(|f| f.id == shared).unwrap().clone();
        assert_eq!((folder.name.as_str(), folder.members.len()), ("Still shared", 1), "neither wrote over the other");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_fork_joins_its_originals_folder_and_the_original_stays() {
        let dir = dir("fork");
        let mut folders = Folders::at(&dir);
        let f = folders.create("Desk");
        folders.put(&f, member("old"));
        folders.follow("old", "new");
        let ids: Vec<String> = folders.list()[0].members.iter().map(|m| m.id.clone()).collect();
        assert!(ids.contains(&"old".to_string()) && ids.contains(&"new".to_string()), "{ids:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
