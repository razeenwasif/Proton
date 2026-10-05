//! The literature, as Research asks for it: through a [`Source`] — the
//! network in the app; in tests nothing, or a script — and the papers it
//! finds written into an investigation's `literature/`, with their open PDFs
//! beside them.
//!
//! The keys some sources give more to are found the first time a worker
//! needs them — in Proton's own environment, then the login shell's, which
//! an app opened from the Dock does not inherit — and kept in memory only.
//! What is ever said of one, anywhere, is whether it was found.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proton_literature::fetch::{Context, Download, Fetch, Job, Reply};
use proton_literature::keys::{Keys, NAMES};
use proton_literature::{pdf, Http};
use proton_research::model::{Access, AddedBy, LiteratureSource, Paper, Record};
use proton_research::store::Folder;
use proton_research::view::KeyState;

/// The line in an investigation's `.gitignore` that keeps its PDFs out of Git.
pub const PDFS_IGNORED: &str = "literature/*.pdf";

/// What asks the literature.
pub trait Source: Send {
    /// Start `job`; what it finds arrives in the [`Fetch`].
    fn fetch(&mut self, job: Job) -> Fetch;
    /// Whether each source's key was found, as far as is known yet.
    fn key(&self, source: LiteratureSource) -> KeyState;
    /// Whether it asks anybody at all: a source that does not says so
    /// rather than fail every search.
    fn live(&self) -> bool {
        true
    }
}

/// The keys as found, and when: looked for again after a while if one was
/// missing, so a key put in the profile since is found without a restart.
type Found = Option<(Keys, Instant)>;

/// The network.
pub struct Live {
    context: Context,
    found: Arc<Mutex<Found>>,
}

/// How long a missing key is believed missing before it is looked for again.
const LOOK_AGAIN: Duration = Duration::from_secs(60);

impl Default for Live {
    fn default() -> Live {
        Live::new()
    }
}

impl Live {
    pub fn new() -> Live {
        let found: Arc<Mutex<Found>> = Arc::default();
        // One worker looks at a time; the core only ever reads what was found.
        let looking = Arc::new(Mutex::new(()));
        let cache = found.clone();
        let context = Context::new(Http::live(None), move || {
            let _one = looking.lock().unwrap_or_else(|e| e.into_inner());
            let known = cache.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some((keys, at)) = known {
                let whole = keys.semantic_scholar.is_some() && keys.openalex.is_some();
                if whole || at.elapsed() < LOOK_AGAIN {
                    return keys;
                }
            }
            let keys = find_keys();
            *cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((keys.clone(), Instant::now()));
            keys
        });
        Live { context, found }
    }
}

/// The keys in Proton's own environment, and those it lacks from the login
/// shell's.
fn find_keys() -> Keys {
    let mut vars: BTreeMap<String, String> =
        NAMES.iter().filter_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()).map(|v| (name.to_string(), v))).collect();
    let missing: Vec<&str> = NAMES.iter().copied().filter(|name| !vars.contains_key(*name)).collect();
    if !missing.is_empty() {
        vars.extend(proton_claude::process::login_vars(&missing));
    }
    Keys::from_vars(&vars)
}

impl Source for Live {
    fn fetch(&mut self, job: Job) -> Fetch {
        Fetch::start(&self.context, job)
    }

    fn key(&self, source: LiteratureSource) -> KeyState {
        if source == LiteratureSource::Arxiv {
            return KeyState::NotNeeded;
        }
        match &*self.found.lock().unwrap_or_else(|e| e.into_inner()) {
            Some((keys, _)) => keys.state(source),
            None => KeyState::NotLooked,
        }
    }
}

/// Nobody: what Research asks when it is not the app's — in tests, which
/// must never reach the network.
pub struct Offline;

/// What an offline source answers.
pub const OFFLINE: &str = "the literature is not asked here";

impl Source for Offline {
    fn fetch(&mut self, _: Job) -> Fetch {
        Fetch::scripted(vec![Reply::Failed { why: OFFLINE.into() }])
    }

    fn key(&self, source: LiteratureSource) -> KeyState {
        if source == LiteratureSource::Arxiv {
            KeyState::NotNeeded
        } else {
            KeyState::NotLooked
        }
    }

    fn live(&self) -> bool {
        false
    }
}

/// A script, for tests: each job answered as `answer` says, and kept in
/// `asked` to be looked at.
pub struct Scripted {
    pub asked: Arc<Mutex<Vec<Job>>>,
    answer: Box<dyn FnMut(&Job) -> Fetch + Send>,
    keys: Keys,
}

impl Scripted {
    /// Each job answered with these replies, all at once.
    pub fn new(mut answer: impl FnMut(&Job) -> Vec<Reply> + Send + 'static) -> Scripted {
        Scripted::with(move |job| Fetch::scripted(answer(job)))
    }

    /// Each job answered by the fetch `answer` makes: one that never answers,
    /// say.
    pub fn with(answer: impl FnMut(&Job) -> Fetch + Send + 'static) -> Scripted {
        Scripted { asked: Arc::default(), answer: Box::new(answer), keys: Keys::default() }
    }

    /// With these keys found.
    pub fn keyed(mut self, keys: Keys) -> Scripted {
        self.keys = keys;
        self
    }
}

impl Source for Scripted {
    fn fetch(&mut self, job: Job) -> Fetch {
        self.asked.lock().unwrap_or_else(|e| e.into_inner()).push(job.clone());
        (self.answer)(&job)
    }

    fn key(&self, source: LiteratureSource) -> KeyState {
        self.keys.state(source)
    }
}

/// A paper of a record, as it is first written: with why it has no PDF when
/// no open copy of it is known anywhere.
pub fn paper(id: String, record: Record, by: AddedBy, round: Option<u32>, queries: Vec<String>, now: u64) -> Paper {
    noted(Paper { id, record, added: now, round, by, queries, ..Paper::default() })
}

/// A paper saying why it has no PDF, when no open copy of it is known.
pub fn noted(mut paper: Paper) -> Paper {
    if paper.pdf_note.is_none() && pdf::links(&paper.record).is_empty() {
        paper.pdf_note = Some(
            match paper.record.access {
                Access::Closed => "It is behind a paywall, as far as the sources know.",
                _ => "No open copy of it is known.",
            }
            .to_string(),
        );
    }
    paper
}

/// Start keeping the open PDFs of `papers` in `folder`, no bigger than its
/// investigation allows: `None` when none of them has one to be had. An
/// investigation made before there were PDFs is told to keep them out of Git
/// first.
pub fn start_downloads(source: &mut dyn Source, folder: &Folder, papers: &[Paper]) -> Option<(Fetch, Vec<String>)> {
    let items: Vec<Download> = papers
        .iter()
        .filter(|p| !p.has_pdf)
        .filter_map(|p| {
            let urls = pdf::links(&p.record);
            let dest = folder.paper_pdf(&p.id).ok()?;
            (!urls.is_empty() && !dest.exists()).then(|| Download { key: p.id.clone(), urls, dest })
        })
        .collect();
    if items.is_empty() {
        return None;
    }
    if folder.ensure_ignored(PDFS_IGNORED).is_err() {
        // Never a PDF where Git would take it in.
        return None;
    }
    let ids = items.iter().map(|i| i.key.clone()).collect();
    let cap = u64::from(folder.investigation().literature.pdf_mb.max(1)) * 1024 * 1024;
    Some((source.fetch(Job::Download { items, cap }), ids))
}

/// What a download came to, written into its paper — read again first, so
/// what the scientist wrote in it since is kept. Whether it was a paper's.
pub fn settle_download(folder: &Folder, reply: &Reply) -> bool {
    let (Reply::Saved { key: id, .. } | Reply::Unsaved { key: id, .. }) = reply else { return false };
    let Some(mut paper) = folder.papers().into_iter().find(|p| &p.id == id) else { return false };
    match reply {
        Reply::Saved { bytes, sha256, url, .. } => {
            paper.pdf_from = Some(url.clone());
            paper.pdf_sha256 = Some(sha256.clone());
            paper.pdf_bytes = Some(*bytes);
            paper.pdf_note = None;
        }
        Reply::Unsaved { why, .. } => paper.pdf_note = Some(format!("Its PDF could not be kept: {why}.")),
        _ => {}
    }
    folder.write_paper(&paper).is_ok()
}

/// When a source that said it was out of calls may be asked again: OpenAlex
/// without a key counts its hundred a day to midnight, UTC; anyone else is
/// left ten minutes.
pub fn rest_until(source: LiteratureSource, key: KeyState, now: u64) -> u64 {
    match (source, key) {
        (LiteratureSource::OpenAlex, KeyState::Missing | KeyState::NotLooked) => (now / 86_400 + 1) * 86_400,
        _ => now + 600,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paper_says_why_it_has_no_pdf_and_a_tired_source_is_left_alone() {
        let closed = paper("P001".into(), Record { title: "Paywalled".into(), access: Access::Closed, ..Record::default() }, AddedBy::Scientist, None, Vec::new(), 7);
        assert!(closed.pdf_note.as_deref().is_some_and(|n| n.contains("paywall")));
        let open = paper("P002".into(), Record { arxiv: Some("2101.00001".into()), ..Record::default() }, AddedBy::Round, Some(1), vec!["sky".into()], 7);
        assert_eq!((open.pdf_note, open.round, open.added), (None, Some(1), 7), "arXiv has every paper's PDF");

        let day = 86_400;
        assert_eq!(rest_until(LiteratureSource::OpenAlex, KeyState::Missing, 3 * day + 5), 4 * day, "to midnight");
        assert_eq!(rest_until(LiteratureSource::OpenAlex, KeyState::Found, 100), 700);
        assert_eq!(rest_until(LiteratureSource::SemanticScholar, KeyState::Missing, 100), 700);
        assert_eq!(Offline.key(LiteratureSource::SemanticScholar), KeyState::NotLooked);
        assert!(matches!(Offline.fetch(Job::Lookup { idents: Vec::new(), sources: Vec::new() }).poll().as_slice(), [Reply::Failed { why }] if why == OFFLINE));
    }
}
