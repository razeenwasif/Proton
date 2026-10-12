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
    const asked = win.commands.slice(at).map((c) => c.type).filter((t) => t.startsWith('pdf')).join(',');
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
