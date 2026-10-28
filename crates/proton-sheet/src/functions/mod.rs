//! The functions a formula may call, by family. Each takes its arguments
//! unworked — `IF` works out only the branch it takes, `INDEX` and `OFFSET`
//! hand back references — and the helpers here read them as Excel does:
//! `SUM(A1:A9)` skips text in the cells, `SUM("5")` reads the 5.

mod date;
mod info;
mod logic;
mod lookup;
pub(crate) mod math;
mod stats;
mod text;

use crate::book::{Ctx, Ev};
use crate::formula::Expr;
use crate::refs::{CellRef, Range};
use crate::value::{parse_number, ErrorKind, Value};

pub type R = Result<Ev, ErrorKind>;

/// Whether a function's value can change with nothing it reads changing,
/// or reads what cannot be known before it is worked out.
pub fn volatile(name: &str) -> bool {
    matches!(name.to_ascii_uppercase().as_str(), "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "INDIRECT" | "OFFSET")
}

/// A function worked out; one this does not know is #NAME?.
pub fn call(cx: &Ctx, name: &str, args: &[Expr]) -> Ev {
    let up = name.to_ascii_uppercase();
    let up = up.strip_prefix("_XLWS.").unwrap_or(&up);
    let found = math::call(cx, up, args)
        .or_else(|| stats::call(cx, up, args))
        .or_else(|| logic::call(cx, up, args))
        .or_else(|| lookup::call(cx, up, args))
        .or_else(|| text::call(cx, up, args))
        .or_else(|| date::call(cx, up, args))
        .or_else(|| info::call(cx, up, args));
    match found {
        Some(Ok(ev)) => ev,
        Some(Err(e)) => Ev::err(e),
        None => Ev::err(ErrorKind::Name),
    }
}

/// Whether a function is one this knows: what the window offers as you type.
pub fn known(name: &str) -> bool {
    NAMES.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// Every function there is, for completion and the agent.
pub const NAMES: &[&str] = &[
    // Maths
    "SUM", "SUMIF", "SUMIFS", "SUMPRODUCT", "SUMSQ", "PRODUCT", "ROUND", "ROUNDUP", "ROUNDDOWN", "INT", "TRUNC", "MOD",
    "ABS", "POWER", "SQRT", "EXP", "LN", "LOG", "LOG10", "MIN", "MAX", "MINIFS", "MAXIFS", "RAND", "RANDBETWEEN", "PI",
    "SIGN", "CEILING", "CEILING.MATH", "FLOOR", "FLOOR.MATH", "QUOTIENT", "GCD", "LCM", "FACT", "EVEN", "ODD",
    // Statistics
    "AVERAGE", "AVERAGEA", "AVERAGEIF", "AVERAGEIFS", "MEDIAN", "MODE", "MODE.SNGL", "STDEV", "STDEV.S", "STDEV.P",
    "STDEVP", "VAR", "VAR.S", "VAR.P", "VARP", "COUNT", "COUNTA", "COUNTBLANK", "COUNTIF", "COUNTIFS", "LARGE", "SMALL",
    "RANK", "RANK.EQ", "PERCENTILE", "PERCENTILE.INC", "QUARTILE", "QUARTILE.INC", "CORREL",
    // Logic
    "IF", "IFS", "AND", "OR", "NOT", "XOR", "IFERROR", "IFNA", "SWITCH", "CHOOSE", "TRUE", "FALSE",
    // Lookup
    "VLOOKUP", "HLOOKUP", "XLOOKUP", "INDEX", "MATCH", "XMATCH", "OFFSET", "INDIRECT", "ROW", "ROWS", "COLUMN",
    "COLUMNS", "TRANSPOSE",
    // Text
    "CONCAT", "CONCATENATE", "TEXTJOIN", "LEFT", "RIGHT", "MID", "LEN", "FIND", "SEARCH", "SUBSTITUTE", "REPLACE",
    "UPPER", "LOWER", "PROPER", "TRIM", "TEXT", "VALUE", "REPT", "EXACT", "CHAR", "CODE", "T",
    // Dates
    "DATE", "TODAY", "NOW", "YEAR", "MONTH", "DAY", "WEEKDAY", "EDATE", "EOMONTH", "DATEDIF", "NETWORKDAYS", "HOUR",
    "MINUTE", "SECOND", "TIME", "DAYS", "DATEVALUE",
    // Information
    "ISBLANK", "ISNUMBER", "ISTEXT", "ISNONTEXT", "ISLOGICAL", "ISERROR", "ISERR", "ISNA", "NA", "N",
];

pub fn num(v: f64) -> R {
    if v.is_finite() {
        Ok(Ev::V(Value::Number(v)))
    } else {
        Err(ErrorKind::Num)
    }
}

pub fn txt(s: impl Into<String>) -> R {
    Ok(Ev::V(Value::text(s)))
}

pub fn boolean(b: bool) -> R {
    Ok(Ev::V(Value::Bool(b)))
}

/// Between `min` and `max` arguments, or #VALUE!.
pub fn arity(args: &[Expr], min: usize, max: usize) -> Result<(), ErrorKind> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(ErrorKind::Value)
    }
}

/// An argument as a number; one left out is `default`.
pub fn number(cx: &Ctx, args: &[Expr], i: usize, default: Option<f64>) -> Result<f64, ErrorKind> {
    match args.get(i) {
        None | Some(Expr::Missing) => default.ok_or(ErrorKind::Value),
        Some(e) => cx.value(e).to_number(),
    }
}

pub fn text_arg(cx: &Ctx, args: &[Expr], i: usize, default: Option<&str>) -> Result<String, ErrorKind> {
    match args.get(i) {
        None | Some(Expr::Missing) => default.map(str::to_string).ok_or(ErrorKind::Value),
        Some(e) => cx.value(e).to_text(),
    }
}

pub fn bool_arg(cx: &Ctx, args: &[Expr], i: usize, default: bool) -> Result<bool, ErrorKind> {
    match args.get(i) {
        None | Some(Expr::Missing) => Ok(default),
        Some(e) => cx.value(e).to_bool(),
    }
}

/// The value an argument stands for, an error in it returned.
pub fn scalar(cx: &Ctx, e: &Expr) -> Result<Value, ErrorKind> {
    match cx.value(e) {
        Value::Error(k) => Err(k),
        v => Ok(v),
    }
}

/// Every value an argument holds, and whether it was typed into the call
/// (`SUM(1,"2")`) rather than read from cells or an array.
pub fn each_value(cx: &Ctx, e: &Expr, mut f: impl FnMut(Value, bool) -> Result<(), ErrorKind>) -> Result<(), ErrorKind> {
    if matches!(e, Expr::Missing) {
        return Ok(());
    }
    match cx.eval(e) {
        Ev::V(v) => f(v, true),
        Ev::Area(s, r) => {
            let mut out = Ok(());
            cx.each(s, r, |_, v| {
                if out.is_ok() {
                    out = f(v, false);
                }
            });
            out
        }
        Ev::Arr(a) => {
            for v in a.into_iter().flatten() {
                f(v, false)?;
            }
            Ok(())
        }
    }
}

/// The numbers in the arguments as SUM, AVERAGE and MAX read them: in cells
/// and arrays only numbers count; typed in, TRUE is 1 and text that reads as
/// a number is one; an error anywhere is the answer.
pub fn numbers(cx: &Ctx, args: &[Expr]) -> Result<Vec<f64>, ErrorKind> {
    let mut out = Vec::new();
    for a in args {
        each_value(cx, a, |v, direct| {
            match v {
                Value::Number(n) => out.push(n),
                Value::Error(e) => return Err(e),
                Value::Bool(b) if direct => out.push(if b { 1.0 } else { 0.0 }),
                Value::Text(t) if direct => out.push(parse_number(&t).ok_or(ErrorKind::Value)?),
                _ => {}
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// An argument that must be a reference.
pub fn area(cx: &Ctx, e: &Expr) -> Result<(usize, Range), ErrorKind> {
    match cx.eval(e) {
        Ev::Area(s, r) => Ok((s, r)),
        Ev::V(Value::Error(k)) => Err(k),
        _ => Err(ErrorKind::Value),
    }
}

/// `r` cut to where its sheet has anything, keeping its start, so a whole
/// column is the rows in use.
pub fn cut(cx: &Ctx, sheet: usize, r: Range) -> Range {
    let used = cx.book.workbook().sheets.get(sheet).and_then(|s| s.used());
    let row = used.map_or(r.start.row, |u| u.end.row.min(r.end.row).max(r.start.row));
    let col = used.map_or(r.start.col, |u| u.end.col.min(r.end.col).max(r.start.col));
    Range::new(r.start, CellRef::new(row, col))
}

/// An argument as rows of values: a reference's cells (cut to what is in
/// use), an array, or one value.
pub fn table(cx: &Ctx, e: &Expr) -> Result<Vec<Vec<Value>>, ErrorKind> {
    match cx.eval(e) {
        Ev::V(Value::Error(k)) => Err(k),
        ev => Ok(cx.rows(ev)),
    }
}

/// A one-row or one-column table as a list, or #N/A.
pub fn vector(rows: Vec<Vec<Value>>) -> Result<Vec<Value>, ErrorKind> {
    if rows.len() == 1 {
        Ok(rows.into_iter().next().unwrap_or_default())
    } else if rows.iter().all(|r| r.len() == 1) {
        Ok(rows.into_iter().map(|mut r| r.remove(0)).collect())
    } else {
        Err(ErrorKind::NA)
    }
}

/// A condition as COUNTIF and SUMIF take one: `5`, `">=10"`, `"<>"`,
/// `"ap*"`.
#[derive(Debug, Clone)]
pub struct Criteria {
    op: &'static str,
    value: Value,
    wild: bool,
}

impl Criteria {
    pub fn new(v: &Value) -> Criteria {
        let Value::Text(s) = v else {
            return Criteria { op: "=", value: v.clone(), wild: false };
        };
        let (op, rest) = ["<=", ">=", "<>", "<", ">", "="]
            .into_iter()
            .find_map(|op| s.strip_prefix(op).map(|r| (op, r)))
            .unwrap_or(("=", s.as_str()));
        let value = if rest.is_empty() {
            Value::Empty
        } else if let Some(n) = parse_number(rest) {
            Value::Number(n)
        } else if let Some(n) = crate::dates::parse_date(rest) {
            Value::Number(n)
        } else if rest.eq_ignore_ascii_case("TRUE") {
            Value::Bool(true)
        } else if rest.eq_ignore_ascii_case("FALSE") {
            Value::Bool(false)
        } else if let Some(e) = ErrorKind::parse(rest) {
            Value::Error(e)
        } else {
            Value::text(rest)
        };
        let wild = matches!(&value, Value::Text(t) if t.contains(['*', '?', '~'])) && matches!(op, "=" | "<>");
        Criteria { op, value, wild }
    }

    pub fn matches(&self, cell: &Value) -> bool {
        let eq = || match (&self.value, cell) {
            (Value::Empty, c) => matches!(c, Value::Empty) || matches!(c, Value::Text(t) if t.is_empty()),
            (Value::Text(p), Value::Text(t)) if self.wild => wildcard(p, t),
            (Value::Text(p), Value::Text(t)) => p.to_lowercase() == t.to_lowercase(),
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Number(a), Value::Text(t)) => parse_number(t) == Some(*a),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Error(a), Value::Error(b)) => a == b,
            _ => false,
        };
        match self.op {
            "=" => eq(),
            "<>" => !eq(),
            op => {
                let ord = match (&self.value, cell) {
                    (Value::Number(a), Value::Number(b)) => b.partial_cmp(a),
                    (Value::Text(a), Value::Text(b)) => Some(b.to_lowercase().cmp(&a.to_lowercase())),
                    _ => None,
                };
                let Some(ord) = ord else { return false };
                use std::cmp::Ordering::*;
                match op {
                    "<" => ord == Less,
                    "<=" => ord != Greater,
                    ">" => ord == Greater,
                    _ => ord != Less,
                }
            }
        }
    }
}

/// `*` any run, `?` any one character, `~` before either the character
/// itself; without regard to case.
pub fn wildcard(pattern: &str, text: &str) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum P {
        Any,
        One,
        C(char),
    }
    let mut pat = Vec::new();
    let mut chars = pattern.chars().flat_map(char::to_lowercase);
    while let Some(c) = chars.next() {
        pat.push(match c {
            '*' => P::Any,
            '?' => P::One,
            '~' => match chars.next() {
                Some(n) => P::C(n),
                None => P::C('~'),
            },
            c => P::C(c),
        });
    }
    let text: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    // The classic two-pointer match, backing up to the last `*`.
    let (mut p, mut t, mut star, mut mark) = (0, 0, None, 0);
    while t < text.len() {
        match pat.get(p) {
            Some(P::One) => {
                p += 1;
                t += 1;
            }
            Some(P::C(c)) if *c == text[t] => {
                p += 1;
                t += 1;
            }
            Some(P::Any) => {
                star = Some(p);
                mark = t;
                p += 1;
            }
            _ => match star {
                Some(s) => {
                    p = s + 1;
                    mark += 1;
                    t = mark;
                }
                None => return false,
            },
        }
    }
    pat[p..].iter().all(|x| *x == P::Any)
}

/// The offsets in a set of (range, criteria) pairs where every criterion
/// holds — what the `…IFS` functions add up, count or average.
pub fn ifs_matches(cx: &Ctx, pairs: &[Expr]) -> Result<Vec<(u32, u32)>, ErrorKind> {
    if pairs.is_empty() || pairs.len() % 2 != 0 {
        return Err(ErrorKind::Value);
    }
    let mut ranges = Vec::new();
    for pair in pairs.chunks(2) {
        let (s, r) = area(cx, &pair[0])?;
        let crit = Criteria::new(&cx.value(&pair[1]));
        ranges.push((s, r, crit));
    }
    let (rows, cols) = (ranges[0].1.rows(), ranges[0].1.cols());
    if ranges.iter().any(|(_, r, _)| r.rows() != rows || r.cols() != cols) {
        return Err(ErrorKind::Value);
    }
    // Only as far as any of the sheets has anything; past that every cell
    // is empty, and an empty cell matches every range or none.
    let mut used_rows = 0;
    let mut used_cols = 0;
    for (s, r, _) in &ranges {
        let c = cut(cx, *s, *r);
        used_rows = used_rows.max(c.rows());
        used_cols = used_cols.max(c.cols());
    }
    let mut out = Vec::new();
    for dr in 0..used_rows {
        'cell: for dc in 0..used_cols {
            for (s, r, crit) in &ranges {
                let at = CellRef::new(r.start.row + dr, r.start.col + dc);
                if !crit.matches(&cx.get(*s, at)) {
                    continue 'cell;
                }
            }
            out.push((dr, dc));
        }
    }
    Ok(out)
}

/// The cells of `e` at the offsets given, as SUMIFS reads its sum range.
pub fn at_offsets(cx: &Ctx, e: &Expr, offsets: &[(u32, u32)]) -> Result<Vec<Value>, ErrorKind> {
    let (s, r) = area(cx, e)?;
    Ok(offsets.iter().map(|(dr, dc)| cx.get(s, CellRef::new(r.start.row + dr, r.start.col + dc))).collect())
}

/// The numbers among values read from cells.
pub fn only_numbers(values: Vec<Value>) -> Result<Vec<f64>, ErrorKind> {
    let mut out = Vec::new();
    for v in values {
        match v {
            Value::Number(n) => out.push(n),
            Value::Error(e) => return Err(e),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod testing {
    use crate::book::Book;
    use crate::model::Workbook;
    use crate::refs::CellRef;
    use crate::value::Value;

    /// A sheet with cells typed in, and a formula worked out in a free cell.
    pub fn sheet(cells: &[(&str, &str)]) -> Book {
        let mut book = Book::new(Workbook::default());
        book.set_now(45366.75); // 15 March 2024, 18:00
        let edits: Vec<_> = cells.iter().map(|(at, input)| (0, CellRef::parse(at).expect("a cell"), input.to_string())).collect();
        book.set_many(&edits);
        book
    }

    pub fn eval(book: &mut Book, formula: &str) -> Value {
        let at = CellRef::parse("ZZ9999").unwrap();
        book.set(0, at, &format!("={formula}"));
        book.value(0, at)
    }

    /// Each formula against what it comes to, on the cells given.
    pub fn check(cells: &[(&str, &str)], cases: &[(&str, Value)]) {
        let mut book = sheet(cells);
        for (f, want) in cases {
            let got = eval(&mut book, f);
            let same = match (&got, want) {
                (Value::Number(a), Value::Number(b)) => (a - b).abs() <= 1e-9 * b.abs().max(1.0),
                _ => got == *want,
            };
            assert!(same, "={f}: got {got:?}, want {want:?}");
        }
    }

    pub fn n(x: f64) -> Value {
        Value::Number(x)
    }

    pub fn t(s: &str) -> Value {
        Value::text(s)
    }

    pub fn e(k: crate::value::ErrorKind) -> Value {
        Value::Error(k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_and_criteria_read_as_excels() {
        assert!(wildcard("ap*", "Apple"));
        assert!(wildcard("?pple", "apple"));
        assert!(!wildcard("?pple", "pple"));
        assert!(wildcard("*~**", "a*b"));
        assert!(!wildcard("*~**", "ab"));
        assert!(wildcard("a*e", "ae"));
        let c = Criteria::new(&Value::text(">=10"));
        assert!(c.matches(&Value::Number(10.0)) && !c.matches(&Value::Number(9.0)) && !c.matches(&Value::text("x")));
        let c = Criteria::new(&Value::text("<>"));
        assert!(c.matches(&Value::Number(1.0)) && !c.matches(&Value::Empty));
        let c = Criteria::new(&Value::Number(5.0));
        assert!(c.matches(&Value::text("5")) && !c.matches(&Value::Number(6.0)));
        let c = Criteria::new(&Value::text("<>b*"));
        assert!(c.matches(&Value::text("apple")) && !c.matches(&Value::text("Banana")));
    }
}
