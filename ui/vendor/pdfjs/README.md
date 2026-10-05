# pdf.js — vendored

`pdf.min.mjs` and `pdf.worker.min.mjs`, from
[pdf.js](https://github.com/mozilla/pdf.js) **6.3.289**, fetched from
`https://cdnjs.cloudflare.com/ajax/libs/pdf.js/6.3.289/`.

Copyright Mozilla Foundation, licensed under the **Apache License 2.0**. The
full notice is retained at the top of each file, which is what the licence
asks for.

Beside them, from the npm package `pdfjs-dist@6.3.289` (fetched from
`https://cdn.jsdelivr.net/npm/pdfjs-dist@6.3.289/` on 2026-10-06, each file
checked against the registry's published SHA-256), what a PDF may lean on
without carrying it — 198 files, 2.9 MB:

| Folder | What it is for | Licence |
|---|---|---|
| `cmaps/` | the character maps CJK text is encoded with, packed (`.bcmap`) | Adobe's CMap resources, BSD 3-Clause |
| `standard_fonts/` | the standard fonts a PDF may name without embedding | Foxit's (`LICENSE_FOXIT`, BSD 3-Clause); Liberation Sans (`LICENSE_LIBERATION`, GPLv2 with the font exception) — separate files the viewer loads, not part of Proton's code |
| `wasm/` | JPEG 2000 (OpenJPEG, BSD 2-Clause) and JBIG2 (PDFium, BSD 3-Clause) decoders, and qcms colour management (MIT); each decoder with its plain-JS fallback, which pdf.js imports when WebAssembly is refused — as Proton's `script-src 'self'` may | as each `LICENSE_*` says; pdf.js's glue Apache 2.0 |
| `iccs/` | the CMYK profile colours are converted with | CC0 |

`quickjs-eval.*` (458 KB), the sandbox pdf.js runs a PDF's own scripts in, is
left out: Proton does not run them.

`ui/document.js` passes their folders to `getDocument` (`cMapUrl`,
`standardFontDataUrl`, `wasmUrl`, `iccUrl`).

## Why it is in the repository rather than fetched

Proton's content security policy is `script-src 'self'`. Nothing loads from a
CDN at run time, deliberately: an editor that needs the network to open a file
is an editor that stops working on a train.

## Why it is not imported at boot

Together these are 1.7MB, and a session that never opens a PDF should never
pay for them. `ui/document.js` imports the library on first use, which is also
why it is a separate module from `app.js`.

## Updating

Replace both files with the same version of each — the worker and the library
check that they match and refuse to run otherwise — and the folders beside
them with the same version's, and update the versions above.
