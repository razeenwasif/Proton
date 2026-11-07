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

  await test('Logic sits after Circuit: ⌃7 opens it, in Codex\'s chrome and on an opaque ground', async () => {
    const asked = () => (win.invokes ?? []).filter((/** @type {any} */ c) => c.name === 'set_opaque').map((/** @type {any} */ c) => c.args.color);
    const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
    const before = win.commands.length, opaque = asked().length;
    /** @type {HTMLElement} */ (doc.body).focus();
    key('7', {ctrlKey: true}); await settle();
    assert(sent(before, 'setMode').some((c) => c.mode === 'logic'), '⌃7 did not ask for Logic');
    assert($('.workspace.on')?.dataset.ws === 'logic', 'Logic is not on screen');
    assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Logic kept the old chrome');
    assert(!shown($('.titlebar [data-term-toggle]')), 'Logic offers a terminal it cannot show');
    const tab = $('.titlebar [data-mode="logic"]');
    assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃7') && tab.textContent.trim() === 'Logic', 'the title bar does not show Logic');
    assert($('.mode-rail [data-mode="logic"]')?.textContent.trim() === 'Logic', 'the rail does not say Logic');
    assert(!asked().slice(opaque).includes(null) && asked().at(-1) === ground, `Logic is not on the ground: ${JSON.stringify(asked().slice(opaque))}`);
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

  await test('An argument shows its tableau, branch by branch, and a countermodel when it fails', async () => {
    const base = {...IMPLIES, generation: 4, line: 3, kind: 'argument', forms: [], column: null, sopTerms: [], posTerms: [], status: null, models: null, atoms: [], columns: [],
      open: [], counter: null, proofNote: 'closed with 1 free variable on a branch at most'};
    const tableau = {lines: [{n: 1, text: 'all x: man(x) -> mortal(x)', why: 'premise'}, {n: 2, text: 'man(socrates)', why: 'premise'},
      {n: 3, text: 'not mortal(socrates)', why: 'conclusion denied'}, {n: 4, text: 'man(socrates) -> mortal(socrates)', why: '1, γ'}], closed: null, open: false,
      split: [{lines: [{n: 5, text: 'not man(socrates)', why: '4, →'}], closed: [2, 5], open: false, split: []}, {lines: [{n: 6, text: 'mortal(socrates)', why: '4, →'}], closed: [3, 6], open: false, split: []}]};
    await win.pushEvents([{type: 'logicAnswer', answer: {...base, valid: true, tableau}}]); await settle();
    assert(answer().querySelector('.lg-verdict.good')?.textContent.includes('Valid'), 'a proved argument is not said valid');
    const ends = [...answer().querySelectorAll('.lg-tab-end')].map((e) => e.textContent.trim());
    assert(ends.join('|') === '× 2, 5|× 3, 6', `the branches' ends are not drawn: ${ends.join('|')}`);
    assert(answer().querySelectorAll('.lg-tab-split > .lg-tab').length === 2 && answer().textContent.includes('1, γ'), 'the split, or where a line came from, is not drawn');
    assert(answer().textContent.includes('closed with 1 free variable'), 'the prover\'s note is not said');
    // Refuted: a countermodel of two individuals.
    const counter = {sizes: [['individual', 2]], tables: [{name: 'P', shape: 'row', rows: [], cols: ['individual1', 'individual2'], cells: [['true', 'false']]}]};
    await win.pushEvents([{type: 'logicAnswer', answer: {...base, valid: false, tableau: null, counter, proofNote: 'no proof with up to 12 free variables on a branch'}}]); await settle();
    assert(answer().querySelector('.lg-verdict.bad')?.textContent.includes('Not valid') && answer().textContent.includes('A countermodel'), 'a refuted argument is not said so');
    assert(answer().textContent.includes('2 individuals') && answer().querySelector('.lg-model-table td.v1'), 'the countermodel is not shown');
    // Neither: said, and it can try longer.
    await win.pushEvents([{type: 'logicAnswer', answer: {...base, valid: null, tableau: null, more: true, proofNote: 'no proof found in the time'}}]); await settle();
    assert(answer().textContent.includes('Not decided'), 'an undecided argument is not said so');
    const before = win.commands.length;
    answer().querySelector('[data-logic-more]').click(); await settle();
    assert(sent(before, 'logicAsk')[0]?.more === true, 'Try harder did not ask again');
    // A propositional argument's open branch.
    const open = {lines: [{n: 1, text: 'q', why: 'premise'}], closed: null, open: true, split: []};
    await win.pushEvents([{type: 'logicAnswer', answer: {...base, valid: false, tableau: open, open: [['p', false], ['q', true]], counter: null}}]); await settle();
    assert(answer().querySelector('.lg-tab-end.open') && answer().textContent.includes('An open branch'), 'an open branch is not drawn and read');
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

  await test('A proof is coloured, its lines marked right or wrong, checked at the caret, and one found is written in', async () => {
    const proof = 'system fitch\npremises p -> q, q -> r\ngoal p -> r\n\n1  p -> q   premise\n2  q -> r   premise\n3  | p      assume\n4  | r      →E 2, 3\n';
    await win.pushEvents([file({path: 'week1/s.proof', name: 's', kind: 'proof', generation: 6, text: proof,
      diagnostics: [{line: 8, col: 0, message: '4: →E needs q, the antecedent of 2: line 3 is p'}], checks: [[5, true], [6, true], [7, true], [8, false]]})]);
    await settle();
    const gutter = [...ws().querySelectorAll('.lg-gutter span')];
    assert(gutter.filter((g) => g.classList.contains('ok')).map((g) => g.textContent).join() === '5,6,7', 'the right lines are not marked');
    assert(gutter.find((g) => g.textContent === '8')?.classList.contains('bad'), 'the wrong line is not marked');
    const hl = ws().querySelector('[data-logic-hl]');
    assert([...hl.querySelectorAll('.lg-rule')].map((r) => r.textContent).join() === 'premise,premise,assume,→E', 'the rules are not coloured');
    assert(hl.querySelectorAll('.lg-bar').length >= 2 && hl.querySelector('.lg-k')?.textContent === 'system', 'the bars and the header are not coloured');
    assert(ws().querySelector('[data-logic-kind]').textContent === 'proof', 'the file is not said to be a proof');
    const p = (/** @type {any} */ more) => ({n: 4, rule: '→E', ok: false, message: '→E needs q, the antecedent of 2: line 3 is p', complete: false, summary: '1 line is wrong', ...more});
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 6, line: 8, kind: 'proof', text: 'r', proof: p({}), deduction: null, deductionNote: null}}]);
    await settle();
    assert(answer().textContent.includes('Line 4 (→E)') && answer().textContent.includes('the antecedent of 2'), 'the wrong line\'s answer does not say what is wrong');
    assert(answer().textContent.includes('1 line is wrong'), 'the proof as a whole is not said');
    await caret(8);
    const before = win.commands.length;
    answer().querySelector('[data-logic-prove]').click(); await settle();
    const asked = sent(before, 'logicProve')[0];
    assert(asked?.generation === 6 && asked.line === 8, `Find a proof did not ask: ${JSON.stringify(asked)}`);
    const found = 'system fitch\npremises p -> q, q -> r\ngoal p -> r\n\n1  p -> q  premise\n2  q -> r  premise\n3  | p     assume\n4  | q     →E 1, 3\n5  | r     →E 2, 4\n6  p -> r  →I 3-5\n';
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 6, line: 8, kind: 'proof', text: 'r', proof: p({}), deduction: found, deductionNote: null}}]);
    await settle();
    assert(answer().querySelector('.lg-proof')?.textContent.includes('→I 3-5'), 'the proof found is not shown');
    const edits = win.commands.length;
    answer().querySelector('[data-logic-write]').click(); await settle();
    assert(text().value === 'system fitch\npremises p -> q, q -> r\ngoal p -> r\n\n' + found.split('\n\n')[1], `the proof found was not written in under the header: ${JSON.stringify(text().value)}`);
    assert(sent(edits, 'logicEdit').some((c) => c.text.includes('→I 3-5')), 'the proof written in was not sent');
  });

  await test('A valid argument offers a proof in each system, to copy as a .proof file', async () => {
    await win.pushEvents([file({generation: 7})]); await settle();
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 7, line: 3, kind: 'argument', text: '? p, p -> q |= q', valid: true, columns: [], atoms: [], tableau: null, open: [],
      deduction: null, deductionNote: null}}]);
    await settle();
    const asks = [...answer().querySelectorAll('[data-logic-prove]')];
    assert(asks.map((b) => b.textContent).join() === 'Natural deduction,Sequent calculus,Resolution', `the systems are not offered: ${asks.map((b) => b.textContent)}`);
    let before = win.commands.length;
    asks[0].click(); await settle();
    assert(sent(before, 'logicProve')[0]?.generation === 7 && sent(before, 'logicProve')[0].system === 'fitch', 'Natural deduction did not ask for a proof');
    before = win.commands.length;
    asks[2].click(); await settle();
    assert(sent(before, 'logicProve')[0]?.system === 'resolution', 'Resolution did not ask for a refutation');
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 7, line: 3, kind: 'argument', text: '? p, p -> q |= q', valid: true, columns: [], atoms: [], tableau: null, open: [],
      deduction: 'system fitch\npremises p, p -> q\ngoal q\n\n1  p       premise\n2  p -> q  premise\n3  q       →E 2, 1\n', deductionNote: null}}]);
    await settle();
    assert(answer().querySelector('.lg-proof')?.textContent.includes('→E 2, 1') && answer().querySelector('[data-logic-copy-proof]'), 'the proof found is not shown to copy');
    assert([...answer().querySelectorAll('h3')].some((h) => h.textContent === 'Natural deduction'), 'the proof\'s system is not named');
    assert(![...answer().querySelectorAll('[data-logic-prove]')].some((b) => b.dataset.logicProve === 'fitch'), 'the system shown is offered again');
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 7, line: 3, kind: 'argument', text: '? p, p -> q |= q', valid: true, columns: [], atoms: [], tableau: null, open: [],
      deduction: 'system resolution\npremises p, p -> q\ngoal q\n\n# Premise 2, p -> q:\n#   clauses:  not p or q\n\n1  p           premise 1\n2  not p or q  premise 2\n3  not q       goal\n4  q           resolve 1, 2\n5  false       resolve 3, 4\n', deductionNote: null}}]);
    await settle();
    const pre = answer().querySelector('.lg-proof');
    assert([...(pre?.querySelectorAll('.lg-c') ?? [])].some((c) => c.textContent.includes('clauses')) && [...pre.querySelectorAll('.lg-rule')].map((r) => r.textContent).join() === 'premise,premise,goal,resolve,resolve',
      `a refutation is not shown with its clause form: ${pre?.innerHTML}`);
    await win.pushEvents([{type: 'logicAnswer', answer: {...IMPLIES, generation: 7, line: 3, kind: 'argument', text: '? p, p -> q |= q', valid: true, columns: [], atoms: [], tableau: null, open: [],
      deduction: null, deductionNote: 'no proof found in the time'}}]);
    await settle();
    assert(answer().textContent.includes('No proof: no proof found in the time'), 'no proof found is not said');
  });

  await test('A file of 2,000 lines: a keystroke repaints only its line, and a new line only what moved', async () => {
    let big = 'person = size 40\nthing = 1..30\n';
    for (let i = 0; i < 1000; i++) big += `f${i}: person -> thing\nall x: f${i}(x) != ${i % 30} or some y: f${i}(y) > f${i}(x)\n`;
    await win.pushEvents([file({generation: 9, text: big})]); await settle();
    const t = text(), hl = ws().querySelector('[data-logic-hl]'), gutter = ws().querySelector('.lg-gutter');
    /** Logic's own time for a keystroke, the textarea laid out first: the median of fifteen. */
    const ours = (/** @type {string} */ piece) => {
      const times = [];
      for (let k = 0; k < 15; k++) {
        const at = t.value.length - 200 - k;
        t.setRangeText(piece, at, at, 'end');
        void t.scrollHeight;
        const s = win.performance.now();
        t.dispatchEvent(new win.Event('input', {bubbles: true}));
        void hl.offsetHeight; void gutter.offsetHeight;
        times.push(win.performance.now() - s);
      }
      return times.sort((a, b) => a - b)[7];
    };
    const typed = ours('x'), broken = ours('\n');
    assert(typed < 16, `a keystroke took ${typed.toFixed(1)} ms to repaint`);
    assert(broken < 30, `a new line took ${broken.toFixed(1)} ms to repaint`);
    // What is painted is what is typed, line for line, and numbered.
    const lines = t.value.split('\n');
    assert(hl.childElementCount === lines.length && [...hl.children].every((l, i) => l.textContent === (lines[i] || ' ')), 'the painted lines are not the typed ones');
    assert(gutter.childElementCount === lines.length + 1 && gutter.children[lines.length - 1].textContent === String(lines.length), 'the gutter does not number every line');
    t.setRangeText('', 0, t.value.length, 'end');
    t.dispatchEvent(new win.Event('input', {bubbles: true}));
    assert(hl.childElementCount === 1 && gutter.childElementCount === 2, 'emptied, the file still paints its old lines');
  });

  await test('Logic has the agent in its dock, shut until asked for, with @logic', async () => {
    await win.pushEvents([file({generation: 4})]); await settle();
    const dock = ws().querySelector('.xl-dock');
    assert(dock?.querySelector('[data-agent-input]') && dock.querySelector('[data-mention="@logic"]'), 'there is no agent composer with @logic');
    assert(!shown(dock), 'the dock is open before it is asked for');
    const toggle = [...doc.querySelectorAll('[data-panel-toggle="right"]')].find((b) => b.getClientRects().length);
    assert(toggle && !toggle.disabled, 'the dock cannot be shown');
    toggle.click(); await settle();
    assert(shown(dock) && shown(dock.querySelector('[data-agent-thread]')), 'the dock did not open');
    toggle.click(); await settle();
    assert(!shown(dock), 'the dock did not shut');
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
