/* Canvas mode ("raster" in the code): pixel art and painting.
 *
 * Laid out as Game and Reader are, on Codex's ground: the layers on the
 * left, the picture in the middle, and what is selected beside the agent on
 * the right; with no canvas open, a home page of the project's canvases and
 * a new one from the Pixel or the Paint preset.
 *
 * The window keeps its own copy of the canvas, in the same engine the core
 * uses, built as WebAssembly (`raster-runtime.js`). An edit is made there
 * first and shown at once, then sent to the core numbered; the core makes it
 * on the canvas it saves and answers with a hash of the tiles it touched,
 * which must be the hash made here. When it is not — or the edit was
 * refused — this copy is thrown away and fetched again from the core. What
 * the core changes itself (undo, redo, the agent) arrives as the tiles it
 * changed, fetched through `read_raster`. */

import { escapeHtml as esc } from "./html.js";
import { readRaster } from "./bridge.js";
import { createRasterRuntime } from "./raster-runtime.js";
import { createRasterRender } from "./raster-render.js";
import { fit, toCanvas, zoomAt } from "./raster-view.js";

/** @typedef {import('./bindings/RasterFound').RasterFound} Found */
/** @typedef {import('./bindings/RasterView').RasterView} View */
/** @typedef {import('./bindings/RasterLayer').RasterLayer} Layer */
/** @typedef {import('./bindings/RasterOp').RasterOp} Op */
/** @typedef {import('./bindings/RasterBlend').RasterBlend} Blend */

const BLENDS = /** @type {[Blend, string][]} */ ([
  ["normal", "Normal"], ["multiply", "Multiply"], ["screen", "Screen"], ["overlay", "Overlay"],
  ["darken", "Darken"], ["lighten", "Lighten"], ["colorDodge", "Color dodge"], ["colorBurn", "Color burn"],
  ["hardLight", "Hard light"], ["softLight", "Soft light"], ["difference", "Difference"], ["exclusion", "Exclusion"],
  ["add", "Add"], ["subtract", "Subtract"],
]);

/** Sizes a new canvas may start at, by preset. */
const SIZES = {
  pixel: [[16, 16], [32, 32], [64, 64], [128, 128], [256, 256]],
  paint: [[1024, 1024], [2048, 2048], [1920, 1080], [2560, 1440], [4096, 4096]],
};

const KEYS = /** @type {[string, string][]} */ ([
  ["⌘S", "save the canvas"], ["⌘Z ⇧⌘Z", "undo, redo"], ["f", "the whole canvas"], ["1", "100 %"],
  ["two fingers", "move over the canvas"], ["pinch ⌘ wheel", "zoom"], ["␣ drag", "move over the canvas"], ["?", "these keys"],
]);

/** @param {{root: Document, dispatch: (command: any) => unknown, keys?: (title: string, rows: [string, string][]) => void}} host */
export function createRaster(host) {
  const ws = /** @type {HTMLElement} */ (host.root.querySelector('.workspace[data-ws="raster"]'));
  const q = (/** @type {string} */ selector) => /** @type {HTMLElement} */ (ws.querySelector(selector));
  const send = host.dispatch;

  /** @type {Found[]} */
  let canvases = [];
  /** The canvas open, as the core last said. @type {View | null} */
  let view = null;
  /** This window's copy of it. @type {Awaited<ReturnType<typeof createRasterRuntime>> | null} */
  let copy = null;
  /** Which canvas the copy is of. */
  let copyOf = 0;
  /** The layer drawn on and inspected. @type {number | null} */
  let chosen = null;
  /** Edits sent and not yet answered, oldest first. @type {{seq: number, hash: string}[]} */
  let inflight = [];
  let seq = 0;
  let shown = false;
  let asking = false;
  /** How the canvas is seen; refitted until the person moves it.
   *  @type {{zoom: number, x: number, y: number, moved: boolean} | null} */
  let cam = null;
  /** Everything that touches the copy, one after another. */
  let queue = Promise.resolve();
  const later = (/** @type {() => Promise<void> | void} */ job) => {
    queue = queue.then(job).catch((error) => console.error("proton: canvas", error));
    return queue;
  };

  const stage = q("[data-raster-stage]");
  const glCanvas = /** @type {HTMLCanvasElement} */ (q("[data-raster-gl]"));
  let gpu = (() => {
    try {
      return createRasterRender(glCanvas, { onLost: () => { gpu = null; } });
    } catch {
      return null;
    }
  })();

  /* ── the home page ───────────────────────────────────────────── */

  function renderHome() {
    ws.classList.toggle("raster-home", !view);
    q("[data-raster-open]").hidden = Boolean(view);
    const box = q("[data-raster-home]");
    const sizes = (/** @type {"pixel" | "paint"} */ preset) => SIZES[preset].map(([w, h]) => `<option value="${w}x${h}"${(preset === "pixel" ? w === 64 : w === 2048 && h === 2048) ? " selected" : ""}>${w} × ${h}</option>`).join("");
    box.innerHTML =
      `<header class="raster-home-head"><h3>Canvases</h3></header>` +
      (canvases.length
        ? `<div class="raster-cards">${canvases
            .map((c) => `<button class="raster-card" data-raster-open-path="${esc(c.path)}"><b>${esc(c.name)}</b><span>${c.preset === "pixel" ? "Pixel" : "Paint"} · ${c.size[0]} × ${c.size[1]}</span><span class="raster-card-path">${esc(c.path)}</span></button>`)
            .join("")}</div>`
        : `<p class="raster-faint">No canvas in this project yet.</p>`) +
      `<form class="raster-new" data-raster-new>` +
      `<h4>New canvas</h4>` +
      `<label>Name <input name="name" data-raster-new-name placeholder="Hero sprite" autocomplete="off" spellcheck="false" required></label>` +
      `<div class="raster-presets" role="radiogroup" aria-label="Preset">` +
      `<label><input type="radio" name="preset" value="pixel" checked> <b>Pixel</b> <span>a small grid, a palette, a pencil</span></label>` +
      `<label><input type="radio" name="preset" value="paint"> <b>Paint</b> <span>a large canvas and soft brushes</span></label></div>` +
      `<label>Size <select name="size" data-raster-new-size data-pixel="${esc(sizes("pixel"))}" data-paint="${esc(sizes("paint"))}">${sizes("pixel")}</select></label>` +
      `<button class="cc-primary" type="submit">Make it</button></form>`;
  }

  ws.addEventListener("click", (e) => {
    const card = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-raster-open-path]"));
    if (card) send({ type: "rasterOpen", path: card.dataset.rasterOpenPath });
  });
  ws.addEventListener("change", (e) => {
    const radio = /** @type {HTMLInputElement} */ (e.target);
    if (radio.name !== "preset" || !radio.closest("[data-raster-new]")) return;
    const size = /** @type {HTMLSelectElement} */ (q("[data-raster-new-size]"));
    size.innerHTML = size.dataset[radio.value === "paint" ? "paint" : "pixel"] ?? "";
  });
  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (!form.matches("[data-raster-new]")) return;
    e.preventDefault();
    const data = new FormData(form);
    const name = String(data.get("name") ?? "").trim();
    if (!name) return;
    const [width, height] = String(data.get("size") ?? "64x64").split("x").map(Number);
    send({ type: "rasterNew", name, preset: String(data.get("preset") ?? "pixel"), width, height });
  });

  /* ── the bar ─────────────────────────────────────────────────── */

  function renderBar() {
    q("[data-raster-name]").textContent = view?.name ?? "";
    q("[data-raster-size]").textContent = view ? `${view.preset === "pixel" ? "Pixel" : "Paint"} · ${view.width} × ${view.height}` : "";
    q("[data-raster-dirty]").hidden = !view?.dirty;
    const undo = /** @type {HTMLButtonElement} */ (q("[data-raster-undo]"));
    const redo = /** @type {HTMLButtonElement} */ (q("[data-raster-redo]"));
    undo.disabled = !view?.canUndo;
    redo.disabled = !view?.canRedo;
    undo.title = view?.undo ? `Undo ${view.undo}  ⌘Z` : "Undo  ⌘Z";
    redo.title = view?.redo ? `Redo ${view.redo}  ⇧⌘Z` : "Redo  ⇧⌘Z";
    const save = /** @type {HTMLButtonElement} */ (q("[data-raster-save]"));
    save.disabled = !view?.dirty || Boolean(view?.saving);
    save.textContent = view?.saving ? "Saving…" : "Save";
    q("[data-raster-zoom]").textContent = cam ? `${Math.round(cam.zoom * 100)} %` : "";
  }
  q("[data-raster-undo]").addEventListener("click", () => send({ type: "rasterUndo" }));
  q("[data-raster-redo]").addEventListener("click", () => send({ type: "rasterRedo" }));
  q("[data-raster-save]").addEventListener("click", () => send({ type: "rasterSave" }));
  q("[data-raster-leave]").addEventListener("click", () => leave());

  function leave() {
    if (!view) return;
    if (view.dirty) {
      asking = true;
      renderAsk();
      return;
    }
    send({ type: "rasterClose", discard: false });
  }

  function renderAsk() {
    const box = q("[data-raster-ask]");
    box.hidden = !asking;
    if (!asking) return;
    box.innerHTML =
      `<span>${esc(view?.name ?? "This canvas")} has changes that are not saved.</span><span class="grow"></span>` +
      `<button class="cc-quiet" data-raster-ask-answer="stay">Stay</button>` +
      `<button class="cc-quiet" data-raster-ask-answer="discard">Leave without saving</button>` +
      `<button class="cc-primary" data-raster-ask-answer="save">Save and leave</button>`;
  }
  q("[data-raster-ask]").addEventListener("click", (e) => {
    const answer = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-raster-ask-answer]"))?.dataset.rasterAskAnswer;
    if (!answer) return;
    asking = false;
    renderAsk();
    if (answer === "save") {
      send({ type: "rasterSave" });
      send({ type: "rasterClose", discard: true });
    } else if (answer === "discard") {
      send({ type: "rasterClose", discard: true });
    }
  });

  /* ── layers ──────────────────────────────────────────────────── */

  /** The layer chosen, or the top one. */
  function current() {
    if (!view) return null;
    return view.layers.find((l) => l.id === chosen) ?? view.layers[view.layers.length - 1] ?? null;
  }

  function renderLayers() {
    const box = q("[data-raster-layers]");
    if (!view) {
      box.innerHTML = "";
      return;
    }
    const on = current();
    box.innerHTML = [...view.layers]
      .reverse()
      .map(
        (l) =>
          `<div class="raster-layer${l.id === on?.id ? " on" : ""}${l.hidden ? " hidden" : ""}" data-raster-layer="${l.id}">` +
          `<button class="raster-eye" data-raster-eye="${l.id}" aria-pressed="${!l.hidden}" title="${l.hidden ? "Show it" : "Hide it"}">${l.hidden ? "◌" : "●"}</button>` +
          `<span class="raster-layer-name">${esc(l.name)}</span>` +
          `<span class="raster-layer-meta">${l.blend === "normal" ? "" : esc(BLENDS.find(([b]) => b === l.blend)?.[1] ?? l.blend)}${l.opacity < 255 ? ` ${Math.round((l.opacity / 255) * 100)} %` : ""}</span>` +
          (l.locked ? `<span class="raster-layer-flag" title="Locked">🔒</span>` : "") +
          `</div>`,
      )
      .join("");
  }

  q("[data-raster-layers]").addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const eye = /** @type {HTMLElement | null} */ (target.closest("[data-raster-eye]"));
    if (eye) {
      const layer = view?.layers.find((l) => l.id === Number(eye.dataset.rasterEye));
      if (layer) edit({ op: "setLayer", layer: layer.id, hidden: !layer.hidden }, layer.hidden ? "show layer" : "hide layer");
      return;
    }
    const row = /** @type {HTMLElement | null} */ (target.closest("[data-raster-layer]"));
    if (row) {
      chosen = Number(row.dataset.rasterLayer);
      renderLayers();
      renderInspector();
    }
  });
  q("[data-raster-layer-add]").addEventListener("click", () => {
    const on = current();
    const at = on && view ? view.layers.findIndex((l) => l.id === on.id) + 1 : undefined;
    edit({ op: "addLayer", name: "", at }, "new layer");
  });
  q("[data-raster-layer-remove]").addEventListener("click", () => {
    const on = current();
    if (on) edit({ op: "removeLayer", layer: on.id }, "remove layer");
  });
  for (const [selector, by] of /** @type {[string, number][]} */ ([["[data-raster-layer-up]", 1], ["[data-raster-layer-down]", -1]])) {
    q(selector).addEventListener("click", () => {
      const on = current();
      if (!on || !view) return;
      const at = view.layers.findIndex((l) => l.id === on.id) + by;
      if (at >= 0 && at < view.layers.length) edit({ op: "moveLayer", layer: on.id, to: at }, "move layer");
    });
  }

  /* ── the inspector ───────────────────────────────────────────── */

  function renderInspector() {
    const box = q("[data-raster-inspector]");
    const l = current();
    if (!view || !l) {
      box.innerHTML = view ? `<p class="raster-faint">Choose a layer.</p>` : "";
      return;
    }
    box.innerHTML =
      `<header class="raster-insp-head"><h4>${esc(l.name)}</h4></header>` +
      `<div class="raster-fields" data-raster-fields="${l.id}">` +
      `<label class="raster-field"><span>Name</span><input type="text" data-raster-field="name" value="${esc(l.name)}" spellcheck="false"></label>` +
      `<label class="raster-field"><span>Opacity</span><input type="range" min="0" max="255" value="${l.opacity}" data-raster-field="opacity"><small data-raster-opacity>${Math.round((l.opacity / 255) * 100)} %</small></label>` +
      `<label class="raster-field"><span>Blend</span><select data-raster-field="blend">${BLENDS.map(([b, name]) => `<option value="${b}"${b === l.blend ? " selected" : ""}>${name}</option>`).join("")}</select></label>` +
      `<label class="raster-field raster-check"><input type="checkbox" data-raster-field="alphaLock"${l.alphaLock ? " checked" : ""}> <span>Keep its shape — draw only where it has something</span></label>` +
      `<label class="raster-field raster-check"><input type="checkbox" data-raster-field="locked"${l.locked ? " checked" : ""}> <span>Locked</span></label>` +
      `</div>`;
  }

  const inspector = q("[data-raster-inspector]");
  // While opacity is dragged, only this window's copy shows it; one edit
  // goes to the core when it is let go.
  inspector.addEventListener("input", (e) => {
    const field = /** @type {HTMLInputElement} */ (e.target);
    if (field.dataset.rasterField !== "opacity" || !view || !copy) return;
    const id = Number(field.closest("[data-raster-fields]")?.getAttribute("data-raster-fields"));
    const opacity = Number(field.value);
    const box = inspector.querySelector("[data-raster-opacity]");
    if (box) box.textContent = `${Math.round((opacity / 255) * 100)} %`;
    copy.meta(view.layers.map((l) => (l.id === id ? { ...l, opacity } : l)), view.palette);
    paint();
  });
  inspector.addEventListener("change", (e) => {
    const field = /** @type {HTMLInputElement} */ (e.target);
    const name = field.dataset.rasterField;
    if (!name || !view) return;
    const id = Number(field.closest("[data-raster-fields]")?.getAttribute("data-raster-fields"));
    if (name === "opacity" && copy) copy.meta(view.layers, view.palette);
    const value = field.type === "checkbox" ? field.checked : name === "opacity" ? Number(field.value) : field.value;
    if (name === "name" && !String(value).trim()) return renderInspector();
    edit(/** @type {Op} */ ({ op: "setLayer", layer: id, [name]: value }), name === "name" ? "rename layer" : `layer ${name}`);
  });

  /** The right pane's tab: the inspector or the agent. @param {string} which */
  function showPanel(which) {
    const pane = q("#raster-pane");
    pane.dataset.showing = which;
    for (const tab of pane.querySelectorAll("[data-raster-panel]")) {
      const on = /** @type {HTMLElement} */ (tab).dataset.rasterPanel === which;
      tab.classList.toggle("on", on);
      tab.setAttribute("aria-selected", String(on));
    }
    for (const body of pane.querySelectorAll("[data-raster-body]")) {
      /** @type {HTMLElement} */ (body).hidden = /** @type {HTMLElement} */ (body).dataset.rasterBody !== which;
    }
    if (which === "agent") {
      const dot = q("[data-raster-agent-dot]");
      if (dot) dot.hidden = true;
    }
  }
  q("#raster-pane").addEventListener("click", (e) => {
    const tab = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-raster-panel]"));
    if (tab) showPanel(tab.dataset.rasterPanel ?? "inspector");
  });

  /* ── edits ───────────────────────────────────────────────────── */

  /** An edit: made on this window's copy, then sent. @param {Op} op @param {string} label */
  function edit(op, label) {
    if (!view || !copy) return;
    const made = copy.apply(op);
    seq += 1;
    // Refused here too: sent all the same, unpredicted, so the core says why.
    if ("hash" in made) inflight.push({ seq, hash: made.hash });
    send({ type: "rasterEdit", generation: view.generation, seq, op, label });
    paint();
  }

  /** The core's answer to an edit. @param {any} event */
  function applied(event) {
    later(async () => {
      if (!view || event.generation !== view.generation) return;
      // Answered in order: what was sent before this is answered too.
      const at = inflight.findIndex((f) => f.seq === event.seq);
      const mine = at >= 0 ? inflight.splice(0, at + 1)[at] : null;
      view = { ...view, dirty: event.dirty, canUndo: event.canUndo, canRedo: event.canRedo };
      renderBar();
      // An edit this copy refused too: nothing here to take back.
      if (!mine) return;
      if (event.refused || mine.hash !== event.hash) {
        // Not the canvas this window made: start again from the core's.
        if (!event.refused) console.warn(`proton: canvas edit ${event.seq} made different pixels here; fetched again`);
        await reload();
      }
    });
  }

  /** Tiles the core changed itself. @param {any} event */
  function changed(event) {
    later(async () => {
      if (!view || !copy || event.generation !== view.generation) return;
      for (const [layer, tiles] of event.tiles) await fetchTiles(layer, tiles);
      paint();
    });
  }

  /** @param {number} layer @param {number[]} tiles */
  async function fetchTiles(layer, tiles) {
    if (!view || !copy || !tiles.length) return;
    const bytes = await readRaster(view.generation, layer, tiles);
    copy.put(layer, bytes);
  }

  /** This window's copy made again from the core's canvas. */
  async function reload() {
    if (!view) return;
    const next = await createRasterRuntime();
    next.open({ width: view.width, height: view.height, preset: view.preset, palette: view.palette, layers: view.layers, generation: view.generation });
    copy = next;
    copyOf = view.generation;
    inflight = [];
    gpu?.resize(view.width, view.height);
    for (const [layer, tiles] of view.tiles) await fetchTiles(layer, tiles);
    paint();
  }

  /* ── the stage ───────────────────────────────────────────────── */

  function stageSize() {
    return [stage.clientWidth, stage.clientHeight];
  }

  function camera() {
    if (!view) return null;
    const [w, h] = stageSize();
    if (!w || !h) return null;
    if (!cam || !cam.moved) cam = { ...fit(view.width, view.height, w, h, view.preset === "pixel"), moved: false };
    return cam;
  }

  let painting = 0;
  function paint() {
    if (painting) return;
    painting = requestAnimationFrame(() => {
      painting = 0;
      paintNow();
    });
  }

  function colors() {
    const style = getComputedStyle(ws);
    const rgb = (/** @type {string} */ name, /** @type {number[]} */ fallback) => {
      const m = /^#?([0-9a-f]{6})$/i.exec(style.getPropertyValue(name).trim());
      if (!m) return fallback;
      const n = parseInt(m[1], 16);
      return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
    };
    return { ground: rgb("--bg-deep", [0.07, 0.07, 0.08]), light: [0.36, 0.36, 0.38], dark: [0.27, 0.27, 0.29] };
  }

  function paintNow() {
    const empty = q("[data-raster-stage-empty]");
    empty.hidden = true;
    glCanvas.hidden = !view || !gpu;
    if (!view || !copy || !shown) return;
    const c = camera();
    if (!c) return;
    const [w, h] = stageSize();
    // What changed since the last frame, laid together and uploaded; read
    // from the copy's memory before anything else is asked of it.
    const rects = copy.dirty();
    if (rects.length) gpu?.upload(rects, copy.composite());
    gpu?.draw(c, w, h, window.devicePixelRatio || 1, { ...colors(), pixel: view.preset === "pixel" });
    renderBar();
  }
  new ResizeObserver(() => paint()).observe(stage);

  /** A stage point, from a pointer. @param {MouseEvent} e @returns {[number, number]} */
  function stagePoint(e) {
    const box = stage.getBoundingClientRect();
    return [e.clientX - box.left, e.clientY - box.top];
  }

  stage.addEventListener("wheel", (e) => {
    const c = camera();
    if (!c || !view) return;
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      const [sx, sy] = stagePoint(e);
      cam = { ...zoomAt(c, sx, sy, Math.exp(-e.deltaY * 0.01), view.preset === "pixel"), moved: true };
    } else {
      cam = { ...c, x: c.x - e.deltaX, y: c.y - e.deltaY, moved: true };
    }
    paint();
  }, { passive: false });

  let space = false;
  /** @type {{from: [number, number], at: [number, number]} | null} */
  let panning = null;
  stage.addEventListener("keydown", (e) => { if (e.key === " ") space = true; });
  stage.addEventListener("keyup", (e) => { if (e.key === " ") space = false; });
  stage.addEventListener("pointerdown", (e) => {
    stage.focus({ preventScroll: true });
    const c = camera();
    if (!c || !(space || e.button === 1)) return;
    panning = { from: stagePoint(e), at: [c.x, c.y] };
    try {
      stage.setPointerCapture(e.pointerId);
    } catch {
      // A synthetic pointer has nothing to capture.
    }
  });
  stage.addEventListener("pointermove", (e) => {
    if (!panning || !cam) return;
    const [sx, sy] = stagePoint(e);
    cam = { ...cam, x: panning.at[0] + sx - panning.from[0], y: panning.at[1] + sy - panning.from[1], moved: true };
    paint();
  });
  stage.addEventListener("pointerup", () => {
    panning = null;
  });

  /** The whole canvas again. */
  function refit() {
    cam = null;
    paint();
  }

  /** 100 %, about the stage's middle. */
  function actualSize() {
    const c = camera();
    if (!c || !view) return;
    const [w, h] = stageSize();
    cam = { ...zoomAt(c, w / 2, h / 2, 1 / c.zoom, false), moved: true };
    paint();
  }

  /* ── keys ────────────────────────────────────────────────────── */

  /** Canvas mode's own keys. @param {KeyboardEvent} e @returns {boolean} */
  function key(e) {
    if (!shown) return false;
    const target = /** @type {HTMLElement | null} */ (e.target instanceof HTMLElement ? e.target : null);
    const typing = Boolean(target?.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])"));
    if (e.metaKey && !e.altKey && !e.ctrlKey) {
      const k = e.key.toLowerCase();
      if (k === "s" && view) return (send({ type: "rasterSave" }), true);
      if (k === "z" && view && !typing) return (send({ type: e.shiftKey ? "rasterRedo" : "rasterUndo" }), true);
      return false;
    }
    if (typing || e.altKey || e.ctrlKey || e.metaKey || !view) return false;
    if (e.key === "?") return (host.keys?.("— Canvas's keys", KEYS), true);
    if (e.key === "f") return (refit(), true);
    if (e.key === "1") return (actualSize(), true);
    return false;
  }

  /* ── what the core says ──────────────────────────────────────── */

  function render() {
    renderHome();
    renderBar();
    renderAsk();
    renderLayers();
    renderInspector();
    paint();
  }

  render();

  // For the window's tests: this copy's pixels, read from the section.
  /** @type {any} */ (ws).rasterProbe = {
    /** @param {number} x @param {number} y */
    pick: (x, y) => copy?.pick(x, y) ?? null,
    settled: () => queue.then(() => inflight.length === 0),
  };

  return {
    /** @param {{canvases: Found[]}} event */
    home(event) {
      canvases = event.canvases ?? [];
      renderHome();
    },
    /** @param {{canvas: View | null}} event */
    canvas(event) {
      later(async () => {
        const next = event.canvas ?? null;
        const opened = next && next.generation !== copyOf;
        view = next;
        if (!view) {
          copy = null;
          copyOf = 0;
          chosen = null;
          inflight = [];
          cam = null;
        } else if (opened) {
          chosen = null;
          cam = null;
          await reload();
        } else if (copy) {
          // The same canvas: its layers or palette may have changed.
          copy.meta(view.layers, view.palette);
        }
        if (chosen !== null && !view?.layers.some((l) => l.id === chosen)) chosen = null;
        render();
      });
    },
    applied,
    changed,
    /** The mode came on screen, or went. @param {boolean} on */
    show(on) {
      shown = on;
      if (on) paint();
    },
    key,
    /** For tests: a pixel of everything laid together, as this window has it. @param {number} x @param {number} y */
    pick: (x, y) => copy?.pick(x, y) ?? null,
    /** For tests: whether everything sent and fetched has arrived. */
    settled: () => queue.then(() => inflight.length === 0),
    /** A stage point as a canvas point. @param {number} sx @param {number} sy */
    toCanvas: (sx, sy) => (cam ? toCanvas(cam, sx, sy) : null),
  };
}
