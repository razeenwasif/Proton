/* The Lab's 3D view: a run's `proton.scene/1` — spheres and bonds, points,
 * lines, arrows and meshes, in frames — drawn with WebGL 2, turned by
 * dragging, brought nearer with the wheel, and played a frame at a time.
 *
 * Its own small renderer rather than Modeling's: that one is a single
 * context bound to the viewport, compiling a material graph per pixel; this
 * one lights a few hundred primitives the same plain way, in as many
 * canvases as the Lab has scenes open. Nothing is drawn but on a change —
 * a turn, a frame — so an open scene costs nothing while it is still.
 *
 * Without WebGL 2 it says so; a scene is not worth a second, worse picture. */

/** @typedef {[number, number, number]} Vec3 */
/** @typedef {{kind: string, [key: string]: any}} SceneItem */
/** @typedef {{format: string, title?: string, units?: string | null, frames: SceneItem[][]}} SceneDoc */

const PALETTE = ["#7aa2f7", "#e0af68", "#9ece6a", "#f7768e", "#bb9af7", "#7dcfff", "#ff9e64", "#c0caf5"];

/* ── geometry ─────────────────────────────────────────────────────── */

/** A sphere of radius 1: an icosahedron, its faces split `detail` times.
 *  @param {number} [detail] */
export function icosphere(detail = 2) {
  const t = (1 + Math.sqrt(5)) / 2;
  /** @type {Vec3[]} */
  let verts = [[-1, t, 0], [1, t, 0], [-1, -t, 0], [1, -t, 0], [0, -1, t], [0, 1, t], [0, -1, -t], [0, 1, -t], [t, 0, -1], [t, 0, 1], [-t, 0, -1], [-t, 0, 1]];
  let faces = [[0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11], [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
    [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9], [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1]];
  const unit = (/** @type {Vec3} */ v) => { const l = Math.hypot(...v); return /** @type {Vec3} */ ([v[0] / l, v[1] / l, v[2] / l]); };
  verts = verts.map(unit);
  for (let d = 0; d < detail; d++) {
    /** @type {Map<string, number>} */
    const cache = new Map();
    const mid = (/** @type {number} */ a, /** @type {number} */ b) => {
      const key = a < b ? `${a}_${b}` : `${b}_${a}`;
      let i = cache.get(key);
      if (i === undefined) {
        const [p, q] = [verts[a], verts[b]];
        i = verts.push(unit([(p[0] + q[0]) / 2, (p[1] + q[1]) / 2, (p[2] + q[2]) / 2])) - 1;
        cache.set(key, i);
      }
      return i;
    };
    faces = faces.flatMap(([a, b, c]) => {
      const [ab, bc, ca] = [mid(a, b), mid(b, c), mid(c, a)];
      return [[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]];
    });
  }
  const positions = new Float32Array(verts.flat());
  // On a unit sphere the normal is the position.
  return { positions, normals: positions.slice(), indices: new Uint16Array(faces.flat()) };
}

/** A tube of radius 1 along +Y, from 0 to 1, open at its ends, or a cone
 *  closing to a point at 1. @param {number} [segments] @param {boolean} [cone] */
export function tube(segments = 16, cone = false) {
  const positions = [], normals = [], indices = [];
  for (let i = 0; i <= segments; i++) {
    const a = (i / segments) * Math.PI * 2;
    const [x, z] = [Math.cos(a), Math.sin(a)];
    // A cone's sides lean in: its normal leans out and up to match.
    const n = cone ? [x / Math.SQRT2, 1 / Math.SQRT2, z / Math.SQRT2] : [x, 0, z];
    positions.push(x, 0, z, cone ? 0 : x, 1, cone ? 0 : z);
    normals.push(...n, ...n);
  }
  for (let i = 0; i < segments; i++) {
    const [a, b, c, d] = [i * 2, i * 2 + 1, i * 2 + 2, i * 2 + 3];
    indices.push(a, b, c, b, d, c);
  }
  return { positions: new Float32Array(positions), normals: new Float32Array(normals), indices: new Uint16Array(indices) };
}

/** A mesh's triangles, with a normal per face. @param {number[][]} vertices @param {number[][]} faces */
export function faceted(vertices, faces) {
  const positions = [], normals = [];
  for (const face of faces) {
    for (let k = 1; k + 1 < face.length; k++) {
      const [a, b, c] = [vertices[face[0]], vertices[face[k]], vertices[face[k + 1]]];
      if (!a || !b || !c) continue;
      const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]], v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
      const n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
      const l = Math.hypot(n[0], n[1], n[2]) || 1;
      for (const p of [a, b, c]) {
        positions.push(p[0], p[1], p[2]);
        normals.push(n[0] / l, n[1] / l, n[2] / l);
      }
    }
  }
  return { positions: new Float32Array(positions), normals: new Float32Array(normals) };
}

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

/* ── matrices, column-major as GL reads them ──────────────────────── */

/** @param {number[]} a @param {number[]} b */
function mul(a, b) {
  const o = new Array(16).fill(0);
  for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++) for (let k = 0; k < 4; k++) o[c * 4 + r] += a[k * 4 + r] * b[c * 4 + k];
  return o;
}

/** @param {number} fov @param {number} aspect @param {number} near @param {number} far */
function perspective(fov, aspect, near, far) {
  const f = 1 / Math.tan(fov / 2), n = 1 / (near - far);
  return [f / aspect, 0, 0, 0, 0, f, 0, 0, 0, 0, (far + near) * n, -1, 0, 0, 2 * far * near * n, 0];
}

/** @param {number[]} eye @param {number[]} at */
function lookAt(eye, at) {
  const z = norm(sub(eye, at)), x = norm(cross([0, 1, 0], z)), y = cross(z, x);
  return [x[0], y[0], z[0], 0, x[1], y[1], z[1], 0, x[2], y[2], z[2], 0, -dot(x, eye), -dot(y, eye), -dot(z, eye), 1];
}

const sub = (/** @type {number[]} */ a, /** @type {number[]} */ b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (/** @type {number[]} */ a, /** @type {number[]} */ b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (/** @type {number[]} */ a, /** @type {number[]} */ b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const norm = (/** @type {number[]} */ v) => { const l = Math.hypot(v[0], v[1], v[2]) || 1; return [v[0] / l, v[1] / l, v[2] / l]; };

/** The model matrix taking the unit +Y tube from `a` to `b`, `r` thick.
 *  @param {number[]} a @param {number[]} b @param {number} r */
export function between(a, b, r) {
  const d = sub(b, a), len = Math.hypot(d[0], d[1], d[2]) || 1e-9;
  const y = [d[0] / len, d[1] / len, d[2] / len];
  const helper = Math.abs(y[1]) < 0.99 ? [0, 1, 0] : [1, 0, 0];
  const x = norm(cross(helper, y)), z = cross(x, y);
  return [x[0] * r, x[1] * r, x[2] * r, 0, y[0] * len, y[1] * len, y[2] * len, 0, z[0] * r, z[1] * r, z[2] * r, 0, a[0], a[1], a[2], 1];
}

/** @param {number[]} c @param {number} r */
const placed = (c, r) => [r, 0, 0, 0, 0, r, 0, 0, 0, 0, r, 0, c[0], c[1], c[2], 1];

/* ── colour ───────────────────────────────────────────────────────── */

/** @type {CanvasRenderingContext2D | null} */
let swatch = null;
/** A CSS colour as linear-ish RGBA in 0–1. @param {unknown} css @param {number} i */
function colour(css, i) {
  const fallback = PALETTE[i % PALETTE.length];
  swatch ??= document.createElement("canvas").getContext("2d");
  if (!swatch) return [0.6, 0.7, 0.9, 1];
  swatch.fillStyle = "#000";
  swatch.fillStyle = typeof css === "string" && css ? css : fallback;
  const hex = String(swatch.fillStyle);
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(hex);
  if (m) return [parseInt(m[1], 16) / 255, parseInt(m[2], 16) / 255, parseInt(m[3], 16) / 255, 1];
  const rgba = /rgba?\(([^)]+)\)/.exec(hex)?.[1].split(",").map(Number) ?? [150, 170, 230, 1];
  return [rgba[0] / 255, rgba[1] / 255, rgba[2] / 255, rgba[3] ?? 1];
}

/* ── the renderer ─────────────────────────────────────────────────── */

const SHADED_VS = `#version 300 es
in vec3 position; in vec3 normal;
uniform mat4 model, view, proj;
out vec3 vNormal; out vec3 vWorld;
void main() {
  vec4 world = model * vec4(position, 1.0);
  vWorld = world.xyz;
  vNormal = mat3(model) * normal;
  gl_Position = proj * view * world;
}`;
const SHADED_FS = `#version 300 es
precision highp float;
in vec3 vNormal; in vec3 vWorld;
uniform vec4 color; uniform vec3 eye;
out vec4 outColor;
void main() {
  vec3 n = normalize(vNormal);
  vec3 v = normalize(eye - vWorld);
  if (dot(n, v) < 0.0) n = -n;
  vec3 l = normalize(vec3(0.4, 0.8, 0.5));
  float diffuse = max(dot(n, l), 0.0);
  float spec = pow(max(dot(reflect(-l, n), v), 0.0), 32.0) * 0.25;
  float rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.15;
  outColor = vec4(color.rgb * (0.28 + 0.72 * diffuse) + vec3(spec + rim), color.a);
}`;
const FLAT_VS = `#version 300 es
in vec3 position;
uniform mat4 view, proj; uniform float size;
void main() { gl_Position = proj * view * vec4(position, 1.0); gl_PointSize = size; }`;
const FLAT_FS = `#version 300 es
precision highp float;
uniform vec4 color; uniform float round;
out vec4 outColor;
void main() {
  if (round > 0.5 && length(gl_PointCoord - 0.5) > 0.5) discard;
  outColor = color;
}`;

/** @param {WebGL2RenderingContext} gl @param {string} vs @param {string} fs */
function program(gl, vs, fs) {
  const p = /** @type {WebGLProgram} */ (gl.createProgram());
  for (const [type, src] of /** @type {[number, string][]} */ ([[gl.VERTEX_SHADER, vs], [gl.FRAGMENT_SHADER, fs]])) {
    const s = /** @type {WebGLShader} */ (gl.createShader(type));
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? "shader");
    gl.attachShader(p, s);
  }
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? "link");
  return p;
}

/**
 * Draw a scene into a canvas. `null` when there is no WebGL 2 to draw with.
 * @param {HTMLCanvasElement} canvas @param {SceneDoc} doc
 */
export function mountScene(canvas, doc) {
  const context = canvas.getContext("webgl2", { antialias: true });
  if (!context) return null;
  const gl = context;
  const shaded = program(gl, SHADED_VS, SHADED_FS);
  const flat = program(gl, FLAT_VS, FLAT_FS);
  /** @type {WebGLBuffer[]} */
  const buffers = [];
  const vao = /** @type {WebGLVertexArrayObject} */ (gl.createVertexArray());

  /** @param {Float32Array | Uint16Array} data @param {number} [target] */
  const upload = (data, target = gl.ARRAY_BUFFER) => {
    const b = /** @type {WebGLBuffer} */ (gl.createBuffer());
    gl.bindBuffer(target, b);
    gl.bufferData(target, data, gl.STATIC_DRAW);
    buffers.push(b);
    return b;
  };
  /** @param {{positions: Float32Array, normals: Float32Array, indices?: Uint16Array}} g */
  const shape = (g) => ({ p: upload(g.positions), n: upload(g.normals), i: g.indices ? upload(g.indices, gl.ELEMENT_ARRAY_BUFFER) : null, count: g.indices ? g.indices.length : g.positions.length / 3 });
  const ball = shape(icosphere(2)), rod = shape(tube(18)), tip = shape(tube(18, true));

  // Lines, points and meshes are the scene's own: uploaded once, per frame.
  const frames = (doc.frames?.length ? doc.frames : [[]]).map((items) => items.map((item, i) => {
    if (item.kind === "points" || item.kind === "line") return { item, i, buf: upload(new Float32Array((item.xyz ?? []).flat())), count: (item.xyz ?? []).length };
    if (item.kind === "mesh") return { item, i, mesh: shape(faceted(item.vertices ?? [], item.faces ?? [])) };
    return { item, i };
  }));

  const { center, radius } = bounds(doc);
  const view = { yaw: 0.7, pitch: 0.45, distance: radius * 2.8, frame: 0 };
  let queued = false;

  /** @param {number} attribute @param {WebGLBuffer} buffer */
  const attrib = (attribute, buffer) => {
    if (attribute < 0) return;
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.enableVertexAttribArray(attribute);
    gl.vertexAttribPointer(attribute, 3, gl.FLOAT, false, 0, 0);
  };

  function draw() {
    queued = false;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = Math.max(1, Math.round(canvas.clientWidth * dpr)), h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
    gl.viewport(0, 0, w, h);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    const eye = [
      center[0] + view.distance * Math.cos(view.pitch) * Math.sin(view.yaw),
      center[1] + view.distance * Math.sin(view.pitch),
      center[2] + view.distance * Math.cos(view.pitch) * Math.cos(view.yaw),
    ];
    const proj = perspective(Math.PI / 4, w / h, view.distance / 100, view.distance * 10);
    const look = lookAt(eye, center);
    gl.bindVertexArray(vao);

    gl.useProgram(shaded);
    const u = (/** @type {string} */ name) => gl.getUniformLocation(shaded, name);
    gl.uniformMatrix4fv(u("view"), false, look);
    gl.uniformMatrix4fv(u("proj"), false, proj);
    gl.uniform3fv(u("eye"), eye);
    const [pos, nor] = [gl.getAttribLocation(shaded, "position"), gl.getAttribLocation(shaded, "normal")];
    /** @param {{p: WebGLBuffer, n: WebGLBuffer, i: WebGLBuffer | null, count: number}} s @param {number[]} model @param {number[]} rgba */
    const solid = (s, model, rgba) => {
      attrib(pos, s.p);
      attrib(nor, s.n);
      gl.uniformMatrix4fv(u("model"), false, model);
      gl.uniform4fv(u("color"), rgba);
      if (s.i) {
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, s.i);
        gl.drawElements(gl.TRIANGLES, s.count, gl.UNSIGNED_SHORT, 0);
      } else {
        gl.drawArrays(gl.TRIANGLES, 0, s.count);
      }
    };
    const items = frames[Math.min(view.frame, frames.length - 1)];
    for (const { item, i, mesh } of items) {
      const rgba = colour(item.color, i);
      switch (item.kind) {
        case "sphere": solid(ball, placed(item.center, item.radius ?? 1), rgba); break;
        case "bond": solid(rod, between(item.a, item.b, item.radius ?? 0.1), rgba); break;
        case "arrow": {
          const tipStart = item.origin.map((/** @type {number} */ o, /** @type {number} */ k) => o + item.vector[k] * 0.8);
          const end = item.origin.map((/** @type {number} */ o, /** @type {number} */ k) => o + item.vector[k]);
          const length = Math.hypot(item.vector[0], item.vector[1], item.vector[2]);
          solid(rod, between(item.origin, tipStart, length * 0.025), rgba);
          solid(tip, between(tipStart, end, length * 0.07), rgba);
          break;
        }
        case "mesh": if (mesh) solid(mesh, placed([0, 0, 0], 1), [rgba[0], rgba[1], rgba[2], item.opacity ?? 1]); break;
      }
    }

    gl.useProgram(flat);
    const f = (/** @type {string} */ name) => gl.getUniformLocation(flat, name);
    gl.uniformMatrix4fv(f("view"), false, look);
    gl.uniformMatrix4fv(f("proj"), false, proj);
    const at = gl.getAttribLocation(flat, "position");
    if (nor >= 0) gl.disableVertexAttribArray(nor);
    for (const { item, i, buf, count } of items) {
      if (!buf || !count) continue;
      attrib(at, buf);
      gl.uniform4fv(f("color"), colour(item.color, i));
      gl.uniform1f(f("size"), (item.size ?? 5) * dpr);
      gl.uniform1f(f("round"), item.kind === "points" ? 1 : 0);
      gl.drawArrays(item.kind === "points" ? gl.POINTS : gl.LINE_STRIP, 0, count);
    }
  }

  const redraw = () => {
    if (!queued) {
      queued = true;
      requestAnimationFrame(draw);
    }
  };

  // Dragged to turn; the wheel brings it nearer; a double-click puts it back.
  /** @type {{x: number, y: number} | null} */
  let drag = null;
  const down = (/** @type {PointerEvent} */ e) => { drag = { x: e.clientX, y: e.clientY }; canvas.setPointerCapture?.(e.pointerId); };
  const move = (/** @type {PointerEvent} */ e) => {
    if (!drag) return;
    view.yaw -= (e.clientX - drag.x) * 0.01;
    view.pitch = Math.max(-1.5, Math.min(1.5, view.pitch + (e.clientY - drag.y) * 0.01));
    drag = { x: e.clientX, y: e.clientY };
    redraw();
  };
  const up = () => { drag = null; };
  const wheel = (/** @type {WheelEvent} */ e) => {
    e.preventDefault();
    view.distance = Math.max(radius * 0.3, Math.min(radius * 20, view.distance * Math.exp(e.deltaY * 0.001)));
    redraw();
  };
  const reset = () => { Object.assign(view, { yaw: 0.7, pitch: 0.45, distance: radius * 2.8 }); redraw(); };
  canvas.addEventListener("pointerdown", down);
  canvas.addEventListener("pointermove", move);
  canvas.addEventListener("pointerup", up);
  canvas.addEventListener("wheel", wheel, { passive: false });
  canvas.addEventListener("dblclick", reset);
  const resize = new ResizeObserver(redraw);
  resize.observe(canvas);
  redraw();

  return {
    frames: frames.length,
    /** @param {number} i */
    show(i) { view.frame = Math.max(0, Math.min(frames.length - 1, i)); redraw(); },
    destroy() {
      resize.disconnect();
      canvas.removeEventListener("pointerdown", down);
      canvas.removeEventListener("pointermove", move);
      canvas.removeEventListener("pointerup", up);
      canvas.removeEventListener("wheel", wheel);
      canvas.removeEventListener("dblclick", reset);
      for (const b of buffers) gl.deleteBuffer(b);
      gl.deleteProgram(shaded);
      gl.deleteProgram(flat);
      gl.deleteVertexArray(vao);
    },
  };
}
