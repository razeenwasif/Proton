/* The Lab's 3D view: a run's `proton.scene/1` — spheres and bonds, points,
 * lines, arrows and meshes, in frames — drawn with three.js, turned with
 * its OrbitControls, and played a frame at a time.
 *
 * three.js is two megabytes, so it is imported the first time a scene is
 * opened, never at boot. A frame is built the first time it is shown and
 * kept; nothing is drawn but on a change — a turn, a frame, a resize — so a
 * scene left open costs nothing while it is still.
 *
 * Without WebGL it says so; a scene is not worth a second, worse picture. */

/** @typedef {[number, number, number]} Vec3 */
/** @typedef {{kind: string, [key: string]: any}} SceneItem */
/** @typedef {{format: string, title?: string, units?: string | null, frames: SceneItem[][]}} SceneDoc */
/** @typedef {typeof import("./vendor/three/three.module.js")} Three */

const PALETTE = ["#7aa2f7", "#e0af68", "#9ece6a", "#f7768e", "#bb9af7", "#7dcfff", "#ff9e64", "#c0caf5"];

/** Points drawn as small spheres, up to this many in one item. */
const MOST_POINTS = 20000;

/** Where everything in every frame is, so the camera does not jump as it plays.
 *  @param {SceneDoc} doc */
export function bounds(doc) {
  const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
  const add = (/** @type {number[]} */ p, r = 0) => {
    for (let i = 0; i < 3; i++) {
      if (!Number.isFinite(p?.[i])) continue;
      min[i] = Math.min(min[i], p[i] - r);
      max[i] = Math.max(max[i], p[i] + r);
    }
  };
  for (const frame of doc.frames ?? []) {
    for (const item of frame) {
      switch (item.kind) {
        case "sphere": add(item.center, item.radius ?? 1); break;
        case "bond": add(item.a, item.radius ?? 0.1); add(item.b, item.radius ?? 0.1); break;
        case "arrow": add(item.origin); add(item.origin.map((/** @type {number} */ o, /** @type {number} */ i) => o + item.vector[i])); break;
        case "points": case "line": for (const p of item.xyz ?? []) add(p); break;
        case "mesh": for (const p of item.vertices ?? []) add(p); break;
      }
    }
  }
  if (!Number.isFinite(min[0])) return { center: [0, 0, 0], radius: 1 };
  const center = [0, 1, 2].map((i) => (min[i] + max[i]) / 2);
  const radius = Math.max(1e-6, Math.hypot(max[0] - min[0], max[1] - min[1], max[2] - min[2]) / 2);
  return { center, radius };
}

/** A mesh's faces as triangles: each polygon a fan from its first corner,
 *  and any corner that names no vertex dropped with its triangle.
 *  @param {number[][]} faces @param {number} vertices */
export function triangles(faces, vertices) {
  const out = [];
  for (const face of faces ?? []) {
    for (let k = 1; k + 1 < face.length; k++) {
      const tri = [face[0], face[k], face[k + 1]];
      if (tri.every((i) => Number.isInteger(i) && i >= 0 && i < vertices)) out.push(...tri);
    }
  }
  return out;
}

/** @type {Promise<[Three, typeof import("./vendor/three/OrbitControls.js")]> | null} */
let loading = null;
/** three.js and its controls, imported once. */
function three() {
  loading ??= Promise.all([import("./vendor/three/three.module.js"), import("./vendor/three/OrbitControls.js")]);
  return loading;
}

/**
 * Draw a scene into a canvas. `null` when there is no WebGL to draw with.
 * @param {HTMLCanvasElement} canvas @param {SceneDoc} doc
 */
export async function mountScene(canvas, doc) {
  const [THREE, { OrbitControls }] = await three();
  /** @type {import("./vendor/three/three.module.js").WebGLRenderer} */
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: "low-power" });
  } catch {
    return null;
  }
  renderer.setClearColor(0x000000, 0);

  const scene = new THREE.Scene();
  const { center, radius } = bounds(doc);
  const middle = new THREE.Vector3(center[0], center[1], center[2]);
  const camera = new THREE.PerspectiveCamera(40, 1, radius / 200, radius * 60);
  camera.position.set(middle.x + radius * 1.9, middle.y + radius * 1.3, middle.z + radius * 2.1);
  // Lit from where it is seen, and softly from above, so turning it never
  // shows a side in the dark.
  scene.add(new THREE.HemisphereLight(0xdfe6ff, 0x1a1b26, 1.1));
  const key = new THREE.DirectionalLight(0xffffff, 2.2);
  key.position.set(1, 2, 3);
  camera.add(key);
  scene.add(camera);

  const controls = new OrbitControls(camera, canvas);
  controls.target.copy(middle);
  controls.minDistance = radius * 0.2;
  controls.maxDistance = radius * 30;
  controls.update();
  controls.saveState();

  /** What is made, to be let go of. @type {{dispose(): void}[]} */
  const made = [];
  const keep = (/** @type {any} */ thing) => { made.push(thing); return thing; };
  const ball = keep(new THREE.SphereGeometry(1, 32, 16));
  const rod = keep(new THREE.CylinderGeometry(1, 1, 1, 20, 1, true));
  const tip = keep(new THREE.ConeGeometry(1, 1, 24));
  /** @type {Map<string, any>} */
  const materials = new Map();
  /** One material a colour, shared. @param {unknown} css @param {number} i @param {number} [opacity] */
  const paint = (css, i, opacity = 1) => {
    const colour = typeof css === "string" && css ? css : PALETTE[i % PALETTE.length];
    const keyed = `${colour}/${opacity}`;
    let m = materials.get(keyed);
    if (!m) {
      let parsed;
      try { parsed = new THREE.Color(colour); } catch { parsed = new THREE.Color(PALETTE[i % PALETTE.length]); }
      m = keep(new THREE.MeshStandardMaterial({ color: parsed, roughness: 0.45, metalness: 0.05, transparent: opacity < 1, opacity, side: THREE.DoubleSide }));
      materials.set(keyed, m);
    }
    return m;
  };

  const up = new THREE.Vector3(0, 1, 0);
  /** A tube from `a` to `b`, `r` thick. @param {any} geometry @param {number[]} a @param {number[]} b @param {number} r @param {any} material */
  const span = (geometry, a, b, r, material) => {
    const from = new THREE.Vector3(a[0], a[1], a[2]), to = new THREE.Vector3(b[0], b[1], b[2]);
    const along = to.clone().sub(from);
    const length = along.length() || 1e-9;
    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.copy(from).add(to).multiplyScalar(0.5);
    mesh.quaternion.setFromUnitVectors(up, along.normalize());
    mesh.scale.set(r, length, r);
    return mesh;
  };

  /** A frame, built the first time it is shown. @type {Map<number, any>} */
  const built = new Map();
  const frames = doc.frames?.length ? doc.frames : [[]];
  /** @param {number} f */
  function build(f) {
    const group = new THREE.Group();
    frames[f].forEach((item, i) => {
      switch (item.kind) {
        case "sphere": {
          const mesh = new THREE.Mesh(ball, paint(item.color, i));
          mesh.position.set(item.center[0], item.center[1], item.center[2]);
          const r = item.radius ?? 1;
          mesh.scale.set(r, r, r);
          group.add(mesh);
          break;
        }
        case "bond":
          group.add(span(rod, item.a, item.b, item.radius ?? 0.1, paint(item.color, i)));
          break;
        case "arrow": {
          const o = item.origin, v = item.vector;
          const length = Math.hypot(v[0], v[1], v[2]);
          const neck = [o[0] + v[0] * 0.8, o[1] + v[1] * 0.8, o[2] + v[2] * 0.8];
          const end = [o[0] + v[0], o[1] + v[1], o[2] + v[2]];
          group.add(span(rod, o, neck, length * 0.025, paint(item.color, i)));
          group.add(span(tip, neck, end, length * 0.07, paint(item.color, i)));
          break;
        }
        case "points": {
          const xyz = (item.xyz ?? []).slice(0, MOST_POINTS);
          // As small spheres rather than square sprites: a point cloud is
          // usually a shape, and squares do not read as one when it turns.
          const dot = keep(new THREE.InstancedMesh(ball, paint(item.color, i), xyz.length));
          const r = radius * 0.006 * ((item.size ?? 5) / 5);
          const m = new THREE.Matrix4(), q = new THREE.Quaternion(), s = new THREE.Vector3(r, r, r);
          xyz.forEach((/** @type {number[]} */ p, /** @type {number} */ k) => dot.setMatrixAt(k, m.compose(new THREE.Vector3(p[0], p[1], p[2]), q, s)));
          dot.instanceMatrix.needsUpdate = true;
          group.add(dot);
          break;
        }
        case "line": {
          const geometry = keep(new THREE.BufferGeometry());
          geometry.setAttribute("position", new THREE.Float32BufferAttribute((item.xyz ?? []).flat(), 3));
          const css = typeof item.color === "string" && item.color ? item.color : PALETTE[i % PALETTE.length];
          group.add(new THREE.Line(geometry, keep(new THREE.LineBasicMaterial({ color: new THREE.Color(css) }))));
          break;
        }
        case "mesh": {
          const vertices = item.vertices ?? [];
          const geometry = keep(new THREE.BufferGeometry());
          geometry.setAttribute("position", new THREE.Float32BufferAttribute(vertices.flat(), 3));
          geometry.setIndex(triangles(item.faces, vertices.length));
          geometry.computeVertexNormals();
          group.add(new THREE.Mesh(geometry, paint(item.color, i, item.opacity ?? 1)));
          break;
        }
      }
    });
    return group;
  }

  let shown = -1;
  /** @param {number} f */
  function show(f) {
    const next = Math.max(0, Math.min(frames.length - 1, f));
    if (next === shown) return;
    if (shown >= 0) built.get(shown).visible = false;
    if (!built.has(next)) {
      const group = build(next);
      built.set(next, group);
      scene.add(group);
    }
    built.get(next).visible = true;
    shown = next;
    draw();
  }

  let queued = false;
  function draw() {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      const w = Math.max(1, canvas.clientWidth), h = Math.max(1, canvas.clientHeight);
      renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1));
      renderer.setSize(w, h, false);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      renderer.render(scene, camera);
    });
  }

  controls.addEventListener("change", draw);
  const reset = () => { controls.reset(); draw(); };
  canvas.addEventListener("dblclick", reset);
  const resize = new ResizeObserver(draw);
  resize.observe(canvas);
  show(0);

  return {
    frames: frames.length,
    /** @param {number} f */
    show,
    destroy() {
      resize.disconnect();
      canvas.removeEventListener("dblclick", reset);
      controls.dispose();
      for (const thing of made) thing.dispose();
      renderer.dispose();
    },
  };
}
