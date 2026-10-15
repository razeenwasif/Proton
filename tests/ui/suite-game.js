/* Game mode's checks, run by suite.js in the same fixture window: the newer
 * modes' chrome, the home page, a game's rooms as tabs, the World, Layers
 * and Entities tabs, the inspector's edits as ops, unsaved dots, ⌘S, ⌘Z,
 * asking before leaving, a click on the stage, and a room played. The game is the fixture
 * (tests/ui/fixtures/game — proton-game's template, written out). */

import {readFixture} from '/preview-game.js';

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, key: Function, shown: (el: any) => boolean}} t */
export async function gameChecks({test, assert, win, doc, $, settle, shown}) {
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const ws = () => $('.workspace[data-ws="game"]');
  const fixture = await readFixture();
  const home = {type: 'gameHome', games: [{path: 'crossroads', name: 'Crossroads', niche: 'metroidvania', rooms: 2}],
    templates: [{id: 'metroidvania', name: 'Metroidvania — side view, rooms joined into one map'}]};
  /** The open game, as the core sends it. @param {Record<string, any>} [over] */
  const project = (over = {}) => ({type: 'gameProject', project: {
    path: 'crossroads', game: fixture.game, world: fixture.world, prefabs: fixture.prefabs,
    rooms: fixture.rooms.map((/** @type {any} */ r) => ({id: r.id, area: 'crossroads', size: r.size, dirty: false})),
    open: ['crossroads_01'], focus: 'crossroads_01', gameDirty: false, worldDirty: false, dirty: false,
    canUndo: false, canRedo: false, problems: [], assets: ['art/player.png', 'art/sky.png', 'src/forest.psd'], recordings: [], imports: [], ...over}});
  const room = (/** @type {string} */ id, revision = 1) => ({type: 'gameRoom', room: fixture.rooms.find((/** @type {any} */ r) => r.id === id), revision});
  const press = (/** @type {string} */ k, /** @type {KeyboardEventInit} */ extra = {}) =>
    $('[data-game-stage]').dispatchEvent(new win.KeyboardEvent('keydown', {key: k, bubbles: true, cancelable: true, ...extra}));

  await test('Game wears the newer modes\' chrome, and with no game open its home page lists the games here and makes another', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    assert(ws()?.classList.contains('on'), 'Game is not drawn in its own section');
    assert(shown($('.titlebar')) && !shown($('.statusline')), 'Game does not wear the newer modes\' chrome');
    await win.pushEvents([home, {type: 'gameProject', project: null}, {type: 'gameRoom', room: null, revision: 0}]); await settle();
    assert(ws().classList.contains('game-home'), 'with no game open, the home page is not the whole page');
    assert(!shown($('.game-side')) && !shown($('.game-pane')), 'the sidebar or the inspector shows with no game open');
    const cards = [...ws().querySelectorAll('[data-game-open-path]')];
    assert(cards.length === 1 && cards[0].textContent.includes('Crossroads') && cards[0].textContent.includes('2 rooms'), `wrong games: ${cards.map((c) => c.textContent)}`);
    let before = win.commands.length;
    cards[0].click(); await settle();
    assert(sent(before, 'gameOpen')[0]?.path === 'crossroads', 'a game was not opened from its card');
    const name = /** @type {HTMLInputElement} */ ($('[data-game-new-name]'));
    name.value = 'Ember Hollow';
    before = win.commands.length;
    $('[data-game-new] button[type="submit"]').click(); await settle();
    const made = sent(before, 'gameNew')[0];
    assert(made?.name === 'Ember Hollow' && made.template === 'metroidvania', `New game sent ${JSON.stringify(made)}`);
  });

  await test('An open game shows its rooms as tabs, its world, a room\'s layers front first and its entities, and the room on the stage', async () => {
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    assert(!ws().classList.contains('game-home') && shown($('.game-side')) && shown($('.game-pane')), 'the open game is not laid out');
    const tabs = [...ws().querySelectorAll('[data-game-tab]')];
    assert(tabs.length === 1 && tabs[0].classList.contains('on') && !tabs[0].querySelector('.dot'), 'the room tab is wrong');
    assert($('[data-game-name]').textContent === 'Crossroads', 'the game is not named in the bar');
    $('[data-game-side="world"]').click(); await settle();
    const rooms = [...ws().querySelectorAll('[data-game-room]')].map((r) => r.dataset.gameRoom);
    assert(rooms.join() === 'crossroads_01,crossroads_02', `the World lists ${rooms}`);
    let before = win.commands.length;
    ws().querySelector('[data-game-room="crossroads_02"]').click(); await settle();
    assert(sent(before, 'gameOpenRoom')[0]?.room === 'crossroads_02', 'a room was not opened from the World');
    $('[data-game-side="layers"]').click(); await settle();
    const planes = [...ws().querySelectorAll('[data-game-pick="plane"]')].map((r) => r.dataset.gamePickId);
    assert(planes[0] === 'fronds' && planes[planes.length - 1] === 'sky', `the layers are not front first: ${planes}`);
    $('[data-game-side="entities"]').click(); await settle();
    const things = [...ws().querySelectorAll('[data-game-pick]')].map((r) => `${r.dataset.gamePick}:${r.dataset.gamePickId}`);
    for (const one of ['entity:to_east', 'entity:dash_gate', 'spawn:start', 'light:lantern', 'shape:floor']) assert(things.includes(one), `${one} is not listed: ${things}`);
    const canvas = $('[data-game-overlay]');
    assert(shown(canvas) && canvas.width > 0 && canvas.height > 0, 'the room is not drawn on the stage');
  });

  await test('The inspector edits by ops on the room, addressed by id: a plane\'s depth, the abilities a gate opens for', async () => {
    $('[data-game-side="layers"]').click(); await settle();
    ws().querySelector('[data-game-pick="plane"][data-game-pick-id="hills_far"]').click(); await settle();
    const depth = /** @type {HTMLInputElement} */ (ws().querySelector('[data-game-field=\'["planes","#hills_far","depth"]\']'));
    assert(depth && depth.value === '1500', 'the plane\'s depth is not in the inspector');
    let before = win.commands.length;
    depth.value = '1800';
    depth.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const edit = sent(before, 'gameEdit')[0];
    assert(edit?.target === 'room:crossroads_01' && JSON.stringify(edit.ops) === JSON.stringify([{op: 'set', path: ['planes', '#hills_far', 'depth'], value: 1800}]),
      `the depth was sent as ${JSON.stringify(edit)}`);
    $('[data-game-side="entities"]').click(); await settle();
    ws().querySelector('[data-game-pick="entity"][data-game-pick-id="dash_gate"]').click(); await settle();
    const twin = /** @type {HTMLInputElement} */ (ws().querySelector('[data-game-kind="abilities"][value="double_jump"]'));
    assert(twin && !twin.checked, 'the gate\'s abilities are not offered');
    before = win.commands.length;
    twin.checked = true;
    twin.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const gate = sent(before, 'gameEdit')[0];
    assert(JSON.stringify(gate?.ops?.[0]?.value) === JSON.stringify(['dash', 'double_jump']), `the gate was sent as ${JSON.stringify(gate)}`);
    // Remove, from the inspector.
    before = win.commands.length;
    ws().querySelector('[data-game-remove]').click(); await settle();
    const removed = sent(before, 'gameEdit')[0];
    assert(removed?.ops?.[0]?.op === 'remove' && removed.ops[0].path.join('/') === 'entities/#dash_gate', `remove sent ${JSON.stringify(removed)}`);
  });

  await test('Unsaved changes are dots, ⌘S saves and ⌘Z undoes, and leaving asks first', async () => {
    const dirty = project({dirty: true, canUndo: true, rooms: fixture.rooms.map((/** @type {any} */ r) => ({id: r.id, area: 'crossroads', size: r.size, dirty: r.id === 'crossroads_01'}))});
    await win.pushEvents([dirty]); await settle();
    assert(ws().querySelector('[data-game-tab="crossroads_01"] .dot'), 'the changed room\'s tab has no dot');
    assert(!$('[data-game-save]').disabled && !$('[data-game-undo]').disabled, 'Save or Undo is not offered');
    $('[data-game-stage]').focus();
    let before = win.commands.length;
    press('s', {metaKey: true}); await settle();
    assert(sent(before, 'gameSave').length === 1, '⌘S did not save');
    before = win.commands.length;
    press('z', {metaKey: true}); await settle();
    press('z', {metaKey: true, shiftKey: true}); await settle();
    assert(sent(before, 'gameUndo').length === 1 && sent(before, 'gameRedo').length === 1, '⌘Z or ⇧⌘Z did not reach the game');
    before = win.commands.length;
    $('[data-game-leave]').click(); await settle();
    assert(shown($('[data-game-ask]')) && !sent(before, 'gameClose').length, 'leaving with changes did not ask first');
    $('[data-game-ask-answer="discard"]').click(); await settle();
    assert(sent(before, 'gameClose')[0]?.discard === true, 'leaving without saving did not discard');
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
  });

  await test('The room\'s art is drawn under what is solid, a drag sends one edit, and a plane can be hidden or shown alone', async () => {
    const gl = $('[data-game-gl]');
    for (let i = 0; i < 40 && gl.width <= 300; i++) await settle();
    assert(shown(gl) && gl.width > 300, 'the art was not drawn');
    const canvas = $('[data-game-overlay]');
    const box = canvas.getBoundingClientRect();
    const first = fixture.rooms[0];
    const zoom = Math.min((box.width - 48) / first.size[0], (box.height - 48) / first.size[1]);
    const stage = (/** @type {number} */ x, /** @type {number} */ y) => [box.left + box.width / 2 + (x - first.size[0] / 2) * zoom, box.top + box.height / 2 + (y - first.size[1] / 2) * zoom];
    const fire = (/** @type {string} */ type, /** @type {number[]} */ [x, y]) => canvas.dispatchEvent(new win.PointerEvent(type, {clientX: x, clientY: y, bubbles: true, pointerId: 1, button: 0}));
    const [bx, by] = stage(700, 1800);
    const before = win.commands.length;
    fire('pointerdown', [bx, by]);
    for (let step = 1; step <= 4; step++) fire('pointermove', [bx + step * 10, by]);
    fire('pointerup', [bx + 40, by]);
    await settle();
    const edits = sent(before, 'gameEdit');
    assert(edits.length === 1, `a drag sent ${edits.length} edits`);
    const move = edits[0].ops[0];
    assert(move.op === 'set' && move.path.join('/') === 'entities/#bench/at' && move.value[1] === 1800 && Math.abs(move.value[0] - (700 + Math.round(40 / zoom))) <= 1,
      `the drag was sent as ${JSON.stringify(move)}`);
    // A plane hidden, then shown alone; neither is saved.
    $('[data-game-side="layers"]').click(); await settle();
    const eye = () => ws().querySelector('[data-game-hide="hills_far"]');
    eye().click(); await settle();
    assert(eye().getAttribute('aria-pressed') === 'false', 'the plane was not hidden');
    eye().dispatchEvent(new win.MouseEvent('click', {bubbles: true, altKey: true})); await settle();
    assert(eye().classList.contains('solo') && ws().querySelector('[data-game-hide="sky"]').getAttribute('aria-pressed') === 'false', 'the plane was not shown alone');
    // A click ends showing it alone, back to what was hidden before; another shows it.
    eye().click(); await settle();
    assert(!eye().classList.contains('solo') && eye().getAttribute('aria-pressed') === 'false', 'showing it alone did not end');
    eye().click(); await settle();
    assert(eye().getAttribute('aria-pressed') === 'true', 'showing it again did not');
    assert(!sent(before + 1, 'gameEdit').length, 'hiding a plane was sent as an edit');
  });

  await test('Collision is drawn with the pen, its points dragged one edit at a time, and an image on the play plane traced', async () => {
    const canvas = $('[data-game-overlay]');
    const first = fixture.rooms[0];
    // Measured each time: the pen's choices in the bar can wrap it, and the
    // stage — fitted to the room — changes with it.
    const stage = (/** @type {number} */ x, /** @type {number} */ y) => {
      const box = canvas.getBoundingClientRect();
      const zoom = Math.min((box.width - 48) / first.size[0], (box.height - 48) / first.size[1]);
      return [box.left + box.width / 2 + (x - first.size[0] / 2) * zoom, box.top + box.height / 2 + (y - first.size[1] / 2) * zoom];
    };
    const fire = (/** @type {string} */ type, /** @type {number[]} */ [x, y], /** @type {Record<string, any>} */ more = {}) =>
      canvas.dispatchEvent(new win.PointerEvent(type, {clientX: x, clientY: y, bubbles: true, pointerId: 1, button: 0, ...more}));
    $('[data-game-stage]').focus();
    press('p'); await settle();
    assert($('[data-game-tool="pen"]').getAttribute('aria-pressed') === 'true' && shown($('[data-game-pen-kind]')), 'p did not take up the pen');
    const kind = /** @type {HTMLSelectElement} */ ($('[data-game-pen-kind]'));
    kind.value = 'oneway';
    kind.dispatchEvent(new win.Event('change', {bubbles: true}));
    await settle();
    let before = win.commands.length;
    for (const [x, y] of [[1000, 1300], [1200, 1300], [1400, 1250]]) {
      fire('pointerdown', stage(x, y));
      fire('pointerup', stage(x, y));
    }
    press('Enter'); await settle();
    press('v'); await settle();
    const drawn = sent(before, 'gameEdit')[0];
    const value = drawn?.ops?.[0]?.value;
    assert(drawn?.ops?.[0]?.op === 'insert' && drawn.ops[0].path.join() === 'collision' && value.kind === 'oneway' && value.points.length === 3 && !value.closed,
      `the line was sent as ${JSON.stringify(drawn)}`);
    assert(Math.abs(value.points[1][0] - 1200) <= 6 && Math.abs(value.points[1][1] - 1300) <= 6, `a point landed at ${value.points[1]}`);
    // The floor, selected: its second point dragged.
    $('[data-game-side="entities"]').click(); await settle();
    ws().querySelector('[data-game-pick="shape"][data-game-pick-id="floor"]').click(); await settle();
    before = win.commands.length;
    const [vx, vy] = stage(2200, 1800);
    fire('pointerdown', [vx, vy]);
    for (let step = 1; step <= 3; step++) fire('pointermove', [vx, vy - step * 5]);
    fire('pointerup', [vx, vy - 15]); await settle();
    const dragged = sent(before, 'gameEdit');
    assert(dragged.length === 1 && dragged[0].ops[0].path.join('/') === 'collision/#floor/points', `a point's drag sent ${JSON.stringify(dragged)}`);
    const points = dragged[0].ops[0].value;
    assert(points.length === 4 && points[1][0] === 2200 && points[1][1] < 1800 && points[0][1] === 1800, `the floor became ${JSON.stringify(points)}`);
    // An image on the play plane: traced on request.
    $('[data-game-side="layers"]').click(); await settle();
    ws().querySelector('[data-game-pick="plane"][data-game-pick-id="main"]').click(); await settle();
    ws().querySelector('[data-game-pick="item"][data-game-pick-id="ground_1"]').click(); await settle();
    before = win.commands.length;
    $('[data-game-trace]').click(); await settle();
    const traced = sent(before, 'gameTrace')[0];
    assert(traced?.room === 'crossroads_01' && traced.item === 'ground_1', `Trace sent ${JSON.stringify(traced)}`);
    ws().querySelector('[data-game-pick="plane"][data-game-pick-id="hills_far"]').click(); await settle();
    ws().querySelector('[data-game-pick="item"][data-game-pick-id="far_0"]').click(); await settle();
    assert(!$('[data-game-trace]'), 'an image off the play plane offers to be traced into collision');
  });

  await test('A click on the stage selects what is there, ⎋ selects nothing, and ? lists Game\'s keys', async () => {
    const canvas = $('[data-game-overlay]');
    const box = canvas.getBoundingClientRect();
    const first = fixture.rooms[0];
    const scale = Math.min((box.width - 48) / first.size[0], (box.height - 48) / first.size[1]);
    const x = box.left + (box.width - first.size[0] * scale) / 2 + 400 * scale;
    const y = box.top + (box.height - first.size[1] * scale) / 2 + 1800 * scale;
    canvas.dispatchEvent(new win.PointerEvent('pointerdown', {clientX: x, clientY: y, bubbles: true}));
    await settle();
    assert($('[data-game-inspector] h4')?.textContent === 'start', `the click selected ${$('[data-game-inspector] h4')?.textContent}`);
    press('Escape'); await settle();
    assert(!ws().querySelector('[data-game-pick].on'), '⎋ left something selected');
    press('?'); await settle();
    assert(shown($('#whichkey')) && $('#whichkey').textContent.includes('Game\'s keys'), '? did not list Game\'s keys');
    doc.body.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Escape', bubbles: true})); await settle();
    $('[data-game-panel="agent"]').click(); await settle();
    assert(shown($('[data-game-body="agent"] [data-agent-input]')), 'the agent is not beside the room');
    $('[data-game-panel="inspector"]').click(); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('▶ plays the room in the stage by the runtime: the game has the keys, Step is one tick, and ⎋ stops and sends the run back', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    const frame = () => new Promise((done) => win.requestAnimationFrame(() => done(undefined)));
    const tick = () => Number(($('[data-game-tick]')?.textContent ?? '').match(/tick (\d+)/)?.[1] ?? -1);
    const until = async (/** @type {() => boolean} */ ok, what = '') => {
      for (let i = 0; i < 120 && !ok(); i++) await frame();
      assert(ok(), `timed out waiting for ${what}`);
    };
    let before = win.commands.length;
    $('[data-game-play]').click(); await settle();
    const asked = sent(before, 'gamePlay')[0];
    assert(asked?.room === 'crossroads_01' && Array.isArray(asked.abilities), `▶ sent ${JSON.stringify(asked)}`);
    // The core's answer: the world it cooked, where to start, the keys.
    const world = await (await fetch('/tests/ui/fixtures/game-world.json')).text();
    const parsed = JSON.parse(world);
    const keys = [['ArrowLeft', 1], ['ArrowRight', 2], ['KeyZ', 16], ['Space', 16]];
    const start = [0, ...parsed.rooms[0].spawns[0][1], 0];
    await win.pushEvents([{type: 'gamePlay', world, start, rooms: fixture.rooms, keys, replay: null}]);
    await until(() => tick() > 3, 'the run to tick');
    assert(ws().classList.contains('game-playing') && !$('[data-game-stop]').disabled, 'the stage is not playing');
    // Held keys are the game's: none reaches the core or the editor's keys.
    before = win.commands.length;
    const hold = (/** @type {string} */ type, /** @type {string} */ code, /** @type {string} */ k) =>
      $('[data-game-stage]').dispatchEvent(new win.KeyboardEvent(type, {key: k, code, bubbles: true, cancelable: true}));
    hold('keydown', 'ArrowRight', 'ArrowRight');
    const from = tick();
    await until(() => tick() > from + 10, 'ticks with → held');
    hold('keyup', 'ArrowRight', 'ArrowRight');
    hold('keydown', 'Slash', '?'); await frame();
    assert(!shown($('#whichkey')), '? reached the editor while the game played');
    assert(!sent(before, 'key').length, 'a key went to the core while the game played');
    // Paused, it holds; Step is exactly one tick.
    $('[data-game-pause]').click(); await frame(); await frame();
    const held = tick();
    await frame(); await frame(); await frame();
    assert(tick() === held, `paused, the run went on: ${held} → ${tick()}`);
    $('[data-game-step]').click(); await frame();
    assert(tick() === held + 1, `Step went from ${held} to ${tick()}`);
    // ⎋ stops it, and the run goes to the core to be kept and checked.
    before = win.commands.length;
    doc.activeElement?.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Escape', code: 'Escape', bubbles: true, cancelable: true}));
    await settle();
    assert(!ws().classList.contains('game-playing') && $('[data-game-stop]').disabled, '⎋ did not stop the run');
    const run = sent(before, 'gameRecorded')[0];
    assert(run && run.room === 0 && /^[0-9a-f]{16}$/.test(run.hash), `the run was not sent back: ${JSON.stringify(run)?.slice(0, 200)}`);
    assert(run.replay.reduce((/** @type {number} */ n, /** @type {number[]} */ r) => n + r[1], 0) === held + 1, 'the run sent back is not as long as it was played');
    assert(run.replay.some((/** @type {number[]} */ r) => r[0] === 2), 'the run sent back has no → in it');
    // A run played back plays to its end, and stops there to be looked at.
    await win.pushEvents([{type: 'gamePlay', world, start, rooms: fixture.rooms, keys, replay: [[2, 12], [0, 6]]}]);
    await until(() => tick() === 18 && Boolean(win.document.querySelector('[data-game-tick]')?.textContent.includes('paused')), 'the playback to end');
    before = win.commands.length;
    $('[data-game-stop]').click(); await settle();
    assert(!sent(before, 'gameRecorded').length, 'a run played back was sent back as a new run');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('The agent\'s run is drawn over the room it crossed until ⎋, and @game is a chip beside the agent', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    press('f'); await settle();
    const canvas = $('[data-game-overlay]');
    const first = fixture.rooms[0];
    /** Whether the overlay draws anything in a few pixels round a room point. */
    const inked = (/** @type {number} */ rx, /** @type {number} */ ry) => {
      const box = canvas.getBoundingClientRect();
      const scale = Math.min((box.width - 48) / first.size[0], (box.height - 48) / first.size[1]);
      const ratio = canvas.width / box.width;
      const x = ((box.width - first.size[0] * scale) / 2 + rx * scale) * ratio;
      const y = ((box.height - first.size[1] * scale) / 2 + ry * scale) * ratio;
      const data = canvas.getContext('2d').getImageData(Math.round(x - 6), Math.round(y - 6), 12, 12).data;
      for (let i = 3; i < data.length; i += 4) if (data[i] > 0) return true;
      return false;
    };
    assert(!inked(1500, 900), 'something is already drawn where the run will go');
    const points = Array.from({length: 60}, (_, i) => [600 + i * 40, 900]);
    await win.pushEvents([{type: 'gamePath', parts: [{room: 'crossroads_01', points, hurt: [[2000, 900]]}, {room: 'crossroads_02', points: [[0, 0], [50, 0]], hurt: []}]}]);
    await settle();
    assert(inked(1500, 900) && inked(2000, 900), 'the agent\'s run is not drawn over the room');
    press('Escape'); await settle();
    assert(!inked(1500, 900), '⎋ with nothing selected did not clear the agent\'s run');
    $('[data-game-panel="agent"]').click(); await settle();
    const chip = $('[data-game-body="agent"] [data-mention="@game"]');
    assert(chip && shown(chip), 'there is no @game chip beside the agent');
    chip.click(); await settle();
    assert($('[data-game-body="agent"] [data-agent-input]').textContent.includes('@game'), 'the chip did not put @game in the question');
    $('[data-game-body="agent"] [data-agent-input]').textContent = '';
    $('[data-game-panel="inspector"]').click(); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('A layered file is imported from its inspector, and an image cut into pieces is loaded piece by piece and picked as one', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    // An image cut in two, placed high on the play plane.
    const sky = await (await fetch('/tests/ui/fixtures/game/art/sky.png')).arrayBuffer();
    win.gameFiles = {
      'art/cut.img.json': JSON.stringify({format: 1, source: 'cut.psd', size: [400, 200], chunks: [
        {file: 'art/cut/0_0.png', at: [0, 0], size: [200, 200]}, {file: 'art/cut/1_0.png', at: [200, 0], size: [200, 200]}]}),
      'art/cut/0_0.png': sky, 'art/cut/1_0.png': sky,
    };
    const first = structuredClone(fixture.rooms[0]);
    first.planes.find((/** @type {any} */ p) => p.gameplay).items.push({id: 'cut', image: 'art/cut.img.json', at: [1100, 300], scale: 1, rot: 0, flip: false, opacity: 1, blend: 'normal'});
    const reads = (win.invokes ?? []).length;
    await win.pushEvents([project({assets: ['art/sky.png', 'art/cut.img.json', 'src/forest.psd']}), {type: 'gameRoom', room: first, revision: 7}]); await settle();
    press('f'); await settle();
    const read = () => (win.invokes ?? []).slice(reads).filter((/** @type {any} */ c) => c.name === 'read_game').map((/** @type {any} */ c) => c.args.key);
    for (let i = 0; i < 30 && !read().includes('art/cut/1_0.png'); i++) await settle();
    assert(read().includes('art/cut.img.json') && read().includes('art/cut/0_0.png') && read().includes('art/cut/1_0.png'), `read ${read()}`);
    await settle();
    // A click on its second piece is a click on it.
    const canvas = $('[data-game-overlay]');
    const box = canvas.getBoundingClientRect();
    const zoom = Math.min((box.width - 48) / first.size[0], (box.height - 48) / first.size[1]);
    const [x, y] = [box.left + box.width / 2 + (1400 - first.size[0] / 2) * zoom, box.top + box.height / 2 + (400 - first.size[1] / 2) * zoom];
    canvas.dispatchEvent(new win.PointerEvent('pointerdown', {clientX: x, clientY: y, bubbles: true, pointerId: 1, button: 0}));
    canvas.dispatchEvent(new win.PointerEvent('pointerup', {clientX: x, clientY: y, bubbles: true, pointerId: 1, button: 0}));
    await settle();
    // An item is titled by its image.
    assert($('[data-game-inspector] h4')?.textContent === 'cut.img.json', `the click selected ${$('[data-game-inspector] h4')?.textContent}`);
    press('Escape'); await settle();

    // The layered file, from the Assets tab: it is brought in, not dragged.
    $('[data-game-side="assets"]').click(); await settle();
    const row = ws().querySelector('[data-game-pick="asset"][data-game-pick-id="src/forest.psd"]');
    assert(row && row.getAttribute('draggable') !== 'true', 'a .psd is offered to drag onto the stage');
    assert(ws().querySelector('[data-game-pick-id="art/cut.img.json"]')?.getAttribute('draggable') === 'true', 'an image cut up cannot be placed');
    row.click(); await settle();
    const form = $('[data-game-import-form]');
    assert(form && shown(form), 'selecting a .psd did not offer to import it');
    assert(form.querySelector('[data-game-import-room]').value === 'crossroads_01', 'it would not go into the room on screen');
    form.querySelector('[data-game-import-scale]').value = '2';
    form.querySelector('[data-game-import-normals]').checked = true;
    const before = win.commands.length;
    form.querySelector('[data-game-import]').click(); await settle();
    const asked = sent(before, 'gameImport')[0];
    assert(asked?.source === 'src/forest.psd' && asked.room === 'crossroads_01' && asked.scale === 2 && asked.normals === true, `Import sent ${JSON.stringify(asked)}`);
    // Imported, and saved again since: said on its row and in the inspector.
    await win.pushEvents([project({assets: ['art/sky.png', 'art/cut.img.json', 'src/forest.psd'],
      imports: [{source: 'src/forest.psd', room: 'crossroads_02', planes: 3, items: 1, scale: 2, normals: true, changed: true}]})]);
    await settle();
    assert(ws().querySelector('[data-game-pick-id="src/forest.psd"] .game-changed'), 'a file changed since it was imported is not marked');
    assert($('[data-game-import]').textContent === 'Import again' && $('[data-game-import-form]').textContent.includes('saved since'), 'the inspector does not say it was imported, and changed');
    assert($('[data-game-import-room]').value === 'crossroads_02', 'importing again would not go where it went before');
    win.gameFiles = undefined;
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('An Aseprite file is imported as an animation, which its inspector plays, the player looks like, and a run plays frame by frame', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    const frame = () => new Promise((done) => win.requestAnimationFrame(() => done(undefined)));
    const sheet = await (await fetch('/tests/ui/fixtures/game/art/player.png')).arrayBuffer();
    const anim = {format: 1, source: 'art/hero.aseprite', size: [24, 24], sheets: ['art/hero.sheet.png'],
      frames: [0, 1, 2, 3, 4].map((i) => ({sheet: 0, rect: [i * 4, 0, 4, 16], offset: [8, 8], ms: 40, pivot: [12, 24]})),
      clips: [{name: 'idle', from: 0, to: 0, direction: 'forward', repeat: 0}, {name: 'run', from: 1, to: 3, direction: 'pingPong', repeat: 0}, {name: 'jump', from: 4, to: 4, direction: 'forward', repeat: 1}],
      boxes: [{name: 'hitbox', kind: 'hit', frames: [null, [0, 0, 8, 8], null, null, null]}], cues: [{frame: 2, name: 'step'}]};
    win.gameFiles = {'art/hero.anim.json': JSON.stringify(anim), 'art/hero.sheet.png': sheet};
    const assets = ['art/hero.aseprite', 'art/hero.anim.json', 'art/sky.png'];
    await win.pushEvents([project({assets}), room('crossroads_01')]); await settle();
    // The Aseprite file: offered as an animation, and imported as one.
    $('[data-game-side="assets"]').click(); await settle();
    ws().querySelector('[data-game-pick-id="art/hero.aseprite"]').click(); await settle();
    const form = $('[data-game-import-form]');
    assert(form && form.textContent.includes('Import as an animation') && !form.querySelector('[data-game-import-room]'), 'an .aseprite is not offered as an animation');
    let before = win.commands.length;
    form.querySelector('[data-game-import]').click(); await settle();
    assert(sent(before, 'gameImport')[0]?.source === 'art/hero.aseprite', `Import sent ${JSON.stringify(sent(before, 'gameImport'))}`);
    // The animation: its clips, one played small, frame by frame.
    ws().querySelector('[data-game-pick-id="art/hero.anim.json"]').click();
    const panel = () => $('[data-game-anim]');
    for (let i = 0; i < 30 && !panel()?.querySelector('[data-game-preview-clip]'); i++) await settle();
    assert(panel()?.querySelectorAll('[data-game-preview-clip]').length === 3 && panel().textContent.includes('hitbox (hit)') && panel().textContent.includes('step at frame 3'), `the animation says ${panel()?.textContent}`);
    panel().querySelector('[data-game-preview-clip="run"]').click();
    const shownFrames = new Set();
    for (let i = 0; i < 40 && shownFrames.size < 3; i++) {
      shownFrames.add($('[data-game-anim-preview]')?.dataset.frame);
      await new Promise((done) => setTimeout(done, 15));
    }
    assert([...shownFrames].every((f) => ['1', '2', '3'].includes(f)) && shownFrames.size === 3, `the run played frames ${[...shownFrames]}`);
    assert($('[data-game-anim-preview]').width === 144, `the preview is ${$('[data-game-anim-preview]').width} wide`);
    before = win.commands.length;
    panel().querySelector('[data-game-anim-player]').click(); await settle();
    const given = sent(before, 'gameEdit')[0];
    assert(given?.target === 'prefab:player' && given.ops[0].path.join() === 'sprite' && given.ops[0].value === 'art/hero.anim.json', `the player was given ${JSON.stringify(given)}`);
    // The player looks like it: drawn at the start as the frame it rests on;
    // its inspector chooses each state's clip.
    const prefabs = fixture.prefabs.map((/** @type {any} */ p) => (p.id === 'player' ? {...p, sprite: 'art/hero.anim.json'} : p));
    await win.pushEvents([project({assets, prefabs}), room('crossroads_01')]); await settle();
    for (let i = 0; i < 30 && !ws().gameProbe.drawn().includes('art/hero.sheet.png'); i++) await settle();
    assert(ws().gameProbe.drawn().includes('art/hero.sheet.png'), 'the player is not drawn from its sheet');
    $('[data-game-side="world"]').click(); await settle();
    ws().querySelector('[data-game-pick="prefab"][data-game-pick-id="player"]').click(); await settle();
    const fall = /** @type {HTMLSelectElement} */ ($('[data-game-clip="fall"]'));
    assert(fall && [...fall.options].map((o) => o.value).join() === ',idle,run,jump', 'the player\'s states cannot choose its clips');
    before = win.commands.length;
    fall.value = 'jump';
    fall.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const chose = sent(before, 'gameEdit');
    assert(chose.length === 1 && chose[0].target === 'prefab:player' && JSON.stringify(chose[0].ops[0]) === '{"op":"set","path":["clips"],"value":{"fall":"jump"}}', `choosing sent ${JSON.stringify(chose)}`);
    // Played: the run shows its frames and says its cue.
    const animated = JSON.parse(await (await fetch('/tests/ui/fixtures/game-anim.json')).text());
    await win.pushEvents([{type: 'gamePlay', world: animated.world, start: animated.start, rooms: fixture.rooms, keys: [['ArrowRight', 2]], replay: [[2, 90]]}]);
    const seen = new Set();
    for (let i = 0; i < 200 && !(seen.has(2) && ws().gameProbe.cue() === 'step'); i++) {
      const words = ws().gameProbe.frame();
      if (words && words[8] > 0) seen.add(words[16 + 6]);
      await frame();
    }
    assert(seen.has(1) && seen.has(2) && ws().gameProbe.cue() === 'step', `the run showed frames ${[...seen]}, said ${ws().gameProbe.cue()}`);
    $('[data-game-stop]').click(); await settle();
    win.gameFiles = undefined;
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('A DragonBones armature is imported as a skeleton, posed small in its inspector, drawn on the player, and played by the tick', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    const frame = () => new Promise((done) => win.requestAnimationFrame(() => done(undefined)));
    const fixtureSkel = JSON.parse(await (await fetch('/tests/ui/fixtures/game-skel.json')).text());
    const atlas = await (await fetch('/tests/ui/fixtures/game/art/player.png')).arrayBuffer();
    win.gameFiles = {'art/hero.skel.json': fixtureSkel.skel, 'art/hero_tex.png': atlas};
    const assets = ['art/hero_ske.json', 'art/hero.skel.json', 'art/sky.png'];
    await win.pushEvents([project({assets}), room('crossroads_01')]); await settle();
    // The armature: offered as a skeleton.
    $('[data-game-side="assets"]').click(); await settle();
    ws().querySelector('[data-game-pick-id="art/hero_ske.json"]').click(); await settle();
    const form = $('[data-game-import-form]');
    assert(form?.textContent.includes('Import as a skeleton'), `an armature is offered as ${form?.querySelector('h4')?.textContent}`);
    let before = win.commands.length;
    form.querySelector('[data-game-import]').click(); await settle();
    assert(sent(before, 'gameImport')[0]?.source === 'art/hero_ske.json', 'the armature was not imported');
    // The skeleton: its clips, the wave posed small and moving.
    ws().querySelector('[data-game-pick-id="art/hero.skel.json"]').click();
    const panel = () => $('[data-game-anim]');
    for (let i = 0; i < 30 && !panel()?.querySelector('[data-game-preview-clip]'); i++) await settle();
    assert(panel()?.textContent.includes('3 bones') && panel().querySelectorAll('[data-game-preview-clip]').length === 2 && panel().textContent.includes('swish in idle'), `the skeleton says ${panel()?.textContent}`);
    const posed = new Set();
    for (let i = 0; i < 40 && posed.size < 3; i++) {
      posed.add($('[data-game-anim-preview]')?.dataset.frame);
      await new Promise((done) => setTimeout(done, 20));
    }
    assert(posed.size >= 3 && $('[data-game-anim-preview]').width === 160, `the preview posed frames ${[...posed]}`);
    // The player as a skeleton: drawn from its atlas where the run starts.
    const prefabs = fixture.prefabs.map((/** @type {any} */ p) => (p.id === 'player' ? {...p, sprite: 'art/hero.skel.json', pivot: [0, 0]} : p));
    await win.pushEvents([project({assets, prefabs}), room('crossroads_01')]); await settle();
    for (let i = 0; i < 30 && !ws().gameProbe.drawn().includes('art/hero_tex.png'); i++) await settle();
    assert(ws().gameProbe.drawn().includes('art/hero_tex.png'), `the player is not drawn from its atlas: ${ws().gameProbe.drawn()}`);
    $('[data-game-side="world"]').click(); await settle();
    ws().querySelector('[data-game-pick="prefab"][data-game-pick-id="player"]').click(); await settle();
    assert([...(/** @type {HTMLSelectElement} */ ($('[data-game-clip="jump"]'))?.options ?? [])].map((o) => o.value).join() === ',idle,jump', 'a skeleton\'s clips are not offered for its states');
    // Played: its ticks, its cue.
    before = win.commands.length;
    await win.pushEvents([{type: 'gamePlay', world: fixtureSkel.world, start: fixtureSkel.start, rooms: fixture.rooms, keys: [['ArrowRight', 2]], replay: [[0, 80]]}]);
    const seen = new Set();
    for (let i = 0; i < 200 && !(seen.size > 5 && ws().gameProbe.cue() === 'swish'); i++) {
      const words = ws().gameProbe.frame();
      if (words && words[8] > 0) seen.add(words[16 + 6]);
      await frame();
    }
    assert(seen.size > 5 && [...seen].every((f) => f >= 0 && f < 30) && ws().gameProbe.cue() === 'swish', `the run posed ticks ${[...seen]}, said ${ws().gameProbe.cue()}`);
    $('[data-game-stop]').click(); await settle();
    win.gameFiles = undefined;
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('Lights are placed with l and set in the inspector, fog and the finish are fields, ⇧P shows the finish, and a run says its frame times', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    // l, then a click: a light on the play plane, where the click was.
    press('l'); await settle();
    assert($('[data-game-tool="light"]').getAttribute('aria-pressed') === 'true', 'l did not take the light tool');
    const canvas = $('[data-game-overlay]');
    const box = canvas.getBoundingClientRect();
    let before = win.commands.length;
    canvas.dispatchEvent(new win.PointerEvent('pointerdown', {clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, bubbles: true, pointerId: 1, button: 0}));
    canvas.dispatchEvent(new win.PointerEvent('pointerup', {clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, bubbles: true, pointerId: 1, button: 0}));
    await settle();
    const placed = sent(before, 'gameEdit')[0];
    const value = placed?.ops[0].value;
    assert(placed?.ops[0].op === 'insert' && placed.ops[0].path.join() === 'lights' && value.plane === 'main' && value.id === 'light_2' && value.glow === 0.3,
      `the light placed was ${JSON.stringify(placed)}`);
    assert(Math.abs(value.at[0] - 1920) < 60 && Math.abs(value.at[1] - 1080) < 60, `the light went to ${value.at}`);
    assert($('[data-game-tool="select"]').getAttribute('aria-pressed') === 'true', 'the tool stayed on lights after one');
    // The template's lantern: its glow and height are fields.
    $('[data-game-side="entities"]').click(); await settle();
    ws().querySelector('[data-game-pick="light"][data-game-pick-id="lantern"]').click(); await settle();
    const glow = /** @type {HTMLInputElement} */ (ws().querySelector('[data-game-field=\'["lights","#lantern","glow"]\']'));
    assert(glow?.value === '0.35' && ws().querySelector('[data-game-field=\'["lights","#lantern","height"]\']'), 'the light has no glow or height to set');
    // The room's fog: blank, its colour makes the whole of it; cleared, takes it away.
    press('Escape'); await settle();
    const bare = structuredClone(fixture.rooms[0]);
    bare.ambience.fog = null;
    await win.pushEvents([{type: 'gameRoom', room: bare, revision: 30}]); await settle();
    const fog = /** @type {HTMLInputElement} */ (ws().querySelector('[data-game-field=\'["ambience","fog","color"]\']'));
    before = win.commands.length;
    fog.value = '#405060';
    fog.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const made = sent(before, 'gameEdit')[0]?.ops[0];
    assert(made?.path.join() === 'ambience,fog' && made.value.color === '#405060' && made.value.far === 3000, `fog's colour sent ${JSON.stringify(made)}`);
    await win.pushEvents([room('crossroads_01', 31)]); await settle();
    const fogged = /** @type {HTMLInputElement} */ (ws().querySelector('[data-game-field=\'["ambience","fog","color"]\']'));
    assert(fogged.value === '#8fa3c7' && ws().querySelector('[data-game-field=\'["ambience","fog","far"]\']'), 'the template\'s fog is not shown');
    before = win.commands.length;
    fogged.value = '';
    fogged.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(JSON.stringify(sent(before, 'gameEdit')[0]?.ops) === '[{"op":"set","path":["ambience","fog"],"value":null}]', 'clearing the fog did not take it away');
    // The finish: the game's fields; no grade is none.
    $('[data-game-side="world"]').click(); await settle();
    ws().querySelector('[data-game-pick="game"]').click(); await settle();
    const grade = /** @type {HTMLSelectElement} */ (ws().querySelector('[data-game-field=\'["post","grade"]\']'));
    assert(grade && ws().querySelector('[data-game-field=\'["post","bloom"]\']') && ws().querySelector('[data-game-field=\'["post","vignette"]\']'), 'the finish is not in the game\'s settings');
    grade.value = 'art/sky.png';
    grade.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    before = win.commands.length;
    grade.value = '';
    grade.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(sent(before, 'gameEdit')[0]?.ops[0].value === null, 'no grade was sent as something');
    press('P'); await settle();
    assert($('[data-game-post]').getAttribute('aria-pressed') === 'true', '⇧P did not show the finish');
    press('P'); await settle();
    // The Run tab: the finish's quality kept; a run's frame times read out.
    $('[data-game-panel="run"]').click(); await settle();
    const quality = /** @type {HTMLSelectElement} */ ($('[data-game-quality]'));
    quality.value = 'high';
    quality.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(win.localStorage.getItem('proton.game.quality') === 'high', 'the quality was not kept');
    const times = /** @type {HTMLInputElement} */ ($('[data-game-times]'));
    times.checked = true;
    times.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const world = await (await fetch('/tests/ui/fixtures/game-world.json')).text();
    await win.pushEvents([{type: 'gamePlay', world, start: [0, ...JSON.parse(world).rooms[0].spawns[0][1], 0], rooms: fixture.rooms, keys: [], replay: [[0, 40]]}]);
    const frame = () => new Promise((done) => win.requestAnimationFrame(() => done(undefined)));
    for (let i = 0; i < 60 && !(ws().gameProbe.frame()?.[1] > 20); i++) await frame();
    assert(ws().gameProbe.frame()?.[1] > 20, 'the run did not go');
    const took = ws().gameProbe.times();
    assert(took.frame > 0 && took.draw >= 0, `the run's frame times are ${JSON.stringify(took)}`);
    $('[data-game-stop]').click(); await settle();
    times.checked = false;
    times.dispatchEvent(new win.Event('change', {bubbles: true}));
    quality.value = 'medium';
    quality.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('Combat is set in the inspector — a prefab\'s health and behaviour, the game\'s swing, an entity\'s patrol and flag — a prefab dragged in is placed, and a fight is played with its health and boxes', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    await win.pushEvents([project(), room('crossroads_02')]); await settle();
    const field = (/** @type {string[]} */ path) => /** @type {HTMLInputElement | null} */ (ws().querySelector(`[data-game-field='${JSON.stringify(path)}']`));
    const change = async (/** @type {HTMLInputElement} */ input, /** @type {string} */ value) => {
      const before = win.commands.length;
      input.value = value;
      input.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
      return sent(before, 'gameEdit')[0];
    };
    // The crawler: its health, and its behaviour's numbers with their defaults.
    $('[data-game-side="world"]').click(); await settle();
    ws().querySelector('[data-game-pick="prefab"][data-game-pick-id="crawler"]').click(); await settle();
    assert(field(['health'])?.value === '2' && field(['behaviour', 'params', 'speed'])?.value === '90' && field(['behaviour', 'params', 'damage'])?.value === '1',
      'the crawler\'s health and numbers are not in the inspector');
    const faster = await change(/** @type {HTMLInputElement} */ (field(['behaviour', 'params', 'speed'])), '140');
    assert(faster?.target === 'prefab:crawler' && JSON.stringify(faster.ops) === '[{"op":"set","path":["behaviour","params","speed"],"value":140}]', `speed sent ${JSON.stringify(faster)}`);
    const row = ws().querySelector('[data-game-pick="prefab"][data-game-pick-id="crawler"]');
    assert(row.getAttribute('draggable') === 'true' && ws().querySelector('[data-game-pick="prefab"][data-game-pick-id="player"]').getAttribute('draggable') !== 'true', 'only things a room places can be dragged');
    // The game's swing.
    ws().querySelector('[data-game-pick="game"]').click(); await settle();
    assert(field(['combat', 'damage'])?.value === '1' && field(['combat', 'hitstop'])?.value === '4' && field(['combat', 'pogo'])?.value === '0.8', 'the swing is not in the game\'s settings');
    const stop = await change(/** @type {HTMLInputElement} */ (field(['combat', 'hitstop'])), '6');
    assert(stop?.target === 'game' && stop.ops[0].path.join() === 'combat,hitstop' && stop.ops[0].value === 6, `hitstop sent ${JSON.stringify(stop)}`);
    // The crawler in the room: its patrol, which way it faces, the flag it sets.
    $('[data-game-side="entities"]').click(); await settle();
    ws().querySelector('[data-game-pick="entity"][data-game-pick-id="crawler_1"]').click(); await settle();
    const patrol = [...ws().querySelectorAll(`[data-game-field='${JSON.stringify(['entities', '#crawler_1', 'props', 'patrol'])}']`)].map((i) => i.value);
    assert(patrol.join() === '1100,1700', `the patrol shows ${patrol}`);
    const flag = await change(/** @type {HTMLInputElement} */ (field(['entities', '#crawler_1', 'props', 'flag'])), 'first_blood');
    assert(JSON.stringify(flag?.ops) === '[{"op":"set","path":["entities","#crawler_1","props","flag"],"value":"first_blood"}]', `the flag sent ${JSON.stringify(flag)}`);
    assert(field(['entities', '#crawler_1', 'props', 'facing']), 'which way it faces is not a field');
    // Dragged from the list onto the stage: placed where it was dropped.
    $('[data-game-side="world"]').click(); await settle();
    const dt = new win.DataTransfer();
    ws().querySelector('[data-game-pick="prefab"][data-game-pick-id="crawler"]').dispatchEvent(new win.DragEvent('dragstart', {dataTransfer: dt, bubbles: true}));
    const stage = $('[data-game-stage]');
    const box = stage.getBoundingClientRect();
    let before = win.commands.length;
    stage.dispatchEvent(new win.DragEvent('drop', {dataTransfer: dt, clientX: box.left + box.width / 2, clientY: box.top + box.height / 2, bubbles: true, cancelable: true}));
    await settle();
    const placed = sent(before, 'gameEdit')[0];
    assert(placed?.target === 'room:crossroads_02' && placed.ops[0].op === 'insert' && placed.ops[0].path.join() === 'entities' && placed.ops[0].value.id === 'crawler_2' && placed.ops[0].value.prefab === 'crawler',
      `dropping the crawler sent ${JSON.stringify(placed)}`);
    // A fight played: things drawn with who they are, the player's health as
    // pips, the boxes the run tests.
    $('[data-game-panel="run"]').click(); await settle();
    const boxes = /** @type {HTMLInputElement} */ ($('[data-game-boxes]'));
    boxes.checked = true;
    boxes.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const fight = JSON.parse(await (await fetch('/tests/ui/fixtures/game-fight.json')).text());
    await win.pushEvents([{type: 'gamePlay', world: fight.world, start: fight.start, rooms: fixture.rooms, keys: [], replay: fight.replay}]);
    const frame = () => new Promise((done) => win.requestAnimationFrame(() => done(undefined)));
    /** @type {Set<number>} */
    const who = new Set();
    let flashed = false;
    for (let i = 0; i < 400 && !(who.size >= 4 && flashed); i++) {
      const words = ws().gameProbe.frame();
      for (let r = 0; words && r < words[8]; r++) {
        who.add(words[16 + r * 9 + 8]);
        flashed ||= Boolean(words[16 + r * 9 + 5] & 2);
      }
      await frame();
    }
    assert(who.has(0) && who.size >= 4 && flashed, `the fight drew ${[...who]}, flashed ${flashed}`);
    const words = ws().gameProbe.frame();
    assert(words[13] === 5 && words[12] <= 5, `the header's health is ${words[12]} of ${words[13]}`);
    const tested = ws().gameProbe.boxes();
    assert(tested.length >= 2 && tested.some((/** @type {number[]} */ b) => b[0] === 0), `the boxes tested are ${JSON.stringify(tested)}`);
    $('[data-game-stop]').click(); await settle();
    boxes.checked = false;
    boxes.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('Feel and sound are set in the game\'s settings — look-ahead, shake, squash, loudness, a sound for each event — an area has its music, a sound can be heard, and a run wakes the sound, kicks up dust and can be silenced', async () => {
    await win.dispatch({type: 'setMode', mode: 'game'}); await settle();
    await win.pushEvents([project({assets: ['art/player.png', 'audio/jump.wav', 'audio/land.wav', 'audio/theme.mp3']}), room('crossroads_01')]); await settle();
    const field = (/** @type {string[]} */ path) => /** @type {HTMLInputElement | null} */ (ws().querySelector(`[data-game-field='${JSON.stringify(path)}']`));
    $('[data-game-side="world"]').click(); await settle();
    ws().querySelector('[data-game-pick="game"]').click(); await settle();
    assert(field(['feel', 'lookahead'])?.value === '96' && field(['feel', 'shake'])?.value === '0.5' && field(['audio', 'effects'])?.value === '0.8', 'feel and loudness are not in the game\'s settings');
    // The template's sounds, event by event; a choice sends the whole map.
    const jump = /** @type {HTMLSelectElement} */ (ws().querySelector('[data-game-sound="jump"]'));
    assert(jump?.value === 'audio/jump.wav' && ws().querySelector('[data-game-sound="enter"]')?.value === '', `the jump's sound shows ${jump?.value}`);
    let before = win.commands.length;
    jump.value = '';
    jump.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const map = sent(before, 'gameEdit')[0];
    assert(map?.target === 'game' && map.ops[0].path.join() === 'audio,sounds' && !('jump' in map.ops[0].value) && map.ops[0].value.land === 'audio/land.wav',
      `clearing the jump's sound sent ${JSON.stringify(map)}`);
    // An area: its music.
    ws().querySelector('[data-game-pick="area"][data-game-pick-id="crossroads"]').click(); await settle();
    const music = /** @type {HTMLSelectElement} */ (field(['areas', '#crossroads', 'music']));
    assert(music && [...music.options].map((o) => o.value).includes('audio/theme.mp3') && !ws().querySelector('[data-game-sound]'), 'the area has no music to choose');
    before = win.commands.length;
    music.value = 'audio/theme.mp3';
    music.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const chose = sent(before, 'gameEdit')[0];
    assert(chose?.target === 'world' && JSON.stringify(chose.ops) === '[{"op":"set","path":["areas","#crossroads","music"],"value":"audio/theme.mp3"}]', `music sent ${JSON.stringify(chose)}`);
    before = win.commands.length;
    music.value = '';
    music.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(sent(before, 'gameEdit')[0]?.ops[0].value === null, 'no music was sent as something');
    // A sound in the assets can be heard.
    $('[data-game-side="assets"]').click(); await settle();
    ws().querySelector('[data-game-pick="asset"][data-game-pick-id="audio/jump.wav"]').click(); await settle();
    assert(ws().querySelector('[data-game-listen="audio/jump.wav"]'), 'a sound cannot be listened to');
    // Play wakes the sound; a jump kicks up dust, a landing more.
    $('[data-game-panel="run"]').click(); await settle();
    $('[data-game-play]').click(); await settle();
    assert(ws().gameProbe.audio() === 'running', `the sound is ${ws().gameProbe.audio()} after Play`);
    const world = await (await fetch('/tests/ui/fixtures/game-world.json')).text();
    await win.pushEvents([{type: 'gamePlay', world, start: [0, ...JSON.parse(world).rooms[0].spawns[0][1], 0], rooms: fixture.rooms, keys: [], replay: [[0, 20], [16, 3], [0, 120]]}]);
    const frame = () => new Promise((done) => win.requestAnimationFrame(() => done(undefined)));
    const seen = new Set();
    for (let i = 0; i < 300 && !(seen.has('jump') && seen.has('land')); i++) {
      for (const kind of ws().gameProbe.sparks()) seen.add(kind);
      await frame();
    }
    assert(seen.has('jump') && seen.has('land'), `the run kicked up ${[...seen]}`);
    $('[data-game-stop]').click(); await settle();
    const mute = /** @type {HTMLInputElement} */ ($('[data-game-mute]'));
    mute.checked = true;
    mute.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(win.localStorage.getItem('proton.game.muted') === '1', 'silence was not kept');
    mute.checked = false;
    mute.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    await win.pushEvents([project(), room('crossroads_01')]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
