//! A schematic written for other programs: its netlist in KiCad's form,
//! which board programs and simulators read, and its bill of materials.

use crate::build::Built;

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The netlist as KiCad writes it (its `.net`, version E): each part with
/// its value and footprint, each net with its pins and what they do.
pub fn kicad(b: &Built, source: &str) -> String {
    let mut o = String::from("(export (version \"E\")\n");
    o.push_str(&format!("  (design (source {}) (tool \"Proton CAD\"))\n", q(source)));
    o.push_str("  (components\n");
    for p in &b.parts {
        let value = p.value.clone().unwrap_or_else(|| p.symbol.title.clone());
        o.push_str(&format!("    (comp (ref {}) (value {})", q(&p.reference), q(&value)));
        if let Some(f) = &p.footprint {
            o.push_str(&format!(" (footprint {})", q(f)));
        }
        o.push_str(&format!(" (libsource (lib \"proton\") (part {}) (description {})))\n", q(&p.symbol.kind), q(&p.symbol.title)));
    }
    o.push_str("  )\n  (nets\n");
    for (k, n) in b.nets.iter().enumerate() {
        o.push_str(&format!("    (net (code {}) (name {})\n", q(&(k + 1).to_string()), q(&n.name)));
        for &(part, pin) in &n.pins {
            let pl = &b.parts[part];
            let pi = &pl.symbol.pins[pin];
            o.push_str(&format!("      (node (ref {}) (pin {}) (pinfunction {}) (pintype {}))\n", q(&pl.reference), q(&pi.number), q(&pi.name), q(pi.kind.word())));
        }
        o.push_str("    )\n");
    }
    o.push_str("  )\n)\n");
    o
}

fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// The bill of materials as CSV: alike parts on one row.
pub fn bom(b: &Built) -> String {
    let mut rows: Vec<(String, String, String, Vec<String>)> = Vec::new();
    for p in &b.parts {
        let key = (p.symbol.title.clone(), p.value.clone().unwrap_or_default(), p.footprint.clone().unwrap_or_default());
        match rows.iter_mut().find(|r| (r.0.clone(), r.1.clone(), r.2.clone()) == key) {
            Some(r) => r.3.push(p.reference.clone()),
            None => rows.push((key.0, key.1, key.2, vec![p.reference.clone()])),
        }
    }
    let mut o = String::from("Item,Quantity,References,Part,Value,Footprint\n");
    for (k, r) in rows.iter().enumerate() {
        o.push_str(&format!("{},{},{},{},{},{}\n", k + 1, r.3.len(), csv(&r.3.join(" ")), csv(&r.0), csv(&r.1), csv(&r.2)));
    }
    o
}
