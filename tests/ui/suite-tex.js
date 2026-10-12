/* LaTeX's checks, run by suite.js in the same fixture window: its place on
 * the rail, Codex's editor with LaTeX's own dock beside it, and the dock's
 * tabs. */

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, key: Function, shown: (el: any) => boolean}} t */
export async function texChecks({test, assert, win, doc, $, settle, key, shown}) {
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);

  await test('LaTeX is the tenth mode, on ⌃0: Codex\'s editor with its own dock in place of Codex\'s', async () => {
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    doc.body.focus();
    const before = win.commands.length;
    key('0', {ctrlKey: true}); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'tex'), '⌃0 did not ask for LaTeX');
    const section = $('.workspace.on');
    assert(section?.dataset.ws === 'codex' && section.dataset.dock === 'typeset', 'LaTeX is not drawn in Codex\'s section with its own dock');
    assert(shown($('#tex-pane')) && !shown($('#codex-dock')), 'the dock shown is not LaTeX\'s');
    assert(shown(section.querySelector('#codex-splits')), 'the editor is not beside it');
    const tab = $('.titlebar [data-mode="tex"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃0') && tab.textContent.trim() === 'LaTeX', 'the title bar does not show LaTeX');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'LaTeX does not wear Codex\'s chrome');
    assert($('[data-tex-pdf] .tex-empty'), 'the PDF tab says nothing before there is a PDF');
    // Its width is its own: the PDF wants more of the window than the agent does.
    const pane = $('#tex-pane').getBoundingClientRect().width;
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    assert(shown($('#codex-dock')) && !shown($('#tex-pane')), 'Codex did not get its dock back');
    assert($('.workspace.on').dataset.dock === 'panels', 'Codex\'s section kept LaTeX\'s dock');
    assert(pane > $('#codex-dock').getBoundingClientRect().width + 40, `the PDF pane is no wider than Codex's dock: ${pane}`);
  });

  await test('TeX is installed only after two say-sos: a plan of what it downloads, then Install; and it can be taken away', async () => {
    await win.dispatch({type: 'setMode', mode: 'tex'}); await settle();
    const card = () => $('#tex-pane [data-tex-pdf]');
    await win.pushEvents([{type: 'texEnv', env: {state: 'missing', needs: [], free: 120_000_000_000, home: '/Users/you/.proton/tex'}}]); await settle();
    assert(card().textContent.includes('TinyTeX') && card().textContent.includes('~/.proton/tex'), 'the card does not say what TeX is or where it goes');
    const bundles = [...card().querySelectorAll('[data-tex-bundle]')].map((b) => [b.value, b.checked]);
    assert(JSON.stringify(bundles) === JSON.stringify([['full', true], ['small', false]]), `the bundles offered: ${JSON.stringify(bundles)}`);
    const small = card().querySelector('[data-tex-bundle][value="small"]');
    small.checked = true;
    small.dispatchEvent(new win.Event('change', {bubbles: true}));
    let at = win.commands.length;
    card().querySelector('[data-tex-env="plan"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'texPlan')) === JSON.stringify([{type: 'texPlan', bundle: 'small'}]), `not planned as chosen: ${JSON.stringify(sent(at, 'texPlan'))}`);
    assert(!sent(at, 'texCreate').length, 'installed without the second say-so');

    const plan = {bundle: 'small', tinytex: 'v2026.10', texlab: 'v5.26.0', home: '/Users/you/.proton/tex', free: 120_000_000_000, fits: true,
      downloads: [{what: 'TinyTeX-1 v2026.10', host: 'github.com', bytes: 67_200_000, exact: true, verified: true},
        {what: 'latexmk, by TeX\'s package manager', host: 'ctan.org', bytes: 1_000_000, exact: false, verified: false},
        {what: 'texlab v5.26.0', host: 'github.com', bytes: 10_200_000, exact: true, verified: true}],
      download: 78_400_000, disk: 400_000_000, kept: 260_000_000, tinytexUrl: '', tinytexSha256: null, texlabUrl: '', texlabSha256: null};
    await win.pushEvents([{type: 'texEnv', env: {state: 'planned', plan}}]); await settle();
    assert(card().querySelectorAll('tr').length === 3 && card().textContent.includes('about'), 'the plan does not say what it downloads, or which sizes are estimates');
    at = win.commands.length;
    card().querySelector('[data-tex-env="create"]').click(); await settle();
    assert(sent(at, 'texCreate').length === 1, 'Install did not ask for the install');

    await win.pushEvents([{type: 'texEnv', env: {state: 'making', stage: 'texlab', done: 0, total: 0, log: ['$ curl texlab']}}]); await settle();
    const stages = [...card().querySelectorAll('.lean-stages li')].map((li) => li.className);
    assert(JSON.stringify(stages) === JSON.stringify(['done', 'done', 'on', '', '']), `the stages: ${stages}`);
    await win.pushEvents([{type: 'texEnv', env: {state: 'failed', step: 'texlab', why: 'curl stopped with exit status 22', log: []}}]); await settle();
    assert(card().textContent.includes('Installing texlab failed') && card().querySelector('[data-tex-env="plan"]'), 'a failure does not say what failed, or offer to try again');

    await win.pushEvents([{type: 'texEnv', env: {state: 'ready', tinytex: 'v2026.10', texlab: 'v5.26.0', bundle: 'full', size: 812_000_000, home: '/Users/you/.proton/tex'}}]); await settle();
    assert(card().querySelector('.tex-empty') && card().textContent.includes('TinyTeX v2026.10'), 'installed TeX is not said beside the empty page');
    card().querySelector('[data-tex-env="confirm"]').click(); await settle();
    at = win.commands.length;
    card().querySelector('[data-tex-env="remove"]').click(); await settle();
    assert(sent(at, 'texRemove').length === 1, 'Remove did not ask');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('A build is said in the pane\'s header, its log read into errors a click from their line, and a missing package found and installed', async () => {
    await win.dispatch({type: 'setMode', mode: 'tex'}); await settle();
    const pane = $('#tex-pane');
    await win.pushEvents([{type: 'texEnv', env: {state: 'ready', tinytex: 'v2026.10', texlab: 'v5.26.0', bundle: 'full', size: 812_000_000, home: '/Users/you/.proton/tex'}}]); await settle();
    let at = win.commands.length;
    pane.querySelector('[data-tex-compile]').click(); await settle();
    assert(sent(at, 'texCompile').length === 1, 'the play button did not typeset');
    const build = (more) => ({type: 'texBuild', build: {state: 'running', main: 'main.tex', engine: 'pdfLaTeX', seconds: 0, pdf: null, version: 0, errors: 0, warnings: 0, boxes: 0, pages: null, said: null, ...more}});
    await win.pushEvents([build({})]); await settle();
    assert(pane.querySelector('[data-tex-build]').textContent.includes('Typesetting') && pane.querySelector('[data-tex-stop]'), 'a build under way is not said, or cannot be stopped');
    at = win.commands.length;
    pane.querySelector('[data-tex-stop]').click(); await settle();
    assert(sent(at, 'texStop').length === 1, 'Stop did not ask');

    const entry = (kind, file, line, message, context = null) => ({kind, file, line, message, context});
    await win.pushEvents([build({state: 'failed', errors: 1, seconds: 1.4}), {type: 'texLog', root: '/Users/you/paper', log: {pages: null, missing: ['tikz-cd.sty'], entries: [
      entry('error', 'chapters/intro.tex', 12, 'Undefined control sequence.', 'We use \\foo'),
      entry('warning', 'main.tex', 31, "Reference `fig:loop' on page 3 undefined on input line 31."),
      entry('box', 'chapters/intro.tex', 14, 'Overfull \\hbox (12.0pt too wide) in paragraph at lines 14--15'),
      entry('warning', '/Users/you/.proton/tex/TinyTeX/texmf-dist/tex/latex/hyperref/hyperref.sty', null, 'hyperref: a package\'s own')]}}]); await settle();
    assert(!pane.querySelector('[data-tex-body="log"]').hidden, 'an error does not bring the log forward');
    assert(pane.querySelector('[data-tex-build]').classList.contains('bad') && pane.querySelector('[data-tex-log-count]').textContent === '1', 'the error is not counted');
    const groups = [...pane.querySelectorAll('.tex-group h4')].map((h) => h.textContent.replace(/\s+/g, ' ').trim());
    assert(JSON.stringify(groups) === JSON.stringify(['Errors 1', 'Warnings 2', 'Bad boxes 1']), `the groups: ${groups}`);
    const error = pane.querySelector('.tex-entry.error');
    assert(error.textContent.includes('chapters/intro.tex:12') && error.querySelector('.tex-context')?.textContent === 'We use \\foo', 'the error does not say where, or what TeX was reading');
    at = win.commands.length;
    error.click(); await settle();
    assert(JSON.stringify(sent(at, 'jumpTo')[0]) === JSON.stringify({type: 'jumpTo', mode: 'tex', path: 'chapters/intro.tex', line: 11, column: 0}), `not taken to its line: ${JSON.stringify(sent(at, 'jumpTo'))}`);
    assert(pane.querySelector('.tex-entry.warning[disabled]'), 'a package\'s own file is not offered to open');

    // What it lacks: found, then installed, each when asked.
    at = win.commands.length;
    pane.querySelector('[data-tex-packages="find"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'texFindPackages')[0]?.files) === JSON.stringify(['tikz-cd.sty']), 'the package was not looked for');
    await win.pushEvents([{type: 'texPackages', packages: {state: 'found', packages: [{name: 'tikz-cd', files: ['tikz-cd.sty']}], unknown: []}}]); await settle();
    assert(!sent(at, 'texInstallPackages').length, 'installed before the second say-so');
    pane.querySelector('[data-tex-packages="install"]').click(); await settle();
    assert(JSON.stringify(sent(at, 'texInstallPackages')[0]?.packages) === JSON.stringify(['tikz-cd']), 'Install did not ask for tikz-cd');
    await win.pushEvents([{type: 'texPackages', packages: {state: 'idle'}}, build({state: 'done', pdf: '/Users/you/.proton/tex/builds/paper-1/main_tex/view/main.pdf', version: 1, seconds: 0.6, pages: 3}),
      {type: 'texLog', root: '/Users/you/paper', log: {pages: 3, missing: [], entries: []}}]); await settle();
    assert(pane.querySelector('[data-tex-build]').textContent.includes('Typeset in under a second · 3 pages'), pane.querySelector('[data-tex-build]').textContent);
    assert(pane.querySelector('[data-tex-log]').textContent.includes('Nothing to say'), 'a clean log does not say so');
    pane.querySelector('[data-tex-tab="pdf"]').click(); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('The PDF is drawn in the pane, read again in place after each build, zoomed, and double-clicked for where in the source', async () => {
    await win.dispatch({type: 'setMode', mode: 'tex'}); await settle();
    const pane = $('#tex-pane');
    pane.querySelector('[data-tex-tab="pdf"]').click(); await settle();
    await win.pushEvents([{type: 'texEnv', env: {state: 'ready', tinytex: 'v2026.10', texlab: 'v5.26.0', bundle: 'full', size: 812_000_000, home: '/Users/you/.proton/tex'}}]); await settle();
    const done = (version) => ({type: 'texBuild', build: {state: 'done', main: 'main.tex', engine: 'pdfLaTeX', seconds: 0.4, pdf: '/tests/ui/fixtures/three-pages.pdf', version, errors: 0, warnings: 0, boxes: 0, pages: 3, said: null}});
    await win.pushEvents([done(1)]);
    const pages = pane.querySelector('[data-tex-pages]');
    const wait = async (ready) => { for (let i = 0; i < 60 && !ready(); i++) await settle(); };
    await wait(() => pages.querySelectorAll('.doc-page canvas').length > 0);
    assert(!pages.hidden && pages.querySelectorAll('.doc-page').length === 3 && pages.querySelector('.doc-page canvas'), 'the PDF is not drawn in the pane');
    assert(pane.querySelector('[data-tex-page]').textContent === '1 / 3', `where it is: ${pane.querySelector('[data-tex-page]').textContent}`);
    assert(!pane.querySelector('[data-tex-note]').checkVisibility?.() || pane.querySelector('[data-tex-note]').hidden, 'the empty page is still over it');

    // Typeset again: read in place, at the same place.
    pages.scrollTop = pages.querySelector('.doc-page[data-page="2"]').offsetTop + 40; await settle();
    const before = pages.scrollTop;
    const first = pages.querySelector('.doc-page');
    await win.pushEvents([done(2)]);
    await wait(() => pages.querySelector('.doc-page') !== first && pages.querySelector('.doc-page[data-page="2"] canvas'));
    assert(pages.querySelector('.doc-page') !== first, 'the PDF was not read again');
    assert(Math.abs(pages.scrollTop - before) < 4, `the place was lost: ${before} → ${pages.scrollTop}`);

    // Zoomed by its buttons; its keys its own while it has the keyboard.
    const width = pages.querySelector('.doc-page').offsetWidth;
    pane.querySelector('[data-tex-zoom="1"]').click(); await settle();
    assert(pages.querySelector('.doc-page').offsetWidth > width, '+ did not zoom in');
    pane.querySelector('[data-tex-zoom="0"]').click(); await settle();
    pages.focus();
    const keyed = win.commands.length;
    const top = pages.scrollTop;
    key('j'); key('j'); await settle();
    assert(pages.scrollTop > top && !win.commands.slice(keyed).some((c) => c.type === 'key'), 'j did not scroll the PDF, or reached the editor');

    // A double-click asks where in the source that is.
    const page = pages.querySelector('.doc-page[data-page="1"]');
    const box = page.getBoundingClientRect();
    const at = win.commands.length;
    page.dispatchEvent(new win.MouseEvent('dblclick', {bubbles: true, clientX: box.left + 72, clientY: box.top + 100}));
    await settle();
    const asked = sent(at, 'texSyncInverse')[0];
    assert(asked?.page === 1 && asked.x > 0 && asked.y > 0, `not asked where: ${JSON.stringify(asked)}`);
    const saved = win.commands.length;
    pane.querySelector('[data-tex-save-pdf]').click(); await settle();
    assert(sent(saved, 'texSavePdf').length === 1, 'Save PDF did not ask');

    // From the source: ⌘⏎ or the target asks, and the place is marked.
    doc.body.focus();
    const forward = win.commands.length;
    key('Enter', {metaKey: true}); await settle();
    pane.querySelector('[data-tex-sync]').click(); await settle();
    assert(sent(forward, 'texSyncForward').length === 2, 'neither ⌘⏎ nor the target asked where the line is');
    await win.pushEvents([{type: 'texSync', found: {page: 2, rects: [{x: 72, y: 90, w: 300, h: 12}, {x: 72, y: 104, w: 120, h: 12}]}}]); await settle();
    const mark = pages.querySelector('.doc-page[data-page="2"] .pdf-mark');
    // Page 2 of the fixture is 400 points wide; the mark is round both lines, 26 points tall.
    const scale = pages.querySelector('.doc-page[data-page="2"]').offsetWidth / 400;
    assert(mark && Math.abs(parseFloat(mark.style.height) - 26 * scale) < 3, `the place is not marked round its lines: ${mark?.style.cssText} at ${scale}`);
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('LaTeX\'s dock shows the PDF, the log or the agent, and the agent\'s staged changes are there too', async () => {
    await win.dispatch({type: 'setMode', mode: 'tex'}); await settle();
    const pane = $('#tex-pane');
    pane.querySelector('[data-tex-tab="log"]').click(); await settle();
    assert(!pane.querySelector('[data-tex-body="log"]').hidden && pane.querySelector('[data-tex-body="pdf"]').hidden, 'the Log tab did not show');
    pane.querySelector('[data-tex-tab="agent"]').click(); await settle();
    const agent = pane.querySelector('[data-tex-body="agent"]');
    assert(!agent.hidden && agent.querySelector('[data-agent-input]') && agent.querySelector('[data-agent-thread]'), 'the agent is not in the pane');
    await win.pushEvents([{type: 'review', files: [{path: 'main.tex', creates: false, hunks: [{line: 3, lines: [{mark: 'removed', text: 'Helo.'}, {mark: 'added', text: 'Hello.'}]}]}]}]); await settle();
    const staged = agent.querySelector('[data-staged]');
    assert(staged && !staged.hidden && staged.textContent.includes('main.tex'), 'the staged change is not shown in LaTeX\'s pane');
    await win.pushEvents([{type: 'review', files: []}]); await settle();
    pane.querySelector('[data-tex-tab="pdf"]').click(); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('The outline is under the file tree in LaTeX alone, stepped in by depth, its words counted, a heading a click from its line', async () => {
    await win.dispatch({type: 'setMode', mode: 'tex'}); await settle();
    await win.pushEvents([{type: 'texOutline', main: 'main.tex', outline: {words: 12345, headings: [
      {level: 2, title: 'Introduction', file: 'main.tex', line: 3, label: 'sec:intro'},
      {level: 3, title: 'Data', file: 'chapters/method.tex', line: 4, label: null},
      {level: 2, title: 'Results', file: 'main.tex', line: 9, label: null}]}}]); await settle();
    const box = $('.workspace.on [data-tex-outline]');
    assert(shown(box), 'the outline is not shown');
    assert(box.querySelector('[data-tex-words]').textContent === '12,345 words', `the words are not counted: ${box.querySelector('[data-tex-words]').textContent}`);
    const rows = [...box.querySelectorAll('.tex-heading')];
    assert(rows.map((r) => r.textContent).join('|') === 'Introduction|Data|Results', `not its headings: ${rows.map((r) => r.textContent)}`);
    assert(rows[1].getBoundingClientRect().left === rows[0].getBoundingClientRect().left
      && parseFloat(getComputedStyle(rows[1]).paddingLeft) > parseFloat(getComputedStyle(rows[0]).paddingLeft), 'a subsection is not stepped in');
    assert(rows[0].title.includes('sec:intro'), 'the label is not said');
    const at = win.commands.length;
    rows[1].click(); await settle();
    assert(JSON.stringify(sent(at, 'jumpTo')[0]) === JSON.stringify({type: 'jumpTo', mode: 'tex', path: 'chapters/method.tex', line: 3, column: 0}), `not taken to its line: ${JSON.stringify(sent(at, 'jumpTo'))}`);
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    assert(!shown($('.workspace.on [data-tex-outline]')), 'Codex shows LaTeX\'s outline');
    await win.pushEvents([{type: 'texOutline', main: null, outline: {words: 0, headings: []}}]); await settle();
  });
}
