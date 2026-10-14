/* Game mode's skeletons, posed: where each bone is at a moment of a clip,
 * which part each slot shows and in what colour, and where a point of a part
 * lands. The renderer's shader carries a part's points by the bones this
 * poses; what is drawn otherwise — boxes over a run, the inspector's small
 * player, the tests — is carried here, the same way. No DOM and no GL.
 *
 * A clip's keys are offsets from where each bone stands at rest: moved by
 * them, turned by them, scaled by them, each eased from one key to the next
 * as DragonBones eases (proton-game's `import::skel` keeps its keys as it
 * writes them). A bone's matrix is a, b, c, d, tx, ty — x' = a·x + c·y + tx —
 * in the skeleton's pixels, y down; its world matrix its parent's times its
 * own. */

/** @typedef {import('./bindings/GameSkel').GameSkel} Skel */
/** @typedef {import('./bindings/GameEase').GameEase} Ease */
/** @typedef {{bones: Float64Array, shown: Int32Array, colors: Float64Array}} Pose */

/** A skeleton as its file says it, every list it may leave out when empty
 *  — proton-game's writer leaves out `[]` — put back.
 *  @param {any} raw @returns {Skel} */
export function skelOf(raw) {
  const list = (/** @type {any} */ v) => (Array.isArray(v) ? v : []);
  return {
    ...raw,
    fps: raw.fps || 24,
    bones: list(raw.bones).map((b) => ({ parent: null, at: [0, 0], rot: 0, skew: 0, scale: [1, 1], length: 0, ...b })),
    slots: list(raw.slots).map((s) => ({
      bone: 0, shown: 0, color: [1, 1, 1, 1], ...s,
      parts: list(s.parts).map((m) => ({ ...m, points: list(m.points).map((p) => ({ uv: [0, 0], ...p, bones: list(p.bones) })), triangles: list(m.triangles) })),
    })),
    clips: list(raw.clips).map((c) => ({
      frames: 1, repeat: 0, ...c,
      bones: list(c.bones).map((t) => ({ ...t, moved: list(t.moved), turned: list(t.turned), scaled: list(t.scaled) })),
      slots: list(c.slots).map((t) => ({ ...t, shown: list(t.shown), color: list(t.color) })),
      cues: list(c.cues),
    })),
    boxes: list(raw.boxes).map((b) => ({ ...b, points: list(b.points) })),
  };
}

/** How far along a key's way to the next a value is at `p` (0–1) of it.
 *  @param {Ease} ease @param {number} p */
export function eased(ease, p) {
  switch (ease.kind) {
    case "step":
      return 0;
    case "in":
      return (p * p - p) * ease.amount + p;
    case "out":
      return (1 - (1 - p) * (1 - p) - p) * ease.amount + p;
    case "inOut":
      return (0.5 * (1 - Math.cos(p * Math.PI)) - p) * ease.amount + p;
    case "bezier": {
      // The curve's t where its x is p, by halving; its y there.
      const [x1, y1, x2, y2] = ease.points;
      const at = (/** @type {number} */ t, /** @type {number} */ a, /** @type {number} */ b) => 3 * (1 - t) * (1 - t) * t * a + 3 * (1 - t) * t * t * b + t * t * t;
      let lo = 0;
      let hi = 1;
      for (let i = 0; i < 30; i++) {
        const mid = (lo + hi) / 2;
        if (at(mid, x1, x2) < p) lo = mid;
        else hi = mid;
      }
      return at((lo + hi) / 2, y1, y2);
    }
    default:
      return p;
  }
}

/** A track's value at frame `f`: its first key's before it, its last's
 *  after, and between two keys, eased from one to the next.
 *  @param {{frame: number, ease: Ease}[]} keys @param {(k: any) => number[]} value @param {number} f @param {number[]} rest */
export function sample(keys, value, f, rest) {
  if (!keys.length) return rest;
  if (f <= keys[0].frame) return value(keys[0]);
  let i = 0;
  while (i + 1 < keys.length && keys[i + 1].frame <= f) i++;
  if (i === keys.length - 1) return value(keys[i]);
  const a = keys[i];
  const b = keys[i + 1];
  const span = b.frame - a.frame;
  const e = span > 0 ? eased(a.ease, (f - a.frame) / span) : 1;
  const va = value(a);
  const vb = value(b);
  return va.map((v, n) => v + (vb[n] - v) * e);
}

/** A bone's matrix from a place, a turn and a skew (degrees) and a scale.
 *  @param {number[]} at @param {number} rot @param {number} skew @param {number[]} scale */
export function matrixOf(at, rot, skew, scale) {
  const r = (rot * Math.PI) / 180;
  const k = ((rot + skew) * Math.PI) / 180;
  return [Math.cos(r) * scale[0], Math.sin(r) * scale[0], -Math.sin(k) * scale[1], Math.cos(k) * scale[1], at[0], at[1]];
}

/** `p` then `q`: what `q` places, placed by `p`. @param {ArrayLike<number>} p @param {number[]} q */
function times(p, q) {
  return [p[0] * q[0] + p[2] * q[1], p[1] * q[0] + p[3] * q[1], p[0] * q[2] + p[2] * q[3], p[1] * q[2] + p[3] * q[3], p[0] * q[4] + p[2] * q[5] + p[4], p[1] * q[4] + p[3] * q[5] + p[5]];
}

/** The skeleton posed at `frame` (a clip's own frames, fractions and all)
 *  of clip `clip` — at rest for none: each bone's world matrix, six numbers
 *  a bone; the part each slot shows (-1 none); each slot's colour.
 *  @param {Skel} skel @param {number | null} clip @param {number} frame @returns {Pose} */
export function pose(skel, clip, frame) {
  const c = clip === null ? null : skel.clips[clip] ?? null;
  const n = skel.bones.length;
  const bones = new Float64Array(n * 6);
  const tracks = new Map((c?.bones ?? []).map((t) => [t.bone, t]));
  skel.bones.forEach((b, i) => {
    const t = tracks.get(i);
    const moved = t ? sample(t.moved, (k) => k.value, frame, [0, 0]) : [0, 0];
    const turned = t ? sample(t.turned, (k) => k.value, frame, [0, 0]) : [0, 0];
    const scaled = t ? sample(t.scaled, (k) => k.value, frame, [1, 1]) : [1, 1];
    const local = matrixOf([b.at[0] + moved[0], b.at[1] + moved[1]], b.rot + turned[0], b.skew + turned[1], [b.scale[0] * scaled[0], b.scale[1] * scaled[1]]);
    const world = b.parent === null || b.parent === undefined ? local : times(bones.subarray(b.parent * 6, b.parent * 6 + 6), local);
    bones.set(world, i * 6);
  });
  const shown = new Int32Array(skel.slots.length);
  const colors = new Float64Array(skel.slots.length * 4);
  const slotTracks = new Map((c?.slots ?? []).map((t) => [t.slot, t]));
  skel.slots.forEach((s, i) => {
    const t = slotTracks.get(i);
    let part = s.shown;
    for (const [at, value] of t?.shown ?? []) if (at <= frame) part = value;
    shown[i] = part;
    colors.set(t?.color.length ? sample(t.color, (k) => k.color, frame, s.color) : s.color, i * 4);
  });
  return { bones, shown, colors };
}

/** Where a point of a part lands, posed: each bone's place for it, by how
 *  much that bone carries it. @param {Pose} posed @param {[number, number, number[]][]} carried */
export function skin(posed, carried) {
  let x = 0;
  let y = 0;
  for (const [bone, weight, p] of carried) {
    const m = posed.bones.subarray(bone * 6, bone * 6 + 6);
    x += (m[0] * p[0] + m[2] * p[1] + m[4]) * weight;
    y += (m[1] * p[0] + m[3] * p[1] + m[5]) * weight;
  }
  return [x, y];
}

/** Each clip's ticks at `tick` a second — what proton-game's `clip_ticks`
 *  counts, the same rounding — as the run of a run's frames it is.
 *  @param {Skel} skel @param {number} tick @returns {{from: number, to: number}[]} */
export function clipTicks(skel, tick) {
  let from = 0;
  return skel.clips.map((c) => {
    const ticks = Math.max(1, Math.round((Math.max(1, c.frames) / Math.max(1, skel.fps)) * tick));
    const run = { from, to: from + ticks - 1 };
    from += ticks;
    return run;
  });
}

/** The clip, and the frame of it, a run's frame is — a run counts a
 *  skeleton's clips in ticks, one after another; at rest, none.
 *  @param {Skel} skel @param {number} tick @param {number | null | undefined} frame @returns {{clip: number | null, frame: number}} */
export function momentOf(skel, tick, frame) {
  if (frame === null || frame === undefined || frame < 0) return { clip: restingClip(skel), frame: 0 };
  const runs = clipTicks(skel, tick);
  const clip = runs.findIndex((r) => frame >= r.from && frame <= r.to);
  if (clip < 0) return { clip: null, frame: 0 };
  return { clip, frame: ((frame - runs[clip].from) / tick) * skel.fps };
}

/** The clip a skeleton rests in: one called idle, else the first.
 *  @param {Skel} skel @param {Record<string, string | undefined>} [clips] */
export function restingClip(skel, clips) {
  const name = clips?.idle;
  const i = skel.clips.findIndex((c) => c.name === name);
  if (i >= 0) return i;
  const idle = skel.clips.findIndex((c) => c.name.toLowerCase() === "idle");
  return idle >= 0 ? idle : skel.clips.length ? 0 : null;
}

/** The boxes shown in a pose, their corners in the skeleton's pixels: a box
 *  is shown while its slot shows the part it is.
 *  @param {Skel} skel @param {Pose} posed */
export function boxesOf(skel, posed) {
  return skel.boxes
    .filter((b) => posed.shown[b.slot] === b.part)
    .map((b) => ({ name: b.name, kind: b.kind, points: b.points.map((p) => skin(posed, [[b.bone, 1, p]])) }));
}

/** A pose's bones as the shader takes them: two vec4s a bone — a, b, c, d
 *  and tx, ty. @param {Pose} posed */
export function boneUniforms(posed) {
  const n = posed.bones.length / 6;
  const out = new Float32Array(n * 8);
  for (let i = 0; i < n; i++) {
    const m = posed.bones.subarray(i * 6, i * 6 + 6);
    out.set([m[0], m[1], m[2], m[3], m[4], m[5], 0, 0], i * 8);
  }
  return out;
}
