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
}
