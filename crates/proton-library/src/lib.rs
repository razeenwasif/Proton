//! A library of your own documents, searched by passage and answered with
//! citations.
//!
//! This is the piece a grounded answer needs. `proton-index` finds *files* by
//! name and greps for a literal string; neither can answer "what do my notes
//! say about attention sparsity", because the words in the question are not
//! the words in the note. What is missing is passages, scored, with a citation
//! back to where each came from.
//!
//! ## Lexical first, not embeddings
//!
//! BM25 rather than a vector store, deliberately, and not as a placeholder:
//!
//! - It needs no API, no key and no network, so a search costs nothing and
//!   works on a train.
//! - It is deterministic, so a test can assert which passage came first rather
//!   than that something plausible came back.
//! - Technical notes are full of exact terms — `FlashAttention`, `mmap`,
//!   `O(n log n)` — and exact terms are the case lexical retrieval is best at
//!   and embeddings are worst at.
//!
//! What it cannot do is match "how do I make attention cheaper" to a note that
//! only ever says "sparsity". That is worth adding embeddings for, later, on
//! top of this rather than instead of it — the passages and the citations are
//! the same either way, which is why they come first.
//!
//! ## Nothing outside a named root is ever read
//!
//! A library that could reach anywhere under the home directory is one that
//! can put a medical record in a prompt by accident. Roots are listed
//! explicitly and the walker refuses anything outside them.

pub mod chunk;
pub mod pdf;
pub mod scope;
pub mod score;
pub mod shelf;

use std::path::{Path, PathBuf};

pub use chunk::Passage;
pub use scope::{Scope, Source};
pub use shelf::Shelf;

/// One result: a passage, and why it was chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub passage: Passage,
    pub score: f32,
    /// The citation, ready to draw.
    pub cite: String,
}

/// Everything indexed, and the roots it came from.
#[derive(Debug, Default, Clone)]
pub struct Library {
    roots: Vec<PathBuf>,
    passages: Vec<Passage>,
    /// Written when a document is read, because the root it came from is known
    /// then and would have to be looked up again on the way out.
    cites: Vec<String>,
    index: score::Bm25,
    /// Every document read, by the path a citation names it with. Kept once
    /// rather than per passage: there are twenty-eight thousand passages and
    /// thirteen hundred documents, and both the count and the scope want the
    /// document rather than the passage.
    docs: Vec<String>,
    /// Which document each passage came from, as an index into `docs`.
    doc_of: Vec<usize>,
}

impl Library {
    pub fn new() -> Library {
        Library::default()
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn len(&self) -> usize {
        self.passages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.passages.is_empty()
    }

    /// How many documents were read, as distinct from passages.
    ///
    /// Counted as they arrive rather than worked out on demand. It used to
    /// sort and dedup every passage's path, which was fine at six thousand
    /// passages and is not at twenty-eight thousand — the panel asks for this
    /// on every tick of a walk that runs for minutes.
    pub fn documents(&self) -> usize {
        self.docs.len()
    }

    /// The folders a search can be narrowed to, with how much is in each.
    ///
    /// Grouped at the first segment, which on a real vault is the subject —
    /// twenty-three folders in one root and nine in the other, which is a list
    /// somebody can read. Deeper would be a file tree, and a file tree of
    /// thirteen hundred documents is not something anybody ticks through. A
    /// document sitting loose at the top of a root is its own source, having
    /// no folder to belong to.
    pub fn sources(&self) -> Vec<Source> {
        let mut documents: std::collections::HashMap<&str, usize> = Default::default();
        let mut passages: std::collections::HashMap<&str, usize> = Default::default();
        for doc in &self.docs {
            *documents.entry(Self::head(doc)).or_default() += 1;
        }
        for doc in &self.doc_of {
            *passages.entry(Self::head(&self.docs[*doc])).or_default() += 1;
        }

        let mut out: Vec<Source> = documents
            .into_iter()
            .map(|(path, documents)| Source {
                label: path.rsplit('/').next().unwrap_or(path).to_string(),
                documents,
                passages: passages.get(path).copied().unwrap_or_default(),
                path: path.to_string(),
            })
            .collect();
        // By name, so the panel does not reshuffle between walks — and the
        // vault's own numbering (`00 - `, `01 - `) then does the ordering it
        // was written to do.
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// Note a root, so citations can be written relative to it.
    pub fn add_root(&mut self, root: &Path) {
        if !self.roots.iter().any(|r| r == root) {
            self.roots.push(root.to_path_buf());
        }
    }

    /// Read one text document into the library.
    pub fn add(&mut self, path: &Path, text: &str) {
        self.take(path, chunk::markdown(path, text));
    }

    /// Read one PDF, already extracted to a string per page.
    ///
    /// Separate from [`add`](Self::add) because getting the text out of a PDF
    /// costs the better part of a second and can panic, so it happens
    /// elsewhere and behind a cache — and because a PDF is cut into pages
    /// rather than into sections.
    pub fn add_pages(&mut self, path: &Path, pages: &[String]) {
        self.take(path, chunk::pages(path, pages));
    }

    fn take(&mut self, path: &Path, passages: Vec<Passage>) {
        let root = self.root_for(path).to_path_buf();
        // A document that produced no passages — a note with nothing readable
        // in it — is not one that was read.
        if passages.is_empty() {
            return;
        }
        let doc = self.docs.len();
        self.docs.push(
            path.strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/"),
        );

        for passage in passages {
            self.index.add(&passage.text, &passage.title);
            let cite = passage.cite(&root);
            self.passages.push(passage);
            // Kept beside the passage rather than computed on the way out: the
            // root a document came from is known now and would have to be
            // looked up again later.
            self.cites.push(cite);
            self.doc_of.push(doc);
        }
    }

    /// How many passages a scope admits — what a search actually looks at.
    ///
    /// The picker's header says "4 of N", and N being the whole library while
    /// the search saw a tenth of it is a header that lies about the thing it
    /// is describing.
    pub fn passages_in(&self, scope: &Scope) -> usize {
        if scope.is_everything() {
            return self.passages.len();
        }
        self.doc_of
            .iter()
            .filter(|doc| scope.admits(&self.docs[**doc]))
            .count()
    }

    /// The best passages for a query, most relevant first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        self.search_in(query, limit, &Scope::everything())
    }

    /// The best passages a scope admits.
    ///
    /// The whole library is scored and the scope applied after, never before.
    /// BM25 weighs a term by how rare it is, and rarity is a property of the
    /// corpus — re-deriving it per subset would make a passage's score depend
    /// on what else happened to be selected, so the same question would rank
    /// the same passage differently on different days. See [`crate::scope`].
    pub fn search_in(&self, query: &str, limit: usize, scope: &Scope) -> Vec<Hit> {
        let mut scored: Vec<(usize, f32)> = self
            .index
            .scores(query)
            .into_iter()
            .enumerate()
            .filter(|(_, s)| *s > 0.0)
            .filter(|(i, _)| {
                scope.is_everything()
                    || self
                        .doc_of
                        .get(*i)
                        .is_some_and(|doc| scope.admits(&self.docs[*doc]))
            })
            .collect();

        // Ties broken by position so the same query gives the same answer
        // twice — a search that reshuffles is one you cannot cite from.
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scored.truncate(limit);

        scored
            .into_iter()
            .map(|(i, score)| Hit {
                passage: self.passages[i].clone(),
                score,
                cite: self.cites[i].clone(),
            })
            .collect()
    }

    /// The first segment of a path — the subject folder, or the file itself
    /// when it sits loose at the top of a root.
    fn head(path: &str) -> &str {
        path.split_once('/').map(|(head, _)| head).unwrap_or(path)
    }

    /// Which root a path came from, for writing its citation.
    fn root_for(&self, path: &Path) -> &Path {
        self.roots
            .iter()
            .filter(|root| path.starts_with(root))
            // The longest match, or a nested root would cite against the
            // shallower one and produce a needlessly long path.
            .max_by_key(|root| root.as_os_str().len())
            .map(PathBuf::as_path)
            .unwrap_or(Path::new(""))
    }
}
