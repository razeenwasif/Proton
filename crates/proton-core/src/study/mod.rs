//! Notebook: notebooks of the person's own sources, to study from.
//!
//! One for the session, whichever project is open. Notebooks are kept in a
//! folder of the person's own — `study.folder` in their settings, offered as
//! `~/Github/Notebooks` — and their sources are drawn from a knowledge base of
//! folders they name, `study.roots`, less what `study.exclude` leaves out
//! ([`kb`]). What a notebook is on disk is `proton-study`'s.
//!
//! Reading a source can take seconds — a book of a thousand pages — and
//! bringing one down from iCloud longer, so both happen on a worker: the
//! source is listed as being read meanwhile, and the answer is taken on the
//! heartbeat. A file in iCloud is never brought down unless the person asked
//! for exactly that file, having been told how big it is.

pub mod jobs;
mod kb;
mod learning;
pub mod media;
mod quizzes;
mod review;
pub mod tutor;
mod visual;
pub mod voice;
mod web;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;

use proton_library::extract::{Body, Pdfs, Unread};
use proton_library::{dataless, Kind};
use proton_study::lock::{self, Holder, Lock};
use proton_study::model::{Note, Origin, Role, Source, SourceStatus, Units};
use proton_study::view::{StudyCue, StudyNoteRow, StudyPlace, StudySourceRow, StudySummary, StudyView};
use proton_study::{role, snapshot, Book, Folder};
use serde_json::Value;

use crate::bus::{Command, Event, NoticeLevel};
use crate::folder;
use crate::settings::Settings;
use crate::workspace::notice;

pub use kb::Kb;
use tutor::{Polled, Tutor, Tutors};

/// How often the open notebook's lock is said to be held, in seconds.
const BEAT: u64 = 30;

/// Whether a command is Notebook's.
pub fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::StudyFolder { .. }
            | Command::StudyRoots { .. }
            | Command::StudyExclude { .. }
            | Command::StudyKbPause { .. }
            | Command::StudyKbRefresh
            | Command::StudyKbBrowse { .. }
            | Command::StudyKbSearch { .. }
            | Command::StudyDiscover { .. }
            | Command::StudyNew { .. }
            | Command::StudyOpen { .. }
            | Command::StudyClose
            | Command::StudyDetails { .. }
            | Command::StudyArchive { .. }
            | Command::StudyAdd { .. }
            | Command::StudyDownload { .. }
            | Command::StudyPaste { .. }
            | Command::StudyAddUrls { .. }
            | Command::StudyRemove { .. }
            | Command::StudySelect { .. }
            | Command::StudyRefresh { .. }
            | Command::StudyRole { .. }
            | Command::StudyShow { .. }
            | Command::StudyShowNote { .. }
            | Command::StudyNote { .. }
            | Command::StudyNoteDelete { .. }
            | Command::StudyNoteSource { .. }
            | Command::StudyAsk { .. }
            | Command::StudyStopAnswer
            | Command::StudyChatNew
            | Command::StudyChatOpen { .. }
            | Command::StudyTutor { .. }
            | Command::StudySaveAnswer { .. }
            | Command::StudyMake { .. }
            | Command::StudyCancelJob { .. }
            | Command::StudyShowMade { .. }
            | Command::StudyBinMade { .. }
            | Command::StudyShowTask { .. }
            | Command::StudyReview { .. }
            | Command::StudyRate { .. }
            | Command::StudyReviewUndo
            | Command::StudyReviewEnd
            | Command::StudyCard { .. }
            | Command::StudyCardDelete { .. }
            | Command::StudyDeckExport { .. }
            | Command::StudyQuizStart { .. }
            | Command::StudyQuizAnswer { .. }
            | Command::StudyQuizDispute { .. }
            | Command::StudyQuizMarkAgain { .. }
            | Command::StudyQuizFinish { .. }
            | Command::StudyExamSit { .. }
            | Command::StudyExamExport { .. }
            | Command::StudyProgress
            | Command::StudyLesson { .. }
            | Command::StudyVoiceSetup { .. }
            | Command::StudyVoiceFind { .. }
            | Command::StudyVoiceSample { .. }
            | Command::StudyVoiceKey
            | Command::StudyAudioVoice { .. }
            | Command::StudySlidesExport { .. }
            | Command::StudySaveImage { .. }
    )
}

/// The open notebook, and whether this Proton may write it.
struct Open {
    book: Book,
    /// `None` while another Proton holds it: then it is read only.
    lock: Option<Lock>,
    holder: Option<Holder>,
    beat: u64,
}

/// What a worker says of one source.
enum Answer {
    /// It is coming down from iCloud.
    Downloading,
    Read(Box<snapshot::Read>),
    /// A page of the web, fetched and read.
    Fetched(Box<web::Fetched>),
    /// Its file turned out to be in iCloud.
    InCloud,
    Failed(String),
}

/// Sources being read, for one notebook, on a worker.
struct Batch {
    dir: PathBuf,
    answers: Receiver<(String, Answer)>,
    stop: Arc<AtomicBool>,
}

/// A file of the knowledge base, found: where it is, what the catalogue says
/// of it, and its root's label.
type Located = Option<(PathBuf, proton_library::catalog::Entry, String)>;

/// One source for a worker to read: brought down from iCloud first, or not.
struct Item {
    id: String,
    path: PathBuf,
    kind: Kind,
    download: bool,
}

/// Notebook, for the session.
pub struct Study {
    /// The person's settings file. `None` in tests that want none.
    person: Option<PathBuf>,
    /// Where notebooks are kept, and that folder as it was typed.
    folder: Option<(Folder, String)>,
    kb: Kb,
    open: Option<Open>,
    batches: Vec<Batch>,
    instance: String,
    /// Whether Notebook has been opened yet: nothing is walked until it is.
    entered: bool,
    /// Sources' PDFs opened in the document viewer: the shell reads only
    /// these, as it reads only what a workspace opened.
    documents: std::collections::HashSet<PathBuf>,
    /// The open notebook's chat, and the conversation answering it.
    tutor: Option<Tutor>,
    /// What makes tutors: the engines, or a test's script.
    tutors: Box<dyn Tutors>,
    /// The studio's calls.
    jobs: jobs::Jobs,
    /// The open notebook's session of flashcards, while there is one.
    review: Option<proton_study::deck::Session>,
    /// The quiz last looked at: drawn again as its answers are marked.
    quiz: Option<String>,
    /// An exam being sat against the clock: its quiz, its attempt, and when
    /// its time is up.
    sitting: Option<(String, u32, i64)>,
    /// What the time is, in seconds since the epoch: a test's, or the clock's.
    clock: fn() -> i64,
    /// What speaks audio overviews and joins their pieces.
    voice: Box<dyn voice::VoiceKit>,
    /// Overviews being spoken.
    voicing: Vec<voice::Voicing>,
    /// Errands to Fish under way: voices found, samples fetched.
    errands: Vec<voice::Errand>,
    /// The voices last found, for their samples.
    found_voices: Vec<proton_voice::Voice>,
    /// The audio overview last looked at: drawn again as it is spoken.
    audio: Option<String>,
    /// What fetches pages of the web: a test's script, or, once one is
    /// asked for, the network.
    web: Option<proton_literature::http::Http>,
}

impl std::fmt::Debug for Study {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Study")
            .field("folder", &self.folder.as_ref().map(|(_, typed)| typed))
            .field("open", &self.open.as_ref().map(|o| o.book.slug()))
            .field("reading", &self.batches.len())
            .finish()
    }
}

impl Default for Study {
    fn default() -> Study {
        Study {
            person: None,
            folder: None,
            kb: Kb::default(),
            open: None,
            batches: Vec::new(),
            documents: Default::default(),
            tutor: None,
            tutors: Box::new(tutor::Live),
            jobs: jobs::Jobs::default(),
            review: None,
            quiz: None,
            sitting: None,
            clock: || proton_study::now() as i64,
            voice: Box::new(voice::LiveVoices::default()),
            voicing: Vec::new(),
            errands: Vec::new(),
            found_voices: Vec::new(),
            audio: None,
            web: None,
            instance: {
                // Unique to this Study, so two in one process — a test's —
                // are two holders of a lock, as two Protons are.
                static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                format!("{}-{}-{}", std::process::id(), proton_study::now(), MADE.fetch_add(1, Ordering::Relaxed))
            },
            entered: false,
        }
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// What a source's citations will count, before it is read.
fn units_of(kind: Kind) -> Units {
    match kind {
        Kind::Pdf | Kind::Slides => Units::Pages,
        Kind::Captions => Units::Seconds,
        _ => Units::Lines,
    }
}

/// Read a file through once, so that iCloud brings it down.
fn bring_down(path: &Path) -> Result<(), String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    std::io::copy(&mut file, &mut std::io::sink()).map_err(|e| e.to_string())?;
    Ok(())
}

/// A worker: each source read — brought down first if asked — and its words
/// kept in the notebook; each answer sent as it is had.
fn read_all(items: Vec<Item>, snapshots: PathBuf, pdfs: Pdfs, stop: Arc<AtomicBool>, answers: Sender<(String, Answer)>) {
    // Nothing is brought down unless an item asks for it.
    let _no_downloads = dataless::NoDownloads::on_this_thread();
    for item in items {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if item.download {
            let _ = answers.send((item.id.clone(), Answer::Downloading));
            let fetched = {
                let _download = dataless::Downloads::on_this_thread();
                bring_down(&item.path)
            };
            if let Err(why) = fetched {
                let _ = answers.send((item.id, Answer::Failed(format!("it could not be downloaded: {why}"))));
                continue;
            }
        }
        let answer = match snapshot::read(&item.path, item.kind, &pdfs) {
            Ok(read) => match snapshot::keep_in(&snapshots, &item.id, &read.body) {
                Ok(()) => Answer::Read(Box::new(read)),
                Err(why) => Answer::Failed(why),
            },
            Err(Unread::InCloud) => Answer::InCloud,
            Err(why) => Answer::Failed(why.say()),
        };
        if answers.send((item.id, answer)).is_err() {
            return;
        }
    }
}

impl Study {
    pub fn new() -> Study {
        Study::default()
    }

    /// Read where notebooks are kept, and the knowledge base, from the
    /// person's own settings file — which is also where changes are written.
    pub fn with_person_settings(mut self, file: &Path) -> Study {
        self.person = Some(file.to_path_buf());
        self.configure();
        self
    }

    /// Make tutors with this rather than the engines: for a test.
    /// Speak and join by `kit`: for a test, which must not ask Fish.
    pub fn with_voice(mut self, kit: Box<dyn voice::VoiceKit>) -> Study {
        self.voice = kit;
        self
    }

    /// Fetch pages of the web through `http`: for a test, a script.
    pub fn with_web(mut self, http: proton_literature::http::Http) -> Study {
        self.web = Some(http);
        self
    }

    /// Tell the time by `clock`: for a test of a sitting against it.
    pub fn with_clock(mut self, clock: fn() -> i64) -> Study {
        self.clock = clock;
        self
    }

    pub fn with_tutors(mut self, tutors: Box<dyn Tutors>) -> Study {
        self.tutors = tutors;
        self
    }

    /// Say which files are in iCloud with this, for a test.
    pub fn flags_with(mut self, flags: proton_library::catalog::FlagFn) -> Study {
        self.kb.flags_with(flags);
        self
    }

    fn settings(&self) -> Option<Settings> {
        self.person.as_ref().map(|file| Settings::at(file.clone()))
    }

    /// Take the settings as they are now.
    fn configure(&mut self) {
        let Some(settings) = self.settings() else { return };
        self.folder = settings
            .get("study.folder")
            .and_then(Value::as_str)
            .and_then(|typed| crate::settings::research_path(typed).map(|path| (Folder::new(path), typed.to_string())));
        let roots = strings(settings.get("study.roots"));
        let exclude = strings(settings.get("study.exclude"));
        let pdfs = settings.get("study.pdfs").and_then(Value::as_bool).unwrap_or(true);
        let pdftotext = settings.get("study.pdftotext").and_then(Value::as_bool).unwrap_or(false);
        self.kb.configure(roots, exclude, pdfs, pdftotext);
    }

    fn save_setting(&self, key: &str, value: Value) -> Option<Event> {
        let mut settings = self.settings()?;
        settings.set(key, value).err().map(|why| notice(format!("{key} could not be saved: {why}"), NoticeLevel::Warn))
    }

    /* ── the heartbeat ──────────────────────────────────────────────── */

    /// Notebook was opened: begin on the knowledge base, open the notebook
    /// last open, and say where everything stands.
    pub fn enter(&mut self) -> Vec<Event> {
        if !self.entered {
            self.entered = true;
            self.kb.start();
            if self.open.is_none() {
                if let Some((folder, _)) = &self.folder {
                    if let Some((slug, _)) = folder.list().into_iter().find(|(_, n)| !n.archived) {
                        let _ = self.open_quietly(&slug);
                    }
                }
            }
        }
        if let Some(open) = &mut self.open {
            let _ = open.book.reread();
        }
        self.check_files();
        let mut events = vec![self.list_event(), self.kb.event()];
        events.extend(self.notebook_event());
        events.extend(self.chat_event());
        events.extend(self.studio_event());
        events
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if self.kb.poll() {
            events.push(self.kb.event());
        }
        events.extend(self.take_answers());
        events.extend(self.poll_tutor());
        let roots = self.kb.labelled_roots();
        if self.jobs.poll(self.tutors.as_mut(), &roots) {
            events.extend(self.studio_event());
            // A deck made is cards due: the list says how many.
            events.push(self.list_event());
            // An answer marked, or a mark that could not be made.
            if let Some(quiz) = self.quiz.clone() {
                events.extend(self.quiz_event(&quiz));
            }
            // A script written: spoken, if it may be.
            events.extend(self.voice_waiting());
        }
        events.extend(self.poll_voices());
        events.extend(self.poll_review());
        events.extend(self.poll_sitting());
        if let Some(open) = &mut self.open {
            let now = proton_study::now();
            if now.saturating_sub(open.beat) >= BEAT {
                open.beat = now;
                match &open.lock {
                    Some(lock) => lock.beat(now),
                    // Read only: see whether the other Proton has let go.
                    None => {
                        if let Ok(lock) = lock::acquire(open.book.dir(), &self.instance, now) {
                            open.lock = Some(lock);
                            open.holder = None;
                            events.extend(self.notebook_event());
                        }
                    }
                }
            }
        }
        events
    }

    /// Let go: stop the workers and the tutor, release the lock.
    pub fn shutdown(&mut self) {
        if let Some(tutor) = &mut self.tutor {
            tutor.stop();
        }
        self.tutor = None;
        self.jobs.stop_all();
        for voicing in &self.voicing {
            voicing.cancel();
        }
        self.voicing.clear();
        for batch in &self.batches {
            batch.stop.store(true, Ordering::Relaxed);
        }
        self.batches.clear();
        self.open = None;
    }

    /// Whether anything is being read.
    pub fn is_busy(&self) -> bool {
        !self.batches.is_empty()
    }

    /// A source's PDF, if it was opened in the document viewer from here.
    pub fn document_file(&self, path: &Path) -> Option<PathBuf> {
        self.documents.contains(path).then(|| path.to_path_buf())
    }

    /* ── commands ───────────────────────────────────────────────────── */

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::StudyFolder { folder } => self.set_folder(&folder),
            Command::StudyRoots { roots } => self.set_roots(roots),
            Command::StudyExclude { rule, on } => self.exclude(&rule, on),
            Command::StudyKbPause { paused } => {
                self.kb.pause(paused);
                vec![self.kb.event()]
            }
            Command::StudyKbRefresh => {
                self.kb.refresh();
                vec![self.kb.event()]
            }
            Command::StudyKbBrowse { root, dir } => vec![self.kb.folder(&root, &dir, &self.sources_by_path())],
            Command::StudyKbSearch { query } => vec![self.kb.search(&query, &self.sources_by_path())],
            Command::StudyDiscover { about } => self.discover(&about),
            Command::StudyNew { title } => self.create(&title),
            Command::StudyOpen { slug } => self.open(&slug),
            Command::StudyClose => {
                self.close_notebook();
                vec![self.list_event(), Event::StudyClosed]
            }
            Command::StudyDetails { title, course, exam } => self.details(&title, &course, exam.as_deref()),
            Command::StudyArchive { slug, archived } => self.archive(&slug, archived),
            Command::StudyAdd { paths } => self.add(paths, false),
            Command::StudyDownload { paths } => self.add(paths, true),
            Command::StudyPaste { title, text } => self.paste(&title, &text),
            Command::StudyAddUrls { urls } => self.add_urls(&urls),
            Command::StudyRemove { id } => self.remove(&id),
            Command::StudySelect { ids } => self.select(&ids),
            Command::StudyRefresh { id } => self.refresh(&id),
            Command::StudyRole { id, role } => self.set_role(&id, role),
            Command::StudyShow { id, at } => self.show(&id, at),
            Command::StudyShowNote { id } => self.show_note(&id),
            Command::StudyNote { id, title, body } => self.write_note(id, &title, &body),
            Command::StudyNoteDelete { id } => self.delete_note(&id),
            Command::StudyNoteSource { id } => self.note_source(&id),
            Command::StudyAsk { text } => self.ask(&text),
            Command::StudyStopAnswer => {
                if let Some(tutor) = &mut self.tutor {
                    tutor.interrupt();
                }
                Vec::new()
            }
            Command::StudyChatNew => self.new_chat(),
            Command::StudyChatOpen { id } => self.open_chat(&id),
            Command::StudyTutor { settings } => self.set_tutor(settings),
            Command::StudySaveAnswer { turn } => self.save_answer(turn),
            Command::StudyMake { kind, options } => self.make(kind, options),
            Command::StudyCancelJob { id } => {
                if !self.cancel_voicing(&id) {
                    if let Some(open) = &self.open {
                        let dir = open.book.dir().to_path_buf();
                        self.jobs.cancel(&dir, &id);
                    }
                }
                let mut events: Vec<Event> = self.studio_event().into_iter().collect();
                if self.audio.as_deref() == Some(id.as_str()) {
                    events.extend(self.audio_shown(&id));
                }
                events
            }
            Command::StudyShowMade { id } => self.show_made(&id),
            Command::StudyBinMade { id } => {
                let result = self.writable().and_then(|book| book.bin_made(&id).map_err(|why| vec![notice(why, NoticeLevel::Error)]));
                let mut events = result.err().unwrap_or_default();
                events.extend(self.studio_event());
                events
            }
            Command::StudyShowTask { id } => self.show_task(&id),
            Command::StudyReview { deck, mode, shuffle } => self.start_review(deck, mode, shuffle),
            Command::StudyRate { rating, took } => self.rate(rating, took),
            Command::StudyReviewUndo => self.undo_review(),
            Command::StudyReviewEnd => self.end_review(),
            Command::StudyCard { deck, card, kind, front, back, topic } => self.save_card(&deck, card, kind, &front, &back, &topic),
            Command::StudyCardDelete { deck, card } => self.delete_card(&deck, &card),
            Command::StudyDeckExport { deck, anki } => self.export_deck(&deck, anki),
            Command::StudyQuizStart { quiz, missed } => self.start_quiz(&quiz, missed),
            Command::StudyQuizAnswer { quiz, attempt, question, given, hinted } => self.answer_quiz(&quiz, attempt, &question, given, hinted),
            Command::StudyQuizDispute { quiz, attempt, question, why } => self.dispute(&quiz, attempt, &question, &why),
            Command::StudyQuizMarkAgain { quiz, attempt, question } => self.mark_again(&quiz, attempt, &question),
            Command::StudyQuizFinish { quiz, attempt } => self.finish_quiz(&quiz, attempt),
            Command::StudyExamSit { quiz, minutes } => self.sit(&quiz, minutes),
            Command::StudyExamExport { quiz, html, scheme } => self.export_exam(&quiz, html, scheme),
            Command::StudyProgress => self.progress(),
            Command::StudyVoiceSetup { consent, hosts } => self.voice_setup(consent, hosts),
            Command::StudyVoiceFind { query, language } => self.find_voices(&query, &language),
            Command::StudyVoiceSample { id } => self.fetch_sample(&id),
            Command::StudyVoiceKey => {
                self.voice.forget();
                let mut events = self.voice_waiting();
                let voice = self.voice_state();
                events.push(Event::StudyVoiceState { voice });
                events
            }
            Command::StudyAudioVoice { id } => self.voice_audio(&id),
            Command::StudySlidesExport { id, pptx } => self.export_slides(&id, pptx),
            Command::StudySaveImage { id, png } => self.save_image(&id, &png),
            Command::StudyLesson { objective } => self.lesson(&objective),
            _ => Vec::new(),
        }
    }

    /// Keep notebooks in the folder somebody typed. A folder that is not
    /// there is made only when its parent is, so a slip of the keyboard
    /// cannot make a tree of them.
    fn set_folder(&mut self, typed: &str) -> Vec<Event> {
        let typed = typed.trim().trim_end_matches(['/', '\\']);
        let Some(path) = crate::settings::research_path(typed) else {
            return vec![notice("Say where notebooks are to be kept.".into(), NoticeLevel::Warn)];
        };
        let mut said = Vec::new();
        if path.exists() && !path.is_dir() {
            return vec![notice(format!("{}: not a folder", folder::tilde(&path)), NoticeLevel::Error)];
        }
        if !path.exists() {
            match path.parent().filter(|parent| parent.is_dir()) {
                Some(_) => match std::fs::create_dir(&path) {
                    Ok(()) => said.push(notice(format!("Made {}.", folder::tilde(&path)), NoticeLevel::Info)),
                    Err(e) => return vec![notice(format!("{}: {e}", folder::tilde(&path)), NoticeLevel::Error)],
                },
                None => {
                    let parent = path.parent().unwrap_or(&path);
                    return vec![notice(format!("{}: no such folder", folder::tilde(parent)), NoticeLevel::Error)];
                }
            }
        }
        said.extend(self.save_setting("study.folder", Value::from(typed)));
        self.close_notebook();
        self.folder = Some((Folder::new(folder::normalise(&path)), typed.to_string()));
        let mut events = vec![self.list_event(), Event::StudyClosed];
        events.extend(said);
        events
    }

    fn set_roots(&mut self, roots: Vec<String>) -> Vec<Event> {
        let roots: Vec<String> = roots.into_iter().map(|r| r.trim().trim_end_matches(['/', '\\']).to_string()).filter(|r| !r.is_empty()).collect();
        let mut events: Vec<Event> = roots
            .iter()
            .filter(|typed| !crate::settings::research_path(typed).is_some_and(|p| p.is_dir()))
            .map(|typed| notice(format!("{typed}: no such folder"), NoticeLevel::Warn))
            .collect();
        events.extend(self.save_setting("study.roots", Value::from(roots.clone())));
        let rules = self.kb.rules().written().to_vec();
        self.kb.configure(roots, rules, self.kb_pdfs(), self.kb_pdftotext());
        events.push(self.kb.event());
        events
    }

    fn kb_pdfs(&self) -> bool {
        self.settings().and_then(|s| s.get("study.pdfs").and_then(Value::as_bool)).unwrap_or(true)
    }

    fn kb_pdftotext(&self) -> bool {
        self.settings().and_then(|s| s.get("study.pdftotext").and_then(Value::as_bool)).unwrap_or(false)
    }

    /// Leave something out of the knowledge base, or let it back in.
    fn exclude(&mut self, rule: &str, on: bool) -> Vec<Event> {
        let rule = rule.trim().to_string();
        if rule.is_empty() {
            return Vec::new();
        }
        let mut rules = self.kb.rules().written().to_vec();
        rules.retain(|r| *r != rule);
        if on {
            rules.push(rule);
        }
        let mut events: Vec<Event> = self.save_setting("study.exclude", Value::from(rules.clone())).into_iter().collect();
        let roots = self.kb.typed().to_vec();
        self.kb.configure(roots, rules, self.kb_pdfs(), self.kb_pdftotext());
        events.push(self.kb.event());
        events
    }

    /* ── notebooks ──────────────────────────────────────────────────── */

    fn list_event(&self) -> Event {
        let (notebooks, folder) = match &self.folder {
            Some((folder, typed)) => {
                let notebooks = folder
                    .list()
                    .into_iter()
                    .map(|(slug, notebook)| {
                        let (count, due) = match &self.open {
                            Some(open) if open.book.slug() == slug => (open.book.sources.sources.len(), self.due_today(&open.book)),
                            _ => Book::read(&folder.root().join(&slug)).map(|b| (b.sources.sources.len(), self.due_today(&b))).unwrap_or((0, 0)),
                        };
                        StudySummary::of(&slug, &notebook, count, due)
                    })
                    .collect();
                (notebooks, Some(typed.clone()))
            }
            None => (Vec::new(), None),
        };
        Event::StudyList { notebooks, open: self.open.as_ref().map(|o| o.book.slug().to_string()), folder }
    }

    fn notebook_event(&self) -> Option<Event> {
        let open = self.open.as_ref()?;
        let book = &open.book;
        let selected = book.selection();
        Some(Event::StudyNotebook {
            slug: book.slug().to_string(),
            title: book.notebook.title.clone(),
            course: book.notebook.course.clone(),
            exam: book.notebook.exam.clone(),
            read_only: open.holder.as_ref().map(|h| match h.host.is_empty() {
                true => format!("another Proton (pid {})", h.pid),
                false => format!("another Proton on {} (pid {})", h.host, h.pid),
            }),
            sources: book.sources.sources.iter().map(|s| StudySourceRow::of(s, selected.contains(&s.id))).collect(),
            notes: book.notes().iter().map(StudyNoteRow::of).collect(),
        })
    }

    /// Which files are sources of the open notebook: labelled path → id.
    fn sources_by_path(&self) -> HashMap<String, String> {
        let Some(open) = &self.open else { return HashMap::new() };
        open.book.sources.sources.iter().filter_map(|s| Some((s.origin.labelled()?, s.id.clone()))).collect()
    }

    /// What in the knowledge base is like the open notebook: by what the
    /// person said it is about, or by the words its sources share, and its
    /// course.
    fn discover(&mut self, about: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let query = match about.trim() {
            "" => {
                let texts: Vec<String> = open
                    .book
                    .sources
                    .sources
                    .iter()
                    .filter(|s| s.status.has_words())
                    .filter_map(|s| snapshot::words(&open.book, &s.id, s.units))
                    .map(|body| snapshot::plain(&body))
                    .collect();
                let mut words = proton_study::discover::about(&texts, 8);
                let course = open.book.notebook.course.trim().to_lowercase();
                if !course.is_empty() && !words.contains(&course) {
                    words.insert(0, course);
                }
                words.join(" ")
            }
            about => about.to_string(),
        };
        if query.is_empty() {
            return vec![notice("Add a source first: what is like it is what is found.".into(), NoticeLevel::Info)];
        }
        if self.kb.catalog().is_none() {
            return vec![notice("The knowledge base is still being listed.".into(), NoticeLevel::Info)];
        }
        vec![self.kb.discover(&query, &self.sources_by_path())]
    }

    fn no_folder() -> Vec<Event> {
        vec![notice("Choose where notebooks are kept first.".into(), NoticeLevel::Warn)]
    }

    fn create(&mut self, title: &str) -> Vec<Event> {
        let Some((folder, _)) = &self.folder else { return Study::no_folder() };
        match folder.create(title, proton_study::now()) {
            Ok(book) => {
                let slug = book.slug().to_string();
                self.open(&slug)
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// Open a notebook, taking its lock if no other Proton holds it.
    fn open_quietly(&mut self, slug: &str) -> Result<(), String> {
        let (folder, _) = self.folder.as_ref().ok_or("no folder")?;
        let mut book = folder.open(slug)?;
        let now = proton_study::now();
        let (lock, holder) = match lock::acquire(book.dir(), &self.instance, now) {
            Ok(lock) => (Some(lock), None),
            Err(holder) => (None, Some(holder)),
        };
        if lock.is_some() {
            book.notebook.opened = now;
            let _ = book.save();
        }
        // Its latest chat, to carry on; or a new one.
        if let Some(mut was) = self.tutor.take() {
            was.stop();
        }
        self.tutor = Some(match book.chats().first().and_then(|c| book.chat(&c.id)) {
            Some(chat) => Tutor::of(chat),
            None => Tutor::new(&book),
        });
        self.review = None;
        self.quiz = None;
        self.sitting = None;
        self.audio = None;
        self.open = Some(Open { book, lock, holder, beat: now });
        Ok(())
    }

    fn open(&mut self, slug: &str) -> Vec<Event> {
        if let Err(why) = self.open_quietly(slug) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        self.check_files();
        let mut events = vec![self.list_event()];
        events.extend(self.notebook_event());
        events.extend(self.chat_event());
        events.extend(self.studio_event());
        events
    }

    /// The open notebook, if this Proton may write it.
    fn writable(&mut self) -> Result<&mut Book, Vec<Event>> {
        match &mut self.open {
            None => Err(vec![notice("Open a notebook first.".into(), NoticeLevel::Warn)]),
            Some(Open { lock: None, .. }) => Err(vec![notice("Another Proton has this notebook open: it is read only here.".into(), NoticeLevel::Warn)]),
            Some(open) => Ok(&mut open.book),
        }
    }

    /// Say what changed in the open notebook, having saved it.
    fn saved(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(open) = &self.open {
            if let Err(why) = open.book.save().and_then(|()| open.book.save_sources()) {
                events.push(notice(why, NoticeLevel::Error));
            }
        }
        events.extend(self.notebook_event());
        events
    }

    fn details(&mut self, title: &str, course: &str, exam: Option<&str>) -> Vec<Event> {
        let exam = exam.map(str::trim).filter(|e| !e.is_empty());
        if let Some(day) = exam {
            let ok = day.len() == 10 && day.as_bytes()[4] == b'-' && day.as_bytes()[7] == b'-' && day.split('-').all(|p| p.bytes().all(|b| b.is_ascii_digit()));
            if !ok {
                return vec![notice(format!("{day}: an exam's day is written YYYY-MM-DD"), NoticeLevel::Warn)];
            }
        }
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let title = title.trim();
        if !title.is_empty() {
            book.notebook.title = title.to_string();
        }
        book.notebook.course = course.trim().to_string();
        book.notebook.exam = exam.map(str::to_string);
        let mut events = self.saved();
        events.insert(0, self.list_event());
        events
    }

    fn archive(&mut self, slug: &str, archived: bool) -> Vec<Event> {
        let Some((folder, _)) = &self.folder else { return Study::no_folder() };
        let mut book = match folder.open(slug) {
            Ok(book) => book,
            Err(why) => return vec![notice(why, NoticeLevel::Error)],
        };
        if lock::holder(book.dir(), proton_study::now()).is_some_and(|holder| holder.instance != self.instance) {
            return vec![notice("Another Proton has that notebook open.".into(), NoticeLevel::Warn)];
        }
        book.notebook.archived = archived;
        if let Err(why) = book.save() {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let mut events = Vec::new();
        if archived && self.open.as_ref().is_some_and(|o| o.book.slug() == slug) {
            self.close_notebook();
            events.push(Event::StudyClosed);
        } else if let Some(open) = &mut self.open {
            let _ = open.book.reread();
        }
        events.insert(0, self.list_event());
        events
    }

    /* ── sources ────────────────────────────────────────────────────── */

    /// Add files of the knowledge base, by their labelled paths. A file in
    /// iCloud is listed and left there unless `download` says to bring it
    /// down; one already a source is said to be.
    fn add(&mut self, paths: Vec<String>, download: bool) -> Vec<Event> {
        if self.kb.catalog().is_none() {
            return vec![notice("The knowledge base is still being listed.".into(), NoticeLevel::Info)];
        }
        // What each path is, asked of the catalogue before the notebook is
        // borrowed to be written.
        let found: Vec<(String, Located)> = paths
            .into_iter()
            .map(|path| {
                let at = self.kb.locate(&path).map(|(file, entry)| {
                    let root = self.kb.catalog().map(|c| c.roots()[entry.root].label.clone()).unwrap_or_default();
                    (file, entry.clone(), root)
                });
                (path, at)
            })
            .collect();
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let mut said = Vec::new();
        let mut items = Vec::new();
        let now = proton_study::now();
        for (path, at) in found {
            let Some((file, entry, root)) = at else {
                said.push(format!("{path}: not in the knowledge base"));
                continue;
            };
            // Already a source: brought down if it is waiting in iCloud and
            // that was asked; said otherwise.
            if let Some(source) = book.sources.sources.iter_mut().find(|s| s.origin.labelled().as_deref() == Some(path.as_str())) {
                if download && source.status == SourceStatus::InCloud {
                    source.status = SourceStatus::Downloading;
                    items.push(Item { id: source.id.clone(), path: file, kind: entry.kind, download: true });
                } else {
                    said.push(format!("{} is already {}", source.title, source.id));
                }
                continue;
            }
            if !entry.kind.readable() && !entry.kind.converted() {
                said.push(format!("{path}: a {} file is not read yet", entry.kind.name()));
                continue;
            }
            let id = book.sources.take_id();
            let status = match (entry.dataless, download) {
                (true, false) => SourceStatus::InCloud,
                (true, true) => SourceStatus::Downloading,
                (false, _) => SourceStatus::Reading,
            };
            if status != SourceStatus::InCloud {
                items.push(Item { id: id.clone(), path: file, kind: entry.kind, download: entry.dataless });
            }
            book.sources.sources.push(Source {
                id: id.clone(),
                title: entry.rel.rsplit('/').next().unwrap_or(&entry.rel).to_string(),
                kind: entry.kind.name().to_string(),
                origin: Origin::File { root, rel: entry.rel.clone() },
                role: role::guess(&entry.rel, entry.kind),
                size: entry.size,
                mtime: entry.mtime,
                sha256: String::new(),
                units: units_of(entry.kind),
                count: 0,
                thin: Vec::new(),
                status,
                added: now,
                labels: Vec::new(),
            });
            let _ = book.tick(&id);
        }
        if !items.is_empty() {
            self.start_reading(items);
        }
        let mut events = self.saved();
        events.insert(0, self.list_event());
        events.extend(said.into_iter().map(|s| notice(s, NoticeLevel::Info)));
        events
    }

    fn start_reading(&mut self, items: Vec<Item>) {
        let Some(open) = &self.open else { return };
        let (send, answers) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let snapshots = open.book.snapshots();
        let pdfs = Pdfs { extractor: proton_library::pdf::Extractor::default(), cache: proton_library::pdf::cache_dir() };
        let halt = Arc::clone(&stop);
        let _ = std::thread::Builder::new().name("proton-study-read".into()).spawn(move || read_all(items, snapshots, pdfs, halt, send));
        self.batches.push(Batch { dir: open.book.dir().to_path_buf(), answers, stop });
    }

    fn start_fetching(&mut self, items: Vec<web::Fetch>) {
        let Some(open) = &self.open else { return };
        let (send, answers) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (snapshots, kept) = (open.book.snapshots(), open.book.web_dir());
        let pdfs = Pdfs { extractor: proton_library::pdf::Extractor::default(), cache: proton_library::pdf::cache_dir() };
        let http = self.web.get_or_insert_with(|| proton_literature::http::Http::live(None)).clone();
        let halt = Arc::clone(&stop);
        let _ = std::thread::Builder::new().name("proton-study-web".into()).spawn(move || web::fetch_all(items, http, snapshots, kept, pdfs, halt, send));
        self.batches.push(Batch { dir: open.book.dir().to_path_buf(), answers, stop });
    }

    /// What the workers have read, written into their notebooks.
    fn take_answers(&mut self) -> Vec<Event> {
        let mut answered: Vec<(PathBuf, String, Answer)> = Vec::new();
        self.batches.retain(|batch| loop {
            match batch.answers.try_recv() {
                Ok((id, answer)) => answered.push((batch.dir.clone(), id, answer)),
                Err(TryRecvError::Empty) => break true,
                Err(TryRecvError::Disconnected) => break false,
            }
        });
        if answered.is_empty() {
            return Vec::new();
        }
        let mut said = Vec::new();
        let mut touched = false;
        for (dir, id, answer) in answered {
            // The open notebook is written in memory; another, from its files.
            let mut other = None;
            let book = match &mut self.open {
                Some(open) if open.book.dir() == dir => &mut open.book,
                _ => match Book::read(&dir) {
                    Ok(book) => other.insert(book),
                    Err(_) => continue,
                },
            };
            let same = |sha256: &str| book.sources.sources.iter().find(|s| s.id != id && s.sha256 == sha256 && s.status.has_words()).map(|s| s.id.clone());
            let duplicate = match &answer {
                Answer::Read(read) => same(&read.sha256),
                Answer::Fetched(fetched) => same(&fetched.read.sha256),
                _ => None,
            };
            if let Some(first) = duplicate {
                let title = book.sources.get(&id).map(|s| s.title.clone()).unwrap_or_default();
                book.sources.sources.retain(|s| s.id != id);
                snapshot::forget(book, &id);
                said.push(notice(format!("{title} is the same file as {first}, which is kept."), NoticeLevel::Info));
            } else if let Some(source) = book.sources.get_mut(&id) {
                match answer {
                    Answer::Downloading => source.status = SourceStatus::Downloading,
                    Answer::InCloud => source.status = SourceStatus::InCloud,
                    Answer::Failed(why) => source.status = SourceStatus::Failed { why },
                    Answer::Read(read) => {
                        source.size = read.size;
                        source.mtime = read.mtime;
                        source.sha256 = read.sha256.clone();
                        source.units = read.units();
                        source.count = read.count();
                        source.thin = read.thin();
                        source.status = SourceStatus::Ready;
                    }
                    Answer::Fetched(fetched) => {
                        let read = &fetched.read;
                        // Titled by what the page calls itself, unless the
                        // person has named it since.
                        if let Origin::Web { url, fetched: when } = &mut source.origin {
                            if source.title == web::provisional(url) && !fetched.title.trim().is_empty() {
                                source.title = fetched.title.trim().chars().take(160).collect();
                            }
                            *when = read.mtime;
                        }
                        source.kind = fetched.kind.name().to_string();
                        source.size = read.size;
                        source.mtime = read.mtime;
                        source.sha256 = read.sha256.clone();
                        source.units = read.units();
                        source.count = read.count();
                        source.thin = read.thin();
                        source.status = SourceStatus::Ready;
                    }
                }
            }
            if let Err(why) = book.save_sources() {
                said.push(notice(why, NoticeLevel::Error));
            }
            touched |= other.is_none();
        }
        if touched {
            said.extend(self.notebook_event());
        }
        said
    }

    fn paste(&mut self, title: &str, text: &str) -> Vec<Event> {
        let text = text.trim();
        if text.is_empty() {
            return vec![notice("There is nothing to add.".into(), NoticeLevel::Info)];
        }
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let id = book.sources.take_id();
        let body = Body::Lines(format!("{text}\n"));
        if let Err(why) = snapshot::keep(book, &id, &body) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let sha256 = snapshot::digest_text(text);
        let title = match title.trim() {
            "" => text.lines().next().unwrap_or("Pasted text").chars().take(60).collect(),
            title => title.to_string(),
        };
        book.sources.sources.push(Source {
            id: id.clone(),
            title,
            kind: Kind::Markdown.name().into(),
            origin: Origin::Pasted,
            role: Role::Notes,
            size: text.len() as u64,
            mtime: proton_study::now(),
            sha256,
            units: Units::Lines,
            count: text.lines().count(),
            thin: Vec::new(),
            status: SourceStatus::Ready,
            added: proton_study::now(),
            labels: Vec::new(),
        });
        let _ = book.tick(&id);
        let mut events = self.saved();
        events.insert(0, self.list_event());
        events
    }

    /// Add pages of the web as sources, fetched on a worker: a page as its
    /// article, a PDF whole, a YouTube video as its captions.
    fn add_urls(&mut self, typed: &[String]) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let mut said = Vec::new();
        let mut fetches = Vec::new();
        for typed in typed {
            let Some(url) = web::address(typed) else {
                said.push(format!("{}: not a web address — it would begin https://", typed.trim()));
                continue;
            };
            if let Some(source) = book.sources.sources.iter().find(|s| matches!(&s.origin, Origin::Web { url: u, .. } if *u == url)) {
                said.push(format!("{} is already {}", source.title, source.id));
                continue;
            }
            let video = proton_library::formats::web::youtube(&url).is_some();
            let id = book.sources.take_id();
            book.sources.sources.push(Source {
                id: id.clone(),
                title: web::provisional(&url),
                kind: if video { Kind::Captions } else { Kind::Web }.name().to_string(),
                origin: Origin::Web { url: url.clone(), fetched: 0 },
                role: if video { Role::Recording } else { Role::Reading },
                size: 0,
                mtime: 0,
                sha256: String::new(),
                units: if video { Units::Seconds } else { Units::Lines },
                count: 0,
                thin: Vec::new(),
                status: SourceStatus::Reading,
                added: proton_study::now(),
                labels: Vec::new(),
            });
            let _ = book.tick(&id);
            fetches.push(web::Fetch { id, url });
        }
        if !fetches.is_empty() {
            self.start_fetching(fetches);
        }
        let mut events = match said.len() == typed.len() {
            true => Vec::new(),
            false => {
                let mut events = self.saved();
                events.insert(0, self.list_event());
                events
            }
        };
        events.extend(said.into_iter().map(|s| notice(s, NoticeLevel::Info)));
        events
    }

    fn remove(&mut self, id: &str) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let before = book.sources.sources.len();
        book.sources.sources.retain(|s| s.id != id);
        if book.sources.sources.len() == before {
            return vec![notice(format!("{id}: no such source"), NoticeLevel::Warn)];
        }
        snapshot::forget(book, id);
        let ticked: Vec<String> = book.selection().into_iter().filter(|s| s != id).collect();
        let _ = book.select(&ticked);
        let mut events = self.saved();
        events.insert(0, self.list_event());
        events
    }

    fn select(&mut self, ids: &[String]) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        if let Err(why) = book.select(ids) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        self.notebook_event().into_iter().collect()
    }

    fn set_role(&mut self, id: &str, role: Role) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        match book.sources.get_mut(id) {
            Some(source) => source.role = role,
            None => return vec![notice(format!("{id}: no such source"), NoticeLevel::Warn)],
        }
        self.saved()
    }

    /// Read a source's file again — after it was edited, or came back.
    fn refresh(&mut self, id: &str) -> Vec<Event> {
        let file = {
            let Some(open) = &self.open else { return Vec::new() };
            let Some(source) = open.book.sources.get(id) else {
                return vec![notice(format!("{id}: no such source"), NoticeLevel::Warn)];
            };
            match &source.origin {
                Origin::File { root, rel } => self.kb.file(root, rel).map(|path| (path, Kind::from_name(&source.kind))),
                Origin::Web { url, .. } => {
                    let fetch = web::Fetch { id: id.to_string(), url: url.clone() };
                    let book = match self.writable() {
                        Ok(book) => book,
                        Err(events) => return events,
                    };
                    if let Some(source) = book.sources.get_mut(id) {
                        source.status = SourceStatus::Reading;
                    }
                    self.start_fetching(vec![fetch]);
                    return self.saved();
                }
                _ => return vec![notice(format!("{} has no file to read again.", source.title), NoticeLevel::Info)],
            }
        };
        let Some((path, kind)) = file else {
            return vec![notice("Its folder is no longer in the knowledge base.".into(), NoticeLevel::Warn)];
        };
        let in_cloud = std::fs::symlink_metadata(&path).is_ok_and(|m| dataless::is_dataless(&m));
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        if let Some(source) = book.sources.get_mut(id) {
            source.status = if in_cloud { SourceStatus::InCloud } else { SourceStatus::Reading };
        }
        if !in_cloud {
            self.start_reading(vec![Item { id: id.to_string(), path, kind, download: false }]);
        }
        self.saved()
    }

    /* ── the tutor ──────────────────────────────────────────────────── */

    fn close_notebook(&mut self) {
        if let Some(tutor) = &mut self.tutor {
            tutor.stop();
        }
        self.tutor = None;
        self.review = None;
        self.quiz = None;
        self.sitting = None;
        self.audio = None;
        self.open = None;
    }

    /// The open chat, whole.
    fn chat_event(&self) -> Option<Event> {
        let open = self.open.as_ref()?;
        let tutor = self.tutor.as_ref()?;
        Some(Event::StudyChat {
            id: tutor.chat.id.clone(),
            title: tutor.chat.title.clone(),
            settings: tutor.chat.settings.clone(),
            turns: tutor.chat.turns.clone(),
            live: None,
            busy: tutor.busy(),
            error: tutor.error.clone(),
            grounding: tutor.grounding().map(str::to_string),
            chats: open.book.chats(),
            lesson: tutor.chat.lesson.as_ref().and_then(|id| learning::objectives(&open.book).into_iter().find(|o| &o.id == id)),
        })
    }

    fn ask(&mut self, text: &str) -> Vec<Event> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        let roots = self.kb.labelled_roots();
        let (Some(open), Some(tutor)) = (&self.open, &mut self.tutor) else {
            return vec![notice("Open a notebook first.".into(), NoticeLevel::Warn)];
        };
        if open.lock.is_none() {
            return vec![notice("Another Proton has this notebook open: it is read only here.".into(), NoticeLevel::Warn)];
        }
        if let Err(why) = tutor.ask(&open.book, &roots, self.tutors.as_mut(), text) {
            tutor.error = Some(why);
        }
        self.chat_event().into_iter().collect()
    }

    fn poll_tutor(&mut self) -> Vec<Event> {
        let (Some(open), Some(tutor)) = (&self.open, &mut self.tutor) else { return Vec::new() };
        match tutor.poll(&open.book) {
            None => Vec::new(),
            Some(Polled::Live(live)) => vec![Event::StudyChatLive { id: tutor.chat.id.clone(), live }],
            Some(Polled::Ended(failed)) => {
                let mut events: Vec<Event> = self.chat_event().into_iter().collect();
                events.extend(failed.map(|why| notice(format!("The chat could not be kept: {why}"), NoticeLevel::Error)));
                events
            }
        }
    }

    fn new_chat(&mut self) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        if let Some(mut was) = self.tutor.take() {
            was.stop();
        }
        self.tutor = Some(Tutor::new(&open.book));
        self.chat_event().into_iter().collect()
    }

    fn open_chat(&mut self, id: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let Some(chat) = open.book.chat(id) else {
            return vec![notice(format!("{id}: no such chat"), NoticeLevel::Warn)];
        };
        if let Some(mut was) = self.tutor.take() {
            was.stop();
        }
        self.tutor = Some(Tutor::of(chat));
        self.chat_event().into_iter().collect()
    }

    /// Who answers, and how: kept as the notebook's, and the open chat's from
    /// its next question — a new conversation, told what was said.
    fn set_tutor(&mut self, settings: proton_study::tutor::TutorSettings) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let settings = tutor::changed(&book.notebook.tutor, settings);
        book.notebook.tutor = settings.clone();
        let mut events: Vec<Event> = book.save().err().map(|why| notice(why, NoticeLevel::Error)).into_iter().collect();
        if let Some(tutor) = &mut self.tutor {
            if tutor.chat.settings != settings {
                tutor.stop();
                tutor.chat.settings = settings;
            }
        }
        events.extend(self.chat_event());
        events
    }

    /// Keep an answer as a note, its citations as they were written.
    fn save_answer(&mut self, turn: usize) -> Vec<Event> {
        let Some(found) = self.tutor.as_ref().and_then(|t| t.chat.turns.get(turn).cloned().map(|turn| (turn, t.chat.id.clone()))) else {
            return vec![notice("There is no such answer.".into(), NoticeLevel::Warn)];
        };
        let (turn, chat) = found;
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let now = proton_study::now();
        let note = Note {
            id: book.new_note_id(),
            title: proton_study::tutor::title(&turn.asked),
            created: now,
            updated: now,
            from: Some(chat),
            body: format!("{}\n\n> Asked: {}", turn.answer.trim(), turn.asked.trim()),
        };
        if let Err(why) = book.write_note(&note) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let id = note.id.clone();
        let mut events = self.saved();
        events.extend(self.note_event(&id));
        events.push(notice(format!("Kept as {id}."), NoticeLevel::Info));
        events
    }

    /* ── the studio ─────────────────────────────────────────────────── */

    /// The open notebook's studio: its calls, and what they made.
    fn studio_event(&self) -> Option<Event> {
        let open = self.open.as_ref()?;
        let mut jobs = self.voicing_rows(open.book.dir());
        jobs.extend(self.jobs.rows(open.book.dir()));
        Some(Event::StudyStudio { jobs, made: open.book.made() })
    }

    fn make(&mut self, kind: proton_study::studio::Make, options: proton_study::studio::Options) -> Vec<Event> {
        if kind == proton_study::studio::Make::Table && options.columns.iter().all(|c| c.trim().is_empty()) {
            return vec![notice("Name the table's columns.".into(), NoticeLevel::Warn)];
        }
        if kind == proton_study::studio::Make::Report && options.ask.trim().is_empty() {
            return vec![notice("Say what the report is to be.".into(), NoticeLevel::Warn)];
        }
        if kind == proton_study::studio::Make::Marking {
            return vec![notice("A mark is asked for by answering.".into(), NoticeLevel::Warn)];
        }
        if kind == proton_study::studio::Make::DeepResearch && options.focus.trim().is_empty() {
            return vec![notice("Say what to research.".into(), NoticeLevel::Warn)];
        }
        let words = |o: &Open| o.book.sources.sources.iter().any(|s| s.status.has_words());
        if kind == proton_study::studio::Make::Discover && options.focus.trim().is_empty() && !self.open.as_ref().is_some_and(words) {
            return vec![notice("Say what to look for: the notebook has no sources to go by yet.".into(), NoticeLevel::Warn)];
        }
        if kind == proton_study::studio::Make::Slides && options.slide.is_some() {
            let deck = options.slides.as_deref().and_then(|d| self.open.as_ref()?.book.slides(d));
            if !deck.is_some_and(|d| d.slides.iter().any(|s| Some(&s.id) == options.slide.as_ref())) || options.ask.trim().is_empty() {
                return vec![notice("Say how the slide is to change.".into(), NoticeLevel::Warn)];
            }
        }
        if kind == proton_study::studio::Make::Diagnostic && self.open.as_ref().is_some_and(|o| learning::objectives(&o.book).is_empty()) {
            return vec![notice("Make the learning objectives first: a diagnostic asks a question of each.".into(), NoticeLevel::Warn)];
        }
        if kind == proton_study::studio::Make::PastPaper {
            let paper = options.source.as_deref().and_then(|id| self.open.as_ref()?.book.sources.get(id));
            if !paper.is_some_and(|s| s.status.has_words()) {
                return vec![notice("Choose a past paper among the sources, read.".into(), NoticeLevel::Warn)];
            }
        }
        if let Some(deck) = options.deck.as_deref() {
            if kind != proton_study::studio::Make::Flashcards || self.open.as_ref().and_then(|o| o.book.deck(deck)).is_none() {
                return vec![notice(format!("{deck}: no such deck to add cards to"), NoticeLevel::Warn)];
            }
        }
        let book = match self.writable() {
            Ok(book) => book.clone(),
            Err(events) => return events,
        };
        self.jobs.ask(&book, kind, options);
        let roots = self.kb.labelled_roots();
        self.jobs.poll(self.tutors.as_mut(), &roots);
        self.studio_event().into_iter().collect()
    }

    fn show_made(&mut self, id: &str) -> Vec<Event> {
        if id.starts_with('Q') {
            return self.show_quiz(id);
        }
        if id.starts_with('L') {
            return self.show_overview(id);
        }
        if id.starts_with('A') {
            return self.audio_shown(id);
        }
        if id.starts_with('P') {
            return self.show_slides(id);
        }
        if id.starts_with('G') {
            return self.show_graphic(id);
        }
        if id.starts_with('D') {
            return match self.deck_event(id) {
                Some(event) => vec![event],
                None => vec![notice(format!("{id}: nothing made has that id"), NoticeLevel::Warn)],
            };
        }
        let Some(open) = &self.open else { return Vec::new() };
        let Some((made, body)) = open.book.made_one(id) else {
            return vec![notice(format!("{id}: nothing made has that id"), NoticeLevel::Warn)];
        };
        use proton_study::studio::StudyMadeBody;
        let text = match &body {
            StudyMadeBody::Prose { markdown } => markdown.clone(),
            StudyMadeBody::Map { nodes } => nodes.iter().flat_map(|n| n.cites.iter()).map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            StudyMadeBody::Table { rows, .. } => rows.iter().flat_map(|r| r.cites.iter()).map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            StudyMadeBody::Objectives { objectives } => objectives.iter().flat_map(|o| o.cites.iter()).map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            StudyMadeBody::Cards { cards } => cards.iter().flat_map(|c| c.cites.iter()).map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            StudyMadeBody::Questions { questions } | StudyMadeBody::Paper { questions, .. } => questions.iter().flat_map(|q| q.cites.iter()).map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            StudyMadeBody::Overview { topics, .. } => topics.iter().map(|t| t.summary.clone()).collect::<Vec<_>>().join("\n"),
            StudyMadeBody::Audio { segments, .. } => segments.iter().flat_map(|s| s.cites.iter()).map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            StudyMadeBody::Slides { slides, .. } => slides.iter().flat_map(|s| s.cites.iter()).map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            StudyMadeBody::Slide { slide } => slide.cites.iter().map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            StudyMadeBody::Graphic { blocks, .. } => blocks.iter().flat_map(|b| b.cites.iter()).map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "),
            // What the web gave cites the web, by number, not the notebook.
            StudyMadeBody::Found { .. } => String::new(),
        };
        let citations = proton_study::cite::resolve(&open.book, &text);
        vec![Event::StudyMadeOpen { made, body, citations }]
    }

    fn show_task(&self, id: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let file = proton_study::studio::task_file(&open.book, id);
        let Some(task) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
            return vec![notice(format!("{id}: no such call"), NoticeLevel::Warn)];
        };
        let field = |key: &str| task.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        vec![Event::StudyTask {
            id: id.to_string(),
            title: field("title"),
            engine: field("engine"),
            model: field("model"),
            state: field("state"),
            why: task.get("why").and_then(Value::as_str).map(str::to_string),
            instructions: field("instructions"),
            prompt: field("prompt"),
        }]
    }

    /* ── the reader ─────────────────────────────────────────────────── */

    /// Show a source: a PDF whose file is here, unchanged and not in iCloud,
    /// in the document viewer at the page; anything else as its words were
    /// kept — never by reading a file in iCloud, which would bring it down.
    fn show(&mut self, id: &str, at: Option<StudyPlace>) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let Some(source) = open.book.sources.get(id) else {
            return vec![notice(format!("{id}: no such source"), NoticeLevel::Warn)];
        };
        let file = match &source.origin {
            Origin::File { root, rel } => self.kb.file(root, rel),
            Origin::Web { .. } => open.book.web_pdf(id),
            _ => None,
        };
        let meta = file.as_ref().and_then(|f| std::fs::symlink_metadata(f).ok());
        let in_cloud = meta.as_ref().is_some_and(dataless::is_dataless);
        if source.kind == Kind::Pdf.name() && source.status == SourceStatus::Ready && !in_cloud && meta.as_ref().is_some_and(|m| m.is_file()) {
            if let Some(path) = file {
                self.documents.insert(path.clone());
                let page = at.and_then(|a| u32::try_from(a.from).ok());
                return vec![Event::Document { path: path.display().to_string(), name: source.title.clone(), kind: crate::bus::ViewerKind::Pdf, page }];
            }
        }
        let Some(body) = snapshot::words(&open.book, id, source.units) else {
            let why = match &source.status {
                SourceStatus::InCloud => "It is in iCloud and has not been read yet.".to_string(),
                SourceStatus::Reading | SourceStatus::Downloading => "It is still being read.".to_string(),
                SourceStatus::Failed { why } => format!("It could not be read: {why}"),
                _ => "Its words are not kept here.".to_string(),
            };
            return vec![notice(format!("{}: {why}", source.title), NoticeLevel::Info)];
        };
        let gone = matches!(source.origin, Origin::File { .. }) && meta.is_none();
        let said = match (&source.status, in_cloud) {
            _ if gone => Some("Its file is gone: these are its words as they were read.".to_string()),
            (SourceStatus::Missing, _) => Some("Its file is gone: these are its words as they were read.".to_string()),
            (SourceStatus::Changed, _) => Some("Its file has changed since it was read: these are its words as they were.".to_string()),
            (_, true) => Some("Its file is in iCloud: these are its words as they were read.".to_string()),
            _ => None,
        };
        let view = match body {
            Body::Lines(text) => match Kind::from_name(&source.kind) {
                Kind::Markdown | Kind::Notebook | Kind::Document | Kind::Web => StudyView::Sections { sections: proton_study::view::sections(&text) },
                _ => StudyView::Lines { text },
            },
            Body::Pages(pages) => StudyView::Pages { pages },
            Body::Cues(cues) => StudyView::Cues { cues: cues.into_iter().map(|c| StudyCue { start: c.start, end: c.end, text: c.text }).collect() },
        };
        vec![Event::StudySource { id: id.to_string(), title: source.title.clone(), kind: source.kind.clone(), at, said, view }]
    }

    /* ── notes ──────────────────────────────────────────────────────── */

    fn note_event(&self, id: &str) -> Option<Event> {
        let open = self.open.as_ref()?;
        let note = open.book.note(id)?;
        let source = open.book.sources.sources.iter().find(|s| matches!(&s.origin, Origin::Note { note } if note == id)).map(|s| s.id.clone());
        Some(Event::StudyNote { id: note.id, title: note.title, body: note.body, updated: note.updated, source })
    }

    fn show_note(&mut self, id: &str) -> Vec<Event> {
        match self.note_event(id) {
            Some(event) => vec![event],
            None => vec![notice(format!("{id}: no such note"), NoticeLevel::Warn)],
        }
    }

    /// Write a note, new or not. A note that is a source is read again, so
    /// what the tutor reads is what was written.
    fn write_note(&mut self, id: Option<String>, title: &str, body: &str) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let now = proton_study::now();
        let title = match title.trim() {
            "" => body.lines().map(|l| l.trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("Untitled note").chars().take(80).collect(),
            title => title.to_string(),
        };
        let note = match id.as_deref().and_then(|id| book.note(id)) {
            Some(mut note) => {
                note.title = title;
                note.body = body.trim_end().to_string();
                note.updated = now;
                note
            }
            None => Note { id: book.new_note_id(), title, created: now, updated: now, from: None, body: body.trim_end().to_string() },
        };
        if let Err(why) = book.write_note(&note) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        // A note that is a source: its words, kept again.
        let snapshots = book.snapshots();
        if let Some(source) = book.sources.sources.iter_mut().find(|s| matches!(&s.origin, Origin::Note { note: n } if *n == note.id)) {
            let text = format!("# {}\n\n{}\n", note.title, note.body);
            if snapshot::keep_in(&snapshots, &source.id, &Body::Lines(text.clone())).is_ok() {
                source.title = note.title.clone();
                source.count = text.lines().count();
                source.sha256 = snapshot::digest_text(&text);
                source.mtime = now;
            }
        }
        let id = note.id.clone();
        let mut events = self.saved();
        events.extend(self.note_event(&id));
        events
    }

    fn delete_note(&mut self, id: &str) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        if let Err(why) = book.bin_note(id) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let mut events = self.saved();
        events.push(notice(format!("{id} is in the notebook's bin, .notebook/bin/."), NoticeLevel::Info));
        events
    }

    /// Make a note a source: its words kept as a source's are, ticked.
    fn note_source(&mut self, id: &str) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let Some(note) = book.note(id) else {
            return vec![notice(format!("{id}: no such note"), NoticeLevel::Warn)];
        };
        if let Some(source) = book.sources.sources.iter().find(|s| matches!(&s.origin, Origin::Note { note: n } if n == id)) {
            return vec![notice(format!("{} is already {}", note.title, source.id), NoticeLevel::Info)];
        }
        let source_id = book.sources.take_id();
        let text = format!("# {}\n\n{}\n", note.title, note.body);
        if let Err(why) = snapshot::keep(book, &source_id, &Body::Lines(text.clone())) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let now = proton_study::now();
        book.sources.sources.push(Source {
            id: source_id.clone(),
            title: note.title.clone(),
            kind: Kind::Markdown.name().into(),
            origin: Origin::Note { note: id.to_string() },
            role: Role::Notes,
            size: text.len() as u64,
            mtime: now,
            sha256: snapshot::digest_text(&text),
            units: Units::Lines,
            count: text.lines().count(),
            thin: Vec::new(),
            status: SourceStatus::Ready,
            added: now,
            labels: Vec::new(),
        });
        let _ = book.tick(&source_id);
        let mut events = self.saved();
        events.insert(0, self.list_event());
        events.extend(self.note_event(id));
        events
    }

    /// Note the open notebook's sources whose files have gone or changed
    /// since they were read, by their stamps alone.
    fn check_files(&mut self) {
        let Some(open) = &mut self.open else { return };
        let mut changed = false;
        for source in &mut open.book.sources.sources {
            let Origin::File { root, rel } = &source.origin else { continue };
            if !source.status.has_words() {
                continue;
            }
            let now = match self.kb.file(root, rel).and_then(|p| std::fs::symlink_metadata(p).ok()) {
                None => SourceStatus::Missing,
                Some(meta) => {
                    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                    match (meta.len(), mtime) == (source.size, source.mtime) {
                        true => SourceStatus::Ready,
                        false => SourceStatus::Changed,
                    }
                }
            };
            if now != source.status {
                source.status = now;
                changed = true;
            }
        }
        if changed && open.lock.is_some() {
            let _ = open.book.save_sources();
        }
    }
}
