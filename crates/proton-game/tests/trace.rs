//! Collision traced from art: outlines that enclose what is opaque, with as
//! few points as the shape needs.

use proton_game::trace::{area, outlines, Mask, Tracing};

/// A mask `w` by `h`, solid where `inside` says.
fn mask(w: usize, h: usize, inside: impl Fn(usize, usize) -> bool) -> Mask {
    let alpha = (0..w * h).map(|i| if inside(i % w, i / w) { 255 } else { 0 }).collect();
    Mask { width: w, height: h, alpha }
}

#[test]
fn a_rectangle_traces_to_its_four_corners_enclosing_its_area() {
    // As large as a slab of ground is: the trace may lose half a pixel along
    // a side, which is under a percent of anything worth tracing.
    let m = mask(300, 200, |x, y| (20..260).contains(&x) && (40..160).contains(&y));
    let rings = outlines(&m, &Tracing::default());
    assert_eq!(rings.len(), 1, "{rings:?}");
    let ring = &rings[0];
    assert!(ring.len() <= 8, "a rectangle took {} points", ring.len());
    let enclosed = area(ring).abs();
    assert!((enclosed - 240.0 * 120.0).abs() / (240.0 * 120.0) < 0.01, "encloses {enclosed}");
    for p in ring {
        assert!(p[0] >= 19.5 && p[0] <= 260.5 && p[1] >= 39.5 && p[1] <= 160.5, "a point strays: {p:?}");
    }
}

#[test]
fn a_disc_keeps_its_area_with_a_modest_number_of_points() {
    let m = mask(200, 200, |x, y| ((x as f64 - 100.0).powi(2) + (y as f64 - 100.0).powi(2)).sqrt() < 80.0);
    let rings = outlines(&m, &Tracing::default());
    assert_eq!(rings.len(), 1);
    let enclosed = area(&rings[0]).abs();
    let disc = std::f64::consts::PI * 80.0 * 80.0;
    assert!((enclosed - disc).abs() / disc < 0.02, "encloses {enclosed} of {disc}");
    assert!(rings[0].len() >= 8 && rings[0].len() <= 64, "{} points", rings[0].len());
}

#[test]
fn a_ring_traces_to_an_outside_and_a_hole_running_the_other_way() {
    let m = mask(120, 120, |x, y| {
        let r = ((x as f64 - 60.0).powi(2) + (y as f64 - 60.0).powi(2)).sqrt();
        (25.0..50.0).contains(&r)
    });
    let rings = outlines(&m, &Tracing::default());
    assert_eq!(rings.len(), 2, "{} rings", rings.len());
    let signs: Vec<bool> = rings.iter().map(|r| area(r) > 0.0).collect();
    assert_ne!(signs[0], signs[1], "the hole runs the same way as the outside");
}

#[test]
fn separate_blobs_trace_separately_and_specks_are_dropped() {
    let m = mask(100, 40, |x, y| ((5..35).contains(&x) && (5..35).contains(&y)) || ((60..95).contains(&x) && (5..35).contains(&y)) || (x == 48 && y == 20));
    let rings = outlines(&m, &Tracing::default());
    assert_eq!(rings.len(), 2, "{rings:?}");
}

#[test]
fn the_threshold_decides_what_counts_as_solid() {
    let alpha = (0..50 * 50).map(|i| if (i % 50) < 25 { 200 } else { 60 }).collect();
    let m = Mask { width: 50, height: 50, alpha };
    let strict = outlines(&m, &Tracing { threshold: 128, ..Tracing::default() });
    let loose = outlines(&m, &Tracing { threshold: 50, ..Tracing::default() });
    let near = |got: f64, want: f64| (got - want).abs() / want < 0.04;
    assert!(near(area(&strict[0]).abs(), 25.0 * 50.0), "{}", area(&strict[0]));
    assert!(near(area(&loose[0]).abs(), 50.0 * 50.0), "{}", area(&loose[0]));
}

#[test]
fn a_png_is_read_for_its_alpha() {
    let files = proton_game::template::files("metroidvania", "x").unwrap();
    let ground = files.iter().find(|f| f.rel == "art/ground.png").unwrap();
    let m = Mask::of_png(&ground.bytes).unwrap();
    assert_eq!((m.width, m.height), (512, 128));
    let rings = outlines(&m, &Tracing::default());
    assert_eq!(rings.len(), 1, "the slab is one piece");
    assert!(area(&rings[0]).abs() > 512.0 * 120.0);
}
