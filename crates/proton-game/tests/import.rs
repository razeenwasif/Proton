//! Art brought in: `.psd` files written to Adobe's description read back as
//! painted, groups made into planes at the depths their names give, what
//! cannot be reproduced refused or said, large layers cut with their empty
//! pieces dropped, and normal maps made from brightness.

use proton_game::format::{Blend, Image};
use proton_game::import::psd::{self, Psd};
use proton_game::import::psd_write::{write, Entry, Header, Layer};
use proton_game::import::{decode_png, encode_png, import, Importing, CHUNK};

/// A gradient no two pixels of which are alike, so a misplaced byte shows.
fn gradient(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            out.extend([(x * 7 + seed as u32) as u8, (y * 11) as u8, (x * y + seed as u32) as u8, (40 + x * 3 + y) as u8 | 1]);
        }
    }
    out
}

fn read(width: u32, height: u32, entries: &[Entry]) -> Psd {
    psd::read(&write(width, height, entries, Header::default(), None)).expect("the file reads")
}

#[test]
fn layers_read_back_as_painted_whether_raw_run_length_or_zipped() {
    let entries: Vec<Entry> = (0..4u16)
        .map(|c| Entry::Layer(Layer { compression: c, ..Layer::of(&format!("layer {c}"), [3 + c as i32 * 10, 5], 9, 6, gradient(9, 6, c as u8)) }))
        .collect();
    let doc = read(64, 32, &entries);
    assert_eq!((doc.width, doc.height), (64, 32));
    assert_eq!(doc.layers.len(), 4);
    // Bottom to top: the panel's last is the file's first.
    for (i, layer) in doc.layers.iter().enumerate() {
        let c = 3 - i;
        assert_eq!(layer.name, format!("layer {c}"));
        assert_eq!((layer.left, layer.top, layer.width, layer.height), (3 + c as i32 * 10, 5, 9, 6));
        assert_eq!(layer.rgba, gradient(9, 6, c as u8), "layer {c}'s pixels");
        assert!(layer.visible && layer.groups.is_empty());
    }
}

#[test]
fn a_name_is_its_unicode_one_and_long_runs_and_short_ones_both_unpack() {
    // Runs of 200 of a colour and then noise: both halves of PackBits.
    let mut rgba = Vec::new();
    for x in 0..300u32 {
        let v = if x < 200 { 90 } else { (x * 37 % 251) as u8 };
        rgba.extend([v, v / 2, 255 - v, 255]);
    }
    let doc = read(300, 1, &[Entry::Layer(Layer::of("Fougères — près", [0, 0], 300, 1, rgba.clone()))]);
    assert_eq!(doc.layers[0].name, "Fougères — près");
    assert_eq!(doc.layers[0].rgba, rgba);
}

#[test]
fn groups_are_read_top_down_and_a_hidden_group_hides_what_is_in_it() {
    let entries = vec![
        Entry::Layer(Layer::filled("Leaves [-150]", [0, 0], 4, 4, [0, 200, 0, 255])),
        Entry::group("Far hills [800]", vec![
            Entry::Layer(Layer::filled("near ridge", [0, 10], 8, 2, [90, 90, 120, 255])),
            Entry::group("trees", vec![Entry::Layer(Layer::filled("pine", [2, 8], 2, 2, [10, 60, 10, 255]))]),
        ]),
        Entry::Group { name: "Old".into(), hidden: true, opacity: 255, children: vec![Entry::Layer(Layer::filled("sketch", [0, 0], 2, 2, [0, 0, 0, 255]))] },
        Entry::Group { name: "Haze".into(), hidden: false, opacity: 128, children: vec![Entry::Layer(Layer { opacity: 200, ..Layer::filled("mist", [0, 0], 2, 2, [255, 255, 255, 255]) })] },
    ];
    let doc = read(16, 16, &entries);
    let by = |name: &str| doc.layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no layer {name}"));
    assert_eq!(by("pine").groups, ["Far hills [800]", "trees"]);
    assert_eq!(by("near ridge").groups, ["Far hills [800]"]);
    assert!(by("Leaves [-150]").groups.is_empty());
    assert!(!by("sketch").visible, "a layer in a hidden group was shown");
    assert!(by("pine").visible);
    // A group's opacity is its layers' too.
    assert_eq!(by("mist").opacity, (200u32 * 128 / 255) as u8);
    // Bottom to top, as drawn.
    let order: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(order, ["mist", "sketch", "pine", "near ridge", "Leaves [-150]"]);
}

#[test]
fn a_layer_mask_takes_away_what_it_hides() {
    // A 4×4 layer; the mask covers its left half, white, and is black outside.
    let mask = ([0, 0], 2, 4, 0u8, vec![255u8; 8]);
    let layer = Layer { mask: Some(mask), ..Layer::filled("masked", [0, 0], 4, 4, [200, 100, 50, 255]) };
    let doc = read(4, 4, &[Entry::Layer(layer)]);
    let alpha: Vec<u8> = doc.layers[0].rgba.chunks(4).map(|p| p[3]).collect();
    assert_eq!(alpha, [255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0]);
}

#[test]
fn what_cannot_be_reproduced_is_refused_with_what_to_do_or_said() {
    let one = [Entry::Layer(Layer::filled("a", [0, 0], 2, 2, [1, 2, 3, 255]))];
    let refused = |header: Header| psd::read(&write(4, 4, &one, header, None)).unwrap_err();
    assert!(refused(Header { mode: 4, ..Header::default() }).contains("CMYK"));
    assert!(refused(Header { depth: 16, ..Header::default() }).contains("8 bits a channel"));
    assert!(refused(Header { version: 2, ..Header::default() }).contains(".psb"));
    assert!(refused(Header { mode: 2, ..Header::default() }).contains("Indexed"));
    assert!(psd::read(b"GIF89a").unwrap_err().contains("not a Photoshop file"));
    let cut = write(4, 4, &one, Header::default(), None);
    assert!(psd::read(&cut[..cut.len() / 2]).is_err(), "half a file was read");

    let said = read(8, 8, &[
        Entry::Layer(Layer { tags: vec![*b"lfx2"], ..Layer::filled("glowing", [0, 0], 2, 2, [255, 0, 0, 255]) }),
        Entry::Layer(Layer { tags: vec![*b"SoLd"], ..Layer::filled("placed", [0, 0], 2, 2, [255, 0, 0, 255]) }),
        Entry::Layer(Layer { clipped: true, ..Layer::filled("shading", [0, 0], 2, 2, [0, 0, 0, 255]) }),
        Entry::Layer(Layer { tags: vec![*b"curv"], ..Layer::of("grade", [0, 0], 0, 0, Vec::new()) }),
    ]);
    let notes = said.notes.join("\n");
    assert!(notes.contains("glowing's layer effects"), "{notes}");
    assert!(notes.contains("placed is a smart object"), "{notes}");
    assert!(notes.contains("shading is clipped"), "{notes}");
    assert!(notes.contains("grade is a Curves layer"), "{notes}");
    assert!(!said.layers.iter().any(|l| l.name == "grade"), "an adjustment layer came out as pixels");
}

#[test]
fn a_file_saved_flat_is_its_merged_image() {
    let flat: Vec<u8> = (0..6u8).flat_map(|i| [i * 40, 255 - i * 40, 7]).collect();
    let doc = psd::read(&write(3, 2, &[], Header::default(), Some(&flat))).unwrap();
    assert_eq!(doc.layers.len(), 1);
    let rgb: Vec<u8> = doc.layers[0].rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    assert_eq!(rgb, flat);
}

#[test]
fn a_painted_room_comes_in_as_planes_at_the_depths_its_groups_name() {
    let entries = vec![
        Entry::Layer(Layer { blend: *b"scrn", ..Layer::filled("Glints [-40]", [100, 100], 10, 10, [255, 255, 200, 255]) }),
        Entry::group("Ground [play]", vec![Entry::Layer(Layer::filled("slab", [0, 150], 200, 50, [80, 60, 40, 255]))]),
        Entry::group("Far hills [800]", vec![
            Entry::Layer(Layer { opacity: 153, ..Layer::filled("ridge", [0, 60], 200, 40, [90, 90, 120, 255]) }),
            Entry::Layer(Layer { blend: *b"over", ..Layer::filled("trees", [20, 50], 30, 20, [20, 70, 20, 255]) }),
        ]),
        Entry::Layer(Layer { hidden: true, ..Layer::filled("sketch", [0, 0], 4, 4, [0, 0, 0, 255]) }),
        Entry::Layer(Layer::filled("Mist", [0, 0], 200, 30, [255, 255, 255, 60])),
        Entry::group("Sky [still] [3000]", vec![Entry::Layer(Layer::filled("gradient", [0, 0], 200, 200, [120, 150, 220, 255]))]),
    ];
    // A layer with a clear margin round it is trimmed to what is painted.
    let mut margin = vec![0u8; 20 * 20 * 4];
    for y in 5..9 {
        for x in 6..12 {
            margin[(y * 20 + x) * 4..][..4].copy_from_slice(&[10, 20, 30, 255]);
        }
    }
    let mut entries = entries;
    entries.insert(0, Entry::Layer(Layer::of("Lantern [-10]", [50, 40], 20, 20, margin)));
    let bytes = write(200, 200, &entries, Header::default(), None);
    let made = import("src/Forest Room.psd", &bytes, &Importing::default()).unwrap();

    let planes: Vec<(&str, f64)> = made.planes.iter().map(|p| (p.id.as_str(), p.depth)).collect();
    assert_eq!(planes, [("sky", 3000.0), ("mist", 0.0), ("far_hills", 800.0), ("glints", -40.0), ("lantern", -10.0)]);
    assert_eq!(made.planes[0].parallax, Some(0.0), "[still] did not keep the sky still");
    assert_eq!(made.planes[0].name, "Sky");
    // The play group's layers go to the room's own play plane.
    assert_eq!(made.play.len(), 1);
    assert_eq!(made.play[0].id, "forest_room_slab");
    assert_eq!(made.play[0].at, [0, 150]);
    let hills = &made.planes[2];
    // Back to front, as drawn: the panel lists the ridge over the trees.
    assert_eq!(hills.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["forest_room_trees", "forest_room_ridge"]);
    assert_eq!(hills.items[1].opacity, 0.6);
    assert_eq!(made.planes[3].items[0].blend, Blend::Screen);
    let lantern = &made.planes[4].items[0];
    assert_eq!(lantern.at, [56, 45], "the clear margin was not trimmed");
    let file = made.files.iter().find(|(f, _)| f == &lantern.image).expect("the lantern's image");
    let (w, h, rgba) = decode_png(&file.1).unwrap();
    assert_eq!((w, h), (6, 4));
    assert!(rgba.chunks(4).all(|p| p == [10, 20, 30, 255]));
    assert!(lantern.image.starts_with("art/forest_room/"), "{}", lantern.image);
    let notes = made.notes.join("\n");
    assert!(notes.contains("sketch is hidden"), "{notes}");
    assert!(notes.contains("trees blends by over"), "{notes}");
    assert!(notes.contains("Mist has no depth"), "{notes}");
}

#[test]
fn a_scale_places_and_sizes_what_comes_in() {
    let bytes = write(100, 100, &[Entry::Layer(Layer::filled("Rock [0]", [10, 20], 5, 5, [1, 1, 1, 255]))], Header::default(), None);
    let made = import("rock.psd", &bytes, &Importing { scale: 2.0, normals: false }).unwrap();
    let item = &made.planes[0].items[0];
    assert_eq!((item.at, item.scale), ([20, 40], 2.0));
    assert!(import("rock.psd", &bytes, &Importing { scale: 0.0, normals: false }).is_err());
}

#[test]
fn a_layer_larger_than_a_texture_is_cut_and_its_empty_pieces_dropped() {
    // Painted at both ends, clear between: the middle piece is not written.
    let (w, h) = (5000u32, 300u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in (0..900).chain(4500..w) {
            rgba[((y * w + x) * 4) as usize..][..4].copy_from_slice(&[x as u8, y as u8, 9, 255]);
        }
    }
    let bytes = write(w, h, &[Entry::Layer(Layer { compression: 1, ..Layer::of("Wide [200]", [0, 0], w, h, rgba) })], Header::default(), None);
    let made = import("wide.psd", &bytes, &Importing::default()).unwrap();
    assert_eq!(made.cut, 1);
    let item = &made.planes[0].items[0];
    assert_eq!(item.image, "art/wide/wide.img.json");
    let sidecar = made.files.iter().find(|(f, _)| f == &item.image).unwrap();
    let image: Image = serde_json::from_slice(&sidecar.1).unwrap();
    assert_eq!(image.size, [w, h]);
    let pieces: Vec<([u32; 2], [u32; 2])> = image.chunks.iter().map(|c| (c.at, c.size)).collect();
    assert_eq!(pieces, [([0, 0], [CHUNK, h]), ([2 * CHUNK, 0], [w - 2 * CHUNK, h])]);
    // Each piece is the image's own pixels there.
    let last = made.files.iter().find(|(f, _)| f == &image.chunks[1].file).unwrap();
    let (pw, _, piece) = decode_png(&last.1).unwrap();
    let x = 4600 - 2 * CHUNK;
    assert_eq!(&piece[((7 * pw + x) * 4) as usize..][..4], [(4600u32 % 256) as u8, 7, 9, 255]);
}

#[test]
fn normals_come_from_brightness_bright_raised_and_edges_facing_out() {
    // A bright square on a dark ground.
    let (w, h) = (24u32, 24u32);
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let v = if (8..16).contains(&x) && (8..16).contains(&y) { 250 } else { 20 };
            rgba.extend([v, v, v, 255]);
        }
    }
    let png = encode_png(w, h, &rgba).unwrap();
    let made = import("bump [100].png", &png, &Importing { scale: 1.0, normals: true }).unwrap();
    let item = &made.planes[0].items[0];
    assert_eq!(item.image, "art/bump/bump.img.json");
    let sidecar = made.files.iter().find(|(f, _)| f == &item.image).unwrap();
    let image: Image = serde_json::from_slice(&sidecar.1).unwrap();
    assert_eq!(image.normal.len(), 1);
    let map = made.files.iter().find(|(f, _)| f == &image.normal[0].file).unwrap();
    let (_, _, n) = decode_png(&map.1).unwrap();
    let at = |x: u32, y: u32| &n[((y * w + x) * 4) as usize..][..4];
    assert_eq!(at(2, 2), [128, 128, 255, 255], "flat ground does not face straight out");
    assert!(at(8, 12)[0] < 100, "the square's left edge does not face left: {:?}", at(8, 12));
    assert!(at(15, 12)[0] > 156, "the square's right edge does not face right: {:?}", at(15, 12));
    assert!(at(12, 8)[1] > 156, "the square's top edge does not face up: {:?}", at(12, 8));
}

#[test]
fn a_png_is_one_plane_named_and_placed_by_its_file() {
    let png = encode_png(3, 2, &[9u8; 24]).unwrap();
    let made = import("art-src/hills [600].png", &png, &Importing::default()).unwrap();
    assert_eq!(made.planes.len(), 1);
    assert_eq!((made.planes[0].id.as_str(), made.planes[0].depth), ("hills", 600.0));
    assert_eq!(made.planes[0].items[0].image, "art/hills/hills.png");
    assert_eq!(made.size, [3, 2]);
    assert!(import("notes.txt", b"hello", &Importing::default()).unwrap_err().contains("not art that can be imported"));
}

#[test]
fn a_file_of_nothing_but_hidden_layers_says_so() {
    let bytes = write(8, 8, &[Entry::Layer(Layer { hidden: true, ..Layer::filled("off", [0, 0], 2, 2, [1, 1, 1, 255]) })], Header::default(), None);
    assert!(import("off.psd", &bytes, &Importing::default()).unwrap_err().contains("nothing to import"));
}
