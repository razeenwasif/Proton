# Proton brand

| File | Use |
|---|---|
| `proton-icon.svg` | Master app icon — dark-crimson rounded tile, white art. Vector source of truth. |
| `proton-icon-{16..1024}.png` | Rasterised from the SVG. |
| `proton.ico` | Multi-size Windows icon (16/32/48/64/128/256), for the exe and installer. |
| `proton-mark.svg` | Glyph only, painted with `currentColor`. Use in docs and favicons — anywhere the crimson tile would fight the surface. |

## Mark

A nucleus with three orbits at 60° and three electrons — the classic atom, drawn
as thinly as it can be and still hold together. It stays legible down to 32px;
at 16px it collapses to a bright core in a lattice, which is the intended read.

The tile stays crimson even though the app themes are cool. An icon is found in
a dock, not colour-matched to a window — and it is the one place Proton is
allowed a colour of its own.

## Palette

The icon has its own palette — the application themes live in
`ui/styles.css` and share nothing with it.

| | Value | Role |
|---|---|---|
| Tile top | `#7A0F26` | Gradient start, upper left |
| Tile mid | `#520A19` | |
| Tile bottom | `#2E050F` | Gradient end |
| Glow | `#FF5C79` at 28% | Radial, behind the mark |
| Art | `#FFFFFF` | Orbits, electrons, nucleus |

A white 16% sheen covers the top half, and a 1px white 10% inner stroke keeps
the tile edge from disappearing on a dark dock.

## Regenerating

The PNGs and the `.ico` are derived. Edit `proton-icon.svg`, then re-rasterise —
`brand/build-icons.mjs` renders every size from the SVG and repacks `proton.ico`.

## The UI cut

The application draws its own copy of the mark, inlined as `#i-proton` in
`ui/index.html`. It is the same construction with two optical
changes: strokes are **20** rather than 13, and the electrons are **r27** rather
than r21.

That is deliberate, not drift. The master is drawn for a 512px tile; the UI
copy renders at 13–42px, where a 13/512 stroke works out under a pixel and the
orbits wash out to grey. Scale the master down and you get mush — so the small
cut is drawn heavier and the two are kept in sync by hand.
