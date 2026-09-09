//! What a modifier stack has to get right.
//!
//! Each of the three is a well-defined function from one triangle mesh to
//! another, so each is checked against what it should produce — a count, a
//! position, a winding, a bounding box — rather than against whatever it
//! happened to produce the first time it ran.

use proton_mesh::{growth, Mesh, Step};

/// A single triangle in the x>0 half space, so mirroring it is easy to read.
const WING: &str = "v 1 0 0\nv 3 0 0\nv 1 2 0\nf 1 2 3\n";

/// A closed solid: two triangles back to back is not one, so a tetrahedron.
const SOLID: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\n\
                     f 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n";

fn mesh(text: &str) -> Mesh {
    Mesh::parse(text).unwrap()
}

fn point(mesh: &Mesh, v: usize) -> [f32; 3] {
    [
        mesh.positions[v * 3],
        mesh.positions[v * 3 + 1],
        mesh.positions[v * 3 + 2],
    ]
}

/* ── mirror ───────────────────────────────────────────────────────── */

#[test]
fn mirroring_reflects_across_the_plane_through_the_origin() {
    let out = mesh(WING).apply(&[Step::Mirror {
        axis: 0,
        weld: false,
    }]);
    assert_eq!(out.vertices(), 6, "three there and three reflected");
    assert_eq!(out.count(), 2);
    assert_eq!(point(&out, 3), [-1.0, 0.0, 0.0]);
    assert_eq!(point(&out, 4), [-3.0, 0.0, 0.0]);
    assert_eq!(point(&out, 5), [-1.0, 2.0, 0.0]);
    assert_eq!(out.bounds, [-3.0, 0.0, 0.0, 3.0, 2.0, 0.0]);
}

#[test]
fn the_mirrored_half_is_wound_the_other_way_round() {
    // A reflection reverses orientation. Copying the winding unchanged leaves
    // the whole mirrored half inside-out — invisible from outside once back
    // faces are culled, which is exactly how it goes unnoticed.
    let out = mesh(WING).apply(&[Step::Mirror {
        axis: 0,
        weld: false,
    }]);
    let normal = |t: usize| {
        let [a, b, c] = [0, 1, 2].map(|k| point(&out, out.triangles[t * 3 + k] as usize));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    // Both lie in the z plane, so a correct mirror leaves them facing the same
    // way; a copied winding would flip the second one's sign.
    assert!(normal(0)[2] > 0.0, "{:?}", normal(0));
    assert!(normal(1)[2] > 0.0, "the mirrored half is inside-out");
}

#[test]
fn welding_merges_the_vertices_sitting_on_the_plane() {
    // A triangle with one corner on x = 0. Without a weld the seam becomes two
    // coincident vertices, which read as two loose boundaries — and a
    // subdivide after them pulls the halves visibly apart.
    let on_plane = "v 0 0 0\nv 2 0 0\nv 0 2 0\nf 1 2 3\n";
    let welded = mesh(on_plane).apply(&[Step::Mirror {
        axis: 0,
        weld: true,
    }]);
    let loose = mesh(on_plane).apply(&[Step::Mirror {
        axis: 0,
        weld: false,
    }]);
    assert_eq!(welded.vertices(), 4, "the two on the plane are shared");
    assert_eq!(loose.vertices(), 6);
    assert_eq!(welded.count(), 2);
    // And the seam really is one surface now, not two sheets meeting.
    assert!(!loose.closed);
}

#[test]
fn mirroring_a_solid_leaves_it_closed() {
    let out = mesh(SOLID).apply(&[Step::Mirror {
        axis: 1,
        weld: false,
    }]);
    assert!(out.closed, "two solids are still solids");
    assert_eq!(out.count(), 8);
}

#[test]
fn each_axis_mirrors_its_own_coordinate_and_no_other() {
    for axis in 0..3 {
        let out = mesh("v 1 2 3\nv 4 2 3\nv 1 5 3\nf 1 2 3\n")
            .apply(&[Step::Mirror { axis, weld: false }]);
        let there = point(&out, 0);
        let back = point(&out, 3);
        for k in 0..3 {
            let want = match k == axis {
                true => -there[k],
                false => there[k],
            };
            assert_eq!(back[k], want, "axis {axis}, coordinate {k}");
        }
    }
}

/* ── array ────────────────────────────────────────────────────────── */

#[test]
fn an_array_is_copies_in_a_row() {
    let out = mesh(WING).apply(&[Step::Array {
        count: 3,
        offset: [5.0, 0.0, 0.0],
    }]);
    assert_eq!(out.count(), 3);
    assert_eq!(out.vertices(), 9);
    // The first copy is where the original was — the offset is between
    // copies, not applied to all of them.
    assert_eq!(point(&out, 0), [1.0, 0.0, 0.0]);
    assert_eq!(point(&out, 3), [6.0, 0.0, 0.0]);
    assert_eq!(point(&out, 6), [11.0, 0.0, 0.0]);
    assert_eq!(out.bounds, [1.0, 0.0, 0.0, 13.0, 2.0, 0.0]);
}

#[test]
fn every_copy_indexes_its_own_vertices() {
    // The mistake this catches: reusing the first copy's indices, which draws
    // the first copy three times over and leaves the rest as loose points.
    let out = mesh(WING).apply(&[Step::Array {
        count: 2,
        offset: [5.0, 0.0, 0.0],
    }]);
    assert_eq!(out.triangles, [0, 1, 2, 3, 4, 5]);
}

#[test]
fn an_array_of_one_changes_nothing() {
    let before = mesh(WING);
    let after = before.apply(&[Step::Array {
        count: 1,
        offset: [5.0, 0.0, 0.0],
    }]);
    assert_eq!(after.positions, before.positions);
    assert_eq!(after.triangles, before.triangles);
}

#[test]
fn an_array_of_none_is_read_as_one_rather_than_deleting_the_object() {
    // A count somebody is halfway through typing should not make their model
    // disappear.
    let out = mesh(WING).apply(&[Step::Array {
        count: 0,
        offset: [1.0, 0.0, 0.0],
    }]);
    assert_eq!(out.count(), 1);
}

/* ── subdivide ────────────────────────────────────────────────────── */

#[test]
fn each_level_turns_every_triangle_into_four() {
    let one = mesh(SOLID).apply(&[Step::Subdivide { levels: 1 }]);
    let two = mesh(SOLID).apply(&[Step::Subdivide { levels: 2 }]);
    assert_eq!(one.count(), 16);
    assert_eq!(two.count(), 64);
    // Six edges of a tetrahedron, each giving one new vertex.
    assert_eq!(one.vertices(), 4 + 6);
}

#[test]
fn subdividing_a_solid_leaves_it_closed() {
    // It adds no holes and closes none, and culling depends on the answer.
    assert!(mesh(SOLID).apply(&[Step::Subdivide { levels: 2 }]).closed);
}

#[test]
fn subdivision_smooths_rather_than_only_splitting() {
    // The point of Loop over a plain split: the shape changes. A tetrahedron
    // pulls in toward its middle as it rounds off.
    let before = mesh(SOLID);
    let after = before.apply(&[Step::Subdivide { levels: 1 }]);
    let span = |m: &Mesh| m.bounds[3] - m.bounds[0];
    assert!(
        span(&after) < span(&before),
        "the corners did not move: {:?} vs {:?}",
        after.bounds,
        before.bounds
    );
    // And a corner really moved off where it was.
    assert_ne!(point(&after, 1), point(&before, 1));
}

#[test]
fn a_flat_sheet_stays_flat() {
    // Every vertex of a plane is in the same plane, and the weights are an
    // average of coplanar points, so subdividing must not lift anything out of
    // it. A sign error in the boundary rule shows up here immediately.
    let quad = "v 0 0 0\nv 2 0 0\nv 2 2 0\nv 0 2 0\nf 1 2 3\nf 1 3 4\n";
    let out = mesh(quad).apply(&[Step::Subdivide { levels: 2 }]);
    for v in 0..out.vertices() {
        assert_eq!(point(&out, v)[2], 0.0, "vertex {v} left the plane");
    }
}

#[test]
fn a_boundary_vertex_moves_only_along_its_own_boundary() {
    // Loop's rim rule is a cubic B-spline refinement of the boundary: three
    // quarters of where the vertex was, and an eighth from each neighbour
    // *along the rim*. Treating a rim vertex as an interior one pulls it
    // toward the middle of the surface instead, and every plane, wall and
    // sheet then shrinks a little further with every level.
    //
    // For this square the first level puts the corner at exactly (0.25, 0.25):
    // 3/4 * (0, 0) + 1/8 * ((2, 0) + (0, 2)).
    let quad = "v 0 0 0\nv 2 0 0\nv 2 2 0\nv 0 2 0\nf 1 2 3\nf 1 3 4\n";
    let once = mesh(quad).apply(&[Step::Subdivide { levels: 1 }]);
    assert_eq!(point(&once, 0), [0.25, 0.25, 0.0]);

    // Rounding the corners off *is* that rule — a subdivided square is not a
    // square, here or in any other tool. What it must not do is keep creeping:
    // the rim settles on a curve rather than collapsing toward the middle.
    let span = |levels| {
        let out = mesh(quad).apply(&[Step::Subdivide { levels }]);
        out.bounds[3] - out.bounds[0]
    };
    let (four, six) = (span(4), span(6));
    assert!(four > 1.7, "the rim collapsed inward: {four}");
    assert!(
        (six - four).abs() < 0.01,
        "the rim is still creeping: {four} then {six}"
    );
}

/* ── the stack ────────────────────────────────────────────────────── */

#[test]
fn the_order_of_the_stack_is_the_order_it_runs_in() {
    // Mirror then array is two rows of pairs; array then mirror is one row
    // reflected into two. Different shapes, and the reason it is a list.
    let both = [
        Step::Mirror {
            axis: 0,
            weld: false,
        },
        // Offset along the axis being mirrored, or the two commute and the
        // test proves nothing: reflecting in x leaves a y offset untouched,
        // and both orders then give the same four triangles.
        Step::Array {
            count: 2,
            offset: [10.0, 0.0, 0.0],
        },
    ];
    let forward = mesh(WING).apply(&both);
    let backward = mesh(WING).apply(&[both[1], both[0]]);
    assert_eq!(
        forward.count(),
        backward.count(),
        "both make four triangles"
    );

    // Mirrored first, the resulting pair is copied ten along: it reaches from
    // -3 to 13. Copied first, the whole row is then reflected: -13 to 13.
    assert_eq!(forward.bounds, [-3.0, 0.0, 0.0, 13.0, 2.0, 0.0]);
    assert_eq!(backward.bounds, [-13.0, 0.0, 0.0, 13.0, 2.0, 0.0]);
}

#[test]
fn an_empty_stack_returns_the_mesh_it_was_given() {
    let before = mesh(SOLID);
    let after = before.apply(&[]);
    assert_eq!(after, before);
}

#[test]
fn how_much_a_stack_grows_is_known_before_it_runs() {
    // So a stack that would make ten million triangles can be refused with a
    // number in the message, rather than by running out of memory first.
    assert_eq!(growth(&[]), 1.0);
    assert_eq!(
        growth(&[Step::Mirror {
            axis: 0,
            weld: true
        }]),
        2.0
    );
    assert_eq!(
        growth(&[Step::Array {
            count: 7,
            offset: [1.0; 3]
        }]),
        7.0
    );
    assert_eq!(growth(&[Step::Subdivide { levels: 3 }]), 64.0);

    // And the prediction is exact, which is the only reason it is worth
    // checking a stack against a budget without running it.
    let stack = [
        Step::Mirror {
            axis: 0,
            weld: false,
        },
        Step::Subdivide { levels: 2 },
        Step::Array {
            count: 3,
            offset: [1.0, 0.0, 0.0],
        },
    ];
    let out = mesh(SOLID).apply(&stack);
    assert_eq!(
        out.count() as f64,
        mesh(SOLID).count() as f64 * growth(&stack)
    );
}

#[test]
fn a_stack_does_not_touch_the_file_it_was_read_from() {
    // The whole idea of non-destructive. Turning a modifier off has to give
    // back exactly what is on disk, which it can only do if the original was
    // never overwritten.
    let before = mesh(SOLID);
    let _ = before.apply(&[
        Step::Mirror {
            axis: 0,
            weld: true,
        },
        Step::Subdivide { levels: 2 },
    ]);
    assert_eq!(before, mesh(SOLID));
}
