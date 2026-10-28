//! `.csv` and `.tsv`: a sheet's values as delimited text, read with their
//! quoting (RFC 4180's, and its newlines in quotes) and written with it.

use std::collections::BTreeMap;

use crate::book::Book;
use crate::model::Cell;
use crate::refs::CellRef;
use crate::value::{general, Value};

/// The delimiter a file's name says: a tab for `.tsv` and `.tab`, a comma
/// otherwise.
pub fn delimiter_for(path: &std::path::Path) -> char {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("tsv" | "tab") => '\t',
        _ => ',',
    }
}

/// Delimited text as rows of fields.
pub fn rows(text: &str, delim: char) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut at_start = true;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' if at_start => {
                quoted = true;
                at_start = false;
            }
            c if c == delim => {
                row.push(std::mem::take(&mut field));
                at_start = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                row.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut row));
                at_start = true;
            }
            c => {
                field.push(c);
                at_start = false;
            }
        }
    }
    if !field.is_empty() || !row.is_empty() || !at_start {
        row.push(field);
        out.push(row);
    }
    out
}

/// Delimited text as a sheet's cells: each field as if typed, so numbers
/// and dates are read as numbers and dates.
pub fn read(text: &str, delim: char) -> BTreeMap<CellRef, Cell> {
    let mut cells = BTreeMap::new();
    for (r, row) in rows(text, delim).into_iter().enumerate() {
        for (c, field) in row.into_iter().enumerate() {
            if !field.is_empty() && r <= crate::refs::MAX_ROW as usize && c <= crate::refs::MAX_COL as usize {
                cells.insert(CellRef::new(r as u32, c as u32), Cell { input: field, style: 0 });
            }
        }
    }
    cells
}

fn field(s: &str, delim: char) -> String {
    if s.contains([delim, '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A sheet's values as delimited text, from A1 to the last cell in use;
/// formulas as what they come to.
pub fn write(book: &Book, sheet: usize, delim: char) -> String {
    let Some(used) = book.workbook().sheets.get(sheet).and_then(|s| s.used()) else { return String::new() };
    let mut out = String::new();
    for row in 0..=used.end.row {
        let fields: Vec<String> = (0..=used.end.col)
            .map(|col| {
                let text = match book.value(sheet, CellRef::new(row, col)) {
                    Value::Empty => String::new(),
                    Value::Number(n) => general(n),
                    Value::Text(t) => t,
                    Value::Bool(b) => if b { "TRUE" } else { "FALSE" }.into(),
                    Value::Error(e) => e.text().into(),
                };
                field(&text, delim)
            })
            .collect();
        out.push_str(&fields.join(&delim.to_string()));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Workbook;

    #[test]
    fn quoting_is_read_and_written_as_rfc_4180_has_it() {
        let text = "\u{feff}name,note,n\r\n\"Smith, J\",\"said \"\"hi\"\"\nthen left\",3\nplain,,4.5\n\"\",x,\n";
        assert_eq!(
            rows(text, ','),
            vec![
                vec!["name", "note", "n"],
                vec!["Smith, J", "said \"hi\"\nthen left", "3"],
                vec!["plain", "", "4.5"],
                vec!["", "x", ""],
            ]
        );
        let mut wb = Workbook::default();
        wb.sheets[0].cells = read(text, ',');
        let mut book = Book::new(wb);
        book.set(0, CellRef::parse("D2").unwrap(), "=C2*2");
        let back = write(&book, 0, ',');
        assert_eq!(back, "name,note,n,\n\"Smith, J\",\"said \"\"hi\"\"\nthen left\",3,6\nplain,,4.5,\n,x,,\n");
        assert_eq!(rows(&back, ',')[1][1], "said \"hi\"\nthen left", "what is written reads back");
        assert_eq!(rows("a\tb\n1\t2", '\t'), vec![vec!["a", "b"], vec!["1", "2"]]);
        assert_eq!(delimiter_for(std::path::Path::new("x.TSV")), '\t');
    }
}
