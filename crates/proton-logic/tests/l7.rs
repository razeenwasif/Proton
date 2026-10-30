//! L7: the sequent calculus and resolution — proofs written here, right and
//! wrong, each mistake named; clause form step by step; and proofs found for
//! the arguments L5's tableaux were tried on, checked as any proof is.

use std::time::Duration;

use proton_logic::parse::{formula, formulas};
use proton_logic::proofs::{check, Report};
use proton_logic::{resolution, sequent};

fn argument(premises: &str, goal: &str) -> (Vec<proton_logic::Expr>, proton_logic::Expr) {
    (if premises.is_empty() { Vec::new() } else { formulas(premises).unwrap() }, formula(goal).unwrap())
}

fn checked(text: &str) -> Report {
    let r = check(text);
    assert!(r.header.unread.is_empty(), "{:?}", r.header.unread);
    r
}

/// The mistake on proof line `n`, which must be the only one.
fn wrong(text: &str, n: usize) -> String {
    let r = checked(text);
    let bad: Vec<_> = r.lines.iter().filter(|l| !l.ok).collect();
    assert_eq!(bad.iter().map(|l| l.n).collect::<Vec<_>>(), vec![n], "{:?}", r.lines);
    assert!(!r.complete);
    bad[0].message.clone()
}

const VALID: [(&str, &str); 8] = [
    ("p -> q, q -> r", "p -> r"),
    ("", "not (p and q) <-> not p or not q"),
    ("", "((p -> q) -> p) -> p"),
    ("", "p or not p"),
    ("p and q", "q and p"),
    ("p <-> q, q", "p"),
    ("p xor q, p", "not q"),
    ("false", "q"),
];

const FIRST_ORDER: [(&str, &str); 9] = [
    ("all x: P(x) -> Q(x), P(a)", "Q(a)"),
    ("all x: P(x) -> Q(x), all x: Q(x) -> R(x)", "all x: P(x) -> R(x)"),
    ("some y: all x: L(x, y)", "all x: some y: L(x, y)"),
    ("not (all x: P(x))", "some x: not P(x)"),
    ("", "(all x: P(x)) -> P(c)"),
    ("", "some x: D(x) -> all y: D(y)"),
    ("all x, y: some z: all w: (P(x) and Q(y) -> R(z) and S(w))", "(some x, y: P(x) and Q(y)) -> some z: R(z)"),
    ("(some x: P(x)) <-> (some x: Q(x)), all x, y: P(x) and Q(y) -> (R(x) <-> S(y))", "(all x: P(x) -> R(x)) <-> (all x: Q(x) -> S(x))"),
    ("all x: P(x) -> P(f(x)), P(a)", "P(f(f(a)))"),
];

const INVALID: [(&str, &str); 4] = [("q, p -> q", "p"), ("p -> q", "q -> p"), ("p or q", "p"), ("", "p and not p")];

/* ── the sequent calculus ─────────────────────────────────────────── */

const MODUS_PONENS: &str = "\
system sequent
premises p -> q, p
goal q

1  p ⊢ p, q          Ax
2  q, p ⊢ q          Ax
3  p -> q, p ⊢ q     →L 1, 2
";

#[test]
fn a_sequent_proof_is_checked_line_by_line() {
    let r = checked(MODUS_PONENS);
    assert!(r.complete, "{r:?}");
    assert!(r.summary.starts_with("a proof of q from p -> q, p"), "{}", r.summary);
    assert_eq!(r.lines[2].text, "p -> q, p ⊢ q");
    // Other names for the rule, and |- for the turnstile.
    assert!(checked(&MODUS_PONENS.replace("→L 1, 2", "->L 2, 1").replace("p -> q, p ⊢ q ", "p -> q, p |- q ")).complete);
    // A line above with what the rule does not give.
    let r = checked(&MODUS_PONENS.replace("2  q, p ⊢ q ", "2  r, p ⊢ q "));
    assert!(!r.lines[1].ok && r.lines[1].message.contains("axiom"), "{:?}", r.lines[1]);
    assert!(r.lines[2].message.contains("line 2 has r on its left"), "{:?}", r.lines[2]);
    let m = wrong(&MODUS_PONENS.replace("1  p ⊢ p, q ", "1  p ⊢ p, r "), 3);
    assert!(m.contains("has r on its right"), "{m}");
    // The wrong connective.
    let m = wrong(&MODUS_PONENS.replace("→L 1, 2", "∧L 1"), 3);
    assert!(m.contains("∧L has 1 line") || m.contains("conjunction"), "{m}");
    let m = wrong(&MODUS_PONENS.replace("→L 1, 2", "∧R 1, 2"), 3);
    assert!(m.contains("∧R needs a conjunction on the right"), "{m}");
    // Not the argument yet: the last line has more on its left.
    let r = checked(&MODUS_PONENS.replace("goal q", "goal r"));
    assert!(!r.complete && r.summary.contains("not the premises ⊢ the goal"), "{}", r.summary);
}

#[test]
fn the_quantifier_rules_keep_their_conditions() {
    let good = "\
system sequent
premises all x: P(x) and Q(x)
goal all x: P(x)

1  P(a), Q(a) ⊢ P(a)            Ax
2  P(a) and Q(a) ⊢ P(a)         ∧L 1
3  all x: P(x) and Q(x) ⊢ P(a)  ∀L 2
4  all x: P(x) and Q(x) ⊢ all x: P(x)   ∀R 3
";
    assert!(checked(good).complete, "{:?}", checked(good));
    // ∀R of a name the sequent below has.
    let bad = "\
system sequent
premises P(a)
goal all x: P(x)

1  P(a) ⊢ P(a)           Ax
2  P(a) ⊢ all x: P(x)    ∀R 1
";
    let m = wrong(bad, 2);
    assert!(m.contains("does not mention: a is in it"), "{m}");
    // ∀L with something that is not an instance.
    let m = wrong(&good.replace("3  all x: P(x) and Q(x) ⊢ P(a)  ∀L 2", "3  all x: P(x) or Q(x) ⊢ P(a)   ∀L 2").replace("4  all x: P(x) and Q(x)", "4  all x: P(x) or Q(x)").replace("premises all x: P(x) and Q(x)", "premises all x: P(x) or Q(x)"), 3);
    assert!(m.contains("which is not"), "{m}");
    // Cut, and weakening.
    let cut = "\
system sequent
premises p
goal p or q

1  p ⊢ p              Ax
2  p ⊢ p, q           WR 1
3  p ⊢ p or q         ∨R 2
4  p, p or q ⊢ p or q Ax
5  p ⊢ p or q         Cut 3, 4
";
    assert!(checked(cut).complete, "{:?}", checked(cut));
}

#[test]
fn sequent_proofs_are_found_for_valid_arguments() {
    for (ps, g) in VALID.iter().chain(&FIRST_ORDER) {
        let (premises, goal) = argument(ps, g);
        let text = sequent::find(&premises, &goal, Duration::from_secs(20)).unwrap_or_else(|e| panic!("{ps} |= {g}: {e}"));
        assert!(checked(&text).complete, "{ps} |= {g}:\n{text}");
    }
}

#[test]
fn no_sequent_proof_is_found_for_what_does_not_follow() {
    for (ps, g) in INVALID {
        let (premises, goal) = argument(ps, g);
        let e = sequent::find(&premises, &goal, Duration::from_secs(2)).unwrap_err();
        assert!(e.contains("does not hold"), "{ps} |= {g}: {e}");
    }
    let (premises, goal) = argument("some x: P(x)", "all x: P(x)");
    assert!(sequent::find(&premises, &goal, Duration::from_secs(2)).is_err());
}

/* ── resolution ───────────────────────────────────────────────────── */

const SOCRATES: &str = "\
system resolution
premises all x: man(x) -> mortal(x), man(socrates)
goal mortal(socrates)

1  not man(x) or mortal(x)   premise 1
2  man(socrates)             premise 2
3  not mortal(socrates)      goal
4  mortal(socrates)          resolve 1, 2
5  false                     resolve 3, 4
";

#[test]
fn clause_form_is_shown_step_by_step() {
    let (premises, goal) = argument("all x: some y: L(x, y) -> not (all z: R(z) <-> P)", "some x: P(x)");
    let forms = resolution::forms(&premises, Some(&goal)).unwrap();
    let labels: Vec<&str> = forms[0].steps.iter().map(|(l, _)| *l).collect();
    assert_eq!(labels, vec!["no →, ↔ or xor", "negations in", "prenex", "Skolem functions", "clauses"], "{:?}", forms[0].steps);
    let skolem = &forms[0].steps[3].1;
    assert!(skolem.contains("sk1(x)") && skolem.contains("sk2(x)"), "{skolem}");
    assert_eq!(forms[1].of, "the goal denied");
    assert_eq!(forms[1].steps.last().unwrap().1, "not P(x)");
    // Each existential its own Skolem function, numbered through the argument.
    let (premises, goal) = argument("some x: P(x), some x: Q(x)", "false");
    let forms = resolution::forms(&premises, Some(&goal)).unwrap();
    assert_eq!((forms[0].steps.last().unwrap().1.as_str(), forms[1].steps.last().unwrap().1.as_str()), ("P(sk1)", "Q(sk2)"));
}

#[test]
fn a_resolution_proof_is_checked_line_by_line() {
    let r = checked(SOCRATES);
    assert!(r.complete, "{r:?}");
    assert!(r.summary.starts_with("a refutation"), "{}", r.summary);
    // An instance of a clause, and one with more beside it, are clauses of it too.
    assert!(checked(&SOCRATES.replace("1  not man(x) or mortal(x)   premise 1", "1  not man(socrates) or mortal(socrates)   premise 1")).complete);
    // Not a clause of the premise named.
    let m = wrong(&SOCRATES.replace("premise 1", "premise 2"), 1);
    assert!(m.contains("not a clause of premise 2: its clauses are man(socrates)"), "{m}");
    // A resolvent that is not.
    let r = checked(&SOCRATES.replace("4  mortal(socrates)          resolve 1, 2", "4  mortal(plato)             resolve 1, 2"));
    assert!(r.lines[3].message.contains("resolve to mortal(socrates)"), "{:?}", r.lines[3]);
    let m = wrong(&SOCRATES.replace("resolve 3, 4", "resolve 2, 4"), 5);
    assert!(m.contains("no literals that clash"), "{m}");
    // A Skolem function by another name is not the clause form's.
    let sk = "\
system resolution
premises some x: P(x)
goal P(c)

1  P(c)      premise 1
2  not P(c)  goal
3  false     resolve 1, 2
";
    let m = wrong(sk, 1);
    assert!(m.contains("its clauses are P(sk1)"), "{m}");
    // Without the empty clause, not yet a refutation.
    let r = checked(&SOCRATES.replace("5  false                     resolve 3, 4\n", ""));
    assert!(!r.complete && r.summary.contains("not reached"), "{}", r.summary);
}

#[test]
fn refutations_are_found_for_valid_arguments() {
    for (ps, g) in VALID.iter().chain(&FIRST_ORDER) {
        let (premises, goal) = argument(ps, g);
        let text = resolution::find(&premises, &goal, Duration::from_secs(20)).unwrap_or_else(|e| panic!("{ps} |= {g}: {e}"));
        assert!(checked(&text).complete, "{ps} |= {g}:\n{text}");
    }
    let (premises, goal) = argument("all x: man(x) -> mortal(x), man(socrates)", "mortal(socrates)");
    let text = resolution::find(&premises, &goal, Duration::from_secs(5)).unwrap();
    assert!(text.contains("# Premise 1, all x: man(x) -> mortal(x):") && text.contains("not man(x) or mortal(x)"), "{text}");
}

#[test]
fn the_clauses_saturate_when_it_does_not_follow() {
    for (ps, g) in INVALID.iter().chain(&[("some x: P(x)", "all x: P(x)"), ("all x: some y: L(x, y)", "some y: all x: L(x, y)")]) {
        let (premises, goal) = argument(ps, g);
        let e = resolution::find(&premises, &goal, Duration::from_secs(5)).unwrap_err();
        assert!(e.contains("saturate"), "{ps} |= {g}: {e}");
    }
}
