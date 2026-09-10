/* Modeling's renderer, on the GPU.
 *
 * ## Why this replaced the 2D canvas rather than joining it
 *
 * The canvas version said, in its own header and for four revisions running,
 * that the moment there was geometry to push it would be the wrong tool and
 * should be *replaced rather than extended*. This is that. Keeping both would
 * mean two renderers that can disagree about what a scene looks like, and the
 * only thing worse than a picture that is wrong is two pictures that are
 * differently wrong.
 *
 * Three things change, and they are the three the canvas could not do:
 *
 * - **A depth buffer.** A painter's sort is an ordering of whole triangles,
 *   so two that intersect, or three that overlap in a cycle, show through one
 *   another. Now they do not, because occlusion is decided per pixel.
 * - **The material runs per pixel.** A canvas fills flat polygons, so a
 *   material could only be sampled once per triangle and a noise was as fine
 *   as the mesh was. The graph the core compiles is turned into a fragment
 *   shader here, and runs at every pixel it covers.
 * - **Room.** The budget was ninety thousand triangles because that was where
 *   filling paths stopped holding thirty frames a second.
 *
 * ## The material is compiled twice, on purpose
 *
 * `proton-scene` turns the graph into instructions; this turns those
 * instructions into GLSL. Neither half guesses at the other: the core decides
 * what a graph *means* and this decides how a GPU runs it, which is the same
 * split every real pipeline has. A consequence worth stating: the noise here
 * is not bit-for-bit the noise the canvas computed, because it is a different
 * implementation of the same idea — as it would be on any two renderers.
 *
 * ## If there is no WebGL 2
 *
 * It says so, and draws nothing. There is no second renderer to fall back to,
 * by the choice above, and a viewport that quietly showed a worse picture
 * would be the thing this project keeps refusing.
 */

/* ── the context ─────────────────────────────────────────────────── */

let gl = null;
let why = "";

/** Take a canvas. Answers whether there is a GPU behind it. */
export function start(canvas) {
  if (gl) return true;
  try {
    gl = canvas.getContext("webgl2", {
      // Multisampling, which is what replaces the softness a canvas got for
      // free from filling one path at a time.
      antialias: true,
      depth: true,
      // The picture is redrawn from scratch every frame, so there is nothing
      // to preserve and saying so lets the driver skip a copy.
      preserveDrawingBuffer: false,
    });
  } catch (err) {
    why = String(err?.message ?? err);
  }
  if (!gl) {
    why = why || "this build of the webview has no WebGL 2";
    return false;
  }
  gl.enable(gl.DEPTH_TEST);
  gl.depthFunc(gl.LEQUAL);
  gl.frontFace(gl.CCW);
  return true;
}

/** Why there is no renderer, when there is not. */
export function problem() {
  return gl ? "" : why;
}

/** What is drawing, for the badge that says so. */
export function adapter() {
  if (!gl) return "";
  const info = gl.getExtension("WEBGL_debug_renderer_info");
  const said = info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : "";
  // `ANGLE (NVIDIA, NVIDIA GeForce RTX 4090 (0x1234) Direct3D11 ...)` is a
  // sentence nobody wants in a corner badge. The make and model is.
  const inside = said.match(/\(([^,]+),\s*([^(]+?)\s*(?:\(0x|Direct3D|OpenGL|Vulkan)/);
  return (inside ? inside[2] : said).trim();
}

/* ── shaders ─────────────────────────────────────────────────────── */

function build(kind, source) {
  const shader = gl.createShader(kind);
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const said = gl.getShaderInfoLog(shader);
    gl.deleteShader(shader);
    // With the source, numbered. A driver's line numbers are useless against
    // a program that was generated rather than written.
    const listing = source
      .split("\n")
      .map((line, at) => `${String(at + 1).padStart(3)} | ${line}`)
      .join("\n");
    throw new Error(`${said}\n${listing}`);
  }
  return shader;
}

function link(vertex, fragment) {
  const program = gl.createProgram();
  gl.attachShader(program, build(gl.VERTEX_SHADER, vertex));
  gl.attachShader(program, build(gl.FRAGMENT_SHADER, fragment));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(gl.getProgramInfoLog(program));
  }
  const where = {};
  const uniforms = gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS);
  for (let i = 0; i < uniforms; i++) {
    const name = gl.getActiveUniform(program, i).name;
    where[name] = gl.getUniformLocation(program, name);
  }
  return { program, where };
}

const SURFACE_VERTEX = `#version 300 es
in vec3 aPos;
uniform mat4 uModelView;
uniform mat4 uProj;
out vec3 vEye;
out vec3 vObj;
void main() {
  // Kept for the material, which is authored in the object's own space so a
  // pattern stays put on a thing that is moved.
  vObj = aPos;
  vec4 eye = uModelView * vec4(aPos, 1.0);
  vEye = eye.xyz;
  gl_Position = uProj * eye;
}`;

/* The part of every material's fragment shader that is not the material.
 *
 * The noise is the same construction the core's instruction means — a value
 * lattice, smoothed, in octaves — written for a GPU. The lighting is the terms
 * a metal/rough workflow is made of and no more: one light, no shadows, and a
 * flat grey room standing in for an environment, which is the difference
 * between a legible metal and a silhouette. */
const PRELUDE = `#version 300 es
precision highp float;

in vec3 vEye;
in vec3 vObj;
out vec4 fragment;

uniform vec3 uLight;
uniform vec3 uTint;
uniform float uPicked;

float hash(vec3 p) {
  p = fract(p * 0.3183099 + vec3(0.1, 0.2, 0.3));
  p *= 17.0;
  return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

float lattice(vec3 x) {
  vec3 i = floor(x);
  vec3 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(mix(hash(i + vec3(0,0,0)), hash(i + vec3(1,0,0)), f.x),
        mix(hash(i + vec3(0,1,0)), hash(i + vec3(1,1,0)), f.x), f.y),
    mix(mix(hash(i + vec3(0,0,1)), hash(i + vec3(1,0,1)), f.x),
        mix(hash(i + vec3(0,1,1)), hash(i + vec3(1,1,1)), f.x), f.y),
    f.z);
}

float fbm(vec3 p, int octaves) {
  float sum = 0.0;
  float amp = 0.5;
  float total = 0.0;
  for (int i = 0; i < 8; i++) {
    if (i >= octaves) break;
    sum += amp * lattice(p);
    total += amp;
    amp *= 0.5;
    p *= 2.0;
  }
  return total > 0.0 ? sum / total : 0.0;
}

float checker(vec3 p, float scale) {
  vec3 cell = floor(p * scale);
  return mod(cell.x + cell.y + cell.z, 2.0);
}

const float LIGHT = 3.2;
const float AMBIENT = 0.22;
const float ROOM = 0.5;

vec3 lit(vec3 base, float metal, float rough, vec3 n, vec3 v) {
  float ndl = max(dot(n, uLight), 0.0);
  float ndv = max(dot(n, v), 0.0);
  vec3 h = normalize(uLight + v);
  float ndh = max(dot(n, h), 0.0);
  float vdh = max(dot(v, h), 0.0);

  float a = rough * rough;
  float a2 = a * a;
  float d = ndh * ndh * (a2 - 1.0) + 1.0;
  float dist = a2 / (3.14159265 * d * d);
  float k = a * 0.5;
  float vis = 0.5 / (ndl * (ndv * (1.0 - k) + k) + ndv * (ndl * (1.0 - k) + k) + 1e-6);
  float spec = (ndl > 0.0 && ndv > 0.0) ? dist * vis : 0.0;

  vec3 f0 = mix(vec3(0.04), base, metal);
  vec3 f = f0 + (1.0 - f0) * pow(1.0 - vdh, 5.0);
  vec3 diffuse = base * (1.0 - metal) * (1.0 - f) / 3.14159265;
  vec3 direct = (diffuse + spec * f) * ndl * LIGHT;
  vec3 ambient = base * (1.0 - metal) * AMBIENT + f0 * ROOM;
  vec3 total = direct + ambient;
  // Reinhard, so a highlight rolls off instead of clipping to a flat disc.
  return sqrt(total / (1.0 + total));
}
`;

/** The GLSL a compiled material becomes.
 *
 * Mechanical, because the hard part was already done: the core emits its
 * instructions in dependency order, so each becomes one line that may only
 * name lines above it. */
function materialSource(shader) {
  const lines = [];
  const usable = shader && !shader.problem && shader.ops?.length;
  if (!usable) {
    // No material, or one the core could not compile. A plain grey that is
    // visibly plain; the panel is where the reason belongs.
    lines.push("  vec3 base = vec3(0.78);");
    lines.push("  float metal = 0.0;");
    lines.push("  float rough = 0.55;");
  } else {
    shader.ops.forEach((op, at) => {
      const r = `r${at}`;
      switch (op.op) {
        case "constant":
          lines.push(`  vec3 ${r} = vec3(${op.value.map(num).join(", ")});`);
          break;
        case "position":
          lines.push(`  vec3 ${r} = vObj;`);
          break;
        case "noise":
          lines.push(
            `  vec3 ${r} = vec3(fbm(r${op.at} * ${num(op.scale)}, ${octaves(op.detail)}));`,
          );
          break;
        case "checker":
          lines.push(`  vec3 ${r} = vec3(checker(r${op.at}, ${num(op.scale)}));`);
          break;
        case "mix":
          lines.push(
            `  vec3 ${r} = mix(r${op.a}, r${op.b}, clamp(r${op.fac}.x, 0.0, 1.0));`,
          );
          break;
        default:
          // An instruction from a later core. Grey, and not a guess.
          lines.push(`  vec3 ${r} = vec3(0.78);`);
      }
    });
    lines.push(`  vec3 base = r${shader.base};`);
    lines.push(`  float metal = clamp(r${shader.metallic}.x, 0.0, 1.0);`);
    lines.push(`  float rough = clamp(r${shader.roughness}.x, 0.03, 1.0);`);
  }

  return `${PRELUDE}
void main() {
${lines.join("\n")}
  // The face's normal, from how the view-space position changes across the
  // triangle. Exact, needs nothing in the vertex buffer, and faceted — which
  // is honest until the loader produces vertex normals.
  vec3 n = normalize(cross(dFdx(vEye), dFdy(vEye)));
  vec3 v = normalize(-vEye);
  if (dot(n, v) < 0.0) n = -n;
  vec3 colour = lit(mix(base, uTint, uPicked * 0.45), metal, rough, n, v);
  fragment = vec4(colour, 1.0);
}`;
}

/** A float GLSL will accept: `1` is an int there and `1.0` is not. */
function num(value) {
  const n = Number(value);
  return Number.isFinite(n) ? (Number.isInteger(n) ? `${n}.0` : `${n}`) : "0.0";
}

function octaves(detail) {
  return Math.max(1, Math.min(8, Math.round(Number(detail) || 1)));
}

/* Programs, by the material they came from. */
const programs = new Map();

function programFor(shader) {
  const key = shader && !shader.problem && shader.ops?.length ? shader.stamp : "";
  const had = programs.get(key);
  if (had) return had;
  let made;
  try {
    made = link(SURFACE_VERTEX, materialSource(shader));
  } catch (err) {
    // A material that will not compile falls back to the plain grey one rather
    // than taking the whole viewport down with it.
    console.error("material would not compile:", err.message);
    made = programs.get("") ?? link(SURFACE_VERTEX, materialSource(null));
    programs.set("", made);
  }
  programs.set(key, made);
  return made;
}

const LINE_VERTEX = `#version 300 es
in vec3 aPos;
uniform mat4 uModelViewProj;
void main() { gl_Position = uModelViewProj * vec4(aPos, 1.0); }`;

const LINE_FRAGMENT = `#version 300 es
precision highp float;
uniform vec4 uColour;
out vec4 fragment;
void main() {
  // Premultiplied, which is what a WebGL canvas composites with. Writing the
  // colour straight makes a grid whose token says thirteen per cent alpha
  // come out as solid white over everything behind it.
  fragment = vec4(uColour.rgb * uColour.a, uColour.a);
}`;

let lineProgram = null;
let lineBuffer = null;
let lineArray = null;

/* ── meshes ──────────────────────────────────────────────────────── */

const uploaded = new Map();

/** Put one mesh on the GPU, or hand back the one already there.
 *
 * Keyed by the same string the core keys geometry by, and stamped with the
 * delivery serial: a key does not move when a modifier's setting changes, but
 * the mesh under it does.
 */
function meshFor(key, mesh) {
  const had = uploaded.get(key);
  if (had && had.serial === mesh.serial) return had;
  if (had) {
    gl.deleteVertexArray(had.array);
    gl.deleteBuffer(had.positions);
    gl.deleteBuffer(had.indices);
  }

  const array = gl.createVertexArray();
  gl.bindVertexArray(array);
  const positions = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, positions);
  gl.bufferData(gl.ARRAY_BUFFER, mesh.positions, gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
  const indices = gl.createBuffer();
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indices);
  gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, mesh.triangles, gl.STATIC_DRAW);
  gl.bindVertexArray(null);

  const made = {
    array,
    positions,
    indices,
    count: mesh.triangles.length,
    closed: mesh.closed,
    serial: mesh.serial,
  };
  uploaded.set(key, made);
  return made;
}

/** Forget a mesh nothing refers to any more. */
export function forget(key) {
  const had = uploaded.get(key);
  if (!had) return;
  gl.deleteVertexArray(had.array);
  gl.deleteBuffer(had.positions);
  gl.deleteBuffer(had.indices);
  uploaded.delete(key);
}

/* ── the frame ───────────────────────────────────────────────────── */

/** Draw one. Answers how many triangles went to the GPU.
 *
 * Not how many were drawn: with a depth buffer, how much of a triangle
 * survives is decided per pixel and there is no honest count of "drawn" to
 * report any more. What this returns is what the scene is made of.
 */
export function frame(scene) {
  const {
    canvas,
    nodes,
    matrices,
    geometry,
    shaders,
    view,
    proj,
    light,
    selected,
    tint,
    lines,
    ratio,
  } = scene;

  const width = Math.round(canvas.clientWidth * ratio);
  const height = Math.round(canvas.clientHeight * ratio);
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  gl.viewport(0, 0, width, height);
  // Transparent, so the pane's own background shows through and the viewport
  // stays part of the theme rather than a grey rectangle inside it.
  gl.clearColor(0, 0, 0, 0);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  gl.enable(gl.DEPTH_TEST);

  let triangles = 0;
  for (const [i, node] of nodes.entries()) {
    const mesh = node.geometry ? geometry.get(node.geometry) : null;
    if (!mesh?.positions) continue;
    const held = meshFor(node.geometry, mesh);
    const shader = node.material ? shaders.get(node.material) : null;
    const { program, where } = programFor(shader);

    gl.useProgram(program);
    gl.uniformMatrix4fv(where.uModelView, false, multiply(view, matrices[i]));
    gl.uniformMatrix4fv(where.uProj, false, proj);
    gl.uniform3fv(where.uLight, light);
    gl.uniform3fv(where.uTint, tint);
    gl.uniform1f(where.uPicked, i === selected ? 1 : 0);

    // Back faces are thrown away only for a solid. On a sheet there is nothing
    // behind the surface to hide them, and culling would make a wall vanish
    // the moment you orbited past its edge.
    if (held.closed) {
      gl.enable(gl.CULL_FACE);
      gl.cullFace(gl.BACK);
    } else {
      gl.disable(gl.CULL_FACE);
    }

    gl.bindVertexArray(held.array);
    gl.drawElements(gl.TRIANGLES, held.count, gl.UNSIGNED_INT, 0);
    triangles += held.count / 3;
  }
  gl.bindVertexArray(null);
  gl.disable(gl.CULL_FACE);

  drawLines(lines, proj, view);
  return triangles;
}

/** The grid, the axes and the proxies, as coloured line segments.
 *
 * Depth-tested, which the canvas version could not be: a proxy for a thing
 * with no surface now goes behind the things that have one, and the floor grid
 * goes behind everything standing on it.
 */
function drawLines(batches, proj, view) {
  if (!batches?.length) return;
  if (!lineProgram) {
    lineProgram = link(LINE_VERTEX, LINE_FRAGMENT);
    lineArray = gl.createVertexArray();
    lineBuffer = gl.createBuffer();
    gl.bindVertexArray(lineArray);
    gl.bindBuffer(gl.ARRAY_BUFFER, lineBuffer);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
    gl.bindVertexArray(null);
  }

  // Blended, so a faint grid is faint — and depth-*tested* but not
  // depth-*writing*, which is the rule for anything you can see through: the
  // floor is occluded by what stands on it without occluding what is behind.
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  gl.depthMask(false);

  gl.useProgram(lineProgram.program);
  gl.uniformMatrix4fv(lineProgram.where.uModelViewProj, false, multiply(proj, view));
  gl.bindVertexArray(lineArray);
  gl.bindBuffer(gl.ARRAY_BUFFER, lineBuffer);
  for (const batch of batches) {
    if (!batch.points.length) continue;
    gl.bufferData(gl.ARRAY_BUFFER, batch.points, gl.DYNAMIC_DRAW);
    gl.uniform4fv(lineProgram.where.uColour, batch.colour);
    gl.drawArrays(gl.LINES, 0, batch.points.length / 3);
  }
  gl.bindVertexArray(null);
  gl.depthMask(true);
  gl.disable(gl.BLEND);
}

/** Column-major 4x4 product, the convention WebGL wants. */
function multiply(a, b) {
  const out = new Float32Array(16);
  for (let col = 0; col < 4; col++) {
    for (let row = 0; row < 4; row++) {
      let sum = 0;
      for (let k = 0; k < 4; k++) sum += a[k * 4 + row] * b[col * 4 + k];
      out[col * 4 + row] = sum;
    }
  }
  return out;
}
