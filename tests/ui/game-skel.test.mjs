// Game mode's skeletons posed in node (ui/game-skel.js), against what is
// worked out here by hand: a bone turned on its parent, a point carried by
// one bone and by two, each ease DragonBones has, a slot's part and colour
// keyed, a run's ticks back to a clip's frames, and boxes where their bones
// put them.
import {test} from 'node:test';
import {deepEqual, equal, ok} from 'node:assert/strict';
import {boxesOf, clipTicks, eased, momentOf, pose, restingClip, skin} from '../../ui/game-skel.js';

const near = (a, b, what) => ok(a.every((v, i) => Math.abs(v - b[i]) < 1e-9), `${what}: ${a} is not ${b}`);
const linear = {kind: 'linear'};
const skel = {
  format: 1, source: 'hero_ske.json', atlas: 'hero_tex.png', atlasSize: [64, 32], fps: 24,
  bones: [
    {name: 'root', parent: null, at: [0, 0], rot: 0, skew: 0, scale: [1, 1], length: 0},
    {name: 'body', parent: 0, at: [0, -20], rot: 0, skew: 0, scale: [1, 1], length: 0},
    {name: 'arm', parent: 1, at: [4, -6], rot: 90, skew: 0, scale: [1, 1], length: 0},
  ],
  slots: [{name: 'arm', bone: 2, shown: 0, color: [1, 1, 1, 1], parts: [{name: 'arm', points: [], triangles: []}, {name: 'fist', points: [], triangles: []}]}],
  clips: [
    {name: 'wave', frames: 10, repeat: 0, cues: [],
     bones: [{bone: 2, moved: [], turned: [{frame: 0, value: [0, 0], ease: linear}, {frame: 10, value: [90, 0], ease: linear}], scaled: []},
             {bone: 1, moved: [{frame: 0, value: [0, 0], ease: {kind: 'step'}}, {frame: 4, value: [0, -4], ease: linear}], turned: [], scaled: [{frame: 0, value: [2, 1], ease: linear}]}],
     slots: [{slot: 0, shown: [[0, 0], [6, 1]], color: [{frame: 0, color: [1, 1, 1, 1], ease: linear}, {frame: 10, color: [1, 1, 1, 0], ease: linear}]}]},
    {name: 'Idle', frames: 4, repeat: 1, cues: [], bones: [], slots: []},
  ],
  boxes: [{name: 'hitbox', kind: 'hit', slot: 0, part: 1, bone: 2, points: [[0, 0], [10, 0]]}],
};

test('every ease DragonBones has, half way', () => {
  equal(eased({kind: 'step'}, 0.5), 0);
  equal(eased(linear, 0.5), 0.5);
  equal(eased({kind: 'in', amount: 1}, 0.5), 0.25);
  equal(eased({kind: 'out', amount: 1}, 0.5), 0.75);
  equal(eased({kind: 'out', amount: 0.5}, 0.5), 0.625);
  ok(Math.abs(eased({kind: 'inOut', amount: 1}, 0.5) - 0.5) < 1e-12);
  ok(Math.abs(eased({kind: 'inOut', amount: 1}, 0.25) - 0.5 * (1 - Math.cos(Math.PI / 4))) < 1e-12);
  ok(Math.abs(eased({kind: 'bezier', points: [0.42, 0, 0.58, 1]}, 0.5) - 0.5) < 1e-6);
  ok(eased({kind: 'bezier', points: [0.42, 0, 0.58, 1]}, 0.2) < 0.2, 'an ease-in-out Bézier is slow at first');
});

test('a bone turned on its parent, and points carried by one bone and by two', () => {
  const rest = pose(skel, null, 0);
  near(Array.from(rest.bones.subarray(12, 18)), [0, 1, -1, 0, 4, -26], 'the arm at rest');
  // Half through the wave: the arm turned 45 more; the body stepped, not yet moved, and twice as wide.
  const half = pose(skel, 0, 5);
  const s = Math.SQRT1_2;
  near(Array.from(half.bones.subarray(6, 12)), [2, 0, 0, 1, 0, -24], 'the body moved 4 up by frame 5, and scaled');
  near(Array.from(half.bones.subarray(12, 18)), [-2 * s, s, -2 * s, -s, 8, -30], 'the arm, turned to 135 on the body');
  near(skin(half, [[2, 1, [10, 0]]]), [8 - 20 * s, -30 + 10 * s], 'a point on the arm');
  near(skin(half, [[1, 0.5, [0, 0]], [2, 0.5, [10, 0]]]), [(0 + 8 - 20 * s) / 2, (-24 - 30 + 10 * s) / 2], 'a point the body and the arm carry half each');
  // The slot's part from frame 6, its colour fading.
  deepEqual([pose(skel, 0, 5).shown[0], pose(skel, 0, 6).shown[0]], [0, 1]);
  near(Array.from(pose(skel, 0, 5).colors), [1, 1, 1, 0.5], 'the colour half way');
  // Before the first key and after the last, theirs.
  near(Array.from(pose(skel, 0, 12).bones.subarray(12, 16)), Array.from(pose(skel, 0, 10).bones.subarray(12, 16)), 'past the end');
});

test('a run\'s ticks back to a clip\'s frames, and boxes where their bones are', () => {
  deepEqual(clipTicks(skel, 60), [{from: 0, to: 24}, {from: 25, to: 34}]);
  deepEqual(momentOf(skel, 60, 30), {clip: 1, frame: 2});
  const at = momentOf(skel, 60, 12);
  ok(at.clip === 0 && Math.abs(at.frame - 4.8) < 1e-9, `tick 12 is ${JSON.stringify(at)}`);
  deepEqual(momentOf(skel, 60, null), {clip: 1, frame: 0}, 'at rest: the clip called idle');
  equal(restingClip(skel, {idle: 'wave'}), 0);
  equal(boxesOf(skel, pose(skel, 0, 0)).length, 0, 'a box is shown only with its part');
  const [hit] = boxesOf(skel, pose(skel, 0, 6));
  equal(hit.kind, 'hit');
  near(hit.points[1], skin(pose(skel, 0, 6), [[2, 1, [10, 0]]]), 'its corner, carried by the arm');
});

test('a skeleton as proton-game writes it — its empty lists left out — is posed whole', async () => {
  const {readFileSync} = await import('node:fs');
  const {skelOf} = await import('../../ui/game-skel.js');
  const fixture = JSON.parse(readFileSync(new URL('../../tests/ui/fixtures/game-skel.json', import.meta.url)).toString());
  const raw = JSON.parse(fixture.skel);
  ok(raw.clips.some((c) => c.cues === undefined || c.slots === undefined), 'the written skeleton leaves nothing out, and this tests nothing');
  const s = skelOf(raw);
  for (let clip = 0; clip < s.clips.length; clip++) {
    for (const f of [0, 3.5, s.clips[clip].frames]) {
      const posed = pose(s, clip, f);
      ok(posed.bones.every(Number.isFinite), `clip ${clip} at ${f}`);
    }
  }
  deepEqual(boxesOf(s, pose(s, 0, 0)), []);
  // The wave turns the arm: two moments of it differ.
  const arm = s.bones.findIndex((b) => b.name === 'arm');
  ok(Math.abs(pose(s, 0, 0).bones[arm * 6 + 1] - pose(s, 0, 6).bones[arm * 6 + 1]) > 0.5, "the arm did not turn: its sine is the same at 0 and 6");
});
