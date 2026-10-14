/* Canvas mode's picture on the stage: WebGL 2, one texture the canvas's size.
 *
 * The layers are laid together in the engine (raster-runtime.wasm), so every
 * blend mode is the same integer code the core saves with; the GPU only shows
 * the result. What changed is uploaded as rectangles cut straight from the
 * engine's composite — `UNPACK_ROW_LENGTH` and the skips pick a rectangle out
 * of the whole without copying it — and drawn over a checkerboard where the
 * picture is clear. Pixels are sampled nearest at 100 % and above, so pixel
 * art stays pixels; a pixel canvas always is. A paint canvas seen smaller
 * than itself is sampled from mipmaps, made again only once the picture is
 * still — never a frame of a stroke — and until then from the picture
 * itself.
 *
 * While pixels are being moved, scaled or turned, they are lifted onto a
 * texture of their own and drawn over the picture where the transform puts
 * them — the GPU's preview; the pixels are made once, when it is let go. */

const VERTEX = `#version 300 es
in vec2 a_corner;
uniform vec4 u_rect;   // stage x, y, width, height (CSS pixels)
uniform vec2 u_stage;  // stage width, height
out vec2 v_uv;
void main() {
  vec2 at = u_rect.xy + a_corner * u_rect.zw;
  gl_Position = vec4(at / u_stage * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
  v_uv = a_corner;
}`;

const FRAGMENT = `#version 300 es
precision mediump float;
uniform sampler2D u_tex;
uniform vec2 u_size;     // canvas pixels
uniform float u_check;   // canvas pixels a checker square
uniform vec3 u_light;
uniform vec3 u_dark;
uniform float u_line;    // canvas pixels a device pixel: the grid's width; 0 for none
in vec2 v_uv;
out vec4 o;
void main() {
  vec2 px = v_uv * u_size;
  vec2 cell = floor(px / u_check);
  vec3 ground = mod(cell.x + cell.y, 2.0) < 1.0 ? u_light : u_dark;
  vec4 c = texture(u_tex, v_uv);              // premultiplied
  vec3 seen = c.rgb + ground * (1.0 - c.a);
  // The pixel grid: a hairline at every pixel's left and top edge.
  vec2 f = fract(px);
  if (u_line > 0.0 && (f.x < u_line || f.y < u_line)) seen = mix(seen, vec3(0.5), 0.35);
  o = vec4(seen, 1.0);
}`;

// What is being moved: a quad at four stage points, premultiplied, laid over.
const FLOAT_VERTEX = `#version 300 es
in vec2 a_at;
in vec2 a_uv;
uniform vec2 u_stage;
out vec2 v_uv;
void main() {
  gl_Position = vec4(a_at / u_stage * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
  v_uv = a_uv;
}`;

const FLOAT_FRAGMENT = `#version 300 es
precision mediump float;
uniform sampler2D u_tex;
in vec2 v_uv;
out vec4 o;
void main() { o = texture(u_tex, v_uv); }`;

/** @param {HTMLCanvasElement} canvas @param {{onLost?: () => void}} [hooks] */
export function createRasterRender(canvas, hooks = {}) {
  const gl = canvas.getContext("webgl2", { alpha: false, antialias: false, premultipliedAlpha: true, preserveDrawingBuffer: false });
  if (!gl) return null;
  const compile = (/** @type {number} */ type, /** @type {string} */ source) => {
    const s = /** @type {WebGLShader} */ (gl.createShader(type));
    gl.shaderSource(s, source);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? "shader");
    return s;
  };
  const link = (/** @type {string} */ vertex, /** @type {string} */ fragment) => {
    const made = /** @type {WebGLProgram} */ (gl.createProgram());
    gl.attachShader(made, compile(gl.VERTEX_SHADER, vertex));
    gl.attachShader(made, compile(gl.FRAGMENT_SHADER, fragment));
    gl.linkProgram(made);
    if (!gl.getProgramParameter(made, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(made) ?? "program");
    return made;
  };
  const program = link(VERTEX, FRAGMENT);
  const floater = link(FLOAT_VERTEX, FLOAT_FRAGMENT);
  const floatVao = gl.createVertexArray();
  const floatBuffer = gl.createBuffer();
  gl.bindVertexArray(floatVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, floatBuffer);
  const floatAt = gl.getAttribLocation(floater, "a_at");
  const floatUv = gl.getAttribLocation(floater, "a_uv");
  gl.enableVertexAttribArray(floatAt);
  gl.vertexAttribPointer(floatAt, 2, gl.FLOAT, false, 16, 0);
  gl.enableVertexAttribArray(floatUv);
  gl.vertexAttribPointer(floatUv, 2, gl.FLOAT, false, 16, 8);
  /** @type {WebGLTexture | null} */
  let floating = null;
  const where = (/** @type {string} */ name) => gl.getUniformLocation(program, name);
  const u = { rect: where("u_rect"), stage: where("u_stage"), size: where("u_size"), check: where("u_check"), light: where("u_light"), dark: where("u_dark"), tex: where("u_tex"), line: where("u_line") };
  const corners = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, corners);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
  const vao = gl.createVertexArray();
  gl.bindVertexArray(vao);
  const loc = gl.getAttribLocation(program, "a_corner");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

  /** @type {WebGLTexture | null} */
  let texture = null;
  let size = [0, 0];
  /** How many sizes the texture has: 1, or a mipmap's whole chain. */
  let levels = 1;
  /** Whether the mipmaps are of an older picture. */
  let stale = false;
  let lost = false;
  canvas.addEventListener("webglcontextlost", (e) => {
    e.preventDefault();
    lost = true;
    hooks.onLost?.();
  });

  return {
    /** A texture for a canvas of this size, clear; with mipmaps for one seen
     *  small. @param {number} w @param {number} h @param {boolean} [mipmaps] */
    resize(w, h, mipmaps = false) {
      if (lost) return;
      if (texture) gl.deleteTexture(texture);
      texture = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, texture);
      levels = mipmaps ? Math.floor(Math.log2(Math.max(1, w, h))) + 1 : 1;
      stale = levels > 1;
      gl.texStorage2D(gl.TEXTURE_2D, levels, gl.RGBA8, Math.max(1, w), Math.max(1, h));
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      size = [w, h];
    },
    /** Rectangles of the composite, cut from it where they are.
     *  @param {number[]} rects x, y, w, h… @param {Uint8Array} composite premultiplied RGBA, the canvas's width */
    upload(rects, composite) {
      if (lost || !texture || !rects.length) return;
      gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      gl.pixelStorei(gl.UNPACK_ROW_LENGTH, size[0]);
      for (let i = 0; i < rects.length; i += 4) {
        const [x, y, w, h] = [rects[i], rects[i + 1], rects[i + 2], rects[i + 3]];
        gl.pixelStorei(gl.UNPACK_SKIP_PIXELS, x);
        gl.pixelStorei(gl.UNPACK_SKIP_ROWS, y);
        gl.texSubImage2D(gl.TEXTURE_2D, 0, x, y, w, h, gl.RGBA, gl.UNSIGNED_BYTE, composite);
      }
      gl.pixelStorei(gl.UNPACK_ROW_LENGTH, 0);
      gl.pixelStorei(gl.UNPACK_SKIP_PIXELS, 0);
      gl.pixelStorei(gl.UNPACK_SKIP_ROWS, 0);
      stale = levels > 1;
    },
    /** Pixels being moved, premultiplied, `w` by `h` — or none.
     *  @param {Uint8Array | null} pixels @param {number} [w] @param {number} [h] */
    float(pixels, w = 0, h = 0) {
      if (lost) return;
      if (floating) gl.deleteTexture(floating);
      floating = null;
      if (!pixels || !w || !h) return;
      floating = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, floating);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    },
    /** The picture where the view puts it, on the ground colour, and what
     *  is being moved where its four corners are (stage pixels: top left,
     *  top right, bottom left, bottom right).
     *  @param {{zoom: number, x: number, y: number}} view @param {number} width @param {number} height
     *  @param {number} ratio @param {{ground: number[], light: number[], dark: number[], pixel: boolean, grid?: boolean, still?: boolean,
     *          float?: {corners: number[][], nearest: boolean}}} look */
    draw(view, width, height, ratio, look) {
      if (lost) return;
      const w = Math.round(width * ratio);
      const h = Math.round(height * ratio);
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      gl.viewport(0, 0, w, h);
      gl.clearColor(look.ground[0], look.ground[1], look.ground[2], 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      if (!texture) return;
      gl.useProgram(program);
      gl.bindVertexArray(vao);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, texture);
      const nearest = look.pixel || view.zoom >= 1;
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, nearest ? gl.NEAREST : gl.LINEAR);
      // Seen small and still: the mipmaps, made again if the picture changed.
      const small = view.zoom < 1 && levels > 1;
      if (small && stale && look.still) {
        gl.generateMipmap(gl.TEXTURE_2D);
        stale = false;
      }
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, small && !stale ? gl.LINEAR_MIPMAP_LINEAR : gl.LINEAR);
      gl.uniform1i(u.tex, 0);
      gl.uniform4f(u.rect, view.x, view.y, size[0] * view.zoom, size[1] * view.zoom);
      gl.uniform2f(u.stage, width, height);
      gl.uniform2f(u.size, size[0], size[1]);
      // Squares of about eight screen pixels, whatever the zoom; a pixel's own
      // size when a pixel canvas is seen large.
      gl.uniform1f(u.check, look.pixel && view.zoom >= 8 ? 1 : Math.max(1 / 64, 8 / view.zoom));
      gl.uniform3f(u.light, look.light[0], look.light[1], look.light[2]);
      gl.uniform3f(u.dark, look.dark[0], look.dark[1], look.dark[2]);
      // The grid once a pixel is six screen pixels or more: below that it is
      // only a shade over the picture.
      gl.uniform1f(u.line, look.grid && view.zoom >= 6 ? 1 / (view.zoom * ratio) : 0);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      if (floating && look.float) {
        const [a, b, c, d] = look.float.corners;
        gl.useProgram(floater);
        gl.bindVertexArray(floatVao);
        gl.bindBuffer(gl.ARRAY_BUFFER, floatBuffer);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([a[0], a[1], 0, 0, b[0], b[1], 1, 0, c[0], c[1], 0, 1, d[0], d[1], 1, 1]), gl.DYNAMIC_DRAW);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, floating);
        const filter = look.float.nearest ? gl.NEAREST : gl.LINEAR;
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
        gl.uniform1i(gl.getUniformLocation(floater, "u_tex"), 0);
        gl.uniform2f(gl.getUniformLocation(floater, "u_stage"), width, height);
        gl.enable(gl.BLEND);
        gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
        gl.disable(gl.BLEND);
      }
    },
    /** One pixel of the drawn frame, as RGBA — for tests. @param {number} x @param {number} y */
    probe(x, y) {
      const out = new Uint8Array(4);
      gl.readPixels(x, canvas.height - 1 - y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, out);
      return Array.from(out);
    },
    destroy() {
      if (texture) gl.deleteTexture(texture);
      if (floating) gl.deleteTexture(floating);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    },
  };
}
