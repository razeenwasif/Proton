/* Circuit's checks, run by suite.js in the same fixture window: its place
 * on the rail, the home page, and a circuit as the core's events draw it —
 * on the canvas, the circuits listed, the commands its clicks and keys
 * send. The fixture transport answers none of these; the events are pushed.
 * The half adder here is as the core sent it, its pins placed there. */

const HALF = {"parts":[{"id":"A","kind":"in","x":0,"y":0,"rot":0,"mirror":false,"settings":[["label","A"]],"pins":[{"x":0,"y":0,"input":false}],"problem":null},{"id":"B","kind":"in","x":0,"y":2,"rot":0,"mirror":false,"settings":[["label","B"]],"pins":[{"x":0,"y":2,"input":false}],"problem":null},{"id":"C","kind":"out","x":8,"y":4,"rot":0,"mirror":false,"settings":[["label","C"]],"pins":[{"x":8,"y":4,"input":true}],"problem":null},{"id":"S","kind":"out","x":8,"y":0,"rot":0,"mirror":false,"settings":[["label","S"]],"pins":[{"x":8,"y":0,"input":true}],"problem":null},{"id":"X","kind":"xor","x":4,"y":0,"rot":0,"mirror":false,"settings":[],"pins":[{"x":4,"y":0,"input":true},{"x":4,"y":2,"input":true},{"x":7,"y":1,"input":false}],"problem":null},{"id":"Y","kind":"and","x":4,"y":4,"rot":0,"mirror":false,"settings":[],"pins":[{"x":4,"y":4,"input":true},{"x":4,"y":6,"input":true},{"x":7,"y":5,"input":false}],"problem":null}],"wires":[[0,0,2,0],[0,2,3,2],[2,0,2,4],[2,0,4,0],[2,4,4,4],[3,2,3,6],[3,2,4,2],[3,6,4,6],[7,1,8,1],[7,5,8,5],[8,0,8,1],[8,4,8,5]]};

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, key: Function, shown: (el: any) => boolean}} t */
export async function circuitChecks({test, assert, win, doc, $, settle, key, shown}) {
  const ws = () => $('.workspace[data-ws="circuit"]');
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const project = (/** @type {Partial<any>} */ more = {}) => ({type: 'circuitProject', project: {path: 'labs/adder.circuit', name: 'adder', top: 'main',
    circuits: ['half', 'main'], shown: 'half', parts: HALF.parts, wires: HALF.wires, tests: [], generation: 4, dirty: false,
    canUndo: false, canRedo: false, made: [], palette: ['in', 'out', 'clock', 'const', 'not', 'and', 'or', 'xor', 'mux', 'register'], ...more}});
  /** How many of the canvas's pixels are drawn on. */
  const inked = () => {
    const c = /** @type {HTMLCanvasElement} */ (ws().querySelector('[data-circuit-canvas]'));
    const data = /** @type {CanvasRenderingContext2D} */ (c.getContext('2d')).getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i] > 0) n++;
    return n;
  };
  const frames = () => new Promise((r) => win.requestAnimationFrame(() => win.requestAnimationFrame(r)));

  await test('Circuit sits after Excel: ⌃7 opens it, in Codex\'s chrome and on an opaque ground, and is the last', async () => {
    const asked = () => (win.invokes ?? []).filter((/** @type {any} */ c) => c.name === 'set_opaque').map((/** @type {any} */ c) => c.args.color);
    const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
    await win.dispatch({type: 'setMode', mode: 'chat'}); await settle();
    const before = win.commands.length, opaque = asked().length;
    /** @type {HTMLElement} */ (doc.body).focus();
    key('7', {ctrlKey: true}); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'circuit'), '⌃7 did not ask for Circuit');
    assert($('.workspace.on')?.dataset.ws === 'circuit', 'Circuit is not on screen');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Circuit kept the old chrome');
    assert(!shown($('.titlebar [data-term-toggle]')), 'Circuit offers a terminal it cannot show');
    const tab = $('.titlebar [data-mode="circuit"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃7') && tab.textContent.trim() === 'Circuit', 'the title bar does not show Circuit');
    assert($('.mode-rail [data-mode="circuit"]')?.textContent.trim() === 'Circuit', 'the rail does not say Circuit');
    assert(!asked().slice(opaque).includes(null) && asked().at(-1) === ground, `Circuit is not on the ground: ${JSON.stringify(asked().slice(opaque))}`);
    const before8 = win.commands.length;
    key('8', {ctrlKey: true}); await settle();
    assert(!sent(before8, 'setMode').length, '⌃8 asked for a mode');
  });

  await test('With no project open, Circuit lists the folder\'s and makes one', async () => {
    await win.pushEvents([{type: 'circuitHome', projects: [{path: 'labs/adder.circuit', name: 'adder'}, {path: 'cpu.circuit', name: 'cpu'}]},
      {type: 'circuitProject', project: null}]);
    await settle();
    assert(shown(ws().querySelector('[data-circuit-home]')) && !shown(ws().querySelector('[data-circuit-project]')), 'the home page is not what is shown');
    const rows = [...ws().querySelectorAll('[data-circuit-list] [data-circuit-open]')];
    assert(rows.map((r) => r.dataset.circuitOpen).join() === 'labs/adder.circuit,cpu.circuit', 'the projects are not listed');
    const before = win.commands.length;
    rows[0].click(); await settle();
    assert(sent(before, 'circuitOpen')[0]?.path === 'labs/adder.circuit', 'clicking a project did not open it');
    const form = ws().querySelector('[data-circuit-form]');
    form.querySelector('input').value = 'Traffic Lights';
    form.requestSubmit(); await settle();
    assert(sent(before, 'circuitNew')[0]?.name === 'Traffic Lights', 'the form did not make a project');
  });

  await test('A circuit is drawn on the canvas and fitted, its circuits listed with the top marked', async () => {
    await win.pushEvents([project()]); await settle(); await frames();
    assert(shown(ws().querySelector('[data-circuit-project]')) && !shown(ws().querySelector('[data-circuit-home]')), 'the project is not shown');
    assert(inked() > 500, `the canvas is all but empty: ${inked()} pixels`);
    assert(ws().querySelector('[data-circuit-empty]').hidden, 'a circuit with parts says it has none');
    const list = [...ws().querySelectorAll('[data-circuit-show]')];
    assert(list.map((r) => r.dataset.circuitShow).join() === 'half,main' && list[0].classList.contains('on'), 'the circuits are not listed');
    assert(list[1].textContent.includes('top') && !list[1].querySelector('[data-circuit-top]'), 'the top circuit is not marked');
    assert(ws().querySelector('[data-circuit-shown]').textContent.includes('top is main'), 'the header does not say which circuit is the top');
    // Every part is inside the stage once fitted.
    const {view} = win.circuitDebug?.() ?? {};
    const stage = ws().querySelector('[data-circuit-stage]').getBoundingClientRect();
    if (view) {
      for (const p of HALF.parts) for (const pin of p.pins) {
        const x = view.ox + pin.x * view.scale, y = view.oy + pin.y * view.scale;
        assert(x > 0 && y > 0 && x < stage.width && y < stage.height, `pin ${p.id} at (${x}, ${y}) is off the stage`);
      }
    }
  });

  await test('Editing: placing from the palette and with space, wiring from a pin, moving, turning, settings, deleting, undo', async () => {
    await win.pushEvents([project()]); await settle(); await frames();
    const stage = ws().querySelector('[data-circuit-stage]');
    const edits = (/** @type {number} */ from) => sent(from, 'circuitEdit').map((c) => {
      assert(c.generation === 4 && c.circuit === 'half', `an edit went to ${c.generation}/${c.circuit}`);
      return c.ops;
    });
    const r = stage.getBoundingClientRect();
    const v = () => win.circuitDebug().view;
    const at = (/** @type {number} */ x, /** @type {number} */ y) => ({clientX: r.left + v().ox + x * v().scale, clientY: r.top + v().oy + y * v().scale, bubbles: true, pointerId: 1, button: 0});
    const drag = (/** @type {number[][]} */ path) => {
      stage.dispatchEvent(new win.PointerEvent('pointerdown', at(path[0][0], path[0][1])));
      for (const q of path.slice(1)) stage.dispatchEvent(new win.PointerEvent('pointermove', at(q[0], q[1])));
      const end = path[path.length - 1];
      stage.dispatchEvent(new win.PointerEvent('pointerup', at(end[0], end[1])));
    };

    // The palette lists what the core offers; a click places it at the cursor.
    const kinds = [...ws().querySelectorAll('[data-circuit-place]')].map((b) => b.dataset.circuitPlace);
    assert(kinds.slice(0, 4).join() === 'in,out,clock,const' && kinds.includes('register'), `the palette is ${kinds.join()}`);
    let before = win.commands.length;
    ws().querySelector('[data-circuit-place="not"]').click(); await settle();
    let ops = edits(before);
    assert(ops.length === 1 && ops[0][0].op === 'place' && ops[0][0].kind === 'not', `placing sent ${JSON.stringify(ops)}`);
    const cursor = [ops[0][0].x, ops[0][0].y];
    assert(doc.activeElement === stage, 'the drawing does not have the keys after placing');
    // The cursor moves by h j k l; space places the last kind again there.
    before = win.commands.length;
    key('l'); key('l'); key('j'); key(' '); await settle();
    ops = edits(before);
    assert(ops[0]?.[0].kind === 'not' && ops[0][0].x === cursor[0] + 2 && ops[0][0].y === cursor[1] + 1, `space placed ${JSON.stringify(ops)}`);
    // `a` picks a part by name.
    before = win.commands.length;
    key('a'); await settle();
    const pickIn = stage.querySelector('.ci-pop [name=kind]');
    assert(pickIn && doc.activeElement === pickIn, 'a does not ask which part');
    pickIn.value = 'mux';
    stage.querySelector('.ci-pop').requestSubmit(); await settle();
    assert(edits(before)[0]?.[0].kind === 'mux', 'the part picked was not placed');

    // A drag from a pin draws a wire, across and then down.
    const xor = HALF.parts.find((/** @type {any} */ p) => p.kind === 'xor');
    const out = xor.pins.find((/** @type {any} */ q) => !q.input);
    before = win.commands.length;
    drag([[out.x, out.y], [out.x + 1, out.y + 1], [out.x + 2, out.y + 3]]); await settle();
    ops = edits(before);
    assert(JSON.stringify(ops[0]?.[0]) === JSON.stringify({op: 'wire', points: [[out.x, out.y], [out.x + 2, out.y], [out.x + 2, out.y + 3]]}), `the wire sent ${JSON.stringify(ops)}`);
    // A drag from a part's body moves it — selected — by whole steps.
    const and = HALF.parts.find((/** @type {any} */ p) => p.kind === 'and');
    before = win.commands.length;
    drag([[and.x + 1.5, and.y + 1], [and.x + 2, and.y + 2], [and.x + 3.5, and.y + 3]]); await settle();
    ops = edits(before);
    assert(JSON.stringify(ops[0]?.[0]) === JSON.stringify({op: 'move', ids: [and.id], wires: [], dx: 2, dy: 2}), `the move sent ${JSON.stringify(ops)}`);
    // r turns, m mirrors, Enter edits its settings, x deletes: the selection.
    before = win.commands.length;
    key('r'); key('m'); await settle();
    ops = edits(before);
    assert(ops[0]?.[0].op === 'rotate' && ops[0][0].ids.join() === and.id && ops[1]?.[0].op === 'mirror', `r and m sent ${JSON.stringify(ops)}`);
    key('Enter'); await settle();
    const pop = stage.querySelector('.ci-pop');
    assert(pop && pop.querySelector('[name=id]').value === and.id, 'Enter does not edit the part');
    pop.querySelector('[name=id]').value = 'Carry';
    pop.querySelector('[name=settings]').value = 'inputs=3 label="carry out"';
    before = win.commands.length;
    pop.requestSubmit(); await settle();
    ops = edits(before);
    assert(JSON.stringify(ops[0]?.[0]) === JSON.stringify({op: 'set', id: and.id, newId: 'Carry', settings: [['inputs', '3'], ['label', 'carry out']]}), `settings sent ${JSON.stringify(ops)}`);
    assert(doc.activeElement === stage, 'the keys did not come back to the drawing');
    // Dragging a box over nothing but the wires selects them; x deletes them.
    await win.pushEvents([project()]); await settle(); await frames();
    stage.focus();
    key('Escape');
    const xs = HALF.wires.flatMap((/** @type {number[]} */ w) => [w[0], w[2]]), ys = HALF.wires.flatMap((/** @type {number[]} */ w) => [w[1], w[3]]);
    const mx = Math.min(...xs), Mx = Math.max(...xs);
    before = win.commands.length;
    drag([[mx + 1.5, -3], [mx + 2, 2], [mx + 2.5, Math.max(...ys) + 3]]); await settle();
    key('x'); await settle();
    ops = edits(before);
    const gone = ops[0]?.[0];
    assert(gone?.op === 'delete' && gone.ids.length === 0 && gone.wires.length > 0 && gone.wires.every((/** @type {number[]} */ w) => w[0] === mx + 2 && w[2] === mx + 2), `the box deleted ${JSON.stringify(gone)} (wires span ${mx}–${Mx})`);
    // y copies; u, ⌘Z and ⌃r and ⇧⌘Z undo and redo.
    ws().querySelector('[data-circuit-place="and"]'); // still offered
    before = win.commands.length;
    drag([[and.x + 1.5, and.y + 1], [and.x + 1.5, and.y + 1]]); await settle();
    key('y'); await settle();
    assert(sent(before, 'circuitCopy')[0]?.ids.join() === and.id, 'y did not copy the selection');
    key('u'); key('z', {metaKey: true}); key('r', {ctrlKey: true}); key('z', {metaKey: true, shiftKey: true}); await settle();
    assert(sent(before, 'circuitUndo').length === 2 && sent(before, 'circuitRedo').length === 2, 'undo and redo were not asked for');
    // What an edit made comes back selected.
    await win.pushEvents([project({made: [HALF.parts[0].id]})]); await settle();
    before = win.commands.length;
    key('Delete'); await settle();
    assert(edits(before)[0]?.[0].ids.join() === HALF.parts[0].id, 'what was made is not what is selected');
  });

  await test('Clicks and keys: another circuit, a new one, the top, zoom and fit, ⌘S and close', async () => {
    const before = win.commands.length;
    ws().querySelector('[data-circuit-show="main"]').click(); await settle();
    assert(sent(before, 'circuitShow')[0]?.circuit === 'main', 'clicking a circuit did not show it');
    ws().querySelector('[data-circuit-top="half"]').click(); await settle();
    assert(sent(before, 'circuitSetTop')[0]?.circuit === 'half', 'top? did not make it the top');
    ws().querySelector('[data-circuit-add]').click(); await settle();
    const input = ws().querySelector('[data-circuit-add-form] input');
    assert(input && doc.activeElement === input, 'adding a circuit does not ask its name');
    input.value = 'mux';
    ws().querySelector('[data-circuit-add-form]').requestSubmit(); await settle();
    assert(sent(before, 'circuitAdd')[0]?.name === 'mux', 'the name did not add a circuit');
    await win.pushEvents([project({dirty: true})]); await settle();
    assert(!ws().querySelector('[data-circuit-dirty]').hidden, 'an unsaved project does not say so');

    const stage = ws().querySelector('[data-circuit-stage]');
    stage.focus();
    const scale = win.circuitDebug().view.scale;
    key('+'); await frames();
    assert(win.circuitDebug().view.scale > scale, '+ did not zoom in');
    key('='); await frames();
    assert(Math.abs(win.circuitDebug().view.scale - scale) < 1e-6, '= did not fit it again');
    key('s', {metaKey: true}); await settle();
    assert(sent(before, 'circuitSave').length === 1, '⌘S did not save');
    await win.pushEvents([project({dirty: false})]); await settle();
    ws().querySelector('[data-circuit-close]').click(); await settle();
    assert(sent(before, 'circuitClose')[0]?.discard === true, 'close did not close');
    await win.pushEvents([{type: 'circuitProject', project: null}]); await settle();
    assert(shown(ws().querySelector('[data-circuit-home]')), 'closing did not go home');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
