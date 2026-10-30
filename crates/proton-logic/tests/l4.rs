//! L4: symmetry broken without losing models or changing counts; why a
//! theory has no model; every puzzle the same through Logic4Fun's text;
//! and the solver's timing budgets, kept by a release build.
//!
//! ```text
//! cargo test --release -p proton-logic --test l4 -- --nocapture
//! ```

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use proton_logic::eval::broken;
use proton_logic::solve::{conflict, interchangeable, solve};
use proton_logic::{check, l4f, parse, Checked};

fn theory(text: &str) -> Checked {
    check(&parse(text).unwrap()).unwrap()
}

fn count(c: &Checked) -> u64 {
    let out = solve(c, 1, true, Duration::from_secs(60)).unwrap();
    assert!(out.finished && !out.symmetry);
    out.found
}

#[test]
fn unnamed_elements_are_interchangeable_until_something_orders_them() {
    assert_eq!(interchangeable(&theory("s = size 3\nf: s -> s\n")), vec!["s"]);
    // Named, a range, or put in order: not.
    assert!(interchangeable(&theory("s = a | b\nt = 1..3\nf: s -> t\n")).is_empty());
    assert!(interchangeable(&theory("s = size 3\nf: s -> s\nall x: f(x) >= x\n")).is_empty());
    assert!(interchangeable(&theory("s = size 3\nf: s -> s\nf(first(s)) = first(s)\n")).is_empty());
    // Equal and not equal tell nothing apart.
    assert_eq!(interchangeable(&theory("s = size 3\nf: s -> s\nall x: f(x) != x\n")), vec!["s"]);
}

#[test]
fn symmetry_is_broken_when_finding_and_not_when_counting() {
    let c = theory("s = size 3\nf: s -> s\n");
    assert_eq!(count(&c), 27);
    let found = solve(&c, 100, false, Duration::from_secs(10)).unwrap();
    assert!(found.symmetry && found.finished);
    // The 27 functions of three unnamed things fall into 7 kinds; the
    // least-number rule finds each at least once, and far fewer than 27.
    assert!((7..27).contains(&found.models.len()), "{}", found.models.len());
    for m in &found.models {
        assert_eq!(broken(&c, m).unwrap(), vec![]);
    }
    // Whatever has a model still has one found.
    for (text, sat) in [
        ("p = size 3\nh = size 3\nroost: p -> h  one-to-one\n", true),
        ("p = size 4\nh = size 3\nroost: p -> h  one-to-one\n", false),
        ("s = size 1..4\nf: s -> s  one-to-one\nall x: f(x) != x\n", true),
        ("n = size 4\ncell: n, n -> n\nall r, a, b: a != b -> cell(r, a) != cell(r, b)\nall k, a, b: a != b -> cell(a, k) != cell(b, k)\n", true),
    ] {
        let out = solve(&theory(text), 1, false, Duration::from_secs(10)).unwrap();
        assert_eq!(out.found > 0, sat, "{text}");
    }
    // Latin squares, counted, are still 576.
    let latin = theory("n = size 4\ncell: n, n -> n\nall r, a, b: a != b -> cell(r, a) != cell(r, b)\nall k, a, b: a != b -> cell(a, k) != cell(b, k)\n");
    assert_eq!(count(&latin), 576);
    let some = solve(&latin, 1000, false, Duration::from_secs(10)).unwrap();
    assert!(some.finished && some.found < 576, "{}", some.found);
}

#[test]
fn no_model_is_explained_by_the_fewest_lines_that_conflict() {
    // Lines 3, 4 and 6 cannot all hold; line 5 has nothing to do with it.
    let c = theory("n = 1..5\nx, y: n\nx > 3\ny > x\nx + y >= 2\ny < 3\n");
    assert_eq!(solve(&c, 1, true, Duration::from_secs(5)).unwrap().found, 0);
    let why = conflict(&c, Duration::from_secs(10), &AtomicBool::new(false)).unwrap();
    assert_eq!((why.lines, why.kinds.len(), why.minimal), (vec![3, 4, 6], 0, true));
    // Four pigeons, three holes: the one-to-one alone.
    let c = theory("p = size 4\nh = size 3\nroost: p -> h  one-to-one\n");
    let why = conflict(&c, Duration::from_secs(10), &AtomicBool::new(false)).unwrap();
    assert_eq!(why.lines, Vec::<usize>::new());
    assert_eq!(why.kinds, vec![(3, "roost".to_string(), "one-to-one".to_string())]);
    // A kind and a line together.
    let c = theory("p = a | b | c\nf: p -> p  one-to-one\nf(a) = b\nf(c) = b\nf(b) = c\n");
    let why = conflict(&c, Duration::from_secs(10), &AtomicBool::new(false)).unwrap();
    assert_eq!((why.lines.clone(), why.kinds.len()), (vec![3, 4], 1), "{why:?}");
}

/// Every puzzle of the solver's tests, written as Logic4Fun's text and read
/// back, has as many models.
#[test]
fn every_puzzle_has_the_same_models_through_logic4funs_text() {
    let puzzles = [
        "friend = Ana | Ben | Cy\npet = cat | dog | eel\nfloor = 1..3\nkeeps: friend -> pet  one-to-one\nhome: friend -> floor  one-to-one\n\
         all f: keeps(f) = cat -> home(f) = 1\nall f: keeps(f) = cat -> home(Ben) > home(f)\nkeeps(Cy) = eel\nhome(Ana) != 3\nhome(Cy) < home(Ben)\n",
        "file = 1..6\nrank = 1..6\nqueen: file -> rank  one-to-one\nall a, b: a != b -> abs(queen(a) - queen(b)) != abs(a - b)\n",
        "row = size 3\ncol = size 3\nsym = size 3\ncell: row, col -> sym\nall r, a, b: a != b -> cell(r, a) != cell(r, b)\nall k, a, b: a != b -> cell(a, k) != cell(b, k)\n",
        "friend = Ana | Ben | Cy\nlikes: friend -> bool\nexactly 2 f: likes(f)\n",
        "p = a | b | c\nboss: p -> p  partial\nall x: defined(boss(x)) -> boss(x) != x\nat most 1 x: not defined(boss(x))\n",
        "a = size 3\nb = size 2\nf: a -> b  onto\n",
        "p = size 3\nfriends: p, p -> bool  symmetric\nall x: not friends(x, x)\n",
        "s = a < b < c < d\nnext: s -> s  partial\nall x: defined(next(x)) <-> x != last(s)\nall x: defined(next(x)) -> next(x) = succ(x)\n",
        "p, q: bool\np -> q\nnot q or r\n",
    ];
    for text in puzzles {
        let c = theory(text);
        let (written, notes) = l4f::write(&c).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert!(notes.is_empty(), "{text}: {notes:?}");
        let back = check(&l4f::read(&written).unwrap_or_else(|e| panic!("{written}: {e}")).0).unwrap_or_else(|e| panic!("{written}: {e:?}"));
        assert_eq!(count(&back), count(&c), "{text}\nas\n{written}");
    }
}

/// The classic five houses: who drinks water, and who owns the zebra.
const ZEBRA: &str = "\
house = 1..5
colour = red | green | ivory | yellow | blue
nation = English | Spaniard | Ukrainian | Norwegian | Japanese
drink = coffee | tea | milk | juice | water
smoke = OldGold | Kools | Chesterfield | LuckyStrike | Parliament
pet = dog | snails | fox | horse | zebra
c: colour -> house  one-to-one
n: nation -> house  one-to-one
d: drink -> house  one-to-one
s: smoke -> house  one-to-one
p: pet -> house  one-to-one
n(English) = c(red)
n(Spaniard) = p(dog)
d(coffee) = c(green)
n(Ukrainian) = d(tea)
c(green) = c(ivory) + 1
s(OldGold) = p(snails)
s(Kools) = c(yellow)
d(milk) = 3
n(Norwegian) = 1
abs(s(Chesterfield) - p(fox)) = 1
abs(s(Kools) - p(horse)) = 1
s(LuckyStrike) = d(juice)
n(Japanese) = s(Parliament)
abs(n(Norwegian) - c(blue)) = 1
";

/// A Sudoku of 17 clues, the fewest a Sudoku with one answer can have: the
/// first of Gordon Royle's list of them. `0` is a blank.
const SEVENTEEN: &str = "000000010400000000020000000000050407008000300001090000300400200050100000000806000";

fn sudoku(grid: &str) -> String {
    let mut text = String::from(
        "ix = 0..8\ndigit = 1..9\nthird = 0..2\ncell: ix, ix -> digit\nbox: ix -> third\n\
         all r, a, b: a != b -> cell(r, a) != cell(r, b)\n\
         all k, a, b: a != b -> cell(a, k) != cell(b, k)\n\
         all r, k, s, t: box(r) = box(s) and box(k) = box(t) and (r != s or k != t) -> cell(r, k) != cell(s, t)\n",
    );
    for i in 0..9 {
        text.push_str(&format!("box({i}) = {}\n", i / 3));
    }
    for (i, ch) in grid.chars().enumerate() {
        if ch != '0' {
            text.push_str(&format!("cell({}, {}) = {ch}\n", i / 9, i % 9));
        }
    }
    text
}

/// What took how long, said; and in a release build, held to its budget.
fn timed(what: &str, budget_ms: u64, run: impl FnOnce() -> String) {
    let t = Instant::now();
    let said = run();
    let ms = t.elapsed().as_millis() as u64;
    println!("{what}: {ms} ms — {said}");
    if !cfg!(debug_assertions) {
        assert!(ms <= budget_ms, "{what} took {ms} ms, over its {budget_ms} ms");
    }
}

#[test]
fn the_solvers_timing_budgets() {
    timed("eight queens, all 92", 1000, || {
        let c = theory("file = 1..8\nrank = 1..8\nqueen: file -> rank  one-to-one\nall a, b: a != b -> abs(queen(a) - queen(b)) != abs(a - b)\n");
        let out = solve(&c, 1, true, Duration::from_secs(60)).unwrap();
        assert_eq!(out.found, 92);
        format!("{} choices", out.choices)
    });
    timed("five houses, every answer", 1000, || {
        let c = theory(ZEBRA);
        let out = solve(&c, 2, true, Duration::from_secs(60)).unwrap();
        assert_eq!((out.found, out.finished), (1, true));
        let m = &out.models[0];
        // The Norwegian drinks water; the Japanese owns the zebra.
        assert_eq!(m.values["d"][&vec![4]], m.values["n"][&vec![3]]);
        assert_eq!(m.values["p"][&vec![4]], m.values["n"][&vec![4]]);
        format!("{} choices", out.choices)
    });
    timed("a Sudoku of 17 clues", 1000, || {
        let c = theory(&sudoku(SEVENTEEN));
        let out = solve(&c, 1, false, Duration::from_secs(60)).unwrap();
        assert_eq!(out.found, 1, "{out:?}");
        assert_eq!(broken(&c, &out.models[0]).unwrap(), vec![]);
        format!("{} choices", out.choices)
    });
}

/// Logic4Fun's language at large: cardinality ranges and sizes from other
/// sorts, an alias, partial functions and EST, MIN and MAX, SUCC and PRED,
/// NOT, XOR, IMP and IFF written both ways, clause form with `;`, a binary
/// predicate written between its arguments, negated comparisons, constants,
/// and comments — read, solved, written back, read again: the same models.
#[test]
fn logic4funs_language_at_large() {
    let text = "\
Sorts:
% a sort of 3, one of 4, one of 2 to 3
level enum: low, mid, high.
slot {cardinality = level.size + 1 alias place}
spare {cardinality >= 2 cardinality <= 3}

Vocabulary:
function rank(level) : place {all_different}
function next(slot) : slot {partial}
predicate above(slot, slot)
name top : slot
name lucky : bool

Constraints:
rank(low) < rank(mid).
NOT(rank(high) = MIN_slot).
ALL x (EST(next(x)) IFF x != MAX_slot).
ALL x (EST(next(x)) IMP next(x) = SUCC(x)).
above(x, y) -> x > y.
x > y -> x above y.
top !<= rank(high) ; lucky.
top = MAX_slot.
lucky XOR rank(mid) !>= 2.
";
    let (t, notes) = l4f::read(text).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    // `X alias Y` names X again: an alias naming a sort already declared is refused.
    assert!(l4f::read("Sorts:\ns cardinality = 2.\nt alias s.\n").unwrap_err().message.contains("already"));
    let c = check(&t).unwrap_or_else(|e| panic!("{e:?}"));
    let n = count(&c);
    assert!(n > 0);
    // Worked out by hand: rank is one-to-one from 3 levels into 4 slots, low
    // below mid, high not 0; top is 3; lucky differs from rank(mid) < 2, so
    // is rank(mid) >= 2, and must be true when rank(high) = 3; spare is 2 or
    // 3 and free.
    let mut by_hand = 0;
    for lo in 0..4 {
        for mi in 0..4 {
            for hi in 0..4 {
                if lo == mi || mi == hi || lo == hi || lo >= mi || hi == 0 {
                    continue;
                }
                let lucky = mi >= 2;
                // `top !<= rank(high) ; lucky`: top > rank(high), or lucky.
                if !(3 > hi || lucky) {
                    continue;
                }
                by_hand += 1;
            }
        }
    }
    // Two sizes of spare, which nothing else says anything of.
    assert_eq!(n, by_hand * 2, "the solver's {n} against {by_hand} × 2");
    let (written, _) = l4f::write(&c).unwrap();
    let back = check(&l4f::read(&written).unwrap().0).unwrap();
    assert_eq!(count(&back), n, "{written}");
}
