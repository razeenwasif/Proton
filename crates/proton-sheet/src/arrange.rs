//! Sorting, filters and conditional colour: what a sheet's rows are in
//! order, which are shown, and what colour a cell's value gives it.
//!
//! None of these change what a cell holds except sorting, which moves rows
//! whole — formulas in them moved as copying moves them, so `=A7*2` in a
//! row sorted to the top reads `=A2*2`. A filter hides rows and a rule
//! colours cells as they are drawn; both are worked out from the values
//! each time, so they follow the cells as those change.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::book::Book;
use crate::edit::moved_input;
use crate::functions::Criteria;
use crate::model::{Cell, Rule, When};
use crate::numfmt;
use crate::refs::{col_of, CellRef, Range};
use crate::value::Value;

/// A range's rows reordered by some of its columns, each ascending or not,
/// the first deciding and the rest breaking ties; the order kept among
/// equals. Empty values go last whichever way, as Excel puts them. What
/// the range's cells become, every one of them.
pub fn sort(book: &Book, sheet: usize, range: Range, keys: &[(u32, bool)]) -> Vec<(CellRef, Cell)> {
    let Some(s) = book.workbook().sheets.get(sheet) else { return Vec::new() };
    let rows: Vec<u32> = (range.start.row..=range.end.row).collect();
    let mut order: Vec<u32> = rows.clone();
    order.sort_by(|a, b| {
        for &(col, up) in keys {
            let (x, y) = (book.value(sheet, CellRef::new(*a, col)), book.value(sheet, CellRef::new(*b, col)));
            let o = match (x.is_empty(), y.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                _ if up => x.compare(&y),
                _ => y.compare(&x),
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
    let mut out = Vec::new();
    for (to, from) in rows.iter().zip(&order) {
        for col in range.start.col..=range.end.col {
            let cell = s.cells.get(&CellRef::new(*from, col)).cloned().unwrap_or_default();
            let input = moved_input(&cell.input, *to as i64 - *from as i64, 0);
            out.push((CellRef::new(*to, col), Cell { input, style: cell.style }));
        }
    }
    out
}

/// A cell's value as it shows: what a filter's ticks name.
pub fn shown(book: &Book, sheet: usize, at: CellRef) -> String {
    let wb = book.workbook();
    let style = wb.sheets.get(sheet).and_then(|s| s.cells.get(&at)).map_or(0, |c| c.style);
    let format = wb.styles.get(style as usize).map_or("", |s| s.format.as_str());
    numfmt::show(&book.value(sheet, at), format).text
}

/// `A1:D99`, or a column's letters, read; a range cut to the sheet's last
/// row in use, so a filter over whole columns is over the rows there are.
pub fn range_in(book: &Book, sheet: usize, text: &str) -> Option<Range> {
    let text = text.trim();
    let r = Range::parse(text).or_else(|| {
        let (a, b) = text.split_once(':')?;
        crate::book::resolve(&crate::refs::A1::parse(a)?, &crate::refs::A1::parse(b)?)
    })?;
    let used = book.workbook().sheets.get(sheet)?.used();
    let end = used.map_or(r.start.row, |u| u.end.row.max(r.start.row)).min(r.end.row);
    Some(Range::new(r.start, CellRef::new(end, r.end.col)))
}

/// The rows a sheet's filter hides, as runs `[first, last]`.
pub fn hidden(book: &Book, sheet: usize) -> Vec<[u32; 2]> {
    let Some(f) = book.workbook().sheets.get(sheet).and_then(|s| s.filter.as_ref()) else { return Vec::new() };
    let Some(range) = range_in(book, sheet, &f.range) else { return Vec::new() };
    let tests: Vec<(u32, Option<HashSet<String>>, Option<Criteria>)> = f
        .columns
        .iter()
        .filter_map(|(letters, c)| {
            let col = col_of(letters)?;
            let values = c.values.as_ref().map(|v| v.iter().map(|s| s.to_lowercase()).collect());
            let criteria = (!c.criteria.trim().is_empty()).then(|| Criteria::new(&Value::text(c.criteria.trim())));
            Some((col, values, criteria))
        })
        .collect();
    let mut runs: Vec<[u32; 2]> = Vec::new();
    // The first row is the headers', always shown.
    for row in range.start.row + 1..=range.end.row {
        let keep = tests.iter().all(|(col, values, criteria)| {
            let at = CellRef::new(row, *col);
            values.as_ref().is_none_or(|v| v.contains(&shown(book, sheet, at).to_lowercase()))
                && criteria.as_ref().is_none_or(|c| c.matches(&book.value(sheet, at)))
        });
        if !keep {
            match runs.last_mut() {
                Some(run) if run[1] + 1 == row => run[1] = row,
                _ => runs.push([row, row]),
            }
        }
    }
    runs
}

/// The values a filtered column has under its header, as they show, each
/// with how many rows have it — the filter's list to tick.
pub fn distinct(book: &Book, sheet: usize, range: Range, col: u32) -> Vec<(String, u32)> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for row in range.start.row + 1..=range.end.row {
        let text = shown(book, sheet, CellRef::new(row, col));
        let n = counts.entry(text.clone()).or_insert(0);
        if *n == 0 {
            order.push(text);
        }
        *n += 1;
    }
    order.sort_by(|a, b| {
        let (x, y) = (crate::book::literal(a), crate::book::literal(b));
        match (a.is_empty(), b.is_empty()) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => x.compare(&y),
        }
    });
    order.into_iter().map(|t| (t.clone(), counts[&t])).collect()
}

/// What a rule does to a cell it applies to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Paint {
    pub fill: Option<String>,
    pub color: Option<String>,
    pub bold: bool,
}

/// A sheet's rules, with what each needs of its whole range worked out
/// once: the duplicates, the top and bottom thresholds, a scale's ends.
pub struct Painter {
    rules: Vec<(Rule, Range, Prepared)>,
}

enum Prepared {
    Plain,
    Duplicates(HashSet<String>),
    /// The value a number must reach (top) or not pass (bottom).
    Threshold(f64),
    Scale(f64, f64, [u8; 3], [u8; 3]),
    Never,
}

fn key(v: &Value) -> Option<String> {
    match v {
        Value::Empty => None,
        Value::Text(t) => Some(format!("t{}", t.to_lowercase())),
        Value::Number(n) => Some(format!("n{n}")),
        Value::Bool(b) => Some(format!("b{b}")),
        Value::Error(e) => Some(format!("e{}", e.text())),
    }
}

fn rgb(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

impl Painter {
    pub fn new(book: &Book, sheet: usize) -> Painter {
        let Some(s) = book.workbook().sheets.get(sheet) else { return Painter { rules: Vec::new() } };
        let mut rules = Vec::new();
        for rule in &s.rules {
            let Some(range) = range_in(book, sheet, &rule.range) else { continue };
            let values = || {
                let mut out = Vec::new();
                for r in range.start.row..=range.end.row {
                    for c in range.start.col..=range.end.col {
                        out.push(book.value(sheet, CellRef::new(r, c)));
                    }
                }
                out
            };
            let numbers = || values().into_iter().filter_map(|v| if let Value::Number(n) = v { Some(n) } else { None }).collect::<Vec<f64>>();
            let prepared = match &rule.when {
                When::Duplicates => {
                    let mut seen = HashSet::new();
                    let mut twice = HashSet::new();
                    for v in values() {
                        if let Some(k) = key(&v) {
                            if !seen.insert(k.clone()) {
                                twice.insert(k);
                            }
                        }
                    }
                    Prepared::Duplicates(twice)
                }
                When::Top { n } | When::Bottom { n } => {
                    let mut v = numbers();
                    if v.is_empty() || *n == 0 {
                        Prepared::Never
                    } else {
                        v.sort_by(f64::total_cmp);
                        let k = (*n as usize).min(v.len());
                        Prepared::Threshold(if matches!(rule.when, When::Top { .. }) { v[v.len() - k] } else { v[k - 1] })
                    }
                }
                When::Scale { low, high } => {
                    let v = numbers();
                    match (v.iter().copied().reduce(f64::min), v.iter().copied().reduce(f64::max), rgb(low), rgb(high)) {
                        (Some(a), Some(b), Some(lo), Some(hi)) => Prepared::Scale(a, b, lo, hi),
                        _ => Prepared::Never,
                    }
                }
                _ => Prepared::Plain,
            };
            rules.push((rule.clone(), range, prepared));
        }
        Painter { rules }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// What the first rule that applies to a cell does to it.
    pub fn paint(&self, at: CellRef, v: &Value) -> Option<Paint> {
        for (rule, range, prepared) in &self.rules {
            if !range.contains(at) || v.is_empty() {
                continue;
            }
            let n = if let Value::Number(n) = v { Some(*n) } else { None };
            let hit = match (&rule.when, prepared) {
                (_, Prepared::Never) => false,
                (When::Greater { value }, _) => n.is_some_and(|n| n > *value),
                (When::Less { value }, _) => n.is_some_and(|n| n < *value),
                (When::Between { low, high }, _) => n.is_some_and(|n| n >= low.min(*high) && n <= high.max(*low)),
                (When::Equal { value }, _) => v.compare(&crate::book::literal(value)) == Ordering::Equal,
                (When::Contains { text }, _) => v.to_text().is_ok_and(|t| t.to_lowercase().contains(&text.to_lowercase())),
                (When::Duplicates, Prepared::Duplicates(twice)) => key(v).is_some_and(|k| twice.contains(&k)),
                (When::Top { .. }, Prepared::Threshold(t)) => n.is_some_and(|n| n >= *t),
                (When::Bottom { .. }, Prepared::Threshold(t)) => n.is_some_and(|n| n <= *t),
                (When::Scale { .. }, Prepared::Scale(a, b, lo, hi)) => {
                    let Some(n) = n else { continue };
                    let t = if b > a { ((n - a) / (b - a)).clamp(0.0, 1.0) } else { 0.5 };
                    let mix = |i: usize| (lo[i] as f64 + (hi[i] as f64 - lo[i] as f64) * t).round() as u8;
                    return Some(Paint { fill: Some(format!("#{:02x}{:02x}{:02x}", mix(0), mix(1), mix(2))), color: rule.color.clone(), bold: rule.bold });
                }
                _ => false,
            };
            if hit {
                return Some(Paint { fill: rule.fill.clone(), color: rule.color.clone(), bold: rule.bold });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Condition, Filter, Workbook};

    fn book(cells: &[(&str, &str)]) -> Book {
        let mut wb = Workbook::default();
        for (at, input) in cells {
            wb.sheets[0].cells.insert(CellRef::parse(at).unwrap(), Cell { input: input.to_string(), style: 0 });
        }
        Book::new(wb)
    }

    const PEOPLE: [(&str, &str); 12] = [
        ("A1", "Name"), ("B1", "Age"), ("C1", "Double"),
        ("A2", "Cyd"), ("B2", "41"), ("C2", "=B2*2"),
        ("A3", "ada"), ("B3", "36"), ("C3", "=B3*2"),
        ("A4", "Bea"), ("B4", ""), ("C4", "=B4*2"),
    ];

    #[test]
    fn sorting_moves_rows_whole_and_their_formulas_with_them() {
        let b = book(&PEOPLE);
        let r = Range::parse("A2:C4").unwrap();
        let by_name: Vec<(String, String)> = sort(&b, 0, r, &[(0, true)]).into_iter().map(|(at, c)| (at.name(), c.input)).collect();
        assert_eq!(&by_name[..3], &[("A2".into(), "ada".into()), ("B2".into(), "36".into()), ("C2".into(), "=B2*2".into())], "without regard to case");
        assert_eq!(by_name[6], ("A4".into(), "Cyd".into()));
        // By age, highest first: the empty one last whichever way.
        let by_age: Vec<String> = sort(&b, 0, r, &[(1, false)]).into_iter().filter(|(at, _)| at.col == 0).map(|(_, c)| c.input).collect();
        assert_eq!(by_age, vec!["Cyd", "ada", "Bea"]);
        let up: Vec<String> = sort(&b, 0, r, &[(1, true)]).into_iter().filter(|(at, _)| at.col == 0).map(|(_, c)| c.input).collect();
        assert_eq!(up, vec!["ada", "Cyd", "Bea"]);
    }

    #[test]
    fn a_filter_hides_rows_by_ticked_values_and_by_condition() {
        let mut b = book(&[("A1", "Fruit"), ("B1", "Kg"), ("A2", "apple"), ("B2", "4"), ("A3", "pear"), ("B3", "12"), ("A4", "Apple"), ("B4", "20"), ("A5", "fig"), ("B5", "1")]);
        b.workbook_mut().sheets[0].filter = Some(Filter {
            range: "A1:B5".into(),
            columns: [("A".to_string(), Condition { values: Some(vec!["apple".into(), "fig".into()]), criteria: String::new() })].into(),
        });
        assert_eq!(hidden(&b, 0), vec![[2, 2]], "pear goes; Apple matches apple");
        b.workbook_mut().sheets[0].filter.as_mut().unwrap().columns.insert("B".into(), Condition { values: None, criteria: ">2".into() });
        assert_eq!(hidden(&b, 0), vec![[2, 2], [4, 4]]);
        let d = distinct(&b, 0, Range::parse("A1:B5").unwrap(), 1);
        assert_eq!(d, vec![("1".to_string(), 1), ("4".to_string(), 1), ("12".to_string(), 1), ("20".to_string(), 1)], "numbers in order");
    }

    #[test]
    fn rules_colour_what_they_say_and_the_first_wins() {
        let mut b = book(&[("A1", "5"), ("A2", "15"), ("A3", "25"), ("A4", "15"), ("B1", "pear tree"), ("B2", "x")]);
        let rule = |range: &str, when: When, fill: &str| Rule { range: range.into(), when, fill: Some(fill.into()), color: None, bold: false };
        b.workbook_mut().sheets[0].rules = vec![
            rule("A1:A4", When::Top { n: 1 }, "#00ff00"),
            rule("A1:A4", When::Duplicates, "#ff0000"),
            rule("A1:A4", When::Less { value: 10.0 }, "#0000ff"),
            rule("B:B", When::Contains { text: "TREE".into() }, "#123456"),
        ];
        let p = Painter::new(&b, 0);
        let fill = |at: &str| p.paint(CellRef::parse(at).unwrap(), &b.value(0, CellRef::parse(at).unwrap())).and_then(|x| x.fill);
        assert_eq!(fill("A3").as_deref(), Some("#00ff00"));
        assert_eq!(fill("A2").as_deref(), Some("#ff0000"));
        assert_eq!(fill("A1").as_deref(), Some("#0000ff"));
        assert_eq!(fill("B1").as_deref(), Some("#123456"));
        assert_eq!(fill("B2"), None);
        b.workbook_mut().sheets[0].rules = vec![Rule { range: "A1:A3".into(), when: When::Scale { low: "#000000".into(), high: "#ffffff".into() }, fill: None, color: None, bold: false }];
        let p = Painter::new(&b, 0);
        let fill = |at: &str| p.paint(CellRef::parse(at).unwrap(), &b.value(0, CellRef::parse(at).unwrap())).and_then(|x| x.fill);
        assert_eq!((fill("A1").as_deref(), fill("A2").as_deref(), fill("A3").as_deref()), (Some("#000000"), Some("#808080"), Some("#ffffff")));
    }
}

/// A chart's data: its categories (or x values) and its series, each
/// with its name and a value per category, none where a cell is not a
/// number. At most `MOST_POINTS` categories.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartData {
    pub categories: Vec<String>,
    pub xs: Vec<Option<f64>>,
    pub series: Vec<(String, Vec<Option<f64>>)>,
}

pub const MOST_POINTS: usize = 5000;

/// A chart's range, whether its first row names the series, and whether
/// its first column gives the categories.
pub fn chart_shape(book: &Book, sheet: usize, range: &str) -> Option<(Range, bool, bool)> {
    let r = range_in(book, sheet, range)?;
    let labelled = r.cols() > 1;
    let header = (r.start.col + u32::from(labelled)..=r.end.col).any(|c| matches!(book.value(sheet, CellRef::new(r.start.row, c)), Value::Text(_)));
    Some((r, header, labelled))
}

/// What a chart over `range` draws. The first row names the series when
/// any cell after the first in it is text; the first column gives the
/// categories when the range is wider than one column.
pub fn chart_data(book: &Book, sheet: usize, range: &str) -> Option<ChartData> {
    let (r, header, labelled) = chart_shape(book, sheet, range)?;
    let first_row = r.start.row + u32::from(header);
    let value_cols: Vec<u32> = (r.start.col + u32::from(labelled)..=r.end.col).collect();
    let rows: Vec<u32> = (first_row..=r.end.row).take(MOST_POINTS).collect();
    let num = |v: Value| if let Value::Number(n) = v { Some(n) } else { None };
    let categories = rows
        .iter()
        .map(|row| if labelled { shown(book, sheet, CellRef::new(*row, r.start.col)) } else { (row - first_row + 1).to_string() })
        .collect();
    let xs = rows.iter().map(|row| if labelled { num(book.value(sheet, CellRef::new(*row, r.start.col))) } else { Some((row - first_row + 1) as f64) }).collect();
    let series = value_cols
        .iter()
        .map(|c| {
            let name = if header { shown(book, sheet, CellRef::new(r.start.row, *c)) } else { format!("Series {}", c - value_cols[0] + 1) };
            (name, rows.iter().map(|row| num(book.value(sheet, CellRef::new(*row, *c)))).collect())
        })
        .collect();
    Some(ChartData { categories, xs, series })
}

#[cfg(test)]
mod chart_tests {
    use super::*;
    use crate::model::Workbook;

    #[test]
    fn a_chart_reads_its_names_categories_and_values() {
        let mut wb = Workbook::default();
        for (at, v) in [("A1", "Month"), ("B1", "Rent"), ("C1", "Food"), ("A2", "Jan"), ("B2", "1450"), ("C2", "300"), ("A3", "Feb"), ("B3", "1450"), ("C3", "x")] {
            wb.sheets[0].cells.insert(CellRef::parse(at).unwrap(), Cell { input: v.into(), style: 0 });
        }
        let b = Book::new(wb);
        let d = chart_data(&b, 0, "A1:C3").unwrap();
        assert_eq!(d.categories, vec!["Jan", "Feb"]);
        assert_eq!(d.series, vec![("Rent".to_string(), vec![Some(1450.0), Some(1450.0)]), ("Food".to_string(), vec![Some(300.0), None])]);
        let one = chart_data(&b, 0, "B2:B3").unwrap();
        assert_eq!((one.categories.clone(), one.series[0].0.clone()), (vec!["1".to_string(), "2".to_string()], "Series 1".to_string()));
    }
}
