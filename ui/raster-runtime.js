/* Canvas mode's engine in the window: proton-raster built as WebAssembly
 * (`raster-runtime.wasm`, from crates/proton-raster-runtime), behind a plain
 * C ABI. This wraps it.
 *
 * One rule matters more than the rest: a call that can allocate can grow the
 * WebAssembly's memory, and a view taken on the memory before that is of
 * memory that is gone. So every view here is taken after the call, from
 * `memory.buffer` as it is then, and read before the next call. */

/** @typedef {import('./bindings/RasterLayer').RasterLayer} Layer */
/** @typedef {import('./bindings/RasterOp').RasterOp} Op */

const ABI = 1;

/** Pixels laid through layers in a frame, to begin with: about two
 *  milliseconds of the engine's work. */
export const FRAME_BUDGET = 400_000;
/** The most a frame is given: about five milliseconds — inside a 120 Hz
 *  frame, with room for the rest of it. */
const FRAME_MOST = 900_000;

/** The next frame's budget, from this one's and whether work was left over:
 *  more while the ink falls behind the pen, back down as it catches up — so a stroke under many layers keeps up at the cost of a longer
 *  frame, and a light one keeps its frames short.
 *  @param {number} budget @param {boolean} behind */
export function nextBudget(budget, behind) {
  return behind ? Math.min(FRAME_MOST, Math.round(budget * 1.3)) : Math.max(FRAME_BUDGET, Math.round(budget / 1.15));
}

/** The module, compiled once a window. @type {Promise<WebAssembly.Module> | null} */
let compiled = null;

export function compileRaster() {
  // Beside this module, wherever the page that loaded it is.
  compiled ??= fetch(new URL("raster-runtime.wasm", import.meta.url))
    .then((r) => {
      if (!r.ok) throw new Error(`raster-runtime.wasm: ${r.status}`);
      return r.arrayBuffer();
    })
    .then((bytes) => WebAssembly.compile(bytes));
  return compiled;
}

/** An engine of its own: its own canvas, its own memory. */
export async function createRasterRuntime() {
  const instance = await WebAssembly.instantiate(await compileRaster(), {});
  /** @type {any} */
  const rr = instance.exports;
  if (rr.rr_abi() !== ABI) throw new Error(`raster-runtime.wasm speaks ABI ${rr.rr_abi()}, not ${ABI}`);
  let width = 0;
  let height = 0;

  /** Why the last call failed. */
  function why() {
    const at = rr.rr_error();
    const length = new DataView(rr.memory.buffer).getUint32(at, true);
    return new TextDecoder().decode(new Uint8Array(rr.memory.buffer, at + 4, length));
  }

  /** Bytes in, through memory the engine gave. @param {(...args: number[]) => number} fn
   *  @param {Uint8Array} bytes @param {number[]} first */
  function pass(fn, bytes, ...first) {
    const at = rr.rr_alloc(bytes.length);
    new Uint8Array(rr.memory.buffer, at, bytes.length).set(bytes);
    const result = fn(...first, at, bytes.length);
    rr.rr_free(at, bytes.length);
    return result;
  }
  const text = (/** @type {unknown} */ value) => new TextEncoder().encode(JSON.stringify(value));

  return {
    /** A canvas to mirror, with no pixels yet.
     *  @param {{width: number, height: number, preset: string, palette: number[][], layers: Layer[], generation: number}} canvas */
    open(canvas) {
      if (pass(rr.rr_open, text(canvas)) !== 0) throw new Error(why());
      width = canvas.width;
      height = canvas.height;
    },
    /** The layers or palette changed. @param {Layer[]} layers @param {number[][]} [palette] */
    meta(layers, palette) {
      if (pass(rr.rr_meta, text({ layers, palette })) !== 0) throw new Error(why());
    },
    /** Tiles of a layer, as `read_raster` sends them. @param {number} layer @param {ArrayBuffer} bytes */
    put(layer, bytes) {
      if (pass(rr.rr_put, new Uint8Array(bytes), layer) !== 0) throw new Error(why());
    },
    /** An op, made here: the hash of what it touched, or why it was refused.
     *  @param {Op} op @returns {{hash: string} | {refused: string}} */
    apply(op) {
      if (pass(rr.rr_apply, text(op)) !== 0) return { refused: why() };
      return { hash: ((BigInt(rr.rr_hash_hi() >>> 0) << 32n) | BigInt(rr.rr_hash_lo() >>> 0)).toString(16).padStart(16, "0") };
    },
    /** The tiles the last op or stroke touched, as [layer, index] pairs. @returns {[number, number][]} */
    touched() {
      const at = rr.rr_touched();
      const count = new Uint32Array(rr.memory.buffer, at, 1)[0];
      const words = new Uint32Array(rr.memory.buffer, at + 4, count * 2);
      return Array.from({ length: count }, (_, i) => /** @type {[number, number]} */ ([words[2 * i], words[2 * i + 1]]));
    },
    /** A pencil or eraser begun with a tip, or a brush (an eraser with a
     *  brush erases softly).
     *  @param {{layer: number, tool: string, color: number[], tip?: {size: number, round: boolean, mirror: (number | null)[]}, pixelPerfect?: boolean,
     *           brush?: import('./bindings/RasterBrush').RasterBrush}} stroke */
    strokeBegin(stroke) {
      if (pass(rr.rr_stroke_begin, text(stroke)) !== 0) throw new Error(why());
    },
    /** More of it: x, y (1/256 px), pressure, three numbers a point. @param {Int32Array} points */
    strokeAdd(points) {
      if (!points.length) return;
      const bytes = points.length * 4;
      const at = rr.rr_alloc(bytes);
      new Int32Array(rr.memory.buffer, at, points.length).set(points);
      const result = rr.rr_stroke_add(at, points.length);
      rr.rr_free(at, bytes);
      if (result !== 0) throw new Error(why());
    },
    /** The stroke finished, as the op the core is to make, and its hash.
     *  @returns {{op: Op, hash: string}} */
    strokeEnd() {
      const at = rr.rr_stroke_end();
      const length = new DataView(rr.memory.buffer).getUint32(at, true);
      if (!length) throw new Error(why());
      const op = JSON.parse(new TextDecoder().decode(new Uint8Array(rr.memory.buffer, at + 4, length)));
      return { op, hash: ((BigInt(rr.rr_hash_hi() >>> 0) << 32n) | BigInt(rr.rr_hash_lo() >>> 0)).toString(16).padStart(16, "0") };
    },
    /** The stroke taken away; nothing is sent. */
    strokeCancel() {
      rr.rr_stroke_cancel();
    },
    /** An op shown and not kept: a shape being dragged. @param {Op} op */
    preview(op) {
      if (pass(rr.rr_preview, text(op)) !== 0) throw new Error(why());
    },
    /** What was previewed taken away. */
    clearPreview() {
      rr.rr_preview_clear();
    },
    /** Tiles laid together again since last asked: their rectangles, as x,
     *  y, w, h. Given a budget — pixels laid through layers — no more than
     *  that, the rest left for the next call (`pending`).
     *  @param {number} [budget] */
    dirty(budget) {
      const at = budget === undefined ? rr.rr_dirty() : rr.rr_dirty_some(Math.min(budget, 0xffffffff) >>> 0);
      const count = new Uint32Array(rr.memory.buffer, at, 1)[0];
      return Array.from(new Uint32Array(rr.memory.buffer, at + 4, count * 4));
    },
    /** How many parts of tiles wait to be laid together. */
    pending() {
      return rr.rr_pending() >>> 0;
    },
    /** The layers laid together: premultiplied RGBA, row by row. A view —
     *  read it before the next call. */
    composite() {
      const at = rr.rr_composite();
      return new Uint8Array(rr.memory.buffer, at, width * height * 4);
    },
    /** A pixel of everything laid together, straight. @param {number} x @param {number} y */
    pick(x, y) {
      const v = rr.rr_pick(x, y) >>> 0;
      return [v & 255, (v >>> 8) & 255, (v >>> 16) & 255, v >>> 24];
    },
    /** A pixel of one layer. @param {number} layer @param {number} x @param {number} y */
    layerPixel(layer, x, y) {
      const v = rr.rr_layer_pixel(layer, x, y) >>> 0;
      return [v & 255, (v >>> 8) & 255, (v >>> 16) & 255, v >>> 24];
    },
    /** The layers, the palette, and which tiles each layer has.
     *  @returns {{layers: Layer[], palette: number[][], tiles: [number, number[]][]}} */
    info() {
      const at = rr.rr_info();
      const length = new DataView(rr.memory.buffer).getUint32(at, true);
      return JSON.parse(new TextDecoder().decode(new Uint8Array(rr.memory.buffer, at + 4, length)));
    },
    /** A tile's RGBA, copied, or null when clear. @param {number} layer @param {number} index */
    tile(layer, index) {
      const at = rr.rr_tile(layer, index);
      return at ? new Uint8Array(rr.memory.buffer, at, 64 * 64 * 4).slice() : null;
    },
    get width() {
      return width;
    },
    get height() {
      return height;
    },
  };
}

/** Tiles as `read_raster` lays them out, from a copy that has them: the
 *  stand-in core of the preview and the tests.
 *  @param {Awaited<ReturnType<typeof createRasterRuntime>>} from @param {number} generation
 *  @param {number} layer @param {number[]} tiles @param {number} [version] @param {number} [lastSeq] */
export function tilesPayload(from, generation, layer, tiles, version = 0, lastSeq = 0) {
  /** @type {(Uint32Array | Uint8Array)[]} */
  const parts = [new Uint32Array([1, generation, version >>> 0, Math.floor(version / 2 ** 32), lastSeq, tiles.length])];
  for (const index of tiles) {
    const px = from.tile(layer, index);
    parts.push(new Uint32Array([index, px ? 1 : 0]));
    if (px) parts.push(px);
  }
  const length = parts.reduce((n, p) => n + p.byteLength, 0);
  const out = new Uint8Array(length);
  let at = 0;
  for (const p of parts) {
    out.set(new Uint8Array(p.buffer, p.byteOffset, p.byteLength), at);
    at += p.byteLength;
  }
  return out.buffer;
}

/** A `read_raster` payload with only the tiles named kept, and the last edit
 *  the core had made when it read them.
 *  @param {ArrayBuffer} bytes @param {number[]} keep @returns {{lastSeq: number, bytes: ArrayBuffer}} */
export function keepTiles(bytes, keep) {
  const view = new DataView(bytes);
  const lastSeq = view.getUint32(16, true);
  const count = view.getUint32(20, true);
  /** @type {Uint8Array[]} */
  const parts = [];
  let at = 24;
  let kept = 0;
  for (let i = 0; i < count; i++) {
    const present = view.getUint32(at + 4, true) === 1;
    const length = 8 + (present ? 64 * 64 * 4 : 0);
    if (keep.includes(view.getUint32(at, true))) {
      parts.push(new Uint8Array(bytes, at, length));
      kept += 1;
    }
    at += length;
  }
  const out = new Uint8Array(24 + parts.reduce((n, p) => n + p.length, 0));
  out.set(new Uint8Array(bytes, 0, 24));
  new DataView(out.buffer).setUint32(20, kept, true);
  let to = 24;
  for (const p of parts) {
    out.set(p, to);
    to += p.length;
  }
  return { lastSeq, bytes: out.buffer };
}
