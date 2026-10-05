//! One real round, on the real engines: Claude Code, Codex and Antigravity,
//! signed in as the person running it. It spends their limits, so it never
//! runs unless asked for:
//!
//! ```text
//! PROTON_LIVE_ROOT=/some/folder cargo test -p proton-core --test live_round -- --ignored --nocapture
//! ```
//!
//! Run it from a clean environment — `env -i HOME=… PATH=… LANG=…` — as
//! Proton itself is started: a shell inside another agent's session carries
//! that session's variables, and the engines read them.
//!
//! It makes an investigation under `$PROTON_LIVE_ROOT/research/`, runs one
//! round to its end, says each call as it starts and ends, and leaves the
//! folder to be read. What it checks is what a round must do whatever the
//! models say: it finished, within its cap, with a report, and no model
//! family reviewed or judged its own hypotheses.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_core::bus::Event;
use proton_core::research::Research;
use proton_research::model::Discipline;
use proton_research::store::Store;
use proton_research::supervisor::RoundStatus;

#[test]
#[ignore = "runs the real engines and spends their limits"]
fn a_real_round_on_the_real_engines() {
    let root = std::env::var_os("PROTON_LIVE_ROOT").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("proton-live-round"));
    std::fs::create_dir_all(&root).unwrap();
    let mut research = Research::new(&root);
    let title = std::env::var("PROTON_LIVE_TITLE").unwrap_or_else(|_| "Why is the daytime sky blue rather than violet?".into());
    let goal = std::env::var("PROTON_LIVE_GOAL").unwrap_or_else(|_| {
        "Rayleigh scattering favours the shortest wavelengths, so why is the clear daytime sky seen as blue rather than \
         violet? A good answer is quantitative: it predicts the colour a spectroradiometer and a colour-matching observer \
         would report."
            .into()
    });
    let discipline = match std::env::var("PROTON_LIVE_DISCIPLINE").as_deref() {
        Ok("mathematics") => Discipline::Mathematics,
        _ => Discipline::Physics,
    };
    research.create(&title, &goal, discipline);
    let started = Instant::now();
    let say = |what: String| println!("[{:>5.0}s] {what}", started.elapsed().as_secs_f64());

    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut last = None;
    let mut watch = |events: Vec<Event>, seen: &mut BTreeMap<String, String>| {
        for event in events {
            match event {
                Event::Notice { text, .. } => say(format!("notice: {text}")),
                Event::ResearchRound { status, used, cap, tasks, .. } => {
                    for t in &tasks {
                        let state = format!("{}{}", t.state, t.why.as_deref().map(|w| format!(" ({w})")).unwrap_or_default());
                        if seen.get(&t.id) != Some(&state) {
                            let about = if t.targets.is_empty() { String::new() } else { format!(" on {}", t.targets.join(", ")) };
                            say(format!("{} {:?} by {:?}{about} [{}]: {state}", t.id, t.phase, t.seat, t.model));
                            seen.insert(t.id.clone(), state);
                        }
                    }
                    last = Some((status, used, cap));
                }
                _ => {}
            }
        }
    };

    watch(research.start(), &mut seen);
    let deadline = started + Duration::from_secs(60 * 60);
    while research.is_busy() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        watch(research.poll(), &mut seen);
    }
    say(format!("over: {last:?}"));

    let store = Store::new(root.join("research"));
    let folder = store.list().into_iter().next().expect("the investigation");
    let (status, used, cap) = last.expect("the round was never drawn");
    assert_eq!(status, RoundStatus::Idle, "the round did not finish within the hour");
    assert!(used <= cap, "{used} calls of {cap}");
    assert!(folder.dir().join("rounds/round-001.md").exists(), "no round report");

    let hypotheses = folder.hypotheses();
    let family = |id: &str| hypotheses.iter().find(|h| h.id == id).map(|h| h.author.family());
    for r in folder.reviews() {
        assert_ne!(Some(r.reviewer.family()), family(&r.hypothesis), "{} reviewed its own family's {}", r.reviewer.seat.name(), r.hypothesis);
    }
    for m in folder.matches() {
        assert!(Some(m.judge.family()) != family(&m.a) && Some(m.judge.family()) != family(&m.b), "{m:?}");
    }
    println!("\n{}", std::fs::read_to_string(folder.dir().join("rounds/round-001.md")).unwrap_or_default());
    println!("{} hypotheses, {} reviews, {} matches, {} experiments proposed", hypotheses.len(), folder.reviews().len(), folder.matches().len(), folder.experiments().len());
    println!("the investigation is in {}", folder.dir().display());
}
