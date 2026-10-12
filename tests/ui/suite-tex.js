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
