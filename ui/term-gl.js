/* The terminal, drawn by the GPU.

   Opt-in, from Settings → Appearance → Terminal. The text renderer draws each
   row as spans, and a shell printing fast changes nearly every row on every
   frame: new markup to parse, style, lay out and paint, sixty times a second,
   on the same thread the editor needs. This draws the screen the way Zed
   does, and the WebGL renderers of xterm.js and VS Code: each glyph is
   rasterised once into a sheet on the GPU, and a frame is two instanced draws
   — the cells' colours, then their glyphs — with no layout at all.

   Cells are counted, never measured. A wide character arrives followed by the
   blank cell it covers, so it is drawn in its own cell and spills into the
   next, and the rest of the line stays on the grid. */

/** @typedef {{text: string, fg?: number | null, bg?: number | null, rgb?: string | null,
 *   bold?: boolean, italic?: boolean, underline?: boolean, inverse?: boolean}} Run */
/** @typedef {{rows: Run[][], cursor: number[], cursorVisible: boolean}} Screen */
/** Straight, not premultiplied: red, green, blue and alpha, from 0 to 1.
 *  @typedef {number[]} Colour */

const BOXES = `#version 300 es
layout(location = 0) in vec2 corner;
layout(location = 1) in vec4 box;
layout(location = 2) in vec4 color;
uniform vec2 screen;
out vec4 tint;
void main() {
  vec2 at = (box.xy + corner * box.zw) / screen * 2.0 - 1.0;
  gl_Position = vec4(at.x, -at.y, 0.0, 1.0);
  tint = vec4(color.rgb * color.a, color.a);
}`;
const FILL = `#version 300 es
precision highp float;
in vec4 tint;
out vec4 colour;
void main() { colour = tint; }`;

const MARKS = `#version 300 es
layout(location = 0) in vec2 corner;
layout(location = 1) in vec4 place;
layout(location = 2) in vec4 color;
uniform vec2 screen;
uniform vec2 slot;
uniform vec2 sheet;
out vec2 uv;
out vec4 tint;
void main() {
  vec2 at = (place.xy + corner * slot) / screen * 2.0 - 1.0;
  gl_Position = vec4(at.x, -at.y, 0.0, 1.0);
  uv = (place.zw + corner * slot) / sheet;
  tint = color;
}`;
// A glyph is coverage, in its cell's colour. An emoji brings colours of its
// own: it is marked with a negative alpha and drawn as it was rasterised.
const INK = `#version 300 es
precision highp float;
uniform sampler2D glyphs;
in vec2 uv;
in vec4 tint;
out vec4 colour;
void main() {
  vec4 texel = texture(glyphs, uv);
  colour = tint.a < 0.0 ? texel : vec4(tint.rgb, 1.0) * (texel.a * tint.a);
}`;

/** The sheet starts small and doubles when it fills, up to this. */
const LARGEST_SHEET = 4096;
/** What joins the character before it rather than taking a cell — what has
 *  no width, by the same rule the core's terminal uses to fill its cells:
 *  combining marks, format characters such as the zero-width joiner, and the
 *  vowels and finals of a Hangul syllable spelled out in jamo. */
const JOINS = /^[\p{Mn}\p{Me}\p{Cf}ᅠ-ᇿ]$/u;
/** The answer for each character of the basic plane, worked out once each:
 *  0 not yet asked, 1 joins, 2 takes a cell of its own. */
const joining = new Uint8Array(0x10000);
/** @param {number} code */
function joins(code) {
  if (code > 0xffff) return JOINS.test(String.fromCodePoint(code));
  if (!joining[code]) joining[code] = JOINS.test(String.fromCharCode(code)) ? 1 : 2;
  return joining[code] === 1;
}

/** The xterm palette past the theme's sixteen: a 6×6×6 cube, then greys.
 *  @param {number} index @returns {Colour} */
function xterm(index) {
  if (index >= 232) {
    const v = (8 + (index - 232) * 10) / 255;
    return [v, v, v, 1];
  }
  const i = index - 16;
  const level = (/** @type {number} */ n) => (n === 0 ? 0 : (55 + n * 40) / 255);
  return [level(Math.floor(i / 36)), level(Math.floor(i / 6) % 6), level(i % 6), 1];
}

/** One of the 240 colours past the theme's sixteen, as `#rrggbb`, for the
 *  text renderer — which otherwise has no name to give them.
 *  @param {number} index */
export function xtermHex(index) {
  const byte = (/** @type {number} */ v) => Math.round(v * 255).toString(16).padStart(2, "0");
  return "#" + xterm(index).slice(0, 3).map(byte).join("");
}

/**
 * @param {WebGL2RenderingContext} gl
 * @param {string} vertex @param {string} fragment
 */
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
 * A GPU renderer over `screen`, or null where WebGL 2 is not to be had — in
 * which case the text renderer carries on as it always has.
 *
 * It keeps the last screen it was given, so it can draw it again on its own
 * when something other than the shell changes the picture: the panel's size,
 * the display, a font arriving.
 *
 * @param {HTMLElement} screen the terminal's screen, for its size, font and colours
 * @param {{onLost?: () => void}} [hooks] told if the GPU takes the context back
 */
export function createTerminalGL(screen, hooks = {}) {
  const parent = screen.parentElement;
  if (!parent) return null;
  const pane = parent;
  const canvas = document.createElement("canvas");
  canvas.className = "term-gl";
  canvas.setAttribute("aria-hidden", "true");
  // No depth, stencil or multisampling: each would be another buffer the size
  // of the screen, for nothing a grid of quads needs. And the low-power GPU,
  // where a Mac has two: text is not worth waking the other one for.
  const context = canvas.getContext("webgl2", {
    alpha: true, antialias: false, premultipliedAlpha: true, depth: false, stencil: false, powerPreference: "low-power",
  });
  if (!context) return null;
  const gl = context;

  /** @type {WebGLProgram} */ let boxes;
  /** @type {WebGLProgram} */ let marks;
  try {
    boxes = program(gl, BOXES, FILL);
    marks = program(gl, MARKS, INK);
  } catch (error) {
    console.error("proton: the GPU terminal could not start", error);
    return null;
  }
  pane.append(canvas);
  const at = {
    boxScreen: gl.getUniformLocation(boxes, "screen"),
    markScreen: gl.getUniformLocation(marks, "screen"),
    slot: gl.getUniformLocation(marks, "slot"),
    sheet: gl.getUniformLocation(marks, "sheet"),
  };

  const quad = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
  /** A pass: the shared corner of a quad, then eight floats an instance — a
   *  place and a colour. */
  const pass = () => {
    const vao = gl.createVertexArray();
    const data = gl.createBuffer();
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, quad);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, data);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 4, gl.FLOAT, false, 32, 0);
    gl.vertexAttribDivisor(1, 1);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 4, gl.FLOAT, false, 32, 16);
    gl.vertexAttribDivisor(2, 1);
    gl.bindVertexArray(null);
    return { vao, data };
  };
  const fills = pass();
  const glyphs = pass();

  // Glyphs are rasterised one at a time into a scratch canvas the size of a
  // slot and copied up into the sheet; the sheet itself only exists on the GPU.
  const sheet = gl.createTexture();
  let sheetSize = 1024;
  const scratch = document.createElement("canvas");
  const ink = /** @type {CanvasRenderingContext2D} */ (scratch.getContext("2d", { willReadFrequently: true }));
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);

  /** @typedef {{u: number, v: number, colour: boolean}} Slot */
  /** @type {Map<string, Slot>} */
  let slots = new Map();
  /** Printable ASCII by style — bold, then italic, as bits — and character
   *  code: what nearly every screen is made of, found without making a key.
   *  @type {(Slot | null | undefined)[][]} */
  let ascii = [[], [], [], []];
  let next = 0;
  /** The style the scratch canvas's font is set to: parsing a font is not
   *  free, and a first frame sets it for every glyph. */
  let inked = -1;

  // Everything that depends on the font and the display, in device pixels.
  let styled = false, fitted = false, lost = false, fontKey = "";
  let dpr = 1, cellW = 7, cellH = 17, fontPx = 13, family = "monospace";
  let padX = 0, padY = 0, slotW = 1, slotH = 1, baseline = 0, line = 1;
  /** Where the grid starts on the canvas: the screen's padding. */
  let originX = 0, originY = 0;
  /** The theme's colours, read once a restyle rather than once a frame. */
  let colours = {
    text: [1, 1, 1, 1], ground: [0, 0, 0, 1], cursor: [1, 1, 1, 1],
    fg: /** @type {Colour[]} */ ([]), bg: /** @type {Colour[]} */ ([]),
  };
  const palette = Array.from({ length: 256 }, (_, i) => (i < 16 ? [1, 1, 1, 1] : xterm(i)));
  /** @type {Map<string, Colour>} */
  const trueColours = new Map();

  let boxData = new Float32Array(2048 * 8), boxCount = 0;
  let glyphData = new Float32Array(4096 * 8), glyphCount = 0;
  /** The run being drawn's colours, after reverse video. Set by `paint`
   *  rather than returned, so that a frame makes no garbage. */
  /** @type {Colour} */
  let fgNow = colours.text;
  /** @type {Colour | null} */
  let bgNow = null;
  /** @type {Screen | null} */
  let last = null;
  let lastFocused = false;
  /** The frame a draw is waiting for, if one is. */
  let pending = 0;

  /** @param {number} style bold, then italic, as bits */
  const fontFor = (style) => `${style & 2 ? "italic " : ""}${style & 1 ? 680 : 400} ${fontPx}px ${family}`;

  /** CSS colours — tokens or values — as the page would paint them. */
  function resolver() {
    const probe = document.createElement("i");
    probe.hidden = true;
    pane.append(probe);
    const pixel = /** @type {CanvasRenderingContext2D} */ (
      Object.assign(document.createElement("canvas"), { width: 1, height: 1 }).getContext("2d", { willReadFrequently: true }));
    return {
      /** @param {string} css @returns {Colour} */
      read(css) {
        probe.style.color = "";
        probe.style.color = css;
        pixel.clearRect(0, 0, 1, 1);
        pixel.fillStyle = getComputedStyle(probe).color;
        pixel.fillRect(0, 0, 1, 1);
        const [r, g, b, a] = pixel.getImageData(0, 0, 1, 1).data;
        return [r / 255, g / 255, b / 255, a / 255];
      },
      done() { probe.remove(); },
    };
  }

  /** The colour behind the text: the first ancestor that paints one.
   *  @param {(css: string) => Colour} read @returns {Colour} */
  function groundColour(read) {
    for (let node = /** @type {Element | null} */ (screen); node; node = node.parentElement) {
      const colour = read(getComputedStyle(node).backgroundColor);
      if (colour[3] > 0.5) return [colour[0], colour[1], colour[2], 1];
    }
    return read("var(--bg)");
  }

  /** Read the font, the cell and the theme: on the first frame, and again
   *  whenever any of them may have changed. The glyphs are only drawn again
   *  if the font or the display really did change. */
  function restyle() {
    dpr = window.devicePixelRatio || 1;
    const style = getComputedStyle(screen);
    const size = parseFloat(style.fontSize) || 13;
    // The cell, measured the way the panel measures it to tell the shell.
    const probe = document.createElement("span");
    probe.style.cssText = "position:absolute;visibility:hidden;white-space:pre;padding:0";
    probe.textContent = "0".repeat(100);
    screen.append(probe);
    const width = probe.getBoundingClientRect().width / 100 || 7;
    probe.remove();
    const key = `${size}|${style.fontFamily}|${width}|${style.lineHeight}|${dpr}`;
    if (key !== fontKey) {
      fontKey = key;
      family = style.fontFamily || "monospace";
      cellW = width * dpr;
      cellH = (parseFloat(style.lineHeight) || size * 1.3) * dpr;
      fontPx = size * dpr;
      // A slot holds a glyph two cells wide, with room around it for what
      // overhangs its cell: italics, and the icons a prompt draws with.
      padX = Math.ceil(fontPx * 0.3);
      padY = Math.ceil(fontPx * 0.3);
      slotW = Math.ceil(cellW * 2) + padX * 2;
      slotH = Math.ceil(cellH) + padY * 2;
      scratch.width = slotW;
      scratch.height = slotH;
      inked = -1;
      ink.font = fontFor(0);
      const metrics = ink.measureText("M");
      const ascent = metrics.fontBoundingBoxAscent || fontPx * 0.8;
      const descent = metrics.fontBoundingBoxDescent || fontPx * 0.2;
      baseline = Math.round((cellH - (ascent + descent)) / 2 + ascent);
      line = Math.max(1, Math.round(dpr));
      fresh(false);
    }

    const { read, done } = resolver();
    const ansi = (/** @type {number} */ i) => read(`var(--ansi-${i})`);
    colours = {
      text: read(style.color),
      ground: groundColour(read),
      cursor: read("var(--cursor)"),
      fg: Array.from({ length: 16 }, (_, i) => ansi(i)),
      // As the text renderer has them: black and bright black are the
      // theme's grounds, so a "black" bar sits on the page rather than in it.
      bg: Array.from({ length: 16 }, (_, i) => (i === 0 ? read("var(--bg-deep)") : i === 8 ? read("var(--e2)") : ansi(i))),
    };
    done();
    styled = true;
    fitted = false;
  }

  /** An empty sheet, bigger than the last if it filled up.
   *  @param {boolean} grow */
  function fresh(grow) {
    if (grow && sheetSize < LARGEST_SHEET) sheetSize *= 2;
    slots = new Map();
    ascii = [[], [], [], []];
    next = 0;
    gl.bindTexture(gl.TEXTURE_2D, sheet);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, sheetSize, sheetSize, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  }

  /** Lay the canvas over the screen, on whole device pixels so the
   *  compositor never has to resample the text. It covers the padding too,
   *  as the text does: a glyph in the last column may overhang its cell. */
  function fit() {
    const style = getComputedStyle(screen);
    const px = (/** @type {string} */ name) => parseFloat(style.getPropertyValue(name)) || 0;
    const outer = screen.getBoundingClientRect(), frame = pane.getBoundingClientRect();
    const snap = (/** @type {number} */ v) => Math.round(v * dpr) / dpr;
    const left = snap(outer.left - frame.left + screen.clientLeft);
    const top = snap(outer.top - frame.top + screen.clientTop);
    const width = Math.max(1, Math.floor(screen.clientWidth));
    const height = Math.max(1, Math.floor(screen.clientHeight));
    originX = Math.round(px("padding-left") * dpr);
    originY = Math.round(px("padding-top") * dpr);
    const place = `left:${left}px;top:${top}px;width:${width}px;height:${height}px`;
    if (canvas.style.cssText !== place) canvas.style.cssText = place;
    // Setting a canvas's size throws its picture away, even to the same size.
    const w = Math.round(width * dpr), h = Math.round(height * dpr);
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
    fitted = true;
  }

  /** Where a glyph is in the sheet, rasterising it there first if it is new —
   *  or null when the sheet is full.
   *  @param {string} text @param {number} style bold, then italic, as bits
   *  @returns {Slot | null} */
  function place(text, style) {
    const key = style + text;
    const known = slots.get(key);
    if (known) return known;
    const across = Math.floor(sheetSize / slotW);
    if (next >= across * Math.floor(sheetSize / slotH)) return null;
    const u = (next % across) * slotW, v = Math.floor(next / across) * slotH;
    next++;
    ink.clearRect(0, 0, slotW, slotH);
    if (inked !== style) {
      ink.font = fontFor(style);
      ink.fillStyle = "#fff";
      ink.textBaseline = "alphabetic";
      inked = style;
    }
    ink.fillText(text, padX, padY + baseline);
    // Only something well past the alphabets can be an emoji; checking every
    // letter's pixels would make the first frame slow for nothing.
    let colour = false;
    if (/** @type {number} */ (text.codePointAt(0)) >= 0x2100) {
      const pixels = ink.getImageData(0, 0, slotW, slotH).data;
      for (let i = 0; i < pixels.length && !colour; i += 4) {
        colour = pixels[i + 3] > 32 && (Math.abs(pixels[i] - pixels[i + 1]) > 24 || Math.abs(pixels[i + 1] - pixels[i + 2]) > 24);
      }
    }
    gl.bindTexture(gl.TEXTURE_2D, sheet);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, u, v, gl.RGBA, gl.UNSIGNED_BYTE, scratch);
    const slot = { u, v, colour };
    slots.set(key, slot);
    return slot;
  }

  /** @param {string} hex @returns {Colour} */
  function trueColour(hex) {
    let colour = trueColours.get(hex);
    if (!colour) {
      const n = parseInt(hex.slice(1), 16);
      colour = [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, 1];
      trueColours.set(hex, colour);
    }
    return colour;
  }

  /** A run's foreground and background, after reverse video, into
   *  `fgNow` and `bgNow`. @param {Run} run */
  function paint(run) {
    const fg = run.rgb ? trueColour(run.rgb) : run.fg != null ? (run.fg < 16 ? colours.fg[run.fg] : palette[run.fg]) : colours.text;
    const bg = run.bg != null ? (run.bg < 16 ? colours.bg[run.bg] : palette[run.bg]) : null;
    fgNow = run.inverse ? bg ?? colours.ground : fg;
    bgNow = run.inverse ? fg : bg;
  }

  /** @param {number} x @param {number} y @param {number} w @param {number} h @param {Colour} c */
  function box(x, y, w, h, c) {
    if ((boxCount + 1) * 8 > boxData.length) {
      const grown = new Float32Array(boxData.length * 2);
      grown.set(boxData);
      boxData = grown;
    }
    const o = boxCount++ * 8;
    boxData[o] = x; boxData[o + 1] = y; boxData[o + 2] = w; boxData[o + 3] = h;
    boxData[o + 4] = c[0]; boxData[o + 5] = c[1]; boxData[o + 6] = c[2]; boxData[o + 7] = c[3];
  }

  /** @param {number} x @param {number} y @param {Slot} slot @param {Colour} c */
  function mark(x, y, slot, c) {
    if ((glyphCount + 1) * 8 > glyphData.length) {
      const grown = new Float32Array(glyphData.length * 2);
      grown.set(glyphData);
      glyphData = grown;
    }
    const o = glyphCount++ * 8;
    glyphData[o] = x - padX; glyphData[o + 1] = y - padY; glyphData[o + 2] = slot.u; glyphData[o + 3] = slot.v;
    glyphData[o + 4] = c[0]; glyphData[o + 5] = c[1]; glyphData[o + 6] = c[2]; glyphData[o + 7] = slot.colour ? -1 : c[3];
  }

  /** The frame's quads, or false when the sheet filled up partway through.
   *  @param {Screen} event @param {boolean} focused */
  function build(event, focused) {
    boxCount = 0;
    glyphCount = 0;
    const [cursorRow, cursorCol] = event.cursor;
    const block = event.cursorVisible && focused;
    const x = (/** @type {number} */ column) => originX + Math.round(column * cellW);
    const y = (/** @type {number} */ row) => originY + Math.round(row * cellH);
    for (let row = 0; row < event.rows.length; row++) {
      const top = y(row), bottom = y(row + 1);
      // The character under a block cursor is drawn in the ground colour, so
      // the cursor covers the cell without hiding what is in it.
      const under = block && row === cursorRow ? cursorCol : -1;
      let column = 0;
      for (const run of event.rows[row]) {
        paint(run);
        const fg = fgNow, bg = bgNow;
        const style = (run.bold ? 1 : 0) | (run.italic ? 2 : 0);
        const known = ascii[style];
        const text = run.text, start = column;
        for (let i = 0; i < text.length; column++) {
          const code = text.charCodeAt(i);
          let end = i + (code >= 0xd800 && code < 0xdc00 ? 2 : 1);
          // Whatever joins the character comes with it, into the same cell.
          while (end < text.length && text.charCodeAt(end) >= 0x300) {
            const next = /** @type {number} */ (text.codePointAt(end));
            if (!joins(next)) break;
            end += next > 0xffff ? 2 : 1;
          }
          if (code !== 32 || end > i + 1) {
            const slot = code < 127 && end === i + 1
              ? known[code] ?? (known[code] = place(text[i], style))
              : place(text.slice(i, end), style);
            if (!slot) return false;
            mark(x(column), top, slot, column === under ? colours.ground : fg);
          }
          i = end;
        }
        if (bg) box(x(start), top, x(column) - x(start), bottom - top, bg);
        if (run.underline) box(x(start), Math.min(bottom - line, top + baseline + line), x(column) - x(start), line, fg);
      }
    }
    // The cursor, as the text renderer draws it: a block while the shell has
    // the keyboard, an outline of one while it does not.
    if (event.cursorVisible) {
      const left = x(cursorCol), top = y(cursorRow);
      const w = x(cursorCol + 1) - left, h = y(cursorRow + 1) - top;
      if (focused) box(left, top, w, h, colours.cursor);
      else {
        box(left, top, w, line, colours.cursor);
        box(left, top + h - line, w, line, colours.cursor);
        box(left, top, line, h, colours.cursor);
        box(left + w - line, top, line, h, colours.cursor);
      }
    }
    return true;
  }

  function submit() {
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);

    gl.useProgram(boxes);
    gl.uniform2f(at.boxScreen, canvas.width, canvas.height);
    gl.bindVertexArray(fills.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, fills.data);
    gl.bufferData(gl.ARRAY_BUFFER, boxData.subarray(0, boxCount * 8), gl.STREAM_DRAW);
    if (boxCount) gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, boxCount);

    gl.useProgram(marks);
    gl.bindTexture(gl.TEXTURE_2D, sheet);
    gl.uniform2f(at.markScreen, canvas.width, canvas.height);
    gl.uniform2f(at.slot, slotW, slotH);
    gl.uniform2f(at.sheet, sheetSize, sheetSize);
    gl.bindVertexArray(glyphs.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, glyphs.data);
    gl.bufferData(gl.ARRAY_BUFFER, glyphData.subarray(0, glyphCount * 8), gl.STREAM_DRAW);
    if (glyphCount) gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, glyphCount);
    gl.bindVertexArray(null);
  }

  /** Draw a screen: at the next frame, and once, however many screens
   *  arrive before it — only the last of them would ever be seen.
   *  @param {Screen} event @param {boolean} focused */
  function draw(event, focused) {
    last = event;
    lastFocused = focused;
    if (!pending && !lost) pending = requestAnimationFrame(render);
  }

  function render() {
    pending = 0;
    if (lost || !last) return;
    // A hidden pane has nothing to measure, and a cell measured there would
    // be wrong for as long as it is shown. The observer below draws the
    // moment it has a size again.
    if (!screen.clientWidth) return;
    if ((window.devicePixelRatio || 1) !== dpr) styled = false;
    if (!styled) restyle();
    if (!fitted) fit();
    // A sheet that fills partway through a frame is started again, larger,
    // and the frame built from nothing — rare, and only ever once.
    if (!build(last, lastFocused)) {
      fresh(true);
      build(last, lastFocused);
    }
    submit();
  }

  /** The last screen again, for a change that is not the shell's. */
  function redraw(focused = lastFocused) {
    if (last) draw(last, focused);
  }

  const resized = new ResizeObserver(() => {
    fitted = false;
    redraw();
  });
  resized.observe(screen);
  // The terminal's font is a web font, and may arrive after the first frame
  // has been drawn in a fallback.
  const fontsArrived = () => {
    // Glyphs drawn before it came were drawn in a fallback, whatever the
    // measurements say: draw them all again.
    fontKey = "";
    styled = false;
    redraw();
  };
  document.fonts?.addEventListener("loadingdone", fontsArrived);
  canvas.addEventListener("webglcontextlost", (e) => {
    e.preventDefault();
    lost = true;
    hooks.onLost?.();
  });

  return {
    draw,
    redraw,
    /** The theme, the font or its size changed: read them again. */
    restyle() {
      styled = false;
      redraw();
    },
    /** Stop: the canvas goes, and the text renderer takes over again. */
    destroy() {
      cancelAnimationFrame(pending);
      resized.disconnect();
      document.fonts?.removeEventListener("loadingdone", fontsArrived);
      canvas.remove();
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    },
  };
}
