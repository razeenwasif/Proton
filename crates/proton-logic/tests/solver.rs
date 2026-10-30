//! L3: the model finder, on puzzles whose models are known: a puzzle with
//! one answer, n queens, Latin squares, colourings of the Petersen graph,
//! the pigeonhole principle. Every model found is checked by the evaluator,
//! which shares nothing with the search.

use std::time::Duration;

use proton_logic::eval::{broken, Model};
use proton_logic::solve::{solve, Outcome};
use proton_logic::{check, l4f, parse, Checked};

fn theory(text: &str) -> Checked {
    check(&parse(text).unwrap()).unwrap()
}

/// All of a theory's models, each checked to be one.
fn all(c: &Checked, keep: usize) -> Outcome {
    let out = solve(c, keep, true, Duration::from_secs(60)).unwrap();
    for m in &out.models {
        assert_eq!(broken(c, m).unwrap(), vec![], "a model the evaluator does not agree with: {m:?}");
    }
    out
}

fn at(m: &Model, f: &str, args: &[i64]) -> i64 {
    m.values[f][&args.to_vec()]
}

#[test]
fn a_puzzle_with_one_answer_has_one_model() {
    let c = theory(
        "friend = Ana | Ben | Cy\npet = cat | dog | eel\nfloor = 1..3\nkeeps: friend -> pet  one-to-one\nhome: friend -> floor  one-to-one\n\
         all f: keeps(f) = cat -> home(f) = 1\nall f: keeps(f) = cat -> home(Ben) > home(f)\nkeeps(Cy) = eel\nhome(Ana) != 3\nhome(Cy) < home(Ben)\n",
    );
    let out = all(&c, 10);
    assert!(out.finished);
    assert_eq!(out.found, 1);
    let m = &out.models[0];
    // Ana cat 1, Ben dog 3, Cy eel 2.
    assert_eq!([0, 1, 2].map(|f| at(m, "keeps", &[f])), [0, 1, 2]);
    assert_eq!([0, 1, 2].map(|f| at(m, "home", &[f])), [1, 3, 2]);
}

fn queens(n: i64) -> String {
    format!("file = 1..{n}\nrank = 1..{n}\nqueen: file -> rank  one-to-one\nall a, b: a != b -> abs(queen(a) - queen(b)) != abs(a - b)\n")
}

#[test]
fn n_queens_has_its_known_number_of_solutions() {
    for (n, want) in [(4, 2), (5, 10), (6, 4), (8, 92)] {
        let out = all(&theory(&queens(n)), 3);
        assert!(out.finished, "{n} queens did not finish");
        assert_eq!(out.found, want, "{n} queens");
    }
}

#[test]
fn eight_queens_from_logic4funs_text_has_the_same_92() {
    let c = theory(&queens(8));
    let (text, _) = l4f::write(&c).unwrap();
    let back = check(&l4f::read(&text).unwrap().0).unwrap();
    let out = all(&back, 1);
    assert_eq!((out.found, out.finished), (92, true));
}

fn latin(n: u32) -> String {
    format!(
        "row = size {n}\ncol = size {n}\nsym = size {n}\ncell: row, col -> sym\n\
         all r, a, b: a != b -> cell(r, a) != cell(r, b)\nall c, a, b: a != b -> cell(a, c) != cell(b, c)\n"
    )
}

#[test]
fn latin_squares_are_counted() {
    // Order 3 has 12, order 4 has 576.
    for (n, want) in [(3, 12), (4, 576)] {
        let out = all(&theory(&latin(n)), 2);
        assert_eq!((out.found, out.finished), (want, true), "order {n}");
    }
}

#[test]
fn the_petersen_graph_has_120_three_colourings() {
    let edges = [(0, 1), (1, 2), (2, 3), (3, 4), (4, 0), (0, 5), (1, 6), (2, 7), (3, 8), (4, 9), (5, 7), (7, 9), (9, 6), (6, 8), (8, 5)];
    let mut text = String::from("vertex = v0 | v1 | v2 | v3 | v4 | v5 | v6 | v7 | v8 | v9\ncolour = red | green | blue\npaint: vertex -> colour\n");
    for (a, b) in edges {
        text.push_str(&format!("paint(v{a}) != paint(v{b})\n"));
    }
    let out = all(&theory(&text), 1);
    assert_eq!((out.found, out.finished), (120, true));
}

#[test]
fn four_pigeons_have_no_three_holes_to_themselves() {
    let out = all(&theory("pigeon = size 4\nhole = size 3\nroost: pigeon -> hole  one-to-one\n"), 5);
    assert_eq!((out.found, out.finished, out.models.len()), (0, true, 0));
    // Nor does a constraint false of every model: none.
    let out = all(&theory("s = a | b\nf: s -> s\nall x: f(x) = a\nf(b) = b\n"), 5);
    assert_eq!((out.found, out.finished), (0, true));
}

#[test]
fn counting_quantifiers_partial_functions_and_onto() {
    // Two of three like it: three ways.
    let out = all(&theory("friend = Ana | Ben | Cy\nlikes: friend -> bool\nexactly 2 f: likes(f)\n"), 5);
    assert_eq!(out.found, 3);
    // A partial boss: no one their own, at most one without one.
    let out = all(&theory("p = a | b | c\nboss: p -> p  partial\nall x: defined(boss(x)) -> boss(x) != x\nat most 1 x: not defined(boss(x))\n"), 50);
    // Each of three has two choices, or none for one of them: 2^3 + 3 * 2^2.
    assert_eq!(out.found, 8 + 12);
    // Onto from three to two: six.
    let out = all(&theory("a = size 3\nb = size 2\nf: a -> b  onto\n"), 10);
    assert_eq!(out.found, 6);
    // Symmetric: a relation's pairs counted once.
    let out = all(&theory("p = size 3\nfriends: p, p -> bool  symmetric\nall x: not friends(x, x)\n"), 10);
    assert_eq!(out.found, 8);
}

#[test]
fn a_sort_of_a_range_of_sizes_is_searched_smallest_first() {
    // A derangement needs at least two.
    let out = all(&theory("s = size 1..4\nf: s -> s  one-to-one\nall x: f(x) != x\n"), 1);
    assert!(out.finished);
    assert_eq!(out.models[0].sizes["s"], 2);
    // Derangements of 2, 3 and 4: 1 + 2 + 9.
    assert_eq!(out.found, 12);
}

#[test]
fn propositions_and_constants_are_cells_too() {
    let out = all(&theory("p, q: bool\np -> q\nnot q or r\n"), 10);
    // p q r: models of (p -> q) and (q -> r): four.
    assert_eq!(out.found, 4);
    let out = all(&theory("n = 1..5\nx, y: n\nx + y = 6\nx < y\n"), 10);
    assert_eq!(out.found, 2);
}

/// A 9×9 Sudoku: a full grid with two clues in three taken away, solved.
#[test]
fn a_sudoku_is_solved() {
    let full = |r: i64, c: i64| (r * 3 + r / 3 + c) % 9 + 1;
    let mut text = String::from(
        "ix = 0..8\ndigit = 1..9\nthird = 0..2\ncell: ix, ix -> digit\nbox: ix -> third\n\
         all r, a, b: a != b -> cell(r, a) != cell(r, b)\n\
         all c, a, b: a != b -> cell(a, c) != cell(b, c)\n\
         all r, c, s, t: box(r) = box(s) and box(c) = box(t) and (r != s or c != t) -> cell(r, c) != cell(s, t)\n",
    );
    for i in 0..9 {
        text.push_str(&format!("box({i}) = {}\n", i / 3));
    }
    for r in 0..9 {
        for c in 0..9 {
            if (r * 9 + c) % 3 == 0 {
                text.push_str(&format!("cell({r}, {c}) = {}\n", full(r, c)));
            }
        }
    }
    let c = theory(&text);
    let out = solve(&c, 1, false, Duration::from_secs(60)).unwrap();
    println!("sudoku: {} ms, {} choices", out.elapsed_ms, out.choices);
    assert!(out.found == 1 && !out.finished && !out.timed_out, "{out:?}");
    let m = &out.models[0];
    assert_eq!(broken(&c, m).unwrap(), vec![]);
    for r in 0..9 {
        for col in 0..9 {
            if (r * 9 + col) % 3 == 0 {
                assert_eq!(at(m, "cell", &[r, col]), full(r, col));
            }
        }
    }
}
