# Backlog

What is planned and not built. Each item says what it is, what is still
open about it, and what it waits on. Items leave this file when they land.

## Research

Research's milestones after Lean (P0–P7, `6c103a9`).

### Waiting on the scientist

- **Lean, live (P8).** Make Lean from the Lab pane's Lean card (about 3 GB
  to download, about 15 GB kept). Then
  `PROTON_LEAN_RECORD=1 cargo test -p proton-core --test live_lean -- --ignored --nocapture`
  records `crates/proton-research/fixtures/lean/*.jsonl` again from a real
  Lean — the offline tests' expected statements may need adjusting to its
  real output — and a live mathematics round follows, with the scientist's
  go-ahead. Unverified until then: whether `lake exe cache get` builds its
  tool without Xcode, Mathlib's cache host, the real `lean --json` message
  shape, time and memory per check, and whether 30 turns cover three
  attempts.
- **A live round with the literature on.** `tests/live_round.rs` defaults to
  it; set `PROTON_MCP=target/release/proton-mcp` so Claude Code and Codex
  read the papers through Proton's server. It spends the scientist's limits.
- **Semantic Scholar's fixtures.** Its pool for callers without a key
  answered 429 throughout 2026-10-06, so `tests/fixtures/s2_*.json` were never
  recorded: `PROTON_LITERATURE_RECORD=1 cargo test -p proton-literature --test live semantic -- --ignored`
  once it answers.

### Later builds

- **Manuscripts.** The paper drafted inside the investigation, every figure
  and number traced to the run, code and environment that produced it, with
  inline annotation to refine it. Open: LaTeX or Typst, and where it compiles.
- **Remote compute.** The Lab's code run elsewhere. Open: SSH, Slurm, or
  both, and what provenance and sandboxing mean off the machine.
- **Lean's statements compared.** Lean's `comparator` (sandboxed on Linux)
  to check a proved statement against an agreed one, rather than leaving
  faithfulness to the prover's notes, the judges and the scientist.
- **The Lab on Linux and Windows.** bubblewrap has never been run for real,
  and the Lab is Unix-only (no `Scripts\python.exe`).

## Excel

The spreadsheet mode asked for on 2026-10-27 as Sheets and named Excel on
2026-10-28 is built, E0–E8, as [`EXCEL.md`](EXCEL.md) planned it, and
installed on 2026-10-28. What is left of it.

### Waiting on the person

- **Excel in the installed app, with your own workbooks.** Open real `.xlsx`
  and `.csv` files of yours — ones Excel or Google Sheets made, not Proton —
  and say what breaks or what you reach for that is missing. Everything so far
  was tested in the browser and the core.
- **Charts checked in Excel.** Charts written into an `.xlsx` are read back by
  Proton's own reader; Quick Look draws no charts, and Excel is not on this
  Mac to open one in.

### Later builds

- **.xlsx features kept through a save.** Merged cells, conditional formats,
  filters, comments and data validation are read today as "not kept", and said
  when the file opens; keeping them means writing them back as Excel does.
  Pictures, pivot tables and macros the same.
- **Borders, fonts and row heights.** Cell borders, a font's family and size,
  and rows of their own height; today a style is bold, italic, underline,
  colours, alignment, wrapping and a number format.
- **Merging, and hiding rows and columns.** Cells merged across a range; rows
  and columns hidden and shown again by hand, not only by a filter.
- **Finding across a workbook.** `/` searches the sheet shown, or a range of
  it; every sheet at once, and patterns, are not there yet.
- **Formula completion.** Function names and their arguments offered as a
  formula is typed, and the cells a formula reads outlined in the grid.
- **Spilled arrays and dynamic-array functions.** A formula's array result
  filling the cells below and beside it, as Excel 365's do; then FILTER, SORT,
  UNIQUE and SEQUENCE. Today a cell shows an array's first value.
- **More functions.** Excel has about 500 to Proton's 140: financial (PMT,
  NPV, IRR, FV), regular expressions, LET and LAMBDA, and the rest as asked
  for.
- **Pivot tables.** Summaries of a range by rows and columns, kept in the
  workbook and redrawn as its cells change.
- **Comments on cells.** Notes on a cell, kept in `comments.json` as the
  folder format already plans, and to and from `.xlsx`.
- **Printing.** A sheet or a range to the printer or a PDF, with page breaks
  and headers.
- **A million rows, lighter.** Opening a million formulas takes 4 s and about
  0.7–1 GB; a whole-column sum visits every cell of its rows. Keeping each
  column's cells apart, and the formulas' trees smaller, would bring both down
  (`EXCEL.md`, Measured).
- **Two people on one sheet.** Editing a workbook together would need a
  server; a workbook is one person's for now.

## Circuit

The digital logic mode after hneemann's Digital, asked for on 2026-10-28,
is built, C0–C9, as [`CIRCUIT.md`](CIRCUIT.md) planned it, on 2026-10-30.
What is left of it.

### Waiting on the person

- **Circuit in the installed app, with your own circuits.** Your CPU's 79
  test tables pass from its `.dig` files and imported; the window has been
  tested in the browser and timed in WebKit, not used by hand in the app.
- **Icarus Verilog and GHDL.** The check that compiles the written Verilog
  and drives it through a full adder's rows skips itself while they are not
  on this Mac (`brew install icarus-verilog ghdl`); VHDL has no run check
  yet.

### Later builds

- **A state table from a circuit of flip-flops.** The state machine read back
  from a drawn circuit, as Digital does; today machines go one way, text to
  circuit.
- **State machines drawn by dragging.** The Machines tab is text with a
  diagram drawn from it; states cannot be moved about on the diagram.
- **The 74xx library.** Digital's chips as subcircuits, made here.
- **FPGA boards and JEDEC files.** BASYS3 and TinyFPGA, and GALs; they need
  the hardware to test against.

## CAD

A CAD mode, asked for on 2026-11-07, is planned in [`CAD.md`](CAD.md):
parametric parts on Proton's own boundary-representation kernel,
assemblies with mates and clearance checks, liquid-cooling routes with their
loop schematic, drawings (blueprint among the styles), and electronics —
schematics, footprints, boards and Gerbers. "CAD" on ⌃7. First use: the
person's FormD T1 build with a custom loop.

### Waiting on the person

- **The FormD T1's figures** and the parts of the build (motherboard,
  card, radiator, pump, fittings), so the library makes those first.
- **Whether a board for the case comes before the build is finished.**
- **Whether curved panels drawn with splines are wanted**: K2 refuses a
  spline in a profile, since what it sweeps is not one of the kernel's five
  surfaces; two exact swept surfaces would add it.

## Logic

A logic mode after logic-calculator.com and Logic for Fun, asked for on
2026-10-30, is built as [`LOGIC.md`](LOGIC.md) describes, L0–L9: a
calculator (truth tables, properties, normal forms, Karnaugh maps,
equivalence, arguments), a model finder for puzzles written in first-order
logic, and proofs by semantic tableaux, natural deduction, sequent calculus
and resolution; "Logic" on ⌃6, Proton's own language and Logic4Fun's both
read and written; the agent's logic tools.
Fitch's rule names were settled at L6: `forall x`'s written, the common
others read.

### Found by measuring

- **Natural deduction on eight Pelletier problems.** P18, P19, P26, P33,
  P35, P39, P40 and P42 are not found in three seconds; the other systems
  prove them. Why each escapes the search is not yet looked into.

### Later builds

After L9, for a complete logic workbench:
- **More proof styles.** Hilbert-style axiomatic proofs; tree-shaped
  natural deduction; Boolean simplification step by step, each law named.
- **More first-order machinery.** Herbrand models; equality by
  paramodulation.
- **SAT with the search shown.** DPLL and CDCL, for large propositional
  problems.
- **Other logics.** Modal (K, T, S4, S5) with tableaux and Kripke
  countermodels; intuitionistic, with Kripke countermodels; three-valued and
  fuzzy; relevant logics.
- **Temporal logic.** LTL and CTL model checking.
- **Syllogisms** with Venn diagrams.
- **English to logic and back,** by the agent.
- **To Lean.** A statement handed to Lean in Research.
