//! The `Name.sheet/` folder: what a workbook is on disk, in text that diffs.
//!
//! ```text
//! Name.sheet/
//!   workbook.json      sheets in order, styles, names, widths, frozen panes
//!   sheets/<id>.cells  a line per cell with anything in it, row by row:
//!                      reference, tab, what was typed[, tab, style]
//! ```
//!
//! The same workbook is written as the same bytes, so a save that changed
//! nothing changes no file, and a cell changed is a line changed.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::{Cell, Frozen, Sheet, Style, Workbook};
use crate::refs::{col_name, col_of, CellRef};

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct StoredSheet {
    id: String,
    name: String,
    /// Column letters to widths in points.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    widths: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "is_unfrozen")]
    frozen: Frozen,
}

fn is_unfrozen(f: &Frozen) -> bool {
    *f == Frozen::default()
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    sheets: Vec<StoredSheet>,
    /// Every style but the default, which is number 0: the first here is 1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    styles: Vec<Style>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    names: BTreeMap<String, String>,
}

/// Text as a `.cells` line keeps it: tabs, newlines and backslashes escaped.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A sheet's cells as its `.cells` file.
pub fn cells_text(sheet: &Sheet) -> String {
    let mut out = String::new();
    for (at, cell) in &sheet.cells {
        out.push_str(&at.name());
        out.push('\t');
        out.push_str(&escape(&cell.input));
        if cell.style != 0 {
            out.push('\t');
            out.push_str(&cell.style.to_string());
        }
        out.push('\n');
    }
    out
}

/// A `.cells` file read back; `name` says which, for the error.
pub fn read_cells(text: &str, name: &str) -> Result<BTreeMap<CellRef, Cell>, String> {
    let mut cells = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let at = parts.next().unwrap_or_default();
        let input = parts.next().ok_or_else(|| format!("{name}:{}: a cell with no tab after its reference", n + 1))?;
        let style = match parts.next() {
            Some(s) => s.parse().map_err(|_| format!("{name}:{}: “{s}” is not a style's number", n + 1))?,
            None => 0,
        };
        let at = CellRef::parse(at).ok_or_else(|| format!("{name}:{}: “{at}” is not a cell", n + 1))?;
        cells.insert(at, Cell { input: unescape(input), style });
    }
    Ok(cells)
}

/// `workbook.json` for a workbook.
pub fn workbook_text(wb: &Workbook) -> String {
    let stored = Stored {
        version: VERSION,
        sheets: wb
            .sheets
            .iter()
            .map(|s| StoredSheet {
                id: s.id.clone(),
                name: s.name.clone(),
                widths: s.widths.iter().map(|(c, w)| (col_name(*c), *w)).collect(),
                frozen: s.frozen,
            })
            .collect(),
        styles: wb.styles.iter().skip(1).cloned().collect(),
        names: wb.names.clone(),
    };
    let mut s = serde_json::to_string_pretty(&stored).unwrap_or_default();
    s.push('\n');
    s
}

/// A folder read into a workbook.
pub fn read(dir: &Path) -> Result<Workbook, String> {
    let shown = dir.display();
    let text = std::fs::read_to_string(dir.join("workbook.json")).map_err(|e| format!("{shown}/workbook.json: {e}"))?;
    let stored: Stored = serde_json::from_str(&text).map_err(|e| format!("{shown}/workbook.json: {e}"))?;
    if stored.version > VERSION {
        return Err(format!("{shown} was written by a newer Proton (version {})", stored.version));
    }
    let mut styles = vec![Style::default()];
    styles.extend(stored.styles);
    let mut sheets = Vec::new();
    for s in stored.sheets {
        if s.id.is_empty() || s.id.contains(['/', '\\', '.']) {
            return Err(format!("{shown}: “{}” is not a sheet's id", s.id));
        }
        let file = dir.join("sheets").join(format!("{}.cells", s.id));
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let cells = read_cells(&text, &format!("sheets/{}.cells", s.id))?;
        if let Some(bad) = cells.values().find(|c| c.style as usize >= styles.len()) {
            return Err(format!("{shown}: sheets/{}.cells names style {}, which there is not", s.id, bad.style));
        }
        let mut widths = BTreeMap::new();
        for (letters, w) in s.widths {
            let col = col_of(&letters).ok_or_else(|| format!("{shown}: “{letters}” is not a column"))?;
            widths.insert(col, w);
        }
        sheets.push(Sheet { id: s.id, name: s.name, cells, widths, frozen: s.frozen });
    }
    if sheets.is_empty() {
        return Err(format!("{shown} has no sheets"));
    }
    Ok(Workbook { sheets, styles, names: stored.names })
}

/// A workbook written into its folder: each file only if it changed, each
/// whole or not at all, and sheets no longer there taken away.
pub fn write(dir: &Path, wb: &Workbook) -> Result<(), String> {
    let sheets_dir = dir.join("sheets");
    std::fs::create_dir_all(&sheets_dir).map_err(|e| format!("{}: {e}", sheets_dir.display()))?;
    for sheet in &wb.sheets {
        put(&sheets_dir.join(format!("{}.cells", sheet.id)), &cells_text(sheet))?;
    }
    // A sheet taken away: its file goes too.
    if let Ok(entries) = std::fs::read_dir(&sheets_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_suffix(".cells") {
                if !wb.sheets.iter().any(|s| s.id == id) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    // Last, so a folder read halfway through a save names only sheets that
    // are whole.
    put(&dir.join("workbook.json"), &workbook_text(wb))
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

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-sheet-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> Workbook {
        let mut wb = Workbook::default();
        let bold = wb.style_id(&Style { bold: true, format: "#,##0.00".into(), ..Default::default() });
        let s = &mut wb.sheets[0];
        s.cells.insert(CellRef::new(0, 0), Cell { input: "Item".into(), style: bold });
        s.cells.insert(CellRef::new(0, 1), Cell { input: "Cost".into(), style: bold });
        s.cells.insert(CellRef::new(1, 0), Cell { input: "tab\there, line\nthere, a \\ too".into(), style: 0 });
        s.cells.insert(CellRef::new(1, 1), Cell { input: "12.5".into(), style: 0 });
        s.cells.insert(CellRef::new(2, 1), Cell { input: "=SUM(B2:B2)".into(), style: bold });
        s.widths.insert(0, 140.0);
        s.frozen = Frozen { rows: 1, cols: 0 };
        let mut other = Sheet::new("s2", "Q1 data");
        other.cells.insert(CellRef::new(4, 27), Cell { input: "=Sheet1!B3*2".into(), style: 0 });
        wb.sheets.push(other);
        wb.names.insert("Total".into(), "Sheet1!$B$3".into());
        wb
    }

    #[test]
    fn a_workbook_comes_back_from_its_folder_as_it_went_and_saves_the_same_bytes() {
        let dir = scratch("round");
        let wb = sample();
        write(&dir, &wb).unwrap();
        let back = read(&dir).unwrap();
        assert_eq!(back, wb);
        let before = std::fs::read_to_string(dir.join("sheets/s1.cells")).unwrap();
        assert_eq!(before, "A1\tItem\t1\nB1\tCost\t1\nA2\ttab\\there, line\\nthere, a \\\\ too\nB2\t12.5\nB3\t=SUM(B2:B2)\t1\n");
        let modified = std::fs::metadata(dir.join("sheets/s1.cells")).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(&dir, &back).unwrap();
        assert_eq!(std::fs::metadata(dir.join("sheets/s1.cells")).unwrap().modified().unwrap(), modified, "an unchanged file is not written");
        let json = std::fs::read_to_string(dir.join("workbook.json")).unwrap();
        assert!(json.contains("\"A\": 140.0") && json.contains("\"name\": \"Q1 data\""), "{json}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_sheet_taken_away_takes_its_file_and_a_bad_folder_says_why() {
        let dir = scratch("gone");
        let mut wb = sample();
        write(&dir, &wb).unwrap();
        wb.sheets.pop();
        write(&dir, &wb).unwrap();
        assert!(!dir.join("sheets/s2.cells").exists());
        std::fs::write(dir.join("sheets/s1.cells"), "B7\n").unwrap();
        assert!(read(&dir).unwrap_err().contains("s1.cells:1"));
        std::fs::write(dir.join("sheets/s1.cells"), "B7\tx\t9\n").unwrap();
        assert!(read(&dir).unwrap_err().contains("style 9"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
