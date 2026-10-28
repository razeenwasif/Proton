//! Changes to a sheet's shape and to many cells at once, as Excel makes
//! them: rows and columns inserted and deleted with every formula that
//! points across them following — on every sheet, and in the names — and
//! a reference into what was deleted becoming #REF!; and cells copied or
//! filled with their formulas moved as far as they went.

use std::collections::BTreeMap;

use crate::formula::{self, Expr};
use crate::model::{Cell, Workbook};
use crate::refs::{CellRef, Range, A1, MAX_COL, MAX_ROW};
use crate::value::ErrorKind;

/// Rows or columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Rows,
    Cols,
}

/// Where a row or column goes after a change: moved, or gone.
#[derive(Debug, Clone, Copy)]
enum Change {
    /// `n` inserted before `at`.
    Insert { at: u32, n: u32 },
    /// `n` deleted from `at`.
    Delete { at: u32, n: u32 },
}

impl Change {
    fn point(self, i: u32) -> Option<u32> {
        match self {
            Change::Insert { at, n } => Some(if i >= at { i + n } else { i }),
            Change::Delete { at, n } => {
                if i < at {
                    Some(i)
                } else if i < at + n {
                    None
                } else {
                    Some(i - n)
                }
            }
        }
    }

    /// A span `[a, b]`, which grows when rows go in inside it and shrinks
    /// when some of it is deleted; gone when all of it is.
    fn span(self, a: u32, b: u32) -> Option<(u32, u32)> {
        match self {
            Change::Insert { at, n } => Some((if a >= at { a + n } else { a }, if b >= at { b + n } else { b })),
            Change::Delete { at, n } => {
                let end = at + n;
                if a >= at && b < end {
                    return None;
                }
                let a2 = if a < at { a } else if a < end { at } else { a - n };
                let b2 = if b < at { b } else if b < end { at - 1 } else { b - n };
                Some((a2, b2))
            }
        }
    }
}

/// A formula's references to `target` after a change there. `home` is the
/// sheet the formula is on; `names` the sheets' names, to know which a
/// sheet-qualified reference means.
fn adjust(e: &Expr, home: usize, target: usize, names: &[String], axis: Axis, change: Change) -> Expr {
    let points_here = |sheet: &Option<String>| match sheet {
        None => home == target,
        Some(n) => names.get(target).is_some_and(|t| t.eq_ignore_ascii_case(n)),
    };
    let a = |e: &Expr| Box::new(adjust(e, home, target, names, axis, change));
    match e {
        Expr::Ref { sheet, at } if points_here(sheet) => {
            let moved = match axis {
                Axis::Rows => match at.row {
                    Some((r, abs)) => change.point(r).map(|r| A1 { row: Some((r, abs)), ..*at }),
                    None => Some(*at),
                },
                Axis::Cols => match at.col {
                    Some((c, abs)) => change.point(c).map(|c| A1 { col: Some((c, abs)), ..*at }),
                    None => Some(*at),
                },
            };
            match moved {
                Some(at) if fits(&at) => Expr::Ref { sheet: sheet.clone(), at },
                _ => Expr::Error(ErrorKind::Ref),
            }
        }
        Expr::Range { sheet, start, end } if points_here(sheet) => {
            let pick = |x: &A1| match axis {
                Axis::Rows => x.row,
                Axis::Cols => x.col,
            };
            let (Some((s, sa)), Some((t, ta))) = (pick(start), pick(end)) else {
                // Whole columns when rows change, or whole rows when columns
                // do: untouched.
                return e.clone();
            };
            match change.span(s.min(t), s.max(t)) {
                Some((lo, hi)) => {
                    let put = |x: &A1, v: u32, abs: bool| match axis {
                        Axis::Rows => A1 { row: Some((v, abs)), ..*x },
                        Axis::Cols => A1 { col: Some((v, abs)), ..*x },
                    };
                    let (start, end) = (put(start, lo, sa), put(end, hi, ta));
                    if fits(&start) && fits(&end) {
                        Expr::Range { sheet: sheet.clone(), start, end }
                    } else {
                        Expr::Error(ErrorKind::Ref)
                    }
                }
                None => Expr::Error(ErrorKind::Ref),
            }
        }
        Expr::Neg(x) => Expr::Neg(a(x)),
        Expr::Plus(x) => Expr::Plus(a(x)),
        Expr::Percent(x) => Expr::Percent(a(x)),
        Expr::Binary(op, x, y) => Expr::Binary(*op, a(x), a(y)),
        Expr::RangeOp(x, y) => Expr::RangeOp(a(x), a(y)),
        Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(|x| adjust(x, home, target, names, axis, change)).collect()),
        Expr::Array(rows) => Expr::Array(rows.iter().map(|r| r.iter().map(|x| adjust(x, home, target, names, axis, change)).collect()).collect()),
        other => other.clone(),
    }
}

fn fits(a: &A1) -> bool {
    a.row.is_none_or(|(r, _)| r <= MAX_ROW) && a.col.is_none_or(|(c, _)| c <= MAX_COL)
}

/// Every formula in the workbook, and every name, after `change` on `target`.
fn adjust_all(wb: &mut Workbook, target: usize, axis: Axis, change: Change) {
    let names: Vec<String> = wb.sheets.iter().map(|s| s.name.clone()).collect();
    for (home, sheet) in wb.sheets.iter_mut().enumerate() {
        for cell in sheet.cells.values_mut() {
            if let Some(src) = cell.input.strip_prefix('=') {
                if let Ok(e) = formula::parse(src) {
                    let moved = adjust(&e, home, target, &names, axis, change);
                    if moved != e {
                        cell.input = format!("={}", moved.write());
                    }
                }
            }
        }
    }
    for def in wb.names.values_mut() {
        if let Ok(e) = formula::parse(def.trim_start_matches('=')) {
            // A name is nobody's sheet: only qualified references in it count.
            let moved = adjust(&e, usize::MAX, target, &names, axis, change);
            if moved != e {
                *def = moved.write();
            }
        }
    }
}

/// The cells of a sheet moved as a change moves their rows or columns,
/// those in what was deleted gone.
fn move_cells(cells: &BTreeMap<CellRef, Cell>, axis: Axis, change: Change) -> BTreeMap<CellRef, Cell> {
    let mut out = BTreeMap::new();
    for (at, cell) in cells {
        let moved = match axis {
            Axis::Rows => change.point(at.row).filter(|r| *r <= MAX_ROW).map(|r| CellRef::new(r, at.col)),
            Axis::Cols => change.point(at.col).filter(|c| *c <= MAX_COL).map(|c| CellRef::new(at.row, c)),
        };
        if let Some(to) = moved {
            out.insert(to, cell.clone());
        }
    }
    out
}

fn apply(wb: &mut Workbook, sheet: usize, axis: Axis, change: Change) {
    adjust_all(wb, sheet, axis, change);
    let Some(s) = wb.sheets.get_mut(sheet) else { return };
    s.cells = move_cells(&s.cells, axis, change);
    // A filter and the conditional colours keep to the cells they were over.
    if let Some(f) = &mut s.filter {
        match moved_range(&f.range, axis, change) {
            Some(r) => {
                f.range = r;
                if axis == Axis::Cols {
                    f.columns = std::mem::take(&mut f.columns)
                        .into_iter()
                        .filter_map(|(letters, c)| Some((crate::refs::col_name(change.point(crate::refs::col_of(&letters)?)?), c)))
                        .collect();
                }
            }
            None => s.filter = None,
        }
    }
    s.rules.retain_mut(|rule| match moved_range(&rule.range, axis, change) {
        Some(r) => {
            rule.range = r;
            true
        }
        None => false,
    });
    if axis == Axis::Cols {
        s.widths = s.widths.iter().filter_map(|(c, w)| change.point(*c).map(|c| (c, *w))).collect();
    }
    // Frozen panes keep their place unless what was frozen went.
    let frozen = match axis {
        Axis::Rows => &mut s.frozen.rows,
        Axis::Cols => &mut s.frozen.cols,
    };
    if let Change::Delete { at, n } = change {
        if at < *frozen {
            *frozen -= n.min(*frozen - at);
        }
    } else if let Change::Insert { at, n } = change {
        if at < *frozen {
            *frozen += n;
        }
    }
}

/// A range written `A1:D9` after a change; whole columns and rows, which a
/// change across them leaves whole, as they were; none when all of it went.
fn moved_range(text: &str, axis: Axis, change: Change) -> Option<String> {
    let Some(r) = Range::parse(text) else { return Some(text.to_string()) };
    let (a, b) = match axis {
        Axis::Rows => (r.start.row, r.end.row),
        Axis::Cols => (r.start.col, r.end.col),
    };
    let (a, b) = change.span(a, b)?;
    let r = match axis {
        Axis::Rows => Range::new(CellRef::new(a, r.start.col), CellRef::new(b, r.end.col)),
        Axis::Cols => Range::new(CellRef::new(r.start.row, a), CellRef::new(r.end.row, b)),
    };
    Some(r.name())
}

/// `n` rows or columns put in before `at`.
pub fn insert(wb: &mut Workbook, sheet: usize, axis: Axis, at: u32, n: u32) {
    if n > 0 {
        apply(wb, sheet, axis, Change::Insert { at, n });
    }
}

/// `n` rows or columns from `at` taken away.
pub fn delete(wb: &mut Workbook, sheet: usize, axis: Axis, at: u32, n: u32) {
    if n > 0 {
        apply(wb, sheet, axis, Change::Delete { at, n });
    }
}

/// What a cell's input becomes copied `rows` down and `cols` across: a
/// formula's relative references moved with it, anything else as it was.
pub fn moved_input(input: &str, rows: i64, cols: i64) -> String {
    match input.strip_prefix('=').map(formula::parse) {
        Some(Ok(e)) if rows != 0 || cols != 0 => format!("={}", e.shifted(rows, cols).write()),
        _ => input.to_string(),
    }
}

/// Cells to fill `to` from `from`, which it starts with, as the fill handle
/// fills: down or across, a run of numbers continued as a series where the
/// source has two or more in that direction, everything else repeated with
/// formulas moved. The cells of `from` are not among those returned.
pub fn fill(cells: &BTreeMap<CellRef, Cell>, from: Range, to: Range) -> Vec<(CellRef, Cell)> {
    let down = to.rows() > from.rows() || to.end.row > from.end.row || to.start.row < from.start.row;
    let mut out = Vec::new();
    let get = |r: u32, c: u32| cells.get(&CellRef::new(r, c)).cloned().unwrap_or_default();
    let number = |cell: &Cell| -> Option<f64> {
        if cell.input.starts_with('=') {
            return None;
        }
        match crate::book::literal(&cell.input) {
            crate::value::Value::Number(n) => Some(n),
            _ => None,
        }
    };
    if down {
        let h = from.rows();
        for c in to.start.col..=to.end.col {
            let src: Vec<Cell> = (from.start.row..=from.end.row).map(|r| get(r, c)).collect();
            let nums: Option<Vec<f64>> = (h >= 2).then(|| src.iter().map(number).collect()).flatten();
            let step = nums.as_ref().map(|v| (v[v.len() - 1] - v[0]) / (v.len() - 1) as f64);
            for r in to.start.row..=to.end.row {
                if (from.start.row..=from.end.row).contains(&r) {
                    continue;
                }
                let k = (r as i64 - from.start.row as i64).rem_euclid(h as i64) as usize;
                let base = &src[k];
                let input = match (&nums, step) {
                    (Some(v), Some(step)) => crate::value::general(v[0] + step * (r as f64 - from.start.row as f64)),
                    _ => moved_input(&base.input, r as i64 - (from.start.row as i64 + k as i64), 0),
                };
                out.push((CellRef::new(r, c), Cell { input, style: base.style }));
            }
        }
    } else {
        let w = from.cols();
        for r in to.start.row..=to.end.row {
            let src: Vec<Cell> = (from.start.col..=from.end.col).map(|c| get(r, c)).collect();
            let nums: Option<Vec<f64>> = (w >= 2).then(|| src.iter().map(number).collect()).flatten();
            let step = nums.as_ref().map(|v| (v[v.len() - 1] - v[0]) / (v.len() - 1) as f64);
            for c in to.start.col..=to.end.col {
                if (from.start.col..=from.end.col).contains(&c) {
                    continue;
                }
                let k = (c as i64 - from.start.col as i64).rem_euclid(w as i64) as usize;
                let base = &src[k];
                let input = match (&nums, step) {
                    (Some(v), Some(step)) => crate::value::general(v[0] + step * (c as f64 - from.start.col as f64)),
                    _ => moved_input(&base.input, 0, c as i64 - (from.start.col as i64 + k as i64)),
                };
                out.push((CellRef::new(r, c), Cell { input, style: base.style }));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Sheet;

    fn wb(cells: &[(&str, &str)]) -> Workbook {
        let mut wb = Workbook::default();
        wb.sheets.push(Sheet::new("s2", "Other"));
        for (at, input) in cells {
            let (sheet, at) = match at.split_once('!') {
                Some((_, a)) => (1, a),
                None => (0, *at),
            };
            wb.sheets[sheet].cells.insert(CellRef::parse(at).unwrap(), Cell { input: input.to_string(), style: 0 });
        }
        wb
    }

    fn input(wb: &Workbook, sheet: usize, at: &str) -> String {
        wb.sheets[sheet].cells.get(&CellRef::parse(at).unwrap()).map(|c| c.input.clone()).unwrap_or_default()
    }

    #[test]
    fn formulas_follow_rows_put_in_and_taken_out_on_every_sheet() {
        let mut w = wb(&[
            ("A1", "1"),
            ("A2", "2"),
            ("A3", "3"),
            ("A5", "=SUM(A1:A3)"),
            ("B5", "=$A$3*2"),
            ("C5", "=SUM(A:A)"),
            ("Other!A1", "=Sheet1!A3+Sheet1!A1"),
        ]);
        w.names.insert("Last".into(), "Sheet1!$A$3".into());
        insert(&mut w, 0, Axis::Rows, 1, 2);
        assert_eq!(input(&w, 0, "A4"), "2");
        assert_eq!(input(&w, 0, "A7"), "=SUM(A1:A5)", "a range grows round rows put in it");
        assert_eq!(input(&w, 0, "B7"), "=$A$5*2", "absolute references move too");
        assert_eq!(input(&w, 0, "C7"), "=SUM(A:A)");
        assert_eq!(input(&w, 1, "A1"), "=Sheet1!A5+Sheet1!A1", "and those on other sheets");
        assert_eq!(w.names["Last"], "Sheet1!$A$5");

        delete(&mut w, 0, Axis::Rows, 3, 2); // the old A2 and A3
        assert_eq!(input(&w, 0, "A5"), "=SUM(A1:A3)", "a range shrinks");
        assert_eq!(input(&w, 0, "B5"), "=#REF!*2", "a reference to what went is #REF!");
        assert_eq!(input(&w, 1, "A1"), "=#REF!+Sheet1!A1");
        assert_eq!(w.names["Last"], "#REF!");
    }

    #[test]
    fn columns_move_widths_and_formulas() {
        let mut w = wb(&[("A1", "1"), ("B1", "2"), ("C1", "=A1+B1"), ("D1", "=SUM(A1:B1)")]);
        w.sheets[0].widths.insert(1, 80.0);
        insert(&mut w, 0, Axis::Cols, 1, 1);
        assert_eq!(input(&w, 0, "D1"), "=A1+C1");
        assert_eq!(input(&w, 0, "E1"), "=SUM(A1:C1)");
        assert_eq!(w.sheets[0].widths.get(&2), Some(&80.0));
        delete(&mut w, 0, Axis::Cols, 0, 1);
        assert_eq!(input(&w, 0, "C1"), "=#REF!+B1");
        assert_eq!(input(&w, 0, "D1"), "=SUM(A1:B1)");
    }

    #[test]
    fn filling_continues_a_series_and_moves_formulas() {
        let w = wb(&[("A1", "1"), ("A2", "3"), ("B1", "=A1*2"), ("C1", "x")]);
        let cells = &w.sheets[0].cells;
        let r = |s: &str| Range::parse(s).unwrap();
        let filled: Vec<(String, String)> = fill(cells, r("A1:C2"), r("A1:C4")).into_iter().map(|(at, c)| (at.name(), c.input)).collect();
        assert!(filled.contains(&("A3".into(), "5".into())) && filled.contains(&("A4".into(), "7".into())), "{filled:?}");
        assert!(filled.contains(&("B3".into(), "=A3*2".into())), "{filled:?}");
        assert!(filled.contains(&("C3".into(), "x".into())) && filled.contains(&("C4".into(), String::new())), "{filled:?}");
        let across: Vec<(String, String)> = fill(cells, r("B1"), r("B1:D1")).into_iter().map(|(at, c)| (at.name(), c.input)).collect();
        assert_eq!(across, vec![("C1".into(), "=B1*2".into()), ("D1".into(), "=C1*2".into())]);
        assert_eq!(moved_input("=$A1+B$1", 2, 1), "=$A3+C$1");
        assert_eq!(moved_input("plain", 2, 1), "plain");
    }
}
