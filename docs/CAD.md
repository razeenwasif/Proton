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

- **The FormD T1's figures** are looked up from what FormD publishes when
  the build is made (2.1 assumed until then); the person measures where
  nothing is published.
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
