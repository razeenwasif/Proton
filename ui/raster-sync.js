/* Canvas mode's edits between the window and the core, as a state machine.
 *
 * The window makes an edit on its own copy first, then sends it numbered
 * with the hash of the tiles it touched. The core answers each, in order,
 * with its own hash. When they agree, nothing more is done. When they do not
 * — or the core refused — the tiles the edit touched are stale: the window's
 * copy of them is not the core's, and they are fetched again. Tiles the core
 * changed itself (undo, redo, the agent) are stale the same way.
 *
 * Two rules keep a fetch from undoing the window's own work:
 *
 * - a stale tile is not fetched while an edit on its way touches it — the
 *   window's copy has that edit and the core's may not yet; it is fetched
 *   once the edit is answered;
 * - a tile that arrives older than the last edit sent to it — the core
 *   numbers what it read it after — is not put, and is fetched again.
 *
 * An edit sent without a prediction — refused by this copy, or one it does
 * not make itself, like turning the canvas — touched nothing this copy
 * knows of: when the core makes it, the copy is loaded again.
 *
 * No DOM and no WebAssembly here: `fetch` and `put` are given, so the node
 * tests drive it with stand-ins. */

/** @typedef {[number, number]} TileKey [layer, index] */
/** @typedef {{seq: number, hash: string | null, touched: TileKey[], meta: boolean}} Sent */

/**
 * @param {{
 *   fetch: (layer: number, tiles: number[]) => Promise<{lastSeq: number, bytes: ArrayBuffer}>,
 *   put: (layer: number, bytes: ArrayBuffer, keep: number[]) => void,
 *   reload: () => Promise<void>,
 *   painted?: () => void,
 * }} io
 */
export function createSync(io) {
  /** Edits sent and not yet answered, oldest first. @type {Sent[]} */
  let inflight = [];
  /** Tiles whose copy here is not the core's, by "layer:index". @type {Map<string, TileKey>} */
  const stale = new Map();
  /** The last edit sent to touch each tile. @type {Map<string, number>} */
  const lastTouch = new Map();
  let seq = 0;
  /** @type {Promise<void> | null} */
  let pumping = null;
  /** Bumped on a new canvas: anything about the last one is let go. */
  let era = 0;

  const key = (/** @type {TileKey} */ t) => `${t[0]}:${t[1]}`;
  const busy = (/** @type {string} */ k) => inflight.some((s) => s.touched.some((t) => key(t) === k));

  /** The stale tiles that can be fetched now, by layer. */
  function ready() {
    /** @type {Map<number, number[]>} */
    const out = new Map();
    for (const [k, [layer, index]] of stale) {
      if (busy(k)) continue;
      out.set(layer, [...(out.get(layer) ?? []), index]);
    }
    return out;
  }

  /** Fetch what can be fetched, until nothing more can. */
  function pump() {
    pumping ??= (async () => {
      const mine = era;
      for (let rounds = 0; rounds < 8; rounds++) {
        const asking = ready();
        if (!asking.size) break;
        for (const [layer, tiles] of asking) {
          const {lastSeq, bytes} = await io.fetch(layer, tiles);
          if (mine !== era) return;
          // What arrived old, or under an edit sent since, is fetched again.
          const keep = tiles.filter((index) => {
            const k = key([layer, index]);
            return stale.has(k) && !busy(k) && (lastTouch.get(k) ?? 0) <= lastSeq;
          });
          if (keep.length) io.put(layer, bytes, keep);
          for (const index of keep) stale.delete(key([layer, index]));
        }
        io.painted?.();
      }
    })().finally(() => {
      pumping = null;
    });
    return pumping;
  }

  /** A new canvas: nothing about the last is waited on. */
  function reset() {
    era += 1;
    inflight = [];
    stale.clear();
    lastTouch.clear();
  }

  return {
    /** The number the next edit is sent under. */
    next() {
      seq += 1;
      return seq;
    },
    /** An edit sent: its number, the hash made here, the tiles it touched,
     *  and whether it changed the layers themselves.
     *  @param {number} n @param {string | null} hash @param {TileKey[]} touched @param {boolean} meta */
    sent(n, hash, touched, meta = false) {
      for (const t of touched) lastTouch.set(key(t), n);
      inflight.push({ seq: n, hash, touched, meta });
    },
    /** The core's answer. @param {{seq: number, hash: string, refused?: string | null}} answer */
    async answered(answer) {
      const at = inflight.findIndex((s) => s.seq === answer.seq);
      if (at < 0) return;
      // Answered in order: what was sent before it was answered too.
      const mine = inflight.splice(0, at + 1)[at];
      if (mine.hash === null) {
        // Not made here: made there, the copy is behind by all of it.
        if (!answer.refused) {
          reset();
          await io.reload();
        }
        return;
      }
      if (!answer.refused && answer.hash === mine.hash) {
        await pump();
        return;
      }
      if (!answer.refused) console.warn(`proton: canvas edit ${answer.seq} made different pixels here; its tiles are fetched again`);
      if (mine.meta) {
        // The layers themselves differ: everything again.
        reset();
        await io.reload();
        return;
      }
      for (const t of mine.touched) stale.set(key(t), t);
      await pump();
    },
    /** Tiles the core changed itself. @param {[number, number[]][]} tiles */
    async changed(tiles) {
      for (const [layer, list] of tiles) for (const index of list) stale.set(key([layer, index]), [layer, index]);
      await pump();
    },
    reset,
    /** For tests and the window's probe: what is waited on and what is stale. */
    state: () => ({ inflight: inflight.map((s) => s.seq), stale: [...stale.keys()].sort() }),
    /** Whether everything sent is answered and nothing is stale. */
    settled: () => (pumping ?? Promise.resolve()).then(() => inflight.length === 0 && stale.size === 0),
  };
}
