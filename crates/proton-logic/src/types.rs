//! A theory's names checked and given types: each sort's elements, each
//! symbol's argument and result sorts, each variable's sort, inferred where
//! it is not written. A name nothing declares, where a formula is expected,
//! is a proposition, so `? p -> q` needs no declarations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ast::*;
use crate::parse::Diag;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ty {
    Bool,
    Int,
    Sort(String),
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Bool => write!(f, "bool"),
            Ty::Int => write!(f, "int"),
            Ty::Sort(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub args: Vec<Ty>,
    pub result: Ty,
    pub props: Props,
}

/// The names a theory declares, and the propositions it uses undeclared.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sig {
    pub sorts: BTreeMap<String, SortKind>,
    pub symbols: BTreeMap<String, Symbol>,
    /// Each named element: its sort and its place in it.
    pub elements: BTreeMap<String, (String, usize)>,
    /// Propositions used without being declared, first used first.
    pub propositions: Vec<String>,
    /// Sorts in the order declared.
    pub order: Vec<String>,
    /// In a file that declares nothing — a worksheet's questions — the
    /// functions and predicates used undeclared: each one's arity, and
    /// whether it is a predicate. Their arguments, and their values when
    /// not true or false, are individuals ([`INDIVIDUAL`]).
    #[serde(default)]
    pub loose: BTreeMap<String, (usize, bool)>,
}

/// The sort of the things a worksheet's undeclared functions and
/// predicates are of, and its unsorted variables range over.
pub const INDIVIDUAL: &str = "individual";

/// A theory checked: every binder given its sort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    pub theory: Theory,
    pub sig: Sig,
}

impl Sig {
    /// Whether values of the type can be added and ordered.
    pub fn ordered(&self, t: &Ty) -> bool {
        match t {
            Ty::Int => true,
            Ty::Bool => false,
            Ty::Sort(s) => !matches!(self.sorts.get(s), Some(SortKind::Named(_))),
        }
    }

    /// Whether a sort's values are numbers, so they mix with numbers.
    pub fn numeric(&self, t: &Ty) -> bool {
        match t {
            Ty::Int => true,
            Ty::Sort(s) => matches!(self.sorts.get(s), Some(SortKind::Range(..))),
            Ty::Bool => false,
        }
    }
}

pub fn check(theory: &Theory) -> Result<Checked, Vec<Diag>> {
    // Nothing declared: undeclared functions and predicates are loose.
    let loose = !theory.lines.iter().any(|l| matches!(l.item, Some(Item::Sort(_)) | Some(Item::Symbol(_))));
    let mut c = Checker { sig: Sig::default(), diags: Vec::new(), line: 0, loose, things: Vec::new() };
    c.sig.sorts.insert("bool".into(), SortKind::Ordered(vec!["false".into(), "true".into()]));
    let mut out = theory.clone();
    for line in &mut out.lines {
        c.line = line.at;
        // The names a statement gives as arguments, anywhere in it: in a
        // worksheet, things, not propositions, even with `=` between two.
        c.things.clear();
        if let Some(item) = &line.item {
            argument_names(item, &mut c.things);
        }
        match &mut line.item {
            Some(Item::Sort(s)) => c.sort(s),
            Some(Item::Symbol(s)) => c.symbol(s),
            Some(Item::Constraint(e)) => c.formula(e),
            Some(Item::Question(q)) => match q {
                Question::Formula(e) => c.formula(e),
                Question::Entails(ps, g) => {
                    ps.iter_mut().for_each(|p| c.formula(p));
                    c.formula(g);
                }
                Question::Same(a, b) => {
                    c.formula(a);
                    c.formula(b);
                }
            },
            None => {}
        }
    }
    c.sig.sorts.remove("bool");
    if c.diags.is_empty() {
        Ok(Checked { theory: out, sig: c.sig })
    } else {
        Err(c.diags)
    }
}

fn argument_names(item: &Item, out: &mut Vec<String>) {
    fn args(e: &Expr, inside: bool, out: &mut Vec<String>) {
        match e {
            Expr::Name(n) if inside => out.push(n.clone()),
            Expr::App(_, xs) => xs.iter().for_each(|x| args(x, true, out)),
            Expr::Not(a) | Expr::Neg(a) => args(a, inside, out),
            Expr::Bin(_, a, b) => {
                args(a, inside, out);
                args(b, inside, out);
            }
            Expr::Quant(_, _, body) => args(body, inside, out),
            _ => {}
        }
    }
    match item {
        Item::Constraint(e) | Item::Question(Question::Formula(e)) => args(e, false, out),
        Item::Question(Question::Entails(ps, g)) => ps.iter().chain(std::iter::once(g)).for_each(|e| args(e, false, out)),
        Item::Question(Question::Same(a, b)) => [a, b].into_iter().for_each(|e| args(e, false, out)),
        _ => {}
    }
}

struct Checker {
    sig: Sig,
    diags: Vec<Diag>,
    line: usize,
    loose: bool,
    /// The names the formula being checked gives as arguments.
    things: Vec<String>,
}

/// The variables in scope: a name and its sort, if known yet.
type Scope = Vec<(String, Option<Ty>)>;

impl Checker {
    fn err(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.diags.iter().any(|d| d.line == self.line && d.message == message) {
            self.diags.push(Diag { line: self.line, col: 0, message });
        }
    }

    fn taken(&self, name: &str) -> bool {
        name == "int" || self.sig.sorts.contains_key(name) || self.sig.symbols.contains_key(name) || self.sig.elements.contains_key(name) || BUILT_IN.contains(&name)
    }

    fn sort(&mut self, s: &SortDecl) {
        // `int = lo..hi` sets the numbers' bounds; any other name is new.
        if s.name != "int" && self.taken(&s.name) {
            return self.err(format!("{} is declared already", s.name));
        }
        if s.name == "int" && !matches!(s.kind, SortKind::Range(..)) {
            return self.err("int is a range: `int = 0..99`");
        }
        if let SortKind::Named(names) | SortKind::Ordered(names) = &s.kind {
            for (k, n) in names.iter().enumerate() {
                if self.taken(n) {
                    self.err(format!("{n} is declared already"));
                } else {
                    self.sig.elements.insert(n.clone(), (s.name.clone(), k));
                }
            }
        }
        self.sig.sorts.insert(s.name.clone(), s.kind.clone());
        self.sig.order.push(s.name.clone());
    }

    fn ty_of(&mut self, sort: &str) -> Option<Ty> {
        match sort {
            "bool" => Some(Ty::Bool),
            "int" => Some(Ty::Int),
            s if self.sig.sorts.contains_key(s) => Some(Ty::Sort(s.to_string())),
            INDIVIDUAL if self.loose => Some(Ty::Sort(INDIVIDUAL.into())),
            s => {
                self.err(format!("{s} is not a sort"));
                None
            }
        }
    }

    fn symbol(&mut self, s: &SymbolDecl) {
        let args: Option<Vec<Ty>> = s.args.iter().map(|a| self.ty_of(a)).collect();
        let (Some(args), Some(result)) = (args, self.ty_of(&s.result)) else { return };
        if s.props.symmetric && (args.len() != 2 || args[0] != args[1]) {
            self.err("symmetric is for two arguments of one sort");
        }
        if (s.props.one_to_one || s.props.onto) && (args.is_empty() || result == Ty::Bool) {
            self.err("one-to-one and onto are for functions");
        }
        for n in &s.names {
            if self.taken(n) {
                self.err(format!("{n} is declared already"));
            } else {
                self.sig.symbols.insert(n.clone(), Symbol { args: args.clone(), result: result.clone(), props: s.props });
            }
        }
    }

    /// A constraint or a question's formula: true or false, no free variables.
    fn formula(&mut self, e: &mut Expr) {
        // Twice: what the first pass learns of each variable's sort, the
        // second uses where the variable came before what said it.
        let mut scope = Scope::new();
        self.infer(e, &mut scope, Some(&Ty::Bool), false);
        let n = self.diags.len();
        let t = self.infer(e, &mut scope, Some(&Ty::Bool), true);
        if self.diags.len() == n && t.is_some_and(|t| t != Ty::Bool) {
            self.err("a constraint is true or false, not a value");
        }
    }

    /// The type of `e`, given what is expected of it; binders' sorts filled
    /// in on the last pass, when every problem is also said.
    fn infer(&mut self, e: &mut Expr, scope: &mut Scope, want: Option<&Ty>, last: bool) -> Option<Ty> {
        let say = |c: &mut Checker, m: String| {
            if last {
                c.err(m);
            }
        };
        match e {
            Expr::Num(_) => Some(Ty::Int),
            Expr::Bool(_) => Some(Ty::Bool),
            Expr::End { sort, .. } => {
                let t = self.ty_of(sort)?;
                if !self.sig.ordered(&t) {
                    say(self, format!("{sort} has no first or last: its elements are not ordered"));
                }
                Some(t)
            }
            Expr::Name(n) => {
                if let Some(k) = scope.iter().rposition(|(v, _)| v == n) {
                    if scope[k].1.is_none() {
                        if let Some(w) = want {
                            scope[k].1 = Some(w.clone());
                        }
                    }
                    return scope[k].1.clone();
                }
                if let Some((sort, _)) = self.sig.elements.get(n) {
                    return Some(Ty::Sort(sort.clone()));
                }
                if let Some(s) = self.sig.symbols.get(n).cloned() {
                    if !s.args.is_empty() {
                        say(self, format!("{n} takes {} argument{}", s.args.len(), if s.args.len() == 1 { "" } else { "s" }));
                    }
                    return Some(s.result);
                }
                if self.sig.sorts.contains_key(n) {
                    say(self, format!("{n} is a sort, not a value"));
                    return None;
                }
                if want == Some(&Ty::Bool) {
                    if !self.sig.propositions.contains(n) {
                        self.sig.propositions.push(n.clone());
                    }
                    return Some(Ty::Bool);
                }
                if self.sig.propositions.contains(n) {
                    return Some(Ty::Bool);
                }
                // A loose constant: an individual, where one is wanted.
                if self.loose && (want == Some(&Ty::Sort(INDIVIDUAL.into())) || (want.is_none() && last) || self.sig.loose.contains_key(n)) {
                    return self.loose_symbol(n, 0, false, last);
                }
                if want.is_some() || last {
                    say(self, format!("{n} is not declared"));
                }
                None
            }
            Expr::App(f, args) => self.app(f, args, scope, want, last),
            Expr::Not(a) => {
                self.boolean(a, scope, last, "not");
                Some(Ty::Bool)
            }
            Expr::Neg(a) => {
                let t = self.infer(a, scope, Some(&Ty::Int), last);
                if t.as_ref().is_some_and(|t| !self.sig.ordered(t)) {
                    say(self, "only numbers are negated".into());
                }
                Some(Ty::Int)
            }
            Expr::Bin(op, a, b) if op.is_logical() => {
                let name = op.text();
                self.boolean(a, scope, last, name);
                self.boolean(b, scope, last, name);
                Some(Ty::Bool)
            }
            Expr::Bin(op @ (Op::Add | Op::Sub | Op::Mul), a, b) => {
                let (op, ta, tb) = (*op, self.infer(a, scope, None, last), self.infer(b, scope, None, last));
                let ta = ta.or_else(|| self.infer(a, scope, tb.as_ref(), last));
                let tb = tb.or_else(|| self.infer(b, scope, ta.as_ref(), last));
                for t in [&ta, &tb].into_iter().flatten() {
                    if !self.sig.ordered(t) {
                        say(self, format!("{} is not done to {t}: its values are not numbers or in order", op.text()));
                    }
                }
                Some(Ty::Int)
            }
            Expr::Bin(op, a, b) => {
                // A comparison. Either side may say what the other is.
                let op = *op;
                let ta = self.infer(a, scope, None, last);
                let tb = self.infer(b, scope, ta.as_ref(), last);
                let ta = match ta {
                    Some(t) => Some(t),
                    None => self.infer(a, scope, tb.as_ref(), last),
                };
                // Neither side saying: `p = q` between undeclared names is
                // between propositions.
                let loose = |e: &Expr, c: &Checker| matches!(e, Expr::Name(n) if !scope.iter().any(|(v, _)| v == n) && !c.taken(n) && !c.things.contains(n));
                let (ta, tb) = match (ta, tb) {
                    (None, None) if matches!(op, Op::Eq | Op::Ne) && loose(a, self) && loose(b, self) => {
                        (self.infer(a, scope, Some(&Ty::Bool), last), self.infer(b, scope, Some(&Ty::Bool), last))
                    }
                    other => other,
                };
                match (&ta, &tb) {
                    (Some(x), Some(y)) => {
                        let same = x == y || (self.sig.numeric(x) && self.sig.numeric(y)) || (self.sig.ordered(x) && *y == Ty::Int) || (self.sig.ordered(y) && *x == Ty::Int);
                        if !same {
                            say(self, format!("{} and {} cannot be compared", x, y));
                        } else if !matches!(op, Op::Eq | Op::Ne) && (!self.sig.ordered(x) || !self.sig.ordered(y)) {
                            say(self, format!("{} is not done to {}: its values are not in order", op.text(), if self.sig.ordered(x) { y } else { x }));
                        }
                    }
                    _ => say(self, format!("cannot tell what “{}” compares", op.text())),
                }
                Some(Ty::Bool)
            }
            Expr::Quant(q, binders, body) => {
                let n = scope.len();
                for b in binders.iter() {
                    if self.taken(&b.name) {
                        say(self, format!("{} is declared already, so cannot be a variable", b.name));
                    }
                    let t = b.sort.as_deref().and_then(|s| self.ty_of(s));
                    scope.push((b.name.clone(), t));
                }
                self.boolean(body, scope, last, "a quantifier");
                if let (Quant::Exactly(_) | Quant::AtLeast(_) | Quant::AtMost(_), true) = (q, binders.len() > 1) {
                    say(self, "a counting quantifier counts one variable".into());
                }
                for (k, b) in binders.iter_mut().enumerate() {
                    match &scope[n + k].1 {
                        Some(Ty::Sort(s)) => b.sort = Some(s.clone()),
                        Some(Ty::Int) => b.sort = Some("int".into()),
                        Some(Ty::Bool) => b.sort = Some("bool".into()),
                        None if self.loose => b.sort = Some(INDIVIDUAL.into()),
                        None => say(self, format!("cannot tell what {} ranges over: write `{} in sort`", b.name, b.name)),
                    }
                }
                scope.truncate(n);
                Some(Ty::Bool)
            }
        }
    }

    /// A loose function, predicate or constant, as first used; used again,
    /// it must be used alike.
    fn loose_symbol(&mut self, f: &str, arity: usize, predicate: bool, last: bool) -> Option<Ty> {
        match self.sig.loose.get(f).copied() {
            Some((n, p)) if n != arity || p != predicate => {
                if last {
                    self.err(format!("{f} is used with {n} and with {arity} arguments, or as both a predicate and a function"));
                }
            }
            Some(_) => {}
            None => {
                self.sig.loose.insert(f.to_string(), (arity, predicate));
            }
        }
        Some(if predicate { Ty::Bool } else { Ty::Sort(INDIVIDUAL.into()) })
    }

    fn boolean(&mut self, e: &mut Expr, scope: &mut Scope, last: bool, under: &str) {
        let t = self.infer(e, scope, Some(&Ty::Bool), last);
        if last && t.as_ref().is_some_and(|t| *t != Ty::Bool) {
            let t = t.unwrap();
            self.err(format!("{under} needs true or false, and is given a {t}"));
        }
    }

    fn app(&mut self, f: &str, args: &mut [Expr], scope: &mut Scope, want: Option<&Ty>, last: bool) -> Option<Ty> {
        let say = |c: &mut Checker, m: String| {
            if last {
                c.err(m);
            }
        };
        let arity = |c: &mut Checker, n: usize| {
            if args.len() != n {
                say(c, format!("{f} takes {n} argument{}", if n == 1 { "" } else { "s" }));
                false
            } else {
                true
            }
        };
        match f {
            "abs" => {
                if arity(self, 1) {
                    let t = self.infer(&mut args[0], scope, Some(&Ty::Int), last);
                    if t.as_ref().is_some_and(|t| !self.sig.ordered(t)) {
                        say(self, "abs is of numbers".into());
                    }
                }
                Some(Ty::Int)
            }
            "min" | "max" => {
                if !arity(self, 2) {
                    return None;
                }
                let (a, b) = args.split_at_mut(1);
                let ta = self.infer(&mut a[0], scope, None, last);
                let tb = self.infer(&mut b[0], scope, ta.as_ref(), last);
                let t = ta.or(tb)?;
                if !self.sig.ordered(&t) {
                    say(self, format!("{f} is of values in order"));
                }
                Some(t)
            }
            "succ" | "pred" => {
                if !arity(self, 1) {
                    return None;
                }
                let t = self.infer(&mut args[0], scope, None, last)?;
                if !self.sig.ordered(&t) {
                    say(self, format!("{f} is of values in order"));
                }
                Some(t)
            }
            "defined" => {
                if arity(self, 1) {
                    let partial = matches!(&args[0], Expr::App(g, _) if self.sig.symbols.get(g).is_some_and(|s| s.props.partial));
                    if !partial {
                        say(self, "defined(…) is asked of a partial function's value".into());
                    }
                    self.infer(&mut args[0], scope, None, last);
                }
                Some(Ty::Bool)
            }
            _ => {
                let Some(sym) = self.sig.symbols.get(f).cloned() else {
                    if self.loose {
                        let predicate = want == Some(&Ty::Bool) || self.sig.loose.get(f).is_some_and(|(_, p)| *p);
                        for a in args.iter_mut() {
                            self.infer(a, scope, Some(&Ty::Sort(INDIVIDUAL.into())), last);
                        }
                        return self.loose_symbol(f, args.len(), predicate, last);
                    }
                    say(self, format!("{f} is not declared"));
                    for a in args.iter_mut() {
                        self.infer(a, scope, None, last);
                    }
                    return None;
                };
                if !arity(self, sym.args.len()) {
                    return Some(sym.result);
                }
                for (a, want) in args.iter_mut().zip(&sym.args) {
                    let got = self.infer(a, scope, Some(want), last);
                    if let Some(got) = got {
                        let fits = got == *want || (got == Ty::Int && self.sig.numeric(want));
                        if !fits {
                            say(self, format!("{f} takes a {want}, and is given a {got}"));
                        }
                    }
                }
                Some(sym.result)
            }
        }
    }
}
