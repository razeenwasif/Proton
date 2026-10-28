//! Maths: sums, rounding, and the rest of what a calculator has.

use super::*;
use crate::value::round_half_away;

pub fn call(cx: &Ctx, name: &str, args: &[Expr]) -> Option<R> {
    let one = |f: fn(f64) -> f64| -> R {
        arity(args, 1, 1)?;
        num(f(number(cx, args, 0, None)?))
    };
    Some(match name {
        "SUM" => numbers(cx, args).and_then(|v| num(v.iter().sum())),
        "PRODUCT" => numbers(cx, args).and_then(|v| num(if v.is_empty() { 0.0 } else { v.iter().product() })),
        "SUMSQ" => numbers(cx, args).and_then(|v| num(v.iter().map(|x| x * x).sum())),
        "MIN" => numbers(cx, args).and_then(|v| num(v.into_iter().reduce(f64::min).unwrap_or(0.0))),
        "MAX" => numbers(cx, args).and_then(|v| num(v.into_iter().reduce(f64::max).unwrap_or(0.0))),
        "SUMIF" => sumif(cx, args),
        "SUMIFS" => arity(args, 3, 255)
            .and_then(|_| ifs_matches(cx, &args[1..]))
            .and_then(|m| at_offsets(cx, &args[0], &m))
            .and_then(only_numbers)
            .and_then(|v| num(v.iter().sum())),
        "MINIFS" | "MAXIFS" => arity(args, 3, 255)
            .and_then(|_| ifs_matches(cx, &args[1..]))
            .and_then(|m| at_offsets(cx, &args[0], &m))
            .and_then(only_numbers)
            .and_then(|v| {
                let f = if name == "MINIFS" { f64::min } else { f64::max };
                num(v.into_iter().reduce(f).unwrap_or(0.0))
            }),
        "SUMPRODUCT" => sumproduct(cx, args),
        "ROUND" | "ROUNDUP" | "ROUNDDOWN" => (|| {
            arity(args, 1, 2)?;
            let x = number(cx, args, 0, None)?;
            let d = number(cx, args, 1, Some(0.0))?.trunc() as i32;
            num(match name {
                "ROUND" => round_half_away(x, d),
                "ROUNDUP" => directed(x, d, f64::ceil),
                _ => directed(x, d, f64::floor),
            })
        })(),
        "TRUNC" => (|| {
            arity(args, 1, 2)?;
            let x = number(cx, args, 0, None)?;
            let d = number(cx, args, 1, Some(0.0))?.trunc() as i32;
            num(directed(x, d, f64::floor))
        })(),
        "INT" => one(f64::floor),
        "ABS" => one(f64::abs),
        "SIGN" => one(|x| if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }),
        "EXP" => one(f64::exp),
        "SQRT" => (|| {
            arity(args, 1, 1)?;
            let x = number(cx, args, 0, None)?;
            if x < 0.0 { Err(ErrorKind::Num) } else { num(x.sqrt()) }
        })(),
        "LN" | "LOG10" => (|| {
            arity(args, 1, 1)?;
            let x = number(cx, args, 0, None)?;
            if x <= 0.0 {
                return Err(ErrorKind::Num);
            }
            num(if name == "LN" { x.ln() } else { x.log10() })
        })(),
        "LOG" => (|| {
            arity(args, 1, 2)?;
            let x = number(cx, args, 0, None)?;
            let b = number(cx, args, 1, Some(10.0))?;
            if x <= 0.0 || b <= 0.0 {
                return Err(ErrorKind::Num);
            }
            if b == 1.0 {
                return Err(ErrorKind::Div0);
            }
            num(x.ln() / b.ln())
        })(),
        "POWER" => (|| {
            arity(args, 2, 2)?;
            let (x, y) = (number(cx, args, 0, None)?, number(cx, args, 1, None)?);
            match crate::book::apply(crate::formula::BinOp::Pow, &Value::Number(x), &Value::Number(y)) {
                Value::Error(e) => Err(e),
                v => Ok(Ev::V(v)),
            }
        })(),
        "MOD" => (|| {
            arity(args, 2, 2)?;
            let (n, d) = (number(cx, args, 0, None)?, number(cx, args, 1, None)?);
            if d == 0.0 {
                return Err(ErrorKind::Div0);
            }
            // The divisor's sign, as Excel's is.
            num(n - d * (n / d).floor())
        })(),
        "QUOTIENT" => (|| {
            arity(args, 2, 2)?;
            let (n, d) = (number(cx, args, 0, None)?, number(cx, args, 1, None)?);
            if d == 0.0 { Err(ErrorKind::Div0) } else { num((n / d).trunc()) }
        })(),
        "PI" => arity(args, 0, 0).and_then(|_| num(std::f64::consts::PI)),
        "RAND" => arity(args, 0, 0).and_then(|_| num(cx.random())),
        "RANDBETWEEN" => (|| {
            arity(args, 2, 2)?;
            let (lo, hi) = (number(cx, args, 0, None)?.ceil(), number(cx, args, 1, None)?.floor());
            if lo > hi {
                return Err(ErrorKind::Num);
            }
            num(lo + (cx.random() * (hi - lo + 1.0)).floor())
        })(),
        "CEILING" | "FLOOR" => (|| {
            arity(args, 1, 2)?;
            let x = number(cx, args, 0, None)?;
            let s = number(cx, args, 1, Some(if x < 0.0 { -1.0 } else { 1.0 }))?;
            if s == 0.0 {
                return if name == "FLOOR" { Err(ErrorKind::Div0) } else { num(0.0) };
            }
            if x > 0.0 && s < 0.0 {
                return Err(ErrorKind::Num);
            }
            let q = crate::value::fifteen(x / s);
            num(if name == "CEILING" { q.ceil() } else { q.floor() } * s)
        })(),
        "CEILING.MATH" | "FLOOR.MATH" => (|| {
            arity(args, 1, 3)?;
            let x = number(cx, args, 0, None)?;
            let s = number(cx, args, 1, Some(1.0))?.abs();
            let away = number(cx, args, 2, Some(0.0))? != 0.0;
            if s == 0.0 {
                return num(0.0);
            }
            let q = crate::value::fifteen(x / s);
            let up = name == "CEILING.MATH";
            // A negative number goes toward zero unless `mode` says away.
            let r = if x < 0.0 && away { if up { q.floor() } else { q.ceil() } } else if up { q.ceil() } else { q.floor() };
            num(r * s)
        })(),
        "EVEN" | "ODD" => (|| {
            arity(args, 1, 1)?;
            let x = number(cx, args, 0, None)?;
            let a = x.abs().ceil();
            let r = if name == "EVEN" {
                if a % 2.0 == 0.0 { a } else { a + 1.0 }
            } else if a % 2.0 == 1.0 {
                a
            } else {
                a + 1.0
            };
            num(if x < 0.0 { -r } else { r })
        })(),
        "FACT" => (|| {
            arity(args, 1, 1)?;
            let x = number(cx, args, 0, None)?.trunc();
            if x < 0.0 {
                return Err(ErrorKind::Num);
            }
            num((1..=x as u64).fold(1.0, |acc, k| acc * k as f64))
        })(),
        "GCD" | "LCM" => (|| {
            let v = numbers(cx, args)?;
            if v.iter().any(|x| *x < 0.0) {
                return Err(ErrorKind::Num);
            }
            let v: Vec<u64> = v.iter().map(|x| x.trunc() as u64).collect();
            fn gcd(a: u64, b: u64) -> u64 {
                if b == 0 { a } else { gcd(b, a % b) }
            }
            let r = if name == "GCD" {
                v.into_iter().fold(0, gcd) as f64
            } else {
                v.into_iter().fold(1u64, |a, b| if a == 0 || b == 0 { 0 } else { a / gcd(a, b) * b }) as f64
            };
            num(r)
        })(),
        _ => return None,
    })
}

/// Rounded toward or away from zero at `d` places: `f` is how the
/// magnitude goes (`ceil` away, `floor` toward).
fn directed(x: f64, d: i32, f: fn(f64) -> f64) -> f64 {
    let d = d.clamp(-308, 308);
    let m = 10f64.powi(d.abs());
    let a = x.abs();
    let r = if d >= 0 { f(crate::value::fifteen(a * m)) / m } else { f(crate::value::fifteen(a / m)) * m };
    if x < 0.0 { -r } else { r }
}

fn sumif(cx: &Ctx, args: &[Expr]) -> R {
    arity(args, 2, 3)?;
    let crit = Criteria::new(&cx.value(&args[1]));
    let (s, r) = area(cx, &args[0])?;
    let (ss, sr) = match args.get(2) {
        Some(e) if !matches!(e, Expr::Missing) => area(cx, e)?,
        _ => (s, r),
    };
    let c = cut(cx, s, r);
    let mut total = 0.0;
    for dr in 0..c.rows() {
        for dc in 0..c.cols() {
            if crit.matches(&cx.get(s, CellRef::new(r.start.row + dr, r.start.col + dc))) {
                match cx.get(ss, CellRef::new(sr.start.row + dr, sr.start.col + dc)) {
                    Value::Number(n) => total += n,
                    Value::Error(e) => return Err(e),
                    _ => {}
                }
            }
        }
    }
    num(total)
}

fn sumproduct(cx: &Ctx, args: &[Expr]) -> R {
    arity(args, 1, 255)?;
    let mut tables = Vec::new();
    for a in args {
        tables.push(table(cx, a)?);
    }
    let (rows, cols) = (tables[0].len(), tables[0].first().map_or(0, Vec::len));
    if tables.iter().any(|t| t.len() != rows || t.iter().any(|r| r.len() != cols)) {
        return Err(ErrorKind::Value);
    }
    let mut total = 0.0;
    for r in 0..rows {
        for c in 0..cols {
            let mut p = 1.0;
            for t in &tables {
                p *= match &t[r][c] {
                    Value::Number(n) => *n,
                    Value::Error(e) => return Err(*e),
                    // TRUE and FALSE count only once arithmetic made them
                    // numbers, as in SUMPRODUCT((A1:A9>0)*1).
                    _ => 0.0,
                };
            }
            total += p;
        }
    }
    num(total)
}

#[cfg(test)]
mod tests {
    use crate::functions::testing::*;
    use crate::value::ErrorKind::*;

    #[test]
    fn sums_and_rounding_come_to_what_excel_says() {
        let cells = [("A1", "1"), ("A2", "2"), ("A3", "'3"), ("A4", "TRUE"), ("A5", "4"), ("B1", "apple"), ("B2", "banana"), ("B3", "apple"), ("C1", "10"), ("C2", "20"), ("C3", "30")];
        check(
            &cells,
            &[
                ("SUM(A1:A5)", n(7.0)),
                ("SUM(A1:A5,\"3\",TRUE)", n(11.0)),
                ("SUM(A:A)", n(7.0)),
                ("SUM(\"x\")", e(Value)),
                ("SUM(1/0,A1)", e(Div0)),
                ("PRODUCT(A1:A5)", n(8.0)),
                ("MAX(A1:A5,-1)", n(4.0)),
                ("MIN(B1:B3)", n(0.0)),
                ("SUMIF(B1:B3,\"apple\",C1:C3)", n(40.0)),
                ("SUMIF(C1:C3,\">15\")", n(50.0)),
                ("SUMIF(B1:B3,\"b*\",C1:C3)", n(20.0)),
                ("SUMIFS(C1:C3,B1:B3,\"apple\",C1:C3,\">10\")", n(30.0)),
                ("MAXIFS(C1:C3,B1:B3,\"apple\")", n(30.0)),
                ("SUMPRODUCT(A1:A2,C1:C2)", n(50.0)),
                ("SUMPRODUCT((C1:C3>15)*C1:C3)", n(50.0)),
                ("ROUND(2.675,2)", n(2.68)),
                ("ROUND(-2.5,0)", n(-3.0)),
                ("ROUND(1234.567,-2)", n(1200.0)),
                ("ROUNDUP(3.2,0)", n(4.0)),
                ("ROUNDUP(-3.14159,1)", n(-3.2)),
                ("ROUNDDOWN(-3.14159,1)", n(-3.1)),
                ("ROUNDDOWN(31415.92654,-2)", n(31400.0)),
                ("INT(-8.9)", n(-9.0)),
                ("TRUNC(-8.9)", n(-8.0)),
                ("MOD(3,2)", n(1.0)),
                ("MOD(-3,2)", n(1.0)),
                ("MOD(3,-2)", n(-1.0)),
                ("MOD(1,0)", e(Div0)),
                ("SQRT(-1)", e(Num)),
                ("LOG(8,2)", n(3.0)),
                ("LOG10(1000)", n(3.0)),
                ("POWER(2,10)", n(1024.0)),
                ("CEILING(2.5,1)", n(3.0)),
                ("CEILING(-2.5,-2)", n(-4.0)),
                ("FLOOR(3.7,2)", n(2.0)),
                ("FLOOR(-2.5,-2)", n(-2.0)),
                ("CEILING(2.5,-1)", e(Num)),
                ("CEILING.MATH(-5.5,2,-1)", n(-6.0)),
                ("FLOOR.MATH(-5.5,2)", n(-6.0)),
                ("EVEN(1.5)", n(2.0)),
                ("ODD(-2)", n(-3.0)),
                ("FACT(5)", n(120.0)),
                ("GCD(24,36)", n(12.0)),
                ("LCM(4,6)", n(12.0)),
                ("QUOTIENT(-10,3)", n(-3.0)),
                ("SIGN(-4)", n(-1.0)),
                ("ABS(-4)", n(4.0)),
            ],
        );
    }

    #[test]
    fn rand_stays_in_its_range() {
        let mut book = sheet(&[]);
        for _ in 0..50 {
            let crate::value::Value::Number(x) = eval(&mut book, "RANDBETWEEN(1,6)") else { panic!() };
            assert!((1.0..=6.0).contains(&x) && x.fract() == 0.0, "{x}");
            let crate::value::Value::Number(r) = eval(&mut book, "RAND()") else { panic!() };
            assert!((0.0..1.0).contains(&r));
        }
    }
}
