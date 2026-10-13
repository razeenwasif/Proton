//! A canvas: its size, its preset, its palette, and its layers bottom to top.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::fixed::Hash;
use crate::tile::Grid;

/// Which of the two ways of working a canvas is made for. It decides what the
/// window offers — a grid and a palette, or soft brushes — and how large a
/// canvas may be; the pixels are the same either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterPreset")]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    #[default]
    Pixel,
    Paint,
}

impl Preset {
    /// The largest side a canvas of this preset may have.
    pub fn most(self) -> u32 {
        match self {
            Preset::Pixel => 1024,
            Preset::Paint => 4096,
        }
    }

    /// The most bytes of history it keeps, and the most steps.
    pub fn history(self) -> (u64, u32) {
        match self {
            Preset::Pixel => (64 << 20, 2000),
            Preset::Paint => (512 << 20, 1000),
        }
    }
}

/// How a layer is laid over what is under it: the W3C's separable modes,
/// and Photoshop's add and subtract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterBlend")]
#[serde(rename_all = "camelCase")]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Add,
    Subtract,
}

impl Blend {
    pub const ALL: [Blend; 14] = [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Overlay,
        Blend::Darken,
        Blend::Lighten,
        Blend::ColorDodge,
        Blend::ColorBurn,
        Blend::HardLight,
        Blend::SoftLight,
        Blend::Difference,
        Blend::Exclusion,
        Blend::Add,
        Blend::Subtract,
    ];
}

/// What a layer is, apart from its pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterLayer")]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    pub id: u32,
    pub name: String,
    /// 0 to 255.
    pub opacity: u8,
    pub blend: Blend,
    pub hidden: bool,
    /// Not to be drawn on.
    pub locked: bool,
    /// Drawn on only where it already has something: shading inside a line.
    pub alpha_lock: bool,
}

/// A layer: what it is, and its pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub info: LayerInfo,
    pub grid: Grid,
}

/// A canvas, as the core holds it and as the window's WebAssembly mirrors it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub preset: Preset,
    /// Colours to draw with, as straight RGBA.
    pub palette: Vec<[u8; 4]>,
    /// Bottom to top.
    pub layers: Vec<Layer>,
    /// The id the next layer made gets.
    pub next_id: u32,
    /// Which state of the document this is: changed by every change, set back
    /// by undo. A canvas is unsaved when this differs from the state saved.
    pub state: u64,
    /// The last state given out, so no two states share a number.
    pub made: u64,
}

impl Canvas {
    /// A canvas of one empty layer.
    pub fn new(width: u32, height: u32, preset: Preset) -> Result<Canvas, String> {
        if width == 0 || height == 0 {
            return Err("a canvas needs a width and a height".into());
        }
        if width > preset.most() || height > preset.most() {
            return Err(format!("a {} canvas is at most {} pixels a side", name(preset), preset.most()));
        }
        let mut canvas = Canvas { width, height, preset, palette: Vec::new(), layers: Vec::new(), next_id: 1, state: 0, made: 0 };
        canvas.push_layer(if preset == Preset::Pixel { "Sprite" } else { "Background" }.into());
        Ok(canvas)
    }

    /// A new empty layer on top; its id.
    pub fn push_layer(&mut self, name: String) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.layers.push(Layer { info: LayerInfo { id, name, opacity: 255, blend: Blend::Normal, hidden: false, locked: false, alpha_lock: false }, grid: Grid::new(self.width, self.height) });
        id
    }

    pub fn layer(&self, id: u32) -> Option<&Layer> {
        self.layers.iter().find(|l| l.info.id == id)
    }

    pub fn layer_mut(&mut self, id: u32) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.info.id == id)
    }

    /// Where a layer is in the stack, from the bottom.
    pub fn place(&self, id: u32) -> Option<u32> {
        self.layers.iter().position(|l| l.info.id == id).map(|i| i as u32)
    }

    pub fn infos(&self) -> Vec<LayerInfo> {
        self.layers.iter().map(|l| l.info.clone()).collect()
    }

    /// A new state, for a change just made.
    pub fn advance(&mut self) {
        self.made += 1;
        self.state = self.made;
    }

    /// The whole document as one number: its size, palette, layers and every
    /// tile — for tests, and for telling two copies apart.
    pub fn hash(&self) -> u64 {
        let mut h = Hash::default();
        h.word(self.width as u64);
        h.word(self.height as u64);
        h.word(self.preset as u64);
        for c in &self.palette {
            h.word(u32::from_le_bytes(*c) as u64);
        }
        for layer in &self.layers {
            let i = &layer.info;
            h.word(i.id as u64);
            h.bytes(i.name.as_bytes());
            h.word(i.opacity as u64 | (i.blend as u64) << 8 | (i.hidden as u64) << 16 | (i.locked as u64) << 17 | (i.alpha_lock as u64) << 18);
            for index in 0..layer.grid.cols * layer.grid.rows {
                h.word(layer.grid.hash(index));
            }
        }
        h.0
    }
}

pub fn name(preset: Preset) -> &'static str {
    match preset {
        Preset::Pixel => "pixel",
        Preset::Paint => "paint",
    }
}
