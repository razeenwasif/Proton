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
use crate::canvas::{Blend, Canvas, Layer, SELECTION};
use crate::fill;
use crate::filter::{self, Filter};
use crate::gen::{self, Generator};
use crate::fixed::Hash;
use crate::history::Patch;
use crate::sample;
use crate::select::{self, Area, Mode, FEATHER_MOST};
use crate::stroke::{self, Mask, Stroke, Tip, Tool};
use crate::tile::{Grid, Tile};

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
    /// What is selected: `area` joined to it by `mode`, its edge feathered
    /// by `feather` pixels. Smooth, an ellipse's or a lasso's edge is soft by
    /// a pixel; a pixel canvas's are hard.
    #[serde(rename_all = "camelCase")]
    Select {
        area: Area,
        #[serde(default)]
        mode: Mode,
        #[serde(default)]
        feather: u32,
        #[serde(default)]
        smooth: bool,
    },
    /// What is selected of a layer cleared — all of it, with nothing selected.
    Clear { layer: u32 },
    /// What is selected of a layer copied, to paste — all of it, with
    /// nothing selected; cut, it is cleared as well.
    Copy {
        layer: u32,
        #[serde(default)]
        cut: bool,
    },
    /// What was copied, as a new layer where it was, over `above` (on top if
    /// not said).
    Paste {
        #[serde(default)]
        #[ts(optional)]
        above: Option<u32>,
    },
    /// What is selected of a layer — all of it, with nothing selected —
    /// moved, scaled or turned: each pixel of `bounds` (left, top, width,
    /// height) sampled from where `inverse` says it came from (see
    /// `sample::Inverse`), by the nearest pixel or, smooth, the four about
    /// it, and laid over what was left. The selection moves with it.
    Transform {
        layer: u32,
        bounds: [i32; 4],
        #[ts(type = "number[]")]
        inverse: [i64; 6],
        #[serde(default)]
        smooth: bool,
    },
    /// The whole canvas turned clockwise by `turns` quarters, then, if
    /// `flip`, mirrored left to right.
    Orient {
        turns: u8,
        #[serde(default)]
        flip: bool,
    },
    /// The whole canvas resized: its every layer, by the nearest pixel or,
    /// smooth, weighed.
    Resize {
        width: u32,
        height: u32,
        #[serde(default)]
        smooth: bool,
    },
    /// The canvas cut to `rect` (left, top, width, height) — or grown, a
    /// rectangle past its edges adding clear pixels.
    Crop { rect: [i32; 4] },
    /// A generator's picture laid over a layer, held to what is selected.
    Generate { layer: u32, generator: Generator },
    /// A layer made over by a filter, held to what is selected.
    Filter { layer: u32, filter: Filter },
    /// The whole canvas `factor` (2 or 3) times the size by Scale2x or
    /// Scale3x, a pixel picture's edges kept crisp.
    Upscale { factor: u32 },
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
                let sel = self.selecting().then(|| self.selection.clone());
                let target = self.layer_mut(*layer).expect("found above");
                for index in tiles {
                    let before = target.grid.tile(index).cloned();
                    let (tx, ty, tw, th) = target.grid.bounds(index);
                    let mut tile = before.as_deref().cloned().unwrap_or_default();
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
                            tile.set(px, py, new);
                        }
                    }
                    let mut after = (!tile.is_clear()).then(|| Arc::new(tile));
                    if let Some(sel) = &sel {
                        after = select::clip(sel, index, before.as_ref(), after);
                    }
                    if after != before {
                        target.grid.put(index, after);
                        patch.tiles.push((*layer, index, before));
                        applied.touched.push((*layer, index));
                        applied.changed = true;
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
                let sel = self.selecting().then(|| self.selection.clone());
                let target = self.layer_mut(*layer).expect("found above");
                for index in dabs.mask.tiles() {
                    let before = target.grid.tile(index).cloned();
                    let mut after = brush::paint(&dabs.mask, index, before.as_ref(), None, *erase, *color, lock, width, height);
                    if let Some(sel) = &sel {
                        after = select::clip(sel, index, before.as_ref(), after);
                    }
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
            Op::Select { area, mode, feather, smooth } => {
                if *feather > FEATHER_MOST {
                    return Err(format!("a feather is at most {FEATHER_MOST} pixels"));
                }
                let mut shape = select::shape(self, area, *smooth)?;
                let alpha = match area {
                    Area::All | Area::Nothing | Area::Invert => shape,
                    _ => {
                        select::feather(&mut shape, self.width, self.height, *feather);
                        let mut now = select::dense(&self.selection);
                        select::combine(&mut now, &shape, *mode);
                        now
                    }
                };
                let grid = select::from_dense(self.width, self.height, &alpha);
                self.select(grid, &mut patch, &mut applied);
            }
            Op::Clear { layer } => {
                let target = self.unlocked(*layer)?;
                let left = select::split(&target.grid, &self.selection, self.selecting()).1;
                self.set_grid(*layer, left, &mut patch, &mut applied);
            }
            Op::Copy { layer, cut } => {
                let target = if *cut { self.unlocked(*layer)? } else { self.layer(*layer).ok_or_else(|| format!("there is no layer {layer}"))? };
                let (lifted, left) = select::split(&target.grid, &self.selection, self.selecting());
                self.clip = Some(lifted);
                if *cut {
                    self.set_grid(*layer, left, &mut patch, &mut applied);
                }
            }
            Op::Paste { above } => {
                let clip = self.clip.clone().ok_or("nothing is copied")?;
                let at = match above {
                    Some(id) => Some(self.place(*id).ok_or_else(|| format!("there is no layer {id}"))? + 1),
                    None => None,
                };
                patch.layers = Some((self.layers.clone(), self.next_id));
                let id = self.push_layer("Pasted".into());
                let mut layer = self.layers.pop().expect("just pushed");
                layer.grid = clip;
                applied.touched = present(&layer);
                let at = at.map_or(self.layers.len(), |a| (a as usize).min(self.layers.len()));
                self.layers.insert(at, layer);
                applied.made = Some(id);
                applied.meta = true;
                applied.changed = true;
            }
            Op::Transform { layer, bounds, inverse, smooth } => {
                if !sample::sane(inverse) {
                    return Err("a transform that large cannot be sampled".into());
                }
                let target = self.unlocked(*layer)?;
                let selecting = self.selecting();
                let (lifted, left) = select::split(&target.grid, &self.selection, selecting);
                let [x, y, w, h] = bounds.map(|v| v as i64);
                let edges = [x, y, x.saturating_add(w), y.saturating_add(h)];
                let moved = sample::transform(&lifted, self.width, self.height, edges, inverse, *smooth);
                self.set_grid(*layer, sample::over(&left, &moved), &mut patch, &mut applied);
                if selecting {
                    let carried = sample::transform(&self.selection, self.width, self.height, edges, inverse, *smooth);
                    self.select(select::compact(carried), &mut patch, &mut applied);
                }
            }
            Op::Generate { layer, generator } => {
                let target = self.unlocked(*layer)?;
                let made = gen::make(self, generator)?;
                let laid = sample::over(&target.grid, &made);
                self.held(*layer, laid, &mut patch, &mut applied);
            }
            Op::Filter { layer, filter } => {
                let target = self.unlocked(*layer)?;
                let (made, kept) = filter::apply(&target.grid, &self.palette, filter)?;
                self.held(*layer, made, &mut patch, &mut applied);
                if let Some(colors) = kept.filter(|c| *c != self.palette) {
                    patch.palette = Some(std::mem::replace(&mut self.palette, colors));
                    applied.meta = true;
                    applied.changed = true;
                }
            }
            Op::Upscale { factor } => {
                if !(2..=3).contains(factor) {
                    return Err("Scale2x and Scale3x grow a canvas 2 or 3 times".into());
                }
                let f = *factor;
                let (w, h) = (self.width * f, self.height * f);
                self.reshape(w, h, |g| filter::upscale(g, f), &mut patch, &mut applied)?;
                // A selection grows square: Scale2x would round its corners.
                let grown = sample::resize(&patch.selection.clone().unwrap_or_else(|| Grid::new(w / f, h / f)), w, h, false);
                self.selection = select::compact(grown);
            }
            Op::Orient { turns, flip } => {
                if turns % 4 != 0 || *flip {
                    let (w, h) = if turns % 2 == 1 { (self.height, self.width) } else { (self.width, self.height) };
                    let (turns, flip) = (*turns, *flip);
                    self.reshape(w, h, |g| sample::orient(g, turns, flip), &mut patch, &mut applied)?;
                }
            }
            Op::Resize { width, height, smooth } => {
                if (*width, *height) != (self.width, self.height) {
                    let (w, h, smooth) = (*width, *height, *smooth);
                    self.reshape(w, h, |g| sample::resize(g, w, h, smooth), &mut patch, &mut applied)?;
                }
            }
            Op::Crop { rect } => {
                let [x, y, w, h] = *rect;
                if (x, y, w, h) != (0, 0, self.width as i32, self.height as i32) {
                    if w <= 0 || h <= 0 {
                        return Err("a canvas needs a width and a height".into());
                    }
                    let r = [x as i64, y as i64, w as i64, h as i64];
                    self.reshape(w as u32, h as u32, |g| sample::crop(g, r), &mut patch, &mut applied)?;
                }
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
                // A swatch recoloured is the palette's colour everywhere; a
                // colour replaced on its own keeps to the selection.
                let sel = (swatch.is_none() && self.selecting()).then(|| self.selection.clone());
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
                        let mut after = (!tile.is_clear()).then(|| Arc::new(tile));
                        if let Some(sel) = &sel {
                            after = select::clip(sel, index, Some(&before), after);
                        }
                        if after.as_ref().is_some_and(|a| Arc::ptr_eq(a, &before)) {
                            continue;
                        }
                        target.grid.put(index, after);
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
        let sel = self.selecting().then(|| self.selection.clone());
        let target = self.layer_mut(layer).expect("found above");
        for index in mask.tiles() {
            let before = target.grid.tile(index).cloned();
            let mut after = stroke::paint(mask, index, before.as_ref(), tool, color, lock, width, height);
            if let Some(sel) = &sel {
                after = select::clip(sel, index, before.as_ref(), after);
            }
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
            h.word(self.grid(layer).map(|g| g.hash(index)).unwrap_or(crate::tile::CLEAR));
        }
        h.0
    }

    /// Put back what a patch holds; the patch that would undo that, and what
    /// it changed.
    pub fn restore(&mut self, patch: Patch) -> (Patch, Applied) {
        let mut inverse = Patch::default();
        let mut applied = Applied { changed: true, ..Applied::default() };
        if let Some((w, h)) = patch.size {
            inverse.size = Some((self.width, self.height));
            (self.width, self.height) = (w, h);
            self.clip = None;
            applied.meta = true;
        }
        if let Some(sel) = patch.selection {
            let old = std::mem::replace(&mut self.selection, sel);
            applied.touched.extend(tiles_between(&old, &self.selection).into_iter().map(|i| (SELECTION, i)));
            inverse.selection = Some(old);
        }
        if let Some((layers, next_id)) = patch.layers {
            let old = std::mem::replace(&mut self.layers, layers);
            applied.touched.extend(differ(&old, &self.layers));
            inverse.layers = Some((old, self.next_id));
            self.next_id = next_id;
            applied.meta = true;
        }
        if let Some(palette) = patch.palette {
            inverse.palette = Some(std::mem::replace(&mut self.palette, palette));
            applied.meta = true;
        }
        for (layer, index, tile) in patch.tiles {
            if let Some(grid) = self.grid_mut(layer) {
                let now = grid.tile(index).cloned();
                grid.put(index, tile);
                inverse.tiles.push((layer, index, now));
                applied.touched.push((layer, index));
            }
        }
        (inverse, applied)
    }
}

impl Canvas {
    /// Layers made from pictures brought in — a name, opacity, whether
    /// hidden, how they blend, and their pixels, bottom to top — put over
    /// layer `above` (on top if none): what changed, and how to take it
    /// back. Not an op: pixels from outside are the core's to bring in, and
    /// the window is sent them.
    pub fn bring_in(&mut self, layers: Vec<(String, u8, bool, Blend, Grid)>, above: Option<u32>) -> Result<(Applied, Patch), String> {
        if layers.is_empty() {
            return Err("there is nothing to bring in".into());
        }
        let mut patch = Patch { layers: Some((self.layers.clone(), self.next_id)), ..Patch::default() };
        let mut applied = Applied { meta: true, changed: true, ..Applied::default() };
        let start = match above {
            Some(id) => (self.place(id).ok_or_else(|| format!("there is no layer {id}"))? + 1) as usize,
            None => self.layers.len(),
        };
        for (n, (name, opacity, hidden, blend, grid)) in layers.into_iter().enumerate() {
            if grid.width != self.width || grid.height != self.height {
                return Err("a layer brought in is not the canvas's size".into());
            }
            let id = self.push_layer(if name.trim().is_empty() { format!("Layer {}", self.next_id) } else { name });
            let mut layer = self.layers.pop().expect("just pushed");
            layer.info.opacity = opacity;
            layer.info.hidden = hidden;
            layer.info.blend = blend;
            layer.grid = grid;
            applied.touched.extend(present(&layer));
            applied.made = Some(id);
            self.layers.insert((start + n).min(self.layers.len()), layer);
        }
        self.advance();
        patch.tiles.clear();
        Ok((applied, patch))
    }

    /// A layer that may be drawn on.
    fn unlocked(&self, id: u32) -> Result<&Layer, String> {
        let target = self.layer(id).ok_or_else(|| format!("there is no layer {id}"))?;
        if target.info.locked {
            return Err(format!("{} is locked", target.info.name));
        }
        Ok(target)
    }

    /// A layer's pixels replaced by `grid`: each tile that differs
    /// remembered and told.
    fn set_grid(&mut self, id: u32, grid: Grid, patch: &mut Patch, applied: &mut Applied) {
        let Some(target) = self.layer_mut(id) else { return };
        for index in tiles_between(&target.grid, &grid) {
            patch.tiles.push((id, index, target.grid.tile(index).cloned()));
            target.grid.put(index, grid.tile(index).cloned());
            applied.touched.push((id, index));
            applied.changed = true;
        }
    }

    /// A layer's pixels replaced by `grid` where the selection lets them be,
    /// and as they were where it does not.
    fn held(&mut self, id: u32, grid: Grid, patch: &mut Patch, applied: &mut Applied) {
        let grid = match (self.selecting(), self.layer(id)) {
            (true, Some(target)) => {
                let mut kept = grid;
                for index in 0..kept.cols * kept.rows {
                    let made = select::clip(&self.selection, index, target.grid.tile(index), kept.tile(index).cloned());
                    kept.put(index, made);
                }
                kept
            }
            _ => grid,
        };
        self.set_grid(id, grid, patch, applied);
    }

    /// A new selection: the old remembered, and the tiles that differ told
    /// as the window's layer 0.
    fn select(&mut self, grid: Grid, patch: &mut Patch, applied: &mut Applied) {
        let changed = tiles_between(&self.selection, &grid);
        if changed.is_empty() {
            return;
        }
        patch.selection.get_or_insert_with(|| self.selection.clone());
        self.selection = grid;
        applied.touched.extend(changed.into_iter().map(|i| (SELECTION, i)));
        applied.changed = true;
    }

    /// The canvas made `width` by `height`, every layer and the selection
    /// by `remake`: all of it remembered, and all of it told.
    fn reshape(&mut self, width: u32, height: u32, remake: impl Fn(&Grid) -> Grid, patch: &mut Patch, applied: &mut Applied) -> Result<(), String> {
        let most = self.preset.most();
        if width == 0 || height == 0 || width > most || height > most {
            return Err(format!("a {} canvas is 1 to {most} pixels a side", crate::canvas::name(self.preset)));
        }
        patch.layers = Some((self.layers.clone(), self.next_id));
        patch.selection = Some(self.selection.clone());
        patch.size = Some((self.width, self.height));
        for layer in &mut self.layers {
            layer.grid = remake(&layer.grid);
        }
        self.selection = select::compact(remake(&self.selection));
        (self.width, self.height) = (width, height);
        self.clip = None;
        applied.touched = self.layers.iter().flat_map(present).collect();
        applied.meta = true;
        applied.changed = true;
        Ok(())
    }
}

/// The tiles two grids of a size hold differently.
pub fn tiles_between(a: &Grid, b: &Grid) -> Vec<u32> {
    let n = (a.cols * a.rows).max(b.cols * b.rows);
    (0..n)
        .filter(|&i| match (a.tile(i), b.tile(i)) {
            (None, None) => false,
            (Some(x), Some(y)) => !Arc::ptr_eq(x, y) && x.px != y.px,
            _ => true,
        })
        .collect()
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
