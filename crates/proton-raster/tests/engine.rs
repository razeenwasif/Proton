//! The engine: blend modes as the W3C defines them, ops applied whole or not
//! at all, undo and redo back to the same pixels, history held to its limits,
//! and the rules that keep every build making the same pixels.

use proton_raster::blend::{flatten, mix};
use proton_raster::{Blend, Canvas, History, Op, Preset};

/// A small seeded generator for test pixels: the same every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u8 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 56) as u8
    }
}

/// The W3C's `B(Cb, Cs)`, in floats.
fn reference(mode: Blend, cb: f64, cs: f64) -> f64 {
    let multiply = |a: f64, b: f64| a * b;
    let screen = |a: f64, b: f64| a + b - a * b;
    let hard = |cb: f64, cs: f64| if cs <= 0.5 { multiply(cb, 2.0 * cs) } else { screen(cb, 2.0 * cs - 1.0) };
    match mode {
        Blend::Normal => cs,
        Blend::Multiply => cb * cs,
        Blend::Screen => screen(cb, cs),
        Blend::Overlay => hard(cs, cb),
        Blend::Darken => cb.min(cs),
        Blend::Lighten => cb.max(cs),
        Blend::ColorDodge => {
            if cb == 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        Blend::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs == 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        Blend::HardLight => hard(cb, cs),
        Blend::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 { ((16.0 * cb - 12.0) * cb + 4.0) * cb } else { cb.sqrt() };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        Blend::Difference => (cb - cs).abs(),
        Blend::Exclusion => cb + cs - 2.0 * cb * cs,
        Blend::Add => (cb + cs).min(1.0),
        Blend::Subtract => (cb - cs).max(0.0),
    }
}

/// One pixel of `src` at `opacity` in `mode` over `back`, both straight, by
/// the W3C's formula in floats: straight 8-bit RGBA.
fn composite(back: [u8; 4], src: [u8; 4], opacity: u8, mode: Blend) -> [f64; 4] {
    let ab = back[3] as f64 / 255.0;
    let a_s = src[3] as f64 / 255.0 * opacity as f64 / 255.0;
    let ao = a_s + ab * (1.0 - a_s);
    let mut out = [0.0; 4];
    for c in 0..3 {
        let (cb, cs) = (back[c] as f64 / 255.0, src[c] as f64 / 255.0);
        let co = cs * a_s * (1.0 - ab) + cb * ab * (1.0 - a_s) + a_s * ab * reference(mode, cb, cs);
        out[c] = if ao > 0.0 { co / ao * 255.0 } else { 0.0 };
    }
    out[3] = ao * 255.0;
    out
}

fn one_pixel(back: [u8; 4], src: [u8; 4], opacity: u8, mode: Blend) -> Canvas {
    let mut c = Canvas::new(1, 1, Preset::Paint).unwrap();
    let bottom = c.layers[0].info.id;
    c.apply(&Op::FillRect { layer: bottom, rect: [0, 0, 1, 1], color: back }).unwrap();
    let (applied, _) = c.apply(&Op::AddLayer { name: "top".into(), at: None }).unwrap();
    let top = applied.made.unwrap();
    c.apply(&Op::FillRect { layer: top, rect: [0, 0, 1, 1], color: src }).unwrap();
    c.apply(&Op::SetLayer { layer: top, name: None, opacity: Some(opacity), blend: Some(mode), hidden: None, locked: None, alpha_lock: None }).unwrap();
    c
}

#[test]
fn every_blend_mode_lands_within_a_shade_of_the_w3c_formula() {
    let mut rng = Lcg(7);
    for mode in Blend::ALL {
        for _ in 0..400 {
            let back = [rng.next(), rng.next(), rng.next(), rng.next()];
            let src = [rng.next(), rng.next(), rng.next(), rng.next()];
            let opacity = if rng.next() < 128 { 255 } else { rng.next() };
            let got = flatten(&one_pixel(back, src, opacity, mode));
            let want = composite(back, src, opacity, mode);
            assert!((got[3] as f64 - want[3]).abs() <= 1.0, "{mode:?} alpha: {back:?} under {src:?}@{opacity}: {} vs {:.2}", got[3], want[3]);
            // Colour is only as exact as the alpha it is divided by.
            if want[3] >= 16.0 {
                for c in 0..3 {
                    assert!((got[c] as f64 - want[c]).abs() <= 1.0, "{mode:?} channel {c}: {back:?} under {src:?}@{opacity}: {} vs {:.2}", got[c], want[c]);
                }
            }
        }
    }
}

#[test]
fn the_blend_functions_meet_at_their_edges() {
    let m = 65535u64;
    for mode in Blend::ALL {
        for (cb, cs) in [(0, 0), (m, m), (0, m), (m, 0), (m / 2, m / 2), (m / 4, 3 * m / 4)] {
            let got = mix(mode, cb, cs) as f64 / m as f64;
            let want = reference(mode, cb as f64 / m as f64, cs as f64 / m as f64);
            assert!((got - want).abs() < 2.0 / m as f64 * 4.0, "{mode:?}({cb}, {cs}) = {got}, not {want}");
        }
    }
}

/// A stack of every mode, flattened, as one number: any change to how a
/// mode is computed shows here, in every build alike.
#[test]
fn a_stack_of_every_mode_flattens_to_the_same_pixels_every_time() {
    let (w, h) = (40u32, 30u32);
    let mut c = Canvas::new(w, h, Preset::Paint).unwrap();
    let mut rng = Lcg(42);
    let bottom = c.layers[0].info.id;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let color = [rng.next(), rng.next(), rng.next(), 255];
            c.apply(&Op::FillRect { layer: bottom, rect: [x, y, 1, 1], color }).unwrap();
        }
    }
    for (i, mode) in Blend::ALL.into_iter().enumerate() {
        let (applied, _) = c.apply(&Op::AddLayer { name: format!("{mode:?}"), at: None }).unwrap();
        let id = applied.made.unwrap();
        let i = i as i32;
        c.apply(&Op::FillRect { layer: id, rect: [i * 2, i, 14, 12], color: [rng.next(), rng.next(), rng.next(), 128 + rng.next() / 2] }).unwrap();
        c.apply(&Op::SetLayer { layer: id, name: None, opacity: Some(200), blend: Some(mode), hidden: None, locked: None, alpha_lock: None }).unwrap();
    }
    let flat = flatten(&c);
    let mut h = proton_raster::fixed::Hash::default();
    h.bytes(&flat);
    assert_eq!(h.hex(), GOLDEN, "the flattened stack changed");
}

const GOLDEN: &str = "710cd088d620ad86";

#[test]
fn an_op_that_cannot_be_made_changes_nothing() {
    let mut c = Canvas::new(8, 8, Preset::Pixel).unwrap();
    let id = c.layers[0].info.id;
    let before = (c.hash(), c.state);
    assert!(c.apply(&Op::RemoveLayer { layer: id }).unwrap_err().contains("at least one layer"));
    assert!(c.apply(&Op::RemoveLayer { layer: 99 }).is_err());
    c.apply(&Op::SetLayer { layer: id, name: None, opacity: None, blend: None, hidden: None, locked: Some(true), alpha_lock: None }).unwrap();
    let locked = (c.hash(), c.state);
    assert!(c.apply(&Op::FillRect { layer: id, rect: [0, 0, 2, 2], color: [255, 0, 0, 255] }).unwrap_err().contains("locked"));
    assert_eq!((c.hash(), c.state), locked);
    assert!(c.apply(&Op::SetPalette { colors: vec![[0, 0, 0, 255]; 257] }).is_err());
    assert_ne!(before, locked, "locking is a change");
    // Asked to make what is already there: no change, no new state.
    let (applied, _) = c.apply(&Op::SetLayer { layer: id, name: None, opacity: None, blend: None, hidden: None, locked: Some(true), alpha_lock: None }).unwrap();
    assert!(!applied.changed);
    assert_eq!(c.state, locked.1);
}

#[test]
fn a_fill_clips_to_the_canvas_tidies_clear_tiles_and_keeps_alpha_locked_shapes() {
    let mut c = Canvas::new(100, 70, Preset::Paint).unwrap();
    let id = c.layers[0].info.id;
    let (applied, _) = c.apply(&Op::FillRect { layer: id, rect: [-10, 60, 200, 50], color: [10, 20, 30, 255] }).unwrap();
    // Rows 60–69, across: they straddle the tiles' edge at 64, so all four
    // tiles of the 2 × 2 grid.
    assert_eq!(applied.touched, [(id, 0), (id, 1), (id, 2), (id, 3)]);
    assert_eq!(c.layers[0].grid.pixel(99, 69), [10, 20, 30, 255]);
    assert_eq!(c.layers[0].grid.pixel(0, 59), [0, 0, 0, 0]);
    // Cleared again: the tiles are let go.
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 100, 70], color: [0, 0, 0, 0] }).unwrap();
    assert!(c.layers[0].grid.tiles.iter().all(Option::is_none));
    // Alpha-locked: colour changes only where there is something.
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 4, 4], color: [9, 9, 9, 100] }).unwrap();
    c.apply(&Op::SetLayer { layer: id, name: None, opacity: None, blend: None, hidden: None, locked: None, alpha_lock: Some(true) }).unwrap();
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 8, 8], color: [200, 100, 0, 255] }).unwrap();
    assert_eq!(c.layers[0].grid.pixel(3, 3), [200, 100, 0, 100]);
    assert_eq!(c.layers[0].grid.pixel(6, 6), [0, 0, 0, 0]);
}

#[test]
fn undo_and_redo_go_back_to_the_same_pixels_and_saved_is_saved_again() {
    let mut c = Canvas::new(130, 70, Preset::Paint).unwrap();
    let mut history = History::new(Preset::Paint);
    let base = c.layers[0].info.id;
    let start = c.hash();
    let saved = c.state;
    history.apply(&mut c, &Op::FillRect { layer: base, rect: [5, 5, 100, 40], color: [255, 0, 0, 255] }, "fill").unwrap();
    let added = history.apply(&mut c, &Op::AddLayer { name: "Ink".into(), at: None }, "add").unwrap().made.unwrap();
    history.apply(&mut c, &Op::FillRect { layer: added, rect: [60, 30, 60, 30], color: [0, 0, 255, 200] }, "fill").unwrap();
    history.apply(&mut c, &Op::SetLayer { layer: added, name: None, opacity: Some(90), blend: Some(Blend::Multiply), hidden: None, locked: None, alpha_lock: None }, "blend").unwrap();
    history.apply(&mut c, &Op::MoveLayer { layer: added, to: 0 }, "move").unwrap();
    history.apply(&mut c, &Op::RemoveLayer { layer: base }, "remove").unwrap();
    let end = (c.hash(), flatten(&c));
    assert_ne!(c.state, saved, "changed, but not unsaved");
    let mut undone = Vec::new();
    while let Some((applied, label)) = history.undo(&mut c) {
        assert!(applied.changed);
        undone.push(label);
    }
    assert_eq!(undone, ["remove", "move", "blend", "fill", "add", "fill"]);
    assert_eq!(c.hash(), start, "undo did not go back to the start");
    assert_eq!(c.state, saved, "undone to the saved state, but unsaved");
    while history.redo(&mut c).is_some() {}
    assert_eq!((c.hash(), flatten(&c)), end, "redo did not come back to the end");
    // A new change after an undo drops what could have been redone.
    history.undo(&mut c);
    history.apply(&mut c, &Op::SetPalette { colors: vec![[1, 2, 3, 255]] }, "palette").unwrap();
    assert!(!history.can_redo());
}

#[test]
fn what_undo_sends_the_window_is_every_tile_that_changed() {
    let mut c = Canvas::new(128, 128, Preset::Paint).unwrap();
    let mut history = History::new(Preset::Paint);
    let base = c.layers[0].info.id;
    history.apply(&mut c, &Op::FillRect { layer: base, rect: [0, 0, 128, 10], color: [1, 1, 1, 255] }, "fill").unwrap();
    history.apply(&mut c, &Op::RemoveLayer { layer: base }, "remove").unwrap_err();
    let ink = history.apply(&mut c, &Op::AddLayer { name: String::new(), at: None }, "add").unwrap().made.unwrap();
    history.apply(&mut c, &Op::RemoveLayer { layer: base }, "remove").unwrap();
    let (applied, _) = history.undo(&mut c).unwrap();
    assert!(applied.meta);
    let mut touched = applied.touched.clone();
    touched.sort();
    assert_eq!(touched, [(base, 0), (base, 1)], "the layer back from removal is not all sent");
    assert!(c.layer(ink).is_some());
}

#[test]
fn history_is_held_to_its_bytes_the_oldest_let_go_and_the_newest_kept() {
    let tile = proton_raster::tile::TILE_BYTES as u64;
    let mut c = Canvas::new(256, 64, Preset::Paint).unwrap();
    let mut history = History::with_limits(2 * tile, 100);
    let id = c.layers[0].info.id;
    for i in 0..4 {
        history.apply(&mut c, &Op::FillRect { layer: id, rect: [i * 64, 0, 64, 64], color: [9, 9, 9, 255] }, "fill").unwrap();
        // Filling a clear tile keeps nothing before it: it costs no bytes.
        history.apply(&mut c, &Op::FillRect { layer: id, rect: [i * 64, 0, 64, 64], color: [8, 8, 8, 255] }, "again").unwrap();
    }
    assert!(history.bytes() <= 2 * tile, "{} bytes kept", history.bytes());
    assert!(history.steps() < 8 && history.can_undo());
    // A step larger than the whole allowance is still kept, alone.
    let mut big = History::with_limits(1, 100);
    big.apply(&mut c, &Op::FillRect { layer: id, rect: [0, 0, 256, 64], color: [7, 7, 7, 255] }, "big").unwrap();
    big.apply(&mut c, &Op::FillRect { layer: id, rect: [0, 0, 256, 64], color: [6, 6, 6, 255] }, "bigger").unwrap();
    assert_eq!(big.steps(), 1);
    assert_eq!(big.undo(&mut c).unwrap().1, "bigger");
}

#[test]
fn the_window_and_the_core_agree_on_the_tiles_an_op_touched() {
    let mut a = Canvas::new(200, 200, Preset::Paint).unwrap();
    let mut b = a.clone();
    let id = a.layers[0].info.id;
    let op = Op::FillRect { layer: id, rect: [50, 50, 100, 30], color: [3, 4, 5, 255] };
    let (x, _) = a.apply(&op).unwrap();
    let (y, _) = b.apply(&op).unwrap();
    assert_eq!(a.hash_touched(&x.touched), b.hash_touched(&y.touched));
    b.apply(&Op::FillRect { layer: id, rect: [60, 60, 1, 1], color: [0, 0, 0, 255] }).unwrap();
    assert_ne!(a.hash_touched(&x.touched), b.hash_touched(&x.touched), "a pixel's difference did not show");
}

/// What makes the window's WebAssembly and the core make the same pixels:
/// no floats, no unordered maps, no arithmetic in `usize` (32 bits in
/// WebAssembly, 64 natively).
#[test]
fn the_engine_holds_no_floats_unordered_maps_or_pointer_sized_arithmetic() {
    let engine = [
        ("fixed.rs", include_str!("../src/fixed.rs")),
        ("tile.rs", include_str!("../src/tile.rs")),
        ("canvas.rs", include_str!("../src/canvas.rs")),
        ("blend.rs", include_str!("../src/blend.rs")),
        ("op.rs", include_str!("../src/op.rs")),
        ("history.rs", include_str!("../src/history.rs")),
        ("stroke.rs", include_str!("../src/stroke.rs")),
        ("fill.rs", include_str!("../src/fill.rs")),
        ("brush.rs", include_str!("../src/brush.rs")),
    ];
    for (name, source) in engine {
        let code: String = source.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for banned in ["f32", "f64", "HashMap", "HashSet", ": usize", "as usize +", "as usize -", "as usize *", "as usize /"] {
            assert!(!code.contains(banned), "{name} uses {banned}");
        }
    }
}
