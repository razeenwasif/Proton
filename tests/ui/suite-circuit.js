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
    canUndo: false, canRedo: false, made: [], data: [], tables: [], machines: [], palette: ['in', 'out', 'clock', 'const', 'not', 'and', 'or', 'xor', 'mux', 'register'], ...more}});
  /** How many of the canvas's pixels are drawn on. */
  const inked = () => {
    const c = /** @type {HTMLCanvasElement} */ (ws().querySelector('[data-circuit-canvas]'));
    const data = /** @type {CanvasRenderingContext2D} */ (c.getContext('2d')).getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i] > 0) n++;
    return n;
  };
  const frames = () => new Promise((r) => win.requestAnimationFrame(() => win.requestAnimationFrame(r)));

  await test('Circuit sits after Excel: ⌃4 opens it, in Codex\'s chrome and on an opaque ground, and Logic follows it on ⌃5', async () => {
    const asked = () => (win.invokes ?? []).filter((/** @type {any} */ c) => c.name === 'set_opaque').map((/** @type {any} */ c) => c.args.color);
    const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    const before = win.commands.length, opaque = asked().length;
    /** @type {HTMLElement} */ (doc.body).focus();
    key('4', {ctrlKey: true}); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'circuit'), '⌃4 did not ask for Circuit');
    assert($('.workspace.on')?.dataset.ws === 'circuit', 'Circuit is not on screen');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Circuit kept the old chrome');
    assert(!shown($('.titlebar [data-term-toggle]')), 'Circuit offers a terminal it cannot show');
    const tab = $('.titlebar [data-mode="circuit"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃4') && tab.textContent.trim() === 'Circuit', 'the title bar does not show Circuit');
    assert($('.mode-rail [data-mode="circuit"]')?.textContent.trim() === 'Circuit', 'the rail does not say Circuit');
    assert(!asked().slice(opaque).includes(null) && asked().at(-1) === ground, `Circuit is not on the ground: ${JSON.stringify(asked().slice(opaque))}`);
    const before5 = win.commands.length;
    key('5', {ctrlKey: true}); await settle();
    assert(sent(before5, 'setMode').some((c) => c.mode === 'logic'), '⌃5 did not ask for Logic');
    await win.dispatch({type: 'setMode', mode: 'circuit'}); await settle();
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

  await test('Running: ⌘R, wires coloured by value, inputs clicked, a clock edge, gate steps, the clock left running, a quick run', async () => {
    await win.pushEvents([project(), {type: 'circuitRun', run: null}]); await settle(); await frames();
    const stage = ws().querySelector('[data-circuit-stage]');
    stage.focus();
    let before = win.commands.length;
    key('r', {metaKey: true}); await settle();
    assert(sent(before, 'circuitStart')[0]?.generation === 4, '⌘R did not start it');
    const bit = (/** @type {string} */ hex) => ({bits: 1, hex, z: false, someZ: false});
    // Every wire high but the last, low; A high, B low.
    const wires = HALF.wires.map((/** @type {any} */ _w, /** @type {number} */ i) => bit(i === HALF.wires.length - 1 ? '0' : '1'));
    const running = (/** @type {Partial<any>} */ more = {}) => ({type: 'circuitRun', run: {generation: 4, circuit: 'half', wires,
      values: [['A', bit('1')], ['B', bit('0')], ['C', bit('0')], ['S', bit('1')]], waiting: 0, stepped: [], error: null, quick: null, ...more}});
    await win.pushEvents([running()]); await settle(); await frames();
    assert(shown(ws().querySelector('[data-circuit-running]')), 'the run\'s controls are not shown');
    assert(ws().querySelector('[data-circuit-status]').textContent === 'running', 'it does not say it is running');
    // A high wire is drawn green: its middle's pixel has more green than red.
    const c = /** @type {HTMLCanvasElement} */ (ws().querySelector('[data-circuit-canvas]'));
    const dpr = win.devicePixelRatio || 1;
    const w0 = HALF.wires[0], v = win.circuitDebug().view;
    const px = Math.round((v.ox + ((w0[0] + w0[2]) / 2) * v.scale) * dpr), py = Math.round((v.oy + ((w0[1] + w0[3]) / 2) * v.scale) * dpr);
    const [red, green] = /** @type {CanvasRenderingContext2D} */ (c.getContext('2d')).getImageData(px, py, 1, 1).data;
    assert(green > red + 40, `a high wire is not green: rgb ${red}, ${green}`);

    // A click on an input flips it.
    const a = HALF.parts.find((/** @type {any} */ p) => p.id === 'A');
    const r = stage.getBoundingClientRect();
    const at = {clientX: r.left + v.ox + (a.x - 0.7) * v.scale, clientY: r.top + v.oy + a.y * v.scale, bubbles: true, pointerId: 1, button: 0};
    before = win.commands.length;
    stage.dispatchEvent(new win.PointerEvent('pointerdown', at));
    stage.dispatchEvent(new win.PointerEvent('pointerup', at)); await settle();
    const set = sent(before, 'circuitSet')[0];
    assert(set?.id === 'A' && set.value === '0' && set.hold === false, `clicking A sent ${JSON.stringify(set)}`);
    assert(!sent(before, 'circuitEdit').length, 'clicking an input moved it');
    // s is a clock edge; gate by gate holds it, and . takes a gate delay.
    before = win.commands.length;
    key('s'); await settle();
    const hold = ws().querySelector('[data-circuit-hold]');
    hold.click(); stage.focus();
    key('s'); key('.'); await settle();
    const edges = sent(before, 'circuitClock');
    assert(edges.length === 2 && edges[0].hold === false && edges[1].hold === true, `clock edges ${JSON.stringify(edges)}`);
    assert(sent(before, 'circuitStep').length === 1, '. did not step a gate');
    await win.pushEvents([running({waiting: 3})]); await settle();
    assert(ws().querySelector('[data-circuit-status]').textContent.includes('3 waiting'), 'what waits is not said');
    hold.click(); stage.focus();
    // c leaves the clock running, an edge each half period; Escape stops it.
    before = win.commands.length;
    key('c'); await new Promise((done) => setTimeout(done, 700));
    assert(win.circuitDebug().live && sent(before, 'circuitClock').length >= 2, `the clock left running sent ${sent(before, 'circuitClock').length} edges`);
    key('Escape'); await settle();
    const after = sent(before, 'circuitClock').length;
    await new Promise((done) => setTimeout(done, 400));
    assert(!win.circuitDebug().live && sent(before, 'circuitClock').length === after, 'Escape did not stop the clock');
    // A quick run: asked for, its progress said, ⎋ stops it.
    before = win.commands.length;
    ws().querySelector('[data-circuit-quick]').click(); await settle();
    assert(sent(before, 'circuitQuick').length === 1, 'the quick run was not asked for');
    await win.pushEvents([running({wires: [], values: [], quick: {running: true, cycles: 120000, hz: 2400000, ended: null}})]); await settle();
    assert(ws().querySelector('[data-circuit-status]').textContent.includes('2.40 MHz'), 'the quick run\'s speed is not said');
    stage.focus();
    key('Escape'); await settle();
    assert(sent(before, 'circuitStop').length === 1, '⎋ did not stop the quick run');
    await win.pushEvents([running({quick: {running: false, cycles: 120001, hz: 2400000, ended: 'a Break rose'}})]); await settle();
    assert(ws().querySelector('[data-circuit-status]').textContent.includes('a Break rose'), 'why it ended is not said');
    // An error is said, and marked.
    await win.pushEvents([running({error: 'the net n3 is driven two ways at once'})]); await settle();
    assert(ws().querySelector('[data-circuit-status]').classList.contains('bad'), 'an error is not marked');
    before = win.commands.length;
    key('r', {metaKey: true}); await settle();
    assert(sent(before, 'circuitStop').length === 1, '⌘R did not stop it');
    await win.pushEvents([{type: 'circuitRun', run: null}]); await settle();
    assert(!shown(ws().querySelector('[data-circuit-running]')), 'the run\'s controls stayed');
  });

  await test('A display is lit from its value, and a running RAM is kept as a data file', async () => {
    const parts = [
      {id: 'H', kind: 'hexdisplay', x: 2, y: 0, rot: 0, mirror: false, settings: [], pins: [{x: 2, y: 0, input: true}, {x: 2, y: 1, input: true}], problem: null},
      {id: 'M', kind: 'ram', x: 8, y: 0, rot: 0, mirror: false, settings: [['addr', '2']], problem: null,
        pins: [0, 1, 2, 3, 4].map((i) => ({x: 8, y: i, input: true})).concat([{x: 11, y: 0, input: false}])},
    ];
    await win.pushEvents([project({parts, wires: [], palette: ['hexdisplay', 'ram', 'rom']}),
      {type: 'circuitRun', run: {generation: 4, circuit: 'half', wires: [], values: [['H', {bits: 5, hex: '8', z: false, someZ: false}]], waiting: 0, stepped: [], error: null, quick: null}}]);
    await settle(); await frames();
    // An 8 lights every segment: the middle one, g, is lit.
    const c = /** @type {HTMLCanvasElement} */ (ws().querySelector('[data-circuit-canvas]'));
    const dpr = win.devicePixelRatio || 1, v = win.circuitDebug().view;
    const px = Math.round((v.ox + (2 + 1.5 - 0.15) * v.scale) * dpr), py = Math.round((v.oy + 1 * v.scale) * dpr);
    const [red, green] = /** @type {CanvasRenderingContext2D} */ (c.getContext('2d')).getImageData(px, py, 1, 1).data;
    assert(red > green + 40, `the display's middle segment is not lit: rgb ${red}, ${green}`);
    // Enter on a running RAM asks what to keep its contents as.
    const stage = ws().querySelector('[data-circuit-stage]');
    stage.focus();
    key('Escape');
    const r = stage.getBoundingClientRect();
    const at = {clientX: r.left + v.ox + 9.5 * v.scale, clientY: r.top + v.oy + 2 * v.scale, bubbles: true, pointerId: 1, button: 0};
    stage.dispatchEvent(new win.PointerEvent('pointerdown', at));
    stage.dispatchEvent(new win.PointerEvent('pointerup', at)); await settle();
    key('Enter'); await settle();
    const pop = stage.querySelector('.ci-pop [name=name]');
    assert(pop?.value === 'm', 'Enter on a running RAM does not ask for a data file');
    pop.value = 'boot';
    const before = win.commands.length;
    stage.querySelector('.ci-pop').requestSubmit(); await settle();
    const saved = sent(before, 'circuitSaveMemory')[0];
    assert(saved?.id === 'M' && saved.name === 'boot' && saved.generation === 4, `saving sent ${JSON.stringify(saved)}`);
    await win.pushEvents([{type: 'circuitRun', run: null}]); await settle();
  });

  await test('Tests: t opens the tables, written ones kept and run, failures shown, a new one headed; the graph drawn and sent to Data', async () => {
    const tables = [['truth', 'A B S C\n0 0 0 0\n'], ['wrong', 'A B S C\n1 1 1 1\n']];
    await win.pushEvents([project({tables}), {type: 'circuitRun', run: null}]); await settle(); await frames();
    const stage = ws().querySelector('[data-circuit-stage]');
    stage.focus();
    key('t'); await settle();
    assert(shown(ws().querySelector('[data-circuit-panel]')), 't did not open the panel');
    const rows = [...ws().querySelectorAll('[data-circuit-table]')];
    assert(rows.map((r) => r.dataset.circuitTable).join() === 'truth,wrong' && rows[0].classList.contains('on'), 'the tables are not listed');
    const text = ws().querySelector('[data-circuit-table-text]');
    assert(text.value === 'A B S C\n0 0 0 0\n', `the first table is not shown: ${JSON.stringify(text.value)}`);
    // Written, then ⌘↩: kept and run.
    text.focus();
    text.value = 'A B S C\n0 0 0 0\n0 1 1 0\n';
    let before = win.commands.length;
    key('Enter', {metaKey: true}); await settle();
    const kept = sent(before, 'circuitTestSet')[0];
    assert(kept?.label === 'truth' && kept.text === 'A B S C\n0 0 0 0\n0 1 1 0\n' && kept.circuit === 'half', `keeping sent ${JSON.stringify(kept)}`);
    assert(sent(before, 'circuitTestRun')[0]?.label === 'truth', '⌘↩ did not run it');
    // Results: a failure shown, and marked in the list.
    await win.pushEvents([{type: 'circuitTests', generation: 4, circuit: 'half', results: [{label: 'truth', rows: 2, failures: [], error: null}, {label: 'wrong', rows: 1, failures: ['line 2, row 1: S is 0, not 0x1'], error: null}]}]);
    await settle();
    ws().querySelector('[data-circuit-table="wrong"]').click(); await settle();
    assert(ws().querySelector('[data-circuit-table="wrong"] .ci-badge.bad') && ws().querySelector('[data-circuit-table="truth"] .ci-badge.ok'), 'passing and failing are not marked');
    assert(ws().querySelector('.ci-failure')?.textContent.includes('S is 0, not 0x1') && ws().querySelector('.ci-result-sum').textContent.includes('1 of 1'), 'the failing row is not shown');
    // Run all; remove; a new one named, headed with the circuit's inputs and outputs.
    before = win.commands.length;
    ws().querySelector('[data-circuit-run-all]').click(); await settle();
    assert(sent(before, 'circuitTestRun')[0]?.label === null, 'Run all did not run them all');
    ws().querySelector('[data-circuit-table-delete]').click(); await settle();
    assert(sent(before, 'circuitTestSet').some((c) => c.label === 'wrong' && c.text === null), 'Remove did not take it away');
    ws().querySelector('[data-circuit-table-new]').click(); await settle();
    const name = ws().querySelector('[data-circuit-table-form] input');
    assert(name && doc.activeElement === name, 'a new table does not ask its name');
    name.value = 'carry';
    ws().querySelector('[data-circuit-table-form]').requestSubmit(); await settle();
    const made = sent(before, 'circuitTestSet').find((c) => c.label === 'carry');
    assert(made?.text === 'A B C S\n', `the new table is ${JSON.stringify(made)}`);

    // The graph: drawn from the run's recording, sent to Data, cleared.
    ws().querySelector('[data-circuit-tab="graph"]').click(); await settle();
    const bit = (/** @type {string} */ hex) => ({bits: 1, hex, z: false, someZ: false});
    await win.pushEvents([{type: 'circuitRun', run: {generation: 4, circuit: 'half', wires: [], values: [['A', bit('1')]], waiting: 0, stepped: [], error: null, quick: null,
      trace: {signals: [['A', 1], ['B', 1], ['D', 8]], samples: [['0', '0', 'a'], ['1', '0', 'a'], ['1', '1', '1f'], ['0', null, '1f']], total: 4}}}]);
    await settle(); await frames();
    const c = /** @type {HTMLCanvasElement} */ (ws().querySelector('[data-circuit-graph-canvas]'));
    const data = /** @type {CanvasRenderingContext2D} */ (c.getContext('2d')).getImageData(0, 0, c.width, c.height).data;
    let ink = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i] > 0) ink++;
    assert(ink > 300 && ws().querySelector('[data-circuit-graph-empty]').hidden, `the graph is all but empty: ${ink} pixels`);
    before = win.commands.length;
    ws().querySelector('[data-circuit-trace-data]').click();
    ws().querySelector('[data-circuit-trace-clear]').click(); await settle();
    assert(sent(before, 'circuitTraceToData')[0]?.generation === 4 && sent(before, 'circuitTraceClear').length === 1, 'To Data and Clear were not asked for');
    stage.focus();
    key('t'); await settle();
    assert(!shown(ws().querySelector('[data-circuit-panel]')), 't did not close the panel');
    await win.pushEvents([{type: 'circuitRun', run: null}]); await settle();
  });

  await test('Analysis: a circuit or expressions analysed, the table, the least forms and a Karnaugh map drawn, and a circuit built', async () => {
    await win.pushEvents([project(), {type: 'circuitRun', run: null}]); await settle(); await frames();
    const stage = ws().querySelector('[data-circuit-stage]');
    stage.focus();
    if (!shown(ws().querySelector('[data-circuit-panel]'))) key('t');
    await settle();
    ws().querySelector('[data-circuit-tab="analysis"]').click(); await settle();
    const form = ws().querySelector('[data-circuit-analyse-form]');
    let before = win.commands.length;
    form.requestSubmit(); await settle();
    const asked = sent(before, 'circuitAnalyse')[0];
    assert(asked?.circuit === 'half' && asked.expression === null && asked.generation === 4, `analysing sent ${JSON.stringify(asked)}`);
    form.elements.namedItem('expression').value = "Y = A B + C'";
    before = win.commands.length;
    form.requestSubmit(); await settle();
    assert(sent(before, 'circuitAnalyse')[0]?.expression === "Y = A B + C'", 'expressions were not sent');
    // A full adder's analysis, as the core sends it.
    await win.pushEvents([{type: 'circuitAnalysis', generation: 4, analysis: {"source":"main","inputs":["A","B","Cin"],"outputs":["Cout","S"],"columns":["00010111","01101001"],"sop":["B·Cin + A·Cin + A·B","A'·B'·Cin + A'·B·Cin' + A·B'·Cin' + A·B·Cin"],"pos":["(B + Cin)·(A + Cin)·(A + B)","(A + B + Cin)·(A + B' + Cin')·(A' + B + Cin')·(A' + B' + Cin)"],"groups":[[[3,3],[5,5],[6,6]],[[1,7],[2,7],[4,7],[7,7]]],"exact":true,"error":null}}]); await settle(); await frames();
    const rows = ws().querySelectorAll('.ci-truth-table tbody tr');
    assert(rows.length === 8 && rows[7].textContent.replace(/\s/g, '') === '11111', `the truth table has ${rows.length} rows: ${rows[7]?.textContent}`);
    const forms = [...ws().querySelectorAll('.ci-form-line')].map((l) => l.textContent);
    assert(forms[0] === 'Cout = B·Cin + A·Cin + A·B' && forms[1].startsWith('Cout = (B + Cin)'), `the forms are ${JSON.stringify(forms.slice(0, 2))}`);
    const maps = ws().querySelectorAll('[data-circuit-kmap]');
    assert(maps.length === 2 && maps[0].width > 0, 'the Karnaugh maps are not drawn');
    // Build: a name is asked for, then the style and source sent.
    before = win.commands.length;
    form.elements.namedItem('expression').value = '';
    ws().querySelector('[data-circuit-build]').click(); await settle();
    assert(!sent(before, 'circuitSynthesise').length && doc.activeElement === form.elements.namedItem('name'), 'building without a name did not ask for one');
    form.elements.namedItem('name').value = 'adder2';
    form.elements.namedItem('style').value = 'nor';
    ws().querySelector('[data-circuit-build]').click(); await settle();
    const built = sent(before, 'circuitSynthesise')[0];
    assert(built?.name === 'adder2' && built.style === 'nor' && built.circuit === 'half' && built.expression === null, `building sent ${JSON.stringify(built)}`);
    // A circuit that cannot be tabulated says why.
    await win.pushEvents([{type: 'circuitAnalysis', generation: 4, analysis: {source: 'half', inputs: [], outputs: [], columns: [], sop: [], pos: [], groups: [], exact: true, error: 'this circuit keeps state'}}]); await settle();
    assert(ws().querySelector('.ci-truth .ci-failure')?.textContent.includes('keeps state'), 'the reason there is no table is not said');
    stage.focus();
    key('t'); await settle();
  });

  await test('Machines: written and read as typed, drawn with their state table, kept, and built with D or JK flip-flops', async () => {
    const text = "inputs X\noutputs Z\nstate S0\nstate S1\nstate S2 Z=1\nS0 -> S1 when X\nS1 -> S2 when X\nS1 -> S0 when X'\nS2 -> S0 when !X\n";
    await win.pushEvents([project({machines: [['ones', text]]}), {type: 'circuitRun', run: null}]); await settle(); await frames();
    const stage = ws().querySelector('[data-circuit-stage]');
    stage.focus();
    if (!shown(ws().querySelector('[data-circuit-panel]'))) key('t');
    await settle();
    let before = win.commands.length;
    ws().querySelector('[data-circuit-tab="machines"]').click(); await settle();
    assert(ws().querySelector('[data-circuit-machine="ones"]')?.classList.contains('on'), 'the machine is not listed');
    const area = ws().querySelector('[data-circuit-machine-text]');
    assert(area.value === text, 'its text is not shown');
    assert(sent(before, 'circuitMachineShow')[0]?.name === 'ones', 'the machine was not read');
    await win.pushEvents([{type: 'circuitMachine', generation: 4, machine: {...{"name":null,"inputs":["X"],"outputs":["Z"],"states":["S0","S1","S2"],"stateOutputs":["","","Z=1"],"transitions":[[0,1,"X",""],[1,2,"X",""],[1,0,"X'",""],[2,0,"!X",""]],"mealy":false,"bits":2,"table":[[0,"0",0,"0"],[0,"1",1,"0"],[1,"0",0,"0"],[1,"1",2,"0"],[2,"0",0,"1"],[2,"1",2,"1"]],"error":null}, name: 'ones'}}]); await settle(); await frames();
    assert(ws().querySelectorAll('.ci-machine-table tbody tr').length === 6, 'the state table is not drawn');
    const c = /** @type {HTMLCanvasElement} */ (ws().querySelector('[data-circuit-machine-diagram]'));
    const data = /** @type {CanvasRenderingContext2D} */ (c.getContext('2d')).getImageData(0, 0, c.width, c.height).data;
    let ink = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i] > 0) ink++;
    assert(ink > 500, `the diagram is all but empty: ${ink} pixels`);
    // Typing reads it again, a moment after; Keep keeps it.
    area.focus();
    area.value = text + 'S2 -> S2 when X\n';
    area.dispatchEvent(new win.Event('input'));
    before = win.commands.length;
    await new Promise((done) => setTimeout(done, 400));
    assert(sent(before, 'circuitMachineShow')[0]?.text?.endsWith('S2 -> S2 when X\n'), 'what was typed was not read');
    ws().querySelector('[data-circuit-machine-keep]').click(); await settle();
    assert(sent(before, 'circuitMachineSet')[0]?.name === 'ones', 'Keep did not keep it');
    await win.pushEvents([{type: 'circuitMachine', generation: 4, machine: {name: null, inputs: [], outputs: [], states: [], stateOutputs: [], transitions: [], mealy: false, bits: 0, table: [], error: 'line 3: Y is not one of the inputs'}}]); await settle();
    assert(ws().querySelector('.ci-machine-table .ci-failure')?.textContent.includes('Y is not'), 'a machine that does not read does not say why');
    // Build: a name is asked for; then the flip-flops chosen are sent.
    before = win.commands.length;
    ws().querySelector('[data-circuit-machine-build]').click(); await settle();
    assert(!sent(before, 'circuitMachineBuild').length, 'it built without a name');
    ws().querySelector('[data-circuit-machine-circuit]').value = 'detector';
    ws().querySelector('[data-circuit-machine-ff]').value = 'jk';
    ws().querySelector('[data-circuit-machine-build]').click(); await settle();
    const built = sent(before, 'circuitMachineBuild')[0];
    assert(built?.name === 'ones' && built.flipflop === 'jk' && built.circuit === 'detector', `building sent ${JSON.stringify(built)}`);
    // A new machine: named, and begun from a template.
    ws().querySelector('[data-circuit-machine-new]').click(); await settle();
    const name = ws().querySelector('[data-circuit-machine-form] input');
    name.value = 'blink';
    ws().querySelector('[data-circuit-machine-form]').requestSubmit(); await settle();
    const made = sent(before, 'circuitMachineSet').find((c) => c.name === 'blink');
    assert(made?.text?.startsWith('inputs X\noutputs Z'), `the new machine is ${JSON.stringify(made)}`);
    stage.focus();
    key('t'); await settle();
  });

  await test('Export and import: Verilog, VHDL or SVG written out; a Digital file on the home page imported; a popup keeps its clicks', async () => {
    await win.pushEvents([project(), {type: 'circuitRun', run: null}]); await settle(); await frames();
    let before = win.commands.length;
    ws().querySelector('[data-circuit-export]').click(); await settle();
    const choices = [...ws().querySelectorAll('[data-circuit-export-as]')].map((b) => b.dataset.circuitExportAs);
    assert(choices.join() === 'verilog,vhdl,svg', `the export menu offers ${choices.join()}`);
    // A click inside the popup is the popup's: it stays open for it.
    const stage = ws().querySelector('[data-circuit-stage]');
    const choice = ws().querySelector('[data-circuit-export-as="vhdl"]');
    const r = choice.getBoundingClientRect();
    choice.dispatchEvent(new win.PointerEvent('pointerdown', {clientX: r.left + 4, clientY: r.top + 4, bubbles: true, pointerId: 1, button: 0}));
    assert(ws().querySelector('.ci-pop'), 'pressing inside the popup closed it');
    choice.click(); await settle();
    const asked = sent(before, 'circuitExport')[0];
    assert(asked?.format === 'vhdl' && asked.circuit === 'half' && asked.generation === 4 && !ws().querySelector('.ci-pop'), `exporting sent ${JSON.stringify(asked)}`);
    // The home page: a Digital file, marked so, imported when opened.
    await win.pushEvents([{type: 'circuitHome', projects: [{path: 'labs/adder.circuit', name: 'adder', kind: 'circuit'}, {path: 'course/cpu.dig', name: 'cpu', kind: 'dig'}]}, {type: 'circuitProject', project: null}]);
    await settle();
    const dig = ws().querySelector('[data-circuit-list] [data-circuit-import="course/cpu.dig"]');
    assert(dig?.textContent.includes('Digital'), 'a Digital file is not listed as one');
    before = win.commands.length;
    dig.click(); await settle();
    assert(sent(before, 'circuitImport')[0]?.path === 'course/cpu.dig', 'opening a Digital file did not import it');
    // The project back, for the checks after.
    await win.pushEvents([project()]); await settle(); await frames();
    void stage;
  });

  await test('Circuit has the agent in its dock, with @circuit, and tells the core what is selected', async () => {
    await win.pushEvents([project()]); await settle(); await frames();
    const dock = ws().querySelector('.xl-dock');
    assert(dock?.querySelector('[data-agent-input]') && dock.querySelector('[data-mention="@circuit"]'), 'there is no agent composer with @circuit');
    const toggle = [...doc.querySelectorAll('[data-panel-toggle="right"]')].find((b) => b.getClientRects().length);
    assert(toggle && !toggle.disabled, 'the dock cannot be shown');
    const stage = ws().querySelector('[data-circuit-stage]');
    stage.focus();
    key('Escape');
    const before = win.commands.length;
    key('a', {metaKey: true});
    await new Promise((done) => setTimeout(done, 300)); await settle();
    const told = sent(before, 'circuitSelect').at(-1);
    assert(told?.generation === 4 && told.circuit === 'half' && told.ids.length === HALF.parts.length && told.wires.length === HALF.wires.length, `the core was told ${JSON.stringify(told)?.slice(0, 200)}`);
    key('Escape');
  });

  await test('A circuit of 2,500 parts: drawn quickly at every zoom, as boxes when far out, only what is on the stage', async () => {
    const {junctionsOf} = await import('/circuit.js');
    assert(junctionsOf(HALF, () => false).join() === '2,0,3,2', `the half adder's junctions: ${junctionsOf(HALF, () => false)}`);
    assert(junctionsOf(HALF, (w) => w.join() === '2,0,2,4').join() === '3,2', 'a wire left out still made a junction');
    // The half adder 417 times over, as tests/ui/circuit-bench.js lays it out.
    const parts = [], wires = [];
    for (let i = 0; i < 417; i++) {
      const ox = (i % 21) * 12, oy = Math.floor(i / 21) * 9;
      for (const p of HALF.parts) parts.push({...p, id: `${p.id}${i}`, x: p.x + ox, y: p.y + oy, pins: p.pins.map((q) => ({...q, x: q.x + ox, y: q.y + oy}))});
      for (const w of HALF.wires) wires.push([w[0] + ox, w[1] + oy, w[2] + ox, w[3] + oy]);
    }
    await win.pushEvents([project({parts, wires})]); await settle();
    const stage = ws().querySelector('[data-circuit-stage]');
    stage.focus();
    key('='); await frames();
    /** The drawing's own time, the median of a dozen, in ms. */
    const median = () => {
      const draw = win.circuitDebug().draw, times = [];
      for (let i = 0; i < 12; i++) {
        const t = win.performance.now();
        draw();
        times.push(win.performance.now() - t);
      }
      return times.sort((a, b) => a - b)[6];
    };
    assert(win.circuitDebug().view.scale < 6 && inked() > 1000, `fitted, the circuit is not drawn small: ${win.circuitDebug().view.scale}`);
    // Ten times the measured time (tests/ui/circuit-bench.js): a regression,
    // not a slow machine, is what fails this.
    const fitted = median();
    key('+'); key('+'); key('+'); await frames();
    const close = median();
    await win.pushEvents([{type: 'circuitRun', run: {generation: 4, circuit: 'half', wires: wires.map((_, i) => ({bits: 1, hex: String(i % 2), z: false, someZ: false})),
      values: parts.filter((p) => p.kind === 'in').map((p) => [p.id, {bits: 1, hex: '1', z: false, someZ: false}]), waiting: 0, stepped: [], error: null, quick: null}}]);
    await settle();
    const running = median();
    assert(fitted < 16 && close < 16 && running < 16, `drawing 2,502 parts took ${fitted.toFixed(1)}, ${close.toFixed(1)} and ${running.toFixed(1)} ms`);
    // Panned so most of it is off the stage, less is drawn, and quicker.
    const ink = inked();
    stage.dispatchEvent(new win.WheelEvent('wheel', {deltaX: 2000, deltaY: 600, bubbles: true, cancelable: true})); await frames();
    assert(inked() < ink, 'panning away drew as much');
    await win.pushEvents([{type: 'circuitRun', run: null}, project()]); await settle();
    key('='); await frames();
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
