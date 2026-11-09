//! A schematic built: each part's symbol made and placed, each net's pins
//! found, and the electrical rules checked.
//!
//! The rules: a reference used twice; a pin that is not there, or on two
//! nets, or marked unconnected and connected; two outputs driving one net
//! (an output, a supply's output); a power input on a net nothing on the
//! board supplies and that is not said to be `power`; an input nothing
//! drives; a net of one pin; pins left unconnected without `nc`; and a
//! part whose pinout is not yet checked against its standard.

use std::collections::HashMap;

use proton_cad::part::ast::Diag;

use crate::lang::{Item, PinRef, Schematic};
use crate::symbol::{make, PinKind, Symbol};

/// A part as placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub reference: String,
    pub line: usize,
    pub index: usize,
    pub symbol: Symbol,
    pub value: Option<String>,
    pub footprint: Option<String>,
    /// Its middle on the sheet, mm; placed for it if the file does not say.
    pub at: [f64; 2],
    pub placed: bool,
    pub turn: u32,
    pub mirror: bool,
}

/// A net as built: its pins, by part and pin.
#[derive(Debug, Clone, PartialEq)]
pub struct NetOut {
    pub name: String,
    pub line: usize,
    pub index: usize,
    pub power: bool,
    pub current: Option<f64>,
    pub pins: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

/// What the rules check found, at its line.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub line: usize,
    pub level: Level,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    pub title: Option<String>,
    pub sheet: String,
    pub parts: Vec<Placed>,
    pub nets: Vec<NetOut>,
    /// Pins marked to be left unconnected.
    pub nc: Vec<(usize, usize)>,
    pub findings: Vec<Finding>,
}

impl Built {
    /// The net a pin is on.
    pub fn net_of(&self, part: usize, pin: usize) -> Option<&NetOut> {
        self.nets.iter().find(|n| n.pins.contains(&(part, pin)))
    }

    pub fn errors(&self) -> usize {
        self.findings.iter().filter(|f| f.level == Level::Error).count()
    }
}

/// A schematic's sheet size, as `size` takes it.
pub fn sheet_size(name: &str) -> (f64, f64) {
    proton_cad::drawing::size(&name.replace('_', "-")).unwrap_or((297.0, 210.0))
}

/// The schematic `s` built and checked.
pub fn build(s: &Schematic) -> Built {
    let mut findings = Vec::new();
    let err = |line: usize, m: String| Finding { line, level: Level::Error, message: m };
    let warn = |line: usize, m: String| Finding { line, level: Level::Warning, message: m };
    let (mut title, mut sheet) = (None, "A4".to_string());
    for l in &s.items {
        match &l.item {
            Item::Name(t) => title = Some(t.clone()),
            Item::Sheet(x) => sheet = x.clone(),
            _ => {}
        }
    }
    let (w, h) = sheet_size(&sheet);
    // The parts, each made from the library.
    let mut parts: Vec<Placed> = Vec::new();
    let mut by_ref: HashMap<String, usize> = HashMap::new();
    for (index, l) in s.items.iter().enumerate() {
        let Item::Part(p) = &l.item else { continue };
        if by_ref.contains_key(&p.reference) {
            findings.push(err(l.line, format!("{} is a reference already", p.reference)));
            continue;
        }
        let symbol = match make(&p.kind, &p.args) {
            Ok(sym) => sym,
            Err(e) => {
                findings.push(err(l.line, format!("{}: {e}", p.reference)));
                continue;
            }
        };
        if let Some(c) = symbol.caution {
            findings.push(warn(l.line, format!("{}: {c}", p.reference)));
        }
        let mut numbers: Vec<&str> = symbol.pins.iter().map(|x| x.number.as_str()).collect();
        numbers.sort_unstable();
        if numbers.windows(2).any(|x| x[0] == x[1]) {
            findings.push(err(l.line, format!("{}: two pins have one number", p.reference)));
        }
        by_ref.insert(p.reference.clone(), parts.len());
        parts.push(Placed { reference: p.reference.clone(), line: l.line, index, symbol, value: p.value.clone(), footprint: p.footprint.clone(), at: p.at.unwrap_or([0.0, 0.0]), placed: p.at.is_none(), turn: p.turn, mirror: p.mirror });
    }
    // Those the file does not place, set out in rows from the top left.
    let (mut x, mut y) = (35.0, h - 45.0);
    for p in parts.iter_mut().filter(|p| p.placed) {
        p.at = [x, y];
        x += 55.0;
        if x > w - 35.0 {
            x = 35.0;
            y -= 55.0;
        }
    }
    // A pin named.
    let find = |r: &PinRef, line: usize, findings: &mut Vec<Finding>| -> Option<(usize, usize)> {
        let Some(&k) = by_ref.get(&r.part) else {
            findings.push(err(line, format!("{}: there is no part {}", r.text(), r.part)));
            return None;
        };
        match parts[k].symbol.pin(&r.pin) {
            Some(i) => Some((k, i)),
            None => {
                let sym = &parts[k].symbol;
                let names: Vec<String> = sym.pins.iter().map(|p| if p.name == p.number || p.name == "~" { p.number.clone() } else { format!("{} {}", p.number, p.name) }).collect();
                findings.push(err(line, format!("{}: {} has no pin {} — its pins: {}", r.text(), r.part, r.pin, names.join(", "))));
                None
            }
        }
    };
    let mut nets: Vec<NetOut> = Vec::new();
    let mut nc: Vec<(usize, usize, usize)> = Vec::new();
    let mut on: HashMap<(usize, usize), String> = HashMap::new();
    for (index, l) in s.items.iter().enumerate() {
        match &l.item {
            Item::Net(n) => {
                if nets.iter().any(|x| x.name == n.name) {
                    findings.push(err(l.line, format!("{} is a net already: one net, one statement", n.name)));
                    continue;
                }
                let mut ps = Vec::new();
                for r in &n.pins {
                    let Some(at) = find(r, l.line, &mut findings) else { continue };
                    if let Some(other) = on.get(&at) {
                        findings.push(err(l.line, format!("{} is on {other} already", r.text())));
                        continue;
                    }
                    if ps.contains(&at) {
                        continue;
                    }
                    on.insert(at, n.name.clone());
                    ps.push(at);
                }
                nets.push(NetOut { name: n.name.clone(), line: l.line, index, power: n.power, current: n.current, pins: ps });
            }
            Item::Nc(rs) => {
                for r in rs {
                    if let Some(at) = find(r, l.line, &mut findings) {
                        nc.push((at.0, at.1, l.line));
                    }
                }
            }
            _ => {}
        }
    }
    // Marked unconnected, and connected.
    for &(k, i, line) in &nc {
        if let Some(net) = on.get(&(k, i)) {
            findings.push(err(line, format!("{}.{} is marked unconnected and is on {net}", parts[k].reference, parts[k].symbol.pins[i].number)));
        }
    }
    // Each net's drivers and loads.
    for n in &nets {
        let kinds: Vec<(PinKind, String)> = n.pins.iter().map(|&(k, i)| (parts[k].symbol.pins[i].kind, format!("{}.{}", parts[k].reference, parts[k].symbol.pins[i].number))).collect();
        let outs: Vec<&String> = kinds.iter().filter(|k| matches!(k.0, PinKind::Output | PinKind::PowerOut)).map(|k| &k.1).collect();
        if outs.len() > 1 {
            findings.push(err(n.line, format!("{}: {} all drive it", n.name, outs.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" and "))));
        }
        for (k, who) in &kinds {
            if *k == PinKind::Nc {
                findings.push(err(n.line, format!("{}: {who} is not to be connected", n.name)));
            }
        }
        let supplied = n.power || kinds.iter().any(|k| k.0 == PinKind::PowerOut);
        let takes: Vec<&String> = kinds.iter().filter(|k| k.0 == PinKind::PowerIn).map(|k| &k.1).collect();
        if !takes.is_empty() && !supplied {
            findings.push(err(n.line, format!("{}: nothing supplies {} — say `net {} power …` if a cable brings it", n.name, takes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), n.name)));
        }
        let driven = n.power || kinds.iter().any(|k| k.0.drives());
        let inputs: Vec<&String> = kinds.iter().filter(|k| k.0 == PinKind::Input).map(|k| &k.1).collect();
        if !inputs.is_empty() && !driven {
            findings.push(warn(n.line, format!("{}: nothing drives {}", n.name, inputs.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))));
        }
        if n.pins.len() == 1 && !n.power {
            findings.push(warn(n.line, format!("{}: one pin only, {} — it joins nothing", n.name, kinds[0].1)));
        }
    }
    // Pins left unconnected, a part at a time.
    for (k, p) in parts.iter().enumerate() {
        let loose: Vec<&str> = p
            .symbol
            .pins
            .iter()
            .enumerate()
            .filter(|(i, pin)| pin.kind != PinKind::Nc && !on.contains_key(&(k, *i)) && !nc.iter().any(|x| x.0 == k && x.1 == *i))
            .map(|(_, pin)| pin.number.as_str())
            .collect();
        if !loose.is_empty() {
            findings.push(warn(p.line, format!("{}: pin{} {} {} connected to nothing — `nc {}.{}` if meant", p.reference, if loose.len() == 1 { "" } else { "s" }, loose.join(", "), if loose.len() == 1 { "is" } else { "are" }, p.reference, loose[0])));
        }
    }
    findings.sort_by_key(|f| (f.line, f.level == Level::Warning));
    Built { title, sheet, parts, nets, nc: nc.into_iter().map(|x| (x.0, x.1)).collect(), findings }
}

/// A diagnostic as the rules check gives it, for a reader of `Diag`s.
pub fn diag(f: &Finding) -> Diag {
    Diag::new(f.line, 1, f.message.clone())
}
