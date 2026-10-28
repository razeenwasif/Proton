//! What a cell holds once worked out, and how one kind becomes another the
//! way Excel turns them: text to a number for arithmetic, anything to text
//! for `&`, and the order comparisons put them in.

use std::cmp::Ordering;

/// An error, as a cell shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    Div0,
    NA,
    Name,
    Null,
    Num,
    Ref,
    Value,
    /// A formula that reads itself, through however many others.
    Cycle,
}

impl ErrorKind {
    pub fn text(self) -> &'static str {
        match self {
            ErrorKind::Div0 => "#DIV/0!",
            ErrorKind::NA => "#N/A",
            ErrorKind::Name => "#NAME?",
            ErrorKind::Null => "#NULL!",
            ErrorKind::Num => "#NUM!",
            ErrorKind::Ref => "#REF!",
            ErrorKind::Value => "#VALUE!",
            ErrorKind::Cycle => "#CYCLE!",
        }
    }

    pub fn parse(s: &str) -> Option<ErrorKind> {
        let up = s.to_ascii_uppercase();
        [
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
        .find(|e| e.text() == up)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
}

impl Value {
    pub fn text(s: impl Into<String>) -> Value {
        Value::Text(s.into())
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Value::Empty)
    }

    /// As a number for arithmetic: text that reads as one is, empty is 0,
    /// TRUE is 1; other text is #VALUE!.
    pub fn to_number(&self) -> Result<f64, ErrorKind> {
        match self {
            Value::Empty => Ok(0.0),
            Value::Number(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Text(t) => parse_number(t).ok_or(ErrorKind::Value),
            Value::Error(e) => Err(*e),
        }
    }

    /// As text for `&` and the text functions.
    pub fn to_text(&self) -> Result<String, ErrorKind> {
        match self {
            Value::Empty => Ok(String::new()),
            Value::Number(n) => Ok(general(*n)),
            Value::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.into()),
            Value::Text(t) => Ok(t.clone()),
            Value::Error(e) => Err(*e),
        }
    }

    /// As TRUE or FALSE for IF and the logic functions: numbers by whether
    /// they are 0, the words TRUE and FALSE, and nothing else.
    pub fn to_bool(&self) -> Result<bool, ErrorKind> {
        match self {
            Value::Empty => Ok(false),
            Value::Number(n) => Ok(*n != 0.0),
            Value::Bool(b) => Ok(*b),
            Value::Text(t) => match t.to_ascii_uppercase().as_str() {
                "TRUE" => Ok(true),
                "FALSE" => Ok(false),
                _ => Err(ErrorKind::Value),
            },
            Value::Error(e) => Err(*e),
        }
    }

    /// The order `=`, `<` and sorting put values in, as Excel's: numbers,
    /// then text (without case), then TRUE and FALSE; an empty cell is 0
    /// against a number and "" against text.
    pub fn compare(&self, other: &Value) -> Ordering {
        fn rank(v: &Value) -> u8 {
            match v {
                Value::Number(_) | Value::Empty => 0,
                Value::Text(_) => 1,
                Value::Bool(_) => 2,
                Value::Error(_) => 3,
            }
        }
        match (self, other) {
            (Value::Empty, Value::Empty) => Ordering::Equal,
            (Value::Empty, Value::Text(t)) => "".cmp(t.to_lowercase().as_str()),
            (Value::Text(t), Value::Empty) => t.to_lowercase().as_str().cmp(""),
            (Value::Empty, Value::Bool(b)) => false.cmp(b),
            (Value::Bool(b), Value::Empty) => b.cmp(&false),
            (Value::Empty, Value::Number(n)) => 0f64.partial_cmp(n).unwrap_or(Ordering::Equal),
            (Value::Number(n), Value::Empty) => n.partial_cmp(&0.0).unwrap_or(Ordering::Equal),
            (Value::Number(a), Value::Number(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Value::Text(a), Value::Text(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (a, b) => rank(a).cmp(&rank(b)),
        }
    }
}

/// Text read as a number, as typing it into a cell or using it in arithmetic
/// does: `12`, `-3.5`, `1e3`, `1,234`, `45%`, `$12`. Dates are left to the
/// cell's own reading.
pub fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (t, percent) = match t.strip_suffix('%') {
        Some(rest) => (rest.trim_end(), true),
        None => (t, false),
    };
    let (neg, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let t = t.strip_prefix('$').unwrap_or(t);
    // Thousands separators only between groups of three.
    let plain: String = if t.contains(',') {
        let (int, frac) = t.split_once('.').map_or((t, None), |(i, f)| (i, Some(f)));
        let groups: Vec<&str> = int.split(',').collect();
        let ok = !groups[0].is_empty() && groups[0].len() <= 3 && groups[1..].iter().all(|g| g.len() == 3);
        if !ok {
            return None;
        }
        match frac {
            Some(f) => format!("{}.{f}", groups.concat()),
            None => groups.concat(),
        }
    } else {
        t.to_string()
    };
    if plain.is_empty()
        || !plain.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        || !plain.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')
    {
        return None;
    }
    let mut n: f64 = plain.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    if neg {
        n = -n;
    }
    if percent {
        n /= 100.0;
    }
    Some(n)
}

/// A number as text the way Excel's General shows it in a formula's
/// result: up to fifteen significant digits, no trailing zeros, scientific
/// only when very large or very small.
pub fn general(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if !n.is_finite() {
        return "#NUM!".into();
    }
    let abs = n.abs();
    if !(1e-9..1e15).contains(&abs) {
        let s = format!("{:.14E}", n);
        let (mant, exp) = s.split_once('E').unwrap_or((&s, "0"));
        let mant = trim_zeros(mant);
        let e: i32 = exp.parse().unwrap_or(0);
        return format!("{mant}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let digits = 15 - (abs.log10().floor() as i32 + 1);
    let s = format!("{:.*}", digits.clamp(0, 20) as usize, n);
    trim_zeros(&s)
}

/// `n` to fifteen significant digits, as Excel holds a result before
/// rounding it, so 2.675 is the 2.675 it was typed as and not the
/// 2.67499999… a double makes it.
pub fn fifteen(n: f64) -> f64 {
    if n == 0.0 || !n.is_finite() {
        return n;
    }
    format!("{n:.14e}").parse().unwrap_or(n)
}

/// `n` rounded to `digits` places, halves away from zero, as ROUND and a
/// number format round; negative `digits` round left of the point.
pub fn round_half_away(n: f64, digits: i32) -> f64 {
    let digits = digits.clamp(-308, 308);
    if digits >= 0 {
        let m = 10f64.powi(digits);
        let r = fifteen(n * m).round() / m;
        if r.is_finite() { r } else { n }
    } else {
        let m = 10f64.powi(-digits);
        fifteen(n / m).round() * m
    }
}

fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        let t = s.trim_end_matches('0').trim_end_matches('.');
        if t == "-0" { "0".into() } else { t.to_string() }
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_reads_as_a_number_as_a_cell_does() {
        for (s, n) in [("12", 12.0), (" -3.5 ", -3.5), ("1e3", 1000.0), ("1,234.5", 1234.5), ("45%", 0.45), ("$12", 12.0), (".5", 0.5)] {
            assert_eq!(parse_number(s), Some(n), "{s}");
        }
        for s in ["", "abc", "1,23", "12a", "--1", "e5", "1.2.3"] {
            assert_eq!(parse_number(s), None, "{s}");
        }
    }

    #[test]
    fn general_shows_fifteen_digits_and_no_more() {
        assert_eq!(general(0.1 + 0.2), "0.3");
        assert_eq!(general(1.0 / 3.0), "0.333333333333333");
        assert_eq!(general(12.0), "12");
        assert_eq!(general(-2.5), "-2.5");
        assert_eq!(general(123456789012345678.0), "1.23456789012346E+17");
        assert_eq!(general(0.0000000001), "1E-10");
        assert_eq!(round_half_away(2.675, 2), 2.68, "as typed, not as a double holds it");
        assert_eq!(round_half_away(-2.5, 0), -3.0);
        assert_eq!(round_half_away(1234.0, -2), 1200.0);
    }

    #[test]
    fn values_compare_as_excel_orders_them() {
        assert_eq!(Value::text("abc").compare(&Value::text("ABC")), Ordering::Equal);
        assert_eq!(Value::Number(5.0).compare(&Value::text("1")), Ordering::Less, "numbers before text");
        assert_eq!(Value::text("z").compare(&Value::Bool(false)), Ordering::Less, "text before logicals");
        assert_eq!(Value::Empty.compare(&Value::Number(0.0)), Ordering::Equal);
        assert_eq!(Value::Empty.compare(&Value::text("")), Ordering::Equal);
        assert_eq!(Value::text("TRUE").to_bool(), Ok(true));
        assert_eq!(Value::text("x").to_number(), Err(ErrorKind::Value));
    }
}
