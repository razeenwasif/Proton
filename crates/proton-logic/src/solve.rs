//! The model finder: a theory's models, found.
//!
//! - **Cells:** each function's value at each argument, each predicate's
//!   truth, each constant and proposition, is a cell whose possible values
//!   are its sort's (and, for a partial function, none).
//! - **Constraints** are compiled once — names to cells, elements to their
//!   values, variables to slots — and each `all` at the top of one is
//!   spread into an instance a value. An instance true whatever the cells
//!   are is dropped; one false whatever they are means there is no model.
//! - **Three values:** an instance is worked out with the cells set so far,
//!   true, false or not known yet. False is a dead end. Not known, with one
//!   cell it reads still open, strikes off that cell's values that would
//!   make it false.
//! - **One-to-one** is kept as a whole: a value given to one argument is
//!   struck off the others'. **Symmetric** functions have one cell for
//!   `f(a, b)` and `f(b, a)`. **Onto** is checked of each model.
//! - **The search** sets the open cell with the fewest values left, each in
//!   turn, and keeps the first models found; it goes on to count all of them
//!   where it can within the time given. A sort of a range of sizes is
//!   searched smallest first.
//!
//! Nothing here comes from FINDER: it is the plain backtracking search with
//! forward checking that constraint solvers have used since the seventies.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::ast::*;
use crate::eval::{domain, Model};
use crate::types::{Checked, Sig, Ty};

/// No more cells than this are searched.
pub const MOST_CELLS: usize = 200_000;
/// No more instances than this are made from the constraints.
pub const MOST_INSTANCES: usize = 2_000_000;

/// What a search found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// The first models, as many as were asked to be kept.
    pub models: Vec<Model>,
    /// How many models were found in all.
    pub found: u64,
    /// Whether the search went through everything: `found` is every model.
    pub finished: bool,
    /// Whether it stopped for want of time, rather than having enough.
    pub timed_out: bool,
    /// How many times the search chose a value.
    pub choices: u64,
    pub elapsed_ms: u64,
}

/// A value that is not there: a partial function's, at an argument it has none.
const NONE: i64 = i64::MIN + 1;

/* ── compiling ────────────────────────────────────────────────────── */

#[derive(Debug, Clone)]
enum C {
    Const(i64),
    Var(usize),
    /// A symbol applied: the symbol, its arguments.
    Cell(usize, Vec<C>),
    Not(Box<C>),
    Bin(Op, Box<C>, Box<C>),
    Neg(Box<C>),
    Abs(Box<C>),
    MinMax(bool, Box<C>, Box<C>),
    /// `succ` (true) or `pred` within a domain.
    Step(bool, Box<C>, Rc<Vec<i64>>),
    Defined(Box<C>),
    /// A quantifier over one slot's domain.
    Quant(Quant, usize, Rc<Vec<i64>>, Box<C>),
}

/// A symbol, as cells: the domains of its arguments and of its value.
#[derive(Debug)]
struct Sym {
    name: String,
    args: Vec<Rc<Vec<i64>>>,
    /// The values it may take, the last `NONE` when it is partial.
    values: Rc<Vec<i64>>,
    first: usize,
    count: usize,
    props: Props,
}

impl Sym {
    /// The cell of these arguments' values, if they are in its domains.
    fn cell(&self, args: &[i64]) -> Option<usize> {
        let mut idx = 0usize;
        let mut pos: Vec<usize> = Vec::with_capacity(args.len());
        for (a, d) in args.iter().zip(&self.args) {
            pos.push(position(d, *a)?);
        }
        // Either order of a symmetric pair is the one cell.
        if self.props.symmetric && pos.len() == 2 && pos[0] > pos[1] {
            pos.swap(0, 1);
        }
        for (p, d) in pos.iter().zip(&self.args) {
            idx = idx * d.len() + p;
        }
        Some(self.first + idx)
    }
}

fn position(d: &[i64], v: i64) -> Option<usize> {
    // Domains are sorted runs: a range's numbers, or 0..n.
    let first = *d.first()?;
    let k = v.checked_sub(first)?;
    (k >= 0 && (k as usize) < d.len() && d[k as usize] == v).then_some(k as usize)
}

struct Compiler<'a> {
    sig: &'a Sig,
    model: &'a Model,
    syms: Vec<Sym>,
    by_name: BTreeMap<String, usize>,
    /// The variables in scope: name, slot and sort.
    scope: Vec<(String, usize, Ty)>,
    slots: usize,
}

impl Compiler<'_> {
    fn dom(&self, t: &Ty) -> Rc<Vec<i64>> {
        Rc::new(domain(self.sig, self.model, t))
    }

    fn ty_of(&self, e: &Expr) -> Option<Ty> {
        match e {
            Expr::Name(n) => {
                if let Some((_, _, t)) = self.scope.iter().rev().find(|(v, _, _)| v == n) {
                    return Some(t.clone());
                }
                if let Some((s, _)) = self.sig.elements.get(n) {
                    return Some(Ty::Sort(s.clone()));
                }
                self.sig.symbols.get(n).map(|s| s.result.clone())
            }
            Expr::App(f, args) => match f.as_str() {
                "succ" | "pred" | "min" | "max" => args.first().and_then(|a| self.ty_of(a)),
                f => self.sig.symbols.get(f).map(|s| s.result.clone()),
            },
            Expr::End { sort, .. } => Some(crate::eval::ty_named(sort)),
            Expr::Num(_) => Some(Ty::Int),
            _ => None,
        }
    }

    fn compile(&mut self, e: &Expr) -> Result<C, String> {
        Ok(match e {
            Expr::Num(n) => C::Const(*n),
            Expr::Bool(b) => C::Const(*b as i64),
            Expr::Name(n) => {
                if let Some((_, slot, _)) = self.scope.iter().rev().find(|(v, _, _)| v == n) {
                    return Ok(C::Var(*slot));
                }
                if let Some((_, k)) = self.sig.elements.get(n) {
                    return Ok(C::Const(*k as i64));
                }
                match self.by_name.get(n) {
                    Some(&s) => C::Cell(s, Vec::new()),
                    None => return Err(format!("{n} has no value")),
                }
            }
            Expr::End { last, sort } => {
                let d = domain(self.sig, self.model, &crate::eval::ty_named(sort));
                C::Const(*(if *last { d.last() } else { d.first() }).ok_or_else(|| format!("{sort} is empty"))?)
            }
            Expr::Not(a) => C::Not(Box::new(self.compile(a)?)),
            Expr::Neg(a) => C::Neg(Box::new(self.compile(a)?)),
            Expr::Bin(op, a, b) => C::Bin(*op, Box::new(self.compile(a)?), Box::new(self.compile(b)?)),
            Expr::App(f, args) => match f.as_str() {
                "abs" => C::Abs(Box::new(self.compile(&args[0])?)),
                "min" | "max" => C::MinMax(f == "max", Box::new(self.compile(&args[0])?), Box::new(self.compile(&args[1])?)),
                "succ" | "pred" => {
                    let t = self.ty_of(&args[0]).ok_or_else(|| format!("cannot tell what {f} steps through"))?;
                    C::Step(f == "succ", Box::new(self.compile(&args[0])?), self.dom(&t))
                }
                "defined" => C::Defined(Box::new(self.compile(&args[0])?)),
                name => {
                    let s = *self.by_name.get(name).ok_or_else(|| format!("{name} is not declared"))?;
                    C::Cell(s, args.iter().map(|a| self.compile(a)).collect::<Result<_, _>>()?)
                }
            },
            Expr::Quant(q, binders, body) => {
                // Several binders are nested quantifiers, the first outermost;
                // a counting quantifier has one.
                let n = self.scope.len();
                let mut slots = Vec::new();
                for b in binders {
                    let t = crate::eval::ty_named(b.sort.as_deref().unwrap_or("int"));
                    let slot = self.slots;
                    self.slots += 1;
                    slots.push((slot, self.dom(&t)));
                    self.scope.push((b.name.clone(), slot, t));
                }
                let mut c = self.compile(body)?;
                self.scope.truncate(n);
                for (slot, d) in slots.into_iter().rev() {
                    c = C::Quant(*q, slot, d, Box::new(c));
                }
                c
            }
        })
    }
}

/* ── working out, three ways ──────────────────────────────────────── */

/// A term's value: known, a partial function's missing value, or not known yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V {
    Is(i64),
    Gone,
    Open,
}

const UNSET: i64 = i64::MIN;

struct Space {
    syms: Vec<Sym>,
    /// Each cell's symbol.
    owner: Vec<usize>,
    /// Each cell's possible values, by position in its symbol's values.
    dom: Vec<Vec<bool>>,
    left: Vec<u32>,
    /// Each cell's value once it has one left; `UNSET` before.
    val: Vec<i64>,
}

impl Space {
    fn eval(&self, c: &C, env: &mut Vec<i64>) -> V {
        match c {
            C::Const(v) => V::Is(*v),
            C::Var(s) => V::Is(env[*s]),
            C::Cell(s, args) => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    match self.eval(a, env) {
                        V::Is(v) => vals.push(v),
                        other => return other,
                    }
                }
                match self.syms[*s].cell(&vals) {
                    None => V::Gone,
                    Some(cell) => match self.val[cell] {
                        UNSET => V::Open,
                        NONE => V::Gone,
                        v => V::Is(v),
                    },
                }
            }
            C::Not(a) => match self.truth(a, env) {
                Some(b) => V::Is(!b as i64),
                None => V::Open,
            },
            C::Bin(op, a, b) if op.is_logical() => {
                let x = self.truth(a, env);
                // The left settles some without the right.
                match (op, x) {
                    (Op::And, Some(false)) => return V::Is(0),
                    (Op::Or, Some(true)) => return V::Is(1),
                    (Op::Imp, Some(false)) => return V::Is(1),
                    _ => {}
                }
                let y = self.truth(b, env);
                let r = match (op, x, y) {
                    (Op::And, _, Some(false)) => Some(false),
                    (Op::And, Some(true), Some(true)) => Some(true),
                    (Op::Or, _, Some(true)) => Some(true),
                    (Op::Or, Some(false), Some(false)) => Some(false),
                    (Op::Imp, _, Some(true)) => Some(true),
                    (Op::Imp, Some(true), Some(false)) => Some(false),
                    (Op::Iff, Some(p), Some(q)) => Some(p == q),
                    (Op::Xor, Some(p), Some(q)) => Some(p != q),
                    _ => None,
                };
                r.map_or(V::Open, |b| V::Is(b as i64))
            }
            C::Bin(op, a, b) => {
                let (x, y) = (self.eval(a, env), self.eval(b, env));
                if x == V::Open || y == V::Open {
                    return V::Open;
                }
                let (V::Is(x), V::Is(y)) = (x, y) else {
                    // No comparison holds of a missing value; arithmetic on one has none.
                    return if op.level() == 6 { V::Is(0) } else { V::Gone };
                };
                V::Is(match op {
                    Op::Eq => (x == y) as i64,
                    Op::Ne => (x != y) as i64,
                    Op::Lt => (x < y) as i64,
                    Op::Le => (x <= y) as i64,
                    Op::Gt => (x > y) as i64,
                    Op::Ge => (x >= y) as i64,
                    Op::Add => x.saturating_add(y),
                    Op::Sub => x.saturating_sub(y),
                    _ => x.saturating_mul(y),
                })
            }
            C::Neg(a) => match self.eval(a, env) {
                V::Is(v) => V::Is(-v),
                other => other,
            },
            C::Abs(a) => match self.eval(a, env) {
                V::Is(v) => V::Is(v.abs()),
                other => other,
            },
            C::MinMax(max, a, b) => match (self.eval(a, env), self.eval(b, env)) {
                (V::Is(x), V::Is(y)) => V::Is(if *max { x.max(y) } else { x.min(y) }),
                (V::Open, _) | (_, V::Open) => V::Open,
                _ => V::Gone,
            },
            C::Step(up, a, d) => match self.eval(a, env) {
                V::Is(v) => {
                    let n = if *up { v + 1 } else { v - 1 };
                    if position(d, n).is_some() {
                        V::Is(n)
                    } else {
                        V::Gone
                    }
                }
                other => other,
            },
            C::Defined(a) => match self.eval(a, env) {
                V::Is(_) => V::Is(1),
                V::Gone => V::Is(0),
                V::Open => V::Open,
            },
            C::Quant(q, slot, d, body) => {
                let (mut yes, mut open) = (0usize, 0usize);
                for v in d.iter() {
                    env[*slot] = *v;
                    match self.truth(body, env) {
                        Some(true) => yes += 1,
                        None => open += 1,
                        Some(false) => {
                            if *q == Quant::All {
                                return V::Is(0);
                            }
                        }
                    }
                    // Settled before the end: enough found, or too few can be.
                    let settled = match q {
                        Quant::Some => yes >= 1,
                        Quant::AtLeast(m) => yes >= *m as usize,
                        Quant::AtMost(m) | Quant::Exactly(m) => yes > *m as usize,
                        Quant::All => false,
                    };
                    if settled {
                        return V::Is(matches!(q, Quant::Some | Quant::AtLeast(_)) as i64);
                    }
                }
                let (lo, hi) = (yes, yes + open);
                let r = match q {
                    Quant::All => (open == 0).then_some(true),
                    Quant::Some => (hi == 0).then_some(false),
                    Quant::AtLeast(m) => {
                        let m = *m as usize;
                        if lo >= m {
                            Some(true)
                        } else if hi < m {
                            Some(false)
                        } else {
                            None
                        }
                    }
                    Quant::AtMost(m) => {
                        let m = *m as usize;
                        if hi <= m {
                            Some(true)
                        } else if lo > m {
                            Some(false)
                        } else {
                            None
                        }
                    }
                    Quant::Exactly(m) => {
                        let m = *m as usize;
                        if lo > m || hi < m {
                            Some(false)
                        } else if lo == m && hi == m {
                            Some(true)
                        } else {
                            None
                        }
                    }
                };
                r.map_or(V::Open, |b| V::Is(b as i64))
            }
        }
    }

    /// True, false, or not known; a missing value is false.
    fn truth(&self, c: &C, env: &mut Vec<i64>) -> Option<bool> {
        match self.eval(c, env) {
            V::Is(v) => Some(v != 0),
            V::Gone => Some(false),
            V::Open => None,
        }
    }

    /// The cells an instance reads, as far as its own arguments say: those
    /// read through another cell's value are its symbol's, `whole`.
    fn reads(&self, c: &C, env: &mut Vec<i64>, cells: &mut Vec<usize>, whole: &mut Vec<usize>, budget: &mut usize) {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        match c {
            C::Const(_) | C::Var(_) => {}
            C::Cell(s, args) => {
                let mut vals = Vec::new();
                let mut fixed = true;
                for a in args {
                    self.reads(a, env, cells, whole, budget);
                    match self.static_value(a, env) {
                        Some(v) => vals.push(v),
                        None => fixed = false,
                    }
                }
                if fixed {
                    if let Some(cell) = self.syms[*s].cell(&vals) {
                        if !cells.contains(&cell) {
                            cells.push(cell);
                        }
                    }
                } else if !whole.contains(s) {
                    whole.push(*s);
                }
            }
            C::Not(a) | C::Neg(a) | C::Abs(a) | C::Defined(a) | C::Step(_, a, _) => self.reads(a, env, cells, whole, budget),
            C::Bin(_, a, b) | C::MinMax(_, a, b) => {
                self.reads(a, env, cells, whole, budget);
                self.reads(b, env, cells, whole, budget);
            }
            C::Quant(_, slot, d, body) => {
                for v in d.iter() {
                    env[*slot] = *v;
                    self.reads(body, env, cells, whole, budget);
                }
            }
        }
    }

    /// A term's value from constants and variables alone, if it has one.
    fn static_value(&self, c: &C, env: &mut Vec<i64>) -> Option<i64> {
        match c {
            C::Const(v) => Some(*v),
            C::Var(s) => Some(env[*s]),
            C::Neg(a) => self.static_value(a, env).map(|v| -v),
            C::Abs(a) => self.static_value(a, env).map(|v| v.abs()),
            C::Bin(op @ (Op::Add | Op::Sub | Op::Mul), a, b) => {
                let (x, y) = (self.static_value(a, env)?, self.static_value(b, env)?);
                Some(match op {
                    Op::Add => x.saturating_add(y),
                    Op::Sub => x.saturating_sub(y),
                    _ => x.saturating_mul(y),
                })
            }
            C::Step(up, a, d) => {
                let v = self.static_value(a, env)?;
                let n = if *up { v + 1 } else { v - 1 };
                position(d, n).map(|_| n)
            }
            _ => None,
        }
    }
}

/// An instance: a constraint's compiled body, and its top variables' values.
struct Inst {
    body: Rc<C>,
    env: Vec<i64>,
    /// The cells it reads, when they can be told from its arguments, and
    /// the symbols it reads through another cell's value.
    cells: Vec<usize>,
    whole: Vec<usize>,
}

/* ── the search ───────────────────────────────────────────────────── */

struct Search<'a> {
    space: Space,
    insts: Vec<Inst>,
    /// For each cell, the instances that read it; for each symbol, those
    /// that read it through another cell.
    watch: Vec<Vec<usize>>,
    watch_sym: Vec<Vec<usize>>,
    /// Domain removals, to undo: (cell, position).
    trail: Vec<(usize, usize)>,
    slots: usize,
    keep: usize,
    /// Whether to go on past the models kept, counting.
    count: bool,
    deadline: Instant,
    /// Set from outside to stop the search where it is.
    stop: &'a AtomicBool,
    out: Outcome,
    stopped: bool,
    sizes: BTreeMap<String, u32>,
    sig: Sig,
}

impl Search<'_> {
    fn value_at(&self, cell: usize, pos: usize) -> i64 {
        self.space.syms[self.space.owner[cell]].values[pos]
    }

    fn remove(&mut self, cell: usize, pos: usize, queue: &mut Vec<usize>) -> bool {
        if !self.space.dom[cell][pos] {
            return true;
        }
        self.space.dom[cell][pos] = false;
        self.space.left[cell] -= 1;
        self.trail.push((cell, pos));
        match self.space.left[cell] {
            0 => false,
            1 => {
                let p = self.space.dom[cell].iter().position(|b| *b).unwrap();
                self.space.val[cell] = self.value_at(cell, p);
                queue.push(cell);
                true
            }
            _ => true,
        }
    }

    /// The values of the open cell `u` that make an instance false, struck
    /// off: every value tried first, then those struck, so a value left
    /// alone is not lost to the trying.
    fn prune(&mut self, u: usize, body: &C, ienv: &[i64], env: &mut Vec<i64>, queue: &mut Vec<usize>) -> bool {
        let mut bad = Vec::new();
        for p in 0..self.space.dom[u].len() {
            if !self.space.dom[u][p] {
                continue;
            }
            self.space.val[u] = self.value_at(u, p);
            env[..ienv.len()].copy_from_slice(ienv);
            if self.space.truth(body, env) == Some(false) {
                bad.push(p);
            }
        }
        self.space.val[u] = UNSET;
        bad.into_iter().all(|p| self.remove(u, p, queue))
    }

    fn undo(&mut self, to: usize) {
        while self.trail.len() > to {
            let (cell, pos) = self.trail.pop().unwrap();
            self.space.dom[cell][pos] = true;
            self.space.left[cell] += 1;
            if self.space.left[cell] > 1 {
                self.space.val[cell] = UNSET;
            }
        }
    }

    /// What the cells in `queue` being set means for the rest; false at a
    /// dead end.
    fn propagate(&mut self, mut queue: Vec<usize>) -> bool {
        let mut env = vec![0i64; self.slots];
        while let Some(cell) = queue.pop() {
            let s = self.space.owner[cell];
            // One-to-one: the value is no one else's.
            if self.space.syms[s].props.one_to_one && self.space.val[cell] != NONE {
                let (first, count) = (self.space.syms[s].first, self.space.syms[s].count);
                let pos = self.space.dom[cell].iter().position(|b| *b).unwrap();
                for other in first..first + count {
                    if other != cell && !self.remove(other, pos, &mut queue) {
                        return false;
                    }
                }
            }
            let mut todo: Vec<usize> = self.watch[cell].clone();
            todo.extend(self.watch_sym[s].iter().copied());
            for i in todo {
                let inst = &self.insts[i];
                env[..inst.env.len()].copy_from_slice(&inst.env);
                match self.space.truth(&inst.body, &mut env) {
                    Some(true) => {}
                    Some(false) => return false,
                    None => {
                        // One cell left open: its values that would make it false go.
                        let open: Vec<usize> = inst.cells.iter().copied().filter(|c| self.space.val[*c] == UNSET).collect();
                        if open.len() != 1 {
                            continue;
                        }
                        let u = open[0];
                        let body = inst.body.clone();
                        let ienv = inst.env.clone();
                        if !self.prune(u, &body, &ienv, &mut env, &mut queue) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn search(&mut self) {
        if self.stopped {
            return;
        }
        if self.out.choices % 256 == 0 {
            if self.stop.load(Ordering::Relaxed) {
                self.stopped = true;
                return;
            }
            if Instant::now() >= self.deadline {
                self.stopped = true;
                self.out.timed_out = true;
                return;
            }
        }
        // The open cell with the fewest values left.
        let mut best: Option<(u32, usize)> = None;
        for (c, &n) in self.space.left.iter().enumerate() {
            if n > 1 && best.is_none_or(|(m, _)| n < m) {
                best = Some((n, c));
                if n == 2 {
                    break;
                }
            }
        }
        let Some((_, cell)) = best else {
            self.found();
            return;
        };
        let positions: Vec<usize> = (0..self.space.dom[cell].len()).filter(|p| self.space.dom[cell][*p]).collect();
        for p in positions {
            if self.stopped {
                return;
            }
            self.out.choices += 1;
            let mark = self.trail.len();
            let mut queue = Vec::new();
            let mut ok = true;
            for q in 0..self.space.dom[cell].len() {
                if q != p && !self.remove(cell, q, &mut queue) {
                    ok = false;
                    break;
                }
            }
            if ok && self.propagate(queue) {
                self.search();
            }
            self.undo(mark);
        }
    }

    /// Every cell set: a model, if every instance holds and every onto
    /// function reaches each value.
    fn found(&mut self) {
        let mut env = vec![0i64; self.slots];
        for inst in &self.insts {
            env[..inst.env.len()].copy_from_slice(&inst.env);
            if self.space.truth(&inst.body, &mut env) != Some(true) {
                return;
            }
        }
        for s in &self.space.syms {
            if s.props.onto {
                let got: Vec<i64> = (s.first..s.first + s.count).map(|c| self.space.val[c]).collect();
                if s.values.iter().any(|v| *v != NONE && !got.contains(v)) {
                    return;
                }
            }
        }
        self.out.found += 1;
        if self.out.models.len() < self.keep {
            let mut m = Model { sizes: self.sizes.clone(), values: BTreeMap::new() };
            for s in &self.space.syms {
                let table = m.values.entry(s.name.clone()).or_default();
                for (args, cell) in arguments(&s.args).into_iter().filter_map(|a| s.cell(&a).map(|c| (a, c))) {
                    let v = self.space.val[cell];
                    if v != NONE {
                        table.insert(args, v);
                    }
                }
            }
            debug_assert!(crate::eval::broken(&Checked { theory: Theory::default(), sig: self.sig.clone() }, &m).is_ok());
            self.out.models.push(m);
        }
        if !self.count && self.out.models.len() >= self.keep {
            self.stopped = true;
        }
    }
}

fn arguments(args: &[Rc<Vec<i64>>]) -> Vec<Vec<i64>> {
    let mut out = vec![Vec::new()];
    for d in args {
        out = out.into_iter().flat_map(|row| d.iter().map(move |v| [row.clone(), vec![*v]].concat())).collect();
    }
    out
}

/// A theory's models: the first `keep` of them, and — when `count` — how
/// many there are, as far as `time` allows.
pub fn solve(c: &Checked, keep: usize, count: bool, time: Duration) -> Result<Outcome, String> {
    solve_or_stop(c, keep, count, time, &AtomicBool::new(false))
}

/// The same, stopped where it is when `stop` is set.
pub fn solve_or_stop(c: &Checked, keep: usize, count: bool, time: Duration, stop: &AtomicBool) -> Result<Outcome, String> {
    let started = Instant::now();
    let deadline = started + time;
    // Each sort of a range of sizes, smallest first: every combination.
    let ranged: Vec<(String, u32, u32)> = c
        .sig
        .sorts
        .iter()
        .filter_map(|(n, k)| match k {
            SortKind::Sizes(a, b) => Some((n.clone(), *a, *b)),
            _ => None,
        })
        .collect();
    let mut combos: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new()];
    for (n, a, b) in &ranged {
        combos = combos.into_iter().flat_map(|m| (*a..=*b).map(move |s| {
            let mut m = m.clone();
            m.insert(n.clone(), s);
            m
        })).collect();
    }
    combos.sort_by_key(|m| m.values().map(|v| *v as u64).sum::<u64>());
    let mut total = Outcome { finished: true, ..Outcome::default() };
    for sizes in combos {
        let left = deadline.saturating_duration_since(Instant::now());
        let want = keep.saturating_sub(total.models.len());
        if !count && want == 0 {
            total.finished = false;
            break;
        }
        let out = solve_sized(c, sizes, want, count, left, stop)?;
        total.found += out.found;
        total.choices += out.choices;
        total.models.extend(out.models);
        if !out.finished {
            total.finished = false;
            total.timed_out = out.timed_out;
            break;
        }
    }
    total.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(total)
}

fn solve_sized(c: &Checked, sizes: BTreeMap<String, u32>, keep: usize, count: bool, time: Duration, stop: &AtomicBool) -> Result<Outcome, String> {
    let model = Model { sizes: sizes.clone(), values: BTreeMap::new() };
    let mut comp = Compiler { sig: &c.sig, model: &model, syms: Vec::new(), by_name: BTreeMap::new(), scope: Vec::new(), slots: 0 };
    // The cells: every symbol, and every proposition used undeclared.
    let mut first = 0usize;
    let mut decls: Vec<(String, Vec<Ty>, Ty, Props)> = c.sig.symbols.iter().map(|(n, s)| (n.clone(), s.args.clone(), s.result.clone(), s.props)).collect();
    decls.extend(c.sig.propositions.iter().map(|p| (p.clone(), Vec::new(), Ty::Bool, Props::default())));
    for (name, args, result, props) in decls {
        let arg_doms: Vec<Rc<Vec<i64>>> = args.iter().map(|t| comp.dom(t)).collect();
        let mut values = domain(&c.sig, &model, &result);
        if props.partial {
            values.push(NONE);
        }
        let count: usize = arg_doms.iter().map(|d| d.len()).product();
        if first + count > MOST_CELLS {
            return Err(format!("more than {MOST_CELLS} values to find: make the sorts smaller"));
        }
        comp.by_name.insert(name.clone(), comp.syms.len());
        if props.symmetric && props.one_to_one {
            return Err(format!("{name} cannot be both symmetric and one-to-one"));
        }
        comp.syms.push(Sym { name, args: arg_doms, values: Rc::new(values), first, count, props });
        first += count;
    }
    // The constraints, compiled, and each top `all` spread into instances.
    let mut bodies: Vec<(Rc<C>, Vec<i64>)> = Vec::new();
    for line in &c.theory.lines {
        if let Some(Item::Constraint(e)) = &line.item {
            let compiled = comp.compile(e).map_err(|m| format!("line {}: {m}", line.at))?;
            bodies.push((Rc::new(compiled), Vec::new()));
        }
    }
    let slots = comp.slots.max(1);
    let syms = comp.syms;
    let cells = first;
    let mut owner = vec![0usize; cells];
    for (i, s) in syms.iter().enumerate() {
        owner[s.first..s.first + s.count].fill(i);
    }
    let dom: Vec<Vec<bool>> = (0..cells).map(|c| vec![true; syms[owner[c]].values.len()]).collect();
    let left: Vec<u32> = dom.iter().map(|d| d.len() as u32).collect();
    let mut space = Space { syms, owner, dom, left, val: vec![UNSET; cells] };
    // A symmetric function's cells for its arguments the other way round are
    // never read: given one value, they are not searched.
    for s in space.syms.iter().filter(|s| s.props.symmetric) {
        let n = s.args[0].len();
        for i in 0..n {
            for j in 0..i {
                let c = s.first + i * n + j;
                space.dom[c].iter_mut().skip(1).for_each(|b| *b = false);
                space.left[c] = 1;
            }
        }
    }
    for c in 0..cells {
        if space.left[c] == 1 {
            space.val[c] = space.syms[space.owner[c]].values[0];
        }
        if space.left[c] == 0 {
            return Ok(Outcome { finished: true, ..Outcome::default() });
        }
    }
    let mut insts: Vec<Inst> = Vec::new();
    let mut env = vec![0i64; slots];
    let mut stack: Vec<(Rc<C>, Vec<i64>)> = bodies;
    while let Some((body, fixed)) = stack.pop() {
        if insts.len() > MOST_INSTANCES {
            return Err(format!("more than {MOST_INSTANCES} instances of the constraints: make the sorts smaller"));
        }
        // An `all` at the top: an instance a value. `and` at the top: two.
        match &*body {
            C::Quant(Quant::All, slot, d, inner) => {
                let inner: Rc<C> = Rc::new((**inner).clone());
                for v in d.iter() {
                    let mut f = fixed.clone();
                    if f.len() <= *slot {
                        f.resize(slot + 1, 0);
                    }
                    f[*slot] = *v;
                    stack.push((inner.clone(), f));
                }
                continue;
            }
            C::Bin(Op::And, a, b) => {
                stack.push((Rc::new((**a).clone()), fixed.clone()));
                stack.push((Rc::new((**b).clone()), fixed));
                continue;
            }
            _ => {}
        }
        env[..fixed.len()].copy_from_slice(&fixed);
        match space.truth(&body, &mut env) {
            Some(true) => continue,
            Some(false) => return Ok(Outcome { finished: true, ..Outcome::default() }),
            None => {}
        }
        let (mut cells_read, mut whole) = (Vec::new(), Vec::new());
        let mut budget = 100_000;
        env[..fixed.len()].copy_from_slice(&fixed);
        space.reads(&body, &mut env, &mut cells_read, &mut whole, &mut budget);
        if budget == 0 {
            whole = (0..space.syms.len()).collect();
        }
        insts.push(Inst { body, env: fixed, cells: cells_read, whole });
    }
    let mut watch = vec![Vec::new(); cells];
    let mut watch_sym = vec![Vec::new(); space.syms.len()];
    for (i, inst) in insts.iter().enumerate() {
        for &c in &inst.cells {
            watch[c].push(i);
        }
        for &s in &inst.whole {
            watch_sym[s].push(i);
        }
    }
    let mut search = Search {
        space,
        insts,
        watch,
        watch_sym,
        trail: Vec::new(),
        slots,
        keep,
        count,
        deadline: Instant::now() + time,
        stop,
        out: Outcome::default(),
        stopped: false,
        sizes,
        sig: c.sig.clone(),
    };
    // What the instances say before anything is chosen: each looked at once.
    let all: Vec<usize> = (0..cells).collect();
    let fixed: Vec<usize> = all.iter().copied().filter(|c| search.space.val[*c] != UNSET).collect();
    if search.first_look() && search.propagate(fixed) {
        search.search();
    }
    search.out.finished = !search.stopped;
    Ok(search.out)
}

impl Search<'_> {
    /// Each instance looked at before the search: one with a single cell
    /// open strikes off that cell's values it rules out.
    fn first_look(&mut self) -> bool {
        let mut queue = Vec::new();
        let mut env = vec![0i64; self.slots];
        for i in 0..self.insts.len() {
            let open: Vec<usize> = self.insts[i].cells.iter().copied().filter(|c| self.space.val[*c] == UNSET).collect();
            if open.len() != 1 {
                continue;
            }
            let u = open[0];
            let body = self.insts[i].body.clone();
            let ienv = self.insts[i].env.clone();
            if !self.prune(u, &body, &ienv, &mut env, &mut queue) {
                return false;
            }
        }
        self.propagate(queue)
    }
}
