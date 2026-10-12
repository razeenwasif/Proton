/* The PDF mode's checks, run by suite.js in the same fixture window: its
 * place on the rail and ⌃-, the Open box, tabs drawn from views the core
 * names by key, and a PDF opened anywhere becoming one of its tabs. */

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, key: Function, shown: (el: any) => boolean}} t */
export async function pdfChecks({test, assert, win, doc, $, settle, shown}) {
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const wait = async (/** @type {() => boolean} */ ready, tries = 60) => {
    for (let i = 0; i < tries && !ready(); i++) await settle();
    return ready();
  };
  const FIXTURE = '/tests/ui/fixtures/three-pages.pdf';
  const tabs = (/** @type {any[]} */ open, /** @type {number | null} */ active) =>
    ({type: 'pdfTabs', tabs: open, active, recent: ['~/Papers/old.pdf'], project: ['papers/a.pdf', 'notes/scan.pdf'], root: '/Users/you/project'});

  await test('PDF is the eleventh mode, on ⌃-: its own section, drawn as Notebook is, with an Open box first', async () => {
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    doc.body.focus();
    const before = win.commands.length;
    doc.body.dispatchEvent(new KeyboardEvent('keydown', {key: '-', code: 'Minus', ctrlKey: true, bubbles: true, cancelable: true})); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'pdf'), '⌃- did not ask for PDF');
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    assert($('.workspace.on')?.dataset.ws === 'pdf', 'PDF is not drawn in its own section');
    const tab = $('.titlebar [data-mode="pdf"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃-'), 'the title bar does not show PDF');
    assert(shown($('.titlebar')) && !shown($('.statusline')), 'PDF does not wear Notebook\'s chrome');
    await win.pushEvents([tabs([], null)]); await settle();
    const box = $('[data-pdf-open]');
    assert(shown(box), 'with nothing open, the Open box is not shown');
    const rows = [...box.querySelectorAll('.pdf-open-row')].map((r) => r.textContent);
    assert(rows.length === 3 && rows[0].includes('papers/a.pdf') && rows[2].includes('~/Papers/old.pdf'), `not the project's PDFs, then those opened before: ${rows}`);
    const input = box.querySelector('[data-pdf-open-path]');
    input.value = 'scan'; input.dispatchEvent(new Event('input', {bubbles: true})); await settle();
    assert([...box.querySelectorAll('.pdf-open-row')].map((r) => r.textContent.trim()).join('|').startsWith('notes/scan.pdf'), 'typing does not narrow the list');
    let at = win.commands.length;
    input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true})); await settle();
    assert(JSON.stringify(sent(at, 'pdfOpen')) === JSON.stringify([{type: 'pdfOpen', path: 'notes/scan.pdf'}]), `Enter did not open the one chosen: ${JSON.stringify(sent(at, 'pdfOpen'))}`);
    input.value = '~/Downloads/thing.pdf'; input.dispatchEvent(new Event('input', {bubbles: true})); await settle();
    at = win.commands.length;
    input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true})); await settle();
    assert(sent(at, 'pdfOpen')[0]?.path === '~/Downloads/thing.pdf', 'a typed path is not opened as typed');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('A tab is drawn from the view the core names, read again in place, and closed or brought forward', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const one = {id: 1, name: 'three-pages.pdf', path: '~/three-pages.pdf', dirty: false};
    const two = {id: 2, name: 'other.pdf', path: '~/other.pdf', dirty: true};
    await win.pushEvents([tabs([one, two], 1), {type: 'pdfView', tab: 1, key: FIXTURE, version: 1, page: 2, quiet: false}]); await settle();
    const pages = $('[data-pdf-pages]');
    assert(await wait(() => pages.querySelectorAll('.doc-page canvas').length > 0), 'the PDF was not drawn');
    assert(!shown($('[data-pdf-open]')) && shown(pages), 'the Open box still covers the pages');
    assert(pages.querySelectorAll('.doc-page').length === 3, 'not every page has a slot');
    assert(await wait(() => /^2 \/ 3/.test($('[data-pdf-page]').textContent)), `not at the page asked for: ${$('[data-pdf-page]').textContent}`);
    const strip = [...doc.querySelectorAll('.pdf-tab')];
    assert(strip.length === 2 && strip[0].classList.contains('on') && strip[1].querySelector('.dot'), 'the tabs, the one on top and the unsaved one, are not drawn');
    // A full bar wraps rather than squeezing the tabs out of sight.
    const ws = $('.workspace[data-ws="pdf"]');
    const wide = ws.style.getPropertyValue('--col-r');
    ws.style.setProperty('--col-r', '640px'); await settle();
    const squeezed = strip[0].getBoundingClientRect().width;
    const zoom = $('[data-pdf-zoom="1"]').getBoundingClientRect();
    ws.style.setProperty('--col-r', wide); await settle();
    assert(squeezed > 60 && zoom.width > 0 && zoom.right <= $('.pdf-bar').getBoundingClientRect().right + 1, `a narrow bar squeezed the tabs (${squeezed}px) or hid its zoom`);

    // A new version of the same tab is read again where it was.
    const top = pages.scrollTop;
    await win.pushEvents([{type: 'pdfView', tab: 1, key: FIXTURE + '?v=2', version: 2, page: null, quiet: false}]);
    await wait(() => false, 8);
    assert(Math.abs(pages.scrollTop - top) < 4, `the reload moved the page: ${top} → ${pages.scrollTop}`);

    // j scrolls the pages; the Open box's field types instead.
    pages.focus();
    const was = pages.scrollTop;
    pages.dispatchEvent(new KeyboardEvent('keydown', {key: 'j', bubbles: true, cancelable: true})); await settle();
    assert(pages.scrollTop > was, 'j did not scroll the PDF');

    let at = win.commands.length;
    strip[1].click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfFocus')) === JSON.stringify([{type: 'pdfFocus', tab: 2}]), 'a tab did not come forward');
    at = win.commands.length;
    strip[0].querySelector('[data-pdf-close]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfClose')) === JSON.stringify([{type: 'pdfClose', tab: 1}]) && !sent(at, 'pdfFocus').length, 'closing asked for the wrong thing');
    $('[data-pdf-open-box]').click(); await settle();
    assert(shown($('[data-pdf-open]')) && doc.activeElement === $('[data-pdf-open-path]'), '+ did not open the box');
    $('[data-pdf-open-path]').dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
    assert(!shown($('[data-pdf-open]')), 'Escape did not put the box away');
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('Reading: page labels, thumbnails and bookmarks', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const one = {id: 7, name: 'outline.pdf', path: '~/outline.pdf', dirty: false};
    await win.pushEvents([tabs([one], 7), {type: 'pdfView', tab: 7, key: '/tests/ui/fixtures/outline.pdf', version: 1, page: null, quiet: false}]); await settle();
    const pages = $('[data-pdf-pages]'), chip = () => $('[data-pdf-page]').textContent;
    assert(await wait(() => pages.querySelectorAll('.doc-page canvas').length > 0 && chip().startsWith('i · 1 / 3')), `not drawn with its label: ${chip()}`);

    // The pages small, labelled as the document labels them.
    $('[data-pdf-side="pages"]').click(); await settle();
    const thumbs = [...doc.querySelectorAll('.pdf-thumb')];
    assert(thumbs.map((t) => t.textContent.trim()).join(',') === 'i,1,2', `the thumbnails' labels: ${thumbs.map((t) => t.textContent)}`);
    assert(await wait(() => doc.querySelectorAll('.pdf-thumb canvas').length === 3), 'the thumbnails were not drawn');
    thumbs[2].click(); await settle();
    assert(await wait(() => chip().startsWith('2 · 3 / 3')), `a thumbnail did not go to its page: ${chip()}`);

    // Bookmarks, nested; one goes to its page.
    $('[data-pdf-side="outline"]').click(); await settle();
    const marks = [...doc.querySelectorAll('.pdf-mark-row')];
    assert(marks.map((m) => `${m.textContent}${m.classList.contains('top') ? '' : '>'}`).join(',') === 'Preface,Chapter one,Appendix>', `the bookmarks: ${marks.map((m) => m.textContent)}`);
    marks[0].click(); await settle();
    assert(await wait(() => chip().startsWith('i · 1 / 3')), `a bookmark did not go to its page: ${chip()}`);
  });

  await test('Reading: links over the pages, and search as you type, marked and stepped through', async () => {
    const pages = $('[data-pdf-pages]'), chip = () => $('[data-pdf-page]').textContent;
    // Links: into the document, gone to; out of it, asked for.
    const links = [...pages.querySelectorAll('.doc-page[data-page="1"] .pdf-link')];
    assert(links.length === 2, `page 1's links are not laid over it: ${links.length}`);
    const at = win.commands.length;
    links.find((l) => l.dataset.url).click(); await settle();
    assert(JSON.stringify(sent(at, 'openLink')) === JSON.stringify([{type: 'openLink', url: 'https://example.org/proton'}]), 'the web link was not opened');
    links.find((l) => l.dataset.dest).click();
    assert(await wait(() => chip().startsWith('2 · 3 / 3')), `the link into the document did not go: ${chip()}`);

    // ⌘F, typed, found and marked; ⌘G steps; Escape clears.
    pages.focus();
    pages.dispatchEvent(new KeyboardEvent('keydown', {key: 'f', metaKey: true, bubbles: true, cancelable: true})); await settle();
    const field = doc.querySelector('[data-pdf-find-query]');
    assert(field && doc.activeElement === field, '⌘F did not take the keyboard to the search');
    // "the" is on page i twice and page 2 once: three, in order.
    field.value = 'the'; field.dispatchEvent(new Event('input', {bubbles: true}));
    assert(await wait(() => doc.querySelectorAll('.pdf-hit-row').length === 3), `not found: ${doc.querySelector('.pdf-find-said')?.textContent}`);
    assert([...doc.querySelectorAll('.pdf-hit-page')].map((p) => p.textContent).join(',') === 'i,i,2', 'the hits are not each on its labelled page');
    assert(doc.activeElement === doc.querySelector('[data-pdf-find-query]') && doc.querySelector('[data-pdf-find-query]').value === 'the', 'the field lost what was typed');
    assert(await wait(() => pages.querySelector('.pdf-hit.on')), 'the hit is not marked on its page');
    field.dispatchEvent(new KeyboardEvent('keydown', {key: 'g', metaKey: true, bubbles: true, cancelable: true})); await settle();
    assert(doc.querySelectorAll('.pdf-hit-row')[1].classList.contains('on'), '⌘G did not go to the next');
    doc.querySelector('[data-pdf-find-query]').dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
    assert(!pages.querySelector('.pdf-hit'), 'Escape left the marks');
  });

  await test('Reading: two side by side, one at a time, and the view turned', async () => {
    const pages = $('[data-pdf-pages]');
    // Two side by side, one at a time (Space turns the page), and back.
    $('[data-pdf-layout="two"]').click(); await settle();
    assert(pages.dataset.layout === 'two', 'not two side by side');
    $('[data-pdf-layout="single"]').click(); await settle();
    const visible = () => [...pages.querySelectorAll('.doc-page')].filter((p) => !p.hidden).map((p) => p.dataset.page);
    assert(visible().length === 1, `not one at a time: ${visible()}`);
    const before = visible()[0];
    pages.focus();
    pages.dispatchEvent(new KeyboardEvent('keydown', {key: ' ', bubbles: true, cancelable: true})); await settle();
    assert(Number(visible()[0]) === Math.min(3, Number(before) + 1), `Space did not turn the page: ${before} → ${visible()}`);
    $('[data-pdf-layout="continuous"]').click(); await settle();
    assert(visible().length === 3, 'not back to one after another');

    // The view turned: page 1, taller than wide, is wider than tall.
    const first = () => pages.querySelector('.doc-page[data-page="1"]').getBoundingClientRect();
    assert(first().height > first().width, 'page 1 starts upright');
    pages.dispatchEvent(new KeyboardEvent('keydown', {key: 'r', bubbles: true, cancelable: true})); await settle();
    assert(first().width > first().height, 'r did not turn the view');
    for (let i = 0; i < 3; i++) { pages.dispatchEvent(new KeyboardEvent('keydown', {key: 'r', bubbles: true, cancelable: true})); await settle(); }
    assert(first().height > first().width, 'four turns are not a whole one');
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('Changes: undo, redo and save from the bar and the keys; a changed tab asks before closing; Save as; read only said', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const changed = {id: 3, name: 'paper.pdf', path: '~/Papers/paper.pdf', dirty: true, readOnly: null, undo: 'turn page 2', redo: null};
    await win.pushEvents([tabs([changed], 3), {type: 'pdfView', tab: 3, key: FIXTURE, version: 2, page: null, quiet: false}]); await settle();
    assert(await wait(() => $('[data-pdf-pages] .doc-page canvas')), 'not drawn');
    const undo = $('[data-pdf-undo]'), redo = $('[data-pdf-redo]'), save = $('[data-pdf-save]');
    assert(!undo.disabled && undo.title.includes('turn page 2') && redo.disabled && !save.disabled, 'the bar does not say what can be undone and saved');
    let at = win.commands.length;
    undo.click(); save.click(); await settle();
    assert(sent(at, 'pdfUndo').length === 1 && sent(at, 'pdfSave').length === 1, 'the bar did not ask');
    const pages = $('[data-pdf-pages]');
    pages.focus();
    at = win.commands.length;
    for (const [k, extra] of [['z', {metaKey: true}], ['z', {metaKey: true, shiftKey: true}], ['s', {metaKey: true}], ['u', {}], ['r', {ctrlKey: true}]]) {
      pages.dispatchEvent(new KeyboardEvent('keydown', {key: k, bubbles: true, cancelable: true, ...extra})); await settle();
    }
    const asked = win.commands.slice(at).map((c) => c.type).filter((t) => t.startsWith('pdf') && t !== 'pdfAt').join(',');
    assert(asked === 'pdfUndo,pdfRedo,pdfSave,pdfUndo,pdfRedo', `the keys asked: ${asked}`);

    // Closing a changed tab asks: save and close, or close letting them go.
    doc.querySelector('.pdf-tab [data-pdf-close]').click(); await settle();
    const ask = $('[data-pdf-ask]');
    assert(shown(ask) && ask.textContent.includes('paper.pdf'), 'closing a changed tab did not ask');
    at = win.commands.length;
    ask.querySelector('[data-pdf-ask-do="save"]').click(); await settle();
    assert(win.commands.slice(at).map((c) => c.type).join(',') === 'pdfSave,pdfClose' && !shown(ask), 'Save and close did not save, then close');
    doc.querySelector('.pdf-tab [data-pdf-close]').click(); await settle();
    at = win.commands.length;
    ask.querySelector('[data-pdf-ask-do="discard"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfClose')) === JSON.stringify([{type: 'pdfClose', tab: 3, discard: true}]), 'closing without saving did not say so');

    // ⇧⌘S: where, with the name chosen.
    pages.focus();
    pages.dispatchEvent(new KeyboardEvent('keydown', {key: 'S', metaKey: true, shiftKey: true, bubbles: true, cancelable: true})); await settle();
    const field = ask.querySelector('[data-pdf-save-as-path]');
    assert(field && doc.activeElement === field && field.value === '~/Papers/paper.pdf' && field.selectionStart === '~/Papers/'.length && field.selectionEnd === '~/Papers/paper'.length, 'Save as does not offer the name to change');
    field.value = '~/Papers/paper-marked.pdf';
    at = win.commands.length;
    ask.querySelector('[data-pdf-save-as-form]').requestSubmit(); await settle();
    assert(JSON.stringify(sent(at, 'pdfSaveAs')) === JSON.stringify([{type: 'pdfSaveAs', path: '~/Papers/paper-marked.pdf'}]) && !shown(ask), 'Save as did not ask');

    // A file only read says so, and offers nothing to save.
    await win.pushEvents([tabs([{...changed, dirty: false, undo: null, readOnly: 'it is protected by a password'}], 3)]); await settle();
    assert(shown($('[data-pdf-readonly]')) && $('[data-pdf-readonly]').title.includes('password') && $('[data-pdf-save]').disabled && $('[data-pdf-save-as]').disabled, 'read only is not said');
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('The page grid: pages chosen with click, ⌘ and ⇧, turned, deleted, a blank put after, and dragged into order', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const one = {id: 4, name: 'outline.pdf', path: '~/Papers/outline.pdf', dirty: false, readOnly: null, undo: null, redo: null};
    await win.pushEvents([tabs([one], 4), {type: 'pdfView', tab: 4, key: '/tests/ui/fixtures/outline.pdf', version: 1, page: null, quiet: false}]); await settle();
    assert(await wait(() => $('[data-pdf-pages] .doc-page canvas') && /\/ 3/.test($('[data-pdf-page]').textContent)), 'not drawn');
    $('[data-pdf-grid-toggle]').click(); await settle();
    const grid = $('[data-pdf-grid]');
    assert(shown(grid) && grid.querySelectorAll('.pdf-cell').length === 3, 'the grid does not show every page');
    const cell = (n) => grid.querySelector(`[data-pdf-cell="${n}"]`);
    const chosen = () => [...grid.querySelectorAll('.pdf-cell.on')].map((c) => c.dataset.pdfCell).join(',');
    assert(chosen() === '1', 'the page being read is not chosen');
    cell(3).click(); await settle();
    cell(1).dispatchEvent(new MouseEvent('click', {bubbles: true, metaKey: true})); await settle();
    assert(chosen() === '1,3', `⌘-click did not add: ${chosen()}`);
    cell(2).click(); cell(3).dispatchEvent(new MouseEvent('click', {bubbles: true, shiftKey: true})); await settle();
    assert(chosen() === '2,3', `⇧-click did not run from the last: ${chosen()}`);
    assert($('[data-pdf-which]').textContent === '2 pages', 'the tools do not say which pages they act on');

    let at = win.commands.length;
    grid.querySelector('[data-pdf-grid-do="right"]').click(); await settle();
    grid.querySelector('[data-pdf-grid-do="blank"]').click(); await settle();
    $('[data-pdf-tools] [data-pdf-tool="left"]').click(); await settle();
    const done = sent(at, 'pdfRotate').map((c) => `${c.pages}:${c.by}`).join(' ');
    assert(done === '2,3:90 2,3:-90', `turned: ${done}`);
    assert(JSON.stringify(sent(at, 'pdfInsertBlank')) === JSON.stringify([{type: 'pdfInsertBlank', after: 3, count: 1}]), 'no blank after');
    at = win.commands.length;
    grid.focus();
    grid.dispatchEvent(new KeyboardEvent('keydown', {key: 'Backspace', bubbles: true, cancelable: true})); await settle();
    assert(JSON.stringify(sent(at, 'pdfDelete')) === JSON.stringify([{type: 'pdfDelete', pages: [2, 3]}]), '⌫ did not delete the pages chosen');

    // Dragged: page 3 dropped on page 1's near half goes before it.
    cell(3).click(); await settle();
    const data = new DataTransfer();
    cell(3).dispatchEvent(new DragEvent('dragstart', {bubbles: true, dataTransfer: data}));
    const r = cell(1).getBoundingClientRect();
    cell(1).dispatchEvent(new DragEvent('dragover', {bubbles: true, cancelable: true, dataTransfer: data, clientX: r.left + 4, clientY: r.top + 10}));
    assert(cell(1).classList.contains('drop-before'), 'where it would go is not shown');
    at = win.commands.length;
    cell(1).dispatchEvent(new DragEvent('drop', {bubbles: true, cancelable: true, dataTransfer: data})); await settle();
    assert(JSON.stringify(sent(at, 'pdfMove')) === JSON.stringify([{type: 'pdfMove', pages: [3], before: 1}]), `not moved: ${JSON.stringify(sent(at, 'pdfMove'))}`);

    grid.dispatchEvent(new KeyboardEvent('keydown', {key: 'a', metaKey: true, bubbles: true, cancelable: true})); await settle();
    assert(chosen() === '1,2,3', '⌘A did not choose all');
    grid.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
    assert(!shown(grid), 'Escape did not go back to reading');
  });

  await test('The tools take pages out, split, insert from a file, and combine PDFs in an order chosen', async () => {
    const tools = $('[data-pdf-tools]');
    const field = (name) => tools.querySelector(`[data-pdf-typed="${name}"]`);
    const type = (name, value) => { field(name).value = value; field(name).dispatchEvent(new Event('input', {bubbles: true})); };
    assert(field('take').value === '~/Papers/outline-pages.pdf' && field('into').value === '~/Papers/outline-combined.pdf', 'the new files are not named beside it');
    let at = win.commands.length;
    tools.querySelector('[data-pdf-tool="take"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfExtract')) === JSON.stringify([{type: 'pdfExtract', pages: [1], path: '~/Papers/outline-pages.pdf'}]), `not taken out: ${JSON.stringify(sent(at, 'pdfExtract'))}`);
    type('every', '2');
    tools.querySelector('[data-pdf-tool="split"]').click(); await settle();
    type('every', ''); type('at', '2, 3');
    tools.querySelector('[data-pdf-tool="split"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfSplit').map((c) => [c.every, c.at])) === JSON.stringify([[2, []], [null, [2, 3]]]), `not split: ${JSON.stringify(sent(at, 'pdfSplit'))}`);
    type('insert', '~/Papers/more.pdf');
    tools.querySelector('[data-pdf-tool="insert"]').click(); await settle();
    assert(sent(at, 'pdfInsertFile')[0]?.path === '~/Papers/more.pdf', 'not inserted');

    // Combined: this one first, then two added, the last moved up.
    for (const path of ['~/a.pdf', '~/b.pdf']) {
      tools.querySelector('[data-pdf-typed="adding"]').value = path;
      tools.querySelector('[data-pdf-typed="adding"]').dispatchEvent(new Event('input', {bubbles: true}));
      tools.querySelector('[data-pdf-tool="add"]').click(); await settle();
    }
    tools.querySelector('[data-pdf-tool="up"][data-at="2"]').click(); await settle();
    at = win.commands.length;
    tools.querySelector('[data-pdf-tool="combine"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfCombine')) === JSON.stringify([{type: 'pdfCombine', paths: ['~/Papers/outline.pdf', '~/b.pdf', '~/a.pdf'], path: '~/Papers/outline-combined.pdf'}]), `not combined: ${JSON.stringify(sent(at, 'pdfCombine'))}`);
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('Comments are listed, ringed on their page, chosen, edited, recoloured, answered and deleted', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const one = {id: 5, name: 'outline.pdf', path: '~/outline.pdf', dirty: false, readOnly: null, undo: null, redo: null};
    await win.pushEvents([tabs([one], 5), {type: 'pdfView', tab: 5, key: '/tests/ui/fixtures/outline.pdf', version: 1, page: null, quiet: false}]); await settle();
    const pages = $('[data-pdf-pages]');
    assert(await wait(() => pages.querySelector('.doc-page[data-page="1"] canvas')), 'not drawn');
    const note = {id: '40 0', page: 1, kind: 'Text', rect: [200, 300, 224, 324], color: '#4fa3ff', text: 'Check this', author: 'Ada', date: 'D:20261012101500Z', replyTo: null};
    const answer = {id: '41 0', page: 1, kind: 'Text', rect: [200, 300, 224, 324], color: null, text: 'Done', author: 'Grace', date: 'D:20261012102000Z', replyTo: '40 0'};
    await win.pushEvents([{type: 'pdfComments', tab: 5, comments: [note, answer]}]); await settle();
    assert($('[data-pdf-side="comments"]').textContent === 'Comments 1', 'the tab does not count the comments');
    assert(pages.querySelector('.doc-page[data-page="1"] .pdf-comment-box[data-comment="40 0"]'), 'the comment is not ringed on its page');
    $('[data-pdf-side="comments"]').click(); await settle();
    const row = doc.querySelector('[data-pdf-comment="40 0"]');
    assert(row && row.textContent.includes('Check this') && row.textContent.includes('Grace: Done'), `the list: ${row?.textContent}`);
    row.click();
    assert(await wait(() => pages.querySelector('.pdf-pop')), 'choosing it did not open it');
    const pop = pages.querySelector('.pdf-pop');
    assert(pop.textContent.includes('Ada') && pop.textContent.includes('Grace') && pages.querySelector('.pdf-comment-box.on'), 'not shown as chosen, with its answer');
    pop.querySelector('[data-pdf-pop-text]').value = 'Check this again';
    pop.querySelector('[data-pdf-pop-reply]').value = 'Thanks';
    let at = win.commands.length;
    pop.querySelector('[data-pdf-swatch="#ff5a4f"]').click(); await settle();
    pop.querySelector('[data-pdf-pop="save"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfEditComment')) === JSON.stringify([{type: 'pdfEditComment', id: '40 0', color: '#ff5a4f'}, {type: 'pdfEditComment', id: '40 0', text: 'Check this again'}]), `not edited: ${JSON.stringify(sent(at, 'pdfEditComment'))}`);
    assert(JSON.stringify(sent(at, 'pdfReply')) === JSON.stringify([{type: 'pdfReply', id: '40 0', text: 'Thanks'}]), 'not answered');
    // Chosen by clicking on it on the page, and deleted with ⌫.
    const slot = pages.querySelector('.doc-page[data-page="1"]').getBoundingClientRect();
    const ring = pages.querySelector('.pdf-comment-box[data-comment="40 0"]').getBoundingClientRect();
    pages.dispatchEvent(new MouseEvent('click', {bubbles: true, clientX: ring.left + ring.width / 2, clientY: ring.top + ring.height / 2}));
    assert(await wait(() => pages.querySelector('.pdf-pop')) && slot.width > 0, 'a click on it did not choose it');
    pages.focus();
    at = win.commands.length;
    pages.dispatchEvent(new KeyboardEvent('keydown', {key: 'Backspace', bubbles: true, cancelable: true})); await settle();
    assert(JSON.stringify(sent(at, 'pdfDeleteComment')) === JSON.stringify([{type: 'pdfDeleteComment', id: '40 0'}]), 'not deleted');
  });

  await test('Comments are made: text marked from a selection, a note clicked in, a rectangle and ink drawn', async () => {
    const pages = $('[data-pdf-pages]');
    const slot = pages.querySelector('.doc-page[data-page="1"]');
    const span = [...slot.querySelectorAll('.textLayer span')].find((s) => s.textContent.includes('Preface'));
    assert(span, 'no text to select');
    const range = doc.createRange();
    range.selectNodeContents(span);
    win.getSelection().removeAllRanges(); win.getSelection().addRange(range);
    pages.dispatchEvent(new MouseEvent('mouseup', {bubbles: true}));
    assert(await wait(() => pages.querySelector('.pdf-selbar')), 'a selection is not offered a bar');
    let at = win.commands.length;
    pages.querySelector('[data-pdf-mark="highlight"]').click(); await settle();
    const marked = sent(at, 'pdfComment')[0];
    assert(marked?.page === 1 && marked.mark.kind === 'highlight' && marked.mark.boxes.length >= 1 && marked.mark.color === '#ffd400', `not highlighted: ${JSON.stringify(marked)}`);
    const [x1, y1, x2, y2] = marked.mark.boxes[0];
    assert(x2 > x1 && y2 > y1 && y1 > 300 && y2 < 380, `the box is not in the page's own points over the words: ${marked.mark.boxes[0]}`);

    // A note, clicked in, written, added.
    $('[data-pdf-tool-pick="note"]').click(); await settle();
    const r = slot.getBoundingClientRect();
    pages.dispatchEvent(new MouseEvent('click', {bubbles: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2}));
    const pop = await wait(() => pages.querySelector('.pdf-pop [data-pdf-pop-text]')) && pages.querySelector('.pdf-pop');
    assert(pop, 'no note to write');
    pop.querySelector('[data-pdf-pop-text]').value = 'A thought';
    at = win.commands.length;
    pop.querySelector('[data-pdf-pop="add"]').click(); await settle();
    const added = sent(at, 'pdfComment')[0];
    assert(added?.mark.kind === 'note' && added.mark.text === 'A thought' && Math.abs(added.mark.rect[0] - 150) < 2 && Math.abs(added.mark.rect[1] - 200) < 2, `not a note in the middle: ${JSON.stringify(added?.mark)}`);

    // A rectangle dragged, and ink drawn.
    const drag = async (tool, points) => {
      $(`[data-pdf-tool-pick="${tool}"]`).click(); await settle();
      const [first, ...rest] = points;
      pages.dispatchEvent(new PointerEvent('pointerdown', {bubbles: true, button: 0, pointerId: 1, clientX: r.left + first[0], clientY: r.top + first[1]}));
      for (const [x, y] of rest) pages.dispatchEvent(new PointerEvent('pointermove', {bubbles: true, pointerId: 1, clientX: r.left + x, clientY: r.top + y}));
      pages.dispatchEvent(new PointerEvent('pointerup', {bubbles: true, pointerId: 1})); await settle();
    };
    at = win.commands.length;
    await drag('square', [[20, 20], [120, 90]]);
    await drag('ink', [[30, 200], [50, 220], [70, 210], [90, 240]]);
    const made = sent(at, 'pdfComment');
    assert(made[0]?.mark.kind === 'square' && made[0].mark.rect.length === 4 && made[0].mark.rect[2] > made[0].mark.rect[0], `no rectangle: ${JSON.stringify(made[0]?.mark)}`);
    assert(made[1]?.mark.kind === 'ink' && made[1].mark.strokes[0].length === 4, `no ink: ${JSON.stringify(made[1]?.mark)}`);
    pages.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
    assert($('[data-pdf-tool-pick="select"]').getAttribute('aria-pressed') === 'true', 'Escape did not put the tool down');
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('A form: inputs over its fields, filled in as typed, boxes and choices set, highlighting, and flattened when agreed', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const one = {id: 6, name: 'form.pdf', path: '~/form.pdf', dirty: false, readOnly: null, undo: null, redo: null};
    await win.pushEvents([tabs([one], 6), {type: 'pdfView', tab: 6, key: '/tests/ui/fixtures/outline.pdf', version: 1, page: null, quiet: false}]); await settle();
    const pages = $('[data-pdf-pages]');
    assert(await wait(() => pages.querySelector('.doc-page[data-page="1"] canvas')), 'not drawn');
    const field = (name, kind, rects, extra = {}) => ({name, kind, value: '', options: [], widgets: rects.map(([rect, state]) => ({page: 1, rect, state: state ?? null})), readOnly: false, required: false, multiline: false, ...extra});
    await win.pushEvents([{type: 'pdfFields', tab: 6, fields: [
      field('name', 'text', [[[40, 200, 200, 220]]], {required: true}),
      field('agree', 'checkbox', [[[40, 170, 52, 182], 'Yes']]),
      field('size', 'radio', [[[40, 140, 52, 152], 'small'], [[80, 140, 92, 152], 'large']]),
      field('colour', 'combo', [[[40, 100, 140, 116]]], {options: ['Red', 'Blue']}),
      field('sign', 'signature', [[[40, 60, 140, 80]]]),
    ]}]); await settle();
    const page = pages.querySelector('.doc-page[data-page="1"]');
    const inputs = [...page.querySelectorAll('.pdf-field')];
    assert(inputs.length === 5, `not an input over each widget but the signature: ${inputs.length}`);
    assert(page.classList.contains('pdf-fields-lit') && page.querySelector('[data-name="name"]').classList.contains('required'), 'fields are not lit, the required one marked');
    const text = page.querySelector('input[data-name="name"]');
    const b = text.getBoundingClientRect(), slot = page.getBoundingClientRect();
    assert(Math.abs((b.left - slot.left) - 40 * slot.width / 300) < 3, 'the input is not where its widget is');

    let at = win.commands.length;
    text.focus(); text.value = 'Ada'; text.dispatchEvent(new Event('input', {bubbles: true}));
    text.blur(); await settle();
    page.querySelector('[data-name="agree"]').click(); await settle();
    page.querySelector('[data-name="size"][data-state="large"]').click(); await settle();
    const select = page.querySelector('select[data-name="colour"]');
    select.value = 'Blue'; select.dispatchEvent(new Event('change', {bubbles: true})); await settle();
    const set = sent(at, 'pdfSetField').map((c) => `${c.name}=${c.value}`).join(' ');
    assert(set === 'name=Ada agree=Yes size=large colour=Blue', `not filled in: ${set}`);

    // Highlighting off; flattening asks first.
    const tools = $('[data-pdf-tools]');
    tools.querySelector('[data-pdf-form-do="highlight"]').click(); await settle();
    assert(!page.classList.contains('pdf-fields-lit'), 'highlighting did not go off');
    tools.querySelector('[data-pdf-form-do="confirm"]').click(); await settle();
    at = win.commands.length;
    tools.querySelector('[data-pdf-form-do="flatten"]').click(); await settle();
    assert(sent(at, 'pdfFlatten').length === 1, 'Flatten did not ask');
    await win.pushEvents([{type: 'pdfFields', tab: 6, fields: []}]); await settle();
    assert(!page.querySelector('.pdf-field') && !tools.querySelector('[data-pdf-form-do]'), 'the form is still shown');
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('A protected PDF asks for its password; another is given one; its properties set, hidden data removed, and it compressed', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    await win.pushEvents([tabs([], null), {type: 'pdfPassword', path: '/Users/you/locked.pdf', name: 'locked.pdf', wrong: false}]); await settle();
    const lock = $('[data-pdf-locked]');
    assert(shown(lock) && lock.textContent.includes('locked.pdf') && !lock.querySelector('.pdf-wrong'), 'the password is not asked for');
    const field = lock.querySelector('[data-pdf-unlock-password]');
    assert(doc.activeElement === field && field.type === 'password', 'its field is not ready, or shows what is typed');
    field.value = 's3cret';
    let at = win.commands.length;
    lock.querySelector('[data-pdf-unlock]').requestSubmit(); await settle();
    assert(JSON.stringify(sent(at, 'pdfOpen')) === JSON.stringify([{type: 'pdfOpen', path: '/Users/you/locked.pdf', password: 's3cret'}]), 'not opened with it');
    assert(!shown(lock) && !doc.body.innerHTML.includes('s3cret'), 'the password was kept in the page');
    await win.pushEvents([{type: 'pdfPassword', path: '/Users/you/locked.pdf', name: 'locked.pdf', wrong: true}]); await settle();
    assert(lock.querySelector('.pdf-wrong'), 'a wrong one is not said');
    lock.querySelector('[data-pdf-unlock-cancel]').click(); await settle();

    const one = {id: 8, name: 'report.pdf', path: '~/report.pdf', dirty: false, readOnly: null, undo: null, redo: null, protected: false};
    await win.pushEvents([tabs([one], 8), {type: 'pdfView', tab: 8, key: FIXTURE, version: 1, page: null, quiet: false},
      {type: 'pdfInfo', tab: 8, protected: false, onDisk: 2_400_000, now: 2_400_000, info: {title: 'Draft', author: 'Ada', subject: '', keywords: '', creator: 'Word', producer: 'macOS', created: '', modified: '', version: '1.7', pages: 3, hidden: ['XMP metadata', 'JavaScript']}}]); await settle();
    const tools = $('[data-pdf-tools]');
    assert(tools.querySelector('[data-pdf-prop="title"]').value === 'Draft' && tools.textContent.includes('PDF 1.7 · 3 pages') && tools.textContent.includes('Made with Word, macOS'), 'the properties are not shown');
    tools.querySelector('[data-pdf-prop="title"]').value = 'Final';
    tools.querySelector('[data-pdf-prop="title"]').dispatchEvent(new Event('input', {bubbles: true}));
    at = win.commands.length;
    tools.querySelector('[data-pdf-tool="props"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfSetInfo')) === JSON.stringify([{type: 'pdfSetInfo', title: 'Final', author: 'Ada', subject: '', keywords: ''}]), 'not set');
    assert(tools.textContent.includes('XMP metadata, JavaScript') && tools.textContent.includes('On disk 2.3 MB'), 'what it hides, or its size, is not said');
    tools.querySelector('[data-pdf-tool="sanitize"]').click(); await settle();
    tools.querySelector('[data-pdf-tool="compress"]').click(); await settle();
    assert(sent(at, 'pdfSanitize').length === 1 && sent(at, 'pdfCompress').length === 1, 'not cleaned or compressed');

    // A password given: what it allows chosen.
    const user = tools.querySelector('[data-pdf-secret="user"]');
    user.value = 'open'; user.dispatchEvent(new Event('input', {bubbles: true}));
    const copy = tools.querySelector('[data-pdf-allow="copy"]');
    copy.checked = false; copy.dispatchEvent(new Event('change', {bubbles: true}));
    at = win.commands.length;
    tools.querySelector('[data-pdf-tool="protect"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfProtect')) === JSON.stringify([{type: 'pdfProtect', protection: {user: 'open', owner: '', print: true, copy: false, edit: false}}]), `not protected as chosen: ${JSON.stringify(sent(at, 'pdfProtect'))}`);
    await win.pushEvents([tabs([{...one, protected: true, dirty: true}], 8), {type: 'pdfInfo', tab: 8, protected: true, onDisk: 2_400_000, now: 900_000, info: {title: 'Final', author: '', subject: '', keywords: '', creator: '', producer: '', created: '', modified: '', version: '1.7', pages: 3, hidden: []}}]); await settle();
    assert(doc.querySelector('.pdf-tab .pdf-tab-lock') && tools.querySelector('[data-pdf-tool="unprotect"]') && tools.textContent.includes('as it would be saved 879 KB'), 'protected is not shown, or the size as it would be saved');
    at = win.commands.length;
    tools.querySelector('[data-pdf-tool="unprotect"]').click(); await settle();
    assert(sent(at, 'pdfUnprotect').length === 1, 'not unprotected');
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('OCR: a page with no text offers to be made searchable, in the languages chosen; the reading followed and stopped', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const one = {id: 9, name: 'scan.pdf', path: '~/scan.pdf', dirty: false, readOnly: null, undo: null, redo: null, protected: false};
    await win.pushEvents([tabs([one], 9), {type: 'pdfView', tab: 9, key: '/tests/ui/fixtures/scan.pdf', version: 1, page: null, quiet: false}]); await settle();
    const tools = $('[data-pdf-tools]');
    const offer = $('[data-pdf-ocr-offer]');
    assert(await wait(() => tools.textContent.includes('page 1 has no text')), `the page with no text is not found: ${tools.querySelector('[data-pdf-ocr]')?.textContent}`);
    assert(win.commands.filter((c) => c.type === 'pdfOcrLangs').length === 1, 'the languages are not asked for, once');
    assert(shown(offer) && offer.textContent === 'Make searchable', 'the page does not offer to be made searchable');
    assert(tools.firstElementChild?.matches('[data-pdf-ocr]'), 'OCR is not first, for a PDF with pages to read');
    await win.pushEvents([{type: 'pdfOcrLangs', langs: ['eng', 'fra', 'chi_sim']}]); await settle();
    const lang = (code) => tools.querySelector(`[data-pdf-ocr-lang="${code}"]`);
    assert(tools.textContent.includes('Chinese (simplified)') && lang('eng').checked && !lang('fra').checked, 'English is not chosen first among the languages');
    lang('fra').click(); await settle();
    assert(lang('fra').checked, 'French was not chosen');
    let at = win.commands.length;
    offer.click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfOcr')) === JSON.stringify([{type: 'pdfOcr', pages: [1], langs: ['eng', 'fra']}]), `the offer did not read the page: ${JSON.stringify(sent(at, 'pdfOcr'))}`);
    await win.pushEvents([{type: 'pdfOcr', tab: 9, state: 'running', done: 0, total: 1, said: ''}]); await settle();
    assert(offer.textContent === 'OCR 1/1' && tools.querySelector('[data-pdf-ocr-progress]') && tools.textContent.includes('Reading page 1 of 1'), 'the reading is not followed');
    at = win.commands.length;
    tools.querySelector('[data-pdf-ocr-do="stop"]').click(); await settle();
    assert(sent(at, 'pdfOcrStop').length === 1, 'Stop did not stop it');
    await win.pushEvents([{type: 'pdfOcr', tab: 9, state: 'stopped', done: 0, total: 1, said: ''}]); await settle();
    assert(tools.textContent.includes('Stopped: nothing was changed.') && offer.textContent === 'Make searchable', 'stopping is not said, or the offer not made again');

    // Every page, from the tools.
    tools.querySelector('[data-pdf-ocr-which="all"]').click(); await settle();
    at = win.commands.length;
    tools.querySelector('[data-pdf-ocr-do="start"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfOcr')) === JSON.stringify([{type: 'pdfOcr', pages: [1, 2], langs: ['eng', 'fra']}]), `not every page: ${JSON.stringify(sent(at, 'pdfOcr'))}`);
    await win.pushEvents([{type: 'pdfOcr', tab: 9, state: 'done', done: 2, total: 2, said: ''}]); await settle();
    assert(tools.textContent.includes('Read 2 pages'), 'what was read is not said');
    // Without tesseract, what it would need.
    await win.pushEvents([{type: 'pdfOcrLangs', langs: []}]); await settle();
    assert(tools.textContent.includes('brew install tesseract poppler') && offer.textContent === 'No text', 'what OCR needs is not said');
    await win.pushEvents([tabs([], null)]); await settle();
    assert(!shown(offer), 'the offer outlived its PDF');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('The agent sits beside the tools: the page being read is told to the core, @pdf asks about it, and ⌘Z in its box is the box\'s', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const one = {id: 10, name: 'outline.pdf', path: '~/outline.pdf', dirty: true, readOnly: null, undo: 'turn page 1', redo: null, protected: false};
    let at = win.commands.length;
    await win.pushEvents([tabs([one], 10), {type: 'pdfView', tab: 10, key: '/tests/ui/fixtures/outline.pdf', version: 1, page: 2, quiet: false}]); await settle();
    assert(await wait(() => sent(at, 'pdfAt').some((c) => c.tab === 10 && c.page === 2), 120), `the page being read was not told: ${JSON.stringify(sent(at, 'pdfAt'))}`);

    // A dot while the agent works out of sight; gone once it is looked at.
    const pane = $('#pdf-pane');
    const agent = (activity) => win.pushEvents([{type: 'agent', provider: 'claude', model: 'claude-opus-5-5', credential: '', activity, inputTokens: 0, outputTokens: 0, cachedTokens: 0,
      transcript: [{speaker: 'you', text: 'what is on this page? @pdf', thinking: '', streaming: false, calls: []},
        {speaker: 'proton', text: 'Chapter one begins here.', thinking: '', streaming: activity !== 'idle', calls: [{id: 'c1', name: 'pdf_read', detail: '', result: '--- page 2 of 3 ---', state: 'done'}]}]}]);
    await agent('thinking'); await settle();
    assert(!pane.querySelector('[data-pdf-agent-dot]').hidden, 'no dot on Agent while it works');
    pane.querySelector('[data-pdf-panel="agent"]').click(); await settle();
    assert(pane.dataset.showing === 'agent' && pane.querySelector('[data-pdf-agent-dot]').hidden && !shown(pane.querySelector('[data-pdf-tools]')), 'the agent is not shown in place of the tools');
    assert(pane.querySelector('[data-agent-thread]').textContent.includes('Chapter one begins here.'), 'the conversation is not drawn here');
    await agent('idle'); await settle();

    // @pdf written by its chip, and asked.
    const input = pane.querySelector('[data-agent-input]');
    assert(doc.activeElement === input, 'the box is not ready to type in');
    input.textContent = 'summarise';
    pane.querySelector('[data-mention="@pdf"]').click(); await settle();
    assert(input.textContent === 'summarise @pdf', `the chip did not write @pdf: ${input.textContent}`);
    // ⌘Z in the box undoes typing, not the PDF's last change.
    at = win.commands.length;
    input.dispatchEvent(new KeyboardEvent('keydown', {key: 'z', metaKey: true, bubbles: true, cancelable: true})); await settle();
    assert(!sent(at, 'pdfUndo').length, '⌘Z in the agent\'s box undid a change to the PDF');
    input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true})); await settle();
    assert(JSON.stringify(sent(at, 'ask')) === JSON.stringify([{type: 'ask', text: 'summarise @pdf'}]), `not asked: ${JSON.stringify(sent(at, 'ask'))}`);
    pane.querySelector('[data-pdf-panel="tools"]').click(); await settle();
    assert(pane.dataset.showing === 'tools' && shown(pane.querySelector('[data-pdf-tools]')), 'the tools did not come back');
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('? lists the PDF\'s keys where which-key lists the leader\'s, the next key puts them away, and g p opens the pages', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    const one = {id: 11, name: 'three-pages.pdf', path: '~/three-pages.pdf', dirty: false, readOnly: null, undo: null, redo: null, protected: false};
    await win.pushEvents([tabs([one], 11), {type: 'pdfView', tab: 11, key: FIXTURE, version: 1, page: null, quiet: false}]); await settle();
    const pages = $('[data-pdf-pages]');
    assert(await wait(() => pages.querySelector('.doc-page canvas')), 'not drawn');
    const press = (k) => pages.dispatchEvent(new KeyboardEvent('keydown', {key: k, bubbles: true, cancelable: true}));
    pages.focus();
    press('?'); await settle();
    const wk = $('#whichkey');
    assert(shown(wk) && wk.classList.contains('keys') && wk.textContent.includes('turn the view') && wk.textContent.includes('g p'), 'the keys are not shown');
    await wait(() => false, 3);
    const top = pages.scrollTop;
    press('j'); await settle();
    assert(!shown(wk) && pages.scrollTop === top, 'the next key did not put them away, or did something besides');
    press('g'); press('p'); await settle();
    assert(shown($('[data-pdf-grid]')), 'g p did not open the pages');
    press('g'); press('p'); await settle();
    assert(!shown($('[data-pdf-grid]')), 'g p did not put the pages away');
    await win.pushEvents([tabs([], null)]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('The library shelves your books by folder, the ones being read first where they were left, covers as they come, narrowed by what is typed', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    await win.pushEvents([tabs([], null)]); await settle();
    const book = (path, title, more = {}) => ({path, file: path.split('/').pop(), title, author: '', pages: 300, size: 1, shelf: '', folder: '~/Documents/Books', inCloud: false, page: null, read: null, ...more});
    const books = [
      book('~/Documents/Books/ed3book.pdf', 'Speech and Language Processing', {author: 'Daniel Jurafsky'}),
      book('~/Documents/Books/Machine Learning/mml-book.pdf', 'Mathematics for Machine Learning', {shelf: 'Machine Learning', page: 120, pages: 400, read: 1_790_000_100}),
      book('~/Documents/Books/Machine Learning/bishop.pdf', 'Pattern Recognition', {shelf: 'Machine Learning', author: 'Christopher M. Bishop'}),
      book('~/Documents/Books/Computer Vision/mvg.pdf', 'Multiple View Geometry', {shelf: 'Computer Vision', inCloud: true}),
    ];
    let at = win.commands.length;
    await win.pushEvents([{type: 'pdfLibrary', folders: ['~/Documents/Books'], books, walking: false, missing: []}]); await settle();
    const lib = $('[data-pdf-library]');
    const shelves = [...lib.querySelectorAll('.pdf-shelf h4')].map((h) => h.firstChild.textContent.trim());
    assert(JSON.stringify(shelves) === JSON.stringify(['Continue reading', 'Books', 'Computer Vision', 'Machine Learning']), `shelves: ${shelves}`);
    const reading = lib.querySelector('.pdf-shelf .pdf-book');
    assert(reading.dataset.pdfBook.endsWith('mml-book.pdf') && reading.textContent.includes('page 120 of 400') && reading.querySelector('.pdf-book-bar i').style.width === '30%', 'not continued where it was left');
    assert(!shown($('[data-pdf-open-list]')) && $('[data-pdf-open] h3').textContent === 'Library', 'the library is not the home page');
    const ws = $('.workspace[data-ws="pdf"]');
    assert(ws.classList.contains('pdf-home') && !shown(ws.querySelector('.pdf-tools')) && !shown(ws.querySelector('.pdf-side')) && !shown(ws.querySelector('.pdf-bar')),
      'with nothing open, the library is not the whole page');
    assert(lib.querySelector('[data-pdf-book$="mvg.pdf"] .pdf-cover-cloud'), 'a book in iCloud is not said to be');

    // Covers asked for as they come on screen, once each; drawn, they take the titles' place.
    assert(await wait(() => sent(at, 'pdfCovers').length > 0, 60), 'no covers asked for');
    const asked = sent(at, 'pdfCovers').flatMap((c) => c.paths);
    assert(asked.includes('~/Documents/Books/ed3book.pdf') && new Set(asked).size === asked.length, `covers asked: ${asked}`);
    const pixel = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
    await win.pushEvents([{type: 'pdfCovers', covers: [{path: '~/Documents/Books/ed3book.pdf', data: pixel}, {path: '~/Documents/Books/Machine Learning/bishop.pdf', data: null}]}]); await settle();
    assert(lib.querySelector('[data-pdf-cover="~/Documents/Books/ed3book.pdf"] img')?.getAttribute('src') === pixel, 'the cover was not drawn');
    assert(lib.querySelector('[data-pdf-cover$="bishop.pdf"] .pdf-cover-title')?.textContent === 'Pattern Recognition', 'a book with no cover lost its title');

    // What is typed narrows the shelves; Enter opens the first book it names.
    const input = $('[data-pdf-open-path]');
    input.value = 'bishop'; input.dispatchEvent(new Event('input', {bubbles: true})); await settle();
    const visible = () => [...lib.querySelectorAll('.pdf-book')].filter((b) => b.checkVisibility()).map((b) => b.dataset.pdfBook);
    const covers = [...lib.querySelectorAll('.pdf-cover')];
    const narrowed = visible();
    assert(narrowed.length === 1 && narrowed[0].endsWith('bishop.pdf'), `not narrowed: ${narrowed}`);
    assert(covers.every((c) => c.isConnected), 'the shelves were drawn again for a key typed');
    at = win.commands.length;
    input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true})); await settle();
    assert(sent(at, 'pdfOpen')[0]?.path === '~/Documents/Books/Machine Learning/bishop.pdf', `Enter did not open the book: ${JSON.stringify(sent(at, 'pdfOpen'))}`);
    assert(visible().length === 5, 'opening a book left the shelves narrowed');
    // A book clicked opens: the core opens it where it was left.
    at = win.commands.length;
    lib.querySelector('[data-pdf-book$="mml-book.pdf"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfOpen')) === JSON.stringify([{type: 'pdfOpen', path: '~/Documents/Books/Machine Learning/mml-book.pdf'}]), `not opened: ${JSON.stringify(sent(at, 'pdfOpen'))}`);

    // Renamed in the library only: ✎ opens its form, Save sends the name and draws it at once.
    const bishop = '~/Documents/Books/Machine Learning/bishop.pdf';
    lib.querySelector(`[data-pdf-rename="${bishop}"]`).click(); await settle();
    let form = lib.querySelector('[data-pdf-rename-form]');
    const titleField = form?.querySelector('[name=title]');
    assert(form?.dataset.path === bishop && doc.activeElement === titleField && titleField.value === 'Pattern Recognition', 'its form is not ready to type in');
    assert(!form.querySelector('[data-pdf-rename-own]'), 'a book never renamed offers its own title back');
    titleField.value = 'PRML'; titleField.dispatchEvent(new Event('input', {bubbles: true}));
    form.querySelector('[name=author]').value = 'Bishop';
    at = win.commands.length;
    form.requestSubmit(); await settle();
    assert(JSON.stringify(sent(at, 'pdfRenameBook')) === JSON.stringify([{type: 'pdfRenameBook', path: bishop, title: 'PRML', author: 'Bishop'}]), `not renamed: ${JSON.stringify(sent(at, 'pdfRenameBook'))}`);
    assert(!lib.querySelector('[data-pdf-rename-form]') && lib.querySelector(`[data-pdf-book="${bishop}"] .pdf-book-title`).textContent === 'PRML', 'the new name is not drawn');
    assert(doc.activeElement === lib.querySelector(`[data-pdf-book="${bishop}"]`), 'the book did not get the keyboard back');
    // F2 on a book renames it; Escape leaves it as it was.
    const ed3 = lib.querySelector('[data-pdf-book="~/Documents/Books/ed3book.pdf"]');
    ed3.focus();
    ed3.dispatchEvent(new KeyboardEvent('keydown', {key: 'F2', bubbles: true, cancelable: true})); await settle();
    form = lib.querySelector('[data-pdf-rename-form]');
    assert(form?.dataset.path === '~/Documents/Books/ed3book.pdf', 'F2 did not rename it');
    form.querySelector('[name=title]').dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
    assert(!lib.querySelector('[data-pdf-rename-form]') && shown($('[data-pdf-open]')), 'Escape did not put the form away, or put the library away too');
    // Renamed, it can have its own title back.
    await win.pushEvents([{type: 'pdfLibrary', folders: ['~/Documents/Books'], walking: false, missing: [],
      books: books.map((b) => (b.path === bishop ? {...b, title: 'PRML', author: 'Bishop', renamed: true} : {...b, renamed: false}))}]); await settle();
    lib.querySelector(`[data-pdf-rename="${bishop}"]`).click(); await settle();
    at = win.commands.length;
    lib.querySelector('[data-pdf-rename-own]').click(); await settle();
    assert(JSON.stringify(sent(at, 'pdfRenameBook')[0]) === JSON.stringify({type: 'pdfRenameBook', path: bishop, title: '', author: ''}), 'its own title was not asked for');

    // The folders, changed; looked through again.
    $('[data-pdf-lib-edit]').click(); await settle();
    const folders = lib.querySelector('[data-pdf-lib-folders-form]');
    folders.querySelector('textarea').value = '~/Documents/Books\n~/Papers';
    at = win.commands.length;
    folders.requestSubmit(); await settle();
    assert(JSON.stringify(sent(at, 'pdfLibraryFolders')[0]?.folders) === '["~/Documents/Books","~/Papers"]', `not set: ${JSON.stringify(sent(at, 'pdfLibraryFolders'))}`);
    $('[data-pdf-lib-scan]').click(); await settle();
    assert(sent(at, 'pdfLibraryScan').length === 1, 'not looked through again');
    // With no folders, it asks for them.
    await win.pushEvents([{type: 'pdfLibrary', folders: [], books: [], walking: false, missing: []}]); await settle();
    assert(lib.querySelector('[data-pdf-lib-folders-form]') && shown($('[data-pdf-open-list]')), 'no folders, and nothing asking for them');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('A PDF opened in the PDF mode is a tab, and the document viewer hands its PDF over at its page', async () => {
    await win.dispatch({type: 'setMode', mode: 'pdf'}); await settle();
    let at = win.commands.length;
    await win.pushEvents([{type: 'document', path: '/Users/you/paper.pdf', name: 'paper.pdf', kind: 'pdf', page: 4}]); await settle();
    assert(JSON.stringify(sent(at, 'pdfOpen')) === JSON.stringify([{type: 'pdfOpen', path: '/Users/you/paper.pdf', page: 4}]), 'not opened as a tab');
    assert(!shown($('#doc-viewer')), 'the viewer opened over the PDF mode');

    await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
    await win.pushEvents([{type: 'document', path: FIXTURE, name: 'three-pages.pdf', kind: 'pdf', page: 2}]); await settle();
    assert(await wait(() => $('#doc-viewer [data-doc-at]')?.textContent.startsWith('page 2')), 'the viewer did not open');
    at = win.commands.length;
    $('#doc-viewer [data-doc-in-pdf]').click(); await settle();
    assert(sent(at, 'setMode').some((c) => c.mode === 'pdf'), 'Open in PDF did not go to the PDF mode');
    assert(JSON.stringify(sent(at, 'pdfOpen')) === JSON.stringify([{type: 'pdfOpen', path: FIXTURE, page: 2}]), `not handed over at its page: ${JSON.stringify(sent(at, 'pdfOpen'))}`);
    assert(!shown($('#doc-viewer')), 'the viewer stayed open');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
