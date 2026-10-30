//! Semantic tableaux: an argument proved by refuting its premises with its
//! conclusion denied — every branch closed — or shown to fail.
//!
//! - **The rules** are Smullyan's: α rules (`and`, `not or`, `not ->`,
//!   `not not`) add both parts to the branch; β rules (`or`, `not and`,
//!   `->`, `<->` and `xor`) split it. α rules are used first.
//! - **First-order:** a universal (γ) is instantiated with a new free
//!   variable and put back at the end of the branch's queue, to be used
//!   again; an existential (δ) with a Skolem function of the branch's free
//!   variables. A branch closes on two literals that unify; one
//!   substitution must close every branch, so the search goes back over
//!   which pair closed each — with a bound on the free variables, raised
//!   until a proof is found or the time is up. This is the plan of
//!   Beckert and Posegga's leanTAP, written from the method.
//! - **What it does not know:** `=` and `<` are predicates like any other
//!   here — no rules for equality — so a proof that needs equality's
//!   properties is not found. Nothing it finds is wrong for that.
//! - **When it fails:** a propositional argument's tableau is expanded
//!   whole, and an open branch read as a countermodel; a first-order one has
//!   a countermodel of up to [`MOST_THINGS`] individuals looked for with the
//!   model finder.

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::ast::*;
use crate::eval::Model;
use crate::types::{Checked, Sig, Symbol, Ty, INDIVIDUAL};

/// Free variables a branch may have, at most, while the bound is raised.
pub const MOST_VARIABLES: u32 = 12;
/// A countermodel is looked for with up to this many individuals.
pub const MOST_THINGS: u32 = 4;
/// Nodes of a tableau shown, at most.
pub const MOST_NODES: usize = 2000;

/* ── terms and formulas ───────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq)]
enum T {
    /// A variable a quantifier binds, until it is instantiated.
    B(String),
    /// A free variable, which a closing substitution may give a value.
    V(u32),
    F(String, Vec<T>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Fm {
    Atom(String, Vec<T>),
    Const(bool),
    Not(Box<Fm>),
    Bin(Op, Box<Fm>, Box<Fm>),
    All(String, Box<Fm>),
    Ex(String, Box<Fm>),
}

fn not(f: Fm) -> Fm {
    Fm::Not(Box::new(f))
}

/// Whether an expression is a formula, rather than a term.
fn is_formula(sig: &Sig, e: &Expr) -> bool {
    match e {
        Expr::Bool(_) | Expr::Not(_) | Expr::Quant(..) => true,
        Expr::Bin(op, ..) => op.level() <= 6,
        Expr::Name(n) => sig.propositions.contains(n) || sig.symbols.get(n).is_some_and(|s| s.args.is_empty() && s.result == Ty::Bool),
        Expr::App(f, _) => f == "defined" || sig.loose.get(f).is_some_and(|(_, p)| *p) || sig.symbols.get(f).is_some_and(|s| s.result == Ty::Bool),
        _ => false,
    }
}

fn to_fm(sig: &Sig, e: &Expr) -> Result<Fm, String> {
    Ok(match e {
        Expr::Bool(b) => Fm::Const(*b),
        Expr::Name(n) => Fm::Atom(n.clone(), Vec::new()),
        Expr::App(f, _) if f == "defined" => return Err("defined(…) is the model finder's, not the prover's".into()),
        Expr::App(f, args) => Fm::Atom(f.clone(), args.iter().map(to_t).collect::<Result<_, _>>()?),
        Expr::Not(a) => not(to_fm(sig, a)?),
        Expr::Bin(op, a, b) if op.is_logical() => Fm::Bin(*op, Box::new(to_fm(sig, a)?), Box::new(to_fm(sig, b)?)),
        // `=` between formulas is `<->`; between terms, a predicate.
        Expr::Bin(op @ (Op::Eq | Op::Ne), a, b) if is_formula(sig, a) && is_formula(sig, b) => {
            Fm::Bin(if *op == Op::Eq { Op::Iff } else { Op::Xor }, Box::new(to_fm(sig, a)?), Box::new(to_fm(sig, b)?))
        }
        Expr::Bin(Op::Ne, a, b) => not(Fm::Atom("=".into(), vec![to_t(a)?, to_t(b)?])),
        Expr::Bin(op, a, b) => Fm::Atom(op.text().into(), vec![to_t(a)?, to_t(b)?]),
        Expr::Quant(q, binders, body) => {
            let mut f = to_fm(sig, body)?;
            for b in binders.iter().rev() {
                f = match q {
                    Quant::All => Fm::All(b.name.clone(), Box::new(f)),
                    Quant::Some => Fm::Ex(b.name.clone(), Box::new(f)),
                    _ => return Err("counting quantifiers are the model finder's, not the prover's".into()),
                };
            }
            f
        }
        other => return Err(format!("“{}” is a value, not a formula", crate::print::expr(other))),
    })
}

fn to_t(e: &Expr) -> Result<T, String> {
    Ok(match e {
        Expr::Name(n) => T::B(n.clone()),
        Expr::Num(n) => T::F(n.to_string(), Vec::new()),
        Expr::End { last, sort } => T::F(format!("{}({sort})", if *last { "last" } else { "first" }), Vec::new()),
        Expr::App(f, args) => T::F(f.clone(), args.iter().map(to_t).collect::<Result<_, _>>()?),
        Expr::Neg(a) => T::F("-".into(), vec![to_t(a)?]),
        Expr::Bin(op, a, b) if op.level() >= 7 => T::F(op.text().into(), vec![to_t(a)?, to_t(b)?]),
        other => return Err(format!("“{}” is a formula where a value was expected", crate::print::expr(other))),
    })
}

/// `f` with the bound `x` replaced by `t`, where no quantifier inside binds it again.
fn put(f: &Fm, x: &str, t: &T) -> Fm {
    fn term(u: &T, x: &str, t: &T) -> T {
        match u {
            T::B(n) if n == x => t.clone(),
            T::F(g, args) => T::F(g.clone(), args.iter().map(|a| term(a, x, t)).collect()),
            other => other.clone(),
        }
    }
    match f {
        Fm::Atom(p, args) => Fm::Atom(p.clone(), args.iter().map(|a| term(a, x, t)).collect()),
        Fm::Const(_) => f.clone(),
        Fm::Not(a) => not(put(a, x, t)),
        Fm::Bin(op, a, b) => Fm::Bin(*op, Box::new(put(a, x, t)), Box::new(put(b, x, t))),
        Fm::All(y, a) if y != x => Fm::All(y.clone(), Box::new(put(a, x, t))),
        Fm::Ex(y, a) if y != x => Fm::Ex(y.clone(), Box::new(put(a, x, t))),
        _ => f.clone(),
    }
}

/// Whether a formula is more than propositions: it has a quantifier, or
/// `=` or an order, whose meaning propositions would not keep.
fn first_order(f: &Fm) -> bool {
    match f {
        Fm::All(..) | Fm::Ex(..) => true,
        Fm::Atom(p, _) => ["=", "<", "<=", ">", ">="].contains(&p.as_str()),
        Fm::Not(a) => first_order(a),
        Fm::Bin(_, a, b) => first_order(a) || first_order(b),
        Fm::Const(_) => false,
    }
}

fn has_quantifier(f: &Fm) -> bool {
    match f {
        Fm::All(..) | Fm::Ex(..) => true,
        Fm::Not(a) => has_quantifier(a),
        Fm::Bin(_, a, b) => has_quantifier(a) || has_quantifier(b),
        _ => false,
    }
}

/// Which formula of a branch's queue is taken first: what costs nothing,
/// then what adds without splitting, then splits, then universals.
fn rank(f: &Fm) -> u8 {
    match rule(f) {
        Rule::Lit | Rule::Done | Rule::Absurd => 0,
        Rule::Alpha(..) | Rule::Delta(..) => 1,
        Rule::Beta(..) => 2,
        Rule::Gamma(..) => 3,
    }
}

/* ── unification ──────────────────────────────────────────────────── */

type Subst = Vec<Option<T>>;

fn walk(t: &T, s: &Subst) -> T {
    match t {
        T::V(v) => match s.get(*v as usize).and_then(|x| x.as_ref()) {
            Some(u) => walk(u, s),
            None => t.clone(),
        },
        _ => t.clone(),
    }
}

fn occurs(v: u32, t: &T, s: &Subst) -> bool {
    match walk(t, s) {
        T::V(w) => w == v,
        T::F(_, args) => args.iter().any(|a| occurs(v, a, s)),
        T::B(_) => false,
    }
}

fn unify(a: &T, b: &T, s: &mut Subst) -> bool {
    let (a, b) = (walk(a, s), walk(b, s));
    match (&a, &b) {
        (T::V(x), T::V(y)) if x == y => true,
        (T::V(x), _) | (_, T::V(x)) => {
            let other = if matches!(a, T::V(v) if v == *x) { &b } else { &a };
            if occurs(*x, other, s) {
                return false;
            }
            if s.len() <= *x as usize {
                s.resize(*x as usize + 1, None);
            }
            s[*x as usize] = Some(other.clone());
            true
        }
        (T::F(f, xs), T::F(g, ys)) => f == g && xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| unify(x, y, s)),
        (T::B(x), T::B(y)) => x == y,
        _ => false,
    }
}

fn resolve(t: &T, s: &Subst) -> T {
    match walk(t, s) {
        T::F(f, args) => T::F(f, args.iter().map(|a| resolve(a, s)).collect()),
        other => other,
    }
}

fn resolve_fm(f: &Fm, s: &Subst) -> Fm {
    match f {
        Fm::Atom(p, args) => Fm::Atom(p.clone(), args.iter().map(|a| resolve(a, s)).collect()),
        Fm::Not(a) => not(resolve_fm(a, s)),
        Fm::Bin(op, a, b) => Fm::Bin(*op, Box::new(resolve_fm(a, s)), Box::new(resolve_fm(b, s))),
        Fm::All(x, a) => Fm::All(x.clone(), Box::new(resolve_fm(a, s))),
        Fm::Ex(x, a) => Fm::Ex(x.clone(), Box::new(resolve_fm(a, s))),
        Fm::Const(_) => f.clone(),
    }
}

/* ── the tree ─────────────────────────────────────────────────────── */

/// A line of a tableau: its number, the formula, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabLine {
    pub n: usize,
    pub text: String,
    pub why: String,
}

/// A branch: its lines, then how it ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTree {
    pub lines: Vec<TabLine>,
    pub end: TabEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabEnd {
    /// Closed by two lines that contradict each other (the same line when
    /// it is false of itself).
    Closed(usize, usize),
    /// Every formula used, and nothing contradicts: a countermodel.
    Open,
    /// Split in two.
    Split(Box<TabTree>, Box<TabTree>),
}

/// What the prover made of an argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tableau {
    /// Proved (`Some(true)`), shown to fail (`Some(false)`), or neither in
    /// the time or with the free variables allowed.
    pub valid: Option<bool>,
    pub tree: Option<TabTree>,
    /// For a propositional argument that fails, its open branch's
    /// propositions: each, and whether it is true.
    pub open: Vec<(String, bool)>,
    /// For a first-order one, a countermodel found by the model finder:
    /// the theory it is a model of, and it.
    #[serde(skip)]
    pub counter: Option<(Checked, Model)>,
    /// The free variables the proof needed.
    pub variables: u32,
    pub note: String,
}

#[derive(Debug, Clone)]
struct Node {
    fm: Fm,
    /// The line it came from, and the rule.
    from: Option<usize>,
    rule: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum Ev {
    Add(usize),
    Split,
    Right,
    Close(usize, usize),
    Open,
}

/// The branches as the log of a search or an expansion has them.
fn build(nodes: &[Node], log: &[Ev], s: &Subst) -> TabTree {
    let mut number = std::collections::HashMap::new();
    let mut next = 1;
    fn branch(nodes: &[Node], log: &[Ev], i: &mut usize, s: &Subst, number: &mut std::collections::HashMap<usize, usize>, next: &mut usize) -> TabTree {
        let mut lines = Vec::new();
        while *i < log.len() {
            let ev = log[*i];
            *i += 1;
            match ev {
                Ev::Add(id) => {
                    number.insert(id, *next);
                    let node = &nodes[id];
                    let why = match node.from {
                        Some(f) => format!("{}, {}", number.get(&f).copied().unwrap_or(0), node.rule),
                        None => node.rule.to_string(),
                    };
                    lines.push(TabLine { n: *next, text: show(&resolve_fm(&node.fm, s)), why });
                    *next += 1;
                }
                Ev::Close(a, b) => {
                    let (a, b) = (number.get(&a).copied().unwrap_or(0), number.get(&b).copied().unwrap_or(0));
                    return TabTree { lines, end: TabEnd::Closed(b.min(a), a.max(b)) };
                }
                Ev::Open => return TabTree { lines, end: TabEnd::Open },
                Ev::Split => {
                    let left = branch(nodes, log, i, s, number, next);
                    // The right branch starts at its marker.
                    if matches!(log.get(*i), Some(Ev::Right)) {
                        *i += 1;
                    }
                    let right = branch(nodes, log, i, s, number, next);
                    return TabTree { lines, end: TabEnd::Split(Box::new(left), Box::new(right)) };
                }
                Ev::Right => {}
            }
        }
        TabTree { lines, end: TabEnd::Open }
    }
    let mut i = 0;
    branch(nodes, log, &mut i, s, &mut number, &mut next)
}

/// A formula written back in Proton's language: free variables as `X1`,
/// Skolem functions as `sk1(…)`.
fn show(f: &Fm) -> String {
    fn term(t: &T) -> Expr {
        match t {
            T::B(n) => Expr::name(n),
            T::V(v) => Expr::Name(format!("X{v}")),
            T::F(f, args) if args.is_empty() => match f.parse::<i64>() {
                Ok(n) => Expr::Num(n),
                Err(_) => Expr::name(f),
            },
            T::F(f, args) if args.len() == 2 && ["+", "-", "*"].contains(&f.as_str()) => {
                let op = match f.as_str() {
                    "+" => Op::Add,
                    "-" => Op::Sub,
                    _ => Op::Mul,
                };
                Expr::bin(op, term(&args[0]), term(&args[1]))
            }
            T::F(f, args) => Expr::App(f.clone(), args.iter().map(term).collect()),
        }
    }
    fn fm(f: &Fm) -> Expr {
        match f {
            Fm::Atom(p, args) if args.len() == 2 && ["=", "<", "<=", ">", ">="].contains(&p.as_str()) => {
                let op = match p.as_str() {
                    "=" => Op::Eq,
                    "<" => Op::Lt,
                    "<=" => Op::Le,
                    ">" => Op::Gt,
                    _ => Op::Ge,
                };
                Expr::bin(op, term(&args[0]), term(&args[1]))
            }
            Fm::Atom(p, args) if args.is_empty() => Expr::name(p),
            Fm::Atom(p, args) => Expr::App(p.clone(), args.iter().map(term).collect()),
            Fm::Const(b) => Expr::Bool(*b),
            Fm::Not(a) => Expr::negation(fm(a)),
            Fm::Bin(op, a, b) => Expr::bin(*op, fm(a), fm(b)),
            Fm::All(x, a) => Expr::Quant(Quant::All, vec![Binder { name: x.clone(), sort: None }], Box::new(fm(a))),
            Fm::Ex(x, a) => Expr::Quant(Quant::Some, vec![Binder { name: x.clone(), sort: None }], Box::new(fm(a))),
        }
    }
    crate::print::expr(&fm(f))
}

/* ── the rules ────────────────────────────────────────────────────── */

/// What a rule does with a formula.
enum Rule {
    /// A literal, true or false of its own, or a contradiction.
    Lit,
    Done,
    Absurd,
    /// Both parts added.
    Alpha(Vec<Fm>, &'static str),
    /// The branch split: each side's parts.
    Beta(Vec<Fm>, Vec<Fm>, &'static str),
    /// A universal, by its variable and body (the body negated if `neg`).
    Gamma(String, Fm, bool),
    /// An existential, likewise.
    Delta(String, Fm, bool),
}

fn rule(f: &Fm) -> Rule {
    use Fm::*;
    match f {
        Const(true) => Rule::Done,
        Const(false) => Rule::Absurd,
        Atom(..) => Rule::Lit,
        Bin(Op::And, a, b) => Rule::Alpha(vec![(**a).clone(), (**b).clone()], "∧"),
        Bin(Op::Or, a, b) => Rule::Beta(vec![(**a).clone()], vec![(**b).clone()], "∨"),
        Bin(Op::Imp, a, b) => Rule::Beta(vec![not((**a).clone())], vec![(**b).clone()], "→"),
        Bin(Op::Iff, a, b) => Rule::Beta(vec![(**a).clone(), (**b).clone()], vec![not((**a).clone()), not((**b).clone())], "↔"),
        Bin(Op::Xor, a, b) => Rule::Beta(vec![(**a).clone(), not((**b).clone())], vec![not((**a).clone()), (**b).clone()], "⊕"),
        Bin(..) => Rule::Lit,
        All(x, a) => Rule::Gamma(x.clone(), (**a).clone(), false),
        Ex(x, a) => Rule::Delta(x.clone(), (**a).clone(), false),
        Not(g) => match &**g {
            Const(b) => {
                if *b {
                    Rule::Absurd
                } else {
                    Rule::Done
                }
            }
            Atom(..) => Rule::Lit,
            Not(a) => Rule::Alpha(vec![(**a).clone()], "¬¬"),
            Bin(Op::And, a, b) => Rule::Beta(vec![not((**a).clone())], vec![not((**b).clone())], "¬∧"),
            Bin(Op::Or, a, b) => Rule::Alpha(vec![not((**a).clone()), not((**b).clone())], "¬∨"),
            Bin(Op::Imp, a, b) => Rule::Alpha(vec![(**a).clone(), not((**b).clone())], "¬→"),
            Bin(Op::Iff, a, b) => Rule::Beta(vec![(**a).clone(), not((**b).clone())], vec![not((**a).clone()), (**b).clone()], "¬↔"),
            Bin(Op::Xor, a, b) => Rule::Beta(vec![(**a).clone(), (**b).clone()], vec![not((**a).clone()), not((**b).clone())], "¬⊕"),
            Bin(..) => Rule::Lit,
            All(x, a) => Rule::Delta(x.clone(), (**a).clone(), true),
            Ex(x, a) => Rule::Gamma(x.clone(), (**a).clone(), true),
        },
    }
}

/// A literal's sign, predicate and arguments.
fn literal(f: &Fm) -> Option<(bool, &str, &[T])> {
    match f {
        Fm::Atom(p, args) => Some((true, p, args)),
        Fm::Not(a) => match &**a {
            Fm::Atom(p, args) => Some((false, p, args)),
            _ => None,
        },
        _ => None,
    }
}

/* ── the search ───────────────────────────────────────────────────── */

type K<'a> = &'a mut dyn FnMut(&mut Prover, &Subst) -> bool;

struct Prover {
    nodes: Vec<Node>,
    log: Vec<Ev>,
    vars: u32,
    skolems: u32,
    deadline: Instant,
    out_of_time: bool,
    steps: u64,
}

impl Prover {
    fn add(&mut self, fm: Fm, from: Option<usize>, rule: &'static str) -> usize {
        self.nodes.push(Node { fm, from, rule });
        let id = self.nodes.len() - 1;
        self.log.push(Ev::Add(id));
        id
    }

    /// The next formula on the branch — the cheapest, the first of those —
    /// or none: an open branch.
    fn next(&mut self, unexp: &[usize], lits: &[usize], free: &[u32], lim: u32, s: &Subst, k: K) -> bool {
        let Some(best) = (0..unexp.len()).min_by_key(|&i| (rank(&self.nodes[unexp[i]].fm), i)) else { return false };
        let rest: Vec<usize> = unexp.iter().enumerate().filter(|(i, _)| *i != best).map(|(_, n)| *n).collect();
        self.prove(unexp[best], &rest, lits, free, lim, s, k)
    }

    /// Node `cur` taken on a branch whose queue is `unexp`, whose literals
    /// are `lits`; `k` goes on once the branch is closed.
    #[allow(clippy::too_many_arguments)]
    fn prove(&mut self, cur: usize, unexp: &[usize], lits: &[usize], free: &[u32], lim: u32, s: &Subst, k: K) -> bool {
        self.steps += 1;
        if self.steps % 512 == 0 && Instant::now() >= self.deadline {
            self.out_of_time = true;
        }
        if self.out_of_time {
            return false;
        }
        let fm = self.nodes[cur].fm.clone();
        match rule(&fm) {
            Rule::Done => self.next(unexp, lits, free, lim, s, k),
            Rule::Absurd => {
                self.log.push(Ev::Close(cur, cur));
                k(self, s)
            }
            Rule::Alpha(parts, name) => {
                let ids: Vec<usize> = parts.into_iter().map(|p| self.add(p, Some(cur), name)).collect();
                let queue: Vec<usize> = ids.iter().chain(unexp).copied().collect();
                self.next(&queue, lits, free, lim, s, k)
            }
            Rule::Beta(left, right, name) => {
                self.log.push(Ev::Split);
                let ids: Vec<usize> = left.into_iter().map(|p| self.add(p, Some(cur), name)).collect();
                let queue: Vec<usize> = ids.iter().chain(unexp).copied().collect();
                let (unexp, lits, free) = (unexp.to_vec(), lits.to_vec(), free.to_vec());
                let mut on = |p: &mut Prover, s2: &Subst| -> bool {
                    let mark = p.log.len();
                    p.log.push(Ev::Right);
                    let ids: Vec<usize> = right.iter().cloned().map(|f| p.add(f, Some(cur), name)).collect();
                    let queue: Vec<usize> = ids.iter().chain(&unexp).copied().collect();
                    if p.next(&queue, &lits, &free, lim, s2, k) {
                        return true;
                    }
                    p.log.truncate(mark);
                    false
                };
                // The left first; once it closes, `on` takes the right.
                self.next(&queue, &lits, &free, lim, s, &mut on)
            }
            Rule::Gamma(x, body, neg) => {
                if free.len() as u32 >= lim {
                    return false;
                }
                self.vars += 1;
                let v = self.vars;
                let inst = put(&body, &x, &T::V(v));
                let id = self.add(if neg { not(inst) } else { inst }, Some(cur), "γ");
                // Put back at the end, to be used again.
                let queue: Vec<usize> = std::iter::once(id).chain(unexp.iter().copied()).chain(std::iter::once(cur)).collect();
                let free: Vec<u32> = free.iter().copied().chain(std::iter::once(v)).collect();
                self.next(&queue, lits, &free, lim, s, k)
            }
            Rule::Delta(x, body, neg) => {
                self.skolems += 1;
                let sk = T::F(format!("sk{}", self.skolems), free.iter().map(|v| T::V(*v)).collect());
                let inst = put(&body, &x, &sk);
                let id = self.add(if neg { not(inst) } else { inst }, Some(cur), "δ");
                let queue: Vec<usize> = std::iter::once(id).chain(unexp.iter().copied()).collect();
                self.next(&queue, lits, free, lim, s, k)
            }
            Rule::Lit => {
                let (sign, p, args) = literal(&fm).unwrap();
                // Closed against each contradicting literal in turn, the newest first.
                for &m in lits.iter().rev() {
                    let other = self.nodes[m].fm.clone();
                    let Some((sign2, p2, args2)) = literal(&other) else { continue };
                    if sign2 == sign || p2 != p || args2.len() != args.len() {
                        continue;
                    }
                    let mut s2 = s.clone();
                    if args.iter().zip(args2).all(|(a, b)| unify(a, b, &mut s2)) {
                        let mark = self.log.len();
                        self.log.push(Ev::Close(m, cur));
                        if k(self, &s2) {
                            return true;
                        }
                        self.log.truncate(mark);
                        if self.out_of_time {
                            return false;
                        }
                    }
                }
                let lits: Vec<usize> = lits.iter().copied().chain(std::iter::once(cur)).collect();
                self.next(unexp, &lits, free, lim, s, k)
            }
        }
    }
}

/* ── an argument ──────────────────────────────────────────────────── */

/// An argument's premises and conclusion, proved or refuted. The formulas
/// are a checked theory's (with `c`'s signature).
pub fn prove(c: &Checked, premises: &[Expr], goal: &Expr, time: Duration) -> Result<Tableau, String> {
    let sig = &c.sig;
    let mut start: Vec<(Fm, &'static str)> = Vec::new();
    for p in premises {
        start.push((to_fm(sig, p)?, "premise"));
    }
    start.push((not(to_fm(sig, goal)?), "conclusion denied"));
    let first_order = start.iter().any(|(f, _)| first_order(f));
    let deadline = Instant::now() + time;
    let mut p = Prover { nodes: Vec::new(), log: Vec::new(), vars: 0, skolems: 0, deadline, out_of_time: false, steps: 0 };
    // The bound on free variables matters only where there is a quantifier.
    let limits: Vec<u32> = if start.iter().any(|(f, _)| has_quantifier(f)) { (1..=MOST_VARIABLES).collect() } else { vec![0] };
    for lim in limits {
        p.nodes.clear();
        p.log.clear();
        p.vars = 0;
        p.skolems = 0;
        let ids: Vec<usize> = start.iter().map(|(f, why)| p.add(f.clone(), None, why)).collect();
        let mut closed: Option<Subst> = None;
        let mut done = |_: &mut Prover, s: &Subst| {
            closed = Some(s.clone());
            true
        };
        if p.next(&ids, &[], &[], lim, &Vec::new(), &mut done) {
            let s = closed.unwrap_or_default();
            let tree = build(&p.nodes, &p.log, &s);
            let note = if first_order { format!("closed with {lim} free variable{} on a branch at most", if lim == 1 { "" } else { "s" }) } else { String::new() };
            return Ok(Tableau { valid: Some(true), tree: Some(tree), open: Vec::new(), counter: None, variables: lim, note });
        }
        if p.out_of_time {
            break;
        }
    }
    if !first_order {
        // Expanded whole: an open branch is a countermodel.
        let (tree, open) = expand(&start);
        return Ok(Tableau { valid: Some(false), tree: Some(tree), open, counter: None, variables: 0, note: String::new() });
    }
    // First-order: a small countermodel, if there is one.
    let counter = countermodel(c, premises, goal, deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(500)))?;
    let note = if p.out_of_time {
        "no proof found in the time".to_string()
    } else if start.iter().any(|(f, _)| has_quantifier(f)) {
        format!("no proof with up to {MOST_VARIABLES} free variables on a branch")
    } else {
        "no proof without the rules of equality, which the prover does not have".to_string()
    };
    Ok(match counter {
        Some(cm) => Tableau { valid: Some(false), tree: None, open: Vec::new(), counter: Some(cm), variables: 0, note },
        None => Tableau { valid: None, tree: None, open: Vec::new(), counter: None, variables: 0, note: format!("{note}, and no countermodel of up to {MOST_THINGS} individuals") },
    })
}

/// A propositional tableau expanded whole, every branch to its end: the
/// tree, and the first open branch's propositions.
fn expand(start: &[(Fm, &'static str)]) -> (TabTree, Vec<(String, bool)>) {
    let mut nodes: Vec<Node> = Vec::new();
    let mut log: Vec<Ev> = Vec::new();
    let mut open: Option<Vec<(String, bool)>> = None;
    let ids: Vec<usize> = start
        .iter()
        .map(|(f, why)| {
            nodes.push(Node { fm: f.clone(), from: None, rule: why });
            log.push(Ev::Add(nodes.len() - 1));
            nodes.len() - 1
        })
        .collect();
    #[allow(clippy::too_many_arguments)]
    fn go(nodes: &mut Vec<Node>, log: &mut Vec<Ev>, queue: Vec<usize>, lits: Vec<usize>, open: &mut Option<Vec<(String, bool)>>) {
        if log.len() > MOST_NODES * 2 {
            log.push(Ev::Open);
            return;
        }
        let Some((&cur, rest)) = queue.split_first() else {
            log.push(Ev::Open);
            if open.is_none() {
                let mut seen: Vec<(String, bool)> = lits.iter().filter_map(|&l| literal(&nodes[l].fm).map(|(sign, p, args)| (show(&Fm::Atom(p.to_string(), args.to_vec())), sign))).collect();
                seen.dedup();
                let mut out = Vec::new();
                for x in seen {
                    if !out.contains(&x) {
                        out.push(x);
                    }
                }
                *open = Some(out);
            }
            return;
        };
        let add = |nodes: &mut Vec<Node>, log: &mut Vec<Ev>, f: Fm, rule: &'static str| {
            nodes.push(Node { fm: f, from: Some(cur), rule });
            log.push(Ev::Add(nodes.len() - 1));
            nodes.len() - 1
        };
        let fm = nodes[cur].fm.clone();
        match rule(&fm) {
            Rule::Done => go(nodes, log, rest.to_vec(), lits, open),
            Rule::Absurd => log.push(Ev::Close(cur, cur)),
            Rule::Alpha(parts, name) => {
                let ids: Vec<usize> = parts.into_iter().map(|p| add(nodes, log, p, name)).collect();
                go(nodes, log, ids.into_iter().chain(rest.iter().copied()).collect(), lits, open);
            }
            Rule::Beta(left, right, name) => {
                log.push(Ev::Split);
                let l: Vec<usize> = left.into_iter().map(|p| add(nodes, log, p, name)).collect();
                go(nodes, log, l.into_iter().chain(rest.iter().copied()).collect(), lits.clone(), open);
                log.push(Ev::Right);
                let r: Vec<usize> = right.into_iter().map(|p| add(nodes, log, p, name)).collect();
                go(nodes, log, r.into_iter().chain(rest.iter().copied()).collect(), lits, open);
            }
            Rule::Lit => {
                let (sign, p, args) = literal(&fm).unwrap();
                if let Some(&m) = lits.iter().find(|&&m| literal(&nodes[m].fm).is_some_and(|(s2, p2, a2)| s2 != sign && p2 == p && a2 == args)) {
                    log.push(Ev::Close(m, cur));
                    return;
                }
                go(nodes, log, rest.to_vec(), lits.into_iter().chain(std::iter::once(cur)).collect(), open);
            }
            Rule::Gamma(..) | Rule::Delta(..) => go(nodes, log, rest.to_vec(), lits, open),
        }
    }
    go(&mut nodes, &mut log, ids, Vec::new(), &mut open);
    (build(&nodes, &log, &Vec::new()), open.unwrap_or_default())
}

/// A model of the premises with the conclusion false, of up to
/// [`MOST_THINGS`] individuals, from the model finder.
fn countermodel(c: &Checked, premises: &[Expr], goal: &Expr, time: Duration) -> Result<Option<(Checked, Model)>, String> {
    let mut t = c.clone();
    t.theory.lines.clear();
    for p in premises {
        t.theory.lines.push(Line { item: Some(Item::Constraint(p.clone())), comment: None, at: 0 });
    }
    t.theory.lines.push(Line { item: Some(Item::Constraint(Expr::negation(goal.clone()))), comment: None, at: 0 });
    // The loose functions and predicates declared, over individuals.
    let individual = Ty::Sort(INDIVIDUAL.into());
    for (name, (arity, predicate)) in std::mem::take(&mut t.sig.loose) {
        t.sig.symbols.insert(name, Symbol { args: vec![individual.clone(); arity], result: if predicate { Ty::Bool } else { individual.clone() }, props: Props::default() });
    }
    t.sig.sorts.insert(INDIVIDUAL.into(), SortKind::Sizes(1, MOST_THINGS));
    let out = crate::solve::solve_or_stop(&t, 1, false, time, &AtomicBool::new(false))?;
    Ok(out.models.into_iter().next().map(|m| (t, m)))
}
