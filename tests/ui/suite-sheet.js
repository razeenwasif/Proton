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
    sheets: [{name: 'Sheet1', rows: 7, cols: 3, frozen: [0, 0], widths: [[0, 90]], hidden: [], filter: null, rules: [], charts: []}, {name: 'Q2', rows: 0, cols: 0, frozen: [0, 0], widths: [], hidden: [], filter: null, rules: [], charts: []}],
    active: 0, styles: [plain, {...plain, bold: true, fill: '#ffff00'}], generation: 3, version: 1, dirty: false, canUndo: false, canRedo: false, dropped: [], ...more}});
  const cell = (/** @type {number} */ row, /** @type {number} */ col, /** @type {string} */ text, input = text, kind = 't', style = 0) =>
    ({row, col, text, input, style, kind, color: null, fill: null, bold: false});
  const cells = (/** @type {any[]} */ list, partial = false, generation = 3) => ({type: 'sheetCells', generation, version: 1, sheet: 0,
    rows: [0, 100], cols: [0, 30], cells: list, partial});

  await test('Excel sits after Notebook: ⌃5 opens it, in Codex\'s chrome and on an opaque ground, and Circuit follows it on ⌃6', async () => {
    const asked = () => (win.invokes ?? []).filter((/** @type {any} */ c) => c.name === 'set_opaque').map((/** @type {any} */ c) => c.args.color);
    const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    const before = win.commands.length, opaque = asked().length;
    /** @type {HTMLElement} */ (doc.body).focus();
    key('5', {ctrlKey: true}); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'sheet'), '⌃5 did not ask for Excel');
    assert($('.workspace.on')?.dataset.ws === 'sheet', 'Excel is not on screen');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Excel kept the old chrome');
    assert(!shown($('.titlebar [data-term-toggle]')), 'Excel offers a terminal it cannot show');
    const tab = $('.titlebar [data-mode="sheet"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃5') && tab.textContent.trim() === 'Excel', 'the title bar does not show Excel');
    assert($('.mode-rail [data-mode="sheet"]')?.textContent.trim() === 'Excel', 'the rail does not say Excel');
    assert(!asked().slice(opaque).includes(null) && asked().at(-1) === ground, `Excel is not on the ground: ${JSON.stringify(asked().slice(opaque))}`);
    const before6 = win.commands.length;
    key('6', {ctrlKey: true}); await settle();
    assert(sent(before6, 'setMode').some((c) => c.mode === 'circuit'), '⌃6 did not ask for Circuit');
    key('5', {ctrlKey: true}); await settle();
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

  await test('Vim\'s keys act on the selection: v and y copy, dd deletes rows, o puts one in, p pastes, ⌘B and :freeze format', async () => {
    await win.pushEvents([book({active: 0, version: 3}), cells([cell(0, 0, 'Item', 'Item', 't', 1), cell(1, 1, '1,450.00', '1450', 'n'), cell(6, 1, '1997.95', '=SUM(B2:B5)', 'n')])]);
    await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.focus();
    // To A1, then v and two down and one across: A1:B3, copied.
    key('g'); key('g'); key('0'); await settle();
    let before = win.commands.length;
    key('v'); key('j'); key('j'); key('l'); await settle();
    assert(ws().querySelector('[data-sheet-name]').value === 'A1:B3', `visual selected ${ws().querySelector('[data-sheet-name]').value}`);
    key('y'); await settle();
    const copied = sent(before, 'sheetCopy')[0];
    assert(copied?.range === 'A1:B3' && copied.cut === false, `copied ${JSON.stringify(copied)}`);
    assert(ws().querySelector('[data-sheet-name]').value === 'A1', 'a copy did not leave visual');
    // The core's text goes on the system clipboard.
    await win.pushEvents([{type: 'sheetClip', text: 'Item\t\n\t1,450.00\n'}]); await settle();
    assert(win.clipboardText === 'Item\t\n\t1,450.00\n', `the clipboard has ${JSON.stringify(win.clipboardText)}`);
    // p pastes what the clipboard has, at the cursor.
    key('j'); key('j'); key('j'); await settle();
    before = win.commands.length;
    win.clipboardText = 'a\tb';
    key('p'); await settle();
    const pasted = sent(before, 'sheetPaste')[0];
    assert(pasted?.at === 'A4' && pasted.text === 'a\tb', `pasted ${JSON.stringify(pasted)}`);
    // dd takes the row out; o puts one in below; V and d two rows.
    key('d'); key('d'); await settle();
    const deleted = sent(before, 'sheetDelete')[0];
    assert(deleted && !deleted.cols && deleted.at === 3 && deleted.n === 1, `dd sent ${JSON.stringify(deleted)}`);
    key('o'); await settle();
    const put = sent(before, 'sheetInsert')[0];
    assert(put && !put.cols && put.at === 4 && put.n === 1, `o sent ${JSON.stringify(put)}`);
    key('V'); key('j'); key('d'); await settle();
    const rows = sent(before, 'sheetDelete')[1];
    assert(rows?.at === 4 && rows.n === 2, `Vjd sent ${JSON.stringify(rows)}`);
    // ⌘B makes the cursor's cell bold, as only that.
    key('b', {metaKey: true}); await settle();
    const bold = sent(before, 'sheetFormat')[0];
    assert(bold?.range === 'A5' && bold.patch.bold === true && bold.patch.italic === null && bold.patch.format === null, `⌘B sent ${JSON.stringify(bold)}`);
    // The format bar's number format.
    const fmt = ws().querySelector('[data-sheet-format]');
    fmt.value = '0.00'; fmt.dispatchEvent(new win.Event('change')); await settle();
    assert(sent(before, 'sheetFormat')[1]?.patch.format === '0.00', 'the format bar did not set a number format');
    // :freeze at B2 freezes a row and a column.
    scroll.focus();
    key('g'); key('g'); key('0'); key('j'); key('l'); key(':'); await settle();
    const cmd = ws().querySelector('[data-sheet-cmd]');
    assert(!cmd.hidden && doc.activeElement === cmd, ': did not open the command line');
    cmd.value = 'freeze';
    key('Enter'); await settle();
    const freeze = sent(before, 'sheetFreeze')[0];
    assert(freeze?.rows === 1 && freeze.cols === 1 && cmd.hidden, `:freeze sent ${JSON.stringify(freeze)}`);
  });

  await test('Frozen rows stay on screen as the grid scrolls, and a click there picks them', async () => {
    await win.pushEvents([book({active: 0, version: 4, sheets: [{name: 'Sheet1', rows: 500, cols: 3, frozen: [1, 0], widths: [], hidden: [], filter: null, rules: [], charts: []}, {name: 'Q2', rows: 0, cols: 0, frozen: [0, 0], widths: [], hidden: [], filter: null, rules: [], charts: []}]}),
      cells([cell(0, 0, 'Item', 'Item', 't'), cell(300, 0, 'deep', 'deep', 't')])]);
    await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.scrollTop = 300 * 24 - 24; scroll.dispatchEvent(new win.Event('scroll')); await settle();
    const head = /** @type {HTMLElement | null} */ (ws().querySelector('[data-cell="0,0"]'));
    const deep = /** @type {HTMLElement | null} */ (ws().querySelector('[data-cell="300,0"]'));
    assert(head && deep, 'the frozen row or the one scrolled to is not drawn');
    const box = scroll.getBoundingClientRect();
    const y = (/** @type {HTMLElement} */ el) => el.getBoundingClientRect().top - box.top;
    assert(Math.abs(y(head) - 24) < 1, `the frozen row is at ${y(head)}px, not under the letters`);
    assert(y(deep) >= 48 - 1, `row 301 is at ${y(deep)}px, under the frozen row`);
    // A click on the frozen row picks row 1, not the row scrolled under it.
    scroll.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, clientX: box.left + 60, clientY: box.top + 30, button: 0}));
    win.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true}));
    await settle();
    assert(ws().querySelector('[data-sheet-name]').value === 'A1', `a click on the frozen row picked ${ws().querySelector('[data-sheet-name]').value}`);
  });

  await test('Dragging the fill handle fills, and dragging a letter\'s edge sets the column\'s width', async () => {
    await win.pushEvents([book({active: 0, version: 5}), cells([cell(0, 0, '1', '1', 'n'), cell(1, 0, '2', '2', 'n')])]);
    await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.scrollTop = 0; scroll.scrollLeft = 0; scroll.dispatchEvent(new win.Event('scroll')); await settle();
    const box = scroll.getBoundingClientRect();
    const at = (/** @type {number} */ r, /** @type {number} */ c) => ({clientX: box.left + 52 + c * 72 + 30, clientY: box.top + 24 + r * 24 + 12});
    // A1:A2 selected by a drag.
    scroll.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, button: 0, ...at(0, 0)}));
    win.dispatchEvent(new win.MouseEvent('mousemove', {bubbles: true, ...at(1, 0)}));
    win.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true})); await settle();
    assert(ws().querySelector('[data-sheet-name]').value === 'A1:A2', `the drag selected ${ws().querySelector('[data-sheet-name]').value}`);
    const before = win.commands.length;
    const handle = ws().querySelector('[data-fill-handle]');
    handle.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, button: 0, ...at(1, 0)}));
    win.dispatchEvent(new win.MouseEvent('mousemove', {bubbles: true, ...at(5, 0)}));
    win.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true})); await settle();
    const fill = sent(before, 'sheetFill')[0];
    assert(fill?.from === 'A1:A2' && fill.to === 'A1:A6', `the handle filled ${JSON.stringify(fill)}`);
    const edge = ws().querySelector('[data-resize-col="1"]');
    const x = edge.getBoundingClientRect().left + 3;
    edge.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, button: 0, clientX: x, clientY: box.top + 10}));
    win.dispatchEvent(new win.MouseEvent('mousemove', {bubbles: true, clientX: x + 48, clientY: box.top + 10}));
    win.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true})); await settle();
    const w = sent(before, 'sheetWidth')[0];
    assert(w?.col === 1 && w.width === 90, `the edge set ${JSON.stringify(w)}`);
  });

  await test('A filter hides its rows from the grid, its ▾ lists the values to tick, and sorting and colour rules are asked for', async () => {
    const tabOf = (/** @type {any} */ more) => ({name: 'Sheet1', rows: 6, cols: 2, frozen: [0, 0], widths: [], hidden: [], filter: null, rules: [], charts: [], ...more});
    await win.pushEvents([book({active: 0, version: 6, sheets: [tabOf({hidden: [[2, 3]], filter: {range: 'A1:B6', columns: [1]}})]}),
      cells([cell(0, 0, 'Fruit'), cell(0, 1, 'Kg'), cell(1, 0, 'pear'), cell(2, 0, 'apple'), cell(3, 0, 'fig'), cell(4, 0, 'kiwi'), cell(4, 1, '9', '9', 'n', 0)])]);
    await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.scrollTop = 0; scroll.scrollLeft = 0; scroll.dispatchEvent(new win.Event('scroll')); await settle();
    assert(!ws().querySelector('[data-cell="2,0"]') && !ws().querySelector('[data-cell="3,0"]'), 'hidden rows are drawn');
    const kiwi = /** @type {HTMLElement} */ (ws().querySelector('[data-cell="4,0"]'));
    assert(kiwi && parseFloat(kiwi.style.top) === 24 + 2 * 24, `row 5 is at ${kiwi?.style.top}, not straight after row 2`);
    assert(ws().querySelector('.xl-rh[data-row="4"]')?.classList.contains('gap'), 'the row after hidden ones does not say so');
    // j from row 2 goes past the hidden rows to row 5.
    scroll.focus();
    key('g'); key('g'); key('j'); key('j'); await settle();
    assert(ws().querySelector('[data-sheet-name]').value === 'A5', `j went to ${ws().querySelector('[data-sheet-name]').value}`);
    // The ▾ on B asks for B's values; the list ticks and applies.
    const before = win.commands.length;
    const button = ws().querySelector('[data-filter-col="1"]');
    assert(button?.classList.contains('on') && ws().querySelector('[data-filter-col="0"]') && !ws().querySelector('[data-filter-col="0"]').classList.contains('on'), 'the header buttons are not drawn as they are');
    button.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, button: 0})); await settle();
    assert(sent(before, 'sheetFilterValues')[0]?.col === 1, 'the ▾ did not ask for the values');
    await win.pushEvents([{type: 'sheetFilterValues', col: 1, values: [['9', 1], ['', 4]], ticked: null, criteria: ''}]); await settle();
    const pop = ws().querySelector('.xl-pop');
    assert(pop && pop.querySelectorAll('[data-value]').length === 2, 'the values are not listed');
    /** @type {HTMLInputElement} */ (pop.querySelector('[data-value="1"]')).checked = false;
    pop.querySelector('[data-criteria]').value = '>5';
    pop.querySelector('[data-apply]').click(); await settle();
    const set = sent(before, 'sheetFilterSet')[0];
    assert(set?.col === 1 && JSON.stringify(set.values) === '["9"]' && set.criteria === '>5' && !ws().querySelector('.xl-pop'), `applied ${JSON.stringify(set)}`);
    // Sorting by the cursor's column: the filter's rows under its header.
    ws().querySelector('[data-sheet-sort="down"]').click(); await settle();
    const sort = sent(before, 'sheetSort')[0];
    assert(sort?.range === 'A2:B6' && JSON.stringify(sort.keys) === '[[0,false]]', `sorted ${JSON.stringify(sort)}`);
    // The colour rules: one added over the selection.
    ws().querySelector('[data-sheet-rules]').click(); await settle();
    const rules = ws().querySelector('.xl-pop');
    rules.querySelector('[data-rule-when]').value = 'greater';
    rules.querySelector('[data-rule-when]').dispatchEvent(new win.Event('change'));
    rules.querySelector('[data-rule-a]').value = '5';
    rules.querySelector('[data-add-rule]').click(); await settle();
    const rule = sent(before, 'sheetRuleAdd')[0]?.rule;
    assert(rule?.range === 'A5' && rule.when.when === 'greater' && rule.when.value === 5 && rule.fill, `added ${JSON.stringify(rule)}`);
    // A cell a rule colours is drawn so.
    await win.pushEvents([cells([{...cell(4, 1, '9', '9', 'n'), fill: '#ff0000', bold: true}], true)]); await settle();
    const nine = /** @type {HTMLElement} */ (ws().querySelector('[data-cell="4,1"]'));
    assert(nine?.style.background.includes('255, 0, 0') && nine.style.fontWeight === '700', 'a rule\'s colour is not drawn');
    await win.pushEvents([book({active: 0, version: 7})]); await settle();
  });

  await test('A chart is drawn from its series, moved by its header, made another kind, and removed', async () => {
    const chart = {kind: 'bar', range: 'A1:B3', title: 'Costs', at: 'E2', width: 400, height: 260};
    const view = {chart, categories: ['Rent', 'Food'], xs: [null, null], series: [['Cost', [1450, -300]]]};
    await win.pushEvents([book({active: 0, version: 8, sheets: [{name: 'Sheet1', rows: 3, cols: 2, frozen: [0, 0], widths: [], hidden: [], filter: null, rules: [], charts: [view]}]})]);
    await settle();
    const el = /** @type {HTMLElement} */ (ws().querySelector('[data-chart="0"]'));
    assert(el && parseFloat(el.style.left) === 52 + 4 * 72 && parseFloat(el.style.top) === 24 + 24, `the chart is at ${el?.style.left}, ${el?.style.top}`);
    assert(el.querySelectorAll('svg rect').length === 2 && el.querySelector('.xl-chart-title').textContent === 'Costs', 'the bars or the title are not drawn');
    const before = win.commands.length;
    const head = el.querySelector('[data-chart-drag]');
    const x = head.getBoundingClientRect().left + 200, y = head.getBoundingClientRect().top + 10;
    head.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, button: 0, clientX: x, clientY: y}));
    win.dispatchEvent(new win.MouseEvent('mousemove', {bubbles: true, clientX: x + 72, clientY: y + 48}));
    win.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true})); await settle();
    const moved = sent(before, 'sheetChartSet')[0];
    assert(moved?.index === 0 && moved.chart.at === 'F4' && moved.chart.range === 'A1:B3', `moving it sent ${JSON.stringify(moved)}`);
    const kind = el.querySelector('[data-chart-kind]');
    kind.value = 'pie'; kind.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(sent(before, 'sheetChartSet')[1]?.chart.kind === 'pie', 'the kind did not change');
    el.querySelector('[data-chart-remove]').click(); await settle();
    assert(sent(before, 'sheetChartRemove')[0]?.index === 0, 'the chart was not removed');
    // The format bar's chart button charts the selection, beside it.
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.focus();
    key('g'); key('g'); key('0'); key('v'); key('j'); key('j'); key('l'); await settle();
    ws().querySelector('[data-sheet-chart]').click(); await settle();
    const made = sent(before, 'sheetChartAdd')[0]?.chart;
    assert(made?.range === 'A1:B3' && made.at === 'D1' && made.kind === 'bar', `the chart button made ${JSON.stringify(made)}`);
    key('Escape'); await settle();
  });

  await test('Excel has the agent in its dock, with @sheet, and tells the core what is selected', async () => {
    await win.pushEvents([book({active: 0, version: 9})]); await settle();
    const dock = ws().querySelector('.xl-dock');
    assert(dock?.querySelector('[data-agent-input]') && dock.querySelector('[data-mention="@sheet"]'), 'there is no agent composer with @sheet');
    const toggle = [...doc.querySelectorAll('[data-panel-toggle="right"]')].find((b) => b.getClientRects().length);
    assert(toggle && !toggle.disabled, 'the dock cannot be shown');
    const before = win.commands.length;
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.focus();
    key('g'); key('g'); key('0'); key('v'); key('j'); key('l'); await settle(); await settle();
    const told = sent(before, 'sheetSelect').at(-1);
    assert(told?.range === 'A1:B2' && told.generation === 3, `the core was told ${JSON.stringify(told)}`);
    key('Escape'); await settle();
  });

  await test('A range goes to Data from the format bar, and a table comes back from Data\'s dock', async () => {
    await win.pushEvents([book({active: 0, version: 10})]); await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    scroll.focus();
    key('g'); key('g'); key('0'); key('v'); key('j'); key('l'); await settle();
    const before = win.commands.length;
    ws().querySelector('[data-sheet-to-data]').click(); await settle();
    const to = sent(before, 'sheetToData')[0];
    assert(to?.range === 'A1:B2' && to.generation === 3, `sent ${JSON.stringify(to)}`);
    key('Escape'); await settle();
    $('[data-df-to-sheet]').click(); await settle();
    assert(sent(before, 'sheetFromData').length === 1, 'Data\'s dock did not send its table');
  });

  await test('/ asks the core for what is typed, marks what it finds, and n and N walk it round the ends', async () => {
    await win.pushEvents([book({version: 20}), cells([cell(0, 0, 'Rent'), cell(1, 0, 'rent due'), cell(1, 2, 'Rent'), cell(5, 0, 'other')])]); await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    const bar = ws().querySelector('[data-sheet-find]');
    const field = ws().querySelector('[data-sheet-find-text]');
    const name = ws().querySelector('[data-sheet-name]');
    scroll.focus();
    key('g'); key('g'); key('0'); await settle();
    assert(!shown(bar), 'the bar is open before it is asked for');
    const height = scroll.clientHeight;
    const before = win.commands.length;
    key('/'); await settle();
    assert(shown(bar) && doc.activeElement === field, '/ did not open the bar and take the keys');
    assert(scroll.clientHeight < height, 'the grid did not make room');
    assert(sent(before, 'sheetSet').length === 0 && !shown(ws().querySelector('[data-sheet-editor]')), '/ started a cell');
    field.value = 'rent'; field.dispatchEvent(new win.Event('input'));
    await new Promise((done) => win.setTimeout(done, 180));
    const asked = sent(before, 'sheetFind').at(-1);
    assert(asked?.text === 'rent' && asked.generation === 3 && asked.sheet === 0 && asked.whole === false && asked.within === null, `asked ${JSON.stringify(asked)}`);
    const found = (/** @type {number} */ ask, /** @type {Record<string, any>} */ more = {}) => ({type: 'sheetMatches', generation: 3, sheet: 0, ask, matches: [[0, 0], [1, 0], [1, 2]], total: 3, shownOnly: 0, hidden: 0, ...more});
    await win.pushEvents([found(asked.ask - 1, {matches: [[5, 0]], total: 1})]); await settle();
    assert(!ws().querySelector('.xl-c.hit'), 'an answer to an older search was taken');
    await win.pushEvents([found(asked.ask)]); await settle();
    assert(ws().querySelectorAll('.xl-c.hit').length === 3, `${ws().querySelectorAll('.xl-c.hit').length} cells marked`);
    const count = ws().querySelector('[data-sheet-find-count]');
    assert(count.textContent === '1 of 3', `the count says ${count.textContent}`);
    // Enter goes on from the cursor, and / gives the keys back to the grid.
    key('Enter'); await settle();
    assert(name.value === 'A2' && doc.activeElement === scroll, `Enter went to ${name.value}`);
    assert(count.textContent === '2 of 3', `the count says ${count.textContent}`);
    const typed = win.commands.length;
    key('n'); await settle();
    assert(name.value === 'C2', `n went to ${name.value}`);
    key('n'); await settle();
    assert(name.value === 'A1', `n did not go round to the first: ${name.value}`);
    key('N', {shiftKey: true}); await settle();
    assert(name.value === 'C2', `N did not go round to the last: ${name.value}`);
    assert(sent(typed, 'sheetSet').length === 0 && !shown(ws().querySelector('[data-sheet-editor]')), 'n and N started a cell');
    // Off the matches the count says how many, and n goes to the next past the cursor.
    key('j'); key('j'); await settle();
    assert(count.textContent === '3 found', `the count says ${count.textContent}`);
    key('N', {shiftKey: true}); await settle();
    assert(name.value === 'C2', `N from past them went to ${name.value}`);
    // An edit is looked for again.
    const edit = win.commands.length;
    await win.pushEvents([book({version: 21})]);
    await new Promise((done) => win.setTimeout(done, 180));
    assert(sent(edit, 'sheetFind').length === 1, 'a change did not search again');
    // Escape takes it all away, and n types again.
    key('Escape'); await settle();
    assert(!shown(bar) && !ws().querySelector('.xl-c.hit') && doc.activeElement === scroll, 'Escape did not close the find');
    assert(scroll.clientHeight === height, 'the grid did not take its room back');
    key('n'); await settle();
    assert(shown(ws().querySelector('[data-sheet-editor]')), 'n does not start a cell with no search');
    key('Escape'); await settle();
  });

  await test('⌘F opens find and keeps its Enter; a range selected is what is searched and the bar says so', async () => {
    await win.pushEvents([book({version: 30})]); await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    const field = ws().querySelector('[data-sheet-find-text]');
    const within = ws().querySelector('[data-sheet-find-within]');
    scroll.focus();
    key('g'); key('g'); key('0'); key('v'); key('j'); key('l'); await settle();
    const before = win.commands.length;
    key('f', {metaKey: true}); await settle();
    assert(doc.activeElement === field && shown(ws().querySelector('[data-sheet-find]')), '⌘F did not open the bar');
    assert(shown(within) && within.textContent.includes('A1:B2'), `the range is not said: ${within.textContent}`);
    field.value = 'x'; field.dispatchEvent(new win.Event('input'));
    key('Enter'); await settle();
    const asked = sent(before, 'sheetFind').at(-1);
    assert(asked?.within === 'A1:B2', `asked ${JSON.stringify(asked)}`);
    await win.pushEvents([{type: 'sheetMatches', generation: 3, sheet: 0, ask: asked.ask, matches: [[1, 1]], total: 1, shownOnly: 0, hidden: 2}]); await settle();
    assert(ws().querySelector('[data-sheet-name]').value === 'B2', 'Enter ahead of the answer did not go to it');
    assert(doc.activeElement === field, 'Enter after ⌘F left the bar');
    assert(ws().querySelector('[data-sheet-find-count]').textContent.includes('2 hidden by a filter'), 'what a filter hides is not said');
    within.click(); await settle();
    assert(!shown(within) && sent(before, 'sheetFind').at(-1).within === null, 'the range did not go');
    ws().querySelector('[data-sheet-find-whole]').click(); await settle();
    assert(sent(before, 'sheetFind').at(-1).whole === true, 'Whole cell was not asked for');
    ws().querySelector('[data-sheet-find-whole]').click(); await settle();
    key('Escape'); await settle();
    assert(doc.activeElement === scroll, 'Escape did not give the keys back');
  });

  await test('Replace sends the cell found and goes on; Replace all sends what the bar says', async () => {
    await win.pushEvents([book({version: 40}), cells([cell(0, 0, 'Rent'), cell(1, 0, 'rent due')])]); await settle();
    const scroll = ws().querySelector('[data-sheet-scroll]');
    const field = ws().querySelector('[data-sheet-find-text]');
    const swap = ws().querySelector('[data-sheet-replace-text]');
    const name = ws().querySelector('[data-sheet-name]');
    scroll.focus();
    key('g'); key('g'); key('0'); await settle();
    const before = win.commands.length;
    key('f', {metaKey: true, altKey: true}); await settle();
    assert(shown(swap), '⌘⌥F did not show the replace row');
    field.value = 'rent'; field.dispatchEvent(new win.Event('input'));
    swap.value = 'rates';
    await new Promise((done) => win.setTimeout(done, 180));
    const asked = sent(before, 'sheetFind').at(-1);
    await win.pushEvents([{type: 'sheetMatches', generation: 3, sheet: 0, ask: asked.ask, matches: [[0, 0], [1, 0]], total: 3, shownOnly: 1, hidden: 0}]); await settle();
    assert(ws().querySelector('[data-sheet-find-count]').textContent.includes('1 only show it'), 'what only shows it is not said');
    swap.focus(); key('Enter'); await settle();
    const one = sent(before, 'sheetReplace').at(-1);
    assert(one?.at === 'A1' && one.text === 'rent' && one.with === 'rates' && one.generation === 3, `sent ${JSON.stringify(one)}`);
    assert(name.value === 'A2', `Replace did not go on: ${name.value}`);
    assert(doc.activeElement === swap, 'Replace took the keys from its field');
    const beforeAll = win.commands.length;
    ws().querySelector('[data-sheet-replace-all]').click(); await settle();
    const all = sent(beforeAll, 'sheetReplace').at(-1);
    assert(all && all.at === null && all.with === 'rates' && all.text === 'rent', `sent ${JSON.stringify(all)}`);
    // With words in Find already, ⌘⌥F puts the keys in Replace with.
    key('Escape'); await settle();
    key('f', {metaKey: true, altKey: true}); await settle();
    assert(doc.activeElement === swap, 'with words in Find, the keys did not go to Replace with');
    key('Escape'); await settle();
    field.value = '';
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
