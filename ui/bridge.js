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
  // Each window has a core of its own, and its pushes a channel of their own.
  const label = tauri.window?.getCurrentWindow().label ?? "main";
  return tauri.event.listen(`proton://events/${label}`, ({payload}) => transport.receive(payload));
});
const transport = createTransport({send: command => send(command), ready, ordered: HAS_SHELL});
export const dispatch = transport.dispatch;
export const dispatchMany = transport.dispatchMany;
export const resetTransport = () => transport.reset();
export const onEvents = transport.onEvents;
export const onCommand = transport.onCommand;
/** @param {(event: import('./protocol').WireEvent) => void} listener */
export function onEvent(listener) { return onEvents(events => events.forEach(listener)); }
/** @param {Event} event */
function deliver(event) { transport.deliver([event]); }

/** Current SSH remote status for this window. @returns {Promise<import('./native-api').RemoteStatus>} */
export async function remoteStatus() {
  if (!tauri) {
    return {
      connected: false,
      host: null,
      folder: null,
      remoteHostname: null,
      remoteHome: null,
      error: null,
      forwardedPorts: [],
    };
  }
  return tauri.core.invoke('remote_status');
}

/** Discovered SSH hosts from ~/.ssh/config and recent remote folders. @returns {Promise<import('./native-api').RemoteCatalog>} */
export async function remoteHosts() {
  if (!tauri) {
    return {
      hosts: [{ alias: 'mini', hostName: '100.97.247.34', user: 'razeenwasif', port: 22 }],
      recents: [{ host: 'mini', folder: '~/Github/Applications/Proton' }],
    };
  }
  return tauri.core.invoke('remote_hosts');
}

/** Browse git/dev repos on a remote SSH host.
 *  @param {string} host
 *  @param {string | null} [path]
 *  @returns {Promise<import('./native-api').RemoteDirEntry[]>} */
export async function remoteBrowse(host, path = null) {
  if (!tauri) {
    return [
      { name: 'Applications/Proton', path: '~/Github/Applications/Proton', isRepo: true, hasDevcontainer: true },
      { name: 'Applications/Graviton', path: '~/Github/Applications/Graviton', isRepo: true, hasDevcontainer: false },
    ];
  }
  return tauri.core.invoke('remote_browse', { host, path });
}

/** Connect to an SSH host (in a new window or by replacing this window's session).
 *  @param {string} host
 *  @param {string | null} [folder]
 *  @param {boolean} [newWindow]
 *  @returns {Promise<import('./native-api').RemoteStatus>} */
export async function remoteConnect(host, folder = null, newWindow = true) {
  if (!tauri) throw new Error('Remote-SSH requires the Proton desktop app');
  return tauri.core.invoke('remote_connect', { host, folder, newWindow });
}

/** Disconnect this window from SSH back to a local session.
 *  @returns {Promise<import('./native-api').RemoteStatus>} */
export async function remoteDisconnect() {
  if (!tauri) throw new Error('Remote-SSH requires the Proton desktop app');
  return tauri.core.invoke('remote_disconnect');
}

/** Manually forward a TCP port over the window's active SSH ControlMaster tunnel.
 *  @param {number} localPort
 *  @param {number | null} [remotePort]
 *  @returns {Promise<import('./native-api').RemoteStatus>} */
export async function remoteForwardPort(localPort, remotePort = null) {
  if (!tauri) throw new Error('Remote-SSH requires the Proton desktop app');
  return tauri.core.invoke('remote_forward_port', { localPort, remotePort });
}

/** Subscribe to remote SSH status push updates for this window.
 *  @param {(status: import('./native-api').RemoteStatus) => void} listener */
export function onRemoteStatus(listener) {
  if (!tauri?.event?.listen) return Promise.resolve(() => {});
  const label = tauri.window?.getCurrentWindow().label ?? 'main';
  return tauri.event.listen(`proton://remote-status/${label}`, ({ payload }) => listener(payload));
}

/** Subscribe to native File -> Connect to Host (SSH) menu requests for this window.
 *  @param {() => void} listener */
export function onOpenRemote(listener) {
  if (!tauri?.event?.listen) return Promise.resolve(() => {});
  const label = tauri.window?.getCurrentWindow().label ?? 'main';
  return tauri.event.listen(`proton://open-remote/${label}`, () => listener());
}

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

/** @param {import('./native-api').FullscreenState | null | undefined} state */
export function applyFullscreenState(state) {
  const root = document.documentElement;
  if (state?.active) {
    root.dataset.fullscreen = "true";
  } else {
    delete root.dataset.fullscreen;
  }
  const h = state?.active && Number(state.notchHeight) > 0 ? Number(state.notchHeight) : 0;
  const w = h > 0 && Number(state?.notchWidth) > 0 ? Number(state?.notchWidth) : 0;
  const l = h > 0 && Number(state?.notchLeft) > 0 ? Number(state?.notchLeft) : 0;
  if (h > 0) {
    root.dataset.notch = "true";
  } else {
    delete root.dataset.notch;
  }
  root.style.setProperty("--notch-h", `${h}px`);
  root.style.setProperty("--notch-w", `${w}px`);
  root.style.setProperty("--notch-left", `${l}px`);
}

/** Toggle borderless notch-wrapping full screen on macOS.
 * @returns {Promise<import('./native-api').FullscreenState>} */
export async function toggleFullscreen() {
  if (!tauri) return { active: false, notchHeight: 0, notchWidth: 0, notchLeft: 0 };
  const state = await tauri.core.invoke("toggle_fullscreen");
  applyFullscreenState(state);
  return state;
}

export async function syncFullscreenState() {
  if (!tauri) return;
  try {
    const state = await tauri.core.invoke("fullscreen_state");
    applyFullscreenState(state);
  } catch (err) {
    console.error("proton: could not read fullscreen state", err);
  }
  const label = tauri.window?.getCurrentWindow().label ?? "main";
  await tauri.event?.listen(`proton://fullscreen/${label}`, ({ payload }) => applyFullscreenState(payload));
}

/* The window is undecorated because the titlebar is drawn in the page, which
   means the traffic lights and dragging are ours to wire. */
export function wireWindowChrome() {
  if (!tauri) return;
  const win = tauri.window?.getCurrentWindow();
  if (!win) return;

  // Two sets: the chrome rail's, and Codex's title bar's. Each is close,
  // minimise, full screen (⌥-click to maximise) in that order.
  /** @type {Array<(e: MouseEvent) => void>} */
  const actions = [
    () => { void win.close(); },
    () => { void win.minimize(); },
    (e) => { if (e.altKey) { void win.toggleMaximize(); } else { void toggleFullscreen(); } },
  ];
  document.querySelectorAll(".lights").forEach((set) => {
    set.querySelectorAll("i").forEach((light, i) => {
      if (!(light instanceof HTMLElement)) return;
      light.style.cursor = "pointer";
      light.addEventListener("click", (e) => actions[i]?.(e));
    });
  });

  // With the titlebar gone, the chrome rail is the drag surface — and in
  // Codex, the title bar is. Only their empty parts drag; every control on
  // them keeps working.
  for (const surface of document.querySelectorAll(".chrome-rail, .titlebar")) {
    surface.addEventListener("mousedown", (e) => {
      if (document.documentElement.dataset.fullscreen === "true") return;
      if (!(e instanceof MouseEvent) || !(e.target instanceof Element) || e.button !== 0 ||
        e.target.closest("button, .lights, .avatars")) return;
      win.startDragging();
    });
    surface.addEventListener("dblclick", (e) => {
      if (!(e.target instanceof Element) || e.target.closest("button, .lights, .avatars")) return;
      if (document.documentElement.dataset.fullscreen === "true") {
        void toggleFullscreen();
      } else {
        win.toggleMaximize();
      }
    });
  }

  void syncFullscreenState();
}
