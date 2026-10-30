//! L6: natural deduction — proofs written here, right and wrong, each
//! mistake named on its line; and proofs found, then checked as any is.

use std::time::Duration;

use proton_logic::fitch::{check, find, read, Checked};
use proton_logic::parse::{formula, formulas};

fn checked(text: &str) -> Checked {
    let f = read(text);
    assert!(f.unread.is_empty(), "{:?}", f.unread);
    check(&f)
}

/// The mistake on proof line `n`, which must be the only one.
fn wrong(text: &str, n: usize) -> String {
    let c = checked(text);
    let bad: Vec<_> = c.lines.iter().filter(|l| !l.ok).collect();
    assert_eq!(bad.iter().map(|l| l.n).collect::<Vec<_>>(), vec![n], "{:?}", c.lines);
    assert!(!c.complete);
    bad[0].message.clone()
}

const SYLLOGISM: &str = "\
system fitch
premises p -> q, q -> r
goal p -> r

1  p -> q        premise
2  q -> r        premise
3  | p           assume
4  | q           →E 1, 3
5  | r           →E 2, 4
6  p -> r        →I 3-5
";

#[test]
fn a_right_proof_is_complete() {
    let c = checked(SYLLOGISM);
    assert!(c.complete, "{c:?}");
    assert!(c.lines.iter().all(|l| l.ok));
    assert!(c.summary.starts_with("a proof of p -> r from p -> q, q -> r"), "{}", c.summary);
    // Without its last line: every line right, the goal not reached.
    let short: String = SYLLOGISM.lines().take(9).collect::<Vec<_>>().join("\n");
    let c = checked(&short);
    assert!(!c.complete && c.summary.contains("not reached"), "{}", c.summary);
}

#[test]
fn each_mistake_is_named_on_its_line() {
    // →E with the wrong antecedent.
    let m = wrong(&SYLLOGISM.replace("→E 2, 4", "→E 2, 3"), 5);
    assert!(m.contains("→E needs"), "{m}");
    // A line of a subproof that has ended.
    let m = wrong(&format!("{SYLLOGISM}7  q             R 4\n"), 7);
    assert!(m.contains("inside a subproof that has ended"), "{m}");
    // →I citing a range that is not where the subproof ends.
    let m = wrong(&SYLLOGISM.replace("→I 3-5", "→I 3-4"), 6);
    assert!(m.contains("does not end at line 4"), "{m}");
    // Two bars with one subproof open.
    let m = wrong(&SYLLOGISM.replace("5  | r ", "5  | | r "), 5);
    assert!(m.contains("needs an assumption"), "{m}");
    // A premise the header does not name.
    let m = wrong(&format!("{SYLLOGISM}7  s             premise\n"), 7);
    assert!(m.contains("not one of the premises"), "{m}");
    // The same number twice; and so a line citing the one it took the place of.
    let c = checked(&SYLLOGISM.replace("5  | r ", "4  | r "));
    let bad: Vec<_> = c.lines.iter().filter(|l| !l.ok).map(|l| (l.n, l.message.as_str())).collect();
    assert_eq!(bad, vec![(4, "there is a line 4 already"), (6, "there is no line 5")]);
    // A line cited that is not there yet.
    let m = wrong(&SYLLOGISM.replace("→E 1, 3", "→E 1, 5"), 4);
    assert!(m.contains("not before"), "{m}");
}

#[test]
fn negation_rules_and_indirect_proof() {
    let text = "\
premises p -> q, not q
goal not p
1  p -> q     premise
2  not q      premise
3  | p        assume
4  | q        →E 1, 3
5  | false    ¬E 4, 2
6  not p      ¬I 3-5
";
    assert!(checked(text).complete);
    // IP gives what was denied, not its negation.
    let m = wrong(&text.replace("¬I 3-5", "IP 3-5"), 6);
    assert!(m.contains("gives what 3 denies"), "{m}");
    // RAA reads either way.
    assert!(checked(&text.replace("¬I 3-5", "RAA 3-5")).complete);
    // A subproof not ending in ⊥.
    let m = wrong(&text.replace("5  | false    ¬E 4, 2\n6  not p      ¬I 3-5", "5  not p      ¬I 3-4"), 5);
    assert!(m.contains("must end with ⊥"), "{m}");
    // Excluded middle, by IP and De Morgan-free means.
    let lem = "\
goal p or not p
1  | not (p or not p)    assume
2  | | p                 assume
3  | | p or not p        ∨I 2
4  | | false             ¬E 3, 1
5  | not p             ¬I 2-4
6  | p or not p        ∨I 5
7  | false             ¬E 6, 1
8  p or not p          IP 1-7
";
    assert!(checked(lem).complete, "{:?}", checked(lem));
}

#[test]
fn other_courses_rule_names_are_read() {
    let text = "\
premises p and q, p -> r
goal r and q
1  p and q      premise
2  p -> r       premise
3  p            Simp 1
4  q            &E 1
5  r            MP 2, 3
6  r and q      Conj 5, 4
";
    assert!(checked(text).complete, "{:?}", checked(text));
}

#[test]
fn quantifier_rules_and_their_conditions() {
    let text = "\
premises all x: man(x) -> mortal(x), man(socrates)
goal mortal(socrates)
1  all x: man(x) -> mortal(x)          premise
2  man(socrates)                       premise
3  man(socrates) -> mortal(socrates)   ∀E 1
4  mortal(socrates)                    →E 3, 2
";
    assert!(checked(text).complete);
    let ex = "\
premises some x: P(x), all x: P(x) -> Q(x)
goal some x: Q(x)
1  some x: P(x)              premise
2  all x: P(x) -> Q(x)       premise
3  | P(c)                    assume
4  | P(c) -> Q(c)            ∀E 2
5  | Q(c)                    →E 4, 3
6  | some x: Q(x)            ∃I 5
7  some x: Q(x)              ∃E 1, 3-6
";
    assert!(checked(ex).complete, "{:?}", checked(ex));
    // ∃E needs a new name: c is in the conclusion.
    let bad = "\
premises some x: P(x)
goal P(c)
1  some x: P(x)    premise
2  | P(c)          assume
3  | P(c)          R 2
4  P(c)            ∃E 1, 2-3
";
    let m = wrong(bad, 4);
    assert!(m.contains("must be new"), "{m}");
    // ∀I of a name something assumed is about.
    let bad = "\
premises P(a)
goal all x: P(x)
1  P(a)          premise
2  all x: P(x)   ∀I 1
";
    let m = wrong(bad, 2);
    assert!(m.contains("still assumed"), "{m}");
    // ∀I of an arbitrary name.
    let good = "\
premises all x: P(x) and Q(x)
goal all x: P(x)
1  all x: P(x) and Q(x)   premise
2  P(c) and Q(c)          ∀E 1
3  P(c)                   ∧E 2
4  all x: P(x)            ∀I 3
";
    assert!(checked(good).complete, "{:?}", checked(good));
    // =E: equals for equals.
    let eq = "\
premises a = b, P(a)
goal P(b)
1  a = b     premise
2  P(a)      premise
3  P(b)      =E 1, 2
";
    assert!(checked(eq).complete);
}

fn argument(premises: &str, goal: &str) -> (Vec<proton_logic::Expr>, proton_logic::Expr) {
    (formulas(premises).unwrap(), formula(goal).unwrap())
}

#[test]
fn proofs_are_found_for_valid_arguments() {
    let cases = [
        ("p, p -> q", "q"),
        ("p -> q, q -> r", "p -> r"),
        ("p -> q", "not q -> not p"),
        ("not (p or q)", "not p and not q"),
        ("not p and not q", "not (p or q)"),
        ("not (p and q)", "not p or not q"),
        ("", "p or not p"),
        ("", "((p -> q) -> p) -> p"),
        ("p or q, not p", "q"),
        ("not not p", "p"),
        ("p and (q or r)", "(p and q) or (p and r)"),
        ("(p and q) -> r", "p -> (q -> r)"),
        ("p <-> q, q", "p"),
        ("p -> q, p -> not q", "not p"),
        ("all x: P(x) -> Q(x), all x: Q(x) -> R(x)", "all x: P(x) -> R(x)"),
        ("some x: P(x), all x: P(x) -> Q(x)", "some x: Q(x)"),
        ("all x: not P(x)", "not (some x: P(x))"),
        ("some y: all x: L(x, y)", "all x: some y: L(x, y)"),
        ("all x: P(x) -> Q(x), P(a)", "Q(a)"),
    ];
    for (ps, g) in cases {
        let (premises, goal) = argument(ps, g);
        let text = find(&premises, &goal, Duration::from_secs(10)).unwrap_or_else(|e| panic!("{ps} |= {g}: {e}"));
        // find checks what it finds; here, once more, as the file would be.
        assert!(checked(&text).complete, "{ps} |= {g}:\n{text}");
    }
}

#[test]
fn no_proof_is_found_for_what_does_not_follow() {
    for (ps, g) in [("q, p -> q", "p"), ("p or q", "p"), ("some x: P(x)", "all x: P(x)")] {
        let (premises, goal) = argument(ps, g);
        assert!(find(&premises, &goal, Duration::from_secs(2)).is_err(), "{ps} |= {g}");
    }
}

#[test]
fn a_found_proof_reads_as_a_proof_file() {
    let (premises, goal) = argument("p -> q, q -> r", "p -> r");
    let text = find(&premises, &goal, Duration::from_secs(5)).unwrap();
    assert!(text.starts_with("system fitch\npremises p -> q, q -> r\ngoal p -> r\n\n"), "{text}");
    assert!(text.contains("| p ") && text.contains("→I 3-5"), "{text}");
}

#[test]
fn found_proofs_are_short() {
    // Searched shallow first: no nest of assumptions that go nowhere.
    for (ps, g, most) in [("not (p and q)", "not p or not q", 14), ("not (all x: P(x))", "some x: not P(x)", 10), ("", "((p -> q) -> p) -> p", 12)] {
        let (premises, goal) = argument(ps, g);
        let text = find(&premises, &goal, Duration::from_secs(10)).unwrap();
        let lines = checked(&text).lines.len();
        assert!(lines <= most, "{ps} |= {g}: {lines} lines\n{text}");
    }
}
