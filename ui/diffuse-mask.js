/* Diffuse's mask painter: a sheet over the graph where the part of a
 * picture to redraw is painted — the mockups' "Inpainting" screen.
 *
 * The mask is kept at the picture's own size, white where painted, on a
 * canvas of its own; what is on screen is the picture, fitted, with the mask
 * over it tinted. A brush stroke is dabs along the pointer's path, close
 * enough to read as a line, each a disc softened at its edge by "soft edge";
 * the eraser takes paint away the same way. Done hands back the mask as a
 * PNG — black where nothing is painted — and how much of the picture it
 * covers; Cancel, nothing.
 *
 * The core keeps it beside the flow and sends it to forge in the picture's
 * alpha, transparent where masked, as ComfyUI's Load Image reads one. */

import { escapeHtml as esc } from "./html.js";

/** The most strokes ⌘Z takes back. */
const UNDO = 30;

/**
 * Open the sheet over `host`.
 * @param {{host: HTMLElement, title: string, image: string, mask: string | null,
 *          done: (png: Uint8Array, share: number) => void, cancel: () => void}} o
 */
export async function openMask(o) {
  const picture = await loadImage(o.image);
  const w = picture.naturalWidth;
  const h = picture.naturalHeight;
  const mask = document.createElement("canvas");
  mask.width = w;
  mask.height = h;
  const m = /** @type {CanvasRenderingContext2D} */ (mask.getContext("2d", { willReadFrequently: true }));
  if (o.mask) {
    // Kept as a grey PNG: its brightness becomes how painted each pixel is.
    const kept = await loadImage(o.mask).catch(() => null);
    if (kept && kept.naturalWidth === w && kept.naturalHeight === h) {
      m.drawImage(kept, 0, 0);
      const data = m.getImageData(0, 0, w, h);
      for (let i = 0; i < data.data.length; i += 4) {
        const a = data.data[i];
        data.data[i] = data.data[i + 1] = data.data[i + 2] = 255;
        data.data[i + 3] = a;
      }
      m.putImageData(data, 0, 0);
    }
  }

  const sheet = document.createElement("div");
  sheet.className = "df-mask";
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-label", `Paint the mask for ${o.title}`);
  sheet.innerHTML =
    `<div class="df-mask-head"><b>Mask</b><span class="df-faint">${esc(o.title)} · ${w} × ${h}</span><span class="grow"></span>` +
    `<button class="cc-quiet" data-mask-cancel>Cancel</button><button class="cc-primary" data-mask-done>Done</button></div>` +
    `<div class="df-mask-body">` +
    `<div class="df-mask-tools" role="toolbar" aria-label="Mask tools">` +
    `<button aria-pressed="true" data-mask-tool="brush" title="Brush  b" aria-label="Brush">✎</button>` +
    `<button aria-pressed="false" data-mask-tool="eraser" title="Eraser  e — x swaps" aria-label="Eraser">⌫</button>` +
    `<span class="df-mask-sep"></span>` +
    `<button data-mask-invert title="Invert the mask  i" aria-label="Invert the mask">◐</button>` +
    `<button data-mask-clear title="Clear the mask" aria-label="Clear the mask">∅</button>` +
    `<button data-mask-undo title="Undo a stroke  ⌘Z" aria-label="Undo a stroke">↶</button></div>` +
    `<div class="df-mask-stage"><canvas data-mask-view></canvas></div>` +
    `<div class="df-mask-side">` +
    `<label class="df-field">Brush size <span data-mask-size-said></span><input type="range" min="2" max="400" data-mask-size></label>` +
    `<span class="df-faint">[ and ] change it</span>` +
    `<label class="df-field">Soft edge <span data-mask-soft-said></span><input type="range" min="0" max="100" value="30" data-mask-soft></label>` +
    `<p class="df-mask-share" data-mask-share></p>` +
    `<p class="df-faint">Only what is painted is redrawn; the rest is kept as it was.</p></div></div>`;
  o.host.append(sheet);
  const q = (/** @type {string} */ s) => /** @type {HTMLElement} */ (sheet.querySelector(s));
  const view = /** @type {HTMLCanvasElement} */ (q("[data-mask-view]"));
  const v = /** @type {CanvasRenderingContext2D} */ (view.getContext("2d"));
  const size = /** @type {HTMLInputElement} */ (q("[data-mask-size]"));
  const soft = /** @type {HTMLInputElement} */ (q("[data-mask-soft]"));
  size.value = String(Math.max(4, Math.round(Math.min(w, h) / 16)));
  let tool = "brush";
  /** @type {ImageData[]} */
  const undo = [];
  /** The view's scale: screen pixels a picture pixel. */
  let scale = 1;

  const tint = document.createElement("canvas");
  tint.width = w;
  tint.height = h;
  const t = /** @type {CanvasRenderingContext2D} */ (tint.getContext("2d"));

  function fit() {
    const box = q(".df-mask-stage").getBoundingClientRect();
    scale = Math.min(1, Math.max(0.05, Math.min((box.width - 24) / w, (box.height - 24) / h)));
    view.width = Math.max(1, Math.round(w * scale));
    view.height = Math.max(1, Math.round(h * scale));
    draw();
  }

  function draw() {
    v.clearRect(0, 0, view.width, view.height);
    v.drawImage(picture, 0, 0, view.width, view.height);
    t.globalCompositeOperation = "source-over";
    t.clearRect(0, 0, w, h);
    t.drawImage(mask, 0, 0);
    t.globalCompositeOperation = "source-in";
    t.fillStyle = "#f694ff";
    t.fillRect(0, 0, w, h);
    v.globalAlpha = 0.5;
    v.drawImage(tint, 0, 0, view.width, view.height);
    v.globalAlpha = 1;
  }

  /** How much of the picture is painted, as a share: every 4th pixel looked at. */
  function share() {
    const data = m.getImageData(0, 0, w, h).data;
    let sum = 0;
    let n = 0;
    for (let i = 3; i < data.length; i += 16) {
      sum += data[i];
      n++;
    }
    return n ? sum / (n * 255) : 0;
  }
  const sayShare = () => {
    q("[data-mask-share]").textContent = `${Math.round(share() * 100)}% of the picture is masked.`;
  };
  const saySizes = () => {
    q("[data-mask-size-said]").textContent = `${size.value} px`;
    q("[data-mask-soft-said]").textContent = `${soft.value}%`;
  };

  /** A dab at a picture point. @param {number} x @param {number} y */
  function dab(x, y) {
    const r = Number(size.value) / 2;
    const hard = 1 - Number(soft.value) / 100;
    const g = m.createRadialGradient(x, y, r * hard * 0.999, x, y, r);
    g.addColorStop(0, "rgba(255,255,255,1)");
    g.addColorStop(1, "rgba(255,255,255,0)");
    m.globalCompositeOperation = tool === "eraser" ? "destination-out" : "source-over";
    m.fillStyle = g;
    m.beginPath();
    m.arc(x, y, r, 0, Math.PI * 2);
    m.fill();
    m.globalCompositeOperation = "source-over";
  }

  /** @type {{id: number, x: number, y: number} | null} */
  let stroke = null;
  const at = (/** @type {PointerEvent} */ e) => {
    const r = view.getBoundingClientRect();
    return [(e.clientX - r.left) / scale, (e.clientY - r.top) / scale];
  };
  view.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    undo.push(m.getImageData(0, 0, w, h));
    if (undo.length > UNDO) undo.shift();
    const [x, y] = at(e);
    stroke = { id: e.pointerId, x, y };
    try {
      view.setPointerCapture(e.pointerId);
    } catch {
      // A pointer already gone: the stroke is one dab.
    }
    dab(x, y);
    draw();
  });
  view.addEventListener("pointermove", (e) => {
    if (!stroke || stroke.id !== e.pointerId) return;
    const [x, y] = at(e);
    // Dabs an eighth of the brush apart, so a fast stroke is a smooth line.
    const step = Math.max(1, Number(size.value) / 8);
    const d = Math.hypot(x - stroke.x, y - stroke.y);
    for (let s = step; s <= d; s += step) dab(stroke.x + ((x - stroke.x) * s) / d, stroke.y + ((y - stroke.y) * s) / d);
    if (d >= step) stroke = { id: stroke.id, x, y };
    draw();
  });
  const end = () => {
    if (!stroke) return;
    stroke = null;
    sayShare();
  };
  view.addEventListener("pointerup", end);
  view.addEventListener("pointercancel", end);

  const setTool = (/** @type {string} */ to) => {
    tool = to;
    for (const b of sheet.querySelectorAll("[data-mask-tool]")) b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.maskTool === tool));
  };
  function invert() {
    undo.push(m.getImageData(0, 0, w, h));
    const data = m.getImageData(0, 0, w, h);
    for (let i = 0; i < data.data.length; i += 4) {
      data.data[i] = data.data[i + 1] = data.data[i + 2] = 255;
      data.data[i + 3] = 255 - data.data[i + 3];
    }
    m.putImageData(data, 0, 0);
    draw();
    sayShare();
  }
  function back() {
    const was = undo.pop();
    if (!was) return;
    m.putImageData(was, 0, 0);
    draw();
    sayShare();
  }

  /** The mask as a PNG: white where painted on black. @returns {Promise<Uint8Array>} */
  async function png() {
    const out = document.createElement("canvas");
    out.width = w;
    out.height = h;
    const c = /** @type {CanvasRenderingContext2D} */ (out.getContext("2d"));
    c.fillStyle = "#000";
    c.fillRect(0, 0, w, h);
    c.drawImage(mask, 0, 0);
    const blob = await new Promise((done) => out.toBlob(done, "image/png"));
    return new Uint8Array(await /** @type {Blob} */ (blob).arrayBuffer());
  }

  const close = () => {
    sheet.remove();
    window.removeEventListener("resize", fit);
  };
  sheet.addEventListener("click", async (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const tb = /** @type {HTMLElement | null} */ (target.closest("[data-mask-tool]"));
    if (tb) setTool(tb.dataset.maskTool ?? "brush");
    if (target.closest("[data-mask-invert]")) invert();
    if (target.closest("[data-mask-undo]")) back();
    if (target.closest("[data-mask-clear]")) {
      undo.push(m.getImageData(0, 0, w, h));
      m.clearRect(0, 0, w, h);
      draw();
      sayShare();
    }
    if (target.closest("[data-mask-cancel]")) {
      close();
      o.cancel();
    }
    if (target.closest("[data-mask-done]")) {
      const bytes = await png();
      const covered = share();
      close();
      o.done(bytes, covered);
    }
  });
  sheet.addEventListener("input", saySizes);
  sheet.addEventListener("keydown", (e) => {
    const mod = e.metaKey || e.ctrlKey;
    const k = e.key.toLowerCase();
    let taken = true;
    if (mod && k === "z") back();
    else if (!mod && k === "[") size.value = String(Math.max(2, Math.round(Number(size.value) / 1.25)));
    else if (!mod && k === "]") size.value = String(Math.min(400, Math.round(Number(size.value) * 1.25) + 1));
    else if (!mod && k === "b") setTool("brush");
    else if (!mod && k === "e") setTool("eraser");
    else if (!mod && k === "x") setTool(tool === "brush" ? "eraser" : "brush");
    else if (!mod && k === "i") invert();
    else if (k === "escape") /** @type {HTMLElement} */ (q("[data-mask-cancel]")).click();
    else if (k === "enter") /** @type {HTMLElement} */ (q("[data-mask-done]")).click();
    else taken = false;
    if (taken) {
      e.preventDefault();
      e.stopPropagation();
      saySizes();
    }
  });
  sheet.tabIndex = -1;
  window.addEventListener("resize", fit);
  saySizes();
  sayShare();
  requestAnimationFrame(fit);
  sheet.focus();
  return { close, share, sheet };
}

/** An image, loaded. @param {string} url @returns {Promise<HTMLImageElement>} */
function loadImage(url) {
  return new Promise((done, failed) => {
    const img = new Image();
    img.onload = () => done(img);
    img.onerror = () => failed(new Error("the picture could not be read"));
    img.src = url;
  });
}
