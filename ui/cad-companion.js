// @ts-check
/** A second file shown under the one open in CAD (split view): an
 *  assembly's blueprint under the assembly, the model under its drawing.
 *  Read-only, from its saved file; its 3D view turns and zooms, its sheet
 *  pans and zooms. The divider between the two is dragged, and kept. */

import { escapeHtml as esc } from "./html.js";

/** @typedef {import("./bindings/CadCompanionView").CadCompanionView} CompanionView */
/** @typedef {import("./bindings/CadFound").CadFound} Found */

const SPLIT_KEY = "proton.cad.split";

/** Where the divider is, as the share of the height the top keeps: within
 *  reach of both, so neither pane vanishes. @param {number} share */
export const clampShare = (share) => Math.min(0.85, Math.max(0.15, Number.isFinite(share) ? share : 0.5));

/** The files one can show below: what CAD opens, but schematics and boards.
 *  @param {Found[]} parts */
export const showable = (parts) => parts.filter((p) => ["part", "assembly", "drawing", "import"].includes(p.kind));

/**
 * @param {{
 *   main: HTMLElement,
 *   section: HTMLElement,
 *   gutter: HTMLElement,
 *   dispatch: (command: any) => void,
 *   resized: () => void,
 * }} host
 */
export function createCompanion(host) {
  const { main, section, gutter } = host;
  const q = (/** @type {string} */ s) => /** @type {HTMLElement} */ (section.querySelector(s));
  const els = {
    pick: /** @type {HTMLSelectElement} */ (q("[data-cad-comp-pick]")),
    note: q("[data-cad-comp-note]"),
    canvas: /** @type {HTMLCanvasElement} */ (q("[data-cad-comp-canvas]")),
    sheet: q("[data-cad-comp-sheet]"),
    status: q("[data-cad-comp-status]"),
  };
  const state = {
    /** @type {CompanionView | null} */ comp: null,
    /** @type {Found[]} */ parts: [],
    /** What the 3D view last drew, and fitted. */ drawn: "",
    fitted: "",
    /** The sheet's zoom and corner, pixels; what it shows. */
    sheet: { zoom: 1, x: 0, y: 0, shown: "", fitted: "" },
    /** @type {{at: number[], from: number[]} | null} */ panning: null,
    share: 0.5,
  };
  try {
    state.share = clampShare(parseFloat(localStorage.getItem(SPLIT_KEY) ?? "0.5"));
  } catch {
    // No storage: the even split.
  }
  /** @type {Awaited<ReturnType<typeof import("./cad-view.js").mountView>> | null} */
  let view = null;
  /** @type {Promise<void> | null} */
  let viewing = null;
  let noView = "";

  function layout() {
    const open = !!state.comp;
    section.hidden = !open;
    gutter.hidden = !open;
    main.classList.toggle("cd-split", open);
    main.style.setProperty("--cd-split", `${(state.share * 100).toFixed(2)}%`);
  }

  async function ensureView() {
    if (view || noView) return;
    viewing ??= (async () => {
      try {
        const { mountView } = await import("./cad-view.js");
        view = await mountView(els.canvas, { hover: () => {}, pick: () => {}, pickEdge: () => {} });
        if (!view) noView = "This window has no WebGL: the model cannot be drawn in 3D here.";
      } catch (e) {
        noView = `The 3D view did not start: ${e}`;
      }
      render();
    })();
    await viewing;
  }

  function fitSheet() {
    const d = state.comp?.drawing;
    if (!d) return;
    const r = els.sheet.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return;
    const zoom = Math.min((r.width - 24) / d.width, (r.height - 24) / d.height);
    state.sheet.zoom = Math.max(zoom, 0.05);
    state.sheet.x = (r.width - d.width * state.sheet.zoom) / 2;
    state.sheet.y = (r.height - d.height * state.sheet.zoom) / 2;
    placeSheet();
  }

  function placeSheet() {
    const svg = /** @type {SVGSVGElement | null} */ (els.sheet.querySelector("svg"));
    const d = state.comp?.drawing;
    if (!svg || !d) return;
    svg.style.width = `${d.width}px`;
    svg.style.height = `${d.height}px`;
    svg.style.transform = `translate(${state.sheet.x}px, ${state.sheet.y}px) scale(${state.sheet.zoom})`;
  }

  function renderPick() {
    const c = state.comp;
    const files = showable(state.parts);
    if (c?.path && !files.some((f) => f.path === c.path)) files.unshift({ path: c.path, name: c.name, kind: c.kind });
    // Nothing chosen yet: a first line saying so.
    const none = c && !c.path ? `<option value="" selected disabled>Choose a file…</option>` : "";
    els.pick.innerHTML = none + files.map((f) => `<option value="${esc(f.path)}"${f.path === c?.path ? " selected" : ""}>${esc(f.path)}</option>`).join("");
  }

  function render() {
    layout();
    const c = state.comp;
    if (!c) return;
    renderPick();
    const d = c.drawing;
    els.note.textContent = !c.path ? "nothing chosen" : c.error ? "" : d ? "the drawing, as saved" : c.kind === "import" ? "another program's file" : `the ${c.kind}, as saved`;
    els.status.textContent = c.error ?? noView;
    els.sheet.hidden = !d;
    els.canvas.hidden = !!d || !!c.error || !!noView;
    if (d) {
      if (state.sheet.shown !== d.svg) {
        state.sheet.shown = d.svg;
        els.sheet.innerHTML = d.svg;
        els.sheet.querySelector("svg")?.classList.add("cd-sheet-svg");
      }
      const key = `${c.path}:${d.width}x${d.height}`;
      if (state.sheet.fitted !== key) {
        state.sheet.fitted = key;
        requestAnimationFrame(fitSheet);
      } else placeSheet();
      return;
    }
    const shape = c.assembly ?? c.part;
    if (!shape) return;
    if (!view) return void ensureView();
    const shown = JSON.stringify([c.path, shape.generation, shape.text]);
    if (shown !== state.drawn) {
      state.drawn = shown;
      if (c.assembly) view.show(/** @type {any} */ ({ bodies: c.assembly.bodies, sketches: [], bounds: c.assembly.bounds }), { offsets: {}, marked: [] });
      else view.show(/** @type {any} */ (c.part), { sketch: null, edges: [], faces: [] });
    }
    if (state.fitted !== shown) {
      state.fitted = shown;
      requestAnimationFrame(() => {
        view?.resize();
        view?.fit(shape.bounds);
      });
    }
  }

  /* ── its controls ─────────────────────────────────────────────── */

  section.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const c = state.comp;
    if (t.closest("[data-cad-comp-refresh]") && c) host.dispatch({ type: "cadCompanion", path: c.path });
    if (t.closest("[data-cad-comp-close]")) host.dispatch({ type: "cadCompanion", path: null });
    if (t.closest("[data-cad-comp-fit]") && c) {
      if (c.drawing) fitSheet();
      else if (view && (c.assembly ?? c.part)) view.fit(/** @type {any} */ (c.assembly ?? c.part).bounds);
    }
  });
  els.pick.addEventListener("change", () => host.dispatch({ type: "cadCompanion", path: els.pick.value }));

  // The sheet: the wheel zooms about the pointer, a drag pans, a double
  // click fits it.
  els.sheet.addEventListener("wheel", (e) => {
    e.preventDefault();
    const r = els.sheet.getBoundingClientRect();
    const px = e.clientX - r.left, py = e.clientY - r.top;
    const k = Math.exp(-e.deltaY * 0.0015);
    const zoom = Math.min(40, Math.max(0.05, state.sheet.zoom * k));
    const f = zoom / state.sheet.zoom;
    state.sheet.x = px - (px - state.sheet.x) * f;
    state.sheet.y = py - (py - state.sheet.y) * f;
    state.sheet.zoom = zoom;
    placeSheet();
  }, { passive: false });
  /** Keeps the pointer while it drags, where the window lets it.
   *  @param {HTMLElement} el @param {PointerEvent} e */
  const capture = (el, e) => {
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      // A pointer the window no longer has: the drag goes on without.
    }
  };
  els.sheet.addEventListener("pointerdown", (e) => {
    capture(els.sheet, e);
    state.panning = { at: [e.clientX, e.clientY], from: [state.sheet.x, state.sheet.y] };
  });
  els.sheet.addEventListener("pointermove", (e) => {
    const p = state.panning;
    if (!p) return;
    state.sheet.x = p.from[0] + e.clientX - p.at[0];
    state.sheet.y = p.from[1] + e.clientY - p.at[1];
    placeSheet();
  });
  const stopPan = () => (state.panning = null);
  els.sheet.addEventListener("pointerup", stopPan);
  els.sheet.addEventListener("pointercancel", stopPan);
  els.sheet.addEventListener("dblclick", fitSheet);

  // The divider: dragged, the top's share of the height; double-clicked,
  // even again.
  gutter.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    capture(gutter, e);
    const move = (/** @type {PointerEvent} */ m) => {
      const r = main.getBoundingClientRect();
      const head = /** @type {HTMLElement | null} */ (main.querySelector(".cd-main-head, header"));
      const top = r.top + (head?.offsetHeight ?? 0);
      state.share = clampShare((m.clientY - top) / Math.max(1, r.bottom - top));
      layout();
      host.resized();
      resize();
    };
    const up = () => {
      gutter.removeEventListener("pointermove", move);
      gutter.removeEventListener("pointerup", up);
      try {
        localStorage.setItem(SPLIT_KEY, String(state.share));
      } catch {
        // Kept for this window only.
      }
    };
    gutter.addEventListener("pointermove", move);
    gutter.addEventListener("pointerup", up);
  });
  gutter.addEventListener("dblclick", () => {
    state.share = 0.5;
    layout();
    host.resized();
    resize();
  });

  function resize() {
    if (!state.comp) return;
    view?.resize();
    if (state.comp.drawing) placeSheet();
  }

  return {
    /** @param {CompanionView | null} comp */
    show(comp) {
      const was = !!state.comp;
      state.comp = comp;
      if (!comp) {
        state.drawn = "";
        state.fitted = "";
        state.sheet.fitted = "";
      }
      render();
      if (was !== !!comp) requestAnimationFrame(() => {
        host.resized();
        resize();
      });
    },
    /** @param {Found[]} parts */
    parts(parts) {
      state.parts = parts;
      if (state.comp) renderPick();
    },
    resize,
    isOpen: () => !!state.comp,
    debug: () => ({ comp: state.comp, share: state.share, sheet: state.sheet, view: view?.debug() ?? null }),
  };
}
