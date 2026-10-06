//! FSRS-6: when a card is next due, from how well it was remembered.
//!
//! The Free Spaced Repetition Scheduler models each card by its *stability*
//! — how many days until the chance of recalling it falls to 90% — and its
//! *difficulty*, from 1 to 10, and after each review moves both by formulas
//! fitted to many millions of real reviews. A card is next due when its
//! chance of recall falls to the retention asked for.
//!
//! This is py-fsrs's scheduler (open-spaced-repetition/py-fsrs, FSRS-6, 21
//! parameters), formula for formula: the default parameters, learning steps
//! of one and ten minutes, a relearning step of ten, a retention of 0.9, and
//! its fuzz, made deterministic here so a review can be replayed. The tests
//! are py-fsrs's own, with its expected numbers.
//!
//! Two things are Proton's: a card is never scheduled past the day of the
//! exam its notebook is for, and in the fortnight before it a higher
//! retention is asked for, so what is due is what would otherwise be lost.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// FSRS-6's default parameters, as py-fsrs ships them.
pub const DEFAULT: [f64; 21] = [
    0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835, 0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

const STABILITY_MIN: f64 = 0.001;
const DAY: i64 = 86_400;

/// How well a card was remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "StudyRating")]
#[serde(rename_all = "camelCase")]
pub enum Rating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Rating {
    fn n(self) -> f64 {
        self as u8 as f64
    }

    pub const ALL: [Rating; 4] = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy];
}

/// Where a card is in learning it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// New, or still in its first steps.
    Learning,
    Review,
    /// Forgotten, and in its steps again.
    Relearning,
}

/// A card's memory, as FSRS models it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub state: State,
    /// Which learning or relearning step it is on.
    pub step: Option<usize>,
    pub stability: Option<f64>,
    pub difficulty: Option<f64>,
    /// Seconds since the epoch.
    pub due: i64,
    pub last_review: Option<i64>,
    /// How many times it has been reviewed: what its fuzz is seeded with.
    pub reviews: u32,
}

impl Memory {
    /// A new card, due now.
    pub fn new(now: i64) -> Memory {
        Memory { state: State::Learning, step: Some(0), stability: None, difficulty: None, due: now, last_review: None, reviews: 0 }
    }
}

/// The scheduler: its parameters and the choices around them.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheduler {
    pub parameters: [f64; 21],
    /// The chance of recall a card is next due at.
    pub retention: f64,
    /// In seconds.
    pub learning_steps: Vec<i64>,
    pub relearning_steps: Vec<i64>,
    /// In days.
    pub maximum_interval: i64,
    pub fuzz: bool,
    /// The day of the exam, in seconds since the epoch: nothing is
    /// scheduled past it, and in the fortnight before it [`EXAM_RETENTION`]
    /// is asked for.
    pub exam: Option<i64>,
}

/// The retention asked for in the fortnight before an exam.
pub const EXAM_RETENTION: f64 = 0.95;
/// How long before an exam it is asked for.
const EXAM_RUN_IN: i64 = 14 * DAY;

impl Default for Scheduler {
    fn default() -> Scheduler {
        Scheduler {
            parameters: DEFAULT,
            retention: 0.9,
            learning_steps: vec![60, 600],
            relearning_steps: vec![600],
            maximum_interval: 36_500,
            fuzz: true,
            exam: None,
        }
    }
}

impl Scheduler {
    fn w(&self, i: usize) -> f64 {
        self.parameters[i]
    }

    fn decay(&self) -> f64 {
        -self.w(20)
    }

    fn factor(&self) -> f64 {
        0.9f64.powf(1.0 / self.decay()) - 1.0
    }

    /// The chance a card is recalled at a moment.
    pub fn retrievability(&self, memory: &Memory, at: i64) -> f64 {
        let (Some(last), Some(stability)) = (memory.last_review, memory.stability) else { return 0.0 };
        let days = (at - last).max(0).div_euclid(DAY) as f64;
        (1.0 + self.factor() * days / stability).powf(self.decay())
    }

    fn clamp_difficulty(d: f64) -> f64 {
        d.clamp(1.0, 10.0)
    }

    fn clamp_stability(s: f64) -> f64 {
        s.max(STABILITY_MIN)
    }

    fn initial_stability(&self, rating: Rating) -> f64 {
        Self::clamp_stability(self.w(rating as usize - 1))
    }

    fn initial_difficulty(&self, rating: Rating, clamp: bool) -> f64 {
        let d = self.w(4) - (self.w(5) * (rating.n() - 1.0)).exp() + 1.0;
        if clamp {
            Self::clamp_difficulty(d)
        } else {
            d
        }
    }

    /// The retention asked for at a moment: higher in an exam's run-in.
    fn retention_at(&self, at: i64) -> f64 {
        match self.exam {
            Some(exam) if at >= exam - EXAM_RUN_IN && at < exam => self.retention.max(EXAM_RETENTION),
            _ => self.retention,
        }
    }

    /// Days until a card of this stability is next due.
    fn next_interval(&self, stability: f64, at: i64) -> i64 {
        let retention = self.retention_at(at);
        let interval = stability / self.factor() * (retention.powf(1.0 / self.decay()) - 1.0);
        (interval.round() as i64).max(1).min(self.maximum_interval)
    }

    fn short_term_stability(&self, stability: f64, rating: Rating) -> f64 {
        let mut increase = (self.w(17) * (rating.n() - 3.0 + self.w(18))).exp() * stability.powf(-self.w(19));
        if rating != Rating::Again {
            increase = increase.max(1.0);
        }
        Self::clamp_stability(stability * increase)
    }

    fn next_difficulty(&self, difficulty: f64, rating: Rating) -> f64 {
        let target = self.initial_difficulty(Rating::Easy, false);
        let delta = -(self.w(6) * (rating.n() - 3.0));
        let damped = difficulty + (10.0 - difficulty) * delta / 9.0;
        Self::clamp_difficulty(self.w(7) * target + (1.0 - self.w(7)) * damped)
    }

    fn next_stability(&self, difficulty: f64, stability: f64, retrievability: f64, rating: Rating) -> f64 {
        let next = match rating {
            Rating::Again => {
                let long = self.w(11) * difficulty.powf(-self.w(12)) * ((stability + 1.0).powf(self.w(13)) - 1.0) * ((1.0 - retrievability) * self.w(14)).exp();
                let short = stability / (self.w(17) * self.w(18)).exp();
                long.min(short)
            }
            _ => {
                let hard = if rating == Rating::Hard { self.w(15) } else { 1.0 };
                let easy = if rating == Rating::Easy { self.w(16) } else { 1.0 };
                stability * (1.0 + self.w(8).exp() * (11.0 - difficulty) * stability.powf(-self.w(9)) * (((1.0 - retrievability) * self.w(10)).exp() - 1.0) * hard * easy)
            }
        };
        Self::clamp_stability(next)
    }

    /// A card reviewed at `at`, as `rating` says it was remembered. `seed`
    /// makes its fuzz the same each time the review is replayed.
    pub fn review(&self, memory: &Memory, rating: Rating, at: i64, seed: u64) -> Memory {
        let mut card = memory.clone();
        let days_since = card.last_review.map(|last| (at - last).div_euclid(DAY));
        let recent = days_since.is_some_and(|d| d < 1);

        // The memory: first sight, the same day, or after a while.
        match (card.stability, card.difficulty) {
            (Some(stability), Some(difficulty)) => {
                card.stability = Some(if recent {
                    self.short_term_stability(stability, rating)
                } else {
                    self.next_stability(difficulty, stability, self.retrievability(&card, at), rating)
                });
                card.difficulty = Some(self.next_difficulty(difficulty, rating));
            }
            _ => {
                card.stability = Some(self.initial_stability(rating));
                card.difficulty = Some(self.initial_difficulty(rating, true));
            }
        }
        let stability = card.stability.unwrap_or(STABILITY_MIN);

        // When next.
        let to_review = |card: &mut Memory| {
            card.state = State::Review;
            card.step = None;
            self.next_interval(stability, at) * DAY
        };
        let interval = match card.state {
            State::Learning | State::Relearning => {
                let steps = if card.state == State::Learning { &self.learning_steps } else { &self.relearning_steps };
                let step = card.step.unwrap_or(0);
                if steps.is_empty() || (step >= steps.len() && rating != Rating::Again) {
                    to_review(&mut card)
                } else {
                    match rating {
                        Rating::Again => {
                            card.step = Some(0);
                            steps[0]
                        }
                        Rating::Hard => match (step, steps.len()) {
                            (0, 1) => steps[0] * 3 / 2,
                            (0, _) => (steps[0] + steps[1]) / 2,
                            (step, _) => steps[step.min(steps.len() - 1)],
                        },
                        Rating::Good if step + 1 >= steps.len() => to_review(&mut card),
                        Rating::Good => {
                            card.step = Some(step + 1);
                            steps[step + 1]
                        }
                        Rating::Easy => to_review(&mut card),
                    }
                }
            }
            State::Review => match rating {
                Rating::Again if !self.relearning_steps.is_empty() => {
                    card.state = State::Relearning;
                    card.step = Some(0);
                    self.relearning_steps[0]
                }
                _ => self.next_interval(stability, at) * DAY,
            },
        };
        let mut interval = match self.fuzz && card.state == State::Review {
            true => self.fuzzed(interval, seed),
            false => interval,
        };
        // Never past the exam.
        if let Some(exam) = self.exam {
            if card.state == State::Review && at < exam && at + interval > exam {
                interval = exam - at;
            }
        }
        card.due = at + interval;
        card.last_review = Some(at);
        card.reviews += 1;
        card
    }

    /// An interval of days spread a little, over py-fsrs's range, so cards
    /// learnt together do not all fall due together — from a seed, not a
    /// random draw, so that replaying the reviews gives the same days. The
    /// draw is floored, as ts-fsrs floors it: py-fsrs rounds, which can land
    /// a day past the range's end.
    fn fuzzed(&self, interval: i64, seed: u64) -> i64 {
        let days = interval / DAY;
        if (days as f64) < 2.5 {
            return interval;
        }
        let ranges = [(2.5, 7.0, 0.15), (7.0, 20.0, 0.1), (20.0, f64::INFINITY, 0.05)];
        let delta: f64 = 1.0 + ranges.iter().map(|(start, end, factor)| factor * ((days as f64).min(*end) - start).max(0.0)).sum::<f64>();
        let max = ((days as f64 + delta).round() as i64).min(self.maximum_interval);
        let min = ((days as f64 - delta).round() as i64).max(2).min(max);
        let draw = unit(seed);
        let fuzzed = ((draw * (max - min + 1) as f64) + min as f64).floor() as i64;
        fuzzed.min(self.maximum_interval) * DAY
    }

    /// What each rating would make of a card now: when it would next be due.
    pub fn preview(&self, memory: &Memory, at: i64, seed: u64) -> [i64; 4] {
        Rating::ALL.map(|rating| self.review(memory, rating, at, seed).due - at)
    }
}

/// A number in [0, 1) from a seed: SplitMix64.
fn unit(seed: u64) -> f64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// A card's seed for its `n`th review, from its id.
pub fn seed(card: &str, n: u32) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in card.bytes().chain(n.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2022-11-29 12:30:00 UTC, where py-fsrs's tests begin.
    const START: i64 = 1_669_725_000;

    fn days(seconds: i64) -> i64 {
        seconds.div_euclid(DAY)
    }

    #[test]
    fn py_fsrs_test_review_card() {
        // tests/test_basic.py, test_review_card: no fuzz, each review when due.
        use Rating::*;
        let scheduler = Scheduler { fuzz: false, ..Scheduler::default() };
        let mut card = Memory::new(START);
        let mut at = START;
        let mut intervals = Vec::new();
        for rating in [Good, Good, Good, Good, Good, Good, Again, Again, Good, Good, Good, Good, Good] {
            card = scheduler.review(&card, rating, at, 0);
            intervals.push(days(card.due - card.last_review.unwrap()));
            at = card.due;
        }
        assert_eq!(intervals, [0, 2, 11, 46, 163, 498, 0, 0, 2, 4, 7, 12, 21]);
    }

    #[test]
    fn py_fsrs_test_memo_state() {
        // tests/test_basic.py, test_memo_state: reviewed after the given days.
        use Rating::*;
        let scheduler = Scheduler::default();
        let mut card = Memory::new(START);
        let mut at = START;
        for (rating, ivl) in [(Again, 0), (Good, 0), (Good, 1), (Good, 3), (Good, 8), (Good, 21)] {
            at += ivl * DAY;
            card = scheduler.review(&card, rating, at, 0);
        }
        assert!((card.stability.unwrap() - 53.62691).abs() < 1e-4, "stability {:?}", card.stability);
        assert!((card.difficulty.unwrap() - 6.3574867).abs() < 1e-4, "difficulty {:?}", card.difficulty);
    }

    #[test]
    fn first_ratings_go_through_the_steps() {
        let scheduler = Scheduler { fuzz: false, ..Scheduler::default() };
        let new = Memory::new(START);
        let [again, hard, good, easy] = scheduler.preview(&new, START, 0);
        assert_eq!((again, hard, good), (60, 330, 600), "one minute; halfway between the steps; ten minutes");
        assert!(easy >= DAY, "Easy graduates at once");
        let r = scheduler.retrievability(&new, START);
        assert_eq!(r, 0.0, "never reviewed: nothing to recall");
    }

    #[test]
    fn fuzz_is_deterministic_and_within_its_range() {
        let scheduler = Scheduler::default();
        let a = scheduler.fuzzed(30 * DAY, seed("D001-007", 4));
        let b = scheduler.fuzzed(30 * DAY, seed("D001-007", 4));
        assert_eq!(a, b, "a replayed review is fuzzed the same");
        let spread: std::collections::BTreeSet<i64> = (0..200).map(|n| scheduler.fuzzed(30 * DAY, seed("c", n)) / DAY).collect();
        assert!(spread.len() > 3 && spread.iter().all(|d| (27..=33).contains(d)), "{spread:?}");
        assert_eq!(scheduler.fuzzed(2 * DAY, 9), 2 * DAY, "under two and a half days, none");
    }

    #[test]
    fn nothing_is_scheduled_past_the_exam_and_its_run_in_asks_for_more() {
        use Rating::*;
        let exam = START + 20 * DAY;
        let scheduler = Scheduler { fuzz: false, exam: Some(exam), ..Scheduler::default() };
        let mut card = Memory::new(START);
        let mut at = START;
        for rating in [Good, Good, Good, Good, Good] {
            card = scheduler.review(&card, rating, at, 0);
            assert!(card.due <= exam || at >= exam, "due after the exam: {} > {exam}", card.due);
            at = card.due;
        }
        // In the run-in, the same card comes back sooner.
        let open = Scheduler { fuzz: false, ..Scheduler::default() };
        let reviewed = Memory { state: State::Review, step: None, stability: Some(10.0), difficulty: Some(5.0), due: START, last_review: Some(START - 10 * DAY), reviews: 3 };
        let far = open.review(&reviewed, Good, START, 0).due - START;
        let near = Scheduler { exam: Some(START + 30 * DAY), ..open.clone() }.review(&reviewed, Good, START, 0).due - START;
        assert!(near < far, "the run-in asks for more: {near} against {far}");
    }
}
