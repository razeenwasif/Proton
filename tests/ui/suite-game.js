/* Game mode's checks, run by suite.js in the same fixture window: the newer
 * modes' chrome, the home page, a game's rooms as tabs, the World, Layers
 * and Entities tabs, the inspector's edits as ops, unsaved dots, ⌘S, ⌘Z,
 * asking before leaving, and a click on the stage. The game is the fixture
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
    canUndo: false, canRedo: false, problems: [], assets: ['art/player.png', 'art/sky.png'], ...over}});
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
}
