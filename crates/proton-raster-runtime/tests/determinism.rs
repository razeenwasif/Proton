//! One canvas, two builds: the ops the window makes in WebAssembly, made here
//! natively, must touch tiles that hash the same and lay together into the
//! same composite. This writes the fixture the node test runs through
//! `ui/raster-runtime.wasm` (`PROTON_BLESS=1`) and otherwise checks it.

use proton_raster::{Blend, Op, ShapeKind, Tip, Tool};
use serde_json::{json, Value};

fn ops() -> Vec<Op> {
    let mut out = vec![
        Op::FillRect { layer: 1, rect: [0, 0, 100, 70], color: [40, 60, 90, 255] },
        Op::FillRect { layer: 1, rect: [10, 5, 30, 50], color: [220, 180, 40, 200] },
        Op::SetPalette { colors: vec![[26, 28, 44, 255], [93, 39, 93, 255]] },
    ];
    for (i, mode) in Blend::ALL.into_iter().enumerate() {
        let id = i as u32 + 2;
        let i = i as i32;
        out.push(Op::AddLayer { name: format!("{mode:?}"), at: None });
        out.push(Op::FillRect { layer: id, rect: [i * 6, (i * 4) % 60, 22, 16], color: [(i * 37 % 256) as u8, (200 - i * 11) as u8, (i * 53 % 256) as u8, 150 + (i as u8) * 7] });
        out.push(Op::SetLayer { layer: id, name: None, opacity: Some(160 + (i as u8) * 6), blend: Some(mode), hidden: None, locked: None, alpha_lock: None });
    }
    out.push(Op::MoveLayer { layer: 3, to: 9 });
    out.push(Op::SetLayer { layer: 5, name: None, opacity: None, blend: None, hidden: Some(true), locked: None, alpha_lock: None });
    out.push(Op::RemoveLayer { layer: 7 });
    // The pixel tools.
    let tip = |size, round, mirror| Tip { size, round, mirror };
    out.push(Op::Stroke { layer: 1, tool: Tool::Pencil, tip: tip(1, false, [Some(100), None]), color: [250, 240, 230, 255], pixel_perfect: true, points: zigzag(5, 60, 40, 7) });
    out.push(Op::Stroke { layer: 4, tool: Tool::Eraser, tip: tip(5, true, [None, Some(70)]), color: [0, 0, 0, 255], pixel_perfect: false, points: zigzag(20, 15, 50, 3) });
    out.push(Op::Stroke { layer: 6, tool: Tool::Pencil, tip: tip(3, true, [None, None]), color: [10, 200, 120, 150], pixel_perfect: false, points: zigzag(70, 10, 25, 11) });
    out.push(Op::Fill { layer: 1, x: 50, y: 35, color: [90, 10, 140, 255], tolerance: 12, contiguous: true, diagonal: true });
    out.push(Op::Fill { layer: 4, x: 1, y: 1, color: [255, 255, 0, 255], tolerance: 0, contiguous: false, diagonal: false });
    out.push(Op::Shape { layer: 2, shape: ShapeKind::Line, from: [3, 66], to: [96, 2], filled: false, tip: tip(2, false, [None, None]), color: [0, 0, 0, 255] });
    out.push(Op::Shape { layer: 2, shape: ShapeKind::Rect, from: [60, 40], to: [80, 55], filled: true, tip: tip(1, false, [None, None]), color: [255, 128, 0, 200] });
    out.push(Op::Shape { layer: 2, shape: ShapeKind::Ellipse, from: [10, 30], to: [45, 60], filled: false, tip: tip(1, false, [Some(100), None]), color: [255, 255, 255, 255] });
    out.push(Op::ReplaceColor { from: [40, 60, 90, 255], to: [41, 61, 91, 255], layer: None, swatch: None });
    out
}

/// A stroke's points: a zigzag of `n` steps from `x`, `y`, `dx` across a step,
/// a fraction of a pixel off each time, at a pressure that rises.
fn zigzag(x: i32, y: i32, n: i32, dx: i32) -> Vec<i32> {
    (0..n).flat_map(|i| [(x + i * dx) * 256 + (i * 37) % 256, (y + if i % 2 == 0 { 0 } else { 3 + i % 4 }) * 256 + (i * 91) % 256, (i * 1200).min(65535)]).collect()
}

fn hex(h: u64) -> String {
    format!("{h:016x}")
}

fn hash(bytes: &[u8]) -> String {
    let mut h = proton_raster::fixed::Hash::default();
    h.bytes(bytes);
    h.hex()
}

#[test]
fn the_window_and_the_core_make_the_same_canvas() {
    let opening = json!({"width": 100, "height": 70, "preset": "paint", "palette": [], "generation": 1,
        "layers": [{"id": 1, "name": "Background", "opacity": 255, "blend": "normal", "hidden": false, "locked": false, "alphaLock": false}]});
    proton_raster_runtime::open(opening.to_string().as_bytes()).unwrap();
    let mut hashes = Vec::new();
    for op in ops() {
        hashes.push(hex(proton_raster_runtime::apply(serde_json::to_string(&op).unwrap().as_bytes()).unwrap()));
    }
    let rects = proton_raster_runtime::dirty();
    assert_eq!(rects[0], 4, "a 100 × 70 canvas is four tiles");
    let composite = hash(&proton_raster_runtime::composite());
    // And the core's own canvas, made by the same ops, flattens alike.
    let mut core = proton_raster::Canvas::new(100, 70, proton_raster::Preset::Paint).unwrap();
    for op in ops() {
        core.apply(&op).unwrap();
    }
    let flat = hash(&proton_raster::blend::flatten(&core));
    let fixture = json!({"open": opening, "ops": ops(), "hashes": hashes, "composite": composite, "flat": flat,
        "pick": [[15, 20, proton_raster_runtime::pick(15, 20)], [70, 40, proton_raster_runtime::pick(70, 40)]]});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/ui/fixtures/raster-ops.json");
    if std::env::var_os("PROTON_BLESS").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap() + "\n").unwrap();
    }
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture: run with PROTON_BLESS=1 once")).unwrap();
    assert_eq!(kept, fixture, "the canvas the ops make changed: bless it again only if that was meant");
}

/// A small seeded generator, the same every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

#[test]
fn a_stroke_drawn_in_pieces_is_the_stroke_made_whole() {
    let mut rng = Lcg(99);
    for round in 0..40 {
        let (w, h) = (40 + rng.next() % 150, 30 + rng.next() % 120);
        let layer = serde_json::json!({"id": 1, "name": "Background", "opacity": 255, "blend": "normal", "hidden": false, "locked": false, "alphaLock": round % 5 == 4});
        let opening = serde_json::json!({"width": w, "height": h, "preset": "paint", "layers": [layer], "generation": 1});
        // Something under it, so the eraser and alpha-lock have work.
        let ground = Op::FillRect { layer: 1, rect: [0, 0, w as i32 / 2, h as i32], color: [9, 99, 199, 200] };
        let tool = if round % 3 == 0 { Tool::Eraser } else { Tool::Pencil };
        let tip = Tip { size: 1 + rng.next() % 6, round: rng.next() % 2 == 0, mirror: [(rng.next() % 2 == 0).then_some(w as i32), (rng.next() % 3 == 0).then_some(h as i32 - 1)] };
        let perfect = rng.next() % 2 == 0;
        let n = 1 + rng.next() % 60;
        let points: Vec<i32> = (0..n).flat_map(|_| [(rng.next() % (w * 256 + 2000)) as i32 - 1000, (rng.next() % (h * 256 + 2000)) as i32 - 1000, (rng.next() % 65536) as i32]).collect();
        let color = [(rng.next() % 256) as u8, 30, 200, 128 + (rng.next() % 128) as u8];
        let whole = Op::Stroke { layer: 1, tool, tip: tip.clone(), color, pixel_perfect: perfect, points: points.clone() };

        // Made whole, natively.
        let mut core = proton_raster::Canvas::new(w, h, proton_raster::Preset::Paint).unwrap();
        core.layers[0].info.alpha_lock = round % 5 == 4;
        core.apply(&ground).unwrap();
        let (applied, _) = core.apply(&whole).unwrap();
        let want = core.hash_touched(&applied.touched);

        // Drawn in pieces of any size, as the window draws it.
        proton_raster_runtime::open(opening.to_string().as_bytes()).unwrap();
        proton_raster_runtime::apply(serde_json::to_string(&ground).unwrap().as_bytes()).unwrap();
        let begin = serde_json::json!({"layer": 1, "tool": tool, "tip": tip, "color": color, "pixelPerfect": perfect});
        proton_raster_runtime::stroke_begin(begin.to_string().as_bytes()).unwrap();
        let mut at = 0;
        while at < points.len() {
            let take = 3 * (1 + rng.next() as usize % 7);
            let end = (at + take).min(points.len());
            proton_raster_runtime::stroke_add(&points[at..end]).unwrap();
            at = end;
        }
        let op = proton_raster_runtime::stroke_end().unwrap();
        assert_eq!(op, whole, "round {round}: the op given back is not the stroke drawn");
        assert_eq!(proton_raster_runtime::last_hash(), want, "round {round}: the stroke drawn in pieces touched different pixels");
        assert_eq!(proton_raster_runtime::canvas_hash(), core.hash(), "round {round}: the canvas drawn in pieces differs from the one made whole");
    }
}
