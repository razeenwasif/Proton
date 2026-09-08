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

use proton_design::{Component, Theme, Token};

/// The theme that is open, and where the inspector is in it.
#[derive(Debug, Default)]
pub struct Design {
    theme: Option<Theme>,
    path: Option<PathBuf>,
    /// Which token the inspector has selected.
    selected: usize,
    /// Why there is no theme, when there is a reason worth showing.
    problem: Option<String>,
    /// The component the artboard draws, when one has been opened. Separate
    /// from the theme because they are two files with two lifetimes: a project
    /// has one palette and many components, and switching which component is
    /// on the artboard must not disturb the palette being edited beside it.
    component: Option<Component>,
    /// Why there is no component. Kept apart from `problem` for the same
    /// reason — "no theme" and "that file needs building first" are different
    /// sentences about different halves of the screen.
    trouble: Option<String>,
    /// Which layer the tree has selected. `usize::MAX` for none, which is what
    /// a fresh component has: opening a file should not assert that its first
    /// element is interesting.
    layer: usize,
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

    /// The component on the artboard, if there is one.
    pub fn component(&self) -> Option<&Component> {
        self.component.as_ref()
    }

    /// The element tree the layer panel draws.
    pub fn layers(&self) -> Vec<proton_design::Layer> {
        self.component
            .as_ref()
            .map(Component::layers)
            .unwrap_or_default()
    }

    /// Which layer is selected, or `usize::MAX`.
    pub fn layer(&self) -> usize {
        self.layer
    }

    /// Pick a layer. Out of range clears the selection rather than clamping:
    /// a click on a tree that has since changed under it should select
    /// nothing, not something else.
    pub fn select_layer(&mut self, index: usize) -> bool {
        let to = match index < self.layers().len() {
            true => index,
            false => usize::MAX,
        };
        let moved = to != self.layer;
        self.layer = to;
        moved
    }

    /// Where a layer was written: the file, and the line.
    pub fn layer_place(&self, index: usize) -> Option<(std::path::PathBuf, usize)> {
        let component = self.component.as_ref()?;
        let layer = component.layers().into_iter().nth(index)?;
        Some((component.path().to_path_buf(), layer.line))
    }

    pub fn trouble(&self) -> Option<&str> {
        self.trouble.as_deref()
    }

    /// Put a component on the artboard.
    ///
    /// A failure is remembered rather than only returned: pointing the
    /// artboard at a `.jsx` is an ordinary mistake, and the answer belongs on
    /// the artboard where somebody is looking, not only in a notice that
    /// scrolls away.
    pub fn show(&mut self, path: &Path) -> Result<(), String> {
        match Component::read(path) {
            Ok(component) => {
                self.component = Some(component);
                self.trouble = None;
                self.layer = usize::MAX;
                Ok(())
            }
            Err(why) => {
                // The old component is dropped. Leaving it up would mean the
                // artboard showing one file while the header named another.
                self.component = None;
                self.trouble = Some(why.clone());
                Err(why)
            }
        }
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
        // Both halves, and both asked every time — `||` would skip the second
        // whenever the first had changed, which is exactly the save that
        // touched a component and its stylesheet together.
        let theme = self.reload_theme();
        let component = self.reload_component();
        theme || component
    }

    fn reload_theme(&mut self) -> bool {
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

    /// The half the milestone is stated in terms of: *editing the component in
    /// Codex updates the artboard*.
    fn reload_component(&mut self) -> bool {
        let Some(path) = self.component.as_ref().map(|one| one.path().to_path_buf()) else {
            return false;
        };
        match Component::read(&path) {
            // Compared rather than assigned: this runs on every save in the
            // project, and repainting the artboard for a save that touched
            // something else would reload an iframe under somebody's cursor.
            Ok(fresh) if self.component.as_ref() != Some(&fresh) => {
                self.component = Some(fresh);
                self.trouble = None;
                // The file changed under the selection, so the index means
                // something else now. Cleared rather than kept: outlining the
                // wrong element is worse than outlining none.
                self.layer = usize::MAX;
                true
            }
            Ok(_) => false,
            // Deleted or renamed while open. Said, rather than leaving the
            // last good render up as though it were still the file.
            Err(why) => {
                let changed = self.trouble.as_deref() != Some(why.as_str());
                self.component = None;
                self.trouble = Some(why);
                self.layer = usize::MAX;
                changed
            }
        }
    }
}

/// Look for a component to put on the artboard.
///
/// Shallow and conventional, like [`discover`]: the point is to have something
/// on the artboard the first time Design mode is opened, not to guess right
/// about somebody's project. Anything found here is a starting point they can
/// change, and finding nothing is not a failure.
pub fn discover_component(root: &Path) -> Option<PathBuf> {
    // Components directories only. `ui/` and `src/` were in this list and are
    // not any more: they hold entry points, and pointing a *component*
    // artboard at a project's `index.html` puts the whole application shell on
    // it. Finding nothing is the better answer — the artboard falls back to
    // drawing the palette, which is still worth looking at.
    const DIRS: &[&str] = &[
        "components",
        "ui/components",
        "src/components",
        "app/components",
    ];

    for dir in DIRS {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        // Sorted, so the same project opens the same component twice. A
        // directory listing is in whatever order the filesystem felt like.
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && proton_design::is_markup(path))
            .collect();
        found.sort();
        if let Some(first) = found.into_iter().next() {
            return Some(first);
        }
    }
    None
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
