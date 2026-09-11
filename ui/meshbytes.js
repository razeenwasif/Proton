/* A mesh as bytes — the page's half of the layout `proton-mesh` writes.
 *
 * The table of what sits where is in `crates/proton-mesh/src/bytes.rs`, with
 * the tests that pin it; this reads exactly that and nothing else. Every
 * section starts on a four-byte boundary, so the arrays below are *views* onto
 * the buffer the shell handed over — no parse and no copy. That is the whole
 * reason there is a second channel: as JSON, the same mesh took longer to
 * parse and copy here than it did to cross.
 *
 * Little-endian, like every machine this runs on. A typed array reads in the
 * machine's own order, so on a big-endian one these views would be garbage
 * shaped like a mesh — which is why that is checked, once, and refused in
 * words rather than drawn.
 *
 * No index is checked against the vertex count here. WebGL does that itself,
 * per draw, and must: an out-of-range index is an error there, never a read
 * past the end of a buffer.
 */

export const MAGIC = "PMSH";
export const VERSION = 1;
export const HEADER = 48;

const CLOSED = 1;
const NORMALS = 2;

const LITTLE = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;

/** Read a mesh out of an `ArrayBuffer`, or say why it is not one. */
export function decodeMesh(buffer) {
  if (!LITTLE) throw new Error("this machine is big-endian and meshes are sent little-endian");
  if (!(buffer instanceof ArrayBuffer)) throw new Error("a mesh arrives as an ArrayBuffer");
  if (buffer.byteLength < HEADER) throw new Error(`${buffer.byteLength} bytes is too short for a mesh`);

  const head = new DataView(buffer, 0, HEADER);
  const magic = String.fromCharCode(...new Uint8Array(buffer, 0, 4));
  if (magic !== MAGIC) throw new Error("these bytes are not a mesh");
  const version = head.getUint32(4, true);
  if (version !== VERSION) {
    // A page and a core from different builds. Reading on would draw a mesh
    // laid out one way through the offsets of another.
    throw new Error(`this is a version ${version} mesh and this page reads version ${VERSION}`);
  }

  const vertices = head.getUint32(8, true);
  const triangles = head.getUint32(12, true);
  const flags = head.getUint32(16, true);
  const hasNormals = (flags & NORMALS) !== 0;
  const want = HEADER + (vertices * 3 * (hasNormals ? 2 : 1) + triangles * 3) * 4;
  if (buffer.byteLength !== want) {
    throw new Error(
      `a mesh of ${vertices} vertices and ${triangles} triangles is ${want} bytes, and this is ${buffer.byteLength}`,
    );
  }

  let at = HEADER;
  const positions = new Float32Array(buffer, at, vertices * 3);
  at += vertices * 12;
  const normals = hasNormals ? new Float32Array(buffer, at, vertices * 3) : null;
  if (hasNormals) at += vertices * 12;
  const indices = new Uint32Array(buffer, at, triangles * 3);

  const bounds = [];
  for (let k = 0; k < 6; k++) bounds.push(head.getFloat32(24 + k * 4, true));

  return {
    positions,
    normals,
    triangles: indices,
    bounds,
    closed: (flags & CLOSED) !== 0,
    skipped: head.getUint32(20, true),
  };
}

/** Write a mesh in the same layout.
 *
 * Only the design preview needs this: it has no core, so its stand-in meshes
 * are encoded here and then read back through `decodeMesh` — the same path a
 * real mesh takes, rather than a shortcut around it that the preview would
 * then be the only thing exercising. */
export function encodeMesh(mesh) {
  const positions = Float32Array.from(mesh.positions);
  const normals = mesh.normals?.length ? Float32Array.from(mesh.normals) : null;
  const indices = Uint32Array.from(mesh.triangles);
  const vertices = positions.length / 3;
  const triangles = indices.length / 3;
  const buffer = new ArrayBuffer(
    HEADER + (positions.length + (normals ? normals.length : 0) + indices.length) * 4,
  );
  const head = new DataView(buffer, 0, HEADER);
  for (let i = 0; i < 4; i++) head.setUint8(i, MAGIC.charCodeAt(i));
  head.setUint32(4, VERSION, true);
  head.setUint32(8, vertices, true);
  head.setUint32(12, triangles, true);
  head.setUint32(16, (mesh.closed ? CLOSED : 0) | (normals ? NORMALS : 0), true);
  head.setUint32(20, mesh.skipped ?? 0, true);
  (mesh.bounds ?? [0, 0, 0, 0, 0, 0]).forEach((b, k) => head.setFloat32(24 + k * 4, b, true));

  let at = HEADER;
  new Float32Array(buffer, at, positions.length).set(positions);
  at += positions.length * 4;
  if (normals) {
    new Float32Array(buffer, at, normals.length).set(normals);
    at += normals.length * 4;
  }
  new Uint32Array(buffer, at, indices.length).set(indices);
  return buffer;
}
