import {orderingChecks} from './ordering.js';
const frame = document.querySelector('iframe');
// Keep the test repeatable without changing the local browser preview's layout.
const layoutKeys = ['chat', 'codex', 'design', 'modeling', 'game', 'video', 'data'].map((mode) => `proton.panes.${mode}`);
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
  for (const mode of ['chat', 'design', 'modeling', 'game', 'video', 'data', 'codex']) {
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
await test('The desktop app\'s sessions from other folders are listed apart, by folder, and one open elsewhere says so', async () => {
  const ws = $('.workspace[data-ws="chat"]');
  await ccPush({type: 'claudeSessions', active: 's1', busy: [], waiting: [], engine: 'claude', sessions: [
    {id: 's1', title: 'Fix the build', updated: Date.now(), folder: '', elsewhere: false, engine: 'claude'},
    {id: 'x1', title: 'Make the poll configurable', updated: Date.now() - 1000, folder: '', elsewhere: false, engine: 'codex'},
    {id: 'd1', title: 'Batting averages', updated: Date.now() - 5000, folder: '/Users/you/Github/Cricket-Analysis', elsewhere: true, engine: 'claude'},
    {id: 'd2', title: 'Quantisation and SNR', updated: Date.now() - 9000, folder: '/Users/you/Research/TinyML', elsewhere: false, engine: 'claude'}]});
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
