//! The engine: a workbook with what each formula comes to, kept up to date.
//!
//! Each formula's precedents — the cells and ranges it reads — are kept, with
//! an index from each cell to the formulas that read it. A cell set marks
//! what depends on it, through however many others; those are worked out in
//! the order their precedents allow, and nothing else is. Formulas that read
//! themselves, however far round, are found by that order having nowhere to
//! start, and show #CYCLE!. Formulas whose reach is only known when worked
//! out — INDIRECT, OFFSET, `INDEX(…):INDEX(…)` — and those that change by
//! themselves — NOW, RAND — are worked out after every change.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::formula::{self, BinOp, Expr, ParseError};
use crate::model::{Cell, Workbook};
use crate::refs::{CellRef, Range, A1, MAX_COL, MAX_ROW};
use crate::value::{parse_number, ErrorKind, Value};

/// A cell of the workbook: its sheet's place, and where on it.
pub type Key = (usize, CellRef);

/// What a formula reads.
#[derive(Debug, Clone, PartialEq)]
enum Dep {
    Cell(Key),
    Range(usize, Range),
}

/// A value being worked out: one value, a rectangle of cells still to be
/// read, or an array.
#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    V(Value),
    Area(usize, Range),
    Arr(Vec<Vec<Value>>),
}

impl Ev {
    pub fn err(e: ErrorKind) -> Ev {
        Ev::V(Value::Error(e))
    }
}

#[derive(Debug, Default)]
struct Deps {
    reads: HashMap<Key, Vec<Dep>>,
    by_cell: HashMap<Key, HashSet<Key>>,
    by_range: Vec<(usize, Range, Key)>,
    volatile: HashSet<Key>,
}

impl Deps {
    fn remove(&mut self, f: Key) {
        if let Some(old) = self.reads.remove(&f) {
            for d in old {
                match d {
                    Dep::Cell(k) => {
                        if let Some(set) = self.by_cell.get_mut(&k) {
                            set.remove(&f);
                            if set.is_empty() {
                                self.by_cell.remove(&k);
                            }
                        }
                    }
                    Dep::Range(..) => {}
                }
            }
            self.by_range.retain(|(_, _, k)| *k != f);
        }
        self.volatile.remove(&f);
    }

    fn add(&mut self, f: Key, reads: Vec<Dep>, volatile: bool) {
        for d in &reads {
            match d {
                Dep::Cell(k) => {
                    self.by_cell.entry(*k).or_default().insert(f);
                }
                Dep::Range(s, r) => self.by_range.push((*s, *r, f)),
            }
        }
        self.reads.insert(f, reads);
        if volatile {
            self.volatile.insert(f);
        }
    }

    /// The formulas that read `k` directly.
    fn readers(&self, k: Key) -> Vec<Key> {
        let mut out: Vec<Key> = self.by_cell.get(&k).map(|s| s.iter().copied().collect()).unwrap_or_default();
        out.extend(self.by_range.iter().filter(|(s, r, _)| *s == k.0 && r.contains(k.1)).map(|(_, _, f)| *f));
        out
    }
}

/// A workbook, and what its formulas come to.
pub struct Book {
    wb: Workbook,
    parsed: HashMap<Key, Result<Expr, ParseError>>,
    values: HashMap<Key, Value>,
    deps: Deps,
    /// Now, as a serial: what TODAY and NOW say until it is set again.
    now: f64,
    seed: u64,
    /// Formulas worked out since the book was made: what the tests read to
    /// see that a change reached only what depends on it.
    computed: u64,
}

/// What typing `input` into a cell puts there, when it is not a formula.
pub fn literal(input: &str) -> Value {
    if input.is_empty() {
        return Value::Empty;
    }
    if let Some(text) = input.strip_prefix('\'') {
        return Value::text(text);
    }
    match input.to_ascii_uppercase().as_str() {
        "TRUE" => return Value::Bool(true),
        "FALSE" => return Value::Bool(false),
        _ => {}
    }
    if let Some(e) = ErrorKind::parse(input) {
        return Value::Error(e);
    }
    if let Some(n) = parse_number(input) {
        return Value::Number(n);
    }
    if let Some(n) = crate::dates::parse_date(input).or_else(|| crate::dates::parse_time(input)) {
        return Value::Number(n);
    }
    Value::text(input)
}

impl Book {
    /// A workbook opened: every formula read and worked out.
    pub fn new(wb: Workbook) -> Book {
        let mut book = Book { wb, parsed: HashMap::new(), values: HashMap::new(), deps: Deps::default(), now: crate::dates::local_now(), seed: 0x9e37_79b9_7f4a_7c15, computed: 0 };
        book.rebuild();
        book
    }

    pub fn workbook(&self) -> &Workbook {
        &self.wb
    }

    /// The workbook to change in ways that are not a cell's input — styles,
    /// widths, sheets' names; `rebuild` after a change that moves cells or
    /// sheets.
    pub fn workbook_mut(&mut self) -> &mut Workbook {
        &mut self.wb
    }

    /// What TODAY and NOW say: a serial, set before working out again.
    pub fn set_now(&mut self, serial: f64) {
        self.now = serial;
    }

    pub fn now(&self) -> f64 {
        self.now
    }

    /// Everything read and worked out again, as on opening.
    pub fn rebuild(&mut self) {
        self.parsed.clear();
        self.values.clear();
        self.deps = Deps::default();
        let mut formulas = Vec::new();
        for (s, sheet) in self.wb.sheets.iter().enumerate() {
            for (at, cell) in &sheet.cells {
                if cell.input.starts_with('=') && cell.input.len() > 1 {
                    formulas.push((s, *at, cell.input.clone()));
                } else {
                    let v = literal(&cell.input);
                    if !v.is_empty() {
                        self.values.insert((s, *at), v);
                    }
                }
            }
        }
        let keys: Vec<Key> = formulas.iter().map(|(s, at, _)| (*s, *at)).collect();
        for (s, at, input) in formulas {
            self.index_formula((s, at), &input);
        }
        self.recalc(keys);
    }

    /// What the cell comes to.
    pub fn value(&self, sheet: usize, at: CellRef) -> Value {
        self.values.get(&(sheet, at)).cloned().unwrap_or(Value::Empty)
    }

    /// What was typed into it.
    pub fn input(&self, sheet: usize, at: CellRef) -> &str {
        self.wb.sheets.get(sheet).and_then(|s| s.cells.get(&at)).map_or("", |c| c.input.as_str())
    }

    /// Why the cell's formula could not be read, if it could not.
    pub fn problem(&self, sheet: usize, at: CellRef) -> Option<&ParseError> {
        self.parsed.get(&(sheet, at)).and_then(|p| p.as_ref().err())
    }

    /// Several cells typed into at once, as one change; the cells whose
    /// values changed because of it, the typed ones among them.
    pub fn set_many(&mut self, edits: &[(usize, CellRef, String)]) -> Vec<Key> {
        let mut changed = Vec::new();
        for (sheet, at, input) in edits {
            let key = (*sheet, *at);
            let Some(s) = self.wb.sheets.get_mut(*sheet) else { continue };
            match s.cells.get_mut(at) {
                Some(cell) if input.is_empty() && cell.style == 0 => {
                    s.cells.remove(at);
                }
                Some(cell) => cell.input = input.clone(),
                None if input.is_empty() => {}
                None => {
                    s.cells.insert(*at, Cell { input: input.clone(), style: 0 });
                }
            }
            self.deps.remove(key);
            self.parsed.remove(&key);
            if input.starts_with('=') && input.len() > 1 {
                self.index_formula(key, input);
            } else {
                let v = literal(input);
                if v.is_empty() {
                    self.values.remove(&key);
                } else {
                    self.values.insert(key, v);
                }
            }
            changed.push(key);
        }
        self.recalc(changed)
    }

    /// One cell typed into.
    pub fn set(&mut self, sheet: usize, at: CellRef, input: &str) -> Vec<Key> {
        self.set_many(&[(sheet, at, input.to_string())])
    }

    fn index_formula(&mut self, key: Key, input: &str) {
        let parsed = formula::parse(&input[1..]);
        if let Ok(e) = &parsed {
            let mut reads = Vec::new();
            let mut volatile = false;
            self.collect(key.0, e, &mut reads, &mut volatile, 0);
            self.deps.add(key, reads, volatile);
        }
        self.parsed.insert(key, parsed);
    }

    /// What `e` reads, found without working it out.
    fn collect(&self, sheet: usize, e: &Expr, out: &mut Vec<Dep>, volatile: &mut bool, depth: u32) {
        match e {
            Expr::Ref { sheet: s, at } => {
                if let (Some(sh), Some(r)) = (self.sheet_of(sheet, s.as_deref()), resolve(at, at)) {
                    if r.start == r.end {
                        out.push(Dep::Cell((sh, r.start)));
                    } else {
                        out.push(Dep::Range(sh, r));
                    }
                }
            }
            Expr::Range { sheet: s, start, end } => {
                if let (Some(sh), Some(r)) = (self.sheet_of(sheet, s.as_deref()), resolve(start, end)) {
                    out.push(Dep::Range(sh, r));
                }
            }
            Expr::Name(n) => {
                if depth < 8 {
                    if let Some(e) = self.name(n) {
                        self.collect(sheet, &e, out, volatile, depth + 1);
                    }
                }
            }
            Expr::Neg(a) | Expr::Plus(a) | Expr::Percent(a) => self.collect(sheet, a, out, volatile, depth),
            Expr::Binary(_, a, b) => {
                self.collect(sheet, a, out, volatile, depth);
                self.collect(sheet, b, out, volatile, depth);
            }
            Expr::RangeOp(a, b) => {
                *volatile = true;
                self.collect(sheet, a, out, volatile, depth);
                self.collect(sheet, b, out, volatile, depth);
            }
            Expr::Call(name, args) => {
                if crate::functions::volatile(name) {
                    *volatile = true;
                }
                for a in args {
                    self.collect(sheet, a, out, volatile, depth);
                }
            }
            _ => {}
        }
    }

    /// A defined name's reference, read.
    pub(crate) fn name(&self, n: &str) -> Option<Expr> {
        let (_, def) = self.wb.names.iter().find(|(k, _)| k.eq_ignore_ascii_case(n))?;
        formula::parse(def.strip_prefix('=').unwrap_or(def)).ok()
    }

    /// The sheet a reference names, or the formula's own.
    pub(crate) fn sheet_of(&self, own: usize, named: Option<&str>) -> Option<usize> {
        match named {
            None => Some(own),
            Some(n) => self.wb.sheet_index(n),
        }
    }

    /// The formulas that depend on `changed`, worked out in an order that
    /// puts each after what it reads; the cells whose values moved.
    fn recalc(&mut self, changed: Vec<Key>) -> Vec<Key> {
        // Everything downstream of the change, and what changes by itself.
        let mut dirty: HashSet<Key> = HashSet::new();
        let mut queue: VecDeque<Key> = VecDeque::new();
        for k in &changed {
            if self.parsed.contains_key(k) {
                dirty.insert(*k);
            }
            queue.push_back(*k);
        }
        for v in self.deps.volatile.iter() {
            if dirty.insert(*v) {
                queue.push_back(*v);
            }
        }
        while let Some(k) = queue.pop_front() {
            for f in self.deps.readers(k) {
                if dirty.insert(f) {
                    queue.push_back(f);
                }
            }
        }
        // Kahn's order over the dirty formulas: each waits for the dirty
        // cells it reads.
        let mut waits: HashMap<Key, usize> = HashMap::new();
        let mut unblocks: HashMap<Key, Vec<Key>> = HashMap::new();
        let dirty_list: Vec<Key> = dirty.iter().copied().collect();
        // The dirty cells by sheet, in order, so a range finds the ones
        // inside it without looking at every one.
        let mut by_sheet: HashMap<usize, BTreeSet<CellRef>> = HashMap::new();
        for (s, at) in &dirty_list {
            by_sheet.entry(*s).or_default().insert(*at);
        }
        for f in &dirty_list {
            let mut n = 0;
            if let Some(reads) = self.deps.reads.get(f) {
                for d in reads {
                    match d {
                        Dep::Cell(k) if dirty.contains(k) => {
                            n += 1;
                            unblocks.entry(*k).or_default().push(*f);
                        }
                        Dep::Range(s, r) => {
                            let Some(cells) = by_sheet.get(s) else { continue };
                            let band = CellRef::new(r.start.row, 0)..=CellRef::new(r.end.row, MAX_COL);
                            for at in cells.range(band).filter(|at| r.contains(**at)) {
                                n += 1;
                                unblocks.entry((*s, *at)).or_default().push(*f);
                            }
                        }
                        _ => {}
                    }
                }
            }
            waits.insert(*f, n);
        }
        let mut ready: VecDeque<Key> = waits.iter().filter(|(_, n)| **n == 0).map(|(k, _)| *k).collect();
        let mut moved: Vec<Key> = changed.iter().filter(|k| !self.parsed.contains_key(k)).copied().collect();
        let mut done = 0;
        while let Some(f) = ready.pop_front() {
            done += 1;
            let v = self.compute(f);
            let before = self.values.get(&f);
            if before != Some(&v) && !(before.is_none() && v.is_empty()) {
                moved.push(f);
            }
            if v.is_empty() {
                self.values.remove(&f);
            } else {
                self.values.insert(f, v);
            }
            if let Some(next) = unblocks.get(&f) {
                for n in next {
                    if let Some(w) = waits.get_mut(n) {
                        *w -= 1;
                        if *w == 0 {
                            ready.push_back(*n);
                        }
                    }
                }
            }
        }
        // What never became ready reads itself, somewhere round.
        if done < dirty_list.len() {
            for (f, n) in &waits {
                if *n > 0 {
                    if self.values.get(f) != Some(&Value::Error(ErrorKind::Cycle)) {
                        moved.push(*f);
                    }
                    self.values.insert(*f, Value::Error(ErrorKind::Cycle));
                }
            }
        }
        moved.sort_unstable();
        moved.dedup();
        moved
    }

    /// How many formulas have been worked out since the book was made.
    pub fn computed(&self) -> u64 {
        self.computed
    }

    fn compute(&mut self, key: Key) -> Value {
        self.computed += 1;
        let Some(Ok(expr)) = self.parsed.get(&key) else {
            return Value::Error(ErrorKind::Name);
        };
        let expr = expr.clone();
        self.seed = self.seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let cx = Ctx { book: self, sheet: key.0, at: key.1, seed: std::cell::Cell::new(self.seed) };
        let ev = cx.eval(&expr);
        // A whole result in a cell: one value, or the first of many; a
        // formula that comes to nothing shows 0, as Excel's do.
        match cx.scalar(ev) {
            Value::Empty => Value::Number(0.0),
            v => v,
        }
    }
}

/// A reference's two ends as a rectangle: whole columns run every row,
/// whole rows every column.
pub(crate) fn resolve(a: &A1, b: &A1) -> Option<Range> {
    let (c0, c1) = match (a.col, b.col) {
        (Some((x, _)), Some((y, _))) => (x, y),
        (None, None) => (0, MAX_COL),
        _ => return None,
    };
    let (r0, r1) = match (a.row, b.row) {
        (Some((x, _)), Some((y, _))) => (x, y),
        (None, None) => (0, MAX_ROW),
        _ => return None,
    };
    Some(Range::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
}

/// Working out one formula: the book to read, and where the formula is.
pub struct Ctx<'a> {
    pub book: &'a Book,
    pub sheet: usize,
    pub at: CellRef,
    seed: std::cell::Cell<u64>,
}

impl Ctx<'_> {
    pub fn get(&self, sheet: usize, at: CellRef) -> Value {
        self.book.value(sheet, at)
    }

    /// Now, as a serial.
    pub fn now(&self) -> f64 {
        self.book.now
    }

    /// A number in [0, 1), for RAND.
    pub fn random(&self) -> f64 {
        let mut x = self.seed.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Each cell with anything in it inside `r`, row by row.
    pub fn each(&self, sheet: usize, r: Range, mut f: impl FnMut(CellRef, Value)) {
        let Some(s) = self.book.wb.sheets.get(sheet) else { return };
        let from = CellRef::new(r.start.row, 0);
        let to = CellRef::new(r.end.row, MAX_COL);
        for (at, _) in s.cells.range(from..=to) {
            if r.contains(*at) {
                f(*at, self.get(sheet, *at));
            }
        }
    }

    /// `r` as rows of values, cut to where the sheet has anything so a
    /// whole column is not a million rows.
    pub fn grid(&self, sheet: usize, r: Range) -> Vec<Vec<Value>> {
        let used = self.book.wb.sheets.get(sheet).and_then(|s| s.used());
        let end_row = used.map_or(r.start.row, |u| u.end.row.min(r.end.row).max(r.start.row));
        let end_col = used.map_or(r.start.col, |u| u.end.col.min(r.end.col).max(r.start.col));
        let r = Range::new(r.start, CellRef::new(end_row, end_col));
        (r.start.row..=r.end.row)
            .map(|row| (r.start.col..=r.end.col).map(|col| self.get(sheet, CellRef::new(row, col))).collect())
            .collect()
    }

    /// Any value as rows of values.
    pub fn rows(&self, ev: Ev) -> Vec<Vec<Value>> {
        match ev {
            Ev::V(v) => vec![vec![v]],
            Ev::Area(s, r) => self.grid(s, r),
            Ev::Arr(a) => a,
        }
    }

    /// One value from what may be many: a single cell's, or the cell of a
    /// column or row in line with the formula (Excel's implicit
    /// intersection), or the first of an array.
    pub fn scalar(&self, ev: Ev) -> Value {
        match ev {
            Ev::V(v) => v,
            Ev::Area(s, r) => {
                if r.start == r.end {
                    self.get(s, r.start)
                } else if r.cols() == 1 && (r.start.row..=r.end.row).contains(&self.at.row) && s == self.sheet {
                    self.get(s, CellRef::new(self.at.row, r.start.col))
                } else if r.rows() == 1 && (r.start.col..=r.end.col).contains(&self.at.col) && s == self.sheet {
                    self.get(s, CellRef::new(r.start.row, self.at.col))
                } else {
                    Value::Error(ErrorKind::Value)
                }
            }
            Ev::Arr(a) => a.first().and_then(|r| r.first()).cloned().unwrap_or(Value::Empty),
        }
    }

    /// An expression's value as one value.
    pub fn value(&self, e: &Expr) -> Value {
        let ev = self.eval(e);
        self.scalar(ev)
    }

    pub fn eval(&self, e: &Expr) -> Ev {
        match e {
            Expr::Number(n) => Ev::V(Value::Number(*n)),
            Expr::Text(s) => Ev::V(Value::text(s.clone())),
            Expr::Bool(b) => Ev::V(Value::Bool(*b)),
            Expr::Error(k) => Ev::err(*k),
            Expr::Missing => Ev::V(Value::Empty),
            Expr::Ref { sheet, at } => match (self.book.sheet_of(self.sheet, sheet.as_deref()), resolve(at, at)) {
                (Some(s), Some(r)) => Ev::Area(s, r),
                _ => Ev::err(ErrorKind::Ref),
            },
            Expr::Range { sheet, start, end } => match (self.book.sheet_of(self.sheet, sheet.as_deref()), resolve(start, end)) {
                (Some(s), Some(r)) => Ev::Area(s, r),
                _ => Ev::err(ErrorKind::Ref),
            },
            Expr::Name(n) => match self.book.name(n) {
                Some(e) => self.eval(&e),
                None => Ev::err(ErrorKind::Name),
            },
            Expr::RangeOp(a, b) => match (self.eval(a), self.eval(b)) {
                (Ev::Area(s1, r1), Ev::Area(s2, r2)) if s1 == s2 => {
                    let start = CellRef::new(r1.start.row.min(r2.start.row), r1.start.col.min(r2.start.col));
                    let end = CellRef::new(r1.end.row.max(r2.end.row), r1.end.col.max(r2.end.col));
                    Ev::Area(s1, Range::new(start, end))
                }
                (Ev::V(Value::Error(e)), _) | (_, Ev::V(Value::Error(e))) => Ev::err(e),
                _ => Ev::err(ErrorKind::Value),
            },
            Expr::Neg(a) => self.unary(a, |n| -n),
            Expr::Plus(a) => self.eval(a),
            Expr::Percent(a) => self.unary(a, |n| n / 100.0),
            Expr::Binary(op, a, b) => {
                let (x, y) = (self.eval(a), self.eval(b));
                self.binary(*op, x, y)
            }
            Expr::Call(name, args) => crate::functions::call(self, name, args),
            Expr::Array(rows) => Ev::Arr(rows.iter().map(|r| r.iter().map(|e| self.value(e)).collect()).collect()),
        }
    }

    fn unary(&self, a: &Expr, f: impl Fn(f64) -> f64) -> Ev {
        match self.eval(a) {
            Ev::V(v) => Ev::V(num_result(v.to_number().map(&f))),
            many => Ev::Arr(
                self.rows(many).into_iter().map(|r| r.into_iter().map(|v| num_result(v.to_number().map(&f))).collect()).collect(),
            ),
        }
    }

    /// An operator, element by element when either side is many values — as
    /// `SUMPRODUCT((A1:A9>0)*B1:B9)` needs.
    pub fn binary(&self, op: BinOp, x: Ev, y: Ev) -> Ev {
        let many = |e: &Ev| match e {
            Ev::V(_) => false,
            Ev::Area(_, r) => r.start != r.end,
            Ev::Arr(a) => a.len() > 1 || a.first().is_some_and(|r| r.len() > 1),
        };
        if !many(&x) && !many(&y) {
            return Ev::V(apply(op, &self.scalar(x), &self.scalar(y)));
        }
        let (a, b) = (self.rows(x), self.rows(y));
        let rows = a.len().max(b.len());
        let cols = a.iter().chain(&b).map(Vec::len).max().unwrap_or(0);
        let pick = |m: &Vec<Vec<Value>>, r: usize, c: usize| -> Value {
            let rr = if m.len() == 1 { 0 } else { r };
            let row = match m.get(rr) {
                Some(row) => row,
                None => return Value::Error(ErrorKind::NA),
            };
            let cc = if row.len() == 1 { 0 } else { c };
            row.get(cc).cloned().unwrap_or(Value::Error(ErrorKind::NA))
        };
        Ev::Arr((0..rows).map(|r| (0..cols).map(|c| apply(op, &pick(&a, r, c), &pick(&b, r, c))).collect()).collect())
    }
}

fn num_result(r: Result<f64, ErrorKind>) -> Value {
    match r {
        Ok(n) if n.is_finite() => Value::Number(n),
        Ok(_) => Value::Error(ErrorKind::Num),
        Err(e) => Value::Error(e),
    }
}

/// An operator on two values, as Excel works it.
pub fn apply(op: BinOp, a: &Value, b: &Value) -> Value {
    if let Value::Error(e) = a {
        return Value::Error(*e);
    }
    if let Value::Error(e) = b {
        return Value::Error(*e);
    }
    use std::cmp::Ordering::*;
    let cmp = || a.compare(b);
    match op {
        BinOp::Eq => Value::Bool(cmp() == Equal),
        BinOp::Ne => Value::Bool(cmp() != Equal),
        BinOp::Lt => Value::Bool(cmp() == Less),
        BinOp::Le => Value::Bool(cmp() != Greater),
        BinOp::Gt => Value::Bool(cmp() == Greater),
        BinOp::Ge => Value::Bool(cmp() != Less),
        BinOp::Concat => match (a.to_text(), b.to_text()) {
            (Ok(x), Ok(y)) => Value::text(x + &y),
            (Err(e), _) | (_, Err(e)) => Value::Error(e),
        },
        _ => {
            let (x, y) = match (a.to_number(), b.to_number()) {
                (Ok(x), Ok(y)) => (x, y),
                (Err(e), _) | (_, Err(e)) => return Value::Error(e),
            };
            match op {
                BinOp::Add => num_result(Ok(x + y)),
                BinOp::Sub => num_result(Ok(x - y)),
                BinOp::Mul => num_result(Ok(x * y)),
                BinOp::Div if y == 0.0 => Value::Error(ErrorKind::Div0),
                BinOp::Div => num_result(Ok(x / y)),
                BinOp::Pow if x == 0.0 && y == 0.0 => Value::Error(ErrorKind::Num),
                BinOp::Pow => num_result(Ok(x.powf(y))),
                _ => unreachable!("comparisons and & are above"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Sheet;

    fn at(s: &str) -> CellRef {
        CellRef::parse(s).unwrap()
    }

    #[test]
    fn a_change_reaches_what_depends_on_it_and_nothing_else() {
        let mut book = Book::new(Workbook::default());
        let mut edits = Vec::new();
        for r in 0..20_000u32 {
            edits.push((0, CellRef::new(r, 0), (r + 1).to_string()));
            edits.push((0, CellRef::new(r, 1), format!("=A{}*2", r + 1)));
        }
        edits.push((0, at("D1"), "=SUM(B:B)".into()));
        edits.push((0, at("D2"), "=D1+1".into()));
        edits.push((0, at("E1"), "=A2".into()));
        book.set_many(&edits);
        assert_eq!(book.value(0, at("D1")), Value::Number(20_000.0 * 20_001.0));
        let before = book.computed();
        let moved = book.set(0, at("A1"), "101");
        assert_eq!(book.computed() - before, 3, "B1, the SUM and what reads the SUM");
        assert_eq!(moved, vec![(0, at("A1")), (0, at("B1")), (0, at("D1")), (0, at("D2"))]);
        assert_eq!(book.value(0, at("D2")), Value::Number(20_000.0 * 20_001.0 + 201.0));
        assert_eq!(book.value(0, at("E1")), Value::Number(2.0));
    }

    #[test]
    fn formulas_are_worked_out_after_what_they_read_whatever_the_order_typed() {
        let mut book = Book::new(Workbook::default());
        book.set_many(&[(0, at("A1"), "=A2+1".into()), (0, at("A2"), "=A3*2".into()), (0, at("A3"), "5".into())]);
        assert_eq!(book.value(0, at("A1")), Value::Number(11.0));
        book.set(0, at("A3"), "");
        assert_eq!(book.value(0, at("A1")), Value::Number(1.0), "an emptied cell reads as 0");
        assert_eq!(book.value(0, at("A3")), Value::Empty);
    }

    #[test]
    fn cycles_are_found_and_broken() {
        let mut book = Book::new(Workbook::default());
        book.set_many(&[(0, at("A1"), "=B1+1".into()), (0, at("B1"), "=A1+1".into()), (0, at("C1"), "=A1*2".into()), (0, at("D1"), "=SUM(D2:D9)+D1".into())]);
        for c in ["A1", "B1", "C1", "D1"] {
            assert_eq!(book.value(0, at(c)), Value::Error(ErrorKind::Cycle), "{c}");
        }
        book.set(0, at("B1"), "4");
        assert_eq!(book.value(0, at("A1")), Value::Number(5.0), "the cycle broken");
        assert_eq!(book.value(0, at("C1")), Value::Number(10.0));
    }

    #[test]
    fn sheets_names_and_errors_reach_across() {
        let mut wb = Workbook::default();
        wb.sheets.push(Sheet::new("s2", "Q1 data"));
        wb.names.insert("Rate".into(), "Sheet1!$B$1".into());
        let mut book = Book::new(wb);
        book.set_many(&[(0, at("B1"), "0.1".into()), (1, at("A1"), "200".into()), (0, at("C1"), "='Q1 data'!A1*Rate".into()), (0, at("C2"), "=Nowhere!A1".into()), (0, at("C3"), "=1/0+C1".into()), (0, at("C4"), "=SUM(".into())]);
        assert_eq!(book.value(0, at("C1")), Value::Number(20.0));
        book.set(1, at("A1"), "300");
        assert_eq!(book.value(0, at("C1")), Value::Number(30.0), "a change on another sheet reaches here");
        book.set(0, at("B1"), "0.5");
        assert_eq!(book.value(0, at("C1")), Value::Number(150.0), "and through a name");
        assert_eq!(book.value(0, at("C2")), Value::Error(ErrorKind::Ref));
        assert_eq!(book.value(0, at("C3")), Value::Error(ErrorKind::Div0));
        assert_eq!(book.value(0, at("C4")), Value::Error(ErrorKind::Name));
        assert!(book.problem(0, at("C4")).is_some());
    }

    #[test]
    fn typing_reads_as_a_cell_reads_it() {
        assert_eq!(literal("12"), Value::Number(12.0));
        assert_eq!(literal("'12"), Value::text("12"));
        assert_eq!(literal("true"), Value::Bool(true));
        assert_eq!(literal("#N/A"), Value::Error(ErrorKind::NA));
        assert_eq!(literal("2024-03-15"), Value::Number(45366.0));
        assert_eq!(literal("18:00"), Value::Number(0.75));
        assert_eq!(literal("50%"), Value::Number(0.5));
        assert_eq!(literal("hello"), Value::text("hello"));
    }

    #[test]
    fn volatile_functions_are_worked_out_on_every_change() {
        let mut book = Book::new(Workbook::default());
        book.set_now(45366.0);
        book.set(0, at("A1"), "=TODAY()");
        assert_eq!(book.value(0, at("A1")), Value::Number(45366.0));
        book.set_now(45367.0);
        book.set(0, at("Z9"), "1");
        assert_eq!(book.value(0, at("A1")), Value::Number(45367.0));
    }
}
