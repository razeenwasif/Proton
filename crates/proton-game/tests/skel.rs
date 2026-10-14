//! Skeletons: a DragonBones armature brought in — bones parents first, an
//! image's four points where its pivot, trimming and transform put them, a
//! region turned on its atlas read across, a weighted mesh's points found on
//! the bones that carry them (worked out here by hand), a bounding box a
//! hitbox; turns unwound as DragonBones unwinds them, eases as it reads them,
//! a slot's parts and colour, events as cues; what is left out said — and
//! the player's skeleton cooked into clips of one-tick frames a run plays.

use proton_game::format::{BoxKind, Ease, Skel};
use proton_game::import::skel::{import_skel, is_skel_source};
use proton_game::project::Project;
use proton_game::sim::{input, Happened, Sim, World};
use proton_game::template;
use serde_json::json;

fn atlas() -> serde_json::Value {
    json!({ "name": "hero", "imagePath": "hero_tex.png", "width": 64, "height": 32, "SubTexture": [
        { "name": "body", "x": 0, "y": 0, "width": 16, "height": 24 },
        { "name": "arm", "x": 16, "y": 0, "width": 8, "height": 16, "frameX": -2, "frameY": -1, "frameWidth": 12, "frameHeight": 18 },
        { "name": "cape", "x": 32, "y": 0, "width": 16, "height": 8, "rotated": true }
    ]})
}

fn armature() -> serde_json::Value {
    json!({ "frameRate": 24, "name": "hero", "version": "5.5", "armature": [{
        "name": "hero", "frameRate": 24,
        // A child listed before its parent: put after it.
        "bone": [
            { "name": "arm", "parent": "body", "transform": { "x": 4, "y": -6, "skX": 90, "skY": 90 } },
            { "name": "root" },
            { "name": "body", "parent": "root", "transform": { "y": -20 } }
        ],
        "slot": [
            { "name": "body", "parent": "body" },
            { "name": "arm", "parent": "arm" },
            { "name": "cape", "parent": "body", "color": { "aM": 50 } },
            { "name": "skin", "parent": "body" },
            { "name": "hit", "parent": "arm" }
        ],
        "skin": [{ "name": "", "slot": [
            { "name": "body", "display": [{ "name": "body" }] },
            { "name": "arm", "display": [{ "name": "arm", "transform": { "x": 2 }, "pivot": { "x": 0, "y": 0 } }] },
            { "name": "cape", "display": [{ "name": "cape", "pivot": { "x": 0, "y": 0 } }] },
            { "name": "skin", "display": [{ "name": "skin", "type": "mesh", "path": "body",
                "vertices": [0, -20, 4, -30, 4, -40], "uvs": [0, 0, 1, 0, 1, 1], "triangles": [0, 1, 2],
                // Bone indices are the file's: body is 2, arm 0.
                "weights": [1, 2, 1.0, 2, 2, 0.5, 0, 0.5, 1, 0, 1.0],
                "slotPose": [1, 0, 0, 1, 0, 0],
                "bonePose": [2, 1, 0, 0, 1, 0, -20, 0, 0, 1, -1, 0, 4, -26] }] },
            { "name": "hit", "display": [{ "name": "Hitbox", "type": "boundingBox", "subType": "rectangle", "width": 10, "height": 6, "transform": { "x": 5 } }] }
        ]}],
        "ik": [{ "name": "reach", "bone": "arm", "target": "root" }],
        "animation": [
            { "name": "idle", "duration": 10, "playTimes": 0,
              "bone": [
                { "name": "arm", "rotateFrame": [{ "duration": 5, "tweenEasing": 0, "rotate": 0 }, { "duration": 5, "rotate": -170 }, { "duration": 0, "rotate": 170 }] },
                { "name": "body", "translateFrame": [{ "duration": 10, "tweenEasing": 1, "x": 0, "y": 0 }, { "duration": 0, "x": 0, "y": -4 }] }
              ],
              "slot": [{ "name": "cape", "displayFrame": [{ "duration": 5, "value": 0 }, { "duration": 5, "value": -1 }],
                         "colorFrame": [{ "duration": 10, "tweenEasing": 0, "value": { "aM": 100 } }, { "duration": 0, "value": { "aM": 0 } }] }],
              "frame": [{ "duration": 3 }, { "duration": 7, "events": [{ "name": "step" }] }] },
            { "name": "spin", "duration": 4, "playTimes": 1,
              "bone": [{ "name": "arm", "rotateFrame": [{ "duration": 4, "rotate": 0, "clockwise": -1, "tweenEasing": -0.5, "curve": null }, { "duration": 0, "rotate": 90 }] }],
              "zOrder": { "frame": [{ "duration": 4, "zOrder": [0, 1] }] } }
        ]
    }]})
}

fn read(rel: &str) -> Option<Vec<u8>> {
    (rel == "art/hero_tex.json").then(|| atlas().to_string().into_bytes())
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

#[test]
fn a_dragonbones_armature_becomes_bones_and_parts_on_them() {
    let bytes = armature().to_string().into_bytes();
    assert!(is_skel_source("art/hero_ske.json", &bytes) && !is_skel_source("art/hero_tex.json", atlas().to_string().as_bytes()));
    let made = import_skel("art/hero_ske.json", &bytes, &read).unwrap();
    let s = &made.skel;
    assert_eq!((made.path.as_str(), s.atlas.as_str(), s.atlas_size, s.fps), ("art/hero.skel.json", "art/hero_tex.png", [64, 32], 24.0));
    // Parents first; the arm turned a quarter.
    assert_eq!(s.bones.iter().map(|b| (b.name.as_str(), b.parent)).collect::<Vec<_>>(), [("root", None), ("body", Some(0)), ("arm", Some(1))]);
    assert_eq!((s.bones[2].at, s.bones[2].rot, s.bones[2].skew), ([4.0, -6.0], 90.0, 0.0));
    // The body: centred on its bone. The arm: trimmed, its pivot its top left, moved 2.
    let corners = |slot: usize| s.slots[slot].parts[0].points.iter().map(|p| p.bones[0].2).collect::<Vec<_>>();
    assert_eq!(corners(0), [[-8.0, -12.0], [8.0, -12.0], [8.0, 12.0], [-8.0, 12.0]]);
    assert_eq!(corners(1), [[4.0, 1.0], [12.0, 1.0], [12.0, 17.0], [4.0, 17.0]]);
    assert_eq!(s.slots[1].parts[0].points[0].uv, [16.0, 0.0]);
    // The cape, turned on its atlas: eight wide, sixteen tall, read across.
    assert_eq!(corners(2), [[0.0, 0.0], [8.0, 0.0], [8.0, 16.0], [0.0, 16.0]]);
    assert_eq!(s.slots[2].parts[0].points.iter().map(|p| p.uv).collect::<Vec<_>>(), [[48.0, 0.0], [48.0, 8.0], [32.0, 8.0], [32.0, 0.0]]);
    assert_eq!(s.slots[2].color, [1.0, 1.0, 1.0, 0.5]);
    // The mesh: each point where its bones held it when bound.
    let skin = &s.slots[3].parts[0];
    assert_eq!(skin.points[0].bones, [(1, 1.0, [0.0, 0.0])]);
    let p1 = &skin.points[1].bones;
    assert_eq!((p1[0].0, p1[0].1, p1[1].0, p1[1].1), (1, 0.5, 2, 0.5));
    assert!(close(p1[0].2, [4.0, -10.0]) && close(p1[1].2, [-4.0, 0.0]), "{p1:?}");
    assert!(close(skin.points[2].bones[0].2, [-14.0, 0.0]), "{:?}", skin.points[2].bones);
    assert_eq!((skin.triangles.clone(), skin.points[2].uv), (vec![[0, 1, 2]], [16.0, 24.0]));
    // The bounding box: a hitbox on the arm.
    assert_eq!(s.boxes.len(), 1);
    let b = &s.boxes[0];
    assert_eq!((b.kind, b.bone, b.slot, b.points.clone()), (BoxKind::Hit, 2, 4, vec![[0.0, -3.0], [10.0, -3.0], [10.0, 3.0], [0.0, 3.0]]));
    // What is left out, said.
    assert!(made.notes.iter().any(|n| n.contains("inverse kinematics")) && made.notes.iter().any(|n| n.contains("draw-order keys in spin")), "{:?}", made.notes);
}

#[test]
fn clips_keep_their_keys_turns_unwound_and_eased_as_dragonbones_reads_them() {
    let made = import_skel("art/hero_ske.json", armature().to_string().as_bytes(), &read).unwrap();
    let idle = &made.skel.clips[0];
    assert_eq!((idle.name.as_str(), idle.frames, idle.repeat), ("idle", 10, 0));
    let arm = idle.bones.iter().find(|t| t.bone == 2).unwrap();
    // 0 → −170 → 170 goes the short way: on to −190, not back round.
    assert_eq!(arm.turned.iter().map(|k| (k.frame, k.value[0])).collect::<Vec<_>>(), [(0, 0.0), (5, -170.0), (10, -190.0)]);
    assert_eq!((arm.turned[0].ease.clone(), arm.turned[1].ease.clone()), (Ease::Linear, Ease::Step));
    let body = idle.bones.iter().find(|t| t.bone == 1).unwrap();
    assert_eq!((body.moved[1].value, body.moved[0].ease.clone()), ([0.0, -4.0], Ease::Out { amount: 1.0 }));
    let cape = &idle.slots[0];
    assert_eq!((cape.slot, cape.shown.clone()), (2, vec![(0, 0), (5, -1)]));
    assert_eq!(cape.color.iter().map(|k| k.color[3]).collect::<Vec<_>>(), [1.0, 0.0]);
    assert_eq!(idle.cues.iter().map(|c| (c.frame, c.name.as_str())).collect::<Vec<_>>(), [(3, "step")]);
    // Counter-clockwise to a quarter right: three quarters the other way.
    let spin = &made.skel.clips[1];
    assert_eq!(spin.bones[0].turned.iter().map(|k| k.value[0]).collect::<Vec<_>>(), [0.0, -270.0]);
    assert_eq!(spin.bones[0].turned[0].ease, Ease::In { amount: 0.5 });
    // Written and read back, the same.
    let back: Skel = proton_game::doc::parse(&made.path, std::str::from_utf8(&made.files[0].1).unwrap()).unwrap();
    assert_eq!(back, made.skel);
}

#[test]
fn a_skeleton_without_its_atlas_or_with_too_many_bones_is_refused() {
    let bytes = armature().to_string().into_bytes();
    assert!(import_skel("art/hero_ske.json", &bytes, &|_| None).unwrap_err().contains("atlas"));
    let mut many = armature();
    many["armature"][0]["bone"] = json!((0..70).map(|i| json!({ "name": format!("b{i}") })).collect::<Vec<_>>());
    assert!(import_skel("art/hero_ske.json", many.to_string().as_bytes(), &read).unwrap_err().contains("at most 64"));
}

#[test]
fn the_players_skeleton_plays_its_clips_by_the_tick() {
    let dir = std::env::temp_dir().join(format!("proton-skel-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    template::write(&dir, &template::files("metroidvania", "Skel").unwrap()).unwrap();
    let made = import_skel("art/hero_ske.json", armature().to_string().as_bytes(), &read).unwrap();
    for (rel, bytes) in &made.files {
        std::fs::write(dir.join(rel), bytes).unwrap();
    }
    let prefab = dir.join("prefabs/player.prefab.json");
    let mut text: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prefab).unwrap()).unwrap();
    text["sprite"] = json!("art/hero.skel.json");
    text["clips"] = json!({ "jump": "spin" });
    std::fs::write(&prefab, text.to_string()).unwrap();
    let world = World::cook(&Project::open(&dir).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    // Ten frames at 24 a second are 25 ticks at 60; four are 10.
    let a = &world.anims[0];
    assert_eq!((a.frames.len(), a.clips[0].from, a.clips[0].to, a.clips[1].from, a.clips[1].to), (35, 0, 24, 25, 34));
    assert_eq!((a.clips[0].repeat, a.clips[1].repeat, a.cues.clone(), world.cues.clone()), (0, 1, vec![(8, 0)], vec!["step".to_string()]));
    let mut sim = Sim::new(world, u32::MAX);
    let mut cues = 0;
    for _ in 0..60 {
        sim.step(0);
        cues += sim.happened().iter().filter(|h| matches!(h, Happened::Cue(0))).count();
    }
    // Standing: idle, round and round, its tick the frame.
    let still = sim.frame()[0];
    assert!(still.clip == Some(0) && still.frame.is_some_and(|f| f <= 24) && cues >= 2, "{still:?}, {cues} cues");
    // Rising plays the spin, once, and holds its last tick.
    for _ in 0..4 {
        sim.step(input::JUMP);
    }
    let up = sim.frame()[0];
    assert_eq!(up.clip, Some(1));
    for _ in 0..20 {
        sim.step(input::JUMP);
    }
    assert!(sim.state.anim.held || sim.state.anim.state != 2, "{:?}", sim.state.anim);
}
