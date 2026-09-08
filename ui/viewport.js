/* Modeling's viewport.
 *
 * ## What this draws, and what it does not
 *
 * `scene.proton.json` holds transforms and kinds. It holds no geometry — there
 * is no mesh loader yet, and nothing in the file says what a mesh's vertices
 * are. So this draws *proxies*: a box where a mesh sits, a frustum for a
 * camera, a burst for a light, an axis cross for an empty. Each one is drawn
 * at the object's real world transform, so the picture answers where things
 * are, how big, which way round, and what is inside what.
 *
 * That is a real answer and a limited one, and saying so is better than
 * drawing a convincing lump and letting somebody believe their mesh looks like
 * that. When geometry arrives in the format, this is where it gets drawn.
 *
 * ## Why a 2D canvas and not WebGL
 *
 * Wireframe proxies need no shaders, no buffers, no material system and no
 * depth buffer — a painter's-algorithm sort over a few hundred line segments
 * is exact enough for lines that do not occlude each other. WebGL would be a
 * few hundred lines of setup to draw the same picture, and this UI has no
 * bundler and no runtime dependencies to spend on a library.
 *
 * The moment there is geometry to push — real meshes, many thousands of
 * triangles, materials, lighting — this becomes the wrong tool and should be
 * replaced rather than extended. It is deliberately small so that is cheap.
 */

/* ── small matrix maths ──────────────────────────────────────────────
 *
 * Column-major 4x4 as a flat 16-array, the same convention WebGL uses, so
 * none of this has to be transposed if it is ever handed to a GPU. */

const DEG = Math.PI / 180;

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

function proxyFor(kind) {
  switch (kind) {
    case "mesh":
      return CUBE;
    case "light":
      return BURST;
    case "camera":
      return FRUSTUM;
    default:
      return CROSS;
  }
}

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
  // 50mm on full frame, which is what the badge in the corner claims.
  const focal = (height * 0.5) / Math.tan(0.5 * 39.6 * DEG);
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
  // Painter's algorithm over whole objects. Wireframes barely occlude, so
  // sorting segments individually would cost more than it shows.
  const order = lastNodes
    .map((_, i) => i)
    .map((i) => ({ i, z: (project(apply(matrices[i], [0, 0, 0])) ?? [0, 0, Infinity])[2] }))
    .sort((a, b) => b.z - a.z);

  for (const { i } of order) {
    const node = lastNodes[i];
    const chosen = i === lastSelected;
    ctx.strokeStyle = chosen ? accent : ink;
    ctx.lineWidth = chosen ? 1.8 : 1;
    ctx.globalAlpha = chosen ? 1 : 0.75;
    ctx.beginPath();
    for (const [from, to] of proxyFor(node.kind)) {
      const a = project(apply(matrices[i], from));
      const b = project(apply(matrices[i], to));
      if (!a || !b) continue;
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
    }
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
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

/** Which node is under a point, or -1.
 *
 * By distance to the object's projected origin rather than by testing the
 * proxy's edges. A wireframe is mostly holes, so an edge test would miss the
 * middle of a box — clicking the centre of a cube has to select the cube. The
 * limit is that two objects at the same place go to whichever is nearer the
 * camera, which is also what a modelling tool does.
 */
function pick(x, y) {
  if (!canvas) return -1;
  const view = lookAt(eye(), camera.target);
  const height = canvas.clientHeight;
  const width = canvas.clientWidth;
  const focal = (height * 0.5) / Math.tan(0.5 * 39.6 * DEG);
  const matrices = worlds(lastNodes);

  let best = -1;
  let bestZ = Infinity;
  for (const [i] of lastNodes.entries()) {
    const p = apply(view, apply(matrices[i], [0, 0, 0]));
    if (p[2] > -0.01) continue;
    const sx = width / 2 + (focal * p[0]) / -p[2];
    const sy = height / 2 - (focal * p[1]) / -p[2];
    if (Math.hypot(sx - x, sy - y) > 22) continue;
    if (-p[2] < bestZ) {
      bestZ = -p[2];
      best = i;
    }
  }
  return best;
}

/** Wire the canvas up. `picked` is called with a node index, or -1. */
export function startViewport(element, picked) {
  canvas = element;
  onPick = picked ?? (() => {});
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
  const matrices = worlds(nodes ?? []);
  if (!matrices.length) return;
  let lo = [Infinity, Infinity, Infinity];
  let hi = [-Infinity, -Infinity, -Infinity];
  for (const m of matrices) {
    const p = apply(m, [0, 0, 0]);
    for (let axis = 0; axis < 3; axis++) {
      lo[axis] = Math.min(lo[axis], p[axis]);
      hi[axis] = Math.max(hi[axis], p[axis]);
    }
  }
  camera.target = lo.map((v, axis) => (v + hi[axis]) / 2);
  const size = Math.max(...hi.map((v, axis) => v - lo[axis]), 1);
  // Room for the proxies themselves, which stick out past their origins.
  camera.dist = size * 2.2 + 4;
}
