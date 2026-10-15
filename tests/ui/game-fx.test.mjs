// Game mode's feel and sound, without a window: particles that are a closed
// function of time and seed — the same moment, the same particles, however
// it was reached — and the sounds a tick's events and cues play.
import {test} from 'node:test';
import {deepEqual, equal, notDeepEqual, ok} from 'node:assert/strict';

const {AIR, BURSTS, air, burst, living, noise, seedOf} = await import('../../ui/game-fx.js');
const {EVENTS, musicFor, soundsFor} = await import('../../ui/game-audio.js');
const {squashed} = await import('../../ui/game-scene.js');

test('the same seed at the same moment is the same air, and another seed is other air', () => {
  const view = [400, 300, 1920, 1080];
  for (const preset of Object.keys(AIR)) {
    const a = air(preset, view, 12.5, seedOf('crossroads_01:0'));
    deepEqual(air(preset, view, 12.5, seedOf('crossroads_01:0')), a, `${preset} is not the same twice`);
    notDeepEqual(air(preset, view, 12.5, seedOf('crossroads_02:0')), a, `${preset} is the same for another room`);
    ok(a.length > 0, `${preset} has nothing in it`);
    // Inside the view, give or take the margin it is laid in and half a streak.
    for (const m of a) ok(m.x >= view[0] - 100 && m.x <= view[0] + view[2] + 100 && m.y >= view[1] - 100 && m.y <= view[1] + view[3] + 100, `${preset} strayed to ${m.x}, ${m.y}`);
  }
  equal(air('fireflies', view, 1, 1).length, 0, 'a kind there is not makes something');
});

test('the air stays where it is in the room as the camera moves', () => {
  const seed = seedOf('room:0');
  const here = air('snow', [0, 0, 1920, 1080], 3, seed);
  const moved = air('snow', [200, 0, 1920, 1080], 3, seed);
  // What both views see well inside them is the same flakes in the same places.
  const inside = (/** @type {any[]} */ list) => list.filter((m) => m.x > 300 && m.x < 1800).map((m) => `${m.x.toFixed(3)},${m.y.toFixed(3)}`).sort();
  deepEqual(inside(moved), inside(here));
});

test('a burst flies from where it began, falls, fades, and is over when its longest life is', () => {
  const spark = {kind: 'hit', at: [100, 200], start: 2, seed: 7};
  equal(burst(spark, 1.9).length, 0, 'before it began');
  const early = burst(spark, 2.02);
  equal(early.length, BURSTS.hit.count);
  deepEqual(burst(spark, 2.02), early, 'the same moment twice differs');
  const later = burst(spark, 2.2);
  ok(later.length <= early.length && later.every((m, i) => m.opacity <= early[i].opacity + 1e-9), 'it did not fade');
  equal(burst(spark, 2 + BURSTS.hit.life[1] + 0.01).length, 0, 'after it was over');
  deepEqual(living([spark, {kind: 'land', at: [0, 0], start: 0, seed: 1}], 2.1).map((s) => s.kind), ['hit']);
  // Dust thrown behind a body facing left goes the other way.
  const right = burst({kind: 'dash', at: [0, 0], start: 0, seed: 3}, 0.1);
  const left = burst({kind: 'dash', at: [0, 0], start: 0, seed: 3, flip: true}, 0.1);
  ok(right.every((m, i) => Math.sign(m.x + m.w / 2) === -Math.sign(left[i].x + left[i].w / 2) || Math.abs(m.x + m.w / 2) < 1e-6));
  equal(noise(1, 2, 3), noise(1, 2, 3));
});

test('a tick plays the sounds of what it said, and of its cues, once each', () => {
  const bit = Object.fromEntries(EVENTS.map(([b, name]) => [name, b]));
  const sounds = {jump: 'audio/jump.wav', land: 'audio/land.wav', hurt: 'audio/hurt.wav', die: 'audio/die.wav', step: 'audio/step.wav', hit: 'audio/hit.wav', kill: 'audio/hit.wav'};
  deepEqual(soundsFor(bit.jump, [], sounds), ['audio/jump.wav']);
  deepEqual(soundsFor(bit.land | bit.dash, ['step', 'step'], sounds), ['audio/land.wav', 'audio/step.wav'], 'a dash has no sound, and a cue said twice is one');
  deepEqual(soundsFor(bit.hit | bit.kill, [], sounds), ['audio/hit.wav'], 'one file for two events plays once');
  deepEqual(soundsFor(bit.hurt | bit.die, [], sounds), ['audio/die.wav'], 'a death is its own sound, not the hurt under it');
  deepEqual(soundsFor(bit.hurt | bit.die, [], {hurt: 'audio/hurt.wav'}), ['audio/hurt.wav'], 'with no sound for dying, the hurt is heard');
  deepEqual(soundsFor(0, [], sounds), []);
});

test('a room plays its area\'s music, and a room in no area none', () => {
  const world = {areas: [{id: 'caves', music: 'audio/caves.mp3'}, {id: 'town', music: null}], rooms: [{id: 'a', area: 'caves'}, {id: 'b', area: 'town'}, {id: 'c', area: ''}]};
  equal(musicFor(world, 'a'), 'audio/caves.mp3');
  equal(musicFor(world, 'b'), null);
  equal(musicFor(world, 'c'), null);
});

test('a body squashes and stretches about its feet', () => {
  const sprite = {key: 'k', plane: 'main', item: null, x: 90, y: 120, w: 20, h: 80, rot: 0, flip: false, opacity: 1, tint: [0, 0, 0, 0], blend: 'normal'};
  const s = squashed(sprite, {key: 'k', at: [100, 200], pivot: [10, 80], scale: [1.3, 0.75]});
  equal(s.y + s.h, 200, 'its feet moved');
  equal(s.x + s.w / 2, 100, 'it is not squashed about its middle');
  equal(s.h, 60);
  deepEqual(squashed(sprite, {key: 'k', at: [100, 200], pivot: [10, 80]}), sprite);
});
