//! A million rows, timed. Ignored by default — it wants a release build
//! and a few hundred megabytes:
//!
//! ```text
//! cargo test --release -p proton-sheet --test scale -- --ignored --nocapture
//! ```
//!
//! The budgets are for an M1-class Mac. They are what a person waits for:
//! opening the workbook, typing into a cell everything depends on, a
//! screen of cells drawn, saving.

use std::time::Instant;

use proton_sheet::model::{Cell, Workbook};
use proton_sheet::{Book, CellRef};

const ROWS: u32 = 1_000_000;

fn timed<T>(what: &str, budget_ms: u128, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    let ms = t.elapsed().as_millis();
    println!("{what}: {ms} ms (budget {budget_ms} ms)");
    assert!(ms <= budget_ms, "{what} took {ms} ms, over its {budget_ms} ms");
    out
}

#[test]
#[ignore = "a million rows: run in release, by hand"]
fn a_million_rows_open_recalculate_draw_and_save_in_time() {
    let mut wb = Workbook::default();
    let cells = &mut wb.sheets[0].cells;
    for r in 0..ROWS {
        cells.insert(CellRef::new(r, 0), Cell { input: (r + 1).to_string(), style: 0 });
        cells.insert(CellRef::new(r, 1), Cell { input: format!("=A{}*2", r + 1), style: 0 });
    }
    cells.insert(CellRef::new(0, 3), Cell { input: "=SUM(B:B)".into(), style: 0 });
    cells.insert(CellRef::new(1, 3), Cell { input: "=AVERAGE(A:A)".into(), style: 0 });

    let mut book = timed("open: a million formulas read and worked out", 8_000, || Book::new(wb));
    let n = ROWS as f64;
    assert_eq!(book.value(0, CellRef::new(0, 3)), proton_sheet::Value::Number(n * (n + 1.0)));

    let before = book.computed();
    timed("an edit everything sums depends on", 300, || book.set(0, CellRef::new(0, 0), "1000001"));
    assert_eq!(book.computed() - before, 3, "B1 and the two that read the columns");

    timed("a screen of cells, 60 rows by 20 columns, formatted", 20, || {
        let mut shown = 0;
        for r in 500_000..500_060 {
            for c in 0..20 {
                let v = book.value(0, CellRef::new(r, c));
                shown += proton_sheet::numfmt::show(&v, "#,##0.00").text.len();
            }
        }
        shown
    });

    let dir = std::env::temp_dir().join(format!("proton-sheet-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    timed("save as a .sheet folder", 4_000, || proton_sheet::folder::write(&dir, book.workbook()).unwrap());
    let size = std::fs::metadata(dir.join("sheets/s1.cells")).map(|m| m.len()).unwrap_or(0);
    println!("  sheets/s1.cells: {} MB", size / 1_000_000);
    let back = timed("read the folder back", 4_000, || proton_sheet::folder::read(&dir).unwrap());
    assert_eq!(back.sheets[0].cells.len(), ROWS as usize * 2 + 2);
    let _ = std::fs::remove_dir_all(&dir);
    let bytes = timed("save as .xlsx", 15_000, || proton_sheet::xlsx::write(&book).unwrap());
    println!("  .xlsx: {} MB", bytes.len() / 1_000_000);
    timed("open the .xlsx", 15_000, || proton_sheet::xlsx::read(&bytes).unwrap());
}
