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
    circuits: ['half', 'main'], shown: 'half', parts: HALF.parts, wires: HALF.wires, tests: [], generation: 4, dirty: false, ...more}});
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
