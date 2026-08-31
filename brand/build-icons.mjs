// Rasterise proton-icon.svg to every PNG size and repack proton.ico.
//   npm i -D sharp && node brand/build-icons.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const here = dirname(fileURLToPath(import.meta.url));
const svg = readFileSync(join(here, "proton-icon.svg"));

const PNG_SIZES = [16, 32, 48, 64, 128, 256, 512, 1024];
const ICO_SIZES = [16, 32, 48, 64, 128, 256];

const png = new Map();
for (const size of PNG_SIZES) {
  const buf = await sharp(svg, { density: 384 }).resize(size, size).png().toBuffer();
  writeFileSync(join(here, `proton-icon-${size}.png`), buf);
  png.set(size, buf);
}

// ICO is a 6-byte header, a 16-byte directory entry per image, then the PNGs
// verbatim — Vista and later accept PNG-compressed entries, so no BMP encoding.
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);              // reserved
header.writeUInt16LE(1, 2);              // type: icon
header.writeUInt16LE(ICO_SIZES.length, 4);

let offset = 6 + 16 * ICO_SIZES.length;
const entries = [];
for (const size of ICO_SIZES) {
  const buf = png.get(size);
  const e = Buffer.alloc(16);
  e.writeUInt8(size >= 256 ? 0 : size, 0); // 0 means 256
  e.writeUInt8(size >= 256 ? 0 : size, 1);
  e.writeUInt16LE(1, 4);                   // colour planes
  e.writeUInt16LE(32, 6);                  // bits per pixel
  e.writeUInt32LE(buf.length, 8);
  e.writeUInt32LE(offset, 12);
  offset += buf.length;
  entries.push(e);
}

writeFileSync(
  join(here, "proton.ico"),
  Buffer.concat([header, ...entries, ...ICO_SIZES.map((s) => png.get(s))]),
);
console.log(`wrote ${PNG_SIZES.length} PNGs and proton.ico`);
