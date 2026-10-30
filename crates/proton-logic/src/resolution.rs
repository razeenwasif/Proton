//! Resolution: an argument's premises and its goal denied put into clause
//! form, step by step, and the empty clause derived — or a `.proof` file of
//! clauses checked line by line.
//!
//! ```text
//! system resolution
//! premises all x: man(x) -> mortal(x), man(socrates)
//! goal mortal(socrates)
//!
//! 1  not man(x) or mortal(x)   premise 1
//! 2  man(socrates)             premise 2
//! 3  not mortal(socrates)      goal
//! 4  mortal(socrates)          resolve 1, 2
//! 5  false                     resolve 3, 4
//! ```
//!
//! - **Clause form:** `→`, `↔` and `xor` written with `and`, `or` and
//!   `not`; negations moved in to the atoms; each quantifier given its own
//!   variable and moved to the front; existentials replaced by Skolem
//!   functions `sk1`, `sk2`, … numbered through the premises and then the
//!   goal; the rest multiplied out into clauses.
//! - **In a clause** a name is a variable when it is `u` to `z`, with digits
//!   or primes after, and the header does not use it as a name: `x`, `y1`,
//!   `z'`. `false` is the empty clause.
//! - **Lines:** `premise k` and `goal` for a clause of the clause form — or
//!   an instance of one, or one with more beside it; `resolve i, j` for a
//!   resolvent of lines `i` and `j`, its variables unified, likewise; and
//!   `factor i` for an instance of `i` whose like literals merge. A proof is
//!   complete with the empty clause.
//! - **Found** by saturation, the lightest clause first, with factoring,
//!   tautologies dropped and clauses subsumed by one already used set aside.
//!   It always ends for propositional logic: with the empty clause, or with
//!   no clause left to add, and then the argument does not hold.
//! - `=` and `<` are predicates like any other here, as in the tableau.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashSet};
use std::time::{Duration, Instant};

use crate::ast::*;
use crate::fitch::subst;
use crate::parse::{self, Diag};
use crate::print;
use crate::proofs::{self, Header, LineReport, Report};

/// A formula's clauses, at most; and clauses kept in a search.
pub const MOST_CLAUSES: usize = 256;
pub const MOST_KEPT: usize = 40_000;

/* ── terms, literals, clauses ─────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Tm {
    V(u32),
    F(String, Vec<Tm>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Lit {
    pos: bool,
    pred: String,
    args: Vec<Tm>,
}

type Clause = Vec<Lit>;

/// Whether a name is a variable's: `u` to `z`, then digits, primes or `_`.
pub fn variable_like(n: &str) -> bool {
    let mut c = n.chars();
    matches!(c.next(), Some('u'..='z')) && c.all(|c| c.is_ascii_digit() || c == '\'' || c == '_')
}

/// Variable names, as many as wanted, none of them taken.
fn var_names(n: usize, taken: &BTreeSet<String>) -> Vec<String> {
    (0..).flat_map(|i| ["x", "y", "z", "u", "v", "w"].map(move |b| if i == 0 { b.to_string() } else { format!("{b}{i}") })).filter(|v| !taken.contains(v)).take(n).collect()
}

fn to_tm(e: &Expr, vars: &mut Vec<String>, is_var: &dyn Fn(&str) -> bool) -> Result<Tm, String> {
    Ok(match e {
        Expr::Name(n) if is_var(n) => match vars.iter().position(|v| v == n) {
            Some(i) => Tm::V(i as u32),
            None => {
                vars.push(n.clone());
                Tm::V(vars.len() as u32 - 1)
            }
        },
        Expr::Name(n) => Tm::F(n.clone(), Vec::new()),
        Expr::Num(k) => Tm::F(k.to_string(), Vec::new()),
        Expr::App(f, args) => Tm::F(f.clone(), args.iter().map(|a| to_tm(a, vars, is_var)).collect::<Result<_, _>>()?),
        Expr::Neg(a) => Tm::F("-".into(), vec![to_tm(a, vars, is_var)?]),
        Expr::Bin(op, a, b) if op.level() >= 7 => Tm::F(op.text().into(), vec![to_tm(a, vars, is_var)?, to_tm(b, vars, is_var)?]),
        other => return Err(format!("“{}” is not a term", print::expr(other))),
    })
}

fn to_lit(e: &Expr, vars: &mut Vec<String>, is_var: &dyn Fn(&str) -> bool) -> Result<Lit, String> {
    let tms = |args: &[&Expr], vars: &mut Vec<String>| args.iter().map(|a| to_tm(a, vars, is_var)).collect::<Result<Vec<_>, _>>();
    Ok(match e {
        Expr::Not(a) => {
            let l = to_lit(a, vars, is_var)?;
            Lit { pos: !l.pos, ..l }
        }
        Expr::Name(p) => Lit { pos: true, pred: p.clone(), args: Vec::new() },
        Expr::App(p, args) => Lit { pos: true, pred: p.clone(), args: tms(&args.iter().collect::<Vec<_>>(), vars)? },
        Expr::Bin(Op::Ne, a, b) => Lit { pos: false, pred: "=".into(), args: tms(&[a, b], vars)? },
        Expr::Bin(op, a, b) if op.level() == 6 => Lit { pos: true, pred: op.text().into(), args: tms(&[a, b], vars)? },
        other => return Err(format!("“{}” is not a literal: a clause is literals joined by or", print::expr(other))),
    })
}

fn tm_expr(t: &Tm, names: &[String]) -> Expr {
    match t {
        Tm::V(i) => Expr::Name(names[*i as usize].clone()),
        Tm::F(f, args) if args.is_empty() => f.parse::<i64>().map(Expr::Num).unwrap_or_else(|_| Expr::name(f)),
        Tm::F(f, args) if args.len() == 1 && f == "-" => Expr::Neg(Box::new(tm_expr(&args[0], names))),
        Tm::F(f, args) if args.len() == 2 && ["+", "-", "*"].contains(&f.as_str()) => {
            let op = [Op::Add, Op::Sub, Op::Mul][["+", "-", "*"].iter().position(|o| o == f).unwrap()];
            Expr::bin(op, tm_expr(&args[0], names), tm_expr(&args[1], names))
        }
        Tm::F(f, args) => Expr::App(f.clone(), args.iter().map(|a| tm_expr(a, names)).collect()),
    }
}

const COMPARISONS: [(&str, Op); 5] = [("=", Op::Eq), ("<", Op::Lt), ("<=", Op::Le), (">", Op::Gt), (">=", Op::Ge)];

fn lit_expr(l: &Lit, names: &[String]) -> Expr {
    let atom = match COMPARISONS.iter().find(|(p, _)| *p == l.pred) {
        Some((_, op)) if l.args.len() == 2 => Expr::bin(*op, tm_expr(&l.args[0], names), tm_expr(&l.args[1], names)),
        _ if l.args.is_empty() => Expr::name(&l.pred),
        _ => Expr::App(l.pred.clone(), l.args.iter().map(|a| tm_expr(a, names)).collect()),
    };
    if l.pos {
        atom
    } else {
        Expr::negation(atom)
    }
}

fn vars_in(c: &Clause) -> usize {
    fn top(t: &Tm) -> usize {
        match t {
            Tm::V(i) => *i as usize + 1,
            Tm::F(_, args) => args.iter().map(top).max().unwrap_or(0),
        }
    }
    c.iter().flat_map(|l| l.args.iter().map(top)).max().unwrap_or(0)
}

fn disjunction(mut lits: Vec<Expr>) -> Expr {
    if lits.is_empty() {
        return Expr::Bool(false);
    }
    let first = lits.remove(0);
    lits.into_iter().fold(first, |a, b| Expr::bin(Op::Or, a, b))
}

/// A clause written in Proton's language, its variables named apart from `consts`.
fn clause_text(c: &Clause, consts: &BTreeSet<String>) -> String {
    let names = var_names(vars_in(c), consts);
    print::expr(&disjunction(c.iter().map(|l| lit_expr(l, &names)).collect()))
}

/// Its literals sorted and once each, its variables numbered as they come;
/// none for a tautology.
fn norm(mut c: Clause) -> Option<Clause> {
    fn renumber(c: &mut Clause) {
        fn go(t: &mut Tm, map: &mut Vec<u32>) {
            match t {
                Tm::V(i) => {
                    let k = match map.iter().position(|v| v == i) {
                        Some(k) => k,
                        None => {
                            map.push(*i);
                            map.len() - 1
                        }
                    };
                    *t = Tm::V(k as u32 + 1_000_000);
                }
                Tm::F(_, args) => args.iter_mut().for_each(|a| go(a, map)),
            }
        }
        fn back(t: &mut Tm) {
            match t {
                Tm::V(i) => *i -= 1_000_000,
                Tm::F(_, args) => args.iter_mut().for_each(back),
            }
        }
        let mut map = Vec::new();
        c.iter_mut().flat_map(|l| l.args.iter_mut()).for_each(|t| go(t, &mut map));
        c.iter_mut().flat_map(|l| l.args.iter_mut()).for_each(back);
    }
    c.sort();
    c.dedup();
    if c.iter().any(|l| l.pos && c.iter().any(|m| !m.pos && m.pred == l.pred && m.args == l.args)) {
        return None;
    }
    for _ in 0..2 {
        renumber(&mut c);
        c.sort();
        c.dedup();
    }
    Some(c)
}

/* ── unification and matching ─────────────────────────────────────── */

type Sub = Vec<Option<Tm>>;

fn walk(t: &Tm, s: &Sub) -> Tm {
    match t {
        Tm::V(v) => match s.get(*v as usize).and_then(|x| x.as_ref()) {
            Some(u) => walk(u, s),
            None => t.clone(),
        },
        _ => t.clone(),
    }
}

fn occurs(v: u32, t: &Tm, s: &Sub) -> bool {
    match walk(t, s) {
        Tm::V(w) => w == v,
        Tm::F(_, args) => args.iter().any(|a| occurs(v, a, s)),
    }
}

fn bind(s: &mut Sub, v: u32, t: Tm) {
    if s.len() <= v as usize {
        s.resize(v as usize + 1, None);
    }
    s[v as usize] = Some(t);
}

fn unify(a: &Tm, b: &Tm, s: &mut Sub) -> bool {
    match (walk(a, s), walk(b, s)) {
        (Tm::V(x), Tm::V(y)) if x == y => true,
        (Tm::V(x), t) | (t, Tm::V(x)) => {
            if occurs(x, &t, s) {
                return false;
            }
            bind(s, x, t);
            true
        }
        (Tm::F(f, xs), Tm::F(g, ys)) => f == g && xs.len() == ys.len() && xs.iter().zip(&ys).all(|(x, y)| unify(x, y, s)),
    }
}

fn apply(t: &Tm, s: &Sub) -> Tm {
    match walk(t, s) {
        Tm::F(f, args) => Tm::F(f, args.iter().map(|a| apply(a, s)).collect()),
        v => v,
    }
}

fn apply_clause(c: &[Lit], s: &Sub) -> Clause {
    c.iter().map(|l| Lit { pos: l.pos, pred: l.pred.clone(), args: l.args.iter().map(|a| apply(a, s)).collect() }).collect()
}

fn shift(c: &Clause, by: u32) -> Clause {
    fn go(t: &Tm, by: u32) -> Tm {
        match t {
            Tm::V(i) => Tm::V(i + by),
            Tm::F(f, args) => Tm::F(f.clone(), args.iter().map(|a| go(a, by)).collect()),
        }
    }
    c.iter().map(|l| Lit { pos: l.pos, pred: l.pred.clone(), args: l.args.iter().map(|a| go(a, by)).collect() }).collect()
}

/// A clause's variables made constants, to be matched against and never bound.
fn freeze(c: &Clause) -> Clause {
    fn go(t: &Tm) -> Tm {
        match t {
            Tm::V(i) => Tm::F(format!("?{i}"), Vec::new()),
            Tm::F(f, args) => Tm::F(f.clone(), args.iter().map(go).collect()),
        }
    }
    c.iter().map(|l| Lit { pos: l.pos, pred: l.pred.clone(), args: l.args.iter().map(go).collect() }).collect()
}

/// `p` made `t` by binding only `p`'s variables.
fn matches(p: &Tm, t: &Tm, s: &mut Sub) -> bool {
    match p {
        Tm::V(v) => match s.get(*v as usize).and_then(|x| x.clone()) {
            Some(u) => u == *t,
            None => {
                bind(s, *v, t.clone());
                true
            }
        },
        Tm::F(f, ps) => matches!(t, Tm::F(g, ts) if f == g && ps.len() == ts.len() && ps.iter().zip(ts).all(|(p, t)| matches(p, t, s))),
    }
}

/// Whether some instance of `c` is among `d`'s literals (`d` frozen).
fn subsumes(c: &[Lit], d: &[Lit], s: &mut Sub) -> bool {
    let Some((l, rest)) = c.split_first() else { return true };
    d.iter().any(|m| {
        if m.pos != l.pos || m.pred != l.pred || m.args.len() != l.args.len() {
            return false;
        }
        let mut t = s.clone();
        l.args.iter().zip(&m.args).all(|(p, a)| matches(p, a, &mut t)) && subsumes(rest, d, &mut t)
    })
}

/// Every binary resolvent of two clauses, their variables kept apart.
fn resolvents(a: &Clause, b: &Clause) -> Vec<Clause> {
    let off = vars_in(a) as u32;
    let b = shift(b, off);
    let mut out = Vec::new();
    for (i, l) in a.iter().enumerate() {
        for (j, m) in b.iter().enumerate() {
            if l.pos == m.pos || l.pred != m.pred || l.args.len() != m.args.len() {
                continue;
            }
            let mut s = Sub::new();
            if l.args.iter().zip(&m.args).all(|(x, y)| unify(x, y, &mut s)) {
                let rest: Clause = a.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, x)| x.clone()).chain(b.iter().enumerate().filter(|(k, _)| *k != j).map(|(_, x)| x.clone())).collect();
                if let Some(c) = norm(apply_clause(&rest, &s)) {
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
        }
    }
    out
}

fn factors(c: &Clause) -> Vec<Clause> {
    let mut out = Vec::new();
    for i in 0..c.len() {
        for j in i + 1..c.len() {
            let (l, m) = (&c[i], &c[j]);
            if l.pos != m.pos || l.pred != m.pred || l.args.len() != m.args.len() {
                continue;
            }
            let mut s = Sub::new();
            if l.args.iter().zip(&m.args).all(|(x, y)| unify(x, y, &mut s)) {
                if let Some(f) = norm(apply_clause(c, &s)) {
                    if f.len() < c.len() && !out.contains(&f) {
                        out.push(f);
                    }
                }
            }
        }
    }
    out
}

/* ── clause form ──────────────────────────────────────────────────── */

/// A formula's clause form: what it was, each step's result in words, and
/// its clauses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// `premise 1`, or `the goal denied`.
    pub of: String,
    pub formula: String,
    pub steps: Vec<(&'static str, String)>,
    clauses: Vec<Clause>,
}

fn formula_like(e: &Expr) -> bool {
    matches!(e, Expr::Not(_) | Expr::Quant(..) | Expr::Bool(_)) || matches!(e, Expr::Bin(op, ..) if op.is_logical())
}

/// `→`, `↔` and `xor` (and `=` and `!=` between formulas) in `and`, `or`, `not`.
fn elim(e: &Expr) -> Expr {
    let not = Expr::negation;
    let or = |a, b| Expr::bin(Op::Or, a, b);
    let and = |a, b| Expr::bin(Op::And, a, b);
    match e {
        Expr::Not(a) => not(elim(a)),
        Expr::Bin(op, a, b) => {
            let (a2, b2) = (elim(a), elim(b));
            let between_formulas = formula_like(a) || formula_like(b);
            match op {
                Op::And | Op::Or => Expr::bin(*op, a2, b2),
                Op::Imp => or(not(a2), b2),
                Op::Iff => and(or(not(a2.clone()), b2.clone()), or(a2, not(b2))),
                Op::Eq if between_formulas => and(or(not(a2.clone()), b2.clone()), or(a2, not(b2))),
                Op::Xor => and(or(a2.clone(), b2.clone()), or(not(a2), not(b2))),
                Op::Ne if between_formulas => and(or(a2.clone(), b2.clone()), or(not(a2), not(b2))),
                _ => e.clone(),
            }
        }
        Expr::Quant(q, bs, a) => Expr::Quant(*q, bs.clone(), Box::new(elim(a))),
        _ => e.clone(),
    }
}

/// Negations moved in to the atoms (`neg`: the whole is negated).
fn nnf(e: &Expr, neg: bool) -> Result<Expr, String> {
    Ok(match e {
        Expr::Not(a) => nnf(a, !neg)?,
        Expr::Bin(op @ (Op::And | Op::Or), a, b) => {
            let op = if !neg { *op } else if *op == Op::And { Op::Or } else { Op::And };
            Expr::bin(op, nnf(a, neg)?, nnf(b, neg)?)
        }
        Expr::Quant(q @ (Quant::All | Quant::Some), bs, a) => {
            let q = if !neg { *q } else if *q == Quant::All { Quant::Some } else { Quant::All };
            Expr::Quant(q, bs.clone(), Box::new(nnf(a, neg)?))
        }
        Expr::Quant(..) => return Err("counting quantifiers are the model finder's, not resolution's".into()),
        Expr::Bool(b) => Expr::Bool(*b != neg),
        atom if neg => Expr::negation(atom.clone()),
        atom => atom.clone(),
    })
}

/// Each quantifier given a variable of its own, none of them `taken`: its
/// own name when that is a variable's and free.
fn rename(e: &Expr, taken: &mut BTreeSet<String>) -> Expr {
    match e {
        Expr::Quant(q, bs, a) => {
            let mut body = (**a).clone();
            let mut out = Vec::new();
            for b in bs {
                let v = if variable_like(&b.name) && !taken.contains(&b.name) { b.name.clone() } else { var_names(1, taken).remove(0) };
                taken.insert(v.clone());
                body = subst(&body, &b.name, &Expr::Name(v.clone()));
                out.push(Binder { name: v, sort: None });
            }
            Expr::Quant(*q, out, Box::new(rename(&body, taken)))
        }
        Expr::Not(a) => Expr::negation(rename(a, taken)),
        Expr::Bin(op, a, b) if op.is_logical() => Expr::bin(*op, rename(a, taken), rename(b, taken)),
        _ => e.clone(),
    }
}

fn prenex(e: &Expr) -> (Vec<(Quant, String)>, Expr) {
    match e {
        Expr::Quant(q, bs, a) => {
            let (mut rest, m) = prenex(a);
            let mut out: Vec<(Quant, String)> = bs.iter().map(|b| (*q, b.name.clone())).collect();
            out.append(&mut rest);
            (out, m)
        }
        Expr::Bin(op, a, b) if op.is_logical() => {
            let (mut pa, ma) = prenex(a);
            let (mut pb, mb) = prenex(b);
            pa.append(&mut pb);
            (pa, Expr::bin(*op, ma, mb))
        }
        _ => (Vec::new(), e.clone()),
    }
}

fn prefixed(prefix: &[(Quant, String)], m: Expr) -> Expr {
    prefix.iter().rev().fold(m, |m, (q, v)| Expr::Quant(*q, vec![Binder { name: v.clone(), sort: None }], Box::new(m)))
}

fn cnf(e: &Expr) -> Result<Vec<Vec<Expr>>, String> {
    Ok(match e {
        Expr::Bin(Op::And, a, b) => tidy([cnf(a)?, cnf(b)?].concat()),
        Expr::Bin(Op::Or, a, b) => {
            let (ca, cb) = (cnf(a)?, cnf(b)?);
            // Distributed, then tidied: what is too many is what is left.
            if ca.len() * cb.len() > MOST_CLAUSES * MOST_CLAUSES {
                return Err(format!("more than {MOST_CLAUSES} clauses: too many to write out"));
            }
            let c = tidy(ca.iter().flat_map(|x| cb.iter().map(move |y| [x.clone(), y.clone()].concat())));
            if c.len() > MOST_CLAUSES {
                return Err(format!("more than {MOST_CLAUSES} clauses: too many to write out"));
            }
            c
        }
        Expr::Bool(true) => Vec::new(),
        Expr::Bool(false) => vec![Vec::new()],
        lit => vec![vec![lit.clone()]],
    })
}

/// Clauses as distributing leaves them, tidied: each literal once, a
/// clause with a literal and its negation dropped (it always holds), and a
/// clause said before dropped. Without it nested biconditionals multiply
/// past any count worth writing out — three propositions have 27 clauses
/// at most, and Pelletier 12's distribute to thousands.
fn tidy(clauses: impl IntoIterator<Item = Vec<Expr>>) -> Vec<Vec<Expr>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for c in clauses {
        let mut lits: Vec<Expr> = Vec::new();
        for l in c {
            if !lits.contains(&l) {
                lits.push(l);
            }
        }
        let denied = |l: &Expr| match l {
            Expr::Not(a) => (**a).clone(),
            l => Expr::negation(l.clone()),
        };
        if lits.iter().any(|l| lits.contains(&denied(l))) {
            continue;
        }
        let mut key: Vec<String> = lits.iter().map(print::expr).collect();
        key.sort();
        if seen.insert(key) {
            out.push(lits);
        }
    }
    out
}

/// Every name anywhere in the argument: a variable or a Skolem function is none of them.
fn symbols(fs: &[&Expr]) -> BTreeSet<String> {
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
    fs.iter().for_each(|e| go(e, &mut out));
    out
}

fn bound_names(fs: &[&Expr]) -> BTreeSet<String> {
    fn go(e: &Expr, out: &mut BTreeSet<String>) {
        match e {
            Expr::Quant(_, bs, a) => {
                out.extend(bs.iter().map(|b| b.name.clone()));
                go(a, out);
            }
            Expr::Not(a) => go(a, out),
            Expr::Bin(_, a, b) => {
                go(a, out);
                go(b, out);
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    fs.iter().for_each(|e| go(e, &mut out));
    out
}

/// The names the header uses as names: in a clause these are never variables.
fn constants(premises: &[Expr], goal: Option<&Expr>) -> BTreeSet<String> {
    premises.iter().chain(goal).flat_map(|e| e.free()).collect()
}

/// The clause form of each premise and of the goal denied: the same each
/// time for the same header, so a file's `premise k` lines are checked
/// against what the finder writes.
pub fn forms(premises: &[Expr], goal: Option<&Expr>) -> Result<Vec<Form>, String> {
    let all: Vec<&Expr> = premises.iter().chain(goal).collect();
    let consts = constants(premises, goal);
    // Names that are not bound: the variables are named apart from them.
    let bound = bound_names(&all);
    let mut taken: BTreeSet<String> = symbols(&all).into_iter().filter(|n| !bound.contains(n) || consts.contains(n)).collect();
    let mut sk = 0;
    let mut out = Vec::new();
    let denied = goal.map(|g| Expr::negation(g.clone()));
    for (k, f) in premises.iter().chain(denied.as_ref()).enumerate() {
        let of = if k < premises.len() { format!("premise {}", k + 1) } else { "the goal denied".into() };
        let mut steps: Vec<(&'static str, String)> = Vec::new();
        let mut say = |label: &'static str, e: &Expr| {
            let t = print::expr(e);
            if steps.last().map_or(print::expr(f) != t, |(_, last)| *last != t) {
                steps.push((label, t));
            }
        };
        let e = elim(f);
        say("no →, ↔ or xor", &e);
        let e = nnf(&e, false).map_err(|m| format!("{of}: {m}"))?;
        say("negations in", &e);
        let (prefix, matrix) = prenex(&rename(&e, &mut taken));
        say("prenex", &prefixed(&prefix, matrix.clone()));
        let mut universals = Vec::new();
        let mut m = matrix;
        for (q, v) in &prefix {
            if *q == Quant::All {
                universals.push(v.clone());
                continue;
            }
            let name = loop {
                sk += 1;
                let n = format!("sk{sk}");
                if !taken.contains(&n) {
                    break n;
                }
            };
            let term = if universals.is_empty() { Expr::Name(name) } else { Expr::App(name, universals.iter().map(|u| Expr::name(u)).collect()) };
            m = subst(&m, v, &term);
        }
        say("Skolem functions", &prefixed(&universals.iter().map(|u| (Quant::All, u.clone())).collect::<Vec<_>>(), m.clone()));
        let lists = cnf(&m).map_err(|e| format!("{of}: {e}"))?;
        let is_var = |n: &str| universals.iter().any(|u| u == n);
        let mut clauses = Vec::new();
        for lits in &lists {
            let mut vars = Vec::new();
            let c: Clause = lits.iter().map(|l| to_lit(l, &mut vars, &is_var)).collect::<Result<_, _>>().map_err(|e| format!("{of}: {e}"))?;
            if let Some(c) = norm(c) {
                if !clauses.contains(&c) {
                    clauses.push(c);
                }
            }
        }
        let shown: Vec<String> = clauses.iter().map(|c| clause_text(c, &consts)).collect();
        steps.push(("clauses", if shown.is_empty() { "none: it always holds".into() } else { shown.join(";  ") }));
        out.push(Form { of, formula: print::expr(f), steps, clauses });
    }
    Ok(out)
}

/* ── checking ─────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    Premise,
    Goal,
    Resolve,
    Factor,
}

const RULES: [(Rule, &[&str]); 4] = [
    (Rule::Premise, &["premise", "hyp", "given"]),
    (Rule::Goal, &["goal", "denied", "negated goal", "goal denied"]),
    (Rule::Resolve, &["resolve", "res", "resolution", "resolvent"]),
    (Rule::Factor, &["factor", "fac", "instance"]),
];

fn rule_named(w: &str) -> Option<Rule> {
    let w = w.to_lowercase();
    RULES.iter().find(|(_, names)| names.contains(&w.as_str())).map(|(r, _)| *r)
}

fn rule_name(r: Rule) -> &'static str {
    RULES.iter().find(|(x, _)| *x == r).unwrap().1[0]
}

/// A line's clause: literals joined by `or`, or `false` for the empty one.
fn read_clause(body: &str, consts: &BTreeSet<String>) -> Result<Clause, String> {
    if ["false", "⊥", "□", "{}", "[]"].contains(&body.trim()) {
        return Ok(Vec::new());
    }
    let e = parse::formula(body).map_err(|d| d.message)?;
    fn flat(e: &Expr, out: &mut Vec<Expr>) {
        match e {
            Expr::Bin(Op::Or, a, b) => {
                flat(a, out);
                flat(b, out);
            }
            Expr::Bool(false) => {}
            _ => out.push(e.clone()),
        }
    }
    let mut lits = Vec::new();
    flat(&e, &mut lits);
    let is_var = |n: &str| variable_like(n) && !consts.contains(n);
    let mut vars = Vec::new();
    lits.iter().map(|l| to_lit(l, &mut vars, &is_var)).collect()
}

struct RLine {
    at: usize,
    n: usize,
    clause: Clause,
    text: String,
    rule: Rule,
    cites: Vec<usize>,
}

fn listed(cs: &[Clause], consts: &BTreeSet<String>) -> String {
    let mut s: Vec<String> = cs.iter().take(4).map(|c| clause_text(c, consts)).collect();
    if cs.len() > 4 {
        s.push(format!("and {} more", cs.len() - 4));
    }
    s.join("; ")
}

fn check_line(lines: &[RLine], k: usize, forms: &[Form], h: &Header, consts: &BTreeSet<String>) -> Result<String, String> {
    let l = &lines[k];
    let target = freeze(&l.clause);
    let fits = |c: &Clause| subsumes(c, &target, &mut Sub::new());
    let cited = |m: usize| -> Result<&Clause, String> {
        match lines.iter().position(|x| x.n == m) {
            Some(i) if i < k => Ok(&lines[i].clause),
            Some(_) => Err(format!("line {m} is not before this one")),
            None => Err(format!("there is no line {m}")),
        }
    };
    match l.rule {
        Rule::Premise => {
            let [p] = l.cites[..] else { return Err("premise names which: premise 1, premise 2…".into()) };
            if p == 0 || p > h.premises.len() {
                return Err(format!("there is no premise {p}: the header names {}", h.premises.len()));
            }
            let form = &forms[p - 1];
            if form.clauses.iter().any(fits) {
                Ok(format!("a clause of premise {p}"))
            } else {
                Err(format!("this is not a clause of premise {p}: its clauses are {}", listed(&form.clauses, consts)))
            }
        }
        Rule::Goal => {
            if h.goal.is_none() {
                return Err("the header names no goal".into());
            }
            let form = forms.last().unwrap();
            if form.clauses.iter().any(fits) {
                Ok("a clause of the goal denied".into())
            } else {
                Err(format!("this is not a clause of the goal denied: its clauses are {}", listed(&form.clauses, consts)))
            }
        }
        Rule::Resolve => {
            let [i, j] = l.cites[..] else { return Err("resolve cites 2 lines".into()) };
            let rs = resolvents(cited(i)?, cited(j)?);
            if rs.iter().any(fits) {
                Ok(format!("a resolvent of {i} and {j}"))
            } else if rs.is_empty() {
                Err(format!("lines {i} and {j} have no literals that clash: one's negation unifying with the other"))
            } else {
                Err(format!("lines {i} and {j} resolve to {}, not this", listed(&rs, consts)))
            }
        }
        Rule::Factor => {
            let [i] = l.cites[..] else { return Err("factor cites 1 line".into()) };
            if fits(cited(i)?) {
                Ok(format!("line {i} with its variables given values and like literals merged"))
            } else {
                Err(format!("this is not an instance of line {i}"))
            }
        }
    }
}

/// A resolution proof's file read and checked, as any `.proof` file's is.
pub fn report(text: &str) -> Report {
    let (mut header, body) = proofs::header(text);
    let consts = constants(&header.premises, header.goal.as_ref());
    let forms = match forms(&header.premises, header.goal.as_ref()) {
        Ok(f) => f,
        Err(message) => {
            let at = text.lines().position(|l| l.trim_start().starts_with("premises") || l.trim_start().starts_with("goal")).map_or(1, |i| i + 1);
            header.unread.push(Diag { line: at, col: 0, message });
            Vec::new()
        }
    };
    let names = RULES.iter().map(|(r, _)| rule_name(*r)).collect::<Vec<_>>().join(" ");
    let mut lines = Vec::new();
    for (at, t) in body {
        let read = proofs::split(&t, &|w| rule_named(w).is_some(), &names).and_then(|s| {
            if s.depth > 0 {
                return Err(format!("line {}: a resolution proof has no bars; each line is a clause", s.n));
            }
            let clause = read_clause(&s.body, &consts).map_err(|m| format!("line {}: {m}", s.n))?;
            let cites = s.cites.iter().map(|c| match c {
                proofs::Cite::Line(m) => Ok(*m),
                proofs::Cite::Range(..) => Err(format!("line {}: a resolution proof cites lines, not ranges", s.n)),
            }).collect::<Result<_, _>>()?;
            let text = if clause.is_empty() { "false".into() } else { print::expr(&parse::formula(&s.body).map_err(|d| d.message)?) };
            Ok(RLine { at, n: s.n, clause, text, rule: rule_named(&s.rule).unwrap(), cites })
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
            let r = if !seen.insert(l.n) {
                Err(format!("there is a line {} already", l.n))
            } else if forms.is_empty() && matches!(l.rule, Rule::Premise | Rule::Goal) {
                Err("the header's clause form could not be made".into())
            } else {
                check_line(&lines, k, &forms, &header, &consts)
            };
            LineReport { at: l.at, n: l.n, text: l.text.clone(), rule: rule_name(l.rule).into(), ok: r.is_ok(), message: r.unwrap_or_else(|e| e) }
        })
        .collect();
    let reached = lines.iter().zip(&out).any(|(l, r)| l.clause.is_empty() && r.ok);
    let wrong = out.iter().filter(|l| !l.ok).count() + header.unread.len();
    let summary = if out.is_empty() {
        "no lines yet".into()
    } else if wrong > 0 {
        format!("{wrong} line{} wrong", if wrong == 1 { " is" } else { "s are" })
    } else if reached {
        match &header.goal {
            Some(g) => format!("a refutation: {} with {} denied cannot all hold, so {} follows", proofs::premises_said(&header.premises), print::expr(g), print::expr(g)),
            None => "a refutation: the premises cannot all hold".into(),
        }
    } else {
        "every line is right; the empty clause is not reached yet".into()
    };
    Report { complete: wrong == 0 && reached, lines: out, summary, header }
}

/* ── finding ──────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy)]
enum From {
    Premise(usize),
    Goal,
    Resolve(usize, usize),
    Factor(usize),
}

struct Kept {
    clause: Clause,
    from: From,
}

fn weight(c: &Clause) -> usize {
    fn tm(t: &Tm) -> usize {
        match t {
            Tm::V(_) => 1,
            Tm::F(_, args) => 1 + args.iter().map(tm).sum::<usize>(),
        }
    }
    c.iter().map(|l| 1 + l.args.iter().map(tm).sum::<usize>()).sum()
}

/// A resolution refutation of `premises` with `goal` denied, found, as a
/// `.proof` file's text with each formula's clause form above it; or why
/// none was.
pub fn find(premises: &[Expr], goal: &Expr, time: Duration) -> Result<String, String> {
    let forms = forms(premises, Some(goal))?;
    let consts = constants(premises, Some(goal));
    let deadline = Instant::now() + time;
    let mut kept: Vec<Kept> = Vec::new();
    let mut seen: HashSet<Clause> = HashSet::new();
    let mut active: Vec<usize> = Vec::new();
    let mut passive: BinaryHeap<Reverse<(usize, usize)>> = BinaryHeap::new();
    let mut empty = None;
    // A clause subsumes another only if it is no longer: as sets, a clause
    // subsumes its own factors, and they would never be kept.
    let mut add = |c: Clause, from: From, kept: &mut Vec<Kept>, active: &[usize], passive: &mut BinaryHeap<Reverse<(usize, usize)>>| {
        if !seen.insert(c.clone()) {
            return None;
        }
        let frozen = freeze(&c);
        if active.iter().any(|&a| kept[a].clause.len() <= frozen.len() && subsumes(&kept[a].clause, &frozen, &mut Sub::new())) {
            return None;
        }
        kept.push(Kept { clause: c.clone(), from });
        let id = kept.len() - 1;
        passive.push(Reverse((weight(&c), id)));
        c.is_empty().then_some(id)
    };
    for (k, f) in forms.iter().enumerate() {
        let from = if k < premises.len() { From::Premise(k + 1) } else { From::Goal };
        for c in &f.clauses {
            empty = empty.or(add(c.clone(), from, &mut kept, &active, &mut passive));
        }
    }
    let mut gave_up = false;
    while empty.is_none() {
        let Some(Reverse((_, g))) = passive.pop() else { break };
        if Instant::now() >= deadline || kept.len() >= MOST_KEPT {
            gave_up = true;
            break;
        }
        let frozen = freeze(&kept[g].clause);
        if active.iter().any(|&a| kept[a].clause.len() <= frozen.len() && subsumes(&kept[a].clause, &frozen, &mut Sub::new())) {
            continue;
        }
        active.push(g);
        let given = kept[g].clause.clone();
        for f in factors(&given) {
            if let Some(e) = add(f, From::Factor(g), &mut kept, &active, &mut passive) {
                empty = Some(e);
            }
        }
        for a in active.clone() {
            if empty.is_some() {
                break;
            }
            for r in resolvents(&kept[a].clause, &given) {
                if let Some(e) = add(r, From::Resolve(a, g), &mut kept, &active, &mut passive) {
                    empty = Some(e);
                    break;
                }
            }
        }
    }
    let Some(end) = empty else {
        return Err(if gave_up { "no refutation found in the time".into() } else { "the clauses saturate without the empty clause: the argument does not hold".into() });
    };
    // The clauses the empty one came from, in the order they were made.
    let mut used = BTreeSet::new();
    let mut todo = vec![end];
    while let Some(i) = todo.pop() {
        if used.insert(i) {
            match kept[i].from {
                From::Resolve(a, b) => todo.extend([a, b]),
                From::Factor(a) => todo.push(a),
                _ => {}
            }
        }
    }
    let order: Vec<usize> = used.into_iter().collect();
    let line = |i: usize| order.iter().position(|&x| x == i).unwrap() + 1;
    let rows: Vec<(String, String)> = order
        .iter()
        .map(|&i| {
            let just = match kept[i].from {
                From::Premise(k) => format!("premise {k}"),
                From::Goal => "goal".into(),
                From::Resolve(a, b) => format!("resolve {}, {}", line(a), line(b)),
                From::Factor(a) => format!("factor {}", line(a)),
            };
            (clause_text(&kept[i].clause, &consts), just)
        })
        .collect();
    let mut text = proofs::write_header("resolution", premises, goal);
    for f in &forms {
        text.push_str(&format!("# {}, {}:\n", capital(&f.of), f.formula));
        let width = f.steps.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
        for (label, said) in &f.steps {
            text.push_str(&format!("#   {:<width$}  {said}\n", format!("{label}:"), width = width + 1));
        }
    }
    text.push('\n');
    text.push_str(&proofs::write_lines(&rows));
    let checked = report(&text);
    if !checked.complete {
        let bad = checked.lines.iter().find(|c| !c.ok).map(|c| format!("line {}: {}", c.n, c.message)).unwrap_or(checked.summary);
        return Err(format!("the proof found did not check ({bad}): a fault in the finder"));
    }
    Ok(text)
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
