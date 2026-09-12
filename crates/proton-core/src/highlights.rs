//! One highlighter per file, kept for as long as the file is open.
//!
//! Kept here rather than on the buffer, because a buffer is text and history
//! and nothing else — it has no business owning a parse tree. And kept at all,
//! rather than rebuilt per redraw, because the tree is the thing incremental
//! parsing is incremental *against*: throw it away and every keystroke pays
//! for a full parse again.

use std::collections::HashMap;
use std::path::PathBuf;

use proton_syntax::{Grammars, Span, Syntax};
use proton_text::Buffer;

#[derive(Debug, Default)]
pub struct Highlighters {
    /// `None` means "looked, and there is no grammar for this" — remembered so
    /// a plain-text file is not re-examined on every redraw.
    by_path: HashMap<PathBuf, Option<Syntax>>,
    notices: Vec<String>,
}

impl Highlighters {
    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }
    /// Versions restart at zero when a file is reopened. A parse tree from
    /// the previous buffer must not be reused just because the path matches.
    pub fn forget(&mut self, buffer: &Buffer) {
        if let Some(path) = buffer.path() {
            self.by_path.remove(path);
        }
    }

    /// Coloured runs for a window of the buffer.
    ///
    /// Empty when the file has no grammar, which the front end reads as "draw
    /// it plain" — not as an error, and not as "no colours on these lines".
    pub fn spans(
        &mut self,
        buffer: &Buffer,
        first_line: usize,
        count: usize,
        grammars: &Grammars,
    ) -> Vec<Vec<Span>> {
        let Some(path) = buffer.path() else {
            return Vec::new();
        };
        let syntax = self.by_path.entry(path.to_path_buf()).or_insert_with(|| {
            // The built-in table is tried first. A plugin that claimed `.rs`
            // would otherwise decide how Rust is coloured by being installed,
            // and a file whose colours depend on what you happen to have
            // installed is a file you cannot reason about.
            Syntax::for_path(path).or_else(|| {
                let extension = path.extension()?.to_str()?.to_ascii_lowercase();
                let added = grammars.for_extension(&extension)?;
                Syntax::from_grammar(added, grammars)
            })
        });

        match syntax {
            Some(syntax) => {
                let was_limited = syntax.is_limited();
                syntax.limit_parse_time(
                    (buffer.text().len_bytes() >= 1_000_000)
                        .then_some(std::time::Duration::from_millis(25)),
                );
                let spans = syntax.spans(buffer, first_line, count);
                if !was_limited && syntax.is_limited() {
                    self.notices.push(format!("Syntax highlighting paused for {} to keep this large file responsive. Reopen the file to retry.", buffer.name()));
                }
                spans
            }
            None => Vec::new(),
        }
    }

    /// The language a file is being highlighted as, for the statusline.
    pub fn language(&self, buffer: &Buffer) -> Option<&str> {
        let path = buffer.path()?;
        self.by_path.get(path)?.as_ref().map(Syntax::language)
    }
}
