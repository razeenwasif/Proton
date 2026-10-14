/* Canvas mode's stand-in for the core, in the browser preview.
 *
 * The preview has no core, so this is one: a second copy of the engine (its
 * own WebAssembly instance, with its own memory) holds the canvas the window
 * edits, answers each edit with the hash of what it touched, and gives tiles
 * out as `read_raster` lays them out. Undo replays every edit but the last
 * onto a fresh copy — slow on a large canvas, and only ever the preview's.
 * Nothing is written to disk: a save here is only here. */

import { createRasterRuntime, tilesPayload } from "./raster-runtime.js";

/** @typedef {Awaited<ReturnType<typeof createRasterRuntime>>} Runtime */

/** The palette a new pixel canvas starts with — the core's `PALETTE16`. */
const PALETTE16 = ["1b1b23", "3d3d4f", "6e6e86", "a8a8bf", "f2f2f7", "5a1f2e", "b0303f", "e8743b", "f4c95d", "8fd16a", "3e9e5c", "1f5f5b", "2f6fb0", "6fb7e8", "6b3fa0", "e07ab8"]
  .map((h) => [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16), 255]);

/** @param {(event: any) => void} _deliver */
export function createRasterPreview(_deliver) {
  /** @type {{path: string, name: string, preset: string, size: [number, number]}[]} */
  const canvases = [];
  /** @type {{path: string, name: string, preset: string, width: number, height: number, core: Runtime,
   *          ops: {op: any, label: string}[], undone: {op: any, label: string}[], saved: number, generation: number, version: number, lastSeq: number} | null} */
  let open = null;
  let generations = 0;

  const first = (/** @type {string} */ preset) => ({ id: 1, name: preset === "pixel" ? "Sprite" : "Background", opacity: 255, blend: "normal", hidden: false, locked: false, alphaLock: false });

  /** A copy with the edits made. @param {string} preset @param {number} width @param {number} height @param {any[]} ops */
  async function made(preset, width, height, ops) {
    const core = await createRasterRuntime();
    core.open({ width, height, preset, palette: preset === "pixel" ? PALETTE16 : [], layers: [first(preset)], generation: generations });
    for (const op of ops) core.apply(op);
    return core;
  }

  const home = () => ({ type: "rasterHome", canvases: canvases.map((c) => ({ ...c })), images: [], games: [] });
  const canvasEvent = () => {
    if (!open) return { type: "rasterCanvas", canvas: null };
    const info = open.core.info();
    return {
      type: "rasterCanvas",
      canvas: {
        path: open.path, name: open.name, preset: open.preset, width: info.width, height: info.height,
        palette: info.palette, layers: info.layers, tiles: info.tiles, generation: open.generation, version: open.version,
        dirty: open.ops.length !== open.saved, canUndo: open.ops.length > 0, canRedo: open.undone.length > 0,
        undo: open.ops[open.ops.length - 1]?.label ?? null, redo: open.undone[open.undone.length - 1]?.label ?? null, saving: false,
      },
    };
  };

  /** Undo or redo: the copy made again, and every tile sent. @param {boolean} back */
  async function step(back) {
    if (!open) return [];
    const from = back ? open.ops : open.undone;
    const to = back ? open.undone : open.ops;
    const moved = from.pop();
    if (!moved) return [];
    to.push(moved);
    const before = open.core.info().tiles;
    open.core = await made(open.preset, open.width, open.height, open.ops.map((o) => o.op));
    open.version += 1;
    const after = open.core.info().tiles;
    /** @type {Map<number, Set<number>>} */
    const tiles = new Map();
    for (const [layer, list] of [...before, ...after]) {
      const set = tiles.get(layer) ?? new Set();
      list.forEach((i) => set.add(i));
      tiles.set(layer, set);
    }
    return [canvasEvent(), { type: "rasterChanged", generation: open.generation, version: open.version, tiles: [...tiles].map(([l, s]) => [l, [...s]]), meta: true }];
  }

  return {
    sync: () => [home(), canvasEvent()],
    /** @param {number} generation @param {number} layer @param {number[]} tiles */
    async tiles(generation, layer, tiles) {
      if (!open || open.generation !== generation) throw new Error(`layer ${layer} of canvas ${generation} is not open`);
      return tilesPayload(open.core, generation, layer, tiles, open.version, open.lastSeq);
    },
    /** @param {any} command */
    async command(command) {
      switch (command.type) {
        case "rasterNew": {
          const path = `${command.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "canvas"}.canvas`;
          if (canvases.some((c) => c.path === path)) return [{ type: "notice", text: `${path} is already there`, level: "warn" }];
          generations += 1;
          canvases.push({ path, name: path.replace(/\.canvas$/, ""), preset: command.preset, size: [command.width, command.height] });
          open = { path, name: path.replace(/\.canvas$/, ""), preset: command.preset, width: command.width, height: command.height, core: await made(command.preset, command.width, command.height, []), ops: [], undone: [], saved: 0, generation: generations, version: 0, lastSeq: 0 };
          return [home(), canvasEvent()];
        }
        case "rasterOpen": {
          const found = canvases.find((c) => c.path === command.path);
          if (!found) return [{ type: "notice", text: `${command.path} is not a canvas here`, level: "warn" }];
          generations += 1;
          open = { path: found.path, name: found.name, preset: found.preset, width: found.size[0], height: found.size[1], core: await made(found.preset, found.size[0], found.size[1], []), ops: [], undone: [], saved: 0, generation: generations, version: 0, lastSeq: 0 };
          return [home(), canvasEvent()];
        }
        case "rasterClose":
          if (open && !command.discard && open.ops.length !== open.saved) return [{ type: "notice", text: "the canvas has unsaved changes", level: "warn" }];
          open = null;
          return [home(), canvasEvent()];
        case "rasterEdit": {
          if (!open || command.generation !== open.generation) return [];
          open.lastSeq = command.seq;
          // What the core tells of with the canvas: its layers, palette and size.
          const shape = () => {
            const info = /** @type {NonNullable<typeof open>} */ (open).core.info();
            return JSON.stringify([info.layers, info.palette, info.width, info.height]);
          };
          const before = shape();
          const done = open.core.apply(command.op);
          const answer = (/** @type {any} */ extra) => ({
            type: "rasterApplied", generation: open?.generation, seq: command.seq, version: open?.version, dirty: open ? open.ops.length !== open.saved : false,
            canUndo: Boolean(open?.ops.length), canRedo: Boolean(open?.undone.length), hash: "", refused: null, ...extra,
          });
          if ("refused" in done) return [answer({ refused: done.refused }), { type: "notice", text: done.refused, level: "warn" }];
          open.ops.push({ op: command.op, label: command.label || "edit" });
          open.undone = [];
          open.version += 1;
          const meta = shape() !== before;
          return [answer({ hash: done.hash }), ...(meta ? [canvasEvent()] : [])];
        }
        case "rasterUndo":
          return step(true);
        case "rasterRedo":
          return step(false);
        case "rasterSave":
          if (!open) return [];
          open.saved = open.ops.length;
          return [canvasEvent(), { type: "notice", text: "saved — in the preview, only here", level: "info" }];
        default:
          return [];
      }
    },
  };
}
