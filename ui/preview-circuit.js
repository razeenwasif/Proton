/* Circuit for the browser preview: one project in a pretend folder, a
 * full adder made of two half adders, so the drawing can be designed and
 * tested against. Each circuit is as the core sent it — dumped from
 * crates/proton-core with the pins it placed — because placing pins is the
 * simulator's to do and this must never grow into a second. */

/** The circuits, as drawn. @type {Record<string, {parts: any[], wires: number[][]}>} */
const DRAWN = {"main":{"parts":[{"id":"A","kind":"in","x":0,"y":0,"rot":0,"mirror":false,"settings":[["label","A"]],"pins":[{"x":0,"y":0,"input":false}],"problem":null},{"id":"B","kind":"in","x":0,"y":1,"rot":0,"mirror":false,"settings":[["label","B"]],"pins":[{"x":0,"y":1,"input":false}],"problem":null},{"id":"Cin","kind":"in","x":0,"y":4,"rot":0,"mirror":false,"settings":[["label","Cin"]],"pins":[{"x":0,"y":4,"input":false}],"problem":null},{"id":"Cout","kind":"out","x":16,"y":1,"rot":0,"mirror":false,"settings":[["label","Cout"]],"pins":[{"x":16,"y":1,"input":true}],"problem":null},{"id":"H1","kind":"sub:half","x":2,"y":0,"rot":0,"mirror":false,"settings":[],"pins":[{"x":2,"y":0,"input":true},{"x":2,"y":1,"input":true},{"x":5,"y":0,"input":false},{"x":5,"y":1,"input":false}],"problem":null},{"id":"H2","kind":"sub:half","x":7,"y":1,"rot":0,"mirror":false,"settings":[],"pins":[{"x":7,"y":1,"input":true},{"x":7,"y":2,"input":true},{"x":10,"y":1,"input":false},{"x":10,"y":2,"input":false}],"problem":null},{"id":"O","kind":"or","x":12,"y":0,"rot":0,"mirror":false,"settings":[],"pins":[{"x":12,"y":0,"input":true},{"x":12,"y":2,"input":true},{"x":15,"y":1,"input":false}],"problem":null},{"id":"S","kind":"out","x":16,"y":4,"rot":0,"mirror":false,"settings":[["label","S"]],"pins":[{"x":16,"y":4,"input":true}],"problem":null}],"wires":[[0,0,2,0],[0,1,2,1],[0,4,6,4],[5,0,12,0],[5,1,7,1],[6,2,6,4],[6,2,7,2],[10,1,11,1],[10,2,10,4],[10,4,16,4],[11,1,11,2],[11,2,12,2],[15,1,16,1]]},"half":{"parts":[{"id":"A","kind":"in","x":0,"y":0,"rot":0,"mirror":false,"settings":[["label","A"]],"pins":[{"x":0,"y":0,"input":false}],"problem":null},{"id":"B","kind":"in","x":0,"y":2,"rot":0,"mirror":false,"settings":[["label","B"]],"pins":[{"x":0,"y":2,"input":false}],"problem":null},{"id":"C","kind":"out","x":8,"y":4,"rot":0,"mirror":false,"settings":[["label","C"]],"pins":[{"x":8,"y":4,"input":true}],"problem":null},{"id":"S","kind":"out","x":8,"y":0,"rot":0,"mirror":false,"settings":[["label","S"]],"pins":[{"x":8,"y":0,"input":true}],"problem":null},{"id":"X","kind":"xor","x":4,"y":0,"rot":0,"mirror":false,"settings":[],"pins":[{"x":4,"y":0,"input":true},{"x":4,"y":2,"input":true},{"x":7,"y":1,"input":false}],"problem":null},{"id":"Y","kind":"and","x":4,"y":4,"rot":0,"mirror":false,"settings":[],"pins":[{"x":4,"y":4,"input":true},{"x":4,"y":6,"input":true},{"x":7,"y":5,"input":false}],"problem":null}],"wires":[[0,0,2,0],[0,2,3,2],[2,0,2,4],[2,0,4,0],[2,4,4,4],[3,2,3,6],[3,2,4,2],[3,6,4,6],[7,1,8,1],[7,5,8,5],[8,0,8,1],[8,4,8,5]]}};

/** @param {(events: any[]) => void} _deliver */
export function createCircuitPreview(_deliver) {
  /** @type {{path: string, name: string}[]} */
  const projects = [{ path: "labs/adder.circuit", name: "adder" }];
  /** @type {null | {path: string, name: string, top: string, circuits: string[], shown: string, generation: number, dirty: boolean}} */
  let open = null;
  let generations = 0;

  const view = () => {
    if (!open) return { type: "circuitProject", project: null };
    const d = DRAWN[open.shown] ?? { parts: [], wires: [] };
    return {
      type: "circuitProject",
      project: { path: open.path, name: open.name, top: open.top, circuits: open.circuits, shown: open.shown, parts: d.parts, wires: d.wires,
        tests: open.shown === "main" ? ["table"] : [], generation: open.generation, dirty: open.dirty },
    };
  };
  const home = () => ({ type: "circuitHome", projects });
  const notice = (/** @type {string} */ message) => ({ type: "notice", message, level: "warn" });

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
          return [home(), view()];
        case "circuitSave":
          if (!open) return [];
          open.dirty = false;
          return [home(), view()];
        case "circuitShow":
          if (!open || !open.circuits.includes(command.circuit)) return [notice(`there is no circuit ${command.circuit}`)];
          open.shown = command.circuit;
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
