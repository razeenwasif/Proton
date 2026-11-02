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
pub mod code;
mod latex;
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
            | Command::StudyKbAll { .. }
            | Command::StudyDiscover { .. }
            | Command::StudyNew { .. }
            | Command::StudyOpen { .. }
            | Command::StudyClose
            | Command::StudyDetails { .. }
            | Command::StudyArchive { .. }
            | Command::StudyAdd { .. }
            | Command::StudyAddFolder { .. }
            | Command::StudyDownload { .. }
            | Command::StudyPaste { .. }
            | Command::StudyAddUrls { .. }
            | Command::StudyTranscribe { .. }
            | Command::StudyMakeReadable { .. }
            | Command::StudyMakeDigest { .. }
            | Command::StudyArtifacts
            | Command::StudyImportArtifacts
            | Command::StudyAddArtifacts { .. }
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
            | Command::StudyFileOpen { .. }
            | Command::StudyFileCompile { .. }
            | Command::StudyFileSave { .. }
            | Command::StudyCategoryAdd { .. }
            | Command::StudyCategoryRename { .. }
            | Command::StudyCategoryRemove { .. }
            | Command::StudyCategoryMove { .. }
            | Command::StudyCategorize { .. }
            | Command::StudyStopAnswer
            | Command::StudyChatNew
            | Command::StudyChatOpen { .. }
            | Command::StudyTutor { .. }
            | Command::StudySaveAnswer { .. }
            | Command::StudyMake { .. }
            | Command::StudyCodeRun { .. }
            | Command::StudyCodeSave { .. }
            | Command::StudyCodePython { .. }
            | Command::StudyCodePythons
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
/// A tutor answering in a notebook no longer open: left to finish, its turn
/// kept in that notebook's chat when it does.
struct Parked {
    book: Book,
    tutor: Tutor,
}

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
    /// A recording, measured, to be transcribed when asked: how long it
    /// plays, and its file's stamp.
    Recording { seconds: u32, size: u64, mtime: u64 },
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
    /// A folder's: what its walk passes over.
    scope: Option<proton_library::folder::Scope>,
}

/// Where a folder source's folder is, and what a walk of it passes over:
/// its root's rules, for one of the knowledge base; the defaults otherwise.
fn folder_at(kb: &kb::Kb, origin: &Origin) -> Option<(PathBuf, proton_library::folder::Scope)> {
    let Origin::Folder { root, path } = origin else { return None };
    match root {
        Some(label) => Some((kb.file(label, path)?, proton_library::folder::Scope::within(kb.rules(), label, path))),
        None => Some((folder::expand(path), proton_library::folder::Scope::anywhere())),
    }
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
    /// `.tex` files a tutor wrote, being compiled.
    compiles: Vec<latex::Compile>,
    /// Tutors still answering in notebooks left while they were.
    parked: Vec<Parked>,
    /// What makes tutors: the engines, or a test's script.
    tutors: Box<dyn Tutors>,
    /// The studio's calls.
    jobs: jobs::Jobs,
    /// An import reading the sessions on a thread, until it answers.
    importing: Option<std::sync::mpsc::Receiver<Vec<proton_claude::import::Found>>>,
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
    /// Sources' pages being drawn, to be read from their pictures.
    drawing: Vec<Drawing>,
    /// Coding problems' solutions being built and run.
    runs: Vec<CodeRun>,
    /// What the studio was last told of this machine's compilers and
    /// Pythons: told again when more is found.
    told: u64,
    /// The Python coding problems are run with, when the person chose one
    /// (`study.code.python`); otherwise the first on the login shell's PATH.
    code_python: Option<PathBuf>,
}

/// A student's solution being built and run on a worker: the notebook, the
/// set of problems, the problem, and what it came to, once it has.
struct CodeRun {
    dir: PathBuf,
    made: String,
    problem: String,
    outcome: std::sync::mpsc::Receiver<proton_study::code::StudyCodeOutcome>,
}

/// A source's pages being drawn on a worker: the notebook, the source, the
/// pages, and what the worker says once they are drawn.
type Drawing = (PathBuf, String, Vec<u32>, Receiver<Result<(), String>>);

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
            compiles: Vec::new(),
            parked: Vec::new(),
            tutors: Box::new(tutor::Live),
            jobs: jobs::Jobs::default(),
            importing: None,
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
            drawing: Vec::new(),
            runs: Vec::new(),
            told: 0,
            code_python: None,
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
        Kind::Captions | Kind::Audio | Kind::Video => Units::Seconds,
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
fn read_all(items: Vec<Item>, snapshots: PathBuf, pdfs: Pdfs, media: Arc<dyn voice::Media>, stop: Arc<AtomicBool>, answers: Sender<(String, Answer)>) {
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
        // A recording is only measured — by a program of its own, so never
        // a file still in iCloud, which it would bring down.
        if item.kind.recording() {
            let answer = match std::fs::symlink_metadata(&item.path) {
                Err(why) => Answer::Failed(why.to_string()),
                Ok(meta) if dataless::is_dataless(&meta) => Answer::InCloud,
                Ok(meta) => {
                    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                    let seconds = media.duration(&item.path).map_or(0, |s| s.round().max(0.0) as u32);
                    Answer::Recording { seconds, size: meta.len(), mtime }
                }
            };
            if answers.send((item.id, answer)).is_err() {
                return;
            }
            continue;
        }
        let read = match &item.scope {
            Some(scope) => snapshot::read_folder(&item.path, scope, &pdfs),
            None => snapshot::read(&item.path, item.kind, &pdfs),
        };
        let answer = match read {
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
        self.code_python = settings.get("study.code.python").and_then(Value::as_str).filter(|p| !p.trim().is_empty()).map(folder::expand);
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
            // Which compilers and Pythons are here, looked for before coding
            // problems are asked for, so the studio can say what it offers.
            let chosen = self.code_python.clone();
            std::thread::spawn(move || {
                code::Tools::here();
                code::look_for_pythons(chosen);
            });
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
        events.extend(self.poll_import());
        events.extend(self.poll_tutor());
        events.extend(self.poll_parked());
        events.extend(self.poll_compiles());
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
            // Sources labelled, or their pages read: the notebook says so.
            let open = self.open.as_ref().map(|o| o.book.dir().to_path_buf());
            let mut said = false;
            for (dir, make, options) in self.jobs.take_done() {
                if Some(&dir) != open.as_ref() {
                    continue;
                }
                match make {
                    proton_study::studio::Make::Labels => said = true,
                    proton_study::studio::Make::Readable => {
                        said |= self.thin_again(options.source.as_deref().unwrap_or_default());
                    }
                    _ => {}
                }
            }
            if said {
                events.extend(self.notebook_event());
            }
        }
        events.extend(self.poll_drawing());
        events.extend(self.poll_code());
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
        for parked in &mut self.parked {
            parked.tutor.stop();
        }
        self.parked.clear();
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
            Command::StudyKbAll { root, dir } => vec![self.kb.all(&root, &dir, &self.sources_by_path())],
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
            Command::StudyAddFolder { path } => self.add_folder(&path),
            Command::StudyDownload { paths } => self.add(paths, true),
            Command::StudyPaste { title, text } => self.paste(&title, &text),
            Command::StudyAddUrls { urls } => self.add_urls(&urls),
            Command::StudyTranscribe { id } => self.transcribe(&id),
            Command::StudyMakeReadable { id } => self.make_readable(&id),
            Command::StudyMakeDigest { id } => self.make_digest(&id),
            Command::StudyArtifacts => vec![self.artifacts_event()],
            Command::StudyImportArtifacts => self.import_artifacts(),
            Command::StudyAddArtifacts { ids } => self.add_artifacts(&ids),
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
            Command::StudyFileOpen { name } => self.open_file(&name),
            Command::StudyFileCompile { name } => self.compile(&name),
            Command::StudyFileSave { name, body } => self.save_file(&name, &body),
            Command::StudyCategoryAdd { name } => self.categories(|s| s.add_category(&name).map(|_| ())),
            Command::StudyCategoryRename { from, to } => self.categories(|s| s.rename_category(&from, &to).map(|_| ())),
            Command::StudyCategoryRemove { name } => self.categories(|s| s.remove_category(&name)),
            Command::StudyCategoryMove { name, to } => self.categories(|s| s.move_category(&name, to)),
            Command::StudyCategorize { ids, category } => self.categories(|s| s.categorize(&ids, category.as_deref())),
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
            Command::StudyCodeRun { made, problem, code } => self.code_run(&made, &problem, &code),
            Command::StudyCodeSave { made, problem, code } => self.code_save(&made, &problem, &code),
            Command::StudyCodePython { path } => self.choose_python(&path),
            Command::StudyCodePythons => {
                let chosen = self.code_python.clone();
                std::thread::spawn(move || code::look_for_pythons(chosen));
                Vec::new()
            }
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
            Command::StudyVoiceFind { query, language, mine } => self.find_voices(&query, &language, mine),
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
        let labels = book.labels();
        Some(Event::StudyNotebook {
            slug: book.slug().to_string(),
            title: book.notebook.title.clone(),
            course: book.notebook.course.clone(),
            exam: book.notebook.exam.clone(),
            read_only: open.holder.as_ref().map(|h| match h.host.is_empty() {
                true => format!("another Proton (pid {})", h.pid),
                false => format!("another Proton on {} (pid {})", h.host, h.pid),
            }),
            sources: book
                .sources
                .sources
                .iter()
                .map(|s| StudySourceRow { labels: labels.get(&s.id).cloned().unwrap_or_default(), ..StudySourceRow::of(s, selected.contains(&s.id)) })
                .collect(),
            categories: book.sources.categories.clone(),
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
        // The tutor of the notebook left: still answering, it is left to
        // finish there; otherwise let go.
        self.park_tutor();
        // Its own tutor, if one was left answering here; else its latest
        // chat, to carry on; or a new one.
        let parked = self.parked.iter().position(|p| p.book.dir() == book.dir()).map(|at| self.parked.remove(at).tutor);
        self.tutor = Some(match parked {
            Some(tutor) => tutor,
            None => match book.chats().first().and_then(|c| book.chat(&c.id)) {
                Some(chat) => Tutor::of(chat),
                None => Tutor::new(&book),
            },
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
        // A recording with captions beside it: those are its words, free.
        let beside: Vec<Option<String>> = found
            .iter()
            .map(|(path, at)| {
                at.as_ref().filter(|(_, entry, _)| entry.kind.recording())?;
                let stem = path.rsplit_once('.').map_or(path.as_str(), |(stem, _)| stem);
                ["srt", "vtt", "en.srt", "en.vtt"].iter().map(|ext| format!("{stem}.{ext}")).find(|p| self.kb.locate(p).is_some())
            })
            .collect();
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let mut said = Vec::new();
        let mut items = Vec::new();
        let now = proton_study::now();
        for ((path, at), beside) in found.into_iter().zip(beside) {
            let Some((file, entry, root)) = at else {
                said.push(format!("{path}: not in the knowledge base"));
                continue;
            };
            // Already a source: brought down if it is waiting in iCloud and
            // that was asked; said otherwise.
            if let Some(source) = book.sources.sources.iter_mut().find(|s| s.origin.labelled().as_deref() == Some(path.as_str())) {
                if download && source.status == SourceStatus::InCloud {
                    source.status = SourceStatus::Downloading;
                    items.push(Item { id: source.id.clone(), path: file, kind: entry.kind, download: true, scope: None });
                } else {
                    said.push(format!("{} is already {}", source.title, source.id));
                }
                continue;
            }
            if !entry.kind.addable() {
                said.push(format!("{path}: a {} file is not read yet", entry.kind.name()));
                continue;
            }
            if let Some(captions) = beside {
                said.push(format!("{path} has captions beside it, {captions}: add those to read it free, rather than transcribe it."));
            }
            let id = book.sources.take_id();
            let status = match (entry.dataless, download) {
                (true, false) => SourceStatus::InCloud,
                (true, true) => SourceStatus::Downloading,
                (false, _) => SourceStatus::Reading,
            };
            if status != SourceStatus::InCloud {
                items.push(Item { id: id.clone(), path: file, kind: entry.kind, download: entry.dataless, scope: None });
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

    /// Add a folder as one source — a lab, its files read together, cited by
    /// file and line: one of the knowledge base as the picker names it,
    /// `Courses/COMP2310/lab1`, or any folder by its path. One inside a root
    /// of the knowledge base is kept as the picker would name it, and its
    /// rules apply.
    fn add_folder(&mut self, typed: &str) -> Vec<Event> {
        let typed = typed.trim().trim_end_matches('/');
        if typed.is_empty() {
            return vec![notice("Say which folder to add.".into(), NoticeLevel::Info)];
        }
        let origin = if typed.starts_with('~') || typed.starts_with('/') {
            let dir = folder::normalise(&folder::expand(typed));
            let within = self.kb.labelled_roots().into_iter().find_map(|(label, root)| {
                let rel = dir.strip_prefix(folder::normalise(&root)).ok()?;
                Some((label, rel.to_string_lossy().replace('\\', "/")))
            });
            match within {
                Some((_, rel)) if rel.is_empty() => {
                    return vec![notice(format!("{} is a whole folder of the knowledge base: add a folder inside it.", folder::tilde(&dir)), NoticeLevel::Info)];
                }
                Some((label, rel)) => Origin::Folder { root: Some(label), path: rel },
                None => Origin::Folder { root: None, path: folder::tilde(&dir) },
            }
        } else {
            match typed.split_once('/') {
                Some((label, rel)) if self.kb.root_path(label).is_some() && !rel.trim_matches('/').is_empty() => {
                    Origin::Folder { root: Some(label.to_string()), path: rel.trim_matches('/').to_string() }
                }
                _ => return vec![notice(format!("{typed}: not a folder of the knowledge base. Give a folder's path: ~/… or /…"), NoticeLevel::Info)],
            }
        };
        let Some((dir, scope)) = folder_at(&self.kb, &origin) else {
            return vec![notice(format!("{typed}: not a folder of the knowledge base"), NoticeLevel::Info)];
        };
        if dir == folder::expand("~") || dir.parent().is_none() {
            return vec![notice(format!("{} is too much to be one source: add a lab's folder.", folder::tilde(&dir)), NoticeLevel::Info)];
        }
        if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            return vec![notice(format!("{}: not a folder", folder::tilde(&dir)), NoticeLevel::Warn)];
        }
        if let Some(why) = scope.refused() {
            return vec![notice(format!("{} is left out of the knowledge base: {why}", folder::tilde(&dir)), NoticeLevel::Info)];
        }
        if let Some(open) = &self.open {
            let same = open.book.sources.sources.iter().find(|s| folder_at(&self.kb, &s.origin).is_some_and(|(d, _)| d == dir));
            if let Some(source) = same {
                return vec![notice(format!("{} is already {}.", source.title, source.id), NoticeLevel::Info)];
            }
        }
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let id = book.sources.take_id();
        book.sources.sources.push(Source {
            id: id.clone(),
            title: folder::name_of(&dir),
            kind: Kind::Folder.name().to_string(),
            origin,
            role: Role::Lab,
            size: 0,
            mtime: 0,
            sha256: String::new(),
            units: Units::Lines,
            count: 0,
            thin: Vec::new(),
            status: SourceStatus::Reading,
            added: proton_study::now(),
            labels: Vec::new(),
        });
        let _ = book.tick(&id);
        self.start_reading(vec![Item { id, path: dir, kind: Kind::Folder, download: false, scope: Some(scope) }]);
        let mut events = self.saved();
        events.insert(0, self.list_event());
        events
    }

    fn start_reading(&mut self, items: Vec<Item>) {
        let Some(open) = &self.open else { return };
        let (send, answers) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let snapshots = open.book.snapshots();
        let pdfs = Pdfs { extractor: proton_library::pdf::Extractor::default(), cache: proton_library::pdf::cache_dir() };
        let halt = Arc::clone(&stop);
        let media = self.voice.media();
        let _ = std::thread::Builder::new().name("proton-study-read".into()).spawn(move || read_all(items, snapshots, pdfs, media, halt, send));
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
        book.sources.forget(&id);
                snapshot::forget(book, &id);
                said.push(notice(format!("{title} is the same file as {first}, which is kept."), NoticeLevel::Info));
            } else if let Some(source) = book.sources.get_mut(&id) {
                match answer {
                    Answer::Downloading => source.status = SourceStatus::Downloading,
                    Answer::Recording { seconds, size, mtime } => {
                        source.size = size;
                        source.mtime = mtime;
                        source.units = Units::Seconds;
                        source.count = seconds as usize;
                        source.status = SourceStatus::Untranscribed { seconds };
                    }
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

    /// Transcribe a recording among the sources, as the person asked having
    /// been told what it costs: its sound sent to Fish on a worker, its
    /// words kept as timed cues. Never a file still in iCloud.
    fn transcribe(&mut self, id: &str) -> Vec<Event> {
        let file = {
            let Some(open) = &self.open else { return Vec::new() };
            let Some(source) = open.book.sources.get(id) else {
                return vec![notice(format!("{id}: no such source"), NoticeLevel::Warn)];
            };
            let waiting = matches!(source.status, SourceStatus::Untranscribed { .. } | SourceStatus::Failed { .. });
            if !Kind::from_name(&source.kind).recording() || !waiting {
                return vec![notice(format!("{} is not a recording waiting to be transcribed.", source.title), NoticeLevel::Info)];
            }
            match &source.origin {
                Origin::File { root, rel } => self.kb.file(root, rel),
                _ => None,
            }
        };
        let Some(file) = file else {
            return vec![notice("Its folder is no longer in the knowledge base.".into(), NoticeLevel::Warn)];
        };
        match std::fs::symlink_metadata(&file) {
            Err(_) => return vec![notice("Its file is gone.".into(), NoticeLevel::Warn)],
            Ok(meta) if dataless::is_dataless(&meta) => return vec![notice("It is in iCloud: download it first.".into(), NoticeLevel::Info)],
            Ok(_) => {}
        }
        let speaker = match self.voice.speaker() {
            Ok(speaker) => speaker,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let media = self.voice.media();
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        if let Some(source) = book.sources.get_mut(id) {
            source.status = SourceStatus::Transcribing;
        }
        let (snapshots, work, dir) = (book.snapshots(), book.dir().join(".notebook/heard").join(id), book.dir().to_path_buf());
        let (send, answers) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (halt, id) = (Arc::clone(&stop), id.to_string());
        let _ = std::thread::Builder::new().name("proton-study-hear".into()).spawn(move || voice::hear(id, file, speaker, media, snapshots, work, halt, send));
        self.batches.push(Batch { dir, answers, stop });
        self.saved()
    }

    /// A source's pages with few words, drawn on a worker and then read from
    /// their pictures, eight to a call: the source's own file, here, never
    /// one in iCloud.
    /// Digest a long source: a call for each part not digested and not
    /// being digested, run as the studio runs its calls.
    fn make_digest(&mut self, id: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let Some(source) = open.book.sources.get(id).filter(|s| s.status.has_words()) else {
            return vec![notice(format!("{id} has no words to digest."), NoticeLevel::Info)];
        };
        let Some(parts) = proton_study::digest::parts(&open.book, id) else {
            return vec![notice(format!("{} is short enough to be read whole.", source.title), NoticeLevel::Info)];
        };
        let book = open.book.clone();
        let underway = self.jobs.digesting(book.dir(), id);
        let missing = proton_study::digest::missing(&book, id);
        if missing.is_empty() {
            return vec![notice(format!("{} is digested already.", source.title), NoticeLevel::Info)];
        }
        let missing: Vec<_> = missing.into_iter().filter(|p| !underway.contains(&p.hash)).collect();
        if missing.is_empty() {
            return vec![notice(format!("{} is being digested already.", source.title), NoticeLevel::Info)];
        }
        for part in &missing {
            let options = proton_study::studio::Options { source: Some(id.to_string()), part: Some(part.asked(parts.len())), ..Default::default() };
            self.jobs.ask(&book, proton_study::studio::Make::Digest, options);
        }
        let roots = self.kb.labelled_roots();
        self.jobs.poll(self.tutors.as_mut(), &roots);
        self.studio_event().into_iter().collect()
    }

    fn make_readable(&mut self, id: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let Some(source) = open.book.sources.get(id) else {
            return vec![notice(format!("{id}: no such source"), NoticeLevel::Warn)];
        };
        if source.kind != Kind::Pdf.name() || source.thin.is_empty() || !source.status.has_words() {
            return vec![notice(format!("{} has no pages with few words to read.", source.title), NoticeLevel::Info)];
        }
        if self.drawing.iter().any(|(_, s, _, _)| s == id) {
            return vec![notice(format!("{}'s pages are being drawn.", source.title), NoticeLevel::Info)];
        }
        let file = match &source.origin {
            Origin::File { root, rel } => self.kb.file(root, rel),
            Origin::Web { .. } => open.book.web_pdf(id),
            Origin::Made { file } => Some(open.book.files_dir().join(file)),
            _ => None,
        };
        let Some(file) = file.filter(|f| std::fs::symlink_metadata(f).is_ok_and(|m| m.is_file() && !dataless::is_dataless(&m))) else {
            return vec![notice(format!("{}'s file is not here to draw: in iCloud, or gone.", source.title), NoticeLevel::Info)];
        };
        if proton_library::pdf::find_pdftoppm().is_none() {
            return vec![notice("Pages cannot be drawn here: Poppler is not installed (brew install poppler).".into(), NoticeLevel::Warn)];
        }
        let pages: Vec<u32> = source.thin.iter().map(|&p| p as u32).collect();
        let (dir, into) = (open.book.dir().to_path_buf(), open.book.pages_dir(id));
        let (send, rx) = mpsc::channel();
        let drawn: Vec<usize> = source.thin.clone();
        let _ = std::thread::Builder::new().name("proton-study-draw".into()).spawn(move || {
            let _ = send.send(proton_library::pdf::draw_pages(&file, &drawn, &into, proton_study::readable::DPI).map(|_| ()));
        });
        let said = format!("Drawing {}'s {} pages with few words, to be read from their pictures.", source.title, pages.len());
        self.drawing.push((dir, id.to_string(), pages, rx));
        vec![notice(said, NoticeLevel::Info)]
    }

    /// Pages drawn: read, eight to a call.
    fn poll_drawing(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut drawn = Vec::new();
        self.drawing.retain(|(dir, id, pages, rx)| match rx.try_recv() {
            Ok(outcome) => {
                drawn.push((dir.clone(), id.clone(), pages.clone(), outcome));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                drawn.push((dir.clone(), id.clone(), pages.clone(), Err("the pages could not be drawn".into())));
                false
            }
        });
        for (dir, id, pages, outcome) in drawn {
            let Some(book) = self.open.as_ref().filter(|o| o.book.dir() == dir).map(|o| o.book.clone()) else { continue };
            if let Err(why) = outcome {
                events.push(notice(format!("{id}'s pages could not be drawn: {why}"), NoticeLevel::Warn));
                continue;
            }
            for batch in pages.chunks(proton_study::readable::BATCH) {
                let options = proton_study::studio::Options { source: Some(id.clone()), pages: batch.to_vec(), ..Default::default() };
                self.jobs.ask(&book, proton_study::studio::Make::Readable, options);
            }
            let roots = self.kb.labelled_roots();
            self.jobs.poll(self.tutors.as_mut(), &roots);
            events.extend(self.studio_event());
        }
        events
    }

    /// The artifacts Proton keeps, each with the source it already is here.
    fn artifacts_event(&self) -> Event {
        let store = proton_claude::artifacts::Store::open();
        let have: HashMap<String, String> = self
            .open
            .as_ref()
            .map(|o| o.book.sources.sources.iter().filter_map(|s| match &s.origin {
                Origin::Artifact { id, .. } => Some((id.clone(), s.id.clone())),
                _ => None,
            }).collect())
            .unwrap_or_default();
        let artifacts = store
            .list()
            .iter()
            .map(|a| {
                let latest = a.versions.iter().rev().find(|v| !v.file.is_empty()).or(a.versions.last());
                proton_study::view::StudyArtifactRow {
                    id: a.id.clone(),
                    title: a.title.clone(),
                    description: a.description.clone(),
                    icon: a.icon.clone(),
                    seq: latest.map_or(0, |v| v.seq),
                    at: latest.map_or(0, |v| v.at),
                    file: latest.map(|v| v.file.clone()).unwrap_or_default(),
                    source: have.get(&a.id).cloned(),
                }
            })
            .collect();
        Event::StudyArtifacts { artifacts, importing: self.importing.is_some() }
    }

    /// Read the sessions for artifacts Proton does not keep yet — on a thread,
    /// because it is a gigabyte of files on a long-used machine.
    fn import_artifacts(&mut self) -> Vec<Event> {
        if self.importing.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new().name("proton-artifact-import".into()).spawn(move || {
                let _ = tx.send(proton_claude::import::discover());
            });
            self.importing = Some(rx);
        }
        vec![self.artifacts_event()]
    }

    /// An import that has finished reading: keep what it found.
    fn poll_import(&mut self) -> Vec<Event> {
        let Some(importing) = &self.importing else { return Vec::new() };
        let found = match importing.try_recv() {
            Ok(found) => found,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Vec::new(),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        self.importing = None;
        let done = proton_claude::artifacts::Store::open().import(found);
        let mended = match done.mended {
            0 => String::new(),
            1 => " One kept before without the files its page uses — clips, pictures — has them now.".to_string(),
            n => format!(" {n} kept before without the files their pages use — clips, pictures — have them now."),
        };
        let text = match (done.added, done.updated) {
            (0, 0) if done.mended > 0 => mended.trim_start().to_string(),
            (0, 0) => "No artifacts to import: every one published from this machine is already here.".to_string(),
            (added, updated) => {
                let mut parts = Vec::new();
                if added > 0 {
                    parts.push(format!("{added} artifact{}", if added == 1 { "" } else { "s" }));
                }
                if updated > 0 {
                    parts.push(format!("{updated} newer version{}", if updated == 1 { "" } else { "s" }));
                }
                let remote = if done.remote > 0 {
                    format!(" {} of them no longer have a page on this machine and open on claude.ai.", done.remote)
                } else {
                    String::new()
                };
                format!("Imported {}.{remote}{mended}", parts.join(" and "))
            }
        };
        vec![self.artifacts_event(), notice(text, NoticeLevel::Info)]
    }

    /// Add artifacts as sources: each the latest version of it Proton keeps,
    /// read as its page's article, on a worker.
    fn add_artifacts(&mut self, ids: &[String]) -> Vec<Event> {
        let store = proton_claude::artifacts::Store::open();
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let mut said = Vec::new();
        let mut items = Vec::new();
        for id in ids {
            let Some(artifact) = store.list().iter().find(|a| &a.id == id) else {
                said.push(format!("{id}: no such artifact is kept here"));
                continue;
            };
            if let Some(source) = book.sources.sources.iter().find(|s| matches!(&s.origin, Origin::Artifact { id: a, .. } if a == id)) {
                said.push(format!("{} is already {}", source.title, source.id));
                continue;
            }
            let Some(version) = artifact.versions.iter().rev().find(|v| !v.file.is_empty()) else {
                said.push(format!("{}: only its address is kept here, so there is nothing to read", artifact.title));
                continue;
            };
            let path = store.root().join(&artifact.id).join(version.seq.to_string()).join(&version.file);
            let source = book.sources.take_id();
            book.sources.sources.push(Source {
                id: source.clone(),
                title: artifact.title.clone(),
                kind: Kind::Artifact.name().to_string(),
                origin: Origin::Artifact { id: artifact.id.clone(), seq: version.seq, file: version.file.clone() },
                role: Role::Reading,
                size: 0,
                mtime: version.at / 1000,
                sha256: String::new(),
                units: Units::Lines,
                count: 0,
                thin: Vec::new(),
                status: SourceStatus::Reading,
                added: proton_study::now(),
                labels: Vec::new(),
            });
            let _ = book.tick(&source);
            items.push(Item { id: source, path, kind: Kind::Artifact, download: false, scope: None });
        }
        if !items.is_empty() {
            self.start_reading(items);
        }
        let mut events = self.saved();
        events.insert(0, self.list_event());
        events.push(self.artifacts_event());
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
        book.sources.forget(&id);
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

    /// A change to the categories, saved, and the panel drawn again — or
    /// why it was not made.
    fn categories(&mut self, change: impl FnOnce(&mut proton_study::model::Sources) -> Result<(), String>) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        if let Err(why) = change(&mut book.sources) {
            return vec![notice(why, NoticeLevel::Warn)];
        }
        if let Err(why) = book.save_sources() {
            return vec![notice(why, NoticeLevel::Error)];
        }
        self.notebook_event().into_iter().collect()
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
                Origin::File { root, rel } => self.kb.file(root, rel).map(|path| (path, Kind::from_name(&source.kind), None)),
                Origin::Made { file } => Some((open.book.files_dir().join(file), Kind::from_name(&source.kind), None)),
                Origin::Folder { .. } => match folder_at(&self.kb, &source.origin) {
                    Some((dir, _)) if !dir.is_dir() => return vec![notice(format!("{}'s folder is gone: {}", source.title, folder::tilde(&dir)), NoticeLevel::Warn)],
                    found => found.map(|(dir, scope)| (dir, Kind::Folder, Some(scope))),
                },
                Origin::Artifact { id: artifact, .. } => {
                    let store = proton_claude::artifacts::Store::open();
                    let latest = store.list().iter().find(|a| &a.id == artifact).and_then(|a| a.versions.iter().rev().find(|v| !v.file.is_empty()).cloned());
                    let Some(version) = latest else {
                        return vec![notice(format!("{} is no longer kept here.", source.title), NoticeLevel::Warn)];
                    };
                    let path = store.root().join(artifact).join(version.seq.to_string()).join(&version.file);
                    let artifact = artifact.clone();
                    let book = match self.writable() {
                        Ok(book) => book,
                        Err(events) => return events,
                    };
                    if let Some(source) = book.sources.get_mut(id) {
                        source.origin = Origin::Artifact { id: artifact, seq: version.seq, file: version.file.clone() };
                        source.status = SourceStatus::Reading;
                    }
                    self.start_reading(vec![Item { id: id.to_string(), path, kind: Kind::Artifact, download: false, scope: None }]);
                    return self.saved();
                }
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
        let Some((path, kind, scope)) = file else {
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
            self.start_reading(vec![Item { id: id.to_string(), path, kind, download: false, scope }]);
        }
        self.saved()
    }

    /* ── the tutor ──────────────────────────────────────────────────── */

    fn close_notebook(&mut self) {
        self.park_tutor();
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

    /// Show a file the tutor wrote, in Proton: a PDF in the document viewer,
    /// as a source's is; anything else in Notebook, to read and edit. Not
    /// handed to the desktop — the app it would go to (Quark, here) may take
    /// a file sent while it is running and show nothing.
    fn open_file(&mut self, name: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let Some(name) = proton_study::tutor::file_name(name) else {
            return vec![notice(format!("{name}: not a file here"), NoticeLevel::Warn)];
        };
        let path = open.book.files_dir().join(&name);
        if !path.is_file() {
            return vec![notice(format!("{name} is not in the notebook's files/ any more."), NoticeLevel::Warn)];
        }
        if crate::bus::ViewerKind::of(&path).is_some() {
            self.documents.insert(path.clone());
            return vec![Event::Document { path: path.display().to_string(), name, kind: crate::bus::ViewerKind::Pdf, page: None }];
        }
        let body = match std::fs::read_to_string(&path) {
            Ok(body) => body,
            Err(why) => return vec![notice(format!("{name}: {why}"), NoticeLevel::Warn)],
        };
        let pdf = name.ends_with(".tex") && open.book.files_dir().join(format!("{}.pdf", name.trim_end_matches(".tex"))).is_file();
        vec![Event::StudyFile { name, body, pdf }]
    }

    /// Write a file the tutor wrote, as edited in Notebook.
    fn save_file(&mut self, name: &str, body: &str) -> Vec<Event> {
        let written = match self.writable() {
            Ok(book) => book.write_file(name, body),
            Err(events) => return events,
        };
        match written {
            Ok(_) => {
                let mut events = vec![notice(format!("Saved {name}."), NoticeLevel::Info)];
                events.extend(self.open_file(name));
                events
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// Compile a `.tex` the tutor wrote, asked for with its button.
    fn compile(&mut self, name: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        if self.compiles.iter().any(|c| c.name == name) {
            return vec![notice(format!("{name} is being compiled already."), NoticeLevel::Info)];
        }
        let Some(name) = proton_study::tutor::file_name(name) else {
            return vec![notice(format!("{name}: not a file here"), NoticeLevel::Warn)];
        };
        match latex::start(&open.book.files_dir(), &name) {
            Ok(compile) => {
                self.compiles.push(compile);
                vec![notice(format!("Compiling {name} with pdflatex…"), NoticeLevel::Info)]
            }
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// Say how each compile ended; a PDF made is marked on the turns that
    /// wrote its `.tex`, whose button then opens it.
    fn poll_compiles(&mut self) -> Vec<Event> {
        if self.compiles.is_empty() {
            return Vec::new();
        }
        let mut ended = Vec::new();
        self.compiles.retain(|c| match c.ended() {
            Some(result) => {
                ended.push((c.name.clone(), result));
                false
            }
            None => true,
        });
        let mut events = Vec::new();
        for (name, result) in ended {
            let pdf = format!("{}.pdf", name.trim_end_matches(".tex"));
            match result {
                Ok(()) => {
                    events.push(notice(format!("Compiled {pdf}, in the notebook's files/."), NoticeLevel::Info));
                    if let (Some(open), Some(tutor)) = (&self.open, &mut self.tutor) {
                        let mut marked = false;
                        for file in tutor.chat.turns.iter_mut().flat_map(|t| t.files.iter_mut()).filter(|f| f.name == name) {
                            file.pdf = true;
                            marked = true;
                        }
                        if marked {
                            let _ = open.book.write_chat(&tutor.chat);
                        }
                    }
                    events.extend(self.chat_event());
                    events.extend(self.keep_made_pdf(&pdf));
                }
                Err(why) => events.push(notice(format!("{name} did not compile — {why}"), NoticeLevel::Error)),
            }
        }
        events
    }

    /// A PDF compiled into `files/`, made a source of the notebook and
    /// ticked, so the tutor reads it — or, compiled again, its source read
    /// again: one source a file, however often it is remade.
    fn keep_made_pdf(&mut self, pdf: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let path = open.book.files_dir().join(pdf);
        let Ok(meta) = std::fs::metadata(&path) else { return Vec::new() };
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let existing = open.book.sources.sources.iter().find(|s| matches!(&s.origin, Origin::Made { file } if file == pdf)).map(|s| s.id.clone());
        let book = match self.writable() {
            Ok(book) => book,
            Err(events) => return events,
        };
        let (id, said) = match existing {
            Some(id) => {
                if let Some(source) = book.sources.get_mut(&id) {
                    source.status = SourceStatus::Reading;
                    source.size = meta.len();
                    source.mtime = mtime;
                }
                (id.clone(), format!("{pdf} is {id} already: read again as compiled now."))
            }
            None => {
                let id = book.sources.take_id();
                book.sources.sources.push(Source {
                    id: id.clone(),
                    title: pdf.to_string(),
                    kind: Kind::Pdf.name().to_string(),
                    origin: Origin::Made { file: pdf.to_string() },
                    role: role::guess(pdf, Kind::Pdf),
                    size: meta.len(),
                    mtime,
                    sha256: String::new(),
                    units: units_of(Kind::Pdf),
                    count: 0,
                    thin: Vec::new(),
                    status: SourceStatus::Reading,
                    added: proton_study::now(),
                    labels: Vec::new(),
                });
                let _ = book.tick(&id);
                (id.clone(), format!("{pdf} is a source now, {id}, ticked for the tutor."))
            }
        };
        self.start_reading(vec![Item { id, path, kind: Kind::Pdf, download: false, scope: None }]);
        let mut events = self.saved();
        events.insert(0, self.list_event());
        events.push(notice(said, NoticeLevel::Info));
        events
    }

    /// Leave the open notebook's tutor: one answering goes on in the
    /// background, to be kept in its notebook; one idle is let go.
    fn park_tutor(&mut self) {
        let Some(mut tutor) = self.tutor.take() else { return };
        match (&self.open, tutor.busy()) {
            (Some(open), true) if open.lock.is_some() => self.parked.push(Parked { book: open.book.clone(), tutor }),
            _ => tutor.stop(),
        }
    }

    /// Tutors answering in notebooks left: each turn that ends is kept in its
    /// own notebook's chat, with the notes and files it wrote, and the tutor
    /// let go — unless that notebook is open again, when it is its tutor.
    fn poll_parked(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut done = Vec::new();
        for (at, parked) in self.parked.iter_mut().enumerate() {
            let Some(Polled::Ended(failed, drafts)) = parked.tutor.poll(&parked.book) else { continue };
            let title = parked.book.notebook.title.clone();
            if let Some(why) = failed {
                events.push(notice(format!("The chat in {title} could not be kept: {why}"), NoticeLevel::Error));
            }
            // What it wrote, kept there: notes new or again, and files.
            let mut notes = Vec::new();
            let mut files = Vec::new();
            for draft in drafts {
                let now = proton_study::now();
                if let Some(name) = draft.file {
                    match parked.book.write_file(&name, &draft.body) {
                        Ok(_) => files.push(proton_study::tutor::StudyTurnFile { name, pdf: false }),
                        Err(why) => events.push(notice(why, NoticeLevel::Error)),
                    }
                    continue;
                }
                let before = draft.id.as_deref().and_then(|id| parked.book.note(id));
                let updated = before.is_some();
                let note = match before {
                    Some(mut note) => {
                        note.title = draft.title;
                        note.body = draft.body;
                        note.updated = now;
                        note
                    }
                    None => Note { id: parked.book.new_note_id(), title: draft.title, created: now, updated: now, from: Some(parked.tutor.chat.id.clone()), body: draft.body },
                };
                match parked.book.write_note(&note) {
                    Ok(()) => notes.push(proton_study::tutor::StudyTurnNote { id: note.id, title: note.title, updated }),
                    Err(why) => events.push(notice(why, NoticeLevel::Error)),
                }
            }
            if !notes.is_empty() || !files.is_empty() {
                if let Some(turn) = parked.tutor.chat.turns.last_mut() {
                    turn.notes = notes;
                    turn.files = files;
                }
                let _ = parked.book.write_chat(&parked.tutor.chat);
            }
            events.push(notice(format!("The tutor finished answering in {title}; it is kept in its chat."), NoticeLevel::Info));
            done.push(at);
        }
        for at in done.into_iter().rev() {
            let mut parked = self.parked.remove(at);
            parked.tutor.stop();
        }
        events
    }

    fn poll_tutor(&mut self) -> Vec<Event> {
        let (Some(open), Some(tutor)) = (&self.open, &mut self.tutor) else { return Vec::new() };
        match tutor.poll(&open.book) {
            None => Vec::new(),
            Some(Polled::Live(live)) => vec![Event::StudyChatLive { id: tutor.chat.id.clone(), live }],
            Some(Polled::Ended(failed, notes)) => {
                let mut events = self.keep_tutor_notes(notes);
                events.extend(self.chat_event());
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
        // Said without asking any program: this is drawn often.
        let languages = match code::Tools::found() {
            Some(tools) => {
                let mut here = tools.languages();
                if self.code_python.as_ref().is_some_and(|p| p.is_file()) && !here.contains(&proton_study::code::Language::Python) {
                    here.insert(0, proton_study::code::Language::Python);
                }
                here.iter().map(|l| l.key().to_string()).collect()
            }
            None => Vec::new(),
        };
        Some(Event::StudyStudio { jobs, made: open.book.made(), languages, pythons: self.pythons_offered() })
    }

    /// The Pythons coding problems could be run with, the chosen one marked.
    fn pythons_offered(&self) -> Vec<proton_study::code::StudyPython> {
        let found = code::pythons().unwrap_or_default();
        let chosen = match &self.code_python {
            Some(path) => found.iter().position(|p| &p.path == path),
            None => found.iter().position(|p| p.default),
        };
        found
            .iter()
            .enumerate()
            .map(|(at, p)| proton_study::code::StudyPython {
                path: p.path.display().to_string(),
                // A framework build's prefix is a long way from anything a
                // person would recognise; its interpreter's path is not.
                name: folder::tilde(if p.prefix.to_string_lossy().contains(".framework") { &p.path } else { &p.prefix }),
                version: p.version.clone(),
                libraries: p.libraries.clone(),
                chosen: Some(at) == chosen,
            })
            .collect()
    }

    /// The Python coding problems are made for now: the one chosen, or the
    /// first on the login shell's PATH — asked again what it has, so a
    /// library installed since is offered.
    fn python_now(&self) -> Option<code::FoundPython> {
        let path = self.code_python.clone().filter(|p| p.is_file()).or_else(|| code::default_python())?;
        let python = code::probe(&path)?;
        code::remember(&python);
        Some(python)
    }

    /// Choose the Python coding problems are run with: a path to one, or
    /// nothing for the first on the login shell's PATH.
    fn choose_python(&mut self, typed: &str) -> Vec<Event> {
        let typed = typed.trim();
        if typed.is_empty() {
            self.code_python = None;
            let mut events: Vec<Event> = self.save_setting("study.code.python", Value::Null).into_iter().collect();
            events.extend(self.studio_event());
            return events;
        }
        let mut path = folder::expand(typed);
        // An environment's folder means its Python.
        if path.is_dir() {
            if let Some(python) = ["bin/python3", "bin/python", "python.exe"].iter().map(|p| path.join(p)).find(|p| p.is_file()) {
                path = python;
            }
        }
        let Some(python) = code::probe(&path) else {
            return vec![notice(format!("{} is not a Python that Proton could run.", folder::tilde(&path)), NoticeLevel::Warn)];
        };
        code::remember(&python);
        self.code_python = Some(path.clone());
        let mut events: Vec<Event> = self.save_setting("study.code.python", Value::from(path.display().to_string())).into_iter().collect();
        events.extend(self.studio_event());
        events
    }

    /// A set of coding problems, as the student has it.
    fn coding_event(&self, made: &str) -> Option<Event> {
        let open = self.open.as_ref()?;
        let Some((_, proton_study::studio::StudyMadeBody::Coding { problems, .. })) = open.book.made_one(made) else { return None };
        let running = self.runs.iter().filter(|r| r.dir == open.book.dir() && r.made == made).map(|r| r.problem.clone()).collect();
        Some(Event::StudyCoding { made: made.to_string(), saved: open.book.code_state(made, &problems), running })
    }

    /// The problem, of a set in the notebook open, and the Python the set
    /// was made for.
    fn coding_problem(&self, made: &str, problem: &str) -> Result<(proton_study::code::StudyCodingProblem, Option<PathBuf>), Vec<Event>> {
        let open = self.open.as_ref().ok_or_else(|| vec![notice("Open a notebook first.".into(), NoticeLevel::Warn)])?;
        match open.book.made_one(made) {
            Some((_, proton_study::studio::StudyMadeBody::Coding { problems, python, .. })) => problems
                .into_iter()
                .find(|p| p.id == problem)
                .map(|p| (p, python.map(|py| PathBuf::from(py.path))))
                .ok_or_else(|| vec![notice(format!("{made} has no problem {problem}"), NoticeLevel::Warn)]),
            _ => Err(vec![notice(format!("{made}: no such set of coding problems"), NoticeLevel::Warn)]),
        }
    }

    /// Keep what has been written for a problem.
    fn code_save(&mut self, made: &str, problem: &str, code: &str) -> Vec<Event> {
        let found = match self.coding_problem(made, problem) {
            Ok((found, _)) => found,
            Err(events) => return events,
        };
        let language = proton_study::code::Language::from_key(&found.language).unwrap_or(proton_study::code::Language::Python);
        match self.writable() {
            Ok(book) => match book.save_code(made, problem, language, code) {
                Ok(()) => Vec::new(),
                Err(why) => vec![notice(why, NoticeLevel::Error)],
            },
            Err(events) => events,
        }
    }

    /// Save what has been written, then build it and run its tests on a
    /// worker: its outcome comes on a later beat.
    fn code_run(&mut self, made: &str, problem: &str, code: &str) -> Vec<Event> {
        let (found, python) = match self.coding_problem(made, problem) {
            Ok(found) => found,
            Err(events) => return events,
        };
        // The set's own Python, which has the libraries it was made for;
        // the one chosen now, if that has gone.
        let python = python.filter(|p| p.is_file()).or_else(|| self.code_python.clone());
        // Not run unless kept: what is run is what was written. The window,
        // which shows the run under way as it asks, is told it is not.
        let mut refused = self.code_save(made, problem, code);
        if !refused.is_empty() {
            refused.extend(self.coding_event(made));
            return refused;
        }
        let Some(open) = &self.open else { return Vec::new() };
        let dir = open.book.dir().to_path_buf();
        if self.runs.iter().any(|r| r.dir == dir && r.made == made && r.problem == problem) {
            return Vec::new();
        }
        let at = dir.join(".notebook").join("code").join(made).join(problem);
        let (tx, rx) = std::sync::mpsc::channel();
        let code = code.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(code::run(&at, &found, &code, &code::tools_with(python.as_deref())));
        });
        self.runs.push(CodeRun { dir, made: made.to_string(), problem: problem.to_string(), outcome: rx });
        self.coding_event(made).into_iter().collect()
    }

    /// What runs have come to: each logged, and said.
    fn poll_code(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let found = code::generation();
        if found != self.told {
            self.told = found;
            events.extend(self.studio_event());
        }
        let mut finished = Vec::new();
        self.runs.retain(|run| match run.outcome.try_recv() {
            Ok(outcome) => {
                finished.push((run.dir.clone(), run.made.clone(), run.problem.clone(), outcome));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for (dir, made, problem, outcome) in finished {
            if let Ok(book) = Book::read(&dir) {
                let _ = book.log_code_run(&made, &problem, &outcome);
            }
            if self.open.as_ref().is_some_and(|o| o.book.dir() == dir) {
                events.extend(self.coding_event(&made));
                events.push(Event::StudyCodeRan { made, problem, outcome });
            }
        }
        events
    }

    fn make(&mut self, kind: proton_study::studio::Make, mut options: proton_study::studio::Options) -> Vec<Event> {
        if kind == proton_study::studio::Make::Coding {
            if proton_lab::sandbox::available() == proton_lab::sandbox::Kind::None {
                return vec![notice("Proton runs code only in a sandbox, and this machine has none: on Linux, install bubblewrap.".into(), NoticeLevel::Warn)];
            }
            let python = self.python_now();
            let here = code::tools_with(python.as_ref().map(|p| p.path.as_path())).languages();
            if here.is_empty() {
                return vec![notice("No compiler or interpreter Proton can run was found: install Python, a C compiler, Java, Node or Rust.".into(), NoticeLevel::Warn)];
            }
            if let Some(wanted) = proton_study::code::Language::from_key(&options.code) {
                if !here.contains(&wanted) {
                    return vec![notice(format!("{} cannot be run here: its compiler was not found.", wanted.name()), NoticeLevel::Warn)];
                }
            }
            options.languages = here.iter().map(|l| l.key().to_string()).collect();
            options.python = python.filter(|_| here.contains(&proton_study::code::Language::Python)).map(|p| p.used());
        }
        if kind == proton_study::studio::Make::Table && options.columns.iter().all(|c| c.trim().is_empty()) {
            return vec![notice("Name the table's columns.".into(), NoticeLevel::Warn)];
        }
        if kind == proton_study::studio::Make::Report && options.ask.trim().is_empty() {
            return vec![notice("Say what the report is to be.".into(), NoticeLevel::Warn)];
        }
        if kind == proton_study::studio::Make::Marking {
            return vec![notice("A mark is asked for by answering.".into(), NoticeLevel::Warn)];
        }
        let read = |o: &Open| o.book.sources.sources.iter().filter(|s| s.status.has_words()).count();
        if kind == proton_study::studio::Make::SourceGuide {
            let source = options.source.as_deref().and_then(|id| self.open.as_ref()?.book.sources.get(id));
            if !source.is_some_and(|s| s.status.has_words()) {
                return vec![notice("Choose a source that has been read.".into(), NoticeLevel::Warn)];
            }
        }
        if kind == proton_study::studio::Make::Labels && self.open.as_ref().map_or(0, read) < 2 {
            return vec![notice("Sources are labelled once two of them have been read.".into(), NoticeLevel::Info)];
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
        if id.starts_with('K') {
            let mut events = self.show_made_generic(id);
            events.extend(self.coding_event(id));
            return events;
        }
        if id.starts_with('D') {
            return match self.deck_event(id) {
                Some(event) => vec![event],
                None => vec![notice(format!("{id}: nothing made has that id"), NoticeLevel::Warn)],
            };
        }
        self.show_made_generic(id)
    }

    /// Something made, whole, its citations resolved.
    fn show_made_generic(&mut self, id: &str) -> Vec<Event> {
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
            StudyMadeBody::Coding { problems, .. } => problems.iter().map(|p| format!("{}\n{}", p.prompt, p.cites.iter().map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("\n"),
            StudyMadeBody::Circuits { problems, .. } => problems.iter().map(|p| format!("{}\n{}", p.statement, p.cites.iter().map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("\n"),
            StudyMadeBody::Logic { problems, .. } => problems.iter().map(|p| format!("{}\n{}", p.statement, p.cites.iter().map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("\n"),
            // What the web gave cites the web, by number, not the notebook;
            // a guide and labels are shown with their sources.
            StudyMadeBody::Found { .. } | StudyMadeBody::Guide { .. } | StudyMadeBody::Labels { .. } | StudyMadeBody::Pages { .. } | StudyMadeBody::Digest { .. } => String::new(),
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
            Origin::Made { file } => Some(open.book.files_dir().join(file)),
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
                SourceStatus::Untranscribed { .. } => "It is a recording, not transcribed yet.".to_string(),
                SourceStatus::Transcribing => "It is being transcribed.".to_string(),
                _ => "Its words are not kept here.".to_string(),
            };
            return vec![notice(format!("{}: {why}", source.title), NoticeLevel::Info)];
        };
        let gone = matches!(source.origin, Origin::File { .. } | Origin::Made { .. }) && meta.is_none();
        let what = if matches!(source.origin, Origin::Folder { .. }) { "folder" } else { "file" };
        let said = match (&source.status, in_cloud) {
            _ if gone => Some("Its file is gone: these are its words as they were read.".to_string()),
            (SourceStatus::Missing, _) => Some(format!("Its {what} is gone: these are its words as they were read.")),
            (SourceStatus::Changed, _) => Some(format!("Its {what} has changed since it was read: these are its words as they were.")),
            (_, true) => Some("Its file is in iCloud: these are its words as they were read.".to_string()),
            _ => None,
        };
        let view = match body {
            Body::Lines(text) => match Kind::from_name(&source.kind) {
                Kind::Markdown | Kind::Notebook | Kind::Document | Kind::Html | Kind::Web | Kind::Artifact => StudyView::Sections { sections: proton_study::view::sections(&text) },
                _ => StudyView::Lines { text },
            },
            Body::Pages(pages) => StudyView::Pages { pages },
            Body::Cues(cues) => StudyView::Cues { cues: cues.into_iter().map(|c| StudyCue { start: c.start, end: c.end, text: c.text }).collect() },
        };
        let guide = open.book.guide(id);
        let citations = guide.as_ref().map(|g| proton_study::cite::resolve(&open.book, &g.summary)).unwrap_or_default();
        let digest = proton_study::digest::view(&open.book, id);
        let digest_citations = digest.as_ref().map(|d| proton_study::cite::resolve(&open.book, &d.sections.iter().map(|s| format!("[[{}]] {}", s.at, s.summary)).collect::<Vec<_>>().join("\n"))).unwrap_or_default();
        vec![Event::StudySource { id: id.to_string(), title: source.title.clone(), kind: source.kind.clone(), at, said, view, guide, citations, digest, digest_citations }]
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
        let note = match self.put_note(id, title, body, None) {
            Ok(note) => note,
            Err(events) => return events,
        };
        let mut events = self.saved();
        events.extend(self.note_event(&note.id));
        events
    }

    /// A note written: new — given no id, or one no note has — or written
    /// again, its source's words kept again if it is one. Not yet saved.
    fn put_note(&mut self, id: Option<String>, title: &str, body: &str, from: Option<String>) -> Result<Note, Vec<Event>> {
        let book = self.writable()?;
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
            None => Note { id: book.new_note_id(), title, created: now, updated: now, from, body: body.trim_end().to_string() },
        };
        if let Err(why) = book.write_note(&note) {
            return Err(vec![notice(why, NoticeLevel::Error)]);
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
        Ok(note)
    }

    /// Keep the notes a tutor's answer wrote, and say on its turn which they
    /// are: one it names that is not there is made new.
    fn keep_tutor_notes(&mut self, drafts: Vec<proton_study::tutor::NoteDraft>) -> Vec<Event> {
        let Some(chat) = self.tutor.as_ref().map(|t| t.chat.id.clone()) else { return Vec::new() };
        let mut kept = Vec::new();
        let mut files = Vec::new();
        let mut events = Vec::new();
        for draft in drafts {
            if let Some(name) = draft.file.clone() {
                let written = match self.writable() {
                    Ok(book) => book.write_file(&name, &draft.body),
                    Err(why) => {
                        events.extend(why);
                        continue;
                    }
                };
                match written {
                    Ok(_) => files.push(proton_study::tutor::StudyTurnFile { name, pdf: false }),
                    Err(why) => events.push(notice(why, NoticeLevel::Error)),
                }
                continue;
            }
            let before = draft.id.as_deref().and_then(|id| self.open.as_ref()?.book.note(id)).is_some();
            match self.put_note(draft.id, &draft.title, &draft.body, Some(chat.clone())) {
                Ok(note) => kept.push(proton_study::tutor::StudyTurnNote { id: note.id, title: note.title, updated: before }),
                Err(why) => events.extend(why),
            }
        }
        if kept.is_empty() && files.is_empty() {
            return events;
        }
        if !kept.is_empty() {
            events.extend(self.saved());
            let said = match kept.as_slice() {
                [one] => format!("{} the note “{}” ({}).", if one.updated { "Wrote again" } else { "Kept" }, one.title, one.id),
                many => format!("Kept {} notes: {}.", many.len(), many.iter().map(|n| format!("“{}”", n.title)).collect::<Vec<_>>().join(", ")),
            };
            events.push(notice(said, NoticeLevel::Info));
        }
        if !files.is_empty() {
            let names = files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ");
            events.push(notice(format!("Kept {names} in the notebook's files/."), NoticeLevel::Info));
        }
        if let (Some(open), Some(tutor)) = (&self.open, &mut self.tutor) {
            if let Some(turn) = tutor.chat.turns.last_mut() {
                turn.notes = kept;
                turn.files = files;
            }
            if let Err(why) = open.book.write_chat(&tutor.chat) {
                events.push(notice(format!("The chat could not be kept: {why}"), NoticeLevel::Error));
            }
        }
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

    /// A source's pages with few words, counted again once some were read.
    fn thin_again(&mut self, id: &str) -> bool {
        let Some(open) = &mut self.open else { return false };
        let Some(Body::Pages(pages)) = open.book.sources.get(id).and_then(|s| snapshot::words(&open.book, id, s.units)) else { return false };
        let Some(source) = open.book.sources.get_mut(id) else { return false };
        source.thin = proton_library::pdf::thin(&pages);
        if open.lock.is_some() {
            let _ = open.book.save_sources();
        }
        true
    }

    /// Note the open notebook's sources whose files have gone or changed
    /// since they were read, by their stamps alone.
    fn check_files(&mut self) {
        let Some(open) = &mut self.open else { return };
        let mut changed = false;
        let files = open.book.files_dir();
        for source in &mut open.book.sources.sources {
            if !source.status.has_words() {
                continue;
            }
            let now = match &source.origin {
                Origin::File { .. } | Origin::Made { .. } => match match &source.origin {
                    Origin::Made { file } => Some(files.join(file)),
                    Origin::File { root, rel } => self.kb.file(root, rel),
                    _ => None,
                }
                .and_then(|p| std::fs::symlink_metadata(p).ok())
                {
                    None => SourceStatus::Missing,
                    Some(meta) => {
                        let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                        match (meta.len(), mtime) == (source.size, source.mtime) {
                            true => SourceStatus::Ready,
                            false => SourceStatus::Changed,
                        }
                    }
                },
                // A folder by its files' sizes together and its newest change.
                Origin::Folder { .. } => match folder_at(&self.kb, &source.origin) {
                    Some((dir, scope)) if dir.is_dir() => match proton_library::folder::stamp(&dir, &scope) {
                        Ok(stamp) if (stamp.size, stamp.mtime) == (source.size, source.mtime) => SourceStatus::Ready,
                        Ok(_) => SourceStatus::Changed,
                        Err(_) => continue,
                    },
                    _ => SourceStatus::Missing,
                },
                _ => continue,
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
