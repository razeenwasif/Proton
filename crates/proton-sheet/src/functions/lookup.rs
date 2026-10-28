//! Lookup and reference: finding a value in a table, and references made
//! while working out — INDEX, OFFSET and INDIRECT hand back cells, so
//! `SUM(OFFSET(A1,0,0,3))` adds up three of them.

use std::cmp::Ordering;

use super::*;
use crate::refs::{MAX_COL, MAX_ROW};

pub fn call(cx: &Ctx, name: &str, args: &[Expr]) -> Option<R> {
    Some(match name {
        "VLOOKUP" | "HLOOKUP" => (|| {
            arity(args, 3, 4)?;
            let x = scalar(cx, &args[0])?;
            let mut rows = table(cx, &args[1])?;
            if name == "HLOOKUP" {
                rows = transpose(rows);
            }
            let k = number(cx, args, 2, None)?.trunc();
            let approx = bool_arg(cx, args, 3, true)?;
            if k < 1.0 {
                return Err(ErrorKind::Value);
            }
            let keys: Vec<Value> = rows.iter().map(|r| r.first().cloned().unwrap_or(Value::Empty)).collect();
            let i = if approx { last_not_above(&keys, &x) } else { exact(&keys, &x, true) }.ok_or(ErrorKind::NA)?;
            rows[i].get(k as usize - 1).cloned().map(Ev::V).ok_or(ErrorKind::Ref)
        })(),
        "MATCH" => (|| {
            arity(args, 2, 3)?;
            let x = scalar(cx, &args[0])?;
            let keys = vector(table(cx, &args[1])?)?;
            let kind = number(cx, args, 2, Some(1.0))?;
            let i = if kind == 0.0 {
                exact(&keys, &x, true)
            } else if kind > 0.0 {
                last_not_above(&keys, &x)
            } else {
                last_not_below(&keys, &x)
            };
            i.map_or(Err(ErrorKind::NA), |i| num(i as f64 + 1.0))
        })(),
        "XMATCH" => (|| {
            arity(args, 2, 4)?;
            let x = scalar(cx, &args[0])?;
            let keys = vector(table(cx, &args[1])?)?;
            let mode = number(cx, args, 2, Some(0.0))?;
            let search = number(cx, args, 3, Some(1.0))?;
            xfind(&keys, &x, mode, search).map_or(Err(ErrorKind::NA), |i| num(i as f64 + 1.0))
        })(),
        "XLOOKUP" => xlookup(cx, args),
        "INDEX" => index(cx, args),
        "OFFSET" => offset(cx, args),
        "INDIRECT" => (|| {
            arity(args, 1, 2)?;
            let s = text_arg(cx, args, 0, None)?;
            match crate::formula::parse(s.trim()) {
                Ok(e @ (Expr::Ref { .. } | Expr::Range { .. } | Expr::Name(_))) => match cx.eval(&e) {
                    ev @ Ev::Area(..) => Ok(ev),
                    _ => Err(ErrorKind::Ref),
                },
                _ => Err(ErrorKind::Ref),
            }
        })(),
        "ROW" | "COLUMN" => (|| {
            arity(args, 0, 1)?;
            let at = match args.first() {
                None => cx.at,
                Some(e) => area(cx, e)?.1.start,
            };
            num(if name == "ROW" { at.row } else { at.col } as f64 + 1.0)
        })(),
        "ROWS" | "COLUMNS" => (|| {
            arity(args, 1, 1)?;
            let (r, c) = match cx.eval(&args[0]) {
                Ev::Area(_, r) => (r.rows() as usize, r.cols() as usize),
                Ev::Arr(a) => (a.len(), a.first().map_or(0, Vec::len)),
                Ev::V(Value::Error(e)) => return Err(e),
                Ev::V(_) => (1, 1),
            };
            num(if name == "ROWS" { r } else { c } as f64)
        })(),
        "TRANSPOSE" => arity(args, 1, 1).and_then(|_| Ok(Ev::Arr(transpose(table(cx, &args[0])?)))),
        _ => return None,
    })
}

fn transpose(rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    (0..cols).map(|c| rows.iter().map(|r| r.get(c).cloned().unwrap_or(Value::Empty)).collect()).collect()
}

/// Whether two values are the same for a lookup: text without case, and a
/// pattern's wildcards when `wild`.
fn same(key: &Value, x: &Value, wild: bool) -> bool {
    match (key, x) {
        (Value::Text(k), Value::Text(p)) if wild && p.contains(['*', '?', '~']) => wildcard(p, k),
        (Value::Empty, _) => false,
        _ => key.compare(x) == Ordering::Equal && std::mem::discriminant(key) == std::mem::discriminant(x),
    }
}

fn exact(keys: &[Value], x: &Value, wild: bool) -> Option<usize> {
    keys.iter().position(|k| same(k, x, wild))
}

/// Whether a key and the value sought are of a kind to be ordered against
/// each other, as a sorted lookup compares only like with like.
fn alike(k: &Value, x: &Value) -> bool {
    matches!((k, x), (Value::Number(_), Value::Number(_)) | (Value::Text(_), Value::Text(_)) | (Value::Bool(_), Value::Bool(_)))
}

/// The last key not above `x` in keys sorted ascending: a sorted lookup.
fn last_not_above(keys: &[Value], x: &Value) -> Option<usize> {
    let mut found = None;
    for (i, k) in keys.iter().enumerate() {
        if !alike(k, x) {
            continue;
        }
        if k.compare(x) == Ordering::Greater {
            break;
        }
        found = Some(i);
    }
    found
}

/// The last key not below `x` in keys sorted descending.
fn last_not_below(keys: &[Value], x: &Value) -> Option<usize> {
    let mut found = None;
    for (i, k) in keys.iter().enumerate() {
        if !alike(k, x) {
            continue;
        }
        if k.compare(x) == Ordering::Less {
            break;
        }
        found = Some(i);
    }
    found
}

/// XMATCH's and XLOOKUP's search: `mode` 0 exact, -1 or the next smaller,
/// 1 or the next larger, 2 with wildcards; `search` -1 from the end. Keys
/// need not be sorted.
fn xfind(keys: &[Value], x: &Value, mode: f64, search: f64) -> Option<usize> {
    let order: Vec<usize> = if search < 0.0 { (0..keys.len()).rev().collect() } else { (0..keys.len()).collect() };
    if let Some(i) = order.iter().copied().find(|&i| same(&keys[i], x, mode == 2.0)) {
        return Some(i);
    }
    if mode == 0.0 || mode == 2.0 {
        return None;
    }
    let mut best: Option<usize> = None;
    for i in order {
        let k = &keys[i];
        if !alike(k, x) {
            continue;
        }
        let side = k.compare(x);
        let wanted = if mode < 0.0 { Ordering::Less } else { Ordering::Greater };
        if side != wanted {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => k.compare(&keys[b]) == if mode < 0.0 { Ordering::Greater } else { Ordering::Less },
        };
        if better {
            best = Some(i);
        }
    }
    best
}

fn xlookup(cx: &Ctx, args: &[Expr]) -> R {
    arity(args, 3, 6)?;
    let x = scalar(cx, &args[0])?;
    let look = cx.eval(&args[1]);
    let keys = vector(cx.rows(look.clone()))?;
    let mode = number(cx, args, 4, Some(0.0))?;
    let search = number(cx, args, 5, Some(1.0))?;
    let Some(i) = xfind(&keys, &x, mode, search) else {
        return match args.get(3) {
            Some(e) if !matches!(e, Expr::Missing) => Ok(cx.eval(e)),
            _ => Err(ErrorKind::NA),
        };
    };
    let i = i as u32;
    // Down a column, the matching row of what is returned; along a row, the
    // matching column.
    let down = match &look {
        Ev::Area(_, r) => r.cols() == 1,
        _ => keys.len() > 1 && cx.rows(look.clone()).len() > 1,
    };
    match cx.eval(&args[2]) {
        Ev::Area(s, r) => {
            let cut = if down {
                (i < r.rows()).then(|| Range::new(CellRef::new(r.start.row + i, r.start.col), CellRef::new(r.start.row + i, r.end.col)))
            } else {
                (i < r.cols()).then(|| Range::new(CellRef::new(r.start.row, r.start.col + i), CellRef::new(r.end.row, r.start.col + i)))
            };
            cut.map(|r| Ev::Area(s, r)).ok_or(ErrorKind::Value)
        }
        Ev::V(Value::Error(e)) => Err(e),
        ev => {
            let rows = cx.rows(ev);
            let picked = if down { rows.get(i as usize).cloned().map(|r| vec![r]) } else { Some(rows.iter().map(|r| vec![r.get(i as usize).cloned().unwrap_or(Value::Empty)]).collect()) };
            picked.map(Ev::Arr).ok_or(ErrorKind::Value)
        }
    }
}

fn index(cx: &Ctx, args: &[Expr]) -> R {
    arity(args, 2, 3)?;
    let row = number(cx, args, 1, Some(0.0))?.trunc();
    let col = number(cx, args, 2, Some(0.0))?.trunc();
    if row < 0.0 || col < 0.0 {
        return Err(ErrorKind::Value);
    }
    let (row, col) = (row as u32, col as u32);
    match cx.eval(&args[0]) {
        Ev::Area(s, r) => {
            // One row or column: the one number counts along it.
            let (row, col) = if args.len() == 2 && r.rows() == 1 { (1, row) } else { (row, col) };
            if row > r.rows() || col > r.cols() {
                return Err(ErrorKind::Ref);
            }
            let (r0, r1) = if row == 0 { (r.start.row, r.end.row) } else { (r.start.row + row - 1, r.start.row + row - 1) };
            let (c0, c1) = if col == 0 { (r.start.col, r.end.col) } else { (r.start.col + col - 1, r.start.col + col - 1) };
            Ok(Ev::Area(s, Range::new(CellRef::new(r0, c0), CellRef::new(r1, c1))))
        }
        Ev::V(Value::Error(e)) => Err(e),
        ev => {
            let rows = cx.rows(ev);
            let (row, col) = if args.len() == 2 && rows.len() == 1 { (1, row) } else { (row, col) };
            let (row, col) = (row.max(1) as usize, col.max(1) as usize);
            rows.get(row - 1).and_then(|r| r.get(col - 1)).cloned().map(Ev::V).ok_or(ErrorKind::Ref)
        }
    }
}

fn offset(cx: &Ctx, args: &[Expr]) -> R {
    arity(args, 3, 5)?;
    let (s, r) = area(cx, &args[0])?;
    let dr = number(cx, args, 1, None)?.trunc() as i64;
    let dc = number(cx, args, 2, None)?.trunc() as i64;
    let h = number(cx, args, 3, Some(r.rows() as f64))?.trunc() as i64;
    let w = number(cx, args, 4, Some(r.cols() as f64))?.trunc() as i64;
    if h < 1 || w < 1 {
        return Err(ErrorKind::Ref);
    }
    let (r0, c0) = (r.start.row as i64 + dr, r.start.col as i64 + dc);
    let (r1, c1) = (r0 + h - 1, c0 + w - 1);
    if r0 < 0 || c0 < 0 || r1 > MAX_ROW as i64 || c1 > MAX_COL as i64 {
        return Err(ErrorKind::Ref);
    }
    Ok(Ev::Area(s, Range::new(CellRef::new(r0 as u32, c0 as u32), CellRef::new(r1 as u32, c1 as u32))))
}

#[cfg(test)]
mod tests {
    use crate::functions::testing::*;
    use crate::value::ErrorKind::*;

    const FRUIT: [(&str, &str); 12] = [
        ("A1", "apple"), ("B1", "0.5"), ("C1", "red"),
        ("A2", "banana"), ("B2", "0.25"), ("C2", "yellow"),
        ("A3", "cherry"), ("B3", "3"), ("C3", "red"),
        ("E1", "10"), ("E2", "20"), ("E3", "30"),
    ];

    #[test]
    fn lookups_find_what_excel_finds() {
        check(
            &FRUIT,
            &[
                ("VLOOKUP(\"banana\",A1:C3,2,FALSE)", n(0.25)),
                ("VLOOKUP(\"BANANA\",A1:C3,3,FALSE)", t("yellow")),
                ("VLOOKUP(\"b*\",A1:C3,3,FALSE)", t("yellow")),
                ("VLOOKUP(\"kiwi\",A1:C3,2,FALSE)", e(NA)),
                ("VLOOKUP(25,E1:E3,1)", n(20.0)),
                ("VLOOKUP(5,E1:E3,1)", e(NA)),
                ("VLOOKUP(\"apple\",A1:C3,4,FALSE)", e(Ref)),
                ("HLOOKUP(20,{10,20,30;\"a\",\"b\",\"c\"},2,FALSE)", t("b")),
                ("MATCH(\"cherry\",A1:A3,0)", n(3.0)),
                ("MATCH(25,E1:E3)", n(2.0)),
                ("MATCH(25,{30,20,10},-1)", n(1.0)),
                ("XMATCH(\"cherry\",A1:A3)", n(3.0)),
                ("XMATCH(25,E1:E3,1)", n(3.0)),
                ("XMATCH(25,E1:E3,-1)", n(2.0)),
                ("XLOOKUP(\"cherry\",A1:A3,B1:B3)", n(3.0)),
                ("XLOOKUP(\"kiwi\",A1:A3,B1:B3,\"none\")", t("none")),
                ("XLOOKUP(\"red\",C1:C3,A1:A3,,0,-1)", t("cherry")),
                ("SUM(XLOOKUP(\"banana\",A1:A3,B1:C3))", n(0.25)),
                ("INDEX(A1:C3,2,3)", t("yellow")),
                ("INDEX(E1:E3,2)", n(20.0)),
                ("SUM(INDEX(E1:E3,0,1))", n(60.0)),
                ("SUM(INDEX(E1:E3,1):INDEX(E1:E3,2))", n(30.0)),
                ("INDEX({1,2;3,4},2,1)", n(3.0)),
                ("INDEX(A1:C3,4,1)", e(Ref)),
                ("SUM(OFFSET(E1,1,0,2,1))", n(50.0)),
                ("OFFSET(E1,-1,0)", e(Ref)),
                ("INDIRECT(\"B\"&3)", n(3.0)),
                ("SUM(INDIRECT(\"E1:E3\"))", n(60.0)),
                ("INDIRECT(\"nowhere\")", e(Ref)),
                ("ROW(C3)", n(3.0)),
                ("COLUMN(C3)", n(3.0)),
                ("ROWS(A1:C3)", n(3.0)),
                ("COLUMNS({1,2,3})", n(3.0)),
                ("INDEX(TRANSPOSE(A1:B2),2,1)", n(0.5)),
            ],
        );
    }

    #[test]
    fn indirect_and_offset_follow_their_cells_as_they_change() {
        let mut book = sheet(&[("A1", "1"), ("A2", "2"), ("B1", "A2")]);
        let at = crate::refs::CellRef::parse("C1").unwrap();
        book.set(0, at, "=INDIRECT(B1)*10");
        assert_eq!(book.value(0, at), n(20.0));
        book.set(0, crate::refs::CellRef::parse("A2").unwrap(), "5");
        assert_eq!(book.value(0, at), n(50.0), "a cell INDIRECT reads changed");
        book.set(0, crate::refs::CellRef::parse("B1").unwrap(), "A1");
        assert_eq!(book.value(0, at), n(10.0), "what it names changed");
    }
}
