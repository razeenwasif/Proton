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

  const ws = () => $('.workspace[data-ws="study"]');
  const sent = (/** @type {number} */ from, /** @type {string} */ type) => win.commands.slice(from).filter((/** @type {any} */ c) => c.type === type);
  const row = (/** @type {string} */ id, /** @type {Partial<any>} */ more = {}) => ({id, title: `${id}.pdf`, kind: 'pdf', role: 'reading',
    origin: {from: 'file', root: 'Courses', rel: `COMP3670/${id}.pdf`}, status: {state: 'ready'}, units: 'pages', count: 3, size: 2_400_000,
    thin: 0, selected: true, labels: [], ...more});
  const notebook = (/** @type {any[]} */ sources, readOnly = null) => ({type: 'studyNotebook', slug: 'exam', title: 'COMP3670 exam', course: 'COMP3670',
    exam: '2026-11-12', readOnly, sources, notes: []});
  const kb = {type: 'studyKb', roots: [{label: 'Courses', path: '~/Courses', files: 3, readable: 2, inCloud: 0, missing: false},
    {label: 'ANU', path: '~/Library/Mobile Documents/com~apple~CloudDocs/ANU', files: 2, readable: 2, inCloud: 2, missing: false}],
    rules: ['Courses/Private/'], started: true, ready: true, walking: false, done: 5, total: 5, paused: false, documents: 2, passages: 9};

  await test('Notebook asks where notebooks are kept first, offering ~/Github/Notebooks', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([{type: 'studyList', notebooks: [], open: null, folder: null}, {type: 'studyClosed'}]); await settle();
    const form = ws().querySelector('[data-study-folder-form]');
    assert(form, 'no folder card');
    assert(form.querySelector('input').value === '~/Github/Notebooks', `it offers ${form.querySelector('input').value}`);
    const before = win.commands.length;
    form.querySelector('button[type="submit"]').click(); await settle();
    assert(sent(before, 'studyFolder')[0]?.folder === '~/Github/Notebooks', 'the folder was not asked for');
    await win.pushEvents([{type: 'studyList', notebooks: [], open: null, folder: '~/Github/Notebooks'}]); await settle();
    const title = ws().querySelector('[data-study-new-form] [name="title"]');
    assert(title, 'with no notebooks, the form for one');
    title.value = 'COMP3670 exam';
    const made = win.commands.length;
    ws().querySelector('[data-study-new-form]').requestSubmit(); await settle();
    assert(sent(made, 'studyNew')[0]?.title === 'COMP3670 exam', 'no notebook asked for');
  });

  await test('A notebook\'s sources are ticked for the tutor, and one in iCloud is downloaded only once its size is said', async () => {
    await win.pushEvents([{type: 'studyList', notebooks: [{slug: 'exam', title: 'COMP3670 exam', course: 'COMP3670', exam: '2026-11-12', sources: 3, opened: 1, archived: false}],
      open: 'exam', folder: '~/Github/Notebooks'},
      notebook([row('S01'), row('S02', {status: {state: 'reading'}, selected: false}), row('S03', {status: {state: 'inCloud'}, selected: false, size: 5_100_000})])]); await settle();
    const rows = [...ws().querySelectorAll('.st-source')];
    assert(rows.length === 3, `${rows.length} rows`);
    assert(rows[1].textContent.includes('Reading') && rows[1].querySelector('input').disabled, 'a source being read can be ticked');
    const before = win.commands.length;
    const tick = rows[0].querySelector('input');
    tick.checked = false; tick.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(JSON.stringify(sent(before, 'studySelect')[0]?.ids) === '[]', `wrong selection: ${JSON.stringify(sent(before, 'studySelect'))}`);
    const asked = win.commands.length;
    rows[2].querySelector('[data-study-get]').click(); await settle();
    assert(sent(asked, 'studyDownload').length === 0, 'it downloaded before saying how much');
    const ask = ws().querySelector('.st-ask');
    assert(ask?.textContent.includes('4.9 MB'), `the size is not said: ${ask?.textContent}`);
    ask.querySelector('[data-study-get-yes]').click(); await settle();
    assert(JSON.stringify(sent(asked, 'studyDownload')[0]?.paths) === '["Courses/COMP3670/S03.pdf"]', 'not downloaded by its path');
    ws().querySelector('[data-study-view="overview"]').click(); await settle();
    const read = ws().querySelector('[data-study-role="S01"]');
    read.value = 'pastPaper'; read.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(sent(asked, 'studyRole')[0]?.role === 'pastPaper', 'its role was not changed');
  });

  await test('The picker browses the knowledge base, says what is left out and why, and asks before bringing files down from iCloud', async () => {
    const before = win.commands.length;
    ws().querySelector('[data-study-pick]').click(); await settle();
    const picker = ws().querySelector('[data-study-picker]');
    assert(!picker.hidden, 'the picker did not open');
    await win.pushEvents([kb]); await settle();
    const browse = sent(before, 'studyKbBrowse').at(-1);
    assert(browse?.root === 'Courses', `it did not browse the first root: ${JSON.stringify(browse)}`);
    await win.pushEvents([{type: 'studyKbFolder', root: 'Courses', dir: '', folders: [{name: 'COMP3670', path: 'Courses/COMP3670', files: 2, readable: 2, inCloud: 0}],
      files: [{name: 'notes.md', path: 'Courses/notes.md', kind: 'markdown', size: 4000, mtime: 1, inCloud: false, readable: true, source: null},
        {name: 'S01.pdf', path: 'Courses/S01.pdf', kind: 'pdf', size: 4000, mtime: 1, inCloud: false, readable: true, source: 'S01'}],
      left: [{name: 'Private', path: 'Courses/Private', dir: true, why: 'left out by your rule Courses/Private/', rule: 'Courses/Private/'}]}]); await settle();
    assert(picker.querySelector('[data-study-choose="Courses/S01.pdf"]').disabled, 'a file already a source can be added again');
    const left = picker.querySelector('.st-left');
    assert(left?.textContent.includes('your rule'), 'what is left out is not said');
    const back = win.commands.length;
    left.querySelector('[data-study-let]').click(); await settle();
    assert(sent(back, 'studyExclude')[0]?.on === false && sent(back, 'studyExclude')[0]?.rule === 'Courses/Private/', 'the rule was not taken back');
    const choose = picker.querySelector('[data-study-choose="Courses/notes.md"]');
    choose.checked = true; choose.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    picker.querySelector('[data-study-root="ANU"]').click(); await settle();
    await win.pushEvents([{type: 'studyKbFolder', root: 'ANU', dir: '', folders: [], left: [],
      files: [{name: 'Exam 2022.pdf', path: 'ANU/Exam 2022.pdf', kind: 'pdf', size: 1_800_000, mtime: 1, inCloud: true, readable: true, source: null}]}]); await settle();
    assert(picker.querySelector('.st-cloud'), 'a file in iCloud has no cloud');
    const cloudy = picker.querySelector('[data-study-choose="ANU/Exam 2022.pdf"]');
    cloudy.checked = true; cloudy.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const foot = picker.querySelector('[data-study-foot]');
    assert(foot.textContent.includes('1 in iCloud (1.7 MB)'), `the footer does not say what is in iCloud: ${foot.textContent}`);
    const down = win.commands.length;
    foot.querySelector('[data-study-download]').click(); await settle();
    assert(sent(down, 'studyDownload').length === 0, 'it downloaded before asking');
    assert(foot.textContent.includes('Download 1 file from iCloud — 1.7 MB'), `it did not ask: ${foot.textContent}`);
    foot.querySelector('[data-study-confirm="yes"]').click(); await settle();
    assert(JSON.stringify(sent(down, 'studyDownload')[0]?.paths) === '["Courses/notes.md","ANU/Exam 2022.pdf"]', `wrong paths: ${JSON.stringify(sent(down, 'studyDownload'))}`);
    assert(picker.hidden, 'the picker stayed open');
  });

  await test('The picker searches names and words, and Escape puts it away', async () => {
    ws().querySelector('[data-study-pick]').click(); await settle();
    const picker = ws().querySelector('[data-study-picker]');
    const search = picker.querySelector('[data-study-search]');
    const before = win.commands.length;
    search.value = 'uniform cost'; search.dispatchEvent(new win.Event('input', {bubbles: true}));
    await new Promise((done) => setTimeout(done, 300));
    assert(sent(before, 'studyKbSearch')[0]?.query === 'uniform cost', 'no search');
    await win.pushEvents([{type: 'studyKbResults', query: 'uniform cost', files: [],
      passages: [{path: 'Courses/notes.md', cite: 'Courses/notes.md § Search L4-9', title: 'Search', text: 'Uniform-cost search orders the frontier by g(n).'}]}]); await settle();
    assert(picker.querySelector('.st-hit')?.textContent.includes('§ Search L4-9'), 'the passage is not shown with where it is');
    search.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Escape', bubbles: true})); await settle();
    assert(picker.hidden, 'Escape did not put it away');
  });

  await test('A source opens in the reader at the place asked for, and Back returns to the notebook', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01', {kind: 'markdown', units: 'lines', title: 'Search.md'})])]); await settle();
    const before = win.commands.length;
    ws().querySelector('[data-study-show="S01"]').click(); await settle();
    assert(sent(before, 'studyShow')[0]?.id === 'S01', 'the reader was not asked for');
    await win.pushEvents([{type: 'studySource', id: 'S01', title: 'Search.md', kind: 'markdown', at: {from: 7, to: 9}, said: 'Its file is in iCloud: these are its words as they were read.',
      view: {shape: 'sections', sections: [{from: 1, to: 6, text: '# Search\n\nBFS is complete.'}, {from: 7, to: 12, text: '## Uniform cost\n\nOrders by $g(n)$.'}]}}]); await settle();
    const marked = [...ws().querySelectorAll('.st-sec.at')];
    assert(marked.length === 1 && marked[0].textContent.includes('Uniform cost'), 'the cited section is not marked');
    assert(ws().querySelector('.st-reader .st-banner')?.textContent.includes('iCloud'), 'why its words are shown is not said');
    await win.pushEvents([{type: 'studySource', id: 'S01', title: 'Exam.pdf', kind: 'pdf', at: {from: 2, to: 2}, said: null,
      view: {shape: 'pages', pages: ['one', '', 'three']}}]); await settle();
    assert(ws().querySelector('.st-page.at')?.dataset.page === '2', 'the cited page is not marked');
    assert(ws().querySelector('.st-page[data-page="2"]')?.textContent.includes('No words'), 'an empty page is not said to be');
    ws().querySelector('[data-study-back]').click(); await settle();
    assert(!ws().querySelector('.st-reader') && ws().querySelector('.st-nb-head'), 'Back did not return to the notebook');
  });

  await test('A note is written, saved, and made a source', async () => {
    ws().querySelector('[data-study-new-note]').click(); await settle();
    const form = ws().querySelector('[data-study-note-form]');
    assert(form, 'no note to write');
    form.querySelector('textarea').value = '# Uniform cost\nOrders the frontier by $g(n)$.';
    const before = win.commands.length;
    form.requestSubmit(); await settle();
    const wrote = sent(before, 'studyNote')[0];
    assert(wrote && wrote.id === null && wrote.body.startsWith('# Uniform cost'), `wrong note: ${JSON.stringify(wrote)}`);
    await win.pushEvents([{type: 'studyNote', id: 'N001', title: 'Uniform cost', body: wrote.body, updated: 1, source: null}]); await settle();
    assert(ws().querySelector('[data-study-note-form]')?.dataset.id === 'N001', 'the saved note is not the one shown');
    const made = win.commands.length;
    ws().querySelector('[data-study-note-source]').click(); await settle();
    assert(sent(made, 'studyNoteSource')[0]?.id === 'N001', 'it was not made a source');
    ws().querySelector('[data-study-back]').click(); await settle();
  });

  const turn = (asked, answer, citations) => ({asked, answer, citations, at: 1, engine: 'claude', model: 'claude-sonnet-5-5', error: null});
  const cite = (n, raw, resolved = true) => ({n, raw, source: raw.split(' ')[0], title: 'Exam.pdf', units: 'pages', place: resolved ? {from: 3, to: 3} : {from: 99, to: 99},
    quote: resolved ? '## Question 3\nShow that UCS is optimal when each step costs at least $\\varepsilon$.' : '', resolved});
  const chatEvent = (more = {}) => ({type: 'studyChat', id: 'C001', title: 'Why is UCS optimal?', settings: {engine: 'claude', model: '', style: 'default', custom: '', length: 'default', language: ''},
    turns: [turn('Why is UCS optimal?', 'It orders by cost [[S01 p. 3]], unlike [[S01 p. 99]].', [cite(1, 'S01 p. 3'), cite(2, 'S01 p. 99', false)])],
    live: null, busy: false, error: null, grounding: 'tools', chats: [{id: 'C001', title: 'Why is UCS optimal?', updated: 1, turns: 1}], ...more});

  await test('The tutor\'s answers draw their citations as chips: over one, its passage; clicked, the source at that place', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01')]), chatEvent()]); await settle();
    ws().querySelector('[data-study-view="chat"]')?.click(); await settle();
    const chips = [...ws().querySelectorAll('.st-answer .st-cite')];
    assert(chips.length === 2 && chips[0].textContent === '1', `wrong chips: ${chips.map((c) => c.textContent)}`);
    assert(chips[1].classList.contains('bad'), 'a citation of nothing is not marked');
    assert(!ws().querySelector('.st-answer').textContent.includes('[['), 'the brackets show');
    chips[0].dispatchEvent(new win.MouseEvent('mouseover', {bubbles: true})); await settle();
    const tip = ws().querySelector('.st-tip');
    assert(!tip.hidden && tip.textContent.includes('Exam.pdf') && tip.textContent.includes('page 3'), `no passage: ${tip.textContent}`);
    assert(!tip.querySelector('h2'), 'a heading in a quote is drawn as a heading');
    const before = win.commands.length;
    chips[0].click(); await settle();
    const shown = sent(before, 'studyShow')[0];
    assert(shown?.id === 'S01' && shown.at?.from === 3, `wrong place: ${JSON.stringify(shown)}`);
    const nothing = win.commands.length;
    chips[1].click(); await settle();
    assert(sent(nothing, 'studyShow').length === 0, 'a citation of nothing opened something');
    await win.pushEvents([chatEvent()]); await settle();
  });

  await test('Asking sends the question, draws the answer as it comes, and Stop stops it', async () => {
    ws().querySelector('[data-study-view="chat"]')?.click(); await settle();
    const box = ws().querySelector('[data-study-ask-form] textarea');
    box.value = 'And A*?';
    const before = win.commands.length;
    box.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Enter', bubbles: true})); await settle();
    assert(sent(before, 'studyAsk')[0]?.text === 'And A*?', 'not asked');
    assert(box.value === '', 'the question stayed in the box');
    await win.pushEvents([chatEvent({busy: true})]); await settle();
    box.value = 'half a next question';
    await win.pushEvents([{type: 'studyChatLive', id: 'C001', live: {asked: 'And A*?', steps: [{kind: 'search', text: 'Searched for “A star”', done: true}],
      answer: 'A* adds a heuristic [[S01 p. 3]]', citations: [cite(1, 'S01 p. 3')], thinking: false}}]); await settle();
    const live = ws().querySelector('[data-study-live]');
    assert(live.textContent.includes('Searched for') && live.querySelector('.st-cite'), 'the answer coming is not drawn');
    assert(ws().querySelector('[data-study-ask-form] textarea').value === 'half a next question', 'what was being typed was lost');
    const stop = ws().querySelector('[data-study-stop]');
    assert(!stop.hidden, 'no Stop while it answers');
    const stopped = win.commands.length;
    stop.click(); await settle();
    assert(sent(stopped, 'studyStopAnswer').length === 1, 'Stop asked nothing');
    await win.pushEvents([chatEvent()]); await settle();
    ws().querySelector('[data-study-ask-form] textarea').value = '';
  });

  await test('Who answers, and how, is chosen in the tutor\'s menu; an answer can be kept as a note', async () => {
    await win.pushEvents([{type: 'claudeCapabilities', capabilities: {engine: 'codex', models: [{value: 'gpt-6.1-sol', resolved: '', name: 'GPT-6.1 Sol', description: '', efforts: []}],
      modes: [], commands: [], plan: '', email: '', missing: null}}]); await settle();
    ws().querySelector('[data-study-menu="tutor"]').click(); await settle();
    const form = ws().querySelector('[data-study-tutor-form]');
    assert(form, 'no menu');
    const codex = form.querySelector('input[name="engine"][value="codex"]');
    codex.checked = true; codex.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const fresh = ws().querySelector('[data-study-tutor-form]');
    assert(fresh.querySelector('select[name="model"] option[value="gpt-6.1-sol"]'), 'the engine\'s models are not offered');
    fresh.querySelector('select[name="model"]').value = 'gpt-6.1-sol';
    fresh.querySelector('input[name="style"][value="guide"]').checked = true;
    fresh.querySelector('input[name="language"]').value = 'French';
    const before = win.commands.length;
    fresh.requestSubmit(); await settle();
    const set = sent(before, 'studyTutor')[0]?.settings;
    assert(set?.engine === 'codex' && set.model === 'gpt-6.1-sol' && set.style === 'guide' && set.language === 'French', `wrong settings: ${JSON.stringify(set)}`);
    const kept = win.commands.length;
    ws().querySelector('[data-study-keep="0"]').click(); await settle();
    assert(sent(kept, 'studySaveAnswer')[0]?.turn === 0, 'the answer was not kept');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('The studio makes from the sources: a tile asks for its options, and a table its columns', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01')]), {type: 'studyStudio', jobs: [], made: []}]); await settle();
    const studio = ws().querySelector('[data-study-studio-body]');
    assert(studio.querySelectorAll('.st-tile').length === 9, 'the tiles are not all there');
    studio.querySelector('[data-study-tile="mindMap"]').click(); await settle();
    const form = studio.querySelector('[data-study-make-form="mindMap"]');
    form.querySelector('input').value = 'informed search';
    const before = win.commands.length;
    form.requestSubmit(); await settle();
    const made = sent(before, 'studyMake')[0];
    assert(made?.kind === 'mindMap' && made.options.focus === 'informed search', `wrong make: ${JSON.stringify(made)}`);
    studio.querySelector('[data-study-tile="table"]').click(); await settle();
    const table = studio.querySelector('[data-study-make-form="table"]');
    const none = win.commands.length;
    table.requestSubmit(); await settle();
    assert(sent(none, 'studyMake').length === 0, 'a table was asked for with no columns');
    table.querySelector('input').value = 'Algorithm, Complete?';
    table.requestSubmit(); await settle();
    assert(JSON.stringify(sent(none, 'studyMake')[0]?.options.columns) === '["Algorithm","Complete?"]', 'the columns were not sent');
  });

  await test('What is being made, what failed and what was made are listed; a map opens, folds, and shows what made it', async () => {
    const studio = ws().querySelector('[data-study-studio-body]');
    await win.pushEvents([{type: 'studyStudio', jobs: [
      {id: 'J0003', kind: 'faq', title: 'FAQ', state: 'running', why: null, made: null, engine: 'claude', started: 3},
      {id: 'J0002', kind: 'table', title: 'Table', state: 'failed', why: 'Its answer could not be read', made: null, engine: 'claude', started: 2}],
      made: [{id: 'M001', kind: 'mindMap', title: 'Search', made: 1, by: 'Claude Code', task: 'J0001'}]}]); await settle();
    const jobs = [...studio.querySelectorAll('.st-job')];
    assert(jobs.length === 2 && jobs[0].querySelector('.lab-spin') && jobs[1].textContent.includes('could not be read'), 'the calls are not drawn');
    const stop = win.commands.length;
    jobs[0].querySelector('[data-study-cancel]').click(); await settle();
    assert(sent(stop, 'studyCancelJob')[0]?.id === 'J0003', 'not cancelled');
    const before = win.commands.length;
    studio.querySelector('[data-study-made="M001"]').click(); await settle();
    assert(sent(before, 'studyShowMade')[0]?.id === 'M001', 'not opened');
    await win.pushEvents([{type: 'studyMadeOpen', made: {id: 'M001', kind: 'mindMap', title: 'Search', made: 1, by: 'Claude Code', task: 'J0001'},
      body: {shape: 'map', nodes: [{id: '1', parent: null, label: 'Search', cites: []}, {id: '2', parent: '1', label: 'Informed', cites: []},
        {id: '3', parent: '2', label: 'A*', cites: ['S01 p. 3']}, {id: '4', parent: '3', label: 'Admissible $h$', cites: []}]},
      citations: [cite(1, 'S01 p. 3')]}]); await settle();
    assert(ws().querySelectorAll('.st-map-node').length === 4, 'the map is not drawn');
    assert(ws().querySelector('.st-map .st-cite'), 'a node\'s citation is not a chip');
    const deep = [...ws().querySelectorAll('.st-map-node')].find((n) => n.querySelector(':scope > .st-map-label')?.textContent.startsWith('A*'));
    assert(deep?.hasAttribute('data-folded'), 'a deep branch is not folded at first');
    deep.querySelector('[data-study-fold]').click(); await settle();
    assert(!deep.hasAttribute('data-folded'), 'it did not unfold');
    const prompt = win.commands.length;
    ws().querySelector('[data-study-task="J0001"]').click(); await settle();
    assert(sent(prompt, 'studyShowTask')[0]?.id === 'J0001', 'the prompt was not asked for');
    await win.pushEvents([{type: 'studyTask', id: 'J0001', title: 'Search', engine: 'claude', model: 'm', state: 'done', why: null,
      instructions: 'Rules: cite everything.', prompt: 'Map the ideas of these sources.'}]); await settle();
    assert(ws().querySelector('.st-task-view')?.textContent.includes('Map the ideas'), 'the prompt is not shown');
    ws().querySelector('[data-study-back]').click(); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('A notebook another Proton has open is read only here', async () => {
    await win.pushEvents([notebook([row('S01')], 'another Proton (pid 4242)')]); await settle();
    assert(ws().querySelector('.st-banner')?.textContent.includes('read only'), 'nothing says it is read only');
    assert(!ws().querySelector('[data-study-pick]') && !ws().querySelector('[data-study-remove]'), 'it can still be changed');
    assert(ws().querySelector('[data-study-tick="S01"]').disabled, 'its sources can still be ticked');
    await win.pushEvents([notebook([row('S01')])]); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });
}
