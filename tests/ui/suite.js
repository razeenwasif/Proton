import {orderingChecks} from './ordering.js';
const frame = document.querySelector('iframe');
// Keep the test repeatable without changing the local browser preview's layout.
const layoutKeys = [...['chat', 'codex', 'design', 'modeling', 'game', 'video', 'data', 'research'].map((mode) => `proton.panes.${mode}`),
  // Which artifact each conversation had open.
  'proton.chat.panes'];
const savedLayouts = layoutKeys.map((key) => [key, localStorage.getItem(key)]);
layoutKeys.forEach((key) => localStorage.removeItem(key));
const restoreLayouts = () => savedLayouts.forEach(([key, value]) => {
  if (value === null) localStorage.removeItem(key);
  else localStorage.setItem(key, value);
});
window.addEventListener('pagehide', restoreLayouts, {once: true});
const html = await fetch('/index.html').then((r) => r.text());
frame.srcdoc = html.replace('<head>', '<head><base href="/"><script src="tests/ui/transport.js"></script>');
await new Promise((done) => frame.addEventListener('load', done, {once: true}));
const win = frame.contentWindow;
const doc = win.document;
const $ = (selector) => doc.querySelector(selector);
const settle = () => new Promise((done) => setTimeout(done, 100));
const key = (value, options = {}) => doc.activeElement.dispatchEvent(new win.KeyboardEvent('keydown', {
  key: value, bubbles: true, cancelable: true, ...options,
}));
const assert = (condition, message) => { if (!condition) throw new Error(message); };
let passed = 0, failed = 0;
async function test(name, run) {
  const row = document.createElement('li');
  let timeout;
  try { await Promise.race([Promise.resolve().then(run), new Promise((_, reject) => {
    timeout = setTimeout(() => reject(new Error('timed out')), 5000);
  })]); passed++; row.textContent = `PASS — ${name}`; }
  catch (err) { failed++; row.className = 'fail'; row.textContent = `FAIL — ${name}: ${err.message}`; }
  finally { clearTimeout(timeout); }
  document.querySelector('#results').append(row);
}
await settle();
await test('Native boot preserves the buffer instead of opening demo source', () => {
  assert(!win.commands.some((c) => c.type === 'openBuffer'), 'boot opened a fixture');
  assert(!win.commands.some((c) => c.type === 'setSetting'), 'appearance cache overwrote project settings on boot');
});
await test('Native boot does not load browser demo fixtures', () => {
  assert(!win.performance.getEntriesByType('resource').some(entry => entry.name.includes('/preview-bridge.js')),
    'native startup loaded the browser-only fixture module');
});
await test('Status and task counts reflect native events', () => {
  assert($('#sl-project').textContent === 'Fixture', `wrong workspace; commands: ${JSON.stringify(win.commands)}; errors: ${win.testErrors.join('; ')}`);
  assert($('#sl-git').textContent === 'test-branch', 'wrong branch');
  assert($('#sl-filetype').textContent === 'c', 'wrong file type');
  assert($('[data-task-badge]').hidden, 'empty queue has a badge');
});
await test('Clicking search opens the core picker and focuses its input', async () => {
  $('.rail [data-open="telescope"]').click(); await settle();
  assert(win.commands.some((c) => c.type === 'openPicker'), 'no core command');
  assert(doc.activeElement === $('[data-picker-query]'), 'query does not have focus');
  assert($('.app').inert, 'background remains interactive');
});
await test('Typing and pasting update the query without editor commands', async () => {
  const before = win.commands.length;
  const input = $('[data-picker-query]'); input.value = 'PROGRESS pasted query';
  input.dispatchEvent(new win.Event('input', {bubbles: true})); await settle();
  const sent = win.commands.slice(before);
  assert(sent.some((c) => c.type === 'pickerQuery' && c.text === input.value), 'query was not sent');
  assert(!sent.some((c) => c.type === 'key'), 'query leaked into editor');
  assert(!$('#telescope .preview').textContent.includes('dashboard.tsx'), 'demo preview remains');
});
await test('Scrim dismissal cancels the core picker and releases the background', async () => {
  $('#scrim').click(); await settle();
  assert(!$('.app').inert && !$('#telescope').classList.contains('on'), 'picker still owns view');
  assert(win.commands.some((c) => c.type === 'key' && c.key.key === 'Escape'), 'core not cancelled');
});
await test('Command-K follows the same search path and Escape closes it', async () => {
  key('k', {metaKey: true}); await settle();
  assert($('#telescope').classList.contains('on'), 'search not open');
  key('Escape'); await settle();
  assert(!$('#telescope').classList.contains('on'), 'search did not close');
});
await test('A file list cut short says so, and says why', async () => {
  key('k', {metaKey: true}); await settle();
  const count = $('[data-picker-count]');
  assert(!/cut short/.test(count.textContent) && !count.title, `a whole list was called cut short: ${count.textContent}`);
  // Over a home folder the core lists only so many files, and a file it never
  // reached would otherwise look like one that is not there.
  await win.pushEvents([{type: 'picker', kind: 'files', title: 'find_files', query: 'zz', items: [],
    selected: 0, total: 50000, elapsedMs: 2, scanning: false, cutShort: true}]); await settle();
  assert(/cut short/.test(count.textContent), `the footer does not say the list was cut short: ${count.textContent}`);
  assert(/50000/.test(count.title), `the footer does not say why: ${count.title}`);
  const empty = $('[data-picker-results]').textContent;
  assert(/files listed/.test(empty), `an empty list reads as no matches anywhere: ${empty}`);
  key('Escape'); await settle();
});
await test('Settings contains focus and never forwards typing to Vim', async () => {
  $('.rail [data-open="settings"]').click(); await settle();
  const before = win.commands.length;
  assert(doc.activeElement.closest('#settings'), 'focus outside Settings');
  key('Tab'); assert(doc.activeElement.matches('[data-st-nav]'), 'Tab missed navigation');
  key('d'); await settle();
  assert(!win.commands.slice(before).some((c) => c.type === 'key'), 'Settings forwarded a key');
  key('Escape'); await settle();
  assert(!$('.app').inert, 'Settings left background inert');
});
await test('Opening Settings from a picker cancels core state first', async () => {
  key('k', {metaKey: true}); await settle(); key(',', {metaKey: true}); await settle();
  assert($('#settings').classList.contains('on'), 'Settings did not open');
  key('Escape'); await settle();
});
await test('Hidden modes never report a five-line editor viewport', async () => {
  const before = win.commands.length;
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  assert(!win.commands.slice(before).some((c) => c.type === 'viewport'), 'hidden editor measured');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  const reports = win.commands.slice(before).filter((c) => c.type === 'viewport');
  assert(reports.length && reports.every((c) => c.lines > 5 && c.view === 0), 'invalid viewport after reveal');
});
await test('Resizing the frame remeasures the visible editor', async () => {
  const before = win.commands.length; frame.style.height = '800px'; await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'viewport' && c.lines > 5), 'no resize report');
});
await test('Native window chrome retains a 13px rounded edge', () => {
  assert(win.getComputedStyle($('.app')).borderTopLeftRadius === '13px', 'radius lost under backdrop');
});
await test('Sidebar collapse gives the editor more room and restores across modes', async () => {
  const section = $('.workspace.on');
  const toggle = $('[data-panel-toggle="left"]');
  const before = section.querySelector('main').clientWidth;
  toggle.click(); await settle();
  assert(section.querySelector('aside').hidden, 'sidebar still visible');
  assert(section.querySelector('main').clientWidth > before, 'editor did not gain space');
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  assert(section.querySelector('aside').hidden, 'mode change lost collapsed state');
  toggle.click(); await settle();
  assert(!section.querySelector('aside').hidden, 'sidebar could not be restored');
});
await test('Narrow windows show the requested panel without squeezing out the editor', async () => {
  // Narrow enough that the two side panels cannot both sit beside a usable
  // stage. Codex has no rails, so that is narrower than it is in other modes.
  frame.style.width = '760px'; await settle();
  const section = $('.workspace.on');
  $('[data-panel-toggle="right"]').click(); await settle();
  assert(!$('#codex-dock').hidden, 'right panel did not open');
  assert(section.querySelector('aside').hidden, 'both side panels still visible');
  assert(section.querySelector('main').clientWidth >= 320, 'editor became too narrow');
  frame.style.width = '1400px'; await settle();
  assert(!section.querySelector('aside').hidden, 'wide layout did not restore sidebar');
});
await test('Each mode keeps a usable stage, and every one has a right panel to toggle', async () => {
  for (const mode of ['chat', 'design', 'modeling', 'game', 'video', 'data', 'research', 'codex']) {
    await win.dispatch({type: 'setMode', mode}); await settle();
    const section = $('.workspace.on'), stage = section.querySelector('main');
    assert(stage.clientWidth >= 320, `${mode} lost its stage`);
    assert(stage.getBoundingClientRect().right <= section.getBoundingClientRect().right + 1, `${mode} overflows`);
    // Chat's is its artifacts pane.
    const toggle = [...doc.querySelectorAll('[data-panel-toggle="right"]')].find((b) => b.getClientRects().length);
    assert(toggle && !toggle.disabled, `${mode} has the wrong panel controls`);
  }
});
const shown = (el) => Boolean(el) && win.getComputedStyle(el).display !== 'none' && el.getClientRects().length > 0;
await test('Chat wears Codex\'s chrome: a title bar for the rails, no statusline, no terminal toggle', async () => {
  await win.dispatch({type: 'setMode', mode: 'chat'}); await settle();
  assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.rail')) && !shown($('.statusline')), 'Chat kept the old chrome');
  assert(!shown($('.titlebar [data-term-toggle]')), 'Chat offers a terminal it cannot show');
  const ground = win.getComputedStyle($('.workspace[data-ws="chat"] .cc-main')).backgroundColor;
  const bg = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim();
  const probe = doc.createElement('i'); probe.style.color = bg; doc.body.append(probe);
  const expected = win.getComputedStyle(probe).color; probe.remove();
  assert(ground === expected, `Chat is not on the theme's ground: ${ground} vs ${expected}`);
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('Codex trades the rails and statusline for a title bar, Data does too, and Design does not', async () => {
  assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Codex kept the old chrome');
  const bar = $('.titlebar').getBoundingClientRect(), app = $('.app').getBoundingClientRect();
  assert(Math.abs(bar.right - app.right) < 2 && bar.height > app.height - 4 && bar.width < 60,
    'title bar is not a column down the right edge');
  assert($('.workspace.on').getBoundingClientRect().top - app.top < 2, 'the code lost the top of the window');
  const tab = [...doc.querySelectorAll('.titlebar [role="tab"]')].find((t) => t.textContent === 'Data');
  assert(tab && tab.getAttribute('aria-selected') === 'false', 'title bar has no Data mode');
  const before = win.commands.length;
  tab.click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'setMode' && c.mode === 'data'), 'title bar did not switch modes');
  assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.statusline')), 'Data kept the old chrome');
  const data = $('.workspace[data-ws="data"]');
  assert(win.getComputedStyle(data).fontFamily.includes('JetBrains'), 'Data is not set in Codex\'s type');
  assert(!shown(data.querySelector('aside.side > header')) && !shown(data.querySelector('aside.side > .find')), 'Data\'s project panel kept its header');
  await win.dispatch({type: 'setMode', mode: 'design'}); await settle();
  assert(!shown($('.titlebar')) && shown($('.chrome-rail')) && shown($('.statusline')), 'Design lost its chrome');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  assert(shown($('.titlebar')), 'title bar did not come back');
});
await test('Codex can switch and open projects without the rails', async () => {
  const tiles = doc.querySelectorAll('.titlebar [data-workspace]');
  assert(tiles.length === 1 && shown(tiles[0]) && tiles[0].getAttribute('aria-selected') === 'true', 'no project tiles in the title bar');
  let before = win.commands.length;
  $('.titlebar [data-open-workspace]').click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'openWorkspacePicker'), 'the + did not ask for a folder');
  $('.workspace.on [data-editor]').focus();
  before = win.commands.length;
  key('o', {metaKey: true}); await settle();
  const sent = win.commands.slice(before);
  assert(sent.some((c) => c.type === 'openWorkspacePicker'), '⌘O did not ask for a folder');
  assert(!sent.some((c) => c.type === 'key'), '⌘O reached Vim');
});
await test('which-key lists what is bound under the pending prefix', async () => {
  await win.pushEvents([{type: 'leader', prefix: 'f'}]);
  assert($('#whichkey').classList.contains('on'), 'which-key did not open');
  assert($('[data-wk-grid]').textContent.includes('projects') && $('[data-wk-head]').textContent.includes('find'), 'the f family is not listed');
  await win.pushEvents([{type: 'leader', prefix: ''}]);
  assert($('[data-wk-grid]').textContent.includes('+project'), 'the top level lost the project family');
  await win.pushEvents([{type: 'leader', prefix: null}]);
  assert(!$('#whichkey').classList.contains('on'), 'which-key stayed open');
});
await test('Codex says in passing what a statusline would have said', async () => {
  await win.pushEvents([{type: 'notice', level: 'info', text: '/fixture'}]);
  const line = $('.msgline');
  assert(shown(line) && line.textContent === '/fixture', 'the message did not appear');
  assert(line.closest('.stage-code'), 'the message is not at the foot of the code');
});
await test('The terminal sits beside the code and toggles without editor commands', async () => {
  const pane = $('[data-term-pane]'), toggle = $('.titlebar [data-term-toggle]');
  const code = $('.workspace.on .stage-code');
  const wasOpen = !pane.hidden;
  if (!wasOpen) { toggle.click(); await settle(); }
  const narrow = code.clientWidth, before = win.commands.length;
  assert(!pane.hidden && pane.closest('main'), 'terminal is not in the stage');
  toggle.click(); await settle();
  assert(pane.hidden && code.clientWidth > narrow, 'closing the terminal gave the code no room');
  toggle.click(); await settle();
  assert(!pane.hidden && toggle.getAttribute('aria-pressed') === 'true', 'terminal did not reopen');
  const sent = win.commands.slice(before);
  assert(sent.some((c) => c.type === 'openTerminal'), 'opening the pane did not ask for a shell');
  assert(!sent.some((c) => c.type === 'key'), 'toggling reached Vim');
  if (!wasOpen) { toggle.click(); await settle(); }
});
const shellScreen = (text, running = true) => ({type: 'terminal', rows: text ? [[{text}]] : [], cursor: [0, 0],
  cursorVisible: running, running, scrollback: 0});
const fixtureProjects = {type: 'workspaces', items: [{name: 'Fixture', path: '/fixture', modified: false}], active: 0};
await test('Each project shows its own shell, and one without asks for one', async () => {
  const pane = $('[data-term-pane]'), screen = $('[data-term-screen]'), toggle = $('.titlebar [data-term-toggle]');
  const wasOpen = !pane.hidden;
  if (!wasOpen) { toggle.click(); await settle(); }
  await win.pushEvents([shellScreen('alpha %')]);
  assert(screen.textContent.includes('alpha %'), 'the shell was not drawn');
  const before = win.commands.length;
  await win.pushEvents([fixtureProjects, shellScreen('', false)]); await settle();
  assert(!screen.textContent.includes('alpha %'), "the last project's shell stayed on screen");
  const asked = win.commands.slice(before).filter((c) => c.type === 'openTerminal').length;
  assert(asked === 1, `asked for ${asked} shells, not one`);
  if (!wasOpen) { toggle.click(); await settle(); }
});
await test('An idle terminal is told its size once, not once a frame', async () => {
  const pane = $('[data-term-pane]'), toggle = $('.titlebar [data-term-toggle]');
  const wasOpen = !pane.hidden;
  if (!wasOpen) { toggle.click(); await settle(); }
  win.echoTerminalSize = true;
  try {
    const before = win.commands.length;
    await win.pushEvents([shellScreen('idle %')]);
    for (let i = 0; i < 5; i++) await settle();
    const told = win.commands.slice(before).filter((c) => c.type === 'terminalSize').length;
    assert(told === 1, `told its size ${told} times in half a second`);
  } finally { win.echoTerminalSize = false; }
  if (!wasOpen) { toggle.click(); await settle(); }
});
await test('Tab in the code goes to the editor and does not walk off to the next control', async () => {
  const editor = $('.workspace.on .split.on [data-editor]');
  editor.focus();
  const before = win.commands.length;
  const tab = new win.KeyboardEvent('keydown', {key: 'Tab', bubbles: true, cancelable: true});
  editor.dispatchEvent(tab);
  await settle();
  assert(tab.defaultPrevented, 'the browser was left to move the focus');
  assert(win.commands.slice(before).some((c) => c.type === 'key' && c.key.key === 'Tab'), 'Tab never reached the editor');
  assert(doc.activeElement === editor, 'the focus left the editor');
});
await test('A paste while the shell has the keyboard goes to the shell, as a paste', async () => {
  const pane = $('[data-term-pane]'), toggle = $('.titlebar [data-term-toggle]');
  const wasOpen = !pane.hidden;
  if (!wasOpen) { toggle.click(); await settle(); }
  win.getSelection()?.removeAllRanges();
  $('[data-term-screen]').click(); await settle();
  const field = $('[data-term-input]');
  assert(doc.activeElement === field, 'the terminal did not take the keyboard through its input');
  // ⌘V with the screen focused hands the focus to the field first.
  $('[data-term-screen]').focus();
  const mac = /Mac/.test(win.navigator.platform);
  key('v', mac ? {metaKey: true} : {ctrlKey: true, shiftKey: true});
  assert(doc.activeElement === field, '⌘V did not move the focus to where a paste can land');
  const before = win.commands.length;
  const data = new win.DataTransfer();
  data.setData('text/plain', 'echo hi\n');
  const paste = new win.ClipboardEvent('paste', {clipboardData: data, bubbles: true, cancelable: true});
  field.dispatchEvent(paste);
  await settle();
  const sent = win.commands.slice(before);
  assert(sent.some((c) => c.type === 'terminalPaste' && c.text === 'echo hi\n'), `no paste reached the shell: ${JSON.stringify(sent)}`);
  assert(paste.defaultPrevented && field.value === '', 'the paste also landed in the hidden field');
  // An input method's text goes to the shell too, and leaves nothing behind.
  field.value = 'é';
  field.dispatchEvent(new win.InputEvent('input', {bubbles: true, data: 'é', inputType: 'insertText'}));
  await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'terminalInput' && c.text === 'é'), 'composed text was lost');
  assert(field.value === '', 'the field kept what was typed into it');
  $('.workspace.on [data-editor]').click();
  if (!wasOpen) { toggle.click(); await settle(); }
  $('.workspace.on [data-editor]').focus();
});
/** One line of coloured text per row: a red number, then a word on blue. */
const colourScreen = (first, rows = 1) => ({type: 'terminal', running: true, scrollback: 0, cursor: [0, 0], cursorVisible: true,
  rows: Array.from({length: rows}, (_, i) => [{text: `${first + i} `, fg: 1}, {text: 'drawn', bg: 4}])});
/** The canvas's pixels, top-down — read before the frame is shown, which is
 *  the only time a canvas that does not keep its picture still has it. */
const canvasPixels = (canvas, x, y, w, h) => {
  const gl = canvas.getContext('webgl2'), out = new Uint8Array(w * h * 4);
  gl.readPixels(x, canvas.height - y - h, w, h, gl.RGBA, gl.UNSIGNED_BYTE, out);
  return out;
};
/** A theme token as the page paints it, `[r, g, b]`. */
const tokenRgb = (name) => {
  const probe = doc.createElement('i'); probe.style.color = `var(${name})`; doc.body.append(probe);
  const rgb = win.getComputedStyle(probe).color.match(/\d+/g).slice(0, 3).map(Number); probe.remove();
  return rgb;
};
const near = (pixels, offset, rgb, slack) => rgb.every((v, i) => Math.abs(pixels[offset + i] - v) <= slack);
await test('The GPU draws the terminal, and the rows under it catch up once it is still', async () => {
  const pane = $('[data-term-pane]'), screen = $('[data-term-screen]'), toggle = $('.titlebar [data-term-toggle]');
  const wasOpen = !pane.hidden;
  if (!wasOpen) { toggle.click(); await settle(); }
  await win.pushEvents([colourScreen(1)]); await settle();
  await win.dispatch({type: 'setSetting', key: 'terminal.renderer', value: 'gpu'}); await settle();
  try {
    const canvas = pane.querySelector('canvas.term-gl');
    assert(canvas && screen.classList.contains('gpu'), 'Settings asked for the GPU and the text renderer stayed');
    // The cell, as the panel measures it.
    const probe = doc.createElement('span');
    probe.style.cssText = 'position:absolute;visibility:hidden;white-space:pre;padding:0';
    probe.textContent = '0'.repeat(100); screen.append(probe);
    const style = win.getComputedStyle(screen), dpr = canvas.width / parseFloat(canvas.style.width);
    const cell = {w: probe.getBoundingClientRect().width / 100 * dpr, h: parseFloat(style.lineHeight) * dpr};
    probe.remove();
    const left = parseFloat(style.paddingLeft) * dpr, top = parseFloat(style.paddingTop) * dpr;

    await win.pushEvents([colourScreen(2)]);
    // The GPU draws at the next frame. Read it in that frame, after the draw
    // and before it is shown.
    await new Promise((done) => win.requestAnimationFrame(done));
    // The word's background, along the top of its fourth cell where no letter reaches.
    const ground = canvasPixels(canvas, Math.round(left + 4.5 * cell.w), Math.round(top + 2), 1, 1);
    assert(near(ground, 0, tokenRgb('--ansi-4'), 3), `the background was not drawn: ${[...ground]}`);
    // The digit, somewhere in the first cell, in the theme's red.
    const digit = canvasPixels(canvas, Math.round(left), Math.round(top), Math.floor(cell.w), Math.floor(cell.h));
    const red = tokenRgb('--ansi-1');
    let inked = false;
    for (let i = 0; i < digit.length && !inked; i += 4) inked = near(digit, i, red, 10);
    assert(inked, 'the digit was not drawn in its colour');
    // The rows underneath have not been touched for that screen yet...
    assert(screen.textContent.includes('1 drawn') && !screen.textContent.includes('2 drawn'), 'the rows were rewritten for a moving screen');
    // ...and catch up once it has been still for a moment.
    await settle(); await settle();
    assert(screen.textContent.includes('2 drawn'), 'the rows never caught up with a still screen');
    // A press brings them up to date at once, so a selection starts on what is shown.
    await win.pushEvents([colourScreen(3)]);
    screen.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, cancelable: true}));
    assert(screen.textContent.includes('3 drawn'), 'a press started a selection on rows the screen no longer shows');
    // The rows are there for the mouse and the clipboard, and must not show.
    const row = screen.querySelector('.trow span');
    assert(win.getComputedStyle(row).color === 'rgba(0, 0, 0, 0)', 'the rows under the picture were visible');
    // Back to text: the canvas goes, and the rows are the screen again.
    await win.pushEvents([colourScreen(4)]);
    await win.dispatch({type: 'setSetting', key: 'terminal.renderer', value: 'text'}); await settle();
    assert(!pane.querySelector('canvas.term-gl') && !screen.classList.contains('gpu'), 'the GPU renderer outlived its setting');
    assert(screen.textContent.includes('4 drawn'), 'the text renderer came back to a stale screen');
  } finally {
    await win.dispatch({type: 'setSetting', key: 'terminal.renderer', value: 'text'}); await settle();
    if (!wasOpen) { toggle.click(); await settle(); }
  }
});
await test('The text terminal colours all 256 of xterm\'s colours, not only the theme\'s sixteen', async () => {
  const screen = $('[data-term-screen]');
  await win.pushEvents([{type: 'terminal', running: true, scrollback: 0, cursor: [0, 0], cursorVisible: false,
    rows: [[{text: 'cube', fg: 196, bg: 21}, {text: 'grey', fg: 244}]]}]);
  const [cube, grey] = screen.querySelectorAll('.trow span');
  assert(cube && win.getComputedStyle(cube).color === 'rgb(255, 0, 0)', `colour 196 was ${cube && win.getComputedStyle(cube).color}`);
  assert(win.getComputedStyle(cube).backgroundColor === 'rgb(0, 0, 255)', 'background 21 was not drawn');
  assert(grey && win.getComputedStyle(grey).color === 'rgb(128, 128, 128)', `grey 244 was ${grey && win.getComputedStyle(grey).color}`);
});
await test('Visual-line mode lights the whole line before the cursor moves', async () => {
  await win.pushEvents([
    {type: 'vimMode', mode: 'VisualLine', label: 'V-LINE'},
    {type: 'viewport', view: 0, firstLine: 0, lines: ['alpha beta', 'gamma'], totalLines: 2, version: 0,
      highlights: [], matches: [], git: [], stops: []},
    {type: 'cursor', view: 0, line: 0, column: 3, offset: 3, selection: {line: 0, column: 3, offset: 3}},
  ]);
  const editor = $('.workspace.on [data-editor]');
  const selected = [...editor.querySelectorAll('.sel')].map((n) => n.textContent).join('');
  assert(selected.replace(/\s+$/, '') === 'alpha beta'.replace('a', '') || selected.includes('beta'), `line not selected: ${JSON.stringify(selected)}`);
  await win.pushEvents([{type: 'vimMode', mode: 'Normal', label: 'NORMAL'},
    {type: 'cursor', view: 0, line: 0, column: 3, offset: 3, selection: null}]);
  assert(!editor.querySelector('.sel'), 'the selection stayed after leaving visual mode');
});
await test('Moving to a project does not open a terminal that was shut, and ␣tt still does', async () => {
  const pane = $('[data-term-pane]'), toggle = $('.titlebar [data-term-toggle]');
  const wasOpen = !pane.hidden;
  if (wasOpen) { toggle.click(); await settle(); }
  await win.pushEvents([{type: 'leader', prefix: null}, fixtureProjects, shellScreen('beta %')]); await settle();
  assert(pane.hidden, 'switching projects opened the terminal');
  await win.pushEvents([{type: 'leader', prefix: null}, shellScreen('beta %')]); await settle();
  assert(!pane.hidden, '␣tt no longer opens the terminal');
  // Back to the code, and to the pane as it was.
  $('.workspace.on [data-editor]').click();
  if (!wasOpen) { toggle.click(); await settle(); }
  $('.workspace.on [data-editor]').focus();
  assert(doc.documentElement.dataset.keys === 'code', 'the keyboard stayed with the shell');
});
await test('A dock shortcut reopens the hidden right panel', async () => {
  const toggle = $('[data-panel-toggle="right"]');
  if (toggle.getAttribute('aria-expanded') === 'true') toggle.click();
  await settle();
  $('.rail [data-dock="git"]').click(); await settle();
  assert(!$('#codex-dock').hidden, 'Git stayed hidden');
  assert($('#codex-dock [data-panel="git"]').classList.contains('on'), 'wrong dock panel');
});
await test('Keyboard resizing changes pane width without editor commands', async () => {
  const gutter = $('.workspace.on [data-resize="--col-l"]');
  const pane = $('.workspace.on aside');
  gutter.focus(); const before = pane.clientWidth, sent = win.commands.length;
  key('ArrowRight'); await settle();
  assert(pane.clientWidth === before + 20, 'keyboard resize had no effect');
  assert(!win.commands.slice(sent).some((c) => c.type === 'key'), 'resize reached Vim');
  key('Home'); await settle();
});
const choose = async (name, value) => {
  const input = $(`[data-display-setting="${name}"]`);
  input.value = String(value); input.dispatchEvent(new win.Event('change', {bubbles: true})); await settle();
};
await test('Display settings persist typed values and change the rendered UI', async () => {
  $('.rail [data-open="settings"]').click(); await settle();
  const treeSize = parseFloat(win.getComputedStyle($('.tree')).fontSize);
  await choose('ui.textScale', 1.25);
  assert(parseFloat(win.getComputedStyle($('.tree')).fontSize) > treeSize, 'labels did not grow');
  await choose('ui.density', 'compact');
  assert(doc.documentElement.dataset.density === 'compact', 'density did not change');
  await choose('ui.blur', 0);
  assert(win.getComputedStyle($('#settings')).backdropFilter.includes('0px'), 'blur stayed on');
  await choose('editor.fontSize', 18);
  assert(parseFloat(win.getComputedStyle($('.workspace.on [data-editor]')).fontSize) === 18, 'editor text did not grow');
  assert(win.commands.some((c) => c.type === 'setSetting' && c.key === 'editor.fontSize' && c.value === 18), 'font size not persisted as a number');
  key('Escape'); await settle();
  const report = win.commands.filter((c) => c.type === 'viewport').at(-1);
  assert(report.lines > 5 && report.lines < 40, 'font resize did not remeasure viewport');
});
await test('Settings sync is read-only and workspace defaults reset display preferences', async () => {
  const before = win.commands.length;
  win.pushEvents([{type: 'settings', values: {}, path: '/other/.proton/settings.json'}]); await settle();
  assert(parseFloat(win.getComputedStyle($('.workspace.on [data-editor]')).fontSize) === 16, 'old workspace font leaked');
  assert(doc.documentElement.dataset.density === 'comfortable', 'old density leaked');
  assert(!win.commands.slice(before).some((c) => c.type === 'setSetting'), 'rendering settings wrote them back');
});
await test('The file tree exposes hierarchy and a single Tab stop', () => {
  const tree = $('.workspace.on [data-tree]');
  assert(tree.getAttribute('role') === 'tree', 'no tree role');
  assert(tree.querySelectorAll('[role="treeitem"][tabindex="0"]').length === 1, 'invalid roving Tab stop');
  const folder = tree.querySelector('[data-tree-path="src"]');
  assert(folder.getAttribute('aria-expanded') === 'false' && folder.getAttribute('aria-level') === '1', 'folder semantics missing');
});
await test('Tree arrows expand folders and navigate without editing the buffer', async () => {
  const tree = $('.workspace.on [data-tree]');
  tree.querySelector('[data-tree-path="src"]').focus();
  const before = win.commands.length;
  key('ArrowRight'); await settle();
  assert(doc.activeElement.dataset.treePath === 'src', 'expanding lost focus');
  assert(doc.activeElement.getAttribute('aria-expanded') === 'true', 'folder did not expand');
  key('ArrowRight'); await settle();
  assert(doc.activeElement.dataset.treePath === 'src/fixture.c', 'right arrow missed first child');
  assert(doc.activeElement.getAttribute('aria-level') === '2', 'child hierarchy missing');
  key('ArrowDown'); await settle();
  assert(doc.activeElement.dataset.treePath === 'src/helpers.c', 'down arrow missed sibling');
  assert(!win.commands.slice(before).some((c) => c.type === 'key' || c.type === 'openBuffer'), 'navigation edited or opened a file');
});
await test('Tree refresh restores focus to the surviving parent', async () => {
  await win.dispatch({type: 'toggleTreeNode', path: 'src'}); await settle();
  assert(doc.activeElement.dataset.treePath === 'src', 'removed child lost focus instead of finding parent');
  assert(doc.activeElement.getAttribute('aria-expanded') === 'false', 'collapse state stale');
});
await test('Type-to-find moves focus and Enter opens exactly the chosen file', async () => {
  const before = win.commands.length;
  key('r'); await settle();
  assert(doc.activeElement.dataset.treePath === 'README.md', 'type-to-find missed README');
  key('Enter'); await settle();
  const commands = win.commands.slice(before);
  assert(commands.filter((c) => c.type === 'openBuffer' && c.path === 'README.md').length === 1, 'file was not opened exactly once');
  assert(!commands.some((c) => c.type === 'key'), 'tree key leaked into Vim');
  assert(doc.activeElement.dataset.treePath === 'README.md', 'opening lost keyboard focus');
  key('Escape'); await settle();
  assert(doc.activeElement.matches('[data-editor]'), 'Escape did not return to the editor');
});
await test('A click in the code takes the keyboard back from the project panel', async () => {
  $('.workspace.on [data-tree] [data-tree-path="README.md"]').focus();
  assert(doc.documentElement.dataset.keys === 'elsewhere', 'the caret does not say the keys are elsewhere');
  const caret = $('.workspace.on .split.on [data-editor] .caret');
  assert(!caret || win.getComputedStyle(caret).animationName === 'none', 'the caret blinks for keys that are not coming');
  const number = $('.workspace.on .split.on [data-editor] .ln .n');
  const box = number.getBoundingClientRect();
  const at = {clientX: box.left + box.width / 2, clientY: box.top + box.height / 2};
  assert(doc.elementFromPoint(at.clientX, at.clientY)?.closest('.ln'), 'the line is not on screen to click');
  number.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, cancelable: true, button: 0, ...at}));
  number.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true, ...at}));
  await settle();
  assert(doc.activeElement.matches('[data-editor]'), 'the keyboard stayed in the project panel');
  assert(doc.documentElement.dataset.keys === 'code', 'the caret still says the keys are elsewhere');
  const before = win.commands.length;
  key('i'); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'key' && c.key.key === 'i'), 'i did not reach Vim');
  key('Escape'); await settle();
});
await test('A press in the code takes the keyboard back from the shell, click or no click', async () => {
  const pane = $('[data-term-pane]'), toggle = $('.titlebar [data-term-toggle]');
  const wasOpen = !pane.hidden;
  if (!wasOpen) { toggle.click(); await settle(); }
  $('[data-term-screen]').click(); await settle();
  assert(doc.documentElement.dataset.keys === 'elsewhere', 'the shell does not have the keyboard');
  // A press and a release, and no click: what a cursor move that redraws the
  // line under the pointer leaves behind.
  const number = $('.workspace.on .split.on [data-editor] .ln .n');
  const box = number.getBoundingClientRect();
  const at = {clientX: box.left + box.width / 2, clientY: box.top + box.height / 2};
  number.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, cancelable: true, button: 0, ...at}));
  number.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true, ...at}));
  await settle();
  assert(doc.documentElement.dataset.keys === 'code', 'the shell kept the keyboard');
  const before = win.commands.length;
  key('i'); await settle();
  const sent = win.commands.slice(before);
  assert(sent.some((c) => c.type === 'key' && c.key.key === 'i'), 'i did not reach Vim');
  assert(!sent.some((c) => c.type === 'terminalInput'), 'i was typed into the shell');
  key('Escape'); await settle();
  if (!wasOpen) { toggle.click(); await settle(); }
});
await test('The caret blinks on a timer, not an animation restyled every frame', async () => {
  const caret = $('.workspace.on .split.on [data-editor] .caret');
  assert(caret, 'no caret to look at');
  const running = caret.getAnimations().length + [...caret.querySelectorAll('*')].reduce((n, el) => n + el.getAnimations().length, 0);
  assert(running === 0, `the caret has ${running} running animation(s)`);
  assert(win.getComputedStyle(caret).animationName === 'none', 'a CSS animation is attached to the caret');
});
await test('A file clicked in the project panel takes the keyboard with it; a folder keeps it', async () => {
  const tree = $('.workspace.on [data-tree]');
  tree.querySelector('[data-tree-path="README.md"]').click(); await settle();
  assert(doc.activeElement.matches('[data-editor]'), 'the keyboard stayed in the project panel');
  const before = win.commands.length;
  key('i'); await settle();
  const sent = win.commands.slice(before);
  assert(sent.some((c) => c.type === 'key' && c.key.key === 'i'), 'i did not reach Vim');
  assert(!sent.some((c) => c.type === 'openBuffer'), 'i found a file instead');
  key('Escape'); await settle();
  tree.querySelector('[data-tree-path="src"]').click(); await settle();
  assert(doc.activeElement.dataset.treePath === 'src', 'a folder took the keyboard out of the panel');
  tree.querySelector('[data-tree-path="src"]').click(); await settle();
  $('.workspace.on [data-editor]').focus();
});
const primaryModifier = /Mac/.test(win.navigator.platform) ? {metaKey: true} : {ctrlKey: true};
await test('Editor shortcuts dispatch one Undo or Redo without Vim keystrokes', async () => {
  $('.workspace.on [data-editor]').focus();
  const before = win.commands.length;
  key('z', primaryModifier); await settle();
  key('z', {...primaryModifier, shiftKey: true}); await settle();
  const commands = win.commands.slice(before);
  assert(commands.filter((c) => c.type === 'perform').map((c) => c.action.verb).join() === 'Undo,Redo', 'wrong history commands');
  assert(!commands.some((c) => c.type === 'key'), 'shortcut also reached Vim');
});
await test('History buttons target their pane before dispatching', async () => {
  const before = win.commands.length;
  $('.workspace.on [data-history="undo"]').click(); await settle();
  $('.workspace.on [data-history="redo"]').click(); await settle();
  const commands = win.commands.slice(before).filter((c) => ['focusSplit', 'perform'].includes(c.type));
  assert(commands.length === 4, 'unexpected history dispatch count');
  assert(commands[0].type === 'focusSplit' && commands[0].index === 0 && commands[1].action.verb === 'Undo', 'undo targeted the wrong pane');
  assert(commands[2].type === 'focusSplit' && commands[3].action.verb === 'Redo', 'redo did not focus first');
});
await test('Picker inputs retain native undo and never change editor history', async () => {
  $('.rail [data-open="telescope"]').click(); await settle();
  const before = win.commands.length;
  const allowed = key('z', primaryModifier); await settle();
  assert(allowed, 'native undo was prevented');
  assert(!win.commands.slice(before).some((c) => c.type === 'perform' || c.type === 'key'), 'picker undo edited the buffer');
  key('Escape'); await settle();
});
await test('Native editable fields retain their own history in Codex', async () => {
  const field = doc.createElement('textarea');
  $('.workspace.on').append(field); field.focus();
  try {
    const before = win.commands.length;
    assert(key('z', primaryModifier), 'native undo was prevented'); await settle();
    assert(!win.commands.slice(before).some((c) => c.type === 'perform' || c.type === 'key'), 'field undo edited the buffer');
  } finally { field.remove(); }
});
await test('History controls stay scoped to Codex', async () => {
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  assert(!$('.workspace.on [data-history]'), 'text history appeared in Data');
  const field = doc.createElement('textarea');
  $('.workspace.on').append(field); field.focus();
  const before = win.commands.length;
  key('z', primaryModifier); await settle(); field.remove();
  assert(!win.commands.slice(before).some((c) => c.type === 'perform'), 'Data invoked code undo');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('Windows/Linux shortcuts include Ctrl+Y and exclude Alt or Meta chords', async () => {
  const {historyShortcut} = await import('/history-controls.js');
  const commands = [], dispatch = async (command) => { commands.push(command); };
  const send = (key, modifiers) => historyShortcut(new KeyboardEvent('keydown', {key, cancelable: true, ...modifiers}), dispatch, false);
  assert(send('z', {ctrlKey: true}) && send('z', {ctrlKey: true, shiftKey: true}) && send('y', {ctrlKey: true}), 'Ctrl shortcut missing');
  assert(!send('z', {ctrlKey: true, altKey: true}) && !send('z', {metaKey: true}), 'unrelated modifier accepted');
  assert(commands.map((c) => c.action.verb).join() === 'Undo,Redo,Redo', 'wrong Ctrl history commands');
});
await test('Rapid file requests leave the last file and its metadata visible', async () => {
  win.replyDelay = 20;
  try {
    const requests = ['src/fixture.c', 'src/helpers.c', 'README.md'].map(path => win.dispatch({type: 'openBuffer', path}));
    await Promise.all(requests); await settle();
    assert($('#sl-path').textContent === 'README.md', 'metadata reverted to an earlier file');
    assert($('.workspace.on [data-editor] .tx').textContent.trim() === 'README.md', 'viewport and active file disagree');
  } finally { win.replyDelay = 0; }
});
await test('Reordered and duplicate background batches cannot repaint stale text', async () => {
  const viewport = text => ({type: 'viewport', view: 0, firstLine: 0, lines: [text], totalLines: 1,
    highlights: [], matches: [], git: [], stops: []});
  const old = win.makeBatch([viewport('older')]), newer = win.makeBatch([viewport('newer')]);
  const pushed = win.pushBatch(newer);
  assert(!$('.workspace.on [data-editor]').textContent.includes('newer'), 'newer event bypassed the ordering gap');
  await win.pushBatch(old); await pushed;
  assert($('.workspace.on [data-editor] .tx').textContent.trim() === 'newer', 'latest viewport was lost');
  await win.pushBatch(old);
  assert($('.workspace.on [data-editor] .tx').textContent.trim() === 'newer', 'duplicate batch reverted text');
});
await test('Viewport and cursor paint once per transaction and preserve unchanged rows', async () => {
  const viewport = {type: 'viewport', view: 0, firstLine: 0, lines: ['abc', 'unchanged'], totalLines: 2,
    highlights: [], matches: [], git: [], stops: []};
  const cursor = {type: 'cursor', view: 0, line: 0, column: 1, selection: null};
  await win.pushEvents([viewport, cursor]);
  const stable = $('.workspace.on [data-editor] .win').children[1];
  win.protonPerformance.start();
  await win.pushEvents([{...viewport, lines: ['axc', 'unchanged']}, {...cursor, column: 2}]);
  const result = win.protonPerformance.stop();
  assert(result.paints === 1, `painted ${result.paints} times for one edit`);
  assert($('.workspace.on [data-editor] .win').children[1] === stable, 'unchanged row was rebuilt');
  assert($('.workspace.on [data-editor] .caret').textContent === 'c', 'caret painted before final cursor state');
});
await test('File tabs give way to the path until Settings turns them on', async () => {
  const strip = $('.workspace.on [data-buffers]'), crumbs = $('.workspace.on [data-crumbs]');
  assert(!shown(strip) && shown(crumbs), 'tabs are showing by default');
  assert(crumbs.textContent === $('.workspace.on [data-buffers] .buf.on [role=tab]').title.split(' — ')[0], 'path is not the open file');
  await choose('editor.tabs', 'shown');
  assert(shown(strip) && !shown(crumbs), 'tabs did not come back');
  assert(win.commands.some((c) => c.type === 'setSetting' && c.key === 'editor.tabs' && c.value === 'shown'), 'choice was not persisted');
});
await test('Unchanged buffer metadata preserves the tab DOM and keyboard focus', async () => {
  const tab = $('.workspace.on [data-buffers] .buf.on [role=tab]');
  tab.tabIndex = 0; tab.focus();
  const data = [{name: 'fixture.c', path: 'src/fixture.c', modified: false},
    {name: 'README.md', path: 'README.md', modified: false},
    {name: 'helpers.c', path: 'src/helpers.c', modified: false}];
  await win.pushEvents([{type: 'buffers', names: data, active: 1}]);
  assert($('.workspace.on [data-buffers] .buf.on [role=tab]') === tab, 'unchanged tabs were rebuilt');
  assert(doc.activeElement === tab, 'buffer refresh dropped keyboard focus');
});
await test('Rapid pane clicks reserve focus and cursor placement before typing', async () => {
  await win.dispatch({type: 'split', vertical: true}); await settle();
  win.replyDelay = 15;
  const click = index => {
    const target = $(`.workspace.on [data-editor][data-view="${index}"] .tx`);
    const rect = target.getBoundingClientRect();
    target.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, button: 0,
      clientX: rect.left + 3, clientY: rect.top + rect.height / 2}));
    doc.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true}));
  };
  const before = win.commands.length;
  try {
    click(0); click(1);
    await win.dispatch({type: 'key', key: {key: 'i', ctrl: false, alt: false, shift: false, meta: false}});
    const commands = win.commands.slice(before).filter(command => ['focusSplit', 'placeCursor', 'key'].includes(command.type));
    assert(commands.map(command => command.type).join() === 'focusSplit,placeCursor,focusSplit,placeCursor,key', 'cursor gestures interleaved');
    assert(commands[0].index === 0 && commands[2].index === 1, 'rapid click trusted stale local focus');
    assert($('.workspace.on .split.on').dataset.view === '1', 'wrong focused pane');
  } finally { win.replyDelay = 0; }
});
await test('Rapid resizing reports both visible panes with useful heights', async () => {
  const before = win.commands.length;
  frame.style.height = '560px'; await settle();
  frame.style.height = '820px'; await settle();
  const reports = win.commands.slice(before).filter(command => command.type === 'viewport');
  for (const view of [0, 1]) {
    const heights = reports.filter(command => command.view === view).map(command => command.lines);
    assert(heights.length >= 2 && Math.max(...heights) > Math.min(...heights), `pane ${view} missed a resize`);
    assert(Math.min(...heights) > 5, `pane ${view} measured a hidden viewport`);
  }
  await win.dispatch({type: 'onlySplit'}); await settle();
});
await test('Performance sampling is opt-in, reports phases, and contains no typed text', async () => {
  win.protonPerformance.start();
  await win.dispatch({type: 'key', key: {key: 'private-text', ctrl: false, alt: false, shift: false, meta: false}});
  await settle();
  const result = win.protonPerformance.stop();
  const sample = result.samples.find(sample => sample.type === 'key');
  assert(sample && sample.nextFrameMs >= 0 && sample.replyMs >= 0 && sample.renderMs >= 0, 'timing phases missing');
  assert(!JSON.stringify(result).includes('private-text'), 'profiler retained input');
  await win.dispatch({type: 'key', key: {key: 'x', ctrl: false, alt: false, shift: false, meta: false}});
  assert(win.protonPerformance.snapshot().samples.length === result.samples.length, 'sampling continued after stop');
});
await test('Transaction rendering reduces work in a 60-line typing fixture', async () => {
  await win.dispatch({type: 'openBuffer', path: 'src/fixture.c'}); await settle();
  const lines = Array.from({length: 60}, (_, index) => `const row${index} = ${index};`);
  const events = index => [
    {type: 'viewport', view: 0, firstLine: 0, lines: [`const typed = ${index};`, ...lines.slice(1)], totalLines: 60,
      version: index, highlights: [], matches: [], git: [], stops: []},
    {type: 'cursor', view: 0, line: 0, column: index % 10, offset: index % 10, selection: null},
  ];
  await win.pushEvents(events(0));
  const measure = async batched => {
    win.protonPerformance.start();
    const times = [];
    for (let index = 1; index <= 40; index++) {
      const batch = events(index), start = performance.now();
      if (batched) await win.pushEvents(batch);
      else for (const event of batch) await win.pushEvents([event]);
      times.push(performance.now() - start);
    }
    times.sort((a, b) => a - b);
    return {...win.protonPerformance.stop(), median: times[20]};
  };
  const separate = await measure(false), batched = await measure(true);
  assert(separate.paints === 80 && batched.paints === 40, 'transactions did not halve pane renders');
  const report = document.createElement('p'); report.id = 'performance-result';
  report.textContent = `60-line fixture, 40 edits: per-event delivery ${separate.paints} paints / ${separate.rows} rows / ${separate.median.toFixed(2)} ms median render; batched delivery ${batched.paints} paints / ${batched.rows} rows / ${batched.median.toFixed(2)} ms median render. Browser fixture, not native typing latency.`;
  document.querySelector('#status').after(report);
});
await test('Buffer tabs expose selection and navigate without editing or opening', async () => {
  const strip = $('.workspace.on [data-buffers]');
  assert(strip.getAttribute('role') === 'tablist', 'missing tablist');
  assert(strip.querySelectorAll('[role=tab][tabindex="0"]').length === 1, 'more than one tab stop');
  const selected = strip.querySelector('[aria-selected=true]'); selected.focus();
  const before = win.commands.length;
  key('ArrowRight'); key('End'); key('Home'); key('x'); await settle();
  assert(doc.activeElement === strip.querySelector('[role=tab]'), 'Home missed the first tab');
  assert(!win.commands.slice(before).some(c => c.type === 'key' || c.type === 'focusBuffer'), 'tab navigation leaked into editing');
  key('End'); key('Enter'); await settle();
  assert(doc.activeElement.getAttribute('aria-selected') === 'true', 'Enter did not activate tab');
  assert(doc.getElementById(doc.activeElement.getAttribute('aria-controls'))?.getAttribute('role') === 'tabpanel', 'tab has no panel');
  key('Escape'); await settle();
  assert(doc.activeElement.matches('[data-editor]'), 'Escape did not focus editor');
});
await test('Deleting a tab closes once and restores focus to a surviving tab', async () => {
  await win.dispatch({type: 'openBuffer', path: 'temporary.txt'}); await settle();
  const tab = $('.workspace.on [data-buffers] [aria-selected=true]'); tab.focus();
  const before = win.commands.length;
  key('Delete'); await settle();
  assert(!$('.workspace.on [data-buffers]').textContent.includes('temporary.txt'), 'tab did not close');
  assert(doc.activeElement.matches('[role=tab][data-buffer]'), 'closing dropped focus');
  const sent = win.commands.slice(before);
  assert(sent.filter(c => c.type === 'closeBuffer').length === 1 && !sent.some(c => c.type === 'key'), 'Delete closed twice or edited');
  assert(sent.find(c => c.type === 'closeBuffer').force === false, 'close bypassed unsaved guard');
});
await test('Refused tab closes preserve the tab and expose a dismissible message', async () => {
  const tab = doc.activeElement;
  win.refuseClose = true;
  try { key('Delete'); await settle(); } finally { win.refuseClose = false; }
  assert(tab.isConnected && doc.activeElement === tab, 'refusal lost the tab or focus');
  const message = $('.editor-message');
  assert(!message.hidden && message.textContent.includes('Unsaved changes'), 'refusal is not visible');
  assert(message.querySelector('[role=status]'), 'refusal is not announced');
  const dismiss = message.querySelector('button'); dismiss.focus(); dismiss.click();
  assert(message.hidden && doc.activeElement.matches('[data-editor]'), 'dismissal lost focus');
});
await test('Split separators support keyboard resizing and restoring equal shares', async () => {
  await win.dispatch({type: 'split', vertical: false}); await settle();
  const gutter = $('.workspace.on .sp-gutter');
  assert(gutter.getAttribute('role') === 'separator' && gutter.getAttribute('aria-orientation') === 'horizontal', 'separator semantics incorrect');
  gutter.focus(); const before = win.commands.length;
  const top = gutter.previousElementSibling, height = top.getBoundingClientRect().height;
  key('ArrowDown'); await settle();
  assert(top.getBoundingClientRect().height > height + 15, 'Down did not resize');
  assert(Number(gutter.getAttribute('aria-valuenow')) > 50, 'separator value is stale');
  key('Home'); await settle();
  assert(Math.abs(top.getBoundingClientRect().height - gutter.nextElementSibling.getBoundingClientRect().height) < 2, 'Home did not reset');
  assert(!win.commands.slice(before).some(c => c.type === 'key'), 'resize leaked into Vim');
  key('Escape'); await settle();
  assert(doc.activeElement.matches('[data-editor]'), 'separator Escape lost focus');
});
await test('Closing a focused split returns focus and disables the last close button', async () => {
  const close = $('.workspace.on .split.on [data-split=close]'); close.focus(); close.click(); await settle();
  assert($('.workspace.on').querySelectorAll('.split').length === 1, 'split stayed open');
  assert(doc.activeElement.matches('[data-editor]'), 'split closure dropped keyboard focus');
  assert($('.workspace.on [data-split=close]').disabled, 'last close button is interactive');
});
await test('File loading indicates busy state and a failed open preserves the file', async () => {
  const previous = $('#sl-path').textContent;
  win.replyDelay = 280; win.failOpen = true;
  try {
    const pending = win.dispatch({type: 'openBuffer', path: 'missing.txt'}); await settle(); await settle();
    assert($('#codex-splits').getAttribute('aria-busy') === 'true' && !$('.editor-loading').hidden, 'loading state absent');
    await pending;
  } finally { win.replyDelay = 0; win.failOpen = false; }
  assert($('.editor-loading').hidden && $('#codex-splits').getAttribute('aria-busy') === 'false', 'loading did not settle');
  assert($('#sl-path').textContent === previous, 'failed open replaced active file');
  assert(!$('.editor-message').hidden && $('.editor-message').dataset.level === 'error', 'open error missing');
  $('.editor-message button').click();
  await win.dispatch({type: 'openBuffer', path: previous});
  assert(!$('#sl-status').textContent.includes('Could not open'), 'successful retry retained a stale error');
  assert($('.editor-message').hidden, 'successful retry retained the error banner');
});
await test('Empty files show guidance that disappears after typing', async () => {
  const viewport = {type: 'viewport', view: 0, firstLine: 0, lines: [''], totalLines: 1, version: 0,
    highlights: [], matches: [], git: [], stops: []};
  await win.pushEvents([viewport]);
  const hint = $('.workspace.on .editor-empty');
  assert(!hint.hidden && hint.textContent.includes('Press i'), 'empty guidance missing');
  await win.pushEvents([{...viewport, lines: ['written'], version: 1}]);
  assert(hint.hidden, 'empty guidance covers written content');
});
await test('Backward visual selection highlights Unicode characters in the editor', async () => {
  await win.pushEvents([
    {type: 'state', mode: 'codex', vimMode: 'visual', vimLabel: 'VISUAL'},
    {type: 'viewport', view: 0, firstLine: 0, lines: ['a😀βdef'], totalLines: 1, version: 0,
      highlights: [], matches: [], git: [], stops: []},
    {type: 'cursor', view: 0, line: 0, column: 1, offset: 1, selection: {line: 0, column: 4}},
  ]);
  const editor = $('.workspace.on [data-editor]');
  assert(editor.querySelector('.caret').textContent === '😀', 'caret split a Unicode character');
  assert(editor.querySelector('.sel')?.textContent === 'βde', 'backward selection lost its highlight');
  await win.pushEvents([{type: 'state', mode: 'codex', vimMode: 'normal', vimLabel: 'NORMAL'}]);
});
await test('A 100,000-line document keeps only visible rows in the DOM', async () => {
  win.installProjectFixtures(); await settle();
  const editor = $('.workspace.on [data-editor]');
  assert(editor.querySelectorAll('.ln').length < 100, 'large file expanded into a full-document DOM');
  assert(editor.textContent.includes('Alpha row 10000'), 'distant viewport did not render');
  const height = parseFloat(editor.querySelector('.doc').style.height);
  assert(height > 1000000, 'scroll range no longer represents the document');
});
await test('Rapid project clicks preserve the final project and its scroll position', async () => {
  win.replyDelay = 30;
  try {
    for (const index of [1, 0, 1, 0, 1]) $(`[data-workspace="${index}"]`).click();
    await settle(); await settle(); await settle(); await settle();
    assert($('#sl-project').textContent === 'Beta', 'final project selection was lost');
    assert($('.workspace.on [data-editor]').textContent.includes('Beta row 30000'), 'stale viewport overwrote the new project scroll');
    assert($('.workspace.on [role=tab][data-buffer]').title.includes('/Beta/same.txt'), 'tab path belongs to a different project');
  } finally { win.replyDelay = 0; }
});
/* ── Chat: Claude Code ─────────────────────────────────────────────── */
const ccRun = (text, extra = {}) => ({kind: 'text', id: `t-${text}`, text, done: true, parent: null, ...extra});
const ccTool = (id, name, input, extra = {}) => ({kind: 'tool', id, name, input, state: 'done', streamed: 0, result: null,
  detail: null, parent: null, ask: null, artifact: null, ...extra});
const ccStatus = (extra = {}) => ({session: 's1', title: 'Fix the build', activity: 'idle', doing: '', model: 'claude-opus-5-5',
  mode: 'default', effort: null, since: null, cost: 0, context: 1200, output: 0, tasks: [], limit: null, error: null, version: '2', ...extra});
const ccCaps = {models: [{value: 'default', resolved: 'claude-opus-5-5', name: 'Default (recommended)', description: 'Opus', efforts: ['low', 'high']},
  {value: 'opus', resolved: 'claude-opus-5-5', name: 'Opus 5.5', description: 'Best', efforts: ['low', 'high']},
  {value: 'haiku', resolved: 'claude-haiku-4-5', name: 'Haiku 4.5', description: 'Fast', efforts: []}],
  commands: [{name: 'compact', description: 'Summarise the conversation', hint: ''}, {name: 'init', description: 'Write a CLAUDE.md', hint: ''}],
  plan: 'Claude Max', email: 'someone@example.com'};
const ccItems = (items, reset = false) => ({type: 'claudeItems', session: 's1', items, reset, earlier: false});
const ccPush = async (...events) => { await win.pushEvents(events); await settle(); };
const sentSince = (before, type) => win.commands.slice(before).filter((c) => c.type === type);
await test('Chat draws Claude Code\'s sessions, prose, tools and status from the core', async () => {
  await win.dispatch({type: 'setMode', mode: 'chat'}); await settle();
  // Paths are shown from the project they are in.
  await ccPush(fixtureProjects);
  await ccPush(
    {type: 'claudeSessions', active: 's1', busy: ['s2'], waiting: [], sessions: [
      {id: 's1', title: 'Fix the build', updated: Date.now()}, {id: 's2', title: 'Port the parser', updated: Date.now() - 1000}]},
    {type: 'claudeCapabilities', capabilities: ccCaps},
    ccItems([
      {kind: 'user', id: 'u1', text: 'Why does it fail?', images: 0},
      {kind: 'thinking', id: 'th1', text: '', tokens: 320, done: true, parent: null},
      ccRun('It fails because **the lockfile** is stale.'),
      ccTool('e1', 'Edit', {file_path: '/fixture/src/fixture.c', old_string: 'a', new_string: 'b'}, {detail: {structuredPatch: [
        {oldStart: 3, oldLines: 1, newStart: 3, newLines: 1, lines: ['-int a;', '+int b;']}]}}),
      ccTool('q1', 'TaskCreate', {subject: 'quiet'}),
      {kind: 'turn', id: 'turn1', ms: 4200, turns: 2, cost: 0.02, error: null},
    ], true),
    {type: 'claudeStatus', status: ccStatus()});
  const ws = $('.workspace[data-ws="chat"]');
  const rows = [...ws.querySelectorAll('.cc-session')];
  assert(rows.length === 2 && rows[0].classList.contains('on') && rows[1].querySelector('.cc-spin'), 'the session list is wrong');
  assert(ws.querySelector('.cc-bubble')?.textContent.includes('Why does it fail?'), 'your message is not a bubble');
  assert(ws.querySelector('.cc-prose strong')?.textContent === 'the lockfile', 'the reply was not drawn as markdown');
  const edit = ws.querySelector('[data-item="e1"]');
  assert(edit.querySelector('.cc-verb').textContent === 'Update' && edit.querySelector('.cc-target').textContent === 'src/fixture.c', 'the edit row does not say what it did');
  assert(edit.querySelector('.cc-note-inline').textContent === '+1 −1', 'the edit row does not count its lines');
  assert(win.getComputedStyle(ws.querySelector('[data-item="q1"]')).display === 'none', 'a task tool has a row of its own');
  assert(ws.querySelector('[data-claude-title]').textContent === 'Fix the build', 'the header does not name the session');
  assert(ws.querySelector('[data-claude-model]').textContent.includes('Opus 5.5'), 'the model pill does not name the model');
  assert(ws.querySelector('[data-claude-mode]').textContent.includes('Ask permissions'), 'the mode pill does not name the mode');
  assert(ws.querySelector('[data-claude-account]').textContent.includes('Claude Max'), 'the account is not shown');
  edit.querySelector('[data-toggle]').click(); await settle();
  assert(edit.querySelector('.cc-diff tr.del') && edit.querySelector('.cc-diff tr.add'), 'the edit did not open into its diff');
});
await test('Chat redraws only the rows that changed, and keeps an open diff open', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  const bubble = ws.querySelector('[data-item="u1"]');
  await ccPush(ccItems([ccRun('Streaming', {id: 't-live', done: false})]));
  await ccPush(ccItems([ccRun('Streaming more', {id: 't-live', done: false})]));
  assert(ws.querySelector('[data-item="u1"]') === bubble, 'an unchanged row was rebuilt');
  assert(ws.querySelector('[data-item="t-live"] .cc-caret'), 'a streaming reply has no caret');
  assert(ws.querySelector('[data-item="e1"] .cc-diff'), 'the open diff closed under a streaming reply');
  const items = ws.querySelectorAll('[data-claude-items] > .cc-item');
  assert(items[items.length - 1].dataset.item === 't-live', 'a new row was not added at the end');
});
await test('A permission card answers the request it was asked with, as allow, always or deny', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  const ask = {request: 'req-7', kind: 'permission', description: 'touch x', blocked: null,
    suggestions: [{type: 'addRules', rules: [{toolName: 'Bash', ruleContent: 'touch:*'}], behavior: 'allow', destination: 'session'}]};
  await ccPush(ccItems([ccTool('b1', 'Bash', {command: 'touch x', description: 'Make x'}, {state: 'waiting', ask})]),
    {type: 'claudeStatus', status: ccStatus({activity: 'waiting'})});
  const card = ws.querySelector('[data-item="b1"] .cc-ask');
  assert(card && card.textContent.includes('run this command') && card.querySelector('code')?.textContent.includes('touch x'), 'the card does not show the command');
  let before = win.commands.length;
  card.querySelector('[data-claude-allow]:not([data-remember])').click(); await settle();
  let answer = sentSince(before, 'claudeAnswer')[0];
  assert(answer?.request === 'req-7' && answer.answer.kind === 'allow' && answer.answer.remember === false, `Yes sent ${JSON.stringify(answer)}`);
  before = win.commands.length;
  card.querySelector('[data-remember="1"]').click(); await settle();
  answer = sentSince(before, 'claudeAnswer')[0];
  assert(answer?.answer.remember === true && card.querySelector('[data-remember="1"]').textContent.includes('Bash(touch:*)'), 'Always did not offer or send the rule');
  before = win.commands.length;
  card.querySelector('[data-claude-why]').value = 'Use mktemp instead';
  card.querySelector('[data-claude-deny]').click(); await settle();
  answer = sentSince(before, 'claudeAnswer')[0];
  assert(answer?.answer.kind === 'deny' && answer.answer.message === 'Use mktemp instead', `No sent ${JSON.stringify(answer)}`);
});
await test('A question card sends the options chosen, by question', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  const questions = [{question: 'Which database?', header: 'Storage', multiSelect: false,
    options: [{label: 'SQLite', description: 'One file'}, {label: 'Postgres', description: 'A server'}]}];
  await ccPush(ccItems([ccTool('a1', 'AskUserQuestion', {questions}, {state: 'waiting', ask: {request: 'req-q', kind: 'question', description: '', suggestions: [], blocked: null}})]));
  const form = ws.querySelector('[data-item="a1"] form[data-claude-question]');
  assert(form && form.querySelectorAll('.cc-option').length === 3, 'the options (and something else) are not offered');
  form.querySelector('input[value="Postgres"]').click();
  const before = win.commands.length;
  form.requestSubmit(); await settle();
  const answer = sentSince(before, 'claudeAnswer')[0];
  assert(answer?.request === 'req-q' && answer.answer.kind === 'questions' && answer.answer.answers['Which database?'] === 'Postgres', `sent ${JSON.stringify(answer)}`);
});
await test('The composer sends, stops, steps the mode, and offers Claude Code\'s commands', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  const input = ws.querySelector('[data-claude-input]');
  input.focus();
  input.value = '/co'; input.dispatchEvent(new win.Event('input', {bubbles: true})); await settle();
  const offered = [...ws.querySelectorAll('[data-claude-command]')].map((r) => r.dataset.claudeCommand);
  assert(offered.includes('compact') && offered.includes('council'), `slash menu offered ${offered}`);
  input.value = 'Run the tests'; input.dispatchEvent(new win.Event('input', {bubbles: true}));
  let before = win.commands.length;
  input.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true})); await settle();
  const sent = sentSince(before, 'claudeSend')[0];
  assert(sent?.text === 'Run the tests' && Array.isArray(sent.images) && input.value === '', `Enter sent ${JSON.stringify(sent)}`);
  assert(!sentSince(before, 'key').length, 'typing in the composer reached Vim');
  await ccPush({type: 'claudeStatus', status: ccStatus({activity: 'working', since: Date.now()})});
  assert(!ws.querySelector('[data-claude-working]').hidden, 'nothing says Claude is working');
  before = win.commands.length;
  ws.querySelector('[data-claude-send]').click(); await settle();
  assert(sentSince(before, 'claudeStop').length === 1, 'the empty send button did not stop the turn');
  before = win.commands.length;
  input.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Tab', shiftKey: true, bubbles: true, cancelable: true})); await settle();
  assert(sentSince(before, 'claudeMode')[0]?.mode === 'acceptEdits', 'Shift+Tab did not step to the next mode');
  await ccPush({type: 'claudeStatus', status: ccStatus()});
});
await test('An artifact published while you watch opens beside the chat, sandboxed, from artifact://', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  const artifact = {id: '4a1f9d2e-77b3-4c41-9e0a-5c2d8f1b6a30', seq: 2, title: 'Frame times', url: 'https://claude.ai/artifact/abc', icon: 'chart', file: 'index.html'};
  await ccPush({type: 'artifacts', artifacts: [{id: artifact.id, url: artifact.url, title: 'Frame times', icon: 'chart', description: '',
    project: '/fixture', session: 's1', versions: [{seq: 1, file: 'index.html', at: 1}, {seq: 2, file: 'index.html', at: 2}]}]});
  await ccPush(ccItems([ccTool('art1', 'Artifact', {file_path: '/fixture/index.html'}, {artifact})]));
  assert(ws.dataset.rightOpen === 'true', 'the artifacts pane did not open');
  const frame = ws.querySelector('[data-artifact-frame]');
  assert(frame.getAttribute('src') === `artifact://localhost/${artifact.id}/2/index.html`, `framed ${frame.getAttribute('src')}`);
  const sandbox = frame.getAttribute('sandbox') ?? '';
  assert(sandbox.includes('allow-scripts') && !sandbox.includes('allow-same-origin'), 'the artifact can reach the editor');
  assert(ws.querySelector('[data-item="art1"] .cc-artifact b')?.textContent === 'Frame times', 'the transcript has no card for it');
  assert(ws.querySelector('[data-artifact-version]').options.length === 2, 'its versions are not offered');
  let before = win.commands.length;
  ws.querySelector('[data-artifact-open]').click(); await settle();
  assert(sentSince(before, 'openLink')[0]?.url === artifact.url, 'Open on claude.ai did not ask for the link');
  ws.querySelector('[data-artifact-close]').click(); await settle();
  assert(ws.dataset.rightOpen === 'false', 'the pane did not close');
});
await test('Artifacts published before Proton kept them import with one click, and a pageless one opens on claude.ai', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  ws.querySelector('[data-artifact-gallery]').click(); await settle();
  let before = win.commands.length;
  ws.querySelector('[data-artifact-import]').click(); await settle();
  assert(sentSince(before, 'importArtifacts').length === 1, 'the button did not ask for an import');
  assert(ws.querySelector('.cc-importing')?.textContent.includes('Reading your sessions'), 'nothing says the import is running');
  await ccPush({type: 'artifacts', importing: false, artifacts: [
    {id: 'Gone1', url: 'https://claude.ai/artifact/Gone1', title: 'Old chart', icon: 'chart', description: '', project: '/fixture', session: 's0',
      versions: [{seq: 1, file: '', at: 5}]}]});
  assert(ws.querySelector('[data-artifact-import]') && ws.querySelectorAll('.cc-gallery-row').length === 1, 'the imported artifact is not listed');
  ws.querySelector('.cc-gallery-open').click(); await settle();
  const empty = ws.querySelector('[data-artifact-empty]');
  assert(!empty.hidden && empty.textContent.includes('no longer on this machine'), 'a pageless artifact did not say where it is');
  assert(ws.querySelector('[data-artifact-frame]').hidden, 'a pageless artifact framed nothing');
  before = win.commands.length;
  empty.querySelector('[data-artifact-open]').click(); await settle();
  assert(sentSince(before, 'openLink')[0]?.url === 'https://claude.ai/artifact/Gone1', 'it did not open on claude.ai');
  ws.querySelector('[data-artifact-close]').click(); await settle();
});
await test('Replies are markdown that never becomes HTML, and their links open in the browser', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  await ccPush(ccItems([ccRun('<img src=x onerror="window.pwned=1"><script>window.pwned=1</script> see [docs](https://example.com/a?b=1&c=2)', {id: 'evil'})]));
  const prose = ws.querySelector('[data-item="evil"]');
  assert(!prose.querySelector('img, script') && prose.textContent.includes('<script>'), 'HTML in a reply was drawn as HTML');
  assert(!win.pwned, 'a reply ran script');
  const link = prose.querySelector('a[data-link]');
  const before = win.commands.length;
  const click = new win.MouseEvent('click', {bubbles: true, cancelable: true});
  link.dispatchEvent(click); await settle();
  assert(click.defaultPrevented && sentSince(before, 'openLink')[0]?.url === 'https://example.com/a?b=1&c=2', 'the link was followed in the window');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('A tool\'s code is coloured by the file it is in: a read, an edit\'s diff, a command — and its output is not', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  await win.dispatch({type: 'setMode', mode: 'chat'}); await settle();
  const source = 'fn main() {\n    /* a comment\n       over two lines */\n    let n: u32 = 42;\n}';
  await ccPush(ccItems([
    ccTool('hl-read', 'Read', {file_path: '/fixture/src/main.rs'}, {detail: {file: {content: source, numLines: 5}}}),
    ccTool('hl-edit', 'Edit', {file_path: '/fixture/src/main.rs', old_string: 'u32', new_string: 'u64'},
      {detail: {structuredPatch: [{oldStart: 1, oldLines: 2, newStart: 1, newLines: 2,
        lines: [' /* keep', '-  of this */ let n: u32 = 42;', '+  of this */ let n: u64 = 43;']}]}}),
    ccTool('hl-bash', 'Bash', {command: 'cargo test --release | grep -v ok'}, {result: 'test result: ok <b>'}),
  ]));
  for (const id of ['hl-read', 'hl-edit', 'hl-bash']) { ws.querySelector(`[data-toggle="${id}"]`).click(); await settle(); }
  const read = ws.querySelector('[data-item="hl-read"] .cc-tool-detail code[data-lang="rust"]');
  assert(read?.querySelector('.k')?.textContent === 'fn' && read.querySelector('.ty')?.textContent === 'u32', 'the file read is not coloured as Rust');
  assert(read.querySelector('.cm')?.textContent === '/* a comment\n       over two lines */', 'a comment over two lines is not one');
  assert(read.textContent === source, 'the code shown is not the code read');
  const rows = [...ws.querySelectorAll('[data-item="hl-edit"] .cc-diff tr:not(.hunk)')];
  assert(rows.length === 3, `${rows.length} diff rows`);
  // The context line opens a comment that the changed lines close.
  assert(rows[1].classList.contains('del') && rows[1].querySelector('td:last-child .cm')?.textContent === '  of this */', `the comment did not carry into the removed line: ${rows[1].innerHTML}`);
  assert(rows[2].classList.contains('add') && rows[2].querySelector('.ty')?.textContent === 'u64' && rows[2].querySelector('.num')?.textContent === '43', 'the added line is not coloured');
  assert(rows[1].lastElementChild.textContent === '  of this */ let n: u32 = 42;', 'the removed line is not what was removed');
  const bash = ws.querySelector('[data-item="hl-bash"] .cc-tool-detail');
  assert(bash.querySelector('code[data-lang="bash"] .fn')?.textContent === 'cargo' && bash.querySelector('code .at')?.textContent === '--release', 'the command is not coloured');
  assert(bash.querySelector('.cc-output').textContent === 'test result: ok <b>' && !bash.querySelector('.cc-output b, .cc-output span'), 'the output was coloured or drawn as HTML');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('The desktop app\'s sessions from other folders are listed apart, by folder, and one open elsewhere says so', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  await ccPush({type: 'claudeSessions', active: 's1', busy: [], waiting: [], engine: 'claude', sessions: [
    {id: 's1', title: 'Fix the build', updated: Date.now(), cwd: '', elsewhere: false, engine: 'claude'},
    {id: 'x1', title: 'Make the poll configurable', updated: Date.now() - 1000, cwd: '', elsewhere: false, engine: 'codex'},
    {id: 'd1', title: 'Batting averages', updated: Date.now() - 5000, cwd: '/Users/you/Github/Cricket-Analysis', elsewhere: true, engine: 'claude'},
    {id: 'd2', title: 'Quantisation and SNR', updated: Date.now() - 9000, cwd: '/Users/you/Research/TinyML', elsewhere: false, engine: 'claude'}]});
  const groups = [...ws.querySelectorAll('.cc-group')].map((g) => g.textContent);
  assert(groups[groups.length - 1] === 'Claude desktop', `groups are ${groups}`);
  const row = (id) => ws.querySelector(`[data-claude-session="${id}"]`);
  assert(row('d1').querySelector('.cc-session-where')?.textContent === 'Cricket-Analysis', 'a desktop session does not name its folder');
  assert(row('d1').querySelector('.cc-elsewhere') && !row('d2').querySelector('.cc-elsewhere'), 'a session open in another Claude Code is not marked');
  assert(row('x1').querySelector('.cc-session-where[data-engine="codex"]')?.textContent === 'Codex', 'a Codex conversation does not say so');
  assert(!row('s1').querySelector('.cc-session-where'), 'a Claude Code session here is tagged');
  const before = win.commands.length;
  row('d1').click(); await settle();
  assert(sentSince(before, 'claudeOpen')[0]?.session === 'd1', 'opening a desktop session did not ask for it');
});
await test('The engine picker starts a conversation with Codex, and the composer speaks its language', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  const codexModes = [{value: 'default', name: 'Ask permissions', note: 'Ask before commands and edits', icon: 'eye'},
    {value: 'acceptEdits', name: 'Auto', note: 'Work in the project; ask to go beyond it', icon: 'pen'},
    {value: 'plan', name: 'Read only', note: 'Change nothing', icon: 'bulb'}];
  await ccPush(
    {type: 'claudeCapabilities', capabilities: {engine: 'codex', plan: 'ChatGPT Plus', email: 'someone@example.com', missing: null, commands: [], modes: codexModes,
      models: [{value: 'gpt-6.1-sol', resolved: 'gpt-6.1-sol', name: 'GPT-6.1-Sol', description: 'Workhorse', efforts: ['low', 'high']}]}},
    {type: 'claudeCapabilities', capabilities: {engine: 'gemini', plan: '', email: '', missing: 'The Gemini CLI is not installed.', commands: [], modes: [], models: []}});
  ws.querySelector('[data-claude-engine]').click(); await settle();
  const pop = ws.querySelector('[data-claude-pop]');
  const rows = [...pop.querySelectorAll('[data-claude-set-engine]')];
  assert(rows.map((r) => r.dataset.claudeSetEngine).join() === 'claude,codex,gemini,antigravity', 'the picker does not offer the four engines');
  const gemini = pop.querySelector('[data-claude-set-engine="gemini"]');
  assert(gemini.classList.contains('off') && gemini.textContent.includes('Not installed'), 'an engine that is not installed is not said to be');
  let before = win.commands.length;
  pop.querySelector('[data-claude-set-engine="codex"]').click(); await settle();
  assert(sentSince(before, 'claudeEngine')[0]?.engine === 'codex', 'choosing Codex did not ask for it');
  await ccPush(ccItems([], true), {type: 'claudeStatus', status: ccStatus({engine: 'codex', model: 'gpt-6.1-sol', title: '', context: 0})});
  const input = ws.querySelector('[data-claude-input]');
  assert(ws.querySelector('[data-claude-engine]').textContent.includes('Codex'), 'the engine pill does not name Codex');
  assert(input.placeholder.startsWith('Ask Codex'), `the placeholder is ${input.placeholder}`);
  assert(ws.querySelector('[data-claude-welcome-title]').textContent === 'What should Codex work on?', 'the welcome is still Claude\'s');
  assert(ws.querySelector('[data-claude-model]').textContent.includes('GPT-6.1-Sol'), 'the model pill does not name Codex\'s model');
  assert(ws.querySelector('[data-claude-account]').textContent.includes('ChatGPT Plus'), 'the account is not Codex\'s');
  input.focus();
  before = win.commands.length;
  input.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Tab', shiftKey: true, bubbles: true, cancelable: true})); await settle();
  assert(sentSince(before, 'claudeMode')[0]?.mode === 'acceptEdits', 'Shift+Tab did not step through Codex\'s modes');
  await ccPush({type: 'claudeStatus', status: ccStatus({engine: 'codex', model: 'gpt-6.1-sol', mode: 'acceptEdits', title: ''})});
  assert(ws.querySelector('[data-claude-mode]').textContent.includes('Auto'), 'the mode pill does not use Codex\'s name for the mode');
});
await test('A Codex patch reads as an edit of its file, and its approval asks in Codex\'s name', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  const files = [{path: '/fixture/src/poll.ts', kind: 'update', movePath: null,
    structuredPatch: [{oldStart: 3, oldLines: 1, newStart: 3, newLines: 1, lines: ['-export const POLL_MS = 1000;', '+export const POLL_MS = 2000;']}]}];
  const ask = {request: 'codex-3', kind: 'permission', description: '', suggestions: [{type: 'session'}], blocked: null};
  await ccPush(ccItems([ccTool('p1', 'Patch', {files: [{path: files[0].path, kind: 'update'}]}, {state: 'waiting', ask, detail: {files}})]),
    {type: 'claudeStatus', status: ccStatus({engine: 'codex', model: 'gpt-6.1-sol', activity: 'waiting'})});
  const row = ws.querySelector('[data-item="p1"]');
  assert(row.querySelector('.cc-verb').textContent === 'Update' && row.querySelector('.cc-target').textContent === 'src/poll.ts', 'the patch row does not say what it changes');
  const card = row.querySelector('.cc-ask');
  assert(card?.textContent.includes('Allow Codex to edit') && card.querySelector('.cc-diff tr.add'), 'the approval is not Codex\'s, or shows no diff');
  const before = win.commands.length;
  card.querySelector('[data-remember="1"]').click(); await settle();
  const answer = sentSince(before, 'claudeAnswer')[0];
  assert(answer?.request === 'codex-3' && answer.answer.remember === true, `"for this session" sent ${JSON.stringify(answer)}`);
  await ccPush({type: 'claudeStatus', status: ccStatus()}, {type: 'claudeCapabilities', capabilities: ccCaps});
});
const folderEvent = (folders, sessions = []) => ({type: 'claudeSessions', active: 's1', busy: [], waiting: ['f2'], engine: 'claude', folders, sessions});
const chatRow = (id, title, extra = {}) => ({id, title, updated: Date.now(), cwd: '', elsewhere: false, engine: 'claude', ...extra});
await test('Chats sit in folders: a folder folds, says what is waiting in it, and its menu renames or deletes it', async () => {
  // Focus and the pane need the tab on screen.
  await win.dispatch({type: 'setMode', mode: 'chat'}); await settle();
  const ws = $('.workspace[data-ws="chat"]');
  await ccPush(folderEvent([
    {id: 'F1', name: 'Research', collapsed: false, sessions: [chatRow('f1', 'Linear Programming'), chatRow('f2', 'Convex Optimization', {cwd: '/Users/you/Github'})]},
    {id: 'F2', name: 'Empty', collapsed: false, sessions: []},
  ], [chatRow('s1', 'Fix the build')]));
  const research = ws.querySelector('[data-folder="F1"]');
  assert(research.querySelector('.cc-folder-name').textContent === 'Research' && research.querySelector('.cc-folder-count').textContent === '2', 'the folder does not name itself and count its chats');
  assert(research.querySelectorAll('.cc-session').length === 2 && !ws.querySelector('.cc-loose [data-claude-session="f1"]'), 'a chat in a folder is not drawn there, or is drawn twice');
  assert(research.querySelector('[data-claude-session="f2"] .cc-session-where')?.textContent === 'Github', 'a chat from another folder does not say which');
  assert(ws.querySelector('[data-folder="F2"] .cc-folder-empty'), 'an empty folder does not say how to fill it');
  let before = win.commands.length;
  research.querySelector('[data-folder-toggle]').click(); await settle();
  assert(sentSince(before, 'chatFolderCollapse')[0]?.collapsed === true, 'folding was not kept');
  const folded = ws.querySelector('[data-folder="F1"]');
  assert(!folded.querySelector('.cc-session') && folded.querySelector('.cc-folder-head .cc-wait'), 'a folded folder shows its chats, or hides that one is waiting');
  ws.querySelector('[data-folder-menu="F2"]').click(); await settle();
  const menu = ws.querySelector('.cc-side-menu');
  assert(!menu.hidden && menu.textContent.includes('Rename') && menu.textContent.includes('Delete folder'), 'the folder menu is wrong');
  before = win.commands.length;
  menu.querySelector('[data-folder-delete="F2"]').click(); await settle();
  assert(sentSince(before, 'chatFolderDelete')[0]?.folder === 'F2' && menu.hidden, 'deleting did not ask the core');
  ws.querySelector('[data-folder-menu="F1"]').click(); await settle();
  ws.querySelector('.cc-side-menu [data-folder-rename="F1"]').click(); await settle();
  const field = ws.querySelector('[data-folder-input="F1"]');
  assert(field && doc.activeElement === field && field.selectionEnd - field.selectionStart === 'Research'.length, 'renaming does not put the name in a field, selected');
  before = win.commands.length;
  field.value = 'Optimisation'; field.dispatchEvent(new win.Event('input', {bubbles: true}));
  field.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true})); await settle();
  const renamed = sentSince(before, 'chatFolderRename')[0];
  assert(renamed?.folder === 'F1' && renamed.name === 'Optimisation', `renaming sent ${JSON.stringify(renamed)}`);
  assert(!sentSince(before, 'key').length && !sentSince(before, 'claudeStop').length, 'the name reached Vim or stopped the turn');
});
await test('A chat moves into a folder by its menu or by being dragged there, and a new folder is named in place', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  await ccPush(folderEvent([{id: 'F1', name: 'Research', collapsed: false, sessions: [chatRow('f1', 'Linear Programming')]}],
    [chatRow('s1', 'Fix the build'), chatRow('s3', 'Port the parser')]));
  ws.querySelector('[data-session-menu="s3"]').click(); await settle();
  const menu = ws.querySelector('.cc-side-menu');
  assert([...menu.querySelectorAll('[data-move-to]')].map((b) => b.textContent).join() === 'Research' && menu.querySelector('[data-new-folder-with="s3"]'), 'the chat menu does not offer the folders and a new one');
  let before = win.commands.length;
  menu.querySelector('[data-move-to="F1"]').click(); await settle();
  let moved = sentSince(before, 'chatFolderMove')[0];
  assert(moved?.session === 's3' && moved.folder === 'F1', `the menu moved ${JSON.stringify(moved)}`);
  // Dragged onto the folder, and a folder's chat dragged back to the list.
  const drag = async (row, onto) => {
    const data = new win.DataTransfer();
    row.dispatchEvent(new win.DragEvent('dragstart', {bubbles: true, cancelable: true, dataTransfer: data}));
    const over = new win.DragEvent('dragover', {bubbles: true, cancelable: true, dataTransfer: data});
    onto.dispatchEvent(over);
    onto.dispatchEvent(new win.DragEvent('drop', {bubbles: true, cancelable: true, dataTransfer: data}));
    row.dispatchEvent(new win.DragEvent('dragend', {bubbles: true, dataTransfer: data}));
    await settle();
    return over.defaultPrevented;
  };
  before = win.commands.length;
  assert(await drag(ws.querySelector('[data-claude-session="s1"]'), ws.querySelector('[data-folder="F1"] .cc-folder-toggle')), 'a folder does not take a dragged chat');
  moved = sentSince(before, 'chatFolderMove')[0];
  assert(moved?.session === 's1' && moved.folder === 'F1', `dragging in moved ${JSON.stringify(moved)}`);
  before = win.commands.length;
  await drag(ws.querySelector('[data-claude-session="f1"]'), ws.querySelector('.cc-loose'));
  moved = sentSince(before, 'chatFolderMove')[0];
  assert(moved?.session === 'f1' && moved.folder === null, `dragging out moved ${JSON.stringify(moved)}`);
  before = win.commands.length;
  ws.querySelector('[data-claude-new-folder]').click(); await settle();
  const asked = sentSince(before, 'chatFolderNew')[0];
  assert(asked && asked.name === '' && asked.sessions.length === 0, `the new folder button sent ${JSON.stringify(asked)}`);
  await ccPush(folderEvent([{id: 'F1', name: 'Research', collapsed: false, sessions: []}, {id: 'F9', name: 'New folder', collapsed: false, sessions: []}]));
  const field = ws.querySelector('[data-folder-input="F9"]');
  assert(field && doc.activeElement === field && field.value === 'New folder', 'a new folder is not waiting to be named');
  field.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
  assert(!ws.querySelector('[data-folder-input]'), 'Escape did not leave the name as it was');
  await ccPush(folderEvent([], [chatRow('s1', 'Fix the build')]));
});
await test('A chat is deleted from its menu once you confirm, and one open in another Claude Code cannot be', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  await ccPush(folderEvent([], [chatRow('s1', 'Fix the build'), chatRow('old', 'Old experiment'), chatRow('live', 'Open elsewhere', {cwd: '/Users/you/Github', elsewhere: true})]));
  ws.querySelector('[data-session-menu="old"]').click(); await settle();
  const menu = ws.querySelector('.cc-side-menu');
  let before = win.commands.length;
  menu.querySelector('[data-delete-ask="old"]').click(); await settle();
  assert(!sentSince(before, 'claudeDelete').length, 'it was deleted without asking');
  assert(menu.textContent.includes('Delete “Old experiment”?') && menu.textContent.includes('Trash'), 'the question does not name the chat or say where it goes');
  assert(doc.activeElement === menu.querySelector('[data-delete-cancel]'), 'Enter would delete rather than cancel');
  menu.querySelector('[data-delete-cancel]').click(); await settle();
  assert(menu.hidden && !sentSince(before, 'claudeDelete').length, 'cancelling deleted it');
  ws.querySelector('[data-session-menu="old"]').click(); await settle();
  ws.querySelector('.cc-side-menu [data-delete-ask="old"]').click(); await settle();
  before = win.commands.length;
  ws.querySelector('.cc-side-menu [data-delete-chat="old"]').click(); await settle();
  assert(sentSince(before, 'claudeDelete')[0]?.session === 'old', 'confirming did not ask the core to delete it');
  ws.querySelector('[data-session-menu="live"]').click(); await settle();
  const offered = ws.querySelector('.cc-side-menu');
  assert(!offered.querySelector('[data-delete-ask]') && offered.textContent.includes('Close it in the other Claude Code first'), 'a chat open elsewhere could be deleted');
  offered.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
  assert(offered.hidden, 'Escape did not close the menu');
});
await test('Each chat keeps its own artifacts pane, and one published while you were elsewhere is waiting', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  const artifact = (id, session, seq = 1) => ({id, url: `https://claude.ai/artifact/${id}`, title: `Artifact ${id}`, icon: 'chart', description: '',
    project: '/fixture', session, versions: Array.from({length: seq}, (_, i) => ({seq: i + 1, file: 'index.html', at: Date.now()}))});
  const open = () => ws.dataset.rightOpen === 'true';
  const shown = () => ws.dataset.artifact ?? null;
  const visit = (session) => ccPush({type: 'claudeItems', session, items: [{kind: 'user', id: `u-${session}`, text: session, images: 0}], reset: true, earlier: false});
  // Brought in as an import is, so neither counts as published while away.
  await ccPush({type: 'artifacts', importing: true, artifacts: [artifact('A1', 'milp'), artifact('B1', 'cvx')]},
    {type: 'artifacts', importing: false, artifacts: [artifact('A1', 'milp'), artifact('B1', 'cvx')]});
  await visit('milp');
  assert(!open(), 'a chat that never opened an artifact has its pane open');
  await ccPush({type: 'claudeItems', session: 'milp', reset: false, earlier: false, items: [ccTool('pub', 'Artifact', {title: 'A'}, {
    artifact: {id: 'A1', seq: 1, title: 'Artifact A1', url: 'https://claude.ai/artifact/A1', icon: 'chart', file: 'index.html'}})]});
  assert(open() && shown() === 'A1', 'the MILP chat did not open its artifact');
  await visit('cvx');
  assert(!open() && shown() === null, 'the CVX chat inherited the MILP chat\'s pane');
  ws.querySelector('[data-artifacts-toggle]').click(); await settle();
  const gallery = ws.querySelector('[data-artifact-gallery-list]');
  assert(gallery.querySelector('.cc-gallery-group')?.textContent === 'In this chat' && gallery.querySelector('.cc-gallery-row [data-claude-artifact="B1"]'), 'the list does not put this chat\'s artifacts first');
  gallery.querySelector('[data-claude-artifact="B1"]').click(); await settle();
  assert(open() && shown() === 'B1', 'the CVX chat did not open its own artifact');
  await visit('milp');
  assert(open() && shown() === 'A1', 'going back to the MILP chat did not bring back its artifact');
  ws.querySelector('[data-artifact-close]').click(); await settle();
  await visit('cvx');
  assert(open() && shown() === 'B1', 'closing the MILP chat\'s pane closed the CVX chat\'s');
  await visit('milp');
  assert(!open(), 'the MILP chat forgot it had closed its pane');
  // The MILP chat publishes a second version while the CVX chat is on screen.
  await visit('cvx');
  await ccPush({type: 'artifacts', importing: false, artifacts: [artifact('A1', 'milp', 2), artifact('B1', 'cvx')]});
  assert(shown() === 'B1', 'another chat publishing changed this chat\'s pane');
  await visit('milp');
  assert(open() && shown() === 'A1' && ws.querySelector('[data-artifact-frame]').getAttribute('src').includes('/A1/2/'), 'what the MILP chat published while away was not waiting open');
  await ccPush(ccItems([], true), {type: 'claudeStatus', status: ccStatus()});
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('No browser runtime errors', () => {
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
});
await test('Codex and Data ask for an opaque window in their ground, and the glass modes give it back', async () => {
  const asked = () => (win.invokes ?? []).filter((c) => c.name === 'set_opaque').map((c) => c.args.color);
  const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
  let before = asked().length;
  await win.pushEvents([{type: 'modeChanged', mode: 'design'}]);
  assert(asked().slice(before).includes(null), `leaving Codex for Design kept the window opaque: ${JSON.stringify(asked().slice(before))}`);
  before = asked().length;
  $('.rail [data-mode="data"]')?.click(); await settle();
  await win.pushEvents([{type: 'modeChanged', mode: 'data'}]);
  assert(asked().slice(before).at(-1) === ground, `Data did not ask for the ground: ${JSON.stringify(asked().slice(before))}`);
  before = asked().length;
  await win.pushEvents([{type: 'modeChanged', mode: 'design'}]);
  await win.pushEvents([{type: 'modeChanged', mode: 'codex'}]);
  const now = asked().slice(before);
  assert(now.includes(null), `Design kept the window opaque: ${JSON.stringify(now)}`);
  const last = now.at(-1);
  assert(/^#[0-9a-f]{6}$/.test(last ?? ''), `Codex did not ask for its ground: ${JSON.stringify(now)}`);
  assert(last === ground, `asked for ${last}, the theme's ground is ${ground}`);
});
await test('Codex provider selection clears the previous model and displays Astra', async () => {
  await win.dispatch({type: 'setSetting', key: 'agent.model', value: 'claude-opus-5'});
  $('.rail [data-open="settings"]').click(); await settle();
  $('[data-st-nav="models"]').click();
  const before = win.commands.length;
  $('[data-set-choice="agent.provider"] [data-value="codex"]').click(); await settle();
  const writes = win.commands.slice(before).filter(c => c.type === 'setSetting');
  assert(writes[0]?.key === 'agent.model' && writes[0].value === null, 'old provider model survived');
  assert(writes[1]?.key === 'agent.provider' && writes[1].value === 'codex', 'Codex was not selected');
  await win.pushEvents([{type: 'agent', transcript: [], activity: 'idle', provider: 'codex', model: 'gpt-6-astra',
    credential: 'Uses your Codex sign-in', inputTokens: 0, outputTokens: 0, cachedTokens: 0}]);
  assert($('[data-agent-provider]').textContent === 'Codex', 'provider is mislabeled');
  assert($('[data-model-pick]').value === 'gpt-6-astra', 'Astra is not selected');
  key('Escape'); await settle();
});
await test('Antigravity is a provider, with its own models and its own tools named', async () => {
  $('.rail [data-open="settings"]').click(); await settle();
  $('[data-st-nav="models"]').click();
  const before = win.commands.length;
  $('[data-set-choice="agent.provider"] [data-value="antigravity"]').click(); await settle();
  const writes = win.commands.slice(before).filter(c => c.type === 'setSetting');
  assert(writes.some(c => c.key === 'agent.provider' && c.value === 'antigravity'), 'Antigravity was not selected');
  assert($('[data-set-bool="agent.antigravityEdits"]') && !$('[data-set-bool="agent.antigravityEdits"]').checked, 'edits are not off by default');
  key('Escape'); await settle();
  await win.pushEvents([{type: 'agent', provider: 'antigravity', model: 'gemini-3.8-flash-high',
    credential: 'Uses your Antigravity sign-in', activity: 'idle', inputTokens: 0, outputTokens: 0, cachedTokens: 0,
    transcript: [{speaker: 'you', text: 'what does main do?', thinking: '', streaming: false, calls: []},
      {speaker: 'proton', text: 'It parses arguments.', thinking: '', streaming: false,
        calls: [{id: 'agy-2', name: 'view_file', detail: 'AbsolutePath=/fixture/main.c', result: '', state: 'done'}]}]}]);
  assert($('[data-agent-provider]').textContent === 'Antigravity', 'provider is mislabeled');
  const models = [...$('[data-model-pick]').options].map(o => o.value);
  assert(models.includes('gemini-3.1-pro-high') && $('[data-model-pick]').value === 'gemini-3.8-flash-high', `wrong models: ${models}`);
  assert([...doc.querySelectorAll('#codex-dock .tool b')].some(b => b.textContent === 'Read file'), 'its tool row is unnamed');
});
await test('An answer in the agent panel colours its code, draws its TeX, keeps its citations, and never becomes HTML', async () => {
  const agent = (transcript) => win.pushEvents([{type: 'agent', provider: 'codex', model: 'gpt-6-astra', credential: '',
    activity: 'idle', inputTokens: 0, outputTokens: 0, cachedTokens: 0, transcript}]);
  const answer = ['Run it like this:', '', '```rust', '#[derive(Debug)]', 'fn main() { let n: u32 = 42; println!("{n}"); } // done', '```', '',
    'The energy is $E = mc^2$; $5 and $10 are prices, and \\$x\\$ is not TeX. Then', '$$', '\\int_0^1 x^2\\,dx = \\tfrac{1}{3}', '$$',
    'See [[notes/physics.md § Energy L1-4]] and [the docs](https://example.com/a).', '', '<img src=x onerror="window.pwned=1">'].join('\n');
  await agent([{speaker: 'you', text: 'why is $x$ here?', thinking: '', streaming: false, calls: []},
    {speaker: 'proton', text: answer, thinking: 'Recall $a^2 + b^2 = c^2$.', streaming: false, calls: []}]);
  const thread = $('#codex-dock [data-agent-thread]');
  const reply = () => thread.querySelector('.msg:not(.you) > .md');
  const code = reply().querySelector('.md-code code[data-lang="rust"]');
  assert(code?.querySelector('.k')?.textContent === 'fn', 'a Rust keyword is not coloured');
  assert(code.querySelector('.at')?.textContent === '#[derive(Debug)]' && code.querySelector('.ty')?.textContent === 'u32', 'an attribute or a type is not coloured');
  assert(code.querySelector('.num')?.textContent === '42' && code.querySelector('.cm')?.textContent === '// done', 'a number or a comment is not coloured');
  assert(code.textContent === '#[derive(Debug)]\nfn main() { let n: u32 = 42; println!("{n}"); } // done', 'the code is not what was written');
  // KaTeX loads the first time there is an equation; until then each is its source.
  for (let i = 0; i < 30 && thread.querySelector('.md-math[data-tex]'); i++) await settle();
  const maths = [...reply().querySelectorAll('.md-math')];
  assert(maths.length === 2, `${maths.length} equations, not 2`);
  assert(maths[0].querySelector('.katex') && !maths[0].classList.contains('md-display'), 'the inline equation was not drawn');
  assert(maths[1].closest('.md-math-block') && maths[1].querySelector('.katex-display'), 'the display equation was not drawn on its own');
  assert(maths[0].querySelector('annotation')?.textContent === 'E = mc^2', 'the equation drawn is not the one written');
  assert(reply().textContent.includes('$5 and $10 are prices') && reply().textContent.includes('$x$ is not TeX'), 'prices or escaped dollars were read as TeX');
  assert(thread.querySelector('.thought .md .katex'), 'the thinking\'s equation was not drawn');
  assert(thread.querySelector('.msg.you').textContent.includes('why is $x$ here?'), 'what you wrote was read as TeX');
  assert(reply().querySelector('.cite')?.dataset.cite === 'notes/physics.md § Energy L1-4', 'the citation was lost');
  assert(!reply().querySelector('img') && !win.pwned && reply().textContent.includes('<img'), 'HTML in an answer was drawn as HTML');

  // A link opens in the browser; the copy button copies the code as written.
  let before = win.commands.length;
  const click = new win.MouseEvent('click', {bubbles: true, cancelable: true});
  reply().querySelector('a[data-link]').dispatchEvent(click); await settle();
  assert(click.defaultPrevented && win.commands.slice(before).find((c) => c.type === 'openLink')?.url === 'https://example.com/a', 'the link was followed in the window');
  let copied = null;
  Object.defineProperty(win.navigator, 'clipboard', {configurable: true, value: {writeText: async (text) => { copied = text; }}});
  reply().querySelector('[data-copy]').click(); await settle();
  delete win.navigator.clipboard;
  assert(copied === code.textContent, 'the copy button did not copy the code');

  // An equation still being written is its source until it closes.
  await agent([{speaker: 'proton', text: 'So\n$$\n\\frac{a}{', thinking: '', streaming: true, calls: []}]);
  assert(reply().querySelector('.md-math.md-pending')?.textContent.includes('\\frac{a}{'), 'an unfinished equation is not shown as its source');
  await agent([{speaker: 'proton', text: 'So\n$$\n\\frac{a}{b}\n$$', thinking: '', streaming: false, calls: []}]);
  assert(reply().querySelector('.md-math-block .katex-display') && !reply().querySelector('.md-pending'), 'the finished equation was not drawn');
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
});
await test('The agent panel keeps its threads: the clock lists them, a row opens one, and one is deleted after asking', async () => {
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  // An earlier check may have left the dock on another of its panels.
  $('#codex-dock .dock-tab[data-dockpanel="agent"]').click(); await settle();
  assert($('#codex-dock').dataset.showing === 'agent', 'the dock is not on the agent');
  await win.pushEvents([{type: 'chats', current: '.proton/chats/why.json', conversations: [
    {path: '.proton/chats/why.json', title: 'Why is it slow', turns: 4, provider: 'claude'},
    {path: '.proton/chats/notes.json', title: 'Notes on <b>caching</b>', turns: 1, provider: 'codex'}]}]);
  const list = $('[data-chat-threads]'), clock = $('[data-chat-history]');
  assert(list.hidden, 'the threads are showing before they were asked for');
  assert($('[data-dock-title]').textContent === 'Why is it slow', 'the header does not name the open thread');
  clock.click(); await settle();
  assert(!list.hidden && clock.getAttribute('aria-expanded') === 'true', 'the clock did not open the threads');
  const rows = [...list.querySelectorAll('.row.chat[data-chat]')];
  assert(rows.length === 2 && rows[0].classList.contains('sel') && !rows[1].classList.contains('sel'), 'the threads are not listed with the open one marked');
  assert(rows[1].querySelector('.chat-name').textContent === 'Notes on <b>caching</b>' && rows[1].querySelector('.chat-meta').textContent === '1 turn · Codex',
    `the row says ${rows[1].textContent}`);
  let before = win.commands.length;
  rows[1].click(); await settle();
  assert(sentSince(before, 'openChat')[0]?.path === '.proton/chats/notes.json' && list.hidden, 'a row did not open its thread');

  // Deleting asks first, in the row; Cancel keeps it.
  clock.click(); await settle();
  list.querySelector('[data-chat-forget=".proton/chats/notes.json"]').click(); await settle();
  assert(list.querySelector('.row.chat.ask')?.textContent.includes('Delete “Notes on <b>caching</b>”?'), 'deleting did not ask');
  before = win.commands.length;
  list.querySelector('[data-chat-keep]').click(); await settle();
  assert(!list.querySelector('.row.chat.ask') && !sentSince(before, 'deleteChat').length && !list.hidden, 'cancelling deleted it or closed the list');
  list.querySelector('[data-chat-forget=".proton/chats/notes.json"]').click(); await settle();
  list.querySelector('[data-chat-forget-yes]').click(); await settle();
  assert(sentSince(before, 'deleteChat')[0]?.path === '.proton/chats/notes.json', 'confirming did not delete it');
  assert(!sentSince(before, 'openChat').length, 'deleting a thread opened it');

  // Somewhere else puts the list away.
  doc.querySelector('.workspace.on')?.click(); await settle();
  assert(list.hidden && clock.getAttribute('aria-expanded') === 'false', 'the list stayed open');
});
await test('A notebook colours its code cells as they are typed, and its prose draws code, TeX and links', async () => {
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  const cell = (kind, source, selected = false) => ({kind, source, outputs: [], executionCount: null, running: false, selected});
  await win.pushEvents([{type: 'notebook', path: '/fixture/fit.ipynb', problem: '', language: 'python', cells: [
    cell('markdown', '# Fit\nThe loss is $L = \\frac{1}{n}\\sum_i (y_i - \\hat y_i)^2$; see [the docs](https://example.com/fit).\n\n```python\nprint("hi")\n```'),
    cell('code', 'import numpy as np\ndef fit(x):\n    return np.mean(x)  # the average', true)]}]);
  await settle();
  const box = $('.workspace[data-ws="data"] [data-cells]');
  const code = box?.querySelector('[data-cell="1"]');
  const behind = code?.querySelector('.cell-code > .cell-hl'), src = code?.querySelector('.cell-code > .cell-src[data-src="1"]');
  assert(behind?.querySelector('.k')?.textContent === 'import' && behind.querySelector('.cm')?.textContent === '# the average', 'the code cell is not coloured as Python');
  assert(src.textContent === 'import numpy as np\ndef fit(x):\n    return np.mean(x)  # the average' && !src.querySelector('span'), 'what is typed into is not plain text');
  assert(win.getComputedStyle(src).color === 'rgba(0, 0, 0, 0)', 'the typed text is not clear over its colours');
  const a = src.getBoundingClientRect(), b = behind.getBoundingClientRect();
  assert(Math.abs(a.top - b.top) < 1 && Math.abs(a.left - b.left) < 1 && Math.abs(a.width - b.width) < 1, 'the colours are not under the text');

  // Typing recolours what is behind and leaves what is typed into alone —
  // in the markup a browser types: Chrome ends each line with a <br>, and
  // keeps one more at the end for an empty last line.
  src.focus();
  src.innerHTML = 'while True:<br>    pass<br><br>';
  src.dispatchEvent(new win.InputEvent('input', {bubbles: true}));
  assert(behind.querySelector('.k')?.textContent === 'while' && behind.querySelector('.cn')?.textContent === 'True', 'typing did not recolour the cell');
  assert(behind.textContent === 'while True:\n    pass\n', `the colours lost a line: ${JSON.stringify(behind.textContent)}`);
  let sent = win.commands.length;
  src.blur(); await settle();
  assert(win.commands.slice(sent).find((c) => c.type === 'setCell')?.source === 'while True:\n    pass\n', 'the cell was saved with its lines run together');

  const prose = box.querySelector('[data-cell="0"] .cell-md');
  for (let i = 0; i < 30 && prose.querySelector('.md-math[data-tex]'); i++) await settle();
  assert(prose.querySelector('h1')?.textContent === 'Fit' && prose.querySelector('.md-math .katex'), 'the prose is not drawn, TeX and all');
  assert(prose.querySelector('.md-code code[data-lang="python"] .fn')?.textContent === 'print', 'a code block in the prose is not coloured');
  sent = win.commands.length;
  const click = new win.MouseEvent('click', {bubbles: true, cancelable: true});
  prose.querySelector('a[data-link]').dispatchEvent(click); await settle();
  assert(click.defaultPrevented && win.commands.slice(sent).find((c) => c.type === 'openLink')?.url === 'https://example.com/fit',
    'a link in a notebook was followed in the window');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('In a notebook, i puts the caret in the selected cell', async () => {
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  const cell = (kind, source, selected = false) => ({kind, source, outputs: [], executionCount: null, running: false, selected});
  await win.pushEvents([{type: 'notebook', path: '/fixture/fit.ipynb', problem: '', language: 'python', cells: [
    cell('code', 'import numpy as np'), cell('code', 'np.mean([1, 2, 3])', true)]}]);
  await settle();
  // What the core sends for `i`. The caret goes in at the next frame.
  await win.pushEvents([{type: 'action', action: {verb: 'Insert', object: 'Here', count: 1, register: null}, description: 'insert'}]);
  await new Promise((done) => win.requestAnimationFrame(done));
  const src = $('.workspace[data-ws="data"] .split.on [data-cell="1"] .cell-src');
  assert(src && doc.activeElement === src, 'i did not put the caret in the selected cell');
  assert($('#sl-status').textContent === 'insert', 'the action no longer shows in the statusline');
  src.blur(); await settle();
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('Research sits beside Data after Codex: ⌃4 opens it, in Codex\'s chrome and on an opaque ground', async () => {
  const asked = () => (win.invokes ?? []).filter((c) => c.name === 'set_opaque').map((c) => c.args.color);
  const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
  await win.dispatch({type: 'setMode', mode: 'design'}); await settle();
  const before = win.commands.length, opaque = asked().length;
  doc.body.focus();
  key('4', {ctrlKey: true}); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'setMode' && c.mode === 'research'), '⌃4 did not ask for Research');
  assert($('.workspace.on')?.dataset.ws === 'research', 'Research is not on screen');
  assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.rail')) && !shown($('.statusline')), 'Research kept the old chrome');
  assert(!shown($('.titlebar [data-term-toggle]')), 'Research offers a terminal it cannot show');
  const tab = $('.titlebar [data-mode="research"]');
  assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃4'), 'the title bar does not show Research');
  // Codex, Data and Research side by side, in the title bar and on the rail.
  for (const list of ['.tb-modes', '.mode-rail']) {
    const order = [...doc.querySelectorAll(`${list} [data-mode]`)].map((b) => b.dataset.mode).join();
    assert(order === 'chat,codex,data,research,design,modeling,game,video', `${list} is not in the new order: ${order}`);
  }
  assert(asked().slice(opaque).at(-1) === ground, `Research did not ask for the ground: ${JSON.stringify(asked().slice(opaque))}`);
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
const researchRow = (id, elo, seat, title, status = 'active') => ({id, title, author: {seat, model: ''}, family: seat === 'claude' ? 'anthropic' : 'openai',
  round: 1, status, elo, wins: elo > 1200 ? 1 : 0, losses: elo < 1200 ? 1 : 0, verdicts: ['strong'], parents: [], duplicateOf: null, experiments: 0});
const researchBoard = (slug = 'sky', title = 'Why is the sky blue?') => ({type: 'researchBoard', slug, title, discipline: 'physics',
  brief: `# ${title}\n\n**Discipline:** Physics\n\n## Goal\n\nExplain the colour.\n`, plan: '# Research plan\n\n1. Rayleigh: $I \\propto \\lambda^{-4}$\n',
  overview: '# Overview\n\n_Nothing yet._\n', verdicts: '# Verdicts\n', sandbox: '# Sandbox log\n',
  hypotheses: [researchRow('H002', 1216, 'codex', 'Cones and the sun'), researchRow('H001', 1184, 'claude', 'Absorption', 'retired')]});
const researchSummary = (slug, title, hypotheses = 2) => ({slug, title, discipline: 'physics', created: 0, hypotheses, rounds: 1});
await test('Research makes an investigation from a brief, and keeps what was typed when that is refused', async () => {
  await win.dispatch({type: 'setMode', mode: 'research'}); await settle();
  await win.pushEvents([{type: 'researchList', investigations: [], open: null}]);
  const ws = $('.workspace[data-ws="research"]');
  assert(ws.querySelector('[data-research-list] .cc-none'), 'an empty project says nothing of it');
  assert(ws.querySelector('.rs-welcome [data-research-new]') && $('[data-research-tabs]').hidden, 'no welcome, or tabs with nothing to show');
  ws.querySelector('.cc-side [data-research-new]').click(); await settle();
  const form = ws.querySelector('[data-research-form]');
  assert(form && doc.activeElement === form.querySelector('[name="title"]'), 'the form did not open with its title focused');
  let before = win.commands.length;
  form.querySelector('[type="submit"]').click(); await settle();
  assert(!win.commands.slice(before).some((c) => c.type === 'researchNew'), 'an untitled investigation was asked for');
  const type = (name, value) => { form[name].value = value; form[name].dispatchEvent(new win.Event('input', {bubbles: true})); };
  type('title', 'Why is the sky blue?'); type('discipline', 'mathematics'); type('goal', 'Explain the colour.');
  form.goal.focus();
  form.querySelector('[type="submit"]').click(); await settle();
  const sent = win.commands.slice(before).find((c) => c.type === 'researchNew');
  assert(sent?.title === 'Why is the sky blue?' && sent.goal === 'Explain the colour.' && sent.discipline === 'mathematics', `wrong command: ${JSON.stringify(sent)}`);
  // Refused, or merely redrawn: the form stays as it was, caret and all.
  await win.pushEvents([{type: 'researchList', investigations: [], open: null}]);
  assert(ws.querySelector('[data-research-form]') === form && form.title.value === 'Why is the sky blue?', 'what was typed was lost');
  assert(doc.activeElement === form.goal, 'a redraw took the caret');
  // Escape puts it away without sending anything.
  before = win.commands.length;
  form.goal.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
  assert(!ws.querySelector('[data-research-form]') && ws.querySelector('.rs-welcome'), 'Escape did not put the form away');
  assert(!win.commands.slice(before).some((c) => c.type === 'key' || c.type === 'researchNew'), 'Escape reached the core');
  // ⌘⏎ sends it; when the list grows the form goes and the board is the new one's.
  ws.querySelector('.rs-welcome [data-research-new]').click(); await settle();
  const again = ws.querySelector('[data-research-form]');
  assert(again.title.value === 'Why is the sky blue?', 'the draft was not kept');
  before = win.commands.length;
  again.goal.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Enter', metaKey: true, bubbles: true, cancelable: true})); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'researchNew'), '⌘⏎ did not create');
  await win.pushEvents([{type: 'researchList', investigations: [researchSummary('sky', 'Why is the sky blue?', 0)], open: 'sky'}, {...researchBoard(), hypotheses: []},
    {type: 'researchHypothesis', slug: 'sky', detail: null}]);
  assert(!ws.querySelector('[data-research-form]'), 'the form stayed after it was made');
  assert($('[data-research-title]').textContent === 'Why is the sky blue?' && !$('[data-research-tabs]').hidden, 'the board is not the new one');
  assert(ws.querySelector('.rs-item.on')?.dataset.researchOpen === 'sky', 'the list does not mark it');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('Research ranks the hypotheses, opens one whole, and lets another investigation be chosen', async () => {
  await win.dispatch({type: 'setMode', mode: 'research'}); await settle();
  const ws = $('.workspace[data-ws="research"]');
  await win.pushEvents([{type: 'researchList', investigations: [researchSummary('sky', 'Why is the sky blue?'), researchSummary('cubes', 'Sums of three cubes', 0)], open: 'sky'},
    researchBoard(), {type: 'researchHypothesis', slug: 'sky', detail: null}]);
  assert(ws.querySelectorAll('[data-research-open]').length === 2, 'the list is not the investigations');
  const board = ws.querySelector('[data-research-board]');
  assert(![...board.querySelectorAll('h1')].some((h) => h.textContent === 'Why is the sky blue?'), 'the brief repeats the title over itself');
  assert(board.querySelector('.rs-file[title="research/sky/RESEARCH_PLAN.md"]'), 'the plan does not say which file it is');
  // Kept in a folder of their own, the files say so, and so does the list.
  await win.pushEvents([{type: 'researchList', investigations: [researchSummary('sky', 'Why is the sky blue?'), researchSummary('cubes', 'Sums of three cubes', 0)], open: 'sky',
    folder: '~/Github/Research/'}, researchBoard()]);
  assert(board.querySelector('.rs-file[title="~/Github/Research/sky/RESEARCH_PLAN.md"]'), 'the plan does not say where it is kept');
  assert(ws.querySelector('[data-research-open="sky"]')?.title.startsWith('~/Github/Research/sky/'), 'the list does not say where it is kept');
  await win.pushEvents([{type: 'researchList', investigations: [], open: null, folder: '~/Github/Research/'}]);
  assert(ws.querySelector('[data-research-list] .cc-none')?.textContent.includes('~/Github/Research/'), 'an empty folder of their own does not say which');
  await win.pushEvents([{type: 'researchList', investigations: [researchSummary('sky', 'Why is the sky blue?'), researchSummary('cubes', 'Sums of three cubes', 0)], open: 'sky'},
    researchBoard()]);
  assert(board.querySelector('.rs-file[title="research/sky/RESEARCH_PLAN.md"]'), 'a list that does not say where is not the project\'s research/');
  // Where they are kept is changed from under the list, and a refusal keeps what was typed.
  const where = ws.querySelector('[data-research-where]');
  assert(where.textContent.includes("Kept in each project's research/"), `the list does not say where they are kept: ${where.textContent}`);
  where.querySelector('[data-research-where-change]').click(); await settle();
  let field = where.querySelector('[data-research-where-form] input');
  assert(field && doc.activeElement === field && field.value === '', 'the folder form did not open on an empty field');
  field.value = '~/Github/Research'; field.dispatchEvent(new win.Event('input', {bubbles: true}));
  const asked = win.commands.length;
  where.querySelector('[type="submit"]').click(); await settle();
  assert(win.commands.slice(asked).find((c) => c.type === 'researchFolder')?.folder === '~/Github/Research', 'the folder was not asked for');
  await win.pushEvents([{type: 'notice', level: 'error', text: '~/Github: no such folder'}]);
  assert(where.querySelector('[data-research-where-form] input')?.value === '~/Github/Research', 'a refusal lost what was typed');
  await win.pushEvents([{type: 'researchList', investigations: [researchSummary('sky', 'Why is the sky blue?'), researchSummary('cubes', 'Sums of three cubes', 0)], open: 'sky',
    folder: '~/Github/Research/'}, researchBoard()]);
  assert(!where.querySelector('form') && where.textContent.includes('Kept in ~/Github/Research/'), `the folder line did not follow: ${where.textContent}`);
  where.querySelector('[data-research-where-change]').click(); await settle();
  field = where.querySelector('[data-research-where-form] input');
  assert(field?.value === '~/Github/Research', 'the form does not start from where they are');
  field.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true})); await settle();
  assert(!where.querySelector('form'), 'Escape did not put the folder form away');
  await win.pushEvents([{type: 'researchList', investigations: [researchSummary('sky', 'Why is the sky blue?'), researchSummary('cubes', 'Sums of three cubes', 0)], open: 'sky'},
    researchBoard()]);
  for (let i = 0; i < 30 && board.querySelector('.md-math[data-tex]'); i++) await settle();
  assert(board.querySelector('.md-math .katex'), 'the plan\'s TeX is not drawn');
  ws.querySelector('[data-research-tab="hypotheses"]').click(); await settle();
  const rows = [...board.querySelectorAll('.rs-row[data-research-hypothesis]')];
  assert(rows.map((r) => r.dataset.researchHypothesis).join() === 'H002,H001', 'the leaderboard is not in the core\'s order');
  assert(rows[1].classList.contains('retired') && rows[1].querySelector('.rs-rank').textContent === '–', 'a retired hypothesis is ranked');
  assert($('[data-research-count]').textContent === '2', 'the tab does not count them');
  let before = win.commands.length;
  rows[0].click(); await settle();
  assert(win.commands.slice(before).find((c) => c.type === 'researchShow')?.hypothesis === 'H002', 'the row did not ask for its hypothesis');
  await win.pushEvents([{type: 'researchHypothesis', slug: 'sky', detail: {row: researchBoard().hypotheses[0], statement: 'Scattering goes as $\\lambda^{-4}$.',
    rationale: 'Small scatterers.', predictions: ['A peak near 405 nm.'], assumptions: [], children: [], experiments: [], task: null,
    reviews: [{id: 'H002.1', reviewer: {seat: 'claude', model: 'claude-opus-5-5'}, kind: 'review', verdict: 'strong', correctness: 5, novelty: 2, testability: 5,
      round: 1, summary: 'Sound.', critique: 'Quantify it.', citations: ['Bohren (1985)']}],
    matches: [{round: 1, opponent: 'H001', opponentTitle: 'Absorption', judge: {seat: 'antigravity', model: ''}, won: true, rationale: 'Explains it.'}]}}]);
  assert(board.querySelector('.rs-h')?.textContent.includes('H002') && board.querySelectorAll('.rs-review').length === 1, 'the hypothesis is not drawn whole');
  assert(board.querySelector('.rs-match.won [data-research-hypothesis="H001"]'), 'the tournament does not link its opponent');
  // Another investigation's hypothesis is not this one's.
  await win.pushEvents([{type: 'researchHypothesis', slug: 'cubes', detail: null}]);
  assert(board.querySelector('.rs-detail'), 'a stray event closed the hypothesis');
  before = win.commands.length;
  board.querySelector('[data-research-back]').click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'researchShow' && c.hypothesis === null), 'back did not let it go');
  await win.pushEvents([{type: 'researchHypothesis', slug: 'sky', detail: null}]);
  assert(board.querySelector('.rs-table') && !board.querySelector('.rs-detail'), 'the leaderboard did not come back');
  before = win.commands.length;
  ws.querySelector('[data-research-open="cubes"]').click(); await settle();
  assert(win.commands.slice(before).find((c) => c.type === 'researchOpen')?.slug === 'cubes', 'the list did not open the investigation');
  // A project with none clears the board.
  await win.pushEvents([{type: 'researchList', investigations: [], open: null}]);
  assert(ws.querySelector('.rs-welcome') && $('[data-research-tabs]').hidden && $('[data-research-title]').textContent === 'Research', 'the last project\'s board stayed');
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
const researchSeats = (over = {}) => ['claude', 'codex', 'antigravity'].map((seat) => ({seat,
  role: {claude: 'investigator', codex: 'engineer', antigravity: 'sandbox'}[seat], enabled: true, why: null, cooling: null, running: 0, model: '', ...(over[seat] ?? {})}));
const researchRound = (status, extra = {}) => ({type: 'researchRound', slug: 'sky', round: 1, status, used: 0, cap: 20,
  seats: researchSeats(), tasks: [], held: null, ...extra});
const researchTask = (id, phase, seat, state, extra = {}) => ({id, round: 1, phase, seat, role: 'investigator', targets: [], state,
  why: null, model: '', attempts: 0, started: 100, ended: state === 'done' ? 112 : null, ...extra});
const openSky = async (round) => {
  await win.dispatch({type: 'setMode', mode: 'research'}); await settle();
  await win.pushEvents([{type: 'researchList', investigations: [researchSummary('sky', 'Why is the sky blue?')], open: 'sky'},
    researchBoard(), {type: 'researchHypothesis', slug: 'sky', detail: null}, round]);
  return $('.workspace[data-ws="research"]');
};
await test('Research runs a round from the board and follows it call by call', async () => {
  const ws = await openSky(researchRound({status: 'idle'}, {round: 0}));
  const go = ws.querySelector('[data-research-go]');
  assert(shown(go) && go.textContent === 'Run round' && $('[data-research-status]').textContent === 'No rounds yet', 'no way to start a round');
  assert(!shown(ws.querySelector('[data-research-meter]')) && !shown(ws.querySelector('[data-research-pause]')), 'an idle round shows its spending');
  let before = win.commands.length;
  go.click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'researchStart'), 'Run round did not ask for one');
  await win.pushEvents([researchRound({status: 'running'}, {used: 4, tasks: [researchTask('T0001', 'frame', 'claude', 'done', {model: 'claude-opus-5-5'}),
    researchTask('T0002', 'generate', 'claude', 'running'), researchTask('T0003', 'generate', 'codex', 'failed', {why: 'Not logged in'}),
    researchTask('T0004', 'reflect', 'antigravity', 'ready', {targets: ['H001', 'H002']})]})]);
  assert($('[data-research-status]').textContent === 'Round 1 · running', 'the round does not say it is running');
  assert(ws.querySelector('[data-research-meter]').dataset.label === '4/20', 'the meter does not say what is spent');
  assert(shown(ws.querySelector('[data-research-pause]')) && shown(ws.querySelector('[data-research-stop]')) && !shown(go), 'wrong controls for a running round');
  assert(ws.querySelector('[data-research-tab="activity"]').getAttribute('aria-selected') === 'true', 'a round that starts is not followed');
  assert($('[data-research-busy]').textContent === '1', 'the tab does not count the calls out');
  const rows = [...ws.querySelectorAll('[data-research-task]')];
  assert(rows.map((r) => r.dataset.researchTask).join() === 'T0001,T0002,T0003,T0004', 'the calls are not in the round\'s order');
  assert(rows[2].textContent.includes('Not logged in') && rows[2].dataset.state === 'failed', 'a failed call does not say why');
  assert(rows[3].textContent.includes('H001, H002'), 'a call does not say what it is about');
  assert(ws.querySelector('.rs-item.on .rs-live'), 'the list does not mark the running investigation');
  before = win.commands.length;
  ws.querySelector('[data-research-pause]').click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'researchPause'), 'Pause did not ask');
  await win.pushEvents([researchRound({status: 'paused', interrupted: true}, {used: 4})]);
  assert($('[data-research-status]').textContent.includes('paused when Proton closed') && go.textContent === 'Resume', 'a round paused by a relaunch does not say so');
  before = win.commands.length;
  go.click(); await settle();
  ws.querySelector('[data-research-stop]').click(); await settle();
  const sent = win.commands.slice(before).map((c) => c.type);
  assert(sent.includes('researchResume') && sent.includes('researchStop'), `wrong commands: ${sent}`);
  await win.pushEvents([researchRound({status: 'running'}, {held: 'Another Proton (process 42) is running this investigation\'s rounds.'})]);
  assert($('[data-research-status]').textContent === 'Read only' && ws.querySelector('[data-research-pause]').disabled, 'a held investigation can be driven');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('A call opens into what it said, and only that call\'s words are drawn there', async () => {
  const ws = await openSky(researchRound({status: 'running'}, {tasks: [researchTask('T0001', 'frame', 'claude', 'done'), researchTask('T0002', 'generate', 'claude', 'running')]}));
  ws.querySelector('[data-research-tab="activity"]').click(); await settle();
  let before = win.commands.length;
  ws.querySelector('[data-research-task="T0001"]').click(); await settle();
  assert(win.commands.slice(before).find((c) => c.type === 'researchTask')?.task === 'T0001', 'the call was not asked for');
  assert(ws.querySelector('.rs-transcript')?.textContent.includes('Reading'), 'no place for what it said');
  await win.pushEvents([{type: 'researchTranscript', slug: 'sky', task: 'T0002', items: [{kind: 'text', id: 'x', text: 'Wrong call.', done: true, parent: null}]}]);
  assert(!ws.querySelector('.rs-transcript').textContent.includes('Wrong call'), 'another call\'s words were drawn');
  await win.pushEvents([{type: 'researchTranscript', slug: 'sky', task: 'T0001', items: [
    {kind: 'user', id: 'u', text: 'Frame the investigation.', images: 0},
    {kind: 'tool', id: 't', name: 'WebSearch', input: {query: 'Rayleigh scattering'}, state: 'done', streamed: 0, result: null, detail: null, parent: null, ask: null, artifact: null},
    {kind: 'text', id: 'a', text: 'The plan is to integrate the spectrum.', done: true, parent: null},
    {kind: 'turn', id: 'e', ms: 12000, turns: 1, cost: 0, error: null}]}]);
  const box = ws.querySelector('.rs-transcript');
  assert(box.querySelector('.rs-tool')?.textContent.includes('Rayleigh scattering') && box.textContent.includes('integrate the spectrum') && box.textContent.includes('Answered in 12s'),
    `the transcript is not drawn: ${box.textContent}`);
  before = win.commands.length;
  ws.querySelector('[data-research-task="T0001"]').click(); await settle();
  assert(win.commands.slice(before).find((c) => c.type === 'researchTask')?.task === null && !ws.querySelector('.rs-transcript'), 'the transcript did not close');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('The scientist tells the team, and adds hypotheses of their own with /h', async () => {
  const ws = await openSky(researchRound({status: 'idle'}));
  const input = ws.querySelector('[data-research-input]');
  assert(shown(input), 'no composer');
  const type = (text) => { input.value = text; input.dispatchEvent(new win.Event('input', {bubbles: true})); };
  const enter = (shiftKey = false) => input.dispatchEvent(new win.KeyboardEvent('keydown', {key: 'Enter', shiftKey, bubbles: true, cancelable: true}));
  type('Focus on polarisation.');
  let before = win.commands.length;
  enter(true); await settle();
  assert(!win.commands.slice(before).some((c) => c.type === 'researchFeedback'), '⇧⏎ sent');
  enter(); await settle();
  assert(win.commands.slice(before).find((c) => c.type === 'researchFeedback')?.text === 'Focus on polarisation.' && input.value === '', 'it was not told to the team');
  type('/h Ozone: The Chappuis band deepens the zenith blue.');
  assert($('[data-research-hint]').textContent.includes('“Ozone”'), 'the composer does not say it will add a hypothesis');
  before = win.commands.length;
  ws.querySelector('[data-research-send]').click(); await settle();
  const own = win.commands.slice(before).find((c) => c.type === 'researchAddHypothesis');
  assert(own?.title === 'Ozone' && own.statement === 'The Chappuis band deepens the zenith blue.', `wrong hypothesis: ${JSON.stringify(own)}`);
  assert(ws.querySelector('[data-research-send]').disabled, 'an empty composer can send');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('The table says who cannot sit, and a hypothesis is pinned or retired from its page', async () => {
  const later = Math.floor(Date.now() / 1000) + 600;
  const seats = researchSeats({claude: {running: 1}, codex: {why: 'Codex is not installed.'}, antigravity: {cooling: later}});
  const ws = await openSky(researchRound({status: 'running'}, {seats}));
  let rows = [...ws.querySelectorAll('.rs-seat-row')];
  assert(rows.length === 3 && rows[0].textContent.includes('1 call out'), 'the table does not say who is working');
  assert(rows[1].dataset.kind === 'out' && rows[1].textContent.includes('Codex is not installed.'), 'the table does not say who cannot sit');
  assert(rows[2].dataset.kind === 'cooling' && rows[2].textContent.includes('At its limit until'), 'the table does not say who is at its limit');
  assert(rows.every((r) => r.querySelector('input').disabled), 'the table can change under a running round');
  await win.pushEvents([researchRound({status: 'idle'}, {seats})]);
  rows = [...ws.querySelectorAll('.rs-seat-row')];
  let before = win.commands.length;
  rows[2].querySelector('input').click(); await settle();
  const seat = win.commands.slice(before).find((c) => c.type === 'researchSeat');
  assert(seat?.seat === 'antigravity' && seat.enabled === false, `wrong seat command: ${JSON.stringify(seat)}`);
  const detail = (status) => ({type: 'researchHypothesis', slug: 'sky', detail: {row: {...researchBoard().hypotheses[0], status}, statement: 'Cones.', rationale: '',
    predictions: [], assumptions: [], children: [], experiments: [], task: null, reviews: [], matches: []}});
  await win.pushEvents([detail('active')]);
  before = win.commands.length;
  ws.querySelector('[data-research-status="retired"]').click(); await settle();
  const retire = win.commands.slice(before).find((c) => c.type === 'researchSetStatus');
  assert(retire?.hypothesis === 'H002' && retire.status === 'retired', `wrong status command: ${JSON.stringify(retire)}`);
  await win.pushEvents([detail('retired')]);
  assert(ws.querySelector('[data-research-status="active"]')?.textContent === 'Bring back' && !ws.querySelector('[data-research-status="pinned"]'), 'a retired hypothesis cannot be brought back');
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('The Lab\'s Python is planned, then made, each on its own say-so', async () => {
  await win.dispatch({type: 'setMode', mode: 'research'}); await settle();
  const box = $('[data-lab-env]');
  const text = () => box.textContent.replace(/\s+/g, ' ');
  const prefix = '/Users/you/.proton/lab/envs/lab';
  assert(!win.commands.some((c) => c.type.startsWith('lab')), 'the Lab was asked before it was told to');
  await win.pushEvents([{type: 'labEnv', env: {state: 'missing', maker: {kind: 'conda', conda: '/Users/you/miniconda3/bin/conda'}, index: null, specs: ['python=3.12', 'numpy', 'scipy'], prefix}}]);
  assert(text().includes('Python 3.12 with numpy and scipy') && text().includes('~/.proton/lab/envs/lab'), `the Lab does not say what it would be: ${text()}`);
  assert(text().includes('conda has no copy of it yet') && text().includes('Nothing is installed yet'), 'the first say-so does not say what it costs');
  let before = win.commands.length;
  box.querySelector('[data-lab="plan"]').click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'labPlan'), 'planning was not asked for');
  await win.pushEvents([{type: 'labEnv', env: {state: 'planning', log: ['Collecting package metadata (repodata.json): done']}}]);
  assert(box.querySelector('.lab-log')?.textContent.includes('Collecting package metadata'), 'what conda says is not shown');
  await win.pushEvents([{type: 'labEnv', env: {state: 'planned', plan: {packages: [
    {name: 'numpy', version: '2.1.3', build: 'h0', size: 5301200}, {name: 'python', version: '3.12.7', build: 'h1', size: 13412345},
    {name: 'tzdata', version: '2024b', build: 'h2', size: null}], download: 18713545, cached: 1, prefix}}}]);
  assert(text().includes('17.8 MB') && text().includes('1 already in conda') && box.querySelectorAll('.lab-packages tr').length === 3,
    `the second say-so does not show every package and the download: ${text()}`);
  before = win.commands.length;
  box.querySelector('[data-lab="create"]').click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'labCreate'), 'making it was not asked for');
  await win.pushEvents([{type: 'labEnv', env: {state: 'creating', fetched: 1, total: 2, log: []}}]);
  assert(box.querySelector('.lab-bar > i').style.width === '50%' && text().includes('1 of 2 downloaded'), 'making it shows no progress');
  await win.pushEvents([{type: 'labEnv', env: {state: 'ready', python: '3.12.7', prefix, packages: [{name: 'numpy', version: '2.1.3', build: '', size: null}]}}]);
  assert(text().includes('Python 3.12.7') && text().includes('numpy 2.1.3'), 'a ready Lab does not say what it has');
  before = win.commands.length;
  box.querySelector('[data-lab="confirm"]').click(); await settle();
  box.querySelector('[data-lab="keep"]').click(); await settle();
  box.querySelector('[data-lab="confirm"]').click(); await settle();
  assert(box.querySelector('.lab-confirm')?.textContent.includes('Runs keep what they wrote'), 'removing does not ask first');
  box.querySelector('[data-lab="remove"]').click(); await settle();
  const sent = win.commands.slice(before).map((c) => c.type).filter((t) => t.startsWith('lab'));
  assert(sent.join() === 'labRemove', `wrong commands: ${sent}`);
  await win.pushEvents([{type: 'labEnv', env: {state: 'failed', step: 'plan', why: 'PackagesNotFoundError: nosuchpackage', log: ['x']}}]);
  assert(text().includes('Planning failed: PackagesNotFoundError'), 'a failure does not say what failed');
  before = win.commands.length;
  box.querySelector('[data-lab="cancel"]').click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'labCancel'), 'Start over did not ask');
  await win.pushEvents([{type: 'labEnv', env: {state: 'noPython'}}]);
  assert(text().includes('There is neither here') && !box.querySelector('button'), 'with nothing to make it, the Lab offers what it cannot do');
  // No conda, but a Python: a virtual environment, from PyPI.
  await win.pushEvents([{type: 'labEnv', env: {state: 'missing', maker: {kind: 'pip', python: '/opt/homebrew/bin/python3', version: '3.13.1'}, index: null,
    specs: ['python=3.13.1', 'numpy', 'scipy'], prefix}}]);
  assert(text().includes('Python 3.13.1 with numpy and scipy') && text().includes('empty virtual environment') && text().includes('PyPI'), `the pip plan is not said: ${text()}`);
  await win.pushEvents([{type: 'labEnv', env: {state: 'planned', plan: {packages: [{name: 'numpy', version: '2.1.3', build: '', size: 5301200},
    {name: 'scipy', version: '1.14.1', build: '', size: null}], download: 5301200, cached: 0, prefix}}}]);
  assert(text().includes('2 packages from PyPI') && text().includes('1 whose size PyPI did not say') && !text().includes('cached'), `the pip plan is not drawn: ${text()}`);
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('An experiment is read, then run in the sandbox or turned down, and its run opens whole', async () => {
  await openSky(researchRound({status: 'idle'}));
  const work = $('[data-lab-work]');
  const text = () => work.textContent.replace(/\s+/g, ' ');
  const code = Array.from({length: 20}, (_, i) => `x${i} = ${i}`).join('\n');
  const proposal = {id: 'E002', hypothesis: 'H004', proposer: {seat: 'codex', model: 'gpt-6.1-sol'}, round: 1, status: 'proposed', run: null,
    purpose: 'Find the dominant wavelength.', expectation: 'Between 470 and 480 nm.', code};
  const lab = (extra = {}) => ({type: 'researchLab', slug: 'sky', experiments: [proposal], runs: [], sandbox: 'seatbelt', ...extra});
  await win.pushEvents([{type: 'labEnv', env: {state: 'missing', maker: {kind: 'conda', conda: '/c'}, index: null, specs: ['python=3.12'], prefix: '/p'}}, lab()]);
  assert(text().includes('Waiting for you') && text().includes('E002 for H004') && text().includes('Find the dominant wavelength'), `the proposal is not shown: ${text()}`);
  assert(work.querySelector('.lab-code code[data-lang="python"] .num'), 'the code is not coloured');
  assert(work.querySelector('[data-lab-open="E002"]') && !text().includes('x19 = 19'), 'a long proposal is shown whole before it is asked for');
  work.querySelector('[data-lab-open="E002"]').click(); await settle();
  assert(text().includes('x19 = 19'), 'the rest of the code did not open');
  assert(work.querySelector('[data-lab-approve="E002"]').disabled, 'it can be run with no Python to run it in');
  await win.pushEvents([{type: 'labEnv', env: {state: 'ready', python: '3.12.7', prefix: '/p', packages: []}}]);
  const run = work.querySelector('[data-lab-approve="E002"]');
  assert(!run.disabled && run.textContent === 'Run in the sandbox', 'a sandboxed run is not offered');
  let before = win.commands.length;
  run.click(); await settle();
  const approved = win.commands.slice(before).find((c) => c.type === 'researchApprove');
  assert(approved?.experiment === 'E002' && approved.unsandboxed === false, `wrong approval: ${JSON.stringify(approved)}`);
  before = win.commands.length;
  work.querySelector('[data-lab-reject="E002"]').click(); await settle();
  assert(win.commands.slice(before).find((c) => c.type === 'researchReject')?.experiment === 'E002', 'Turn down did not ask');
  // With no sandbox, running it says what that means.
  await win.pushEvents([lab({sandbox: 'none'})]);
  const bare = work.querySelector('[data-lab-approve="E002"]');
  assert(bare.dataset.unsandboxed === '1' && bare.textContent === 'Run without a sandbox' && text().includes('can do anything you can'), 'an unsandboxed run is not said to be one');
  // A round's proposals: one card a hypothesis, in the core's order, the
  // earlier ones folded under it and the hypotheses past the fifth folded too.
  const of = (id, hypothesis) => ({...proposal, id, hypothesis, code: 'x = 1'});
  const many = [of('E009', 'H004'), of('E002', 'H004'), of('E008', 'H001'), of('E007', 'H002'), of('E006', 'H003'), of('E005', 'H005'),
    of('E004', 'H006'), of('E003', 'H007')];
  await win.pushEvents([lab({experiments: many})]);
  const cards = () => [...work.querySelectorAll(':scope > .lab-proposal')].map((c) => c.dataset.experiment);
  assert(cards().join() === 'E009,E008,E007,E006,E005', `not one card a hypothesis, the first five open: ${cards().join()}`);
  assert(text().includes('Waiting for you 8 for 7 hypotheses'), `the count does not say how many hypotheses: ${text()}`);
  const earlier = work.querySelector('[data-lab-fold="earlier:H004"]');
  assert(earlier && !earlier.open && earlier.querySelector('[data-experiment="E002"]') && earlier.textContent.includes('1 earlier proposal for H004'),
    'the earlier proposal is not folded under its hypothesis');
  const more = work.querySelector('[data-lab-fold="more"]');
  assert(more && !more.open && more.textContent.includes('2 more, for hypotheses ranked lower') && more.querySelector('[data-experiment="E003"]'),
    'the hypotheses ranked lower are not folded');
  more.open = true; await settle();
  await win.pushEvents([lab({experiments: many})]);
  assert(work.querySelector('[data-lab-fold="more"]')?.open, 'an opened fold closed when it was drawn again');
  // Runs, and one whole.
  const row = {id: 'R001', experiment: 'E001', hypothesis: 'H001', state: 'done', exit: 0, seconds: 2.4, started: 0, sandbox: 'seatbelt'};
  await win.pushEvents([lab({experiments: [], runs: [row, {...row, id: 'R002', state: 'failed', exit: 1}]})]);
  assert(text().includes('Runs') && text().includes('Failed · exit 1'), 'a failed run does not say how');
  before = win.commands.length;
  work.querySelector('[data-lab-run="R001"]').click(); await settle();
  assert(win.commands.slice(before).find((c) => c.type === 'researchShowRun')?.run === 'R001', 'the run was not asked for');
  await win.pushEvents([{type: 'researchRun', slug: 'sky', detail: {row, stdout: 'peak at 405.2 nm', stdoutCut: false, stderr: '', stderrCut: false,
    metrics: {peak_nm: {value: 405.2, unit: 'nm'}}, figures: ['figures/spectrum.png'], scenes: [],
    record: {code: {sha256: 'abcdef0123456789'}, python: '/Users/you/.proton/lab/envs/lab/bin/python', about: {approved: {by: 'scientist', at: 0}}, limits: {seconds: 600}}}}]);
  assert(text().includes('Done after 2.4 s, in Seatbelt') && text().includes('peak_nm') && text().includes('figures/spectrum.png') && text().includes('peak at 405.2 nm'),
    `the run is not drawn whole: ${text()}`);
  assert(text().includes('abcdef012345') && text().includes('~/.proton/lab/envs/lab/bin/python') && text().includes('by you'), 'the run does not say where it came from');
  work.querySelector('.lab-detail [data-lab-run=""]').click(); await settle();
  assert(work.querySelector('.lab-runs'), 'back did not go back to the runs');
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('A run\'s pictures are drawn, and its 3D scene opens from it', async () => {
  await openSky(researchRound({status: 'idle'}));
  const work = $('[data-lab-work]');
  // One pixel of PNG, and two frames of a scene.
  const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='), (c) => c.charCodeAt(0));
  const scene = {format: 'proton.scene/1', frames: [[{kind: 'sphere', center: [0, 0, 0], radius: 1}], [{kind: 'points', xyz: [[1, 0, 0], [0, 1, 0]]}]]};
  win.labFiles = {'sky/R001/figures/spectrum.png': png.buffer, 'sky/R001/orbit.scene.json': new TextEncoder().encode(JSON.stringify(scene)).buffer,
    'sky/R001/broken.scene.json': new TextEncoder().encode('{"format": "something else"}').buffer};
  const row = {id: 'R001', experiment: 'E001', hypothesis: 'H001', state: 'done', exit: 0, seconds: 2.4, started: 0, sandbox: 'seatbelt'};
  await win.pushEvents([{type: 'researchLab', slug: 'sky', experiments: [], runs: [row], sandbox: 'seatbelt'},
    {type: 'researchRun', slug: 'sky', detail: {row, stdout: '', stdoutCut: false, stderr: '', stderrCut: false, metrics: {},
      figures: ['figures/spectrum.png'], scenes: ['broken.scene.json', 'orbit.scene.json'], record: {}}}]);
  for (let i = 0; i < 20 && !work.querySelector('img[data-lab-figure]')?.src; i++) await settle();
  const img = work.querySelector('img[data-lab-figure]');
  // As a data: URL: the app's img-src allows those and refuses blob: ones.
  assert(img?.src.startsWith('data:image/png;base64,'), 'the picture was not read from the run, or not as the app\'s policy lets it load');
  work.querySelector('[data-lab-scene-open="broken.scene.json"]').click();
  for (let i = 0; i < 20 && !work.querySelector('.lab-scene .lab-err'); i++) await settle();
  assert(work.querySelector('.lab-scene .lab-err')?.textContent.includes('not a scene the Lab can draw'), 'a file that is not a scene was drawn');
  work.querySelector('[data-lab-scene-close]').click(); await settle();
  work.querySelector('[data-lab-scene-open="orbit.scene.json"]').click();
  for (let i = 0; i < 20 && !(work.querySelector('.lab-frames:not([hidden])') || work.querySelector('.lab-scene .lab-err')); i++) await settle();
  const drawn = work.querySelector('.lab-scene canvas.lab-canvas');
  if (drawn) {
    assert(work.querySelector('[data-lab-frame-n]').textContent === '1 / 2', 'a scene that moves has no frames to step through');
    const slider = work.querySelector('[data-lab-frame]');
    slider.value = '1'; slider.dispatchEvent(new win.Event('input', {bubbles: true}));
    assert(work.querySelector('[data-lab-frame-n]').textContent === '2 / 2', 'the slider did not step');
  } else {
    assert(work.querySelector('.lab-scene .lab-err')?.textContent.includes('no WebGL'), 'without WebGL the scene does not say why');
  }
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
  work.querySelector('[data-lab-scene-close]').click(); await settle();
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('Proposals run unasked only where a sandbox can hold them, and only when allowed', async () => {
  await openSky(researchRound({status: 'idle'}));
  const work = $('[data-lab-work]');
  // Back from whichever run was open.
  await win.pushEvents([{type: 'researchRun', slug: 'sky', detail: null},
    {type: 'researchLab', slug: 'sky', experiments: [], runs: [], sandbox: 'seatbelt', autoRun: false}]);
  const box = work.querySelector('[data-lab-auto]');
  assert(box && !box.checked, 'the switch is missing, or on by itself');
  const before = win.commands.length;
  box.click(); await settle();
  const sent = win.commands.slice(before).find((c) => c.type === 'researchAutoRun');
  assert(sent?.enabled === true, `wrong command: ${JSON.stringify(sent)}`);
  await win.pushEvents([{type: 'researchLab', slug: 'sky', experiments: [], runs: [], sandbox: 'none', autoRun: false}]);
  assert(!work.querySelector('[data-lab-auto]'), 'a machine with no sandbox offers to run code unasked');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('A long name in the project panel scrolls sideways instead of being cut', async () => {
  const tree = $('.workspace.on [data-tree]');
  const space = tree.closest('.workspace');
  const was = space.style.getPropertyValue('--col-l');
  space.style.setProperty('--col-l', '90px'); await settle();
  const rows = [...tree.querySelectorAll('.row')];
  const name = (row) => row.querySelector('span:last-child');
  assert(tree.scrollWidth > tree.clientWidth, 'nothing to scroll');
  assert(rows.every(row => name(row).scrollWidth <= name(row).clientWidth + 1), 'a name is clipped');
  assert(new Set(rows.map(row => Math.round(row.getBoundingClientRect().width))).size === 1, 'rows differ in width');
  assert(rows[0].getBoundingClientRect().width >= tree.scrollWidth - 1, 'a highlight would stop short');
  if (was) space.style.setProperty('--col-l', was); else space.style.removeProperty('--col-l');
});
await orderingChecks(test, assert);
document.querySelector('#status').textContent = `${passed} passed · ${failed} failed`;
document.title = `Proton UI tests: ${passed} passed, ${failed} failed`;
restoreLayouts();
