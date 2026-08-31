//! One highlighter per file, kept for as long as the file is open.
//!
//! Kept here rather than on the buffer, because a buffer is text and history
//! and nothing else — it has no business owning a parse tree. And kept at all,
//! rather than rebuilt per redraw, because the tree is the thing incremental
//! parsing is incremental *against*: throw it away and every keystroke pays
//! for a full parse again.

use std::collections::HashMap;
use std::path::PathBuf;

use proton_syntax::{Span, Syntax};
use proton_text::Buffer;

#[derive(Debug, Default)]
pub struct Highlighters {
    /// `None` means "looked, and there is no grammar for this" — remembered so
    /// a plain-text file is not re-examined on every redraw.
    by_path: HashMap<PathBuf, Option<Syntax>>,
}

impl Highlighters {
    /// Coloured runs for a window of the buffer.
    ///
    /// Empty when the file has no grammar, which the front end reads as "draw
    /// it plain" — not as an error, and not as "no colours on these lines".
    pub fn spans(&mut self, buffer: &Buffer, first_line: usize, count: usize) -> Vec<Vec<Span>> {
        let Some(path) = buffer.path() else {
            return Vec::new();
        };
        let syntax = self
            .by_path
            .entry(path.to_path_buf())
            .or_insert_with(|| Syntax::for_path(path));

        match syntax {
            Some(syntax) => syntax.spans(buffer, first_line, count),
            None => Vec::new(),
        }
    }

    /// The language a file is being highlighted as, for the statusline.
    pub fn language(&self, buffer: &Buffer) -> Option<&'static str> {
        let path = buffer.path()?;
        self.by_path.get(path)?.as_ref().map(Syntax::language)
    }
}
