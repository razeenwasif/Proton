# KaTeX — vendored

`katex.mjs`, `katex.min.css` and the twenty `fonts/*.woff2`, from
[KaTeX](https://katex.org) **0.19.0**, fetched from
`https://cdnjs.cloudflare.com/ajax/libs/KaTeX/0.19.0/`.

Copyright Khan Academy and other contributors, licensed under the **MIT
License**; the notice is in `LICENSE`, which is what the licence asks for.

## What changed

The stylesheet names each font three times — woff2, woff and ttf — and only
the woff2 files are here: every engine Proton runs in reads them. Its `src`
lists were cut to the woff2 alone, so nothing asks for a file that is not
there. Nothing else was changed.

## Why it is in the repository rather than fetched

Proton's content security policy is `script-src 'self'` and `font-src 'self'`.
Nothing loads from a CDN at run time, deliberately: an editor that needs the
network to show an equation is an editor that stops working on a train.

## Why it is not imported at boot

The module is 600KB, and a session whose replies hold no mathematics should
never pay for it. `ui/markdown.js` imports it, and adds the stylesheet, the
first time a reply has an equation in it, and draws the equations already on
screen once it has arrived.

`katex.d.mts` describes the two calls Proton makes, so the type checker reads
that rather than the library.
