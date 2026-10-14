//! Aseprite files read: frames and their times, layers in groups, cels raw,
//! compressed and linked, tags, and pixels in RGBA, grayscale or a palette's
//! indices — made here byte by byte as Aseprite's file format describes,
//! since no art of anyone's is kept in the tests. What Canvas cannot hold is
//! said. Slices and the text cels carry are kept for Game.

use proton_raster::ase_write::{cel, layer, linked, old_palette, palette, slice, tags, user_text, Ase};
use proton_raster::port::{self, Image};
use proton_raster::{aseprite, Blend, Direction, Preset};

fn rgba(n: usize, c: [u8; 4]) -> Vec<u8> {
    c.repeat(n)
}

const RED: [u8; 4] = [200, 30, 40, 255];
const SKY: [u8; 4] = [80, 150, 230, 255];

/// Three frames: a sky behind, a body in a group, a hidden group's layer,
/// a tilemap; cels raw, compressed and linked; two tags.
fn animated() -> Vec<u8> {
    Ase {
        width: 8,
        height: 6,
        depth: 32,
        transparent: 0,
        frames: vec![
            (
                100,
                vec![
                    layer(1 | 8, 0, 0, 0, 255, "Sky"),
                    layer(1, 1, 0, 0, 128, "Body"),
                    layer(1, 0, 1, 2, 255, "Torso"),
                    layer(0, 1, 0, 0, 255, "Ghosts"),
                    layer(1, 0, 1, 12, 255, "Boo"),
                    layer(1, 2, 0, 0, 255, "Tiles"),
                    layer(1 | 64, 0, 0, 0, 255, "Sketch"),
                    cel(0, 0, 0, 255, 8, 6, &rgba(48, SKY), true),
                    cel(2, 1, 1, 255, 2, 2, &rgba(4, RED), false),
                    cel(4, 0, 0, 255, 1, 1, &rgba(1, RED), true),
                    tags(&[(0, 1, 2, 0, "idle"), (1, 2, 3, 2, "jump")]),
                ],
            ),
            (60, vec![linked(0, 0), cel(2, 3, 2, 128, 2, 2, &rgba(4, RED), true)]),
            (250, vec![linked(0, 1), cel(2, 6, 4, 255, 4, 4, &rgba(16, RED), true)]),
        ],
    }
    .bytes()
}

#[test]
fn an_animation_reads_with_its_frames_layers_groups_and_tags() {
    let sprite = aseprite::read(&animated()).unwrap();
    assert_eq!((sprite.width, sprite.height, sprite.durations.clone()), (8, 6, vec![100, 60, 250]));
    let names: Vec<_> = sprite.layers.iter().map(|l| (l.name.as_str(), l.groups.clone())).collect();
    assert_eq!(names, [("Sky", vec![]), ("Torso", vec!["Body".to_string()]), ("Boo", vec!["Ghosts".to_string()])]);
    // A group's opacity is laid onto its layers; a hidden group hides them.
    let (sky, torso, boo) = (&sprite.layers[0], &sprite.layers[1], &sprite.layers[2]);
    assert!(sky.background && sky.visible);
    assert_eq!((torso.opacity, torso.blend, torso.visible), (128, 2, true));
    assert!(!boo.visible);
    // A linked cel is the cel it links to; a cel's opacity is in its alpha.
    assert_eq!(sky.cels[1], sky.cels[0]);
    assert_eq!(sky.cels[2], sky.cels[0]);
    assert_eq!(torso.cels[0], Some(Image { left: 1, top: 1, width: 2, height: 2, rgba: rgba(4, RED) }));
    assert_eq!(torso.cels[1].as_ref().unwrap().rgba[..4], [200, 30, 40, 128]);
    assert_eq!(boo.cels[1], None);
    assert_eq!(sprite.tags.iter().map(|t| (t.name.as_str(), t.from, t.to, t.direction, t.repeat)).collect::<Vec<_>>(), [("idle", 0, 1, Direction::PingPong, 0), ("jump", 1, 2, Direction::PingPongReverse, 2)]);
    for said in ["Body's opacity", "Tiles is a tilemap", "Sketch is a reference"] {
        assert!(sprite.notes.iter().any(|n| n.contains(said.split(' ').next().unwrap()) && n.contains(said.split(' ').next_back().unwrap())), "{said}: {:?}", sprite.notes);
    }
}

#[test]
fn an_animation_comes_in_as_a_canvas_of_frames() {
    let picture = port::read(&animated(), "unused").unwrap();
    assert!(picture.notes.iter().any(|n| n.contains("Ghosts / Boo") && n.contains("hue")), "{:?}", picture.notes);
    let c = port::canvas_of(&picture, Preset::Pixel).unwrap();
    assert_eq!((c.frames.clone(), c.tags.len()), (vec![100, 60, 250], 2));
    let names: Vec<_> = c.layers.iter().map(|l| (l.info.name.as_str(), l.info.blend, l.info.hidden)).collect();
    assert_eq!(names, [("Sky", Blend::Normal, false), ("Body / Torso", Blend::Screen, false), ("Ghosts / Boo", Blend::Normal, true)]);
    let torso = &c.layers[1];
    assert_eq!((torso.cels[0].pixel(1, 1), torso.cels[0].pixel(3, 3)), (RED, [0; 4]));
    assert_eq!(torso.cels[1].pixel(4, 3), [200, 30, 40, 128]);
    // Past the sprite's edge is left out.
    assert_eq!((torso.cels[2].pixel(7, 5), torso.cels[2].pixel(6, 4)), (RED, RED));
    assert_eq!(c.layers[0].cels[2].pixel(7, 5), SKY);
}

#[test]
fn indexed_and_grayscale_sprites_read_as_their_colours() {
    let colors = [[0, 0, 0, 255], [250, 200, 20, 255], [10, 120, 60, 255]];
    let indexed = Ase {
        width: 4,
        height: 1,
        depth: 8,
        transparent: 0,
        frames: vec![(80, vec![palette(&colors), layer(1 | 8, 0, 0, 0, 255, "Back"), layer(1, 0, 0, 0, 255, "Ink"), cel(0, 0, 0, 255, 4, 1, &[0, 1, 2, 0], true), cel(1, 0, 0, 255, 4, 1, &[1, 0, 2, 0], false)])],
    };
    let sprite = aseprite::read(&indexed.bytes()).unwrap();
    assert_eq!(sprite.palette, colors);
    // Index 0 is clear on every layer but the background.
    let back = sprite.layers[0].cels[0].as_ref().unwrap();
    assert_eq!(back.rgba[..4], [0, 0, 0, 255]);
    let ink = sprite.layers[1].cels[0].as_ref().unwrap();
    assert_eq!(ink.rgba, [[250, 200, 20, 255], [0; 4], [10, 120, 60, 255], [0; 4]].concat());
    // A pixel canvas keeps the file's palette.
    let c = port::canvas_of(&port::read(&indexed.bytes(), "x").unwrap(), Preset::Pixel).unwrap();
    assert_eq!(c.palette, colors);

    // An older file: its palette in the old chunk.
    let old = Ase { frames: vec![(80, vec![old_palette(&[[1, 2, 3], [9, 8, 7]]), layer(1, 0, 0, 0, 255, "Ink"), cel(0, 0, 0, 255, 2, 1, &[1, 0], false)])], ..indexed };
    let sprite = aseprite::read(&old.bytes()).unwrap();
    assert_eq!(sprite.palette, [[1, 2, 3, 255], [9, 8, 7, 255]]);
    assert_eq!(sprite.layers[0].cels[0].as_ref().unwrap().rgba, [[9, 8, 7, 255], [0; 4]].concat());

    let gray = Ase { width: 2, height: 1, depth: 16, transparent: 0, frames: vec![(80, vec![layer(1, 0, 0, 0, 255, "Shade"), cel(0, 0, 0, 255, 2, 1, &[90, 255, 200, 64], true)])] };
    let sprite = aseprite::read(&gray.bytes()).unwrap();
    assert_eq!(sprite.layers[0].cels[0].as_ref().unwrap().rgba, [90, 90, 90, 255, 200, 200, 200, 64]);
}

#[test]
fn a_file_that_is_not_whole_is_refused_not_guessed() {
    let bytes = animated();
    for cut in [10, 130, 200, bytes.len() - 3] {
        assert!(aseprite::read(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    let mut wrong = bytes.clone();
    wrong[4] = 0;
    assert!(aseprite::read(&wrong).is_err());
    assert!(!port::is_aseprite(&wrong) && port::is_aseprite(&bytes));
    // A link to a frame not yet read.
    let ahead = Ase { width: 2, height: 2, depth: 32, transparent: 0, frames: vec![(80, vec![layer(1, 0, 0, 0, 255, "A"), linked(0, 1)])] };
    assert!(aseprite::read(&ahead.bytes()).unwrap_err().contains("links"));
    // A cel claiming more pixels than it has.
    let short = Ase { width: 2, height: 2, depth: 32, transparent: 0, frames: vec![(80, vec![layer(1, 0, 0, 0, 255, "A"), cel(0, 0, 0, 255, 2, 2, &rgba(2, RED), true)])] };
    assert!(aseprite::read(&short.bytes()).is_err());
}

#[test]
fn slices_and_the_text_cels_carry_are_kept() {
    let bytes = Ase {
        width: 16,
        height: 16,
        depth: 32,
        transparent: 0,
        frames: vec![
            (
                100,
                vec![
                    layer(1, 0, 0, 0, 255, "Body"),
                    cel(0, 0, 0, 255, 2, 2, &rgba(4, RED), false),
                    user_text("step"),
                    slice("hitbox", &[(0, [2, 3, 8, 6], None), (1, [4, 3, 8, 6], None)]),
                    user_text("hit"),
                    slice("feet", &[(0, [6, 12, 4, 4], Some([2, 4]))]),
                    tags(&[(0, 1, 0, 0, "walk")]),
                    user_text("ignored: a tag's"),
                ],
            ),
            (100, vec![cel(0, 1, 0, 255, 2, 2, &rgba(4, RED), true)]),
        ],
    }
    .bytes();
    let sprite = aseprite::read(&bytes).unwrap();
    assert_eq!(sprite.texts, [aseprite::CelText { layer: 0, frame: 0, text: "step".into() }]);
    assert_eq!(sprite.slices.len(), 2);
    let hit = &sprite.slices[0];
    assert_eq!((hit.name.as_str(), hit.text.as_deref(), hit.keys.len()), ("hitbox", Some("hit"), 2));
    assert_eq!((hit.keys[1].frame, hit.keys[1].rect, hit.keys[1].pivot), (1, [4, 3, 8, 6], None));
    assert_eq!(sprite.slices[1].keys[0].pivot, Some([2, 4]));
    assert_eq!(sprite.tags[0].name, "walk");
}
