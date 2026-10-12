/* The PDF mode's stand-ins for the browser preview: tabs over the test
 * fixtures, each drawn from the fixture itself — the preview has no core to
 * hold a view, so a view's key is the file's address, which readDocument
 * fetches. Scripted; the real tabs are proton-core's `pdf`. */

/** The PDFs the preview's "project" has. */
const PROJECT = ["tests/ui/fixtures/three-pages.pdf", "tests/ui/fixtures/outline.pdf", "tests/ui/fixtures/scan.pdf", "tests/ui/fixtures/cjk.pdf"];

/** What the preview only reads, and does not answer, of the core's commands. */
const QUIET = new Set(["pdfAt", "pdfOcrStop"]);

/** @param {(event: any) => void} _deliver */
export function createPdfPreview(_deliver) {
  /** @type {{id: number, name: string, path: string, dirty: boolean}[]} */
  const tabs = [];
  /** @type {number | null} */
  let active = null;
  /** @type {string[]} */
  const recent = [];
  let next = 0;
  /** Whether it has said that changes are the app's. */
  let said = false;

  const tabsEvent = () => ({ type: "pdfTabs", tabs: tabs.map((t) => ({ ...t, path: t.path })), active, recent: [...recent], project: PROJECT, root: "/" });
  const viewEvent = (/** @type {{id: number, path: string}} */ t, /** @type {number | null} */ page) =>
    ({ type: "pdfView", tab: t.id, key: `/${t.path.replace(/^\//, "")}`, version: 1, page, quiet: false });

  return {
    /** What the window sees on entering the mode. */
    entered() {
      const on = tabs.find((t) => t.id === active);
      return on ? [tabsEvent(), viewEvent(on, null)] : [tabsEvent()];
    },
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "pdfOpen": {
          const path = String(command.path ?? "").replace(/^\//, "");
          if (!path.endsWith(".pdf")) return [{ type: "notice", text: `${path}: not a PDF`, level: "error" }];
          let tab = tabs.find((t) => t.path === path);
          if (!tab) {
            tab = { id: ++next, name: path.split("/").pop() ?? path, path, dirty: false };
            tabs.push(tab);
          }
          active = tab.id;
          if (!recent.includes(path)) recent.unshift(path);
          return [tabsEvent(), viewEvent(tab, command.page ?? null)];
        }
        case "pdfFocus": {
          const tab = tabs.find((t) => t.id === command.tab);
          if (!tab) return [];
          active = tab.id;
          return [tabsEvent(), viewEvent(tab, null)];
        }
        case "pdfClose": {
          const at = tabs.findIndex((t) => t.id === command.tab);
          if (at < 0) return [];
          tabs.splice(at, 1);
          if (active === command.tab) active = (tabs[at] ?? tabs[tabs.length - 1])?.id ?? null;
          const on = tabs.find((t) => t.id === active);
          return on ? [tabsEvent(), viewEvent(on, null)] : [tabsEvent()];
        }
        case "pdfOcrLangs":
          // No tesseract in a browser: OCR says what it would need.
          return [{ type: "pdfOcrLangs", langs: [] }];
        default:
          // A change: the core's, which the preview has none of. Said once.
          if (QUIET.has(command.type) || said) return [];
          said = true;
          return [{ type: "notice", text: "The preview only draws PDFs: changing one is the app's (its core holds every change).", level: "info" }];
      }
    },
  };
}
