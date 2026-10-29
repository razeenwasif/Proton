//! Logic for Fun's language, read into a [`Theory`] and written from one.
//!
//! Read as the site's syntax page gives it: a `Sorts:`, a `Vocabulary:` and a
//! `Constraints:` section — the three boxes of its solver, which is how a
//! `.l4f` file keeps them — sorts by `enum:` or `cardinality`, symbols by
//! `function`, `predicate` and `name`, constraints in first-order form or
//! clause form, each ending in a full stop. Nothing here comes from FINDER;
//! where the site leaves a formula's grouping to its scope settings, this
//! reads it the usual way (`NOT`, then `AND`, `OR`, `IMP`, `IFF`, ties to the
//! left), and the writer brackets every connective so nothing rests on them.
//!
//! What Logic4Fun says that Proton does not — print settings, priorities,
//! `hidden` — is left out and named in the notes.

use std::collections::BTreeMap;

use crate::ast::*;
use crate::lex::{lex, Dialect, Tok, Token};
use crate::parse::{show, Diag};
use crate::types::{Checked, Sig, Ty};

/// What reading or writing left out, each said once.
pub type Notes = Vec<String>;

fn diag(t: &Token, message: impl Into<String>) -> Diag {
    Diag { line: t.line, col: t.col, message: message.into() }
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    None,
    Sorts,
    Vocabulary,
    Constraints,
}

/// A `.l4f` file, or the three boxes' text under their headings.
pub fn read(text: &str) -> Result<(Theory, Notes), Diag> {
    let toks = lex(text, Dialect::L4f).map_err(|e| Diag { line: e.line, col: e.col, message: e.message })?;
    // Newlines mean nothing here; comments are kept as lines of their own.
    let toks: Vec<Token> = toks.into_iter().filter(|t| t.tok != Tok::Newline).collect();
    let mut r = Reader { toks, i: 0, theory: Theory::default(), notes: Notes::new(), sizes: BTreeMap::new(), aliases: BTreeMap::new(), declared: Vec::new(), binary: BTreeMap::new() };
    r.read()?;
    Ok((r.theory, r.notes))
}

struct Reader {
    toks: Vec<Token>,
    i: usize,
    theory: Theory,
    notes: Notes,
    /// Each sort's size, for `s.size` in another's.
    sizes: BTreeMap<String, u32>,
    aliases: BTreeMap<String, String>,
    /// Constants, elements and propositions: names that are not variables.
    declared: Vec<String>,
    /// Two-argument symbols, which may be written between their arguments:
    /// whether each gives true or false.
    binary: BTreeMap<String, bool>,
}

const SPECS: [&str; 15] = ["all_different", "injective", "surjective", "total", "partial", "commutative", "priority", "no_priority", "scope", "hidden", "cut", "no_cut", "print", "bijective", "symmetric"];

impl Reader {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.tok)
    }

    fn here(&self) -> Token {
        self.toks.get(self.i).or(self.toks.last()).cloned().unwrap_or(Token { tok: Tok::Newline, line: 1, col: 1 })
    }

    fn eat(&mut self, s: &'static str) -> bool {
        if self.peek() == Some(&Tok::Sym(s)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn word(&self) -> Option<String> {
        match self.peek() {
            Some(Tok::Ident(w)) => Some(w.clone()),
            _ => None,
        }
    }

    fn expect(&mut self, s: &'static str) -> Result<(), Diag> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(diag(&self.here(), format!("“{s}” was expected, not “{}”", self.peek().map(show).unwrap_or_default())))
        }
    }

    fn name(&mut self, what: &str) -> Result<String, Diag> {
        match self.word() {
            Some(w) => {
                self.i += 1;
                Ok(w)
            }
            None => Err(diag(&self.here(), format!("{what} was expected"))),
        }
    }

    fn note(&mut self, n: String) {
        if !self.notes.contains(&n) {
            self.notes.push(n);
        }
    }

    fn push(&mut self, item: Item, at: usize) {
        self.theory.lines.push(Line { item: Some(item), comment: None, at });
    }

    fn read(&mut self) -> Result<(), Diag> {
        let mut section = Section::None;
        while self.i < self.toks.len() {
            let t = self.here();
            if let Tok::Comment(c) = &t.tok {
                self.theory.lines.push(Line { item: None, comment: Some(c.clone()), at: t.line });
                self.i += 1;
                continue;
            }
            // A heading: `Sorts:`, `Vocabulary:`, `Constraints:`.
            if let (Some(w), Some(Tok::Sym(":"))) = (self.word(), self.toks.get(self.i + 1).map(|t| &t.tok)) {
                let next = match w.to_lowercase().as_str() {
                    "sorts" => Some(Section::Sorts),
                    "vocabulary" => Some(Section::Vocabulary),
                    "constraints" => Some(Section::Constraints),
                    _ => None,
                };
                if let Some(next) = next {
                    section = next;
                    self.i += 2;
                    continue;
                }
            }
            match section {
                Section::None => return Err(diag(&t, "a Logic4Fun file starts with `Sorts:`, then `Vocabulary:` and `Constraints:`")),
                Section::Sorts => self.sort()?,
                Section::Vocabulary => self.vocabulary()?,
                Section::Constraints => self.constraint()?,
            }
        }
        Ok(())
    }

    /// `name enum: a, b, c.`, `name cardinality = 8.`, `name { … }`,
    /// `name alias other.`
    fn sort(&mut self) -> Result<(), Diag> {
        let at = self.here();
        let name = self.name("a sort's name")?;
        let mut kind: Option<SortKind> = None;
        let (mut lo, mut hi): (Option<u32>, Option<u32>) = (None, None);
        let braced = self.eat("{");
        loop {
            match self.word().as_deref() {
                Some("enum") => {
                    self.i += 1;
                    self.eat(":");
                    let mut names = vec![self.name("an element")?];
                    while self.eat(",") {
                        names.push(self.name("an element")?);
                    }
                    self.declared.extend(names.iter().cloned());
                    self.sizes.insert(name.clone(), names.len() as u32);
                    // Logic4Fun's enumerations are ordered as listed.
                    kind = Some(SortKind::Ordered(names));
                }
                Some("cardinality" | "size") => {
                    self.i += 1;
                    let rel = match self.peek() {
                        Some(Tok::Sym(s)) => *s,
                        _ => return Err(diag(&self.here(), "a size relation was expected: = < > <= >= !=")),
                    };
                    self.i += 1;
                    let n = self.size()?;
                    match rel {
                        "=" => {
                            lo = Some(n);
                            hi = Some(n);
                        }
                        "<" => hi = Some(n.saturating_sub(1)),
                        "<=" => hi = Some(n),
                        ">" => lo = Some(n + 1),
                        ">=" => lo = Some(n),
                        _ => return Err(diag(&at, format!("a size {rel} {n} is not read: say it with = < > <= >="))),
                    }
                }
                Some("alias") => {
                    self.i += 1;
                    let other = self.name("a sort")?;
                    self.aliases.insert(other, name.clone());
                }
                _ => break,
            }
            if !braced {
                break;
            }
        }
        if braced {
            self.expect("}")?;
        }
        self.eat(".");
        if kind.is_none() {
            let (a, b) = (lo.unwrap_or(1), hi.unwrap_or(30));
            if hi.is_none() {
                self.note(format!("{name}: no largest size is said, so 30 is taken"));
            }
            if b < a || b == 0 {
                return Err(diag(&at, format!("{name} can have no size")));
            }
            self.sizes.insert(name.clone(), b);
            kind = Some(if a == b { SortKind::Range(0, a as i64 - 1) } else { SortKind::Sizes(a, b) });
        }
        let kind = kind.unwrap();
        // `natnum`, `nat` and `int` set the numbers' bounds.
        let name = if matches!(name.as_str(), "natnum" | "nat" | "int") { "int".to_string() } else { name };
        if name == "int" && !matches!(kind, SortKind::Range(..)) {
            return Err(diag(&at, "the numbers' sort takes one cardinality"));
        }
        self.push(Item::Sort(SortDecl { name, kind }), at.line);
        Ok(())
    }

    /// A size: a number, `s.size`, or sums and differences of them.
    fn size(&mut self) -> Result<u32, Diag> {
        let mut total: i64 = 0;
        let mut sign = 1;
        loop {
            let v = match self.peek().cloned() {
                Some(Tok::Num(n)) => {
                    self.i += 1;
                    n
                }
                Some(Tok::Ident(s)) => {
                    self.i += 1;
                    self.expect(".")?;
                    if self.word().as_deref() != Some("size") {
                        return Err(diag(&self.here(), format!("`{s}.size` was expected")));
                    }
                    self.i += 1;
                    let s = self.aliases.get(&s).cloned().unwrap_or(s);
                    *self.sizes.get(&s).ok_or_else(|| diag(&self.here(), format!("{s} is not a sort declared above")))? as i64
                }
                _ => return Err(diag(&self.here(), "a size was expected")),
            };
            total += sign * v;
            // `.` ends the statement unless `size` follows, which was taken above.
            if self.eat("+") {
                sign = 1;
            } else if self.eat("-") {
                sign = -1;
            } else {
                break;
            }
        }
        if !(0..=1_000_000).contains(&total) {
            return Err(diag(&self.here(), "a size is from 0 to a million"));
        }
        Ok(total as u32)
    }

    fn sort_name(&mut self) -> Result<String, Diag> {
        let s = self.name("a sort")?;
        Ok(match s.as_str() {
            "natnum" | "nat" => "int".into(),
            _ => self.aliases.get(&s).cloned().unwrap_or(s),
        })
    }

    /// `function …`, `predicate …`, `name …`: one, or several in braces.
    fn vocabulary(&mut self) -> Result<(), Diag> {
        let t = self.here();
        let kind = self.name("`function`, `predicate` or `name`")?;
        if !matches!(kind.as_str(), "function" | "predicate" | "name") {
            return Err(diag(&t, format!("the vocabulary declares with `function`, `predicate` or `name`, not “{kind}”")));
        }
        if self.eat("{") {
            while !self.eat("}") {
                if self.i >= self.toks.len() {
                    return Err(diag(&t, "a `}` is missing"));
                }
                if let Some(Tok::Comment(c)) = self.peek().cloned() {
                    let line = self.here().line;
                    self.theory.lines.push(Line { item: None, comment: Some(c), at: line });
                    self.i += 1;
                    continue;
                }
                self.declaration(&kind)?;
            }
        } else {
            self.declaration(&kind)?;
        }
        self.eat(".");
        Ok(())
    }

    fn declaration(&mut self, kind: &str) -> Result<(), Diag> {
        let at = self.here();
        let name = self.name("a name")?;
        let mut args = Vec::new();
        if kind != "name" && self.eat("(") && !self.eat(")") {
            args.push(self.sort_name()?);
            while self.eat(",") {
                args.push(self.sort_name()?);
            }
            self.expect(")")?;
        }
        let result = if kind == "predicate" {
            "bool".to_string()
        } else {
            self.expect(":")?;
            self.sort_name()?
        };
        let mut props = Props::default();
        // Kinds: one in braces, several, or one alone.
        let braced = self.eat("{");
        while let Some(w) = self.word().filter(|w| SPECS.contains(&w.as_str())) {
            self.i += 1;
            match w.as_str() {
                "all_different" | "injective" => props.one_to_one = true,
                "surjective" => props.onto = true,
                "bijective" => {
                    props.one_to_one = true;
                    props.onto = true;
                }
                "partial" => props.partial = true,
                "total" => {}
                "commutative" | "symmetric" => props.symmetric = true,
                "scope" => {
                    if let Some(Tok::Num(_)) = self.peek() {
                        self.i += 1;
                    }
                    self.note(format!("{name}: `scope` is a FINDER setting, left out"));
                }
                "print" => {
                    self.eat(":");
                    if matches!(self.word().as_deref(), Some("none" | "brief" | "full")) {
                        self.i += 1;
                    }
                    self.note(format!("{name}: `print` is a FINDER setting, left out"));
                }
                other => self.note(format!("{name}: `{other}` is a FINDER setting, left out")),
            }
            if !braced {
                break;
            }
        }
        if braced {
            self.expect("}")?;
        }
        if args.len() == 2 {
            self.binary.insert(name.clone(), result == "bool");
        }
        if args.is_empty() {
            self.declared.push(name.clone());
        }
        self.push(Item::Symbol(SymbolDecl { names: vec![name], args, result, props }), at.line);
        Ok(())
    }

    /// `F.`, or clause form: `A, B -> C ; D.` Free variables are taken as
    /// meant for every value.
    fn constraint(&mut self) -> Result<(), Diag> {
        let at = self.here();
        let mut first = vec![self.expr(0, 0)?];
        while self.eat(",") {
            first.push(self.expr(0, 0)?);
        }
        let e = if self.eat("->") {
            let mut then = vec![self.expr(0, 0)?];
            while self.eat(";") {
                then.push(self.expr(0, 0)?);
            }
            let ante = first.into_iter().reduce(|a, b| Expr::bin(Op::And, a, b)).unwrap();
            let cons = then.into_iter().reduce(|a, b| Expr::bin(Op::Or, a, b)).unwrap();
            Expr::bin(Op::Imp, ante, cons)
        } else {
            if first.len() > 1 {
                return Err(diag(&at, "premises separated by commas need `->` and a conclusion"));
            }
            let mut e = first.pop().unwrap();
            while self.eat(";") {
                e = Expr::bin(Op::Or, e, self.expr(0, 0)?);
            }
            e
        };
        if !self.eat(".") {
            return Err(diag(&self.here(), format!("a constraint ends with a full stop, not “{}”", self.peek().map(show).unwrap_or_else(|| "the end".into()))));
        }
        let e = iffs(e, &self.binary);
        let free: Vec<String> = e.free().into_iter().filter(|n| !self.declared.contains(n)).collect();
        let e = if free.is_empty() { e } else { Expr::Quant(Quant::All, free.into_iter().map(|name| Binder { name, sort: None }).collect(), Box::new(e)) };
        self.push(Item::Constraint(e), at.line);
        Ok(())
    }

    /// The operator next, written between its arguments.
    fn infix(&self, depth: usize) -> Option<(Op, Option<String>)> {
        let op = match self.peek()? {
            Tok::Sym("=") => Op::Eq,
            Tok::Sym("!=") => Op::Ne,
            Tok::Sym("<") => Op::Lt,
            Tok::Sym("<=") => Op::Le,
            Tok::Sym(">") => Op::Gt,
            Tok::Sym(">=") => Op::Ge,
            Tok::Sym("+") => Op::Add,
            Tok::Sym("-") => Op::Sub,
            Tok::Sym("*") => Op::Mul,
            Tok::Sym("and") => Op::And,
            Tok::Sym("or") => Op::Or,
            Tok::Sym("xor") => Op::Xor,
            Tok::Sym("<->") => Op::Iff,
            // Outside brackets `->` is the clause's arrow.
            Tok::Sym("->") if depth > 0 => Op::Imp,
            Tok::Ident(w) => match w.as_str() {
                "AND" => Op::And,
                "OR" => Op::Or,
                "XOR" => Op::Xor,
                "IMP" => Op::Imp,
                "IFF" => Op::Iff,
                "DIF" => return Some((Op::Sub, Some("DIF".into()))),
                other => {
                    return self.binary.get(other).map(|pred| (if *pred { Op::Eq } else { Op::Add }, Some(other.to_string())));
                }
            },
            _ => return None,
        };
        Some((op, None))
    }

    fn expr(&mut self, min: u8, depth: usize) -> Result<Expr, Diag> {
        let mut left = self.prefix(depth)?;
        while let Some((op, named)) = self.infix(depth) {
            let level = op.level();
            if level < min {
                break;
            }
            self.i += 1;
            // Ties go to the left, `IMP` included.
            let right = self.expr(level + 1, depth)?;
            left = match named.as_deref() {
                Some("DIF") => Expr::App("abs".into(), vec![Expr::bin(Op::Sub, left, right)]),
                Some(f) => Expr::App(f.to_string(), vec![left, right]),
                None => Expr::bin(op, left, right),
            };
        }
        Ok(left)
    }

    fn args(&mut self, depth: usize) -> Result<Vec<Expr>, Diag> {
        self.expect("(")?;
        let mut args = vec![self.expr(0, depth + 1)?];
        while self.eat(",") {
            args.push(self.expr(0, depth + 1)?);
        }
        self.expect(")")?;
        Ok(args)
    }

    fn prefix(&mut self, depth: usize) -> Result<Expr, Diag> {
        let t = self.here();
        let Some(tok) = self.peek().cloned() else { return Err(diag(&t, "the constraint ends too soon")) };
        self.i += 1;
        let two = |args: Vec<Expr>, op: Op| -> Result<Expr, Diag> {
            let [a, b]: [Expr; 2] = args.try_into().map_err(|_| diag(&t, format!("{} takes two arguments", op.text())))?;
            Ok(Expr::bin(op, a, b))
        };
        match tok {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Sym("(") => {
                let e = self.expr(0, depth + 1)?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Sym("not") => Ok(Expr::negation(self.expr(5, depth)?)),
            Tok::Sym("true") => Ok(Expr::Bool(true)),
            Tok::Sym("false") => Ok(Expr::Bool(false)),
            Tok::Sym("all") => self.quantifier(Quant::All, depth),
            Tok::Sym("some") => self.quantifier(Quant::Some, depth),
            Tok::Ident(w) => {
                let (base, sub) = match w.split_once('_') {
                    Some((b, s)) if matches!(b, "MIN" | "MAX" | "PRED" | "SUCC") => (b.to_string(), Some(s.to_string())),
                    _ => (w.clone(), None),
                };
                let called = self.peek() == Some(&Tok::Sym("("));
                match base.as_str() {
                    "TRUE" => Ok(Expr::Bool(true)),
                    "FALSE" => Ok(Expr::Bool(false)),
                    "ALL" => self.quantifier(Quant::All, depth),
                    "SOME" => self.quantifier(Quant::Some, depth),
                    "NOT" => Ok(Expr::negation(if called { let mut a = self.args(depth)?; if a.len() != 1 { return Err(diag(&t, "NOT takes one argument")); } a.pop().unwrap() } else { self.expr(5, depth)? })),
                    "AND" | "OR" | "XOR" | "IMP" | "IFF" if called => {
                        let op = match base.as_str() {
                            "AND" => Op::And,
                            "OR" => Op::Or,
                            "XOR" => Op::Xor,
                            "IMP" => Op::Imp,
                            _ => Op::Iff,
                        };
                        let args = self.args(depth)?;
                        two(args, op)
                    }
                    "EST" => Ok(Expr::App("defined".into(), self.args(depth)?)),
                    "DIF" => {
                        let args = self.args(depth)?;
                        Ok(Expr::App("abs".into(), vec![two(args, Op::Sub)?]))
                    }
                    "SUCC" | "PRED" => Ok(Expr::App(base.to_lowercase(), self.args(depth)?)),
                    "MIN" | "MAX" => match sub {
                        Some(sort) => Ok(Expr::End { last: base == "MAX", sort: self.aliases.get(&sort).cloned().unwrap_or(sort) }),
                        None => Err(diag(&t, format!("say which sort's {base}: {base}_sort"))),
                    },
                    "LEN" => Err(diag(&t, "LEN is not read yet")),
                    _ if called => Ok(Expr::App(w, self.args(depth)?)),
                    _ => Ok(Expr::Name(w)),
                }
            }
            other => Err(diag(&t, format!("a formula cannot start with “{}”", show(&other)))),
        }
    }

    /// `ALL x F`, `ALL x ALL y F`: the body runs as far as it can.
    fn quantifier(&mut self, q: Quant, depth: usize) -> Result<Expr, Diag> {
        let name = self.name("a variable")?;
        let body = self.expr(0, depth)?;
        Ok(Expr::Quant(q, vec![Binder { name, sort: None }], Box::new(body)))
    }
}

/// Whether an expression is a formula, as written: true or false.
fn formula(e: &Expr, binary: &BTreeMap<String, bool>) -> bool {
    match e {
        Expr::Bool(_) | Expr::Not(_) | Expr::Quant(..) => true,
        Expr::Bin(op, ..) => op.level() <= 6,
        Expr::App(f, _) => f == "defined" || binary.get(f) == Some(&true),
        _ => false,
    }
}

/// `(A) = (B)` between formulas is Proton's `A <-> B`.
fn iffs(e: Expr, binary: &BTreeMap<String, bool>) -> Expr {
    match e {
        Expr::Bin(op, a, b) => {
            let (a, b) = (iffs(*a, binary), iffs(*b, binary));
            let op = match op {
                Op::Eq if formula(&a, binary) && formula(&b, binary) => Op::Iff,
                Op::Ne if formula(&a, binary) && formula(&b, binary) => Op::Xor,
                op => op,
            };
            Expr::bin(op, a, b)
        }
        Expr::Not(a) => Expr::negation(iffs(*a, binary)),
        Expr::Neg(a) => Expr::Neg(Box::new(iffs(*a, binary))),
        Expr::App(f, args) => Expr::App(f, args.into_iter().map(|a| iffs(a, binary)).collect()),
        Expr::Quant(q, b, body) => Expr::Quant(q, b, Box::new(iffs(*body, binary))),
        e => e,
    }
}

/// A checked theory in Logic4Fun's language: the three boxes under their
/// headings. Questions are Proton's alone and left out; a range not
/// starting at 0 is written from 0, its numbers moved down with it, where
/// that keeps the meaning.
pub fn write(c: &Checked) -> Result<(String, Notes), String> {
    let mut w = Writer { sig: &c.sig, notes: Notes::new(), vars: Vec::new() };
    let (mut sorts, mut vocab, mut cons) = (String::new(), String::new(), String::new());
    for p in &c.sig.propositions {
        vocab.push_str(&format!("name {p} : bool\n"));
    }
    for line in &c.theory.lines {
        let comment = line.comment.as_ref().map(|c| format!("% {c}\n")).unwrap_or_default();
        match &line.item {
            Some(Item::Sort(s)) => {
                sorts.push_str(&comment);
                let text = match &s.kind {
                    SortKind::Named(n) | SortKind::Ordered(n) => format!("{} enum: {}.", s.name, n.join(", ")),
                    SortKind::Range(a, b) => {
                        if s.name == "int" && *a != 0 {
                            return Err("Logic4Fun's numbers start at 0: write int = 0..n".into());
                        }
                        format!("{} cardinality = {}.", if s.name == "int" { "natnum" } else { &s.name }, b - a + 1)
                    }
                    SortKind::Size(n) => format!("{} cardinality = {n}.", s.name),
                    SortKind::Sizes(a, b) => format!("{} {{cardinality >= {a} cardinality <= {b}}}", s.name),
                };
                sorts.push_str(&text);
                sorts.push('\n');
            }
            Some(Item::Symbol(s)) => {
                vocab.push_str(&comment);
                for name in &s.names {
                    let mut specs = Vec::new();
                    if s.props.one_to_one {
                        specs.push("all_different");
                    }
                    if s.props.onto {
                        specs.push("surjective");
                    }
                    if s.props.partial {
                        specs.push("partial");
                    }
                    if s.props.symmetric {
                        specs.push("commutative");
                    }
                    let specs = if specs.is_empty() { String::new() } else { format!(" {{{}}}", specs.join(" ")) };
                    let args = if s.args.is_empty() { String::new() } else { format!("({})", s.args.iter().map(|a| sort_word(a)).collect::<Vec<_>>().join(", ")) };
                    let text = if s.args.is_empty() {
                        format!("name {name} : {}{specs}", sort_word(&s.result))
                    } else if s.result == "bool" {
                        format!("predicate {name}{args}{specs}")
                    } else {
                        format!("function {name}{args} : {}{specs}", sort_word(&s.result))
                    };
                    vocab.push_str(&text);
                    vocab.push('\n');
                }
            }
            Some(Item::Constraint(e)) => {
                cons.push_str(&comment);
                let text = w.formula(e).map_err(|m| format!("line {}: {m}", line.at))?;
                cons.push_str(&text);
                cons.push_str(".\n");
            }
            Some(Item::Question(_)) => w.note("questions are Proton's: left out".into()),
            None => {
                if let Some(c) = &line.comment {
                    cons.push_str(&format!("% {c}\n"));
                }
            }
        }
    }
    Ok((format!("Sorts:\n{sorts}\nVocabulary:\n{vocab}\nConstraints:\n{cons}"), w.notes))
}

fn sort_word(s: &str) -> &str {
    if s == "int" {
        "natnum"
    } else {
        s
    }
}

struct Writer<'a> {
    sig: &'a Sig,
    notes: Notes,
    /// Variables in scope and their sorts.
    vars: Vec<(String, String)>,
}

impl Writer<'_> {
    fn note(&mut self, n: String) {
        if !self.notes.contains(&n) {
            self.notes.push(n);
        }
    }

    /// Where a sort's values start, when written from 0.
    fn offset(&self, t: &Option<Ty>) -> i64 {
        match t {
            Some(Ty::Sort(s)) => match self.sig.sorts.get(s) {
                Some(SortKind::Range(a, _)) => *a,
                _ => 0,
            },
            _ => 0,
        }
    }

    /// The type of a term, as far as writing needs it.
    fn ty(&self, e: &Expr) -> Option<Ty> {
        match e {
            Expr::Num(_) | Expr::Neg(_) => Some(Ty::Int),
            Expr::Name(n) => {
                if let Some((_, s)) = self.vars.iter().rev().find(|(v, _)| v == n) {
                    return Some(match s.as_str() {
                        "int" => Ty::Int,
                        "bool" => Ty::Bool,
                        s => Ty::Sort(s.into()),
                    });
                }
                if let Some((s, _)) = self.sig.elements.get(n) {
                    return Some(Ty::Sort(s.clone()));
                }
                self.sig.symbols.get(n).map(|s| s.result.clone()).or(Some(Ty::Bool))
            }
            Expr::App(f, args) => match f.as_str() {
                "abs" => Some(Ty::Int),
                "min" | "max" | "succ" | "pred" => args.first().and_then(|a| self.ty(a)),
                "defined" => Some(Ty::Bool),
                f => self.sig.symbols.get(f).map(|s| s.result.clone()),
            },
            Expr::End { sort, .. } => Some(Ty::Sort(sort.clone())),
            Expr::Bin(op, ..) if op.level() >= 7 => Some(Ty::Int),
            _ => Some(Ty::Bool),
        }
    }

    /// A formula, every connective bracketed.
    fn formula(&mut self, e: &Expr) -> Result<String, String> {
        Ok(match e {
            Expr::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            Expr::Not(a) => format!("NOT({})", self.formula(a)?),
            Expr::Bin(op, a, b) if op.is_logical() => {
                let word = match op {
                    Op::And => "AND",
                    Op::Or => "OR",
                    Op::Xor => "XOR",
                    Op::Imp => "IMP",
                    _ => "IFF",
                };
                format!("({}) {word} ({})", self.formula(a)?, self.formula(b)?)
            }
            Expr::Bin(op, a, b) if op.level() == 6 => {
                let (ta, tb) = (self.ty(a), self.ty(b));
                // A number against a sort written from 0 is moved with it.
                let side = |w: &mut Self, x: &Expr, other: &Option<Ty>| -> Result<String, String> {
                    match (x, w.offset(other)) {
                        (Expr::Num(n), k) if k != 0 => Ok((n - k).to_string()),
                        _ => w.term(x),
                    }
                };
                let (oa, ob) = (self.offset(&ta), self.offset(&tb));
                if oa != ob && !matches!(a.as_ref(), Expr::Num(_)) && !matches!(b.as_ref(), Expr::Num(_)) {
                    return Err("a sort not starting at 0 is compared with numbers: write it from 0 for Logic4Fun".into());
                }
                format!("{} {} {}", side(self, a, &tb)?, op.text(), side(self, b, &ta)?)
            }
            Expr::Quant(q, binders, body) => self.quantifier(*q, binders, body)?,
            other => self.term(other)?,
        })
    }

    fn quantifier(&mut self, q: Quant, binders: &[Binder], body: &Expr) -> Result<String, String> {
        let n = self.vars.len();
        self.vars.extend(binders.iter().map(|b| (b.name.clone(), b.sort.clone().unwrap_or_default())));
        let out = match q {
            Quant::All | Quant::Some => {
                let word = if q == Quant::All { "ALL" } else { "SOME" };
                let head: String = binders.iter().map(|b| format!("{word} {} ", b.name)).collect();
                format!("{head}({})", self.formula(body)?)
            }
            counted => {
                self.vars.truncate(n);
                let b = &binders[0];
                let e = counting(counted, b, body)?;
                return self.formula(&e);
            }
        };
        self.vars.truncate(n);
        Ok(out)
    }

    fn term(&mut self, e: &Expr) -> Result<String, String> {
        Ok(match e {
            Expr::Name(n) => n.clone(),
            Expr::Num(n) => n.to_string(),
            Expr::Bool(_) | Expr::Not(_) | Expr::Quant(..) => format!("({})", self.formula(e)?),
            Expr::End { last, sort } => format!("{}_{sort}", if *last { "MAX" } else { "MIN" }),
            Expr::Neg(_) => return Err("Logic4Fun's numbers are not negative".into()),
            Expr::App(f, args) => match f.as_str() {
                "abs" => match &args[0] {
                    Expr::Bin(Op::Sub, a, b) => format!("DIF({}, {})", self.term(a)?, self.term(b)?),
                    x => format!("DIF({}, 0)", self.term(x)?),
                },
                "defined" => format!("EST({})", self.term(&args[0])?),
                "succ" | "pred" => format!("{}({})", f.to_uppercase(), self.term(&args[0])?),
                "min" | "max" => return Err(format!("{f}(a, b) has no Logic4Fun form")),
                f => {
                    let sorts = self.sig.symbols.get(f).map(|s| s.args.clone()).unwrap_or_default();
                    let mut out = Vec::new();
                    for (a, s) in args.iter().zip(sorts.iter().map(|s| Some(s.clone())).chain(std::iter::repeat(None))) {
                        out.push(match (a, self.offset(&s)) {
                            (Expr::Num(n), k) if k != 0 => (n - k).to_string(),
                            _ => self.term(a)?,
                        });
                    }
                    format!("{f}({})", out.join(", "))
                }
            },
            Expr::Bin(op, a, b) if op.level() >= 7 => {
                if *op == Op::Mul {
                    return Err("Logic4Fun has no `*`".into());
                }
                let (ta, tb) = (self.ty(a), self.ty(b));
                // A difference of two of one sort is the same from 0; anything
                // else with a moved sort is not.
                let same = *op == Op::Sub && ta == tb;
                if !same && (self.offset(&ta) != 0 || self.offset(&tb) != 0) {
                    return Err("arithmetic on a sort not starting at 0: write it from 0 for Logic4Fun".into());
                }
                format!("({} {} {})", self.term(a)?, op.text(), self.term(b)?)
            }
            Expr::Bin(..) => format!("({})", self.formula(e)?),
        })
    }
}

/// `exactly n x: F`, `at least n x: F`, `at most n x: F` in `ALL` and
/// `SOME`, which is all Logic4Fun counts with.
fn counting(q: Quant, b: &Binder, body: &Expr) -> Result<Expr, String> {
    let at_least = |n: u32| -> Result<Expr, String> {
        if n == 0 {
            return Ok(Expr::Bool(true));
        }
        if n > 4 {
            return Err(format!("counting to {n} is too long written out for Logic4Fun"));
        }
        let names: Vec<String> = (1..=n).map(|k| format!("{}{k}", b.name)).collect();
        let mut parts = Vec::new();
        for (i, x) in names.iter().enumerate() {
            for y in &names[i + 1..] {
                parts.push(Expr::bin(Op::Ne, Expr::name(x), Expr::name(y)));
            }
            parts.push(rename(body, &b.name, x));
        }
        let all = parts.into_iter().reduce(|a, c| Expr::bin(Op::And, a, c)).unwrap();
        Ok(Expr::Quant(Quant::Some, names.into_iter().map(|name| Binder { name, sort: b.sort.clone() }).collect(), Box::new(all)))
    };
    Ok(match q {
        Quant::AtLeast(n) => at_least(n)?,
        Quant::AtMost(n) => Expr::negation(at_least(n + 1)?),
        Quant::Exactly(n) => Expr::bin(Op::And, at_least(n)?, Expr::negation(at_least(n + 1)?)),
        _ => unreachable!(),
    })
}

/// `e` with the free `from` renamed `to`.
pub fn rename(e: &Expr, from: &str, to: &str) -> Expr {
    match e {
        Expr::Name(n) if n == from => Expr::name(to),
        Expr::Name(_) | Expr::Num(_) | Expr::Bool(_) | Expr::End { .. } => e.clone(),
        Expr::App(f, args) => Expr::App(f.clone(), args.iter().map(|a| rename(a, from, to)).collect()),
        Expr::Not(a) => Expr::negation(rename(a, from, to)),
        Expr::Neg(a) => Expr::Neg(Box::new(rename(a, from, to))),
        Expr::Bin(op, a, b) => Expr::bin(*op, rename(a, from, to), rename(b, from, to)),
        Expr::Quant(q, binders, body) => {
            if binders.iter().any(|b| b.name == from) {
                e.clone()
            } else {
                Expr::Quant(*q, binders.clone(), Box::new(rename(body, from, to)))
            }
        }
    }
}
