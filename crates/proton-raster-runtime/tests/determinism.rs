//! One canvas, two builds: the ops the window makes in WebAssembly, made here
//! natively, must touch tiles that hash the same and lay together into the
//! same composite. This writes the fixture the node test runs through
//! `ui/raster-runtime.wasm` (`PROTON_BLESS=1`) and otherwise checks it.

use proton_raster::{Blend, Op};
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
