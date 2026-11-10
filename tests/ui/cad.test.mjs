// CAD's sketch editor's arithmetic, without a window: names given, numbers
// written as the file writes them, and the lines of the language the mouse
// and the constraint buttons make.
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {num, round, fresh, level, rectText, holdText, dimensionText, toggleConstruction, splinePoints, spot} from '../../ui/cad-sketch.js';
import {stepText, placeable, volumeText, onFace, unitScale, lines, toggled} from '../../ui/cad.js';
import {PART} from '../../ui/preview-cad-part.js';

const base = PART.sketches.find((s) => s.name === 'base');

test('numbers are written to a nanometre, as briefly as they read', () => {
  assert.equal(num(80.00000000000001), '80');
  assert.equal(num(-0.0000000001), '0');
  assert.equal(num(1 / 3), '0.333333333');
  assert.equal(round(12.34567), '12.346');
});

test('a name is the first of its kind not taken', () => {
  assert.equal(fresh('l', ['l1', 'l2', 'r', 'l4']), 'l3');
  assert.equal(fresh('s', []), 's1');
});

test('a line drawn nearly level or upright is held so, and only nearly', () => {
  assert.equal(level([0, 0], [10, 0.3]), 'horizontal');
  assert.equal(level([0, 0], [-10, 0.2]), 'horizontal');
  assert.equal(level([0, 0], [0.2, -10]), 'vertical');
  assert.equal(level([0, 0], [10, 1]), null);
  assert.equal(level([0, 0], [0, 0]), null);
});

test('a rectangle is written from its bottom left, by name when one is there', () => {
  assert.equal(rectText('r2', {at: [30, 20]}, [10, 5]), 'rect r2 corner (10, 5) size (20, 15)');
  assert.equal(rectText('r2', {name: 'origin', at: [0, 0]}, [8, 4]), 'rect r2 corner origin size (8, 4)');
  // A named corner that is not the bottom left is written as numbers.
  assert.equal(rectText('r2', {name: 'origin', at: [0, 0]}, [-8, 4]), 'rect r2 corner (-8, 0) size (8, 4)');
  assert.equal(spot({at: [1.5, -2]}), '(1.5, -2)');
});

test('constraints are written for what is chosen, or say what they want', () => {
  const shape = (name) => ({kind: 'shape', name});
  const point = (name) => ({kind: 'point', name});
  assert.deepEqual(holdText('horizontal', [shape('r.top')], base), {text: 'horizontal r.top'});
  assert.deepEqual(holdText('vertical', [point('h1.center'), point('r.bl')], base), {text: 'vertical h1.center r.bl'});
  assert.deepEqual(holdText('coincident', [point('h1.center'), shape('r.left')], base), {text: 'on h1.center r.left'});
  assert.deepEqual(holdText('equal', [shape('h1'), shape('h2')], base), {text: 'equal h1 h2'});
  assert.deepEqual(holdText('concentric', [shape('h1'), shape('h2')], base), {text: 'concentric h1 h2'});
  assert.deepEqual(holdText('tangent', [shape('r.top'), shape('h1')], base), {text: 'tangent r.top h1'});
  assert.ok('why' in holdText('parallel', [shape('h1'), shape('r.top')], base));
  assert.ok('why' in holdText('equal', [shape('h1'), shape('r.top')], base));
  assert.deepEqual(holdText('fix', [point('r.bl')], base), {text: 'fix r.bl'});
});

test('a dimension is written at what it measures as drawn', () => {
  const shape = (name) => ({kind: 'shape', name});
  const point = (name) => ({kind: 'point', name});
  assert.deepEqual(dimensionText([shape('r.bottom')], base), {text: 'length r.bottom = 80'});
  assert.deepEqual(dimensionText([shape('h1')], base), {text: 'diameter h1 = 3.4'});
  // Level between two points: how far across.
  assert.deepEqual(dimensionText([point('h1.center'), point('h2.center')], base), {text: 'hdistance h1.center h2.center = 60'});
  assert.deepEqual(dimensionText([point('r.bl'), point('r.tl')], base), {text: 'vdistance r.bl r.tl = 40'});
  assert.deepEqual(dimensionText([point('r.bl'), point('h2.center')], base), {text: 'distance r.bl h2.center = 70.711'});
  assert.deepEqual(dimensionText([point('h1.center'), shape('r.top')], base), {text: 'distance h1.center r.top = 30'});
  assert.deepEqual(dimensionText([shape('r.bottom'), shape('r.right')], base), {text: 'angle r.bottom r.right = 90'});
  assert.ok('why' in dimensionText([], base));
});

test('construction is toggled on a shape\'s own line', () => {
  assert.equal(toggleConstruction('line a (0, 0) (1, 0)'), 'construction line a (0, 0) (1, 0)');
  assert.equal(toggleConstruction('construction line a (0, 0) (1, 0)'), 'line a (0, 0) (1, 0)');
});

test('a spline is drawn from its first control point to its last', () => {
  const pts = splinePoints([[0, 0], [1, 2], [3, 2], [4, 0]]);
  assert.deepEqual(pts[0], [0, 0]);
  assert.ok(Math.hypot(pts.at(-1)[0] - 4, pts.at(-1)[1]) < 1e-9);
  assert.ok(pts.every((p) => p[1] >= -1e-12 && p[1] <= 2));
});

test('a step\'s settings write its line', () => {
  assert.equal(stepText.sketch('s2', 'plate.top', ' 2 '), 'sketch s2 on plate.top offset 2');
  assert.equal(stepText.sketch('s2', 'XY', ''), 'sketch s2 on XY');
  assert.equal(stepText.extrude('e1', 'base', 't * 2', true), 'extrude e1 from base distance t * 2 symmetric');
  assert.equal(stepText.extrude('e1', 'base', '', false), 'extrude e1 from base distance 10');
  assert.equal(stepText.revolve('v1', 'base', 'mid', ''), 'revolve v1 from base about mid');
  assert.equal(stepText.revolve('v1', 'base', 'yaxis', '90'), 'revolve v1 from base about yaxis angle 90');
  assert.ok(placeable('plate.top') && placeable('spin.end'));
  assert.ok(placeable('plate.side(r.left)') && !placeable('plate'), 'since K4 a side is placeable too');
  assert.equal(volumeText(904.78), '904.8 mm³');
  assert.equal(volumeText(9545.5), '9.55 cm³');
});

test('an extrude or revolve made into a body, or right through, writes its line', () => {
  assert.equal(stepText.extrude('slot', 's', '-2', false, false, 'cut', 'plate'), 'extrude slot from s distance -2 cut plate');
  assert.equal(stepText.extrude('win', 's', '3', true, true, 'cut', 'plate'), 'extrude win from s through cut plate');
  assert.equal(stepText.extrude('e', 's', '3', false, false, 'join', ''), 'extrude e from s distance 3', 'no body, no join');
  assert.equal(stepText.revolve('v', 's', 'yaxis', '90', 'intersect', 'plate'), 'revolve v from s about yaxis angle 90 intersect plate');
  assert.ok(placeable('plate.side(r.left)') && placeable('m3.side(wall1)'));
  assert.ok(!placeable('plate.side(r.left') && !placeable('plate.edges(parallel z)'));
});

test('a point on a face is in the face\'s own coordinates, in the part\'s units', () => {
  // The top of a plate 3 high: origin (0, 0, 3), up, x along x.
  assert.deepEqual(onFace([0, 0, 3, 0, 0, 1, 1, 0, 0], [10, 20, 3], 1), [10, 20]);
  // A side facing -y: its x along x, its y = n × x = up.
  assert.deepEqual(onFace([0, 0, 0, 0, -1, 0, 1, 0, 0], [5, 0, 2], 1), [5, 2]);
  // In inches.
  assert.deepEqual(onFace([0, 0, 0, 0, 0, 1, 1, 0, 0], [25.4, 50.8, 0], unitScale('in')), [1, 2]);
});

test('rounds, chamfers, shells, sweeps and lofts are written as the language has them', () => {
  assert.equal(stepText.fillet('r1', ['p.top & p.side(r.left)', 'p.edges(parallel z)'], '2'), 'fillet r1 on p.top & p.side(r.left), p.edges(parallel z) radius 2');
  assert.equal(stepText.fillet('r1', ['p.edges'], ' '), 'fillet r1 on p.edges radius 1');
  assert.equal(stepText.chamfer('c1', ['p.top & p.side(r.left)'], '1', ''), 'chamfer c1 on p.top & p.side(r.left) distance 1');
  assert.equal(stepText.chamfer('c1', ['p.top & p.side(r.left)'], '1', '2'), 'chamfer c1 on p.top & p.side(r.left) distance 1 and 2');
  assert.equal(stepText.shell('s', 'p', '1.5'), 'shell s of p thickness 1.5');
  assert.equal(stepText.shell('s', 'p', '1.5', ['p.top', 'p.side(r.left)']), 'shell s of p thickness 1.5 open p.top, p.side(r.left)');
  assert.equal(stepText.sweep('w', 'ring', 'path'), 'sweep w from ring along path');
  assert.equal(stepText.sweep('w', 'ring', 'path', 'cut', 'p'), 'sweep w from ring along path cut p');
  assert.equal(stepText.loft('l', 'a', 'b', 'join', 'p'), 'loft l from a to b join p');
  assert.deepEqual(lines('  a & b\n\n c.edges(convex, round) \n'), ['a & b', 'c.edges(convex, round)']);
  assert.deepEqual(toggled(['a', 'b'], 'a'), ['b']);
  assert.deepEqual(toggled(['a'], 'b'), ['a', 'b']);
});

test('an assembly\'s lines are written as its language has them', async () => {
  const {asmText, relPath, lineIndex, offsets} = await import('../../ui/cad-assembly.js');
  const {ASSEMBLY} = await import('../../ui/preview-cad-assembly.js');
  assert.equal(asmText.instance('pin', 'parts/pin.part'), 'part pin from "parts/pin.part"');
  assert.equal(asmText.instance('loop', 'loop.assembly', 'assembly'), 'assembly loop from "loop.assembly"');
  assert.equal(asmText.place('pin', [1.5, 0, -2]), 'place pin at (1.5, 0, -2)');
  assert.equal(asmText.place('pin', [0, 0, 0], [0, 0, 90.0000000001]), 'place pin at (0, 0, 0) turn (0, 0, 90)');
  assert.equal(asmText.mate('m', 'planar', 'a.top', 'b.bottom', ''), 'mate m planar a.top to b.bottom');
  assert.equal(asmText.mate('m', 'planar', 'a.top', 'b.bottom', '2'), 'mate m planar a.top to b.bottom offset 2');
  assert.equal(asmText.mate('m', 'angle', 'a.top', 'b.top', ''), 'mate m angle a.top to b.top = 90');
  assert.equal(asmText.mate('m', 'concentric', 'a.side(c)', 'b.side(c)', '4'), 'mate m concentric a.side(c) to b.side(c)');
  assert.equal(relPath('rig.assembly', 'parts/pin.part'), 'parts/pin.part');
  assert.equal(relPath('builds/t1/rig.assembly', 'parts/pin.part'), '../../parts/pin.part');
  assert.equal(relPath('builds/rig.assembly', 'builds/sub/pin.part'), 'sub/pin.part');
  assert.equal(lineIndex('a\n  place pin at (0, 0, 0)\nb', 'place pin '), 1);
  assert.deepEqual(offsets(ASSEMBLY, 'apart'), {block: [0, 0, 30], pin: [0, 0, 60]});
  assert.deepEqual(offsets(ASSEMBLY, null), {});
});

test('routes are written as the language has them, and ports and points read', async () => {
  const {asmText, portOf, points} = await import('../../ui/cad-assembly.js');
  assert.equal(asmText.route('t1', 'hard', '14/16', 'a.socket', 'b.socket'), 'route t1 hard 14/16 from a.socket to b.socket');
  assert.equal(asmText.route('t1', 'soft', '10/13', 'a.socket', 'b.socket', [[1, 2, 3], [4.5, 0, -1]], {bend: '40', lead: '', insert: '12'}), 'route t1 soft 10/13 from a.socket to b.socket via (1, 2, 3), (4.5, 0, -1) bend 40 insert 12');
  assert.equal(portOf('f1.socket.side(wall1)'), 'f1.socket');
  assert.equal(portOf('loop.pump.inlet.side(wall2)'), 'loop.pump.inlet');
  assert.equal(portOf('f1.body.top'), null);
  assert.deepEqual(points('1, 2, 3\n\n 4 5 6 \nbad\n7, 8'), [[1, 2, 3], [4, 5, 6]]);
});

import {moved, boxAt, itemOfBox, scaleOf, freshName, drawText} from '../../ui/cad-drawing.js';
import {DRAWING} from '../../ui/preview-cad-drawing.js';

test('a drawing\'s view is moved by its middle, a detail by its place, the parts list by its corner', () => {
  const view = DRAWING.items.find((i) => i.name === 'front');
  assert.equal(moved(view, 10.04, -5), 'view front front at (95, 125)');
  assert.equal(moved({...view, kind: 'detail', text: 'detail c of f at (100, 170) radius 12 scale 2 place (60, 60)'}, 5, 5), 'detail c of f at (100, 170) radius 12 scale 2 place (65, 65)');
  assert.equal(moved(DRAWING.items.find((i) => i.kind === 'bom'), -4, 0), 'bom at (580, 74)');
  assert.equal(moved({...view, kind: 'dim', text: 'dim w f distance a to b'}, 1, 1), null);
});

test('the box under a point is the smallest, never the title block; boxes are their statements', () => {
  assert.equal(boxAt(DRAWING, 85, 130)?.name, 'front');
  assert.equal(boxAt(DRAWING, 500, 25), null, 'the title block is not picked');
  assert.equal(boxAt(DRAWING, 300, 200), null);
  assert.equal(itemOfBox(DRAWING, 'iso')?.kind, 'view', 'the view, not its balloons');
  assert.equal(itemOfBox(DRAWING, 'bom')?.kind, 'bom');
  assert.equal(itemOfBox(DRAWING, 'title'), null);
});

test('a drawing\'s scale is read as written, names are fresh, and lines are the language\'s', () => {
  assert.equal(scaleOf('drawing "x"\nscale 1 / 2\n'), '1/2');
  assert.equal(scaleOf('drawing "x"\n'), null);
  assert.equal(freshName('v', ['v1', 'v2']), 'v3');
  assert.equal(drawText.view('v1', 'top', 120.04, 80), 'view v1 top at (120, 80)');
  assert.equal(drawText.dim('d1', 'f', 'diameter', 'plate.side(h1)', ''), 'dim d1 f diameter plate.side(h1)');
  assert.equal(drawText.note('n1', 'f', 'say "hi"', 'm3'), 'note n1 f "say \'hi\'" at m3');
});

import {referenceFor, partLine, netLine, schematicMoved, schematicItemOfBox} from '../../ui/cad-schematic.js';
import {SCHEMATIC} from '../../ui/preview-cad-schematic.js';

test('a schematic\'s lines: a part named for its kind, a net\'s pins, a part moved or given a place', () => {
  assert.equal(referenceFor('resistor', ['R1', 'R2']), 'R3');
  assert.equal(referenceFor('atx-24', ['J1']), 'J2');
  assert.equal(referenceFor('ic', []), 'U1');
  assert.equal(partLine({reference: 'U1', kind: 'ic', takes: '"NE555" pins (1 GND power-in)', value: '', footprint: 'SOIC-8'}), 'part U1 ic "NE555" pins (1 GND power-in) footprint SOIC-8');
  assert.equal(partLine({reference: 'R1', kind: 'resistor', value: '4k7', at: [10.04, 20]}), 'part R1 resistor value 4k7 at (10, 20)');
  assert.equal(netLine('+12V', true, 'P1.13 J1.2,J2.2', ' 3 '), 'net +12V power P1.13, J1.2, J2.2 current 3');
  const j1 = schematicItemOfBox(SCHEMATIC, 'J1');
  assert.equal(schematicMoved(SCHEMATIC, j1, 5, -10), 'part J1 fan-4pin at (165, 150)');
  const placed = {...SCHEMATIC, parts: SCHEMATIC.parts.map((p) => (p.reference === 'C1' ? {...p, at: [90, 150]} : p))};
  const c1 = {...schematicItemOfBox(SCHEMATIC, 'C1'), text: 'part C1 capacitor value 10u turn 90'};
  assert.equal(schematicMoved(placed, c1, 10, 0), 'part C1 capacitor value 10u at (100, 150) turn 90');
  assert.equal(schematicItemOfBox(SCHEMATIC, 'title'), null);
});

test('the split keeps both panes in reach, and offers only what CAD shows', async () => {
  const {clampShare, showable} = await import('../../ui/cad-companion.js');
  assert.equal(clampShare(0.5), 0.5);
  assert.equal(clampShare(0.01), 0.15);
  assert.equal(clampShare(2), 0.85);
  assert.equal(clampShare(NaN), 0.5);
  const files = [
    {path: 'a.part', name: 'a', kind: 'part'},
    {path: 'b.assembly', name: 'b', kind: 'assembly'},
    {path: 'c.drawing', name: 'c', kind: 'drawing'},
    {path: 'd.step', name: 'd', kind: 'import'},
    {path: 'e.schematic', name: 'e', kind: 'schematic'},
    {path: 'f.board', name: 'f', kind: 'board'},
  ];
  assert.deepEqual(showable(files).map((f) => f.path), ['a.part', 'b.assembly', 'c.drawing', 'd.step']);
});
