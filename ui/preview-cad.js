/* CAD for the browser preview: a pretend folder of parts, and the sample
 * part as the core draws it (preview-cad-part.js, dumped from
 * crates/proton-core). The kernel is the core's: this shows the sample,
 * holds what is typed, and builds nothing — it must never grow into a
 * second kernel. */

import { PART } from "./preview-cad-part.js";
import { ASSEMBLY } from "./preview-cad-assembly.js";
import { DRAWING } from "./preview-cad-drawing.js";
import { SCHEMATIC } from "./preview-cad-schematic.js";
import { BOARD } from "./preview-cad-board.js";

/** A few of the library's families, as the core lists them. */
const FAMILIES = [
  { name: "fan", title: "Fan", group: "Cooling", params: [{ name: "size", default: "120", choices: ["80", "92", "120", "140"], note: "square, mm" }, { name: "thickness", default: "25", choices: ["15", "25", "38"], note: "mm" }] },
  { name: "gpu", title: "Graphics card", group: "Graphics", params: [{ name: "length", default: "304", choices: [], note: "mm" }, { name: "slots", default: "3", choices: [], note: "slots" }] },
  { name: "formd-t1", title: "FormD T1 (V2.1)", group: "Cases", params: [{ name: "wall", default: "1.5", choices: [], note: "mm" }] },
];

/** @param {(events: any[]) => void} _deliver */
export function createCadPreview(_deliver) {
  const parts = [{ path: "case/foot.part", name: "foot", kind: "part" }, { path: "pump-bracket.part", name: "pump-bracket", kind: "part" }, { path: "rig.assembly", name: "rig", kind: "assembly" }, { path: "rig.drawing", name: "rig", kind: "drawing" }, { path: "hub.schematic", name: "hub", kind: "schematic" }, { path: "hub.board", name: "hub", kind: "board" }];
  /** @type {any} */
  let open = null;
  let generations = 0;
  const home = () => ({ type: "cadHome", parts });
  const partEvent = () => ({ type: "cadPart", part: open && !open.asm && !open.drw && !open.sch && !open.brd ? { ...open, dirty: open.text !== open.saved } : null });
  const asmEvent = () => ({ type: "cadAssembly", assembly: open?.asm ? { ...open, dirty: open.text !== open.saved } : null });
  const drwEvent = () => ({ type: "cadDrawing", drawing: open?.drw ? { ...open, dirty: open.text !== open.saved } : null });
  const schEvent = () => ({ type: "cadSchematic", schematic: open?.sch ? { ...open, dirty: open.text !== open.saved } : null });
  const brdEvent = () => ({ type: "cadBoard", board: open?.brd ? { ...open, dirty: open.text !== open.saved } : null });
  const docEvents = () => [partEvent(), asmEvent(), drwEvent(), schEvent(), brdEvent()];
  const notice = (/** @type {string} */ message) => ({ type: "notice", message, level: "info" });
  /** @param {string} path @param {any} part */
  const show = (path, part) => {
    open = { ...structuredClone(part), path, generation: ++generations, saved: part.text };
  };
  const blank = (/** @type {string} */ name) => ({ ...PART, name, title: name, text: `part "${name}"\nunits mm\n`, material: null, params: [], steps: [], bodies: [], sketches: [], errors: [], notes: [], canUndo: false, canRedo: false, bounds: [-50, -50, -50, 50, 50, 50] });
  return {
    sync: () => [home(), { type: "cadFamilies", families: FAMILIES }, ...docEvents()],
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "cadOpen":
          if (open?.path === command.path) return [home(), ...docEvents()];
          if (open && open.text !== open.saved) return [{ type: "notice", message: "save or discard the open part's changes before opening another", level: "warn" }];
          if (command.path === "rig.assembly") {
            show(command.path, ASSEMBLY);
            open.asm = true;
          } else if (command.path === "hub.board") {
            show(command.path, BOARD);
            open.brd = true;
          } else if (command.path === "hub.schematic") {
            show(command.path, SCHEMATIC);
            open.sch = true;
          } else if (command.path === "rig.drawing") {
            show(command.path, DRAWING);
            open.drw = true;
          } else show(command.path, command.path === "pump-bracket.part" ? PART : blank("foot"));
          return docEvents();
        case "cadDraw":
          if (!open || command.generation !== open.generation) return [];
          if (open.text !== open.saved) return [{ type: "notice", message: "save it first: its drawing reads it from its file", level: "warn" }];
          show("rig.drawing", DRAWING);
          open.drw = true;
          return [notice("opened rig.drawing, which was there — the browser preview has the sample blueprint only"), ...docEvents()];
        case "cadNew": {
          const path = `${String(command.name).toLowerCase().replace(/[^a-z0-9]+/g, "-")}.part`;
          parts.push({ path, name: path.replace(/\.part$/, ""), kind: "part" });
          show(path, blank(command.name));
          return [home(), partEvent(), notice(`made ${path}`)];
        }
        case "cadClose":
          if (!command.discard && open && open.text !== open.saved) return [{ type: "notice", message: "the part has unsaved changes", level: "warn" }];
          open = null;
          return [home(), ...docEvents()];
        case "cadSave":
          if (open) open.saved = open.text;
          return [home(), ...docEvents()];
        case "cadText":
          if (!open || command.generation !== open.generation) return [];
          open.text = command.text;
          return docEvents();
        case "cadLibrary":
        case "cadNewAssembly":
        case "cadNewSchematic":
        case "cadMakeBoard":
        case "cadFit":
        case "cadEdit":
        case "cadDrag":
        case "cadUndo":
        case "cadRedo":
          return [notice("The browser preview has no kernel: drawing and building happen in the app.")];
        case "cadExport":
          return [notice(`The browser preview writes no files: in the app this writes ${open?.path.replace(/\.(part|assembly|drawing)$/, `.${command.format}`)}.`)];
      }
      return [];
    },
  };
}
