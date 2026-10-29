//! A circuit drawn as SVG, for notes and for Quark's LaTeX.
//!
//! Drawn as the window draws it: each part in its own frame, turned and
//! mirrored as it is, its body sized from its pins — the pins the reader
//! places — gates as their shapes and every other part as a box around its
//! pins; wires, and a dot where three or more ends meet; labels upright. In
//! black on white, a grid step 20 units, so a page can scale it as it likes.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::circ::{Part, PinAt, Project};

const S: f64 = 20.0;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A pin taken back into its part's frame, as the window does.
fn local(p: &Part, x: i32, y: i32) -> (f64, f64) {
    let (mut dx, mut dy) = ((x - p.x) as f64, (y - p.y) as f64);
    for _ in 0..p.rot % 4 {
        (dx, dy) = (-dy, dx);
    }
    if p.mirror {
        dy = -dy;
    }
    (dx, dy)
}

/// A point of a part's frame out on the grid.
fn world(p: &Part, lx: f64, ly: f64) -> (f64, f64) {
    let (mut x, mut y) = (lx, if p.mirror { -ly } else { ly });
    for _ in 0..p.rot % 4 {
        (x, y) = (y, -x);
    }
    (p.x as f64 + x, p.y as f64 + y)
}

/// The box around a part's pins in its frame.
fn frame(p: &Part, pins: &[PinAt]) -> (f64, f64, f64, f64) {
    let mut pts: Vec<(f64, f64)> = pins.iter().map(|q| local(p, q.x, q.y)).collect();
    pts.push((0.0, 0.0));
    let xs = pts.iter().map(|q| q.0);
    let ys = pts.iter().map(|q| q.1);
    (xs.clone().fold(f64::MAX, f64::min), xs.fold(f64::MIN, f64::max), ys.clone().fold(f64::MAX, f64::min), ys.fold(f64::MIN, f64::max))
}

fn box_name(kind: &str) -> String {
    let short = match kind {
        "mux" => "MUX",
        "demux" => "DEMUX",
        "decoder" => "DEC",
        "priority" => "PRI",
        "add" => "+",
        "subtract" => "−",
        "multiply" => "×",
        "divide" => "÷",
        "compare" => "CMP",
        "negate" => "−x",
        "shifter" => "SHIFT",
        "register" => "REG",
        "dff" => "D",
        "tff" => "T",
        "jkff" => "JK",
        "rsff" => "RS",
        "ram2" | "ram" => "RAM",
        "rom" => "ROM",
        "lut" => "LUT",
        "counter" => "CTR",
        "driver" | "driverinv" => "EN",
        "bitext" => "EXT",
        "bitsel" => "BIT",
        "seg7" | "hexdisplay" => "8.",
        "rgbled" => "RGB",
        _ => "",
    };
    if let Some(sub) = kind.strip_prefix("sub:") {
        sub.to_string()
    } else if short.is_empty() {
        kind.to_string()
    } else {
        short.to_string()
    }
}

/// The circuit `circuit` of a project, as an SVG document.
pub fn svg(project: &Project, circuit: &str) -> Result<String, String> {
    let d = project.circuits.get(circuit).ok_or_else(|| format!("there is no circuit {circuit}"))?;
    let pins: HashMap<String, Vec<PinAt>> = project.pins(circuit).into_iter().filter_map(|(id, p)| p.ok().map(|p| (id, p))).collect();
    // How far it reaches, with room for labels.
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut take = |x: f64, y: f64| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    };
    for w in &d.wires {
        take(w.a.0 as f64, w.a.1 as f64);
        take(w.b.0 as f64, w.b.1 as f64);
    }
    for p in &d.parts {
        take(p.x as f64, p.y as f64);
        for q in pins.get(&p.id).into_iter().flatten() {
            take(q.x as f64, q.y as f64);
        }
    }
    if x0 > x1 {
        (x0, y0, x1, y1) = (0.0, 0.0, 4.0, 4.0);
    }
    let (vx, vy, vw, vh) = ((x0 - 4.0) * S, (y0 - 2.0) * S, (x1 - x0 + 8.0) * S, (y1 - y0 + 4.0) * S);
    let mut s = String::new();
    let _ = writeln!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{vx} {vy} {vw} {vh}\" width=\"{vw}\" height=\"{vh}\" font-family=\"Menlo, monospace\" font-size=\"11\">\n<title>{}</title>\n<rect x=\"{vx}\" y=\"{vy}\" width=\"{vw}\" height=\"{vh}\" fill=\"white\"/>",
        esc(circuit)
    );
    // Wires, and the dots where three or more ends meet.
    let _ = writeln!(s, "<g stroke=\"#333\" stroke-width=\"1.6\" stroke-linecap=\"round\">");
    let mut ends: HashMap<(i32, i32), u32> = HashMap::new();
    for w in &d.wires {
        let _ = writeln!(s, "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>", w.a.0 as f64 * S, w.a.1 as f64 * S, w.b.0 as f64 * S, w.b.1 as f64 * S);
        *ends.entry(w.a).or_default() += 1;
        *ends.entry(w.b).or_default() += 1;
    }
    s.push_str("</g>\n");
    for q in pins.values().flatten() {
        if let Some(n) = ends.get_mut(&(q.x, q.y)) {
            *n += 1;
        }
    }
    let mut dots: Vec<_> = ends.into_iter().filter(|(_, n)| *n >= 3).map(|(p, _)| p).collect();
    dots.sort();
    for (x, y) in dots {
        let _ = writeln!(s, "<circle cx=\"{}\" cy=\"{}\" r=\"3\" fill=\"#333\"/>", x as f64 * S, y as f64 * S);
    }
    for p in &d.parts {
        let Some(pp) = pins.get(&p.id) else {
            let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"60\" height=\"40\" fill=\"none\" stroke=\"#c33\" stroke-dasharray=\"4 3\"/>", (p.x as f64 - 0.5) * S, (p.y as f64 - 0.5) * S);
            continue;
        };
        part(&mut s, p, pp);
    }
    s.push_str("</svg>\n");
    Ok(s)
}

fn text(s: &mut String, t: &str, (x, y): (f64, f64), anchor: &str, colour: &str) {
    if t.is_empty() {
        return;
    }
    let _ = writeln!(s, "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"{anchor}\" dominant-baseline=\"central\" fill=\"{colour}\">{}</text>", x * S, y * S, esc(t));
}

fn part(s: &mut String, p: &Part, pins: &[PinAt]) {
    let (fx0, fx1, fy0, fy1) = frame(p, pins);
    let label = p.attrs.get("label").cloned().unwrap_or_default();
    let mirror = if p.mirror { " scale(1,-1)" } else { "" };
    let _ = writeln!(s, "<g transform=\"translate({} {}) rotate({}){mirror}\" fill=\"white\" stroke=\"#111\" stroke-width=\"1.6\" stroke-linejoin=\"round\">", p.x as f64 * S, p.y as f64 * S, -90 * (p.rot % 4) as i32);
    let k = p.kind.as_str();
    let gate = matches!(k, "and" | "nand" | "or" | "nor" | "xor" | "xnor");
    let inverted = matches!(k, "nand" | "nor" | "xnor" | "not");
    let mut words: Vec<(String, (f64, f64), &str, &str)> = Vec::new();
    if gate {
        let x1 = fx1 - if inverted { 1.0 } else { 0.0 };
        let (top, bot) = (fy0 - 0.5, fy1 + 0.5);
        let (mid, half) = ((top + bot) / 2.0, (bot - top) / 2.0);
        if k == "and" || k == "nand" {
            let flat = fx0.max(x1 - half);
            let _ = writeln!(s, "<path d=\"M{} {} H{} A{} {} 0 0 1 {} {} H{} Z\"/>", fx0 * S, top * S, flat * S, half * S, half * S, flat * S, bot * S, fx0 * S);
        } else {
            let back = fx0 + if k.contains("xor") || k.contains("xnor") { 0.35 } else { 0.0 };
            let c = back + (x1 - back) * 0.6;
            let _ = writeln!(s, "<path d=\"M{b} {t} Q{c} {t} {x} {m} Q{c} {bt} {b} {bt} Q{bb} {m} {b} {t} Z\"/>", b = back * S, t = top * S, c = c * S, x = x1 * S, m = mid * S, bt = bot * S, bb = (back + 0.6) * S);
            if k.contains("xor") || k.contains("xnor") {
                let _ = writeln!(s, "<path fill=\"none\" d=\"M{} {} Q{} {} {} {}\"/>", fx0 * S, top * S, (fx0 + 0.6) * S, mid * S, fx0 * S, bot * S);
            }
        }
        if inverted {
            let _ = writeln!(s, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>", (x1 + 0.3) * S, mid * S, 0.28 * S);
        }
    } else {
        match k {
            "not" => {
                let x1 = fx1 - 0.5;
                let _ = writeln!(s, "<path d=\"M0 {} L{} 0 L0 {} Z\"/><circle cx=\"{}\" cy=\"0\" r=\"{}\"/>", -0.7 * S, (x1 - 0.3) * S, 0.7 * S, (x1 - 0.15) * S, 0.28 * S);
            }
            "in" | "clock" | "button" => {
                let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", -1.4 * S, -0.5 * S, 1.1 * S, S);
                words.push((label.clone(), world(p, -1.6, 0.0), "end", "#111"));
            }
            "out" | "probe" => {
                let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", 0.3 * S, -0.5 * S, 1.1 * S, S);
                words.push((label.clone(), world(p, 1.6, 0.0), "start", "#111"));
            }
            "led" => {
                let _ = writeln!(s, "<circle cx=\"{}\" cy=\"0\" r=\"{}\"/>", 0.85 * S, 0.55 * S);
                words.push((label.clone(), world(p, 1.6, 0.0), "start", "#111"));
            }
            "const" => words.push((p.attrs.get("value").cloned().unwrap_or_else(|| "1".into()), world(p, -0.4, 0.0), "end", "#111")),
            "ground" => {
                let _ = writeln!(s, "<path fill=\"none\" d=\"M0 0 V{a} M{l} {a} H{r} M{l2} {b} H{r2} M{l3} {c} H{r3}\"/>", a = 0.4 * S, l = -0.5 * S, r = 0.5 * S, b = 0.6 * S, l2 = -0.3 * S, r2 = 0.3 * S, c = 0.8 * S, l3 = -0.12 * S, r3 = 0.12 * S);
            }
            "vdd" => {
                let _ = writeln!(s, "<path fill=\"none\" d=\"M0 0 V{a} M{l} {a} H{r}\"/>", a = -0.4 * S, l = -0.5 * S, r = 0.5 * S);
            }
            "tunnel" => {
                let _ = writeln!(s, "<path d=\"M0 0 L{} {} L{} {} Z\"/>", 0.6 * S, -0.35 * S, 0.6 * S, 0.35 * S);
                words.push((p.attrs.get("net").cloned().unwrap_or_default(), world(p, 0.8, 0.0), "start", "#555"));
            }
            "split" => {
                let _ = writeln!(s, "<rect fill=\"#111\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", (fx0 + 0.35) * S, (fy0 - 0.3) * S, 0.3 * S, (fy1 - fy0 + 0.6) * S);
            }
            "pullup" | "pulldown" => {
                let dir = if k == "pullup" { -1.0 } else { 1.0 };
                let _ = writeln!(s, "<path fill=\"none\" d=\"M0 0 V{}\"/><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", 0.3 * dir * S, -0.18 * S, (if dir < 0.0 { -1.0 } else { 0.3 }) * S, 0.36 * S, 0.7 * S);
            }
            "text" => words.push((p.attrs.get("text").cloned().unwrap_or_default(), world(p, 0.0, 0.0), "start", "#555")),
            "reset" | "break" => {
                let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", -1.4 * S, -0.5 * S, 1.1 * S, S);
                words.push((k.to_uppercase(), world(p, -0.85, 0.0), "middle", "#111"));
            }
            _ => {
                let (x0, x1) = (fx0, fx1.max(fx0 + 2.0));
                let (y0, y1) = if matches!(k, "seg7" | "hexdisplay" | "rgbled") { (-0.5, fy1.max(2.0) + 0.5) } else { (fy0 - 0.5, fy1 + 0.5) };
                let x1 = if matches!(k, "seg7" | "hexdisplay" | "rgbled") { 3.0 } else { x1 };
                let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", x0 * S, y0 * S, (x1 - x0) * S, (y1 - y0) * S);
                words.push((box_name(k), world(p, (x0 + x1) / 2.0, (y0 + y1) / 2.0), "middle", "#111"));
                words.push((label.clone(), world(p, (x0 + x1) / 2.0, y0 - 0.45), "middle", "#555"));
            }
        }
    }
    // Pins: inputs hollow, outputs filled.
    for q in pins {
        let (x, y) = local(p, q.x, q.y);
        let _ = writeln!(s, "<circle cx=\"{}\" cy=\"{}\" r=\"2.4\" fill=\"{}\"/>", x * S, y * S, if q.input { "white" } else { "#111" });
    }
    s.push_str("</g>\n");
    if gate && !label.is_empty() {
        words.push((label, world(p, (fx0 + fx1) / 2.0, fy0 - 1.0), "middle", "#555"));
    }
    for (t, at, anchor, colour) in words {
        text(s, &t, at, anchor, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circuit_is_drawn_as_svg() {
        let mut p = Project::new();
        p.circuits.insert(
            "main".into(),
            crate::circ::parse("part A in at 0,0 label=A\npart B in at 0,2 label=\"B & C\"\npart G nand at 3,0 label=gate\npart Y out at 8,1 label=Y\npart Z frob at 9,9\nwire 0,0 3,0\nwire 0,2 3,2\nwire 7,1 8,1\n").unwrap(),
        );
        let svg = svg(&p, "main").unwrap();
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("<title>main</title>") && svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<line ").count(), 3);
        assert!(svg.contains(" A") && svg.contains("B &amp; C"), "labels, escaped");
        assert!(svg.contains("stroke-dasharray"), "a part that cannot be placed is marked");
        assert!(svg.matches("<path").count() >= 1 && svg.contains("<circle"), "the gate's body and its bubble");
        assert!(super::svg(&p, "nowhere").is_err());
    }
}
