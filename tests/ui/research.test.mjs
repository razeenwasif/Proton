// What the Research composer reads as a hypothesis of the scientist's own.
import {test} from 'node:test';
import {deepEqual, equal} from 'node:assert/strict';
import {ownHypothesis} from '../../ui/research.js';

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
