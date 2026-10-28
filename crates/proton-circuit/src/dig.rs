//! Digital's `.dig` files read into a netlist: a circuit's parts placed on
//! its grid, its wires, and the other `.dig` files beside it it uses as
//! subcircuits, each use inlined as its own copy.
//!
//! Built from how Digital *behaves* — its files and where its parts' pins
//! fall, found by placing them — never from its sources: Digital is
//! GPL-3.0, Proton MIT. A part this does not know is named, not guessed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use quick_xml::events::Event as XmlEvent;

use crate::parts::{Kind, Shift};
use crate::sim::{NetId, Netlist, PartId};
use crate::value::mask;

/// The grid: every pin is on a multiple of it.
const G: i32 = 20;

/// A splitter's bit groups: each one's lowest bit and width.
type Groups = Vec<(u8, u8)>;
/// A grid point.
type Point = (i32, i32);
/// Pin names and widths.
type Names = Vec<(String, u8)>;

/// An attribute's value as the file gives it.
#[derive(Debug, Clone, PartialEq)]
pub enum Attr {
    Str(String),
    Int(i64),
    Bool(bool),
    Rotation(u8),
    Test(String),
    Other,
}

#[derive(Debug, Clone)]
pub struct Element {
    pub name: String,
    pub attrs: HashMap<String, Attr>,
    pub x: i32,
    pub y: i32,
}

impl Element {
    pub fn int(&self, k: &str, default: i64) -> i64 {
        match self.attrs.get(k) {
            Some(Attr::Int(v)) => *v,
            Some(Attr::Str(s)) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    pub fn str(&self, k: &str) -> &str {
        match self.attrs.get(k) {
            Some(Attr::Str(s)) => s,
            _ => "",
        }
    }

    /// A splitter's groups, in and out, with Digital's defaults where the
    /// file leaves them out.
    pub fn splitting(&self) -> (Groups, Groups) {
        let or = |k: &str, d: &'static str| -> Groups {
            match self.attrs.get(k) {
                Some(Attr::Str(s)) => groups(s),
                _ => groups(d),
            }
        };
        (or("Input Splitting", "4,4"), or("Output Splitting", "8"))
    }

    pub fn flag(&self, k: &str) -> bool {
        matches!(self.attrs.get(k), Some(Attr::Bool(true)))
    }

    fn bits(&self) -> u8 {
        self.int("Bits", 1).clamp(1, 64) as u8
    }

    fn rotation(&self) -> u8 {
        match self.attrs.get("rotation") {
            Some(Attr::Rotation(r)) => *r % 4,
            _ => 0,
        }
    }

    pub fn label(&self) -> &str {
        self.str("Label")
    }
}

/// One `.dig` file: its parts, its wires, and its own attributes.
#[derive(Debug, Clone, Default)]
pub struct Circuit {
    pub elements: Vec<Element>,
    pub wires: Vec<((i32, i32), (i32, i32))>,
    /// A subcircuit's width in grid steps, when it says.
    pub width: Option<i32>,
}

/// A `.dig` file's XML read.
pub fn parse(xml: &str) -> Result<Circuit, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut c = Circuit::default();
    // Where we are: the tags open, and the text collected.
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut el: Option<Element> = None;
    let mut key: Option<String> = None;
    let mut wire: (Option<Point>, Option<Point>) = (None, None);
    let mut circuit_key: Option<String> = None;
    let num = |e: &quick_xml::events::BytesStart, k: &[u8]| -> i32 {
        e.attributes().flatten().find(|a| a.key.as_ref() == k).and_then(|a| String::from_utf8_lossy(&a.value).parse().ok()).unwrap_or(0)
    };
    loop {
        let ev = reader.read_event().map_err(|e| format!("at {}: {e}", reader.buffer_position()))?;
        match ev {
            XmlEvent::Start(ref e) | XmlEvent::Empty(ref e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let empty = matches!(ev, XmlEvent::Empty(_));
                match name.as_str() {
                    "visualElement" => el = Some(Element { name: String::new(), attrs: HashMap::new(), x: 0, y: 0 }),
                    "pos" => {
                        if let Some(el) = &mut el {
                            el.x = num(e, b"x");
                            el.y = num(e, b"y");
                        }
                    }
                    "p1" => wire.0 = Some((num(e, b"x"), num(e, b"y"))),
                    "p2" => wire.1 = Some((num(e, b"x"), num(e, b"y"))),
                    "rotation" => {
                        if let (Some(el), Some(k)) = (&mut el, &key) {
                            el.attrs.insert(k.clone(), Attr::Rotation(num(e, b"rotation") as u8));
                        }
                    }
                    _ => {}
                }
                text.clear();
                if !empty {
                    path.push(name);
                }
            }
            XmlEvent::Text(t) => text.push_str(&t.decode().map_err(|e| e.to_string())?),
            XmlEvent::GeneralRef(r) => {
                let s = if r.is_char_ref() {
                    r.resolve_char_ref().ok().flatten().map(String::from).unwrap_or_default()
                } else {
                    match r.decode().map_err(|e| e.to_string())?.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    }
                    .to_string()
                };
                text.push_str(&s);
            }
            XmlEvent::CData(t) => text.push_str(&String::from_utf8_lossy(&t)),
            XmlEvent::End(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                path.pop();
                let parent = path.last().map(String::as_str).unwrap_or("");
                let in_el = path.iter().any(|p| p == "visualElement");
                match name.as_str() {
                    "elementName" if in_el => {
                        if let Some(el) = &mut el {
                            el.name = text.trim().to_string();
                        }
                    }
                    "string" if parent == "entry" && in_el => match &key {
                        None => key = Some(text.clone()),
                        Some(k) => {
                            if let Some(el) = &mut el {
                                el.attrs.insert(k.clone(), Attr::Str(text.clone()));
                            }
                        }
                    },
                    "string" if parent == "entry" && path.len() == 3 => {
                        if circuit_key.is_none() {
                            circuit_key = Some(text.clone());
                        }
                    }
                    "int" | "long" if parent == "entry" => {
                        if in_el {
                            if let (Some(el), Some(k)) = (&mut el, &key) {
                                el.attrs.insert(k.clone(), Attr::Int(text.trim().parse().unwrap_or(0)));
                            }
                        } else if circuit_key.as_deref() == Some("Width") {
                            c.width = text.trim().parse().ok();
                        }
                    }
                    "boolean" if parent == "entry" && in_el => {
                        if let (Some(el), Some(k)) = (&mut el, &key) {
                            el.attrs.insert(k.clone(), Attr::Bool(text.trim() == "true"));
                        }
                    }
                    "dataString" => {
                        if let (Some(el), Some(k)) = (&mut el, &key) {
                            el.attrs.insert(k.clone(), Attr::Test(text.clone()));
                        }
                    }
                    "entry" => {
                        if in_el {
                            if let (Some(el), Some(k)) = (&mut el, &key) {
                                el.attrs.entry(k.clone()).or_insert(Attr::Other);
                            }
                            key = None;
                        } else {
                            circuit_key = None;
                        }
                    }
                    "visualElement" => {
                        if let Some(el) = el.take() {
                            c.elements.push(el);
                        }
                    }
                    "wire" => {
                        if let (Some(a), Some(b)) = wire {
                            c.wires.push((a, b));
                        }
                        wire = (None, None);
                    }
                    _ => {}
                }
                text.clear();
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    Ok(c)
}

/// A pin of a placed part: where it is, which way it faces, how wide.
#[derive(Debug, Clone, Copy)]
struct Pin {
    at: (i32, i32),
    input: bool,
    bits: Option<u8>,
}

/// A splitter's groups: `16`, `4,4,8`, `1*4`, `0-3,4-15`, as (first bit, width).
pub fn groups(spec: &str) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut next = 0u32;
    for tok in spec.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if let Some((a, b)) = tok.split_once('-') {
            let (a, b): (u32, u32) = (a.trim().parse().unwrap_or(0), b.trim().parse().unwrap_or(0));
            let (lo, hi) = (a.min(b), a.max(b));
            out.push((lo as u8, (hi - lo + 1) as u8));
            next = hi + 1;
        } else if let Some((w, n)) = tok.split_once('*') {
            let (w, n): (u32, u32) = (w.trim().parse().unwrap_or(1), n.trim().parse().unwrap_or(1));
            for _ in 0..n {
                out.push((next as u8, w as u8));
                next += w;
            }
        } else {
            let w: u32 = tok.parse().unwrap_or(1);
            out.push((next as u8, w as u8));
            next += w;
        }
    }
    out
}

/// The files a circuit uses, read once each.
pub struct Library {
    dir: PathBuf,
    circuits: HashMap<String, Circuit>,
}

impl Library {
    pub fn new(dir: &Path) -> Library {
        Library { dir: dir.to_path_buf(), circuits: HashMap::new() }
    }

    pub fn get(&mut self, name: &str) -> Result<&Circuit, String> {
        if !self.circuits.contains_key(name) {
            let path = self.dir.join(name);
            let xml = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let c = parse(&xml).map_err(|e| format!("{name}: {e}"))?;
            self.circuits.insert(name.to_string(), c);
        }
        Ok(&self.circuits[name])
    }
}

/// A subcircuit's pins: its inputs down the left, its outputs down the
/// right, each in the order the file lists them (Digital's own order,
/// which its "order inputs" dialog rewrites).
fn sub_pins(c: &Circuit) -> (Names, Names, i32) {
    let ins: Vec<&Element> = c.elements.iter().filter(|e| e.name == "In" || e.name == "Clock").collect();
    let outs: Vec<&Element> = c.elements.iter().filter(|e| e.name == "Out").collect();
    let w = c.width.unwrap_or(3) * G;
    (ins.iter().map(|e| (e.label().to_string(), e.bits())).collect(), outs.iter().map(|e| (e.label().to_string(), e.bits())).collect(), w)
}

/// A part's pins where they fall, turned and mirrored as it is.
fn place(el: &Element, lib: &mut Library) -> Result<Vec<Pin>, String> {
    let bits = el.bits();
    let inp = |x: i32, y: i32, b: Option<u8>| Pin { at: (x, y), input: true, bits: b };
    let out = |x: i32, y: i32, b: Option<u8>| Pin { at: (x, y), input: false, bits: b };
    let wide = el.flag("wideShape");
    let gate_w = 3 * G;
    let mut p: Vec<Pin> = match el.name.as_str() {
        "In" | "Clock" | "Const" | "Ground" | "VDD" => vec![out(0, 0, Some(bits))],
        "Out" | "LED" | "Probe" => vec![inp(0, 0, Some(bits))],
        "Tunnel" => vec![Pin { at: (0, 0), input: true, bits: None }],
        "Not" => vec![inp(0, 0, Some(bits)), out(if wide { 3 * G } else { 2 * G }, 0, Some(bits))],
        "And" | "Or" | "NAnd" | "NOr" | "XOr" | "XNOr" => {
            let n = el.int("Inputs", 2).max(1) as i32;
            let mut v = Vec::new();
            let slots = if n % 2 == 0 { n + 1 } else { n };
            for i in 0..slots {
                if n % 2 == 0 && i == n / 2 {
                    continue;
                }
                v.push(inp(0, i * G, Some(bits)));
            }
            let w = if wide { gate_w + G } else { gate_w };
            // An inverted output's bubble pushes the pin a step further out.
            let bubble = if matches!(el.name.as_str(), "NAnd" | "NOr" | "XNOr") { G } else { 0 };
            v.push(out(w + bubble, (slots - 1) / 2 * G, Some(bits)));
            v
        }
        "Multiplexer" => {
            let s = el.int("Selector Bits", 1).clamp(1, 8) as i32;
            let n = 1 << s;
            let mut v = vec![];
            let flip = el.flag("flipSelPos");
            let (h, sel_y) = if n == 2 { (2 * G, if flip { 0 } else { 2 * G }) } else { (n * G, if flip { -G } else { n * G }) };
            v.push(inp(G, sel_y, Some(s as u8)));
            for i in 0..n {
                let y = if n == 2 { i * 2 * G } else { i * G };
                v.push(inp(0, y, Some(bits)));
            }
            v.push(out(2 * G, h / 2, Some(bits)));
            v
        }
        "Demultiplexer" => {
            let s = el.int("Selector Bits", 1).clamp(1, 8) as i32;
            let n = 1 << s;
            let flip = el.flag("flipSelPos");
            let (h, sel_y) = if n == 2 { (2 * G, if flip { 0 } else { 2 * G }) } else { (n * G, if flip { -G } else { n * G }) };
            let mut v = vec![inp(G, sel_y, Some(s as u8)), inp(0, h / 2, Some(bits))];
            for i in 0..n {
                v.push(out(2 * G, if n == 2 { i * 2 * G } else { i * G }, Some(bits)));
            }
            v
        }
        "Decoder" => {
            let s = el.int("Selector Bits", 1).clamp(1, 8) as i32;
            let n = 1 << s;
            let flip = el.flag("flipSelPos");
            let mut v = vec![inp(G, if flip { 0 } else { n * G }, Some(s as u8))];
            for i in 0..n {
                v.push(out(2 * G, if n == 2 { i * 2 * G } else { i * G }, Some(1)));
            }
            v
        }
        "PriorityEncoder" => {
            let s = el.int("Selector Bits", 1).clamp(1, 8) as i32;
            let mut v: Vec<Pin> = (0..(1 << s)).map(|i| inp(0, i * G, Some(1))).collect();
            v.push(out(4 * G, 0, Some(s as u8)));
            v.push(out(4 * G, G, Some(1)));
            v
        }
        "Add" | "Sub" => vec![inp(0, 0, Some(bits)), inp(0, G, Some(bits)), inp(0, 2 * G, Some(1)), out(3 * G, 0, Some(bits)), out(3 * G, G, Some(1))],
        "Mul" => vec![inp(0, 0, Some(bits)), inp(0, 2 * G, Some(bits)), out(3 * G, G, Some((bits * 2).min(64)))],
        "Div" => vec![inp(0, 0, Some(bits)), inp(0, 2 * G, Some(bits)), out(3 * G, 0, Some(bits)), out(3 * G, 2 * G, Some(bits))],
        "Comparator" => vec![inp(0, 0, Some(bits)), inp(0, G, Some(bits)), out(3 * G, 0, Some(1)), out(3 * G, G, Some(1)), out(3 * G, 2 * G, Some(1))],
        "Neg" => vec![inp(0, 0, Some(bits)), out(3 * G, 0, Some(bits))],
        "BarrelShifter" => vec![inp(0, 0, Some(bits)), inp(0, 2 * G, None), out(3 * G, G, Some(bits))],
        "Register" => vec![inp(0, 0, Some(bits)), inp(0, G, Some(1)), inp(0, 2 * G, Some(1)), out(3 * G, G, Some(bits))],
        "D_FF" => vec![inp(0, 0, Some(bits)), inp(0, G, Some(1)), out(3 * G, 0, Some(bits)), out(3 * G, 2 * G, Some(bits))],
        "JK_FF" | "RS_FF" => vec![inp(0, 0, Some(1)), inp(0, G, Some(1)), inp(0, 2 * G, Some(1)), out(3 * G, 0, Some(1)), out(3 * G, 2 * G, Some(1))],
        "T_FF" => vec![inp(0, 0, Some(1)), inp(0, G, Some(1)), out(3 * G, 0, Some(1)), out(3 * G, 2 * G, Some(1))],
        "RAMDualAccess" => {
            let a = el.int("AddrBits", 8).clamp(1, 24) as u8;
            vec![
                inp(0, 0, Some(1)),
                inp(0, G, Some(1)),
                inp(0, 2 * G, Some(1)),
                inp(0, 3 * G, Some(a)),
                inp(0, 4 * G, Some(bits)),
                inp(0, 5 * G, Some(a)),
                out(3 * G, 0, Some(bits)),
                out(3 * G, G, Some(bits)),
            ]
        }
        "Splitter" => {
            let spread = el.int("splitterSpreading", 1).max(1) as i32 * G;
            let (ins, outs) = el.splitting();
            let mut v: Vec<Pin> = ins.iter().enumerate().map(|(i, (_, w))| inp(0, i as i32 * spread, Some(*w))).collect();
            v.extend(outs.iter().enumerate().map(|(i, (_, w))| out(G, i as i32 * spread, Some(*w))));
            v
        }
        "Text" | "Testcase" | "Rectangle" => Vec::new(),
        name if name.ends_with(".dig") => {
            let (ins, outs, w) = sub_pins(lib.get(name)?);
            // With one output the box is drawn like a gate: an even count of
            // inputs leaves the middle slot empty and the output sits level
            // with it.
            let n = ins.len() as i32;
            let one = outs.len() == 1;
            let gap = one && n % 2 == 0;
            let mut v: Vec<Pin> = ins.iter().enumerate().map(|(i, (_, b))| {
                let i = i as i32;
                inp(0, if gap && i >= n / 2 { i + 1 } else { i } * G, Some(*b))
            }).collect();
            if one {
                let slots = if gap { n + 1 } else { n.max(1) };
                v.push(out(w, (slots - 1) / 2 * G, Some(outs[0].1)));
            } else {
                v.extend(outs.iter().enumerate().map(|(i, (_, b))| out(w, i as i32 * G, Some(*b))));
            }
            v
        }
        other => return Err(format!("the part {other} is not one Proton knows yet")),
    };
    // Mirrored, then turned a quarter at a time.
    let mirror = el.flag("mirror");
    for pin in &mut p {
        let (mut x, mut y) = pin.at;
        if mirror {
            y = -y;
        }
        for _ in 0..el.rotation() {
            (x, y) = (y, -x);
        }
        pin.at = (el.x + x, el.y + y);
    }
    Ok(p)
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[a] = b;
    }
}

/// A circuit's connectivity, worked out once from its drawing: each part's
/// pins, and which of them share a net.
struct Topology {
    pins: Vec<Vec<Pin>>,
    /// For each part, for each pin, its net's number in this circuit.
    class: Vec<Vec<usize>>,
    classes: usize,
    /// Each net's width, where some pin says.
    widths: Vec<Option<u8>>,
}

fn topology(c: &Circuit, lib: &mut Library) -> Result<Topology, String> {
    let mut pins = Vec::new();
    for el in &c.elements {
        pins.push(place(el, lib)?);
    }
    // Union-find over wires first, then pins.
    let nw = c.wires.len();
    let mut ids: Vec<(usize, usize)> = Vec::new(); // (part, pin) per pin node
    for (e, ps) in pins.iter().enumerate() {
        for i in 0..ps.len() {
            ids.push((e, i));
        }
    }
    let mut parent: Vec<usize> = (0..nw + ids.len()).collect();
    // Points where things meet.
    let mut at: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (w, (a, b)) in c.wires.iter().enumerate() {
        at.entry(*a).or_default().push(w);
        at.entry(*b).or_default().push(w);
    }
    for (k, (e, i)) in ids.iter().enumerate() {
        at.entry(pins[*e][*i].at).or_default().push(nw + k);
    }
    for nodes in at.values() {
        for w in nodes.windows(2) {
            union(&mut parent, w[0], w[1]);
        }
    }
    // Things join only where ends meet. Digital's editor splits a wire
    // wherever another one joins it, so a wire end, or a pin, lying along
    // a wire is one passing by, not a junction.
    // Tunnels of one name are one net.
    let mut tunnels: HashMap<&str, usize> = HashMap::new();
    for (k, (e, _)) in ids.iter().enumerate() {
        let el = &c.elements[*e];
        if el.name == "Tunnel" {
            let name = el.str("NetName");
            match tunnels.get(name) {
                Some(&first) => union(&mut parent, first, nw + k),
                None => {
                    tunnels.insert(name, nw + k);
                }
            }
        }
    }
    let mut number: HashMap<usize, usize> = HashMap::new();
    let mut class = Vec::new();
    let mut widths: Vec<Option<u8>> = Vec::new();
    let mut k = 0;
    for ps in &pins {
        let mut row = Vec::new();
        for pin in ps {
            let root = find(&mut parent, nw + k);
            let n = number.len();
            let id = *number.entry(root).or_insert(n);
            if id == widths.len() {
                widths.push(None);
            }
            if let Some(b) = pin.bits {
                widths[id] = Some(widths[id].map_or(b, |w: u8| w.max(b)));
            }
            row.push(id);
            k += 1;
        }
        class.push(row);
    }
    Ok(Topology { pins, class, classes: number.len(), widths })
}

/// The pins of a file's parts that touch nothing — no wire, no other
/// pin, no tunnel. In a finished drawing almost none do, so these are
/// where a part's pins are not where this reader puts them.
pub fn loose_pins(dir: &Path, file: &str) -> Result<Vec<String>, String> {
    let mut lib = Library::new(dir);
    let c = lib.get(file)?.clone();
    let t = topology(&c, &mut lib)?;
    let mut members = vec![0usize; t.classes];
    for row in &t.class {
        for k in row {
            members[*k] += 1;
        }
    }
    // A class with a wire in it has more than its pins: count wires too.
    let mut wired = vec![false; t.classes];
    for (e, pins) in t.pins.iter().enumerate() {
        for (i, pin) in pins.iter().enumerate() {
            if c.wires.iter().any(|(a, b)| pin.at == *a || pin.at == *b) {
                wired[t.class[e][i]] = true;
            }
        }
    }
    let mut out = Vec::new();
    for (e, el) in c.elements.iter().enumerate() {
        for (i, pin) in t.pins[e].iter().enumerate() {
            let k = t.class[e][i];
            if members[k] == 1 && !wired[k] {
                let attrs: Vec<String> = el
                    .attrs
                    .iter()
                    .filter(|(k, _)| !matches!(k.as_str(), "Label" | "Description" | "intFormat" | "Testdata" | "Color"))
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .collect();
                out.push(format!(
                    "{} {:?} pin {i} ({}) at {:?}, part at ({}, {}) [{}]",
                    el.name,
                    el.label(),
                    if pin.input { "in" } else { "out" },
                    pin.at,
                    el.x,
                    el.y,
                    attrs.join(", ")
                ));
            }
        }
    }
    Ok(out)
}

/// A circuit built: its netlist, and the names its tests use.
#[derive(Debug, Default)]
pub struct Built {
    pub netlist: Netlist,
    /// The top circuit's inputs and clocks by label.
    pub inputs: HashMap<String, (PartId, u8)>,
    /// Its outputs by label, as the parts that show them.
    pub outputs: HashMap<String, PartId>,
    /// Registers marked as probes, at any depth, by label.
    pub registers: HashMap<String, PartId>,
    /// The program memory: the RAM marked so.
    pub program: Option<PartId>,
    /// The test cases in the top circuit: label and text.
    pub tests: Vec<(String, String)>,
}

/// The circuit in `file`, in `dir`, built with every subcircuit inlined.
pub fn build(dir: &Path, file: &str) -> Result<Built, String> {
    let mut lib = Library::new(dir);
    let mut topo_cache: HashMap<String, std::rc::Rc<Topology>> = HashMap::new();
    let mut built = Built::default();
    let top = lib.get(file)?.clone();
    for el in &top.elements {
        if el.name == "Testcase" {
            if let Some(Attr::Test(t)) = el.attrs.get("Testdata") {
                built.tests.push((el.label().to_string(), t.clone()));
            }
        }
    }
    instance(&mut lib, &mut topo_cache, file, &top, &mut built, "", &HashMap::new(), 0)?;
    Ok(built)
}

#[allow(clippy::too_many_arguments)]
fn instance(
    lib: &mut Library,
    cache: &mut HashMap<String, std::rc::Rc<Topology>>,
    name: &str,
    c: &Circuit,
    built: &mut Built,
    prefix: &str,
    bound: &HashMap<String, NetId>,
    depth: u32,
) -> Result<(), String> {
    if depth > 32 {
        return Err(format!("{name} uses itself, through its subcircuits"));
    }
    let topo = match cache.get(name) {
        Some(t) => t.clone(),
        None => {
            let t = std::rc::Rc::new(topology(c, lib)?);
            cache.insert(name.to_string(), t.clone());
            t
        }
    };
    let top = depth == 0;
    // Each net of this circuit: the parent's, where an In or Out of this
    // one is bound to it; a new one otherwise.
    let mut nets: Vec<Option<NetId>> = vec![None; topo.classes];
    for (e, el) in c.elements.iter().enumerate() {
        if !top && matches!(el.name.as_str(), "In" | "Clock" | "Out") {
            if let Some(&parent) = bound.get(el.label()) {
                let k = topo.class[e][0];
                match nets[k] {
                    None => nets[k] = Some(parent),
                    Some(have) if have != parent => {
                        // An input wired straight to an output: copied across.
                        built.netlist.part(Kind::Buffer, &format!("{prefix}{}", el.label()), &[have], &[parent]);
                    }
                    _ => {}
                }
            }
        }
    }
    // A net is named for a tunnel or pin on it, where it has one.
    let mut called: Vec<Option<&str>> = vec![None; topo.classes];
    for (e, el) in c.elements.iter().enumerate() {
        let label = if el.name == "Tunnel" { el.str("NetName") } else if matches!(el.name.as_str(), "In" | "Clock" | "Out") { el.label() } else { "" };
        if !label.is_empty() {
            called[topo.class[e][0]].get_or_insert(label);
        }
    }
    let mut net = |k: usize, nl: &mut Netlist| -> NetId {
        if let Some(n) = nets[k] {
            return n;
        }
        let name = match called[k] {
            Some(l) => format!("{prefix}{l}"),
            None => format!("{prefix}n{k}"),
        };
        let n = nl.net(topo.widths[k].unwrap_or(1), &name);
        nets[k] = Some(n);
        n
    };
    for (e, el) in c.elements.iter().enumerate() {
        let cls = &topo.class[e];
        let pins = &topo.pins[e];
        let label = el.label();
        let pname = if label.is_empty() { format!("{prefix}{}#{e}", el.name) } else { format!("{prefix}{label}") };
        let ins: Vec<NetId> = pins.iter().zip(cls).filter(|(p, _)| p.input).map(|(_, k)| net(*k, &mut built.netlist)).collect();
        let outs: Vec<NetId> = pins.iter().zip(cls).filter(|(p, _)| !p.input).map(|(_, k)| net(*k, &mut built.netlist)).collect();
        let nl = &mut built.netlist;
        let bits = el.bits();
        match el.name.as_str() {
            "In" | "Clock" => {
                if top {
                    let kind = if el.name == "Clock" { Kind::Clock } else { Kind::Input };
                    let id = nl.part(kind, &pname, &[], &outs);
                    built.inputs.insert(label.to_string(), (id, bits));
                } else if !bound.contains_key(label) {
                    // An input the parent leaves open: it floats.
                }
            }
            "Out" | "LED" | "Probe" => {
                let id = nl.part(Kind::Output, &pname, &ins, &[]);
                if top && !label.is_empty() {
                    built.outputs.insert(label.to_string(), id);
                }
            }
            "Const" => {
                nl.part(Kind::Const(el.int("Value", 1) as u64 & mask(bits)), &pname, &[], &outs);
            }
            "Ground" => {
                nl.part(Kind::Const(0), &pname, &[], &outs);
            }
            "VDD" => {
                nl.part(Kind::Const(u64::MAX), &pname, &[], &outs);
            }
            "Tunnel" | "Text" | "Testcase" | "Rectangle" => {}
            "Not" => {
                nl.part(Kind::Not, &pname, &ins, &outs);
            }
            "And" | "Or" | "NAnd" | "NOr" | "XOr" | "XNOr" => {
                let kind = match el.name.as_str() {
                    "And" => Kind::And,
                    "Or" => Kind::Or,
                    "NAnd" => Kind::Nand,
                    "NOr" => Kind::Nor,
                    "XOr" => Kind::Xor,
                    _ => Kind::Xnor,
                };
                nl.part(kind, &pname, &ins, &outs);
            }
            "Multiplexer" => {
                nl.part(Kind::Mux, &pname, &ins, &outs);
            }
            "Demultiplexer" => {
                nl.part(Kind::Demux, &pname, &ins, &outs);
            }
            "Decoder" => {
                nl.part(Kind::Decoder, &pname, &ins, &outs);
            }
            "PriorityEncoder" => {
                nl.part(Kind::PriorityEncoder, &pname, &ins, &outs);
            }
            "Add" => {
                nl.part(Kind::Add, &pname, &ins, &outs);
            }
            "Sub" => {
                nl.part(Kind::Sub, &pname, &ins, &outs);
            }
            "Mul" => {
                nl.part(Kind::Mul { signed: el.flag("signed") }, &pname, &ins, &outs);
            }
            "Div" => {
                nl.part(Kind::Div { signed: el.flag("signed") }, &pname, &ins, &outs);
            }
            "Comparator" => {
                nl.part(Kind::Comparator { signed: el.flag("Signed") }, &pname, &ins, &outs);
            }
            "Neg" => {
                nl.part(Kind::Neg, &pname, &ins, &outs);
            }
            "BarrelShifter" => {
                let mode = match el.str("barrelShifterMode") {
                    "arithmetic" => Shift::Arithmetic,
                    "rotate" => Shift::Rotate,
                    _ => Shift::Logical,
                };
                let left = el.str("direction") != "right";
                nl.part(Kind::Shifter { left, mode, signed_shift: el.flag("barrelSigned") }, &pname, &ins, &outs);
            }
            "Register" => {
                let id = nl.part(Kind::Register, &pname, &ins, &outs);
                if !label.is_empty() && (el.flag("valueIsProbe") || el.flag("isProgramCounter")) {
                    built.registers.insert(label.to_string(), id);
                }
            }
            "D_FF" => {
                nl.part(Kind::DFlipFlop, &pname, &ins, &outs);
            }
            "T_FF" => {
                nl.part(Kind::TFlipFlop, &pname, &ins, &outs);
            }
            "JK_FF" => {
                nl.part(Kind::JKFlipFlop, &pname, &ins, &outs);
            }
            "RS_FF" => {
                nl.part(Kind::RSFlipFlop, &pname, &ins, &outs);
            }
            "RAMDualAccess" => {
                let id = nl.part(Kind::RamDual { addr_bits: el.int("AddrBits", 8).clamp(1, 24) as u8 }, &pname, &ins, &outs);
                if el.flag("isProgramMemory") {
                    built.program = Some(id);
                }
            }
            "Splitter" => {
                let (gi, go) = el.splitting();
                nl.part(Kind::Splitter { ins: gi, outs: go }, &pname, &ins, &outs);
            }
            sub if sub.ends_with(".dig") => {
                let sc = lib.get(sub)?.clone();
                let (sins, souts, _) = sub_pins(&sc);
                let mut b: HashMap<String, NetId> = HashMap::new();
                for ((l, _), n) in sins.iter().zip(&ins) {
                    b.insert(l.clone(), *n);
                }
                for ((l, _), n) in souts.iter().zip(&outs) {
                    b.insert(l.clone(), *n);
                }
                let p = format!("{pname}/");
                instance(lib, cache, sub, &sc, built, &p, &b, depth + 1)?;
            }
            other => return Err(format!("the part {other} is not one Proton knows yet")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_read_widths_counts_and_ranges() {
        assert_eq!(groups("16"), vec![(0, 16)]);
        assert_eq!(groups("1*4"), vec![(0, 1), (1, 1), (2, 1), (3, 1)]);
        assert_eq!(groups("0-3,4-15"), vec![(0, 4), (4, 12)]);
        assert_eq!(groups("11-11,10-8,6-4"), vec![(11, 1), (8, 3), (4, 3)]);
        assert_eq!(groups("1,1-3"), vec![(0, 1), (1, 3)]);
    }

    #[test]
    fn a_little_dig_file_is_read_and_runs() {
        // An AND of two inputs, wired as Digital draws it: inputs at the
        // gate's left, its output 60 across.
        let xml = r#"<?xml version="1.0" encoding="utf-8"?><circuit><version>2</version><attributes/><visualElements>
          <visualElement><elementName>And</elementName><elementAttributes/><pos x="200" y="100"/></visualElement>
          <visualElement><elementName>In</elementName><elementAttributes><entry><string>Label</string><string>A</string></entry></elementAttributes><pos x="100" y="100"/></visualElement>
          <visualElement><elementName>In</elementName><elementAttributes><entry><string>Label</string><string>B</string></entry></elementAttributes><pos x="100" y="140"/></visualElement>
          <visualElement><elementName>Out</elementName><elementAttributes><entry><string>Label</string><string>Y</string></entry></elementAttributes><pos x="320" y="120"/></visualElement>
          <visualElement><elementName>Testcase</elementName><elementAttributes><entry><string>Testdata</string><testData><dataString>A B Y
0 0 0
1 1 1
</dataString></testData></entry></elementAttributes><pos x="0" y="0"/></visualElement>
          </visualElements><wires>
          <wire><p1 x="100" y="100"/><p2 x="200" y="100"/></wire>
          <wire><p1 x="100" y="140"/><p2 x="200" y="140"/></wire>
          <wire><p1 x="260" y="120"/><p2 x="320" y="120"/></wire>
          </wires></circuit>"#;
        let dir = std::env::temp_dir().join(format!("proton-circuit-dig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("and.dig"), xml).unwrap();
        let b = build(&dir, "and.dig").unwrap();
        assert_eq!(b.tests.len(), 1);
        assert!(b.tests[0].1.starts_with("A B Y"));
        let (a, _) = b.inputs["A"];
        let (bb, _) = b.inputs["B"];
        let y = b.outputs["Y"];
        let mut sim = crate::Sim::new(b.netlist, 1).unwrap();
        sim.set(a, 1).unwrap();
        assert_eq!(sim.shown(y).get(), 0);
        sim.set(bb, 1).unwrap();
        assert_eq!(sim.shown(y).get(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn el(name: &str, attrs: &[(&str, &str)], x: i32, y: i32) -> String {
        let a: String = attrs
            .iter()
            .map(|(k, v)| match *k {
                "Bits" | "Inputs" | "AddrBits" => format!("<entry><string>{k}</string><int>{v}</int></entry>"),
                "isProgramMemory" | "wideShape" => format!("<entry><string>{k}</string><boolean>{v}</boolean></entry>"),
                _ => format!("<entry><string>{k}</string><string>{v}</string></entry>"),
            })
            .collect();
        format!("<visualElement><elementName>{name}</elementName><elementAttributes>{a}</elementAttributes><pos x=\"{x}\" y=\"{y}\"/></visualElement>")
    }

    fn wire(a: (i32, i32), b: (i32, i32)) -> String {
        format!("<wire><p1 x=\"{}\" y=\"{}\"/><p2 x=\"{}\" y=\"{}\"/></wire>", a.0, a.1, b.0, b.1)
    }

    fn file(attrs: &str, els: &[String], wires: &[String]) -> String {
        format!("<?xml version=\"1.0\"?><circuit><version>2</version><attributes>{attrs}</attributes><visualElements>{}</visualElements><wires>{}</wires></circuit>", els.concat(), wires.concat())
    }

    fn folder(files: &[(&str, String)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-circuit-dig-{}-{}", std::process::id(), files[0].0));
        std::fs::create_dir_all(&dir).unwrap();
        for (n, x) in files {
            std::fs::write(dir.join(n), x).unwrap();
        }
        dir
    }

    #[test]
    fn pins_where_digital_puts_them_and_joins_only_at_ends() {
        // pass.dig: B, listed first though drawn lower, is wired to Y; A
        // is left open. One output, two inputs: drawn like a gate, so its
        // pins are at 0 and 40 and Y at (60, 20).
        let sub = file(
            "",
            &[el("In", &[("Label", "B")], 0, 100), el("Out", &[("Label", "Y")], 100, 100), el("In", &[("Label", "A")], 0, 0)],
            &[wire((0, 100), (100, 100))],
        );
        // top.dig: P and Q into pass.dig, P on its first pin; a NAnd, whose
        // bubble puts its output at 80; a wire drawn across F's pin.
        let top = file(
            "",
            &[
                el("In", &[("Label", "P")], 100, 0),
                el("In", &[("Label", "Q")], 100, 40),
                el("pass.dig", &[], 200, 0),
                el("Out", &[("Label", "Y")], 300, 20),
                el("NAnd", &[], 200, 200),
                el("Out", &[("Label", "N")], 340, 220),
                el("Out", &[("Label", "F")], 200, 400),
            ],
            &[
                wire((100, 0), (200, 0)),
                wire((100, 40), (200, 40)),
                wire((260, 20), (300, 20)),
                wire((100, 0), (100, 200)),
                wire((100, 200), (200, 200)),
                wire((100, 40), (100, 240)),
                wire((100, 240), (200, 240)),
                wire((280, 220), (340, 220)),
                wire((100, 400), (300, 400)),
            ],
        );
        let dir = folder(&[("top.dig", top), ("pass.dig", sub)]);
        assert!(loose_pins(&dir, "top.dig").unwrap().iter().all(|l| l.contains("F") || l.contains("pass.dig")));
        let b = build(&dir, "top.dig").unwrap();
        let mut sim = crate::Sim::new(b.netlist, 1).unwrap();
        sim.set(b.inputs["P"].0, 1).unwrap();
        assert_eq!(sim.shown(b.outputs["Y"]).get(), 1, "Y follows the first pin, B");
        assert_eq!(sim.shown(b.outputs["N"]).get(), 1);
        sim.set(b.inputs["Q"].0, 1).unwrap();
        assert_eq!(sim.shown(b.outputs["N"]).get(), 0);
        assert!(sim.shown(b.outputs["F"]).is_z(), "a wire across a pin does not join it");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_splitter_left_unset_has_digitals_defaults() {
        let e = Element { name: "Splitter".into(), attrs: HashMap::new(), x: 0, y: 0 };
        assert_eq!(e.splitting(), (vec![(0, 4), (4, 4)], vec![(0, 8)]));
    }

    #[test]
    fn programs_load_first_and_inputs_marked_x_are_tried_both_ways() {
        // A RAM marked as the program memory, read on its second port.
        let ram = file(
            "",
            &[
                el("RAMDualAccess", &[("AddrBits", "4"), ("Bits", "8"), ("isProgramMemory", "true")], 200, 0),
                el("In", &[("Label", "A"), ("Bits", "4")], 100, 100),
                el("Out", &[("Label", "D"), ("Bits", "8")], 300, 20),
                el("In", &[("Label", "S")], 100, 300),
                el("Out", &[("Label", "T")], 200, 300),
            ],
            &[wire((100, 100), (200, 100)), wire((260, 20), (300, 20)), wire((100, 300), (200, 300))],
        );
        let dir = folder(&[("ram.dig", ram)]);
        let b = build(&dir, "ram.dig").unwrap();
        let ok = crate::testlang::run(&b, "", "A D\n0 7\n1 9\n\nprogram(7, 9)\n");
        assert!(ok.passed(), "{:?} {:?}", ok.failures, ok.error);
        let x = crate::testlang::run(&b, "", "S T\n1 1\nx 0\n");
        assert_eq!(x.rows, 2);
        assert_eq!(x.failures.len(), 1, "S as 0 passes, S as 1 does not: {:?}", x.failures);
        let _ = std::fs::remove_dir_all(dir);
    }
}
