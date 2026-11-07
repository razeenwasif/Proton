/* CAD's 3D view: a part's bodies as the core tessellated them, their
 * edges, and its sketches as lines on their planes, drawn with three.js
 * and turned with its OrbitControls (docs/CAD.md, K3).
 *
 * Z is up, as the part's XY is its top plane. Each body's points come with
 * the exact normal of the face they are on, so a cylinder is smooth and an
 * edge is sharp. A face is found under the pointer by the triangle hit and
 * the face that triangle is from: named in the status line, and chosen on
 * a click — a flat one can be sketched on.
 *
 * three.js is imported the first time a part is shown (lab3d.js shares
 * it); nothing is drawn but on a change. Without WebGL it says so. */

import { three } from "./lab3d.js";

/** @typedef {import("./bindings/CadPartView").CadPartView} PartView */
/** @typedef {import("./bindings/CadBody").CadBody} Body */
/** @typedef {import("./bindings/CadSketchView").CadSketchView} SketchView */

const BODY = 0x8b93a8, HOVER = 0xa3b4e8, PICKED = 0x7aa2f7, EDGE = 0x1b1d26, SKETCH = 0xe0af68, PICKED_SKETCH = 0xff9e64;

/** A sketch's shapes as runs of points in the world: lines, circles and
 *  arcs cut finely enough to look round. @param {SketchView} s @returns {number[][]} */
export function sketchRuns(s) {
  const f = s.frame, k = s.scale;
  const world = (/** @type {number} */ x, /** @type {number} */ y) => [f[0] + (f[3] * x + f[6] * y) * k, f[1] + (f[4] * x + f[7] * y) * k, f[2] + (f[5] * x + f[8] * y) * k];
  const runs = [];
  for (const x of s.shapes) {
    if (x.kind === "line") runs.push([...world(...x.pts[0]), ...world(...x.pts[1])]);
    else if (x.kind === "circle" || x.kind === "arc") {
      const [c] = x.pts, r = x.radius ?? 0;
      let a0 = 0, a1 = 2 * Math.PI;
      if (x.kind === "arc") {
        a0 = Math.atan2(x.pts[1][1] - c[1], x.pts[1][0] - c[0]);
        a1 = Math.atan2(x.pts[2][1] - c[1], x.pts[2][0] - c[0]);
        while (a1 <= a0) a1 += 2 * Math.PI;
      }
      const n = Math.max(8, Math.ceil(((a1 - a0) / (2 * Math.PI)) * 96));
      const run = [];
      for (let i = 0; i <= n; i++) {
        const t = a0 + ((a1 - a0) * i) / n;
        run.push(...world(c[0] + r * Math.cos(t), c[1] + r * Math.sin(t)));
      }
      runs.push(run);
    } else if (x.kind === "spline") runs.push(x.pts.flatMap((p) => world(p[0], p[1])));
  }
  return runs;
}

/** @param {HTMLCanvasElement} canvas @param {{hover: (name: string | null) => void, pick: (body: string | null, face: number | null, point: number[] | null) => void}} host */
export async function mountView(canvas, host) {
  const [THREE, { OrbitControls }] = await three();
  /** @type {import("./vendor/three/three.module.js").WebGLRenderer} */
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: "low-power" });
  } catch {
    return null;
  }
  renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1));
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(35, 1, 0.1, 10000);
  camera.up.set(0, 0, 1);
  camera.position.set(160, -220, 140);
  scene.add(new THREE.HemisphereLight(0xe8ecff, 0x30323c, 1.4));
  const key = new THREE.DirectionalLight(0xffffff, 1.6);
  scene.add(key);
  const controls = new OrbitControls(camera, canvas);
  controls.enableDamping = false;
  controls.screenSpacePanning = true;
  const world = new THREE.Group();
  scene.add(world);
  /** @type {any[]} */
  let kept = [];
  const keep = (/** @type {any} */ x) => (kept.push(x), x);
  /** Each body's mesh, its colour per point, its triangles' faces, and
   *  the points of each face. @type {{name: string, mesh: any, colors: any, faceOf: number[], faces: Body["faces"], facePoints: number[][]}[]} */
  let bodies = [];
  let grid = /** @type {any} */ (null);
  let hovered = /** @type {[number, number] | null} */ (null);
  let picked = /** @type {[number, number] | null} */ (null);
  let frameAsked = 0;
  const draw = () => {
    if (frameAsked) return;
    frameAsked = requestAnimationFrame(() => {
      frameAsked = 0;
      key.position.copy(camera.position).add(new THREE.Vector3(40, -30, 80));
      renderer.render(scene, camera);
    });
  };
  controls.addEventListener("change", draw);

  function resize() {
    const w = canvas.clientWidth || 1, h = canvas.clientHeight || 1;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    draw();
  }

  function clear() {
    for (const x of kept) x.dispose?.();
    kept = [];
    world.clear();
    bodies = [];
  }

  /** Colour a face's points. @param {[number, number] | null} at @param {number} color */
  function paint(at, color) {
    if (!at) return;
    const b = bodies[at[0]];
    if (!b) return;
    const c = new THREE.Color(color);
    for (const i of b.facePoints[at[1]] ?? []) b.colors.setXYZ(i, c.r, c.g, c.b);
    b.colors.needsUpdate = true;
  }

  function repaint() {
    for (let i = 0; i < bodies.length; i++) for (let f = 0; f < bodies[i].faces.length; f++) paint([i, f], BODY);
    paint(hovered, HOVER);
    paint(picked, PICKED);
    draw();
  }

  /** @param {PartView} part @param {{sketch?: number | null}} [opts] */
  function show(part, opts = {}) {
    clear();
    for (const b of part.bodies) {
      const g = keep(new THREE.BufferGeometry());
      g.setAttribute("position", new THREE.Float32BufferAttribute(b.positions, 3));
      g.setAttribute("normal", new THREE.Float32BufferAttribute(b.normals, 3));
      const colors = new THREE.Float32BufferAttribute(new Float32Array(b.positions.length), 3);
      g.setAttribute("color", colors);
      g.setIndex(b.indices);
      const m = keep(new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.55, metalness: 0.1, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1, side: THREE.DoubleSide }));
      const mesh = new THREE.Mesh(g, m);
      world.add(mesh);
      // Which points each face has: what is coloured when it is hovered.
      const facePoints = b.faces.map(() => /** @type {Set<number>} */ (new Set()));
      b.faceOf.forEach((f, t) => {
        for (let k = 0; k < 3; k++) facePoints[f]?.add(b.indices[3 * t + k]);
      });
      bodies.push({ name: b.name, mesh, colors, faceOf: b.faceOf, faces: b.faces, facePoints: facePoints.map((s) => [...s]) });
      const lines = [];
      for (const e of b.edges) for (let i = 0; i + 5 < e.length; i += 3) lines.push(e[i], e[i + 1], e[i + 2], e[i + 3], e[i + 4], e[i + 5]);
      const eg = keep(new THREE.BufferGeometry());
      eg.setAttribute("position", new THREE.Float32BufferAttribute(lines, 3));
      world.add(new THREE.LineSegments(eg, keep(new THREE.LineBasicMaterial({ color: EDGE }))));
    }
    for (const s of part.sketches) {
      const color = s.index === opts.sketch ? PICKED_SKETCH : SKETCH;
      const mat = keep(new THREE.LineBasicMaterial({ color, transparent: true, opacity: part.bodies.length ? 0.75 : 1 }));
      for (const run of sketchRuns(s)) {
        const g = keep(new THREE.BufferGeometry());
        g.setAttribute("position", new THREE.Float32BufferAttribute(run, 3));
        world.add(new THREE.Line(g, mat));
      }
    }
    // The ground: a grid on XY, as big as the part.
    const [x0, y0, , x1, y1] = part.bounds;
    const span = Math.max(20, x1 - x0, y1 - y0) * 1.6;
    const step = 10 ** Math.floor(Math.log10(span / 10));
    const n = Math.ceil(span / step);
    if (grid) {
      grid.geometry.dispose();
      grid.material.dispose();
      scene.remove(grid);
    }
    grid = new THREE.GridHelper(n * step, n, 0x3a3d4a, 0x2a2c36);
    grid.rotation.x = Math.PI / 2;
    grid.position.set(Math.round((x0 + x1) / 2 / step) * step, Math.round((y0 + y1) / 2 / step) * step, 0);
    scene.add(grid);
    if (picked && !bodies[picked[0]]) picked = null;
    hovered = null;
    repaint();
  }

  /** Look at all of it. @param {number[]} b */
  function fit(b) {
    // At the canvas's size now: a fit made before it had one looks wrong.
    resize();
    const center = new THREE.Vector3((b[0] + b[3]) / 2, (b[1] + b[4]) / 2, (b[2] + b[5]) / 2);
    const radius = Math.max(5, Math.hypot(b[3] - b[0], b[4] - b[1], b[5] - b[2]) / 2);
    const dir = camera.position.clone().sub(controls.target).normalize();
    if (!Number.isFinite(dir.x) || dir.length() < 0.5) dir.set(0.5, -0.7, 0.5).normalize();
    // The narrower of the two angles the view takes in.
    const half = (camera.fov * Math.PI) / 360;
    const narrow = Math.min(half, Math.atan(Math.tan(half) * camera.aspect));
    const d = (radius / Math.sin(narrow)) * 1.1;
    controls.target.copy(center);
    camera.position.copy(center).add(dir.multiplyScalar(d));
    camera.near = d / 200;
    camera.far = d * 50;
    camera.updateProjectionMatrix();
    controls.update();
    draw();
  }

  const ray = new THREE.Raycaster();
  /** Where the last ray hit, in the world. @type {number[] | null} */
  let hitPoint = null;
  /** The body and face under a point of the canvas. @param {MouseEvent} e @returns {[number, number] | null} */
  function under(e) {
    const r = canvas.getBoundingClientRect();
    const at = new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1);
    ray.setFromCamera(at, camera);
    const hits = ray.intersectObjects(bodies.map((b) => b.mesh), false);
    const hit = hits[0];
    hitPoint = hit ? [hit.point.x, hit.point.y, hit.point.z] : null;
    if (!hit || hit.faceIndex == null) return null;
    const i = bodies.findIndex((b) => b.mesh === hit.object);
    return [i, bodies[i].faceOf[hit.faceIndex]];
  }

  let moveFrame = 0, downAt = { x: 0, y: 0 };
  canvas.addEventListener("pointermove", (e) => {
    if (moveFrame || e.buttons) return;
    moveFrame = requestAnimationFrame(() => {
      moveFrame = 0;
      const at = under(e);
      if (JSON.stringify(at) === JSON.stringify(hovered)) return;
      hovered = at;
      host.hover(at ? `${bodies[at[0]].faces[at[1]].name}` : null);
      repaint();
    });
  });
  canvas.addEventListener("pointerleave", () => {
    hovered = null;
    host.hover(null);
    repaint();
  });
  canvas.addEventListener("pointerdown", (e) => (downAt = { x: e.clientX, y: e.clientY }));
  canvas.addEventListener("click", (e) => {
    if (Math.hypot(e.clientX - downAt.x, e.clientY - downAt.y) > 4) return;
    const at = under(e);
    picked = at;
    repaint();
    host.pick(at ? bodies[at[0]].name : null, at ? at[1] : null, at ? hitPoint : null);
  });

  const watch = new ResizeObserver(resize);
  watch.observe(canvas);
  resize();

  return {
    show,
    fit,
    resize,
    /** No face chosen. */
    unpick() {
      picked = null;
      repaint();
    },
    destroy() {
      watch.disconnect();
      clear();
      renderer.dispose();
    },
    /** What a test asks: what is drawn. */
    debug() {
      return { bodies: bodies.map((b) => ({ name: b.name, faces: b.faces.length, triangles: b.faceOf.length })), hovered, picked };
    },
  };
}
