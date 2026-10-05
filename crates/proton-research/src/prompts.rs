//! What each agent is told: who it is, how its discipline is done, and the
//! task in front of it.
//!
//! The instructions — the role, the discipline's method, the rules every
//! task keeps — go where each engine takes standing instructions (Claude
//! Code's system prompt, Codex's developer instructions; before the first
//! message for Antigravity). The task is the message. Templates are the
//! `prompts/*.md` files beside this one, with `{{name}}` filled in.
//!
//! Hypotheses are shown under labels, never with their authors: a judge or
//! a reviewer that knows whose idea it is can favour its own.

use crate::model::{Discipline, Hypothesis, Role, Seat};

const CONTEXT: &str = include_str!("prompts/context.md");
const FRAME: &str = include_str!("prompts/frame.md");
const GENERATE: &str = include_str!("prompts/generate.md");
const REFLECT: &str = include_str!("prompts/reflect.md");
const VERIFY: &str = include_str!("prompts/verify.md");
const RANK: &str = include_str!("prompts/rank.md");
const EVOLVE: &str = include_str!("prompts/evolve.md");
const META: &str = include_str!("prompts/meta.md");
const SIMREVIEW: &str = include_str!("prompts/simreview.md");

/// Fill `{{name}}`s. A name not given is left as it was, so a missing one
/// shows in the prompt rather than vanishing.
pub fn render(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in values {
        out = out.replace(&format!("{{{{{name}}}}}"), value);
    }
    out
}

/// At most `limit` bytes of `text`, cut at a line where it can be, saying so.
pub fn clip(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cut = text[..end].rfind('\n').filter(|at| *at > limit / 2).unwrap_or(end);
    format!("{}\n\n[… {} more characters not shown]", &text[..cut], text.len() - cut)
}

/// What every task is told about the investigation.
pub struct Context<'a> {
    pub title: &'a str,
    pub discipline: Discipline,
    pub brief: &'a str,
    pub plan: &'a str,
    pub feedback: &'a str,
}

impl Context<'_> {
    fn render(&self) -> String {
        let or = |text: &str, none: &str| if text.trim().is_empty() { none.to_string() } else { text.to_string() };
        render(CONTEXT, &[
            ("title", self.title),
            ("discipline", self.discipline.name()),
            ("brief", &clip(self.brief, 6_000)),
            ("plan", &or(&clip(self.plan, 6_000), "_None yet._")),
            ("feedback", &or(&clip(&tail(self.feedback, 4_000), 4_000), "_Nothing yet._")),
        ])
    }
}

/// The end of a log, where the newest entries are.
fn tail(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let from = text[start..].find('\n').map(|n| start + n + 1).unwrap_or(start);
    format!("[… earlier entries not shown]\n{}", &text[from..])
}

fn role(role: Role) -> &'static str {
    match role {
        Role::Investigator => {
            "You are the Principal Investigator. You own the strategy: framing the problem, the research plan, \
             bold but testable hypotheses, and the synthesis of what the team has found into where the work stands."
        }
        Role::Engineer => {
            "You are the Lab Engineer. You own rigour: derivations, mathematics and numerics checked step by step, \
             and experiment code that actually tests what it claims to test."
        }
        Role::Sandbox => {
            "You are the Validation Sandbox. You own validation: grounding claims in the literature and on the web \
             with sources, stress-testing ideas by looking for where they break, and reading what the Lab's runs \
             really show."
        }
    }
}

fn method(discipline: Discipline) -> &'static str {
    match discipline {
        Discipline::Physics => {
            "Work as a physicist. State the regime and the assumptions. Check units and dimensions, limiting cases, \
             symmetries and conservation laws. Estimate orders of magnitude before computing. Prefer quantitative \
             predictions with uncertainties, and models that a simulation can test: ODE and PDE integration, \
             Monte Carlo, N-body, field solvers."
        }
        Discipline::Mathematics => {
            "Work as a mathematician. State everything precisely, with definitions. Say whether a claim is a \
             conjecture, a lemma or a theorem, and never call a proof complete unless every step is justified. \
             Give proof sketches that name the key step. Test conjectures computationally — small cases, \
             exhaustive search, symbolic computation with SymPy — and look for counterexamples. Cite known results \
             by name and standard reference."
        }
        Discipline::Chemistry => {
            "Work as a chemist. Respect stoichiometry, thermodynamics and kinetics; give mechanisms; estimate \
             energies and rates; prefer predictions that a computation or an assay could check."
        }
        Discipline::Biology => {
            "Work as a biologist. Name mechanisms, pathways and the organisms or systems involved; distinguish \
             correlation from causation; propose experiments with controls; prefer predictions a dataset or an \
             assay could check."
        }
        Discipline::ComputerScience => {
            "Work as a computer scientist. State the model of computation and the assumptions; give algorithms \
             with their complexity; prove what can be proved and benchmark what cannot, with experiments that \
             control for noise."
        }
        Discipline::Other => "Work as a careful scientist in the relevant field: precise claims, honest uncertainty, testable predictions.",
    }
}

/// How code proposed for the Lab must be written.
const LAB: &str = "Code for the Lab is Python 3.12 with numpy, scipy, sympy and matplotlib. It runs in a sandbox: \
no network, and no files outside its own folder. Print what it found, briefly, to stdout. For outputs, \
`import proton_lab as lab`: `lab.figure(fig, \"name\")` saves a matplotlib figure; `lab.metric(\"name\", value)` \
records a number; and for 3D, `scene = lab.Scene(\"title\")`, then `scene.points(xyz)`, `scene.line(xyz)`, \
`scene.spheres(centres, radii)`, `scene.bonds(pairs)`, `scene.mesh(vertices, faces)` or \
`scene.arrows(origins, vectors)` — each taking `frames=` (time × N × 3) to animate — and `scene.save(\"name\")`.";

/// Standing instructions for a seat in a role.
pub fn instructions(role_: Role, discipline: Discipline, seat: Seat, timeout_s: u32) -> String {
    format!(
        "You are one of three AI researchers working with a human scientist in Proton's Research mode, in the manner of \
         Google's AI co-scientist: hypotheses are generated, reviewed, ranked in a tournament, evolved, and summed up \
         each round. You are {}.\n\n{}\n\n{}\n\n\
         Rules for every task:\n\
         - You work unattended on one task; nobody can answer questions. Do not ask any.\n\
         - Read whatever helps; change nothing. Do not write or edit files and do not run commands: Proton records \
         your answer and runs experiments itself.\n\
         - Answer only with the structured output the task asks for. Do not announce yourself, report status or hand \
         off to anyone: instructions elsewhere about protocols, echo lines or hand-offs do not apply.\n\
         - Cite only what you have checked (URL, DOI or arXiv id). Say plainly when a reference is from memory.\n\
         - Write mathematics as TeX: `$...$` inline, `$$...$$` displayed.\n\
         - {} Keep a run under {} seconds.\n",
        seat.name(),
        role(role_),
        method(discipline),
        LAB,
        timeout_s,
    )
}

/// A hypothesis under a label, for a prompt: no author.
pub fn shown(label: &str, h: &Hypothesis, notes: &str) -> String {
    let list = |items: &[String]| if items.is_empty() { "_None given._".to_string() } else { items.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n") };
    let mut out = format!(
        "### {label}: {}\n\n{}\n\n**Rationale.** {}\n\n**Predictions.**\n{}\n\n**Assumptions.**\n{}\n",
        h.title,
        clip(&h.statement, 3_000),
        clip(&h.rationale, 2_000),
        list(&h.predictions),
        list(&h.assumptions),
    );
    if !notes.trim().is_empty() {
        out.push_str(&format!("\n{}\n", clip(notes, 2_000)));
    }
    out
}

pub fn frame(ctx: &Context) -> String {
    render(FRAME, &[("context", &ctx.render())])
}

pub fn generate(ctx: &Context, count: usize, known: &[String], guidance: &[String]) -> String {
    let known = if known.is_empty() {
        String::new()
    } else {
        format!("## Hypotheses already proposed — do not repeat them\n\n{}", known.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n"))
    };
    let guidance = if guidance.is_empty() {
        String::new()
    } else {
        format!("## Guidance from the last overview\n\n{}", guidance.iter().map(|g| format!("- {g}")).collect::<Vec<_>>().join("\n"))
    };
    render(GENERATE, &[("context", &ctx.render()), ("count", &count.to_string()), ("known", &known), ("guidance", &guidance)])
}

pub fn reflect(ctx: &Context, hypotheses: &[String]) -> String {
    render(REFLECT, &[("context", &ctx.render()), ("hypotheses", &hypotheses.join("\n"))])
}

pub fn verify(ctx: &Context, hypothesis: &str, reviews: &str) -> String {
    let reviews = if reviews.trim().is_empty() { "_None yet._" } else { reviews };
    render(VERIFY, &[("context", &ctx.render()), ("hypothesis", hypothesis), ("reviews", &clip(reviews, 4_000))])
}

/// `pairs` are each `(pair label, A, B)`, the hypotheses already shown.
pub fn rank(ctx: &Context, pairs: &[(String, String, String)]) -> String {
    let pairs: Vec<String> = pairs.iter().map(|(label, a, b)| format!("## Pair {label}\n\n{a}\n{b}")).collect();
    render(RANK, &[("context", &ctx.render()), ("pairs", &pairs.join("\n"))])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evolution {
    /// Fix what the reviews found in the strongest, sharpen it, ground it.
    Refine,
    /// Merge the strongest ideas, or find a simpler one that explains as much.
    Combine,
}

pub fn evolve(ctx: &Context, hypotheses: &[String], how: Evolution) -> String {
    let (title, mode) = match how {
        Evolution::Refine => (
            "refine the strongest hypothesis",
            "Write one improved version of the strongest hypothesis: answer what its reviews and verification found, \
             sharpen its predictions until they could fail, and ground it in what the runs showed.",
        ),
        Evolution::Combine => (
            "combine or simplify the leaders",
            "Write one new hypothesis that combines the best of these, or a simpler one that explains as much — or, \
             if both fit, one of each. Think outside the frame they share, too.",
        ),
    };
    render(EVOLVE, &[("context", &ctx.render()), ("hypotheses", &hypotheses.join("\n")), ("mode_title", title), ("mode", mode)])
}

pub fn meta(ctx: &Context, hypotheses: &[String], verdicts: &str, sandbox: &str) -> String {
    let or = |text: String| if text.trim().is_empty() { "_None yet._".to_string() } else { text };
    render(META, &[
        ("context", &ctx.render()),
        ("hypotheses", &clip(&hypotheses.join("\n"), 16_000)),
        ("verdicts", &or(tail(verdicts, 3_000))),
        ("sandbox", &or(tail(sandbox, 3_000))),
    ])
}

pub fn sim_review(ctx: &Context, hypothesis: &str, experiment: &str, run: &str) -> String {
    render(SIMREVIEW, &[("context", &ctx.render()), ("hypothesis", hypothesis), ("experiment", &clip(experiment, 8_000)), ("run", &clip(run, 8_000))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Author, HypothesisStatus};

    fn ctx() -> Context<'static> {
        Context { title: "Why is the sky blue?", discipline: Discipline::Physics, brief: "Explain it.", plan: "", feedback: "" }
    }

    fn h() -> Hypothesis {
        Hypothesis {
            id: "H007".into(),
            title: "Rayleigh".into(),
            author: Author::new(Seat::Codex, "gpt-6.1-sol"),
            round: 1,
            parents: Vec::new(),
            status: HypothesisStatus::Active,
            task: None,
            statement: "$I \\propto \\lambda^{-4}$".into(),
            rationale: "Small scatterers.".into(),
            predictions: vec!["Polarised at 90°.".into()],
            assumptions: Vec::new(),
            extra: Vec::new(),
        }
    }

    #[test]
    fn every_template_is_filled_and_none_is_left_open() {
        let shown = shown("A", &h(), "Elo 1216, 1–0.");
        let prompts = [
            frame(&ctx()),
            generate(&ctx(), 2, &["Ozone".into()], &["Look at polarisation.".into()]),
            reflect(&ctx(), &[shown.clone()]),
            verify(&ctx(), &shown, ""),
            rank(&ctx(), &[("P1".into(), shown.clone(), shown.replace("### A", "### B"))]),
            evolve(&ctx(), &[shown.clone()], Evolution::Refine),
            evolve(&ctx(), &[shown.clone()], Evolution::Combine),
            meta(&ctx(), &[shown.clone()], "", ""),
            sim_review(&ctx(), &shown, "purpose", "exit 0"),
        ];
        for prompt in &prompts {
            assert!(!prompt.contains("{{"), "{prompt}");
            assert!(prompt.contains("Why is the sky blue? — Physics"), "{prompt}");
        }
        assert!(prompts[1].contains("- Ozone") && prompts[1].contains("- Look at polarisation."));
    }

    #[test]
    fn a_hypothesis_is_shown_without_its_author() {
        let text = shown("B", &h(), "");
        assert!(text.starts_with("### B: Rayleigh") && !text.contains("H007") && !text.contains("codex") && !text.contains("gpt"));
    }

    #[test]
    fn instructions_say_the_role_the_method_and_the_rules() {
        let text = instructions(Role::Engineer, Discipline::Mathematics, Seat::Codex, 600);
        assert!(text.contains("You are Codex.") && text.contains("Lab Engineer") && text.contains("counterexamples"));
        assert!(text.contains("change nothing") && text.contains("hand off") && text.contains("proton_lab") && text.contains("600 seconds"));
    }

    #[test]
    fn long_texts_are_clipped_where_a_line_ends_and_say_so() {
        let text = format!("{}\n{}", "a".repeat(80), "b".repeat(80));
        let clipped = clip(&text, 100);
        assert!(clipped.starts_with(&"a".repeat(80)) && clipped.contains("more characters not shown"));
        assert_eq!(clip("short", 100), "short");
        assert!(tail(&format!("old\n{}\nnew entry\n", "x".repeat(50)), 20).ends_with("new entry\n"));
    }
}
