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
  const state = { mode: "codex", vim: "Normal", pending: "" };
  const LABEL = {
    Normal: "NORMAL", Insert: "INSERT", Visual: "VISUAL",
    VisualLine: "V-LINE", Replace: "REPLACE", Command: "COMMAND",
  };
  const vimEvent = () => ({ type: "vimMode", mode: state.vim, label: LABEL[state.vim] });

  send = async (command) => {
    if (command.type === "sync") {
      return [{ type: "state", mode: state.mode, vimMode: state.vim, vimLabel: LABEL[state.vim] }];
    }
    if (command.type === "setMode") {
      state.mode = command.mode;
      return [{ type: "modeChanged", mode: state.mode }];
    }
    if (command.type !== "key") return [];

    const k = command.key.key;
    const before = state.vim;
    if (k === "Escape") state.vim = "Normal";
    else if (state.vim === "Insert" || state.vim === "Replace") {
      return k.length === 1 ? [{ type: "input", text: k }] : [];
    } else if (k === "i" || k === "a" || k === "o") state.vim = "Insert";
    else if (k === "v") state.vim = state.vim === "Visual" ? "Normal" : "Visual";
    else if (k === "V") state.vim = state.vim === "VisualLine" ? "Normal" : "VisualLine";
    else if (k === ":") state.vim = "Command";
    else if (k === " ") return [{ type: "leader", prefix: "" }];

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

  const bar = document.querySelector(".titlebar");
  bar?.addEventListener("mousedown", (e) => {
    // Only the empty parts of the bar drag the window; controls keep working.
    if (e.button !== 0 || e.target.closest("button, .lights, .modes, .crumb")) return;
    win.startDragging();
  });
  bar?.addEventListener("dblclick", (e) => {
    if (e.target.closest("button, .lights, .modes")) return;
    win.toggleMaximize();
  });
}
