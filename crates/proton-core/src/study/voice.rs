//! Audio overviews, spoken: a script made by the studio ([`proton_study::audio`])
//! voiced by Fish Audio a piece at a time on a worker, the pieces joined by
//! ffmpeg, and each turn timed so the transcript follows the voice.
//!
//! Nothing is sent to Fish until the person has agreed to its free tier's
//! terms — free until the end of November 2026, and what it is sent may be
//! used to improve its models — and chosen the voices. Its key is read from
//! `FISH_API_KEY` (or `FISH_AUDIO_API_KEY`): Proton's environment, then the
//! login shell's; it is held in memory only, and the page is told only
//! whether it was found. A piece spoken is kept until the whole is joined, so
//! an overview that stopped part way — out of credit, Fish busy, Proton quit
//! — is taken up again where it stopped, not paid for twice.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use proton_study::audio::{self, Audio};
use proton_study::view::{StudyHost, StudyVoice, StudyVoiceState};
use proton_study::Book;
use proton_voice::fish::{self, Fish, FishError, Key, Speech, Voice, VoiceQuery};
use serde_json::Value;

use super::media;
use super::Study;
use crate::bus::{Event, NoticeLevel};
use crate::study::jobs::StudyJob;
use crate::workspace::notice;

/// The names Fish's key may go by.
const KEY_NAMES: [&str; 2] = ["FISH_API_KEY", "FISH_AUDIO_API_KEY"];

/// What speaks: Fish in the app, a script in a test.
pub trait Speaker: Send + Sync {
    fn speak(&self, speech: &Speech, out: &Path, cancel: &AtomicBool) -> Result<u64, FishError>;
    fn voices(&self, query: &VoiceQuery, cancel: &AtomicBool) -> Result<(u64, Vec<Voice>), FishError>;
    fn sample(&self, url: &str, out: &Path, cancel: &AtomicBool) -> Result<u64, FishError>;
}

impl Speaker for Fish {
    fn speak(&self, speech: &Speech, out: &Path, cancel: &AtomicBool) -> Result<u64, FishError> {
        Fish::speak(self, speech, out, cancel)
    }
    fn voices(&self, query: &VoiceQuery, cancel: &AtomicBool) -> Result<(u64, Vec<Voice>), FishError> {
        Fish::voices(self, query, cancel)
    }
    fn sample(&self, url: &str, out: &Path, cancel: &AtomicBool) -> Result<u64, FishError> {
        Fish::sample(self, url, out, cancel)
    }
}

/// What measures and joins sound: ffmpeg in the app, a script in a test.
pub trait Media: Send + Sync {
    /// How long a file plays, in seconds.
    fn duration(&self, file: &Path) -> Result<f64, String>;
    /// The parts, one after another, as one file.
    fn join(&self, parts: &[PathBuf], out: &Path) -> Result<(), String>;
}

/// ffmpeg and ffprobe, as installed.
pub struct Ffmpeg;

impl Media for Ffmpeg {
    fn duration(&self, file: &Path) -> Result<f64, String> {
        let out = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
            .arg(file)
            .output()
            .map_err(|e| format!("ffprobe could not be run: {e}"))?;
        String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().map_err(|_| format!("ffprobe could not say how long {} is", file.display()))
    }

    fn join(&self, parts: &[PathBuf], out: &Path) -> Result<(), String> {
        let list = out.with_extension("txt");
        let text: String = parts.iter().map(|p| format!("file '{}'\n", p.display().to_string().replace('\'', "'\\''"))).collect();
        std::fs::write(&list, text).map_err(|e| e.to_string())?;
        let part = out.with_extension("joining.mp3");
        let status = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "concat", "-safe", "0", "-i"])
            .arg(&list)
            .args(["-c", "copy"])
            .arg(&part)
            .status()
            .map_err(|e| format!("ffmpeg could not be run: {e}"))?;
        let _ = std::fs::remove_file(&list);
        if !status.success() {
            let _ = std::fs::remove_file(&part);
            return Err("ffmpeg could not join the pieces".into());
        }
        std::fs::rename(&part, out).map_err(|e| e.to_string())
    }
}

/// What makes the speaker and the media: the real ones, or a test's.
pub trait VoiceKit: Send {
    /// Fish, with its key; `Err` says why not.
    fn speaker(&mut self) -> Result<Arc<dyn Speaker>, String>;
    fn media(&self) -> Arc<dyn Media>;
    /// Look for the key again, as when it has just been set.
    fn forget(&mut self) {}
}

/// Fish with the key from the environment, ffmpeg as installed.
#[derive(Default)]
pub struct LiveVoices {
    /// The key once looked for: whether it was found.
    found: Option<Option<Key>>,
}

impl VoiceKit for LiveVoices {
    fn speaker(&mut self) -> Result<Arc<dyn Speaker>, String> {
        let found = self.found.get_or_insert_with(|| {
            let own = KEY_NAMES.iter().find_map(|name| std::env::var(name).ok().and_then(|v| Key::new(&v)));
            own.or_else(|| {
                let vars = proton_claude::process::login_vars(&KEY_NAMES);
                KEY_NAMES.iter().find_map(|name| vars.get(*name).and_then(|v| Key::new(v)))
            })
        });
        match found {
            Some(key) => Ok(Arc::new(Fish::new(key.clone(), Arc::new(fish::Ureq::default())))),
            None => Err("Fish Audio's key is not set: export FISH_API_KEY in your shell (from fish.audio/app/api-keys), then look again.".into()),
        }
    }
    fn media(&self) -> Arc<dyn Media> {
        Arc::new(Ffmpeg)
    }
    fn forget(&mut self) {
        self.found = None;
    }
}

/// What a voicing worker says.
enum Step {
    Spoken { done: usize, of: usize },
    Joined { durations: Vec<f64> },
    Failed(String),
}

/// An overview being spoken.
pub(super) struct Voicing {
    dir: PathBuf,
    id: String,
    title: String,
    rx: Receiver<Step>,
    cancel: Arc<AtomicBool>,
    done: usize,
    of: usize,
    started: u64,
}

/// A worker's errand to Fish: the voices found, or a sample fetched.
pub(super) enum Errand {
    Voices { query: String, rx: Receiver<Result<(u64, Vec<Voice>), String>> },
    Sample { id: String, rx: Receiver<Result<(), String>> },
}

impl Voicing {
    /// Stop it: the worker stops before its next piece.
    pub(super) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Study {
    fn voice_settings(&self) -> (Option<String>, Vec<StudyHost>) {
        let Some(settings) = self.settings() else { return (None, Vec::new()) };
        let consent = settings.get("study.voice.consent").and_then(Value::as_str).map(str::to_string);
        let hosts = settings.get("study.voice.hosts").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        (consent, hosts)
    }

    /// Where audio overviews stand.
    pub(super) fn voice_state(&mut self) -> StudyVoiceState {
        let (consented, hosts) = self.voice_settings();
        StudyVoiceState { key: self.voice.speaker().is_ok(), consented, hosts }
    }

    /// Agree to Fish's terms, or not, and choose the voices: kept in the
    /// person's settings. Overviews waiting on them are spoken.
    pub(super) fn voice_setup(&mut self, consent: bool, hosts: Vec<StudyHost>) -> Vec<Event> {
        let today = date_today((self.clock)());
        let mut events: Vec<Event> = Vec::new();
        let consent = match consent {
            true => Value::from(self.voice_settings().0.unwrap_or(today)),
            false => Value::Null,
        };
        events.extend(self.save_setting("study.voice.consent", consent));
        let hosts: Vec<StudyHost> = hosts.into_iter().filter(|h| !h.id.trim().is_empty()).take(2).collect();
        events.extend(self.save_setting("study.voice.hosts", serde_json::to_value(&hosts).unwrap_or_default()));
        self.voice.forget();
        events.extend(self.voice_waiting());
        events.extend(self.voice_event());
        events
    }

    /// The voice state, for whatever of it is shown.
    fn voice_event(&mut self) -> Option<Event> {
        let voice = self.voice_state();
        Some(Event::StudyVoiceState { voice })
    }

    /// Speak an overview: on a worker, a piece at a time, each kept until
    /// all are joined.
    pub(super) fn voice_audio(&mut self, id: &str) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book.clone(),
            Err(events) => return events,
        };
        if self.voicing.iter().any(|v| v.id == id && v.dir == book.dir()) {
            return Vec::new();
        }
        let Some(mut kept) = book.audio(id) else { return vec![notice(format!("{id}: no such audio overview"), NoticeLevel::Warn)] };
        let (consent, hosts) = self.voice_settings();
        let speakers = kept.format.speakers();
        let blocked = if consent.is_none() {
            Some("Waiting for you to agree to Fish Audio's terms.".to_string())
        } else if hosts.len() < speakers {
            Some(match speakers {
                1 => "Waiting for you to choose a voice.".to_string(),
                _ => "Waiting for you to choose two voices.".to_string(),
            })
        } else {
            self.voice.speaker().err()
        };
        if let Some(why) = blocked {
            kept.state = "waiting".into();
            kept.why = Some(why);
            let _ = book.save_audio(id, &kept);
            return self.audio_shown(id);
        }
        let speaker = match self.voice.speaker() {
            Ok(speaker) => speaker,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let media = self.voice.media();
        let voices: Vec<String> = hosts.iter().take(speakers).map(|h| h.id.clone()).collect();
        let chunks = audio::chunks(&kept.segments, speakers);
        kept.state = "voicing".into();
        kept.why = None;
        kept.voices = voices.clone();
        kept.done = 0;
        kept.of = chunks.len();
        if let Err(why) = book.save_audio(id, &kept) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let parts = book.audio_parts(id);
        let out = book.audio_file(id);
        std::thread::spawn(move || {
            // Nothing in iCloud is brought down by a voicing.
            let _quiet = proton_library::dataless::NoDownloads::on_this_thread();
            let failed = |why: String| Step::Failed(why);
            if let Err(e) = std::fs::create_dir_all(&parts) {
                let _ = tx.send(failed(e.to_string()));
                return;
            }
            let mut files = Vec::new();
            for (i, chunk) in chunks.iter().enumerate() {
                let file = parts.join(format!("{i:03}.mp3"));
                // Spoken already, before a stop: not paid for twice.
                if !file.metadata().is_ok_and(|m| m.len() > 0) {
                    let speech = Speech { text: chunk.text.clone(), voices: voices.clone(), speed: 1.0, bitrate: 128 };
                    if let Err(e) = speaker.speak(&speech, &file, &stop) {
                        let _ = tx.send(failed(e.describe()));
                        return;
                    }
                }
                files.push(file);
                let _ = tx.send(Step::Spoken { done: i + 1, of: chunks.len() });
            }
            let durations: Result<Vec<f64>, String> = files.iter().map(|f| media.duration(f)).collect();
            let durations = match durations {
                Ok(d) => d,
                Err(why) => {
                    let _ = tx.send(failed(why));
                    return;
                }
            };
            if let Err(why) = media.join(&files, &out) {
                let _ = tx.send(failed(why));
                return;
            }
            let _ = std::fs::remove_dir_all(&parts);
            let _ = tx.send(Step::Joined { durations });
        });
        self.voicing.push(Voicing { dir: book.dir().to_path_buf(), id: id.to_string(), title: kept.title.clone(), rx, cancel, done: 0, of: kept.of, started: proton_study::now() });
        let mut events = self.audio_shown(id);
        events.extend(self.studio_event());
        events
    }

    /// Stop speaking an overview: what was spoken is kept, to go on from.
    pub(super) fn cancel_voicing(&mut self, id: &str) -> bool {
        let Some(open) = &self.open else { return false };
        let dir = open.book.dir().to_path_buf();
        let Some(at) = self.voicing.iter().position(|v| v.id == id && v.dir == dir) else { return false };
        let voicing = self.voicing.remove(at);
        voicing.cancel.store(true, Ordering::Relaxed);
        if let Some(mut kept) = open.book.audio(id) {
            kept.state = "waiting".into();
            kept.why = Some("Stopped: what was spoken is kept, to go on from.".into());
            let _ = open.book.save_audio(id, &kept);
        }
        true
    }

    /// The overviews waiting to be spoken, spoken — those the setup allows.
    pub(super) fn voice_waiting(&mut self) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        if open.lock.is_none() {
            return Vec::new();
        }
        let waiting: Vec<String> = open.book.audio_ids().into_iter().filter(|id| open.book.audio(id).is_some_and(|a| a.state == "waiting")).collect();
        let mut events = Vec::new();
        for id in waiting {
            events.extend(self.voice_audio(&id));
        }
        events
    }

    /// The overviews being spoken, as the studio lists its calls.
    pub(super) fn voicing_rows(&self, dir: &Path) -> Vec<StudyJob> {
        self.voicing
            .iter()
            .filter(|v| v.dir == dir)
            .map(|v| StudyJob {
                id: v.id.clone(),
                kind: proton_study::studio::Make::Audio,
                title: format!("Voicing {}", v.title),
                state: "running".into(),
                why: Some(format!("{} of {} pieces spoken", v.done, v.of)),
                made: None,
                engine: "fish".into(),
                started: v.started,
            })
            .collect()
    }

    /// What the workers have said: pieces spoken, an overview joined, a
    /// voicing failed; voices found, a sample fetched.
    pub(super) fn poll_voices(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut changed = Vec::new();
        let mut i = 0;
        while i < self.voicing.len() {
            let mut ended = None;
            loop {
                match self.voicing[i].rx.try_recv() {
                    Ok(Step::Spoken { done, of }) => {
                        self.voicing[i].done = done;
                        self.voicing[i].of = of;
                        let v = &self.voicing[i];
                        if let Ok(book) = Book::read(&v.dir) {
                            if let Some(mut kept) = book.audio(&v.id) {
                                kept.done = done;
                                let _ = book.save_audio(&v.id, &kept);
                            }
                        }
                        changed.push(self.voicing[i].id.clone());
                    }
                    Ok(Step::Joined { durations }) => {
                        ended = Some(Ok(durations));
                        break;
                    }
                    Ok(Step::Failed(why)) => {
                        ended = Some(Err(why));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        ended = Some(Err("The voicing stopped.".into()));
                        break;
                    }
                }
            }
            let Some(outcome) = ended else {
                i += 1;
                continue;
            };
            let v = self.voicing.remove(i);
            changed.push(v.id.clone());
            let Ok(book) = Book::read(&v.dir) else { continue };
            let Some(mut kept) = book.audio(&v.id) else { continue };
            match outcome {
                Ok(durations) => {
                    let chunks = audio::chunks(&kept.segments, kept.format.speakers());
                    kept.timings = audio::timings(&kept.segments, &chunks, &durations);
                    kept.duration = durations.iter().sum();
                    kept.state = "ready".into();
                    kept.why = None;
                    kept.done = kept.of;
                    events.push(notice(format!("\"{}\" is ready to listen to.", kept.title), NoticeLevel::Info));
                }
                Err(why) => {
                    kept.state = "failed".into();
                    kept.why = Some(why);
                }
            }
            let _ = book.save_audio(&v.id, &kept);
        }
        if !changed.is_empty() {
            events.extend(self.studio_event());
            if let Some(shown) = self.audio.clone() {
                if changed.contains(&shown) {
                    events.extend(self.audio_shown(&shown));
                }
            }
        }
        // Errands.
        let mut kept = Vec::new();
        for errand in std::mem::take(&mut self.errands) {
            match errand {
                Errand::Voices { query, rx } => match rx.try_recv() {
                    Ok(found) => {
                        let (total, voices, error) = match found {
                            Ok((total, voices)) => (total, voices, None),
                            Err(why) => (0, Vec::new(), Some(why)),
                        };
                        self.found_voices = voices.clone();
                        let voices = voices.into_iter().map(|v| StudyVoice { id: v.id, title: v.title, description: v.description, languages: v.languages, sample: v.sample.is_some(), sample_text: v.sample_text, author: v.author, uses: v.uses }).collect();
                        events.push(Event::StudyVoices { query, voices, total, error });
                    }
                    Err(TryRecvError::Empty) => kept.push(Errand::Voices { query, rx }),
                    Err(TryRecvError::Disconnected) => {}
                },
                Errand::Sample { id, rx } => match rx.try_recv() {
                    Ok(Ok(())) => events.push(Event::StudyVoiceSample { id: id.clone(), url: Some(format!("_voices/{id}.mp3")), error: None }),
                    Ok(Err(why)) => events.push(Event::StudyVoiceSample { id, url: None, error: Some(why) }),
                    Err(TryRecvError::Empty) => kept.push(Errand::Sample { id, rx }),
                    Err(TryRecvError::Disconnected) => {}
                },
            }
        }
        self.errands = kept;
        events
    }

    /// Fish's voices, by name and language: found on a worker.
    pub(super) fn find_voices(&mut self, query: &str, language: &str) -> Vec<Event> {
        let speaker = match self.voice.speaker() {
            Ok(speaker) => speaker,
            Err(why) => return vec![Event::StudyVoices { query: query.to_string(), voices: Vec::new(), total: 0, error: Some(why) }],
        };
        let (tx, rx) = mpsc::channel();
        let asked = VoiceQuery { title: query.to_string(), language: language.to_string(), page: 1 };
        std::thread::spawn(move || {
            let _ = tx.send(speaker.voices(&asked, &AtomicBool::new(false)).map_err(|e| e.describe()));
        });
        self.errands.push(Errand::Voices { query: query.to_string(), rx });
        Vec::new()
    }

    /// A voice's sample, fetched to the cache to be listened to.
    pub(super) fn fetch_sample(&mut self, id: &str) -> Vec<Event> {
        let Some(dir) = media::voices_dir() else { return vec![notice("There is nowhere to keep voices' samples.".into(), NoticeLevel::Warn)] };
        let out = dir.join(format!("{id}.mp3"));
        if out.is_file() {
            return vec![Event::StudyVoiceSample { id: id.to_string(), url: Some(format!("_voices/{id}.mp3")), error: None }];
        }
        let Some(url) = self.found_voices.iter().find(|v| v.id == id).and_then(|v| v.sample.clone()) else {
            return vec![Event::StudyVoiceSample { id: id.to_string(), url: None, error: Some("That voice has no sample.".into()) }];
        };
        let speaker = match self.voice.speaker() {
            Ok(speaker) => speaker,
            Err(why) => return vec![Event::StudyVoiceSample { id: id.to_string(), url: None, error: Some(why) }],
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(speaker.sample(&url, &out, &AtomicBool::new(false)).map(|_| ()).map_err(|e| e.describe()));
        });
        self.errands.push(Errand::Sample { id: id.to_string(), rx });
        Vec::new()
    }

    /// An overview, as it stands; and it, the one looked at.
    pub(super) fn audio_shown(&mut self, id: &str) -> Vec<Event> {
        self.audio = Some(id.to_string());
        let voice = self.voice_state();
        let Some(open) = &self.open else { return Vec::new() };
        let book = &open.book;
        let (Some((made, _)), Some(kept)) = (book.made_one(id), book.audio(id)) else {
            return vec![notice(format!("{id}: no such audio overview"), NoticeLevel::Warn)];
        };
        let Audio { format, segments, chapters, timings, duration, state, why, done, of, .. } = kept;
        let url = (state == "ready" && book.audio_file(id).is_file()).then(|| format!("{}/{id}.mp3", book.slug()));
        let text: String = segments.iter().map(|s| format!("{}\n{}\n", s.display, s.cites.iter().map(|c| format!("[[{c}]]")).collect::<String>())).collect();
        let citations = proton_study::cite::resolve(book, &text);
        vec![Event::StudyAudio { made, format, segments, chapters, timings, duration, state, why, done, of, url, citations, voice }]
    }
}

/// `YYYY-MM-DD` of a moment, here.
fn date_today(at: i64) -> String {
    let local = at + proton_study::clock::offset(at);
    let days = local.div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_said_as_it_is_written() {
        let noon = proton_study::clock::midnight_utc("2026-10-06").unwrap() + 12 * 3600;
        assert_eq!(date_today(noon - proton_study::clock::offset(noon)), "2026-10-06");
        let leap = proton_study::clock::midnight_utc("2028-02-29").unwrap() + 12 * 3600;
        assert_eq!(date_today(leap - proton_study::clock::offset(leap)), "2028-02-29");
    }
}
