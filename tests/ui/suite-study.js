/* Notebook's checks, run by suite.js in the same fixture window: its place on
 * the rail, and its sources, tutor and studio as the core's events draw them. */

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, key: Function, shown: (el: any) => boolean}} t */
export async function studyChecks({test, assert, win, doc, $, settle, key, shown}) {
  await test('Notebook sits after Research: ⌃5 opens it, in Codex\'s chrome and on an opaque ground, and Design is ⌃6', async () => {
    const asked = () => (win.invokes ?? []).filter((c) => c.name === 'set_opaque').map((c) => c.args.color);
    const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
    await win.dispatch({type: 'setMode', mode: 'design'}); await settle();
    const before = win.commands.length, opaque = asked().length;
    doc.body.focus();
    key('5', {ctrlKey: true}); await settle();
    assert(win.commands.slice(before).some((c) => c.type === 'setMode' && c.mode === 'study'), '⌃5 did not ask for Notebook');
    assert($('.workspace.on')?.dataset.ws === 'study', 'Notebook is not on screen');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Notebook kept the old chrome');
    assert(!shown($('.titlebar [data-term-toggle]')), 'Notebook offers a terminal it cannot show');
    const tab = $('.titlebar [data-mode="study"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃5') && tab.textContent.trim() === 'Notebook', 'the title bar does not show Notebook');
    assert($('.mode-rail [data-mode="study"]')?.textContent.trim() === 'Notebook', 'the rail does not say Notebook');
    assert($('.titlebar [data-mode="design"]')?.title.includes('⌃6'), 'Design is not ⌃6');
    assert(asked().slice(opaque).at(-1) === ground, `Notebook did not ask for the ground: ${JSON.stringify(asked().slice(opaque))}`);
    const before6 = win.commands.length;
    key('6', {ctrlKey: true}); await settle();
    assert(win.commands.slice(before6).some((c) => c.type === 'setMode' && c.mode === 'design'), '⌃6 did not ask for Design');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
