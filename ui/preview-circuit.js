/* Circuit for the browser preview: one project in a pretend folder, a
 * full adder made of two half adders, so the drawing can be designed and
 * tested against. Edits are applied as plainly as a stand-in can: parts
 * moved and turned with their pins, wires added without splitting. Each circuit is as the core sent it — dumped from
 * crates/proton-core with the pins it placed — because placing pins is the
 * simulator's to do and this must never grow into a second. */

/** The circuits, as drawn. @type {Record<string, {parts: any[], wires: number[][]}>} */
const DRAWN = {"main":{"parts":[{"id":"A","kind":"in","x":0,"y":0,"rot":0,"mirror":false,"settings":[["label","A"]],"pins":[{"x":0,"y":0,"input":false}],"problem":null},{"id":"B","kind":"in","x":0,"y":1,"rot":0,"mirror":false,"settings":[["label","B"]],"pins":[{"x":0,"y":1,"input":false}],"problem":null},{"id":"Cin","kind":"in","x":0,"y":4,"rot":0,"mirror":false,"settings":[["label","Cin"]],"pins":[{"x":0,"y":4,"input":false}],"problem":null},{"id":"Cout","kind":"out","x":16,"y":1,"rot":0,"mirror":false,"settings":[["label","Cout"]],"pins":[{"x":16,"y":1,"input":true}],"problem":null},{"id":"H1","kind":"sub:half","x":2,"y":0,"rot":0,"mirror":false,"settings":[],"pins":[{"x":2,"y":0,"input":true},{"x":2,"y":1,"input":true},{"x":5,"y":0,"input":false},{"x":5,"y":1,"input":false}],"problem":null},{"id":"H2","kind":"sub:half","x":7,"y":1,"rot":0,"mirror":false,"settings":[],"pins":[{"x":7,"y":1,"input":true},{"x":7,"y":2,"input":true},{"x":10,"y":1,"input":false},{"x":10,"y":2,"input":false}],"problem":null},{"id":"O","kind":"or","x":12,"y":0,"rot":0,"mirror":false,"settings":[],"pins":[{"x":12,"y":0,"input":true},{"x":12,"y":2,"input":true},{"x":15,"y":1,"input":false}],"problem":null},{"id":"S","kind":"out","x":16,"y":4,"rot":0,"mirror":false,"settings":[["label","S"]],"pins":[{"x":16,"y":4,"input":true}],"problem":null}],"wires":[[0,0,2,0],[0,1,2,1],[0,4,6,4],[5,0,12,0],[5,1,7,1],[6,2,6,4],[6,2,7,2],[10,1,11,1],[10,2,10,4],[10,4,16,4],[11,1,11,2],[11,2,12,2],[15,1,16,1]]},"half":{"parts":[{"id":"A","kind":"in","x":0,"y":0,"rot":0,"mirror":false,"settings":[["label","A"]],"pins":[{"x":0,"y":0,"input":false}],"problem":null},{"id":"B","kind":"in","x":0,"y":2,"rot":0,"mirror":false,"settings":[["label","B"]],"pins":[{"x":0,"y":2,"input":false}],"problem":null},{"id":"C","kind":"out","x":8,"y":4,"rot":0,"mirror":false,"settings":[["label","C"]],"pins":[{"x":8,"y":4,"input":true}],"problem":null},{"id":"S","kind":"out","x":8,"y":0,"rot":0,"mirror":false,"settings":[["label","S"]],"pins":[{"x":8,"y":0,"input":true}],"problem":null},{"id":"X","kind":"xor","x":4,"y":0,"rot":0,"mirror":false,"settings":[],"pins":[{"x":4,"y":0,"input":true},{"x":4,"y":2,"input":true},{"x":7,"y":1,"input":false}],"problem":null},{"id":"Y","kind":"and","x":4,"y":4,"rot":0,"mirror":false,"settings":[],"pins":[{"x":4,"y":4,"input":true},{"x":4,"y":6,"input":true},{"x":7,"y":5,"input":false}],"problem":null}],"wires":[[0,0,2,0],[0,2,3,2],[2,0,2,4],[2,0,4,0],[2,4,4,4],[3,2,3,6],[3,2,4,2],[3,6,4,6],[7,1,8,1],[7,5,8,5],[8,0,8,1],[8,4,8,5]]}};

/** The full adder running with A and Cin high, as the core sent it. The
 *  preview shows this one state; running anything else is the app's. */
const RUNNING = {"circuit":"main","wires":[{"bits":1,"hex":"1","z":false,"someZ":false},{"bits":1,"hex":"0","z":false,"someZ":false},{"bits":1,"hex":"1","z":false,"someZ":false},{"bits":1,"hex":"0","z":false,"someZ":false},{"bits":1,"hex":"1","z":false,"someZ":false},{"bits":1,"hex":"1","z":false,"someZ":false},{"bits":1,"hex":"1","z":false,"someZ":false},{"bits":1,"hex":"1","z":false,"someZ":false},{"bits":1,"hex":"0","z":false,"someZ":false},{"bits":1,"hex":"0","z":false,"someZ":false},{"bits":1,"hex":"1","z":false,"someZ":false},{"bits":1,"hex":"1","z":false,"someZ":false},{"bits":1,"hex":"1","z":false,"someZ":false}],"values":[["A",{"bits":1,"hex":"1","z":false,"someZ":false}],["B",{"bits":1,"hex":"0","z":false,"someZ":false}],["Cin",{"bits":1,"hex":"1","z":false,"someZ":false}],["Cout",{"bits":1,"hex":"1","z":false,"someZ":false}],["S",{"bits":1,"hex":"0","z":false,"someZ":false}]],"waiting":0,"stepped":[],"error":null,"quick":null};

/** Where a new part's pins go, unturned, for the kinds the sample shows:
 *  Digital's layouts as the core places them. Others are drawn as a box
 *  with no pins — this is a stand-in, the core places them all. */
const PINS = /** @type {Record<string, [number, number, boolean][]>} */ ({
  in: [[0, 0, false]], clock: [[0, 0, false]], const: [[0, 0, false]], out: [[0, 0, true]], led: [[0, 0, true]], tunnel: [[0, 0, true]],
  not: [[0, 0, true], [2, 0, false]],
  and: [[0, 0, true], [0, 2, true], [3, 1, false]], or: [[0, 0, true], [0, 2, true], [3, 1, false]], xor: [[0, 0, true], [0, 2, true], [3, 1, false]],
  nand: [[0, 0, true], [0, 2, true], [4, 1, false]], nor: [[0, 0, true], [0, 2, true], [4, 1, false]], xnor: [[0, 0, true], [0, 2, true], [4, 1, false]],
});
const PREFIX = /** @type {Record<string, string>} */ ({ in: "In", out: "Out", clock: "Clk", const: "K", tunnel: "T", split: "P" });

/** A pin turned a quarter about its part's origin, as the core turns it. @param {any} part @param {any} pin */
const turn = (part, pin) => ({ ...pin, x: part.x + (pin.y - part.y), y: part.y - (pin.x - part.x) });

/** @param {(events: any[]) => void} _deliver */
export function createCircuitPreview(_deliver) {
  /** @type {{path: string, name: string}[]} */
  const projects = [{ path: "labs/adder.circuit", name: "adder" }];
  /** @type {null | {path: string, name: string, top: string, circuits: string[], shown: string, generation: number, dirty: boolean}} */
  let open = null;
  let generations = 0;
  /** The circuits as edited, and the steps to undo: whole copies. */
  let drawn = structuredClone(DRAWN);
  /** @type {string[]} */
  let done = [];
  /** @type {string[]} */
  let undone = [];
  /** @type {string[]} */
  let made = [];
  /** Test tables by circuit, then label. @type {Record<string, Record<string, string>>} */
  const tables = { main: { table: "A B Cin Cout S\nloop(i, 8)\n(i>>2&1) (i>>1&1) (i&1) (((i>>2&1)+(i>>1&1)+(i&1))>>1) (((i>>2&1)+(i>>1&1)+(i&1))&1)\nend loop\n" } };

  const view = () => {
    if (!open) return { type: "circuitProject", project: null };
    const d = drawn[open.shown] ?? { parts: [], wires: [] };
    return {
      type: "circuitProject",
      project: { path: open.path, name: open.name, top: open.top, circuits: open.circuits, shown: open.shown, parts: d.parts, wires: d.wires,
        tests: Object.keys(tables[open.shown] ?? {}), tables: Object.entries(tables[open.shown] ?? {}), data: [], generation: open.generation, dirty: open.dirty,
        canUndo: done.length > 0, canRedo: undone.length > 0, made,
        palette: ["in", "out", "clock", "const", "not", "and", "or", "xor", "nand", "nor", "tunnel", ...open.circuits.filter((c) => c !== open?.shown && c !== "main").map((c) => `sub:${c}`)] },
    };
  };
  const home = () => ({ type: "circuitHome", projects });
  const notice = (/** @type {string} */ message) => ({ type: "notice", message, level: "warn" });

  let running = false;
  /** The canned run, for the sample's main circuit as it was dumped. */
  const runView = () => {
    const pristine = open && JSON.stringify(drawn.main) === JSON.stringify(DRAWN.main);
    return { type: "circuitRun", run: { ...RUNNING, generation: open?.generation ?? 0, circuit: open?.shown ?? "main",
      ...(open?.shown === "main" && pristine ? {} : { wires: [], values: [] }) } };
  };

  /** An edit's ops, applied to the circuit shown. @param {any[]} ops */
  function edit(ops) {
    if (!open) return [];
    const d = drawn[open.shown] ?? (drawn[open.shown] = { parts: [], wires: [] });
    done.push(JSON.stringify(drawn));
    undone = [];
    made = [];
    const part = (/** @type {string} */ id) => d.parts.find((/** @type {any} */ p) => p.id === id);
    const sameWire = (/** @type {number[]} */ a, /** @type {number[]} */ b) => a.join() === b.join();
    for (const op of ops) {
      if (op.op === "place") {
        const prefix = PREFIX[op.kind] ?? (op.kind.startsWith("sub:") ? "S" : "U");
        let n = 1;
        while (part(`${prefix}${n}`)) n++;
        const id = `${prefix}${n}`;
        const settings = ["in", "out", "clock"].includes(op.kind) ? [["label", id]] : [];
        d.parts.push({ id, kind: op.kind, x: op.x, y: op.y, rot: 0, mirror: false, settings, problem: null,
          pins: (PINS[op.kind] ?? []).map(([x, y, input]) => ({ x: op.x + x, y: op.y + y, input })) });
        made.push(id);
      } else if (op.op === "move") {
        for (const id of op.ids) {
          const p = part(id);
          if (!p) continue;
          p.x += op.dx; p.y += op.dy;
          p.pins = p.pins.map((/** @type {any} */ q) => ({ ...q, x: q.x + op.dx, y: q.y + op.dy }));
        }
        d.wires = d.wires.map((/** @type {number[]} */ w) => (op.wires.some((/** @type {number[]} */ m) => sameWire(m, w)) ? [w[0] + op.dx, w[1] + op.dy, w[2] + op.dx, w[3] + op.dy] : w));
      } else if (op.op === "rotate") {
        for (const id of op.ids) {
          const p = part(id);
          if (!p) continue;
          p.rot = (p.rot + 1) % 4;
          p.pins = p.pins.map((/** @type {any} */ q) => turn(p, q));
        }
      } else if (op.op === "delete") {
        d.parts = d.parts.filter((/** @type {any} */ p) => !op.ids.includes(p.id));
        d.wires = d.wires.filter((/** @type {number[]} */ w) => !op.wires.some((/** @type {number[]} */ m) => sameWire(m, w)));
      } else if (op.op === "wire") {
        for (let i = 1; i < op.points.length; i++) d.wires.push([...op.points[i - 1], ...op.points[i]]);
      } else if (op.op === "set") {
        const p = part(op.id);
        if (p) {
          p.settings = op.settings.filter((/** @type {string[]} */ kv) => kv[1] !== "");
          if (op.newId) p.id = op.newId;
        }
      } else {
        done.pop();
        return [notice("the preview does not do that; the app does"), view()];
      }
    }
    open.dirty = true;
    return [view()];
  }

  /** @param {string[]} from @param {string[]} to */
  function step(from, to) {
    const last = from.pop();
    if (!open || !last) return [];
    to.push(JSON.stringify(drawn));
    drawn = JSON.parse(last);
    made = [];
    open.dirty = true;
    return [view()];
  }

  return {
    sync: () => [home(), view()],
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "circuitOpen":
          if (open && open.path === command.path) return [home(), view()];
          if (open?.dirty) return [notice("save or discard the open circuit's changes before opening another")];
          if (!projects.some((p) => p.path === command.path)) return [notice(`${command.path} is not a circuit`)];
          open = { path: command.path, name: "adder", top: "main", circuits: ["half", "main"], shown: "main", generation: ++generations, dirty: false };
          return [home(), view()];
        case "circuitNew": {
          const slug = String(command.name).toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-|-$/g, "") || "circuit";
          const path = `${slug}.circuit`;
          if (projects.some((p) => p.path === path)) return [notice(`${path} is already there`)];
          projects.push({ path, name: slug });
          projects.sort((a, b) => a.path.localeCompare(b.path));
          open = { path, name: slug, top: "main", circuits: ["main"], shown: "main", generation: ++generations, dirty: false };
          return [home(), view()];
        }
        case "circuitClose":
          if (open?.dirty && !command.discard) return [notice("the circuit has unsaved changes")];
          open = null;
          running = false;
          return [home(), view(), { type: "circuitRun", run: null }];
        case "circuitSave":
          if (!open) return [];
          open.dirty = false;
          return [home(), view()];
        case "circuitShow":
          if (!open || !open.circuits.includes(command.circuit)) return [notice(`there is no circuit ${command.circuit}`)];
          open.shown = command.circuit;
          made = [];
          return [view()];
        case "circuitAdd": {
          if (!open) return [];
          const name = String(command.name).trim();
          if (open.circuits.includes(name)) return [notice(`there is a circuit ${name} already`)];
          open.circuits = [...open.circuits, name].sort();
          open.shown = name;
          open.dirty = true;
          return [view()];
        }
        case "circuitStart":
          if (!open) return [];
          running = true;
          return [runView()];
        case "circuitStop":
          running = false;
          return [{ type: "circuitRun", run: null }];
        case "circuitSet": case "circuitClock": case "circuitStep": case "circuitQuick":
          return [notice("the preview shows one running state; running a circuit is the app's")];
        case "circuitTestSet": {
          if (!open) return [];
          const t = (tables[command.circuit] ??= {});
          if (command.text === null) delete t[command.label];
          else t[command.label] = command.text.endsWith("\n") ? command.text : `${command.text}\n`;
          open.dirty = true;
          return [view()];
        }
        case "circuitTestRun": case "circuitTraceToData": case "circuitTraceClear":
          return [notice("the preview does not run tables or record; the app does")];
        case "circuitEdit":
          return edit(command.ops);
        case "circuitUndo":
          return step(done, undone);
        case "circuitRedo":
          return step(undone, done);
        case "circuitCopy": {
          const d = open && drawn[open.shown];
          if (!d) return [];
          const lines = d.parts.filter((/** @type {any} */ p) => command.ids.includes(p.id))
            .map((/** @type {any} */ p) => `part ${p.id} ${p.kind} at ${p.x},${p.y}${p.rot ? ` rot ${p.rot}` : ""}${p.settings.map((/** @type {string[]} */ kv) => ` ${kv[0]}=${kv[1]}`).join("")}`);
          return [{ type: "circuitClip", text: lines.map((l) => `${l}\n`).join("") }];
        }
        case "circuitSetTop":
          if (!open || !open.circuits.includes(command.circuit)) return [];
          open.top = command.circuit;
          open.dirty = true;
          return [view()];
        default:
          return [];
      }
    },
  };
}
