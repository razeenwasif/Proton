/* Excel for the browser preview: two workbooks in a pretend project, one
 * open with a budget in it, so the grid can be designed and tested against.
 * Formulas here are sums and arithmetic of cells and no more — the real
 * engine is crates/proton-sheet, and this must never grow into a second. */

/** @param {number} col */
const colName = (col) => {
  let s = "";
  for (let n = col + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s;
  return s;
};
/** @param {string} name @returns {[number, number] | null} */
const parse = (name) => {
  const m = /^\$?([A-Z]{1,3})\$?(\d+)$/i.exec(name.trim());
  if (!m) return null;
  let c = 0;
  for (const ch of m[1].toUpperCase()) c = c * 26 + ch.charCodeAt(0) - 64;
  return [Number(m[2]) - 1, c - 1];
};

/** `+ - * /` and brackets over numbers, or null. @param {string} src @returns {number | null} */
function arithmetic(src) {
  const t = src.replace(/\s+/g, "");
  let i = 0;
  /** @returns {number | null} */
  const atom = () => {
    if (t[i] === "-") { i++; const a = atom(); return a === null ? null : -a; }
    if (t[i] === "(") { i++; const a = sum(); if (t[i] !== ")") return null; i++; return a; }
    const m = /^\d*\.?\d+/.exec(t.slice(i));
    if (!m) return null;
    i += m[0].length;
    return Number(m[0]);
  };
  /** @returns {number | null} */
  const product = () => {
    let a = atom();
    while (a !== null && (t[i] === "*" || t[i] === "/")) { const op = t[i++]; const b = atom(); if (b === null) return null; a = op === "*" ? a * b : a / b; }
    return a;
  };
  /** @returns {number | null} */
  const sum = () => {
    let a = product();
    while (a !== null && (t[i] === "+" || t[i] === "-")) { const op = t[i++]; const b = product(); if (b === null) return null; a = op === "+" ? a + b : a - b; }
    return a;
  };
  const n = sum();
  return i === t.length ? n : null;
}

const BUDGET = {
  A1: "Item", B1: "Cost", C1: "Paid",
  A2: "Rent", B2: "1450", C2: "TRUE",
  A3: "Groceries", B3: "386.20", C3: "TRUE",
  A4: "Power", B4: "92.75", C4: "FALSE",
  A5: "Internet", B5: "69", C5: "TRUE",
  A7: "Total", B7: "=SUM(B2:B5)",
};

/** @param {(event: any) => void} deliver */
export function createSheetPreview(deliver) {
  const workbooks = [
    { path: "budget.sheet", name: "budget", kind: "folder" },
    { path: "data/people.csv", name: "people", kind: "csv" },
  ];
  /** @type {{path: string, name: string, kind: string, sheets: {name: string, cells: Map<string, string>, frozen?: number[], widths?: Map<number, number>}[], active: number, generation: number, version: number, done: any[], undone: any[], saved: number, state: number} | null} */
  let book = null;
  let generations = 0;
  let states = 0;
  /** @type {{sheet: number, rows: number[], cols: number[]} | null} */
  let view = null;

  /** @param {string} path @param {Record<string, string>} cells */
  function open(path, cells) {
    const found = workbooks.find((w) => w.path === path) ?? { path, name: path.replace(/\..*$/, ""), kind: "folder" };
    book = { ...found, sheets: [{ name: "Sheet1", cells: new Map(Object.entries(cells)) }], active: 0, generation: ++generations, version: 0, done: [], undone: [], saved: 0, state: 0 };
  }

  /** What a cell comes to: its value, or a formula's of sums and arithmetic. @param {number} s @param {string} name @param {number} [depth] @returns {string | number | boolean} */
  function value(s, name, depth = 0) {
    const input = book?.sheets[s]?.cells.get(name) ?? "";
    if (!input.startsWith("=")) {
      if (input === "TRUE" || input === "FALSE") return input === "TRUE";
      const n = Number(input.replace(/,/g, ""));
      return input !== "" && Number.isFinite(n) ? n : input.replace(/^'/, "");
    }
    if (depth > 20) return "#CYCLE!";
    const num = (/** @type {string} */ ref) => {
      const v = value(s, ref.toUpperCase(), depth + 1);
      return typeof v === "number" ? v : 0;
    };
    const expr = input.slice(1).replace(/SUM\(([A-Z]+\d+):([A-Z]+\d+)\)/gi, (_, a, b) => {
      const [r0, c0] = parse(a) ?? [0, 0], [r1, c1] = parse(b) ?? [0, 0];
      let total = 0;
      for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) total += num(`${colName(c)}${r + 1}`);
      return String(total);
    }).replace(/\$?[A-Z]{1,3}\$?\d+/gi, (ref) => String(num(ref.replace(/\$/g, ""))));
    if (!/^[\d.+\-*/() ]*$/.test(expr)) return "#NAME?";
    const n = arithmetic(expr || "0");
    return n === null ? "#VALUE!" : Number.isFinite(n) ? n : "#DIV/0!";
  }

  /** @param {number} s @param {string} name */
  function cellView(s, name) {
    const [row, col] = parse(name) ?? [0, 0];
    const input = book?.sheets[s]?.cells.get(name) ?? "";
    const v = value(s, name);
    const kind = input === "" ? "" : typeof v === "number" ? "n" : typeof v === "boolean" ? "b" : String(v).startsWith("#") ? "e" : "t";
    const text = typeof v === "boolean" ? (v ? "TRUE" : "FALSE") : typeof v === "number" ? String(Math.round(v * 1e10) / 1e10) : v;
    return { row, col, text, input, style: 0, kind, color: null };
  }

  const bookEvent = () => ({
    type: "sheetBook",
    book: book && {
      path: book.path, name: book.name, kind: book.kind, active: book.active, generation: book.generation, version: book.version,
      dirty: book.state !== book.saved, canUndo: book.done.length > 0, canRedo: book.undone.length > 0, dropped: [],
      styles: [{ bold: false, italic: false, underline: false, color: null, fill: null, format: "", align: null, wrap: false }],
      sheets: book.sheets.map((sh) => {
        let rows = 0, cols = 0;
        for (const name of sh.cells.keys()) {
          const [r, c] = parse(name) ?? [0, 0];
          rows = Math.max(rows, r + 1);
          cols = Math.max(cols, c + 1);
        }
        return { name: sh.name, rows, cols, frozen: sh.frozen ?? [0, 0], widths: [...(sh.widths ?? new Map())] };
      }),
    },
  });

  function cellsEvent() {
    if (!book || !view || view.sheet !== book.active) return null;
    const sheet = view.sheet;
    const cells = [...(book.sheets[sheet]?.cells.keys() ?? [])]
      .map((name) => cellView(sheet, name))
      .filter((c) => c.row >= view.rows[0] && c.row <= view.rows[1] && c.col >= view.cols[0] && c.col <= view.cols[1]);
    return { type: "sheetCells", generation: book.generation, version: book.version, sheet, rows: view.rows, cols: view.cols, cells, partial: false };
  }

  /** Cells put into the open sheet, as one step. @param {number} s @param {[string, string][]} cells @param {boolean} [record] */
  function put(s, cells, record = true) {
    if (!book) return;
    const sheet = book.sheets[s];
    const before = cells.map(([name]) => [name, sheet.cells.get(name) ?? ""]);
    for (const [name, input] of cells) {
      if (input) sheet.cells.set(name, input);
      else sheet.cells.delete(name);
    }
    if (record) {
      book.state = ++states;
      book.done.push({ s, before, after: cells, state: book.state });
      book.undone = [];
    }
    book.version++;
  }

  open("budget.sheet", BUDGET);
  // A workbook made since: only what a test puts in it.
  const made = /** @type {Record<string, Record<string, string>>} */ ({});

  return {
    sync() {
      return [{ type: "sheetHome", workbooks }, bookEvent()];
    },
    /** @param {any} command */
    command(command) {
      switch (command.type) {
        case "sheetOpen":
          if (book?.path !== command.path) open(command.path, command.path === "budget.sheet" ? BUDGET : made[command.path] ?? { A1: "name", B1: "age", A2: "Ada", B2: "36" });
          view = null;
          return [{ type: "sheetHome", workbooks }, bookEvent()];
        case "sheetNew": {
          const path = command.folder ?? `${command.name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.sheet`;
          workbooks.push({ path, name: command.name, kind: path.endsWith(".xlsx") ? "xlsx" : path.endsWith(".csv") ? "csv" : "folder" });
          made[path] = {};
          open(path, {});
          view = null;
          return [{ type: "sheetHome", workbooks }, bookEvent(), { type: "notice", text: `made ${command.name} in ${path}`, level: "info" }];
        }
        case "sheetClose":
          if (book && book.state !== book.saved && !command.discard) return [{ type: "notice", text: "the workbook has unsaved changes", level: "warn" }];
          book = null;
          return [bookEvent()];
        case "sheetSave":
          if (!book) return [];
          book.saved = book.state;
          return [bookEvent(), { type: "notice", text: `saved ${book.path}`, level: "info" }];
        case "sheetView":
          if (!book || command.generation !== book.generation) return [];
          view = { sheet: command.sheet, rows: command.rows, cols: command.cols };
          return [cellsEvent()].filter(Boolean);
        case "sheetSet":
          if (!book || command.generation !== book.generation) return [{ type: "notice", text: "that workbook is no longer open", level: "warn" }];
          put(command.sheet, command.cells);
          return [bookEvent(), cellsEvent()].filter(Boolean);
        case "sheetUndo":
        case "sheetRedo": {
          if (!book) return [];
          const back = command.type === "sheetUndo";
          const step = (back ? book.done : book.undone).pop();
          if (!step) return [];
          put(step.s, back ? step.before : step.after, false);
          (back ? book.undone : book.done).push(step);
          book.state = back ? book.done.at(-1)?.state ?? 0 : step.state;
          book.active = step.s;
          return [bookEvent(), cellsEvent()].filter(Boolean);
        }
        case "sheetTab":
          if (book) book.active = Math.min(command.sheet, book.sheets.length - 1);
          return [bookEvent()];
        case "sheetAddSheet":
          if (!book) return [];
          book.sheets.splice(book.active + 1, 0, { name: command.name ?? `Sheet${book.sheets.length + 1}`, cells: new Map() });
          book.active++;
          book.state = ++states;
          return [bookEvent()];
        case "sheetRenameSheet":
          if (book?.sheets[command.sheet]) book.sheets[command.sheet].name = command.name;
          if (book) book.state = ++states;
          return [bookEvent()];
        case "sheetRemoveSheet":
          if (!book || book.sheets.length < 2) return [{ type: "notice", text: "a workbook keeps at least one sheet", level: "warn" }];
          book.sheets.splice(command.sheet, 1);
          book.active = Math.min(book.active, book.sheets.length - 1);
          book.state = ++states;
          return [bookEvent(), cellsEvent()].filter(Boolean);
        case "sheetFreeze":
          if (book) book.sheets[command.sheet].frozen = [command.rows, command.cols];
          return [bookEvent(), cellsEvent()].filter(Boolean);
        case "sheetWidth": {
          if (!book) return [];
          const sh = book.sheets[command.sheet];
          sh.widths ??= new Map();
          if (command.width === null) sh.widths.delete(command.col);
          else sh.widths.set(command.col, command.width);
          return [bookEvent(), cellsEvent()].filter(Boolean);
        }
        case "sheetCopy": {
          if (!book) return [];
          const [a, b = a] = command.range.split(":");
          const [r0, c0] = parse(a) ?? [0, 0], [r1, c1] = parse(b) ?? [0, 0];
          const lines = [];
          for (let r = r0; r <= r1; r++) {
            const line = [];
            for (let c = c0; c <= c1; c++) line.push(cellView(command.sheet, `${colName(c)}${r + 1}`).text);
            lines.push(line.join("\t"));
          }
          return [{ type: "sheetClip", text: lines.join("\n") + "\n" }];
        }
        case "sheetPaste": {
          if (!book) return [];
          const [r0, c0] = parse(command.at) ?? [0, 0];
          const cells = command.text.replace(/\r/g, "").replace(/\n$/, "").split("\n")
            .flatMap((/** @type {string} */ line, /** @type {number} */ r) => line.split("\t").map((v, c) => [`${colName(c0 + c)}${r0 + r + 1}`, v]));
          put(command.sheet, cells);
          return [bookEvent(), cellsEvent()].filter(Boolean);
        }
        default:
          // Fills, formats, rows and columns are the real engine's.
          return command.type.startsWith("sheet") ? [{ type: "notice", text: "the preview does not do that: the app does", level: "info" }] : [];
      }
    },
  };
}
