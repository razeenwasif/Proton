// In-memory native transport for DOM regression tests. No filesystem or model calls.
window.commands = [];
window.testErrors = [];
const originalError = console.error;
console.error = (...args) => { window.testErrors.push(args.map(String).join(' ')); originalError(...args); };
window.addEventListener('error', (event) => window.testErrors.push(event.message));
window.addEventListener('unhandledrejection', (event) => window.testErrors.push(String(event.reason)));
let mode = 'codex';
let settings = {};
const settingsEvent = () => ({type: 'settings', values: settings, path: '/fixture/.proton/settings.json'});
let query = '';
let picker = false;
let treeExpanded = false;
let activePath = 'src/fixture.c';
const treeRow = (name, path, depth, dir = false) => ({name, path, depth, dir, expanded: dir && treeExpanded, active: path === activePath, truncated: 0});
const tree = () => ({type: 'tree', rows: [treeRow('src', 'src', 0, true),
  ...(treeExpanded ? [treeRow('fixture.c', 'src/fixture.c', 1), treeRow('helpers.c', 'src/helpers.c', 1)] : []),
  treeRow('README.md', 'README.md', 0)]});
let focused = 0, vertical = true;
let views = [{buffer: 0, name: 'fixture.c', cell: 0}];
const buffers = [{name: 'fixture.c', path: 'src/fixture.c', modified: false}];
const heights = new Map();
let sequence = 0;
window.replyDelay = 0;
window.makeBatch = events => ({sequence: events.length ? ++sequence : sequence, events});
const lines = Array.from({length: 200}, (_, i) => `line ${i + 1}`);
const viewport = (count = 40, view = focused) => window.projectFixture ? projectViewport(count, view) : ({type: 'viewport', view, firstLine: 0,
  lines: (views[view]?.buffer === 0 ? lines : [buffers[views[view]?.buffer]?.path ?? '']).slice(0, count),
  totalLines: views[view]?.buffer === 0 ? lines.length : 1, highlights: [], matches: [], git: [], stops: []});
const cursor = (view = focused, column = 0) => ({type: 'cursor', view, line: 0, column, selection: null});
const splits = () => ({type: 'splits', views: structuredClone(views), focused, vertical});
const bufferEvent = () => ({type: 'buffers', names: structuredClone(buffers), active: views[focused].buffer});
const show = () => [splits(), bufferEvent(), viewport(heights.get(focused) ?? 40), cursor()];
const projectItems = ['Alpha', 'Beta'].map(name => ({name, path: `/fixture/${name}`, modified: false}));
function projectViewport(count = 40, view = 0) {
  const {active, first} = window.projectFixture;
  const start = first[active];
  return {type: 'viewport', view, firstLine: start, totalLines: 100000, version: 0,
    lines: Array.from({length: Math.min(count, 100000 - start)}, (_, i) => `${projectItems[active].name} row ${start + i}`),
    highlights: [], matches: [], git: [], stops: []};
}
function projectEvents(index) {
  window.projectFixture.active = index;
  focused = 0; views = [{buffer: 0, name: 'same.txt', cell: 0}];
  return [{type: 'state', mode: 'codex', vimLabel: 'NORMAL'}, splits(),
    {type: 'buffers', names: [{name: 'same.txt', path: `${projectItems[index].path}/same.txt`, modified: false}], active: 0},
    projectViewport(), {type: 'cursor', view: 0, line: window.projectFixture.first[index], column: 0, selection: null},
    {type: 'workspaces', items: projectItems, active: index}, settingsEvent()];
}
window.installProjectFixtures = () => {
  window.projectFixture = {active: 0, first: [10000, 30000]};
  window.pushEvents(projectEvents(0));
};
const pick = () => ({type: 'picker', kind: 'files', title: 'Find files', query,
  items: [{label: 'fixture.c', detail: 'src', path: 'src/fixture.c', positions: []}],
  selected: 0, total: 1, elapsedMs: 0, scanning: false, cutShort: false});
window.__TAURI__ = {
  event: {listen: async (_, callback) => { window.pushBatch = payload => callback({payload}); window.pushEvents = events => callback({payload: window.makeBatch(events)}); return () => {}; }},
  window: {getCurrentWindow: () => ({close() {}, minimize() {}, toggleMaximize() {}, startDragging() {}})},
  core: {invoke: async (name, args = {}) => {
    const {command} = args;
    if (name !== 'dispatch') (window.invokes ??= []).push({name, args});
    if (name === 'backdrop') return 'vibrancy';
    // A document's bytes, as the shell reads one: the fixture's, from the server.
    if (name === 'read_document') {
      const response = await fetch(args.path);
      if (!response.ok) throw new Error(`${args.path}: not a document Proton opened`);
      return await response.arrayBuffer();
    }
    // The open canvas's tiles, as a test's stand-in core lays them out.
    if (name === 'read_raster') {
      const bytes = await window.rasterRead?.(args.generation, args.layer, args.tiles);
      if (!bytes) throw new Error(`layer ${args.layer} of canvas ${args.generation} is not open`);
      return bytes;
    }
    // Pictures handed to the core, kept as a test can look at them.
    if (name === 'put_raster_blob') {
      (window.rasterBlobs ??= []).push(args);
      return window.rasterBlobs.length;
    }
    if (name === 'paste_raster_image') return window.rasterPaste?.() ?? null;
    if (name === 'paste_text') return window.clipboardText ?? null;
    if (name === 'copy_text') { window.clipboardText = args?.text ?? null; return null; }
    if (name === 'copy_raster_image') return true;
    // The open game's images: the fixture game's, from the server.
    if (name === 'read_game') {
      // A test's own files first: an image cut up, say.
      const own = window.gameFiles?.[args.key];
      if (own !== undefined) return typeof own === 'string' ? new TextEncoder().encode(own).buffer : own;
      const response = await fetch(`/tests/ui/fixtures/game/${args.key}`);
      if (!response.ok) throw new Error(`${args.key}: not an image of the open game`);
      return await response.arrayBuffer();
    }
    // A Lab run's files, as a test puts them.
    if (name === 'read_lab') {
      const bytes = window.labFiles?.[args.key];
      if (!bytes) throw new Error(`${args.key}: not a file a run left`);
      return bytes;
    }
    if (name !== 'dispatch') return [];
    window.commands.push(command);
    const events = (() => { switch (command.type) {
      case 'focusWorkspace': return window.projectFixture ? projectEvents(command.index) : [];
      case 'sync': return [{type: 'state', mode, vimLabel: 'NORMAL'}, splits(), viewport(),
        {type: 'buffers', names: [{name: 'fixture.c', path: 'src/fixture.c', modified: false}], active: 0},
        {type: 'workspaces', items: [{name: 'Fixture', path: '/fixture', modified: false}], active: 0},
        {type: 'repo', branch: 'test-branch', files: []}, {type: 'tasks', tasks: []}, settingsEvent(), tree()];
      case 'setMode': mode = command.mode; return [{type: 'modeChanged', mode}, splits(), viewport()];
      case 'toggleTreeNode': treeExpanded = !treeExpanded; return [tree()];
      case 'openBuffer': {
        if (window.failOpen) return [{type: 'notice', level: 'error', text: 'Could not open the file. It is no longer available.'}];
        activePath = command.path;
        let index = buffers.findIndex(buffer => buffer.path === activePath);
        if (index < 0) { index = buffers.length; buffers.push({name: activePath.split('/').at(-1), path: activePath, modified: false}); }
        views[focused].buffer = index; views[focused].name = buffers[index].name;
        return [...show(), tree()];
      }
      case 'focusBuffer': views[focused].buffer = command.index; views[focused].name = buffers[command.index].name; return show();
      case 'split': vertical = command.vertical; views.push({...views[focused]}); focused = views.length - 1; return [splits(), bufferEvent(), ...views.flatMap((_, view) => [viewport(40, view), cursor(view)])];
      case 'focusSplit': focused = command.index; return show();
      case 'onlySplit': views = [{...views[focused]}]; focused = 0; return show();
      case 'closeSplit': if (views.length > 1) { views.splice(command.index, 1); focused = Math.min(focused, views.length - 1); } return show();
      case 'closeBuffer': {
        if (window.refuseClose) return [{type: 'notice', level: 'warn', text: 'Unsaved changes. Save before closing.'}];
        if (buffers.length === 1) return [];
        buffers.splice(command.index, 1);
        views.forEach(view => { if (view.buffer >= command.index) view.buffer = Math.max(0, view.buffer - 1); view.name = buffers[view.buffer].name; });
        return show();
      }
      case 'placeCursor': return [cursor(focused, command.column)];
      // The real core answered every size with a screen; with `echoTerminalSize`
      // this does too, which is what kept the panel measuring itself.
      case 'terminalSize': return window.echoTerminalSize ? [{type: 'terminal', running: true, scrollback: 0,
        cursor: [0, 0], cursorVisible: true, rows: Array.from({length: command.rows}, () => [{text: '%'}])}] : [];
      case 'setSetting': {
        const [group, key] = command.key.split('.');
        if (key) (settings[group] ??= {})[key] = command.value;
        else settings[group] = command.value;
        return [settingsEvent()];
      }
      case 'viewport':
        heights.set(command.view, command.lines);
        if (window.projectFixture) window.projectFixture.first[window.projectFixture.active] = command.firstLine;
        return [viewport(command.lines, command.view)];
      case 'openPicker': picker = true; query = ''; return [pick()];
      case 'pickerQuery': query = command.text; return picker ? [pick()] : [];
      case 'key':
        if (picker && ['Escape', 'Enter'].includes(command.key.key)) {
          picker = false; return [{type: 'pickerClosed'}];
        }
        return [];
      default: return [];
    } })();
    const batch = window.makeBatch(events);
    if (window.replyDelay) await new Promise(resolve => setTimeout(resolve, window.replyDelay));
    return batch;
  }},
};
