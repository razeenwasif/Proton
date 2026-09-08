//! Design mode's document: the tokens in a `theme.css`.
//!
//! Design's document is component source plus a stylesheet of custom
//! properties, and this owns the second half. It is the same job
//! `proton-data::notebook` does for `.ipynb`, and it is here for the same
//! reason: the format comes first, because everything drawn on top of it is
//! only as trustworthy as the file it writes back.
//!
//! ## The file is edited, not regenerated
//!
//! A theme is somebody's stylesheet. It has comments explaining why a colour
//! is what it is, blank lines that group tokens, a media query for the dark
//! palette, and rules that have nothing to do with tokens at all. A writer
//! that parsed all that into a model and printed the model back would produce
//! a diff touching every line, and the load-bearing rule of this project is
//! that every mode's document diffs in Git. So a change here is a splice: the
//! bytes of one value are replaced and nothing else in the file moves.
//!
//! That is why [`Token`] carries the span its value occupied rather than only
//! the value. The span is the edit.
//!
//! ## What counts as a token
//!
//! A CSS custom property — `--name: value` — declared inside any block. The
//! block it was in is kept as its scope, because a theme states the same token
//! more than once on purpose: once for light, once inside
//! `@media (prefers-color-scheme: dark)`, once under `[data-theme="dark"]`.
//! Three tokens named `--bg` that are one token would be a palette that could
//! only be edited in whichever place happened to be found first.

mod component;
pub mod markup;
mod theme;

pub use component::{is_markup, Component};
pub use markup::{layers, Layer};
pub use theme::{Theme, Token};
