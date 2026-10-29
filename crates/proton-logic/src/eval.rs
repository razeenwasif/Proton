//! Formulas worked out in a model: each sort's elements, each symbol's
//! table. Values are numbers: a number is itself, an element its place in
//! its sort, true 1 and false 0.
//!
//! A partial function may have no value at an argument. A comparison with
//! a value that is not there is false, arithmetic on it has no value, and
//! `defined(t)` says whether there is one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ast::*;
use crate::types::{Checked, Sig, Ty};

/// What the numbers range over when nothing says: Logic4Fun's 30.
pub const INT_DEFAULT: (i64, i64) = (0, 29);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    /// The size chosen for each sort declared with a range of sizes.
    pub sizes: BTreeMap<String, u32>,
    /// Each symbol's table: arguments to value. A constant's or
    /// proposition's is at no arguments.
    pub values: BTreeMap<String, BTreeMap<Vec<i64>, i64>>,
}

impl Model {
    pub fn set(&mut self, symbol: &str, args: Vec<i64>, value: i64) {
        self.values.entry(symbol.to_string()).or_default().insert(args, value);
    }
}

/// The values a type has in a model.
pub fn domain(sig: &Sig, model: &Model, t: &Ty) -> Vec<i64> {
    match t {
        Ty::Bool => vec![0, 1],
        Ty::Int => {
            let (a, b) = match sig.sorts.get("int") {
                Some(SortKind::Range(a, b)) => (*a, *b),
                _ => INT_DEFAULT,
            };
            (a..=b).collect()
        }
        Ty::Sort(s) => match sig.sorts.get(s) {
            Some(SortKind::Range(a, b)) => (*a..=*b).collect(),
            Some(SortKind::Named(n) | SortKind::Ordered(n)) => (0..n.len() as i64).collect(),
            Some(SortKind::Size(n)) => (0..*n as i64).collect(),
            Some(SortKind::Sizes(a, _)) => (0..model.sizes.get(s).copied().unwrap_or(*a) as i64).collect(),
            None => Vec::new(),
        },
    }
}

pub fn ty_named(sort: &str) -> Ty {
    match sort {
        "bool" => Ty::Bool,
        "int" => Ty::Int,
        s => Ty::Sort(s.to_string()),
    }
}

/// A value as it is written: an element's name, a number, true or false.
pub fn show(sig: &Sig, t: &Ty, v: i64) -> String {
    match t {
        Ty::Bool => (v != 0).to_string(),
        Ty::Int => v.to_string(),
        Ty::Sort(s) => match sig.sorts.get(s) {
            Some(SortKind::Named(n) | SortKind::Ordered(n)) => n.get(v as usize).cloned().unwrap_or_else(|| v.to_string()),
            Some(SortKind::Size(_) | SortKind::Sizes(..)) => format!("{s}{}", v + 1),
            _ => v.to_string(),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V {
    Is(i64),
    /// A partial function's value that is not there.
    None,
}

struct Eval<'a> {
    sig: &'a Sig,
    model: &'a Model,
    /// Variables in scope: name, value and sort.
    vars: Vec<(String, i64, String)>,
}

/// Whether a checked formula holds in the model.
pub fn holds(sig: &Sig, model: &Model, e: &Expr) -> Result<bool, String> {
    let mut ev = Eval { sig, model, vars: Vec::new() };
    ev.truth(e)
}

/// The value of a closed term, `None` where a partial function has none.
pub fn value(sig: &Sig, model: &Model, e: &Expr) -> Result<Option<i64>, String> {
    let mut ev = Eval { sig, model, vars: Vec::new() };
    Ok(match ev.val(e)? {
        V::Is(v) => Some(v),
        V::None => None,
    })
}

/// The lines of a theory its model breaks, with why: a constraint false, or
/// a symbol's table not of its kind. Empty when the model is one.
pub fn broken(c: &Checked, model: &Model) -> Result<Vec<(usize, String)>, String> {
    let mut out = Vec::new();
    for line in &c.theory.lines {
        match &line.item {
            Some(Item::Constraint(e)) => {
                if !holds(&c.sig, model, e)? {
                    out.push((line.at, "is false".to_string()));
                }
            }
            Some(Item::Symbol(s)) => {
                for name in &s.names {
                    if let Some(why) = kind_broken(&c.sig, model, name) {
                        out.push((line.at, why));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Every argument a symbol takes, in order.
pub fn arguments(sig: &Sig, model: &Model, args: &[Ty]) -> Vec<Vec<i64>> {
    let mut out = vec![Vec::new()];
    for t in args {
        let d = domain(sig, model, t);
        out = out.into_iter().flat_map(|row| d.iter().map(move |v| [row.clone(), vec![*v]].concat())).collect();
    }
    out
}

fn kind_broken(sig: &Sig, model: &Model, name: &str) -> Option<String> {
    let s = sig.symbols.get(name)?;
    let table = model.values.get(name).cloned().unwrap_or_default();
    let range = domain(sig, model, &s.result);
    let mut seen: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for args in arguments(sig, model, &s.args) {
        match table.get(&args) {
            None if !s.props.partial => return Some(format!("{name} has no value at ({})", join(sig, &s.args, &args))),
            None => {}
            Some(v) => {
                if !range.contains(v) {
                    return Some(format!("{name}({}) is outside {}", join(sig, &s.args, &args), s.result));
                }
                if s.props.one_to_one {
                    if let Some(other) = seen.get(v) {
                        return Some(format!("{name} is not one-to-one: ({}) and ({}) both give {}", join(sig, &s.args, other), join(sig, &s.args, &args), show(sig, &s.result, *v)));
                    }
                }
                seen.insert(*v, args.clone());
                if s.props.symmetric && table.get(&vec![args[1], args[0]]) != Some(v) {
                    return Some(format!("{name} is not symmetric at ({})", join(sig, &s.args, &args)));
                }
            }
        }
    }
    if s.props.onto {
        if let Some(v) = range.iter().find(|v| !seen.contains_key(v)) {
            return Some(format!("{name} is not onto: nothing gives {}", show(sig, &s.result, *v)));
        }
    }
    None
}

fn join(sig: &Sig, tys: &[Ty], args: &[i64]) -> String {
    tys.iter().zip(args).map(|(t, v)| show(sig, t, *v)).collect::<Vec<_>>().join(", ")
}

impl Eval<'_> {
    fn truth(&mut self, e: &Expr) -> Result<bool, String> {
        Ok(match self.val(e)? {
            V::Is(v) => v != 0,
            V::None => false,
        })
    }

    fn table(&self, name: &str, args: &[i64]) -> Result<V, String> {
        let partial = self.sig.symbols.get(name).is_some_and(|s| s.props.partial);
        match self.model.values.get(name).and_then(|t| t.get(args)) {
            Some(v) => Ok(V::Is(*v)),
            None if partial => Ok(V::None),
            None if args.is_empty() => Err(format!("the model gives {name} no value")),
            None => Err(format!("the model gives {name} no value at ({})", args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "))),
        }
    }

    fn val(&mut self, e: &Expr) -> Result<V, String> {
        Ok(match e {
            Expr::Num(n) => V::Is(*n),
            Expr::Bool(b) => V::Is(*b as i64),
            Expr::Name(n) => {
                if let Some((_, v, _)) = self.vars.iter().rev().find(|(x, _, _)| x == n) {
                    return Ok(V::Is(*v));
                }
                if let Some((_, k)) = self.sig.elements.get(n) {
                    return Ok(V::Is(*k as i64));
                }
                self.table(n, &[])?
            }
            Expr::End { last, sort } => {
                let d = domain(self.sig, self.model, &ty_named(sort));
                let v = if *last { d.last() } else { d.first() };
                v.map_or(V::None, |v| V::Is(*v))
            }
            Expr::Not(a) => V::Is(!self.truth(a)? as i64),
            Expr::Neg(a) => match self.val(a)? {
                V::Is(v) => V::Is(-v),
                V::None => V::None,
            },
            Expr::Bin(op, a, b) => match op {
                Op::And => V::Is((self.truth(a)? && self.truth(b)?) as i64),
                Op::Or => V::Is((self.truth(a)? || self.truth(b)?) as i64),
                Op::Xor => V::Is((self.truth(a)? != self.truth(b)?) as i64),
                Op::Imp => V::Is((!self.truth(a)? || self.truth(b)?) as i64),
                Op::Iff => V::Is((self.truth(a)? == self.truth(b)?) as i64),
                _ => {
                    let (x, y) = (self.val(a)?, self.val(b)?);
                    let (V::Is(x), V::Is(y)) = (x, y) else {
                        // A value that is not there: no comparison holds, and
                        // arithmetic gives none.
                        return Ok(if op.level() == 6 { V::Is(0) } else { V::None });
                    };
                    match op {
                        Op::Eq => V::Is((x == y) as i64),
                        Op::Ne => V::Is((x != y) as i64),
                        Op::Lt => V::Is((x < y) as i64),
                        Op::Le => V::Is((x <= y) as i64),
                        Op::Gt => V::Is((x > y) as i64),
                        Op::Ge => V::Is((x >= y) as i64),
                        Op::Add => V::Is(x.saturating_add(y)),
                        Op::Sub => V::Is(x.saturating_sub(y)),
                        _ => V::Is(x.saturating_mul(y)),
                    }
                }
            },
            Expr::App(f, args) => match f.as_str() {
                "abs" => match self.val(&args[0])? {
                    V::Is(v) => V::Is(v.abs()),
                    V::None => V::None,
                },
                "min" | "max" => match (self.val(&args[0])?, self.val(&args[1])?) {
                    (V::Is(a), V::Is(b)) => V::Is(if f == "min" { a.min(b) } else { a.max(b) }),
                    _ => V::None,
                },
                "succ" | "pred" => {
                    let t = self.ty_of(&args[0]);
                    match self.val(&args[0])? {
                        V::Is(v) => {
                            let n = if f == "succ" { v + 1 } else { v - 1 };
                            // Past either end there is no next element.
                            let d = t.map(|t| domain(self.sig, self.model, &t)).unwrap_or_default();
                            if d.contains(&n) {
                                V::Is(n)
                            } else {
                                V::None
                            }
                        }
                        V::None => V::None,
                    }
                }
                "defined" => V::Is((self.val(&args[0])? != V::None) as i64),
                name => {
                    let mut vals = Vec::with_capacity(args.len());
                    for a in args {
                        match self.val(a)? {
                            V::Is(v) => vals.push(v),
                            V::None => return Ok(V::None),
                        }
                    }
                    self.table(name, &vals)?
                }
            },
            Expr::Quant(q, binders, body) => {
                // Every assignment to the binders, counted where the body holds.
                let doms: Vec<Vec<i64>> = binders.iter().map(|b| domain(self.sig, self.model, &ty_named(b.sort.as_deref().unwrap_or("int")))).collect();
                let need = match q {
                    Quant::All => None,
                    Quant::Some => Some(1),
                    Quant::AtLeast(n) => Some(*n as usize),
                    Quant::Exactly(n) | Quant::AtMost(n) => Some(*n as usize + 1),
                };
                let mut count = 0usize;
                let mut idx = vec![0usize; binders.len()];
                let mut all = true;
                if doms.iter().all(|d| !d.is_empty()) {
                    loop {
                        let n = self.vars.len();
                        self.vars.extend(binders.iter().zip(&idx).zip(&doms).map(|((b, i), d)| (b.name.clone(), d[*i], b.sort.clone().unwrap_or_else(|| "int".into()))));
                        let ok = self.truth(body);
                        self.vars.truncate(n);
                        if ok? {
                            count += 1;
                        } else {
                            all = false;
                            if *q == Quant::All {
                                break;
                            }
                        }
                        if need.is_some_and(|k| count >= k) {
                            break;
                        }
                        // The next assignment, the last binder fastest; done when
                        // every binder has come round.
                        let mut k = binders.len();
                        let done = loop {
                            if k == 0 {
                                break true;
                            }
                            k -= 1;
                            idx[k] += 1;
                            if idx[k] < doms[k].len() {
                                break false;
                            }
                            idx[k] = 0;
                        };
                        if done {
                            break;
                        }
                    }
                }
                V::Is(match q {
                    Quant::All => all,
                    Quant::Some => count >= 1,
                    Quant::AtLeast(n) => count >= *n as usize,
                    Quant::AtMost(n) => count <= *n as usize,
                    Quant::Exactly(n) => count == *n as usize,
                } as i64)
            }
        })
    }

    /// A term's type, for `succ` and `pred` to know where their sort ends.
    fn ty_of(&self, e: &Expr) -> Option<Ty> {
        match e {
            Expr::Name(n) => {
                if let Some((_, _, s)) = self.vars.iter().rev().find(|(x, _, _)| x == n) {
                    return Some(ty_named(s));
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
            Expr::End { sort, .. } => Some(ty_named(sort)),
            _ => None,
        }
    }
}
