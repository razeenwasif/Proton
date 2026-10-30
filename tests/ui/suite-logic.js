/* Logic's checks, run by suite.js in the same fixture window: its place on
 * the rail, the home page, a file typed and its lines asked, and the
 * answers drawn — verdicts, truth tables, forms, a Karnaugh map — and a
 * formula built in Circuit. The fixture transport answers none of these;
 * the events are pushed, shaped as the core's (crates/proton-core/src/logic.rs). */

const TEXT = '# notes\n? p -> q\n? q, p -> q |= p\n? p -> q == not q -> not p\nall x: f(x)\n';

/** The core's answer for `? p -> q`, as it makes it. */
const IMPLIES = {generation: 3, line: 2, kind: 'formula', text: '? p -> q', message: null, atoms: ['p', 'q'],
  columns: [{name: 'p -> q', values: '1101'}], status: 'contingent', models: 3, model: [false, false], falsifier: [true, false],
  valid: null, countermodel: null, same: null, differ: null,
  forms: [{name: 'Least sum of products', text: 'not p or q'}, {name: 'Least product of sums', text: 'not p or q'}, {name: 'NAND only', text: 'nand(p, nand(q, q))'}],
  exact: true, more: false, column: '1101', sopTerms: [[0, 2], [1, 1]], posTerms: [[2, 3]]};

/** @param {{test: Function, assert: Function, win: any, doc: Document, $: (s: string) => any, settle: () => Promise<void>, key: Function, shown: (el: any) => boolean}} t */
export async function logicChecks({test, assert, win, doc, $, settle, key, shown}) {
  const ws = () => $('.workspace[data-ws="logic"]');
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const file = (/** @type {Partial<any>} */ more = {}) => ({type: 'logicFile', file: {path: 'week1/sheet.logic', name: 'sheet', kind: 'logic', text: TEXT,
    dirty: false, generation: 3, diagnostics: [], notes: [], ...more}});
  const text = () => /** @type {HTMLTextAreaElement} */ (ws().querySelector('[data-logic-text]'));
  const answer = () => ws().querySelector('[data-logic-answer]');
  /** The caret put on a line, as a click there would. @param {number} line */
  const caret = async (line) => {
    const t = text();
    const at = t.value.split('\n').slice(0, line - 1).reduce((n, l) => n + l.length + 1, 0);
    t.focus();
    t.setSelectionRange(at, at);
    t.dispatchEvent(new win.MouseEvent('click', {bubbles: true}));
    await new Promise((r) => setTimeout(r, 300));
  };

  await test('Logic sits after Circuit: ⌃8 opens it, in Codex\'s chrome and on an opaque ground, and is the last', async () => {
    const asked = () => (win.invokes ?? []).filter((/** @type {any} */ c) => c.name === 'set_opaque').map((/** @type {any} */ c) => c.args.color);
    const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
    await win.dispatch({type: 'setMode', mode: 'chat'}); await settle();
    const before = win.commands.length, opaque = asked().length;
    /** @type {HTMLElement} */ (doc.body).focus();
    key('8', {ctrlKey: true}); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'logic'), '⌃8 did not ask for Logic');
    assert($('.workspace.on')?.dataset.ws === 'logic', 'Logic is not on screen');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Logic kept the old chrome');
    assert(!shown($('.titlebar [data-term-toggle]')), 'Logic offers a terminal it cannot show');
    const tab = $('.titlebar [data-mode="logic"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃8') && tab.textContent.trim() === 'Logic', 'the title bar does not show Logic');
    assert($('.mode-rail [data-mode="logic"]')?.textContent.trim() === 'Logic', 'the rail does not say Logic');
    assert(!asked().slice(opaque).includes(null) && asked().at(-1) === ground, `Logic is not on the ground: ${JSON.stringify(asked().slice(opaque))}`);
    const before9 = win.commands.length;
    key('9', {ctrlKey: true}); await settle();
    assert(!sent(before9, 'setMode').length, '⌃9 asked for a mode');
  });

  await test('With no file open, Logic lists the folder\'s and makes a worksheet or a theory', async () => {
    await win.pushEvents([{type: 'logicHome', files: [{path: 'week1/sheet.logic', name: 'sheet', kind: 'logic'}, {path: 'puzzles/pets.l4f', name: 'pets', kind: 'l4f'}]},
      {type: 'logicFile', file: null}, {type: 'logicAnswer', answer: null}]);
    await settle();
    assert(shown(ws().querySelector('[data-logic-home]')) && !shown(ws().querySelector('[data-logic-file]')), 'the home page is not what is shown');
    const rows = [...ws().querySelectorAll('[data-logic-list] [data-logic-open]')];
    assert(rows.map((r) => r.dataset.logicOpen).join() === 'week1/sheet.logic,puzzles/pets.l4f', 'the files are not listed');
    assert(rows[1].textContent.includes('Logic4Fun'), 'a Logic4Fun file does not say so');
    const before = win.commands.length;
    rows[0].click(); await settle();
    assert(sent(before, 'logicOpen')[0]?.path === 'week1/sheet.logic', 'clicking a file did not open it');
    const form = ws().querySelector('[data-logic-form]');
    form.querySelector('input').value = 'Week 4';
    form.querySelector('select').value = 'theory';
    form.requestSubmit(); await settle();
    const made = sent(before, 'logicNew')[0];
    assert(made?.name === 'Week 4' && made.kind === 'theory', `the form did not make a theory: ${JSON.stringify(made)}`);
  });

  await test('A file is typed in, coloured, its mistakes marked, and its lines asked as the caret moves', async () => {
    await win.pushEvents([file({diagnostics: [{line: 5, col: 0, message: 'f is not declared'}]})]); await settle();
    assert(shown(ws().querySelector('[data-logic-file]')) && text().value === TEXT, 'the file is not in the editor');
    const hl = ws().querySelector('[data-logic-hl]');
    assert(hl.querySelectorAll('.lg-q').length === 3 && hl.querySelector('.lg-k')?.textContent === 'not' && hl.querySelector('.lg-c')?.textContent === '# notes', 'the text is not coloured');
    assert(ws().querySelector('.lg-gutter .bad')?.textContent === '5', 'line 5\'s mistake is not marked');
    assert(ws().querySelector('[data-logic-diags]').textContent.includes('f is not declared'), 'the mistake is not said');
    // Opening asks the first question's line.
    let before = win.commands.length;
    await caret(3);
    const ask = sent(before, 'logicAsk').at(-1);
    assert(ask?.line === 3 && ask.generation === 3 && ask.more === false, `the caret on line 3 did not ask it: ${JSON.stringify(ask)}`);
    // The caret moving within the line asks nothing more.
    before = win.commands.length;
    text().setSelectionRange(text().selectionStart + 2, text().selectionStart + 2);
    text().dispatchEvent(new win.KeyboardEvent('keyup', {key: 'ArrowRight', bubbles: true}));
    await new Promise((r) => setTimeout(r, 300));
    assert(!sent(before, 'logicAsk').length, 'the same line was asked again');
    // Typing: the text sent once it settles, and its line asked again.
    before = win.commands.length;
    text().setRangeText(' or r', text().selectionStart, text().selectionStart, 'end');
    text().dispatchEvent(new win.Event('input', {bubbles: true}));
    assert(!sent(before, 'logicEdit').length, 'every keystroke is sent');
    await new Promise((r) => setTimeout(r, 400));
    const edit = sent(before, 'logicEdit').at(-1);
    assert(edit?.text.includes(' or r') && edit.generation === 3, 'what was typed was not sent');
    assert(sent(before, 'logicAsk').some((c) => c.line === 3), 'the edited line was not asked again');
    // A symbol button types at the caret.
    before = win.commands.length;
    ws().querySelector('[data-logic-sym="¬"]').click();
    await new Promise((r) => setTimeout(r, 400));
    assert(text().value.includes('¬'), 'the symbol was not typed');
    // ⌘⏎ asks now; ⌘S saves.
    before = win.commands.length;
    text().focus();
    key('Enter', {metaKey: true}); await settle();
    assert(sent(before, 'logicAsk').length === 1, '⌘⏎ did not ask');
    key('s', {metaKey: true}); await settle();
    assert(sent(before, 'logicSave').length === 1 && sent(before, 'logicEdit').length >= 1, '⌘S did not send the text and save');
    // The core's copy does not go back over what was just typed.
    const typed = text().value;
    await win.pushEvents([file({dirty: true})]); await settle();
    assert(text().value === typed && !ws().querySelector('[data-logic-dirty]').hidden, 'the core\'s older copy was put over the typing, or unsaved is not shown');
    await win.pushEvents([file()]);
  });

  await test('A formula is answered: its verdict, truth table, forms, Karnaugh map, and built in Circuit', async () => {
    await win.pushEvents([file({generation: 4, text: TEXT}), {type: 'logicAnswer', answer: {...IMPLIES, generation: 4}}]); await settle();
    const a = answer();
    assert(a.querySelector('.lg-verdict')?.textContent.includes('Contingent') && a.textContent.includes('true in 3 of 4'), 'the verdict is not said');
    const rows = [...a.querySelectorAll('.lg-table tbody tr')].map((r) => [...r.querySelectorAll('td')].map((d) => d.textContent).filter(Boolean).join(''));
    assert(rows.join() === '001,011,100,111', `the table is not p, q and p -> q: ${rows.join()}`);
    assert([...a.querySelectorAll('.lg-form-text')].map((f) => f.textContent).includes('nand(p, nand(q, q))'), 'the forms are not listed');
    assert(ws().querySelector('.lg-gutter .on')?.textContent === '2', 'the answered line is not marked');
    const canvas = /** @type {HTMLCanvasElement} */ (a.querySelector('[data-logic-kmap]'));
    const ink = canvas && [...canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data].filter((v, i) => i % 4 === 3 && v > 0).length;
    assert(ink > 200, 'the Karnaugh map is not drawn');
    const before = win.commands.length;
    a.querySelector('[data-logic-build="nand"]').click(); await settle();
    const built = sent(before, 'circuitFromTable')[0];
    assert(sent(before, 'setMode')[0]?.mode === 'circuit', 'Build in Circuit did not go to Circuit');
    assert(built && built.inputs.join() === 'p,q' && built.column === '1101' && built.style === 'nand' && built.project === 'logic', `not built from its table: ${JSON.stringify(built)}`);
    await win.dispatch({type: 'setMode', mode: 'logic'}); await settle();
    // Past ten propositions, the least forms wait to be asked for.
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 4, more: true, forms: []}}]); await settle();
    const more = win.commands.length;
    answer().querySelector('[data-logic-more]').click(); await settle();
    assert(sent(more, 'logicAsk')[0]?.more === true, 'Find the least forms did not ask for them');
  });

  await test('Arguments, equivalences, declarations and mistakes are each said, a counterexample row marked', async () => {
    const base = {...IMPLIES, generation: 4, forms: [], column: null, sopTerms: [], posTerms: [], status: null, models: null};
    await win.pushEvents([{type: 'logicAnswer', answer: {...base, line: 3, kind: 'argument', text: '? q, p -> q |= p', valid: false, countermodel: [true, false], atoms: ['q', 'p'],
      columns: [{name: 'q', values: '0011'}, {name: 'p -> q', values: '1011'}, {name: 'p', values: '0101'}]}}]);
    await settle();
    assert(answer().querySelector('.lg-verdict.bad')?.textContent.includes('Not valid') && answer().textContent.includes('A counterexample'), 'an invalid argument is not said so');
    const marked = [...answer().querySelectorAll('.lg-table tr.marked')];
    assert(marked.length === 1, `the counterexample's row is not the one marked: ${marked.length}`);
    await win.pushEvents([{type: 'logicAnswer', answer: {...base, line: 4, kind: 'same', same: true, columns: [{name: 'p -> q', values: '1101'}, {name: 'not q -> not p', values: '1101'}]}}]); await settle();
    assert(answer().querySelector('.lg-verdict.good')?.textContent.includes('Equivalent'), 'an equivalence is not said');
    await win.pushEvents([{type: 'logicAnswer', answer: {...base, line: 5, kind: 'declaration', text: 'person = Ann | Bo', message: 'person: 2 elements, in no order — Ann, Bo', columns: []}}]); await settle();
    assert(answer().textContent.includes('2 elements, in no order'), 'a declaration is not described');
    await win.pushEvents([{type: 'logicAnswer', answer: {...base, line: 5, kind: 'error', message: 'f is not declared', columns: []}}]); await settle();
    assert(answer().querySelector('.lg-verdict.bad')?.textContent.includes('Line 5') && answer().textContent.includes('f is not declared'), 'a mistake is not said');
    // An answer for another file is not shown.
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 99}}]); await settle();
    assert(answer().textContent.includes('f is not declared'), 'another file\'s answer was shown');
  });

  await test('A truth table typed in is read back as formulas', async () => {
    const before = win.commands.length;
    const details = answer().querySelector('.lg-from');
    details.open = true;
    const form = details.querySelector('form');
    form.elements.namedItem('names').value = 'a b c';
    form.elements.namedItem('column').value = '00010111';
    form.requestSubmit(); await settle();
    const asked = sent(before, 'logicFromTable')[0];
    assert(asked?.names === 'a b c' && asked.column === '00010111', 'the table was not sent');
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 4, line: 0, kind: 'table', text: '00010111', atoms: ['a', 'b', 'c'], columns: [],
      status: null, models: null, forms: [{name: 'Least sum of products', text: 'b and c or a and c or a and b'}], column: '00010111', sopTerms: [[3, 3], [5, 5], [6, 6]]}}]);
    await settle();
    assert(answer().textContent.includes('b and c or a and c or a and b') && answer().querySelector('[data-logic-kmap]'), 'the table\'s formulas are not shown');
    assert(answer().querySelector('.lg-from form').elements.namedItem('column').value === '00010111', 'what was typed into the table was lost');
  });

  await test('A theory\'s models are searched for, shown one at a time, counted, and stopped', async () => {
    await win.pushEvents([file({generation: 5, text: 'p = a | b\nf: p -> p\n'})]); await settle();
    let before = win.commands.length;
    text().focus();
    key('Enter', {metaKey: true, shiftKey: true}); await settle();
    const asked = sent(before, 'logicSolve')[0];
    assert(asked?.generation === 5 && asked.count === false && asked.keep >= 1, `⌘⇧⏎ did not search: ${JSON.stringify(asked)}`);
    assert(ws().querySelector('[data-logic-view="models"]')?.getAttribute('aria-selected') === 'true', 'the panel did not turn to the models');
    await win.pushEvents([{type: 'logicModels', models: {generation: 5, running: true, found: 0, finished: false, timedOut: false, counted: false, elapsedMs: 0, choices: 0, models: [], error: null, symmetry: false, conflict: null}}]);
    await settle();
    assert(answer().textContent.includes('Searching'), 'a search going is not said');
    before = win.commands.length;
    text().focus();
    key('Escape'); await settle();
    assert(sent(before, 'logicStop').length === 1, '⎋ did not stop the search');
    const table = (/** @type {string[]} */ row) => ({name: 'f', shape: 'row', rows: [], cols: ['a', 'b'], cells: [row]});
    await win.pushEvents([{type: 'logicModels', models: {generation: 5, running: false, found: 4, finished: false, timedOut: false, counted: false, elapsedMs: 3, choices: 7,
      models: [{sizes: [], tables: [table(['a', 'a'])]}, {sizes: [], tables: [table(['a', 'b'])]}], error: null, symmetry: true, conflict: null}}]);
    await settle();
    assert(answer().querySelector('.lg-nav')?.textContent.includes('Model 1 of'), 'the first model is not shown');
    assert([...answer().querySelectorAll('.lg-model-table td')].map((d) => d.textContent).join() === 'a,a', 'the model\'s table is not drawn');
    assert(answer().textContent.includes('shown once'), 'symmetry broken is not said');
    answer().querySelector('[data-logic-model="1"]').click(); await settle();
    assert([...answer().querySelectorAll('.lg-model-table td')].map((d) => d.textContent).join() === 'a,b', '› did not show the next model');
    before = win.commands.length;
    answer().querySelector('[data-logic-count]').click(); await settle();
    assert(sent(before, 'logicSolve')[0]?.count === true, 'Count them all did not count');
    await win.pushEvents([{type: 'logicModels', models: {generation: 5, running: false, found: 0, finished: true, timedOut: false, counted: true, elapsedMs: 1, choices: 2, models: [], error: null,
      symmetry: false, conflict: {lines: [2], kinds: [[2, 'f is one-to-one']], minimal: true}}}]);
    await settle();
    assert(answer().textContent.includes('No model: the constraints cannot all hold'), 'no model is not said so');
    assert(answer().querySelector('.lg-conflict')?.textContent.includes('f: p -> p') && answer().textContent.includes('f is one-to-one'), 'the conflicting lines are not listed with their text');
    assert(ws().querySelector('.lg-gutter .clash')?.textContent === '2', 'the conflicting line is not marked');
    // The line's answer is a tab away.
    ws().querySelector('[data-logic-view="line"]').click(); await settle();
    assert(!answer().textContent.includes('No model'), 'the line\'s tab still shows the models');
  });

  await test('Closing a file with unsaved changes asks first, then goes home', async () => {
    await win.pushEvents([file({generation: 4, dirty: true})]); await settle();
    const confirm = win.confirm;
    win.confirm = () => false;
    let before = win.commands.length;
    ws().querySelector('[data-logic-close]').click(); await settle();
    assert(!sent(before, 'logicClose').length, 'closing did not ask first');
    win.confirm = () => true;
    before = win.commands.length;
    ws().querySelector('[data-logic-close]').click(); await settle();
    win.confirm = confirm;
    assert(sent(before, 'logicClose')[0]?.discard === true, 'close did not close');
    await win.pushEvents([{type: 'logicFile', file: null}, {type: 'logicAnswer', answer: null}]); await settle();
    assert(shown(ws().querySelector('[data-logic-home]')), 'closing did not go home');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
