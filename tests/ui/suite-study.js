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
    assert(studio.querySelectorAll('.st-tile').length === 14, 'the tiles are not all there');
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

  const deckMade = {id: 'D001', kind: 'flashcards', title: 'Flashcards: search', made: 1, by: 'Claude Code', task: 'J0004'};
  const flash = (/** @type {string} */ id, /** @type {string} */ kind, /** @type {string} */ front, /** @type {string} */ back) =>
    ({id, kind, front, back, topic: 'Search', objectives: [], cites: ['S01 p. 3'], task: 'J0004', added: 1, edited: null});
  const deckEvent = {type: 'studyDeck', made: deckMade, citations: [cite(1, 'S01 p. 3')],
    counts: {new: 1, learning: 0, review: 1, missed: 1, total: 2},
    cards: [{card: flash('C001', 'basic', 'Why is UCS optimal?', 'It expands by path cost [[S01 p. 3]].'), state: 'review', due: 1, recall: 0.84, reviews: 3, missed: 1},
      {card: flash('C002', 'cloze', 'A heuristic is {{c1::admissible::a word}} when $h(n) \\le h^*(n)$.', ''), state: 'new', due: null, recall: null, reviews: 0, missed: 0}]};
  const reviewing = (/** @type {any} */ card, /** @type {Partial<any>} */ more = {}) => ({type: 'studyReviewing', review: {deck: 'D001', title: 'Flashcards: search', mode: 'due',
    shuffle: false, card: card && {deck: 'D001', deckTitle: 'Flashcards: search', card, state: 'new', intervals: [60, 330, 600, 345600]},
    citations: [cite(1, 'S01 p. 3')], left: {new: 1, learning: 0, review: 1, missed: 1, total: 2}, remaining: 0, done: 0, again: 0, wait: null, undo: false, ...more}});

  await test('Flashcards are asked for by how many and how hard; a deck opens with what is due, and cards are written by hand', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01')]), {type: 'studyStudio', jobs: [], made: [deckMade]}]); await settle();
    const studio = ws().querySelector('[data-study-studio-body]');
    studio.querySelector('[data-study-tile="flashcards"]').click(); await settle();
    const form = studio.querySelector('[data-study-make-form="flashcards"]');
    form.querySelector('[name="amount"]').value = 'more';
    form.querySelector('[name="level"]').value = 'hard';
    const before = win.commands.length;
    form.requestSubmit(); await settle();
    const made = sent(before, 'studyMake')[0];
    assert(made?.kind === 'flashcards' && made.options.amount === 'more' && made.options.level === 'hard' && made.options.deck === null, `wrong make: ${JSON.stringify(made)}`);
    const open = win.commands.length;
    studio.querySelector('[data-study-made="D001"]').click(); await settle();
    assert(sent(open, 'studyShowMade')[0]?.id === 'D001', 'the deck was not asked for');
    await win.pushEvents([deckEvent]); await settle();
    const view = ws().querySelector('.st-deck-view');
    assert(view && view.querySelectorAll('.st-card-row').length === 2, 'the deck is not drawn');
    assert(view.querySelector('[data-study-review="due"]')?.textContent.includes('Study 2'), 'what is due is not offered');
    assert(view.querySelector('.st-card-row .st-cloze.shown')?.textContent.includes('admissible'), 'a cloze is not shown whole in the list');
    assert(view.querySelector('.st-card-row .st-cite'), 'a card\'s citation is not a chip');
    assert(view.querySelector('.st-card-row .st-state.review') && view.textContent.includes('84% remembered'), 'where a card is in being learnt is not said');
    // Added by hand.
    view.querySelector('[data-study-card-add]').click(); await settle();
    const card = ws().querySelector('[data-study-card-form="new"]');
    card.querySelector('[name="front"]').value = 'What is $h^*$?';
    card.querySelector('[name="back"]').value = 'The true cost to the goal.';
    const add = win.commands.length;
    card.requestSubmit(); await settle();
    const asked = sent(add, 'studyCard')[0];
    assert(asked?.deck === 'D001' && asked.card === null && asked.kind === 'basic' && asked.front === 'What is $h^*$?', `wrong card: ${JSON.stringify(asked)}`);
    // Edited, and taken out once asked.
    await win.pushEvents([deckEvent]); await settle();
    ws().querySelector('[data-study-card-edit="C001"]').click(); await settle();
    assert(ws().querySelector('[data-study-card-form="C001"] [name="front"]')?.value === 'Why is UCS optimal?', 'the card is not up to be edited');
    ws().querySelector('[data-study-card-cancel]').click(); await settle();
    ws().querySelector('[data-study-card-delete="C002"]').click(); await settle();
    const gone = win.commands.length;
    ws().querySelector('[data-study-card-delete-yes="C002"]').click(); await settle();
    assert(sent(gone, 'studyCardDelete')[0]?.card === 'C002', 'not taken out');
    // More cards for this deck, and out for Anki.
    await win.pushEvents([deckEvent]); await settle();
    ws().querySelector('[data-study-more-cards]').click(); await settle();
    const more = win.commands.length;
    ws().querySelector('[data-study-more-form]').requestSubmit(); await settle();
    assert(sent(more, 'studyMake')[0]?.options.deck === 'D001', 'more cards were not asked for this deck');
    const out = win.commands.length;
    ws().querySelector('[data-study-export="anki"]').click(); await settle();
    assert(sent(out, 'studyDeckExport')[0]?.anki === true, 'not written out for Anki');
  });

  await test('Review turns a card over with Space and rates it 1–4, each rating saying when it comes back; Z undoes, Esc ends', async () => {
    const start = win.commands.length;
    ws().querySelector('[data-study-review="due"]').click(); await settle();
    assert(JSON.stringify(sent(start, 'studyReview')[0]) === JSON.stringify({type: 'studyReview', deck: 'D001', mode: 'due', shuffle: false}), 'no session asked for');
    await win.pushEvents([reviewing(deckEvent.cards[1].card)]); await settle();
    const flash = () => ws().querySelector('.st-flash');
    assert(flash()?.querySelector('.st-cloze:not(.shown)')?.textContent === 'a word', 'the cloze is not hidden behind its hint');
    assert(!ws().querySelector('[data-study-rate]') && ws().querySelector('[data-study-flip]'), 'it can be rated before it is turned over');
    assert(ws().contains(doc.activeElement), 'the session does not have the keys');
    key(' '); await settle();
    assert(flash()?.querySelector('.st-cloze.shown')?.textContent.includes('admissible'), 'Space did not turn it over');
    const rates = [...ws().querySelectorAll('[data-study-rate]')].map((b) => b.textContent);
    assert(rates.length === 4 && rates[0].includes('Missed it') && rates[0].includes('1 min') && rates[1].includes('6 min') && rates[2].includes('Got it') && rates[3].includes('4 d'), `the ratings: ${rates}`);
    const rated = win.commands.length;
    key('3'); await settle();
    const rate = sent(rated, 'studyRate')[0];
    assert(rate?.rating === 'good' && typeof rate.took === 'number', `not rated: ${JSON.stringify(rate)}`);
    await win.pushEvents([reviewing(deckEvent.cards[0].card, {done: 1, undo: true})]); await settle();
    assert(ws().querySelector('[data-study-flip]') && flash()?.textContent.includes('Why is UCS optimal?'), 'the next card is not shown, front first');
    const undo = win.commands.length;
    key('z'); await settle();
    assert(sent(undo, 'studyReviewUndo').length === 1, 'Z did not take it back');
    // Explained in the chat; the session waits.
    const explain = win.commands.length;
    ws().querySelector('[data-study-explain]').click(); await settle();
    assert(sent(explain, 'studyAsk')[0]?.text.includes('Explain this flashcard') && sent(explain, 'studyAsk')[0]?.text.includes('[[S01 p. 3]]'), 'not explained');
    assert(!ws().querySelector('.st-flash') && ws().querySelector('[data-study-resume]'), 'the chat does not offer the session back');
    ws().querySelector('[data-study-resume]').click(); await settle();
    assert(ws().querySelector('.st-flash'), 'not back to the card');
    await win.pushEvents([reviewing(null, {done: 3, again: 1, wait: Math.floor(Date.now() / 1000) + 300, left: {new: 0, learning: 1, review: 0, missed: 1, total: 2}})]); await settle();
    assert(ws().querySelector('.st-review-done')?.textContent.includes('3 cards reviewed, 1 missed') && ws().textContent.includes('comes back at'), 'the end of the session is not said');
    const end = win.commands.length;
    key('Escape'); await settle();
    assert(sent(end, 'studyReviewEnd').length === 1, 'Esc did not end it');
    await win.pushEvents([{type: 'studyReviewing', review: null}, deckEvent]); await settle();
    assert(ws().querySelector('.st-deck-view') && !ws().querySelector('.st-review'), 'not back to the deck');
    ws().querySelector('[data-study-back]').click(); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  const quizMade = {id: 'Q001', kind: 'quiz', title: 'Quiz: search', made: 1, by: 'Claude Code', task: 'J0005'};
  const ask = (/** @type {string} */ id, /** @type {string} */ kind, /** @type {Partial<any>} */ more) => ({id, kind, prompt: '', options: [], correct: [], rationales: [], blanks: [], number: null,
    tolerance: 0, unit: '', rubric: [], modelAnswer: '', hint: '', explanation: '', topic: 'Search', objectives: ['O001'], level: 'medium', cites: [], ...more});
  const questions = [
    ask('q01', 'choice', {prompt: 'Which is complete?', options: ['DFS', 'BFS', 'Greedy'], correct: [1], rationales: ['Can descend for ever.', 'Layer by layer.', 'Can loop.'], hint: 'Layers.', explanation: 'BFS goes layer by layer [[S01 p. 3]].', cites: ['S01 p. 3']}),
    ask('q02', 'blanks', {prompt: 'A heuristic is ___ when it never overestimates.', blanks: [['admissible']]}),
    ask('q03', 'short', {prompt: 'Why is UCS optimal?', rubric: [{id: 'c1', text: 'orders by $g(n)$', points: 2}, {id: 'c2', text: 'costs above zero', points: 1}], modelAnswer: 'By $g(n)$, costs ≥ ε.'}),
  ];
  const quizEvent = (/** @type {any[]} */ attempts, /** @type {Partial<any>} */ more = {}) => ({type: 'studyQuiz', made: quizMade, questions, attempts, marking: [], results: [],
    mastery: [], citations: [cite(1, 'S01 p. 3')], drafts: [], sections: [], minutes: null, blueprint: '', source: null, ...more});
  const attempt = (/** @type {any[]} */ answers, /** @type {Partial<any>} */ more = {}) => ({n: 1, questions: ['q01', 'q02', 'q03'], answers, started: 1, finished: null, ends: null, score: 0, of: 0, ...more});
  const answered = (/** @type {string} */ question, /** @type {any} */ given, /** @type {any} */ grade, /** @type {Partial<any>} */ more = {}) =>
    ({question, given, hinted: false, grade, disputed: null, second: null, unmarked: null, at: 1, ...more});

  await test('A quiz opens when asked for, begins, and checks a choice at once, with why each option is right or wrong', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01')]), {type: 'studyStudio', jobs: [], made: [quizMade]}]); await settle();
    // Sent unasked — a mark coming back — it is not shown.
    await win.pushEvents([quizEvent([])]); await settle();
    assert(!ws().querySelector('.st-quiz-view'), 'a quiz no one asked for took the middle');
    const studio = ws().querySelector('[data-study-studio-body]');
    const open = win.commands.length;
    studio.querySelector('[data-study-made="Q001"]').click(); await settle();
    assert(sent(open, 'studyShowMade')[0]?.id === 'Q001', 'the quiz was not asked for');
    await win.pushEvents([quizEvent([])]); await settle();
    assert(ws().querySelector('.st-quiz-intro')?.textContent.includes('3 questions'), 'the quiz is not drawn');
    const begin = win.commands.length;
    ws().querySelector('[data-study-quiz-start="all"]').click(); await settle();
    assert(sent(begin, 'studyQuizStart')[0]?.missed === false, 'not begun');
    await win.pushEvents([quizEvent([attempt([])])]); await settle();
    assert(ws().querySelector('.st-quiz-progress')?.textContent.includes('Question 1 of 3'), 'the first question is not shown');
    ws().querySelector('[data-study-hint]').click(); await settle();
    assert(ws().querySelector('.st-hint')?.textContent.includes('Layers'), 'the hint is not shown');
    ws().querySelectorAll('.st-option input')[1].checked = true;
    const check = win.commands.length;
    ws().querySelector('[data-study-answer-form]').requestSubmit(); await settle();
    const answer = sent(check, 'studyQuizAnswer')[0];
    assert(answer?.question === 'q01' && answer.given.kind === 'choice' && answer.given.index === 1 && answer.hinted === true, `wrong answer sent: ${JSON.stringify(answer)}`);
    await win.pushEvents([quizEvent([attempt([answered('q01', {kind: 'choice', index: 1}, {score: 1, of: 1, by: 'rule', criteria: [], feedback: 'Layer by layer.'})], {score: 1, of: 1})])]); await settle();
    assert(ws().querySelector('.st-mark.right') && ws().querySelector('.st-option.correct.chosen'), 'the right answer is not marked right');
    assert(ws().querySelector('.st-explain')?.textContent.includes('layer by layer') && ws().querySelector('.st-explain .st-cite'), 'why is not shown, with its citation');
    assert(ws().querySelector('.st-dot-step.right'), 'the step is not marked');
  });

  await test('Blanks are typed in place; a written answer waits on its mark, which shows its criteria and can be disputed', async () => {
    ws().querySelector('[data-study-quiz-go="1"]').click(); await settle();
    const blank = ws().querySelector('.st-question .st-blank');
    assert(blank, 'the blank is not an input');
    blank.value = 'Admissible';
    const check = win.commands.length;
    ws().querySelector('[data-study-answer-form]').requestSubmit(); await settle();
    assert(JSON.stringify(sent(check, 'studyQuizAnswer')[0]?.given) === JSON.stringify({kind: 'blanks', words: ['Admissible']}), 'the blank was not sent');
    const one = answered('q01', {kind: 'choice', index: 1}, {score: 1, of: 1, by: 'rule', criteria: [], feedback: ''});
    const two = answered('q02', {kind: 'blanks', words: ['Admissible']}, {score: 1, of: 1, by: 'rule', criteria: [], feedback: ''});
    await win.pushEvents([quizEvent([attempt([one, two])])]); await settle();
    ws().querySelector('[data-study-quiz-go="2"]').click(); await settle();
    ws().querySelector('[data-study-answer-form] textarea').value = 'It orders the frontier by g(n). So the first goal is cheapest.';
    const write = win.commands.length;
    ws().querySelector('[data-study-answer-form]').requestSubmit(); await settle();
    assert(sent(write, 'studyQuizAnswer')[0]?.given.kind === 'text', 'the written answer was not sent');
    const three = answered('q03', {kind: 'text', text: 'It orders the frontier by g(n). So the first goal is cheapest.'}, null);
    await win.pushEvents([quizEvent([attempt([one, two, three])], {marking: ['1/q03']})]); await settle();
    assert(ws().querySelector('.st-marking')?.textContent.includes('Being marked'), 'nothing says it is being marked');
    const grade = {score: 2, of: 3, by: 'Codex · gpt', feedback: 'Say why costs must be positive.', criteria: [
      {id: 'c1', points: 2, evidence: ['It orders the frontier by g(n)'], why: 'Names g.'}, {id: 'c2', points: 0, evidence: [], why: 'Not said.'}]};
    await win.pushEvents([quizEvent([attempt([one, two, {...three, grade}])])]); await settle();
    const mark = ws().querySelector('.st-mark.part');
    assert(mark && mark.textContent.includes('2 / 3') && mark.textContent.includes('marked by Codex'), 'the mark is not shown');
    assert(mark.querySelector('.st-criteria q')?.textContent === 'It orders the frontier by g(n)', 'the words that earned it are not quoted');
    assert(ws().querySelector('.st-explain')?.textContent.includes('model answer'), 'the model answer is not offered');
    mark.querySelector('[data-study-dispute]').click(); await settle();
    const form = ws().querySelector('[data-study-dispute-form="q03"]');
    form.querySelector('textarea').value = 'My second line says why.';
    const dispute = win.commands.length;
    form.requestSubmit(); await settle();
    const disputed = sent(dispute, 'studyQuizDispute')[0];
    assert(disputed?.question === 'q03' && disputed.why === 'My second line says why.' && disputed.attempt === 1, `not disputed: ${JSON.stringify(disputed)}`);
  });

  await test('A finished quiz says how it went, by objective, and offers the missed, the weak areas and the tutor', async () => {
    const finish = win.commands.length;
    ws().querySelector('[data-study-quiz-finish]').click(); await settle();
    assert(sent(finish, 'studyQuizFinish')[0]?.attempt === 1, 'not finished');
    const done = attempt([answered('q01', {kind: 'choice', index: 0}, {score: 0, of: 1, by: 'rule', criteria: [], feedback: ''})], {finished: 2, score: 0, of: 1});
    await win.pushEvents([quizEvent([done], {results: [{objective: 'O001', text: 'Explain why UCS is optimal', topic: 'Search', score: 0, of: 1, questions: ['q01']}],
      mastery: [{objective: 'O001', text: 'Explain why UCS is optimal', topic: 'Search', level: 0.3, evidence: 2, state: 'focus'}]})]); await settle();
    assert(ws().querySelector('.st-quiz-score h2')?.textContent === '0%', 'the score is not shown');
    assert(ws().querySelector('.st-result')?.textContent.includes('Explain why UCS is optimal') && ws().querySelector('.st-stand.focus'), 'how it went by objective is not shown');
    const again = win.commands.length;
    ws().querySelector('[data-study-quiz-start="missed"]').click(); await settle();
    assert(sent(again, 'studyQuizStart')[0]?.missed === true, 'the missed were not tried again');
    await win.pushEvents([quizEvent([done], {results: [{objective: 'O001', text: 'Explain why UCS is optimal', topic: 'Search', score: 0, of: 1, questions: ['q01']}],
      mastery: [{objective: 'O001', text: 'Explain why UCS is optimal', topic: 'Search', level: 0.3, evidence: 2, state: 'focus'}]})]); await settle();
    const weak = win.commands.length;
    ws().querySelector('[data-study-quiz-weak]').click(); await settle();
    const make = sent(weak, 'studyMake')[0];
    assert(make?.kind === 'quiz' && JSON.stringify(make.options.objectives) === '["O001"]', `the weak areas were not asked for: ${JSON.stringify(make)}`);
    const tutor = win.commands.length;
    ws().querySelector('[data-study-quiz-results-ask]').click(); await settle();
    assert(sent(tutor, 'studyAsk')[0]?.text.includes('Here is how my attempt at "Quiz: search" went'), 'the tutor was not asked');
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  const examMade = {id: 'Q002', kind: 'exam', title: 'Practice final', made: 1, by: 'Codex', task: 'J0006'};
  const paperQuestions = [
    ask('q01', 'choice', {prompt: 'Which is complete?', options: ['DFS', 'BFS'], correct: [1], marks: 2}),
    ask('q02', 'long', {prompt: 'Prove A* optimal.', rubric: [{id: 'c1', text: 'sets up $G_2$', points: 4}, {id: 'c2', text: 'concludes', points: 4}], modelAnswer: '…', marks: 8}),
  ];
  const sections = [{title: 'Section A', instructions: 'Answer all.', questions: ['q01']}, {title: 'Section B', instructions: '', questions: ['q02']}];
  const examEvent = (/** @type {any[]} */ attempts, /** @type {Partial<any>} */ more = {}) => ({type: 'studyQuiz', made: examMade, questions: paperQuestions, attempts, marking: [], results: [],
    mastery: [], citations: [], drafts: [], sections, minutes: 90, blueprint: 'Two sections; **90 minutes**.', source: null, ...more});

  await test('A practice exam says what it is like and is sat against the clock, each answer saved as it is written', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01'), row('S02', {role: 'pastPaper'})]), {type: 'studyStudio', jobs: [], made: [examMade]}]); await settle();
    // The past paper tile offers the notebook's past papers.
    const studio = ws().querySelector('[data-study-studio-body]');
    studio.querySelector('[data-study-tile="pastPaper"]').click(); await settle();
    const pick = studio.querySelector('[data-study-make-form="pastPaper"] select[name="source"]');
    assert(pick && [...pick.options].map((o) => o.value).join() === 'S02', 'the past papers are not offered');
    const past = win.commands.length;
    studio.querySelector('[data-study-make-form="pastPaper"]').requestSubmit(); await settle();
    assert(sent(past, 'studyMake')[0]?.options.source === 'S02', 'the past paper was not asked for');
    studio.querySelector('[data-study-made="Q002"]').click(); await settle();
    await win.pushEvents([examEvent([])]); await settle();
    const intro = ws().querySelector('.st-paper-intro');
    assert(intro?.textContent.includes('90 minutes · 2 questions · 10 marks') && intro.querySelectorAll('.st-paper-sections li').length === 2, 'the paper is not described');
    const out = win.commands.length;
    intro.querySelector('[data-study-export-exam="scheme"]').click(); await settle();
    assert(JSON.stringify(sent(out, 'studyExamExport')[0]) === JSON.stringify({type: 'studyExamExport', quiz: 'Q002', html: true, scheme: true}), 'the marking scheme was not written out');
    intro.querySelector('[name="minutes"]').value = '45';
    const sit = win.commands.length;
    intro.querySelector('[data-study-sit-start]').requestSubmit(); await settle();
    assert(JSON.stringify(sent(sit, 'studyExamSit')[0]) === JSON.stringify({type: 'studyExamSit', quiz: 'Q002', minutes: 45}), 'not sat');
    const ends = Math.floor(Date.now() / 1000) + 45 * 60;
    const sitting = {n: 1, questions: ['q01', 'q02'], answers: [], started: 1, finished: null, ends, score: 0, of: 0};
    await win.pushEvents([examEvent([sitting])]); await settle();
    assert(/^4[45]:\d\d$/.test(ws().querySelector('[data-study-timer]')?.textContent ?? ''), `the clock: ${ws().querySelector('[data-study-timer]')?.textContent}`);
    assert(ws().querySelectorAll('.st-paper-section').length === 2 && ws().querySelectorAll('.st-sitting .st-question').length === 2, 'the paper is not on the page');
    assert(!ws().querySelector('[data-study-hint]') && !ws().querySelector('[data-study-answer-form]'), 'a sitting offers hints or checking');
    // Written, and saved once the writing pauses; what is being written stays.
    const area = ws().querySelector('[data-study-sit-form="q02"] textarea');
    area.focus();
    area.value = 'Suppose G2 is a suboptimal goal on the frontier.';
    area.dispatchEvent(new win.Event('input', {bubbles: true}));
    assert(ws().querySelector('[data-study-saved="q02"]')?.textContent === 'Saving…', 'nothing says it is being saved');
    const typed = win.commands.length;
    await new Promise((r) => setTimeout(r, 900)); await settle();
    const saved = sent(typed, 'studyQuizAnswer')[0];
    assert(saved?.attempt === 1 && saved.question === 'q02' && saved.given.text === 'Suppose G2 is a suboptimal goal on the frontier.', `not saved: ${JSON.stringify(saved)}`);
    area.value += ' Then';
    await win.pushEvents([examEvent([sitting], {drafts: [{question: 'q02', given: {kind: 'text', text: 'Suppose G2 is a suboptimal goal on the frontier.'}}]})]); await settle();
    assert(ws().querySelector('[data-study-sit-form="q02"] textarea').value.endsWith(' Then'), 'what was being written was drawn over');
    // Handed in, once asked.
    ws().querySelector('.st-sitting-bar [data-study-hand-in]').click(); await settle();
    const hand = win.commands.length;
    ws().querySelector('.st-sitting-bar [data-study-hand-in-yes]').click(); await settle();
    assert(sent(hand, 'studyQuizFinish')[0]?.attempt === 1, 'not handed in');
    const done = {...sitting, finished: 2, score: 7, of: 10, answers: [
      answered('q01', {kind: 'choice', index: 1}, {score: 2, of: 2, by: 'rule', criteria: [], feedback: ''}),
      answered('q02', {kind: 'text', text: 'Suppose…'}, {score: 5, of: 8, by: 'Claude Code', criteria: [], feedback: ''})]};
    await win.pushEvents([examEvent([done])]); await settle();
    const bySection = [...ws().querySelectorAll('.st-results .st-result')].map((r) => r.textContent);
    assert(bySection[0]?.includes('Section A') && bySection[0].includes('2/2') && bySection[1]?.includes('5/8'), `by section: ${bySection}`);
    assert(ws().querySelector('.st-quiz-score')?.textContent.includes('sat against the clock'), 'the sitting is not said');
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
