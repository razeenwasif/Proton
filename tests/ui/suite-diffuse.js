/* Diffuse's checks, run by suite.js in the same fixture window: ⌃\ and the
 * newer modes' chrome, the flows and a new one, the graph drawn from the
 * core's answer, a wire pulled to a socket and to empty space, a node moved
 * and removed, fields set from the inspector, forge not serving, and a flow
 * with changes asked about before it closes.
 *
 * The core is stood in for by the preview's (`preview-diffuse.js`), whose
 * answers are pushed as the core's would be. */

import {createDiffusePreview} from '/preview-diffuse.js';

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, shown: (el: any) => boolean}} t */
export async function diffuseChecks({test, assert, win, doc, $, settle, shown}) {
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const ws = () => $('.workspace[data-ws="diffuse"]');
  const core = createDiffusePreview(() => {});
  /** The stand-in's answer to a command, pushed as the core's. @param {any} command */
  const answer = async (command) => {
    await win.pushEvents(core.command(command));
    await settle();
  };
  const fire = (/** @type {Element} */ el, /** @type {string} */ type, /** @type {number} */ x, /** @type {number} */ y, /** @type {PointerEventInit} */ extra = {}) =>
    el.dispatchEvent(new win.PointerEvent(type, {bubbles: true, button: 0, clientX: x, clientY: y, pointerId: 41, ...extra}));
  const centre = (/** @type {Element} */ el) => {
    const r = el.getBoundingClientRect();
    return [r.left + r.width / 2, r.top + r.height / 2];
  };
  const press = (/** @type {string} */ k, /** @type {KeyboardEventInit} */ extra = {}) =>
    $('[data-df-stage]').dispatchEvent(new win.KeyboardEvent('keydown', {key: k, bubbles: true, cancelable: true, ...extra}));

  await test('Diffuse is the thirteenth mode, on ⌃\\, in the newer modes\' chrome, and lists the flows here', async () => {
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    const start = win.commands.length;
    let before = win.commands.length;
    doc.body.dispatchEvent(new win.KeyboardEvent('keydown', {key: '\\', code: 'Backslash', ctrlKey: true, bubbles: true, cancelable: true}));
    await settle();
    assert(sent(before, 'setMode').some((/** @type {any} */ c) => c.mode === 'diffuse'), '⌃\\ did not ask for Diffuse');
    await win.dispatch({type: 'setMode', mode: 'diffuse'}); await settle();
    const tab = $('.tb-modes [data-mode="diffuse"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃\\') && tab.textContent === 'Diffuse', 'the title bar does not show Diffuse on ⌃\\');
    assert(ws()?.classList.contains('on') && shown($('.titlebar')) && !shown($('.statusline')), 'Diffuse does not wear the newer modes\' chrome');
    assert(sent(start, 'diffuseLook').length === 1, `coming on screen looked at forge ${sent(start, 'diffuseLook').length} times, not once`);
    await win.pushEvents(core.sync()); await settle();
    const flows = [...ws().querySelectorAll('[data-df-open]')].map((b) => b.dataset.dfOpen);
    assert(flows.includes('lighthouse.flow.json'), `the flows here are ${flows}`);
    assert(shown($('[data-df-empty]')), 'with no flow open, nothing says so');
    assert($('[data-df-chip]').textContent.includes('RTX 4090') && $('[data-df-chip]').textContent.includes('24 GB'), `the chip says ${$('[data-df-chip]').textContent}`);
    before = win.commands.length;
    ws().querySelector('[data-df-open="lighthouse.flow.json"]').click(); await settle();
    assert(sent(before, 'diffuseOpen')[0]?.path === 'lighthouse.flow.json', 'a flow was not opened from the list');
    /** @type {HTMLInputElement} */ ($('[data-df-new-name]')).value = 'harbour';
    before = win.commands.length;
    $('[data-df-new] button[type="submit"]').click(); await settle();
    const made = sent(before, 'diffuseNew')[0];
    assert(made?.name === 'harbour' && made.template === 'textToImage', `New flow sent ${JSON.stringify(made)}`);
  });

  await test('A flow is drawn from the core\'s answer: its nodes, a wire for each link, and that it is ready to run', async () => {
    await answer({type: 'diffuseOpen', path: 'lighthouse.flow.json'});
    assert(ws().querySelectorAll('[data-df-node]').length === 7, 'not every node is drawn');
    assert(ws().querySelectorAll('.df-wire').length === 9, `${ws().querySelectorAll('.df-wire').length} wires for 9 links`);
    assert(ws().querySelector('.df-tab.on')?.textContent.includes('lighthouse.flow.json'), 'the flow is not the tab shown');
    assert($('[data-df-foot]').textContent.includes('ready to run'), `the foot says ${$('[data-df-foot]').textContent}`);
    // Each wire runs between the sockets it joins.
    const sampler = ws().querySelector('[data-df-in="5:model"]');
    assert(sampler?.classList.contains('linked'), 'a linked input is not drawn as one');
    assert(!/** @type {HTMLButtonElement} */ ($('[data-df-run]')).disabled, 'a flow ready to run, on a forge that serves, cannot be run');
  });

  await test('A wire pulled to a socket links them; let go on space, it asks for a node that takes it and adds it linked, as one edit', async () => {
    await answer({type: 'diffuseEdit', path: 'lighthouse.flow.json', ops: [{op: 'add', type: 'PreviewImage', at: [800, 320], values: {}}]});
    const stage = $('[data-df-stage]');
    let before = win.commands.length;
    const [ox, oy] = centre(ws().querySelector('[data-df-out="6:image"]'));
    const [ix, iy] = centre(ws().querySelector('[data-df-in="8:images"]'));
    fire(ws().querySelector('[data-df-out="6:image"]'), 'pointerdown', ox, oy);
    fire(stage, 'pointermove', ix, iy);
    fire(stage, 'pointerup', ix, iy);
    await settle();
    let edit = sent(before, 'diffuseEdit')[0];
    assert(JSON.stringify(edit?.ops) === JSON.stringify([{op: 'connect', from: '6:image', to: '8:images'}]), `pulled to a socket, it sent ${JSON.stringify(edit)}`);
    await answer(edit);
    // To empty space: the search, only what takes an image.
    const box = stage.getBoundingClientRect();
    before = win.commands.length;
    fire(ws().querySelector('[data-df-out="6:image"]'), 'pointerdown', ox, oy);
    fire(stage, 'pointermove', box.right - 60, box.bottom - 40);
    fire(stage, 'pointerup', box.right - 60, box.bottom - 40);
    await settle();
    const search = $('[data-df-search]');
    assert(shown(search) && search.textContent.includes('takes IMAGE'), 'letting go on space did not ask for a node that fits');
    const offered = [...search.querySelectorAll('[data-df-pick]')].map((b) => b.dataset.dfPick).sort();
    assert(JSON.stringify(offered) === JSON.stringify(['PreviewImage', 'SaveImage', 'UpscaleImage', 'VAEEncode']), `it offered ${offered}`);
    const input = search.querySelector('input');
    input.value = 'ups';
    input.dispatchEvent(new win.Event('input', {bubbles: true}));
    input.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true}));
    await settle();
    edit = sent(before, 'diffuseEdit')[0];
    assert(edit?.ops.length === 2 && edit.ops[0].op === 'add' && edit.ops[0].type === 'UpscaleImage' && JSON.stringify(edit.ops[1]) === JSON.stringify({op: 'connect', from: '6:image', to: '9:image'}),
      `the node picked was sent as ${JSON.stringify(edit)}`);
    assert(!shown(search), 'the search stayed open');
    await answer(edit);
    assert(JSON.stringify([...ws().querySelectorAll('.df-node.on')].map((n) => n.dataset.dfNode)) === '["9"]', 'the node added is not the one chosen');
  });

  await test('A node is moved by its head and taken away with ⌫, each one edit; ⌘Z asks for undo', async () => {
    const stage = $('[data-df-stage]');
    const head = ws().querySelector('[data-df-grab="9"]');
    const [hx, hy] = centre(head);
    let before = win.commands.length;
    fire(head, 'pointerdown', hx, hy);
    fire(stage, 'pointermove', hx - 40, hy - 30);
    fire(stage, 'pointerup', hx - 40, hy - 30);
    await settle();
    const place = sent(before, 'diffuseEdit')[0];
    assert(place?.ops.length === 1 && place.ops[0].op === 'place' && place.ops[0].id === '9', `a move sent ${JSON.stringify(place)}`);
    await answer(place);
    before = win.commands.length;
    press('Backspace'); await settle();
    const removed = sent(before, 'diffuseEdit')[0];
    assert(JSON.stringify(removed?.ops) === JSON.stringify([{op: 'remove', ids: ['9']}]), `⌫ sent ${JSON.stringify(removed)}`);
    await answer(removed);
    before = win.commands.length;
    press('z', {metaKey: true}); await settle();
    assert(sent(before, 'diffuseUndo').length === 1, '⌘Z did not ask for undo');
    await answer({type: 'diffuseUndo'});
    assert(ws().querySelector('[data-df-node="9"]'), 'undo did not bring the node back');
  });

  await test('The inspector sets a node\'s fields as their kinds allow, and its seed\'s way after a run', async () => {
    ws().querySelector('[data-df-grab="5"]').dispatchEvent(new win.PointerEvent('pointerdown', {bubbles: true, button: 0, clientX: 0, clientY: 0, pointerId: 42}));
    $('[data-df-stage]').dispatchEvent(new win.PointerEvent('pointerup', {bubbles: true, button: 0, clientX: 0, clientY: 0, pointerId: 42}));
    await settle();
    const steps = /** @type {HTMLInputElement} */ (ws().querySelector('[data-df-set="steps"]'));
    assert(steps && steps.type === 'number' && steps.max === '150' && steps.value === '30', 'steps is not a number field with its range');
    const sampler = /** @type {HTMLSelectElement} */ (ws().querySelector('[data-df-set="sampler"]'));
    assert(sampler?.tagName === 'SELECT' && [...sampler.options].some((o) => o.value === 'euler_a'), 'the sampler is not chosen from its list');
    let before = win.commands.length;
    steps.value = '40';
    steps.dispatchEvent(new win.Event('change', {bubbles: true}));
    await settle();
    assert(JSON.stringify(sent(before, 'diffuseEdit')[0]?.ops) === JSON.stringify([{op: 'set', id: '5', name: 'steps', value: 40}]), 'steps was not set as a number');
    before = win.commands.length;
    ws().querySelector('[data-df-seed="random"]').click(); await settle();
    assert(JSON.stringify(sent(before, 'diffuseEdit')[0]?.ops) === JSON.stringify([{op: 'seed', id: '5', mode: 'random'}]), 'the seed\'s way was not sent');
    assert(ws().querySelector('[data-df-inspector]').textContent.includes('Load Checkpoint 1'), 'the inputs do not say what feeds them');
  });

  await test('forge not serving: a banner says why and what to do, the Server tab what was checked, and Try again looks again', async () => {
    const known = core.sync()[1];
    await win.pushEvents([{...known, known: {...known.known, health: {state: 'offline', url: 'http://atlas:7420/img/v1', ms: 12, status: null, message: "forge isn't serving images: start it in Nexus › Inference"}, fresh: false}}]);
    await settle();
    const banner = $('[data-df-banner]');
    assert(shown(banner) && banner.textContent.includes('Nexus › Inference'), 'no banner says forge is off');
    assert($('[data-df-chip]').textContent.includes('not serving'), `the chip says ${$('[data-df-chip]').textContent}`);
    let before = win.commands.length;
    banner.querySelector('[data-df-look]').click(); await settle();
    assert(sent(before, 'diffuseLook')[0]?.again === true, 'Try again did not look again');
    banner.querySelector('[data-df-panel-go="server"]').click(); await settle();
    const server = $('[data-df-server]');
    assert(shown(server) && server.textContent.includes('atlas\'s gateway answers') && server.querySelector('[name="home"]')?.value === 'http://atlas:7420/img/v1', 'the Server tab does not say what was checked and where');
    assert(server.querySelector('.df-check.bad')?.textContent.includes('image server'), 'forge\'s server is not marked off');
    await win.pushEvents([known]); await settle();
    assert(!shown(banner), 'the banner stayed once forge served');
  });

  await test('A flow with changes is asked about before it closes', async () => {
    const tab = ws().querySelector('.df-tab.on');
    assert(tab?.querySelector('.df-dirty'), 'a flow with changes has no dot');
    tab.querySelector('[data-df-close]').click(); await settle();
    const ask = $('[data-df-ask]');
    assert(shown(ask) && ask.textContent.includes('has changes not saved'), 'closing a flow with changes did not ask');
    const before = win.commands.length;
    ask.querySelector('[data-df-answer="discard"]').click(); await settle();
    const closed = sent(before, 'diffuseClose')[0];
    assert(closed?.path === 'lighthouse.flow.json' && closed.discard === true, `closing sent ${JSON.stringify(closed)}`);
    assert(!shown(ask), 'the question stayed');
  });

  /** A PNG of one pixel: what a test's output reads as. */
  const onePixel = () => Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=='), (c) => c.charCodeAt(0)).buffer;
  const run = (/** @type {Record<string, any>} */ extra) => ({id: 1, run: 'r1', flow: 'lighthouse.flow.json', state: 'running', position: null, nodes: [], progress: null, preview: null,
    outputs: [], message: null, seconds: null, started: Date.now(), ...extra});

  await test('Run sends the flow, ⌘↩ too; while it runs the node at work is outlined with its step, the rest say how they stand, and Stop stops it', async () => {
    await answer({type: 'diffuseOpen', path: 'lighthouse.flow.json'});
    let before = win.commands.length;
    $('[data-df-run]').click(); await settle();
    assert(sent(before, 'diffuseRun')[0]?.path === 'lighthouse.flow.json', `Run did not send the flow: ${$('[data-df-run]').disabled ? 'disabled' : 'enabled'}, ${$('[data-df-run]').title}`);
    before = win.commands.length;
    press('Enter', {metaKey: true}); await settle();
    assert(sent(before, 'diffuseRun').length === 1, '⌘↩ did not run it');
    await win.pushEvents([{type: 'diffuseRuns', runs: [run({nodes: [{id: '1', state: 'cached', seconds: null}, {id: '5', state: 'running', seconds: null}],
      progress: {node: '5', step: 12, steps: 30}, preview: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=='})]}]);
    await settle();
    const sampler = ws().querySelector('[data-df-node="5"]');
    assert(sampler.classList.contains('working') && sampler.textContent.includes('step 12 / 30'), 'the sampler at work is not shown so');
    assert(sampler.querySelector('img.df-shot.blurred'), 'the preview is not on the sampler');
    assert(ws().querySelector('[data-df-node="1"] .run-cached') && ws().querySelector('[data-df-node="7"] .run-waiting'), 'the other nodes do not say how they stand');
    assert($('[data-df-run-label]').textContent === 'Queue' && !$('[data-df-count]').hidden, 'Run does not offer to queue another while one goes');
    before = win.commands.length;
    $('[data-df-stop]').click(); await settle();
    assert(sent(before, 'diffuseCancel')[0]?.id === 1, 'Stop did not cancel the run');
  });

  await test('A finished run\'s image is on its Save node and in the Queue, and the gallery lists the outputs with what made them', async () => {
    win.diffuseFiles = {'outputs/lighthouse/image_0001.png': onePixel()};
    await win.pushEvents([{type: 'diffuseRuns', runs: [run({state: 'done', seconds: 9.5, nodes: [{id: '5', state: 'done', seconds: 7.1}, {id: '7', state: 'done', seconds: 0.2}],
      outputs: [{node: '7', path: 'outputs/lighthouse/image_0001.png', width: 1024, height: 1024, kept: true}]})]}]);
    await settle();
    const img = ws().querySelector('[data-df-node="7"] img[data-df-img]');
    await new Promise((done) => setTimeout(done, 50));
    assert(img?.src.startsWith('data:image/png;base64,'), `the Save node's image is ${img?.src}`);
    assert(ws().querySelector('[data-df-node="5"] .run-done')?.textContent === 'done 7.1 s', 'the sampler does not say how long it took');
    ws().querySelector('[data-df-panel="queue"]').click(); await settle();
    assert($('[data-df-queue]').textContent.includes('done in 9.5 s') && $('[data-df-queue] [data-df-show-image]'), 'the Queue does not show the finished run and its image');
    let before = win.commands.length;
    ws().querySelector('[data-df-view="gallery"]').click(); await settle();
    assert(sent(before, 'diffuseGallery').length === 1, 'the gallery was not asked for');
    await win.pushEvents([{type: 'diffuseGallery', images: [{path: 'outputs/lighthouse/image_0001.png', folder: 'outputs/lighthouse', madeAt: 1792980000, width: 1024, height: 1024, graph: true,
      made: {seed: 42, steps: 30, cfg: 6.5, sampler: 'dpmpp_2m', scheduler: 'karras', denoise: 1, checkpoint: 'sd_xl_base_1.0.safetensors', prompt: 'a lighthouse at dusk', negative: 'blurry'}}]}]);
    await settle();
    assert(shown($('[data-df-gallery]')) && ws().querySelectorAll('.df-tile').length === 1, 'the gallery does not list the output');
    ws().querySelector('[data-df-panel="inspector"]').click(); await settle();
    const inspector = $('[data-df-inspector]');
    assert(inspector.textContent.includes('a lighthouse at dusk') && inspector.textContent.includes('42'), 'the chosen image does not say what made it');
    before = win.commands.length;
    inspector.querySelector('[data-df-open-flow]').click(); await settle();
    assert(sent(before, 'diffuseOpen')[0]?.path === 'lighthouse.flow.json' && !shown($('[data-df-gallery]')), 'Open its flow did not open it on the graph');
  });

  await test('The gateway\'s addresses are set from the Server tab, into the person\'s own settings', async () => {
    ws().querySelector('[data-df-panel="server"]').click(); await settle();
    const form = $('[data-df-gateways]');
    /** @type {HTMLInputElement} */ (form.querySelector('[name="home"]')).value = 'http://192.168.68.62:7420/img/v1';
    /** @type {HTMLInputElement} */ (form.querySelector('[name="away"]')).value = '';
    const before = win.commands.length;
    form.querySelector('button[type="submit"]').click(); await settle();
    const set = sent(before, 'diffuseGateways')[0];
    assert(set?.home === 'http://192.168.68.62:7420/img/v1' && set.away === undefined, `the addresses sent were ${JSON.stringify(set)}`);
  });

  await test('A flow and chosen images go to the Trash after asking; Diffuse says which folder it keeps them in, and moves it', async () => {
    ws().querySelector('[data-df-side="flows"]').click(); await settle();
    assert(ws().querySelector('.df-folder code')?.textContent === '~/Diffuse', `the folder shown is ${ws().querySelector('.df-folder code')?.textContent}`);
    ws().querySelector('[data-df-delete-flow="lighthouse.flow.json"]').click(); await settle();
    const ask = $('[data-df-ask]');
    assert(shown(ask) && ask.textContent.includes('Move lighthouse.flow.json to the Trash') && ask.textContent.includes('stay where they are'), 'deleting a flow did not ask');
    let before = win.commands.length;
    ask.querySelector('[data-df-answer="trash"]').click(); await settle();
    assert(sent(before, 'diffuseDelete')[0]?.path === 'lighthouse.flow.json', 'the flow was not sent to the Trash');
    // Two images chosen with ⌘-click, then ⌫.
    const image = (/** @type {string} */ p) => ({path: p, folder: 'outputs/lighthouse', madeAt: 1792980000, width: 64, height: 64, graph: true, made: {seed: 1}});
    win.diffuseFiles = {'outputs/lighthouse/image_0001.png': onePixel(), 'outputs/lighthouse/image_0002.png': onePixel()};
    ws().querySelector('[data-df-view="gallery"]').click(); await settle();
    await win.pushEvents([{type: 'diffuseGallery', images: [image('outputs/lighthouse/image_0002.png'), image('outputs/lighthouse/image_0001.png')]}]); await settle();
    const tile = (/** @type {number} */ n) => [...ws().querySelectorAll('[data-df-pick-image]')][n];
    tile(0).click(); await settle();
    // The gallery is drawn again after each choice: the tile is found afresh.
    tile(1).dispatchEvent(new win.MouseEvent('click', {bubbles: true, metaKey: true})); await settle();
    assert(ws().querySelectorAll('.df-tile.on').length === 2 && $('[data-df-inspector]').textContent.includes('2 images'), 'two images were not chosen');
    press('Backspace'); await settle();
    assert(shown(ask) && ask.textContent.includes('2 images'), '⌫ did not ask about the two images');
    before = win.commands.length;
    ask.querySelector('[data-df-answer="trash"]').click(); await settle();
    const gone = sent(before, 'diffuseDeleteImages')[0];
    assert(JSON.stringify(gone?.paths?.sort()) === JSON.stringify(['outputs/lighthouse/image_0001.png', 'outputs/lighthouse/image_0002.png']), `the images sent were ${JSON.stringify(gone)}`);
    // The folder, moved.
    ws().querySelector('[data-df-view="graph"]').click(); await settle();
    ws().querySelector('[data-df-folder-edit]').click(); await settle();
    const field = /** @type {HTMLInputElement} */ (ws().querySelector('[data-df-folder-form] input'));
    field.value = '~/Pictures/Diffuse';
    before = win.commands.length;
    ws().querySelector('[data-df-folder-form] button[type="submit"]').click(); await settle();
    assert(sent(before, 'diffuseFolder')[0]?.folder === '~/Pictures/Diffuse', 'the folder was not moved');
  });

  await test('A new flow starts from any of the starters, an output\'s graph opens as a new flow, and so does a PNG dropped on the graph', async () => {
    await win.pushEvents(core.sync()); await settle();
    ws().querySelector('[data-df-side="flows"]').click(); await settle();
    const starters = [...ws().querySelectorAll('[data-df-new] [name="template"]')].map((r) => r.value);
    assert(JSON.stringify(starters) === JSON.stringify(['textToImage', 'imageToImage', 'inpaint', 'hiresFix', 'flux', '']), `the starters offered are ${starters}`);
    /** @type {HTMLInputElement} */ ($('[data-df-new-name]')).value = 'sharp';
    /** @type {HTMLInputElement} */ (ws().querySelector('[data-df-new] [value="hiresFix"]')).checked = true;
    let before = win.commands.length;
    $('[data-df-new] button[type="submit"]').click(); await settle();
    assert(sent(before, 'diffuseNew')[0]?.template === 'hiresFix', `New sent ${JSON.stringify(sent(before, 'diffuseNew')[0])}`);
    // From the gallery.
    ws().querySelector('[data-df-view="gallery"]').click(); await settle();
    await win.pushEvents([{type: 'diffuseGallery', images: [{path: 'outputs/lighthouse/image_0003.png', folder: 'outputs/lighthouse', madeAt: 1792980000, width: 64, height: 64, graph: true, made: {seed: 7}}]}]);
    await settle();
    ws().querySelector('[data-df-pick-image]').click(); await settle();
    before = win.commands.length;
    $('[data-df-inspector] [data-df-open-image]').click(); await settle();
    assert(sent(before, 'diffuseOpenImage')[0]?.path === 'outputs/lighthouse/image_0003.png' && !shown($('[data-df-gallery]')), 'Open its graph did not ask for it on the graph');
    // Dropped: the window reads the graph out of the PNG itself.
    const enc = (/** @type {string} */ t) => new TextEncoder().encode(t);
    const chunk = (/** @type {string} */ kind, /** @type {Uint8Array} */ data) => {
      const out = new Uint8Array(12 + data.length);
      new DataView(out.buffer).setUint32(0, data.length);
      out.set([...kind].map((c) => c.charCodeAt(0)), 4);
      out.set(data, 8);
      return out;
    };
    const png = (/** @type {string | null} */ text) => {
      const parts = [Uint8Array.of(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a), chunk('IHDR', new Uint8Array(13)), ...(text ? [chunk('tEXt', enc(`proton-flow\0${text}`))] : []), chunk('IDAT', enc('x'))];
      return new Blob(parts, {type: 'image/png'});
    };
    const graph = {nodes: [{id: '1', type: 'SaveImage', values: {prefix: 'x'}}], links: []};
    const drop = (/** @type {File} */ file) => {
      const transfer = new win.DataTransfer();
      transfer.items.add(file);
      $('[data-df-stage]').dispatchEvent(new win.DragEvent('drop', {bubbles: true, cancelable: true, dataTransfer: transfer}));
    };
    before = win.commands.length;
    drop(new win.File([png(JSON.stringify(graph))], 'harbour.png', {type: 'image/png'}));
    await new Promise((done) => setTimeout(done, 100)); await settle();
    const opened = sent(before, 'diffuseOpenGraph')[0];
    assert(opened?.name === 'harbour.png' && JSON.stringify(opened.graph) === JSON.stringify(graph), `a dropped PNG sent ${JSON.stringify(opened)}`);
    drop(new win.File([png(null)], 'photo.png', {type: 'image/png'}));
    await new Promise((done) => setTimeout(done, 100)); await settle();
    assert(shown($('[data-df-ask]')) && $('[data-df-ask]').textContent.includes('photo.png holds no graph'), 'a PNG with no graph was not said so');
    $('[data-df-ask] [data-df-answer="stay"]').click(); await settle();
  });
}
