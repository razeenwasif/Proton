//! Schematics in CAD (docs/CAD.md, E0): a `.schematic` read, built and
//! checked by `proton_board`, drawn on its sheet, written beside it as SVG,
//! PDF, a KiCad netlist and a bill of materials.

use std::path::Path;

use proton_board::build::{Built, Level};
use proton_board::lang::{self, Item, Schematic};
use proton_board::symbol::kinds;
use proton_cad::drawing::out;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cad::{CadDiag, CadOp};
use crate::cad_drawing::{CadDrawingItem, CadSheetBox};

/// A schematic's file ends so.
pub const EXTENSION: &str = proton_board::EXTENSION;

/// A pin of a part, and the net it is on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadPinRow {
    pub number: String,
    pub name: String,
    pub kind: String,
    pub net: Option<String>,
    /// Marked to be left unconnected.
    pub nc: bool,
}

/// A part of the schematic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadSchematicPart {
    pub index: u32,
    pub line: u32,
    pub reference: String,
    pub kind: String,
    pub title: String,
    pub value: Option<String>,
    pub footprint: Option<String>,
    /// Where it is on the sheet, and whether the file says so.
    pub at: Vec<f64>,
    pub placed: bool,
    pub pins: Vec<CadPinRow>,
    pub source: String,
    pub caution: Option<String>,
    /// Its footprint drawn, millimetres, as SVG; how many pads; and where
    /// its lands are from, and any caution on them.
    pub land_svg: Option<String>,
    pub pads: u32,
    pub land_source: Option<String>,
    pub land_caution: Option<String>,
}

/// A net: its pins, what drives it, what it carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadSchematicNet {
    pub index: u32,
    pub line: u32,
    pub name: String,
    pub power: bool,
    pub current: Option<f64>,
    pub pins: Vec<String>,
}

/// What the rules check found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadFinding {
    pub line: u32,
    pub error: bool,
    pub message: String,
}

/// A kind of part the library makes, and what its line takes after it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadSymbolKind {
    pub kind: String,
    pub title: String,
    pub takes: String,
}

/// The schematic open: its sheet drawn, its parts and nets, what the rules
/// check found, and its text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadSchematicView {
    pub path: String,
    pub name: String,
    pub generation: u32,
    pub title: Option<String>,
    pub sheet: String,
    pub width: f64,
    pub height: f64,
    pub svg: String,
    pub boxes: Vec<CadSheetBox>,
    /// Every statement but blanks and comments, in order.
    pub items: Vec<CadDrawingItem>,
    pub parts: Vec<CadSchematicPart>,
    pub nets: Vec<CadSchematicNet>,
    pub findings: Vec<CadFinding>,
    pub kinds: Vec<CadSymbolKind>,
    pub text: String,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Where the text stops reading, if it does: the rest is what last read.
    pub error: Option<CadDiag>,
}

/// The schematic as the window shows it.
pub fn view(s: &Schematic, b: &Built, path: &str, name: &str, generation: u32) -> CadSchematicView {
    let sheet = proton_board::sheet::draw(b, path);
    // A statement's error; its warnings are the window's to find by line.
    let first = |line: usize| b.findings.iter().find(|f| f.line == line && f.level == Level::Error).map(|f| f.message.clone());
    let items = s
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let (kind, name) = match &l.item {
                Item::Part(p) => ("part", p.reference.clone()),
                Item::Net(n) => ("net", n.name.clone()),
                Item::Nc(_) => ("nc", "unconnected".to_string()),
                _ => return None,
            };
            Some(CadDrawingItem { index: i as u32, line: l.line as u32, kind: kind.into(), name, text: lang::statement(&l.item), error: first(l.line) })
        })
        .collect();
    let parts = b
        .parts
        .iter()
        .enumerate()
        .map(|(k, p)| CadSchematicPart {
            index: p.index as u32,
            line: p.line as u32,
            reference: p.reference.clone(),
            kind: p.symbol.kind.clone(),
            title: p.symbol.title.clone(),
            value: p.value.clone(),
            footprint: p.footprint.clone(),
            at: p.at.to_vec(),
            placed: !p.placed,
            pins: p
                .symbol
                .pins
                .iter()
                .enumerate()
                .map(|(i, pin)| CadPinRow { number: pin.number.clone(), name: pin.name.clone(), kind: pin.kind.word().into(), net: b.net_of(k, i).map(|n| n.name.clone()), nc: b.nc.contains(&(k, i)) })
                .collect(),
            source: p.symbol.source.into(),
            caution: p.symbol.caution.map(str::to_string),
            land_svg: p.land.as_ref().map(|f| out::svg(&proton_board::footprint::draw(f))),
            pads: p.land.as_ref().map_or(0, |f| f.pads.len() as u32),
            land_source: p.land.as_ref().map(|f| f.source.clone()),
            land_caution: p.land.as_ref().and_then(|f| f.caution.clone()),
        })
        .collect();
    let nets = b
        .nets
        .iter()
        .map(|n| CadSchematicNet {
            index: n.index as u32,
            line: n.line as u32,
            name: n.name.clone(),
            power: n.power,
            current: n.current,
            pins: n.pins.iter().map(|&(k, i)| format!("{}.{}", b.parts[k].reference, b.parts[k].symbol.pins[i].number)).collect(),
        })
        .collect();
    CadSchematicView {
        path: path.into(),
        name: name.into(),
        generation,
        title: b.title.clone(),
        sheet: b.sheet.clone(),
        width: sheet.width,
        height: sheet.height,
        svg: out::svg(&sheet),
        boxes: sheet.boxes.iter().map(|(n, x)| CadSheetBox { name: n.clone(), x0: x[0], y0: x[1], x1: x[2], y1: x[3] }).collect(),
        items,
        parts,
        nets,
        findings: b.findings.iter().map(|f| CadFinding { line: f.line as u32, error: f.level == Level::Error, message: f.message.clone() }).collect(),
        kinds: kinds().into_iter().map(|(k, t, a)| CadSymbolKind { kind: k.into(), title: t.into(), takes: a.into() }).collect(),
        text: String::new(),
        dirty: false,
        can_undo: false,
        can_redo: false,
        error: None,
    }
}

/// The schematic written beside its file: `svg` or `pdf` its sheet, `net`
/// its netlist for KiCad, `csv` its bill of materials, `kicad` its parts'
/// footprints as a KiCad library, `NAME.pretty`.
pub fn export(b: &Built, file: &Path, shown: &str, format: &str) -> Result<String, String> {
    if format == "kicad" {
        let dir = file.with_extension("pretty");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut written: Vec<String> = Vec::new();
        for f in b.parts.iter().filter_map(|p| p.land.as_ref()) {
            if written.contains(&f.name) {
                continue;
            }
            let to = dir.join(format!("{}.kicad_mod", f.name));
            std::fs::write(&to, proton_board::footprint::kicad_mod(f)).map_err(|e| format!("{}: {e}", to.display()))?;
            written.push(f.name.clone());
        }
        if written.is_empty() {
            return Err("no part has a footprint yet: give one, `footprint 0603`".into());
        }
        let shown = Path::new(shown).with_extension("pretty").to_string_lossy().replace('\\', "/");
        return Ok(format!("{shown} ({} footprint{})", written.len(), if written.len() == 1 { "" } else { "s" }));
    }
    let (bytes, ext): (Vec<u8>, &str) = match format {
        "svg" => (out::svg(&proton_board::sheet::draw(b, shown)).into_bytes(), "svg"),
        "pdf" => (out::pdf(&proton_board::sheet::draw(b, shown)), "pdf"),
        "net" => (proton_board::netlist::kicad(b, shown).into_bytes(), "net"),
        "csv" => (proton_board::netlist::bom(b).into_bytes(), "csv"),
        other => return Err(format!("a schematic is written as SVG, PDF, a netlist (net), a bill of materials (csv) or KiCad footprints (kicad), not `{other}`")),
    };
    let to = file.with_extension(ext);
    std::fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(Path::new(shown).with_extension(ext).to_string_lossy().replace('\\', "/"))
}

/// An edit: statements put in, replaced or taken out.
pub fn edit(s: &mut Schematic, op: CadOp) -> Result<(), String> {
    let one = |text: &str| -> Result<Vec<lang::Line>, String> {
        let got = lang::read(text).map_err(|e| format!("`{}` does not read: {}", text.trim(), e.message))?;
        Ok(got.items.into_iter().filter(|l| !matches!(l.item, Item::Blank)).collect())
    };
    match op {
        CadOp::Insert { at, text } => {
            let new = one(&text)?;
            let at = at.map_or(s.items.len(), |a| (a as usize).min(s.items.len()));
            s.items.splice(at..at, new);
        }
        CadOp::Replace { index, text } => {
            let new = one(&text)?;
            let i = index as usize;
            if i >= s.items.len() {
                return Err(format!("there is no item {index}"));
            }
            s.items.splice(i..=i, new);
        }
        CadOp::Remove { index } => {
            if (index as usize) >= s.items.len() {
                return Err(format!("there is no item {index}"));
            }
            s.items.remove(index as usize);
        }
        _ => return Err("a schematic is edited a statement at a time".into()),
    }
    Ok(())
}

/// What the agent is told of a schematic: its parts, nets and findings.
pub fn said(b: &Built) -> String {
    let mut out = format!("A {} schematic{}: {} part{}, {} net{}.\n", b.sheet, b.title.as_ref().map_or(String::new(), |t| format!(", \"{t}\"")), b.parts.len(), if b.parts.len() == 1 { "" } else { "s" }, b.nets.len(), if b.nets.len() == 1 { "" } else { "s" });
    for p in &b.parts {
        out.push_str(&format!("- {} (line {}): {}{}{}\n", p.reference, p.line, p.symbol.title, p.value.as_ref().map_or(String::new(), |v| format!(", {v}")), p.footprint.as_ref().map_or(String::new(), |f| format!(", footprint {f}"))));
    }
    for n in &b.nets {
        let pins: Vec<String> = n.pins.iter().map(|&(k, i)| format!("{}.{} ({})", b.parts[k].reference, b.parts[k].symbol.pins[i].number, b.parts[k].symbol.pins[i].name)).collect();
        out.push_str(&format!("- net {}{}{}: {}\n", n.name, if n.power { ", powered from off the board" } else { "" }, n.current.map_or(String::new(), |a| format!(", {a} A")), pins.join(", ")));
    }
    if b.findings.is_empty() {
        out.push_str("The rules check finds nothing wrong.\n");
    } else {
        out.push_str("The rules check:\n");
        for f in &b.findings {
            out.push_str(&format!("- line {}, {}: {}\n", f.line, if f.level == Level::Error { "error" } else { "warning" }, f.message));
        }
    }
    out
}
