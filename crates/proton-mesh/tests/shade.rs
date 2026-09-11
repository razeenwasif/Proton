//! What working out normals has to get right.
//!
//! Two shapes carry most of it: a cube, which must stay a cube, and a sphere
//! made by subdividing, which must come out round. Between them they catch
//! both ways smoothing goes wrong — too little, and too much.

use proton_mesh::{Mesh, Step, CREASE};

/// A unit cube, wound outward, six quads.
const CUBE: &str = "v -0.5 -0.5 -0.5\nv -0.5 -0.5 0.5\nv -0.5 0.5 -0.5\nv -0.5 0.5 0.5\n\
                    v 0.5 -0.5 -0.5\nv 0.5 -0.5 0.5\nv 0.5 0.5 -0.5\nv 0.5 0.5 0.5\n\
                    f 1 2 4 3\nf 5 7 8 6\nf 1 3 7 5\nf 2 6 8 4\nf 3 4 8 7\nf 1 5 6 2\n";

/// An octahedron, wound outward: the usual start for a sphere by subdivision.
const OCTAHEDRON: &str = "v 1 0 0\nv -1 0 0\nv 0 1 0\nv 0 -1 0\nv 0 0 1\nv 0 0 -1\n\
                          f 1 3 5\nf 3 2 5\nf 2 4 5\nf 4 1 5\n\
                          f 3 1 6\nf 2 3 6\nf 4 2 6\nf 1 4 6\n";

/// A quarter of a tube: an arc from straight up to straight across, a metre
/// long, with its first edge lying on the x = 0 plane — so mirroring it in x
/// makes half a tube with a seam along the top.
const QUARTER_TUBE: &str = "v 0 1 0\nv 0.5 0.8660254 0\nv 0.8660254 0.5 0\nv 1 0 0\n\
                            v 0 1 1\nv 0.5 0.8660254 1\nv 0.8660254 0.5 1\nv 1 0 1\n\
                            f 1 5 6 2\nf 2 6 7 3\nf 3 7 8 4\n";

fn mesh(text: &str) -> Mesh {
    Mesh::parse(text).unwrap()
}

fn at(list: &[f32], v: usize) -> [f32; 3] {
    [list[v * 3], list[v * 3 + 1], list[v * 3 + 2]]
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
}

#[test]
fn a_cube_keeps_its_edges() {
    // Its faces meet at 90°, well past the crease, so no corner is shared
    // between two sides: 6 faces of 4 corners each is 24 vertices, and every
    // normal points straight out of the face it belongs to.
    let out = mesh(CUBE).shade(CREASE);
    assert_eq!(out.vertices(), 24);
    assert_eq!(out.count(), 12);
    for v in 0..out.vertices() {
        let n = at(&out.normals, v);
        let axis = n.iter().filter(|c| c.abs() > 0.999).count();
        let rest = n.iter().filter(|c| c.abs() < 1e-4).count();
        assert_eq!((axis, rest), (1, 2), "vertex {v} is not flat: {n:?}");
    }
}

#[test]
fn every_normal_on_a_cube_faces_the_side_it_is_on() {
    // The same fact from the other direction, and the one that catches a
    // normal worked out with the winding backwards: it would still be
    // axis-aligned, and point into the cube.
    let out = mesh(CUBE).shade(CREASE);
    for v in 0..out.vertices() {
        let n = at(&out.normals, v);
        let p = at(&out.positions, v);
        let along: f32 = (0..3).map(|k| n[k] * p[k]).sum();
        assert!(along > 0.0, "vertex {v} at {p:?} faces inward: {n:?}");
    }
}

#[test]
fn smoothing_across_every_edge_is_what_turns_a_cube_into_a_lump() {
    // The reason there is a crease angle at all, shown by switching it off:
    // every corner becomes one vertex facing diagonally out.
    let out = mesh(CUBE).shade(180.0);
    assert_eq!(out.vertices(), 8);
    let third = 1.0 / 3f32.sqrt();
    for v in 0..8 {
        let p = at(&out.positions, v);
        let want = p.map(|c| c.signum() * third);
        assert!(
            near(at(&out.normals, v), want),
            "{:?} at {p:?}",
            at(&out.normals, v)
        );
    }
}

#[test]
fn a_subdivided_solid_is_smooth_and_faces_out() {
    // After three levels neighbouring faces are a few degrees apart, so no
    // vertex is split — the shaded mesh has exactly the vertices the
    // subdivision made — and every normal points away from the middle.
    //
    // Only "away", not "radially away". An octahedron subdivided by Loop
    // converges on a rounded octahedron and not on a sphere — this test first
    // assumed otherwise and was corrected by a vertex at radius 0.49 on a
    // shape whose corners started at 1 — so its true normals are not radial
    // and asserting they were would be asserting a wrong prior.
    let solid = mesh(OCTAHEDRON).apply(&[Step::Subdivide { levels: 3 }]);
    let out = solid.shade(CREASE);
    assert_eq!(
        out.vertices(),
        solid.vertices(),
        "a smooth surface was split"
    );
    for v in 0..out.vertices() {
        let n = at(&out.normals, v);
        let p = at(&out.positions, v);
        let along: f32 = (0..3).map(|k| n[k] * p[k]).sum();
        assert!(along > 0.0, "vertex {v} faces inward: {n:?} at {p:?}");
    }
}

#[test]
fn on_an_actual_sphere_every_normal_is_radial() {
    // Which is the one shape where the right answer is known exactly. The
    // subdivided octahedron's topology, with every vertex pushed out onto the
    // unit sphere: the true normal at each point is then the point itself.
    let mut sphere = mesh(OCTAHEDRON).apply(&[Step::Subdivide { levels: 3 }]);
    for v in 0..sphere.vertices() {
        let p = at(&sphere.positions, v);
        let len = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        for (k, c) in p.iter().enumerate() {
            sphere.positions[v * 3 + k] = c / len;
        }
    }
    let out = sphere.shade(CREASE);
    assert_eq!(out.vertices(), sphere.vertices());
    let mut worst = 1.0f32;
    for v in 0..out.vertices() {
        let n = at(&out.normals, v);
        let p = at(&out.positions, v);
        worst = worst.min((0..3).map(|k| n[k] * p[k]).sum());
    }
    // Facets eleven degrees across, averaged around each vertex: the error is
    // what is left after the ring cancels, and three degrees is generous.
    assert!(
        worst > 3f32.to_radians().cos(),
        "a normal is {:.2}° off radial",
        worst.acos().to_degrees()
    );
}

#[test]
fn every_normal_is_unit_length() {
    // A renderer lights with these directly. One that is 0.9 long makes its
    // corner a tenth darker, and it looks like a smudge nobody can explain.
    for text in [CUBE, OCTAHEDRON, QUARTER_TUBE] {
        let out = mesh(text)
            .apply(&[Step::Subdivide { levels: 2 }])
            .shade(CREASE);
        for v in 0..out.vertices() {
            let n = at(&out.normals, v);
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-4, "vertex {v} is {len} long");
        }
    }
}

#[test]
fn shading_moves_no_point_and_changes_no_triangle() {
    // Splitting a vertex makes copies of it; it must not move one. Every
    // triangle's corners are where they were before.
    let before = mesh(CUBE);
    let after = before.shade(CREASE);
    assert_eq!(after.count(), before.count());
    for t in 0..before.count() * 3 {
        assert_eq!(
            at(&after.positions, after.triangles[t] as usize),
            at(&before.positions, before.triangles[t] as usize),
            "corner {t} moved"
        );
    }
    assert_eq!(after.bounds, before.bounds);
}

#[test]
fn a_flat_sheet_faces_the_way_it_is_wound() {
    let quad = mesh("v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n").shade(CREASE);
    assert_eq!(quad.vertices(), 4, "a flat quad has nothing to split");
    for v in 0..4 {
        assert!(near(at(&quad.normals, v), [0.0, 0.0, 1.0]));
    }
}

#[test]
fn a_welded_mirror_seam_is_shaded_as_one_surface() {
    // Why the mirror's weld exists, seen from the shading side. Welded, the
    // vertices on the seam are shared, the faces either side of it are 30°
    // apart — inside the crease — and the seam faces straight up. Unwelded,
    // each side keeps its own copy of the seam and each copy leans its own
    // way, which renders as a line down the middle of a surface that is not
    // supposed to have one.
    let welded = mesh(QUARTER_TUBE)
        .apply(&[Step::Mirror {
            axis: 0,
            weld: true,
        }])
        .shade(CREASE);
    let loose = mesh(QUARTER_TUBE)
        .apply(&[Step::Mirror {
            axis: 0,
            weld: false,
        }])
        .shade(CREASE);

    let seam = |out: &Mesh| -> Vec<[f32; 3]> {
        (0..out.vertices())
            .filter(|&v| at(&out.positions, v)[0].abs() < 1e-6)
            .map(|v| at(&out.normals, v))
            .collect()
    };

    let joined = seam(&welded);
    assert_eq!(joined.len(), 2, "one seam vertex at each end of the tube");
    for n in &joined {
        assert!(near(*n, [0.0, 1.0, 0.0]), "the welded seam leans: {n:?}");
    }

    let split = seam(&loose);
    assert_eq!(split.len(), 4, "two copies of each seam vertex");
    let leans = split.iter().filter(|n| n[0].abs() > 0.1).count();
    assert_eq!(leans, 4, "every loose seam copy leans to its own side");
}

#[test]
fn shading_a_solid_leaves_it_a_solid() {
    // Splitting a vertex along a crease makes two copies of one point. Asking
    // whether the result is closed afterwards would find a hole along every
    // edge of the cube, and culling would be switched off for no reason.
    assert!(mesh(CUBE).shade(CREASE).closed);
}

#[test]
fn a_modifier_throws_away_normals_that_no_longer_describe_the_surface() {
    let shaded = mesh(CUBE).shade(CREASE);
    assert!(!shaded.normals.is_empty());
    let moved = shaded.apply(&[Step::Array {
        count: 2,
        offset: [2.0, 0.0, 0.0],
    }]);
    assert!(
        moved.normals.is_empty(),
        "the old normals would have described half the new mesh"
    );
}

#[test]
fn a_file_read_on_its_own_has_no_normals_until_shaded() {
    assert!(mesh(CUBE).normals.is_empty());
}
