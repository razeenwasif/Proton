/* A PDF, drawn into a box that scrolls: the viewer over a mode's stage has
 * one, and LaTeX's pane another, which is drawn again in place each time the
 * document is typeset.
 *
 * `pdf.js` is imported on first use rather than at boot: it is 1.7MB of
 * vendored library, and a session that never opens a PDF never pays for it.
 *
 * Pages are drawn as they come near the box's window and let go of as they
 * leave it: drawn all at once, a page is a canvas of eight megabytes at a
 * high-DPI screen's resolution, and a seven-hundred-page textbook would ask
 * for more memory than the window has. Each drawn page carries its text,
 * laid out by pdf.js over the picture, so it can be selected and copied.
 *
 * Read again in place (`reload`), the document keeps the page and the place
 * on it that was at the top, and its scale: the pages on screen are drawn
 * before the old ones are taken away, so nothing flashes white between. */

/** Zoom steps, so `+` and `−` land on round numbers people recognise. */
export const ZOOM = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];

/** How far beyond the window a page is drawn before it is reached. */
const NEAR = "1200px 0px";

/** The space around the pages, matching the box's padding. */
export const GUTTER = 32;

/** The loaded library, once something has needed it. */
let pdfjs = /** @type {any} */ (null);

/** One worker for every document: a reload does not start another. */
let worker = /** @type {any} */ (null);

export async function library() {
  if (pdfjs) return pdfjs;
  // By a name the type checker does not follow: the vendored library is
  // checked by its makers, not here.
  const vendored = "./vendor/pdfjs/pdf.min.mjs";
  pdfjs = await import(vendored);
  // The worker is what keeps parsing off the main thread; without it pdf.js
  // parses inline, and a large document freezes the window while it loads.
  pdfjs.GlobalWorkerOptions.workerSrc = new URL("./vendor/pdfjs/pdf.worker.min.mjs", import.meta.url).href;
  return pdfjs;
}

/** Where pdf.js finds what a PDF may lean on without carrying it: the
 *  character maps CJK text is written in, the standard fonts it names
 *  without embedding, the decoders for JPEG 2000 and JBIG2 images, and the
 *  colour profile CMYK is converted with — vendored beside the library (see
 *  vendor/pdfjs/README.md). The decoders are WebAssembly with a plain-JS
 *  fallback beside each, which pdf.js imports when a content security policy
 *  refuses to compile WebAssembly: Proton's allows only `script-src 'self'`. */
function assets() {
  const dir = (/** @type {string} */ name) => new URL(`./vendor/pdfjs/${name}/`, import.meta.url).href;
  return { cMapUrl: dir("cmaps"), cMapPacked: true, standardFontDataUrl: dir("standard_fonts"), wasmUrl: dir("wasm"), iccUrl: dir("iccs") };
}

/** @typedef {{n: number, slot: HTMLElement, w: number, h: number, drawn: number, task: any}} Page */
/** @typedef {{doc: any, scale: number, pages: Page[], seen: IntersectionObserver}} Open */

/**
 * @param {HTMLElement} box the element that scrolls, and holds the pages
 * @param {{onWhere?: (page: number, pages: number, scale: number) => void,
 *   onPoint?: (page: number, x: number, y: number) => void}} [hooks]
 *   `onWhere`, as the page at the top changes; `onPoint`, on a double-click,
 *   with the point in PDF points from the page's top left
 */
export function createPdfView(box, hooks = {}) {
  /** @type {Open | null} */
  let open = null;
  /** Which load is the current one, so one superseded while it loads stops. */
  let generation = 0;
  /** Whether the scale follows the box's width. */
  let fitting = true;
  /** `g` pressed once, waiting for the second of `gg`. */
  let pendingG = 0;

  const note = (/** @type {string} */ text) => {
    box.innerHTML = `<div class="doc-note">${text}</div>`;
  };

  /** The scale at which a page `width` wide fills the box, within the steps' range. */
  function fitted(/** @type {number} */ width) {
    const room = (box.clientWidth || 800) - GUTTER;
    return Math.min(ZOOM[ZOOM.length - 1], Math.max(ZOOM[0], room / width));
  }

  /** Each slot at its page's size, at the current scale. */
  function layout() {
    if (!open) return;
    for (const page of open.pages) {
      page.slot.style.width = `${Math.floor(page.w * open.scale)}px`;
      page.slot.style.height = `${Math.floor(page.h * open.scale)}px`;
    }
  }

  /** Pages coming near are drawn; pages gone far are let go of. */
  function near(/** @type {IntersectionObserverEntry[]} */ entries) {
    if (!open) return;
    for (const entry of entries) {
      const page = open.pages[Number(/** @type {HTMLElement} */ (entry.target).dataset.page) - 1];
      if (!page) continue;
      if (entry.isIntersecting) void draw(open, page);
      else forget(page);
    }
  }

  /** Draw one page of `into`, and lay its text over it. */
  async function draw(/** @type {Open} */ into, /** @type {Page} */ page) {
    if (page.task || page.drawn === into.scale) return;
    const { doc, scale } = into;
    const pdfPage = await doc.getPage(page.n);
    if (into.scale !== scale) return;
    const viewport = pdfPage.getViewport({ scale });
    // Its own size, which a document of mixed pages need not share with the first.
    page.w = viewport.width / scale;
    page.h = viewport.height / scale;
    page.slot.style.width = `${Math.floor(viewport.width)}px`;
    page.slot.style.height = `${Math.floor(viewport.height)}px`;
    // Device pixels, so a page is not soft on a high-DPI screen.
    const ratio = window.devicePixelRatio || 1;
    const canvas = document.createElement("canvas");
    canvas.width = Math.floor(viewport.width * ratio);
    canvas.height = Math.floor(viewport.height * ratio);
    const task = pdfPage.render({ canvasContext: canvas.getContext("2d"), viewport, transform: ratio === 1 ? null : [ratio, 0, 0, ratio, 0, 0] });
    page.task = task;
    try {
      await task.promise;
    } catch {
      // Called off: it went far, or the zoom changed; a newer draw counts.
      if (page.task === task) page.task = null;
      return;
    }
    if (page.task !== task) return;
    page.task = null;
    if (into.scale !== scale) return;
    const text = document.createElement("div");
    text.className = "textLayer";
    page.slot.style.setProperty("--total-scale-factor", String(scale));
    const mark = page.slot.querySelector(".pdf-mark");
    page.slot.replaceChildren(canvas, text, ...(mark ? [mark] : []));
    page.drawn = scale;
    try {
      await new pdfjs.TextLayer({ textContentSource: pdfPage.streamTextContent(), container: text, viewport }).render();
    } catch {
      // A page whose text cannot be read is still a picture of itself.
    }
  }

  /** Let a page's picture go, keeping its place. */
  function forget(/** @type {Page} */ page) {
    page.task?.cancel();
    page.task = null;
    if (page.drawn) {
      page.slot.replaceChildren();
      page.drawn = 0;
    }
  }

  /** Let a whole document go. @param {Open | null} what */
  function drop(what) {
    if (!what) return;
    what.seen.disconnect();
    for (const page of what.pages) forget(page);
    what.doc.destroy?.();
  }

  async function load(/** @type {Uint8Array} */ bytes) {
    const lib = await library();
    worker ||= new lib.PDFWorker();
    return lib.getDocument({ data: bytes, worker, ...assets() }).promise;
  }

  /** Slots for every page of `doc`, laid out at the first page's size. */
  async function slots(/** @type {any} */ doc, /** @type {number | null} */ scale) {
    const first = (await doc.getPage(1)).getViewport({ scale: 1 });
    /** @type {Page[]} */
    const pages = [];
    for (let n = 1; n <= doc.numPages; n++) {
      const slot = document.createElement("div");
      slot.className = "doc-page";
      slot.dataset.page = String(n);
      pages.push({ n, slot, w: first.width, h: first.height, drawn: 0, task: null });
    }
    const seen = new IntersectionObserver(near, { root: box, rootMargin: NEAR });
    return { doc, scale: scale ?? fitted(first.width), pages, seen };
  }

  /** The page at the top of the box, and how far down it the top is, in points. */
  function anchor() {
    if (!open) return null;
    const top = box.scrollTop;
    const page = open.pages.find((p) => p.slot.offsetTop + p.slot.offsetHeight > top) ?? open.pages[0];
    return { n: page.n, offset: (top - page.slot.offsetTop) / open.scale, left: box.scrollLeft };
  }

  /** Show a document, at `page`. */
  async function show(/** @type {Uint8Array} */ bytes, /** @type {{page?: number | null}} */ at = {}) {
    drop(open);
    open = null;
    fitting = true;
    const mine = ++generation;
    note("Loading…");
    let doc;
    try {
      doc = await load(bytes);
    } catch (err) {
      if (mine === generation) note(`This does not look like a PDF.<br>${escape(String(err))}`);
      return false;
    }
    if (mine !== generation) {
      doc.destroy();
      return false;
    }
    const made = await slots(doc, null);
    if (mine !== generation) {
      doc.destroy();
      return false;
    }
    box.replaceChildren(...made.pages.map((p) => p.slot));
    open = made;
    layout();
    for (const page of made.pages) made.seen.observe(page.slot);
    if (at.page && at.page > 1) goTo(at.page);
    where();
    return true;
  }

  /** Read the document again, in place: the same page, the same place on
   *  it, the same scale — or, fitted, the box's width. */
  async function reload(/** @type {Uint8Array} */ bytes) {
    if (!open) return show(bytes);
    const was = anchor();
    const mine = ++generation;
    let doc;
    try {
      doc = await load(bytes);
    } catch {
      return false;
    }
    if (mine !== generation) {
      doc.destroy();
      return false;
    }
    const made = await slots(doc, fitting ? null : open.scale);
    if (mine !== generation) {
      doc.destroy();
      return false;
    }
    // The pages that will be on screen, drawn before they are shown.
    const height = box.clientHeight;
    const start = Math.max(1, (was?.n ?? 1) - 1);
    for (const page of made.pages) {
      page.slot.style.width = `${Math.floor(page.w * made.scale)}px`;
      page.slot.style.height = `${Math.floor(page.h * made.scale)}px`;
    }
    const visible = made.pages.filter((p) => p.n >= start && p.n <= start + Math.ceil(height / (p.h * made.scale)) + 1);
    await Promise.all(visible.map((p) => draw(made, p)));
    if (mine !== generation) {
      drop(made);
      return false;
    }
    const old = open;
    box.replaceChildren(...made.pages.map((p) => p.slot));
    open = made;
    layout();
    const page = was ? made.pages[Math.min(made.pages.length, was.n) - 1] : null;
    if (page && was) {
      box.scrollTop = page.slot.offsetTop + was.offset * made.scale;
      box.scrollLeft = was.left;
    }
    for (const p of made.pages) made.seen.observe(p.slot);
    // Drawn already, the visible ones are not drawn again; the old go.
    old.seen.disconnect();
    old.doc.destroy?.();
    where();
    return true;
  }

  /** Draw at `scale`, keeping the page that was at the top of the box there. */
  function rescale(/** @type {number} */ scale) {
    if (!open || Math.abs(scale - open.scale) < 1e-6) return;
    const was = anchor();
    for (const page of open.pages) forget(page);
    open.scale = scale;
    layout();
    const page = was ? open.pages[was.n - 1] : null;
    if (page && was) box.scrollTop = page.slot.offsetTop + was.offset * scale;
    open.seen.disconnect();
    for (const p of open.pages) open.seen.observe(p.slot);
    where();
  }

  /** The next zoom step in or out from wherever it is now, fitted or not. */
  function zoom(/** @type {number} */ by) {
    if (!open) return;
    const now = open.scale;
    const scale = by > 0 ? ZOOM.find((z) => z > now + 1e-6) : [...ZOOM].reverse().find((z) => z < now - 1e-6);
    if (scale) {
      fitting = false;
      rescale(scale);
    }
  }

  /** The page's width filling the box, and kept so as the box changes. */
  function fit() {
    fitting = true;
    if (open) rescale(fitted(open.pages[0].w));
  }

  /** Show page `n`, `offset` points down it, at the top of the box. */
  function goTo(/** @type {number} */ n, offset = 0) {
    const page = open?.pages[Math.min(open.pages.length, Math.max(1, n)) - 1];
    if (page && open) box.scrollTop = page.slot.offsetTop + offset * open.scale - GUTTER / 2;
  }

  /** Bring a box on page `n` — in points, from the page's top left — into
   *  view, and mark it a moment. @param {number} n @param {{x: number, y: number, w: number, h: number}} rect */
  function reveal(n, rect) {
    if (!open) return;
    const page = open.pages[n - 1];
    if (!page) return;
    const s = open.scale;
    const top = page.slot.offsetTop + rect.y * s;
    if (top < box.scrollTop + 40 || top + rect.h * s > box.scrollTop + box.clientHeight - 40) {
      box.scrollTop = top - box.clientHeight / 3;
    }
    for (const old of box.querySelectorAll(".pdf-mark")) old.remove();
    const mark = document.createElement("div");
    mark.className = "pdf-mark";
    mark.style.left = `${rect.x * s}px`;
    mark.style.top = `${rect.y * s}px`;
    mark.style.width = `${Math.max(8, rect.w * s)}px`;
    mark.style.height = `${Math.max(8, rect.h * s)}px`;
    page.slot.append(mark);
    setTimeout(() => mark.remove(), 2400);
  }

  /** Which page is at the top of the box. */
  function where() {
    if (!open) return;
    const line = box.scrollTop + box.clientHeight / 3;
    const page = open.pages.find((p) => p.slot.offsetTop + p.slot.offsetHeight >= line) ?? open.pages[open.pages.length - 1];
    hooks.onWhere?.(page.n, open.pages.length, open.scale);
  }

  /** Let the document go, and empty the box. */
  function close() {
    generation++;
    drop(open);
    open = null;
    box.innerHTML = "";
  }

  let wherePending = false;
  box.addEventListener("scroll", () => {
    if (wherePending) return;
    wherePending = true;
    requestAnimationFrame(() => {
      wherePending = false;
      where();
    });
  });

  // A double-click on a page says where, in the page's own points.
  box.addEventListener("dblclick", (e) => {
    const slot = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest(".doc-page"));
    if (!slot || !open || !hooks.onPoint) return;
    const at = slot.getBoundingClientRect();
    hooks.onPoint(Number(slot.dataset.page), (e.clientX - at.left) / open.scale, (e.clientY - at.top) / open.scale);
  });

  // Fitted, the pages follow the box's width.
  new ResizeObserver(() => {
    if (fitting && open) rescale(fitted(open.pages[0].w));
  }).observe(box);

  /**
   * A key, while the box has the keyboard. Its own keys scroll, page and
   * zoom it; answers whether it was one of them.
   * @param {KeyboardEvent} e
   */
  function key(e) {
    const screen = box.clientHeight * 0.9;
    const by = (/** @type {number} */ dy) => {
      box.scrollBy({ top: dy });
      return true;
    };
    if (e.ctrlKey) {
      switch (e.key) {
        case "d": return by(screen / 2);
        case "u": return by(-screen / 2);
        case "f": return by(screen);
        case "b": return by(-screen);
      }
      return false;
    }
    const g = pendingG;
    pendingG = 0;
    switch (e.key) {
      case "j": case "ArrowDown": return by(60);
      case "k": case "ArrowUp": return by(-60);
      case "h": case "ArrowLeft":
        box.scrollBy({ left: -60 });
        return true;
      case "l": case "ArrowRight":
        box.scrollBy({ left: 60 });
        return true;
      case " ": return by(e.shiftKey ? -screen : screen);
      case "PageDown": return by(screen);
      case "PageUp": return by(-screen);
      case "Home":
        box.scrollTop = 0;
        return true;
      case "End": case "G":
        box.scrollTop = box.scrollHeight;
        return true;
      case "g":
        if (g && Date.now() - g < 800) box.scrollTop = 0;
        else pendingG = Date.now();
        return true;
      case "+": case "=":
        zoom(1);
        return true;
      case "-":
        zoom(-1);
        return true;
      case "0":
        fit();
        return true;
    }
    return false;
  }

  return {
    show, reload, close, zoom, fit, goTo, reveal, key, note,
    isOpen: () => open !== null,
    scale: () => open?.scale ?? 1,
    pages: () => open?.pages.length ?? 0,
  };
}

function escape(/** @type {string} */ text) {
  return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c);
}
