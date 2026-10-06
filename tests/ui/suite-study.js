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

  await test('Select all ticks every file of a folder that can be added, and With its subfolders every one below it', async () => {
    ws().querySelector('[data-study-pick]').click(); await settle();
    const picker = ws().querySelector('[data-study-picker]');
    // The last search put away, for the folder to show.
    const search = picker.querySelector('[data-study-search]');
    search.value = ''; search.dispatchEvent(new win.Event('input', {bubbles: true}));
    await new Promise((done) => setTimeout(done, 300));
    const file = (/** @type {string} */ name, /** @type {Partial<any>} */ more = {}) => ({name, path: `Courses/COMP3670/${name}`, kind: 'pdf', size: 1000, mtime: 1, inCloud: false, readable: true, source: null, ...more});
    await win.pushEvents([{type: 'studyKbFolder', root: 'Courses', dir: 'COMP3670', left: [],
      folders: [{name: 'Week 1', path: 'Courses/COMP3670/Week 1', files: 3, readable: 3, inCloud: 0}],
      files: [file('Exam_2020.pdf'), file('notes.md', {kind: 'markdown'}), file('S01.pdf', {source: 'S01'}), file('data.zip', {kind: 'other', readable: false})]}]); await settle();
    const all = () => picker.querySelector('[data-study-choose-all]');
    assert(picker.querySelector('.st-select-bar')?.textContent.includes('Select all 2 files here'), `not offered: ${picker.querySelector('.st-select-bar')?.textContent}`);
    all().checked = true; all().dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const foot = picker.querySelector('[data-study-foot]');
    assert(foot.textContent.includes('2 files ticked'), `not ticked: ${foot.textContent}`);
    const one = picker.querySelector('[data-study-choose="Courses/COMP3670/notes.md"]');
    one.checked = false; one.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(all().indeterminate && !all().checked, 'some ticked is not said to be some');
    all().checked = false; all().dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(foot.textContent.includes('Tick files'), 'unticking all left some');
    const before = win.commands.length;
    picker.querySelector('[data-study-choose-deep]').click(); await settle();
    const asked = sent(before, 'studyKbAll')[0];
    assert(asked?.root === 'Courses' && asked.dir === 'COMP3670', `not asked for the folder: ${JSON.stringify(asked)}`);
    await win.pushEvents([{type: 'studyKbAll', root: 'Courses', dir: 'COMP3670', total: 3,
      files: [file('Exam_2020.pdf'), file('notes.md', {kind: 'markdown'}), file('Week 1/lecture.pdf', {name: 'lecture.pdf'})]}]); await settle();
    assert(foot.textContent.includes('3 files ticked') && picker.textContent.includes('subfolders too'), `not every one below: ${foot.textContent}`);
    const add = win.commands.length;
    foot.querySelector('[data-study-add]').click(); await settle();
    assert(JSON.stringify(sent(add, 'studyAdd')[0]?.paths) === JSON.stringify(['Courses/COMP3670/Exam_2020.pdf', 'Courses/COMP3670/notes.md', 'Courses/COMP3670/Week 1/lecture.pdf']),
      `wrong paths: ${JSON.stringify(sent(add, 'studyAdd'))}`);
  });

  await test('Like this notebook finds what in the folders is like it and not in it yet, by words it shows in the box', async () => {
    ws().querySelector('[data-study-pick]').click(); await settle();
    const picker = ws().querySelector('[data-study-picker]');
    const discover = picker.querySelector('[data-study-discover]');
    assert(discover && !discover.hidden, 'nothing finds what is like the notebook');
    const before = win.commands.length;
    discover.click(); await settle();
    assert(sent(before, 'studyDiscover')[0]?.about === '', 'it did not ask');
    assert(picker.textContent.includes('Looking for what is like this notebook'), 'it does not say it is looking');
    await win.pushEvents([{type: 'studyKbResults', query: 'comp3670 frontier search', discovered: true,
      files: [{name: 'Week 4.pdf', path: 'Courses/COMP3670/Week 4.pdf', kind: 'pdf', size: 900_000, mtime: 1, inCloud: false, readable: true, source: null}],
      passages: [{path: 'Courses/COMP3670/Week 4.pdf', cite: 'Courses/COMP3670/Week 4.pdf p. 7', title: 'Week 4', text: 'A* orders the frontier by f(n) = g(n) + h(n).'}]}]); await settle();
    assert(picker.querySelector('[data-study-search]').value === 'comp3670 frontier search', 'the words it was found by are not in the box');
    const found = picker.querySelector('.st-found');
    assert(found?.textContent.includes('Week 4.pdf') && found.textContent.includes('p. 7 A* orders'), `not shown with its passage: ${found?.textContent}`);
    const tick = found.querySelector('[data-study-choose]');
    tick.checked = true; tick.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    const add = win.commands.length;
    picker.querySelector('[data-study-add]').click(); await settle();
    assert(JSON.stringify(sent(add, 'studyAdd')[0]?.paths) === '["Courses/COMP3670/Week 4.pdf"]', `not added: ${JSON.stringify(sent(add, 'studyAdd'))}`);
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

  await test('A page of the web is added by its address, read as its article, opened in the browser, or fetched again', async () => {
    await win.pushEvents([notebook([row('S01')])]); await settle();
    ws().querySelector('[data-study-view="overview"]').click(); await settle();
    ws().querySelector('[data-study-web]').click(); await settle();
    const form = ws().querySelector('[data-study-web-form]');
    assert(form && doc.activeElement === form.querySelector('input'), 'no address to type');
    const before = win.commands.length;
    form.requestSubmit(); await settle();
    assert(sent(before, 'studyAddUrls').length === 0, 'an empty address was sent');
    form.querySelector('input').value = 'en.wikipedia.org/wiki/Uniform-cost_search';
    form.requestSubmit(); await settle();
    assert(JSON.stringify(sent(before, 'studyAddUrls')[0]?.urls) === '["en.wikipedia.org/wiki/Uniform-cost_search"]', `not sent: ${JSON.stringify(sent(before, 'studyAddUrls'))}`);
    assert(!ws().querySelector('[data-study-web-form]'), 'the form stayed up');
    const url = 'https://en.wikipedia.org/wiki/Uniform-cost_search';
    await win.pushEvents([notebook([row('S01'), row('S02', {kind: 'web', units: 'lines', count: 42, title: 'Uniform-cost search', origin: {from: 'web', url, fetched: 1_790_000_000}}),
      row('S03', {kind: 'slides', title: 'Week 3.pptx', origin: {from: 'file', root: 'Courses', rel: 'Week 3.pptx'}})])]); await settle();
    ws().querySelector('[data-study-view="chat"]').click(); await settle();
    const web = ws().querySelector('.st-source[data-study-source="S02"]');
    assert(web?.title.includes(url) && web.querySelector('use')?.getAttribute('href') === '#i-globe', 'the page is not said to be from the web');
    await win.pushEvents([{type: 'studySource', id: 'S02', title: 'Uniform-cost search', kind: 'web', at: null, said: null,
      view: {shape: 'sections', sections: [{from: 1, to: 4, text: '# Uniform-cost search\n\nExpands the cheapest node first.'}]}}]); await settle();
    const head = ws().querySelector('.st-reader-head');
    assert(head.textContent.includes('fetched') && head.textContent.includes('2026'), `when it was fetched is not said: ${head.textContent}`);
    const opened = win.commands.length;
    head.querySelector('[data-study-page]').click(); await settle();
    assert(sent(opened, 'openLink')[0]?.url === url, 'the page did not open in the browser');
    head.querySelector('[data-study-refresh]').click(); await settle();
    assert(sent(opened, 'studyRefresh')[0]?.id === 'S02', 'it was not fetched again');
    await win.pushEvents([{type: 'studySource', id: 'S03', title: 'Week 3.pptx', kind: 'slides', at: {from: 2, to: 2}, said: null,
      view: {shape: 'pages', pages: ['Search', 'Uniform cost\n\nNotes: say it slowly.']}}]); await settle();
    assert(ws().querySelector('.st-page[data-page="2"] header')?.textContent === 'Slide 2', 'a deck\'s pages are not its slides');
    assert(!ws().querySelector('[data-study-page]'), 'a file is not a page to open');
    ws().querySelector('[data-study-back]').click(); await settle();
  });

  await test('Your artifacts are listed to add as sources, and one opens as itself, framed as the chat frames one', async () => {
    await win.pushEvents([notebook([row('S01')])]); await settle();
    ws().querySelector('[data-study-view="overview"]').click(); await settle();
    const before = win.commands.length;
    ws().querySelector('[data-study-artifacts]').click(); await settle();
    assert(sent(before, 'studyArtifacts').length === 1, 'the artifacts were not asked for');
    assert(ws().querySelector('[data-study-artifacts-form]')?.textContent.includes('Looking'), 'it does not say it is looking');
    const id = '4a1f9d2e-77b3-4c41-9e0a-5c2d8f1b6a30';
    const kept = (/** @type {Partial<any>} */ more) => ({id, title: 'Fleet grid frame times', description: '', icon: 'chart', seq: 2, at: 1_790_000_000_000, file: 'index.html', source: null, ...more});
    await win.pushEvents([{type: 'studyArtifacts', artifacts: [kept({}), kept({id: '9c3b7e21-4d8a-4f6b-a1e2-7b5c0d9e8f14', title: 'Telemetry glossary', source: 'S01'}),
      kept({id: '0b7e5a10-1c2d-4e3f-8a9b-0c1d2e3f4a5b', title: 'Published elsewhere', file: ''})]}]); await settle();
    const picks = [...ws().querySelectorAll('[data-study-artifact-pick]')];
    assert(picks.length === 3 && !picks[0].disabled && picks[1].disabled && picks[2].disabled, 'a source already, or one with no page, can be ticked');
    const form = () => ws().querySelector('[data-study-artifacts-form]');
    assert(form().textContent.includes('already S01') && form().textContent.includes('only its address is kept'), 'why one cannot be added is not said');
    assert(form().querySelector('[type=submit]').disabled, 'nothing ticked can be added');
    picks[0].checked = true; picks[0].dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(form().querySelector('[type=submit]').textContent.includes('Add 1 as a source'), 'the ticked are not counted');
    form().requestSubmit(); await settle();
    assert(JSON.stringify(sent(before, 'studyAddArtifacts')[0]?.ids) === JSON.stringify([id]), `not added: ${JSON.stringify(sent(before, 'studyAddArtifacts'))}`);
    assert(!form(), 'the list stayed up');

    await win.pushEvents([notebook([row('S01'), row('S02', {kind: 'artifact', units: 'lines', title: 'Fleet grid frame times', origin: {from: 'artifact', id, seq: 2, file: 'index.html'}})])]); await settle();
    ws().querySelector('[data-study-view="chat"]').click(); await settle();
    const source = ws().querySelector('.st-source[data-study-source="S02"]');
    assert(source?.title.includes('Artifact, version 2') && source.querySelector('use')?.getAttribute('href') === '#i-layers', 'the source is not said to be an artifact');
    await win.pushEvents([{type: 'studySource', id: 'S02', title: 'Fleet grid frame times', kind: 'artifact', at: null, said: null, guide: null, citations: [],
      view: {shape: 'sections', sections: [{from: 1, to: 3, text: '# Frame times\n\nBefore and after.'}]}}]); await settle();
    ws().querySelector('[data-study-artifact-open]').click(); await settle();
    const frame = ws().querySelector('.st-artifact-frame');
    assert(/^(artifact:\/\/localhost|http:\/\/artifact\.localhost|\/fixtures\/artifacts)\//.test(frame?.getAttribute('src') ?? '') && frame.getAttribute('src').endsWith(`/${id}/2/index.html`),
      `framed ${frame?.getAttribute('src')}`);
    const chat = doc.querySelector('[data-artifact-frame]');
    assert(frame.getAttribute('sandbox') === chat.getAttribute('sandbox') && frame.getAttribute('allow') === chat.getAttribute('allow'), 'not sandboxed as the chat sandboxes one');
    assert(frame.hasAttribute('allowfullscreen') && frame.getAttribute('allow').includes('fullscreen'), 'its videos cannot go full screen');
    ws().querySelector('[data-study-artifact-back]').click(); await settle();
    assert(ws().querySelector('.st-reader .st-sec')?.textContent.includes('Before and after'), 'Back did not return to its words');
    ws().querySelector('[data-study-back]').click(); await settle();
  });

  await test('A recording is transcribed only once what is sent to Fish, and what it costs, is said', async () => {
    const lecture = (/** @type {any} */ status) => row('S02', {kind: 'video', units: 'seconds', count: 3120, title: 'Lecture 5.mp4', role: 'recording',
      origin: {from: 'file', root: 'Courses', rel: 'COMP3670/Lecture 5.mp4'}, status});
    await win.pushEvents([notebook([row('S01'), lecture({state: 'untranscribed', seconds: 3120})])]); await settle();
    const badge = ws().querySelector('[data-study-transcribe="S02"]');
    assert(badge?.textContent.includes('Transcribe · 52 min'), `no way to transcribe it: ${badge?.textContent}`);
    assert(ws().querySelector('[data-study-tick="S02"]').disabled, 'a recording not yet heard can be ticked');
    const before = win.commands.length;
    badge.click(); await settle();
    assert(sent(before, 'studyTranscribe').length === 0, 'it was sent before what it costs was said');
    const ask = ws().querySelector('.st-ask');
    assert(ask?.textContent.includes('52 min recording') && ask.textContent.includes('about $0.31') && ask.textContent.includes('Fish Audio'), `the cost is not said: ${ask?.textContent}`);
    ask.querySelector('[data-study-transcribe-no]').click(); await settle();
    assert(!ws().querySelector('.st-ask') && sent(before, 'studyTranscribe').length === 0, 'Cancel did not cancel');
    ws().querySelector('[data-study-transcribe="S02"]').click(); await settle();
    ws().querySelector('[data-study-transcribe-yes]').click(); await settle();
    assert(sent(before, 'studyTranscribe')[0]?.id === 'S02', 'it was not transcribed when asked');
    await win.pushEvents([notebook([row('S01'), lecture({state: 'transcribing'})])]); await settle();
    assert(ws().querySelector('.st-source[data-study-source="S02"]')?.textContent.includes('Transcribing'), 'it is not said to be transcribing');
    await win.pushEvents([notebook([row('S01'), lecture({state: 'failed', why: 'Fish Audio says the account has no credit left for this.'})])]); await settle();
    const again = ws().querySelector('[data-study-transcribe="S02"]');
    assert(again?.textContent.includes('Not transcribed') && again.title.includes('no credit'), 'a failed transcription cannot be tried again');
  });

  await test('A deck\'s pages with few words are read from their pictures only once how many calls it takes is said', async () => {
    const week = (/** @type {number} */ thin) => row('S03', {title: 'Week 3.pdf', thin});
    await win.pushEvents([notebook([row('S01'), week(12)]), {type: 'studyStudio', jobs: [], made: []}]); await settle();
    const badge = ws().querySelector('[data-study-readable="S03"]');
    assert(badge?.textContent === '12 thin pages', `no way to read them: ${badge?.textContent}`);
    const before = win.commands.length;
    badge.click(); await settle();
    const ask = ws().querySelector('.st-ask');
    assert(ask?.textContent.includes('12 pages with few words') && ask.textContent.includes('2 calls'), `what it takes is not said: ${ask?.textContent}`);
    assert(sent(before, 'studyMakeReadable').length === 0, 'it was asked before it was said');
    ask.querySelector('[data-study-readable-yes]').click(); await settle();
    assert(sent(before, 'studyMakeReadable')[0]?.id === 'S03', 'they were not read when asked');
    await win.pushEvents([{type: 'studyStudio', made: [], jobs: [{id: 'J0012', kind: 'readable', title: 'Reading pages 2–9 of Week 3.pdf', state: 'running', why: null,
      made: null, engine: 'claude', started: 3, about: 'S03'}]}]); await settle();
    assert(ws().querySelector('.st-source[data-study-source="S03"]')?.textContent.includes('Reading pages'), 'it is not said to be reading them');
    await win.pushEvents([notebook([row('S01'), week(0)]), {type: 'studyStudio', jobs: [], made: []}]); await settle();
    assert(!ws().querySelector('[data-study-readable="S03"]'), 'read, they are still said to be thin');
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

  await test('Sources are labelled by topic and ticked by label; a source is given its guide on asking, whose topics ask the tutor', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    const search = (/** @type {string[]} */ labels) => row('S01', {kind: 'markdown', units: 'lines', title: 'Search.md', labels});
    const exam = (/** @type {string[]} */ labels) => row('S02', {title: 'Exam_2020.pdf', labels});
    const job = (/** @type {any} */ more) => ({type: 'studyStudio', made: [], jobs: [{id: 'J0010', kind: 'labels', title: 'Topic labels', state: 'running', why: null, made: null, engine: 'claude', started: 1, about: null, ...more}]});
    await win.pushEvents([notebook([search([]), exam([])]), {type: 'studyStudio', jobs: [], made: []}]); await settle();
    const before = win.commands.length;
    ws().querySelector('[data-study-label-all]').click(); await settle();
    assert(sent(before, 'studyMake')[0]?.kind === 'labels', 'labelling was not asked for');
    await win.pushEvents([job({})]); await settle();
    assert(ws().querySelector('.st-sources-head')?.textContent.includes('Labelling'), 'it is not said to be labelling');
    await win.pushEvents([notebook([search(['Search']), exam(['Exams', 'Search'])]), job({state: 'done', made: 'labels'})]); await settle();
    const chips = [...ws().querySelectorAll('[data-study-label-pick]')].map((c) => c.textContent);
    assert(JSON.stringify(chips) === '["Exams","Search"]', `the labels are not offered to tick by: ${JSON.stringify(chips)}`);
    assert(ws().querySelector('.st-source[data-study-source="S02"] .st-labels')?.textContent === 'ExamsSearch', 'a source does not show its labels');
    const tick = win.commands.length;
    ws().querySelector('[data-study-label-pick="Exams"]').click(); await settle();
    assert(JSON.stringify(sent(tick, 'studySelect')[0]?.ids) === '["S02"]', `not ticked by label: ${JSON.stringify(sent(tick, 'studySelect'))}`);

    const view = {shape: 'sections', sections: [{from: 1, to: 4, text: '# Search\n\nBFS is complete.'}]};
    await win.pushEvents([{type: 'studySource', id: 'S01', title: 'Search.md', kind: 'markdown', at: null, said: null, guide: null, citations: [], view}]); await settle();
    const ask = win.commands.length;
    ws().querySelector('[data-study-guide="S01"]').click(); await settle();
    const made = sent(ask, 'studyMake')[0];
    assert(made?.kind === 'sourceGuide' && made.options.source === 'S01', `the guide was not asked for: ${JSON.stringify(made)}`);
    const guiding = (/** @type {string} */ state) => ({type: 'studyStudio', made: [], jobs: [{id: 'J0011', kind: 'sourceGuide', title: 'Guide to Search.md', state, why: null,
      made: state === 'done' ? 'S01' : null, engine: 'claude', started: 2, about: 'S01'}]});
    await win.pushEvents([guiding('running')]); await settle();
    assert(ws().querySelector('.st-reader')?.textContent.includes('Writing its guide'), 'it is not said to be writing it');
    const shown = win.commands.length;
    await win.pushEvents([guiding('done')]); await settle();
    assert(sent(shown, 'studyShow')[0]?.id === 'S01', 'the source was not asked for again once its guide was written');
    await win.pushEvents([{type: 'studySource', id: 'S01', title: 'Search.md', kind: 'markdown', at: null, said: null, view, citations: [cite(1, 'S01 § Search L1-2')],
      guide: {summary: 'It shows breadth-first search is complete [[S01 § Search L1-2]].', topics: ['Breadth-first search'], questions: ['Is BFS optimal?'], made: 1, by: 'Claude Code', task: 'J0011'}}]); await settle();
    const guide = ws().querySelector('.st-guide');
    assert(guide?.querySelector('.st-cite') && guide.textContent.includes('Breadth-first search'), 'the guide is not shown, cited');
    const asked = win.commands.length;
    guide.querySelector('[data-study-ask-topic]').click(); await settle();
    assert(sent(asked, 'studyAsk')[0]?.text === 'What does Search.md (S01) say about Breadth-first search?', `the topic was not asked: ${JSON.stringify(sent(asked, 'studyAsk'))}`);
  });

  await test('The studio makes from the sources: a tile asks for its options, and a table its columns', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01')]), {type: 'studyStudio', jobs: [], made: []}]); await settle();
    const studio = ws().querySelector('[data-study-studio-body]');
    assert(studio.querySelectorAll('.st-tile').length === 20, 'the tiles are not all there');
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

  await test('Deep Research asks what to research; what it found is ticked to add, its report can be a source, and a page opens in the browser', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    const url = (/** @type {string} */ path) => `https://example.edu/${path}`;
    await win.pushEvents([notebook([row('S01'), row('S05', {kind: 'web', units: 'lines', title: 'UCS', origin: {from: 'web', url: url('ucs'), fetched: 1}})]),
      {type: 'studyStudio', jobs: [], made: []}]); await settle();
    const studio = ws().querySelector('[data-study-studio-body]');
    studio.querySelector('[data-study-tile="deepResearch"]').click(); await settle();
    const form = studio.querySelector('[data-study-make-form="deepResearch"]');
    assert(form?.textContent.includes('through Claude Code'), 'it does not say the web is searched through Claude Code');
    const before = win.commands.length;
    form.requestSubmit(); await settle();
    assert(sent(before, 'studyMake').length === 0, 'it was asked to research nothing');
    form.querySelector('[name="focus"]').value = 'uniform-cost search';
    form.requestSubmit(); await settle();
    const asked = sent(before, 'studyMake')[0];
    assert(asked?.kind === 'deepResearch' && asked.options.focus === 'uniform-cost search', `wrong asking: ${JSON.stringify(asked)}`);
    const made = {id: 'W001', kind: 'deepResearch', title: 'Uniform-cost search', made: 1, by: 'Claude Code · claude-opus-5-5', task: 'J0009'};
    await win.pushEvents([{type: 'studyMadeOpen', made, citations: [], body: {shape: 'found', title: 'Uniform-cost search',
      report: '# Uniform-cost search\n\nIt expands the cheapest node first [1], and is optimal when steps cost something [2].',
      found: [{title: 'UCS', url: url('ucs'), why: 'The definition.', kind: 'page'}, {title: 'Notes', url: url('notes.pdf'), why: 'Lecture notes.', kind: 'pdf'},
        {title: 'A lecture', url: url('lecture'), why: 'Worked by hand.', kind: 'video'}]}}]); await settle();
    const view = ws().querySelector('.st-made-view');
    assert(view.querySelector('.st-doc')?.textContent.includes('cheapest node first [1]'), 'the report is not shown');
    const rows = [...view.querySelectorAll('.st-found-row')];
    assert(rows[0].classList.contains('added') && rows[0].textContent.includes('added as S05') && rows[0].querySelector('input').disabled, 'a source already is not said to be');
    const add = () => ws().querySelector('[data-study-found-add]');
    assert(add().textContent === 'Add 2 as sources', `the rest are not ticked: ${add().textContent}`);
    const untick = ws().querySelector(`[data-study-found-pick="${url('notes.pdf')}"]`);
    untick.checked = false; untick.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(add().textContent === 'Add 1 as a source', `unticking does not count: ${add().textContent}`);
    const adding = win.commands.length;
    add().click(); await settle();
    assert(JSON.stringify(sent(adding, 'studyAddUrls')[0]?.urls) === JSON.stringify([url('lecture')]), `wrong sources added: ${JSON.stringify(sent(adding, 'studyAddUrls'))}`);
    ws().querySelector('[data-study-found-report]').click(); await settle();
    const pasted = sent(adding, 'studyPaste')[0];
    assert(pasted?.title === 'Uniform-cost search' && pasted.text.includes('## Sources') && pasted.text.includes(`2. Notes — ${url('notes.pdf')}`), `the report was not kept: ${JSON.stringify(pasted)}`);
    ws().querySelectorAll('.st-found-row [data-study-page]')[2].click(); await settle();
    assert(sent(adding, 'openLink')[0]?.url === url('lecture'), 'the page did not open in the browser');
    ws().querySelector('[data-study-back]').click(); await settle();
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

  await test('Progress is by learning objective: what to focus on, with a lesson or practice for each, and a diagnostic', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01')]), {type: 'studyStudio', jobs: [], made: []}]); await settle();
    const ask = win.commands.length;
    ws().querySelector('[data-study-view="progress"]').click(); await settle();
    assert(sent(ask, 'studyProgress').length === 1, 'the progress was not asked for');
    await win.pushEvents([{type: 'studyProgress', due: 0, objectives: [], quizzes: []}]); await settle();
    const make = win.commands.length;
    ws().querySelector('[data-study-progress-make="objectives"]').click(); await settle();
    assert(sent(make, 'studyMake')[0]?.kind === 'objectives', 'with no objectives, they are not offered');
    const mastery = (/** @type {string} */ objective, /** @type {string} */ state, /** @type {number | null} */ level, /** @type {string} */ topic = 'Search') =>
      ({objective, text: `objective ${objective}`, topic, level, evidence: level === null ? 0 : 2, state});
    await win.pushEvents([{type: 'studyProgress', due: 4, objectives: [mastery('O002', 'focus', 0.3), mastery('O003', 'notStarted', null, 'Heuristics'), mastery('O001', 'strong', 0.95)],
      quizzes: [{id: 'Q001', title: 'Quiz: search', kind: 'quiz', attempts: 2, best: 0.8, last: 0.6, finished: 1}]}]); await settle();
    const groups = [...ws().querySelectorAll('.st-standing-group h3')].map((h) => h.textContent);
    assert(groups[0]?.startsWith('Focus areas') && groups.some((g) => g.startsWith('Not started')) && groups.at(-1)?.startsWith('Strengths'), `the groups: ${groups}`);
    assert(ws().querySelector('.st-tally.focus b')?.textContent === '1' && ws().querySelector('.st-progress-go')?.textContent.includes('4 cards due today'), 'the tallies or the cards due are not shown');
    assert(ws().querySelector('.st-quiz-row')?.textContent.includes('best 80%'), 'how the quizzes went is not shown');
    const lesson = win.commands.length;
    ws().querySelector('[data-study-lesson="O002"]').click(); await settle();
    assert(sent(lesson, 'studyLesson')[0]?.objective === 'O002', 'no lesson asked for');
    const practise = win.commands.length;
    ws().querySelector('[data-study-practise="O003"]').click(); await settle();
    assert(JSON.stringify(sent(practise, 'studyMake')[0]?.options.objectives) === '["O003"]', 'no practice asked for');
    const diagnostic = win.commands.length;
    ws().querySelector('[data-study-progress-make="diagnostic"]').click(); await settle();
    assert(sent(diagnostic, 'studyMake')[0]?.kind === 'diagnostic', 'no diagnostic asked for');
  });

  await test('A lesson is a chat of its own, which says what it is on and ends in a check of three questions', async () => {
    const chatOf = (/** @type {any} */ lesson) => ({type: 'studyChat', id: 'C009', title: 'Lesson: objective O002', live: null, busy: false, error: null, grounding: 'tools', chats: [],
      settings: {engine: 'claude', model: '', style: 'guide', custom: '', length: 'default', language: ''}, turns: [], lesson});
    await win.pushEvents([chatOf({id: 'O002', text: 'Explain why UCS is optimal', topic: 'Search', cites: []})]); await settle();
    ws().querySelector('[data-study-view="chat"]').click(); await settle();
    const banner = ws().querySelector('.st-lesson');
    assert(banner?.textContent.includes('A lesson on O002') && banner.textContent.includes('Explain why UCS is optimal'), 'the lesson is not said');
    assert(ws().querySelector('.st-chat-head')?.textContent.includes('Learning guide'), 'the tutor is not a learning guide');
    const check = win.commands.length;
    banner.querySelector('[data-study-lesson-check]').click(); await settle();
    const made = sent(check, 'studyMake')[0];
    assert(made?.kind === 'quiz' && made.options.count === 3 && JSON.stringify(made.options.objectives) === '["O002"]', `no check asked for: ${JSON.stringify(made)}`);
    await win.pushEvents([chatOf(null)]); await settle();
    assert(!ws().querySelector('.st-lesson'), 'a chat that is no lesson says it is one');
  });

  await test('A Learning Overview teaches topic by topic: its cards turn over, its checks are marked at once, and it ends in a quiz', async () => {
    const overviewMade = {id: 'L001', kind: 'overview', title: 'Search, an overview', made: 1, by: 'Claude Code', task: 'J0007'};
    await win.pushEvents([{type: 'studyStudio', jobs: [], made: [overviewMade]}]); await settle();
    const open = win.commands.length;
    ws().querySelector('[data-study-made="L001"]').click(); await settle();
    assert(sent(open, 'studyShowMade')[0]?.id === 'L001', 'not asked for');
    await win.pushEvents([{type: 'studyOverview', made: overviewMade, deck: 'D004', quiz: 'Q005', citations: [cite(1, 'S01 p. 3')], topics: [
      {title: 'Uninformed search', summary: 'BFS goes layer by layer [[S01 p. 3]], so it is complete when b is finite.', map: [{id: '1', parent: null, label: 'Uninformed', cites: []}],
        cards: [flash('C001', 'basic', 'Is BFS complete?', 'Yes, when b is finite.')],
        check: [ask('k01', 'choice', {prompt: 'Which is complete?', options: ['DFS', 'BFS'], correct: [1], explanation: 'Layers.'})]},
      {title: 'Informed search', summary: 'A* orders by f(n) = g(n) + h(n); with an admissible h it is optimal.', map: [], cards: [],
        check: [ask('k01', 'blanks', {prompt: 'A heuristic that never overestimates is ___.', blanks: [['admissible']]})]}]}]); await settle();
    const view = ws().querySelector('.st-overview-view');
    assert(view && view.querySelectorAll('.st-ov-topic').length === 2 && view.querySelectorAll('.st-ov-nav button').length === 2, 'the overview is not drawn');
    assert(view.querySelector('.st-ov-topic .st-cite'), 'a summary\'s citation is not a chip');
    const card = view.querySelector('[data-study-ov-card]');
    card.click(); await settle();
    assert(card.classList.contains('flipped'), 'a card does not turn over');
    const checks = view.querySelectorAll('[data-study-check]');
    checks[0].querySelectorAll('.st-option input')[0].checked = true;
    checks[0].querySelector('[data-study-check-go]').click(); await settle();
    assert(checks[0].classList.contains('wrong') && checks[0].querySelectorAll('.st-option')[1].classList.contains('correct'), 'a wrong check is not marked, nor the right one shown');
    checks[1].querySelector('.st-blank').value = ' Admissible.';
    checks[1].querySelector('[data-study-check-go]').click(); await settle();
    assert(checks[1].classList.contains('right'), 'a right blank is not marked right');
    const quiz = win.commands.length;
    view.querySelector('.st-ov-end [data-study-made="Q005"]').click(); await settle();
    assert(sent(quiz, 'studyShowMade')[0]?.id === 'Q005', 'the quiz it ends in was not asked for');
    ws().querySelector('[data-study-back]')?.click(); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  const audioMade = {id: 'A001', kind: 'audio', title: 'Search, talked through', made: 1, by: 'Claude Code', task: 'J0008'};
  const said = [
    {speaker: 0, text: 'Today: search.', display: 'Today: search.', cites: []},
    {speaker: 1, text: 'It goes layer by layer.', display: 'BFS goes layer by layer [[S01 p. 3]].', cites: ['S01 p. 3']},
    {speaker: 0, text: 'And UCS orders by cost.', display: 'UCS orders by $g(n)$.', cites: []},
  ];
  const audioEvent = (/** @type {Partial<any>} */ more) => ({type: 'studyAudio', made: audioMade, format: 'deepDive', segments: said, chapters: [{title: 'Opening', segment: 0}, {title: 'Cost', segment: 2}],
    timings: [], duration: 0, state: 'waiting', why: null, done: 0, of: 0, url: null, citations: [cite(1, 'S01 p. 3')], voice: {key: true, consented: null, hosts: []}, ...more});

  await test('An audio overview is asked for by format, length and language, and nothing is spoken before Fish\'s terms are agreed to', async () => {
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01')]), {type: 'studyStudio', jobs: [], made: [audioMade]}]); await settle();
    const studio = ws().querySelector('[data-study-studio-body]');
    studio.querySelector('[data-study-tile="audio"]').click(); await settle();
    const form = studio.querySelector('[data-study-make-form="audio"]');
    form.querySelector('[name="format"]').value = 'debate';
    form.querySelector('[name="amount"]').value = 'fewer';
    form.querySelector('[name="language"]').value = 'French';
    const make = win.commands.length;
    form.requestSubmit(); await settle();
    const asked = sent(make, 'studyMake')[0];
    assert(asked?.kind === 'audio' && asked.options.format === 'debate' && asked.options.amount === 'fewer' && asked.options.language === 'French', `wrong: ${JSON.stringify(asked)}`);
    studio.querySelector('[data-study-made="A001"]').click(); await settle();
    await win.pushEvents([audioEvent({why: "Waiting for you to agree to Fish Audio's terms."})]); await settle();
    const consent = ws().querySelector('.st-voice-consent');
    assert(consent?.textContent.includes('may use what it is sent to improve its models') && consent.textContent.includes('30 November 2026'), 'the terms are not said');
    assert(ws().querySelectorAll('.st-line-said').length === 3, 'the transcript is not shown before it is spoken');
    const agree = win.commands.length;
    consent.querySelector('[data-study-voice-agree]').click(); await settle();
    assert(sent(agree, 'studyVoiceSetup')[0]?.consent === true, 'not agreed');
  });

  await test('Two voices are found, listened to and chosen; a missing key is said, and a voicing can be stopped', async () => {
    await win.pushEvents([audioEvent({voice: {key: false, consented: '2026-10-06', hosts: []}})]); await settle();
    assert(ws().querySelector('.st-audio-view')?.textContent.includes('FISH_API_KEY'), 'a missing key is not said');
    const look = win.commands.length;
    ws().querySelector('[data-study-voice-key]').click(); await settle();
    assert(sent(look, 'studyVoiceKey').length === 1, 'not looked for again');
    await win.pushEvents([audioEvent({voice: {key: true, consented: '2026-10-06', hosts: []}})]); await settle();
    const find = ws().querySelector('[data-study-voice-find]');
    find.querySelector('[name="query"]').value = 'narrator';
    const finding = win.commands.length;
    find.requestSubmit(); await settle();
    assert(JSON.stringify(sent(finding, 'studyVoiceFind')[0]) === JSON.stringify({type: 'studyVoiceFind', query: 'narrator', language: 'en', mine: false}), 'not found');
    await win.pushEvents([{type: 'studyVoices', query: 'narrator', total: 2, error: null, voices: [
      {id: 'v1', title: 'Warm Narrator', description: '', languages: ['en'], sample: true, sampleText: '', author: 'fish', uses: 10},
      {id: 'v2', title: 'Bright Host', description: '', languages: ['en'], sample: false, sampleText: '', author: '', uses: 5}]}]); await settle();
    assert(ws().querySelectorAll('.st-voice').length === 2, 'the voices are not listed');
    const listen = win.commands.length;
    ws().querySelector('[data-study-voice-listen="v1"]').click(); await settle();
    assert(sent(listen, 'studyVoiceSample')[0]?.id === 'v1', 'not listened to');
    await win.pushEvents([{type: 'studyVoiceSample', id: 'v1', url: '_voices/v1.mp3', error: null}]); await settle();
    assert(/^(study:\/\/localhost|http:\/\/study\.localhost)\/_voices\/v1\.mp3$/.test(ws().querySelector('.st-sample')?.getAttribute('src') ?? ''), `the sample's sound: ${ws().querySelector('.st-sample')?.getAttribute('src')}`);
    ws().querySelector('[data-study-voice-pick="v1"]').click(); await settle();
    ws().querySelector('[data-study-voice-pick="v2"]').click(); await settle();
    const use = win.commands.length;
    ws().querySelector('[data-study-voice-use]').click(); await settle();
    const setup = sent(use, 'studyVoiceSetup')[0];
    assert(setup?.consent === true && setup.hosts.map((/** @type {any} */ h) => h.id).join() === 'v1,v2', `not chosen: ${JSON.stringify(setup)}`);
    await win.pushEvents([audioEvent({state: 'voicing', done: 3, of: 12, voice: {key: true, consented: '2026-10-06', hosts: [{id: 'v1', title: 'Warm Narrator'}, {id: 'v2', title: 'Bright Host'}]}})]); await settle();
    assert(ws().querySelector('.st-voicing')?.textContent.includes('3 of 12'), 'how far it is spoken is not said');
    const stop = win.commands.length;
    ws().querySelector('[data-study-voice-stop]').click(); await settle();
    assert(sent(stop, 'studyCancelJob')[0]?.id === 'A001', 'not stopped');
  });

  await test('A spoken overview plays from study://, its transcript follows the voice, and a chapter, a turn or a speed is chosen', async () => {
    const hosts = [{id: 'v1', title: 'Warm Narrator'}, {id: 'v2', title: 'Bright Host'}];
    await win.pushEvents([audioEvent({state: 'ready', url: 'search/A001.mp3', duration: 30, timings: [[0, 10], [10, 20], [20, 30]], voice: {key: true, consented: '2026-10-06', hosts}})]); await settle();
    const sound = ws().querySelector('[data-study-sound]');
    assert(/^(study:\/\/localhost|http:\/\/study\.localhost)\/search\/A001\.mp3$/.test(sound?.getAttribute('src') ?? ''), `the sound: ${sound?.getAttribute('src')}`);
    assert(ws().querySelector('.st-line-said.speaker1 b')?.textContent === 'Bright Host', 'a turn does not say whose voice it is');
    assert(ws().querySelector('.st-line-said .st-cite'), 'a turn\'s citation is not a chip');
    ws().querySelector('[data-study-segment="1"]').click(); await settle();
    assert(Math.abs(sound.currentTime - 10) < 0.01 && ws().querySelector('.st-line-said.now')?.getAttribute('data-study-segment') === '1', 'a turn clicked is not where it plays from');
    ws().querySelector('[data-study-chapter="2"]').click(); await settle();
    assert(Math.abs(sound.currentTime - 20) < 0.01 && ws().querySelector('.st-line-said.now')?.getAttribute('data-study-segment') === '2', 'a chapter is not jumped to');
    const speed = ws().querySelector('[data-study-speed]');
    speed.value = '1.5';
    speed.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
    assert(sound.playbackRate === 1.5, 'the speed is not set');
    // Drawn again while it plays, the same sound is kept.
    await win.pushEvents([audioEvent({state: 'ready', url: 'search/A001.mp3', duration: 30, timings: [[0, 10], [10, 20], [20, 30]], voice: {key: true, consented: '2026-10-06', hosts}})]); await settle();
    assert(ws().querySelector('[data-study-sound]') === sound, 'the sound was drawn over');
    ws().querySelector('[data-study-back]').click(); await settle();
    await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  });

  await test('The voices and Fish\'s key are set from the Audio overview tile, a voice found by its link or among your own', async () => {
    const studio = ws().querySelector('[data-study-studio-body]');
    studio.querySelector('[data-study-tile="audio"]').click(); await settle();
    const before = win.commands.length;
    studio.querySelector('[data-study-voices]').click(); await settle();
    assert(sent(before, 'studyVoiceKey').length === 1, 'the key was not looked for');
    const view = () => ws().querySelector('.st-voices-view');
    assert(view(), 'the voices are not shown on their own');
    await win.pushEvents([{type: 'studyVoiceState', voice: {key: false, consented: '2026-10-06', hosts: []}}]); await settle();
    assert(view().textContent.includes('~/.zshrc') && view().querySelector('[data-study-voice-key]'), 'where the key goes is not said');
    const host = (/** @type {string} */ id, /** @type {string} */ title) => ({id, title});
    await win.pushEvents([{type: 'studyVoiceState', voice: {key: true, consented: '2026-10-06', hosts: [host('v1', 'Warm Narrator'), host('v2', 'Bright Host')]}}]); await settle();
    assert(view().textContent.includes('key is set') && view().textContent.includes('Warm Narrator') && view().textContent.includes('Bright Host'), `the voices are not said: ${view().textContent}`);
    view().querySelector('[data-study-voice-change]').click(); await settle();
    const find = view().querySelector('[data-study-voice-find]');
    const link = 'https://fish.audio/m/802e3bc2b27e49c2995d23ef70e6ac89';
    find.querySelector('[name="query"]').value = link;
    find.querySelector('[name="mine"]').checked = true;
    const finding = win.commands.length;
    find.requestSubmit(); await settle();
    const asked = sent(finding, 'studyVoiceFind')[0];
    assert(asked?.query === link && asked.mine === true, `not found by its link among your own: ${JSON.stringify(asked)}`);
    const voice = (/** @type {string} */ id, /** @type {string} */ title) => ({id, title, description: '', languages: ['en'], sample: false, sampleText: '', author: 'me', uses: 1});
    await win.pushEvents([{type: 'studyVoices', query: link, total: 2, error: null, voices: [voice('802e3bc2b27e49c2995d23ef70e6ac89', 'My lecture voice'), voice('v2', 'Bright Host')]}]); await settle();
    view().querySelector('[data-study-voice-pick="802e3bc2b27e49c2995d23ef70e6ac89"]').click(); await settle();
    view().querySelector('[data-study-voice-pick="v2"]').click(); await settle();
    const use = win.commands.length;
    view().querySelector('[data-study-voice-use]').click(); await settle();
    const setup = sent(use, 'studyVoiceSetup')[0];
    assert(setup?.hosts.map((/** @type {any} */ h) => h.title).join() === 'My lecture voice,Bright Host', `not chosen: ${JSON.stringify(setup)}`);
    view().querySelector('[data-study-back]').click(); await settle();
    assert(!view(), 'Back did not leave the voices');
    studio.querySelector('[data-study-make-cancel]')?.click(); await settle();
  });

  await test('A deck shows a slide at a time with its notes, moves with the keys, revises a slide, presents, and goes out for PowerPoint', async () => {
    const deckMade = {id: 'P001', kind: 'slides', title: 'Search', made: 1, by: 'Claude Code', task: 'J0009'};
    await win.dispatch({type: 'setMode', mode: 'study'}); await settle();
    await win.pushEvents([notebook([row('S01')]), {type: 'studyStudio', jobs: [], made: [deckMade]}]); await settle();
    ws().querySelector('[data-study-made="P001"]').click(); await settle();
    const slide = (/** @type {string} */ id, /** @type {string} */ layout, /** @type {string} */ title, /** @type {string[]} */ points, /** @type {Partial<any>} */ more = {}) =>
      ({id, layout, title, points, right: [], notes: '', cites: [], ...more});
    await win.pushEvents([{type: 'studySlides', made: deckMade, citations: [cite(1, 'S01 p. 3')], slides: [
      slide('s01', 'title', 'Search', ['How agents find their way']),
      slide('s02', 'points', 'BFS', ['Layer by layer', 'Complete when $b$ is finite [[S01 p. 3]]'], {notes: 'Say why [[S01 p. 3]].', cites: ['S01 p. 3']}),
      slide('s03', 'two', 'BFS or DFS', ['Complete'], {right: ['Not complete']})]}]); await settle();
    assert(ws().querySelector('.st-slide.title h1')?.textContent === 'Search' && ws().querySelectorAll('.st-slide-thumb').length === 3, 'the deck is not drawn');
    assert(ws().contains(doc.activeElement), 'the deck does not have the keys');
    key('ArrowRight'); await settle();
    assert(ws().querySelector('.st-slide.points li:nth-child(2)')?.textContent.includes('Complete when') && !ws().querySelector('.st-slide')?.textContent.includes('S01'), 'the next slide is not shown, or shows its citation');
    assert(ws().querySelector('.st-slide-notes')?.textContent.includes('Say why') && ws().querySelector('.st-slide-notes .st-cite'), 'its notes are not shown with their citation');
    ws().querySelector('[data-study-slide-revise-open]').click(); await settle();
    const form = ws().querySelector('[data-study-slide-revise="s02"]');
    form.querySelector('textarea').value = 'fewer words';
    const revise = win.commands.length;
    form.requestSubmit(); await settle();
    const asked = sent(revise, 'studyMake')[0];
    assert(asked?.kind === 'slides' && asked.options.slides === 'P001' && asked.options.slide === 's02' && asked.options.ask === 'fewer words', `not revised: ${JSON.stringify(asked)}`);
    ws().querySelector('[data-study-slide-go="2"]').click(); await settle();
    assert(ws().querySelectorAll('.st-slide.two ul').length === 2, 'two columns are not two');
    ws().querySelector('[data-study-present]').click(); await settle();
    assert(ws().querySelector('.st-slide-frame.presenting'), 'it does not present');
    key('Escape'); await settle();
    assert(!ws().querySelector('.st-slide-frame.presenting'), 'Escape does not stop presenting');
    const out = win.commands.length;
    ws().querySelector('[data-study-slides-out="pptx"]').click(); await settle();
    assert(JSON.stringify(sent(out, 'studySlidesExport')[0]) === JSON.stringify({type: 'studySlidesExport', id: 'P001', pptx: true}), 'not written out');
  });

  await test('An infographic is drawn from its template and saved as a PNG drawn here; so is a mind map', async () => {
    const graphicMade = {id: 'G001', kind: 'infographic', title: 'Search, at a glance', made: 1, by: 'Claude Code', task: 'J0010'};
    await win.pushEvents([{type: 'studyStudio', jobs: [], made: [graphicMade]}]); await settle();
    ws().querySelector('[data-study-made="G001"]').click(); await settle();
    await win.pushEvents([{type: 'studyGraphic', made: graphicMade, style: 'numbers', subtitle: 'Four ways through', takeaway: 'Order by cost.', citations: [cite(1, 'S01 p. 3')], blocks: [
      {heading: 'BFS', figure: 'O(b^d)', text: 'Layer by layer, complete when b is finite.', cites: ['S01 p. 3']},
      {heading: 'UCS', figure: 'g(n)', text: 'Cheapest path first.', cites: []}]}]); await settle();
    const svg = ws().querySelector('[data-study-graphic] svg');
    assert(svg && svg.textContent.includes('Search, at a glance') && svg.textContent.includes('O(b^d)') && svg.textContent.includes('Order by cost.'), 'the infographic is not drawn');
    assert(ws().querySelector('.st-graphic-sources .st-cite'), 'where a block is from is not said');
    const save = win.commands.length;
    ws().querySelector('[data-study-picture="G001"]').click();
    await new Promise((r) => setTimeout(r, 600)); await settle();
    const png = sent(save, 'studySaveImage')[0];
    assert(png?.id === 'G001' && png.png.startsWith('data:image/png;base64,iVBOR'), `no PNG was drawn: ${String(png?.png).slice(0, 40)}`);
    // A mind map, drawn as a tree and saved the same way.
    await win.pushEvents([{type: 'studyMadeOpen', made: {id: 'M001', kind: 'mindMap', title: 'Search', made: 1, by: 'Claude Code', task: 'J0001'},
      body: {shape: 'map', nodes: [{id: '1', parent: null, label: 'Search', cites: []}, {id: '2', parent: '1', label: 'BFS', cites: []}, {id: '3', parent: '1', label: 'UCS', cites: []}]}, citations: []}]); await settle();
    const map = win.commands.length;
    ws().querySelector('[data-study-picture="M001"]').click();
    await new Promise((r) => setTimeout(r, 600)); await settle();
    assert(sent(map, 'studySaveImage')[0]?.png.startsWith('data:image/png;base64,'), 'the map was not drawn');
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
