// What the Research composer reads as a hypothesis of the scientist's own.
import {test} from 'node:test';
import {deepEqual, equal} from 'node:assert/strict';
import {ownHypothesis} from '../../ui/research.js';
import {bytes} from '../../ui/lab.js';

test('/h Title: statement is a hypothesis, and anything else is feedback', () => {
  deepEqual(ownHypothesis('/h Sieve by 7: A sieve modulo 7 rules out a sixth of the triples.'),
    {title: 'Sieve by 7', statement: 'A sieve modulo 7 rules out a sixth of the triples.'});
  deepEqual(ownHypothesis('  /hypothesis Ozone: The Chappuis band deepens the zenith blue.  '),
    {title: 'Ozone', statement: 'The Chappuis band deepens the zenith blue.'});
  equal(ownHypothesis('Focus on polarisation.'), null);
  equal(ownHypothesis('/home is where the heart is'), null);
  equal(ownHypothesis('/h'), null);
});

test('without a title, the first line is one, cut to fit', () => {
  const long = 'Multiple scattering whitens the horizon because every path near the horizon crosses so much air';
  const own = ownHypothesis(`/h ${long}`);
  equal(own?.statement, long);
  equal(own?.title.length, 78);
  equal(own?.title.endsWith('…'), true);
  // A colon far into the text is part of the statement, not a title.
  const late = ownHypothesis(`/h ${'x'.repeat(130)}: y`);
  equal(late?.statement, `${'x'.repeat(130)}: y`);
});

test('a download is said in the unit people read it in', () => {
  equal(bytes(512), '512 B');
  equal(bytes(5 * 1024), '5 KB');
  equal(bytes(18713545), '17.8 MB');
  equal(bytes(3 * 1024 ** 3), '3.00 GB');
});

import {icosphere, tube, faceted, bounds, between} from '../../ui/lab3d.js';

test('a sphere, a tube and a mesh are built the shape they say', () => {
  const ball = icosphere(1);
  equal(ball.indices.length / 3, 80, 'an icosahedron split once has 80 faces');
  for (let i = 0; i < ball.positions.length; i += 3) {
    const r = Math.hypot(ball.positions[i], ball.positions[i + 1], ball.positions[i + 2]);
    equal(Math.abs(r - 1) < 1e-6, true, 'every point of the sphere is on it');
  }
  const rod = tube(8);
  equal(rod.indices.length / 3, 16);
  const ys = Array.from(rod.positions).filter((_, i) => i % 3 === 1);
  equal(Math.min(...ys), 0);
  equal(Math.max(...ys), 1);
  const square = faceted([[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]], [[0, 1, 2, 3]]);
  equal(square.positions.length / 3, 6, 'a quad is two triangles');
  deepEqual(Array.from(square.normals.slice(0, 3)), [0, 0, 1]);
});

test('a scene is framed by everything in every frame, and a bond reaches from end to end', () => {
  const doc = {format: 'proton.scene/1', frames: [[{kind: 'sphere', center: [0, 0, 0], radius: 1}], [{kind: 'points', xyz: [[10, 0, 0]]}]]};
  const b = bounds(doc);
  deepEqual(b.center, [4.5, 0, 0]);
  equal(Math.abs(b.radius - Math.hypot(11, 2, 2) / 2) < 1e-9, true);
  deepEqual(bounds({format: 'proton.scene/1', frames: [[]]}), {center: [0, 0, 0], radius: 1}, 'an empty scene still has somewhere to look');
  const m = between([1, 2, 3], [1, 2, 7], 0.5);
  // The tube's far end, (0, 1, 0), lands on the bond's.
  const end = [m[4] + m[12], m[5] + m[13], m[6] + m[14]];
  deepEqual(end.map((v) => Math.round(v * 1e9) / 1e9), [1, 2, 7]);
});
