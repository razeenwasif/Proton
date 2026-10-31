/** Native IPC and window integration. Browser fixtures load only in preview.
 * @typedef {import('./protocol').WireCommand} Command
 * @typedef {import('./protocol').WireEvent} Event
 */
import {createTransport, keyCommand} from './transport.js';

/** @type {import('./native-api').NativeApi | undefined} */
const tauri = globalThis.__TAURI__;
export const HAS_SHELL = Boolean(tauri);
const preview = tauri ? null : import('./preview-bridge.js').then(module => module.createPreview(deliver));
/** @param {Command} command */
async function send(command) {
  if (tauri) return tauri.core.invoke('dispatch', {command});
  const adapter = await preview;
  if (!adapter) throw new Error('Browser preview is unavailable');
  return adapter.send(command);
}

/* ── dispatch ─────────────────────────────────────────────────────────── */

// Register before the initial sync so no post-snapshot background events are
// lost. The shell numbers both channels under the same session lock.
const ready = Promise.resolve().then(() => {
  if (!tauri) return;
  if (!tauri.event?.listen) throw new Error("Native event listener is unavailable");
  return tauri.event.listen("proton://events", ({payload}) => transport.receive(payload));
});
const transport = createTransport({send: command => send(command), ready, ordered: HAS_SHELL});
export const dispatch = transport.dispatch;
export const dispatchMany = transport.dispatchMany;
export const onEvents = transport.onEvents;
export const onCommand = transport.onCommand;
/** @param {(event: import('./protocol').WireEvent) => void} listener */
export function onEvent(listener) { return onEvents(events => events.forEach(listener)); }
/** @param {Event} event */
function deliver(event) { transport.deliver([event]); }

/** The bytes of a file the page renders itself — a PDF, so far.
 *
 * Raw rather than base64: the shell answers with `ipc::Response`, which hands
 * the webview an ArrayBuffer, so a large document does not inflate by a third
 * on the way through and does not queue behind the JSON channel.
 */
/** @param {string} path */
export async function readDocument(path) {
  if (!tauri) {
    // The design preview has no shell to read the disk, so it fetches from
    // the server instead — the same arrangement as every other mock in this
    // file, and what lets the viewer be designed against a real document.
    const response = await fetch(path);
    if (!response.ok) throw new Error(`${path}: ${response.status}`);
    return new Uint8Array(await response.arrayBuffer());
  }
  const body = await tauri.core.invoke("read_document", { path });
  // Tauri hands back an ArrayBuffer, or a plain array on older transports.
  return body instanceof ArrayBuffer ? new Uint8Array(body) : new Uint8Array(body);
}

/** Save a copy of a document the viewer shows, where the person chooses:
 *  the system's save panel in the app — the path it went to, or null when
 *  cancelled — and a download in the design preview.
 *  @param {string} path @param {string} name @returns {Promise<string | null>} */
export async function saveDocumentAs(path, name) {
  if (!tauri) {
    const bytes = await readDocument(path);
    const link = Object.assign(document.createElement("a"), { href: URL.createObjectURL(new Blob([bytes], { type: "application/pdf" })), download: name });
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 1000);
    return name;
  }
  return tauri.core.invoke("save_document_as", { path });
}

/** A file a Lab run left, as bytes: `<investigation>/<run>/<path>`.
 *  @param {string} key */
export async function readLab(key) {
  if (!tauri) {
    const adapter = await preview;
    if (!adapter) throw new Error('Browser preview is unavailable');
    return adapter.readLab(key);
  }
  const body = await tauri.core.invoke("read_lab", { key });
  return body instanceof ArrayBuffer ? body : new Uint8Array(body).buffer;
}

/** ⌘V in the editor: the clipboard's text, read by the shell — none if it
 *  holds none, or there is no shell. @returns {Promise<string | null>} */
export async function pasteText() {
  if (!tauri) return null;
  return tauri.core.invoke("paste_text");
}

/** What the editor yanked or deleted, on the system clipboard too: through
 *  the shell, or the page's own clipboard with none. @param {string} text */
export async function copyText(text) {
  if (tauri) return tauri.core.invoke("copy_text", { text });
  await navigator.clipboard?.writeText(text).catch(() => undefined);
}

/** @param {import('./protocol').KeyInput} event */
export const sendKey = event => dispatch(keyCommand(event));

/** @param {Extract<Command, {type: "setMode"}>["mode"]} mode */
export const setMode = (mode) => dispatch({ type: "setMode", mode });
export const sync = () => dispatch({ type: "sync" });

/* ── backdrop ─────────────────────────────────────────────────────────
   Which material the compositor actually gave us. `solid` is a real
   answer, not a failure: Windows turns Mica off when transparency
   effects are disabled or under battery saver, and the page paints its
   own ground in that case. */
export async function applyBackdrop() {
  if (!tauri) return "solid";
  let material = "solid";
  try {
    material = await tauri.core.invoke("backdrop");
  } catch (err) {
    console.error("proton: could not read the backdrop", err);
  }
  document.documentElement.dataset.backdrop = material;
  return material;
}

/** Make the window opaque in `color` (`#rrggbb`), or give it back its
 *  see-through material with `null`. Codex paints every pixel itself, and a
 *  see-through window under it is compositor work nobody sees. */
/** @param {string | null} color */
export async function setOpaque(color) {
  if (!tauri) return;
  try {
    await tauri.core.invoke("set_opaque", { color });
  } catch (err) {
    console.error("proton: could not change the window's ground", err);
  }
}

/** Ask for a material. Returns what was actually applied, which may differ. */
/** @param {string} material */
export async function setBackdrop(material) {
  if (!tauri) return "solid";
  let applied = "solid";
  try {
    applied = await tauri.core.invoke("set_backdrop", { material });
  } catch (err) {
    console.error("proton: could not set the backdrop", err);
  }
  document.documentElement.dataset.backdrop = applied;
  return applied;
}

/** Quit: having saved every file with unsaved changes, when `save` — which
 *  is refused, with which could not be saved and why, if one could not be.
 *  @param {boolean} save */
export async function quitApp(save) {
  if (!tauri) return;
  await tauri.core.invoke("quit", { save });
}

/* ── window controls ──────────────────────────────────────────────────── */

/* The window is undecorated because the titlebar is drawn in the page, which
   means the traffic lights and dragging are ours to wire. */
export function wireWindowChrome() {
  if (!tauri) return;
  const win = tauri.window?.getCurrentWindow();
  if (!win) return;

  // Two sets: the chrome rail's, and Codex's title bar's. Each is close,
  // minimise, maximise in that order, whichever one is on screen.
  const actions = [() => win.close(), () => win.minimize(), () => win.toggleMaximize()];
  document.querySelectorAll(".lights").forEach((set) => {
    set.querySelectorAll("i").forEach((light, i) => {
      if (!(light instanceof HTMLElement)) return;
      light.style.cursor = "pointer";
      light.addEventListener("click", () => actions[i]?.());
    });
  });

  // With the titlebar gone, the chrome rail is the drag surface — and in
  // Codex, the title bar is. Only their empty parts drag; every control on
  // them keeps working.
  for (const surface of document.querySelectorAll(".chrome-rail, .titlebar")) {
    surface.addEventListener("mousedown", (e) => {
      if (!(e instanceof MouseEvent) || !(e.target instanceof Element) || e.button !== 0 ||
        e.target.closest("button, .lights, .avatars")) return;
      win.startDragging();
    });
    surface.addEventListener("dblclick", (e) => {
      if (!(e.target instanceof Element) || e.target.closest("button, .lights, .avatars")) return;
      win.toggleMaximize();
    });
  }
}
