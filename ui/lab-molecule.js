/* The Lab's molecule view: a molecule or a protein read by ui/molecule.js,
 * drawn with three.js — imported the first time, as the 3D scenes are.
 *
 * Atoms are impostors: a square a sphere, facing the camera, the sphere
 * worked out a pixel at a time and written into the depth buffer, so a
 * protein's hundred thousand atoms are four corners each, not a thousand
 * triangles. Bonds are one instanced cylinder, half in each atom's colour.
 * A protein's cartoon is a tube along its α-carbons, flattened into ribbons
 * for helices and into arrows for strands, turned by each residue's C=O.
 *
 * Clicking an atom says what it is; a second says how far apart the two
 * are. Nothing is drawn but on a change. */

import { three } from "./lab3d.js";
import { backbone, describe, element, isProtein, isWater, ready } from "./molecule.js";

/** @typedef {import("./molecule.js").Molecule} Molecule */
/** @typedef {import("./molecule.js").Model} Model */
/** @typedef {import("./molecule.js").Atom} Atom */
/** @typedef {"ball" | "space" | "sticks" | "cartoon"} Style */

/** The ways to draw, by what they are called. @type {Record<Style, string>} */
export const STYLES = { cartoon: "Cartoon", ball: "Ball and stick", sticks: "Sticks", space: "Spacefill" };

const STRUCTURE = { H: "#e0607e", E: "#e8c35a", C: "#b9c0cc" };
const PICKED = "#7dcfff";

const SPHERE_VERTEX = /* glsl */ `
attribute vec3 center;
attribute float radius;
attribute vec3 tint;
varying vec3 vCenter;
varying vec3 vPos;
varying float vRadius;
varying vec3 vTint;
void main() {
  vec4 c = modelViewMatrix * vec4(center, 1.0);
  // In front of the sphere, wide enough for its outline seen at an angle.
  vec3 p = c.xyz + vec3(position.xy * radius * 1.6, radius);
  vCenter = c.xyz;
  vPos = p;
  vRadius = radius;
  vTint = tint;
  gl_Position = projectionMatrix * vec4(p, 1.0);
}`;

const SPHERE_FRAGMENT = /* glsl */ `
uniform mat4 projectionMatrix;
varying vec3 vCenter;
varying vec3 vPos;
varying float vRadius;
varying vec3 vTint;
void main() {
  vec3 dir = normalize(vPos);
  float b = dot(dir, vCenter);
  float c = dot(vCenter, vCenter) - vRadius * vRadius;
  float disc = b * b - c;
  if (disc < 0.0) discard;
  vec3 hit = dir * (b - sqrt(disc));
  vec3 n = normalize(hit - vCenter);
  // The light the bonds have: from beside the camera, above and right.
  vec3 l = normalize(vec3(0.27, 0.53, 0.8));
  float diffuse = max(dot(n, l), 0.0);
  float spec = pow(max(dot(reflect(-l, n), -dir), 0.0), 40.0) * 0.3;
  gl_FragColor = vec4(vTint * (0.3 + 0.8 * diffuse) + spec, 1.0);
  vec4 clip = projectionMatrix * vec4(hit, 1.0);
  gl_FragDepth = (clip.z / clip.w) * 0.5 + 0.5;
  #include <colorspace_fragment>
}`;

/** The directions atoms spread most, next most and least: the covariance's
 *  eigenvectors, by power iteration, right-handed. @param {Atom[]} atoms
 *  @returns {[number, number, number][]} */
export function axes(atoms) {
  const n = atoms.length;
  const mid = [0, 0, 0];
  for (const a of atoms) { mid[0] += a.x / n; mid[1] += a.y / n; mid[2] += a.z / n; }
  const C = [[0, 0, 0], [0, 0, 0], [0, 0, 0]];
  for (const a of atoms) {
    const p = [a.x - mid[0], a.y - mid[1], a.z - mid[2]];
    for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) C[i][j] += p[i] * p[j];
  }
  const norm = (/** @type {number[]} */ v) => { const l = Math.hypot(v[0], v[1], v[2]); return l > 1e-12 ? v.map((x) => x / l) : null; };
  const times = (/** @type {number[][]} */ M, /** @type {number[]} */ v) => M.map((row) => row[0] * v[0] + row[1] * v[1] + row[2] * v[2]);
  /** @param {number[][]} M @param {number[]} start @param {number[] | null} away */
  const power = (M, start, away) => {
    let v = start;
    for (let k = 0; k < 60; k++) {
      let w = times(M, v);
      if (away) { const d = w[0] * away[0] + w[1] * away[1] + w[2] * away[2]; w = w.map((x, i) => x - d * away[i]); }
      const u = norm(w);
      if (!u) break;
      v = u;
    }
    return v;
  };
  const e1 = power(C, [1, 0.31, 0.17], null);
  let e2 = power(C, norm([-e1[1], e1[0] + 0.2, 0.3]) ?? [0, 1, 0], e1);
  const d = e2[0] * e1[0] + e2[1] * e1[1] + e2[2] * e1[2];
  e2 = norm(e2.map((x, i) => x - d * e1[i])) ?? [0, 1, 0];
  const e3 = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
  return /** @type {[number, number, number][]} */ ([e1, e2, e3]);
}

/** The styles a molecule can be drawn in: a cartoon only for a protein.
 *  @param {Molecule} molecule @returns {Style[]} */
export function stylesFor(molecule) {
  const first = molecule.models[0];
  const protein = !first.pending && isProtein(first);
  return protein ? ["cartoon", "ball", "sticks", "space"] : ["ball", "sticks", "space"];
}

/** Atom and bond sizes, Å, in a style. @param {Style} style @param {Atom} a */
function atomRadius(style, a) {
  const e = element(a.el);
  if (style === "space") return e.vdw;
  if (style === "sticks") return 0.16;
  return Math.max(0.18, e.vdw * 0.24);
}

/**
 * Draw a molecule into a canvas. `null` when there is no WebGL to draw with.
 * @param {HTMLCanvasElement} canvas @param {Molecule} molecule
 * @param {{style?: Style, onPick?: (text: string | null) => void}} [options]
 */
export async function mountMolecule(canvas, molecule, options = {}) {
  const [THREE, { OrbitControls }] = await three();
  /** @type {any} */
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: "low-power" });
  } catch {
    return null;
  }
  renderer.setClearColor(0x000000, 0);
  const T = /** @type {any} */ (THREE);

  const scene = /** @type {any} */ (new THREE.Scene());
  const camera = /** @type {any} */ (new THREE.PerspectiveCamera(35, 1, 0.05, 5000));
  scene.add(new THREE.HemisphereLight(0xdfe6ff, 0x1a1b26, 1.1));
  const key = new THREE.DirectionalLight(0xffffff, 2.2);
  key.position.set(1, 2, 3);
  camera.add(key);
  scene.add(camera);
  const controls = /** @type {any} */ (new OrbitControls(camera, canvas));
  // Everything drawn is in here, turned so a molecule shows its widest face.
  const root = new T.Group();
  scene.add(root);

  /** What is made, to be let go of. @type {{dispose(): void}[]} */
  const made = [];
  const keep = (/** @type {any} */ thing) => { made.push(thing); return thing; };
  const rod = keep(new T.CylinderGeometry(1, 1, 1, 10, 1, true));
  const bondPaint = keep(new T.MeshStandardMaterial({ roughness: 0.45, metalness: 0.05 }));
  const ribbonPaint = keep(new T.MeshStandardMaterial({ vertexColors: true, roughness: 0.5, metalness: 0.02, side: T.DoubleSide }));
  const spherePaint = keep(new T.ShaderMaterial({ vertexShader: SPHERE_VERTEX, fragmentShader: SPHERE_FRAGMENT }));
  const quad = keep(new T.InstancedBufferGeometry());
  quad.setAttribute("position", new T.Float32BufferAttribute([-1, -1, 0, 1, -1, 0, 1, 1, 0, -1, 1, 0], 3));
  quad.setIndex([0, 1, 2, 0, 2, 3]);
  const ring = keep(new T.SphereGeometry(1, 24, 12));
  const ringPaint = keep(new T.MeshBasicMaterial({ color: new T.Color(PICKED), transparent: true, opacity: 0.35, depthWrite: false }));
  const linePaint = keep(new T.LineBasicMaterial({ color: new T.Color(PICKED) }));

  /** @type {Map<string, any>} */
  const colours = new Map();
  /** A colour, in the renderer's linear space. @param {string} css */
  const colour = (css) => {
    let c = colours.get(css);
    if (!c) { c = new T.Color(css); colours.set(css, c); }
    return c;
  };

  const kinds = stylesFor(molecule);
  /** @type {Style} */
  let style = options.style && kinds.includes(options.style) ? options.style : kinds[0];

  /** Spheres for `atoms` (indices), as impostors. @param {Atom[]} atoms @param {number[]} which @param {(a: Atom) => number} size */
  function spheres(atoms, which, size) {
    const geometry = keep(new T.InstancedBufferGeometry());
    geometry.index = quad.index;
    geometry.setAttribute("position", quad.getAttribute("position"));
    const center = new Float32Array(which.length * 3), radius = new Float32Array(which.length), tint = new Float32Array(which.length * 3);
    which.forEach((i, k) => {
      const a = atoms[i], c = colour(element(a.el).color);
      center.set([a.x, a.y, a.z], k * 3);
      radius[k] = size(a);
      tint.set([c.r, c.g, c.b], k * 3);
    });
    geometry.setAttribute("center", new T.InstancedBufferAttribute(center, 3));
    geometry.setAttribute("radius", new T.InstancedBufferAttribute(radius, 1));
    geometry.setAttribute("tint", new T.InstancedBufferAttribute(tint, 3));
    geometry.instanceCount = which.length;
    const mesh = new T.Mesh(geometry, spherePaint);
    // Its corners are not where its spheres are: three.js cannot cull it.
    mesh.frustumCulled = false;
    return mesh;
  }

  const up = new T.Vector3(0, 1, 0);
  /** Bonds as cylinders, half in each atom's colour; a double bond two, a
   *  triple three, side by side in the plane of a neighbour.
   *  @param {Model} model @param {Set<number>} shown @param {number} r */
  function cylinders(model, shown, r) {
    const atoms = model.atoms;
    const bonds = (model.bonds ?? []).filter(([i, j]) => shown.has(i) && shown.has(j));
    /** @type {number[][]} */
    const near = atoms.map(() => []);
    for (const [i, j] of bonds) { near[i].push(j); near[j].push(i); }
    /** @type {[any, any, number, string][]} */
    const pieces = [];
    const P = (/** @type {number} */ i) => new T.Vector3(atoms[i].x, atoms[i].y, atoms[i].z);
    for (const [i, j, order] of bonds) {
      const a = P(i), b = P(j);
      const along = b.clone().sub(a).normalize();
      const lines = order >= 2 && order <= 3 && style !== "space" ? order : 1;
      let side = new T.Vector3();
      if (lines > 1) {
        const other = near[i].find((k) => k !== j) ?? near[j].find((k) => k !== i);
        const towards = other != null ? P(other).sub(a) : new T.Vector3(along.y, -along.x, along.z + 1);
        side = towards.sub(along.clone().multiplyScalar(towards.dot(along)));
        if (side.length() < 1e-6) side = new T.Vector3(1, 0, 0).cross(along);
        side.normalize();
      }
      const thin = lines === 1 ? r : r * 0.6;
      const gap = r * 1.5;
      for (let k = 0; k < lines; k++) {
        const shift = side.clone().multiplyScalar((k - (lines - 1) / 2) * gap);
        const from = a.clone().add(shift), to = b.clone().add(shift);
        const mid = from.clone().add(to).multiplyScalar(0.5);
        pieces.push([from, mid, thin, element(atoms[i].el).color], [mid, to, thin, element(atoms[j].el).color]);
      }
    }
    const mesh = keep(new T.InstancedMesh(rod, bondPaint, Math.max(1, pieces.length)));
    mesh.count = pieces.length;
    const m = new T.Matrix4(), q = new T.Quaternion(), s = new T.Vector3();
    pieces.forEach(([from, to, thick, css], k) => {
      const d = to.clone().sub(from);
      q.setFromUnitVectors(up, d.clone().normalize());
      s.set(thick, d.length(), thick);
      mesh.setMatrixAt(k, m.compose(from.clone().add(to).multiplyScalar(0.5), q, s));
      mesh.setColorAt(k, colour(css));
    });
    mesh.instanceMatrix.needsUpdate = true;
    if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
    return mesh;
  }

  /** A protein's cartoon: one tube a piece of chain, ribbons and arrows
   *  where the residues are helix and strand. @param {Model} model */
  function cartoon(model) {
    const group = new T.Group();
    const atoms = model.atoms;
    const V = (/** @type {number} */ i) => new T.Vector3(atoms[i].x, atoms[i].y, atoms[i].z);
    const SUB = 8, SIDES = 10;
    for (const piece of backbone(model).pieces) {
      const rs = piece.residues;
      // A strand's α-carbons zig-zag a pleat apart; drawn through their
      // running average it lies flat, as strands are drawn.
      const points = rs.map((r, k) => {
        const p = V(r.ca);
        if (r.ss !== "E" || k === 0 || k === rs.length - 1) return p;
        return p.multiplyScalar(0.5).add(V(rs[k - 1].ca).multiplyScalar(0.25)).add(V(rs[k + 1].ca).multiplyScalar(0.25));
      });
      // Each residue's C=O, turned to agree with the one before, so a ribbon
      // does not twist through itself.
      /** @type {any[]} */
      const guides = [];
      rs.forEach((r, k) => {
        let g = r.o != null ? V(r.o).sub(points[k]) : new T.Vector3(0, 0, 1);
        if (k > 0 && g.dot(guides[k - 1]) < 0) g.multiplyScalar(-1);
        guides.push(g.normalize());
      });
      const curve = new T.CatmullRomCurve3(points, false, "centripetal");
      const n = (rs.length - 1) * SUB + 1;
      const positions = [], colors = [];
      /** @type {number[]} */
      const index = [];
      for (let s = 0; s < n; s++) {
        const u = s / (n - 1);
        const at = s / SUB;
        const k = Math.min(rs.length - 1, Math.floor(at)), f = at - k;
        const p = curve.getPoint(u);
        const t = curve.getTangent(u).normalize();
        const g = guides[k].clone().multiplyScalar(1 - f).add(guides[Math.min(rs.length - 1, k + 1)].clone().multiplyScalar(f));
        const nrm = g.sub(t.clone().multiplyScalar(g.dot(t))).normalize();
        const bin = t.clone().cross(nrm).normalize();
        const r = rs[Math.min(rs.length - 1, Math.round(at))];
        let wide = 0.28, thick = 0.28;
        if (r.ss === "H") { wide = 1.3; thick = 0.28; }
        if (r.ss === "E") {
          wide = 1.0; thick = 0.3;
          // The strand's last residue is its arrowhead.
          const idx = Math.round(at);
          const last = idx === rs.length - 1 || rs[idx + 1]?.ss !== "E";
          if (last && idx > 0) {
            const into = at - (idx - 0.5);
            wide = Math.max(0.28, 1.7 * (1 - Math.max(0, Math.min(1, into))));
          }
        }
        const c = colour(STRUCTURE[r.ss]);
        for (let side = 0; side < SIDES; side++) {
          const th = (side / SIDES) * Math.PI * 2;
          // A rounded rectangle, by a superellipse.
          const cx = Math.sign(Math.cos(th)) * Math.abs(Math.cos(th)) ** 0.5, cy = Math.sign(Math.sin(th)) * Math.abs(Math.sin(th)) ** 0.5;
          const v = p.clone().add(nrm.clone().multiplyScalar(cx * wide)).add(bin.clone().multiplyScalar(cy * thick));
          positions.push(v.x, v.y, v.z);
          colors.push(c.r, c.g, c.b);
        }
        if (s > 0) {
          for (let side = 0; side < SIDES; side++) {
            const a = (s - 1) * SIDES + side, b = (s - 1) * SIDES + ((side + 1) % SIDES);
            index.push(a, b, a + SIDES, b, b + SIDES, a + SIDES);
          }
        }
      }
      const geometry = keep(new T.BufferGeometry());
      geometry.setAttribute("position", new T.Float32BufferAttribute(positions, 3));
      geometry.setAttribute("color", new T.Float32BufferAttribute(colors, 3));
      geometry.setIndex(index);
      geometry.computeVertexNormals();
      group.add(new T.Mesh(geometry, ribbonPaint));
    }
    return group;
  }

  const models = molecule.models;
  /** What each frame drew, and what can be picked in it. @type {Map<number, {group: any, pick: [number, number][]}>} */
  const built = new Map();

  /** A frame in the style chosen. @param {number} f */
  function build(f) {
    const model = ready(models[f]);
    const group = new T.Group();
    const atoms = model.atoms;
    const protein = isProtein(model);
    /** @type {number[]} */
    let shown = atoms.map((_, i) => i).filter((i) => !(protein && isWater(atoms[i])));
    /** @type {[number, number][]} */
    let pick;
    if (style === "cartoon" && protein) {
      group.add(cartoon(model));
      // What is not protein — a ligand, a metal — beside it, as ball and stick.
      const others = shown.filter((i) => atoms[i].het);
      group.add(spheres(atoms, others, (a) => atomRadius("ball", a)));
      group.add(cylinders(model, new Set(others), 0.12));
      const cas = backbone(model).pieces.flatMap((p) => p.residues.map((r) => r.ca));
      pick = [...cas.map((i) => /** @type {[number, number]} */ ([i, 1.2])), ...others.map((i) => /** @type {[number, number]} */ ([i, atomRadius("ball", atoms[i])]))];
    } else {
      const drawn = style === "cartoon" ? "ball" : style;
      group.add(spheres(atoms, shown, (a) => atomRadius(drawn, a)));
      if (drawn !== "space") group.add(cylinders(model, new Set(shown), drawn === "sticks" ? 0.16 : 0.11));
      pick = shown.map((i) => [i, atomRadius(drawn, atoms[i])]);
    }
    return { group, pick };
  }

  /** Where a model is, to look at it whole: turned so its longest spread
   *  runs across the view and its shortest away from it. @param {Model} model */
  function fit(model) {
    const atoms = model.atoms;
    if (!atoms.length) return;
    const [e1, e2, e3] = axes(atoms);
    const basis = new T.Matrix4().makeBasis(new T.Vector3(...e1), new T.Vector3(...e2), new T.Vector3(...e3));
    root.quaternion.setFromRotationMatrix(basis).invert();
    root.updateMatrixWorld(true);
    const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
    const v = new T.Vector3();
    for (const a of atoms) {
      v.set(a.x, a.y, a.z).applyQuaternion(root.quaternion);
      const p = [v.x, v.y, v.z];
      for (let c = 0; c < 3; c++) { min[c] = Math.min(min[c], p[c]); max[c] = Math.max(max[c], p[c]); }
    }
    const mid = new T.Vector3((min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2);
    const radius = Math.max(2, Math.hypot(max[0] - min[0], max[1] - min[1], max[2] - min[2]) / 2 + 2);
    camera.near = radius / 100;
    camera.far = radius * 40;
    // Near enough to fill the view: the radius is a box's half-diagonal,
    // larger than what is in it.
    camera.position.set(mid.x, mid.y + radius * 0.25, mid.z + radius * 2.3);
    camera.updateProjectionMatrix();
    controls.target.copy(mid);
    controls.minDistance = radius * 0.1;
    controls.maxDistance = radius * 12;
    controls.update();
    controls.saveState();
  }

  /* ── picking ── */

  /** @type {number[]} */
  let picked = [];
  const marks = new T.Group();
  root.add(marks);
  /** @type {any} */
  let between = null;
  function markPicked() {
    marks.clear();
    between?.dispose();
    between = null;
    const model = models[shown];
    const atoms = model.atoms;
    for (const i of picked) {
      const a = atoms[i];
      const mesh = new T.Mesh(ring, ringPaint);
      const r = (built.get(shown)?.pick.find(([k]) => k === i)?.[1] ?? 0.5) * 1.35 + 0.15;
      mesh.position.set(a.x, a.y, a.z);
      mesh.scale.set(r, r, r);
      marks.add(mesh);
    }
    if (picked.length === 2) {
      const [a, b] = picked.map((i) => atoms[i]);
      between = new T.BufferGeometry();
      between.setAttribute("position", new T.Float32BufferAttribute([a.x, a.y, a.z, b.x, b.y, b.z], 3));
      marks.add(new T.Line(between, linePaint));
    }
    const say = options.onPick;
    if (!say) return;
    if (!picked.length) return say(null);
    const lines = picked.map((i) => describe(atoms[i], i));
    if (picked.length === 2) {
      const [a, b] = picked.map((i) => atoms[i]);
      lines.push(`${Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z).toFixed(2)} Å apart`);
    }
    say(lines.join("\n"));
  }

  /** The nearest pickable atom under a point of the canvas, if any.
   *  @param {number} x @param {number} y */
  function atomAt(x, y) {
    const rect = canvas.getBoundingClientRect();
    const ray = new T.Raycaster();
    ray.setFromCamera(new T.Vector2(((x - rect.left) / rect.width) * 2 - 1, -((y - rect.top) / rect.height) * 2 + 1), camera);
    // Into the atoms' own frame, which the root turned.
    ray.ray.applyMatrix4(root.matrixWorld.clone().invert());
    const o = ray.ray.origin, d = ray.ray.direction;
    const atoms = models[shown].atoms;
    let best = -1, bestT = Infinity;
    for (const [i, r] of built.get(shown)?.pick ?? []) {
      const a = atoms[i];
      const ox = o.x - a.x, oy = o.y - a.y, oz = o.z - a.z;
      const b = ox * d.x + oy * d.y + oz * d.z;
      const c = ox * ox + oy * oy + oz * oz - r * r;
      const disc = b * b - c;
      if (disc < 0) continue;
      const t = -b - Math.sqrt(disc);
      if (t > 0 && t < bestT) { bestT = t; best = i; }
    }
    return best;
  }

  /** @type {{x: number, y: number} | null} */
  let down = null;
  const onDown = (/** @type {PointerEvent} */ e) => { down = { x: e.clientX, y: e.clientY }; };
  const onUp = (/** @type {PointerEvent} */ e) => {
    // A drag turns it; only a click picks.
    if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4) return void (down = null);
    down = null;
    const i = atomAt(e.clientX, e.clientY);
    if (i < 0) picked = [];
    else if (picked.length === 1 && picked[0] !== i) picked = [picked[0], i];
    else if (picked[0] !== i || picked.length !== 1) picked = [i];
    markPicked();
    draw();
  };
  canvas.addEventListener("pointerdown", onDown);
  canvas.addEventListener("pointerup", onUp);

  /* ── frames ── */

  let shown = -1;
  /** @param {number} f */
  function show(f) {
    const next = Math.max(0, Math.min(models.length - 1, f));
    if (next === shown) return;
    const before = built.get(shown);
    if (before) before.group.visible = false;
    let frame = built.get(next);
    if (!frame) {
      frame = build(next);
      built.set(next, frame);
      root.add(frame.group);
    }
    frame.group.visible = true;
    // A trajectory keeps the camera where it is; different molecules in
    // one file are each looked at whole.
    const different = shown < 0 || molecule.kind === "SMILES" || molecule.kind === "SDF";
    shown = next;
    if (different) fit(models[next]);
    if (picked.length) {
      picked = picked.filter((i) => i < models[next].atoms.length);
      markPicked();
    }
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
    frames: models.length,
    styles: kinds,
    get style() { return style; },
    /** @param {number} f */
    show,
    /** Drawn again in another style, frames rebuilt as they are shown.
     *  @param {Style} next */
    restyle(next) {
      if (!kinds.includes(next) || next === style) return;
      style = next;
      for (const { group } of built.values()) root.remove(group);
      built.clear();
      const frame = build(shown);
      built.set(shown, frame);
      root.add(frame.group);
      markPicked();
      draw();
    },
    destroy() {
      resize.disconnect();
      canvas.removeEventListener("dblclick", reset);
      canvas.removeEventListener("pointerdown", onDown);
      canvas.removeEventListener("pointerup", onUp);
      controls.dispose();
      between?.dispose();
      for (const thing of made) thing.dispose();
      renderer.dispose();
    },
  };
}
