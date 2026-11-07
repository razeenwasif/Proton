# CAD — Proton's mode for parts, assemblies, drawings and boards

Parametric 3D design and electronics, in a tab of its own:
- **parts** sketched on a plane with dimensions and constraints, then
  extruded, revolved, cut, holed, filleted and patterned, with a feature
  history that can be edited at any step;
- **assemblies** of parts and of other assemblies, placed by mates, with
  clearances checked and a bill of materials;
- **routes** for liquid cooling: hard and soft tubes between fittings, bent
  to each tube's radius, with a cut list and the loop's own schematic;
- **drawings**: orthographic, section and detail views of a part or an
  assembly, dimensioned, with a title block and balloons — drawn as paper
  or as a blueprint;
- **electronics**: schematics, footprints, a board laid out and routed,
  and Gerbers, with the board brought into an assembly in 3D.

What it is for, first: the person's FormD T1 build — the case, every part
in it and a custom liquid-cooling loop — modelled, checked for fit, and
drawn. Asked for on 2026-11-07.

## Decided (2026-11-07)

- **The label is "CAD"**, on the rail and the title bar, on **⌃8**, after
  Logic's ⌃7. In the code it is `cad`: `ModeId::Cad`, the crates
  `proton-cad` (the kernel, parts, assemblies, routes, drawings) and
  `proton-board` (electronics), the core module `cad`, `ui/cad*.js`,
  commands `Cad*` and events `cad*`.
- **Parametric 3D and electronics, both,** the person's choice over code
  CAD and 2D drafting. They will 3D print, machine or send parts to others,
  and draw them.
- **Proton's own kernel, no new external crates,** the person's choice over
  binding OpenCascade or taking a Rust B-rep crate. The kernel is a
  boundary representation (below), not meshes, because machining and
  sharing need exact STEP files, which meshes cannot give.
- **Nothing is taken from another CAD program's code.** SolveSpace (GPL),
  FreeCAD and OpenCascade (LGPL), KiCad (GPL) and their libraries are read
  for what they do, not copied; the kernel, the solver and the libraries are
  written from published mathematics and published standards.
- **Documents are text files that diff,** Proton's rule: `.part`,
  `.assembly`, `.drawing`, `.schematic` and `.board`. What is drawn with the
  mouse is written to the same text, which can be edited, read by the agent,
  and kept in Git.
- **The window draws with three.js,** already vendored for Research's 3D
  view; the kernel sends meshes tessellated from exact geometry and keeps
  the geometry itself.

## The kernel

A boundary representation: a solid is shells of faces, a face is a surface
bounded by loops of edges, an edge is a curve between two vertices.

**Surfaces** are analytic: plane, cylinder, cone, sphere and torus — what
milled, turned, drilled, bent and filleted parts are made of, and what a
bent tube is (cylinders joined by tori). **Curves** are lines, circles,
ellipses, and cubic B-splines for what two analytic surfaces meet in when
that is not one of them (two cylinders at a tee, a hole through a cone).
Free-form NURBS surfaces are not built, so a shell that is sculpted rather
than made is outside it; a STEP file holding one is read for display and
measurement only (K13).

**The operations:** extrude and revolve (to a depth, through all, to a face,
symmetric, with draft), sweep along a path, loft between two profiles of
the same edge count; union, cut and intersect; holes (simple, counterbored,
countersunk, tapped, with M2–M8, UNC 6-32 and G1/4 threads as cosmetic
threads and their tap drills); linear, circular and mirrored patterns;
shell; constant-radius fillets and chamfers where the faces either side are
analytic (plane–plane, plane–cylinder, cylinder–cylinder at right angles),
and refused with the reason elsewhere; sheet metal (K12).

**Robustness** is the hard part of any kernel, and is designed for, not
hoped for: one tolerance (10⁻⁶ mm in a part of a metre), predicates that
say "on" before "in" or "out", every result checked for being a closed,
oriented, manifold solid before it is kept, and a failed step reported
with the faces that caused it, the earlier result left as it was.

**Names that survive edits.** A face is known by what made it — "the top
of extrude 2, from the line `top` of sketch 1" — not by a number, so a
fillet on that edge still finds it when an earlier dimension changes; when
it cannot, the fillet says which edge it lost.

## The documents

```
part "Pump bracket"
units mm
material aluminium-6061
param t = 2
param hole = 3.4

sketch base on XY
  rect r corner (0, 0) size (80, 40)
  circle h1 center (10, 10) diameter hole
  circle h2 center (70, 10) diameter hole
  equal h1 h2
  horizontal h1.center h2.center
end

extrude plate from base distance t
hole m3 on plate.top at h1.center, h2.center thread M3 through
fillet plate.edges(parallel z) radius 2
```

An `.assembly` lists instances — a part or assembly file, or a library
part with its parameters — and the mates between them (fastened, revolute,
slider, cylindrical, planar, distance, angle). A route belongs to an
assembly and names the fittings it runs between. A `.drawing` names a
part or assembly, its sheet, its views and its dimensions. A `.schematic`
and a `.board` are symbols, nets, footprints, tracks and zones, by name.

## The parts library

Parametric, generated from published standards and specifications, each
part checked against them in tests:
- **Boards and power:** Mini-ITX motherboards (170 × 170 mm, the mounting
  holes and keep-outs of the ATX specification, the socket, the I/O
  shield), SFX and SFX-L power supplies, M.2 drives, memory.
- **Graphics cards** by their published length, height and thickness, the
  slot bracket and the power connectors' place, since every card differs.
- **Cooling:** fans (80, 92, 120, 140 mm; 15, 25, 38 mm thick, their holes),
  radiators (120–360 mm, by thickness), CPU blocks, GPU blocks, pumps and
  reservoirs, G1/4 fittings (straight, 45°, 90°, rotary, extenders,
  compression for 12/16, 14/16 and 10/13 mm tube), drain ports.
- **Fasteners:** ISO screws M2–M5, UNC 6-32, standoffs, nuts.

**The FormD T1** is modelled from what FormD publishes and from the
person's own measurements, and its known layouts (sandwich, the radiator
above or below) are kept as its configurations. Its published figures are
taken as given; where a figure is missing the part says so rather than
guessing, and asks for a measurement.

## Liquid cooling

A **route** is a run of tube between two fittings' ports. A hard tube is
straight runs and bends, each bend at least the tube's own minimum radius,
and is written to the kernel as cylinders and tori, exact; a soft tube is a
smooth curve between the ports, its length checked against the bend radius
it can take. A route is checked for clearance against everything in the
assembly, and gives a cut list (the length of each straight before it is
bent, by the tube's centre line) and a bend list (angle and place) to work
from at the bench.

**The loop schematic** is made from the routes: pump, reservoir, blocks,
radiators and fittings in the order coolant meets them, the flow's
direction, each tube's length, and the parts list — drawn as a diagram
beside the 3D, and on a drawing sheet.

## Drawings

Sheets in ISO (A4–A1) and ANSI (A–D) sizes; first- or third-angle views,
isometric views, sections, details; hidden lines worked out from the exact
geometry, so a hole's silhouette is a line where the cylinder turns away.
Dimensions (linear, radial, diameter, angular, ordinate) attached to the
geometry and moved with it; centre marks, thread notes, a title block, a
bill of materials with balloons for an assembly. **Blueprint** is a style:
white lines on blue. Out as PDF, SVG and DXF.

## Electronics

- **Schematics:** symbols (Proton's own, for the common parts, and any
  part made from a pin list), wires, labels, power symbols, buses; nets
  named; the electrical rules checked (an output driving an output, a pin
  left unconnected).
- **Footprints,** generated from IPC-7351 nominal land patterns: chip
  resistors and capacitors 0402–2512, SOT, SOIC, TSSOP, QFP, QFN,
  through-hole headers, USB-C, and fan and RGB headers.
- **The board:** its outline drawn, or taken from a sketch of a part so it
  fits the case; two or four layers; placement, a ratsnest, tracks and vias
  routed by hand with clearances kept as they are drawn, zones poured with
  thermal reliefs; the design rules checked.
- **Out:** Gerber X2, Excellon drills, a bill of materials and a
  pick-and-place file; the board, its components as their bodies, into an
  assembly and into STEP.
- **KiCad's** symbol and footprint libraries, where KiCad is installed, read
  so their parts can be used; its project files read later.

## The window

The home page lists the project's parts, assemblies, drawings, schematics
and boards. A part shows the 3D view in the middle, the feature history on
the left (each step's error marked where it failed) and the selected
step's parameters on the right; a sketch opens flat on its plane, with
constraints drawn as glyphs, dimensions edited in place, and the solver
saying how many degrees of freedom are left — and, over-constrained, which
constraints disagree. An assembly shows its tree and mates; a drawing its
sheet; a schematic and a board their canvases. ⌘S saves; unsaved files
join the quit prompt.

## The agent

`cad_read`, `cad_write`, `cad_measure` and `cad_check` (clearances,
interference), `@cad` for the open file, and a part written by the agent
lands in the same text, so it is built, shown and checked like one drawn by
hand.

## Not built, and why

- **Free-form surfaces** (NURBS modelling), for car bodies and consumer
  shells: a kernel of its own size again. STEP files with them are shown,
  not edited.
- **General fillets** — variable radius, between free-form faces, or
  where three fillets meet in a corner patch: refused with the reason.
- **Simulation** (stress, thermal, flow), and **CAM** (toolpaths, G-code):
  later builds, if asked for.
- **An autorouter:** routing is by hand.

## Milestones

Each lands with its tests passing, in this order: the parts and the build
first (K0–K11), then the electronics (E0–E5), then what is left (K12–K14).

| | What | Tested by |
|---|---|---|
| **K0** | `proton-cad`: vectors, placements, tolerance; curves and surfaces with their evaluation, projection and intersections (every analytic pair; marching to a B-spline otherwise) | intersections against closed forms; random points on, off and near |
| **K1** | Sketches: points, lines, arcs, circles, splines; constraints and dimensions; a solver that counts degrees of freedom and names conflicting constraints; the `.part` language read and printed back unchanged | solved sketches against known answers; round trips |
| **K2** | The B-rep: topology, validity, tessellation; extrude and revolve of profiles with holes; STL and 3MF out | closed, manifold, oriented; volumes and areas against formulas |
| **K3** | The mode: `ModeId::Cad` on ⌃8, the home page, the 3D view, the feature history, sketching on a plane with the mouse, dimensions edited, ⌘S and the quit prompt | browser tests; a part built by hand and read back |
| **K4** | Booleans, cut extrudes, holes and threads, patterns, mirror; names that survive edits | random booleans checked valid; edits that keep and lose references |
| **K5** | Fillets, chamfers, shell, sweep and loft | the analytic cases against formulas; refusals named |
| **K6** | STEP (AP214) out, exact; read back by Proton's own reader | round trips; a file checked by hand in another CAD program |
| **K7** | Assemblies: instances, mates solved, degrees of freedom, explode, interference and clearance, the bill of materials | mates against known placements |
| **K8** | The parts library, and the FormD T1 with the person's measurements | each part against its specification |
| **K9** | Routes: hard and soft tube, fittings' ports, cut and bend lists, clearance; the loop schematic | lengths against hand-worked loops |
| **K10** | Drawings: sheets, views, hidden lines, sections, dimensions, title block, BOM and balloons, blueprint; PDF, SVG, DXF | views against hand-drawn projections; files read back |
| **K11** | The agent's tools, `@cad`; measured; docs; the status page | through `run_tool`; timing budgets |
| **E0** | `proton-board`: schematics, symbols, nets, the rules check; `.schematic` | nets against hand-worked circuits |
| **E1** | Footprints from IPC-7351 | pads against the standard's tables |
| **E2** | The board: outline, stack-up, placement, ratsnest, routing by hand, vias, zones, the design rules | clearances found and kept |
| **E3** | Gerber X2, Excellon, BOM, pick-and-place | read back by Proton's own reader |
| **E4** | The board in 3D, in an assembly and in STEP | the board's solid against its outline and thickness |
| **E5** | KiCad's libraries read | symbols and footprints against KiCad's own files |
| **K12** | Sheet metal: flanges, bends with a K-factor, the flat pattern to DXF | flat lengths against bend tables |
| **K13** | STEP, STL and DXF read in: shown, measured and mated to | files from manufacturers' sites, kept out of the repository |
| **K14** | Later builds as asked: simulation, CAM | — |

## Open

- **The FormD T1's figures.** Which version (2.1 is assumed), and the
  person's measurements where FormD publishes none.
- **The parts in the build:** which motherboard, card, radiator, pump and
  fittings, so their parts are made to their sizes first.
- **Whether electronics should come before the build is finished**, if a
  board for the case (a fan or lighting controller, front I/O) is wanted
  sooner.
