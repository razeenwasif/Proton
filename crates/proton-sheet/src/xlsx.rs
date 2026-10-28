//! `.xlsx`, Excel's own files: read into a workbook and written from one.
//!
//! Read: sheets, shared and inline strings, numbers, true and false, errors,
//! formulas (shared ones expanded), number formats, bold, italic,
//! underline, colours, fills, alignment and wrapping, column widths, frozen
//! panes and defined names. Values cached for formulas are not read; the
//! engine works them out. What is not kept — charts, pictures, pivot tables,
//! macros, comments, merged cells — is listed, so it is said when the file
//! opens rather than lost without a word on saving.
//!
//! Written: the same, with each formula's value cached so another program
//! shows it before working anything out, and newer functions given the
//! `_xlfn.` prefix Excel looks for.

use std::collections::{BTreeMap, HashMap};

use crate::book::{literal, Book};
use crate::formula::{self, Expr};
use crate::model::{Cell, Frozen, Sheet, Style, Workbook};
use crate::refs::CellRef;
use crate::value::{ErrorKind, Value};
use crate::xml::{esc, events, tree, unesc_x, Ev, Node};
use crate::zip::{self, Archive};

/// A file read, and what it held that is not kept.
#[derive(Debug)]
pub struct Read {
    pub workbook: Workbook,
    pub dropped: Vec<String>,
}

/// The number formats Excel has without saying them, by number.
fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "$#,##0;($#,##0)",
        6 => "$#,##0;[Red]($#,##0)",
        7 => "$#,##0.00;($#,##0.00)",
        8 => "$#,##0.00;[Red]($#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mm:ss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// Functions Excel writes with `_xlfn.` before them, having come after 2007.
const FUTURE: &[&str] = &[
    "CONCAT", "TEXTJOIN", "IFS", "SWITCH", "XOR", "MAXIFS", "MINIFS", "XLOOKUP", "XMATCH", "STDEV.S", "STDEV.P", "VAR.S",
    "VAR.P", "MODE.SNGL", "RANK.EQ", "PERCENTILE.INC", "QUARTILE.INC", "CEILING.MATH", "FLOOR.MATH", "IFNA", "DAYS",
];

/// `#RRGGBB` from Excel's `FFRRGGBB`.
fn color_of(n: Option<&Node>) -> Option<String> {
    let rgb = n?.attr("rgb")?;
    let hex = if rgb.len() == 8 { &rgb[2..] } else { rgb };
    (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| format!("#{}", hex.to_ascii_lowercase()))
}

fn flag(n: &Node, name: &str) -> bool {
    n.child(name).is_some_and(|b| !matches!(b.attr("val"), Some("0" | "false" | "none")))
}

/// The styles a file's cells name, as ours: one for each of its cell
/// formats, in order.
fn read_styles(xml: &str) -> Result<Vec<Style>, String> {
    let root = tree(xml)?;
    let mut formats: HashMap<u32, String> = HashMap::new();
    if let Some(nf) = root.child("numFmts") {
        for f in nf.all("numFmt") {
            if let (Some(id), Some(code)) = (f.attr("numFmtId").and_then(|i| i.parse().ok()), f.attr("formatCode")) {
                formats.insert(id, code.to_string());
            }
        }
    }
    let fonts: Vec<&Node> = root.child("fonts").map(|f| f.all("font").collect()).unwrap_or_default();
    let fills: Vec<&Node> = root.child("fills").map(|f| f.all("fill").collect()).unwrap_or_default();
    let mut out = Vec::new();
    for xf in root.child("cellXfs").map(|x| x.all("xf").collect::<Vec<_>>()).unwrap_or_default() {
        let num = |k: &str| xf.attr(k).and_then(|v| v.parse::<usize>().ok());
        let mut s = Style::default();
        if let Some(id) = num("numFmtId") {
            s.format = formats.get(&(id as u32)).cloned().or_else(|| builtin_format(id as u32).map(str::to_string)).unwrap_or_default();
            if s.format.eq_ignore_ascii_case("general") {
                s.format.clear();
            }
        }
        if let Some(font) = num("fontId").and_then(|i| fonts.get(i)) {
            s.bold = flag(font, "b");
            s.italic = flag(font, "i");
            s.underline = flag(font, "u");
            // Black is what text is anyway.
            s.color = color_of(font.child("color")).filter(|c| c != "#000000");
        }
        if let Some(fill) = num("fillId").and_then(|i| fills.get(i)) {
            if let Some(p) = fill.child("patternFill").filter(|p| p.attr("patternType") == Some("solid")) {
                s.fill = color_of(p.child("fgColor"));
            }
        }
        if let Some(a) = xf.child("alignment") {
            s.align = a.attr("horizontal").filter(|h| matches!(*h, "left" | "center" | "right")).map(str::to_string);
            s.wrap = matches!(a.attr("wrapText"), Some("1" | "true"));
        }
        out.push(s);
    }
    Ok(out)
}

fn read_strings(xml: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    // Text inside <t>, but not a phonetic reading's.
    let mut in_t = false;
    let mut in_rph = false;
    events(xml, |ev| {
        match ev {
            Ev::Start(n, _) if n == "si" => cur = Some(String::new()),
            Ev::Start(n, _) if n == "rPh" => in_rph = true,
            Ev::End(n) if n == "rPh" => in_rph = false,
            Ev::Start(n, _) if n == "t" => in_t = true,
            Ev::End(n) if n == "t" => in_t = false,
            Ev::Text(t) if in_t && !in_rph => {
                if let Some(c) = cur.as_mut() {
                    c.push_str(&t);
                }
            }
            Ev::End(n) if n == "si" => out.push(unesc_x(&cur.take().unwrap_or_default())),
            _ => {}
        }
        Ok(())
    })?;
    Ok(out)
}

/// Text as a cell's input: a `'` before it when typing it would make it
/// something else — a number, a date, a formula.
fn text_input(t: &str) -> String {
    if t.starts_with(['=', '\'']) || !matches!(literal(t), Value::Text(_)) {
        format!("'{t}")
    } else {
        t.to_string()
    }
}

fn number_input(v: &str) -> Option<String> {
    let n: f64 = v.trim().parse().ok()?;
    n.is_finite().then(|| format!("{n}"))
}

/// A formula from a file as a cell's input: `_xlfn.` and its kin taken off.
fn formula_input(f: &str) -> String {
    let mut s = f.to_string();
    for p in ["_xlfn._xlws.", "_xlfn.", "_xlws.", "_xludf."] {
        s = s.replace(p, "");
    }
    format!("={s}")
}

struct SheetRead {
    cells: BTreeMap<CellRef, Cell>,
    widths: BTreeMap<u32, f64>,
    frozen: Frozen,
    merged: bool,
    conditional: bool,
}

fn read_sheet(xml: &str, strings: &[String], style_map: &[u32]) -> Result<SheetRead, String> {
    let mut out = SheetRead { cells: BTreeMap::new(), widths: BTreeMap::new(), frozen: Frozen::default(), merged: false, conditional: false };
    // The cell being read.
    let mut at: Option<CellRef> = None;
    let mut kind = String::new();
    let mut style = 0u32;
    let mut value = String::new();
    let mut formula = String::new();
    let mut shared: Option<(String, Option<String>)> = None; // (si, ref) of a shared formula
    let mut inline = String::new();
    let mut inside = "";
    let mut row: u32 = 0;
    let mut next_col: u32 = 0;
    // Shared formulas by their number: where the first is and what it says.
    let mut masters: HashMap<String, (CellRef, String)> = HashMap::new();
    events(xml, |ev| {
        match ev {
            Ev::Start(n, attrs) => {
                let a = |k: &str| attrs.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
                match n.as_str() {
                    "row" => {
                        if let Some(r) = a("r").and_then(|r| r.parse::<u32>().ok()) {
                            row = r.saturating_sub(1);
                        }
                        next_col = 0;
                    }
                    "c" => {
                        let cell = a("r").and_then(CellRef::parse).unwrap_or(CellRef::new(row, next_col));
                        row = cell.row;
                        next_col = cell.col + 1;
                        at = Some(cell);
                        kind = a("t").unwrap_or("n").to_string();
                        style = a("s").and_then(|s| s.parse::<usize>().ok()).and_then(|s| style_map.get(s)).copied().unwrap_or(0);
                        value.clear();
                        formula.clear();
                        inline.clear();
                        shared = None;
                    }
                    "v" => inside = "v",
                    "f" => {
                        inside = "f";
                        if a("t") == Some("shared") {
                            shared = a("si").map(|si| (si.to_string(), a("ref").map(str::to_string)));
                        }
                    }
                    "t" if at.is_some() => inside = "t",
                    "pane" if a("state").is_some_and(|s| s.starts_with("frozen")) => {
                        let n = |k: &str| a(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as u32;
                        out.frozen = Frozen { rows: n("ySplit"), cols: n("xSplit") };
                    }
                    "col" => {
                        let n = |k: &str| a(k).and_then(|v| v.parse::<u32>().ok());
                        if let (Some(min), Some(max), Some(w)) = (n("min"), n("max"), a("width").and_then(|w| w.parse::<f64>().ok())) {
                            if a("customWidth").is_some_and(|c| c == "1" || c == "true") && max - min < 16_384 {
                                // Characters of the default font to points.
                                let points = ((w * 7.0 + 5.0) * 0.75 * 100.0).round() / 100.0;
                                for c in min.saturating_sub(1)..max {
                                    out.widths.insert(c, points);
                                }
                            }
                        }
                    }
                    "mergeCell" => out.merged = true,
                    "conditionalFormatting" => out.conditional = true,
                    _ => {}
                }
            }
            Ev::Text(t) => match inside {
                "v" => value.push_str(&t),
                "f" => formula.push_str(&t),
                "t" => inline.push_str(&t),
                _ => {}
            },
            Ev::End(n) => match n.as_str() {
                "v" | "f" | "t" => inside = "",
                "c" => {
                    let Some(cell) = at.take() else { return Ok(()) };
                    // A shared formula: the first says it, the rest are it
                    // moved as far as they are from the first.
                    let f = match &shared {
                        Some((si, _)) if !formula.is_empty() => {
                            masters.insert(si.clone(), (cell, formula.clone()));
                            formula.clone()
                        }
                        Some((si, _)) => match masters.get(si) {
                            Some((home, src)) => match formula::parse(src) {
                                Ok(e) => e.shifted(cell.row as i64 - home.row as i64, cell.col as i64 - home.col as i64).write(),
                                Err(_) => src.clone(),
                            },
                            None => String::new(),
                        },
                        None => formula.clone(),
                    };
                    let input = if !f.is_empty() {
                        formula_input(&f)
                    } else {
                        match kind.as_str() {
                            "s" => value.trim().parse::<usize>().ok().and_then(|i| strings.get(i)).map(|t| text_input(t)).unwrap_or_default(),
                            "inlineStr" | "str" => text_input(&unesc_x(&inline)),
                            "b" => if value.trim() == "1" { "TRUE" } else { "FALSE" }.to_string(),
                            "e" => value.trim().to_string(),
                            _ => number_input(&value).unwrap_or_default(),
                        }
                    };
                    if !input.is_empty() || style != 0 {
                        out.cells.insert(cell, Cell { input, style });
                    }
                }
                _ => {}
            },
        }
        Ok(())
    })?;
    Ok(out)
}

/// Where a relationship points, from the folder its part is in.
fn resolve_target(base: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in target.split('/') {
        match p {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// An `.xlsx` file's bytes read into a workbook.
pub fn read(bytes: &[u8]) -> Result<Read, String> {
    let zip = Archive::new(bytes)?;
    let mut dropped = Vec::new();
    let wb_xml = zip.text("xl/workbook.xml").map_err(|_| "not an Excel workbook: it has no xl/workbook.xml".to_string())?;
    let rels = tree(&zip.text("xl/_rels/workbook.xml.rels")?)?;
    let targets: HashMap<String, String> = rels
        .all("Relationship")
        .filter_map(|r| Some((r.attr("Id")?.to_string(), resolve_target("xl", r.attr("Target")?))))
        .collect();
    let strings = if zip.has("xl/sharedStrings.xml") { read_strings(&zip.text("xl/sharedStrings.xml")?)? } else { Vec::new() };
    let mut workbook = Workbook { sheets: Vec::new(), styles: vec![Style::default()], names: BTreeMap::new() };
    let file_styles = if zip.has("xl/styles.xml") { read_styles(&zip.text("xl/styles.xml")?)? } else { Vec::new() };
    let style_map: Vec<u32> = file_styles.iter().map(|s| workbook.style_id(s)).collect();

    let root = tree(&wb_xml)?;
    let sheets: Vec<&Node> = root.child("sheets").map(|s| s.all("sheet").collect()).unwrap_or_default();
    let mut names_by_index = Vec::new();
    for (i, s) in sheets.iter().enumerate() {
        let name = s.attr("name").unwrap_or("Sheet").to_string();
        names_by_index.push(name.clone());
        let Some(path) = s.attr("id").and_then(|id| targets.get(id)) else {
            dropped.push(format!("“{name}”, which is not a worksheet"));
            continue;
        };
        if !path.contains("worksheets/") {
            dropped.push(format!("“{name}”, a chart sheet"));
            continue;
        }
        let read = read_sheet(&zip.text(path)?, &strings, &style_map).map_err(|e| format!("{path}: {e}"))?;
        if read.merged {
            dropped.push(format!("merged cells on “{name}”"));
        }
        if read.conditional {
            dropped.push(format!("conditional formats on “{name}”"));
        }
        let mut sheet = Sheet::new(&format!("s{}", i + 1), &name);
        sheet.charts = read_charts(&zip, path);
        sheet.cells = read.cells;
        sheet.widths = read.widths;
        sheet.frozen = read.frozen;
        workbook.sheets.push(sheet);
    }
    if workbook.sheets.is_empty() {
        return Err("the workbook has no worksheets".into());
    }
    if let Some(dn) = root.child("definedNames") {
        for d in dn.all("definedName") {
            let Some(name) = d.attr("name") else { continue };
            // Excel's own (`_xlnm.Print_Area`) and hidden names are its business.
            if name.starts_with("_xlnm.") || d.attr("hidden") == Some("1") {
                continue;
            }
            let mut def = formula_input(&d.text)[1..].to_string();
            // A name for one sheet only is kept as the workbook's, said.
            if let Some(i) = d.attr("localSheetId").and_then(|i| i.parse::<usize>().ok()) {
                dropped.push(format!("the name {name} being only {}'s", names_by_index.get(i).map_or("one sheet", |s| s.as_str())));
            }
            def = def.trim().to_string();
            workbook.names.entry(name.to_string()).or_insert(def);
        }
    }
    let mut seen = |what: &str| {
        if !dropped.iter().any(|d| d == what) {
            dropped.push(what.to_string());
        }
    };
    for n in zip.names() {
        if n.starts_with("xl/charts/chart") && n.ends_with(".xml") && !workbook.sheets.iter().any(|s| !s.charts.is_empty()) {
            seen("charts");
        } else if n.starts_with("xl/media/") {
            seen("pictures");
        } else if n.starts_with("xl/pivotTables/") || n.starts_with("xl/pivotCache/") {
            seen("pivot tables");
        } else if n == "xl/vbaProject.bin" {
            seen("macros");
        } else if n.starts_with("xl/comments") || n.starts_with("xl/threadedComments/") {
            seen("comments");
        } else if n.starts_with("xl/tables/") {
            seen("tables' formatting");
        }
    }
    Ok(Read { workbook, dropped })
}

/// A formula written for the file: no `=`, newer functions prefixed.
fn file_formula(input: &str) -> String {
    let src = &input[1..];
    fn prefix(e: &Expr) -> Expr {
        match e {
            Expr::Call(name, args) => {
                let args = args.iter().map(prefix).collect();
                let up = name.to_ascii_uppercase();
                Expr::Call(if FUTURE.contains(&up.as_str()) { format!("_xlfn.{up}") } else { name.clone() }, args)
            }
            Expr::Neg(a) => Expr::Neg(Box::new(prefix(a))),
            Expr::Plus(a) => Expr::Plus(Box::new(prefix(a))),
            Expr::Percent(a) => Expr::Percent(Box::new(prefix(a))),
            Expr::Binary(op, a, b) => Expr::Binary(*op, Box::new(prefix(a)), Box::new(prefix(b))),
            Expr::RangeOp(a, b) => Expr::RangeOp(Box::new(prefix(a)), Box::new(prefix(b))),
            Expr::Array(rows) => Expr::Array(rows.iter().map(|r| r.iter().map(prefix).collect()).collect()),
            other => other.clone(),
        }
    }
    match formula::parse(src) {
        Ok(e) if FUTURE.iter().any(|f| src.to_ascii_uppercase().contains(f)) => prefix(&e).write(),
        _ => src.to_string(),
    }
}

/// A range as a chart's reference: the sheet's name, `$`s.
fn absolute(sheet: &str, r: crate::refs::Range) -> String {
    let at = |c: CellRef| format!("${}${}", crate::refs::col_name(c.col), c.row + 1);
    let body = if r.start == r.end { at(r.start) } else { format!("{}:{}", at(r.start), at(r.end)) };
    format!("{}{body}", formula::sheet_prefix(sheet))
}

/// A chart's part: its kind, title and series, each series' references
/// into the sheet and the values they had, so a program shows it before
/// working anything out.
fn chart_xml(book: &Book, si: usize, chart: &crate::model::Chart) -> String {
    use crate::model::ChartKind;
    let name = &book.workbook().sheets[si].name;
    let mut series = String::new();
    if let Some((r, header, labelled)) = crate::arrange::chart_shape(book, si, &chart.range) {
        let first = r.start.row + u32::from(header);
        let span = |c: u32| crate::refs::Range::new(CellRef::new(first, c), CellRef::new(r.end.row.max(first), c));
        let rows = (first..=r.end.row.max(first)).collect::<Vec<u32>>();
        let cache_num = |c: u32| {
            let pts: String = rows
                .iter()
                .enumerate()
                .filter_map(|(i, row)| match book.value(si, CellRef::new(*row, c)) {
                    Value::Number(n) => Some(format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", number_text(n))),
                    _ => None,
                })
                .collect();
            format!("<c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>{pts}</c:numCache>", rows.len())
        };
        let cache_str = |c: u32| {
            let pts: String = rows.iter().enumerate().map(|(i, row)| format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(&crate::arrange::shown(book, si, CellRef::new(*row, c))))).collect();
            format!("<c:strCache><c:ptCount val=\"{}\"/>{pts}</c:strCache>", rows.len())
        };
        let value_cols: Vec<u32> = (r.start.col + u32::from(labelled)..=r.end.col).collect();
        for (i, c) in value_cols.iter().enumerate() {
            let tx = if header {
                let at = CellRef::new(r.start.row, *c);
                format!("<c:tx><c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{}</c:v></c:pt></c:strCache></c:strRef></c:tx>", esc(&absolute(name, crate::refs::Range::cell(at))), esc(&crate::arrange::shown(book, si, at)))
            } else {
                String::new()
            };
            let (cat_tag, val_tag) = if chart.kind == ChartKind::Scatter { ("c:xVal", "c:yVal") } else { ("c:cat", "c:val") };
            let cat = if labelled {
                let f = esc(&absolute(name, span(r.start.col)));
                if chart.kind == ChartKind::Scatter {
                    format!("<{cat_tag}><c:numRef><c:f>{f}</c:f>{}</c:numRef></{cat_tag}>", cache_num(r.start.col))
                } else {
                    format!("<{cat_tag}><c:strRef><c:f>{f}</c:f>{}</c:strRef></{cat_tag}>", cache_str(r.start.col))
                }
            } else {
                String::new()
            };
            let val = format!("<{val_tag}><c:numRef><c:f>{}</c:f>{}</c:numRef></{val_tag}>", esc(&absolute(name, span(*c))), cache_num(*c));
            let marker = if chart.kind == ChartKind::Line { "<c:marker><c:symbol val=\"none\"/></c:marker>" } else { "" };
            series.push_str(&format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/>{tx}{marker}{cat}{val}</c:ser>"));
        }
    }
    let axes_ids = "<c:axId val=\"500000001\"/><c:axId val=\"500000002\"/>";
    let axis = |tag: &str, id: u32, cross: u32, pos: &str| {
        format!("<c:{tag}><c:axId val=\"{id}\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{pos}\"/><c:crossAx val=\"{cross}\"/></c:{tag}>")
    };
    let plot = match chart.kind {
        ChartKind::Bar => format!("<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>{series}{axes_ids}</c:barChart>{}{}", axis("catAx", 500000001, 500000002, "b"), axis("valAx", 500000002, 500000001, "l")),
        ChartKind::Line => format!("<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>{series}<c:marker val=\"1\"/>{axes_ids}</c:lineChart>{}{}", axis("catAx", 500000001, 500000002, "b"), axis("valAx", 500000002, 500000001, "l")),
        ChartKind::Scatter => format!("<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{series}{axes_ids}</c:scatterChart>{}{}", axis("valAx", 500000001, 500000002, "b"), axis("valAx", 500000002, 500000001, "l")),
        ChartKind::Pie => format!("<c:pieChart><c:varyColors val=\"1\"/>{series}<c:firstSliceAng val=\"0\"/></c:pieChart>"),
    };
    let title = if chart.title.is_empty() {
        "<c:autoTitleDeleted val=\"1\"/>".to_string()
    } else {
        format!("<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>", esc(&chart.title))
    };
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:chart>{title}<c:plotArea><c:layout/>{plot}</c:plotArea><c:legend><c:legendPos val=\"r\"/><c:overlay val=\"0\"/></c:legend><c:plotVisOnly val=\"1\"/></c:chart></c:chartSpace>")
}

/// A sheet's charts read from its drawing: where each is, its kind, its
/// title, and the range its series' references cover.
fn read_charts(zip: &Archive, sheet_path: &str) -> Vec<crate::model::Chart> {
    use crate::model::{Chart, ChartKind};
    let (dir, file) = sheet_path.rsplit_once('/').unwrap_or(("", sheet_path));
    let rels_of = |dir: &str, file: &str| -> HashMap<String, String> {
        zip.text(&format!("{dir}/_rels/{file}.rels"))
            .ok()
            .and_then(|t| tree(&t).ok())
            .map(|r| r.all("Relationship").filter_map(|x| Some((x.attr("Id")?.to_string(), resolve_target(dir, x.attr("Target")?)))).collect())
            .unwrap_or_default()
    };
    let sheet_rels = rels_of(dir, file);
    let mut out = Vec::new();
    for target in sheet_rels.values().filter(|t| t.contains("/drawings/")) {
        let Ok(drawing) = zip.text(target).map_err(|_| ()).and_then(|t| tree(&t).map_err(|_| ())) else { continue };
        let (ddir, dfile) = target.rsplit_once('/').unwrap_or(("", target));
        let drels = rels_of(ddir, dfile);
        for anchor in drawing.children.iter().filter(|a| a.name == "twoCellAnchor" || a.name == "oneCellAnchor") {
            let cell = |n: Option<&crate::xml::Node>| -> (u32, u32, u64, u64) {
                let v = |k: &str| n.and_then(|n| n.child(k)).and_then(|x| x.text.trim().parse::<u64>().ok()).unwrap_or(0);
                (v("col") as u32, v("row") as u32, v("colOff"), v("rowOff"))
            };
            let (c0, r0, _, _) = cell(anchor.child("from"));
            let (c1, r1, co, ro) = cell(anchor.child("to"));
            // The chart the frame shows, wherever in the frame it is.
            fn find<'a>(n: &'a crate::xml::Node, name: &str) -> Option<&'a crate::xml::Node> {
                if n.name == name {
                    return Some(n);
                }
                n.children.iter().find_map(|c| find(c, name))
            }
            let Some(id) = find(anchor, "chart").and_then(|c| c.attr("id")) else { continue };
            let Some(Ok(xml)) = drels.get(id).map(|p| zip.text(p)) else { continue };
            let Ok(space) = tree(&xml) else { continue };
            let Some(plot) = find(&space, "plotArea") else { continue };
            let Some(kind_node) = plot.children.iter().find(|c| c.name.ends_with("Chart")) else { continue };
            let kind = match kind_node.name.as_str() {
                "barChart" | "bar3DChart" => ChartKind::Bar,
                "scatterChart" => ChartKind::Scatter,
                "pieChart" | "pie3DChart" | "doughnutChart" => ChartKind::Pie,
                _ => ChartKind::Line,
            };
            // Every reference in the series: the range is what they cover.
            let mut refs = Vec::new();
            fn formulas(n: &crate::xml::Node, out: &mut Vec<String>) {
                if n.name == "f" {
                    out.push(n.text.clone());
                }
                for c in &n.children {
                    formulas(c, out);
                }
            }
            for ser in kind_node.all("ser") {
                formulas(ser, &mut refs);
            }
            let mut span: Option<crate::refs::Range> = None;
            for f in &refs {
                let body = f.rsplit_once('!').map_or(f.as_str(), |(_, b)| b).replace('$', "");
                let Some(r) = crate::refs::Range::parse(&body) else { continue };
                span = Some(match span {
                    None => r,
                    Some(s) => crate::refs::Range::new(
                        CellRef::new(s.start.row.min(r.start.row), s.start.col.min(r.start.col)),
                        CellRef::new(s.end.row.max(r.end.row), s.end.col.max(r.end.col)),
                    ),
                });
            }
            let Some(range) = span else { continue };
            let title = find(&space, "title").map(|t| {
                fn texts(n: &crate::xml::Node, out: &mut String) {
                    if n.name == "t" {
                        out.push_str(&n.text);
                    }
                    for c in &n.children {
                        texts(c, out);
                    }
                }
                let mut s = String::new();
                texts(t, &mut s);
                s
            });
            out.push(Chart {
                kind,
                range: range.name(),
                title: title.unwrap_or_default(),
                at: CellRef::new(r0, c0).name(),
                width: ((c1.saturating_sub(c0)) * 64 + (co / 9525) as u32).max(120),
                height: ((r1.saturating_sub(r0)) * 20 + (ro / 9525) as u32).max(80),
            });
        }
    }
    out
}

fn number_text(n: f64) -> String {
    format!("{n}")
}

/// The styles part: a font, a fill and a format for each style, and a
/// cell format joining them, numbered as the workbook's styles are.
fn write_styles(styles: &[Style]) -> String {
    let mut formats: Vec<(u32, String)> = Vec::new();
    let mut fonts: Vec<String> = vec!["<font><sz val=\"11\"/><name val=\"Calibri\"/></font>".into()];
    let mut fills: Vec<String> = vec!["<fill><patternFill patternType=\"none\"/></fill>".into(), "<fill><patternFill patternType=\"gray125\"/></fill>".into()];
    let mut xfs = Vec::new();
    for s in styles {
        let fmt_id = if s.format.is_empty() {
            0
        } else if let Some(id) = (0..=49).find(|i| builtin_format(*i) == Some(s.format.as_str())) {
            id
        } else if let Some((id, _)) = formats.iter().find(|(_, c)| *c == s.format) {
            *id
        } else {
            let id = 164 + formats.len() as u32;
            formats.push((id, s.format.clone()));
            id
        };
        let mut font = String::from("<font>");
        if s.bold {
            font.push_str("<b/>");
        }
        if s.italic {
            font.push_str("<i/>");
        }
        if s.underline {
            font.push_str("<u/>");
        }
        font.push_str("<sz val=\"11\"/>");
        if let Some(c) = &s.color {
            font.push_str(&format!("<color rgb=\"FF{}\"/>", c.trim_start_matches('#').to_ascii_uppercase()));
        }
        font.push_str("<name val=\"Calibri\"/></font>");
        let font_id = fonts.iter().position(|f| *f == font).unwrap_or_else(|| {
            fonts.push(font);
            fonts.len() - 1
        });
        let fill_id = match &s.fill {
            None => 0,
            Some(c) => {
                let rgb = c.trim_start_matches('#').to_ascii_uppercase();
                let fill = format!("<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{rgb}\"/><bgColor indexed=\"64\"/></patternFill></fill>");
                fills.iter().position(|f| *f == fill).unwrap_or_else(|| {
                    fills.push(fill);
                    fills.len() - 1
                })
            }
        };
        let mut xf = format!("<xf numFmtId=\"{fmt_id}\" fontId=\"{font_id}\" fillId=\"{fill_id}\" borderId=\"0\" xfId=\"0\"");
        if fmt_id != 0 {
            xf.push_str(" applyNumberFormat=\"1\"");
        }
        if font_id != 0 {
            xf.push_str(" applyFont=\"1\"");
        }
        if fill_id != 0 {
            xf.push_str(" applyFill=\"1\"");
        }
        if s.align.is_some() || s.wrap {
            xf.push_str(" applyAlignment=\"1\"><alignment");
            if let Some(a) = &s.align {
                xf.push_str(&format!(" horizontal=\"{}\"", esc(a)));
            }
            if s.wrap {
                xf.push_str(" wrapText=\"1\"");
            }
            xf.push_str("/></xf>");
        } else {
            xf.push_str("/>");
        }
        xfs.push(xf);
    }
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">");
    if !formats.is_empty() {
        out.push_str(&format!("<numFmts count=\"{}\">", formats.len()));
        for (id, code) in &formats {
            out.push_str(&format!("<numFmt numFmtId=\"{id}\" formatCode=\"{}\"/>", esc(code)));
        }
        out.push_str("</numFmts>");
    }
    out.push_str(&format!("<fonts count=\"{}\">{}</fonts>", fonts.len(), fonts.concat()));
    out.push_str(&format!("<fills count=\"{}\">{}</fills>", fills.len(), fills.concat()));
    out.push_str("<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>");
    out.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
    out.push_str(&format!("<cellXfs count=\"{}\">{}</cellXfs>", xfs.len(), xfs.concat()));
    out.push_str("<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles>");
    out.push_str("</styleSheet>");
    out
}

/// A workbook as an `.xlsx` file's bytes, each formula's value as `book`
/// has worked it out.
pub fn write(book: &Book) -> Result<Vec<u8>, String> {
    let wb = book.workbook();
    let mut strings: Vec<String> = Vec::new();
    let mut string_ids: HashMap<String, usize> = HashMap::new();
    let mut intern = |s: &str| -> usize {
        if let Some(i) = string_ids.get(s) {
            return *i;
        }
        strings.push(s.to_string());
        string_ids.insert(s.to_string(), strings.len() - 1);
        strings.len() - 1
    };
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut sheet_parts = Vec::new();
    // The sheets with charts, and the charts' numbers, for the content types.
    let mut drawings: Vec<usize> = Vec::new();
    let mut charts_made: Vec<usize> = Vec::new();
    for (si, sheet) in wb.sheets.iter().enumerate() {
        let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">");
        if let Some(u) = sheet.used() {
            x.push_str(&format!("<dimension ref=\"{}\"/>", if u.start == u.end { u.start.name() } else { u.name() }));
        }
        let f = sheet.frozen;
        if f.rows > 0 || f.cols > 0 {
            let pane = match (f.rows > 0, f.cols > 0) {
                (true, true) => "bottomRight",
                (true, false) => "bottomLeft",
                _ => "topRight",
            };
            let mut attrs = String::new();
            if f.cols > 0 {
                attrs.push_str(&format!(" xSplit=\"{}\"", f.cols));
            }
            if f.rows > 0 {
                attrs.push_str(&format!(" ySplit=\"{}\"", f.rows));
            }
            x.push_str(&format!(
                "<sheetViews><sheetView workbookViewId=\"0\"><pane{attrs} topLeftCell=\"{}\" activePane=\"{pane}\" state=\"frozen\"/></sheetView></sheetViews>",
                CellRef::new(f.rows, f.cols).name()
            ));
        }
        x.push_str("<sheetFormatPr defaultRowHeight=\"15\"/>");
        if !sheet.widths.is_empty() {
            x.push_str("<cols>");
            for (c, w) in &sheet.widths {
                let chars = ((w / 0.75 - 5.0) / 7.0).max(0.0);
                x.push_str(&format!("<col min=\"{0}\" max=\"{0}\" width=\"{chars}\" customWidth=\"1\"/>", c + 1));
            }
            x.push_str("</cols>");
        }
        x.push_str("<sheetData>");
        let mut row: Option<u32> = None;
        for (at, cell) in &sheet.cells {
            if row != Some(at.row) {
                if row.is_some() {
                    x.push_str("</row>");
                }
                x.push_str(&format!("<row r=\"{}\">", at.row + 1));
                row = Some(at.row);
            }
            let s = if cell.style != 0 { format!(" s=\"{}\"", cell.style) } else { String::new() };
            let r = at.name();
            if cell.input.starts_with('=') && cell.input.len() > 1 {
                let f = esc(&file_formula(&cell.input));
                match book.value(si, *at) {
                    Value::Number(n) => x.push_str(&format!("<c r=\"{r}\"{s}><f>{f}</f><v>{}</v></c>", number_text(n))),
                    Value::Text(t) => x.push_str(&format!("<c r=\"{r}\"{s} t=\"str\"><f>{f}</f><v>{}</v></c>", esc(&t))),
                    Value::Bool(b) => x.push_str(&format!("<c r=\"{r}\"{s} t=\"b\"><f>{f}</f><v>{}</v></c>", b as u8)),
                    Value::Error(e) => {
                        // #CYCLE! is ours; Excel says a cycle's cells are 0.
                        let shown = if e == ErrorKind::Cycle { "0".to_string() } else { e.text().to_string() };
                        let t = if e == ErrorKind::Cycle { "" } else { " t=\"e\"" };
                        x.push_str(&format!("<c r=\"{r}\"{s}{t}><f>{f}</f><v>{shown}</v></c>"));
                    }
                    Value::Empty => x.push_str(&format!("<c r=\"{r}\"{s}><f>{f}</f></c>")),
                }
                continue;
            }
            match literal(&cell.input) {
                Value::Empty => x.push_str(&format!("<c r=\"{r}\"{s}/>")),
                Value::Number(n) => x.push_str(&format!("<c r=\"{r}\"{s}><v>{}</v></c>", number_text(n))),
                Value::Text(t) => x.push_str(&format!("<c r=\"{r}\"{s} t=\"s\"><v>{}</v></c>", intern(&t))),
                Value::Bool(b) => x.push_str(&format!("<c r=\"{r}\"{s} t=\"b\"><v>{}</v></c>", b as u8)),
                Value::Error(e) => x.push_str(&format!("<c r=\"{r}\"{s} t=\"e\"><v>{}</v></c>", e.text())),
            }
        }
        if row.is_some() {
            x.push_str("</row>");
        }
        x.push_str("</sheetData>");
        if !sheet.charts.is_empty() {
            x.push_str("<drawing r:id=\"rId1\"/>");
            drawings.push(si);
        }
        x.push_str("</worksheet>");
        sheet_parts.push((format!("xl/worksheets/sheet{}.xml", si + 1), x.into_bytes()));
    }
    // Each sheet's charts: a drawing that places them, and a part each.
    let mut chart_n = 0;
    for (d, &si) in drawings.iter().enumerate() {
        let (d, sheet) = (d + 1, &wb.sheets[si]);
        sheet_parts.push((
            format!("xl/worksheets/_rels/sheet{}.xml.rels", si + 1),
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing\" Target=\"../drawings/drawing{d}.xml\"/></Relationships>").into_bytes(),
        ));
        let mut anchors = String::new();
        let mut rels = String::new();
        for (k, chart) in sheet.charts.iter().enumerate() {
            chart_n += 1;
            let at = CellRef::parse(&chart.at).unwrap_or(CellRef::new(0, 0));
            let (cols, col_off) = (chart.width / 64, (chart.width % 64) as u64 * 9525);
            let (rows, row_off) = (chart.height / 20, (chart.height % 20) as u64 * 9525);
            anchors.push_str(&format!(
                "<xdr:twoCellAnchor><xdr:from><xdr:col>{}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>{}</xdr:col><xdr:colOff>{col_off}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{row_off}</xdr:rowOff></xdr:to><xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{}\" name=\"Chart {}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/></xdr:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart r:id=\"rId{}\"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor>",
                at.col, at.row, at.col + cols, at.row + rows, k + 2, k + 1, k + 1
            ));
            rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart\" Target=\"../charts/chart{chart_n}.xml\"/>", k + 1));
            sheet_parts.push((format!("xl/charts/chart{chart_n}.xml"), chart_xml(book, si, chart).into_bytes()));
            charts_made.push(chart_n);
        }
        sheet_parts.push((
            format!("xl/drawings/drawing{d}.xml"),
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">{anchors}</xdr:wsDr>").into_bytes(),
        ));
        sheet_parts.push((
            format!("xl/drawings/_rels/drawing{d}.xml.rels"),
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>").into_bytes(),
        ));
    }

    let ns = "xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";
    let mut workbook = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook {ns}><bookViews><workbookView/></bookViews><sheets>");
    for (i, s) in wb.sheets.iter().enumerate() {
        workbook.push_str(&format!("<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>", esc(&s.name), i + 1, i + 1));
    }
    workbook.push_str("</sheets>");
    if !wb.names.is_empty() {
        workbook.push_str("<definedNames>");
        for (name, def) in &wb.names {
            workbook.push_str(&format!("<definedName name=\"{}\">{}</definedName>", esc(name), esc(def.trim_start_matches('='))));
        }
        workbook.push_str("</definedNames>");
    }
    workbook.push_str("<calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/></workbook>");

    let n = wb.sheets.len();
    let mut rels = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for i in 1..=n {
        rels.push_str(&format!("<Relationship Id=\"rId{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{i}.xml\"/>"));
    }
    rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>", n + 1));
    rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings\" Target=\"sharedStrings.xml\"/>", n + 2));
    rels.push_str("</Relationships>");

    let mut types = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>");
    for i in 1..=n {
        types.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"));
    }
    for d in 1..=drawings.len() {
        types.push_str(&format!("<Override PartName=\"/xl/drawings/drawing{d}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\"/>"));
    }
    for c in &charts_made {
        types.push_str(&format!("<Override PartName=\"/xl/charts/chart{c}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\"/>"));
    }
    types.push_str("<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/><Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/></Types>");

    let mut sst = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"{0}\" uniqueCount=\"{0}\">", strings.len());
    for s in &strings {
        let space = if s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) { " xml:space=\"preserve\"" } else { "" };
        sst.push_str(&format!("<si><t{space}>{}</t></si>", esc(s)));
    }
    sst.push_str("</sst>");

    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";
    parts.push(("[Content_Types].xml".into(), types.into_bytes()));
    parts.push(("_rels/.rels".into(), root_rels.as_bytes().to_vec()));
    parts.push(("xl/workbook.xml".into(), workbook.into_bytes()));
    parts.push(("xl/_rels/workbook.xml.rels".into(), rels.into_bytes()));
    parts.push(("xl/styles.xml".into(), write_styles(&wb.styles).into_bytes()));
    parts.push(("xl/sharedStrings.xml".into(), sst.into_bytes()));
    parts.extend(sheet_parts);
    zip::write(&parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Workbook {
        let mut wb = Workbook::default();
        let bold = wb.style_id(&Style { bold: true, color: Some("#c00000".into()), ..Default::default() });
        let money = wb.style_id(&Style { format: "$#,##0.00".into(), fill: Some("#ffff00".into()), align: Some("right".into()), ..Default::default() });
        let date = wb.style_id(&Style { format: "yyyy-mm-dd".into(), italic: true, underline: true, wrap: true, ..Default::default() });
        let s = &mut wb.sheets[0];
        let mut put = |at: &str, input: &str, style: u32| {
            s.cells.insert(CellRef::parse(at).unwrap(), Cell { input: input.into(), style });
        };
        put("A1", "Item", bold);
        put("B1", "Cost", bold);
        put("C1", "When", bold);
        put("A2", "Tea & <biscuits>", 0);
        put("B2", "12.5", money);
        put("C2", "45366", date);
        put("A3", "'0042", 0);
        put("B3", "-0.1", money);
        put("A4", "  spaced  ", 0);
        put("B4", "=SUM(B2:B3)", money);
        put("C4", "=XLOOKUP(\"Item\",A1:A3,B1:B3,\"none\")", 0);
        put("A5", "TRUE", 0);
        put("B5", "#N/A", 0);
        put("C5", "=IF(A5,\"yes\",\"no\")", 0);
        put("D9", "", bold);
        s.widths.insert(0, 140.0);
        s.widths.insert(3, 60.0);
        s.frozen = Frozen { rows: 1, cols: 1 };
        s.charts.push(crate::model::Chart { kind: crate::model::ChartKind::Bar, range: "A1:B3".into(), title: "Costs".into(), at: "E2".into(), width: 480, height: 300 });
        let mut other = Sheet::new("s2", "Q1 & Q2");
        other.cells.insert(CellRef::new(0, 0), Cell { input: "=Sheet1!B4*2".into(), style: 0 });
        other.cells.insert(CellRef::new(1, 0), Cell { input: "line one\nline two".into(), style: 0 });
        wb.sheets.push(other);
        wb.names.insert("Total".into(), "Sheet1!$B$4".into());
        wb
    }

    #[test]
    fn a_workbook_goes_through_xlsx_and_comes_back_unchanged() {
        let wb = sample();
        let bytes = write(&Book::new(wb.clone())).unwrap();
        assert_eq!(bytes, write(&Book::new(wb.clone())).unwrap(), "the same bytes each time");
        let back = read(&bytes).unwrap();
        assert!(back.dropped.is_empty(), "{:?}", back.dropped);
        assert_eq!(back.workbook, wb);
    }

    #[test]
    fn what_is_written_holds_the_values_and_the_prefixes_excel_expects() {
        let bytes = write(&Book::new(sample())).unwrap();
        let zip = Archive::new(&bytes).unwrap();
        let s1 = zip.text("xl/worksheets/sheet1.xml").unwrap();
        assert!(s1.contains("<f>SUM(B2:B3)</f><v>12.4</v>"), "{s1}");
        assert!(s1.contains("_xlfn.XLOOKUP(&quot;Item&quot;,A1:A3,B1:B3,&quot;none&quot;)</f><v>Cost</v>"), "{s1}");
        assert!(s1.contains("t=\"str\"><f>IF(A5,&quot;yes&quot;,&quot;no&quot;)</f><v>yes</v>"), "{s1}");
        assert!(s1.contains("state=\"frozen\""));
        let sst = zip.text("xl/sharedStrings.xml").unwrap();
        assert!(sst.contains("Tea &amp; &lt;biscuits&gt;") && sst.contains("<t xml:space=\"preserve\">  spaced  </t>"), "{sst}");
    }

    #[test]
    fn excels_own_ways_are_read() {
        // Shared formulas, inline strings, a cell with no reference, rich
        // text, a theme colour, a built-in date format, a sheet-scoped name.
        let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
            <row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B3" si="0">A1*2</f><v>2</v></c></row>
            <row r="2"><c r="A2"><v>2</v></c><c r="B2"><f t="shared" si="0"/><v>4</v></c></row>
            <row r="3"><c><v>3</v></c><c s="1"><f t="shared" si="0"/></c><c t="inlineStr"><is><t>inline</t></is></c><c t="s"><v>0</v></c></row>
            </sheetData><mergeCells count="1"><mergeCell ref="A5:B5"/></mergeCells></worksheet>"#;
        let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets><definedNames><definedName name="_xlnm.Print_Area" localSheetId="0">Data!$A$1:$B$3</definedName><definedName name="Top">Data!$A$1</definedName></definedNames></workbook>"#;
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="x" Target="/xl/worksheets/sheet1.xml"/></Relationships>"#;
        let sst = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><r><rPr><b/></rPr><t>rich</t></r><r><t xml:space="preserve"> text_x000D_</t></r><rPh><t>ignored</t></rPh></si></sst>"#;
        let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts><font><sz val="11"/></font><font><b/><color theme="1"/></font></fonts><fills><fill/><fill/></fills><cellXfs><xf numFmtId="0" fontId="0" fillId="0"/><xf numFmtId="14" fontId="1" fillId="0"/></cellXfs></styleSheet>"#;
        let parts = vec![
            ("xl/workbook.xml".to_string(), workbook.as_bytes().to_vec()),
            ("xl/_rels/workbook.xml.rels".to_string(), rels.as_bytes().to_vec()),
            ("xl/worksheets/sheet1.xml".to_string(), sheet.as_bytes().to_vec()),
            ("xl/sharedStrings.xml".to_string(), sst.as_bytes().to_vec()),
            ("xl/styles.xml".to_string(), styles.as_bytes().to_vec()),
            ("xl/media/image1.png".to_string(), vec![0]),
        ];
        let r = read(&zip::write(&parts).unwrap()).unwrap();
        let s = &r.workbook.sheets[0];
        let input = |a: &str| s.cells.get(&CellRef::parse(a).unwrap()).map(|c| c.input.clone()).unwrap_or_default();
        assert_eq!(s.name, "Data");
        assert_eq!(input("B2"), "=A2*2");
        assert_eq!(input("B3"), "=A3*2");
        assert_eq!(input("A3"), "3");
        assert_eq!(input("C3"), "inline");
        assert_eq!(input("D3"), "rich text\r");
        let style = s.cells[&CellRef::parse("B3").unwrap()].style;
        assert_eq!(r.workbook.styles[style as usize], Style { bold: true, format: "m/d/yyyy".into(), ..Default::default() });
        assert_eq!(r.workbook.names.get("Top").map(String::as_str), Some("Data!$A$1"));
        assert_eq!(r.workbook.names.len(), 1);
        assert_eq!(r.dropped, vec!["merged cells on “Data”", "pictures"]);
        let book = Book::new(r.workbook);
        assert_eq!(book.value(0, CellRef::parse("B3").unwrap()), Value::Number(6.0));
    }
}
