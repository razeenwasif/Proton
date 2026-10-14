//! Selections and moving pixels: shapes selected and joined, feathered and
//! inverted; edits held to what is selected; copy, cut and paste; pixels
//! moved with the selection; and the whole canvas turned, flipped, resized
//! and cropped — every one of them undone.

use proton_raster::{Area, Brush, Canvas, History, Mode, Op, Preset, ShapeKind, Tip, Tool, SELECTION};

const RED: [u8; 4] = [220, 40, 40, 255];
const BLUE: [u8; 4] = [30, 60, 200, 255];

fn canvas(w: u32, h: u32, preset: Preset) -> (Canvas, u32) {
    let c = Canvas::new(w, h, preset).unwrap();
    let id = c.layers[0].info.id;
    (c, id)
}

fn select(area: Area, mode: Mode) -> Op {
    Op::Select { area, mode, feather: 0, smooth: false }
}

/// How selected a pixel is.
fn sel(c: &Canvas, x: u32, y: u32) -> u8 {
    c.selection.pixel(x, y)[3]
}

fn count(c: &Canvas, f: impl Fn(u8) -> bool) -> u32 {
    let mut n = 0;
    for y in 0..c.height {
        for x in 0..c.width {
            n += f(sel(c, x, y)) as u32;
        }
    }
    n
}

#[test]
fn shapes_are_selected_and_joined_as_their_modes_say() {
    let (mut c, _) = canvas(100, 80, Preset::Paint);
    c.apply(&select(Area::Rect { rect: [10, 10, 30, 20] }, Mode::Replace)).unwrap();
    assert_eq!(count(&c, |s| s == 255), 600);
    c.apply(&select(Area::Rect { rect: [30, 10, 30, 20] }, Mode::Add)).unwrap();
    assert_eq!(count(&c, |s| s == 255), 50 * 20);
    c.apply(&select(Area::Rect { rect: [20, 0, 10, 80] }, Mode::Subtract)).unwrap();
    assert_eq!(count(&c, |s| s == 255), 40 * 20);
    c.apply(&select(Area::Rect { rect: [0, 0, 35, 80] }, Mode::Intersect)).unwrap();
    assert_eq!(count(&c, |s| s == 255), 15 * 20);
    c.apply(&select(Area::Invert, Mode::Replace)).unwrap();
    assert_eq!(count(&c, |s| s == 255), 100 * 80 - 300);
    c.apply(&select(Area::Nothing, Mode::Replace)).unwrap();
    assert!(!c.selecting());
    c.apply(&select(Area::All, Mode::Replace)).unwrap();
    assert_eq!(count(&c, |s| s == 255), 8000);
}

#[test]
fn an_ellipse_and_a_lasso_are_soft_edged_on_a_paint_canvas_and_hard_on_a_pixel_one() {
    for (preset, smooth) in [(Preset::Paint, true), (Preset::Pixel, false)] {
        let (mut c, _) = canvas(64, 64, preset);
        c.apply(&Op::Select { area: Area::Ellipse { rect: [4, 8, 50, 40] }, mode: Mode::Replace, feather: 0, smooth }).unwrap();
        assert_eq!(sel(&c, 29, 28), 255, "the middle");
        assert_eq!(sel(&c, 2, 2), 0, "a corner of the box");
        let soft = count(&c, |s| s > 0 && s < 255);
        assert!(if smooth { soft > 20 } else { soft == 0 }, "{preset:?}: {soft} soft pixels");
        // The ellipse's area, within its edge.
        let area = count(&c, |s| s >= 128) as f64;
        assert!((area - std::f64::consts::PI * 25.0 * 20.0).abs() < 60.0, "{area}");
    }
    // A right triangle by its three corners: half its box.
    let (mut c, _) = canvas(64, 64, Preset::Paint);
    let pts = [8, 8, 56, 8, 8, 56].map(|v| v * 256).to_vec();
    c.apply(&Op::Select { area: Area::Lasso { points: pts }, mode: Mode::Replace, feather: 0, smooth: true }).unwrap();
    let area = count(&c, |s| s >= 128) as i64;
    assert!((area - 48 * 48 / 2).abs() < 50, "{area}");
    // The same outline drawn the other way round selects the same.
    let mut d = Canvas::new(64, 64, Preset::Paint).unwrap();
    let back = [8, 56, 56, 8, 8, 8].map(|v| v * 256).to_vec();
    d.apply(&Op::Select { area: Area::Lasso { points: back }, mode: Mode::Replace, feather: 0, smooth: true }).unwrap();
    assert_eq!(c.selection, d.selection);
}

#[test]
fn a_feather_softens_the_edge_and_the_wand_takes_what_is_like() {
    let (mut c, id) = canvas(80, 40, Preset::Paint);
    c.apply(&Op::Select { area: Area::Rect { rect: [20, 0, 40, 40] }, mode: Mode::Replace, feather: 8, smooth: true }).unwrap();
    let row: Vec<u8> = (10..30).map(|x| sel(&c, x, 20)).collect();
    assert!(row.windows(2).all(|w| w[0] <= w[1]) && row[0] < 30 && row[19] > 225, "{row:?}");
    c.apply(&select(Area::Nothing, Mode::Replace)).unwrap();
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 30, 40], color: BLUE }).unwrap();
    c.apply(&select(Area::Wand { layer: id, x: 5, y: 5, tolerance: 0, contiguous: true, diagonal: false }, Mode::Replace)).unwrap();
    assert_eq!(count(&c, |s| s == 255), 30 * 40);
}

#[test]
fn every_edit_keeps_to_the_selection() {
    let (mut c, id) = canvas(64, 64, Preset::Pixel);
    c.apply(&select(Area::Rect { rect: [10, 10, 20, 20] }, Mode::Replace)).unwrap();
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 64, 64], color: RED }).unwrap();
    assert_eq!((c.layers[0].grid.pixel(15, 15), c.layers[0].grid.pixel(5, 5)), (RED, [0; 4]));
    let tip = Tip { size: 1, round: false, mirror: [None, None] };
    c.apply(&Op::Stroke { layer: id, tool: Tool::Pencil, tip: tip.clone(), color: BLUE, pixel_perfect: false, points: vec![2 * 256, 20 * 256, 65535, 60 * 256, 20 * 256, 65535] }).unwrap();
    assert_eq!((c.layers[0].grid.pixel(20, 20), c.layers[0].grid.pixel(40, 20)), (BLUE, [0; 4]));
    c.apply(&Op::Shape { layer: id, shape: ShapeKind::Line, from: [0, 25], to: [63, 25], filled: false, tip, color: BLUE }).unwrap();
    assert_eq!(c.layers[0].grid.pixel(50, 25), [0; 4]);
    c.apply(&Op::Fill { layer: id, x: 50, y: 50, color: BLUE, tolerance: 0, contiguous: true, diagonal: false }).unwrap();
    assert_eq!(c.layers[0].grid.pixel(50, 50), [0; 4], "a bucket outside the selection");
    let brush = Brush { size: 30, hardness: 100, spacing: 10, flow: 255, opacity: 255, size_pressure: 0, flow_pressure: 0, opacity_pressure: 0 };
    c.apply(&Op::Brush { layer: id, erase: false, brush, color: BLUE, points: vec![32 * 256, 32 * 256, 65535] }).unwrap();
    assert_eq!((c.layers[0].grid.pixel(28, 28), c.layers[0].grid.pixel(34, 34)), (BLUE, [0; 4]));
    // Half selected, half painted, and no darker for it.
    let (mut s, id) = canvas(32, 32, Preset::Paint);
    s.apply(&Op::Select { area: Area::Rect { rect: [0, 0, 32, 32] }, mode: Mode::Replace, feather: 0, smooth: false }).unwrap();
    s.apply(&Op::Select { area: Area::Rect { rect: [0, 0, 32, 32] }, mode: Mode::Intersect, feather: 0, smooth: false }).unwrap();
    s.selection = proton_raster::select::from_dense(32, 32, &[128u8; 32 * 32]);
    s.apply(&Op::FillRect { layer: id, rect: [0, 0, 32, 32], color: RED }).unwrap();
    assert_eq!(s.layers[0].grid.pixel(3, 3), [220, 40, 40, 128]);
}

#[test]
fn copied_pixels_paste_as_a_layer_and_a_cut_clears_them() {
    let (mut c, id) = canvas(64, 64, Preset::Paint);
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 64, 64], color: RED }).unwrap();
    assert!(c.apply(&Op::Paste { above: None }).is_err(), "nothing copied yet");
    c.apply(&select(Area::Rect { rect: [8, 8, 8, 8] }, Mode::Replace)).unwrap();
    let (copied, _) = c.apply(&Op::Copy { layer: id, cut: false }).unwrap();
    assert!(!copied.changed, "copying changes nothing");
    let (pasted, _) = c.apply(&Op::Paste { above: Some(id) }).unwrap();
    let new = pasted.made.unwrap();
    let layer = c.layer(new).unwrap();
    assert_eq!((layer.info.name.as_str(), layer.grid.pixel(10, 10), layer.grid.pixel(30, 30)), ("Pasted", RED, [0; 4]));
    let (cut, _) = c.apply(&Op::Copy { layer: id, cut: true }).unwrap();
    assert!(cut.changed);
    assert_eq!((c.layer(id).unwrap().grid.pixel(10, 10), c.layer(id).unwrap().grid.pixel(30, 30)), ([0; 4], RED));
    c.apply(&Op::Clear { layer: new }).unwrap();
    assert_eq!(c.layer(new).unwrap().grid.pixel(10, 10), [0; 4]);
}

#[test]
fn moved_pixels_carry_their_selection_and_one_undo_puts_both_back() {
    let (mut c, id) = canvas(64, 64, Preset::Pixel);
    let mut h = History::new(Preset::Pixel);
    h.apply(&mut c, &Op::FillRect { layer: id, rect: [4, 4, 6, 6], color: RED }, "fill").unwrap();
    h.apply(&mut c, &select(Area::Rect { rect: [4, 4, 6, 6] }, Mode::Replace), "select").unwrap();
    let before = c.hash();
    // Ten across, twenty down: each result pixel from ten left, twenty up.
    let one = 65536i64;
    let applied = h.apply(&mut c, &Op::Transform { layer: id, bounds: [14, 24, 6, 6], inverse: [one, 0, -10 * one, 0, one, -20 * one], smooth: false }, "move").unwrap();
    assert!(applied.touched.iter().any(|t| t.0 == SELECTION) && applied.touched.iter().any(|t| t.0 == id));
    assert_eq!((c.layers[0].grid.pixel(15, 25), c.layers[0].grid.pixel(5, 5)), (RED, [0; 4]));
    assert_eq!((sel(&c, 15, 25), sel(&c, 5, 5)), (255, 0));
    h.undo(&mut c);
    assert_eq!(c.hash(), before);
    // With nothing selected, the whole layer moves.
    h.apply(&mut c, &select(Area::Nothing, Mode::Replace), "deselect").unwrap();
    h.apply(&mut c, &Op::Transform { layer: id, bounds: [0, 0, 64, 64], inverse: [one, 0, -one, 0, one, 0], smooth: false }, "move").unwrap();
    assert_eq!((c.layers[0].grid.pixel(5, 4), c.layers[0].grid.pixel(4, 4)), (RED, [0; 4]));
}

#[test]
fn the_canvas_turns_flips_resizes_and_crops_and_undo_puts_it_back() {
    let (mut c, id) = canvas(40, 24, Preset::Pixel);
    let mut h = History::new(Preset::Pixel);
    h.apply(&mut c, &Op::FillRect { layer: id, rect: [0, 0, 3, 2], color: RED }, "fill").unwrap();
    h.apply(&mut c, &select(Area::Rect { rect: [0, 0, 5, 5] }, Mode::Replace), "select").unwrap();
    let start = c.hash();
    for _ in 0..4 {
        h.apply(&mut c, &Op::Orient { turns: 1, flip: false }, "turn").unwrap();
    }
    assert_eq!(c.hash(), start, "four quarter turns are no turn");
    h.apply(&mut c, &Op::Orient { turns: 1, flip: false }, "turn").unwrap();
    assert_eq!((c.width, c.height), (24, 40));
    assert_eq!(c.layers[0].grid.pixel(23, 0), RED, "the top-left corner turned to the top right");
    assert_eq!(sel(&c, 23, 0), 255, "the selection turns with it");
    h.undo(&mut c);
    assert_eq!((c.width, c.height, c.hash()), (40, 24, start));
    h.apply(&mut c, &Op::Orient { turns: 0, flip: true }, "flip").unwrap();
    assert_eq!(c.layers[0].grid.pixel(39, 0), RED);
    h.apply(&mut c, &Op::Orient { turns: 0, flip: true }, "flip").unwrap();
    assert_eq!(c.hash(), start, "two flips are none");
    h.apply(&mut c, &Op::Resize { width: 80, height: 48, smooth: false }, "resize").unwrap();
    assert_eq!((c.layers[0].grid.pixel(5, 3), c.layers[0].grid.pixel(6, 4)), (RED, [0; 4]));
    h.undo(&mut c);
    h.apply(&mut c, &Op::Crop { rect: [1, 1, 10, 10] }, "crop").unwrap();
    assert_eq!((c.width, c.height, c.layers[0].grid.pixel(0, 0), c.layers[0].grid.pixel(2, 0)), (10, 10, RED, [0; 4]));
    h.undo(&mut c);
    assert_eq!(c.hash(), start);
    assert!(c.apply(&Op::Resize { width: 2000, height: 10, smooth: false }).is_err(), "past a pixel canvas's most");
    assert!(c.apply(&Op::Crop { rect: [0, 0, 0, 10] }).is_err());
}

/// A turned, smoothly sampled layer, and a feathered, joined selection, as
/// numbers: a change to how pixels are sampled or shapes are selected
/// changes them, and must be meant. `PROTON_BLESS=1` prints new ones.
#[test]
fn a_smooth_turn_and_a_feathered_selection_make_what_they_always_made() {
    let (mut c, id) = canvas(96, 96, Preset::Paint);
    for i in 0..8 {
        c.apply(&Op::FillRect { layer: id, rect: [i * 12, 0, 12, 96], color: [(i * 30) as u8, 200 - (i * 20) as u8, 90, 255 - (i * 10) as u8] }).unwrap();
    }
    // A sixth of a turn about the middle, scaled by 0.8: the inverse turns
    // back and scales by 1.25, in 1/65536.
    let (cos, sin, k) = (0.5f64, 0.866_025_403_784_438_6f64, 1.25f64);
    let m = |v: f64| (v * 65536.0).round() as i64;
    let (a, b, d, e) = (m(k * cos), m(k * sin), m(-k * sin), m(k * cos));
    let (cx, cy) = (48.0, 48.0);
    let inverse = [a, b, m(cx - k * (cos * cx + sin * cy)), d, e, m(cy - k * (-sin * cx + cos * cy))];
    c.apply(&Op::Transform { layer: id, bounds: [0, 0, 96, 96], inverse, smooth: true }).unwrap();
    c.apply(&Op::Select { area: Area::Ellipse { rect: [10, 20, 60, 40] }, mode: Mode::Replace, feather: 6, smooth: true }).unwrap();
    c.apply(&Op::Select { area: Area::Lasso { points: [5, 5, 90, 30, 40, 90].map(|v| v * 256).to_vec() }, mode: Mode::Add, feather: 0, smooth: true }).unwrap();
    let hashes = format!("{:016x} {:016x}", c.layers[0].grid.tiles.iter().fold(0u64, |h, t| h.rotate_left(5) ^ t.as_ref().map_or(0, |t| t.hash())), c.hash());
    if std::env::var("PROTON_BLESS").is_ok() {
        println!("sampled and selected: {hashes}");
        return;
    }
    assert_eq!(hashes, "8da9340e23f43107 793d953ed014af14");
}
