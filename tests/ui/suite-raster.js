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
  /** The last edit the stand-in core made, as the real one numbers what it reads. */
  let lastSeq = 0;
  win.rasterRead = (/** @type {number} */ g, /** @type {number} */ l, /** @type {number[]} */ tiles) => (g === generation ? tilesPayload(core, g, l, tiles, version, lastSeq) : null);
  const viewOf = (/** @type {Record<string, any>} */ extra = {}) => {
    const info = core.info();
    return {type: 'rasterCanvas', canvas: {path: 'hero.canvas', name: 'hero', preset: 'pixel', width: info.width, height: info.height, palette: info.palette, layers: info.layers,
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
  });

  /** A pointer down, through, and up over canvas pixels. @param {number[][]} pixels @param {KeyboardEventInit & {shiftKey?: boolean, up?: boolean}} [extra] */
  const drag = async (pixels, extra = {}) => {
    const stage = $('[data-raster-stage]');
    const fire = (/** @type {string} */ type, /** @type {number[]} */ p) => {
      const [x, y] = probe().at(p[0], p[1]);
      stage.dispatchEvent(new win.PointerEvent(type, {clientX: x, clientY: y, pointerId: 7, pointerType: 'mouse', button: 0, buttons: 1, bubbles: true, cancelable: true, ...extra}));
    };
    fire('pointerdown', pixels[0]);
    for (const p of pixels.slice(1)) {
      fire('pointermove', p);
      await frame();
    }
    if (extra.up !== false) fire('pointerup', pixels[pixels.length - 1]);
    await frame();
  };
  /** The core's answer to the last edit sent: the stand-in core makes it too. */
  const answer = async (/** @type {number} */ from, /** @type {string} */ [wrong] = []) => {
    // Commands go out a moment after the key or the click that sends them.
    await settle();
    const edit = sent(from, 'rasterEdit').at(-1);
    if (!edit) throw new Error(`no edit was sent, only ${JSON.stringify(win.commands.slice(from).map((/** @type {any} */ c) => c.type))}`);
    const made = core.apply(edit.op);
    version += 1;
    lastSeq = edit.seq;
    await win.pushEvents([{type: 'rasterApplied', generation, seq: edit.seq, version, hash: wrong ?? made.hash, refused: made.refused ?? null, dirty: true, canUndo: true, canRedo: false}]);
    await settle(); await probe().settled();
    return edit;
  };

  await test('A pencil stroke shows as it is drawn, goes to the core as the points drawn, and is fetched again only where the core disagrees', async () => {
    await win.pushEvents([viewOf()]); await settle(); await probe().settled();
    // The ground layer, and the pencil in a colour of its own.
    ws().querySelector('[data-raster-layer="1"] .raster-layer-name').click(); await settle();
    press('b');
    const input = /** @type {HTMLInputElement} */ ($('[data-raster-color]'));
    input.value = '#ff0080';
    input.dispatchEvent(new win.Event('input', {bubbles: true}));
    let before = win.commands.length;
    const fetched = reads();
    await drag([[60, 60], [64, 60], [68, 62], [70, 66]], {up: false});
    // Shown while the pen is still down, and nothing sent yet.
    assert(JSON.stringify(probe().pick(64, 60)) === '[255,0,128,255]', `the stroke is not shown as drawn: ${probe().pick(64, 60)}`);
    assert(!sent(before, 'rasterEdit').length, 'a stroke was sent before it was finished');
    const [x, y] = probe().at(70, 66);
    $('[data-raster-stage]').dispatchEvent(new win.PointerEvent('pointerup', {clientX: x, clientY: y, pointerId: 7, pointerType: 'mouse', button: 0, bubbles: true}));
    await frame();
    let edit = await answer(before);
    assert(edit.op.op === 'stroke' && edit.op.tool === 'pencil' && edit.op.layer === 1 && edit.op.points.length === 12 && edit.op.color.join() === '255,0,128,255', `the stroke sent was ${JSON.stringify(edit.op).slice(0, 200)}`);
    assert(reads() === fetched, 'a stroke the core agreed with was fetched again');
    assert(JSON.stringify(probe().sync()) === '{"inflight":[],"stale":[]}', `still waiting: ${JSON.stringify(probe().sync())}`);
    // The core makes it differently: only the tile it touched is fetched again.
    before = win.commands.length;
    const invokes = (win.invokes ?? []).length;
    await drag([[5, 5], [8, 5]]);
    edit = await answer(before, ['0000000000000000']);
    const asked = (win.invokes ?? []).slice(invokes).filter((/** @type {any} */ c) => c.name === 'read_raster');
    assert(asked.length === 1 && JSON.stringify(asked[0].args.tiles) === '[0]' && asked[0].args.layer === 1, `fetched again: ${JSON.stringify(asked.map((/** @type {any} */ a) => a.args))}`);
    assert(JSON.stringify(probe().pick(6, 5)) === '[255,0,128,255]', 'the tile fetched is not the core\'s');
  });

  await test('⎋ takes back a stroke being drawn and sends nothing; a shape is shown while dragged and sent when let go', async () => {
    let before = win.commands.length;
    const under = JSON.stringify(probe().pick(30, 40));
    await drag([[30, 40], [36, 40]], {up: false});
    assert(JSON.stringify(probe().pick(33, 40)) === '[255,0,128,255]', 'the stroke was not drawn');
    press('Escape'); await frame();
    assert(JSON.stringify(probe().pick(33, 40)) === under, '⎋ did not take the stroke back');
    const [x, y] = probe().at(36, 40);
    $('[data-raster-stage]').dispatchEvent(new win.PointerEvent('pointerup', {clientX: x, clientY: y, pointerId: 7, pointerType: 'mouse', button: 0, bubbles: true}));
    await settle();
    assert(!sent(before, 'rasterEdit').length, 'a stroke taken back was sent');
    // A rectangle, dragged: shown as it goes, sent once.
    press('r');
    before = win.commands.length;
    await drag([[50, 10], [58, 16], [62, 20]], {up: false});
    assert(JSON.stringify(probe().pick(62, 15)) === '[255,0,128,255]', 'the rectangle is not shown while dragged');
    assert(!sent(before, 'rasterEdit').length, 'the shape was sent while dragged');
    const [ex, ey] = probe().at(62, 20);
    $('[data-raster-stage]').dispatchEvent(new win.PointerEvent('pointerup', {clientX: ex, clientY: ey, pointerId: 7, pointerType: 'mouse', button: 0, bubbles: true}));
    await frame();
    const edit = await answer(before);
    assert(edit.op.op === 'shape' && edit.op.shape === 'rect' && edit.op.from.join() === '50,10' && edit.op.to.join() === '62,20', `the shape sent was ${JSON.stringify(edit.op)}`);
    assert(probe().sync().stale.length === 0, 'the shape the core agreed with is stale');
  });

  await test('A fill, a colour picked, the palette and the grid', async () => {
    // The ink layer is hidden by now: the ground there is blue.
    press('i');
    await drag([[40, 40]]);
    assert($('[data-raster-color]').value === '#1428c8', `the colour picked is ${$('[data-raster-color]').value}`);
    // The pink stroke, filled blue.
    press('g');
    let before = win.commands.length;
    await drag([[64, 60]]);
    assert(JSON.stringify(probe().pick(64, 60)) === '[20,40,200,255]', `the fill is not shown at once: ${probe().pick(64, 60)}`);
    const edit = await answer(before);
    assert(edit.op.op === 'fill' && edit.op.x === 64 && edit.op.y === 60 && edit.op.contiguous === true && edit.op.color.join() === '20,40,200,255', `the fill sent was ${JSON.stringify(edit.op)}`);
    assert(probe().sync().stale.length === 0, 'a fill the core agreed with is stale');
    $('[data-raster-side="palette"]').click(); await settle();
    await win.pushEvents([viewOf({palette: [[26, 28, 44, 255], [240, 240, 247, 255]]})]); await settle();
    const swatches = ws().querySelectorAll('[data-raster-swatch]');
    assert(swatches.length === 2, `the palette shows ${swatches.length} swatches`);
    swatches[1].click(); await settle();
    assert($('[data-raster-color]').value === '#f0f0f7', 'a swatch did not become the colour drawn with');
    assert(ws().querySelector('[data-raster-swatch-remove]'), 'a colour in the palette cannot be removed');
    before = win.commands.length;
    $('[data-raster-swatch-remove]').click(); await settle();
    const removed = sent(before, 'rasterEdit')[0];
    assert(removed?.op.op === 'setPalette' && removed.op.colors.length === 1, `removing the swatch sent ${JSON.stringify(removed?.op)}`);
    $('[data-raster-side="layers"]').click(); await settle();
    press('G'); await settle();
    assert($('[data-raster-grid]').getAttribute('aria-pressed') === 'true', 'G did not show the grid');
    press('G'); await settle();
  });

  /** A stroke drawn as a tablet's pen draws it in the app: the shell tells of
   *  the pressure (`__protonPen`) just before each of WebKit's mouse events.
   *  @param {number[][]} points x, y, pressure 0 to 1 @param {string} [pointerType] */
  const penDrag = async (points, pointerType = 'mouse') => {
    const stage = $('[data-raster-stage]');
    const fire = (/** @type {string} */ type, /** @type {number[]} */ p) => {
      const [x, y] = probe().at(p[0], p[1]);
      if (pointerType === 'mouse' && type !== 'pointerup') win.__protonPen('d', p[2]);
      stage.dispatchEvent(new win.PointerEvent(type, {clientX: x, clientY: y, pointerId: 7, pointerType, pressure: p[2], button: 0, buttons: 1, bubbles: true, cancelable: true}));
    };
    fire('pointerdown', points[0]);
    for (const p of points.slice(1)) {
      fire('pointermove', p);
      await frame();
    }
    if (pointerType === 'mouse') win.__protonPen('u', 0);
    fire('pointerup', points[points.length - 1]);
    await frame();
  };

  await test('A brush takes its size from the pen\'s pressure — the shell\'s word of a tablet, or a browser\'s pen — a mouse presses fully, and the eraser end erases', async () => {
    win.localStorage.removeItem('proton.canvas.brushes');
    // n is the brush on a pixel canvas; the Ink brush's size is nearly all pressure.
    press('n'); await settle();
    assert($('[data-raster-tool="brush"]').getAttribute('aria-pressed') === 'true' && ws().querySelector('.raster-tool-head').textContent === 'Brush', 'n did not take the brush');
    const preset = /** @type {HTMLSelectElement} */ (ws().querySelector('[data-raster-option="brush.preset"]'));
    preset.value = 'ink';
    preset.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(/** @type {HTMLInputElement} */ (ws().querySelector('[data-raster-option="brush.size"]')).value === '6', 'the Ink brush was not chosen');
    assert(JSON.parse(win.localStorage.getItem('proton.canvas.brushes') ?? '{}').brush?.size === 6, 'the brush chosen was not kept');
    const input = /** @type {HTMLInputElement} */ ($('[data-raster-color]'));
    input.value = '#00ff00';
    input.dispatchEvent(new win.Event('input', {bubbles: true}));
    // Light at the left, hard at the right.
    let before = win.commands.length;
    const fetched = reads();
    await penDrag(Array.from({length: 13}, (_, i) => [16 + i * 2, 50, 0.08 + i * 0.075]));
    let edit = await answer(before);
    const pressures = edit.op.points.filter((/** @type {number} */ _, /** @type {number} */ i) => i % 3 === 2);
    assert(edit.op.op === 'brush' && !edit.op.erase && edit.op.brush.size === 6 && pressures[0] < 8000 && pressures.at(-1) > 60000, `the brush stroke sent was ${JSON.stringify(edit.op).slice(0, 240)}`);
    const green = (/** @type {number} */ x, /** @type {number} */ y) => {
      const c = probe().pick(x, y);
      return c[1] > 120 && c[2] < 150;
    };
    const across = (/** @type {number} */ x) => Array.from({length: 20}, (_, i) => 40 + i).filter((y) => green(x, y)).length;
    assert(across(18) <= 2 && across(40) >= 4, `pressure did not size the brush: ${across(18)} px at the light end, ${across(40)} at the hard end`);
    assert(reads() === fetched && JSON.stringify(probe().sync()) === '{"inflight":[],"stale":[]}', `a brush stroke the core agreed with was fetched again: ${JSON.stringify(probe().sync())}`);
    // A mouse, the shell saying nothing: full pressure.
    before = win.commands.length;
    await drag([[16, 70], [30, 70]]);
    edit = await answer(before);
    assert(edit.op.op === 'brush' && edit.op.points.every((/** @type {number} */ v, /** @type {number} */ i) => i % 3 !== 2 || v === 65535), 'a mouse did not press fully');
    // A browser that knows the pen gives its pressure on the event.
    before = win.commands.length;
    await penDrag([[60, 70, 0.5], [70, 70, 0.5]], 'pen');
    edit = await answer(before);
    assert(edit.op.points[2] === 32768, `a browser pen's pressure was ${edit.op.points[2]}`);
    // The eraser end near the tablet erases, whatever the tool — softly, as
    // a brush, while the brush is the tool; a pixel at a time with the pencil.
    win.__protonPen('e', 0);
    before = win.commands.length;
    await penDrag([[20, 70, 1], [26, 70, 1]]);
    edit = await answer(before);
    assert(edit.op.op === 'brush' && edit.op.erase === true, `the eraser end sent ${JSON.stringify(edit.op).slice(0, 120)}`);
    assert(probe().pick(22, 70)[3] < 255, 'the eraser end did not erase');
    press('b');
    before = win.commands.length;
    await penDrag([[40, 76, 1], [44, 76, 1]]);
    edit = await answer(before);
    assert(edit.op.op === 'stroke' && edit.op.tool === 'eraser', `the eraser end with the pencil sent ${JSON.stringify(edit.op).slice(0, 120)}`);
    win.__protonPen('a', 0);
  });

  /** The core's answer to the last edit, with the canvas it now is — for an
   *  edit to the layers or the canvas's size. @param {number} from */
  const answerWithCanvas = async (from) => {
    await settle();
    const edit = sent(from, 'rasterEdit').at(-1);
    const made = core.apply(edit.op);
    version += 1;
    lastSeq = edit.seq;
    await win.pushEvents([{type: 'rasterApplied', generation, seq: edit.seq, version, hash: made.hash ?? '', refused: made.refused ?? null, dirty: true, canUndo: true, canRedo: false}, viewOf({dirty: true, canUndo: true})]);
    await settle(); await probe().settled(); await settle(); await probe().settled();
    return edit;
  };

  await test('A selection holds edits to it, ⇧ adds to it, ⌘D and ⌘A change it, and its ants go round it', async () => {
    const stage = $('[data-raster-stage]');
    press('s'); await settle();
    assert($('[data-raster-tool="select"]').getAttribute('aria-pressed') === 'true', 's did not take the select tool');
    let before = win.commands.length;
    await drag([[10, 10], [20, 14], [29, 19]]);
    let edit = await answer(before);
    assert(edit.op.op === 'select' && edit.op.area.kind === 'rect' && edit.op.area.rect.join() === '10,10,20,10' && edit.op.mode === 'replace', `the selection sent was ${JSON.stringify(edit.op)}`);
    assert(JSON.stringify(probe().sync()) === '{"inflight":[],"stale":[]}', `the selection made here is not the core's: ${JSON.stringify(probe().sync())}`);
    assert(probe().ants() === 4, `the ants go along ${probe().ants()} edges, not a rectangle's four`);
    // The pencil across it changes only what is selected.
    press('b');
    const input = /** @type {HTMLInputElement} */ ($('[data-raster-color]'));
    input.value = '#ff00ff';
    input.dispatchEvent(new win.Event('input', {bubbles: true}));
    const outside = JSON.stringify(probe().pick(5, 15));
    before = win.commands.length;
    await drag([[2, 15], [40, 15]]);
    await answer(before);
    assert(JSON.stringify(probe().pick(15, 15)) === '[255,0,255,255]' && JSON.stringify(probe().pick(5, 15)) === outside, `the stroke did not keep to the selection: ${probe().pick(15, 15)} ${probe().pick(5, 15)}`);
    assert(probe().sync().stale.length === 0, 'a stroke held to the selection was not the core\'s');
    // ⇧ adds an ellipse: s takes the select tool, and again its next shape.
    press('s'); press('s'); await settle();
    before = win.commands.length;
    await drag([[50, 40], [60, 50]], {shiftKey: true});
    edit = await answer(before);
    assert(edit.op.area.kind === 'ellipse' && edit.op.mode === 'add', `⇧ with the second shape sent ${JSON.stringify(edit.op)}`);
    before = win.commands.length;
    press('d', {metaKey: true});
    edit = await answer(before);
    assert(edit.op.area.kind === 'nothing' && probe().ants() === 0, '⌘D did not select nothing');
    before = win.commands.length;
    press('a', {metaKey: true});
    edit = await answer(before);
    assert(edit.op.area.kind === 'all' && probe().ants() === 4, `⌘A did not select all: ${probe().ants()} edges`);
    before = win.commands.length;
    press('d', {metaKey: true});
    await answer(before);
    stage.focus();
  });

  await test('The move tool moves what is selected, ⌘T keeps a move on ↩, ⌘C ⌘V paste a layer, ⌫ clears, and the canvas turns', async () => {
    // Nothing selected, and a red square to move.
    if (probe().ants()) {
      const from = win.commands.length;
      press('d', {metaKey: true});
      await answer(from);
    }
    core.apply({op: 'fillRect', layer: 1, rect: [50, 50, 10, 10], color: [255, 0, 0, 255]});
    version += 1;
    await win.pushEvents([{type: 'rasterChanged', generation, version, tiles: [[1, [0]]], meta: false}]); await settle(); await probe().settled();
    press('s'); await settle();
    const shape = /** @type {HTMLSelectElement} */ (ws().querySelector('[data-raster-option="select.shape"]'));
    shape.value = 'rect';
    shape.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    let before = win.commands.length;
    await drag([[50, 50], [59, 59]]);
    await answer(before);
    press('v'); await settle();
    before = win.commands.length;
    await drag([[55, 55], [60, 57], [65, 60]], {up: false});
    assert(probe().moving(), 'the move tool did not lift what is selected');
    assert(JSON.stringify(probe().pick(55, 55)) !== '[255,0,0,255]', 'what was lifted is still shown where it was');
    const [ux, uy] = probe().at(65, 60);
    $('[data-raster-stage]').dispatchEvent(new win.PointerEvent('pointerup', {clientX: ux, clientY: uy, pointerId: 7, pointerType: 'mouse', button: 0, bubbles: true}));
    await frame();
    let edit = await answer(before);
    assert(edit.op.op === 'transform' && edit.op.bounds.join() === '60,55,10,10' && edit.op.inverse.join() === `65536,0,${-10 * 65536},0,65536,${-5 * 65536}` && edit.op.smooth === false,
      `the move sent was ${JSON.stringify(edit.op)}`);
    assert(JSON.stringify(probe().pick(62, 57)) === '[255,0,0,255]' && !probe().moving(), `the square did not move: ${probe().pick(62, 57)}`);
    assert(JSON.stringify(probe().sync()) === '{"inflight":[],"stale":[]}', `the move made here is not the core's: ${JSON.stringify(probe().sync())}`);
    // ⌘T: moved inside, kept on ↩.
    press('t', {metaKey: true}); await settle();
    assert(probe().moving() && ws().querySelector('[data-raster-transform="keep"]'), '⌘T did not begin a transform');
    before = win.commands.length;
    await drag([[65, 60], [67, 60]]);
    assert(!sent(before, 'rasterEdit').length, 'a ⌘T move was sent before ↩');
    press('Enter');
    edit = await answer(before);
    assert(edit.op.op === 'transform' && edit.op.bounds.join() === '62,55,10,10', `↩ kept ${JSON.stringify(edit.op)}`);
    // Copied and pasted as a layer over the one chosen; then cleared.
    const layers = ws().querySelectorAll('[data-raster-layer]').length;
    before = win.commands.length;
    press('c', {metaKey: true});
    edit = await answer(before);
    assert(edit.op.op === 'copy' && edit.op.cut === false, `⌘C sent ${JSON.stringify(edit.op)}`);
    before = win.commands.length;
    press('v', {metaKey: true});
    edit = await answerWithCanvas(before);
    assert(edit.op.op === 'paste' && ws().querySelectorAll('[data-raster-layer]').length === layers + 1, `⌘V sent ${JSON.stringify(edit.op)}, and the layers are ${ws().querySelectorAll('[data-raster-layer]').length}`);
    assert(JSON.stringify(probe().sync()) === '{"inflight":[],"stale":[]}', `the paste made here is not the core's: ${JSON.stringify(probe().sync())}`);
    before = win.commands.length;
    press('Backspace');
    edit = await answer(before);
    assert(edit.op.op === 'clear', `⌫ sent ${JSON.stringify(edit.op)}`);
    // The canvas turned: made by the core, the copy loaded again at its new size.
    const fetched = reads();
    before = win.commands.length;
    ws().querySelector('[data-raster-canvas="cw"]').click(); await settle();
    edit = await answerWithCanvas(before);
    assert(edit.op.op === 'orient' && edit.op.turns === 1, `the turn sent was ${JSON.stringify(edit.op)}`);
    assert($('[data-raster-size]').textContent.includes('80 × 96') && reads() > fetched, `the turned canvas was not loaded again: ${$('[data-raster-size]').textContent}, ${reads() - fetched} reads`);
    assert(JSON.stringify(probe().pick(79, 0)) === JSON.stringify(core.pick(79, 0)), `the copy is not the core's turned canvas: ${probe().pick(79, 0)} against ${core.pick(79, 0)}`);
  });

  await test('The Make panel shows a sprite before it is kept, keeps a filter, makes a ramp, and the canvas grows by Scale2x', async () => {
    if (probe().ants()) {
      const from = win.commands.length;
      press('d', {metaKey: true});
      await answer(from);
    }
    $('[data-raster-panel="make"]').click(); await settle();
    const box = ws().querySelector('[data-raster-make]');
    assert(shown(box) && box.querySelector('[data-make="generator"]'), 'the Make panel is not shown');
    const set = async (/** @type {string} */ name, /** @type {string} */ value) => {
      const field = /** @type {HTMLInputElement} */ (box.querySelector(`[data-make="${name}"]`));
      field.value = value;
      field.dispatchEvent(new win.Event('change', {bubbles: true}));
      await settle();
    };
    // The sprite lands in the middle of the 80 × 96 canvas.
    const middle = JSON.stringify(probe().pick(40, 48));
    let before = win.commands.length;
    await set('scale', '3');
    await set('seed', '5');
    assert(!sent(before, 'rasterEdit').length, 'a sprite shown was sent before it was kept');
    assert(box.querySelector('[data-make-note]').textContent.includes('Showing the sprite'), `the panel does not say what it shows: ${box.querySelector('[data-make-note]').textContent}`);
    const shownThere = JSON.stringify(probe().pick(40, 48));
    box.querySelector('[data-make-action="keep-generator"]').click();
    let edit = await answer(before);
    assert(edit.op.op === 'generate' && edit.op.generator.kind === 'sprite' && edit.op.generator.seed === 5 && edit.op.generator.scale === 3 && edit.op.generator.at.join() === '19,27',
      `the sprite kept was ${JSON.stringify(edit.op)}`);
    assert(JSON.stringify(probe().pick(40, 48)) === shownThere && shownThere !== middle, `what was kept is not what was shown: ${probe().pick(40, 48)} against ${shownThere}, ${middle} before`);
    assert(JSON.stringify(probe().sync()) === '{"inflight":[],"stale":[]}', `the sprite made here is not the core's: ${JSON.stringify(probe().sync())}`);
    // Leaving the panel takes away what it shows.
    await set('seed', '6');
    $('[data-raster-panel="inspector"]').click(); await settle();
    assert(JSON.stringify(probe().pick(40, 48)) === shownThere, 'a sprite shown stayed when the panel was left');
    $('[data-raster-panel="make"]').click(); await settle();
    // A filter: an outline round what is there.
    before = win.commands.length;
    await set('filter', 'outline');
    ws().querySelector('[data-make-action="keep-filter"]').click();
    edit = await answer(before);
    assert(edit.op.op === 'filter' && edit.op.filter.kind === 'outline', `the filter kept was ${JSON.stringify(edit.op)}`);
    assert(probe().sync().stale.length === 0, 'the outline made here is not the core\'s');
    // A ramp of the colour drawn with, added to the palette.
    $('[data-raster-side="palette"]').click(); await settle();
    const swatches = view => view.canvas.palette.length;
    const had = swatches(viewOf());
    before = win.commands.length;
    ws().querySelector('[data-raster-ramp]').click();
    edit = await answerWithCanvas(before);
    assert(edit.op.op === 'setPalette' && edit.op.colors.length > had && edit.op.colors.length <= had + 5, `the ramp sent ${JSON.stringify(edit.op).slice(0, 200)}`);
    $('[data-raster-side="layers"]').click(); await settle();
    // ×2: made by the core, the copy loaded again at twice the size.
    $('[data-raster-panel="inspector"]').click(); await settle();
    before = win.commands.length;
    ws().querySelector('[data-raster-canvas="up2"]').click(); await settle();
    edit = await answerWithCanvas(before);
    assert(edit.op.op === 'upscale' && edit.op.factor === 2 && $('[data-raster-size]').textContent.includes('160 × 192'), `×2 sent ${JSON.stringify(edit.op)}; the canvas is ${$('[data-raster-size]').textContent}`);
    assert(JSON.stringify(probe().pick(80, 96)) === JSON.stringify(core.pick(80, 96)), 'the copy is not the core\'s grown canvas');
  });

  await test('Pictures come in from the project, a drop and the clipboard, and a canvas goes out as a PNG, a PSD or to a game', async () => {
    const invoked = (/** @type {string} */ name, /** @type {number} */ from) => (win.invokes ?? []).slice(from).filter((/** @type {any} */ c) => c.name === name);
    // The home page's pictures, and the games to send to.
    await win.pushEvents([{...home, images: ['art/leaf.png', 'art/scene.psd'], games: ['crossroads']}]); await settle();
    const pictures = [...ws().querySelectorAll('[data-raster-import-path]')];
    assert(pictures.length === 2, `the home page offers ${pictures.length} pictures`);
    $('[data-raster-panel="inspector"]').click(); await settle();
    // Written out: a PNG four times the size, a PSD; sent to a game; one brought in.
    const scale = /** @type {HTMLSelectElement} */ (ws().querySelector('[data-raster-export-scale]'));
    scale.value = '4';
    scale.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    let before = win.commands.length;
    ws().querySelector('[data-raster-export="png"]').click();
    ws().querySelector('[data-raster-export="psd"]').click();
    ws().querySelector('[data-raster-send]').click();
    ws().querySelector('[data-raster-bring-in]').click(); await settle();
    const out = sent(before, 'rasterExport');
    assert(out.length === 2 && out[0].format === 'png' && out[0].scale === 4 && out[1].format === 'psd', `exports sent ${JSON.stringify(out)}`);
    assert(sent(before, 'rasterSend')[0]?.game === 'crossroads' && sent(before, 'rasterSend')[0]?.layers === true, `sent ${JSON.stringify(sent(before, 'rasterSend'))}`);
    const brought = sent(before, 'rasterImport')[0];
    assert(brought?.path === 'art/leaf.png' && brought.layer === true, `brought in ${JSON.stringify(brought)}`);
    // A PNG dropped on the canvas: its bytes handed over raw, then brought in.
    const png = await (await win.fetch('/tests/ui/fixtures/game/art/player.png')).arrayBuffer();
    const file = new win.File([png], 'leaf.png', {type: 'image/png'});
    const drop = new win.DataTransfer();
    drop.items.add(file);
    before = win.commands.length;
    let invokes = (win.invokes ?? []).length;
    $('[data-raster-stage]').dispatchEvent(new win.DragEvent('drop', {dataTransfer: drop, bubbles: true, cancelable: true}));
    for (let i = 0; i < 20 && !sent(before, 'rasterImport').length; i++) await settle();
    const handed = invoked('put_raster_blob', invokes)[0];
    assert(handed && handed.args instanceof win.Uint8Array && handed.args.byteLength === png.byteLength, `the drop was handed over as ${handed && Object.prototype.toString.call(handed.args)}`);
    const dropped = sent(before, 'rasterImport')[0];
    assert(dropped?.blob === win.rasterBlobs.length && dropped.layer === true && dropped.name === 'leaf.png', `the drop brought in ${JSON.stringify(dropped)}`);
    // ⌘C: the core's copy, then its pixels to the system's clipboard.
    before = win.commands.length;
    invokes = (win.invokes ?? []).length;
    press('c', {metaKey: true});
    await answer(before);
    assert(invoked('copy_raster_image', invokes).length === 1, 'the copy was not put on the clipboard');
    // ⌘V with another app's picture there: brought in, not Canvas's copy pasted.
    win.rasterPaste = () => 9;
    before = win.commands.length;
    press('v', {metaKey: true}); await settle(); await settle();
    assert(sent(before, 'rasterImport')[0]?.blob === 9 && !sent(before, 'rasterEdit').length, `⌘V with a picture sent ${JSON.stringify(win.commands.slice(before).map((/** @type {any} */ c) => c.type))}`);
    win.rasterPaste = undefined;
    win.rasterRead = undefined;
    await win.pushEvents([{type: 'rasterCanvas', canvas: null}]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
