//! Test tables, in the language Digital's test cases are written in, run
//! against a circuit.
//!
//! ```text
//! A B Cin   S Cout          # the signals: inputs, then what is checked
//! 0 0 0     0 0             # a row: inputs set, then outputs checked
//! loop(i, 8)                # rows written many times
//!   (i>>2) (i>>1&1) (i&1)  ((i^(i>>1)^(i>>2))&1) X
//! end loop
//! let a = random(1 << 16);  # a variable
//! init R1=0x87;             # a register's value before the rows
//! program(0x8321,0x8432)    # the program memory's words
//! declare Z = FL & 1;       # a signal worked out from others
//! ```
//!
//! A value is a number (`12`, `0x1f`, `0b101`), `(an expression)`, `X`
//! for anything, `Z` for high-Z, or `C` for a clock pulse — up, settled,
//! down, settled — before the outputs are read.

use std::collections::HashMap;

use crate::dig::Built;
use crate::sim::{PartId, Sim};
use crate::value::mask;

/// A test case's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub label: String,
    pub rows: usize,
    /// The rows that failed, the first few: what was set, what differed.
    pub failures: Vec<String>,
    /// Why it could not run at all, if it could not.
    pub error: Option<String>,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.failures.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    Id(String),
    Op(&'static str),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    // `=` is equality, as in Digital's tests; `==` too.
    const OPS: [&str; 23] = ["<<", ">>", "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/", "%", "&", "|", "^", "~", "!", "<", ">", "(", ")", "="];
    while i < c.len() {
        if c[i].is_whitespace() {
            i += 1;
            continue;
        }
        if c[i].is_ascii_digit() {
            let start = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let t: String = c[start..i].iter().filter(|ch| **ch != '_').collect();
            let n = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                i64::from_str_radix(h, 16)
            } else if let Some(b) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
                i64::from_str_radix(b, 2)
            } else {
                t.parse()
            };
            out.push(Tok::Num(n.map_err(|_| format!("{t} is not a number"))?));
            continue;
        }
        if c[i].is_alphabetic() || c[i] == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Id(c[start..i].iter().collect()));
            continue;
        }
        if c[i] == ',' {
            out.push(Tok::Op(","));
            i += 1;
            continue;
        }
        let rest: String = c[i..(i + 2).min(c.len())].iter().collect();
        match OPS.iter().find(|op| rest.starts_with(**op)) {
            Some(op) => {
                out.push(Tok::Op(op));
                i += op.len();
            }
            None => return Err(format!("“{}” is not understood", c[i])),
        }
    }
    Ok(out)
}

/// An expression's tree.
#[derive(Debug, Clone)]
enum Expr {
    Num(i64),
    Var(String),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    i: usize,
}

fn power(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "=" | "!=" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn expr(&mut self, min: u8) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        while let Some(Tok::Op(op)) = self.peek() {
            let op = *op;
            let Some(p) = power(op) else { break };
            if p < min {
                break;
            }
            self.i += 1;
            let rhs = self.expr(p + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.toks.get(self.i).cloned() {
            Some(Tok::Op(op @ ("-" | "~" | "!"))) => {
                self.i += 1;
                Ok(Expr::Unary(op, Box::new(self.unary()?)))
            }
            Some(Tok::Op("(")) => {
                self.i += 1;
                let e = self.expr(0)?;
                if self.peek() != Some(&Tok::Op(")")) {
                    return Err("a ( is not closed".into());
                }
                self.i += 1;
                Ok(e)
            }
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(Expr::Num(n))
            }
            Some(Tok::Id(name)) => {
                self.i += 1;
                if self.peek() == Some(&Tok::Op("(")) {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::Op(")")) {
                        loop {
                            args.push(self.expr(0)?);
                            match self.peek() {
                                Some(Tok::Op(",")) => self.i += 1,
                                Some(Tok::Op(")")) => break,
                                _ => return Err(format!("{name}( is not closed")),
                            }
                        }
                    }
                    self.i += 1;
                    return Ok(Expr::Call(name, args));
                }
                Ok(Expr::Var(name))
            }
            other => Err(format!("an expression was expected, not {other:?}")),
        }
    }
}

fn parse_expr(s: &str) -> Result<Expr, String> {
    let mut p = Parser { toks: lex(s)?, i: 0 };
    let e = p.expr(0)?;
    if p.i != p.toks.len() {
        return Err(format!("something follows the end of “{s}”"));
    }
    Ok(e)
}

struct Env<'a> {
    vars: HashMap<String, i64>,
    rng: u64,
    /// Signals the expressions may read: the circuit's values now.
    signal: &'a dyn Fn(&str) -> Option<i64>,
}

impl Env<'_> {
    fn eval(&mut self, e: &Expr) -> Result<i64, String> {
        Ok(match e {
            Expr::Num(n) => *n,
            Expr::Var(v) => match self.vars.get(v) {
                Some(x) => *x,
                None => (self.signal)(v).ok_or_else(|| format!("{v} is not known"))?,
            },
            Expr::Unary(op, a) => {
                let a = self.eval(a)?;
                match *op {
                    "-" => a.wrapping_neg(),
                    "~" => !a,
                    _ => (a == 0) as i64,
                }
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.eval(a)?, self.eval(b)?);
                match *op {
                    "+" => a.wrapping_add(b),
                    "-" => a.wrapping_sub(b),
                    "*" => a.wrapping_mul(b),
                    "/" if b == 0 => return Err("division by zero".into()),
                    "/" => a.wrapping_div(b),
                    "%" if b == 0 => return Err("division by zero".into()),
                    "%" => a.wrapping_rem(b),
                    "&" => a & b,
                    "|" => a | b,
                    "^" => a ^ b,
                    "<<" => a.wrapping_shl(b as u32),
                    ">>" => ((a as u64).wrapping_shr(b as u32)) as i64,
                    "==" | "=" => (a == b) as i64,
                    "!=" => (a != b) as i64,
                    "<" => (a < b) as i64,
                    ">" => (a > b) as i64,
                    "<=" => (a <= b) as i64,
                    ">=" => (a >= b) as i64,
                    "&&" => (a != 0 && b != 0) as i64,
                    _ => (a != 0 || b != 0) as i64,
                }
            }
            Expr::Call(f, args) => {
                let v: Result<Vec<i64>, String> = args.iter().map(|a| self.eval(a)).collect();
                let v = v?;
                match (f.as_str(), v.as_slice()) {
                    ("random", [n]) => {
                        self.rng ^= self.rng << 13;
                        self.rng ^= self.rng >> 7;
                        self.rng ^= self.rng << 17;
                        if *n <= 0 { 0 } else { (self.rng % *n as u64) as i64 }
                    }
                    ("signExt", [x, bits]) => {
                        let b = (*bits).clamp(1, 64) as u32;
                        if b == 64 { *x } else { (x << (64 - b)) >> (64 - b) }
                    }
                    ("abs", [x]) => x.abs(),
                    _ => return Err(format!("{f} is not a function the tests know")),
                }
            }
        })
    }
}

/// One statement of a test, with its line for messages.
#[derive(Debug, Clone)]
enum Stmt {
    Row(Vec<String>, usize),
    Loop(String, Expr, Vec<Stmt>),
    Repeat(Expr, Vec<String>, usize),
    Let(String, Expr),
    Init(String, Expr),
    Program(Vec<Expr>),
    Declare(String, Expr),
}

/// A row's cells: words, and bracketed expressions kept whole.
fn cells(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for ch in line.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
                if depth == 0 {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The text of a call's brackets: `loop(i, 8)` → `i, 8`.
fn inside(s: &str) -> Option<&str> {
    let a = s.find('(')?;
    let b = s.rfind(')')?;
    (b > a).then(|| &s[a + 1..b])
}

fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur).trim().to_string());
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// A test's text read: its signals, and its statements.
fn read(text: &str) -> Result<(Vec<String>, Vec<Stmt>), String> {
    let lines: Vec<(usize, String)> = text
        .lines()
        .enumerate()
        .map(|(n, l)| (n + 1, l.split('#').next().unwrap_or("").trim().to_string()))
        .filter(|(_, l)| !l.is_empty())
        .collect();
    let Some((_, head)) = lines.first() else { return Err("the test is empty".into()) };
    let names: Vec<String> = head.split_whitespace().map(str::to_string).collect();
    let mut i = 1;
    let body = block(&lines, &mut i)?;
    Ok((names, body))
}

fn block(lines: &[(usize, String)], i: &mut usize) -> Result<Vec<Stmt>, String> {
    let mut out = Vec::new();
    while *i < lines.len() {
        let (n, line) = &lines[*i];
        let n = *n;
        *i += 1;
        let lower = line.to_ascii_lowercase();
        let stmt_end = |s: &str| s.trim().trim_end_matches(';').trim().to_string();
        if lower.starts_with("end loop") || lower == "end" {
            return Ok(out);
        }
        if lower.starts_with("loop(") || lower.starts_with("loop (") {
            let args = split_args(inside(line).ok_or_else(|| format!("line {n}: loop( is not closed"))?);
            let (var, count) = match args.as_slice() {
                [v, c] => (v.clone(), parse_expr(c).map_err(|e| format!("line {n}: {e}"))?),
                _ => return Err(format!("line {n}: loop takes a variable and a count")),
            };
            let body = block(lines, i)?;
            out.push(Stmt::Loop(var, count, body));
        } else if lower.starts_with("repeat(") || lower.starts_with("repeat (") {
            let close = line.find(')').ok_or_else(|| format!("line {n}: repeat( is not closed"))?;
            let count = parse_expr(&line[line.find('(').unwrap_or(0) + 1..close]).map_err(|e| format!("line {n}: {e}"))?;
            out.push(Stmt::Repeat(count, cells(&line[close + 1..]), n));
        } else if let Some(rest) = line.strip_prefix("let ") {
            let (v, e) = stmt_end(rest).split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).ok_or_else(|| format!("line {n}: let needs =") )?;
            out.push(Stmt::Let(v, parse_expr(&e).map_err(|err| format!("line {n}: {err}"))?));
        } else if let Some(rest) = line.strip_prefix("declare ") {
            let (v, e) = stmt_end(rest).split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).ok_or_else(|| format!("line {n}: declare needs =") )?;
            out.push(Stmt::Declare(v, parse_expr(&e).map_err(|err| format!("line {n}: {err}"))?));
        } else if let Some(rest) = line.strip_prefix("init ") {
            let (v, e) = stmt_end(rest).split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).ok_or_else(|| format!("line {n}: init needs =") )?;
            out.push(Stmt::Init(v, parse_expr(&e).map_err(|err| format!("line {n}: {err}"))?));
        } else if lower.starts_with("program(") || lower.starts_with("program (") {
            let words: Result<Vec<Expr>, String> = split_args(inside(line).unwrap_or("")).iter().map(|w| parse_expr(w)).collect();
            out.push(Stmt::Program(words.map_err(|e| format!("line {n}: {e}"))?));
        } else {
            out.push(Stmt::Row(cells(line), n));
        }
    }
    Ok(out)
}

/// What a signal name means in the circuit.
#[derive(Clone, Copy)]
enum Sig {
    Input(PartId, u8),
    Output(PartId),
    Register(PartId),
}

struct Runner<'a> {
    sim: Sim,
    names: Vec<String>,
    sigs: Vec<Option<Sig>>,
    declared: HashMap<String, Expr>,
    built: &'a Built,
    rows: usize,
    failures: Vec<String>,
    vars: HashMap<String, i64>,
    rng: u64,
}

impl Runner<'_> {
    fn read_signal(sim: &Sim, built: &Built, name: &str) -> Option<i64> {
        if let Some(&p) = built.outputs.get(name) {
            return Some(sim.shown(p).get() as i64);
        }
        if let Some(&p) = built.registers.get(name) {
            return Some(sim.output(p, 0).get() as i64);
        }
        built.inputs.get(name).map(|(p, _)| sim.output(*p, 0).get() as i64)
    }

    fn eval(&mut self, e: &Expr) -> Result<i64, String> {
        let sim = &self.sim;
        let built = self.built;
        let declared = self.declared.clone();
        let lookup = move |name: &str| -> Option<i64> {
            if let Some(d) = declared.get(name) {
                let mut env = Env { vars: HashMap::new(), rng: 1, signal: &|n| Runner::read_signal(sim, built, n) };
                return env.eval(d).ok();
            }
            Runner::read_signal(sim, built, name)
        };
        let mut env = Env { vars: self.vars.clone(), rng: self.rng, signal: &lookup };
        let v = env.eval(e);
        self.rng = env.rng;
        v
    }

    fn run(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for s in stmts {
            if self.failures.len() >= 10 {
                return Ok(());
            }
            match s {
                Stmt::Let(v, e) => {
                    let x = self.eval(e)?;
                    self.vars.insert(v.clone(), x);
                }
                Stmt::Declare(v, e) => {
                    self.declared.insert(v.clone(), e.clone());
                }
                Stmt::Init(name, e) => {
                    let x = self.eval(e)?;
                    let p = self.built.registers.get(name).ok_or_else(|| format!("init {name}: there is no register labelled {name} marked as a probe"))?;
                    self.sim.set(*p, x as u64).map_err(|e| e.to_string())?;
                }
                Stmt::Program(words) => {
                    let mut v = Vec::new();
                    for w in words {
                        v.push(self.eval(w)? as u64);
                    }
                    let p = self.built.program.ok_or("program(…) needs a RAM marked as the program memory")?;
                    self.sim.load(p, &v).map_err(|e| e.to_string())?;
                }
                Stmt::Loop(var, count, body) => {
                    let n = self.eval(count)?;
                    let saved = self.vars.get(var).copied();
                    for k in 0..n.max(0) {
                        self.vars.insert(var.clone(), k);
                        self.run(body)?;
                    }
                    match saved {
                        Some(x) => self.vars.insert(var.clone(), x),
                        None => self.vars.remove(var),
                    };
                }
                Stmt::Repeat(count, cells, line) => {
                    let n = self.eval(count)?;
                    for k in 0..n.max(0) {
                        self.vars.insert("n".into(), k);
                        self.row(cells, *line)?;
                    }
                }
                Stmt::Row(cells, line) => self.row(cells, *line)?,
            }
        }
        Ok(())
    }

    /// One row: inputs set, clocks pulsed, outputs checked.
    fn row(&mut self, cells: &[String], line: usize) -> Result<(), String> {
        if cells.len() != self.names.len() {
            return Err(format!("line {line}: {} values for {} signals", cells.len(), self.names.len()));
        }
        self.rows += 1;
        // An input that does not matter is tried both ways, 0 and 1, each
        // input so marked against every other, as Digital does.
        let free: Vec<(PartId, u8)> = cells.iter().enumerate().filter_map(|(k, c)| match self.sigs[k] {
            Some(Sig::Input(p, bits)) if c.eq_ignore_ascii_case("x") => Some((p, bits)),
            _ => None,
        }).collect();
        let free = &free[..free.len().min(8)];
        for combo in 0..1u32 << free.len() {
            for (i, (p, bits)) in free.iter().enumerate() {
                let v = (combo >> i & 1) as u64 & mask(*bits);
                self.sim.set(*p, v).map_err(|e| format!("line {line}: {e}"))?;
            }
            self.check_row(cells, line)?;
        }
        Ok(())
    }

    fn check_row(&mut self, cells: &[String], line: usize) -> Result<(), String> {
        let mut pulses = Vec::new();
        let mut checks = Vec::new();
        for (k, cell) in cells.iter().enumerate() {
            let lc = cell.to_ascii_lowercase();
            match self.sigs[k] {
                Some(Sig::Input(p, bits)) => {
                    if lc == "c" {
                        pulses.push(p);
                    } else if lc == "x" || lc == "z" {
                        // Set above, or left as it is.
                    } else {
                        let v = self.value(cell, line)?;
                        self.sim.set(p, v as u64 & mask(bits)).map_err(|e| format!("line {line}: {e}"))?;
                    }
                }
                _ => checks.push((k, cell.clone())),
            }
        }
        for p in pulses {
            self.sim.set(p, 1).map_err(|e| format!("line {line}: {e}"))?;
            self.sim.set(p, 0).map_err(|e| format!("line {line}: {e}"))?;
        }
        let mut wrong = Vec::new();
        for (k, cell) in checks {
            let lc = cell.to_ascii_lowercase();
            if lc == "x" {
                continue;
            }
            let name = self.names[k].clone();
            let got = match self.sigs[k] {
                Some(Sig::Output(p)) => self.sim.shown(p),
                Some(Sig::Register(p)) => self.sim.output(p, 0),
                _ => {
                    // A declared signal.
                    let e = self.declared.get(&name).cloned().ok_or_else(|| format!("{name} is not a signal of the circuit"))?;
                    let v = self.eval(&e)?;
                    crate::value::Value::new(v as u64, 64)
                }
            };
            if lc == "z" {
                if got.z != mask(got.bits) {
                    wrong.push(format!("{name} is {got}, not Z"));
                }
                continue;
            }
            let want = self.value(&cell, line)? as u64 & mask(got.bits);
            if got.is_z() || got.get() != want {
                wrong.push(format!("{name} is {got}, not {want:#x}"));
            }
        }
        if !wrong.is_empty() {
            self.failures.push(format!("line {line}, row {}: {}", self.rows, wrong.join("; ")));
        }
        Ok(())
    }

    fn value(&mut self, cell: &str, line: usize) -> Result<i64, String> {
        let e = parse_expr(cell).map_err(|e| format!("line {line}: {e}"))?;
        self.eval(&e).map_err(|e| format!("line {line}: {e}"))
    }
}

/// A test case run on a fresh copy of a built circuit.
pub fn run(built: &Built, label: &str, text: &str) -> Outcome {
    let mut out = Outcome { label: label.to_string(), rows: 0, failures: Vec::new(), error: None };
    let (names, body) = match read(text) {
        Ok(x) => x,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    let sim = match Sim::new(built.netlist.clone(), 0x5eed) {
        Ok(s) => s,
        Err(e) => {
            out.error = Some(e.to_string());
            return out;
        }
    };
    let sigs = names
        .iter()
        .map(|n| {
            if let Some((p, b)) = built.inputs.get(n) {
                Some(Sig::Input(*p, *b))
            } else if let Some(p) = built.outputs.get(n) {
                Some(Sig::Output(*p))
            } else {
                built.registers.get(n).map(|p| Sig::Register(*p))
            }
        })
        .collect();
    let mut r = Runner { sim, names, sigs, declared: HashMap::new(), built, rows: 0, failures: Vec::new(), vars: HashMap::new(), rng: 0x2545_f491_4f6c_dd1d };
    // The program is in memory before the first row, wherever the test
    // writes it.
    let (program, rest): (Vec<Stmt>, Vec<Stmt>) = body.into_iter().partition(|s| matches!(s, Stmt::Program(_)));
    if let Err(e) = r.run(&program).and_then(|_| r.run(&rest)) {
        out.error = Some(e);
    }
    out.rows = r.rows;
    out.failures = r.failures;
    out
}

/// Every test case of a built circuit, run.
pub fn run_all(built: &Built) -> Vec<Outcome> {
    built.tests.iter().map(|(l, t)| run(built, l, t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_cells_and_expressions_are_read() {
        assert_eq!(cells("(00) (a) (b) ((a + b) & 0xFFFF)"), vec!["(00)", "(a)", "(b)", "((a + b) & 0xFFFF)"]);
        assert_eq!(cells("c   0x0087 x"), vec!["c", "0x0087", "x"]);
        let mut env = Env { vars: HashMap::from([("i".to_string(), 5)]), rng: 7, signal: &|_| None };
        assert_eq!(env.eval(&parse_expr("(i^(i>>1)^(i>>2))&1").unwrap()), Ok(0));
        assert_eq!(env.eval(&parse_expr("1 << 4").unwrap()), Ok(16));
        assert_eq!(env.eval(&parse_expr("-1 & 0xF").unwrap()), Ok(15));
        let (names, body) = read("A B Y\n# c\nloop(i, 4)\n (i&1) (i>>1) ((i&1)&(i>>1))\nend loop\nlet q = 3;\ninit R1=0x87;\nprogram(1,2,0x3)\n").unwrap();
        assert_eq!(names, vec!["A", "B", "Y"]);
        assert!(matches!(&body[0], Stmt::Loop(v, _, inner) if v == "i" && inner.len() == 1));
        assert!(matches!(&body[2], Stmt::Init(n, _) if n == "R1"));
        assert!(matches!(&body[3], Stmt::Program(w) if w.len() == 3));
    }
}
