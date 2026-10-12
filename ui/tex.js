/* LaTeX's dock: what the source typesets to, the build's log, and the agent.
 *
 * The mode is Codex's section with this pane where Codex's dock was, so the
 * editor beside it is Codex's own — vim, highlighting, the language server.
 * This draws the pane: its three tabs, TeX as the core says it stands, the
 * PDF and the log. The agent's tab is the same conversation as Codex's
 * dock, painted by app.js like every other copy of it. */

import { icon } from "./html.js";
import { commandOf, envHtml, readyHtml } from "./tex-env.js";

/** @typedef {import("./bindings/TexEnv").TexEnv} Env */
/** @typedef {import("./bindings/TexBundle").TexBundle} Bundle */

/** The pane's tabs, in order. */
export const TABS = /** @type {const} */ (["pdf", "log", "agent"]);

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createTex(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector("#tex-pane"));
  if (!found) return null;
  const pane = found;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (pane.querySelector(sel));
  /** @type {{env: Env | null, bundle: Bundle, confirming: boolean}} */
  const state = { env: null, bundle: "full", confirming: false };

  /** Show one of the pane's tabs. @param {string} which */
  function show(which) {
    if (!TABS.includes(/** @type {any} */ (which))) return;
    pane.dataset.showing = which;
    for (const tab of pane.querySelectorAll("[data-tex-tab]")) {
      const on = /** @type {HTMLElement} */ (tab).dataset.texTab === which;
      tab.classList.toggle("on", on);
      tab.setAttribute("aria-selected", String(on));
    }
    for (const body of pane.querySelectorAll("[data-tex-body]")) {
      /** @type {HTMLElement} */ (body).hidden = /** @type {HTMLElement} */ (body).dataset.texBody !== which;
    }
    if (which === "agent") {
      q("[data-tex-agent-dot]")?.setAttribute("hidden", "");
      const thread = q("[data-tex-body=agent] [data-agent-thread]");
      if (thread) thread.scrollTop = thread.scrollHeight;
      /** @type {HTMLElement | null} */ (pane.querySelector("[data-tex-body=agent] [data-agent-input]"))?.focus();
    }
  }

  /** The PDF tab: TeX's card until it is installed, then the PDF, or what
   *  stands in for one before there is one. */
  function renderPdf() {
    const pdf = q("[data-tex-pdf]");
    const env = state.env;
    if (env && env.state !== "ready") {
      pdf.innerHTML = `<div class="lab-env tex-env-card"><header>TeX</header>${envHtml(env, state.bundle)}</div>`;
      return;
    }
    const ready = env?.state === "ready" ? readyHtml(env, state.confirming) : "";
    pdf.innerHTML = `<div class="tex-empty">${icon("tex")}<h3>Nothing typeset yet</h3>` +
      `<p>Open a <code>.tex</code> file from the tree, and its PDF is drawn here each time it is saved.</p>${ready}</div>`;
  }

  pane.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const tab = /** @type {HTMLElement | null} */ (t.closest("[data-tex-tab]"));
    if (tab) return show(tab.dataset.texTab ?? "pdf");
    const button = /** @type {HTMLElement | null} */ (t.closest("[data-tex-env]"));
    if (!button) return;
    const what = button.dataset.texEnv ?? "";
    if (what === "confirm" || what === "keep") {
      state.confirming = what === "confirm";
      return renderPdf();
    }
    state.confirming = false;
    const command = commandOf(what, state.bundle);
    if (command) host.dispatch(command);
  });

  pane.addEventListener("change", (e) => {
    const t = /** @type {HTMLInputElement} */ (e.target);
    if (t.matches("[data-tex-bundle]") && t.checked) state.bundle = /** @type {Bundle} */ (t.value);
  });

  /** Where TeX stands. @param {{env: Env}} event */
  function env(event) {
    if (event.env.state !== "ready") state.confirming = false;
    state.env = event.env;
    renderPdf();
  }

  renderPdf();
  return { show, env };
}
