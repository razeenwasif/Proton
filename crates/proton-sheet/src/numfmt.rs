//! Excel's number formats: a value as a cell shows it.
//!
//! The parts people use: digit places (`0`, `#`, `?`), the point,
//! thousands (`#,##0`) and scaling by them (`0,"k"`), percentages,
//! scientific (`0.00E+00`), text in quotes or after `\`, `_` spaces and `*`
//! fills (a space and nothing here), colours (`[Red]`), currencies
//! (`[$€-407]`), dates and times (`yyyy-mm-dd`, `h:mm AM/PM`, `[h]:mm`), `@`
//! for text, and up to four sections — positive, negative, zero, text.

use crate::dates::ymd;
use crate::value::{general, round_half_away, Value};

/// A value shown: its text, and the colour a section gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    pub color: Option<&'static str>,
}

impl Shown {
    fn plain(text: String) -> Shown {
        Shown { text, color: None }
    }
}

/// `v` shown with the format `code`; an empty code is General.
pub fn show(v: &Value, code: &str) -> Shown {
    let sections = split(code);
    match v {
        Value::Empty => Shown::plain(String::new()),
        Value::Error(e) => Shown::plain(e.text().into()),
        Value::Bool(b) => Shown::plain(if *b { "TRUE" } else { "FALSE" }.into()),
        Value::Text(t) => {
            // The fourth section is for text; a lone section with `@` is too.
            let section = match sections.len() {
                4 => Some(&sections[3]),
                1 if sections[0].contains('@') => Some(&sections[0]),
                _ => None,
            };
            match section {
                Some(s) => {
                    let (s, color) = strip_brackets(s);
                    Shown { text: text_section(&s, t), color }
                }
                None => Shown::plain(t.clone()),
            }
        }
        Value::Number(n) => number(*n, &sections),
    }
}

/// A format's sections, split at `;` outside quotes and escapes.
fn split(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = code.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        let cur = out.last_mut().expect("a section");
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            '\\' if !quoted => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ';' if !quoted => out.push(String::new()),
            c => cur.push(c),
        }
    }
    out.truncate(4);
    out
}

const COLORS: [(&str, &str); 8] = [
    ("black", "#000000"),
    ("blue", "#0000ff"),
    ("cyan", "#00ffff"),
    ("green", "#00ff00"),
    ("magenta", "#ff00ff"),
    ("red", "#ff0000"),
    ("white", "#ffffff"),
    ("yellow", "#ffff00"),
];

/// A section without its bracketed parts: the colour named, a currency's
/// symbol kept as text, conditions and locales dropped. `[h]`, `[m]` and
/// `[s]` stay for the date formatter.
fn strip_brackets(s: &str) -> (String, Option<&'static str>) {
    let mut out = String::new();
    let mut color = None;
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        // A `[` in quotes is text.
        if rest[..i].matches('"').count() % 2 == 1 {
            let close = rest[i..].find('"').map_or(rest.len(), |j| i + j + 1);
            out.push_str(&rest[..close]);
            rest = &rest[close..];
            continue;
        }
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find(']') else {
            out.push_str(&rest[i..]);
            return (out, color);
        };
        let inner = &rest[i + 1..i + j];
        let lower = inner.to_ascii_lowercase();
        if let Some((_, hex)) = COLORS.iter().find(|(n, _)| *n == lower) {
            color = Some(*hex);
        } else if let Some(cur) = inner.strip_prefix('$') {
            let symbol = cur.split('-').next().unwrap_or("");
            if !symbol.is_empty() {
                out.push('"');
                out.push_str(symbol);
                out.push('"');
            }
        } else if matches!(lower.as_str(), "h" | "hh" | "m" | "mm" | "s" | "ss") {
            out.push_str(&rest[i..i + j + 1]);
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    (out, color)
}

fn number(n: f64, sections: &[String]) -> Shown {
    // Which section, and whether it shows the sign itself.
    let (index, signed) = match sections.len() {
        1 => (0, true),
        2 => (if n < 0.0 { 1 } else { 0 }, false),
        _ => (if n > 0.0 { 0 } else if n < 0.0 { 1 } else { 2 }, false),
    };
    let (code, color) = strip_brackets(&sections[index]);
    let shown_n = if signed { n } else { n.abs() };
    if code.trim().is_empty() && sections.len() > 1 {
        return Shown { text: String::new(), color };
    }
    let text = if code.is_empty() || code.eq_ignore_ascii_case("general") {
        general(shown_n)
    } else if let Some(g) = find_general(&code) {
        // `"Total: "General` — General among literals.
        let (before, after) = code.split_at(g);
        format!("{}{}{}", literal_text(before), general(shown_n), literal_text(&after[7..]))
    } else if is_date(&code) {
        date(n, &code)
    } else {
        digits(shown_n, &code)
    };
    Shown { text, color }
}

fn find_general(code: &str) -> Option<usize> {
    let lower = code.to_ascii_lowercase();
    let i = lower.find("general")?;
    (code[..i].matches('"').count() % 2 == 0).then_some(i)
}

/// A format's text parts alone: quotes, escapes, `_` spaces.
fn literal_text(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    out.push(q);
                }
            }
            '\\' => out.extend(chars.next()),
            '_' => {
                chars.next();
                out.push(' ');
            }
            '*' => {
                chars.next();
            }
            c => out.push(c),
        }
    }
    out
}

fn text_section(code: &str, t: &str) -> String {
    let mut out = String::new();
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        match c {
            '@' => out.push_str(t),
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    out.push(q);
                }
            }
            '\\' => out.extend(chars.next()),
            '_' => {
                chars.next();
                out.push(' ');
            }
            '*' => {
                chars.next();
            }
            c => out.push(c),
        }
    }
    out
}

/// Whether a section shows a date or time: a date's letters outside quotes.
fn is_date(code: &str) -> bool {
    let mut quoted = false;
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if !quoted => {
                chars.next();
            }
            c if !quoted && matches!(c.to_ascii_lowercase(), 'y' | 'm' | 'd' | 'h' | 's') => return true,
            '[' if !quoted => return true,
            _ => {}
        }
    }
    false
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Lit(String),
    /// `0`, `#` or `?`.
    Digit(char),
    Point,
    Comma,
    Percent,
    /// `E+` or `E-`: whether `+` shows on positive exponents.
    Exp(bool),
}

fn tokens(code: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = code.chars().peekable();
    let lit = |out: &mut Vec<Tok>, s: &str| match out.last_mut() {
        Some(Tok::Lit(l)) => l.push_str(s),
        _ => out.push(Tok::Lit(s.into())),
    };
    while let Some(c) = chars.next() {
        match c {
            '0' | '#' | '?' => out.push(Tok::Digit(c)),
            '.' => out.push(Tok::Point),
            ',' => out.push(Tok::Comma),
            '%' => out.push(Tok::Percent),
            'E' | 'e' if matches!(chars.peek(), Some('+' | '-')) => {
                let plus = chars.next() == Some('+');
                out.push(Tok::Exp(plus));
            }
            '"' => {
                let mut s = String::new();
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    s.push(q);
                }
                lit(&mut out, &s);
            }
            '\\' => {
                if let Some(n) = chars.next() {
                    lit(&mut out, &n.to_string());
                }
            }
            '_' => {
                chars.next();
                lit(&mut out, " ");
            }
            '*' => {
                chars.next();
            }
            '@' => {}
            c => lit(&mut out, &c.to_string()),
        }
    }
    out
}

/// A number shown by digit places.
fn digits(n: f64, code: &str) -> String {
    let toks = tokens(code);
    let exp_at = toks.iter().position(|t| matches!(t, Tok::Exp(_)));
    let mantissa_end = exp_at.unwrap_or(toks.len());
    let point_at = toks[..mantissa_end].iter().position(|t| *t == Tok::Point);
    let int_end = point_at.unwrap_or(mantissa_end);
    let is_digit = |t: &Tok| matches!(t, Tok::Digit(_));
    let int_places: Vec<usize> = (0..int_end).filter(|&i| is_digit(&toks[i])).collect();
    let frac_places: Vec<usize> = point_at.map_or(Vec::new(), |p| (p + 1..mantissa_end).filter(|&i| is_digit(&toks[i])).collect());
    let exp_places: Vec<usize> = exp_at.map_or(Vec::new(), |e| (e + 1..toks.len()).filter(|&i| is_digit(&toks[i])).collect());

    // Commas: between integer places they group thousands; after the last
    // digit place, each divides by a thousand.
    let last_place = frac_places.last().or(int_places.last()).copied();
    let mut scale = 0;
    let mut grouped = false;
    for (i, t) in toks[..mantissa_end].iter().enumerate() {
        if *t == Tok::Comma {
            match last_place {
                Some(l) if i > l => scale += 1,
                _ if i < int_end && int_places.first().is_some_and(|f| i > *f) => grouped = true,
                _ => {}
            }
        }
    }
    let percents = toks.iter().filter(|t| **t == Tok::Percent).count() as i32;
    let negative = n < 0.0;
    let mut x = n.abs() * 100f64.powi(percents) / 1000f64.powi(scale);

    let mut exponent = 0i32;
    if exp_at.is_some() && x != 0.0 {
        // Engineering-style when there are several integer places: the
        // exponent a multiple of their count.
        let step = int_places.len().max(1) as i32;
        let e = x.log10().floor() as i32;
        exponent = if step > 1 { e.div_euclid(step) * step } else { e - (int_places.len() as i32 - 1).max(0) };
        x /= 10f64.powi(exponent);
        // Rounding can carry the mantissa to ten: 9.999 → 10.0.
        if round_half_away(x, frac_places.len() as i32) >= 10f64.powi(step.max(int_places.len() as i32)) {
            x /= 10f64.powi(step);
            exponent += step;
        }
    }
    let x = round_half_away(x, frac_places.len() as i32);
    let fixed = format!("{:.*}", frac_places.len(), x);
    let (int_s, frac_s) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let int_digits: Vec<char> = if int_s == "0" { Vec::new() } else { int_s.chars().collect() };

    let mut out_at: Vec<String> = vec![String::new(); toks.len()];
    // Integer places, filled from the right; the leftmost takes any digits
    // left over; thousands separators every three digits shown.
    {
        let mut di = int_digits.len();
        let mut shown = 0;
        let push = |s: &mut String, c: char, shown: &mut usize| {
            if grouped && *shown > 0 && *shown % 3 == 0 {
                s.insert(0, ',');
            }
            s.insert(0, c);
            *shown += 1;
        };
        for (k, &i) in int_places.iter().enumerate().rev() {
            let Tok::Digit(p) = toks[i] else { continue };
            let mut s = String::new();
            if di > 0 {
                di -= 1;
                push(&mut s, int_digits[di], &mut shown);
            } else if p == '0' {
                push(&mut s, '0', &mut shown);
            } else if p == '?' {
                s.insert(0, ' ');
            }
            if k == 0 {
                while di > 0 {
                    di -= 1;
                    push(&mut s, int_digits[di], &mut shown);
                }
            }
            out_at[i] = s;
        }
    }
    // Fraction places: trailing zeros dropped where the place is `#`,
    // spaces where it is `?`.
    {
        let f: Vec<char> = frac_s.chars().collect();
        let mut keep = f.len();
        while keep > 0 {
            let Tok::Digit(p) = toks[frac_places[keep - 1]] else { break };
            if f[keep - 1] != '0' || p == '0' {
                break;
            }
            keep -= 1;
        }
        for (k, &i) in frac_places.iter().enumerate() {
            let Tok::Digit(p) = toks[i] else { continue };
            out_at[i] = if k < keep { f[k].to_string() } else if p == '?' { " ".into() } else { String::new() };
        }
    }
    // The exponent's places.
    if let Some(e) = exp_at {
        let Tok::Exp(plus) = toks[e] else { unreachable!() };
        let mut s = format!("{:0w$}", exponent.abs(), w = exp_places.len().max(1));
        let sign = if exponent < 0 { "-" } else if plus { "+" } else { "" };
        s.insert_str(0, sign);
        out_at[e] = format!("E{s}");
        for &i in &exp_places {
            out_at[i] = String::new();
        }
    }

    let mut out = String::new();
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Lit(s) => out.push_str(s),
            Tok::Point => out.push('.'),
            Tok::Percent => out.push('%'),
            Tok::Comma => {}
            Tok::Digit(_) | Tok::Exp(_) => out.push_str(&out_at[i]),
        }
    }
    // A minus only when something other than zero is shown.
    let nonzero = int_digits.iter().chain(frac_s.chars().collect::<Vec<_>>().iter()).any(|c| *c != '0');
    if negative && nonzero {
        out.insert(0, '-');
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum DTok {
    Lit(String),
    Year(usize),
    /// Month or minute, decided once all are read.
    M(usize),
    Minute(usize),
    Day(usize),
    Hour(usize),
    Second(usize),
    /// `.0`, `.00` after seconds.
    Frac(usize),
    /// `[h]`, `[m]`, `[s]`: elapsed, not the clock's.
    Elapsed(char, usize),
    AmPm,
    /// `A/P`.
    Ap,
}

fn date_tokens(code: &str) -> Vec<DTok> {
    let chars: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let lit = |out: &mut Vec<DTok>, s: &str| match out.last_mut() {
        Some(DTok::Lit(l)) => l.push_str(s),
        _ => out.push(DTok::Lit(s.into())),
    };
    let run = |i: usize, c: char| chars[i..].iter().take_while(|x| x.to_ascii_lowercase() == c).count();
    while i < chars.len() {
        let c = chars[i];
        let lower = c.to_ascii_lowercase();
        let upper_rest: String = chars[i..].iter().collect::<String>().to_ascii_uppercase();
        if upper_rest.starts_with("AM/PM") {
            out.push(DTok::AmPm);
            i += 5;
            continue;
        }
        if upper_rest.starts_with("A/P") {
            out.push(DTok::Ap);
            i += 3;
            continue;
        }
        match lower {
            'y' | 'm' | 'd' | 'h' | 's' => {
                let k = run(i, lower);
                out.push(match lower {
                    'y' => DTok::Year(k),
                    'm' => DTok::M(k),
                    'd' => DTok::Day(k),
                    'h' => DTok::Hour(k),
                    _ => DTok::Second(k),
                });
                i += k;
            }
            '[' => {
                let close = chars[i..].iter().position(|x| *x == ']').map_or(chars.len(), |j| i + j);
                let inner: String = chars[i + 1..close].iter().collect::<String>().to_ascii_lowercase();
                if let Some(u) = inner.chars().next() {
                    out.push(DTok::Elapsed(u, inner.len()));
                }
                i = close + 1;
            }
            '.' if matches!(out.last(), Some(DTok::Second(_))) && chars.get(i + 1) == Some(&'0') => {
                let k = chars[i + 1..].iter().take_while(|x| **x == '0').count();
                out.push(DTok::Frac(k));
                i += 1 + k;
            }
            '"' => {
                let close = chars[i + 1..].iter().position(|x| *x == '"').map_or(chars.len(), |j| i + 1 + j);
                lit(&mut out, &chars[i + 1..close].iter().collect::<String>());
                i = close + 1;
            }
            '\\' => {
                if let Some(n) = chars.get(i + 1) {
                    lit(&mut out, &n.to_string());
                }
                i += 2;
            }
            '_' => {
                lit(&mut out, " ");
                i += 2;
            }
            '*' => i += 2,
            _ => {
                lit(&mut out, &c.to_string());
                i += 1;
            }
        }
    }
    // `m` after an hour or before a second is minutes.
    let n = out.len();
    for k in 0..n {
        if let DTok::M(w) = out[k] {
            let before = out[..k].iter().rev().find(|t| !matches!(t, DTok::Lit(_)));
            let after = out[k + 1..].iter().find(|t| !matches!(t, DTok::Lit(_)));
            if matches!(before, Some(DTok::Hour(_) | DTok::Elapsed('h', _))) || matches!(after, Some(DTok::Second(_))) {
                out[k] = DTok::Minute(w);
            }
        }
    }
    out
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn date(n: f64, code: &str) -> String {
    let toks = date_tokens(code);
    let frac_digits = toks.iter().find_map(|t| if let DTok::Frac(k) = t { Some(*k) } else { None }).unwrap_or(0);
    // Rounded to what is shown, so 23:59:59.7 is the next day's 00:00:00.
    let unit = 86_400.0 * 10f64.powi(frac_digits as i32);
    let ticks = (n * unit).round();
    let day_n = (ticks / unit).floor();
    let in_day = ticks - day_n * unit;
    let Some((y, mo, d)) = ymd(day_n) else { return "#".repeat(8) };
    let secs_total = (in_day / 10f64.powi(frac_digits as i32)).floor() as u64;
    let sub = (in_day as u64) % 10u64.pow(frac_digits as u32);
    let (h, mi, s) = (secs_total / 3600, secs_total / 60 % 60, secs_total % 60);
    let twelve = toks.iter().any(|t| matches!(t, DTok::AmPm | DTok::Ap));
    let wd = crate::dates::weekday(day_n) as usize;
    let elapsed_secs = n.abs() * 86_400.0;
    let mut out = String::new();
    for t in &toks {
        match t {
            DTok::Lit(s) => out.push_str(s),
            DTok::Year(k) => out.push_str(&if *k <= 2 { format!("{:02}", y % 100) } else { format!("{y:04}") }),
            DTok::M(k) => out.push_str(&match k {
                1 => mo.to_string(),
                2 => format!("{mo:02}"),
                3 => MONTHS[mo as usize - 1][..3].to_string(),
                4 => MONTHS[mo as usize - 1].to_string(),
                _ => MONTHS[mo as usize - 1][..1].to_string(),
            }),
            DTok::Day(k) => out.push_str(&match k {
                1 => d.to_string(),
                2 => format!("{d:02}"),
                3 => DAYS[wd][..3].to_string(),
                _ => DAYS[wd].to_string(),
            }),
            DTok::Hour(k) => {
                let hh = if twelve { (h + 11) % 12 + 1 } else { h };
                out.push_str(&if *k >= 2 { format!("{hh:02}") } else { hh.to_string() });
            }
            DTok::Minute(k) => out.push_str(&if *k >= 2 { format!("{mi:02}") } else { mi.to_string() }),
            DTok::Second(k) => out.push_str(&if *k >= 2 { format!("{s:02}") } else { s.to_string() }),
            DTok::Frac(k) => out.push_str(&format!(".{sub:0k$}")),
            DTok::Elapsed(u, k) => {
                let v = match u {
                    'h' => (elapsed_secs / 3600.0).floor(),
                    'm' => (elapsed_secs / 60.0).floor(),
                    _ => elapsed_secs.round(),
                } as u64;
                out.push_str(&format!("{v:0k$}"));
            }
            DTok::AmPm => out.push_str(if h < 12 { "AM" } else { "PM" }),
            DTok::Ap => out.push_str(if h < 12 { "A" } else { "P" }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: f64, code: &str) -> String {
        show(&Value::Number(n), code).text
    }

    #[test]
    fn numbers_show_as_excel_shows_them() {
        for (n, code, want) in [
            (1234.567, "", "1234.567"),
            (1234.567, "General", "1234.567"),
            (1234.567, "0", "1235"),
            (1234.567, "0.00", "1234.57"),
            (1234.567, "#,##0", "1,235"),
            (1234567.891, "#,##0.00", "1,234,567.89"),
            (0.5, "#.##", ".5"),
            (0.5, "0.##", "0.5"),
            (5.0, "#.##", "5."),
            (2.5, "0.0?", "2.5 "),
            (7.0, "000", "007"),
            (0.256, "0%", "26%"),
            (0.256, "0.0%", "25.6%"),
            (12345.0, "0.00E+00", "1.23E+04"),
            (0.000123, "0.00E+00", "1.23E-04"),
            (12345.0, "##0.0E+0", "12.3E+3"),
            (1500000.0, "0.0,,\"M\"", "1.5M"),
            (1500.0, "#,##0,\"k\"", "2k"),
            (-5.0, "0", "-5"),
            (-5.0, "0;(0)", "(5)"),
            (0.0, "0;-0;\"zero\"", "zero"),
            (-0.001, "0.00", "0.00"),
            (1234.5, "$#,##0.00", "$1,234.50"),
            (1234.5, "[$€-407]#,##0.00", "€1,234.50"),
            (5.0, "\"Total: \"0", "Total: 5"),
            (5.0, "0\\ \\k\\g", "5 kg"),
            (5551234.0, "000-0000", "555-1234"),
            (2.675, "0.00", "2.68"),
            (999.96, "0.0", "1000.0"),
            (5.0, "General\" units\"", "5 units"),
        ] {
            assert_eq!(s(n, code), want, "{n} as {code:?}");
        }
        assert_eq!(show(&Value::Number(-5.0), "0;[Red]-0").color, Some("#ff0000"));
        assert_eq!(show(&Value::text("x"), "0;0;0;\"<\"@\">\"").text, "<x>");
        assert_eq!(show(&Value::text("x"), "0.00").text, "x");
        assert_eq!(show(&Value::Bool(true), "0.00").text, "TRUE");
    }

    #[test]
    fn dates_and_times_show_as_excel_shows_them() {
        let at = 45366.75; // Friday 15 March 2024, 18:00
        for (code, want) in [
            ("yyyy-mm-dd", "2024-03-15"),
            ("m/d/yy", "3/15/24"),
            ("d mmm yyyy", "15 Mar 2024"),
            ("dddd, mmmm d", "Friday, March 15"),
            ("ddd", "Fri"),
            ("mmmmm", "M"),
            ("hh:mm", "18:00"),
            ("h:mm AM/PM", "6:00 PM"),
            ("h:mm:ss A/P", "6:00:00 P"),
            ("yyyy-mm-dd hh:mm:ss", "2024-03-15 18:00:00"),
            ("[h]:mm", "1088802:00"),
        ] {
            assert_eq!(s(at, code), want, "{code}");
        }
        assert_eq!(s(1.5, "[h]:mm"), "36:00");
        assert_eq!(s(0.999999, "hh:mm:ss"), "00:00:00", "rounds to the next day");
        assert_eq!(s(0.5 + 1.5 / 86_400.0, "mm:ss.0"), "00:01.5");
    }
}
