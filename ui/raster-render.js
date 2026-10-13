/* Canvas mode's picture on the stage: WebGL 2, one texture the canvas's size.
 *
 * The layers are laid together in the engine (raster-runtime.wasm), so every
 * blend mode is the same integer code the core saves with; the GPU only shows
 * the result. What changed is uploaded as rectangles cut straight from the
 * engine's composite — `UNPACK_ROW_LENGTH` and the skips pick a rectangle out
 * of the whole without copying it — and drawn over a checkerboard where the
 * picture is clear. Pixels are sampled nearest at 100 % and above, so pixel
 * art stays pixels; a pixel canvas always is. */

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
in vec2 v_uv;
out vec4 o;
void main() {
  vec2 px = v_uv * u_size;
  vec2 cell = floor(px / u_check);
  vec3 ground = mod(cell.x + cell.y, 2.0) < 1.0 ? u_light : u_dark;
  vec4 c = texture(u_tex, v_uv);              // premultiplied
  o = vec4(c.rgb + ground * (1.0 - c.a), 1.0);
}`;

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
  const program = /** @type {WebGLProgram} */ (gl.createProgram());
  gl.attachShader(program, compile(gl.VERTEX_SHADER, VERTEX));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, FRAGMENT));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) ?? "program");
  const where = (/** @type {string} */ name) => gl.getUniformLocation(program, name);
  const u = { rect: where("u_rect"), stage: where("u_stage"), size: where("u_size"), check: where("u_check"), light: where("u_light"), dark: where("u_dark"), tex: where("u_tex") };
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
  let lost = false;
  canvas.addEventListener("webglcontextlost", (e) => {
    e.preventDefault();
    lost = true;
    hooks.onLost?.();
  });

  return {
    /** A texture for a canvas of this size, clear. @param {number} w @param {number} h */
    resize(w, h) {
      if (lost) return;
      if (texture) gl.deleteTexture(texture);
      texture = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, Math.max(1, w), Math.max(1, h));
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
    },
    /** The picture where the view puts it, on the ground colour.
     *  @param {{zoom: number, x: number, y: number}} view @param {number} width @param {number} height
     *  @param {number} ratio @param {{ground: number[], light: number[], dark: number[], pixel: boolean}} look */
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
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.uniform1i(u.tex, 0);
      gl.uniform4f(u.rect, view.x, view.y, size[0] * view.zoom, size[1] * view.zoom);
      gl.uniform2f(u.stage, width, height);
      gl.uniform2f(u.size, size[0], size[1]);
      // Squares of about eight screen pixels, whatever the zoom; a pixel's own
      // size when a pixel canvas is seen large.
      gl.uniform1f(u.check, look.pixel && view.zoom >= 8 ? 1 : Math.max(1 / 64, 8 / view.zoom));
      gl.uniform3f(u.light, look.light[0], look.light[1], look.light[2]);
      gl.uniform3f(u.dark, look.dark[0], look.dark[1], look.dark[2]);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    },
    /** One pixel of the drawn frame, as RGBA — for tests. @param {number} x @param {number} y */
    probe(x, y) {
      const out = new Uint8Array(4);
      gl.readPixels(x, canvas.height - 1 - y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, out);
      return Array.from(out);
    },
    destroy() {
      if (texture) gl.deleteTexture(texture);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    },
  };
}
