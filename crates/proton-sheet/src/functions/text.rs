//! Text: joining, cutting, finding and changing case. Positions count
//! characters, from 1, as Excel's do.

use super::*;

pub fn call(cx: &Ctx, name: &str, args: &[Expr]) -> Option<R> {
    let s = |i: usize| text_arg(cx, args, i, None);
    Some(match name {
        "CONCAT" | "CONCATENATE" => (|| {
            let mut out = String::new();
            for a in args {
                if name == "CONCATENATE" {
                    out.push_str(&scalar(cx, a)?.to_text()?);
                } else {
                    each_value(cx, a, |v, _| {
                        out.push_str(&v.to_text()?);
                        Ok(())
                    })?;
                }
            }
            txt(out)
        })(),
        "TEXTJOIN" => (|| {
            arity(args, 3, 255)?;
            let sep = s(0)?;
            let skip = bool_arg(cx, args, 1, true)?;
            let mut parts = Vec::new();
            // Cell by cell, the empty ones too: they are parts unless skipped.
            for a in &args[2..] {
                for v in table(cx, a)?.into_iter().flatten() {
                    let t = v.to_text()?;
                    if !(skip && t.is_empty()) {
                        parts.push(t);
                    }
                }
            }
            txt(parts.join(&sep))
        })(),
        "LEFT" | "RIGHT" => (|| {
            arity(args, 1, 2)?;
            let t: Vec<char> = s(0)?.chars().collect();
            let k = number(cx, args, 1, Some(1.0))?.trunc();
            if k < 0.0 {
                return Err(ErrorKind::Value);
            }
            let k = (k as usize).min(t.len());
            txt(if name == "LEFT" { t[..k].iter().collect::<String>() } else { t[t.len() - k..].iter().collect() })
        })(),
        "MID" => (|| {
            arity(args, 3, 3)?;
            let t: Vec<char> = s(0)?.chars().collect();
            let (start, k) = (number(cx, args, 1, None)?.trunc(), number(cx, args, 2, None)?.trunc());
            if start < 1.0 || k < 0.0 {
                return Err(ErrorKind::Value);
            }
            let from = (start as usize - 1).min(t.len());
            let to = (from + k as usize).min(t.len());
            txt(t[from..to].iter().collect::<String>())
        })(),
        "LEN" => arity(args, 1, 1).and_then(|_| num(s(0)?.chars().count() as f64)),
        "FIND" | "SEARCH" => (|| {
            arity(args, 2, 3)?;
            let (needle, hay) = (s(0)?, s(1)?);
            let start = number(cx, args, 2, Some(1.0))?.trunc();
            let hay: Vec<char> = hay.chars().collect();
            if start < 1.0 || start as usize > hay.len() + 1 {
                return Err(ErrorKind::Value);
            }
            let from = start as usize - 1;
            let found = if name == "FIND" {
                let n: Vec<char> = needle.chars().collect();
                (from..=hay.len().saturating_sub(n.len())).find(|&i| i + n.len() <= hay.len() && hay[i..i + n.len()] == n[..])
            } else {
                // SEARCH: without case, with wildcards; the earliest start
                // from which some stretch matches.
                let pat = format!("{needle}*");
                (from..hay.len().max(from + 1)).find(|&i| wildcard(&pat, &hay[i.min(hay.len())..].iter().collect::<String>()))
            };
            found.map_or(Err(ErrorKind::Value), |i| num(i as f64 + 1.0))
        })(),
        "SUBSTITUTE" => (|| {
            arity(args, 3, 4)?;
            let (t, old, new) = (s(0)?, s(1)?, s(2)?);
            if old.is_empty() {
                return txt(t);
            }
            match args.get(3) {
                None | Some(Expr::Missing) => txt(t.replace(&old, &new)),
                Some(_) => {
                    let k = number(cx, args, 3, None)?.trunc();
                    if k < 1.0 {
                        return Err(ErrorKind::Value);
                    }
                    match t.match_indices(&old).nth(k as usize - 1) {
                        Some((i, _)) => txt(format!("{}{new}{}", &t[..i], &t[i + old.len()..])),
                        None => txt(t),
                    }
                }
            }
        })(),
        "REPLACE" => (|| {
            arity(args, 4, 4)?;
            let t: Vec<char> = s(0)?.chars().collect();
            let (start, k) = (number(cx, args, 1, None)?.trunc(), number(cx, args, 2, None)?.trunc());
            if start < 1.0 || k < 0.0 {
                return Err(ErrorKind::Value);
            }
            let from = (start as usize - 1).min(t.len());
            let to = (from + k as usize).min(t.len());
            txt(format!("{}{}{}", t[..from].iter().collect::<String>(), s(3)?, t[to..].iter().collect::<String>()))
        })(),
        "UPPER" => arity(args, 1, 1).and_then(|_| txt(s(0)?.to_uppercase())),
        "LOWER" => arity(args, 1, 1).and_then(|_| txt(s(0)?.to_lowercase())),
        "PROPER" => arity(args, 1, 1).and_then(|_| {
            let mut out = String::new();
            let mut after_letter = false;
            for c in s(0)?.chars() {
                if after_letter {
                    out.extend(c.to_lowercase());
                } else {
                    out.extend(c.to_uppercase());
                }
                after_letter = c.is_alphabetic();
            }
            txt(out)
        }),
        "TRIM" => arity(args, 1, 1).and_then(|_| txt(s(0)?.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" "))),
        "TEXT" => (|| {
            arity(args, 2, 2)?;
            let v = scalar(cx, &args[0])?;
            let v = match &v {
                Value::Text(t) => parse_number(t).or_else(|| crate::dates::parse_date(t)).map_or(v.clone(), Value::Number),
                _ => v,
            };
            txt(crate::numfmt::show(&v, &s(1)?).text)
        })(),
        "VALUE" => arity(args, 1, 1).and_then(|_| match scalar(cx, &args[0])? {
            Value::Number(n) => num(n),
            Value::Empty => num(0.0),
            v => {
                let t = v.to_text()?;
                parse_number(&t)
                    .or_else(|| crate::dates::parse_date(&t))
                    .or_else(|| crate::dates::parse_time(&t))
                    .map_or(Err(ErrorKind::Value), num)
            }
        }),
        "REPT" => (|| {
            arity(args, 2, 2)?;
            let k = number(cx, args, 1, None)?.trunc();
            if !(0.0..=32_767.0).contains(&k) {
                return Err(ErrorKind::Value);
            }
            txt(s(0)?.repeat(k as usize))
        })(),
        "EXACT" => arity(args, 2, 2).and_then(|_| boolean(s(0)? == s(1)?)),
        "CHAR" => (|| {
            arity(args, 1, 1)?;
            let k = number(cx, args, 0, None)?.trunc();
            if !(1.0..=255.0).contains(&k) {
                return Err(ErrorKind::Value);
            }
            txt(char::from(k as u8).to_string())
        })(),
        "CODE" => arity(args, 1, 1).and_then(|_| s(0)?.chars().next().map_or(Err(ErrorKind::Value), |c| num(c as u32 as f64))),
        "T" => arity(args, 1, 1).and_then(|_| match scalar(cx, &args[0])? {
            Value::Text(t) => txt(t),
            _ => txt(""),
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::functions::testing::*;
    use crate::value::ErrorKind::*;
    use crate::value::Value::Bool;

    #[test]
    fn text_functions_count_characters_as_excel_does() {
        check(
            &[("A1", "Café au lait"), ("A2", "  too   many  spaces "), ("B1", "1"), ("B2", "2"), ("B4", "4"), ("C1", "45366.75"), ("C2", "0.256")],
            &[
                ("LEFT(A1,4)", t("Café")),
                ("RIGHT(A1)", t("t")),
                ("MID(A1,6,2)", t("au")),
                ("MID(A1,20,2)", t("")),
                ("LEN(A1)", n(12.0)),
                ("FIND(\"a\",A1)", n(2.0)),
                ("FIND(\"A\",A1)", e(Value)),
                ("SEARCH(\"A\",A1,3)", n(6.0)),
                ("SEARCH(\"l?it\",A1)", n(9.0)),
                ("SUBSTITUTE(\"a-b-c\",\"-\",\"+\")", t("a+b+c")),
                ("SUBSTITUTE(\"a-b-c\",\"-\",\"+\",2)", t("a-b+c")),
                ("REPLACE(\"abcdef\",2,3,\"X\")", t("aXef")),
                ("UPPER(\"café\")", t("CAFÉ")),
                ("PROPER(\"this is a TITLE's case\")", t("This Is A Title'S Case")),
                ("TRIM(A2)", t("too many spaces")),
                ("CONCAT(B1:B4,\"!\")", t("124!")),
                ("CONCATENATE(\"a\",1,TRUE)", t("a1TRUE")),
                ("TEXTJOIN(\", \",TRUE,B1:B4)", t("1, 2, 4")),
                ("TEXTJOIN(\",\",FALSE,B1:B4)", t("1,2,,4")),
                ("\"x\"&1/4", t("x0.25")),
                ("TEXT(C2,\"0.0%\")", t("25.6%")),
                ("TEXT(C1,\"yyyy-mm-dd hh:mm\")", t("2024-03-15 18:00")),
                ("TEXT(1234.5,\"$#,##0.00\")", t("$1,234.50")),
                ("VALUE(\"1,234\")", n(1234.0)),
                ("VALUE(\"x\")", e(Value)),
                ("REPT(\"ab\",3)", t("ababab")),
                ("EXACT(\"a\",\"A\")", Bool(false)),
                ("CHAR(65)", t("A")),
                ("CODE(\"A\")", n(65.0)),
                ("T(1)", t("")),
            ],
        );
    }
}
