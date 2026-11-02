//! Finding text in a sheet and replacing it, by the rule Codex's `/` keeps:
//! literal, and with Vim's smartcase — a search with a capital in it is
//! case-sensitive, one without is not.
//!
//! A cell is found by what it holds, as typed, or by what it shows: `SUM`
//! finds the formula, `1,998.95` what it comes to. Only what a cell holds can
//! be replaced, so one found by what it shows alone is said to be. Rows a
//! filter hides are counted and not found, and so not replaced, as Excel's
//! replace leaves them.

use crate::arrange;
use crate::book::Book;
use crate::numfmt;
use crate::refs::{CellRef, Range, MAX_COL};
use crate::value::Value;

/// What is looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    text: String,
    /// A capital in it: letters must match as written.
    exact: bool,
    /// All of what a cell holds or shows, not a part of it.
    whole: bool,
}

impl Query {
    pub fn new(text: &str, whole: bool) -> Query {
        Query { text: text.to_string(), exact: text.chars().any(char::is_uppercase), whole }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Whether a number in General format could show it.
    fn could_be_number(&self) -> bool {
        self.text.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'E' | 'e'))
    }

    /// How many bytes of `hay` it takes up from the start, if it is there.
    /// Letters are compared one by one, so what is counted is `hay`'s own
    /// length whatever folding them would do to it.
    fn at_start(&self, hay: &str) -> Option<usize> {
        let mut got = hay.char_indices();
        for want in self.text.chars() {
            let (_, c) = got.next()?;
            let same = want == c
                || (!self.exact
                    && if want.is_ascii() && c.is_ascii() { want.eq_ignore_ascii_case(&c) } else { want.to_lowercase().eq(c.to_lowercase()) });
            if !same {
                return None;
            }
        }
        Some(got.next().map_or(hay.len(), |(i, _)| i))
    }

    /// Whether `hay` has it.
    pub fn finds(&self, hay: &str) -> bool {
        if self.text.is_empty() {
            return false;
        }
        if self.whole {
            return self.at_start(hay) == Some(hay.len());
        }
        hay.char_indices().any(|(i, _)| self.at_start(&hay[i..]).is_some())
    }

    /// `input` with every place it has this replaced, or None if it has none.
    pub fn replaced(&self, input: &str, with: &str) -> Option<String> {
        if !self.finds(input) {
            return None;
        }
        if self.whole {
            return Some(with.to_string());
        }
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(c) = rest.chars().next() {
            if let Some(n) = self.at_start(rest) {
                out.push_str(with);
                rest = &rest[n..];
            } else {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        Some(out)
    }
}

/// A cell found: `held` when what it holds has the text, not only what it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub at: CellRef,
    pub held: bool,
}

/// What a search came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    /// Row by row, as many as were asked for.
    pub hits: Vec<Hit>,
    /// All those found, however many were asked for.
    pub total: u32,
    /// Of those, the ones that only show it.
    pub shown_only: u32,
    /// Cells that have it in rows a filter hides, which are not in `total`.
    pub hidden: u32,
}

/// Every cell of a sheet, or of `within`, that has the text.
pub fn search(book: &Book, sheet: usize, within: Option<Range>, query: &Query, most: usize) -> Search {
    let mut out = Search::default();
    let wb = book.workbook();
    let Some(s) = wb.sheets.get(sheet) else { return out };
    if query.is_empty() {
        return out;
    }
    let hidden = arrange::hidden(book, sheet);
    let mut runs = hidden.iter().peekable();
    let from = within.map_or(CellRef::new(0, 0), |r| CellRef::new(r.start.row, 0));
    let to = within.map_or(CellRef::new(u32::MAX, MAX_COL), |r| CellRef::new(r.end.row, MAX_COL));
    for (at, cell) in s.cells.range(from..=to) {
        if within.is_some_and(|r| !r.contains(*at)) {
            continue;
        }
        let held = query.finds(&cell.input);
        let found = held || {
            let format = wb.styles.get(cell.style as usize).map_or("", |st| st.format.as_str());
            let value = book.value(sheet, *at);
            // Writing out a number is most of the time a long search takes,
            // and without a format it can only show these.
            let skip = matches!(value, Value::Number(n) if n.is_finite()) && format.is_empty() && !query.could_be_number();
            !skip && query.finds(&numfmt::show(&value, format).text)
        };
        if !found {
            continue;
        }
        // Cells come row by row, so the hidden runs are only ever passed.
        while runs.next_if(|run| run[1] < at.row).is_some() {}
        if runs.peek().is_some_and(|run| run[0] <= at.row) {
            out.hidden += 1;
            continue;
        }
        out.total += 1;
        out.shown_only += u32::from(!held);
        if out.hits.len() < most {
            out.hits.push(Hit { at: *at, held });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cell, Condition, Filter, Style, Workbook};

    fn book(cells: &[(&str, &str)]) -> Book {
        let mut wb = Workbook::default();
        for (at, input) in cells {
            wb.sheets[0].cells.insert(CellRef::parse(at).unwrap(), Cell { input: input.to_string(), style: 0 });
        }
        Book::new(wb)
    }

    fn names(s: &Search) -> Vec<String> {
        s.hits.iter().map(|h| h.at.name()).collect()
    }

    #[test]
    fn a_search_is_literal_and_a_capital_makes_it_exact() {
        let lower = Query::new("rent", false);
        assert!(lower.finds("Rent") && lower.finds("PARENTS") && lower.finds("rent"));
        let capital = Query::new("Rent", false);
        assert!(capital.finds("Rent") && !capital.finds("rent") && !capital.finds("PARENTS"));
        assert!(Query::new("a.c", false).finds("a.c") && !Query::new("a.c", false).finds("abc"), "no patterns");
        assert!(Query::new("é", false).finds("CAFÉ"), "letters past ASCII fold too");
        let whole = Query::new("rent", true);
        assert!(whole.finds("RENT") && !whole.finds("rents"));
        assert!(!Query::new("", false).finds("anything"));
    }

    #[test]
    fn replacing_keeps_everything_else_and_every_place_it_is_found() {
        let q = Query::new("a", false);
        assert_eq!(q.replaced("banAna", "oo").as_deref(), Some("boonoonoo"));
        assert_eq!(q.replaced("xyz", "oo"), None);
        assert_eq!(Query::new("aa", false).replaced("aaaaa", "b").as_deref(), Some("bba"), "places do not overlap");
        assert_eq!(Query::new("a", false).replaced("a", "aa").as_deref(), Some("aa"), "what is put in is not looked in");
        // İ lowers to two characters: the length used is the cell's own.
        assert_eq!(Query::new("x", false).replaced("İxİ", "y").as_deref(), Some("İyİ"));
        assert_eq!(Query::new("cost", true).replaced("Cost", "Price").as_deref(), Some("Price"));
        assert_eq!(Query::new("cost", true).replaced("Costs", "Price"), None);
    }

    #[test]
    fn a_cell_is_found_by_what_it_holds_or_what_it_shows_row_by_row() {
        let mut b = book(&[("C1", "rent"), ("A2", "=SUM(B2:B3)"), ("B2", "1450"), ("B3", "50"), ("A1", "Rent")]);
        let one = search(&b, 0, None, &Query::new("rent", false), 100);
        assert_eq!(names(&one), ["A1", "C1"], "by row, then by column");
        assert_eq!((one.total, one.shown_only), (2, 0));

        let formula = search(&b, 0, None, &Query::new("sum", false), 100);
        assert_eq!(names(&formula), ["A2"]);
        assert!(formula.hits[0].held);

        let value = search(&b, 0, None, &Query::new("1500", false), 100);
        assert_eq!(names(&value), ["A2"], "what the formula comes to");
        assert_eq!((value.hits[0].held, value.shown_only), (false, 1));

        // A number format's text is found, as what the cell shows.
        let style = b.workbook_mut().style_id(&Style { format: "#,##0.00".into(), ..Style::default() });
        b.workbook_mut().sheets[0].cells.get_mut(&CellRef::parse("B2").unwrap()).unwrap().style = style;
        let shown = search(&b, 0, None, &Query::new("1,450.00", false), 100);
        assert_eq!(names(&shown), ["B2"]);
        assert!(!shown.hits[0].held);
    }

    #[test]
    fn a_search_keeps_to_a_range_and_says_how_many_past_what_was_asked_for() {
        let b = book(&[("A1", "x"), ("B1", "x"), ("C1", "x"), ("A2", "x"), ("B2", "x"), ("C2", "x"), ("B3", "x")]);
        let q = Query::new("x", false);
        let inside = search(&b, 0, Some(Range::parse("B1:C2").unwrap()), &q, 100);
        assert_eq!(names(&inside), ["B1", "C1", "B2", "C2"]);
        let few = search(&b, 0, None, &q, 2);
        assert_eq!(few.hits.len(), 2);
        assert_eq!(few.total, 7);
        assert_eq!(search(&b, 0, None, &Query::new("", false), 100), Search::default());
        assert_eq!(search(&b, 9, None, &q, 100), Search::default(), "no such sheet");
    }

    #[test]
    fn rows_a_filter_hides_are_counted_and_not_found() {
        let mut wb = Workbook::default();
        for (at, input) in [("A1", "Name"), ("B1", "Keep"), ("A2", "pear"), ("B2", "no"), ("A3", "pear"), ("B3", "yes"), ("A4", "pear"), ("B4", "no"), ("A5", "pear"), ("B5", "yes")] {
            wb.sheets[0].cells.insert(CellRef::parse(at).unwrap(), Cell { input: input.into(), style: 0 });
        }
        let mut filter = Filter { range: "A1:B5".into(), ..Filter::default() };
        filter.columns.insert("B".into(), Condition { values: Some(vec!["yes".into()]), criteria: String::new() });
        wb.sheets[0].filter = Some(filter);
        let found = search(&Book::new(wb), 0, None, &Query::new("pear", false), 100);
        assert_eq!(names(&found), ["A3", "A5"]);
        assert_eq!((found.total, found.hidden), (2, 2));
    }
}
