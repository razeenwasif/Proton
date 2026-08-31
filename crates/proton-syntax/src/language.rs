//! Which grammar parses what, and how its captures become colours.

use tree_sitter::Language as Grammar;

/// A grammar, its highlight query, and the extensions it claims.
pub struct Language {
    pub id: &'static str,
    pub extensions: &'static [&'static str],
    grammar: fn() -> Grammar,
    /// The `highlights.scm` shipped with the grammar, joined when the query is
    /// compiled. Using the grammar's own query rather than writing one keeps
    /// Proton out of the business of tracking every language's syntax as it
    /// changes.
    ///
    /// A list rather than one string because some grammars build on another —
    /// TypeScript's own query covers only what TypeScript adds, and without
    /// JavaScript's in front of it every `const` in a `.ts` file is plain text.
    pub queries: &'static [&'static str],
}

impl Language {
    pub fn grammar(&self) -> Grammar {
        (self.grammar)()
    }
}

impl std::fmt::Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Language").field("id", &self.id).finish()
    }
}

/// TypeScript is JavaScript plus additions, and its query is written that way.
const TYPESCRIPT: &[&str] = &[
    tree_sitter_javascript::HIGHLIGHT_QUERY,
    tree_sitter_typescript::HIGHLIGHTS_QUERY,
];

pub const LANGUAGES: &[Language] = &[
    Language {
        id: "rust",
        extensions: &["rs"],
        grammar: || tree_sitter_rust::LANGUAGE.into(),
        queries: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
    },
    Language {
        id: "typescript",
        extensions: &["ts", "mts", "cts"],
        grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        queries: TYPESCRIPT,
    },
    Language {
        id: "tsx",
        extensions: &["tsx"],
        grammar: || tree_sitter_typescript::LANGUAGE_TSX.into(),
        queries: TYPESCRIPT,
    },
    Language {
        id: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        grammar: || tree_sitter_javascript::LANGUAGE.into(),
        queries: &[tree_sitter_javascript::HIGHLIGHT_QUERY],
    },
    Language {
        id: "python",
        extensions: &["py", "pyi"],
        grammar: || tree_sitter_python::LANGUAGE.into(),
        queries: &[tree_sitter_python::HIGHLIGHTS_QUERY],
    },
    Language {
        id: "json",
        extensions: &["json", "jsonc"],
        grammar: || tree_sitter_json::LANGUAGE.into(),
        queries: &[tree_sitter_json::HIGHLIGHTS_QUERY],
    },
    Language {
        id: "toml",
        extensions: &["toml"],
        grammar: || tree_sitter_toml_ng::LANGUAGE.into(),
        queries: &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
    },
];

/// Look a language up by file extension. `None` is an ordinary answer: a file
/// with no grammar is shown as plain text, not as an error.
pub fn for_path(path: &std::path::Path) -> Option<&'static Language> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|l| l.extensions.contains(&extension.as_str()))
}
