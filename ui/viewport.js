/* Modeling's viewport.
 *
 * ## What this draws
 *
 * `scene.proton.json` holds transforms and kinds; a node that has geometry
 * names the file it is in, `Event::Geometry` brings the triangles, and
 * `Event::Scene` brings its material already compiled into a short program.
 * So an object with a mesh is drawn as its surface — the material run at each
 * triangle, lit, back faces culled, sorted back to front — and everything else
 * is drawn as a *proxy*: a frustum for a camera, a burst for a light, an axis
 * cross for an empty, and a box for a mesh whose geometry has not arrived,
 * could not be read, or is too large to draw.
 *
 * A proxy box is a stand-in and looks like one. That is deliberate: the thing
 * this must never do is draw a convincing lump and let somebody believe their
 * mesh looks like that. The same goes for a material this cannot run — it
 * draws in a plain grey rather than in a guess.
 *
 * ## Why a 2D canvas, and where it stops
 *
 * This began as wireframes, which need no shaders and no depth buffer. Solid
 * triangles stretch that further than it wants to go, and the cost was
 * measured rather than guessed — the table is in `proton-core::modeling`. The
 * short version: a 2D canvas charges about 0.8 µs per drawn triangle, because
 * a solid is one `fill()` per triangle, and 80,000 triangles is where orbiting
 * stops holding 30 fps. That number is `BUDGET`, and past it the core sends a
 * mesh's bounding box instead of its vertices.
 *
 * A painter's sort is also not a depth buffer: triangles that intersect, or
 * that overlap in a cycle, show through one another. For reading a scene's
 * layout that is exact enough; for anything more it is not.
 *
 * And the shading is per *face*, because a 2D canvas fills flat polygons. A
 * material is run once for a whole triangle, so a texture is only as fine as
 * the mesh is and a sphere shows its facets.
 *
 * Both are the same conclusion, which the first version of this file already
 * predicted: past this point the right move is WebGL, not a bigger number. It
 * is deliberately small so that swap stays cheap.
 */

/* ── small matrix maths ──────────────────────────────────────────────
 *
 * Column-major 4x4 as a flat 16-array, the same convention WebGL uses, so
 * none of this has to be transposed if it is ever handed to a GPU. */

const DEG = Math.PI / 180;

/** The vertical angle the camera sees, in radians.
 *
 * The badge in the corner says "50mm", so this has to be the angle a 50mm
 * lens covers on full frame *vertically*: 2·atan(12/50), about 27°. It was
 * 39.6° — which is the number a search for "50mm field of view" returns,
 * because that is the *horizontal* angle across a 36mm frame. Applied to the
 * height it made a 33mm lens wearing a 50mm label.
 *
 * One constant, used by the projection, the picking and the framing, so those
 * three can never disagree about what the camera is.
 */
const FOV = 2 * Math.atan(12 / 50);

function multiply(a, b) {
  const out = new Array(16).fill(0);
  for (let col = 0; col < 4; col++) {
    for (let row = 0; row < 4; row++) {
      let sum = 0;
      for (let k = 0; k < 4; k++) sum += a[k * 4 + row] * b[col * 4 + k];
      out[col * 4 + row] = sum;
    }
  }
  return out;
}

/** Translate, rotate (XYZ, degrees), scale — applied scale first, as every
 *  modelling tool means by a transform. */
function compose(position, rotation, scale) {
  const [rx, ry, rz] = rotation.map((d) => d * DEG);
  const [cx, sx] = [Math.cos(rx), Math.sin(rx)];
  const [cy, sy] = [Math.cos(ry), Math.sin(ry)];
  const [cz, sz] = [Math.cos(rz), Math.sin(rz)];

  // R = Rz * Ry * Rx, written out rather than multiplied, because three
  // matrix products per object per frame is work for no clarity.
  const r = [
    cy * cz,
    cy * sz,
    -sy,
    sx * sy * cz - cx * sz,
    sx * sy * sz + cx * cz,
    sx * cy,
    cx * sy * cz + sx * sz,
    cx * sy * sz - sx * cz,
    cx * cy,
  ];

  return [
    r[0] * scale[0], r[1] * scale[0], r[2] * scale[0], 0,
    r[3] * scale[1], r[4] * scale[1], r[5] * scale[1], 0,
    r[6] * scale[2], r[7] * scale[2], r[8] * scale[2], 0,
    position[0], position[1], position[2], 1,
  ];
}

function apply(m, [x, y, z]) {
  return [
    m[0] * x + m[4] * y + m[8] * z + m[12],
    m[1] * x + m[5] * y + m[9] * z + m[13],
    m[2] * x + m[6] * y + m[10] * z + m[14],
  ];
}

/** A view matrix looking from `eye` at `at`, with +Y up. */
function lookAt(eye, at) {
  const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
  const norm = (v) => {
    const len = Math.hypot(v[0], v[1], v[2]) || 1;
    return [v[0] / len, v[1] / len, v[2] / len];
  };
  const cross = (a, b) => [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ];
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

  // Looking down -Z, as OpenGL does.
  const f = norm(sub(eye, at));
  // Straight up or straight down leaves the side vector undefined. The pitch
  // is clamped away from the poles below, so this is belt and braces.
  const up = Math.abs(f[1]) > 0.999 ? [0, 0, 1] : [0, 1, 0];
  const s = norm(cross(up, f));
  const u = cross(f, s);

  return [
    s[0], u[0], f[0], 0,
    s[1], u[1], f[1], 0,
    s[2], u[2], f[2], 0,
    -dot(s, eye), -dot(u, eye), -dot(f, eye), 1,
  ];
}

/* ── the proxies ─────────────────────────────────────────────────────
 *
 * Each is a list of line segments in local space, in metres. A mesh is a unit
 * cube because that is the honest stand-in for geometry nobody has loaded; the
 * others are the shapes every modelling tool draws for things that have no
 * surface of their own. */

const HALF = 0.5;
const CUBE = (() => {
  const c = [-HALF, HALF];
  const corners = [];
  for (const x of c) for (const y of c) for (const z of c) corners.push([x, y, z]);
  const lines = [];
  for (let i = 0; i < corners.length; i++) {
    for (let j = i + 1; j < corners.length; j++) {
      // An edge joins corners that differ in exactly one axis.
      const differs = corners[i].filter((v, k) => v !== corners[j][k]).length;
      if (differs === 1) lines.push([corners[i], corners[j]]);
    }
  }
  return lines;
})();

const CROSS = [
  [[-0.35, 0, 0], [0.35, 0, 0]],
  [[0, -0.35, 0], [0, 0.35, 0]],
  [[0, 0, -0.35], [0, 0, 0.35]],
];

const BURST = (() => {
  const lines = [...CROSS];
  const r = 0.28;
  for (const [dx, dy] of [[1, 1], [1, -1], [-1, 1], [-1, -1]]) {
    lines.push([[dx * r * 0.4, dy * r * 0.4, 0], [dx * r, dy * r, 0]]);
  }
  return lines;
})();

/** A camera points down its own -Z, so the frustum opens away from the body. */
const FRUSTUM = (() => {
  const d = -0.6;
  const w = 0.3;
  const far = [
    [-w, -w, d],
    [w, -w, d],
    [w, w, d],
    [-w, w, d],
  ];
  const lines = far.map((corner) => [[0, 0, 0], corner]);
  for (let i = 0; i < 4; i++) lines.push([far[i], far[(i + 1) % 4]]);
  return lines;
})();

/** The twelve edges of a box, from a `[minx, miny, minz, maxx, maxy, maxz]`. */
function boxLines(bounds) {
  const corners = [];
  for (const x of [bounds[0], bounds[3]]) {
    for (const y of [bounds[1], bounds[4]]) {
      for (const z of [bounds[2], bounds[5]]) corners.push([x, y, z]);
    }
  }
  const lines = [];
  for (let i = 0; i < corners.length; i++) {
    for (let j = i + 1; j < corners.length; j++) {
      const differs = corners[i].filter((v, k) => v !== corners[j][k]).length;
      if (differs === 1) lines.push([corners[i], corners[j]]);
    }
  }
  return lines;
}

/** What to draw in wireframe for a node, or `null` when its surface is drawn.
 *
 * A mesh whose geometry is loaded needs no proxy — the triangles are the
 * answer. A mesh whose geometry is missing, unreadable or too large gets a
 * box, and when the loader managed to read far enough to know the real extent
 * the box is that extent rather than a unit cube standing in for it. A box the
 * true size of the thing is a useful stand-in; one that is always a metre
 * across is a picture of the loader's ignorance. */
function proxyFor(node) {
  switch (node.kind) {
    case "light":
      return BURST;
    case "camera":
      return FRUSTUM;
    case "mesh": {
      const mesh = node.geometry ? geometry.get(node.geometry) : null;
      if (mesh?.positions) return null;
      if (mesh && mesh.bounds && mesh.bounds[3] > mesh.bounds[0]) {
        return boxLines(mesh.bounds);
      }
      return CUBE;
    }
    default:
      return CROSS;
  }
}

/* ── materials ───────────────────────────────────────────────────────
 *
 * A material arrives compiled: a dozen instructions, in dependency order, that
 * `proton-scene` produced from the graph in the file. What a graph *means* is
 * the core's business; running it is this file's.
 *
 * It is run once per triangle, at the triangle's middle, in the object's own
 * space. That is the honest limit of a canvas that fills flat polygons: a
 * pattern is as fine as the mesh is, and a noise on eight triangles looks like
 * eight facets. A GPU would run the same program per pixel — which is the same
 * conclusion this file keeps reaching, and the same answer: WebGL. */

const shaders = new Map();

/** Take the compiled materials, as `Event::Scene` sends them. */
export function setShaders(list) {
  const before = shaders.size;
  let moved = list.length !== before;
  for (const one of list ?? []) {
    // Stamped, so a slider that changed nothing does not throw away every
    // surface in the scene and work them all out again.
    const stamp = JSON.stringify(one);
    if (shaders.get(one.id)?.stamp !== stamp) moved = true;
    shaders.set(one.id, { ...one, stamp });
  }
  if (moved) {
    surfaces.clear();
    drawViewport();
  }
}

/* Surface properties per triangle, by geometry key and material.
 *
 * They depend on the mesh and the material and on nothing else — not on the
 * camera — so they are worked out when one of those changes and kept. The
 * lighting is what runs per frame. */
const surfaces = new Map();

/** A deterministic value in 0..1 from a lattice point. */
function hash(x, y, z) {
  let n = (x | 0) * 374761393 + (y | 0) * 668265263 + (z | 0) * 2147483647;
  n = (n ^ (n >>> 13)) >>> 0;
  n = Math.imul(n, 1274126177) >>> 0;
  return ((n ^ (n >>> 16)) >>> 0) / 4294967295;
}

/** Value noise: the lattice, smoothly interpolated. */
function lattice(x, y, z) {
  const [ix, iy, iz] = [Math.floor(x), Math.floor(y), Math.floor(z)];
  const [fx, fy, fz] = [x - ix, y - iy, z - iz];
  // Smoothstep on each axis, so the lattice does not show as a grid of creases.
  const sx = fx * fx * (3 - 2 * fx);
  const sy = fy * fy * (3 - 2 * fy);
  const sz = fz * fz * (3 - 2 * fz);
  const mix = (a, b, t) => a + (b - a) * t;
  const face = (dz) =>
    mix(
      mix(hash(ix, iy, iz + dz), hash(ix + 1, iy, iz + dz), sx),
      mix(hash(ix, iy + 1, iz + dz), hash(ix + 1, iy + 1, iz + dz), sx),
      sy,
    );
  return mix(face(0), face(1), sz);
}

/** Octaves of it, each half the amplitude and twice the frequency. */
function fbm(x, y, z, detail) {
  let sum = 0;
  let amp = 0.5;
  let total = 0;
  for (let octave = 0; octave < Math.max(1, Math.min(8, Math.round(detail))); octave++) {
    sum += amp * lattice(x, y, z);
    total += amp;
    amp *= 0.5;
    x *= 2;
    y *= 2;
    z *= 2;
  }
  return total > 0 ? sum / total : 0;
}

/** Run a program at one point. `out` is filled with three channels per
 *  register, and is reused between triangles. */
function run(ops, out, px, py, pz) {
  for (let i = 0; i < ops.length; i++) {
    const op = ops[i];
    const at = i * 3;
    switch (op.op) {
      case "constant":
        out[at] = op.value[0];
        out[at + 1] = op.value[1];
        out[at + 2] = op.value[2];
        break;
      case "position":
        out[at] = px;
        out[at + 1] = py;
        out[at + 2] = pz;
        break;
      case "noise": {
        const from = op.at * 3;
        const n = fbm(
          out[from] * op.scale,
          out[from + 1] * op.scale,
          out[from + 2] * op.scale,
          op.detail,
        );
        out[at] = n;
        out[at + 1] = n;
        out[at + 2] = n;
        break;
      }
      case "checker": {
        const from = op.at * 3;
        const cell =
          (Math.floor(out[from] * op.scale) +
            Math.floor(out[from + 1] * op.scale) +
            Math.floor(out[from + 2] * op.scale)) &
          1;
        out[at] = cell;
        out[at + 1] = cell;
        out[at + 2] = cell;
        break;
      }
      case "mix": {
        const a = op.a * 3;
        const b = op.b * 3;
        const t = Math.max(0, Math.min(1, out[op.fac * 3]));
        for (let k = 0; k < 3; k++) {
          out[at + k] = out[a + k] + (out[b + k] - out[a + k]) * t;
        }
        break;
      }
      default:
        // A program from a later core. Nothing sensible to compute, and
        // guessing would put a colour on screen the file does not describe.
        out[at] = 0.8;
        out[at + 1] = 0.8;
        out[at + 2] = 0.8;
    }
  }
}

/** What a node's surface is made of, per triangle. */
function surfaceFor(node, mesh) {
  const shader = node.material ? shaders.get(node.material) : null;
  const usable = shader && !shader.problem && shader.ops.length;
  const key = `${node.geometry}|${usable ? shader.id : ""}`;
  const stamp = usable ? shader.stamp : "";
  const had = surfaces.get(key);
  // Both halves: the material may have changed under the same mesh, and the
  // mesh may have changed under the same key.
  if (had && had.stamp === stamp && had.serial === mesh.serial) return had;

  const faces = mesh.triangles.length / 3;
  const base = new Float32Array(faces * 3);
  const metal = new Float32Array(faces);
  const rough = new Float32Array(faces);

  if (!usable) {
    // No material, or one this build cannot render. A plain grey that is
    // visibly *plain* — the panel is where the reason belongs.
    base.fill(0.78);
    metal.fill(0);
    rough.fill(0.55);
  } else {
    const out = new Float32Array(shader.ops.length * 3);
    const points = mesh.positions;
    const tris = mesh.triangles;
    for (let f = 0; f < faces; f++) {
      const a = tris[f * 3] * 3;
      const b = tris[f * 3 + 1] * 3;
      const c = tris[f * 3 + 2] * 3;
      // The middle of the triangle, which is the one point on it a flat fill
      // can honestly be said to have.
      run(
        shader.ops,
        out,
        (points[a] + points[b] + points[c]) / 3,
        (points[a + 1] + points[b + 1] + points[c + 1]) / 3,
        (points[a + 2] + points[b + 2] + points[c + 2]) / 3,
      );
      base[f * 3] = out[shader.base * 3];
      base[f * 3 + 1] = out[shader.base * 3 + 1];
      base[f * 3 + 2] = out[shader.base * 3 + 2];
      metal[f] = Math.max(0, Math.min(1, out[shader.metallic * 3]));
      rough[f] = Math.max(0.03, Math.min(1, out[shader.roughness * 3]));
    }
  }

  const made = { base, metal, rough, stamp, serial: mesh.serial };
  surfaces.set(key, made);
  return made;
}

/* Colour strings, by quantised colour.
 *
 * A shaded colour varies per triangle, so the fixed ramp of ready-made strings
 * that flat shading used will not do — and building one string per triangle is
 * forty thousand allocations a frame, which is the garbage the scratch buffers
 * exist to avoid. Quantising to 32 steps a channel means a scene reuses a few
 * hundred strings between them, and the banding is invisible on a surface that
 * is already one flat tone per triangle. */
const tones = new Map();
function toneOf(key) {
  let text = tones.get(key);
  if (text === undefined) {
    const r = (key >> 10) & 31;
    const g = (key >> 5) & 31;
    const b = key & 31;
    text = `rgb(${r << 3},${g << 3},${b << 3})`;
    tones.set(key, text);
  }
  return text;
}

/* ── the camera the viewer drives ───────────────────────────────────── */

const camera = { yaw: 0.7, pitch: 0.5, dist: 12, target: [0, 0, 0] };

/* ── geometry ────────────────────────────────────────────────────────
 *
 * Kept by the path the scene writes, which is what both ends key it by. A mesh
 * arrives once and is reused by every object naming it, so ten chairs sharing
 * `chair.obj` cost one load and one copy. */

const geometry = new Map();

/** Bumped on every delivery, so a stale derived thing can be spotted. */
let serial = 0;

/** Take one mesh, as `Event::Geometry` sends it.
 *
 * Converted to typed arrays here, once, rather than on every frame: a plain JS
 * array of 200,000 numbers is 200,000 boxed doubles, and reading it in the
 * draw loop would be most of the frame. */
export function setGeometry(what) {
  if (!what?.path) return;
  // Counted, so anything derived from a mesh can tell one delivery from the
  // next. A key does not move when a modifier's setting changes — the mesh
  // under it does — and a cache that only watched the key kept a surface with
  // fewer entries than the new mesh has triangles. Reading past the end of it
  // gave `undefined`, which arithmetic turns into `NaN`, which quantises to
  // black. Every subdivided object rendered as a silhouette.
  serial += 1;
  if (!what.mesh) {
    // No vertices: unreadable, or over the budget. The bounds are still real,
    // so the box drawn in its place is the object's true extent rather than a
    // unit cube pretending to be one.
    geometry.set(what.path, {
      positions: null,
      bounds: what.bounds,
      count: what.triangles,
      problem: what.problem ?? "",
      serial,
    });
  } else {
    geometry.set(what.path, {
      positions: Float32Array.from(what.mesh.positions),
      triangles: Uint32Array.from(what.mesh.triangles),
      bounds: what.mesh.bounds,
      closed: what.mesh.closed,
      count: what.mesh.triangles.length / 3,
      problem: what.problem ?? "",
      serial,
    });
  }
  drawViewport();
}

/** Whether a mesh is already held, so nothing is asked for twice. */
export function hasGeometry(path) {
  return geometry.has(path);
}

let lastDrawn = 0;

/* Scratch space, grown and reused.
 *
 * Measuring the fill rate turned up 200–400 ms outliers among 20 ms frames,
 * and they were garbage collections of the arrays the measurement itself was
 * allocating each frame. A viewport that stutters once a second is worse than
 * one that is uniformly slower, so nothing here is allocated per frame. */
const scratch = {
  // Per vertex of the object being walked: where it is on screen, where it is
  // in front of the eye, and whether it is in front of the eye at all.
  sx: new Float32Array(0),
  sy: new Float32Array(0),
  ex: new Float32Array(0),
  ey: new Float32Array(0),
  ez: new Float32Array(0),
  ok: new Uint8Array(0),
  // Per gathered face, across the whole scene: its three screen corners, how
  // far away it is, and the quantised colour it came out.
  corner: new Float32Array(0),
  depth: new Float32Array(0),
  tone: new Uint32Array(0),
  order: new Uint32Array(0),
};

function room(vertices, faces) {
  if (scratch.ok.length < vertices) {
    for (const key of ["sx", "sy", "ex", "ey", "ez"]) {
      scratch[key] = new Float32Array(vertices);
    }
    scratch.ok = new Uint8Array(vertices);
  }
  if (scratch.depth.length < faces) {
    scratch.corner = new Float32Array(faces * 6);
    scratch.depth = new Float32Array(faces);
    scratch.tone = new Uint32Array(faces);
    scratch.order = new Uint32Array(faces);
  }
}

/** One directional light on one surface, as a quantised `rgb()` key.
 *
 * ## What this is
 *
 * The terms a metal/rough workflow is made of: a Lambert diffuse, and a
 * specular lobe of GGX times Smith times Schlick over the usual denominator.
 * Metal takes its reflection colour from the base and loses its diffuse, which
 * is what makes metal look like metal rather than like grey plastic.
 *
 * ## What it is not
 *
 * One light and no world. There is no environment to reflect, so the ambient
 * term below is a flat stand-in for one — a real renderer integrates an image
 * of the surroundings and that is most of what makes a smooth metal readable.
 * There are no shadows, nothing bounces, and none of the light that scatters
 * back out of a rough surface after several hits is accounted for.
 *
 * And it is *per face*: everything here is computed once for a whole triangle,
 * because a 2D canvas fills flat polygons. A sphere shows its facets. All
 * three of those are the same sentence with the same ending, which is WebGL.
 */
function litTone(base, metal, rough, nx, ny, nz, vx, vy, vz, key) {
  const ndl = nx * key[0] + ny * key[1] + nz * key[2];
  const ndv = nx * vx + ny * vy + nz * vz;

  let spec = 0;
  let fresnel = 0;
  if (ndl > 0 && ndv > 0) {
    // Halfway between the light and the eye.
    let hx = key[0] + vx;
    let hy = key[1] + vy;
    let hz = key[2] + vz;
    const len = Math.hypot(hx, hy, hz) || 1;
    hx /= len;
    hy /= len;
    hz /= len;
    const ndh = Math.max(0, nx * hx + ny * hy + nz * hz);
    const vdh = Math.max(0, vx * hx + vy * hy + vz * hz);

    // GGX: how much of the surface is tilted to catch this reflection.
    const a = rough * rough;
    const a2 = a * a;
    const d = ndh * ndh * (a2 - 1) + 1;
    const dist = a2 / (Math.PI * d * d);
    // Smith, in the height-correlated form, which already carries the 4·n·l·n·v
    // the specular would otherwise be divided by.
    const k = a / 2;
    const vis = 0.5 / (ndl * (ndv * (1 - k) + k) + ndv * (ndl * (1 - k) + k) + 1e-6);
    fresnel = Math.pow(1 - vdh, 5);
    spec = dist * vis;
  }

  const out = [0, 0, 0];
  for (let c = 0; c < 3; c++) {
    // Dielectrics reflect about 4% head-on and take their colour from within;
    // metals reflect their own colour and have nothing within.
    const f0 = 0.04 * (1 - metal) + base[c] * metal;
    const f = f0 + (1 - f0) * fresnel;
    const diffuse = (base[c] * (1 - metal) * (1 - f)) / Math.PI;
    const lit = (diffuse + spec * f) * Math.max(0, ndl) * LIGHT;
    // The stand-in for an environment, in two parts, because a metal and a
    // plastic take different things from a room. Without it a face turned away
    // is black, which is true of a surface in a void and of nothing else — and
    // a metal is *only* its reflections, so one with no environment renders
    // almost black however brightly it is lit. A flat grey room is a poor
    // environment and an enormously better one than none.
    const ambient = base[c] * (1 - metal) * AMBIENT + f0 * ROOM;
    // Reinhard, so a bright highlight rolls off instead of clipping to a flat
    // white disc — the one place where not doing this is obvious.
    const value = (lit + ambient) / (1 + lit + ambient);
    out[c] = Math.min(31, Math.max(0, Math.round(Math.sqrt(value) * 31)));
  }
  return (out[0] << 10) | (out[1] << 5) | out[2];
}

/** How many triangles may go into one path before it is filled. Measured. */
const BATCH = 16;

/** How hard the key light is, and how much of the missing world stands in.
 *
 * `ROOM` is the reflected part: what a mirror pointed at nothing in particular
 * would return. It is why a metal here is legible rather than a silhouette. */
const LIGHT = 3.2;
const AMBIENT = 0.22;
const ROOM = 0.5;

/** A CSS colour as [r, g, b].
 *
 * Through a canvas, which normalises anything the browser accepts — a hex, an
 * `rgb()`, a `color-mix()` — into something worth parsing. Writing a parser
 * for CSS colour syntax to read our own tokens would be a small mistake that
 * grows. */
const swatch = document.createElement("canvas").getContext("2d");
function rgb(css, fallback) {
  try {
    swatch.fillStyle = "#000";
    swatch.fillStyle = css;
    const said = swatch.fillStyle;
    if (said.startsWith("#") && said.length === 7) {
      return [
        parseInt(said.slice(1, 3), 16),
        parseInt(said.slice(3, 5), 16),
        parseInt(said.slice(5, 7), 16),
      ];
    }
    const nums = (said.match(/[\d.]+/g) ?? []).map(Number);
    if (nums.length < 3) return fallback;
    // Two shapes come back from that normalisation and they are on different
    // scales. `rgb(232, 237, 251)` counts to 255; a token written with
    // `color-mix` comes back as `color(srgb 0.91 0.929 0.984 / 0.78)`, which
    // counts to 1 — and reading those floats as bytes gives a colour that is
    // black to three decimal places. Which is exactly what it did.
    const unit = said.startsWith("color(");
    return nums.slice(0, 3).map((n) => (unit ? n * 255 : n));
  } catch {
    /* an unreadable token is a theme problem, not a reason to stop drawing */
  }
  return fallback;
}

/** Where the key light is, in world space.
 *
 * Fixed, and not any light in the scene. The scene's lights have no units, no
 * falloff and nothing casting shadows behind them, so lighting by them would
 * be a picture claiming to be a render. This is a shading *cue*: it says which
 * way a face points, and nothing more. */
const KEY = (() => {
  const v = [-0.4, 0.82, 0.55];
  const len = Math.hypot(...v);
  return v.map((n) => n / len);
})();

/** Where the eye is, from the orbit. */
function eye() {
  const { yaw, pitch, dist, target } = camera;
  return [
    target[0] + dist * Math.cos(pitch) * Math.sin(yaw),
    target[1] + dist * Math.sin(pitch),
    target[2] + dist * Math.cos(pitch) * Math.cos(yaw),
  ];
}

/** World transforms for every node, by index.
 *
 * The list arrives parents-before-children — `Scene::objects` walks it depth
 * first — so one pass is enough and a child never needs a parent that has not
 * been computed yet. */
function worlds(nodes) {
  const out = [];
  const byId = new Map();
  for (const [i, node] of nodes.entries()) {
    const local = compose(node.position, node.rotation, node.scale);
    const parent = byId.get(node.parent);
    const world = parent === undefined ? local : multiply(out[parent], local);
    out.push(world);
    byId.set(node.id, i);
  }
  return out;
}

let lastNodes = [];
let lastSelected = -1;
let canvas = null;
let onPick = () => {};
/* Told after every frame, not only when an event arrives.
 *
 * A mesh usually lands while this pane is hidden — the app starts in Codex —
 * and a hidden canvas has no size, so the draw returns without drawing and the
 * count stays at whatever it was. Reporting from the draw itself is the only
 * place that cannot be stale, and it also means the number moves while you
 * orbit, which is when it is worth watching. */
let onDraw = () => {};

/** Draw the scene. Safe to call with nothing — an empty viewport is a real
 *  state and should show the grid rather than a blank rectangle. */
export function drawViewport(nodes = lastNodes, selected = lastSelected) {
  lastNodes = nodes ?? [];
  lastSelected = selected ?? -1;
  if (!canvas) return;

  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  if (!width || !height) return;
  // Sized in device pixels and scaled back, or every line is soft on a display
  // that is not exactly 1x.
  if (canvas.width !== Math.round(width * ratio)) {
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
  }

  const ctx = canvas.getContext("2d");
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);

  const view = lookAt(eye(), camera.target);
  const focal = height * 0.5 / Math.tan(FOV / 2);
  const project = (world) => {
    const p = apply(view, world);
    // Behind the eye: no honest place to put it on screen.
    if (p[2] > -0.01) return null;
    return [width / 2 + (focal * p[0]) / -p[2], height / 2 - (focal * p[1]) / -p[2], -p[2]];
  };

  drawGrid(ctx, project);

  const style = getComputedStyle(document.documentElement);
  const accent = style.getPropertyValue("--accent").trim() || "#7aa2f7";
  const ink = style.getPropertyValue("--ink-3").trim() || "#7f8497";

  const matrices = worlds(lastNodes);
  lastDrawn = drawSolids(ctx, lastNodes, matrices, view, focal, width, height, accent);

  // Proxies last, over the solids: a stand-in for a thing that has no surface
  // of its own — or whose surface is not here — should not be hidden behind
  // one that has.
  for (const [i, node] of lastNodes.entries()) {
    const lines = proxyFor(node);
    if (!lines) continue;
    const chosen = i === lastSelected;
    ctx.strokeStyle = chosen ? accent : ink;
    ctx.lineWidth = chosen ? 1.8 : 1;
    ctx.globalAlpha = chosen ? 1 : 0.75;
    ctx.beginPath();
    for (const [from, to] of lines) {
      const a = project(apply(matrices[i], from));
      const b = project(apply(matrices[i], to));
      if (!a || !b) continue;
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
    }
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  onDraw(lastDrawn, lastNodes.length);
}

/** Every mesh triangle in the scene, sorted once and filled. Answers how many.
 *
 * One list across all objects rather than one per object. Sorting per object
 * draws each of them correctly and still stacks two interleaved objects the
 * wrong way round — a lamp inside a room would be painted over by the wall
 * behind it — which is precisely the question somebody opens a viewport to
 * answer. */
function drawSolids(ctx, nodes, matrices, view, focal, width, height, accent) {
  let faces = 0;
  let widest = 0;
  for (const node of nodes) {
    const mesh = node.geometry ? geometry.get(node.geometry) : null;
    if (!mesh?.positions) continue;
    faces += mesh.triangles.length / 3;
    widest = Math.max(widest, mesh.positions.length / 3);
  }
  if (!faces) return 0;
  room(widest, faces);

  // What the selected object is blended toward. Part way rather than all the
  // way: an object washed flat in accent hides the very material somebody
  // selected it to look at.
  const chosen = rgb(accent, [122, 162, 247]).map((c) => c / 255);

  // The key light, rotated into view space. The view's rotation is orthonormal,
  // so a normal worked out from view-space corners and dotted with the light
  // turned the same way gives the number world space would — and saves
  // transforming every vertex twice to get it.
  const key = [
    view[0] * KEY[0] + view[4] * KEY[1] + view[8] * KEY[2],
    view[1] * KEY[0] + view[5] * KEY[1] + view[9] * KEY[2],
    view[2] * KEY[0] + view[6] * KEY[1] + view[10] * KEY[2],
  ];

  const { sx, sy, ex, ey, ez, ok, corner, depth, tone } = scratch;
  const halfW = width / 2;
  const halfH = height / 2;
  // One reused triple for the colour going into the lighting, so the loop
  // allocates nothing per triangle.
  const shade = [0, 0, 0];
  let n = 0;

  for (let i = 0; i < nodes.length; i++) {
    const mesh = nodes[i].geometry ? geometry.get(nodes[i].geometry) : null;
    if (!mesh?.positions) continue;
    // What the material makes of every triangle. Worked out once per mesh and
    // material and kept: it depends on neither the camera nor the light.
    const surf = surfaceFor(nodes[i], mesh);
    const mv = multiply(view, matrices[i]);
    const points = mesh.positions;
    const vertices = points.length / 3;

    // Every vertex projected once. The indexed format is what makes that
    // possible: a corner shared by six triangles is transformed once rather
    // than six times. The matrix arithmetic is written out rather than calling
    // `apply`, which returns a fresh array — a hundred thousand of those a
    // frame is the garbage collection that shows up as a stutter.
    for (let v = 0; v < vertices; v++) {
      const at = v * 3;
      const x = points[at];
      const y = points[at + 1];
      const z = points[at + 2];
      const px = mv[0] * x + mv[4] * y + mv[8] * z + mv[12];
      const py = mv[1] * x + mv[5] * y + mv[9] * z + mv[13];
      const pz = mv[2] * x + mv[6] * y + mv[10] * z + mv[14];
      if (pz > -0.01) {
        ok[v] = 0;
        continue;
      }
      ok[v] = 1;
      ex[v] = px;
      ey[v] = py;
      ez[v] = pz;
      sx[v] = halfW + (focal * px) / -pz;
      sy[v] = halfH - (focal * py) / -pz;
    }

    // Back faces are thrown away only for a solid. On a sheet — a wall, a
    // plane, a piece of cloth — there is nothing behind the surface to hide
    // them, and culling would make it vanish the moment you orbited past its
    // edge. `closed` is what the loader worked out about which this is.
    const cull = mesh.closed;
    const mine = i === lastSelected ? 1 : 0;
    const tris = mesh.triangles;

    for (let t = 0; t < tris.length; t += 3) {
      const a = tris[t];
      const b = tris[t + 1];
      const c = tris[t + 2];
      if (!ok[a] || !ok[b] || !ok[c]) continue;

      const ax = sx[a];
      const ay = sy[a];
      const abx = sx[b] - ax;
      const aby = sy[b] - ay;
      const acx = sx[c] - ax;
      const acy = sy[c] - ay;
      // Signed area on screen, which is where perspective has already been
      // applied — so this is the exact facing test, not an approximation.
      //
      // A face pointing at the eye has *negative* area here, not positive.
      // Screen y runs downward — `sy` is `halfH - …` — and that mirror flips
      // the sign of a two-dimensional cross product. Getting it backwards
      // culls the front of every solid and draws its inside, which on a convex
      // shape is invisible: the silhouette is identical and only the shading
      // is wrong. It took comparing against `normal · (eye - point)` to see.
      const area = abx * acy - aby * acx;
      if (area === 0 || (cull && area >= 0)) continue;

      // The face's normal, in view space.
      const ux = ex[b] - ex[a];
      const uy = ey[b] - ey[a];
      const uz = ez[b] - ez[a];
      const wx = ex[c] - ex[a];
      const wy = ey[c] - ey[a];
      const wz = ez[c] - ez[a];
      let nx = uy * wz - uz * wy;
      let ny = uz * wx - ux * wz;
      let nz = ux * wy - uy * wx;
      const len = Math.hypot(nx, ny, nz) || 1;
      nx /= len;
      ny /= len;
      nz /= len;
      // Turned to face the eye. On a two-sided surface the far side is lit as
      // if it were the near one, which is what looking at the back of a sheet
      // should look like — and on a solid this never fires, because the only
      // faces left are already pointing this way.
      if (nz < 0) {
        nx = -nx;
        ny = -ny;
        nz = -nz;
      }
      // Toward the eye, which sits at the origin of view space.
      const mx = (ex[a] + ex[b] + ex[c]) / 3;
      const my = (ey[a] + ey[b] + ey[c]) / 3;
      const mz = (ez[a] + ez[b] + ez[c]) / 3;
      const away = Math.hypot(mx, my, mz) || 1;

      const face = t / 3;
      shade[0] = surf.base[face * 3];
      shade[1] = surf.base[face * 3 + 1];
      shade[2] = surf.base[face * 3 + 2];
      if (mine) {
        for (let c = 0; c < 3; c++) shade[c] += (chosen[c] - shade[c]) * 0.45;
      }
      tone[n] = litTone(
        shade,
        surf.metal[face],
        surf.rough[face],
        nx, ny, nz,
        -mx / away, -my / away, -mz / away,
        key,
      );
      depth[n] = mz;
      const k = n * 6;
      corner[k] = ax;
      corner[k + 1] = ay;
      corner[k + 2] = sx[b];
      corner[k + 3] = sy[b];
      corner[k + 4] = sx[c];
      corner[k + 5] = sy[c];
      n++;
    }
  }

  // Furthest first. View-space z is negative in front of the eye, so ascending
  // z is back to front.
  const order = scratch.order.subarray(0, n);
  for (let k = 0; k < n; k++) order[k] = k;
  order.sort((p, q) => depth[p] - depth[q]);

  // Batched by colour, and flushed every `BATCH` triangles whether the colour
  // changed or not.
  //
  // The cap is not tidiness. Filling one path costs the rasteriser roughly the
  // square of how many sub-paths are in it — thirty thousand triangles in a
  // single `fill()` measured 1,317 ms, the same triangles in batches of
  // sixteen measured 2.8 ms, and one at a time 8.8 ms. Sixteen is the bottom
  // of that curve on this machine. Batching without a cap was making a scene
  // slower the more of it shared a colour, which is the opposite of what an
  // optimisation is for.
  let current = "";
  let held = 0;
  for (let q = 0; q < n; q++) {
    const f = order[q];
    const want = toneOf(tone[f]);
    if (want !== current || held >= BATCH) {
      if (current) ctx.fill();
      ctx.fillStyle = want;
      ctx.beginPath();
      current = want;
      held = 0;
    }
    const k = f * 6;
    ctx.moveTo(corner[k], corner[k + 1]);
    ctx.lineTo(corner[k + 2], corner[k + 3]);
    ctx.lineTo(corner[k + 4], corner[k + 5]);
    ctx.closePath();
    held++;
  }
  if (current) ctx.fill();
  return n;
}

/** The ground plane and the axes through the origin. */
function drawGrid(ctx, project) {
  const style = getComputedStyle(document.documentElement);
  const faint = style.getPropertyValue("--edge-hi").trim() || "rgba(255,255,255,.13)";
  const span = 10;

  ctx.strokeStyle = faint;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let n = -span; n <= span; n++) {
    for (const line of [
      [[n, 0, -span], [n, 0, span]],
      [[-span, 0, n], [span, 0, n]],
    ]) {
      const a = project(line[0]);
      const b = project(line[1]);
      if (!a || !b) continue;
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
    }
  }
  ctx.stroke();

  // X red, Y green, Z blue — the convention every 3D tool shares, so it needs
  // no legend.
  const axes = [
    [[0, 0, 0], [span, 0, 0], "--err"],
    [[0, 0, 0], [0, span, 0], "--ok"],
    [[0, 0, 0], [0, 0, span], "--info"],
  ];
  ctx.lineWidth = 1.4;
  for (const [from, to, token] of axes) {
    const a = project(from);
    const b = project(to);
    if (!a || !b) continue;
    ctx.strokeStyle = style.getPropertyValue(token).trim() || "#888";
    ctx.beginPath();
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0], b[1]);
    ctx.stroke();
  }
}

/** How much room a node takes up in its own space: min xyz then max xyz.
 *
 * The mesh's real box when its geometry is here, and the proxy's own reach
 * otherwise. One function, so framing and picking cannot disagree about how
 * big a thing is — which they did when both worked from the origin alone and a
 * ten-metre building counted for as much as an empty.
 */
function extentOf(node) {
  const mesh = node.geometry ? geometry.get(node.geometry) : null;
  if (mesh?.bounds && mesh.bounds[3] > mesh.bounds[0]) return mesh.bounds;
  switch (node.kind) {
    case "mesh":
      return [-HALF, -HALF, -HALF, HALF, HALF, HALF];
    case "camera":
      return [-0.3, -0.3, -0.6, 0.3, 0.3, 0];
    default:
      return [-0.35, -0.35, -0.35, 0.35, 0.35, 0.35];
  }
}

/** The eight corners of a node's box, in world space. */
function corners(node, world) {
  const box = extentOf(node);
  const out = [];
  for (const x of [box[0], box[3]]) {
    for (const y of [box[1], box[4]]) {
      for (const z of [box[2], box[5]]) out.push(apply(world, [x, y, z]));
    }
  }
  return out;
}

/** Which node is under a point, or -1.
 *
 * By the rectangle its box covers on screen, not by testing the surface. Two
 * reasons it is not an exact hit test against the triangles: a proxy is mostly
 * holes, so clicking the middle of a wireframe box has to select it; and a
 * mesh with a hundred thousand triangles would be a hundred thousand
 * ray-triangle tests per click.
 *
 * What it costs is that clicking a gap inside a thing's bounding box still
 * selects it — the inside of a doorway picks the wall. When two boxes cover the
 * point, the nearer one wins, which is also what a modelling tool does.
 */
function pick(x, y) {
  if (!canvas) return -1;
  const view = lookAt(eye(), camera.target);
  const height = canvas.clientHeight;
  const width = canvas.clientWidth;
  const focal = height * 0.5 / Math.tan(FOV / 2);
  const matrices = worlds(lastNodes);

  let best = -1;
  let bestZ = Infinity;
  for (const [i, node] of lastNodes.entries()) {
    let lo = [Infinity, Infinity];
    let hi = [-Infinity, -Infinity];
    let near = Infinity;
    let seen = 0;
    for (const point of corners(node, matrices[i])) {
      const p = apply(view, point);
      if (p[2] > -0.01) continue;
      seen++;
      const sx = width / 2 + (focal * p[0]) / -p[2];
      const sy = height / 2 - (focal * p[1]) / -p[2];
      lo = [Math.min(lo[0], sx), Math.min(lo[1], sy)];
      hi = [Math.max(hi[0], sx), Math.max(hi[1], sy)];
      near = Math.min(near, -p[2]);
    }
    if (!seen) continue;
    // A grown edge, so a thing drawn edge-on — a plane seen from the side, an
    // empty — is still something you can hit.
    const pad = 6;
    const inside =
      x >= lo[0] - pad && x <= hi[0] + pad && y >= lo[1] - pad && y <= hi[1] + pad;
    if (!inside) continue;
    if (near < bestZ) {
      bestZ = near;
      best = i;
    }
  }
  return best;
}

/** Wire the canvas up. `picked` is called with a node index, or -1. */
export function startViewport(element, picked, drew) {
  canvas = element;
  onPick = picked ?? (() => {});
  onDraw = drew ?? (() => {});
  if (!canvas) return;

  let dragging = null;
  let moved = 0;

  canvas.addEventListener("pointerdown", (e) => {
    dragging = { x: e.clientX, y: e.clientY, pan: e.shiftKey || e.button === 1 };
    moved = 0;
    // Capture is a convenience — it keeps a drag alive past the edge of the
    // canvas — and it throws for a pointer id the browser does not know about.
    // Losing it must not cost the click.
    try {
      canvas.setPointerCapture(e.pointerId);
    } catch {
      /* not a real pointer; the drag still works inside the canvas */
    }
  });

  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const dx = e.clientX - dragging.x;
    const dy = e.clientY - dragging.y;
    dragging.x = e.clientX;
    dragging.y = e.clientY;
    moved += Math.abs(dx) + Math.abs(dy);

    if (dragging.pan) {
      // Pan along the camera's own axes, scaled by distance, so the ground
      // moves with the pointer at any zoom.
      const view = lookAt(eye(), camera.target);
      const right = [view[0], view[4], view[8]];
      const up = [view[1], view[5], view[9]];
      const k = camera.dist * 0.0015;
      for (let axis = 0; axis < 3; axis++) {
        camera.target[axis] += -dx * k * right[axis] + dy * k * up[axis];
      }
    } else {
      camera.yaw -= dx * 0.008;
      // Clamped short of straight up: at the pole the side vector is
      // undefined and the picture flips over.
      camera.pitch = Math.max(-1.5, Math.min(1.5, camera.pitch + dy * 0.008));
    }
    drawViewport();
  });

  canvas.addEventListener("pointerup", (e) => {
    const wasDrag = moved > 4;
    dragging = null;
    canvas.releasePointerCapture?.(e.pointerId);
    // A drag that ends over an object is a drag, not a click on it.
    if (wasDrag) return;
    const box = canvas.getBoundingClientRect();
    onPick(pick(e.clientX - box.left, e.clientY - box.top));
  });

  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      // Multiplicative, so a step feels the same close up and far away, and
      // bounded so the scene cannot be lost by spinning the wheel.
      camera.dist = Math.max(0.5, Math.min(400, camera.dist * (1 + Math.sign(e.deltaY) * 0.12)));
      drawViewport();
    },
    { passive: false },
  );

  // The pane resizes with the window and with the gutters either side of it.
  new ResizeObserver(() => drawViewport()).observe(canvas);
}

/** Put the camera where the whole scene is visible. */
export function frameAll(nodes) {
  const list = nodes ?? [];
  const matrices = worlds(list);
  if (!matrices.length) return;
  let lo = [Infinity, Infinity, Infinity];
  let hi = [-Infinity, -Infinity, -Infinity];
  // Over every corner of every box, not over the origins. A building modelled
  // around the world origin has its origin in the middle of the scene and its
  // walls a hundred metres out; framing by origins would put the camera three
  // metres from the middle of it.
  for (const [i, node] of list.entries()) {
    for (const p of corners(node, matrices[i])) {
      for (let axis = 0; axis < 3; axis++) {
        lo[axis] = Math.min(lo[axis], p[axis]);
        hi[axis] = Math.max(hi[axis], p[axis]);
      }
    }
  }
  camera.target = lo.map((v, axis) => (v + hi[axis]) / 2);
  // Half the longest side is the radius of a sphere that holds the lot; the
  // extents are already the real ones, so nothing is added for the proxies.
  const radius = Math.max(...hi.map((v, axis) => v - lo[axis]), 0.5) / 2;
  // What distance puts a sphere of that radius inside the view: it subtends
  // the whole angle at exactly `radius / sin(fov/2)`. Derived rather than
  // tuned, so changing the lens reframes correctly instead of cropping.
  camera.dist = radius / Math.sin(FOV / 2);
}
