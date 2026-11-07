/* CAD for the browser preview: a pretend folder of parts, and the sample
 * part as the core draws it (preview-cad-part.js, dumped from
 * crates/proton-core). The kernel is the core's: this shows the sample,
 * holds what is typed, and builds nothing — it must never grow into a
 * second kernel. */

import { PART } from "./preview-cad-part.js";

/** @param {(events: any[]) => void} _deliver */
export function createCadPreview(_deliver) {
  const parts = [{ path: "case/foot.part", name: "foot" }, { path: "pump-bracket.part", name: "pump-bracket" }];
  /** @type {any} */
  let open = null;
  let generations = 0;
  const home = () => ({ type: "cadHome", parts });
  const partEvent = () => ({ type: "cadPart", part: open ? { ...open, dirty: open.text !== open.saved } : null });
  const notice = (/** @type {string} */ message) => ({ type: "notice", message, level: "info" });
  /** @param {string} path @param {any} part */
  const show = (path, part) => {
    open = { ...structuredClone(part), path, generation: ++generations, saved: part.text };
  };
  const blank = (/** @type {string} */ name) => ({ ...PART, name, title: name, text: `part "${name}"\nunits mm\n`, material: null, params: [], steps: [], bodies: [], sketches: [], errors: [], notes: [], canUndo: false, canRedo: false, bounds: [-50, -50, -50, 50, 50, 50] });
  return {
    sync: () => [home(), partEvent()],
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "cadOpen":
          if (open?.path === command.path) return [home(), partEvent()];
          if (open && open.text !== open.saved) return [{ type: "notice", message: "save or discard the open part's changes before opening another", level: "warn" }];
          show(command.path, command.path === "pump-bracket.part" ? PART : blank("foot"));
          return [partEvent()];
        case "cadNew": {
          const path = `${String(command.name).toLowerCase().replace(/[^a-z0-9]+/g, "-")}.part`;
          parts.push({ path, name: path.replace(/\.part$/, "") });
          show(path, blank(command.name));
          return [home(), partEvent(), notice(`made ${path}`)];
        }
        case "cadClose":
          if (!command.discard && open && open.text !== open.saved) return [{ type: "notice", message: "the part has unsaved changes", level: "warn" }];
          open = null;
          return [home(), partEvent()];
        case "cadSave":
          if (open) open.saved = open.text;
          return [home(), partEvent()];
        case "cadText":
          if (!open || command.generation !== open.generation) return [];
          open.text = command.text;
          return [partEvent()];
        case "cadEdit":
        case "cadDrag":
        case "cadUndo":
        case "cadRedo":
          return [notice("The browser preview has no kernel: drawing and building happen in the app.")];
        case "cadExport":
          return [notice(`The browser preview writes no files: in the app this writes ${open?.path.replace(/\.part$/, `.${command.format}`)}.`)];
      }
      return [];
    },
  };
}
