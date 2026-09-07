//! Design mode: the theme file the inspector edits.
//!
//! `proton-design` owns the stylesheet format. What lives here is the editor's
//! half — which theme is open, which token the inspector is on, and what
//! happens to the file when somebody drags a colour.
//!
//! ## `.css` is not hijacked the way `.ipynb` is
//!
//! Opening a notebook switches to Data mode, because a notebook is not a thing
//! you usefully edit as text. A stylesheet is. More than that, M7's second
//! half is *editing the component in Codex updates the artboard* — so Codex
//! has to keep being able to open these files. Design is pointed at a theme
//! explicitly, or finds one when the mode is first entered; it never takes a
//! file away from the editor.
//!
//! ## A token edit reaches the disk
//!
//! The inspector is an editor of a file, not a view of one, and the milestone
//! it serves is stated as "editing a token in the inspector changes
//! `theme.css`". So `set` writes. There is no separate save, because a theme
//! with unsaved token changes is a palette that disagrees with the artboard
//! rendering it.

use std::path::{Path, PathBuf};

use proton_design::{Theme, Token};

/// The theme that is open, and where the inspector is in it.
#[derive(Debug, Default)]
pub struct Design {
    theme: Option<Theme>,
    path: Option<PathBuf>,
    /// Which token the inspector has selected.
    selected: usize,
    /// Why there is no theme, when there is a reason worth showing.
    problem: Option<String>,
}

impl Design {
    pub fn is_open(&self) -> bool {
        self.theme.is_some()
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

    /// The tokens, as the inspector draws them.
    pub fn tokens(&self) -> Vec<Token> {
        self.theme
            .as_ref()
            .map(|theme| theme.tokens().to_vec())
            .unwrap_or_default()
    }

    /// Read a stylesheet as the theme.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        let theme = Theme::parse(&text);
        if theme.tokens().is_empty() {
            // Not an error — a stylesheet with no custom properties is a
            // perfectly good stylesheet — but the inspector would be empty and
            // silent about why, which is the failure people cannot debug.
            self.problem = Some(format!(
                "{} has no custom properties to edit",
                path.display()
            ));
        } else {
            self.problem = None;
        }
        self.selected = 0;
        self.theme = Some(theme);
        self.path = Some(path.to_path_buf());
        Ok(())
    }

    /// Put the inspector on a token, clamped to one that exists.
    pub fn select(&mut self, index: usize) -> bool {
        let last = self.tokens().len().saturating_sub(1);
        let to = index.min(last);
        let moved = to != self.selected;
        self.selected = to;
        moved
    }

    /// Change a token and write the file.
    ///
    /// Both halves, or neither: a theme whose token changed in memory and not
    /// on disk is one where the next read puts the old colour back, and the
    /// artboard and the file would disagree about what the palette is.
    pub fn set(&mut self, name: &str, scope: &str, value: &str) -> Result<bool, String> {
        let Some(theme) = self.theme.as_mut() else {
            return Err("no theme is open".into());
        };
        let Some(path) = self.path.clone() else {
            return Err("no theme is open".into());
        };
        if !theme.set(name, scope, value) {
            // Either it is not there or it already had that value. Neither is
            // a failure and neither is worth a disk write.
            return Ok(false);
        }
        std::fs::write(&path, theme.to_text())
            .map_err(|err| format!("{}: {err}", path.display()))?;
        Ok(true)
    }

    /// Re-read the file from disk.
    ///
    /// The other half of the two-way binding: Codex edits `theme.css` as text
    /// like any other file, and the inspector has to notice rather than keep
    /// showing the palette as it was when the mode was opened.
    pub fn reload(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return false;
        };
        let fresh = Theme::parse(&text);
        if self.theme.as_ref().is_some_and(|now| now == &fresh) {
            return false;
        }
        self.theme = Some(fresh);
        self.selected = self.selected.min(self.tokens().len().saturating_sub(1));
        true
    }
}

/// Look for a theme under the project.
///
/// Conventional names first, then the stylesheet a `ui/` folder is most likely
/// to be using. Deliberately shallow: walking a whole project to find a `.css`
/// would pick up whatever a dependency shipped, and a palette editor pointed
/// at `node_modules` is worse than one pointed at nothing.
pub fn discover(root: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &["theme.css", "tokens.css", "styles.css", "style.css"];
    const DIRS: &[&str] = &["", "ui", "src", "styles", "css", "assets", "app"];

    for dir in DIRS {
        for name in NAMES {
            let path = match dir.is_empty() {
                true => root.join(name),
                false => root.join(dir).join(name),
            };
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}
