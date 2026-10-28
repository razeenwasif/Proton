//! Logic: IF and its kin, which work out only the branch they take.

use super::*;

pub fn call(cx: &Ctx, name: &str, args: &[Expr]) -> Option<R> {
    Some(match name {
        "IF" => (|| {
            arity(args, 1, 3)?;
            let cond = scalar(cx, &args[0])?.to_bool()?;
            match (cond, args.get(1), args.get(2)) {
                (true, Some(e), _) if !matches!(e, Expr::Missing) => Ok(cx.eval(e)),
                (true, _, _) => num(0.0),
                (false, _, Some(e)) if !matches!(e, Expr::Missing) => Ok(cx.eval(e)),
                (false, _, Some(_)) => num(0.0),
                (false, _, None) => boolean(false),
            }
        })(),
        "IFS" => (|| {
            if args.is_empty() || args.len() % 2 != 0 {
                return Err(ErrorKind::Value);
            }
            for pair in args.chunks(2) {
                if scalar(cx, &pair[0])?.to_bool()? {
                    return Ok(cx.eval(&pair[1]));
                }
            }
            Err(ErrorKind::NA)
        })(),
        "AND" | "OR" | "XOR" => (|| {
            let mut seen = Vec::new();
            for a in args {
                each_value(cx, a, |v, direct| {
                    match v {
                        Value::Bool(b) => seen.push(b),
                        Value::Number(n) => seen.push(n != 0.0),
                        Value::Error(e) => return Err(e),
                        Value::Text(t) if direct => seen.push(Value::Text(t).to_bool()?),
                        _ => {}
                    }
                    Ok(())
                })?;
            }
            if seen.is_empty() {
                return Err(ErrorKind::Value);
            }
            boolean(match name {
                "AND" => seen.iter().all(|b| *b),
                "OR" => seen.iter().any(|b| *b),
                _ => seen.iter().filter(|b| **b).count() % 2 == 1,
            })
        })(),
        "NOT" => arity(args, 1, 1).and_then(|_| boolean(!scalar(cx, &args[0])?.to_bool()?)),
        "TRUE" => arity(args, 0, 0).and_then(|_| boolean(true)),
        "FALSE" => arity(args, 0, 0).and_then(|_| boolean(false)),
        "IFERROR" | "IFNA" => (|| {
            arity(args, 2, 2)?;
            let ev = cx.eval(&args[0]);
            let caught = match cx.scalar(ev.clone()) {
                Value::Error(ErrorKind::NA) => true,
                Value::Error(_) => name == "IFERROR",
                _ => false,
            };
            Ok(if caught { cx.eval(&args[1]) } else { ev })
        })(),
        "SWITCH" => (|| {
            if args.len() < 3 {
                return Err(ErrorKind::Value);
            }
            let x = scalar(cx, &args[0])?;
            let rest = &args[1..];
            for pair in rest.chunks(2) {
                match pair {
                    [case, result] => {
                        if scalar(cx, case)?.compare(&x) == std::cmp::Ordering::Equal {
                            return Ok(cx.eval(result));
                        }
                    }
                    [default] => return Ok(cx.eval(default)),
                    _ => {}
                }
            }
            Err(ErrorKind::NA)
        })(),
        "CHOOSE" => (|| {
            arity(args, 2, 255)?;
            let i = number(cx, args, 0, None)?.trunc();
            if i < 1.0 || i as usize >= args.len() {
                return Err(ErrorKind::Value);
            }
            Ok(cx.eval(&args[i as usize]))
        })(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::functions::testing::*;
    use crate::value::ErrorKind::*;
    use crate::value::Value::Bool;

    #[test]
    fn logic_takes_only_its_branch() {
        check(
            &[("A1", "5"), ("A2", "0"), ("A3", "x"), ("B1", "1"), ("B2", "2")],
            &[
                ("IF(A1>3,\"big\",\"small\")", t("big")),
                ("IF(A2,1/0,\"fine\")", t("fine")),
                ("IF(FALSE,1)", Bool(false)),
                ("IF(TRUE,,1)", n(0.0)),
                ("IF(A3,1,2)", e(Value)),
                ("IFS(A1>9,\"a\",A1>4,\"b\")", t("b")),
                ("IFS(A1>9,\"a\")", e(NA)),
                ("AND(A1,TRUE)", Bool(true)),
                ("AND(A1:A3)", Bool(false)),
                ("OR(A2,FALSE)", Bool(false)),
                ("XOR(TRUE,TRUE,TRUE)", Bool(true)),
                ("NOT(A2)", Bool(true)),
                ("AND(A3)", e(Value)),
                ("IFERROR(1/0,\"none\")", t("none")),
                ("IFERROR(A1,\"none\")", n(5.0)),
                ("IFNA(1/0,\"none\")", e(Div0)),
                ("IFNA(NA(),\"none\")", t("none")),
                ("SWITCH(A1,1,\"one\",5,\"five\",\"other\")", t("five")),
                ("SWITCH(A1,1,\"one\",\"other\")", t("other")),
                ("SWITCH(A1,1,\"one\",2,\"two\")", e(NA)),
                ("CHOOSE(2,\"a\",\"b\",\"c\")", t("b")),
                ("SUM(CHOOSE(2,A1:A2,B1:B2))", n(3.0)),
                ("CHOOSE(4,\"a\",\"b\",\"c\")", e(Value)),
            ],
        );
    }
}
