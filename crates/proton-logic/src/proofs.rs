//! A `.proof` file, whatever its system: its header read, each line split
//! into its number, what it says, its rule and what it cites, and the file
//! checked — or a proof found — by its system: [`fitch`](crate::fitch),
//! [`sequent`](crate::sequent) or [`resolution`](crate::resolution).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ast::Expr;
use crate::parse::{self, Diag};
use crate::{fitch, print, resolution, sequent};

/// The systems a `.proof` file can name, Fitch's first: a file naming none is his.
pub const SYSTEMS: [&str; 3] = ["fitch", "sequent", "resolution"];

/// A line's citations: a line, or a subproof's first and last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cite {
    Line(usize),
    Range(usize, usize),
}

/// What the header says, and what of it could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    pub system: String,
    pub premises: Vec<Expr>,
    pub goal: Option<Expr>,
    pub unread: Vec<Diag>,
}

/// The header read; the other lines given back, comments taken off, with
/// their line in the file, for the system's own reader.
pub fn header(text: &str) -> (Header, Vec<(usize, String)>) {
    let mut h = Header { system: "fitch".into(), ..Default::default() };
    let mut body = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let at = i + 1;
        let t = raw.split('#').next().unwrap_or("").trim();
        if t.is_empty() {
            continue;
        }
        let bad = |message: String| Diag { line: at, col: 0, message };
        if let Some(rest) = t.strip_prefix("system ") {
            h.system = rest.trim().to_lowercase();
            if !SYSTEMS.contains(&h.system.as_str()) {
                h.unread.push(bad(format!("the system “{}” is not one Proton checks: {}", rest.trim(), SYSTEMS.join(", "))));
            }
        } else if let Some(rest) = t.strip_prefix("premises") {
            match parse::formulas(rest.trim_start_matches(':')) {
                Ok(ps) => h.premises = ps,
                Err(d) => h.unread.push(bad(format!("premises: {}", d.message))),
            }
        } else if let Some(rest) = t.strip_prefix("goal") {
            match parse::formula(rest.trim_start_matches(':')) {
                Ok(g) => h.goal = Some(g),
                Err(d) => h.unread.push(bad(format!("goal: {}", d.message))),
            }
        } else {
            body.push((at, t.to_string()));
        }
    }
    (h, body)
}

/// A proof's line taken apart: `12  | | A ∧ B   ∧I 3, 7`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub n: usize,
    /// Its bars: how many subproofs it is in.
    pub depth: usize,
    pub body: String,
    /// The rule as written, one word or two.
    pub rule: String,
    pub cites: Vec<Cite>,
}

/// A line taken apart from the right: what it cites, then before that a
/// rule `known` knows — one word, or two — then what it says. `rules` names
/// them, for a line with none.
pub fn split(t: &str, known: &dyn Fn(&str) -> bool, rules: &str) -> Result<Split, String> {
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: usize = digits.parse().map_err(|_| "a proof's line starts with its number".to_string())?;
    let mut rest = t[digits.len()..].trim_start_matches(['.', ':', ')']).trim_start();
    let mut depth = 0;
    while let Some(r) = rest.strip_prefix('|') {
        // `|-` is a turnstile, not a bar.
        if r.starts_with('-') {
            break;
        }
        depth += 1;
        rest = r.trim_start();
    }
    let words: Vec<(usize, &str)> = rest.split_whitespace().map(|w| (w.as_ptr() as usize - rest.as_ptr() as usize, w)).collect();
    let mut k = words.len();
    while k > 0 && words[k - 1].1.chars().all(|c| c.is_ascii_digit() || matches!(c, ',' | '-' | '–')) {
        k -= 1;
    }
    let two = (k >= 2).then(|| format!("{} {}", words[k - 2].1, words[k - 1].1));
    let (rule, start) = match two {
        Some(w) if known(&w) => (w, k - 2),
        _ if k >= 1 && known(words[k - 1].1) => (words[k - 1].1.to_string(), k - 1),
        _ => return Err(format!("line {n} has no rule at its end: {rules}")),
    };
    let body = rest[..words.get(start).map_or(rest.len(), |w| w.0)].trim().to_string();
    let mut cites = Vec::new();
    for w in words[k..].iter().flat_map(|(_, w)| w.split(',')).filter(|w| !w.is_empty()) {
        let w = w.replace('–', "-");
        cites.push(match w.split_once('-') {
            Some((a, b)) => Cite::Range(a.parse().map_err(|_| format!("“{w}” is not a line"))?, b.parse().map_err(|_| format!("“{w}” is not a line"))?),
            None => Cite::Line(w.parse().map_err(|_| format!("“{w}” is not a line"))?),
        });
    }
    Ok(Split { n, depth, body, rule, cites })
}

/// The lines a line cites, each one line: `want` of them.
pub fn lines_cited(s: &Split, rule: &str, want: usize) -> Result<Vec<usize>, String> {
    let ns: Vec<usize> = s.cites.iter().filter_map(|c| if let Cite::Line(n) = c { Some(*n) } else { None }).collect();
    if ns.len() != want || ns.len() != s.cites.len() {
        return Err(format!("{rule} cites {want} line{}", if want == 1 { "" } else { "s" }));
    }
    Ok(ns)
}

/* ── the file checked, whatever its system ────────────────────────── */

/// A line checked: what it says, by which rule, and whether it is right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineReport {
    /// Its line in the file, from 1; its number in the proof.
    pub at: usize,
    pub n: usize,
    pub text: String,
    /// The rule by the name Proton writes it.
    pub rule: String,
    pub ok: bool,
    pub message: String,
}

/// A `.proof` file checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub header: Header,
    pub lines: Vec<LineReport>,
    /// Every line right and the proof's end reached.
    pub complete: bool,
    pub summary: String,
}

/// A `.proof` file checked by the system its header names.
pub fn check(text: &str) -> Report {
    let (h, _) = header(text);
    match h.system.as_str() {
        "sequent" => sequent::report(text),
        "resolution" => resolution::report(text),
        "fitch" => {
            let f = fitch::read(text);
            let c = fitch::check(&f);
            let lines = c.lines.iter().zip(&f.lines).map(|(c, l)| LineReport { at: c.at, n: c.n, text: print::expr(&l.formula), rule: fitch::rule_name(l.rule).into(), ok: c.ok, message: c.message.clone() }).collect();
            Report { header: Header { system: f.system, premises: f.premises, goal: f.goal, unread: f.unread }, lines, complete: c.complete, summary: c.summary }
        }
        // A system Proton does not check: the header's mistake says so.
        _ => Report { summary: "not checked".into(), header: h, ..Default::default() },
    }
}

/// A proof of `goal` from `premises` in `system`, found, as a `.proof`
/// file's text; or why none was.
pub fn find(system: &str, premises: &[Expr], goal: &Expr, time: Duration) -> Result<String, String> {
    match system {
        "fitch" => fitch::find(premises, goal, time),
        "sequent" => sequent::find(premises, goal, time),
        "resolution" => resolution::find(premises, goal, time),
        other => Err(format!("“{other}” is not a system Proton proves in: {}", SYSTEMS.join(", "))),
    }
}

/// The header a found proof is written with.
pub fn write_header(system: &str, premises: &[Expr], goal: &Expr) -> String {
    format!("system {system}\npremises {}\ngoal {}\n\n", premises.iter().map(print::expr).collect::<Vec<_>>().join(", "), print::expr(goal))
}

/// Numbered lines in columns: what each says, then its justification.
pub fn write_lines(rows: &[(String, String)]) -> String {
    let width = rows.iter().map(|(b, _)| b.chars().count()).max().unwrap_or(0);
    let digits = rows.len().to_string().len();
    rows.iter().enumerate().map(|(k, (b, j))| format!("{:>digits$}  {b:<width$}  {j}\n", k + 1)).collect()
}

/// The summary's argument: `p, q` or `nothing`.
pub fn premises_said(premises: &[Expr]) -> String {
    if premises.is_empty() {
        "nothing".into()
    } else {
        premises.iter().map(print::expr).collect::<Vec<_>>().join(", ")
    }
}
