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
  const state = { mode: "codex", vim: "Normal", cmdline: "", leader: null };
  const LABEL = {
    Normal: "NORMAL", Insert: "INSERT", Visual: "VISUAL",
    VisualLine: "V-LINE", Replace: "REPLACE", Command: "COMMAND",
  };
  const vimEvent = () => ({ type: "vimMode", mode: state.vim, label: LABEL[state.vim] });

  // Read-only file viewing, so the design preview shows real code instead of
  // an empty pane. It cannot edit — the rope lives in Rust — and it must never
  // learn how to.
  const doc = { lines: [""], first: 0, height: 60, name: "[No Name]" };
  const viewportEvent = () => ({
    type: "viewport",
    firstLine: doc.first,
    lines: doc.lines.slice(doc.first, doc.first + doc.height),
    totalLines: doc.lines.length,
    version: 0,
  });

  send = async (command) => {
    if (command.type === "sync") {
      return [{ type: "state", mode: state.mode, vimMode: state.vim, vimLabel: LABEL[state.vim] }];
    }
    if (command.type === "viewport") {
      doc.first = command.firstLine;
      doc.height = command.lines;
      return [viewportEvent()];
    }
    if (command.type === "openBuffer") {
      try {
        const res = await fetch("/" + command.path);
        if (!res.ok) throw new Error(res.status);
        doc.lines = (await res.text()).split("\n");
        doc.name = command.path.split("/").pop();
      } catch (err) {
        return [{ type: "notice", text: `${command.path}: ${err}`, level: "error" }];
      }
      return [
        { type: "buffers", names: [{ name: doc.name, path: command.path, modified: false }], active: 0 },
        viewportEvent(),
        { type: "cursor", line: 0, column: 0, offset: 0, selection: null },
      ];
    }
    if (command.type === "setMode") {
      state.mode = command.mode;
      return [{ type: "modeChanged", mode: state.mode }];
    }
    if (command.type !== "key") return [];

    const k = command.key.key;
    const before = state.vim;

    if (k === "Escape") {
      state.cmdline = "";
      state.leader = null;
      state.vim = "Normal";
      return [{ type: "cmdline", text: null }, vimEvent()];
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
      return [{ type: "cmdline", text: state.cmdline }];
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
    else if (k === ":") { state.vim = "Command"; state.cmdline = ""; }
    else if (k === " ") { state.leader = ""; return [{ type: "leader", prefix: "" }]; }

    if (state.vim === "Command") return [{ type: "cmdline", text: "" }, vimEvent()];
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
  for (const event of events ?? []) {
    for (const fn of listeners) fn(event);
  }
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
