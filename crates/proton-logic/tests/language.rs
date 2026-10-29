//! L0: the language read, typed, printed back the same; Logic4Fun's read
//! and written; formulas worked out in models. The puzzles are written
//! here: none is the site's.

use proton_logic::ast::*;
use proton_logic::eval::{broken, holds, Model};
use proton_logic::{check, l4f, parse, print};

/// Three friends, each with a different pet on a different floor. The cat's
/// owner lives on floor 1, Ben above them; Cy keeps the eel; Ana is not on
/// floor 3; Cy lives below Ben. One answer: Ana, cat, 1; Cy, eel, 2; Ben,
/// dog, 3.
const FRIENDS: &str = "\
# Three friends, three pets, three floors
friend = Ana | Ben | Cy
pet = cat | dog | eel
floor = 1..3

keeps: friend -> pet  one-to-one
home: friend -> floor  one-to-one

all f: keeps(f) = cat -> home(f) = 1
all f: keeps(f) = cat -> home(Ben) > home(f)  # Ben lives above the cat
keeps(Cy) = eel
home(Ana) != 3
home(Cy) < home(Ben)
";

/// The same puzzle as Logic4Fun would have it: floors from 0, the first
/// clue in clause form.
const FRIENDS_L4F: &str = "\
Sorts:
friend enum: Ana, Ben, Cy.
pet enum: cat, dog, eel.
floor cardinality = 3.

Vocabulary:
function {
  keeps(friend) : pet {all_different}
  home(friend) : floor {all_different}
}

Constraints:
% clause form: x is every friend
keeps(x) = cat -> home(x) = 0.
ALL f (keeps(f) = cat IMP home(Ben) > home(f)).
keeps(Cy) = eel.
home(Ana) != 2.
home(Cy) < home(Ben).
";

/// A model of the friends: who keeps what, and where. `floor` is the floor
/// as the sort numbers it.
fn friends(pets: [i64; 3], floors: [i64; 3]) -> Model {
    let mut m = Model::default();
    for f in 0..3 {
        m.set("keeps", vec![f], pets[f as usize]);
        m.set("home", vec![f], floors[f as usize]);
    }
    m
}

/// A theory with its read-at line numbers wiped, to compare what it says.
fn said(mut t: Theory) -> Theory {
    for l in &mut t.lines {
        l.at = 0;
    }
    t
}

#[test]
fn a_theory_prints_back_as_written_and_reads_back_the_same() {
    let t = parse(FRIENDS).unwrap();
    assert_eq!(print::print(&t), FRIENDS);
    assert_eq!(said(parse(&print::print(&t)).unwrap()), said(t.clone()));
    assert_eq!(t.lines.len(), 13);
    assert!(matches!(&t.lines[1].item, Some(Item::Sort(SortDecl { name, kind: SortKind::Named(n) })) if name == "friend" && n.len() == 3));
    assert!(matches!(&t.lines[3].item, Some(Item::Sort(SortDecl { kind: SortKind::Range(1, 3), .. }))));
    assert!(matches!(&t.lines[5].item, Some(Item::Symbol(SymbolDecl { props: Props { one_to_one: true, .. }, .. }))));
    assert_eq!(t.lines[9].comment.as_deref(), Some("Ben lives above the cat"));
    assert_eq!(t.lines[9].at, 10);
}

#[test]
fn a_variables_sort_is_found_from_what_it_is_given_to() {
    let c = check(&parse(FRIENDS).unwrap()).unwrap();
    let Some(Item::Constraint(Expr::Quant(Quant::All, binders, _))) = &c.theory.lines[8].item else { panic!() };
    assert_eq!(binders[0].sort.as_deref(), Some("friend"));
    assert!(c.sig.propositions.is_empty());
    assert_eq!(c.sig.elements["eel"], ("pet".to_string(), 2));
}

#[test]
fn the_answer_is_a_model_and_a_wrong_one_breaks_the_lines_it_should() {
    let c = check(&parse(FRIENDS).unwrap()).unwrap();
    // Ana cat 1, Ben dog 3, Cy eel 2.
    assert_eq!(broken(&c, &friends([0, 1, 2], [1, 3, 2])).unwrap(), vec![]);
    // Ben with the cat: he is on floor 3, not 1, and not above himself.
    let wrong = broken(&c, &friends([1, 0, 2], [1, 3, 2])).unwrap();
    assert_eq!(wrong.iter().map(|(l, _)| *l).collect::<Vec<_>>(), vec![9, 10]);
    // Two on one floor is not one-to-one, and said so on the declaration.
    let crowded = broken(&c, &friends([0, 1, 2], [1, 3, 3])).unwrap();
    assert!(crowded.iter().any(|(l, why)| *l == 7 && why.contains("not one-to-one")), "{crowded:?}");
    // A model with a hole in it is not worked out at all.
    let mut holed = friends([0, 1, 2], [1, 3, 2]);
    holed.values.get_mut("home").unwrap().remove(&vec![2]);
    assert!(broken(&c, &holed).is_err());
}

#[test]
fn logic4funs_text_reads_as_the_same_puzzle() {
    let (t, notes) = l4f::read(FRIENDS_L4F).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        print::print(&t),
        "friend = Ana < Ben < Cy\npet = cat < dog < eel\nfloor = 0..2\nkeeps: friend -> pet  one-to-one\nhome: friend -> floor  one-to-one\n\
         # clause form: x is every friend\nall x: keeps(x) = cat -> home(x) = 0\nall f: keeps(f) = cat -> home(Ben) > home(f)\n\
         keeps(Cy) = eel\nhome(Ana) != 2\nhome(Cy) < home(Ben)\n"
    );
    let c = check(&t).unwrap();
    // Its floors from 0: the same answer, a floor down.
    assert_eq!(broken(&c, &friends([0, 1, 2], [0, 2, 1])).unwrap(), vec![]);
    assert_eq!(broken(&c, &friends([1, 0, 2], [0, 2, 1])).unwrap().len(), 2);
}

#[test]
fn a_theory_written_for_logic4fun_reads_back_with_the_same_models() {
    let c = check(&parse(FRIENDS).unwrap()).unwrap();
    let (text, notes) = l4f::write(&c).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    // Floors 1..3 are written from 0, the numbers said of them with them.
    assert!(text.contains("floor cardinality = 3."), "{text}");
    assert!(text.contains("function keeps(friend) : pet {all_different}"), "{text}");
    assert!(text.contains("ALL f ((keeps(f) = cat) IMP (home(f) = 0))."), "{text}");
    assert!(text.contains("home(Ana) != 2."), "{text}");
    assert!(text.contains("% Ben lives above the cat"), "{text}");
    let (back, _) = l4f::read(&text).unwrap();
    let back = check(&back).unwrap();
    assert_eq!(broken(&back, &friends([0, 1, 2], [0, 2, 1])).unwrap(), vec![]);
    assert_eq!(broken(&back, &friends([1, 0, 2], [0, 2, 1])).unwrap().len(), 2);
}

const QUEENS: &str = "\
file = 1..8
rank = 1..8
queen: file -> rank  one-to-one
all a, b: a != b -> abs(queen(a) - queen(b)) != abs(a - b)
";

fn queens(ranks: [i64; 8]) -> Model {
    let mut m = Model::default();
    for (f, r) in ranks.iter().enumerate() {
        m.set("queen", vec![f as i64 + 1], *r);
    }
    m
}

#[test]
fn eight_queens_in_both_languages() {
    let c = check(&parse(QUEENS).unwrap()).unwrap();
    assert_eq!(broken(&c, &queens([1, 5, 8, 6, 3, 7, 2, 4])).unwrap(), vec![]);
    // Two on one diagonal: files 1 and 2, ranks 1 and 2.
    assert_eq!(broken(&c, &queens([1, 2, 8, 6, 3, 7, 5, 4])).unwrap().len(), 1);
    let (text, _) = l4f::write(&c).unwrap();
    assert!(text.contains("ALL a ALL b ((a != b) IMP (DIF(queen(a), queen(b)) != DIF(a, b)))."), "{text}");
    let (back, _) = l4f::read(&text).unwrap();
    let back = check(&back).unwrap();
    let down = |r: [i64; 8]| {
        let mut m = Model::default();
        for (f, r) in r.iter().enumerate() {
            m.set("queen", vec![f as i64], r - 1);
        }
        m
    };
    assert_eq!(broken(&back, &down([1, 5, 8, 6, 3, 7, 2, 4])).unwrap(), vec![]);
    assert_eq!(broken(&back, &down([1, 2, 8, 6, 3, 7, 5, 4])).unwrap().len(), 1);
}

#[test]
fn calculator_questions_need_no_declarations_and_read_any_notation() {
    let t = parse("? p, p -> q |= q\n? p → q ≡ ¬q → ¬p\n? (p && q) || !r\n? |= p | ~p\n? a ∧ b ⊨ a\n").unwrap();
    assert_eq!(print::print(&t), "? p, p -> q |= q\n? p -> q == not q -> not p\n? p and q or not r\n? |= p or not p\n? a and b |= a\n");
    let c = check(&t).unwrap();
    assert_eq!(c.sig.propositions, vec!["p", "q", "r", "a", "b"]);
}

#[test]
fn connectives_bind_as_the_plan_says_and_print_with_only_the_brackets_needed() {
    let one = |s: &str| match parse(s).unwrap().lines.remove(0).item {
        Some(Item::Constraint(e)) => e,
        other => panic!("{other:?}"),
    };
    let p = Expr::name;
    let b = |op, x, y| Expr::bin(op, x, y);
    // not, then and, or, ->, <->.
    assert_eq!(one("not p and q or r -> s <-> t"), b(Op::Iff, b(Op::Imp, b(Op::Or, b(Op::And, Expr::negation(p("p")), p("q")), p("r")), p("s")), p("t")));
    // -> to the right.
    assert_eq!(one("a -> b -> c"), b(Op::Imp, p("a"), b(Op::Imp, p("b"), p("c"))));
    // not takes a comparison whole.
    assert_eq!(one("not x = y"), Expr::negation(b(Op::Eq, p("x"), p("y"))));
    for s in ["(a -> b) -> c", "not (p and q)", "a - (b - c)", "a - b - c", "p and (all x: q(x))", "not (all x: q(x))", "-(-x) = 3", "a <-> b <-> c", "a <-> (b <-> c)", "(p or q) and r"] {
        let e = one(s);
        assert_eq!(print::expr(&e), s, "{s} printed differently");
    }
    assert_eq!(print::expr(&one("((p))  and ((q or r))")), "p and (q or r)");
    // A statement goes on past a line that ends in an operator, or with a bracket open.
    let t = parse("all x:\n  p(x) and\n  q(x)\n(a or\n b)\n").unwrap();
    assert_eq!(t.lines.len(), 2);
}

#[test]
fn mistakes_are_named_on_their_lines() {
    let errs = |s: &str| check(&parse(s).unwrap()).unwrap_err().into_iter().map(|d| (d.line, d.message)).collect::<Vec<_>>();
    let has = |es: &[(usize, String)], line: usize, bit: &str| es.iter().any(|(l, m)| *l == line && m.contains(bit));
    let e = errs("colour = red | green\nsize = 1..3\nc: colour\nn: size\nc < red\nc = n\nf(c)\nall x: x = x\n");
    assert!(has(&e, 5, "not in order"), "{e:?}");
    assert!(has(&e, 6, "cannot be compared"), "{e:?}");
    assert!(has(&e, 7, "f is not declared"), "{e:?}");
    assert!(has(&e, 8, "cannot tell what x ranges over"), "{e:?}");
    let e = errs("s = a | b\nt = b | c\n");
    assert!(has(&e, 2, "b is declared already"), "{e:?}");
    let e = errs("s = a | b\ng: s -> s\ng(a, b) = a\nall x in s: g(x)\n");
    assert!(has(&e, 3, "g takes 1 argument"), "{e:?}");
    assert!(has(&e, 4, "needs true or false"), "{e:?}");
    // Parse errors, with the place.
    let d = parse("p and and q").unwrap_err();
    assert_eq!((d.line, d.col), (1, 7));
    assert!(parse("a = b = c").unwrap_err().message.contains("do not chain"));
    assert!(parse("s = a |").is_err());
}

#[test]
fn counting_and_partial_functions_are_worked_out_and_kept_in_logic4fun() {
    let t = parse("friend = Ana | Ben | Cy\nlikes: friend -> bool\nboss: friend -> friend  partial\nexactly 2 f: likes(f)\nat most 1 f: defined(boss(f))\nall f: defined(boss(f)) -> boss(f) != f\n").unwrap();
    let c = check(&t).unwrap();
    let mut m = Model::default();
    for (f, l) in [(0, 1), (1, 1), (2, 0)] {
        m.set("likes", vec![f], l);
    }
    m.set("boss", vec![2], 0);
    assert_eq!(broken(&c, &m).unwrap(), vec![]);
    m.set("likes", vec![2], 1);
    m.set("boss", vec![0], 0);
    let b = broken(&c, &m).unwrap().into_iter().map(|(l, _)| l).collect::<Vec<_>>();
    assert_eq!(b, vec![4, 5, 6]);
    // Logic4Fun has only ALL and SOME: the counts are written out, and mean the same.
    let (text, _) = l4f::write(&c).unwrap();
    assert!(text.contains("SOME f1 SOME f2"), "{text}");
    assert!(text.contains("EST(boss(f))"), "{text}");
    let back = check(&l4f::read(&text).unwrap().0).unwrap();
    assert_eq!(broken(&back, &m).unwrap().len(), 3);
    m.set("likes", vec![2], 0);
    m.values.get_mut("boss").unwrap().remove(&vec![0]);
    assert_eq!(broken(&back, &m).unwrap(), vec![]);
}

#[test]
fn questions_formulas_are_worked_out_under_a_valuation() {
    let c = check(&parse("? p -> q\n").unwrap()).unwrap();
    let Some(Item::Question(Question::Formula(e))) = &c.theory.lines[0].item else { panic!() };
    for (p, q, want) in [(0, 0, true), (0, 1, true), (1, 0, false), (1, 1, true)] {
        let mut m = Model::default();
        m.set("p", vec![], p);
        m.set("q", vec![], q);
        assert_eq!(holds(&c.sig, &m, e).unwrap(), want);
    }
}

#[test]
fn logic4fun_settings_proton_cannot_keep_are_named() {
    let (t, notes) = l4f::read("Sorts:\ns enum: a, b.\nt {cardinality >= 2 cardinality <= 4}\nVocabulary:\nfunction f(s) : t {all_different print: brief}\nConstraints:\nf(a) != f(b).\n").unwrap();
    assert!(notes.iter().any(|n| n.contains("print")), "{notes:?}");
    assert_eq!(print::print(&t), "s = a < b\nt = size 2..4\nf: s -> t  one-to-one\nf(a) != f(b)\n");
    assert!(check(&t).is_ok());
}

/// Formulas made at random, printed and read back: the same formula, so
/// the printer's brackets and the parser's precedences agree everywhere.
#[test]
fn random_formulas_print_and_read_back_the_same() {
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }
    const OPS: [Op; 14] = [Op::Iff, Op::Imp, Op::Or, Op::Xor, Op::And, Op::Eq, Op::Ne, Op::Lt, Op::Le, Op::Gt, Op::Ge, Op::Add, Op::Sub, Op::Mul];
    fn make(r: &mut Rng, depth: u32) -> Expr {
        if depth == 0 {
            return match r.below(4) {
                0 => Expr::Num(r.below(10) as i64),
                1 => Expr::Bool(r.below(2) == 1),
                2 => Expr::End { last: r.below(2) == 1, sort: "s".into() },
                _ => Expr::name(["p", "q", "x", "y"][r.below(4) as usize]),
            };
        }
        match r.below(7) {
            0 => Expr::negation(make(r, depth - 1)),
            1 => Expr::Neg(Box::new(make(r, depth - 1))),
            2 => Expr::App("f".into(), (0..1 + r.below(2)).map(|_| make(r, depth - 1)).collect()),
            3 => {
                let q = [Quant::All, Quant::Some, Quant::Exactly(2), Quant::AtLeast(1), Quant::AtMost(3)][r.below(5) as usize];
                let n = if matches!(q, Quant::All | Quant::Some) { 1 + r.below(2) } else { 1 };
                let binders = (0..n).map(|k| Binder { name: format!("v{k}"), sort: if r.below(2) == 0 { Some("s".into()) } else { None } }).collect();
                Expr::Quant(q, binders, Box::new(make(r, depth - 1)))
            }
            _ => Expr::bin(OPS[r.below(14) as usize], make(r, depth - 1), make(r, depth - 1)),
        }
    }
    let mut r = Rng(0x5eed_1234_abcd);
    let mut tried = 0;
    for _ in 0..5000 {
        let depth = 1 + r.below(4) as u32;
        let e = make(&mut r, depth);
        let text = proton_logic::print::expr(&e);
        // Comparisons do not chain, so a formula printing `a = b = c` cannot be
        // written; those are skipped, the rest must read back exactly.
        let Ok(t) = parse(&text) else { continue };
        tried += 1;
        match &t.lines[0].item {
            Some(Item::Constraint(back)) => assert_eq!(back, &e, "{text}"),
            other => panic!("{text} read as {other:?}"),
        }
    }
    assert!(tried > 3000, "only {tried} of 5000 could be written");
}
