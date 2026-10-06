//! Flashcards, in the open notebook: a deck opened, a session of review
//! card by card, cards written by hand, a deck written out for Anki.
//!
//! A card's schedule is its deck's log ([`proton_study::deck`]): each rating
//! is written there as it is made, and everything drawn is replayed from it,
//! so a deck edited or a log appended to by another hand is what is drawn
//! the next time.

use std::collections::HashMap;

use proton_study::deck::{self, CardKind, Decks, Limits, Mode, Session, StudyCard};
use proton_study::fsrs::{Rating, Scheduler};
use proton_study::view::{card_state, StudyCardRow, StudyReview, StudyReviewCard};
use proton_study::{clock, Book};
use serde_json::Value;

use super::Study;
use crate::bus::{Event, NoticeLevel};
use crate::workspace::notice;

/// The longest a card counts as looked at: past it, the person was away.
const LONGEST_LOOK: u64 = 10 * 60 * 1000;

fn now() -> i64 {
    proton_study::now() as i64
}

impl Study {
    /// The scheduler for a notebook: the person's retention, and its exam's day.
    pub(super) fn scheduler(&self, book: &Book) -> Scheduler {
        let retention = self.settings().and_then(|s| s.get("study.srs.retention").and_then(Value::as_f64)).filter(|r| (0.7..=0.99).contains(r)).unwrap_or(0.9);
        Scheduler { retention, exam: book.notebook.exam.as_deref().and_then(clock::exam_day), ..Scheduler::default() }
    }

    /// How many new cards and reviews a day: the person's, or twenty and two hundred.
    pub(super) fn limits(&self) -> Limits {
        let settings = self.settings();
        let number = |key: &str, fallback: usize| settings.as_ref().and_then(|s| s.get(key).and_then(Value::as_u64)).map_or(fallback, |n| n as usize);
        let usual = Limits::default();
        Limits { new: number("study.srs.new", usual.new), reviews: number("study.srs.reviews", usual.reviews) }
    }

    /// How many cards a notebook has due today.
    pub(super) fn due_today(&self, book: &Book) -> usize {
        if book.deck_ids().is_empty() {
            return 0;
        }
        let now = now();
        Decks::load(book, self.scheduler(book)).counts(None, now, clock::today(now), self.limits()).due()
    }

    /// A deck, whole: its cards, where each is in being learnt, and what of
    /// it is due.
    pub(super) fn deck_event(&self, id: &str) -> Option<Event> {
        let open = self.open.as_ref()?;
        let book = &open.book;
        let (made, _) = book.made_one(id)?;
        let decks = Decks::load(book, self.scheduler(book));
        let loaded = decks.loaded(id)?;
        let now = now();
        let cards = loaded
            .deck
            .cards
            .iter()
            .filter_map(|card| {
                let memory = loaded.memories.get(&card.id)?;
                let learnt = memory.reviews > 0;
                Some(StudyCardRow {
                    card: card.clone(),
                    state: card_state(memory),
                    due: learnt.then_some(memory.due),
                    recall: learnt.then(|| decks.scheduler.retrievability(memory, now)),
                    reviews: memory.reviews,
                    missed: loaded.log.iter().filter(|r| r.card == card.id && r.rating == Rating::Again).count() as u32,
                })
            })
            .collect();
        let counts = decks.counts(Some(id), now, clock::today(now), self.limits());
        let text: String = loaded.deck.cards.iter().map(card_text).collect();
        let citations = proton_study::cite::resolve(book, &text);
        Some(Event::StudyDeck { made, cards, counts, citations })
    }

    /* ── a session ──────────────────────────────────────────────────── */

    pub(super) fn start_review(&mut self, deck: Option<String>, mode: Mode, shuffle: bool) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book.clone(),
            Err(events) => return events,
        };
        let decks = Decks::load(&book, self.scheduler(&book));
        if let Some(id) = &deck {
            if decks.loaded(id).is_none() {
                return vec![notice(format!("{id}: no such deck"), NoticeLevel::Warn)];
            }
        }
        if decks.decks.is_empty() {
            return vec![notice("There are no flashcards yet: make some in the studio.".into(), NoticeLevel::Info)];
        }
        self.review = Some(Session::start(&decks, deck, mode, shuffle, now()));
        self.review_event(&book, &decks)
    }

    /// The session as it stands, its next card chosen.
    fn review_event(&mut self, book: &Book, decks: &Decks) -> Vec<Event> {
        let limits = self.limits();
        let Some(session) = &mut self.review else { return vec![Event::StudyReviewing { review: None }] };
        let now = now();
        let day = clock::today(now);
        let (next, wait) = match session.next(decks, now, day, limits) {
            Ok(next) => (next, None),
            Err(at) => (None, Some(at)),
        };
        let card = next.as_ref().and_then(|at| {
            let (card, memory) = decks.get(at)?;
            Some(StudyReviewCard {
                deck: at.deck.clone(),
                deck_title: decks.loaded(&at.deck).map(|d| d.deck.title.clone()).unwrap_or_default(),
                card: card.clone(),
                state: card_state(memory),
                intervals: decks.intervals(at, now)?.to_vec(),
            })
        });
        let citations = card.as_ref().map(|c| proton_study::cite::resolve(book, &card_text(&c.card))).unwrap_or_default();
        let title = session.scope.as_deref().and_then(|id| decks.loaded(id)).map_or_else(|| book.notebook.title.clone(), |d| d.deck.title.clone());
        let review = StudyReview {
            deck: session.scope.clone(),
            title,
            mode: session.mode,
            shuffle: session.shuffle,
            card,
            citations,
            left: decks.counts(session.scope.as_deref(), now, day, limits),
            remaining: session.remaining(),
            done: session.done,
            again: session.again,
            wait,
            undo: !session.history.is_empty(),
        };
        vec![Event::StudyReviewing { review: Some(review) }]
    }

    /// A session waiting on a card being learnt shows it, once it can.
    pub(super) fn poll_review(&mut self) -> Vec<Event> {
        let Some(at) = self.review.as_ref().and_then(|s| s.waiting) else { return Vec::new() };
        let Some(open) = &self.open else { return Vec::new() };
        if now() < at {
            return Vec::new();
        }
        let book = open.book.clone();
        let decks = Decks::load(&book, self.scheduler(&book));
        self.review_event(&book, &decks)
    }

    pub(super) fn rate(&mut self, rating: Rating, took: u64) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book.clone(),
            Err(events) => return events,
        };
        let Some(session) = &self.review else { return Vec::new() };
        let Some(card) = session.current.clone() else { return Vec::new() };
        let mode = session.mode;
        let mut decks = Decks::load(&book, self.scheduler(&book));
        let at = now();
        if let Err(why) = decks.rate(&book, &card, rating, at, took.min(LONGEST_LOOK), mode) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        if let Some(session) = &mut self.review {
            session.rated(&card, rating, at);
        }
        self.review_event(&book, &decks)
    }

    pub(super) fn undo_review(&mut self) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book.clone(),
            Err(events) => return events,
        };
        let Some(session) = &mut self.review else { return Vec::new() };
        let Some((card, at)) = session.undone() else { return Vec::new() };
        let mut decks = Decks::load(&book, self.scheduler(&book));
        if let Err(why) = decks.undo(&book, &card, at) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        self.review_event(&book, &decks)
    }

    /// The session over: the list's counts, and the deck it was of, again.
    pub(super) fn end_review(&mut self) -> Vec<Event> {
        let scope = self.review.take().and_then(|s| s.scope);
        let mut events = vec![Event::StudyReviewing { review: None }, self.list_event()];
        events.extend(scope.and_then(|id| self.deck_event(&id)));
        events
    }

    /* ── cards by hand ──────────────────────────────────────────────── */

    pub(super) fn save_card(&mut self, id: &str, card: Option<String>, kind: CardKind, front: &str, back: &str, topic: &str) -> Vec<Event> {
        let (front, back, topic) = (front.trim(), back.trim(), topic.trim());
        let wrong = match kind {
            _ if front.is_empty() => Some("A card needs a front."),
            CardKind::Basic if back.is_empty() => Some("A card needs a back: the answer."),
            CardKind::Cloze if deck::deletions(front).is_empty() => Some("A cloze card hides something: write it as {{c1::what is hidden}}."),
            _ => None,
        };
        if let Some(wrong) = wrong {
            return vec![notice(wrong.into(), NoticeLevel::Warn)];
        }
        let book = match self.writable() {
            Ok(book) => book.clone(),
            Err(events) => return events,
        };
        let Some(mut deck) = book.deck(id) else { return vec![notice(format!("{id}: no such deck"), NoticeLevel::Warn)] };
        let now = proton_study::now();
        match card {
            Some(card) => {
                let Some(found) = deck.cards.iter_mut().find(|c| c.id == card) else {
                    return vec![notice(format!("{card}: no such card"), NoticeLevel::Warn)];
                };
                found.kind = kind;
                found.front = front.to_string();
                found.back = back.to_string();
                found.topic = topic.to_string();
                found.edited = Some(now);
            }
            None => {
                deck.take(StudyCard {
                    id: String::new(),
                    kind,
                    front: front.to_string(),
                    back: back.to_string(),
                    topic: topic.to_string(),
                    objectives: Vec::new(),
                    cites: Vec::new(),
                    task: String::new(),
                    added: now,
                    edited: None,
                });
            }
        }
        if let Err(why) = book.save_deck(id, &deck) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        self.deck_event(id).into_iter().collect()
    }

    /// Take a card out. Its reviews stay in the log, read past: a card no
    /// longer in its deck is not replayed.
    pub(super) fn delete_card(&mut self, id: &str, card: &str) -> Vec<Event> {
        let book = match self.writable() {
            Ok(book) => book.clone(),
            Err(events) => return events,
        };
        let Some(mut deck) = book.deck(id) else { return vec![notice(format!("{id}: no such deck"), NoticeLevel::Warn)] };
        deck.cards.retain(|c| c.id != card);
        if let Err(why) = book.save_deck(id, &deck) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        if let Some(session) = &mut self.review {
            session.left.retain(|c| !(c.deck == id && c.card == card));
        }
        self.deck_event(id).into_iter().collect()
    }

    /// Write a deck out into the notebook's `exports/`: for Anki, its cards
    /// as notes of its own kinds, its TeX as MathJax; or as CSV.
    pub(super) fn export_deck(&self, id: &str, anki: bool) -> Vec<Event> {
        let Some(open) = &self.open else { return Vec::new() };
        let book = &open.book;
        let Some(deck) = book.deck(id) else { return vec![notice(format!("{id}: no such deck"), NoticeLevel::Warn)] };
        let titles: HashMap<String, String> = book.sources.sources.iter().map(|s| (s.id.clone(), s.title.clone())).collect();
        let (text, ext) = match anki {
            true => (deck::anki(&deck, &format!("Proton::{}::{}", book.notebook.title, deck.title), &titles), "anki.txt"),
            false => (deck::csv(&deck), "csv"),
        };
        let file = book.dir().join("exports").join(format!("{id}-{}.{ext}", proton_research::store::slug(&deck.title)));
        if let Err(why) = proton_research::store::atomic_write(&file, text.as_bytes()) {
            return vec![notice(why, NoticeLevel::Error)];
        }
        let then = if anki { " In Anki: File › Import, and choose it." } else { "" };
        vec![notice(format!("{} cards written to {}.{then}", deck.cards.len(), crate::folder::tilde(&file)), NoticeLevel::Info)]
    }
}

/// A card's words, and its citations as the tutor writes them: what is
/// resolved for its chips.
fn card_text(card: &StudyCard) -> String {
    let cites: String = card.cites.iter().map(|c| format!("[[{c}]]")).collect();
    format!("{}\n{}\n{cites}\n", card.front, card.back)
}
