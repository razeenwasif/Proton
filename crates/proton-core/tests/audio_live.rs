//! An audio overview, live: a real engine writing a short script of a
//! notebook's one source, and Fish Audio speaking it in two voices, joined by
//! ffmpeg. It spends a little of the engine's plan and of the Fish account's
//! credit — and on Fish's free tier, what it is sent may be used to improve
//! its models — so it is run by hand, with the key in the shell and two
//! voices named:
//!
//! ```text
//! FISH_API_KEY=… PROTON_FISH_VOICES=<id>,<id> \
//! cargo test -p proton-core --test audio_live -- --ignored --nocapture
//! ```
//!
//! The voices' ids are on their pages at fish.audio. Running it is agreeing
//! to Fish's terms for this run's script, which is written to a scratch
//! notebook's settings, not the person's own.

use std::time::{Duration, Instant};

use proton_core::bus::{Command, Event};
use proton_core::mode::ModeId;
use proton_core::session::Session;
use proton_study::audio::AudioFormat;
use proton_study::model::SourceStatus;
use proton_study::studio::{Amount, Make, Options};
use proton_study::view::StudyHost;

fn until<T>(session: &mut Session, within: Duration, mut want: impl FnMut(&Event) -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        for event in session.apply(Command::Poll) {
            if let Some(found) = want(&event) {
                return Some(found);
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

#[test]
#[ignore = "asks an engine for a script and Fish Audio to speak it, which spends plan quota and Fish credit"]
fn a_short_brief_is_written_spoken_and_joined() {
    let voices: Vec<String> = std::env::var("PROTON_FISH_VOICES").expect("name two voices in PROTON_FISH_VOICES").split(',').map(|v| v.trim().to_string()).collect();
    assert_eq!(voices.len(), 2, "two voices");
    let root = std::env::temp_dir().join(format!("proton-audio-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let notes = root.join("kb/Notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("search.md"),
        "# Uniform-cost search\n\nIt expands the frontier node with the lowest path cost g(n). In this course's convention it is optimal only when \
         every step costs at least 0.25: then the first goal it expands is a cheapest one.\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("Notebooks")).unwrap();
    let settings = root.join("settings.json");
    std::fs::write(&settings, serde_json::json!({"study": {"roots": [notes], "folder": root.join("Notebooks")}}).to_string()).unwrap();
    let mut session = Session::new(&root).with_person_settings(settings);
    session.apply(Command::SetMode { mode: ModeId::Study });
    until(&mut session, Duration::from_secs(30), |e| matches!(e, Event::StudyKb { ready: true, walking: false, .. }).then_some(())).expect("the knowledge base");
    session.apply(Command::StudyNew { title: "Audio, live".into() });
    session.apply(Command::StudyAdd { paths: vec!["Notes/search.md".into()] });
    until(&mut session, Duration::from_secs(30), |e| match e {
        Event::StudyNotebook { sources, .. } if sources.iter().any(|s| s.status == SourceStatus::Ready) => Some(()),
        _ => None,
    })
    .expect("the source read");
    let hosts = voices.iter().enumerate().map(|(i, id)| StudyHost { id: id.clone(), title: format!("Voice {}", i + 1) }).collect();
    session.apply(Command::StudyVoiceSetup { consent: true, hosts });
    session.apply(Command::StudyMake { kind: Make::Audio, options: Options { format: Some(AudioFormat::Brief), amount: Amount::Fewer, ..Options::default() } });
    let job = until(&mut session, Duration::from_secs(16 * 60), |e| match e {
        Event::StudyStudio { jobs, .. } => jobs.iter().find(|j| j.kind == Make::Audio && j.id.starts_with('J') && matches!(j.state.as_str(), "done" | "failed")).cloned(),
        _ => None,
    })
    .expect("a script within sixteen minutes");
    assert_eq!(job.state, "done", "{:?}", job.why);
    // Looked at, it is drawn again as it is spoken.
    session.apply(Command::StudyShowMade { id: "A001".into() });
    let ready = until(&mut session, Duration::from_secs(25 * 60), |e| match e {
        Event::StudyAudio { state, why, url, duration, timings, .. } if state == "ready" || state == "failed" => Some((state.clone(), why.clone(), url.clone(), *duration, timings.len())),
        _ => None,
    });
    let (state, why, url, duration, timings) = ready.expect("the overview");
    println!("\n{state}: {why:?}, {url:?}, {duration:.1} s, {timings} turns timed");
    assert_eq!(state, "ready", "{why:?}");
    assert!(duration > 30.0, "a brief runs some minutes, not {duration} s");
    let file = root.join("Notebooks/audio-live/.notebook/audio/A001.mp3");
    assert!(file.metadata().map(|m| m.len()).unwrap_or(0) > 100_000, "the sound is there");
    session.shutdown();
}
