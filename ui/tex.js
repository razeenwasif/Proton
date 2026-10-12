/* LaTeX's dock: what the source typesets to, the build's log, and the agent.
 *
 * The mode is Codex's section with this pane where Codex's dock was, so the
 * editor beside it is Codex's own — vim, highlighting, the language server.
 * This draws the pane: its three tabs, TeX as the core says it stands, the
 * PDF and the log. The agent's tab is the same conversation as Codex's
 * dock, painted by app.js like every other copy of it. */

import { readDocument } from "./bridge.js";
import { escapeHtml as esc, icon } from "./html.js";
import { createPdfView } from "./pdf-view.js";
import { commandOf, envHtml, readyHtml } from "./tex-env.js";

/** @typedef {import("./bindings/TexEnv").TexEnv} Env */
/** @typedef {import("./bindings/TexBundle").TexBundle} Bundle */
/** @typedef {import("./bindings/TexBuild").TexBuild} Build */
/** @typedef {import("./bindings/TexLog").TexLog} Log */
/** @typedef {import("./bindings/TexLogEntry").TexLogEntry} Entry */
/** @typedef {import("./bindings/TexPackages").TexPackages} Packages */

/** The log's groups, in the order they are shown. */
const GROUPS = /** @type {[Entry["kind"], string][]} */ ([["error", "Errors"], ["warning", "Warnings"], ["box", "Bad boxes"]]);

/** A count of something, said. @param {number} n @param {string} one */
const count = (n, one) => `${n} ${one}${n === 1 ? "" : "s"}`;

/** What a build came to, in a few words. @param {Build} b */
export function buildSaid(b) {
  switch (b.state) {
    case "running": return "Typesetting…";
    case "noMain": return "No file here begins a document";
    case "stopped": return "Stopped";
    case "timedOut": return "Took too long, and was stopped";
    case "failed": return b.errors ? `Not typeset: ${count(b.errors, "error")}` : "Not typeset";
    default: {
      const pages = b.pages ? ` · ${count(b.pages, "page")}` : "";
      const errors = b.errors ? ` · ${count(b.errors, "error")}` : "";
      return `Typeset in ${b.seconds < 1 ? "under a second" : `${b.seconds.toFixed(1)} s`}${pages}${errors}`;
    }
  }
}

/** The pane's tabs, in order. */
export const TABS = /** @type {const} */ (["pdf", "log", "agent"]);

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createTex(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector("#tex-pane"));
  if (!found) return null;
  const pane = found;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (pane.querySelector(sel));
  /** @type {{env: Env | null, bundle: Bundle, confirming: boolean, build: Build | null, log: Log | null, root: string, packages: Packages,
   *   shown: {pdf: string, version: number} | null}} */
  const state = { env: null, bundle: "full", confirming: false, build: null, log: null, root: "", packages: { state: "idle" }, shown: null };
  const pages = q("[data-tex-pages]");
  const pdfView = createPdfView(pages, {
    onWhere: (page, count) => {
      q("[data-tex-page]").textContent = `${page} / ${count}`;
    },
    onPoint: (page, x, y) => host.dispatch({ type: "texSyncInverse", page, x, y }),
  });

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
    const pdf = q("[data-tex-note]");
    const env = state.env;
    const showing = env?.state === "ready" && state.shown !== null;
    pages.hidden = !showing;
    pdf.hidden = showing;
    q("[data-tex-pdf-bar]").hidden = !showing;
    if (showing) return;
    if (env && env.state !== "ready") {
      pdf.innerHTML = `<div class="lab-env tex-env-card"><header>TeX</header>${envHtml(env, state.bundle)}</div>`;
      return;
    }
    const ready = env?.state === "ready" ? readyHtml(env, state.confirming) : "";
    const b = state.build;
    const head = b?.state === "noMain"
      ? `<h3>Which file is the document?</h3><p>No file here begins one with <code>\\documentclass</code>. Open the one that does, or put
         <code>% !TEX root = main.tex</code> at the top of a chapter.</p>`
      : b?.pdf ? `<h3>${esc(buildSaid(b))}</h3><p><code>${esc(b.main ?? "")}</code></p>`
        : `<h3>Nothing typeset yet</h3><p>Open a <code>.tex</code> file from the tree, and its PDF is drawn here each time it is saved.</p>`;
    pdf.innerHTML = `<div class="tex-empty">${icon("tex")}${head}${ready}</div>`;
  }

  /** The header: where the build stands, and what can be done about it. */
  function renderHead() {
    const at = q("[data-tex-build]");
    const b = state.build;
    const running = b?.state === "running";
    const ready = state.env?.state === "ready";
    at.className = `tex-build ${b?.state === "failed" || (b?.errors ?? 0) > 0 ? "bad" : ""}`;
    at.innerHTML = (b ? `${running ? '<span class="lab-spin"></span>' : ""}<span>${esc(buildSaid(b))}</span>` : "") +
      (ready ? (running
        ? `<button class="hbtn" data-tex-stop title="Stop typesetting" aria-label="Stop typesetting">${icon("stop")}</button>`
        : `<button class="hbtn" data-tex-compile title="Typeset now — saving does it too" aria-label="Typeset now">${icon("play")}</button>`) : "");
    const chip = q("[data-tex-log-count]");
    const errors = state.log?.entries.filter((e) => e.kind === "error").length ?? 0;
    const warnings = state.log?.entries.filter((e) => e.kind === "warning").length ?? 0;
    chip.hidden = !errors && !warnings;
    chip.textContent = String(errors || warnings);
    chip.className = `chip ${errors ? "bad" : ""}`;
  }

  /** What a document lacks, and getting it: say-so one, then two. */
  function packagesHtml() {
    const missing = state.log?.missing ?? [];
    const p = state.packages;
    if (p.state === "finding") return `<div class="tex-missing"><p class="lab-busy"><span class="lab-spin"></span> Asking CTAN which packages have ${esc(p.files.join(", "))}…</p></div>`;
    if (p.state === "installing") return `<div class="tex-missing"><p class="lab-busy"><span class="lab-spin"></span> Installing ${esc(p.packages.join(", "))}…</p></div>`;
    if (p.state === "found") {
      const unknown = p.unknown.length ? `<p class="lab-faint">No package has ${esc(p.unknown.join(", "))}.</p>` : "";
      if (!p.packages.length) return `<div class="tex-missing">${unknown}<div class="lab-acts"><button class="cc-quiet" data-tex-packages="forget">Put it away</button></div></div>`;
      return `<div class="tex-missing"><p>${p.packages.map((x) => `<b>${esc(x.name)}</b> has ${esc(x.files.join(", "))}`).join("; ")}.</p>${unknown}
        <div class="lab-acts"><button class="cc-quiet" data-tex-packages="forget">Not now</button>
        <button class="cc-primary" data-tex-packages="install" data-names="${esc(p.packages.map((x) => x.name).join(" "))}">Install ${esc(p.packages.map((x) => x.name).join(", "))}</button></div></div>`;
    }
    if (p.state === "failed") return `<div class="tex-missing"><p class="lab-err">${esc(p.why)}</p><div class="lab-acts"><button class="cc-quiet" data-tex-packages="forget">Put it away</button></div></div>`;
    if (!missing.length) return "";
    return `<div class="tex-missing"><p>This document needs <b>${esc(missing.join(", "))}</b>, which TeX does not have yet.</p>
      <div class="lab-acts"><button class="cc-primary" data-tex-packages="find" data-files="${esc(missing.join(" "))}">Find the package</button></div></div>`;
  }

  /** The log: errors, warnings and bad boxes, each a click from its line. */
  function renderLog() {
    const box = q("[data-tex-log]");
    const log = state.log;
    if (!log) {
      box.innerHTML = `<p class="lab-faint">The log of the last build is read here.</p>`;
      return;
    }
    const groups = GROUPS.map(([kind, name]) => {
      const entries = log.entries.filter((e) => e.kind === kind);
      if (!entries.length) return "";
      const rows = entries.map((e) => {
        const where = e.file ? `${e.file}${e.line ? `:${e.line}` : ""}` : "";
        const jump = e.file && !e.file.startsWith("/") ? ` data-tex-jump="${esc(e.file)}" data-line="${e.line ?? 1}"` : "";
        return `<button class="tex-entry ${kind}"${jump}${jump ? "" : " disabled"}>` +
          `<span class="tex-where">${esc(where)}</span><span class="tex-message">${esc(e.message)}</span>` +
          (e.context ? `<code class="tex-context">${esc(e.context)}</code>` : "") + `</button>`;
      }).join("");
      return `<section class="tex-group"><h4>${esc(name)} <span class="lab-faint">${entries.length}</span></h4>${rows}</section>`;
    }).join("");
    box.innerHTML = packagesHtml() + (groups || `<p class="lab-faint">${icon("check", "lab-ok")} Nothing to say: no errors, warnings or bad boxes.</p>`);
  }

  pane.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const tab = /** @type {HTMLElement | null} */ (t.closest("[data-tex-tab]"));
    if (tab) return show(tab.dataset.texTab ?? "pdf");
    if (t.closest("[data-tex-compile]")) return void host.dispatch({ type: "texCompile" });
    if (t.closest("[data-tex-save-pdf]")) return void host.dispatch({ type: "texSavePdf" });
    if (t.closest("[data-tex-sync]")) return void host.dispatch({ type: "texSyncForward" });
    const zoom = /** @type {HTMLElement | null} */ (t.closest("[data-tex-zoom]"));
    if (zoom) {
      const by = Number(zoom.dataset.texZoom);
      if (by === 0) pdfView.fit();
      else pdfView.zoom(by);
      return;
    }
    if (t.closest("[data-tex-stop]")) return void host.dispatch({ type: "texStop" });
    const jump = /** @type {HTMLElement | null} */ (t.closest("[data-tex-jump]"));
    if (jump) {
      host.dispatch({ type: "jumpTo", mode: "tex", path: jump.dataset.texJump, line: Math.max(0, Number(jump.dataset.line) - 1), column: 0 });
      return;
    }
    const packages = /** @type {HTMLElement | null} */ (t.closest("[data-tex-packages]"));
    if (packages) {
      const what = packages.dataset.texPackages;
      if (what === "find") host.dispatch({ type: "texFindPackages", files: (packages.dataset.files ?? "").split(" ").filter(Boolean) });
      if (what === "install") host.dispatch({ type: "texInstallPackages", packages: (packages.dataset.names ?? "").split(" ").filter(Boolean) });
      if (what === "forget") host.dispatch({ type: "texForgetPackages" });
      return;
    }
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

  // The PDF's own keys while it has the keyboard: nothing reaches the editor.
  pages.addEventListener("keydown", (e) => {
    if (e.metaKey || e.altKey) return;
    if (pdfView.key(e)) {
      e.preventDefault();
      e.stopPropagation();
    }
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
    renderHead();
  }

  /** Where the build stands; a new PDF read and drawn in place.
   *  @param {{build: Build}} event */
  function build(event) {
    state.build = event.build;
    renderHead();
    const b = event.build;
    if (b.pdf && (state.shown?.pdf !== b.pdf || state.shown.version !== b.version)) void draw(b.pdf, b.version);
    else renderPdf();
  }

  /** Read the PDF and draw it: in place, when one is drawn already. @param {string} path @param {number} version */
  async function draw(path, version) {
    const first = state.shown === null || state.shown.pdf !== path;
    state.shown = { pdf: path, version };
    renderPdf();
    let bytes;
    try {
      bytes = await readDocument(path);
    } catch (err) {
      pdfView.note(`Could not read the PDF.<br>${esc(String(err))}`);
      return;
    }
    if (state.shown?.version !== version || state.shown.pdf !== path) return;
    if (first) await pdfView.show(bytes);
    else await pdfView.reload(bytes);
  }

  /** What the last build's log said. @param {{log: Log, root: string}} event */
  function log(event) {
    state.log = event.log;
    state.root = event.root;
    renderLog();
    renderHead();
    // An error is worth seeing at once; a PDF made with warnings is not.
    if (event.log.entries.some((e) => e.kind === "error") || event.log.missing.length) show("log");
  }

  /** Where a line of the source is: the PDF shown, there, marked a moment.
   *  @param {{found: import("./bindings/TexSync").TexSync}} event */
  function sync(event) {
    const { page, rects } = event.found;
    if (!rects.length) return;
    show("pdf");
    // The lines a source line made, as one mark round them all.
    const x = Math.min(...rects.map((r) => r.x)), y = Math.min(...rects.map((r) => r.y));
    const right = Math.max(...rects.map((r) => r.x + r.w)), bottom = Math.max(...rects.map((r) => r.y + r.h));
    pdfView.reveal(page, { x, y, w: right - x, h: bottom - y });
  }

  /** Finding or installing what a document lacks. @param {{packages: Packages}} event */
  function packages(event) {
    state.packages = event.packages;
    renderLog();
  }

  renderPdf();
  renderHead();
  renderLog();
  return { show, env, build, log, packages, sync, pdf: () => pdfView };
}
