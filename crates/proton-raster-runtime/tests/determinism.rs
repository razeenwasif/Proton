//! One canvas, two builds: the ops the window makes in WebAssembly, made here
//! natively, must touch tiles that hash the same and lay together into the
//! same composite. This writes the fixture the node test runs through
//! `ui/raster-runtime.wasm` (`PROTON_BLESS=1`) and otherwise checks it.

use proton_raster::filter::Filter;
use proton_raster::gen::{noise, sprite, Generator, Mix};
use proton_raster::{Area, Blend, Brush, Mode, Op, ShapeKind, Tip, Tool};
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
    // Brushes: hard and soft, pressure on size, flow and opacity, an eraser.
    let brush = |size, hardness, flow, opacity, pressure| Brush { size, hardness, spacing: 9, flow, opacity, size_pressure: pressure, flow_pressure: pressure / 2, opacity_pressure: pressure / 3 };
    out.push(Op::Brush { layer: 1, erase: false, brush: brush(14, 90, 255, 255, 80), color: [240, 30, 90, 255], points: zigzag(4, 20, 30, 3) });
    out.push(Op::Brush { layer: 2, erase: false, brush: brush(40, 0, 50, 200, 60), color: [30, 200, 240, 220], points: zigzag(10, 40, 12, 8) });
    out.push(Op::Brush { layer: 1, erase: true, brush: brush(22, 40, 160, 255, 30), color: [0, 0, 0, 255], points: zigzag(50, 5, 20, 2) });
    out.push(Op::Brush { layer: 6, erase: false, brush: brush(3, 100, 255, 255, 100), color: [255, 255, 255, 255], points: zigzag(60, 50, 25, 1) });
    // Selections, and edits held to them; copied, pasted, moved and turned.
    let sel = |area, mode, feather, smooth| Op::Select { area, mode, feather, smooth };
    out.push(sel(Area::Ellipse { rect: [10, 5, 60, 50] }, Mode::Replace, 0, true));
    out.push(sel(Area::Lasso { points: [20, 10, 90, 20, 50, 65, 5, 40].map(|v| v * 256 + 77).to_vec() }, Mode::Subtract, 3, true));
    out.push(sel(Area::Rect { rect: [60, 30, 30, 30] }, Mode::Add, 0, false));
    out.push(Op::FillRect { layer: 2, rect: [0, 0, 100, 70], color: [200, 30, 160, 230] });
    out.push(Op::Brush { layer: 1, erase: false, brush: brush(18, 50, 200, 255, 50), color: [10, 250, 40, 255], points: zigzag(2, 30, 30, 3) });
    out.push(Op::Copy { layer: 1, cut: true });
    out.push(Op::Paste { above: Some(1) });
    out.push(sel(Area::Wand { layer: 2, x: 70, y: 40, tolerance: 20, contiguous: true, diagonal: true }, Mode::Intersect, 0, false));
    let one = 65536i64;
    out.push(Op::Transform { layer: 2, bounds: [0, 0, 100, 70], inverse: [58000, 21000, -9 * one, -21000, 58000, 15 * one], smooth: true });
    out.push(sel(Area::Invert, Mode::Replace, 0, false));
    out.push(Op::Clear { layer: 4 });
    out.push(sel(Area::Nothing, Mode::Replace, 0, false));
    out.push(Op::Orient { turns: 2, flip: true });
    // Generators and filters, from seeds.
    let ramp = proton_raster::gen::color::ramp([60, 140, 200, 255], 5, 30, 26000);
    out.push(Op::Generate { layer: 4, generator: Generator::Noise { noise: noise::Kind::Gradient, seed: 9, cell: 20, octaves: 3, colors: ramp.clone(), mix: Mix::Dither, dither: 4, rect: Some([5, 5, 60, 40]) } });
    out.push(Op::Generate { layer: 4, generator: Generator::Noise { noise: noise::Kind::Value, seed: 10, cell: 9, octaves: 2, colors: ramp, mix: Mix::Smooth, dither: 4, rect: Some([50, 30, 50, 40]) } });
    out.push(Op::Generate { layer: 6, generator: Generator::Cave { seed: 11, density: 45, steps: 4, cell: 2, wrap: true, color: [40, 30, 50, 255], floor: Some([200, 190, 170, 255]), rect: Some([0, 40, 50, 30]) } });
    out.push(Op::Generate { layer: 2, generator: Generator::Sprite { template: sprite::Kind::Creature, seed: 12, at: [70, 5], scale: 2, color: None } });
    out.push(Op::Filter { layer: 2, filter: Filter::Outline { color: [0, 0, 0, 255], diagonal: true, inside: false } });
    out.push(Op::Filter { layer: 4, filter: Filter::Shadow { dx: 3, dy: -2, color: [10, 0, 30, 140], blur: 4 } });
    out.push(Op::Filter { layer: 6, filter: Filter::Pixelate { size: 3 } });
    out.push(Op::Filter { layer: 1, filter: Filter::Quantize { colors: 6, palette: false, dither: 8, keep: true } });
    out
}

/// A stroke's points: a zigzag of `n` steps from `x`, `y`, `dx` across a step,
/// a fraction of a pixel off each time, at a pressure that rises.
fn zigzag(x: i32, y: i32, n: i32, dx: i32) -> Vec<i32> {
    (0..n).flat_map(|i| [(x + i * dx) * 256 + (i * 37) % 256, (y + if i % 2 == 0 { 0 } else { 3 + i % 4 }) * 256 + (i * 91) % 256, (i * 1200).min(65535)]).collect()
}

/// The layers of a canvas laid together as the window's composite is: every
/// tile, premultiplied.
fn composite_of(c: &proton_raster::Canvas) -> Vec<u8> {
    let grid = proton_raster::Grid::new(c.width, c.height);
    let mut out = vec![0u8; (c.width * c.height * 4) as usize];
    for index in 0..grid.cols * grid.rows {
        let (x, y, w, h) = grid.bounds(index);
        let px = proton_raster::blend::premultiplied(&proton_raster::blend::tile(c, index));
        for row in 0..h {
            let from = (row * proton_raster::TILE * 4) as usize;
            let to = (((y + row) * c.width + x) * 4) as usize;
            out[to..to + (w * 4) as usize].copy_from_slice(&px[from..from + (w * 4) as usize]);
        }
    }
    out
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
        // Every fourth round under a soft selection, which the stroke keeps to.
        let selected = (round % 4 == 2).then(|| Op::Select { area: Area::Ellipse { rect: [5, 3, w as i32 - 10, h as i32 - 6] }, mode: Mode::Replace, feather: 2, smooth: true });
        let tool = if round % 3 == 0 { Tool::Eraser } else { Tool::Pencil };
        // Every other round a brush, soft or hard, its size and strength on
        // the pressure.
        let brush = (round % 2 == 1).then(|| Brush {
            size: 1 + rng.next() % 60,
            hardness: (rng.next() % 101) as u8,
            spacing: 1 + (rng.next() % 40) as u8,
            flow: 1 + (rng.next() % 255) as u8,
            opacity: 1 + (rng.next() % 255) as u8,
            size_pressure: (rng.next() % 101) as u8,
            flow_pressure: (rng.next() % 101) as u8,
            opacity_pressure: (rng.next() % 101) as u8,
        });
        let tip = Tip { size: 1 + rng.next() % 6, round: rng.next() % 2 == 0, mirror: [(rng.next() % 2 == 0).then_some(w as i32), (rng.next() % 3 == 0).then_some(h as i32 - 1)] };
        let perfect = rng.next() % 2 == 0;
        let n = 1 + rng.next() % 60;
        let points: Vec<i32> = (0..n).flat_map(|_| [(rng.next() % (w * 256 + 2000)) as i32 - 1000, (rng.next() % (h * 256 + 2000)) as i32 - 1000, (rng.next() % 65536) as i32]).collect();
        let color = [(rng.next() % 256) as u8, 30, 200, 128 + (rng.next() % 128) as u8];
        let whole = match &brush {
            Some(b) => Op::Brush { layer: 1, erase: tool == Tool::Eraser, brush: b.clone(), color, points: points.clone() },
            None => Op::Stroke { layer: 1, tool, tip: tip.clone(), color, pixel_perfect: perfect, points: points.clone() },
        };

        // Made whole, natively.
        let mut core = proton_raster::Canvas::new(w, h, proton_raster::Preset::Paint).unwrap();
        core.layers[0].info.alpha_lock = round % 5 == 4;
        core.apply(&ground).unwrap();
        if let Some(s) = &selected {
            core.apply(s).unwrap();
        }
        let (applied, _) = core.apply(&whole).unwrap();
        let want = core.hash_touched(&applied.touched);

        // Drawn in pieces of any size, as the window draws it.
        proton_raster_runtime::open(opening.to_string().as_bytes()).unwrap();
        proton_raster_runtime::apply(serde_json::to_string(&ground).unwrap().as_bytes()).unwrap();
        if let Some(s) = &selected {
            proton_raster_runtime::apply(serde_json::to_string(s).unwrap().as_bytes()).unwrap();
        }
        let begin = match &brush {
            Some(b) => serde_json::json!({"layer": 1, "tool": tool, "brush": b, "color": color}),
            None => serde_json::json!({"layer": 1, "tool": tool, "tip": tip, "color": color, "pixelPerfect": perfect}),
        };
        proton_raster_runtime::stroke_begin(begin.to_string().as_bytes()).unwrap();
        let mut at = 0;
        while at < points.len() {
            let take = 3 * (1 + rng.next() as usize % 7);
            let end = (at + take).min(points.len());
            proton_raster_runtime::stroke_add(&points[at..end]).unwrap();
            // A frame: what changed laid together, as the window does.
            proton_raster_runtime::dirty();
            at = end;
        }
        let op = proton_raster_runtime::stroke_end().unwrap();
        assert_eq!(op, whole, "round {round}: the op given back is not the stroke drawn");
        assert_eq!(proton_raster_runtime::last_hash(), want, "round {round}: the stroke drawn in pieces touched different pixels");
        assert_eq!(proton_raster_runtime::canvas_hash(), core.hash(), "round {round}: the canvas drawn in pieces differs from the one made whole");
        // And what the window shows, laid together a part at a time, is the
        // whole canvas laid together at once.
        proton_raster_runtime::dirty();
        assert!(proton_raster_runtime::composite() == composite_of(&core), "round {round}: the picture laid together in parts differs");
    }
}
