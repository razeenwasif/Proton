//! Learning, in the open notebook: where the student stands objective by
//! objective, from everything they have done ([`proton_study::mastery`]);
//! a lesson on an objective, which is a chat with the tutor as a learning
//! guide begun on the objective and what its passages say; and a Learning
//! Overview opened ([`proton_study::overview`]).

use proton_study::deck::Decks;
use proton_study::studio::{StudyMadeBody, StudyObjective};
use proton_study::tutor::Style;
use proton_study::{clock, mastery, Book};

use super::tutor::Tutor;
use super::Study;
use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;

/// The notebook's learning objectives, if it has them.
pub(super) fn objectives(book: &Book) -> Vec<StudyObjective> {
    match book.made_one("objectives") {
        Some((_, StudyMadeBody::Objectives { objectives })) => objectives,
        _ => Vec::new(),
    }
}

impl Study {
    /// Where the student stands.
    pub(super) fn progress(&self) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let book = &open.book;
        let now = (self.clock)();
        let decks = Decks::load(book, self.scheduler(book));
        let mut evidence = mastery::from_quizzes(book);
        evidence.extend(mastery::from_cards(&decks));
        let objectives = mastery::mastery(&objectives(book), &evidence, now);
        let due = decks.counts(None, now, clock::today(now), self.limits()).due();
        vec![Event::StudyProgress { objectives, due, quizzes: mastery::quiz_rows(book) }]
    }

    /// A lesson on an objective: a new chat, the tutor a learning guide in
    /// it alone, begun on what the objective asks and where it is taught.
    pub(super) fn lesson(&mut self, id: &str) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book.clone(),
            Err(events) => return events,
        };
        let Some(objective) = objectives(&book).into_iter().find(|o| o.id == id) else {
            return vec![notice(format!("{id}: no such learning objective"), NoticeLevel::Warn)];
        };
        if let Some(mut was) = self.tutor.take() {
            was.stop();
        }
        let mut tutor = Tutor::new(&book);
        tutor.chat.settings.style = Style::Guide;
        tutor.chat.lesson = Some(objective.id.clone());
        tutor.chat.title = format!("Lesson: {}", objective.text.chars().take(70).collect::<String>());
        self.tutor = Some(tutor);
        let cites: String = objective.cites.iter().map(|c| format!("[[{c}]]")).collect::<Vec<_>>().join(" ");
        let course = match book.notebook.course.trim() {
            "" => String::new(),
            course => format!(" of {course}"),
        };
        let asked = format!(
            "Teach me this, from the sources: \"{}\" — learning objective {}{course}{}. Find out first what I know of it; then lead me \
             through it a step at a time, checking I have each step before the next. When I have it, say so.",
            objective.text,
            objective.id,
            if cites.is_empty() { String::new() } else { format!(", taught in {cites}") }
        );
        self.ask(&asked)
    }

    /// A Learning Overview, opened.
    pub(super) fn show_overview(&self, id: &str) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let book = &open.book;
        let (Some((made, _)), Some(overview)) = (book.made_one(id), book.overview(id)) else {
            return vec![notice(format!("{id}: no such overview"), NoticeLevel::Warn)];
        };
        let text: String = overview
            .topics
            .iter()
            .map(|t| {
                let cites = t.map.iter().flat_map(|n| n.cites.iter()).chain(t.cards.iter().flat_map(|c| c.cites.iter())).chain(t.check.iter().flat_map(|q| q.cites.iter()));
                format!("{}\n{}\n{}\n", t.summary, t.check.iter().map(|q| format!("{} {}", q.prompt, q.explanation)).collect::<String>(), cites.map(|c| format!("[[{c}]]")).collect::<String>())
            })
            .collect();
        let citations = proton_study::cite::resolve(book, &text);
        vec![Event::StudyOverview { made, topics: overview.topics, deck: overview.deck, quiz: overview.quiz, citations }]
    }
}
