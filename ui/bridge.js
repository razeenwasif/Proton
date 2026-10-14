/** Native IPC and window integration. Browser fixtures load only in preview.
 * @typedef {import('./protocol').WireCommand} Command
 * @typedef {import('./protocol').WireEvent} Event
 */
import {createTransport, keyCommand} from './transport.js';

/** @type {import('./native-api').NativeApi | undefined} */
const tauri = globalThis.__TAURI__;
export const HAS_SHELL = Boolean(tauri);
/** Browser-only delay controls, retained for geometry race checks. @type {Map<string, number>} */
export const mockLatency = new Map();
const preview = tauri ? null : import('./preview-bridge.js').then(module => module.createPreview(deliver, mockLatency));
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

/** One mesh, as bytes — the binary half of an `Event::Geometry`.
 *
 * From the shell through `ipc::Response`, which hands the webview an
 * ArrayBuffer; the page lays typed arrays straight over it. In the preview the
 * stand-in's mesh is encoded into the same layout first, so the decoder is
 * exercised here exactly as it is in the app.
 */
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

/** An image of the open game, as bytes: a path inside its folder.
 *  @param {string} key */
export async function readGame(key) {
  if (!tauri) {
    const adapter = await preview;
    if (!adapter) throw new Error('Browser preview is unavailable');
    return adapter.readGame(key);
  }
  const body = await tauri.core.invoke("read_game", { key });
  return body instanceof ArrayBuffer ? body : new Uint8Array(body).buffer;
}

/** Tiles of one layer of the open canvas, as bytes (see the shell's
 *  `read_raster` for the layout). @param {number} generation @param {number} layer @param {number[]} tiles */
export async function readRaster(generation, layer, tiles) {
  if (!tauri) {
    const adapter = await preview;
    if (!adapter) throw new Error('Browser preview is unavailable');
    return adapter.readRaster(generation, layer, tiles);
  }
  const body = await tauri.core.invoke("read_raster", { generation, layer, tiles });
  return body instanceof ArrayBuffer ? body : new Uint8Array(body).buffer;
}

/** A picture handed to the core — an image dropped on Canvas — as the
 *  request's raw body: its number, to bring in. @param {Uint8Array} bytes @returns {Promise<number>} */
export async function putRasterBlob(bytes) {
  // The browser preview has no core to keep it.
  if (!tauri) throw new Error("bringing a picture in needs the app");
  return tauri.core.invoke("put_raster_blob", bytes);
}

/** ⌘V in Canvas: a picture another app copied, kept by the core — its
 *  number — or none, for Canvas's own copy. @returns {Promise<number | null>} */
export async function pasteRasterImage() {
  if (!tauri) return null;
  return tauri.core.invoke("paste_raster_image");
}

/** Canvas's copy put on the system's clipboard as a PNG. @returns {Promise<boolean>} */
export async function copyRasterImage() {
  if (!tauri) return false;
  return tauri.core.invoke("copy_raster_image");
}

/** @param {string} key */
export async function readGeometry(key) {
  if (!tauri) {
    const adapter = await preview;
    if (!adapter) throw new Error('Browser preview is unavailable');
    return adapter.readGeometry(key);
  }
  const body = await tauri.core.invoke("read_geometry", { key });
  // An ArrayBuffer, or a plain array of bytes on an older transport — which
  // has to be copied into one before anything can be laid over it.
  return body instanceof ArrayBuffer ? body : new Uint8Array(body).buffer;
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
