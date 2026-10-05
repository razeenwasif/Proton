//! Investigations as folders: `research/<slug>/` in the project.
//!
//! ```text
//! README.md            the scientist's brief: goal, discipline, constraints
//! investigation.json   the seats, the budget, the Lab's policy
//! AGENTS.md, CLAUDE.md how the engines are to work here
//! RESEARCH_PLAN.md     the Principal Investigator's plan, rewritten each round
//! overview.md          the meta-review: where the investigation stands
//! VERDICT.md           the Lab Engineer's verifications, appended
//! SANDBOX_LOG.md       Lab runs and what they showed, appended
//! feedback.md          what the scientist told the team, appended
//! hypotheses/H007.md   one hypothesis each
//! reviews/H007.1.md    one review each
//! tournament.jsonl     one match a line
//! experiments/E003/    proposal.md and main.py
//! literature/P003.md   a paper the team found or the scientist added, and
//!                      P003.pdf beside it when an open copy was kept (not committed)
//! runs/R005/           what running an experiment gave
//! rounds/round-002.md  what each round did
//! tasks/T0031.json     every agent call, for provenance
//! .investigation/      machine state: the lock, caches (not committed)
//! ```

use std::path::{Path, PathBuf};

use crate::markdown;
use crate::model::{next_id, Discipline, Experiment, Hypothesis, Investigation, Match, Paper, Review};

/// What an investigation keeps out of Git: its machine state, what runs
/// write, and papers' PDFs — megabytes each, and other people's copyright.
pub const IGNORED: &str = ".investigation/\nruns/*/out/\nliterature/*.pdf\n";

/// Write a file whole or not at all: to a neighbour, then renamed over it.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let partial = path.with_file_name(format!(".{name}.partial"));
    std::fs::write(&partial, bytes).map_err(|e| format!("{}: {e}", partial.display()))?;
    std::fs::rename(&partial, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Add to the end of a file, making it if there is none.
pub fn append(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// A folder name from a title: lower case, words joined by hyphens, at most
/// six words, nothing a filesystem anywhere would refuse.
pub fn slug(title: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(6)
        .map(|w| w.to_lowercase())
        .collect();
    let slug = words.join("-");
    if slug.is_empty() {
        "investigation".into()
    } else {
        slug
    }
}

/// The folder all of a project's investigations are in.
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every investigation here, the newest first. A folder whose
    /// `investigation.json` will not read is passed over.
    pub fn list(&self) -> Vec<Folder> {
        let Ok(entries) = std::fs::read_dir(&self.root) else { return Vec::new() };
        let mut out: Vec<Folder> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| Folder::open(&entry.path()).ok())
            .collect();
        out.sort_by(|a, b| b.investigation.created.cmp(&a.investigation.created).then_with(|| a.slug.cmp(&b.slug)));
        out
    }

    pub fn open(&self, slug: &str) -> Result<Folder, String> {
        if slug.is_empty() || slug.contains(['/', '\\']) || slug.starts_with('.') {
            return Err(format!("{slug}: not an investigation"));
        }
        Folder::open(&self.root.join(slug))
    }

    /// A new investigation, in a folder of its own named after its title.
    pub fn create(&self, title: &str, goal: &str, discipline: Discipline, now: u64) -> Result<Folder, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("An investigation needs a title.".into());
        }
        let base = slug(title);
        let mut name = base.clone();
        let mut n = 2;
        while self.root.join(&name).exists() {
            name = format!("{base}-{n}");
            n += 1;
        }
        let dir = self.root.join(&name);
        let investigation = Investigation::new(title, discipline, now);
        let files: [(&str, String); 9] = [
            ("README.md", brief(title, goal, discipline)),
            ("AGENTS.md", PROTOCOL.to_string()),
            ("CLAUDE.md", "@AGENTS.md\n".to_string()),
            ("RESEARCH_PLAN.md", "# Research plan\n\n_Written by the Principal Investigator in the first round, and rewritten after every overview._\n".into()),
            ("overview.md", "# Overview\n\n_Where the investigation stands: written by the meta-review at the end of each round._\n".into()),
            ("VERDICT.md", "# Verdicts\n\n_The Lab Engineer's verifications, newest last._\n".into()),
            ("SANDBOX_LOG.md", "# Sandbox log\n\n_Lab runs and what they showed, newest last._\n".into()),
            ("feedback.md", "# Feedback\n\n_What you told the team, newest last._\n".into()),
            (".gitignore", IGNORED.into()),
        ];
        for (name, text) in files {
            atomic_write(&dir.join(name), text.as_bytes())?;
        }
        for sub in ["hypotheses", "reviews", "experiments", "literature", "runs", "rounds", "tasks", ".investigation"] {
            std::fs::create_dir_all(dir.join(sub)).map_err(|e| format!("{}: {e}", dir.join(sub).display()))?;
        }
        let mut folder = Folder { dir, slug: name, investigation: investigation.clone() };
        folder.save(investigation)?;
        Ok(folder)
    }
}

fn brief(title: &str, goal: &str, discipline: Discipline) -> String {
    let goal = match goal.trim() {
        "" => "_What you want found out. Edit this file: the team reads it before every task._",
        goal => goal,
    };
    format!(
        "# {title}\n\n**Discipline:** {}\n\n## Goal\n\n{goal}\n\n## Constraints\n\n_Anything the team must keep to: assumptions, \
         methods to prefer or avoid, what is already known._\n",
        discipline.name()
    )
}

/// How the engines are to work in an investigation's folder. Codex reads
/// `AGENTS.md` where it runs, Claude Code `CLAUDE.md` (which points here);
/// the role and the task arrive with each call.
pub const PROTOCOL: &str = "# How to work in this investigation

This folder is an investigation run by Proton's Research mode: a team of AI
researchers — Claude Code as Principal Investigator, Codex as Lab Engineer,
Antigravity as Validation Sandbox — working with a human scientist, in the
manner of Google's AI co-scientist (generate, review, rank by tournament,
evolve, meta-review).

- You are working unattended on one task. Nobody can answer questions.
- Read anything here or in the project; change nothing. Do not write or edit
  files and do not run commands. Proton writes every file from your answer.
- Answer only with the structured output your task asks for. Do not announce
  yourself, report status, or hand off to another agent: instructions
  elsewhere about protocols, echo lines or hand-offs do not apply here.
- This file is the whole of your standing instructions here. Any AGENTS.md,
  CLAUDE.md or GEMINI.md in a folder above this one, or in your own global
  settings, is written for other work and does not apply.
- Experiments are Python code inside your answer. Proton's Lab runs them in a
  sandbox (no network, no access outside the run's folder) after the
  scientist approves, and the results come back in a later task.
- Cite only sources you have actually checked; give a URL, DOI or arXiv id,
  and say plainly when a reference is from memory and unverified.
- `literature/` holds the papers the team has gathered: `P003.md` is what the
  sources say about one, with its abstract, and `P003.pdf` is its text when an
  open copy was kept. Cite a paper as P003, and a page of it as
  [[literature/P003.pdf p. 4]].

`README.md` is the scientist's brief and outranks everything else here.
";

/// One investigation's folder.
#[derive(Debug, Clone)]
pub struct Folder {
    dir: PathBuf,
    slug: String,
    investigation: Investigation,
}

impl Folder {
    pub fn open(dir: &Path) -> Result<Folder, String> {
        let path = dir.join("investigation.json");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let investigation: Investigation = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let slug = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        Ok(Folder { dir: dir.to_path_buf(), slug, investigation })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn investigation(&self) -> &Investigation {
        &self.investigation
    }

    pub fn save(&mut self, investigation: Investigation) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&investigation).map_err(|e| e.to_string())? + "\n";
        atomic_write(&self.dir.join("investigation.json"), text.as_bytes())?;
        self.investigation = investigation;
        Ok(())
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }

    pub fn brief(&self) -> String {
        self.read("README.md")
    }
    pub fn plan(&self) -> String {
        self.read("RESEARCH_PLAN.md")
    }
    pub fn overview(&self) -> String {
        self.read("overview.md")
    }
    pub fn verdicts(&self) -> String {
        self.read("VERDICT.md")
    }
    pub fn sandbox_log(&self) -> String {
        self.read("SANDBOX_LOG.md")
    }
    pub fn feedback(&self) -> String {
        self.read("feedback.md")
    }

    /// Markdown files in a subfolder, in name order, read with `parse`;
    /// one that will not parse is passed over rather than stopping the rest.
    fn each<T>(&self, sub: &str, parse: impl Fn(&str) -> Result<T, String>) -> Vec<T> {
        let Ok(entries) = std::fs::read_dir(self.dir.join(sub)) else { return Vec::new() };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "md")).collect();
        paths.sort();
        paths.iter().filter_map(|p| parse(&std::fs::read_to_string(p).ok()?).ok()).collect()
    }

    pub fn hypotheses(&self) -> Vec<Hypothesis> {
        self.each("hypotheses", markdown::parse_hypothesis)
    }

    pub fn reviews(&self) -> Vec<Review> {
        self.each("reviews", markdown::parse_review)
    }

    /// The tournament, in the order it was played. A line that will not
    /// read is passed over.
    pub fn matches(&self) -> Vec<Match> {
        self.read("tournament.jsonl").lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    }

    /// The papers in `literature/`, each knowing whether its PDF is beside it.
    pub fn papers(&self) -> Vec<Paper> {
        let mut papers = self.each("literature", markdown::parse_paper);
        for paper in &mut papers {
            paper.has_pdf = self.dir.join("literature").join(format!("{}.pdf", paper.id)).is_file();
        }
        papers
    }

    pub fn write_paper(&self, p: &Paper) -> Result<(), String> {
        atomic_write(&self.dir.join("literature").join(format!("{}.md", p.id)), markdown::render_paper(p).as_bytes())
    }

    /// The next paper's id, counting a PDF left on its own too, so that a
    /// new paper never takes over somebody's file.
    pub fn next_paper(&self) -> String {
        let stems: Vec<String> = std::fs::read_dir(self.dir.join("literature"))
            .map(|entries| entries.flatten().filter_map(|e| e.path().file_stem()?.to_str().map(str::to_string)).collect())
            .unwrap_or_default();
        next_id("P", stems.iter().map(String::as_str))
    }

    /// Where a paper's PDF is, or would be: only for an id that is a
    /// paper's, so a name from elsewhere cannot point outside the folder.
    pub fn paper_pdf(&self, id: &str) -> Result<PathBuf, String> {
        let digits = id.strip_prefix('P').unwrap_or("");
        if digits.len() < 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("{id}: not a paper"));
        }
        Ok(self.dir.join("literature").join(format!("{id}.pdf")))
    }

    /// Make sure `.gitignore` has `line`, for an investigation made before it
    /// was one of [`IGNORED`]'s. Whatever else is there is kept.
    pub fn ensure_ignored(&self, line: &str) -> Result<(), String> {
        let path = self.dir.join(".gitignore");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.lines().any(|l| l.trim() == line) {
            return Ok(());
        }
        let joined = if text.is_empty() || text.ends_with('\n') { format!("{text}{line}\n") } else { format!("{text}\n{line}\n") };
        atomic_write(&path, joined.as_bytes())
    }

    pub fn experiments(&self) -> Vec<Experiment> {
        let Ok(entries) = std::fs::read_dir(self.dir.join("experiments")) else { return Vec::new() };
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        dirs.iter()
            .filter_map(|dir| {
                let proposal = std::fs::read_to_string(dir.join("proposal.md")).ok()?;
                let code = std::fs::read_to_string(dir.join("main.py")).unwrap_or_default();
                markdown::parse_experiment(&proposal, &code).ok()
            })
            .collect()
    }

    pub fn next_hypothesis(&self) -> String {
        let ids: Vec<String> = self.hypotheses().into_iter().map(|h| h.id).collect();
        next_id("H", ids.iter().map(String::as_str))
    }

    /// `H007.1`, `H007.2`…: a hypothesis's reviews are numbered under it.
    pub fn next_review(&self, hypothesis: &str) -> String {
        let n = self.reviews().iter().filter(|r| r.hypothesis == hypothesis).count();
        format!("{hypothesis}.{}", n + 1)
    }

    pub fn next_experiment(&self) -> String {
        let ids: Vec<String> = self.experiments().into_iter().map(|e| e.id).collect();
        next_id("E", ids.iter().map(String::as_str))
    }

    /// The next of a kind of file in a subfolder: `tasks` → `T0031`.
    pub fn next_in(&self, sub: &str, prefix: &str) -> String {
        let names: Vec<String> = std::fs::read_dir(self.dir.join(sub))
            .map(|entries| entries.flatten().filter_map(|e| e.file_name().to_str().map(|n| n.split('.').next().unwrap_or("").to_string())).collect())
            .unwrap_or_default();
        let next = next_id(prefix, names.iter().map(String::as_str));
        // Tasks run to four digits from the start: a long investigation has hundreds.
        match prefix {
            "T" => format!("T{:04}", next[1..].parse::<u32>().unwrap_or(1)),
            _ => next,
        }
    }

    pub fn write_hypothesis(&self, h: &Hypothesis) -> Result<(), String> {
        atomic_write(&self.dir.join("hypotheses").join(format!("{}.md", h.id)), markdown::render_hypothesis(h).as_bytes())
    }

    pub fn write_review(&self, r: &Review) -> Result<(), String> {
        atomic_write(&self.dir.join("reviews").join(format!("{}.md", r.id)), markdown::render_review(r).as_bytes())
    }

    pub fn write_experiment(&self, e: &Experiment) -> Result<(), String> {
        let dir = self.dir.join("experiments").join(&e.id);
        atomic_write(&dir.join("main.py"), e.code.as_bytes())?;
        atomic_write(&dir.join("proposal.md"), markdown::render_experiment(e).as_bytes())
    }

    pub fn add_match(&self, m: &Match) -> Result<(), String> {
        append(&self.dir.join("tournament.jsonl"), &(serde_json::to_string(m).map_err(|e| e.to_string())? + "\n"))
    }

    pub fn write_plan(&self, text: &str) -> Result<(), String> {
        atomic_write(&self.dir.join("RESEARCH_PLAN.md"), text.as_bytes())
    }

    pub fn write_overview(&self, text: &str) -> Result<(), String> {
        atomic_write(&self.dir.join("overview.md"), text.as_bytes())
    }

    pub fn add_verdict(&self, entry: &str) -> Result<(), String> {
        append(&self.dir.join("VERDICT.md"), &format!("\n{}\n", entry.trim()))
    }

    pub fn add_sandbox_log(&self, entry: &str) -> Result<(), String> {
        append(&self.dir.join("SANDBOX_LOG.md"), &format!("\n{}\n", entry.trim()))
    }

    /// What the scientist said, under a heading saying when.
    pub fn add_feedback(&self, text: &str, when: &str) -> Result<(), String> {
        append(&self.dir.join("feedback.md"), &format!("\n## {when}\n\n{}\n", text.trim()))
    }

    pub fn write_round(&self, round: u32, text: &str) -> Result<(), String> {
        atomic_write(&self.dir.join("rounds").join(format!("round-{round:03}.md")), text.as_bytes())
    }

    /// An agent call's record: what it was asked, what it answered, who
    /// and when — the provenance every hypothesis and review points at.
    pub fn write_task(&self, id: &str, record: &serde_json::Value) -> Result<(), String> {
        let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())? + "\n";
        atomic_write(&self.dir.join("tasks").join(format!("{id}.json")), text.as_bytes())
    }

    /// Machine state kept between runs of Proton, not committed.
    pub fn state_path(&self, name: &str) -> PathBuf {
        self.dir.join(".investigation").join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Author, HypothesisStatus, Seat};

    fn root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-research-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn hypothesis(id: &str) -> Hypothesis {
        Hypothesis {
            id: id.into(),
            title: format!("Idea {id}"),
            author: Author::new(Seat::Claude, "claude-opus-5-5"),
            round: 1,
            parents: Vec::new(),
            status: HypothesisStatus::Active,
            task: None,
            statement: "It is so.".into(),
            rationale: "Because.".into(),
            predictions: vec!["This.".into()],
            assumptions: Vec::new(),
            extra: Vec::new(),
        }
    }

    #[test]
    fn a_new_investigation_is_a_folder_with_its_brief_and_protocol() {
        let dir = root("new");
        let store = Store::new(&dir);
        let folder = store.create("Why is the sky blue?", "Explain the colour of the daytime sky.", Discipline::Physics, 100).unwrap();
        assert_eq!(folder.slug(), "why-is-the-sky-blue");
        assert!(folder.brief().contains("## Goal\n\nExplain the colour of the daytime sky."));
        assert!(folder.dir().join("AGENTS.md").exists() && folder.dir().join("hypotheses").is_dir());
        assert_eq!(std::fs::read_to_string(folder.dir().join(".gitignore")).unwrap(), ".investigation/\nruns/*/out/\nliterature/*.pdf\n");
        assert!(folder.dir().join("literature").is_dir());
        // The same title again gets a folder of its own.
        let again = store.create("Why is the sky blue?", "", Discipline::Physics, 200).unwrap();
        assert_eq!(again.slug(), "why-is-the-sky-blue-2");
        let listed: Vec<String> = store.list().into_iter().map(|f| f.slug().to_string()).collect();
        assert_eq!(listed, ["why-is-the-sky-blue-2", "why-is-the-sky-blue"], "newest first");
        assert!(store.open("../etc").is_err() && store.open(".investigation").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_is_written_is_read_back_and_numbered_on() {
        let dir = root("rw");
        let folder = Store::new(&dir).create("Primes", "", Discipline::Mathematics, 1).unwrap();
        assert_eq!(folder.next_hypothesis(), "H001");
        folder.write_hypothesis(&hypothesis("H001")).unwrap();
        folder.write_hypothesis(&hypothesis("H002")).unwrap();
        assert_eq!(folder.hypotheses().len(), 2);
        assert_eq!(folder.next_hypothesis(), "H003");
        assert_eq!(folder.next_review("H002"), "H002.1");
        assert_eq!(folder.next_in("tasks", "T"), "T0001");
        folder.write_task("T0001", &serde_json::json!({ "phase": "generate" })).unwrap();
        assert_eq!(folder.next_in("tasks", "T"), "T0002");

        let m = Match {
            round: 1,
            a: "H001".into(),
            b: "H002".into(),
            judge: Author::new(Seat::Codex, ""),
            winner: "H002".into(),
            rationale: "Sharper prediction.".into(),
            swapped: true,
            task: None,
            at: 5,
        };
        folder.add_match(&m).unwrap();
        crate::store::append(&folder.dir().join("tournament.jsonl"), "not json\n").unwrap();
        assert_eq!(folder.matches(), vec![m], "a broken line is passed over");

        folder.add_feedback("Focus on polarisation.", "Round 1").unwrap();
        assert!(folder.feedback().ends_with("## Round 1\n\nFocus on polarisation.\n"));
        // A hypothesis file broken by hand does not hide the others.
        std::fs::write(folder.dir().join("hypotheses").join("H009.md"), "no front matter").unwrap();
        assert_eq!(folder.hypotheses().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn papers_are_numbered_past_any_pdf_and_their_pdfs_kept_out_of_git() {
        let dir = root("papers");
        let folder = Store::new(&dir).create("Sky", "", Discipline::Physics, 1).unwrap();
        assert_eq!(folder.next_paper(), "P001");
        let paper = |id: &str| Paper { id: id.into(), record: crate::model::Record { title: format!("Paper {id}"), ..Default::default() }, ..Default::default() };
        folder.write_paper(&paper("P001")).unwrap();
        // A PDF the scientist dropped in on its own still takes its number.
        std::fs::write(folder.dir().join("literature").join("P002.pdf"), b"%PDF-1.4").unwrap();
        assert_eq!(folder.next_paper(), "P003");
        folder.write_paper(&paper("P002")).unwrap();
        let papers = folder.papers();
        assert_eq!(papers.iter().map(|p| (p.id.as_str(), p.has_pdf)).collect::<Vec<_>>(), [("P001", false), ("P002", true)]);
        assert!(folder.paper_pdf("P002").unwrap().ends_with("literature/P002.pdf"));
        for bad in ["../x", "P1", "P00a", "H001", ""] {
            assert!(folder.paper_pdf(bad).is_err(), "{bad}");
        }

        // An investigation from before: its .gitignore gains the line once,
        // and keeps what somebody added to it.
        std::fs::write(folder.dir().join(".gitignore"), ".investigation/\nruns/*/out/\nnotes/private").unwrap();
        folder.ensure_ignored("literature/*.pdf").unwrap();
        folder.ensure_ignored("literature/*.pdf").unwrap();
        assert_eq!(std::fs::read_to_string(folder.dir().join(".gitignore")).unwrap(), ".investigation/\nruns/*/out/\nnotes/private\nliterature/*.pdf\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_slug_is_a_few_safe_words() {
        assert_eq!(slug("  Riemann's ζ(s): zeros on ½?  "), "riemann-s-ζ-s-zeros-on");
        assert_eq!(slug("???"), "investigation");
    }
}
