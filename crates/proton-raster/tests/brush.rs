//! Brushes: dabs that fall where they fall however the points are broken
//! up, a size and a strength from the pen's pressure, a soft eraser, and a
//! brush stroke as one edit.

use proton_raster::{Brush, Canvas, Dabs, History, Op, Preset};

const INK: [u8; 4] = [30, 40, 160, 255];

fn brush() -> Brush {
    Brush { size: 24, hardness: 60, spacing: 8, flow: 180, opacity: 230, size_pressure: 80, flow_pressure: 40, opacity_pressure: 20 }
}

/// A seeded walk of `n` points: a hand's wobble, its pressure rising and
/// falling.
fn walk(seed: u64, n: usize, side: i32) -> Vec<i32> {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut next = |m: i32| {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((s >> 33) % m as u64) as i32
    };
    let (mut x, mut y, mut p) = (side * 128, side * 128, 30000);
    let mut out = Vec::new();
    for _ in 0..n {
        x = (x + next(2400) - 1200).clamp(-2000, side * 256 + 2000);
        y = (y + next(2400) - 1200).clamp(-2000, side * 256 + 2000);
        p = (p + next(12000) - 6000).clamp(0, 65535);
        out.extend([x, y, p]);
    }
    out
}

#[test]
fn a_brush_stroke_drawn_in_pieces_is_the_stroke_made_whole() {
    for seed in 0..40u64 {
        let points = walk(seed, 60, 200);
        let mut whole = Dabs::new(brush(), 200, 200);
        whole.add(&points);
        let mut pieces = Dabs::new(brush(), 200, 200);
        let mut at = 0;
        let mut cut = seed as usize;
        while at < points.len() / 3 {
            cut = (cut * 7 + 3) % 5;
            let to = (at + cut + 1).min(points.len() / 3);
            pieces.add(&points[at * 3..to * 3]);
            at = to;
        }
        assert_eq!(whole.mask, pieces.mask, "seed {seed}");
    }
}

/// How many pixels of a row a layer has anything in.
fn across(c: &Canvas, layer: u32, y: u32) -> u32 {
    (0..c.width).filter(|&x| c.layer(layer).unwrap().grid.pixel(x, y)[3] > 0).count() as u32
}

/// How many pixels of a column.
fn down(c: &Canvas, layer: u32, x: u32) -> u32 {
    (0..c.height).filter(|&y| c.layer(layer).unwrap().grid.pixel(x, y)[3] > 0).count() as u32
}

#[test]
fn pressure_makes_a_line_thinner_and_lighter() {
    let mut c = Canvas::new(200, 100, Preset::Paint).unwrap();
    let id = c.layers[0].info.id;
    // Left to right, pressing harder: thin at the start, full at the end.
    let points: Vec<i32> = (0..=20).flat_map(|i| [(10 + i * 9) * 256, 50 * 256, i * 65535 / 20]).collect();
    c.apply(&Op::Brush { layer: id, erase: false, brush: brush(), color: INK, points }).unwrap();
    let (thin, thick) = (down(&c, id, 40), down(&c, id, 180));
    assert!(thin < thick && thick >= 22, "{thin} then {thick}");
    let (light, dark) = (c.layer(id).unwrap().grid.pixel(40, 50)[3], c.layer(id).unwrap().grid.pixel(180, 50)[3]);
    assert!(light < dark, "{light} then {dark}");
    // No pressure, and a brush whose size hangs wholly on it, lays nothing.
    let mut none = Canvas::new(64, 64, Preset::Paint).unwrap();
    let id = none.layers[0].info.id;
    let all = Brush { size_pressure: 100, ..brush() };
    let (applied, _) = none.apply(&Op::Brush { layer: id, erase: false, brush: all, color: INK, points: vec![2560, 2560, 0, 12800, 2560, 0] }).unwrap();
    assert!(!applied.changed);
}

#[test]
fn a_soft_eraser_takes_away_as_much_as_it_would_cover() {
    let mut c = Canvas::new(64, 64, Preset::Paint).unwrap();
    let id = c.layers[0].info.id;
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 64, 64], color: [200, 50, 50, 255] }).unwrap();
    let hard = Brush { size: 10, hardness: 100, spacing: 10, flow: 255, opacity: 128, size_pressure: 0, flow_pressure: 0, opacity_pressure: 0 };
    c.apply(&Op::Brush { layer: id, erase: true, brush: hard.clone(), color: INK, points: vec![32 * 256, 32 * 256, 65535] }).unwrap();
    assert_eq!(c.layer(id).unwrap().grid.pixel(32, 32), [200, 50, 50, 127]);
    assert_eq!(c.layer(id).unwrap().grid.pixel(2, 2), [200, 50, 50, 255]);
    // Alpha-locked, the brush only recolours what is there.
    let mut l = Canvas::new(64, 64, Preset::Paint).unwrap();
    let id = l.layers[0].info.id;
    l.apply(&Op::FillRect { layer: id, rect: [0, 0, 32, 64], color: [200, 50, 50, 255] }).unwrap();
    l.apply(&Op::SetLayer { layer: id, name: None, opacity: None, blend: None, hidden: None, locked: None, alpha_lock: Some(true) }).unwrap();
    let solid = Brush { opacity: 255, size: 20, ..hard };
    l.apply(&Op::Brush { layer: id, erase: false, brush: solid, color: INK, points: vec![32 * 256, 32 * 256, 65535] }).unwrap();
    assert_eq!(l.layer(id).unwrap().grid.pixel(28, 32), INK);
    assert_eq!(l.layer(id).unwrap().grid.pixel(36, 32), [0, 0, 0, 0]);
    assert_eq!(across(&l, id, 32), 32);
}

#[test]
fn a_brush_stroke_is_one_edit_and_a_wrong_brush_is_refused() {
    let mut c = Canvas::new(300, 300, Preset::Paint).unwrap();
    let id = c.layers[0].info.id;
    let mut history = History::new(Preset::Paint);
    let before = c.hash();
    let applied = history.apply(&mut c, &Op::Brush { layer: id, erase: false, brush: brush(), color: INK, points: walk(7, 80, 300) }, "paint").unwrap();
    assert!(applied.changed && !applied.touched.is_empty(), "{applied:?}");
    history.undo(&mut c);
    assert_eq!(c.hash(), before);
    for wrong in [Brush { size: 0, ..brush() }, Brush { size: 1001, ..brush() }, Brush { spacing: 0, ..brush() }, Brush { hardness: 101, ..brush() }, Brush { flow: 0, ..brush() }] {
        assert!(c.apply(&Op::Brush { layer: id, erase: false, brush: wrong, color: INK, points: vec![0, 0, 65535] }).is_err());
    }
    assert!(c.apply(&Op::Brush { layer: id, erase: false, brush: brush(), color: INK, points: vec![0, 0] }).is_err(), "points come in threes");
}

/// The pixels one pressure stroke makes, as a number: a change to how
/// brushes lay dabs changes it, and must be meant. `PROTON_BLESS=1` prints
/// the new one.
#[test]
fn a_pressure_stroke_makes_the_pixels_it_always_made() {
    let mut c = Canvas::new(256, 256, Preset::Paint).unwrap();
    let id = c.layers[0].info.id;
    c.apply(&Op::FillRect { layer: id, rect: [0, 0, 256, 128], color: [240, 220, 180, 255] }).unwrap();
    c.apply(&Op::Brush { layer: id, erase: false, brush: brush(), color: INK, points: walk(3, 120, 256) }).unwrap();
    let soft = Brush { size: 60, hardness: 0, spacing: 12, flow: 40, opacity: 255, size_pressure: 0, flow_pressure: 80, opacity_pressure: 0 };
    c.apply(&Op::Brush { layer: id, erase: false, brush: soft.clone(), color: [250, 120, 40, 200], points: walk(4, 60, 256) }).unwrap();
    c.apply(&Op::Brush { layer: id, erase: true, brush: soft, color: INK, points: walk(5, 40, 256) }).unwrap();
    let hash = format!("{:016x}", c.hash());
    if std::env::var("PROTON_BLESS").is_ok() {
        println!("pressure stroke: {hash}");
        return;
    }
    assert_eq!(hash, "5ed6ad3ecaacaade");
}
