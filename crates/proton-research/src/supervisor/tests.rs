//! Rounds played with made-up answers.

use super::*;
use crate::model::{Discipline, LiteraturePolicy, Record};
use serde_json::json;

/// A proof of `theorem`, as a prover answers it.
fn proof_of(theorem: &str) -> Value {
    json!({ "label": "A", "kind": "proof", "lean": format!("import Mathlib\n\ntheorem {theorem} (n : ℕ) : n + 0 = n := by simp\n"),
        "notes": "It is the hypothesis, for every n." })
}

/// An investigation that does not use the literature: what a round was
/// before there was one.
fn snapshot() -> Snapshot {
    let mut snap = lit_snapshot();
    snap.investigation.as_mut().unwrap().literature = LiteraturePolicy::off();
    snap
}

/// A new investigation, which does.
fn lit_snapshot() -> Snapshot {
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

/// A paper a search found.
fn found(title: &str, doi: &str, query: &str, rank: u32) -> Candidate {
    Candidate {
        record: Record {
            title: title.into(),
            doi: Some(doi.into()),
            year: Some(2001),
            abstract_: format!("{title}: a mechanism that scatters light, by molecules in the sky."),
            sources: vec![LiteratureSource::OpenAlex],
            ..Record::default()
        },
        queries: vec![query.into()],
        rank,
    }
}

/// What the literature does in a test: answers with these papers, fails,
/// or never answers.
#[derive(Clone)]
enum Lit {
    Found(Vec<Candidate>),
    Failed(String),
    Never,
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
        Phase::Frame => json!({ "plan": "Test scattering laws.", "criteria": ["Quantitative"], "questions": ["Why blue not violet?"], "queries": ["rayleigh scattering sky"] }),
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
        Phase::Meta => json!({ "overview": "Rayleigh leads.", "plan": "Next: polarisation.", "guidance": ["Look at polarisation."], "retire": labels.iter().take(1).collect::<Vec<_>>(),
            "queries": ["skylight polarisation"] }),
        Phase::SimReview => json!({ "finding": "supports", "summary": "The spectrum peaks at 470 nm.", "issues": [], "follow_up": proposal("") }),
        Phase::Prove => proof_of(&task.targets[0]),
    }
}

/// What the prover answers and what Lean says of each file, in a test: in
/// turn, the last of each said again. What was told and checked is kept.
#[derive(Default)]
struct Proving {
    /// Answers, `{theorem}` standing for the theorem's name; none, a proof.
    answers: Vec<Value>,
    /// `clean`, `sorry`, `error` or `slow`; none, `clean`.
    lean: Vec<&'static str>,
    /// Hold each check, rather than answer it: the round waits on it.
    hold: bool,
    held: Vec<(String, ProofCheck)>,
    told: Vec<String>,
    checked: Vec<String>,
    asked: usize,
}

impl Proving {
    fn answer(&mut self, sup: &Supervisor, d: &Dispatch) -> Value {
        let theorem = sup.task(&d.task).unwrap().targets[0].clone();
        let n = self.asked;
        self.asked += 1;
        match self.answers.get(n).or(self.answers.last()) {
            Some(answer) => serde_json::from_str(&answer.to_string().replace("{theorem}", &theorem)).unwrap(),
            None => proof_of(&theorem),
        }
    }

    /// What Lean says of a check: its messages, as `lean --json` writes them.
    fn check(&mut self, c: &ProofCheck) -> LeanRun {
        let n = self.checked.len();
        self.checked.push(c.assembled.text.clone());
        let says = self.lean.get(n).or(self.lean.last()).copied().unwrap_or("clean");
        let t = &c.assembled.theorem;
        let message = |severity: &str, line: u32, column: u32, data: &str, kind: &str| {
            json!({ "severity": severity, "pos": { "line": line, "column": column }, "data": data, "kind": kind }).to_string()
        };
        let statement = message("information", c.assembled.check_line, 0, &format!("@{t} : ∀ (n : ℕ), n + 0 = n"), "[anonymous]");
        let axioms = |list: &str| message("information", c.assembled.axioms_line, 0, &format!("'{t}' depends on axioms: [{list}]"), "[anonymous]");
        let (lines, ended) = match says {
            "clean" => (vec![statement, axioms("propext")], Ended::Exited(Some(0))),
            "sorry" => (vec![message("warning", 3, 8, "declaration uses 'sorry'", "hasSorry"), statement, axioms("sorryAx")], Ended::Exited(Some(0))),
            "error" => (vec![message("error", 3, 30, "unknown identifier 'Nat.add_zero_left'", "[anonymous]"), statement, axioms("sorryAx")], Ended::Exited(Some(1))),
            "slow" => (Vec::new(), Ended::TimedOut),
            other => panic!("Lean says {other}?"),
        };
        LeanRun { lines, ended, err: String::new(), attempt: n as u32 + 1 }
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
fn play(sup: &mut Supervisor, snap: Snapshot, fate: impl FnMut(&Dispatch, usize) -> Fate) -> (Vec<Effect>, Snapshot) {
    let models = snap.models.clone();
    play_reporting(sup, snap, fate, move |seat| models.get(&seat).cloned().unwrap_or_default())
}

/// The same, with each seat reporting the model `report` says as it starts.
fn play_reporting(sup: &mut Supervisor, snap: Snapshot, fate: impl FnMut(&Dispatch, usize) -> Fate, report: impl Fn(Seat) -> String) -> (Vec<Effect>, Snapshot) {
    let (effects, snap, _) = play_with(sup, snap, fate, report, Lit::Found(Vec::new()));
    (effects, snap)
}

/// The same, the literature doing what `lit` says; and what it was asked.
fn play_with(sup: &mut Supervisor, snap: Snapshot, fate: impl FnMut(&Dispatch, usize) -> Fate, report: impl Fn(Seat) -> String, lit: Lit) -> (Vec<Effect>, Snapshot, Vec<LiteratureSearch>) {
    play_all(sup, snap, fate, report, lit, &mut Proving::default())
}

/// A round played with the prover and Lean doing what `proving` says.
fn play_proving(sup: &mut Supervisor, snap: Snapshot, fate: impl FnMut(&Dispatch, usize) -> Fate, proving: &mut Proving) -> (Vec<Effect>, Snapshot) {
    let models = snap.models.clone();
    let (effects, snap, _) = play_all(sup, snap, fate, move |seat| models.get(&seat).cloned().unwrap_or_default(), Lit::Found(Vec::new()), proving);
    (effects, snap)
}

fn play_all(
    sup: &mut Supervisor,
    mut snap: Snapshot,
    mut fate: impl FnMut(&Dispatch, usize) -> Fate,
    report: impl Fn(Seat) -> String,
    lit: Lit,
    proving: &mut Proving,
) -> (Vec<Effect>, Snapshot, Vec<LiteratureSearch>) {
    let mut effects = Vec::new();
    let mut asked = Vec::new();
    let mut now = 1_000;
    for step in 0..400 {
        now += 30;
        // As the driver does: the literature first, for the hypotheses wait on it.
        if let Some(search) = sup.literature_due(&snap, now) {
            asked.push(search);
            let answered = match &lit {
                Lit::Found(papers) => Some(Ok(papers.clone())),
                Lit::Failed(why) => Some(Err(why.clone())),
                Lit::Never => None,
            };
            if let Some(answered) = answered {
                let out = sup.literature_answered(answered, Vec::new(), &snap, now);
                snap = applied(&snap, &out);
                effects.extend(out);
            }
        }
        effects.extend(sup.literature_overdue(now));
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
            let model = report(d.seat);
            if let Some(refused) = sup.started(&d.task, "native", &model, &snap, now) {
                effects.push(Effect::Notice(refused.why));
                snap = applied(&snap, &refused.effects);
                effects.extend(refused.effects);
                continue;
            }
            let fate_now = fate(&d, step);
            loop {
                let mut answered = match fate_now {
                    Fate::Answer if d.phase == Phase::Prove => {
                        let value = proving.answer(sup, &d);
                        sup.completed(&d.task, Some(&value), "", &snap, now)
                    }
                    Fate::Answer => sup.completed(&d.task, Some(&answer(sup, &d, step)), "", &snap, now),
                    Fate::Garbage => sup.completed(&d.task, None, "I could not decide.", &snap, now),
                    Fate::Fail(ref failure) => {
                        let out = sup.failed(&d.task, failure.clone(), &snap, now);
                        snap = applied(&snap, &out);
                        effects.extend(out);
                        break;
                    }
                };
                // A proof goes to Lean, and what Lean says back to the round.
                let answered = loop {
                    match answered {
                        Answered::Check(check) if proving.hold => {
                            proving.held.push((d.task.clone(), check));
                            break None;
                        }
                        Answered::Check(check) => answered = sup.lean_checked(&d.task, &proving.check(&check), &snap, now),
                        other => break Some(other),
                    }
                };
                match answered {
                    None => break,
                    Some(Answered::Effects(out)) => {
                        snap = applied(&snap, &out);
                        effects.extend(out);
                        break;
                    }
                    Some(Answered::Repair(told)) => {
                        if d.phase == Phase::Prove {
                            proving.told.push(told);
                        }
                    }
                    Some(Answered::Check(_)) => unreachable!(),
                }
            }
        }
    }
    (effects, snap, asked)
}

fn papers(effects: &[Effect]) -> Vec<&Paper> {
    effects.iter().filter_map(|e| if let Effect::Paper(p) = e { Some(p) } else { None }).collect()
}

/// The calls the round counted are the calls its tasks made: the
/// literature is asked by the core, for none.
fn counted(sup: &Supervisor) {
    assert_eq!(sup.state().used, sup.state().tasks.iter().filter(|t| t.round == sup.state().round).map(|t| t.calls).sum::<u32>(), "{:#?}", sup.state().tasks);
}

#[test]
fn the_hypotheses_wait_for_the_literature_the_plan_asked_for_and_are_shown_it() {
    let snap = lit_snapshot();
    let mut sup = Supervisor::new(0, 1, 7);
    sup.start_round(&snap, 1_000).unwrap();
    // Before the plan is framed nothing is searched.
    assert_eq!(sup.literature_due(&snap, 1_001), None, "searched before the plan said what for");
    let mut prompts: Vec<(Phase, String)> = Vec::new();
    let lit = Lit::Found(vec![
        found("Skylight spectra", "10.1364/josaa.18.000412", "rayleigh scattering sky", 0),
        found("Ozone and the twilight zenith", "10.1364/josa.43.000113", "rayleigh scattering sky", 1),
    ]);
    let (effects, snap, asked) = play_with(&mut sup, snap, |d, _| {
        prompts.push((d.phase, d.prompt.clone()));
        Fate::Answer
    }, |seat| lit_snapshot().models[&seat].clone(), lit);
    assert_eq!(sup.state().status, RoundStatus::Idle);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].queries, ["rayleigh scattering sky"], "the plan's queries");
    assert_eq!(asked[0].sources, LiteratureSource::ALL);
    let added = papers(&effects);
    assert_eq!(added.iter().map(|p| (p.id.as_str(), p.round, p.by)).collect::<Vec<_>>(), [("P001", Some(1), AddedBy::Round), ("P002", Some(1), AddedBy::Round)]);
    assert_eq!(snap.papers.len(), 2);
    let generate: Vec<&String> = prompts.iter().filter(|(p, _)| *p == Phase::Generate).map(|(_, prompt)| prompt).collect();
    assert_eq!(generate.len(), 3);
    assert!(generate.iter().all(|p| p.contains("### The literature the team has gathered") && p.contains("- **P001**") && p.contains("references")));
    let reflect = prompts.iter().find(|(p, _)| *p == Phase::Reflect).map(|(_, prompt)| prompt).unwrap();
    assert!(reflect.contains("closest to it") && reflect.contains("P001 *Skylight spectra*"), "{reflect}");
    counted(&sup);
    assert!(effects.iter().any(|e| matches!(e, Effect::RoundReport { text, .. } if text.contains("## Literature") && text.contains("Added P001, P002."))));
    assert_eq!(sup.state().queries, ["skylight polarisation"], "the overview's queries, for the next round");

    // The next round searches for what the overview asked, numbers on, and adds none it has.
    let snap = Snapshot { plan: "# Research plan\n\nNext: polarisation.".into(), ..snap };
    sup.start_round(&snap, 90_000).unwrap();
    assert!(sup.ready(&snap, 90_001).is_empty(), "the hypotheses did not wait for the literature");
    let search = sup.literature_due(&snap, 90_002).expect("searched at once, with no plan to frame");
    assert_eq!(search.queries, ["skylight polarisation"]);
    let again = vec![found("Skylight spectra, again", "10.1364/JOSAA.18.000412", "skylight polarisation", 0), found("Polarised skylight", "10.1/pol", "skylight polarisation", 1)];
    let effects = sup.literature_answered(Ok(again), vec!["Semantic Scholar: busy".into()], &snap, 90_003);
    assert_eq!(papers(&effects).iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["P003"], "the one it has is not added twice");
    assert!(effects.iter().any(|e| matches!(e, Effect::Notice(n) if n.contains("Semantic Scholar: busy"))));
    assert!(sup.ready(&snap, 90_004).iter().any(|d| d.phase == Phase::Generate));
}

#[test]
fn the_literature_is_waited_for_until_its_deadline_and_what_comes_late_still_comes() {
    let mut snap = lit_snapshot();
    snap.plan = "# Research plan\n\nAlready framed.".into();
    let mut sup = Supervisor::new(1, 1, 3);
    sup.start_round(&snap, 1_000).unwrap();
    let search = sup.literature_due(&snap, 1_000).unwrap();
    assert_eq!(search.queries, ["sky blue"], "the title's words, with nothing else to go on");
    assert_eq!(sup.literature_due(&snap, 1_001), None, "asked once");
    assert!(sup.ready(&snap, 1_060).is_empty() && sup.literature_overdue(1_060).is_empty());
    // Two minutes on, the hypotheses go without it, once.
    let overdue = sup.literature_overdue(1_120);
    assert!(matches!(overdue.as_slice(), [Effect::Notice(n)] if n.contains("go on without it")));
    assert!(sup.literature_overdue(1_121).is_empty());
    assert_eq!(sup.ready(&snap, 1_121).len(), 3);
    // What it finds after reaches the reviews.
    let late = sup.literature_answered(Ok(vec![found("Late", "10.1/late", "daytime sky blue", 0)]), Vec::new(), &snap, 1_500);
    assert_eq!(papers(&late).len(), 1);
    counted(&sup);
    // Proton closed mid-search asks again when the round is resumed.
    let mut sup = Supervisor::new(1, 1, 3);
    sup.start_round(&snap, 2_000).unwrap();
    sup.literature_due(&snap, 2_000).unwrap();
    let mut back = Supervisor::restore(serde_json::from_str(&serde_json::to_string(sup.state()).unwrap()).unwrap());
    back.resume();
    assert!(back.literature_due(&snap, 2_010).is_some());
}

#[test]
fn a_search_that_fails_is_said_and_one_never_answered_is_not_waited_on_forever() {
    for lit in [Lit::Failed("arXiv could not be reached".into()), Lit::Never] {
        let never = matches!(lit, Lit::Never);
        let snap = lit_snapshot();
        let mut sup = Supervisor::new(0, 1, 5);
        sup.start_round(&snap, 1_000).unwrap();
        let (effects, _, asked) = play_with(&mut sup, snap, |_, _| Fate::Answer, |seat| lit_snapshot().models[&seat].clone(), lit);
        assert_eq!(asked.len(), 1);
        assert_eq!(sup.state().status, RoundStatus::Idle, "the round did not end");
        assert_eq!(hypotheses(&effects).iter().filter(|h| h.parents.is_empty()).count(), 6, "the hypotheses went on without it");
        let said = if never { "go on without it" } else { "could not be searched: arXiv could not be reached" };
        assert!(effects.iter().any(|e| matches!(e, Effect::Notice(n) if n.contains(said))), "{effects:#?}");
        let step = sup.state().literature.as_ref().unwrap();
        assert_eq!(step.state.terminal(), true, "{:?}", step.state);
        counted(&sup);
    }
}

#[test]
fn an_investigation_without_the_literature_waits_for_none() {
    let snap = snapshot();
    let mut sup = Supervisor::new(1, 1, 9);
    let snap = Snapshot { plan: "# Research plan\n\nFramed.".into(), ..snap };
    sup.start_round(&snap, 1_000).unwrap();
    assert!(sup.state().literature.is_none());
    assert_eq!(sup.literature_due(&snap, 1_000), None);
    assert_eq!(sup.ready(&snap, 1_000).len(), 3, "the hypotheses wait for nothing");
}

/// Rounds with the literature doing anything at all: the round ends, every
/// call is counted and the literature is none of them, papers are numbered
/// in turn and never more than a round adds, and a seed plays the same twice.
#[test]
fn whatever_the_literature_does_the_round_ends_and_costs_no_call() {
    for seed in 1..=40u64 {
        let run = |seed: u64| {
            let mut snap = lit_snapshot();
            snap.investigation.as_mut().unwrap().literature.papers = 1 + (seed % 4) as u32;
            let mut sup = Supervisor::new(0, 1, seed);
            sup.start_round(&snap, 1_000).unwrap();
            let lit = match seed % 3 {
                0 => Lit::Never,
                1 => Lit::Failed("no".into()),
                _ => Lit::Found((0..(seed % 7) as u32).map(|n| found(&format!("Paper {n}"), &format!("10.1/{n}"), "rayleigh scattering sky", n)).collect()),
            };
            let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let (effects, snap, _) = play_with(&mut sup, snap, |_, _| match next_random(&mut rng) % 10 {
                0 => Fate::Garbage,
                1 => Fate::Fail(Failure::Crashed { why: "gone".into() }),
                2 => Fate::Fail(Failure::Limited { why: "limit".into(), resets: None }),
                _ => Fate::Answer,
            }, |seat| lit_snapshot().models[&seat].clone(), lit);
            (sup, effects, snap)
        };
        let (sup, effects, snap) = run(seed);
        assert_eq!(sup.state().status, RoundStatus::Idle, "seed {seed}");
        assert!(sup.state().literature.as_ref().is_some_and(|s| s.state.terminal()), "seed {seed}");
        counted(&sup);
        let ids: Vec<&str> = papers(&effects).iter().map(|p| p.id.as_str()).collect();
        let most = snap.investigation.as_ref().unwrap().literature.papers as usize;
        assert!(ids.len() <= most, "seed {seed}: {ids:?}");
        assert_eq!(ids, (1..=ids.len()).map(|n| format!("P{n:03}")).collect::<Vec<_>>(), "seed {seed}");
        let (_, again, _) = run(seed);
        assert_eq!(format!("{effects:?}"), format!("{again:?}"), "seed {seed} played differently twice");
    }
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

#[test]
fn a_judge_refused_as_it_starts_with_nowhere_to_go_does_not_stall_the_round() {
    // Codex is out, and Antigravity turns out to be running Claude: nobody
    // may review Claude Code's hypotheses, and the round must end anyway.
    let mut snap = snapshot();
    snap.models.remove(&Seat::Antigravity);
    snap.unavailable.insert(Seat::Codex, "Codex is not installed.".into());
    let mut sup = Supervisor::new(0, 1, 11);
    sup.start_round(&snap, 1_000).unwrap();
    // Claude Code runs Claude, and so — it turns out — does Antigravity.
    let (effects, _) = play_reporting(&mut sup, snap, |_, _| Fate::Answer, |_| "claude-opus-5-5".into());
    assert_eq!(sup.state().status, RoundStatus::Idle, "the round stalled: {:?}", sup.state().tasks.iter().map(|t| (&t.id, t.phase, &t.state)).collect::<Vec<_>>());
    assert!(effects.iter().any(|e| matches!(e, Effect::RoundReport { .. })));
}

/// A mathematics investigation on a machine with Lean made.
fn math_snapshot() -> Snapshot {
    let mut snap = snapshot();
    let i = snap.investigation.as_mut().unwrap();
    i.discipline = Discipline::Mathematics;
    i.title = "Does adding zero change a number?".into();
    snap.lean = Some(LeanFacts { lean: "v4.34.1".into(), mathlib: "v4.34.1".into(), sources: Some("/h/.proton/lean/project/.lake/packages/mathlib".into()) });
    snap
}

fn kept(effects: &[Effect]) -> Vec<&Kept> {
    effects.iter().filter_map(|e| if let Effect::Proof(k) = e { Some(k) } else { None }).collect()
}

fn prover(sup: &Supervisor) -> &Task {
    sup.state().tasks.iter().find(|t| t.phase == Phase::Prove).expect("a proof was planned")
}

/// A round to its end with the prover and Lean as `proving` says.
fn proved(seed: u64, proving: &mut Proving) -> (Supervisor, Vec<Effect>, Snapshot) {
    let snap = math_snapshot();
    let mut sup = Supervisor::new(0, 1, seed);
    sup.start_round(&snap, 1_000).unwrap();
    let (effects, snap) = play_proving(&mut sup, snap, |_, _| Fate::Answer, proving);
    assert_eq!(sup.state().status, RoundStatus::Idle, "{:#?}", sup.state().tasks);
    counted(&sup);
    independent(&effects, &snap);
    (sup, effects, snap)
}

#[test]
fn a_mathematics_round_proves_its_verified_leader_in_lean_before_the_overview() {
    let mut proving = Proving::default();
    let (sup, effects, _) = proved(7, &mut proving);
    let proofs = kept(&effects);
    assert_eq!(proofs.len(), 1);
    let p = &proofs[0].record;
    assert_eq!((p.outcome, p.kind, p.attempts, p.attempt), (FormalOutcome::Proved, ProofKind::Proof, 1, 1));
    assert_eq!((p.statement.as_deref(), p.axioms.as_slice()), (Some("∀ (n : ℕ), n + 0 = n"), ["propext".to_string()].as_slice()));
    assert_eq!((p.lean.as_str(), p.mathlib.as_str()), ("v4.34.1", "v4.34.1"));
    assert!(proofs[0].lean.contains(&format!("theorem {} (n : ℕ)", p.hypothesis)));
    let verified = effects.iter().find_map(|e| if let Effect::Review(r) = e { (r.kind == ReviewKind::Verification).then(|| r.hypothesis.clone()) } else { None });
    assert_eq!(Some(p.hypothesis.clone()), verified, "the leader verified is the one proved");
    assert!(effects.iter().any(|e| matches!(e, Effect::Review(r) if r.kind == ReviewKind::Formal && r.verdict == Verdict::Strong && r.correctness == 5)));
    assert!(effects.iter().any(|e| matches!(e, Effect::Verdict(v) if v.contains("— Lean: proved") && v.contains("```lean\n∀ (n : ℕ), n + 0 = n\n```"))));
    let t = prover(&sup);
    assert_eq!((t.calls, proving.checked.len()), (1, 1), "one call, one check");
    assert!(proving.checked[0].ends_with(&format!("#check @{0}\n#print axioms {0}\n", p.hypothesis)));
    let meta = sup.state().tasks.iter().find(|t| t.phase == Phase::Meta).unwrap();
    assert!(meta.started.unwrap() > t.ended.unwrap(), "the overview went before the proof was settled");
    assert!(effects.iter().any(|e| matches!(e, Effect::RoundReport { text, .. } if text.contains("## Proof") && text.contains(": proved —"))));
}

#[test]
fn lean_errors_go_back_to_the_prover_and_the_mended_file_is_proved() {
    let mut proving = Proving { lean: vec!["error", "clean"], ..Proving::default() };
    let (sup, effects, _) = proved(7, &mut proving);
    assert_eq!(proving.told.len(), 1);
    let told = &proving.told[0];
    assert!(told.contains("Line 3, column 30 (error):\nunknown identifier 'Nat.add_zero_left'") && told.contains("whole corrected file"), "{told}");
    let p = &kept(&effects)[0].record;
    assert_eq!((p.outcome, p.attempts, p.attempt), (FormalOutcome::Proved, 2, 2));
    assert_eq!(prover(&sup).calls, 2, "the mending was a call");
}

#[test]
fn out_of_calls_a_statement_lean_accepted_is_kept_with_its_proof_open() {
    let mut proving = Proving { lean: vec!["sorry"], ..Proving::default() };
    let (sup, effects, snap) = proved(7, &mut proving);
    let policy = snap.investigation.as_ref().unwrap().lean.clone();
    assert_eq!((prover(&sup).calls, proving.checked.len() as u32), (policy.calls, policy.calls), "the proof's calls, all spent, and no more");
    assert!(proving.told.iter().all(|t| t.contains("Finish the proof")), "{:?}", proving.told);
    let p = &kept(&effects)[0].record;
    assert_eq!((p.outcome, p.attempts), (FormalOutcome::Open, policy.calls));
    assert!(p.messages.iter().any(|m| m.text.contains("sorry")));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Review(r) if r.kind == ReviewKind::Formal)), "an open proof earns no review");
    assert!(effects.iter().any(|e| matches!(e, Effect::Overview(_))), "the overview was written");
}

#[test]
fn a_disproof_is_kept_and_reviewed_as_one() {
    let answer = json!({ "label": "A", "kind": "disproof", "lean": "import Mathlib\n\ntheorem {theorem} : ¬ ∀ n : ℕ, n + 1 = n := by simp\n",
        "notes": "The hypothesis claims every n is its own successor; its negation is proved." });
    let mut proving = Proving { answers: vec![answer], ..Proving::default() };
    let (_, effects, _) = proved(7, &mut proving);
    let p = &kept(&effects)[0].record;
    assert_eq!((p.outcome, p.kind), (FormalOutcome::Disproved, ProofKind::Disproof));
    assert!(effects.iter().any(|e| matches!(e, Effect::Review(r) if r.kind == ReviewKind::Formal && r.verdict == Verdict::Flawed)));
}

#[test]
fn a_hypothesis_with_no_faithful_statement_is_kept_unformalized_without_a_check() {
    let answer = json!({ "label": "A", "kind": "none", "lean": "", "notes": "It is about how people feel about zero." });
    let mut proving = Proving { answers: vec![answer], ..Proving::default() };
    let (_, effects, _) = proved(7, &mut proving);
    assert!(proving.checked.is_empty(), "nothing went to Lean");
    let k = kept(&effects)[0];
    assert_eq!((k.record.outcome, k.lean.as_str()), (FormalOutcome::Unformalized, ""));
    assert!(effects.iter().any(|e| matches!(e, Effect::Verdict(v) if v.contains("no faithful statement") && !v.contains("The file is"))));
}

#[test]
fn a_file_that_could_run_code_is_sent_back_unchecked_and_that_costs_a_call() {
    let unsafe_file = json!({ "label": "A", "kind": "proof", "lean": "import Mathlib\n\n#eval IO.println \"hi\"\n\ntheorem {theorem} : True := trivial\n", "notes": "n" });
    let mut proving = Proving { answers: vec![unsafe_file, proof_of("{theorem}")], ..Proving::default() };
    let (sup, effects, _) = proved(7, &mut proving);
    assert!(proving.told[0].contains("`#eval`") && proving.told[0].contains("Answer again"), "{:?}", proving.told);
    assert_eq!((proving.checked.len(), prover(&sup).calls), (1, 2), "the refused file never reached Lean, and the refusal was a call");
    assert_eq!(kept(&effects)[0].record.outcome, FormalOutcome::Proved);
}

#[test]
fn physics_never_goes_to_lean() {
    let mut snap = snapshot();
    snap.lean = math_snapshot().lean;
    let mut sup = Supervisor::new(0, 1, 7);
    sup.start_round(&snap, 1_000).unwrap();
    let (effects, _) = play(&mut sup, snap, |_, _| Fate::Answer);
    assert!(!sup.state().tasks.iter().any(|t| t.phase == Phase::Prove));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Notice(n) if n.contains("Lean"))));
}

#[test]
fn without_lean_the_round_says_so_once_and_goes_on() {
    let mut snap = math_snapshot();
    snap.lean = None;
    let mut sup = Supervisor::new(0, 1, 7);
    sup.start_round(&snap, 1_000).unwrap();
    let (effects, _) = play(&mut sup, snap, |_, _| Fate::Answer);
    assert_eq!(sup.state().status, RoundStatus::Idle);
    let said: Vec<&String> = effects.iter().filter_map(|e| if let Effect::Notice(n) = e { n.contains("Lean is not made").then_some(n) } else { None }).collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(!sup.state().tasks.iter().any(|t| t.phase == Phase::Prove));
    assert!(effects.iter().any(|e| matches!(e, Effect::RoundReport { text, .. } if text.contains("## Proof") && text.contains("Lean is not made"))));
}

#[test]
fn the_overview_waits_while_lean_checks_and_a_round_closed_mid_check_picks_the_proof_up_again() {
    let snap = math_snapshot();
    let mut sup = Supervisor::new(0, 1, 7);
    sup.start_round(&snap, 1_000).unwrap();
    let mut proving = Proving { hold: true, ..Proving::default() };
    let (_, snap) = play_proving(&mut sup, snap, |_, _| Fate::Answer, &mut proving);
    let (task, check) = proving.held.pop().expect("a proof went to Lean");
    assert_eq!(sup.task(&task).unwrap().state, TaskState::Checking);
    let meta = sup.state().tasks.iter().find(|t| t.phase == Phase::Meta).expect("the overview is planned");
    assert_eq!(meta.state, TaskState::Ready, "the overview started while Lean checked");
    assert!(matches!(sup.state().status, RoundStatus::Running));
    assert!(sup.ready(&snap, 90_000).is_empty(), "nothing else starts while Lean checks");

    // Proton closes mid-check: the proof is asked for again, with its last file.
    let saved: RoundState = serde_json::from_str(&serde_json::to_string(sup.state()).unwrap()).unwrap();
    let mut back = Supervisor::restore(saved);
    back.resume();
    let again = back.ready(&snap, 90_001);
    let prove = again.iter().find(|d| d.phase == Phase::Prove).expect("the proof is asked for again");
    assert!(prove.prompt.contains("## The last attempt") && prove.prompt.contains("being checked when Proton closed"), "{}", prove.prompt);
    assert!(prove.prompt.contains(&format!("theorem {}", check.hypothesis)));
    assert_eq!(back.task(&task).unwrap().attempts, 1);

    // In the first, Lean answers: the proof is kept, and the overview goes.
    let run = Proving::default().check(&check);
    let Answered::Effects(effects) = sup.lean_checked(&task, &run, &snap, 90_002) else { panic!() };
    assert_eq!(kept(&effects)[0].record.outcome, FormalOutcome::Proved);
    let snap = applied(&snap, &effects);
    assert!(sup.ready(&snap, 90_003).iter().any(|d| d.phase == Phase::Meta));
}

#[test]
fn stopping_a_round_stops_its_check() {
    let snap = math_snapshot();
    let mut sup = Supervisor::new(0, 1, 7);
    sup.start_round(&snap, 1_000).unwrap();
    let mut proving = Proving { hold: true, ..Proving::default() };
    let (_, snap) = play_proving(&mut sup, snap, |_, _| Fate::Answer, &mut proving);
    let (task, check) = proving.held.pop().unwrap();
    assert!(sup.stop(90_000).contains(&task), "the check was not among what to stop");
    assert_eq!(sup.task(&task).unwrap().state, TaskState::Cancelled);
    let run = Proving::default().check(&check);
    assert_eq!(sup.lean_checked(&task, &run, &snap, 90_001), Answered::Effects(Vec::new()), "a check that ends after its round stopped changes nothing");
}

/// Mathematics rounds with fates and Lean's verdicts drawn from a seed:
/// whatever happens, the round ends within its budget, the proof within its
/// calls, nobody proves their own family's work, and the same seed plays
/// the same round.
#[test]
fn whatever_happens_a_proving_round_ends_within_its_calls() {
    for seed in 1..=40u64 {
        let run = |seed: u64| {
            let snap = math_snapshot();
            let mut sup = Supervisor::new(0, 1, seed);
            sup.start_round(&snap, 1_000).unwrap();
            let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let says = ["clean", "sorry", "error", "slow"];
            let lean: Vec<&'static str> = (0..4).map(|k| says[((seed >> k) % 4) as usize]).collect();
            let mut proving = Proving { lean, ..Proving::default() };
            let (effects, snap) = play_proving(&mut sup, snap, |_, _| match next_random(&mut rng) % 14 {
                0 => Fate::Garbage,
                1 => Fate::Fail(Failure::Crashed { why: "gone".into() }),
                2 => Fate::Fail(Failure::TimedOut),
                3 => Fate::Fail(Failure::Limited { why: "limit".into(), resets: None }),
                4 if seed % 3 == 0 => Fate::Fail(Failure::Unavailable { why: "signed out".into() }),
                _ => Fate::Answer,
            }, &mut proving);
            (sup, effects, snap)
        };
        let (sup, effects, snap) = run(seed);
        let i = snap.investigation.as_ref().unwrap();
        assert!(sup.state().used <= i.budget.calls, "seed {seed}: {} calls", sup.state().used);
        let proving: u32 = sup.state().tasks.iter().filter(|t| t.phase == Phase::Prove).map(|t| t.calls).sum();
        assert!(proving <= i.lean.calls, "seed {seed}: the proof took {proving} calls");
        assert!(sup.state().tasks.iter().all(|t| t.state.terminal()), "seed {seed}: {:#?}", sup.state().tasks);
        assert_eq!(sup.state().status, RoundStatus::Idle, "seed {seed}");
        assert!(kept(&effects).len() <= 1, "seed {seed}: one proof a round");
        independent(&effects, &snap);
        let (_, again, _) = run(seed);
        assert_eq!(format!("{effects:?}"), format!("{again:?}"), "seed {seed} played differently twice");
    }
}
