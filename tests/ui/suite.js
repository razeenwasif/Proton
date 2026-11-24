import {orderingChecks} from './ordering.js';
import {sheetChecks} from './suite-sheet.js';
import {circuitChecks} from './suite-circuit.js';
import {logicChecks} from './suite-logic.js';
import {cadChecks} from './suite-cad.js';
const frame = document.querySelector('iframe');
// Keep the test repeatable without changing the local browser preview's layout.
const layoutKeys = [...['codex', 'data', 'sheet', 'circuit', 'logic', 'cad'].map((mode) => `proton.panes.${mode}`)];
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
/** The next frame: what streams — an answer, a shell — is painted at once
 *  and then at most once a frame, with whatever came last. */
const nextFrame = () => new Promise((done) => win.requestAnimationFrame(() => done(undefined)));
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
  for (const mode of ['data', 'sheet', 'codex']) {
    await win.dispatch({type: 'setMode', mode}); await settle();
    const section = $('.workspace.on'), stage = section.querySelector('main');
    assert(stage.clientWidth >= 320, `${mode} lost its stage`);
    assert(stage.getBoundingClientRect().right <= section.getBoundingClientRect().right + 1, `${mode} overflows`);
    const toggle = [...doc.querySelectorAll('[data-panel-toggle="right"]')].find((b) => b.getClientRects().length);
    assert(toggle && !toggle.disabled, `${mode} has the wrong panel controls`);
  }
});
const shown = (el) => Boolean(el) && win.getComputedStyle(el).display !== 'none' && el.getClientRects().length > 0;
await test('Codex trades the rails and statusline for a title bar, and Data does too', async () => {
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
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  assert(shown($('.titlebar')), 'title bar went with Data');
});
await test('The rail names only keys that do something, and its plugin button opens the plugin commands', async () => {
  const titles = [...doc.querySelectorAll('.rail button[title]')].map((b) => b.title).join(' | ');
  for (const dead of ['␣aa', '␣cv', '␣dd', '␣px']) assert(!titles.includes(dead), `the rail still names ${dead}: ${titles}`);
  assert(titles.includes('␣ct') && titles.includes('Plugin commands  ␣pc'), `the rail's keys: ${titles}`);
  const before = win.commands.length;
  $('.rail [data-picker="pluginCommands"]').click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'openPicker' && c.kind === 'pluginCommands'), 'the plugin button did not open the plugin commands');
  $('#scrim').click(); await settle();
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
await test('The wheel looks back through the terminal over its screen, and scrolls everything else as the browser does', async () => {
  const screen = $('[data-term-screen]');
  const before = win.commands.length;
  const up = new win.WheelEvent('wheel', {deltaY: -120, bubbles: true, cancelable: true});
  screen.dispatchEvent(up); await settle();
  assert(up.defaultPrevented && win.commands.slice(before).some((c) => c.type === 'terminalScroll' && c.lines === 3), 'the wheel over the screen did not look back');
  // Anywhere else the wheel is the browser's, never held for this thread.
  const elsewhere = new win.WheelEvent('wheel', {deltaY: 120, bubbles: true, cancelable: true});
  $('.workspace.on')?.dispatchEvent(elsewhere); await settle();
  assert(!elsewhere.defaultPrevented && !win.commands.slice(before).some((c) => c.type === 'terminalScroll' && c.lines === -3), 'the wheel was taken outside the terminal');
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
await test('⌘V in the editor pastes the clipboard: read by the shell on the Mac, else through a hidden field, to the core; ⌃V stays vim\'s', async () => {
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  $('.workspace.on [data-editor]').focus();
  const field = $('[data-editor-paste]');
  const mac = /Mac/.test(win.navigator.platform);
  const chord = mac ? {metaKey: true} : {ctrlKey: true, shiftKey: true};
  let before = win.commands.length;
  if (mac) {
    // The shell reads the clipboard: nothing waits on a page's paste.
    win.clipboardText = '● a point\n';
    key('v', chord);
    await settle();
    assert(doc.activeElement !== field, 'the shell\'s paste moved the focus');
    win.clipboardText = null;
    key('v', chord);
    await settle();
  } else {
    key('v', chord);
    assert(doc.activeElement === field, '⌘V did not move the focus to where a paste can land');
    const data = new win.DataTransfer();
    data.setData('text/plain', '● a point\n');
    const paste = new win.ClipboardEvent('paste', {clipboardData: data, bubbles: true, cancelable: true});
    field.dispatchEvent(paste);
    await settle();
    assert(paste.defaultPrevented && field.value === '' && doc.activeElement !== field, 'the hidden field kept the paste or the keyboard');
  }
  const sent = win.commands.slice(before).filter((c) => c.type === 'editorPaste');
  assert(sent.length === 1 && sent[0].text === '● a point\n', `the pastes that reached the editor: ${JSON.stringify(sent)}`);
  // ⌃V without ⇧ is vim's block selection, not a paste.
  $('.workspace.on [data-editor]').focus();
  before = win.commands.length;
  key('v', {ctrlKey: true});
  await settle();
  assert(doc.activeElement !== field && win.commands.slice(before).some((c) => c.type === 'key' && c.key.key === 'v' && c.key.ctrl), '⌃V did not go to the editor as a key');
  key('Escape'); await settle();
  // A yank or a delete the core reports goes on the system clipboard.
  win.clipboardText = null;
  await win.pushEvents([{type: 'clipboard', text: 'yanked line\n'}]); await settle();
  assert(win.clipboardText === 'yanked line\n', `a yank did not reach the clipboard: ${win.clipboardText}`);
  win.clipboardText = null;
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
await test('A shell\'s screen arrives as the rows that changed, and a burst of them is drawn once a frame', async () => {
  const pane = $('[data-term-pane]'), screen = $('[data-term-screen]'), toggle = $('.titlebar [data-term-toggle]');
  const wasOpen = !pane.hidden;
  if (!wasOpen) { toggle.click(); await settle(); }
  const rows = (texts) => ({type: 'terminal', running: true, scrollback: 0, cursor: [0, 0], cursorVisible: false,
    rows: texts.map((text) => text === null ? null : [{text}])});
  await settle();
  await win.pushEvents([rows(['one', 'two', 'three'])]);
  const lines = () => [...screen.querySelectorAll('.trow')].map((r) => r.textContent);
  assert(lines().join('|') === 'one|two|three', `the screen was not drawn: ${lines()}`);
  const first = screen.children[0];
  await settle();
  await win.pushEvents([rows([null, 'TWO', null])]);
  assert(lines().join('|') === 'one|TWO|three', `the rows left out were not kept: ${lines()}`);
  assert(screen.children[0] === first, 'a row that did not change was drawn again');
  // Ten screens in one breath: the first drawn at once, the last at the next frame.
  await settle();
  for (let i = 0; i < 10; i++) win.pushEvents([rows([null, `burst ${i}`, null])]);
  assert(lines()[1] === 'burst 0', `the first of a burst was not drawn at once: ${lines()}`);
  await nextFrame();
  assert(lines().join('|') === 'one|burst 9|three', `the last of a burst was not drawn: ${lines()}`);
  await settle();
  if (!wasOpen) { toggle.click(); await settle(); }
});
await test('The text terminal colours all 256 of xterm\'s colours, not only the theme\'s sixteen', async () => {
  const screen = $('[data-term-screen]');
  await win.pushEvents([{type: 'terminal', running: true, scrollback: 0, cursor: [0, 0], cursorVisible: false,
    rows: [[{text: 'cube', fg: 196, bg: 21}, {text: 'grey', fg: 244}]]}]);
  await nextFrame();
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
await test('A click whose pointer twitches inside the character pressed on is not a drag; one that reaches another is', async () => {
  const target = $('.workspace.on [data-editor] .tx');
  const rect = target.getBoundingClientRect();
  const at = {clientX: rect.left + 3, clientY: rect.top + rect.height / 2};
  const before = win.commands.length;
  target.dispatchEvent(new win.MouseEvent('mousedown', {bubbles: true, cancelable: true, button: 0, ...at}));
  doc.dispatchEvent(new win.MouseEvent('mousemove', {bubbles: true, clientX: at.clientX + 1, clientY: at.clientY}));
  await new Promise((done) => win.requestAnimationFrame(() => win.requestAnimationFrame(done)));
  assert(!win.commands.slice(before).some((c) => c.type === 'selectTo'), 'a twitch inside the character became a drag');
  doc.dispatchEvent(new win.MouseEvent('mousemove', {bubbles: true, clientX: at.clientX + 60, clientY: at.clientY}));
  await new Promise((done) => win.requestAnimationFrame(() => win.requestAnimationFrame(done)));
  doc.dispatchEvent(new win.MouseEvent('mouseup', {bubbles: true}));
  assert(win.commands.slice(before).some((c) => c.type === 'selectTo'), 'a drag to another character did not select');
  await win.dispatch({type: 'key', key: {key: 'Escape', ctrl: false, alt: false, shift: false, meta: false}}); await settle();
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
await test('A trackpad scroll is never pulled back by the core answering late, nor queues behind itself', async () => {
  win.installProjectFixtures(); await settle();
  const editor = $('.workspace.on [data-editor]');
  const line = editor.querySelector('.ln').getBoundingClientRect().height;
  const frame = () => new Promise((done) => win.requestAnimationFrame(done));
  const before = win.commands.length;
  // Three lines a frame, with every answer two frames late: each answer is
  // for a place the pane has already passed, and must not be followed.
  win.replyDelay = 30;
  try {
    let want = editor.scrollTop;
    for (let i = 0; i < 30; i++) {
      assert(Math.abs(editor.scrollTop - want) <= 1, `frame ${i}: pulled back from ${want} to ${editor.scrollTop}`);
      want = editor.scrollTop + 3 * line;
      editor.scrollTop = want;
      await frame();
    }
    const reports = win.commands.slice(before).filter((c) => c.type === 'viewport').length;
    assert(reports < 20, `${reports} reports for 30 frames: they queued behind each other`);
    await settle(); await settle();
    assert(editor.scrollTop === want, 'the pane did not stay where it was scrolled');
    const top = Math.floor(editor.scrollTop / line);
    assert([...editor.querySelectorAll('.ln')][0]?.textContent.endsWith(`Alpha row ${top}`), 'the lines where it stopped were not drawn');
  } finally { win.replyDelay = 0; }
  // A line further, and the rows still on screen are kept: one is built.
  editor.querySelectorAll('.ln').forEach((row) => { row.dataset.kept = '1'; });
  editor.scrollTop += line; await settle();
  const built = [...editor.querySelectorAll('.ln')].filter((row) => !row.dataset.kept).length;
  assert(built === 1, `a one-line scroll built ${built} rows`);
  // The core moving the window itself — G, a search hit — is still followed.
  win.projectFixture.first[0] = 50000;
  await win.pushEvents([{type: 'viewport', view: 0, firstLine: 50000, totalLines: 100000, version: 0,
    lines: Array.from({length: 30}, (_, i) => `Alpha row ${50000 + i}`), highlights: [], matches: [], git: [], stops: []}]);
  await settle();
  assert(Math.round(editor.scrollTop / line) === 50000, 'the pane did not follow the core to line 50000');
});
const sentSince = (before, type) => win.commands.slice(before).filter((c) => c.type === type);
await test('No browser runtime errors', () => {
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
});
await test('Every mode asks for an opaque window in the theme\'s ground, and never gives it back', async () => {
  const asked = () => (win.invokes ?? []).filter((c) => c.name === 'set_opaque').map((c) => c.args.color);
  const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
  const before = asked().length;
  for (const mode of ['data', 'sheet', 'circuit', 'codex']) {
    await win.pushEvents([{type: 'modeChanged', mode}]);
  }
  const now = asked().slice(before);
  assert(!now.includes(null), `a mode gave the opaque window back: ${JSON.stringify(now)}`);
  assert(asked().at(-1) === ground, `asked for ${asked().at(-1)}, the theme's ground is ${ground}`);
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
/** Codex's dock, showing the agent: only a thread on screen is painted as
 *  an answer streams — one off screen is painted when it comes into view. */
const showAgentDock = async () => {
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  $('#codex-dock [data-dockpanel="agent"]')?.click(); await settle();
};
await test('Antigravity is a provider, with its own models and its own tools named', async () => {
  $('.rail [data-open="settings"]').click(); await settle();
  $('[data-st-nav="models"]').click();
  const before = win.commands.length;
  $('[data-set-choice="agent.provider"] [data-value="antigravity"]').click(); await settle();
  const writes = win.commands.slice(before).filter(c => c.type === 'setSetting');
  assert(writes.some(c => c.key === 'agent.provider' && c.value === 'antigravity'), 'Antigravity was not selected');
  // One switch for every provider, on unless turned off.
  assert($('[data-set-bool="agent.writeFiles"]')?.checked, 'writing is not on by default');
  assert(!$('[data-set-bool="agent.antigravityEdits"]'), 'the old Antigravity-only switch is still there');
  key('Escape'); await settle();
  await showAgentDock();
  await win.pushEvents([{type: 'agent', provider: 'antigravity', model: 'gemini-4-argon-high',
    credential: 'Uses your Antigravity sign-in', activity: 'idle', inputTokens: 0, outputTokens: 0, cachedTokens: 0,
    transcript: [{speaker: 'you', text: 'what does main do?', thinking: '', streaming: false, calls: []},
      {speaker: 'proton', text: 'It parses arguments.', thinking: '', streaming: false,
        calls: [{id: 'agy-2', name: 'view_file', detail: 'AbsolutePath=/fixture/main.c', result: '', state: 'done'}]}]}]);
  assert($('[data-agent-provider]').textContent === 'Antigravity', 'provider is mislabeled');
  const models = [...$('[data-model-pick]').options].map(o => o.value);
  assert(models[0] === 'gemini-4-argon-high' && models.includes('gemini-3.8-flash-high') && $('[data-model-pick]').value === 'gemini-4-argon-high', `wrong models: ${models}`);
  assert([...doc.querySelectorAll('#codex-dock .tool b')].some(b => b.textContent === 'Read file'), 'its tool row is unnamed');
});
await test('The file open in Codex is named in the composer as going with each question, and the chip switches that off and on', async () => {
  await showAgentDock();
  await win.pushEvents([{type: 'buffers', names: [{name: 'queue.h', path: 'src/queue.h', modified: false}], active: 0}]);
  const chip = $('#codex-dock [data-open-file-chip]');
  assert(chip && !chip.hidden && chip.textContent === 'queue.h' && !chip.classList.contains('off'), `chip: ${chip?.outerHTML}`);
  let before = win.commands.length;
  chip.click(); await settle();
  const off = win.commands.slice(before).find((c) => c.type === 'setSetting' && c.key === 'agent.openFile');
  assert(off?.value === false, JSON.stringify(off));
  await win.dispatch({type: 'setSetting', key: 'agent.openFile', value: false}); await settle();
  assert(chip.classList.contains('off') && chip.title.includes('not sent'), chip.title);
  before = win.commands.length;
  chip.click(); await settle();
  assert(win.commands.slice(before).find((c) => c.type === 'setSetting' && c.key === 'agent.openFile')?.value === true, 'not switched back on');
  await win.dispatch({type: 'setSetting', key: 'agent.openFile', value: true}); await settle();
  // An unnamed buffer goes with nothing, and says nothing.
  await win.pushEvents([{type: 'buffers', names: [{name: '[No Name]', path: null, modified: false}], active: 0}]);
  assert(chip.hidden, 'a buffer with no file is named as going with the question');
});
await test('A question shows what was typed with what went along as chips, and a file an answer links is named and opens', async () => {
  await showAgentDock();
  await win.pushEvents([{type: 'agent', provider: 'claude', model: 'claude-opus-5-5', credential: '', activity: 'idle', inputTokens: 0, outputTokens: 0, cachedTokens: 0,
    transcript: [{speaker: 'you', text: 'see if you can read it now', thinking: '', streaming: false, calls: [], attached: ['TinyML/Review.tex']},
      {speaker: 'proton', text: 'I have read [Review.tex](file:///Users/r/Github/Research/TinyML/Review.tex#L12) in full.', thinking: '', streaming: false, calls: [], attached: []}]}]);
  await nextFrame();
  const thread = $('#codex-dock [data-agent-thread]');
  const you = thread.querySelector('.msg.you');
  assert(you.textContent.includes('see if you can read it now') && you.querySelector('.attached-chip')?.textContent === '📄 Review.tex', you.innerHTML);
  const link = thread.querySelector('.msg:not(.you) .file-link');
  assert(link?.textContent === 'Review.tex' && !thread.textContent.includes('file:///'), thread.innerHTML);
  const before = win.commands.length;
  link.click(); await settle();
  const sentNow = win.commands.slice(before);
  assert(sentNow.some((c) => c.type === 'openBuffer' && c.path === '/Users/r/Github/Research/TinyML/Review.tex'), JSON.stringify(sentNow));
  assert(sentNow.some((c) => c.type === 'placeCursor' && c.line === 11), JSON.stringify(sentNow));
});
await test('Many tool calls fold into one line that counts them, with a command to answer kept out of the fold', async () => {
  await showAgentDock();
  const read = (n, failed) => ({id: `R${n}`, name: 'view_file', detail: `Path=/Users/r/f${n}.tex`, result: failed ? 'file does not exist' : '', state: failed ? 'failed' : 'done', undo: false, asks: false});
  const calls = [...Array.from({length: 10}, (_, n) => read(n, n !== 5)),
    {id: 'C9', name: 'run_command', detail: 'make', result: 'waiting for you to run it or not', state: 'running', undo: false, asks: true}];
  const push = (text) => win.pushEvents([{type: 'agent', provider: 'antigravity', model: 'gemini-4-argon-high', credential: '', activity: 'working', inputTokens: 0, outputTokens: 0, cachedTokens: 0,
    transcript: [{speaker: 'you', text: 'read it', thinking: '', streaming: false, calls: [], attached: []},
      {speaker: 'proton', text, thinking: '', streaming: true, calls, attached: []}]}]);
  await push('Reading');
  await nextFrame();
  const dock = $('#codex-dock');
  const fold = dock.querySelector('.tool-fold');
  assert(fold && !fold.open, 'ten calls were not folded');
  assert(fold.querySelector('summary').textContent.includes('Read file ×10') && fold.querySelector('.fold-failed')?.textContent === '9 failed', fold.querySelector('summary').textContent);
  assert(fold.querySelectorAll('.tool').length === 10, 'the rows are not inside the fold');
  assert(dock.querySelector('.tool-ask') && !fold.querySelector('.tool-ask'), 'the command to answer was folded away');
  // Opened, it stays open as the answer goes on.
  fold.open = true; fold.dispatchEvent(new win.Event('toggle')); await settle();
  await push('Reading more');
  await nextFrame();
  assert(dock.querySelector('.tool-fold')?.open, 'a redraw closed the fold');
  dock.querySelector('.tool-fold').open = false; dock.querySelector('.tool-fold').dispatchEvent(new win.Event('toggle'));
});
await test('A file the agent wrote has an Undo, and a command it proposes runs only from Run', async () => {
  await showAgentDock();
  await win.pushEvents([{type: 'agent', provider: 'claude', model: 'claude-opus-5-5', credential: '', activity: 'working', inputTokens: 0, outputTokens: 0, cachedTokens: 0,
    transcript: [{speaker: 'you', text: 'make a cheatsheet and build it', thinking: '', streaming: false, calls: []},
      {speaker: 'proton', text: '', thinking: '', streaming: true, calls: [
        {id: 'E1', name: 'edit_file', detail: 'path=notes/cheatsheet.md', result: 'created notes/cheatsheet.md (12 lines)', state: 'done', undo: true, asks: false},
        {id: 'C1', name: 'run_command', detail: 'make -C build all && ./build/test --verbose', result: 'waiting for you to run it or not', state: 'running', undo: false, asks: true}]}]}]);
  const dock = $('#codex-dock');
  const ask = dock.querySelector('.tool-ask');
  // The whole command, before it is run.
  assert(ask?.querySelector('code')?.textContent === 'make -C build all && ./build/test --verbose', ask?.textContent);
  assert([...dock.querySelectorAll('.tool b')].some((b) => b.textContent === 'Run command'), 'the row is unnamed');
  let before = win.commands.length;
  dock.querySelector('[data-agent-undo="E1"]').click(); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'undoAgentEdit' && c.call === 'E1'), 'Undo sent nothing');
  before = win.commands.length;
  dock.querySelector('[data-agent-run="C1"]').click(); await settle();
  const ran = win.commands.slice(before).find((c) => c.type === 'agentCommand');
  assert(ran?.call === 'C1' && ran.run === true, JSON.stringify(ran));
  // Skip says no.
  await win.pushEvents([{type: 'agent', provider: 'claude', model: 'claude-opus-5-5', credential: '', activity: 'working', inputTokens: 0, outputTokens: 0, cachedTokens: 0, from: 1,
    transcript: [{speaker: 'proton', text: '', thinking: '', streaming: true, calls: [
      {id: 'C2', name: 'run_command', detail: 'rm -rf build', result: 'waiting for you to run it or not', state: 'running', undo: false, asks: true}]}]}]);
  before = win.commands.length;
  dock.querySelector('[data-agent-skip="C2"]').click(); await settle();
  const skipped = win.commands.slice(before).find((c) => c.type === 'agentCommand');
  assert(skipped?.call === 'C2' && skipped.run === false, JSON.stringify(skipped));
});
await test('An answer in the agent panel colours its code, draws its TeX, keeps its citations, and never becomes HTML', async () => {
  const agent = async (transcript) => {
    await win.pushEvents([{type: 'agent', provider: 'codex', model: 'gpt-6-astra', credential: '',
      activity: 'idle', inputTokens: 0, outputTokens: 0, cachedTokens: 0, transcript}]);
    await nextFrame();
  };
  const answer = ['Run it like this:', '', '```rust', '#[derive(Debug)]', 'fn main() { let n: u32 = 42; println!("{n}"); } // done', '```', '',
    'The energy is $E = mc^2$; $5 and $10 are prices, and \\$x\\$ is not TeX. Then', '$$', '\\int_0^1 x^2\\,dx = \\tfrac{1}{3}', '$$',
    'See [[notes/physics.md § Energy L1-4]] and [the docs](https://example.com/a).', '', '<img src=x onerror="window.pwned=1">'].join('\n');
  await showAgentDock();
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
await test('A streaming answer arrives as the entries that changed, spliced onto the conversation the window has', async () => {
  const agent = async (transcript, from) => {
    await win.pushEvents([{type: 'agent', provider: 'codex', model: 'gpt-6-astra', credential: '',
      activity: 'idle', inputTokens: 0, outputTokens: 0, cachedTokens: 0, transcript, ...(from === undefined ? {} : {from})}]);
    await nextFrame();
  };
  const you = {speaker: 'you', text: 'first question', thinking: '', streaming: false, calls: []};
  await showAgentDock();
  await agent([you, {speaker: 'proton', text: 'Partly', thinking: '', streaming: true, calls: []}]);
  const thread = $('#codex-dock [data-agent-thread]');
  const asked = thread.querySelector('.msg.you');
  await agent([{speaker: 'proton', text: 'Partly answered, and more', thinking: '', streaming: false, calls: []}], 1);
  const texts = () => [...thread.querySelectorAll('.msg')].map((m) => m.textContent);
  assert(texts().length === 2 && texts()[0].includes('first question') && texts()[1].includes('Partly answered, and more'), `wrong conversation: ${texts()}`);
  assert(thread.querySelector('.msg.you') === asked, 'the question was drawn again');
  await agent([{speaker: 'you', text: 'second question', thinking: '', streaming: false, calls: []}], 2);
  assert(texts().length === 3 && texts()[2].includes('second question'), `the new entry was not added: ${texts()}`);
  // A whole conversation — another chat opened — replaces it all.
  await agent([{speaker: 'you', text: 'another chat', thinking: '', streaming: false, calls: []}]);
  assert(texts().length === 1 && texts()[0].includes('another chat'), `the old conversation is still there: ${texts()}`);
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
await test('What the agent proposes for a notebook is drawn on its cells and answered there or from its panel', async () => {
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  const rewrite = {id: 7, action: 'replace', place: 'on', kind: '', source: 'x = 2', said: 'rewrite cell 0', cell: 0};
  const added = {id: 8, action: 'insert', place: 'below', kind: 'markdown', source: '# Why', said: 'add a cell at 1', cell: 0};
  const cell = {kind: 'code', source: 'x = 1', outputs: [], executionCount: null, running: false, selected: true, key: 1, proposals: [rewrite, added]};
  await win.pushEvents([{type: 'notebook', path: '/fixture/p.ipynb', problem: '', language: 'python', cells: [cell],
    modified: true, proposals: [rewrite, added]}]);
  await settle();
  const drawn = $('.workspace[data-ws="data"] [data-cell="0"]');
  assert(drawn?.querySelectorAll('.proposal').length === 2 && drawn.querySelector('.proposal-new'), 'the proposals are not drawn on the cell');
  assert($('[data-nb-save]').classList.contains('dirty'), 'unsaved changes are not shown on Save');
  let sent = win.commands.length;
  drawn.querySelector('[data-proposal="7"][data-accept="1"]').click(); await settle();
  const answered = win.commands.slice(sent).find((c) => c.type === 'answerProposal');
  assert(answered?.id === 7 && answered.accept === true, 'accepting on the cell did not answer it');

  // The panel: the agent's own tab beside the table, listing what waits.
  $('[data-data-panel="agent"]').click(); await settle();
  assert(!$('.data-agent').hidden && $('[data-df-dock]').hidden, 'the agent tab did not take the panel');
  assert($('[data-proposals]').textContent.includes('2 changes proposed'), 'the panel does not list the proposals');
  sent = win.commands.length;
  $('[data-proposals-all="0"]').click(); await settle();
  assert(win.commands.slice(sent).filter((c) => c.type === 'answerProposal' && c.accept === false).length === 2, 'reject all did not answer both');
  $('[data-data-panel="table"]').click(); await settle();
  assert($('.data-agent').hidden && !$('[data-df-dock]').hidden, 'the table did not come back');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('Refer on a staged hunk goes to it, and Reject answers it', async () => {
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  await win.pushEvents([{type: 'review', files: [{path: 'src/lib.rs', creates: false,
    hunks: [{line: 12, lines: [{mark: 'removed', text: 'old'}, {mark: 'added', text: 'new'}]}]}]}]);
  await settle();
  const staged = $('#codex-dock [data-staged]');
  let sent = win.commands.length;
  staged.querySelector('[data-refer]').click(); await settle();
  const went = win.commands.slice(sent);
  assert(went.find((c) => c.type === 'jumpTo')?.line === 11 && went.find((c) => c.type === 'jumpTo')?.path === 'src/lib.rs',
    'Refer did not go to the hunk');
  assert(!went.some((c) => c.type === 'rejectHunk'), 'Refer threw the hunk away');
  sent = win.commands.length;
  staged.querySelector('[data-reject]').click(); await settle();
  assert(win.commands.slice(sent).find((c) => c.type === 'rejectHunk')?.hunk === 0, 'Reject did not reject the hunk');
  await win.pushEvents([{type: 'review', files: []}]); await settle();
});
await test('Quitting with unsaved changes asks, and Cancel stays', async () => {
  await win.pushEvents([{type: 'unsaved', files: ['src/main.rs', 'notes.md']}]); await settle();
  const ask = $('[data-quit-ask]');
  assert(ask && !ask.hidden && ask.textContent.includes('src/main.rs') && ask.textContent.includes('notes.md'), 'the unsaved files are not named');
  assert(ask.querySelector('[data-quit="save"]') && ask.querySelector('[data-quit="lose"]'), 'the choices are missing');
  ask.querySelector('[data-quit="stay"]').click(); await settle();
  assert(ask.hidden, 'Cancel did not put the question away');
  assert(ask.querySelector('h2').textContent.includes('quitting'), 'quitting is not what it asks about');
  // One window of several closing asks about closing it, not quitting.
  await win.pushEvents([{type: 'unsaved', files: ['notes.md'], closing: true}]); await settle();
  assert(!ask.hidden && ask.querySelector('h2').textContent === 'Save your changes before closing this window?', `closing a window asks: ${ask.querySelector('h2').textContent}`);
  assert(ask.querySelector('[data-quit="save"]').textContent === 'Save all and close' && ask.querySelector('[data-quit="lose"]').textContent === 'Close without saving', 'the choices still say quit');
  ask.querySelector('[data-quit="stay"]').click(); await settle();
  assert(ask.hidden, 'Cancel did not put the window\'s question away');
});
await test('A running cell sends only itself: it is drawn again and the notebook\'s other cells are left as they are', async () => {
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  const cell = (key, source, extra = {}) => ({kind: 'code', source, outputs: [], executionCount: null, running: false, selected: false, key, proposals: [], ...extra});
  await win.pushEvents([{type: 'notebook', path: '/fixture/run.ipynb', problem: '', language: 'python', modified: false, proposals: [], cells: [
    cell(11, 'a = 1', {selected: true}), cell(12, 'for i in range(3): print(i)', {running: true}), cell(13, 'a + 1')]}]);
  await settle();
  const box = $('.workspace[data-ws="data"] .split.on [data-cells]');
  const at = (i) => box.querySelector(`:scope > [data-cell="${i}"]`);
  const [first, third] = [at(0), at(2)];
  const lines = (text, extra = {}) => win.pushEvents([{type: 'notebookCells', path: '/fixture/run.ipynb', modified: Boolean(extra.modified),
    cells: [{index: 1, cell: cell(12, 'for i in range(3): print(i)', {running: true, outputs: [{type: 'stream', name: 'stdout', text}], ...extra})}]}]);
  await lines('0\n'); await settle();
  assert(at(1).textContent.includes('0') && at(1).classList.contains('running'), 'the running cell\'s first line is not shown');
  await lines('0\n1\n2\n', {running: false, executionCount: 4, modified: true}); await settle();
  assert(at(1).textContent.includes('2') && !at(1).classList.contains('running') && at(1).textContent.includes('[4]'), `the finished cell is not shown: ${at(1).textContent}`);
  assert(at(0) === first && at(2) === third, 'a cell that did not change was drawn again');
  assert(box.children.length === 3, `${box.children.length} cells, not 3`);
  assert($('[data-nb-save]').classList.contains('dirty'), 'the notebook is not marked unsaved');
  // A cell that is not where the window has it is left to the next whole notebook.
  await win.pushEvents([{type: 'notebookCells', path: '/fixture/run.ipynb', modified: true, cells: [{index: 2, cell: cell(99, 'elsewhere')}]}]); await settle();
  assert(at(2) === third && !at(2).textContent.includes('elsewhere'), 'a cell out of step was drawn');
  assert(win.testErrors.length === 0, win.testErrors.join('; '));
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
await test('A PDF opens over the mode on screen, draws its pages as they come near, and keeps the keyboard', async () => {
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  const before = win.commands.length;
  // Cited by its second page, in Data rather than Codex.
  await win.pushEvents([{type: 'document', path: '/tests/ui/fixtures/three-pages.pdf', name: 'three-pages.pdf', kind: 'pdf', page: 2}]);
  const viewer = $('#doc-viewer');
  const wait = async (ready) => { for (let i = 0; i < 40 && !ready(); i++) await settle(); };
  await wait(() => viewer.querySelectorAll('.doc-page').length === 3 && viewer.querySelector('.textLayer span'));
  assert(!viewer.hidden && viewer.closest('.workspace.on')?.dataset.ws === 'data', 'the PDF did not open over the mode on screen');
  assert(viewer.querySelectorAll('.doc-page').length === 3, 'not a place a page');
  assert(viewer.querySelector('.doc-page canvas'), 'no page was drawn');
  assert([...viewer.querySelectorAll('.textLayer span')].some((s) => s.textContent.includes('Proton reads this sentence')), 'the page\'s text is not there to select');
  assert($('[data-doc-at]').textContent === 'page 2 of 3', `it did not open at the page cited: ${$('[data-doc-at]').textContent}`);
  // Its keys move it, and nothing reaches the notebook underneath.
  const box = viewer.querySelector('[data-doc-pages-box]');
  const top = box.scrollTop;
  key('j'); key('j'); await settle();
  assert(box.scrollTop > top, 'j did not scroll the document');
  key('d'); key('d'); key('x'); await settle();
  assert(!win.commands.slice(before).some((c) => c.type === 'key'), 'a key reached the core from over a PDF');
  const width = viewer.querySelector('.doc-page').offsetWidth;
  key('-'); await settle();
  assert(viewer.querySelector('.doc-page').offsetWidth < width, '- did not zoom out');
  key('Escape'); await settle();
  assert(viewer.hidden && !viewer.querySelector('.doc-page'), 'Escape did not put it away');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('A PDF that leans on what it does not carry — CJK text in a font it does not embed — is drawn and its text read', async () => {
  // 日本語 in a CID font encoded with Adobe's UniJIS-UCS2-H: neither drawn nor
  // extracted unless pdf.js finds its vendored character maps.
  await win.pushEvents([{type: 'document', path: '/tests/ui/fixtures/cjk.pdf', name: 'cjk.pdf', kind: 'pdf', page: null}]);
  const viewer = $('#doc-viewer');
  const text = () => [...viewer.querySelectorAll('.textLayer span')].map((s) => s.textContent).join('');
  for (let i = 0; i < 60 && !text().includes('日本語'); i++) await settle();
  assert(text().includes('日本語'), `the CJK text was not read: ${JSON.stringify(text())} ${viewer.querySelector('.doc-note')?.textContent ?? ''}`);
  assert(viewer.querySelector('.doc-page canvas'), 'the page was not drawn');
  key('Escape'); await settle();
  assert(viewer.hidden, 'Escape did not put it away');
});
await test('A notebook draws an SVG figure as a picture, whose scripts and styles stay its own', async () => {
  await win.dispatch({type: 'setMode', mode: 'data'}); await settle();
  win.svgRan = false;
  // A kernel's SVG with what must not escape it: a script, a handler, a style
  // for everything — and a minus sign, which plain btoa would refuse.
  const svg = '<?xml version="1.0"?>\n<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" onload="parent.svgRan = true">\n' +
    '<style>*{display:none !important}</style><script>parent.svgRan = true</script>\n<text x="2" y="15">\u22121</text></svg>\n';
  const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
  await win.pushEvents([{type: 'notebook', path: '/fixture/plot.ipynb', problem: '', language: 'python', cells: [
    {kind: 'code', source: 'plt.plot(x)', executionCount: 1, running: false, selected: true,
      outputs: [{type: 'svg', svg}, {type: 'image', mime: 'image/png', base64: png}]}]}]);
  await settle();
  const ws = $('.workspace[data-ws="data"]');
  const [vector, raster] = ws.querySelectorAll('.split.on img.out-img');
  assert(vector?.src.startsWith('data:image/svg+xml') && raster?.src.startsWith('data:image/png;base64,'), `the pictures are not data URLs of their types: ${vector?.src.slice(0, 40)} ${raster?.src.slice(0, 40)}`);
  await vector.decode();
  await raster.decode();
  assert(vector.naturalWidth > 0 && raster.naturalWidth > 0, 'a picture did not draw');
  assert(!ws.querySelector('.out-wrap svg, .split.on .cell svg:not(.i)'), 'the SVG was put in the page rather than drawn as a picture');
  assert(win.svgRan === false, 'the SVG\'s script ran');
  assert(win.getComputedStyle(ws).display !== 'none', 'the SVG\'s style reached the window');
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
});
await test('Research has gone to Fairy: Excel follows Data at ⌃3, in Codex\'s chrome and on an opaque ground', async () => {
  const asked = () => (win.invokes ?? []).filter((c) => c.name === 'set_opaque').map((c) => c.args.color);
  const ground = win.getComputedStyle(doc.documentElement).getPropertyValue('--bg').trim().toLowerCase();
  await win.dispatch({type: 'setMode', mode: 'codex'}); await settle();
  const before = win.commands.length, opaque = asked().length;
  doc.body.focus();
  key('3', {ctrlKey: true}); await settle();
  assert(win.commands.slice(before).some((c) => c.type === 'setMode' && c.mode === 'sheet'), '⌃3 did not ask for Excel');
  assert($('.workspace.on')?.dataset.ws === 'sheet', 'Excel is not on screen');
  assert(shown($('.titlebar')) && !shown($('.chrome-rail')) && !shown($('.rail')) && !shown($('.statusline')), 'Excel kept the old chrome');
  const tab = $('.titlebar [data-mode="sheet"]');
  assert(tab?.getAttribute('aria-selected') === 'true' && tab.title.includes('⌃3'), 'the title bar does not show Excel at ⌃3');
  // No Research anywhere: not in the title bar, not on the rail, no section of its own.
  for (const list of ['.tb-modes', '.mode-rail']) {
    const order = [...doc.querySelectorAll(`${list} [data-mode]`)].map((b) => b.dataset.mode).join();
    assert(order === 'codex,data,sheet,circuit,logic,cad', `${list} is not in the new order: ${order}`);
  }
  assert(!$('.workspace[data-ws="research"]'), 'Research still has a section');
  {
    const now = win.commands.length;
    key('7', {ctrlKey: true}); await settle();
    assert(!win.commands.slice(now).some((c) => c.type === 'setMode'), '⌃7 asked for a mode past the last');
  }
  assert(!asked().slice(opaque).includes(null) && asked().at(-1) === ground, `Excel is not on the ground: ${JSON.stringify(asked().slice(opaque))}`);
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
await sheetChecks({test, assert, win, doc, $, settle, key, shown});
await circuitChecks({test, assert, win, doc, $, settle, key, shown});
await logicChecks({test, assert, win, doc, $, settle, key, shown});
await cadChecks({test, assert, win, doc, $, settle, key, shown});
await orderingChecks(test, assert);
document.querySelector('#status').textContent = `${passed} passed · ${failed} failed`;
document.title = `Proton UI tests: ${passed} passed, ${failed} failed`;
restoreLayouts();
