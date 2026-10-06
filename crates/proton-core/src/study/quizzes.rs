//! Quizzes and practice, in the open notebook: an attempt begun, each answer
//! marked as it is given — by rule, or by a call to another family of model
//! than the one that set the question ([`jobs::Marking`]) — a mark disputed
//! and marked again by yet another hand, an attempt finished, and how it
//! went objective by objective. What happened is the quiz's attempts log
//! ([`proton_study::quiz`]); everything drawn is replayed from it.
//!
//! An exam paper ([`proton_study::exam`]) can also be sat against the clock:
//! its answers are kept as drafts as they are written, apart from the log,
//! and nothing is marked until it is handed in — by the person, or by the
//! heartbeat when the time is up.

use std::collections::HashMap;

use proton_claude::Engine;
use proton_study::deck::Decks;
use proton_study::quiz::{self, Given, Line, Quiz};
use proton_study::studio::StudyMadeBody;
use proton_study::{exam, mastery, Book};

use super::jobs::Marking;

/// How long after its time a sitting still takes an answer: what was being
/// saved as the time ran out.
const GRACE: i64 = 30;
use super::Study;
use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;

/// The engine whose name a mark's `by` begins with.
fn engine_of(by: &str) -> Option<Engine> {
    Engine::ALL.into_iter().find(|e| by.starts_with(e.name()))
}

impl Study {
    /// A quiz, whole, as it stands.
    pub(super) fn quiz_event(&self, id: &str) -> Option<Event> {
        let open = self.open.as_ref()?;
        let book = &open.book;
        let (made, _) = book.made_one(id)?;
        let quiz = book.quiz(id)?;
        let attempts = quiz::attempts(&book.quiz_lines(id));
        let marking: Vec<String> = attempts
            .iter()
            .flat_map(|a| a.answers.iter().map(move |answer| (a.n, answer)))
            .filter(|(n, answer)| self.jobs.marking(book.dir(), id, *n, &answer.question))
            .map(|(n, answer)| format!("{n}/{}", answer.question))
            .collect();
        let objectives = match book.made_one("objectives") {
            Some((_, StudyMadeBody::Objectives { objectives })) => objectives,
            _ => Vec::new(),
        };
        let named: HashMap<String, (String, String)> = objectives.iter().map(|o| (o.id.clone(), (o.text.clone(), o.topic.clone()))).collect();
        let results = attempts.last().map(|a| quiz::results(&quiz, a, &named)).unwrap_or_default();
        // Where the objectives this quiz tests stand, from everything done.
        let tested: Vec<&String> = quiz.questions.iter().flat_map(|q| q.objectives.iter()).collect();
        let mut evidence = mastery::from_quizzes(book);
        evidence.extend(mastery::from_cards(&Decks::load(book, self.scheduler(book))));
        let of: Vec<_> = objectives.iter().filter(|o| tested.contains(&&o.id)).cloned().collect();
        let mastery = mastery::mastery(&of, &evidence, (self.clock)());
        let text: String = quiz
            .questions
            .iter()
            .map(|q| format!("{}\n{}\n{}\n{}\n", q.prompt, q.explanation, q.model_answer, q.cites.iter().map(|c| format!("[[{c}]]")).collect::<String>()))
            .collect();
        let drafts = attempts.last().filter(|a| a.finished.is_none() && a.ends.is_some()).map(|a| book.drafts(id, a.n)).unwrap_or_default();
        let text = format!("{text}\n{}", quiz.blueprint);
        let citations = proton_study::cite::resolve(book, &text);
        Some(Event::StudyQuiz {
            made,
            questions: quiz.questions,
            attempts,
            marking,
            results,
            mastery,
            citations,
            drafts,
            sections: quiz.sections,
            minutes: quiz.minutes,
            blueprint: quiz.blueprint,
            source: quiz.source,
        })
    }

    /// The quiz's events, it being the one looked at; a sitting of it under
    /// way is timed from here.
    fn quiz_shown(&mut self, id: &str) -> Vec<Event> {
        self.quiz = Some(id.to_string());
        if let Some(open) = &self.open {
            let sitting = quiz::attempts(&open.book.quiz_lines(id)).into_iter().rev().find(|a| a.finished.is_none() && a.ends.is_some());
            if let Some(a) = sitting {
                self.sitting = Some((id.to_string(), a.n, a.ends.unwrap_or_default()));
            }
        }
        match self.quiz_event(id) {
            Some(event) => vec![event],
            None => vec![notice(format!("{id}: no such quiz"), NoticeLevel::Warn)],
        }
    }

    pub(super) fn show_quiz(&mut self, id: &str) -> Vec<Event> {
        self.quiz_shown(id)
    }

    /// The open notebook, writable, and a quiz of it.
    fn quiz_of(&mut self, id: &str) -> Result<(Book, Quiz), Vec<Event>> {
        let book = self.writable()?.clone();
        let quiz = book.quiz(id).ok_or_else(|| vec![notice(format!("{id}: no such quiz"), NoticeLevel::Warn)])?;
        Ok((book, quiz))
    }

    pub(super) fn start_quiz(&mut self, id: &str, missed: bool) -> Vec<Event> {
        let (book, quiz) = match self.quiz_of(id) {
            Ok(found) => found,
            Err(events) => return events,
        };
        let attempts = quiz::attempts(&book.quiz_lines(id));
        let questions = match (missed, attempts.last()) {
            (true, Some(last)) => quiz::missed(last),
            _ => quiz.questions.iter().map(|q| q.id.clone()).collect(),
        };
        if questions.is_empty() {
            return vec![notice("Every question was got whole: there is nothing to try again.".into(), NoticeLevel::Info)];
        }
        let n = attempts.iter().map(|a| a.n).max().unwrap_or(0) + 1;
        if let Err(why) = book.log_quiz(id, &Line::Started { attempt: n, questions, at: (self.clock)(), ends: None }) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        self.quiz_shown(id)
    }

    pub(super) fn answer_quiz(&mut self, id: &str, attempt: u32, question: &str, given: Given, hinted: bool) -> Vec<Event> {
        let (book, quiz) = match self.quiz_of(id) {
            Ok(found) => found,
            Err(events) => return events,
        };
        let Some(asked) = quiz.question(question) else { return vec![notice(format!("{question}: no such question"), NoticeLevel::Warn)] };
        let Some(of) = quiz::attempts(&book.quiz_lines(id)).into_iter().find(|a| a.n == attempt) else {
            return vec![notice(format!("No attempt {attempt} at this quiz."), NoticeLevel::Warn)];
        };
        if of.finished.is_some() || !of.questions.iter().any(|q| q == question) {
            return vec![notice("That attempt is finished, or not of that question.".into(), NoticeLevel::Warn)];
        }
        let at = (self.clock)();
        // Against the clock: kept as a draft, marked when it is handed in.
        if let Some(ends) = of.ends {
            if at > ends + GRACE {
                return vec![notice("The time is up: the paper was handed in.".into(), NoticeLevel::Warn)];
            }
            if let Err(why) = book.save_draft(id, attempt, question, &given) {
                return vec![notice(why, NoticeLevel::Error)];
            }
            return self.quiz_shown(id);
        }
        let mut lines = vec![Line::Answered { attempt, question: question.to_string(), given: given.clone(), hinted, at }];
        let marked = quiz::mark(asked, &given);
        if let Some(grade) = &marked {
            lines.push(Line::Marked { attempt, question: question.to_string(), grade: grade.clone(), second: false, at });
        }
        for line in &lines {
            if let Err(why) = book.log_quiz(id, line) {
                return vec![notice(why, NoticeLevel::Error)];
            }
        }
        if marked.is_none() {
            self.queue_marking(&book, &quiz, Marking { quiz: id.to_string(), attempt, question: question.to_string(), second: false }, &[]);
        }
        self.quiz_shown(id)
    }

    pub(super) fn dispute(&mut self, id: &str, attempt: u32, question: &str, why: &str) -> Vec<Event> {
        let why = why.trim();
        if why.is_empty() {
            return vec![notice("Say why the mark is wrong.".into(), NoticeLevel::Warn)];
        }
        let (book, quiz) = match self.quiz_of(id) {
            Ok(found) => found,
            Err(events) => return events,
        };
        let answered = quiz::attempts(&book.quiz_lines(id)).into_iter().find(|a| a.n == attempt).and_then(|a| a.answer(question).cloned());
        let Some(first) = answered.as_ref().and_then(|a| a.grade.clone()).filter(|g| g.by_model()) else {
            return vec![notice("Only a mark a model made can be disputed.".into(), NoticeLevel::Warn)];
        };
        if answered.as_ref().is_some_and(|a| a.disputed.is_some()) {
            return vec![notice("That mark was disputed already.".into(), NoticeLevel::Warn)];
        }
        if let Err(why) = book.log_quiz(id, &Line::Disputed { attempt, question: question.to_string(), why: why.to_string(), at: (self.clock)() }) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let first_by: Vec<Engine> = engine_of(&first.by).into_iter().collect();
        self.queue_marking(&book, &quiz, Marking { quiz: id.to_string(), attempt, question: question.to_string(), second: true }, &first_by);
        self.quiz_shown(id)
    }

    pub(super) fn mark_again(&mut self, id: &str, attempt: u32, question: &str) -> Vec<Event> {
        let (book, quiz) = match self.quiz_of(id) {
            Ok(found) => found,
            Err(events) => return events,
        };
        let answered = quiz::attempts(&book.quiz_lines(id)).into_iter().find(|a| a.n == attempt).and_then(|a| a.answer(question).cloned());
        let Some(answered) = answered.filter(|a| a.unmarked.is_some()) else { return self.quiz_shown(id) };
        if self.jobs.marking(book.dir(), id, attempt, question) {
            return self.quiz_shown(id);
        }
        let second = answered.disputed.is_some();
        let avoid: Vec<Engine> = answered.grade.as_ref().filter(|_| second).and_then(|g| engine_of(&g.by)).into_iter().collect();
        self.queue_marking(&book, &quiz, Marking { quiz: id.to_string(), attempt, question: question.to_string(), second }, &avoid);
        self.quiz_shown(id)
    }

    pub(super) fn finish_quiz(&mut self, id: &str, attempt: u32) -> Vec<Event> {
        let (book, quiz) = match self.quiz_of(id) {
            Ok(found) => found,
            Err(events) => return events,
        };
        let Some(of) = quiz::attempts(&book.quiz_lines(id)).into_iter().find(|a| a.n == attempt) else { return self.quiz_shown(id) };
        if of.finished.is_some() {
            return self.quiz_shown(id);
        }
        let at = (self.clock)();
        // A sitting handed in: its drafts are its answers, and are marked now.
        let mut written = Vec::new();
        if of.ends.is_some() {
            for draft in book.drafts(id, attempt) {
                let Some(asked) = quiz.question(&draft.question) else { continue };
                let mut lines = vec![Line::Answered { attempt, question: draft.question.clone(), given: draft.given.clone(), hinted: false, at }];
                match quiz::mark(asked, &draft.given) {
                    Some(grade) => lines.push(Line::Marked { attempt, question: draft.question.clone(), grade, second: false, at }),
                    None => written.push(draft.question.clone()),
                }
                for line in &lines {
                    if let Err(why) = book.log_quiz(id, line) {
                        return vec![notice(why, NoticeLevel::Error)];
                    }
                }
            }
        }
        if let Err(why) = book.log_quiz(id, &Line::Finished { attempt, at }) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        book.forget_drafts(id, attempt);
        if self.sitting.as_ref().is_some_and(|(q, n, _)| q == id && *n == attempt) {
            self.sitting = None;
        }
        for question in written {
            self.queue_marking(&book, &quiz, Marking { quiz: id.to_string(), attempt, question, second: false }, &[]);
        }
        self.quiz_shown(id)
    }

    /// Sit an exam paper against the clock.
    pub(super) fn sit(&mut self, id: &str, minutes: u32) -> Vec<Event> {
        if !(1..=600).contains(&minutes) {
            return vec![notice("A sitting is a minute to ten hours long.".into(), NoticeLevel::Warn)];
        }
        let (book, quiz) = match self.quiz_of(id) {
            Ok(found) => found,
            Err(events) => return events,
        };
        let attempts = quiz::attempts(&book.quiz_lines(id));
        if let Some(open) = attempts.iter().find(|a| a.finished.is_none() && a.ends.is_some()) {
            self.sitting = Some((id.to_string(), open.n, open.ends.unwrap_or_default()));
            return self.quiz_shown(id);
        }
        let n = attempts.iter().map(|a| a.n).max().unwrap_or(0) + 1;
        let at = (self.clock)();
        let ends = at + i64::from(minutes) * 60;
        let questions = quiz.questions.iter().map(|q| q.id.clone()).collect();
        if let Err(why) = book.log_quiz(id, &Line::Started { attempt: n, questions, at, ends: Some(ends) }) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        self.sitting = Some((id.to_string(), n, ends));
        self.quiz_shown(id)
    }

    /// A sitting whose time is up is handed in.
    pub(super) fn poll_sitting(&mut self) -> Vec<Event> {
        let Some((id, attempt, ends)) = self.sitting.clone() else { return Vec::new() };
        if (self.clock)() < ends + GRACE {
            return Vec::new();
        }
        self.sitting = None;
        let mut events = vec![notice("The time is up: the paper was handed in, and is being marked.".into(), NoticeLevel::Info)];
        events.extend(self.finish_quiz(&id, attempt));
        events
    }

    /// Write an exam paper out: as Markdown, or a page to print; alone, or
    /// with its marking scheme.
    pub(super) fn export_exam(&mut self, id: &str, html: bool, scheme: bool) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let Some(quiz) = open.book.quiz(id) else { return vec![notice(format!("{id}: no such paper"), NoticeLevel::Warn)] };
        let text = match html {
            true => exam::html(&quiz, scheme),
            false => exam::markdown(&quiz, scheme),
        };
        let name = format!("{id}-{}{}.{}", proton_research::store::slug(&quiz.title), if scheme { "-scheme" } else { "" }, if html { "html" } else { "md" });
        let file = open.book.dir().join("exports").join(name);
        if let Err(why) = proton_research::store::atomic_write(&file, text.as_bytes()) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let then = if html { " Open it in a browser to print it." } else { "" };
        vec![notice(format!("Written to {}.{then}", crate::folder::tilde(&file)), NoticeLevel::Info)]
    }

    /// Mark a written answer: on an engine of another family than the one
    /// that set the quiz, and than any in `not`, when one is here — else the
    /// one that is.
    fn queue_marking(&mut self, book: &Book, quiz: &Quiz, marking: Marking, not: &[Engine]) {
        let setter = Engine::parse(&quiz.engine);
        let mut choice = None;
        for engine in [Engine::Codex, Engine::Claude, Engine::Antigravity] {
            if Some(engine) == setter || not.contains(&engine) || self.tutors.missing(engine).is_some() {
                continue;
            }
            choice = Some(engine);
            break;
        }
        let tutor = Engine::parse(&book.notebook.tutor.engine).unwrap_or(Engine::Claude);
        let engine = choice.or(setter.filter(|e| *e != Engine::Gemini)).unwrap_or(tutor);
        let model = if engine == tutor { book.notebook.tutor.model.clone() } else { String::new() };
        self.jobs.mark(book, marking, engine, model);
        let roots = self.kb.labelled_roots();
        self.jobs.poll(self.tutors.as_mut(), &roots);
    }
}
