//! Canvas mode's engine as WebAssembly: the window's own copy of the canvas.
//!
//! The window paints on this copy the moment the pen moves, and lays the
//! layers together here, so every blend mode is the same integer code the
//! core flattens and exports with. It sends each edit to the core, which
//! makes it on the canvas it saves, and checks the hash the core answers
//! with against the one this made. Tiles the core changed itself are fetched
//! and put here. A plain C ABI, as the game's runtime has — no bindings
//! generator, and nothing the window reads that it cannot see:
//!
//! ```text
//! rr_abi() → 1
//! rr_alloc(len) → ptr, rr_free(ptr, len)
//! rr_open(ptr, len) → 0 | -1       a canvas: size, preset, palette, layers, generation (JSON)
//! rr_meta(ptr, len) → 0 | -1       its layers or palette changed (JSON)
//! rr_put(layer, ptr, len) → 0 | -1 a read_raster payload: tiles of one layer
//! rr_apply(ptr, len) → 0 | -1      an op, made here first (JSON); its hash kept
//! rr_hash_lo(), rr_hash_hi()       the hash of the tiles the last op touched
//! rr_dirty() → ptr                 what was laid together again since the last
//!                                  call — whole tiles, or the parts of them a
//!                                  brush changed: [count, (x, y, width,
//!                                  height)…] as u32s
//! rr_dirty_some(budget) → ptr      as rr_dirty, laying no more than `budget`
//!                                  pixel-layers; the rest waits for the next
//! rr_pending() → u32               how many parts wait
//! rr_composite() → ptr             the layers laid together: premultiplied RGBA,
//!                                  row by row, the canvas's size
//! rr_pick(x, y) → u32              a pixel of that, straight RGBA, little-endian
//! rr_layer_pixel(layer, x, y) → u32 a pixel of one layer
//! rr_stroke_begin(ptr, len) → 0|-1 a pencil or eraser begun (JSON: layer, tool,
//!                                  tip, color, pixelPerfect) — or a brush
//!                                  (JSON: layer, tool, brush, color)
//! rr_stroke_add(ptr, n) → 0|-1      n numbers: x, y (1/256 px), pressure, …
//! rr_stroke_end() → ptr            the stroke as an op (JSON: length, then
//!                                  bytes); its hash kept, as rr_apply's
//! rr_stroke_cancel()               the stroke taken away, nothing sent
//! rr_preview(ptr, len) → 0|-1      an op shown and not kept: a shape being
//!                                  dragged, a generator being tuned
//! rr_preview_clear()               what was previewed taken away
//! rr_touched() → ptr               the tiles the last op touched:
//!                                  [count, (layer, index)…] as u32s
//! rr_info() → ptr                  the size, layers, palette and which tiles each
//!                                  layer has (the selection as layer 0), as
//!                                  JSON: length, then bytes
//! rr_width(), rr_height() → u32    the canvas's size now
//! rr_ramp(ptr, len) → ptr          a colour's ramp (JSON: base, steps, shift,
//!                                  spread): its colours as JSON, length first
//! rr_selection_version() → u32     a number that changes when the selection does
//! rr_outline() → ptr               the selection's edges, for marching ants:
//!                                  [count, (x0, y0, x1, y1)…] as i32s
//! rr_lift(layer) → ptr             what of a layer is selected — all of it with
//!                                  nothing selected — for a transform's preview:
//!                                  [x, y, width, height] as u32s, then that
//!                                  rectangle premultiplied, RGBA; width 0 if
//!                                  there is nothing
//! rr_tile(layer, index) → ptr | 0   a tile's RGBA (0: clear) — for a second
//!                                  copy standing in for the core, in the
//!                                  browser preview and the window's tests
//! rr_error() → ptr                 why the last call failed: length, then bytes
//! ```

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

use std::collections::BTreeMap;

use proton_raster::history::Patch;
use proton_raster::tile::{Tile, TILE, TILE_BYTES};
use proton_raster::{blend, brush, select, stroke, Brush, Canvas, Dabs, Grid, Layer, LayerInfo, Op, Preset, Stroke, Tip, Tool, SELECTION};
use serde::Deserialize;

pub const ABI: u32 = 1;

/// What a stroke being drawn covers: a pencil's pixels, or a brush's dabs.
enum Marks {
    Pencil(Stroke),
    Brush(Dabs),
}

/// A stroke as it is drawn: the tiles as they were under it, and what it
/// covers so far.
struct Live {
    layer: u32,
    tool: Tool,
    color: [u8; 4],
    lock: bool,
    marks: Marks,
    bases: BTreeMap<u32, Option<Arc<Tile>>>,
    /// The selection the stroke keeps to, if anything is selected.
    sel: Option<Grid>,
}

impl Live {
    /// Tile `index` as the stroke so far leaves it, from what was under it —
    /// or, given the tile as it is and a part of it, only that part made
    /// again, for a brush.
    fn paint(&self, index: u32, base: Option<&Arc<Tile>>, part: Option<(&Option<Arc<Tile>>, [u32; 4])>, width: u32, height: u32) -> Option<Arc<Tile>> {
        let painted = match &self.marks {
            Marks::Pencil(s) => stroke::paint(&s.mask, index, base, self.tool, self.color, self.lock, width, height),
            Marks::Brush(d) => brush::paint(&d.mask, index, base, part, self.tool == Tool::Eraser, self.color, self.lock, width, height),
        };
        match &self.sel {
            Some(sel) => select::clip(sel, index, base, painted),
            None => painted,
        }
    }
}

/// What the layers under a stroke's layer lay together to, tile by tile,
/// while the stroke is drawn. They do not change under it, so a frame lays
/// together only the stroke's layer and those over it, from this: painting
/// on the top layer of sixteen costs one layer a frame, not sixteen. Kept as
/// laid together, at 16 bits a channel; at most [`BELOW_MOST`] tiles, the
/// oldest let go first.
struct Below {
    /// The stroke's layer's place, from the bottom: the layers under it.
    place: u32,
    tiles: BTreeMap<u32, Vec<u16>>,
    order: VecDeque<u32>,
}

/// 256 tiles of 64 × 64 at 16 bits a channel: 8 MB.
const BELOW_MOST: usize = 256;

/// A whole tile, as a part of it.
const WHOLE: [u32; 4] = [0, 0, TILE, TILE];

/// Part of tile `index` to lay together again, joined to any part already
/// waiting.
fn mark(dirty: &mut BTreeMap<u32, [u32; 4]>, index: u32, part: [u32; 4]) {
    let r = dirty.entry(index).or_insert(part);
    *r = [r[0].min(part[0]), r[1].min(part[1]), r[2].max(part[2]), r[3].max(part[3])];
}

#[derive(Default)]
struct Held {
    canvas: Option<Canvas>,
    generation: u32,
    /// Premultiplied RGBA, the canvas's size.
    composite: Vec<u8>,
    /// Tiles to lay together again, and which part of each.
    dirty: BTreeMap<u32, [u32; 4]>,
    /// The rectangles the last `rr_dirty` laid together, as u32s.
    rects: Vec<u32>,
    /// What `rr_info` last said: length, then JSON.
    info: Vec<u8>,
    /// A stroke being drawn.
    live: Option<Live>,
    /// What is under it, laid together.
    below: Option<Below>,
    /// What a preview changed, to put back.
    preview: Option<Patch>,
    /// The tiles the last op touched, as u32s: count, then layer and index.
    touched: Vec<u32>,
    /// What `rr_stroke_end` gave: length, then the op as JSON.
    ended: Vec<u8>,
    /// Bumped whenever the selection may have changed.
    selection_version: u32,
    /// What `rr_outline` gave.
    outline: Vec<i32>,
    /// What `rr_lift` gave.
    lifted: Vec<u8>,
    /// What `rr_ramp` gave: length, then JSON.
    ramp: Vec<u8>,
    hash: u64,
    error: Vec<u8>,
}

thread_local! {
    static HELD: RefCell<Held> = RefCell::new(Held::default());
}

fn failed(why: String) -> i32 {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let bytes = why.into_bytes();
        h.error = (bytes.len() as u32).to_le_bytes().to_vec();
        h.error.extend(bytes);
    });
    -1
}

fn json<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        return &[];
    }
    // SAFETY: the window passes what `rr_alloc` gave it, `len` long.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

#[no_mangle]
pub extern "C" fn rr_abi() -> u32 {
    ABI
}

/// Room for `len` bytes, for the window to write into.
#[no_mangle]
pub extern "C" fn rr_alloc(len: usize) -> *mut u8 {
    let mut room = Vec::<u8>::with_capacity(len.max(1));
    let ptr = room.as_mut_ptr();
    std::mem::forget(room);
    ptr
}

/// Give back what `rr_alloc` gave.
///
/// # Safety
/// `ptr` and `len` must be what `rr_alloc` returned and was asked for.
#[no_mangle]
pub unsafe extern "C" fn rr_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
    }
}

#[no_mangle]
pub extern "C" fn rr_error() -> *const u8 {
    HELD.with(|h| h.borrow().error.as_ptr())
}

/// A canvas to mirror, as the core sends it: no pixels yet.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Opening {
    pub width: u32,
    pub height: u32,
    pub preset: Preset,
    #[serde(default)]
    pub palette: Vec<[u8; 4]>,
    pub layers: Vec<LayerInfo>,
    #[serde(default)]
    pub generation: u32,
}

/// Open a canvas — the safe half of `rr_open`.
pub fn open(bytes: &[u8]) -> Result<(), String> {
    let o: Opening = serde_json::from_slice(bytes).map_err(|e| format!("not a canvas: {e}"))?;
    let mut canvas = Canvas::new(o.width, o.height, o.preset)?;
    canvas.palette = o.palette;
    canvas.layers = o.layers.into_iter().map(|info| Layer { info, grid: Grid::new(o.width, o.height) }).collect();
    canvas.next_id = canvas.layers.iter().map(|l| l.info.id).max().unwrap_or(0) + 1;
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        h.composite = vec![0; (o.width * o.height * 4) as usize];
        h.dirty = (0..tiles_of(&canvas)).map(|i| (i, WHOLE)).collect();
        h.generation = o.generation;
        h.live = None;
        h.below = None;
        h.preview = None;
        h.selection_version += 1;
        h.canvas = Some(canvas);
    });
    Ok(())
}

fn tiles_of(canvas: &Canvas) -> u32 {
    let g = Grid::new(canvas.width, canvas.height);
    g.cols * g.rows
}

#[no_mangle]
pub extern "C" fn rr_open(ptr: *const u8, len: usize) -> i32 {
    match open(json(ptr, len)) {
        Ok(()) => 0,
        Err(why) => failed(why),
    }
}

#[derive(Deserialize)]
struct Meta {
    layers: Vec<LayerInfo>,
    #[serde(default)]
    palette: Option<Vec<[u8; 4]>>,
}

/// The layers or palette changed: kept layers keep their pixels, new ones
/// start clear — the safe half of `rr_meta`.
pub fn meta(bytes: &[u8]) -> Result<(), String> {
    let m: Meta = serde_json::from_slice(bytes).map_err(|e| format!("not a layer list: {e}"))?;
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let canvas = held.canvas.as_mut().ok_or("no canvas is open")?;
        let before = canvas.layers.clone();
        // What is under a stroke may now be other layers, or seen otherwise.
        held.below = None;
        let mut old = std::mem::take(&mut canvas.layers);
        canvas.layers = m
            .layers
            .into_iter()
            .map(|info| match old.iter().position(|l| l.info.id == info.id) {
                Some(i) => Layer { info, grid: old.swap_remove(i).grid },
                None => Layer { info, grid: Grid::new(canvas.width, canvas.height) },
            })
            .collect();
        canvas.next_id = canvas.layers.iter().map(|l| l.info.id).max().unwrap_or(0) + 1;
        if let Some(palette) = m.palette {
            canvas.palette = palette;
        }
        // Laid together again only where what is seen changed: a layer
        // renamed changes nothing on the canvas.
        for (_, index) in proton_raster::op::differ(&before, &canvas.layers) {
            mark(&mut held.dirty, index, WHOLE);
        }
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn rr_meta(ptr: *const u8, len: usize) -> i32 {
    match meta(json(ptr, len)) {
        Ok(()) => 0,
        Err(why) => failed(why),
    }
}

/// Tiles of one layer, from `read_raster` — the safe half of `rr_put`.
pub fn put(layer: u32, bytes: &[u8]) -> Result<(), String> {
    let word = |at: usize| -> Result<u32, String> {
        bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| "the tiles end too soon".to_string())
    };
    if word(0)? != ABI {
        return Err("tiles in a layout this window does not read".into());
    }
    let (generation, count) = (word(4)?, word(20)?);
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        if generation != held.generation {
            return Err("tiles of a canvas since closed".into());
        }
        let canvas = held.canvas.as_mut().ok_or("no canvas is open")?;
        if layer == SELECTION {
            held.selection_version += 1;
        }
        let (width, height) = (canvas.width, canvas.height);
        let target = canvas.grid_mut(layer).ok_or_else(|| format!("there is no layer {layer}"))?;
        let mut at = 24usize;
        for _ in 0..count {
            let (index, present) = (word(at)?, word(at + 4)?);
            at += 8;
            let tile = if present == 1 {
                let px = bytes.get(at..at + TILE_BYTES as usize).ok_or("a tile ends too soon")?.to_vec();
                at += TILE_BYTES as usize;
                Some(Arc::new(Tile { px }))
            } else {
                None
            };
            if index >= target.cols * target.rows {
                continue;
            }
            if layer == SELECTION {
                target.put(index, tile);
                continue;
            }
            mark(&mut held.dirty, index, WHOLE);
            if let Some(below) = held.below.as_mut() {
                below.tiles.remove(&index);
            }
            // Under a stroke being drawn, the tile fetched is what the stroke
            // is over: the picture is made again from it and the stroke.
            if let Some(live) = held.live.as_mut().filter(|l| l.layer == layer) {
                if let Some(base) = live.bases.get_mut(&index) {
                    *base = tile.clone();
                    let painted = live.paint(index, tile.as_ref(), None, width, height);
                    target.put(index, painted);
                    continue;
                }
            }
            target.put(index, tile);
        }
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn rr_put(layer: u32, ptr: *const u8, len: usize) -> i32 {
    match put(layer, json(ptr, len)) {
        Ok(()) => 0,
        Err(why) => failed(why),
    }
}

/// An op, made here first; the hash of what it touched, which the core's
/// answer must match — the safe half of `rr_apply`.
pub fn apply(bytes: &[u8]) -> Result<u64, String> {
    let op: Op = serde_json::from_slice(bytes).map_err(|e| format!("not an edit: {e}"))?;
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let canvas = held.canvas.as_mut().ok_or("no canvas is open")?;
        let size = (canvas.width, canvas.height);
        let (applied, _) = canvas.apply(&op)?;
        held.below = None;
        held.hash = canvas.hash_touched(&applied.touched);
        if (canvas.width, canvas.height) != size {
            // Turned, resized or cropped: everything laid together again.
            held.composite = vec![0; (canvas.width * canvas.height * 4) as usize];
            held.dirty = (0..tiles_of(canvas)).map(|i| (i, WHOLE)).collect();
            held.live = None;
            held.preview = None;
            held.selection_version += 1;
        }
        for &(layer, index) in &applied.touched {
            if layer == SELECTION {
                held.selection_version += 1;
            } else {
                mark(&mut held.dirty, index, WHOLE);
            }
        }
        held.touched = flat(&applied.touched);
        Ok(held.hash)
    })
}

#[no_mangle]
pub extern "C" fn rr_apply(ptr: *const u8, len: usize) -> i32 {
    match apply(json(ptr, len)) {
        Ok(_) => 0,
        Err(why) => failed(why),
    }
}

#[no_mangle]
pub extern "C" fn rr_hash_lo() -> u32 {
    HELD.with(|h| h.borrow().hash as u32)
}

#[no_mangle]
pub extern "C" fn rr_hash_hi() -> u32 {
    HELD.with(|h| (h.borrow().hash >> 32) as u32)
}

/// Lay the dirty tiles together into the composite; their rectangles.
pub fn dirty() -> Vec<u32> {
    dirty_some(u64::MAX)
}

/// Lay together what is dirty, as much as `budget` allows — counted in
/// pixels laid, one for each layer a pixel is laid through — and leave the
/// rest for the next call; the rectangles laid. At least one part is always
/// laid. A frame given a budget is never long, whatever the canvas: a stroke
/// under fifteen layers has its ink catch up a frame later rather than drop
/// one, and a large canvas opened shows tile by tile instead of all at once
/// after a wait.
pub fn dirty_some(budget: u64) -> Vec<u32> {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let Some(canvas) = held.canvas.as_ref() else { return vec![0] };
        let grid = Grid::new(canvas.width, canvas.height);
        let layers = canvas.layers.len() as u32;
        let mut rects = vec![0u32];
        let mut spent = 0u64;
        while let Some((index, part)) = held.dirty.pop_first() {
            let (x, y, tw, th) = grid.bounds(index);
            let part = [part[0], part[1], part[2].min(tw), part[3].min(th)];
            if part[0] >= part[2] || part[1] >= part[3] {
                continue;
            }
            let (w, h) = (part[2] - part[0], part[3] - part[1]);
            // What this part costs: its pixels through every layer with any
            // there — or, under a stroke, through the stroke's layer and
            // those over it, and the layers under it once for the whole tile.
            let present = canvas.layers.iter().filter(|l| !l.info.hidden && l.grid.tile(index).is_some()).count() as u64;
            let cost = match held.below.as_ref() {
                Some(b) if b.tiles.contains_key(&index) => (w * h) as u64 * (layers - b.place) as u64,
                Some(b) => (TILE * TILE) as u64 * b.place as u64 + (w * h) as u64 * (layers - b.place) as u64,
                None => (w * h) as u64 * present.max(1),
            };
            if spent > 0 && spent.saturating_add(cost) > budget {
                held.dirty.insert(index, part);
                break;
            }
            spent += cost;
            let acc = match held.below.as_mut() {
                Some(below) => {
                    if !below.tiles.contains_key(&index) {
                        let under = blend::fold(canvas, index, WHOLE, 0, below.place, vec![0; (TILE * TILE * 4) as usize]);
                        below.tiles.insert(index, under.into_iter().map(|v| v as u16).collect());
                        below.order.push_back(index);
                        while below.order.len() > BELOW_MOST {
                            if let Some(gone) = below.order.pop_front() {
                                below.tiles.remove(&gone);
                            }
                        }
                    }
                    let under = &below.tiles[&index];
                    let mut acc = Vec::with_capacity((w * h * 4) as usize);
                    for row in 0..h {
                        let from = Tile::at(part[0], part[1] + row);
                        acc.extend(under[from..from + (w * 4) as usize].iter().map(|&v| v as u64));
                    }
                    blend::fold(canvas, index, part, below.place, layers, acc)
                }
                None => blend::region(canvas, index, part),
            };
            let px = blend::premultiplied(&acc);
            for row in 0..h {
                let from = (row * w * 4) as usize;
                let to = (((y + part[1] + row) * canvas.width + x + part[0]) * 4) as usize;
                held.composite[to..to + (w * 4) as usize].copy_from_slice(&px[from..from + (w * 4) as usize]);
            }
            rects.extend([x + part[0], y + part[1], w, h]);
            rects[0] += 1;
        }
        held.rects = rects.clone();
        rects
    })
}

/// How many parts of tiles wait to be laid together.
pub fn pending() -> u32 {
    HELD.with(|h| h.borrow().dirty.len() as u32)
}

#[no_mangle]
pub extern "C" fn rr_dirty_some(budget: u32) -> *const u32 {
    dirty_some(budget as u64);
    HELD.with(|h| h.borrow().rects.as_ptr())
}

#[no_mangle]
pub extern "C" fn rr_pending() -> u32 {
    pending()
}

#[no_mangle]
pub extern "C" fn rr_dirty() -> *const u32 {
    dirty();
    HELD.with(|h| h.borrow().rects.as_ptr())
}

/// The composite as it stands.
pub fn composite() -> Vec<u8> {
    HELD.with(|h| h.borrow().composite.clone())
}

#[no_mangle]
pub extern "C" fn rr_composite() -> *const u8 {
    HELD.with(|h| h.borrow().composite.as_ptr())
}

/// A pixel of everything laid together, straight: the eyedropper's colour.
pub fn pick(x: u32, y: u32) -> u32 {
    HELD.with(|h| {
        let h = h.borrow();
        let Some(canvas) = h.canvas.as_ref() else { return 0 };
        if x >= canvas.width || y >= canvas.height {
            return 0;
        }
        let grid = Grid::new(canvas.width, canvas.height);
        let (index, tx, ty) = grid.locate(x, y);
        let acc = blend::tile(canvas, index);
        let i = Tile::at(tx, ty);
        let px = blend::straight(&acc[i..i + 4]);
        u32::from_le_bytes([px[0], px[1], px[2], px[3]])
    })
}

#[no_mangle]
pub extern "C" fn rr_pick(x: u32, y: u32) -> u32 {
    pick(x, y)
}

#[no_mangle]
pub extern "C" fn rr_layer_pixel(layer: u32, x: u32, y: u32) -> u32 {
    HELD.with(|h| {
        let h = h.borrow();
        h.canvas.as_ref().and_then(|c| c.grid(layer)).map(|g| u32::from_le_bytes(g.pixel(x, y))).unwrap_or(0)
    })
}

#[no_mangle]
pub extern "C" fn rr_tile(layer: u32, index: u32) -> *const u8 {
    HELD.with(|h| {
        let h = h.borrow();
        h.canvas.as_ref().and_then(|c| c.grid(layer)).and_then(|g| g.tile(index)).map(|t| t.px.as_ptr()).unwrap_or(std::ptr::null())
    })
}

/// The layers, the palette and the tiles each layer has.
pub fn info() -> serde_json::Value {
    HELD.with(|h| {
        let h = h.borrow();
        let Some(canvas) = h.canvas.as_ref() else { return serde_json::Value::Null };
        let tiles: Vec<(u32, Vec<u32>)> = canvas
            .selecting()
            .then_some((SELECTION, &canvas.selection))
            .into_iter()
            .chain(canvas.layers.iter().map(|l| (l.info.id, &l.grid)))
            .map(|(id, g)| (id, (0..g.cols * g.rows).filter(|&i| g.tile(i).is_some()).collect()))
            .collect();
        serde_json::json!({ "width": canvas.width, "height": canvas.height, "layers": canvas.infos(), "palette": canvas.palette, "tiles": tiles })
    })
}

#[no_mangle]
pub extern "C" fn rr_info() -> *const u8 {
    let text = info().to_string().into_bytes();
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        h.info = (text.len() as u32).to_le_bytes().to_vec();
        h.info.extend(text);
        h.info.as_ptr()
    })
}

fn flat(touched: &[(u32, u32)]) -> Vec<u32> {
    let mut out = vec![touched.len() as u32];
    for &(layer, index) in touched {
        out.extend([layer, index]);
    }
    out
}

#[no_mangle]
pub extern "C" fn rr_touched() -> *const u32 {
    HELD.with(|h| h.borrow().touched.as_ptr())
}

/// A stroke begun, as the window starts one: a pencil's or an eraser's tip,
/// or a brush — an eraser with a brush erases softly.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Begin {
    pub layer: u32,
    pub tool: Tool,
    #[serde(default)]
    pub tip: Option<Tip>,
    #[serde(default)]
    pub brush: Option<Brush>,
    pub color: [u8; 4],
    #[serde(default)]
    pub pixel_perfect: bool,
}

/// Begin a stroke — the safe half of `rr_stroke_begin`. One being drawn is
/// taken away first.
pub fn stroke_begin(bytes: &[u8]) -> Result<(), String> {
    let b: Begin = serde_json::from_slice(bytes).map_err(|e| format!("not a stroke: {e}"))?;
    stroke_cancel();
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let canvas = held.canvas.as_ref().ok_or("no canvas is open")?;
        let layer = canvas.layer(b.layer).ok_or_else(|| format!("there is no layer {}", b.layer))?;
        if layer.info.locked {
            return Err(format!("{} is locked", layer.info.name));
        }
        let marks = match (b.brush, b.tip) {
            (Some(brush), _) => {
                brush.check()?;
                Marks::Brush(Dabs::new(brush, canvas.width, canvas.height))
            }
            (None, Some(tip)) => {
                if !(1..=Tip::MOST).contains(&tip.size) {
                    return Err(format!("a tip is 1 to {} pixels across", Tip::MOST));
                }
                Marks::Pencil(Stroke::new(tip, b.pixel_perfect, canvas.width, canvas.height))
            }
            (None, None) => return Err("a stroke needs a tip or a brush".into()),
        };
        let sel = canvas.selecting().then(|| canvas.selection.clone());
        held.live = Some(Live { layer: b.layer, tool: b.tool, color: b.color, lock: layer.info.alpha_lock, marks, bases: BTreeMap::new(), sel });
        // Worth keeping only with layers under it.
        held.below = canvas.place(b.layer).filter(|&place| place > 0).map(|place| Below { place, tiles: BTreeMap::new(), order: VecDeque::new() });
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn rr_stroke_begin(ptr: *const u8, len: usize) -> i32 {
    match stroke_begin(json(ptr, len)) {
        Ok(()) => 0,
        Err(why) => failed(why),
    }
}

/// More of the stroke: numbers in threes — the safe half of `rr_stroke_add`.
pub fn stroke_add(points: &[i32]) -> Result<(), String> {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let live = held.live.as_mut().ok_or("no stroke is being drawn")?;
        let canvas = held.canvas.as_mut().ok_or("no canvas is open")?;
        // A pencil's tiles are made again whole — a corner taken back can be
        // anywhere in one; a brush's only where its dabs fell.
        let changed: Vec<(u32, Option<[u32; 4]>)> = match &mut live.marks {
            Marks::Pencil(s) => s.add(points).into_iter().map(|i| (i, None)).collect(),
            Marks::Brush(d) => {
                d.add(points);
                d.take_rects().into_iter().map(|(i, r)| (i, Some(r))).collect()
            }
        };
        let target = canvas.layer_mut(live.layer).ok_or("the stroke's layer has gone")?;
        let (width, height) = (target.grid.width, target.grid.height);
        for (index, part) in changed {
            let base = live.bases.entry(index).or_insert_with(|| target.grid.tile(index).cloned()).clone();
            let now = target.grid.tile(index).cloned();
            let painted = live.paint(index, base.as_ref(), part.map(|p| (&now, p)), width, height);
            target.grid.put(index, painted);
            mark(&mut held.dirty, index, part.unwrap_or(WHOLE));
        }
        Ok(())
    })
}

/// More of the stroke: `n` numbers at `ptr`.
///
/// # Safety
/// `ptr` is where the window wrote `n` numbers, in memory `rr_alloc` gave.
#[no_mangle]
pub unsafe extern "C" fn rr_stroke_add(ptr: *const i32, n: usize) -> i32 {
    let points = if ptr.is_null() || n == 0 {
        &[][..]
    } else {
        // SAFETY: as the caller promises.
        unsafe { std::slice::from_raw_parts(ptr, n) }
    };
    match stroke_add(points) {
        Ok(()) => 0,
        Err(why) => failed(why),
    }
}

/// The stroke finished: it stays on the canvas as drawn, and is given back
/// as the op the core is to make — the safe half of `rr_stroke_end`.
pub fn stroke_end() -> Result<Op, String> {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let live = held.live.take().ok_or("no stroke is being drawn")?;
        held.below = None;
        let canvas = held.canvas.as_ref().ok_or("no canvas is open")?;
        let grid = &canvas.layer(live.layer).ok_or("the stroke's layer has gone")?.grid;
        // Touched as the core will count it: a tile whose pixels differ from
        // what was under the stroke.
        let touched: Vec<(u32, u32)> = live.bases.iter().filter(|(&i, base)| grid.tile(i).cloned() != **base).map(|(&i, _)| (live.layer, i)).collect();
        held.hash = canvas.hash_touched(&touched);
        held.touched = flat(&touched);
        Ok(match live.marks {
            Marks::Pencil(s) => Op::Stroke { layer: live.layer, tool: live.tool, tip: s.tip, color: live.color, pixel_perfect: s.pixel_perfect, points: s.points },
            Marks::Brush(d) => Op::Brush { layer: live.layer, erase: live.tool == Tool::Eraser, brush: d.brush, color: live.color, points: d.points },
        })
    })
}

#[no_mangle]
pub extern "C" fn rr_stroke_end() -> *const u8 {
    let text = match stroke_end() {
        Ok(op) => serde_json::to_vec(&op).unwrap_or_default(),
        Err(why) => {
            failed(why);
            Vec::new()
        }
    };
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        h.ended = (text.len() as u32).to_le_bytes().to_vec();
        h.ended.extend(text);
        h.ended.as_ptr()
    })
}

/// The stroke taken away: the tiles as they were.
pub fn stroke_cancel() {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let Some(live) = held.live.take() else { return };
        held.below = None;
        let Some(target) = held.canvas.as_mut().and_then(|c| c.layer_mut(live.layer)) else { return };
        for (index, base) in live.bases {
            target.grid.put(index, base);
            mark(&mut held.dirty, index, WHOLE);
        }
    })
}

#[no_mangle]
pub extern "C" fn rr_stroke_cancel() {
    stroke_cancel();
}

/// An op shown, not kept: the last one shown is taken away first — the
/// safe half of `rr_preview`.
pub fn preview(bytes: &[u8]) -> Result<(), String> {
    let op: Op = serde_json::from_slice(bytes).map_err(|e| format!("not an edit: {e}"))?;
    preview_clear();
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let canvas = held.canvas.as_mut().ok_or("no canvas is open")?;
        if matches!(op, Op::Orient { .. } | Op::Resize { .. } | Op::Crop { .. } | Op::Upscale { .. }) {
            return Err("a change to the canvas's size is not previewed".into());
        }
        let (state, made, clip) = (canvas.state, canvas.made, canvas.clip.clone());
        held.below = None;
        let (applied, patch) = canvas.apply(&op)?;
        (canvas.state, canvas.made, canvas.clip) = (state, made, clip);
        for &(layer, index) in &applied.touched {
            if layer == SELECTION {
                held.selection_version += 1;
            } else {
                mark(&mut held.dirty, index, WHOLE);
            }
        }
        held.preview = Some(patch);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn rr_preview(ptr: *const u8, len: usize) -> i32 {
    match preview(json(ptr, len)) {
        Ok(()) => 0,
        Err(why) => failed(why),
    }
}

/// What was previewed taken away.
pub fn preview_clear() {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let (Some(patch), Some(canvas)) = (held.preview.take(), held.canvas.as_mut()) else { return };
        held.below = None;
        let (_, applied) = canvas.restore(patch);
        for &(layer, index) in &applied.touched {
            if layer == SELECTION {
                held.selection_version += 1;
            } else {
                mark(&mut held.dirty, index, WHOLE);
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn rr_preview_clear() {
    preview_clear();
}

/// The canvas's size now: a turn or a crop changes it.
#[no_mangle]
pub extern "C" fn rr_width() -> u32 {
    HELD.with(|h| h.borrow().canvas.as_ref().map_or(0, |c| c.width))
}

#[no_mangle]
pub extern "C" fn rr_height() -> u32 {
    HELD.with(|h| h.borrow().canvas.as_ref().map_or(0, |c| c.height))
}

#[no_mangle]
pub extern "C" fn rr_selection_version() -> u32 {
    HELD.with(|h| h.borrow().selection_version)
}

/// The selection's edges, for marching ants: x0, y0, x1, y1 a segment.
pub fn outline() -> Vec<i32> {
    HELD.with(|h| h.borrow().canvas.as_ref().map(|c| select::outline(&c.selection)).unwrap_or_default())
}

#[no_mangle]
pub extern "C" fn rr_outline() -> *const i32 {
    let segments = outline();
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        h.outline = vec![(segments.len() / 4) as i32];
        h.outline.extend(segments);
        h.outline.as_ptr()
    })
}

/// What of a layer is selected — all of it, with nothing selected — as the
/// window shows it moving: where, and its pixels premultiplied.
pub fn lift(layer: u32) -> Vec<u8> {
    HELD.with(|h| {
        let h = h.borrow();
        let mut out = vec![0u8; 16];
        let Some(canvas) = h.canvas.as_ref() else { return out };
        let Some(target) = canvas.layer(layer) else { return out };
        let (lifted, _) = select::split(&target.grid, &canvas.selection, canvas.selecting());
        let Some([x, y, w, ht]) = select::bounds(&lifted) else { return out };
        for (i, v) in [x, y, w, ht].into_iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out.resize(16 + (w * ht * 4) as usize, 0);
        for index in lifted.covering(x as i64, y as i64, w as i64, ht as i64) {
            let Some(t) = lifted.tile(index) else { continue };
            let (tx, ty, tw, th) = lifted.bounds(index);
            for row in ty.max(y)..(ty + th).min(y + ht) {
                for col in tx.max(x)..(tx + tw).min(x + w) {
                    let p = t.get(col - tx, row - ty);
                    let at = 16 + (((row - y) * w + (col - x)) * 4) as usize;
                    let a = p[3] as u32;
                    out[at..at + 4].copy_from_slice(&[proton_raster::fixed::mul255(p[0] as u32, a) as u8, proton_raster::fixed::mul255(p[1] as u32, a) as u8, proton_raster::fixed::mul255(p[2] as u32, a) as u8, p[3]]);
                }
            }
        }
        out
    })
}

#[no_mangle]
pub extern "C" fn rr_lift(layer: u32) -> *const u8 {
    let bytes = lift(layer);
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        h.lifted = bytes;
        h.lifted.as_ptr()
    })
}

/// A ramp asked for: a colour, how many steps, how far the hue turns
/// (degrees) and how far the lightness reaches each way (1/65536).
#[derive(Deserialize)]
pub struct Ramp {
    pub base: [u8; 4],
    pub steps: u32,
    #[serde(default)]
    pub shift: u32,
    pub spread: u32,
}

/// A colour's ramp, darkest first — the safe half of `rr_ramp`.
pub fn ramp(bytes: &[u8]) -> Result<Vec<[u8; 4]>, String> {
    let r: Ramp = serde_json::from_slice(bytes).map_err(|e| format!("not a ramp: {e}"))?;
    Ok(proton_raster::gen::color::ramp(r.base, r.steps, r.shift, r.spread))
}

#[no_mangle]
pub extern "C" fn rr_ramp(ptr: *const u8, len: usize) -> *const u8 {
    let text = match ramp(json(ptr, len)) {
        Ok(colors) => serde_json::to_vec(&colors).unwrap_or_default(),
        Err(why) => {
            failed(why);
            Vec::new()
        }
    };
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        h.ramp = (text.len() as u32).to_le_bytes().to_vec();
        h.ramp.extend(text);
        h.ramp.as_ptr()
    })
}

/// The whole canvas as one number, for tests.
pub fn canvas_hash() -> u64 {
    HELD.with(|h| h.borrow().canvas.as_ref().map(Canvas::hash).unwrap_or(0))
}

/// The hash the last op or stroke made, for tests.
pub fn last_hash() -> u64 {
    HELD.with(|h| h.borrow().hash)
}
