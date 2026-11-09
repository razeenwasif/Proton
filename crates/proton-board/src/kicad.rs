//! KiCad's libraries read: its symbols (`.kicad_sym`) and footprints (a
//! `.pretty` folder of `.kicad_mod`), so their parts can be used — `part U1
//! kicad "Device:R"`, `footprint "Resistor_SMD:R_0603_1608Metric"` — from
//! the project's own libraries, or KiCad's where it is installed.
//!
//! Both are S-expressions. A symbol's pins keep their numbers, names and
//! electrical types (KiCad's `power_in` is a power input here, `no_connect`
//! not to be connected, `unspecified` and `free` passive); a symbol that
//! `extends` another takes its pins. A footprint's pads keep their numbers,
//! shapes, sizes and drills, its courtyard and body their boxes; KiCad's y
//! runs down, Proton's up.

use std::path::{Path, PathBuf};

use crate::footprint::{Footprint, Pad, Shape};
use crate::symbol::{Body, Pin, PinKind, Symbol};

/// An S-expression: a word, a string, or a list.
#[derive(Debug, Clone, PartialEq)]
pub enum Sexp {
    Atom(String),
    Str(String),
    List(Vec<Sexp>),
}

impl Sexp {
    /// The list's head word.
    pub fn head(&self) -> Option<&str> {
        match self {
            Sexp::List(xs) => match xs.first() {
                Some(Sexp::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn items(&self) -> &[Sexp] {
        match self {
            Sexp::List(xs) => xs,
            _ => &[],
        }
    }

    /// Lists in it headed `name`.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Sexp> + 'a {
        self.items().iter().filter(move |x| x.head() == Some(name))
    }

    pub fn get(&self, name: &str) -> Option<&Sexp> {
        self.items().iter().find(|x| x.head() == Some(name))
    }

    /// The `k`th item, as text: a word's or a string's.
    pub fn text(&self, k: usize) -> Option<&str> {
        match self.items().get(k) {
            Some(Sexp::Atom(a)) | Some(Sexp::Str(a)) => Some(a),
            _ => None,
        }
    }

    pub fn num(&self, k: usize) -> Option<f64> {
        self.text(k).and_then(|t| t.parse().ok())
    }
}

/// An S-expression read; where it does not read, why.
pub fn parse(text: &str) -> Result<Sexp, String> {
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut stack: Vec<Vec<Sexp>> = Vec::new();
    let mut done: Option<Sexp> = None;
    let line = |i: usize| cs[..i.min(cs.len())].iter().filter(|c| **c == '\n').count() + 1;
    while i < cs.len() {
        let c = cs[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                stack.push(Vec::new());
                i += 1;
            }
            ')' => {
                let list = stack.pop().ok_or_else(|| format!("line {}: a `)` with no `(`", line(i)))?;
                let x = Sexp::List(list);
                match stack.last_mut() {
                    Some(top) => top.push(x),
                    None if done.is_none() => done = Some(x),
                    None => return Err(format!("line {}: more after the expression", line(i))),
                }
                i += 1;
            }
            '"' => {
                let start = i;
                let mut s = String::new();
                i += 1;
                loop {
                    match cs.get(i) {
                        None => return Err(format!("line {}: a string is not closed", line(start))),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('\\') => {
                            match cs.get(i + 1) {
                                Some('n') => s.push('\n'),
                                Some(&x) => s.push(x),
                                None => return Err(format!("line {}: a string is not closed", line(start))),
                            }
                            i += 2;
                        }
                        Some(&x) => {
                            s.push(x);
                            i += 1;
                        }
                    }
                }
                stack.last_mut().ok_or_else(|| format!("line {}: a string outside a list", line(start)))?.push(Sexp::Str(s));
            }
            _ => {
                let start = i;
                while i < cs.len() && !cs[i].is_whitespace() && cs[i] != '(' && cs[i] != ')' && cs[i] != '"' {
                    i += 1;
                }
                let w: String = cs[start..i].iter().collect();
                stack.last_mut().ok_or_else(|| format!("line {}: `{w}` outside a list", line(start)))?.push(Sexp::Atom(w));
            }
        }
    }
    if !stack.is_empty() {
        return Err("a `(` is not closed".into());
    }
    done.ok_or_else(|| "nothing to read".into())
}

fn kind_of(t: &str) -> PinKind {
    match t {
        "input" => PinKind::Input,
        "output" => PinKind::Output,
        "bidirectional" => PinKind::Bidir,
        "tri_state" => PinKind::Tristate,
        "power_in" => PinKind::PowerIn,
        "power_out" => PinKind::PowerOut,
        "open_collector" => PinKind::OpenCollector,
        "open_emitter" => PinKind::OpenEmitter,
        "no_connect" => PinKind::Nc,
        _ => PinKind::Passive,
    }
}

/// A symbol's pins, from its units' `pin`s, and where each points from.
fn pins_of(sym: &Sexp) -> Vec<(Pin, f64, f64)> {
    let mut out = Vec::new();
    let mut take = |s: &Sexp| {
        for p in s.all("pin") {
            let number = p.get("number").and_then(|n| n.text(1)).unwrap_or("").to_string();
            let name = p.get("name").and_then(|n| n.text(1)).unwrap_or("~").to_string();
            let at = p.get("at");
            let (x, y) = (at.and_then(|a| a.num(1)).unwrap_or(0.0), at.and_then(|a| a.num(2)).unwrap_or(0.0));
            if !out.iter().any(|(q, _, _): &(Pin, f64, f64)| q.number == number) {
                out.push((Pin { number, name, kind: kind_of(p.text(1).unwrap_or("passive")) }, x, y));
            }
        }
    };
    take(sym);
    for unit in sym.all("symbol") {
        take(unit);
    }
    out
}

/// A `.kicad_sym` read: each symbol by its name.
pub fn symbols(text: &str, library: &str) -> Result<Vec<(String, Symbol)>, String> {
    let lib = parse(text)?;
    if lib.head() != Some("kicad_symbol_lib") {
        return Err("not a KiCad symbol library: it does not begin `(kicad_symbol_lib`".into());
    }
    let raw: Vec<&Sexp> = lib.all("symbol").collect();
    let mut out = Vec::new();
    for s in &raw {
        let name = s.text(1).unwrap_or("").to_string();
        let prop = |s: &Sexp, k: &str| s.all("property").find(|p| p.text(1) == Some(k)).and_then(|p| p.text(2)).map(str::to_string);
        // A symbol that extends another takes its pins.
        let base = match s.get("extends").and_then(|e| e.text(1)) {
            Some(parent) => raw.iter().find(|r| r.text(1) == Some(parent)).copied().ok_or_else(|| format!("{name} extends {parent}, which the library has not"))?,
            None => s,
        };
        let mut pins = pins_of(base);
        // Numbered as they are: 1, 2 … 10, then names.
        pins.sort_by(|a, b| match (a.0.number.parse::<u32>(), b.0.number.parse::<u32>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => a.0.number.cmp(&b.0.number),
        });
        // Each pin's side, and its height for its place down that side.
        let mut left: Vec<(usize, f64)> = Vec::new();
        let mut right: Vec<(usize, f64)> = Vec::new();
        for (i, (_, x, y)) in pins.iter().enumerate() {
            if *x <= 0.0 {
                left.push((i, *y));
            } else {
                right.push((i, *y));
            }
        }
        left.sort_by(|a, b| b.1.total_cmp(&a.1));
        right.sort_by(|a, b| b.1.total_cmp(&a.1));
        let reference = prop(s, "Reference").or_else(|| prop(base, "Reference")).unwrap_or_else(|| "U".into());
        let prefix: &'static str = match reference.trim_end_matches('?') {
            "R" => "R",
            "C" => "C",
            "L" => "L",
            "D" => "D",
            "Q" => "Q",
            "J" => "J",
            "F" => "F",
            "SW" => "SW",
            "Y" => "Y",
            _ => "U",
        };
        out.push((
            name.clone(),
            Symbol {
                kind: format!("{library}:{name}"),
                title: prop(s, "Description").filter(|d| !d.is_empty()).unwrap_or_else(|| name.clone()),
                prefix,
                pins: pins.into_iter().map(|p| p.0).collect(),
                body: Body::Box { left: left.into_iter().map(|x| x.0).collect(), right: right.into_iter().map(|x| x.0).collect() },
                source: "a KiCad library",
                caution: None,
            },
        ));
    }
    Ok(out)
}

/// A `.kicad_mod` read: its pads, courtyard and body.
pub fn footprint(text: &str, name: &str) -> Result<Footprint, String> {
    let f = parse(text)?;
    if !matches!(f.head(), Some("footprint") | Some("module")) {
        return Err("not a KiCad footprint: it does not begin `(footprint`".into());
    }
    let mut pads = Vec::new();
    let mut smd = true;
    for p in f.all("pad") {
        let number = p.text(1).unwrap_or("").to_string();
        let kind = p.text(2).unwrap_or("smd");
        if kind == "np_thru_hole" || number.is_empty() {
            continue;
        }
        let shape = match p.text(3).unwrap_or("rect") {
            "circle" => Shape::Round,
            "oval" => Shape::Oval,
            "roundrect" => Shape::RoundRect,
            _ => Shape::Rect,
        };
        let at = p.get("at").ok_or_else(|| format!("pad {number} has no place"))?;
        let size = p.get("size").ok_or_else(|| format!("pad {number} has no size"))?;
        let (mut sx, mut sy) = (size.num(1).unwrap_or(0.0), size.num(2).unwrap_or(0.0));
        // Turned a quarter: its sides swapped.
        if at.num(3).is_some_and(|r| (r.abs() - 90.0).abs() < 1e-9 || (r.abs() - 270.0).abs() < 1e-9) {
            std::mem::swap(&mut sx, &mut sy);
        }
        let drill = p.get("drill").and_then(|d| d.num(1).or_else(|| d.num(2)));
        if drill.is_some() {
            smd = false;
        }
        pads.push(Pad { number, at: [at.num(1).unwrap_or(0.0), -at.num(2).unwrap_or(0.0)], size: [sx, sy], shape, drill });
    }
    // A layer's drawing's box: its lines', rectangles' and circles' ends.
    let boxed = |layer: &str| -> Option<[f64; 4]> {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for g in f.items().iter().filter(|g| matches!(g.head(), Some("fp_line") | Some("fp_rect") | Some("fp_poly") | Some("fp_circle"))) {
            if g.get("layer").and_then(|l| l.text(1)) != Some(layer) {
                continue;
            }
            let mut pts: Vec<(f64, f64)> = Vec::new();
            for k in ["start", "end", "center"] {
                if let Some(q) = g.get(k) {
                    pts.push((q.num(1).unwrap_or(0.0), q.num(2).unwrap_or(0.0)));
                }
            }
            if let Some(ps) = g.get("pts") {
                for xy in ps.all("xy") {
                    pts.push((xy.num(1).unwrap_or(0.0), xy.num(2).unwrap_or(0.0)));
                }
            }
            for (x, y) in pts {
                b = [b[0].min(x), b[1].min(-y), b[2].max(x), b[3].max(-y)];
            }
        }
        b[0].is_finite().then_some(b)
    };
    let fab = boxed("F.Fab");
    let body = fab.map_or([1.0, 1.0], |b| [b[2] - b[0], b[3] - b[1]]);
    let mut fp = crate::footprint::Footprint { name: name.to_string(), pads, body, courtyard: [0.0; 4], smd, source: "a KiCad library".into(), caution: None };
    fp.courtyard = boxed("F.CrtYd").unwrap_or_else(|| {
        let (mut lo, mut hi) = ([-body[0] / 2.0, -body[1] / 2.0], [body[0] / 2.0, body[1] / 2.0]);
        for p in &fp.pads {
            lo = [lo[0].min(p.at[0] - p.size[0] / 2.0), lo[1].min(p.at[1] - p.size[1] / 2.0)];
            hi = [hi[0].max(p.at[0] + p.size[0] / 2.0), hi[1].max(p.at[1] + p.size[1] / 2.0)];
        }
        [lo[0] - 0.25, lo[1] - 0.25, hi[0] + 0.25, hi[1] + 0.25]
    });
    Ok(fp)
}

/// Where libraries are looked for: the project's own `.kicad_sym` files
/// and `.pretty` folders, and KiCad's where it is installed.
#[derive(Debug, Clone, Default)]
pub struct Libraries {
    /// A symbol library by its nickname: its file's stem.
    pub symbols: Vec<(String, PathBuf)>,
    /// A footprint library by its nickname: its folder's stem.
    pub footprints: Vec<(String, PathBuf)>,
}

/// Where KiCad keeps its libraries, on the systems it is installed on.
pub const KICAD_HOMES: [&str; 4] = [
    "/Applications/KiCad/KiCad.app/Contents/SharedSupport",
    "/usr/share/kicad",
    "/usr/local/share/kicad",
    "C:/Program Files/KiCad/share/kicad",
];

impl Libraries {
    /// The project's libraries, three folders deep, then KiCad's.
    pub fn find(root: &Path) -> Libraries {
        let mut l = Libraries::default();
        let mut stack = vec![(root.to_path_buf(), 0u32)];
        let mut looked = 0;
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                looked += 1;
                if looked > 5000 {
                    break;
                }
                let p = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || ["node_modules", "target", "build", "dist"].contains(&name.as_str()) {
                    continue;
                }
                l.take(&p);
                if p.is_dir() && depth < 3 && p.extension().and_then(|x| x.to_str()) != Some("pretty") {
                    stack.push((p, depth + 1));
                }
            }
        }
        for home in KICAD_HOMES {
            for (sub, _) in [("symbols", 0), ("footprints", 0)] {
                if let Ok(entries) = std::fs::read_dir(Path::new(home).join(sub)) {
                    for e in entries.flatten() {
                        l.take(&e.path());
                    }
                }
            }
        }
        l
    }

    /// A file or folder, if it is a library.
    fn take(&mut self, p: &Path) {
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match p.extension().and_then(|x| x.to_str()) {
            Some("kicad_sym") if p.is_file() && !self.symbols.iter().any(|s| s.0 == stem) => self.symbols.push((stem, p.to_path_buf())),
            Some("pretty") if p.is_dir() && !self.footprints.iter().any(|s| s.0 == stem) => self.footprints.push((stem, p.to_path_buf())),
            _ => {}
        }
    }

    /// A symbol by `Library:Name`.
    pub fn symbol(&self, full: &str) -> Result<Symbol, String> {
        let (lib, name) = full.split_once(':').ok_or_else(|| format!("`{full}`: a KiCad symbol is `Library:Name`"))?;
        let path = &self.symbols.iter().find(|s| s.0 == lib).ok_or_else(|| format!("no symbol library {lib} in the project, nor KiCad installed with one"))?.1;
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        symbols(&text, lib)?.into_iter().find(|s| s.0 == name).map(|s| s.1).ok_or_else(|| format!("{lib} has no symbol {name}"))
    }

    /// A footprint by `Library:Name`.
    pub fn footprint(&self, full: &str) -> Result<Footprint, String> {
        let (lib, name) = full.split_once(':').ok_or_else(|| format!("`{full}`: a KiCad footprint is `Library:Name`"))?;
        let dir = &self.footprints.iter().find(|s| s.0 == lib).ok_or_else(|| format!("no footprint library {lib} in the project, nor KiCad installed with one"))?.1;
        let path = dir.join(format!("{name}.kicad_mod"));
        let text = std::fs::read_to_string(&path).map_err(|_| format!("{lib} has no footprint {name}"))?;
        footprint(&text, full)
    }
}
