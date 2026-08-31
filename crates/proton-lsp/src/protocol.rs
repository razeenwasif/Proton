//! The slice of LSP that Proton actually speaks.
//!
//! Hand-written rather than pulled from a crate, for two reasons. The response
//! shapes we care about are three, and each has an `untagged` subtlety worth
//! stating explicitly instead of inheriting. And the ambient alternative keeps
//! changing how it spells a URI, which is a lot of churn to import for
//! `{ line, character }`.

use serde::{Deserialize, Serialize};

/// LSP counts columns in **UTF-16 code units**, not bytes and not characters —
/// which is why `proton-text` carries a UTF-16 index at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/* ── diagnostics ──────────────────────────────────────────────────── */

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub range: Range,
    /// 1 error, 2 warning, 3 information, 4 hint. Absent means "the server
    /// declined to say", which the spec leaves to the client — we treat it as
    /// information rather than as an error, so an unlabelled note cannot make
    /// a clean file look broken.
    #[serde(default)]
    pub severity: Option<u8>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub code: Option<Code>,
    pub message: String,
}

/// `E0308` and `2` are both legal codes.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Code {
    Number(i64),
    Text(String),
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Code::Number(n) => write!(f, "{n}"),
            Code::Text(s) => f.write_str(s),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PublishDiagnostics {
    pub uri: String,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

/* ── completion ───────────────────────────────────────────────────── */

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    #[serde(default)]
    pub kind: Option<u8>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub insert_text: Option<String>,
    #[serde(default)]
    pub sort_text: Option<String>,
    #[serde(default)]
    pub text_edit: Option<Edit>,
    #[serde(default)]
    pub label_details: Option<LabelDetails>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelDetails {
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// A server may send a plain edit or an insert/replace pair; both carry the
/// same replacement text and differ only in how much they cover.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Edit {
    Plain {
        range: Range,
        #[serde(rename = "newText")]
        new_text: String,
    },
    InsertReplace {
        insert: Range,
        replace: Range,
        #[serde(rename = "newText")]
        new_text: String,
    },
}

impl CompletionItem {
    /// What to put in the buffer, and over what.
    ///
    /// A `textEdit` is authoritative — it is how a server says "you typed
    /// `HashM`, replace all of it, not just append". Falling back to appending
    /// `insertText` at the cursor is what produces `HashMHashMap`.
    pub fn edit(&self) -> (Option<Range>, String) {
        match &self.text_edit {
            // The insert variant covers what was typed; the replace variant
            // also eats the identifier to the right of the cursor. Insert is
            // the less destructive default.
            Some(Edit::Plain { range, new_text }) => (Some(*range), new_text.clone()),
            Some(Edit::InsertReplace {
                insert, new_text, ..
            }) => (Some(*insert), new_text.clone()),
            None => (
                None,
                self.insert_text
                    .clone()
                    .unwrap_or_else(|| self.label.clone()),
            ),
        }
    }

    /// The dim text beside the label: `-> Vec<T>`, `std::collections`.
    pub fn hint(&self) -> Option<String> {
        if let Some(details) = &self.label_details {
            let joined: String = [details.detail.as_deref(), details.description.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            if !joined.is_empty() {
                return Some(joined);
            }
        }
        self.detail.clone()
    }
}

/// `textDocument/completion` answers with a bare array or with a list that
/// also says whether it was truncated. Untagged handles both; the flag is
/// dropped because we re-request on every keystroke anyway.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CompletionResponse {
    Items(Vec<CompletionItem>),
    List {
        #[serde(default)]
        items: Vec<CompletionItem>,
    },
}

impl CompletionResponse {
    pub fn into_items(self) -> Vec<CompletionItem> {
        match self {
            CompletionResponse::Items(items) | CompletionResponse::List { items } => items,
        }
    }
}

/* ── go to definition ─────────────────────────────────────────────── */

#[derive(Debug, Clone, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationLink {
    pub target_uri: String,
    /// Where to actually put the cursor — the name, not the whole item. A
    /// server that sends a link is telling you it knows the difference.
    pub target_selection_range: Range,
}

/// Three legal shapes for one answer. Order matters: a `LocationLink` has no
/// `uri` field, so it fails to parse as a `Location` and falls through — but
/// only if `Location` is tried first.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DefinitionResponse {
    One(Location),
    Many(Vec<Location>),
    Links(Vec<LocationLink>),
}

impl DefinitionResponse {
    /// The first target, which is the one to jump to. A list happens for a
    /// trait method with several impls; picking one beats refusing to move.
    pub fn first(self) -> Option<(String, Position)> {
        match self {
            DefinitionResponse::One(l) => Some((l.uri, l.range.start)),
            DefinitionResponse::Many(mut ls) => ls.drain(..).next().map(|l| (l.uri, l.range.start)),
            DefinitionResponse::Links(mut ls) => ls
                .drain(..)
                .next()
                .map(|l| (l.target_uri, l.target_selection_range.start)),
        }
    }
}
