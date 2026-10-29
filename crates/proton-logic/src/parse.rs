//! Proton's logic language read into a [`Theory`].
//!
//! A statement is a line. It goes on to the next when a bracket is open or
//! the line ends in an operator, a comma or a colon. `name = a | b`,
//! `name = a < b`, `name = 1..8` and `name = size n` declare sorts;
//! `names: sorts -> sort kinds` declare symbols; `?` asks; anything else is
//! a constraint.

use crate::ast::*;
use crate::lex::{lex, Dialect, Tok, Token};

/// Something wrong with the text, and where.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Diag {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

pub fn parse(text: &str) -> Result<Theory, Diag> {
    let toks = lex(text, Dialect::Proton).map_err(|e| Diag { line: e.line, col: e.col, message: e.message })?;
    let mut theory = Theory::default();
    // Names declared so far: a sort's name, or a symbol's.
    let mut declared: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        // One statement's tokens, and the comment ending its last line.
        let start = i;
        let mut depth = 0i32;
        let mut comment = None;
        let mut stmt: Vec<Token> = Vec::new();
        while i < toks.len() {
            let t = &toks[i];
            i += 1;
            match &t.tok {
                Tok::Comment(c) => comment = Some(c.clone()),
                Tok::Newline => {
                    let goes_on = depth > 0 || stmt.last().is_some_and(|l| continues(&l.tok));
                    if !goes_on {
                        break;
                    }
                }
                Tok::Sym("(") => {
                    depth += 1;
                    stmt.push(t.clone());
                }
                Tok::Sym(")") => {
                    depth -= 1;
                    stmt.push(t.clone());
                }
                _ => stmt.push(t.clone()),
            }
        }
        let at = toks[start].line;
        if stmt.is_empty() {
            theory.lines.push(Line { item: None, comment, at });
            continue;
        }
        let item = statement(&stmt, &declared)?;
        match &item {
            Item::Sort(s) => declared.push(s.name.clone()),
            Item::Symbol(s) => declared.extend(s.names.iter().cloned()),
            _ => {}
        }
        theory.lines.push(Line { item: Some(item), comment, at: stmt[0].line });
    }
    // A file ending in a newline reads as one more, empty, line: not kept.
    while theory.lines.last().is_some_and(|l| l.item.is_none() && l.comment.is_none()) {
        theory.lines.pop();
    }
    Ok(theory)
}

/// A token after which the statement goes on to the next line.
fn continues(t: &Tok) -> bool {
    matches!(t, Tok::Sym(s) if !matches!(*s, ")" | "}" | "."))
        || matches!(t, Tok::Ident(w) if matches!(w.as_str(), "and" | "or" | "xor" | "not"))
}

fn diag(t: &Token, message: impl Into<String>) -> Diag {
    Diag { line: t.line, col: t.col, message: message.into() }
}

fn statement(toks: &[Token], declared: &[String]) -> Result<Item, Diag> {
    if toks[0].tok == Tok::Sym("?") {
        return question(&toks[1..], &toks[0]).map(Item::Question);
    }
    if let Some(sort) = sort_decl(toks, declared)? {
        return Ok(Item::Sort(sort));
    }
    if let Some(sym) = symbol_decl(toks)? {
        return Ok(Item::Symbol(sym));
    }
    let mut p = Parser { toks, i: 0 };
    let e = p.expr(0)?;
    p.end()?;
    Ok(Item::Constraint(e))
}

fn ident(t: &Token) -> Option<&str> {
    match &t.tok {
        Tok::Ident(s) => Some(s.as_str()),
        _ => None,
    }
}

const KEYWORDS: [&str; 13] = ["all", "some", "exactly", "at", "not", "and", "or", "xor", "true", "false", "in", "first", "last"];

/// `name = a | b`, `name = a < b`, `name = lo..hi`, `name = size n[..m]`.
fn sort_decl(toks: &[Token], declared: &[String]) -> Result<Option<SortDecl>, Diag> {
    let (Some(name), Some(Tok::Sym("="))) = (ident(&toks[0]), toks.get(1).map(|t| &t.tok)) else { return Ok(None) };
    if declared.iter().any(|d| d == name) || KEYWORDS.contains(&name) || toks.len() < 3 {
        return Ok(None);
    }
    let rest = &toks[2..];
    let num = |k: usize| -> Option<(i64, usize)> {
        match (rest.get(k).map(|t| &t.tok), rest.get(k + 1).map(|t| &t.tok)) {
            (Some(Tok::Num(n)), _) => Some((*n, k + 1)),
            (Some(Tok::Sym("-")), Some(Tok::Num(n))) => Some((-*n, k + 2)),
            _ => None,
        }
    };
    let decl = |kind| Ok(Some(SortDecl { name: name.to_string(), kind }));
    // A range.
    if let Some((lo, k)) = num(0) {
        if rest.get(k).map(|t| &t.tok) == Some(&Tok::Sym("..")) {
            if let Some((hi, k)) = num(k + 1) {
                if k == rest.len() {
                    if hi < lo {
                        return Err(diag(&rest[0], format!("{name} = {lo}..{hi} has no numbers in it")));
                    }
                    return decl(SortKind::Range(lo, hi));
                }
            }
        }
        return Ok(None);
    }
    // A size, or a range of sizes.
    if ident(&rest[0]) == Some("size") {
        let size = |k: usize| match rest.get(k).map(|t| &t.tok) {
            Some(Tok::Num(n)) if *n >= 1 && *n <= 1_000_000 => Ok(*n as u32),
            _ => Err(diag(rest.get(k).unwrap_or(&rest[0]), "a size is a number from 1")),
        };
        let a = size(1)?;
        return match rest.get(2).map(|t| &t.tok) {
            None => decl(SortKind::Size(a)),
            Some(Tok::Sym("..")) if rest.len() == 4 => {
                let b = size(3)?;
                if b < a {
                    return Err(diag(&rest[3], format!("size {a}..{b} has no sizes in it")));
                }
                decl(SortKind::Sizes(a, b))
            }
            _ => Err(diag(&rest[2], "after `size n` comes nothing, or `..m`")),
        };
    }
    // Named elements: `a | b | c`, or ordered, `a < b < c`.
    let sep = match rest.get(1).map(|t| &t.tok) {
        Some(Tok::Sym("|")) => "|",
        Some(Tok::Sym("<")) => "<",
        _ => return Ok(None),
    };
    let mut names = Vec::new();
    for (k, t) in rest.iter().enumerate() {
        if k % 2 == 0 {
            match ident(t) {
                Some(n) if !KEYWORDS.contains(&n) => names.push(n.to_string()),
                _ => return Ok(None),
            }
        } else if t.tok != Tok::Sym(sep) {
            return Ok(None);
        }
    }
    if rest.len() % 2 == 0 {
        return Err(diag(rest.last().unwrap(), format!("{name}'s elements end with “{sep}”")));
    }
    for (k, n) in names.iter().enumerate() {
        if names[..k].contains(n) {
            return Err(diag(&rest[0], format!("{n} is in {name} twice")));
        }
    }
    decl(if sep == "|" { SortKind::Named(names) } else { SortKind::Ordered(names) })
}

/// `p, q: bool`, `c: s`, `f: a, b -> c one-to-one onto`.
fn symbol_decl(toks: &[Token]) -> Result<Option<SymbolDecl>, Diag> {
    let Some(colon) = toks.iter().position(|t| t.tok == Tok::Sym(":")) else { return Ok(None) };
    // The names before the colon: identifiers and commas only.
    let head = &toks[..colon];
    if head.is_empty() || head.len() % 2 == 0 || ident(&head[0]).is_some_and(|w| KEYWORDS.contains(&w)) {
        return Ok(None);
    }
    let mut names = Vec::new();
    for (k, t) in head.iter().enumerate() {
        if k % 2 == 0 {
            match ident(t) {
                Some(n) => names.push(n.to_string()),
                None => return Ok(None),
            }
        } else if t.tok != Tok::Sym(",") {
            return Ok(None);
        }
    }
    let rest = &toks[colon + 1..];
    let arrow = rest.iter().position(|t| t.tok == Tok::Sym("->"));
    let (args, result_at) = match arrow {
        Some(a) => {
            let mut args = Vec::new();
            for (k, t) in rest[..a].iter().enumerate() {
                if k % 2 == 0 {
                    args.push(ident(t).ok_or_else(|| diag(t, "a function's arguments are sorts"))?.to_string());
                } else if t.tok != Tok::Sym(",") {
                    return Err(diag(t, "sorts are separated by commas"));
                }
            }
            if args.is_empty() {
                return Err(diag(&rest[a], "a function takes at least one sort; a constant is `c: sort`"));
            }
            (args, a + 1)
        }
        None => (Vec::new(), 0),
    };
    let result = rest.get(result_at).and_then(ident).ok_or_else(|| diag(rest.get(result_at).unwrap_or(&toks[colon]), "the sort it gives is missing"))?;
    let mut props = Props::default();
    let mut k = result_at + 1;
    while k < rest.len() {
        let word = ident(&rest[k]).ok_or_else(|| diag(&rest[k], "after the sort come its kinds: one-to-one, onto, partial, symmetric"))?;
        // `one-to-one` is three words and two dashes.
        if word == "one" && rest.get(k + 2).and_then(ident) == Some("to") && rest.get(k + 4).and_then(ident) == Some("one") {
            props.one_to_one = true;
            k += 5;
            continue;
        }
        match word {
            "injective" | "all_different" => props.one_to_one = true,
            "onto" | "surjective" => props.onto = true,
            "bijective" => {
                props.one_to_one = true;
                props.onto = true;
            }
            "partial" => props.partial = true,
            "symmetric" | "commutative" => props.symmetric = true,
            other => return Err(diag(&rest[k], format!("“{other}” is not a kind: one-to-one, onto, partial or symmetric"))),
        }
        k += 1;
    }
    Ok(Some(SymbolDecl { names, args, result: result.to_string(), props }))
}

fn question(toks: &[Token], q: &Token) -> Result<Question, Diag> {
    if toks.is_empty() {
        return Err(diag(q, "a question needs a formula after the ?"));
    }
    let mut p = Parser { toks, i: 0 };
    if p.eat("|=") {
        let goal = p.expr(0)?;
        p.end()?;
        return Ok(Question::Entails(Vec::new(), goal));
    }
    let first = p.expr(0)?;
    if p.eat("==") {
        let second = p.expr(0)?;
        p.end()?;
        return Ok(Question::Same(first, second));
    }
    let mut premises = vec![first];
    while p.eat(",") {
        premises.push(p.expr(0)?);
    }
    if p.eat("|=") {
        let goal = p.expr(0)?;
        p.end()?;
        return Ok(Question::Entails(premises, goal));
    }
    p.end()?;
    if premises.len() > 1 {
        return Err(diag(q, "several formulas ask whether they entail one: `? A, B |= C`"));
    }
    Ok(Question::Formula(premises.pop().unwrap()))
}

struct Parser<'a> {
    toks: &'a [Token],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.tok)
    }

    fn here(&self) -> &Token {
        self.toks.get(self.i).or(self.toks.last()).unwrap()
    }

    fn eat(&mut self, sym: &'static str) -> bool {
        if self.peek() == Some(&Tok::Sym(sym)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, w: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Ident(s)) if s == w) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn end(&self) -> Result<(), Diag> {
        match self.toks.get(self.i) {
            None => Ok(()),
            Some(t) => Err(diag(t, format!("“{}” was not expected here", show(&t.tok)))),
        }
    }

    fn expect(&mut self, sym: &'static str) -> Result<(), Diag> {
        if self.eat(sym) {
            Ok(())
        } else {
            Err(diag(self.here(), format!("“{sym}” was expected")))
        }
    }

    /// The operator next, if it binds at least as tightly as `min`.
    fn infix(&self) -> Option<Op> {
        Some(match self.peek()? {
            Tok::Sym("<->") => Op::Iff,
            Tok::Sym("->") => Op::Imp,
            Tok::Sym("or") | Tok::Sym("|") => Op::Or,
            Tok::Sym("and") => Op::And,
            Tok::Sym("=") => Op::Eq,
            Tok::Sym("!=") => Op::Ne,
            Tok::Sym("<") => Op::Lt,
            Tok::Sym("<=") => Op::Le,
            Tok::Sym(">") => Op::Gt,
            Tok::Sym(">=") => Op::Ge,
            Tok::Sym("+") => Op::Add,
            Tok::Sym("-") => Op::Sub,
            Tok::Sym("*") => Op::Mul,
            Tok::Ident(w) => match w.as_str() {
                "and" => Op::And,
                "or" => Op::Or,
                "xor" => Op::Xor,
                _ => return None,
            },
            _ => return None,
        })
    }

    fn expr(&mut self, min: u8) -> Result<Expr, Diag> {
        let mut left = self.prefix()?;
        while let Some(op) = self.infix() {
            let level = op.level();
            if level < min {
                break;
            }
            self.i += 1;
            let right = match op {
                // Groups to the right.
                Op::Imp => self.expr(level)?,
                _ => self.expr(level + 1)?,
            };
            if matches!(op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge) {
                if let Some(next) = self.infix().filter(|o| o.level() == 6) {
                    return Err(diag(self.here(), format!("comparisons do not chain: bracket one, or write `and` before “{}”", next.text())));
                }
            }
            left = Expr::bin(op, left, right);
        }
        Ok(left)
    }

    fn prefix(&mut self) -> Result<Expr, Diag> {
        let Some(tok) = self.peek().cloned() else {
            return Err(diag(self.here(), "the formula ends too soon"));
        };
        let at = self.here().clone();
        self.i += 1;
        match tok {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Sym("not") => Ok(Expr::negation(self.expr(5)?)),
            Tok::Sym("true") => Ok(Expr::Bool(true)),
            Tok::Sym("false") => Ok(Expr::Bool(false)),
            Tok::Sym("-") => Ok(Expr::Neg(Box::new(self.expr(9)?))),
            Tok::Sym("(") => {
                let e = self.expr(0)?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Sym("all") => self.quantifier(Quant::All),
            Tok::Sym("some") => self.quantifier(Quant::Some),
            Tok::Ident(w) if matches!(w.as_str(), "and" | "or" | "xor" | "in") => Err(diag(&at, format!("a formula cannot start with “{w}”"))),
            Tok::Ident(w) => match w.as_str() {
                "not" => Ok(Expr::negation(self.expr(5)?)),
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "all" => self.quantifier(Quant::All),
                "some" => self.quantifier(Quant::Some),
                "exactly" => {
                    let n = self.count()?;
                    self.quantifier(Quant::Exactly(n))
                }
                "at" => {
                    let most = if self.eat_word("least") {
                        false
                    } else if self.eat_word("most") {
                        true
                    } else {
                        return Err(diag(self.here(), "`at least n x:` or `at most n x:`"));
                    };
                    let n = self.count()?;
                    self.quantifier(if most { Quant::AtMost(n) } else { Quant::AtLeast(n) })
                }
                "first" | "last" => {
                    self.expect("(")?;
                    let sort = match self.peek() {
                        Some(Tok::Ident(s)) => s.clone(),
                        _ => return Err(diag(self.here(), format!("`{w}(sort)` names a sort"))),
                    };
                    self.i += 1;
                    self.expect(")")?;
                    Ok(Expr::End { last: w == "last", sort })
                }
                _ => {
                    if self.eat("(") {
                        let mut args = vec![self.expr(0)?];
                        while self.eat(",") {
                            args.push(self.expr(0)?);
                        }
                        self.expect(")")?;
                        Ok(Expr::App(w, args))
                    } else {
                        Ok(Expr::Name(w))
                    }
                }
            },
            other => Err(diag(&at, format!("a formula cannot start with “{}”", show(&other)))),
        }
    }

    fn count(&mut self) -> Result<u32, Diag> {
        match self.peek() {
            Some(Tok::Num(n)) if *n >= 0 && *n <= u32::MAX as i64 => {
                let n = *n as u32;
                self.i += 1;
                Ok(n)
            }
            _ => Err(diag(self.here(), "a count was expected")),
        }
    }

    /// `x, y in s: body`, the body as far to the right as it goes.
    fn quantifier(&mut self, q: Quant) -> Result<Expr, Diag> {
        let mut binders = Vec::new();
        loop {
            let name = match self.peek() {
                Some(Tok::Ident(n)) if !KEYWORDS.contains(&n.as_str()) => n.clone(),
                _ => return Err(diag(self.here(), "a variable was expected after the quantifier")),
            };
            self.i += 1;
            let sort = if self.eat_word("in") {
                match self.peek() {
                    Some(Tok::Ident(s)) => {
                        let s = s.clone();
                        self.i += 1;
                        Some(s)
                    }
                    _ => return Err(diag(self.here(), "a sort was expected after `in`")),
                }
            } else {
                None
            };
            binders.push(Binder { name, sort });
            if !self.eat(",") {
                break;
            }
        }
        if !matches!(q, Quant::All | Quant::Some) && binders.len() > 1 {
            return Err(diag(self.here(), "a counting quantifier counts one variable"));
        }
        self.expect(":")?;
        let body = self.expr(0)?;
        Ok(Expr::Quant(q, binders, Box::new(body)))
    }
}

pub(crate) fn show(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => s.clone(),
        Tok::Num(n) => n.to_string(),
        Tok::Sym(s) => s.to_string(),
        Tok::Newline => "the end of the line".into(),
        Tok::Comment(_) => "a comment".into(),
    }
}
