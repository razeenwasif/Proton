# Circuit — Proton's digital logic mode

A logic simulator in a tab of its own, after hneemann's
[Digital](https://github.com/hneemann/Digital). You draw gates, flip-flops,
memories and buses. You run them and click their inputs while the wires show
their values. You check them against test tables, and get truth tables,
minimised expressions and circuits from one another. An agent can read and
build circuits too. Asked for on 2026-10-28.

## Decided (2026-10-28)

- **The label is "Circuit"**, on the rail and the title bar, on **⌃7**, after
  Excel's ⌃6. In the code it is `circuit`: `ModeId::Circuit`, the crate
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
  data/<name>.hex                ROM and RAM contents (C4)
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
  renamed. Wires are straight segments on the grid and join only where their
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

**Built so far** (2026-10-29): C0 — the engine, the `.dig` reader and test
language, and the `Name.circuit/` folder; C1 — the mode on ⌃7: its home page
of projects, making and opening one, its circuits listed with the top marked,
the one shown drawn on a canvas (gate shapes, boxes around other parts' pins,
wires and junctions; pan, zoom and fit), a circuit added and the top changed
as held edits, ⌘S and the quit prompt. Editing the drawing is C2.

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

## Decided at the start

On 2026-10-28: the label "Circuit" on ⌃7; the simulator in the core (moved to
WebAssembly in the window only if C3's measurements show clicks feel slow);
and `.dig` import in C7.
