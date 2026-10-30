//! The calculator: what a formula of propositions is, whether an argument
//! holds, whether two formulas are the same, and a formula for a truth
//! table.
//!
//! - **Truth tables** for up to [`MOST_SHOWN`] propositions, row `r` being
//!   the propositions' bits with the first the most significant, as
//!   Circuit's tables are.
//! - **Properties** — tautology, contradiction, contingent, how many
//!   models — counted from the table up to [`MOST_COUNTED`] propositions;
//!   past that, a search ([`sat`]) answers whether there is a model, and
//!   what one is, without counting.
//! - **Forms:** negation normal form; CNF and DNF by distributing, while
//!   they stay under [`MOST_CLAUSES`] clauses; the full (canonical) CNF and
//!   DNF; the least sum of products and product of sums by Circuit's
//!   Quine–McCluskey, up to [`MOST_LEAST`] propositions; and the formula in
//!   NAND only and NOR only. Each is left out where it would be too long to
//!   read.
//! - Formulas with quantifiers, or values other than true and false, are the
//!   solver's, and refused here with that said.

use proton_circuit::analysis;
use serde::{Deserialize, Serialize};

use crate::ast::*;
use crate::print;
use crate::types::{Checked, Sig, Ty};

/// No truth table is shown for more propositions than this.
pub const MOST_SHOWN: usize = 16;
/// Models are counted, row by row, up to this many propositions.
pub const MOST_COUNTED: usize = 20;
/// CNF and DNF are given while they have no more clauses than this.
pub const MOST_CLAUSES: usize = 256;
/// The full forms are given while they have no more terms than this.
pub const MOST_FULL: usize = 64;
/// The least forms are found up to this many propositions, quick enough
/// to answer as a formula is typed: 18 ms at 10, 270 ms at 12, in a
/// release build (Circuit's Quine–McCluskey; the product of sums costs most).
pub const MOST_LEAST: usize = 10;
/// A least form, NAND or NOR form is given while it has no more terms.
pub const MOST_TERMS: usize = 128;
/// Negation normal form is given while it has no more parts than this.
pub const MOST_NNF: u64 = 4096;
/// Steps of the table — a column for each part of the formula — are shown
/// up to this many propositions and this many parts.
const MOST_STEP_ATOMS: usize = 10;
const MOST_STEPS: usize = 24;

/// A formula of propositions, each by its place in the list of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum P {
    Var(usize),
    Const(bool),
    Not(Box<P>),
    Bin(Op, Box<P>, Box<P>),
}

impl P {
    fn bin(op: Op, a: P, b: P) -> P {
        P::Bin(op, Box::new(a), Box::new(b))
    }

    fn not(a: P) -> P {
        P::Not(Box::new(a))
    }

    /// Its value in a row of `n` propositions.
    pub fn eval(&self, n: usize, row: u64) -> bool {
        match self {
            P::Var(i) => row >> (n - 1 - i) & 1 == 1,
            P::Const(b) => *b,
            P::Not(a) => !a.eval(n, row),
            P::Bin(op, a, b) => {
                let (x, y) = (a.eval(n, row), b.eval(n, row));
                match op {
                    Op::And => x && y,
                    Op::Or => x || y,
                    Op::Xor => x != y,
                    Op::Imp => !x || y,
                    _ => x == y,
                }
            }
        }
    }

    /// Its value under an assignment, one bool a proposition.
    pub fn holds(&self, values: &[bool]) -> bool {
        match self {
            P::Var(i) => values[*i],
            P::Const(b) => *b,
            P::Not(a) => !a.holds(values),
            P::Bin(op, a, b) => {
                let (x, y) = (a.holds(values), b.holds(values));
                match op {
                    Op::And => x && y,
                    Op::Or => x || y,
                    Op::Xor => x != y,
                    Op::Imp => !x || y,
                    _ => x == y,
                }
            }
        }
    }

    pub fn to_expr(&self, atoms: &[String]) -> Expr {
        match self {
            P::Var(i) => Expr::name(&atoms[*i]),
            P::Const(b) => Expr::Bool(*b),
            P::Not(a) => Expr::negation(a.to_expr(atoms)),
            P::Bin(op, a, b) => Expr::bin(*op, a.to_expr(atoms), b.to_expr(atoms)),
        }
    }
}

/// Formulas made propositional, over one list of propositions, first used
/// first.
pub fn propositional(sig: &Sig, formulas: &[&Expr]) -> Result<(Vec<String>, Vec<P>), String> {
    let mut atoms = Vec::new();
    let ps = formulas.iter().map(|e| to_p(sig, e, &mut atoms)).collect::<Result<Vec<_>, _>>()?;
    Ok((atoms, ps))
}

fn bool_named(sig: &Sig, n: &str) -> bool {
    sig.propositions.iter().any(|p| p == n) || sig.symbols.get(n).is_some_and(|s| s.args.is_empty() && s.result == Ty::Bool)
}

fn to_p(sig: &Sig, e: &Expr, atoms: &mut Vec<String>) -> Result<P, String> {
    Ok(match e {
        Expr::Bool(b) => P::Const(*b),
        Expr::Name(n) if bool_named(sig, n) => {
            let i = atoms.iter().position(|a| a == n).unwrap_or_else(|| {
                atoms.push(n.clone());
                atoms.len() - 1
            });
            P::Var(i)
        }
        Expr::Not(a) => P::not(to_p(sig, a, atoms)?),
        Expr::Bin(op, a, b) if op.is_logical() => P::bin(*op, to_p(sig, a, atoms)?, to_p(sig, b, atoms)?),
        // `=` and `!=` between formulas are `<->` and `xor`.
        Expr::Bin(op @ (Op::Eq | Op::Ne), a, b) => {
            let (x, y) = (to_p(sig, a, atoms), to_p(sig, b, atoms));
            match (x, y) {
                (Ok(x), Ok(y)) => P::bin(if *op == Op::Eq { Op::Iff } else { Op::Xor }, x, y),
                _ => return Err(not_propositional(e)),
            }
        }
        _ => return Err(not_propositional(e)),
    })
}

fn not_propositional(e: &Expr) -> String {
    format!("“{}” is more than propositions: the solver answers it", print::expr(e))
}

/* ── answers ──────────────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Tautology,
    Contradiction,
    Contingent,
}

/// A column of a truth table: its heading and a `0` or `1` a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub values: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub atoms: Vec<String>,
    /// The columns, the propositions' own left out: they are the row's bits.
    pub table: Option<Vec<Column>>,
    pub what: What,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum What {
    Formula(Box<FormulaAnswer>),
    Argument { valid: bool, countermodel: Option<Vec<bool>> },
    Same { same: bool, differ: Option<Vec<bool>> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormulaAnswer {
    pub status: Status,
    /// How many rows make it true, where counted.
    pub models: Option<u64>,
    /// A row making it true, and one making it false, where there are.
    pub model: Option<Vec<bool>>,
    pub falsifier: Option<Vec<bool>>,
    pub forms: Forms,
}

/// The formula in other forms, each in Proton's language; `None` where
/// one would be too long or there are too many propositions to find it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forms {
    pub nnf: Option<String>,
    pub cnf: Option<String>,
    pub dnf: Option<String>,
    pub full_cnf: Option<String>,
    pub full_dnf: Option<String>,
    pub least_sop: Option<String>,
    pub least_pos: Option<String>,
    /// Whether the least forms are known to be least.
    pub exact: bool,
    pub nand: Option<String>,
    pub nor: Option<String>,
    /// The least sum's terms and product's, as Circuit's Karnaugh maps take
    /// them: `(value, care)`, the first proposition the top bit.
    pub sop_terms: Vec<(u32, u32)>,
    pub pos_terms: Vec<(u32, u32)>,
}

/// A question asked of a checked theory's signature.
pub fn ask(c: &Checked, q: &Question) -> Result<Answer, String> {
    ask_up_to(c, q, MOST_LEAST)
}

/// The same, the least forms found for up to `least` propositions — as many
/// as Circuit minimises (16) when asked for, at the time that takes.
pub fn ask_up_to(c: &Checked, q: &Question, least: usize) -> Result<Answer, String> {
    match q {
        Question::Formula(e) => {
            let (atoms, ps) = propositional(&c.sig, &[e])?;
            formula(atoms, &ps[0], e, least)
        }
        Question::Entails(premises, goal) => {
            let all: Vec<&Expr> = premises.iter().chain(std::iter::once(goal)).collect();
            let (atoms, ps) = propositional(&c.sig, &all)?;
            let (goal_p, prem) = ps.split_last().unwrap();
            let ante = prem.iter().cloned().reduce(|a, b| P::bin(Op::And, a, b)).unwrap_or(P::Const(true));
            let counter = P::bin(Op::And, ante, P::not(goal_p.clone()));
            let found = model_of(&counter, atoms.len());
            let mut cols: Vec<(String, &P)> = premises.iter().zip(prem).map(|(e, p)| (print::expr(e), p)).collect();
            cols.push((print::expr(goal), goal_p));
            let table = columns(atoms.len(), &cols);
            Ok(Answer { atoms, table, what: What::Argument { valid: found.is_none(), countermodel: found } })
        }
        Question::Same(a, b) => {
            let (atoms, ps) = propositional(&c.sig, &[a, b])?;
            let differ = model_of(&P::bin(Op::Xor, ps[0].clone(), ps[1].clone()), atoms.len());
            let table = columns(atoms.len(), &[(print::expr(a), &ps[0]), (print::expr(b), &ps[1])]);
            Ok(Answer { atoms, table, what: What::Same { same: differ.is_none(), differ } })
        }
    }
}

/// A row making `p` true: from the table where it is small, else searched.
fn model_of(p: &P, n: usize) -> Option<Vec<bool>> {
    if n <= MOST_COUNTED {
        (0..1u64 << n).find(|r| p.eval(n, *r)).map(|r| bits(n, r))
    } else {
        sat::solve(p, n)
    }
}

fn bits(n: usize, row: u64) -> Vec<bool> {
    (0..n).map(|i| row >> (n - 1 - i) & 1 == 1).collect()
}

fn columns(n: usize, cols: &[(String, &P)]) -> Option<Vec<Column>> {
    (n <= MOST_SHOWN).then(|| cols.iter().map(|(name, p)| Column { name: name.clone(), values: (0..1u64 << n).map(|r| if p.eval(n, r) { '1' } else { '0' }).collect() }).collect())
}

fn formula(atoms: Vec<String>, p: &P, e: &Expr, least: usize) -> Result<Answer, String> {
    let n = atoms.len();
    let (status, models, model, falsifier) = if n <= MOST_COUNTED {
        let mut count = 0u64;
        let (mut yes, mut no) = (None, None);
        for r in 0..1u64 << n {
            if p.eval(n, r) {
                count += 1;
                yes.get_or_insert(r);
            } else {
                no.get_or_insert(r);
            }
        }
        let status = if no.is_none() {
            Status::Tautology
        } else if yes.is_none() {
            Status::Contradiction
        } else {
            Status::Contingent
        };
        (status, Some(count), yes.map(|r| bits(n, r)), no.map(|r| bits(n, r)))
    } else {
        let yes = sat::solve(p, n);
        let no = sat::solve(&P::not(p.clone()), n);
        let status = match (&yes, &no) {
            (_, None) => Status::Tautology,
            (None, _) => Status::Contradiction,
            _ => Status::Contingent,
        };
        (status, None, yes, no)
    };
    // The table: a column for each part of the formula when it is small.
    let table = if n <= MOST_STEP_ATOMS {
        let mut parts = Vec::new();
        steps(p, &mut parts);
        if parts.is_empty() || parts.len() > MOST_STEPS {
            parts = vec![p.clone()];
        }
        let named: Vec<(String, P)> = parts.into_iter().map(|q| (print::expr(&q.to_expr(&atoms)), q)).collect();
        let refs: Vec<(String, &P)> = named.iter().map(|(s, q)| (s.clone(), q)).collect();
        columns(n, &refs)
    } else {
        columns(n, &[(print::expr(e), p)])
    };
    let forms = forms(&atoms, p, least);
    Ok(Answer { atoms, table, what: What::Formula(Box::new(FormulaAnswer { status, models, model, falsifier, forms })) })
}

/// Each part of a formula that is not a proposition, inner first, each once.
fn steps(p: &P, out: &mut Vec<P>) {
    match p {
        P::Var(_) | P::Const(_) => return,
        P::Not(a) => steps(a, out),
        P::Bin(_, a, b) => {
            steps(a, out);
            steps(b, out);
        }
    }
    if !out.contains(p) {
        out.push(p.clone());
    }
}

/* ── forms ────────────────────────────────────────────────────────── */

/// A literal: a proposition, and whether it is negated.
type Lit = (usize, bool);
/// A conjunction or disjunction of literals, sorted, each once.
type Clause = Vec<Lit>;

pub fn forms(atoms: &[String], p: &P, most_least: usize) -> Forms {
    let n = atoms.len();
    let mut f = Forms::default();
    // `<->` and `xor` double what they hold in negation normal form: its
    // size is reckoned before it is made.
    if nnf_size(p) <= MOST_NNF {
        let nnf = nnf(p, false);
        f.nnf = Some(print::expr(&nnf.to_expr(atoms)));
        f.cnf = normal(&nnf, true).map(|c| print::expr(&clauses_expr(atoms, &c, true)));
        f.dnf = normal(&nnf, false).map(|c| print::expr(&clauses_expr(atoms, &c, false)));
    }
    if (1..=analysis::MOST_VARIABLES).contains(&n) {
        let column: Vec<u8> = (0..1u64 << n).map(|r| if p.eval(n, r) { b'1' } else { b'0' }).collect();
        let ones = column.iter().filter(|c| **c == b'1').count();
        if ones <= MOST_FULL {
            f.full_dnf = Some(print::expr(&full(atoms, &column, b'1')));
        }
        if column.len() - ones <= MOST_FULL {
            f.full_cnf = Some(print::expr(&full(atoms, &column, b'0')));
        }
        if n <= most_least {
            if let Ok(m) = analysis::minimise(n, &column) {
                least(&mut f, atoms, &m);
            }
        }
    }
    f
}

/// The least forms, and NAND and NOR from them, where they are short enough.
fn least(f: &mut Forms, atoms: &[String], m: &analysis::Minimal) {
    f.exact = m.exact;
    if m.sop.len() <= MOST_TERMS {
        f.least_sop = Some(print::expr(&terms_expr(atoms, &m.sop, false)));
        f.nand = Some(gates(atoms, &m.sop, false).text(atoms));
        f.sop_terms = m.sop.iter().map(|t| (t.value, t.care)).collect();
    }
    if m.pos.len() <= MOST_TERMS {
        f.least_pos = Some(print::expr(&terms_expr(atoms, &m.pos, true)));
        f.nor = Some(gates(atoms, &m.pos, true).text(atoms));
        f.pos_terms = m.pos.iter().map(|t| (t.value, t.care)).collect();
    }
}

/// How many parts a formula's negation normal form has, without making it.
fn nnf_size(p: &P) -> u64 {
    match p {
        P::Var(_) | P::Const(_) => 1,
        P::Not(a) => nnf_size(a),
        P::Bin(op, a, b) => {
            let (x, y) = (nnf_size(a), nnf_size(b));
            match op {
                Op::Iff | Op::Xor => x.saturating_add(y).saturating_mul(2).saturating_add(3),
                _ => x.saturating_add(y).saturating_add(1),
            }
        }
    }
}

/// Negations pushed in to the propositions, `->`, `<->` and `xor` written
/// with `and`, `or` and `not`; `neg` says the whole is negated.
pub fn nnf(p: &P, neg: bool) -> P {
    match p {
        P::Var(_) => {
            if neg {
                P::not(p.clone())
            } else {
                p.clone()
            }
        }
        P::Const(b) => P::Const(*b != neg),
        P::Not(a) => nnf(a, !neg),
        P::Bin(op, a, b) => {
            let (and, or) = if neg { (Op::Or, Op::And) } else { (Op::And, Op::Or) };
            match op {
                Op::And => P::bin(and, nnf(a, neg), nnf(b, neg)),
                Op::Or => P::bin(or, nnf(a, neg), nnf(b, neg)),
                // a -> b is not a or b.
                Op::Imp => P::bin(or, nnf(a, !neg), nnf(b, neg)),
                // a <-> b is (not a or b) and (a or not b); negated, xor.
                Op::Iff | Op::Xor => {
                    let iff = (*op == Op::Iff) != neg;
                    if iff {
                        P::bin(Op::And, P::bin(Op::Or, nnf(a, true), nnf(b, false)), P::bin(Op::Or, nnf(a, false), nnf(b, true)))
                    } else {
                        P::bin(Op::Or, P::bin(Op::And, nnf(a, false), nnf(b, true)), P::bin(Op::And, nnf(a, true), nnf(b, false)))
                    }
                }
                _ => unreachable!("only connectives are propositional"),
            }
        }
    }
}

/// A formula in negation normal form as clauses (`cnf`) or terms, by
/// distributing; none when it grows past [`MOST_CLAUSES`]. A clause that
/// holds anyway is dropped, and so is one another clause already says.
fn normal(p: &P, cnf: bool) -> Option<Vec<Clause>> {
    // In CNF `and` joins clauses and `or` is distributed; in DNF the other way.
    let joins = if cnf { Op::And } else { Op::Or };
    let out = match p {
        P::Var(i) => vec![vec![(*i, false)]],
        P::Not(a) => match **a {
            P::Var(i) => vec![vec![(i, true)]],
            _ => unreachable!("negation normal form"),
        },
        // In CNF true is no clauses and false one empty clause; in DNF the reverse.
        P::Const(b) => {
            if *b == cnf {
                vec![]
            } else {
                vec![vec![]]
            }
        }
        P::Bin(op, a, b) => {
            let (x, y) = (normal(a, cnf)?, normal(b, cnf)?);
            if *op == joins {
                [x, y].concat()
            } else {
                if x.len() * y.len() > MOST_CLAUSES * 4 {
                    return None;
                }
                let mut out = Vec::new();
                for c in &x {
                    for d in &y {
                        out.push([c.clone(), d.clone()].concat());
                    }
                }
                out
            }
        }
    };
    let out = tidy(out);
    (out.len() <= MOST_CLAUSES).then_some(out)
}

fn tidy(cs: Vec<Clause>) -> Vec<Clause> {
    let mut cs: Vec<Clause> = cs
        .into_iter()
        .map(|mut c| {
            c.sort();
            c.dedup();
            c
        })
        // A proposition with its negation: always true in a clause, always
        // false in a term; dropped either way.
        .filter(|c| !c.windows(2).any(|w| w[0].0 == w[1].0))
        .collect();
    cs.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
    cs.dedup();
    let mut kept: Vec<Clause> = Vec::new();
    for c in cs {
        if !kept.iter().any(|k| k.iter().all(|l| c.contains(l))) {
            kept.push(c);
        }
    }
    kept
}

fn lit(atoms: &[String], (i, neg): Lit) -> Expr {
    if neg {
        Expr::negation(Expr::name(&atoms[i]))
    } else {
        Expr::name(&atoms[i])
    }
}

/// Clauses (`cnf`) or terms as a formula.
fn clauses_expr(atoms: &[String], cs: &[Clause], cnf: bool) -> Expr {
    let (inner, outer) = if cnf { (Op::Or, Op::And) } else { (Op::And, Op::Or) };
    let parts: Vec<Expr> = cs.iter().map(|c| c.iter().map(|l| lit(atoms, *l)).reduce(|a, b| Expr::bin(inner, a, b)).unwrap_or(Expr::Bool(!cnf))).collect();
    parts.into_iter().reduce(|a, b| Expr::bin(outer, a, b)).unwrap_or(Expr::Bool(cnf))
}

/// The full DNF (each row that is 1) or CNF (each row that is 0).
fn full(atoms: &[String], column: &[u8], want: u8) -> Expr {
    let n = atoms.len();
    let cnf = want == b'0';
    let cs: Vec<Clause> = column
        .iter()
        .enumerate()
        .filter(|(_, v)| **v == want)
        // A row's term says each bit; its clause says each bit's opposite.
        .map(|(r, _)| bits(n, r as u64).into_iter().enumerate().map(|(i, b)| (i, b == cnf)).collect())
        .collect();
    clauses_expr(atoms, &cs, cnf)
}

/// Quine–McCluskey's terms as a formula: products summed, or, for the
/// zeros (`pos`), each term's clause of opposite literals.
fn terms_expr(atoms: &[String], terms: &[analysis::Term], pos: bool) -> Expr {
    let n = atoms.len();
    let cs: Vec<Clause> = terms.iter().map(|t| (0..n).filter(|i| t.care >> (n - 1 - i) & 1 == 1).map(|i| (i, (t.value >> (n - 1 - i) & 1 == 1) == pos)).collect()).collect();
    clauses_expr(atoms, &cs, pos)
}

/// A formula of one kind of gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Var(usize),
    Const(bool),
    Nand(Vec<Gate>),
    Nor(Vec<Gate>),
}

impl Gate {
    pub fn eval(&self, values: &[bool]) -> bool {
        match self {
            Gate::Var(i) => values[*i],
            Gate::Const(b) => *b,
            Gate::Nand(g) => !g.iter().all(|x| x.eval(values)),
            Gate::Nor(g) => !g.iter().any(|x| x.eval(values)),
        }
    }

    pub fn text(&self, atoms: &[String]) -> String {
        match self {
            Gate::Var(i) => atoms[*i].clone(),
            Gate::Const(b) => b.to_string(),
            Gate::Nand(g) | Gate::Nor(g) => {
                let word = if matches!(self, Gate::Nand(_)) { "nand" } else { "nor" };
                format!("{word}({})", g.iter().map(|x| x.text(atoms)).collect::<Vec<_>>().join(", "))
            }
        }
    }
}

/// The least sum of products in NAND only (`nor` false), or the least
/// product of sums in NOR only: two levels, a literal's negation its gate
/// with itself.
pub fn gates(atoms: &[String], terms: &[analysis::Term], nor: bool) -> Gate {
    let n = atoms.len();
    let gate = |g: Vec<Gate>| if nor { Gate::Nor(g) } else { Gate::Nand(g) };
    if terms.is_empty() {
        return Gate::Const(nor);
    }
    // In NAND: a term's literals, each its own value; in NOR the clause of
    // opposites, so a 1 in the term is the proposition negated.
    let literal = |i: usize, bit: bool| {
        let negated = bit == nor;
        if negated {
            gate(vec![Gate::Var(i), Gate::Var(i)])
        } else {
            Gate::Var(i)
        }
    };
    let firsts: Vec<Gate> = terms
        .iter()
        .map(|t| {
            let lits: Vec<(usize, bool)> = (0..n).filter(|i| t.care >> (n - 1 - i) & 1 == 1).map(|i| (i, t.value >> (n - 1 - i) & 1 == 1)).collect();
            match lits.as_slice() {
                // A term of nothing: in a sum it is true, so its gate is false.
                [] => Gate::Const(nor),
                // One literal: its gate's output is its negation, so a negated
                // proposition is the proposition, and the rest its NOT.
                [(i, bit)] => {
                    if *bit == nor {
                        Gate::Var(*i)
                    } else {
                        gate(vec![Gate::Var(*i), Gate::Var(*i)])
                    }
                }
                _ => gate(lits.iter().map(|(i, bit)| literal(*i, *bit)).collect()),
            }
        })
        .collect();
    gate(firsts)
}

/* ── a truth table to a formula ───────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FromTable {
    pub atoms: Vec<String>,
    pub forms: Forms,
}

/// A column of `0`, `1` and `x` (or `-`, don't care), one a row, read back
/// as formulas over the names given, or `p`, `q`, `r`… when none are.
pub fn from_column(names: &[String], column: &str) -> Result<FromTable, String> {
    let column: Vec<u8> = column
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .map(|c| match c {
            '0' | 'F' | 'f' => Ok(b'0'),
            '1' | 'T' | 't' => Ok(b'1'),
            'x' | 'X' | '-' | '*' => Ok(b'x'),
            other => Err(format!("“{other}” is not 0, 1 or x")),
        })
        .collect::<Result<_, _>>()?;
    let n = column.len().trailing_zeros() as usize;
    if column.len() < 2 || column.len() != 1 << n {
        return Err(format!("a column has 2, 4, 8… rows, a power of two; this has {}", column.len()));
    }
    if n > analysis::MOST_VARIABLES {
        return Err(format!("no more than {} propositions", analysis::MOST_VARIABLES));
    }
    let atoms: Vec<String> = if names.is_empty() {
        let default = ["p", "q", "r", "s", "t", "u", "v", "w"];
        (0..n).map(|i| default.get(i).map(|s| s.to_string()).unwrap_or_else(|| format!("p{}", i + 1))).collect()
    } else if names.len() == n {
        names.to_vec()
    } else {
        return Err(format!("{} rows are {n} propositions, and {} are named", column.len(), names.len()));
    };
    let mut forms = Forms::default();
    if n <= MOST_LEAST {
        least(&mut forms, &atoms, &analysis::minimise(n, &column)?);
    }
    // Don't cares are taken as 0 for the full DNF, and 1 for the full CNF.
    let as_ones: Vec<u8> = column.iter().map(|c| if *c == b'x' { b'1' } else { *c }).collect();
    let as_zeros: Vec<u8> = column.iter().map(|c| if *c == b'x' { b'0' } else { *c }).collect();
    if as_zeros.iter().filter(|c| **c == b'1').count() <= MOST_FULL {
        forms.full_dnf = Some(print::expr(&full(&atoms, &as_zeros, b'1')));
    }
    if as_ones.iter().filter(|c| **c == b'0').count() <= MOST_FULL {
        forms.full_cnf = Some(print::expr(&full(&atoms, &as_ones, b'0')));
    }
    Ok(FromTable { atoms, forms })
}

/* ── the search ───────────────────────────────────────────────────── */

pub mod sat {
    //! Whether a formula has a model, past where tables go: the formula
    //! made clauses by Tseitin's encoding (a new proposition for each part,
    //! so the clauses grow with the formula, not exponentially), then the
    //! Davis–Putnam–Logemann–Loveland search, unit clauses first, branching
    //! on a proposition of the shortest clause left.

    use super::P;
    use crate::ast::Op;

    /// A literal: a variable from 1, negative when negated.
    type L = i32;

    struct Enc {
        clauses: Vec<Vec<L>>,
        next: i32,
    }

    impl Enc {
        fn fresh(&mut self) -> L {
            self.next += 1;
            self.next
        }

        /// A literal standing for `p`, its meaning said in clauses.
        fn lit(&mut self, p: &P) -> L {
            match p {
                P::Var(i) => *i as L + 1,
                P::Const(b) => {
                    let v = self.fresh();
                    self.clauses.push(vec![if *b { v } else { -v }]);
                    v
                }
                P::Not(a) => -self.lit(a),
                P::Bin(op, a, b) => {
                    let (x, y) = (self.lit(a), self.lit(b));
                    let v = self.fresh();
                    let cs: Vec<Vec<L>> = match op {
                        Op::And => vec![vec![-v, x], vec![-v, y], vec![v, -x, -y]],
                        Op::Or => vec![vec![-v, x, y], vec![v, -x], vec![v, -y]],
                        Op::Imp => vec![vec![-v, -x, y], vec![v, x], vec![v, -y]],
                        Op::Iff => vec![vec![-v, -x, y], vec![-v, x, -y], vec![v, x, y], vec![v, -x, -y]],
                        _ => vec![vec![-v, x, y], vec![-v, -x, -y], vec![v, -x, y], vec![v, x, -y]],
                    };
                    self.clauses.extend(cs);
                    v
                }
            }
        }
    }

    /// A model of `p` over its `n` propositions, if it has one.
    pub fn solve(p: &P, n: usize) -> Option<Vec<bool>> {
        let mut e = Enc { clauses: Vec::new(), next: n as i32 };
        let top = e.lit(p);
        e.clauses.push(vec![top]);
        let mut values = vec![0i8; e.next as usize + 1];
        if dpll(&e.clauses, &mut values) {
            Some((1..=n).map(|v| values[v] > 0).collect())
        } else {
            None
        }
    }

    fn value(values: &[i8], l: L) -> i8 {
        let v = values[l.unsigned_abs() as usize];
        if l > 0 {
            v
        } else {
            -v
        }
    }

    fn dpll(clauses: &[Vec<L>], values: &mut Vec<i8>) -> bool {
        let saved = values.clone();
        // Unit clauses, until there are none.
        loop {
            let mut changed = false;
            let mut shortest: Option<(usize, L)> = None;
            for c in clauses {
                let mut open = Vec::new();
                let mut satisfied = false;
                for &l in c {
                    match value(values, l) {
                        1 => {
                            satisfied = true;
                            break;
                        }
                        0 => open.push(l),
                        _ => {}
                    }
                }
                if satisfied {
                    continue;
                }
                match open.len() {
                    0 => {
                        *values = saved;
                        return false;
                    }
                    1 => {
                        let l = open[0];
                        values[l.unsigned_abs() as usize] = if l > 0 { 1 } else { -1 };
                        changed = true;
                    }
                    k => {
                        if shortest.is_none_or(|(s, _)| k < s) {
                            shortest = Some((k, open[0]));
                        }
                    }
                }
            }
            if changed {
                continue;
            }
            let Some((_, l)) = shortest else { return true };
            for guess in [l, -l] {
                let mut tried = values.clone();
                tried[guess.unsigned_abs() as usize] = if guess > 0 { 1 } else { -1 };
                if dpll(clauses, &mut tried) {
                    *values = tried;
                    return true;
                }
            }
            *values = saved;
            return false;
        }
    }
}
