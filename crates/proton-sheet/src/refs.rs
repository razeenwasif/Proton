//! Cell references as a person writes them: `B7`, `$B$7`, `B7:D9`, `C:C`,
//! `3:3`. Rows and columns are counted from 0 here and from 1 (and A) on
//! screen.

/// The last row and column a sheet has, as Excel's do.
pub const MAX_ROW: u32 = 1_048_575;
pub const MAX_COL: u32 = 16_383;

/// One cell, 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellRef {
    pub row: u32,
    pub col: u32,
}

impl CellRef {
    pub fn new(row: u32, col: u32) -> CellRef {
        CellRef { row, col }
    }

    /// As it is written: `B7`.
    pub fn name(self) -> String {
        format!("{}{}", col_name(self.col), self.row + 1)
    }

    /// `B7` read, `$`s allowed and ignored.
    pub fn parse(s: &str) -> Option<CellRef> {
        let a = A1::parse(s)?;
        (a.col.is_some() && a.row.is_some()).then(|| CellRef::new(a.row.unwrap().0, a.col.unwrap().0))
    }
}

/// A rectangle of cells, its corners in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    pub start: CellRef,
    pub end: CellRef,
}

impl Range {
    pub fn new(a: CellRef, b: CellRef) -> Range {
        Range {
            start: CellRef::new(a.row.min(b.row), a.col.min(b.col)),
            end: CellRef::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }

    pub fn cell(c: CellRef) -> Range {
        Range { start: c, end: c }
    }

    pub fn contains(&self, c: CellRef) -> bool {
        (self.start.row..=self.end.row).contains(&c.row) && (self.start.col..=self.end.col).contains(&c.col)
    }

    pub fn rows(&self) -> u32 {
        self.end.row - self.start.row + 1
    }

    pub fn cols(&self) -> u32 {
        self.end.col - self.start.col + 1
    }

    /// As it is written: `B7:D9`, `B7` for one cell.
    pub fn name(&self) -> String {
        if self.start == self.end {
            self.start.name()
        } else {
            format!("{}:{}", self.start.name(), self.end.name())
        }
    }

    /// `B7:D9` or `B7` read.
    pub fn parse(s: &str) -> Option<Range> {
        match s.split_once(':') {
            Some((a, b)) => Some(Range::new(CellRef::parse(a)?, CellRef::parse(b)?)),
            None => CellRef::parse(s).map(Range::cell),
        }
    }
}

/// A column's letters: 0 is A, 25 Z, 26 AA.
pub fn col_name(col: u32) -> String {
    let mut n = col + 1;
    let mut out = Vec::new();
    while n > 0 {
        let r = (n - 1) % 26;
        out.push(b'A' + r as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Letters as a column: A is 0. `None` past XFD.
pub fn col_of(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n: u32 = 0;
    for c in letters.chars() {
        let c = c.to_ascii_uppercase();
        if !c.is_ascii_uppercase() {
            return None;
        }
        n = n * 26 + (c as u32 - 'A' as u32 + 1);
    }
    (n >= 1 && n - 1 <= MAX_COL).then(|| n - 1)
}

/// One end of a reference as written: a column, a row, or both, each with
/// whether it is absolute (`$`). `C` and `3` are whole columns and rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct A1 {
    pub col: Option<(u32, bool)>,
    pub row: Option<(u32, bool)>,
}

impl A1 {
    pub fn parse(s: &str) -> Option<A1> {
        let b = s.as_bytes();
        let mut i = 0;
        let col_abs = b.first() == Some(&b'$');
        if col_abs {
            i += 1;
        }
        let letters_start = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        let letters = &s[letters_start..i];
        let col = if letters.is_empty() {
            None
        } else {
            Some((col_of(letters)?, col_abs))
        };
        let row_abs = b.get(i) == Some(&b'$');
        if row_abs {
            i += 1;
        }
        let digits = &s[i..];
        let row = if digits.is_empty() {
            None
        } else {
            if !digits.bytes().all(|c| c.is_ascii_digit()) || digits.starts_with('0') {
                return None;
            }
            let n: u32 = digits.parse().ok()?;
            if n == 0 || n - 1 > MAX_ROW {
                return None;
            }
            Some((n - 1, row_abs))
        };
        if col.is_none() && row.is_none() {
            return None;
        }
        // A `$` with nothing after it, or letters then `$` then nothing.
        if (col_abs && col.is_none()) || (row_abs && row.is_none()) {
            return None;
        }
        Some(A1 { col, row })
    }

    /// As written, `$`s and all.
    pub fn name(&self) -> String {
        let mut s = String::new();
        if let Some((c, abs)) = self.col {
            if abs {
                s.push('$');
            }
            s.push_str(&col_name(c));
        }
        if let Some((r, abs)) = self.row {
            if abs {
                s.push('$');
            }
            s.push_str(&(r + 1).to_string());
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_letters_both_ways() {
        for (n, s) in [(0, "A"), (25, "Z"), (26, "AA"), (51, "AZ"), (52, "BA"), (701, "ZZ"), (702, "AAA"), (16383, "XFD")] {
            assert_eq!(col_name(n), s);
            assert_eq!(col_of(s), Some(n));
        }
        assert_eq!(col_of("XFE"), None);
        assert_eq!(col_of("a"), Some(0));
    }

    #[test]
    fn references_are_read_as_written() {
        assert_eq!(CellRef::parse("B7"), Some(CellRef::new(6, 1)));
        assert_eq!(CellRef::parse("$B$7"), Some(CellRef::new(6, 1)));
        assert_eq!(CellRef::parse("B0"), None);
        assert_eq!(CellRef::parse("B07"), None);
        assert_eq!(CellRef::parse("7"), None);
        assert_eq!(Range::parse("D9:B7"), Some(Range::new(CellRef::new(6, 1), CellRef::new(8, 3))));
        assert_eq!(Range::parse("D9:B7").unwrap().name(), "B7:D9");
        let a = A1::parse("$C").unwrap();
        assert_eq!((a.col, a.row), (Some((2, true)), None));
        let r = A1::parse("12").unwrap();
        assert_eq!((r.col, r.row), (None, Some((11, false))));
        assert_eq!(A1::parse("$").map(|a| a.name()), None);
        assert_eq!(A1::parse("A$1").unwrap().name(), "A$1");
    }
}
