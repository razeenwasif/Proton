/* Excel's checks, run by suite.js in the same fixture window: its place on
 * the rail, the home page, and the grid as the core's events draw it —
 * the cells on screen asked for, typing turned into `sheetSet`, the tabs,
 * ⌘S. The fixture transport answers none of these; the events are pushed. */

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, key: Function, shown: (el: any) => boolean}} t */
export async function sheetChecks({test, assert, win, doc, $, settle, key, shown}) {
  const ws = () => $('.workspace[data-ws="sheet"]');
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const plain = {bold: false, italic: false, underline: false, color: null, fill: null, format: '', align: null, wrap: false};
  const book = (/** @type {Partial<any>} */ more = {}) => ({type: 'sheetBook', book: {path: 'budget.sheet', name: 'budget', kind: 'folder',
    sheets: [{name: 'Sheet1', rows: 7, cols: 3, frozen: [0, 0], widths: [[0, 90]]}, {name: 'Q2', rows: 0, cols: 0, frozen: [0, 0], widths: []}],
    active: 0, styles: [plain, {...plain, bold: true, fill: '#ffff00'}], generation: 3, version: 1, dirty: false, canUndo: false, canRedo: false, dropped: [], ...more}});
  const cell = (/** @type {number} */ row, /** @type {number} */ col, /** @type {string} */ text, input = text, kind = 't', style = 0) =>
    ({row, col, text, input, style, kind, color: null});
  const cells = (/** @type {any[]} */ list, partial = false, generation = 3) => ({type: 'sheetCells', generation, version: 1, sheet: 0,
    rows: [0, 100], cols: [0, 30], cells: list, partial});

  await test('Excel sits after Notebook: ⌃6 opens it, in Codex\'s chrome and on an opaque ground, and is the last', async () => {
    const asked = () => (win.invokes ?? []).filter((/** @type {any} */ c) => c.name === 'set_opaque').map((/** @type {any} */ c) => c.args.color);
    const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
    await win.dispatch({type: 'setMode', mode: 'chat'}); await settle();
    const before = win.commands.length, opaque = asked().length;
    /** @type {HTMLElement} */ (doc.body).focus();
    key('6', {ctrlKey: true}); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'sheet'), '⌃6 did not ask for Excel');
    assert($('.workspace.on')?.dataset.ws === 'sheet', 'Excel is not on screen');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Excel kept the old chrome');
    assert(!shown($('.titlebar [data-term-toggle]')), 'Excel offers a terminal it cannot show');
    const tab = $('.titlebar [data-mode="sheet"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃6') && tab.textContent.trim() === 'Excel', 'the title bar does not show Excel');
    assert($('.mode-rail [data-mode="sheet"]')?.textContent.trim() === 'Excel', 'the rail does not say Excel');
    assert(!asked().slice(opaque).includes(null) && asked().at(-1) === ground, `Excel is not on the ground: ${JSON.stringify(asked().slice(opaque))}`);
    const before7 = win.commands.length;
    key('7', {ctrlKey: true}); await settle();
    assert(!sent(before7, 'setMode').length, '⌃7 asked for a mode');
  });

  await test('With no workbook open, Excel lists the project\'s and makes one as a folder or an .xlsx', async () => {
    await win.pushEvents([{type: 'sheetHome', workbooks: [{path: 'budget.sheet', name: 'budget', kind: 'folder'}, {path: 'data/q1.xlsx', name: 'q1', kind: 'xlsx'}]},
      {type: 'sheetBook', book: null}]);
    await settle();
    assert(shown(ws().querySelector('[data-sheet-home]')) && !shown(ws().querySelector('[data-sheet-book]')), 'the home page is not what is shown');
    const rows = [...ws().querySelectorAll('[data-sheet-list] [data-sheet-open]')];
    assert(rows.map((r) => r.dataset.sheetOpen).join() === 'budget.sheet,data/q1.xlsx', 'the workbooks are not listed');
    assert(rows[1].textContent.includes('Excel'), 'an .xlsx does not say what it is');
    const before = win.commands.length;
    rows[1].click(); await settle();
    assert(sent(before, 'sheetOpen')[0]?.path === 'data/q1.xlsx', 'clicking a workbook did not open it');
    const form = ws().querySelector('[data-sheet-form]');
    form.querySelector('input').value = 'Q3 Plan';
    form.querySelector('select').value = 'xlsx';
    form.requestSubmit(); await settle();
    const made = sent(before, 'sheetNew')[0];
    assert(made?.name === 'Q3 Plan' && made.folder === 'q3-plan.xlsx', `made ${JSON.stringify(made)}`);
  });

  await test('The grid asks for the cells on screen and draws them where they go, numbers to the right', async () => {
    const before = win.commands.length;
    await win.pushEvents([book()]); await settle();
    assert(shown(ws().querySelector('[data-sheet-book]')) && !shown(ws().querySelector('[data-sheet-home]')), 'the workbook is not shown');
    const asked = sent(before, 'sheetView').at(-1);
    assert(asked?.generation === 3 && asked.sheet === 0 && asked.rows[0] === 0 && asked.rows[1] >= 60 && asked.cols[1] >= 12, `asked for ${JSON.stringify(asked)}`);
    await win.pushEvents([cells([cell(0, 0, 'Item', 'Item', 't', 1), cell(1, 1, '1,450.00', '1450', 'n'), cell(6, 1, '1997.95', '=SUM(B2:B5)', 'n'), cell(2, 2, '#DIV/0!', '=1/0', 'e')])]);
    await settle();
    const drawn = (/** @type {string} */ at) => /** @type {HTMLElement | null} */ (ws().querySelector(`[data-cell="${at}"]`));
    assert(drawn('1,1')?.textContent === '1,450.00' && drawn('1,1')?.style.textAlign === 'right', 'a number is not drawn to the right');
    assert(drawn('0,0')?.style.fontWeight === '700' && drawn('0,0')?.style.background.includes('255, 255, 0'), 'a style is not drawn');
    assert(drawn('2,2')?.classList.contains('err'), 'an error is not marked');
    // Column A is 90pt wide: B starts 120px along, after the row numbers.
    const a = drawn('0,0'), b = drawn('1,1');
    assert(a && b && parseFloat(b.style.left) - parseFloat(a.style.left) === 120, `column A is ${b && a ? parseFloat(b.style.left) - parseFloat(a.style.left) : '?'}px`);
    assert(ws().querySelector('[data-sheet-name]').value === 'A1', 'the name box does not say A1');
    // A cell event for another workbook is not drawn.
    await win.pushEvents([cells([cell(5, 5, 'stale')], true, 2)]); await settle();
    assert(!drawn('5,5'), 'a cell of another workbook was drawn');
  });

  await test('Typing into a cell sets it, Enter moves down, and the formula bar shows what was typed', async () => {
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.focus();
    key('j'); key('j'); key('j'); key('j'); key('j'); key('j'); key('l'); await settle();
    assert(ws().querySelector('[data-sheet-name]').value === 'B7', `the name box says ${ws().querySelector('[data-sheet-name]').value}`);
    assert(ws().querySelector('[data-sheet-bar]').value === '=SUM(B2:B5)', 'the formula bar does not show the formula');
    key('j'); await settle();
    const before = win.commands.length;
    key('4'); await settle();
    const editor = ws().querySelector('[data-sheet-editor]');
    assert(!editor.hidden && editor.value === '4' && doc.activeElement === editor, 'typing did not start the cell over');
    editor.value = '42';
    editor.dispatchEvent(new win.Event('input'));
    key('Enter'); await settle();
    const set = sent(before, 'sheetSet')[0];
    assert(set?.generation === 3 && set.sheet === 0 && JSON.stringify(set.cells) === '[["B8","42"]]', `set ${JSON.stringify(set)}`);
    assert(editor.hidden && ws().querySelector('[data-sheet-name]').value === 'B9', 'Enter did not move down');
    await win.pushEvents([cells([cell(7, 1, '42', '42', 'n')], true), book({dirty: true, canUndo: true})]); await settle();
    assert(ws().querySelector('[data-cell="7,1"]')?.textContent === '42', 'the cell sent back is not drawn');
    assert(!ws().querySelector('[data-sheet-dirty]').hidden, 'an unsaved workbook does not say so');
    // Escape leaves what was there.
    key('Enter'); await settle();
    editor.value = 'not this';
    key('Escape'); await settle();
    assert(editor.hidden && sent(before, 'sheetSet').length === 1, 'Escape set the cell');
  });

  await test('x clears the selection, u undoes, ⌘S saves, and the tabs switch, add and remove sheets', async () => {
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.focus();
    key('k'); key('K', {shiftKey: true}); await settle();
    const before = win.commands.length;
    key('x'); await settle();
    const cleared = sent(before, 'sheetSet')[0];
    assert(cleared && cleared.cells.some((/** @type {string[]} */ c) => c[0] === 'B8' && c[1] === ''), `cleared ${JSON.stringify(cleared)}`);
    key('u'); await settle();
    assert(sent(before, 'sheetUndo').length === 1, 'u did not undo');
    key('s', {metaKey: true}); await settle();
    assert(sent(before, 'sheetSave').length === 1, '⌘S did not save');
    const tabs = [...ws().querySelectorAll('[data-sheet-tab]')];
    assert(tabs.map((t) => t.textContent).join() === 'Sheet1,Q2' && tabs[0].classList.contains('on'), 'the tabs are not the sheets');
    tabs[1].click(); await settle();
    assert(sent(before, 'sheetTab')[0]?.sheet === 1, 'a tab did not switch');
    ws().querySelector('[data-sheet-add]').click(); await settle();
    assert(sent(before, 'sheetAddSheet').length === 1, '+ did not add a sheet');
    ws().querySelector('[data-sheet-remove]').click(); await settle();
    assert(sent(before, 'sheetRemoveSheet')[0]?.sheet === 0, 'the sheet shown was not the one removed');
    // Another sheet shown: its cells are asked for, and the last one's go.
    const asked = win.commands.length;
    await win.pushEvents([book({active: 1})]); await settle();
    assert(sent(asked, 'sheetView').at(-1)?.sheet === 1, 'the new sheet\'s cells were not asked for');
    assert(!ws().querySelector('[data-cell="1,1"]'), 'the last sheet\'s cells are still drawn');
  });

  await test('Scrolling a long way asks for the rows there, and the grid goes on', async () => {
    await win.pushEvents([book({active: 0, version: 2})]); await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    const space = ws().querySelector('[data-sheet-space]');
    const tall = space.offsetHeight;
    const before = win.commands.length;
    scroll.scrollTop = scroll.scrollHeight; scroll.dispatchEvent(new win.Event('scroll')); await settle();
    const asked = sent(before, 'sheetView').at(-1);
    assert(asked && asked.rows[1] > 150, `scrolling to the end asked for ${JSON.stringify(asked)}`);
    assert(space.offsetHeight > tall, 'the grid did not go on');
    const first = Number(ws().querySelector('.xl-rh')?.textContent);
    assert(first > 100, `the row numbers start at ${first}`);
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
