/* Game mode's sound: what a run's events and its animations' cues play,
 * and each area's music, crossfaded as the player goes from one area to
 * another — by Web Audio, from the game's own `.wav`, `.mp3` and `.m4a`
 * files, read as the window reads its images.
 *
 * A window may not make a sound before a person has done something in it,
 * so the context is made, or woken, by the click or key that starts a run
 * (`resume`), and sounds asked for before that are dropped rather than
 * played late. Sound is the window's alone: it never touches the run, so a
 * muted run and a loud one are the same run. */

/** `pg_step`'s bits, by the names game.json gives sounds (proton-game's
 *  `SOUND_EVENTS`). @type {[number, string][]} */
export const EVENTS = [
  [1, "jump"], [2, "land"], [4, "dash"], [8, "hurt"], [16, "enter"], [32, "gain"],
  [128, "swing"], [256, "hit"], [512, "kill"], [1024, "die"], [2048, "rest"], [4096, "flag"], [8192, "shot"],
];

/** How many of one sound may sound at once; another takes the oldest's place. */
const VOICES = 4;
/** Seconds music takes to fade out and in. */
const FADE = 1.2;

/** The files a tick plays: each event it said and each cue, that has a
 *  sound, once each. A death is the death's sound alone, not the hurt's
 *  under it too. @param {number} bits @param {string[]} cues @param {Record<string, string | undefined>} sounds @returns {string[]} */
export function soundsFor(bits, cues, sounds) {
  const said = EVENTS.filter(([bit]) => bits & bit).map(([, name]) => name);
  const names = said.includes("die") && sounds.die ? said.filter((n) => n !== "hurt") : said;
  /** @type {string[]} */
  const out = [];
  for (const name of [...names, ...cues]) {
    const file = sounds[name];
    if (file && !out.includes(file)) out.push(file);
  }
  return out;
}

/** The music a room plays: its area's, or none.
 *  @param {{rooms: {id: string, area: string}[], areas: {id: string, music?: string | null}[]}} world @param {string} room */
export function musicFor(world, room) {
  const area = world.rooms.find((r) => r.id === room)?.area;
  return world.areas.find((a) => a.id === area)?.music || null;
}

/**
 * @param {{
 *   read: (key: string) => Promise<ArrayBuffer>,
 *   volumes: () => {effects: number, music: number},
 *   muted: () => boolean,
 * }} host
 */
export function createAudio(host) {
  /** @type {AudioContext | null} */
  let ctx = null;
  /** @type {GainNode | null} */
  let effects = null;
  /** @type {GainNode | null} */
  let music = null;
  /** Each file, decoded once. @type {Map<string, Promise<AudioBuffer | null>>} */
  const buffers = new Map();
  /** Each file's voices sounding now. @type {Map<string, AudioBufferSourceNode[]>} */
  const voices = new Map();
  /** The music playing: its file, its source and its own fade. @type {{key: string, source: AudioBufferSourceNode | null, gain: GainNode} | null} */
  let playing = null;

  function context() {
    if (!ctx) {
      const Context = window.AudioContext ?? /** @type {any} */ (window).webkitAudioContext;
      ctx = /** @type {AudioContext} */ (new Context());
      effects = ctx.createGain();
      effects.connect(ctx.destination);
      music = ctx.createGain();
      music.connect(ctx.destination);
    }
    return ctx;
  }

  /** The volumes now, muted or not. */
  function level() {
    if (!effects || !music) return;
    const v = host.volumes();
    const on = host.muted() ? 0 : 1;
    effects.gain.value = Math.max(0, Math.min(1, v.effects)) * on;
    music.gain.value = Math.max(0, Math.min(1, v.music)) * on;
  }

  /** A file, decoded. @param {string} key */
  function load(key) {
    let found = buffers.get(key);
    if (!found) {
      const c = context();
      found = host
        .read(key)
        .then((bytes) => c.decodeAudioData(bytes.slice(0)))
        .catch(() => null);
      buffers.set(key, found);
    }
    return found;
  }

  return {
    /** Make or wake the context: called from the click or key that starts a run. */
    resume() {
      const c = context();
      level();
      return c.state === "running" ? Promise.resolve() : c.resume().catch(() => undefined);
    },
    /** Read and decode files before they are needed. @param {string[]} keys */
    preload(keys) {
      if (!ctx) return;
      for (const key of keys) void load(key);
    },
    /** Play a file once. @param {string} key */
    async play(key) {
      if (!ctx || ctx.state !== "running" || host.muted()) return;
      const buffer = await load(key);
      if (!buffer || !ctx || !effects) return;
      level();
      const source = ctx.createBufferSource();
      source.buffer = buffer;
      source.connect(effects);
      const list = voices.get(key) ?? [];
      if (list.length >= VOICES) list.shift()?.stop();
      list.push(source);
      voices.set(key, list);
      source.onended = () => {
        const now = voices.get(key) ?? [];
        const at = now.indexOf(source);
        if (at >= 0) now.splice(at, 1);
      };
      source.start();
    },
    /** Play `key` as the music, looping — faded in as what played fades
     *  out; none fades it out. The same music again goes on as it was.
     *  @param {string | null} key */
    async music(key) {
      if (playing?.key === (key ?? "")) return;
      const c = ctx;
      if (!c || !music) return;
      if (playing) {
        const was = playing;
        was.gain.gain.cancelScheduledValues(c.currentTime);
        was.gain.gain.setValueAtTime(was.gain.gain.value, c.currentTime);
        was.gain.gain.linearRampToValueAtTime(0, c.currentTime + FADE);
        was.source?.stop(c.currentTime + FADE);
      }
      const gain = c.createGain();
      gain.connect(music);
      playing = { key: key ?? "", source: null, gain };
      if (!key) return;
      const mine = playing;
      const buffer = await load(key);
      if (!buffer || playing !== mine) return;
      level();
      const source = c.createBufferSource();
      source.buffer = buffer;
      source.loop = true;
      source.connect(gain);
      gain.gain.setValueAtTime(0, c.currentTime);
      gain.gain.linearRampToValueAtTime(1, c.currentTime + FADE);
      source.start();
      mine.source = source;
    },
    /** The volumes or the mute changed. */
    level,
    /** End the music at once, and every sound. */
    stop() {
      playing?.source?.stop();
      playing = null;
      for (const list of voices.values()) for (const v of list) v.stop();
      voices.clear();
    },
    /** The context's state, for tests: none until one is made. */
    state: () => ctx?.state ?? "none",
    /** What the music is now, for tests. */
    playing: () => playing?.key || null,
  };
}
