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
