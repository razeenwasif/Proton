//! The `Name.circuit/` folder: a project of circuits in text that diffs.
//!
//! ```text
//! Name.circuit/
//!   project.json                  which circuit is the top
//!   circuits/<name>.circ          a circuit: a line per part, a line per wire
//!   tests/<circuit>.<label>.tests a test table for a circuit, in Digital's language
//!   data/<name>.hex               a memory's contents (see [`crate::hex`]), named by
//!                                 a ROM, RAM or table's `file=<name>`
//!   machines/<name>.fsm           a state machine (see [`crate::fsm`])
//! ```
//!
//! A `.circ` file is a line per part, sorted by id, then a line per wire,
//! sorted, so moving one part changes one line:
//!
//! ```text
//! part A in at 2,4 bits=8 label=A
//! part U1 and at 12,4 inputs=3
//! part S sub:alu at 30,2 rot 1
//! wire 10,4 12,4
//! ```
//!
//! Positions are grid steps. Pins sit where Digital puts them, so a
//! circuit is built through the same reader as a `.dig` file (see
//! [`crate::dig`]). Another circuit of the project is placed as
//! `sub:<name>`; its `in` and `out` parts, in id order, are its pins.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dig::{self, Attr, Element};
use crate::edit;

const VERSION: u32 = 1;
const G: i32 = 20;

/// A part on the grid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub id: String,
    /// `and`, `in`, `mux`… or `sub:<circuit>`.
    pub kind: String,
    pub x: i32,
    pub y: i32,
    /// Quarter turns, anticlockwise as Digital turns.
    #[serde(default)]
    pub rot: u8,
    #[serde(default)]
    pub mirror: bool,
    /// Settings by their `.circ` names: `bits`, `inputs`, `label`…
    #[serde(default)]
    pub attrs: BTreeMap<String, String>,
}

/// A wire: a segment between two grid points — straight as drawn here, at a
/// slant if Digital drew it so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Wire {
    pub a: (i32, i32),
    pub b: (i32, i32),
}

impl Wire {
    /// The lesser end first, so one wire has one way of being written.
    pub fn new(a: (i32, i32), b: (i32, i32)) -> Wire {
        if a <= b {
            Wire { a, b }
        } else {
            Wire { a: b, b: a }
        }
    }
}

/// One circuit: its parts and its wires.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drawing {
    pub parts: Vec<Part>,
    pub wires: Vec<Wire>,
    /// Its box's width in grid steps when placed in another; 3 when unsaid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
}

/// A part kind's `.circ` name and Digital's.
const KINDS: &[(&str, &str)] = &[
    ("in", "In"),
    ("out", "Out"),
    ("clock", "Clock"),
    ("button", "Button"),
    ("reset", "Reset"),
    ("break", "Break"),
    ("const", "Const"),
    ("ground", "Ground"),
    ("vdd", "VDD"),
    ("led", "LED"),
    ("probe", "Probe"),
    ("tunnel", "Tunnel"),
    ("split", "Splitter"),
    ("text", "Text"),
    ("not", "Not"),
    ("and", "And"),
    ("or", "Or"),
    ("nand", "NAnd"),
    ("nor", "NOr"),
    ("xor", "XOr"),
    ("xnor", "XNOr"),
    ("mux", "Multiplexer"),
    ("demux", "Demultiplexer"),
    ("decoder", "Decoder"),
    ("priority", "PriorityEncoder"),
    ("add", "Add"),
    ("subtract", "Sub"),
    ("multiply", "Mul"),
    ("divide", "Div"),
    ("compare", "Comparator"),
    ("negate", "Neg"),
    ("shifter", "BarrelShifter"),
    ("register", "Register"),
    ("dff", "D_FF"),
    ("tff", "T_FF"),
    ("jkff", "JK_FF"),
    ("rsff", "RS_FF"),
    ("ram2", "RAMDualAccess"),
    ("ram", "RAMDualPort"),
    ("rom", "ROM"),
    ("lut", "LookUpTable"),
    ("counter", "Counter"),
    ("driver", "Driver"),
    ("driverinv", "DriverInvSel"),
    ("pullup", "PullUp"),
    ("pulldown", "PullDown"),
    ("bitext", "BitExtender"),
    ("bitsel", "BitSelector"),
    ("seg7", "Seven-Seg"),
    ("hexdisplay", "Seven-Seg-Hex"),
    ("rgbled", "RGBLED"),
];

/// How a setting is typed in Digital's files.
#[derive(Clone, Copy, PartialEq)]
enum Ty {
    Int,
    Str,
    Bool,
}

/// A setting's `.circ` name, Digital's, and its type.
const ATTRS: &[(&str, &str, Ty)] = &[
    ("bits", "Bits", Ty::Int),
    ("inputs", "Inputs", Ty::Int),
    ("sel", "Selector Bits", Ty::Int),
    ("addr", "AddrBits", Ty::Int),
    ("value", "Value", Ty::Int),
    ("spread", "splitterSpreading", Ty::Int),
    ("label", "Label", Ty::Str),
    ("net", "NetName", Ty::Str),
    ("in", "Input Splitting", Ty::Str),
    ("out", "Output Splitting", Ty::Str),
    ("mode", "barrelShifterMode", Ty::Str),
    ("dir", "direction", Ty::Str),
    ("text", "Description", Ty::Str),
    ("inbits", "inputBits", Ty::Int),
    ("outbits", "outputBits", Ty::Int),
    ("data", "Data", Ty::Str),
    // Resolved to `data` from the project's data/<name>.hex when built.
    ("file", "file", Ty::Str),
    ("wide", "wideShape", Ty::Bool),
    ("flip", "flipSelPos", Ty::Bool),
    ("signed", "signed", Ty::Bool),
    ("shiftsigned", "barrelSigned", Ty::Bool),
    ("probe", "valueIsProbe", Ty::Bool),
    ("pc", "isProgramCounter", Ty::Bool),
    ("program", "isProgramMemory", Ty::Bool),
];

/// The part kinds a `.circ` file can name, besides `sub:<circuit>`.
pub fn kinds() -> impl Iterator<Item = &'static str> {
    KINDS.iter().map(|(k, _)| *k)
}

/// The settings a `.circ` file can give.
pub fn settings() -> impl Iterator<Item = &'static str> {
    ATTRS.iter().map(|(k, _, _)| *k)
}

/// Ids sorted as people count: `U2` before `U10`.
fn id_order(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let digits = s.len() - s.bytes().rev().take_while(u8::is_ascii_digit).count();
        let n: u64 = s[digits..].parse().unwrap_or(0);
        (s[..digits].to_string(), n, s.len())
    };
    split(a).cmp(&split(b)).then_with(|| a.cmp(b))
}

fn bare(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.,*-+:/#".contains(&c))
}

fn quote(v: &str) -> String {
    if bare(v) {
        return v.to_string();
    }
    let mut s = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

fn point(s: &str) -> Option<(i32, i32)> {
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// A line's words, a quoted value kept whole: `label="A B"` is one.
fn words(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => match chars.next() {
                Some('n') => cur.push('\n'),
                Some(c) => cur.push(c),
                None => return Err("a line ends in \\".into()),
            },
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if quoted {
        return Err("a quote is not closed".into());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

/// A `.circ` file read, its parts in id order and its wires sorted.
pub fn parse(text: &str) -> Result<Drawing, String> {
    let mut d = Drawing::default();
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let w = words(line).map_err(|e| format!("line {n}: {e}"))?;
        match w[0].as_str() {
            "part" => {
                if w.len() < 5 || w[3] != "at" {
                    return Err(format!("line {n}: a part is “part <id> <kind> at <x>,<y> …”"));
                }
                let (x, y) = point(&w[4]).ok_or_else(|| format!("line {n}: “{}” is not a point", w[4]))?;
                let mut p = Part { id: w[1].clone(), kind: w[2].clone(), x, y, rot: 0, mirror: false, attrs: BTreeMap::new() };
                let mut i = 5;
                while i < w.len() {
                    match w[i].as_str() {
                        "rot" => {
                            p.rot = w.get(i + 1).and_then(|r| r.parse::<u8>().ok()).ok_or_else(|| format!("line {n}: rot takes 0 to 3"))? % 4;
                            i += 1;
                        }
                        "mirror" => p.mirror = true,
                        kv => {
                            let (k, v) = kv.split_once('=').ok_or_else(|| format!("line {n}: “{kv}” is not a setting"))?;
                            p.attrs.insert(k.to_string(), v.to_string());
                        }
                    }
                    i += 1;
                }
                d.parts.push(p);
            }
            "wire" => {
                let (a, b) = match (w.get(1).and_then(|s| point(s)), w.get(2).and_then(|s| point(s))) {
                    (Some(a), Some(b)) if w.len() == 3 => (a, b),
                    _ => return Err(format!("line {n}: a wire is “wire <x>,<y> <x>,<y>”")),
                };
                // Drawn here, a wire runs straight; one from Digital may
                // slant, and joins only at its ends all the same.
                d.wires.push(Wire::new(a, b));
            }
            "width" => {
                d.width = Some(w.get(1).and_then(|s| s.parse().ok()).ok_or_else(|| format!("line {n}: width takes a number"))?);
            }
            other => return Err(format!("line {n}: “{other}” is not part, wire or width")),
        }
    }
    // Held as it is written: parts in id order, wires sorted.
    d.parts.sort_by(|a, b| id_order(&a.id, &b.id));
    d.wires.sort();
    d.wires.dedup();
    let mut seen = std::collections::BTreeSet::new();
    for p in &d.parts {
        if !seen.insert(p.id.as_str()) {
            return Err(format!("two parts are called {}", p.id));
        }
    }
    Ok(d)
}

/// A circuit as its `.circ` text: the same circuit, the same bytes.
pub fn write(d: &Drawing) -> String {
    let mut parts: Vec<&Part> = d.parts.iter().collect();
    parts.sort_by(|a, b| id_order(&a.id, &b.id));
    let mut wires = d.wires.clone();
    wires.sort();
    wires.dedup();
    let mut s = String::new();
    if let Some(w) = d.width {
        s.push_str(&format!("width {w}\n"));
    }
    for p in parts {
        s.push_str(&format!("part {} {} at {},{}", p.id, p.kind, p.x, p.y));
        if p.rot % 4 != 0 {
            s.push_str(&format!(" rot {}", p.rot % 4));
        }
        if p.mirror {
            s.push_str(" mirror");
        }
        for (k, v) in &p.attrs {
            s.push_str(&format!(" {k}={}", quote(v)));
        }
        s.push('\n');
    }
    for w in wires {
        s.push_str(&format!("wire {},{} {},{}\n", w.a.0, w.a.1, w.b.0, w.b.1));
    }
    s
}

/// A drawing's parts in the order the reader is given them: by id. A
/// built circuit's `top_parts` are in this order.
pub fn ordered(d: &Drawing) -> Vec<&Part> {
    let mut parts: Vec<&Part> = d.parts.iter().collect();
    parts.sort_by(|a, b| id_order(&a.id, &b.id));
    parts
}

/// A circuit as the reader takes it: Digital's names, Digital's units.
/// Its wires are in the drawing's order; a built circuit's `top_wires` too.
/// A part's `file=` is left as it is; [`Project`] reads it.
pub fn to_dig(d: &Drawing) -> Result<dig::Circuit, String> {
    to_dig_with(d, &BTreeMap::new())
}

/// As [`to_dig`], each `file=<name>` read from these contents by name.
pub fn to_dig_with(d: &Drawing, data: &BTreeMap<String, String>) -> Result<dig::Circuit, String> {
    // In and out parts first, in id order: they are the pins.
    let parts = ordered(d);
    let mut elements = Vec::new();
    for p in parts {
        let name = if let Some(sub) = p.kind.strip_prefix("sub:") {
            format!("{sub}.circ")
        } else {
            KINDS.iter().find(|(k, _)| *k == p.kind).map(|(_, d)| d.to_string()).ok_or_else(|| format!("{}: “{}” is not a part Proton knows", p.id, p.kind))?
        };
        let mut attrs = HashMap::new();
        for (k, v) in &p.attrs {
            let &(_, key, ty) = ATTRS.iter().find(|(c, _, _)| c == k).ok_or_else(|| format!("{}: “{k}” is not a setting", p.id))?;
            // Digital's comparator spells it with a capital.
            let key = if key == "signed" && name == "Comparator" { "Signed" } else { key };
            let val = match ty {
                Ty::Int => Attr::Int(v.parse().map_err(|_| format!("{}: {k} takes a number, not “{v}”", p.id))?),
                Ty::Str => Attr::Str(v.clone()),
                Ty::Bool => Attr::Bool(matches!(v.as_str(), "true" | "yes" | "1")),
            };
            attrs.insert(key.to_string(), val);
        }
        if let Some(Attr::Str(file)) = attrs.remove("file") {
            let name = file.trim_start_matches("data/").trim_end_matches(".hex");
            let text = data.get(name).ok_or_else(|| format!("{}: there is no data/{name}.hex", p.id))?;
            attrs.insert("Data".into(), Attr::Str(text.clone()));
        }
        if p.rot % 4 != 0 {
            attrs.insert("rotation".into(), Attr::Rotation(p.rot % 4));
        }
        if p.mirror {
            attrs.insert("mirror".into(), Attr::Bool(true));
        }
        elements.push(Element { name, attrs, x: p.x * G, y: p.y * G });
    }
    let wires = d.wires.iter().map(|w| ((w.a.0 * G, w.a.1 * G), (w.b.0 * G, w.b.1 * G))).collect();
    Ok(dig::Circuit { elements, wires, width: d.width })
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Stored {
    version: u32,
    top: String,
}

/// A project: its circuits by name, and their test tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// The circuit that runs when the project does.
    pub top: String,
    pub circuits: BTreeMap<String, Drawing>,
    /// Test tables by circuit, then label.
    pub tests: BTreeMap<String, BTreeMap<String, String>>,
    /// Memory contents by name, as `data/<name>.hex` holds them.
    pub data: BTreeMap<String, String>,
    /// State machines by name, as `machines/<name>.fsm` holds them.
    pub machines: BTreeMap<String, String>,
}

/// A name a file can carry: no separators, no dots, nothing hidden.
pub fn is_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && !s.starts_with('-') && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ')
}

/// A pin as drawn: where it is, in grid steps, and whether it is an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinAt {
    pub x: i32,
    pub y: i32,
    pub input: bool,
}

impl Project {
    fn library(&self) -> Result<dig::Library, String> {
        let mut lib = HashMap::new();
        for (n, d) in &self.circuits {
            lib.insert(format!("{n}.circ"), to_dig_with(d, &self.data).map_err(|e| format!("{n}: {e}"))?);
        }
        Ok(dig::Library::of(lib))
    }

    /// Each part of a circuit by id, and where its pins are — or why they
    /// could not be placed: a part Proton does not know, a circuit not in
    /// the project.
    pub fn pins(&self, circuit: &str) -> Vec<(String, Result<Vec<PinAt>, String>)> {
        let Some(d) = self.circuits.get(circuit) else { return Vec::new() };
        let mut lib = HashMap::new();
        for (n, d) in &self.circuits {
            // A circuit that does not convert places nothing; its parts say why.
            if let Ok(c) = to_dig_with(d, &self.data) {
                lib.insert(format!("{n}.circ"), c);
            }
        }
        let mut lib = dig::Library::of(lib);
        d.parts
            .iter()
            .map(|p| {
                let one = Drawing { parts: vec![p.clone()], wires: Vec::new(), width: None };
                let placed = to_dig_with(&one, &self.data).and_then(|c| dig::pins_of(&c.elements[0], &mut lib)).map(|pins| {
                    pins.into_iter().map(|((x, y), input)| PinAt { x: x.div_euclid(G), y: y.div_euclid(G), input }).collect()
                });
                (p.id.clone(), placed)
            })
            .collect()
    }

    /// A new project: one circuit, `main`, empty.
    pub fn new() -> Project {
        let mut p = Project { top: "main".into(), ..Default::default() };
        p.circuits.insert("main".into(), Drawing::default());
        p
    }

    /// The circuit `name` built, its subcircuits inlined and its tables
    /// attached, ready to run.
    pub fn build(&self, name: &str) -> Result<dig::Built, String> {
        let mut built = dig::build_from(self.library()?, &format!("{name}.circ"))?;
        if let Some(t) = self.tests.get(name) {
            built.tests.extend(t.iter().map(|(l, t)| (l.clone(), t.clone())));
        }
        Ok(built)
    }
}

/// A folder read into a project.
pub fn read(dir: &Path) -> Result<Project, String> {
    let shown = dir.display();
    let text = std::fs::read_to_string(dir.join("project.json")).map_err(|e| format!("{shown}/project.json: {e}"))?;
    let stored: Stored = serde_json::from_str(&text).map_err(|e| format!("{shown}/project.json: {e}"))?;
    if stored.version > VERSION {
        return Err(format!("{shown} was written by a newer Proton (version {})", stored.version));
    }
    let mut p = Project { top: stored.top, ..Default::default() };
    for (sub, ext) in [("circuits", "circ"), ("tests", "tests"), ("data", "hex"), ("machines", "fsm")] {
        let Ok(entries) = std::fs::read_dir(dir.join(sub)) else { continue };
        let mut names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for file in names {
            let Some(stem) = file.strip_suffix(&format!(".{ext}")) else { continue };
            if stem.starts_with('.') {
                continue;
            }
            let path = dir.join(sub).join(&file);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if ext == "circ" {
                p.circuits.insert(stem.to_string(), parse(&text).map_err(|e| format!("{sub}/{file}: {e}"))?);
            } else if ext == "hex" {
                p.data.insert(stem.to_string(), text);
            } else if ext == "fsm" {
                p.machines.insert(stem.to_string(), text);
            } else {
                let (circuit, label) = stem.split_once('.').unwrap_or((stem, ""));
                p.tests.entry(circuit.to_string()).or_default().insert(label.to_string(), text);
            }
        }
    }
    if !p.circuits.contains_key(&p.top) {
        return Err(format!("{shown}: its top circuit, {}, is not in circuits/", p.top));
    }
    Ok(p)
}

/// A project written into its folder: each file only if it changed, each
/// whole or not at all, and circuits and tables no longer there taken away.
pub fn write_project(dir: &Path, p: &Project) -> Result<(), String> {
    let mut want: BTreeMap<(&str, String), String> = BTreeMap::new();
    for (n, d) in &p.circuits {
        want.insert(("circuits", format!("{n}.circ")), write(d));
    }
    for (c, tables) in &p.tests {
        for (l, t) in tables {
            let file = if l.is_empty() { format!("{c}.tests") } else { format!("{c}.{l}.tests") };
            want.insert(("tests", file), t.clone());
        }
    }
    for (n, text) in &p.data {
        want.insert(("data", format!("{n}.hex")), text.clone());
    }
    for (n, text) in &p.machines {
        want.insert(("machines", format!("{n}.fsm")), text.clone());
    }
    for sub in ["circuits", "tests", "data", "machines"] {
        let d = dir.join(sub);
        if want.keys().any(|(s, _)| *s == sub) {
            std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
    }
    for ((sub, file), text) in &want {
        put(&dir.join(sub).join(file), text)?;
    }
    for (sub, ext) in [("circuits", ".circ"), ("tests", ".tests"), ("data", ".hex"), ("machines", ".fsm")] {
        if let Ok(entries) = std::fs::read_dir(dir.join(sub)) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(ext) && !name.starts_with('.') && !want.contains_key(&(sub, name.clone())) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    // Last, so a folder read halfway through a save names a top that is whole.
    let mut json = serde_json::to_string_pretty(&Stored { version: VERSION, top: p.top.clone() }).unwrap_or_default();
    json.push('\n');
    put(&dir.join("project.json"), &json)
}

/// `text` into `path`, through a file beside it renamed over it; nothing
/// done when it already says that.
fn put(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read(path).ok().as_deref() == Some(text.as_bytes()) {
        return Ok(());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let partial = path.with_file_name(format!(".{name}.part"));
    std::fs::write(&partial, text).map_err(|e| format!("{}: {e}", partial.display()))?;
    std::fs::rename(&partial, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A Digital `.dig` file, and every circuit it places, as a project: its
/// circuits named for their files, its test cases as tables, `top` the
/// file given. The parts Proton has no kind for are left out and named,
/// file by file.
pub fn import_dig(dir: &Path, file: &str) -> Result<(Project, Vec<String>), String> {
    let mut lib = dig::Library::new(dir);
    let stem = |f: &str| f.trim_end_matches(".dig").to_string();
    let mut p = Project { top: stem(file), ..Default::default() };
    let mut dropped: Vec<String> = Vec::new();
    let mut queue = vec![file.to_string()];
    while let Some(f) = queue.pop() {
        let name = stem(&f);
        if p.circuits.contains_key(&name) {
            continue;
        }
        if !is_name(&name) {
            return Err(format!("{f}: “{name}” cannot name a circuit"));
        }
        let c = lib.get(&f)?.clone();
        let mut d = Drawing { width: c.width, ..Default::default() };
        let (mut pins_in, mut pins_out) = (0, 0);
        let mut left_out: BTreeMap<String, usize> = BTreeMap::new();
        let mut tests: BTreeMap<String, String> = BTreeMap::new();
        for el in &c.elements {
            let at = |v: i32| (v as f64 / G as f64).round() as i32;
            let kind = if dig::is_sub(&el.name) {
                let sub = stem(&el.name);
                queue.push(el.name.clone());
                format!("sub:{sub}")
            } else if el.name == "Testcase" {
                if let Some(Attr::Test(t)) = el.attrs.get("Testdata") {
                    let mut label: String = el.label().chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '_' }).collect();
                    if label.is_empty() {
                        label = "test".into();
                    }
                    let mut unique = label.clone();
                    let mut n = 2;
                    while tests.contains_key(&unique) {
                        unique = format!("{label}{n}");
                        n += 1;
                    }
                    tests.insert(unique, if t.ends_with('\n') { t.clone() } else { format!("{t}\n") });
                }
                continue;
            } else if let Some((k, _)) = KINDS.iter().find(|(_, dn)| *dn == el.name) {
                k.to_string()
            } else {
                *left_out.entry(el.name.clone()).or_default() += 1;
                continue;
            };
            // Inputs and outputs in file order: Digital's pin order.
            let id = match kind.as_str() {
                "in" | "clock" => {
                    pins_in += 1;
                    format!("I{pins_in}")
                }
                "out" => {
                    pins_out += 1;
                    format!("O{pins_out}")
                }
                k => edit::fresh_id(&d, edit::prefix(k)),
            };
            let mut attrs = BTreeMap::new();
            for (key, v) in &el.attrs {
                let key = if key == "Signed" { "signed" } else { key.as_str() };
                let Some((ck, _, _)) = ATTRS.iter().find(|(_, dk, _)| *dk == key) else { continue };
                let v = match v {
                    Attr::Str(s) => s.clone(),
                    Attr::Int(i) => i.to_string(),
                    Attr::Bool(b) => b.to_string(),
                    _ => continue,
                };
                if !v.is_empty() {
                    attrs.insert(ck.to_string(), v);
                }
            }
            let rot = match el.attrs.get("rotation") {
                Some(Attr::Rotation(r)) => *r % 4,
                _ => 0,
            };
            d.parts.push(Part { id, kind, x: at(el.x), y: at(el.y), rot, mirror: el.flag("mirror"), attrs });
        }
        for (a, b) in &c.wires {
            let g = |q: (i32, i32)| ((q.0 as f64 / G as f64).round() as i32, (q.1 as f64 / G as f64).round() as i32);
            let (a, b) = (g(*a), g(*b));
            if a != b {
                d.wires.push(Wire::new(a, b));
            }
        }
        for (k, n) in left_out {
            dropped.push(format!("{f}: {n} × {k}"));
        }
        // Held as it is written.
        let d = parse(&write(&d)).map_err(|e| format!("{f}: {e}"))?;
        p.circuits.insert(name.clone(), d);
        if !tests.is_empty() {
            p.tests.insert(name, tests);
        }
    }
    Ok((p, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A half adder, with its table. Wires join only at their ends, so a
    /// branch splits the wire it leaves.
    fn adder() -> Project {
        let half = "\
part A in at 0,0 label=A
part B in at 0,2 label=B
part C out at 8,4 label=C
part S out at 8,0 label=S
part X xor at 4,0
part Y and at 4,4
wire 0,0 2,0
wire 2,0 4,0
wire 0,2 3,2
wire 3,2 4,2
wire 2,0 2,4
wire 2,4 4,4
wire 3,2 3,6
wire 3,6 4,6
wire 7,1 8,1
wire 8,0 8,1
wire 7,5 8,5
wire 8,4 8,5
";
        let mut p = Project::new();
        p.circuits.insert("half".into(), parse(half).unwrap());
        p.top = "half".into();
        p.circuits.remove("main");
        p.tests.entry("half".into()).or_default().insert("table".into(), "A B S C\n0 0 0 0\n0 1 1 0\n1 0 1 0\n1 1 0 1\n".into());
        p
    }

    #[test]
    fn a_circuit_is_written_sorted_and_reads_back_the_same_bytes() {
        let text = "part U10 and at 1,1\npart U2 or at 5,5 rot 1 mirror inputs=3 label=\"two words\"\nwire 4,4 1,4\nwire 0,0 0,3\n";
        let d = parse(text).unwrap();
        let out = write(&d);
        assert_eq!(out, "part U2 or at 5,5 rot 1 mirror inputs=3 label=\"two words\"\npart U10 and at 1,1\nwire 0,0 0,3\nwire 1,4 4,4\n");
        assert_eq!(write(&parse(&out).unwrap()), out);
        assert_eq!(parse("wire 0,0 1,1").unwrap().wires.len(), 1, "a slanting wire, as Digital draws some, is read");
        assert!(parse("part A in at 0,0\npart A out at 1,1").unwrap_err().contains("two parts"));
    }

    #[test]
    fn a_project_runs_its_tables_and_saves_only_what_changed() {
        let p = adder();
        let b = p.build("half").unwrap();
        let outcomes = crate::testlang::run_all(&b);
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].passed(), "{:?} {:?}", outcomes[0].failures, outcomes[0].error);

        let dir = std::env::temp_dir().join(format!("proton-circuit-circ-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_project(&dir, &p).unwrap();
        assert!(dir.join("circuits/half.circ").exists() && dir.join("tests/half.table.tests").exists());
        let back = read(&dir).unwrap();
        assert_eq!(back, p);
        let file = dir.join("circuits/half.circ");
        let when = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut q = back.clone();
        q.tests.clear();
        write_project(&dir, &q).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), when, "an unchanged circuit is not written");
        assert!(!dir.join("tests/half.table.tests").exists(), "a table taken away goes from the folder");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_built_circuit_says_which_part_and_net_each_drawn_one_is() {
        // A button and a clock into an AND, to an LED and a Break; one
        // wire left touching nothing.
        let d = parse(
            "part B button at 0,0 label=B\npart C clock at 0,2 label=C\npart U and at 2,0\npart L led at 7,1\npart X break at 7,3\n\
             wire 0,0 2,0\nwire 0,2 2,2\nwire 5,1 7,1\nwire 6,1 6,3\nwire 6,3 7,3\nwire 20,20 21,20\n",
        )
        .unwrap();
        let mut p = Project::new();
        p.circuits.insert("main".into(), d.clone());
        let b = p.build("main").unwrap();
        let order: Vec<&str> = ordered(&d).iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order, vec!["B", "C", "L", "U", "X"]);
        assert!(b.top_parts.iter().all(Option::is_some));
        assert_eq!(b.breaks.len(), 1);
        let net_of = |i: usize| b.top_wires[i];
        // Wires are held sorted: (0,0)-(2,0), (0,2)-(2,2), (5,1)-(7,1), (6,1)-(6,3), (6,3)-(7,3), (20,20)-(21,20).
        assert!(net_of(5).is_none(), "a wire on no pin has no net");
        let mut sim = crate::Sim::new(b.netlist, 1).unwrap();
        let (button, clock) = (b.top_parts[0].unwrap(), b.top_parts[1].unwrap());
        sim.set(button, 1).unwrap();
        sim.set(clock, 1).unwrap();
        assert_eq!(sim.value(net_of(2).unwrap()).get(), 1, "the AND's output wire is high");
        // The (5,1)-(7,1) wire and the (6,1)-(6,3) one share no end, so the
        // Break is fed only through its own leg.
        assert!(sim.shown(b.breaks[0]).is_z() || sim.shown(b.breaks[0]).get() == 0);
    }

    fn run(p: &Project) -> (dig::Built, crate::Sim) {
        let b = p.build("main").unwrap();
        let sim = crate::Sim::new(b.netlist.clone(), 1).unwrap();
        (b, sim)
    }

    fn project(main: &str) -> Project {
        let mut p = Project::new();
        p.circuits.insert("main".into(), parse(main).unwrap());
        p
    }

    #[test]
    fn a_counter_addresses_a_rom_whose_contents_are_a_data_file() {
        let mut p = project(
            "part K1 const at 0,0\npart Clk clock at 0,1 label=Clk\npart K0 const at 0,2 value=0\npart N counter at 2,0 bits=2\n\
             part R rom at 7,0 addr=2 bits=8 file=table\npart K2 const at 6,1\npart D out at 12,0 bits=8 label=D\n\
             wire 0,0 2,0\nwire 0,1 2,1\nwire 0,2 2,2\nwire 5,0 7,0\nwire 6,1 7,1\nwire 10,0 12,0\n",
        );
        assert!(p.build("main").unwrap_err().contains("data/table.hex"), "a file that is not there is said");
        p.data.insert("table".into(), "v2.0 raw\n11 22 33 44\n".into());
        let (b, mut sim) = run(&p);
        let (clk, d) = (b.inputs["Clk"].0, b.outputs["D"]);
        let mut seen = vec![sim.shown(d).get()];
        for _ in 0..4 {
            sim.tick(clk).unwrap();
            seen.push(sim.shown(d).get());
        }
        assert_eq!(seen, vec![0x11, 0x22, 0x33, 0x44, 0x11]);
        // The folder keeps it as data/table.hex.
        let dir = std::env::temp_dir().join(format!("proton-circuit-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_project(&dir, &p).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("data/table.hex")).unwrap(), "v2.0 raw\n11 22 33 44\n");
        assert_eq!(read(&dir).unwrap(), p);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn displays_drivers_pulls_tables_and_bit_parts_run() {
        // A tri-state driver onto a pulled-down net; a hex display; a
        // table of XOR; a bit picked and a value sign-extended.
        let p = project(
            "part A in at 0,0 label=A\npart E in at 0,1 label=E\npart T driver at 2,0\npart P pulldown at 6,0\npart Y out at 6,0 label=Y\n\
             part Dg in at 0,4 bits=4 label=Dg\npart H hexdisplay at 2,4 label=H\n\
             part X1 in at 0,7 label=X1\npart X2 in at 0,8 label=X2\npart L lut at 2,7 inputs=2 data=0,1,1,0\npart Q out at 6,7 label=Q\n\
             part V in at 0,11 bits=4 label=V\npart S bitext at 2,11 inbits=4 outbits=8\npart W out at 6,11 bits=8 label=W\n\
             part Sel in at 0,14 bits=2 label=Sel\npart Vb in at 0,13 bits=4 label=Vb\npart B bitsel at 2,13 bits=4 sel=2\npart Bt out at 5,13 label=Bt\n\
             wire 0,0 2,0\nwire 0,1 2,1\nwire 4,0 6,0\nwire 0,4 2,4\nwire 0,7 2,7\nwire 0,8 2,8\nwire 5,7 6,7\n\
             wire 0,11 2,11\nwire 5,11 6,11\nwire 0,13 2,13\nwire 0,14 2,14\nwire 4,13 5,13\n",
        );
        let (b, mut sim) = run(&p);
        let set = |sim: &mut crate::Sim, k: &str, v: u64| {
            sim.set(b.inputs[k].0, v).unwrap();
        };
        let out = |sim: &crate::Sim, k: &str| sim.shown(b.outputs[k]);
        set(&mut sim, "A", 1);
        assert_eq!(out(&sim, "Y").get(), 0, "the driver is off: the net is pulled down");
        set(&mut sim, "E", 1);
        assert_eq!(out(&sim, "Y").get(), 1);
        set(&mut sim, "Dg", 0xa);
        let h = out(&sim, "H");
        assert_eq!((h.bits, h.get() & 0xf), (5, 0xa), "the digit, with its point beside it on one bus");
        for (x1, x2) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            set(&mut sim, "X1", x1);
            set(&mut sim, "X2", x2);
            assert_eq!(out(&sim, "Q").get(), x1 ^ x2);
        }
        set(&mut sim, "V", 0b1010);
        assert_eq!(out(&sim, "W").get(), 0xfa, "sign-extended");
        set(&mut sim, "Vb", 0b0100);
        set(&mut sim, "Sel", 2);
        assert_eq!(out(&sim, "Bt").get(), 1);
    }

    #[test]
    fn a_dig_file_and_its_subcircuits_come_in_as_a_project_that_passes_its_tests() {
        // pass.dig: B listed first, so its first pin; and.dig places it, with
        // a test and a part Proton does not have.
        let dir = std::env::temp_dir().join(format!("proton-circuit-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let el = |name: &str, attrs: &str, x: i32, y: i32| format!("<visualElement><elementName>{name}</elementName><elementAttributes>{attrs}</elementAttributes><pos x=\"{x}\" y=\"{y}\"/></visualElement>");
        let label = |l: &str| format!("<entry><string>Label</string><string>{l}</string></entry>");
        let wire = |a: (i32, i32), b: (i32, i32)| format!("<wire><p1 x=\"{}\" y=\"{}\"/><p2 x=\"{}\" y=\"{}\"/></wire>", a.0, a.1, b.0, b.1);
        let file = |els: Vec<String>, wires: Vec<String>| format!("<?xml version=\"1.0\"?><circuit><version>2</version><attributes/><visualElements>{}</visualElements><wires>{}</wires></circuit>", els.concat(), wires.concat());
        std::fs::write(
            dir.join("pass.dig"),
            file(vec![el("In", &label("B"), 0, 100), el("Out", &label("Y"), 100, 100), el("In", &label("A"), 0, 0)], vec![wire((0, 100), (100, 100))]),
        )
        .unwrap();
        let test = "<entry><string>Testdata</string><testData><dataString>P Q Y\n1 0 1\n0 1 0\n</dataString></testData></entry><entry><string>Label</string><string>first pin</string></entry>";
        std::fs::write(
            dir.join("top.dig"),
            file(
                vec![el("In", &label("P"), 100, 0), el("In", &label("Q"), 100, 40), el("pass.dig", "", 200, 0), el("Out", &label("Y"), 300, 20), el("Rectangle", "", 0, 0), el("Testcase", test, 0, 200)],
                vec![wire((100, 0), (200, 0)), wire((100, 40), (200, 40)), wire((260, 20), (300, 20))],
            ),
        )
        .unwrap();
        let (p, dropped) = import_dig(&dir, "top.dig").unwrap();
        assert_eq!(p.top, "top");
        assert_eq!(p.circuits.keys().cloned().collect::<Vec<_>>(), vec!["pass", "top"]);
        assert_eq!(dropped, vec!["top.dig: 1 × Rectangle"]);
        assert!(write(&p.circuits["top"]).contains("part S1 sub:pass at 10,0"), "{}", write(&p.circuits["top"]));
        assert_eq!(p.tests["top"].keys().cloned().collect::<Vec<_>>(), vec!["first_pin"]);
        let b = p.build("top").unwrap();
        let outcomes = crate::testlang::run_all(&b);
        assert!(outcomes[0].passed(), "{:?} {:?}", outcomes[0].failures, outcomes[0].error);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_subcircuit_is_placed_by_name_with_its_pins_in_id_order() {
        // pass: A wired to Y; B open. Two inputs and one output, so drawn
        // like a gate: A at 0, B at 2 steps, Y at (3, 1).
        let mut p = Project::new();
        p.circuits.insert("pass".into(), parse("part A in at 0,0 label=A\npart B in at 0,5 label=B\npart Y out at 5,0 label=Y\nwire 0,0 5,0\n").unwrap());
        p.circuits.insert(
            "main".into(),
            parse("part P in at 0,0 label=P\npart Q in at 0,2 label=Q\npart U sub:pass at 2,0\npart Y out at 6,1 label=Y\nwire 0,0 2,0\nwire 0,2 2,2\nwire 5,1 6,1\n").unwrap(),
        );
        let b = p.build("main").unwrap();
        let pins = p.pins("main");
        let u = &pins.iter().find(|(id, _)| id == "U").unwrap().1;
        assert_eq!(u.as_ref().unwrap(), &vec![PinAt { x: 2, y: 0, input: true }, PinAt { x: 2, y: 2, input: true }, PinAt { x: 5, y: 1, input: false }]);
        let mut sim = crate::Sim::new(b.netlist, 1).unwrap();
        sim.set(b.inputs["P"].0, 1).unwrap();
        assert_eq!(sim.shown(b.outputs["Y"]).get(), 1);
        assert!(Project::new().build("nowhere").unwrap_err().contains("nowhere"));
    }
}
