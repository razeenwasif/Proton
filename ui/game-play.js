/* Game mode's Play: the game run in the stage, by the simulation the core
 * runs — proton-game, as WebAssembly (`game-runtime.wasm`).
 *
 * The core cooks the world from the game as it is now, unsaved edits and
 * all, and sends it with where to start (`gamePlay`). This loads it into the
 * simulation and steps it at the game's tick on a fixed clock: a frame of
 * the screen may run several ticks or none, and what is drawn between two
 * ticks is eased between them, so the run is the same run at any frame rate.
 * The input held each tick is kept as it is played; on Stop it is sent back
 * (`gameRecorded`), and the core runs it again to see that it ends in the
 * same place — the two builds checked against each other every time.
 *
 * The keys the game uses are the game's while it plays. ⌘S, ⌃ with a digit
 * and Esc still are Proton's: Esc stops. */

import { layout } from "./game-scene.js";

/** @typedef {import('./bindings/GameRoom').GameRoom} Room */

/** The runtime's ABI, and the words in a frame's header and in each record
 *  (proton-game-runtime). */
const ABI = 1;
const HEADER = 12;
const RECORD = 6;
const DOOR = 24;

/** The module, compiled once a window. @type {Promise<WebAssembly.Module> | null} */
let compiled = null;

function compile() {
  compiled ??= fetch("game-runtime.wasm")
    .then((r) => {
      if (!r.ok) throw new Error(`game-runtime.wasm: ${r.status}`);
      return r.arrayBuffer();
    })
    .then((bytes) => WebAssembly.compile(bytes));
  return compiled;
}

/**
 * @param {{
 *   canvas: HTMLCanvasElement,
 *   gpu: () => ReturnType<typeof import('./game-render.js').createGameRender>,
 *   sizes: Map<string, [number, number]>,
 *   parts: Map<string, import('./game-scene.js').Parts>,
 *   want: (keys: string[]) => void,
 *   stageSize: () => number[],
 *   focal: () => number,
 *   ground: () => [number, number, number],
 *   dispatch: (command: any) => unknown,
 *   changed: (state: {playing: boolean, paused: boolean, tick: number}) => void,
 *   ended: () => void,
 * }} host
 */
export function createPlayer(host) {
  /** @type {any} */
  let pg = null;
  /** @type {any} */
  let world = null;
  /** @type {Room[]} */
  let rooms = [];
  /** @type {Map<string, number>} */
  let keys = new Map();
  const held = new Set();
  /** @type {{room: number, at: [number, number], abilities: number}} */
  let start = { room: 0, at: [0, 0], abilities: 0 };
  /** The input held, tick by tick, as runs. @type {[number, number][]} */
  let recording = [];
  /** A recording being played back: its runs, and where in them. @type {{runs: [number, number][], at: number, left: number} | null} */
  let playback = null;
  let playing = false;
  let paused = false;
  let raf = 0;
  let last = 0;
  let left = 0;
  let dt = 1 / 60;
  /** The two last frames, to ease between. @type {Int32Array | null} */
  let now = null;
  /** @type {Int32Array | null} */
  let before = null;

  /** Why the runtime's last call failed. */
  function why() {
    const at = pg.pg_error();
    const length = new DataView(pg.memory.buffer).getUint32(at, true);
    return new TextDecoder().decode(new Uint8Array(pg.memory.buffer, at + 4, length));
  }

  function frameWords() {
    const at = pg.pg_frame();
    const count = new Int32Array(pg.memory.buffer, at, HEADER)[8];
    return new Int32Array(pg.memory.buffer.slice(at, at + (HEADER + count * RECORD) * 4));
  }

  /** The input this tick: the keys held, or the recording's. */
  function input() {
    if (playback) {
      while (playback.at < playback.runs.length && playback.left === 0) {
        playback.at++;
        playback.left = playback.runs[playback.at]?.[1] ?? 0;
      }
      if (playback.at >= playback.runs.length) return null;
      playback.left--;
      return playback.runs[playback.at][0];
    }
    let bits = 0;
    for (const b of held) bits |= b;
    return bits;
  }

  function tick() {
    const bits = input();
    if (bits === null) {
      pause(true);
      return;
    }
    if (!playback) {
      const run = recording[recording.length - 1];
      if (run && run[0] === bits) run[1]++;
      else recording.push([bits, 1]);
    }
    pg.pg_step(bits);
    before = now;
    now = frameWords();
  }

  /** @param {number} t */
  function loop(t) {
    if (!playing) return;
    raf = requestAnimationFrame(loop);
    if (!paused) {
      left += Math.min(0.25, (t - (last || t)) / 1000);
      while (left >= dt) {
        tick();
        left -= dt;
        if (paused) break;
      }
    }
    last = t;
    draw(paused ? 1 : left / dt);
    host.changed({ playing, paused, tick: now?.[1] ?? 0 });
  }

  /** Draw the frame, eased `alpha` of the way from the one before. @param {number} alpha */
  function draw(alpha) {
    if (!now || !world) return;
    const [width, height] = host.stageSize();
    if (!width || !height) return;
    const room = rooms[now[2]];
    if (!room) return;
    const ease = (/** @type {number} */ i) => (before && before[2] === now?.[2] ? before[i] + (now[i] - before[i]) * alpha : now?.[i] ?? 0);
    const center = /** @type {[number, number]} */ ([ease(3) / 256, ease(4) / 256]);
    const [vw, vh] = world.view;
    const zoom = Math.min(width / vw, height / vh);
    const view = { scale: zoom, x: width / 2 - center[0] * zoom, y: height / 2 - center[1] * zoom };
    const drawn = [];
    for (let i = 0; i < now[8]; i++) {
      const o = HEADER + i * RECORD;
      const p = before && before[2] === now[2] && before[8] > i ? HEADER + i * RECORD : -1;
      const at = p >= 0 && before ? [before[o + 1] + (now[o + 1] - before[o + 1]) * alpha, before[o + 2] + (now[o + 2] - before[o + 2]) * alpha] : [now[o + 1], now[o + 2]];
      drawn.push({ key: world.sprites[now[o]], at, pivot: [now[o + 3], now[o + 4]], flip: Boolean(now[o + 5]) });
    }
    host.want([...room.planes.flatMap((p) => p.items.map((i) => i.image)), ...drawn.map((d) => d.key)]);
    const sprites = layout(room, { focal: host.focal(), view: world.view, center, sizes: host.sizes, parts: host.parts, sprites: drawn });
    const ratio = window.devicePixelRatio || 1;
    host.gpu()?.draw(sprites, view, width, height, ratio, host.ground());
    // Over it: what is outside the game's view, a door's fade, and a readout.
    const c = host.canvas;
    c.width = Math.round(width * ratio);
    c.height = Math.round(height * ratio);
    c.style.width = `${width}px`;
    c.style.height = `${height}px`;
    const g = c.getContext("2d");
    if (!g) return;
    g.setTransform(ratio, 0, 0, ratio, 0, 0);
    g.clearRect(0, 0, width, height);
    const box = [width / 2 - (vw * zoom) / 2, height / 2 - (vh * zoom) / 2, vw * zoom, vh * zoom];
    g.fillStyle = "#000";
    g.fillRect(0, 0, width, box[1]);
    g.fillRect(0, box[1] + box[3], width, height);
    g.fillRect(0, 0, box[0], height);
    g.fillRect(box[0] + box[2], 0, width, height);
    const door = now[5];
    if (door) {
      g.globalAlpha = door <= DOOR / 2 ? door / (DOOR / 2) : (DOOR - door) / (DOOR / 2);
      g.fillRect(box[0], box[1], box[2], box[3]);
      g.globalAlpha = 1;
    }
    g.font = "11px ui-monospace, monospace";
    g.fillStyle = "rgba(255, 255, 255, 0.7)";
    g.fillText(`${room.id} · ${now[1]}${playback ? " · playing back" : ""}${now[6] ? ` · ${now[6]} hurt` : ""}`, box[0] + 8, box[1] + 16);
  }

  /** Whether a key is Proton's even while the game plays: ⌘ with anything
   *  (⌘S saves), ⌃ with a digit or minus (the modes), and typing in a field —
   *  the agent's, say. @param {KeyboardEvent} e */
  function proton(e) {
    const target = e.target instanceof Element ? e.target : null;
    return e.metaKey || (e.ctrlKey && /^(Digit|Minus)/.test(e.code)) ||
      Boolean(target?.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])"));
  }
  /** @param {KeyboardEvent} e */
  function keydown(e) {
    if (!playing || proton(e)) return;
    // Every other key is the game's, mapped or not: nothing reaches the
    // editor underneath, whose keys would edit the room being played.
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return stop();
    const bit = keys.get(e.code);
    if (bit !== undefined) held.add(bit);
  }
  /** @param {KeyboardEvent} e */
  function keyup(e) {
    const bit = keys.get(e.code);
    if (bit !== undefined) held.delete(bit);
    if (playing && !proton(e)) {
      e.preventDefault();
      e.stopPropagation();
    }
  }
  window.addEventListener("keydown", keydown, true);
  window.addEventListener("keyup", keyup, true);
  window.addEventListener("blur", () => held.clear());

  /** @param {boolean} on */
  function pause(on) {
    paused = on;
    last = 0;
    left = 0;
    held.clear();
    host.changed({ playing, paused, tick: now?.[1] ?? 0 });
  }

  /** End the run: what was played goes to the core to be kept and checked. */
  function stop() {
    if (!playing) return;
    playing = false;
    cancelAnimationFrame(raf);
    held.clear();
    if (!playback && recording.length && pg) {
      const hash = (BigInt(pg.pg_hash_hi() >>> 0) << 32n | BigInt(pg.pg_hash_lo() >>> 0)).toString(16).padStart(16, "0");
      host.dispatch({ type: "gameRecorded", room: start.room, at: start.at, abilities: start.abilities, replay: recording, hash });
    }
    playback = null;
    host.changed({ playing, paused: false, tick: 0 });
    host.ended();
  }

  return {
    /** Play what the core cooked. @param {{world: string, start: number[], rooms: Room[], keys: [string, number][], replay?: [number, number][] | null}} event */
    async start(event) {
      if (playing) stop();
      const module = await compile();
      const instance = await WebAssembly.instantiate(module, {});
      pg = instance.exports;
      if (pg.pg_abi() !== ABI) throw new Error(`game-runtime.wasm speaks ABI ${pg.pg_abi()}, not ${ABI}: it was built from other sources`);
      const bytes = new TextEncoder().encode(event.world);
      const at = pg.pg_alloc(bytes.length);
      new Uint8Array(pg.memory.buffer, at, bytes.length).set(bytes);
      const loaded = pg.pg_load(at, bytes.length);
      pg.pg_free(at, bytes.length);
      if (loaded !== 0) throw new Error(`the game's world did not load: ${why()}`);
      world = JSON.parse(event.world);
      dt = 1 / Math.max(1, world.tick);
      rooms = event.rooms;
      keys = new Map(event.keys);
      start = { room: event.start[0], at: [event.start[1], event.start[2]], abilities: event.start[3] };
      if (pg.pg_start(start.room, start.at[0], start.at[1], start.abilities) !== 0) throw new Error(`the run did not start: ${why()}`);
      recording = [];
      playback = event.replay ? { runs: event.replay, at: 0, left: event.replay[0]?.[1] ?? 0 } : null;
      before = null;
      now = frameWords();
      playing = true;
      paused = false;
      last = 0;
      left = 0;
      raf = requestAnimationFrame(loop);
      host.changed({ playing, paused, tick: 0 });
    },
    stop,
    /** @param {boolean} on */
    pause,
    /** One tick, paused. */
    step() {
      if (!playing) return;
      if (!paused) pause(true);
      tick();
      draw(1);
      host.changed({ playing, paused, tick: now?.[1] ?? 0 });
    },
    playing: () => playing,
    paused: () => paused,
    /** The frame's header and records, for tests. */
    frame: () => (now ? [...now] : null),
  };
}
