//! The pixel tools: a pencil that steps cleanly and paints a crossing once,
//! an eraser, alpha-lock and locked layers; a bucket that spreads to what is
//! like it; lines, rectangles and ellipses; and a colour replaced everywhere
//! with its swatch, as one edit.

use proton_raster::{Canvas, History, Op, Preset, ShapeKind, Tip, Tool};

const RED: [u8; 4] = [220, 40, 40, 255];
const BLUE: [u8; 4] = [30, 60, 200, 255];

fn canvas(w: u32, h: u32) -> (Canvas, u32) {
    let c = Canvas::new(w, h, Preset::Pixel).unwrap();
    let id = c.layers[0].info.id;
    (c, id)
}

fn tip(size: u32) -> Tip {
    Tip { size, round: false, mirror: [None, None] }
}

/// Pixels' centres, as a stroke's points.
fn points(pixels: &[(i32, i32)]) -> Vec<i32> {
    pixels.iter().flat_map(|&(x, y)| [x * 256 + 128, y * 256 + 128, 65535]).collect()
}

fn pencil(layer: u32, pixels: &[(i32, i32)], perfect: bool) -> Op {
    Op::Stroke { layer, tool: Tool::Pencil, tip: tip(1), color: RED, pixel_perfect: perfect, points: points(pixels) }
}

fn painted(c: &Canvas, layer: u32) -> Vec<(i32, i32)> {
    let l = c.layer(layer).unwrap();
    let mut out = Vec::new();
    for y in 0..c.height {
        for x in 0..c.width {
            if l.grid().pixel(x, y)[3] > 0 {
                out.push((x as i32, y as i32));
            }
        }
    }
    out
}

#[test]
fn a_pixel_perfect_pencil_steps_without_the_corners_of_its_ls() {
    // A staircase drawn as a hand draws it: across one, down one.
    let steps = [(0, 0), (1, 0), (1, 1), (2, 1), (2, 2), (3, 2)];
    let (mut c, id) = canvas(8, 8);
    c.apply(&pencil(id, &steps, true)).unwrap();
    assert_eq!(painted(&c, id), [(0, 0), (1, 1), (2, 2), (3, 2)], "the corners were not taken out");
    let (mut plain, id) = canvas(8, 8);
    plain.apply(&pencil(id, &steps, false)).unwrap();
    assert_eq!(painted(&plain, id).len(), 6);
    // Points far apart are joined, with no gap.
    let (mut c, id) = canvas(16, 16);
    c.apply(&pencil(id, &[(1, 1), (12, 5)], false)).unwrap();
    assert_eq!(painted(&c, id).len(), 12);
}

#[test]
fn a_stroke_paints_where_it_crosses_itself_once_and_mirrors() {
    let (mut c, id) = canvas(8, 8);
    let half = [255, 0, 0, 128];
    c.apply(&Op::Stroke { layer: id, tool: Tool::Pencil, tip: tip(1), color: half, pixel_perfect: false, points: points(&[(0, 3), (6, 3), (3, 0), (3, 6)]) }).unwrap();
    // Where the line crosses back over itself it is no darker.
    assert_eq!(c.layer(id).unwrap().grid().pixel(3, 3), half);
    let (mut m, id) = canvas(16, 16);
    let mirrored = Tip { size: 1, round: false, mirror: [Some(16), None] };
    m.apply(&Op::Stroke { layer: id, tool: Tool::Pencil, tip: mirrored, color: RED, pixel_perfect: false, points: points(&[(2, 5)]) }).unwrap();
    assert_eq!(painted(&m, id), [(2, 5), (13, 5)]);
}

#[test]
fn an_eraser_clears_and_alpha_lock_keeps_the_shape() {
    let (mut c, id) = canvas(8, 8);
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 4, 4], color: BLUE }).unwrap();
    c.apply(&Op::Stroke { layer: id, tool: Tool::Eraser, tip: tip(2), color: RED, pixel_perfect: false, points: points(&[(0, 0)]) }).unwrap();
    assert_eq!(painted(&c, id).len(), 12);
    c.apply(&Op::SetLayer { layer: id, name: None, opacity: None, blend: None, hidden: None, locked: None, alpha_lock: Some(true) }).unwrap();
    // Locked to its shape: the pencil colours only what is there.
    c.apply(&pencil(id, &[(3, 3), (6, 6)], false)).unwrap();
    assert_eq!(c.layer(id).unwrap().grid().pixel(3, 3), RED);
    assert_eq!(c.layer(id).unwrap().grid().pixel(5, 5), [0, 0, 0, 0]);
    c.apply(&Op::SetLayer { layer: id, name: None, opacity: None, blend: None, hidden: None, locked: Some(true), alpha_lock: None }).unwrap();
    assert!(c.apply(&pencil(id, &[(1, 1)], false)).unwrap_err().contains("locked"));
}

#[test]
fn a_bucket_spreads_to_what_is_like_it() {
    let (mut c, id) = canvas(10, 10);
    // A box of blue with a gap at one corner of its wall.
    c.apply(&Op::Shape { layer: id, shape: ShapeKind::Rect, from: [2, 2], to: [7, 7], filled: false, tip: tip(1), color: BLUE }).unwrap();
    c.apply(&Op::FillRect { layer: id, rect: [7, 7, 1, 1], color: [0, 0, 0, 0] }).unwrap();
    // Inside, four-connected: the corner gap is diagonal, so it stays in.
    c.apply(&Op::Fill { layer: id, x: 4, y: 4, color: RED, tolerance: 0, contiguous: true, diagonal: false }).unwrap();
    assert_eq!(c.layer(id).unwrap().grid().pixel(4, 4), RED);
    assert_eq!(c.layer(id).unwrap().grid().pixel(0, 0), [0, 0, 0, 0], "four-connected fill leaked through a diagonal");
    // Eight-connected, it gets out through the corner.
    let (mut d, id2) = canvas(10, 10);
    d.apply(&Op::Shape { layer: id2, shape: ShapeKind::Rect, from: [2, 2], to: [7, 7], filled: false, tip: tip(1), color: BLUE }).unwrap();
    d.apply(&Op::FillRect { layer: id2, rect: [7, 7, 1, 1], color: [0, 0, 0, 0] }).unwrap();
    d.apply(&Op::Fill { layer: id2, x: 4, y: 4, color: RED, tolerance: 0, contiguous: true, diagonal: true }).unwrap();
    assert_eq!(d.layer(id2).unwrap().grid().pixel(0, 0), RED);
    // Everywhere like it, not only what touches it.
    let (mut g, id3) = canvas(6, 1);
    for x in [0, 2, 4] {
        g.apply(&Op::FillRect { layer: id3, rect: [x, 0, 1, 1], color: BLUE }).unwrap();
    }
    g.apply(&Op::Fill { layer: id3, x: 0, y: 0, color: RED, tolerance: 0, contiguous: false, diagonal: false }).unwrap();
    assert_eq!(painted(&g, id3), [(0, 0), (2, 0), (4, 0)]);
    assert_eq!(g.layer(id3).unwrap().grid().pixel(4, 0), RED);
    // Within a tolerance, a near colour is the same colour.
    let (mut t, id4) = canvas(2, 1);
    t.apply(&Op::FillRect { layer: id4, rect: [0, 0, 1, 1], color: [100, 100, 100, 255] }).unwrap();
    t.apply(&Op::FillRect { layer: id4, rect: [1, 0, 1, 1], color: [104, 100, 97, 255] }).unwrap();
    t.apply(&Op::Fill { layer: id4, x: 0, y: 0, color: RED, tolerance: 4, contiguous: true, diagonal: false }).unwrap();
    assert_eq!(t.layer(id4).unwrap().grid().pixel(1, 0), RED);
    // Filling with the colour already there changes nothing.
    let (applied, _) = t.apply(&Op::Fill { layer: id4, x: 0, y: 0, color: RED, tolerance: 0, contiguous: true, diagonal: false }).unwrap();
    assert!(!applied.changed);
}

#[test]
fn shapes_are_drawn_between_their_corners() {
    let (mut c, id) = canvas(12, 12);
    c.apply(&Op::Shape { layer: id, shape: ShapeKind::Rect, from: [8, 9], to: [2, 3], filled: false, tip: tip(1), color: RED }).unwrap();
    assert_eq!(painted(&c, id).len(), 2 * 7 + 2 * 5, "a rectangle's edge, from either corner");
    let (mut f, id) = canvas(12, 12);
    f.apply(&Op::Shape { layer: id, shape: ShapeKind::Rect, from: [2, 3], to: [8, 9], filled: true, tip: tip(1), color: RED }).unwrap();
    assert_eq!(painted(&f, id).len(), 49);
    let (mut e, id) = canvas(16, 16);
    e.apply(&Op::Shape { layer: id, shape: ShapeKind::Ellipse, from: [1, 1], to: [12, 8], filled: false, tip: tip(1), color: RED }).unwrap();
    let px = painted(&e, id);
    assert!(px.iter().all(|&(x, y)| (1..=12).contains(&x) && (1..=8).contains(&y)), "the ellipse left its box");
    assert!(px.contains(&(1, 4)) || px.contains(&(1, 5)), "it does not reach its box's side");
    let (mut l, id) = canvas(16, 16);
    l.apply(&Op::Shape { layer: id, shape: ShapeKind::Line, from: [0, 0], to: [9, 3], filled: false, tip: tip(1), color: RED }).unwrap();
    assert_eq!(painted(&l, id).len(), 10);
}

#[test]
fn a_colour_replaced_everywhere_with_its_swatch_is_one_edit() {
    let (mut c, id) = canvas(130, 10);
    let mut history = History::new(Preset::Pixel);
    history.apply(&mut c, &Op::SetPalette { colors: vec![BLUE, RED] }, "palette").unwrap();
    history.apply(&mut c, &Op::FillRect { layer: id, rect: [0, 0, 130, 4], color: BLUE }, "fill").unwrap();
    let ink = history.apply(&mut c, &Op::AddLayer { name: "Ink".into(), at: None }, "add").unwrap().made.unwrap();
    history.apply(&mut c, &Op::FillRect { layer: ink, rect: [100, 5, 4, 4], color: BLUE }, "fill").unwrap();
    let before = c.hash();
    let green = [40, 200, 90, 255];
    let applied = history.apply(&mut c, &Op::ReplaceColor { from: BLUE, to: green, layer: None, swatch: Some(0) }, "recolour").unwrap();
    assert!(applied.meta && applied.touched.len() == 4, "{applied:?}");
    assert_eq!(c.palette[0], green);
    assert_eq!(c.layer(ink).unwrap().grid().pixel(101, 6), green);
    assert_eq!(c.layer(id).unwrap().grid().pixel(129, 0), green);
    history.undo(&mut c);
    assert_eq!(c.hash(), before, "one undo did not take the whole recolour back");
    assert!(c.apply(&Op::ReplaceColor { from: BLUE, to: green, layer: None, swatch: Some(9) }).is_err());
}
