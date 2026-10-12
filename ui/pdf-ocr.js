/* The PDF mode's OCR: pages that are pictures — scans, with no words to
 * find or select — read by tesseract through the core, and what it read laid
 * over them, unseen, as one change to undo. This finds the pages with no
 * text, offers to read them, and says how far the reading has got. */

import { escapeHtml as esc } from "./html.js";

/** What tesseract's language codes are called, for the common ones. */
const NAMES = /** @type {Record<string, string>} */ ({
  eng: "English", fra: "French", deu: "German", spa: "Spanish", ita: "Italian", por: "Portuguese",
  nld: "Dutch", swe: "Swedish", dan: "Danish", nor: "Norwegian", fin: "Finnish", pol: "Polish",
  ces: "Czech", rus: "Russian", ukr: "Ukrainian", ell: "Greek", tur: "Turkish", ara: "Arabic",
  heb: "Hebrew", hin: "Hindi", ben: "Bengali", urd: "Urdu", fas: "Persian", tha: "Thai",
  vie: "Vietnamese", ind: "Indonesian", jpn: "Japanese", kor: "Korean",
  chi_sim: "Chinese (simplified)", chi_tra: "Chinese (traditional)", lat: "Latin", equ: "Equations",
});

/** A page's text is looked for in this many pages at most. */
const LOOK_AT = 3000;

/** The languages chosen before, if any were. @returns {string[]} */
function chosenBefore() {
  try {
    const kept = JSON.parse(localStorage.getItem("proton.pdf.ocrLangs") ?? "[]");
    return Array.isArray(kept) ? kept.filter((l) => typeof l === "string") : [];
  } catch {
    return [];
  }
}

/** @param {string[]} langs */
function keepChosen(langs) {
  try {
    localStorage.setItem("proton.pdf.ocrLangs", JSON.stringify(langs));
  } catch {
    // Not kept: English, or the first there is, next time.
  }
}

/**
 * @param {{dispatch: (command: any) => unknown, editable: () => boolean, box: HTMLElement, offer: HTMLElement,
 *   page: () => number, pages: () => number[], count: () => number, onChange: () => void}} host
 *   `pages`: the pages the tools act on — those chosen in the grid, or the one being read;
 *   `offer`: the bar's chip, shown on a page with no text
 */
export function createPdfOcr(host) {
  /** @type {{langs: string[] | null, asked: boolean, chosen: string[], which: "these" | "blank" | "all",
   *   blank: number[] | null, job: {state: string, done: number, total: number, said: string} | null}} */
  const state = { langs: null, asked: false, chosen: chosenBefore(), which: "blank", blank: null, job: null };
  /** Which look is the current one, so one overtaken stops. */
  let generation = 0;

  const running = () => state.job?.state === "running";

  /** The pages with no text in `doc`, looked for page by page; the
   *  languages asked for the first time there is a PDF. @param {any} doc */
  async function look(doc) {
    const mine = ++generation;
    state.blank = null;
    offer();
    if (!doc) return;
    if (!state.asked) {
      state.asked = true;
      host.dispatch({ type: "pdfOcrLangs" });
    }
    /** @type {number[]} */
    const blank = [];
    for (let n = 1; n <= Math.min(doc.numPages, LOOK_AT); n++) {
      try {
        const content = await (await doc.getPage(n)).getTextContent();
        if (mine !== generation) return;
        if (!content.items.some((/** @type {any} */ i) => i.str?.trim())) blank.push(n);
      } catch {
        // A document closed under it, or a page that cannot be read: left out.
        if (mine !== generation) return;
      }
    }
    state.blank = blank;
    if (state.which === "blank" && !blank.length) state.which = "these";
    offer();
    host.onChange();
  }

  /** The languages tesseract reads here. @param {{langs: string[]}} event */
  function langs(event) {
    state.langs = event.langs;
    state.chosen = state.chosen.filter((l) => event.langs.includes(l));
    if (!state.chosen.length && event.langs.length) state.chosen = [event.langs.includes("eng") ? "eng" : event.langs[0]];
    offer();
    host.onChange();
  }

  /** How far the reading has got, or how it ended. @param {{tab: number, state: string, done: number, total: number, said: string}} event */
  function progress(event) {
    const was = running();
    state.job = { state: event.state, done: event.done, total: event.total, said: event.said };
    offer();
    const bar = /** @type {HTMLProgressElement | null} */ (host.box.querySelector("[data-pdf-ocr-progress]"));
    if (was && running() && bar) {
      bar.max = Math.max(1, event.total);
      bar.value = event.done;
      const said = host.box.querySelector("[data-pdf-ocr-said]");
      if (said) said.textContent = reading();
      return;
    }
    host.onChange();
  }

  /** How far it has got, said. */
  function reading() {
    const job = state.job;
    if (!job) return "";
    return job.done >= job.total ? "Laying the words over the pages…" : `Reading page ${job.done + 1} of ${job.total}…`;
  }

  /** The pages a choice reads. @param {string} which */
  function pagesFor(which) {
    if (which === "blank") return state.blank ?? [];
    if (which === "all") return Array.from({ length: host.count() }, (_, i) => i + 1);
    return host.pages();
  }

  /** Read `pages` in the languages chosen. @param {number[]} pages */
  function start(pages) {
    if (!pages.length || running() || !host.editable()) return;
    host.dispatch({ type: "pdfOcr", pages, langs: [...state.chosen] });
  }

  /** The bar's chip: on a page with no text, an offer to read it; while
   *  reading, how far it has got. */
  function offer() {
    const chip = host.offer;
    const job = state.job;
    if (job && running()) {
      chip.hidden = false;
      chip.textContent = `OCR ${Math.min(job.done + 1, job.total)}/${job.total}`;
      chip.title = "Reading pages with OCR — Stop is in Tools";
      chip.dataset.pdfOcrOffer = "running";
      return;
    }
    const page = host.page();
    const blank = Boolean(state.blank?.includes(page)) && host.editable();
    chip.hidden = !blank;
    if (!blank) return;
    const missing = state.langs !== null && !state.langs.length;
    chip.dataset.pdfOcrOffer = missing ? "missing" : "offer";
    chip.textContent = missing ? "No text" : "Make searchable";
    const others = (state.blank?.length ?? 1) - 1;
    chip.title = missing
      ? "This page is a picture, with no text to find or select. OCR could read it, with tesseract and Poppler's pdftoppm: brew install tesseract poppler"
      : `This page is a picture, with no text to find or select: read it${others ? ` and the ${others} other page${others === 1 ? "" : "s"} like it` : ""} with OCR, the words laid over it unseen`;
  }

  /** The chip clicked: the pages with no text read, and the section shown. */
  function offered() {
    if (host.offer.dataset.pdfOcrOffer === "offer" && state.langs?.length) start(state.blank ?? []);
    host.box.querySelector("[data-pdf-ocr]")?.scrollIntoView({ block: "nearest" });
  }

  /** The tools panel's OCR section. */
  function toolsHtml() {
    const blank = state.blank;
    const about = blank === null ? "looking for pages with no text…"
      : !blank.length ? "every page has text"
      : blank.length === 1 ? `page ${blank[0]} has no text`
      : `${blank.length} pages have no text`;
    const head = `<h4>OCR <span class="lab-faint">${esc(about)}</span></h4>`;
    const job = state.job;
    if (state.langs === null) return `<section class="pdf-tool" data-pdf-ocr>${head}<p class="lab-faint">Asking tesseract what it reads…</p></section>`;
    if (!state.langs.length || job?.state === "missing") {
      return `<section class="pdf-tool" data-pdf-ocr>${head}
        <p>Reading pictures of pages needs tesseract and Poppler's pdftoppm, which are not on this Mac.</p>
        <p class="lab-faint">brew install tesseract poppler</p></section>`;
    }
    if (job && running()) {
      return `<section class="pdf-tool" data-pdf-ocr>${head}
        <p data-pdf-ocr-said>${esc(reading())}</p>
        <progress class="pdf-ocr-progress" data-pdf-ocr-progress max="${Math.max(1, job.total)}" value="${job.done}"></progress>
        <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-ocr-do="stop">Stop</button></div></section>`;
    }
    const ended = !job ? ""
      : job.state === "done" ? `Read ${job.total} page${job.total === 1 ? "" : "s"}: their words can be found, selected and copied now. Undo takes them away.`
      : job.state === "stopped" ? "Stopped: nothing was changed."
      : job.state === "timedOut" ? `A page took too long to read, and nothing was changed.${job.said ? ` ${job.said}` : ""}`
      : `It could not be read, and nothing was changed.${job.said ? ` ${job.said}` : ""}`;
    const these = host.pages();
    const choice = (/** @type {string} */ value, /** @type {string} */ label, /** @type {number} */ n) =>
      `<label><input type="radio" name="pdf-ocr-which" data-pdf-ocr-which="${value}"${state.which === value ? " checked" : ""}${n ? "" : " disabled"}> ${esc(label)}</label>`;
    const langList = state.langs.map((l) =>
      `<label title="${esc(l)}"><input type="checkbox" data-pdf-ocr-lang="${esc(l)}"${state.chosen.includes(l) ? " checked" : ""}> ${esc(NAMES[l] ?? l)}</label>`).join("");
    const pages = pagesFor(state.which);
    const can = host.editable() && pages.length > 0 && state.chosen.length > 0;
    return `<section class="pdf-tool" data-pdf-ocr>${head}
      ${ended ? `<p class="${job?.state === "done" ? "" : "pdf-ocr-ended"}">${esc(ended)}</p>` : ""}
      <div class="pdf-ocr-which">
        ${choice("these", these.length === 1 ? `This page (${these[0]})` : `The ${these.length} pages chosen`, these.length)}
        ${choice("blank", `The pages with no text${blank?.length ? ` (${blank.length})` : ""}`, blank?.length ?? 0)}
        ${choice("all", `Every page (${host.count()})`, host.count())}
      </div>
      <div class="pdf-tool-checks pdf-ocr-langs" aria-label="Languages">${langList}</div>
      <div class="pdf-tool-row"><button class="cc-primary" data-pdf-ocr-do="start"${can ? "" : " disabled"}>Make searchable</button></div>
      <p class="lab-faint">Each page is drawn at 300 dpi and read by tesseract on this Mac. Its words are laid over the page unseen; the picture is left as it was.</p>
    </section>`;
  }

  /** A click in the tools panel that is OCR's. @param {HTMLElement} t @returns {boolean} whether it was */
  function toolClick(t) {
    const which = /** @type {HTMLInputElement | null} */ (t.closest("[data-pdf-ocr-which]"));
    if (which) {
      state.which = /** @type {any} */ (which.dataset.pdfOcrWhich);
      return true;
    }
    const lang = /** @type {HTMLInputElement | null} */ (t.closest("[data-pdf-ocr-lang]"));
    if (lang) {
      const code = lang.dataset.pdfOcrLang ?? "";
      state.chosen = lang.checked ? [...state.chosen.filter((l) => l !== code), code] : state.chosen.filter((l) => l !== code);
      keepChosen(state.chosen);
      return true;
    }
    const act = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-ocr-do]"));
    if (!act) return false;
    if (act.dataset.pdfOcrDo === "start") start(pagesFor(state.which));
    if (act.dataset.pdfOcrDo === "stop") host.dispatch({ type: "pdfOcrStop" });
    return true;
  }

  /** Whether OCR is for this document: pages with no text, or reading. */
  const wanted = () => Boolean(state.blank?.length) || running();

  return { look, langs, progress, offer, offered, toolsHtml, toolClick, wanted };
}
