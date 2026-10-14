//! Animation: an Aseprite file brought in as frames on a sheet — laid
//! together, trimmed, the same picture kept once — its tags clips, its
//! slices hitboxes and feet, a cel's text a cue; a sprite sheet's JSON read
//! as it is, Aseprite's map of frames in their numbers' order and Canvas's
//! own; and the player playing its states' clips in the run, on, back and to
//! the end and back, held when played out, cues said, the same in every run.

use proton_game::format::{Anim, BoxKind, Direction};
use proton_game::import::anim::{import_anim, is_anim_source};
use proton_game::project::Project;
use proton_game::sim::{input, Happened, Sim, World};
use proton_game::template;
use proton_raster::ase_write::{cel, layer, linked, slice, tags, user_text, Ase};

const RED: [u8; 4] = [200, 30, 40, 255];
const BLUE: [u8; 4] = [40, 60, 220, 255];

fn square(n: usize, c: [u8; 4]) -> Vec<u8> {
    c.repeat(n)
}

/// A walk of four frames on a 16 × 24 sprite: a body that moves, a hidden
/// sketch, the last frame linked to the second; tags, a hitbox, the feet, a
/// cue.
fn walker() -> Vec<u8> {
    Ase {
        width: 16,
        height: 24,
        depth: 32,
        transparent: 0,
        frames: vec![
            (
                100,
                vec![
                    layer(1, 0, 0, 0, 255, "Body"),
                    layer(0, 0, 0, 0, 255, "Sketch"),
                    cel(0, 4, 8, 255, 4, 16, &square(64, RED), true),
                    cel(1, 0, 0, 255, 16, 24, &square(16 * 24, BLUE), true),
                    tags(&[(0, 0, 0, 0, "idle"), (1, 3, 2, 3, "run")]),
                    slice("Hitbox", &[(0, [2, 8, 8, 16], None), (2, [6, 8, 8, 16], None), (3, [0, 0, 0, 0], None)]),
                    slice("feet", &[(0, [4, 20, 4, 4], Some([2, 4]))]),
                ],
            ),
            (80, vec![cel(0, 6, 8, 255, 4, 16, &square(64, RED), false), user_text("step")]),
            (80, vec![cel(0, 8, 8, 255, 4, 16, &square(64, RED), true)]),
            (120, vec![linked(0, 1)]),
        ],
    }
    .bytes()
}

#[test]
fn an_aseprite_file_becomes_frames_on_a_sheet_with_clips_boxes_feet_and_cues() {
    let bytes = walker();
    assert!(is_anim_source("art/hero/walk.aseprite", &bytes));
    let made = import_anim("art/hero/walk.aseprite", &bytes).unwrap();
    assert_eq!(made.path, "art/hero/walk.anim.json");
    let a = &made.anim;
    assert_eq!((a.size, a.sheets.clone()), ([16, 24], vec!["art/hero/walk.sheet.png".to_string()]));
    // Trimmed to the body (the sketch is hidden), placed where it was.
    assert_eq!(a.frames.iter().map(|f| (f.offset, [f.rect[2], f.rect[3]], f.ms)).collect::<Vec<_>>(), [
        ([4, 8], [4, 16], 100),
        ([6, 8], [4, 16], 80),
        ([8, 8], [4, 16], 80),
        ([6, 8], [4, 16], 120),
    ]);
    // Frame 4 is frame 2 again: one picture on the sheet for both.
    assert_eq!(a.frames[3].rect, a.frames[1].rect);
    assert_ne!(a.frames[2].rect, a.frames[1].rect);
    let (w, h, sheet) = proton_raster::file::decode_png(&made.files[0].1).unwrap();
    let px = |x: u32, y: u32| &sheet[((y * w + x) * 4) as usize..][..4];
    let r = a.frames[2].rect;
    assert_eq!((px(r[0], r[1]), px(r[0] + 3, r[1] + 15)), (&RED[..], &RED[..]));
    assert!(w <= 2048 && h == 16, "{w} × {h}");
    // Tags are clips; slices boxes and feet; a cel's text a cue.
    assert_eq!(a.clips.iter().map(|c| (c.name.as_str(), c.from, c.to, c.direction)).collect::<Vec<_>>(), [("idle", 0, 0, Direction::Forward), ("run", 1, 3, Direction::PingPong)]);
    assert_eq!(a.boxes.len(), 1);
    assert_eq!((a.boxes[0].kind, a.boxes[0].frames.clone()), (BoxKind::Hit, vec![Some([2, 8, 8, 16]), Some([2, 8, 8, 16]), Some([6, 8, 8, 16]), None]));
    assert!(a.frames.iter().all(|f| f.pivot == Some([6, 24])), "{:?}", a.frames.iter().map(|f| f.pivot).collect::<Vec<_>>());
    assert_eq!(a.cues.iter().map(|c| (c.frame, c.name.as_str())).collect::<Vec<_>>(), [(1, "step")]);
    // What was written reads back as the animation.
    let back: Anim = proton_game::doc::parse(&made.path, std::str::from_utf8(&made.files[1].1).unwrap()).unwrap();
    assert_eq!(&back, a);
}

#[test]
fn a_sprite_sheets_json_is_read_as_it_is() {
    // Aseprite's map of frames, out of order as a map keeps them.
    let frame = |x: u32, ms: u32| serde_json::json!({ "frame": {"x": x, "y": 0, "w": 10, "h": 12}, "rotated": false, "trimmed": true,
        "spriteSourceSize": {"x": 3, "y": 4, "w": 10, "h": 12}, "sourceSize": {"w": 16, "h": 16}, "duration": ms });
    let mut frames = serde_json::Map::new();
    for i in [10u32, 2, 0, 1, 3, 4, 5, 6, 7, 8, 9] {
        frames.insert(format!("bat {i}.aseprite"), frame(i * 10, 50 + i));
    }
    let doc = serde_json::json!({ "frames": frames, "meta": { "image": "bat-sheet.png", "size": {"w": 110, "h": 12}, "scale": "1",
        "frameTags": [{"name": "fly", "from": 0, "to": 10, "direction": "pingpong", "repeat": "3"}],
        "slices": [{"name": "hurtbox", "keys": [{"frame": 0, "bounds": {"x": 2, "y": 2, "w": 12, "h": 12}}]},
                   {"name": "Pivot", "keys": [{"frame": 0, "bounds": {"x": 6, "y": 14, "w": 4, "h": 2}, "pivot": {"x": 2, "y": 2}}]}] } });
    let bytes = serde_json::to_vec(&doc).unwrap();
    assert!(is_anim_source("art/bat-sheet.json", &bytes) && !is_anim_source("art/bat.anim.json", &bytes));
    let made = import_anim("art/bat-sheet.json", &bytes).unwrap();
    assert_eq!((made.path.as_str(), made.anim.sheets.clone(), made.anim.size), ("art/bat.anim.json", vec!["art/bat-sheet.png".to_string()], [16, 16]));
    assert_eq!(made.anim.frames.iter().map(|f| f.rect[0]).collect::<Vec<_>>(), (0..11).map(|i| i * 10).collect::<Vec<_>>(), "frames out of their numbers' order");
    assert_eq!((made.anim.frames[10].ms, made.anim.frames[0].offset), (60, [3, 4]));
    let c = &made.anim.clips[0];
    assert_eq!((c.name.as_str(), c.direction, c.repeat), ("fly", Direction::PingPong, 3));
    assert_eq!((made.anim.boxes[0].kind, made.anim.frames[5].pivot), (BoxKind::Hurt, Some([8, 16])));
    assert_eq!(made.files.len(), 1, "a sheet's image is left where it is");
    // A frame turned on its sheet is refused, not read wrongly.
    let mut turned = doc.clone();
    turned["frames"]["bat 3.aseprite"]["rotated"] = serde_json::json!(true);
    assert!(import_anim("art/bat-sheet.json", &serde_json::to_vec(&turned).unwrap()).unwrap_err().contains("turned"));
}

#[test]
fn a_canvas_sheet_comes_in_frame_for_frame() {
    let mut c = proton_raster::Canvas::new(8, 8, proton_raster::Preset::Pixel).unwrap();
    let id = c.layers[0].info.id;
    for f in 0..3u32 {
        if f > 0 {
            c.apply(&proton_raster::Op::AddFrame { at: None, copy: None }).unwrap();
        }
        c.apply(&proton_raster::Op::FillRect { layer: proton_raster::cel_id(id, f), rect: [f as i32 * 2, 0, 2, 8], color: RED }).unwrap();
    }
    c.frames = vec![90, 90, 150];
    c.tags = vec![proton_raster::Tag { name: "idle".into(), from: 0, to: 2, direction: proton_raster::Direction::Forward, repeat: 0 }];
    let (_, json) = proton_raster::port::sheet(&c, "slime-sheet.png", 0, 1).unwrap();
    let made = import_anim("art/slime-sheet.json", json.as_bytes()).unwrap();
    assert_eq!(made.anim.frames.iter().map(|f| (f.rect, f.ms)).collect::<Vec<_>>(), [([0, 0, 8, 8], 90), ([8, 0, 8, 8], 90), ([0, 8, 8, 8], 150)]);
    assert_eq!((made.anim.clips[0].name.as_str(), made.anim.size), ("idle", [8, 8]));
}

/// The template, its player animated by the walker.
fn animated(tag: &str) -> (World, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("proton-anim-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    template::write(&dir, &template::files("metroidvania", "Anim").unwrap()).unwrap();
    let made = import_anim("art/walk.aseprite", &walker()).unwrap();
    for (rel, bytes) in &made.files {
        std::fs::write(dir.join(rel), bytes).unwrap();
    }
    let prefab = dir.join("prefabs/player.prefab.json");
    let mut text: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prefab).unwrap()).unwrap();
    text["sprite"] = serde_json::json!("art/walk.anim.json");
    // Rising and falling play the run, the clip named for them not being there.
    text["clips"] = serde_json::json!({ "jump": "run" });
    std::fs::write(&prefab, text.to_string()).unwrap();
    (World::cook(&Project::open(&dir).unwrap()).unwrap(), dir)
}

#[test]
fn the_player_plays_its_states_clips_and_says_their_cues() {
    let (world, dir) = animated("states");
    let a = &world.anims[0];
    // Times in ticks at 60 a second; the feet from the slice.
    assert_eq!(a.frames.iter().map(|f| f.0).collect::<Vec<_>>(), [6, 5, 5, 7]);
    assert_eq!(world.player.states, [Some(0), Some(1), Some(1), Some(0), Some(1), Some(0), Some(0)]);
    let mut sim = Sim::new(world.clone(), u32::MAX);
    for _ in 0..30 {
        sim.step(0);
    }
    let still = sim.frame()[0];
    assert_eq!((still.clip, still.frame, still.pivot), (Some(0), Some(0), [6, 24]), "standing plays idle");
    // Running: the run clip, to the end and back, its cue said at frame 2.
    let mut frames = Vec::new();
    let mut cues = 0;
    sim.happened();
    for _ in 0..40 {
        sim.step(input::RIGHT);
        let f = sim.frame()[0];
        if frames.last() != f.frame.as_ref() {
            frames.push(f.frame.unwrap());
        }
        cues += sim.happened().iter().filter(|h| matches!(h, Happened::Cue(0))).count();
    }
    assert_eq!(&frames[..6], [1, 2, 3, 2, 1, 2], "the run went {frames:?}");
    assert!(cues >= 2 && world.cues == ["step"], "{cues} cues of {:?}", world.cues);
    // The same input, the same run — and its hash moves with the animation.
    let replay = |w: &World| {
        let mut s = Sim::new(w.clone(), u32::MAX);
        for i in 0..90 {
            s.step(if i < 50 { input::RIGHT } else if i < 55 { input::JUMP } else { 0 });
        }
        (s.hash(), s.state.anim)
    };
    let back: World = serde_json::from_str(&serde_json::to_string(&world).unwrap()).unwrap();
    assert_eq!(replay(&world), replay(&back));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_clip_played_out_holds_its_last_frame() {
    let (mut world, dir) = animated("held");
    // Run plays once, on: 1, 2, 3, then holds.
    world.anims[0].clips[1].repeat = 1;
    world.anims[0].clips[1].direction = 0;
    let mut sim = Sim::new(world, u32::MAX);
    for _ in 0..120 {
        sim.step(input::RIGHT);
    }
    let a = sim.state.anim;
    assert!(a.held && a.frame == 3 && a.plays == 1, "{a:?}");
    let _ = std::fs::remove_dir_all(dir);
}
