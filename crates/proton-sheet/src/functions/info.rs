//! Information: what kind of value something is.

use super::*;

pub fn call(cx: &Ctx, name: &str, args: &[Expr]) -> Option<R> {
    if name == "NA" {
        return Some(arity(args, 0, 0).and(Err(ErrorKind::NA)));
    }
    let test: fn(&Value) -> bool = match name {
        "ISBLANK" => |v| v.is_empty(),
        "ISNUMBER" => |v| matches!(v, Value::Number(_)),
        "ISTEXT" => |v| matches!(v, Value::Text(_)),
        "ISNONTEXT" => |v| !matches!(v, Value::Text(_)),
        "ISLOGICAL" => |v| matches!(v, Value::Bool(_)),
        "ISERROR" => |v| matches!(v, Value::Error(_)),
        "ISERR" => |v| matches!(v, Value::Error(e) if *e != ErrorKind::NA),
        "ISNA" => |v| matches!(v, Value::Error(ErrorKind::NA)),
        "N" => {
            return Some(arity(args, 1, 1).and_then(|_| match cx.value(&args[0]) {
                Value::Number(n) => num(n),
                Value::Bool(b) => num(if b { 1.0 } else { 0.0 }),
                Value::Error(e) => Err(e),
                _ => num(0.0),
            }))
        }
        _ => return None,
    };
    Some(arity(args, 1, 1).and_then(|_| boolean(test(&cx.value(&args[0])))))
}

#[cfg(test)]
mod tests {
    use crate::functions::testing::*;
    use crate::value::ErrorKind::*;
    use crate::value::Value::Bool;

    #[test]
    fn kinds_are_told_apart() {
        check(
            &[("A1", "5"), ("A2", "x"), ("A3", "=1/0"), ("A4", "=\"\"")],
            &[
                ("ISBLANK(B1)", Bool(true)),
                ("ISBLANK(A4)", Bool(false)),
                ("ISNUMBER(A1)", Bool(true)),
                ("ISNUMBER(\"5\")", Bool(false)),
                ("ISTEXT(A2)", Bool(true)),
                ("ISERROR(A3)", Bool(true)),
                ("ISERR(NA())", Bool(false)),
                ("ISNA(NA())", Bool(true)),
                ("ISLOGICAL(TRUE)", Bool(true)),
                ("NA()", e(NA)),
                ("N(TRUE)", n(1.0)),
                ("N(A2)", n(0.0)),
                ("NOSUCH(1)", e(Name)),
            ],
        );
    }
}
