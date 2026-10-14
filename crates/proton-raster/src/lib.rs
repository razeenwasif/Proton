//! Canvas mode: Proton's own pixel-art and painting engine.
//!
//! One engine for two presets. **Pixel** is a small grid seen through
//! nearest-neighbour, drawn a pixel at a time from a palette, and grown with
//! procedural generators; **Paint** is a large canvas of soft, pressure-led
//! brushes over layers that blend. A pixel editor is a painter with a one-pixel
//! hard brush, a grid and a palette, so they share everything below the tools.
//!
//! ## Whole numbers only
//!
//! The window paints a stroke the moment the pen moves, on its own copy of the
//! canvas, by this crate built as WebAssembly; the core applies the same
//! stroke to the document it saves. They must make the same pixels, to the
//! byte, or the picture drifts from the file. So nothing here uses a float,
//! an unordered map, or arithmetic on `usize` (32 bits in WebAssembly, 64
//! natively); `tests/engine.rs` keeps it so. Inputs — positions, pressure,
//! transforms — are made whole numbers once, before either side sees them.
//!
//! ## The pieces
//!
//! - [`fixed`]: rounding, square roots and hashing in whole numbers.
//! - [`tile`]: pixels in 64 × 64 tiles, shared until changed; empty ones absent.
//! - [`canvas`]: the document — its size, preset, palette and layers.
//! - [`blend`]: the layers composited, mode by mode, as the W3C defines them.
//! - [`op`]: every change, as a value: applied whole or not at all.
//! - [`stroke`], [`fill`]: what a pencil, a shape or a bucket covers, as a
//!   mask, and painting it.
//! - [`brush`]: dabs along a path, sized and weighted by the pen's pressure,
//!   building up a stroke's cover toward its opacity.
//! - [`select`], [`sample`]: what is selected and edits held to it; pixels
//!   moved, turned, resized and cropped.
//! - [`history`]: undo and redo by the tiles each change touched.
//! - [`file`]: a canvas on disk, a folder of PNGs and a `canvas.json` (`io`).

pub mod blend;
pub mod brush;
pub mod canvas;
#[cfg(feature = "io")]
pub mod file;
pub mod fill;
pub mod fixed;
pub mod history;
pub mod op;
pub mod sample;
pub mod select;
pub mod stroke;
pub mod text;
pub mod tile;

pub use brush::{Brush, Dabs};
pub use canvas::{Blend, Canvas, Layer, LayerInfo, Preset, SELECTION};
pub use select::{Area, Mode};
pub use history::History;
pub use op::{Applied, Op, ShapeKind};
pub use stroke::{Stroke, Tip, Tool};
pub use tile::{Grid, Tile, TILE};
