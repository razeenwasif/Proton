/* Diffuse's graph, as numbers: where a node's sockets sit, the wire between
 * two of them, the colour of each socket type, and what a search of the
 * catalogue finds. Pure — no DOM — so the node tests run it as it is.
 *
 * A node card is laid out to these measures exactly: a head, then one row
 * per socket, inputs on the left and outputs on the right, then its fields.
 * Sockets come first so a port's place never depends on a field's value,
 * and a wire can be drawn from the flow alone. */

/** @typedef {import('./bindings/DiffuseCatalogue').DiffuseCatalogue} Catalogue */
/** @typedef {import('./bindings/DiffuseNodeType').DiffuseNodeType} NodeType */
/** @typedef {import('./bindings/DiffuseInput').DiffuseInput} Input */
/** @typedef {import('./bindings/DiffuseOutput').DiffuseOutput} Output */
/** @typedef {import('./bindings/DiffuseFlow').DiffuseFlow} Flow */
/** @typedef {import('./bindings/DiffuseNode').DiffuseNode} Node */

/** A card's width, its head's height, a socket row's, and the gap above the rows. */
export const NODE_W = 220;
export const HEAD = 30;
export const ROW = 22;
export const GAP = 6;

/** The socket types the v1 catalogue names, coloured as the mockups are. */
export const SOCKET_COLOURS = /** @type {Record<string, string>} */ ({
  MODEL: "#a277ff",
  CLIP: "#ffca85",
  VAE: "#ff6767",
  CONDITIONING: "#f694ff",
  LATENT: "#82e2ff",
  IMAGE: "#61ffca",
  MASK: "#c9c6d6",
  UPSCALER: "#b9a2ff",
});

/** A socket type's colour: its own, or one made from its name for a type a
 *  newer server adds. @param {string} type */
export function colourOf(type) {
  const own = SOCKET_COLOURS[type];
  if (own) return own;
  let h = 0;
  for (const c of type) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 70% 70%)`;
}

/** A node's socket rows: the nth input socket beside the nth output.
 *  @param {NodeType} kind @returns {{input: Input | null, output: Output | null}[]} */
export function rowsOf(kind) {
  const inputs = kind.inputs.filter((i) => i.type);
  const rows = Math.max(inputs.length, kind.outputs.length);
  return Array.from({ length: rows }, (_, i) => ({ input: inputs[i] ?? null, output: kind.outputs[i] ?? null }));
}

/** The inputs set on the node, not linked. @param {NodeType} kind */
export const fieldsOf = (kind) => kind.inputs.filter((i) => !i.type);

/** A port, `node:name`, read. @param {string} port */
export function portOf(port) {
  const at = port.indexOf(":");
  return at > 0 ? { node: port.slice(0, at), name: port.slice(at + 1) } : null;
}

/** Where a port sits on the graph: an input on the card's left edge, an
 *  output on its right, at the middle of its row. `null` for a port the
 *  node's type has not.
 *  @param {Node} node @param {NodeType} kind @param {"in" | "out"} side @param {string} name @returns {[number, number] | null} */
export function portAt(node, kind, side, name) {
  const rows = rowsOf(kind);
  const row = rows.findIndex((r) => (side === "in" ? r.input?.name : r.output?.name) === name);
  if (row < 0) return null;
  const y = node.at[1] + HEAD + GAP + row * ROW + ROW / 2;
  return [side === "in" ? node.at[0] : node.at[0] + NODE_W, y];
}

/** A wire from an output at `a` to an input at `b`: a curve that leaves and
 *  arrives level, as ComfyUI's do. @param {[number, number]} a @param {[number, number]} b */
export function wire(a, b) {
  const dx = Math.max(40, Math.abs(b[0] - a[0]) / 2);
  return `M${a[0]} ${a[1]} C${a[0] + dx} ${a[1]} ${b[0] - dx} ${b[1]} ${b[0]} ${b[1]}`;
}

/** The id the core gives the next node added: one more than the largest
 *  number among them (`proton-diffuse`'s `Flow::fresh_id`), so a batch can
 *  name a node it adds. @param {Flow} flow */
export function freshId(flow) {
  const ids = new Set(flow.nodes.map((n) => n.id));
  let next = Math.max(0, ...flow.nodes.map((n) => Number(n.id)).filter((n) => Number.isInteger(n) && n >= 0)) + 1;
  while (ids.has(String(next))) next += 1;
  return String(next);
}

/** The order the catalogue's groups are listed in; others follow. */
const GROUPS = ["Load", "Prompt", "Latent", "Sample", "Image", "Output"];

/** The catalogue's nodes by group, in the order above.
 *  @param {Catalogue} catalogue @returns {{group: string, nodes: NodeType[]}[]} */
export function grouped(catalogue) {
  /** @type {Map<string, NodeType[]>} */
  const by = new Map();
  for (const node of catalogue.nodes) by.set(node.group, [...(by.get(node.group) ?? []), node]);
  const order = [...by.keys()].sort((a, b) => (GROUPS.indexOf(a) + 1 || 99) - (GROUPS.indexOf(b) + 1 || 99) || a.localeCompare(b));
  return order.map((group) => ({ group, nodes: by.get(group) ?? [] }));
}

/** What a search finds: nodes whose title (or type) holds the words typed,
 *  those starting with them first; with `fits`, only those that can take a
 *  wire of that type into an input (`side` "in") or give one from an output
 *  ("out"). Grouped as the catalogue is.
 *  @param {Catalogue} catalogue @param {string} query @param {{type: string, side: "in" | "out"} | null} [fits]
 *  @returns {{group: string, nodes: NodeType[]}[]} */
export function search(catalogue, query, fits = null) {
  const q = query.trim().toLowerCase();
  const fitting = (/** @type {NodeType} */ n) =>
    !fits || (fits.side === "in" ? n.inputs.some((i) => i.type === fits.type) : n.outputs.some((o) => o.type === fits.type));
  const score = (/** @type {NodeType} */ n) => {
    if (!q) return 1;
    const title = n.title.toLowerCase();
    if (title.startsWith(q)) return 3;
    if (title.split(/\s+/).some((w) => w.startsWith(q))) return 2;
    return title.includes(q) || n.type.toLowerCase().includes(q) ? 1 : 0;
  };
  return grouped({ ...catalogue, nodes: catalogue.nodes.filter((n) => fitting(n) && score(n) > 0) })
    .map(({ group, nodes }) => ({ group, nodes: [...nodes].sort((a, b) => score(b) - score(a)) }))
    .filter((g) => g.nodes.length);
}

/** The first input of a type that takes `type`, or the first output that
 *  gives it. @param {NodeType} kind @param {string} type @param {"in" | "out"} side */
export function socketFor(kind, type, side) {
  return side === "in" ? (kind.inputs.find((i) => i.type === type)?.name ?? null) : (kind.outputs.find((o) => o.type === type)?.name ?? null);
}

/** A field's value as a card shows it: short. @param {unknown} value */
export function shortValue(value) {
  if (value === undefined || value === null) return "—";
  const text = typeof value === "string" ? value : JSON.stringify(value);
  const plain = text.replace(/\.safetensors$|\.ckpt$|\.pth$/, "");
  return plain.length > 26 ? `${plain.slice(0, 25)}…` : plain;
}

/** The text a PNG keeps under `key` — the graph forge writes as
 *  `proton-flow` — read from its head, before the pixels; `null` when it
 *  holds none, or is not a PNG. `tEXt`, and `iTXt` uncompressed.
 *  @param {Uint8Array} bytes @param {string} [key] */
export function pngText(bytes, key = "proton-flow") {
  const signature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  if (bytes.length < 8 || signature.some((b, i) => bytes[i] !== b)) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const latin = (/** @type {Uint8Array} */ part) => String.fromCharCode(...part);
  let at = 8;
  while (at + 8 <= bytes.length) {
    const length = view.getUint32(at);
    const kind = latin(bytes.subarray(at + 4, at + 8));
    const data = bytes.subarray(at + 8, Math.min(bytes.length, at + 8 + length));
    if (kind === "IDAT" || kind === "IEND") break;
    const nul = data.indexOf(0);
    if (nul > 0 && latin(data.subarray(0, nul)) === key) {
      if (kind === "tEXt") {
        let text = "";
        for (let i = nul + 1; i < data.length; i += 0x8000) text += latin(data.subarray(i, Math.min(data.length, i + 0x8000)));
        return text;
      }
      if (kind === "iTXt" && data[nul + 1] === 0) {
        // Keyword, NUL, compressed flag, method, language NUL, translated keyword NUL, text.
        let i = nul + 3;
        i = data.indexOf(0, i) + 1;
        i = data.indexOf(0, i) + 1;
        if (i > 0) return new TextDecoder().decode(data.subarray(i));
      }
    }
    at += 12 + length;
  }
  return null;
}
