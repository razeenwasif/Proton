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
    /// A second grammar for the parts the first one hands over.
    ///
    /// Markdown is two grammars by design: one for the block structure —
    /// headings, fences, lists — and one for what is inside a paragraph:
    /// emphasis, links, code spans. The block grammar marks those stretches
    /// as `inline` nodes and stops, and without the second pass a markdown
    /// file gets its headings coloured and nothing else.
    pub inline: Option<Companion>,
}

/// A grammar that parses ranges the main one delegated, and its query.
pub struct Companion {
    /// Its own name, for the compiled-query cache — which is keyed by
    /// `&'static str` and must not be handed a string built at run time.
    pub id: &'static str,
    /// The node type whose ranges it is given.
    pub inside: &'static str,
    pub grammar: fn() -> Grammar,
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

/// C++ stands in the same relation to C, and for the same reason: its own
/// query is seventy lines of what C++ adds — templates, namespaces, `auto` —
/// and says almost nothing about comments, strings or numbers. Alone it would
/// colour a `.cpp` file's templates and leave its string literals grey. The
/// C++ *grammar* is a superset of C's, so C's patterns match here too.
const CPP: &[&str] = &[
    tree_sitter_c::HIGHLIGHT_QUERY,
    tree_sitter_cpp::HIGHLIGHT_QUERY,
];

/// LaTeX's colours: the grammar ships none, so Proton's own.
const LATEX: &str = include_str!("../queries/latex.scm");

pub const LANGUAGES: &[Language] = &[
    Language {
        id: "latex",
        extensions: &["tex", "sty", "cls", "ltx", "dtx"],
        grammar: || codebook_tree_sitter_latex::LANGUAGE.into(),
        queries: &[LATEX],
        inline: None,
    },
    Language {
        id: "rust",
        extensions: &["rs"],
        grammar: || tree_sitter_rust::LANGUAGE.into(),
        queries: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
        inline: None,
    },
    Language {
        id: "c",
        // `.h` is claimed here rather than by C++ because it has to go
        // somewhere and a header is not readable from its extension. C's
        // query against a C++ header still colours the comments, strings and
        // types; C++'s against a C header would be the worse trade.
        extensions: &["c", "h"],
        grammar: || tree_sitter_c::LANGUAGE.into(),
        queries: &[tree_sitter_c::HIGHLIGHT_QUERY],
        inline: None,
    },
    Language {
        id: "cpp",
        extensions: &[
            "cpp", "cc", "cxx", "c++", "hpp", "hh", "hxx", "h++", "ipp", "tcc",
        ],
        grammar: || tree_sitter_cpp::LANGUAGE.into(),
        queries: CPP,
        inline: None,
    },
    Language {
        id: "asm",
        // `.s` and `.S` both arrive lower-cased, which is the point of
        // lower-casing: the difference between them is whether the
        // preprocessor runs, not what the file is.
        extensions: &["s", "asm"],
        grammar: || tree_sitter_asm::LANGUAGE.into(),
        queries: &[tree_sitter_asm::HIGHLIGHTS_QUERY],
        inline: None,
    },
    Language {
        id: "typescript",
        extensions: &["ts", "mts", "cts"],
        grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        queries: TYPESCRIPT,
        inline: None,
    },
    Language {
        id: "tsx",
        extensions: &["tsx"],
        grammar: || tree_sitter_typescript::LANGUAGE_TSX.into(),
        queries: TYPESCRIPT,
        inline: None,
    },
    Language {
        id: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        grammar: || tree_sitter_javascript::LANGUAGE.into(),
        queries: &[tree_sitter_javascript::HIGHLIGHT_QUERY],
        inline: None,
    },
    Language {
        id: "python",
        extensions: &["py", "pyi"],
        grammar: || tree_sitter_python::LANGUAGE.into(),
        queries: &[tree_sitter_python::HIGHLIGHTS_QUERY],
        inline: None,
    },
    Language {
        id: "json",
        extensions: &["json", "jsonc"],
        grammar: || tree_sitter_json::LANGUAGE.into(),
        queries: &[tree_sitter_json::HIGHLIGHTS_QUERY],
        inline: None,
    },
    Language {
        id: "toml",
        extensions: &["toml"],
        grammar: || tree_sitter_toml_ng::LANGUAGE.into(),
        queries: &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
        inline: None,
    },
    Language {
        id: "markdown",
        extensions: &["md", "markdown", "mdx"],
        grammar: || tree_sitter_md::LANGUAGE.into(),
        queries: &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
        // The block grammar stops at the edge of a paragraph and marks what
        // is inside as `inline`. Everything a reader looks for in prose —
        // emphasis, links, code spans — is in there.
        inline: Some(Companion {
            id: "markdown.inline",
            inside: "inline",
            grammar: || tree_sitter_md::INLINE_LANGUAGE.into(),
            queries: &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
        }),
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
