/* CAD's checks, run by suite.js in the same fixture window: its place on
 * the rail, the home page, a part's steps and settings, the sketch drawn
 * and held with the mouse, dimensions typed in place, a drag, ⌘S and ⌘Z,
 * and the 3D view. The fixture transport answers none of these; the part
 * pushed is the core's own, dumped (ui/preview-cad-part.js), and what is
 * checked is what the window sends back. */

import {PART} from '../../ui/preview-cad-part.js';
import {ASSEMBLY} from '../../ui/preview-cad-assembly.js';
import {DRAWING} from '../../ui/preview-cad-drawing.js';
import {SCHEMATIC} from '../../ui/preview-cad-schematic.js';

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, key: Function, shown: (el: any) => boolean}} t */
export async function cadChecks({test, assert, win, doc, $, settle, key, shown}) {
  const ws = () => $('.workspace[data-ws="cad"]');
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const ops = (/** @type {number} */ from) => sent(from, 'cadEdit').flatMap((/** @type {any} */ c) => c.ops);
  const debug = () => win.cadDebug();
  /** The sample part, as the core sends it, with what a check changes. @param {Partial<any>} more */
  const part = (more = {}) => ({type: 'cadPart', part: {...structuredClone(PART), generation: 5, dirty: false, ...more}});
  const svg = () => ws().querySelector('[data-cad-sketch]');
  /** A point of the sketch on the page. @param {[number, number]} p */
  const page = (p) => {
    const r = svg().getBoundingClientRect();
    const [x, y] = debug().sketcher.screen(p);
    return {clientX: r.left + x, clientY: r.top + y};
  };
  /** A click on the sketch at a point of it. @param {[number, number]} p @param {Partial<any>} [more] */
  const click = async (p, more = {}) => {
    const at = {...page(p), bubbles: true, pointerId: 7, button: 0, detail: 1, ...more};
    svg().dispatchEvent(new win.PointerEvent('pointerdown', at));
    svg().dispatchEvent(new win.PointerEvent('pointerup', at));
    await settle();
  };
  const sketchKey = (/** @type {string} */ k, /** @type {Partial<KeyboardEventInit>} */ more = {}) =>
    svg().dispatchEvent(new win.KeyboardEvent('keydown', {key: k, bubbles: true, cancelable: true, ...more}));
  const open = async (/** @type {number} */ index) => {
    ws().querySelector(`.cd-history [data-cad-step="${index}"]`).dispatchEvent(new win.MouseEvent('dblclick', {bubbles: true}));
    await settle();
  };

  await test('CAD sits after Logic: ⌃7 opens it, in Codex\'s chrome and on an opaque ground, and is the last', async () => {
    const asked = () => (win.invokes ?? []).filter((/** @type {any} */ c) => c.name === 'set_opaque').map((/** @type {any} */ c) => c.args.color);
    const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    const before = win.commands.length, opaque = asked().length;
    /** @type {HTMLElement} */ (doc.body).focus();
    key('7', {ctrlKey: true}); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'cad'), '⌃7 did not ask for CAD');
    assert($('.workspace.on')?.dataset.ws === 'cad', 'CAD is not on screen');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'CAD kept the old chrome');
    const tab = $('.titlebar [data-mode="cad"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃7') && tab.textContent.trim() === 'CAD', 'the title bar does not show CAD');
    assert($('.mode-rail [data-mode="cad"]')?.textContent.trim() === 'CAD', 'the rail does not say CAD');
    assert(!asked().slice(opaque).includes(null) && asked().at(-1) === ground, `CAD is not on the ground: ${JSON.stringify(asked().slice(opaque))}`);
    const before8 = win.commands.length;
    key('8', {ctrlKey: true}); await settle();
    assert(!sent(before8, 'setMode').length, '⌃8 asked for a mode');
  });

  await test('With no part open, CAD lists the folder\'s parts, opens one and makes another', async () => {
    await win.pushEvents([{type: 'cadHome', parts: [{path: 'case/foot.part', name: 'foot'}, {path: 'pump-bracket.part', name: 'pump-bracket'}]}, {type: 'cadPart', part: null}]);
    await settle();
    assert(shown(ws().querySelector('[data-cad-home]')) && !shown(ws().querySelector('[data-cad-part]')), 'the home page is not what is shown');
    const rows = [...ws().querySelectorAll('[data-cad-list] [data-cad-open]')];
    assert(rows.map((r) => r.dataset.cadOpen).join() === 'case/foot.part,pump-bracket.part', 'the parts are not listed');
    let before = win.commands.length;
    rows[1].click(); await settle();
    assert(sent(before, 'cadOpen')[0]?.path === 'pump-bracket.part', 'a row did not open its part');
    const form = ws().querySelector('[data-cad-form]');
    form.querySelector('input').value = 'Fan duct';
    before = win.commands.length;
    form.requestSubmit(); await settle();
    assert(sent(before, 'cadNew')[0]?.name === 'Fan duct', 'the form did not make a part');
  });

  await test('A part shows its parameters and steps, each mistake at its step; a step\'s settings write its line', async () => {
    const p = part();
    p.part.steps[3] = {...p.part.steps[3], error: 'no sketch called `post` is drawn above'};
    await win.pushEvents([p]); await settle();
    assert(shown(ws().querySelector('[data-cad-part]')) && !shown(ws().querySelector('[data-cad-home]')), 'the part is not shown');
    assert(ws().querySelector('[data-cad-title]').textContent === 'Pump bracket', 'the part is not named by its title');
    const steps = [...ws().querySelectorAll('.cd-history [data-cad-step]')].map((r) => r.dataset.cadStep);
    assert(steps.join() === 'part,7,8,10,11,12', `the steps are not listed in order: ${steps}`);
    assert(ws().querySelector('.cd-history [data-cad-step="11"]').classList.contains('bad') && ws().querySelector('.cd-history [data-cad-step="11"] .cd-bad')?.title.includes('post'), 'a step\'s mistake is not marked on it');
    // A round, built: its pick and radius, as written.
    assert(ws().querySelector('.cd-history [data-cad-step="12"] .cd-row-what')?.textContent === 'plate.edges(parallel z) · r 2', 'the round is not summed up');
    const later = part();
    later.part.steps[4] = {...later.part.steps[4], note: '`thread` is not built yet'};
    await win.pushEvents([later]); await settle();
    assert(ws().querySelector('.cd-history [data-cad-step="12"]').classList.contains('later'), 'a feature for later is not marked so');
    await win.pushEvents([p]); await settle();
    const names = [...ws().querySelectorAll('.cd-param .cd-pname')].map((x) => x.textContent);
    assert(names.join() === 't,hole', 'the parameters are not listed');
    // The extrude's settings.
    ws().querySelector('.cd-history [data-cad-step="8"]').click(); await settle();
    const distance = ws().querySelector('[data-cad-settings] [data-cad-f="distance"]');
    assert(distance?.value === 't', 'the extrude\'s distance is not shown as written');
    let before = win.commands.length;
    distance.value = '4';
    distance.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'replace', index: 8, text: 'extrude plate from base distance 4'}]), `the distance did not write the line: ${JSON.stringify(ops(before))}`);
    before = win.commands.length;
    const sym = ws().querySelector('[data-cad-settings] [data-cad-f="symmetric"]');
    sym.checked = true;
    sym.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    // (Nothing echoes the 4 back here, so the box still says it.)
    assert(ops(before)[0]?.text === 'extrude plate from base distance 4 symmetric', `half each way did not write the line: ${JSON.stringify(ops(before))}`);
    // A parameter changed where it is listed.
    before = win.commands.length;
    const t = ws().querySelector('[data-cad-param-value="t"]');
    t.value = '2.5';
    t.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'param', name: 't', value: '2.5'}]), 'a parameter did not change');
    // The part's own settings: its bodies, written out.
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    assert(ws().querySelector('[data-cad-settings]').textContent.includes('9.54 cm³'), 'the bodies\' volumes are not shown');
    before = win.commands.length;
    ws().querySelector('[data-cad-export="3mf"]').click(); await settle();
    assert(sent(before, 'cadExport')[0]?.format === '3mf' && sent(before, 'cadExport')[0]?.generation === 5, 'Write 3MF did not ask for one');
    before = win.commands.length;
    ws().querySelector('[data-cad-export="step"]').click(); await settle();
    assert(sent(before, 'cadExport')[0]?.format === 'step', 'Write STEP did not ask for one');
  });

  await test('An extrude is made into a body or cut from it, or goes right through; a hole\'s line is edited as it is', async () => {
    await win.pushEvents([part()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="11"]').click(); await settle();
    const how = ws().querySelector('[data-cad-settings] [data-cad-f="how"]');
    assert(how && how.value === '' && ws().querySelector('[data-cad-settings] [data-cad-f="body"]')?.disabled, 'a body of its own is not what the boss makes');
    let before = win.commands.length;
    how.value = 'join';
    how.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(ops(before)[0]?.text === 'extrude boss from post distance 8 join plate', `joining did not write the line: ${JSON.stringify(ops(before))}`);
    await win.pushEvents([part()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="8"]').click(); await settle();
    before = win.commands.length;
    const through = ws().querySelector('[data-cad-settings] [data-cad-f="through"]');
    through.checked = true;
    through.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(ops(before)[0]?.text === 'extrude plate from base through', `right through did not write the line: ${JSON.stringify(ops(before))}`);
    // A hole, as the core would send it: its line in a box.
    const p = part();
    p.part.steps.push({index: 13, line: 24, kind: 'hole', name: 'm3', text: 'hole m3 on plate.top at (10, 10) thread M3 through', error: null, note: null, sketch: null, distance: null, symmetric: false, through: false, how: null, body: null, axis: null, angle: null, on: 'plate.top', offset: null});
    await win.pushEvents([p]); await settle();
    const row = ws().querySelector('.cd-history [data-cad-step="13"]');
    assert(row?.textContent.includes('on plate.top') && row.querySelector('use')?.getAttribute('href') === '#i-hole', 'the hole is not listed as one');
    row.click(); await settle();
    const line = ws().querySelector('[data-cad-settings] [data-cad-f="line"]');
    assert(line?.value === 'hole m3 on plate.top at (10, 10) thread M3 through', 'the hole\'s line is not offered');
    before = win.commands.length;
    line.value = 'hole m3 on plate.top at (10, 10)\n  thread M4 through';
    line.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'replace', index: 13, text: 'hole m3 on plate.top at (10, 10) thread M4 through'}]), `the edited line was not sent: ${JSON.stringify(ops(before))}`);
    await win.pushEvents([part()]); await settle();
  });

  await test('Adding a sketch asks for one on XY and opens it as it comes back; extrude takes the chosen sketch', async () => {
    let before = win.commands.length;
    ws().querySelector('[data-cad-add="sketch"]').click(); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'insert', at: null, text: 'sketch s1 on XY\nend'}]), `a sketch was not asked for: ${JSON.stringify(ops(before))}`);
    const p = part();
    p.part.steps.push({index: 13, line: 25, kind: 'sketch', name: 's1', text: 'sketch s1 on XY', error: null, note: null, sketch: null, distance: null, symmetric: false, axis: null, angle: null, on: 'XY', offset: null});
    p.part.sketches.push({index: 13, line: 25, name: 's1', on: 'XY', frame: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1], scale: 1, shapes: [], constraints: [], points: [{name: 'origin', x: 0, y: 0}, {name: 'xaxis', x: 0, y: 0}, {name: 'yaxis', x: 0, y: 0}], dof: 0, solved: true, free: [], error: null, profile: '`s1` has no lines, arcs or circles to make a profile of', regions: 0});
    await win.pushEvents([p]); await settle();
    assert(debug().sketching === 13 && shown(svg()) && shown(ws().querySelector('[data-cad-tools]')), 'the new sketch was not opened');
    assert(ws().querySelector('[data-cad-crumb]').textContent.startsWith('Sketch s1 on XY'), 'the sketch is not named over it');
    ws().querySelector('[data-cad-done]').click(); await settle();
    assert(debug().sketching === null && !shown(svg()), 'Done did not leave the sketch');
    ws().querySelector('.cd-history [data-cad-step="7"]').click(); await settle();
    before = win.commands.length;
    ws().querySelector('[data-cad-add="extrude"]').click(); await settle();
    assert(ops(before)[0]?.text === 'extrude e1 from base distance 10', `extrude did not take the chosen sketch: ${JSON.stringify(ops(before))}`);
    await win.pushEvents([part()]); await settle();
  });

  await test('In a sketch the mouse draws lines, a rectangle, a circle and an arc as lines of the language', async () => {
    await open(7);
    assert(debug().sketching === 7 && shown(svg()), 'the sketch did not open on a double-click');
    // Lines: one level, then one ending on the rectangle's corner.
    sketchKey('l'); await settle();
    assert(debug().sketch.tool === 'line', 'L did not take the line tool');
    let before = win.commands.length;
    await click([100, 10]);
    await click([120, 10.0001]);
    let got = ops(before);
    assert(got.length === 1 && got[0].op === 'sketchAdd' && got[0].sketch === 7 && got[0].text === 'line l1 (100, 10) (120, 10)\nhorizontal l1', `the line was not written, held level: ${JSON.stringify(got)}`);
    before = win.commands.length;
    await click([80, 40]);
    got = ops(before);
    assert(got[0]?.text === 'line l2 l1.end r.tr', `the next line did not start from the last and end on the corner: ${JSON.stringify(got)}`);
    sketchKey('Escape'); sketchKey('Escape'); await settle();
    assert(debug().sketch.tool === 'select', '⎋ did not put the tool down');
    // A rectangle by two corners, given bottom-left first whichever way drawn.
    sketchKey('r');
    before = win.commands.length;
    await click([130, 30]);
    await click([110, 20]);
    assert(ops(before)[0]?.text === 'rect r1 corner (110, 20) size (20, 10)', `the rectangle was not written: ${JSON.stringify(ops(before))}`);
    // A circle round the origin, and an arc.
    sketchKey('c');
    before = win.commands.length;
    await click([0, 0]);
    await click([5, 0]);
    assert(ops(before)[0]?.text === 'circle c1 center origin radius 5', `the circle was not written: ${JSON.stringify(ops(before))}`);
    sketchKey('a');
    before = win.commands.length;
    await click([100, 0]);
    await click([110, 0]);
    await click([100, 20]);
    assert(ops(before)[0]?.text === 'arc a1 center (100, 0) from (110, 0) to (100, 10)', `the arc was not written: ${JSON.stringify(ops(before))}`);
    sketchKey('s'); await settle();
  });

  await test('Chosen shapes are held by a constraint, a dimension is typed over in place, ⌫ takes things out', async () => {
    // r.top and r.bottom, chosen and made equal; one is not a constraint of two.
    await click([40, 40]);
    await click([40, 0], {shiftKey: true});
    assert(JSON.stringify(debug().sketch.picked) === JSON.stringify([{kind: 'shape', name: 'r.top'}, {kind: 'shape', name: 'r.bottom'}]), `the sides were not chosen: ${JSON.stringify(debug().sketch.picked)}`);
    let before = win.commands.length;
    ws().querySelector('[data-cad-hold="parallel"]').click(); await settle();
    assert(ops(before)[0]?.text === 'parallel r.top r.bottom', `parallel was not written: ${JSON.stringify(ops(before))}`);
    // A dimension of one chosen line: its length now.
    await click([80, 20]);
    before = win.commands.length;
    sketchKey('d'); await settle();
    assert(ops(before)[0]?.text === 'length r.right = 40', `the dimension was not written at its length: ${JSON.stringify(ops(before))}`);
    // The 60 between the holes, typed over.
    const dim = [...svg().querySelectorAll('[data-dim]')].find((g) => g.querySelector('.cd-dim-text')?.textContent === '60');
    assert(dim, 'the dimension of 60 is not drawn');
    const text = dim.querySelector('.cd-dim-text').getBoundingClientRect();
    dim.querySelector('.cd-dim-text').dispatchEvent(new win.PointerEvent('pointerdown', {clientX: text.left + 2, clientY: text.top + 2, bubbles: true, pointerId: 7, button: 0, detail: 1}));
    await settle();
    const input = ws().querySelector('[data-cad-value]');
    assert(shown(input) && input.value === '60', 'the value is not offered to type over');
    input.value = '58';
    before = win.commands.length;
    input.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true})); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'setValue', sketch: 7, index: 6, which: null, value: '58'}]), `the value was not set: ${JSON.stringify(ops(before))}`);
    assert(!shown(input), 'the box stayed');
    // A circle's own diameter is typed over the same way, by its shape.
    const hole = [...svg().querySelectorAll('[data-dim]')].find((g) => g.dataset.which === 'size');
    assert(hole, 'a hole\'s diameter is not drawn as a dimension');
    // h1 chosen and taken out: its item.
    await click([10 + 1.7, 10]);
    assert(debug().sketch.picked[0]?.name === 'h1', `the hole was not chosen: ${JSON.stringify(debug().sketch.picked)}`);
    before = win.commands.length;
    sketchKey('Backspace'); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'sketchRemove', sketch: 7, indices: [1]}]), `⌫ did not take the hole out: ${JSON.stringify(ops(before))}`);
  });

  await test('A point dragged goes to the core as it moves, and once more as it is let go', async () => {
    const from = page([80, 40]), to = page([90, 45]);
    const before = win.commands.length;
    svg().dispatchEvent(new win.PointerEvent('pointerdown', {...from, bubbles: true, pointerId: 7, button: 0}));
    svg().dispatchEvent(new win.PointerEvent('pointermove', {clientX: (from.clientX + to.clientX) / 2, clientY: (from.clientY + to.clientY) / 2, bubbles: true, pointerId: 7, buttons: 1}));
    await settle();
    svg().dispatchEvent(new win.PointerEvent('pointerup', {...to, bubbles: true, pointerId: 7, button: 0}));
    await settle();
    const drags = sent(before, 'cadDrag');
    assert(drags.length >= 2 && drags.every((d) => d.sketch === 'base' && d.generation === 5) && !drags[0].done && drags.at(-1).done, `the drag was not sent: ${JSON.stringify(drags)}`);
    assert(['r.tr', 'r.top.start', 'r.right.end'].includes(drags[0].point), `the corner was not what was dragged: ${drags[0].point}`);
    const last = drags.at(-1);
    assert(Math.abs(last.x - 90) < 0.6 && Math.abs(last.y - 45) < 0.6, `it was let go somewhere else: ${last.x}, ${last.y}`);
    sketchKey('Escape'); sketchKey('Escape'); await settle();
    assert(debug().sketching === null, '⎋ with nothing chosen did not leave the sketch');
  });

  await test('Typed text goes to the core after a pause; ⌘S saves it first; ⌘Z undoes; an unsaved part says so', async () => {
    ws().querySelector('[data-cad-tab="text"]').click(); await settle();
    const area = ws().querySelector('[data-cad-text]');
    assert(shown(area) && area.value === PART.text, 'the part\'s text is not shown');
    area.focus();
    area.value = PART.text.replace('param t = 3', 'param t = 4');
    area.dispatchEvent(new win.Event('input', {bubbles: true}));
    let before = win.commands.length;
    await new Promise((r) => setTimeout(r, 450));
    assert(sent(before, 'cadText')[0]?.text.includes('param t = 4'), 'the text was not sent after a pause');
    area.value = PART.text.replace('param t = 3', 'param t = 5');
    area.dispatchEvent(new win.Event('input', {bubbles: true}));
    before = win.commands.length;
    area.dispatchEvent(new win.KeyboardEvent('keydown', {key: 's', metaKey: true, bubbles: true, cancelable: true})); await settle();
    const texts = sent(before, 'cadText'), saves = sent(before, 'cadSave');
    assert(texts[0]?.text.includes('param t = 5') && saves.length === 1 && win.commands.indexOf(texts[0]) < win.commands.indexOf(saves[0]), '⌘S did not send the text before saving');
    await win.pushEvents([part({dirty: true, canUndo: true})]); await settle();
    assert(shown(ws().querySelector('[data-cad-dirty]')), 'an unsaved part has no dot');
    assert(!ws().querySelector('[data-cad-undo]').disabled, 'Undo is not offered');
    /** @type {HTMLElement} */ (ws().querySelector('[data-cad-history]')).focus?.();
    before = win.commands.length;
    ws().querySelector('.cd-history [data-cad-step="part"]').dispatchEvent(new win.KeyboardEvent('keydown', {key: 'z', metaKey: true, bubbles: true, cancelable: true})); await settle();
    assert(sent(before, 'cadUndo')[0]?.generation === 5, '⌘Z did not undo');
    ws().querySelector('[data-cad-tab="step"]').click(); await settle();
    await win.pushEvents([part()]); await settle();
    assert(!shown(ws().querySelector('[data-cad-dirty]')), 'a saved part still has its dot');
  });

  await test('Edges picked are rounded or chamfered; a round\'s edges and radius write its line; a shell opens the face clicked; sweeps and lofts are added', async () => {
    await win.pushEvents([part()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    const plate = PART.bodies[0];
    const [a, b] = ['plate.side(r.bottom) & plate.top', 'plate.side(r.right) & plate.top'];
    debug().pickEdge('plate', plate.edgePicks.indexOf(a));
    debug().pickEdge('plate', plate.edgePicks.indexOf(b)); await settle();
    const card = ws().querySelector('[data-cad-edges]');
    assert(card && card.textContent.includes(a) && card.textContent.includes(b), 'the picked edges are not listed');
    const size = card.querySelector('[data-cad-edge-size]');
    size.value = '0.5';
    size.dispatchEvent(new win.Event('change', {bubbles: true}));
    let before = win.commands.length;
    card.querySelector('[data-cad-round]').click(); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'insert', at: null, text: `fillet r1 on ${a}, ${b} radius 0.5`}]), `Round did not write the line: ${JSON.stringify(ops(before))}`);
    assert(!debug().edges.length, 'the edges stay picked after their round');
    // Picked twice is not picked; a seam is not picked at all.
    debug().pickEdge('plate', plate.edgePicks.indexOf(a));
    debug().pickEdge('plate', plate.edgePicks.indexOf(a)); await settle();
    assert(!debug().edges.length, 'an edge picked twice is still picked');
    debug().pickEdge('plate', plate.edgeTurns.indexOf('seam')); await settle();
    assert(!debug().edges.length && ws().querySelector('[data-cad-status]').textContent.includes('seam'), 'a seam was picked');
    debug().pickEdge('plate', plate.edgePicks.indexOf(a));
    before = win.commands.length;
    ws().querySelector('[data-cad-add="chamfer"]').click(); await settle();
    // The size typed for the round stays for the next.
    assert(ops(before)[0]?.text === `chamfer c1 on ${a} distance 0.5`, `Chamfer did not write the line: ${JSON.stringify(ops(before))}`);
    // The round there is: its radius, and an edge clicked added to it.
    ws().querySelector('.cd-history [data-cad-step="12"]').click(); await settle();
    const radius = ws().querySelector('[data-cad-settings] [data-cad-f="value"]');
    assert(ws().querySelector('[data-cad-settings] [data-cad-f="edges"]')?.value === 'plate.edges(parallel z)' && radius?.value === '2', 'the round\'s settings are not shown');
    before = win.commands.length;
    radius.value = '3';
    radius.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(ops(before)[0]?.text === 'fillet corners on plate.edges(parallel z) radius 3', `the radius did not write the line: ${JSON.stringify(ops(before))}`);
    before = win.commands.length;
    debug().pickEdge('plate', plate.edgePicks.indexOf(a)); await settle();
    assert(ops(before)[0]?.text === `fillet corners on plate.edges(parallel z), ${a} radius 2`, `an edge clicked was not added to the round: ${JSON.stringify(ops(before))}`);
    if (debug().view) assert(debug().view.stepFaces.includes('corners'), 'the round\'s faces are not shown as its');
    // A shell: added for the last body, and a face clicked opened.
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    before = win.commands.length;
    ws().querySelector('[data-cad-add="shell"]').click(); await settle();
    assert(ops(before)[0]?.text === 'shell sh1 of boss thickness 2', `Shell did not write the line: ${JSON.stringify(ops(before))}`);
    const p = part();
    p.part.steps.push({...p.part.steps[4], index: 13, line: 22, kind: 'shell', name: 'sh1', text: 'shell sh1 of plate thickness 1', edges: [], value: '1', other: null, path: null, open: [], body: 'plate', error: null, note: null});
    await win.pushEvents([p]); await settle();
    ws().querySelector('.cd-history [data-cad-step="13"]').click(); await settle();
    before = win.commands.length;
    debug().pickFace('plate', plate.faces.findIndex((f) => f.name === 'plate.top'), null); await settle();
    assert(ops(before)[0]?.text === 'shell sh1 of plate thickness 1 open plate.top', `a face clicked was not opened: ${JSON.stringify(ops(before))}`);
    // A sweep and a loft, from the sketches there are.
    ws().querySelector('.cd-history [data-cad-step="10"]').click(); await settle();
    before = win.commands.length;
    ws().querySelector('[data-cad-add="sweep"]').click(); await settle();
    ws().querySelector('[data-cad-add="loft"]').click(); await settle();
    assert(JSON.stringify(ops(before).map((o) => o.text)) === JSON.stringify(['sweep w1 from post along base', 'loft l1 from base to post']), `Sweep and Loft did not write their lines: ${JSON.stringify(ops(before))}`);
  });

  await test('An assembly lists its instances and mates; faces picked are mated; an instance placed, held, renamed and taken out writes its lines; the fit is checked', async () => {
    const asm = (more = {}) => ({type: 'cadAssembly', assembly: {...structuredClone(ASSEMBLY), generation: 9, dirty: false, ...more}});
    await win.pushEvents([{type: 'cadPart', part: null}, asm()]); await settle();
    assert(shown(ws().querySelector('[data-cad-part]')) && !shown(ws().querySelector('[data-cad-home]')), 'the assembly is not shown');
    assert(ws().querySelector('[data-cad-title]').textContent === 'Test rig', 'the assembly is not named by its title');
    const rows = [...ws().querySelectorAll('.cd-history [data-cad-step]')].map((r) => r.dataset.cadStep);
    assert(rows.join() === 'part,4,5,6,13,14,9,10,11,17,12', `the instances, mates, routes and views are not listed: ${rows}`);
    assert(ws().querySelector('[data-cad-crumb]').textContent.includes(`${ASSEMBLY.dof} freedoms left`), 'the freedoms left are not said');
    assert(!shown(ws().querySelector('[data-cad-add="sketch"]')) && shown(ws().querySelector('[data-cad-asm-add="fit"]')), 'the part\'s buttons are offered for an assembly');
    const settings = ws().querySelector('[data-cad-settings]');
    assert(settings.textContent.includes('block, pin, f1, f2 can still move') && settings.querySelector('.cd-bom')?.textContent.includes('Pin'), 'the assembly\'s card does not say what moves, or list its parts');
    assert(settings.querySelector('.cd-bom')?.textContent.includes('Hard tube 14/16 · 203 mm') && settings.querySelector('svg.cd-loop'), 'the tube to cut, or the loop, is not shown');
    // Two faces picked in the view: mated.
    const plate = ASSEMBLY.bodies.find((b) => b.name === 'plate'), pin = ASSEMBLY.bodies.find((b) => b.name === 'pin');
    debug().pickFace('pin', pin.faces.findIndex((f) => f.name === 'p.side(c)'), null);
    debug().pickFace('plate', plate.faces.findIndex((f) => f.name === 'plate.side(r.left)'), null); await settle();
    assert(debug().picks.join() === 'pin.p.side(c),plate.plate.side(r.left)', `the faces were not picked: ${debug().picks}`);
    let before = win.commands.length;
    const value = ws().querySelector('[data-cad-mate-value]');
    value.value = '3';
    value.dispatchEvent(new win.Event('change', {bubbles: true}));
    ws().querySelector('[data-cad-mate-kind="distance"]').click(); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'insert', at: null, text: 'mate m1 distance pin.p.side(c) to plate.plate.side(r.left) = 3'}]), `the mate was not written: ${JSON.stringify(ops(before))}`);
    assert(!debug().picks.length, 'the faces stay picked after their mate');
    // A second face of the same part replaces the first.
    debug().pickFace('pin', 0, null);
    debug().pickFace('pin', 1, null); await settle();
    assert(debug().picks.length === 1 && debug().picks[0] === `pin.${pin.faces[1].name}`, `two faces of one part were picked to mate: ${debug().picks}`);
    ws().querySelector('[data-cad-unpick]').click(); await settle();
    // An instance placed, held still and renamed.
    ws().querySelector('.cd-history [data-cad-step="6"]').click(); await settle();
    const at = ws().querySelector('[data-cad-settings] [data-cad-a-f="at0"]');
    assert(at?.value === '60', 'where the pin is is not shown');
    before = win.commands.length;
    at.value = '70';
    at.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'replace', index: 8, text: 'place pin at (70, 30, 4)'}]), `moving it did not write its place: ${JSON.stringify(ops(before))}`);
    await win.pushEvents([asm()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="6"]').click(); await settle();
    before = win.commands.length;
    const ground = ws().querySelector('[data-cad-settings] [data-cad-a-f="ground"]');
    ground.checked = true;
    ground.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'insert', at: 7, text: 'ground pin'}]), `holding it still did not write its line: ${JSON.stringify(ops(before))}`);
    await win.pushEvents([asm()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="6"]').click(); await settle();
    before = win.commands.length;
    const nm = ws().querySelector('[data-cad-settings] [data-cad-a-f="name"]');
    nm.value = 'peg';
    nm.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(ops(before)[0]?.text === 'part peg from "parts/pin.part"', `renaming did not write its line: ${JSON.stringify(ops(before))}`);
    // Taken out: with its place.
    await win.pushEvents([asm()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="6"]').click(); await settle();
    before = win.commands.length;
    ws().querySelector('[data-cad-settings] [data-cad-remove]').click(); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'remove', index: 8}, {op: 'remove', index: 6}]), `the pin did not go with its place: ${JSON.stringify(ops(before))}`);
    // A mate's settings write its line.
    await win.pushEvents([asm()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="9"]').click(); await settle();
    before = win.commands.length;
    const kind = ws().querySelector('[data-cad-settings] [data-cad-a-f="kind"]');
    kind.value = 'distance';
    kind.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(ops(before)[0]?.text === 'mate seat distance block.b.bottom to plate.plate.top = 0', `the mate's kind did not write its line: ${JSON.stringify(ops(before))}`);
    // A part of the folder added; the fit checked; the view taken apart.
    await win.pushEvents([{type: 'cadHome', parts: [{path: 'parts/plate.part', name: 'plate', kind: 'part'}, {path: 'rig.assembly', name: 'rig', kind: 'assembly'}]}, asm({path: 'rig.assembly'})]); await settle();
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    const form = ws().querySelector('[data-cad-add-instance]');
    before = win.commands.length;
    form.requestSubmit(); await settle();
    assert(ops(before)[0]?.text === 'part plate1 from "parts/plate.part"', `adding a part did not write its line: ${JSON.stringify(ops(before))}`);
    before = win.commands.length;
    ws().querySelector('[data-cad-fit-check]').click(); await settle();
    assert(sent(before, 'cadFit')[0]?.generation === 9, 'Check fit did not ask');
    await win.pushEvents([asm({clashes: [{a: 'plate', b: 'block', overlap: true, volume: 400, gap: 0, at: [0, 0, 0, 0, 0, 0]}]})]); await settle();
    assert(ws().querySelector('.cd-clashes')?.textContent.includes('overlap 400.00 mm³'), 'what the fit check found is not listed');
    ws().querySelector('.cd-history [data-cad-step="12"]').click(); await settle();
    ws().querySelector('[data-cad-explode]').click(); await settle();
    assert(debug().exploded === 'apart' && ws().querySelector('[data-cad-crumb]').textContent.includes('exploded: apart'), 'the view was not taken apart');
    // The home page makes an assembly.
    await win.pushEvents([{type: 'cadAssembly', assembly: null}]); await settle();
    const home = ws().querySelector('[data-cad-form]');
    home.querySelector('input').value = 'Rig two';
    home.querySelector('[data-cad-kind]').value = 'assembly';
    before = win.commands.length;
    home.requestSubmit(); await settle();
    assert(sent(before, 'cadNewAssembly')[0]?.name === 'Rig two', 'the form did not make an assembly');
    await win.pushEvents([part()]); await settle();
  });

  await test('A tube is routed between two sockets picked; a route lists its bends and writes its points', async () => {
    const asm = (more = {}) => ({type: 'cadAssembly', assembly: {...structuredClone(ASSEMBLY), generation: 11, dirty: false, ...more}});
    await win.pushEvents([{type: 'cadPart', part: null}, asm()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    // Two sockets picked: a tube routed between them.
    const f1 = ASSEMBLY.bodies.find((b) => b.name === 'f1'), socket = f1.faces.findIndex((f) => f.name === 'socket.side(wall1)');
    debug().pickFace('f1', socket, null);
    debug().pickFace('f2', socket, null); await settle();
    let early = win.commands.length;
    ws().querySelector('[data-cad-route-kind="hard"]').click(); await settle();
    assert(JSON.stringify(ops(early)) === JSON.stringify([{op: 'insert', at: null, text: 'route t2 hard 14/16 from f1.socket to f2.socket'}]), `the route was not written: ${JSON.stringify(ops(early))}`);
    // A route's settings: its bends listed, its radius and points written.
    ws().querySelector('.cd-history [data-cad-step="17"]').click(); await settle();
    assert(ws().querySelectorAll('[data-cad-settings] .cd-bom tr').length === 3, 'the route\'s bends are not listed');
    early = win.commands.length;
    const via = ws().querySelector('[data-cad-settings] [data-cad-a-f="via"]');
    via.value = '12, 45, 100\n92, 45, 100';
    via.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(ops(early)[0]?.text === 'route t1 hard 14/16 from f1.socket to f2.socket via (12, 45, 100), (92, 45, 100) bend 30', `the points did not write the line: ${JSON.stringify(ops(early))}`);
  });

  await test('The library makes a part from its families: on the home page to open, in an assembly to add', async () => {
    const families = [
      {name: 'fan', title: 'Fan', group: 'Cooling', params: [{name: 'size', default: '120', choices: ['80', '92', '120', '140'], note: 'mm'}, {name: 'thickness', default: '25', choices: ['15', '25', '38'], note: 'mm'}]},
      {name: 'formd-t1', title: 'FormD T1 (V2.1)', group: 'Cases', params: [{name: 'wall', default: '1.5', choices: [], note: 'mm'}]},
    ];
    await win.pushEvents([{type: 'cadPart', part: null}, {type: 'cadAssembly', assembly: null}, {type: 'cadFamilies', families}]); await settle();
    const lib = ws().querySelector('[data-cad-library]');
    assert(shown(lib) && [...lib.querySelectorAll('[data-cad-family]')].map((b) => b.dataset.cadFamily).join() === 'fan,formd-t1', 'the library\'s families are not listed');
    lib.querySelector('[data-cad-family="fan"]').click(); await settle();
    const size = ws().querySelector('[data-cad-library] [data-cad-lib-param="size"]');
    assert(size?.value === '120', 'the family\'s form does not show its defaults');
    size.value = '140';
    size.dispatchEvent(new win.Event('change', {bubbles: true}));
    let before = win.commands.length;
    ws().querySelector('[data-cad-library] [data-cad-lib-make]').click(); await settle();
    assert(JSON.stringify(sent(before, 'cadLibrary')[0]) === JSON.stringify({type: 'cadLibrary', family: 'fan', params: [['size', '140']], add: false}), `Make did not ask for it: ${JSON.stringify(sent(before, 'cadLibrary'))}`);
    // In an assembly: made and added.
    await win.pushEvents([{type: 'cadPart', part: null}, {type: 'cadAssembly', assembly: {...structuredClone(ASSEMBLY), generation: 9, dirty: false}}]); await settle();
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    const t1 = ws().querySelector('[data-cad-settings] [data-cad-family="formd-t1"]');
    assert(t1, 'the assembly does not offer the library');
    t1.click(); await settle();
    before = win.commands.length;
    ws().querySelector('[data-cad-settings] [data-cad-lib-make]').click(); await settle();
    assert(JSON.stringify(sent(before, 'cadLibrary')[0]) === JSON.stringify({type: 'cadLibrary', family: 'formd-t1', params: [], add: true}), `Make and add did not ask for it: ${JSON.stringify(sent(before, 'cadLibrary'))}`);
    await win.pushEvents([part()]); await settle();
  });

  /** The sample blueprint, as the core sends it. @param {Partial<any>} more */
  const drw = (more = {}) => ({type: 'cadDrawing', drawing: {...structuredClone(DRAWING), generation: 21, dirty: false, ...more}});
  /** A point of the sheet, millimetres from its bottom left, on the page. @param {number} x @param {number} y */
  const onSheet = (x, y) => {
    const el = ws().querySelector('[data-cad-sheet]'), r = el.getBoundingClientRect();
    const ppm = el.querySelector('svg').getBoundingClientRect().width / DRAWING.width, s = debug().sheet;
    return {clientX: r.left + s.x + x * ppm, clientY: r.top + s.y + (DRAWING.height - y) * ppm};
  };

  await test('A part\'s drawing, or an assembly\'s blueprint, is asked for from its settings', async () => {
    await win.pushEvents([part(), {type: 'cadAssembly', assembly: null}, {type: 'cadDrawing', drawing: null}]); await settle();
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    let before = win.commands.length;
    ws().querySelector('[data-cad-make-drawing="paper"]').click(); await settle();
    assert(JSON.stringify(sent(before, 'cadDraw')[0]) === JSON.stringify({type: 'cadDraw', generation: 5, blueprint: false}), `Draw it did not ask: ${JSON.stringify(sent(before, 'cadDraw'))}`);
    await win.pushEvents([{type: 'cadPart', part: null}, {type: 'cadAssembly', assembly: {...structuredClone(ASSEMBLY), generation: 9, dirty: false}}]); await settle();
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    before = win.commands.length;
    ws().querySelector('[data-cad-make-drawing="blueprint"]').click(); await settle();
    assert(sent(before, 'cadDraw')[0]?.blueprint === true, 'Blueprint did not ask for one on blue');
  });

  await test('A drawing shows its sheet and its statements; a view dragged is moved, the sheet zoomed about the pointer', async () => {
    await win.pushEvents([{type: 'cadPart', part: null}, {type: 'cadAssembly', assembly: null}, drw()]); await settle();
    const sheet = ws().querySelector('[data-cad-sheet]');
    assert(shown(sheet) && sheet.querySelector('svg.cd-sheet-svg') && !shown(ws().querySelector('[data-cad-canvas]')), 'the sheet is not shown in place of the 3D view');
    const rows = [...ws().querySelectorAll('.cd-history [data-cad-step]')].map((b) => b.querySelector('.cd-row-name').textContent);
    assert(rows.join() === 'Test rig,front,top,right,iso,apart,a,balloons,parts list', `the statements are not listed: ${rows}`);
    assert(ws().querySelector('[data-cad-crumb]').textContent === 'rig.assembly · A2 · 6 views · 0 dimensions', `the crumb: ${ws().querySelector('[data-cad-crumb]').textContent}`);
    assert(shown(ws().querySelector('[data-cad-drw-add="view"]')) && !shown(ws().querySelector('[data-cad-add="sketch"]')), 'the drawing\'s buttons are not the ones shown');
    // The front view, dragged 20 px right and 10 down.
    const from = onSheet(85, 130), ppm = sheet.querySelector('svg').getBoundingClientRect().width / DRAWING.width;
    const before = win.commands.length;
    sheet.dispatchEvent(new win.PointerEvent('pointerdown', {...from, bubbles: true, pointerId: 4, button: 0}));
    sheet.dispatchEvent(new win.PointerEvent('pointermove', {clientX: from.clientX + 20, clientY: from.clientY + 10, bubbles: true, pointerId: 4}));
    assert(shown(ws().querySelector('[data-cad-sheet-pick]')), 'the view dragged is not outlined');
    sheet.dispatchEvent(new win.PointerEvent('pointerup', {clientX: from.clientX + 20, clientY: from.clientY + 10, bubbles: true, pointerId: 4})); await settle();
    const want = `view front front at (${+(85 + 20 / ppm).toFixed(1)}, ${+(130 - 10 / ppm).toFixed(1)})`;
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'replace', index: 6, text: want}]), `the view was not moved: ${JSON.stringify(ops(before))} — ${want}`);
    assert(debug().step === 6, 'the view moved is not chosen');
    // A click on it chooses it; on the sheet's blank, nothing.
    await win.pushEvents([drw()]); await settle();
    const top = onSheet(85, 310);
    sheet.dispatchEvent(new win.PointerEvent('pointerdown', {...top, bubbles: true, pointerId: 4, button: 0}));
    sheet.dispatchEvent(new win.PointerEvent('pointerup', {...top, bubbles: true, pointerId: 4})); await settle();
    assert(debug().step === 7 && ws().querySelector('[data-cad-settings] [data-cad-drw-line="7"]')?.value === 'view top top at (85, 310)', 'a click did not choose the view');
    // Zoomed: the point under the pointer stays under it.
    // (A wheel's clientX is a whole pixel.)
    const p = onSheet(300, 200), at = {clientX: Math.round(p.clientX), clientY: Math.round(p.clientY)};
    const was = debug().sheet.zoom, under = debug().toSheet(at.clientX, at.clientY);
    sheet.dispatchEvent(new win.WheelEvent('wheel', {...at, deltaY: -200, bubbles: true, cancelable: true})); await settle();
    const now = debug().toSheet(at.clientX, at.clientY);
    assert(debug().sheet.zoom > was && Math.abs(now[0] - under[0]) < 1e-6 && Math.abs(now[1] - under[1]) < 1e-6, `zooming moved the point: ${JSON.stringify([under, now])}`);
    sheet.dispatchEvent(new win.MouseEvent('dblclick', {bubbles: true})); await settle();
    assert(debug().sheet.zoom === 1, 'a double click did not fit the sheet');
  });

  await test('A drawing\'s settings write its lines: the sheet, a view, a dimension, a note, a statement typed; and it is written out', async () => {
    await win.pushEvents([drw()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    const set = (/** @type {string} */ sel, /** @type {string} */ v) => {
      const el = ws().querySelector(`[data-cad-settings] ${sel}`);
      el.value = v;
      el.dispatchEvent(new win.Event('change', {bubbles: true}));
    };
    let before = win.commands.length;
    set('[data-cad-drw-sheet]', 'A1');
    set('[data-cad-drw-style]', 'paper'); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'replace', index: 2, text: 'sheet A1'}, {op: 'replace', index: 5, text: 'style paper'}]), `the sheet's settings: ${JSON.stringify(ops(before))}`);
    before = win.commands.length;
    ws().querySelector('[data-cad-drw-view-form] [name=dir]').value = 'left';
    ws().querySelector('[data-cad-drw-view-form]').requestSubmit(); await settle();
    assert(ops(before)[0]?.text === 'view l1 left at (297, 210)', `a view added: ${JSON.stringify(ops(before))}`);
    const dim = ws().querySelector('[data-cad-drw-dim-form]');
    assert(dim.querySelector('[name=view]').value === 'front' && ws().querySelector('#cd-drw-faces option'), 'the dimension form does not offer the views and faces');
    dim.querySelector('[name=kind]').value = 'clearance';
    dim.querySelector('[name=a]').value = 'block';
    before = win.commands.length;
    dim.requestSubmit(); await settle();
    assert(!ops(before).length && ws().querySelector('[data-cad-status]').textContent.includes('two parts'), 'a clearance with one part was sent');
    dim.querySelector('[name=b]').value = 'plate';
    dim.requestSubmit(); await settle();
    assert(ops(before)[0]?.text === 'dim d1 front clearance block to plate', `a dimension added: ${JSON.stringify(ops(before))}`);
    const note = ws().querySelector('[data-cad-drw-note-form]');
    note.querySelector('[name=text]').value = 'M3 "tapped"';
    note.querySelector('[name=at]').value = 'plate.plate.top';
    before = win.commands.length;
    note.requestSubmit(); await settle();
    assert(ops(before)[0]?.text === 'note n1 front "M3 \'tapped\'" at plate.plate.top', `a note added: ${JSON.stringify(ops(before))}`);
    for (const f of ['pdf', 'svg', 'dxf']) {
      before = win.commands.length;
      ws().querySelector(`[data-cad-settings] [data-cad-export="${f}"]`).click(); await settle();
      assert(sent(before, 'cadExport')[0]?.format === f && sent(before, 'cadExport')[0]?.generation === 21, `Write ${f} did not ask`);
    }
    // A statement chosen: typed, or taken out.
    ws().querySelector('.cd-history [data-cad-step="11"]').click(); await settle();
    before = win.commands.length;
    set('[data-cad-drw-line="11"]', 'section a along x = 50 at (500, 310)'); await settle();
    ws().querySelector('[data-cad-settings] [data-cad-remove="11"]').click(); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'replace', index: 11, text: 'section a along x = 50 at (500, 310)'}, {op: 'remove', index: 11}]), `the statement's edits: ${JSON.stringify(ops(before))}`);
    // What could not be drawn is said, at its line.
    await win.pushEvents([drw({errors: [{line: 14, column: 1, message: '`d1`: no parts called `block` and `plate`'}], items: DRAWING.items.map((i) => (i.index === 13 ? {...i, error: 'no parts'} : i))})]); await settle();
    assert(shown(ws().querySelector('[data-cad-banner]')) && ws().querySelector('[data-cad-banner]').textContent.includes('Line 14'), 'what could not be drawn is not said');
    assert(ws().querySelector('.cd-history [data-cad-step="13"]').classList.contains('bad'), 'the statement that could not be drawn is not marked');
    await win.pushEvents([{type: 'cadDrawing', drawing: null}, part()]); await settle();
  });

  /** The sample fan hub's schematic, as the core sends it. @param {Partial<any>} more */
  const sch = (more = {}) => ({type: 'cadSchematic', schematic: {...structuredClone(SCHEMATIC), generation: 31, dirty: false, ...more}});

  await test('A schematic lists its parts and nets with the rules check, and a part dragged on its sheet moves', async () => {
    await win.pushEvents([{type: 'cadPart', part: null}, {type: 'cadAssembly', assembly: null}, {type: 'cadDrawing', drawing: null}, sch()]); await settle();
    const sheet = ws().querySelector('[data-cad-sheet]');
    assert(shown(sheet) && sheet.querySelector('svg.cd-sheet-svg'), 'the schematic\'s sheet is not shown');
    const rows = [...ws().querySelectorAll('.cd-history [data-cad-step]')].map((b) => b.querySelector('.cd-row-name').textContent);
    assert(rows.join() === 'Fan hub,P1,J1,J2,J3,C1,+12V,GND,PWM,TACH,unconnected', `the statements: ${rows}`);
    assert(ws().querySelector('[data-cad-crumb]').textContent === '5 parts · 4 nets · no errors, 3 warnings', ws().querySelector('[data-cad-crumb]').textContent);
    const p1 = ws().querySelector('.cd-history [data-cad-step="2"]');
    assert(p1.classList.contains('later') && !p1.classList.contains('bad'), 'a warning is marked as an error');
    assert(ws().querySelectorAll('[data-cad-settings] .cd-findings li').length === 3, 'the rules check is not listed');
    assert(shown(ws().querySelector('[data-cad-sch-add="part"]')) && !shown(ws().querySelector('[data-cad-drw-add="view"]')), 'the schematic\'s buttons are not the ones shown');
    // J1 dragged 30 px right.
    const j1 = SCHEMATIC.parts.find((p) => p.reference === 'J1');
    const r = sheet.getBoundingClientRect(), ppm = sheet.querySelector('svg').getBoundingClientRect().width / SCHEMATIC.width, sv = debug().sheet;
    const from = {clientX: r.left + sv.x + j1.at[0] * ppm, clientY: r.top + sv.y + (SCHEMATIC.height - j1.at[1]) * ppm};
    const before = win.commands.length;
    sheet.dispatchEvent(new win.PointerEvent('pointerdown', {...from, bubbles: true, pointerId: 5, button: 0}));
    sheet.dispatchEvent(new win.PointerEvent('pointermove', {clientX: from.clientX + 30, clientY: from.clientY, bubbles: true, pointerId: 5}));
    sheet.dispatchEvent(new win.PointerEvent('pointerup', {clientX: from.clientX + 30, clientY: from.clientY, bubbles: true, pointerId: 5})); await settle();
    const want = `part J1 fan-4pin at (${+(160 + 30 / ppm).toFixed(1)}, 160)`;
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'replace', index: 3, text: want}]), `the part was not moved: ${JSON.stringify(ops(before))} — ${want}`);
    // Chosen: its pins and their nets.
    await win.pushEvents([sch()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="2"]').click(); await settle();
    const pins = [...ws().querySelectorAll('[data-cad-settings] .cd-pins tr')].slice(1).map((tr) => tr.lastElementChild.textContent);
    assert(pins.length === 15 && pins[12] === '+12V' && pins[3] === 'GND' && pins[0] === '—' && pins[6] === 'none', `P1's pins: ${pins}`);
    assert(ws().querySelector('[data-cad-settings]').textContent.includes('Serial ATA'), 'the pinout\'s source is not said');
  });

  await test('A schematic\'s settings add a part and a net, set its sheet, and write it out', async () => {
    await win.pushEvents([sch()]); await settle();
    ws().querySelector('.cd-history [data-cad-step="part"]').click(); await settle();
    const kind = ws().querySelector('[data-cad-sch-kind]');
    kind.value = 'header';
    kind.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const form = ws().querySelector('[data-cad-sch-part-form]');
    form.querySelector('[name=takes]').value = '2x5';
    form.querySelector('[name=value]').value = 'front panel';
    let before = win.commands.length;
    form.requestSubmit(); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'insert', at: 7, text: 'part J4 header 2x5 value "front panel"'}]), `the part: ${JSON.stringify(ops(before))}`);
    const net = ws().querySelector('[data-cad-sch-net-form]');
    net.querySelector('[name=name]').value = 'bad name';
    net.querySelector('[name=pins]').value = 'J1.3';
    before = win.commands.length;
    net.requestSubmit(); await settle();
    assert(!ops(before).length && ws().querySelector('[data-cad-status]').textContent.includes('the pins it joins'), 'a net with a bad name was sent');
    net.querySelector('[name=name]').value = 'TACH2';
    net.querySelector('[name=pins]').value = 'J2.3 J3.3';
    net.querySelector('[name=current]').value = '0.1';
    net.requestSubmit(); await settle();
    assert(ops(before)[0]?.text === 'net TACH2 J2.3, J3.3 current 0.1', `the net: ${JSON.stringify(ops(before))}`);
    before = win.commands.length;
    const size = ws().querySelector('[data-cad-sch-sheet]');
    size.value = 'A3';
    size.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(JSON.stringify(ops(before)) === JSON.stringify([{op: 'replace', index: 1, text: 'sheet A3'}]), `the sheet: ${JSON.stringify(ops(before))}`);
    for (const f of ['net', 'csv', 'pdf']) {
      before = win.commands.length;
      ws().querySelector(`[data-cad-settings] [data-cad-export="${f}"]`).click(); await settle();
      assert(sent(before, 'cadExport')[0]?.format === f && sent(before, 'cadExport')[0]?.generation === 31, `Write ${f} did not ask`);
    }
    // The home page makes one.
    await win.pushEvents([{type: 'cadSchematic', schematic: null}]); await settle();
    const home = ws().querySelector('[data-cad-form]');
    home.querySelector('input').value = 'GPU adapter';
    home.querySelector('[data-cad-kind]').value = 'schematic';
    before = win.commands.length;
    home.requestSubmit(); await settle();
    assert(sent(before, 'cadNewSchematic')[0]?.name === 'GPU adapter', 'the form did not make a schematic');
    await win.pushEvents([part()]); await settle();
  });

  await test('CAD\'s Agent tab holds the composer: @cad is a chip, and its keys are its own', async () => {
    await win.pushEvents([part(), {type: 'cadAssembly', assembly: null}, {type: 'cadDrawing', drawing: null}]); await settle();
    ws().querySelector('[data-cad-tab="agent"]').click(); await settle();
    const pane = ws().querySelector('[data-cad-agentpane]');
    assert(shown(pane) && !shown(ws().querySelector('[data-cad-settings]')) && !shown(ws().querySelector('[data-cad-textpane]')), 'the Agent tab does not show the composer alone');
    const input = pane.querySelector('[data-agent-input]');
    pane.querySelector('[data-mention="@cad"]').click(); await settle();
    assert(input.textContent === '@cad', `the chip did not write @cad: ${JSON.stringify(input.textContent)}`);
    // An F typed there is a letter, not Fit; ⌘Z is the composer's.
    const before = win.commands.length;
    const typed = input.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'f', bubbles: true, cancelable: true}));
    input.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'z', metaKey: true, bubbles: true, cancelable: true})); await settle();
    assert(typed && !sent(before, 'cadUndo').length, 'keys typed in the composer drove CAD');
    // On the home page too.
    await win.pushEvents([{type: 'cadPart', part: null}]); await settle();
    assert(shown(pane), 'the Agent tab is gone on the home page');
    ws().querySelector('[data-cad-tab="step"]').click(); await settle();
    assert(!shown(pane), 'Settings did not take the composer away');
    await win.pushEvents([part()]); await settle();
  });

  await test('The 3D view draws the bodies as the core cut them, or says why it cannot', async () => {
    for (let i = 0; i < 30 && !(debug().view || debug().noView); i++) await settle();
    const v = debug().view;
    if (v) {
      assert(JSON.stringify(v.bodies) === JSON.stringify([{name: 'plate', faces: 12, triangles: PART.bodies[0].faceOf.length}, {name: 'boss', faces: 3, triangles: PART.bodies[1].faceOf.length}]), `the bodies are not drawn: ${JSON.stringify(v.bodies)}`);
      // An edge pointed at in the view is found, named, and picked.
      const plate = PART.bodies[0];
      const k = plate.edgePicks.indexOf('plate.side(r.bottom) & plate.top');
      const e = plate.edges[k], n = e.length - 3;
      const [x, y] = debug().viewer.screen([(e[0] + e[n]) / 2, (e[1] + e[n + 1]) / 2, (e[2] + e[n + 2]) / 2]);
      const canvas = ws().querySelector('[data-cad-canvas]'), r = canvas.getBoundingClientRect();
      const at = {clientX: r.left + x, clientY: r.top + y + 2, bubbles: true, pointerId: 3, button: 0};
      canvas.dispatchEvent(new win.PointerEvent('pointermove', at));
      for (let i = 0; i < 3; i++) await settle();
      assert(JSON.stringify(debug().view.hoveredEdge) === JSON.stringify([0, k]), `the edge under the pointer was not found: ${JSON.stringify(debug().view.hoveredEdge)}`);
      canvas.dispatchEvent(new win.MouseEvent('mousedown', at));
      canvas.dispatchEvent(new win.MouseEvent('click', at)); await settle();
      assert(debug().edges.map((x) => x.pick).join() === 'plate.side(r.bottom) & plate.top', `the edge was not picked: ${JSON.stringify(debug().edges)}`);
      assert(JSON.stringify(debug().view.pickedEdges) === JSON.stringify([['plate', k]]), 'the picked edge is not marked');
      ws().querySelector('[data-cad-unpick-edges]').click(); await settle();
      assert(shown(ws().querySelector('[data-cad-canvas]')), 'the view is not on screen');
    } else {
      assert(debug().noView.includes('WebGL'), `without a 3D view it does not say why: ${debug().noView}`);
    }
    const before = win.commands.length;
    ws().querySelector('[data-cad-close]').click(); await settle();
    assert(sent(before, 'cadClose')[0]?.discard === false, 'Close did not ask to close');
    await win.pushEvents([{type: 'cadPart', part: null}]); await settle();
    assert(shown(ws().querySelector('[data-cad-home]')), 'closing did not go home');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
