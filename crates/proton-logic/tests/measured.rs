//! L9: Logic measured. Pelletier's problems (Seventy-five problems for
//! testing automatic theorem provers, 1986), written here in Proton's
//! language, through the tableau and each proof system; the calculator at
//! the sizes it promises; and what runs on every keystroke — reading a
//! file, and checking a proof — on files larger than anyone types. Each is
//! said, and in a release build held to its budget.
//!
//! ```text
//! cargo test --release -p proton-logic --test measured -- --nocapture --test-threads 1
//! ```

use std::time::{Duration, Instant};

use proton_logic::ast::{Item, Question};
use proton_logic::{calc, check, parse, proofs, resolution, sequent, tableau, Checked, Expr};

/// What took how long, said; and in a release build, held to its budget.
fn timed<T>(what: &str, budget_ms: u64, run: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = run();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("{what}: {ms:.1} ms");
    if !cfg!(debug_assertions) {
        assert!(ms <= budget_ms as f64, "{what} took {ms:.1} ms, over its {budget_ms} ms");
    }
    out
}

/// A question read and checked, as a worksheet's line is.
fn asked(question: &str) -> (Checked, Question) {
    let c = check(&parse(&format!("? {question}")).unwrap()).unwrap();
    let q = c.theory.lines.iter().find_map(|l| match &l.item {
        Some(Item::Question(q)) => Some(q.clone()),
        _ => None,
    });
    (c, q.unwrap())
}

fn argument(premises: &str, goal: &str) -> (Checked, Vec<Expr>, Expr) {
    let text = if premises.is_empty() { format!("|= {goal}") } else { format!("{premises} |= {goal}") };
    match asked(&text) {
        (c, Question::Entails(p, g)) => (c, p, g),
        (_, q) => panic!("{text} read as {q:?}"),
    }
}

/// Pelletier's problems, each its number, premises and goal. All hold.
const PELLETIER: &[(&str, &str, &str)] = &[
    ("1", "", "(p -> q) <-> (not q -> not p)"),
    ("2", "", "not not p <-> p"),
    ("3", "", "not (p -> q) -> (q -> p)"),
    ("4", "", "(not p -> q) <-> (not q -> p)"),
    ("5", "", "((p or q) -> (p or r)) -> (p or (q -> r))"),
    ("6", "", "p or not p"),
    ("7", "", "p or not not not p"),
    ("8", "", "((p -> q) -> p) -> p"),
    ("9", "", "((p or q) and (not p or q) and (p or not q)) -> not (not p or not q)"),
    ("10", "q -> r, r -> p and q, p -> q or r", "p <-> q"),
    ("11", "", "p <-> p"),
    ("12", "", "((p <-> q) <-> r) <-> (p <-> (q <-> r))"),
    ("13", "", "p or (q and r) <-> (p or q) and (p or r)"),
    ("14", "", "(p <-> q) <-> ((q or not p) and (not q or p))"),
    ("15", "", "(p -> q) <-> (not p or q)"),
    ("16", "", "(p -> q) or (q -> p)"),
    ("17", "", "((p and (q -> r)) -> s) <-> ((not p or q or s) and (not p or not r or s))"),
    ("18", "", "some y: all x: F(y) -> F(x)"),
    ("19", "", "some x: all y, z: (P(y) -> Q(z)) -> (P(x) -> Q(x))"),
    ("20", "all x, y: some z: all w: (P(x) and Q(y) -> R(z) and S(w))", "(some x, y: P(x) and Q(y)) -> some z: R(z)"),
    ("21", "some x: p -> F(x), some x: F(x) -> p", "some x: p <-> F(x)"),
    ("22", "", "(all x: p <-> F(x)) -> (p <-> all x: F(x))"),
    ("23", "", "(all x: p or F(x)) <-> (p or all x: F(x))"),
    ("24", "not (some x: S(x) and Q(x)), all x: P(x) -> Q(x) or R(x), not (some x: P(x)) -> some x: Q(x), all x: Q(x) or R(x) -> S(x)", "some x: P(x) and R(x)"),
    ("25", "some x: P(x), all x: F(x) -> not G(x) and R(x), all x: P(x) -> G(x) and F(x), (all x: P(x) -> Q(x)) or (some x: P(x) and R(x))", "some x: Q(x) and P(x)"),
    ("26", "(some x: P(x)) <-> (some x: Q(x)), all x, y: P(x) and Q(y) -> (R(x) <-> S(y))", "(all x: P(x) -> R(x)) <-> (all x: Q(x) -> S(x))"),
    ("27", "some x: F(x) and not G(x), all x: F(x) -> H(x), all x: J(x) and I(x) -> F(x), (some x: H(x) and not G(x)) -> all x: I(x) -> not H(x)", "all x: J(x) -> not I(x)"),
    ("29", "(some x: F(x)) and (some x: G(x))", "((all x: F(x) -> H(x)) and (all x: G(x) -> J(x))) <-> (all x, y: F(x) and G(y) -> H(x) and J(y))"),
    ("30", "all x: F(x) or G(x) -> not H(x), all x: (G(x) -> not I(x)) -> F(x) and H(x)", "all x: I(x)"),
    ("31", "not (some x: F(x) and (G(x) or H(x))), some x: I(x) and F(x), all x: not H(x) -> J(x)", "some x: I(x) and J(x)"),
    ("32", "all x: F(x) and (G(x) or H(x)) -> I(x), all x: I(x) and H(x) -> J(x), all x: K(x) -> H(x)", "all x: F(x) and K(x) -> J(x)"),
    ("33", "", "(all x: P(a) and (P(x) -> P(b)) -> P(c)) <-> (all x: (not P(a) or P(x) or P(c)) and (not P(a) or not P(b) or P(c)))"),
    ("35", "", "some x, y: P(x, y) -> all x, y: P(x, y)"),
    ("39", "", "not (some x: all y: F(y, x) <-> not F(y, y))"),
    ("40", "", "(some y: all x: F(x, y) <-> F(x, x)) -> not (all x: some y: all z: F(z, y) <-> not F(z, x))"),
    ("42", "", "not (some y: all x: F(x, y) <-> not (some z: F(x, z) and F(z, x)))"),
    ("44", "all x: F(x) -> (some y: G(y) and H(x, y)) and (some y: G(y) and not H(x, y)), some x: J(x) and (all y: G(y) -> H(x, y))", "some x: J(x) and not F(x)"),
];

#[test]
fn pelletiers_problems_through_the_tableau_and_each_system() {
    let mut said = Vec::new();
    for (n, premises, goal) in PELLETIER {
        let (c, ps, g) = argument(premises, goal);
        // The tableau answers as a line is typed: in 0.8 s, or "Try harder".
        let t = timed(&format!("P{n} tableau"), 800, || tableau::prove(&c, &ps, &g, Duration::from_millis(800)).unwrap());
        let mut row = vec![format!("P{n}"), match t.valid {
            Some(true) => "proved".into(),
            Some(false) => panic!("P{n} holds, and the tableau says it does not"),
            None => format!("undecided: {}", t.note),
        }];
        for system in proofs::SYSTEMS {
            // Found in the window's three seconds, or stopped on time.
            let found = timed(&format!("P{n} {system}"), 3100, || proofs::find(system, &ps, &g, Duration::from_secs(3)));
            row.push(match found {
                Ok(text) => {
                    let r = proofs::check(&text);
                    assert!(r.complete, "P{n} {system}: what was found does not check:\n{text}\n{}", r.summary);
                    format!("{} lines", r.lines.len())
                }
                Err(why) => {
                    assert!(!why.contains("does not hold") && !why.contains("saturate"), "P{n} holds, and {system} says: {why}");
                    "not found".into()
                }
            });
        }
        said.push(row.join(" | "));
    }
    println!("{}", said.join("\n"));
}

/// The calculator at the sizes LOGIC.md promises it.
#[test]
fn the_calculator_at_its_sizes() {
    // A chain over n propositions: p1 -> p2 -> … , each and the next xor'd in.
    let chain = |n: usize| (1..n).map(|i| format!("(p{i} -> p{})", i + 1)).collect::<Vec<_>>().join(" and ");
    let formula = |f: &str| match asked(f) {
        (c, q @ Question::Formula(_)) => (c, q),
        (_, q) => panic!("{f} read as {q:?}"),
    };
    // Ten propositions: the least forms as it is typed.
    let (c, q) = formula(&format!("{} or (p1 xor p10)", chain(10)));
    let a = timed("10 propositions, the table and least forms", 300, || calc::ask(&c, &q).unwrap());
    assert!(a.table.is_some());
    // Sixteen: the most a table is shown for.
    let (c, q) = formula(&format!("{} or (p1 xor p16)", chain(16)));
    let a = timed("16 propositions, the table shown", 500, || calc::ask(&c, &q).unwrap());
    assert_eq!(a.table.unwrap()[0].values.len(), 1 << 16);
    // Twenty: counted, not shown.
    let (c, q) = formula(&format!("{} or (p1 xor p20)", chain(20)));
    let a = timed("20 propositions, the rows counted", 1000, || calc::ask(&c, &q).unwrap());
    assert!(a.table.is_none());
    // Sixty: past counting, the search's.
    let (c, q) = formula(&format!("{} and p1 and not p60", chain(60)));
    let a = timed("60 propositions, by search", 500, || calc::ask(&c, &q).unwrap());
    match a.what {
        calc::What::Formula(f) => assert_eq!(f.status, calc::Status::Contradiction),
        w => panic!("{w:?}"),
    }
    // An argument and an equivalence over twelve.
    let (c, q) = asked(&format!("{}, p1 |= p12", chain(12)));
    let a = timed("an argument over 12 propositions", 300, || calc::ask(&c, &q).unwrap());
    assert!(matches!(a.what, calc::What::Argument { valid: true, .. }));
    let (c, q) = asked(&format!("{} == not ({})", chain(12), (1..12).map(|i| format!("(p{i} and not p{})", i + 1)).collect::<Vec<_>>().join(" or ")));
    let a = timed("an equivalence over 12 propositions", 300, || calc::ask(&c, &q).unwrap());
    assert!(matches!(a.what, calc::What::Same { same: true, .. }));
}

/// What runs on each keystroke: the file read and checked, a proof's lines
/// checked — on a theory of 2,000 lines and a proof of 600.
#[test]
fn what_runs_as_a_file_is_typed() {
    let mut theory = String::from("person = size 40\nthing = 1..30\n");
    for i in 0..1000 {
        theory.push_str(&format!("f{i}: person -> thing\nall x: f{i}(x) != {} or some y: f{i}(y) > f{i}(x)\n", i % 30));
    }
    let lines = timed("a theory of 2,002 lines, read and checked", 100, || {
        let t = parse(&theory).unwrap();
        check(&t).unwrap();
        t.lines.len()
    });
    assert!(lines >= 2000);
    // A proof of 600 lines: p0, then p(i) -> p(i+1) and its →E, 300 times.
    let mut premises = vec!["p0".to_string()];
    let mut rows = vec![("p0".to_string(), "premise".to_string())];
    for i in 0..300 {
        premises.push(format!("p{i} -> p{}", i + 1));
        rows.push((format!("p{i} -> p{}", i + 1), "premise".into()));
        let at = rows.len();
        rows.push((format!("p{}", i + 1), format!("→E {at}, {}", if i == 0 { 1 } else { at - 1 })));
    }
    let header = proofs::write_header("fitch", &parse(&format!("? {} |= p300", premises.join(", "))).ok().and_then(|t| match t.lines.into_iter().find_map(|l| l.item) {
        Some(Item::Question(Question::Entails(p, _))) => Some(p),
        _ => None,
    }).unwrap(), &parse("? p300").ok().and_then(|t| match t.lines.into_iter().find_map(|l| l.item) {
        Some(Item::Question(Question::Formula(g))) => Some(g),
        _ => None,
    }).unwrap());
    let text = format!("{header}{}", proofs::write_lines(&rows));
    let r = timed("a proof of 601 lines, checked", 100, || proofs::check(&text));
    assert!(r.complete && r.lines.len() == 601, "{}", r.summary);
    // The longest proofs found above, checked again: as a keystroke would.
    let (_, ps, g) = argument(PELLETIER[25].1, PELLETIER[25].2);
    for system in ["sequent", "resolution"] {
        let text = match system {
            "sequent" => sequent::find(&ps, &g, Duration::from_secs(5)).unwrap(),
            _ => resolution::find(&ps, &g, Duration::from_secs(5)).unwrap(),
        };
        let r = timed(&format!("P26's {system} proof, checked"), 50, || proofs::check(&text));
        assert!(r.complete);
    }
}


/// A random formula over `atoms` propositions, `depth` connectives deep.
fn random(seed: &mut u64, atoms: u64, depth: u32) -> String {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let r = *seed >> 33;
    if depth == 0 || r % 7 == 0 {
        return format!("p{}", r % atoms);
    }
    let (a, b) = (random(seed, atoms, depth - 1), random(seed, atoms, depth - 1));
    match r % 5 {
        0 => format!("not ({a})"),
        1 => format!("({a}) and ({b})"),
        2 => format!("({a}) or ({b})"),
        3 => format!("({a}) -> ({b})"),
        _ => format!("({a}) <-> ({b})"),
    }
}

/// Propositional logic is decided: every tautology, however its
/// connectives nest, is proved in each system — the promise LOGIC.md makes.
#[test]
fn every_propositional_tautology_is_proved_in_each_system() {
    let mut seed = 20_261_030;
    let mut proved = 0;
    while proved < 40 {
        // A formula or'd with its own negation's part, so many are tautologies.
        let f = random(&mut seed, 4, 4);
        let g = random(&mut seed, 4, 3);
        let tried = format!("({f}) or not ({g})");
        let (c, q) = asked(&tried);
        let Ok(calc::Answer { what: calc::What::Formula(a), .. }) = calc::ask(&c, &q) else { continue };
        if a.status != calc::Status::Tautology {
            continue;
        }
        let Question::Formula(goal) = q else { unreachable!() };
        for system in proofs::SYSTEMS {
            let text = proofs::find(system, &[], &goal, Duration::from_secs(20)).unwrap_or_else(|e| panic!("{system}: {tried}: {e}"));
            assert!(proofs::check(&text).complete, "{system}: {tried}:\n{text}");
        }
        proved += 1;
    }
}
