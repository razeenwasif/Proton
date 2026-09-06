//! Tables, out of the HTML a kernel sends for one.
//!
//! A dataframe arrives in the MIME bundle as `text/html` — a `<table>` — and
//! as `text/plain`, a fixed-width block. Neither is a table Proton can style,
//! search, or draw with its own borders.
//!
//! **The HTML is not put in the page.** It is a string a kernel produced, and
//! a kernel runs whatever code the notebook contains; injecting it into the
//! window would mean the editor's chrome is one `<style>` or one `<script>`
//! away from whatever a downloaded notebook felt like doing. So the rows come
//! out here and the front end draws its own table from them — which also means
//! the table is themed, and can be selected and copied like anything else.
//!
//! This is a scanner for the shape a table has, not an HTML parser. It looks
//! for `<table>`, then rows, then cells, and takes the text out of each. That
//! is enough for what pandas, polars and `to_html` produce, and it cannot be
//! surprised into executing anything, because it never produces markup — only
//! strings.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A table, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Table {
    /// One per column. A dataframe's first heading is the index's, and pandas
    /// leaves it empty — which is worth keeping rather than dropping, because
    /// the blank is what lines the index up over its own column.
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// How many rows the table said it had, when it says. Pandas prints
    /// `3 rows × 4 columns` under a frame it truncated, and a viewer that
    /// silently showed the first thirty of a million would be lying.
    pub total: Option<usize>,
}

impl Table {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.columns.is_empty()
    }
}

/// Pull a table out of a fragment of HTML, if there is one in it.
pub fn from_html(html: &str) -> Option<Table> {
    let start = find_ci(html, "<table")?;
    let body = &html[start..];
    let end = find_ci(body, "</table>").map_or(body.len(), |at| at + "</table>".len());
    let table = &body[..end];

    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();

    for row in rows_of(table) {
        let cells = cells_of(row.text);
        if cells.is_empty() {
            continue;
        }
        if row.heading {
            // The last header row rather than the first: a frame with a
            // MultiIndex has one row per level, and the innermost is the one
            // whose names line up with the columns underneath.
            columns = cells;
        } else {
            rows.push(cells);
        }
    }

    // A table with no `<thead>` — `to_html(header=False)`, or hand-written —
    // still has columns; they are just the width of the first row.
    if columns.is_empty() && !rows.is_empty() {
        columns = vec![String::new(); rows[0].len()];
    }
    if columns.is_empty() && rows.is_empty() {
        return None;
    }

    Some(Table {
        columns,
        rows,
        total: total_of(html),
    })
}

/// `<p>3 rows × 4 columns</p>`, which pandas adds when it truncated the frame.
fn total_of(html: &str) -> Option<usize> {
    let at = html.find(" rows ×")?;
    let before = &html[..at];
    let start = before.rfind(|c: char| !c.is_ascii_digit() && c != ',')? + 1;
    before[start..].replace(',', "").parse().ok()
}

struct Row<'a> {
    text: &'a str,
    heading: bool,
}

/// Every `<tr>`, and whether it was inside a `<thead>`.
fn rows_of(table: &str) -> Vec<Row<'_>> {
    let head = find_ci(table, "<thead").map(|start| {
        let end = find_ci(&table[start..], "</thead>").map_or(table.len(), |at| start + at);
        start..end
    });

    let mut found = Vec::new();
    let mut at = 0;
    while let Some(open) = find_ci(&table[at..], "<tr") {
        let open = at + open;
        let Some(after) = table[open..].find('>').map(|i| open + i + 1) else {
            break;
        };
        let close = find_ci(&table[after..], "</tr>").map_or(table.len(), |i| after + i);
        found.push(Row {
            text: &table[after..close],
            heading: head.as_ref().is_some_and(|range| range.contains(&open)),
        });
        at = close.max(after);
        if at >= table.len() {
            break;
        }
    }
    found
}

/// Every `<th>` or `<td>` in a row, as text.
fn cells_of(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut at = 0;
    while at < row.len() {
        let th = find_ci(&row[at..], "<th");
        let td = find_ci(&row[at..], "<td");
        let Some(open) = match (th, td) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
        .map(|i| at + i) else {
            break;
        };
        let Some(after) = row[open..].find('>').map(|i| open + i + 1) else {
            break;
        };
        let close = find_ci(&row[after..], "</th>")
            .into_iter()
            .chain(find_ci(&row[after..], "</td>"))
            .min()
            .map_or(row.len(), |i| after + i);
        cells.push(text_of(&row[after..close]));
        at = close.max(after);
    }
    cells
}

/// The text of a fragment: tags dropped, entities decoded, spacing collapsed.
fn text_of(fragment: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in fragment.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = unescape(&out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The five entities that matter, plus numeric ones.
///
/// Not a full table: a cell holding `&hellip;` should read as `&hellip;`
/// rather than as nothing, and guessing at the long tail of named entities is
/// how a decoder starts eating text it did not understand.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix('#')
                .and_then(
                    |n| match n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    },
                )
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `find`, ignoring case, because HTML tags may be written either way.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    lower.find(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exactly what pandas 3.0.5 emits for a three-row frame.
    const PANDAS: &str = r#"<div>
<style scoped>
    .dataframe tbody tr th { vertical-align: top; }
</style>
<table border="1" class="dataframe">
  <thead>
    <tr style="text-align: right;">
      <th></th>
      <th>name</th>
      <th>score</th>
      <th>ok</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <th>0</th>
      <td>ada</td>
      <td>91.50</td>
      <td>True</td>
    </tr>
    <tr>
      <th>1</th>
      <td>alan</td>
      <td>88.00</td>
      <td>False</td>
    </tr>
  </tbody>
</table>
</div>"#;

    #[test]
    fn a_pandas_frame_comes_out_as_columns_and_rows() {
        let table = from_html(PANDAS).expect("a table");
        assert_eq!(table.columns, ["", "name", "score", "ok"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0], ["0", "ada", "91.50", "True"]);
        assert_eq!(table.rows[1], ["1", "alan", "88.00", "False"]);
    }

    #[test]
    fn the_style_block_is_not_mistaken_for_content() {
        // The `<style scoped>` pandas ships sits right before the table, and
        // taking text from it would put CSS in the first row.
        let table = from_html(PANDAS).unwrap();
        assert!(
            !table
                .rows
                .iter()
                .flatten()
                .any(|cell| cell.contains("vertical-align")),
            "{:?}",
            table.rows
        );
    }

    #[test]
    fn nothing_that_came_out_is_markup() {
        // The property the whole module exists for: whatever the kernel sent,
        // what leaves here is text.
        let nasty = r#"<table><tr><td><script>alert(1)</script>ok</td>
            <td onclick="x()">two</td></tr></table>"#;
        let table = from_html(nasty).expect("a table");
        for cell in table.rows.iter().flatten() {
            assert!(!cell.contains('<'), "{cell:?}");
            assert!(!cell.contains("onclick"), "{cell:?}");
        }
        assert_eq!(table.rows[0], ["alert(1)ok", "two"]);
    }

    #[test]
    fn entities_are_decoded_and_unknown_ones_are_left_alone() {
        let html = "<table><tr><td>a &amp; b</td><td>&lt;tag&gt;</td>\
                    <td>&#65;&#x42;</td><td>&hellip;</td></tr></table>";
        let table = from_html(html).unwrap();
        assert_eq!(table.rows[0], ["a & b", "<tag>", "AB", "&hellip;"]);
    }

    #[test]
    fn a_truncated_frame_says_how_many_rows_there_really_were() {
        // Showing the first thirty rows of a million without saying so is the
        // one thing a table viewer must not do.
        let html = format!("{PANDAS}<p>1,048,576 rows × 4 columns</p>");
        assert_eq!(from_html(&html).unwrap().total, Some(1_048_576));
        assert_eq!(from_html(PANDAS).unwrap().total, None);
    }

    #[test]
    fn a_multiindex_uses_the_innermost_header_row() {
        let html = "<table><thead><tr><th>outer</th><th colspan=2>group</th></tr>\
                    <tr><th></th><th>a</th><th>b</th></tr></thead>\
                    <tbody><tr><th>0</th><td>1</td><td>2</td></tr></tbody></table>";
        let table = from_html(html).unwrap();
        assert_eq!(table.columns, ["", "a", "b"]);
        assert_eq!(table.rows[0], ["0", "1", "2"]);
    }

    #[test]
    fn a_table_with_no_header_still_has_rows() {
        let html = "<table><tr><td>1</td><td>2</td></tr></table>";
        let table = from_html(html).unwrap();
        assert_eq!(table.rows[0], ["1", "2"]);
        assert_eq!(table.columns.len(), 2);
    }

    #[test]
    fn html_with_no_table_in_it_is_not_a_table() {
        assert!(from_html("<div>just a picture</div>").is_none());
        assert!(from_html("").is_none());
    }

    #[test]
    fn uppercase_tags_are_found_too() {
        let html = "<TABLE><TR><TD>x</TD></TR></TABLE>";
        assert_eq!(from_html(html).unwrap().rows[0], ["x"]);
    }
}
