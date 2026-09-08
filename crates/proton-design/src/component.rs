//! The other half of Design mode's document: the component being drawn.
//!
//! [`Theme`](crate::Theme) owns the palette; this owns the markup the palette
//! is applied to. Together they are what the artboard renders, and the
//! milestone is stated in terms of both: *editing a token in the inspector
//! changes `theme.css`, and editing the component in Codex updates the
//! artboard.*
//!
//! ## Why this holds text and not a tree
//!
//! It would be easy to parse the markup into a node tree — a layer tree wants
//! one, and the eventual reverse binding (drag on the artboard, write the
//! source) will need one. It is deliberately not done here yet, because a tree
//! that can be *written back* has to reproduce the author's file byte for
//! byte apart from the change, the way [`Theme`](crate::Theme) does with
//! spans. A tree that can only be read is a tree that will be rewritten when
//! the writing half arrives, and building the wrong one first is how the
//! document format ends up serving the widget instead of the other way round.
//!
//! So: the source, and the questions that can be answered honestly from it.
//!
//! ## Only HTML, and that is a real limit
//!
//! A `.jsx` or `.svelte` file is not markup, it is a program that produces
//! markup, and rendering one means running the project's bundler. That is a
//! different piece of work with a different failure mode — a preview that goes
//! blank because somebody's build broke — and pretending otherwise by
//! string-stripping JSX into HTML would produce an artboard that lies about
//! what the component looks like.
//!
//! What is here renders `.html` and `.htm` honestly, and says so plainly about
//! anything else rather than showing a wrong picture.

use std::path::{Path, PathBuf};

/// A component file, as the artboard draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    path: PathBuf,
    source: String,
}

/// How large a file will be read into an artboard.
///
/// Two megabytes of markup is not a component, it is a bundled page or a
/// generated report, and putting it through an iframe on every save would cost
/// more than it shows.
const LARGEST: u64 = 2 << 20;

impl Component {
    /// Read a component from disk.
    pub fn read(path: &Path) -> Result<Component, String> {
        if !is_markup(path) {
            return Err(format!(
                "{} is not markup — the artboard renders .html, and a component \
                 written in JSX or Svelte has to be built before it is anything \
                 a browser can draw",
                name(path)
            ));
        }
        let big = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if big > LARGEST {
            return Err(format!(
                "{} is {} MB — too big to be a component",
                name(path),
                big / (1 << 20)
            ));
        }
        let source =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", name(path)))?;
        Ok(Component {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The markup, exactly as the file has it.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether the file is a whole page rather than a fragment.
    ///
    /// A fragment needs a document built around it before a browser will apply
    /// a palette to it; a whole page brings its own `<head>` and must not be
    /// wrapped in a second one. Told apart by looking for the tags that only a
    /// document has.
    pub fn is_document(&self) -> bool {
        let head = self
            .source
            .get(..2048)
            .unwrap_or(&self.source)
            .to_lowercase();
        head.contains("<!doctype") || head.contains("<html")
    }
}

/// Whether a path is something the artboard can draw without a build step.
pub fn is_markup(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_lowercase)
            .as_deref(),
        Some("html" | "htm")
    )
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "proton-component-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_fragment_and_a_page_are_told_apart() {
        // They are wrapped differently and wrapping a page twice gives it two
        // heads, which is not markup any browser agrees about.
        let fragment = Component::read(&write("card.html", "<div class=card>Hi</div>")).unwrap();
        assert!(!fragment.is_document());

        let page = Component::read(&write(
            "page.html",
            "<!doctype html>\n<html><body>Hi</body></html>",
        ))
        .unwrap();
        assert!(page.is_document());
    }

    #[test]
    fn the_source_is_returned_exactly_as_written() {
        // The artboard has to show what the file says, including the
        // whitespace somebody chose. Anything else and "editing the component
        // updates the artboard" is only approximately true.
        let text = "<div>\n  <p>one</p>\n\n  <p>two</p>\n</div>\n";
        let one = Component::read(&write("exact.html", text)).unwrap();
        assert_eq!(one.source(), text);
    }

    #[test]
    fn something_that_needs_building_says_so_rather_than_rendering_wrong() {
        // The failure worth avoiding is not an error, it is an artboard that
        // confidently shows JSX braces as text and lets somebody conclude
        // their component is broken.
        let why = Component::read(&write("Card.jsx", "export const Card = () => <div/>;"))
            .expect_err("JSX is not markup");
        assert!(why.contains("built"), "{why}");
    }
}
