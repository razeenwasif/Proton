//! Which folders of a knowledge base are left out, and why.
//!
//! ## Written as `.gitignore` is, with the root's name in front
//!
//! A knowledge base is several roots — a vault, a courses folder, an archive —
//! and a rule has to say which it means. So a rule starts with the root's
//! label: `OnyxVault/07 - Business/05 - Finance/` leaves out that one folder of
//! that one root, and nothing of the same name anywhere else. Past the label it
//! is a `.gitignore` line, matched by the `ignore` crate's own matcher — `*`,
//! `**`, a trailing `/` for folders only, and `!` to let something back in. A
//! rule that names no root applies to every root, as `.gitignore` would.
//!
//! ## Every leaving-out has a reason
//!
//! The picker shows what was left out and why, so a missing folder is never a
//! mystery: a default rule (`node_modules/`), one of the person's own, a
//! dataset, or a folder whose listing is still in iCloud.
//!
//! Each root's own `.gitignore` files are not read. A course's repository
//! ignoring its handouts would hide them with no reason anybody could see, and
//! the defaults already cover what a `.gitignore` usually says.

use std::collections::HashMap;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;

/// Left out of every root: other people's code and the machinery around one's
/// own. A rule of the person's can let any of them back in with `!`.
pub const DEFAULTS: [&str; 11] = [
    "node_modules/",
    ".venv/",
    "venv/",
    "env/",
    "site-packages/",
    "__pycache__/",
    ".ipynb_checkpoints/",
    "build/",
    "dist/",
    "target/",
    "*.egg-info/",
];

/// A folder of at least this many files, none of them readable, is data — a
/// corpus an assignment ran over — rather than something to study from. The
/// one in the courses folder this was written against is 33,848 files with
/// nothing in them a person reads.
pub const DATASET: usize = 2_000;

/// Why a folder or file was left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// A rule every root has, such as `node_modules/`.
    Default(String),
    /// One of the person's own, as they wrote it.
    Yours(String),
    /// Thousands of files and nothing in them to read.
    Dataset { files: usize },
    /// Its listing is in iCloud: listing it would fetch it.
    InCloud,
    /// It could not be listed.
    Unlistable(String),
}

impl Why {
    /// One line for the picker.
    pub fn say(&self) -> String {
        match self {
            Why::Default(rule) => format!("left out by default ({rule})"),
            Why::Yours(rule) => format!("left out by your rule {rule}"),
            Why::Dataset { files } => format!("a dataset: {files} files, none to read"),
            Why::InCloud => "in iCloud, not listed yet".into(),
            Why::Unlistable(why) => format!("could not be listed: {why}"),
        }
    }
}

/// The person's rules, the defaults, and how big a dataset is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    written: Vec<String>,
    defaults: bool,
    dataset: usize,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules { written: Vec::new(), defaults: true, dataset: DATASET }
    }
}

impl Rules {
    /// The defaults, and these rules after them.
    pub fn new(written: impl IntoIterator<Item = impl Into<String>>) -> Rules {
        let written = written.into_iter().map(Into::into).map(|r: String| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
        Rules { written, ..Rules::default() }
    }

    /// Without the defaults: for a test that wants to see one rule alone.
    pub fn without_defaults(mut self) -> Rules {
        self.defaults = false;
        self
    }

    /// A smaller dataset, for a test that cannot write two thousand files.
    pub fn with_dataset(mut self, files: usize) -> Rules {
        self.dataset = files.max(1);
        self
    }

    pub fn written(&self) -> &[String] {
        &self.written
    }

    pub fn dataset(&self) -> usize {
        self.dataset
    }

    /// The rules that apply under the root with this label, compiled.
    pub fn under(&self, label: &str) -> RootRules {
        // The matcher's root is never touched: every path given to it is
        // relative to the root already.
        let mut builder = GitignoreBuilder::new("");
        let mut yours = HashMap::new();
        let mut whole = None;
        if self.defaults {
            for rule in DEFAULTS {
                let _ = builder.add_line(None, rule);
            }
        }
        for written in &self.written {
            let Some(compiled) = compile(written, label) else { continue };
            match compiled {
                Compiled::Whole => whole = Some(written.clone()),
                Compiled::Line(line) => {
                    if builder.add_line(None, &line).is_ok() {
                        yours.insert(line, written.clone());
                    }
                }
            }
        }
        let matcher = builder.build().unwrap_or_else(|_| Gitignore::empty());
        RootRules { matcher, yours, whole }
    }
}

/// A written rule, as one root's matcher takes it.
enum Compiled {
    /// The rule names the root itself.
    Whole,
    Line(String),
}

/// `Label/rest` becomes `/rest` under the root with that label, and nothing
/// under any other; a rule with no root's label applies under every root as
/// written.
fn compile(written: &str, label: &str) -> Option<Compiled> {
    let (bang, rule) = match written.strip_prefix('!') {
        Some(rest) => ("!", rest),
        None => ("", written),
    };
    let rule = rule.trim_start_matches('/');
    let (head, rest) = rule.split_once('/').unwrap_or((rule, ""));
    if head == label || head.eq_ignore_ascii_case(label) {
        let rest = rest.trim_start_matches('/');
        if rest.is_empty() {
            // Letting a whole root back in means nothing; leaving one out is
            // the same as not naming it.
            return (bang.is_empty()).then_some(Compiled::Whole);
        }
        return Some(Compiled::Line(format!("{bang}/{rest}")));
    }
    // The first part names some other root, or no root at all. Another root's
    // rule has nothing to say here; one naming no root applies everywhere. A
    // rule anchored with `/` names a root by its first part, so if that is
    // nobody's label it is left alone rather than guessed at.
    if written.trim_start_matches('!').starts_with('/') {
        return None;
    }
    Some(Compiled::Line(written.to_string()))
}

/// One root's rules, compiled.
#[derive(Debug, Clone)]
pub struct RootRules {
    matcher: Gitignore,
    /// A compiled line → the rule as the person wrote it.
    yours: HashMap<String, String>,
    /// A rule leaving out the whole root, if there is one.
    whole: Option<String>,
}

impl RootRules {
    /// Whether the whole root is left out, and by which rule.
    pub fn whole(&self) -> Option<Why> {
        self.whole.clone().map(Why::Yours)
    }

    /// Why something a walk met is left out. A walk asks top-down, so every
    /// folder above `rel` was already let in.
    pub fn why(&self, rel: &str, is_dir: bool) -> Option<Why> {
        self.said(self.matcher.matched(rel, is_dir))
    }

    /// Why a path is left out, by itself or by any folder above it: for a path
    /// that did not come from a walk, such as one a model asked to read.
    pub fn why_anywhere(&self, rel: &str, is_dir: bool) -> Option<Why> {
        if let Some(why) = self.whole() {
            return Some(why);
        }
        let rel = rel.trim_start_matches('/');
        if rel.is_empty() {
            return None;
        }
        self.said(self.matcher.matched_path_or_any_parents(rel, is_dir))
    }

    fn said(&self, matched: Match<&ignore::gitignore::Glob>) -> Option<Why> {
        let Match::Ignore(glob) = matched else { return None };
        let line = glob.original();
        Some(match self.yours.get(line) {
            Some(written) => Why::Yours(written.clone()),
            None => Why::Default(line.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_folders_are_left_out_of_every_root() {
        let rules = Rules::default().under("Courses");
        assert_eq!(rules.why("COMP2300/site/node_modules", true), Some(Why::Default("node_modules/".into())));
        assert_eq!(rules.why("ml/a1/venv", true), Some(Why::Default("venv/".into())));
        assert_eq!(rules.why("ml/pkg.egg-info", true), Some(Why::Default("*.egg-info/".into())));
        assert_eq!(rules.why("ml/a1/notes.md", false), None);
        // A file called `build` is not the folder the rule means.
        assert_eq!(rules.why("ml/build", false), None);
    }

    #[test]
    fn a_rule_names_its_root_and_leaves_the_others_alone() {
        let rules = Rules::new(["OnyxVault/07 - Business/05 - Finance/", "OnyxVault/Entertainment/"]);
        let vault = rules.under("OnyxVault");
        assert_eq!(
            vault.why("07 - Business/05 - Finance", true),
            Some(Why::Yours("OnyxVault/07 - Business/05 - Finance/".into()))
        );
        assert_eq!(vault.why("07 - Business/01 - Strategy", true), None);
        // Anchored to the root: a folder of the same name deeper down is not it.
        assert_eq!(vault.why("Entertainment", true), Some(Why::Yours("OnyxVault/Entertainment/".into())));
        assert_eq!(vault.why("Notes/Entertainment", true), None);
        // Another root is untouched by them.
        let courses = rules.under("Courses");
        assert_eq!(courses.why("Entertainment", true), None);
        assert_eq!(courses.why("07 - Business/05 - Finance", true), None);
    }

    #[test]
    fn a_rule_naming_no_root_applies_under_every_root() {
        let rules = Rules::new(["*.log", "Private/"]).without_defaults();
        for label in ["OnyxVault", "Courses"] {
            let under = rules.under(label);
            assert_eq!(under.why("a/b/run.log", false), Some(Why::Yours("*.log".into())));
            assert_eq!(under.why("x/Private", true), Some(Why::Yours("Private/".into())));
        }
    }

    #[test]
    fn a_bang_lets_something_back_in() {
        let rules = Rules::new(["!Courses/COMP1100/build/"]);
        let courses = rules.under("Courses");
        assert_eq!(courses.why("COMP1100/build", true), None, "let back in");
        assert!(courses.why("COMP2300/build", true).is_some(), "only that one");
    }

    #[test]
    fn a_rule_naming_only_a_root_leaves_out_all_of_it() {
        let rules = Rules::new(["ANU/"]);
        assert_eq!(rules.under("ANU").whole(), Some(Why::Yours("ANU/".into())));
        assert_eq!(rules.under("ANU").why_anywhere("Year 1/notes.md", false), Some(Why::Yours("ANU/".into())));
        assert_eq!(rules.under("Courses").whole(), None);
    }

    #[test]
    fn a_path_from_outside_a_walk_is_checked_against_its_folders_too() {
        // What a model asks to read never came from a walk, so the folders
        // above it were never asked about.
        let rules = Rules::new(["OnyxVault/07 - Business/05 - Finance/"]);
        let vault = rules.under("OnyxVault");
        assert!(vault.why_anywhere("07 - Business/05 - Finance/Tax 2025.md", false).is_some());
        assert!(vault.why_anywhere("/07 - Business/05 - Finance/deep/er.md", false).is_some());
        assert!(vault.why_anywhere("00 - ML/Attention.md", false).is_none());
        assert!(Rules::default().under("Courses").why_anywhere("a/node_modules/x/readme.md", false).is_some());
    }

    #[test]
    fn a_rule_anchored_to_a_root_nobody_has_is_not_guessed_at() {
        let rules = Rules::new(["/Elsewhere/secret/"]).without_defaults();
        assert_eq!(rules.under("Courses").why("Elsewhere/secret", true), None);
    }

    #[test]
    fn every_reason_reads_as_a_sentence() {
        assert!(Why::Dataset { files: 33_848 }.say().contains("33848 files"));
        assert!(Why::InCloud.say().contains("iCloud"));
        assert!(Why::Yours("OnyxVault/Entertainment/".into()).say().contains("OnyxVault/Entertainment/"));
    }
}
