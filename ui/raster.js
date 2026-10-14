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
 * A pencil or a brush is drawn as the pen moves: its points go to the copy a
 * frame at a time, and the finished stroke — every point, as the copy saw
 * them — is the edit sent. A brush's points carry the pen's pressure, from a
 * browser's pen or, in the app, from the shell's word of a tablet
 * (`raster-input.js`). A shape is shown as it is dragged and sent when let
 * go; a fill and a colour picked are a click. */

import { escapeHtml as esc } from "./html.js";
import { readRaster } from "./bridge.js";
import { createPen, createStabiliser, curved } from "./raster-input.js";
import { createRasterRuntime, FRAME_BUDGET, keepTiles, nextBudget } from "./raster-runtime.js";
import { createRasterRender } from "./raster-render.js";
import { begin as beginTransform, commit as commitTransform, corners, drag as dragTransform, handles, hit, wholeMove } from "./raster-transform.js";
import { createSync } from "./raster-sync.js";
import { fit, q8, toCanvas, toStage, zoomAt } from "./raster-view.js";

/** @typedef {import('./bindings/RasterFound').RasterFound} Found */
/** @typedef {import('./bindings/RasterView').RasterView} View */
/** @typedef {import('./bindings/RasterLayer').RasterLayer} Layer */
/** @typedef {import('./bindings/RasterOp').RasterOp} Op */
/** @typedef {import('./bindings/RasterBlend').RasterBlend} Blend */
/** @typedef {import('./bindings/RasterBrush').RasterBrush} Brush */
/** @typedef {import('./raster-input.js').Curve} Curve */
/** @typedef {{pointer: number, from: [number, number], to: [number, number], points: [number, number][], mode: string}} Selecting */
/** @typedef {{layer: number, rect: import('./raster-transform.js').Rect, t: import('./raster-transform.js').Transform, free: boolean,
 *            drag: {pointer: number, handle: import('./raster-transform.js').Handle, from: [number, number], start: import('./raster-transform.js').Transform} | null}} Moving */

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
  ["b", "brush on a paint canvas, pencil on a pixel one"], ["n", "the other of the two"], ["e g i", "eraser, fill, pick a colour"],
  ["l r o", "line, rectangle, ellipse"], ["[ ]", "a smaller, a larger tip or brush"], ["the pen's eraser end", "erases while it is near"],
  ["s", "select — a rectangle; again for an ellipse, a lasso"], ["w", "the magic wand"], ["⇧ ⌥ while selecting", "add to it, take from it; both, keep where both are"],
  ["⌘A ⌘D ⇧⌘I", "select all, nothing, the rest"], ["⌫", "clear what is selected"], ["⌘C ⌘X ⌘V", "copy, cut, paste as a layer"],
  ["v", "move what is selected — all of the layer if nothing is"], ["⌘T", "scale and turn it: ↩ keeps it, ⎋ leaves it"],
  ["p", "pixel-perfect"], ["m ⇧M", "mirror across, down"], ["⇧G", "the pixel grid"], ["⌥ click", "pick a colour"],
  ["⎋", "take back the stroke being drawn"], ["⌘S", "save the canvas"], ["⌘Z ⇧⌘Z", "undo, redo"], ["f", "the whole canvas"], ["1", "100 %"],
  ["two fingers", "move over the canvas"], ["pinch ⌘ wheel", "zoom"], ["␣ drag", "move over the canvas"], ["?", "these keys"],
]);

/** The tools, by the key that takes each: b is the brush on a paint canvas
 *  and the pencil on a pixel one. @param {string | undefined} preset */
const toolKeys = (preset) => /** @type {Record<string, string>} */ ({
  ...(preset === "paint" ? { b: "brush", n: "pencil" } : { b: "pencil", n: "brush" }),
  e: "eraser", g: "fill", i: "pick", l: "line", r: "rect", o: "ellipse", s: "select", w: "wand", v: "move",
});

/** The shapes the select tool makes, in the order s steps through them. */
const SELECT_SHAPES = /** @type {const} */ (["rect", "ellipse", "lasso"]);

/** Brushes to start from: a round one whose size follows the pen, an inking
 *  pen, a soft airbrush whose flow does, and a marker that builds up to its
 *  opacity and no further. */
const BRUSHES = /** @type {Record<string, {name: string, brush: Brush}>} */ ({
  round: { name: "Round", brush: { size: 16, hardness: 85, spacing: 8, flow: 255, opacity: 255, sizePressure: 60, flowPressure: 0, opacityPressure: 0 } },
  ink: { name: "Ink", brush: { size: 6, hardness: 100, spacing: 4, flow: 255, opacity: 255, sizePressure: 90, flowPressure: 0, opacityPressure: 0 } },
  soft: { name: "Soft", brush: { size: 80, hardness: 0, spacing: 10, flow: 40, opacity: 255, sizePressure: 0, flowPressure: 80, opacityPressure: 0 } },
  marker: { name: "Marker", brush: { size: 24, hardness: 70, spacing: 6, flow: 255, opacity: 160, sizePressure: 20, flowPressure: 0, opacityPressure: 60 } },
});

/** Where a person's brushes are kept between launches: in this window only. */
const BRUSHES_KEPT = "proton.canvas.brushes";

/** A brush with every number in its range, from whatever was kept.
 *  @param {any} b @param {Brush} fallback @returns {Brush} */
function brushOf(b, fallback) {
  const n = (/** @type {string} */ key, /** @type {number} */ lo, /** @type {number} */ hi) => {
    const v = Math.round(Number(b?.[key]));
    return Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : /** @type {any} */ (fallback)[key];
  };
  return {
    size: n("size", 1, 1000), hardness: n("hardness", 0, 100), spacing: n("spacing", 1, 200), flow: n("flow", 1, 255), opacity: n("opacity", 1, 255),
    sizePressure: n("sizePressure", 0, 100), flowPressure: n("flowPressure", 0, 100), opacityPressure: n("opacityPressure", 0, 100),
  };
}

/** Ops that change the layers themselves, not only their pixels. @param {Op} op */
const changesLayers = (op) =>
  ["addLayer", "removeLayer", "moveLayer", "setLayer", "setPalette", "paste", "orient", "resize", "crop", "upscale"].includes(op.op) ||
  (op.op === "replaceColor" && op.swatch !== undefined && op.swatch !== null) ||
  (op.op === "filter" && op.filter.kind === "quantize" && Boolean(op.filter.keep));

/** Each sprite template's size in its own pixels, a clear cell round it
 *  included — proton-raster's `gen::sprite`. */
const SPRITES = /** @type {Record<string, [number, number]>} */ ({ ship: [14, 14], creature: [14, 12], item: [12, 12] });

/** A seed nobody chose. */
const newSeed = () => Math.floor(Math.random() * 2 ** 32) >>> 0;

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
  /** The brush, the paint eraser's own, the stabiliser's string (0 to 100)
   *  and the pressure curve: a person's, kept between launches. */
  let brushes = (() => {
    /** @type {{brush: Brush, eraser: Brush, stabiliser: number, curve: Curve}} */
    const start = { brush: BRUSHES.round.brush, eraser: { ...BRUSHES.round.brush, size: 40, hardness: 60, sizePressure: 40 }, stabiliser: 0, curve: "linear" };
    try {
      const kept = JSON.parse(localStorage.getItem(BRUSHES_KEPT) ?? "null");
      if (!kept) return start;
      const curve = /** @type {Curve} */ (["soft", "linear", "firm"].includes(kept.curve) ? kept.curve : "linear");
      return { brush: brushOf(kept.brush, start.brush), eraser: brushOf(kept.eraser, start.eraser), stabiliser: Math.min(100, Math.max(0, Math.round(Number(kept.stabiliser)) || 0)), curve };
    } catch {
      return start;
    }
  })();
  const keepBrushes = () => {
    try {
      localStorage.setItem(BRUSHES_KEPT, JSON.stringify(brushes));
    } catch {
      // Kept only for convenience: a window that cannot keep it starts afresh.
    }
  };
  /** The pen: how hard it presses, from the shell's word of a tablet. */
  const stylus = createPen();
  /** @type {any} */ (host.root.defaultView ?? window).__protonPen = (/** @type {string} */ what, /** @type {number} */ p) => stylus.native(what, p);
  /** How much laying together a frame may do; more while it falls behind. */
  let budget = FRAME_BUDGET;
  /** The select tool's shape, how a selection joins what is selected, and
   *  its feather; the wand's own. */
  let selectShape = /** @type {"rect" | "ellipse" | "lasso"} */ ("rect");
  let selectOptions = { mode: "replace", feather: 0 };
  let wandOptions = { tolerance: 16, contiguous: true, diagonal: false };
  /** A selection being dragged out. @type {Selecting | null} */
  let selecting = null;
  /** A transform being made — of what is selected on a layer, or all of it:
   *  the layer, what was lifted and where, the transform so far, whether it
   *  has handles (⌘T) or is the move tool's, and a drag under way.
   *  @type {Moving | null} */
  let moving = null;
  /** The selection's edges as the copy last gave them, and the ants' march. */
  let ants = { version: -1, segments: new Int32Array(0), march: 0, timer: 0 };
  /** The core's version the copy was last loaded at. */
  let loaded = -1;
  /** The Make panel's settings: which generator and which filter, and each
   *  one's own. */
  let make = {
    generator: "sprite", filter: "outline", seed: newSeed(),
    template: "ship", scale: 2, ownColor: false,
    noise: "gradient", cell: 16, octaves: 4, colors: "ramp", steps: 5, mix: "dither", dither: 4,
    density: 46, rounds: 4, caveCell: 1, wrap: false,
    diagonal: false, inside: false,
    dx: 2, dy: 2, blur: 0, shade: 50,
    size: 4,
    count: 8, toPalette: false, ditherColours: 0, keep: true,
  };
  /** What the Make panel shows on the copy, not kept yet: the op, its name,
   *  and which part of the panel it is from. @type {{op: Op, label: string, part: string} | null} */
  let making = null;
  let filled = false;
  let grid = false;
  /** The pixel under the pointer, for the cursor. @type {[number, number] | null} */
  let hover = null;
  /** The point under the pointer, for a brush's cursor. @type {[number, number] | null} */
  let hoverAt = null;
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
    // s again: the next shape.
    if (which === "select" && tool === "select") selectShape = SELECT_SHAPES[(SELECT_SHAPES.indexOf(selectShape) + 1) % SELECT_SHAPES.length];
    finishTransform();
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
      `<button class="cc-quiet" data-raster-ramp title="Five steps of ${hex(color)}, darker and lighter, the hue turning as a painter's shading does">Ramp</button>` +
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
    if (target.closest("[data-raster-ramp]") && copy) {
      const ramp = copy.ramp({ base: color, steps: 5, shift: 25, spread: 24000 });
      const fresh = ramp.filter((c) => !view?.palette.some((p) => p.every((v, i) => v === c[i])));
      edit({ op: "setPalette", colors: [...view.palette, ...fresh] }, "ramp");
    }
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
          : tool === "select" || tool === "wand"
            ? selectFields(number, check)
          : tool === "move"
            ? `<p class="raster-faint">Drag to move what is selected — all of the layer when nothing is. ⌘T scales and turns it.</p>`
          : soft()
            ? brushFields(number)
          : number("size", tipSize, 1, 64, "Size") + check("round", round, "Round") +
            (tool === "pencil" ? check("pixelPerfect", pixelPerfect, "Pixel-perfect — no corners on steps") : "") +
            (tool === "rect" || tool === "ellipse" ? check("filled", filled, "Filled") : "") +
            check("mirrorX", mirror[0], "Mirror across the middle") + check("mirrorY", mirror[1], "Mirror down the middle");
    box.innerHTML =
      (moving?.free
        ? `<h5 class="raster-tool-head">Transform</h5><div class="raster-fields raster-buttons" data-raster-transform-tools>` +
          `<button class="cc-primary" data-raster-transform="keep" title="↩">Keep it</button><button class="cc-quiet" data-raster-transform="leave" title="⎋">Leave it</button>` +
          `<button class="cc-quiet" data-raster-transform="flipX" title="Mirror it across">⇋</button><button class="cc-quiet" data-raster-transform="flipY" title="Mirror it down">⇵</button></div>` +
          `<p class="raster-faint">Drag inside to move it, a corner to scale it (⇧: not in proportion), a side one way, outside to turn it (⇧: by fifteen degrees).</p>`
        : `<h5 class="raster-tool-head">${esc({ pencil: "Pencil", brush: "Brush", eraser: "Eraser", fill: "Fill", pick: "Pick a colour", line: "Line", rect: "Rectangle", ellipse: "Ellipse", select: "Select", wand: "Magic wand", move: "Move" }[tool] ?? tool)}</h5>` +
          `<div class="raster-fields" data-raster-options>${options}</div>`) +
      `<h5 class="raster-layer-head">Layer</h5>` +
      `<header class="raster-insp-head"><h4>${esc(l.name)}</h4></header>` +
      `<div class="raster-fields" data-raster-fields="${l.id}">` +
      `<label class="raster-field"><span>Name</span><input type="text" data-raster-field="name" value="${esc(l.name)}" spellcheck="false"></label>` +
      `<label class="raster-field"><span>Opacity</span><input type="range" min="0" max="255" value="${l.opacity}" data-raster-field="opacity"><small data-raster-opacity>${Math.round((l.opacity / 255) * 100)} %</small></label>` +
      `<label class="raster-field"><span>Blend</span><select data-raster-field="blend">${BLENDS.map(([b, name]) => `<option value="${b}"${b === l.blend ? " selected" : ""}>${name}</option>`).join("")}</select></label>` +
      `<label class="raster-field raster-check"><input type="checkbox" data-raster-field="alphaLock"${l.alphaLock ? " checked" : ""}> <span>Keep its shape — draw only where it has something</span></label>` +
      `<label class="raster-field raster-check"><input type="checkbox" data-raster-field="locked"${l.locked ? " checked" : ""}> <span>Locked</span></label>` +
      `</div>` +
      `<h5 class="raster-layer-head">Canvas</h5>` +
      `<div class="raster-fields raster-buttons" data-raster-canvas-tools>` +
      `<button class="cc-quiet" data-raster-canvas="ccw" title="Turn it a quarter to the left">↺</button><button class="cc-quiet" data-raster-canvas="cw" title="Turn it a quarter to the right">↻</button>` +
      `<button class="cc-quiet" data-raster-canvas="half" title="Turn it half round">180°</button><button class="cc-quiet" data-raster-canvas="flipX" title="Mirror it across">⇋</button>` +
      `<button class="cc-quiet" data-raster-canvas="flipY" title="Mirror it down">⇵</button><button class="cc-quiet" data-raster-canvas="crop" title="Cut it to what is selected"${antsNow().length ? "" : " disabled"}>Crop</button>` +
      `<button class="cc-quiet" data-raster-canvas="up2" title="Twice the size by Scale2x, a pixel picture's edges kept crisp">×2</button><button class="cc-quiet" data-raster-canvas="up3" title="Three times the size by Scale3x">×3</button></div>` +
      `<div class="raster-fields raster-resize"><label class="raster-field"><span>Size</span><span class="raster-size-pair"><input type="number" min="1" max="${view.preset === "pixel" ? 1024 : 4096}" value="${view.width}" data-raster-resize="width" aria-label="Width"> × <input type="number" min="1" max="${view.preset === "pixel" ? 1024 : 4096}" value="${view.height}" data-raster-resize="height" aria-label="Height"></span></label>` +
      `<button class="cc-quiet" data-raster-canvas="resize">Resize</button></div>`;
  }

  /** The select tool's and the wand's fields.
   *  @param {(name: string, value: number, min: number, max: number, label: string) => string} number
   *  @param {(name: string, on: boolean, label: string) => string} check */
  function selectFields(number, check) {
    const choose = (/** @type {string} */ name, /** @type {string} */ value, /** @type {string[][]} */ options, /** @type {string} */ label) =>
      `<label class="raster-field"><span>${label}</span><select data-raster-option="${name}">${options.map(([v, text]) => `<option value="${v}"${v === value ? " selected" : ""}>${text}</option>`).join("")}</select></label>`;
    return (
      (tool === "select"
        ? choose("select.shape", selectShape, [["rect", "Rectangle"], ["ellipse", "Ellipse"], ["lasso", "Lasso"]], "Shape")
        : number("wand.tolerance", wandOptions.tolerance, 0, 255, "Tolerance") + check("wand.contiguous", wandOptions.contiguous, "Only what touches it") + check("wand.diagonal", wandOptions.diagonal, "Across corners too")) +
      choose("select.mode", selectOptions.mode, [["replace", "In place of what is selected"], ["add", "Added — ⇧"], ["subtract", "Taken away — ⌥"], ["intersect", "Where both are — ⇧⌥"]], "Joins") +
      number("select.feather", selectOptions.feather, 0, 250, "Feather")
    );
  }

  /** Whether the tool lays dabs — the brush, and the eraser on a paint
   *  canvas — rather than pixels. */
  function soft() {
    return tool === "brush" || (tool === "eraser" && view?.preset === "paint");
  }

  /** The brush the tool uses: the brush's, or the paint eraser's own. */
  const brushFor = () => (tool === "eraser" ? "eraser" : "brush");

  /** The brush's fields in the inspector.
   *  @param {(name: string, value: number, min: number, max: number, label: string) => string} number */
  function brushFields(number) {
    const b = brushes[brushFor()];
    const same = (/** @type {Brush} */ x) => /** @type {(keyof Brush)[]} */ (Object.keys(x)).every((k) => x[k] === b[k]);
    const preset = Object.entries(BRUSHES).find(([, p]) => same(p.brush))?.[0] ?? "";
    const pct = (/** @type {number} */ v) => `${Math.round((v / 255) * 100)} %`;
    const range = (/** @type {string} */ name, /** @type {number} */ value, /** @type {number} */ min, /** @type {number} */ max, /** @type {string} */ label, /** @type {string} */ shown) =>
      `<label class="raster-field"><span>${label}</span><input type="range" min="${min}" max="${max}" value="${value}" data-raster-option="${name}"><small>${shown}</small></label>`;
    return (
      `<label class="raster-field"><span>Brush</span><select data-raster-option="brush.preset"><option value=""${preset ? "" : " selected"}>Your own</option>` +
      Object.entries(BRUSHES).map(([k, p]) => `<option value="${k}"${k === preset ? " selected" : ""}>${p.name}</option>`).join("") +
      `</select></label>` +
      number("brush.size", b.size, 1, 1000, "Size") +
      range("brush.hardness", b.hardness, 0, 100, "Hardness", `${b.hardness} %`) +
      range("brush.spacing", b.spacing, 1, 200, "Spacing", `${b.spacing} %`) +
      range("brush.flow", b.flow, 1, 255, "Flow", pct(b.flow)) +
      range("brush.opacity", b.opacity, 1, 255, "Opacity", pct(b.opacity)) +
      `<h6 class="raster-sub">From the pen's pressure</h6>` +
      range("brush.sizePressure", b.sizePressure, 0, 100, "Size", `${b.sizePressure} %`) +
      range("brush.flowPressure", b.flowPressure, 0, 100, "Flow", `${b.flowPressure} %`) +
      range("brush.opacityPressure", b.opacityPressure, 0, 100, "Opacity", `${b.opacityPressure} %`) +
      `<label class="raster-field"><span>Curve</span><select data-raster-option="curve">` +
      [["soft", "Soft — a light touch goes far"], ["linear", "Even"], ["firm", "Firm — press for more"]].map(([v, name]) => `<option value="${v}"${v === brushes.curve ? " selected" : ""}>${name}</option>`).join("") +
      `</select></label>` +
      range("stabiliser", brushes.stabiliser, 0, 100, "Stabiliser", `${brushes.stabiliser}`)
    );
  }

  const inspector = q("[data-raster-inspector]");
  inspector.addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const t = /** @type {HTMLElement | null} */ (target.closest("[data-raster-transform]"))?.dataset.rasterTransform;
    if (t === "keep") return finishTransform();
    if (t === "leave") return endTransform();
    if ((t === "flipX" || t === "flipY") && moving) {
      moving.t = t === "flipX" ? { ...moving.t, sx: -moving.t.sx } : { ...moving.t, sy: -moving.t.sy };
      paint();
      return;
    }
    const whichCanvas = /** @type {HTMLElement | null} */ (target.closest("[data-raster-canvas]"))?.dataset.rasterCanvas;
    if (!whichCanvas || !view) return;
    if (whichCanvas === "ccw") whole({ op: "orient", turns: 3, flip: false }, "turn left");
    if (whichCanvas === "cw") whole({ op: "orient", turns: 1, flip: false }, "turn right");
    if (whichCanvas === "half") whole({ op: "orient", turns: 2, flip: false }, "turn round");
    if (whichCanvas === "flipX") whole({ op: "orient", turns: 0, flip: true }, "mirror across");
    if (whichCanvas === "flipY") whole({ op: "orient", turns: 2, flip: true }, "mirror down");
    if (whichCanvas === "up2" || whichCanvas === "up3") whole({ op: "upscale", factor: whichCanvas === "up2" ? 2 : 3 }, whichCanvas === "up2" ? "grow ×2" : "grow ×3");
    if (whichCanvas === "crop") {
      const rect = selectedRect();
      if (rect) whole({ op: "crop", rect: /** @type {[number, number, number, number]} */ (rect) }, "crop");
    }
    if (whichCanvas === "resize") {
      const size = (/** @type {string} */ which) => Math.round(Number(/** @type {HTMLInputElement | null} */ (inspector.querySelector(`[data-raster-resize="${which}"]`))?.value));
      const [width, height] = [size("width"), size("height")];
      if (width > 0 && height > 0) whole({ op: "resize", width, height, smooth: view.preset === "paint" }, "resize");
    }
  });
  // While opacity is dragged, only this window's copy shows it; one edit
  // goes to the core when it is let go.
  inspector.addEventListener("input", (e) => {
    const field = /** @type {HTMLInputElement} */ (e.target);
    // A brush's slider says its number as it moves; it is kept when let go.
    const option = field.dataset.rasterOption;
    if (option && field.type === "range") {
      const shown = field.nextElementSibling;
      const v = Number(field.value);
      if (shown) shown.textContent = option === "brush.flow" || option === "brush.opacity" ? `${Math.round((v / 255) * 100)} %` : option === "stabiliser" ? `${v}` : `${v} %`;
      return;
    }
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
    if (option?.startsWith("brush.") || option === "stabiliser" || option === "curve") {
      const which = brushFor();
      const key = option.slice("brush.".length);
      if (option === "brush.preset") {
        const chosen = BRUSHES[field.value];
        if (chosen) brushes = { ...brushes, [which]: { ...chosen.brush } };
      } else if (option === "stabiliser") {
        brushes = { ...brushes, stabiliser: Math.min(100, Math.max(0, Math.round(Number(field.value)) || 0)) };
      } else if (option === "curve") {
        brushes = { ...brushes, curve: /** @type {Curve} */ (field.value) };
      } else {
        brushes = { ...brushes, [which]: brushOf({ ...brushes[which], [key]: Number(field.value) }, brushes[which]) };
      }
      keepBrushes();
      renderInspector();
      paint();
      return;
    }
    if (option?.startsWith("select.") || option?.startsWith("wand.")) {
      const n = Math.round(Number(field.value));
      if (option === "select.shape") selectShape = /** @type {"rect" | "ellipse" | "lasso"} */ (field.value);
      if (option === "select.mode") selectOptions = { ...selectOptions, mode: field.value };
      if (option === "select.feather") selectOptions = { ...selectOptions, feather: Math.min(250, Math.max(0, n || 0)) };
      if (option === "wand.tolerance") wandOptions = { ...wandOptions, tolerance: Math.min(255, Math.max(0, n || 0)) };
      if (option === "wand.contiguous") wandOptions = { ...wandOptions, contiguous: field.checked };
      if (option === "wand.diagonal") wandOptions = { ...wandOptions, diagonal: field.checked };
      renderInspector();
      return;
    }
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
    if (which !== "make") leaveMade();
    else renderMake();
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

  /* ── the Make panel ──────────────────────────────────────────── */

  /** The generator the Make panel is set to, as an op on the layer chosen. */
  function generatorOp() {
    const layer = current();
    if (!layer || !view || !copy) return null;
    const seed = make.seed >>> 0;
    if (make.generator === "sprite") {
      // In the middle of what is selected, or of the canvas.
      const [sw, sh] = SPRITES[make.template] ?? [14, 14];
      const r = selectedRect() ?? [0, 0, view.width, view.height];
      const at = [Math.round(r[0] + (r[2] - sw * make.scale) / 2), Math.round(r[1] + (r[3] - sh * make.scale) / 2)];
      return /** @type {Op} */ ({ op: "generate", layer: layer.id, generator: { kind: "sprite", template: make.template, seed, at, scale: make.scale, color: make.ownColor ? color : undefined } });
    }
    if (make.generator === "noise") {
      const colors = make.colors === "palette" && view.palette.length >= 2 ? view.palette : copy.ramp({ base: color, steps: make.steps, shift: 25, spread: 26000 });
      return /** @type {Op} */ ({ op: "generate", layer: layer.id, generator: { kind: "noise", noise: make.noise, seed, cell: make.cell, octaves: make.octaves, colors, mix: make.mix, dither: make.dither } });
    }
    return /** @type {Op} */ ({ op: "generate", layer: layer.id, generator: { kind: "cave", seed, density: make.density, steps: make.rounds, cell: make.caveCell, wrap: make.wrap, color } });
  }

  /** The filter the Make panel is set to, as an op on the layer chosen. */
  function filterOp() {
    const layer = current();
    if (!layer) return null;
    const filter =
      make.filter === "outline" ? { kind: "outline", color, diagonal: make.diagonal, inside: make.inside }
      : make.filter === "shadow" ? { kind: "shadow", dx: make.dx, dy: make.dy, color: [0, 0, 0, Math.round((make.shade * 255) / 100)], blur: make.blur }
      : make.filter === "pixelate" ? { kind: "pixelate", size: make.size }
      : { kind: "quantize", colors: make.count, palette: make.toPalette, dither: make.ditherColours, keep: make.keep };
    return /** @type {Op} */ ({ op: "filter", layer: layer.id, filter });
  }

  /** What a part of the panel makes, by name. @param {string} part */
  const madeLabel = (part) => (part === "generator" ? { sprite: "sprite", noise: "noise", cave: "caves" }[make.generator] : { outline: "outline", shadow: "shadow", pixelate: "pixelate", quantize: "fewer colours" }[make.filter]) ?? part;

  /** A part of the panel shown on the copy, and not kept. @param {string} part */
  function showMade(part) {
    leaveMade();
    const op = part === "generator" ? generatorOp() : filterOp();
    if (!op || !copy) return;
    try {
      copy.preview(op);
      making = { op, label: madeLabel(part), part };
    } catch (error) {
      making = null;
      console.warn("proton: canvas", error);
    }
    renderMakeNote();
    paint();
  }

  /** What was shown, taken away. */
  function leaveMade() {
    if (!making) return;
    making = null;
    copy?.clearPreview();
    renderMakeNote();
    paint();
  }

  /** A part of the panel kept, as one edit. @param {string} part */
  function keepMade(part) {
    const op = making?.part === part ? making.op : part === "generator" ? generatorOp() : filterOp();
    leaveMade();
    if (op) edit(op, madeLabel(part));
  }

  function renderMakeNote() {
    const note = ws.querySelector("[data-make-note]");
    if (note) note.textContent = making ? `Showing the ${making.label} on ${current()?.name ?? "the layer"}, not kept yet.` : "A change shows it on the layer chosen, held to what is selected.";
  }

  function renderMake() {
    const box = q("[data-raster-make]");
    if (!view) {
      box.innerHTML = "";
      return;
    }
    const field = (/** @type {string} */ name, /** @type {number} */ min, /** @type {number} */ max, /** @type {string} */ label) =>
      `<label class="raster-field"><span>${label}</span><input type="number" min="${min}" max="${max}" value="${/** @type {any} */ (make)[name]}" data-make="${name}"></label>`;
    const check = (/** @type {string} */ name, /** @type {string} */ label) =>
      `<label class="raster-field raster-check"><input type="checkbox" data-make="${name}"${/** @type {any} */ (make)[name] ? " checked" : ""}> <span>${label}</span></label>`;
    const choose = (/** @type {string} */ name, /** @type {string[][]} */ options, /** @type {string} */ label) =>
      `<label class="raster-field"><span>${label}</span><select data-make="${name}">${options.map(([v, text]) => `<option value="${v}"${String(/** @type {any} */ (make)[name]) === v ? " selected" : ""}>${text}</option>`).join("")}</select></label>`;
    const generator =
      make.generator === "sprite"
        ? choose("template", [["ship", "A ship"], ["creature", "A creature"], ["item", "An item"]], "Kind") + field("scale", 1, 16, "Scale") + check("ownColor", "In a ramp of the colour drawn with")
        : make.generator === "noise"
          ? choose("noise", [["gradient", "Gradient"], ["value", "Value"]], "Noise") + field("cell", 1, 512, "Cell") + field("octaves", 1, 8, "Octaves") +
            choose("colors", [["ramp", "A ramp of the colour drawn with"], ["palette", "The palette"]], "Colours") + (make.colors === "ramp" ? field("steps", 2, 16, "Steps") : "") +
            choose("mix", [["dither", "Dithered"], ["nearest", "Banded"], ["smooth", "Smooth"]], "Laid out") + (make.mix === "dither" ? choose("dither", [["2", "2 × 2"], ["4", "4 × 4"], ["8", "8 × 8"]], "Dither") : "")
          : field("density", 1, 99, "Rock %") + field("rounds", 0, 10, "Rounds") + field("caveCell", 1, 64, "Cell") + check("wrap", "Repeats as a tile");
    const filter =
      make.filter === "outline" ? check("diagonal", "Across corners too") + check("inside", "On its own edge, not round it")
      : make.filter === "shadow" ? field("dx", -64, 64, "Across") + field("dy", -64, 64, "Down") + field("blur", 0, 64, "Blur") + field("shade", 1, 100, "Strength %")
      : make.filter === "pixelate" ? field("size", 2, 128, "Square")
      : field("count", 2, 64, "Colours") + check("toPalette", "The palette's colours instead") +
        choose("ditherColours", [["0", "Not dithered"], ["2", "Dithered 2 × 2"], ["4", "Dithered 4 × 4"], ["8", "Dithered 8 × 8"]], "Dither") + check("keep", "Keep them as the palette");
    box.innerHTML =
      `<h5 class="raster-tool-head">Make</h5><div class="raster-fields" data-make-part="generator">` +
      choose("generator", [["sprite", "A sprite"], ["noise", "Noise"], ["cave", "Caves"]], "What") + generator +
      `<label class="raster-field"><span>Seed</span><span class="raster-size-pair"><input type="number" min="0" max="4294967295" value="${make.seed}" data-make="seed"><button class="cc-quiet" data-make-action="reseed" title="Another seed">⟳</button></span></label>` +
      `<div class="raster-buttons"><button class="cc-quiet" data-make-action="show-generator">Show it</button><button class="cc-primary" data-make-action="keep-generator">Keep it</button><button class="cc-quiet" data-make-action="leave">Leave it</button></div></div>` +
      `<h5 class="raster-layer-head">Filter</h5><div class="raster-fields" data-make-part="filter">` +
      choose("filter", [["outline", "Outline"], ["shadow", "Shadow"], ["pixelate", "Pixelate"], ["quantize", "Fewer colours"]], "What") + filter +
      `<div class="raster-buttons"><button class="cc-quiet" data-make-action="show-filter">Show it</button><button class="cc-primary" data-make-action="keep-filter">Keep it</button><button class="cc-quiet" data-make-action="leave">Leave it</button></div></div>` +
      `<p class="raster-faint" data-make-note></p>`;
    renderMakeNote();
  }

  const makeBox = q("[data-raster-make]");
  makeBox.addEventListener("change", (e) => {
    const field = /** @type {HTMLInputElement} */ (e.target);
    const name = field.dataset.make;
    if (!name) return;
    const was = /** @type {any} */ (make)[name];
    /** @type {any} */ (make)[name] = field.type === "checkbox" ? field.checked : typeof was === "number" ? Math.round(Number(field.value)) || 0 : field.value;
    const part = /** @type {HTMLElement | null} */ (field.closest("[data-make-part]"))?.dataset.makePart ?? "generator";
    renderMake();
    showMade(part);
  });
  makeBox.addEventListener("click", (e) => {
    const action = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-make-action]"))?.dataset.makeAction;
    if (action === "reseed") {
      make = { ...make, seed: newSeed() };
      renderMake();
      showMade("generator");
    }
    if (action === "show-generator") showMade("generator");
    if (action === "show-filter") showMade("filter");
    if (action === "keep-generator") keepMade("generator");
    if (action === "keep-filter") keepMade("filter");
    if (action === "leave") leaveMade();
  });

  /* ── edits ───────────────────────────────────────────────────── */

  const sync = createSync({
    fetch: async (layer, tiles) => {
      if (!view) return { lastSeq: 0, bytes: new ArrayBuffer(24) };
      const bytes = await readRaster(view.generation, layer, tiles);
      return { lastSeq: new DataView(bytes).getUint32(16, true), bytes };
    },
    put: (layer, bytes, keep) => copy?.put(layer, keepTiles(bytes, keep).bytes),
    // After whatever the core said with it — a new size, new layers — and
    // once a version, however many ask.
    reload: async () => {
      later(async () => {
        if (view && loaded !== view.version) await reload();
      });
    },
    painted: () => paint(),
  });

  /** An op: made on this window's copy, then sent. @param {Op} op @param {string} label */
  function edit(op, label) {
    if (!view || !copy) return;
    // A transform under way is kept first: it is what the person sees.
    if (moving) finishTransform();
    // What the Make panel shows is taken away: it is not kept unless asked.
    if (making) leaveMade();
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

  /** A change to the whole canvas — turned, mirrored, resized, cropped —
   *  made by the core alone; the copy is loaded again after, at its new
   *  size. @param {Op} op @param {string} label */
  function whole(op, label) {
    if (!view) return;
    finishTransform();
    cancel();
    send_(op, label, null, []);
  }

  /** How a selection joins what is selected: ⇧ adds, ⌥ takes away, both
   *  keep only where both are; otherwise as the inspector says.
   *  @param {MouseEvent} e */
  const modeOf = (e) => (e.shiftKey && e.altKey ? "intersect" : e.shiftKey ? "add" : e.altKey ? "subtract" : selectOptions.mode);

  /** A selection made. @param {any} area @param {string} [mode] */
  function select(area, mode = "replace") {
    if (!view) return;
    edit(/** @type {Op} */ ({ op: "select", area, mode, feather: selectOptions.feather, smooth: view.preset === "paint" }), area.kind === "nothing" ? "select nothing" : "select");
  }

  /** The whole pixels from one point's to another's, both in: left, top,
   *  width, height. @param {[number, number]} a @param {[number, number]} b */
  const pixelRect = (a, b) => {
    const [x0, y0] = pixel([Math.min(a[0], b[0]), Math.min(a[1], b[1])]);
    const [x1, y1] = pixel([Math.max(a[0], b[0]), Math.max(a[1], b[1])]);
    return [x0, y0, x1 - x0 + 1, y1 - y0 + 1];
  };

  /** The selection being dragged out, made. */
  function finishSelecting() {
    const s = selecting;
    selecting = null;
    if (!s || !view) return paint();
    const small = !cam || Math.hypot((s.to[0] - s.from[0]) * cam.zoom, (s.to[1] - s.from[1]) * cam.zoom) < 3;
    if (selectShape === "lasso") {
      if (s.points.length >= 3 && !small) select({ kind: "lasso", points: s.points.flatMap(([x, y]) => [q8(x), q8(y)]) }, s.mode);
      else if (s.mode === "replace") select({ kind: "nothing" });
      return paint();
    }
    // A click selects nothing, as a drag of no size would.
    if (small) {
      if (s.mode === "replace") select({ kind: "nothing" });
      return paint();
    }
    select({ kind: selectShape, rect: pixelRect(s.from, s.to) }, s.mode);
  }

  /** The selection's edges, from the copy when it has changed. */
  function antsNow() {
    if (!copy) return new Int32Array(0);
    const v = copy.selectionVersion();
    if (v !== ants.version) {
      ants.version = v;
      ants.segments = copy.outline();
      // Crop needs something selected to crop to.
      const crop = /** @type {HTMLButtonElement | null} */ (ws.querySelector('[data-raster-canvas="crop"]'));
      if (crop) crop.disabled = !ants.segments.length;
    }
    return ants.segments;
  }

  /** The rectangle round what is selected, by its edges: left, top, width,
   *  height; none when nothing is. */
  function selectedRect() {
    const segs = antsNow();
    if (!segs.length) return null;
    let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
    for (let i = 0; i < segs.length; i += 2) {
      x0 = Math.min(x0, segs[i]);
      x1 = Math.max(x1, segs[i]);
      y0 = Math.min(y0, segs[i + 1]);
      y1 = Math.max(y1, segs[i + 1]);
    }
    return x1 > x0 && y1 > y0 ? [x0, y0, x1 - x0, y1 - y0] : null;
  }

  /** What is selected of the layer chosen — all of it, with nothing
   *  selected — lifted to be moved: drawn by the GPU where it is going, what
   *  is left of the layer shown under it. Whether there was anything.
   *  @param {boolean} free with handles, to scale and turn it */
  function startTransform(free) {
    if (moving) return true;
    if (!view || !copy) return false;
    cancel();
    const layer = current();
    if (!layer || layer.locked) return false;
    const lifted = copy.lift(layer.id);
    if (!lifted.width) return false;
    gpu?.float(lifted.pixels, lifted.width, lifted.height);
    copy.preview({ op: "clear", layer: layer.id });
    const rect = { x: lifted.x, y: lifted.y, width: lifted.width, height: lifted.height };
    moving = { layer: layer.id, rect, t: beginTransform(rect), free, drag: null };
    renderInspector();
    paint();
    return true;
  }

  /** The transform kept, as one edit. */
  function finishTransform() {
    if (!moving || !view) return;
    const { layer, rect, t } = moving;
    endTransform();
    if (wholeMove(t) && t.dx === 0 && t.dy === 0) return;
    const made = commitTransform(t, rect, view.width, view.height);
    const label = wholeMove(t) ? "move" : "transform";
    // Moved wholly off the canvas, or squashed to nothing: gone.
    edit(/** @type {Op} */ (made ? { op: "transform", layer, bounds: made.bounds, inverse: made.inverse, smooth: view.preset === "paint" && !wholeMove(t) } : { op: "clear", layer }), label);
  }

  /** The transform left: everything as it was. */
  function endTransform() {
    if (!moving) return;
    moving = null;
    copy?.clearPreview();
    gpu?.float(null);
    renderInspector();
    paint();
  }

  /** The core's answer to an edit. @param {any} event */
  function applied(event) {
    later(async () => {
      if (!view || event.generation !== view.generation) return;
      // The core's version too: a copy loaded at an older one is behind.
      view = { ...view, version: event.version, dirty: event.dirty, canUndo: event.canUndo, canRedo: event.canRedo };
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
    loaded = view.version;
    const next = await createRasterRuntime();
    next.open({ width: view.width, height: view.height, preset: view.preset, palette: view.palette, layers: view.layers, generation: view.generation });
    copy = next;
    copyOf = view.generation;
    // A new copy counts its selection's changes from nothing again.
    ants.version = -1;
    gpu?.resize(view.width, view.height, view.preset === "paint");
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
    const rects = copy.dirty(budget);
    if (rects.length) gpu?.upload(rects, copy.composite());
    // What did not fit this frame is laid in the next, given more.
    const behind = copy.pending() > 0;
    budget = nextBudget(budget, behind);
    const ratio = window.devicePixelRatio || 1;
    const float = moving ? { corners: corners(moving.t, moving.rect).map(([x, y]) => toStage(c, x, y)), nearest: view.preset === "pixel" } : undefined;
    gpu?.draw(c, w, h, ratio, { ...colors(), pixel: view.preset === "pixel", grid, still: !drawing && !behind && !moving, float });
    drawCursor(c, w, h, ratio);
    renderBar();
    if (behind) paint();
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
    drawSelection(g, c);
    if (moving) return drawTransform(g, c);
    if (selecting) return drawSelecting(g, c, selecting);
    if (!hover || tool === "pick" || tool === "fill" || tool === "select" || tool === "wand" || tool === "move") return;
    if (soft() && hoverAt) {
      // A brush's size at full pressure, about where the pen is.
      const [x, y] = toStage(c, hoverAt[0], hoverAt[1]);
      const r = Math.max(2, (brushes[brushFor()].size * c.zoom) / 2);
      g.beginPath();
      g.arc(x, y, r, 0, Math.PI * 2);
      g.strokeStyle = "rgba(0, 0, 0, 0.6)";
      g.lineWidth = 3;
      g.stroke();
      g.strokeStyle = "rgba(255, 255, 255, 0.85)";
      g.lineWidth = 1;
      g.stroke();
      return;
    }
    const n = tipSize;
    const start = -Math.floor((n - 1) / 2);
    const [x, y] = toStage(c, hover[0] + start, hover[1] + start);
    g.strokeStyle = "rgba(255, 255, 255, 0.8)";
    g.strokeRect(Math.round(x) + 0.5, Math.round(y) + 0.5, Math.round(n * c.zoom) - 1, Math.round(n * c.zoom) - 1);
    g.strokeStyle = "rgba(0, 0, 0, 0.6)";
    g.strokeRect(Math.round(x) - 0.5, Math.round(y) - 0.5, Math.round(n * c.zoom) + 1, Math.round(n * c.zoom) + 1);
  }
  /** The marching ants round what is selected.
   *  @param {CanvasRenderingContext2D} g @param {{zoom: number, x: number, y: number}} c */
  function drawSelection(g, c) {
    const segs = antsNow();
    if (!segs.length || moving) return;
    g.save();
    g.beginPath();
    for (let i = 0; i < segs.length; i += 4) {
      const [x0, y0] = toStage(c, segs[i], segs[i + 1]);
      const [x1, y1] = toStage(c, segs[i + 2], segs[i + 3]);
      g.moveTo(Math.round(x0) + 0.5, Math.round(y0) + 0.5);
      g.lineTo(Math.round(x1) + 0.5, Math.round(y1) + 0.5);
    }
    g.lineWidth = 1;
    g.setLineDash([4, 4]);
    g.lineDashOffset = ants.march;
    g.strokeStyle = "rgba(0, 0, 0, 0.9)";
    g.stroke();
    g.lineDashOffset = ants.march + 4;
    g.strokeStyle = "rgba(255, 255, 255, 0.95)";
    g.stroke();
    g.restore();
    // They march while they are shown.
    if (!ants.timer && shown) {
      ants.timer = window.setTimeout(() => {
        ants.timer = 0;
        ants.march = (ants.march + 1) % 8;
        paint();
      }, 100);
    }
  }

  /** A selection being dragged out, as its shape.
   *  @param {CanvasRenderingContext2D} g @param {{zoom: number, x: number, y: number}} c @param {Selecting} s */
  function drawSelecting(g, c, s) {
    g.save();
    g.beginPath();
    if (selectShape === "lasso") {
      s.points.forEach(([x, y], i) => {
        const [sx, sy] = toStage(c, x, y);
        if (i === 0) g.moveTo(sx, sy);
        else g.lineTo(sx, sy);
      });
      g.closePath();
    } else {
      const [x, y, w, h] = pixelRect(s.from, s.to);
      const [sx, sy] = toStage(c, x, y);
      if (selectShape === "ellipse") g.ellipse(sx + (w * c.zoom) / 2, sy + (h * c.zoom) / 2, (w * c.zoom) / 2, (h * c.zoom) / 2, 0, 0, Math.PI * 2);
      else g.rect(Math.round(sx) + 0.5, Math.round(sy) + 0.5, Math.round(w * c.zoom), Math.round(h * c.zoom));
    }
    g.lineWidth = 1;
    g.setLineDash([4, 4]);
    g.strokeStyle = "rgba(0, 0, 0, 0.9)";
    g.stroke();
    g.lineDashOffset = 4;
    g.strokeStyle = "rgba(255, 255, 255, 0.95)";
    g.stroke();
    g.restore();
  }

  /** What is being moved: its box, and with ⌘T its handles.
   *  @param {CanvasRenderingContext2D} g @param {{zoom: number, x: number, y: number}} c */
  function drawTransform(g, c) {
    if (!moving) return;
    const [a, b, cc, d] = corners(moving.t, moving.rect).map(([x, y]) => toStage(c, x, y));
    g.save();
    g.beginPath();
    g.moveTo(a[0], a[1]);
    g.lineTo(b[0], b[1]);
    g.lineTo(d[0], d[1]);
    g.lineTo(cc[0], cc[1]);
    g.closePath();
    g.lineWidth = 1;
    g.strokeStyle = "rgba(255, 255, 255, 0.9)";
    g.stroke();
    if (moving.free) {
      for (const [, hx, hy] of handles(moving.t, moving.rect)) {
        const [x, y] = toStage(c, hx, hy);
        g.fillStyle = "rgba(255, 255, 255, 0.95)";
        g.strokeStyle = "rgba(0, 0, 0, 0.7)";
        g.fillRect(Math.round(x) - 3.5, Math.round(y) - 3.5, 7, 7);
        g.strokeRect(Math.round(x) - 3.5, Math.round(y) - 3.5, 7, 7);
      }
    }
    g.restore();
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
  /** A stroke being drawn: the pointer drawing it, points not yet given to
   *  the copy, where its pressure comes from, its stabiliser, and the pen's
   *  last point (canvas pixels and pressure).
   *  @type {{pointer: number, queued: number[], source: "pointer" | "native" | "none",
   *          steady: ReturnType<typeof createStabiliser> | null, last: [number, number, number] | null} | null} */
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
  /** A pointer event's point as the engine takes it — x, y in 1/256 px and
   *  a pressure from 0 to 65535 — through the stroke's stabiliser: none,
   *  while its string is slack. @param {PointerEvent} e */
  const triple = (e) => {
    const p = at(e);
    if (!p || !drawing) return [];
    const pressure = curved(stylus.pressure(drawing.source, e), brushes.curve);
    drawing.last = [p[0], p[1], pressure];
    const steady = drawing.steady ? drawing.steady.add(p[0], p[1], pressure) : [p[0], p[1], pressure];
    return steady.length ? [q8(steady[0]), q8(steady[1]), steady[2]] : [];
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
    // What the Make panel shows goes before anything is drawn on it: taken
    // away later, it would take the drawing with it.
    leaveMade();
    if (e.pointerType === "pen") pen = true;
    const p = at(e);
    if (!p) return;
    // A transform under way takes the pointer: a handle, inside to move it,
    // outside to turn it.
    if (moving) {
      const handle = moving.free ? hit(moving.t, moving.rect, p[0], p[1], 8 / c.zoom) : "move";
      moving.drag = { pointer: e.pointerId, handle, from: p, start: moving.t };
      capture(e);
      return;
    }
    if (tool === "select") {
      selecting = { pointer: e.pointerId, from: p, to: p, points: [p], mode: modeOf(e) };
      capture(e);
      paint();
      return;
    }
    if (tool === "wand") {
      const layer = current();
      const [x, y] = pixel(p);
      if (layer) select({ kind: "wand", layer: layer.id, x, y, ...wandOptions }, modeOf(e));
      return;
    }
    if (tool === "move") {
      if (startTransform(false)) {
        // Lifted just now, by startTransform.
        const m = /** @type {Moving} */ (/** @type {unknown} */ (moving));
        m.drag = { pointer: e.pointerId, handle: "move", from: p, start: m.t };
        capture(e);
      }
      return;
    }
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
    // The pen's eraser end erases, whatever the tool.
    const erasing = tool === "eraser" || stylus.erasing();
    const dabs = tool === "brush" || (erasing && view.preset === "paint");
    const brush = brushes[erasing ? "eraser" : "brush"];
    const source = stylus.source(e);
    drawing = {
      pointer: e.pointerId,
      queued: [],
      source,
      // A string in screen pixels, held at its length however the canvas is seen.
      steady: dabs && brushes.stabiliser > 0 ? createStabiliser((brushes.stabiliser * 0.3) / (c.zoom || 1)) : null,
      last: null,
    };
    const first = triple(e);
    try {
      copy.strokeBegin(dabs ? { layer: layer.id, tool: erasing ? "eraser" : "pencil", brush, color } : { layer: layer.id, tool: erasing ? "eraser" : "pencil", tip: tip(), color, pixelPerfect: !erasing && pixelPerfect });
    } catch (error) {
      // A locked layer, say: the core is asked anyway, so it says why.
      drawing = null;
      edit(dabs ? { op: "brush", layer: layer.id, erase: erasing, brush, color, points: first } : { op: "stroke", layer: layer.id, tool: erasing ? "eraser" : "pencil", tip: tip(), color, pixelPerfect, points: first }, erasing ? "erase" : "draw");
      return;
    }
    drawing.queued = first;
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
    hoverAt = p;
    if (moving?.drag && e.pointerId === moving.drag.pointer && p && view) {
      const d = moving.drag;
      let t = dragTransform(d.start, moving.rect, d.handle, d.from, p, { free: e.shiftKey, snap: e.shiftKey });
      // The move tool, and a pixel canvas, move by whole pixels.
      if (d.handle === "move" && (!moving.free || view.preset === "pixel")) t = { ...t, dx: Math.round(t.dx), dy: Math.round(t.dy) };
      moving.t = t;
      paint();
      return;
    }
    if (selecting && e.pointerId === selecting.pointer && p) {
      selecting.to = p;
      if (selectShape === "lasso") selecting.points.push(p);
      paint();
      return;
    }
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
    // A stabilised line drawn on to where the pen was lifted.
    const rest = drawing.steady && drawing.last ? drawing.steady.end(...drawing.last) : [];
    if (rest.length) drawing.queued.push(q8(rest[0]), q8(rest[1]), rest[2]);
    if (drawing.queued.length) copy.strokeAdd(Int32Array.from(drawing.queued));
    drawing = null;
    const { op, hash } = copy.strokeEnd();
    const erased = (op.op === "stroke" && op.tool === "eraser") || (op.op === "brush" && op.erase);
    send_(op, erased ? "erase" : op.op === "brush" ? "paint" : "draw", hash, copy.touched());
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
    selecting = null;
    leaveMade();
    paint();
  }

  const end = (/** @type {PointerEvent} */ e) => {
    if (e.pointerType === "pen") pen = false;
    panning = null;
    if (moving?.drag && e.pointerId === moving.drag.pointer) {
      moving.drag = null;
      // The move tool's move is kept when let go; ⌘T's waits for ↩.
      if (!moving.free) finishTransform();
    }
    if (selecting && e.pointerId === selecting.pointer) finishSelecting();
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
      if (k === "z" && view && !typing && moving) return (endTransform(), true);
      if (k === "z" && view && !typing && (drawing || shaping || selecting)) return (cancel(), true);
      if (k === "z" && view && !typing) return (send({ type: e.shiftKey ? "rasterRedo" : "rasterUndo" }), true);
      if (!view || typing) return false;
      const layer = current();
      if (k === "a" && !e.shiftKey) return (select({ kind: "all" }), true);
      if (k === "d" && !e.shiftKey) return (select({ kind: "nothing" }), true);
      if (k === "i" && e.shiftKey) return (select({ kind: "invert" }), true);
      if ((k === "c" || k === "x") && !e.shiftKey && layer) return (edit({ op: "copy", layer: layer.id, cut: k === "x" }, k === "x" ? "cut" : "copy"), true);
      if (k === "v" && !e.shiftKey) return (edit({ op: "paste", above: layer?.id }, "paste"), true);
      if (k === "t" && !e.shiftKey) return (startTransform(true), true);
      return false;
    }
    if (typing || e.altKey || e.ctrlKey || e.metaKey || !view) return false;
    if (e.key === "Escape" && moving) return (endTransform(), true);
    if (e.key === "Enter" && moving) return (finishTransform(), true);
    if (e.key === "Escape" && (drawing || shaping || selecting)) return (cancel(), true);
    if ((e.key === "Backspace" || e.key === "Delete") && current()) return (edit({ op: "clear", layer: /** @type {Layer} */ (current()).id }, "clear"), true);
    if (e.key === "?") return (host.keys?.("— Canvas's keys", KEYS), true);
    if (e.key === "f") return (refit(), true);
    if (e.key === "1") return (actualSize(), true);
    const keys = toolKeys(view.preset);
    if (keys[e.key]) return (useTool(keys[e.key]), true);
    if (e.key === "[" || e.key === "]") {
      if (soft()) {
        // A brush by a quarter at a time, so a tap goes as far at 4 px as at 400.
        const which = brushFor();
        const size = brushes[which].size;
        const next = e.key === "]" ? Math.max(size + 1, Math.round(size * 1.25)) : Math.min(size - 1, Math.round(size / 1.25));
        brushes = { ...brushes, [which]: brushOf({ ...brushes[which], size: next }, brushes[which]) };
        keepBrushes();
      } else {
        tipSize = Math.min(64, Math.max(1, tipSize + (e.key === "]" ? 1 : -1)));
      }
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
    if (q("#raster-pane").dataset.showing === "make") renderMake();
    paint();
  }

  render();

  // For the window's tests: this copy's pixels, read from the section.
  /** @type {any} */ (ws).rasterProbe = {
    /** @param {number} x @param {number} y */
    pick: (x, y) => copy?.pick(x, y) ?? null,
    settled: () => queue.then(() => sync.settled()),
    sync: () => sync.state(),
    /** How many edges the marching ants go along. */
    ants: () => antsNow().length / 4,
    /** Whether a transform is under way. */
    moving: () => Boolean(moving),
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
        if (!view || opened) endTransform();
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
          // A pencil for a pixel canvas, a brush for a paint one.
          tool = view.preset === "paint" ? "brush" : "pencil";
          ws.dataset.tool = tool;
          for (const b of ws.querySelectorAll("[data-raster-tool]")) b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.rasterTool === tool));
          await reload();
        } else if (copy && (copy.width !== view.width || copy.height !== view.height)) {
          // Turned, resized or cropped: the copy is loaded again at its size.
          endTransform();
          cancel();
          sync.reset();
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
    settled: () => queue.then(() => sync.settled()),
    /** A stage point as a canvas point. @param {number} sx @param {number} sy */
    toCanvas: (sx, sy) => (cam ? toCanvas(cam, sx, sy) : null),
  };
}
