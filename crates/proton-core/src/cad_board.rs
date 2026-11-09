//! Boards in CAD (docs/CAD.md, E2): a `.board` read, built from its
//! schematic and checked by `proton_board`, drawn from the front; a first
//! board made from the schematic open.

use proton_board::board::{BoardBuilt, Core, Kind};
use proton_board::board_lang::{self, Board, Item};
use proton_board::build::Level;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cad::{CadDiag, CadOp};
use crate::cad_drawing::{CadDrawingItem, CadSheetBox};
use crate::cad_schematic::CadFinding;

/// A board's file ends so.
pub const EXTENSION: &str = proton_board::BOARD;

/// A part on the board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadBoardPart {
    pub reference: String,
    pub footprint: Option<String>,
    /// Where it is, mm on the board; and whether the file places it.
    pub at: Vec<f64>,
    pub placed: bool,
    pub turn: u32,
    pub back: bool,
}

/// A pad, for the track tool to start and end on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadBoardPad {
    /// `J1.2`.
    pub name: String,
    pub net: Option<String>,
    pub x: f64,
    pub y: f64,
    /// Through the board, or on which side.
    pub layer: String,
}

/// A net on the board: what it carries, the width that needs, what is left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadBoardNet {
    pub name: String,
    pub current: Option<f64>,
    /// IPC-2221's width for it, outside and inside, mm.
    pub needs: Option<Vec<f64>>,
    pub pads: u32,
    pub unrouted: u32,
}

/// The board open: drawn, its parts, pads and nets, its rules check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadBoardView {
    pub path: String,
    pub name: String,
    pub generation: u32,
    pub title: Option<String>,
    pub schematic: Option<String>,
    pub layers: u32,
    pub track: f64,
    pub clearance: f64,
    pub svg: String,
    /// The drawing's size, mm, and where the board's origin is on it.
    pub width: f64,
    pub height: f64,
    pub origin: Vec<f64>,
    pub boxes: Vec<CadSheetBox>,
    pub items: Vec<CadDrawingItem>,
    pub parts: Vec<CadBoardPart>,
    pub pads: Vec<CadBoardPad>,
    pub nets: Vec<CadBoardNet>,
    pub findings: Vec<CadFinding>,
    pub text: String,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub error: Option<CadDiag>,
}

/// The board as the window shows it.
pub fn view(b: &Board, built: &BoardBuilt, path: &str, name: &str, generation: u32) -> CadBoardView {
    let drawn = proton_board::board::svg(built);
    let first = |line: usize| built.findings.iter().find(|f| f.line == line && f.level == Level::Error).map(|f| f.message.clone());
    let items = b
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let (kind, name) = match &l.item {
                Item::Place(p) => ("place", p.reference.clone()),
                Item::Track(t) => ("track", t.net.clone()),
                Item::Via(v) => ("via", v.net.clone()),
                Item::Zone(z) => ("zone", z.net.clone()),
                Item::Hole { .. } => ("hole", "hole".to_string()),
                Item::Outline(_) => ("outline", "outline".to_string()),
                Item::Rules(_) => ("rules", "rules".to_string()),
                _ => return None,
            };
            Some(CadDrawingItem { index: i as u32, line: l.line as u32, kind: kind.into(), name, text: board_lang::statement(&l.item), error: first(l.line) })
        })
        .collect();
    let pads = built
        .copper_items
        .iter()
        .filter(|c| c.kind == Kind::Pad)
        .map(|c| {
            let p = match &c.core {
                Core::Pt(p) => *p,
                Core::Seg(a, q) => proton_cad::drawing::P2::new((a.x + q.x) / 2.0, (a.y + q.y) / 2.0),
                Core::Poly(ps) => proton_cad::drawing::P2::new(ps.iter().map(|q| q.x).sum::<f64>() / ps.len() as f64, ps.iter().map(|q| q.y).sum::<f64>() / ps.len() as f64),
            };
            let layer = if c.layers.len() > 1 { "through".to_string() } else { board_lang::LAYERS[c.layers[0]].to_string() };
            CadBoardPad { name: c.what.clone(), net: c.net.clone(), x: p.x, y: p.y, layer }
        })
        .collect();
    CadBoardView {
        path: path.into(),
        name: name.into(),
        generation,
        title: built.title.clone(),
        schematic: built.schematic.clone(),
        layers: built.layers,
        track: built.track,
        clearance: built.clearance,
        svg: drawn.svg,
        width: drawn.width,
        height: drawn.height,
        origin: drawn.origin.to_vec(),
        boxes: drawn.boxes.iter().map(|(n, x)| CadSheetBox { name: n.clone(), x0: x[0], y0: x[1], x1: x[2], y1: x[3] }).collect(),
        items,
        parts: built.parts.iter().map(|p| CadBoardPart { reference: p.reference.clone(), footprint: p.footprint.as_ref().map(|f| f.name.clone()), at: p.at.to_vec(), placed: p.placed, turn: p.turn, back: p.back }).collect(),
        pads,
        nets: built.nets.iter().map(|n| CadBoardNet { name: n.name.clone(), current: n.current, needs: n.needs.map(|(a, b)| vec![a, b]), pads: n.pads as u32, unrouted: n.unrouted as u32 }).collect(),
        findings: built.findings.iter().map(|f| CadFinding { line: f.line as u32, error: f.level == Level::Error, message: f.message.clone() }).collect(),
        text: String::new(),
        dirty: false,
        can_undo: false,
        can_redo: false,
        error: None,
    }
}

/// An edit: statements put in, replaced or taken out.
pub fn edit(b: &mut Board, op: CadOp) -> Result<(), String> {
    let one = |text: &str| -> Result<Vec<board_lang::Line>, String> {
        let got = board_lang::read(text).map_err(|e| format!("`{}` does not read: {}", text.trim(), e.message))?;
        Ok(got.items.into_iter().filter(|l| !matches!(l.item, Item::Blank)).collect())
    };
    match op {
        CadOp::Insert { at, text } => {
            let new = one(&text)?;
            let at = at.map_or(b.items.len(), |a| (a as usize).min(b.items.len()));
            b.items.splice(at..at, new);
        }
        CadOp::Replace { index, text } => {
            let new = one(&text)?;
            let i = index as usize;
            if i >= b.items.len() {
                return Err(format!("there is no item {index}"));
            }
            b.items.splice(i..=i, new);
        }
        CadOp::Remove { index } => {
            if (index as usize) >= b.items.len() {
                return Err(format!("there is no item {index}"));
            }
            b.items.remove(index as usize);
        }
        _ => return Err("a board is edited a statement at a time".into()),
    }
    Ok(())
}

/// The board written beside its file: `svg` its drawing from the front;
/// `gerber` every file a fab and an assembler take, into `NAME-fab/` —
/// refused while it breaks a design rule.
pub fn export(b: &BoardBuilt, sch: &proton_board::Built, file: &std::path::Path, shown: &str, format: &str) -> Result<String, String> {
    match format {
        "gerber" => {
            let errors: Vec<&str> = b.findings.iter().filter(|f| f.level == Level::Error).map(|f| f.message.as_str()).collect();
            if !errors.is_empty() {
                return Err(format!("the board breaks {} design rule{} — mend {} before it is made: {}", errors.len(), if errors.len() == 1 { "" } else { "s" }, if errors.len() == 1 { "it" } else { "them" }, errors.iter().take(3).copied().collect::<Vec<_>>().join("; ")));
            }
            let stem = file.file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| "board".into());
            let dir = file.with_file_name(format!("{stem}-fab"));
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let files = proton_board::gerber::fab(b, sch, &stem);
            for (name, text) in &files {
                std::fs::write(dir.join(name), text).map_err(|e| format!("{name}: {e}"))?;
            }
            let left: usize = b.nets.iter().map(|n| n.unrouted).sum();
            let shown_dir = std::path::Path::new(shown).with_file_name(format!("{stem}-fab")).to_string_lossy().replace('\\', "/");
            Ok(format!("{shown_dir}/ ({} files){}", files.len(), if left > 0 { format!(" — {left} connection{} not routed yet", if left == 1 { "" } else { "s" }) } else { String::new() }))
        }
        "part" => {
            // The board in 3D, as a part: opened, written as STEP, placed
            // in an assembly like any.
            let stem = file.file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| "board".into());
            let to = file.with_file_name(format!("{stem}-3d.part"));
            std::fs::write(&to, proton_board::three_d::to_part(b, b.title.as_deref().unwrap_or(&stem))).map_err(|e| format!("{}: {e}", to.display()))?;
            Ok(std::path::Path::new(shown).with_file_name(format!("{stem}-3d.part")).to_string_lossy().replace('\\', "/"))
        }
        "svg" => {
            let to = file.with_extension("svg");
            std::fs::write(&to, proton_board::board::svg(b).svg).map_err(|e| format!("{}: {e}", to.display()))?;
            Ok(std::path::Path::new(shown).with_extension("svg").to_string_lossy().replace('\\', "/"))
        }
        other => Err(format!("a board is written as Gerbers and drills (gerber), a 3D part (part) or SVG, not `{other}`")),
    }
}

/// What the agent is told of a board.
pub fn said(b: &BoardBuilt) -> String {
    // The board's own size: its outline's, not the parts waiting under it.
    let o = b.outline.first().map_or([0.0; 4], |ps| {
        ps.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |a, p| [a[0].min(p.x), a[1].min(p.y), a[2].max(p.x), a[3].max(p.y)])
    });
    let mut out = format!(
        "A {}-layer board, {} × {} mm, of {}: {} parts ({} placed). Rules: clearance {}, track {}, via {}/{}, edge {}, a {} °C rise on {} oz copper.\n",
        b.layers,
        proton_board::lang::num(((o[2] - o[0]) * 100.0).round() / 100.0),
        proton_board::lang::num(((o[3] - o[1]) * 100.0).round() / 100.0),
        b.schematic.as_deref().unwrap_or("no schematic"),
        b.parts.len(),
        b.parts.iter().filter(|p| p.placed).count(),
        b.clearance,
        b.track,
        b.via.0,
        b.via.1,
        b.edge,
        b.rise,
        b.copper
    );
    for p in &b.parts {
        out.push_str(&format!("- {} {} at ({}, {}){}{}\n", p.reference, p.footprint.as_ref().map_or("—", |f| f.name.as_str()), proton_board::lang::num(p.at[0]), proton_board::lang::num(p.at[1]), if p.turn != 0 { format!(" turned {}", p.turn) } else { String::new() }, if p.back { ", on the back" } else if !p.placed { ", not placed" } else { "" }));
    }
    for pad in b.copper_items.iter().filter(|c| c.kind == Kind::Pad) {
        if let Core::Pt(p) | Core::Seg(p, _) = &pad.core {
            out.push_str(&format!("  pad {} ({}) at ({}, {})\n", pad.what, pad.net.as_deref().unwrap_or("no net"), proton_board::lang::num((p.x * 1000.0).round() / 1000.0), proton_board::lang::num((p.y * 1000.0).round() / 1000.0)));
        } else if let Core::Poly(ps) = &pad.core {
            let (x, y) = (ps.iter().map(|q| q.x).sum::<f64>() / ps.len() as f64, ps.iter().map(|q| q.y).sum::<f64>() / ps.len() as f64);
            out.push_str(&format!("  pad {} ({}) at ({}, {})\n", pad.what, pad.net.as_deref().unwrap_or("no net"), proton_board::lang::num((x * 1000.0).round() / 1000.0), proton_board::lang::num((y * 1000.0).round() / 1000.0)));
        }
    }
    for n in &b.nets {
        out.push_str(&format!("- net {}: {} pads, {}{}\n", n.name, n.pads, if n.unrouted == 0 { "routed".to_string() } else { format!("{} connection{} to route", n.unrouted, if n.unrouted == 1 { "" } else { "s" }) }, n.needs.map_or(String::new(), |(a, i)| format!(", {} A wants {} mm outside, {} inside", n.current.unwrap_or(0.0), proton_board::lang::num((a * 100.0).ceil() / 100.0), proton_board::lang::num((i * 100.0).ceil() / 100.0)))));
    }
    if b.findings.is_empty() {
        out.push_str("The design rules find nothing wrong.\n");
    } else {
        out.push_str("The design rules:\n");
        for f in &b.findings {
            out.push_str(&format!("- line {}, {}: {}\n", f.line, if f.level == Level::Error { "error" } else { "warning" }, f.message));
        }
    }
    out
}
