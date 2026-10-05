# three.js — vendored

`three.module.js`, `three.core.js` and `OrbitControls.js`, from
[three.js](https://threejs.org) **0.186.1**, fetched from the npm package as
`https://cdn.jsdelivr.net/npm/three@0.186.1/` serves it: `build/three.module.js`,
`build/three.core.js` and `examples/jsm/controls/OrbitControls.js`. They are the
package's own files, not a CDN's minified copies.

Copyright 2010–2026 three.js authors, licensed under the **MIT License**; the
notice is in `LICENSE`, which is what the licence asks for.

## What changed

`OrbitControls.js` imports from the bare specifier `'three'`, which a page
can resolve only through an import map — an inline script that Proton's
content security policy refuses. That one import now names
`'./three.module.js'` beside it. Nothing else was changed.

| File | SHA-256 as fetched |
|---|---|
| `three.module.js` | `9052042d676cb0fdc1ddfefe193053f34b7ac0513a616fdac4535d49987812ea` |
| `three.core.js` | `9edde002b066a9a05676a6127f67735b62baf399bdea529f2f7e31657da769e6` |
| `OrbitControls.js` | `3d79d07ecb686b4e5d93232eedab255331c1beef711e13164eaa1f68655a5f2b` (before the import was changed) |

## Why it is in the repository rather than fetched

Proton's content security policy is `script-src 'self'`. Nothing loads from a
CDN at run time, deliberately: a Lab that needs the network to show a run's
scene is a Lab that stops working on a train.

## Why it is not imported at boot

It is two megabytes. `ui/lab3d.js` imports it the first time a run's 3D scene
is opened, and never before.

`three.module.d.ts` and `OrbitControls.d.ts` describe what `ui/lab3d.js` uses,
so the type checker reads them rather than the library.
