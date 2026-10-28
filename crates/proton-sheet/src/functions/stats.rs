//! Statistics: averages, spreads, counts and ranks.

use super::*;

pub fn call(cx: &Ctx, name: &str, args: &[Expr]) -> Option<R> {
    Some(match name {
        "AVERAGE" => numbers(cx, args).and_then(|v| mean(&v).and_then(num)),
        "AVERAGEA" => averagea(cx, args),
        "AVERAGEIF" => averageif(cx, args),
        "AVERAGEIFS" => arity(args, 3, 255)
            .and_then(|_| ifs_matches(cx, &args[1..]))
            .and_then(|m| at_offsets(cx, &args[0], &m))
            .and_then(only_numbers)
            .and_then(|v| mean(&v).and_then(num)),
        "MEDIAN" => numbers(cx, args).and_then(|mut v| {
            if v.is_empty() {
                return Err(ErrorKind::Num);
            }
            v.sort_by(f64::total_cmp);
            let m = v.len() / 2;
            num(if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) / 2.0 })
        }),
        "MODE" | "MODE.SNGL" => numbers(cx, args).and_then(|v| {
            // The value seen most often, the first seen among equals; none
            // seen twice is #N/A.
            let mut best: Option<(f64, usize)> = None;
            for (i, x) in v.iter().enumerate() {
                let c = v[i..].iter().filter(|y| *y == x).count();
                if v[..i].contains(x) {
                    continue;
                }
                if c > 1 && best.is_none_or(|(_, b)| c > b) {
                    best = Some((*x, c));
                }
            }
            best.map_or(Err(ErrorKind::NA), |(x, _)| num(x))
        }),
        "STDEV" | "STDEV.S" => numbers(cx, args).and_then(|v| variance(&v, true).and_then(|x| num(x.sqrt()))),
        "STDEVP" | "STDEV.P" => numbers(cx, args).and_then(|v| variance(&v, false).and_then(|x| num(x.sqrt()))),
        "VAR" | "VAR.S" => numbers(cx, args).and_then(|v| variance(&v, true).and_then(num)),
        "VARP" | "VAR.P" => numbers(cx, args).and_then(|v| variance(&v, false).and_then(num)),
        "COUNT" => count(cx, args),
        "COUNTA" => (|| {
            let mut n = 0.0;
            for a in args {
                if matches!(a, Expr::Missing) {
                    continue;
                }
                each_value(cx, a, |v, _| {
                    if !v.is_empty() {
                        n += 1.0;
                    }
                    Ok(())
                })?;
            }
            num(n)
        })(),
        "COUNTBLANK" => (|| {
            arity(args, 1, 1)?;
            let (s, r) = area(cx, &args[0])?;
            let mut filled = 0.0;
            cx.each(s, r, |_, v| {
                if !(v.is_empty() || matches!(&v, Value::Text(t) if t.is_empty())) {
                    filled += 1.0;
                }
            });
            num(r.rows() as f64 * r.cols() as f64 - filled)
        })(),
        "COUNTIF" => (|| {
            arity(args, 2, 2)?;
            let (s, r) = area(cx, &args[0])?;
            let crit = Criteria::new(&cx.value(&args[1]));
            let c = cut(cx, s, r);
            let mut n = 0.0;
            for row in c.start.row..=c.end.row {
                for col in c.start.col..=c.end.col {
                    if crit.matches(&cx.get(s, CellRef::new(row, col))) {
                        n += 1.0;
                    }
                }
            }
            // Past what is in use every cell is empty: they match a
            // condition for empty cells too.
            if crit.matches(&Value::Empty) {
                n += r.rows() as f64 * r.cols() as f64 - c.rows() as f64 * c.cols() as f64;
            }
            num(n)
        })(),
        "COUNTIFS" => ifs_matches(cx, args).and_then(|m| num(m.len() as f64)),
        "LARGE" | "SMALL" => (|| {
            arity(args, 2, 2)?;
            let mut v = numbers(cx, &args[..1])?;
            let k = number(cx, args, 1, None)?.ceil();
            if k < 1.0 || k as usize > v.len() {
                return Err(ErrorKind::Num);
            }
            v.sort_by(f64::total_cmp);
            let i = k as usize - 1;
            num(if name == "SMALL" { v[i] } else { v[v.len() - 1 - i] })
        })(),
        "RANK" | "RANK.EQ" => (|| {
            arity(args, 2, 3)?;
            let x = number(cx, args, 0, None)?;
            let v = numbers(cx, &args[1..2])?;
            let ascending = number(cx, args, 2, Some(0.0))? != 0.0;
            if !v.contains(&x) {
                return Err(ErrorKind::NA);
            }
            let ahead = v.iter().filter(|y| if ascending { **y < x } else { **y > x }).count();
            num(ahead as f64 + 1.0)
        })(),
        "PERCENTILE" | "PERCENTILE.INC" => (|| {
            arity(args, 2, 2)?;
            let v = numbers(cx, &args[..1])?;
            let k = number(cx, args, 1, None)?;
            percentile(v, k).and_then(num)
        })(),
        "QUARTILE" | "QUARTILE.INC" => (|| {
            arity(args, 2, 2)?;
            let v = numbers(cx, &args[..1])?;
            let q = number(cx, args, 1, None)?.trunc();
            if !(0.0..=4.0).contains(&q) {
                return Err(ErrorKind::Num);
            }
            percentile(v, q / 4.0).and_then(num)
        })(),
        "CORREL" => correl(cx, args),
        _ => return None,
    })
}

fn mean(v: &[f64]) -> Result<f64, ErrorKind> {
    if v.is_empty() {
        Err(ErrorKind::Div0)
    } else {
        Ok(v.iter().sum::<f64>() / v.len() as f64)
    }
}

fn variance(v: &[f64], sample: bool) -> Result<f64, ErrorKind> {
    let n = v.len() as f64;
    if v.is_empty() || (sample && v.len() < 2) {
        return Err(ErrorKind::Div0);
    }
    let m = v.iter().sum::<f64>() / n;
    let ss: f64 = v.iter().map(|x| (x - m) * (x - m)).sum();
    Ok(ss / if sample { n - 1.0 } else { n })
}

/// PERCENTILE.INC: the value at `k` of the way along the sorted numbers,
/// between two of them in proportion.
fn percentile(mut v: Vec<f64>, k: f64) -> Result<f64, ErrorKind> {
    if v.is_empty() || !(0.0..=1.0).contains(&k) {
        return Err(ErrorKind::Num);
    }
    v.sort_by(f64::total_cmp);
    let h = (v.len() - 1) as f64 * k;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(v.len() - 1);
    Ok(v[lo] + (h - lo as f64) * (v[hi] - v[lo]))
}

fn count(cx: &Ctx, args: &[Expr]) -> R {
    let mut n = 0.0;
    for a in args {
        if matches!(a, Expr::Missing) {
            continue;
        }
        each_value(cx, a, |v, direct| {
            let counts = match v {
                Value::Number(_) => true,
                Value::Bool(_) => direct,
                Value::Text(t) => direct && parse_number(&t).is_some(),
                _ => false,
            };
            if counts {
                n += 1.0;
            }
            Ok(())
        })?;
    }
    num(n)
}

/// AVERAGEA: text in cells counts as 0, TRUE as 1.
fn averagea(cx: &Ctx, args: &[Expr]) -> R {
    let mut v = Vec::new();
    for a in args {
        each_value(cx, a, |x, direct| {
            match x {
                Value::Number(n) => v.push(n),
                Value::Bool(b) => v.push(if b { 1.0 } else { 0.0 }),
                Value::Text(t) if direct => v.push(parse_number(&t).ok_or(ErrorKind::Value)?),
                Value::Text(_) => v.push(0.0),
                Value::Error(e) => return Err(e),
                Value::Empty => {}
            }
            Ok(())
        })?;
    }
    mean(&v).and_then(num)
}

fn averageif(cx: &Ctx, args: &[Expr]) -> R {
    arity(args, 2, 3)?;
    let crit = Criteria::new(&cx.value(&args[1]));
    let (s, r) = area(cx, &args[0])?;
    let (ss, sr) = match args.get(2) {
        Some(e) if !matches!(e, Expr::Missing) => area(cx, e)?,
        _ => (s, r),
    };
    let c = cut(cx, s, r);
    let mut v = Vec::new();
    for dr in 0..c.rows() {
        for dc in 0..c.cols() {
            if crit.matches(&cx.get(s, CellRef::new(r.start.row + dr, r.start.col + dc))) {
                match cx.get(ss, CellRef::new(sr.start.row + dr, sr.start.col + dc)) {
                    Value::Number(n) => v.push(n),
                    Value::Error(e) => return Err(e),
                    _ => {}
                }
            }
        }
    }
    mean(&v).and_then(num)
}

/// Pearson's r over the pairs where both sides are numbers.
fn correl(cx: &Ctx, args: &[Expr]) -> R {
    arity(args, 2, 2)?;
    let a: Vec<Value> = table(cx, &args[0])?.into_iter().flatten().collect();
    let b: Vec<Value> = table(cx, &args[1])?.into_iter().flatten().collect();
    if a.len() != b.len() {
        return Err(ErrorKind::NA);
    }
    let mut pairs = Vec::new();
    for (x, y) in a.iter().zip(&b) {
        if let Value::Error(e) = x {
            return Err(*e);
        }
        if let Value::Error(e) = y {
            return Err(*e);
        }
        if let (Value::Number(x), Value::Number(y)) = (x, y) {
            pairs.push((*x, *y));
        }
    }
    if pairs.len() < 2 {
        return Err(ErrorKind::Div0);
    }
    let n = pairs.len() as f64;
    let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in pairs {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
    }
    if sxx == 0.0 || syy == 0.0 {
        return Err(ErrorKind::Div0);
    }
    num(sxy / (sxx * syy).sqrt())
}

#[cfg(test)]
mod tests {
    use crate::functions::testing::*;
    use crate::value::ErrorKind::*;

    #[test]
    fn statistics_come_to_what_excel_says() {
        // Excel's own examples: STDEV.S of 1345…1281 is 27.46391572.
        let strength = ["1345", "1301", "1368", "1322", "1310", "1370", "1318", "1350", "1303", "1299"];
        let mut cells: Vec<(String, String)> = strength.iter().enumerate().map(|(i, v)| (format!("A{}", i + 1), v.to_string())).collect();
        for (at, v) in [("B1", "1"), ("B2", "2"), ("B3", "3"), ("B4", "4"), ("B5", "x"), ("B6", "TRUE"), ("C1", "a"), ("C3", "b"), ("C4", "a"), ("D1", "3"), ("D2", "2"), ("D3", "4"), ("D4", "7"), ("D5", "4")] {
            cells.push((at.into(), v.into()));
        }
        let cells: Vec<(&str, &str)> = cells.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        check(
            &cells,
            &[
                ("STDEV.S(A1:A10)", n(27.463915719843)),
                ("STDEV.P(A1:A10)", n(26.054558142482)),
                ("VAR.S(A1:A10)", n(754.266666666667)),
                ("VAR.P(A1:A10)", n(678.84)),
                ("AVERAGE(B1:B6)", n(2.5)),
                ("AVERAGEA(B1:B6)", n(11.0 / 6.0)),
                ("AVERAGE(C1:C4)", e(Div0)),
                ("MEDIAN(B1:B4)", n(2.5)),
                ("MEDIAN(D1:D5)", n(4.0)),
                ("MODE(D1:D5)", n(4.0)),
                ("MODE(B1:B4)", e(NA)),
                ("COUNT(B1:B6)", n(4.0)),
                ("COUNT(1,\"2\",\"x\",TRUE)", n(3.0)),
                ("COUNTA(B1:B6)", n(6.0)),
                ("COUNTBLANK(C1:C4)", n(1.0)),
                ("COUNTIF(C1:C4,\"a\")", n(2.0)),
                ("COUNTIF(C1:C4,\"\")", n(1.0)),
                ("COUNTIF(C:C,\"<>\")", n(3.0)),
                ("COUNTIF(B1:B6,\">2\")", n(2.0)),
                ("COUNTIFS(C1:C4,\"a\",B1:B4,\">1\")", n(1.0)),
                ("AVERAGEIF(C1:C4,\"a\",B1:B4)", n(2.5)),
                ("AVERAGEIFS(B1:B4,C1:C4,\"a\")", n(2.5)),
                ("LARGE(D1:D5,2)", n(4.0)),
                ("SMALL(D1:D5,1)", n(2.0)),
                ("SMALL(D1:D5,9)", e(Num)),
                ("RANK(4,D1:D5)", n(2.0)),
                ("RANK(4,D1:D5,1)", n(3.0)),
                ("RANK(5,D1:D5)", e(NA)),
                ("PERCENTILE(B1:B4,0.3)", n(1.9)),
                ("QUARTILE(B1:B4,1)", n(1.75)),
                ("CORREL(B1:B4,D1:D4)", n(0.836660026534076)),
            ],
        );
    }
}
