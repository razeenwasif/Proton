//! Every change to a canvas, as a value.
//!
//! The window sends one when a gesture ends; the agent sends them; the core
//! applies them and keeps what each changed for undo. An op is applied whole
//! or not at all: one that cannot be made says why and leaves the canvas as
//! it was. What one changed is reported as the tiles it touched, by layer —
//! what the window must be told of, and what is hashed to check that the
//! window, which painted the same op on its own copy, made the same pixels.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::canvas::{Blend, Canvas, Layer};
use crate::fixed::Hash;
use crate::history::Patch;
use crate::tile::Tile;

/// The most colours a palette holds.
pub const PALETTE: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterOp")]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    /// A new empty layer, `at` places from the bottom (on top if not said).
    #[serde(rename_all = "camelCase")]
    AddLayer {
        #[serde(default)]
        name: String,
        #[serde(default)]
        #[ts(optional)]
        at: Option<u32>,
    },
    RemoveLayer { layer: u32 },
    /// A layer moved to place `to` from the bottom.
    MoveLayer { layer: u32, to: u32 },
    /// What a layer is: each field changed only if it is given.
    #[serde(rename_all = "camelCase")]
    SetLayer {
        layer: u32,
        #[serde(default)]
        #[ts(optional)]
        name: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        opacity: Option<u8>,
        #[serde(default)]
        #[ts(optional)]
        blend: Option<Blend>,
        #[serde(default)]
        #[ts(optional)]
        hidden: Option<bool>,
        #[serde(default)]
        #[ts(optional)]
        locked: Option<bool>,
        #[serde(default)]
        #[ts(optional)]
        alpha_lock: Option<bool>,
    },
    SetPalette { colors: Vec<[u8; 4]> },
    /// A rectangle (left, top, width, height) of a layer set to one colour —
    /// a clear one clears it. With alpha-lock only the colour of what is
    /// already there changes.
    FillRect { layer: u32, rect: [i32; 4], color: [u8; 4] },
}

/// What an op changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// Whether anything did.
    pub changed: bool,
    /// The tiles whose pixels may have, as (layer, tile index).
    pub touched: Vec<(u32, u32)>,
    /// Whether the layers themselves did — their order, names, opacity.
    pub meta: bool,
    /// A layer it made.
    pub made: Option<u32>,
}

impl Canvas {
    /// Apply `op`, and say what it changed and how to take it back. The
    /// canvas's state moves on only if something changed.
    pub fn apply(&mut self, op: &Op) -> Result<(Applied, Patch), String> {
        let mut patch = Patch::default();
        let mut applied = Applied::default();
        match op {
            Op::AddLayer { name, at } => {
                patch.layers = Some((self.layers.clone(), self.next_id));
                let name = if name.trim().is_empty() { format!("Layer {}", self.next_id) } else { name.trim().to_string() };
                let id = self.push_layer(name);
                if let Some(at) = at {
                    let layer = self.layers.pop().expect("just pushed");
                    let at = (*at as usize).min(self.layers.len());
                    self.layers.insert(at, layer);
                }
                applied.made = Some(id);
                applied.meta = true;
                applied.changed = true;
            }
            Op::RemoveLayer { layer } => {
                let at = self.place(*layer).ok_or_else(|| format!("there is no layer {layer}"))?;
                if self.layers.len() == 1 {
                    return Err("a canvas keeps at least one layer".into());
                }
                patch.layers = Some((self.layers.clone(), self.next_id));
                let gone = self.layers.remove(at as usize);
                applied.touched = present(&gone);
                applied.meta = true;
                applied.changed = true;
            }
            Op::MoveLayer { layer, to } => {
                let at = self.place(*layer).ok_or_else(|| format!("there is no layer {layer}"))?;
                let to = (*to).min(self.layers.len() as u32 - 1);
                if to != at {
                    patch.layers = Some((self.layers.clone(), self.next_id));
                    let moved = self.layers.remove(at as usize);
                    // What is seen changes where it has pixels: what it is
                    // over and under changed there.
                    applied.touched = present(&moved);
                    self.layers.insert(to as usize, moved);
                    applied.meta = true;
                    applied.changed = true;
                }
            }
            Op::SetLayer { layer, name, opacity, blend, hidden, locked, alpha_lock } => {
                let before = self.layer(*layer).ok_or_else(|| format!("there is no layer {layer}"))?.info.clone();
                if name.as_ref().is_some_and(|n| n.trim().is_empty()) {
                    return Err("a layer needs a name".into());
                }
                let mut info = before.clone();
                if let Some(n) = name {
                    info.name = n.trim().to_string();
                }
                info.opacity = opacity.unwrap_or(info.opacity);
                info.blend = blend.unwrap_or(info.blend);
                info.hidden = hidden.unwrap_or(info.hidden);
                info.locked = locked.unwrap_or(info.locked);
                info.alpha_lock = alpha_lock.unwrap_or(info.alpha_lock);
                if info != before {
                    patch.layers = Some((self.layers.clone(), self.next_id));
                    let shows = info.hidden != before.hidden || info.opacity != before.opacity || info.blend != before.blend;
                    let target = self.layer_mut(*layer).expect("found above");
                    target.info = info;
                    // What is seen changed wherever the layer has pixels.
                    if shows {
                        applied.touched = present(target);
                    }
                    applied.meta = true;
                    applied.changed = true;
                }
            }
            Op::SetPalette { colors } => {
                if colors.len() as u32 > PALETTE {
                    return Err(format!("a palette holds at most {PALETTE} colours"));
                }
                if *colors != self.palette {
                    patch.palette = Some(self.palette.clone());
                    self.palette = colors.clone();
                    applied.meta = true;
                    applied.changed = true;
                }
            }
            Op::FillRect { layer, rect, color } => {
                let target = self.layer(*layer).ok_or_else(|| format!("there is no layer {layer}"))?;
                if target.info.locked {
                    return Err(format!("{} is locked", target.info.name));
                }
                let lock = target.info.alpha_lock;
                let [x, y, w, h] = rect.map(|v| v as i64);
                let tiles = target.grid.covering(x, y, w, h);
                let target = self.layer_mut(*layer).expect("found above");
                for index in tiles {
                    let before = target.grid.tile(index).cloned();
                    let (tx, ty, tw, th) = target.grid.bounds(index);
                    let mut changed = false;
                    {
                        let tile = target.grid.tile_mut(index);
                        for py in 0..th {
                            for px in 0..tw {
                                let (cx, cy) = ((tx + px) as i64, (ty + py) as i64);
                                if cx < x || cy < y || cx >= x + w || cy >= y + h {
                                    continue;
                                }
                                let old = tile.get(px, py);
                                let new = if lock {
                                    if old[3] == 0 { old } else { [color[0], color[1], color[2], old[3]] }
                                } else {
                                    *color
                                };
                                if new != old {
                                    tile.set(px, py, new);
                                    changed = true;
                                }
                            }
                        }
                    }
                    target.grid.tidy(index);
                    if changed {
                        patch.tiles.push((*layer, index, before));
                        applied.touched.push((*layer, index));
                        applied.changed = true;
                    } else {
                        // Made to look at it, and nothing differed: as it was.
                        target.grid.put(index, before);
                    }
                }
            }
        }
        if applied.changed {
            self.advance();
        }
        Ok((applied, patch))
    }

    /// One number for the tiles an op touched, as they are now: what the
    /// window's copy and the core's must agree on.
    pub fn hash_touched(&self, touched: &[(u32, u32)]) -> u64 {
        let mut sorted = touched.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let mut h = Hash::default();
        for (layer, index) in sorted {
            h.word(layer as u64);
            h.word(index as u64);
            h.word(self.layer(layer).map(|l| l.grid.hash(index)).unwrap_or(crate::tile::CLEAR));
        }
        h.0
    }

    /// Put back what a patch holds; the patch that would undo that, and what
    /// it changed.
    pub fn restore(&mut self, patch: Patch) -> (Patch, Applied) {
        let mut inverse = Patch::default();
        let mut applied = Applied { changed: true, ..Applied::default() };
        if let Some((layers, next_id)) = patch.layers {
            let old = std::mem::replace(&mut self.layers, layers);
            applied.touched = differ(&old, &self.layers);
            inverse.layers = Some((old, self.next_id));
            self.next_id = next_id;
            applied.meta = true;
        }
        if let Some(palette) = patch.palette {
            inverse.palette = Some(std::mem::replace(&mut self.palette, palette));
            applied.meta = true;
        }
        for (layer, index, tile) in patch.tiles {
            if let Some(target) = self.layer_mut(layer) {
                let now = target.grid.tile(index).cloned();
                target.grid.put(index, tile);
                inverse.tiles.push((layer, index, now));
                applied.touched.push((layer, index));
            }
        }
        (inverse, applied)
    }
}

/// The tiles a layer has, as (layer, index).
pub fn present(layer: &Layer) -> Vec<(u32, u32)> {
    let id = layer.info.id;
    (0..layer.grid.cols * layer.grid.rows).filter(|&i| layer.grid.tile(i).is_some()).map(|i| (id, i)).collect()
}

/// The tiles that differ between two stacks of layers, by layer id: a layer
/// in only one of them differs wherever it has pixels, and one that moved, or
/// is drawn differently, wherever it has them.
pub fn differ(a: &[Layer], b: &[Layer]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let same = |x: Option<&Arc<Tile>>, y: Option<&Arc<Tile>>| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => Arc::ptr_eq(x, y),
        _ => false,
    };
    for layer in a.iter().chain(b.iter()) {
        let id = layer.info.id;
        if out.iter().any(|(l, _)| *l == id) {
            continue;
        }
        let (x, y) = (a.iter().find(|l| l.info.id == id), b.iter().find(|l| l.info.id == id));
        match (x, y) {
            (Some(x), Some(y)) => {
                let place = |s: &[Layer]| s.iter().position(|l| l.info.id == id);
                let shows = x.info.hidden != y.info.hidden || x.info.opacity != y.info.opacity || x.info.blend != y.info.blend || place(a) != place(b);
                for i in 0..x.grid.cols * x.grid.rows {
                    if shows && (x.grid.tile(i).is_some() || y.grid.tile(i).is_some()) || !same(x.grid.tile(i), y.grid.tile(i)) {
                        out.push((id, i));
                    }
                }
            }
            (Some(only), None) | (None, Some(only)) => out.extend(present(only)),
            (None, None) => {}
        }
    }
    out
}
