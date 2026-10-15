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
 * and Esc still are Proton's: Esc stops.
 *
 * An animated thing's record names the frame it shows, which is drawn from
 * its sheet; the cues its frames say are shown as they are said. Each record
 * says who it is — the player, a thing in the room, a shot — so what moves
 * is eased from where that one was; a thing just hurt is drawn white, the
 * player spared after a hurt faint. The player's health is pips over the
 * picture, and the boxes the run tests this tick — hitboxes red, hurtboxes
 * green — are drawn over it when asked: the run's own, not worked out again.
 *
 * What happens is felt as well as seen: dust as the player jumps and lands,
 * sparks as a blow lands and a burst as a thing is beaten, the body
 * stretching into a jump and squashing as it lands, the screen shaking at a
 * blow — all from the tick each happened at, so a run played back looks the
 * same — and each event's sound, and the area's music as rooms change. */

import { soundsFor } from "./game-audio.js";
import { living, noise } from "./game-fx.js";
import { GLOW, layout, lightingOf } from "./game-scene.js";

/** @typedef {import('./bindings/GameRoom').GameRoom} Room */

/** The runtime's ABI, and the words in a frame's header and in each record
 *  (proton-game-runtime). */
const ABI = 3;
const HEADER = 16;
const RECORD = 9;
/** A record's look bits. */
const FLIP = 1;
const FLASH = 2;
const BLINK = 4;
/** A shot's `who` is this and its number. */
const SHOT = 1 << 20;
/** `pg_step`'s bits that make something to see. */
const JUMPED = 1;
const LANDED = 2;
const DASHED = 4;
const HURT = 8;
const HIT = 256;
const KILLED = 512;
const DIED = 1024;
const RESTED = 2048;
/** Ticks a squash or stretch, and a shake, take to settle. */
const SQUASH = 10;
const SHAKE = 14;
/** What `pg_step` says when a frame with a cue was shown. */
const CUE = 64;
/** Ticks a cue stays in the readout. */
const CUE_SHOWN = 45;
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
 *   anims?: Map<string, import('./game-scene.js').Anim>,
 *   skels?: Map<string, import('./bindings/GameSkel').GameSkel>,
 *   boxes?: () => boolean,
 *   post?: () => {bloom: number, vignette: number, grade: string | null, quality: "low" | "medium" | "high"},
 *   times?: () => boolean,
 *   feel?: () => {shake: number, squash: number} | undefined,
 *   audio?: ReturnType<typeof import('./game-audio.js').createAudio>,
 *   sounds?: () => Record<string, string | undefined>,
 *   music?: (room: string) => string | null,
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
  /** The last cue said, and the tick it was said at. @type {{name: string, tick: number} | null} */
  let cue = null;
  /** How long frames take, eased: between frames, and drawing one (ms). */
  const times = { frame: 0, draw: 0, last: 0 };
  /** Bursts begun by what happened. @type {import('./game-fx.js').Spark[]} */
  let sparks = [];
  /** The player's last squash or stretch, and the screen's last shake: the tick each began at.
   *  @type {{tick: number, stretch: boolean} | null} */
  let squash = null;
  /** @type {{tick: number, amount: number} | null} */
  let shake = null;

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
    const happened = pg.pg_step(bits);
    before = now;
    now = frameWords();
    /** @type {string[]} */
    let said = [];
    if (happened & CUE) {
      const at = pg.pg_cues();
      const words = new Uint32Array(pg.memory.buffer, at, 1 + new Uint32Array(pg.memory.buffer, at, 1)[0]);
      said = [...words.slice(1)].map((i) => world?.cues?.[i] ?? "").filter(Boolean);
      if (said.length) cue = { name: said.join(", "), tick: now[1] };
    }
    felt(happened);
    for (const file of soundsFor(happened, said, host.sounds?.() ?? {})) void host.audio?.play(file);
    if (!before || before[2] !== now[2]) void host.audio?.music(host.music?.(rooms[now[2]]?.id ?? "") ?? null);
  }

  /** Each record of a frame, by who it is. @param {Int32Array | null} words */
  function records(words) {
    /** @type {Map<number, {x: number, y: number, bits: number}>} */
    const out = new Map();
    for (let i = 0; words && i < words[8]; i++) {
      const o = HEADER + i * RECORD;
      out.set(words[o + 8], { x: words[o + 1], y: words[o + 2], bits: words[o + 5] });
    }
    return out;
  }

  /** What the tick did, as something to see: bursts where it happened, a
   *  squash or stretch, a shake. @param {number} happened */
  function felt(happened) {
    if (!now || !world) return;
    const tick = now[1];
    const time = tick / Math.max(1, world.tick);
    const seed = (/** @type {number} */ who) => Math.imul(tick, 131) + who;
    const mine = records(now);
    const was = before && before[2] === now[2] ? records(before) : new Map();
    const me = mine.get(0);
    const feet = /** @type {[number, number]} */ ([now[9] / 256, now[10] / 256]);
    const flip = Boolean((me?.bits ?? 0) & FLIP);
    const spark = (/** @type {string} */ kind, /** @type {[number, number]} */ at, /** @type {number} */ who, turned = false) =>
      sparks.push({ kind, at, start: time, seed: seed(who), flip: turned, tall: world.view[1] });
    if (happened & JUMPED) {
      spark("jump", feet, 0);
      squash = { tick, stretch: true };
    }
    if (happened & LANDED) {
      spark("land", feet, 0);
      spark("land", feet, 1, true);
      squash = { tick, stretch: false };
    }
    if (happened & DASHED) spark("dash", [feet[0], feet[1] - 30], 0, !flip);
    if (happened & HURT) {
      spark("hurt", [feet[0], feet[1] - 36], 0);
      shake = { tick, amount: 1 };
    }
    if (happened & DIED) {
      spark("kill", [feet[0], feet[1] - 36], 0);
      shake = { tick, amount: 1 };
    }
    if (happened & RESTED) spark("rest", [feet[0], feet[1] - 20], 0);
    if (happened & HIT) {
      // Sparks on what was struck: whatever has just begun to flash.
      for (const [who, r] of mine) {
        if (who > 0 && who < SHOT && r.bits & FLASH && !((was.get(who)?.bits ?? 0) & FLASH)) spark("hit", [r.x, r.y - 20], who);
      }
      shake = { tick, amount: Math.max(shake && tick - shake.tick < SHAKE ? shake.amount : 0, 0.35) };
    }
    if (happened & KILLED) {
      // A burst where what was beaten stood last.
      for (const [who, r] of was) if (who > 0 && who < SHOT && !mine.has(who)) spark("kill", [r.x, r.y - 20], who);
      shake = { tick, amount: Math.max(shake && tick - shake.tick < SHAKE ? shake.amount : 0, 0.6) };
    }
    sparks = living(sparks, time);
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
    const feel = host.feel?.() ?? { shake: 0.5, squash: 0.5 };
    // A shake: a jolt a tick, fading.
    const age = (/** @type {{tick: number}} */ from) => (now?.[1] ?? 0) + alpha - from.tick;
    if (shake && age(shake) < SHAKE && feel.shake > 0) {
      const amp = shake.amount * feel.shake * 12 * (1 - age(shake) / SHAKE);
      const t = now[1];
      center[0] += (noise(t, 1, 0) - 0.5) * 2 * amp;
      center[1] += (noise(t, 2, 0) - 0.5) * 2 * amp;
    }
    const [vw, vh] = world.view;
    const zoom = Math.min(width / vw, height / vh);
    const view = { scale: zoom, x: width / 2 - center[0] * zoom, y: height / 2 - center[1] * zoom };
    // Where each was the tick before, by who it is.
    /** @type {Map<number, number>} */
    const was = new Map();
    if (before && before[2] === now[2]) for (let i = 0; i < before[8]; i++) was.set(before[HEADER + i * RECORD + 8], HEADER + i * RECORD);
    /** @type {import('./game-scene.js').Standing[]} */
    const drawn = [];
    for (let i = 0; i < now[8]; i++) {
      const o = HEADER + i * RECORD;
      const p = was.get(now[o + 8]);
      const at = p !== undefined && before ? [before[p + 1] + (now[o + 1] - before[p + 1]) * alpha, before[p + 2] + (now[o + 2] - before[p + 2]) * alpha] : [now[o + 1], now[o + 2]];
      const bits = now[o + 5];
      const key = now[o] >= 0 ? world.sprites[now[o]] : GLOW;
      // A shot is drawn about its middle.
      const size = now[o + 8] >= SHOT && key !== GLOW ? host.sizes.get(key) : undefined;
      const pivot = size ? [size[0] / 2, size[1] / 2] : [now[o + 3], now[o + 4]];
      /** @type {[number, number] | undefined} */
      let scale;
      if (now[o + 8] === 0 && squash && age(squash) < SQUASH && feel.squash > 0) {
        // Stretched into a jump, squashed by a landing, easing back.
        const k = (1 - age(squash) / SQUASH) * feel.squash;
        scale = squash.stretch ? [1 - 0.25 * k, 1 + 0.3 * k] : [1 + 0.3 * k, 1 - 0.25 * k];
      }
      drawn.push({ key, at, pivot, flip: Boolean(bits & FLIP), frame: now[o + 6] >= 0 ? now[o + 6] : null, flash: Boolean(bits & FLASH), blink: Boolean(bits & BLINK), scale });
    }
    host.want([...room.planes.flatMap((p) => p.items.map((i) => i.image)), ...drawn.map((d) => d.key).filter((k) => k !== GLOW)]);
    // Lights waver by the run's own clock, so a run played back wavers alike.
    const time = (now[1] + alpha) / Math.max(1, world.tick);
    const sprites = layout(room, { focal: host.focal(), view: world.view, center, sizes: host.sizes, parts: host.parts, anims: host.anims, skels: host.skels, tick: world.tick, time, sprites: drawn, sparks });
    const ratio = window.devicePixelRatio || 1;
    const began = performance.now();
    host.gpu()?.draw(sprites, view, width, height, ratio, host.ground(), { ...lightingOf(room, { focal: host.focal(), view: world.view, center, time }), post: host.post?.() ?? null });
    const spent = performance.now() - began;
    times.draw = times.draw ? times.draw * 0.9 + spent * 0.1 : spent;
    if (times.last) times.frame = times.frame ? times.frame * 0.9 + (began - times.last) * 0.1 : began - times.last;
    times.last = began;
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
    // The boxes the run tests now, when asked: count, then hit or hurt and
    // left, top, right, bottom in 1/256 px.
    if (host.boxes?.()) {
      const at = pg.pg_boxes();
      const n = new Int32Array(pg.memory.buffer, at, 1)[0];
      const words = new Int32Array(pg.memory.buffer, at, 1 + n * 5);
      g.lineWidth = 1.5;
      for (let i = 0; i < n; i++) {
        const [hit, l, t, r, b] = words.subarray(1 + i * 5, 6 + i * 5);
        g.strokeStyle = hit ? "#ff4d5e" : "#4dd88a";
        g.strokeRect(view.x + (l / 256) * view.scale, view.y + (t / 256) * view.scale, ((r - l) / 256) * view.scale, ((b - t) / 256) * view.scale);
      }
    }
    // Health, as pips: whole, and lost.
    const most = now[13];
    for (let i = 0; i < most; i++) {
      g.beginPath();
      g.arc(box[0] + 14 + i * 16, box[1] + 30, 5.5, 0, Math.PI * 2);
      g.fillStyle = i < now[12] ? "rgba(255, 255, 255, 0.92)" : "rgba(255, 255, 255, 0.15)";
      g.fill();
    }
    g.font = "11px ui-monospace, monospace";
    g.fillStyle = "rgba(255, 255, 255, 0.7)";
    const said = cue && now[1] - cue.tick < CUE_SHOWN ? ` · ${cue.name}` : "";
    g.fillStyle = "rgba(255, 255, 255, 0.7)";
    g.fillText(`${room.id} · ${now[1]}${playback ? " · playing back" : ""}${now[6] ? ` · ${now[6]} ${most ? (now[6] === 1 ? "death" : "deaths") : "hurt"}` : ""}${now[14] ? ` · ${now[14]} beaten` : ""}${said}`, box[0] + 8, box[1] + 16);
    if (host.times?.()) {
      // The draw's own cost is the GPU's work handed over, not waited for.
      g.fillText(`${times.frame.toFixed(1)} ms a frame · ${times.draw.toFixed(1)} ms to draw · ${sprites.length} sprites`, box[0] + 8, box[1] + (most ? 50 : 30));
    }
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
    host.audio?.stop();
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
      cue = null;
      sparks = [];
      squash = null;
      shake = null;
      now = frameWords();
      host.audio?.preload(Object.values(host.sounds?.() ?? {}).filter((f) => typeof f === "string"));
      void host.audio?.music(host.music?.(rooms[now[2]]?.id ?? "") ?? null);
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
    /** The boxes the run tests now, for tests: [hit, left, top, right, bottom] in 1/256 px. */
    boxes: () => {
      if (!pg || !now) return [];
      const at = pg.pg_boxes();
      const n = new Int32Array(pg.memory.buffer, at, 1)[0];
      const words = [...new Int32Array(pg.memory.buffer, at, 1 + n * 5)];
      return Array.from({ length: n }, (_, i) => words.slice(1 + i * 5, 6 + i * 5));
    },
    /** The last cue said, for tests. */
    cue: () => cue?.name ?? null,
    /** How long frames take, for tests and the readout. */
    times: () => ({ ...times }),
    /** The bursts flying now, by kind, for tests. */
    sparks: () => sparks.map((s) => s.kind),
  };
}
