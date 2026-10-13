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
 * refused — the tiles it touched are fetched again from the core
 * (`raster-sync.js` keeps the rules). What the core changes itself (undo,
 * redo, the agent) arrives as the tiles it changed, fetched the same way.
 *
 * A pencil is drawn as the pen moves: its points go to the copy a frame at a
 * time, and the finished stroke — every point, as the copy saw them — is the
 * edit sent. A shape is shown as it is dragged and sent when let go; a fill
 * and a colour picked are a click. */

import { escapeHtml as esc } from "./html.js";
import { readRaster } from "./bridge.js";
import { createRasterRuntime, keepTiles } from "./raster-runtime.js";
import { createRasterRender } from "./raster-render.js";
import { createSync } from "./raster-sync.js";
import { fit, q8, toCanvas, toStage, zoomAt } from "./raster-view.js";

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
  ["b e g i", "pencil, eraser, fill, pick a colour"], ["l r o", "line, rectangle, ellipse"], ["[ ]", "a smaller, a larger tip"],
  ["p", "pixel-perfect"], ["m ⇧M", "mirror across, down"], ["⇧G", "the pixel grid"], ["⌥ click", "pick a colour"],
  ["⎋", "take back the stroke being drawn"], ["⌘S", "save the canvas"], ["⌘Z ⇧⌘Z", "undo, redo"], ["f", "the whole canvas"], ["1", "100 %"],
  ["two fingers", "move over the canvas"], ["pinch ⌘ wheel", "zoom"], ["␣ drag", "move over the canvas"], ["?", "these keys"],
]);

/** The tools, by the key that takes each. */
const TOOL_KEYS = /** @type {Record<string, string>} */ ({ b: "pencil", e: "eraser", g: "fill", i: "pick", l: "line", r: "rect", o: "ellipse" });

/** Ops that change the layers themselves, not only their pixels. @param {Op} op */
const changesLayers = (op) => ["addLayer", "removeLayer", "moveLayer", "setLayer", "setPalette"].includes(op.op) || (op.op === "replaceColor" && op.swatch !== undefined && op.swatch !== null);

/** `#rrggbb` of a colour. @param {number[]} c */
const hex = (c) => `#${c.slice(0, 3).map((v) => v.toString(16).padStart(2, "0")).join("")}`;

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
  let shown = false;
  /** What the pointer does, and how. */
  let tool = "pencil";
  /** The colour drawn with, straight RGBA. @type {[number, number, number, number]} */
  let color = [27, 27, 35, 255];
  /** Four numbers as a colour. @param {number[]} c @returns {[number, number, number, number]} */
  const rgba = (c) => [c[0] ?? 0, c[1] ?? 0, c[2] ?? 0, c[3] ?? 255];
  let tipSize = 1;
  let round = false;
  let pixelPerfect = true;
  let mirror = [false, false];
  let fillOptions = { tolerance: 0, contiguous: true, diagonal: false };
  let filled = false;
  let grid = false;
  /** The pixel under the pointer, for the cursor. @type {[number, number] | null} */
  let hover = null;
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
  const overlay = /** @type {HTMLCanvasElement} */ (q("[data-raster-overlay]"));
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
  /** @param {string} which */
  function useTool(which) {
    cancel();
    tool = which;
    ws.dataset.tool = which;
    for (const b of ws.querySelectorAll("[data-raster-tool]")) b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.rasterTool === which));
    renderInspector();
    paint();
  }
  q("[data-raster-tools]").addEventListener("click", (e) => {
    const b = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-raster-tool]"));
    if (b) useTool(b.dataset.rasterTool ?? "pencil");
  });
  function renderColor() {
    const input = /** @type {HTMLInputElement} */ (q("[data-raster-color]"));
    input.value = hex(color);
    q("[data-raster-color-swatch]").style.background = `rgba(${color[0]}, ${color[1]}, ${color[2]}, ${color[3] / 255})`;
  }
  q("[data-raster-color]").addEventListener("input", (e) => {
    const v = /** @type {HTMLInputElement} */ (e.target).value;
    color = rgba([parseInt(v.slice(1, 3), 16), parseInt(v.slice(3, 5), 16), parseInt(v.slice(5, 7), 16), 255]);
    renderColor();
    renderPalette();
  });
  function toggleGrid() {
    grid = !grid;
    q("[data-raster-grid]").setAttribute("aria-pressed", String(grid));
    paint();
  }
  q("[data-raster-grid]").addEventListener("click", () => toggleGrid());
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

  /* ── the side: layers, or the palette ────────────────────────── */

  let side = "layers";
  /** @param {string} which */
  function showSide(which) {
    side = which;
    for (const tab of ws.querySelectorAll("[data-raster-side]")) {
      const on = /** @type {HTMLElement} */ (tab).dataset.rasterSide === which;
      tab.classList.toggle("on", on);
      tab.setAttribute("aria-selected", String(on));
    }
    q("[data-raster-palette]").hidden = which !== "palette";
    q("[data-raster-layers]").hidden = which !== "layers";
    q("[data-raster-layer-tools]").hidden = which !== "layers";
    renderPalette();
  }
  ws.querySelector(".raster-side-tabs")?.addEventListener("click", (e) => {
    const tab = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-raster-side]"));
    if (tab) showSide(tab.dataset.rasterSide ?? "layers");
  });

  function renderPalette() {
    const box = q("[data-raster-palette]");
    if (!view || side !== "palette") return;
    const same = (/** @type {number[]} */ c) => c.every((v, i) => v === color[i]);
    box.innerHTML =
      (view.palette.length
        ? `<div class="raster-swatches">${view.palette
            .map((c, i) => `<button class="raster-swatch${same(c) ? " on" : ""}" data-raster-swatch="${i}" style="background: rgba(${c[0]}, ${c[1]}, ${c[2]}, ${c[3] / 255})" title="${hex(c)} — double-click to change it everywhere it is"></button>`)
            .join("")}</div>`
        : `<p class="raster-faint">No palette yet.</p>`) +
      `<div class="raster-palette-tools"><button class="cc-quiet" data-raster-swatch-add>Add ${hex(color)}</button>` +
      (view.palette.some(same) ? `<button class="cc-quiet" data-raster-swatch-remove>Remove it</button>` : "") +
      `</div><p class="raster-palette-hint">Double-click a swatch to change its colour, and every pixel of it, as one edit.</p>`;
  }
  const palette = q("[data-raster-palette]");
  palette.addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    if (!view) return;
    const swatch = /** @type {HTMLElement | null} */ (target.closest("[data-raster-swatch]"));
    if (swatch) {
      color = rgba(view.palette[Number(swatch.dataset.rasterSwatch)]);
      renderColor();
      renderPalette();
      return;
    }
    if (target.closest("[data-raster-swatch-add]")) edit({ op: "setPalette", colors: [...view.palette, color] }, "add a colour");
    if (target.closest("[data-raster-swatch-remove]")) edit({ op: "setPalette", colors: view.palette.filter((c) => !c.every((v, i) => v === color[i])) }, "remove a colour");
  });
  palette.addEventListener("dblclick", (e) => {
    const swatch = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-raster-swatch]"));
    if (!swatch || !view) return;
    const i = Number(swatch.dataset.rasterSwatch);
    const from = view.palette[i];
    const ask = Object.assign(document.createElement("input"), { type: "color", value: hex(from) });
    ask.addEventListener("change", () => {
      const v = ask.value;
      const to = rgba([parseInt(v.slice(1, 3), 16), parseInt(v.slice(3, 5), 16), parseInt(v.slice(5, 7), 16), from[3]]);
      edit({ op: "replaceColor", from, to, swatch: i }, "recolour");
      color = to;
      renderColor();
    });
    ask.click();
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
    const number = (/** @type {string} */ name, /** @type {number} */ value, /** @type {number} */ min, /** @type {number} */ max, /** @type {string} */ label) =>
      `<label class="raster-field"><span>${label}</span><input type="number" min="${min}" max="${max}" value="${value}" data-raster-option="${name}"></label>`;
    const check = (/** @type {string} */ name, /** @type {boolean} */ on, /** @type {string} */ label) =>
      `<label class="raster-field raster-check"><input type="checkbox" data-raster-option="${name}"${on ? " checked" : ""}> <span>${label}</span></label>`;
    const options =
      tool === "fill"
        ? number("tolerance", fillOptions.tolerance, 0, 255, "Tolerance") + check("contiguous", fillOptions.contiguous, "Only what touches it") + check("diagonal", fillOptions.diagonal, "Across corners too")
        : tool === "pick"
          ? `<p class="raster-faint">A click takes the colour there, every layer laid together. ⌥ with any tool does too.</p>`
          : number("size", tipSize, 1, 64, "Size") + check("round", round, "Round") +
            (tool === "pencil" ? check("pixelPerfect", pixelPerfect, "Pixel-perfect — no corners on steps") : "") +
            (tool === "rect" || tool === "ellipse" ? check("filled", filled, "Filled") : "") +
            check("mirrorX", mirror[0], "Mirror across the middle") + check("mirrorY", mirror[1], "Mirror down the middle");
    box.innerHTML =
      `<h5 class="raster-tool-head">${esc({ pencil: "Pencil", eraser: "Eraser", fill: "Fill", pick: "Pick a colour", line: "Line", rect: "Rectangle", ellipse: "Ellipse" }[tool] ?? tool)}</h5>` +
      `<div class="raster-fields" data-raster-options>${options}</div>` +
      `<h5 class="raster-layer-head">Layer</h5>` +
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
    const option = field.dataset.rasterOption;
    if (option) {
      const n = Math.round(Number(field.value));
      if (option === "size") tipSize = Math.min(64, Math.max(1, n || 1));
      if (option === "tolerance") fillOptions = { ...fillOptions, tolerance: Math.min(255, Math.max(0, n || 0)) };
      if (option === "round") round = field.checked;
      if (option === "pixelPerfect") pixelPerfect = field.checked;
      if (option === "filled") filled = field.checked;
      if (option === "contiguous") fillOptions = { ...fillOptions, contiguous: field.checked };
      if (option === "diagonal") fillOptions = { ...fillOptions, diagonal: field.checked };
      if (option === "mirrorX") mirror = [field.checked, mirror[1]];
      if (option === "mirrorY") mirror = [mirror[0], field.checked];
      renderInspector();
      paint();
      return;
    }
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

  const sync = createSync({
    fetch: async (layer, tiles) => {
      if (!view) return { lastSeq: 0, bytes: new ArrayBuffer(24) };
      const bytes = await readRaster(view.generation, layer, tiles);
      return { lastSeq: new DataView(bytes).getUint32(16, true), bytes };
    },
    put: (layer, bytes, keep) => copy?.put(layer, keepTiles(bytes, keep).bytes),
    reload: () => reload(),
    painted: () => paint(),
  });

  /** An op: made on this window's copy, then sent. @param {Op} op @param {string} label */
  function edit(op, label) {
    if (!view || !copy) return;
    const made = copy.apply(op);
    // Refused here too: sent all the same, unpredicted, so the core says why.
    send_(op, label, "hash" in made ? made.hash : null, "hash" in made ? copy.touched() : []);
  }

  /** An edit made here, sent numbered. @param {Op} op @param {string} label
   *  @param {string | null} hash @param {[number, number][]} touched */
  function send_(op, label, hash, touched) {
    if (!view) return;
    const n = sync.next();
    sync.sent(n, hash, touched, changesLayers(op));
    send({ type: "rasterEdit", generation: view.generation, seq: n, op, label });
    paint();
  }

  /** The core's answer to an edit. @param {any} event */
  function applied(event) {
    later(async () => {
      if (!view || event.generation !== view.generation) return;
      view = { ...view, dirty: event.dirty, canUndo: event.canUndo, canRedo: event.canRedo };
      renderBar();
      await sync.answered(event);
    });
  }

  /** Tiles the core changed itself. @param {any} event */
  function changed(event) {
    later(async () => {
      if (!view || !copy || event.generation !== view.generation) return;
      await sync.changed(event.tiles);
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
    // The points queued this frame go to the copy before it is drawn.
    if (drawing?.queued.length) {
      copy.strokeAdd(Int32Array.from(drawing.queued));
      drawing.queued = [];
    }
    const c = camera();
    if (!c) return;
    const [w, h] = stageSize();
    // What changed since the last frame, laid together and uploaded; read
    // from the copy's memory before anything else is asked of it.
    const rects = copy.dirty();
    if (rects.length) gpu?.upload(rects, copy.composite());
    const ratio = window.devicePixelRatio || 1;
    gpu?.draw(c, w, h, ratio, { ...colors(), pixel: view.preset === "pixel", grid });
    drawCursor(c, w, h, ratio);
    renderBar();
  }

  /** Where the tip will land, over the picture: its outline, and the
   *  mirror lines when there are any. @param {{zoom: number, x: number, y: number}} c
   *  @param {number} w @param {number} h @param {number} ratio */
  function drawCursor(c, w, h, ratio) {
    overlay.width = Math.round(w * ratio);
    overlay.height = Math.round(h * ratio);
    overlay.style.width = `${w}px`;
    overlay.style.height = `${h}px`;
    const g = overlay.getContext("2d");
    if (!g || !view) return;
    g.setTransform(ratio, 0, 0, ratio, 0, 0);
    g.clearRect(0, 0, w, h);
    g.lineWidth = 1;
    if (mirror[0] || mirror[1]) {
      g.strokeStyle = "rgba(255, 255, 255, 0.35)";
      g.setLineDash([4, 4]);
      g.beginPath();
      if (mirror[0]) {
        const [x] = toStage(c, view.width / 2, 0);
        g.moveTo(Math.round(x) + 0.5, c.y);
        g.lineTo(Math.round(x) + 0.5, c.y + view.height * c.zoom);
      }
      if (mirror[1]) {
        const [, y] = toStage(c, 0, view.height / 2);
        g.moveTo(c.x, Math.round(y) + 0.5);
        g.lineTo(c.x + view.width * c.zoom, Math.round(y) + 0.5);
      }
      g.stroke();
      g.setLineDash([]);
    }
    if (!hover || tool === "pick" || tool === "fill") return;
    const n = tipSize;
    const start = -Math.floor((n - 1) / 2);
    const [x, y] = toStage(c, hover[0] + start, hover[1] + start);
    g.strokeStyle = "rgba(255, 255, 255, 0.8)";
    g.strokeRect(Math.round(x) + 0.5, Math.round(y) + 0.5, Math.round(n * c.zoom) - 1, Math.round(n * c.zoom) - 1);
    g.strokeStyle = "rgba(0, 0, 0, 0.6)";
    g.strokeRect(Math.round(x) - 0.5, Math.round(y) - 0.5, Math.round(n * c.zoom) + 1, Math.round(n * c.zoom) + 1);
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
  /** A stroke being drawn: the pointer drawing it, and points not yet given to the copy.
   *  @type {{pointer: number, queued: number[]} | null} */
  let drawing = null;
  /** A shape being dragged: where it began. @type {{pointer: number, from: [number, number]} | null} */
  let shaping = null;
  /** Whether a pen is down, so touch is let be (palm rejection). */
  let pen = false;

  const capture = (/** @type {PointerEvent} */ e) => {
    try {
      stage.setPointerCapture(e.pointerId);
    } catch {
      // A synthetic pointer has nothing to capture.
    }
  };
  /** A pointer as a canvas point. @param {PointerEvent} e @returns {[number, number] | null} */
  const at = (e) => {
    const c = camera();
    if (!c) return null;
    const [sx, sy] = stagePoint(e);
    return toCanvas(c, sx, sy);
  };
  const pixel = (/** @type {[number, number]} */ p) => /** @type {[number, number]} */ ([Math.floor(p[0]), Math.floor(p[1])]);
  /** A point as the engine takes it: x, y in 1/256 px and pressure. @param {PointerEvent} e */
  const triple = (e) => {
    const p = at(e);
    if (!p) return [];
    const pressure = e.pointerType === "pen" ? Math.round(Math.min(1, Math.max(0, e.pressure)) * 65535) : 65535;
    return [q8(p[0]), q8(p[1]), pressure];
  };
  /** @returns {import('./bindings/RasterTip').RasterTip} */
  const tip = () => ({ size: tipSize, round, mirror: [mirror[0] && view ? view.width : null, mirror[1] && view ? view.height : null] });
  /** The colour at a canvas point, laid together, as the colour drawn with. @param {[number, number]} p */
  function pickAt(p) {
    if (!copy || !view) return;
    const [x, y] = pixel(p);
    if (x < 0 || y < 0 || x >= view.width || y >= view.height) return;
    const c = copy.pick(x, y);
    if (c[3] === 0) return;
    color = rgba(c);
    renderColor();
    renderPalette();
  }
  /** The shape the drag says, from `from` to the pointer. @param {[number, number]} from @param {[number, number]} to @param {boolean} even */
  function shapeOp(from, to, even) {
    const layer = current();
    if (!layer) return null;
    let [x, y] = to;
    if (even) {
      // ⇧: a square or a circle; a line held to eighths of a turn.
      const [dx, dy] = [x - from[0], y - from[1]];
      if (tool === "line") {
        const step = Math.PI / 4;
        const angle = Math.round(Math.atan2(dy, dx) / step) * step;
        const length = Math.max(Math.abs(dx), Math.abs(dy));
        [x, y] = [from[0] + Math.round(Math.cos(angle) * length / Math.max(Math.abs(Math.cos(angle)), Math.abs(Math.sin(angle)) || 1)), from[1] + Math.round(Math.sin(angle) * length / Math.max(Math.abs(Math.cos(angle)), Math.abs(Math.sin(angle)) || 1))];
      } else {
        const side = Math.max(Math.abs(dx), Math.abs(dy));
        [x, y] = [from[0] + Math.sign(dx || 1) * side, from[1] + Math.sign(dy || 1) * side];
      }
    }
    return /** @type {Op} */ ({ op: "shape", layer: layer.id, shape: tool, from, to: [x, y], filled: tool !== "line" && filled, tip: tip(), color });
  }

  stage.addEventListener("pointerdown", (e) => {
    stage.focus({ preventScroll: true });
    const c = camera();
    if (!c || !view || !copy) return;
    if (space || e.button === 1) {
      panning = { from: stagePoint(e), at: [c.x, c.y] };
      capture(e);
      return;
    }
    if (e.button !== 0 || drawing || shaping) return;
    if (e.pointerType === "touch" && pen) return;
    if (e.pointerType === "pen") pen = true;
    const p = at(e);
    if (!p) return;
    if (tool === "pick" || e.altKey) return pickAt(p);
    const layer = current();
    if (!layer) return;
    if (tool === "fill") {
      const [x, y] = pixel(p);
      edit({ op: "fill", layer: layer.id, x, y, color, ...fillOptions }, "fill");
      return;
    }
    if (tool === "line" || tool === "rect" || tool === "ellipse") {
      shaping = { pointer: e.pointerId, from: pixel(p) };
      const op = shapeOp(shaping.from, shaping.from, false);
      if (op) copy.preview(op);
      capture(e);
      paint();
      return;
    }
    try {
      copy.strokeBegin({ layer: layer.id, tool, tip: tip(), color, pixelPerfect: tool === "pencil" && pixelPerfect });
    } catch (error) {
      // A locked layer, say: the core is asked anyway, so it says why.
      edit({ op: "stroke", layer: layer.id, tool: /** @type {any} */ (tool), tip: tip(), color, pixelPerfect, points: triple(e) }, tool);
      return;
    }
    drawing = { pointer: e.pointerId, queued: triple(e) };
    capture(e);
    paint();
  });

  stage.addEventListener("pointermove", (e) => {
    if (panning && cam) {
      const [sx, sy] = stagePoint(e);
      cam = { ...cam, x: panning.at[0] + sx - panning.from[0], y: panning.at[1] + sy - panning.from[1], moved: true };
      paint();
      return;
    }
    const p = at(e);
    hover = p ? pixel(p) : null;
    if (drawing && e.pointerId === drawing.pointer) {
      // Every point the pointer passed through, not only the last a frame.
      const all = typeof e.getCoalescedEvents === "function" ? e.getCoalescedEvents() : [];
      for (const each of all.length ? all : [e]) drawing.queued.push(...triple(/** @type {PointerEvent} */ (each)));
    } else if (shaping && e.pointerId === shaping.pointer && p && copy) {
      const op = shapeOp(shaping.from, pixel(p), e.shiftKey);
      if (op) copy.preview(op);
    }
    paint();
  });

  /** The stroke being drawn, finished and sent. */
  function finishStroke() {
    if (!drawing || !copy) return;
    if (drawing.queued.length) copy.strokeAdd(Int32Array.from(drawing.queued));
    drawing = null;
    const { op, hash } = copy.strokeEnd();
    send_(op, op.op === "stroke" && op.tool === "eraser" ? "erase" : "draw", hash, copy.touched());
  }

  /** The shape being dragged, sent. @param {PointerEvent} e */
  function finishShape(e) {
    if (!shaping || !copy) return;
    const p = at(e);
    const op = p ? shapeOp(shaping.from, pixel(p), e.shiftKey) : null;
    shaping = null;
    copy.clearPreview();
    if (op) edit(op, op.op === "shape" ? op.shape : "shape");
  }

  /** What is being drawn, taken back: nothing is sent. */
  function cancel() {
    if (drawing) {
      copy?.strokeCancel();
      drawing = null;
    }
    if (shaping) {
      copy?.clearPreview();
      shaping = null;
    }
    paint();
  }

  const end = (/** @type {PointerEvent} */ e) => {
    if (e.pointerType === "pen") pen = false;
    panning = null;
    if (drawing && e.pointerId === drawing.pointer) finishStroke();
    if (shaping && e.pointerId === shaping.pointer) finishShape(e);
  };
  stage.addEventListener("pointerup", end);
  stage.addEventListener("pointercancel", end);
  stage.addEventListener("pointerleave", () => {
    hover = null;
    paint();
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
      if (k === "z" && view && !typing && (drawing || shaping)) return (cancel(), true);
      if (k === "z" && view && !typing) return (send({ type: e.shiftKey ? "rasterRedo" : "rasterUndo" }), true);
      return false;
    }
    if (typing || e.altKey || e.ctrlKey || e.metaKey || !view) return false;
    if (e.key === "Escape" && (drawing || shaping)) return (cancel(), true);
    if (e.key === "?") return (host.keys?.("— Canvas's keys", KEYS), true);
    if (e.key === "f") return (refit(), true);
    if (e.key === "1") return (actualSize(), true);
    if (TOOL_KEYS[e.key]) return (useTool(TOOL_KEYS[e.key]), true);
    if (e.key === "[" || e.key === "]") {
      tipSize = Math.min(64, Math.max(1, tipSize + (e.key === "]" ? 1 : -1)));
      renderInspector();
      paint();
      return true;
    }
    if (e.key === "p") {
      pixelPerfect = !pixelPerfect;
      renderInspector();
      return true;
    }
    if (e.key === "m" || e.key === "M") {
      mirror = e.key === "M" ? [mirror[0], !mirror[1]] : [!mirror[0], mirror[1]];
      renderInspector();
      paint();
      return true;
    }
    if (e.key === "G") return (toggleGrid(), true);
    return false;
  }

  /* ── what the core says ──────────────────────────────────────── */

  function render() {
    renderHome();
    renderBar();
    renderAsk();
    renderLayers();
    renderInspector();
    renderPalette();
    renderColor();
    paint();
  }

  render();

  // For the window's tests: this copy's pixels, read from the section.
  /** @type {any} */ (ws).rasterProbe = {
    /** @param {number} x @param {number} y */
    pick: (x, y) => copy?.pick(x, y) ?? null,
    settled: () => queue.then(() => sync.settled()),
    sync: () => sync.state(),
    /** Where the middle of canvas pixel `x`, `y` is, in the window. @param {number} x @param {number} y */
    at: (x, y) => {
      const c = camera();
      if (!c) return null;
      const box = stage.getBoundingClientRect();
      const [sx, sy] = toStage(c, x + 0.5, y + 0.5);
      return [box.left + sx, box.top + sy];
    },
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
          cancel();
          sync.reset();
          copy = null;
          copyOf = 0;
          chosen = null;
          cam = null;
        } else if (opened) {
          chosen = null;
          cam = null;
          cancel();
          sync.reset();
          if (view.preset === "pixel" && view.palette.length) color = rgba(view.palette[0]);
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
    settled: () => queue.then(() => sync.settled()),
    /** A stage point as a canvas point. @param {number} sx @param {number} sy */
    toCanvas: (sx, sy) => (cam ? toCanvas(cam, sx, sy) : null),
  };
}
