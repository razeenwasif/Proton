/* The shell bridge.
 *
 * Every keystroke and mode change goes to `proton-core` and comes back as
 * events; the page renders events and decides nothing on its own. When Tauri
 * is absent — the plain `node ui/serve.mjs` preview — the same page runs
 * against a local stand-in, so the design work never needs the shell.
 *
 * Types for `Command` and `Event` are generated from the Rust definitions into
 * ./bindings/ by `cargo test -p proton-core`. They are not imported here (this
 * file is plain JS with no build step) but they are the contract, and they will
 * fail the Rust build if they drift.
 *
 * @typedef {import("./bindings/Command").Command} Command
 * @typedef {import("./bindings/Event").Event} Event
 */

const tauri = globalThis.__TAURI__;
export const HAS_SHELL = Boolean(tauri?.core?.invoke);

/* ── transport ────────────────────────────────────────────────────────── */

/** @type {(command: Command) => Promise<Event[]>} */
let send;

if (HAS_SHELL) {
  send = (command) => tauri.core.invoke("dispatch", { command });
} else {
  // Browser preview. Enough of the modal engine to keep the design prototype
  // honest — it must never grow past that: the real state machine is in
  // crates/proton-core/src/vim.rs and this is a demo stand-in, not a fork.
  const state = { mode: "codex", vim: "Normal", cmdline: "", prompt: ":", pattern: "", leader: null };
  const LABEL = {
    Normal: "NORMAL", Insert: "INSERT", Visual: "VISUAL",
    VisualLine: "V-LINE", Replace: "REPLACE", Command: "COMMAND",
  };
  const vimEvent = () => ({ type: "vimMode", mode: state.vim, label: LABEL[state.vim] });

  // Read-only file viewing, so the design preview shows real code instead of
  // an empty pane. It cannot edit — the rope lives in Rust — and it must never
  // learn how to.
  const doc = { lines: [""], first: 0, height: 60, name: "[No Name]", path: null };
  // One split in the preview. The real layout — how many, which buffer each
  // shows, whose cursor is where — lives in crates/proton-core/src/split.rs;
  // this only has to give the chrome something to draw.
  const layout = { count: 1, focused: 0, vertical: true, scroll: [0] };
  // Staged changes for the preview. Filled on first use, so the panel is
  // empty until something asks for it — the way it is in the real thing.
  const staged = {};

  // A canned queue, so the Tasks panel can be designed against. The real one
  // is in crates/proton-core/src/tasks.rs and runs the turns for real.
  let queue = [
    { id: 0, prompt: "Memoise the fleet grid", state: "running",
      note: "Looking at how ranked is rebuilt", tools: 3, files: [] },
    { id: 1, prompt: "Port trajectory.c to the Android target", state: "queued",
      note: "", tools: 0, files: [] },
    { id: 2, prompt: "Ingest six months of telemetry", state: "ready",
      note: "Parquet written, schema inferred, 3 nulls flagged", tools: 6,
      files: ["src/lib/telemetry.ts", "src/schema.json"] },
    { id: 3, prompt: "Explain the pointer bug in trajectory.c", state: "failed",
      note: "no ANTHROPIC_API_KEY", tools: 0, files: [] },
  ];
  const tasksEvent = () => ({ type: "tasks", tasks: queue });

  // A canned pair of plugins, so the settings panel and the theme strip can
  // be designed against. The real ones are wasm components loaded by
  // crates/proton-plugin; this only has to show the chrome.
  const pluginsEvent = () => ({
    type: "plugins",
    items: [
      { id: "dusk", name: "Dusk", description: "A theme pack: two palettes, no code.",
        version: "0.1.0", provides: "2 themes", problem: "" },
      { id: "hello", name: "Hello", description: "The example plugin.",
        version: "0.1.0", provides: "1 theme · 3 commands", problem: "" },
      { id: "broken", name: "broken", description: "", version: "",
        provides: "", problem: "broken: expected a component, found a module" },
    ],
    themes: [
      { id: "dusk-rose", name: "Dusk Rose", dark: true, from: "dusk",
        colours: [["bg", "#1f1a1d"], ["bg-deep", "#151113"], ["text", "#e0d4d9"],
                  ["text-hi", "#f6ecf0"], ["accent", "#d98a9b"]] },
      { id: "dusk-moss", name: "Dusk Moss", dark: true, from: "dusk",
        colours: [["bg", "#1a1f1b"], ["bg-deep", "#121613"], ["text", "#d2ded4"],
                  ["text-hi", "#eef5ef"], ["accent", "#8fbf9f"]] },
    ],
  });

  const splitsEvent = () => ({
    type: "splits",
    views: Array.from({ length: layout.count }, () => ({ buffer: 0, name: doc.name })),
    focused: layout.focused,
    vertical: layout.vertical,
  });

  /// Every pane's content. The preview shows one document in all of them —
  /// the real thing gives each view its own buffer — but the scroll is per
  /// pane, which is the part the chrome has to get right.
  const everyViewport = () =>
    Array.from({ length: layout.count }, (_, i) => ({
      ...viewportEvent(),
      view: i,
      firstLine: layout.scroll[i] ?? 0,
      lines: doc.lines.slice(layout.scroll[i] ?? 0, (layout.scroll[i] ?? 0) + doc.height),
    }));

  const viewportEvent = () => ({
    type: "viewport",
    view: 0,
    firstLine: doc.first,
    lines: doc.lines.slice(doc.first, doc.first + doc.height),
    totalLines: doc.lines.length,
    version: 0,
    // A couple of breakpoints, so the sign column has something to draw.
    stops: [{ line: 3, verified: true, here: true }, { line: 9, verified: false, here: false }],
    // A fixed gutter, so the strip has something to draw. The real marks are
    // a line diff against HEAD in crates/proton-vcs.
    git: doc.lines.slice(doc.first, doc.first + doc.height).map((_, i) =>
      i === 2 ? "added" : i === 3 ? "modified" : i === 7 ? "removed" : null),
    // Literal, folded like the real one in crates/proton-text/src/search.rs.
    // Enough to draw the layer; the rule that matters lives in Rust.
    matches: doc.lines.slice(doc.first, doc.first + doc.height).map((line) => {
      if (!state.pattern) return [];
      const sensitive = /[A-Z]/.test(state.pattern);
      const hay = sensitive ? line : line.toLowerCase();
      const needle = sensitive ? state.pattern : state.pattern.toLowerCase();
      const hits = [];
      for (let at = hay.indexOf(needle); at !== -1; at = hay.indexOf(needle, at + needle.length)) {
        hits.push({ start: at, end: at + needle.length });
      }
      return hits;
    }),
  });

  // A static set of projects, so the workspace rail has something to draw in
  // the preview. Fixed data on purpose: the session — what is open, what is
  // remembered, what a folder is called when two share a name — lives in
  // crates/proton-core/src/session.rs, and this only has to show the chrome.
  const projects = {
    active: 0,
    items: [
      { name: "Proton", path: "C:\\Users\\Razeen\\Projects\\Proton", modified: false },
      { name: "orbit/web", path: "C:\\Users\\Razeen\\Projects\\orbit\\web", modified: true },
      { name: "scratch", path: "C:\\Users\\Razeen\\scratch", modified: false },
    ],
  };
  const workspacesEvent = () => ({ type: "workspaces", ...projects });

  // A fixed tree, so the sidebar has rows to draw and folders to open. The
  // real one is computed from the disk in crates/proton-core/src/tree.rs and
  // honours the project's ignore rules; this only has to show the chrome.
  const FILES = {
    "": ["crates/", "ui/", "docs/", "Cargo.toml", "README.md"],
    "crates": ["proton-core/", "proton-text/"],
    "crates/proton-core": ["src/"],
    "crates/proton-core/src": ["vim.rs", "workspace.rs", "tree.rs", "ex.rs"],
    "crates/proton-text": ["src/"],
    "crates/proton-text/src": ["buffer.rs", "edit.rs"],
    "ui": ["app.js", "bridge.js", "styles.css"],
    "docs": ["ARCHITECTURE.md", "STACK.md"],
  };
  const open = new Set(["crates", "crates/proton-core", "crates/proton-core/src"]);

  const treeRows = (dir = "", depth = 0, rows = []) => {
    for (const entry of FILES[dir] ?? []) {
      const isDir = entry.endsWith("/");
      const name = isDir ? entry.slice(0, -1) : entry;
      const path = dir ? `${dir}/${name}` : name;
      const expanded = isDir && open.has(path);
      rows.push({ depth, name, path, dir: isDir, expanded, active: path === doc.path, truncated: 0 });
      if (expanded) treeRows(path, depth + 1, rows);
    }
    return rows;
  };
  const treeEvent = () => ({ type: "tree", rows: treeRows() });

  // A canned terminal screen, so the panel can be designed against. The real
  // one is a pseudo-terminal in crates/proton-term; this only has to show the
  // chrome and the colours.
  const run = (text, extra = {}) => ({ text, fg: null, bg: null, rgb: null, bold: false, italic: false, underline: false, inverse: false, ...extra });
  const terminalEvent = () => ({
    type: "terminal",
    rows: [
      [run("C:\Users\Razeen\Projects\Proton> "), run("cargo test", { bold: true })],
      [run("   Compiling proton-term v0.0.1", { fg: 8 })],
      [run("    Finished ", { fg: 2, bold: true }), run("test profile in 1.2s")],
      [run("running 7 tests")],
      [run("test result: "), run("ok", { fg: 2, bold: true }), run(". 7 passed; 0 failed")],
      [run("C:\Users\Razeen\Projects\Proton> ")],
    ],
    cursor: [5, 34],
    cursorVisible: true,
    running: true,
  });

  // A canned debug session, so the panel can be designed against. The real
  // one drives a debug adapter from crates/proton-core/src/debug.rs.
  const debugEvent = () => ({
    type: "debug",
    phase: "stopped",
    frames: [
      { name: "add", path: "src/demo.c", line: 4 },
      { name: "main", path: "src/demo.c", line: 11 },
      { name: "ntdll!RtlUserThreadStart", path: null, line: 0 },
    ],
    variables: [
      { name: "a", value: "0", expandable: false },
      { name: "b", value: "1", expandable: false },
      { name: "point", value: "{x = 3, y = 7}", expandable: true },
    ],
    output: ["total=1", "total=3"],
  });

  send = async (command) => {
    if (command.type === "sync") {
      return [
        { type: "state", mode: state.mode, vimMode: state.vim, vimLabel: LABEL[state.vim] },
        splitsEvent(),
        workspacesEvent(),
        treeEvent(),
        viewportEvent(),
        debugEvent(),
        terminalEvent(),
        tasksEvent(),
        pluginsEvent(),
      ];
    }
    if (command.type === "dropTask") {
      queue = queue.filter((t) => t.id !== command.id || t.state === "running");
      return [tasksEvent()];
    }
    if (command.type === "clearTasks") {
      queue = queue.filter((t) => t.state === "running" || t.state === "queued");
      return [tasksEvent()];
    }
    // Staged changes. The stand-in keeps a list so Apply and Refer visibly
    // take a hunk off the panel, which is the interaction worth previewing.
    if (command.type === "review" || command.type === "applyHunk" ||
        command.type === "rejectHunk" || command.type === "applyStaged" ||
        command.type === "rejectStaged") {
      const line = (mark, text) => ({ mark, text });
      staged.files ??= [
        {
          path: "src/routes/dashboard.tsx",
          creates: false,
          hunks: [
            { line: 9, lines: [
              line("context", "const health = useTelemetry(sats, …);"),
              line("removed", "const ranked = [...sats].sort(byHealth);"),
              line("added", "const ranked = useMemo("),
              line("added", "  () => [...sats].sort(byHealth),"),
              line("added", "  [sats, health],"),
              line("added", ");"),
              line("context", "return <Grid rows={ranked} />;"),
            ]},
          ],
        },
        {
          path: "src/lib/orbit.ts",
          creates: false,
          hunks: [
            { line: 54, lines: [
              line("removed", "return { sats: [...store.sats], tick };"),
              line("added", "return { sats: store.sats, tick };  // stable identity"),
            ]},
          ],
        },
      ];
      if (command.type === "applyHunk" || command.type === "rejectHunk") {
        const file = staged.files.find((f) => f.path === command.path);
        if (file) {
          file.hunks.splice(command.hunk, 1);
          if (!file.hunks.length) staged.files = staged.files.filter((f) => f !== file);
        }
      }
      if (command.type === "applyStaged" || command.type === "rejectStaged") staged.files = [];
      return [{ type: "review", files: staged.files }];
    }

    if (command.type === "hover") {
      return [
        {
          type: "hover",
          text:
            "fn magnitude(&self) -> i32\n\n" +
            "How far the point is from the origin, as the crow flies.\n" +
            "Rounded down, because the return type says so.",
          line: doc.first + 4,
          column: 8,
        },
      ];
    }
    if (command.type === "closeHover") return [{ type: "hoverClosed" }];

    if (command.type === "codeActions") {
      const row = (label, detail) => ({
        label, detail, positions: [], path: null, line: 0, column: null, kind: null,
      });
      const items = [
        row("Extract into variable", "refactor.extract"),
        row("Fill match arms", "quickfix"),
        row("Convert to guarded return", "refactor.rewrite"),
        row("Run test", "needs a server command"),
      ];
      return [
        {
          type: "picker",
          kind: "actions",
          title: "code_actions",
          query: "",
          items,
          selected: 0,
          total: items.length,
          elapsedMs: 0.6,
          scanning: false,
        },
      ];
    }

    if (
      command.type === "findReferences" ||
      (command.type === "openPicker" && command.kind === "symbols")
    ) {
      const symbols = command.type !== "findReferences";
      const row = (label, detail, kind) => ({
        label,
        detail,
        positions: [],
        path: "src/lib.rs",
        line: 6,
        column: 4,
        kind: kind ?? null,
      });
      const items = symbols
        ? [
            row("Point", "", "type"),
            row("x", "Point  i32", "field"),
            row("magnitude", "Point  fn(&self) -> i32", "method"),
            row("describe", "fn(Point) -> i32", "function"),
          ]
        : [
            row("pub fn magnitude(&self) -> i32 {", "src/lib.rs:7"),
            row("p.magnitude()", "src/lib.rs:12"),
            row("assert_eq!(p.magnitude(), 7);", "tests/point.rs:9"),
          ];
      return [
        {
          type: "picker",
          kind: symbols ? "symbols" : "references",
          title: symbols ? "document_symbols" : "lsp_references",
          query: "",
          items,
          selected: 0,
          total: items.length,
          elapsedMs: 0.4,
          scanning: false,
        },
      ];
    }

    if (command.type === "split") {
      if (layout.count >= 4) {
        return [{ type: "notice", text: "four splits is as many as are worth having", level: "warn" }];
      }
      layout.vertical = command.vertical;
      layout.scroll.splice(layout.focused + 1, 0, layout.scroll[layout.focused] ?? 0);
      layout.count += 1;
      layout.focused += 1;
      return [splitsEvent(), ...everyViewport()];
    }
    if (command.type === "closeSplit") {
      if (layout.count < 2) {
        return [{ type: "notice", text: "that is the last split", level: "warn" }];
      }
      const index = command.index ?? layout.focused;
      layout.scroll.splice(index, 1);
      layout.count -= 1;
      if (index <= layout.focused) layout.focused = Math.max(0, layout.focused - 1);
      return [splitsEvent(), ...everyViewport()];
    }
    if (command.type === "onlySplit") {
      layout.scroll = [layout.scroll[layout.focused] ?? 0];
      layout.count = 1;
      layout.focused = 0;
      return [splitsEvent(), ...everyViewport()];
    }
    if (command.type === "focusSplit") {
      layout.focused = command.index;
      return [splitsEvent()];
    }
    if (command.type === "stepSplit") {
      const back = ["left", "up", "previous"].includes(command.direction);
      const next = back ? layout.focused - 1 : layout.focused + 1;
      if (next < 0 || next >= layout.count) return [];
      layout.focused = next;
      return [splitsEvent()];
    }
    if (command.type === "openTerminal") return [terminalEvent()];
    if (command.type === "closeTerminal") {
      return [{ type: "terminal", rows: [], cursor: [0, 0], cursorVisible: false, running: false }];
    }
    if (command.type === "terminalInput" || command.type === "terminalSize") return [];
    if (command.type === "focusWorkspace") {
      projects.active = command.index;
      return [workspacesEvent()];
    }
    if (command.type === "toggleTreeNode") {
      if (!open.delete(command.path)) open.add(command.path);
      return [treeEvent()];
    }
    if (command.type === "revealInTree") {
      if (!doc.path) return [{ type: "notice", text: "this buffer is not a file yet", level: "warn" }];
      // Every folder above it, so the row exists to be scrolled to.
      const parts = doc.path.split("/").slice(0, -1);
      parts.reduce((so_far, part) => {
        const path = so_far ? `${so_far}/${part}` : part;
        open.add(path);
        return path;
      }, "");
      return [treeEvent()];
    }
    if (command.type === "viewport") {
      doc.height = command.lines;
      layout.scroll[command.view] = command.firstLine;
      if (command.view === layout.focused) doc.first = command.firstLine;
      return [everyViewport()[command.view]].filter(Boolean);
    }
    if (command.type === "openBuffer") {
      try {
        const res = await fetch("/" + command.path);
        if (!res.ok) throw new Error(res.status);
        doc.lines = (await res.text()).split("\n");
        doc.name = command.path.split("/").pop();
        doc.path = command.path;
      } catch (err) {
        return [{ type: "notice", text: `${command.path}: ${err}`, level: "error" }];
      }
      return [
        { type: "buffers", names: [{ name: doc.name, path: command.path, modified: false }], active: 0 },
        viewportEvent(),
        splitsEvent(),
        { type: "cursor", view: 0, line: 0, column: 0, offset: 0, selection: null },
        // The tree marks where you are, so opening a file redraws it.
        treeEvent(),
      ];
    }
    if (command.type === "setMode") {
      state.mode = command.mode;
      return [{ type: "modeChanged", mode: state.mode }];
    }
    // A real traced step, dumped from the core by
    // `crates/proton-core/tests/dump_step.rs`. Real rather than invented,
    // because invented data is exactly what hides a rendering bug.
    if (command.type === "trace" || command.type === "traceSeek" || command.type === "traceStep") {
      try {
        const step = await fetch("/fixtures/trace-step.json").then((r) => r.json());
        return [step];
      } catch {
        return [{ type: "notice", text: "no trace fixture — run the dump test", level: "warn" }];
      }
    }

    // A canned agent turn, so the design preview can be designed against.
    // Deliberately scripted: the real conversation lives in proton-core and
    // this must never grow into a second one.
    if (command.type === "ask") {
      const you = { speaker: "you", text: command.text, thinking: "", calls: [], streaming: false };
      const reply = {
        speaker: "proton", thinking: "", streaming: true, text: "",
        calls: [{ id: "a", name: "grep", detail: `query=${command.text.split(" ")[0]}`, result: "", state: "running" }],
      };
      const agent = (activity, extra) => ({
        type: "agent", activity, inputTokens: 4210, outputTokens: 183, cachedTokens: 3904,
        transcript: [you, { ...reply, ...extra }],
      });
      // Stagger it so the streaming states are visible while designing them.
      setTimeout(() => deliver(agent("working", {})), 60);
      setTimeout(() => deliver(agent("thinking", {
        thinking: "The grid rebuilds its rows on every tick.",
        calls: [{ ...reply.calls[0], result: "6 hits (+5 more)", state: "done" }],
      })), 700);
      setTimeout(() => deliver(agent("idle", {
        streaming: false,
        thinking: "The grid rebuilds its rows on every tick.",
        calls: [{ ...reply.calls[0], result: "6 hits (+5 more)", state: "done" }],
        text:
          "`ranked` is rebuilt on every tick, so all 40 `Sparkline`s remount.\n\n" +
          "Memoise on `[sats, health]` and the grid stops re-laying out.",
      })), 1400);
      return [];
    }
    if (command.type === "cancelTurn") return [{ type: "agent", activity: "idle", inputTokens: 0, outputTokens: 0, cachedTokens: 0, transcript: [] }];

    if (command.type !== "key") return [];

    const k = command.key.key;
    const before = state.vim;

    if (k === "Escape") {
      state.cmdline = "";
      state.pattern = "";
      state.leader = null;
      state.vim = "Normal";
      // Abandoning a search stops the painting, as the real one does.
      return [{ type: "cmdline", text: null, prompt: state.prompt }, viewportEvent(), vimEvent()];
    }

    if (state.vim === "Command") {
      if (k === "Enter") {
        const text = state.cmdline;
        state.cmdline = "";
        state.vim = "Normal";
        return [{ type: "cmdlineRun", text }, vimEvent()];
      }
      if (k === "Backspace") state.cmdline = state.cmdline.slice(0, -1);
      else if (k.length === 1) state.cmdline += k;
      // Incremental: the pattern being typed is what gets painted.
      if (state.prompt !== ":") state.pattern = state.cmdline;
      return [{ type: "cmdline", text: state.cmdline, prompt: state.prompt }, viewportEvent()];
    }

    if (state.vim === "Insert" || state.vim === "Replace") {
      return k.length === 1 ? [{ type: "input", text: k }] : [];
    }

    if (state.leader !== null) {
      if (k.length === 1) state.leader += k;
      const prefix = state.leader;
      if (prefix.length >= 2) state.leader = null;
      return [{ type: "leader", prefix }];
    }

    if (k === "i" || k === "a" || k === "o") state.vim = "Insert";
    else if (k === "v") state.vim = state.vim === "Visual" ? "Normal" : "Visual";
    else if (k === "V") state.vim = state.vim === "VisualLine" ? "Normal" : "VisualLine";
    else if (k === ":" || k === "/" || k === "?") { state.vim = "Command"; state.cmdline = ""; state.prompt = k; }
    else if (k === " ") { state.leader = ""; return [{ type: "leader", prefix: "" }]; }

    if (state.vim === "Command") return [{ type: "cmdline", text: "", prompt: state.prompt }, vimEvent()];
    return state.vim === before ? [] : [vimEvent()];
  };
}

/* ── dispatch ─────────────────────────────────────────────────────────── */

/** @type {Array<(event: Event) => void>} */
const listeners = [];

/** Subscribe to core events. Returns an unsubscribe function. */
export function onEvent(fn) {
  listeners.push(fn);
  return () => {
    const i = listeners.indexOf(fn);
    if (i >= 0) listeners.splice(i, 1);
  };
}

/** Hand one event to everyone listening. */
function deliver(event) {
  for (const fn of listeners) fn(event);
}

/* Events nobody asked for.
 *
 * `dispatch` returns what a command produced. Language servers do not work
 * that way: a diagnostic appears because rust-analyzer finished thinking,
 * minutes after the keystroke that caused it. Those arrive on their own
 * channel, pushed by the shell's poll thread, and land in the same renderer —
 * which is the point of the events being transport-agnostic. */
if (HAS_SHELL && tauri.event?.listen) {
  tauri.event.listen("proton://events", ({ payload }) => {
    for (const event of payload ?? []) deliver(event);
  });
}

/** @type {(command: Command) => Promise<void>} */
export async function dispatch(command) {
  let events;
  try {
    events = await send(command);
  } catch (err) {
    // A rejected invoke means the core panicked or the command failed to
    // deserialise. Surface it — a silently dropped keystroke is the worst
    // possible failure mode for a modal editor.
    console.error("proton: dispatch failed", command, err);
    return;
  }
  for (const event of events ?? []) deliver(event);
}

/** The bytes of a file the page renders itself — a PDF, so far.
 *
 * Raw rather than base64: the shell answers with `ipc::Response`, which hands
 * the webview an ArrayBuffer, so a large document does not inflate by a third
 * on the way through and does not queue behind the JSON channel.
 */
export async function readDocument(path) {
  if (!HAS_SHELL) throw new Error("no shell: documents need the app");
  const body = await tauri.core.invoke("read_document", { path });
  // Tauri hands back an ArrayBuffer, or a plain array on older transports.
  return body instanceof ArrayBuffer ? new Uint8Array(body) : new Uint8Array(body);
}

export const sendKey = (e) =>
  dispatch({
    type: "key",
    key: { key: e.key, ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, meta: e.metaKey },
  });

export const setMode = (mode) => dispatch({ type: "setMode", mode });
export const sync = () => dispatch({ type: "sync" });

/* ── backdrop ─────────────────────────────────────────────────────────
   Which material the compositor actually gave us. `solid` is a real
   answer, not a failure: Windows turns Mica off when transparency
   effects are disabled or under battery saver, and the page paints its
   own ground in that case. */
export async function applyBackdrop() {
  if (!HAS_SHELL) return "solid";
  let material = "solid";
  try {
    material = await tauri.core.invoke("backdrop");
  } catch (err) {
    console.error("proton: could not read the backdrop", err);
  }
  document.documentElement.dataset.backdrop = material;
  return material;
}

/** Ask for a material. Returns what was actually applied, which may differ. */
export async function setBackdrop(material) {
  if (!HAS_SHELL) return "solid";
  let applied = "solid";
  try {
    applied = await tauri.core.invoke("set_backdrop", { material });
  } catch (err) {
    console.error("proton: could not set the backdrop", err);
  }
  document.documentElement.dataset.backdrop = applied;
  return applied;
}

/* ── window controls ──────────────────────────────────────────────────── */

/* The window is undecorated because the titlebar is drawn in the page, which
   means the traffic lights and dragging are ours to wire. */
export function wireWindowChrome() {
  if (!HAS_SHELL) return;
  const win = tauri.window.getCurrentWindow();

  const lights = document.querySelectorAll(".lights i");
  const actions = [() => win.close(), () => win.minimize(), () => win.toggleMaximize()];
  lights.forEach((light, i) => {
    light.style.cursor = "pointer";
    light.addEventListener("click", () => actions[i]?.());
  });

  // With the titlebar gone, the chrome rail is the drag surface. Only its
  // empty parts drag; every control on it keeps working.
  const rail = document.querySelector(".chrome-rail");
  rail?.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || e.target.closest("button, .lights, .avatars")) return;
    win.startDragging();
  });
  rail?.addEventListener("dblclick", (e) => {
    if (e.target.closest("button, .lights, .avatars")) return;
    win.toggleMaximize();
  });
}
