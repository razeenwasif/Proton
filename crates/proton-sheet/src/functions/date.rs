//! Dates and times, as serial numbers in Excel's 1900 system.

use super::*;
use crate::dates::{hms, month_days, parse_date, parse_time, serial, weekday, ymd};

/// An argument as a serial: a number, or text that reads as a date or time.
fn day(cx: &Ctx, args: &[Expr], i: usize) -> Result<f64, ErrorKind> {
    let v = match args.get(i) {
        None | Some(Expr::Missing) => return Err(ErrorKind::Value),
        Some(e) => scalar(cx, e)?,
    };
    let n = match &v {
        Value::Text(t) => parse_number(t).or_else(|| parse_date(t)).or_else(|| parse_time(t)).ok_or(ErrorKind::Value)?,
        v => v.to_number()?,
    };
    if n < 0.0 {
        Err(ErrorKind::Num)
    } else {
        Ok(n)
    }
}

fn parts(n: f64) -> Result<(i64, u32, u32), ErrorKind> {
    ymd(n).ok_or(ErrorKind::Num)
}

/// `months` after a date, the day kept where the month has it and the
/// month's last day where it has not.
fn add_months(n: f64, months: i64) -> Result<(i64, u32, u32), ErrorKind> {
    let (y, m, d) = parts(n)?;
    let total = y * 12 + (m as i64 - 1) + months;
    let (y, m) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    Ok((y, m, d.min(month_days(y, m))))
}

fn date_of(y: i64, m: u32, d: u32) -> R {
    serial(y, m as i64, d as i64).map_or(Err(ErrorKind::Num), num)
}

pub fn call(cx: &Ctx, name: &str, args: &[Expr]) -> Option<R> {
    Some(match name {
        "DATE" => (|| {
            arity(args, 3, 3)?;
            let (y, m, d) = (number(cx, args, 0, None)?.trunc(), number(cx, args, 1, None)?.trunc(), number(cx, args, 2, None)?.trunc());
            serial(y as i64, m as i64, d as i64).map_or(Err(ErrorKind::Num), num)
        })(),
        "TIME" => (|| {
            arity(args, 3, 3)?;
            let (h, m, s) = (number(cx, args, 0, None)?.trunc(), number(cx, args, 1, None)?.trunc(), number(cx, args, 2, None)?.trunc());
            let secs = h * 3600.0 + m * 60.0 + s;
            if secs < 0.0 {
                return Err(ErrorKind::Num);
            }
            num((secs % 86_400.0) / 86_400.0)
        })(),
        "TODAY" => arity(args, 0, 0).and_then(|_| num(cx.now().floor())),
        "NOW" => arity(args, 0, 0).and_then(|_| num(cx.now())),
        "YEAR" | "MONTH" | "DAY" => (|| {
            arity(args, 1, 1)?;
            let (y, m, d) = parts(day(cx, args, 0)?)?;
            num(match name {
                "YEAR" => y as f64,
                "MONTH" => m as f64,
                _ => d as f64,
            })
        })(),
        "HOUR" | "MINUTE" | "SECOND" => (|| {
            arity(args, 1, 1)?;
            let (h, m, s) = hms(day(cx, args, 0)?);
            num(match name {
                "HOUR" => h,
                "MINUTE" => m,
                _ => s,
            } as f64)
        })(),
        "WEEKDAY" => (|| {
            arity(args, 1, 2)?;
            let w = weekday(day(cx, args, 0)?) as i64; // Sunday 0
            let kind = number(cx, args, 1, Some(1.0))?.trunc() as i64;
            // Where the week starts (Sunday 0) and what its first day is.
            let (first, base) = match kind {
                1 | 17 => (0, 1),
                2 | 11 => (1, 1),
                3 => (1, 0),
                12..=16 => (kind - 10, 1),
                _ => return Err(ErrorKind::Num),
            };
            num(((w - first).rem_euclid(7) + base) as f64)
        })(),
        "EDATE" | "EOMONTH" => (|| {
            arity(args, 2, 2)?;
            let start = day(cx, args, 0)?;
            let months = number(cx, args, 1, None)?.trunc() as i64;
            let (y, m, d) = add_months(start, months)?;
            if name == "EDATE" { date_of(y, m, d) } else { date_of(y, m, month_days(y, m)) }
        })(),
        "DAYS" => (|| {
            arity(args, 2, 2)?;
            num(day(cx, args, 0)?.floor() - day(cx, args, 1)?.floor())
        })(),
        "DATEVALUE" => (|| {
            arity(args, 1, 1)?;
            let t = text_arg(cx, args, 0, None)?;
            parse_date(&t).map_or(Err(ErrorKind::Value), |n| num(n.floor()))
        })(),
        "DATEDIF" => datedif(cx, args),
        "NETWORKDAYS" => (|| {
            arity(args, 2, 3)?;
            let (a, b) = (day(cx, args, 0)?.floor(), day(cx, args, 1)?.floor());
            let mut holidays = Vec::new();
            if let Some(e) = args.get(2) {
                each_value(cx, e, |v, _| {
                    match v {
                        Value::Number(n) => holidays.push(n.floor()),
                        Value::Error(e) => return Err(e),
                        _ => {}
                    }
                    Ok(())
                })?;
            }
            let (lo, hi, sign) = if a <= b { (a, b, 1.0) } else { (b, a, -1.0) };
            let mut n = 0.0;
            let mut d = lo;
            while d <= hi {
                if !matches!(weekday(d), 0 | 6) && !holidays.contains(&d) {
                    n += 1.0;
                }
                d += 1.0;
            }
            num(n * sign)
        })(),
        _ => return None,
    })
}

fn datedif(cx: &Ctx, args: &[Expr]) -> R {
    arity(args, 3, 3)?;
    let (a, b) = (day(cx, args, 0)?.floor(), day(cx, args, 1)?.floor());
    if a > b {
        return Err(ErrorKind::Num);
    }
    let unit = text_arg(cx, args, 2, None)?.to_ascii_uppercase();
    let ((y1, m1, d1), (y2, m2, d2)) = (parts(a)?, parts(b)?);
    let mut months = (y2 - y1) * 12 + m2 as i64 - m1 as i64;
    if d2 < d1 {
        months -= 1;
    }
    num(match unit.as_str() {
        "D" => b - a,
        "M" => months as f64,
        "Y" => (months / 12) as f64,
        "YM" => (months % 12) as f64,
        "MD" => {
            if d2 >= d1 {
                (d2 - d1) as f64
            } else {
                // Days since that day in the month before.
                let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                let from = serial(py, pm as i64, d1.min(month_days(py, pm)) as i64).ok_or(ErrorKind::Num)?;
                b - from
            }
        }
        "YD" => {
            // Days from the start's day-and-month in the end's year, or the
            // year before when that is after the end.
            let mut y = y2;
            let mut from = serial(y, m1 as i64, d1 as i64).ok_or(ErrorKind::Num)?;
            if from > b {
                y -= 1;
                from = serial(y, m1 as i64, d1 as i64).ok_or(ErrorKind::Num)?;
            }
            b - from
        }
        _ => return Err(ErrorKind::Num),
    })
}

#[cfg(test)]
mod tests {
    use crate::functions::testing::*;
    use crate::value::ErrorKind::*;

    #[test]
    fn dates_are_excels_serials() {
        check(
            &[("A1", "2024-03-15"), ("A2", "2024-01-31"), ("A3", "18:30:15"), ("B1", "2024-01-01"), ("B2", "2024-01-15")],
            &[
                ("A1", n(45366.0)),
                ("DATE(2024,3,15)", n(45366.0)),
                ("DATE(2024,14,1)", n(45689.0)),
                ("DATE(1900,2,29)", n(60.0)),
                ("TIME(18,0,0)", n(0.75)),
                ("TIME(25,0,0)", n(1.0 / 24.0)),
                ("TODAY()", n(45366.0)),
                ("NOW()", n(45366.75)),
                ("YEAR(A1)", n(2024.0)),
                ("MONTH(\"2024-03-15\")", n(3.0)),
                ("DAY(A1)", n(15.0)),
                ("HOUR(A3)", n(18.0)),
                ("MINUTE(A3)", n(30.0)),
                ("SECOND(A3)", n(15.0)),
                ("WEEKDAY(A1)", n(6.0)),
                ("WEEKDAY(A1,2)", n(5.0)),
                ("WEEKDAY(A1,3)", n(4.0)),
                ("WEEKDAY(A1,9)", e(Num)),
                ("EDATE(A2,1)", n(45351.0)),
                ("EOMONTH(A1,0)", n(45382.0)),
                ("EOMONTH(A1,-1)", n(45351.0)),
                ("DAYS(A1,B1)", n(74.0)),
                ("DATEDIF(B1,A1,\"D\")", n(74.0)),
                ("DATEDIF(\"2001-06-01\",\"2002-08-15\",\"Y\")", n(1.0)),
                ("DATEDIF(\"2001-06-01\",\"2002-08-15\",\"YM\")", n(2.0)),
                ("DATEDIF(\"2001-06-01\",\"2002-08-15\",\"MD\")", n(14.0)),
                ("DATEDIF(\"2001-06-01\",\"2002-08-15\",\"YD\")", n(75.0)),
                ("DATEDIF(A1,B1,\"D\")", e(Num)),
                ("NETWORKDAYS(B1,A1)", n(55.0)),
                ("NETWORKDAYS(B1,A1,B2)", n(54.0)),
                ("DATEVALUE(\"15 Mar 2024\")", n(45366.0)),
            ],
        );
    }
}
