/* LaTeX's dock: what the source typesets to, the build's log, and the agent.
 *
 * The mode is Codex's section with this pane where Codex's dock was, so the
 * editor beside it is Codex's own — vim, highlighting, the language server.
 * This draws the pane: its three tabs, the PDF and the log as the core says
 * they stand. The agent's tab is the same conversation as Codex's dock,
 * painted by app.js like every other copy of it. */

import { icon } from "./html.js";

/** The pane's tabs, in order. */
export const TABS = /** @type {const} */ (["pdf", "log", "agent"]);

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createTex(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector("#tex-pane"));
  if (!found) return null;
  const pane = found;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (pane.querySelector(sel));

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

  /** What the PDF tab shows before there is a PDF. */
  function renderEmpty() {
    const pdf = q("[data-tex-pdf]");
    if (pdf.querySelector(".tex-empty") || pdf.childElementCount) return;
    pdf.innerHTML = `<div class="tex-empty">${icon("tex")}<h3>Nothing typeset yet</h3>` +
      `<p>Open a <code>.tex</code> file from the tree, and its PDF is drawn here each time it is saved.</p></div>`;
  }

  pane.addEventListener("click", (e) => {
    const tab = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-tex-tab]"));
    if (tab) show(tab.dataset.texTab ?? "pdf");
  });

  renderEmpty();
  return { show };
}
