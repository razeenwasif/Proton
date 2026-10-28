//! A formula read into a tree, and written back out. Excel's grammar and its
//! precedence: `:` binds tightest, then negation (so `-2^2` is 4), `%`, `^`
//! (left to right), `*` `/`, `+` `-`, `&`, and the comparisons last.

use crate::refs::A1;
use crate::value::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinOp {
    fn text(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
            BinOp::Concat => "&",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
        }
    }

    /// How tightly it binds on its left and right.
    fn power(self) -> (u8, u8) {
        match self {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (1, 2),
            BinOp::Concat => (3, 4),
            BinOp::Add | BinOp::Sub => (5, 6),
            BinOp::Mul | BinOp::Div => (7, 8),
            BinOp::Pow => (9, 10),
        }
    }
}

/// Negation binds tighter than `^`; `%` tighter still.
const NEG_POWER: u8 = 11;
const PERCENT_POWER: u8 = 12;
const RANGE_POWER: (u8, u8) = (13, 14);

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
    /// One cell: `B7`, `Data!$B$7`.
    Ref { sheet: Option<String>, at: A1 },
    /// A rectangle, or whole columns (`C:D`) or rows (`3:5`).
    Range { sheet: Option<String>, start: A1, end: A1 },
    /// A defined name, or a word that is not one (#NAME? when worked out).
    Name(String),
    Neg(Box<Expr>),
    Plus(Box<Expr>),
    Percent(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// A function, by its name in capitals.
    Call(String, Vec<Expr>),
    /// `{1,2;3,4}`: rows of constants.
    Array(Vec<Vec<Expr>>),
    /// `:` between two things that are references only once worked out:
    /// `INDEX(…):INDEX(…)`.
    RangeOp(Box<Expr>, Box<Expr>),
    /// An argument left out: the middle of `IF(A1,,2)`.
    Missing,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Err(ErrorKind),
    /// Letters, digits, `$`, `_`, `.`: a reference, a name or a function.
    Word(String),
    /// `Data!` or `'My sheet'!`, before a reference.
    Sheet(String),
    Op(&'static str),
}

/// Why a formula could not be read, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
    pub message: String,
}

fn lex(src: &str) -> Result<Vec<(usize, Tok)>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let err = |at: usize, m: &str| ParseError { at, message: m.into() };
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err(err(start, "a text without its closing quote")),
                    Some('"') if chars.get(i + 1) == Some(&'"') => {
                        s.push('"');
                        i += 2;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            out.push((start, Tok::Str(s)));
            continue;
        }
        if c == '\'' {
            // A quoted sheet name, `''` for a quote in it, then `!`.
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err(err(start, "a sheet name without its closing quote")),
                    Some('\'') if chars.get(i + 1) == Some(&'\'') => {
                        s.push('\'');
                        i += 2;
                    }
                    Some('\'') => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            if chars.get(i) != Some(&'!') {
                return Err(err(start, "a quoted sheet name with no ! after it"));
            }
            i += 1;
            out.push((start, Tok::Sheet(s)));
            continue;
        }
        if c == '#' {
            let rest: String = chars[i..].iter().collect();
            let up = rest.to_ascii_uppercase();
            let found = [
                ErrorKind::Div0,
                ErrorKind::NA,
                ErrorKind::Name,
                ErrorKind::Null,
                ErrorKind::Num,
                ErrorKind::Ref,
                ErrorKind::Value,
                ErrorKind::Cycle,
            ]
            .into_iter()
            .find(|e| up.starts_with(e.text()));
            match found {
                Some(e) => {
                    i += e.text().chars().count();
                    out.push((start, Tok::Err(e)));
                    continue;
                }
                None => return Err(err(start, "an error that is not one")),
            }
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            // Digits running into letters are a word: `1A` is not a number,
            // though `1:3` is rows and the parser decides that.
            if i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '_') {
                while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '.' | '$')) {
                    i += 1;
                }
                let w: String = chars[start..i].iter().collect();
                out.push((start, Tok::Word(w)));
                continue;
            }
            let s: String = chars[start..i].iter().collect();
            let n: f64 = s.parse().map_err(|_| err(start, "a number that is not one"))?;
            out.push((start, Tok::Num(n)));
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' || c == '\\' {
            while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '.' | '$' | '\\')) {
                i += 1;
            }
            let w: String = chars[start..i].iter().collect();
            if chars.get(i) == Some(&'!') {
                i += 1;
                out.push((start, Tok::Sheet(w)));
            } else {
                out.push((start, Tok::Word(w)));
            }
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let op: &'static str = match two.as_str() {
            "<>" => "<>",
            "<=" => "<=",
            ">=" => ">=",
            _ => match c {
                '+' => "+",
                '-' => "-",
                '*' => "*",
                '/' => "/",
                '^' => "^",
                '&' => "&",
                '=' => "=",
                '<' => "<",
                '>' => ">",
                '%' => "%",
                ':' => ":",
                ',' => ",",
                ';' => ";",
                '(' => "(",
                ')' => ")",
                '{' => "{",
                '}' => "}",
                _ => return Err(err(start, &format!("“{c}” means nothing here"))),
            },
        };
        i += op.len();
        out.push((start, Tok::Op(op)));
    }
    Ok(out)
}

struct Parser {
    toks: Vec<(usize, Tok)>,
    i: usize,
    len: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|(_, t)| t)
    }

    fn at(&self) -> usize {
        self.toks.get(self.i).map_or(self.len, |(a, _)| *a)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.i).map(|(_, t)| t.clone());
        self.i += 1;
        t
    }

    fn fail<T>(&self, m: &str) -> Result<T, ParseError> {
        Err(ParseError { at: self.at(), message: m.into() })
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }

    fn expect(&mut self, op: &str) -> Result<(), ParseError> {
        if self.is_op(op) {
            self.i += 1;
            Ok(())
        } else {
            self.fail(&format!("“{op}” was expected"))
        }
    }

    fn expr(&mut self, min: u8) -> Result<Expr, ParseError> {
        let mut lhs = if self.is_op("-") {
            self.i += 1;
            Expr::Neg(Box::new(self.expr(NEG_POWER)?))
        } else if self.is_op("+") {
            self.i += 1;
            Expr::Plus(Box::new(self.expr(NEG_POWER)?))
        } else {
            self.primary()?
        };
        while let Some(Tok::Op(o)) = self.peek() {
            let op = *o;
            if op == "%" {
                if PERCENT_POWER < min {
                    break;
                }
                self.i += 1;
                lhs = Expr::Percent(Box::new(lhs));
                continue;
            }
            if op == ":" {
                if RANGE_POWER.0 < min {
                    break;
                }
                self.i += 1;
                let rhs = self.range_end()?;
                lhs = range(lhs, rhs);
                continue;
            }
            let bin = match op {
                "+" => BinOp::Add,
                "-" => BinOp::Sub,
                "*" => BinOp::Mul,
                "/" => BinOp::Div,
                "^" => BinOp::Pow,
                "&" => BinOp::Concat,
                "=" => BinOp::Eq,
                "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                "<=" => BinOp::Le,
                ">" => BinOp::Gt,
                ">=" => BinOp::Ge,
                _ => break,
            };
            let (l, r) = bin.power();
            if l < min {
                break;
            }
            self.i += 1;
            let rhs = self.expr(r)?;
            lhs = Expr::Binary(bin, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    /// What follows a `:`: a reference end, a row number or a column's
    /// letters — or anything that is a reference once worked out.
    fn range_end(&mut self) -> Result<Expr, ParseError> {
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        match self.next() {
            None => self.fail("the formula stops too soon"),
            Some(Tok::Num(n)) => {
                // `3:5` is rows: a whole number before or after a `:`.
                let next_is_range = self.is_op(":");
                let prev_was_range = self.i >= 2 && matches!(self.toks.get(self.i - 2), Some((_, Tok::Op(":"))));
                if (next_is_range || prev_was_range) && n.fract() == 0.0 && n >= 1.0 {
                    if let Some(a) = A1::parse(&format!("{}", n as u64)) {
                        return Ok(Expr::Ref { sheet: None, at: a });
                    }
                }
                Ok(Expr::Number(n))
            }
            Some(Tok::Str(s)) => Ok(Expr::Text(s)),
            Some(Tok::Err(e)) => Ok(Expr::Error(e)),
            Some(Tok::Sheet(sheet)) => match self.next() {
                Some(Tok::Word(w)) => match A1::parse(&w) {
                    Some(a) => Ok(Expr::Ref { sheet: Some(sheet), at: a }),
                    None => self.fail("a sheet's name with no cell after it"),
                },
                Some(Tok::Num(n)) if n.fract() == 0.0 && n >= 1.0 => match A1::parse(&format!("{}", n as u64)) {
                    Some(a) => Ok(Expr::Ref { sheet: Some(sheet), at: a }),
                    None => self.fail("a row that is not one"),
                },
                _ => self.fail("a sheet's name with no cell after it"),
            },
            Some(Tok::Word(w)) => {
                if self.is_op("(") {
                    self.i += 1;
                    let name = function_name(&w);
                    let mut args = Vec::new();
                    if self.is_op(")") {
                        self.i += 1;
                        return Ok(Expr::Call(name, args));
                    }
                    loop {
                        if self.is_op(",") || self.is_op(")") {
                            args.push(Expr::Missing);
                        } else {
                            args.push(self.expr(0)?);
                        }
                        if self.is_op(",") {
                            self.i += 1;
                            continue;
                        }
                        self.expect(")")?;
                        break;
                    }
                    return Ok(Expr::Call(name, args));
                }
                let up = w.to_ascii_uppercase();
                if up == "TRUE" || up == "FALSE" {
                    return Ok(Expr::Bool(up == "TRUE"));
                }
                if let Some(a) = A1::parse(&w) {
                    let whole = a.col.is_some() && a.row.is_some();
                    let in_range = self.is_op(":") || (self.i >= 2 && matches!(self.toks.get(self.i - 2), Some((_, Tok::Op(":")))));
                    if whole || in_range {
                        return Ok(Expr::Ref { sheet: None, at: a });
                    }
                }
                Ok(Expr::Name(w))
            }
            Some(Tok::Op("(")) => {
                let e = self.expr(0)?;
                self.expect(")")?;
                Ok(e)
            }
            Some(Tok::Op("{")) => {
                let mut rows = vec![Vec::new()];
                loop {
                    let item = match self.next() {
                        Some(Tok::Num(n)) => Expr::Number(n),
                        Some(Tok::Op("-")) => match self.next() {
                            Some(Tok::Num(n)) => Expr::Number(-n),
                            _ => return self.fail("an array holds constants only"),
                        },
                        Some(Tok::Str(s)) => Expr::Text(s),
                        Some(Tok::Err(e)) => Expr::Error(e),
                        Some(Tok::Word(w)) if w.eq_ignore_ascii_case("TRUE") || w.eq_ignore_ascii_case("FALSE") => {
                            Expr::Bool(w.eq_ignore_ascii_case("TRUE"))
                        }
                        _ => return self.fail("an array holds constants only"),
                    };
                    rows.last_mut().expect("a row").push(item);
                    match self.next() {
                        Some(Tok::Op(",")) => {}
                        Some(Tok::Op(";")) => rows.push(Vec::new()),
                        Some(Tok::Op("}")) => break,
                        _ => return self.fail("“}” was expected"),
                    }
                }
                if rows.iter().any(|r| r.len() != rows[0].len()) {
                    return self.fail("an array's rows are not all as long");
                }
                Ok(Expr::Array(rows))
            }
            Some(Tok::Op(o)) => {
                self.i -= 1;
                self.fail(&format!("“{o}” cannot start a value"))
            }
        }
    }
}

/// A function's name as kept: capitals, without the `_xlfn.` an `.xlsx`
/// puts before the newer ones.
fn function_name(w: &str) -> String {
    let up = w.to_ascii_uppercase();
    let up = up.strip_prefix("_XLFN.").unwrap_or(&up);
    let up = up.strip_prefix("_XLWS.").unwrap_or(up);
    up.to_string()
}

/// `lhs:rhs`: two ends of one sheet joined, statically, or worked out.
fn range(lhs: Expr, rhs: Expr) -> Expr {
    match (lhs, rhs) {
        (Expr::Ref { sheet, at: a }, Expr::Ref { sheet: None, at: b }) => Expr::Range { sheet, start: a, end: b },
        (Expr::Ref { sheet: Some(s1), at: a }, Expr::Ref { sheet: Some(s2), at: b }) if s1 == s2 => {
            Expr::Range { sheet: Some(s1), start: a, end: b }
        }
        (l, r) => Expr::RangeOp(Box::new(l), Box::new(r)),
    }
}

/// A formula, without its `=`, read.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser { toks, i: 0, len: src.chars().count() };
    let e = p.expr(0)?;
    if p.i < p.toks.len() {
        return p.fail("something follows the end of the formula");
    }
    Ok(e)
}

/// A sheet's name as a formula writes it: quoted when it must be.
pub fn sheet_prefix(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
        && A1::parse(name).is_none();
    if plain {
        format!("{name}!")
    } else {
        format!("'{}'!", name.replace('\'', "''"))
    }
}

impl Expr {
    /// Written back as a formula, without its `=`.
    pub fn write(&self) -> String {
        fn wrap(e: &Expr, parent: u8) -> String {
            let s = e.write();
            let own = match e {
                Expr::Binary(op, ..) => op.power().0,
                Expr::Neg(_) | Expr::Plus(_) => NEG_POWER,
                Expr::Percent(_) => PERCENT_POWER,
                _ => 255,
            };
            if own < parent { format!("({s})") } else { s }
        }
        match self {
            Expr::Number(n) => crate::value::general(*n),
            Expr::Text(s) => format!("\"{}\"", s.replace('"', "\"\"")),
            Expr::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            Expr::Error(e) => e.text().into(),
            Expr::Ref { sheet, at } => format!("{}{}", sheet.as_deref().map(sheet_prefix).unwrap_or_default(), at.name()),
            Expr::Range { sheet, start, end } => {
                format!("{}{}:{}", sheet.as_deref().map(sheet_prefix).unwrap_or_default(), start.name(), end.name())
            }
            Expr::Name(n) => n.clone(),
            Expr::Neg(e) => format!("-{}", wrap(e, NEG_POWER)),
            Expr::Plus(e) => format!("+{}", wrap(e, NEG_POWER)),
            Expr::Percent(e) => format!("{}%", wrap(e, PERCENT_POWER)),
            Expr::Binary(op, a, b) => {
                let (l, r) = op.power();
                format!("{}{}{}", wrap(a, l), op.text(), wrap(b, r))
            }
            Expr::Call(name, args) => format!("{name}({})", args.iter().map(Expr::write).collect::<Vec<_>>().join(",")),
            Expr::Array(rows) => format!(
                "{{{}}}",
                rows.iter().map(|r| r.iter().map(Expr::write).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join(";")
            ),
            Expr::RangeOp(a, b) => format!("{}:{}", a.write(), b.write()),
            Expr::Missing => String::new(),
        }
    }

    /// Every reference in it, moved by `rows` and `cols` where it is
    /// relative, as copying a formula moves it; one moved off the sheet
    /// becomes #REF!.
    pub fn shifted(&self, rows: i64, cols: i64) -> Expr {
        fn move_a1(a: A1, rows: i64, cols: i64) -> Option<A1> {
            let col = match a.col {
                Some((c, false)) => {
                    let n = c as i64 + cols;
                    if !(0..=crate::refs::MAX_COL as i64).contains(&n) {
                        return None;
                    }
                    Some((n as u32, false))
                }
                other => other,
            };
            let row = match a.row {
                Some((r, false)) => {
                    let n = r as i64 + rows;
                    if !(0..=crate::refs::MAX_ROW as i64).contains(&n) {
                        return None;
                    }
                    Some((n as u32, false))
                }
                other => other,
            };
            Some(A1 { col, row })
        }
        let s = |e: &Expr| Box::new(e.shifted(rows, cols));
        match self {
            Expr::Ref { sheet, at } => match move_a1(*at, rows, cols) {
                Some(at) => Expr::Ref { sheet: sheet.clone(), at },
                None => Expr::Error(ErrorKind::Ref),
            },
            Expr::Range { sheet, start, end } => match (move_a1(*start, rows, cols), move_a1(*end, rows, cols)) {
                (Some(start), Some(end)) => Expr::Range { sheet: sheet.clone(), start, end },
                _ => Expr::Error(ErrorKind::Ref),
            },
            Expr::Neg(e) => Expr::Neg(s(e)),
            Expr::Plus(e) => Expr::Plus(s(e)),
            Expr::Percent(e) => Expr::Percent(s(e)),
            Expr::Binary(op, a, b) => Expr::Binary(*op, s(a), s(b)),
            Expr::Call(n, args) => Expr::Call(n.clone(), args.iter().map(|a| a.shifted(rows, cols)).collect()),
            Expr::RangeOp(a, b) => Expr::RangeOp(s(a), s(b)),
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(src: &str) -> String {
        parse(src).unwrap_or_else(|e| panic!("{src}: {e:?}")).write()
    }

    #[test]
    fn precedence_is_excels() {
        // Negation before ^, so -2^2 is (-2)^2; ^ left to right.
        assert_eq!(parse("-2^2").unwrap(), Expr::Binary(BinOp::Pow, Box::new(Expr::Neg(Box::new(Expr::Number(2.0)))), Box::new(Expr::Number(2.0))));
        assert_eq!(round_trip("1+2*3"), "1+2*3");
        assert_eq!(round_trip("(1+2)*3"), "(1+2)*3");
        assert_eq!(round_trip("2^3^2"), "2^3^2");
        assert_eq!(round_trip("A1&\"x\"=B1"), "A1&\"x\"=B1");
        assert_eq!(round_trip("50%*2"), "50%*2");
    }

    #[test]
    fn references_ranges_and_sheets_are_read() {
        assert_eq!(round_trip("SUM(A1:B$3, $C:$C, 2:4)"), "SUM(A1:B$3,$C:$C,2:4)");
        assert_eq!(round_trip("Data!B2+'My sheet'!C3:D4"), "Data!B2+'My sheet'!C3:D4");
        assert_eq!(round_trip("'It''s'!A1"), "'It''s'!A1");
        assert!(matches!(parse("total").unwrap(), Expr::Name(_)), "a word that is no cell is a name");
        assert!(matches!(parse("INDEX(A:A,2):INDEX(A:A,5)").unwrap(), Expr::RangeOp(..)));
    }

    #[test]
    fn calls_texts_arrays_and_errors_are_read() {
        assert_eq!(round_trip("if(a1>0,\"say \"\"hi\"\"\",)"), "IF(A1>0,\"say \"\"hi\"\"\",)");
        assert_eq!(round_trip("_xlfn.XLOOKUP(1,A:A,B:B)"), "XLOOKUP(1,A:A,B:B)");
        assert_eq!(round_trip("SUM({1,2;3,-4})"), "SUM({1,2;3,-4})");
        assert_eq!(round_trip("IFERROR(#N/A,TRUE)"), "IFERROR(#N/A,TRUE)");
        assert_eq!(round_trip("STDEV.S(A1:A9)"), "STDEV.S(A1:A9)");
        assert_eq!(round_trip("NOW()"), "NOW()");
    }

    #[test]
    fn what_cannot_be_read_says_where() {
        for bad in ["1+", "SUM(1", "\"open", "{1,A1}", "#WHAT", "1 2", "{1,2;3}"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert_eq!(parse("1+").unwrap_err().at, 2);
    }

    #[test]
    fn copying_moves_what_is_relative_only() {
        let e = parse("A1+$B2+C$3+$D$4+SUM(E1:E3)").unwrap();
        assert_eq!(e.shifted(1, 1).write(), "B2+$B3+D$3+$D$4+SUM(F2:F4)");
        assert_eq!(parse("A1").unwrap().shifted(-1, 0).write(), "#REF!");
    }
}
