//! Game mode: the Godot scene the hierarchy shows.
//!
//! `proton-game` owns the file. What lives here is the editor's half — which
//! scene is open, which node the hierarchy is on, and what happens to the file
//! when somebody changes a property.
//!
//! The same shape as [`crate::modeling`] and [`crate::design`], because it is
//! the same job a third time. What differs is whose file it is: a `.tscn`
//! belongs to Godot, and Godot will open it next. That is why nothing here
//! re-prints anything.

use std::path::{Path, PathBuf};

use proton_game::{GameNode, Scene};

/// The scene that is open, and where the hierarchy is in it.
#[derive(Debug, Default)]
pub struct Game {
    scene: Option<Scene>,
    path: Option<PathBuf>,
    /// `usize::MAX` for none.
    selected: usize,
    problem: Option<String>,
}

impl Game {
    pub fn is_open(&self) -> bool {
        self.scene.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The file's text, for the pane that shows it.
    pub fn source(&self) -> String {
        self.scene
            .as_ref()
            .map(|one| one.to_text().to_string())
            .unwrap_or_default()
    }

    pub fn nodes(&self) -> Vec<GameNode> {
        self.scene.as_ref().map(Scene::nodes).unwrap_or_default()
    }

    /// Open a scene, remembering a failure as well as returning it.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        match Scene::read(path) {
            Ok(scene) => {
                self.scene = Some(scene);
                self.path = Some(path.to_path_buf());
                self.problem = None;
                self.selected = usize::MAX;
                Ok(())
            }
            Err(why) => {
                self.scene = None;
                self.path = Some(path.to_path_buf());
                self.problem = Some(why.clone());
                self.selected = usize::MAX;
                Err(why)
            }
        }
    }

    pub fn select(&mut self, index: usize) -> bool {
        let to = match index < self.nodes().len() {
            true => index,
            false => usize::MAX,
        };
        let moved = to != self.selected;
        self.selected = to;
        moved
    }

    /// Change a property, and write the file.
    pub fn set_property(&mut self, node: &str, name: &str, value: &str) -> Result<bool, String> {
        let Some(scene) = self.scene.as_mut() else {
            return Err("no scene is open".into());
        };
        if !scene.set_property(node, name, value)? {
            return Ok(false);
        }
        let Some(path) = self.path.clone() else {
            return Err("no scene is open".into());
        };
        scene.write(&path)?;
        Ok(true)
    }

    /// Re-read the file: a scene changed in Codex, by the agent, or by Godot
    /// itself while it was open beside us.
    pub fn reload(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        match Scene::read(&path) {
            Ok(fresh) if self.scene.as_ref() != Some(&fresh) => {
                self.scene = Some(fresh);
                self.problem = None;
                self.selected = usize::MAX;
                true
            }
            Ok(_) => false,
            Err(why) => {
                let changed = self.problem.as_deref() != Some(why.as_str());
                self.scene = None;
                self.problem = Some(why);
                self.selected = usize::MAX;
                changed
            }
        }
    }
}

/// Look for a Godot scene under the project.
///
/// `project.godot` first, because a Godot project says which scene it starts
/// with and that is the one somebody means. Only then the conventional places,
/// and only files that really parse — `.tscn` is common enough as a name that
/// finding one proves nothing.
pub fn discover(root: &Path) -> Option<PathBuf> {
    if let Some(main) = main_scene(root) {
        return Some(main);
    }
    const DIRS: &[&str] = &["", "scenes", "src", "game"];
    for dir in DIRS {
        let at = match dir.is_empty() {
            true => root.to_path_buf(),
            false => root.join(dir),
        };
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "tscn"))
            .collect();
        // Sorted, so the same project opens the same scene twice.
        found.sort();
        if let Some(first) = found.into_iter().find(|path| Scene::read(path).is_ok()) {
            return Some(first);
        }
    }
    None
}

/// The scene `project.godot` names as the one to run.
fn main_scene(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join("project.godot")).ok()?;
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("run/main_scene"))?;
    let named = line.split('=').nth(1)?.trim().trim_matches('"');
    // `res://` is Godot's way of saying "from the project root".
    let relative = named.strip_prefix("res://")?;
    let path = root.join(relative);
    Scene::read(&path).ok().map(|_| path)
}
