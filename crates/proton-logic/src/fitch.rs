//! Natural deduction, Fitch style: a `.proof` file read, each line checked
//! against its rule, and proofs found.
//!
//! ```text
//! system fitch
//! premises p -> q, q -> r
//! goal p -> r
//!
//! 1  p -> q        premise
//! 2  q -> r        premise
//! 3  | p           assume
//! 4  | q           →E 1, 3
//! 5  | r           →E 2, 4
//! 6  p -> r        →I 3-5
//! ```
//!
//! - **Lines:** a number, a bar `|` for each subproof the line is in, the
//!   formula, its rule, and the lines (`3`) and subproofs (`3-5`) it uses.
//!   An `assume` line opens a subproof, the bars after it say how far it
//!   goes, and another `assume` at the same depth begins the next.
//! - **Rules** are *forall x*'s, by its names, and read by the others
//!   courses use: `→E`, `->E`, `⊃E` and `MP` are one rule. See [`RULES`].
//! - **What can be cited:** a line before this one, not inside a subproof
//!   that has ended; a subproof that has ended, from within the subproof it
//!   was in.
//! - **Proofs found** by a search that works back from the goal with the
//!   introduction rules and forward from what is had with the elimination
//!   rules, splits cases on a disjunction or an existential, and falls back
//!   on indirect proof; bounded, so for first-order logic it may not find one
//!   that exists.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::ast::*;
use crate::parse::{self, Diag};
use crate::{print, proofs};

/* ── rules ────────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rule {
    Premise,
    Assume,
    R,
    AndI,
    AndE,
    OrI,
    OrE,
    ImpI,
    ImpE,
    IffI,
    IffE,
    NotI,
    NotE,
    X,
    IP,
    /// `RAA`: ¬I or IP, as the courses that use the name mean either.
    Raa,
    DNE,
    MT,
    DS,
    LEM,
    DeM,
    AllI,
    AllE,
    ExI,
    ExE,
    EqI,
    EqE,
}

/// Each rule: the name Proton writes (*forall x*'s), then the others it
/// reads, compared without case.
pub const RULES: &[(Rule, &[&str])] = &[
    (Rule::Premise, &["premise", "pr", "prem", "p", "given", "hyp"]),
    (Rule::Assume, &["assume", "as", "assumption", "a"]),
    (Rule::R, &["R", "reit", "reiteration", "rep"]),
    (Rule::AndI, &["∧I", "&I", "^I", "andI", "conj", "adj", "∧intro"]),
    (Rule::AndE, &["∧E", "&E", "^E", "andE", "simp", "∧elim"]),
    (Rule::OrI, &["∨I", "vI", "|I", "orI", "add", "∨intro"]),
    (Rule::OrE, &["∨E", "vE", "|E", "orE", "∨elim", "cases"]),
    (Rule::ImpI, &["→I", "->I", "⊃I", "impI", "CP", "→intro"]),
    (Rule::ImpE, &["→E", "->E", "⊃E", "impE", "MP", "→elim"]),
    (Rule::IffI, &["↔I", "<->I", "≡I", "iffI", "bicondI"]),
    (Rule::IffE, &["↔E", "<->E", "≡E", "iffE", "bicondE"]),
    (Rule::NotI, &["¬I", "~I", "!I", "notI", "-I"]),
    (Rule::NotE, &["¬E", "~E", "!E", "notE", "-E", "⊥I", "falseI"]),
    (Rule::X, &["X", "⊥E", "falseE", "EFQ", "explosion", "ex falso"]),
    (Rule::IP, &["IP", "indirect", "¬¬I"]),
    (Rule::Raa, &["RAA"]),
    (Rule::DNE, &["DNE", "¬¬E", "~~E", "DN"]),
    (Rule::MT, &["MT", "modus tollens"]),
    (Rule::DS, &["DS", "disjunctive syllogism"]),
    (Rule::LEM, &["LEM", "TND"]),
    (Rule::DeM, &["DeM", "de morgan", "DM"]),
    (Rule::AllI, &["∀I", "AI", "allI", "UG", "∀intro"]),
    (Rule::AllE, &["∀E", "AE", "allE", "UI", "∀elim"]),
    (Rule::ExI, &["∃I", "EI", "someI", "EG", "∃intro"]),
    (Rule::ExE, &["∃E", "EE", "someE", "∃elim"]),
    (Rule::EqI, &["=I", "refl"]),
    (Rule::EqE, &["=E", "LL", "sub", "subst"]),
];

fn rule_named(w: &str) -> Option<Rule> {
    let w = w.to_lowercase();
    RULES.iter().find(|(_, names)| names.iter().any(|n| n.to_lowercase() == w)).map(|(r, _)| *r)
}

pub fn rule_name(r: Rule) -> &'static str {
    RULES.iter().find(|(x, _)| *x == r).map(|(_, n)| n[0]).unwrap_or("?")
}

/* ── the file ─────────────────────────────────────────────────────── */

pub use crate::proofs::Cite;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PLine {
    /// Its line in the file, from 1.
    pub at: usize,
    /// Its number in the proof.
    pub n: usize,
    pub depth: usize,
    pub formula: Expr,
    pub rule: Rule,
    pub cites: Vec<Cite>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofFile {
    pub system: String,
    pub premises: Vec<Expr>,
    pub goal: Option<Expr>,
    pub lines: Vec<PLine>,
    /// Lines that could not be read, by file line.
    pub unread: Vec<Diag>,
}

/// A `.proof` file read: its header and its lines, and what could not be
/// read, line by line, so the rest is still checked.
pub fn read(text: &str) -> ProofFile {
    let (h, body) = proofs::header(text);
    let mut f = ProofFile { system: h.system, premises: h.premises, goal: h.goal, lines: Vec::new(), unread: h.unread };
    for (at, t) in body {
        match read_line(&t, at) {
            Ok(l) => f.lines.push(l),
            Err(message) => f.unread.push(Diag { line: at, col: 0, message }),
        }
    }
    f
}

/// `12  | | A ∧ B   ∧I 3, 7`
fn read_line(t: &str, at: usize) -> Result<PLine, String> {
    let all = RULES.iter().map(|(r, _)| rule_name(*r)).collect::<Vec<_>>().join(" ");
    let s = proofs::split(t, &|w| rule_named(w).is_some(), &all)?;
    let formula = parse::formula(&s.body).map_err(|d| format!("line {}: {}", s.n, d.message))?;
    Ok(PLine { at, n: s.n, depth: s.depth, formula, rule: rule_named(&s.rule).unwrap(), cites: s.cites })
}

/* ── checking ─────────────────────────────────────────────────────── */

/// A line's check: right, or what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineCheck {
    pub at: usize,
    pub n: usize,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    pub lines: Vec<LineCheck>,
    /// Every line right, and the last line, outside every subproof, the
    /// goal (when there is one).
    pub complete: bool,
    pub summary: String,
}

/// The subproofs open at each line: each by the index of its assumption.
fn scopes(lines: &[PLine]) -> (Vec<Vec<usize>>, Vec<Option<String>>) {
    let mut open: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    let mut wrong = Vec::new();
    for (k, l) in lines.iter().enumerate() {
        let mut problem = None;
        if l.rule == Rule::Assume {
            // A subproof at this depth: those as deep or deeper end here.
            while open.len() >= l.depth.max(1) {
                open.pop();
            }
            if open.len() + 1 != l.depth {
                problem = Some(format!("an assumption opens a subproof one deeper: this has {} bars where {} were expected", l.depth, open.len() + 1));
            }
            open.push(k);
        } else {
            while open.len() > l.depth {
                open.pop();
            }
            if open.len() != l.depth {
                problem = Some(format!("a line {} subproofs deep needs an assumption above it to open them", l.depth));
            }
        }
        out.push(open.clone());
        wrong.push(problem);
    }
    (out, wrong)
}

struct Proof<'a> {
    lines: &'a [PLine],
    scope: Vec<Vec<usize>>,
    premises: &'a [Expr],
}

impl Proof<'_> {
    fn index(&self, n: usize) -> Option<usize> {
        self.lines.iter().position(|l| l.n == n)
    }

    /// Line `m` as cited at line index `k`: before it, and not inside a
    /// subproof that has ended.
    fn line(&self, k: usize, m: usize) -> Result<&Expr, String> {
        let i = self.index(m).ok_or_else(|| format!("there is no line {m}"))?;
        if i >= k {
            return Err(format!("line {m} is not before this one"));
        }
        if !self.scope[k].starts_with(&self.scope[i]) {
            return Err(format!("line {m} is inside a subproof that has ended"));
        }
        Ok(&self.lines[i].formula)
    }

    /// Subproof `a-b` as cited at line index `k`: its assumption and its
    /// last line, the subproof ended, and in a subproof still open here.
    fn sub(&self, k: usize, a: usize, b: usize) -> Result<(&Expr, &Expr, usize), String> {
        let (i, j) = (self.index(a).ok_or_else(|| format!("there is no line {a}"))?, self.index(b).ok_or_else(|| format!("there is no line {b}"))?);
        if self.lines[i].rule != Rule::Assume {
            return Err(format!("{a}-{b} is not a subproof: line {a} is not an assumption"));
        }
        if j < i || j >= k {
            return Err(format!("{a}-{b} is not a subproof before this line"));
        }
        let mine = &self.scope[i];
        if !self.scope[j].starts_with(mine) {
            return Err(format!("line {b} is not in the subproof {a} opens"));
        }
        // It ends at b: the next line is outside it.
        if j + 1 < self.lines.len() && self.scope[j + 1].starts_with(mine) && j + 1 != k {
            return Err(format!("the subproof {a} opens does not end at line {b}"));
        }
        if !self.scope[k].starts_with(&mine[..mine.len() - 1]) || self.scope[k].starts_with(mine) {
            return Err(format!("the subproof {a}-{b} is not one this line can use"));
        }
        Ok((&self.lines[i].formula, &self.lines[j].formula, i))
    }

    /// The formulas assumed and still open at line index `k`, and the premises.
    fn open_assumptions(&self, k: usize) -> Vec<&Expr> {
        let mut out: Vec<&Expr> = self.premises.iter().collect();
        out.extend(self.lines[..=k].iter().filter(|l| l.rule == Rule::Premise).map(|l| &l.formula));
        out.extend(self.scope[k].iter().map(|&i| &self.lines[i].formula));
        out
    }
}

/// A `.proof` file's lines checked, each against its rule.
pub fn check(f: &ProofFile) -> Checked {
    let (scope, wrong) = scopes(&f.lines);
    let proof = Proof { lines: &f.lines, scope, premises: &f.premises };
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (k, l) in f.lines.iter().enumerate() {
        let result = if !seen.insert(l.n) {
            Err(format!("there is a line {} already", l.n))
        } else if let Some(w) = &wrong[k] {
            Err(w.clone())
        } else {
            check_line(&proof, k)
        };
        out.push(LineCheck { at: l.at, n: l.n, ok: result.is_ok(), message: result.unwrap_or_else(|e| e) });
    }
    let all_ok = out.iter().all(|c| c.ok) && f.unread.is_empty();
    let last = f.lines.last();
    let reached = match (&f.goal, last) {
        (Some(g), Some(l)) => l.depth == 0 && same(&l.formula, g),
        (None, Some(l)) => l.depth == 0,
        _ => false,
    };
    let wrong_count = out.iter().filter(|c| !c.ok).count() + f.unread.len();
    let summary = if f.lines.is_empty() {
        "no lines yet".to_string()
    } else if wrong_count > 0 {
        format!("{wrong_count} line{} wrong", if wrong_count == 1 { " is" } else { "s are" })
    } else if reached {
        match &f.goal {
            Some(g) => format!("a proof of {} from {}", print::expr(g), if f.premises.is_empty() { "nothing".to_string() } else { f.premises.iter().map(print::expr).collect::<Vec<_>>().join(", ") }),
            None => "every line is right".into(),
        }
    } else {
        "every line is right; the goal is not reached yet".into()
    };
    Checked { lines: out, complete: all_ok && reached, summary }
}

/// Two formulas the same, up to what their bound variables are called.
pub fn same(a: &Expr, b: &Expr) -> bool {
    fn go(a: &Expr, b: &Expr, env: &mut Vec<(String, String)>) -> bool {
        match (a, b) {
            (Expr::Name(x), Expr::Name(y)) => match env.iter().rev().find(|(p, q)| p == x || q == y) {
                Some((p, q)) => p == x && q == y,
                None => x == y,
            },
            (Expr::Quant(q1, b1, x1), Expr::Quant(q2, b2, x2)) => {
                if q1 != q2 || b1.len() != b2.len() {
                    return false;
                }
                let n = env.len();
                env.extend(b1.iter().zip(b2).map(|(p, q)| (p.name.clone(), q.name.clone())));
                let r = go(x1, x2, env);
                env.truncate(n);
                r
            }
            (Expr::App(f, xs), Expr::App(g, ys)) => f == g && xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| go(x, y, env)),
            (Expr::Not(x), Expr::Not(y)) | (Expr::Neg(x), Expr::Neg(y)) => go(x, y, env),
            (Expr::Bin(o1, x1, y1), Expr::Bin(o2, x2, y2)) => o1 == o2 && go(x1, x2, env) && go(y1, y2, env),
            _ => a == b,
        }
    }
    go(a, b, &mut Vec::new())
}

/// The first binder taken off a quantifier: its variable, and the rest.
pub(crate) fn open_quant(e: &Expr, want: Quant) -> Option<(String, Expr)> {
    match e {
        Expr::Quant(q, bs, body) if *q == want => {
            let rest = if bs.len() > 1 { Expr::Quant(*q, bs[1..].to_vec(), body.clone()) } else { (**body).clone() };
            Some((bs[0].name.clone(), rest))
        }
        _ => None,
    }
}

/// `e` with the free `x` replaced by `t`.
pub fn subst(e: &Expr, x: &str, t: &Expr) -> Expr {
    match e {
        Expr::Name(n) if n == x => t.clone(),
        Expr::App(f, args) => Expr::App(f.clone(), args.iter().map(|a| subst(a, x, t)).collect()),
        Expr::Not(a) => Expr::negation(subst(a, x, t)),
        Expr::Neg(a) => Expr::Neg(Box::new(subst(a, x, t))),
        Expr::Bin(op, a, b) => Expr::bin(*op, subst(a, x, t), subst(b, x, t)),
        Expr::Quant(q, bs, body) if !bs.iter().any(|b| b.name == x) => Expr::Quant(*q, bs.clone(), Box::new(subst(body, x, t))),
        _ => e.clone(),
    }
}

/// What `x` stands for if `inst` is `body` with it replaced — every
/// occurrence by one term — or `Err` if it is not. `Ok(None)`: `x` is not in
/// `body` and the two are the same.
pub(crate) fn instance(body: &Expr, x: &str, inst: &Expr) -> Result<Option<Expr>, ()> {
    fn go(b: &Expr, x: &str, i: &Expr, got: &mut Option<Expr>, bound: &mut Vec<String>) -> bool {
        match (b, i) {
            (Expr::Name(n), _) if n == x && !bound.contains(n) => match got {
                Some(t) => t == i,
                None => {
                    *got = Some(i.clone());
                    true
                }
            },
            (Expr::App(f, xs), Expr::App(g, ys)) => f == g && xs.len() == ys.len() && xs.iter().zip(ys).all(|(p, q)| go(p, x, q, got, bound)),
            (Expr::Not(p), Expr::Not(q)) | (Expr::Neg(p), Expr::Neg(q)) => go(p, x, q, got, bound),
            (Expr::Bin(o1, p1, q1), Expr::Bin(o2, p2, q2)) => o1 == o2 && go(p1, x, p2, got, bound) && go(q1, x, q2, got, bound),
            (Expr::Quant(q1, bs1, p), Expr::Quant(q2, bs2, q)) => {
                if q1 != q2 || bs1 != bs2 {
                    return false;
                }
                let n = bound.len();
                bound.extend(bs1.iter().map(|b| b.name.clone()));
                let r = go(p, x, q, got, bound);
                bound.truncate(n);
                r
            }
            _ => b == i,
        }
    }
    let mut got = None;
    if go(body, x, inst, &mut got, &mut Vec::new()) {
        Ok(got)
    } else {
        Err(())
    }
}

/// Whether `e` is `body` with *some* occurrences of the term `t` written as
/// the bound `x` — as ∃I and =E allow.
fn some_occurrences(body: &Expr, x: &str, t: &Expr, e: &Expr) -> bool {
    match (body, e) {
        (Expr::Name(n), _) if n == x => e == t,
        (Expr::App(f, xs), Expr::App(g, ys)) => f == g && xs.len() == ys.len() && xs.iter().zip(ys).all(|(p, q)| some_occurrences(p, x, t, q)),
        (Expr::Not(p), Expr::Not(q)) | (Expr::Neg(p), Expr::Neg(q)) => some_occurrences(p, x, t, q),
        (Expr::Bin(o1, p1, q1), Expr::Bin(o2, p2, q2)) => o1 == o2 && some_occurrences(p1, x, t, p2) && some_occurrences(q1, x, t, q2),
        (Expr::Quant(q1, bs1, p), Expr::Quant(q2, bs2, q)) => q1 == q2 && bs1 == bs2 && (bs1.iter().any(|b| b.name == x) && p == q || some_occurrences(p, x, t, q)),
        _ => body == e,
    }
}

/// The names free in a formula: its constants, as the quantifier rules
/// need them.
fn names(e: &Expr) -> BTreeSet<String> {
    e.free().into_iter().collect()
}

pub(crate) fn is_false(e: &Expr) -> bool {
    matches!(e, Expr::Bool(false))
}

fn check_line(p: &Proof, k: usize) -> Result<String, String> {
    let l = &p.lines[k];
    let a = &l.formula;
    let lines = |want: usize| -> Result<Vec<usize>, String> {
        let ns: Vec<usize> = l.cites.iter().filter_map(|c| if let Cite::Line(n) = c { Some(*n) } else { None }).collect();
        if ns.len() != want || l.cites.iter().filter(|c| matches!(c, Cite::Range(..))).count() != 0 {
            return Err(format!("{} cites {} line{}", rule_name(l.rule), want, if want == 1 { "" } else { "s" }));
        }
        Ok(ns)
    };
    let get = |m: usize| p.line(k, m);
    let ok = |s: &str| Ok(s.to_string());
    match l.rule {
        Rule::Premise => {
            if l.depth != 0 {
                return Err("a premise is outside every subproof".into());
            }
            if !p.premises.is_empty() && !p.premises.iter().any(|q| same(q, a)) {
                return Err("this is not one of the premises the header names".into());
            }
            ok("a premise")
        }
        Rule::Assume => ok("an assumption: it opens a subproof"),
        Rule::R => {
            let m = lines(1)?;
            if same(get(m[0])?, a) {
                ok("repeated")
            } else {
                Err(format!("line {} is not this formula", m[0]))
            }
        }
        Rule::AndI => {
            let m = lines(2)?;
            let (x, y) = (get(m[0])?, get(m[1])?);
            match a {
                Expr::Bin(Op::And, l1, r1) if (same(l1, x) && same(r1, y)) || (same(l1, y) && same(r1, x)) => ok("both parts are had"),
                Expr::Bin(Op::And, ..) => Err(format!("this is not line {} and line {} joined", m[0], m[1])),
                _ => Err("∧I makes a conjunction".into()),
            }
        }
        Rule::AndE => {
            let m = lines(1)?;
            match get(m[0])? {
                Expr::Bin(Op::And, l1, r1) if same(l1, a) || same(r1, a) => ok("one part of a conjunction"),
                Expr::Bin(Op::And, ..) => Err(format!("this is neither part of line {}", m[0])),
                _ => Err(format!("line {} is not a conjunction", m[0])),
            }
        }
        Rule::OrI => {
            let m = lines(1)?;
            match a {
                Expr::Bin(Op::Or, l1, r1) if same(l1, get(m[0])?) || same(r1, get(m[0])?) => ok("a disjunction of what is had"),
                Expr::Bin(Op::Or, ..) => Err(format!("line {} is neither side of this", m[0])),
                _ => Err("∨I makes a disjunction".into()),
            }
        }
        Rule::OrE => {
            let (mut m, mut subs) = (Vec::new(), Vec::new());
            for c in &l.cites {
                match c {
                    Cite::Line(n) => m.push(*n),
                    Cite::Range(x, y) => subs.push((*x, *y)),
                }
            }
            if m.len() != 1 || subs.len() != 2 {
                return Err("∨E cites a disjunction and two subproofs: m, i-j, k-l".into());
            }
            let Expr::Bin(Op::Or, d1, d2) = get(m[0])? else { return Err(format!("line {} is not a disjunction", m[0])) };
            let (s1, e1, _) = p.sub(k, subs[0].0, subs[0].1)?;
            let (s2, e2, _) = p.sub(k, subs[1].0, subs[1].1)?;
            let assumed = (same(s1, d1) && same(s2, d2)) || (same(s1, d2) && same(s2, d1));
            if !assumed {
                return Err("the subproofs do not assume each side of the disjunction".into());
            }
            if !same(e1, a) || !same(e2, a) {
                return Err("each subproof must end with this line's formula".into());
            }
            ok("true whichever side holds")
        }
        Rule::ImpI => {
            let (i, j) = one_range(l)?;
            let (s, e, _) = p.sub(k, i, j)?;
            match a {
                Expr::Bin(Op::Imp, x, y) if same(x, s) && same(y, e) => ok("what was assumed implies what it led to"),
                Expr::Bin(Op::Imp, ..) => Err(format!("the subproof {i}-{j} assumes {} and ends with {}", print::expr(s), print::expr(e))),
                _ => Err("→I makes a conditional".into()),
            }
        }
        Rule::ImpE => {
            let m = lines(2)?;
            let (x, y) = (get(m[0])?, get(m[1])?);
            let fits = |c: &Expr, ant: &Expr| matches!(c, Expr::Bin(Op::Imp, l1, r1) if same(l1, ant) && same(r1, a));
            if fits(x, y) || fits(y, x) {
                ok("the antecedent is had, so the consequent")
            } else {
                Err(format!("→E needs A → B and A, and gives B: lines {} and {} are not", m[0], m[1]))
            }
        }
        Rule::IffI => {
            let subs = ranges(l, 2)?;
            let (s1, e1, _) = p.sub(k, subs[0].0, subs[0].1)?;
            let (s2, e2, _) = p.sub(k, subs[1].0, subs[1].1)?;
            match a {
                Expr::Bin(Op::Iff, x, y) if (same(x, s1) && same(y, e1) && same(y, s2) && same(x, e2)) || (same(x, s2) && same(y, e2) && same(y, s1) && same(x, e1)) => ok("each leads to the other"),
                _ => Err("↔I needs a subproof from each side to the other".into()),
            }
        }
        Rule::IffE => {
            let m = lines(2)?;
            let (x, y) = (get(m[0])?, get(m[1])?);
            let fits = |b: &Expr, side: &Expr| matches!(b, Expr::Bin(Op::Iff, l1, r1) if (same(l1, side) && same(r1, a)) || (same(r1, side) && same(l1, a)));
            if fits(x, y) || fits(y, x) {
                ok("one side is had, so the other")
            } else {
                Err("↔E needs A ↔ B and one side, and gives the other".into())
            }
        }
        Rule::NotI | Rule::Raa | Rule::IP => {
            let (i, j) = one_range(l)?;
            let (s, e, _) = p.sub(k, i, j)?;
            if !is_false(e) {
                return Err(format!("the subproof {i}-{j} must end with ⊥ (false)"));
            }
            let not_i = matches!(a, Expr::Not(x) if same(x, s));
            let ip = matches!(s, Expr::Not(x) if same(x, a));
            match l.rule {
                Rule::NotI if not_i => ok("what was assumed leads to a contradiction, so it is false"),
                Rule::IP if ip => ok("its denial leads to a contradiction"),
                Rule::Raa if not_i || ip => ok("its assumption leads to a contradiction"),
                Rule::NotI => Err(format!("¬I gives the negation of what {i} assumes")),
                _ => Err(format!("{} gives what {i} denies", rule_name(l.rule))),
            }
        }
        Rule::NotE => {
            let m = lines(2)?;
            let (x, y) = (get(m[0])?, get(m[1])?);
            let clash = matches!(y, Expr::Not(n) if same(n, x)) || matches!(x, Expr::Not(n) if same(n, y));
            if !is_false(a) {
                return Err("¬E gives ⊥ (false)".into());
            }
            if clash {
                ok("a formula and its negation")
            } else {
                Err(format!("lines {} and {} do not contradict each other", m[0], m[1]))
            }
        }
        Rule::X => {
            let m = lines(1)?;
            if is_false(get(m[0])?) {
                ok("anything follows from ⊥")
            } else {
                Err(format!("line {} is not ⊥ (false)", m[0]))
            }
        }
        Rule::DNE => {
            let m = lines(1)?;
            match get(m[0])? {
                Expr::Not(x) if matches!(&**x, Expr::Not(y) if same(y, a)) => ok("two negations taken off"),
                _ => Err(format!("line {} is not this formula negated twice", m[0])),
            }
        }
        Rule::MT => {
            let m = lines(2)?;
            let (x, y) = (get(m[0])?, get(m[1])?);
            let fits = |c: &Expr, n: &Expr| matches!((c, n, a), (Expr::Bin(Op::Imp, p1, q1), Expr::Not(nq), Expr::Not(np)) if same(q1, nq) && same(p1, np));
            if fits(x, y) || fits(y, x) {
                ok("the consequent is false, so the antecedent")
            } else {
                Err("MT needs A → B and ¬B, and gives ¬A".into())
            }
        }
        Rule::DS => {
            let m = lines(2)?;
            let (x, y) = (get(m[0])?, get(m[1])?);
            let fits = |d: &Expr, n: &Expr| match (d, n) {
                (Expr::Bin(Op::Or, l1, r1), Expr::Not(z)) => (same(z, l1) && same(r1, a)) || (same(z, r1) && same(l1, a)),
                _ => false,
            };
            if fits(x, y) || fits(y, x) {
                ok("one side is false, so the other")
            } else {
                Err("DS needs A ∨ B and the negation of one side, and gives the other".into())
            }
        }
        Rule::LEM => {
            let subs = ranges(l, 2)?;
            let (s1, e1, _) = p.sub(k, subs[0].0, subs[0].1)?;
            let (s2, e2, _) = p.sub(k, subs[1].0, subs[1].1)?;
            let opposite = matches!(s2, Expr::Not(x) if same(x, s1)) || matches!(s1, Expr::Not(x) if same(x, s2));
            if opposite && same(e1, a) && same(e2, a) {
                ok("it follows from a formula and from its negation")
            } else {
                Err("LEM needs a subproof from A and one from ¬A, each ending with this".into())
            }
        }
        Rule::DeM => {
            let m = lines(1)?;
            let x = get(m[0])?;
            if de_morgan(x, a) || de_morgan(a, x) {
                ok("De Morgan")
            } else {
                Err(format!("this is not line {} by De Morgan's laws", m[0]))
            }
        }
        Rule::AllE => {
            let m = lines(1)?;
            let Some((x, body)) = open_quant(get(m[0])?, Quant::All) else { return Err(format!("line {} is not a universal", m[0])) };
            match instance(&body, &x, a) {
                Ok(_) => ok("an instance of a universal"),
                Err(()) => Err(format!("this is not line {} with {x} replaced, everywhere, by one name", m[0])),
            }
        }
        Rule::ExI => {
            let m = lines(1)?;
            let Some((x, body)) = open_quant(a, Quant::Some) else { return Err("∃I makes an existential".into()) };
            let had = get(m[0])?;
            // Some term the cited line has, written as x in some places.
            let terms: Vec<Expr> = had.free().into_iter().map(Expr::Name).collect();
            let fits = terms.iter().any(|t| some_occurrences(&body, &x, t, had)) || same(&body, had);
            if fits {
                ok("something is so")
            } else {
                Err(format!("line {} is not this with a name where {x} is", m[0]))
            }
        }
        Rule::AllI => {
            let m = lines(1)?;
            let Some((x, body)) = open_quant(a, Quant::All) else { return Err("∀I makes a universal".into()) };
            let had = get(m[0])?;
            let c = match instance(&body, &x, had) {
                Ok(Some(Expr::Name(c))) => c,
                Ok(None) => return ok("a universal of what holds whatever x is"),
                _ => return Err(format!("line {} is not this with a name for {x}", m[0])),
            };
            if names(a).contains(&c) {
                return Err(format!("{c} is still in the universal: every {c} must become {x}"));
            }
            if let Some(f) = p.open_assumptions(k).into_iter().find(|f| names(f).contains(&c)) {
                return Err(format!("{c} is in {}, still assumed: ∀I needs a name nothing assumed is about", print::expr(f)));
            }
            ok("what holds of an arbitrary name holds of all")
        }
        Rule::ExE => {
            let (mut m, mut subs) = (Vec::new(), Vec::new());
            for c in &l.cites {
                match c {
                    Cite::Line(n) => m.push(*n),
                    Cite::Range(x, y) => subs.push((*x, *y)),
                }
            }
            if m.len() != 1 || subs.len() != 1 {
                return Err("∃E cites an existential and a subproof: m, i-j".into());
            }
            let ex = get(m[0])?;
            let Some((x, body)) = open_quant(ex, Quant::Some) else { return Err(format!("line {} is not an existential", m[0])) };
            let (s, e, i) = p.sub(k, subs[0].0, subs[0].1)?;
            let c = match instance(&body, &x, s) {
                Ok(Some(Expr::Name(c))) => c,
                _ => return Err(format!("the subproof does not assume line {} with a name for {x}", m[0])),
            };
            if !same(e, a) {
                return Err("the subproof must end with this line's formula".into());
            }
            if names(ex).contains(&c) || names(a).contains(&c) {
                return Err(format!("{c} must be new: it is in the existential or in this line"));
            }
            let outside: Vec<&Expr> = p.open_assumptions(k).into_iter().filter(|f| !std::ptr::eq(*f, &p.lines[i].formula)).collect();
            if let Some(f) = outside.into_iter().find(|f| names(f).contains(&c)) {
                return Err(format!("{c} is in {}, still assumed: ∃E needs a new name", print::expr(f)));
            }
            ok("whatever the something is, this follows")
        }
        Rule::EqI => match a {
            Expr::Bin(Op::Eq, x, y) if x == y => ok("everything is itself"),
            _ => Err("=I gives t = t".into()),
        },
        Rule::EqE => {
            let m = lines(2)?;
            let (x, y) = (get(m[0])?, get(m[1])?);
            let try_eq = |eq: &Expr, f: &Expr| -> bool {
                let Expr::Bin(Op::Eq, s, t) = eq else { return false };
                // f with some s written as t, or t as s, is this line.
                let hole = "\u{0}hole";
                [(&**s, &**t), (&**t, &**s)].iter().any(|(from, to)| {
                    let marked = mark(f, from, hole);
                    marked.iter().any(|b| same(&subst(b, hole, to), a))
                })
            };
            if try_eq(x, y) || try_eq(y, x) {
                ok("equals put for equals")
            } else {
                Err("=E needs a = b and a formula, and gives it with some a written as b".into())
            }
        }
    }
}

/// Every way of marking some of the occurrences of `t` in `f` as `hole`.
fn mark(f: &Expr, t: &Expr, hole: &str) -> Vec<Expr> {
    if f == t {
        return vec![f.clone(), Expr::name(hole)];
    }
    match f {
        Expr::App(g, args) => {
            let mut out = vec![Vec::new()];
            for a in args {
                let opts = mark(a, t, hole);
                out = out.into_iter().flat_map(|row| opts.iter().map(move |o| [row.clone(), vec![o.clone()]].concat())).take(256).collect();
            }
            out.into_iter().map(|xs| Expr::App(g.clone(), xs)).collect()
        }
        Expr::Not(a) => mark(a, t, hole).into_iter().map(Expr::negation).collect(),
        Expr::Bin(op, a, b) => {
            let (xs, ys) = (mark(a, t, hole), mark(b, t, hole));
            xs.iter().flat_map(|x| ys.iter().map(move |y| Expr::bin(*op, x.clone(), y.clone()))).take(256).collect()
        }
        _ => vec![f.clone()],
    }
}

fn de_morgan(x: &Expr, y: &Expr) -> bool {
    let Expr::Not(inner) = x else { return false };
    match (&**inner, y) {
        (Expr::Bin(Op::Or, a, b), Expr::Bin(Op::And, na, nb)) | (Expr::Bin(Op::And, a, b), Expr::Bin(Op::Or, na, nb)) => {
            matches!(&**na, Expr::Not(p) if same(p, a)) && matches!(&**nb, Expr::Not(q) if same(q, b))
        }
        _ => false,
    }
}

fn one_range(l: &PLine) -> Result<(usize, usize), String> {
    match l.cites.as_slice() {
        [Cite::Range(a, b)] => Ok((*a, *b)),
        _ => Err(format!("{} cites one subproof: i-j", rule_name(l.rule))),
    }
}

fn ranges(l: &PLine, n: usize) -> Result<Vec<(usize, usize)>, String> {
    let rs: Vec<(usize, usize)> = l.cites.iter().filter_map(|c| if let Cite::Range(a, b) = c { Some((*a, *b)) } else { None }).collect();
    if rs.len() != n || rs.len() != l.cites.len() {
        return Err(format!("{} cites {n} subproofs", rule_name(l.rule)));
    }
    Ok(rs)
}

/* ── finding a proof ──────────────────────────────────────────────── */

/// A line of a proof being made: its depth, formula, rule and citations.
#[derive(Debug, Clone)]
struct Made {
    depth: usize,
    formula: Expr,
    rule: Rule,
    cites: Vec<Cite>,
}

struct Finder {
    lines: Vec<Made>,
    deadline: Instant,
    steps: u64,
    gave_up: bool,
    /// Goals being worked on, with what was had: a goal asked again with
    /// nothing new is not worked on again.
    path: Vec<(String, usize)>,
    fresh: u32,
    names: BTreeSet<String>,
    /// The lines a case split is being made on, on the way here: not split again.
    split_on: Vec<usize>,
    /// How deep subproofs may nest this round: searched shallow first, so
    /// the proof found is among the shortest.
    limit: usize,
}

/// What is had at a point: each formula and the line (1-based) it is on.
type Had = Vec<(Expr, usize)>;

const MOST_DEPTH: usize = 14;

impl Finder {
    fn push(&mut self, depth: usize, formula: Expr, rule: Rule, cites: Vec<Cite>) -> usize {
        self.lines.push(Made { depth, formula, rule, cites });
        self.lines.len()
    }

    fn find(had: &Had, f: &Expr) -> Option<usize> {
        had.iter().rev().find(|(g, _)| same(g, f)).map(|(_, n)| *n)
    }

    fn new_name(&mut self) -> String {
        loop {
            self.fresh += 1;
            let n = format!("c{}", self.fresh);
            if !self.names.contains(&n) {
                self.names.insert(n.clone());
                return n;
            }
        }
    }

    /// What follows at once from what is had: conjunctions taken apart,
    /// conditionals and biconditionals used whose other side is had, double
    /// negations taken off. Added as lines at `depth`.
    fn saturate(&mut self, had: &mut Had, depth: usize) {
        loop {
            let mut grew = false;
            let snapshot = had.clone();
            for (f, n) in &snapshot {
                let mut add = |me: &mut Finder, had: &mut Had, g: Expr, rule: Rule, cites: Vec<Cite>| {
                    if Finder::find(had, &g).is_none() && had.len() < 400 {
                        let l = me.push(depth, g.clone(), rule, cites);
                        had.push((g, l));
                        grew = true;
                    }
                };
                match f {
                    Expr::Bin(Op::And, a, b) => {
                        add(self, had, (**a).clone(), Rule::AndE, vec![Cite::Line(*n)]);
                        add(self, had, (**b).clone(), Rule::AndE, vec![Cite::Line(*n)]);
                    }
                    Expr::Bin(Op::Imp, a, b) => {
                        if let Some(m) = Finder::find(had, a) {
                            add(self, had, (**b).clone(), Rule::ImpE, vec![Cite::Line(*n), Cite::Line(m)]);
                        }
                    }
                    Expr::Bin(Op::Iff, a, b) => {
                        if let Some(m) = Finder::find(had, a) {
                            add(self, had, (**b).clone(), Rule::IffE, vec![Cite::Line(*n), Cite::Line(m)]);
                        }
                        if let Some(m) = Finder::find(had, b) {
                            add(self, had, (**a).clone(), Rule::IffE, vec![Cite::Line(*n), Cite::Line(m)]);
                        }
                    }
                    Expr::Not(a) => {
                        if let Expr::Not(b) = &**a {
                            add(self, had, (**b).clone(), Rule::DNE, vec![Cite::Line(*n)]);
                        }
                    }
                    _ => {}
                }
            }
            if !grew {
                return;
            }
        }
    }

    /// A line with `goal` on it at `depth`, proved from `had`; the lines it
    /// takes are added, and taken away again if it fails.
    fn prove(&mut self, had: &Had, goal: &Expr, depth: usize) -> Option<usize> {
        self.steps += 1;
        if self.steps % 256 == 0 && Instant::now() >= self.deadline {
            self.gave_up = true;
        }
        if self.gave_up || depth > self.limit {
            return None;
        }
        let key = (print::expr(goal), had.len());
        if self.path.contains(&key) {
            return None;
        }
        self.path.push(key);
        let r = self.prove_here(had, goal, depth);
        self.path.pop();
        r
    }

    fn prove_here(&mut self, had: &Had, goal: &Expr, depth: usize) -> Option<usize> {
        let mark = self.lines.len();
        let mut had = had.clone();
        self.saturate(&mut had, depth);
        // What saturating added stays while what is tried below comes and goes.
        let saturated = self.lines.len();
        if let Some(n) = Finder::find(&had, goal) {
            return Some(n);
        }
        if let Some(n) = had.iter().find(|(f, _)| is_false(f)).map(|(_, n)| *n) {
            return Some(self.push(depth, goal.clone(), Rule::X, vec![Cite::Line(n)]));
        }
        // A contradiction among what is had, for ⊥.
        if is_false(goal) {
            for (f, n) in &had {
                if let Expr::Not(a) = f {
                    if let Some(m) = Finder::find(&had, a) {
                        return Some(self.push(depth, goal.clone(), Rule::NotE, vec![Cite::Line(m), Cite::Line(*n)]));
                    }
                }
            }
        }
        // A disjunction to prove, with one had to split on: the split first.
        let splittable = had.iter().any(|(f, n)| matches!(f, Expr::Bin(Op::Or, ..) | Expr::Quant(Quant::Some, ..)) && !self.split_on.contains(n));
        if matches!(goal, Expr::Bin(Op::Or, ..)) && splittable {
            if let Some(n) = self.cases(&had, goal, depth) {
                return Some(n);
            }
            self.lines.truncate(saturated);
        }
        // Back from the goal by its introduction rule.
        let tried = match goal {
            Expr::Bin(Op::Imp, a, b) => self.subproof(&had, a, b, depth).map(|(i, j)| self.push(depth, goal.clone(), Rule::ImpI, vec![Cite::Range(i, j)])),
            Expr::Bin(Op::And, a, b) => {
                let x = self.prove(&had, a, depth);
                let y = x.and_then(|_| self.prove(&had, b, depth));
                match (x, y) {
                    (Some(x), Some(y)) => Some(self.push(depth, goal.clone(), Rule::AndI, vec![Cite::Line(x), Cite::Line(y)])),
                    _ => None,
                }
            }
            Expr::Bin(Op::Iff, a, b) => {
                let one = self.subproof(&had, a, b, depth);
                let two = one.and_then(|_| self.subproof(&had, b, a, depth));
                match (one, two) {
                    (Some((i, j)), Some((k, l))) => Some(self.push(depth, goal.clone(), Rule::IffI, vec![Cite::Range(i, j), Cite::Range(k, l)])),
                    _ => None,
                }
            }
            Expr::Not(a) => self.subproof(&had, a, &Expr::Bool(false), depth).map(|(i, j)| self.push(depth, goal.clone(), Rule::NotI, vec![Cite::Range(i, j)])),
            Expr::Bin(Op::Or, a, b) => {
                let m = self.lines.len();
                match self.prove(&had, a, depth) {
                    Some(x) => Some(self.push(depth, goal.clone(), Rule::OrI, vec![Cite::Line(x)])),
                    None => {
                        self.lines.truncate(m);
                        self.prove(&had, b, depth).map(|y| self.push(depth, goal.clone(), Rule::OrI, vec![Cite::Line(y)]))
                    }
                }
            }
            Expr::Quant(Quant::All, ..) => {
                let (x, body) = open_quant(goal, Quant::All).unwrap();
                let c = self.new_name();
                self.prove(&had, &subst(&body, &x, &Expr::name(&c)), depth).map(|n| self.push(depth, goal.clone(), Rule::AllI, vec![Cite::Line(n)]))
            }
            Expr::Quant(Quant::Some, ..) => {
                let (x, body) = open_quant(goal, Quant::Some).unwrap();
                let mut terms: Vec<String> = self.names.iter().cloned().collect();
                terms.retain(|t| had.iter().any(|(f, _)| f.free().contains(t)));
                let mut found = None;
                for t in terms {
                    let m = self.lines.len();
                    if let Some(n) = self.prove(&had, &subst(&body, &x, &Expr::name(&t)), depth) {
                        found = Some(self.push(depth, goal.clone(), Rule::ExI, vec![Cite::Line(n)]));
                        break;
                    }
                    self.lines.truncate(m);
                }
                found
            }
            _ => None,
        };
        if tried.is_some() {
            return tried;
        }
        self.lines.truncate(saturated);
        // Forward: cases on a disjunction or an existential had.
        if !matches!(goal, Expr::Bin(Op::Or, ..)) {
            if let Some(n) = self.cases(&had, goal, depth) {
                return Some(n);
            }
            self.lines.truncate(saturated);
        }
        // Forward: universals instantiated with the names about.
        let terms: Vec<String> = {
            let mut t: Vec<String> = had.iter().flat_map(|(f, _)| f.free()).filter(|n| self.names.contains(n)).collect();
            t.extend(goal.free().into_iter().filter(|n| self.names.contains(n)));
            t.sort();
            t.dedup();
            t
        };
        let mut grew = had.clone();
        for (f, n) in had.iter() {
            if let Some((x, body)) = open_quant(f, Quant::All) {
                for t in &terms {
                    let inst = subst(&body, &x, &Expr::name(t));
                    if Finder::find(&grew, &inst).is_none() {
                        let l = self.push(depth, inst.clone(), Rule::AllE, vec![Cite::Line(*n)]);
                        grew.push((inst, l));
                    }
                }
            }
        }
        if grew.len() > had.len() {
            if let Some(n) = self.prove(&grew, goal, depth) {
                return Some(n);
            }
            self.lines.truncate(saturated);
        }
        // For ⊥: a conditional's antecedent proved, or a negation's body.
        if is_false(goal) {
            for (f, n) in had.clone() {
                let m = self.lines.len();
                match &f {
                    Expr::Not(a) if !matches!(**a, Expr::Bool(false)) => {
                        if let Some(x) = self.prove(&had, a, depth) {
                            return Some(self.push(depth, goal.clone(), Rule::NotE, vec![Cite::Line(x), Cite::Line(n)]));
                        }
                    }
                    Expr::Bin(Op::Imp, a, b) if Finder::find(&had, b).is_none() => {
                        if let Some(x) = self.prove(&had, a, depth) {
                            let y = self.push(depth, (**b).clone(), Rule::ImpE, vec![Cite::Line(n), Cite::Line(x)]);
                            let mut more = had.clone();
                            more.push(((**b).clone(), y));
                            if let Some(z) = self.prove(&more, goal, depth) {
                                return Some(z);
                            }
                        }
                    }
                    _ => {}
                }
                self.lines.truncate(m);
            }
            return None;
        }
        // Last: indirect proof.
        let denied = Expr::negation(goal.clone());
        if Finder::find(&had, &denied).is_none() {
            if let Some((i, j)) = self.subproof(&had, &denied, &Expr::Bool(false), depth) {
                return Some(self.push(depth, goal.clone(), Rule::IP, vec![Cite::Range(i, j)]));
            }
        }
        self.lines.truncate(mark);
        None
    }

    /// The goal by cases on a disjunction or an existential had, each split
    /// once on the way.
    fn cases(&mut self, had: &Had, goal: &Expr, depth: usize) -> Option<usize> {
        for (f, n) in had.clone() {
            if self.split_on.contains(&n) {
                continue;
            }
            let m = self.lines.len();
            self.split_on.push(n);
            let made = match &f {
                Expr::Bin(Op::Or, a, b) => {
                    let one = self.subproof(had, a, goal, depth);
                    let two = one.and_then(|_| self.subproof(had, b, goal, depth));
                    match (one, two) {
                        (Some((i, j)), Some((k, l))) => Some(self.push(depth, goal.clone(), Rule::OrE, vec![Cite::Line(n), Cite::Range(i, j), Cite::Range(k, l)])),
                        _ => None,
                    }
                }
                Expr::Quant(Quant::Some, ..) => {
                    let (x, body) = open_quant(&f, Quant::Some).unwrap();
                    let c = self.new_name();
                    self.subproof(had, &subst(&body, &x, &Expr::name(&c)), goal, depth).map(|(i, j)| self.push(depth, goal.clone(), Rule::ExE, vec![Cite::Line(n), Cite::Range(i, j)]))
                }
                _ => None,
            };
            self.split_on.pop();
            if made.is_some() {
                return made;
            }
            self.lines.truncate(m);
        }
        None
    }

    /// A subproof assuming `assume` and ending with `end`: its first and
    /// last lines.
    fn subproof(&mut self, had: &Had, assume: &Expr, end: &Expr, depth: usize) -> Option<(usize, usize)> {
        let mark = self.lines.len();
        let i = self.push(depth + 1, assume.clone(), Rule::Assume, Vec::new());
        let mut inner = had.clone();
        // Assuming what is had already adds nothing, and so the goal's being
        // on the path again is seen as the loop it is.
        if Finder::find(had, assume).is_none() {
            inner.push((assume.clone(), i));
        }
        match self.prove(&inner, end, depth + 1) {
            Some(j) => {
                // A subproof ends inside itself: what was had outside is repeated.
                let j = if j < i { self.push(depth + 1, end.clone(), Rule::R, vec![Cite::Line(j)]) } else { j };
                Some((i, j))
            }
            None => {
                self.lines.truncate(mark);
                None
            }
        }
    }
}

/// A proof of `goal` from `premises`, found, as a `.proof` file's text
/// (with its header); or why none was.
pub fn find(premises: &[Expr], goal: &Expr, time: Duration) -> Result<String, String> {
    let mut names = BTreeSet::new();
    for f in premises.iter().chain(std::iter::once(goal)) {
        names.extend(f.free());
    }
    let mut f = Finder { lines: Vec::new(), deadline: Instant::now() + time, steps: 0, gave_up: false, path: Vec::new(), fresh: 0, names, split_on: Vec::new(), limit: 0 };
    let mut had: Had = Vec::new();
    for p in premises {
        let n = f.push(0, p.clone(), Rule::Premise, Vec::new());
        had.push((p.clone(), n));
    }
    let start = f.lines.len();
    let mut found = None;
    for limit in [2, 4, 6, 9, MOST_DEPTH] {
        f.limit = limit;
        f.lines.truncate(start);
        found = f.prove(&had, goal, 0);
        if found.is_some() || f.gave_up {
            break;
        }
    }
    let Some(last) = found else {
        return Err(if f.gave_up { "no proof found in the time".into() } else { "no proof found within the search's bounds".into() });
    };
    // A goal that was a premise, or found below: the last line is the goal.
    if last != f.lines.len() || f.lines[last - 1].depth != 0 {
        f.push(0, goal.clone(), Rule::R, vec![Cite::Line(last)]);
    }
    let text = write(premises, goal, &tidy(f.lines));
    // What it found is checked as any proof is.
    let checked = check(&read(&text));
    if !checked.complete {
        let bad = checked.lines.iter().find(|c| !c.ok).map(|c| format!("line {}: {}", c.n, c.message)).unwrap_or(checked.summary);
        return Err(format!("the proof found did not check ({bad}): a fault in the finder"));
    }
    Ok(text)
}

/// Lines nothing later uses, left out, and the rest numbered again.
fn tidy(lines: Vec<Made>) -> Vec<Made> {
    let n = lines.len();
    let mut used = vec![false; n];
    used[n - 1] = true;
    for k in (0..n).rev() {
        if !used[k] {
            continue;
        }
        for c in &lines[k].cites {
            match c {
                Cite::Line(m) => used[m - 1] = true,
                Cite::Range(a, b) => {
                    // A subproof used keeps its assumption, its end, and what they use.
                    used[a - 1] = true;
                    used[b - 1] = true;
                }
            }
        }
    }
    // Premises stay, as the header says them.
    for (k, l) in lines.iter().enumerate() {
        if l.rule == Rule::Premise {
            used[k] = true;
        }
    }
    let mut number = vec![0usize; n];
    let mut out = Vec::new();
    for (k, l) in lines.iter().enumerate() {
        if used[k] {
            out.push(l.clone());
            number[k] = out.len();
        }
    }
    for l in &mut out {
        for c in &mut l.cites {
            *c = match *c {
                Cite::Line(m) => Cite::Line(number[m - 1]),
                Cite::Range(a, b) => Cite::Range(number[a - 1], number[b - 1]),
            };
        }
    }
    out
}

/// A proof written as a `.proof` file: the header, then each line, its
/// rule lined up after the longest formula.
fn write(premises: &[Expr], goal: &Expr, lines: &[Made]) -> String {
    let mut out = String::from("system fitch\n");
    out.push_str(&format!("premises {}\n", premises.iter().map(print::expr).collect::<Vec<_>>().join(", ")));
    out.push_str(&format!("goal {}\n\n", print::expr(goal)));
    let body: Vec<String> = lines.iter().map(|l| format!("{}{}", "| ".repeat(l.depth), print::expr(&l.formula))).collect();
    let width = body.iter().map(|b| b.chars().count()).max().unwrap_or(0);
    let digits = lines.len().to_string().len();
    for (k, (l, b)) in lines.iter().zip(&body).enumerate() {
        let cites: Vec<String> = l.cites.iter().map(|c| match c {
            Cite::Line(m) => m.to_string(),
            Cite::Range(a, b) => format!("{a}-{b}"),
        }).collect();
        let just = if cites.is_empty() { rule_name(l.rule).to_string() } else { format!("{} {}", rule_name(l.rule), cites.join(", ")) };
        out.push_str(&format!("{:>digits$}  {b:<width$}  {just}\n", k + 1));
    }
    out
}
