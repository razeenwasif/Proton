//! Animation: frames added, copied, taken out and moved — each layer's
//! pixels in each frame going with them — how long each shows, and tags that
//! keep to the frames they name; all undone to the same pixels. A canvas of
//! frames saved and opened again is the canvas it was, a still picture's
//! folder as it always was; a sprite sheet lays every frame out and says
//! where; onion skin shows other frames under a layer and nowhere else.

use proton_raster::blend::{self, Onion};
use proton_raster::{cel_id, Canvas, Direction, History, Op, Preset, Tag};

const RED: [u8; 4] = [220, 40, 40, 255];
const BLUE: [u8; 4] = [40, 60, 220, 255];

fn dot(layer: u32, x: i32, color: [u8; 4]) -> Op {
    Op::FillRect { layer, rect: [x, 2, 2, 2], color }
}

fn tag(name: &str, from: u32, to: u32) -> Tag {
    Tag { name: name.into(), from, to, direction: Direction::Forward, repeat: 0 }
}

/// A canvas of three frames, a dot in each at another place.
fn three() -> (Canvas, u32) {
    let mut c = Canvas::new(16, 8, Preset::Pixel).unwrap();
    let id = c.layers[0].info.id;
    c.apply(&dot(id, 0, RED)).unwrap();
    c.apply(&Op::AddFrame { at: None, copy: None }).unwrap();
    c.apply(&dot(cel_id(id, 1), 4, RED)).unwrap();
    c.apply(&Op::AddFrame { at: None, copy: None }).unwrap();
    c.apply(&dot(cel_id(id, 2), 8, BLUE)).unwrap();
    (c, id)
}

#[test]
fn a_cel_is_a_layer_in_a_frame_and_drawing_on_one_leaves_the_others() {
    let (c, id) = three();
    assert_eq!((c.frames.len(), c.layers[0].cels.len()), (3, 3));
    let at = |frame: u32, x: u32| c.grid(cel_id(id, frame)).unwrap().pixel(x, 2);
    assert_eq!([at(0, 0), at(0, 4), at(1, 4), at(1, 0), at(2, 8)], [RED, [0; 4], RED, [0; 4], BLUE]);
    assert_eq!(cel_id(id, 0), id, "a layer's own id is its first frame");
    // A layer added later is clear in every frame; a frame past the end is
    // not there.
    let mut c2 = c.clone();
    let (applied, _) = c2.apply(&Op::AddLayer { name: "Top".into(), at: None }).unwrap();
    let top = applied.made.unwrap();
    assert_eq!(c2.layer(top).unwrap().cels.len(), 3);
    assert!(c2.apply(&dot(cel_id(id, 3), 0, RED)).unwrap_err().contains("frame 4"));
    // A flattened frame is that frame's layers.
    assert_eq!(&blend::flatten(&c, 1)[(2 * 16 + 4) * 4..][..4], &RED);
    assert_eq!(&blend::flatten(&c, 0)[(2 * 16 + 4) * 4..][..4], &[0; 4]);
}

#[test]
fn frames_added_copied_taken_out_and_moved_carry_their_pixels_and_undo() {
    let (mut c, id) = three();
    let mut h = History::new(Preset::Pixel);
    let start = c.hash();
    c.tags = vec![tag("walk", 0, 1), tag("hit", 2, 2)];
    c.frames[1] = 150;
    let before = c.hash();
    // A copy of frame 2 put straight after it: its pixels, its time, and —
    // the last frame of "walk" — in "walk".
    h.apply(&mut c, &Op::AddFrame { at: Some(2), copy: Some(1) }, "copy").unwrap();
    assert_eq!(c.frames, [100, 150, 150, 100]);
    assert_eq!(c.grid(cel_id(id, 2)).unwrap().pixel(4, 2), RED);
    assert_eq!(c.grid(cel_id(id, 3)).unwrap().pixel(8, 2), BLUE, "the frame after moved along");
    assert_eq!(c.tags, [tag("walk", 0, 2), tag("hit", 3, 3)]);
    // The copy shares the pixels until one is drawn on.
    h.apply(&mut c, &dot(cel_id(id, 2), 10, BLUE), "draw").unwrap();
    assert_eq!(c.grid(cel_id(id, 1)).unwrap().pixel(10, 2), [0; 4]);
    // Moved to the front, then the last frame taken out — and "hit" with it.
    h.apply(&mut c, &Op::MoveFrame { frame: 2, to: 0 }, "move").unwrap();
    assert_eq!(c.grid(cel_id(id, 0)).unwrap().pixel(10, 2), BLUE);
    assert_eq!(c.frames, [150, 100, 150, 100]);
    h.apply(&mut c, &Op::RemoveFrame { frame: 3 }, "remove").unwrap();
    assert_eq!((c.frames.len(), c.tags.clone()), (3, vec![tag("walk", 0, 2)]));
    for _ in 0..4 {
        h.undo(&mut c).unwrap();
    }
    assert_eq!(c.hash(), before, "undone back to where it began");
    for _ in 0..4 {
        h.redo(&mut c).unwrap();
    }
    assert_eq!(c.frames.len(), 3);
    // A still picture's hash does not count its one frame's time.
    let mut still = Canvas::new(16, 8, Preset::Pixel).unwrap();
    let plain = still.hash();
    still.frames = vec![100];
    assert_eq!(still.hash(), plain);
    assert_ne!(start, before);
}

#[test]
fn what_cannot_be_made_of_frames_is_refused() {
    let (mut c, id) = three();
    assert!(c.apply(&Op::AddFrame { at: None, copy: Some(3) }).is_err());
    assert!(c.apply(&Op::RemoveFrame { frame: 5 }).is_err());
    assert!(c.apply(&Op::SetDuration { from: 0, to: 3, duration: 80 }).is_err());
    assert!(c.apply(&Op::SetDuration { from: 0, to: 2, duration: 0 }).is_err());
    assert!(c.apply(&Op::SetTags { tags: vec![tag("", 0, 1)] }).is_err());
    assert!(c.apply(&Op::SetTags { tags: vec![tag("run", 2, 1)] }).is_err());
    assert!(c.apply(&Op::SetTags { tags: vec![tag("run", 0, 3)] }).is_err());
    let mut one = Canvas::new(4, 4, Preset::Pixel).unwrap();
    assert!(one.apply(&Op::RemoveFrame { frame: 0 }).unwrap_err().contains("at least one frame"));
    // Times and tags set, and set again to the same is no change.
    let (applied, _) = c.apply(&Op::SetDuration { from: 1, to: 2, duration: 80 }).unwrap();
    assert!(applied.changed && applied.meta && applied.touched.is_empty());
    assert_eq!(c.frames, [100, 80, 80]);
    assert!(!c.apply(&Op::SetDuration { from: 1, to: 2, duration: 80 }).unwrap().0.changed);
    c.apply(&Op::SetTags { tags: vec![Tag { name: "  run ".into(), ..tag("x", 0, 2) }] }).unwrap();
    assert_eq!(c.tags[0].name, "run");
    // A cel copied to another frame.
    c.apply(&Op::CopyCel { from: cel_id(id, 2), to: cel_id(id, 0) }).unwrap();
    assert_eq!(c.grid(id).unwrap().pixel(8, 2), BLUE);
    assert_eq!(c.grid(id).unwrap().pixel(0, 2), [0; 4]);
}

#[test]
fn the_window_and_the_core_agree_on_what_a_frame_op_touched() {
    let (c, id) = three();
    for op in [Op::AddFrame { at: Some(1), copy: Some(2) }, Op::RemoveFrame { frame: 0 }, Op::MoveFrame { frame: 0, to: 2 }, Op::CopyCel { from: cel_id(id, 1), to: cel_id(id, 2) }] {
        let (mut core, mut window) = (c.clone(), c.clone());
        let (a, _) = core.apply(&op).unwrap();
        let (b, _) = window.apply(&op).unwrap();
        assert_eq!(core.hash_touched(&a.touched), window.hash_touched(&b.touched), "{op:?}");
        // Every cel whose pixels at its place changed is told.
        let before = c.clone();
        for frame in 0..core.frames.len() as u32 {
            let cel = cel_id(id, frame);
            let (now, then) = (core.grid(cel).unwrap(), before.grid(cel));
            let differs = match then {
                Some(t) => (0..t.cols * t.rows).any(|i| t.tile(i).map(|x| &x.px) != now.tile(i).map(|x| &x.px)),
                None => now.tiles.iter().any(Option::is_some),
            };
            if differs {
                assert!(a.touched.iter().any(|(l, _)| *l == cel), "{op:?}: frame {frame} changed and was not told");
            }
        }
    }
}

#[test]
fn a_canvas_of_frames_is_saved_and_opened_as_it_was() {
    let dir = std::env::temp_dir().join(format!("proton-frames-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut c, id) = three();
    c.frames = vec![100, 60, 250];
    c.tags = vec![Tag { name: "blink".into(), from: 1, to: 2, direction: Direction::PingPong, repeat: 3 }];
    let top = c.push_layer("Clear".into());
    c.apply(&dot(cel_id(top, 2), 12, BLUE)).unwrap();
    let written = proton_raster::file::save(&dir, &c).unwrap();
    // A later frame where a layer has nothing is not written.
    assert!(written.contains(&format!("layers/{id}-2.png")) && written.contains(&format!("layers/{top}-3.png")));
    assert!(!written.contains(&format!("layers/{top}-2.png")));
    let text = std::fs::read_to_string(dir.join("canvas.json")).unwrap();
    assert!(text.contains("\"format\": 2") && text.contains("\"durations\"") && text.contains("\"blink\""), "{text}");
    let back = proton_raster::file::load(&dir).unwrap();
    assert_eq!((back.frames.clone(), back.tags.clone()), (c.frames.clone(), c.tags.clone()));
    assert_eq!(back.hash(), {
        let mut c = c.clone();
        (c.state, c.made) = (back.state, back.made);
        c.hash()
    });
    // Saved again, the same bytes; a frame taken out takes its files.
    let first: Vec<Vec<u8>> = written.iter().map(|f| std::fs::read(dir.join(f)).unwrap()).collect();
    let again = proton_raster::file::save(&dir, &back).unwrap();
    assert_eq!(again, written);
    assert_eq!(again.iter().map(|f| std::fs::read(dir.join(f)).unwrap()).collect::<Vec<_>>(), first);
    let mut fewer = back.clone();
    fewer.apply(&Op::RemoveFrame { frame: 2 }).unwrap();
    proton_raster::file::save(&dir, &fewer).unwrap();
    assert!(!dir.join(format!("layers/{top}-3.png")).exists() && !dir.join(format!("layers/{id}-3.png")).exists());
    // A still picture is format 1, with nothing of frames but their count.
    let still = Canvas::new(8, 8, Preset::Pixel).unwrap();
    proton_raster::file::save(&dir.join("still"), &still).unwrap();
    let text = std::fs::read_to_string(dir.join("still/canvas.json")).unwrap();
    assert!(text.contains("\"format\": 1") && !text.contains("durations") && !text.contains("tags") && !text.contains("cels"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sprite_sheet_lays_out_every_frame_and_says_where() {
    let (mut c, _) = three();
    c.frames = vec![100, 60, 250];
    c.tags = vec![Tag { name: "blink".into(), from: 1, to: 2, direction: Direction::PingPongReverse, repeat: 0 }];
    // As near a square as three make: two columns, two rows.
    let (png, json) = proton_raster::port::sheet(&c, "hero.png", 0, 2).unwrap();
    let (w, h, rgba) = proton_raster::file::decode_png(&png).unwrap();
    assert_eq!((w, h), (2 * 16 * 2, 2 * 8 * 2));
    for frame in 0..3u32 {
        let flat = blend::flatten(&c, frame);
        let (fx, fy) = ((frame % 2) * 32, (frame / 2) * 16);
        for (x, y) in [(0u32, 2u32), (4, 2), (8, 3), (15, 7)] {
            for (dx, dy) in [(0, 0), (1, 1)] {
                let at = (((fy + y * 2 + dy) * w + fx + x * 2 + dx) * 4) as usize;
                let from = ((y * 16 + x) * 4) as usize;
                assert_eq!(&rgba[at..at + 4], &flat[from..from + 4], "frame {frame} at {x}, {y}");
            }
        }
    }
    let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
    let frames = doc["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[2]["frame"], serde_json::json!({ "x": 0, "y": 16, "w": 32, "h": 16 }));
    assert_eq!(frames.iter().map(|f| f["duration"].as_u64().unwrap()).collect::<Vec<_>>(), [100, 60, 250]);
    assert_eq!(doc["meta"]["frameTags"][0], serde_json::json!({ "name": "blink", "from": 1, "to": 2, "direction": "pingpong_reverse" }));
    assert_eq!((doc["meta"]["image"].as_str(), doc["meta"]["size"]["w"].as_u64()), (Some("hero.png"), Some(64)));
    // A strip: every frame in one row.
    let (png, _) = proton_raster::port::sheet(&c, "hero.png", 3, 1).unwrap();
    assert_eq!(proton_raster::file::decode_png(&png).unwrap().0, 48);
}

#[test]
fn onion_skin_shows_other_frames_under_a_layer_and_nothing_more() {
    let (c, _) = three();
    let plain = blend::tile(&c, 1, 0);
    let skins = [Onion { frame: 0, opacity: 128, tint: Some([255, 0, 0]) }, Onion { frame: 2, opacity: 128, tint: None }];
    let onion = blend::fold(&c, 1, 0, [0, 0, 64, 64], 0, 1, vec![0; 64 * 64 * 4], &skins);
    let px = |acc: &[u64], x: usize| blend::straight(&acc[(2 * 64 + x) * 4..][..4]);
    // Frame 1's own dot is as it is; frame 0's shows faintly, tinted; frame
    // 2's faintly, its own colour.
    assert_eq!(px(&onion, 4), px(&plain, 4));
    assert_eq!(px(&plain, 0), [0; 4]);
    let past = px(&onion, 0);
    assert!(past[3] > 100 && past[3] < 160 && past[0] > 200 && past[2] < 30, "{past:?}");
    let next = px(&onion, 8);
    assert!(next[3] > 100 && next[3] < 160 && next[2] > 200, "{next:?}");
}
