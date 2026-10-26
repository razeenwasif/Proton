// Diffuse's graph in node: sockets where the card draws them, a wire that
// leaves and arrives level, the id the core gives the next node, the
// catalogue searched and grouped — and the preview's catalogue the crate's.
import {test} from 'node:test';
import {equal, deepEqual, ok} from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {colourOf, freshId, grouped, HEAD, GAP, NODE_W, ROW, portAt, rowsOf, search, socketFor, wire, shortValue} from '../../ui/diffuse-graph.js';
import {CATALOGUE} from '../../ui/preview-diffuse.js';

const fixture = JSON.parse(readFileSync(new URL('../../crates/proton-diffuse/tests/fixtures/catalogue-v1.json', import.meta.url), 'utf8'));
const kind = (type) => fixture.nodes.find((n) => n.type === type);

test('the preview stands in with the very catalogue the crate tests against', () => {
  deepEqual(CATALOGUE, fixture);
});

test('a socket sits on its row: inputs on the left edge, outputs on the right', () => {
  const sampler = kind('KSampler');
  const rows = rowsOf(sampler);
  equal(rows.length, 4, 'four sockets in, one out, four rows');
  deepEqual(rows.map((r) => r.input?.name), ['model', 'positive', 'negative', 'latent']);
  equal(rows[0].output?.name, 'latent');
  const node = {id: '5', type: 'KSampler', at: [100, 50], values: {}, seed: null, bypass: null};
  deepEqual(portAt(node, sampler, 'in', 'model'), [100, 50 + HEAD + GAP + ROW / 2]);
  deepEqual(portAt(node, sampler, 'in', 'latent'), [100, 50 + HEAD + GAP + 3 * ROW + ROW / 2]);
  deepEqual(portAt(node, sampler, 'out', 'latent'), [100 + NODE_W, 50 + HEAD + GAP + ROW / 2]);
  equal(portAt(node, sampler, 'in', 'seed'), null, 'a field has no socket');
});

test('a wire leaves and arrives level, however far back it goes', () => {
  equal(wire([0, 0], [200, 100]), 'M0 0 C100 0 100 100 200 100');
  equal(wire([300, 0], [100, 0]), 'M300 0 C400 0 0 0 100 0');
});

test('the next node is named as the core names it', () => {
  equal(freshId({nodes: []}), '1');
  equal(freshId({nodes: [{id: '1'}, {id: '7'}, {id: 'loader'}]}), '8');
});

test('the catalogue is listed by group and searched by title, nodes that fit first', () => {
  deepEqual(grouped(fixture).map((g) => g.group), ['Load', 'Prompt', 'Latent', 'Sample', 'Image', 'Output']);
  const ups = search(fixture, 'ups').flatMap((g) => g.nodes.map((n) => n.title));
  deepEqual(ups.sort(), ['Load Upscaler', 'Upscale Image', 'Upscale Latent']);
  // Let go of an IMAGE wire: only what takes an image.
  const takes = search(fixture, '', {type: 'IMAGE', side: 'in'}).flatMap((g) => g.nodes.map((n) => n.type));
  deepEqual(takes.sort(), ['PreviewImage', 'SaveImage', 'UpscaleImage', 'VAEEncode']);
  equal(search(fixture, 'ksa')[0].nodes[0].type, 'KSampler');
  equal(socketFor(kind('VAEDecode'), 'LATENT', 'in'), 'latent');
  equal(socketFor(kind('LoadCheckpoint'), 'VAE', 'out'), 'vae');
});

test('colours are the mockups\', and a newer server\'s type gets one of its own', () => {
  equal(colourOf('LATENT'), '#82e2ff');
  ok(colourOf('AUDIO').startsWith('hsl('));
  equal(colourOf('AUDIO'), colourOf('AUDIO'));
  equal(shortValue('sd_xl_base_1.0.safetensors'), 'sd_xl_base_1.0');
  equal(shortValue(undefined), '—');
});
