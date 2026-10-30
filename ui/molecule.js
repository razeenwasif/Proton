/* Molecules and proteins as the Lab reads them: XYZ (frames of a trajectory
 * too), PDB and mmCIF, MOL and SDF, and SMILES — each read into atoms with
 * their places, bonds found where a file gives none, and a protein's
 * backbone with its helices and strands. Pure: ui/lab-molecule.js draws what
 * this reads, and the unit tests read it without a window.
 *
 * SMILES gives a graph and no places, so the atoms are laid out here: bond
 * lengths from covalent radii, angles from each atom's bonds, and nothing
 * closer than it should be. It is a shape to look at, not a conformer
 * search: no stereo, and one of the shapes the graph allows. */

/** @typedef {{el: string, x: number, y: number, z: number, name?: string, res?: string, seq?: number, ins?: string, chain?: string, het?: boolean}} Atom */
/** @typedef {[number, number, number]} Bond  i, j and an order: 1, 2, 3, or 1.5 aromatic */
/** @typedef {{name: string, atoms: Atom[], bonds: Bond[] | null, sheets?: Span[], helices?: Span[], pending?: () => {atoms: Atom[], bonds: Bond[]}}} Model */
/** @typedef {{chain: string, from: number, to: number}} Span */
/** @typedef {{title: string, kind: string, models: Model[], note?: string}} Molecule */

/* ── elements ─────────────────────────────────────────────────────── */

// Symbol: [name, covalent radius Å (Cordero 2008), van der Waals radius Å
// (Bondi; Alvarez where Bondi has none), colour (Jmol's)].
/** @type {Record<string, [string, number, number, string]>} */
const ELEMENTS = {
  H: ["hydrogen", 0.31, 1.2, "#ffffff"], He: ["helium", 0.28, 1.4, "#d9ffff"],
  Li: ["lithium", 1.28, 1.82, "#cc80ff"], Be: ["beryllium", 0.96, 1.53, "#c2ff00"], B: ["boron", 0.84, 1.92, "#ffb5b5"],
  C: ["carbon", 0.76, 1.7, "#909090"], N: ["nitrogen", 0.71, 1.55, "#3050f8"], O: ["oxygen", 0.66, 1.52, "#ff0d0d"],
  F: ["fluorine", 0.57, 1.47, "#90e050"], Ne: ["neon", 0.58, 1.54, "#b3e3f5"],
  Na: ["sodium", 1.66, 2.27, "#ab5cf2"], Mg: ["magnesium", 1.41, 1.73, "#8aff00"], Al: ["aluminium", 1.21, 1.84, "#bfa6a6"],
  Si: ["silicon", 1.11, 2.1, "#f0c8a0"], P: ["phosphorus", 1.07, 1.8, "#ff8000"], S: ["sulfur", 1.05, 1.8, "#ffff30"],
  Cl: ["chlorine", 1.02, 1.75, "#1ff01f"], Ar: ["argon", 1.06, 1.88, "#80d1e3"],
  K: ["potassium", 2.03, 2.75, "#8f40d4"], Ca: ["calcium", 1.76, 2.31, "#3dff00"], Sc: ["scandium", 1.7, 2.11, "#e6e6e6"],
  Ti: ["titanium", 1.6, 1.87, "#bfc2c7"], V: ["vanadium", 1.53, 1.79, "#a6a6ab"], Cr: ["chromium", 1.39, 1.89, "#8a99c7"],
  Mn: ["manganese", 1.39, 1.97, "#9c7ac7"], Fe: ["iron", 1.32, 1.94, "#e06633"], Co: ["cobalt", 1.26, 1.92, "#f090a0"],
  Ni: ["nickel", 1.24, 1.63, "#50d050"], Cu: ["copper", 1.32, 1.4, "#c88033"], Zn: ["zinc", 1.22, 1.39, "#7d80b0"],
  Ga: ["gallium", 1.22, 1.87, "#c28f8f"], Ge: ["germanium", 1.2, 2.11, "#668f8f"], As: ["arsenic", 1.19, 1.85, "#bd80e3"],
  Se: ["selenium", 1.2, 1.9, "#ffa100"], Br: ["bromine", 1.2, 1.85, "#a62929"], Kr: ["krypton", 1.16, 2.02, "#5cb8d1"],
  Rb: ["rubidium", 2.2, 3.03, "#702eb0"], Sr: ["strontium", 1.95, 2.49, "#00ff00"], Mo: ["molybdenum", 1.54, 2.09, "#54b5b5"],
  Ru: ["ruthenium", 1.46, 2.07, "#248f8f"], Rh: ["rhodium", 1.42, 1.95, "#0a7d8c"], Pd: ["palladium", 1.39, 1.63, "#006985"],
  Ag: ["silver", 1.45, 1.72, "#c0c0c0"], Cd: ["cadmium", 1.44, 1.58, "#ffd98f"], In: ["indium", 1.42, 1.93, "#a67573"],
  Sn: ["tin", 1.39, 2.17, "#668080"], Sb: ["antimony", 1.39, 2.06, "#9e63b5"], Te: ["tellurium", 1.38, 2.06, "#d47a00"],
  I: ["iodine", 1.39, 1.98, "#940094"], Xe: ["xenon", 1.4, 2.16, "#429eb0"], Cs: ["caesium", 2.44, 3.43, "#57178f"],
  Ba: ["barium", 2.15, 2.68, "#00c900"], W: ["tungsten", 1.62, 2.1, "#2194d6"], Os: ["osmium", 1.44, 2.16, "#266696"],
  Ir: ["iridium", 1.41, 2.02, "#175487"], Pt: ["platinum", 1.36, 1.75, "#d0d0e0"], Au: ["gold", 1.36, 1.66, "#ffd123"],
  Hg: ["mercury", 1.32, 1.55, "#b8b8d0"], Pb: ["lead", 1.46, 2.02, "#575961"], Bi: ["bismuth", 1.48, 2.07, "#9e4fb5"],
  U: ["uranium", 1.96, 1.86, "#008fff"],
};
const UNKNOWN = /** @type {[string, number, number, string]} */ (["unknown element", 0.8, 2.0, "#ff1493"]);

/** An element's symbol as written, in its usual case: "CL" and "cl" are Cl.
 *  @param {string} s */
export function symbol(s) {
  const t = String(s ?? "").replace(/[^A-Za-z]/g, "");
  if (!t) return "X";
  const two = t[0].toUpperCase() + (t[1] ?? "").toLowerCase();
  if (t.length >= 2 && ELEMENTS[two]) return two;
  return t[0].toUpperCase();
}

/** What is known of an element. @param {string} el */
export function element(el) {
  const [name, covalent, vdw, color] = ELEMENTS[el] ?? UNKNOWN;
  return { name, covalent, vdw, color };
}

/* ── bonds from places ────────────────────────────────────────────── */

/** Bonds between atoms close enough for their covalent radii — within
 *  0.4 Å more — found through a grid, so a protein's hundred thousand atoms
 *  take a moment rather than a minute. Hydrogens bond only once.
 *  @param {Atom[]} atoms @returns {Bond[]} */
export function findBonds(atoms) {
  const cell = 2.4;
  /** @type {Map<string, number[]>} */
  const grid = new Map();
  const at = (/** @type {number} */ v) => Math.floor(v / cell);
  atoms.forEach((a, i) => {
    const k = `${at(a.x)},${at(a.y)},${at(a.z)}`;
    const list = grid.get(k);
    if (list) list.push(i);
    else grid.set(k, [i]);
  });
  const radius = atoms.map((a) => element(a.el).covalent);
  /** @type {Bond[]} */
  const out = [];
  const hydrogenBonded = new Uint8Array(atoms.length);
  atoms.forEach((a, i) => {
    const cx = at(a.x), cy = at(a.y), cz = at(a.z);
    for (let dx = -1; dx <= 1; dx++) for (let dy = -1; dy <= 1; dy++) for (let dz = -1; dz <= 1; dz++) {
      for (const j of grid.get(`${cx + dx},${cy + dy},${cz + dz}`) ?? []) {
        if (j <= i) continue;
        const b = atoms[j];
        // Two hydrogens are never bonded in a molecule worth drawing.
        if (a.el === "H" && b.el === "H") continue;
        const d = Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
        if (d < 0.4 || d > radius[i] + radius[j] + 0.4) continue;
        if ((a.el === "H" && hydrogenBonded[i]) || (b.el === "H" && hydrogenBonded[j])) continue;
        if (a.el === "H") hydrogenBonded[i] = 1;
        if (b.el === "H") hydrogenBonded[j] = 1;
        out.push([i, j, 1]);
      }
    }
  });
  return out;
}

/* ── XYZ ──────────────────────────────────────────────────────────── */

/** An XYZ file: a count, a comment, then a line an atom; several in a row
 *  are a trajectory's frames. @param {string} text @param {string} title */
export function readXyz(text, title) {
  const lines = text.split(/\r?\n/);
  /** @type {Model[]} */
  const models = [];
  let i = 0;
  while (i < lines.length) {
    if (!lines[i].trim()) { i++; continue; }
    const n = Number(lines[i].trim().split(/\s+/)[0]);
    if (!Number.isInteger(n) || n < 0) throw new Error(`line ${i + 1}: an XYZ frame starts with its number of atoms, not “${lines[i].trim().slice(0, 30)}”`);
    const name = (lines[i + 1] ?? "").trim();
    /** @type {Atom[]} */
    const atoms = [];
    for (let k = 0; k < n; k++) {
      const line = lines[i + 2 + k];
      const f = line?.trim().split(/\s+/) ?? [];
      const [x, y, z] = [Number(f[1]), Number(f[2]), Number(f[3])];
      if (f.length < 4 || ![x, y, z].every(Number.isFinite)) throw new Error(`line ${i + 3 + k}: an atom is its element and three coordinates`);
      // Some programs write atomic numbers rather than symbols.
      const el = /^\d+$/.test(f[0]) ? (BY_NUMBER[Number(f[0])] ?? "X") : symbol(f[0]);
      atoms.push({ el, x, y, z });
    }
    models.push({ name, atoms, bonds: null });
    i += 2 + n;
  }
  if (!models.length) throw new Error("there are no atoms in it");
  return { title: title || (models.length === 1 ? models[0].name : ""), kind: "XYZ", models };
}

const BY_NUMBER = [, "H", "He", "Li", "Be", "B", "C", "N", "O", "F", "Ne", "Na", "Mg", "Al", "Si", "P", "S", "Cl", "Ar", "K", "Ca",
  "Sc", "Ti", "V", "Cr", "Mn", "Fe", "Co", "Ni", "Cu", "Zn", "Ga", "Ge", "As", "Se", "Br", "Kr"];

/* ── PDB ──────────────────────────────────────────────────────────── */

/** A PDB file: ATOM and HETATM records by their columns, MODEL by MODEL, the
 *  first of each atom's alternate places, CONECT kept for HETATMs, and the
 *  HELIX and SHEET records. @param {string} text @param {string} title */
export function readPdb(text, title) {
  /** @type {Model[]} */
  const models = [];
  /** @type {Atom[]} */
  let atoms = [];
  /** @type {Map<number, number>} */
  let serials = new Map();
  /** @type {Bond[]} */
  let conect = [];
  /** @type {Span[]} */
  const helices = [];
  /** @type {Span[]} */
  const sheets = [];
  let name = "", header = "";
  const flush = () => {
    if (!atoms.length) return;
    const bonds = findBonds(atoms);
    // CONECT says what distance cannot: a metal's bonds, or one too long.
    const have = new Set(bonds.map(([i, j]) => `${Math.min(i, j)},${Math.max(i, j)}`));
    for (const [i, j] of conect) {
      const key = `${Math.min(i, j)},${Math.max(i, j)}`;
      if (!have.has(key)) { have.add(key); bonds.push([i, j, 1]); }
    }
    models.push({ name: name || `Model ${models.length + 1}`, atoms, bonds, helices, sheets });
    atoms = [];
    serials = new Map();
    conect = [];
  };
  for (const line of text.split(/\r?\n/)) {
    const record = line.slice(0, 6).trim();
    if (record === "HEADER") header = line.slice(10, 50).trim();
    else if (record === "TITLE" && !title) title = line.slice(10).trim();
    else if (record === "MODEL") { flush(); name = `Model ${line.slice(10, 14).trim() || models.length + 1}`; }
    else if (record === "ENDMDL") flush();
    else if (record === "ATOM" || record === "HETATM") {
      const alt = line[16] ?? " ";
      if (alt !== " " && alt !== "A" && alt !== "1") continue;
      const [x, y, z] = [Number(line.slice(30, 38)), Number(line.slice(38, 46)), Number(line.slice(46, 54))];
      if (![x, y, z].every(Number.isFinite)) continue;
      const atomName = line.slice(12, 16).trim();
      const given = line.slice(76, 78).trim();
      // Without its element column, a name's first letter — after any
      // digit — is usually the element; " CA " is carbon, "CA  " calcium.
      const el = given ? symbol(given) : line[12] === " " ? symbol(atomName.replace(/^\d+/, "")[0] ?? "X") : symbol(atomName.replace(/^\d+/, ""));
      serials.set(Number(line.slice(6, 11)), atoms.length);
      atoms.push({
        el, x, y, z, name: atomName, res: line.slice(17, 20).trim(), chain: line[21]?.trim() || "", seq: Number(line.slice(22, 26)),
        ins: line[26]?.trim() || "", het: record === "HETATM",
      });
    } else if (record === "CONECT") {
      const from = serials.get(Number(line.slice(6, 11)));
      for (let c = 11; c + 5 <= Math.max(line.length, 16) && c < 31; c += 5) {
        const to = serials.get(Number(line.slice(c, c + 5)));
        if (from != null && to != null && from !== to && (atoms[from].het || atoms[to].het)) conect.push([from, to, 1]);
      }
    } else if (record === "HELIX") {
      helices.push({ chain: line[19]?.trim() || "", from: Number(line.slice(21, 25)), to: Number(line.slice(33, 37)) });
    } else if (record === "SHEET") {
      sheets.push({ chain: line[21]?.trim() || "", from: Number(line.slice(22, 26)), to: Number(line.slice(33, 37)) });
    }
  }
  flush();
  if (!models.length) throw new Error("there are no ATOM or HETATM records in it");
  return { title: title || header, kind: "PDB", models };
}

/* ── mmCIF ────────────────────────────────────────────────────────── */

/** A CIF file's words: quoted strings and ;-fenced text fields are one word
 *  each, and # starts a comment. @param {string} text */
export function cifTokens(text) {
  /** @type {string[]} */
  const out = [];
  const lines = text.split(/\r?\n/);
  for (let l = 0; l < lines.length; l++) {
    const line = lines[l];
    if (line.startsWith(";")) {
      const field = [line.slice(1)];
      while (++l < lines.length && !lines[l].startsWith(";")) field.push(lines[l]);
      out.push(field.join("\n").trim());
      continue;
    }
    let i = 0;
    while (i < line.length) {
      const c = line[i];
      if (c === " " || c === "\t") { i++; continue; }
      if (c === "#") break;
      if ((c === "'" || c === '"') && (i === 0 || /\s/.test(line[i - 1]))) {
        // A quote ends only where whitespace follows it.
        let j = i + 1;
        while (j < line.length && !(line[j] === c && (j + 1 === line.length || /\s/.test(line[j + 1])))) j++;
        out.push(line.slice(i + 1, j));
        i = j + 1;
        continue;
      }
      let j = i;
      while (j < line.length && !/\s/.test(line[j])) j++;
      out.push(line.slice(i, j));
      i = j;
    }
  }
  return out;
}

/** A CIF file's categories: each a list of rows of its items, whether a loop
 *  or items one by one. @param {string} text @returns {Map<string, Record<string, string>[]>} */
export function cifTables(text) {
  const words = cifTokens(text);
  /** @type {Map<string, Record<string, string>[]>} */
  const tables = new Map();
  const split = (/** @type {string} */ tag) => {
    const dot = tag.indexOf(".");
    return dot < 0 ? [tag.slice(1), ""] : [tag.slice(1, dot), tag.slice(dot + 1)];
  };
  let i = 0;
  while (i < words.length) {
    const w = words[i];
    if (w === "loop_") {
      /** @type {string[]} */
      const tags = [];
      i++;
      while (i < words.length && words[i].startsWith("_")) tags.push(words[i++]);
      if (!tags.length) continue;
      const [category] = split(tags[0]);
      const items = tags.map((t) => split(t)[1]);
      const rows = tables.get(category) ?? [];
      while (i < words.length && !words[i].startsWith("_") && words[i] !== "loop_" && !/^data_/.test(words[i])) {
        /** @type {Record<string, string>} */
        const row = {};
        for (const item of items) row[item] = words[i++] ?? "?";
        rows.push(row);
      }
      tables.set(category, rows);
    } else if (w.startsWith("_")) {
      const [category, item] = split(w);
      const rows = tables.get(category) ?? [{}];
      rows[0][item] = words[i + 1] ?? "?";
      tables.set(category, rows);
      i += 2;
    } else {
      i++;
    }
  }
  return tables;
}

/** An mmCIF file: `_atom_site` model by model, the first of each atom's
 *  alternate places, and `_struct_conf` and `_struct_sheet_range` for the
 *  helices and strands. @param {string} text @param {string} title */
export function readCif(text, title) {
  const tables = cifTables(text);
  const sites = tables.get("atom_site");
  if (!sites?.length) throw new Error("it has no _atom_site table");
  const given = (/** @type {string | undefined} */ v) => (v == null || v === "?" || v === "." ? "" : v);
  const seqOf = (/** @type {Record<string, string>} */ r) => Number(given(r.auth_seq_id) || given(r.label_seq_id) || NaN);
  /** @type {Map<string, Atom[]>} */
  const byModel = new Map();
  for (const r of sites) {
    const alt = given(r.label_alt_id);
    if (alt && alt !== "A" && alt !== "1") continue;
    const [x, y, z] = [Number(r.Cartn_x), Number(r.Cartn_y), Number(r.Cartn_z)];
    if (![x, y, z].every(Number.isFinite)) continue;
    const model = given(r.pdbx_PDB_model_num) || "1";
    const list = byModel.get(model) ?? [];
    list.push({
      el: symbol(given(r.type_symbol) || given(r.label_atom_id)), x, y, z,
      name: given(r.auth_atom_id) || given(r.label_atom_id), res: given(r.auth_comp_id) || given(r.label_comp_id),
      chain: given(r.auth_asym_id) || given(r.label_asym_id), seq: seqOf(r), ins: given(r.pdbx_PDB_ins_code),
      het: r.group_PDB === "HETATM",
    });
    byModel.set(model, list);
  }
  const span = (/** @type {Record<string, string>} */ r, /** @type {string} */ from, /** @type {string} */ to) => ({
    chain: given(r[`beg_auth_asym_id`]) || given(r[`beg_label_asym_id`]),
    from: Number(given(r[`beg_auth_seq_id`]) || given(r[from])), to: Number(given(r[`end_auth_seq_id`]) || given(r[to])),
  });
  const helices = (tables.get("struct_conf") ?? []).filter((r) => /HELX/i.test(r.conf_type_id ?? "")).map((r) => span(r, "beg_label_seq_id", "end_label_seq_id"));
  const sheets = (tables.get("struct_sheet_range") ?? []).map((r) => span(r, "beg_label_seq_id", "end_label_seq_id"));
  const entry = tables.get("struct")?.[0];
  const models = [...byModel].map(([n, atoms]) => ({ name: `Model ${n}`, atoms, bonds: findBonds(atoms), helices, sheets }));
  if (!models.length) throw new Error("its _atom_site table has no atoms with places");
  return { title: given(entry?.title) || title, kind: "mmCIF", models };
}

/* ── MOL and SDF ──────────────────────────────────────────────────── */

/** A MOL file, or an SDF of several: V2000 atom and bond blocks, a molecule
 *  a frame. @param {string} text @param {string} title */
export function readMol(text, title) {
  /** @type {Model[]} */
  const models = [];
  for (const block of text.split(/^\$\$\$\$\s*$/m)) {
    const lines = block.replace(/^\s*\n/, "").split(/\r?\n/);
    if (lines.length < 4 || !lines.slice(0, 4).some((l) => l.trim())) continue;
    const counts = lines[3];
    if (/V3000/.test(counts)) throw new Error("it is a V3000 MOL file; only V2000 is read");
    const na = Number(counts.slice(0, 3)), nb = Number(counts.slice(3, 6));
    if (!Number.isInteger(na) || !Number.isInteger(nb)) throw new Error(`its counts line is not one: “${counts.trim()}”`);
    /** @type {Atom[]} */
    const atoms = [];
    for (let k = 0; k < na; k++) {
      const l = lines[4 + k] ?? "";
      const [x, y, z] = [Number(l.slice(0, 10)), Number(l.slice(10, 20)), Number(l.slice(20, 30))];
      if (![x, y, z].every(Number.isFinite)) throw new Error(`atom ${k + 1}'s line has no coordinates`);
      atoms.push({ el: symbol(l.slice(31, 34)), x, y, z });
    }
    /** @type {Bond[]} */
    const bonds = [];
    for (let k = 0; k < nb; k++) {
      const l = lines[4 + na + k] ?? "";
      const a = Number(l.slice(0, 3)) - 1, b = Number(l.slice(3, 6)) - 1, t = Number(l.slice(6, 9));
      if (atoms[a] && atoms[b]) bonds.push([a, b, t === 4 ? 1.5 : t >= 1 && t <= 3 ? t : 1]);
    }
    models.push({ name: lines[0].trim(), atoms, bonds });
  }
  if (!models.length) throw new Error("there is no molecule in it");
  return { title: title || models[0].name, kind: models.length > 1 ? "SDF" : "MOL", models };
}

/* ── SMILES ───────────────────────────────────────────────────────── */

/** @typedef {{el: string, aromatic: boolean, charge: number, h: number | null, isotope?: number}} SmilesAtom */

const ORGANIC = ["Cl", "Br", "B", "C", "N", "O", "P", "S", "F", "I"];
const AROMATIC = ["b", "c", "n", "o", "p", "s"];
const VALENCE = /** @type {Record<string, number[]>} */ ({ B: [3], C: [4], N: [3, 5], O: [2], P: [3, 5], S: [2, 4, 6], F: [1], Cl: [1], Br: [1], I: [1] });

/** A SMILES string's atoms and bonds, each mistake named with where it is.
 *  @param {string} s @returns {{atoms: SmilesAtom[], bonds: Bond[]}} */
export function parseSmiles(s) {
  /** @type {SmilesAtom[]} */
  const atoms = [];
  /** @type {Bond[]} */
  const bonds = [];
  /** @type {number[]} */
  const stack = [];
  /** @type {Map<number, {atom: number, order: number | null}>} */
  const rings = new Map();
  let prev = -1;
  /** @type {number | null} */
  let order = null;
  const fail = (/** @type {number} */ i, /** @type {string} */ what) => { throw new Error(`at ${i + 1}: ${what}`); };
  const join = (/** @type {number} */ a, /** @type {number} */ b, /** @type {number | null} */ o) => {
    if (a === b) fail(0, "an atom bonded to itself");
    if (bonds.some(([x, y]) => (x === a && y === b) || (x === b && y === a))) return;
    bonds.push([a, b, o ?? (atoms[a].aromatic && atoms[b].aromatic ? 1.5 : 1)]);
  };
  const add = (/** @type {SmilesAtom} */ atom) => {
    atoms.push(atom);
    const i = atoms.length - 1;
    if (prev >= 0) join(prev, i, order);
    prev = i;
    order = null;
  };
  let i = 0;
  while (i < s.length) {
    const c = s[i];
    if (/\s/.test(c)) break;
    if (c === "(") {
      if (prev < 0) fail(i, "a branch before any atom");
      stack.push(prev); i++; continue;
    }
    if (c === ")") {
      if (!stack.length) fail(i, "a ) with no ( before it");
      prev = /** @type {number} */ (stack.pop()); i++; continue;
    }
    if ("-=#$:/\\".includes(c)) {
      order = c === "=" ? 2 : c === "#" ? 3 : c === "$" ? 4 : c === ":" ? 1.5 : 1;
      i++; continue;
    }
    if (c === ".") { prev = -1; order = null; i++; continue; }
    if (/\d/.test(c) || c === "%") {
      if (prev < 0) fail(i, "a ring bond before any atom");
      let n;
      if (c === "%") {
        n = Number(s.slice(i + 1, i + 3));
        if (!/^\d\d$/.test(s.slice(i + 1, i + 3))) fail(i, "% is followed by two digits");
        i += 3;
      } else {
        n = Number(c); i++;
      }
      const open = rings.get(n);
      if (open) {
        if (open.order != null && order != null && open.order !== order) fail(i - 1, `ring bond ${n} is given two different orders`);
        join(open.atom, prev, order ?? open.order);
        rings.delete(n);
      } else {
        rings.set(n, { atom: prev, order });
      }
      order = null;
      continue;
    }
    if (c === "[") {
      const end = s.indexOf("]", i);
      if (end < 0) fail(i, "a [ with no ] after it");
      const body = s.slice(i + 1, end);
      const m = body.match(/^(\d+)?(se|as|te|[a-z]|[A-Z][a-z]?|\*)(@@?|@[A-Z]{2}\d*)?(H\d*)?([+-]+\d*)?(:\d+)?$/);
      if (!m) fail(i, `“[${body}]” is not an atom`);
      const [, iso, sym, , hs, charge] = /** @type {RegExpMatchArray} */ (m);
      const aromatic = sym !== "*" && sym === sym.toLowerCase();
      const el = sym === "*" ? "X" : symbol(sym);
      if (sym !== "*" && !ELEMENTS[el]) fail(i, `${sym} is not an element`);
      let q = 0;
      if (charge) {
        const digits = charge.match(/\d+/);
        q = (charge[0] === "+" ? 1 : -1) * (digits ? Number(digits[0]) : charge.length);
      }
      add({ el, aromatic, charge: q, h: hs ? (hs.length > 1 ? Number(hs.slice(1)) : 1) : 0, isotope: iso ? Number(iso) : undefined });
      i = end + 1;
      continue;
    }
    const two = s.slice(i, i + 2);
    if (ORGANIC.includes(two)) { add({ el: two, aromatic: false, charge: 0, h: null }); i += 2; continue; }
    if (ORGANIC.includes(c)) { add({ el: c, aromatic: false, charge: 0, h: null }); i++; continue; }
    if (AROMATIC.includes(c)) { add({ el: c.toUpperCase(), aromatic: true, charge: 0, h: null }); i++; continue; }
    if (c === "*") { add({ el: "X", aromatic: false, charge: 0, h: 0 }); i++; continue; }
    fail(i, `“${c}” is not part of SMILES`);
  }
  if (stack.length) fail(s.length - 1, "a ( is never closed");
  if (rings.size) fail(s.length - 1, `ring bond ${[...rings.keys()][0]} is never closed`);
  if (!atoms.length) fail(0, "there are no atoms");
  return { atoms, bonds };
}

/** How many hydrogens an organic-subset atom carries unwritten: what is left
 *  of its smallest valence that fits. An aromatic atom keeps one bond for the
 *  ring's double bonds, of its smallest valence only: an aromatic nitrogen
 *  with three bonds gives the ring its lone pair, and carries no hydrogen.
 *  @param {SmilesAtom} atom @param {number} used */
function implicitH(atom, used) {
  if (atom.h != null) return atom.h;
  const valences = VALENCE[atom.el] ?? [];
  if (atom.aromatic) return Math.max(0, (valences[0] ?? 0) - used - 1);
  const v = valences.find((v) => v >= used);
  return v == null ? 0 : v - used;
}

/** @param {number[]} a @param {number[]} b */
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
/** @param {number[]} a @param {number[]} b */
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
/** @param {number[]} a @param {number[]} b */
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];

/** A small, repeatable random number source. @param {number} seed */
function random(seed) {
  let t = seed >>> 0;
  return () => {
    t = (t + 0x6d2b79f5) >>> 0;
    let r = Math.imul(t ^ (t >>> 15), 1 | t);
    r ^= r + Math.imul(r ^ (r >>> 7), 61 | r);
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}

/** Bond length, Å, by element and order. @param {string} a @param {string} b @param {number} order */
function length(a, b, order) {
  const base = element(a).covalent + element(b).covalent;
  return base - (order === 1.5 ? 0.12 : order === 2 ? 0.18 : order >= 3 ? 0.32 : 0);
}

/** Places for a graph's atoms, hydrogens written out: laid out in four
 *  dimensions so rings can pass through each other while they settle, then
 *  pressed flat into three. Repeatable: the same graph, the same shape.
 *  @param {{atoms: SmilesAtom[], bonds: Bond[]}} graph @returns {{atoms: Atom[], bonds: Bond[]}} */
export function embed(graph) {
  const atoms = graph.atoms.map((a) => ({ el: a.el }));
  /** @type {Bond[]} */
  const bonds = graph.bonds.map((b) => [b[0], b[1], b[2]]);
  const used = graph.atoms.map(() => 0);
  for (const [a, b, o] of graph.bonds) {
    const n = o === 1.5 ? 1 : o;
    used[a] += n; used[b] += n;
  }
  graph.atoms.forEach((atom, i) => {
    for (let k = implicitH(atom, used[i]); k > 0; k--) {
      atoms.push({ el: "H" });
      bonds.push([i, atoms.length - 1, 1]);
    }
  });
  const n = atoms.length;
  /** @type {number[][]} */
  const near = atoms.map(() => []);
  for (const [a, b] of bonds) { near[a].push(b); near[b].push(a); }
  // Each atom's angle from its bonds: a triple bond or two doubles straight,
  // a double or a ring's aromatic bond flat, otherwise tetrahedral.
  const angle = atoms.map((_, i) => {
    const mine = bonds.filter(([a, b]) => a === i || b === i).map(([, , o]) => o);
    if (mine.some((o) => o >= 3) || mine.filter((o) => o === 2).length >= 2) return Math.PI;
    if (mine.some((o) => o === 2 || o === 1.5)) return (2 * Math.PI) / 3;
    return Math.acos(-1 / 3);
  });
  /** What each pair should be: bonded, or across one atom. @type {Map<number, number>} */
  const target = new Map();
  const key = (/** @type {number} */ a, /** @type {number} */ b) => (a < b ? a * n + b : b * n + a);
  /** @type {Map<number, number>} */
  const bondLength = new Map();
  for (const [a, b, o] of bonds) {
    const d = length(atoms[a].el, atoms[b].el, o);
    bondLength.set(key(a, b), d);
    target.set(key(a, b), d);
  }
  for (let j = 0; j < n; j++) {
    for (let x = 0; x < near[j].length; x++) for (let y = x + 1; y < near[j].length; y++) {
      const a = near[j][x], b = near[j][y];
      if (target.has(key(a, b)) && bondLength.has(key(a, b))) continue;
      const da = /** @type {number} */ (bondLength.get(key(a, j))), db = /** @type {number} */ (bondLength.get(key(b, j)));
      target.set(key(a, b), Math.sqrt(da * da + db * db - 2 * da * db * Math.cos(angle[j])));
    }
  }
  const pairs = [...target].map(([k, d]) => [Math.floor(k / n), k % n, d]);
  // Atoms three bonds apart may come nearer than those further: a benzene
  // ring's opposite corners are 2.8 Å apart.
  const close = new Set(target.keys());
  /** @type {Set<number>} */
  const third = new Set();
  for (let a = 0; a < n; a++) {
    for (const b of near[a]) for (const c of near[b]) for (const d of near[c]) {
      if (d !== a && d !== b && !close.has(key(a, d))) third.add(key(a, d));
    }
  }
  const D = 4;
  const rand = random(n * 7919 + bonds.length);
  const spread = 1.6 * Math.cbrt(n) + 1;
  const p = new Float64Array(n * D);
  for (let i = 0; i < n * D; i++) p[i] = (rand() - 0.5) * spread;
  // Fours that lie in one plane: a flat atom with its three neighbours, and
  // the atoms either side of a double or aromatic bond. Bond lengths and
  // angles alone let a ring flatten only slowly, the energy of a pucker
  // going as its fourth power.
  /** @type {number[][]} */
  const flats = [];
  for (let i = 0; i < n; i++) if (angle[i] < 2.2 && angle[i] > 2 && near[i].length === 3) flats.push([i, ...near[i]]);
  for (const [b, c, o] of bonds) {
    if (o !== 2 && o !== 1.5) continue;
    for (const a of near[b]) for (const d of near[c]) if (a !== c && d !== b && a !== d) flats.push([a, b, c, d]);
  }
  // How far apart each pair must keep, if no nearer bond decides it.
  const apart = new Float32Array(n * n);
  for (let a = 0; a < n; a++) for (let b = a + 1; b < n; b++) {
    if (close.has(key(a, b))) continue;
    const h = atoms[a].el === "H" || atoms[b].el === "H";
    apart[a * n + b] = third.has(key(a, b)) ? (h ? 2.2 : 2.5) : h ? 2.4 : 3.0;
  }
  const g = new Float64Array(n * D);
  /** Settle for `steps`, the fourth dimension pressed out as it goes, or
   *  already gone. @param {number} steps @param {boolean} flat */
  const settle = (steps, flat) => {
    for (let step = 0; step < steps; step++) {
      g.fill(0);
      const pull = (/** @type {number} */ a, /** @type {number} */ b, /** @type {number} */ want, /** @type {number} */ k, /** @type {boolean} */ onlyApart) => {
        let d2 = 0;
        for (let c = 0; c < D; c++) { const v = p[a * D + c] - p[b * D + c]; d2 += v * v; }
        const d = Math.sqrt(d2) || 1e-6;
        if (onlyApart && d >= want) return;
        const f = (k * (d - want)) / d;
        for (let c = 0; c < D; c++) {
          const v = (p[a * D + c] - p[b * D + c]) * f;
          g[a * D + c] += v; g[b * D + c] -= v;
        }
      };
      for (const [a, b, d] of pairs) pull(a, b, d, 1, false);
      if (step % 2 === 0 || step > steps - 100) {
        for (let a = 0; a < n; a++) {
          const pa = a * D;
          for (let b = a + 1; b < n; b++) {
            const want = apart[a * n + b];
            if (!want) continue;
            const pb = b * D;
            const d0 = p[pa] - p[pb], d1 = p[pa + 1] - p[pb + 1], d2 = p[pa + 2] - p[pb + 2], d3 = p[pa + 3] - p[pb + 3];
            const d2s = d0 * d0 + d1 * d1 + d2 * d2 + d3 * d3;
            if (d2s >= want * want) continue;
            const d = Math.sqrt(d2s) || 1e-6;
            const f = (0.3 * (d - want)) / d;
            g[pa] += d0 * f; g[pb] -= d0 * f; g[pa + 1] += d1 * f; g[pb + 1] -= d1 * f;
            g[pa + 2] += d2 * f; g[pb + 2] -= d2 * f; g[pa + 3] += d3 * f; g[pb + 3] -= d3 * f;
          }
        }
      }
      if (flat) {
        for (const [i0, i1, i2, i3] of flats) {
          const P = (/** @type {number} */ i) => [p[i * D], p[i * D + 1], p[i * D + 2]];
          const o = P(i0), u = sub(P(i1), o), v = sub(P(i2), o), w = sub(P(i3), o);
          const vw = cross(v, w), wu = cross(w, u), uv = cross(u, v);
          const f = 0.4 * dot(u, vw);
          for (let c = 0; c < 3; c++) {
            g[i1 * D + c] += f * vw[c]; g[i2 * D + c] += f * wu[c]; g[i3 * D + c] += f * uv[c];
            g[i0 * D + c] -= f * (vw[c] + wu[c] + uv[c]);
          }
        }
      }
      const out = flat ? 1 : Math.min(1, step / (steps * 0.8));
      for (let a = 0; a < n; a++) g[a * D + 3] += p[a * D + 3] * out * 2;
      const rate = 0.12 * (1 - step / (steps * 1.25));
      for (let i = 0; i < n * D; i++) p[i] -= Math.max(-0.3, Math.min(0.3, g[i] * rate));
      if (flat) for (let a = 0; a < n; a++) p[a * D + 3] = 0;
    }
  };
  // Free in four dimensions, then pressed into three and settled there.
  settle(1000, false);
  for (let a = 0; a < n; a++) p[a * D + 3] = 0;
  settle(800, true);
  // Centred, so it turns about itself.
  const mid = [0, 1, 2].map((c) => atoms.reduce((s, _, i) => s + p[i * D + c], 0) / n);
  return {
    atoms: atoms.map((a, i) => ({ el: a.el, x: p[i * D] - mid[0], y: p[i * D + 1] - mid[1], z: p[i * D + 2] - mid[2] })),
    bonds,
  };
}

/** A .smi file: one SMILES a line, a name after it; each a frame.
 *  @param {string} text @param {string} title */
export function readSmiles(text, title) {
  /** @type {Model[]} */
  const models = [];
  text.split(/\r?\n/).forEach((line, l) => {
    const t = line.trim();
    if (!t || t.startsWith("#")) return;
    const [smiles, ...name] = t.split(/\s+/);
    let graph;
    try {
      graph = parseSmiles(smiles);
    } catch (e) {
      throw new Error(`line ${l + 1}, ${/** @type {Error} */ (e).message}`);
    }
    // Laid out when first shown: a file of a hundred takes a moment each.
    models.push({ name: name.join(" ") || smiles, atoms: [], bonds: [], pending: () => embed(/** @type {any} */ (graph)) });
  });
  if (!models.length) throw new Error("there is no SMILES in it");
  return { title: title || models[0].name, kind: "SMILES", models, note: "Laid out from its graph: bond lengths and angles, no stereo." };
}

/** A model with its atoms placed and its bonds found, done once.
 *  @param {Model} model */
export function ready(model) {
  if (model.pending) {
    const { atoms, bonds } = model.pending();
    model.atoms = atoms;
    model.bonds = bonds;
    delete model.pending;
  }
  model.bonds ??= findBonds(model.atoms);
  return model;
}

/* ── one door ─────────────────────────────────────────────────────── */

/** The kinds of file read here, by their ending. */
export const ENDINGS = ["xyz", "pdb", "ent", "cif", "mmcif", "mol", "sdf", "smi"];

/** Whether a file's name is one read here. @param {string} path */
export function isMolecule(path) {
  return ENDINGS.includes(path.toLowerCase().split(".").pop() ?? "");
}

/** A file read by its ending. Throws, with what is wrong, for one that is
 *  not what its ending says. @param {string} path @param {string} text @returns {Molecule} */
export function readMolecule(path, text) {
  const ending = path.toLowerCase().split(".").pop() ?? "";
  const name = path.split("/").pop() ?? path;
  const read = { xyz: readXyz, pdb: readPdb, ent: readPdb, cif: readCif, mmcif: readCif, mol: readMol, sdf: readMol, smi: readSmiles }[ending];
  if (!read) throw new Error(`.${ending} is not a molecule file the Lab reads`);
  const molecule = read(text, "");
  // A comment line or a TITLE is a better name than the file's, when given.
  molecule.title ||= name;
  return molecule;
}

/* ── a protein's backbone ─────────────────────────────────────────── */

/** @typedef {{ca: number, o: number | null, ss: "H" | "E" | "C", res: string, seq: number}} Residue */
/** @typedef {{chain: string, residues: Residue[]}} Strand  a run of residues joined end to end */

/** A model's protein backbone: its residues' α-carbons chain by chain, cut
 *  where a gap is too long for a peptide bond, each marked helix, strand or
 *  coil — by the file's own records, or failing those by the α-carbons'
 *  spacing. @param {Model} model @returns {{pieces: Strand[], estimated: boolean}} */
export function backbone(model) {
  /** @type {Map<string, Residue[]>} */
  const chains = new Map();
  /** @type {Map<string, Residue>} */
  const seen = new Map();
  model.atoms.forEach((a, i) => {
    if (a.het || a.seq == null || !Number.isFinite(a.seq)) return;
    const id = `${a.chain}/${a.seq}/${a.ins ?? ""}`;
    if (a.name === "CA" && a.el === "C" && !seen.has(id)) {
      const r = /** @type {Residue} */ ({ ca: i, o: null, ss: "C", res: a.res ?? "", seq: a.seq });
      seen.set(id, r);
      const list = chains.get(a.chain ?? "") ?? [];
      list.push(r);
      chains.set(a.chain ?? "", list);
    } else if (a.name === "O") {
      const r = seen.get(id);
      if (r) r.o = i;
    }
  });
  // An O seen before its CA.
  model.atoms.forEach((a, i) => {
    if (a.name !== "O" || a.het) return;
    const r = seen.get(`${a.chain}/${a.seq}/${a.ins ?? ""}`);
    if (r && r.o == null) r.o = i;
  });
  const given = (model.helices?.length ?? 0) + (model.sheets?.length ?? 0) > 0;
  const within = (/** @type {Span[] | undefined} */ spans, /** @type {string} */ chain, /** @type {number} */ seq) =>
    (spans ?? []).some((s) => (s.chain === chain || !s.chain) && seq >= s.from && seq <= s.to);
  const at = (/** @type {number} */ i) => model.atoms[i];
  const dist = (/** @type {number} */ i, /** @type {number} */ j) => Math.hypot(at(i).x - at(j).x, at(i).y - at(j).y, at(i).z - at(j).z);
  /** @type {Strand[]} */
  const pieces = [];
  for (const [chain, residues] of chains) {
    if (given) {
      for (const r of residues) r.ss = within(model.helices, chain, r.seq) ? "H" : within(model.sheets, chain, r.seq) ? "E" : "C";
    } else {
      estimate(residues, dist);
    }
    let piece = /** @type {Residue[]} */ ([]);
    residues.forEach((r, k) => {
      if (k > 0 && dist(residues[k - 1].ca, r.ca) > 4.3) {
        if (piece.length > 1) pieces.push({ chain, residues: piece });
        piece = [];
      }
      piece.push(r);
    });
    if (piece.length > 1) pieces.push({ chain, residues: piece });
  }
  return { pieces, estimated: !given && pieces.length > 0 };
}

/** Helices and strands from α-carbon spacing alone, for a file that does not
 *  say: four in a row an α-helix's turn apart, or three stretched out
 *  straight. Rough — no hydrogen bonds — and said to be.
 *  @param {Residue[]} rs @param {(i: number, j: number) => number} dist */
function estimate(rs, dist) {
  const helix = rs.map(() => false), strand = rs.map(() => false);
  for (let k = 0; k + 4 < rs.length; k++) {
    const d3 = dist(rs[k].ca, rs[k + 3].ca), d4 = dist(rs[k].ca, rs[k + 4].ca);
    if (d3 > 4.6 && d3 < 5.8 && d4 > 5.6 && d4 < 6.8) for (let m = k; m <= k + 4; m++) helix[m] = true;
  }
  for (let k = 1; k + 1 < rs.length; k++) {
    if (!helix[k] && dist(rs[k - 1].ca, rs[k + 1].ca) > 6.3) strand[k - 1] = strand[k] = strand[k + 1] = true;
  }
  // A strand of fewer than three is a coil's stretch, not a strand.
  let run = 0;
  for (let k = 0; k <= rs.length; k++) {
    if (k < rs.length && strand[k] && !helix[k]) { run++; continue; }
    if (run > 0 && run < 3) for (let m = k - run; m < k; m++) strand[m] = false;
    run = 0;
  }
  rs.forEach((r, k) => { r.ss = helix[k] ? "H" : strand[k] ? "E" : "C"; });
}

/** Whether a model is worth drawing as a cartoon: a protein of some length.
 *  @param {Model} model */
export function isProtein(model) {
  return backbone(model).pieces.reduce((s, p) => s + p.residues.length, 0) >= 8;
}

/** Waters, which a protein's picture leaves out. @param {Atom} a */
export const isWater = (a) => a.res === "HOH" || a.res === "WAT" || a.res === "DOD";

/** What a picked atom is, said in a line. @param {Atom} a @param {number} i */
export function describe(a, i) {
  const what = a.name ? `${a.name} (${element(a.el).name})` : `${a.el}${i + 1} (${element(a.el).name})`;
  const where = a.res ? ` · ${a.res} ${a.seq ?? ""}${a.ins ?? ""}${a.chain ? `, chain ${a.chain}` : ""}` : "";
  return `${what}${where} · ${[a.x, a.y, a.z].map((v) => v.toFixed(2)).join(", ")} Å`;
}
