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
//! rr_dirty() → ptr                 tiles laid together again since the last call:
//!                                  [count, (x, y, width, height)…] as u32s
//! rr_composite() → ptr             the layers laid together: premultiplied RGBA,
//!                                  row by row, the canvas's size
//! rr_pick(x, y) → u32              a pixel of that, straight RGBA, little-endian
//! rr_layer_pixel(layer, x, y) → u32 a pixel of one layer
//! rr_info() → ptr                  the layers, palette and which tiles each layer
//!                                  has, as JSON: length, then bytes
//! rr_tile(layer, index) → ptr | 0   a tile's RGBA (0: clear) — for a second
//!                                  copy standing in for the core, in the
//!                                  browser preview and the window's tests
//! rr_error() → ptr                 why the last call failed: length, then bytes
//! ```

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use proton_raster::tile::{Tile, TILE_BYTES};
use proton_raster::{blend, Canvas, Grid, Layer, LayerInfo, Op, Preset};
use serde::Deserialize;

pub const ABI: u32 = 1;

#[derive(Default)]
struct Held {
    canvas: Option<Canvas>,
    generation: u32,
    /// Premultiplied RGBA, the canvas's size.
    composite: Vec<u8>,
    /// Tiles to lay together again.
    dirty: BTreeSet<u32>,
    /// The rectangles the last `rr_dirty` laid together, as u32s.
    rects: Vec<u32>,
    /// What `rr_info` last said: length, then JSON.
    info: Vec<u8>,
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
        let all: BTreeSet<u32> = (0..tiles_of(&canvas)).collect();
        h.composite = vec![0; (o.width * o.height * 4) as usize];
        h.dirty = all;
        h.generation = o.generation;
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
        held.dirty.extend(proton_raster::op::differ(&before, &canvas.layers).into_iter().map(|(_, i)| i));
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
        let target = canvas.layer_mut(layer).ok_or_else(|| format!("there is no layer {layer}"))?;
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
            if index < target.grid.cols * target.grid.rows {
                target.grid.put(index, tile);
                held.dirty.insert(index);
            }
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
        let (applied, _) = canvas.apply(&op)?;
        held.hash = canvas.hash_touched(&applied.touched);
        held.dirty.extend(applied.touched.iter().map(|(_, i)| *i));
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
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = &mut *h;
        let Some(canvas) = held.canvas.as_ref() else { return vec![0] };
        let grid = Grid::new(canvas.width, canvas.height);
        let mut rects = vec![0u32];
        for index in std::mem::take(&mut held.dirty) {
            let (x, y, w, th) = grid.bounds(index);
            let px = blend::premultiplied(&blend::tile(canvas, index));
            for row in 0..th {
                let from = Tile::at(0, row);
                let to = (((y + row) * canvas.width + x) * 4) as usize;
                held.composite[to..to + (w * 4) as usize].copy_from_slice(&px[from..from + (w * 4) as usize]);
            }
            rects.extend([x, y, w, th]);
            rects[0] += 1;
        }
        held.rects = rects.clone();
        rects
    })
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
        h.canvas.as_ref().and_then(|c| c.layer(layer)).map(|l| u32::from_le_bytes(l.grid.pixel(x, y))).unwrap_or(0)
    })
}

#[no_mangle]
pub extern "C" fn rr_tile(layer: u32, index: u32) -> *const u8 {
    HELD.with(|h| {
        let h = h.borrow();
        h.canvas.as_ref().and_then(|c| c.layer(layer)).and_then(|l| l.grid.tile(index)).map(|t| t.px.as_ptr()).unwrap_or(std::ptr::null())
    })
}

/// The layers, the palette and the tiles each layer has.
pub fn info() -> serde_json::Value {
    HELD.with(|h| {
        let h = h.borrow();
        let Some(canvas) = h.canvas.as_ref() else { return serde_json::Value::Null };
        let tiles: Vec<(u32, Vec<u32>)> = canvas
            .layers
            .iter()
            .map(|l| (l.info.id, (0..l.grid.cols * l.grid.rows).filter(|&i| l.grid.tile(i).is_some()).collect()))
            .collect();
        serde_json::json!({ "layers": canvas.infos(), "palette": canvas.palette, "tiles": tiles })
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
