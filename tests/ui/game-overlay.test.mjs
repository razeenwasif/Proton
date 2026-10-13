// Game mode's overlay, without a canvas: a room fitted to the stage, and what
// a click there lands on.
import {test} from 'node:test';
import {equal, deepEqual, ok} from 'node:assert/strict';
import {fit, pick, toRoom, toStage} from '../../ui/game-overlay.js';

const room = {
  size: [1000, 500],
  entities: [
    {id: 'door', prefab: 'transition', rect: [900, 300, 40, 200], at: null},
    {id: 'bench', prefab: 'save_point', at: [200, 480], rect: null},
  ],
  spawns: [{id: 'start', at: [100, 480]}],
  lights: [{id: 'lamp', at: [500, 200], radius: 300}],
  collision: [{id: 'floor', kind: 'solid', closed: false, points: [[0, 480], [1000, 480]]}],
};

test('a room is fitted into the stage, centred, with a margin, and maps back', () => {
  const view = fit(room, 1048, 548, 24);
  equal(view.scale, 1);
  equal(view.x, 24);
  equal(view.y, 24);
  const [sx, sy] = toStage(view, [500, 250]);
  deepEqual(toRoom(view, sx, sy), [500, 250]);
  // A tall stage: the width decides.
  const tall = fit(room, 548, 2000, 24);
  equal(tall.scale, 0.5);
});

test('a click lands on what stands nearest, an area it is inside, or a line it is on', () => {
  const view = fit(room, 1048, 548, 24);
  const at = (x, y) => pick(room, view, x + view.x, y + view.y);
  deepEqual(at(102, 478), {kind: 'spawn', id: 'start'});
  deepEqual(at(201, 481), {kind: 'entity', id: 'bench'});
  deepEqual(at(920, 400), {kind: 'entity', id: 'door'});
  deepEqual(at(500, 203), {kind: 'light', id: 'lamp'});
  deepEqual(at(650, 484), {kind: 'shape', id: 'floor'});
  equal(at(650, 100), null, 'empty space selected something');
  ok(true);
});
