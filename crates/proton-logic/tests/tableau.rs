//! L5: semantic tableaux, on arguments whose answers are known: the
//! textbook's propositional ones, the syllogisms, quantifier swaps, the
//! drinker paradox and some of Pelletier's problems.

use std::time::Duration;

use proton_logic::ast::{Item, Question};
use proton_logic::eval::broken;
use proton_logic::tableau::{prove, TabEnd, TabTree, Tableau};
use proton_logic::{check, parse};

fn ask(question: &str) -> Tableau {
    let c = check(&parse(&format!("? {question}\n")).unwrap()).unwrap_or_else(|e| panic!("{question}: {e:?}"));
    let Some(Item::Question(Question::Entails(premises, goal))) = c.theory.lines[0].item.clone() else { panic!("{question} is not an argument") };
    prove(&c, &premises, &goal, Duration::from_secs(20)).unwrap_or_else(|e| panic!("{question}: {e}"))
}

/// Every branch of a tree, closed or open.
fn ends(t: &TabTree) -> Vec<&TabEnd> {
    match &t.end {
        TabEnd::Split(a, b) => [ends(a), ends(b)].concat(),
        e => vec![e],
    }
}

#[test]
fn modus_ponens_closes_in_two_branches() {
    let t = ask("p, p -> q |= q");
    assert_eq!(t.valid, Some(true));
    let tree = t.tree.unwrap();
    let lines: Vec<String> = tree.lines.iter().map(|l| format!("{} {} ({})", l.n, l.text, l.why)).collect();
    assert_eq!(lines, vec!["1 p (premise)", "2 p -> q (premise)", "3 not q (conclusion denied)"]);
    let TabEnd::Split(left, right) = &tree.end else { panic!("{:?}", tree.end) };
    assert_eq!((left.lines[0].text.as_str(), left.lines[0].why.as_str(), &left.end), ("not p", "2, →", &TabEnd::Closed(1, 4)));
    assert_eq!((right.lines[0].text.as_str(), &right.end), ("q", &TabEnd::Closed(3, 5)));
}

#[test]
fn propositional_arguments_valid_and_not() {
    for q in ["p -> q, q -> r |= p -> r", "|= not (p and q) <-> not p or not q", "|= ((p -> q) -> p) -> p", "|= p or not p", "p and q |= q and p", "p <-> q, q |= p", "p xor q, p |= not q", "false |= q"] {
        let t = ask(q);
        assert_eq!(t.valid, Some(true), "{q}");
        assert!(ends(t.tree.as_ref().unwrap()).iter().all(|e| matches!(e, TabEnd::Closed(..))), "{q}");
    }
    // Affirming the consequent: q true, p false.
    let t = ask("q, p -> q |= p");
    assert_eq!(t.valid, Some(false));
    assert!(ends(t.tree.as_ref().unwrap()).iter().any(|e| matches!(e, TabEnd::Open)));
    let mut open = t.open.clone();
    open.sort();
    assert_eq!(open, vec![("p".to_string(), false), ("q".to_string(), true)]);
    for q in ["p -> q |= q -> p", "p or q |= p", "|= p and not p"] {
        assert_eq!(ask(q).valid, Some(false), "{q}");
    }
}

#[test]
fn first_order_arguments_are_proved() {
    for q in [
        "all x: P(x) -> Q(x), P(a) |= Q(a)",
        // Barbara: all P are Q, all Q are R, so all P are R.
        "all x: P(x) -> Q(x), all x: Q(x) -> R(x) |= all x: P(x) -> R(x)",
        // Some y every x loves: everyone loves someone.
        "some y: all x: L(x, y) |= all x: some y: L(x, y)",
        "not (all x: P(x)) |= some x: not P(x)",
        "|= (all x: P(x)) -> P(c)",
        // The drinker: someone such that, if they drink, everyone does.
        "|= some x: D(x) -> all y: D(y)",
        // Pelletier 20.
        "all x, y: some z: all w: (P(x) and Q(y) -> R(z) and S(w)) |= (some x, y: P(x) and Q(y)) -> some z: R(z)",
        // Pelletier 26.
        "(some x: P(x)) <-> (some x: Q(x)), all x, y: P(x) and Q(y) -> (R(x) <-> S(y)) |= (all x: P(x) -> R(x)) <-> (all x: Q(x) -> S(x))",
        // A function: f's value is P whenever its argument is.
        "all x: P(x) -> P(f(x)), P(a) |= P(f(f(a)))",
    ] {
        let t = ask(q);
        assert_eq!(t.valid, Some(true), "{q}: {}", t.note);
        assert!(ends(t.tree.as_ref().unwrap()).iter().all(|e| matches!(e, TabEnd::Closed(..))), "{q}");
    }
}

#[test]
fn first_order_arguments_that_fail_have_a_small_countermodel() {
    for q in [
        "some x: P(x) |= all x: P(x)",
        // Swapping the quantifiers the other way does not follow.
        "all x: some y: L(x, y) |= some y: all x: L(x, y)",
        "all x: P(x) or Q(x) |= (all x: P(x)) or (all x: Q(x))",
    ] {
        let t = ask(q);
        assert_eq!(t.valid, Some(false), "{q}");
        let (theory, model) = t.counter.expect("a countermodel");
        assert_eq!(broken(&theory, &model).unwrap(), vec![], "{q}: not a model of the premises and the conclusion denied");
        assert!(model.sizes["individual"] >= 2, "{q}: one thing would not do");
    }
}

#[test]
fn equality_is_a_predicate_here_so_some_proofs_are_not_found() {
    // a = b and P(a) give P(b), but only by equality's own rules: the prover
    // has none, and the model finder, which has, finds no countermodel.
    let t = ask("a = b, P(a) |= P(b)");
    assert_eq!(t.valid, None);
    assert!(t.note.contains("no countermodel"), "{}", t.note);
}
