# Circuit — Proton's digital logic mode

A logic simulator in a tab of its own, after hneemann's
[Digital](https://github.com/hneemann/Digital). You draw gates, flip-flops,
memories and buses. You run them and click their inputs while the wires show
their values. You check them against test tables, and get truth tables,
minimised expressions and circuits from one another. An agent can read and
build circuits too. Asked for on 2026-10-28.

## Decided (2026-10-28)

- **The label is "Circuit"**, on the rail and the title bar, on **⌃6**, after
  Excel's ⌃5. In the code it is `circuit`: `ModeId::Circuit`, the crate
  `proton-circuit`, the core module `circuit`, `ui/circuit*.js`, commands
  `Circuit*` and events `circuit*`.
- **A project is a folder of text that diffs**, `Name.circuit/`, which is
  Proton's rule for every document. Each circuit in it is a file, and so is
  each test table.
- **The simulator is Proton's own**, in Rust, in the core, with no new crate.
  The window draws and sends clicks, and the core sends back only the nets
  that changed, at most once a frame.
- **Nothing is taken from Digital's code.** Digital is GPL-3.0 and Proton is
  MIT, so this is built from how Digital *behaves* (its documentation and
  using it), never by reading or translating its sources. The tests use
  circuits made here, not Digital's examples. Reading `.dig` files reads a
  file format, which is fine; we ship none of theirs. Where each part's pins
  sit was worked out from drawn files: a pin that touches nothing is one
  placed wrong (`cargo run -p proton-circuit --example loose_pins`).
- **Checked against a real design.** The person's 16-bit CPU from their
  architecture course, kept outside the repo, runs with all its own test
  tables when `PROTON_CIRCUIT_DIG_DIR` names its folder (`cargo test --release
  -p proton-circuit --test digital_files -- --ignored`): 79 of 79 pass, the
  extended CPU's 5,574 parts in under a second.
- **Built now,** after Excel, at the person's word on 2026-10-28.
- **Digital's `.dig` files are read**, parts that cannot be mapped named
  when the file opens. The reader and the test language came forward into
  C0, so the engine could be checked against the person's CPU from the start;
  opening `.dig` files in the mode is still C7.

## The format: `Name.circuit/`

```
Name.circuit/
  project.json                   which circuit is the top
  circuits/<name>.circ           a circuit: a line per part, a line per wire
  tests/<circuit>.<label>.tests  a test table for a circuit
  data/<name>.hex                ROM, RAM and table contents
```

- **A `.circ` line is a part or a wire,** parts sorted by id (`U2` before
  `U10`) and then wires sorted, so moving one part changes one line:

  ```
  part A in at 2,4 bits=8 label=A
  part M ram2 at 20,8 addr=8 bits=8 program=true
  part S sub:alu at 30,2 rot 1
  part U1 and at 12,4 inputs=3
  wire 10,4 12,4
  wire 12,4 12,9
  ```

  Positions are grid steps; `rot` (quarter turns) and `mirror` are said only
  when set, and settings are sorted. Ids are kept when parts move or are
  renamed. Wires are segments on the grid — straight as Proton draws them, at
  a slant if Digital drew one so — and join only where their
  ends meet, as Digital's do: a branch splits the wire it leaves.
- **Part kinds** are lower-case words (`in`, `out`, `clock`, `const`, `and`,
  `nand`, `mux`, `decoder`, `add`, `subtract`, `register`, `dff`, `ram2`,
  `split`, `tunnel`…), each one of Digital's parts, so a circuit is built
  through the same reader as a `.dig` file and its pins sit where Digital's
  do.
- **`project.json`** is written in the order its types declare, with nothing
  empty said, as Excel's `workbook.json` is, and last, so a folder read
  halfway through a save names a top that is whole. A file that would not
  change is not written.
- **Other circuits in the folder are parts:** `sub:alu` places `alu.circ`,
  whose `in` and `out` parts, in id order, are its pins.

## The simulator (`proton-circuit`)

- **Nets, not wires.** A circuit is compiled to a netlist. Wires that touch,
  tunnels with one name, and subcircuits placed inline (each use its own copy,
  as Digital does) are merged with union-find. An edit recompiles only what
  changed.
- **Values:**
  - A net is 1 to 64 bits wide.
  - Each bit is 0, 1 or high-Z (a bit mask beside the value).
  - Two drivers disagreeing is a *conflict*: it shows in red, and the
    simulation stops at it and names the net.
  - Pull-ups and pull-downs resolve high-Z.
- **Timing, as Digital's:**
  - When an input changes, every gate it reaches reads its inputs, and then
    all their outputs change at once, as if each gate took one unit of delay.
  - At power-on, gates settle one at a time in a seeded random order, so
    master–slave flip-flops built from gates come to a state. A Reset part
    stays low until settling ends.
- **Oscillation is found, not hung on.** A change that has not settled after a
  set number of steps stops the run. It names the gates still changing, and
  single-gate stepping shows the loop one gate at a time.
- **Ways to run:**
  - **Live:** clicks and the clock, drawn as they happen.
  - **Step:** a clock edge, or one gate.
  - **Quick run:** on a worker thread with nothing drawn, until a Break part
    sees a rising edge or ⎋ is pressed. Its speed is measured and shown.
- **Proved by tests** (`cargo test -p proton-circuit`):
  - every part's truth table;
  - flip-flops from gates settling;
  - oscillation found and named;
  - conflicts and high-Z;
  - the format round trips byte for byte;
  - an edit recompiling only what it touched.

## Parts

The parts a course on digital logic uses, in the first version:

| Group | Parts |
|---|---|
| **In and out** | input (a switch or a number), output, button, switch, clock, constant, ground and power, LED, RGB LED, seven-segment and hex displays, probe |
| **Wiring** | tunnel, splitter and merger (buses), pull-up, pull-down, bit selector, bit extender |
| **Gates** | NOT, AND, OR, NAND, NOR, XOR, XNOR (2–16 inputs, any width), buffer, tri-state driver |
| **Choosing** | multiplexer, demultiplexer, decoder, priority encoder |
| **Arithmetic** | adder, subtractor, multiplier, divider, comparator, negation, shifters, counter |
| **Memory** | D, T, JK and RS flip-flops (with and without enable and reset), register, RAM (one or two ports), ROM, look-up table |
| **Control** | Reset, Break, a subcircuit, a text note |

- ROM and RAM contents are loaded from `.hex` files, and RAM can be saved to
  one.
- Parameters such as a width are set per part. Generic circuits, whose width
  is a parameter of the whole circuit, come later.

## The window

- **Drawn on one 2D canvas,** so a circuit of a few thousand parts pans and
  zooms smoothly. Parts are drawn as Digital draws them: IEEE shapes by
  default, IEC boxes as a choice.
- **When running, wires are coloured by value:** high, low, high-Z and
  conflict each have a colour, and buses show their value in hex.
- **Keys, Vim-like where Proton's other modes are:**
  - `h j k l` and the arrows move the cursor; `space` adds the part last used
    and `a` picks one by name.
  - `w` draws a wire, `r` rotates, `m` mirrors, `x` deletes.
  - `y` and `p` copy and paste; `u` and `⌃r` undo and redo.
  - `⌘R` runs and stops, `s` steps a clock edge and `.` steps one gate.
  - Double-click edits a part's settings and labels.
- **Panels:**
  - The parts, and the circuits in the project.
  - A part's settings.
  - Tests.
  - The measurement graph: a timing diagram of the probes chosen, recorded at
    each clock edge or each step. It can be sent to Data as a dataframe.
- **Edits are held until ⌘S,** and the quit prompt names an unsaved project.

## Tests

- **A test table:** a line naming the signals, then a line per case. A value
  is a number, `X` (any) or `Z`, and `C` pulses the clock.
  `repeat(n)` and `loop(i, n)` write many rows at once.

  ```
  A B Cin   S Cout
  0 0 0     0 0
  1 1 1     1 1
  loop(i, 8)
    (i>>2)&1 (i>>1)&1 i&1   (i^(i>>1)^(i>>2))&1 X
  end loop
  ```

- **Running tests:** they run from the Tests panel, by the agent, and in the
  core's tests. Each failing row is shown with what came instead.

## Analysis and synthesis

- **A truth table from a circuit:** a combinational circuit of up to 16
  inputs, and a state table from one using D or JK flip-flops.
- **Minimised expressions:**
  - Quine–McCluskey finds the prime implicants. An exact cover is found by
    Petrick's method up to about 12 inputs, and a greedy cover above that.
  - Results are shown as sums of products and products of sums.
  - Karnaugh maps are drawn up to 6 inputs, with the chosen groups.
- **A circuit from a truth table or an expression,** as AND/OR, NAND-only or
  NOR-only, laid out and placed as a new circuit or a subcircuit.
- **State machines:**
  - An FSM editor: states and the transitions between them, Moore or Mealy.
  - A state machine becomes a state table, and the state table becomes a
    circuit with D or JK flip-flops: a transition circuit and an output
    circuit.

## Export and import

- **Verilog and VHDL** of a circuit and its subcircuits, so a design can go on
  to a real FPGA toolchain. This is checked with Icarus or GHDL when the Mac
  has them, and skipped when it does not.
- **SVG** of a circuit, for notes and for Quark's LaTeX.
- **`.dig` read:** the parts above, wires, subcircuits and test tables. A
  part that cannot be mapped is named when the file opens.

## The agent

- **Tools:**
  - `circuit_info` gives the project, its circuits and their pins.
  - `circuit_read` gives a circuit as its `.circ` text.
  - `circuit_edit` adds, removes and connects parts. The changes are held
    until the person saves.
  - `circuit_test` runs tables and returns the failing rows.
  - `circuit_analyse` returns a truth table and a minimised expression.
  - `circuit_synthesise` builds a circuit from an expression or a table.
- **`@circuit`** sends the circuit on screen, what is selected, and the last
  test run.
- **With Notebook:** a source's exercise, such as "build a 4-bit adder", can
  open a project with its test table, and the tutor can see what the
  tests said.

## Not built, and why

- FPGA boards and programming them (BASYS3, TinyFPGA), and JEDEC files for
  GALs: they need hardware to test against.
- Parts in Java, or simulated through GHDL or Icarus inside a circuit: Proton
  has no JVM, and co-simulation is a large piece of its own.
- Digital's TCP remote control for assembler IDEs.
- The 74xx chip library: it comes after C4 if asked for. It is a set of
  subcircuits, made here.
- Two people editing one circuit.

## Milestones

Each lands with its tests passing.

**Built** (2026-10-30), C0–C9: C0 — the engine, the `.dig` reader and test
language, and the `Name.circuit/` folder; C1 — the mode on ⌃7: its home page
of projects, making and opening one, its circuits listed with the top marked,
the one shown drawn on a canvas (gate shapes, boxes around other parts' pins,
wires and junctions; pan, zoom and fit), a circuit added and the top changed
as held edits, ⌘S and the quit prompt; C2 — editing: parts placed from the
palette, with `space` and by name with `a`; wires dragged from a pin or a
wire's end (across, then down) or drawn with `w`, splitting the wires they
meet so a junction drawn is one; moving, turning, mirroring and deleting
what is selected (a click, ⇧-click, or a box dragged over nothing); a part's
id and settings with Enter or a double-click; copy and paste as `.circ`
text through the system clipboard; and undo and redo, each edit one step,
back to saved meaning saved. The wheel pans and ⌥-drag too.
C3 — running, of the circuit shown, built afresh after any edit with its
inputs as they were: ⌘R runs and stops; wires coloured by value (green for 1
and 0, blue for high-Z, buses lighter and labelled in hex) and inputs,
outputs and LEDs showing theirs; a click flips an input (a wider one asks
for a number), a button is held while pressed; `s` is a clock edge, and
"gate by gate" holds changes for `.` to take a gate delay at a time; `c`
leaves the clock running at 1–30 Hz; a quick run flips the clocks on a
thread of its own, nothing drawn, until a Break rises or ⎋, and says how
fast it went. Measured (release, this Mac, `a_cpu_clocked_as_fast_as_it_goes`):
the person's basic CPU (144 parts) runs at about 366 kHz, the extended one
(5,574 parts) at about 138 kHz.
C4 — parts and contents: subcircuits, tunnels, splitters, arithmetic,
flip-flops, registers and the two-port RAM were in from C0 by way of the
`.dig` reader; added now are a counter, a ROM, a one-port RAM (`ram`: A,
Din, str, C, ld → D), a look-up table, tri-state drivers (`driver`,
`driverinv`), pull-up and pull-down, a bit extender and selector, Button,
Reset and Break, and three displays — a seven-segment (`seg7`, a–g and the
point), a hex digit (`hexdisplay`, d and the point) and an RGB LED — drawn lit
while running. Their inputs run down the left and outputs to the right: as
these parts were in none of the drawings Digital's layouts were worked out
from, the layouts are Proton's own, and a `.dig` file using them may not
meet its wires. Contents are `data/<name>.hex` (Logisim's `v2.0 raw`, runs
as `n*w`), named by `file=<name>` or given inline with `data=`; Digital's
own contents in a `.dig` are read too. A running RAM is kept as a data file
with Enter or a double-click, a held edit like any other.
C5 — tests and the graph: `t` opens a panel below the drawing. Its Tests
tab lists the circuit's tables, each written there and kept as a held edit
(one that does not read is refused); ⌘↩ or Run keeps and runs one, Run all
runs them all against the circuit built afresh, and each table is marked
passing or failing with its failing rows shown; a new table is headed with
the circuit's inputs and outputs. Its Graph tab is a timing diagram of what
the run has recorded — each input and display after every click, clock
edge and gate step, 10,000 samples kept — bits as high and low, buses as a
band with their value, high-Z in blue; To Data writes it as
`.proton/circuit/<project>-<circuit>.csv` and puts a cell reading it as
`trace_df` in the notebook open (or `from-circuit.ipynb`), as Excel's ranges
go. A table is capped at two million rows.
C6 — analysis and synthesis: the panel's Analysis tab takes the circuit
shown (no state, up to 16 input bits, each bit a variable) or expressions
(`Y = A·B + C'; Z = A ^ B`, with `'`/`!`/`~`, `&`/`*`/`·`/side by side, `^`,
`|`/`+`) to a truth table; each output's least sum of products and product
of sums by Quine–McCluskey — essential primes, then an exact search for the
cover up to 12 variables (a bounded one, Petrick's method in effect) and a
greedy one above — and, for 2 to 6 inputs, a Karnaugh map in Gray order with
each group outlined. Build lays it out as a new circuit, AND/OR, NAND only
or (from the product of sums) NOR only, every junction a wire's end, its
truth table attached as the test "truth". The Machines tab is the FSM
editor, as text rather than drawn: inputs, outputs, states with Moore
outputs, transitions with conditions and Mealy outputs, drawn as a diagram
and a state table as it is typed, kept as `machines/<name>.fsm`; Build
makes `<name>` and `<name>_logic` with D or JK flip-flops (the logic
minimised through the excitation table, codes no state has as don't
cares, the parts joined by tunnels) and the machine's own run as its test.
Not built: a state table read back from a circuit of flip-flops, and
dragging states about.
C7 — out and in: the header's export writes the circuit shown into the
project's `export/` as Verilog-2001 or VHDL-2008 — one module or entity, its
ports the top circuit's inputs, clocks and outputs, its body every part of
the netlist the simulator runs, subcircuits inline; shared nets resolved
by the language, pulls as `tri1`/`tri0` or weak `'H'`/`'L'`, state starting
at 0 and changing on rising edges — or as SVG, drawn as the window draws
it. Icarus Verilog and GHDL are not on this Mac, so the check that compiles
the Verilog and drives it through a full adder's rows skips itself; the
output is otherwise checked by its text. Digital's `.dig` files in the
folder are listed on the home page and imported when opened, with every
circuit they place: positions divided by the grid, settings by name,
inputs and outputs given ids in file order (Digital's pin order), test
cases as tables, slanting wires kept, and parts with no Proton kind left
out and named. Imported this way, the person's whole CPU passes all 79 of
its test tables from the `.circuit` files.
C8 — the agent and Notebook: seven tools, through the window's own
commands so an agent's edit is a held step like any other — `circuit_info`,
`circuit_open` (also making a project or importing a `.dig`), `circuit_read`
(the `.circ` text and every part's pins), `circuit_edit` (the window's ops),
`circuit_test` (the tables, or one given in the call), `circuit_analyse`
and `circuit_synthesise`; `@circuit` sends the circuit on screen with its
pins, what is selected and the last tables run; Circuit's dock is the agent.
Notebook's studio makes Circuit practice: problems from the sources, each
with its inputs and outputs, a test table, and a reference answer —
expressions, or a state machine — that Proton builds and runs against the
table before keeping the problem, so no table is wrong; "Build in Circuit"
sets the problem up in a practice project named for the set, its signals
placed and its table the test "practice", which checks the student's
circuit.
C9 — measured, against the budgets below (see Measured). Quick run met its
budget as C3 built it. Drawing did not: a circuit of 2,500 parts took 85 ms
a frame, and now takes under 3 ms of the window's own time at any zoom. Only
what is on the stage is drawn; each part's outline is worked out once, on the
grid, and every part drawn with one fill and two strokes, its pins in a path
for each kind; labels and values are drawn together a font at a time, and
none under five of the stage's pixels high; the junctions are found once a
circuit; zoomed out below six pixels a step, parts are drawn as their boxes
without pins. The small CPU the budget names is `tiny-cpu.circuit`, in
`crates/proton-circuit/tests/`: an accumulator machine of eight bits with a
program counter, a program ROM, an adder and a subtracter, a control ROM
looked up by the operation and a zero flag, and tunnels for wires, written by
`examples/tiny_cpu.rs`; its table counts down from ten and stops at seven.

| | What | Proved by |
|---|---|---|
| **C0** | `proton-circuit`: values, nets, gates, flip-flops, registers, RAM, ROM, the event simulator, settling, oscillation and conflicts, the `.circ` format; brought forward, the `.dig` reader and Digital's test language | each part's table; settling; oscillation named; format round trips; the person's CPU passing its own tables |
| **C1** | The mode: `ModeId::Circuit` on ⌃7, its home page of projects, opening and making one, a circuit drawn, ⌘S and the quit prompt | core tests of the events; the browser suite |
| **C2** | Editing: placing, wiring, moving, rotating, deleting, settings, copy and paste, undo | core tests; the browser suite |
| **C3** | Running: live, step, single-gate step, quick run and Break; inputs clicked, displays drawn, wires coloured | core tests; the browser suite; a measured speed |
| **C4** | Subcircuits, tunnels, splitters, arithmetic and memory parts, `.hex` contents | core tests |
| **C5** | Test tables and the measurement graph; the graph to Data | core tests; the browser suite |
| **C6** | Truth tables, Quine–McCluskey, K-maps, synthesis, state tables, the FSM editor | each against hand-worked cases; a synthesised circuit passing its table |
| **C7** | Verilog, VHDL and SVG out; `.dig` in | written Verilog run by Icarus when present; `.dig` files made here read |
| **C8** | The agent's tools, `@circuit`, Notebook's exercises | through `run_tool`, as the other tools are tested |
| **C9** | Measured: a small CPU in quick run at 100 kHz or more, a 2,500-part circuit drawn at 60 fps; docs; the status page | timing budgets |

## Measured (C9)

On this Mac, an M-series, release builds. The simulator's budget is kept by
tests that run in a release build and only report in a debug one; drawing's
by `tests/ui/circuit-bench.html`, with a looser check in the browser suite.

**Quick run** — the clock flipped as fast as it goes, as Quick runs it: the
core's quick run for a second, then stopped (`cargo test --release -p
proton-core --lib quick_run -- --nocapture`, and `-p proton-circuit --test
speed` for the simulator alone).

| Circuit | Parts | Clocked at | Budget |
|---|---|---|---|
| `tiny-cpu.circuit`, in the core's quick run | 19 | 1.07 MHz | 100 kHz |
| the same, the simulator alone | 19 | 1.06 MHz | 100 kHz |
| the person's basic CPU (`.dig`, imported) | — | 359 kHz | 100 kHz |
| the person's extended CPU (`.dig`, imported) | — | 223 kHz | 100 kHz |

A debug build runs the tiny CPU at about 190 kHz. The person's CPUs are
measured by an ignored test given their folder (`PROTON_CIRCUIT_DIG_DIR`),
copied to a scratch folder first, with no program loaded; with FIB's program
in RAM the simulator alone ran the extended CPU at about 138 kHz in C3.

**Drawing** — the half adder laid out 417 times, 2,502 parts and 5,004
wires, panned a step a frame at each zoom from fitted to close, and once
running with every wire and input carrying a value. The window's main
thread, per frame, in WebKit as the app has it (a WKWebView on the screen,
invisible), at 60 frames a second:

| Zoom (pixels a step) | Median | p95 | Late frames |
|---|---|---|---|
| 4.5, fitted: parts as boxes | 1 ms | 2 ms | 0 |
| 7.1, the most on the stage drawn whole | 2 ms | 3 ms | 0 |
| 8.8, labels drawn too | 3 ms | 5 ms | 0 |
| 27, close | 1 ms | 2 ms | 2 of 120 |
| 8.8, running | 4 ms | 6 ms | 0 |

WebKit paints the canvas in its GPU process, beside the main thread: about
3 ms of its time a frame over the whole run, and the page's own process
about 2 ms. Headless Chromium, which paints in software, keeps 60 frames a
second at every zoom too, its frames 3–17 ms with the painting in them and
the drawing itself under 3 ms. Before C9 the same circuit took 85 ms a frame
zoomed in, most of it the wires sorted by colour a copy at a time.

## Decided at the start

On 2026-10-28: the label "Circuit" on ⌃7; the simulator in the core (moved to
WebAssembly in the window only if C3's measurements show clicks feel slow);
and `.dig` import in C7.
