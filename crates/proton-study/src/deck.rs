//! Flashcards: decks of cards made from the sources, and the reviews that
//! schedule them.
//!
//! ```text
//! studio/decks/D001.json        a deck: its cards, as made and as edited
//! studio/decks/D001.log.jsonl   every review of them, a line each, only added to
//! ```
//!
//! The log is what a card's schedule is: replayed through [`fsrs`], it gives
//! each card's memory and when it is next due, so nothing is kept that could
//! disagree with it. A review taken back is a line saying so, not a line
//! taken out.
//!
//! A day's review is what is due, within limits — twenty new cards and two
//! hundred reviews a day unless the person says otherwise: cards being
//! learnt first, as they come due within minutes; then the reviews, the
//! least likely to be remembered first; then new cards, in their decks'
//! order.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::fsrs::{self, Memory, Rating, Scheduler, State};
use crate::store::Book;
use crate::studio::Make;

const DAY: i64 = 86_400;
/// How far ahead a card being learnt is shown when nothing else is left.
const LEARN_AHEAD: i64 = 20 * 60;
/// How far back a card missed is "missed lately".
const LATELY: i64 = 7 * DAY;
/// How many cards later a card missed in cramming comes back.
const AGAIN_AFTER: usize = 4;

/// What a card is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyCardKind")]
#[serde(rename_all = "camelCase")]
pub enum CardKind {
    /// A question on the front, its answer on the back.
    #[default]
    Basic,
    /// A sentence with what is to be remembered hidden: `{{c1::…}}`. Its
    /// back is anything more to say.
    Cloze,
}

/// A card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCard {
    /// `C001`: its deck's, never given to another card of it.
    pub id: String,
    #[serde(default)]
    pub kind: CardKind,
    pub front: String,
    #[serde(default)]
    pub back: String,
    #[serde(default)]
    pub topic: String,
    /// The learning objectives it tests: `O003`.
    #[serde(default)]
    pub objectives: Vec<String>,
    /// Where it comes from, as the tutor cites: `S07 p. 12`.
    #[serde(default)]
    pub cites: Vec<String>,
    /// The call that made it, `J0004`; empty for a card of one's own.
    #[serde(default)]
    pub task: String,
    #[ts(type = "number")]
    pub added: u64,
    /// When it was last changed by hand.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub edited: Option<u64>,
}

/// A deck, as its file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deck {
    pub kind: Make,
    pub title: String,
    pub made: u64,
    pub by: String,
    pub task: String,
    /// The number the next card is given.
    pub next: u32,
    pub cards: Vec<StudyCard>,
}

impl Deck {
    pub fn card(&self, id: &str) -> Option<&StudyCard> {
        self.cards.iter().find(|c| c.id == id)
    }

    /// Give a card its id, and take it in.
    pub fn take(&mut self, mut card: StudyCard) -> String {
        card.id = format!("C{:03}", self.next.max(1));
        self.next = self.next.max(1) + 1;
        let id = card.id.clone();
        self.cards.push(card);
        id
    }
}

/// A review: a card, how well it was remembered, and when.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logged {
    pub card: String,
    pub rating: Rating,
    /// Seconds since the epoch.
    pub at: i64,
    /// How long the card was looked at before it was rated, in milliseconds.
    #[serde(default)]
    pub took: u64,
    /// The session it was in: `due`, `cram` or `missed`.
    #[serde(default)]
    pub mode: String,
}

/// A line of a deck's log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Line {
    Review(Logged),
    /// The review of a card at a moment, taken back.
    Undo { undo: String, at: i64 },
}

/// What a session of review is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyReviewMode")]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// What is due today, within the day's limits.
    Due,
    /// Every card, due or not, each until it is remembered.
    Cram,
    /// The cards missed in the last week.
    Missed,
}

impl Mode {
    pub fn key(self) -> &'static str {
        match self {
            Mode::Due => "due",
            Mode::Cram => "cram",
            Mode::Missed => "missed",
        }
    }
}

/// How many new cards and reviews a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub new: usize,
    pub reviews: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { new: 20, reviews: 200 }
    }
}

/// A card of a notebook: its deck, and its id there.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardRef {
    pub deck: String,
    pub card: String,
}

/* ── kept ───────────────────────────────────────────────────────────── */

impl Book {
    fn decks_dir(&self) -> PathBuf {
        self.dir().join("studio/decks")
    }

    fn deck_file(&self, id: &str) -> PathBuf {
        self.decks_dir().join(format!("{id}.json"))
    }

    fn log_file(&self, id: &str) -> PathBuf {
        self.decks_dir().join(format!("{id}.log.jsonl"))
    }

    /// The notebook's decks' ids, in order.
    pub fn deck_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.decks_dir())
            .into_iter()
            .flat_map(|e| e.flatten())
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                let id = name.strip_suffix(".json")?;
                (id.starts_with('D') && id.len() > 1 && id[1..].bytes().all(|b| b.is_ascii_digit())).then(|| id.to_string())
            })
            .collect();
        ids.sort();
        ids
    }

    /// The id the next deck is given.
    pub fn new_deck_id(&self) -> String {
        let highest = self.deck_ids().iter().filter_map(|id| id[1..].parse::<u32>().ok()).max().unwrap_or(0);
        format!("D{:03}", highest + 1)
    }

    pub fn deck(&self, id: &str) -> Option<Deck> {
        if !id.starts_with('D') || id.contains(['/', '\\', '.']) {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(self.deck_file(id)).ok()?).ok()
    }

    pub fn save_deck(&self, id: &str, deck: &Deck) -> Result<(), String> {
        let text = serde_json::to_string_pretty(deck).map_err(|e| e.to_string())?;
        proton_research::store::atomic_write(&self.deck_file(id), format!("{text}\n").as_bytes())
    }

    /// A deck's reviews, in the order they were made, less those taken back.
    pub fn log(&self, id: &str) -> Vec<Logged> {
        let text = std::fs::read_to_string(self.log_file(id)).unwrap_or_default();
        let mut reviews: Vec<Logged> = Vec::new();
        // A line torn by a crash, or written by hand wrong, is passed over.
        for line in text.lines().filter_map(|l| serde_json::from_str::<Line>(l).ok()) {
            match line {
                Line::Review(review) => reviews.push(review),
                Line::Undo { undo, at } => {
                    if let Some(i) = reviews.iter().rposition(|r| r.card == undo && r.at == at) {
                        reviews.remove(i);
                    }
                }
            }
        }
        reviews
    }

    /// Write down a review.
    pub fn log_review(&self, id: &str, review: &Logged) -> Result<(), String> {
        let line = serde_json::to_string(review).map_err(|e| e.to_string())?;
        proton_research::store::append(&self.log_file(id), &format!("{line}\n"))
    }

    /// Take a review back.
    pub fn log_undo(&self, id: &str, card: &str, at: i64) -> Result<(), String> {
        let line = serde_json::to_string(&Line::Undo { undo: card.to_string(), at }).map_err(|e| e.to_string())?;
        proton_research::store::append(&self.log_file(id), &format!("{line}\n"))
    }

    /// Cards added to a deck, less any that say what one of it already does.
    /// How many were added.
    pub fn add_cards(&self, id: &str, cards: Vec<StudyCard>, task: &str) -> Result<usize, String> {
        let mut deck = self.deck(id).ok_or_else(|| format!("{id}: no such deck"))?;
        let fresh = fresh(&deck.cards, cards);
        let added = fresh.len();
        for mut card in fresh {
            card.task = task.to_string();
            deck.take(card);
        }
        self.save_deck(id, &deck)?;
        Ok(added)
    }
}

/* ── memory ─────────────────────────────────────────────────────────── */

/// The seed a card's `n`th review is fuzzed with.
pub fn seed(deck: &str, card: &str, n: u32) -> u64 {
    fsrs::seed(&format!("{deck}/{card}"), n)
}

/// Every card's memory, replayed from its deck's log.
pub fn memories(scheduler: &Scheduler, id: &str, deck: &Deck, log: &[Logged]) -> HashMap<String, Memory> {
    let mut out: HashMap<String, Memory> = deck.cards.iter().map(|c| (c.id.clone(), Memory::new(c.added as i64))).collect();
    for review in log {
        if let Some(memory) = out.get_mut(&review.card) {
            *memory = scheduler.review(memory, review.rating, review.at, seed(id, &review.card, memory.reviews));
        }
    }
    out
}

/// A deck, read, with its log and each card's memory.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub id: String,
    pub deck: Deck,
    pub log: Vec<Logged>,
    pub memories: HashMap<String, Memory>,
}

impl Loaded {
    /// When a card was first reviewed.
    fn first(&self) -> HashMap<&str, i64> {
        let mut first: HashMap<&str, i64> = HashMap::new();
        for review in &self.log {
            first.entry(review.card.as_str()).or_insert(review.at);
        }
        first
    }
}

/// How many cards are due, of each kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StudyCounts {
    /// New cards, within what is left of the day's limit.
    pub new: usize,
    /// Cards being learnt, due today.
    pub learning: usize,
    /// Reviews due today, within what is left of the day's limit.
    pub review: usize,
    /// Cards missed in the last week.
    pub missed: usize,
    pub total: usize,
}

impl StudyCounts {
    pub fn due(&self) -> usize {
        self.new + self.learning + self.review
    }
}

/// What is due today, in the order it is to be shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Due {
    /// Cards being learnt, by when they are due.
    pub learning: Vec<(CardRef, i64)>,
    pub review: Vec<CardRef>,
    pub new: Vec<CardRef>,
}

/// A notebook's decks, each card with its memory: what review is drawn from.
#[derive(Debug, Clone)]
pub struct Decks {
    pub scheduler: Scheduler,
    pub decks: Vec<Loaded>,
}

impl Decks {
    pub fn load(book: &Book, scheduler: Scheduler) -> Decks {
        let decks = book
            .deck_ids()
            .into_iter()
            .filter_map(|id| {
                let deck = book.deck(&id)?;
                let log = book.log(&id);
                let memories = memories(&scheduler, &id, &deck, &log);
                Some(Loaded { id, deck, log, memories })
            })
            .collect();
        Decks { scheduler, decks }
    }

    pub fn loaded(&self, id: &str) -> Option<&Loaded> {
        self.decks.iter().find(|d| d.id == id)
    }

    pub fn get(&self, card: &CardRef) -> Option<(&StudyCard, &Memory)> {
        let loaded = self.loaded(&card.deck)?;
        Some((loaded.deck.card(&card.card)?, loaded.memories.get(&card.card)?))
    }

    fn scoped<'a>(&'a self, scope: Option<&'a str>) -> impl Iterator<Item = &'a Loaded> + 'a {
        self.decks.iter().filter(move |d| scope.is_none_or(|s| s == d.id))
    }

    /// What is due in the day `[start, end)`, at `now`, within the limits
    /// less what has been done today. `shuffle` seeds an order other than
    /// the least remembered first and the decks' own.
    pub fn due(&self, scope: Option<&str>, now: i64, day: (i64, i64), limits: Limits, shuffle: Option<u64>) -> Due {
        let mut due = Due::default();
        let mut reviews: Vec<(CardRef, f64)> = Vec::new();
        let (mut new_today, mut reviewed_today) = (0usize, 0usize);
        for loaded in self.scoped(scope) {
            let first = loaded.first();
            let today: HashSet<&str> = loaded.log.iter().filter(|r| r.at >= day.0 && r.at < day.1).map(|r| r.card.as_str()).collect();
            for card in &loaded.deck.cards {
                let Some(memory) = loaded.memories.get(&card.id) else { continue };
                if today.contains(card.id.as_str()) {
                    match first.get(card.id.as_str()) {
                        Some(at) if *at >= day.0 => new_today += 1,
                        _ => reviewed_today += 1,
                    }
                }
                let at = CardRef { deck: loaded.id.clone(), card: card.id.clone() };
                match (memory.reviews, memory.state) {
                    (0, _) => due.new.push(at),
                    (_, State::Learning | State::Relearning) if memory.due < day.1 => due.learning.push((at, memory.due)),
                    (_, State::Review) if memory.due < day.1 => reviews.push((at, self.scheduler.retrievability(memory, now))),
                    _ => {}
                }
            }
        }
        due.learning.sort_by_key(|(_, at)| *at);
        reviews.sort_by(|a, b| a.1.total_cmp(&b.1));
        due.review = reviews.into_iter().map(|(at, _)| at).collect();
        if let Some(seed) = shuffle {
            shuffled(&mut due.review, seed);
            shuffled(&mut due.new, seed.rotate_left(17));
        }
        due.review.truncate(limits.reviews.saturating_sub(reviewed_today));
        due.new.truncate(limits.new.saturating_sub(new_today));
        due
    }

    /// The cards missed in the week before `now`.
    pub fn missed(&self, scope: Option<&str>, now: i64) -> Vec<CardRef> {
        let mut out = Vec::new();
        for loaded in self.scoped(scope) {
            let lately: HashSet<&str> = loaded.log.iter().filter(|r| r.rating == Rating::Again && r.at > now - LATELY).map(|r| r.card.as_str()).collect();
            out.extend(loaded.deck.cards.iter().filter(|c| lately.contains(c.id.as_str())).map(|c| CardRef { deck: loaded.id.clone(), card: c.id.clone() }));
        }
        out
    }

    pub fn counts(&self, scope: Option<&str>, now: i64, day: (i64, i64), limits: Limits) -> StudyCounts {
        let due = self.due(scope, now, day, limits, None);
        StudyCounts {
            new: due.new.len(),
            learning: due.learning.len(),
            review: due.review.len(),
            missed: self.missed(scope, now).len(),
            total: self.scoped(scope).map(|d| d.deck.cards.len()).sum(),
        }
    }

    /// A review, written down and replayed into its card's memory.
    pub fn rate(&mut self, book: &Book, card: &CardRef, rating: Rating, at: i64, took: u64, mode: Mode) -> Result<Memory, String> {
        let scheduler = self.scheduler.clone();
        let loaded = self.decks.iter_mut().find(|d| d.id == card.deck).ok_or_else(|| format!("{}: no such deck", card.deck))?;
        let memory = loaded.memories.get(&card.card).ok_or_else(|| format!("{}: no such card", card.card))?;
        let next = scheduler.review(memory, rating, at, seed(&card.deck, &card.card, memory.reviews));
        let review = Logged { card: card.card.clone(), rating, at, took, mode: mode.key().into() };
        book.log_review(&card.deck, &review)?;
        loaded.log.push(review);
        loaded.memories.insert(card.card.clone(), next.clone());
        Ok(next)
    }

    /// Take back a card's review at `at`, and replay its deck.
    pub fn undo(&mut self, book: &Book, card: &CardRef, at: i64) -> Result<(), String> {
        book.log_undo(&card.deck, &card.card, at)?;
        let scheduler = self.scheduler.clone();
        if let Some(loaded) = self.decks.iter_mut().find(|d| d.id == card.deck) {
            loaded.log = book.log(&loaded.id);
            loaded.memories = memories(&scheduler, &loaded.id, &loaded.deck, &loaded.log);
        }
        Ok(())
    }

    /// When each rating would make a card due next, from `now`, in seconds.
    pub fn intervals(&self, card: &CardRef, now: i64) -> Option<[i64; 4]> {
        let (_, memory) = self.get(card)?;
        Some(self.scheduler.preview(memory, now, seed(&card.deck, &card.card, memory.reviews)))
    }
}

/// A list put in an order of a seed's, the same each time for the same seed.
fn shuffled<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let j = ((state >> 33) % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

/* ── a session ──────────────────────────────────────────────────────── */

/// A session of review: what it is of, and what it has come to.
#[derive(Debug, Clone)]
pub struct Session {
    /// A deck, or every deck of the notebook.
    pub scope: Option<String>,
    pub mode: Mode,
    pub shuffle: bool,
    pub started: i64,
    /// For cramming and the missed: the cards still to go, in order.
    pub left: VecDeque<CardRef>,
    /// The card being answered.
    pub current: Option<CardRef>,
    pub done: usize,
    pub again: usize,
    /// This session's reviews, the last one last: what undo takes back.
    pub history: Vec<(CardRef, i64, Rating)>,
    /// A card whose review was taken back: the next to answer.
    redo: Option<CardRef>,
    /// When the session, out of cards for now, can show the next card
    /// being learnt.
    pub waiting: Option<i64>,
}

impl Session {
    pub fn start(decks: &Decks, scope: Option<String>, mode: Mode, shuffle: bool, now: i64) -> Session {
        let mut left: VecDeque<CardRef> = match mode {
            Mode::Due => VecDeque::new(),
            Mode::Cram => decks.scoped(scope.as_deref()).flat_map(|d| d.deck.cards.iter().map(|c| CardRef { deck: d.id.clone(), card: c.id.clone() })).collect(),
            Mode::Missed => decks.missed(scope.as_deref(), now).into(),
        };
        if shuffle {
            shuffled(left.make_contiguous(), now as u64);
        }
        Session { scope, mode, shuffle, started: now, left, current: None, done: 0, again: 0, history: Vec::new(), redo: None, waiting: None }
    }

    /// The card to answer next, if any is left. When the only ones left are
    /// being learnt and not yet due, `Err` says when the first can be shown:
    /// twenty minutes before it is due.
    pub fn next(&mut self, decks: &Decks, now: i64, day: (i64, i64), limits: Limits) -> Result<Option<CardRef>, i64> {
        self.waiting = None;
        if let Some(card) = self.redo.take().filter(|c| decks.get(c).is_some()) {
            self.current = Some(card.clone());
            return Ok(Some(card));
        }
        let next = match self.mode {
            Mode::Cram | Mode::Missed => self.left.front().cloned(),
            Mode::Due => {
                let due = decks.due(self.scope.as_deref(), now, day, limits, self.shuffle.then_some(self.started as u64));
                let soon = due.learning.iter().find(|(_, at)| *at <= now).map(|(c, _)| c.clone());
                let ahead = due.learning.first().filter(|(_, at)| *at <= now + LEARN_AHEAD).map(|(c, _)| c.clone());
                match soon.or_else(|| due.review.first().cloned()).or_else(|| due.new.first().cloned()).or(ahead) {
                    Some(card) => Some(card),
                    None => {
                        self.current = None;
                        self.waiting = due.learning.first().map(|(_, at)| at - LEARN_AHEAD);
                        return match self.waiting {
                            Some(at) => Err(at),
                            None => Ok(None),
                        };
                    }
                }
            }
        };
        self.current = next.clone();
        Ok(next)
    }

    /// The current card was rated: counted, and in cramming, a card missed
    /// put back a few cards on.
    pub fn rated(&mut self, card: &CardRef, rating: Rating, at: i64) {
        self.done += 1;
        if rating == Rating::Again {
            self.again += 1;
        }
        self.history.push((card.clone(), at, rating));
        if matches!(self.mode, Mode::Cram | Mode::Missed) {
            if let Some(i) = self.left.iter().position(|c| c == card) {
                let card = self.left.remove(i).unwrap_or_else(|| card.clone());
                if rating == Rating::Again {
                    let at = (i + AGAIN_AFTER).min(self.left.len());
                    self.left.insert(at, card);
                }
            }
        }
        self.current = None;
    }

    /// The last review taken back: the card is to be answered again.
    pub fn undone(&mut self) -> Option<(CardRef, i64)> {
        let (card, at, rating) = self.history.pop()?;
        self.done = self.done.saturating_sub(1);
        if rating == Rating::Again {
            self.again = self.again.saturating_sub(1);
        }
        if matches!(self.mode, Mode::Cram | Mode::Missed) {
            if let Some(i) = self.left.iter().position(|c| *c == card) {
                self.left.remove(i);
            }
            self.left.push_front(card.clone());
        }
        self.redo = Some(card.clone());
        Some((card, at))
    }

    /// How many are left to go, for cramming and the missed.
    pub fn remaining(&self) -> usize {
        self.left.len()
    }
}

/* ── cards made ─────────────────────────────────────────────────────── */

/// A cloze card's hidden parts: `{{c1::answer}}` or `{{c1::answer::hint}}`,
/// braces inside counted, so `{{c1::$\frac{a}{b}$}}` is one.
pub fn deletions(text: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{c") {
        let after = &rest[start + 3..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || !after[digits..].starts_with("::") {
            rest = &rest[start + 3..];
            continue;
        }
        let body = &after[digits + 2..];
        let mut depth = 0i32;
        let mut end = None;
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' if depth == 0 && bytes.get(i + 1) == Some(&b'}') => {
                    end = Some(i);
                    break;
                }
                b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let Some(end) = end else { break };
        let inner = &body[..end];
        let (answer, hint) = match inner.rsplit_once("::") {
            Some((answer, hint)) => (answer.to_string(), Some(hint.to_string())),
            None => (inner.to_string(), None),
        };
        out.push((answer, hint));
        rest = &body[end + 2..];
    }
    out
}

/// A card's words, for telling cards apart: its front, with a cloze's
/// markup taken out.
fn words(card: &StudyCard) -> String {
    card.front.replace("{{c", " ").replace("::", " ").replace("}}", " ").to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// New cards less those that say what a card already does — the same words,
/// or near enough — and less repeats among themselves.
pub fn fresh(existing: &[StudyCard], new: Vec<StudyCard>) -> Vec<StudyCard> {
    let old: Vec<String> = existing.iter().map(words).collect();
    let new_words: Vec<String> = new.iter().map(words).collect();
    let documents: Vec<(String, &str)> = old.iter().enumerate().map(|(i, w)| (format!("o{i}"), w.as_str())).chain(new_words.iter().enumerate().map(|(i, w)| (format!("n{i}"), w.as_str()))).collect();
    let index = proton_research::proximity::Index::new(documents.iter().map(|(id, w)| (id.as_str(), *w)));
    let mut kept: Vec<String> = (0..old.len()).map(|i| format!("o{i}")).collect();
    let mut out = Vec::new();
    for (i, card) in new.into_iter().enumerate() {
        let id = format!("n{i}");
        let same = new_words[i].is_empty() || old.contains(&new_words[i]) || kept.iter().any(|k| index.similarity(&id, k) >= 0.85);
        if !same {
            kept.push(id);
            out.push(card);
        }
    }
    out
}

/// The cards an answer gave, read: each whole, a cloze with something
/// hidden, repeats left out. `Err` when too few of them were cards.
pub fn read_cards(value: &serde_json::Value) -> Result<Vec<StudyCard>, String> {
    #[derive(Deserialize)]
    struct Said {
        #[serde(default)]
        kind: CardKind,
        front: String,
        #[serde(default)]
        back: String,
        #[serde(default)]
        topic: String,
        #[serde(default)]
        objectives: Vec<String>,
        #[serde(default)]
        cites: Vec<String>,
    }
    let said: Vec<Said> = serde_json::from_value(value["cards"].clone()).map_err(|e| format!("its cards: {e}"))?;
    if said.is_empty() {
        return Err("there are no cards".into());
    }
    let total = said.len();
    let now = crate::now();
    let whole: Vec<StudyCard> = said
        .into_iter()
        .filter(|c| !c.front.trim().is_empty())
        .filter(|c| match c.kind {
            CardKind::Basic => !c.back.trim().is_empty(),
            CardKind::Cloze => !deletions(&c.front).is_empty(),
        })
        .map(|c| StudyCard {
            id: String::new(),
            kind: c.kind,
            front: c.front.trim().to_string(),
            back: c.back.trim().to_string(),
            topic: c.topic.trim().to_string(),
            objectives: c.objectives,
            cites: c.cites,
            task: String::new(),
            added: now,
            edited: None,
        })
        .collect();
    if whole.len() * 2 < total {
        return Err(format!(
            "only {} of its {total} cards are whole: a basic card needs a front and a back, a cloze card something hidden in its front as {{{{c1::…}}}}",
            whole.len()
        ));
    }
    let mut cards = fresh(&[], whole);
    for (i, card) in cards.iter_mut().enumerate() {
        card.id = format!("C{:03}", i + 1);
    }
    Ok(cards)
}

/* ── out ────────────────────────────────────────────────────────────── */

fn csv_cell(cell: &str) -> String {
    match cell.contains([',', '"', '\n', '\r']) {
        true => format!("\"{}\"", cell.replace('"', "\"\"")),
        false => cell.to_string(),
    }
}

/// A deck as CSV: a row a card.
pub fn csv(deck: &Deck) -> String {
    let mut out = String::from("kind,front,back,topic,cites\n");
    for card in &deck.cards {
        let kind = match card.kind {
            CardKind::Basic => "basic",
            CardKind::Cloze => "cloze",
        };
        let row = [kind.to_string(), card.front.clone(), card.back.clone(), card.topic.clone(), card.cites.join("; ")];
        out.push_str(&row.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// A deck as Anki imports it: tab-separated, its notes Basic or Cloze, into
/// the deck named, TeX as MathJax, citations as the sources' titles.
pub fn anki(deck: &Deck, name: &str, titles: &HashMap<String, String>) -> String {
    let mut out = String::from("#separator:tab\n#html:true\n#notetype column:1\n#deck column:2\n#tags column:5\n");
    let name = name.replace(['\t', '\n'], " ");
    for card in &deck.cards {
        let kind = match card.kind {
            CardKind::Basic => "Basic",
            CardKind::Cloze => "Cloze",
        };
        let front = anki_field(&card.front, titles);
        let mut back = anki_field(&card.back, titles);
        let from: Vec<String> = card.cites.iter().map(|c| cite_title(c, titles)).collect();
        if !from.is_empty() {
            let from = escape(&from.join("; "));
            back = match back.is_empty() {
                true => format!("<small>{from}</small>"),
                false => format!("{back}<br><br><small>{from}</small>"),
            };
        }
        let mut tags: Vec<String> = vec!["proton".into()];
        if !card.topic.is_empty() {
            tags.push(card.topic.split_whitespace().collect::<Vec<_>>().join("_"));
        }
        tags.extend(card.objectives.iter().cloned());
        out.push_str(&[kind.to_string(), name.clone(), front, back, tags.join(" ")].join("\t"));
        out.push('\n');
    }
    out
}

/// A citation as its source's title and place: `S07 p. 12` → `Lecture 3, p. 12`.
fn cite_title(cite: &str, titles: &HashMap<String, String>) -> String {
    let (id, place) = cite.split_once(' ').unwrap_or((cite, ""));
    match (titles.get(id), place.trim()) {
        (Some(title), "") => title.clone(),
        (Some(title), place) => format!("{title}, {place}"),
        (None, _) => cite.to_string(),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A card's side as an Anki field: HTML, its TeX as MathJax, its lines as
/// breaks, its citations as their sources' titles.
fn anki_field(text: &str, titles: &HashMap<String, String>) -> String {
    // TeX: `$$…$$` to `\[…\]`, `$…$` to `\(…\)`, a `}}` inside spaced so a
    // cloze around it still closes where it should.
    let mut out = String::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        if c == '$' && !text[..i].ends_with('\\') {
            let display = text[i + 1..].starts_with('$');
            let (open, close) = if display { (2, "$$") } else { (1, "$") };
            if let Some(len) = text[i + open..].find(close).filter(|len| *len > 0) {
                out.push_str(&escape(&plain).replace('\n', "<br>"));
                plain.clear();
                let tex = escape(&text[i + open..i + open + len].replace("}}", "} }"));
                out.push_str(&match display {
                    true => format!("\\[{tex}\\]"),
                    false => format!("\\({tex}\\)"),
                });
                i += open + len + close.len();
                continue;
            }
        }
        plain.push(c);
        i += c.len_utf8();
    }
    out.push_str(&escape(&plain).replace('\n', "<br>"));
    // Citations last, so a `$` in a source's title is never taken for TeX.
    let mut said = String::new();
    let mut rest = out.as_str();
    while let Some(start) = rest.find("[[") {
        let Some(end) = rest[start..].find("]]") else { break };
        said.push_str(&rest[..start]);
        said.push_str(&format!("({})", escape(&cite_title(&rest[start + 2..start + end], titles))));
        rest = &rest[start + end + 2..];
    }
    said.push_str(rest);
    said.replace('\t', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Folder;

    /// 2026-10-05 10:00 UTC.
    const NOW: i64 = 1_791_194_400;

    fn book() -> Book {
        let root = std::env::temp_dir().join(format!("proton-study-deck-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        Folder::new(root).create("Search", 1).unwrap()
    }

    fn card(front: &str, back: &str) -> StudyCard {
        StudyCard { id: String::new(), kind: CardKind::Basic, front: front.into(), back: back.into(), topic: "Search".into(), objectives: vec![], cites: vec!["S01 p. 2".into()], task: String::new(), added: NOW as u64 - DAY as u64, edited: None }
    }

    fn deck(cards: Vec<StudyCard>) -> Deck {
        let mut deck = Deck { kind: Make::Flashcards, title: "Search".into(), made: NOW as u64, by: "test".into(), task: "J0001".into(), next: 1, cards: Vec::new() };
        for c in cards {
            deck.take(c);
        }
        deck
    }

    fn no_fuzz() -> Scheduler {
        Scheduler { fuzz: false, ..Scheduler::default() }
    }

    #[test]
    fn a_cloze_hides_what_its_braces_hold() {
        let found = deletions("BFS is {{c1::complete}} when $b$ is {{c2::finite::how big?}}, and costs {{c1::$O(b^{d})$}}.");
        assert_eq!(found, vec![("complete".to_string(), None), ("finite".to_string(), Some("how big?".to_string())), ("$O(b^{d})$".to_string(), None)]);
        assert!(deletions("{{c1::never closed").is_empty());
        assert!(deletions("no deletions {{here}}").is_empty());
    }

    #[test]
    fn cards_read_whole_and_once() {
        let value = serde_json::json!({ "title": "Search", "cards": [
            { "kind": "basic", "front": "Is BFS complete?", "back": "Yes, when $b$ is finite.", "topic": "Search", "objectives": ["O001"], "cites": ["S01 p. 2"] },
            { "kind": "basic", "front": "Is BFS complete?", "back": "Yes.", "topic": "Search", "objectives": [], "cites": [] },
            { "kind": "cloze", "front": "UCS expands the node of least {{c1::path cost}}.", "back": "", "topic": "Search", "objectives": [], "cites": [] },
            { "kind": "cloze", "front": "Nothing hidden here.", "back": "", "topic": "Search", "objectives": [], "cites": [] },
        ]});
        let cards = read_cards(&value).unwrap();
        assert_eq!(cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["C001", "C002"], "the repeat and the cloze hiding nothing are left out");
        assert_eq!(cards[1].kind, CardKind::Cloze);
        let mostly_broken = serde_json::json!({ "cards": [{ "kind": "basic", "front": "Q", "back": "" }, { "kind": "basic", "front": "", "back": "A" }, { "kind": "basic", "front": "Q2", "back": "A2" }] });
        assert!(read_cards(&mostly_broken).unwrap_err().contains("only 1 of its 3"));
    }

    #[test]
    fn near_repeats_of_a_deck_are_not_added_to_it() {
        let existing = deck(vec![card("What is the time complexity of breadth-first search?", "O(b^d)")]).cards;
        let new = vec![
            card("What is the time complexity of breadth first search?", "O(b^d)"),
            card("Why is uniform-cost search optimal?", "It expands in order of path cost."),
            card("why is uniform cost search optimal", "Path costs."),
        ];
        let kept = fresh(&existing, new);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!(kept[0].front.starts_with("Why is uniform-cost"));
    }

    #[test]
    fn the_log_is_the_schedule_and_undo_takes_a_review_back() {
        let book = book();
        let d = deck(vec![card("Q1", "A1"), card("Q2", "A2")]);
        book.save_deck("D001", &d).unwrap();
        let mut decks = Decks::load(&book, no_fuzz());
        let c1 = CardRef { deck: "D001".into(), card: "C001".into() };
        let after_good = decks.rate(&book, &c1, Rating::Good, NOW, 3000, Mode::Due).unwrap();
        decks.rate(&book, &c1, Rating::Good, NOW + 600, 2000, Mode::Due).unwrap();
        // Replayed from the file, the memory is the one held.
        let replayed = Decks::load(&book, no_fuzz());
        assert_eq!(replayed.get(&c1).unwrap().1, decks.get(&c1).unwrap().1);
        assert_eq!(replayed.get(&c1).unwrap().1.state, State::Review);
        // Undo the second: back to one step in.
        decks.undo(&book, &c1, NOW + 600).unwrap();
        assert_eq!(decks.get(&c1).unwrap().1, &after_good);
        assert_eq!(Decks::load(&book, no_fuzz()).get(&c1).unwrap().1, &after_good, "the undo is in the log");
        let lines = std::fs::read_to_string(book.dir().join("studio/decks/D001.log.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 3, "two reviews and an undo, nothing taken out: {lines}");
    }

    #[test]
    fn a_day_is_learning_then_reviews_then_new_within_its_limits() {
        let book = book();
        let d = deck((1..=30).map(|i| card(&format!("Question {i} about search {}", "x".repeat(i)), "A")).collect());
        book.save_deck("D001", &d).unwrap();
        let mut decks = Decks::load(&book, no_fuzz());
        let day = (NOW - 6 * 3600, NOW + 18 * 3600);
        let limits = Limits { new: 5, reviews: 200 };
        let due = decks.due(None, NOW, day, limits, None);
        assert_eq!((due.learning.len(), due.review.len(), due.new.len()), (0, 0, 5), "five new a day");
        // Learn three today: they count against the day's new cards.
        let mut session = Session::start(&decks, None, Mode::Due, false, NOW);
        for _ in 0..3 {
            let next = session.next(&decks, NOW, day, limits).unwrap().unwrap();
            decks.rate(&book, &next, Rating::Good, NOW, 1000, Mode::Due).unwrap();
            session.rated(&next, Rating::Good, NOW);
        }
        let counts = decks.counts(None, NOW, day, limits);
        assert_eq!((counts.learning, counts.new), (3, 2), "three in their steps, two new left of five");
        // The learning cards come back in ten minutes: until then, the new.
        let next = session.next(&decks, NOW, day, limits).unwrap().unwrap();
        assert_eq!(decks.get(&next).unwrap().1.reviews, 0, "a new card while the others wait");
        // Nothing new left: a card being learnt is shown up to twenty minutes early.
        let none_new = Limits { new: 3, reviews: 200 };
        let early = session.next(&decks, NOW + 60, day, none_new).unwrap().unwrap();
        assert_eq!(decks.get(&early).unwrap().1.state, State::Learning);
        // Rated Easy, each graduates: nothing more today, and nothing to wait for.
        for card in ["C001", "C002", "C003"] {
            let card = CardRef { deck: "D001".into(), card: card.into() };
            decks.rate(&book, &card, Rating::Easy, NOW + 700, 1000, Mode::Due).unwrap();
        }
        assert_eq!(session.next(&decks, NOW + 700, day, none_new), Ok(None));
        // A review taken back: that card is the next, whatever else is due.
        let last = CardRef { deck: "D001".into(), card: "C003".into() };
        decks.undo(&book, &last, NOW + 700).unwrap();
        session.history.push((last.clone(), NOW + 700, Rating::Easy));
        assert_eq!(session.undone().map(|(c, _)| c), Some(last.clone()));
        assert_eq!(session.next(&decks, NOW + 701, day, limits), Ok(Some(last.clone())));
        decks.rate(&book, &last, Rating::Easy, NOW + 702, 0, Mode::Due).unwrap();
        // Tomorrow is another day's limit; and in days, the reviews come due.
        let later = NOW + 40 * DAY;
        let due = decks.due(None, later, (later - 3600, later + 20 * 3600), limits, None);
        assert_eq!((due.review.len(), due.new.len()), (3, 5));
    }

    #[test]
    fn learning_not_yet_due_is_waited_for() {
        let book = book();
        book.save_deck("D001", &deck(vec![card("Q1", "A1")])).unwrap();
        let mut decks = Decks::load(&book, no_fuzz());
        let day = (NOW - 3600, NOW + 20 * 3600);
        let c1 = CardRef { deck: "D001".into(), card: "C001".into() };
        decks.rate(&book, &c1, Rating::Good, NOW, 0, Mode::Due).unwrap();
        decks.rate(&book, &c1, Rating::Again, NOW + 30, 0, Mode::Due).unwrap();
        let mut session = Session::start(&decks, None, Mode::Due, false, NOW);
        // Again is a minute; twenty minutes ahead is shown at once.
        assert_eq!(session.next(&decks, NOW + 31, day, Limits::default()), Ok(Some(c1.clone())));
        decks.rate(&book, &c1, Rating::Good, NOW + 40, 0, Mode::Due).unwrap();
        decks.rate(&book, &c1, Rating::Hard, NOW + 50, 0, Mode::Due).unwrap();
        // Hard on the second step: ten minutes, inside the twenty.
        assert!(session.next(&decks, NOW + 51, day, Limits::default()).unwrap().is_some());
        let mut far = Decks::load(&book, Scheduler { learning_steps: vec![60, 3600], ..no_fuzz() });
        far.rate(&book, &c1, Rating::Hard, NOW + 60, 0, Mode::Due).unwrap();
        let wait = session.next(&far, NOW + 61, day, Limits::default());
        assert_eq!(wait, Err(NOW + 60 + 3600 - LEARN_AHEAD), "shown twenty minutes before it is due");
        assert_eq!(session.waiting, Some(NOW + 60 + 3600 - LEARN_AHEAD));
    }

    #[test]
    fn cramming_goes_through_every_card_and_a_miss_comes_back() {
        let book = book();
        book.save_deck("D001", &deck((1..=6).map(|i| card(&format!("Q{i} {}", "y".repeat(i)), "A")).collect())).unwrap();
        let mut decks = Decks::load(&book, no_fuzz());
        let day = (NOW - 3600, NOW + 20 * 3600);
        let mut session = Session::start(&decks, Some("D001".into()), Mode::Cram, false, NOW);
        let mut seen = Vec::new();
        while let Ok(Some(card)) = session.next(&decks, NOW, day, Limits::default()) {
            let rating = if card.card == "C002" && !seen.contains(&card.card) { Rating::Again } else { Rating::Good };
            decks.rate(&book, &card, rating, NOW, 0, Mode::Cram).unwrap();
            session.rated(&card, rating, NOW);
            seen.push(card.card.clone());
            assert!(seen.len() < 20);
        }
        assert_eq!(seen, ["C001", "C002", "C003", "C004", "C005", "C006", "C002"], "missed, it came back four cards on");
        assert_eq!((session.done, session.again), (7, 1));
        // Missed lately: C002.
        let missed = Session::start(&decks, None, Mode::Missed, false, NOW + DAY);
        assert_eq!(missed.left.iter().map(|c| c.card.as_str()).collect::<Vec<_>>(), ["C002"]);
        assert!(Session::start(&decks, None, Mode::Missed, false, NOW + 8 * DAY).left.is_empty(), "a week on, it is not lately");
        // Undo puts the last card back in front.
        let (undone, at) = session.undone().unwrap();
        assert_eq!((undone.card.as_str(), at, session.done), ("C002", NOW, 6));
        assert_eq!(session.left.front(), Some(&undone));
    }

    #[test]
    fn the_shuffle_is_the_same_for_the_same_seed() {
        let mut a: Vec<u32> = (0..20).collect();
        let mut b = a.clone();
        shuffled(&mut a, 7);
        shuffled(&mut b, 7);
        assert_eq!(a, b);
        assert_ne!(a, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn a_deck_goes_out_as_csv_and_as_anki_reads_it() {
        let mut d = deck(vec![card("What does $\\frac{1}{2}$ of a tab\tlook like? [[S01 p. 2]]", "Half, \"so\" <b>")]);
        d.take(StudyCard { kind: CardKind::Cloze, front: "Halved: {{c1::$\\frac{1}{x^{2}}$}}.".into(), back: String::new(), ..card("", "") });
        let csv = csv(&d);
        assert!(csv.starts_with("kind,front,back,topic,cites\nbasic,"), "{csv}");
        assert!(csv.contains("\"Half, \"\"so\"\" <b>\""));
        let titles = HashMap::from([("S01".to_string(), "Lecture 2: Search".to_string())]);
        let anki = anki(&d, "Proton::Search", &titles);
        let lines: Vec<&str> = anki.lines().collect();
        assert_eq!(lines[..5], ["#separator:tab", "#html:true", "#notetype column:1", "#deck column:2", "#tags column:5"]);
        let basic: Vec<&str> = lines[5].split('\t').collect();
        assert_eq!(basic.len(), 5, "{basic:?}");
        assert_eq!(basic[0], "Basic");
        assert_eq!(basic[2], "What does \\(\\frac{1}{2}\\) of a tab look like? (Lecture 2: Search, p. 2)");
        assert_eq!(basic[3], "Half, \"so\" &lt;b&gt;<br><br><small>Lecture 2: Search, p. 2</small>");
        assert_eq!(basic[4], "proton Search");
        let cloze: Vec<&str> = lines[6].split('\t').collect();
        assert_eq!(cloze[2], "Halved: {{c1::\\(\\frac{1}{x^{2} }\\)}}.", "a `}}` in TeX is spaced, so the cloze closes after it");
    }
}
