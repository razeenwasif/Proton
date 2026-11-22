/* Remote-SSH: connect to remote hosts (e.g. mini) over SSH NDJSON,
 * browse remote git repos, manage SSH port forwards, and surface active
 * host status in the titlebar and statusline. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./native-api").RemoteStatus} RemoteStatus */
/** @typedef {import("./native-api").RemoteCatalog} RemoteCatalog */
/** @typedef {import("./native-api").RemoteDirEntry} RemoteDirEntry */
/** @typedef {import("./protocol").Dispatch} Dispatch */

/**
 * Parse `:ssh ...` or `:remote ...` Ex command strings into a structured action.
 * @param {string} raw
 * @returns {null | {
 *   kind: "open" | "connect" | "disconnect" | "forward",
 *   host?: string,
 *   folder?: string | null,
 *   newWindow?: boolean,
 *   localPort?: number,
 *   remotePort?: number,
 *   error?: string,
 * }}
 */
export function parseSshCommand(raw) {
  const trimmed = raw.trim();
  let rest = "";
  if (trimmed === "ssh" || trimmed === "remote") {
    return { kind: "open" };
  } else if (trimmed.startsWith("ssh ")) {
    rest = trimmed.slice(4).trim();
  } else if (trimmed.startsWith("remote ")) {
    rest = trimmed.slice(7).trim();
  } else {
    return null;
  }

  if (!rest) return { kind: "open" };
  if (rest === "local" || rest === "disconnect" || rest === "off" || rest === "close") {
    return { kind: "disconnect" };
  }
  if (rest.startsWith("forward ")) {
    const spec = rest.slice("forward ".length).trim();
    const parts = spec.split(":");
    const parsePort = (s) => (/^\d+$/.test(s) ? Number(s) : NaN);
    const localPort = parts.length === 1 ? parsePort(parts[0]) : parsePort(parts[0]);
    const remotePort = parts.length === 1 ? localPort : parsePort(parts[1]);
    if (
      parts.length > 2 ||
      !Number.isInteger(localPort) ||
      !Number.isInteger(remotePort) ||
      localPort < 1 ||
      localPort > 65535 ||
      remotePort < 1 ||
      remotePort > 65535
    ) {
      return { kind: "forward", error: "usage: :ssh forward <localPort[:remotePort]>" };
    }
    return { kind: "forward", localPort, remotePort };
  }

  let newWindow = false;
  const tokens = rest.split(/\s+/).filter(Boolean);
  while (tokens.length && (tokens[0] === "-w" || tokens[0] === "--new-window")) {
    newWindow = true;
    tokens.shift();
  }
  if (!tokens.length) return { kind: "open" };

  let targetHost = tokens[0];
  /** @type {string | null} */
  let folder = tokens.slice(1).join(" ") || null;
  if (!folder && targetHost.includes(":")) {
    const colon = targetHost.indexOf(":");
    const h = targetHost.slice(0, colon).trim();
    const f = targetHost.slice(colon + 1).trim();
    if (h) {
      targetHost = h;
      folder = f || null;
    }
  }
  return { kind: "connect", host: targetHost, folder, newWindow };
}

/**
 * Summarize RemoteStatus for titlebar button & statusline pill.
 * @param {RemoteStatus | null | undefined} status
 * @returns {{ connected: boolean, label: string, title: string, tone: string }}
 */
export function remoteSummary(status) {
  if (!status || !status.connected || !status.host) {
    if (status?.error) {
      return {
        connected: false,
        label: "SSH disconnected",
        title: `Remote SSH error: ${status.error}`,
        tone: "err",
      };
    }
    return {
      connected: false,
      label: "Local",
      title: "Connect to SSH host (⌘⇧O · :ssh mini)",
      tone: "",
    };
  }
  const folder = status.folder ? ` — ${status.folder}` : "";
  return {
    connected: true,
    label: `SSH: ${status.host}`,
    title: `Connected to ${status.host}${status.remoteHostname ? ` (${status.remoteHostname})` : ""}${folder}`,
    tone: "ok",
  };
}

/**
 * Render the Connect-to-Host (SSH) modal body.
 * @param {{
 *   status: RemoteStatus,
 *   catalog: RemoteCatalog,
 *   dirs: RemoteDirEntry[],
 *   draftHost: string,
 *   draftFolder: string,
 *   browsing: boolean,
 *   busy: boolean,
 *   error: string | null,
 * }} state
 * @returns {string}
 */
export function renderRemoteModalHtml(state) {
  const { status, catalog, dirs, draftHost, draftFolder, browsing, busy, error } = state;

  const activeBanner = status.connected && status.host
    ? `<section class="rm-sec rm-active">` +
      `<div class="rm-active-top">` +
      `<span class="dc-badge ok">Connected</span>` +
      `<strong>${esc(status.host)}</strong>` +
      (status.remoteHostname ? `<code class="rm-host-detail">${esc(status.remoteHostname)}</code>` : "") +
      `<button class="dc-btn danger" type="button" data-rm-act="disconnect" ${busy ? "disabled" : ""}>Disconnect (Local)</button>` +
      `</div>` +
      (status.folder ? `<div class="rm-path"><code>${esc(status.folder)}</code></div>` : "") +
      `<div class="rm-tunnels">` +
      (status.forwardedPorts.length
        ? status.forwardedPorts
            .map((t) =>
              `<span class="chip">${t.localPort === t.remotePort ? esc(String(t.localPort)) : `${esc(String(t.localPort))}→${esc(String(t.remotePort))}`}${t.auto ? " auto" : ""}</span>`
            )
            .join(" ")
        : `<span class="rm-muted">No active forwarded ports (attached container ports forward automatically).</span>`) +
      `</div>` +
      `<form class="rm-fwd-form" data-rm-fwd>` +
      `<input type="text" data-rm-fwd-input placeholder="Forward port: 8080 or 8080:3000" spellcheck="false" autocomplete="off">` +
      `<button class="dc-btn" type="submit" ${busy ? "disabled" : ""}>Forward</button>` +
      `</form>` +
      `</section>`
    : "";

  const hostsHtml = catalog.hosts.length
    ? catalog.hosts
        .map((h) => {
          const active = h.alias === draftHost;
          const detail = [h.user ? `${h.user}@` : "", h.hostName || ""].join("");
          return `<button class="rm-host${active ? " on" : ""}" type="button" data-rm-host="${esc(h.alias)}">` +
            `<strong>${esc(h.alias)}</strong>` +
            (detail ? `<span>${esc(detail)}</span>` : "") +
            `</button>`;
        })
        .join("")
    : `<span class="rm-muted">No entries in <code>~/.ssh/config</code>. Type a host or <code>user@host</code> below.</span>`;

  const recentsHtml = catalog.recents.length
    ? `<section class="rm-sec">` +
      `<div class="dc-sec-head">Recent remote workspaces</div>` +
      `<div class="rm-list">${catalog.recents
        .slice(0, 8)
        .map((r) =>
          `<button class="rm-dir" type="button" data-rm-recent-host="${esc(r.host)}" data-rm-recent-folder="${esc(r.folder)}">` +
          `<span class="chip">${esc(r.host)}</span><code>${esc(r.folder)}</code>` +
          `</button>`
        )
        .join("")}</div></section>`
    : "";

  let browseListHtml = "";
  if (browsing) {
    browseListHtml = `<div class="rm-muted">Loading directories on ${esc(draftHost || "host")}…</div>`;
  } else if (dirs.length) {
    browseListHtml = `<div class="rm-list rm-dirs">${dirs
      .map((d) =>
        `<button class="rm-dir${d.path === draftFolder ? " on" : ""}" type="button" data-rm-dir="${esc(d.path)}" title="${esc(d.path)}">` +
        `<span class="rm-dir-name">${esc(d.name)}</span>` +
        `<span class="rm-dir-tags">${d.isRepo ? '<span class="chip">git</span>' : ""}${d.hasDevcontainer ? '<span class="chip accent">devcontainer</span>' : ""}</span>` +
        `</button>`
      )
      .join("")}</div>`;
  }

  const errHtml = error
    ? `<div class="dc-err">${esc(error)}</div>`
    : "";

  return `<header class="rm-head">` +
    `<span class="rm-brand">${icon("remote")} <strong>Connect via SSH</strong></span>` +
    `<span class="rm-sub">Run workspaces, terminals, LSP, Jupyter kernels &amp; Graviton containers remotely</span>` +
    `<button class="btn" type="button" data-rm-act="close">Close ⎋</button>` +
    `</header>` +
    `<div class="rm-body">` +
    `${errHtml}${activeBanner}` +
    `<section class="rm-sec">` +
    `<div class="dc-sec-head">SSH hosts (~/.ssh/config)</div>` +
    `<div class="rm-hosts">${hostsHtml}</div>` +
    `</section>` +
    `<section class="rm-sec">` +
    `<div class="rm-fields">` +
    `<label class="st-field">Host<input type="text" data-rm-host-input value="${esc(draftHost)}" placeholder="mini" spellcheck="false" autocomplete="off"></label>` +
    `<label class="st-field">Remote folder<input type="text" data-rm-folder-input value="${esc(draftFolder)}" placeholder="~/Github/Applications/Proton" spellcheck="false" autocomplete="off"></label>` +
    `<button class="dc-btn" type="button" data-rm-act="browse" ${busy || !draftHost.trim() ? "disabled" : ""}>Browse repos</button>` +
    `</div>` +
    `${browseListHtml}` +
    `</section>` +
    `${recentsHtml}` +
    `<div class="rm-footer">` +
    `<span class="rm-muted">Tip: <code>:ssh mini</code> or <code>⌘⇧O</code> · Remote Graviton containers use <code>DOCKER_HOST</code> on that host</span>` +
    `<div class="rm-footer-btns">` +
    `<button class="dc-btn" type="button" data-rm-act="connect-here" ${busy || !draftHost.trim() ? "disabled" : ""}>${busy ? "Connecting…" : "Switch This Window"}</button>` +
    `<button class="dc-btn primary" type="button" data-rm-act="connect-window" ${busy || !draftHost.trim() ? "disabled" : ""}>${busy ? "Connecting…" : "Open in New Window"}</button>` +
    `</div>` +
    `</div>` +
    `</div>`;
}

/**
 * Mount the Remote-SSH UI controller.
 * @param {{
 *   root: Document,
 *   hasShell: boolean,
 *   openOverlay: () => Promise<void> | void,
 *   closeOverlay: () => Promise<void> | void,
 *   resetTransport: () => void,
 *   dispatch: Dispatch,
 *   remoteStatus: () => Promise<RemoteStatus>,
 *   remoteHosts: () => Promise<RemoteCatalog>,
 *   remoteBrowse: (host: string, path?: string | null) => Promise<RemoteDirEntry[]>,
 *   remoteConnect: (host: string, folder?: string | null, newWindow?: boolean) => Promise<RemoteStatus>,
 *   remoteDisconnect: () => Promise<RemoteStatus>,
 *   remoteForwardPort: (localPort: number, remotePort?: number | null) => Promise<RemoteStatus>,
 *   onRemoteStatus: (cb: (status: RemoteStatus) => void) => Promise<unknown>,
 *   onOpenRemote: (cb: () => void) => Promise<unknown>,
 *   flashMessage?: (text: string, level?: string) => void,
 * }} opts
 */
export function createRemote(opts) {
  const {
    root,
    hasShell,
    openOverlay,
    closeOverlay,
    resetTransport,
    dispatch,
    remoteStatus,
    remoteHosts,
    remoteBrowse,
    remoteConnect,
    remoteDisconnect,
    remoteForwardPort,
    onRemoteStatus,
    onOpenRemote,
    flashMessage,
  } = opts;

  /** @type {{ status: RemoteStatus, catalog: RemoteCatalog, dirs: RemoteDirEntry[], draftHost: string, draftFolder: string, browsing: boolean, busy: boolean, error: string | null }} */
  const state = {
    status: {
      connected: false,
      host: null,
      folder: null,
      remoteHostname: null,
      remoteHome: null,
      error: null,
      forwardedPorts: [],
    },
    catalog: { hosts: [], recents: [] },
    dirs: [],
    draftHost: "",
    draftFolder: "~/Github",
    browsing: false,
    busy: false,
    error: null,
  };

  const modal = /** @type {HTMLElement | null} */ (root.querySelector("#remote-modal"));
  const tbBtn = /** @type {HTMLElement | null} */ (root.querySelector("[data-remote-tb]"));
  const sl = /** @type {HTMLElement | null} */ (root.querySelector("#sl-remote"));

  function paintChrome() {
    const sum = remoteSummary(state.status);
    if (tbBtn) {
      tbBtn.classList.toggle("rm-on", sum.connected);
      tbBtn.dataset.tone = sum.tone;
      tbBtn.title = sum.title;
      tbBtn.setAttribute("aria-label", sum.label);
    }
    if (sl) {
      sl.dataset.tone = sum.tone;
      sl.classList.toggle("on", sum.connected);
      sl.title = sum.title;
      sl.innerHTML = `${icon("remote")}<span>${esc(sum.label)}</span>`;
    }
  }

  function paintModal() {
    if (modal) {
      modal.innerHTML = renderRemoteModalHtml(state);
    }
    paintChrome();
  }

  async function refresh() {
    if (!hasShell) {
      paintChrome();
      return;
    }
    try {
      state.status = await remoteStatus();
    } catch {
      /* ignore during boot */
    }
    paintChrome();
  }

  /** @param {string} host @param {string | null} [path] */
  async function browse(host, path = null) {
    const clean = host.trim();
    if (!clean) return;
    state.browsing = true;
    state.error = null;
    paintModal();
    try {
      state.dirs = await remoteBrowse(clean, path);
    } catch (err) {
      state.dirs = [];
      state.error = String(err);
    } finally {
      state.browsing = false;
      paintModal();
    }
  }

  async function openModal() {
    await openOverlay();
    state.error = null;
    paintModal();
    try {
      const [status, catalog] = await Promise.all([remoteStatus(), remoteHosts()]);
      state.status = status;
      state.catalog = catalog;
      if (!state.draftHost) {
        const preferred =
          catalog.hosts.find((h) => h.alias === "mini")?.alias ||
          catalog.recents[0]?.host ||
          catalog.hosts[0]?.alias ||
          "mini";
        state.draftHost = preferred;
      }
      if (status.connected && status.folder) {
        state.draftHost = status.host || state.draftHost;
        state.draftFolder = status.folder;
      } else if (catalog.recents[0]?.host === state.draftHost && catalog.recents[0]?.folder) {
        state.draftFolder = catalog.recents[0].folder;
      }
      paintModal();
      if (state.draftHost && !state.dirs.length && !state.browsing) {
        void browse(state.draftHost, null);
      }
    } catch (err) {
      state.error = String(err);
      paintModal();
    }
  }

  /** @param {string} host @param {string | null} folder @param {boolean} newWindow */
  async function connect(host, folder = null, newWindow = true) {
    const clean = host.trim();
    if (!clean) {
      state.error = "Enter an SSH host alias (e.g. mini).";
      paintModal();
      return;
    }
    state.busy = true;
    state.error = null;
    paintModal();
    try {
      const cleanFolder = folder && folder.trim() ? folder.trim() : null;
      const status = await remoteConnect(clean, cleanFolder, newWindow);
      if (newWindow) {
        flashMessage?.(`Opened SSH window on ${clean}`, "info");
        await closeOverlay();
      } else {
        state.status = status;
        resetTransport();
        await closeOverlay();
        await dispatch({ type: "sync" });
        flashMessage?.(`Connected to ${clean}${status.folder ? ` (${status.folder})` : ""}`, "info");
      }
    } catch (err) {
      state.error = String(err);
      flashMessage?.(String(err), "err");
    } finally {
      state.busy = false;
      paintModal();
    }
  }

  async function disconnect() {
    state.busy = true;
    state.error = null;
    paintModal();
    try {
      state.status = await remoteDisconnect();
      resetTransport();
      await closeOverlay();
      await dispatch({ type: "sync" });
      flashMessage?.("Returned to local Proton workspace", "info");
    } catch (err) {
      state.error = String(err);
      flashMessage?.(String(err), "err");
    } finally {
      state.busy = false;
      paintModal();
    }
  }

  /** @param {string} cmdText */
  async function runEx(cmdText) {
    const parsed = parseSshCommand(cmdText);
    if (!parsed) return false;
    if (parsed.kind === "open") {
      await openModal();
      return true;
    }
    if (parsed.kind === "disconnect") {
      await disconnect();
      return true;
    }
    if (parsed.kind === "forward") {
      if (parsed.error || parsed.localPort == null || parsed.remotePort == null) {
        flashMessage?.(parsed.error || "invalid port forward specification", "warn");
        return true;
      }
      try {
        state.status = await remoteForwardPort(parsed.localPort, parsed.remotePort);
        paintChrome();
        flashMessage?.(`Forwarding localhost:${parsed.localPort} -> remote:${parsed.remotePort}`, "info");
      } catch (err) {
        flashMessage?.(String(err), "err");
      }
      return true;
    }
    if (parsed.kind === "connect" && parsed.host) {
      await connect(parsed.host, parsed.folder ?? null, Boolean(parsed.newWindow));
      return true;
    }
    return false;
  }

  tbBtn?.addEventListener("click", () => { void openModal(); });
  sl?.addEventListener("click", () => { void openModal(); });
  sl?.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      void openModal();
    }
  });

  modal?.addEventListener("input", (e) => {
    const t = /** @type {HTMLInputElement | null} */ (e.target instanceof HTMLInputElement ? e.target : null);
    if (!t) return;
    if (t.matches("[data-rm-host-input]")) {
      state.draftHost = t.value;
    } else if (t.matches("[data-rm-folder-input]")) {
      state.draftFolder = t.value;
    }
  });

  modal?.addEventListener("click", (e) => {
    const target = /** @type {Element | null} */ (e.target instanceof Element ? e.target : null);
    if (!target) return;

    const hostBtn = /** @type {HTMLElement | null} */ (target.closest("[data-rm-host]"));
    if (hostBtn && hostBtn.dataset.rmHost) {
      state.draftHost = hostBtn.dataset.rmHost;
      void browse(state.draftHost, null);
      return;
    }

    const dirBtn = /** @type {HTMLElement | null} */ (target.closest("[data-rm-dir]"));
    if (dirBtn && dirBtn.dataset.rmDir) {
      state.draftFolder = dirBtn.dataset.rmDir;
      paintModal();
      return;
    }

    const recentBtn = /** @type {HTMLElement | null} */ (target.closest("[data-rm-recent-host]"));
    if (recentBtn && recentBtn.dataset.rmRecentHost) {
      state.draftHost = recentBtn.dataset.rmRecentHost;
      state.draftFolder = recentBtn.dataset.rmRecentFolder || "~/Github";
      paintModal();
      return;
    }

    const actBtn = /** @type {HTMLElement | null} */ (target.closest("[data-rm-act]"));
    if (!actBtn) return;
    switch (actBtn.dataset.rmAct) {
      case "close":
        void closeOverlay();
        break;
      case "browse":
        void browse(state.draftHost, null);
        break;
      case "connect-window":
        void connect(state.draftHost, state.draftFolder, true);
        break;
      case "connect-here":
        void connect(state.draftHost, state.draftFolder, false);
        break;
      case "disconnect":
        void disconnect();
        break;
    }
  });

  modal?.addEventListener("dblclick", (e) => {
    const dirBtn = /** @type {HTMLElement | null} */ (
      e.target instanceof Element ? e.target.closest("[data-rm-dir]") : null
    );
    if (dirBtn?.dataset.rmDir && state.draftHost.trim()) {
      void connect(state.draftHost, dirBtn.dataset.rmDir, true);
    }
  });

  modal?.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement | null} */ (
      e.target instanceof HTMLFormElement ? e.target : null
    );
    if (!form || !form.matches("[data-rm-fwd]")) return;
    e.preventDefault();
    const input = /** @type {HTMLInputElement | null} */ (form.querySelector("[data-rm-fwd-input]"));
    const val = input?.value?.trim() || "";
    if (!val) return;
    void runEx(`ssh forward ${val}`);
  });

  void onRemoteStatus((status) => {
    state.status = status;
    paintModal();
    if (!status.connected && status.error) {
      flashMessage?.(status.error, "err");
    }
  });

  void onOpenRemote(() => {
    void openModal();
  });

  return {
    openModal,
    refresh,
    runEx,
    status: () => state.status,
  };
}
