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

test('a PNG\'s graph is read from its head, tEXt or iTXt, and nothing from one without', async () => {
  const {pngText} = await import('../../ui/diffuse-graph.js');
  const chunk = (kind, data) => {
    const out = new Uint8Array(12 + data.length);
    new DataView(out.buffer).setUint32(0, data.length);
    out.set([...kind].map((c) => c.charCodeAt(0)), 4);
    out.set(data, 8);
    return out;
  };
  const bytes = (...parts) => {
    const all = [Uint8Array.of(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a), ...parts];
    const out = new Uint8Array(all.reduce((n, p) => n + p.length, 0));
    let at = 0;
    for (const p of all) { out.set(p, at); at += p.length; }
    return out;
  };
  const enc = (s) => new TextEncoder().encode(s);
  const ihdr = chunk('IHDR', new Uint8Array(13));
  const graph = '{"nodes":[{"id":"1","type":"SaveImage","values":{}}],"links":[]}';
  equal(pngText(bytes(ihdr, chunk('tEXt', enc(`Software\0PIL`)), chunk('tEXt', enc(`proton-flow\0${graph}`)), chunk('IDAT', enc('xx')))), graph);
  equal(pngText(bytes(ihdr, chunk('iTXt', enc(`proton-flow\0\0\0en\0\0${graph}`)), chunk('IDAT', enc('xx')))), graph);
  // After the pixels is not read; nor is a PNG with none, nor not a PNG.
  equal(pngText(bytes(ihdr, chunk('IDAT', enc('xx')), chunk('tEXt', enc(`proton-flow\0${graph}`)))), null);
  equal(pngText(bytes(ihdr, chunk('IDAT', enc('xx')))), null);
  equal(pngText(enc('GIF89a.....')), null);
});
