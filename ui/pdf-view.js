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

/** A rectangle in a page's own points, `[x1, y1, x2, y2]` from its bottom
 *  left, as `viewport` shows it: two opposite corners carried over, which
 *  is enough for a page turned by quarters. @param {any} viewport @param {number[]} rect */
export function toViewport(viewport, rect) {
  const [a, b] = viewport.convertToViewportPoint(rect[0], rect[1]);
  const [c, d] = viewport.convertToViewportPoint(rect[2], rect[3]);
  return [a, b, c, d];
}

/** Page `n` of `doc` drawn `width` CSS pixels wide, turned as the view is:
 *  for thumbnails and the page grid. @param {any} doc @param {number} n @param {number} width @param {number} turned */
export async function drawSmall(doc, n, width, turned) {
  const page = await doc.getPage(n);
  const rotation = (page.rotate + turned) % 360;
  const at1 = page.getViewport({ scale: 1, rotation });
  const viewport = page.getViewport({ scale: width / at1.width, rotation });
  const ratio = window.devicePixelRatio || 1;
  const canvas = document.createElement("canvas");
  canvas.width = Math.floor(viewport.width * ratio);
  canvas.height = Math.floor(viewport.height * ratio);
  canvas.style.width = `${Math.floor(viewport.width)}px`;
  canvas.style.height = `${Math.floor(viewport.height)}px`;
  await page.render({ canvasContext: canvas.getContext("2d"), viewport, transform: ratio === 1 ? null : [ratio, 0, 0, ratio, 0, 0] }).promise;
  return canvas;
}

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
/** How the pages are laid out: one after another, one at a time, or two side by side.
 *  @typedef {"continuous" | "single" | "two"} Layout */
/** What a search found on a page: boxes in points from the page's top left.
 *  @typedef {{page: number, rects: {x: number, y: number, w: number, h: number}[]}} Hit */
/** Where a link goes: a web address, or somewhere in the document.
 *  @typedef {{url: string} | {dest: any}} Link */
/** @typedef {{doc: any, scale: number, pages: Page[], seen: IntersectionObserver}} Open */

/**
 * @param {HTMLElement} box the element that scrolls, and holds the pages
 * @param {{onWhere?: (page: number, pages: number, scale: number) => void,
 *   onPoint?: (page: number, x: number, y: number) => void,
 *   onLink?: (link: {url: string}) => void,
 *   onDrawn?: (page: number, slot: HTMLElement, viewport: any, pdfPage: any) => void}} [hooks]
 *   `onWhere`, as the page at the top changes; `onPoint`, on a double-click,
 *   with the point in PDF points from the page's top left; `onLink`, for a
 *   link out of the document — given, a page's links are drawn over it and
 *   one into the document goes there; `onDrawn`, as each page is drawn, for
 *   what a mode lays over it
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
  /** How the pages are laid out, and the page shown when it is one at a time. */
  let arrangement = /** @type {Layout} */ ("continuous");
  let single = 1;
  /** Degrees the view is turned by, on top of each page's own turn. */
  let turned = 0;
  /** Colours to read in — dark, sepia — or the document's own. @type {{background: string, foreground: string} | null} */
  let colors = null;
  /** What a search found, and which of it is the one being looked at. @type {Hit[]} */
  let hits = [];
  let hitOn = -1;

  const note = (/** @type {string} */ text) => {
    box.innerHTML = `<div class="doc-note">${text}</div>`;
  };

  /** The scale at which a page `width` wide fills the box — two of them,
   *  side by side — within the steps' range. */
  function fitted(/** @type {number} */ width) {
    const room = (box.clientWidth || 800) - GUTTER;
    const across = arrangement === "two" ? 2 * width + 16 : width;
    return Math.min(ZOOM[ZOOM.length - 1], Math.max(ZOOM[0], room / across));
  }

  /** A page's viewport, turned as the view is. @param {any} pdfPage @param {number} scale */
  const viewportOf = (pdfPage, scale) => pdfPage.getViewport({ scale, rotation: (pdfPage.rotate + turned) % 360 });

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
    const viewport = viewportOf(pdfPage, scale);
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
    const task = pdfPage.render({
      canvasContext: canvas.getContext("2d"), viewport, transform: ratio === 1 ? null : [ratio, 0, 0, ratio, 0, 0],
      ...(colors ? { pageColors: colors } : {}),
    });
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
    paint(page);
    try {
      await new pdfjs.TextLayer({ textContentSource: pdfPage.streamTextContent(), container: text, viewport }).render();
    } catch {
      // A page whose text cannot be read is still a picture of itself.
    }
    if (hooks.onLink) await linksOver(page, pdfPage, viewport);
    if (page.drawn === scale) hooks.onDrawn?.(page.n, page.slot, viewport, pdfPage);
  }

  /** A page's links, laid over it: each a box that goes where it says. */
  async function linksOver(/** @type {Page} */ page, /** @type {any} */ pdfPage, /** @type {any} */ viewport) {
    let annotations = [];
    try {
      annotations = await pdfPage.getAnnotations({ intent: "display" });
    } catch {
      return;
    }
    for (const a of annotations) {
      if (a.subtype !== "Link" || (!a.url && !a.dest && !a.unsafeUrl)) continue;
      const [x1, y1, x2, y2] = toViewport(viewport, a.rect);
      const link = document.createElement("a");
      link.className = "pdf-link";
      link.href = "#";
      link.style.left = `${Math.min(x1, x2)}px`;
      link.style.top = `${Math.min(y1, y2)}px`;
      link.style.width = `${Math.abs(x2 - x1)}px`;
      link.style.height = `${Math.abs(y2 - y1)}px`;
      const url = a.url ?? a.unsafeUrl;
      if (url) {
        link.dataset.url = url;
        link.title = url;
      } else {
        link.dataset.dest = JSON.stringify(a.dest);
      }
      page.slot.append(link);
    }
  }

  /** What a search found on a drawn page, marked over it — the one being
   *  looked at more strongly. */
  function paint(/** @type {Page} */ page) {
    for (const old of page.slot.querySelectorAll(".pdf-hit")) old.remove();
    if (!open || !page.drawn) return;
    const s = open.scale;
    hits.forEach((hit, i) => {
      if (hit.page !== page.n) return;
      for (const r of hit.rects) {
        const mark = document.createElement("div");
        mark.className = `pdf-hit${i === hitOn ? " on" : ""}`;
        mark.style.left = `${r.x * s}px`;
        mark.style.top = `${r.y * s}px`;
        mark.style.width = `${Math.max(2, r.w * s)}px`;
        mark.style.height = `${Math.max(2, r.h * s)}px`;
        page.slot.append(mark);
      }
    });
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
    const first = viewportOf(await doc.getPage(1), 1);
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
    hits = [];
    hitOn = -1;
    single = 1;
    arrange();
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
    single = Math.min(single, made.pages.length);
    arrange();
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
    if (!page || !open) return;
    if (arrangement === "single" && single !== page.n) {
      single = page.n;
      arrange();
    }
    box.scrollTop = page.slot.offsetTop + offset * open.scale - GUTTER / 2;
    if (arrangement === "single") where();
  }

  /** The slots shown as the layout has them: all, or the one page. */
  function arrange() {
    box.dataset.layout = arrangement;
    if (!open) return;
    for (const page of open.pages) page.slot.hidden = arrangement === "single" && page.n !== single;
  }

  /** Lay the pages out another way, keeping the page that was in view. @param {Layout} how */
  function setLayout(how) {
    if (how === arrangement) return;
    const at = open ? currentPage() : 1;
    arrangement = how;
    single = at;
    arrange();
    if (!open) return;
    if (fitting) {
      const scale = fitted(open.pages[0].w);
      if (Math.abs(scale - open.scale) > 1e-6) {
        for (const page of open.pages) forget(page);
        open.scale = scale;
      }
    }
    layout();
    open.seen.disconnect();
    for (const p of open.pages) open.seen.observe(p.slot);
    goTo(at);
    where();
  }

  /** Turn the view by `by` degrees — not the document, which the core turns. */
  function rotate(/** @type {number} */ by) {
    turned = (((turned + by) % 360) + 360) % 360;
    if (!open) return;
    const at = currentPage();
    for (const page of open.pages) {
      forget(page);
      if (Math.abs(by) % 180 === 90) [page.w, page.h] = [page.h, page.w];
    }
    if (fitting) open.scale = fitted(open.pages[0].w);
    layout();
    open.seen.disconnect();
    for (const p of open.pages) open.seen.observe(p.slot);
    goTo(at);
  }

  /** Read in other colours — or, given nothing, the document's own. @param {{background: string, foreground: string} | null} chosen */
  function setColors(chosen) {
    colors = chosen;
    box.style.setProperty("--page-ground", chosen?.background ?? "#fff");
    if (!open) return;
    for (const page of open.pages) forget(page);
    open.seen.disconnect();
    for (const p of open.pages) open.seen.observe(p.slot);
  }

  /** Mark what a search found, `on` the one looked at — brought into view.
   *  @param {Hit[]} found @param {number} on */
  function mark(found, on = -1) {
    hits = found;
    hitOn = on;
    if (!open) return;
    for (const page of open.pages) paint(page);
    const hit = hits[on];
    if (!hit?.rects.length) return;
    const xs = hit.rects.map((r) => r.x), ys = hit.rects.map((r) => r.y);
    const right = Math.max(...hit.rects.map((r) => r.x + r.w)), bottom = Math.max(...hit.rects.map((r) => r.y + r.h));
    show_(hit.page, { x: Math.min(...xs), y: Math.min(...ys), w: right - Math.min(...xs), h: bottom - Math.min(...ys) });
  }

  /** Bring a box on page `n` into view, without marking it. @param {number} n @param {{x: number, y: number, w: number, h: number}} rect */
  function show_(n, rect) {
    if (!open) return;
    const page = open.pages[n - 1];
    if (!page) return;
    if (arrangement === "single" && single !== n) goTo(n);
    const top = page.slot.offsetTop + rect.y * open.scale;
    if (top < box.scrollTop + 40 || top + rect.h * open.scale > box.scrollTop + box.clientHeight - 40) {
      box.scrollTop = top - box.clientHeight / 3;
    }
  }

  /** Where a destination in the document is: its page, and how far down it,
   *  in points — or nothing, for one that names nothing. @param {any} dest */
  async function resolve(dest) {
    if (!open) return null;
    const doc = open.doc;
    let explicit = dest;
    try {
      if (typeof dest === "string") explicit = await doc.getDestination(dest);
      if (!Array.isArray(explicit)) return null;
      const [ref, kind, ...args] = explicit;
      const index = ref && typeof ref === "object" ? await doc.getPageIndex(ref) : Number(ref);
      if (!Number.isInteger(index)) return null;
      const n = index + 1;
      const name = kind?.name;
      const top = name === "XYZ" ? args[1] : name === "FitH" || name === "FitBH" ? args[0] : null;
      let offset = 0;
      if (typeof top === "number") {
        const viewport = viewportOf(await doc.getPage(n), 1);
        offset = Math.max(0, viewport.convertToViewportPoint(0, top)[1]);
      }
      return { page: n, offset };
    } catch {
      return null;
    }
  }

  /** Go to a destination in the document. @param {any} dest */
  async function go(dest) {
    const at = await resolve(dest);
    if (at) goTo(at.page, at.offset);
    return Boolean(at);
  }

  /** The page in view: the one a third of the way down the box. */
  function currentPage() {
    if (!open) return 1;
    if (arrangement === "single") return single;
    const line = box.scrollTop + box.clientHeight / 3;
    return (open.pages.find((p) => p.slot.offsetTop + p.slot.offsetHeight >= line) ?? open.pages[open.pages.length - 1]).n;
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
    hooks.onWhere?.(currentPage(), open.pages.length, open.scale);
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

  // A link: out of the document, said; into it, gone to.
  box.addEventListener("click", (e) => {
    const link = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest(".pdf-link"));
    if (!link) return;
    e.preventDefault();
    if (link.dataset.url) hooks.onLink?.({ url: link.dataset.url });
    else if (link.dataset.dest) void go(JSON.parse(link.dataset.dest));
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
    // One page at a time, the page keys turn the page.
    if (arrangement === "single" && open) {
      const turn = (/** @type {number} */ by) => {
        goTo(single + by);
        return true;
      };
      switch (e.key) {
        case " ": return turn(e.shiftKey ? -1 : 1);
        case "PageDown": case "n": case "ArrowRight": return turn(1);
        case "PageUp": case "p": case "ArrowLeft": return turn(-1);
        case "Home": return turn(1 - single);
        case "End": case "G": return turn(open.pages.length - single);
      }
    }
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
    setLayout, rotate, setColors, mark, go, resolve,
    isOpen: () => open !== null,
    scale: () => open?.scale ?? 1,
    pages: () => open?.pages.length ?? 0,
    page: currentPage,
    layout: () => arrangement,
    turned: () => turned,
    /** The pdf.js document on screen, for what a mode reads of it. */
    doc: () => open?.doc ?? null,
  };
}

function escape(/** @type {string} */ text) {
  return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c);
}
