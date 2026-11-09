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
