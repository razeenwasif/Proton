//! A canvas out and in: a layered PSD comes back through the reader as the
//! canvas it was — names, opacity, hidden, blend, every pixel where it was;
//! a PNG is the canvas flattened, larger by whole pixels if asked; other
//! files are refused; and layers brought in are one step of undo.

use proton_raster::port::{self, Part, Picture};
use proton_raster::psd_write::{self, Entry, Layer};
use proton_raster::{blend, Blend, Canvas, History, Op, Preset};

fn canvas() -> Canvas {
    let mut c = Canvas::new(70, 40, Preset::Paint).unwrap();
    let ground = c.layers[0].info.id;
    c.apply(&Op::FillRect { layer: ground, rect: [0, 0, 70, 40], color: [30, 60, 90, 255] }).unwrap();
    c.apply(&Op::SetLayer { layer: ground, name: Some("Sky [still]".into()), opacity: None, blend: None, hidden: None, locked: None, alpha_lock: None }).unwrap();
    let hills = c.push_layer("Hills — far".into());
    c.apply(&Op::FillRect { layer: hills, rect: [10, 20, 30, 15], color: [40, 140, 70, 200] }).unwrap();
    c.apply(&Op::SetLayer { layer: hills, name: None, opacity: Some(180), blend: Some(Blend::Multiply), hidden: None, locked: None, alpha_lock: None }).unwrap();
    let ghost = c.push_layer("Ghost".into());
    c.apply(&Op::FillRect { layer: ghost, rect: [50, 5, 8, 8], color: [250, 250, 250, 255] }).unwrap();
    c.apply(&Op::SetLayer { layer: ghost, name: None, opacity: None, blend: Some(Blend::Screen), hidden: Some(true), locked: None, alpha_lock: None }).unwrap();
    c.push_layer("Empty".into());
    c
}

#[test]
fn a_layered_psd_comes_back_as_the_canvas_it_was() {
    let c = canvas();
    let bytes = port::psd(&c);
    // Four channels, and a negative layer count: the merged image carries
    // its transparency.
    assert_eq!(u16::from_be_bytes([bytes[12], bytes[13]]), 4);
    let picture = port::read(&bytes, "unused").unwrap();
    assert!(picture.notes.is_empty(), "{:?}", picture.notes);
    let back = port::canvas_of(&picture, Preset::Paint).unwrap();
    assert_eq!((back.width, back.height, back.layers.len()), (70, 40, 4));
    for (a, b) in c.layers.iter().zip(&back.layers) {
        assert_eq!((&a.info.name, a.info.opacity, a.info.hidden, a.info.blend), (&b.info.name, b.info.opacity, b.info.hidden, b.info.blend));
        for y in 0..40 {
            for x in 0..70 {
                assert_eq!(a.grid.pixel(x, y), b.grid.pixel(x, y), "{} at {x}, {y}", a.info.name);
            }
        }
    }
    assert_eq!(blend::flatten(&back), blend::flatten(&c));
}

#[test]
fn a_png_is_the_canvas_flattened_and_larger_by_whole_pixels() {
    let c = canvas();
    let flat = blend::flatten(&c);
    let (w, h, rgba) = proton_raster::file::decode_png(&port::png(&c, 1).unwrap()).unwrap();
    assert_eq!((w, h), (70, 40));
    assert_eq!(rgba, flat);
    let (w, h, big) = proton_raster::file::decode_png(&port::png(&c, 4).unwrap()).unwrap();
    assert_eq!((w, h), (280, 160));
    for (x, y) in [(0, 0), (15, 25), (69, 39), (52, 7)] {
        for (dx, dy) in [(0, 0), (3, 3), (1, 2)] {
            let at = (((y * 4 + dy) * 280 + x * 4 + dx) * 4) as usize;
            let from = ((y * 70 + x) * 4) as usize;
            assert_eq!(&big[at..at + 4], &flat[from..from + 4], "{x}, {y}");
        }
    }
    // Read back in, a PNG is one layer named as asked.
    let picture = port::read(&port::png(&c, 1).unwrap(), "Flat").unwrap();
    assert_eq!((picture.parts.len(), picture.parts[0].name.as_str()), (1, "Flat"));
}

#[test]
fn other_files_are_refused_and_a_blend_canvas_lacks_is_said() {
    assert!(port::read(b"GIF89a....", "x").unwrap_err().contains("only PNG and Photoshop"));
    let mut layer = Layer::filled("Glow", [0, 0], 2, 2, [255, 200, 0, 255]);
    layer.blend = *b"vLit";
    let bytes = psd_write::write(4, 4, &[Entry::Layer(layer), Entry::group("Group", vec![Entry::Layer(Layer::filled("Inside", [1, 1], 2, 2, [0, 0, 0, 255]))])], psd_write::Header::default(), None);
    let picture = port::read(&bytes, "x").unwrap();
    assert!(picture.notes.iter().any(|n| n.contains("Glow") && n.contains("vLit")), "{:?}", picture.notes);
    assert!(picture.parts.iter().any(|p| p.name == "Group / Inside"), "a layer in a group keeps the group in its name");
}

#[test]
fn layers_brought_in_are_one_step_of_undo() {
    let mut c = canvas();
    let mut h = History::new(Preset::Paint);
    let before = c.hash();
    let part = Part { name: "Pasted".into(), left: 0, top: 0, width: 3, height: 2, rgba: [9u8, 9, 9, 255].repeat(6), opacity: 255, visible: true, blend: Blend::Normal };
    let picture = Picture { width: 3, height: 2, parts: vec![part.clone(), Part { name: "Second".into(), ..part.clone() }], notes: Vec::new() };
    let above = c.layers[1].info.id;
    let grids = picture.parts.iter().map(|p| (p.name.clone(), p.opacity, !p.visible, p.blend, port::grid_of(p, 70, 40, [68, 39]))).collect();
    let applied = h.record(&mut c, "paste", |c| c.bring_in(grids, Some(above))).unwrap();
    assert!(applied.meta && applied.made.is_some());
    assert_eq!(c.layers.iter().map(|l| l.info.name.as_str()).collect::<Vec<_>>(), ["Sky [still]", "Hills — far", "Pasted", "Second", "Ghost", "Empty"]);
    // Placed where asked, cut at the canvas's edge.
    assert_eq!((c.layers[2].grid.pixel(69, 39), c.layers[2].grid.pixel(67, 39)), ([9, 9, 9, 255], [0; 4]));
    h.undo(&mut c);
    assert_eq!(c.hash(), before, "one undo did not take both layers back");
    assert!(c.bring_in(Vec::new(), None).is_err());
}
