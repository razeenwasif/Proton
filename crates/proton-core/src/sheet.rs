//! Excel mode: the workbook open in this project, its history, and its saves.
//!
//! `proton-sheet` owns the cells, the formulas and what they come to. What
//! lives here is the editor's half: which workbook is open and from what kind
//! of file, the undo history, whether it has changed since it was saved, and
//! the workbooks a project holds, for the home page shown when none is.
//!
//! The window draws only the rows on screen, so it asks for them — a sheet,
//! a rectangle — and is answered with each cell there as it shows: its text
//! formatted, what was typed, its style. A change answers with the cells it
//! moved that are in that rectangle, and nothing else, so a sheet of a
//! million rows is drawn as one of ten is. `generation` numbers each
//! workbook opened, so an answer about the last one is never taken for this.
//!
//! Edits are held until ⌘S, as Codex's are, and the quit prompt names an
//! unsaved workbook. A workbook opened from `.xlsx` is saved back to it, and
//! one from `.csv` or `.tsv` too — its first sheet, as such a file holds one.

use std::path::{Path, PathBuf};

use proton_sheet::edit::{self, Axis};
use proton_sheet::{numfmt, Book, Cell, CellRef, Range, Value, Workbook};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bus::{Command, Event, NoticeLevel};
use crate::workspace::notice;

/// A workbook's folder ends so.
pub const EXTENSION: &str = "sheet";

/// No more than this many cells are sent for one rectangle: the window asks
/// for what is on screen, and a screen holds far fewer.
const MOST_ROWS: u32 = 400;
const MOST_COLS: u32 = 120;
/// No more cells than this are copied, filled or formatted in one go.
const MOST_CLIP: u64 = 250_000;

/// What a workbook is kept as on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum SheetFileKind {
    /// A `Name.sheet/` folder of text that diffs.
    Folder,
    Xlsx,
    Csv,
    Tsv,
}

impl SheetFileKind {
    fn of(path: &Path) -> Option<SheetFileKind> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            EXTENSION => Some(SheetFileKind::Folder),
            "xlsx" => Some(SheetFileKind::Xlsx),
            "csv" => Some(SheetFileKind::Csv),
            "tsv" | "tab" => Some(SheetFileKind::Tsv),
            _ => None,
        }
    }
}

/// A workbook found in the project, for the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SheetFound {
    /// As the project names it.
    pub path: String,
    pub name: String,
    pub kind: SheetFileKind,
}

/// How a cell is drawn: `proton-sheet`'s style, as the window reads it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SheetStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<String>,
    pub fill: Option<String>,
    pub format: String,
    pub align: Option<String>,
    pub wrap: bool,
}

impl From<&proton_sheet::Style> for SheetStyle {
    fn from(s: &proton_sheet::Style) -> Self {
        SheetStyle {
            bold: s.bold,
            italic: s.italic,
            underline: s.underline,
            color: s.color.clone(),
            fill: s.fill.clone(),
            format: s.format.clone(),
            align: s.align.clone(),
            wrap: s.wrap,
        }
    }
}

/// A change to cells' style: only the parts given, the rest kept. An empty
/// colour, alignment or format clears it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SheetStylePatch {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub wrap: Option<bool>,
    pub color: Option<String>,
    pub fill: Option<String>,
    pub align: Option<String>,
    pub format: Option<String>,
}

impl SheetStylePatch {
    fn apply(&self, s: &mut proton_sheet::Style) {
        let some = |v: &String| (!v.is_empty()).then(|| v.clone());
        if let Some(b) = self.bold {
            s.bold = b;
        }
        if let Some(b) = self.italic {
            s.italic = b;
        }
        if let Some(b) = self.underline {
            s.underline = b;
        }
        if let Some(b) = self.wrap {
            s.wrap = b;
        }
        if let Some(c) = &self.color {
            s.color = some(c);
        }
        if let Some(c) = &self.fill {
            s.fill = some(c);
        }
        if let Some(a) = &self.align {
            s.align = some(a).filter(|a| matches!(a.as_str(), "left" | "center" | "right"));
        }
        if let Some(f) = &self.format {
            s.format = f.clone();
        }
    }
}

/// One sheet's tab, and what the grid needs to lay it out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SheetTabView {
    pub name: String,
    /// One past the last row and column with anything in them.
    pub rows: u32,
    pub cols: u32,
    /// Rows and columns that stay put while the rest scrolls.
    pub frozen: [u32; 2],
    /// Widths in points, where a column is not the default.
    pub widths: Vec<(u32, f64)>,
}

/// The workbook open: everything but its cells, which the window asks for
/// by the rectangle on screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SheetBookView {
    pub path: String,
    pub name: String,
    pub kind: SheetFileKind,
    pub sheets: Vec<SheetTabView>,
    /// The sheet on screen.
    pub active: u32,
    /// By number, as cells name them; 0 is the default.
    pub styles: Vec<SheetStyle>,
    pub generation: u32,
    #[ts(type = "number")]
    pub version: u64,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// What a file held that was not kept, said when it opened.
    pub dropped: Vec<String>,
}

/// A cell as it is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SheetCellView {
    pub row: u32,
    pub col: u32,
    /// What it shows, formatted.
    pub text: String,
    /// What was typed: the formula bar's.
    pub input: String,
    pub style: u32,
    /// `n` a number, `t` text, `b` true or false, `e` an error, `` nothing:
    /// which way it lines up when its style does not say.
    pub kind: String,
    /// A colour its number format gives it, `[Red]`'s.
    pub color: Option<String>,
}

/// One undoable change.
#[derive(Debug, Clone)]
enum Step {
    /// Cells of one sheet — what was typed and their style — as they were
    /// and as they became.
    Cells { sheet: usize, before: Vec<(CellRef, Cell)>, after: Vec<(CellRef, Cell)> },
    /// Anything to the workbook's shape — a sheet added, renamed, taken
    /// away — kept whole, as such changes are few.
    Book { before: Box<Workbook>, after: Box<Workbook>, active: (usize, usize) },
}

struct Open {
    path: PathBuf,
    shown: String,
    name: String,
    kind: SheetFileKind,
    book: Book,
    active: usize,
    /// Done steps, each with the state it left; undone ones to redo.
    done: Vec<(Step, u64)>,
    undone: Vec<(Step, u64)>,
    state: u64,
    saved: u64,
    /// The last state number given out.
    states: u64,
    generation: u32,
    version: u64,
    /// The rectangle last asked for: (sheet, rows, cols).
    view: Option<(usize, [u32; 2], [u32; 2])>,
    dropped: Vec<String>,
    /// What was last copied here, to paste with its formulas.
    clip: Option<Clip>,
}

#[derive(Debug, Clone)]
struct Clip {
    generation: u32,
    sheet: usize,
    range: Range,
    cells: Vec<(CellRef, Cell)>,
    /// What went on the clipboard: a paste of this is a paste of these.
    text: String,
    cut: bool,
}

impl std::fmt::Debug for Open {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Open").field("shown", &self.shown).field("generation", &self.generation).finish()
    }
}

/// The workbook open in a project, and the editor's place in it.
#[derive(Debug, Default)]
pub struct Sheets {
    open: Option<Open>,
    generations: u32,
}

impl Sheets {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Everything the window needs to draw the mode.
    pub fn events(&self, root: &Path) -> Vec<Event> {
        vec![home_event(root), self.book_event()]
    }

    pub fn apply(&mut self, root: &Path, command: Command) -> Vec<Event> {
        match command {
            Command::SheetNew { name, folder } => self.new_book(root, &name, folder),
            Command::SheetOpen { path } => {
                let file = match inside(root, &path) {
                    Ok(file) => file,
                    Err(why) => return vec![notice(why, NoticeLevel::Warn)],
                };
                if self.open.as_ref().is_some_and(|o| o.path == file) {
                    return self.events(root);
                }
                if self.dirty() {
                    return vec![notice("save or discard the open workbook's changes before opening another".into(), NoticeLevel::Warn)];
                }
                match load(&file) {
                    Ok((wb, dropped)) => {
                        self.show(root, file, wb, dropped.clone());
                        let mut events = self.events(root);
                        if !dropped.is_empty() {
                            events.push(notice(format!("not kept from this file: {}", dropped.join(", ")), NoticeLevel::Warn));
                        }
                        events
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::SheetClose { discard } => {
                if !discard && self.dirty() {
                    return vec![notice("the workbook has unsaved changes".into(), NoticeLevel::Warn)];
                }
                self.open = None;
                self.events(root)
            }
            Command::SheetSave => {
                let Some(open) = &mut self.open else { return Vec::new() };
                match save(open) {
                    Ok(()) => {
                        open.saved = open.state;
                        let mut events = vec![self.book_event(), home_event(root)];
                        events.push(notice(format!("saved {}", self.open.as_ref().map_or("", |o| o.shown.as_str())), NoticeLevel::Info));
                        events
                    }
                    Err(why) => vec![notice(format!("{} was not saved: {why}", open.shown), NoticeLevel::Error)],
                }
            }
            Command::SheetView { generation, sheet, rows, cols } => {
                let Some(open) = &mut self.open else { return Vec::new() };
                if generation != open.generation || sheet as usize >= open.book.workbook().sheets.len() {
                    return Vec::new();
                }
                let rows = [rows[0].min(rows[1]), rows[1].max(rows[0]).min(rows[0].min(rows[1]) + MOST_ROWS)];
                let cols = [cols[0].min(cols[1]), cols[1].max(cols[0]).min(cols[0].min(cols[1]) + MOST_COLS)];
                open.view = Some((sheet as usize, rows, cols));
                vec![cells_event(open, sheet as usize, rows, cols, None)]
            }
            Command::SheetSet { generation, sheet, cells } => self.set(generation, sheet as usize, cells),
            Command::SheetCopy { generation, sheet, range, cut } => self.copy(generation, sheet as usize, &range, cut),
            Command::SheetPaste { generation, sheet, at, text } => self.paste(generation, sheet as usize, &at, &text),
            Command::SheetFill { generation, sheet, from, to } => self.fill(generation, sheet as usize, &from, &to),
            Command::SheetFormat { generation, sheet, range, patch } => self.format(generation, sheet as usize, &range, *patch),
            Command::SheetInsert { generation, sheet, cols, at, n } => self.lines(generation, sheet as usize, cols, at, n, true),
            Command::SheetDelete { generation, sheet, cols, at, n } => self.lines(generation, sheet as usize, cols, at, n, false),
            Command::SheetWidth { generation, sheet, col, width } => {
                if let Err(events) = self.checked(generation, sheet as usize) {
                    return events;
                }
                self.reshape(|wb, active| {
                    let widths = &mut wb.sheets[sheet as usize].widths;
                    match width {
                        Some(w) if (4.0..=1000.0).contains(&w) => {
                            widths.insert(col, (w * 100.0).round() / 100.0);
                        }
                        Some(_) => return Err("a column is between 4 and 1000 points wide".into()),
                        None => {
                            widths.remove(&col);
                        }
                    }
                    Ok(active)
                })
            }
            Command::SheetFreeze { generation, sheet, rows, cols } => {
                if let Err(events) = self.checked(generation, sheet as usize) {
                    return events;
                }
                self.reshape(|wb, active| {
                    wb.sheets[sheet as usize].frozen = proton_sheet::model::Frozen { rows: rows.min(100), cols: cols.min(50) };
                    Ok(active)
                })
            }
            Command::SheetUndo => self.step(true),
            Command::SheetRedo => self.step(false),
            Command::SheetTab { sheet } => {
                let Some(open) = &mut self.open else { return Vec::new() };
                if (sheet as usize) < open.book.workbook().sheets.len() {
                    open.active = sheet as usize;
                }
                vec![self.book_event()]
            }
            Command::SheetAddSheet { name } => self.reshape(|wb, active| {
                let name = match name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
                    Some(n) => n,
                    None => (wb.sheets.len() + 1..).map(|n| format!("Sheet{n}")).find(|n| wb.sheet_index(n).is_none()).unwrap_or_default(),
                };
                check_name(wb, &name, None)?;
                let id = wb.fresh_id();
                wb.sheets.insert(active + 1, proton_sheet::Sheet::new(&id, &name));
                Ok(active + 1)
            }),
            Command::SheetRenameSheet { sheet, name } => self.reshape(|wb, _| {
                let name = name.trim().to_string();
                let i = sheet as usize;
                if i >= wb.sheets.len() {
                    return Err("there is no such sheet".into());
                }
                check_name(wb, &name, Some(i))?;
                wb.sheets[i].name = name;
                Ok(i)
            }),
            Command::SheetRemoveSheet { sheet } => self.reshape(|wb, active| {
                let i = sheet as usize;
                if i >= wb.sheets.len() {
                    return Err("there is no such sheet".into());
                }
                if wb.sheets.len() == 1 {
                    return Err("a workbook keeps at least one sheet".into());
                }
                wb.sheets.remove(i);
                Ok(if active >= wb.sheets.len() { wb.sheets.len() - 1 } else { active })
            }),
            _ => Vec::new(),
        }
    }

    fn new_book(&mut self, root: &Path, name: &str, folder: Option<String>) -> Vec<Event> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return vec![notice("a new workbook needs a name".into(), NoticeLevel::Warn)];
        }
        if self.dirty() {
            return vec![notice("save or discard the open workbook's changes before making another".into(), NoticeLevel::Warn)];
        }
        let folder = folder.filter(|f| !f.trim().is_empty()).unwrap_or_else(|| format!("{}.{EXTENSION}", slug(&name)));
        let file = match inside(root, &folder) {
            Ok(file) => file,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        if file.exists() {
            return vec![notice(format!("{folder} is already there"), NoticeLevel::Warn)];
        }
        let kind = SheetFileKind::of(&file).unwrap_or(SheetFileKind::Folder);
        let wb = Workbook::default();
        self.show(root, file, wb, Vec::new());
        let Some(open) = &mut self.open else { return Vec::new() };
        open.kind = kind;
        if let Err(why) = save(open) {
            self.open = None;
            return vec![notice(why, NoticeLevel::Warn)];
        }
        let mut events = self.events(root);
        events.push(notice(format!("made {name} in {folder}"), NoticeLevel::Info));
        events
    }

    fn show(&mut self, root: &Path, path: PathBuf, wb: Workbook, dropped: Vec<String>) {
        self.generations += 1;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let rel = path.strip_prefix(root).or_else(|_| path.strip_prefix(&real));
        self.open = Some(Open {
            shown: rel.map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| path.display().to_string()),
            kind: SheetFileKind::of(&path).unwrap_or(SheetFileKind::Folder),
            path,
            name,
            book: Book::new(wb),
            active: 0,
            done: Vec::new(),
            undone: Vec::new(),
            state: 0,
            saved: 0,
            states: 0,
            generation: self.generations,
            version: 0,
            view: None,
            dropped,
            clip: None,
        });
    }

    /// The workbook open, if the window's numbers are for it: its
    /// generation and a sheet it has.
    fn checked(&mut self, generation: u32, sheet: usize) -> Result<&mut Open, Vec<Event>> {
        let Some(open) = &mut self.open else {
            return Err(vec![notice("no workbook is open".into(), NoticeLevel::Warn)]);
        };
        if generation != open.generation {
            return Err(vec![notice("that workbook is no longer open".into(), NoticeLevel::Warn)]);
        }
        if sheet >= open.book.workbook().sheets.len() {
            return Err(vec![notice("there is no such sheet".into(), NoticeLevel::Warn)]);
        }
        Ok(open)
    }

    fn set(&mut self, generation: u32, sheet: usize, cells: Vec<(String, String)>) -> Vec<Event> {
        let open = match self.checked(generation, sheet) {
            Ok(open) => open,
            Err(events) => return events,
        };
        let mut after = Vec::new();
        for (at, input) in cells {
            let Some(at) = CellRef::parse(&at) else {
                return vec![notice(format!("{at} is not a cell"), NoticeLevel::Warn)];
            };
            let style = cell_at(&open.book, sheet, at).style;
            after.push((at, Cell { input, style }));
        }
        self.change(sheet, after)
    }

    /// Cells of a sheet changed as one step: those that differ from what
    /// they were, and nothing when none do.
    fn change(&mut self, sheet: usize, cells: Vec<(CellRef, Cell)>) -> Vec<Event> {
        let Some(open) = &mut self.open else { return Vec::new() };
        let mut before = Vec::new();
        let mut after = Vec::new();
        for (at, cell) in cells {
            let was = cell_at(&open.book, sheet, at);
            if was != cell {
                before.push((at, was));
                after.push((at, cell));
            }
        }
        if after.is_empty() {
            return Vec::new();
        }
        let moved = apply_cells(&mut open.book, sheet, &after);
        record(open, Step::Cells { sheet, before, after });
        let mut events = vec![self.book_event()];
        events.extend(self.moved_event(moved));
        events
    }

    /// The selection's cells as text for the clipboard — tab between cells,
    /// a line a row, values as they show — and kept, so a paste of the same
    /// text here brings their formulas and styles.
    fn copy(&mut self, generation: u32, sheet: usize, range: &str, cut: bool) -> Vec<Event> {
        let open = match self.checked(generation, sheet) {
            Ok(open) => open,
            Err(events) => return events,
        };
        let Some(range) = range_of(range) else {
            return vec![notice(format!("{range} is not a range"), NoticeLevel::Warn)];
        };
        let wb = open.book.workbook();
        let range = clip_to_used(wb, sheet, range);
        if range.rows() as u64 * range.cols() as u64 > MOST_CLIP {
            return vec![notice("that is more than can be copied at once".into(), NoticeLevel::Warn)];
        }
        let mut text = String::new();
        let mut cells = Vec::new();
        for r in range.start.row..=range.end.row {
            let mut line = Vec::new();
            for c in range.start.col..=range.end.col {
                let at = CellRef::new(r, c);
                let cell = cell_at(&open.book, sheet, at);
                let format = wb.styles.get(cell.style as usize).map_or("", |st| st.format.as_str());
                let shown = numfmt::show(&open.book.value(sheet, at), format).text;
                line.push(shown.replace(['\t', '\n', '\r'], " "));
                cells.push((at, cell));
            }
            text.push_str(line.join("\t").trim_end_matches('\t'));
            text.push('\n');
        }
        open.clip = Some(Clip { generation, sheet, range, cells, text: text.clone(), cut });
        vec![Event::SheetClip { text }]
    }

    /// Text pasted at a cell: what was copied here, formulas moved and
    /// styles kept — and cut cells emptied — or else tab-separated text, as
    /// if typed.
    fn paste(&mut self, generation: u32, sheet: usize, at: &str, text: &str) -> Vec<Event> {
        let open = match self.checked(generation, sheet) {
            Ok(open) => open,
            Err(events) => return events,
        };
        let Some(at) = CellRef::parse(at) else {
            return vec![notice(format!("{at} is not a cell"), NoticeLevel::Warn)];
        };
        let ours = open.clip.as_ref().filter(|c| c.generation == generation && c.text.trim_end() == text.replace("\r\n", "\n").trim_end()).cloned();
        let mut cells: Vec<(CellRef, Cell)> = Vec::new();
        match ours {
            Some(clip) => {
                let (dr, dc) = (at.row as i64 - clip.range.start.row as i64, at.col as i64 - clip.range.start.col as i64);
                if clip.cut {
                    // A cut is moved once: its cells are emptied, then filled where they land.
                    if clip.sheet == sheet {
                        cells.extend(clip.cells.iter().map(|(from, _)| (*from, Cell::default())));
                    } else {
                        let mut out = self.change(clip.sheet, clip.cells.iter().map(|(from, _)| (*from, Cell::default())).collect());
                        if let Some(open) = &mut self.open {
                            open.clip = None;
                        }
                        out.extend(self.paste_cells(sheet, clip.cells.iter().map(|(from, cell)| (shift(*from, dr, dc), Cell { input: cell.input.clone(), style: cell.style })).collect()));
                        return out;
                    }
                }
                for (from, cell) in &clip.cells {
                    let Some(to) = shift(*from, dr, dc) else { continue };
                    let input = if clip.cut { cell.input.clone() } else { edit::moved_input(&cell.input, dr, dc) };
                    cells.retain(|(a, _)| *a != to);
                    cells.push((to, Cell { input, style: cell.style }));
                }
                if clip.cut {
                    if let Some(open) = &mut self.open {
                        open.clip = None;
                    }
                }
            }
            None => {
                for (dr, row) in proton_sheet::csv::rows(text, '\t').into_iter().enumerate() {
                    for (dc, field) in row.into_iter().enumerate() {
                        let Some(to) = shift(at, dr as i64, dc as i64) else { continue };
                        let style = cell_at(&open.book, sheet, to).style;
                        cells.push((to, Cell { input: field, style }));
                    }
                }
            }
        }
        self.change(sheet, cells)
    }

    fn paste_cells(&mut self, sheet: usize, cells: Vec<(Option<CellRef>, Cell)>) -> Vec<Event> {
        self.change(sheet, cells.into_iter().filter_map(|(at, cell)| at.map(|at| (at, cell))).collect())
    }

    fn fill(&mut self, generation: u32, sheet: usize, from: &str, to: &str) -> Vec<Event> {
        let open = match self.checked(generation, sheet) {
            Ok(open) => open,
            Err(events) => return events,
        };
        let (Some(from), Some(to)) = (range_of(from), range_of(to)) else {
            return vec![notice("a fill needs two ranges".into(), NoticeLevel::Warn)];
        };
        if !(to.contains(from.start) && to.contains(from.end)) || to.rows() as u64 * to.cols() as u64 > MOST_CLIP {
            return vec![notice("a fill goes on from what is selected, down or across".into(), NoticeLevel::Warn)];
        }
        let cells = edit::fill(&open.book.workbook().sheets[sheet].cells, from, to);
        self.change(sheet, cells)
    }

    /// Rows or columns put in or taken out, every formula following.
    fn lines(&mut self, generation: u32, sheet: usize, cols: bool, at: u32, n: u32, insert: bool) -> Vec<Event> {
        if let Err(events) = self.checked(generation, sheet) {
            return events;
        }
        let axis = if cols { Axis::Cols } else { Axis::Rows };
        let n = n.clamp(1, 10_000);
        self.reshape(|wb, active| {
            if insert {
                edit::insert(wb, sheet, axis, at, n);
            } else {
                edit::delete(wb, sheet, axis, at, n);
            }
            Ok(active)
        })
    }

    /// A style changed over a range: only what the patch says, the rest of
    /// each cell's style kept.
    fn format(&mut self, generation: u32, sheet: usize, range: &str, patch: SheetStylePatch) -> Vec<Event> {
        let open = match self.checked(generation, sheet) {
            Ok(open) => open,
            Err(events) => return events,
        };
        let Some(range) = range_of(range) else {
            return vec![notice(format!("{range} is not a range"), NoticeLevel::Warn)];
        };
        let range = clip_to_used(open.book.workbook(), sheet, range);
        if range.rows() as u64 * range.cols() as u64 > MOST_CLIP {
            return vec![notice("that is more cells than can be formatted at once".into(), NoticeLevel::Warn)];
        }
        let mut cells = Vec::new();
        for r in range.start.row..=range.end.row {
            for c in range.start.col..=range.end.col {
                let at = CellRef::new(r, c);
                let cell = cell_at(&open.book, sheet, at);
                let mut style = open.book.workbook().styles.get(cell.style as usize).cloned().unwrap_or_default();
                patch.apply(&mut style);
                let id = open.book.workbook_mut().style_id(&style);
                cells.push((at, Cell { input: cell.input, style: id }));
            }
        }
        self.change(sheet, cells)
    }

    fn step(&mut self, back: bool) -> Vec<Event> {
        let Some(open) = &mut self.open else { return Vec::new() };
        let (from, to) = if back { (&mut open.done, &mut open.undone) } else { (&mut open.undone, &mut open.done) };
        let Some((step, state)) = from.pop() else { return Vec::new() };
        let moved = match &step {
            Step::Cells { sheet, before, after } => {
                open.active = *sheet;
                Some(apply_cells(&mut open.book, *sheet, if back { before } else { after }))
            }
            Step::Book { before, after, active } => {
                *open.book.workbook_mut() = if back { (**before).clone() } else { (**after).clone() };
                open.book.rebuild();
                open.active = if back { active.0 } else { active.1 };
                None
            }
        };
        // Undoing returns to the state before this step: the one the step
        // below it left, or the opening's.
        open.state = if back { from.last().map_or(0, |(_, s)| *s) } else { state };
        to.push((step, state));
        open.version += 1;
        let mut events = vec![self.book_event()];
        match moved {
            Some(moved) => events.extend(self.moved_event(moved)),
            None => events.extend(self.view_event()),
        }
        events
    }

    /// A change to the workbook's shape, as one step.
    fn reshape(&mut self, change: impl FnOnce(&mut Workbook, usize) -> Result<usize, String>) -> Vec<Event> {
        let Some(open) = &mut self.open else { return Vec::new() };
        let before = open.book.workbook().clone();
        let mut after = before.clone();
        let active = match change(&mut after, open.active) {
            Ok(a) => a,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        *open.book.workbook_mut() = after.clone();
        open.book.rebuild();
        let was = open.active;
        open.active = active;
        record(open, Step::Book { before: Box::new(before), after: Box::new(after), active: (was, active) });
        let mut events = vec![self.book_event()];
        events.extend(self.view_event());
        events
    }

    /// The cells a change moved that are on screen, or the whole screen
    /// again when that is most of it.
    fn moved_event(&self, moved: Vec<(usize, CellRef)>) -> Option<Event> {
        let open = self.open.as_ref()?;
        let (sheet, rows, cols) = open.view?;
        let frozen = open.book.workbook().sheets.get(sheet).map_or(Default::default(), |s| s.frozen);
        let inside: Vec<CellRef> = moved
            .into_iter()
            .filter(|(s, at)| *s == sheet && in_view(*at, rows, cols, frozen))
            .map(|(_, at)| at)
            .collect();
        if inside.is_empty() {
            return None;
        }
        if inside.len() > 2000 {
            return Some(cells_event(open, sheet, rows, cols, None));
        }
        Some(cells_event(open, sheet, rows, cols, Some(&inside)))
    }

    /// The rectangle last asked for, again.
    fn view_event(&self) -> Option<Event> {
        let open = self.open.as_ref()?;
        let (sheet, rows, cols) = open.view?;
        (sheet < open.book.workbook().sheets.len()).then(|| cells_event(open, sheet, rows, cols, None))
    }

    fn dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.state != o.saved)
    }

    /// The workbook with unsaved changes, as the project names it.
    pub fn unsaved(&self) -> Vec<String> {
        match &self.open {
            Some(open) if open.state != open.saved => vec![open.shown.clone()],
            _ => Vec::new(),
        }
    }

    /// Save it, as quitting does; what could not be, and why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let Some(open) = &mut self.open else { return Vec::new() };
        if open.state == open.saved {
            return Vec::new();
        }
        match save(open) {
            Ok(()) => {
                open.saved = open.state;
                Vec::new()
            }
            Err(why) => vec![format!("{}: {why}", open.shown)],
        }
    }

    pub fn book_event(&self) -> Event {
        let Some(open) = &self.open else { return Event::SheetBook { book: None } };
        let wb = open.book.workbook();
        Event::SheetBook {
            book: Some(SheetBookView {
                path: open.shown.clone(),
                name: open.name.clone(),
                kind: open.kind,
                sheets: wb
                    .sheets
                    .iter()
                    .map(|s| {
                        let used = s.used();
                        SheetTabView {
                            name: s.name.clone(),
                            rows: used.map_or(0, |u| u.end.row + 1),
                            cols: used.map_or(0, |u| u.end.col + 1),
                            frozen: [s.frozen.rows, s.frozen.cols],
                            widths: s.widths.iter().map(|(c, w)| (*c, *w)).collect(),
                        }
                    })
                    .collect(),
                active: open.active as u32,
                styles: wb.styles.iter().map(SheetStyle::from).collect(),
                generation: open.generation,
                version: open.version,
                dirty: open.state != open.saved,
                can_undo: !open.done.is_empty(),
                can_redo: !open.undone.is_empty(),
                dropped: open.dropped.clone(),
            }),
        }
    }
}

fn record(open: &mut Open, step: Step) {
    open.states += 1;
    open.state = open.states;
    open.done.push((step, open.state));
    open.undone.clear();
    open.version += 1;
}

/// Cells put into a sheet, what was typed and their style; the cells
/// whose values moved, and those put.
fn apply_cells(book: &mut Book, sheet: usize, cells: &[(CellRef, Cell)]) -> Vec<(usize, CellRef)> {
    let edits: Vec<_> = cells.iter().map(|(at, cell)| (sheet, *at, cell.input.clone())).collect();
    let mut moved = book.set_many(&edits);
    // Styles change nothing worked out: they are put straight in.
    let s = &mut book.workbook_mut().sheets[sheet];
    for (at, cell) in cells {
        match s.cells.get_mut(at) {
            Some(kept) if cell.style == 0 && kept.input.is_empty() => {
                s.cells.remove(at);
            }
            Some(kept) => kept.style = cell.style,
            None if cell.style != 0 => {
                s.cells.insert(*at, Cell { input: String::new(), style: cell.style });
            }
            None => {}
        }
    }
    // A cell put shows its new input and style even where its value is as it was.
    moved.extend(cells.iter().map(|(at, _)| (sheet, *at)));
    moved.sort_unstable();
    moved.dedup();
    moved
}

/// A cell as it is kept, or an empty one.
fn cell_at(book: &Book, sheet: usize, at: CellRef) -> Cell {
    book.workbook().sheets.get(sheet).and_then(|s| s.cells.get(&at)).cloned().unwrap_or_default()
}

/// `B7` or `A1:C9`.
fn range_of(s: &str) -> Option<Range> {
    Range::parse(s.trim()).or_else(|| CellRef::parse(s.trim()).map(Range::cell))
}

/// A range cut to where the sheet has anything — a whole column copied is
/// the rows in use — but never smaller than its first cell.
fn clip_to_used(wb: &Workbook, sheet: usize, r: Range) -> Range {
    let Some(used) = wb.sheets.get(sheet).and_then(|s| s.used()) else { return Range::cell(r.start) };
    let row = r.end.row.min(used.end.row.max(r.start.row));
    let col = r.end.col.min(used.end.col.max(r.start.col));
    Range::new(r.start, CellRef::new(row, col))
}

fn shift(at: CellRef, dr: i64, dc: i64) -> Option<CellRef> {
    let (r, c) = (at.row as i64 + dr, at.col as i64 + dc);
    ((0..=proton_sheet::refs::MAX_ROW as i64).contains(&r) && (0..=proton_sheet::refs::MAX_COL as i64).contains(&c)).then(|| CellRef::new(r as u32, c as u32))
}

fn check_name(wb: &Workbook, name: &str, except: Option<usize>) -> Result<(), String> {
    if name.is_empty() || name.chars().count() > 31 || name.contains(['[', ']', ':', '*', '?', '/', '\\']) || name.starts_with('\'') {
        return Err(format!("“{name}” cannot name a sheet: up to 31 characters, none of [ ] : * ? / \\"));
    }
    match wb.sheet_index(name) {
        Some(i) if Some(i) != except => Err(format!("there is a sheet called {name} already")),
        _ => Ok(()),
    }
}

/// The cells of a rectangle as they are drawn: all of them, or `only`
/// those — the ones a change moved, which go even when they are now empty
/// so the window clears them.
fn cells_event(open: &Open, sheet: usize, rows: [u32; 2], cols: [u32; 2], only: Option<&[CellRef]>) -> Event {
    let wb = open.book.workbook();
    let s = &wb.sheets[sheet];
    let view = |at: CellRef| -> SheetCellView {
        let cell = s.cells.get(&at);
        let style = cell.map_or(0, |c| c.style);
        let input = cell.map_or(String::new(), |c| c.input.clone());
        let value = open.book.value(sheet, at);
        let format = wb.styles.get(style as usize).map_or("", |st| st.format.as_str());
        let shown = numfmt::show(&value, format);
        let kind = match value {
            Value::Number(_) => "n",
            Value::Text(_) => "t",
            Value::Bool(_) => "b",
            Value::Error(_) => "e",
            Value::Empty => "",
        };
        SheetCellView { row: at.row, col: at.col, text: shown.text, input, style, kind: kind.into(), color: shown.color.map(str::to_string) }
    };
    let cells = match only {
        Some(list) => list.iter().map(|at| view(*at)).collect(),
        None => {
            let from = CellRef::new(rows[0], 0);
            let to = CellRef::new(rows[1], proton_sheet::refs::MAX_COL);
            // The frozen rows and columns are on screen wherever it is scrolled.
            let mut out: Vec<SheetCellView> = Vec::new();
            if s.frozen.rows > 0 && rows[0] > 0 {
                let top = CellRef::new(s.frozen.rows.min(rows[0]) - 1, proton_sheet::refs::MAX_COL);
                out.extend(s.cells.range(CellRef::new(0, 0)..=top).filter(|(at, _)| in_view(**at, rows, cols, s.frozen)).map(|(at, _)| view(*at)));
            }
            out.extend(s.cells.range(from..=to).filter(|(at, _)| in_view(**at, rows, cols, s.frozen)).map(|(at, _)| view(*at)));
            out
        }
    };
    Event::SheetCells { generation: open.generation, version: open.version, sheet: sheet as u32, rows, cols, cells, partial: only.is_some() }
}

/// Whether a cell is in the rectangle asked for, or in a frozen row or
/// column beside it.
fn in_view(at: CellRef, rows: [u32; 2], cols: [u32; 2], frozen: proton_sheet::model::Frozen) -> bool {
    let row = (rows[0]..=rows[1]).contains(&at.row) || at.row < frozen.rows;
    let col = (cols[0]..=cols[1]).contains(&at.col) || at.col < frozen.cols;
    row && col
}

/// A workbook read from what it is kept as, and what it held that was not kept.
fn load(path: &Path) -> Result<(Workbook, Vec<String>), String> {
    match SheetFileKind::of(path) {
        Some(SheetFileKind::Folder) => proton_sheet::folder::read(path).map(|wb| (wb, Vec::new())),
        Some(SheetFileKind::Xlsx) => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            proton_sheet::xlsx::read(&bytes).map(|r| (r.workbook, r.dropped))
        }
        Some(kind @ (SheetFileKind::Csv | SheetFileKind::Tsv)) => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let text = String::from_utf8_lossy(&bytes);
            let mut wb = Workbook::default();
            wb.sheets[0].name = path.file_stem().map(|s| s.to_string_lossy().chars().take(31).collect()).unwrap_or_else(|| "Sheet1".into());
            wb.sheets[0].cells = proton_sheet::csv::read(&text, if kind == SheetFileKind::Tsv { '\t' } else { ',' });
            Ok((wb, Vec::new()))
        }
        None => Err(format!("{} is not a workbook", path.display())),
    }
}

fn save(open: &Open) -> Result<(), String> {
    match open.kind {
        SheetFileKind::Folder => proton_sheet::folder::write(&open.path, open.book.workbook()),
        SheetFileKind::Xlsx => put(&open.path, &proton_sheet::xlsx::write(&open.book)?),
        SheetFileKind::Csv | SheetFileKind::Tsv => {
            let delim = if open.kind == SheetFileKind::Tsv { '\t' } else { ',' };
            put(&open.path, proton_sheet::csv::write(&open.book, 0, delim).as_bytes())
        }
    }
}

/// Bytes into a file whole or not at all.
fn put(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let partial = path.with_file_name(format!(".{name}.part"));
    std::fs::write(&partial, bytes).map_err(|e| format!("{}: {e}", partial.display()))?;
    std::fs::rename(&partial, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A workbook's path in the project, refused when it leads outside or is
/// not a workbook.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let named = Path::new(rel);
    if rel.is_empty() || !named.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{rel} is not in the project"));
    }
    if SheetFileKind::of(named).is_none() {
        return Err(format!("{rel} is not a workbook: a workbook is a .{EXTENSION} folder, .xlsx, .csv or .tsv"));
    }
    let base = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let path = base.join(named);
    let mut held = path.as_path();
    let real = loop {
        match std::fs::canonicalize(held) {
            Ok(real) => break real,
            Err(_) => match held.parent() {
                Some(up) => held = up,
                None => return Err(format!("{rel} is nowhere")),
            },
        }
    };
    if !real.starts_with(&base) {
        return Err(format!("{rel} leads outside the project"));
    }
    Ok(path)
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "workbook".into()
    } else {
        out
    }
}

pub fn home_event(root: &Path) -> Event {
    Event::SheetHome { workbooks: find(root) }
}

/// The workbooks in a project, three folders deep: `.sheet` folders and
/// `.xlsx`, `.csv` and `.tsv` files.
fn find(root: &Path) -> Vec<SheetFound> {
    const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor"];
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut looked = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            looked += 1;
            if looked > 5000 || out.len() >= 500 {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.starts_with("~$") || SKIP.contains(&name.as_str()) {
                continue;
            }
            let is_dir = path.is_dir();
            match SheetFileKind::of(&path) {
                Some(SheetFileKind::Folder) if is_dir => {}
                Some(kind) if !is_dir && kind != SheetFileKind::Folder => {}
                _ => {
                    if is_dir && depth < 3 {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
            }
            out.push(SheetFound {
                path: path.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default(),
                name: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                kind: SheetFileKind::of(&path).unwrap_or(SheetFileKind::Folder),
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// The cells of a workbook kept on disk, for tests to read back.
#[cfg(test)]
pub(crate) fn inputs(path: &Path) -> std::collections::BTreeMap<String, String> {
    let (wb, _) = load(path).expect("a workbook");
    wb.sheets[0].cells.iter().map(|(at, c)| (at.name(), c.input.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-core-sheet-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn book_of(events: &[Event]) -> Option<SheetBookView> {
        events.iter().rev().find_map(|e| match e {
            Event::SheetBook { book } => Some(book.clone()),
            _ => None,
        })?
    }

    fn cells_of(events: &[Event]) -> Vec<SheetCellView> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::SheetCells { cells, .. } => Some(cells.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn a_workbook_is_made_edited_undone_and_saved_and_the_quit_prompt_names_it() {
        let root = scratch("made");
        let mut sheets = Sheets::default();
        let made = sheets.apply(&root, Command::SheetNew { name: "Q1 Budget".into(), folder: None });
        let book = book_of(&made).unwrap();
        assert_eq!((book.path.as_str(), book.kind, book.dirty), ("q1-budget.sheet", SheetFileKind::Folder, false));
        assert!(root.join("q1-budget.sheet/workbook.json").exists());
        let g = book.generation;
        let view = sheets.apply(&root, Command::SheetView { generation: g, sheet: 0, rows: [0, 40], cols: [0, 10] });
        assert!(cells_of(&view).is_empty());

        let set = sheets.apply(&root, Command::SheetSet { generation: g, sheet: 0, cells: vec![("A1".into(), "2".into()), ("A2".into(), "=A1*21".into())] });
        let shown: Vec<(String, String)> = cells_of(&set).into_iter().map(|c| (CellRef::new(c.row, c.col).name(), c.text)).collect();
        assert_eq!(shown, vec![("A1".to_string(), "2".to_string()), ("A2".to_string(), "42".to_string())]);
        assert!(book_of(&set).unwrap().dirty);
        assert_eq!(sheets.unsaved(), vec!["q1-budget.sheet"]);

        let set = sheets.apply(&root, Command::SheetSet { generation: g, sheet: 0, cells: vec![("A1".into(), "3".into())] });
        assert_eq!(cells_of(&set).iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), vec!["3", "63"], "what depends on it comes too");
        let undo = sheets.apply(&root, Command::SheetUndo);
        assert_eq!(cells_of(&undo).iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), vec!["2", "42"]);
        let saved = sheets.apply(&root, Command::SheetSave);
        assert!(!book_of(&saved).unwrap().dirty);
        assert!(sheets.unsaved().is_empty());
        assert_eq!(inputs(&root.join("q1-budget.sheet")), std::collections::BTreeMap::from([("A1".to_string(), "2".to_string()), ("A2".to_string(), "=A1*21".to_string())]));
        let redo = sheets.apply(&root, Command::SheetRedo);
        assert!(book_of(&redo).unwrap().dirty, "redo moves away from what was saved");
        sheets.apply(&root, Command::SheetUndo);
        assert!(sheets.unsaved().is_empty(), "and undo comes back to it");

        let wrong = sheets.apply(&root, Command::SheetSet { generation: g + 7, sheet: 0, cells: vec![("A1".into(), "9".into())] });
        assert!(matches!(&wrong[0], Event::Notice { .. }), "an edit for another workbook is refused");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sheets_are_added_renamed_and_removed_as_steps() {
        let root = scratch("tabs");
        let mut sheets = Sheets::default();
        sheets.apply(&root, Command::SheetNew { name: "Tabs".into(), folder: None });
        let added = sheets.apply(&root, Command::SheetAddSheet { name: None });
        let b = book_of(&added).unwrap();
        assert_eq!(b.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Sheet1", "Sheet2"]);
        assert_eq!(b.active, 1);
        let refused = sheets.apply(&root, Command::SheetRenameSheet { sheet: 1, name: "sheet1".into() });
        assert!(matches!(&refused[0], Event::Notice { .. }), "names are unique without regard to case");
        let renamed = sheets.apply(&root, Command::SheetRenameSheet { sheet: 1, name: "Data".into() });
        assert_eq!(book_of(&renamed).unwrap().sheets[1].name, "Data");
        let g = book_of(&renamed).unwrap().generation;
        sheets.apply(&root, Command::SheetSet { generation: g, sheet: 0, cells: vec![("A1".into(), "=Data!A1+1".into())] });
        sheets.apply(&root, Command::SheetSet { generation: g, sheet: 1, cells: vec![("A1".into(), "41".into())] });
        let view = sheets.apply(&root, Command::SheetView { generation: g, sheet: 0, rows: [0, 9], cols: [0, 9] });
        assert_eq!(cells_of(&view)[0].text, "42");
        let removed = sheets.apply(&root, Command::SheetRemoveSheet { sheet: 1 });
        assert_eq!(book_of(&removed).unwrap().sheets.len(), 1);
        assert_eq!(cells_of(&removed)[0].text, "#REF!");
        let undone = sheets.apply(&root, Command::SheetUndo);
        assert_eq!(book_of(&undone).unwrap().sheets.len(), 2);
        assert_eq!(cells_of(&undone)[0].text, "42", "the sheet comes back with its cells");
        let last = sheets.apply(&root, Command::SheetRemoveSheet { sheet: 0 });
        sheets.apply(&root, Command::SheetRemoveSheet { sheet: 0 });
        let refused = sheets.apply(&root, Command::SheetRemoveSheet { sheet: 0 });
        assert!(book_of(&last).is_some() && matches!(&refused[0], Event::Notice { .. }), "the last sheet stays");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn copying_pasting_filling_rows_and_formats_are_each_one_step() {
        let root = scratch("edit");
        let mut sheets = Sheets::default();
        let g = book_of(&sheets.apply(&root, Command::SheetNew { name: "Edit".into(), folder: None })).unwrap().generation;
        sheets.apply(&root, Command::SheetView { generation: g, sheet: 0, rows: [0, 40], cols: [0, 10] });
        let set = |cells: &[(&str, &str)]| Command::SheetSet { generation: g, sheet: 0, cells: cells.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
        sheets.apply(&root, set(&[("A1", "1"), ("A2", "2"), ("B1", "=A1*10"), ("C1", "a\tb")]));
        let input = |s: &Sheets, at: &str| cell_at(&s.open.as_ref().unwrap().book, 0, CellRef::parse(at).unwrap()).input;
        let shown = |s: &Sheets, at: &str| s.open.as_ref().unwrap().book.value(0, CellRef::parse(at).unwrap());

        // Copy: the values as they show, a tab between, a line a row.
        let clip = sheets.apply(&root, Command::SheetCopy { generation: g, sheet: 0, range: "A1:C2".into(), cut: false });
        let Event::SheetClip { text } = &clip[0] else { panic!("{clip:?}") };
        assert_eq!(text, "1\t10\ta b\n2\n");
        // Pasted here, its formulas move with it.
        sheets.apply(&root, Command::SheetPaste { generation: g, sheet: 0, at: "E5".into(), text: text.clone() });
        assert_eq!((input(&sheets, "F5"), shown(&sheets, "F5")), ("=E5*10".to_string(), Value::Number(10.0)));
        // From elsewhere, it is typed in.
        sheets.apply(&root, Command::SheetPaste { generation: g, sheet: 0, at: "H1".into(), text: "x\t3\n4\r\n".into() });
        assert_eq!((input(&sheets, "H1"), input(&sheets, "I1"), input(&sheets, "H2")), ("x".into(), "3".into(), "4".into()));
        // A cut moves, once.
        sheets.apply(&root, Command::SheetCopy { generation: g, sheet: 0, range: "H1:I1".into(), cut: true });
        let cut = sheets.open.as_ref().unwrap().clip.as_ref().unwrap().text.clone();
        sheets.apply(&root, Command::SheetPaste { generation: g, sheet: 0, at: "H10".into(), text: cut });
        assert_eq!((input(&sheets, "H1"), input(&sheets, "H10"), input(&sheets, "I10")), (String::new(), "x".into(), "3".into()));

        // Fill: a series down, a formula across.
        sheets.apply(&root, Command::SheetFill { generation: g, sheet: 0, from: "A1:A2".into(), to: "A1:A5".into() });
        assert_eq!(input(&sheets, "A5"), "5");
        sheets.apply(&root, Command::SheetFill { generation: g, sheet: 0, from: "B1".into(), to: "B1:B3".into() });
        assert_eq!((input(&sheets, "B3"), shown(&sheets, "B3")), ("=A3*10".to_string(), Value::Number(30.0)));

        // A row put in above moves everything below, formulas and all.
        sheets.apply(&root, Command::SheetInsert { generation: g, sheet: 0, cols: false, at: 0, n: 1 });
        assert_eq!((input(&sheets, "B2"), shown(&sheets, "B4")), ("=A2*10".to_string(), Value::Number(30.0)));
        sheets.apply(&root, Command::SheetUndo);
        assert_eq!(input(&sheets, "B1"), "=A1*10");

        // Formats change only what they say, and undo takes them back.
        sheets.apply(&root, Command::SheetFormat { generation: g, sheet: 0, range: "A1:B1".into(), patch: Box::new(SheetStylePatch { bold: Some(true), ..Default::default() }) });
        let f = sheets.apply(&root, Command::SheetFormat { generation: g, sheet: 0, range: "A1".into(), patch: Box::new(SheetStylePatch { format: Some("0.00".into()), ..Default::default() }) });
        let a1 = cells_of(&f).into_iter().find(|c| c.row == 0 && c.col == 0).unwrap();
        let styles = book_of(&f).unwrap().styles;
        assert!(styles[a1.style as usize].bold && styles[a1.style as usize].format == "0.00" && a1.text == "1.00", "{a1:?}");
        sheets.apply(&root, Command::SheetUndo);
        sheets.apply(&root, Command::SheetUndo);
        assert_eq!(cell_at(&sheets.open.as_ref().unwrap().book, 0, CellRef::new(0, 0)).style, 0);

        // Widths and frozen panes are the sheet's, and steps too.
        let w = sheets.apply(&root, Command::SheetWidth { generation: g, sheet: 0, col: 2, width: Some(120.0) });
        assert_eq!(book_of(&w).unwrap().sheets[0].widths, vec![(2, 120.0)]);
        let fr = sheets.apply(&root, Command::SheetFreeze { generation: g, sheet: 0, rows: 1, cols: 0 });
        assert_eq!(book_of(&fr).unwrap().sheets[0].frozen, [1, 0]);
        sheets.apply(&root, Command::SheetSave);
        let back = proton_sheet::folder::read(&root.join("edit.sheet")).unwrap();
        assert_eq!((back.sheets[0].widths.get(&2), back.sheets[0].frozen.rows), (Some(&120.0), 1));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn xlsx_and_csv_are_opened_and_saved_back_as_what_they_were() {
        let root = scratch("files");
        std::fs::write(root.join("people.csv"), "name,age\nAda,36\n\"Smith, J\",41\n").unwrap();
        let mut sheets = Sheets::default();
        let home = sheets.events(&root);
        assert!(matches!(&home[0], Event::SheetHome { workbooks } if workbooks.len() == 1 && workbooks[0].kind == SheetFileKind::Csv));
        let opened = sheets.apply(&root, Command::SheetOpen { path: "people.csv".into() });
        let b = book_of(&opened).unwrap();
        assert_eq!((b.kind, b.sheets[0].name.as_str(), b.sheets[0].rows), (SheetFileKind::Csv, "people", 3));
        sheets.apply(&root, Command::SheetSet { generation: b.generation, sheet: 0, cells: vec![("C1".into(), "next".into()), ("C2".into(), "=B2+1".into())] });
        sheets.apply(&root, Command::SheetSave);
        assert_eq!(std::fs::read_to_string(root.join("people.csv")).unwrap(), "name,age,next\nAda,36,37\n\"Smith, J\",41,\n");

        sheets.apply(&root, Command::SheetClose { discard: false });
        sheets.apply(&root, Command::SheetNew { name: "Out".into(), folder: Some("out.xlsx".into()) });
        let b = book_of(&sheets.events(&root)).unwrap();
        assert_eq!(b.kind, SheetFileKind::Xlsx);
        sheets.apply(&root, Command::SheetSet { generation: b.generation, sheet: 0, cells: vec![("B2".into(), "=6*7".into())] });
        sheets.apply(&root, Command::SheetSave);
        assert_eq!(inputs(&root.join("out.xlsx")).get("B2").map(String::as_str), Some("=6*7"));
        assert!(matches!(&sheets.apply(&root, Command::SheetOpen { path: "../x.sheet".into() })[0], Event::Notice { .. }));
        assert!(matches!(&sheets.apply(&root, Command::SheetOpen { path: "notes.txt".into() })[0], Event::Notice { .. }));
        let _ = std::fs::remove_dir_all(root);
    }
}
