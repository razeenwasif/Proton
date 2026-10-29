//! A [`Theory`] written in Proton's language: one statement a line, only
//! the brackets the meaning needs, so what is read back is what was written.

use crate::ast::*;

pub fn print(t: &Theory) -> String {
    let mut out = String::new();
    for line in &t.lines {
        let item = line.item.as_ref().map(item);
        match (item, &line.comment) {
            (Some(i), Some(c)) => out.push_str(&format!("{i}  # {c}")),
            (Some(i), None) => out.push_str(&i),
            (None, Some(c)) => out.push_str(&format!("# {c}")),
            (None, None) => {}
        }
        out.push('\n');
    }
    out
}

pub fn item(i: &Item) -> String {
    match i {
        Item::Sort(s) => sort(s),
        Item::Symbol(s) => symbol(s),
        Item::Constraint(e) => expr(e),
        Item::Question(q) => format!("? {}", question(q)),
    }
}

pub fn sort(s: &SortDecl) -> String {
    let kind = match &s.kind {
        SortKind::Named(n) => n.join(" | "),
        SortKind::Ordered(n) => n.join(" < "),
        SortKind::Range(a, b) => format!("{a}..{b}"),
        SortKind::Size(n) => format!("size {n}"),
        SortKind::Sizes(a, b) => format!("size {a}..{b}"),
    };
    format!("{} = {kind}", s.name)
}

pub fn symbol(s: &SymbolDecl) -> String {
    let mut out = format!("{}: ", s.names.join(", "));
    if !s.args.is_empty() {
        out.push_str(&format!("{} -> ", s.args.join(", ")));
    }
    out.push_str(&s.result);
    for (on, word) in [(s.props.one_to_one, "one-to-one"), (s.props.onto, "onto"), (s.props.partial, "partial"), (s.props.symmetric, "symmetric")] {
        if on {
            out.push_str("  ");
            out.push_str(word);
        }
    }
    out
}

pub fn question(q: &Question) -> String {
    match q {
        Question::Formula(e) => expr(e),
        Question::Entails(p, g) if p.is_empty() => format!("|= {}", expr(g)),
        Question::Entails(p, g) => format!("{} |= {}", p.iter().map(expr).collect::<Vec<_>>().join(", "), expr(g)),
        Question::Same(a, b) => format!("{} == {}", expr(a), expr(b)),
    }
}

pub fn expr(e: &Expr) -> String {
    at(e, 0)
}

/// How tightly an expression holds together as written.
fn level(e: &Expr) -> u8 {
    match e {
        Expr::Quant(..) => 0,
        Expr::Bin(op, ..) => op.level(),
        Expr::Not(_) => 5,
        Expr::Neg(_) => 9,
        _ => 10,
    }
}

/// `e` written where something binding at least `min` is needed.
fn at(e: &Expr, min: u8) -> String {
    let text = bare(e);
    if level(e) < min {
        format!("({text})")
    } else {
        text
    }
}

fn bare(e: &Expr) -> String {
    match e {
        Expr::Name(n) => n.clone(),
        Expr::Num(n) => n.to_string(),
        Expr::Bool(b) => b.to_string(),
        Expr::App(f, args) => format!("{f}({})", args.iter().map(|a| at(a, 0)).collect::<Vec<_>>().join(", ")),
        Expr::End { last, sort } => format!("{}({sort})", if *last { "last" } else { "first" }),
        // A quantifier under `not` is bracketed: it would run on otherwise.
        Expr::Not(a) => format!("not {}", at(a, if matches!(**a, Expr::Quant(..)) { 1 } else { 5 })),
        Expr::Neg(a) => format!("-{}", at(a, 10)),
        Expr::Bin(op, a, b) => {
            let l = op.level();
            let (left, right) = match op {
                Op::Imp => (l + 1, l),
                Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => (l + 1, l + 1),
                _ => (l, l + 1),
            };
            // A quantifier on the left would take in what follows it.
            let left_text = if matches!(**a, Expr::Quant(..)) { format!("({})", bare(a)) } else { at(a, left) };
            format!("{left_text} {} {}", op.text(), at(b, right))
        }
        Expr::Quant(q, binders, body) => {
            let word = match q {
                Quant::All => "all".to_string(),
                Quant::Some => "some".to_string(),
                Quant::Exactly(n) => format!("exactly {n}"),
                Quant::AtLeast(n) => format!("at least {n}"),
                Quant::AtMost(n) => format!("at most {n}"),
            };
            let vars: Vec<String> = binders
                .iter()
                .map(|b| match &b.sort {
                    Some(s) => format!("{} in {s}", b.name),
                    None => b.name.clone(),
                })
                .collect();
            format!("{word} {}: {}", vars.join(", "), at(body, 0))
        }
    }
}
