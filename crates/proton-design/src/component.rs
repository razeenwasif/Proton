//! The other half of Design mode's document: the component being drawn.
//!
//! [`Theme`](crate::Theme) owns the palette; this owns the markup the palette
//! is applied to. Together they are what the artboard renders, and the
//! milestone is stated in terms of both: *editing a token in the inspector
//! changes `theme.css`, and editing the component in Codex updates the
//! artboard.*
//!
//! ## The text is what is held; the tree is derived from it
//!
//! This owns the source, byte for byte. [`layers`](Component::layers) reads a
//! tree out of it on demand — see [`markup`](crate::markup) — rather than the
//! tree being the thing stored and the text being printed back from it.
//!
//! That order is deliberate and it is the same one [`Theme`](crate::Theme)
//! uses. A tree that can be *written back* has to reproduce the author's file
//! exactly apart from the change, so its nodes carry the spans they came from;
//! a tree that *is* the document has to be printed, and printing produces a
//! diff touching every line. Every mode's document diffing in Git is the rule
//! this project is built on, so the file stays the truth and the tree stays a
//! view of it.
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
//!
//! ## Writing back is a splice, never a re-print
//!
//! [`set_text`](Component::set_text) and
//! [`set_attribute`](Component::set_attribute) replace the bytes one thing
//! occupies and move nothing else. Not because it is cheaper — printing a tree
//! would be less code — but because a component is somebody's file. It has the
//! indentation they chose, the blank lines they use to group things, comments
//! explaining a hack, and attribute order that means something to them. A
//! writer that re-printed would turn every rename into a whole-file diff, and
//! *every mode's document diffs in Git* is the rule this project is built on.
//!
//! That is the same reason [`Theme`](crate::Theme) splices, and the reason
//! [`Layer`](crate::Layer) carries spans at all.

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

    /// Replace a layer's text.
    ///
    /// Only a leaf: an element with children has layers inside it, and writing
    /// text over those would delete them. Typing in a text box does not mean
    /// "remove everything under this", so it is refused rather than obeyed.
    ///
    /// Answers whether anything changed. Setting a value to what it already is
    /// is not a failure and not worth a write.
    pub fn set_text(&mut self, index: usize, text: &str) -> Result<bool, String> {
        let layer = self.layer(index)?;
        if !layer.leaf {
            return Err(format!(
                "<{}> has elements inside it — editing its text would delete them",
                layer.tag
            ));
        }
        let inner = layer.inner.clone();
        if self.source.get(inner.clone()) == Some(text) {
            return Ok(false);
        }
        // Refused rather than escaped. Escaping would silently write something
        // other than what was typed, and a design tool that turns `<b>` into
        // `&lt;b&gt;` behind somebody's back is worse than one that says no.
        if text.contains('<') || text.contains('>') {
            return Err("text cannot contain < or > — edit the markup in Codex".into());
        }
        self.source.replace_range(inner, text);
        Ok(true)
    }

    /// Set an attribute on a layer, adding it if it is not there.
    ///
    /// An empty value removes it, because that is what clearing the box in an
    /// inspector means — an element left with `class=""` would be a change
    /// nobody asked for sitting in the diff.
    pub fn set_attribute(&mut self, index: usize, name: &str, value: &str) -> Result<bool, String> {
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!("{name:?} is not an attribute name"));
        }
        if value.contains('"') || value.contains('<') || value.contains('>') {
            return Err("an attribute value cannot contain \" < or >".into());
        }
        let layer = self.layer(index)?;
        let open = layer.open.clone();
        let tag = self.source[open.clone()].to_string();

        let Some(found) = super::markup::attribute_span(&tag, name) else {
            if value.is_empty() {
                return Ok(false);
            }
            // Inserted before the `>`, or before `/>` — written where an
            // author would write it, at the end of the attributes, so the diff
            // is one added attribute rather than a reordered tag.
            let at = match tag.trim_end().ends_with("/>") {
                true => tag.rfind('/').unwrap_or(tag.len() - 1),
                false => tag.len() - 1,
            };
            let pad = match self.source.as_bytes()[open.start + at - 1].is_ascii_whitespace() {
                true => "",
                false => " ",
            };
            self.source
                .insert_str(open.start + at, &format!("{pad}{name}=\"{value}\""));
            return Ok(true);
        };

        let whole = open.start + found.whole.start..open.start + found.whole.end;
        let just_value = open.start + found.value.start..open.start + found.value.end;
        if value.is_empty() {
            self.source.replace_range(whole, "");
            return Ok(true);
        }
        if self.source.get(just_value.clone()) == Some(value) {
            return Ok(false);
        }
        self.source.replace_range(just_value, value);
        Ok(true)
    }

    /// The markup as it now stands, for whoever is writing it to disk.
    pub fn text(&self) -> &str {
        &self.source
    }

    fn layer(&self, index: usize) -> Result<crate::Layer, String> {
        self.layers()
            .into_iter()
            .nth(index)
            .ok_or_else(|| format!("there is no layer {index}"))
    }

    /// The markup, exactly as the file has it.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The element tree, for the layer panel.
    ///
    /// Parsed on demand rather than kept: a component is re-read whenever it
    /// changes, and a tree cached beside the text is one more thing that can
    /// disagree with it. Parsing this file's own 92 KB takes under a
    /// millisecond, which is not worth a cache that can go stale.
    pub fn layers(&self) -> Vec<crate::Layer> {
        crate::layers(&self.source)
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

    /// A file with everything a writer could disturb: comments, blank lines,
    /// odd indentation, attribute order, a quote style.
    const FUSSY: &str = "<!-- the card. do not reformat -->\n<section  class='panel'   id=\"card\">\n\n  <h3 id=\"head\">Pricing</h3>\n\n  <p>Body copy.</p>\n</section>\n";

    #[test]
    fn writing_text_moves_nothing_else_in_the_file() {
        // The rule the whole project rests on. A design tool that reformats is
        // one whose every edit is a whole-file diff, and nobody reviews those.
        let mut one = Component::read(&write("fussy.html", FUSSY)).unwrap();
        // The `<h3>`, which is layer 1.
        assert!(one.set_text(1, "Plans").unwrap());

        let want = FUSSY.replace(">Pricing<", ">Plans<");
        assert_eq!(one.text(), want, "something other than the text moved");
    }

    #[test]
    fn writing_an_attribute_moves_nothing_else_either() {
        let mut one = Component::read(&write("fussy-attr.html", FUSSY)).unwrap();
        assert!(one.set_attribute(0, "class", "panel wide").unwrap());
        // The single quotes the author used are theirs; only what is between
        // them changes.
        assert_eq!(one.text(), FUSSY.replace("'panel'", "'panel wide'"));
    }

    #[test]
    fn an_attribute_that_is_not_there_is_added_where_one_would_write_it() {
        let mut one = Component::read(&write("add-attr.html", FUSSY)).unwrap();
        assert!(one.set_attribute(2, "id", "body").unwrap());
        assert!(
            one.text().contains(r#"<p id="body">Body copy.</p>"#),
            "{}",
            one.text()
        );
    }

    #[test]
    fn clearing_an_attribute_removes_it_rather_than_emptying_it() {
        // `class=""` left behind is a change nobody asked for, sitting in the
        // diff and doing nothing.
        let mut one = Component::read(&write("clear-attr.html", FUSSY)).unwrap();
        assert!(one.set_attribute(1, "id", "").unwrap());
        assert_eq!(one.text(), FUSSY.replace(" id=\"head\"", ""));
    }

    #[test]
    fn setting_what_is_already_there_is_not_a_write() {
        // Every keystroke in an inspector arrives here. Writing the file on
        // each one would put the disk in the typing loop.
        let mut one = Component::read(&write("same.html", FUSSY)).unwrap();
        assert!(!one.set_text(1, "Pricing").unwrap());
        assert!(!one.set_attribute(1, "id", "head").unwrap());
    }

    #[test]
    fn text_is_refused_on_an_element_that_has_elements_inside_it() {
        // Obeying would delete them. Typing in a text box does not mean
        // "remove everything under this".
        let mut one = Component::read(&write("branch.html", FUSSY)).unwrap();
        let why = one
            .set_text(0, "gone")
            .expect_err("a branch is not editable");
        assert!(why.contains("delete"), "{why}");
        assert_eq!(one.text(), FUSSY, "it wrote anyway");
    }

    #[test]
    fn markup_typed_into_a_text_box_is_refused_rather_than_escaped() {
        // Escaping would write something other than what was typed. A tool
        // that silently turns <b> into &lt;b&gt; is worse than one that says
        // no and points at the editor.
        let mut one = Component::read(&write("escape.html", FUSSY)).unwrap();
        let why = one
            .set_text(1, "<b>bold</b>")
            .expect_err("markup is refused");
        assert!(why.contains("Codex"), "{why}");
        assert_eq!(one.text(), FUSSY);
    }

    #[test]
    fn two_edits_in_a_row_both_land() {
        // The spans move after the first one. A writer that kept the old ones
        // would put the second edit in the wrong place — and it would look
        // fine until the offsets happened to matter.
        let mut one = Component::read(&write("twice.html", FUSSY)).unwrap();
        one.set_text(1, "Plans").unwrap();
        one.set_text(2, "Body.").unwrap();
        assert_eq!(
            one.text(),
            FUSSY
                .replace(">Pricing<", ">Plans<")
                .replace("Body copy.", "Body.")
        );
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
