//! Drawings in CAD (docs/CAD.md, K10): a `.drawing` file read and laid out
//! on its sheet by `proton_cad::drawing`, shown as SVG, written beside it
//! as PDF, SVG and DXF; a part's drawing or an assembly's blueprint made
//! from what is open.

use std::path::Path;

use proton_cad::drawing::build::{blueprint, standard, Built};
use proton_cad::drawing::lang::{self, Drawing, Item};
use proton_cad::drawing::out;
use proton_cad::part::ast::Noted;
use proton_cad::vec::Aabb;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cad::{CadDiag, CadOp};

/// A drawing's file ends so.
pub const EXTENSION: &str = "drawing";

/// Where a view, a detail, a table or the title block is on the sheet,
/// millimetres from its bottom left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadSheetBox {
    pub name: String,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// A statement of the drawing, for its list: what it is and its line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadDrawingItem {
    pub index: u32,
    pub line: u32,
    /// `view`, `section`, `detail`, `dim`, `note`, `balloons` or `bom`.
    pub kind: String,
    pub name: String,
    pub text: String,
    /// What went wrong drawing it.
    pub error: Option<String>,
}

/// The drawing open: its text, its sheet drawn, and what is wrong.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CadDrawingView {
    pub path: String,
    pub name: String,
    pub generation: u32,
    pub title: Option<String>,
    /// The part or assembly it is of, as the drawing names it.
    pub of: Option<String>,
    pub sheet: String,
    pub width: f64,
    pub height: f64,
    pub blueprint: bool,
    /// The sheet as SVG, millimetres.
    pub svg: String,
    pub boxes: Vec<CadSheetBox>,
    pub items: Vec<CadDrawingItem>,
    /// The faces a dimension or note can name, and an assembly's parts.
    pub faces: Vec<String>,
    pub parts: Vec<String>,
    /// What could not be drawn, at its line.
    pub errors: Vec<CadDiag>,
    pub text: String,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Where the text stops reading, if it does: the sheet is then the last
    /// that did.
    pub error: Option<CadDiag>,
}

/// The drawing as the window shows it.
pub fn view(d: &Drawing, built: &Built, path: &str, name: &str, generation: u32) -> CadDrawingView {
    let (mut of, mut sheet, mut blue) = (None, "A3".to_string(), false);
    for n in &d.items {
        match &n.item {
            Item::Of(s) => of = Some(s.clone()),
            Item::Sheet(s) => sheet = s.clone(),
            Item::Blueprint(b) => blue = *b,
            _ => {}
        }
    }
    let items = d
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            let (kind, name) = match &n.item {
                Item::View(v) => ("view", v.name.clone()),
                Item::Section(s) => ("section", s.name.clone()),
                Item::Detail(x) => ("detail", x.name.clone()),
                Item::Dim(x) => ("dim", x.name.clone()),
                Item::Note(x) => ("note", x.name.clone()),
                Item::Balloons(v) => ("balloons", v.clone()),
                Item::Bom(_) => ("bom", "bom".to_string()),
                _ => return None,
            };
            let error = built.errors.iter().find(|e| e.line == n.line).map(|e| e.message.clone());
            Some(CadDrawingItem { index: i as u32, line: n.line as u32, kind: kind.into(), name, text: line_of(n), error })
        })
        .collect();
    CadDrawingView {
        path: path.into(),
        name: name.into(),
        generation,
        title: built.title.clone(),
        of,
        sheet,
        width: built.sheet.width,
        height: built.sheet.height,
        blueprint: blue,
        svg: out::svg(&built.sheet),
        boxes: built.sheet.boxes.iter().map(|(n, b)| CadSheetBox { name: n.clone(), x0: b[0], y0: b[1], x1: b[2], y1: b[3] }).collect(),
        items,
        faces: built.faces.clone(),
        parts: built.parts.clone(),
        errors: built.errors.iter().map(CadDiag::from).collect(),
        text: String::new(),
        dirty: false,
        can_undo: false,
        can_redo: false,
        error: None,
    }
}

/// An item's line as the language writes it.
fn line_of(n: &Noted<Item>) -> String {
    lang::write(&Drawing { items: vec![n.clone()] }).trim_end().to_string()
}

/// A part's drawing: its views at a scale they fit.
pub fn for_part(of: &str, title: &str, bounds: &Aabb, blue: bool) -> String {
    let text = standard(of, title, bounds);
    if blue {
        text.replacen("style paper", "style blueprint", 1)
    } else {
        text
    }
}

/// An assembly's blueprint.
pub fn for_assembly(of: &str, title: &str, m: &proton_cad::assembly::Model, blue: bool) -> String {
    blueprint(of, title, m, blue)
}

/// The sheet written beside the drawing, as `pdf`, `svg` or `dxf`: the
/// file's name as the project shows it.
pub fn export(built: &Built, file: &Path, shown: &str, format: &str) -> Result<String, String> {
    let (bytes, ext): (Vec<u8>, &str) = match format {
        "pdf" => (out::pdf(&built.sheet), "pdf"),
        "svg" => (out::svg(&built.sheet).into_bytes(), "svg"),
        "dxf" => (out::dxf(&built.sheet).into_bytes(), "dxf"),
        other => return Err(format!("a drawing is written as PDF, SVG or DXF, not `{other}`")),
    };
    std::fs::write(file.with_extension(ext), bytes).map_err(|e| format!("{}: {e}", file.with_extension(ext).display()))?;
    Ok(Path::new(shown).with_extension(ext).to_string_lossy().replace('\\', "/"))
}

/// An edit to the drawing: statements put in, replaced or taken out.
pub fn edit(d: &mut Drawing, op: CadOp) -> Result<(), String> {
    let one = |text: &str| -> Result<Vec<Noted<Item>>, String> {
        let got = lang::read(text).map_err(|e| format!("`{}` does not read: {}", text.trim(), e.message))?;
        Ok(got.items.into_iter().filter(|n| !matches!(n.item, Item::Blank)).collect())
    };
    match op {
        CadOp::Insert { at, text } => {
            let new = one(&text)?;
            let at = at.map_or(d.items.len(), |a| (a as usize).min(d.items.len()));
            d.items.splice(at..at, new);
        }
        CadOp::Replace { index, text } => {
            let new = one(&text)?;
            let i = index as usize;
            if i >= d.items.len() {
                return Err(format!("there is no item {index}"));
            }
            d.items.splice(i..=i, new);
        }
        CadOp::Remove { index } => {
            if (index as usize) >= d.items.len() {
                return Err(format!("there is no item {index}"));
            }
            d.items.remove(index as usize);
        }
        _ => return Err("a drawing is edited a statement at a time".into()),
    }
    Ok(())
}
