//! Which part of the library a question is allowed to see.
//!
//! ## Why scoping is worth having
//!
//! A real library spans subjects. This one holds high-performance computing,
//! machine learning, convex optimisation, software engineering and a physics
//! course, and BM25 does not know they are different subjects — it knows which
//! words are rare. Asking about "convergence" reaches optimisation, training
//! and numerical methods at once, and the top result is whichever passage
//! happens to say the word most distinctively rather than the one from the
//! field you meant.
//!
//! Narrowing fixes that with no model involved and no tokens spent: the same
//! query, over the part of the corpus the question is actually about.
//!
//! ## Scoring first, filtering second
//!
//! A scoped search scores the *whole* library and then keeps what is in scope,
//! rather than scoring only the subset. That is deliberate. BM25 weighs a term
//! by how rare it is, and rarity is a property of the corpus: `gradient` is
//! unremarkable across a machine-learning folder and distinctive in a physics
//! one. Re-deriving those weights per subset would make a passage's score
//! depend on what else you happened to select, so the same passage would rank
//! differently for the same question. Scoring once keeps the comparison
//! honest.

/// The documents a search may draw on. Empty means all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// Path prefixes, as they appear at the front of a citation.
    prefixes: Vec<String>,
}

impl Scope {
    /// Everything. The default, and what an empty selection means — a panel
    /// with nothing ticked is "no restriction", not "no sources", because the
    /// alternative is an editor that answers nothing until you configure it.
    pub fn everything() -> Scope {
        Scope::default()
    }

    pub fn of(prefixes: impl IntoIterator<Item = impl Into<String>>) -> Scope {
        let prefixes: Vec<String> = prefixes
            .into_iter()
            .map(Into::into)
            // Whitespace before slashes: a blank entry is all spaces, and
            // trimming only slashes leaves it as a prefix that matches
            // nothing, which reads on screen as a source nobody can find.
            .map(|p| p.replace('\\', "/").trim().trim_matches('/').to_string())
            .filter(|p| !p.is_empty())
            .collect();
        Scope { prefixes }
    }

    pub fn prefixes(&self) -> &[String] {
        &self.prefixes
    }

    pub fn is_everything(&self) -> bool {
        self.prefixes.is_empty()
    }

    /// Whether a document, named the way a citation names it, is in scope.
    pub fn admits(&self, path: &str) -> bool {
        if self.prefixes.is_empty() {
            return true;
        }
        let path = path.replace('\\', "/");
        self.prefixes.iter().any(|prefix| under(&path, prefix))
    }
}

/// Whether `path` is `prefix` or sits inside it.
///
/// On a segment boundary, so selecting `00 - ML` does not silently also select
/// `00 - MLOps`. A prefix match without this is the kind of bug that shows up
/// as an answer citing a subject nobody ticked.
fn under(path: &str, prefix: &str) -> bool {
    if !path.starts_with(prefix) {
        return false;
    }
    match path.len() == prefix.len() {
        true => true,
        false => path.as_bytes().get(prefix.len()) == Some(&b'/'),
    }
}

/// One thing that can be ticked: a folder of the library, and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The prefix that selects it, as a citation spells it.
    pub path: String,
    /// What to show. The last segment for a folder, since the panel groups
    /// them under their root already.
    pub label: String,
    pub documents: usize,
    pub passages: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_selected_means_everything() {
        // A panel with nothing ticked is "no restriction". The alternative is
        // an editor that answers nothing until you have configured it.
        let all = Scope::everything();
        assert!(all.is_everything());
        assert!(all.admits("00 - HPC/Roofline.md"));
        assert!(Scope::of(Vec::<String>::new()).admits("anything.md"));
    }

    #[test]
    fn a_folder_admits_what_is_inside_it() {
        let scope = Scope::of(["02 - Data Science"]);
        assert!(scope.admits("02 - Data Science/Data Mining/Bayes.md"));
        assert!(scope.admits("02 - Data Science"));
        assert!(!scope.admits("03 - Software Engineering/Testing.md"));
    }

    #[test]
    fn a_prefix_stops_at_a_segment_boundary() {
        // Selecting `00 - ML` must not silently also select `00 - MLOps`.
        // Without this the panel says one subject and the answer cites another.
        let scope = Scope::of(["00 - ML"]);
        assert!(scope.admits("00 - ML/Attention.md"));
        assert!(!scope.admits("00 - MLOps/Serving.md"));
    }

    #[test]
    fn several_folders_are_a_union() {
        let scope = Scope::of(["00 - HPC", "06 - Mathematics"]);
        assert!(scope.admits("00 - HPC/Roofline.md"));
        assert!(scope.admits("06 - Mathematics/Linear Algebra.md"));
        assert!(!scope.admits("07 - Business/Pricing.md"));
    }

    #[test]
    fn a_single_document_can_be_the_scope() {
        // "Answer from this paper only" is the whole point, and it is the same
        // rule — a document is a prefix that happens to name a file.
        let scope = Scope::of(["xResourcesx/10 - Books/AttentionIsAllYouNeed.pdf"]);
        assert!(scope.admits("xResourcesx/10 - Books/AttentionIsAllYouNeed.pdf"));
        assert!(!scope.admits("xResourcesx/10 - Books/Boyd.pdf"));
    }

    #[test]
    fn a_path_written_with_backslashes_still_matches() {
        // The panel sends what a citation shows, but a hand-edited settings
        // file on Windows will have backslashes in it.
        let scope = Scope::of(["02 - Data Science\\Data Mining"]);
        assert!(scope.admits("02 - Data Science/Data Mining/Bayes.md"));
    }

    #[test]
    fn stray_slashes_and_blanks_are_tidied_rather_than_breaking_it() {
        let scope = Scope::of(["/00 - HPC/", "", "   "]);
        assert_eq!(scope.prefixes(), ["00 - HPC"]);
        assert!(scope.admits("00 - HPC/Roofline.md"));
    }
}
