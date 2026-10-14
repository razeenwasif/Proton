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

/// The palette a new pixel canvas starts with: sixteen of Proton's own — a
/// ramp of greys, and a dark, a mid and a light of the warm and cool hues a
/// sprite most often wants.
pub const PALETTE16: [[u8; 4]; 16] = [
    [0x1b, 0x1b, 0x23, 0xff],
    [0x3d, 0x3d, 0x4f, 0xff],
    [0x6e, 0x6e, 0x86, 0xff],
    [0xa8, 0xa8, 0xbf, 0xff],
    [0xf2, 0xf2, 0xf7, 0xff],
    [0x5a, 0x1f, 0x2e, 0xff],
    [0xb0, 0x30, 0x3f, 0xff],
    [0xe8, 0x74, 0x3b, 0xff],
    [0xf4, 0xc9, 0x5d, 0xff],
    [0x8f, 0xd1, 0x6a, 0xff],
    [0x3e, 0x9e, 0x5c, 0xff],
    [0x1f, 0x5f, 0x5b, 0xff],
    [0x2f, 0x6f, 0xb0, 0xff],
    [0x6f, 0xb7, 0xe8, 0xff],
    [0x6b, 0x3f, 0xa0, 0xff],
    [0xe0, 0x7a, 0xb8, 0xff],
];

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

/// The most frames a canvas may have.
pub const FRAMES_MOST: u32 = 1000;

/// How long a frame is shown when nothing says otherwise, in milliseconds.
pub const DURATION: u32 = 100;

/// The longest a frame may be shown, in milliseconds: a minute.
pub const DURATION_MOST: u32 = 60_000;

/// Layer ids are below this; a cel — a layer in one frame — is known by its
/// layer's id and this many times its frame. A layer's id is its first cel's.
pub const FRAME_STEP: u32 = 1 << 16;

/// The id of a layer's cel in a frame (from 0).
pub fn cel_id(layer: u32, frame: u32) -> u32 {
    layer + frame * FRAME_STEP
}

/// A cel's layer and frame.
pub fn cel_of(id: u32) -> (u32, u32) {
    (id % FRAME_STEP, id / FRAME_STEP)
}

/// The order a tag's frames play in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterDirection")]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Forward,
    Reverse,
    /// To the last and back.
    PingPong,
    /// From the last to the first and back.
    PingPongReverse,
}

/// A run of frames with a name — "idle", "run" — played as one animation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterTag")]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub name: String,
    /// The first frame and the last, from 0.
    pub from: u32,
    pub to: u32,
    #[serde(default)]
    pub direction: Direction,
    /// How many times it plays; 0 for as long as it is shown.
    #[serde(default)]
    pub repeat: u32,
}

/// The most tags a canvas may have.
pub const TAGS_MOST: u32 = 256;

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

/// A layer: what it is, and its pixels in each frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub info: LayerInfo,
    /// Its pixels frame by frame — its cels — one for every frame.
    pub cels: Vec<Grid>,
}

impl Layer {
    /// Its pixels in the first frame: all a still picture has.
    pub fn grid(&self) -> &Grid {
        &self.cels[0]
    }

    /// The tiles any of its cels has, by cel id and index.
    pub fn present(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for (frame, grid) in self.cels.iter().enumerate() {
            let id = cel_id(self.info.id, frame as u32);
            out.extend((0..grid.cols * grid.rows).filter(|&i| grid.tile(i).is_some()).map(|i| (id, i)));
        }
        out
    }
}

/// A layer in one frame: what the layer is, and its pixels there.
pub struct Cel<'a> {
    pub info: &'a LayerInfo,
    pub grid: &'a Grid,
}

pub struct CelMut<'a> {
    pub info: &'a LayerInfo,
    pub grid: &'a mut Grid,
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
    /// How long each frame is shown, in milliseconds: one for every frame,
    /// and a still picture has one.
    pub frames: Vec<u32>,
    /// Runs of frames with names, played as animations.
    pub tags: Vec<Tag>,
    /// The id the next layer made gets.
    pub next_id: u32,
    /// Which state of the document this is: changed by every change, set back
    /// by undo. A canvas is unsaved when this differs from the state saved.
    pub state: u64,
    /// The last state given out, so no two states share a number.
    pub made: u64,
    /// What is selected, as the alpha of a grid the canvas's size: how much
    /// of each pixel an edit may change. A grid with no tiles selects
    /// nothing, and then every edit changes everywhere. Not saved; undone and
    /// redone as edits are. To the window it is layer 0, fetched and checked
    /// as layers' tiles are.
    pub selection: Grid,
    /// Pixels copied, to paste: as they were and where they were, a grid the
    /// canvas's size. Not saved, not undone.
    pub clip: Option<Grid>,
}

/// The id the window knows the selection by, as if it were a layer.
pub const SELECTION: u32 = 0;

impl Canvas {
    /// A canvas of one empty layer.
    pub fn new(width: u32, height: u32, preset: Preset) -> Result<Canvas, String> {
        if width == 0 || height == 0 {
            return Err("a canvas needs a width and a height".into());
        }
        if width > preset.most() || height > preset.most() {
            return Err(format!("a {} canvas is at most {} pixels a side", name(preset), preset.most()));
        }
        let mut canvas = Canvas { width, height, preset, palette: Vec::new(), layers: Vec::new(), frames: vec![DURATION], tags: Vec::new(), next_id: 1, state: 0, made: 0, selection: Grid::new(width, height), clip: None };
        canvas.push_layer(if preset == Preset::Pixel { "Sprite" } else { "Background" }.into());
        Ok(canvas)
    }

    /// A new empty layer on top, clear in every frame; its id.
    pub fn push_layer(&mut self, name: String) -> u32 {
        // Ids are never used twice while the canvas is open; one past the
        // most a cel's id leaves room for takes the least not in use.
        if self.next_id >= FRAME_STEP {
            self.next_id = (1..FRAME_STEP).find(|id| self.layer(*id).is_none()).unwrap_or(FRAME_STEP - 1);
        }
        let id = self.next_id;
        self.next_id += 1;
        let cels = self.frames.iter().map(|_| Grid::new(self.width, self.height)).collect();
        self.layers.push(Layer { info: LayerInfo { id, name, opacity: 255, blend: Blend::Normal, hidden: false, locked: false, alpha_lock: false }, cels });
        id
    }

    /// A layer by its id — a layer's, not a cel's in a later frame.
    pub fn layer(&self, id: u32) -> Option<&Layer> {
        self.layers.iter().find(|l| l.info.id == id)
    }

    pub fn layer_mut(&mut self, id: u32) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.info.id == id)
    }

    /// A cel by its id: a layer's own id is its cel in the first frame.
    pub fn cel(&self, id: u32) -> Option<Cel<'_>> {
        let (layer, frame) = cel_of(id);
        let l = self.layer(layer)?;
        Some(Cel { info: &l.info, grid: l.cels.get(frame as usize)? })
    }

    pub fn cel_mut(&mut self, id: u32) -> Option<CelMut<'_>> {
        let (layer, frame) = cel_of(id);
        let l = self.layer_mut(layer)?;
        Some(CelMut { info: &l.info, grid: l.cels.get_mut(frame as usize)? })
    }

    /// A cel's pixels by its id — or, for [`SELECTION`], the selection's.
    pub fn grid(&self, id: u32) -> Option<&Grid> {
        if id == SELECTION {
            return Some(&self.selection);
        }
        self.cel(id).map(|c| c.grid)
    }

    pub fn grid_mut(&mut self, id: u32) -> Option<&mut Grid> {
        if id == SELECTION {
            return Some(&mut self.selection);
        }
        self.cel_mut(id).map(|c| c.grid)
    }

    /// Whether the canvas is more than a still picture: frames, or tags.
    pub fn animated(&self) -> bool {
        self.frames.len() > 1 || !self.tags.is_empty()
    }

    /// Whether anything is selected: edits are then held to it.
    pub fn selecting(&self) -> bool {
        self.selection.tiles.iter().any(Option::is_some)
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
            // A still picture's hash is what it was before there were frames.
            for (frame, grid) in layer.cels.iter().enumerate() {
                if frame > 0 {
                    h.word(frame as u64);
                }
                for index in 0..grid.cols * grid.rows {
                    h.word(grid.hash(index));
                }
            }
        }
        if self.animated() || self.frames != [DURATION] {
            for d in &self.frames {
                h.word(*d as u64);
            }
            for t in &self.tags {
                h.bytes(t.name.as_bytes());
                h.word(t.from as u64 | (t.to as u64) << 16 | (t.direction as u64) << 32 | (t.repeat as u64) << 40);
            }
        }
        if self.selecting() {
            for index in 0..self.selection.cols * self.selection.rows {
                h.word(self.selection.hash(index));
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
