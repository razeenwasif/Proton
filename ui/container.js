/* Dev Containers: discovery, template creation, build/attach lifecycle,
 * forwarded ports, and container build logs.
 *
 * Pure rendering helpers are exported so Node tests can verify every phase
 * without a browser DOM. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/ContainerView").ContainerView} ContainerView */
/** @typedef {import("./bindings/ContainerPhase").ContainerPhase} ContainerPhase */
/** @typedef {import("./bindings/ForwardedPortView").ForwardedPortView} ForwardedPortView */
/** @typedef {import("./protocol").Dispatch} Dispatch */

/**
 * Summarize container phase into UI label, tone class, dot flag, and statusline text.
 * @param {ContainerView} state
 * @returns {{ label: string, tone: string, dot: boolean, statusText: string }}
 */
export function statusSummary(state) {
  switch (state.phase) {
    case "absent":
      return { label: "No config", tone: "", dot: false, statusText: "" };
    case "detected":
      return {
        label: "Ready",
        tone: "",
        dot: false,
        statusText: `Container: ${state.name || "Ready"}`,
      };
    case "building":
      return { label: "Building", tone: "busy", dot: true, statusText: "Container: Building…" };
    case "starting":
      return { label: "Starting", tone: "busy", dot: true, statusText: "Container: Starting…" };
    case "running":
      return {
        label: "Running",
        tone: "ok",
        dot: true,
        statusText: `Container: ${state.name || state.containerId || "Running"}`,
      };
    case "stopping":
      return { label: "Stopping", tone: "warn", dot: true, statusText: "Container: Stopping…" };
    case "error":
      return { label: "Error", tone: "err", dot: true, statusText: "Container: Error" };
  }
}

/**
 * Render forwarded ports list with clickable localhost URLs.
 * @param {ForwardedPortView[]} ports
 * @returns {string}
 */
export function renderPortsHtml(ports) {
  if (!ports.length) return "";
  const rows = ports
    .map((p) => {
      const mapping = p.hostPort === p.containerPort
        ? String(p.hostPort)
        : `${p.hostPort} → ${p.containerPort}`;
      const label = p.label ? `<span class="dc-port-label">${esc(p.label)}</span>` : "";
      return `<div class="dc-port-row">` +
        `<button class="dc-port-link" type="button" data-dc-port-url="${esc(p.url)}" title="Open ${esc(p.url)}">${esc(mapping)}/${esc(p.protocol)}</button>` +
        `${label}` +
        `</div>`;
    })
    .join("");
  return `<section class="dc-sec"><div class="dc-sec-head">Forwarded ports</div><div class="dc-ports">${rows}</div></section>`;
}

/**
 * Render the complete Dev Container dock panel HTML.
 * @param {ContainerView} state
 * @param {string} [selectedTemplate]
 * @returns {string}
 */
export function renderContainerHtml(state, selectedTemplate) {
  const sum = statusSummary(state);
  const busy = state.phase === "building" || state.phase === "starting" || state.phase === "stopping";
  const chosen = selectedTemplate || state.suggestedTemplate || state.templates[0]?.id || "base";

  const head = `<header class="dc-head">` +
    `<span class="dc-badge ${esc(sum.tone)}">${esc(sum.label)}</span>` +
    `<span class="dc-title">${esc(state.name || "Dev Container")}</span>` +
    `<button class="hbtn" type="button" data-dc-act="refresh" title="Rescan devcontainer.json" ${busy ? "disabled" : ""}>Refresh</button>` +
    `<button class="hbtn" type="button" data-dc-act="settings" title="Container runtime settings">${icon("gear")}</button>` +
    `</header>`;

  if (state.phase === "absent") {
    const cards = state.templates
      .map((t) => {
        const active = t.id === chosen;
        const suggested = t.id === state.suggestedTemplate
          ? `<span class="chip accent">Suggested</span>`
          : "";
        return `<button class="dc-tpl${active ? " on" : ""}" type="button" data-dc-tpl="${esc(t.id)}" aria-pressed="${active}">` +
          `<div class="dc-tpl-top"><strong>${esc(t.label)}</strong>${suggested}</div>` +
          `<div class="dc-tpl-desc">${esc(t.description)}</div>` +
          `</button>`;
      })
      .join("");
    return `<div class="dc-panel">${head}` +
      `<div class="dc-empty">` +
      `<p class="dc-intro">No <code>devcontainer.json</code> found in this project. Pick a template to run terminals, language servers, debuggers, and agent commands inside an isolated Linux container.</p>` +
      `<div class="dc-templates">${cards}</div>` +
      `<div class="dc-actions"><button class="dc-btn primary" type="button" data-dc-create="${esc(chosen)}">Create .devcontainer/devcontainer.json</button></div>` +
      `</div></div>`;
  }

  let configControl = "";
  if (state.configs.length > 1) {
    const opts = state.configs
      .map((c) => `<option value="${esc(c.path)}"${c.path === state.configPath ? " selected" : ""}>${esc(c.path)}</option>`)
      .join("");
    configControl = `<select class="dc-select" data-dc-config ${state.phase === "running" || busy ? "disabled" : ""}>${opts}</select>`;
  } else if (state.configPath) {
    configControl = `<button class="dc-link" type="button" data-dc-open-config title="Open ${esc(state.configPath)}">${esc(state.configPath)}</button>`;
  }

  let actions = "";
  if (busy) {
    actions = `<div class="dc-actions"><span class="dc-busy">${esc(state.stepNote || sum.label)}</span></div>`;
  } else if (state.phase === "running") {
    actions = `<div class="dc-actions">` +
      `<button class="dc-btn primary" type="button" data-dc-act="terminal">Terminal</button>` +
      `<button class="dc-btn" type="button" data-dc-act="detach">Detach</button>` +
      `<button class="dc-btn" type="button" data-dc-act="rebuild">Rebuild</button>` +
      `<button class="dc-btn danger" type="button" data-dc-act="stop">Stop</button>` +
      `</div>`;
  } else {
    actions = `<div class="dc-actions">` +
      `<button class="dc-btn primary" type="button" data-dc-act="start">Start &amp; Attach</button>` +
      `<button class="dc-btn" type="button" data-dc-act="rebuild">Rebuild</button>` +
      (state.configPath ? `<button class="dc-btn" type="button" data-dc-open-config>Edit config</button>` : "") +
      `</div>`;
  }

  const metaRows = [];
  if (configControl) metaRows.push(`<div class="dc-meta-row"><span>Config</span><span>${configControl}</span></div>`);
  if (state.image) metaRows.push(`<div class="dc-meta-row"><span>Source</span><code>${esc(state.image)}</code></div>`);
  if (state.runtime) metaRows.push(`<div class="dc-meta-row"><span>Runtime</span><code>${esc(state.runtime)}</code></div>`);
  if (state.containerId) metaRows.push(`<div class="dc-meta-row"><span>Container</span><code>${esc(state.containerId)}</code></div>`);
  if (state.workspaceFolder) metaRows.push(`<div class="dc-meta-row"><span>Folder</span><code>${esc(state.workspaceFolder)}</code></div>`);
  if (state.remoteUser) metaRows.push(`<div class="dc-meta-row"><span>User</span><code>${esc(state.remoteUser)}</code></div>`);
  const meta = metaRows.length ? `<div class="dc-meta">${metaRows.join("")}</div>` : "";

  const errBox = state.error
    ? `<div class="dc-err"><div>${esc(state.error)}</div><button class="dc-btn" type="button" data-dc-act="settings">Runtime Settings</button></div>`
    : "";

  const ports = renderPortsHtml(state.ports);

  const logs = state.logs.length
    ? `<section class="dc-sec"><div class="dc-sec-head"><span>Build &amp; hook logs</span><button class="hbtn" type="button" data-dc-act="clearLogs">Clear</button></div><pre class="dc-logs">${esc(state.logs.join("\n"))}</pre></section>`
    : "";

  return `<div class="dc-panel">${head}${actions}${errBox}${meta}${ports}${logs}</div>`;
}

/**
 * Mount Dev Container UI controller.
 * @param {{
 *   root: Document | HTMLElement,
 *   dispatch: Dispatch,
 *   openTerminal: () => void,
 *   openSettings: () => void,
 * }} opts
 */
export function createContainer({ root, dispatch, openTerminal, openSettings }) {
  /** @type {ContainerView | null} */
  let lastState = null;
  /** @type {string | undefined} */
  let selectedTemplate = undefined;
  /** @type {string | null} */
  let lastSuggested = null;

  const view = /** @type {HTMLElement | null} */ (root.querySelector("[data-container-view]"));
  const dockDot = /** @type {HTMLElement | null} */ (root.querySelector("[data-container-dot]"));
  const tbDot = /** @type {HTMLElement | null} */ (root.querySelector("[data-container-tb-dot]"));
  const tbBtn = /** @type {HTMLElement | null} */ (root.querySelector("[data-container-tb]"));
  const termBadge = /** @type {HTMLElement | null} */ (root.querySelector("[data-term-container]"));
  const sl = /** @type {HTMLElement | null} */ (root.querySelector("#sl-container"));

  function paint() {
    if (!lastState) return;
    if (view) {
      view.innerHTML = renderContainerHtml(lastState, selectedTemplate);
      const logPre = view.querySelector(".dc-logs");
      if (logPre) logPre.scrollTop = logPre.scrollHeight;
    }

    const sum = statusSummary(lastState);
    if (dockDot) {
      dockDot.hidden = !sum.dot;
      dockDot.dataset.tone = sum.tone;
    }
    if (tbDot) {
      tbDot.hidden = !sum.dot;
      tbDot.dataset.tone = sum.tone;
    }
    if (tbBtn) {
      tbBtn.classList.toggle("dc-on", lastState.phase === "running");
      tbBtn.title = sum.statusText ? `${sum.statusText}  ␣ku` : "Dev Container  ␣ku";
    }
    if (termBadge) {
      if (lastState.phase === "running") {
        termBadge.hidden = false;
        termBadge.textContent = `in container: ${lastState.name || lastState.containerId || "linux"}`;
      } else {
        termBadge.hidden = true;
      }
    }
    if (sl) {
      if (lastState.phase === "absent") {
        sl.hidden = true;
        sl.innerHTML = "";
        sl.dataset.tone = "";
      } else {
        sl.hidden = false;
        sl.dataset.tone = sum.tone;
        sl.innerHTML = `${icon("container")}<span>${esc(sum.statusText)}</span>`;
      }
    }
  }

  view?.addEventListener("click", (e) => {
    const target = /** @type {Element | null} */ (e.target instanceof Element ? e.target : null);
    if (!target || !lastState) return;

    const tpl = /** @type {HTMLElement | null} */ (target.closest("[data-dc-tpl]"));
    if (tpl) {
      selectedTemplate = tpl.dataset.dcTpl;
      paint();
      return;
    }

    const createBtn = /** @type {HTMLElement | null} */ (target.closest("[data-dc-create]"));
    if (createBtn) {
      const template = selectedTemplate || createBtn.dataset.dcCreate || lastState.suggestedTemplate || "base";
      dispatch({ type: "containerInit", template });
      return;
    }

    if (target.closest("[data-dc-open-config]") && lastState.configPath) {
      dispatch({ type: "openBuffer", path: lastState.configPath });
      return;
    }

    const portBtn = /** @type {HTMLElement | null} */ (target.closest("[data-dc-port-url]"));
    if (portBtn && portBtn.dataset.dcPortUrl) {
      dispatch({ type: "openLink", url: portBtn.dataset.dcPortUrl });
      return;
    }

    const actBtn = /** @type {HTMLElement | null} */ (target.closest("[data-dc-act]"));
    if (!actBtn) return;
    switch (actBtn.dataset.dcAct) {
      case "start":
        dispatch({ type: "containerStart", config: lastState.configPath });
        break;
      case "rebuild":
        dispatch({ type: "containerRebuild", config: lastState.configPath });
        break;
      case "stop":
        dispatch({ type: "containerStop" });
        break;
      case "detach":
        dispatch({ type: "containerDetach" });
        break;
      case "refresh":
        dispatch({ type: "containerRefresh" });
        break;
      case "clearLogs":
        dispatch({ type: "containerClearLogs" });
        break;
      case "terminal":
        openTerminal();
        break;
      case "settings":
        openSettings();
        break;
    }
  });

  view?.addEventListener("change", (e) => {
    const target = /** @type {HTMLSelectElement | null} */ (
      e.target instanceof HTMLSelectElement ? e.target : null
    );
    if (!target || !target.matches("[data-dc-config]")) return;
    dispatch({ type: "containerSelectConfig", path: target.value });
  });

  return {
    /** @param {ContainerView} state */
    update(state) {
      lastState = state;
      if (!selectedTemplate || state.suggestedTemplate !== lastSuggested) {
        selectedTemplate = state.suggestedTemplate;
        lastSuggested = state.suggestedTemplate;
      }
      paint();
    },
    /** @returns {ContainerView | null} */
    current() {
      return lastState;
    },
  };
}
