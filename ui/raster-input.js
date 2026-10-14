/* Canvas mode's pen: how hard it presses, and a steadier line than the hand
 * draws. No DOM — the node tests drive it.
 *
 * Pressure comes from one of three places. A browser that knows a pen gives
 * it on the pointer event: `pointerType` "pen" and its pressure. WebKit on
 * the Mac does not — a tablet's pen reaches the page as a mouse — so the
 * shell reads it from AppKit (crates/proton-shell/src/pen.rs) and calls
 * `__protonPen` before the pointer event it belongs to arrives. And a mouse
 * or a trackpad has none: a stroke drawn with one is at full pressure.
 *
 * Whatever a stroke's pressure is, it is made a whole number here, once,
 * after the pressure curve: the copy in the window and the core are both
 * given that number, and so make the same pixels from it. */

/** @typedef {"soft" | "linear" | "firm"} Curve */

/** How much pressure reaches the brush, by the curve chosen: a soft curve
 *  gives more for a light touch, a firm one makes it work for it.
 *  @param {number} p 0 to 1 @param {Curve} curve @returns {number} 0 to 65535 */
export function curved(p, curve = "linear") {
  const x = Math.min(1, Math.max(0, p));
  const y = curve === "soft" ? Math.pow(x, 0.6) : curve === "firm" ? Math.pow(x, 1.7) : x;
  return Math.round(y * 65535);
}

/** The pen as the shell tells of it, and which pressure a stroke has.
 *  @param {() => number} [now] */
export function createPen(now = () => performance.now()) {
  /** The last the shell said: pressed or not, how hard, when. */
  let pressed = false;
  let pressure = 0;
  let at = -Infinity;
  /** Whether the eraser end is near the tablet. */
  let eraser = false;

  return {
    /** What the shell says: "d" pressed, at `p`; "u" lifted; "n" or "e"
     *  near, by its point or its eraser end; "a" away.
     *  @param {string} what @param {number} p */
    native(what, p) {
      if (what === "d") {
        pressed = true;
        pressure = Math.min(1, Math.max(0, p));
        at = now();
      } else if (what === "u") {
        pressed = false;
      } else if (what === "n" || what === "e") {
        eraser = what === "e";
      } else if (what === "a") {
        eraser = false;
        pressed = false;
      }
    },
    /** Where a stroke begun by this pointer event takes its pressure from:
     *  the event's own, the shell's, or none.
     *  @param {{pointerType: string, pressure: number}} e @returns {"pointer" | "native" | "none"} */
    source(e) {
      if (e.pointerType === "pen") return "pointer";
      // The shell's word came just before this event, for this press.
      if (pressed && now() - at < 250) return "native";
      return "none";
    },
    /** How hard, 0 to 1, from a source. @param {"pointer" | "native" | "none"} source
     *  @param {{pressure: number}} e */
    pressure(source, e) {
      if (source === "pointer") return e.pressure;
      if (source === "native") return pressure;
      return 1;
    },
    /** Whether the pen's eraser end is the one near the tablet. */
    erasing: () => eraser,
  };
}

/** A line steadier than the hand: the brush is pulled behind the pen on a
 *  string `length` long, and moves only when the string is taut — a wobble
 *  shorter than the string never reaches the canvas. With no string, every
 *  point passes as it is.
 *  @param {number} length in canvas pixels */
export function createStabiliser(length) {
  /** Where the brush is. @type {[number, number] | null} */
  let brush = null;
  return {
    /** The pen at `x`, `y` and pressure `p`: the points the brush passes, as
     *  x, y and p — none while the string is slack.
     *  @param {number} x @param {number} y @param {number} p @returns {number[]} */
    add(x, y, p) {
      if (!brush || length <= 0) {
        brush = [x, y];
        return [x, y, p];
      }
      const [dx, dy] = [x - brush[0], y - brush[1]];
      const d = Math.hypot(dx, dy);
      if (d <= length) return [];
      const k = (d - length) / d;
      brush = [brush[0] + dx * k, brush[1] + dy * k];
      return [brush[0], brush[1], p];
    },
    /** The pen lifted at `x`, `y`: the brush drawn the rest of the way, so
     *  the line ends where the pen did.
     *  @param {number} x @param {number} y @param {number} p @returns {number[]} */
    end(x, y, p) {
      if (!brush || (brush[0] === x && brush[1] === y)) return [];
      brush = [x, y];
      return [x, y, p];
    },
    /** Where the brush is, for the cursor's string. */
    at: () => brush,
  };
}
