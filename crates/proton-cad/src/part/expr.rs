//! Expressions worked out: a part's parameters, in order, each from those
//! before it; a dimension's value from them.

use std::collections::HashMap;

use super::ast::{Expr, Item, Op, Part};

/// The parameters' values, by name.
pub type Env = HashMap<String, f64>;

/// What an expression comes to, or why it comes to nothing.
pub fn eval(e: &Expr, env: &Env) -> Result<f64, String> {
    let v = match e {
        Expr::Num(x) => *x,
        Expr::Name(n) => *env.get(n).ok_or_else(|| format!("no parameter is called `{n}`"))?,
        Expr::Neg(a) => -eval(a, env)?,
        Expr::Bin(op, a, b) => {
            let (a, b) = (eval(a, env)?, eval(b, env)?);
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => {
                    if b == 0.0 {
                        return Err("a division by nothing".into());
                    }
                    a / b
                }
            }
        }
        Expr::Call(f, args) => {
            let vals = args.iter().map(|a| eval(a, env)).collect::<Result<Vec<f64>, String>>()?;
            let one = |name: &str| -> Result<f64, String> {
                if vals.len() == 1 {
                    Ok(vals[0])
                } else {
                    Err(format!("`{name}` takes one value"))
                }
            };
            match f.as_str() {
                "sqrt" => {
                    let x = one("sqrt")?;
                    if x < 0.0 {
                        return Err("the square root of a negative".into());
                    }
                    x.sqrt()
                }
                "abs" => one("abs")?.abs(),
                // Angles are degrees, as dimensions are.
                "sin" => one("sin")?.to_radians().sin(),
                "cos" => one("cos")?.to_radians().cos(),
                "tan" => one("tan")?.to_radians().tan(),
                "min" | "max" if !vals.is_empty() => {
                    let pick = if f == "min" { f64::min } else { f64::max };
                    vals.iter().copied().fold(vals[0], pick)
                }
                _ => return Err(format!("`{f}` is not a function: sqrt, abs, sin, cos, tan, min, max")),
            }
        }
    };
    if v.is_finite() {
        Ok(v)
    } else {
        Err("a number too big to keep".into())
    }
}

/// The part's parameters, each worked out in order: `(env, mistakes)`, a
/// mistake by the line it is on.
pub fn params(part: &Part) -> (Env, Vec<(usize, String)>) {
    let mut env = Env::new();
    let mut wrong = Vec::new();
    for n in &part.items {
        if let Item::Param { name, value } = &n.item {
            if env.contains_key(name) {
                wrong.push((n.line, format!("`{name}` is a parameter already")));
                continue;
            }
            match eval(value, &env) {
                Ok(v) => {
                    env.insert(name.clone(), v);
                }
                Err(why) => wrong.push((n.line, format!("`{name}`: {why}"))),
            }
        }
    }
    (env, wrong)
}
