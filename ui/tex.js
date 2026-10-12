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
/** @typedef {import("./bindings/TexOutline").TexOutline} Outline */
/** @typedef {import("./bindings/TexTemplate").TexTemplate} Template */
/** @typedef {import("./bindings/TexCitation").TexCitation} Citation */

/** Where new projects were last made, kept in this window's storage. */
const PARENT = "proton.tex.parent";
const FIRST_PARENT = "~/Documents/LaTeX";

/** The folder new projects go in, as last typed. */
function lastParent() {
  try {
    return localStorage.getItem(PARENT) || FIRST_PARENT;
  } catch {
    return FIRST_PARENT;
  }
}

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
   *   shown: {pdf: string, version: number} | null, templates: Template[], gallery: boolean, chosen: string | null, name: string, parent: string,
   *   investigations: [string, string][], cite: {open: boolean, query: string, citations: Citation[], searching: boolean, note: string | null, at: number}}} */
  const state = { env: null, bundle: "full", confirming: false, build: null, log: null, root: "", packages: { state: "idle" }, shown: null,
    templates: [], gallery: false, chosen: null, name: "", parent: lastParent(),
    investigations: [], cite: { open: false, query: "", citations: [], searching: false, note: null, at: 0 } };
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
    pdf.hidden = showing || state.gallery;
    q("[data-tex-pdf-bar]").hidden = !showing;
    // The templates: under the empty state once TeX is in, or over
    // everything while + has them open.
    const gallery = q("[data-tex-gallery]");
    gallery.hidden = !state.templates.length || !(state.gallery || (env?.state === "ready" && !showing && !state.build?.pdf));
    gallery.classList.toggle("over", state.gallery);
    if (showing || state.gallery) return;
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

  /** The templates, and once one is chosen, where its project goes. Drawn
   *  only when they change, so a build's news does not take the keyboard
   *  from the name being typed. */
  function renderGallery() {
    const chosen = state.templates.find((t) => t.id === state.chosen);
    const cards = state.templates.map((t) =>
      `<button type="button" class="tex-template${t.id === state.chosen ? " on" : ""}" data-tex-template="${esc(t.id)}" aria-pressed="${t.id === state.chosen}">` +
      `<b>${esc(t.name)}</b><span>${esc(t.about)}</span><span class="lab-faint">${esc(t.files.join(" · "))}</span></button>`).join("");
    const form = chosen ? `<form class="tex-new" data-tex-new-form>
        <label>Name <input data-tex-new-name value="${esc(state.name)}" placeholder="My paper" spellcheck="false" autocomplete="off"></label>
        <label>In the folder <input data-tex-new-parent value="${esc(state.parent)}" spellcheck="false" autocomplete="off"></label>
        <div class="lab-acts"><button type="button" class="cc-quiet" data-tex-add title="The ${esc(chosen.name)} template's files, added to the project open now">Add to this project</button>
        <button type="submit" class="cc-primary">Make the project</button></div></form>` : "";
    const close = state.gallery ? `<span class="grow"></span><button type="button" class="hbtn" data-tex-gallery-close title="Back" aria-label="Close the templates">${icon("close")}</button>` : "";
    // Research's investigations, each a paper to be: gathered here, written by the agent if asked.
    const writeup = state.investigations.length ? `<section class="tex-writeup"><h4>Or write up an investigation</h4>
        <select data-tex-writeup-slug aria-label="The investigation">${state.investigations.map(([slug, title]) => `<option value="${esc(slug)}">${esc(title)}</option>`).join("")}</select>
        <button type="button" class="cc-primary" data-tex-writeup title="Gathered into paper/ in this project: its hypotheses, runs, figures, verdicts and literature">Write it up</button>
        <label><input type="checkbox" data-tex-writeup-draft> Then ask the agent to write its prose — the edits wait for you in the Agent tab</label></section>` : "";
    q("[data-tex-gallery]").innerHTML = `<div class="tex-gallery-in"><header><h4>Start from a template</h4>${close}</header><div class="tex-templates">${cards}</div>${form}${writeup}</div>`;
  }

  /** The papers the cite picker found, the chosen one ringed. */
  function renderCite() {
    const c = state.cite;
    q("[data-tex-cite]").hidden = !c.open;
    if (!c.open) return;
    const list = q("[data-tex-cite-list]");
    list.innerHTML = c.citations.length ? c.citations.map((x, i) =>
      `<button type="button" class="tex-citation${i === c.at ? " on" : ""}" role="option" aria-selected="${i === c.at}" data-tex-cite-pick="${i}">` +
      `<b>${esc(x.title || "(untitled)")}</b><code>${esc(x.key)}</code>` +
      `<span class="lab-faint">${esc([x.authors, x.year, x.venue].filter(Boolean).join(" · "))}${x.from ? ` — ${esc(x.from)}` : ""}</span></button>`).join("")
      : `<p class="lab-faint">${c.query ? "Nothing here matches." : "Nothing to cite yet: no .bib entries or Research papers."}</p>`;
    q("[data-tex-cite-note]").innerHTML = c.searching ? `<span class="lab-spin"></span> Asking arXiv, Semantic Scholar and OpenAlex…` : esc(c.note ?? "");
    /** @type {HTMLButtonElement} */ (q("[data-tex-cite-online]")).disabled = c.searching || !c.query.trim();
  }

  /** Open or close the cite picker. @param {boolean} open */
  function cite(open) {
    state.cite.open = open;
    if (open) {
      gallery(false);
      host.dispatch({ type: "texCiteSearch", query: state.cite.query });
    }
    renderCite();
    if (open) /** @type {HTMLInputElement} */ (q("[data-tex-cite-query]")).focus();
  }

  /** Cite the paper at `i`, and put the picker away. @param {number} i */
  function citeAt(i) {
    const x = state.cite.citations[i];
    if (!x) return;
    host.dispatch({ type: "texCite", source: x.source, id: x.id });
    cite(false);
  }

  /** Open or close the templates. @param {boolean} open */
  function gallery(open) {
    state.gallery = open;
    if (!open) state.chosen = null;
    if (open && state.cite.open) {
      state.cite.open = false;
      renderCite();
    }
    renderGallery();
    renderPdf();
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
        const entry = `<button class="tex-entry ${kind}"${jump}${jump ? "" : " disabled"}>` +
          `<span class="tex-where">${esc(where)}</span><span class="tex-message">${esc(e.message)}</span>` +
          (e.context ? `<code class="tex-context">${esc(e.context)}</code>` : "") + `</button>`;
        // An error is the agent's to fix, with the log in front of it.
        const fix = kind === "error" ? `<button class="tex-fix" data-tex-fix="${log.entries.indexOf(e)}" title="Ask the agent to fix this — its edits wait for you in the Agent tab">Fix</button>` : "";
        return `<div class="tex-row">${entry}${fix}</div>`;
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
    if (t.closest("[data-tex-cite-open]")) return cite(!state.cite.open);
    if (t.closest("[data-tex-cite-close]")) return cite(false);
    if (t.closest("[data-tex-cite-online]")) return void host.dispatch({ type: "texCiteOnline" });
    const pick = /** @type {HTMLElement | null} */ (t.closest("[data-tex-cite-pick]"));
    if (pick) return citeAt(Number(pick.dataset.texCitePick));
    if (t.closest("[data-tex-writeup]")) {
      const slug = /** @type {HTMLSelectElement | null} */ (pane.querySelector("[data-tex-writeup-slug]"))?.value;
      if (!slug) return;
      host.dispatch({ type: "texWriteUp", slug });
      if (/** @type {HTMLInputElement | null} */ (pane.querySelector("[data-tex-writeup-draft]"))?.checked) {
        const title = state.investigations.find(([s]) => s === slug)?.[1] ?? slug;
        show("agent");
        host.dispatch({ type: "ask", text: `Write the prose of the paper just written up in paper/ from the investigation "${title}". It was gathered from the investigation's files: ` +
          "make it read as a paper — an abstract, an introduction that motivates it, a method, results and a discussion — keeping every \\cite, \\ref, label and figure. Read main.tex first, and use latex_log if it does not typeset." });
      }
      return gallery(false);
    }
    if (t.closest("[data-tex-new]")) return gallery(!state.gallery);
    if (t.closest("[data-tex-gallery-close]")) return gallery(false);
    const template = /** @type {HTMLElement | null} */ (t.closest("[data-tex-template]"));
    if (template) {
      state.chosen = template.dataset.texTemplate ?? null;
      renderGallery();
      /** @type {HTMLInputElement | null} */ (pane.querySelector("[data-tex-new-name]"))?.focus();
      return;
    }
    if (t.closest("[data-tex-add]") && state.chosen) {
      host.dispatch({ type: "texAddTemplate", template: state.chosen });
      return gallery(false);
    }
    const fix = /** @type {HTMLElement | null} */ (t.closest("[data-tex-fix]"));
    if (fix) {
      const e = state.log?.entries[Number(fix.dataset.texFix)];
      if (!e) return;
      const where = e.file ? ` in ${e.file}${e.line ? ` at line ${e.line}` : ""}` : "";
      show("agent");
      host.dispatch({ type: "ask", text: `Fix this LaTeX error${where}: ${e.message} @log` });
      return;
    }
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

  // The picker's keys: up and down choose, Enter cites, Escape puts it away.
  q("[data-tex-cite]").addEventListener("keydown", (e) => {
    const c = state.cite;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      c.at = Math.max(0, Math.min(c.citations.length - 1, c.at + (e.key === "ArrowDown" ? 1 : -1)));
      renderCite();
    } else if (e.key === "Enter") {
      citeAt(c.at);
    } else if (e.key === "Escape") {
      cite(false);
    } else {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
  });

  // The PDF's own keys while it has the keyboard: nothing reaches the editor.
  pages.addEventListener("keydown", (e) => {
    if (e.metaKey || e.altKey) return;
    if (pdfView.key(e)) {
      e.preventDefault();
      e.stopPropagation();
    }
  });

  // What is typed is kept, for the gallery drawn again.
  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let asking;
  pane.addEventListener("input", (e) => {
    const t = /** @type {HTMLInputElement} */ (e.target);
    if (t.matches("[data-tex-cite-query]")) {
      state.cite.query = t.value;
      state.cite.at = 0;
      clearTimeout(asking);
      asking = setTimeout(() => host.dispatch({ type: "texCiteSearch", query: state.cite.query }), 120);
      return;
    }
    if (t.matches("[data-tex-new-name]")) state.name = t.value;
    if (t.matches("[data-tex-new-parent]")) state.parent = t.value;
  });

  pane.addEventListener("submit", (e) => {
    const form = /** @type {HTMLElement} */ (e.target);
    if (!form.matches("[data-tex-new-form]")) return;
    e.preventDefault();
    if (!state.chosen) return;
    const name = state.name.trim();
    if (!name) {
      /** @type {HTMLInputElement | null} */ (form.querySelector("[data-tex-new-name]"))?.focus();
      return;
    }
    try {
      localStorage.setItem(PARENT, state.parent.trim());
    } catch {
      // Not kept: the next one starts from the folder first offered.
    }
    host.dispatch({ type: "texNew", template: state.chosen, parent: state.parent.trim() || FIRST_PARENT, name });
    state.name = "";
    gallery(false);
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

  /** One element in another. @param {HTMLElement} box @param {string} sel */
  const q2 = (box, sel) => /** @type {HTMLElement} */ (box.querySelector(sel));

  /** What a new project can start from. @param {{templates: Template[], investigations: [string, string][]}} event */
  function templates(event) {
    state.templates = event.templates;
    state.investigations = event.investigations ?? [];
    renderGallery();
    renderPdf();
  }

  /** What the cite picker found. @param {{query: string, citations: Citation[], searching: boolean, note: string | null}} event */
  function citations(event) {
    // An answer to a query since typed over is not drawn.
    if (event.query.trim() !== state.cite.query.trim()) return;
    state.cite.citations = event.citations;
    state.cite.searching = event.searching;
    state.cite.note = event.note;
    state.cite.at = Math.min(state.cite.at, Math.max(0, event.citations.length - 1));
    renderCite();
  }

  /** The document's headings and its words, under the file tree, each
   *  heading a click from its line. @param {{main: string | null, outline: Outline}} event */
  function outline(event) {
    const box = /** @type {HTMLElement | null} */ (host.root.querySelector("[data-tex-outline]"));
    if (!box) return;
    box.hidden = !event.main;
    const { headings, words } = event.outline;
    q2(box, "[data-tex-words]").textContent = event.main ? `${words.toLocaleString("en")} word${words === 1 ? "" : "s"}` : "";
    // Stepped in from the shallowest heading there is: a paper's sections
    // start at the edge, as a book's chapters do.
    const top = Math.min(...headings.map((h) => h.level));
    q2(box, "[data-tex-headings]").innerHTML = headings.length
      ? headings.map((h) => `<button class="tex-heading${h.level === top ? " top" : ""}" style="--depth:${h.level - top}" data-tex-jump="${esc(h.file)}" data-line="${h.line}"` +
          ` title="${esc(`${h.file}:${h.line}${h.label ? ` · ${h.label}` : ""}`)}">${esc(h.title || "(untitled)")}</button>`).join("")
      : `<p class="lab-faint">No sections yet.</p>`;
  }

  /** Finding or installing what a document lacks. @param {{packages: Packages}} event */
  function packages(event) {
    state.packages = event.packages;
    renderLog();
  }

  // A heading in the outline goes to its line, as an entry in the log does.
  host.root.querySelector("[data-tex-outline]")?.addEventListener("click", (e) => {
    const jump = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-tex-jump]"));
    if (jump) host.dispatch({ type: "jumpTo", mode: "tex", path: jump.dataset.texJump, line: Math.max(0, Number(jump.dataset.line) - 1), column: 0 });
  });

  renderPdf();
  renderHead();
  renderLog();
  return { show, env, build, log, packages, sync, outline, templates, citations, pdf: () => pdfView };
}
