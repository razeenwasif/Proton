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

import {bounds, triangles} from '../../ui/lab3d.js';

test('a mesh\'s polygons become triangles, and a corner that names no vertex is dropped', () => {
  deepEqual(triangles([[0, 1, 2, 3]], 4), [0, 1, 2, 0, 2, 3], 'a quad is a fan of two');
  deepEqual(triangles([[0, 1, 2], [0, 1, 9]], 3), [0, 1, 2], 'a face reaching past the vertices is left out');
  deepEqual(triangles([[0, 1]], 2), [], 'two corners make no triangle');
  deepEqual(triangles(undefined, 0), []);
});

test('a scene is framed by everything in every frame', () => {
  const doc = {format: 'proton.scene/1', frames: [[{kind: 'sphere', center: [0, 0, 0], radius: 1}], [{kind: 'points', xyz: [[10, 0, 0]]}]]};
  const b = bounds(doc);
  deepEqual(b.center, [4.5, 0, 0]);
  equal(Math.abs(b.radius - Math.hypot(11, 2, 2) / 2) < 1e-9, true);
  deepEqual(bounds({format: 'proton.scene/1', frames: [[]]}), {center: [0, 0, 0], radius: 1}, 'an empty scene still has somewhere to look');
  const arrow = bounds({format: 'proton.scene/1', frames: [[{kind: 'arrow', origin: [0, 0, 0], vector: [0, 4, 0]}]]});
  deepEqual(arrow.center, [0, 2, 0], 'an arrow is framed to its tip');
});
