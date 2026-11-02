# Excel — Proton's spreadsheet mode

**Built, 2026-10-28: E0–E8.** What each milestone is, as planned, is below;
what was measured is at the end.

A spreadsheet in a tab of its own, after Google Sheets and Excel: workbooks of
sheets, formulas recalculated as you type, sorting, filters, conditional
colour, charts, and an agent that reads and writes ranges. Asked for on
2026-10-27 and named Excel on 2026-10-28.

## Decided

- **The label is "Excel"**, on the rail and the title bar, on **⌃5**. In the
  code it is `sheet` — `ModeId::Sheet`, the crate `proton-sheet`, the core
  module `sheet`, `ui/sheet*.js`, commands `Sheet*` and events `sheet*` —
  as Canvas was `raster`.
- **A workbook is a folder of text that diffs**, `Name.sheet/`, which is
  Proton's rule for every document. `.xlsx`, `.csv` and `.tsv` are opened and
  saved too: an `.xlsx` opened is worked on and saved back as `.xlsx`, or
  kept as a sheet.
- **The formula engine is Proton's own**, in `proton-sheet`, with no new
  crate. `.xlsx` is read and written with `quick-xml` and `flate2`, both
  already in the tree, and a small zip reader and writer of our own.
- **Everything is in the first version:** the grid, formulas and files;
  sorting, filters and conditional colour; charts; and the agent with
  `@sheet` and ranges to and from Data's notebooks.
- **Collaboration is not built:** a workbook is one person's, and two people
  editing one sheet would need a server.

## The format: `Name.sheet/`

```
Name.sheet/
  workbook.json        sheets in order, defined names, styles, charts,
                       conditional rules, filters, frozen panes, widths
  sheets/<id>.cells    one line per cell that has anything in it
  comments.json        comments on cells, by sheet and cell
```

- **`<id>.cells`** is a line per non-empty cell, in row-major order: the
  reference, a tab, what was typed — a value, or a formula starting with `=` —
  and, after another tab, the cell's style number when it has one. Tabs,
  newlines and backslashes in text are escaped. A cell changed is a line
  changed; a row inserted renumbers the lines below it, as it does in the
  sheet.
- **Values are not stored for formulas:** they are worked out again on open,
  so a file cannot disagree with itself.
- **`workbook.json`** is written in the order its types declare, with
  nothing empty said, as Game's documents were, so a diff reads as the change.

## The formula engine (`proton-sheet::formula`)

- **References:** A1 and `$A$1`, ranges (`A1:C9`), whole columns and rows
  (`A:A`, `3:3`), other sheets (`Data!B2`, `'My sheet'!B2`), defined names.
- **Values:** numbers (f64, as Excel's are), text, true and false, empty, and
  errors — `#DIV/0!`, `#N/A`, `#NAME?`, `#NULL!`, `#NUM!`, `#REF!`, `#VALUE!`,
  and a cycle shown as `#CYCLE!` with the cells in it named.
- **Recalculation:** each formula's precedents are kept, with an index from
  each cell and range to the formulas that read it; an edit recalculates what
  depends on it and nothing else, in dependency order. Cycles are found, not
  looped through.
- **Functions,** about a hundred to begin with:
  - Maths: SUM, SUMIF(S), PRODUCT, ROUND(UP/DOWN), INT, MOD, ABS, POWER, SQRT, EXP, LN, LOG(10), MIN, MAX, MINIFS, MAXIFS, RAND, RANDBETWEEN, PI, SIGN, CEILING, FLOOR.
  - Statistics: AVERAGE(IF/IFS), MEDIAN, MODE, STDEV(.S/.P), VAR(.S/.P), COUNT, COUNTA, COUNTBLANK, COUNTIF(S), LARGE, SMALL, RANK, PERCENTILE, QUARTILE, CORREL.
  - Logic: IF, IFS, AND, OR, NOT, XOR, IFERROR, IFNA, SWITCH, CHOOSE.
  - Lookup: VLOOKUP, HLOOKUP, XLOOKUP, INDEX, MATCH, XMATCH, OFFSET, INDIRECT, ROW(S), COLUMN(S), TRANSPOSE.
  - Text: CONCAT, CONCATENATE, TEXTJOIN, LEFT, RIGHT, MID, LEN, FIND, SEARCH, SUBSTITUTE, REPLACE, UPPER, LOWER, PROPER, TRIM, TEXT, VALUE, REPT, EXACT.
  - Dates: DATE, TODAY, NOW, YEAR, MONTH, DAY, WEEKDAY, EDATE, EOMONTH, DATEDIF, NETWORKDAYS, HOUR, MINUTE, SECOND, TIME. Serial numbers follow Excel's 1900 system, its 29 Feb 1900 included, so a date reads the same in Excel.
  - Information: ISBLANK, ISNUMBER, ISTEXT, ISERROR, ISNA, NA.
- **Number formats:** Excel's codes, the parts people use — `0`, `0.00`,
  `#,##0`, percentages, currencies, `yyyy-mm-dd` and its kin, `@`, and a
  format's sections for negatives and zero.
- **Proved by tests** against results Excel gives, written down with the
  functions' own documented examples, including its quirks: text compared
  without case, `""` against empty, rounding half away from zero.

## Files

- **`.xlsx` read:** sheets, shared strings, inline strings, numbers, booleans,
  errors, formulas (shared formulas expanded), cached values, styles (number
  formats, bold, italic, colours, fills), column widths, frozen panes,
  autofilters and conditional formats where they are simple, defined names.
  What it cannot keep — pivot tables, macros, images — is said when the file
  opens, so a save back does not silently drop it.
- **`.xlsx` written:** the same, and charts as DrawingML for the four kinds
  drawn here, so the file opens in Excel and Google Sheets as made.
- **`.csv` and `.tsv`:** read with their quoting, numbers and dates
  recognised; written from the sheet on screen.

## The window

- **The grid draws the rows on screen only,** so a million rows scroll as
  ten do: the core sends the visible cells' shown text and styles, and the
  window asks again as it scrolls. Column and row headers, the formula bar
  and name box above, the sheets' tabs below.
- **Vim's keys move cell to cell,** as in Data:
  - `h j k l` and the arrows move; `gg`, `G` and `0`, `$` go to the ends.
  - `i` and `a` edit the cell, `c` replaces it; `↩` commits and moves down, `⇥` right.
  - `v` selects a range, `V` rows.
  - `y`, `d` and `p` copy, cut and paste. Ranges go on the clipboard as tab-separated text.
  - `x` clears.
  - `dd` deletes rows, and `o` and `O` insert them.
  - `u` and `⌃r` undo and redo.
  - `/` and `⌘F` search, `n` and `N` walk what is found; `⌘⌥F` replaces.
  - `:sort`, `:filter` and `:freeze` are the `:` commands.
- **Mouse and keys as a spreadsheet's:**
  - Clicking and dragging selects.
  - Double-click edits.
  - The fill handle fills down and across, and so does `⌘D`.
  - `⌘C` and `⌘V` work with other apps.
  - `⌘B` and `⌘I` set bold and italic.
- **Edits are held until ⌘S,** as Canvas's were, and the quit prompt names
  an unsaved workbook.

## Sort, filter, colour, charts

- **Sorting** a range or a sheet by one or more columns, keeping rows whole.
- **Filters:** a header row's values chosen from, or a condition, hiding the
  rows that do not match. A filter is kept in the workbook, not applied to
  its cells.
- **Conditional colour:** rules over a range — greater than, between,
  text contains, duplicates, top and bottom n, colour scales — worked out
  by the core with the values, so what is drawn is what the rules say.
- **Charts:** line, bar, scatter and pie from a range, drawn as SVG over the
  grid, moved and resized there, and redrawn as their cells change.

## The agent

- **Tools:**
  - `sheet_info` gives the workbook, its sheets and their used ranges.
  - `sheet_read` reads a range as values and formulas.
  - `sheet_write` writes cells and formulas, held until the person saves.
  - `sheet_chart` makes a chart.
- **`@sheet`** sends the sheet on screen and what is selected.
- **Sharing with Data:**
  - A range is sent to a Data notebook as a dataframe.
  - A notebook's dataframe comes back as a sheet.

## Milestones

Each lands with its tests passing.

| | What | Proved by |
|---|---|---|
| **E0** | `proton-sheet`: the model, A1 references, the folder format, the formula parser, evaluator and dependency graph, the functions, number formats | format round trips byte for byte; each function against Excel's documented results; recalculating only what depends; cycles found |
| **E1** | Files: `.xlsx` read and written, `.csv` and `.tsv` | a workbook through `.xlsx` and back unchanged; files written here read by our reader; quoting |
| **E2** | The mode: `ModeId::Sheet` on ⌃6, its home page of workbooks, opening and making one, the grid drawn by visible rows, sheet tabs, ⌘S and the quit prompt | core tests of the events; the browser suite |
| **E3** | Editing: Vim's keys, the formula bar, ranges, copy and paste, fill, inserting and deleting rows and columns, undo, formats, widths, frozen panes | core tests; the browser suite |
| **E4** | Sorting, filters, conditional colour | core tests; the browser suite |
| **E5** | Charts, in the sheet and in `.xlsx` | core and browser tests; a chart written read back |
| **E6** | The agent's tools and `@sheet` | through `run_tool`, as the other tools are tested |
| **E7** | Ranges to and from Data | core tests |
| **E8** | A million rows measured, docs, the status page | a timing budget |

## Measured (E8)

A million rows on this Mac, an M-series, in a release build — column A the
numbers 1 to 1,000,000, column B `=A1*2` down it, and `=SUM(B:B)` and
`=AVERAGE(A:A)` beside them (`cargo test --release -p proton-sheet --test
scale -- --ignored --nocapture`):

| What | Took | Budget |
|---|---|---|
| Open: a million formulas read and worked out | 4.0 s | 8 s |
| A1 changed: B1 and the two that read whole columns, and nothing else | 0.24 s | 0.3 s |
| A screen of cells, 60 rows by 20 columns, formatted | < 1 ms | 20 ms |
| Save as a `.sheet` folder (its `.cells` file is 33 MB) | 0.21 s | 4 s |
| Read the folder back | 0.32 s | 4 s |
| Save as `.xlsx` (15 MB) | 3.7 s | 15 s |
| Open the `.xlsx` | 2.2 s | 15 s |
| Find words in the two million cells | 0.42 s | 0.8 s |
| Find digits, every number written out | 0.93 s | 1.5 s |

The test's process peaked at 1.9 GB holding the workbook several times
over (built, opened, read back from the folder and from the `.xlsx`); one
open workbook of this size is about 0.7–1 GB, most of it each formula's
parsed tree and what it reads. The edit's time is the two whole-column
functions each visiting the two million cells of their rows; a column's
cells kept apart would make that a million. Both are left until a workbook
that size is opened in earnest.
