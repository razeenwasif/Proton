//! Research: the project's investigations.
//!
//! Each is a folder under the project's `research/` (see
//! `proton_research::store`), read whenever it is shown, so what was edited
//! by hand — the brief, a hypothesis — is what the board shows. The round
//! that grows an investigation, and the Lab that runs its code, are driven
//! from here too.

use std::path::Path;

use proton_research::store::Store;
use proton_research::view::{self, Board};
use proton_research::Discipline;

use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;

pub struct Research {
    store: Store,
    /// The investigation on screen.
    open: Option<String>,
    /// The hypothesis shown whole.
    showing: Option<String>,
}

impl std::fmt::Debug for Research {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Research").field("root", &self.store.root()).field("open", &self.open).finish()
    }
}

impl Default for Research {
    fn default() -> Research {
        Research::new(Path::new("."))
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Research {
    pub fn new(root: &Path) -> Research {
        Research { store: Store::new(root.join("research")), open: None, showing: None }
    }

    /// Everything the Research tab draws.
    pub fn sync(&mut self) -> Vec<Event> {
        let folders = self.store.list();
        // The newest, until another is chosen; one that has gone since is let go.
        if self.open.as_ref().is_none_or(|slug| !folders.iter().any(|f| f.slug() == slug)) {
            self.open = folders.first().map(|f| f.slug().to_string());
            self.showing = None;
        }
        let mut events = vec![Event::ResearchList {
            investigations: folders.iter().map(view::summary).collect(),
            open: self.open.clone(),
        }];
        events.extend(self.board());
        events
    }

    /// The open investigation's board, and the hypothesis shown whole.
    fn board(&self) -> Vec<Event> {
        let Some(slug) = &self.open else { return Vec::new() };
        let Ok(folder) = self.store.open(slug) else { return Vec::new() };
        let board = Board::read(&folder);
        let i = folder.investigation();
        vec![
            Event::ResearchBoard {
                slug: slug.clone(),
                title: i.title.clone(),
                discipline: i.discipline,
                brief: folder.brief(),
                plan: folder.plan(),
                overview: folder.overview(),
                verdicts: folder.verdicts(),
                sandbox: folder.sandbox_log(),
                hypotheses: board.rows(),
            },
            Event::ResearchHypothesis { slug: slug.clone(), detail: self.showing.as_deref().and_then(|id| board.detail(id)) },
        ]
    }

    pub fn create(&mut self, title: &str, goal: &str, discipline: Discipline) -> Vec<Event> {
        match self.store.create(title, goal, discipline, now()) {
            Ok(folder) => {
                self.open = Some(folder.slug().to_string());
                self.showing = None;
                self.sync()
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    pub fn open(&mut self, slug: &str) -> Vec<Event> {
        match self.store.open(slug) {
            Ok(_) => {
                self.open = Some(slug.to_string());
                self.showing = None;
                self.sync()
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    pub fn show(&mut self, hypothesis: Option<String>) -> Vec<Event> {
        self.showing = hypothesis;
        self.board()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proton_research::model::{Author, Hypothesis, HypothesisStatus, Seat};

    fn root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-core-research-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_new_investigation_is_listed_opened_and_drawn() {
        let dir = root("new");
        let mut research = Research::new(&dir);
        assert!(matches!(research.sync().as_slice(), [Event::ResearchList { investigations, open: None }] if investigations.is_empty()));

        let events = research.create("Why is the sky blue?", "Explain the colour.", Discipline::Physics);
        let Some(Event::ResearchList { investigations, open }) = events.first() else { panic!("{events:?}") };
        assert_eq!((investigations.len(), open.as_deref()), (1, Some("why-is-the-sky-blue")));
        let board = events.iter().find_map(|e| match e {
            Event::ResearchBoard { title, brief, hypotheses, .. } => Some((title.clone(), brief.clone(), hypotheses.len())),
            _ => None,
        });
        let (title, brief, n) = board.expect("the board is drawn");
        assert_eq!((title.as_str(), n), ("Why is the sky blue?", 0));
        assert!(brief.contains("Explain the colour."));

        // A hypothesis written into the folder shows on the board, and whole when asked for.
        let folder = proton_research::Store::new(dir.join("research")).open("why-is-the-sky-blue").unwrap();
        folder
            .write_hypothesis(&Hypothesis {
                id: "H001".into(),
                title: "Rayleigh".into(),
                author: Author::new(Seat::Claude, "claude-opus-5-5"),
                round: 1,
                parents: Vec::new(),
                status: HypothesisStatus::Active,
                task: None,
                statement: "$I \\propto \\lambda^{-4}$".into(),
                rationale: "Small scatterers.".into(),
                predictions: Vec::new(),
                assumptions: Vec::new(),
                extra: Vec::new(),
            })
            .unwrap();
        let events = research.show(Some("H001".into()));
        assert!(events.iter().any(|e| matches!(e, Event::ResearchBoard { hypotheses, .. } if hypotheses.len() == 1)));
        assert!(events.iter().any(|e| matches!(e, Event::ResearchHypothesis { detail: Some(d), .. } if d.statement.contains("lambda"))));

        assert!(matches!(research.open("nope").as_slice(), [Event::Notice { .. }]));
        assert!(matches!(research.create("  ", "", Discipline::Physics).as_slice(), [Event::Notice { .. }]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
