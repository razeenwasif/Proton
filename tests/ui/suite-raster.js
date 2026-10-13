/* Canvas mode's checks, run by suite.js in the same fixture window: the newer
 * modes' chrome and ⌃=, the home page and a new canvas, a canvas opened into
 * the window's own copy and its tiles fetched, an edit made here first and
 * checked against the core's hash — fetched again when it differs — tiles the
 * core changed itself, a layer's opacity previewed while dragged, and ⌘S.
 *
 * The core is stood in for by a second copy of the engine, here in the test:
 * it answers `read_raster`, and makes each edit to give the hash the core
 * would. */

import {createRasterRuntime, tilesPayload} from '/raster-runtime.js';

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, shown: (el: any) => boolean}} t */
export async function rasterChecks({test, assert, win, doc, $, settle, shown}) {
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const ws = () => $('.workspace[data-ws="raster"]');
  const probe = () => ws().rasterProbe;
  const reads = () => (win.invokes ?? []).filter((/** @type {any} */ c) => c.name === 'read_raster').length;
  const frame = () => new Promise((done) => win.requestAnimationFrame(() => done(undefined)));
  const layer = (/** @type {number} */ id, /** @type {string} */ name) => ({id, name, opacity: 255, blend: 'normal', hidden: false, locked: false, alphaLock: false});
  const press = (/** @type {string} */ k, /** @type {KeyboardEventInit} */ extra = {}) =>
    $('[data-raster-stage]').dispatchEvent(new win.KeyboardEvent('keydown', {key: k, bubbles: true, cancelable: true, ...extra}));

  // The stand-in core: a canvas of two layers, blue under a yellow box.
  const core = await createRasterRuntime();
  const generation = 3;
  core.open({width: 96, height: 80, preset: 'pixel', palette: [], layers: [layer(1, 'Sprite'), layer(2, 'Ink')], generation});
  core.apply({op: 'fillRect', layer: 1, rect: [0, 0, 96, 80], color: [20, 40, 200, 255]});
  core.apply({op: 'fillRect', layer: 2, rect: [10, 10, 30, 20], color: [250, 200, 0, 255]});
  let version = 2;
  win.rasterRead = (/** @type {number} */ g, /** @type {number} */ l, /** @type {number[]} */ tiles) => (g === generation ? tilesPayload(core, g, l, tiles, version) : null);
  const viewOf = (/** @type {Record<string, any>} */ extra = {}) => {
    const info = core.info();
    return {type: 'rasterCanvas', canvas: {path: 'hero.canvas', name: 'hero', preset: 'pixel', width: 96, height: 80, palette: info.palette, layers: info.layers,
      tiles: info.tiles, generation, version, dirty: false, canUndo: false, canRedo: false, undo: null, redo: null, saving: false, ...extra}};
  };
  const home = {type: 'rasterHome', canvases: [{path: 'hero.canvas', name: 'hero', preset: 'pixel', size: [96, 80]}]};

  await test('Canvas is the twelfth mode, on ⌃=, in the newer modes\' chrome, and its home page lists the canvases here and makes another', async () => {
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    let before = win.commands.length;
    doc.body.dispatchEvent(new win.KeyboardEvent('keydown', {key: '=', code: 'Equal', ctrlKey: true, bubbles: true, cancelable: true}));
    await settle();
    assert(sent(before, 'setMode').some((/** @type {any} */ c) => c.mode === 'raster'), '⌃= did not ask for Canvas');
    await win.dispatch({type: 'setMode', mode: 'raster'}); await settle();
    const tab = $('.tb-modes [data-mode="raster"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃=') && tab.textContent === 'Canvas', 'the title bar does not show Canvas on ⌃=');
    assert(ws()?.classList.contains('on') && shown($('.titlebar')) && !shown($('.statusline')), 'Canvas does not wear the newer modes\' chrome');
    await win.pushEvents([home, {type: 'rasterCanvas', canvas: null}]); await settle();
    assert(ws().classList.contains('raster-home') && !shown($('.raster-side')), 'with no canvas open, the home page is not the whole page');
    const cards = [...ws().querySelectorAll('[data-raster-open-path]')];
    assert(cards.length === 1 && cards[0].textContent.includes('Pixel · 96 × 80'), `wrong canvases: ${cards.map((c) => c.textContent)}`);
    before = win.commands.length;
    cards[0].click(); await settle();
    assert(sent(before, 'rasterOpen')[0]?.path === 'hero.canvas', 'a canvas was not opened from its card');
    /** @type {HTMLInputElement} */ ($('[data-raster-new-name]')).value = 'Sky';
    const paint = /** @type {HTMLInputElement} */ (ws().querySelector('[name="preset"][value="paint"]'));
    paint.checked = true;
    paint.dispatchEvent(new win.Event('change', {bubbles: true}));
    assert($('[data-raster-new-size]').value === '2048x2048', `Paint does not offer its sizes: ${$('[data-raster-new-size]').value}`);
    before = win.commands.length;
    $('[data-raster-new] button[type="submit"]').click(); await settle();
    const made = sent(before, 'rasterNew')[0];
    assert(made?.name === 'Sky' && made.preset === 'paint' && made.width === 2048 && made.height === 2048, `New canvas sent ${JSON.stringify(made)}`);
  });

  await test('A canvas opens into the window\'s own copy, its tiles fetched and laid together, its layers top first', async () => {
    const before = reads();
    await win.pushEvents([home, viewOf()]); await settle();
    await probe().settled();
    assert(reads() - before === 2, `${reads() - before} layers fetched, not 2`);
    assert(JSON.stringify(probe().pick(15, 15)) === '[250,200,0,255]', `the yellow box is ${probe().pick(15, 15)}`);
    assert(JSON.stringify(probe().pick(60, 60)) === '[20,40,200,255]', `the ground is ${probe().pick(60, 60)}`);
    const rows = [...ws().querySelectorAll('[data-raster-layer]')].map((r) => r.querySelector('.raster-layer-name').textContent);
    assert(rows.join() === 'Ink,Sprite', `the layers are not top first: ${rows}`);
    assert(!ws().classList.contains('raster-home') && shown($('.raster-side')) && shown($('[data-raster-gl]')), 'the canvas is not laid out');
  });

  await test('An edit is made here first and kept when the core\'s hash agrees, and fetched again when it does not', async () => {
    let before = win.commands.length;
    const fetched = reads();
    ws().querySelector('[data-raster-eye="2"]').click();
    await frame();
    // Shown at once, before the core has said anything.
    assert(JSON.stringify(probe().pick(15, 15)) === '[20,40,200,255]', 'hiding the layer did not show at once');
    const edit = sent(before, 'rasterEdit')[0];
    assert(edit?.generation === generation && edit.seq === 1 && edit.op.op === 'setLayer' && edit.op.layer === 2 && edit.op.hidden === true, `the edit sent was ${JSON.stringify(edit)}`);
    // The core makes it too, and its hash agrees: nothing is fetched.
    const {hash} = core.apply(edit.op);
    version += 1;
    await win.pushEvents([{type: 'rasterApplied', generation, seq: 1, version, hash, refused: null, dirty: true, canUndo: true, canRedo: false}, viewOf({dirty: true, canUndo: true, undo: 'hide layer'})]);
    await settle(); await probe().settled();
    assert(reads() === fetched, 'a canvas the core agreed with was fetched again');
    assert(shown($('[data-raster-dirty]')) && !$('[data-raster-undo]').disabled, 'the canvas is not shown as changed');
    // An answer that does not agree: the copy is made again from the core's.
    before = win.commands.length;
    ws().querySelector('[data-raster-eye="2"]').click();
    await frame();
    assert(JSON.stringify(probe().pick(15, 15)) === '[250,200,0,255]', 'showing the layer did not show at once');
    await win.pushEvents([{type: 'rasterApplied', generation, seq: 2, version: version + 1, hash: '0000000000000000', refused: null, dirty: true, canUndo: true, canRedo: false}]);
    await settle(); await probe().settled();
    assert(reads() > fetched, 'a canvas the core disagreed with was not fetched again');
    assert(JSON.stringify(probe().pick(15, 15)) === '[20,40,200,255]', 'the copy is not the core\'s again');
  });

  await test('Tiles the core changed itself are fetched, a layer\'s opacity is seen while dragged and sent once let go, and ⌘S saves', async () => {
    core.apply({op: 'fillRect', layer: 1, rect: [50, 50, 4, 4], color: [1, 2, 3, 255]});
    version += 1;
    await win.pushEvents([{type: 'rasterChanged', generation, version, tiles: [[1, [0]]], meta: false}]);
    await settle(); await probe().settled();
    assert(JSON.stringify(probe().pick(51, 51)) === '[1,2,3,255]', `the core's change is not here: ${probe().pick(51, 51)}`);
    // The ground layer, chosen; its opacity dragged to nothing.
    ws().querySelector('[data-raster-layer="1"] .raster-layer-name').click(); await settle();
    const range = /** @type {HTMLInputElement} */ ($('[data-raster-field="opacity"]'));
    let before = win.commands.length;
    range.value = '0';
    range.dispatchEvent(new win.Event('input', {bubbles: true}));
    await frame();
    assert(probe().pick(60, 60)[3] === 0, 'the opacity dragged is not seen');
    assert(!sent(before, 'rasterEdit').length, 'an edit was sent while dragging');
    range.dispatchEvent(new win.Event('change', {bubbles: true}));
    await settle();
    const edit = sent(before, 'rasterEdit')[0];
    assert(edit?.op.op === 'setLayer' && edit.op.layer === 1 && edit.op.opacity === 0, `letting go sent ${JSON.stringify(edit)}`);
    before = win.commands.length;
    press('s', {metaKey: true}); await settle();
    assert(sent(before, 'rasterSave').length === 1, '⌘S did not save');
    press('z', {metaKey: true}); await settle();
    assert(sent(before, 'rasterUndo').length === 1, '⌘Z did not undo');
    press('1'); await settle();
    assert($('[data-raster-zoom]').textContent === '100 %', `1 did not show it at 100 %: ${$('[data-raster-zoom]').textContent}`);
    win.rasterRead = undefined;
    await win.pushEvents([{type: 'rasterCanvas', canvas: null}]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
