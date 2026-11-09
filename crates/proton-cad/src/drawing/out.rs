//! A sheet written out: SVG for the screen and the web, PDF for print and
//! sharing, DXF (R12) for a machinist's CAM and other CAD programs — each
//! of Proton's own writing. Pens are ISO 128's lines: visible thick, the
//! rest thin; hidden dashed, centre lines long-dash-dot. A blueprint is
//! drawn white on blue.

use std::fmt::Write as _;

use super::{Anchor, Pen, Prim, Sheet, Style, P2};

fn colors(style: Style) -> (&'static str, &'static str, &'static str) {
    match style {
        Style::Paper => ("#ffffff", "#111111", "#1a4a8a"),
        Style::Blueprint => ("#1d4e89", "#f2f6ff", "#bcd3f5"),
    }
}

fn dash(pen: Pen) -> Option<&'static [f64]> {
    match pen {
        Pen::Hidden => Some(&[3.0, 1.5]),
        Pen::Center => Some(&[8.0, 1.5, 1.0, 1.5]),
        _ => None,
    }
}

fn n(x: f64) -> String {
    let s = format!("{:.3}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The sheet as SVG: millimetres, y down as SVG has it.
pub fn svg(sheet: &Sheet) -> String {
    let (bg, ink, dim) = colors(sheet.style);
    let (w, h) = (sheet.width, sheet.height);
    let y = |p: P2| h - p.y;
    let mut o = String::new();
    let _ = writeln!(o, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}mm" height="{}mm" viewBox="0 0 {} {}" font-family="Helvetica, Arial, sans-serif">"#, n(w), n(h), n(w), n(h));
    let _ = writeln!(o, r#"<rect width="{}" height="{}" fill="{bg}"/>"#, n(w), n(h));
    let stroke = |pen: Pen| {
        let c = if pen == Pen::Dim { dim } else { ink };
        let d = dash(pen).map_or(String::new(), |d| format!(r#" stroke-dasharray="{}""#, d.iter().map(|x| n(*x)).collect::<Vec<_>>().join(" ")));
        format!(r#"fill="none" stroke="{c}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"{d}"#, n(pen.width()))
    };
    for p in &sheet.prims {
        match p {
            Prim::Line { a, b, pen } => {
                let _ = writeln!(o, r#"<line x1="{}" y1="{}" x2="{}" y2="{}" {}/>"#, n(a.x), n(y(*a)), n(b.x), n(y(*b)), stroke(*pen));
            }
            Prim::Poly { pts, closed, pen } => {
                let d: Vec<String> = pts.iter().map(|q| format!("{},{}", n(q.x), n(y(*q)))).collect();
                let _ = writeln!(o, r#"<{} points="{}" {}/>"#, if *closed { "polygon" } else { "polyline" }, d.join(" "), stroke(*pen));
            }
            Prim::Circle { c, r, pen } => {
                let _ = writeln!(o, r#"<circle cx="{}" cy="{}" r="{}" {}/>"#, n(c.x), n(y(*c)), n(*r), stroke(*pen));
            }
            Prim::Fill { pts, pen } => {
                let c = if *pen == Pen::Dim { dim } else { ink };
                let d: Vec<String> = pts.iter().map(|q| format!("{},{}", n(q.x), n(y(*q)))).collect();
                let _ = writeln!(o, r#"<polygon points="{}" fill="{c}"/>"#, d.join(" "));
            }
            Prim::Text { at, size, text, anchor, angle, pen } => {
                let c = if *pen == Pen::Dim { dim } else { ink };
                let a = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let turn = if angle.abs() > 1e-9 { format!(r#" transform="rotate({} {} {})""#, n(-angle), n(at.x), n(y(*at))) } else { String::new() };
                let _ = writeln!(o, r#"<text x="{}" y="{}" font-size="{}" text-anchor="{a}" fill="{c}"{turn}>{}</text>"#, n(at.x), n(y(*at)), n(*size), esc(text));
            }
        }
    }
    o.push_str("</svg>\n");
    o
}

/* ── PDF ───────────────────────────────────────────────────────────── */

/// Text in Helvetica's WinAnsi encoding: Ø, °, ×, ± and the like as
/// themselves, what it has not as `?`.
fn win_ansi(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        let b: u32 = match c {
            '(' | ')' | '\\' => {
                o.push('\\');
                o.push(c);
                continue;
            }
            '–' => 0x96,
            '—' => 0x97,
            c if (c as u32) < 0x80 => c as u32,
            c if (0xA0..=0xFF).contains(&(c as u32)) => c as u32,
            _ => '?' as u32,
        };
        if b < 0x80 {
            o.push(char::from_u32(b).unwrap_or('?'));
        } else {
            let _ = write!(o, "\\{:03o}", b);
        }
    }
    o
}

/// Helvetica's width, roughly: half its height a letter.
fn width(s: &str, size: f64) -> f64 {
    s.chars().count() as f64 * size * 0.52
}

/// The sheet as a one-page PDF, in points.
pub fn pdf(sheet: &Sheet) -> Vec<u8> {
    let k = 72.0 / 25.4;
    let (bg, ink, dim) = colors(sheet.style);
    let rgb = |hex: &str| {
        let v = u32::from_str_radix(&hex[1..], 16).unwrap_or(0);
        format!("{} {} {}", n(((v >> 16) & 255) as f64 / 255.0), n(((v >> 8) & 255) as f64 / 255.0), n((v & 255) as f64 / 255.0))
    };
    let mut c = String::new();
    let _ = writeln!(c, "{} rg 0 0 {} {} re f", rgb(bg), n(sheet.width * k), n(sheet.height * k));
    let pt = |p: P2| format!("{} {}", n(p.x * k), n(p.y * k));
    let set = |c: &mut String, pen: Pen| {
        let col = if pen == Pen::Dim { dim } else { ink };
        let d = dash(pen).map_or("[] 0 d".to_string(), |d| format!("[{}] 0 d", d.iter().map(|x| n(x * k)).collect::<Vec<_>>().join(" ")));
        let _ = writeln!(c, "{} RG {} rg {} w 1 J 1 j {d}", rgb(col), rgb(col), n(pen.width() * k));
    };
    for p in &sheet.prims {
        match p {
            Prim::Line { a, b, pen } => {
                set(&mut c, *pen);
                let _ = writeln!(c, "{} m {} l S", pt(*a), pt(*b));
            }
            Prim::Poly { pts, closed, pen } => {
                if pts.len() < 2 {
                    continue;
                }
                set(&mut c, *pen);
                let _ = write!(c, "{} m", pt(pts[0]));
                for q in &pts[1..] {
                    let _ = write!(c, " {} l", pt(*q));
                }
                let _ = writeln!(c, "{}", if *closed { " s" } else { " S" });
            }
            Prim::Circle { c: ctr, r, pen } => {
                set(&mut c, *pen);
                // Four Béziers.
                let m = 0.552_284_749_8 * r;
                let (x, y) = (ctr.x, ctr.y);
                let q = |a: f64, b: f64| pt(P2::new(a, b));
                let _ = writeln!(
                    c,
                    "{} m {} {} {} c {} {} {} c {} {} {} c {} {} {} c S",
                    q(x + r, y),
                    q(x + r, y + m),
                    q(x + m, y + r),
                    q(x, y + r),
                    q(x - m, y + r),
                    q(x - r, y + m),
                    q(x - r, y),
                    q(x - r, y - m),
                    q(x - m, y - r),
                    q(x, y - r),
                    q(x + m, y - r),
                    q(x + r, y - m),
                    q(x + r, y)
                );
            }
            Prim::Fill { pts, pen } => {
                set(&mut c, *pen);
                let _ = write!(c, "{} m", pt(pts[0]));
                for q in &pts[1..] {
                    let _ = write!(c, " {} l", pt(*q));
                }
                let _ = writeln!(c, " f");
            }
            Prim::Text { at, size, text, anchor, angle, pen } => {
                set(&mut c, *pen);
                let shift = match anchor {
                    Anchor::Start => 0.0,
                    Anchor::Middle => -width(text, *size) / 2.0,
                    Anchor::End => -width(text, *size),
                };
                let (s, co) = angle.to_radians().sin_cos();
                let origin = *at + P2::new(co, s) * shift;
                // The text matrix: the font at the point, turned.
                let _ = writeln!(c, "BT /F1 {} Tf {} {} {} {} {} Tm ({}) Tj ET", n(size * k), n(co), n(s), n(-s), n(co), pt(origin), win_ansi(text));
            }
        }
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>", n(sheet.width * k), n(sheet.height * k)),
        format!("<< /Length {} >>\nstream\n{}endstream", c.len(), c),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        "<< /Producer (Proton CAD) >>".to_string(),
    ];
    let mut out = String::from("%PDF-1.4\n%\u{e2}\u{e3}\u{cf}\u{d3}\n");
    let mut offsets = Vec::new();
    let mut bytes = out.clone().into_bytes();
    out.clear();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, obj).as_bytes());
    }
    let xref = bytes.len();
    let mut x = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for o in &offsets {
        let _ = writeln!(x, "{:010} 00000 n ", o);
    }
    let _ = write!(x, "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{}\n%%EOF\n", objects.len() + 1, xref);
    bytes.extend_from_slice(x.as_bytes());
    bytes
}

/* ── DXF ───────────────────────────────────────────────────────────── */

/// The sheet as DXF R12: each pen a layer with its line type, in
/// millimetres.
pub fn dxf(sheet: &Sheet) -> String {
    let mut o = String::new();
    let mut g = |code: i32, value: &str| {
        let _ = writeln!(o, "{code:>3}\n{value}");
    };
    g(0, "SECTION");
    g(2, "HEADER");
    g(9, "$ACADVER");
    g(1, "AC1009");
    g(9, "$INSUNITS");
    g(70, "4");
    g(0, "ENDSEC");
    g(0, "SECTION");
    g(2, "TABLES");
    g(0, "TABLE");
    g(2, "LTYPE");
    g(70, "3");
    for (name, pattern) in [("CONTINUOUS", vec![]), ("HIDDEN", vec![3.0, -1.5]), ("CENTER", vec![8.0, -1.5, 1.0, -1.5])] {
        g(0, "LTYPE");
        g(2, name);
        g(70, "0");
        g(3, name);
        g(72, "65");
        g(73, &pattern.len().to_string());
        g(40, &n(pattern.iter().map(|x: &f64| x.abs()).sum()));
        for d in pattern {
            g(49, &n(d));
        }
    }
    g(0, "ENDTAB");
    g(0, "TABLE");
    g(2, "LAYER");
    g(70, "6");
    for pen in [Pen::Visible, Pen::Hidden, Pen::Center, Pen::Thin, Pen::Dim, Pen::Frame] {
        g(0, "LAYER");
        g(2, pen.layer());
        g(70, "0");
        g(62, match pen {
            Pen::Visible | Pen::Frame => "7",
            Pen::Hidden => "8",
            Pen::Center => "1",
            Pen::Thin => "9",
            Pen::Dim => "5",
        });
        g(6, match pen {
            Pen::Hidden => "HIDDEN",
            Pen::Center => "CENTER",
            _ => "CONTINUOUS",
        });
    }
    g(0, "ENDTAB");
    g(0, "ENDSEC");
    g(0, "SECTION");
    g(2, "ENTITIES");
    let seg = |o: &mut Vec<(i32, String)>, a: P2, b: P2, pen: Pen| {
        o.extend([(0, "LINE".into()), (8, pen.layer().into()), (10, n(a.x)), (20, n(a.y)), (30, "0".into()), (11, n(b.x)), (21, n(b.y)), (31, "0".into())]);
    };
    let mut ents: Vec<(i32, String)> = Vec::new();
    for p in &sheet.prims {
        match p {
            Prim::Line { a, b, pen } => seg(&mut ents, *a, *b, *pen),
            Prim::Poly { pts, closed, pen } => {
                for w in pts.windows(2) {
                    seg(&mut ents, w[0], w[1], *pen);
                }
                if *closed && pts.len() > 2 {
                    seg(&mut ents, pts[pts.len() - 1], pts[0], *pen);
                }
            }
            Prim::Circle { c, r, pen } => ents.extend([(0, "CIRCLE".into()), (8, pen.layer().into()), (10, n(c.x)), (20, n(c.y)), (30, "0".into()), (40, n(*r))]),
            Prim::Fill { pts, pen } => {
                if pts.len() == 3 {
                    ents.extend([(0, "SOLID".into()), (8, pen.layer().into())]);
                    for (k, q) in [pts[0], pts[1], pts[2], pts[2]].iter().enumerate() {
                        ents.extend([(10 + k as i32, n(q.x)), (20 + k as i32, n(q.y)), (30 + k as i32, "0".into())]);
                    }
                }
            }
            Prim::Text { at, size, text, anchor, angle, pen } => {
                let h = match anchor {
                    Anchor::Start => 0,
                    Anchor::Middle => 1,
                    Anchor::End => 2,
                };
                ents.extend([(0, "TEXT".into()), (8, pen.layer().into()), (10, n(at.x)), (20, n(at.y)), (30, "0".into()), (40, n(*size)), (1, text.replace('\n', " ")), (50, n(*angle))]);
                if h != 0 {
                    ents.extend([(72, h.to_string()), (11, n(at.x)), (21, n(at.y)), (31, "0".into())]);
                }
            }
        }
    }
    for (code, value) in ents {
        g(code, &value);
    }
    g(0, "ENDSEC");
    g(0, "EOF");
    o
}

/// A DXF's entities read back: each one's kind and layer.
pub fn read_dxf(text: &str) -> Result<Vec<(String, String)>, String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() % 2 != 0 {
        return Err("a DXF is pairs of lines: a code and a value".into());
    }
    let mut out = Vec::new();
    let mut in_entities = false;
    let mut cur: Option<(String, String)> = None;
    for pair in lines.chunks(2) {
        let code: i32 = pair[0].trim().parse().map_err(|_| format!("`{}` is not a group code", pair[0]))?;
        let value = pair[1];
        match (code, value) {
            (2, "ENTITIES") => in_entities = true,
            (0, "ENDSEC") => {
                if let Some(c) = cur.take() {
                    out.push(c);
                }
                in_entities = false;
            }
            (0, kind) if in_entities => {
                if let Some(c) = cur.take() {
                    out.push(c);
                }
                cur = Some((kind.to_string(), String::new()));
            }
            (8, layer) if in_entities => {
                if let Some(c) = cur.as_mut() {
                    c.1 = layer.to_string();
                }
            }
            _ => {}
        }
    }
    if !text.trim_end().ends_with("EOF") {
        return Err("the DXF does not end with EOF".into());
    }
    Ok(out)
}
