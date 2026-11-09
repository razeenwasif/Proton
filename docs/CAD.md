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

- **The label is "CAD"**, on the rail and the title bar, on **⌃7**, after
  Logic's ⌃6 (⌃8 until Notebook went to Fairy on 2026-11-09). In the code it is `cad`: `ModeId::Cad`, the crates
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

## Decided (2026-11-09)

- **A blueprint is a layout of the build:** the assembly drawn to show how
  it looks and how it fits together — the whole from the front, side, top
  and at an angle, exploded along its mates, cut through the case, the
  clearances between parts dimensioned, every part ballooned against the
  bill of materials (K10). White lines on blue is a way to draw it, not
  what it is.
- **"Schematic" meant custom boards:** the small PCBs a compact Mini-ITX
  build uses to look tidy — power adapters and extensions, fan and RGB
  hubs, front I/O — designed here, with a schematic as the first step of
  each (E0–E5).
- **The order:** all of CAD first — K5 to K13 and E0 to E5. Then the
  person's PCPartPicker list and their loop's pieces and tubes, modelled
  into a complete custom loop in the FormD T1 as the last test, the T1's
  published figures looked up. K14 (simulation, CAM) only if asked.

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
shell; constant-radius fillets and chamfers where they come out as one of
the kernel's surfaces — along a straight edge between faces running its way
(planes, cylinders along it), a cylinder or a plane; along a round edge
between faces turned about its axis, a torus or a cone — with mitred,
smooth and ball-cornered ends, and refused with the reason elsewhere (two
pipes in a tee); sheet metal (K12).

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
  rect r corner origin size = (80, 40)
  circle h1 center (10, 10) diameter = hole
  circle h2 center (70, 10) diameter = hole
  horizontal h1.center h2.center
  hdistance r.bl h1.center = 10
  vdistance r.bl h1.center = 10
  hdistance h1.center h2.center = 60
end

extrude plate from base distance t
hole m3 on plate.top at h1.center, h2.center thread M3 through
fillet corners on plate.edges(parallel z) radius 2
```

A statement a line; `#` starts a comment, kept with the file. In a sketch a
shape says where it is now — numbers — and a value after `=` is a dimension
the solver keeps: `circle h1 center (10, 10) radius 1.7` is drawn there,
`… diameter = hole` is held to it. A shape's point may be one already drawn —
`line b a.end (80, 40)` starts where `a` ends — so a profile is a run of
shapes sharing points. Names: `origin`, `xaxis` and `yaxis`; a line's
`.start` and `.end`; a circle's or arc's `.center` (an arc's `.start` and
`.end` too); a rectangle's `.bottom`, `.right`, `.top`, `.left` and its
corners `.bl`, `.br`, `.tr`, `.tl`; a spline's `.1`, `.2` … and `.start`,
`.end`. Constraints: `coincident`, `on`, `horizontal`, `vertical`, `parallel`,
`perpendicular`, `tangent`, `equal`, `midpoint`, `symmetric A B about L`,
`concentric`, `fix`; dimensions: `distance`, `hdistance`, `vdistance`,
`length`, `radius`, `diameter`, `angle` (degrees). Solved, the file is
written back with where everything ended up, rounded to a nanometre.

A sketch is drawn `on XY` (its normal +z), `XZ` (its x along x, its y along
z, its normal −y), `YZ` (normal +x), another sketch's plane, or a flat face
of a body above — `plate.top` — with `offset` along its normal. Its profile
is every closed loop its lines, arcs and circles make (construction geometry
left out): a loop inside another is a hole in it, one inside a hole an
island. `extrude NAME from SKETCH distance D` pushes the profile along the
sketch's normal (back for a negative `D`; `symmetric` puts half each side);
`revolve NAME from SKETCH about LINE angle A` turns it about a line of the
sketch — `xaxis`, `yaxis` or one drawn, often as construction — by `A`
degrees, all the way round if no angle is given. Either makes a body of its
own, or ends `join BODY`, `cut BODY` or `intersect BODY` to be made into one;
an extrude can go `through` everything both ways in place of a distance.
`hole NAME on FACE at P, P … diameter D` (or `thread M3`, `clearance M3`)
`depth D` (or `through`), with `counterbore D depth H` or `countersink D
angle A` if wanted, is drilled square into a flat face at sketches' points
or `(x, y)` on the face, and cut from the body the face is on — a blind hole
ending in a drill's 118° point, a tapped one at its tap drill with the thread
kept for drawings. `pattern NAME of F, F … linear (x, y, z) count N` or
`circular about AXIS [angle A] count N` makes features again moved, into the
bodies they were made into; `mirror NAME of F, F … about PLANE` makes them
again mirrored in `XY`, `XZ`, `YZ` or a flat face. `union BODY with OTHER`,
`cut BODY with OTHER` and `intersect BODY with OTHER` make two bodies one.
`fillet NAME on EDGES radius R` rounds edges and `chamfer NAME on EDGES
distance D [and D2]` cuts them flat, the second distance along the second
face; an edge is named by its two faces, `plate.top & plate.side(r.left)`,
or picked among a body's or a face's, `plate.edges(parallel z)`,
`boss.top.edges(convex, round)` (`parallel x|y|z`, `convex`, `concave`,
`round`, `straight`), seams and smooth joins left out. `shell NAME of BODY
thickness T [open FACE, FACE …]` hollows a body to walls that thick, the
faces named taken away. `sweep NAME from SKETCH along PATH` carries a
profile along a sketch's lines and arcs, end to end and turning only on
arcs, the profile standing square to the path where it starts; `loft NAME
from SKETCH to SKETCH` joins two profiles of as many edges in parallel
planes. Faces are named for what made them: an extrude's `NAME.top` and
`NAME.bottom`, a revolve's `NAME.start` and `NAME.end`, each side
`NAME.side(r.left)` after the shape that swept it, a hole's walls
`NAME.side(wall1)`, a sweep's or loft's ends `NAME.start` and `NAME.end`, a
round's and a chamfer's faces `NAME` (a ball at a corner `NAME.corner`), a
shell's inside `NAME.inside(FACE)` and the sides of its openings
`NAME.rim`; a face keeps its name through every boolean, so a sketch
or a hole placed on it finds it after an edit, and one that is gone is said
to be, with why.

An `.assembly` lists instances — `part NAME from "FILE"`, `assembly NAME
from "FILE"`, paths from the assembly's folder — where each is (`place NAME
at (x, y, z) turn (a, b, c)`, turned about x, then y, then z, in degrees,
written back by the solver), what is held still (`ground NAME`, else the
first), the least gap wanted (`clearance 1`), the mates between them (`mate
NAME KIND A to B`, with `= VALUE` for a `distance` or an `angle` and
`offset VALUE` for a `planar`, `flush`, `revolute` or `slider`; `fastened`,
`parallel`, `perpendicular`, `cylindrical` and `concentric` take neither),
and exploded views (`explode NAME INSTANCE by (x, y, z), …`). A mate's ends
are an instance's faces by name, `board.plate.bottom`, an assembly's part's
`loop.pump.body.top`, the part's own `XY`, `XZ`, `YZ`, `x`, `y` and `z`, or
for `fastened` the instance itself. A route belongs to an
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
- **Cooling:** fans (80, 92, 120, 140 mm; 14, 15, 25, 38 mm thick, their holes),
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
bill of materials with balloons for an assembly. **The blueprint** of an
assembly is a sheet laid out for it: the build from four sides and at an
angle, an exploded view, a section through the case, the clearances that
matter dimensioned (a card to a panel, a radiator to a fan, a tube to
anything), and the parts ballooned and listed — on paper or white on blue.
Out as PDF, SVG and DXF.

## Electronics

- **Schematics:** symbols (Proton's own, for the common parts, and any
  part made from a pin list), wires, labels, power symbols, buses; nets
  named; the electrical rules checked (an output driving an output, a pin
  left unconnected).
- **Footprints,** generated from IPC-7351 nominal land patterns: chip
  resistors and capacitors 0402–2512, SOT, SOIC, TSSOP, QFP, QFN,
  through-hole headers, USB-C, and fan and RGB headers — and the PC's own
  connectors, for the boards compact builds use: ATX 24-pin, EPS 8-pin,
  PCIe 6+2-pin, 12V-2×6, SATA power, 4-pin fan, 3-pin 5 V ARGB, and the
  USB 2.0, USB 3 and USB-C front-panel headers.
- **Current:** a net says what it carries (a 12V-2×6 adapter carries up to
  50 A), and its tracks and zones are checked for width against IPC-2152
  for the copper weight and the rise in temperature allowed.
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

Each lands with its tests passing, in this order: the parts (K0–K11), the
electronics (E0–E5), sheet metal and files read in (K12–K13); then the
person's build as the last test. K14 only if asked.

**Built** (2026-11-07): K0 — `crates/proton-cad`, with no dependencies: points,
directions and boxes; frames and rigid motions; one tolerance (10⁻⁶ mm,
10⁻⁹ rad, and 10⁻⁵ mm for fitted curves); the real roots of a polynomial,
double roots included; lines, circles, ellipses and B-splines, evaluated with
two derivatives, measured, and a point's nearest found (an ellipse's by
Eberly's bisection, which cannot fail); B-splines interpolated through points
on a banded solve; planes, cylinders, cones (one nappe), spheres and tori,
each with its parameters and its exact signed distance, which agree. Where
curves and surfaces meet: a line with each surface by formula (a torus's
quartic polished on the true distance), touches told from crossings; a
circle with a plane or a sphere as one trig equation; anything else by
sampling. Two curves by Gauss–Newton. Two surfaces by formula for every pair
that has one — a plane with each (lines, circles, ellipses, a cone's
generators through its apex, a torus's sections across and along its axis),
two spheres, any two surfaces about one axis (where their profiles meet), two
parallel cylinders, two of one radius whose axes cross (two ellipses) — and
the rest marched: seeds where a grid over each surface sees the other's
distance change sign, a predictor–corrector walk to the box's edge or round
to the start, and a cubic B-spline refitted until every midpoint is within
10⁻⁵ mm of both surfaces. Tested (28): every surface's points at no distance
from it and its normals its gradients; nearest points nearer than 20,000 of
the surface's own; points on, half a tolerance off, twice off and well off
told apart on every kind; lines' meetings counted against sign changes
sampled 400,000 times; tangents by formula; a tee of 10 and 5 mm pipes as two
closed loops whose length matches the integral to 10⁻⁶; five marched pairs
on both surfaces within the fit; random pairs of every kind. Measured
(release): a march 4–7 ms, about 65 poles a loop.

K1 — the `.part` language: the head (`part`, `units`, `material`, `param`
with expressions over earlier parameters — `+ − * /`, `sqrt`, `abs`, `min`,
`max`, and `sin`, `cos`, `tan` in degrees), sketches on `XY`, `XZ`, `YZ` or a
face with an offset, and features kept as written for the milestones that
build them; read and written back unchanged, comments and blank lines too;
each mistake named at its line and column. Sketches: points, lines, circles,
arcs, rectangles and splines, construction or not, and the nineteen
constraints and dimensions above. The solver takes damped least-length
Gauss–Newton steps, so what is not constrained stays where it was drawn;
which way a two-way constraint holds (a tangent's side, a dimension's
direction, an angle's sense, circles inside or outside one another) is
read from the drawing. A line or arc tangent at an end they share is held by
the radius being square to the line there — a residual that grows with the
error, where the distance form grows with its square and left a joint 6.5 µm
loose in testing. Then the Jacobian's rows, taken in order into an orthogonal
basis, say how many degrees of freedom are left, which points can move, and
which constraints add nothing (redundant); a sketch that cannot solve names a
smallest set of constraints that cannot all hold, by taking each out in turn.
Tested (19): a bracket file with every construct, 5,000 random expressions and
300 random sketches printed and read back the same; twelve mistakes at their
line and column; a dimensioned rectangle, a 3-4-5 triangle, a circle tangent
to both axes, a belt over two pulleys (its slope −asin 0.1), a slot of two arcs
and two lines (one degree of freedom left until its ends are equal, then
none), symmetry, midpoints, angles, offsets, a point on a circle, circles
tangent inside; degrees of freedom counted down a line's constraints;
redundant and conflicting constraints named, nothing else; parameters
driving a sketch; a solved sketch written back and solved again to the same
text — the incircle of a 3-4-5 triangle at (2, 1), radius 1. Thirty
dimensioned rectangles (120 points) solve in 24 ms in release.

K2 — solids. A profile is read from a solved sketch as a plane graph: every
node must join exactly two shapes, shapes meet only at ends they share (a
crossing, a touch, a stretch along each other, a dangling end are each named
with the shape and the point), and loops nest into regions by rays cast
analytically against lines and arcs. A spline in a profile is refused: its
side would be a free-form surface. Extrude and revolve are one sweep: each
node runs along a path — a line, or a circle about the axis — and each edge
sweeps a face between its start and end copies: planes and cylinders for an
extrude; planes, cylinders, cones, spheres and tori for a revolve. On the axis
a node is a pole (a cone's apex, a sphere's poles) and an edge sweeps
nothing; all the way round, the profile's edges become the seams of the
faces they swept, except a disc's or ring's, which a plane bounds with
circles alone. Each face is a surface, a side (reversed or not) and loops with
the face on their left; each edge is used once each way. The check: edges
end at their vertices and lie on every face that uses them; loops join up;
each edge has exactly two coedges, opposite; in each face's own parameters
its boundary runs anticlockwise and its holes clockwise, and no two of its
loops cross; each shell is connected and `V − E + F − H` is even and at most
2; the volume is positive. Every body a part builds is checked before it is
kept. Area, volume and centroid are integrated on the exact geometry — the
divergence theorem to each face, then Green's theorem in the face's
parameters to its edges, Gauss–Legendre along the edges' own curves — so
they hold to 10⁻⁹ against the formulas, not to the triangles. Tessellation
cuts every edge once (within the chord, turning at most the angle), so the
two faces beside it share its points and the mesh has no cracks; each face is
triangulated in its parameters by a constrained Delaunay triangulation of
its own (Lawson insertion, Sloan's constraint recovery, inside by parity) —
the periodic parameter cut at the seam, or where the face does not reach, the
loop at a pole run along the pole's line and the slivers there dropped; a
sphere or torus starts from a grid, a cylinder or cone is stretched round its
axis so triangles join nearest neighbours there, and any triangle further than
the chord from its surface is split at its longest splittable edge.
Out: binary and text STL, and 3MF (each body an object, shared points,
millimetres) in a stored zip of Proton's own with its CRC-32, the same bytes
for the same part. Tested (24): a plate with a hole; a cylinder, sphere, cone,
torus, tube and frustum turned (the frustum each way, from either side of
the axis, partly and all the way); quarter turns with their caps; a hole
turned all the way into a void, its own shell; 60 random plates with up to
three holes extruded in random frames, and 60 random profiles turned against
Pappus — half of them standing on the axis, about half turned only partly
round, either way; triangles facing
out, their normals agreeing, every point on its face and every centroid
within the chord; broken solids — open, a face turned over, a surface
flipped, a vertex moved, inside out — each named; 300 random polygons with
up to four holes triangulated to their exact area with every segment kept,
points added inside, a 21 × 21 grid on its own circles left Delaunay, a comb
of teeth a hundredth as wide as long; the plan's example built (its hole and
fillet noted for K4 and K5), inches, `XZ` with an offset, a peg on a plate's
top, symmetric and negative distances, revolves about a drawn line and by
angles either way; ten mistakes at their lines; STL and 3MF read back, and a
3MF written for the same bytes twice. Checked by hand: a 3MF opened by
`unzip -t` and Python's own zip and XML readers, closed and in millimetres. Measured (release): a 120 mm
fan grille of 53 holes checked in 14 ms, its mass in 11 ms, tessellated in
7 ms for the screen (5,312 triangles) and 15 ms to 10 µm for a printer
(10,208); a turned fitting to 10 µm in 6.5 ms.

K3 — the mode, on ⌃8 after Logic. The core's `cad` module holds the part
open: its text as it is and as saved, steps to undo, and the view it sends —
the steps (parameters, sketches, features, each with its mistake or why it is
not built yet), each sketch's shapes, constraints, dimensions and named
points where the solver put them with its degrees of freedom and its
profile's regions, and each body tessellated for the screen (points with the
exact normals of their face, triangles, each triangle's face, edges, volume,
area, centroid). The window's edits are lines of the language — `line l2
l1.end r.tr`, `distance a b = 25`, `extrude plate from base distance t` —
added, replaced or taken out by their place; a shape taken out takes its
constraints with it and leaves points drawn from it where they were; a sketch
or feature renamed is renamed where it is used; after each edit every sketch
is solved and written back, so the file says where its shapes are. A point is
dragged with the solver holding it where the mouse is, or letting it go to
where the constraints allow; one step to undo when the mouse lets go. Typing in
the Text tab is held too, keystrokes in a row one step; a line that does not
read is marked and what last built stays on screen. ⌘S saves, the quit prompt
names an unsaved part, Write STL and Write 3MF put the bodies beside the part to
10 µm. The window (`ui/cad.js`, `cad-view.js`, `cad-sketch.js`) lists the
steps, draws the part with three.js (faces named under the pointer, a cap
chosen to sketch on), and opens a sketch flat on its plane in SVG: lines in
runs, rectangles, circles, arcs and points, snapping to points already drawn
(by name) and holding a nearly level or upright line so; the constraints and a
dimension for what is chosen at what it measures now; dimensions typed over in
place; ⌫ takes things out; what can still move drawn in the accent, what
cannot hold in red. Tested: in the core (7), a part built by hand through the
window's edits — a sketch, a rectangle, a hole, dimensions, an extrude, the
hole dragged, its radius typed, a parameter — undone and redone, saved and read
back to the byte, reopened; edits for another part ignored; typing held, a
mistake keeping the last build; a shape taken out with its constraints; a
rename carried to its uses; STL and 3MF written and read back; the quit prompt
in the workspace. In the browser suite (9): ⌃8 and the chrome, the home page,
steps and their mistakes, a step's settings writing its line, a sketch added
and opened, lines, a rectangle, a circle and an arc drawn as their lines,
constraints and dimensions, a value typed over, a drag, ⌫, the text, ⌘S and
⌘Z, and the 3D view drawing the core's triangles; and 9 node tests of the
editor's arithmetic. Not yet looked at in the installed app or in WebKit.

K4 — booleans, and the features made of them. A boolean splits and
classifies: each edge of one solid is cut where it meets the other's faces
(an edge lying in the other's surface where it crosses that face's edges),
and each vertex lying on the other's faces or edges marks them; each pair of
faces' common curves (K0's) are cut at the points on both and kept where
inside both — a piece lying along an edge already there is that edge — and a
face lying in the other's surface is cut by that face's edges; a cut that
crosses a periodic face's seam is cut there too. Faces are split by tracing
their boundary and cuts as a plane arrangement in their parameters, each
piece told in, out or on the other solid (on the same side, or facing it) by
an exact ray, pieces joined by an untouched edge told together; the
operation keeps what it keeps, turning the other's pieces over where they
bound a hole, edges come to lie along one another are made one, and the
result is checked. Every feature keeps the tools it was made of — a profile,
a frame, a sweep — so a pattern makes them again moved and a mirror in a
mirrored frame, whose normal is turned so it stays right-handed and the sweep
run the other way. Along the way the triangulation learned to flip only into
triangles that turn clearly anticlockwise (a flip of a not-quite-convex
quadrilateral had folded two triangles over each other), to take a point at
an edge's end as that end, and to keep refinement clear of existing points;
the tessellator cuts finer the edges whose pieces cross in a sliver and cuts
every curved edge at least twice; a piece of a seam's line used once by a
fragment is placed as a seam is; the check makes sure on the curves of loops
that only look as if they cross. Tested (14 + 9): overlapping boxes, a hole
through a block, a boss on a plate, stacked, overhanging and flush boxes, a
hole across a tube's seam — volumes and areas to 10⁻⁷ of the formulas — and
140 random pairs of boxes, rods, balls and tubes at random angles, each
joined, met and cut, valid, closed, and adding up by inclusion and exclusion
(V(A∪B) + V(A∩B) = V(A) + V(B), V(A−B) + V(A∩B) = V(A)); cuts to a depth and
through, a join, an intersect, M3 tapped, blind with its point, counterbored
and countersunk holes, linear and circular patterns, a mirror and body
booleans against formulas; a sketch on `plate.side(l2)` found after the plate
is widened and said to be gone when `l2` is taken out; every new statement
read and printed back the same. Of 200 random pairs on another seed, one — a
ball grazing a tube's wall, two curves a micrometre apart — leaves a few
triangles unmatched. Measured (release): 49 holes patterned through a plate
and a boss joined, 0.71 s.

K5 — rounds, chamfers, shells, sweeps and lofts. A round is built on the
solid, not by booleans: in the plane across an edge each face is a line or
a circle, the ball's centre runs where both faces moved its radius meet (a
line or a circle: the spine), and where it touches them are the rails,
which take the edge's place; a chamfer's rails are walked a distance over
each face. Then each end: one edge at a corner of three faces runs its
rails on to the corner's other edges — cut back at a ridge, lengthened at a
valley — and bounds the third face by where the round meets it; two edges
at an angle are mitred where their rounds cross; two running on smoothly
(a line into an arc, a hole's rim into itself) share the ball's circle;
three edges of three planes get a piece of the ball, put on the sphere's
equator so no pole or seam is in it. A shell moves every face in (a plane
moved, a cylinder, sphere or torus's tube shrunk, a cone along its axis),
each edge to where its moved faces meet — moved with them where they meet
smoothly — and each vertex to where its edges meet, and cuts that from the
solid; a flat face taken away on its own is opened by a prism on the
inside's copy of it carried out through the wall, so a rounded box's rim
stands square, and faces taken away side by side are moved out with the
inside. A sweep is an extrude for each straight step of its path and a
revolve about each arc's axis — which lies in the profile's plane, the
profile standing square to the arc — joined where one ends and the next
starts, the two caps that lie on one another taken out; a path in space,
bending every way, sweeps as one in the plane does. A loft finds which edge
of one profile meets which of the other by turning one loop until every
pair is flat or about one axis, and builds planes and cones between them.
Tools that do not touch one another — a pattern's holes — are now made one
solid and cut at once. In the window an edge is found under the pointer
before a face (the nearest within six pixels with no face in front of it),
named by its faces, and picked to round or chamfer; a round's settings take
its edges, one a line, and an edge clicked is added to the round chosen; a
face clicked opens or closes a shell chosen. Tested (14): one edge, a box's
top mitred, every edge of a box rounded (against Steiner's formula, volume
and area), rounded corners and then a rounded top run on smoothly, a hole's
rim and a boss's foot (Pappus), chamfers of one and two distances, mitred,
and round a hole's rim; a tee, a round wider than its face, a smooth join,
three chamfers at a corner, nothing picked and a fillet patterned each
refused saying why; trays, closed shells (a void of its own), rounded boxes
shelled closed and open at their top; a tube along a bent path and a ring
round a closed one (Pappus), a sharp corner refused; a pyramid's and a
cone's frustums lofted, a square to a circle refused; an M3 hole's rim, an
L's inside corner, a hole through a round, two rounded blocks joined; and
at random, 120 boxes turned any way with half their edges rounded or
chamfered (every vertex's case, against the edges' sections less their
corners), 30 tubes bent every way in space, and 30 boxes shelled with faces
open at random — every one within 10⁻⁶ of its formula. Measured (release):
a box rounded all over 1.2 ms, and shelled open 26 ms; 64 holes patterned
113 ms (1.66 s a hole at a time), their 64 rims rounded 42 ms more; a tube
with two bends 7.7 ms.

K6 — STEP. A part's bodies are written as one product of ISO 10303-214
(automotive design), in millimetres: each body a `MANIFOLD_SOLID_BREP`, or a
`BREP_WITH_VOIDS` whose voids are closed shells turned over, each lump of a
body in pieces a solid of its own; every face an `ADVANCED_FACE` on its
exact surface (`PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`,
`SPHERICAL_SURFACE`, `TOROIDAL_SURFACE`) named for what made it, every edge
an `EDGE_CURVE` on its exact curve (`LINE`, `CIRCLE`, `ELLIPSE`, or the
`B_SPLINE_CURVE_WITH_KNOTS` two surfaces were fitted to meet in), numbers
with every digit they need to read back the same, the same bytes for the
same part. Proton's own reader takes the text of Part 21 — complex
instances, comments, doubled quotes and `\X2\` text, `1.E-06` — and makes
every solid in a file the kernel's again: lengths from the unit the
geometry's context names (SI prefixes, inches, feet), angles from radians
or degrees, edges run against their curves turned the kernel's way, curves
wrapped as a surface's unwrapped, loops without an outer bound sorted, poles'
vertex loops passed over; a free-form surface or a rational curve is refused
saying so (K13 shows them), and a solid that does not check is refused.
Write STEP is on the part's page beside STL and 3MF. Tested (5): fourteen
kinds of solid — a box, rounded all over, chamfered, a plate with holes and
a countersink, a turned part (planes, cylinders, cones, a torus), a ball, a
closed shell's void, a rounded tray, a bent tube, a tee (a B-spline edge
closed on itself), a slant cut (ellipses), a loft, two bodies — written, read
back valid with every face and its name, their volumes and areas within
10⁻⁹, and written again; the header and the product; Part 21 read as the
standard writes it; a cube in inches written the way other programs write
them, and in metres; a free-form face, no solids, a face turned wrong and
not a STEP file each refused. Measured (release): 64 rounded holes, 134
faces, 168 KB, written in 54 ms and read and checked in 92 ms. Not yet done:
a file opened in another CAD program by hand — none is on this Mac.

K7 — assemblies. An assembly's instances are read from their files and built
once until a file changes; an assembly inside another is built and placed
whole. Each instance not held still has six unknowns — a turn about its
middle and a move, from where it is placed — and each mate is equations
between the instances it holds: a flat face as its plane, a round one as its
axis (and where the face is centred along it), moved as the instances move —
normals opposed or aligned and a point's height over the other plane for
`planar`, `flush` and `distance`; directions crossed to nothing and a point's
offset from the other axis crossed to nothing for `cylindrical`, with the
centres' heights along it for `revolute` and the faces' x crossed for
`slider`; a dot product for `angle` and `perpendicular`; three points carried
together for `fastened`. They are solved by the sketch solver's damped
least-length steps, carried on toward 10⁻¹³ so mated faces lie on each other
and not a few tenths of a nanometre apart; what no mate holds stays where it
was. The rows say how many freedoms are left — a row smaller than 10⁻⁸ is
rounding, not a gradient, and adds none — which instances can still move,
and which mates add nothing; when the mates cannot all hold, a smallest set
of them is named. Every placement the solver moved is written back, so the
file says where everything is, and solved again it does not move. The fit
check cuts every part into triangles as fine as the clearance asks and walks
two trees of boxes for each pair: triangles that cross, or a part inside
another, overlap, by the exact intersection's volume; faces lying on one
another overlap nothing; a gap under the clearance is named with its two
nearest points. The bill of materials counts each part file. In the window an
assembly lists its instances, mates and views; two faces picked on two parts
in the view are mated by a button for each kind; an instance's place, turn,
holding still and name, and a mate's kind, ends and value, write their lines;
a part of the folder is added by name; Check fit lists what it finds; an
exploded view moves its parts apart in 3D; STEP, STL and 3MF write every part
where it is. Tested (12 + 2 + 1): the language read and printed back with
every statement and seven mistakes at their lines; a planar mate seating a
block and leaving it three freedoms; three mates holding it in a corner
exactly; a pin in a hole — cylindrical (two freedoms), revolute (centred, one),
slider (one); gaps, angles and fastenings; mates that cannot all hold named,
one that adds nothing marked, mistakes at their lines; placements written
back and solved again to themselves; an assembly inside another, its parts
mated to and counted in the bill of materials, and one that holds itself
refused; overlaps measured exactly (400 mm³), a 0.5 mm gap found, touching
and far apart passed; an exploded view; a flush offset's sense; in the core,
an assembly made, mated, renamed, checked, written as STEP and read back,
saved, reopened, and a part changed on disk built again; in the browser, the
assembly's page, faces picked and mated, an instance placed, held, renamed
and taken out, a mate's kind changed, a part added, the fit checked, a view
taken apart, an assembly made from the home page. Measured (release): 26
parts held by 75 mates solved in 10 ms, and their fit checked in 24 ms — 20
overlaps of 80 mm³ found, as set.

K8 — the parts library and the FormD T1. A library part is made by a family
with its parameters and written into the project's `library/` folder as a
`.part` file of the language — read back and printed the same, built and
edited like any other — from the home page, or made and added to the
assembly open. The families: a Mini-ITX board (170.18 mm square, ATX 2.2's
holes C, F, H and J at (6.35, 10.16), (163.83, 33.02), (6.35, 165.1) and
(163.83, 165.1) mm from its rear-left corner, ⌀3.96 — read off Figure 3 of
the specification: hole C 4.900 in from hole A, F 11.100 in and 0.900 in
forward, H and J 6.100 in forward — and its rear I/O stack inside ATX's
158.75 × 44.45 mm aperture); SFX and SFX-L power supplies (125 × 63.5 × 100
and 130 deep, SFX12V); M.2 drives (22 wide, 2230–22110); memory (133.35
long); graphics cards by their published length, height and slots of
20.32 mm, the bracket full height (120.02), with G1/4 ports for a water block;
fans of 80, 92, 120 and 140 mm, 15, 25 or 38 thick, holes ⌀4.3 at 71.5,
82.5, 105 and 124.5 mm; radiators of one to three 120 or 140 fans, M3 holes
for their fans and two G1/4 ports, the end tanks' length and the ports'
spacing parameters to check against a maker's drawing; a CPU block's and a
D5's or DDC's envelope, with a reservoir; G1/4 fittings (13.157 mm thread) —
straight and elbow for 14/16, 12/16 and 10/13 tube, extenders and plugs, the
socket a named face to route from; ISO 4762 socket, 7380 button and 10642
flat-head screws M2–M5; ISO 4032 nuts; 6-32 and M3 standoffs. The FormD T1
is NCASE's published figures (the T1 Sandwich Kit's page, read 2026-11-09):
13.5 × 22 × 33.5 cm — 9.95 L — shelled to a wall the person measures, with
the source cited in the file; where the board, card, spine and power supply
sit inside is not published, and the file says so. Its published limits are
a check: a card no more than 140 × 325 mm and 3.25 slots; the CPU cooler no
higher than 113 mm less 20 a slot (NCASE's table, 48 mm at 3.25 slots to
88 at 1.25); one 120 or 240 radiator, or with the bracket kit two 240s or a
280 and a 240, the pairs only with a single-slot card and a 280 on the side;
a 240 with its fans no more than 46 mm thick (V2.0); SFX-L cautioned against
with water cooling. NCASE publishes no model of the case (its 3D files are
accessories) and its manual's spacings are pictures, so the inside waits for
the person's measurements. Tested (9 + 1 + 1): every family made with every
choice, read back the same, built, checked closed and drawn; the board's
holes against ATX's inches; supplies, drives and memory against their sizes;
every fan's holes and volume; a radiator's length, width, holes, ports and
threads; a G1/4 thread, every screw head's ISO size, a nut's flats, a
standoff's; a card's size and slots; the T1's size, 9.95 L and walls; NCASE's
cooler table and every rule; in the core, a library part written, made again
to the same file, opened, and added to an assembly by its path from there;
in the browser, a family chosen, a value changed, made, and made and added.

K9 — routes. `route NAME hard|soft ID/OD from PORT to PORT [via (x, y, z), …]
[bend R] [lead L] [insert I]` runs a tube between two fittings' sockets,
written the way the coolant goes. A port is an instance's hole by name,
`cpu_fit.socket` (its wall, `socket.side(wall1)`), or a round face: its
point is the centre of the socket's mouth — the circle a flat face square to
it meets — and its way out that face's outward normal, where the mates put
the instance. The centre line runs straight out of the first socket for its
lead, through the points given, and straight into the last; each corner is
bent to the radius (hard tube 2.5 × its outside diameter, soft 4 ×, unless
given), a corner needing more straight than it has refused with how much,
and the tube is its ring swept along the line — cylinders and tori, exact,
by K5's sweep, in space. A hard tube's cut list is its length along the
centre line with 10 mm (or `insert`) pushed into each fitting; its bend list
gives each bend's start along the tube as cut, its angle, its radius, and
its twist about the tube from the bend before. Tubes are parts of the
assembly: checked for fit like any (their ends on their fittings touch, and
fit), drawn, written to STEP, and counted in the bill of materials with the
tube to cut in all. The loop is the routes followed from the first's start,
part to part, in the order the coolant meets them, and says where it stops
or which routes are off it. In the window two sockets picked in the view
route a hard or a soft tube; a route's settings take its tube, its ends, its
points one a line and its radius, and list its bends; the assembly's page
draws the loop as a ring, arrows the way the coolant goes, each tube with
its length to cut. Tested (7 + 2 + 2): a straight tube's length, cut and
volume; an L bent at 40 — 60 + 20π + 88 mm, its bend 70 from the end as cut
— and at 30 with 12 pushed in; two bends in planes square to each other
twisted a quarter turn, a U none, an S a half; bends too close, a tube too
big for its socket and a port that is not one refused; a loop closed in the
coolant's order and one open, said so; a tube through a block found by the
fit check, two clear of everything not, and the tube to cut counted; routes
read and printed back, and four mistakes; in the browser, a tube routed
from two sockets picked, and a route's bends listed and its points written.

K10 — drawings. A `.drawing` file is what a sheet shows, one statement a
line: `drawing "T"`, `of "FILE"` (a part or an assembly), `sheet A4…A0,
ANSI_A…ANSI_D`, `scale 1/2`, `angle first|third`, `style paper|blueprint`;
`view NAME front|back|top|bottom|left|right|iso at (x, y) [scale S]
[hidden] [exploded NAME]`; `section NAME along x|y|z = V at (x, y) [from
+|-]`; `detail NAME of VIEW at (x, y) radius R scale S place (x, y)`; `dim
NAME VIEW distance|diameter|radius|angle|clearance A [to B] [offset D]`;
`note NAME VIEW "text" at A`; `balloons VIEW`; `bom at (x, y)`. A view is
the solids projected: every edge sampled along its own curve, and a curved
face's outline found where its tessellation folds away from the eye
(chained into runs); each sample seen if a ray from it to the eye meets no
triangle (a tree of boxes), so a line is cut where it passes behind
something, and dashed there if the view shows hidden lines. Seams are not
drawn; a smooth join only where it is the outline, edge on. Lines on each
other are drawn once — a box's bottom edges under its top's, a hole's ends
seen edge on along the outline — and a hidden line only where nothing seen
is on it. A hole seen end on is a circle, kept a circle to the DXF for a
machine to drill; round faces more than 135° about their axis get centre
marks or axes (holes and bosses, not rounds). A section cuts by Proton's
boolean and hatches the cut faces at 45°. The solids are cut, tessellated
and put in the ray tree once, for all the views that show them alike.
Dimensions are measured on the model — a distance between parallel faces,
a round face's diameter or radius, the angle between faces, an assembly's
parts' nearest gap — and drawn outside their view with arrows and
extension lines, the view's name under them. The frame, a title block with
the projection symbol, the bill of materials as a table, and balloons
numbered as its rows, out from the view's middle and clear of each other.
`standard` lays out a part's front, top, right and isometric views on A3 at
the scale that fits; `blueprint` an assembly's on A2: four views, an
exploded one, a section through the middle, balloons, the bill of materials
and the six tightest gaps measured, on blue if asked. Written by Proton's
own writers: SVG; PDF 1.4 (Helvetica, WinAnsi for Ø ° ×, the line kinds'
dashes); DXF R12, a layer and line type for each kind of line. In the
window a part's settings draw it, and an assembly's make its blueprint,
as `NAME.drawing` beside it (one there already is opened); the drawing's
page shows the sheet to zoom and pan, lists its views, dimensions and notes
with what could not be drawn, takes a view dragged to move it, adds views,
sections, dimensions (faces offered by name) and notes, and writes PDF, SVG
and DXF beside it. Measured (opt-level 2): a 120 mm plate with 64 rounded
holes on A3, four views and a section, in 0.59 s — 797 lines and circles,
SVG 161 KB, PDF 149 KB, DXF 272 KB written in 8 ms. Tested (9 + 2 + 3 + 3):
the six square views and the isometric against the box's corners projected
by hand, each edge once; a hole's circle end on, its sides dashed behind
from the front, its axis past the plate; a section hatched only where it
cuts, at 45°, not in the holes; dimensions saying 100, 60, 4, Ø4.4, R2.2
and 90°, and what cannot be measured said at its line; the language read
and written back, and seven mistakes; SVG, PDF (every object where the
cross-reference says, the stream's length, Ø as \330) and DXF read back,
entity for primitive, layer for line kind; a part's drawing within its
sheet, its views apart; a blueprint's views, balloons as many as the bill's
rows and clear of the view; in the core, a part drawn, edited, a mistake
kept on the last sheet, written as PDF, SVG and DXF, and drawn again opened;
an assembly's blueprint on blue; in the browser, drawings asked for, the
sheet shown, a view dragged and moved, zoom about the pointer, the sheet's
settings, a view, a dimension and a note added, written out, a statement
typed and taken out, what could not be drawn marked.

K11 — the agent. `cad_read` gives the file open in CAD (or `path`) with its
lines numbered, what it builds — each body's volume, area, centre and size;
each sketch solved and how free; an assembly's instances, where they are
and what holds them, its mates and freedoms, its bill of materials and
tubes; a drawing's statements — and every mistake at its line, with the
project's .part, .assembly and .drawing files. `cad_write` writes a file
whole or replaces one passage in it, opens it in CAD and holds it until the
person saves, as typing in the Text tab is, and answers the same way; it
will not change another file while one is open with unsaved changes. Its
description teaches the three languages in brief. `cad_measure` measures
every body, a body or instance, a face (its kind, diameter, axis, exact
area), two faces (flat faces' gap or angle, axes' distance, an axis's
distance from a face) or two bodies or instances (the nearest they come, or
that they overlap); a name it cannot find is answered with the names there
are. `cad_check` says a part's sketches solved and bodies closed; an
assembly's mates (what conflicts, what is free), its fit — shown on its
page too — its tubes and loop; a drawing's statements that cannot be drawn;
and with `t1` the build against the FormD T1's published limits. `@cad`
sends the file open and what it builds. CAD's inspector has an Agent tab,
with the composer and the `@cad` chip; its keys are its own, not CAD's.
`mass::face_areas` gives each face's area exactly. Measured, dev build
(proton-cad at opt-level 2): on a plate with 64 rounded holes, `cad_write`
0.46 s with the part's view sent, `cad_read` 0.2 s, `cad_measure` 0.2–0.44
s, `cad_check` 0.2 s. Tested (1 + 1 + 1 + 1): in the core, through
`run_tool` only — nothing open read, a part written (made, held, not on
disk), changed and its volume with it, a mistake at its line and mended, a
face's area exact, a hole's diameter, two faces' gap, an axis from a face, a
name not found answered with the names, the part checked, another file
refused while one is unsaved, an assembly written with its bill of
materials, an overlap of 400 mm³ found, two parts' and two faces' distance,
a card too long for the T1, `@cad`'s context, a drawing's statement that
cannot be drawn said at its line, a path outside the project refused; the
tool list's count and that `cad_write` says it holds; `@cad` found and
`@cadence` not; in the browser, the Agent tab, the chip, and keys typed in
the composer left alone. The status page is brought up to date once CAD is
finished (E0–E5, K12, K13), not at each milestone.

E0 — schematics. `crates/proton-board`, which uses `proton-cad` for its
sheet and nothing else. A `.schematic` is `schematic "T"`, `sheet A4`, `part
REF KIND [value V] [footprint F] [at (x, y)] [turn 0|90|180|270] [mirror]`,
`net NAME [power] REF.PIN, … [current A]` and `nc REF.PIN, …`; a pin by its
number or its only name, `J1.PWM`; `#` after a space a comment, so `PS_ON#`
is a name. The library: resistor, capacitor (and polarised), inductor,
diode, LED, zener, fuse, switch, crystal; NPN, PNP and N- and P-channel
MOSFETs in SOT-23's order; headers of one or two rows; any chip from its
pin list, `ic "NE555" pins (1 GND power-in, …)`, each pin input, output,
bidir, tristate, passive, power-in, power-out, open-collector, open-emitter
or nc; and the PC's connectors — ATX 24-pin, EPS 8-pin, PCIe 6- and 8-pin,
12V-2×6, SATA power, the 4-pin fan, 5 V ARGB, and the USB 2.0, USB 3 and
USB-C front-panel headers — each with where its pinout is from. The 8-pin
PCIe's (+12 V at 1–3, SENSE1 at 4, SENSE0 at 6) is NVIDIA's power
guidelines' table 2 and the USB 3 header's Intel's internal connector
specification (ID at 10), found 2026-11-09. Two could not be checked: the
12V-2×6's pin numbers and its sideband's S1–S4 (PCI-SIG's ECN is for
members; Wikipedia gives only the sense pins' power table) and the USB-C
Key-A header's (only a summary of Intel's NUC signal table was read) — the
rules check warns on every schematic that uses them until they are. The
rules: a reference twice; a pin not there (answered with the part's pins),
on two nets, or marked unconnected and connected; two outputs on a net; a
power input nothing on the board supplies and not said `power`; an input
nothing drives; a net of one pin; pins left unconnected without `nc`. The
sheet draws each symbol where it is placed (set out in rows where not),
turned and mirrored, its pins numbered and named, its reference and value
beside it, and each pin's net written at its end — a label, so no wire is
routed by hand — and unconnected pins crossed; with CAD's sheet, so it is
written as SVG and PDF as a drawing is. The netlist is KiCad's `.net`
(version E) and the bill of materials CSV, alike parts on a row. In CAD a
schematic is made from the home page, listed with its parts, nets and
unconnected pins (errors marked, warnings dotted), its rules check on the
right, a part chosen shows its pins and their nets; parts and nets are
added from the library by form, the sheet's size set, a part dragged on
the sheet moved, and it is written as PDF, SVG, netlist or bill of
materials. The agent's tools read, write and check schematics, their
language in `cad_write`'s description. Tested (6 + 1 + 1 + 2 + 1): the
language read and written back, and ten mistakes; every connector pin for
pin; a SATA-powered hub of five fans through a buffer built clean, a pin
found by name; every rule at its line; the sheet's labels one to each pin
on a net, a resistor stood on end, the sheet written three ways; the
netlist's parts and nodes and its brackets balanced, the bill grouping
five fans; in the core, a schematic made, edited, a mistake keeping the
last that read, written four ways, opened from the home page; through the
agent's tools; in the browser, its lists and rules, a part dragged and
moved, its pins, a part and a net added, a bad name refused, the sheet's
size, written out, made from the home page.

E1 — footprints. `proton_board::footprint`: surface-mount lands by
IPC-7351B's equations — Z = Lmin + 2Jt + √(CL² + F² + P²), G = Smax −
2Jh − √(CS² + F² + P²), X = Wmin + 2Js + √(CW² + F² + P²), S's tolerance
the root of its parts' squares, F 0.10 and P 0.05 as KiCad's generator
takes them, Z and X rounded up and G down to 0.05, a 0.25 courtyard — at
the nominal density: chips 0603 and up toe 0.35, heel 0, side 0, and
gull-wing leads 0.35, 0.35, 0.03, as published second hand (the standard is
sold); chips smaller than 0603 and QFNs from goals remembered, and said to
be by a caution the rules check repeats. Chips 0201–2512 from typical
thick-film dimensions; SOT-23 (and -5, -6), SOIC-8–16, TSSOP-8–28,
LQFP-32/44/48/64/100 and QFN-16/20/24/32/48 from JEDEC's outlines, numbered
as their packages are (QFNs with their exposed pad); pin headers of one or
two rows at 2.54, 2.0 or 1.27; Mini-Fit Jr. at 4.2 mm and KK 254 at 2.54,
cautioned until checked against the makers' drawings. A part takes its
line's `footprint`, or its kind's own — ATX and EPS and PCIe a Mini-Fit
Jr.'s, a fan a KK 254's, the USB headers pin headers — and the rules check
says which pins have no pad and which pads no pin (a key needs none). A
footprint is written as KiCad's `.kicad_mod` (its y down), every part's
together as `NAME.pretty` beside the schematic, and drawn for the window:
a part chosen shows its footprint, its pads and where its lands are from.
Tested (5 + 1 + 1): a 0603's lands against the equations worked by hand
(pads 0.90 × 0.95 at ±0.775) and an SOIC-8's (2.0 × 0.6 at ±2.475, where
KiCad puts them), every chip and package sound — pads apart, inside the
courtyard — and numbered as its outline, SOT-23-5's missing pin, QFN's
exposed pad; headers' holes and pitches, Mini-Fit Jr.'s; pins against pads,
a footprint unknown; the KiCad file and the drawing; in the core, a fan's
footprint sent and the library written; in the browser, a part's footprint
drawn.

E2 — the board. A `.board` is made from a schematic — `board "T"`, `of
"hub.schematic"`, `layers 2|4`, `copper OZ`, `rules clearance C track W via
PAD/DRILL edge E rise °C`, `outline rect (x, y) size (w, h) [corner r]`,
`outline points …` or `outline from "case.part" sketch NAME` (the board cut
to a part's sketch, its holes too, so it fits where it goes), `hole at (x,
y) drill D`, `place REF at (x, y) [turn 90] [back]`, `track NET LAYER [width
W] (x, y) …`, `via NET at (x, y) [size P/D]`, `zone NET LAYER (x, y) …
[clearance C] [thermal T]` — mm, y up, KiCad's layer names. Built, each
part's footprint is set where it is placed (turned, mirrored to the back),
its pads take their nets from the schematic, and every piece of copper is a
point, a segment or a polygon with how far it reaches. Copper of one net
that touches is joined; of two, a short; nearer than the clearance, said
with how near; a zone joins its own net's copper that touches it, poured
round others' by its clearance with spokes to its pads. Each net's pads
still apart are joined by the shortest connections (the ratsnest), said to
be routed or not. Copper off the board or nearer its edge than the edge
rule, a track narrower than the rule or than IPC-2221's width for its
net's current (I = k·ΔT^0.44·A^0.725, k 0.048 outside and 0.024 inside, the
inside at 0.5 oz; IPC-2152 has only charts, and 2221's is the more
cautious inside), a via's drill under the rule, a layer the board has not,
a net or part the schematic has not, a part with no footprint, not placed
(set out under the board till it is), past the edge, or over another — each
said at its line. A first board is laid out from the schematic open: its
parts in rows on a board sized to them, two layers, the usual rules, and
it breaks none. Drawn from the front: the edge, each layer's copper (the
back's under), zones with spokes, holes, courtyards, names, the ratsnest
dashed. In the window parts are dragged where they go, and the track tool
draws from a pad, on its net, through corners snapped to a quarter
millimetre, to a pad (or a double-click), on the layer and at the width
chosen, ⎋ letting go; the via tool puts vias. The agent writes boards too,
and is told every pad's place and net. Tested (6 + 1 + 1 + 2): the language
read and written back, nine mistakes; IPC-2221's width worked by hand (1 A
0.30 mm, 4 A 2.03 mm on 1 oz outside at 10 °C); a fan hub's board routed
clean, its ground joined by a zone, three connections left as the ratsnest,
+12V's width for 2 A; a short, a clearance of 0.115, the edge, a small
drill, a missing layer, an unknown net, a thin track and a track too thin
for its current, a part with no footprint, one not placed, two over each
other; an outline cut from a part's sketch with its hole, a via in the hole
off the board; the drawing; in the core, a first board made from a
schematic breaking no rule, a track routing a connection, a part moved,
written as SVG, made again and opened; through the agent's tools; in the
browser, its lists and crumb, a part dragged, a board laid out from a
schematic, a track drawn pad to pad on the pad's net, ⎋, a via.

E3 — for a fab. `proton_board::gerber`: Gerber X2 (`%FSLAX46Y46*%`, mm,
each file's function and polarity said) for every copper layer, both
solder masks, both pastes, both silkscreens and the edge; Excellon for the
plated and the unplated holes; the bill of materials and the placements
(reference, value, package, x, y, turn, side) as CSV. A zone is a region,
then every piece of copper near it cleared from it by its clearance (clear
polarity) — another net's, and a ring round each of its own net's pads —
then four spokes across each ring, then all the copper drawn over; pads are
flashed, rounded ones as their rectangles (as the rules check them); masks
open 0.05 round each pad and tent vias; the silk is each part's outline and
a dot by its first pad (no letters yet). Proton's own readers read Gerber
and Excellon back, refusing a file with no end, an aperture not defined, a
coordinate before the format, a line not ended, a region not begun. In CAD
a board writes all thirteen files into `NAME-fab/` beside it, saying what
is not routed yet — and will not while it breaks a design rule. Tested (5 +
1 + 1): every file named, read back and saying what it is; the front's
pads flashed where the board has them and its tracks at their width; the
back's zone, the ten pads it is cleared round, its twelve spokes and the
through pads over it; the masks' openings, the paste, the silk, the edge;
the drills, plated and not, the placements and the bill; five malformed
files refused; in the core, a first board written for a fab and read
back, and a board with a short refused; in the browser, written, and not
while a rule is broken.

E4 — the board in 3D. `proton_board::three_d`: a board written as a
`.part` — its outline sketched, each line starting where the last ends so
it closes exactly, its cut-outs and every hole drawn in it, extruded to its
thickness; each placed part a box the size of its footprint's body and its
package's usual height (a chip 0.3–0.6, SOT-23 1.1, SOIC 1.75, TSSOP 1.2,
LQFP 1.6, QFN 0.9, a pin header 8.5, a KK 254 11, a Mini-Fit Jr. 13), on top
or, for the back, under it — for fit, not for drawings of the parts. So the
board is built, drawn in 3D, written as STEP and placed in an assembly as
any part is: a hub in the case it was cut to fit. In CAD a board writes
`NAME-3d.part` beside it. Tested (3 + 1 + 1): the board's volume — its
outline less its holes, its rounded corners within their chords — and
thickness, checked whole; each box's height and place, on top and under,
turned; written as STEP and read back, five bodies; placed in an assembly
on a tray; in the core, a board's 3D part written and built; in the
browser, asked for.

E5 — KiCad's libraries. `proton_board::kicad`: an S-expression reader;
`.kicad_sym` symbols — their pins' numbers, names and electrical types
(`power_in` a power input, `no_connect` not to be connected, `unspecified`
and `free` passive), a symbol that `extends` another taking its pins, set
out as KiCad sets them, left pins down the left; `.kicad_mod` footprints —
pads' numbers, shapes, sizes, turns and drills (unplated holes are not
pads), the courtyard's and body's boxes from their layers, KiCad's y turned
up. Libraries are found in the project — any `.kicad_sym` file and
`.pretty` folder three folders down — and in KiCad's own folders wherever
it is installed (macOS, Linux, Windows). A schematic takes `part U1 kicad
"Device:NE555P"` and `footprint Library:Name`; a library or a part not
found is said, with what was looked for. KiCad is not installed on the Mac
this was built on, so its files are tested as written in its format, and
Proton's own KiCad footprints are read back the same. Tested (5 + 1): the
S-expressions, strings escaped, three mistakes; a symbol library — a
resistor, one extending it, the 555 with its pins' types and sides; a
surface-mount and a through-hole footprint; five of Proton's footprints
written and read back pad for pad; a project's libraries found and a
timer's schematic of KiCad's parts built clean, and said to want them
without; in the core, a project's own library's fuse and footprint used.

K12 — sheet metal. `sheet NAME from SKETCH thickness T bend R width W [k
K] [side left|right]`: a sheet bent along the sketch's run of lines — a
bracket's L, a channel's U, a Z — its material to their left (or right),
each corner bent to the inside radius R. The face on a bend's inside turns
on R, the other on R + T, about one centre, so a sketched corner that is
the bend's outside is rounded to R + T and one on its inside to R; lines
too short for their bends, lines running straight on, a fold back on
itself, an arc in the run and a K-factor past 0–1 are refused, a short line
with how much its bends need. Extruded W across, it is a solid like any,
its faces named for its lines (`NAME.side(bend1)`, `bend1b` the other
face). Flat, a bend takes its neutral line: its allowance θ·(R + K·T), K
0.44 unless given; the flat length is the straights between the bends and
the allowances — from outside dimensions, their sum less each bend's
deduction 2·(R + T)·tan(θ/2) − BA, as bend tables give it. The flat pattern
is written as DXF — the outline on VISIBLE to cut, each bend's line on
CENTER, each bend's angle and radius said — beside the part, and listed in
its settings; a Sheet button bends the sketch chosen. Not yet: a flange
added on an edge of a sheet already made, holes in the flat. Tested (5 + 1
+ 1): an L checked whole, its volume its section (straights and a quarter
ring) across its width, its flat 30 + 20 less the deduction, its straights
and bend; a table's 90° bend at T 1, R 1, K 0.33 (BD 1.9108); at K 0.5 the
flat length the volume over the thickness; a U with its material outside,
a Z bending both ways, a 30° bend; four refusals; the flat as DXF and the
language read back; in the core, a bracket's step and flat listed and its
DXF written; in the browser, a sketch bent and a flat written.

K13 — other programs' files. `proton_cad::import`: an STL, binary (known by
its size, whatever its header says) or text, welded at a ten-millionth of
its size, every edge two triangles' or refused with how many are open,
each flat run of triangles — joined at an edge, on one plane — made one
face bounded by its loops, so a box is six faces to mate to and a curve the
triangles it was cut into; a STEP's unnamed faces named `face1` … in order.
A maker's `.step` or `.stl` is a part of an assembly like any — `part pump
from "d5.step"` — measured, checked for fit and mated to; read through the
assembly's loader, a binary file a character a byte. A DXF's lines, arcs,
circles and polylines — R12's and later's, their bulges as arcs, in inches
if `$INSUNITS` says — are drawn into a part as a new sketch, its own
numbers, so its outline closes where its ends meet. In CAD the home page
lists STEP, STL and DXF files; a STEP or STL opens read-only — its bodies
shown and measured, edits refused with why — and a part's settings draw a
DXF in. The agent's tools read and measure them. Tested (5 + 1 + 1): a
box's STL six faces, whole, its volume; a peg's ends one face each, its
volume its mesh's; a text STL; a mesh with a hole refused; a STEP's names
kept, and another's named; a STEP and a binary STL placed in an assembly
and mated flat on a plate; a DXF's outline and hole extruded to their
volume, a bulged polyline in inches; in the core, imports listed, an STL
opened read-only and refused an edit, a STEP's names, a DXF drawn into a
part and extruded; in the browser, an import's steps hidden, a DXF drawn
in, and an assembly offered imports but not DXFs. No maker's file is kept
in the repository: they are tested as Proton writes them and as other
programs' style has them.

The final test (2026-11-10) — the person's FormD T1 build with a custom
loop, in `~/Proton/CAD/t1-build`, and what it found. Noctua's STEP of the
NF-A12x15 (2.8 MB, 1,066 faces, the first maker's file read) was refused
whole: its blades are B-spline patches, its edges on them rational
B-splines, its solids placed by an assembly, no bound said to be outer,
cylinders with no seam, edges 0.025 mm off their vertices. Now:
`freeform` keeps a B-spline or NURBS patch once, and `Surface::Free` names
it with a frame, so a surface stays a small value copied freely; it is
evaluated, projected (seeds, then Gauss–Newton), meshed (four pieces a
knot span, split to the chord), drawn, fit-checked and written back as
`B_SPLINE_SURFACE_WITH_KNOTS`, and booleans, rounds and shells refuse a
solid with one, saying why. A rational curve is a cubic through its points
to half the file's tolerance. An assembly's solids are placed by their
`ITEM_DEFINED_TRANSFORMATION`s up to the top, one read for each place,
named by their product. A face whose bounds are all `FACE_BOUND` takes the
one enclosing most as its boundary. A solid read is checked to the coarser
of 0.05 mm and the file's tolerance (`check_read`): closed, manifold, each
edge on its faces, and a face of one loop running its own way — not
slivers, or loops a file need not order. A face the exact triangulation
cannot take — a band round a cylinder with no seam, a boundary touching
itself — is cut by ears from its own boundary points (`mesh::fallback`),
opened at a cut through none of its holes, no triangle reaching more than
a few of its boundary's steps round, wound once for the face. The fan reads
in 0.2 s and meshes closed in 1.5 s (frame 51,414 mm³, impeller 23,201,
eight pads, the label). Also from the build: the loop is followed through
a part and those fastened to it as one stop, and parts fastened together
are too near only if they overlap (`Model::clashes`); the bill of
materials' columns are as wide as their names (the part's up to 120 mm,
then set smaller); balloons stay inside the border; a hole's centre mark
or axis is drawn only where some of it is seen, unless the view shows
hidden lines; a hole off its body is refused, saying a face's (x, y) are
its own; fans 14 mm thick. Tested (4 STEP + 1 surfaces + 5 freeform + 2
fallback + 2 route + 2 drawing): a cube with a patch for its top read,
meshed to its volume, refused a boolean and written back; a cube placed
twice by an assembly; a plate whose bounds do not say which is outer; a
tube with seamless walls meshed closed; patches' identities, rational and
not; a strip and an L cut whole by ears; a loop through two blocks and
their fittings; fittings fastened near their block not too near, sunk
into it overlapping; a blind hole marked only from where it is seen; a
long name widening the table, and balloons kept on an A4.

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

- **The FormD T1's inside.** NCASE publishes its outside (13.5 × 22 × 33.5
  cm, 9.95 L) and its limits, now in the library; where the board, the card,
  the spine and the power supply sit is not published (no model; the
  manual's spacings are pictures), so the person measures them — or NCASE's
  current revision (V2.5) is assumed only where the person says so.
- **The parts in the build** come as a PCPartPicker list with the loop's
  pieces and tubes, after CAD is finished.
- **Splines in profiles** are refused (K2): extruded or turned, a spline
  sweeps a surface of linear extrusion or of revolution, which the five
  analytic surfaces do not include. They can be added as two exact swept
  surfaces of their own if curved panels are wanted.
- **Surfaces that nearly touch.** Two faces meeting at a near tangency — a
  ball grazing a cylinder — leave slivers whose triangles can fail to match
  (1 in 200 random pairs). A boolean that cannot be made says so and leaves
  the part as it was.
- **Rounds and chamfers meeting four or more to a corner**, rounds of two
  sizes meeting, three chamfers at a corner, and a round next to a fitted
  curve are refused, naming the corner. An edge is named by its two faces,
  so where a boolean has left two edges between the same two faces,
  picking one picks both.
- **A shell's corners are of three faces**; walls thicker than a face is
  wide, or than a round is curved, are refused. A curved face taken away
  that meets its neighbours smoothly is refused too.
- **Lofts** are between one-region profiles without holes, every face flat
  or about one axis.
- **3MF is stored, not compressed**: about 60 bytes a point and 45 a
  triangle. A deflate of Proton's own would make it a fifth of that.
