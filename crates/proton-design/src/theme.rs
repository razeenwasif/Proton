//! Reading tokens out of a stylesheet, and putting one back.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One custom property, and where its value sits in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Token {
    /// Including the leading dashes, as CSS writes it: `--accent`.
    pub name: String,
    /// Trimmed of the whitespace either side, but otherwise verbatim —
    /// `var(--x, #fff)` and `oklch(62% .2 250)` are values like any other.
    pub value: String,
    /// The blocks it was declared inside, outermost first, joined by ` > `.
    ///
    /// `:root` for the common case; for a dark palette something like
    /// `@media (prefers-color-scheme: dark) > :root`. Two tokens with the same
    /// name and different scopes are two tokens.
    pub scope: String,
    /// Which line it is on, 1-based, for a panel that wants to jump to it.
    pub line: usize,
    /// The bytes the value occupies. Not sent to the front end: it is a fact
    /// about the text this `Theme` was parsed from, and it is stale the moment
    /// anything else edits the file.
    #[serde(skip)]
    #[ts(skip)]
    span: Range<usize>,
}

/// A stylesheet, and the tokens found in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    text: String,
    tokens: Vec<Token>,
}

impl Theme {
    /// Read a stylesheet. Never fails: anything that is not a token is text
    /// this does not touch, which is most of a stylesheet.
    pub fn parse(text: &str) -> Theme {
        Theme {
            tokens: scan(text),
            text: text.to_string(),
        }
    }

    /// The file, exactly as it now stands.
    pub fn to_text(&self) -> &str {
        &self.text
    }

    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// The first token with this name, whatever scope it is in.
    pub fn get(&self, name: &str) -> Option<&Token> {
        self.tokens.iter().find(|token| token.name == name)
    }

    /// Every declaration of a name — the light one, the dark one, and any
    /// others. A palette editor needs all of them; showing one and writing to
    /// it silently leaves the other themes behind.
    pub fn every(&self, name: &str) -> Vec<&Token> {
        self.tokens
            .iter()
            .filter(|token| token.name == name)
            .collect()
    }

    /// Change one declaration's value, found by name *and* scope.
    ///
    /// Both, because a name alone is ambiguous the moment a theme has a dark
    /// palette, and picking the first match would edit whichever the parser
    /// happened to reach first — light in one file and dark in the next.
    ///
    /// Answers whether anything changed. A value equal to the one already
    /// there is not an edit: rewriting the file to the same bytes still marks
    /// a buffer dirty and still shows up as a save.
    pub fn set(&mut self, name: &str, scope: &str, value: &str) -> bool {
        let Some(token) = self
            .tokens
            .iter()
            .find(|token| token.name == name && token.scope == scope)
        else {
            return false;
        };
        if token.value == value {
            return false;
        }

        let span = token.span.clone();
        self.text.replace_range(span, value);
        // Re-scanned rather than patched: every span after the edit has moved,
        // and a stale span is an edit that lands in the middle of the next
        // token. Scanning a stylesheet takes microseconds.
        self.tokens = scan(&self.text);
        true
    }
}

/// Walk the text and collect every `--name: value` with its block context.
///
/// Hand-written rather than a CSS crate, because what is needed is the span of
/// each value in the original bytes, and a parser that hands back a syntax
/// tree has usually thrown that away. The three things it has to get right are
/// comments, strings and parentheses — each of which can hold a brace or a
/// semicolon that means nothing.
fn scan(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    // The selectors of the blocks we are inside.
    let mut stack: Vec<String> = Vec::new();
    // Where the current selector, or declaration, started.
    let mut start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            // A comment is not code, wherever it appears.
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let opened = i;
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                // A comment sitting between two declarations, or above a
                // selector, is leading trivia and belongs to neither. Left in
                // the span it would hide the declaration after it — the span
                // would start with `/*` rather than `--` — and it would end up
                // pasted onto the front of a selector as if it were part of
                // the name. Only dropped when nothing but whitespace precedes
                // it, because `--x: red /* why */` is a comment *inside* a
                // value and that one has to stay put.
                if text[start..opened].trim().is_empty() {
                    start = i;
                }
            }

            // A string can hold anything, a closing brace included.
            quote @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    // A backslash escapes whatever is next, the quote included.
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }

            b'{' => {
                // Whitespace collapsed, because a selector list is routinely
                // written one per line — `:root,\n:root[data-theme="x"]` — and
                // a scope with a newline in it is one no panel can put on a
                // row. It stays unique either way: this only rewrites the runs
                // of space between the parts, never the parts.
                stack.push(
                    text[start..i]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                i += 1;
                start = i;
            }

            b'}' => {
                // The last declaration in a block is allowed to omit its
                // semicolon, so the brace has to close it. Done before the
                // pop, or it would be recorded as belonging to the block
                // outside the one it was written in.
                if let Some(token) = declaration(text, start..i, &stack) {
                    tokens.push(token);
                }
                stack.pop();
                i += 1;
                start = i;
            }

            b';' => {
                if let Some(token) = declaration(text, start..i, &stack) {
                    tokens.push(token);
                }
                i += 1;
                start = i;
            }

            // Parentheses hide both `;` and `}` — `url(data:...;base64,...)`,
            // and any `var(--x, calc(...))`. Skipped whole.
            b'(' => {
                let mut depth = 1usize;
                i += 1;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        // A string inside the parens still shields its contents.
                        quote @ (b'"' | b'\'') => {
                            i += 1;
                            while i < bytes.len() && bytes[i] != quote {
                                i += if bytes[i] == b'\\' { 2 } else { 1 };
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }

            _ => i += 1,
        }
    }

    // A last declaration with no `;` before the closing brace is legal CSS.
    if let Some(token) = declaration(text, start..bytes.len(), &stack) {
        tokens.push(token);
    }
    tokens
}

/// Turn `--name: value` into a token, if that is what this span holds.
fn declaration(text: &str, span: Range<usize>, stack: &[String]) -> Option<Token> {
    let raw = text.get(span.clone())?;
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("--") {
        return None;
    }
    let lead = raw.len() - trimmed.len();
    let colon = trimmed.find(':')?;

    let name = trimmed[..colon].trim_end();
    // `--a --b: x` is not a declaration of anything.
    if name.len() < 3 || name.contains(char::is_whitespace) {
        return None;
    }

    let after = &trimmed[colon + 1..];
    let value = after.trim();
    // `--x: ;` is valid CSS and means the empty value. There is nothing for a
    // panel to show and nothing to edit, so it is not offered as a token.
    if value.is_empty() {
        return None;
    }

    let value_start = span.start + lead + colon + 1 + (after.len() - after.trim_start().len());
    Some(Token {
        name: name.to_string(),
        value: value.to_string(),
        scope: stack.join(" > "),
        line: text[..value_start].lines().count().max(1),
        span: value_start..value_start + value.len(),
    })
}
