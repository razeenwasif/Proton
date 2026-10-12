/* The PDF mode's tools panel: what is done to the pages — the one being
 * read, or the ones chosen in the grid — and new PDFs made of this one and
 * others: pages taken out, split, combined. Every change is the core's; this
 * gathers what it needs to be asked. */

import { escapeHtml as esc } from "./html.js";

/** @typedef {import("./bindings/PdfTab").PdfTab} Tab */
/** @typedef {import("./bindings/PdfInfo").PdfInfo} Info */

/** A size, said. @param {number} bytes */
export function size(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * @param {{box: HTMLElement, dispatch: (command: any) => unknown, tab: () => Tab | null,
 *   pages: () => number[], count: () => number, project: () => string[], extra?: () => string}} host
 *   `pages`: the pages the tools act on — those chosen in the grid, or the one being read
 */
export function createPdfTools(host) {
  const { box } = host;
  /** What is typed, kept across drawing again. */
  const typed = { insert: "", take: "", every: "", at: "", into: "" };
  /** The PDFs to combine, in order, the open one first. @type {string[]} */
  let combining = [];
  let adding = "";
  /** The tab the list and the names typed are for: another tab starts afresh. */
  let forTab = /** @type {number | null} */ (null);
  /** What it says of itself, how large it is, and whether it has a password. */
  let about = /** @type {{info: Info, protected: boolean, onDisk: number, now: number} | null} */ (null);
  /** Its properties as typed, not yet set. */
  let props = /** @type {Record<string, string> | null} */ (null);
  /** Passwords being typed: held here only until they are asked for, never kept. */
  const secret = { user: "", owner: "", print: true, copy: true, edit: false };

  /** Beside the open file, named after it: `~/Papers/paper-pages.pdf`. @param {string} suffix */
  function beside(suffix) {
    const path = host.tab()?.path ?? "";
    return path ? path.replace(/\.pdf$/i, "") + suffix : "";
  }

  /** The pages the tools act on, said. */
  function which() {
    const pages = host.pages();
    if (!pages.length) return "no page";
    return pages.length === 1 ? `page ${pages[0]}` : `${pages.length} pages`;
  }

  function render() {
    const tab = host.tab();
    if (!tab) {
      box.innerHTML = `<p class="lab-faint">Open a PDF to use its tools.</p>`;
      return;
    }
    if (tab.readOnly) {
      box.innerHTML = `<p class="lab-faint">This PDF can only be read: ${esc(tab.readOnly)}.</p>`;
      return;
    }
    const path = tab.path;
    if (forTab !== tab.id) {
      forTab = tab.id;
      combining = [];
      props = null;
      Object.assign(typed, { insert: "", take: "", every: "", at: "", into: "" });
      Object.assign(secret, { user: "", owner: "" });
    }
    if (!combining.length || combining[0] !== path) combining = [path, ...combining.filter((p) => p !== path)];
    const list = combining.map((p, i) => `<li><span title="${esc(p)}">${esc(p.split("/").pop() ?? p)}</span>` +
      (i ? `<button class="hbtn" data-pdf-tool="up" data-at="${i}" title="Earlier"${i === 1 ? " disabled" : ""}>↑</button>` +
        `<button class="hbtn" data-pdf-tool="down" data-at="${i}" title="Later"${i === combining.length - 1 ? " disabled" : ""}>↓</button>` +
        `<button class="hbtn" data-pdf-tool="drop" data-at="${i}" title="Leave out">×</button>` : `<span class="lab-faint">this one</span>`) + `</li>`).join("");
    const suggestions = host.project().filter((p) => !combining.some((c) => c.endsWith(p))).slice(0, 30);
    const info = about?.info;
    const prop = (/** @type {string} */ key, /** @type {string} */ label) =>
      `<label class="pdf-tool-field">${label} <input data-pdf-prop="${key}" value="${esc(props?.[key] ?? (info ? String(/** @type {any} */ (info)[key] ?? "") : ""))}" spellcheck="false"></label>`;
    const security = about?.protected
      ? `<p>Saved with a password. Whoever opens it is asked for it.</p>
         <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="unprotect">Remove the password</button></div>`
      : `<label class="pdf-tool-field">Password to open it <input type="password" data-pdf-secret="user" autocomplete="new-password"></label>
         <label class="pdf-tool-field">Password to change what it allows <input type="password" data-pdf-secret="owner" autocomplete="new-password" placeholder="the same, if left empty"></label>
         <div class="pdf-tool-checks">
           <label><input type="checkbox" data-pdf-allow="print"${secret.print ? " checked" : ""}> Printing</label>
           <label><input type="checkbox" data-pdf-allow="copy"${secret.copy ? " checked" : ""}> Copying</label>
           <label><input type="checkbox" data-pdf-allow="edit"${secret.edit ? " checked" : ""}> Changing</label>
         </div>
         <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="protect">Protect it</button></div>
         <p class="lab-faint">AES-256. It is written whole when saved, so nothing of it is left readable in the file.</p>`;
    const hidden = info?.hidden ?? [];
    box.innerHTML = (host.extra?.() ?? "") + `
      <section class="pdf-tool"><h4>Properties <span class="lab-faint">${info ? `PDF ${esc(info.version)} · ${info.pages} pages` : ""}</span></h4>
        ${prop("title", "Title")}${prop("author", "Author")}${prop("subject", "Subject")}${prop("keywords", "Keywords")}
        ${info && (info.creator || info.producer) ? `<p class="lab-faint">Made with ${esc([info.creator, info.producer].filter(Boolean).join(", "))}</p>` : ""}
        <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="props">Set the properties</button></div>
      </section>
      <section class="pdf-tool"><h4>Security</h4>${security}</section>
      <section class="pdf-tool"><h4>Clean up</h4>
        <p class="lab-faint">${hidden.length ? `It carries ${esc(hidden.join(", "))}.` : "It carries nothing hidden that Proton looks for."}</p>
        <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="sanitize"${hidden.length ? "" : " disabled"}>Remove hidden data</button></div>
        <p class="lab-faint" data-pdf-sizes>${about ? `On disk ${size(about.onDisk)}${about.now !== about.onDisk ? ` · as it would be saved ${size(about.now)}` : ""}` : ""}</p>
        <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="compress">Compress</button></div>
      </section>` + `
      <section class="pdf-tool"><h4>Pages <span class="lab-faint" data-pdf-which>${esc(which())}</span></h4>
        <div class="pdf-tool-row">
          <button class="cc-quiet" data-pdf-tool="left" title="Turn left">⟲ Turn left</button>
          <button class="cc-quiet" data-pdf-tool="right" title="Turn right">⟳ Turn right</button>
        </div>
        <div class="pdf-tool-row">
          <button class="cc-quiet" data-pdf-tool="blank">Blank page after</button>
          <button class="cc-quiet" data-pdf-tool="delete">Delete</button>
        </div>
        <label class="pdf-tool-field">Insert after it, from <input data-pdf-typed="insert" value="${esc(typed.insert)}" placeholder="~/Papers/other.pdf" spellcheck="false"></label>
        <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="insert">Insert its pages</button></div>
        <label class="pdf-tool-field">Take them out into <input data-pdf-typed="take" value="${esc(typed.take || beside("-pages.pdf"))}" spellcheck="false"></label>
        <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="take">Make the PDF</button></div>
      </section>
      <section class="pdf-tool"><h4>Split</h4>
        <label class="pdf-tool-field">Every <input data-pdf-typed="every" value="${esc(typed.every)}" inputmode="numeric" placeholder="10" class="pdf-tool-short"> pages, or at pages <input data-pdf-typed="at" value="${esc(typed.at)}" placeholder="5, 12" class="pdf-tool-short"></label>
        <p class="lab-faint">Written beside it: ${esc((path.split("/").pop() ?? "").replace(/\.pdf$/i, ""))}-1.pdf and on.</p>
        <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="split">Split</button></div>
      </section>
      <section class="pdf-tool"><h4>Combine</h4>
        <ol class="pdf-combine">${list}</ol>
        <label class="pdf-tool-field">Add <input data-pdf-typed="adding" value="${esc(adding)}" list="pdf-combine-suggest" placeholder="a PDF's path" spellcheck="false"></label>
        <datalist id="pdf-combine-suggest">${suggestions.map((p) => `<option value="${esc(p)}"></option>`).join("")}</datalist>
        <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-tool="add">Add</button></div>
        <label class="pdf-tool-field">Into <input data-pdf-typed="into" value="${esc(typed.into || beside("-combined.pdf"))}" spellcheck="false"></label>
        <div class="pdf-tool-row"><button class="cc-primary" data-pdf-tool="combine"${combining.length < 2 ? " disabled" : ""}>Combine ${combining.length}</button></div>
      </section>`;
  }

  /** The pages to act on, refreshed where it is said. */
  function says() {
    const at = box.querySelector("[data-pdf-which]");
    if (at) at.textContent = which();
  }

  /** Page numbers typed — "5, 12" — as numbers. @param {string} text */
  const numbers = (text) => text.split(/[\s,]+/).map(Number).filter((n) => Number.isInteger(n) && n > 0);

  box.addEventListener("input", (e) => {
    const t = /** @type {HTMLInputElement} */ (e.target);
    if (t.dataset.pdfProp) {
      props = props ?? {};
      props[t.dataset.pdfProp] = t.value;
      return;
    }
    if (t.dataset.pdfSecret === "user" || t.dataset.pdfSecret === "owner") {
      secret[t.dataset.pdfSecret] = t.value;
      return;
    }
    const name = t.dataset.pdfTyped;
    if (name === "adding") adding = t.value;
    else if (name && name in typed) typed[/** @type {keyof typeof typed} */ (name)] = t.value;
  });

  box.addEventListener("click", (e) => {
    const button = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-pdf-tool]"));
    if (!button) return;
    const pages = host.pages();
    const at = Number(button.dataset.at);
    const field = (/** @type {string} */ name) => /** @type {HTMLInputElement | null} */ (box.querySelector(`[data-pdf-typed="${name}"]`))?.value.trim() ?? "";
    switch (button.dataset.pdfTool) {
      case "left": case "right":
        if (pages.length) host.dispatch({ type: "pdfRotate", pages, by: button.dataset.pdfTool === "left" ? -90 : 90 });
        return;
      case "blank":
        if (pages.length) host.dispatch({ type: "pdfInsertBlank", after: pages[pages.length - 1], count: 1 });
        return;
      case "delete":
        if (pages.length) host.dispatch({ type: "pdfDelete", pages });
        return;
      case "insert":
        if (field("insert")) host.dispatch({ type: "pdfInsertFile", after: pages[pages.length - 1] ?? host.count(), path: field("insert") });
        return;
      case "take":
        if (pages.length && field("take")) host.dispatch({ type: "pdfExtract", pages, path: field("take") });
        return;
      case "split": {
        const every = numbers(field("every"))[0];
        host.dispatch({ type: "pdfSplit", every: every ?? null, at: every ? [] : numbers(field("at")) });
        return;
      }
      case "add":
        if (adding.trim() && !combining.includes(adding.trim())) combining.push(adding.trim());
        adding = "";
        return render();
      case "up": case "down": {
        const to = button.dataset.pdfTool === "up" ? at - 1 : at + 1;
        if (to < 1 || to >= combining.length) return;
        [combining[at], combining[to]] = [combining[to], combining[at]];
        return render();
      }
      case "drop":
        combining.splice(at, 1);
        return render();
      case "combine":
        if (combining.length > 1 && field("into")) host.dispatch({ type: "pdfCombine", paths: [...combining], path: field("into") });
        return;
      case "props": {
        const value = (/** @type {string} */ key) => /** @type {HTMLInputElement | null} */ (box.querySelector(`[data-pdf-prop="${key}"]`))?.value ?? "";
        host.dispatch({ type: "pdfSetInfo", title: value("title"), author: value("author"), subject: value("subject"), keywords: value("keywords") });
        props = null;
        return;
      }
      case "protect": {
        if (!secret.user && !secret.owner) return;
        host.dispatch({ type: "pdfProtect", protection: { user: secret.user, owner: secret.owner, print: secret.print, copy: secret.copy, edit: secret.edit } });
        Object.assign(secret, { user: "", owner: "" });
        return;
      }
      case "unprotect":
        host.dispatch({ type: "pdfUnprotect" });
        return;
      case "sanitize":
        host.dispatch({ type: "pdfSanitize" });
        return;
      case "compress":
        host.dispatch({ type: "pdfCompress" });
        return;
    }
  });

  box.addEventListener("change", (e) => {
    const t = /** @type {HTMLInputElement} */ (e.target);
    const allow = t.dataset.pdfAllow;
    if (allow === "print" || allow === "copy" || allow === "edit") secret[allow] = t.checked;
  });

  /** What the tab on top says of itself. @param {{tab: number, info: Info, protected: boolean, onDisk: number, now: number}} event */
  function setAbout(event) {
    about = { info: event.info, protected: event.protected, onDisk: Number(event.onDisk), now: Number(event.now) };
    render();
  }

  return { render, says, setAbout };
}
