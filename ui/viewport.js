/* Modeling's viewport.
 *
 * ## What this is now
 *
 * The camera, the scene, and what the pointer means. The drawing itself is in
 * `gl.js`, on the GPU — this file works out *what* to draw and hands it over.
 *
 * `scene.proton.json` holds transforms and kinds; a node with geometry names
 * the file it is in, `Event::Geometry` brings the triangles, and `Event::Scene`
 * brings its material already compiled into a short program. So an object with
 * a mesh is drawn as its surface, with the material run at every pixel, and
 * everything else is drawn as a *proxy*: a frustum for a camera, a burst for a
 * light, an axis cross for an empty, and a box for a mesh whose geometry has
 * not arrived, could not be read, or is too large to draw.
 *
 * A proxy box is a stand-in and looks like one. That is deliberate: the thing
 * this must never do is draw a convincing lump and let somebody believe their
 * mesh looks like that. The same goes for a material the core could not
 * compile — it draws in a plain grey rather than in a guess.
 *
 * ## What the 2D canvas could not do, and this can
 *
 * The canvas version said for four revisions that it was the wrong tool the
 * moment there was geometry to push, and should be replaced rather than
 * extended. Three of its limits are gone: occlusion is decided per pixel
 * rather than by sorting whole triangles, the material runs per pixel rather
 * than once per triangle, and the budget is no longer set by how fast paths
 * can be filled. What went with it was about four hundred lines — a painter's
 * sort, a scratch-buffer pool, a colour quantiser, a batch-size constant tuned
 * to a rasteriser's quirk, and a software BSDF.
 *
 * ## Normals
 *
 * Worked out in `proton-mesh`, after the modifier stack, and sent with the
 * geometry: smooth across gentle edges and split along sharp ones, so a
 * subdivided mesh shades as round as it is and a cube keeps its corners. A
 * mesh that arrives without them — only the preview's mock sends such a thing
 * — is shaded from each face's own normal instead, which is exact and faceted.
 */

import * as gpu from "./gl.js";

/* ── small matrix maths ──────────────────────────────────────────────
 *
 * Column-major 4x4 as a flat 16-array, the convention WebGL wants — which is
 * why it was written this way in the first version, when nothing needed it to
 * be. */

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

/** A perspective projection, from the same `FOV` everything else uses. */
function perspective(aspect, near, far) {
  const f = 1 / Math.tan(FOV / 2);
  return new Float32Array([
    f / Math.max(aspect, 1e-4), 0, 0, 0,
    0, f, 0, 0,
    0, 0, (far + near) / (near - far), -1,
    0, 0, (2 * far * near) / (near - far), 0,
  ]);
}

/* ── the proxies ─────────────────────────────────────────────────────
 *
 * Each is a list of line segments in local space, in metres: the shapes every
 * modelling tool draws for things that have no surface of their own, and a box
 * for a mesh whose surface is not here. */

const HALF = 0.5;

/** The twelve edges of a box, from `[minx, miny, minz, maxx, maxy, maxz]`. */
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
      // An edge joins corners that differ in exactly one axis.
      const differs = corners[i].filter((v, k) => v !== corners[j][k]).length;
      if (differs === 1) lines.push([corners[i], corners[j]]);
    }
  }
  return lines;
}

const CUBE = boxLines([-HALF, -HALF, -HALF, HALF, HALF, HALF]);

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

/** What to draw in wireframe for a node, or `null` when its surface is drawn.
 *
 * A mesh whose geometry is loaded needs no proxy — the triangles are the
 * answer. A mesh whose geometry is missing, unreadable or too large gets a
 * box, and when the loader read far enough to know the real extent the box is
 * that extent rather than a unit cube standing in for it. A box the true size
 * of the thing is a useful stand-in; one that is always a metre across is a
 * picture of the loader's ignorance. */
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
 * They arrive compiled — a dozen instructions `proton-scene` produced from the
 * graph in the file — and `gl.js` turns those into a fragment shader. What a
 * graph *means* is the core's business; running it is the GPU's. */

const shaders = new Map();

/** Take the compiled materials, as `Event::Scene` sends them. */
export function setShaders(list) {
  let moved = (list?.length ?? 0) !== shaders.size;
  // Replaced, not merged: the list is every material the scene has, so one
  // missing from it was deleted, and keeping it here would go on rendering an
  // object with a material the file no longer contains.
  const current = new Set((list ?? []).map((one) => one.id));
  for (const id of [...shaders.keys()]) {
    if (!current.has(id)) shaders.delete(id);
  }
  for (const one of list ?? []) {
    // Stamped, so a slider that changed nothing does not rebuild a program.
    const stamp = JSON.stringify(one);
    if (shaders.get(one.id)?.stamp !== stamp) moved = true;
    shaders.set(one.id, { ...one, stamp });
  }
  if (moved) drawViewport();
}

/* ── geometry ────────────────────────────────────────────────────────
 *
 * Kept by the path the scene writes, which is what both ends key it by. A mesh
 * arrives once and is reused by every object naming it, so ten chairs sharing
 * `chair.obj` cost one load and one upload. */

const geometry = new Map();

/** Bumped on every delivery, so anything derived from a mesh can tell one
 *  delivery from the next: a key does not move when a modifier's setting
 *  changes, but the mesh under it does. */
let serial = 0;

/** Take one mesh, as `Event::Geometry` sends it. */
export function setGeometry(what) {
  if (!what?.path) return;
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
      // Typed here, once. The buffers go straight to the GPU from these.
      positions: Float32Array.from(what.mesh.positions),
      // Absent only from a sender that does not shade — the preview's mock.
      // The renderer then works each face's normal out for itself.
      normals: what.mesh.normals?.length ? Float32Array.from(what.mesh.normals) : null,
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

/** A CSS colour as `[r, g, b, a]` in 0..1, for the line shader.
 *
 * Through a canvas, which normalises anything the browser accepts — a hex, an
 * `rgb()`, a `color-mix()` — into something worth parsing. Writing a parser for
 * CSS colour syntax to read our own tokens would be a small mistake that grows.
 */
const swatch = document.createElement("canvas").getContext("2d");
function rgba(css, fallback) {
  try {
    swatch.fillStyle = "#000";
    swatch.fillStyle = css;
    const said = swatch.fillStyle;
    if (said.startsWith("#") && said.length === 7) {
      return new Float32Array([
        parseInt(said.slice(1, 3), 16) / 255,
        parseInt(said.slice(3, 5), 16) / 255,
        parseInt(said.slice(5, 7), 16) / 255,
        1,
      ]);
    }
    const nums = (said.match(/[\d.]+/g) ?? []).map(Number);
    if (nums.length < 3) return fallback;
    // Two shapes come back from that normalisation and they are on different
    // scales. `rgb(232, 237, 251)` counts to 255; a token written with
    // `color-mix` comes back as `color(srgb 0.91 0.929 0.984 / 0.78)`, which
    // counts to 1 — and reading those floats as bytes gives a colour that is
    // black to three decimal places. Which is exactly what it once did.
    const unit = said.startsWith("color(");
    const scale = unit ? 1 : 1 / 255;
    return new Float32Array([
      nums[0] * scale,
      nums[1] * scale,
      nums[2] * scale,
      nums.length > 3 ? nums[3] : 1,
    ]);
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

/* ── the camera the viewer drives ───────────────────────────────────── */

const camera = { yaw: 0.7, pitch: 0.5, dist: 12, target: [0, 0, 0] };

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
 * place that cannot be stale. */
let onDraw = () => {};

/** Draw the scene. Safe to call with nothing — an empty viewport is a real
 *  state and should show the grid rather than a blank rectangle. */
export function drawViewport(nodes = lastNodes, selected = lastSelected) {
  lastNodes = nodes ?? [];
  lastSelected = selected ?? -1;
  if (!canvas || !gpu.start(canvas)) return;

  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  if (!width || !height) return;

  const style = getComputedStyle(document.documentElement);
  const matrices = worlds(lastNodes);

  // Near and far from where the camera actually is. A fixed pair would either
  // clip a large scene or spend all the depth buffer's precision on empty
  // space in front of a small one.
  const reach = span(lastNodes, matrices);
  const near = Math.max(0.01, camera.dist * 0.002);
  const far = Math.max(near * 1000, camera.dist * 4 + reach * 4);
  const view = lookAt(eye(), camera.target);

  const count = gpu.frame({
    canvas,
    nodes: lastNodes,
    matrices,
    geometry,
    shaders,
    view: new Float32Array(view),
    proj: perspective(width / height, near, far),
    // The key light, rotated into view space, where the shading is done.
    light: new Float32Array([
      view[0] * KEY[0] + view[4] * KEY[1] + view[8] * KEY[2],
      view[1] * KEY[0] + view[5] * KEY[1] + view[9] * KEY[2],
      view[2] * KEY[0] + view[6] * KEY[1] + view[10] * KEY[2],
    ]),
    selected: lastSelected,
    tint: rgba(
      style.getPropertyValue("--accent").trim(),
      new Float32Array([0.48, 0.64, 0.97, 1]),
    ).slice(0, 3),
    lines: lineBatches(style, matrices),
    ratio: window.devicePixelRatio || 1,
  });
  onDraw(count, lastNodes.length);
}

/** How far the scene reaches from the camera's target, for the far plane. */
function span(nodes, matrices) {
  let out = 1;
  for (const [i, node] of nodes.entries()) {
    for (const point of corners(node, matrices[i])) {
      out = Math.max(
        out,
        Math.hypot(
          point[0] - camera.target[0],
          point[1] - camera.target[1],
          point[2] - camera.target[2],
        ),
      );
    }
  }
  return out;
}

/** The grid, the axes and every proxy, as batches of coloured segments. */
function lineBatches(style, matrices) {
  const ink = rgba(
    style.getPropertyValue("--ink-3").trim(),
    new Float32Array([0.5, 0.52, 0.6, 1]),
  );
  const out = [];
  const SPAN = 10;

  const grid = [];
  for (let n = -SPAN; n <= SPAN; n++) {
    // The axes are drawn separately and in colour, so the grid leaves those
    // two lines out rather than drawing over them.
    if (n !== 0) grid.push(n, 0, -SPAN, n, 0, SPAN, -SPAN, 0, n, SPAN, 0, n);
  }
  out.push({
    points: new Float32Array(grid),
    colour: rgba(
      style.getPropertyValue("--edge-hi").trim(),
      new Float32Array([1, 1, 1, 0.13]),
    ),
  });

  // X red, Y green, Z blue — the convention every 3D tool shares, so it needs
  // no legend.
  for (const [axis, token] of [[0, "--err"], [1, "--ok"], [2, "--info"]]) {
    const from = [0, 0, 0];
    const to = [0, 0, 0];
    from[axis] = -SPAN;
    to[axis] = SPAN;
    out.push({
      points: new Float32Array([...from, ...to]),
      colour: rgba(style.getPropertyValue(token).trim(), ink),
    });
  }

  // Two batches, so the selected object's wireframe costs one colour change
  // rather than one per object.
  const plain = [];
  const picked = [];
  for (const [i, node] of lastNodes.entries()) {
    const lines = proxyFor(node);
    if (!lines) continue;
    const into = i === lastSelected ? picked : plain;
    for (const [from, to] of lines) {
      into.push(...apply(matrices[i], from), ...apply(matrices[i], to));
    }
  }
  out.push({ points: new Float32Array(plain), colour: ink });
  out.push({
    points: new Float32Array(picked),
    colour: rgba(
      style.getPropertyValue("--accent").trim(),
      new Float32Array([0.48, 0.64, 0.97, 1]),
    ),
  });
  return out;
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
 * selects it — the inside of a doorway picks the wall. When two boxes cover
 * the point, the nearer one wins, which is also what a modelling tool does.
 *
 * Deliberately still on the processor, now that there is a GPU. Reading a
 * pixel back to ask what is under the pointer stalls the pipeline behind every
 * frame still in flight, and this answers in microseconds from numbers already
 * to hand.
 */
function pick(x, y) {
  if (!canvas) return -1;
  const view = lookAt(eye(), camera.target);
  const height = canvas.clientHeight;
  const width = canvas.clientWidth;
  const focal = (height * 0.5) / Math.tan(FOV / 2);
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

/** Why there is no picture, when there is not. */
export function renderProblem() {
  return canvas && !gpu.start(canvas) ? gpu.problem() : "";
}

/** What is doing the drawing, for the badge that says so. */
export function renderer() {
  return canvas && gpu.start(canvas) ? gpu.adapter() : "";
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
