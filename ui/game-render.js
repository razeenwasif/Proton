/* Game mode's renderer: the scene's list of sprites drawn with WebGL 2.
 *
 * The terminal's GPU renderer (term-gl.js) is the model: one context per
 * canvas, one unit quad drawn instanced — sixteen floats an instance — with
 * premultiplied alpha, and the context's loss handled rather than crashed on.
 * What is new is what a game's art needs:
 *
 * - each image its own texture, mipmapped so a painted backdrop zoomed out is
 *   not a shimmer, or sampled nearest for pixel art;
 * - a haze per plane — distance's colour mixed in by an amount — done in the
 *   shader, so a hundred planes cost nothing more than their pixels;
 * - the blend a plane or an item asks for, drawn in runs of one texture and
 *   one blend, in the order the scene gives;
 * - a skeleton's parts as triangles, each point carried by up to four bones
 *   in the vertex shader — its pose's bones, two vec4s a bone, as uniforms —
 *   the parts' buffers made once a skeleton and kept;
 * - light: a lit plane's art is the room's ambient colour plus each light's,
 *   falling off to nothing at its reach — shaded by the angle the light meets
 *   it at where the art has a normal map, a second texture of the same size —
 *   at most sixteen lights, in the fragment shader;
 * - afterwards, when asked: the picture drawn into a texture, its brightest
 *   part blurred small and laid back over it (bloom), its colours looked up
 *   in a grade (a strip of a 3D table, the usual layout), and its corners
 *   darkened (vignette).
 *
 * The editor and Play draw with this: the sprites differ, the renderer does
 * not. */

/** @typedef {import('./game-scene.js').Sprite} Sprite */
/** @typedef {import('./game-overlay.js').View} View */
/** @typedef {import('./bindings/GameSkel').GameSkel} Skel */

import { boneUniforms } from "./game-skel.js";
import { GLOW } from "./game-scene.js";

/** The most lights a frame is lit by. */
export const LIGHTS = 16;

/** How a lit fragment is lit: the ambient colour, and each light's colour
 *  times its fall-off and the angle it meets the surface at — straight on,
 *  where there is no normal map. */
const LIGHTING = `
uniform vec3 u_ambient;
uniform int u_count;
uniform vec4 u_lights[${LIGHTS * 2}];   // x, y, reach, height; r, g, b (times intensity)
vec3 lit(vec2 room, vec3 normal, bool shaded) {
  vec3 light = u_ambient;
  for (int i = 0; i < ${LIGHTS}; i++) {
    if (i >= u_count) break;
    vec4 at = u_lights[i * 2];
    vec4 hue = u_lights[i * 2 + 1];
    vec2 d = at.xy - room;
    float fall = clamp(1.0 - length(d) / max(at.z, 1.0), 0.0, 1.0);
    fall *= fall;
    // The map's green is up, the room's y down.
    float facing = shaded ? max(dot(normal, normalize(vec3(d.x, -d.y, at.w))), 0.0) : 1.0;
    light += hue.rgb * fall * facing;
  }
  return min(light, vec3(4.0));
}`;

/** The most bones the skinning shader holds (proton-game's `import::skel::BONES`). */
const BONES = 64;
/** Floats a skinned point: four places, four bones, four weights, its uv. */
const POINT = 18;

const VERTEX_SKIN = `#version 300 es
layout(location = 0) in vec4 a_p01;    // the point on its first bone and its second
layout(location = 1) in vec4 a_p23;    // on its third and fourth
layout(location = 2) in vec4 a_bones;
layout(location = 3) in vec4 a_weights;
layout(location = 4) in vec2 a_uv;     // atlas pixels
uniform vec4 u_bones[${BONES * 2}];
uniform vec2 u_size;
uniform vec3 u_view;
uniform vec4 u_place;   // the feet in room pixels, 1 or -1 for flipped
uniform vec2 u_pivot;   // the feet in the skeleton's pixels
uniform vec2 u_atlas;
out vec2 v_uv;
out vec2 v_room;
vec2 carried(vec2 p, float bone) {
  int i = int(bone) * 2;
  vec4 m = u_bones[i];
  vec4 t = u_bones[i + 1];
  return vec2(m.x * p.x + m.z * p.y + t.x, m.y * p.x + m.w * p.y + t.y);
}
void main() {
  vec2 p = carried(a_p01.xy, a_bones.x) * a_weights.x + carried(a_p01.zw, a_bones.y) * a_weights.y
         + carried(a_p23.xy, a_bones.z) * a_weights.z + carried(a_p23.zw, a_bones.w) * a_weights.w;
  vec2 local = p - u_pivot;
  vec2 room = u_place.xy + vec2(local.x * u_place.z, local.y);
  vec2 stage = room * u_view.x + u_view.yz;
  gl_Position = vec4(stage / u_size * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
  v_uv = a_uv / u_atlas;
  v_room = room;
}`;

const FRAGMENT_SKIN = `#version 300 es
precision mediump float;
uniform sampler2D u_tex;
uniform vec4 u_color;   // the slot's: r, g, b, alpha
uniform vec4 u_tint;
uniform float u_opacity;
uniform float u_lit;
in vec2 v_uv;
in vec2 v_room;
out vec4 color;
${LIGHTING}
void main() {
  vec4 texel = texture(u_tex, v_uv);   // premultiplied
  vec3 rgb = texel.rgb * u_color.rgb;
  if (u_lit > 0.5) rgb *= lit(v_room, vec3(0.0, 0.0, 1.0), false);
  vec3 hazed = mix(rgb, u_tint.rgb * texel.a, u_tint.a);
  color = vec4(hazed, texel.a) * u_color.a * u_opacity;
}`;

const FLOATS = 16;

const VERTEX = `#version 300 es
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_rect;   // x, y, w, h — room pixels
layout(location = 2) in vec4 a_uv;     // u0, v0, u1, v1
layout(location = 3) in vec4 a_tint;   // r, g, b, amount
layout(location = 4) in vec4 a_misc;   // opacity, turn (radians), lit, flipped
uniform vec2 u_size;    // the canvas, in stage pixels
uniform vec3 u_view;    // stage pixels a room pixel, and where room 0,0 is
out vec2 v_uv;
out vec4 v_tint;
out float v_opacity;
out vec2 v_room;
out vec4 v_turn;        // cos, sin, lit, 1 or -1 flipped
void main() {
  vec2 half_size = a_rect.zw * 0.5;
  vec2 local = (a_corner - 0.5) * a_rect.zw;
  float c = cos(a_misc.y), s = sin(a_misc.y);
  vec2 room = a_rect.xy + half_size + vec2(local.x * c - local.y * s, local.x * s + local.y * c);
  vec2 stage = room * u_view.x + u_view.yz;
  gl_Position = vec4(stage / u_size * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
  v_uv = mix(a_uv.xy, a_uv.zw, a_corner);
  v_tint = a_tint;
  v_opacity = a_misc.x;
  v_room = room;
  v_turn = vec4(c, s, a_misc.z, a_misc.w > 0.5 ? -1.0 : 1.0);
}`;

const FRAGMENT = `#version 300 es
precision highp float;
uniform sampler2D u_tex;
uniform sampler2D u_normal;
uniform float u_mapped;   // whether u_normal is the art's own normal map
in vec2 v_uv;
in vec4 v_tint;
in float v_opacity;
in vec2 v_room;
in vec4 v_turn;
out vec4 color;
${LIGHTING}
void main() {
  vec4 texel = texture(u_tex, v_uv);   // premultiplied
  vec3 rgb = texel.rgb;
  if (v_turn.z > 0.5) {
    // The map's normal, flipped and turned with the art.
    vec3 n = texture(u_normal, v_uv).xyz * 2.0 - 1.0;
    n.x *= v_turn.w;
    n.xy = vec2(n.x * v_turn.x + n.y * v_turn.y, -n.x * v_turn.y + n.y * v_turn.x);
    rgb *= lit(v_room, normalize(n), u_mapped > 0.5);
  }
  vec3 hazed = mix(rgb, v_tint.rgb * texel.a, v_tint.a);
  color = vec4(hazed, texel.a) * v_opacity;
}`;

/** A triangle over the whole screen, for the passes after the picture. */
const FULL = `#version 300 es
out vec2 v_uv;
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  v_uv = p;
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

/** What of the picture is bright enough to bloom: past the threshold, eased in. */
const BRIGHT = `#version 300 es
precision mediump float;
uniform sampler2D u_tex;
in vec2 v_uv;
out vec4 color;
void main() {
  vec3 c = texture(u_tex, v_uv).rgb;
  float b = max(c.r, max(c.g, c.b));
  float knee = clamp((b - 0.6) / 0.4, 0.0, 1.0);
  color = vec4(c * knee * knee, 1.0);
}`;

/** One way of a Gaussian blur, nine taps. */
const BLUR = `#version 300 es
precision mediump float;
uniform sampler2D u_tex;
uniform vec2 u_step;
in vec2 v_uv;
out vec4 color;
void main() {
  float w[5] = float[](0.2270270, 0.1945946, 0.1216216, 0.0540541, 0.0162162);
  vec3 c = texture(u_tex, v_uv).rgb * w[0];
  for (int i = 1; i < 5; i++) {
    c += texture(u_tex, v_uv + u_step * float(i)).rgb * w[i];
    c += texture(u_tex, v_uv - u_step * float(i)).rgb * w[i];
  }
  color = vec4(c, 1.0);
}`;

/** The picture, its bloom laid over it, graded and vignetted. */
const COMPOSITE = `#version 300 es
precision highp float;
uniform sampler2D u_tex;
uniform sampler2D u_bloom;
uniform sampler2D u_grade;
uniform float u_amount;     // bloom
uniform float u_vignette;
uniform float u_lut;        // the grade's size a side; 0 for none
in vec2 v_uv;
out vec4 color;
vec3 graded(vec3 c) {
  float n = u_lut;
  float b = clamp(c.b, 0.0, 1.0) * (n - 1.0);
  float lo = floor(b), hi = min(lo + 1.0, n - 1.0);
  vec2 at = vec2(clamp(c.r, 0.0, 1.0) * (n - 1.0) + 0.5, clamp(c.g, 0.0, 1.0) * (n - 1.0) + 0.5);
  vec3 a = texture(u_grade, vec2((lo * n + at.x) / (n * n), at.y / n)).rgb;
  vec3 d = texture(u_grade, vec2((hi * n + at.x) / (n * n), at.y / n)).rgb;
  return mix(a, d, b - lo);
}
void main() {
  vec3 c = texture(u_tex, v_uv).rgb + texture(u_bloom, v_uv).rgb * u_amount;
  if (u_lut > 0.5) c = graded(c);
  float r = length(v_uv - 0.5) * 1.4142;
  c *= 1.0 - u_vignette * smoothstep(0.35, 1.0, r);
  color = vec4(c, 1.0);
}`;

/** How a frame is lit and finished: the ambient colour and the lights (in
 *  room pixels, 0–1 colours times intensity), and what is done afterwards.
 *  @typedef {{ambient: number[], lights: {x: number, y: number, radius: number, height: number, color: number[]}[],
 *             post?: {bloom: number, vignette: number, grade?: string | null, quality: "low" | "medium" | "high"} | null}} Lighting */

/** @param {WebGL2RenderingContext} gl @param {string} vertex @param {string} fragment */
function program(gl, vertex, fragment) {
  const compiled = (/** @type {number} */ type, /** @type {string} */ source) => {
    const shader = gl.createShader(type);
    if (!shader) throw new Error("no shader");
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) ?? "shader");
    return shader;
  };
  const linked = gl.createProgram();
  gl.attachShader(linked, compiled(gl.VERTEX_SHADER, vertex));
  gl.attachShader(linked, compiled(gl.FRAGMENT_SHADER, fragment));
  gl.linkProgram(linked);
  if (!gl.getProgramParameter(linked, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(linked) ?? "program");
  return linked;
}

/**
 * A renderer over `canvas`, or null where WebGL 2 cannot be had — the stage
 * then shows the overlay alone, which is still the room.
 * @param {HTMLCanvasElement} canvas
 * @param {{onLost?: () => void}} [hooks]
 */
export function createGameRender(canvas, hooks = {}) {
  const context = canvas.getContext("webgl2", { alpha: false, premultipliedAlpha: true, antialias: true, preserveDrawingBuffer: false });
  if (!context) return null;
  const gl = context;
  let lost = false;
  canvas.addEventListener("webglcontextlost", (e) => {
    e.preventDefault();
    lost = true;
    hooks.onLost?.();
  });

  const linked = program(gl, VERTEX, FRAGMENT);
  const skinned = program(gl, VERTEX_SKIN, FRAGMENT_SKIN);
  const skinWhere = Object.fromEntries(["u_bones", "u_size", "u_view", "u_place", "u_pivot", "u_atlas", "u_tex", "u_color", "u_tint", "u_opacity"].map((n) => [n, gl.getUniformLocation(skinned, n)]));
  /** Each skeleton's parts on the GPU: by slot, by part. @type {WeakMap<Skel, {vao: WebGLVertexArrayObject | null, count: number}[][]>} */
  const parts = new WeakMap();
  /** @type {(WebGLBuffer | null)[]} */
  const partBuffers = [];
  const where = {
    size: gl.getUniformLocation(linked, "u_size"),
    view: gl.getUniformLocation(linked, "u_view"),
    tex: gl.getUniformLocation(linked, "u_tex"),
    normal: gl.getUniformLocation(linked, "u_normal"),
    mapped: gl.getUniformLocation(linked, "u_mapped"),
  };
  /** The light uniforms of a program. @param {WebGLProgram} p */
  const lightsOf = (p) => ({ ambient: gl.getUniformLocation(p, "u_ambient"), count: gl.getUniformLocation(p, "u_count"), lights: gl.getUniformLocation(p, "u_lights") });
  const lighting = { sprites: lightsOf(linked), skins: lightsOf(skinned) };
  skinWhere.u_lit = gl.getUniformLocation(skinned, "u_lit");
  const bright = program(gl, FULL, BRIGHT);
  const blur = program(gl, FULL, BLUR);
  const composite = program(gl, FULL, COMPOSITE);
  const empty = gl.createVertexArray();
  /** The frame's lights, packed for the shaders. */
  let packed = { ambient: [1, 1, 1], count: 0, lights: new Float32Array(LIGHTS * 8) };
  /** @param {{ambient: WebGLUniformLocation | null, count: WebGLUniformLocation | null, lights: WebGLUniformLocation | null}} u */
  const setLights = (u) => {
    gl.uniform3fv(u.ambient, packed.ambient);
    gl.uniform1i(u.count, packed.count);
    gl.uniform4fv(u.lights, packed.lights);
  };

  /** A texture of given pixels, sampled smooth, clamped. @param {number} w @param {number} h @param {Uint8Array} px */
  function made(w, h, px) {
    const t = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, px);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return t;
  }
  // A surface facing straight out, for art with no normal map.
  const flat = made(1, 1, new Uint8Array([128, 128, 255, 255]));
  // A light's glow: white, falling off from the middle, premultiplied.
  const glow = (() => {
    const n = 64;
    const px = new Uint8Array(n * n * 4);
    for (let y = 0; y < n; y++) {
      for (let x = 0; x < n; x++) {
        const r = Math.hypot(x + 0.5 - n / 2, y + 0.5 - n / 2) / (n / 2);
        const a = Math.round(255 * Math.max(0, 1 - r) ** 2);
        px.set([a, a, a, a], (y * n + x) * 4);
      }
    }
    return made(n, n, px);
  })();

  /** Render targets for the passes after the picture: the picture itself,
   *  and two smaller ones to blur its bright part between. */
  /** @type {{w: number, h: number, scale: number, scene: {fb: WebGLFramebuffer | null, tex: WebGLTexture | null}, a: {fb: WebGLFramebuffer | null, tex: WebGLTexture | null}, b: {fb: WebGLFramebuffer | null, tex: WebGLTexture | null}} | null} */
  let targets = null;
  /** @param {number} w @param {number} h */
  function target(w, h) {
    const tex = made(w, h, /** @type {any} */ (null));
    const fb = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return { fb, tex };
  }
  /** @param {number} w @param {number} h @param {number} scale */
  function targetsFor(w, h, scale) {
    if (targets && targets.w === w && targets.h === h && targets.scale === scale) return targets;
    if (targets) {
      for (const t of [targets.scene, targets.a, targets.b]) {
        gl.deleteFramebuffer(t.fb);
        gl.deleteTexture(t.tex);
      }
    }
    const [bw, bh] = [Math.max(1, Math.round(w / scale)), Math.max(1, Math.round(h / scale))];
    targets = { w, h, scale, scene: target(w, h), a: target(bw, bh), b: target(bw, bh) };
    return targets;
  }
  const vao = gl.createVertexArray();
  gl.bindVertexArray(vao);
  const quad = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 8, 0);
  const instances = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, instances);
  for (let i = 0; i < 4; i++) {
    gl.enableVertexAttribArray(1 + i);
    gl.vertexAttribPointer(1 + i, 4, gl.FLOAT, false, FLOATS * 4, i * 16);
    gl.vertexAttribDivisor(1 + i, 1);
  }
  gl.bindVertexArray(null);
  let data = new Float32Array(FLOATS * 256);

  /** @type {Map<string, {texture: WebGLTexture, width: number, height: number}>} */
  const textures = new Map();

  /** A skeleton's parts as vertex arrays, made the first time it is drawn.
   *  @param {Skel} model */
  function partsOf(model) {
    const made = parts.get(model);
    if (made) return made;
    const out = model.slots.map((slot) =>
      slot.parts.map((mesh) => {
        if (!mesh.points.length || !mesh.triangles.length) return { vao: null, count: 0 };
        const data = new Float32Array(mesh.points.length * POINT);
        mesh.points.forEach((p, i) => {
          const o = i * POINT;
          p.bones.slice(0, 4).forEach(([bone, weight, at], k) => {
            data[o + k * 2] = at[0];
            data[o + k * 2 + 1] = at[1];
            data[o + 8 + k] = bone;
            data[o + 12 + k] = weight;
          });
          data[o + 16] = p.uv[0];
          data[o + 17] = p.uv[1];
        });
        const vao = gl.createVertexArray();
        gl.bindVertexArray(vao);
        const vertices = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, vertices);
        gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
        const layout = /** @type {[number, number, number][]} */ ([[0, 4, 0], [1, 4, 4], [2, 4, 8], [3, 4, 12], [4, 2, 16]]);
        for (const [at, size, offset] of layout) {
          gl.enableVertexAttribArray(at);
          gl.vertexAttribPointer(at, size, gl.FLOAT, false, POINT * 4, offset * 4);
        }
        const indices = gl.createBuffer();
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indices);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint32Array(mesh.triangles.flat()), gl.STATIC_DRAW);
        gl.bindVertexArray(null);
        partBuffers.push(vertices, indices);
        return { vao, count: mesh.triangles.length * 3 };
      }),
    );
    parts.set(model, out);
    return out;
  }

  /** A skeleton, posed, standing where its sprite says: each slot's part
   *  shown, back to front, in its colour.
   *  @param {Sprite} s @param {View} view @param {number} width @param {number} height */
  function drawSkeleton(s, view, width, height) {
    const k = s.skel;
    const texture = textures.get(s.key);
    if (!k || !texture) return;
    gl.useProgram(skinned);
    gl.uniform4fv(skinWhere.u_bones, boneUniforms(k.posed));
    gl.uniform2f(skinWhere.u_size, width, height);
    gl.uniform3f(skinWhere.u_view, view.scale, view.x, view.y);
    gl.uniform4f(skinWhere.u_place, k.at[0], k.at[1], s.flip ? -1 : 1, 0);
    gl.uniform2f(skinWhere.u_pivot, k.pivot[0], k.pivot[1]);
    gl.uniform2f(skinWhere.u_atlas, Math.max(1, texture.width), Math.max(1, texture.height));
    gl.uniform1i(skinWhere.u_tex, 0);
    gl.uniform4fv(skinWhere.u_tint, s.tint);
    gl.uniform1f(skinWhere.u_opacity, s.opacity);
    gl.uniform1f(skinWhere.u_lit, s.lit ? 1 : 0);
    setLights(lighting.skins);
    gl.bindTexture(gl.TEXTURE_2D, texture.texture);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    const meshes = partsOf(k.model);
    k.model.slots.forEach((_, i) => {
      const part = meshes[i]?.[k.posed.shown[i]];
      if (!part?.vao) return;
      const c = k.posed.colors;
      gl.uniform4f(skinWhere.u_color, c[i * 4], c[i * 4 + 1], c[i * 4 + 2], c[i * 4 + 3]);
      gl.bindVertexArray(part.vao);
      gl.drawElements(gl.TRIANGLES, part.count, gl.UNSIGNED_INT, 0);
    });
    gl.bindVertexArray(null);
  }

  /** The sprites, in runs of one texture, one blend and one normal map, in
   *  the scene's order; a skeleton drawn alone, by its own program.
   *  @param {Sprite[]} sprites @param {View} view @param {number} width @param {number} height */
  function drawSprites(sprites, view, width, height) {
    if (data.length < sprites.length * FLOATS) data = new Float32Array(sprites.length * FLOATS * 2);
    sprites.forEach((s, i) => {
      const o = i * FLOATS;
      data[o] = s.x;
      data[o + 1] = s.y;
      data[o + 2] = s.w;
      data[o + 3] = s.h;
      // The part of its texture it is — a frame of a sheet — flipped by
      // swapping its sides.
      const uv = s.uv ?? [0, 0, 1, 1];
      data[o + 4] = s.flip ? uv[2] : uv[0];
      data[o + 5] = uv[1];
      data[o + 6] = s.flip ? uv[0] : uv[2];
      data[o + 7] = uv[3];
      data.set(s.tint, o + 8);
      data[o + 12] = s.opacity;
      data[o + 13] = (s.rot * Math.PI) / 180;
      data[o + 14] = s.lit ? 1 : 0;
      data[o + 15] = s.flip ? 1 : 0;
    });
    gl.useProgram(linked);
    gl.uniform2f(where.size, width, height);
    gl.uniform3f(where.view, view.scale, view.x, view.y);
    gl.uniform1i(where.tex, 0);
    gl.uniform1i(where.normal, 1);
    setLights(lighting.sprites);
    gl.activeTexture(gl.TEXTURE0);
    gl.enable(gl.BLEND);
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, instances);
    gl.bufferData(gl.ARRAY_BUFFER, data.subarray(0, sprites.length * FLOATS), gl.STREAM_DRAW);
    // In runs of one texture and one blend, in the scene's order; a
    // skeleton drawn alone, by its own program.
    let start = 0;
    while (start < sprites.length) {
      const first = sprites[start];
      if (first.skel) {
        drawSkeleton(first, view, width, height);
        gl.useProgram(linked);
        gl.bindVertexArray(vao);
        gl.bindBuffer(gl.ARRAY_BUFFER, instances);
        start += 1;
        continue;
      }
      let end = start + 1;
      while (end < sprites.length && sprites[end].key === first.key && sprites[end].blend === first.blend && sprites[end].normal === first.normal && !sprites[end].skel) end++;
      const texture = first.key === GLOW ? { texture: glow } : textures.get(first.key);
      if (texture) {
        const [src, dst] = blendFor(first.blend);
        gl.blendFunc(src, dst);
        gl.bindTexture(gl.TEXTURE_2D, texture.texture);
        // Its normal map, or a surface facing straight out.
        const normal = first.normal ? textures.get(first.normal) : null;
        gl.activeTexture(gl.TEXTURE1);
        gl.bindTexture(gl.TEXTURE_2D, normal?.texture ?? flat);
        gl.activeTexture(gl.TEXTURE0);
        gl.uniform1f(where.mapped, normal ? 1 : 0);
        // Instances from `start`: the attributes are pointed there.
        for (let i = 0; i < 4; i++) gl.vertexAttribPointer(1 + i, 4, gl.FLOAT, false, FLOATS * 4, start * FLOATS * 4 + i * 16);
        gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, end - start);
      }
      start = end;
    }
    gl.bindVertexArray(null);
  }

  /** The passes after the picture: its bright part blurred small and laid
   *  back over it, graded, vignetted — onto the screen.
   *  @param {NonNullable<typeof targets>} t @param {NonNullable<Lighting["post"]>} post */
  function finish(t, post) {
    gl.disable(gl.BLEND);
    gl.bindVertexArray(empty);
    const pass = (/** @type {WebGLProgram} */ p, /** @type {WebGLFramebuffer | null} */ fb, /** @type {number} */ w, /** @type {number} */ h, /** @type {WebGLTexture | null} */ from) => {
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
      gl.viewport(0, 0, w, h);
      gl.useProgram(p);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, from);
      gl.uniform1i(gl.getUniformLocation(p, "u_tex"), 0);
    };
    const [bw, bh] = [Math.max(1, Math.round(t.w / t.scale)), Math.max(1, Math.round(t.h / t.scale))];
    if (post.bloom > 0) {
      pass(bright, t.a.fb, bw, bh, t.scene.tex);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      pass(blur, t.b.fb, bw, bh, t.a.tex);
      gl.uniform2f(gl.getUniformLocation(blur, "u_step"), 1 / bw, 0);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      pass(blur, t.a.fb, bw, bh, t.b.tex);
      gl.uniform2f(gl.getUniformLocation(blur, "u_step"), 0, 1 / bh);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    }
    pass(composite, null, t.w, t.h, t.scene.tex);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, t.a.tex);
    gl.uniform1i(gl.getUniformLocation(composite, "u_bloom"), 1);
    const grade = post.grade ? textures.get(post.grade) : null;
    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, grade?.texture ?? flat);
    gl.uniform1i(gl.getUniformLocation(composite, "u_grade"), 2);
    // A grade is n² wide and n tall.
    gl.uniform1f(gl.getUniformLocation(composite, "u_lut"), grade && grade.width === grade.height * grade.height ? grade.height : 0);
    gl.uniform1f(gl.getUniformLocation(composite, "u_amount"), post.bloom > 0 ? post.bloom : 0);
    gl.uniform1f(gl.getUniformLocation(composite, "u_vignette"), post.vignette);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindVertexArray(null);
    gl.enable(gl.BLEND);
  }

  /** Blend factors for a sprite's blend, over premultiplied colour. @param {string} blend */
  function blendFor(blend) {
    switch (blend) {
      case "add": return [gl.ONE, gl.ONE];
      case "multiply": return [gl.DST_COLOR, gl.ONE_MINUS_SRC_ALPHA];
      case "screen": return [gl.ONE, gl.ONE_MINUS_SRC_COLOR];
      default: return [gl.ONE, gl.ONE_MINUS_SRC_ALPHA];
    }
  }

  return {
    /** Whether an image is on the GPU. @param {string} key */
    has(key) {
      return textures.has(key);
    },

    /** Put an image on the GPU under `key`, sampled smooth or nearest — or,
     *  `linear`, smooth with no smaller copies, as a grade's table wants.
     *  @param {string} key @param {TexImageSource} image @param {"smooth" | "nearest" | "linear"} filter */
    upload(key, image, filter) {
      if (lost) return;
      const old = textures.get(key);
      if (old) gl.deleteTexture(old.texture);
      const texture = gl.createTexture();
      if (!texture) return;
      gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      const nearest = filter === "nearest";
      // A grade's table: smooth between its cells, and no smaller copies.
      const plain = filter === "linear";
      if (!nearest && !plain) gl.generateMipmap(gl.TEXTURE_2D);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, nearest ? gl.NEAREST : plain ? gl.LINEAR : gl.LINEAR_MIPMAP_LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, nearest ? gl.NEAREST : gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      const width = /** @type {any} */ (image).width ?? 0;
      const height = /** @type {any} */ (image).height ?? 0;
      textures.set(key, { texture, width, height });
    },

    /** Draw `sprites` for `view` on a canvas of `width` by `height` stage
     *  pixels at `ratio` device pixels each, over `ground` (0–1 channels) —
     *  lit as `light` says, and finished as its `post` does.
     *  @param {Sprite[]} sprites @param {View} view @param {number} width @param {number} height @param {number} ratio
     *  @param {[number, number, number]} ground @param {Lighting} [light] */
    draw(sprites, view, width, height, ratio, ground, light) {
      if (lost) return;
      const w = Math.max(1, Math.round(width * ratio));
      const h = Math.max(1, Math.round(height * ratio));
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }
      packed = { ambient: light?.ambient ?? [1, 1, 1], count: Math.min(LIGHTS, light?.lights.length ?? 0), lights: new Float32Array(LIGHTS * 8) };
      (light?.lights ?? []).slice(0, LIGHTS).forEach((l, i) => packed.lights.set([l.x, l.y, l.radius, l.height, l.color[0], l.color[1], l.color[2], 0], i * 8));
      const post = light?.post && light.post.quality !== "low" ? light.post : null;
      const into = post ? targetsFor(w, h, post.quality === "high" ? 2 : 4) : null;
      gl.bindFramebuffer(gl.FRAMEBUFFER, into?.scene.fb ?? null);
      gl.viewport(0, 0, w, h);
      gl.clearColor(ground[0], ground[1], ground[2], 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      if (sprites.length) drawSprites(sprites, view, width, height);
      if (into && post) finish(into, post);
    },

    /** The colour at a stage pixel of what was just drawn, 0–255 — for the
     *  tests, read in the same task as the draw. @param {number} x @param {number} y */
    probe(x, y) {
      const ratio = canvas.width / Math.max(1, canvas.clientWidth || canvas.width);
      const out = new Uint8Array(4);
      gl.readPixels(Math.round(x * ratio), canvas.height - 1 - Math.round(y * ratio), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, out);
      return [...out];
    },

    destroy() {
      for (const t of textures.values()) gl.deleteTexture(t.texture);
      textures.clear();
      for (const b of partBuffers) gl.deleteBuffer(b);
      gl.deleteTexture(flat);
      gl.deleteTexture(glow);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    },
  };
}
