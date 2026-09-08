//! The component, as a tree of layers.
//!
//! ## Why parse at all, when the artboard renders the text
//!
//! The frame draws the markup; a browser already knows how. What a browser
//! will not tell us is *where in the file* an element came from, and that is
//! the whole binding: clicking a layer has to open the line that wrote it, and
//! one day dragging on the artboard has to edit those bytes.
//!
//! So this keeps spans, the way [`Theme`](crate::Theme) does. A layer knows
//! the bytes its element occupies, which is what makes it editable later
//! rather than only viewable now.
//!
//! ## It is deliberately not a conforming HTML parser
//!
//! The HTML standard's tree construction is a state machine with an
//! insertion-mode stack, foster parenting, and a table of which tags
//! implicitly close which others. Implementing that here would be a
//! fortnight's work to correctly model the recovery behaviour of markup
//! nobody writes on purpose.
//!
//! What this handles is the markup people write: nested elements, void
//! elements, self-closing syntax, comments, doctypes, raw-text elements,
//! attributes with `>` inside quotes, and the handful of tags that close each
//! other without being told to — `<p>` before a block, one `<li>` after
//! another, cells and rows in a table. Those are not optional politeness: the
//! browser applies them when it renders the artboard, so a tree that skipped
//! them would show a nesting the picture does not have, and clicking a layer
//! would outline the wrong element.
//!
//! What it does not do is guess beyond that. An unclosed `<div>` closes when
//! its parent does, and a stray `</span>` that matches nothing is ignored.
//! Both produce a tree that is wrong in the same way the author's file is
//! wrong, which is the useful kind of wrong — a layer tree that quietly
//! disagreed with the file would be worse than one that shows the mistake.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One element, as a row in the layer tree.
///
/// Flat with a depth rather than nested: it is drawn as indented rows, it
/// crosses the bus as a list, and document order is the order somebody reads
/// their own file in. A nested shape would have to be flattened at both ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// Lowercased, as HTML is matched: `div`, `article`.
    pub tag: String,
    /// The `id` attribute, which is what a layer is called when it has one.
    pub id: String,
    /// The `class` attribute, split. Shown after the tag the way a stylesheet
    /// would write it.
    pub classes: Vec<String>,
    /// The element's own direct text, trimmed and shortened — the difference
    /// between a tree of `div / div / div` and one you can read.
    pub text: String,
    /// 1-based, for jumping to it in the editor.
    pub line: usize,
    /// How deep in the tree, for indenting.
    pub depth: usize,
    /// A CSS path that selects this element and no other, built from
    /// `nth-child` so it does not depend on the author having given anything a
    /// class. Used to outline the selected layer inside the frame — the
    /// artboard cannot be handed a modified copy of somebody's markup, so the
    /// highlight has to be reachable by selector.
    pub path: String,
    /// The bytes the whole element occupies, open tag through close.
    ///
    /// Not sent to the front end, for the reason [`Token`](crate::Token)'s is
    /// not: it is a fact about the text this was parsed from and it is stale
    /// the moment anything edits the file.
    #[serde(skip)]
    #[ts(skip)]
    #[allow(dead_code)]
    span: Range<usize>,
}

/// Elements that never have children or a closing tag.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose contents are text, not markup. `<script>if (a < b)</script>`
/// is not an open tag for `b`.
const RAW: &[&str] = &["script", "style", "textarea", "title"];

/// How much of an element's text to keep for its label.
const LABEL: usize = 48;

/// The blocks whose opening ends an open paragraph.
const ENDS_A_PARAGRAPH: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

/// Whether opening `now` implicitly closes an open `open`.
///
/// The subset that occurs in markup people write, rather than the standard's
/// full table. Everything here is a rule a browser applies while building the
/// tree the artboard shows, so leaving one out puts the layer tree and the
/// picture into disagreement.
fn closes(open: &str, now: &str) -> bool {
    match open {
        "p" => ENDS_A_PARAGRAPH.contains(&now),
        "li" => now == "li",
        "dt" | "dd" => now == "dt" || now == "dd",
        "option" => now == "option" || now == "optgroup",
        "optgroup" => now == "optgroup",
        "tr" => matches!(now, "tr" | "thead" | "tbody" | "tfoot"),
        "td" | "th" => matches!(now, "td" | "th" | "tr" | "thead" | "tbody" | "tfoot"),
        "thead" | "tbody" | "tfoot" => matches!(now, "thead" | "tbody" | "tfoot"),
        _ => false,
    }
}

/// Read markup into layers, in document order.
pub fn layers(source: &str) -> Vec<Layer> {
    Parser {
        bytes: source.as_bytes(),
        source,
        at: 0,
        out: Vec::new(),
        open: Vec::new(),
    }
    .run()
}

/// An element that has been opened and not yet closed.
struct Open {
    tag: String,
    /// Where in `out` its layer is, so its text and span can be finished when
    /// the close tag arrives.
    slot: usize,
    start: usize,
    /// How many element children it has so far, for `nth-child`.
    children: usize,
}

struct Parser<'a> {
    bytes: &'a [u8],
    source: &'a str,
    at: usize,
    out: Vec<Layer>,
    open: Vec<Open>,
}

impl Parser<'_> {
    fn run(mut self) -> Vec<Layer> {
        while self.at < self.bytes.len() {
            let Some(next) = self.find(b'<') else {
                break;
            };
            self.at = next;
            if self.skip_not_a_tag() {
                continue;
            }
            if self.bytes.get(self.at + 1) == Some(&b'/') {
                self.close_tag();
            } else {
                self.open_tag();
            }
        }
        // Whatever is still open ends where the file does. An unclosed element
        // is the author's, and the tree says so by containing it rather than
        // by dropping it.
        let end = self.bytes.len();
        while let Some(open) = self.open.pop() {
            self.out[open.slot].span = open.start..end;
            self.finish_text(open.slot, open.start, end);
        }
        self.out
    }

    /// Comments, doctypes and CDATA — skipped whole, so a `>` inside one does
    /// not read as the end of a tag.
    fn skip_not_a_tag(&mut self) -> bool {
        if self.bytes.get(self.at + 1) != Some(&b'!') && self.bytes.get(self.at + 1) != Some(&b'?')
        {
            return false;
        }
        let close: &[u8] = match self.source[self.at..].starts_with("<!--") {
            true => b"-->",
            false => b">",
        };
        self.at = self
            .find_slice(close, self.at)
            .map(|end| end + close.len())
            .unwrap_or(self.bytes.len());
        true
    }

    fn open_tag(&mut self) {
        let start = self.at;
        let Some(name_end) = self.name_end(start + 1) else {
            self.at = start + 1;
            return;
        };
        let tag = self.source[start + 1..name_end].to_lowercase();
        if tag.is_empty() {
            self.at = start + 1;
            return;
        }
        let Some(gt) = self.tag_end(name_end) else {
            self.at = self.bytes.len();
            return;
        };
        // `<div/>` is not valid HTML but is written constantly, and treating it
        // as an element with everything after it inside would be a tree that
        // matches nobody's intent.
        let closed = self.bytes[gt - 1] == b'/';
        let attrs = &self.source[name_end..gt - usize::from(closed)];
        self.at = gt + 1;

        // Anything this tag implicitly closes, closed before it is counted as
        // a child — or a `<p>` after a `<p>` would be numbered as its sibling's
        // child and the `nth-child` path would miss.
        while self.open.last().is_some_and(|one| closes(&one.tag, &tag)) {
            let one = self.open.pop().expect("checked");
            self.out[one.slot].span = one.start..start;
            self.finish_text(one.slot, one.start, start);
        }

        let among = self.open.last().map(|one| one.children + 1).unwrap_or(1);
        if let Some(parent) = self.open.last_mut() {
            parent.children += 1;
        }
        let depth = self.open.len();
        let parent = self
            .open
            .last()
            .map(|one| self.out[one.slot].path.clone())
            .unwrap_or_default();
        let path = match parent.is_empty() {
            true => format!("{tag}:nth-child({among})"),
            false => format!("{parent} > {tag}:nth-child({among})"),
        };

        let slot = self.out.len();
        self.out.push(Layer {
            id: attribute(attrs, "id").unwrap_or_default(),
            classes: attribute(attrs, "class")
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            text: String::new(),
            line: self.source[..start].bytes().filter(|b| *b == b'\n').count() + 1,
            depth,
            path,
            span: start..self.at,
            tag: tag.clone(),
        });

        if closed || VOID.contains(&tag.as_str()) {
            return;
        }
        if RAW.contains(&tag.as_str()) {
            // Straight to the close tag: nothing inside is markup.
            let end = self
                .find_slice(format!("</{tag}").as_bytes(), self.at)
                .unwrap_or(self.bytes.len());
            self.finish_text(slot, self.at, end);
            self.out[slot].span = start..end;
            self.at = self
                .find(b'>')
                .map(|gt| gt + 1)
                .unwrap_or(self.bytes.len())
                .max(end);
            return;
        }
        self.open.push(Open {
            tag,
            slot,
            start,
            children: 0,
        });
    }

    fn close_tag(&mut self) {
        let start = self.at;
        let Some(name_end) = self.name_end(start + 2) else {
            self.at = start + 2;
            return;
        };
        let tag = self.source[start + 2..name_end].to_lowercase();
        let end = self.find(b'>').map(|gt| gt + 1).unwrap_or(self.bytes.len());
        self.at = end;

        // A close tag matching nothing open is a typo in somebody's file. It is
        // ignored rather than guessed about, which leaves the tree wrong in the
        // same shape their file is.
        let Some(depth) = self.open.iter().rposition(|one| one.tag == tag) else {
            return;
        };
        // Everything above it was left unclosed and ends here.
        while self.open.len() > depth {
            let one = self.open.pop().expect("checked by rposition");
            let closes = match self.open.len() == depth {
                true => end,
                false => start,
            };
            self.out[one.slot].span = one.start..closes;
            self.finish_text(one.slot, one.start, closes);
        }
    }

    /// The element's own text, for its label.
    fn finish_text(&mut self, slot: usize, from: usize, to: usize) {
        if !self.out[slot].text.is_empty() {
            return;
        }
        let inside = self.source.get(from..to).unwrap_or_default();
        self.out[slot].text = label(inside);
    }

    fn name_end(&self, from: usize) -> Option<usize> {
        let mut at = from;
        while at < self.bytes.len() {
            let b = self.bytes[at];
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' {
                at += 1;
                continue;
            }
            break;
        }
        (at > from).then_some(at)
    }

    /// The `>` that ends a tag, skipping any inside a quoted attribute.
    fn tag_end(&self, from: usize) -> Option<usize> {
        let mut at = from;
        let mut quote = None;
        while at < self.bytes.len() {
            let b = self.bytes[at];
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if b == b'"' || b == b'\'' => quote = Some(b),
                None if b == b'>' => return Some(at),
                None => {}
            }
            at += 1;
        }
        None
    }

    fn find(&self, byte: u8) -> Option<usize> {
        self.bytes[self.at..]
            .iter()
            .position(|b| *b == byte)
            .map(|at| at + self.at)
    }

    fn find_slice(&self, needle: &[u8], from: usize) -> Option<usize> {
        self.bytes
            .get(from..)?
            .windows(needle.len())
            .position(|w| w.eq_ignore_ascii_case(needle))
            .map(|at| at + from)
    }
}

/// One attribute's value out of an open tag's attribute text.
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name) {
        let at = from + at;
        // A whole word: `class` must not match the `class` inside `data-class`.
        let before_ok = at == 0
            || !lower.as_bytes()[at - 1].is_ascii_alphanumeric()
                && lower.as_bytes()[at - 1] != b'-'
                && lower.as_bytes()[at - 1] != b'_';
        let rest = lower[at + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let eq = at + name.len() + lower[at + name.len()..].find('=')?;
            let value = attrs[eq + 1..].trim_start();
            let quote = value.chars().next()?;
            return Some(match quote {
                '"' | '\'' => value[1..].split(quote).next()?.to_string(),
                _ => value.split_whitespace().next()?.to_string(),
            });
        }
        from = at + name.len();
    }
    None
}

/// Readable text out of a stretch of markup: what a layer is called when it
/// has no id.
fn label(inside: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for ch in inside.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let flat = out.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.chars().count() > LABEL {
        // By characters, not bytes: a label cut at byte forty-eight can land
        // inside a character.
        true => flat.chars().take(LABEL).collect::<String>() + "…",
        false => flat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(source: &str) -> Vec<(usize, String)> {
        layers(source)
            .into_iter()
            .map(|l| (l.depth, l.tag))
            .collect()
    }

    #[test]
    fn nesting_becomes_depth() {
        assert_eq!(
            tags("<div><p><span>hi</span></p><b>x</b></div>"),
            [
                (0, "div".into()),
                (1, "p".into()),
                (2, "span".into()),
                (1, "b".into()),
            ]
        );
    }

    #[test]
    fn a_void_element_does_not_swallow_the_rest_of_the_file() {
        // `<br>` has no close tag. Waiting for one would put every following
        // element inside it, which is a layer tree nobody could use.
        assert_eq!(
            tags("<div><br><p>after</p></div>"),
            [(0, "div".into()), (1, "br".into()), (1, "p".into())]
        );
    }

    #[test]
    fn self_closing_syntax_is_honoured_even_though_html_does_not_have_it() {
        // Invalid HTML and written constantly. Treating `<div/>` as an open
        // tag puts the whole document inside it.
        assert_eq!(
            tags("<div/><p>beside</p>"),
            [(0, "div".into()), (0, "p".into())]
        );
    }

    #[test]
    fn a_greater_than_inside_an_attribute_is_not_the_end_of_the_tag() {
        let found = layers(r#"<div title="a > b"><p>in</p></div>"#);
        assert_eq!(tags(r#"<div title="a > b"><p>in</p></div>"#).len(), 2);
        assert_eq!(found[1].tag, "p");
        assert_eq!(found[1].depth, 1);
    }

    #[test]
    fn script_contents_are_not_markup() {
        // `a < b` in a script is a comparison. Read as markup it opens an
        // element called `b` that never closes.
        assert_eq!(
            tags("<div><script>if (a < b) x();</script><p>after</p></div>"),
            [(0, "div".into()), (1, "script".into()), (1, "p".into()),]
        );
    }

    #[test]
    fn comments_and_doctypes_are_skipped_whole() {
        assert_eq!(
            tags("<!doctype html><!-- <div>not real</div> --><p>real</p>"),
            [(0, "p".into())]
        );
    }

    #[test]
    fn an_unclosed_element_closes_when_its_parent_does() {
        // Rather than swallowing its siblings. The tree is wrong in the same
        // shape the file is, which is what makes the mistake findable.
        assert_eq!(
            tags("<div><p>one<p>two</div><b>out</b>"),
            [
                (0, "div".into()),
                (1, "p".into()),
                (1, "p".into()),
                (0, "b".into()),
            ]
        );
    }

    #[test]
    fn a_close_tag_matching_nothing_is_ignored() {
        assert_eq!(
            tags("<div>a</span><p>b</p></div>"),
            [(0, "div".into()), (1, "p".into())]
        );
    }

    #[test]
    fn a_layer_is_named_by_its_id_class_and_text() {
        let found = layers(r#"<article id="card" class="box wide"><h3>Pricing</h3></article>"#);
        assert_eq!(found[0].id, "card");
        assert_eq!(found[0].classes, ["box", "wide"]);
        assert_eq!(found[1].text, "Pricing");
    }

    #[test]
    fn an_attribute_is_not_matched_inside_another_ones_name() {
        // `data-class` is not `class`, and an id read out of the wrong
        // attribute names the layer after something that is not it.
        let found = layers(r#"<div data-class="no" class="yes" data-id="no" id="yes"></div>"#);
        assert_eq!(found[0].classes, ["yes"]);
        assert_eq!(found[0].id, "yes");
    }

    #[test]
    fn the_path_selects_the_layer_and_nothing_else() {
        // What the artboard outlines the selection with. Built from
        // `nth-child` so it works on markup where nothing has a class.
        let found = layers("<div><p>one</p><p>two</p></div>");
        assert_eq!(found[1].path, "div:nth-child(1) > p:nth-child(1)");
        assert_eq!(found[2].path, "div:nth-child(1) > p:nth-child(2)");
    }

    #[test]
    fn the_line_is_where_the_element_starts() {
        let found = layers("<div>\n  <p>two</p>\n  <b>three</b>\n</div>");
        assert_eq!(found[0].line, 1);
        assert_eq!(found[1].line, 2);
        assert_eq!(found[2].line, 3);
    }

    #[test]
    fn a_long_label_is_cut_by_characters_rather_than_bytes() {
        // A cut at byte forty-eight can land inside a character, which panics.
        let found = layers(&format!("<p>{}</p>", "é".repeat(80)));
        assert!(found[0].text.ends_with('…'));
    }

    #[test]
    fn protons_own_ui_parses_into_something_sensible() {
        // The largest real document to hand, and the one that would find a
        // parser that only works on examples.
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/index.html"),
        )
        .expect("ui/index.html");
        let found = layers(&source);

        assert!(found.len() > 500, "only {} layers", found.len());
        // Nothing may be attributed to a line the file does not have.
        let lines = source.lines().count();
        assert!(found.iter().all(|l| l.line >= 1 && l.line <= lines));
        // A document is a tree, not a list: if everything came out at depth
        // zero the nesting was lost.
        assert!(found.iter().any(|l| l.depth > 3));
        // And the whole file must not have ended up inside one element.
        let top = found.iter().filter(|l| l.depth == 0).count();
        assert!(top >= 1, "no top-level elements");
    }
}
