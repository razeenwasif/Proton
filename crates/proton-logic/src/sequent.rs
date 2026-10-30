//! Gentzen's sequent calculus, LK: a `.proof` file of sequents checked line
//! by line, and proofs found.
//!
//! ```text
//! system sequent
//! premises p -> q, p
//! goal q
//!
//! 1  p ⊢ p, q          Ax
//! 2  q, p ⊢ q          Ax
//! 3  p -> q, p ⊢ q     →L 1, 2
//! ```
//!
//! - **A line** is a sequent — formulas, `⊢` (or `|-`), formulas — its rule,
//!   and the lines above it in the tree: its premises. The proof's last line
//!   is the argument: some of the premises `⊢` the goal.
//! - **Sides are sets:** exchange and contraction are free, and a line above
//!   may leave out what the line below has — weakening as it goes — so LK's
//!   rules and G3's, which keep the principal formula or drop it, both check.
//!   `WL`, `WR`, `CL` and `CR` are read, and `Cut`.
//! - **Rules** are named by connective and side — `∧L`, `→R`, `∀L` — and the
//!   connective may be written `&`, `->`, `~`, `and`, `all` and so on.
//! - **Found** by working up from the argument: rules that lose nothing
//!   first, the ones that split after, then a universal on the left or an
//!   existential on the right given a term, as few as will do — so always for
//!   propositional logic, and within a bound for first-order.

use std::collections::{BTreeSet, HashSet};
use std::time::{Duration, Instant};

use crate::ast::*;
use crate::fitch::{instance, is_false, open_quant, same, subst};
use crate::parse::{self, Diag};
use crate::print;
use crate::proofs::{self, Cite, Header, LineReport, Report};

/// How many terms the finder gives universals on the left and existentials
/// on the right, at most, on one branch.
pub const MOST_INSTANCES: u32 = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sequent {
    pub left: Vec<Expr>,
    pub right: Vec<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Ax,
    AndL,
    AndR,
    OrL,
    OrR,
    ImpL,
    ImpR,
    NotL,
    NotR,
    IffL,
    IffR,
    XorL,
    XorR,
    AllL,
    AllR,
    ExL,
    ExR,
    WL,
    WR,
    CL,
    CR,
    Cut,
}

/// Each connective's symbol, its other spellings, and its rules on the left and right.
const CONNECTIVES: [(&str, &[&str], Rule, Rule); 8] = [
    ("∧", &["&", "^", "and", "conj"], Rule::AndL, Rule::AndR),
    ("∨", &["v", "or", "disj"], Rule::OrL, Rule::OrR),
    ("→", &["->", "⊃", "imp", "impl", "if"], Rule::ImpL, Rule::ImpR),
    ("¬", &["~", "not", "neg"], Rule::NotL, Rule::NotR),
    ("↔", &["<->", "≡", "iff"], Rule::IffL, Rule::IffR),
    ("⊕", &["xor"], Rule::XorL, Rule::XorR),
    ("∀", &["all", "forall", "a"], Rule::AllL, Rule::AllR),
    ("∃", &["some", "exists", "e"], Rule::ExL, Rule::ExR),
];

const OTHERS: [(Rule, &str, &[&str]); 6] = [
    (Rule::Ax, "Ax", &["ax", "id", "axiom", "init", "i"]),
    (Rule::Cut, "Cut", &["cut"]),
    (Rule::WL, "WL", &["wl", "weakl", "lw"]),
    (Rule::WR, "WR", &["wr", "weakr", "rw"]),
    (Rule::CL, "CL", &["cl", "contrl", "lc"]),
    (Rule::CR, "CR", &["cr", "contrr", "rc"]),
];

/// A rule by any of its names: `∧L`, `&L`, `andL`, `L∧`, `∧-L`.
pub fn rule_named(w: &str) -> Option<Rule> {
    let w = w.to_lowercase();
    if let Some((r, _, _)) = OTHERS.iter().find(|(_, _, names)| names.contains(&w.as_str())) {
        return Some(*r);
    }
    let (core, left) = if let Some(c) = w.strip_suffix('l') {
        (c.strip_suffix(['-', '_']).unwrap_or(c), true)
    } else if let Some(c) = w.strip_suffix('r') {
        (c.strip_suffix(['-', '_']).unwrap_or(c), false)
    } else if let Some(c) = w.strip_prefix('l') {
        (c.strip_prefix(['-', '_']).unwrap_or(c), true)
    } else {
        let c = w.strip_prefix('r')?;
        (c.strip_prefix(['-', '_']).unwrap_or(c), false)
    };
    CONNECTIVES.iter().find(|(sym, names, ..)| *sym == core || names.contains(&core)).map(|(_, _, l, r)| if left { *l } else { *r })
}

pub fn rule_name(r: Rule) -> String {
    if let Some((_, name, _)) = OTHERS.iter().find(|(x, ..)| *x == r) {
        return (*name).into();
    }
    let (sym, _, l, _) = CONNECTIVES.iter().find(|(_, _, l, rr)| *l == r || *rr == r).unwrap();
    format!("{sym}{}", if *l == r { "L" } else { "R" })
}

fn all_rules() -> String {
    let mut names: Vec<String> = OTHERS.iter().map(|(_, n, _)| (*n).to_string()).collect();
    names.extend(CONNECTIVES.iter().flat_map(|(s, ..)| [format!("{s}L"), format!("{s}R")]));
    names.join(" ")
}

/* ── sequents ─────────────────────────────────────────────────────── */

fn has(set: &[Expr], e: &Expr) -> bool {
    set.iter().any(|x| same(x, e))
}

/// The first of `p` neither in `base` nor in `add`.
fn stray<'a>(p: &'a [Expr], base: &[Expr], add: &[Expr]) -> Option<&'a Expr> {
    p.iter().find(|e| !has(base, e) && !has(add, e))
}

fn side_text(fs: &[Expr]) -> String {
    fs.iter().map(print::expr).collect::<Vec<_>>().join(", ")
}

pub fn show(s: &Sequent) -> String {
    format!("{} ⊢ {}", side_text(&s.left), side_text(&s.right)).trim().to_string()
}

/// `p, q ⊢ r`: either side may be empty.
pub fn read_sequent(body: &str) -> Result<Sequent, String> {
    let (l, r) = ["⊢", "|-", "⇒"].iter().find_map(|t| body.split_once(t)).ok_or_else(|| "a sequent has ⊢ between its two sides".to_string())?;
    let side = |t: &str| if t.trim().is_empty() { Ok(Vec::new()) } else { parse::formulas(t).map_err(|d| d.message) };
    Ok(Sequent { left: side(l)?, right: side(r)? })
}

fn is_true(e: &Expr) -> bool {
    matches!(e, Expr::Bool(true))
}

fn axiom(s: &Sequent) -> bool {
    s.left.iter().any(|e| has(&s.right, e) || is_false(e)) || s.right.iter().any(is_true)
}

fn parts(e: &Expr) -> Option<(Op, Expr, Expr)> {
    match e {
        Expr::Bin(op, a, b) if op.is_logical() => Some((*op, (**a).clone(), (**b).clone())),
        _ => None,
    }
}

/// What a propositional rule adds above, for each of its premises: to the
/// left and to the right, when `f` is its principal formula.
type Adds = Vec<(Vec<Expr>, Vec<Expr>)>;

fn branches(r: Rule, f: &Expr) -> Option<Adds> {
    if let Expr::Not(a) = f {
        let a = (**a).clone();
        return match r {
            Rule::NotL => Some(vec![(vec![], vec![a])]),
            Rule::NotR => Some(vec![(vec![a], vec![])]),
            _ => None,
        };
    }
    let (op, a, b) = parts(f)?;
    Some(match (r, op) {
        (Rule::AndL, Op::And) => vec![(vec![a, b], vec![])],
        (Rule::AndR, Op::And) => vec![(vec![], vec![a]), (vec![], vec![b])],
        (Rule::OrL, Op::Or) => vec![(vec![a], vec![]), (vec![b], vec![])],
        (Rule::OrR, Op::Or) => vec![(vec![], vec![a, b])],
        (Rule::ImpL, Op::Imp) => vec![(vec![], vec![a]), (vec![b], vec![])],
        (Rule::ImpR, Op::Imp) => vec![(vec![a], vec![b])],
        (Rule::IffL, Op::Iff) => vec![(vec![a.clone(), b.clone()], vec![]), (vec![], vec![a, b])],
        (Rule::IffR, Op::Iff) => vec![(vec![a.clone()], vec![b.clone()]), (vec![b], vec![a])],
        (Rule::XorL, Op::Xor) => vec![(vec![a.clone()], vec![b.clone()]), (vec![b], vec![a])],
        (Rule::XorR, Op::Xor) => vec![(vec![a.clone(), b.clone()], vec![]), (vec![], vec![a, b])],
        _ => return None,
    })
}

fn is_left(r: Rule) -> bool {
    matches!(r, Rule::AndL | Rule::OrL | Rule::ImpL | Rule::NotL | Rule::IffL | Rule::XorL | Rule::AllL | Rule::ExL | Rule::WL | Rule::CL)
}

fn arity(r: Rule) -> usize {
    match r {
        Rule::Ax => 0,
        Rule::AndR | Rule::OrL | Rule::ImpL | Rule::IffL | Rule::IffR | Rule::XorL | Rule::XorR | Rule::Cut => 2,
        _ => 1,
    }
}

fn what(r: Rule) -> &'static str {
    match r {
        Rule::AndL | Rule::AndR => "a conjunction",
        Rule::OrL | Rule::OrR => "a disjunction",
        Rule::ImpL | Rule::ImpR => "a conditional",
        Rule::NotL | Rule::NotR => "a negation",
        Rule::IffL | Rule::IffR => "a biconditional",
        Rule::XorL | Rule::XorR => "an exclusive or",
        Rule::AllL | Rule::AllR => "a universal",
        _ => "an existential",
    }
}

/* ── checking ─────────────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SLine {
    pub at: usize,
    pub n: usize,
    pub seq: Sequent,
    pub rule: Rule,
    pub cites: Vec<Cite>,
}

/// Line `m`, the premise `p`, within this sequent's sides with `add` added.
fn fits(m: usize, p: &Sequent, s: &Sequent, add: &(Vec<Expr>, Vec<Expr>), why: &str) -> Result<(), String> {
    if let Some(e) = stray(&p.left, &s.left, &add.0) {
        return Err(format!("line {m} has {} on its left, which {why} does not account for", print::expr(e)));
    }
    if let Some(e) = stray(&p.right, &s.right, &add.1) {
        return Err(format!("line {m} has {} on its right, which {why} does not account for", print::expr(e)));
    }
    Ok(())
}

fn check_line(lines: &[SLine], k: usize) -> Result<String, String> {
    let l = &lines[k];
    let s = &l.seq;
    let name = rule_name(l.rule);
    let mut above = Vec::new();
    for c in &l.cites {
        let Cite::Line(m) = *c else { return Err("a sequent proof cites lines, not subproofs".into()) };
        match lines.iter().position(|x| x.n == m) {
            Some(i) if i < k => above.push((m, &lines[i].seq)),
            Some(_) => return Err(format!("line {m} is not before this one")),
            None => return Err(format!("there is no line {m}")),
        }
    }
    if above.len() != arity(l.rule) {
        return Err(format!("{name} has {} line{} above it", arity(l.rule), if arity(l.rule) == 1 { "" } else { "s" }));
    }
    let side = |left: bool| if left { &s.left } else { &s.right };
    let word = |left: bool| if left { "left" } else { "right" };
    match l.rule {
        Rule::Ax => {
            if axiom(s) {
                Ok("an axiom: a formula on both sides".into())
            } else {
                Err("an axiom has one formula on both sides, ⊥ on the left or ⊤ on the right".into())
            }
        }
        Rule::WL | Rule::WR | Rule::CL | Rule::CR => {
            let (m, p) = above[0];
            fits(m, p, s, &(vec![], vec![]), &name)?;
            Ok(format!("{name}: line {m}, with more beside it"))
        }
        Rule::Cut => {
            let ((m1, p1), (m2, p2)) = (above[0], above[1]);
            let mut why = String::new();
            // The formula cut: one on the right of a line above, and on the
            // left of the other.
            for ((ma, a), (mb, b)) in [((m1, p1), (m2, p2)), ((m2, p2), (m1, p1))] {
                for c in a.right.iter().map(|c| vec![c.clone()]).chain([vec![]]) {
                    let r = fits(ma, a, s, &(vec![], c.clone()), "Cut").and_then(|_| fits(mb, b, s, &(c.clone(), vec![]), "Cut"));
                    match r {
                        Ok(()) => return Ok(c.first().map_or("Cut".into(), |c| format!("Cut on {}", print::expr(c)))),
                        Err(e) if why.is_empty() => why = e,
                        Err(_) => {}
                    }
                }
            }
            Err(why)
        }
        Rule::AllL | Rule::ExR | Rule::AllR | Rule::ExL => {
            let left = is_left(l.rule);
            let q = if matches!(l.rule, Rule::AllL | Rule::AllR) { Quant::All } else { Quant::Some };
            // Whether the term must be a name the sequent does not have.
            let new = matches!(l.rule, Rule::AllR | Rule::ExL);
            let (m, p) = above[0];
            let names: BTreeSet<String> = s.left.iter().chain(&s.right).flat_map(|e| e.free()).collect();
            let mut why = None;
            for f in side(left).iter() {
                let Some((x, body)) = open_quant(f, q) else { continue };
                let other = if left { &p.right } else { &p.left };
                if let Some(e) = stray(other, side(!left), &[]) {
                    why.get_or_insert(format!("line {m} has {} on its {}, which {name} does not account for", print::expr(e), word(!left)));
                    continue;
                }
                let mut bad = None;
                for e in side_of(p, left).iter().filter(|e| !has(side(left), e)) {
                    match instance(&body, &x, e) {
                        Err(()) => bad = Some(format!("line {m} has {} on its {}, which is not {} with a term for {x}", print::expr(e), word(left), print::expr(&body))),
                        Ok(Some(Expr::Name(a))) if new && names.contains(&a) => bad = Some(format!("{name} needs a name for {x} that this sequent does not mention: {a} is in it")),
                        Ok(Some(t)) if new && !matches!(t, Expr::Name(_)) => bad = Some(format!("{name} needs a new name for {x}: {} is not a name", print::expr(&t))),
                        Ok(_) => continue,
                    }
                    break;
                }
                match bad {
                    None => return Ok(format!("{name} of {}", print::expr(f))),
                    Some(b) => {
                        why.get_or_insert(b);
                    }
                }
            }
            Err(why.unwrap_or_else(|| format!("{name} needs {} on the {} of this sequent", what(l.rule), word(left))))
        }
        r => {
            let left = is_left(r);
            let mut why = None;
            for f in side(left).iter() {
                let Some(bs) = branches(r, f) else { continue };
                let said = format!("{name} of {}", print::expr(f));
                let tried = if bs.len() == 1 {
                    fits(above[0].0, above[0].1, s, &bs[0], &said)
                } else {
                    let one = fits(above[0].0, above[0].1, s, &bs[0], &said).and_then(|_| fits(above[1].0, above[1].1, s, &bs[1], &said));
                    one.clone().or_else(|_| fits(above[0].0, above[0].1, s, &bs[1], &said).and_then(|_| fits(above[1].0, above[1].1, s, &bs[0], &said))).or(one)
                };
                match tried {
                    Ok(()) => return Ok(said),
                    Err(e) => {
                        why.get_or_insert(e);
                    }
                }
            }
            Err(why.unwrap_or_else(|| format!("{name} needs {} on the {} of this sequent", what(r), word(left))))
        }
    }
}

fn side_of(s: &Sequent, left: bool) -> &[Expr] {
    if left {
        &s.left
    } else {
        &s.right
    }
}

/// A sequent proof's file read and checked, as any `.proof` file's is.
pub fn report(text: &str) -> Report {
    let (mut header, body) = proofs::header(text);
    let mut lines = Vec::new();
    for (at, t) in body {
        let read = proofs::split(&t, &|w| rule_named(w).is_some(), &all_rules()).and_then(|s| {
            if s.depth > 0 {
                return Err(format!("line {}: a sequent proof has no bars; each line is a sequent", s.n));
            }
            let seq = read_sequent(&s.body).map_err(|m| format!("line {}: {m}", s.n))?;
            Ok(SLine { at, n: s.n, seq, rule: rule_named(&s.rule).unwrap(), cites: s.cites })
        });
        match read {
            Ok(l) => lines.push(l),
            Err(message) => header.unread.push(Diag { line: at, col: 0, message }),
        }
    }
    let mut seen = BTreeSet::new();
    let out: Vec<LineReport> = (0..lines.len())
        .map(|k| {
            let l = &lines[k];
            let r = if seen.insert(l.n) { check_line(&lines, k) } else { Err(format!("there is a line {} already", l.n)) };
            LineReport { at: l.at, n: l.n, text: show(&l.seq), rule: rule_name(l.rule), ok: r.is_ok(), message: r.unwrap_or_else(|e| e) }
        })
        .collect();
    let all_ok = out.iter().all(|l| l.ok) && header.unread.is_empty();
    let reached = lines.last().is_some_and(|l| {
        l.seq.left.iter().all(|e| has(&header.premises, e)) && header.goal.as_ref().is_none_or(|g| l.seq.right.iter().all(|e| same(e, g)))
    });
    let wrong = out.iter().filter(|l| !l.ok).count() + header.unread.len();
    let summary = summary(&header, out.is_empty(), wrong, reached);
    Report { complete: all_ok && reached, lines: out, summary, header }
}

fn summary(h: &Header, empty: bool, wrong: usize, reached: bool) -> String {
    if empty {
        "no lines yet".into()
    } else if wrong > 0 {
        format!("{wrong} line{} wrong", if wrong == 1 { " is" } else { "s are" })
    } else if reached {
        match &h.goal {
            Some(g) => format!("a proof of {} from {}", print::expr(g), proofs::premises_said(&h.premises)),
            None => "every line is right".into(),
        }
    } else {
        "every line is right; the last is not the premises ⊢ the goal yet".into()
    }
}

/* ── finding ──────────────────────────────────────────────────────── */

struct Node {
    seq: Sequent,
    rule: Rule,
    kids: Vec<Node>,
}

struct Finder {
    deadline: Instant,
    steps: u64,
    gave_up: bool,
    failed: HashSet<(String, u32)>,
}

fn key(s: &Sequent) -> String {
    let mut l: Vec<String> = s.left.iter().map(print::expr).collect();
    let mut r: Vec<String> = s.right.iter().map(print::expr).collect();
    l.sort();
    r.sort();
    format!("{} ⊢ {}", l.join(", "), r.join(", "))
}

/// `s` with formula `i` of one side taken away (when `drop`), and more added.
fn above(s: &Sequent, drop: Option<(bool, usize)>, left: &[Expr], right: &[Expr]) -> Sequent {
    let mut out = s.clone();
    match drop {
        Some((true, i)) => {
            out.left.remove(i);
        }
        Some((false, i)) => {
            out.right.remove(i);
        }
        None => {}
    }
    for e in left {
        if !has(&out.left, e) {
            out.left.push(e.clone());
        }
    }
    for e in right {
        if !has(&out.right, e) {
            out.right.push(e.clone());
        }
    }
    out
}

/// Every name and function anywhere in the sequent: a new name is none of them.
fn symbols(s: &Sequent) -> BTreeSet<String> {
    fn go(e: &Expr, out: &mut BTreeSet<String>) {
        match e {
            Expr::Name(n) => {
                out.insert(n.clone());
            }
            Expr::App(f, args) => {
                out.insert(f.clone());
                args.iter().for_each(|a| go(a, out));
            }
            Expr::Not(a) | Expr::Neg(a) => go(a, out),
            Expr::Bin(_, a, b) => {
                go(a, out);
                go(b, out);
            }
            Expr::Quant(_, bs, a) => {
                out.extend(bs.iter().map(|b| b.name.clone()));
                go(a, out);
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    s.left.iter().chain(&s.right).for_each(|e| go(e, &mut out));
    out
}

fn fresh(s: &Sequent) -> String {
    let taken = symbols(s);
    (0..).flat_map(|i| ["a", "b", "c", "d"].map(|b| if i == 0 { b.to_string() } else { format!("{b}{i}") })).find(|n| !taken.contains(n)).unwrap()
}

/// The closed terms where a term goes, smallest first: what a universal
/// may be given.
fn terms(s: &Sequent) -> Vec<Expr> {
    fn term(e: &Expr, bound: &[String], out: &mut Vec<Expr>) -> bool {
        let closed = match e {
            Expr::Name(n) => !bound.contains(n),
            // Every argument looked at, closed or not.
            Expr::App(_, args) => args.iter().filter(|a| !term(a, bound, out)).count() == 0,
            Expr::Neg(a) => term(a, bound, out),
            Expr::Bin(_, a, b) => term(a, bound, out) & term(b, bound, out),
            _ => true,
        };
        if closed && matches!(e, Expr::Name(_) | Expr::App(..)) && !out.contains(e) {
            out.push(e.clone());
        }
        closed
    }
    fn formula(e: &Expr, bound: &mut Vec<String>, out: &mut Vec<Expr>) {
        match e {
            Expr::Not(a) => formula(a, bound, out),
            Expr::Bin(op, a, b) if op.is_logical() => {
                formula(a, bound, out);
                formula(b, bound, out);
            }
            Expr::Bin(_, a, b) => {
                term(a, bound, out);
                term(b, bound, out);
            }
            Expr::App(_, args) => args.iter().for_each(|a| {
                term(a, bound, out);
            }),
            Expr::Quant(_, bs, a) => {
                let n = bound.len();
                bound.extend(bs.iter().map(|b| b.name.clone()));
                formula(a, bound, out);
                bound.truncate(n);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    s.left.iter().chain(&s.right).for_each(|e| formula(e, &mut Vec::new(), &mut out));
    out.sort_by_key(|t| print::expr(t).len());
    out
}

impl Finder {
    fn search(&mut self, s: &Sequent, budget: u32) -> Option<Node> {
        self.steps += 1;
        if self.steps % 256 == 0 && Instant::now() >= self.deadline {
            self.gave_up = true;
        }
        if self.gave_up {
            return None;
        }
        if axiom(s) {
            return Some(Node { seq: s.clone(), rule: Rule::Ax, kids: Vec::new() });
        }
        let one = |me: &mut Finder, rule: Rule, up: Sequent| me.search(&up, budget).map(|k| Node { seq: s.clone(), rule, kids: vec![k] });
        // What loses nothing and does not split.
        for (i, f) in s.left.iter().enumerate() {
            let drop = Some((true, i));
            match f {
                Expr::Not(a) => return one(self, Rule::NotL, above(s, drop, &[], &[(**a).clone()])),
                Expr::Bin(Op::And, a, b) => return one(self, Rule::AndL, above(s, drop, &[(**a).clone(), (**b).clone()], &[])),
                Expr::Quant(Quant::Some, ..) => {
                    let (x, body) = open_quant(f, Quant::Some).unwrap();
                    return one(self, Rule::ExL, above(s, drop, &[subst(&body, &x, &Expr::Name(fresh(s)))], &[]));
                }
                _ => {}
            }
        }
        for (i, f) in s.right.iter().enumerate() {
            let drop = Some((false, i));
            match f {
                Expr::Not(a) => return one(self, Rule::NotR, above(s, drop, &[(**a).clone()], &[])),
                Expr::Bin(Op::Or, a, b) => return one(self, Rule::OrR, above(s, drop, &[], &[(**a).clone(), (**b).clone()])),
                Expr::Bin(Op::Imp, a, b) => return one(self, Rule::ImpR, above(s, drop, &[(**a).clone()], &[(**b).clone()])),
                Expr::Quant(Quant::All, ..) => {
                    let (x, body) = open_quant(f, Quant::All).unwrap();
                    return one(self, Rule::AllR, above(s, drop, &[], &[subst(&body, &x, &Expr::Name(fresh(s)))]));
                }
                _ => {}
            }
        }
        // What splits: both branches must close.
        for left in [true, false] {
            for (i, f) in side_of(s, left).iter().enumerate() {
                let rule = match (left, f) {
                    (true, Expr::Bin(Op::Or, ..)) => Rule::OrL,
                    (true, Expr::Bin(Op::Imp, ..)) => Rule::ImpL,
                    (true, Expr::Bin(Op::Iff, ..)) => Rule::IffL,
                    (true, Expr::Bin(Op::Xor, ..)) => Rule::XorL,
                    (false, Expr::Bin(Op::And, ..)) => Rule::AndR,
                    (false, Expr::Bin(Op::Iff, ..)) => Rule::IffR,
                    (false, Expr::Bin(Op::Xor, ..)) => Rule::XorR,
                    _ => continue,
                };
                let mut kids = Vec::new();
                for (l, r) in branches(rule, f).unwrap() {
                    kids.push(self.search(&above(s, Some((left, i)), &l, &r), budget)?);
                }
                return Some(Node { seq: s.clone(), rule, kids });
            }
        }
        // A universal on the left or an existential on the right given a
        // term: the one choice there is, tried each way within the budget.
        if budget == 0 || !self.failed.insert((key(s), budget)) {
            return None;
        }
        let mut ts = terms(s);
        if ts.is_empty() {
            ts.push(Expr::Name(fresh(s)));
        }
        for left in [true, false] {
            let (q, rule) = if left { (Quant::All, Rule::AllL) } else { (Quant::Some, Rule::ExR) };
            for f in side_of(s, left) {
                let Some((x, body)) = open_quant(f, q) else { continue };
                for t in &ts {
                    let inst = subst(&body, &x, t);
                    if has(side_of(s, left), &inst) {
                        continue;
                    }
                    let up = if left { above(s, None, &[inst], &[]) } else { above(s, None, &[], &[inst]) };
                    if let Some(k) = self.search(&up, budget - 1) {
                        return Some(Node { seq: s.clone(), rule, kids: vec![k] });
                    }
                }
            }
        }
        None
    }
}

fn quantified(e: &Expr) -> bool {
    match e {
        Expr::Quant(..) => true,
        Expr::Not(a) => quantified(a),
        Expr::Bin(_, a, b) => quantified(a) || quantified(b),
        _ => false,
    }
}

/// A sequent proof of `goal` from `premises`, found, as a `.proof` file's
/// text; or why none was.
pub fn find(premises: &[Expr], goal: &Expr, time: Duration) -> Result<String, String> {
    let start = Sequent { left: premises.to_vec(), right: vec![goal.clone()] };
    let mut f = Finder { deadline: Instant::now() + time, steps: 0, gave_up: false, failed: HashSet::new() };
    let first_order = premises.iter().chain([goal]).any(quantified);
    let mut found = None;
    for budget in 0..=if first_order { MOST_INSTANCES } else { 0 } {
        found = f.search(&start, budget);
        if found.is_some() || f.gave_up {
            break;
        }
    }
    let Some(tree) = found else {
        return Err(if f.gave_up {
            "no proof found in the time".into()
        } else if first_order {
            "no proof found within the search's bounds".into()
        } else {
            "a branch ends with no axiom: the argument does not hold".into()
        });
    };
    let mut rows = Vec::new();
    fn put(n: &Node, rows: &mut Vec<(String, String)>) -> usize {
        let kids: Vec<String> = n.kids.iter().map(|k| put(k, rows).to_string()).collect();
        let name = rule_name(n.rule);
        rows.push((show(&n.seq), if kids.is_empty() { name } else { format!("{name} {}", kids.join(", ")) }));
        rows.len()
    }
    put(&tree, &mut rows);
    let text = proofs::write_header("sequent", premises, goal) + &proofs::write_lines(&rows);
    let checked = report(&text);
    if !checked.complete {
        let bad = checked.lines.iter().find(|c| !c.ok).map(|c| format!("line {}: {}", c.n, c.message)).unwrap_or(checked.summary);
        return Err(format!("the proof found did not check ({bad}): a fault in the finder"));
    }
    Ok(text)
}
