//! A canvas on disk: saved and read back the same, byte for byte the second
//! time; layers the canvas's size, pixel layers indexed, clear tiles not kept
//! once read; and a folder that is not a canvas refused with why.

use std::path::PathBuf;

use proton_raster::file::{decode_png, hex, load, save, unhex, FLAT, MANIFEST};
use proton_raster::{Blend, Canvas, Op, Preset};

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-raster-{tag}-{}-{:?}.canvas", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn painted(preset: Preset) -> Canvas {
    let mut c = Canvas::new(150, 90, preset).unwrap();
    let base = c.layers[0].info.id;
    c.apply(&Op::SetPalette { colors: vec![[26, 28, 44, 255], [93, 39, 93, 255], [255, 205, 117, 128]] }).unwrap();
    c.apply(&Op::FillRect { layer: base, rect: [0, 0, 150, 40], color: [26, 28, 44, 255] }).unwrap();
    let top = c.apply(&Op::AddLayer { name: "Highlights — warm".into(), at: None }).unwrap().0.made.unwrap();
    c.apply(&Op::FillRect { layer: top, rect: [70, 10, 50, 60], color: [255, 205, 117, 128] }).unwrap();
    c.apply(&Op::SetLayer { layer: top, name: None, opacity: Some(180), blend: Some(Blend::Screen), hidden: None, locked: None, alpha_lock: Some(true) }).unwrap();
    c
}

fn files(dir: &PathBuf) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for sub in ["", "layers"] {
        for entry in std::fs::read_dir(dir.join(sub)).unwrap().flatten() {
            if entry.path().is_file() {
                out.push((format!("{sub}/{}", entry.file_name().to_string_lossy()), std::fs::read(entry.path()).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_canvas_saved_and_read_back_is_the_same_and_saves_to_the_same_bytes() {
    for preset in [Preset::Pixel, Preset::Paint] {
        let dir = folder(&format!("round-{preset:?}"));
        let canvas = painted(preset);
        let written = save(&dir, &canvas).unwrap();
        assert_eq!(written.last().map(String::as_str), Some(MANIFEST), "canvas.json is not written last");
        assert_eq!(written.contains(&FLAT.to_string()), preset == Preset::Pixel);
        let first = files(&dir);
        let back = load(&dir).unwrap();
        let mut same = canvas.clone();
        // What is not saved: which state of its history it was in.
        same.state = 0;
        same.made = 0;
        assert_eq!(back.hash(), same.hash(), "{preset:?} did not read back the same");
        assert_eq!(back.palette, canvas.palette);
        save(&dir, &back).unwrap();
        assert_eq!(files(&dir), first, "{preset:?} saved again to different bytes");
    }
}

#[test]
fn canvas_json_is_written_to_be_read() {
    let dir = folder("manifest");
    save(&dir, &painted(Preset::Pixel)).unwrap();
    let text = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
    assert!(text.starts_with("{\n  \"format\": 1,\n  \"preset\": \"pixel\",\n  \"size\": [150, 90],"), "{text}");
    assert!(text.contains("\"palette\": [\"#1a1c2c\", \"#5d275d\", \"#ffcd7580\"]"), "{text}");
    assert!(text.contains("\"name\": \"Highlights — warm\"") && text.contains("\"blend\": \"screen\"") && text.contains("\"alphaLock\": true"), "{text}");
}

#[test]
fn a_pixel_layer_of_few_colours_is_indexed_and_a_painted_one_is_not() {
    let dir = folder("indexed");
    save(&dir, &painted(Preset::Pixel)).unwrap();
    let kind = |bytes: &[u8]| png::Decoder::new(bytes).read_info().unwrap().info().color_type;
    assert_eq!(kind(&std::fs::read(dir.join("layers/1.png")).unwrap()), png::ColorType::Indexed);
    let paint = folder("rgba");
    save(&paint, &painted(Preset::Paint)).unwrap();
    assert_eq!(kind(&std::fs::read(paint.join("layers/1.png")).unwrap()), png::ColorType::Rgba);
    // Indexed or not, it is read back as the same pixels.
    let (w, h, rgba) = decode_png(&std::fs::read(dir.join("layers/2.png")).unwrap()).unwrap();
    assert_eq!((w, h), (150, 90));
    assert_eq!(&rgba[((20 * 150 + 80) * 4) as usize..][..4], [255, 205, 117, 128]);
}

#[test]
fn clear_tiles_are_not_kept_and_a_removed_layers_file_goes_with_it() {
    let dir = folder("sparse");
    let mut c = painted(Preset::Paint);
    save(&dir, &c).unwrap();
    let back = load(&dir).unwrap();
    // The highlights cover two of the six tiles; the rest are not kept.
    assert_eq!(back.layers[1].grid.tiles.iter().filter(|t| t.is_some()).count(), 2);
    c.apply(&Op::RemoveLayer { layer: 2 }).unwrap();
    std::fs::write(dir.join("layers/notes.png"), b"theirs").unwrap();
    save(&dir, &c).unwrap();
    assert!(!dir.join("layers/2.png").exists(), "the removed layer's file was left");
    assert!(dir.join("layers/notes.png").exists(), "a file the canvas did not write was deleted");
}

#[test]
fn a_folder_that_is_not_a_canvas_is_refused_with_why() {
    let dir = folder("bad");
    save(&dir, &painted(Preset::Pixel)).unwrap();
    let manifest = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
    std::fs::write(dir.join(MANIFEST), manifest.replace("\"format\": 1", "\"format\": 9")).unwrap();
    assert!(load(&dir).unwrap_err().contains("newer Proton"));
    std::fs::write(dir.join(MANIFEST), manifest.replace("\"size\": [150, 90]", "\"size\": [150, 91]")).unwrap();
    assert!(load(&dir).unwrap_err().contains("not the canvas's 150×91"));
    std::fs::write(dir.join(MANIFEST), manifest.replace("layers/2.png", "../2.png")).unwrap();
    assert!(load(&dir).unwrap_err().contains("not a layer of this canvas"));
    std::fs::write(dir.join(MANIFEST), manifest.replace("\"size\": [150, 90]", "\"size\": [2000, 90]")).unwrap();
    assert!(load(&dir).unwrap_err().contains("at most 1024"));
    assert!(load(&folder("nowhere")).is_err());
}

#[test]
fn colours_are_written_as_hex() {
    assert_eq!(hex([255, 0, 128, 255]), "#ff0080");
    assert_eq!(hex([1, 2, 3, 4]), "#01020304");
    assert_eq!(unhex("#ff0080").unwrap(), [255, 0, 128, 255]);
    assert_eq!(unhex("01020304").unwrap(), [1, 2, 3, 4]);
    assert!(unhex("#ff00").is_err() && unhex("#gg0000").is_err());
}
