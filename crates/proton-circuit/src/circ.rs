//! The `Name.circuit/` folder: a project of circuits in text that diffs.
//!
//! ```text
//! Name.circuit/
//!   project.json                  which circuit is the top
//!   circuits/<name>.circ          a circuit: a line per part, a line per wire
//!   tests/<circuit>.<label>.tests a test table for a circuit, in Digital's language
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

/// A wire: one straight segment between two grid points.
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
                if a.0 != b.0 && a.1 != b.1 {
                    return Err(format!("line {n}: a wire runs straight across or down"));
                }
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

/// A circuit as the reader takes it: Digital's names, Digital's units.
pub fn to_dig(d: &Drawing) -> Result<dig::Circuit, String> {
    // In and out parts first, in id order: they are the pins.
    let mut parts: Vec<&Part> = d.parts.iter().collect();
    parts.sort_by(|a, b| id_order(&a.id, &b.id));
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
            lib.insert(format!("{n}.circ"), to_dig(d).map_err(|e| format!("{n}: {e}"))?);
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
            if let Ok(c) = to_dig(d) {
                lib.insert(format!("{n}.circ"), c);
            }
        }
        let mut lib = dig::Library::of(lib);
        d.parts
            .iter()
            .map(|p| {
                let one = Drawing { parts: vec![p.clone()], wires: Vec::new(), width: None };
                let placed = to_dig(&one).and_then(|c| dig::pins_of(&c.elements[0], &mut lib)).map(|pins| {
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
    for (sub, ext) in [("circuits", "circ"), ("tests", "tests")] {
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
    for sub in ["circuits", "tests"] {
        let d = dir.join(sub);
        if want.keys().any(|(s, _)| *s == sub) {
            std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
    }
    for ((sub, file), text) in &want {
        put(&dir.join(sub).join(file), text)?;
    }
    for (sub, ext) in [("circuits", ".circ"), ("tests", ".tests")] {
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
        assert!(parse("wire 0,0 1,1").unwrap_err().contains("straight"));
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
