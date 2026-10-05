//! Rounds played with made-up answers.

use super::*;
use crate::model::Discipline;
use serde_json::json;

fn snapshot() -> Snapshot {
    let mut models = BTreeMap::new();
    models.insert(Seat::Claude, "claude-opus-5-5".to_string());
    models.insert(Seat::Codex, "gpt-6.1-sol".to_string());
    models.insert(Seat::Antigravity, "gemini-3.8-flash-high".to_string());
    Snapshot {
        investigation: Some(Investigation::new("Why is the sky blue?", Discipline::Physics, 0)),
        brief: "# Why is the sky blue?\n\n## Goal\n\nExplain the colour of the daytime sky.".into(),
        models,
        ..Snapshot::default()
    }
}

fn proposal(code: &str) -> Value {
    json!({ "purpose": "Integrate the spectrum.", "expectation": "Peak below 500 nm.", "code": code })
}

/// A made-up answer to a dispatched task, shaped by what it was shown.
fn answer(sup: &Supervisor, d: &Dispatch, n: usize) -> Value {
    let task = sup.task(&d.task).unwrap();
    let labels: Vec<String> = task.labels.keys().cloned().collect();
    let hypothesis = |i: usize| {
        json!({
            "title": format!("Idea {n}.{i} from {}", d.seat.name()),
            "statement": format!("Mechanism {n} {i} {} scatters light by a process unique to idea {n}{i}.", d.seat.name()),
            "rationale": "Because.", "predictions": ["A measurable thing."], "assumptions": [],
            "experiment": proposal(if i == 0 { "print(1)" } else { "" })
        })
    };
    match d.phase {
        Phase::Frame => json!({ "plan": "Test scattering laws.", "criteria": ["Quantitative"], "questions": ["Why blue not violet?"] }),
        Phase::Generate => json!({ "hypotheses": [hypothesis(0), hypothesis(1)] }),
        Phase::Reflect => json!({ "reviews": labels.iter().map(|l| json!({ "label": l, "verdict": "promising", "correctness": 4, "novelty": 3,
            "testability": 4, "summary": "Sound.", "critique": "Check the dipole approximation.", "citations": [], "experiment": proposal("") })).collect::<Vec<_>>() }),
        Phase::Verify => json!({ "label": "A", "verdict": "strong", "derivation": "$I \\propto \\lambda^{-4}$ holds.", "issues": [], "experiment": proposal("") }),
        Phase::Rank => json!({ "matches": task.pairs.iter().enumerate().map(|(k, p)| json!({ "pair": p.label, "winner": if (n + k) % 2 == 0 { "A" } else { "B" }, "rationale": "Sharper." })).collect::<Vec<_>>() }),
        Phase::Evolve => {
            let mut h = hypothesis(9);
            h["parents"] = json!(labels);
            json!({ "hypotheses": [h] })
        }
        Phase::Meta => json!({ "overview": "Rayleigh leads.", "plan": "Next: polarisation.", "guidance": ["Look at polarisation."], "retire": labels.iter().take(1).collect::<Vec<_>>() }),
        Phase::SimReview => json!({ "finding": "supports", "summary": "The spectrum peaks at 470 nm.", "issues": [], "follow_up": proposal("") }),
    }
}

/// What happens to a dispatched task in a test.
#[derive(Clone)]
enum Fate {
    Answer,
    Garbage,
    Fail(Failure),
}

/// Play the round to its end, deciding each task's fate with `fate`.
/// Returns every effect, and the snapshot as the files would have it.
fn play(sup: &mut Supervisor, mut snap: Snapshot, mut fate: impl FnMut(&Dispatch, usize) -> Fate) -> (Vec<Effect>, Snapshot) {
    let mut effects = Vec::new();
    let mut now = 1_000;
    for step in 0..400 {
        now += 30;
        let dispatches = sup.ready(&snap, now);
        if dispatches.is_empty() {
            let running: Vec<String> = sup.state().tasks.iter().filter(|t| matches!(t.state, TaskState::Running | TaskState::Repairing)).map(|t| t.id.clone()).collect();
            if running.is_empty() {
                // Cooling seats: let time pass.
                if sup.state().tasks.iter().any(|t| t.state == TaskState::Ready) && matches!(sup.state().status, RoundStatus::Running) {
                    now += COOLING;
                    continue;
                }
                break;
            }
        }
        for d in dispatches {
            let model = snap.models.get(&d.seat).cloned().unwrap_or_default();
            if let Some(why) = sup.started(&d.task, "native", &model, &snap) {
                effects.push(Effect::Notice(why));
                continue;
            }
            let mut fate_now = fate(&d, step);
            loop {
                match fate_now {
                    Fate::Answer => {
                        let value = answer(sup, &d, step);
                        match sup.completed(&d.task, Some(&value), "", &snap, now) {
                            Answered::Effects(out) => {
                                snap = applied(&snap, &out);
                                effects.extend(out);
                            }
                            Answered::Repair(_) => continue,
                        }
                        break;
                    }
                    Fate::Garbage => match sup.completed(&d.task, None, "I could not decide.", &snap, now) {
                        Answered::Effects(out) => {
                            snap = applied(&snap, &out);
                            effects.extend(out);
                            break;
                        }
                        Answered::Repair(_) => fate_now = Fate::Garbage,
                    },
                    Fate::Fail(ref failure) => {
                        let out = sup.failed(&d.task, failure.clone(), &snap, now);
                        snap = applied(&snap, &out);
                        effects.extend(out);
                        break;
                    }
                }
            }
        }
    }
    (effects, snap)
}

fn hypotheses(effects: &[Effect]) -> Vec<&Hypothesis> {
    effects.iter().filter_map(|e| if let Effect::Hypothesis(h) = e { Some(h) } else { None }).collect()
}

/// Nobody judged or reviewed their own family's work.
fn independent(effects: &[Effect], snap: &Snapshot) {
    let family = |id: &str| snap.hypotheses.iter().find(|h| h.id == id).map(|h| h.author.family()).unwrap();
    for effect in effects {
        match effect {
            Effect::Match(m) => assert!(m.judge.family() != family(&m.a) && m.judge.family() != family(&m.b), "{m:?}"),
            Effect::Review(r) => assert!(r.reviewer.family() != family(&r.hypothesis), "{r:?}"),
            _ => {}
        }
    }
}

#[test]
fn a_healthy_round_frames_generates_reviews_ranks_evolves_and_sums_up() {
    let snap = snapshot();
    let mut sup = Supervisor::new(0, 1, 7);
    sup.start_round(&snap, 1_000).unwrap();
    let (effects, snap) = play(&mut sup, snap, |_, _| Fate::Answer);

    assert_eq!(sup.state().status, RoundStatus::Idle, "{:#?}", sup.state().tasks);
    let made = hypotheses(&effects);
    assert_eq!(made.iter().filter(|h| h.parents.is_empty()).count(), 6, "two from each seat");
    assert_eq!(made.iter().filter(|h| !h.parents.is_empty()).count(), 2, "the leader refined, the top two combined");
    let ids: BTreeSet<&str> = made.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids.len(), made.len(), "no id given twice");
    let reviewed: BTreeSet<&str> =
        effects.iter().filter_map(|e| if let Effect::Review(r) = e { (r.kind == ReviewKind::Review).then_some(r.hypothesis.as_str()) } else { None }).collect();
    for h in made.iter().filter(|h| h.parents.is_empty()) {
        assert!(reviewed.contains(h.id.as_str()), "{} was not reviewed", h.id);
    }
    assert!(effects.iter().any(|e| matches!(e, Effect::Review(r) if r.kind == ReviewKind::Verification)), "the leader was verified");
    assert!(effects.iter().any(|e| matches!(e, Effect::Verdict(_))), "into VERDICT.md");
    let matches = effects.iter().filter(|e| matches!(e, Effect::Match(_))).count();
    assert!((3..=MATCHES).contains(&matches), "{matches} matches");
    assert!(effects.iter().any(|e| matches!(e, Effect::Plan(p) if p.contains("Test scattering laws."))), "framed");
    assert!(effects.iter().any(|e| matches!(e, Effect::Overview(o) if o.contains("Rayleigh leads."))), "summed up");
    assert!(effects.iter().any(|e| matches!(e, Effect::Status { status: HypothesisStatus::Retired, .. })), "the overview retired one");
    assert!(effects.iter().any(|e| matches!(e, Effect::Experiment(_))), "experiments proposed");
    assert!(effects.iter().any(|e| matches!(e, Effect::RoundReport { round: 1, .. })));
    assert!(sup.state().used <= 20, "{} calls", sup.state().used);
    assert_eq!(sup.state().guidance, ["Look at polarisation."]);
    independent(&effects, &snap);

    // The next round generates with the guidance, and does not frame again.
    let snap = Snapshot { plan: "# Research plan\n\nNext: polarisation.".into(), ..snap };
    sup.start_round(&snap, 50_000).unwrap();
    assert!(!sup.state().tasks.iter().any(|t| t.phase == Phase::Frame));
    let d = sup.ready(&snap, 50_001);
    assert!(d.iter().all(|d| d.phase == Phase::Generate) && d[0].prompt.contains("Look at polarisation."));
}

#[test]
fn a_seat_that_is_signed_out_sits_the_round_out_and_the_rest_carry_on() {
    let snap = snapshot();
    let mut sup = Supervisor::new(0, 1, 3);
    sup.start_round(&snap, 1_000).unwrap();
    let out = Failure::Unavailable { why: "Not logged in · Please run /login".into() };
    let (effects, snap) = play(&mut sup, snap, |d, _| if d.seat == Seat::Codex { Fate::Fail(out.clone()) } else { Fate::Answer });
    assert_eq!(sup.state().status, RoundStatus::Idle);
    assert!(sup.state().out.contains_key(&Seat::Codex));
    assert!(effects.iter().any(|e| matches!(e, Effect::Notice(n) if n.contains("Codex sits"))));
    let codex_runs = sup.state().tasks.iter().filter(|t| t.seat == Seat::Codex && t.started.is_some()).count();
    assert_eq!(codex_runs, 1, "only the task that found it out");
    assert_eq!(hypotheses(&effects).iter().filter(|h| h.parents.is_empty()).count(), 4);
    independent(&effects, &snap);
}

#[test]
fn a_seat_that_hits_its_limit_cools_and_its_judging_goes_elsewhere() {
    let snap = snapshot();
    let mut sup = Supervisor::new(0, 1, 11);
    sup.start_round(&snap, 1_000).unwrap();
    let limited = Failure::Limited { why: "You've hit your usage limit.".into(), resets: None };
    let mut first = true;
    let (effects, snap) = play(&mut sup, snap, |d, _| {
        if d.phase == Phase::Reflect && first {
            first = false;
            Fate::Fail(limited.clone())
        } else {
            Fate::Answer
        }
    });
    assert_eq!(sup.state().status, RoundStatus::Idle);
    assert!(!sup.state().cooling.is_empty());
    let new: Vec<&Hypothesis> = hypotheses(&effects).into_iter().filter(|h| h.parents.is_empty()).collect();
    let reviewed: BTreeSet<&str> = effects.iter().filter_map(|e| if let Effect::Review(r) = e { Some(r.hypothesis.as_str()) } else { None }).collect();
    assert!(new.iter().all(|h| reviewed.contains(h.id.as_str())), "the cooled seat's reviews were done by another");
    independent(&effects, &snap);
}

#[test]
fn an_answer_that_will_not_read_is_asked_again_once_then_given_up() {
    let snap = snapshot();
    let mut sup = Supervisor::new(0, 1, 5);
    sup.start_round(&snap, 1_000).unwrap();
    let d = sup.ready(&snap, 1_001);
    let frame = d.iter().find(|d| d.phase == Phase::Frame).unwrap().task.clone();
    let used = sup.state().used;
    match sup.completed(&frame, None, "Autonomous Integrator online.", &snap, 1_002) {
        Answered::Repair(message) => assert!(message.contains("no JSON object") && message.contains("only the JSON object"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(sup.state().used, used + 1, "a repair is a call");
    assert_eq!(sup.task(&frame).unwrap().state, TaskState::Repairing);
    // Still garbage: given up, and generation goes on without a plan.
    let Answered::Effects(effects) = sup.completed(&frame, None, "Still no.", &snap, 1_003) else { panic!() };
    assert!(effects.iter().any(|e| matches!(e, Effect::Notice(n) if n.contains("could not be read"))));
    assert!(matches!(sup.task(&frame).unwrap().state, TaskState::Failed { failure: Failure::Unparseable { .. } }));
    assert!(sup.ready(&snap, 1_004).iter().any(|d| d.phase == Phase::Generate));
}

#[test]
fn a_crash_is_tried_once_more() {
    let snap = snapshot();
    let mut sup = Supervisor::new(0, 1, 9);
    sup.start_round(&snap, 1_000).unwrap();
    let crashed = Failure::Crashed { why: "Codex stopped unexpectedly.".into() };
    let mut crashes = 0;
    let (effects, _) = play(&mut sup, snap, |d, _| {
        if d.phase == Phase::Generate && d.seat == Seat::Codex && crashes == 0 {
            crashes += 1;
            Fate::Fail(crashed.clone())
        } else {
            Fate::Answer
        }
    });
    let codex = hypotheses(&effects).into_iter().filter(|h| h.author.seat == Seat::Codex && h.parents.is_empty()).count();
    assert_eq!(codex, 2, "the retry generated");
    assert!(sup.state().tasks.iter().any(|t| t.seat == Seat::Codex && t.attempts == 1));
}

#[test]
fn a_judge_running_a_model_of_a_side_it_judges_is_replaced() {
    let mut snap = snapshot();
    // Antigravity, running a Claude model, may not judge Claude Code's work.
    snap.models.insert(Seat::Antigravity, "claude-opus-5-5-low".to_string());
    let mut sup = Supervisor::new(0, 1, 13);
    sup.start_round(&snap, 1_000).unwrap();
    let (effects, snap) = play(&mut sup, snap, |_, _| Fate::Answer);
    assert_eq!(sup.state().status, RoundStatus::Idle);
    independent(&effects, &snap);
}

#[test]
fn pausing_starts_nothing_new_and_stopping_cancels_what_is_running() {
    let snap = snapshot();
    let mut sup = Supervisor::new(0, 1, 1);
    sup.start_round(&snap, 1_000).unwrap();
    let running = sup.ready(&snap, 1_001);
    assert!(!running.is_empty());
    sup.pause();
    assert!(sup.ready(&snap, 1_002).is_empty());
    sup.resume();
    assert!(matches!(sup.state().status, RoundStatus::Running));
    let stopped = sup.stop(1_003);
    assert_eq!(stopped.len(), running.len());
    assert!(sup.state().tasks.iter().all(|t| t.state.terminal()));
    assert_eq!(sup.state().status, RoundStatus::Idle);
    assert!(sup.start_round(&snap, 1_004).is_ok(), "a stopped round can be followed by another");
}

#[test]
fn a_round_proton_was_closed_in_waits_paused_with_nothing_lost() {
    let snap = snapshot();
    let mut sup = Supervisor::new(0, 1, 2);
    sup.start_round(&snap, 1_000).unwrap();
    let running = sup.ready(&snap, 1_001);
    let saved = serde_json::to_string(sup.state()).unwrap();
    let mut back = Supervisor::restore(serde_json::from_str(&saved).unwrap());
    assert_eq!(back.state().status, RoundStatus::Paused { interrupted: true });
    assert!(back.ready(&snap, 1_002).is_empty(), "nothing starts until the scientist resumes");
    for d in &running {
        let t = back.task(&d.task).unwrap();
        assert_eq!((t.state.clone(), t.attempts), (TaskState::Ready, 1));
    }
    back.resume();
    assert_eq!(back.ready(&snap, 1_003).len(), running.len());
}

#[test]
fn a_finished_run_is_reviewed_by_a_seat_of_neither_the_proposers_nor_the_authors_family() {
    let snap = snapshot();
    let mut sup = Supervisor::new(0, 1, 4);
    sup.start_round(&snap, 1_000).unwrap();
    let (_, mut snap) = play(&mut sup, snap, |_, _| Fate::Answer);
    let h = snap.hypotheses.iter().find(|h| h.author.seat == Seat::Claude).unwrap().clone();
    snap.sandbox.push_str("\n## R001 — E001, for H001\n\nexit 0\n\nPeak at 470 nm.\n");
    let run = RunSummary { run: "R001".into(), experiment: "E001".into(), hypothesis: Some(h.id.clone()), proposer: Some(Author::new(Seat::Codex, "gpt-6.1-sol")), report: String::new() };
    assert!(sup.run_finished(&run, &snap, 9_000).is_empty());
    let task = sup.state().tasks.iter().find(|t| t.phase == Phase::SimReview).unwrap();
    assert_eq!(task.seat, Seat::Antigravity, "neither Anthropic nor OpenAI");
    sup.start_round(&snap, 9_001).unwrap();
    let d = sup.ready(&snap, 9_002);
    let review = d.iter().find(|d| d.phase == Phase::SimReview).expect("reviewed in the next round");
    assert!(review.prompt.contains("Peak at 470 nm."), "{}", review.prompt);
    let value = json!({ "finding": "supports", "summary": "Peaks at 470 nm.", "issues": [], "follow_up": proposal("") });
    let Answered::Effects(effects) = sup.completed(&review.task, Some(&value), "", &snap, 9_003) else { panic!() };
    assert!(effects.iter().any(|e| matches!(e, Effect::SandboxLog(s) if s.contains("R001: supports"))));
    assert!(effects.iter().any(|e| matches!(e, Effect::Review(r) if r.kind == ReviewKind::Simulation && r.verdict == Verdict::Strong)));
}

#[test]
fn a_small_budget_is_never_exceeded() {
    let mut snap = snapshot();
    snap.investigation.as_mut().unwrap().budget.calls = 8;
    let mut sup = Supervisor::new(0, 1, 21);
    sup.start_round(&snap, 1_000).unwrap();
    let (effects, _) = play(&mut sup, snap, |_, _| Fate::Answer);
    assert!(sup.state().used <= 8, "{}", sup.state().used);
    assert!(effects.iter().any(|e| matches!(e, Effect::Notice(n) if n.contains("budget"))));
}

#[test]
fn nobody_can_start_a_round_when_no_seat_can_sit() {
    let mut snap = snapshot();
    for seat in Seat::ENGINES {
        snap.unavailable.insert(seat, "not installed".into());
    }
    let why = Supervisor::new(0, 1, 1).start_round(&snap, 0).unwrap_err();
    assert!(why.contains("No seat can take part") && why.contains("Codex: not installed"), "{why}");
}

/// Many rounds, with fates drawn from a seed: whatever happens, the round
/// ends, the budget holds, nobody judges their own, and the same seed plays
/// the same round.
#[test]
fn whatever_happens_a_round_ends_within_its_budget_and_judges_fairly() {
    for seed in 1..=60u64 {
        let run = |seed: u64| {
            let mut snap = snapshot();
            if seed % 5 == 0 {
                snap.models.insert(Seat::Antigravity, "claude-opus-5-5-low".into());
            }
            let mut sup = Supervisor::new(0, 1, seed);
            sup.start_round(&snap, 1_000).unwrap();
            let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let (effects, snap) = play(&mut sup, snap, |_, _| match next_random(&mut rng) % 12 {
                0 => Fate::Garbage,
                1 => Fate::Fail(Failure::Crashed { why: "gone".into() }),
                2 => Fate::Fail(Failure::TimedOut),
                3 => Fate::Fail(Failure::Limited { why: "limit".into(), resets: None }),
                4 if seed % 3 == 0 => Fate::Fail(Failure::Unavailable { why: "signed out".into() }),
                _ => Fate::Answer,
            });
            (sup, effects, snap)
        };
        let (sup, effects, snap) = run(seed);
        let cap = snap.investigation.as_ref().unwrap().budget.calls;
        assert!(sup.state().used <= cap, "seed {seed}: {} calls", sup.state().used);
        assert!(sup.state().tasks.iter().all(|t| t.state.terminal()), "seed {seed}: {:#?}", sup.state().tasks);
        assert_eq!(sup.state().status, RoundStatus::Idle, "seed {seed}");
        independent(&effects, &snap);
        let (_, again, _) = run(seed);
        assert_eq!(format!("{effects:?}"), format!("{again:?}"), "seed {seed} played differently twice");
    }
}
