//! A theory as it is written: its lines, each a declaration, a constraint
//! or a question, with any comment beside it. Both languages read into this
//! and are written from it.

use serde::{Deserialize, Serialize};

/// A file: its lines in order. A statement written over several lines is
/// one line here, at the line it started on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theory {
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub item: Option<Item>,
    /// The comment on the line, without its `#` or `%`.
    pub comment: Option<String>,
    /// Where the statement began, from 1; 0 for a line made, not read.
    #[serde(default)]
    pub at: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    Sort(SortDecl),
    Symbol(SymbolDecl),
    Constraint(Expr),
    Question(Question),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortDecl {
    pub name: String,
    pub kind: SortKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortKind {
    /// `a | b | c`: elements named, in no order.
    Named(Vec<String>),
    /// `a < b < c`: elements named, least first.
    Ordered(Vec<String>),
    /// `1..8`: the numbers from one to the other.
    Range(i64, i64),
    /// `size 5`: five elements, `name1` to `name5`.
    Size(u32),
    /// `size 3..6`: a size the solver chooses.
    Sizes(u32, u32),
}

/// A function, predicate or constant: `f: a, b -> c one-to-one`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolDecl {
    pub names: Vec<String>,
    pub args: Vec<String>,
    /// A sort's name, or `bool`.
    pub result: String,
    pub props: Props,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Props {
    pub one_to_one: bool,
    pub onto: bool,
    pub partial: bool,
    pub symmetric: bool,
}

/// A line beginning `?`: asked, not assumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Question {
    /// `? F`: what is this formula.
    Formula(Expr),
    /// `? A, B |= C`: does the argument hold.
    Entails(Vec<Expr>, Expr),
    /// `? A == B`: are these the same.
    Same(Expr, Expr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expr {
    /// A variable, a constant, a proposition or a sort's element: which is
    /// for typing to say.
    Name(String),
    Num(i64),
    Bool(bool),
    /// A declared function or predicate applied, or one built in: `abs`,
    /// `min`, `max`, `succ`, `pred`, `defined`.
    App(String, Vec<Expr>),
    /// `first(s)` or `last(s)`: a sort's least or greatest element.
    End { last: bool, sort: String },
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Quant(Quant, Vec<Binder>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Iff,
    Imp,
    Or,
    Xor,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quant {
    All,
    Some,
    Exactly(u32),
    AtLeast(u32),
    AtMost(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binder {
    pub name: String,
    pub sort: Option<String>,
}

/// The functions every theory has.
pub const BUILT_IN: [&str; 6] = ["abs", "min", "max", "succ", "pred", "defined"];

impl Op {
    /// How tightly it binds: higher first.
    pub fn level(self) -> u8 {
        match self {
            Op::Iff => 1,
            Op::Imp => 2,
            Op::Or | Op::Xor => 3,
            Op::And => 4,
            Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => 6,
            Op::Add | Op::Sub => 7,
            Op::Mul => 8,
        }
    }

    pub fn is_logical(self) -> bool {
        self.level() <= 4
    }

    pub fn text(self) -> &'static str {
        match self {
            Op::Iff => "<->",
            Op::Imp => "->",
            Op::Or => "or",
            Op::Xor => "xor",
            Op::And => "and",
            Op::Eq => "=",
            Op::Ne => "!=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
        }
    }
}

impl Expr {
    pub fn bin(op: Op, a: Expr, b: Expr) -> Expr {
        Expr::Bin(op, Box::new(a), Box::new(b))
    }

    pub fn negation(a: Expr) -> Expr {
        Expr::Not(Box::new(a))
    }

    pub fn name(s: &str) -> Expr {
        Expr::Name(s.to_string())
    }

    /// The names free in it: neither bound by a quantifier inside it nor
    /// applied as functions. Each once, first seen first.
    pub fn free(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.free_into(&mut Vec::new(), &mut out);
        out
    }

    fn free_into(&self, bound: &mut Vec<String>, out: &mut Vec<String>) {
        match self {
            Expr::Name(n) => {
                if !bound.contains(n) && !out.contains(n) {
                    out.push(n.clone());
                }
            }
            Expr::Num(_) | Expr::Bool(_) | Expr::End { .. } => {}
            Expr::App(_, args) => args.iter().for_each(|a| a.free_into(bound, out)),
            Expr::Not(a) | Expr::Neg(a) => a.free_into(bound, out),
            Expr::Bin(_, a, b) => {
                a.free_into(bound, out);
                b.free_into(bound, out);
            }
            Expr::Quant(_, binders, body) => {
                let n = bound.len();
                bound.extend(binders.iter().map(|b| b.name.clone()));
                body.free_into(bound, out);
                bound.truncate(n);
            }
        }
    }
}
