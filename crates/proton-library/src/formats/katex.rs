//! Formulas KaTeX drew, read back as TeX.
//!
//! A page made with KaTeX holds its formulas as KaTeX drew them: spans of
//! glyphs, each placed by a style. Where KaTeX kept the TeX beside them — a
//! MathML annotation — that is the formula. Where it did not, as a page
//! drawn once and published does not, the TeX is read back from the drawing:
//! the glyphs in order, a script above or below by where it sits, the parts
//! of a fraction by which is above, a root by its sign, a sum's limits by
//! where they sit, a table's cells by their columns and rows. What is not
//! one of those is read as its glyphs, which is how it looks.
//!
//! Each formula is put back as text — `$…$`, `$$…$$` for one displayed — for
//! the page's reader to take as mathematics.

use dom_query::{Document, NodeRef};

/// Every KaTeX formula on a page, made TeX in its place.
pub fn untypeset(doc: &Document) {
    for display in doc.select(".katex-display").nodes() {
        let tex = display.descendants().into_iter().find(|n| has(n, "katex")).map(|k| tex_of(&k)).unwrap_or_default();
        display.set_text(format!("$${tex}$$"));
    }
    for katex in doc.select(".katex").nodes() {
        let tex = tex_of(katex);
        katex.set_text(if tex.is_empty() { String::new() } else { format!("${tex}$") });
    }
}

/// A formula's TeX: as KaTeX kept it, or as read from its drawing.
pub fn tex_of(katex: &NodeRef) -> String {
    let kept = katex.descendants().into_iter().find(|n| n.node_name().as_deref() == Some("annotation") && n.attr("encoding").as_deref() == Some("application/x-tex"));
    if let Some(annotation) = kept {
        return annotation.text().trim().to_string();
    }
    let drawn = katex.descendants().into_iter().find(|n| has(n, "katex-html"));
    let mut out = String::new();
    if let Some(drawn) = drawn {
        walk(&drawn, &mut out);
    }
    tidy(&out)
}

fn classes(node: &NodeRef) -> String {
    node.attr("class").map(|c| c.to_string()).unwrap_or_default()
}

fn has(node: &NodeRef, class: &str) -> bool {
    classes(node).split_whitespace().any(|c| c == class)
}

/// What a drawn part says, in order.
fn walk(node: &NodeRef, out: &mut String) {
    for child in node.children() {
        if child.is_text() {
            out.push_str(&child.text().replace('\u{200b}', ""));
            continue;
        }
        if !child.is_element() {
            continue;
        }
        let class = classes(&child);
        let is = |c: &str| class.split_whitespace().any(|x| x == c);
        if is("katex-mathml") || is("strut") || is("katex-strut") || is("pstrut") || is("vlist-s") || is("frac-line") || is("hide-tail") || is("nulldelimiter") {
            continue;
        }
        if child.node_name().as_deref() == Some("svg") {
            continue;
        }
        if is("mspace") {
            out.push(' ');
        } else if is("text") {
            // Words in a formula: `\text{for all}`, `\text{s.t.}`.
            let words = child.text().replace('\u{a0}', " ");
            out.push_str(&format!("\\text{{{}}}", words.replace(['{', '}'], "")));
        } else if is("msupsub") {
            scripts(&child, out);
        } else if is("mfrac") {
            fraction(&child, out);
        } else if is("sqrt") {
            root(&child, out);
        } else if is("op-limits") {
            limits(&child, out);
        } else if is("mtable") {
            table(&child, out);
        } else if (is("mop") && !is("op-symbol") && !is("op-limits")) && is_word(&child) {
            out.push_str(&format!("\\operatorname{{{}}}", child.text().trim()));
        } else if let Some(font) = font(&class) {
            let mut inner = String::new();
            walk(&child, &mut inner);
            out.push_str(&format!("\\{font}{{{}}}", tidy(&inner)));
        } else {
            walk(&child, out);
        }
    }
}

/// Whether an operator is a word — `log`, `min` — rather than a sign.
fn is_word(node: &NodeRef) -> bool {
    let text = node.text();
    let text = text.trim();
    text.chars().count() > 1 && text.chars().all(char::is_alphabetic)
}

/// The font a part is set in, when it is not the formula's own.
fn font(class: &str) -> Option<&'static str> {
    let is = |c: &str| class.split_whitespace().any(|x| x == c);
    if is("mathbf") || is("boldsymbol") {
        Some("mathbf")
    } else if is("mathbb") || is("amsrm") {
        Some("mathbb")
    } else if is("mathcal") || is("cal") {
        Some("mathcal")
    } else {
        None
    }
}

/// The rows of a stacked part — a script, a fraction, a sum's limits, a
/// table's column — each with how high it sits: the more negative, the
/// higher.
fn rows<'a>(node: &NodeRef<'a>) -> Vec<(f64, NodeRef<'a>)> {
    let Some(vlist) = node.descendants().into_iter().find(|n| has(n, "vlist")) else { return Vec::new() };
    let mut out: Vec<(f64, NodeRef<'a>)> = vlist
        .element_children()
        .into_iter()
        .filter_map(|row| {
            let style = row.attr("style")?.to_string();
            let top = style.split(';').find_map(|d| d.trim().strip_prefix("top:"))?.trim().trim_end_matches("em").parse::<f64>().ok()?;
            Some((top, row))
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn tex(node: &NodeRef) -> String {
    let mut out = String::new();
    walk(node, &mut out);
    tidy(&out)
}

/// A script: above the line a superscript, below it a subscript.
fn scripts(node: &NodeRef, out: &mut String) {
    let mut sub = None;
    let mut sup = None;
    for (top, row) in rows(node) {
        if top < -2.8 {
            sup = Some(tex(&row));
        } else {
            sub = Some(tex(&row));
        }
    }
    if let Some(sub) = sub {
        out.push_str(&format!("_{{{sub}}}"));
    }
    if let Some(sup) = sup {
        out.push_str(&format!("^{{{sup}}}"));
    }
}

/// A fraction: its numerator the part above its line.
fn fraction(node: &NodeRef, out: &mut String) {
    let parts: Vec<String> = rows(node).iter().filter(|(_, row)| !row.descendants().iter().any(|n| has(n, "frac-line"))).map(|(_, row)| tex(row)).collect();
    match parts.as_slice() {
        [numerator, denominator] => out.push_str(&format!("\\frac{{{numerator}}}{{{denominator}}}")),
        _ => parts.iter().for_each(|p| out.push_str(p)),
    }
}

/// A root: what is under its sign.
fn root(node: &NodeRef, out: &mut String) {
    match node.descendants().into_iter().find(|n| has(n, "svg-align")) {
        Some(under) => out.push_str(&format!("\\sqrt{{{}}}", tex(&under))),
        None => walk(node, out),
    }
}

/// A sum, a product, a limit, its limits above and below: the operator the
/// row between them.
fn limits(node: &NodeRef, out: &mut String) {
    let rows = rows(node);
    let at = rows.iter().position(|(_, row)| row.descendants().iter().any(|n| has(n, "op-symbol") || has(n, "mop")));
    let Some(at) = at else {
        rows.iter().for_each(|(_, row)| out.push_str(&tex(row)));
        return;
    };
    out.push_str(&tex(&rows[at].1));
    if let Some((_, below)) = rows.get(at + 1) {
        out.push_str(&format!("_{{{}}}", tex(below)));
    }
    if at > 0 {
        out.push_str(&format!("^{{{}}}", tex(&rows[at - 1].1)));
    }
}

/// A table — a matrix, aligned equations — by its columns, each a stack
/// of cells, read row by row.
fn table(node: &NodeRef, out: &mut String) {
    let columns: Vec<(bool, Vec<String>)> = node
        .element_children()
        .into_iter()
        .filter(|c| classes(c).split_whitespace().any(|x| x.starts_with("col-align")))
        .map(|c| (has(&c, "col-align-r"), rows(&c).iter().map(|(_, cell)| tex(cell)).collect()))
        .collect();
    let height = columns.iter().map(|(_, cells)| cells.len()).max().unwrap_or(0);
    let lines: Vec<String> = (0..height).map(|r| columns.iter().map(|(_, cells)| cells.get(r).cloned().unwrap_or_default()).collect::<Vec<_>>().join(" & ")).collect();
    let env = if columns.first().is_some_and(|(right, _)| *right) { "aligned" } else { "matrix" };
    out.push_str(&format!("\\begin{{{env}}} {} \\end{{{env}}}", lines.join(" \\\\ ")));
}

/// Glyphs as TeX's names for them, where KaTeX drew them as one: what a
/// reader of the TeX expects.
fn named(c: char) -> Option<&'static str> {
    Some(match c {
        'α' => "\\alpha",
        'β' => "\\beta",
        'γ' => "\\gamma",
        'δ' => "\\delta",
        'ε' | 'ϵ' => "\\epsilon",
        'ζ' => "\\zeta",
        'η' => "\\eta",
        'θ' => "\\theta",
        'ι' => "\\iota",
        'κ' => "\\kappa",
        'λ' => "\\lambda",
        'μ' => "\\mu",
        'ν' => "\\nu",
        'ξ' => "\\xi",
        'π' => "\\pi",
        'ρ' => "\\rho",
        'σ' => "\\sigma",
        'τ' => "\\tau",
        'φ' | 'ϕ' => "\\phi",
        'χ' => "\\chi",
        'ψ' => "\\psi",
        'ω' => "\\omega",
        'Γ' => "\\Gamma",
        'Δ' => "\\Delta",
        'Θ' => "\\Theta",
        'Λ' => "\\Lambda",
        'Π' => "\\Pi",
        'Σ' => "\\Sigma",
        'Φ' => "\\Phi",
        'Ψ' => "\\Psi",
        'Ω' => "\\Omega",
        '−' => "-",
        '≤' => "\\le",
        '≥' => "\\ge",
        '≠' => "\\ne",
        '≈' => "\\approx",
        '×' => "\\times",
        '·' | '⋅' => "\\cdot",
        '∈' => "\\in",
        '∉' => "\\notin",
        '⊆' => "\\subseteq",
        '⊂' => "\\subset",
        '∪' => "\\cup",
        '∩' => "\\cap",
        '∀' => "\\forall",
        '∃' => "\\exists",
        '∞' => "\\infty",
        '→' => "\\to",
        '⇒' => "\\Rightarrow",
        '⇔' => "\\Leftrightarrow",
        '∇' => "\\nabla",
        '∂' => "\\partial",
        '∑' => "\\sum",
        '∏' => "\\prod",
        '∫' => "\\int",
        '⟨' => "\\langle",
        '⟩' => "\\rangle",
        '∥' => "\\|",
        '…' | '⋯' => "\\dots",
        '±' => "\\pm",
        '∘' => "\\circ",
        '⪯' => "\\preceq",
        '⪰' => "\\succeq",
        '≺' => "\\prec",
        '≻' => "\\succ",
        '⊤' => "\\top",
        // The slash KaTeX lays over a relation to negate it: `\neq`.
        '\u{e020}' => "\\not",
        '∣' => "|",
        '‖' => "\\|",
        '⋆' => "\\star",
        '′' => "\\prime",
        '∗' => "*",
        '⊥' => "\\perp",
        '≡' => "\\equiv",
        '∼' => "\\sim",
        '≃' => "\\simeq",
        '≅' => "\\cong",
        '∝' => "\\propto",
        '↦' => "\\mapsto",
        '⟹' => "\\implies",
        '⟺' => "\\iff",
        '∖' => "\\setminus",
        '∅' => "\\emptyset",
        '∧' => "\\land",
        '∨' => "\\lor",
        '¬' => "\\lnot",
        'ℓ' => "\\ell",
        '⊕' => "\\oplus",
        '⊗' => "\\otimes",
        '⌊' => "\\lfloor",
        '⌋' => "\\rfloor",
        '⌈' => "\\lceil",
        '⌉' => "\\rceil",
        'ℝ' => "\\mathbb{R}",
        'ℕ' => "\\mathbb{N}",
        'ℤ' => "\\mathbb{Z}",
        'ℚ' => "\\mathbb{Q}",
        _ => return None,
    })
}

/// Glyphs named, and spaces made one.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match named(c) {
            Some(name) => {
                out.push_str(name);
                // `\theta x`, not `\thetax`.
                if name.chars().last().is_some_and(char::is_alphabetic) && chars.get(i + 1).is_some_and(|n| n.is_alphabetic()) {
                    out.push(' ');
                }
            }
            None if c == '\u{a0}' => out.push(' '),
            None => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/katex.html");

    #[test]
    fn what_katex_drew_is_read_back_as_tex() {
        let doc = Document::from(FIXTURE);
        let formulas: Vec<String> = doc.select(".katex").nodes().iter().map(tex_of).collect();
        assert_eq!(
            formulas,
            [
                "x^{2}",
                "x_{1}",
                "x_{i}^{2}",
                "\\frac{a+1}{b}",
                "\\sqrt{x}",
                "\\sum_{i=1}^{n} x_{i}",
                "f(\\theta x + (1 - \\theta)y) \\le \\theta f(x)",
                "e^{-x^{2}}",
                "x \\not = y",
                "\\operatorname{min}_{x} f(x)\\text{ s.t. }|x| \\ge 0",
                "\\frac{1}{n} \\sum_{i=1}^{n} x_{i}^{2}",
                "\\alpha + \\beta",
            ],
            "the last kept its TeX, and is that"
        );
    }

    #[test]
    fn a_formula_is_put_back_as_text_inline_or_displayed() {
        let doc = Document::from(FIXTURE);
        untypeset(&doc);
        let text = doc.select("article").text().to_string();
        assert!(text.contains("Case 4: $\\frac{a+1}{b}$ done."), "{text}");
        assert!(text.contains("$$\\frac{1}{n} \\sum_{i=1}^{n} x_{i}^{2}$$"), "{text}");
        assert!(doc.select(".katex-html").is_empty(), "nothing drawn is left");
    }
}
