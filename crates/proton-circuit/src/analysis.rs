//! Analysis and synthesis: truth tables from circuits and expressions,
//! minimised expressions, and circuits built from them.
//!
//! - A **truth table** is a function of up to 16 one-bit variables, each
//!   output a string of `0`, `1` and `x` (don't care), row `r` being the
//!   variables' bits with the first variable as the most significant.
//! - **Minimising** is Quine–McCluskey: the prime implicants by combining
//!   terms that differ in one bit, then a cover of the ones — exact, by a
//!   bounded search (Petrick's method in effect), up to 12 variables, and
//!   greedy above. A product of sums is the cover of the zeros, inverted.
//! - **Expressions** are read with `'`, `!`, `~` or `¬` for not, `&`, `*`,
//!   `·` or two names side by side for and, `^` or `⊕` for xor, and `|` or
//!   `+` for or, in that order of binding; `Y = …; Z = …` names outputs.
//! - **Synthesis** lays a cover out as a circuit: the inputs down the left,
//!   a rail for each variable and one for its complement, a column of AND
//!   gates and an OR — or NAND and NAND, or from the product of sums NOR
//!   and NOR — for each output, wired so that every junction is a wire's
//!   end, as the reader joins them.

use std::collections::{BTreeSet, HashMap};

use crate::circ::{Drawing, Part, Project, Wire};
use crate::dig::Built;
use crate::edit;

/// No more variables than this.
pub const MOST_VARIABLES: usize = 16;
/// An exact cover is searched for up to this many variables.
pub const EXACT_UP_TO: usize = 12;
/// No more implicants than this are made while combining.
const MOST_IMPLICANTS: usize = 200_000;
/// Nodes of the exact cover search, before it settles for the best found.
const MOST_NODES: usize = 200_000;

/// A function of named one-bit variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    /// Each output: `0`, `1` or `x` a row, `2^inputs` rows.
    pub values: Vec<Vec<u8>>,
}

/// A product term: the variables in `care`, each as its bit in `value`.
/// Bit `i` is variable `n - 1 - i`, so the first variable is the top bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Term {
    pub value: u32,
    pub care: u32,
}

impl Term {
    fn covers(&self, row: u32) -> bool {
        row & self.care == self.value & self.care
    }
    fn literals(&self) -> u32 {
        self.care.count_ones()
    }
}

/// A function minimised: the terms of its sum of products, and the terms
/// whose zeros make its product of sums (each a clause of the complements
/// of its variables' values: a 1 in `value` is the variable inverted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Minimal {
    pub sop: Vec<Term>,
    pub pos: Vec<Term>,
    /// Whether the cover is known to be the least: false when greedy.
    pub exact: bool,
}

/// The prime implicants of the rows that are 1, the don't cares helping.
pub fn primes(n: usize, ones: &[u32], dont: &[u32]) -> Result<Vec<Term>, String> {
    let full = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
    let mut current: BTreeSet<Term> = ones.iter().chain(dont).map(|&m| Term { value: m, care: full }).collect();
    let mut primes = BTreeSet::new();
    while !current.is_empty() {
        // Grouped by what they care about and how many ones: only terms with
        // the same cares and one more one can combine.
        let mut groups: HashMap<(u32, u32), Vec<Term>> = HashMap::new();
        for t in &current {
            groups.entry((t.care, (t.value & t.care).count_ones())).or_default().push(*t);
        }
        let mut next = BTreeSet::new();
        let mut used = BTreeSet::new();
        for (&(care, count), terms) in &groups {
            let Some(above) = groups.get(&(care, count + 1)) else { continue };
            let have: BTreeSet<u32> = above.iter().map(|t| t.value & care).collect();
            for t in terms {
                for bit in 0..n {
                    let b = 1u32 << bit;
                    if care & b == 0 || t.value & b != 0 {
                        continue;
                    }
                    let other = (t.value & care) | b;
                    if have.contains(&other) {
                        next.insert(Term { value: t.value & care & !b, care: care & !b });
                        used.insert(*t);
                        used.insert(Term { value: other, care });
                    }
                }
            }
            if next.len() > MOST_IMPLICANTS {
                return Err(format!("this function has too many implicants to minimise (over {MOST_IMPLICANTS})"));
            }
        }
        for t in &current {
            if !used.contains(t) {
                primes.insert(Term { value: t.value & t.care, care: t.care });
            }
        }
        current = next;
    }
    Ok(primes.into_iter().collect())
}

/// The fewest primes covering every one — fewest terms, then literals —
/// searched for exactly when it can be, else chosen greedily.
fn cover(n: usize, ones: &[u32], primes: &[Term]) -> (Vec<Term>, bool) {
    let mut chosen: Vec<Term> = Vec::new();
    let mut left: BTreeSet<u32> = ones.iter().copied().collect();
    // Essential primes: the only one covering some row.
    loop {
        let mut took = false;
        for &m in left.clone().iter() {
            let by: Vec<&Term> = primes.iter().filter(|p| p.covers(m)).collect();
            if by.len() == 1 && !chosen.contains(by[0]) {
                chosen.push(*by[0]);
                took = true;
            }
        }
        left.retain(|m| !chosen.iter().any(|p| p.covers(*m)));
        if !took {
            break;
        }
    }
    if left.is_empty() {
        return (chosen, true);
    }
    let rest: Vec<Term> = primes.iter().filter(|p| !chosen.contains(p) && left.iter().any(|m| p.covers(*m))).copied().collect();
    let left: Vec<u32> = left.into_iter().collect();
    let greedy = greedy(&left, &rest);
    if n > EXACT_UP_TO {
        chosen.extend(greedy);
        return (chosen, false);
    }
    // Branch and bound: the first row not yet covered, each prime covering it.
    let cost = |ts: &[Term]| (ts.len(), ts.iter().map(|t| t.literals()).sum::<u32>());
    let mut best = greedy.clone();
    let mut nodes = 0usize;
    fn search(left: &[u32], rest: &[Term], picked: &mut Vec<Term>, best: &mut Vec<Term>, nodes: &mut usize, cost: &dyn Fn(&[Term]) -> (usize, u32)) {
        *nodes += 1;
        if *nodes > MOST_NODES || picked.len() >= best.len() && cost(picked) >= cost(best) {
            return;
        }
        let Some(&m) = left.iter().find(|m| !picked.iter().any(|p| p.covers(**m))) else {
            if cost(picked) < cost(best) {
                *best = picked.clone();
            }
            return;
        };
        let mut options: Vec<&Term> = rest.iter().filter(|p| p.covers(m)).collect();
        options.sort_by_key(|p| std::cmp::Reverse(left.iter().filter(|x| p.covers(**x)).count()));
        for p in options {
            picked.push(*p);
            search(left, rest, picked, best, nodes, cost);
            picked.pop();
        }
    }
    search(&left, &rest, &mut Vec::new(), &mut best, &mut nodes, &cost);
    let exact = nodes <= MOST_NODES;
    chosen.extend(best);
    (chosen, exact)
}

fn greedy(left: &[u32], rest: &[Term]) -> Vec<Term> {
    let mut left: BTreeSet<u32> = left.iter().copied().collect();
    let mut out = Vec::new();
    while !left.is_empty() {
        let Some(p) = rest.iter().max_by_key(|p| (left.iter().filter(|m| p.covers(**m)).count(), std::cmp::Reverse(p.literals()))) else { break };
        left.retain(|m| !p.covers(*m));
        out.push(*p);
    }
    out
}

/// An output of a table minimised, both ways.
pub fn minimise(n: usize, column: &[u8]) -> Result<Minimal, String> {
    let rows = |want: u8| -> Vec<u32> { column.iter().enumerate().filter(|(_, v)| **v == want).map(|(i, _)| i as u32).collect() };
    let (ones, zeros, dont) = (rows(b'1'), rows(b'0'), rows(b'x'));
    let p1 = primes(n, &ones, &dont)?;
    let (mut sop, e1) = cover(n, &ones, &p1);
    let p0 = primes(n, &zeros, &dont)?;
    let (mut pos, e0) = cover(n, &zeros, &p0);
    sop.sort();
    pos.sort();
    Ok(Minimal { sop, pos, exact: e1 && e0 })
}

/// A term as a product of literals: `A·B'`, `1` for none.
pub fn product(t: &Term, names: &[String]) -> String {
    let n = names.len();
    let lits: Vec<String> = (0..n)
        .filter(|i| t.care >> (n - 1 - i) & 1 == 1)
        .map(|i| if t.value >> (n - 1 - i) & 1 == 1 { names[i].clone() } else { format!("{}'", names[i]) })
        .collect();
    if lits.is_empty() {
        "1".into()
    } else {
        lits.join("·")
    }
}

/// A cover of the zeros as a clause: `(A' + B)`, each literal the opposite
/// of the term's bit.
pub fn clause(t: &Term, names: &[String]) -> String {
    let n = names.len();
    let lits: Vec<String> = (0..n)
        .filter(|i| t.care >> (n - 1 - i) & 1 == 1)
        .map(|i| if t.value >> (n - 1 - i) & 1 == 1 { format!("{}'", names[i]) } else { names[i].clone() })
        .collect();
    if lits.is_empty() {
        "0".into()
    } else {
        format!("({})", lits.join(" + "))
    }
}

pub fn sop_text(m: &Minimal, names: &[String]) -> String {
    if m.sop.is_empty() {
        return "0".into();
    }
    m.sop.iter().map(|t| product(t, names)).collect::<Vec<_>>().join(" + ")
}

pub fn pos_text(m: &Minimal, names: &[String]) -> String {
    if m.pos.is_empty() {
        return "1".into();
    }
    m.pos.iter().map(|t| clause(t, names)).collect::<Vec<_>>().join("·")
}

/* ── expressions ──────────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Var(String),
    Const(bool),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    Num(bool),
    Not,
    Prime,
    And,
    Or,
    Xor,
    Open,
    Close,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        i += 1;
        match c {
            c if c.is_whitespace() => {}
            '!' | '~' | '¬' => out.push(Tok::Not),
            '\'' | '’' => out.push(Tok::Prime),
            '&' | '*' | '·' | '∧' | '.' => out.push(Tok::And),
            '|' | '+' | '∨' => out.push(Tok::Or),
            '^' | '⊕' => out.push(Tok::Xor),
            '(' => out.push(Tok::Open),
            ')' => out.push(Tok::Close),
            '0' => out.push(Tok::Num(false)),
            '1' => out.push(Tok::Num(true)),
            c if c.is_alphabetic() || c == '_' => {
                let mut name = c.to_string();
                while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                    name.push(cs[i]);
                    i += 1;
                }
                match name.to_ascii_lowercase().as_str() {
                    "and" => out.push(Tok::And),
                    "or" => out.push(Tok::Or),
                    "xor" => out.push(Tok::Xor),
                    "not" => out.push(Tok::Not),
                    _ => out.push(Tok::Name(name)),
                }
            }
            other => return Err(format!("“{other}” is not part of an expression")),
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn or(&mut self) -> Result<Expr, String> {
        let mut e = self.xor()?;
        while self.peek() == Some(&Tok::Or) {
            self.at += 1;
            e = Expr::Or(Box::new(e), Box::new(self.xor()?));
        }
        Ok(e)
    }
    fn xor(&mut self) -> Result<Expr, String> {
        let mut e = self.and()?;
        while self.peek() == Some(&Tok::Xor) {
            self.at += 1;
            e = Expr::Xor(Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut e = self.not()?;
        loop {
            match self.peek() {
                Some(Tok::And) => {
                    self.at += 1;
                    e = Expr::And(Box::new(e), Box::new(self.not()?));
                }
                // Side by side: `A B` and `A(B + C)` are products too.
                Some(Tok::Name(_) | Tok::Num(_) | Tok::Open | Tok::Not) => e = Expr::And(Box::new(e), Box::new(self.not()?)),
                _ => return Ok(e),
            }
        }
    }
    fn not(&mut self) -> Result<Expr, String> {
        if self.peek() == Some(&Tok::Not) {
            self.at += 1;
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        let mut e = match self.toks.get(self.at).cloned() {
            Some(Tok::Name(n)) => {
                self.at += 1;
                Expr::Var(n)
            }
            Some(Tok::Num(b)) => {
                self.at += 1;
                Expr::Const(b)
            }
            Some(Tok::Open) => {
                self.at += 1;
                let e = self.or()?;
                if self.peek() != Some(&Tok::Close) {
                    return Err("a bracket is not closed".into());
                }
                self.at += 1;
                e
            }
            Some(t) => return Err(format!("{t:?} where a name or a bracket was looked for").to_lowercase()),
            None => return Err("the expression ends too soon".into()),
        };
        while self.peek() == Some(&Tok::Prime) {
            self.at += 1;
            e = Expr::Not(Box::new(e));
        }
        Ok(e)
    }
}

fn vars(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::Var(v) => {
            out.insert(v.clone());
        }
        Expr::Const(_) => {}
        Expr::Not(a) => vars(a, out),
        Expr::And(a, b) | Expr::Or(a, b) | Expr::Xor(a, b) => {
            vars(a, out);
            vars(b, out);
        }
    }
}

fn eval(e: &Expr, env: &HashMap<&str, bool>) -> bool {
    match e {
        Expr::Var(v) => env.get(v.as_str()).copied().unwrap_or(false),
        Expr::Const(b) => *b,
        Expr::Not(a) => !eval(a, env),
        Expr::And(a, b) => eval(a, env) && eval(b, env),
        Expr::Or(a, b) => eval(a, env) || eval(b, env),
        Expr::Xor(a, b) => eval(a, env) ^ eval(b, env),
    }
}

/// Expressions as a table: `Y = A·B + C'; Z = A ^ B`, or one unnamed, `Y`.
/// The variables are every name used, sorted.
pub fn expressions(text: &str) -> Result<Table, String> {
    let mut outs: Vec<(String, Expr)> = Vec::new();
    for (k, part) in text.split([';', '\n']).map(str::trim).filter(|p| !p.is_empty()).enumerate() {
        let (name, body) = match part.split_once('=') {
            Some((n, b)) if edit::is_id(n.trim()) => (n.trim().to_string(), b),
            Some(_) => return Err(format!("“{part}” does not name its output")),
            None => (if k == 0 { "Y".to_string() } else { format!("Y{k}") }, part),
        };
        let mut p = Parser { toks: lex(body)?, at: 0 };
        let e = p.or().map_err(|e| format!("{name}: {e}"))?;
        if p.at != p.toks.len() {
            return Err(format!("{name}: something follows where the expression should end"));
        }
        outs.push((name, e));
    }
    if outs.is_empty() {
        return Err("there is no expression".into());
    }
    let mut names = BTreeSet::new();
    for (_, e) in &outs {
        vars(e, &mut names);
    }
    let inputs: Vec<String> = names.into_iter().collect();
    if inputs.len() > MOST_VARIABLES {
        return Err(format!("{} variables: no more than {MOST_VARIABLES}", inputs.len()));
    }
    let n = inputs.len();
    let mut values = vec![Vec::with_capacity(1 << n); outs.len()];
    for row in 0..1u32 << n {
        let env: HashMap<&str, bool> = inputs.iter().enumerate().map(|(i, v)| (v.as_str(), row >> (n - 1 - i) & 1 == 1)).collect();
        for (k, (_, e)) in outs.iter().enumerate() {
            values[k].push(if eval(e, &env) { b'1' } else { b'0' });
        }
    }
    Ok(Table { inputs, outputs: outs.into_iter().map(|(n, _)| n).collect(), values })
}

/* ── tables of circuits ───────────────────────────────────────────── */

/// A circuit's truth table: each bit of each input a variable (`A` for one
/// bit, `A1 A0` for two), each bit of each output a column. Refused for a
/// circuit that keeps state or has a clock.
pub fn table_of(built: &Built, inputs: &[(String, u8)], outputs: &[(String, u8)]) -> Result<Table, String> {
    let sim = crate::Sim::new(built.netlist.clone(), 0x5eed).map_err(|e| e.to_string())?;
    if sim.clocked() > 0 {
        return Err("this circuit keeps state: a truth table is of circuits without flip-flops, registers or memory".into());
    }
    let n: usize = inputs.iter().map(|(_, b)| *b as usize).sum();
    if n > MOST_VARIABLES {
        return Err(format!("its inputs are {n} bits: a truth table takes no more than {MOST_VARIABLES}"));
    }
    if outputs.is_empty() {
        return Err("this circuit has no outputs".into());
    }
    let bit_names = |(name, bits): &(String, u8)| -> Vec<String> { if *bits == 1 { vec![name.clone()] } else { (0..*bits).rev().map(|b| format!("{name}{b}")).collect() } };
    let mut sim = sim;
    let mut table = Table {
        inputs: inputs.iter().flat_map(bit_names).collect(),
        outputs: outputs.iter().flat_map(bit_names).collect(),
        values: Vec::new(),
    };
    table.values = vec![Vec::with_capacity(1 << n); table.outputs.len()];
    for row in 0..1u32 << n {
        let mut shift = n;
        for (name, bits) in inputs {
            shift -= *bits as usize;
            let v = (row >> shift) as u64 & crate::value::mask(*bits);
            let (part, _) = built.inputs.get(name).ok_or_else(|| format!("no input {name}"))?;
            sim.set(*part, v).map_err(|e| e.to_string())?;
        }
        let mut col = 0;
        for (name, bits) in outputs {
            let part = built.outputs.get(name).ok_or_else(|| format!("no output {name}"))?;
            let v = sim.shown(*part);
            for b in (0..*bits).rev() {
                let z = v.z >> b & 1 == 1;
                table.values[col].push(if z { b'x' } else if v.v >> b & 1 == 1 { b'1' } else { b'0' });
                col += 1;
            }
        }
    }
    Ok(table)
}

/// The truth table of a project's circuit: its inputs and outputs in id
/// order, by label.
pub fn project_table(p: &Project, circuit: &str) -> Result<Table, String> {
    let d = p.circuits.get(circuit).ok_or_else(|| format!("there is no circuit {circuit}"))?;
    let built = p.build(circuit)?;
    let mut ins = Vec::new();
    let mut outs = Vec::new();
    for part in crate::circ::ordered(d) {
        let label = part.attrs.get("label").cloned().unwrap_or_default();
        let bits: u8 = part.attrs.get("bits").and_then(|b| b.parse().ok()).unwrap_or(1);
        match part.kind.as_str() {
            "in" if !label.is_empty() => ins.push((label, bits)),
            "clock" => return Err("this circuit has a clock: a truth table is of circuits without one".into()),
            "out" if !label.is_empty() => outs.push((label, bits)),
            _ => {}
        }
    }
    table_of(&built, &ins, &outs)
}

/* ── synthesis ────────────────────────────────────────────────────── */

/// How a cover is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// AND gates into an OR: the sum of products.
    AndOr,
    /// NAND gates into a NAND: the sum of products, NOT gates as one-input NANDs.
    Nand,
    /// NOR gates into a NOR: the product of sums, NOT gates as one-input NORs.
    Nor,
}

/// A circuit built from a table's minimal forms: its inputs, the gates for
/// each output, and its outputs, every junction a wire's end. Its own parts'
/// ids start with a lower-case letter and `_`, as no signal's does.
pub fn synthesise(table: &Table, style: Style) -> Result<Drawing, String> {
    let n = table.inputs.len();
    let mut covers = Vec::new();
    for col in &table.values {
        let m = minimise(n, col)?;
        covers.push(if style == Style::Nor { m.pos } else { m.sop });
    }
    let (gate, last, not) = match style {
        Style::AndOr => ("and", "or", "not"),
        Style::Nand => ("nand", "nand", "nand"),
        Style::Nor => ("nor", "nor", "nor"),
    };
    // Which literals are used: a variable straight (true) or inverted.
    // For NOR, a clause's literal is the opposite of its term's bit, and
    // the gate is fed the literal itself.
    let literal = |t: &Term, i: usize| -> Option<bool> {
        (t.care >> (n - 1 - i) & 1 == 1).then(|| {
            let bit = t.value >> (n - 1 - i) & 1 == 1;
            if style == Style::Nor { !bit } else { bit }
        })
    };
    let mut need = vec![(false, false); n];
    for cover in &covers {
        for t in cover {
            for (i, need) in need.iter_mut().enumerate() {
                match literal(t, i) {
                    Some(true) => need.0 = true,
                    Some(false) => need.1 = true,
                    None => {}
                }
            }
        }
    }
    let mut d = Drawing::default();
    let part = |id: String, kind: &str, x: i32, y: i32, attrs: &[(&str, String)]| Part {
        id,
        kind: kind.into(),
        x,
        y,
        rot: 0,
        mirror: false,
        attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
    };
    // Inputs down the left, two steps apart; each feeds its rail, and its
    // complement's rail through a NOT — a one-input NAND or NOR, a step
    // wider for its bubble.
    let not_w = if not == "not" { 2 } else { 4 };
    let true_x = |i: usize| 3 + 6 * i as i32;
    let false_x = |i: usize| true_x(i) + 1 + not_w;
    let mut rails: Vec<[Option<(i32, i32, i32)>; 2]> = vec![[None, None]; n];
    for (i, name) in table.inputs.iter().enumerate() {
        let y = 2 * i as i32;
        d.parts.push(part(name.clone(), "in", 0, y, &[("label", name.clone())]));
        let (straight, inverted) = need[i];
        let x = true_x(i);
        d.wires.push(Wire::new((0, y), (x, y)));
        if straight {
            rails[i][0] = Some((x, y, y));
        }
        if inverted {
            let id = format!("n_{i}");
            let inputs = if not == "not" { vec![] } else { vec![("inputs", "1".to_string())] };
            d.parts.push(part(id, not, x + 1, y, &inputs));
            d.wires.push(Wire::new((x, y), (x + 1, y)));
            rails[i][1] = Some((false_x(i), y, y));
        }
    }
    let gates_x = false_x(n.saturating_sub(1)) + 3;
    let mut y = 2 * n as i32 + 1;
    let mut taps: Vec<((i32, i32), (i32, i32))> = Vec::new();
    for (k, (cover, name)) in covers.iter().zip(&table.outputs).enumerate() {
        let out_id = name.clone();
        // A constant: no gates.
        let always = |value: &str, d: &mut Drawing, y: i32| {
            d.parts.push(part(format!("k_{k}"), "const", gates_x, y, &[("value", value.to_string())]));
            d.parts.push(part(out_id.clone(), "out", gates_x + 4, y, &[("label", out_id.clone())]));
            d.wires.push(Wire::new((gates_x, y), (gates_x + 4, y)));
        };
        let none_left = if style == Style::Nor { "1" } else { "0" };
        let all = if style == Style::Nor { "0" } else { "1" };
        if cover.is_empty() {
            always(none_left, &mut d, y);
            y += 3;
            continue;
        }
        if cover.iter().any(|t| t.care == 0) {
            always(all, &mut d, y);
            y += 3;
            continue;
        }
        // A gate a term, stacked; each input tapped from its rail.
        let mut outs: Vec<(i32, i32)> = Vec::new();
        for (j, t) in cover.iter().enumerate() {
            let lits: Vec<(usize, bool)> = (0..n).filter_map(|i| literal(t, i).map(|b| (i, b))).collect();
            let m = lits.len() as i32;
            let slots = if m % 2 == 0 { m + 1 } else { m };
            let id = format!("g_{k}_{j}");
            d.parts.push(part(id, gate, gates_x, y, &[("inputs", m.to_string())]));
            for (s, (i, b)) in lits.iter().enumerate() {
                let s = s as i32;
                let py = y + if m % 2 == 0 && s >= m / 2 { s + 1 } else { s };
                let rail = &mut rails[*i][if *b { 0 } else { 1 }];
                if let Some((rx, _, bottom)) = rail {
                    *bottom = (*bottom).max(py);
                    taps.push(((*rx, py), (gates_x, py)));
                }
            }
            let w = if gate == "and" || gate == "or" { 3 } else { 4 };
            outs.push((gates_x + w, y + (slots - 1) / 2));
            y += slots + 1;
        }
        // The last gate: its inputs on rows no term's output is on, so no
        // route's end lies along another's.
        let t = cover.len() as i32;
        let route_x = gates_x + 5;
        let last_x = route_x + t + 1;
        let slots = if t % 2 == 0 { t + 1 } else { t };
        let taken: BTreeSet<i32> = outs.iter().map(|o| o.1).collect();
        let mut ly = outs[0].1;
        let pins_at = |ly: i32| -> Vec<i32> { (0..t).map(|s| ly + if t % 2 == 0 && s >= t / 2 { s + 1 } else { s }).collect() };
        while pins_at(ly).iter().any(|py| taken.contains(py)) {
            ly += 1;
        }
        let last_w = if last == "or" { 3 } else { 4 };
        if t == 1 && style == Style::AndOr {
            // One term: its gate is the output.
            let (ox, oy) = outs[0];
            d.parts.push(part(out_id.clone(), "out", ox + 2, oy, &[("label", out_id.clone())]));
            d.wires.push(Wire::new((ox, oy), (ox + 2, oy)));
            continue;
        }
        d.parts.push(part(format!("f_{k}"), last, last_x, ly, &[("inputs", t.to_string())]));
        for (s, (&(ox, oy), py)) in outs.iter().zip(pins_at(ly)).enumerate() {
            let mx = route_x + s as i32;
            for (a, b) in [((ox, oy), (mx, oy)), ((mx, oy), (mx, py)), ((mx, py), (last_x, py))] {
                if a != b {
                    d.wires.push(Wire::new(a, b));
                }
            }
        }
        let oy = ly + (slots - 1) / 2;
        let ox = last_x + last_w;
        d.parts.push(part(out_id.clone(), "out", ox + 2, oy, &[("label", out_id.clone())]));
        d.wires.push(Wire::new((ox, oy), (ox + 2, oy)));
        y = y.max(ly + slots + 1);
    }
    // The rails, split at every tap so each joins where it ends.
    for pair in &rails {
        for rail in pair {
            let Some((x, top, bottom)) = rail else { continue };
            let mut ys: Vec<i32> = taps.iter().filter(|(a, _)| a.0 == *x).map(|(a, _)| a.1).collect();
            ys.push(*top);
            ys.push(*bottom);
            ys.sort();
            ys.dedup();
            for pair in ys.windows(2) {
                d.wires.push(Wire::new((*x, pair[0]), (*x, pair[1])));
            }
        }
    }
    for (a, b) in taps {
        d.wires.push(Wire::new(a, b));
    }
    // Wires a straight line apart, crossing others only in their middles.
    d.wires.sort();
    d.wires.dedup();
    let text = crate::circ::write(&d);
    crate::circ::parse(&text)
}

/// Rows of a table as text: the header, then a line a row.
pub fn table_text(t: &Table) -> String {
    let mut s = format!("{} | {}\n", t.inputs.join(" "), t.outputs.join(" "));
    let n = t.inputs.len();
    for row in 0..1usize << n {
        let ins: Vec<String> = (0..n).map(|i| ((row >> (n - 1 - i)) & 1).to_string()).collect();
        let outs: Vec<String> = t.values.iter().map(|c| (c[row] as char).to_string()).collect();
        s.push_str(&format!("{} | {}\n", ins.join(" "), outs.join(" ")));
    }
    s
}

/// A table as a test table in Digital's language, for the Tests panel.
pub fn as_test(t: &Table) -> String {
    let mut s = format!("{} {}\n", t.inputs.join(" "), t.outputs.join(" "));
    let n = t.inputs.len();
    for row in 0..1usize << n {
        let ins: Vec<String> = (0..n).map(|i| ((row >> (n - 1 - i)) & 1).to_string()).collect();
        let outs: Vec<String> = t.values.iter().map(|c| if c[row] == b'x' { "x".into() } else { (c[row] as char).to_string() }).collect();
        s.push_str(&format!("{} {}\n", ins.join(" "), outs.join(" ")));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(s: &str) -> Vec<u8> {
        s.bytes().collect()
    }

    #[test]
    fn quine_mccluskey_finds_the_least_sums_and_products() {
        let names: Vec<String> = ["A", "B", "C"].iter().map(|s| s.to_string()).collect();
        // A majority of three: AB + AC + BC.
        let m = minimise(3, &col("00010111")).unwrap();
        assert_eq!(sop_text(&m, &names), "B·C + A·C + A·B");
        assert_eq!(pos_text(&m, &names), "(B + C)·(A + C)·(A + B)");
        assert!(m.exact);
        // XOR has no simpler form; don't cares are used.
        let x = minimise(2, &col("0110")).unwrap();
        assert_eq!(sop_text(&x, &names[..2]), "A'·B + A·B'");
        let d = minimise(2, &col("01x1")).unwrap();
        assert_eq!(sop_text(&d, &names[..2]), "B", "the don't care is not needed to cover row 3");
        assert_eq!(sop_text(&minimise(2, &col("0000")).unwrap(), &names[..2]), "0");
        assert_eq!(sop_text(&minimise(2, &col("1111")).unwrap(), &names[..2]), "1");
        // The cyclic function, whose cover has no essential primes: four terms of two.
        let cyc = minimise(3, &col("10111101")).unwrap();
        assert_eq!(cyc.sop.len(), 3, "{}", sop_text(&cyc, &names));
    }

    #[test]
    fn expressions_read_every_way_they_are_written() {
        for e in ["A·B + C'", "A & B | !C", "(A B) + ~C", "A and B or not C", "C' + A*B"] {
            let t = expressions(e).unwrap();
            assert_eq!((t.inputs.join(""), String::from_utf8(t.values[0].clone()).unwrap()), ("ABC".into(), "10101011".into()), "{e}");
        }
        let t = expressions("S = A ^ B; C = A B").unwrap();
        assert_eq!(t.outputs, vec!["S", "C"]);
        assert_eq!(t.values, vec![col("0110"), col("0001")]);
        assert!(expressions("A + (B").is_err() && expressions("A $ B").is_err() && expressions("").is_err());
    }

    #[test]
    fn a_circuit_built_from_a_table_has_that_table_every_style() {
        let t = expressions("S = A ^ B ^ C; Co = A B + A C + B C").unwrap();
        for style in [Style::AndOr, Style::Nand, Style::Nor] {
            let d = synthesise(&t, style).unwrap();
            let mut p = Project::new();
            p.circuits.insert("main".into(), d);
            let back = project_table(&p, "main").unwrap_or_else(|e| panic!("{style:?}: {e}"));
            assert_eq!(back.inputs, t.inputs);
            for (name, col) in t.outputs.iter().zip(&t.values) {
                let k = back.outputs.iter().position(|o| o == name).unwrap();
                assert_eq!(&back.values[k], col, "{style:?} {name}\n{}", crate::circ::write(&p.circuits["main"]));
            }
        }
        // A constant output, and one that is a single term.
        let t = expressions("Z = A A'; P = A B").unwrap();
        let mut p = Project::new();
        p.circuits.insert("main".into(), synthesise(&t, Style::AndOr).unwrap());
        let back = project_table(&p, "main").unwrap();
        assert_eq!((back.outputs.clone(), back.values), (vec!["P".to_string(), "Z".to_string()], vec![t.values[1].clone(), t.values[0].clone()]));
    }

    #[test]
    fn random_functions_minimise_and_build_true_to_their_tables() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let names: Vec<String> = ["A", "B", "C", "D"].iter().map(|s| s.to_string()).collect();
        for _ in 0..60 {
            let column: Vec<u8> = (0..16).map(|_| match next() % 5 { 0 => b'x', 1 | 2 => b'1', _ => b'0' }).collect();
            let m = minimise(4, &column).unwrap();
            // The sum of products is 1 on every 1 and 0 on every 0.
            for (row, v) in column.iter().enumerate() {
                let sop = m.sop.iter().any(|t| t.covers(row as u32));
                let pos = !m.pos.iter().any(|t| t.covers(row as u32));
                match v {
                    b'1' => assert!(sop && pos, "row {row} of {}", String::from_utf8_lossy(&column)),
                    b'0' => assert!(!sop && !pos, "row {row} of {}", String::from_utf8_lossy(&column)),
                    _ => {}
                }
            }
            let t = Table { inputs: names.clone(), outputs: vec!["Y".into()], values: vec![column.clone()] };
            for style in [Style::AndOr, Style::Nand, Style::Nor] {
                let mut p = Project::new();
                p.circuits.insert("main".into(), synthesise(&t, style).unwrap());
                let back = project_table(&p, "main").unwrap();
                for (row, v) in column.iter().enumerate() {
                    if *v != b'x' {
                        assert_eq!(back.values[0][row], *v, "{style:?}, row {row} of {}", String::from_utf8_lossy(&column));
                    }
                }
            }
        }
    }

    #[test]
    fn a_circuit_with_a_flip_flop_has_no_truth_table() {
        let mut p = Project::new();
        p.circuits.insert("main".into(), crate::circ::parse("part D in at 0,0 label=D\npart C in at 0,1 label=C\npart F dff at 2,0\npart Q out at 6,0 label=Q\nwire 0,0 2,0\nwire 0,1 2,1\nwire 5,0 6,0\n").unwrap());
        assert!(project_table(&p, "main").unwrap_err().contains("keeps state"));
        assert_eq!(table_text(&expressions("A'").unwrap()), "A | Y\n0 | 1\n1 | 0\n");
        assert_eq!(as_test(&expressions("A'").unwrap()), "A Y\n0 1\n1 0\n");
    }
}
