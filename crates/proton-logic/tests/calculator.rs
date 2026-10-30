//! L1: the calculator, each answer against one worked by hand, and every
//! form it gives checked equivalent to its formula by truth table.

use proton_logic::ast::{Item, Question};
use proton_logic::calc::{self, Answer, Status, What};
use proton_logic::{check, parse};

fn ask(text: &str) -> Answer {
    let c = check(&parse(text).unwrap()).unwrap();
    let Some(Item::Question(q)) = &c.theory.lines[0].item else { panic!("{text} is not a question") };
    calc::ask(&c, q).unwrap()
}

fn formula(text: &str) -> (Answer, calc::FormulaAnswer) {
    let a = ask(text);
    let What::Formula(f) = a.what.clone() else { panic!() };
    (a, *f)
}

/// A formula's truth-table column, read from its text.
fn column(text: &str, atoms: &[String]) -> String {
    let t = check(&parse(&format!("? {text}")).unwrap()).unwrap();
    let Some(Item::Question(Question::Formula(e))) = &t.theory.lines[0].item else { panic!() };
    let (own, ps) = calc::propositional(&t.sig, &[e]).unwrap();
    // Its propositions in the order of the formula it came from.
    let n = atoms.len();
    (0..1u64 << n)
        .map(|r| {
            let values: Vec<bool> = own.iter().map(|a| r >> (n - 1 - atoms.iter().position(|b| b == a).unwrap()) & 1 == 1).collect();
            if ps[0].holds(&values) {
                '1'
            } else {
                '0'
            }
        })
        .collect()
}

#[test]
fn an_implication_its_table_properties_and_forms() {
    let (a, f) = formula("? p -> q");
    assert_eq!(a.atoms, vec!["p", "q"]);
    let table = a.table.unwrap();
    assert_eq!(table.last().unwrap().values, "1101");
    assert_eq!(f.status, Status::Contingent);
    assert_eq!(f.models, Some(3));
    assert_eq!(f.model, Some(vec![false, false]));
    assert_eq!(f.falsifier, Some(vec![true, false]));
    assert_eq!(f.forms.nnf.as_deref(), Some("not p or q"));
    assert_eq!(f.forms.cnf.as_deref(), Some("not p or q"));
    assert_eq!(f.forms.dnf.as_deref(), Some("not p or q"));
    assert_eq!(f.forms.full_dnf.as_deref(), Some("not p and not q or not p and q or p and q"));
    assert_eq!(f.forms.full_cnf.as_deref(), Some("not p or q"));
    assert_eq!(f.forms.least_sop.as_deref(), Some("not p or q"));
    assert_eq!(f.forms.least_pos.as_deref(), Some("not p or q"));
    assert!(f.forms.exact);
}

#[test]
fn tautologies_contradictions_and_a_table_of_steps() {
    let (_, f) = formula("? p or not p");
    assert_eq!((f.status, f.models, f.falsifier), (Status::Tautology, Some(2), None));
    let (_, f) = formula("? p and not p");
    assert_eq!((f.status, f.model), (Status::Contradiction, None));
    // Its CNF is itself; its least form says false.
    assert_eq!(f.forms.cnf.as_deref(), Some("p and not p"));
    assert_eq!(f.forms.least_sop.as_deref(), Some("false"));
    // Peirce's law, a column for each part.
    let (a, f) = formula("? ((p -> q) -> p) -> p");
    assert_eq!(f.status, Status::Tautology);
    let names: Vec<String> = a.table.unwrap().into_iter().map(|c| format!("{}={}", c.name, c.values)).collect();
    assert_eq!(names, vec!["p -> q=1101", "(p -> q) -> p=0011", "((p -> q) -> p) -> p=1111"]);
}

#[test]
fn arguments_valid_and_not() {
    let What::Argument { valid, countermodel } = ask("? p, p -> q |= q").what else { panic!() };
    assert!(valid && countermodel.is_none());
    // Affirming the consequent: q true and p false is the counterexample.
    let a = ask("? q, p -> q |= p");
    let What::Argument { valid, countermodel } = a.what else { panic!() };
    assert!(!valid);
    assert_eq!(a.atoms, vec!["q", "p"]);
    assert_eq!(countermodel, Some(vec![true, false]));
    assert_eq!(a.table.unwrap().len(), 3);
    // With nothing assumed, an argument holds when its conclusion always does.
    let What::Argument { valid, .. } = ask("? |= (p -> q) or (q -> p)").what else { panic!() };
    assert!(valid);
}

#[test]
fn equivalences_and_where_two_formulas_differ() {
    let What::Same { same, .. } = ask("? p -> q == not q -> not p").what else { panic!() };
    assert!(same);
    let What::Same { same, .. } = ask("? not (p and q) == not p or not q").what else { panic!() };
    assert!(same);
    let What::Same { same, differ } = ask("? p -> q == q -> p").what else { panic!() };
    assert!(!same);
    assert!(differ == Some(vec![false, true]) || differ == Some(vec![true, false]));
    // `=` between formulas is `<->`.
    let What::Same { same, .. } = ask("? (p = q) == (p <-> q)").what else { panic!() };
    assert!(same);
}

#[test]
fn a_column_read_back_as_formulas() {
    // Majority of three.
    let t = calc::from_column(&["a".into(), "b".into(), "c".into()], "00010111").unwrap();
    assert_eq!(t.forms.least_sop.as_deref(), Some("b and c or a and c or a and b"));
    assert_eq!(t.forms.least_pos.as_deref(), Some("(b or c) and (a or c) and (a or b)"));
    for f in [&t.forms.least_sop, &t.forms.least_pos, &t.forms.full_dnf, &t.forms.full_cnf] {
        assert_eq!(column(f.as_deref().unwrap(), &t.atoms), "00010111");
    }
    // Names not given are p, q, r; a don't care is taken either way.
    let t = calc::from_column(&[], "01x1").unwrap();
    assert_eq!(t.atoms, vec!["p", "q"]);
    assert_eq!(t.forms.least_sop.as_deref(), Some("q"));
    assert!(calc::from_column(&[], "011").unwrap_err().contains("power of two"));
    assert!(calc::from_column(&["a".into()], "0110").unwrap_err().contains("named"));
}

#[test]
fn past_twenty_propositions_the_search_answers() {
    // A chain of thirty implications, its start and its end denied.
    let chain = (1..30).map(|i| format!("(x{i} -> x{})", i + 1)).collect::<Vec<_>>().join(" and ");
    let (a, f) = formula(&format!("? x1 and {chain} and not x30"));
    assert_eq!(a.atoms.len(), 30);
    assert!(a.table.is_none());
    assert_eq!((f.status, f.models), (Status::Contradiction, None));
    // Without the denial, a model: every x true.
    let (_, f) = formula(&format!("? x1 and {chain}"));
    assert_eq!(f.status, Status::Contingent);
    assert!(f.model.unwrap().iter().all(|v| *v));
    // An argument over 26 propositions.
    let letters: Vec<String> = (b'a'..=b'z').map(|c| (c as char).to_string()).collect();
    let premises = letters.windows(2).map(|w| format!("{} -> {}", w[0], w[1])).collect::<Vec<_>>().join(", ");
    let What::Argument { valid, .. } = ask(&format!("? {premises} |= a -> z")).what else { panic!() };
    assert!(valid);
    let What::Argument { valid, countermodel } = ask(&format!("? {premises} |= z -> a")).what else { panic!() };
    assert!(!valid);
    let cm = countermodel.unwrap();
    assert!(cm[25] && !cm[0], "z true and a false: {cm:?}");
}

#[test]
fn nand_and_nor_forms_compute_the_formula() {
    for text in ["p -> q", "p xor q xor r", "(a and b) or (c and not d)", "p", "p or not p", "p and not p", "(p <-> q) and r"] {
        let (a, f) = formula(&format!("? {text}"));
        let c = check(&parse(&format!("? {text}")).unwrap()).unwrap();
        let Some(Item::Question(Question::Formula(e))) = &c.theory.lines[0].item else { panic!() };
        let (atoms, ps) = calc::propositional(&c.sig, &[e]).unwrap();
        let n = atoms.len();
        let column: Vec<u8> = (0..1u64 << n).map(|r| if ps[0].eval(n, r) { b'1' } else { b'0' }).collect();
        let m = proton_circuit::analysis::minimise(n, &column).unwrap();
        for nor in [false, true] {
            let g = calc::gates(&atoms, if nor { &m.pos } else { &m.sop }, nor);
            for r in 0..1u64 << n {
                let values: Vec<bool> = (0..n).map(|i| r >> (n - 1 - i) & 1 == 1).collect();
                assert_eq!(g.eval(&values), ps[0].holds(&values), "{text}, {}, row {r}", g.text(&atoms));
            }
        }
        assert_eq!(f.forms.nand, Some(calc::gates(&atoms, &m.sop, false).text(&atoms)));
        let _ = a;
    }
    let (_, f) = formula("? p and q");
    assert_eq!(f.forms.nand.as_deref(), Some("nand(nand(p, q))"));
    assert_eq!(f.forms.nor.as_deref(), Some("nor(nor(q), nor(p))"));
}

/// Every form of a formula, read back, has the formula's truth table.
#[test]
fn every_form_is_the_formula() {
    let formulas = [
        "p -> q",
        "(p -> q) and (q -> r) -> (p -> r)",
        "p xor q xor r xor s",
        "not (a <-> b) or (c and not a)",
        "(a or b) and (a or c) and (b or not c)",
        "((p and q) or (r and s)) -> not (p xor s)",
        "p <-> (q <-> (r <-> s))",
        "a and b and c and d and e or not a and not e",
        "true -> p",
        "(x1 or x2) and (x3 or x4) and (x5 or x6) and (x7 or x8)",
    ];
    for text in formulas {
        let (a, f) = formula(&format!("? {text}"));
        let want = column(text, &a.atoms);
        let forms = [
            ("nnf", &f.forms.nnf),
            ("cnf", &f.forms.cnf),
            ("dnf", &f.forms.dnf),
            ("full cnf", &f.forms.full_cnf),
            ("full dnf", &f.forms.full_dnf),
            ("least sop", &f.forms.least_sop),
            ("least pos", &f.forms.least_pos),
        ];
        for (name, form) in forms {
            let Some(form) = form else { continue };
            // A form may not use every proposition: what it leaves out it does not depend on.
            assert_eq!(column(form, &a.atoms), want, "{text}: its {name}, {form}");
        }
        assert!(f.forms.cnf.is_some() && f.forms.least_sop.is_some(), "{text}: {:?}", f.forms);
    }
}

#[test]
fn formulas_beyond_propositions_go_to_the_solver() {
    let c = check(&parse("s = a | b\nf: s -> s\n? all x: f(x) = x\n").unwrap()).unwrap();
    let Some(Item::Question(q)) = &c.theory.lines[2].item else { panic!() };
    assert!(calc::ask(&c, q).unwrap_err().contains("the solver answers it"));
}

/// Forms that would be too long to read are left out, not made: parity's
/// negation normal form doubles with each proposition, and its least sum
/// has a term for every other row.
#[test]
fn forms_too_long_to_read_are_left_out() {
    let parity = (1..=16).map(|i| format!("x{i}")).collect::<Vec<_>>().join(" xor ");
    let (a, f) = formula(&format!("? {parity}"));
    assert_eq!(f.models, Some(32768));
    assert!(f.forms.nnf.is_none() && f.forms.cnf.is_none() && f.forms.least_sop.is_none() && f.forms.full_dnf.is_none());
    assert_eq!(a.table.unwrap()[0].values.len(), 65536);
    // At ten, its least forms are found, and too long to give.
    let parity = (1..=10).map(|i| format!("x{i}")).collect::<Vec<_>>().join(" xor ");
    let (_, f) = formula(&format!("? {parity}"));
    assert!(f.forms.least_sop.is_none() && f.forms.nand.is_none());
    let (_, f) = formula("? (a and b) or (c and d) or (e and f) or (g and h) or (i and j)");
    assert_eq!(f.forms.least_sop.as_deref(), Some("i and j or g and h or e and f or c and d or a and b"));
}
