//! Circuit mode: the circuit project open in this project, and its saves.
//!
//! `proton-circuit` owns the format and the simulator. What lives here is
//! the editor's half: which `Name.circuit/` folder is open, which of its
//! circuits is on screen, whether it has changed since it was saved, and the
//! projects a folder holds, for the home page shown when none is.
//!
//! The window is sent a circuit as it draws it: each part with its pins
//! already placed, as the simulator will join them, so a pin drawn is a pin
//! that connects. `generation` numbers each project opened, so an answer
//! about the last one is never taken for this.
//!
//! Edits are held until ⌘S, as Codex's are, and the quit prompt names an
//! unsaved project.

use std::path::{Path, PathBuf};

use proton_circuit::circ::{self, Drawing, Project};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::workspace::notice;

/// A circuit project's folder ends so.
pub const EXTENSION: &str = "circuit";

/// A circuit project found in the folder, for the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitFound {
    /// As the project names it.
    pub path: String,
    pub name: String,
}

/// A pin as drawn, in grid steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitPin {
    pub x: i32,
    pub y: i32,
    pub input: bool,
}

/// A part as drawn: where it is, how it is turned, its settings, and its
/// pins where the simulator joins them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitPartView {
    pub id: String,
    /// `and`, `in`, `mux`… or `sub:<circuit>`.
    pub kind: String,
    pub x: i32,
    pub y: i32,
    pub rot: u8,
    pub mirror: bool,
    /// Its settings by their `.circ` names, sorted.
    pub settings: Vec<(String, String)>,
    pub pins: Vec<CircuitPin>,
    /// Why its pins could not be placed, when they could not.
    pub problem: Option<String>,
}

/// The project open, and the circuit of it on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CircuitProjectView {
    /// As the project names it.
    pub path: String,
    pub name: String,
    /// The circuit that runs when the project does.
    pub top: String,
    pub circuits: Vec<String>,
    /// The circuit on screen.
    pub shown: String,
    pub parts: Vec<CircuitPartView>,
    /// Each wire's ends: x, y, x, y in grid steps.
    pub wires: Vec<[i32; 4]>,
    /// The labels of the shown circuit's test tables.
    pub tests: Vec<String>,
    pub generation: u32,
    pub dirty: bool,
}

struct Open {
    path: PathBuf,
    shown_path: String,
    name: String,
    project: Project,
    /// As it was last read or written: unsaved when they differ.
    saved: Project,
    shown: String,
    generation: u32,
}

impl std::fmt::Debug for Open {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Open").field("shown_path", &self.shown_path).field("generation", &self.generation).finish()
    }
}

/// The circuit project open in a project.
#[derive(Debug, Default)]
pub struct Circuits {
    open: Option<Open>,
    generations: u32,
}

impl Circuits {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Everything the window needs to draw the mode.
    pub fn events(&self, root: &Path) -> Vec<Event> {
        vec![home_event(root), self.project_event()]
    }

    pub fn apply(&mut self, root: &Path, command: Command) -> Vec<Event> {
        match command {
            Command::CircuitNew { name } => self.new_project(root, &name),
            Command::CircuitOpen { path } => {
                let dir = match inside(root, &path) {
                    Ok(dir) => dir,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                if self.open.as_ref().is_some_and(|o| o.path == dir) {
                    return self.events(root);
                }
                if self.dirty() {
                    return vec![notice("save or discard the open circuit's changes before opening another".into(), NoticeLevel::Warn)];
                }
                match circ::read(&dir) {
                    Ok(project) => {
                        self.show(root, dir, project);
                        self.events(root)
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::CircuitClose { discard } => {
                if !discard && self.dirty() {
                    return vec![notice("the circuit has unsaved changes".into(), NoticeLevel::Warn)];
                }
                self.open = None;
                self.events(root)
            }
            Command::CircuitSave => {
                let Some(open) = &mut self.open else { return Vec::new() };
                match circ::write_project(&open.path, &open.project) {
                    Ok(()) => {
                        open.saved = open.project.clone();
                        self.events(root)
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::CircuitShow { circuit } => {
                let Some(open) = &mut self.open else { return Vec::new() };
                if !open.project.circuits.contains_key(&circuit) {
                    return vec![notice(format!("there is no circuit {circuit}"), NoticeLevel::Warn)];
                }
                open.shown = circuit;
                vec![self.project_event()]
            }
            Command::CircuitAdd { name } => {
                let Some(open) = &mut self.open else { return Vec::new() };
                let name = name.trim().to_string();
                if !circ::is_name(&name) {
                    return vec![notice(format!("“{name}” cannot name a circuit: letters, digits, spaces, - and _"), NoticeLevel::Warn)];
                }
                if open.project.circuits.contains_key(&name) {
                    return vec![notice(format!("there is a circuit {name} already"), NoticeLevel::Warn)];
                }
                open.project.circuits.insert(name.clone(), Drawing::default());
                open.shown = name;
                vec![self.project_event()]
            }
            Command::CircuitSetTop { circuit } => {
                let Some(open) = &mut self.open else { return Vec::new() };
                if !open.project.circuits.contains_key(&circuit) {
                    return vec![notice(format!("there is no circuit {circuit}"), NoticeLevel::Warn)];
                }
                open.project.top = circuit;
                vec![self.project_event()]
            }
            _ => Vec::new(),
        }
    }

    fn new_project(&mut self, root: &Path, name: &str) -> Vec<Event> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return vec![notice("a new circuit needs a name".into(), NoticeLevel::Warn)];
        }
        if self.dirty() {
            return vec![notice("save or discard the open circuit's changes before making another".into(), NoticeLevel::Warn)];
        }
        let folder = format!("{}.{EXTENSION}", slug(&name));
        let dir = match inside(root, &folder) {
            Ok(dir) => dir,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        if dir.exists() {
            return vec![notice(format!("{folder} is already there"), NoticeLevel::Warn)];
        }
        let project = Project::new();
        if let Err(why) = circ::write_project(&dir, &project) {
            return vec![notice(why, NoticeLevel::Warn)];
        }
        self.show(root, dir, project);
        let mut events = self.events(root);
        events.push(notice(format!("made {name} in {folder}"), NoticeLevel::Info));
        events
    }

    fn show(&mut self, root: &Path, path: PathBuf, project: Project) {
        self.generations += 1;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let rel = path.strip_prefix(root).or_else(|_| path.strip_prefix(&real));
        self.open = Some(Open {
            shown_path: rel.map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.display().to_string()),
            path,
            name,
            shown: project.top.clone(),
            saved: project.clone(),
            project,
            generation: self.generations,
        });
    }

    fn dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.project != o.saved)
    }

    /// The project with unsaved changes, as the folder names it.
    pub fn unsaved(&self) -> Vec<String> {
        match &self.open {
            Some(open) if open.project != open.saved => vec![open.shown_path.clone()],
            _ => Vec::new(),
        }
    }

    /// Save it, as quitting does; what could not be, and why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let Some(open) = &mut self.open else { return Vec::new() };
        if open.project == open.saved {
            return Vec::new();
        }
        match circ::write_project(&open.path, &open.project) {
            Ok(()) => {
                open.saved = open.project.clone();
                Vec::new()
            }
            Err(why) => vec![format!("{}: {why}", open.shown_path)],
        }
    }

    pub fn project_event(&self) -> Event {
        let Some(open) = &self.open else { return Event::CircuitProject { project: None } };
        let p = &open.project;
        let drawing = p.circuits.get(&open.shown).cloned().unwrap_or_default();
        let mut pins: std::collections::HashMap<String, Result<Vec<circ::PinAt>, String>> = p.pins(&open.shown).into_iter().collect();
        let parts = drawing
            .parts
            .iter()
            .map(|part| {
                let placed = pins.remove(&part.id).unwrap_or_else(|| Ok(Vec::new()));
                let (pins, problem) = match placed {
                    Ok(pins) => (pins.into_iter().map(|p| CircuitPin { x: p.x, y: p.y, input: p.input }).collect(), None),
                    Err(why) => (Vec::new(), Some(why)),
                };
                CircuitPartView {
                    id: part.id.clone(),
                    kind: part.kind.clone(),
                    x: part.x,
                    y: part.y,
                    rot: part.rot,
                    mirror: part.mirror,
                    settings: part.attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    pins,
                    problem,
                }
            })
            .collect();
        Event::CircuitProject {
            project: Some(Box::new(CircuitProjectView {
                path: open.shown_path.clone(),
                name: open.name.clone(),
                top: p.top.clone(),
                circuits: p.circuits.keys().cloned().collect(),
                shown: open.shown.clone(),
                parts,
                wires: drawing.wires.iter().map(|w| [w.a.0, w.a.1, w.b.0, w.b.1]).collect(),
                tests: p.tests.get(&open.shown).map(|t| t.keys().cloned().collect()).unwrap_or_default(),
                generation: open.generation,
                dirty: open.project != open.saved,
            })),
        }
    }
}

/// A `.circuit` folder named relative to the project, refused if it is not
/// one or leads outside.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || !named.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{rel} is not in the project"));
    }
    if named.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
        return Err(format!("{rel} is not a circuit: a circuit is a .{EXTENSION} folder"));
    }
    let base = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let path = base.join(named);
    let mut held = path.as_path();
    let real = loop {
        match std::fs::canonicalize(held) {
            Ok(real) => break real,
            Err(_) => match held.parent() {
                Some(up) => held = up,
                None => return Err(format!("{rel} is nowhere")),
            },
        }
    };
    if !real.starts_with(&base) {
        return Err(format!("{rel} leads outside the project"));
    }
    Ok(path)
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "circuit".into()
    } else {
        out
    }
}

pub fn home_event(root: &Path) -> Event {
    Event::CircuitHome { projects: find(root) }
}

/// The circuit projects in a folder, three folders deep.
fn find(root: &Path) -> Vec<CircuitFound> {
    const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor"];
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut looked = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            looked += 1;
            if looked > 5000 || out.len() >= 500 {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) || !path.is_dir() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some(EXTENSION) {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                out.push(CircuitFound { path: rel, name: name.trim_end_matches(&format!(".{EXTENSION}")).to_string() });
            } else if depth < 3 {
                stack.push((path, depth + 1));
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-core-circuit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn view(events: &[Event]) -> Option<CircuitProjectView> {
        events.iter().rev().find_map(|e| match e {
            Event::CircuitProject { project } => project.as_deref().cloned(),
            _ => None,
        })
    }

    #[test]
    fn a_project_is_made_found_drawn_held_and_saved() {
        let root = scratch("made");
        let mut c = Circuits::default();
        let made = c.apply(&root, Command::CircuitNew { name: "Full Adder".into() });
        let v = view(&made).unwrap();
        assert_eq!((v.path.as_str(), v.top.as_str(), v.dirty), ("full-adder.circuit", "main", false));
        assert!(root.join("full-adder.circuit/project.json").exists());
        let found = match &c.events(&root)[0] {
            Event::CircuitHome { projects } => projects.clone(),
            _ => unreachable!(),
        };
        assert_eq!(found, vec![CircuitFound { path: "full-adder.circuit".into(), name: "full-adder".into() }]);

        // A circuit with parts, drawn with its pins placed.
        std::fs::write(root.join("full-adder.circuit/circuits/main.circ"), "part A in at 0,0 label=A\npart U and at 2,0\npart Z frob at 9,9\nwire 0,0 2,0\n").unwrap();
        c.apply(&root, Command::CircuitClose { discard: false });
        let v = view(&c.apply(&root, Command::CircuitOpen { path: "full-adder.circuit".into() })).unwrap();
        let and = v.parts.iter().find(|p| p.id == "U").unwrap();
        assert_eq!(and.pins, vec![CircuitPin { x: 2, y: 0, input: true }, CircuitPin { x: 2, y: 2, input: true }, CircuitPin { x: 5, y: 1, input: false }]);
        assert!(v.parts.iter().find(|p| p.id == "Z").unwrap().problem.as_deref().unwrap().contains("frob"));
        assert_eq!(v.wires, vec![[0, 0, 2, 0]]);

        // A circuit added is held until ⌘S.
        let v = view(&c.apply(&root, Command::CircuitAdd { name: "half".into() })).unwrap();
        assert!(v.dirty && v.shown == "half");
        assert_eq!(c.unsaved(), vec!["full-adder.circuit".to_string()]);
        assert!(!root.join("full-adder.circuit/circuits/half.circ").exists());
        assert!(c.apply(&root, Command::CircuitClose { discard: false }).iter().any(|e| matches!(e, Event::Notice { .. })));
        let v = view(&c.apply(&root, Command::CircuitSave)).unwrap();
        assert!(!v.dirty);
        assert!(root.join("full-adder.circuit/circuits/half.circ").exists());
        assert!(c.unsaved().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn folders_that_are_not_circuits_or_lead_outside_are_refused() {
        let root = scratch("outside");
        let mut c = Circuits::default();
        for bad in ["../x.circuit", "notes.txt", "/tmp/a.circuit", ""] {
            let events = c.apply(&root, Command::CircuitOpen { path: bad.into() });
            assert!(events.iter().any(|e| matches!(e, Event::Notice { .. })), "{bad}");
        }
        assert!(!c.is_open());
        let _ = std::fs::remove_dir_all(root);
    }
}
