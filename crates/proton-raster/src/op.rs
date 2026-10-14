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

use crate::brush::{self, Brush, Dabs};
use crate::canvas::{Blend, Canvas, Layer};
use crate::fill;
use crate::fixed::Hash;
use crate::history::Patch;
use crate::stroke::{self, Mask, Stroke, Tip, Tool};
use crate::tile::Tile;

/// The most points one stroke may carry: minutes of a pen at 240 a second.
pub const POINTS: u32 = 200_000;

/// What a shape is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/", rename = "RasterShape")]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    Line,
    Rect,
    Ellipse,
}

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
    /// A pencil or eraser drawn through `points`: x, y in 1/256 px and a
    /// pressure, three numbers each. Pixel-perfect takes the corners out of
    /// its steps.
    #[serde(rename_all = "camelCase")]
    Stroke {
        layer: u32,
        tool: Tool,
        tip: Tip,
        color: [u8; 4],
        #[serde(default)]
        pixel_perfect: bool,
        points: Vec<i32>,
    },
    /// A brush drawn through `points`, as a stroke's: dabs a spacing apart,
    /// sized and weighted by each point's pressure. Erasing, it takes away
    /// what is there by as much as it would have covered.
    #[serde(rename_all = "camelCase")]
    Brush {
        layer: u32,
        #[serde(default)]
        erase: bool,
        brush: Brush,
        color: [u8; 4],
        points: Vec<i32>,
    },
    /// A paint bucket at pixel `x`, `y`: what is like it, within
    /// `tolerance`, takes the colour.
    #[serde(rename_all = "camelCase")]
    Fill {
        layer: u32,
        x: i32,
        y: i32,
        color: [u8; 4],
        #[serde(default)]
        tolerance: u8,
        contiguous: bool,
        #[serde(default)]
        diagonal: bool,
    },
    /// A line, a rectangle or an ellipse, from one pixel to another.
    #[serde(rename_all = "camelCase")]
    Shape {
        layer: u32,
        shape: ShapeKind,
        from: [i32; 2],
        to: [i32; 2],
        #[serde(default)]
        filled: bool,
        tip: Tip,
        color: [u8; 4],
    },
    /// One colour made another, on one layer or on every layer not locked —
    /// and a palette's swatch with it, so recolouring a swatch is one edit.
    #[serde(rename_all = "camelCase")]
    ReplaceColor {
        from: [u8; 4],
        to: [u8; 4],
        #[serde(default)]
        #[ts(optional)]
        layer: Option<u32>,
        #[serde(default)]
        #[ts(optional)]
        swatch: Option<u32>,
    },
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
            Op::Stroke { layer, tool, tip, color, pixel_perfect, points } => {
                check_tip(tip)?;
                if points.len() % 3 != 0 || points.len() as u32 > 3 * POINTS {
                    return Err(format!("a stroke is points of three numbers, at most {POINTS} of them"));
                }
                let mut stroke = Stroke::new(tip.clone(), *pixel_perfect, self.width, self.height);
                stroke.add(points);
                self.lay(*layer, &stroke.mask, *tool, *color, &mut patch, &mut applied)?;
            }
            Op::Brush { layer, erase, brush, color, points } => {
                brush.check()?;
                if points.len() % 3 != 0 || points.len() as u32 > 3 * POINTS {
                    return Err(format!("a stroke is points of three numbers, at most {POINTS} of them"));
                }
                let mut dabs = Dabs::new(brush.clone(), self.width, self.height);
                dabs.add(points);
                let target = self.layer(*layer).ok_or_else(|| format!("there is no layer {layer}"))?;
                if target.info.locked {
                    return Err(format!("{} is locked", target.info.name));
                }
                let lock = target.info.alpha_lock;
                let (width, height) = (self.width, self.height);
                let target = self.layer_mut(*layer).expect("found above");
                for index in dabs.mask.tiles() {
                    let before = target.grid.tile(index).cloned();
                    let after = brush::paint(&dabs.mask, index, before.as_ref(), None, *erase, *color, lock, width, height);
                    if after != before {
                        target.grid.put(index, after);
                        patch.tiles.push((*layer, index, before));
                        applied.touched.push((*layer, index));
                        applied.changed = true;
                    }
                }
            }
            Op::Fill { layer, x, y, color, tolerance, contiguous, diagonal } => {
                let target = self.layer(*layer).ok_or_else(|| format!("there is no layer {layer}"))?;
                let mask = fill::flood(&target.grid, *x, *y, *tolerance, *contiguous, *diagonal);
                self.lay(*layer, &mask, Tool::Pencil, *color, &mut patch, &mut applied)?;
            }
            Op::Shape { layer, shape, from, to, filled, tip, color } => {
                check_tip(tip)?;
                let (a, b) = ((from[0], from[1]), (to[0], to[1]));
                let pixels = match shape {
                    ShapeKind::Line => stroke::line(a, b),
                    ShapeKind::Rect => rect(a, b, *filled),
                    ShapeKind::Ellipse => stroke::ellipse(a, b, *filled),
                };
                let mut mask = Mask::new(self.width, self.height);
                let mut stamps = Vec::new();
                for (x, y) in pixels {
                    stamps.clear();
                    tip.stamps(x, y, &mut stamps);
                    for &(sx, sy) in &stamps {
                        mask.stamp(sx, sy, true);
                    }
                }
                self.lay(*layer, &mask, Tool::Pencil, *color, &mut patch, &mut applied)?;
            }
            Op::ReplaceColor { from, to, layer, swatch } => {
                if let Some(i) = swatch {
                    if *i as usize >= self.palette.len() {
                        return Err(format!("the palette has no swatch {i}"));
                    }
                }
                let ids: Vec<u32> = match layer {
                    Some(id) => {
                        let target = self.layer(*id).ok_or_else(|| format!("there is no layer {id}"))?;
                        if target.info.locked {
                            return Err(format!("{} is locked", target.info.name));
                        }
                        vec![*id]
                    }
                    None => self.layers.iter().filter(|l| !l.info.locked).map(|l| l.info.id).collect(),
                };
                for id in ids {
                    let target = self.layer_mut(id).expect("found above");
                    for index in 0..target.grid.cols * target.grid.rows {
                        let Some(before) = target.grid.tile(index).cloned() else { continue };
                        if !before.px.chunks_exact(4).any(|p| p == from) {
                            continue;
                        }
                        let mut tile = (*before).clone();
                        for p in tile.px.chunks_exact_mut(4) {
                            if p == from {
                                p.copy_from_slice(to);
                            }
                        }
                        target.grid.put(index, (!tile.is_clear()).then(|| Arc::new(tile)));
                        patch.tiles.push((id, index, Some(before)));
                        applied.touched.push((id, index));
                        applied.changed = true;
                    }
                }
                if let Some(i) = swatch {
                    if self.palette[*i as usize] != *to {
                        patch.palette = Some(self.palette.clone());
                        self.palette[*i as usize] = *to;
                        applied.meta = true;
                        applied.changed = true;
                    }
                }
            }
        }
        if applied.changed {
            self.advance();
        }
        Ok((applied, patch))
    }

    /// What `mask` covers on a layer, painted by `tool` in `color`: each tile
    /// that changed remembered for undo and told as touched.
    fn lay(&mut self, layer: u32, mask: &Mask, tool: Tool, color: [u8; 4], patch: &mut Patch, applied: &mut Applied) -> Result<(), String> {
        let target = self.layer(layer).ok_or_else(|| format!("there is no layer {layer}"))?;
        if target.info.locked {
            return Err(format!("{} is locked", target.info.name));
        }
        let lock = target.info.alpha_lock;
        let (width, height) = (self.width, self.height);
        let target = self.layer_mut(layer).expect("found above");
        for index in mask.tiles() {
            let before = target.grid.tile(index).cloned();
            let after = stroke::paint(mask, index, before.as_ref(), tool, color, lock, width, height);
            if after != before {
                target.grid.put(index, after);
                patch.tiles.push((layer, index, before));
                applied.touched.push((layer, index));
                applied.changed = true;
            }
        }
        Ok(())
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

fn check_tip(tip: &Tip) -> Result<(), String> {
    match tip.size {
        1..=Tip::MOST => Ok(()),
        n => Err(format!("a tip is 1 to {} pixels across, not {n}", Tip::MOST)),
    }
}

/// The pixels of a rectangle between two corners: its edge, or all of it.
fn rect(a: (i32, i32), b: (i32, i32), filled: bool) -> Vec<(i32, i32)> {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    let mut out = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            if filled || x == x0 || x == x1 || y == y0 || y == y1 {
                out.push((x, y));
            }
        }
    }
    out
}
