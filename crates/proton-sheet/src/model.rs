//! A workbook as it is kept: its sheets, each cell as it was typed, and how
//! cells are drawn. What formulas come to is the engine's (`book`), not kept.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::refs::CellRef;

/// One cell as it was typed: a value, or a formula starting with `=`, and
/// its style's number in the workbook's table (0 is the default).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cell {
    pub input: String,
    pub style: u32,
}

/// How a cell is drawn. Everything left out is the default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    /// The text's colour, `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The cell's colour, `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// A number format, as Excel writes them: `0.00`, `#,##0`, `yyyy-mm-dd`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub format: String,
    /// `left`, `center` or `right`; numbers right and text left otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wrap: bool,
}

/// Rows and columns that stay put while the rest scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Frozen {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rows: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cols: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A filter over a range: its first row the headers, and for the columns
/// that have one, what a row must have there to be shown.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Filter {
    /// `A1:D99`.
    pub range: String,
    /// By column letters.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub columns: BTreeMap<String, Condition>,
}

/// What a filtered column lets through: the values ticked (as they show),
/// and a condition as COUNTIF writes one (`>10`, `ap*`); both, when both.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Condition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub criteria: String,
}

/// When a conditional colour applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "when", rename_all = "camelCase")]
pub enum When {
    Greater { value: f64 },
    Less { value: f64 },
    Between { low: f64, high: f64 },
    /// The same as this, without regard to case.
    Equal { value: String },
    Contains { text: String },
    /// A value more than one cell of the range holds.
    Duplicates,
    Top { n: u32 },
    Bottom { n: u32 },
    /// Every number coloured between two colours by where it falls.
    Scale { low: String, high: String },
}

/// A conditional colour: over a range, when something holds, a fill, a
/// text colour, bold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub range: String,
    #[serde(flatten)]
    pub when: When,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
}

/// What kind of chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartKind {
    #[default]
    Line,
    Bar,
    Scatter,
    Pie,
}

/// A chart over a range: its first row the series' names when it is text,
/// its first column the categories (or a scatter's x). Placed with its top
/// left at a cell, and sized in pixels.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Chart {
    pub kind: ChartKind,
    pub range: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// The cell its top left corner is on.
    pub at: String,
    pub width: u32,
    pub height: u32,
}

/// One sheet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sheet {
    /// What its file is called; kept when the sheet is renamed, so a
    /// rename is one line of `workbook.json`.
    pub id: String,
    pub name: String,
    pub cells: BTreeMap<CellRef, Cell>,
    /// Column widths in points, where they are not the default.
    pub widths: BTreeMap<u32, f64>,
    pub frozen: Frozen,
    pub filter: Option<Filter>,
    /// Conditional colours, the first that applies to a cell winning.
    pub rules: Vec<Rule>,
    pub charts: Vec<Chart>,
}

impl Sheet {
    pub fn new(id: &str, name: &str) -> Sheet {
        Sheet { id: id.into(), name: name.into(), ..Default::default() }
    }

    /// The smallest rectangle holding every cell with anything in it.
    pub fn used(&self) -> Option<crate::refs::Range> {
        let first = self.cells.keys().next()?;
        let (mut r0, mut c0, mut r1, mut c1) = (first.row, first.col, first.row, first.col);
        for c in self.cells.keys() {
            r0 = r0.min(c.row);
            r1 = r1.max(c.row);
            c0 = c0.min(c.col);
            c1 = c1.max(c.col);
        }
        Some(crate::refs::Range::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
    }
}

/// A workbook: its sheets in order, the styles its cells name, and its
/// defined names.
#[derive(Debug, Clone, PartialEq)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
    /// Style 0 is the default and always there.
    pub styles: Vec<Style>,
    /// `Total` → `Sheet1!$B$9`: names a formula may use for a reference.
    pub names: BTreeMap<String, String>,
}

impl Default for Workbook {
    fn default() -> Self {
        Workbook { sheets: vec![Sheet::new("s1", "Sheet1")], styles: vec![Style::default()], names: BTreeMap::new() }
    }
}

impl Workbook {
    /// The sheet called `name`, without regard to case, as Excel finds one.
    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The style's number, added to the table if it is new.
    pub fn style_id(&mut self, style: &Style) -> u32 {
        match self.styles.iter().position(|s| s == style) {
            Some(i) => i as u32,
            None => {
                self.styles.push(style.clone());
                (self.styles.len() - 1) as u32
            }
        }
    }

    /// A sheet's id no other sheet has.
    pub fn fresh_id(&self) -> String {
        (1..).map(|n| format!("s{n}")).find(|id| !self.sheets.iter().any(|s| &s.id == id)).expect("an id")
    }
}
