//! Generators and filters: the colour tables are their formulas; a ramp
//! rises in lightness; a sprite is its own mirror and outlined; noise keeps
//! to its colours; a wrapped cave tiles; an outline is exactly the pixels
//! beside the shape; colours brought down are no more than asked; Scale2x
//! keeps a diagonal a diagonal — and a seed makes the pixels it always made.

use std::collections::BTreeSet;

use proton_raster::filter::Filter;
use proton_raster::gen::{self, color, noise, sprite, Generator, Mix};
use proton_raster::{Area, Canvas, History, Mode, Op, Preset};

const INK: [u8; 4] = [20, 20, 30, 255];

#[test]
fn the_colour_tables_are_their_formulas() {
    for c in 0..256usize {
        let v = c as f64 / 255.0;
        let l = if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        assert!((color::LIN[c] as f64 - l * 65535.0).abs() <= 1.0, "sRGB {c}");
    }
    for a in 0..1024 {
        let s = (a as f64 / 1024.0 * std::f64::consts::TAU).sin() * 32767.0;
        assert!((color::sin(a) as f64 - s).abs() <= 1.0, "sin {a}");
    }
}

#[test]
fn a_ramp_rises_in_lightness_and_keeps_its_colour() {
    for base in [[200, 40, 60, 255], [40, 120, 220, 255], [90, 170, 70, 255], [128, 128, 128, 255], [250, 230, 80, 255]] {
        for steps in [3, 5, 8] {
            let ramp = color::ramp(base, steps, 25, 25000);
            assert_eq!(ramp.len(), steps as usize);
            let ls: Vec<i64> = ramp.iter().map(|&c| color::to_lab(c).l).collect();
            assert!(ls.windows(2).all(|w| w[0] < w[1]), "{base:?} in {steps}: {ls:?}");
        }
        // The middle of an odd ramp is near the colour itself — when the
        // colour is not so light or dark that the ramp is held from it.
        let l = color::to_lab(base).l;
        if (25000..42000).contains(&l) {
            let mid = color::ramp(base, 5, 0, 20000)[2];
            assert!((0..3).all(|i| (mid[i] as i32 - base[i] as i32).abs() <= 24), "{base:?} → {mid:?}");
        }
    }
}

/// A canvas and its one layer.
fn canvas(w: u32, h: u32, preset: Preset) -> (Canvas, u32) {
    let c = Canvas::new(w, h, preset).unwrap();
    let id = c.layers[0].info.id;
    (c, id)
}

#[test]
fn a_sprite_is_its_own_mirror_outlined_and_its_seeds_own() {
    for kind in [sprite::Kind::Ship, sprite::Kind::Creature, sprite::Kind::Item] {
        let (w, h, px) = sprite::make(kind, 11, None);
        for y in 0..h {
            for x in 0..w {
                assert_eq!(px[(y * w + x) as usize], px[(y * w + w - 1 - x) as usize], "{kind:?} at {x}, {y}");
            }
        }
        // Every filled pixel beside nothing is the outline's colour — the
        // ramp's darkest, as every outline pixel is.
        let at = |x: i64, y: i64| if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 { [0; 4] } else { px[(y * w as i64 + x) as usize] };
        let darkest = (0..w as i64).flat_map(|x| (0..h as i64).map(move |y| (x, y))).map(|(x, y)| at(x, y)).filter(|p| p[3] > 0).min_by_key(|p| color::to_lab(*p).l).unwrap();
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                if at(x, y)[3] > 0 {
                    let open = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| at(x + dx, y + dy)[3] == 0);
                    if open {
                        assert_eq!(at(x, y), darkest, "{kind:?}: an edge pixel at {x}, {y} is not outline");
                    }
                }
            }
        }
        assert_eq!(sprite::make(kind, 11, None), sprite::make(kind, 11, None));
        assert_ne!(sprite::make(kind, 11, None).2, sprite::make(kind, 12, None).2, "{kind:?}: two seeds, one sprite");
    }
}

#[test]
fn noise_keeps_to_its_colours_and_its_rectangle() {
    let (c, _) = canvas(64, 48, Preset::Pixel);
    let colors = vec![[10, 10, 40, 255], [40, 90, 160, 255], [120, 200, 230, 255]];
    for mix in [Mix::Nearest, Mix::Dither] {
        let made = gen::make(&c, &Generator::Noise { noise: noise::Kind::Gradient, seed: 3, cell: 12, octaves: 3, colors: colors.clone(), mix, dither: 4, rect: Some([8, 4, 40, 30]) }).unwrap();
        let mut seen = BTreeSet::new();
        for y in 0..48 {
            for x in 0..64 {
                let p = made.pixel(x, y);
                let inside = (8..48).contains(&x) && (4..34).contains(&y);
                assert_eq!(p[3] > 0, inside, "{mix:?} at {x}, {y}");
                if inside {
                    assert!(colors.contains(&p), "{mix:?}: {p:?} is not one of the colours");
                    seen.insert(p);
                }
            }
        }
        assert!(seen.len() >= 2, "{mix:?} used one colour");
    }
}

#[test]
fn a_wrapped_cave_tiles_and_one_not_wrapped_is_walled() {
    let cells = gen::cave::cells(9, 48, 4, 40, 30, true);
    let rock = cells.iter().filter(|&&r| r).count();
    assert!(rock > 200 && rock < 1000, "{rock} of 1200 rock");
    // Wrapped: the rule at the left edge saw the right edge, so laid twice
    // side by side the seam is like anywhere else — at least, not all rock.
    let seam = (0..30).filter(|&y| cells[y * 40] && cells[y * 40 + 39]).count();
    assert!(seam < 30);
    let walled = gen::cave::cells(9, 48, 4, 40, 30, false);
    let edge = (0..40).filter(|&x| walled[x]).count();
    assert!(edge > 30, "past the edge counts as rock: the top row is mostly rock ({edge} of 40)");
}

#[test]
fn an_outline_is_exactly_the_pixels_beside_the_shape() {
    let (mut c, id) = canvas(32, 32, Preset::Pixel);
    c.apply(&Op::Shape { layer: id, shape: proton_raster::ShapeKind::Ellipse, from: [6, 8], to: [24, 22], filled: true, tip: proton_raster::Tip { size: 1, round: false, mirror: [None, None] }, color: [200, 60, 60, 255] }).unwrap();
    let before = c.layers[0].grid.clone();
    c.apply(&Op::Filter { layer: id, filter: Filter::Outline { color: INK, diagonal: false, inside: false } }).unwrap();
    let solid = |x: i32, y: i32| x >= 0 && y >= 0 && x < 32 && y < 32 && before.pixel(x as u32, y as u32)[3] >= 128;
    for y in 0..32 {
        for x in 0..32 {
            let beside = !solid(x, y) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| solid(x + dx, y + dy));
            let now = c.layers[0].grid.pixel(x as u32, y as u32);
            assert_eq!(now == INK, beside, "at {x}, {y}");
        }
    }
}

#[test]
fn colours_brought_down_are_no_more_than_asked_and_kept_as_the_palette() {
    let (mut c, id) = canvas(64, 64, Preset::Pixel);
    c.apply(&Op::Generate { layer: id, generator: Generator::Noise { noise: noise::Kind::Value, seed: 1, cell: 16, octaves: 4, colors: vec![[255, 0, 0, 255], [0, 0, 255, 255]], mix: Mix::Smooth, dither: 4, rect: None } }).unwrap();
    let colours = |c: &Canvas| (0..64).flat_map(|y| (0..64).map(move |x| (x, y))).map(|(x, y)| c.layers[0].grid.pixel(x, y)).collect::<BTreeSet<_>>();
    assert!(colours(&c).len() > 20, "the smooth noise has few colours to start with");
    let mut h = History::new(Preset::Pixel);
    let applied = h.apply(&mut c, &Op::Filter { layer: id, filter: Filter::Quantize { colors: 5, palette: false, dither: 0, keep: true } }, "colours").unwrap();
    assert!(colours(&c).len() <= 5);
    assert!(applied.meta && c.palette.len() == 5, "kept: {:?}", c.palette);
    let palette = c.palette.clone();
    h.undo(&mut c);
    assert!(c.palette.is_empty(), "one undo did not take the palette back too");
    // To the palette, dithered: only its colours.
    c.palette = palette.clone();
    c.apply(&Op::Filter { layer: id, filter: Filter::Quantize { colors: 0, palette: true, dither: 4, keep: false } }).unwrap();
    assert!(colours(&c).iter().all(|p| palette.contains(p)));
    let (mut empty, id) = canvas(8, 8, Preset::Pixel);
    assert!(empty.apply(&Op::Filter { layer: id, filter: Filter::Quantize { colors: 0, palette: true, dither: 0, keep: false } }).is_err(), "no palette to bring them to");
}

#[test]
fn scale2x_keeps_a_diagonal_a_diagonal_and_the_canvas_grows() {
    let (mut c, id) = canvas(16, 16, Preset::Pixel);
    for i in 2..12 {
        c.apply(&Op::FillRect { layer: id, rect: [i, i, 1, 1], color: INK }).unwrap();
    }
    c.apply(&Op::Select { area: Area::Rect { rect: [0, 0, 4, 4] }, mode: Mode::Replace, feather: 0, smooth: false }).unwrap();
    c.apply(&Op::Upscale { factor: 2 }).unwrap();
    assert_eq!((c.width, c.height), (32, 32));
    // A one-pixel diagonal doubled is a two-pixel-wide diagonal, not stairs
    // of 2 × 2 blocks: the corners between steps are filled.
    let g = &c.layers[0].grid;
    assert_eq!(g.pixel(10, 10), INK);
    assert_eq!(g.pixel(11, 10), INK, "Scale2x left a stair");
    assert_eq!(g.pixel(13, 10)[3], 0);
    assert_eq!(c.selection.pixel(7, 7)[3], 255, "the selection grows with it");
    c.apply(&Op::Upscale { factor: 3 }).unwrap();
    assert_eq!((c.width, c.height), (96, 96));
    assert!(c.apply(&Op::Upscale { factor: 4 }).is_err());
    let (mut big, _) = canvas(600, 600, Preset::Pixel);
    assert!(big.apply(&Op::Upscale { factor: 2 }).is_err(), "past a pixel canvas's most");
}

#[test]
fn a_shadow_falls_under_and_pixelate_makes_squares() {
    let (mut c, id) = canvas(32, 32, Preset::Paint);
    c.apply(&Op::FillRect { layer: id, rect: [8, 8, 8, 8], color: [250, 250, 250, 255] }).unwrap();
    c.apply(&Op::Filter { layer: id, filter: Filter::Shadow { dx: 3, dy: 2, color: [0, 0, 0, 160], blur: 0 } }).unwrap();
    let g = &c.layers[0].grid;
    assert_eq!(g.pixel(10, 10), [250, 250, 250, 255], "the shadow went over");
    assert_eq!(g.pixel(17, 16), [0, 0, 0, 160]);
    assert_eq!(g.pixel(7, 7)[3], 0);
    c.apply(&Op::Filter { layer: id, filter: Filter::Pixelate { size: 4 } }).unwrap();
    let g = &c.layers[0].grid;
    for by in 0..8 {
        for bx in 0..8 {
            let first = g.pixel(bx * 4, by * 4);
            assert!((0..4).all(|y| (0..4).all(|x| g.pixel(bx * 4 + x, by * 4 + y) == first)), "block {bx}, {by}");
        }
    }
}

#[test]
fn a_generator_keeps_to_the_selection() {
    let (mut c, id) = canvas(40, 40, Preset::Pixel);
    c.apply(&Op::Select { area: Area::Rect { rect: [0, 0, 20, 40] }, mode: Mode::Replace, feather: 0, smooth: false }).unwrap();
    c.apply(&Op::Generate { layer: id, generator: Generator::Cave { seed: 2, density: 100, steps: 0, cell: 1, wrap: false, color: INK, floor: None, rect: Some([0, 0, 40, 40]) } }).unwrap();
    assert_eq!((c.layers[0].grid.pixel(5, 5), c.layers[0].grid.pixel(30, 5)), (INK, [0; 4]));
    assert!(c.apply(&Op::Generate { layer: id, generator: Generator::Noise { noise: noise::Kind::Value, seed: 1, cell: 4, octaves: 1, colors: vec![INK], mix: Mix::Nearest, dither: 4, rect: None } }).is_err(), "noise in one colour");
}

/// Every generator and filter, from seeds, as one number: a change to how
/// any of them makes pixels changes it, and must be meant. `PROTON_BLESS=1`
/// prints the new one.
#[test]
fn seeds_make_the_pixels_they_always_made() {
    let (mut c, id) = canvas(128, 96, Preset::Pixel);
    c.palette = color::ramp([60, 140, 200, 255], 6, 30, 26000);
    let ops = [
        Op::Generate { layer: id, generator: Generator::Noise { noise: noise::Kind::Gradient, seed: 42, cell: 24, octaves: 4, colors: c.palette.clone(), mix: Mix::Dither, dither: 8, rect: None } },
        Op::Generate { layer: id, generator: Generator::Cave { seed: 5, density: 46, steps: 5, cell: 2, wrap: true, color: [30, 24, 40, 255], floor: None, rect: Some([0, 48, 128, 48]) } },
        Op::Generate { layer: id, generator: Generator::Sprite { template: sprite::Kind::Ship, seed: 77, at: [10, 10], scale: 2, color: None } },
        Op::Generate { layer: id, generator: Generator::Sprite { template: sprite::Kind::Creature, seed: 78, at: [50, 12], scale: 3, color: Some([180, 90, 200, 255]) } },
        Op::Generate { layer: id, generator: Generator::Sprite { template: sprite::Kind::Item, seed: 79, at: [100, 20], scale: 1, color: None } },
        Op::Filter { layer: id, filter: Filter::Outline { color: [0, 0, 0, 255], diagonal: true, inside: true } },
        Op::Filter { layer: id, filter: Filter::Shadow { dx: 2, dy: 2, color: [0, 0, 0, 120], blur: 3 } },
        Op::Filter { layer: id, filter: Filter::Quantize { colors: 12, palette: false, dither: 4, keep: true } },
        Op::Filter { layer: id, filter: Filter::Pixelate { size: 3 } },
        Op::Upscale { factor: 3 },
    ];
    for op in &ops {
        c.apply(op).unwrap();
    }
    let hash = format!("{:016x}", c.hash());
    if std::env::var("PROTON_BLESS").is_ok() {
        println!("seeds: {hash}");
        return;
    }
    assert_eq!(hash, "6277f8d14010ddec");
}
